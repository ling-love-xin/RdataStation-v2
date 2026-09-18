//! L2（Oracle）作为联邦源：连接记录 → 会话级 Secret → 与 MySQL 一条 SQL 跨源（真机）
//!
//! 单独一个测试二进制：本文件启动时注入自己的临时全局库（`federation_sources.rs` 用同一单例，
//! 两个文件的用例放一起会互相看见对方的源，计数类断言就不稳了）。
//!
//! ```text
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! RDS_TEST_ORACLE_URL='oracle://devuser:Dev2026123@192.168.3.138:1521/XEPDB1' \
//! cargo test -p rds-workbench -j 2 --test oracle_federation -- --nocapture --test-threads=1
//! ```
//!
//! **sh / bash 下一律加单引号**（反斜杠会被吃掉，见 `editor_exec_real.rs` 的同一警告）。
//! 首次跑要能访问 DuckDB 社区扩展源（`INSTALL oracle_scanner FROM community`）。

use std::path::PathBuf;
use std::sync::OnceLock;

use connection::model::{ConnectionScope, DataSourceSaveInput};
use editor::channel::ExecChannel;
use editor::execution::RunOptions;
use engine::persistence::global_db::GlobalDatabaseManager;
use rds_workbench::services::data_source_service::DataSourceService;
use rds_workbench::services::editor_exec;

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 注入临时全局库到应用单例（进程内一次）
fn base_dir() -> PathBuf {
    BASE_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("rds_fed_oracle_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let manager = rt
                .block_on(GlobalDatabaseManager::new(
                    dir.join("global.db"),
                    dir.join("analytics.duckdb"),
                    2,
                ))
                .expect("init global db");
            engine::migration::install_global_db_manager(manager).expect("inject singleton");
            std::mem::forget(rt);
            dir
        })
        .clone()
}

/// 建一条**标记为联邦源**的连接记录
fn marked(name: &str, db_type: &str, url: &str) -> DataSourceSaveInput {
    let mut input = DataSourceSaveInput::new(name, db_type, url);
    input.scope = ConnectionScope::Global;
    input.use_duckdb_fed = Some(true);
    input
}

/// 真机台账（架构 §2.1）里 Oracle 的那几条都在这里落地：扩展从 **community** 装、凭据只能走
/// `CREATE SECRET`、`ATTACH` 不接受 `READ_ONLY`（所以写保护靠会话层 + 编辑器闸门）。
#[test]
fn an_oracle_record_joins_the_federation() {
    let (Ok(mysql_url), Ok(oracle_url)) = (
        std::env::var("RDS_TEST_MYSQL_URL"),
        std::env::var("RDS_TEST_ORACLE_URL"),
    ) else {
        eprintln!("⏭️ 未设 RDS_TEST_MYSQL_URL / RDS_TEST_ORACLE_URL，跳过 Oracle 联邦探针");
        return;
    };
    let _ = base_dir();
    engine::driver::AutoDriverRegistrar::register_builtin_drivers();
    let service = DataSourceService::global().expect("全局服务");
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    // 驱动 id 用应用里的叫法（`mysql_native` / `oracle`）；两条都**不建原生连接**
    let mysql_id = rt
        .block_on(service.save(
            &marked(
                "mysql_src",
                "mysql_native",
                &mysql_url.replace("mysql://", "mysql_native://"),
            ),
            None,
        ))
        .expect("保存 MySQL 连接");
    let oracle_id = rt
        .block_on(service.save(&marked("oracle_src", "oracle", &oracle_url), None))
        .expect("保存 Oracle 连接");
    let manager = engine::connection_manager::get_connection_manager();
    assert!(
        !rt.block_on(manager.has_connection(&oracle_id)),
        "本用例刻意不建原生连接：Oracle 由扩展的 scanner 直接读"
    );

    let runner = editor_exec::runner_for_test().expect("建执行器");
    let options = || RunOptions {
        use_transaction: false,
    };

    // ① 一条 SQL 同时读两边：MySQL 源（catalog）× Oracle 源（scanner 表函数）
    //
    // 走表函数路径是为了不依赖源库里有没有表（Oracle 的 `dual` 任何账号都读得到），
    // 探针因此不在客户库里建表；Secret 名是引擎侧定的（`rds_<别名>`，见 `registry::oracle_secret_from_url`）。
    let sql = "SELECT count(*) AS n FROM mysql_src.mysql.user u \
               JOIN oracle_query('rds_oracle_src', 'SELECT 1 AS one FROM dual') o ON 1 = 1";
    let data = runner
        .run(Some(&mysql_id), ExecChannel::Federated, sql, options())
        .expect("Oracle × MySQL 跨源查询该成功");
    assert_eq!(data.rows.len(), 1, "聚合该回一行：{:?}", data.rows);
    let count: i64 = data.rows[0][0].parse().expect("计数是数字");
    assert!(count > 0, "MySQL 侧该有行数：{:?}", data.rows);
    let notice = data.notice.clone().unwrap_or_default();
    assert!(
        notice.contains("oracle_src") && notice.contains("两段"),
        "小字要说明 L2 的限定名是两段（别名.表）：{notice}"
    );
    eprintln!("✅ Oracle × MySQL 跨源：{count} 行 · {notice}");

    // ② 挂载状态：L2 也是“已挂上”（凭据走 Secret 那一路）
    let snapshot = engine::duckdb::federation::session::snapshot_for(&mysql_id).expect("会话该在");
    let oracle = snapshot
        .sources
        .iter()
        .find(|entry| entry.alias() == "oracle_src")
        .expect("Oracle 该在源清单里");
    assert!(oracle.is_ready(), "Oracle 该挂上：{oracle:?}");
    assert!(
        oracle.source.kind.needs_secret(),
        "Oracle 的凭据走 Secret（它也没有 READ_ONLY：写保护靠会话层）"
    );
    eprintln!("ℹ️ 源清单：{snapshot:?}");

    // ③ 重挂（`DETACH` 之后要重建 Secret 再 `ATTACH`）
    let refreshed = runner
        .refresh_sources(Some(&mysql_id), ExecChannel::Federated, Some("oracle_src"))
        .expect("重挂 L2 源该成功");
    eprintln!("✅ 重挂：{refreshed}");

    // ④ 写 L2 源被引擎侧拦（它没有 `READ_ONLY` 那道闸）
    let refusal = runner
        .run(
            Some(&mysql_id),
            ExecChannel::Federated,
            "INSERT INTO oracle_src.SOME_TABLE VALUES (1)",
            options(),
        )
        .expect_err("写 L2 源该被拒");
    assert!(refusal.contains("不支持只读挂载"), "{refusal}");
    assert!(refusal.contains("oracle_src"), "要点名是哪个源：{refusal}");
    eprintln!("✅ 写 L2 源被拦：{refusal}");

    // ⑤ 本地临时对象照常（拒绝不能把分析能力一起挡掉）
    runner
        .run(
            Some(&mysql_id),
            ExecChannel::Federated,
            "CREATE TEMP TABLE scratch AS SELECT 1 AS n",
            options(),
        )
        .expect("本地临时表该能建");
    eprintln!("✅ 本地临时对象照常");

    let _ = oracle_id;
}
