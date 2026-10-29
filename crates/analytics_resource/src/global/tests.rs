    use super::*;
    use engine::persistence::ProjectSqlitePool;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    const BASE_MIGRATION_SQL: &str =
        include_str!("../../../store/migrations/project_meta/007_analytics_resources.sql");
    const ARCHIVE_MIGRATION_SQL: &str =
        include_str!("../../../store/migrations/project_meta/020_analytics_resource_archive.sql");
    const PAYLOAD_MODE_MIGRATION_SQL: &str =
        include_str!("../../../store/migrations/project_meta/023_analytics_resource_payload_mode.sql");
    const TAG_LINK_FK_MIGRATION_SQL: &str = include_str!(
        "../../../store/migrations/project_meta/024_analytics_resource_tags_drop_tag_fk.sql"
    );

    struct TestEnv {
        dir: PathBuf,
        store: GlobalInstanceStore,
        project_pool: Arc<ProjectSqlitePool>,
        project_duckdb: Arc<AsyncMutex<Option<duckdb::Connection>>>,
        project_root: PathBuf,
    }

    async fn create_test_env() -> TestEnv {
        let dir = std::env::temp_dir().join(format!("rds_gtest_{}", Uuid::new_v4().simple()));
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
                .execute_batch(BASE_MIGRATION_SQL)
                .expect("migrate 007");
            inner
                .execute_batch(ARCHIVE_MIGRATION_SQL)
                .expect("migrate 020");
            inner
                .execute_batch(PAYLOAD_MODE_MIGRATION_SQL)
                .expect("migrate 023");
            inner
                .execute_batch(TAG_LINK_FK_MIGRATION_SQL)
                .expect("migrate 024");
        }

        // 项目 DuckDB 写连接（独立临时文件）。
        let duckdb_path = project_dir.join(".RSmeta/analytics.duckdb");
        fs::create_dir_all(duckdb_path.parent().unwrap()).expect("create rsmeta for duckdb");
        let project_duckdb =
            Arc::new(AsyncMutex::new(Some(
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
        fs::write(
            &path,
            "id,sku,qty\n1,a,10\n2,b,20\n3,c,30\n",
        )
        .expect("write csv");
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
        }
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

        // 全局 DuckDB 表真实存在 + 行数一致（经 store 连接验证，Windows 单句柄下
        // 不能另开文件连接复核）。
        let (rows, cols) = env.store.table_stats("orders_v1").await.expect("global stats");
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
        assert!(row.definition_sql.as_deref().unwrap_or("").contains("read_csv_auto"));

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
        let err = env.store.promote(&req).await.expect_err("second promote must conflict");
        assert!(
            matches!(err, InstanceOpError::Conflict { .. }),
            "同名升级应报 Conflict，实际 {err:?}"
        );

        // 覆盖式升级 → 版本 2。
        let out = env.store
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

        // 全局内容表可读回原文（分发重建依赖它）——经 store 连接验证。
        let content = env
            .store
            .read_script_content("calc_script")
            .await
            .expect("read content");
        assert!(content.contains("SELECT 1 AS one"), "内容完整入库");

        let list = list_global_resources(&env.store).expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "analysis");
        assert!(list[0].file_rel_path.as_deref().unwrap_or("").contains("calc_script"));

        let _ = fs::remove_dir_all(&env.dir);
    }

    #[tokio::test]
    async fn g004_distribute_table_to_project() {
        let env = create_test_env().await;
        let csv = write_csv(&env.dir);
        let req = promote_req("shared_orders", &csv, PromoteKind::DataFile);
        env.store.promote(&req).await.expect("promote");

        let project_conn = ProjectDuckConn(
            env.project_duckdb.clone(),
            env.project_pool.clone(),
        );
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
        assert!(promoted_from.starts_with("global:"), "项目行记全局来源");

        let _ = fs::remove_dir_all(&env.dir);
    }

    #[tokio::test]
    async fn g005_distribute_script_rebuilds_file() {
        let env = create_test_env().await;
        let script = env.dir.join("report.md");
        fs::write(&script, "# 周报\n\n数据正常\n").expect("write md");
        let req = promote_req("weekly_note", &script, PromoteKind::Script);
        env.store.promote(&req).await.expect("promote script");

        let project_conn = ProjectDuckConn(
            env.project_duckdb.clone(),
            env.project_pool.clone(),
        );
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
        let project_conn = ProjectDuckConn(
            env.project_duckdb.clone(),
            env.project_pool.clone(),
        );
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
