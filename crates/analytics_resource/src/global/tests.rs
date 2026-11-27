use super::*;
use engine::persistence::ProjectSqlitePool;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

// 项目库建表 SQL 统一从 `crate::store::schema_sql` 取：此前这里手抄了一份迁移清单，
// 加一个迁移要改六处（漏一处只在某个用例里以 no such column 冒出来），见该模块注释。

struct TestEnv {
    dir: PathBuf,
    store: GlobalInstanceStore,
    project_pool: Arc<ProjectSqlitePool>,
    project_duckdb: Arc<AsyncMutex<Option<duckdb::Connection>>>,
    project_root: PathBuf,
    /// 同根连接的所有者；测试结束时随 `TestEnv` 一起释放。
    _root: Arc<shared::duckdb_root::DuckDbRoot>,
}

async fn create_test_env() -> TestEnv {
    let dir = crate::tests::temp_project_dir("gtest");
    let global_dir = dir.join("global");
    let project_dir = dir.join("project");
    fs::create_dir_all(&global_dir).expect("create global dir");
    fs::create_dir_all(&project_dir).expect("create project dir");
    fs::create_dir_all(project_dir.join("resources")).expect("create resources dir");

    // 同根改造：全局库与项目库必须先存在，再由一条 isolated 根连接同时挂上；
    // 两边各开独立连接在 M6 直连 CTAS 下会互相看不见。
    let global_duckdb_path = global_dir.join("analytics.duckdb");
    let project_duckdb_path = project_dir.join(".RSmeta/analytics.duckdb");
    fs::create_dir_all(project_duckdb_path.parent().unwrap()).expect("create rsmeta for duckdb");
    drop(duckdb::Connection::open(&global_duckdb_path).expect("create global duckdb"));
    drop(duckdb::Connection::open(&project_duckdb_path).expect("create project duckdb"));
    let root = shared::duckdb_root::isolated(
        Some(project_duckdb_path.as_path()),
        Some(global_duckdb_path.as_path()),
    )
    .expect("isolated duckdb root");

    let store = GlobalInstanceStore::open_at_handle(
        global_dir.join("global.db"),
        root.handle(shared::duckdb_root::GLOBAL_ALIAS)
            .expect("global handle"),
    )
    .expect("open global store");

    // 项目 SQLite（跑 020/023/024 迁移，与生产结构一致）。
    let db_path = project_dir.join(".RSmeta/project.db");
    fs::create_dir_all(db_path.parent().unwrap()).expect("create rsmeta");
    let pool = Arc::new(
        ProjectSqlitePool::new(db_path.clone(), 2)
            .await
            .expect("create project pool"),
    );
    {
        let conn = pool.acquire().await.expect("acquire");
        let inner = conn.inner().expect("inner");
        inner
            .execute_batch(crate::store::schema_sql::PROJECT_MIGRATIONS_SQL)
            .expect("run analytics_resources migrations");
    }

    // 项目 DuckDB 会话：与全局句柄同根，默认 catalog 指到 analytics。
    let project_duckdb = Arc::new(AsyncMutex::new(Some(
        root.handle(shared::duckdb_root::PROJECT_ALIAS)
            .expect("project handle"),
    )));

    TestEnv {
        dir,
        store,
        project_pool: pool,
        project_duckdb,
        project_root: project_dir,
        _root: root,
    }
}

fn write_csv(dir: &Path) -> PathBuf {
    let path = dir.join("orders.csv");
    fs::write(&path, "id,sku,qty\n1,a,10\n2,b,20\n3,c,30\n").expect("write csv");
    path
}

fn promote_req(name: &str, path: &Path, kind: PromoteKind) -> PromoteRequest {
    PromoteRequest {
        name: name.to_string(),
        kind,
        source_abs_path: path.to_path_buf(),
        // 数据文件分发 = 建项目表（无重建目标）；文件分发 = 重建 resources/<rel> 文件。
        rel_path: match kind {
            PromoteKind::DataFile => None,
            PromoteKind::File => Some(format!("work/{name}")),
        },
        content_hash: format!("hash_{}", Uuid::new_v4().simple()),
        file_size: Some(123),
        // 默认不记来源（这条 helper 只造请求）；要验链的用例自己填。
        source_resource_id: None,
        source_version: None,
        // 文件型请求：本体是文件，表型用例（g015）自己填表名。
        source_table: None,
        // 默认不裁（用例要看全部版本表）；裁剪用例自己填。
        keep_versions: None,
    }
}

/// 读全局最新一版的**实际表名**（G2：`analytics_table` 列，不是资产名）。
fn read_global_table(env: &TestEnv, name: &str) -> Option<String> {
    env.store
        .with_sqlite(|conn| {
            conn.query_row(
                "SELECT analytics_table FROM analytics_resources \
                 WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL \
                 ORDER BY version DESC LIMIT 1",
                rusqlite::params![name],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(sqlite_err("read_global_table"))
        })
        .expect("read global table")
}

#[tokio::test]
async fn g001_promote_data_file_and_register_global() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("orders_v1", &csv, PromoteKind::DataFile);

    let outcome = env.store.promote(&req).await.expect("promote");
    assert_eq!(outcome.version, 1, "首版从 1 开始");
    assert!(!outcome.overwritten);
    assert_eq!(outcome.row_count, Some(3), "CSV 3 行");
    assert_eq!(outcome.column_count, Some(3), "CSV 3 列");

    // 全局 DuckDB 表真实存在 + 行数一致：表名取自登记行的 `analytics_table`
    // （G2 起是 `res_<描述>_<时间戳>`，不再等于资产名）。
    let table = read_global_table(&env, "orders_v1").expect("新升级的行必须登记表名");
    assert!(table.starts_with("res_orders_v1_"), "一版一张表：{table}");
    let (rows, cols) = env.store.table_stats(&table).await.expect("global stats");
    assert_eq!(rows, Some(3), "全局表行数 = CSV 行数");
    assert_eq!(cols, Some(3));

    // 全局登记行（scope = 'global'，kind = 'analysis'）。
    let list = list_global_resources(&env.store).expect("list global");
    assert_eq!(list.len(), 1);
    let row = &list[0];
    assert_eq!(row.name, "orders_v1");
    assert_eq!(row.scope, "global");
    assert_eq!(row.kind, "analysis");
    assert_eq!(row.version, 1);
    assert_eq!(
        row.analytics_table.as_deref(),
        Some(table.as_str()),
        "登记行的表名要和建出来的那张表一致"
    );
    assert_eq!(
        row.structured,
        Some(1),
        "数据文件升级出来的全局表能参与分析"
    );
    assert!(row
        .definition_sql
        .as_deref()
        .unwrap_or("")
        .contains("read_csv_auto"));

    // 版本快照落全局库。
    env.store
        .with_sqlite(|conn| {
            let v: i32 = conn
                .query_row(
                    "SELECT count(*) FROM analytics_resource_versions WHERE resource_id = ?1",
                    rusqlite::params![row.id],
                    |r| r.get(0),
                )
                .map_err(sqlite_err("test_count_versions"))?;
            assert_eq!(v, 1);
            Ok(())
        })
        .expect("version snapshot");

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g002_promote_conflict_then_overwrite() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("orders_conf", &csv, PromoteKind::DataFile);
    env.store.promote(&req).await.expect("first promote");

    // 同名再升级 → Conflict。
    let err = env
        .store
        .promote(&req)
        .await
        .expect_err("second promote must conflict");
    assert!(
        matches!(err, InstanceOpError::Conflict { .. }),
        "同名升级应报 Conflict，实际 {err:?}"
    );

    // 覆盖式升级 → 版本 2。
    let out = env
        .store
        .promote_overwrite(&req)
        .await
        .expect("overwrite promote");
    assert_eq!(out.version, 2, "覆盖后版本接续旧链");
    assert!(out.overwritten);

    let list = list_global_resources(&env.store).expect("list");
    assert_eq!(list.len(), 1, "覆盖后仍一条活跃登记");
    assert_eq!(list[0].version, 2);

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g004_distribute_table_to_project() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("shared_orders", &csv, PromoteKind::DataFile);
    env.store.promote(&req).await.expect("promote");

    let project_conn = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let outcome = env
        .store
        .distribute(&project_conn, &env.project_root, "shared_orders", None)
        .await
        .expect("distribute");
    assert_eq!(outcome.shape, DistributeShape::Table);
    assert_eq!(outcome.version, 1, "项目侧首版");

    // 项目表真实存在 + 行数一致（跨库搬运成功）——经项目写连接验证。
    let (rows, _) = project_table_stats(&project_conn, "shared_orders")
        .await
        .expect("project stats");
    assert_eq!(rows, Some(3), "分发后项目表行数 = 全局表行数");

    // 项目登记（scope = 'project'，来源 = 全局）。
    let pool = &env.project_pool;
    let conn = pool.acquire().await.expect("acquire");
    let conn = conn.inner().expect("inner");
    let (scope, promoted_from, analytics_table, structured): (
        String,
        String,
        Option<String>,
        Option<i32>,
    ) = conn
        .query_row(
            "SELECT scope, promoted_from, analytics_table, structured FROM analytics_resources \
             WHERE name = 'shared_orders' AND deleted_at IS NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("query project row");
    assert_eq!(scope, "project");
    assert!(
        promoted_from.starts_with(crate::model::GLOBAL_PREFIX),
        "项目行记全局来源"
    );
    // B 端的本体是项目分析库里的那张表，表名必须登记：
    // 否则这一行会被读成「归档文件」，双击与查看统计都找不到本体。
    assert_eq!(analytics_table.as_deref(), Some("shared_orders"));
    assert_eq!(structured, Some(1), "分发出来的表能参与分析");

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g006_global_row_missing_returns_not_found() {
    let env = create_test_env().await;
    let project_conn = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let err = env
        .store
        .distribute(&project_conn, &env.project_root, "never_promoted", None)
        .await
        .expect_err("must not find");
    assert!(
        matches!(err, InstanceOpError::NotFound { .. }),
        "未登记的全局资源分发应报 NotFound，实际 {err:?}"
    );
    let _ = fs::remove_dir_all(&env.dir);
}

/// 冲突回执要说清**撞在哪一层**：三查（登记 / 表 / 文件）都可能命中，
/// 但用户接下来要做的事不一样，笼统一句"已有同名"帮不上判断。
///
/// 2026-11-16（G2）起升级侧只会命中登记层：一版一张表、表名带时间戳，
/// 「全局表已被同名对象占用」不再是一种可能，剩下的是「同名资产已经有一版」。
#[tokio::test]
async fn g007_conflict_reports_which_field_was_hit() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("orders_field", &csv, PromoteKind::DataFile);
    env.store.promote(&req).await.expect("first promote");

    // 升级侧：只有登记层会命中（表层不再是同名判据）。
    let err = env
        .store
        .promote(&req)
        .await
        .expect_err("second promote must conflict");
    match err {
        InstanceOpError::Conflict { field, .. } => {
            assert_eq!(field, ConflictField::Registry, "一版一张表后只剩登记层冲突");
        }
        other => panic!("应报 Conflict，实际 {other:?}"),
    }

    // 分发侧：项目登记里已有同名 → 只命中登记层（表在但表不是第一查）。
    let project_conn = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    env.store
        .distribute(&project_conn, &env.project_root, "orders_field", None)
        .await
        .expect("first distribute");
    let err = env
        .store
        .distribute(&project_conn, &env.project_root, "orders_field", None)
        .await
        .expect_err("second distribute must conflict");
    match err {
        InstanceOpError::Conflict { field, .. } => {
            assert_eq!(field, ConflictField::Registry, "登记层先命中");
        }
        other => panic!("应报 Conflict，实际 {other:?}"),
    }

    let _ = fs::remove_dir_all(&env.dir);
}

/// 覆盖 = 同名链的下一版，不是"从 v1 重开"。
///
/// 两端注册层都按同名（含已软删）的 `MAX(version) + 1` 取号；若某端漏掉这条，
/// 覆盖后版本会退回 1，用户看到的历史就断了（本用例专门钉住分发侧）。
#[tokio::test]
async fn g008_distribute_overwrite_keeps_the_version_chain() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("chain_orders", &csv, PromoteKind::DataFile);
    env.store.promote(&req).await.expect("promote");

    let project_conn = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let first = env
        .store
        .distribute(&project_conn, &env.project_root, "chain_orders", None)
        .await
        .expect("first distribute");
    assert_eq!(first.version, 1, "项目侧首版");

    let second = env
        .store
        .distribute_overwrite(&project_conn, &env.project_root, "chain_orders", None)
        .await
        .expect("overwrite distribute");
    assert_eq!(second.version, 2, "覆盖后版本号接着涨（不是退回 v1）");

    // 活跃登记仍只有一条，且它是 v2。
    let pool = &env.project_pool;
    let conn = pool.acquire().await.expect("acquire");
    let conn = conn.inner().expect("inner");
    let versions: Vec<i32> = {
        let mut stmt = conn
            .prepare(
                "SELECT version FROM analytics_resources \
                 WHERE name = 'chain_orders' AND scope = 'project' AND deleted_at IS NULL",
            )
            .expect("prepare");
        let rows = stmt.query_map([], |row| row.get(0)).expect("query");
        rows.map(|r| r.expect("row")).collect()
    };
    assert_eq!(versions, vec![2], "覆盖后只剩 v2 一条活跃登记");

    let _ = fs::remove_dir_all(&env.dir);
}

/// 来源链（A → 全局 → B）只记**直接上游**那一跳。
///
/// 各层版本各自编号：A 的 v5 升级成全局 v1，再下发成 B 的 v1。三行各记自己的
/// 版本号，每行只回指上一跳的 id + 版本——顺着走两跳就能从 B 找回 A 的那一版，
/// 而不用在每一行里冗余整条链（冗余会随上游新增版本而过期）。
#[tokio::test]
async fn g013_origin_chain_points_at_the_immediate_upstream() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let mut req = promote_req("chain_orders", &csv, PromoteKind::DataFile);
    req.source_resource_id = Some("arp_origin_probe".to_string());
    req.source_version = Some(5);
    let outcome = env.store.promote(&req).await.expect("promote");
    assert_eq!(outcome.version, 1, "全局侧从自己的 v1 开始编号");

    // 全局行回指 A 的那一版。
    let (kind, reference): (String, String) = env
        .store
        .with_sqlite(|conn| {
            conn.query_row(
                "SELECT origin_kind, origin_ref FROM analytics_resources \
                 WHERE name = 'chain_orders' AND scope = 'global' AND deleted_at IS NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(sqlite_err("read_global_origin"))
        })
        .expect("read global origin");
    assert_eq!(kind, crate::model::ORIGIN_KIND_PROJECT);
    assert_eq!(
        crate::model::describe(Some(&kind), Some(&reference)),
        "项目存档 arp_origin_probe 的 v5"
    );

    // 全局 v1 → B：B 行记全局那一跳，版本同样从 v1 起（不继承 A 的 v5）。
    let project_conn = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let dist = env
        .store
        .distribute(&project_conn, &env.project_root, "chain_orders", None)
        .await
        .expect("distribute");
    assert_eq!(dist.version, 1, "B 侧从自己的 v1 开始编号");
    let pool = &env.project_pool;
    let conn = pool.acquire().await.expect("acquire");
    let conn = conn.inner().expect("inner");
    let (kind, reference): (String, String) = conn
        .query_row(
            "SELECT origin_kind, origin_ref FROM analytics_resources \
             WHERE name = 'chain_orders' AND deleted_at IS NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read project origin");
    assert_eq!(kind, crate::model::ORIGIN_KIND_GLOBAL);
    assert_eq!(
        crate::model::describe(Some(&kind), Some(&reference)),
        format!("全局实例 {} 的 v1", outcome.resource_id)
    );

    let _ = fs::remove_dir_all(&env.dir);
}

/// G2：一版一张表——同名第二次升级另起一张表，上一版那张**保留**。
///
/// 这是「回到上一版看数据」的前提：登记行逐版记着各自的 `analytics_table`，
/// 表本身不删。覆盖式升级只软删旧登记，不碰旧表。
#[tokio::test]
async fn g014_each_global_version_keeps_its_own_table() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("versioned_orders", &csv, PromoteKind::DataFile);

    let first = env.store.promote(&req).await.expect("first promote");
    assert_eq!(first.version, 1);
    let first_table = read_global_table(&env, "versioned_orders").expect("first table");

    let second = env
        .store
        .promote_overwrite(&req)
        .await
        .expect("overwrite promote");
    assert_eq!(second.version, 2, "覆盖接续版本链，不从 v1 重开");
    assert!(second.overwritten);
    let second_table = read_global_table(&env, "versioned_orders").expect("second table");

    assert_ne!(first_table, second_table, "一版一张表：两版表名必须不同");
    assert!(
        env.store
            .table_exists(&first_table)
            .await
            .expect("probe first"),
        "旧版表不能被删——那是回看上一版的唯一凭据"
    );
    assert!(
        env.store
            .table_exists(&second_table)
            .await
            .expect("probe second"),
        "新版表要真的建出来"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

/// 表型升级（场景 3：联邦临时结果集 → 全局持久表）：本体是项目分析库里的一张表，
/// 升级必须**整表搬运**——采样一次，「全局 v1 和项目 A 的 v5 是同一份数据」就不成立了。
///
/// 顺带钉三件事：来源链指回 A 的 v5；表型行 `structured = 1`（能参与分析）；
/// 升级只读项目表，不改项目侧数据。
#[tokio::test]
async fn g015_table_source_promote_moves_every_row() {
    let env = create_test_env().await;
    let project = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    {
        let mut guard = project.0.lock().await;
        let conn = guard.as_mut().expect("project conn");
        conn.execute_batch(
            "CREATE TABLE daily AS \
             SELECT i AS id, i * 2 AS amount FROM range(1, 101) r(i);",
        )
        .expect("create project table");
    }

    let req = PromoteRequest {
        name: "daily_orders".to_string(),
        // 表型升级：不经文件，直接同根 CTAS。
        kind: PromoteKind::DataFile,
        source_abs_path: env.project_root.join(".RSmeta/analytics.duckdb"),
        rel_path: None,
        content_hash: "hash_daily".to_string(),
        file_size: None,
        source_resource_id: Some("arp_a_v5".to_string()),
        source_version: Some(5),
        source_table: Some("daily".to_string()),
        // 这一步只验搬运与来源链，不裁。
        keep_versions: None,
    };

    let outcome = env
        .store
        .promote_table_from_project(&project, &req, false)
        .await
        .expect("promote table");
    assert_eq!(outcome.version, 1, "A 的 v5 升级成全局 v1（各层各自编号）");
    assert_eq!(outcome.row_count, Some(100), "整表搬运，不采样");

    let table = read_global_table(&env, "daily_orders").expect("表名");
    let (rows, cols) = env
        .store
        .table_stats(&format!("rds_archive.{table}"))
        .await
        .expect("stats");
    assert_eq!(rows, Some(100), "全局表行数 = 项目表行数");
    assert_eq!(cols, Some(2));

    // 表型来源也是结构化（能参与分析），来源链只记直接上游那一跳。
    let list = list_global_resources(&env.store).expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].structured, Some(1));
    assert_eq!(list[0].origin_kind.as_deref(), Some("project"));
    assert!(
        list[0]
            .origin_ref
            .as_deref()
            .unwrap_or("")
            .contains("arp_a_v5"),
        "来源链要指回 A 的 v5：{:?}",
        list[0].origin_ref
    );

    // 升级是读，不是迁移：项目侧那张表还在、还是 100 行。
    let (rows, _) = project_table_stats(&project, "daily")
        .await
        .expect("project stats");
    assert_eq!(rows, Some(100), "升级不动项目侧数据");

    // 同名再升级：不覆盖时按登记冲突拒绝；覆盖时版本接续、旧表仍留库。
    let conflict = env
        .store
        .promote_table_from_project(&project, &req, false)
        .await;
    assert!(
        matches!(
            conflict,
            Err(InstanceOpError::Conflict {
                field: ConflictField::Registry,
                ..
            })
        ),
        "同名表型升级应按登记层冲突拒绝：{conflict:?}"
    );
    // 改一行再覆盖：内容变了才产生新快照；同内容重复升级会复用 v1。
    {
        let mut guard = project.0.lock().await;
        guard
            .as_mut()
            .expect("project conn")
            .execute_batch("INSERT INTO daily SELECT 101, 101;")
            .expect("add one row");
    }
    let overwritten = env
        .store
        .promote_table_from_project(&project, &req, true)
        .await
        .expect("overwrite table");
    assert_eq!(overwritten.version, 2, "内容变化后覆盖接续版本号");
    assert!(overwritten.overwritten, "覆盖结果要标记 overwritten");
    let versions = read_global_version_tables(&env, "daily_orders");
    assert_eq!(versions.len(), 2, "旧版本登记与表都保留");
    assert!(
        versions.iter().any(|(v, _)| *v == 1),
        "v1 登记不能被覆盖删除：{versions:?}"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

/// 表型历史版本恢复：新版本直接指向旧快照，零复制，不原地回滚。
#[tokio::test]
async fn g030_table_version_restore_reuses_the_old_snapshot() {
    let env = create_test_env().await;
    let project = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let make =
        |rows: i32| format!("CREATE TABLE daily AS SELECT i AS id FROM range(1, {rows}) r(i);");
    {
        let mut guard = project.0.lock().await;
        guard
            .as_mut()
            .expect("project conn")
            .execute_batch(&make(101))
            .expect("create 100");
    }
    let req = PromoteRequest {
        name: "restorable_orders".to_string(),
        kind: PromoteKind::DataFile,
        source_abs_path: env.project_root.join(".RSmeta/analytics.duckdb"),
        rel_path: None,
        content_hash: String::new(),
        file_size: None,
        source_resource_id: None,
        source_version: None,
        source_table: Some("daily".to_string()),
        keep_versions: None,
    };
    env.store
        .promote_table_from_project(&project, &req, false)
        .await
        .expect("promote v1");
    {
        let mut guard = project.0.lock().await;
        let conn = guard.as_mut().expect("project conn");
        conn.execute_batch("DROP TABLE daily;").expect("drop daily");
        conn.execute_batch(&make(51)).expect("create 50");
    }
    env.store
        .promote_table_from_project(&project, &req, true)
        .await
        .expect("promote v2");

    let v1_table = read_global_version_tables(&env, "restorable_orders")
        .into_iter()
        .find(|(version, _)| *version == 1)
        .and_then(|(_, table)| table)
        .expect("v1 表名");
    let restored = env
        .store
        .restore_global_table_version("restorable_orders", 1)
        .await
        .expect("restore v1");
    assert_eq!(restored.version, 3, "恢复生成新版本");
    let active = read_global_table(&env, "restorable_orders").expect("active table");
    assert_eq!(active, v1_table, "恢复直接指向 v1 旧快照，零复制");
    let (rows, _) = env
        .store
        .table_stats(&format!("rds_archive.{active}"))
        .await
        .expect("stats");
    assert_eq!(rows, Some(100), "恢复版本读到 v1 的 100 行");

    let tables = read_global_version_tables(&env, "restorable_orders");
    assert_eq!(tables.len(), 3, "v1 / v2 / v3 三条版本索引全留");
    for table in tables
        .iter()
        .filter_map(|(_, table)| table.as_deref())
        .collect::<std::collections::BTreeSet<_>>()
    {
        assert!(
            env.store.table_exists(table).await.expect("exists"),
            "历史快照不删"
        );
    }

    let _ = fs::remove_dir_all(&env.dir);
}

/// 全局表快照：回收站只软删登记，永久删除才逐份清快照与版本索引。
#[tokio::test]
async fn g031_global_table_trash_purge_keeps_then_drops_snapshots() {
    let env = create_test_env().await;
    let project = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let make = |rows: i32| {
        format!(
            "CREATE TABLE snap_orders AS SELECT i AS id, i + 1 AS qty FROM range(1, {rows}) r(i);"
        )
    };
    {
        let mut guard = project.0.lock().await;
        guard
            .as_mut()
            .expect("project conn")
            .execute_batch(&make(101))
            .expect("create v1 source");
    }
    let req = PromoteRequest {
        name: "snap_orders".to_string(),
        kind: PromoteKind::DataFile,
        source_abs_path: env.project_root.join(".RSmeta/analytics.duckdb"),
        rel_path: None,
        content_hash: String::new(),
        file_size: None,
        source_resource_id: None,
        source_version: None,
        source_table: Some("snap_orders".to_string()),
        keep_versions: None,
    };
    env.store
        .promote_table_from_project(&project, &req, false)
        .await
        .expect("promote v1");
    {
        let mut guard = project.0.lock().await;
        guard
            .as_mut()
            .expect("project conn")
            .execute_batch("INSERT INTO snap_orders SELECT 101, 102;")
            .expect("change source");
    }
    env.store
        .promote_table_from_project(&project, &req, true)
        .await
        .expect("promote v2");

    let snapshots = read_global_version_tables(&env, "snap_orders");
    assert_eq!(snapshots.len(), 2, "两版各一份快照");
    let tables: Vec<String> = snapshots
        .into_iter()
        .filter_map(|(_, table)| table)
        .collect();
    for table in &tables {
        assert!(
            env.store.table_exists(table).await.expect("exists"),
            "物理快照在 {table}"
        );
    }

    let trashed = env.store.trash_global("snap_orders").await.expect("trash");
    assert_eq!(trashed.version, 2);
    for table in &tables {
        assert!(
            env.store
                .table_exists(table)
                .await
                .expect("exists after trash"),
            "进回收站不删快照：{table}"
        );
    }
    env.store
        .restore_global("snap_orders")
        .await
        .expect("restore");
    env.store.purge_global("snap_orders").await.expect("purge");

    for table in &tables {
        assert!(
            !env.store
                .table_exists(table)
                .await
                .expect("exists after purge"),
            "永久删除要清掉快照：{table}"
        );
    }
    env.store
        .with_sqlite(|conn| {
            let rows: i64 = conn
                .query_row("SELECT count(*) FROM table_asset_versions", [], |row| {
                    row.get(0)
                })
                .map_err(sqlite_err("count_after_purge"))?;
            assert_eq!(rows, 0, "版本索引随资产一起清掉");
            Ok(())
        })
        .expect("purge cleanup");

    let _ = fs::remove_dir_all(&env.dir);
}

/// 项目表资产的本体在 `analytics.rds_archive`：升级必须按限定名冻结，不能当成普通表。
#[tokio::test]
async fn g032_table_asset_promotes_from_project_archive_schema() {
    let env = create_test_env().await;
    let project = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    {
        let mut guard = project.0.lock().await;
        guard
            .as_mut()
            .expect("project conn")
            .execute_batch(
                "CREATE SCHEMA IF NOT EXISTS rds_archive;
                 CREATE TABLE rds_archive.arc_project AS
                   SELECT i AS id FROM range(1, 21) r(i);",
            )
            .expect("create project snapshot");
    }
    let req = PromoteRequest {
        name: "project_snapshot".to_string(),
        kind: PromoteKind::DataFile,
        source_abs_path: env.project_root.join(".RSmeta/analytics.duckdb"),
        rel_path: None,
        content_hash: String::new(),
        file_size: None,
        source_resource_id: Some("arp_project".to_string()),
        source_version: Some(1),
        // UI 对表资产传 `rds_archive.<物理快照>`。
        source_table: Some("rds_archive.arc_project".to_string()),
        keep_versions: None,
    };
    let outcome = env
        .store
        .promote_table_from_project(&project, &req, false)
        .await
        .expect("promote project snapshot");
    assert_eq!(outcome.version, 1);
    assert_eq!(outcome.row_count, Some(20), "快照整表搬运");
    let table = read_global_table(&env, "project_snapshot").expect("global table");
    let (rows, _) = env
        .store
        .table_stats(&format!("rds_archive.{table}"))
        .await
        .expect("global stats");
    assert_eq!(rows, Some(20));
    assert_eq!(
        env.store
            .global_table_head("project_snapshot")
            .expect("head")
            .expect("asset")
            .analytics_table,
        table,
        "全局 head 指向自己的全局快照，不是项目表"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

/// 全局显示名改动后，项目引用仍按稳定 asset_id 找到同一个快照并可固化。
#[tokio::test]
async fn g033_global_rename_keeps_reference_and_solidify() {
    let env = create_test_env().await;
    let project = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let csv = write_csv(&env.dir);
    let promoted = env
        .store
        .promote(&promote_req("rename_orders", &csv, PromoteKind::DataFile))
        .await
        .expect("promote");
    let referenced = env
        .store
        .reference_to_project(
            &project,
            &env.project_root,
            "rename_orders",
            promoted.version,
            None,
        )
        .await
        .expect("reference");
    let asset_id = env
        .store
        .active_global_asset_id("rename_orders")
        .expect("asset id")
        .expect("asset exists");
    env.store
        .with_sqlite(|conn| {
            conn.execute(
                "UPDATE analytics_resources SET name = 'rename_orders_new' \
                 WHERE id = ?1 AND scope = 'global'",
                rusqlite::params![&asset_id],
            )
            .map(|_| ())
            .map_err(|error| {
                shared::error::CoreError::common(shared::error::CommonError::General(format!(
                    "rename global: {error}"
                )))
            })
        })
        .expect("rename global");

    assert_eq!(
        env.store
            .global_name_by_asset_id(&asset_id)
            .expect("name by id"),
        Some("rename_orders_new".to_string())
    );
    assert_eq!(
        env.store
            .active_ref_count("rename_orders_new")
            .expect("active ref"),
        1,
        "改名后 pin 仍挂在同一 asset_id 上"
    );
    let solid = env
        .store
        .solidify_ref_version(
            &project,
            &env.project_root,
            &referenced.resource_id,
            1,
            "rename_orders_solid",
        )
        .await
        .expect("solidify after rename");
    assert_eq!(solid.version, 1);

    let _ = fs::remove_dir_all(&env.dir);
}

/// 同名链的全部版本登记（含软删行）：`(version, analytics_table)`，版本倒序。
fn read_global_version_tables(env: &TestEnv, name: &str) -> Vec<(i32, Option<String>)> {
    env.store
        .with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT COALESCE(tav.version, ar.version),
                           COALESCE(ts.physical_table, ar.analytics_table)
                    FROM analytics_resources ar
                    LEFT JOIN table_asset_versions tav ON tav.asset_id = ar.id
                    LEFT JOIN table_snapshots ts ON ts.snapshot_id = tav.snapshot_id
                    WHERE ar.name = ?1 AND ar.scope = 'global'
                    ORDER BY COALESCE(tav.version, ar.version) DESC
                    "#,
                )
                .map_err(sqlite_err("read_global_version_tables"))?;
            let rows = stmt
                .query_map(rusqlite::params![name], |row| {
                    Ok((row.get::<_, i32>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .map_err(sqlite_err("read_global_version_tables_rows"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_err("read_global_version_tables_collect"))
        })
        .expect("读版本表")
}

/// 一版一张表会随版本线性增长，`KeepVersions` 只裁全局表、不碰文件 manifest 与 CAS：
/// **当前版本那张表是本体、永远留**；`keep` 数的是历史份数；登记行与版本快照一条不动。
/// 内容被清掉之后，分发要给人话而不是 DuckDB 的 Catalog Error。
#[tokio::test]
async fn g016_version_table_pruning_keeps_current_and_says_why() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);

    // 覆盖式连升三版：同名链版本接续，各自一张表。
    for round in 0..3 {
        let mut req = promote_req("pruned_orders", &csv, PromoteKind::DataFile);
        req.content_hash = format!("hash_round_{round}");
        env.store.promote_overwrite(&req).await.expect("promote");
    }

    let before = read_global_version_tables(&env, "pruned_orders");
    assert_eq!(before.len(), 3, "三版三条登记");
    for (_, table) in &before {
        let table = table.as_deref().expect("每版都要有表名");
        assert!(
            env.store.table_exists(table).await.expect("exists"),
            "裁剪前每版的表都在：{table}"
        );
    }

    // Keep(1)：留当前版 + 最近 1 份历史，删最老那版。
    let removed = env
        .store
        .prune_version_tables("pruned_orders", crate::model::KeepVersions::Keep(1))
        .await
        .expect("prune");
    assert_eq!(removed, 1, "三版留两版");

    let after = read_global_version_tables(&env, "pruned_orders");
    assert_eq!(after.len(), 3, "登记行一条不删（版本元数据永久保留）");
    assert!(
        env.store
            .table_exists(after[0].1.as_deref().expect("表名"))
            .await
            .expect("exists"),
        "当前版本是本体，必须留"
    );
    assert!(
        env.store
            .table_exists(after[1].1.as_deref().expect("表名"))
            .await
            .expect("exists"),
        "最近一份历史留着"
    );
    assert!(
        !env.store
            .table_exists(after[2].1.as_deref().expect("表名"))
            .await
            .expect("exists"),
        "最老那版的内容已清"
    );

    // 当前版本还能分发。
    let project = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    env.store
        .distribute(&project, &env.project_root, "pruned_orders", None)
        .await
        .expect("分发当前版本");

    // 最新那版的内容也没了（手工删 / 裁剪边界）时，分发必须说是「策略清掉的」。
    let current = after[0].1.as_deref().expect("表名").to_string();
    env.store.drop_global_table(&current).await.expect("drop");
    let err = env
        .store
        .distribute(&project, &env.project_root, "pruned_orders", None)
        .await
        .expect_err("没有内容表时分发必须失败");
    assert!(
        format!("{err}").contains("保留策略"),
        "要说是策略清掉的，而不是数据坏了：{err}"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

/// 刀 B：删除全局资产 = 删各版表 + 登记 + 版本快照 + 分发回执，**项目副本不动**。
#[tokio::test]
async fn g017_delete_global_drops_tables_and_registry_only() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("delete_me", &csv, PromoteKind::DataFile);

    let first = env.store.promote(&req).await.expect("promote v1");
    assert_eq!(first.version, 1);
    let first_table = read_global_table(&env, "delete_me").expect("v1 table");
    let second = env.store.promote_overwrite(&req).await.expect("promote v2");
    assert_eq!(second.version, 2);
    let second_table = read_global_table(&env, "delete_me").expect("v2 table");

    // 造一条分发回执（只验"计数与清理"，不必真开项目库分发一次）。
    env.store
        .with_sqlite(|conn| {
            conn.execute(
                "INSERT INTO analytics_resource_dispatch \
                 (id, global_id, global_name, project_root, target_name, version, shape) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'table')",
                rusqlite::params![
                    Uuid::new_v4().simple().to_string(),
                    first.resource_id,
                    "delete_me",
                    "/tmp/other_project",
                    "delete_me_copy",
                    2,
                ],
            )
            .map_err(sqlite_err("insert_dispatch"))
        })
        .expect("insert dispatch");

    // 只走回收站路径：先移入（写 manifest、软删登记），再永久删除。
    env.store.trash_global("delete_me").await.expect("trash");
    let outcome = env.store.purge_global("delete_me").await.expect("purge");
    assert_eq!(outcome.name, "delete_me");
    assert_eq!(outcome.versions, 2, "两版登记都要删");
    assert_eq!(outcome.tables, 2, "一版一张表，两张都要删");
    assert_eq!(outcome.projects, 1, "回执在删除前数过");

    assert!(
        !env.store
            .table_exists(&first_table)
            .await
            .expect("probe v1"),
        "v1 表应已删除"
    );
    assert!(
        !env.store
            .table_exists(&second_table)
            .await
            .expect("probe v2"),
        "v2 表应已删除"
    );

    let (rows, versions, dispatches) = env
        .store
        .with_sqlite(|conn| {
            let rows: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM analytics_resources \
                     WHERE name = 'delete_me' AND scope = 'global'",
                    [],
                    |row| row.get(0),
                )
                .map_err(sqlite_err("count_rows"))?;
            let versions: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM analytics_resource_versions \
                     WHERE resource_id = ?1",
                    rusqlite::params![first.resource_id],
                    |row| row.get(0),
                )
                .map_err(sqlite_err("count_versions"))?;
            let dispatches: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM analytics_resource_dispatch \
                     WHERE global_name = 'delete_me'",
                    [],
                    |row| row.get(0),
                )
                .map_err(sqlite_err("count_dispatch"))?;
            Ok((rows, versions, dispatches))
        })
        .expect("count leftovers");
    assert_eq!((rows, versions, dispatches), (0, 0, 0));

    // 删过的名字再删一次：明确「资源不存在」，不是静默成功。
    assert!(matches!(
        env.store.trash_global("delete_me").await,
        Err(InstanceOpError::NotFound { .. })
    ));

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g023_trash_restore_keeps_table_until_purge() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("trash_orders", &csv, PromoteKind::DataFile);
    env.store.promote(&req).await.expect("promote");
    let table = read_global_table(&env, "trash_orders").expect("table");

    let trashed = env.store.trash_global("trash_orders").await.expect("trash");
    assert_eq!(trashed.name, "trash_orders");
    assert_eq!(trashed.version, 1);
    assert_eq!(trashed.analytics_table.as_deref(), Some(table.as_str()));
    assert!(
        env.store.table_exists(&table).await.expect("probe"),
        "移入回收站只软删登记，表必须留库"
    );
    assert!(
        list_global_resources(&env.store)
            .expect("list")
            .iter()
            .all(|row| row.name != "trash_orders"),
        "回收站条目不应出现在活跃列表"
    );

    let restored = env
        .store
        .restore_global("trash_orders")
        .await
        .expect("restore");
    assert_eq!(restored.version, 1);
    assert_eq!(
        list_global_resources(&env.store)
            .expect("list")
            .iter()
            .filter(|row| row.name == "trash_orders")
            .count(),
        1,
        "恢复后登记回到活跃列表"
    );
    assert!(!trashed.trash_dir.exists(), "恢复成功后回收站条目要清掉");

    // 第二次移入后手工 DROP 表：恢复必须拒绝，而不是复活一条指向空表的登记。
    let trashed = env
        .store
        .trash_global("trash_orders")
        .await
        .expect("trash again");
    env.store
        .drop_global_table(&table)
        .await
        .expect("drop table");
    let refused = env.store.restore_global("trash_orders").await;
    assert!(
        matches!(refused, Err(InstanceOpError::Core(_))),
        "表已不在时应拒绝恢复：{refused:?}"
    );

    env.store.purge_global("trash_orders").await.expect("purge");
    assert!(
        !env.store.table_exists(&table).await.expect("probe"),
        "永久删除后表必须消失"
    );
    assert!(!trashed.trash_dir.exists(), "永久删除要清掉回收站条目");
    assert!(matches!(
        env.store.purge_global("trash_orders").await,
        Err(InstanceOpError::NotFound { .. })
    ));

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g028_global_trash_lists_restores_and_purges() {
    let env = create_test_env().await;
    for name in ["trash_a", "trash_b"] {
        let rel = format!("global/{name}.csv");
        let (source, manifest_id) = seed_project_manifest(&env, &rel, b"id,qty\n1,10\n").await;
        env.store
            .publish_file(&publish_req(name, &env, source, manifest_id, &rel, None))
            .await
            .expect("publish");
    }
    env.store.trash_global("trash_a").await.expect("trash a");
    env.store.trash_global("trash_b").await.expect("trash b");

    let listed = env.store.list_global_trash().expect("list trash");
    let mut names: Vec<_> = listed.iter().map(|entry| entry.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["trash_a", "trash_b"]);
    assert!(
        listed.iter().all(|entry| entry.has_file_payload),
        "文件资产进回收站要带当前物化缓存"
    );

    env.store
        .restore_global("trash_a")
        .await
        .expect("restore a");
    let left = env.store.list_global_trash().expect("list after restore");
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "trash_b");

    env.store.purge_global("trash_b").await.expect("purge b");
    assert!(
        env.store
            .list_global_trash()
            .expect("list after purge")
            .is_empty(),
        "永久删除后回收站列表要清掉"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g029_empty_global_trash_skips_pinned_entries() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("pinned_empty", &csv, PromoteKind::DataFile);
    env.store.promote(&req).await.expect("promote");
    env.store.trash_global("pinned_empty").await.expect("trash");
    // 进回收站后再被项目 pin：清空必须跳过，不能把仍被引用的登记抹掉。
    pin_ref(&env, "arp_pinned_empty", 1, "pinned_empty", 1);

    let (purged, skipped) = env.store.empty_global_trash().await.expect("empty");
    assert_eq!(purged, 0);
    assert_eq!(skipped, 1);
    assert_eq!(
        env.store.list_global_trash().expect("list").len(),
        1,
        "被 pin 的条目仍留在回收站"
    );

    env.store
        .release_global_ref(&env.project_root, "arp_pinned_empty", 1)
        .expect("release");
    let (purged, skipped) = env
        .store
        .empty_global_trash()
        .await
        .expect("empty after release");
    assert_eq!((purged, skipped), (1, 0));
    assert!(env.store.list_global_trash().expect("list").is_empty());

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g024_trash_refuses_while_a_project_pins_the_version() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("pinned_trash", &csv, PromoteKind::DataFile);
    env.store.promote(&req).await.expect("promote");
    pin_ref(&env, "arp_ref_trash", 1, "pinned_trash", 1);

    let refused = env.store.trash_global("pinned_trash").await;
    assert!(
        matches!(refused, Err(InstanceOpError::Referenced { refs: 1, .. })),
        "被项目 pin 的全局版本不能移入回收站：{refused:?}"
    );

    env.store
        .release_global_ref(&env.project_root, "arp_ref_trash", 1)
        .expect("release pin");
    let trashed = env
        .store
        .trash_global("pinned_trash")
        .await
        .expect("trash after release");
    env.store
        .restore_global("pinned_trash")
        .await
        .expect("restore");
    env.store.purge_global("pinned_trash").await.expect("purge");
    assert!(!trashed.trash_dir.exists(), "永久删除后回收站条目要清掉");

    let _ = fs::remove_dir_all(&env.dir);
}

fn pin_ref(
    env: &TestEnv,
    resource_id: &str,
    project_version: i32,
    global_name: &str,
    global_version: i32,
) {
    let global_asset_id = env
        .store
        .active_global_asset_id(global_name)
        .expect("resolve global asset id")
        .expect("global asset exists");
    env.store
        .pin_global_ref(&crate::global::GlobalRefPin {
            project_root: env.project_root.to_string_lossy().replace('\\', "/"),
            project_resource_id: resource_id.to_string(),
            project_version,
            global_asset_id,
            global_version,
        })
        .expect("pin global ref");
}

/// 保留按版本：被 pin 的历史版即使超出 `KeepVersions` 也不能裁；
/// 释放该版 pin 后，下一次裁剪才允许删它。
#[tokio::test]
async fn g018_prune_keeps_only_pinned_versions() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);

    for round in 0..3 {
        let mut req = promote_req("ref_guard_orders", &csv, PromoteKind::DataFile);
        req.content_hash = format!("hash_ref_round_{round}");
        env.store.promote_overwrite(&req).await.expect("promote");
    }
    let versions = read_global_version_tables(&env, "ref_guard_orders");
    assert_eq!(versions.len(), 3);
    let oldest_version = versions[2].0;
    let oldest_table = versions[2].1.clone().expect("oldest table");
    let middle_table = versions[1].1.clone().expect("middle table");

    // 只 pin 最老版本；当前 v3 是本体，Keep(0) 只允许删 v1/v2 中没人引用的。
    pin_ref(&env, "arp_ref_guard", 1, "ref_guard_orders", oldest_version);
    let removed = env
        .store
        .prune_version_tables("ref_guard_orders", crate::model::KeepVersions::MetadataOnly)
        .await
        .expect("prune");
    assert_eq!(removed, 1, "只删中间那版，最老那版有 pin");
    assert!(
        env.store.table_exists(&oldest_table).await.expect("exists"),
        "被 pin 的最老版本必须保留"
    );
    assert!(
        !env.store.table_exists(&middle_table).await.expect("exists"),
        "未被 pin 的中间版本可以裁掉"
    );

    // 释放这一版 pin 后再裁，最老版本才允许被删。
    let released = env
        .store
        .release_global_ref(&env.project_root, "arp_ref_guard", 1)
        .expect("release");
    assert_eq!(released, 1);
    let removed = env
        .store
        .prune_version_tables("ref_guard_orders", crate::model::KeepVersions::MetadataOnly)
        .await
        .expect("prune after release");
    assert!(removed >= 1, "释放 pin 后至少会删掉最老那版");
    assert!(
        !env.store.table_exists(&oldest_table).await.expect("exists"),
        "释放 pin 后历史版本可裁"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

/// 删除守卫：有活跃引用时整条全局资产不能删，登记/表都保持原样；
/// 释放引用后才允许删除。
#[tokio::test]
async fn g019_delete_global_blocked_by_active_ref() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    let req = promote_req("ref_delete_me", &csv, PromoteKind::DataFile);
    let outcome = env.store.promote(&req).await.expect("promote");
    let table = read_global_table(&env, "ref_delete_me").expect("table");

    pin_ref(&env, "arp_ref_delete", 1, "ref_delete_me", outcome.version);
    let err = env
        .store
        .trash_global("ref_delete_me")
        .await
        .expect_err("有引用必须拒绝移入回收站");
    assert!(matches!(err, InstanceOpError::Referenced { refs: 1, .. }));
    assert!(
        env.store.table_exists(&table).await.expect("exists"),
        "拒绝删除时表必须还在"
    );
    assert_eq!(
        env.store.active_ref_count("ref_delete_me").expect("count"),
        1
    );

    env.store
        .release_global_ref(&env.project_root, "arp_ref_delete", 1)
        .expect("release");
    env.store
        .trash_global("ref_delete_me")
        .await
        .expect("trash");
    let deleted = env
        .store
        .purge_global("ref_delete_me")
        .await
        .expect("purge");
    assert_eq!(deleted.versions, 1);
    assert!(
        !env.store.table_exists(&table).await.expect("exists"),
        "释放引用后删除成功"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

/// pin 是版本级 upsert：同一项目引用版本重指到新全局版本时，
/// 旧版本计数归零，新版本计数为一。
#[tokio::test]
async fn g020_pin_ref_upserts_version_and_reactivates() {
    let env = create_test_env().await;
    let csv = write_csv(&env.dir);
    for round in 0..2 {
        let mut req = promote_req("ref_upsert_orders", &csv, PromoteKind::DataFile);
        req.content_hash = format!("hash_upsert_{round}");
        env.store.promote_overwrite(&req).await.expect("promote");
    }

    pin_ref(&env, "arp_ref_upsert", 1, "ref_upsert_orders", 1);
    assert_eq!(
        env.store
            .active_ref_count_for_version("ref_upsert_orders", 1)
            .expect("count v1"),
        1
    );

    pin_ref(&env, "arp_ref_upsert", 1, "ref_upsert_orders", 2);
    assert_eq!(
        env.store
            .active_ref_count_for_version("ref_upsert_orders", 1)
            .expect("count v1"),
        0
    );
    assert_eq!(
        env.store
            .active_ref_count_for_version("ref_upsert_orders", 2)
            .expect("count v2"),
        1
    );
    assert_eq!(
        env.store
            .active_ref_versions("ref_upsert_orders")
            .expect("versions"),
        vec![2]
    );

    env.store
        .release_global_ref(&env.project_root, "arp_ref_upsert", 1)
        .expect("release");
    pin_ref(&env, "arp_ref_upsert", 1, "ref_upsert_orders", 2);
    assert_eq!(
        env.store
            .active_ref_count_for_version("ref_upsert_orders", 2)
            .expect("count reactivated"),
        1,
        "释放后同一版本再 pin 要重新激活"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

/// 引用资产不落数据；同一条引用资产按版本追加；固化选中历史版本时只释放该版 pin。
#[tokio::test]
async fn g021_reference_asset_and_solidify_selected_version() {
    let env = create_test_env().await;
    let project = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let csv = write_csv(&env.dir);
    let v1 = env
        .store
        .promote(&promote_req("ref_orders", &csv, PromoteKind::DataFile))
        .await
        .expect("promote v1");
    assert_eq!(v1.version, 1);

    let first = env
        .store
        .reference_to_project(
            &project,
            &env.project_root,
            "ref_orders",
            v1.version,
            Some("订单引用"),
        )
        .await
        .expect("reference v1");
    assert_eq!(first.version, 1, "引用资产首版从 1 开始");

    // 项目侧只有引用登记，没有物理表，也没有 analytics_table。
    let (kind, table, origin_kind, origin_ref, version, alias, source_table): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        i32,
        Option<String>,
        Option<String>,
    ) = {
        let conn = env.project_pool.acquire().await.expect("project conn");
        let inner = conn.inner().expect("inner");
        inner
            .query_row(
                "SELECT kind, analytics_table, origin_kind, origin_ref, version, alias, source_table \
                 FROM analytics_resources WHERE id = ?1",
                rusqlite::params![first.resource_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .expect("read ref row")
    };
    assert_eq!(kind, "table_ref");
    assert!(table.is_none(), "引用资产不能登记本地表名");
    assert_eq!(origin_kind.as_deref(), Some("global"));
    assert!(origin_ref
        .as_deref()
        .unwrap_or("")
        .contains("\"version\":1"));
    assert_eq!(version, 1);
    assert_eq!(alias.as_deref(), Some("订单引用"));
    assert!(
        source_table.is_some(),
        "引用行要记 pin 版本的全局实际表名，打开/统计才能取样"
    );

    // 引用资产的 name 是全局本名，项目内不能 rename，只能改 alias。
    let store = crate::store::AnalyticsResourceStore::new(env.project_pool.clone());
    let rename_error = store
        .rename_resource(&first.resource_id, "改坏全局名")
        .await
        .expect_err("全局引用资产不能改本名");
    assert!(
        rename_error.to_string().contains("不能改本名"),
        "错误要说清只能改别名：{rename_error}"
    );

    // 引用资产上的项目分组/标签，固化时要复制到新本地资产。
    let folder = store
        .create_folder(crate::store::rows::CreateFolderRequest {
            name: "订单分组".to_string(),
            scope: "project".to_string(),
            parent_folder_id: None,
            color: None,
            icon: None,
        })
        .await
        .expect("create folder");
    store
        .add_resource_to_folder(&first.resource_id, &folder.id)
        .await
        .expect("link folder");
    let tag = store
        .create_tag(crate::store::rows::CreateTagRequest {
            name: format!("全局引用标签{}", Uuid::new_v4().simple()),
            color: None,
            icon: None,
            scope: "global".to_string(),
        })
        .await
        .expect("create tag");
    store
        .add_tag_to_resource(&first.resource_id, &tag.id)
        .await
        .expect("link tag");

    // 全局升到 v2，B 的同一引用资产追加 v2；v1 仍 pin 住。
    fs::write(&csv, "id,sku,qty\n1,a,10\n2,b,20\n").expect("rewrite csv");
    let v2 = env
        .store
        .promote_overwrite(&promote_req("ref_orders", &csv, PromoteKind::DataFile))
        .await
        .expect("promote v2");
    assert_eq!(v2.version, 2);
    let second = env
        .store
        .reference_to_project(
            &project,
            &env.project_root,
            "ref_orders",
            v2.version,
            Some("订单引用"),
        )
        .await
        .expect("reference v2");
    assert_eq!(second.resource_id, first.resource_id, "同一引用资产");
    assert_eq!(second.version, 2);
    assert_eq!(
        env.store
            .active_ref_count_for_version("ref_orders", 1)
            .expect("pin v1"),
        1
    );
    assert_eq!(
        env.store
            .active_ref_count_for_version("ref_orders", 2)
            .expect("pin v2"),
        1
    );

    // 固化选中 v1：新本地资产 v1，原引用仍在，v1 pin 释放，v2 pin 不动。
    let solid = env
        .store
        .solidify_ref_version(
            &project,
            &env.project_root,
            &second.resource_id,
            1,
            "ref_orders_solid_v1",
        )
        .await
        .expect("solidify v1");
    assert_eq!(solid.version, 1, "固化资产从本地 v1 开始");
    assert_eq!(
        env.store
            .active_ref_count_for_version("ref_orders", 1)
            .expect("pin v1"),
        0,
        "固化后释放选中版本 pin"
    );
    assert_eq!(
        env.store
            .active_ref_count_for_version("ref_orders", 2)
            .expect("pin v2"),
        1,
        "其他引用版本 pin 不动"
    );

    // 原引用资产仍在，当前版本仍是 v2。
    let still_ref: (String, i32) = {
        let conn = env.project_pool.acquire().await.expect("project conn");
        let inner = conn.inner().expect("inner");
        inner
            .query_row(
                "SELECT kind, version FROM analytics_resources WHERE id = ?1",
                rusqlite::params![second.resource_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read ref row after solidify")
    };
    assert_eq!(still_ref, ("table_ref".to_string(), 2));

    // 新本地资产继承引用的别名、分组、标签。
    let (solid_alias, solid_folder, solid_tags): (Option<String>, Option<String>, i64) = {
        let conn = env.project_pool.acquire().await.expect("project conn");
        let inner = conn.inner().expect("inner");
        let alias = inner
            .query_row(
                "SELECT alias FROM analytics_resources WHERE id = ?1",
                rusqlite::params![solid.resource_id],
                |row| row.get(0),
            )
            .expect("read solid alias");
        let folder = inner
            .query_row(
                "SELECT folder_id FROM analytics_resource_folder WHERE resource_id = ?1 LIMIT 1",
                rusqlite::params![solid.resource_id],
                |row| row.get(0),
            )
            .optional()
            .expect("read solid folder");
        let tags: i64 = inner
            .query_row(
                "SELECT count(*) FROM analytics_resource_tags WHERE resource_id = ?1",
                rusqlite::params![solid.resource_id],
                |row| row.get(0),
            )
            .expect("read solid tags");
        (alias, folder, tags)
    };
    assert_eq!(solid_alias.as_deref(), Some("订单引用"));
    assert_eq!(solid_folder.as_deref(), Some(folder.id.as_str()));
    assert_eq!(solid_tags, 1, "标签也要复制");

    // 固化出来的是 v1 的内容（3 行），不是 v2（2 行）。
    let rows: Vec<i32> = {
        let mut guard = env.project_duckdb.lock().await;
        let conn = guard.as_mut().expect("project duckdb");
        let mut stmt = conn
            .prepare("SELECT id FROM ref_orders_solid_v1 ORDER BY id")
            .expect("prepare solid table");
        stmt.query_map([], |row| row.get(0))
            .expect("query solid table")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect solid rows")
    };
    assert_eq!(rows, vec![1, 2, 3], "固化的是选中的 v1，不是当前 v2");

    // 标签定义落在 RDS_HOME 的全局库（不是测试临时目录），用例结束要删掉，
    // 否则重复跑会在全局库里越堆越多。
    store.delete_tag(&tag.id).await.expect("delete global tag");
    let _ = fs::remove_dir_all(&env.dir);
}

/// 固化失败（目标名冲突）时不释放 pin、不留下半成品表，原引用仍可再用。
#[tokio::test]
async fn g022_solidify_failure_keeps_pin_and_ref() {
    let env = create_test_env().await;
    let project = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let csv = write_csv(&env.dir);
    let v1 = env
        .store
        .promote(&promote_req("ref_conflict", &csv, PromoteKind::DataFile))
        .await
        .expect("promote v1");
    let first = env
        .store
        .reference_to_project(
            &project,
            &env.project_root,
            "ref_conflict",
            v1.version,
            None,
        )
        .await
        .expect("reference v1");

    // 目标名故意撞引用资产自己的登记名 → 分发在落表前返回 Conflict。
    let error = env
        .store
        .solidify_ref_version(
            &project,
            &env.project_root,
            &first.resource_id,
            1,
            "ref_conflict",
        )
        .await
        .expect_err("同名目标应冲突");
    assert!(
        matches!(error, InstanceOpError::Conflict { .. }),
        "应返回名字冲突：{error:?}"
    );
    assert_eq!(
        env.store
            .active_ref_count_for_version("ref_conflict", 1)
            .expect("pin v1"),
        1,
        "失败不能释放 pin"
    );

    // 项目里不应有半成品表。
    let table_count: i64 = {
        let mut guard = env.project_duckdb.lock().await;
        let conn = guard.as_mut().expect("project duckdb");
        conn.prepare(
            "SELECT count(*) FROM information_schema.tables WHERE table_name = 'ref_conflict'",
        )
        .expect("prepare table count")
        .query_row([], |row| row.get(0))
        .expect("query table count")
    };
    assert_eq!(table_count, 0, "失败后不能留下半成品表");

    // 原引用资产仍在且版本还是 v1。
    let (kind, version): (String, i32) = {
        let conn = env.project_pool.acquire().await.expect("project conn");
        let inner = conn.inner().expect("inner");
        inner
            .query_row(
                "SELECT kind, version FROM analytics_resources WHERE id = ?1",
                rusqlite::params![first.resource_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read ref row")
    };
    assert_eq!((kind.as_str(), version), ("table_ref", 1));

    let _ = fs::remove_dir_all(&env.dir);
}

// ==================== M4：全局 file-first ====================

async fn seed_project_manifest(env: &TestEnv, rel: &str, bytes: &[u8]) -> (PathBuf, String) {
    let path = env.project_root.join("resources").join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create project resources dir");
    }
    fs::write(&path, bytes).expect("write project file");
    let cas = crate::payload::cas::CasStore::new(&env.project_root);
    let object = cas.put_file(&path).await.expect("put project file");
    let manifest =
        crate::payload::manifest::FileManifest::single_file(rel, object.hash, object.size, None);
    let manifest_id = cas.put_manifest(&manifest).await.expect("put manifest");
    (path, manifest_id)
}

fn publish_req(
    name: &str,
    env: &TestEnv,
    source: PathBuf,
    manifest_id: String,
    rel: &str,
    base: Option<i32>,
) -> PublishFileRequest {
    PublishFileRequest {
        name: name.to_string(),
        source_project_root: env.project_root.clone(),
        manifest_id: Some(manifest_id),
        source_abs_path: Some(source),
        rel_path: Some(rel.to_string()),
        source_resource_id: Some("arp_source".to_string()),
        source_version: Some(1),
        base_global_version: base,
    }
}

fn read_global_row(
    env: &TestEnv,
    name: &str,
    version: Option<i32>,
) -> (String, Option<String>, String, Option<String>) {
    env.store
        .with_sqlite(|conn| {
            let row = match version {
                Some(version) => conn.query_row(
                    "SELECT kind, analytics_table, content_hash, file_rel_path \
                     FROM analytics_resources WHERE name = ?1 AND scope = 'global' AND version = ?2",
                    rusqlite::params![name, version],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )?,
                None => conn.query_row(
                    "SELECT kind, analytics_table, content_hash, file_rel_path \
                     FROM analytics_resources WHERE name = ?1 AND scope = 'global' \
                     AND deleted_at IS NULL ORDER BY version DESC LIMIT 1",
                    rusqlite::params![name],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )?,
            };
            Ok(row)
        })
        .expect("read global file row")
}

#[tokio::test]
async fn g090_publish_file_is_file_first_and_base_guarded() {
    let env = create_test_env().await;
    let (source, manifest_id) =
        seed_project_manifest(&env, "global/orders.csv", b"id,qty\n1,10\n").await;

    let first = env
        .store
        .publish_file(&publish_req(
            "orders_file",
            &env,
            source.clone(),
            manifest_id.clone(),
            "global/orders.csv",
            None,
        ))
        .await
        .expect("publish v1");
    assert_eq!(first.version, 1);
    assert_eq!(first.content_class, "tabular");

    let row = read_global_row(&env, "orders_file", None);
    assert_eq!(row.0, "file", "全局文件行必须是 file");
    assert!(row.1.is_none(), "文件行不得有 analytics_table");
    assert_eq!(row.2, first.content_hash);
    assert_eq!(row.3.as_deref(), Some("global/orders.csv"));

    let manifest: String = env
        .store
        .with_sqlite(|conn| {
            conn.query_row(
                "SELECT manifest_id FROM file_asset_versions WHERE resource_id = ?1 AND version = 1",
                rusqlite::params![first.resource_id],
                |row| row.get(0),
            )
            .map_err(sqlite_err("read_global_manifest"))
        })
        .expect("global file version");
    assert_eq!(manifest, manifest_id);
    let current = env.dir.join("global/resources/global/orders.csv");
    assert_eq!(
        fs::read(&current).expect("current cache"),
        b"id,qty\n1,10\n"
    );

    // 没带 base 的同名发布不覆盖 head，只报 StaleBase。
    let stale = env
        .store
        .publish_file(&publish_req(
            "orders_file",
            &env,
            source.clone(),
            manifest_id.clone(),
            "global/orders.csv",
            None,
        ))
        .await
        .expect_err("missing base must conflict");
    assert!(
        matches!(
            stale,
            InstanceOpError::StaleBase {
                base: None,
                head: 1,
                ..
            }
        ),
        "应报 base 落后：{stale:?}"
    );

    // base = 1 才追加 v2；内容变化仍走字节 hash。
    let (source2, manifest2) =
        seed_project_manifest(&env, "global/orders-v2.csv", b"id,qty\n1,11\n").await;
    let second = env
        .store
        .publish_file(&publish_req(
            "orders_file",
            &env,
            source2,
            manifest2,
            "global/orders.csv",
            Some(1),
        ))
        .await
        .expect("publish v2");
    assert_eq!(second.version, 2);
    assert_ne!(
        second.content_hash, first.content_hash,
        "改字节必须换 content_hash"
    );
    let row = read_global_row(&env, "orders_file", None);
    assert_eq!(row.0, "file");
    assert!(row.1.is_none());

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g091_binary_global_file_has_no_table() {
    let env = create_test_env().await;
    let source = env.dir.join("blob.bin");
    fs::write(&source, [0_u8, 159, 146, 150, 255]).expect("write binary");
    let req = PublishFileRequest {
        name: "blob_file".to_string(),
        source_project_root: env.project_root.clone(),
        manifest_id: None,
        source_abs_path: Some(source),
        rel_path: Some("binary/blob.bin".to_string()),
        source_resource_id: None,
        source_version: None,
        base_global_version: None,
    };
    let out = env.store.publish_file(&req).await.expect("publish binary");
    assert_eq!(out.content_class, "binary");
    let row = read_global_row(&env, "blob_file", None);
    assert_eq!(row.0, "file");
    assert!(row.1.is_none(), "二进制全局文件不能建表");
    assert_eq!(
        fs::read(env.dir.join("global/resources/binary/blob.bin")).expect("binary cache"),
        [0_u8, 159, 146, 150, 255]
    );

    // 全局库里没有这条资产对应的物理表（表名不会等于资产名）。
    assert!(
        !env.store
            .table_exists("blob_file")
            .await
            .expect("table exists"),
        "二进制全局文件不能建表"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g092_global_checkout_creates_lease_and_blocks_purge() {
    let env = create_test_env().await;
    let (source, manifest_id) = seed_project_manifest(&env, "global/data.csv", b"a,b\n1,2\n").await;
    env.store
        .publish_file(&publish_req(
            "checkout_file",
            &env,
            source,
            manifest_id,
            "global/data.csv",
            None,
        ))
        .await
        .expect("publish");
    fs::create_dir_all(env.project_root.join("scratchpad")).expect("scratchpad");

    let project = crate::store::AnalyticsResourceStore::new(env.project_pool.clone());
    let out = env
        .store
        .checkout_file(
            &project,
            &env.project_root,
            "checkout_file",
            None,
            "global/data.csv",
        )
        .await
        .expect("checkout");
    assert_eq!(out.content_hash.len(), 64);
    assert!(env
        .project_root
        .join("scratchpad/global/data.csv")
        .is_file());
    let lease = project
        .active_lease("global:checkout_file")
        .await
        .expect("active lease")
        .expect("lease exists");
    assert_eq!(lease.base_scope, "global");
    assert_eq!(lease.base_version, 1);

    let second = env
        .store
        .checkout_file(
            &project,
            &env.project_root,
            "checkout_file",
            None,
            "global/data-2.csv",
        )
        .await
        .expect_err("second checkout must be refused");
    assert!(
        matches!(second, InstanceOpError::LeaseActive { .. }),
        "第二次取回应报已有 lease：{second:?}"
    );
    assert!(!env
        .project_root
        .join("scratchpad/global/data-2.csv")
        .exists());

    let purge = env
        .store
        .purge_global("checkout_file")
        .await
        .expect_err("active lease must block purge");
    assert!(
        matches!(purge, InstanceOpError::LeaseActive { .. }),
        "有活跃取回时 purge 必须拒绝：{purge:?}"
    );

    // 归档后结清：项目侧先消费，再关全局侧；之后同一资产可以再次取回。
    project
        .consume_lease(&lease.lease_id, 1)
        .await
        .expect("consume project lease");
    env.store
        .consume_checkout_lease("checkout_file", &env.project_root, 1)
        .await
        .expect("consume global lease");
    assert!(
        project
            .active_lease("global:checkout_file")
            .await
            .expect("read active")
            .is_none(),
        "项目侧 active 租约应已结清"
    );
    env.store
        .checkout_file(
            &project,
            &env.project_root,
            "checkout_file",
            None,
            "global/data-3.csv",
        )
        .await
        .expect("结清后可以再次取回");

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g093_fork_reuses_manifest_without_moving_head() {
    let env = create_test_env().await;
    let (source, manifest_id) = seed_project_manifest(&env, "global/fork.csv", b"k,v\n1,a\n").await;
    let parent = env
        .store
        .publish_file(&publish_req(
            "fork_parent",
            &env,
            source,
            manifest_id,
            "global/fork.csv",
            None,
        ))
        .await
        .expect("publish parent");

    let fork = env
        .store
        .fork_file("fork_parent", 1, "fork_child")
        .await
        .expect("fork");
    assert_eq!(fork.child_version, 1);
    let row = read_global_row(&env, "fork_child", None);
    assert_eq!(row.0, "file");
    assert!(row.1.is_none());
    assert!(env
        .dir
        .join("global/resources/global/fork_child.csv")
        .is_file());

    // 原 head 不变，fork 关系落库。
    let parent_row = read_global_row(&env, "fork_parent", None);
    assert_eq!(parent_row.2, parent.content_hash);
    let (parent_name, child_name): (String, String) = env
        .store
        .with_sqlite(|conn| {
            conn.query_row(
                "SELECT parent_name, child_name FROM asset_forks WHERE fork_id = ?1",
                rusqlite::params![fork.fork_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(sqlite_err("read_asset_fork"))
        })
        .expect("asset fork row");
    assert_eq!(parent_name, "fork_parent");
    assert_eq!(child_name, "fork_child");

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g094_distribute_file_first_materializes_project_file() {
    let env = create_test_env().await;
    let (source, manifest_id) = seed_project_manifest(&env, "global/dist.csv", b"x,y\n1,2\n").await;
    env.store
        .publish_file(&publish_req(
            "dist_file",
            &env,
            source,
            manifest_id,
            "global/dist.csv",
            None,
        ))
        .await
        .expect("publish");
    // 模拟发布后项目侧工作副本已移走（真实流程里归档会消费草稿）；再分发回来不应冲突。
    fs::remove_file(env.project_root.join("resources/global/dist.csv"))
        .expect("remove published source");
    let project = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let out = env
        .store
        .distribute(&project, &env.project_root, "dist_file", None)
        .await
        .expect("distribute file");
    assert_eq!(out.shape, DistributeShape::File);
    assert!(env.project_root.join("resources/global/dist.csv").is_file());
    let (kind, table, origin_kind): (String, Option<String>, Option<String>) = {
        let conn = env.project_pool.acquire().await.expect("project conn");
        let inner = conn.inner().expect("inner");
        inner
            .query_row(
                "SELECT kind, analytics_table, origin_kind FROM analytics_resources \
                 WHERE id = ?1",
                rusqlite::params![out.resource_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("read project file row")
    };
    assert_eq!(kind, "file");
    assert!(table.is_none(), "文件分发不能落 analytics_table");
    assert_eq!(origin_kind.as_deref(), Some("global"));

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g095_restore_file_version_from_manifest_creates_new_head() {
    let env = create_test_env().await;
    let (source, manifest_id) =
        seed_project_manifest(&env, "global/restore.csv", b"k,v\n1,old\n").await;
    let v1 = env
        .store
        .publish_file(&publish_req(
            "restore_file",
            &env,
            source,
            manifest_id,
            "global/restore.csv",
            None,
        ))
        .await
        .expect("publish v1");
    let (source2, manifest2) =
        seed_project_manifest(&env, "global/restore-v2.csv", b"k,v\n1,new\n").await;
    let v2 = env
        .store
        .publish_file(&publish_req(
            "restore_file",
            &env,
            source2,
            manifest2,
            "global/restore.csv",
            Some(v1.version),
        ))
        .await
        .expect("publish v2");
    assert_eq!(v2.version, 2);
    assert_ne!(v1.content_hash, v2.content_hash);

    let restored = env
        .store
        .restore_global_file_version("restore_file", v1.version)
        .await
        .expect("restore v1");
    assert_eq!(restored.version, 3, "恢复 = 新版，不原地回滚");
    let row = read_global_row(&env, "restore_file", None);
    assert_eq!(row.2, v1.content_hash, "当前内容回到 v1");
    assert_eq!(
        fs::read(env.dir.join("global/resources/global/restore.csv")).expect("current"),
        b"k,v\n1,old\n"
    );

    let versions = env
        .store
        .with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT version FROM analytics_resources WHERE name = 'restore_file' \
                     AND scope = 'global' ORDER BY version",
                )
                .map_err(sqlite_err("read_restore_versions"))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, i32>(0))
                .map_err(sqlite_err("query_restore_versions"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_err("collect_restore_versions"))?;
            Ok(rows)
        })
        .expect("versions");
    assert_eq!(versions, vec![1, 2, 3], "恢复只追加，不删旧版本");

    let _ = fs::remove_dir_all(&env.dir);
}
