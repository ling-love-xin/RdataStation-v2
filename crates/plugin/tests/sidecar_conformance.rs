//! sidecar 一致性验收：**同一套检查，跑任何一个 sidecar 二进制**
//!
//! P1 的退出标准是「能连 → 能查 3000 行（Arrow 到宿主）→ 能取消 → 宿主退出无孤儿进程」。
//! 这个文件把那四条做成了可反复执行的检查：
//!
//! ```sh
//! # ① 先跑本仓的靶子（不用任何外部依赖）
//! cargo test -p rds-plugin --test sidecar_conformance
//!
//! # ② 再跑你自己的 sidecar（Go / Python / 任何语言）
//! RDS_SIDECAR_BIN=/path/to/your-sidecar \
//! RDS_SIDECAR_DRIVER=postgres \
//! RDS_SIDECAR_PARAMS='{"host":"127.0.0.1","port":5432,"database":"demo","username":"me","password":"…"}' \
//! RDS_SIDECAR_SQL_BIG='select * from generate_series(1, 3000)' \
//! RDS_SIDECAR_SQL_SLOW='select pg_sleep(5)' \
//! cargo test -p rds-plugin --test sidecar_conformance -- --nocapture
//! ```
//!
//! # 环境变量
//!
//! | 变量 | 默认 | 说明 |
//! | --- | --- | --- |
//! | `RDS_SIDECAR_BIN` | 本仓靶子 | 被测的 sidecar 可执行文件 |
//! | `RDS_SIDECAR_DRIVER` | `fixture` | 它在 `contributes.drivers` 里声明的 id |
//! | `RDS_SIDECAR_PARAMS` | `{}` | `session.open` 的连接参数（JSON） |
//! | `RDS_SIDECAR_SQL_BIG` | `select rows=3000` | 大结果查询（**必须能出 ≥ 3000 行**） |
//! | `RDS_SIDECAR_EXPECT_ROWS` | `3000` | 上面那条查询应当回来的行数 |
//! | `RDS_SIDECAR_SQL_SLOW` | `select hold_ms=5000` | 慢查询（**必须能跑 ≥ 5 秒**） |
//! | `RDS_SIDECAR_SLOW_SECONDS` | `5` | 上面那条查询的时长（取消要在它结束之前生效） |
//!
//! # 检查项（也就是"你的 sidecar 要满足什么"）
//!
//! 1. **握手**：`initialize` 里报的 `protocol` 必须与宿主一致（不一致当场拒绝加载）。
//! 2. **描述**：`driver.describe` 给得出 `display_name` 与 `capabilities`。
//! 3. **3000 行**：大结果必须走 **Arrow 附件**（> 200 行还内联就违反 §4.2.1.2），
//!    行数对得上，且在宿主侧解得成 `RecordBatch`（承载方式对消费方不可见）。
//! 4. **取消**：`query.cancel` 之后那条在跑的查询要以 `-32004` 收场，而且要**真的停**。
//! 5. **无孤儿**：宿主关掉 stdin 之后对端必须自己退（§4.2.1 的协议级约定）——
//!    这条不满足的话，宿主崩溃时会在用户机器上留下后台进程。
//! 6. **导航面**（P2）：`meta.catalogs` / `meta.schemas` / `meta.objects` / `meta.object_detail` /
//!    `meta.routine_source` 要答得出来，而且形状对（第一层不能空、表要说得出列与原始类型、
//!    例程源码是字符串或 `null`）。库是空的也算过 —— 那时只验「方法答得出来」，并打印说明。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;

use rds_plugin::manifest::BackendCommand;
use rds_plugin::sidecar::driver::{QueryRequest, SessionDriver};
use rds_plugin::sidecar::lifecycle::{Concurrency, ProcessSpec};
use rds_plugin::sidecar::meta::MetaObjectKind;
use rds_plugin::sidecar::process::{ReapOutcome, SidecarProcess, SpawnSpec};
use rds_plugin::sidecar::supervisor::{Deployment, SessionOpened, SidecarSupervisor};

/// 被测 sidecar 的二进制（默认本仓靶子）。
fn binary() -> PathBuf {
    match std::env::var("RDS_SIDECAR_BIN") {
        Ok(path) if !path.is_empty() => PathBuf::from(path),
        _ => PathBuf::from(env!("CARGO_BIN_EXE_rds-sidecar-fixture")),
    }
}

fn driver_id() -> String {
    std::env::var("RDS_SIDECAR_DRIVER").unwrap_or_else(|_| "fixture".to_string())
}

fn connection_params() -> serde_json::Value {
    match std::env::var("RDS_SIDECAR_PARAMS") {
        Ok(raw) if !raw.trim().is_empty() => {
            serde_json::from_str(&raw).expect("RDS_SIDECAR_PARAMS 应当是合法 JSON")
        }
        _ => json!({}),
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn expect_rows() -> usize {
    env_or("RDS_SIDECAR_EXPECT_ROWS", "3000")
        .parse()
        .expect("RDS_SIDECAR_EXPECT_ROWS 应当是数字")
}

fn slow_seconds() -> u64 {
    env_or("RDS_SIDECAR_SLOW_SECONDS", "5")
        .parse()
        .expect("RDS_SIDECAR_SLOW_SECONDS 应当是数字")
}

fn is_fixture() -> bool {
    std::env::var("RDS_SIDECAR_BIN").is_err()
}

/// 起一个进程 + 会话（"连上"那一步）。
async fn connect(plugin_id: &str) -> (SidecarSupervisor, String) {
    let mut supervisor = SidecarSupervisor::new();
    supervisor
        .deploy(Deployment {
            plugin_id: plugin_id.to_string(),
            spec: ProcessSpec::new([driver_id()], 1, Concurrency::Serial),
            command: BackendCommand {
                program: binary(),
                args: Vec::new(),
            },
            env: Vec::new(),
            protocol: None,
        })
        .unwrap_or_else(|e| panic!("部署失败（{}）：{e}", binary().display()));

    let opened = supervisor
        .open_session(
            plugin_id,
            &driver_id(),
            "s1",
            connection_params(),
            Instant::now(),
        )
        .await
        .unwrap_or_else(|e| panic!("握手/开会话失败：{e}"));
    assert!(
        matches!(opened, SessionOpened::Live { .. }),
        "会话应当直接开好：{opened:?}"
    );
    (supervisor, "s1".to_string())
}

/// ①+② 握手与描述。
#[tokio::test]
async fn greets_and_describes_itself() {
    let (supervisor, session_id) = connect("conformance.greet").await;
    let driver = SessionDriver::new(
        supervisor
            .session_conn(&session_id)
            .expect("会话应当有连接"),
        &session_id,
    );

    let descriptor = driver
        .describe(&driver_id())
        .await
        .expect("driver.describe 应当成功");
    assert!(
        !descriptor.display_name.is_empty(),
        "驱动要给得出显示名（下拉与属性面板要用）"
    );
    assert!(
        !descriptor.capabilities.is_empty(),
        "驱动要说得出自己的能力（没说的一律按不支持）"
    );
    println!(
        "  描述：{}（{}），能力 {} 项，服务端版本 {:?}",
        descriptor.display_name,
        descriptor.driver_id,
        descriptor.capabilities.len(),
        descriptor.server_version
    );

    driver.session_ping().await.expect("会话探活应当成功");

    let mut supervisor = supervisor;
    supervisor
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// ③ 大结果必须走 Arrow，行数对得上，且解得出 `RecordBatch`。
#[tokio::test]
async fn a_big_result_arrives_as_arrow_batches() {
    let (mut supervisor, session_id) = connect("conformance.arrow").await;
    let sql = env_or("RDS_SIDECAR_SQL_BIG", "select rows=3000");
    let expected = expect_rows();

    let page = {
        let driver = SessionDriver::new(
            supervisor
                .session_conn(&session_id)
                .expect("会话应当有连接"),
            &session_id,
        );
        driver
            .execute(&QueryRequest::new("conformance-1", &sql).with_max_rows(expected as u64 + 10))
            .await
            .unwrap_or_else(|e| panic!("大结果查询失败：{e}"))
    };

    assert_eq!(
        page.rows_here(),
        expected,
        "行数应当对得上（列 {} 个）",
        page.columns.len()
    );
    assert!(
        page.data.batches().is_some(),
        "超过 200 行的结果必须走 Arrow 附件（§4.2.1.2）；拿到的是内联 JSON"
    );
    let batches = page.data.batches().unwrap();
    assert!(
        !batches.is_empty() && batches[0].num_columns() > 0,
        "Arrow 批应当带 schema 且非空"
    );
    println!(
        "  {} 行，{} 列，Arrow {} 批（首列 {}）",
        expected,
        batches[0].num_columns(),
        batches.len(),
        batches[0].schema().field(0).name()
    );

    // 承载方式对消费方不可见：同一份数据经 `QueryResult` 也拿得到
    let result = page.into_query_result().expect("翻成引擎结果模型");
    assert_eq!(result.total_rows(), expected);
    assert_eq!(result.to_rows().len(), expected);

    supervisor
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// ④ 取消要真的停（而不是等它自己跑完）。
#[tokio::test]
async fn cancelling_stops_a_running_query() {
    let (mut supervisor, session_id) = connect("conformance.cancel").await;
    let sql = env_or("RDS_SIDECAR_SQL_SLOW", "select hold_ms=5000");
    let slow = slow_seconds();
    let request = QueryRequest::new("conformance-cancel", &sql);

    let started = Instant::now();
    let (result, ()) = {
        let driver = SessionDriver::new(
            supervisor
                .session_conn(&session_id)
                .expect("会话应当有连接"),
            &session_id,
        );
        tokio::join!(driver.execute(&request), async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            if let Err(e) = driver.cancel(&request.request_id).await {
                panic!("取消请求没递出去：{e}");
            }
        })
    };

    let error = result.expect_err("被取消的查询不该成功");
    assert!(
        error.is_cancelled(),
        "取消应当以 -32004 收场（拿到的是 {error:?}）"
    );
    assert!(
        started.elapsed() < Duration::from_secs(slow),
        "取消要真的停（用时 {:?}，而那条查询本来要跑 {slow}s）",
        started.elapsed()
    );
    println!("  取消用时 {:?}（原查询 {slow}s）", started.elapsed());

    supervisor
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// ⑤ 宿主关 stdin → 对端自己退（**无孤儿进程的协议级约定**，§4.2.1）。
#[tokio::test]
async fn the_sidecar_exits_when_stdin_closes() {
    let plugin_id = "conformance.orphan";
    // 走进程层（不经 supervisor）：这条检查要看的是"它自己退没退"的结局
    let spec = SpawnSpec::new(plugin_id, 0, binary());
    let process = SidecarProcess::spawn(spec).await.expect("起进程应当成功");
    process
        .conn()
        .initialize("rds-conformance", env!("CARGO_PKG_VERSION"))
        .await
        .expect("握手应当成功");

    // 收摊：丢连接（关 stdin）→ 等它自己退
    let outcome = process.retire(Duration::from_secs(15)).await;
    match outcome {
        ReapOutcome::Exited { .. } => {
            // 自己退的：正常路径（无论退出码，能自己退就说明没变成孤儿）
            println!(
                "  对端在 stdin 关闭后自己退出了（exit_code={:?}）",
                outcome.exit_code()
            );
        }
        ReapOutcome::Forced { pid, .. } => panic!(
            "对端没守「见 stdin EOF 即退」的约定，宿主只能强杀（pid={pid:?}）—— \
             这在用户机器上就是后台残留进程"
        ),
    }
}

/// ⑥ 导航面：五个 `meta.*` 答得出来，形状对得上（P2 的退出标准）。
///
/// 走的就是导航那条路：catalogs → schemas → objects →（挑一个表）detail →（有例程就叫一次源码）。
/// **空库也算过**：那时只验方法答得出来，并打印说明（空是合法状态，不是失败）。
#[tokio::test]
async fn answers_the_metadata_methods() {
    let (supervisor, session_id) = connect("conformance.meta").await;
    let driver = SessionDriver::new(
        supervisor
            .session_conn(&session_id)
            .expect("会话应当有连接"),
        &session_id,
    );
    let descriptor = driver.describe(&driver_id()).await.expect("描述应当成功");

    // ① 第一层不能是空的：拿不出 catalog 概念的库（MySQL 类）请把 schema 名当 catalog 回
    let catalogs = driver.meta_catalogs().await.expect("meta.catalogs");
    assert!(
        !catalogs.is_empty(),
        "导航第一层不能是空的：拿不出 catalog 概念的库，请把 schema 名当 catalog 回"
    );
    let catalog = catalogs[0].clone();
    println!("  catalog：{catalogs:?}");

    // ② schema 层：声明了就给得出来；没声明就是单层库，宿主会把 catalog 当 schema 传
    let schema = if descriptor.supports("schemas") {
        let schemas = driver.meta_schemas(&catalog).await.expect("meta.schemas");
        assert!(
            !schemas.is_empty(),
            "声明了 schemas 能力，却一个 schema 也给不出（导航会在这一层空掉）"
        );
        println!(
            "  schema：{:?}",
            schemas.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
        );
        schemas[0].name.clone()
    } else {
        println!("  驱动没声明 schemas：按单层库验（schema := catalog）");
        catalog.clone()
    };

    // ③ 对象清单：一次给全、含 kind
    let objects = driver
        .meta_objects(&catalog, &schema)
        .await
        .expect("meta.objects");
    let nav_able = objects.iter().filter(|o| o.nav_kind().is_some()).count();
    println!("  对象 {} 个（其中 {} 个摆得进导航）", objects.len(), nav_able);
    let unmappable: Vec<&str> = objects
        .iter()
        .filter(|o| matches!(o.kind, MetaObjectKind::Other(_)))
        .map(|o| o.name.as_str())
        .collect();
    if !unmappable.is_empty() {
        println!("  ⚠️ 报了导航摆不下的类别（宿主会跳过并记日志）：{unmappable:?}");
    }

    let Some(array) = objects.iter().find(|o| o.kind.is_table_like()) else {
        println!("  这个 schema 里没有表 / 视图：详情与源码两项跳过（空库是合法状态）");
        let mut supervisor = supervisor;
        supervisor
            .shutdown_all(Instant::now(), Duration::from_secs(10))
            .await;
        return;
    };

    // ④ 详情：对象身份要回声，列必须带原始类型（属性面板与类型归一化都靠它）
    let detail = driver
        .meta_object_detail(&catalog, &schema, &array.name)
        .await
        .expect("meta.object_detail");
    assert_eq!(detail.object.name, array.name, "详情要把对象身份回声回来");
    assert!(!detail.columns.is_empty(), "表要说得出列（空表也有列）");
    assert!(
        detail.columns.iter().all(|c| !c.type_raw.is_empty()),
        "每列都要有 type_raw（属性面板显示它、归一化类型由它推出）"
    );
    println!(
        "  {} 有 {} 列（首列 {} {}）；索引 {:?}；行数估计 {:?}",
        array.name,
        detail.columns.len(),
        detail.columns[0].name,
        detail.columns[0].type_raw,
        detail.indexes,
        detail.row_count
    );

    // ⑤ 例程源码：有例程就问一句 —— `null` 是合法答案，报错不是
    if let Some(routine) = objects.iter().find(|o| {
        matches!(o.kind, MetaObjectKind::Procedure | MetaObjectKind::Function)
    }) {
        let source = driver
            .meta_routine_source(&catalog, &schema, &routine.name)
            .await
            .expect("meta.routine_source");
        println!(
            "  例程 {} 的源码：{}",
            routine.name,
            if source.is_some() {
                "拿得到"
            } else {
                "对端说没有（合法）"
            }
        );
    }

    let mut supervisor = supervisor;
    supervisor
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// ⑦ 靶子自己的开关也顺便验一下：`--ignore-eof` 得能被强杀掉（宿主的兜底防线）。
///
/// 只在跑默认靶子时执行 —— 用户的 sidecar 不该有这种开关。
#[tokio::test]
async fn a_sidecar_that_ignores_eof_can_be_forced_down() {
    if !is_fixture() {
        println!("  跳过：RDS_SIDECAR_BIN 指定了外部 sidecar");
        return;
    }
    let spec = SpawnSpec::new("conformance.stubborn", 0, binary()).arg("--ignore-eof");
    let process = SidecarProcess::spawn(spec).await.expect("起进程");
    let outcome = process.retire(Duration::from_millis(300)).await;
    assert!(
        matches!(outcome, ReapOutcome::Forced { .. }),
        "不守约定的对端应当被强杀：{outcome:?}"
    );
}
