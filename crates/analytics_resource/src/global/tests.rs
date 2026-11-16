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
}

async fn create_test_env() -> TestEnv {
    let dir = crate::tests::temp_project_dir("gtest");
    let global_dir = dir.join("global");
    let project_dir = dir.join("project");
    fs::create_dir_all(&global_dir).expect("create global dir");
    fs::create_dir_all(&project_dir).expect("create project dir");
    fs::create_dir_all(project_dir.join("resources")).expect("create resources dir");

    let store = GlobalInstanceStore::open_at(
        global_dir.join("global.db"),
        global_dir.join("analytics.duckdb"),
    )
    .await
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

    // 项目 DuckDB 写连接（独立临时文件）。
    let duckdb_path = project_dir.join(".RSmeta/analytics.duckdb");
    fs::create_dir_all(duckdb_path.parent().unwrap()).expect("create rsmeta for duckdb");
    let project_duckdb = Arc::new(AsyncMutex::new(Some(
        duckdb::Connection::open(&duckdb_path).expect("open project duckdb"),
    )));

    TestEnv {
        dir,
        store,
        project_pool: pool,
        project_duckdb,
        project_root: project_dir,
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
        // 数据文件分发 = 建项目表（无重建目标）；脚本分发 = 重建 resources/<rel> 文件。
        rel_path: match kind {
            PromoteKind::DataFile => None,
            PromoteKind::Script => Some(format!("work/{name}")),
        },
        content_hash: format!("hash_{}", Uuid::new_v4().simple()),
        file_size: Some(123),
        // 默认不记来源（这条 helper 只造请求）；要验链的用例自己填。
        source_resource_id: None,
        source_version: None,
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
    assert_eq!(row.structured, Some(1), "数据文件升级出来的全局表能参与分析");
    assert!(
        row.definition_sql
            .as_deref()
            .unwrap_or("")
            .contains("read_csv_auto")
    );

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
async fn g003_promote_script_content_table() {
    let env = create_test_env().await;
    let script = env.dir.join("script.sql");
    fs::write(&script, "SELECT 1 AS one;\n-- 注释\n").expect("write script");
    let req = promote_req("calc_script", &script, PromoteKind::Script);

    let outcome = env.store.promote(&req).await.expect("promote script");
    assert_eq!(outcome.row_count, Some(1), "内容表固定 1 行（内容聚合）");

    // 全局内容表可读回原文（分发重建依赖它）——表名取自登记行（G2 起带时间戳）。
    let table = read_global_table(&env, "calc_script").expect("内容表名");
    let content = env
        .store
        .read_script_content(&table)
        .await
        .expect("read content");
    assert!(content.contains("SELECT 1 AS one"), "内容完整入库");

    let list = list_global_resources(&env.store).expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind, "analysis");
    assert_eq!(
        list[0].structured,
        Some(0),
        "脚本没有列结构，不参与分析"
    );
    assert!(
        list[0]
            .file_rel_path
            .as_deref()
            .unwrap_or("")
            .contains("calc_script")
    );

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
    let (scope, promoted_from): (String, String) = conn
            .query_row(
                "SELECT scope, promoted_from FROM analytics_resources WHERE name = 'shared_orders' AND deleted_at IS NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query project row");
    assert_eq!(scope, "project");
    assert!(
        promoted_from.starts_with(crate::model::GLOBAL_PREFIX),
        "项目行记全局来源"
    );

    let _ = fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn g005_distribute_script_rebuilds_file() {
    let env = create_test_env().await;
    let script = env.dir.join("report.md");
    fs::write(&script, "# 周报\n\n数据正常\n").expect("write md");
    let req = promote_req("weekly_note", &script, PromoteKind::Script);
    env.store.promote(&req).await.expect("promote script");

    let project_conn = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let outcome = env
        .store
        .distribute(&project_conn, &env.project_root, "weekly_note", None)
        .await
        .expect("distribute script");
    assert_eq!(outcome.shape, DistributeShape::File, "脚本分发重建为文件");

    // 文件真实重建在项目 resources/ 下（方案 A 分发侧）。
    let rel = env.store
            .with_sqlite(|conn| {
                conn.query_row(
                    "SELECT file_rel_path FROM analytics_resources WHERE name = 'weekly_note' AND scope = 'global' AND deleted_at IS NULL",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .map_err(sqlite_err("read rel"))
            })
            .expect("read global rel");
    let rebuilt = env.project_root.join("resources").join(&rel);
    let content = fs::read_to_string(&rebuilt).expect("read rebuilt file");
    assert!(content.contains("周报"), "内容重建完整");
    assert!(content.contains("数据正常"));

    // 项目侧是 File 档登记。
    let pool = &env.project_pool;
    let conn = pool.acquire().await.expect("acquire");
    let conn = conn.inner().expect("inner");
    let kind: String = conn
            .query_row(
                "SELECT kind FROM analytics_resources WHERE name = 'weekly_note' AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )
            .expect("query kind");
    assert_eq!(kind, "file");

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

/// 覆盖失败时旧内容必须回到原位：覆盖不是「先删后建」，而是「先藏后换、失败搬回」。
///
/// 失败注入：把全局表 DROP 掉但保留登记行——脚本分发要在内容表里读正文，
/// 读不到就必然报错。此时项目侧旧文件、旧登记都必须还在原处，
/// 备份目录也不能留残骸（否则用户既看不到旧内容，又平白多一堆隐藏文件）。
#[tokio::test]
async fn g009_overwrite_failure_restores_the_old_content() {
    let env = create_test_env().await;
    let script = env.dir.join("report.md");
    fs::write(&script, "# 第一版\n").expect("write md");
    let req = promote_req("ovr_note", &script, PromoteKind::Script);
    env.store.promote(&req).await.expect("promote script");

    let project_conn = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let first = env
        .store
        .distribute(&project_conn, &env.project_root, "ovr_note", None)
        .await
        .expect("first distribute");
    assert_eq!(first.version, 1);
    assert_eq!(first.shape, DistributeShape::File);

    let rel = "work/ovr_note";
    let rebuilt = env.project_root.join("resources").join(rel);
    assert!(rebuilt.is_file(), "首版文件已重建到 resources/");

    // ---- 注入失败：全局表消失、登记还在 ----
    // 表名按登记行取（G2 起表名带时间戳，不再等于资产名）。
    let table = read_global_table(&env, "ovr_note").expect("登记行必有表名");
    env.store
        .drop_global_table(&table)
        .await
        .expect("drop global table");

    let err = env
        .store
        .distribute_overwrite(&project_conn, &env.project_root, "ovr_note", None)
        .await
        .expect_err("内容表没了，覆盖分发必须失败");
    assert!(
        matches!(err, InstanceOpError::Core(_)),
        "读不到内容表应报存储错误，实际 {err:?}"
    );

    // 旧文件回到原位，内容仍是第一版。
    assert!(rebuilt.is_file(), "失败后旧文件必须搬回原位");
    let content = fs::read_to_string(&rebuilt).expect("read restored file");
    assert!(content.contains("第一版"), "旧内容不能被清掉");

    // 旧登记仍是活跃行（没被这次失败永久软删）。
    let pool = &env.project_pool;
    let conn = pool.acquire().await.expect("acquire");
    let conn = conn.inner().expect("inner");
    let active: i64 = conn
        .query_row(
            "SELECT count(*) FROM analytics_resources \
             WHERE name = 'ovr_note' AND scope = 'project' AND deleted_at IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("count active rows");
    assert_eq!(active, 1, "回滚后旧登记仍是活跃行");

    // 备份目录不留残骸。
    let backup_dir = env
        .project_root
        .join(crate::payload::META_DIR_NAME)
        .join("overwrite-backup");
    assert!(!backup_dir.exists(), "回滚后不留备份目录");

    let _ = fs::remove_dir_all(&env.dir);
}

/// 脚本内容表只该有一行：多于一行时宁可报错，也不能静默取首行，
/// 否则产出的文件"看着正常、内容不全"，用户与索引都对不上。
#[tokio::test]
async fn g010_script_content_with_extra_rows_is_rejected() {
    let env = create_test_env().await;
    let script = env.dir.join("report.md");
    fs::write(&script, "# 初稿\n").expect("write md");
    let req = promote_req("bad_rows", &script, PromoteKind::Script);
    env.store.promote(&req).await.expect("promote script");

    // 把表写坏：再插一行（模拟外部误写，或未来改成追加语义却漏了读侧）。
    // 表名按登记行取（G2 起不等于资产名）。
    let table = read_global_table(&env, "bad_rows").expect("内容表名");
    let insert_sql = format!(
        "INSERT INTO {} (\"filename\", \"content\", \"content_hash\") \
         VALUES ('work/bad_rows', '# 第二行', 'h2');",
        crate::global::quote_ident(&table)
    );
    env.store
        .with_duckdb(|conn| {
            conn.execute(&insert_sql, duckdb::params![])
                .map_err(|e| duckdb_err("test_insert_extra_row", &e.to_string()))?;
            Ok(())
        })
        .await
        .expect("insert extra row");

    let project_conn = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    let err = env
        .store
        .distribute(&project_conn, &env.project_root, "bad_rows", None)
        .await
        .expect_err("多行内容表必须被拒绝");
    assert!(
        matches!(err, InstanceOpError::Core(_)),
        "应报存储错误，实际 {err:?}"
    );
    assert!(
        format!("{err:?}").contains("应只有一行"),
        "错误要说清原因，实际 {err:?}"
    );

    // 拒绝发生在读内容阶段，项目侧不该留下半成品文件，也不该有登记。
    assert!(
        !env.project_root
            .join("resources")
            .join("work/bad_rows")
            .exists(),
        "被拒绝时不得重建文件"
    );
    let pool = &env.project_pool;
    let conn = pool.acquire().await.expect("acquire");
    let inner = conn.inner().expect("inner");
    let registered: i64 = inner
        .query_row(
            "SELECT count(*) FROM analytics_resources WHERE name = 'bad_rows'",
            [],
            |row| row.get(0),
        )
        .expect("count rows");
    assert_eq!(registered, 0, "被拒绝时不落任何项目登记");

    let _ = fs::remove_dir_all(&env.dir);
}

/// 脚本提升到全局有体积上限：超了就在读内容之前拒绝，并指向「按数据文件归档」。
///
/// 上限的作用不是省磁盘，而是不让一段超大文本被整段读进内存、再落成全局库里的一条
/// 巨型行——脚本承载的是定义（正常几百 KB），16 MiB 已是异常量级。边界取 `>`
/// 而不是 `>=`：恰好等于上限仍然放行，这里用「上限 + 1 字节」证伪。
#[tokio::test]
async fn g011_oversized_script_is_rejected_before_reading() {
    let env = create_test_env().await;
    let script = env.dir.join("huge.sql");
    fs::write(&script, vec![b'a'; (MAX_SCRIPT_BYTES + 1) as usize]).expect("write huge script");

    let err = env
        .store
        .promote(&promote_req("huge_script", &script, PromoteKind::Script))
        .await
        .expect_err("超过上限必须被拒绝");
    assert!(
        matches!(err, InstanceOpError::Core(_)),
        "应报存储错误，实际 {err:?}"
    );
    let text = format!("{err:?}");
    assert!(
        text.contains("超过上限"),
        "错误要说清是体积问题，实际 {text}"
    );
    assert!(
        text.contains("16 MiB"),
        "错误要给出具体上限值，实际 {text}"
    );
    assert!(
        text.contains("数据文件"),
        "错误要指出替代路径（按数据文件归档），实际 {text}"
    );

    // 拒绝发生在读内容之前：全局库里既没有表（按表名前缀查），也没有登记行。
    let any_table = env
        .store
        .with_duckdb(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT count(*) FROM information_schema.tables \
                     WHERE table_name LIKE 'res_huge_script%'",
                )
                .map_err(|e| duckdb_err("prepare_huge_probe", &e.to_string()))?;
            let count: i64 = stmt
                .query_row([], |row| row.get(0))
                .map_err(|e| duckdb_err("query_huge_probe", &e.to_string()))?;
            Ok(count > 0)
        })
        .await
        .expect("probe tables");
    assert!(!any_table, "被拒绝时不得建全局表");
    let global_rows: i64 = env
        .store
        .with_sqlite(|conn| {
            conn.query_row(
                "SELECT count(*) FROM analytics_resources \
                 WHERE name = 'huge_script' AND scope = 'global'",
                [],
                |row| row.get(0),
            )
            .map_err(sqlite_err("count_global_rows"))
        })
        .expect("count global rows");
    assert_eq!(global_rows, 0, "被拒绝时不落任何全局登记");

    let _ = fs::remove_dir_all(&env.dir);
}

/// 分发要在全局侧留回执：谁用过、用到第几版。
///
/// 项目侧登记只记「我来自哪」，全局侧原本完全不知道「我被谁用过」——于是
/// 「删掉这个全局资源之前，有项目依赖它吗」没有依据。这里钉两件事：分发成功
/// 即记一条；同一目标反复覆盖分发只累加次数，不另起一行。
#[tokio::test]
async fn g012_distribute_leaves_a_receipt_on_the_global_side() {
    let env = create_test_env().await;
    let script = env.dir.join("note.md");
    fs::write(&script, "# 初稿\n").expect("write md");
    let req = promote_req("dispatch_note", &script, PromoteKind::Script);
    env.store.promote(&req).await.expect("promote script");

    let project_conn = ProjectDuckConn(env.project_duckdb.clone(), env.project_pool.clone());
    env.store
        .distribute(&project_conn, &env.project_root, "dispatch_note", None)
        .await
        .expect("distribute");

    let receipts = env
        .store
        .dispatched_projects("dispatch_note")
        .await
        .expect("receipts");
    assert_eq!(receipts.len(), 1, "一次分发收口成一行：{receipts:?}");
    let receipt = &receipts[0];
    assert_eq!(receipt.target_name, "dispatch_note");
    assert_eq!(receipt.version, 1, "回执要记分发时的全局版本");
    assert_eq!(receipt.shape, DistributeShape::File);
    assert_eq!(receipt.dispatch_count, 1);
    assert!(
        receipt.project_root.ends_with("project"),
        "记的是目标项目根，实际 {}",
        receipt.project_root
    );
    assert!(
        !receipt.project_root.contains('\\'),
        "项目根要规范化成 /，实际 {}",
        receipt.project_root
    );

    // 覆盖分发一次：同一目标仍是一行，次数 +1。
    env.store
        .distribute_overwrite(&project_conn, &env.project_root, "dispatch_note", None)
        .await
        .expect("overwrite distribute");
    let receipts = env
        .store
        .dispatched_projects("dispatch_note")
        .await
        .expect("receipts after overwrite");
    assert_eq!(receipts.len(), 1, "重复分发不另起行：{receipts:?}");
    assert_eq!(receipts[0].dispatch_count, 2, "次数要累加");

    // 从没被分发过的全局存档：零行，而不是报错。
    assert!(
        env.store
            .dispatched_projects("never_used")
            .await
            .expect("empty receipts")
            .is_empty(),
        "零行是「没人用过」这个事实，不是错误"
    );

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
