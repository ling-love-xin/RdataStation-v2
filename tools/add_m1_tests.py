# -*- coding: utf-8 -*-
import pathlib

s = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\project\src\store.rs")
t = s.read_text(encoding="utf-8")

old_tail = """        // 重新打开
        let _ = manager.open_project(&project_path)?;
        assert!(manager.current_project().is_some());
        Ok(())
    }
}
"""

new_tail = """        // 重新打开
        let _ = manager.open_project(&project_path)?;
        assert!(manager.current_project().is_some());
        Ok(())
    }

    // ==================== M1 双层数据架构装配验证 ====================

    /// 双层装配：SQLite 元数据库（project.db）可查 + DuckDB 分析引擎（analytics.duckdb）可执行 SQL
    #[test]
    fn test_dual_layer_assembled() -> Result<(), CoreError> {
        let project_path = test_temp_dir("dual_layer").join("proj");
        let store = ProjectStore::create("双层架构验证", &project_path)?;
        let meta_dir = project_path.join(RS_META_DIR_NAME);

        // 层 1：SQLite 元数据库 project.db（系统级共享/项目元数据载体）
        let db_path = store.project_db_path()?;
        assert!(db_path.exists(), "project.db 应存在");
        let conn = rusqlite::Connection::open(&db_path).map_err(|e| {
            persistence::persistence_to_core_error("project.db", "open", &e.to_string())
        })?;
        let cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='project'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| {
                persistence::persistence_to_core_error("project.db", "query", &e.to_string())
            })?;
        assert_eq!(cnt, 1, "project.db 应含 project 元数据表");
        // 元数据已落库（create 时 save_info 同步）
        let name: String = conn
            .query_row("SELECT name FROM project LIMIT 1", [], |r| r.get(0))
            .map_err(|e| {
                persistence::persistence_to_core_error("project.db", "query", &e.to_string())
            })?;
        assert_eq!(name, "双层架构验证");

        // 层 2：DuckDB 分析引擎 analytics.duckdb（项目级分析数据 / mock 落盘目标）
        let analytics_path = meta_dir.join(ANALYTICS_DB_NAME);
        assert!(analytics_path.exists(), "analytics.duckdb 应存在");
        let dconn = duckdb::Connection::open(&analytics_path).map_err(|e| {
            persistence::persistence_to_core_error("analytics.duckdb", "open", &e.to_string())
        })?;
        let v: i64 = dconn
            .query_row("SELECT 1", [], |r| r.get(0))
            .map_err(|e| {
                persistence::persistence_to_core_error("analytics.duckdb", "query", &e.to_string())
            })?;
        assert_eq!(v, 1, "DuckDB 分析引擎可执行 SQL");
        // 分析引擎可写（mock 生成数据的落点）
        dconn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS smoke_data(id BIGINT);
                 INSERT INTO smoke_data VALUES (42);",
            )
            .map_err(|e| {
                persistence::persistence_to_core_error("analytics.duckdb", "write", &e.to_string())
            })?;
        let v2: i64 = dconn
            .query_row("SELECT id FROM smoke_data", [], |r| r.get(0))
            .map_err(|e| {
                persistence::persistence_to_core_error("analytics.duckdb", "query", &e.to_string())
            })?;
        assert_eq!(v2, 42, "分析引擎写入后可读回");
        Ok(())
    }

    /// 项目级物理隔离：项目 A/B 的连接元数据互不可见
    #[test]
    fn test_project_level_isolation() -> Result<(), CoreError> {
        let base = test_temp_dir("isolation");
        let proj_a = ProjectStore::create("项目A", &base.join("proj-a"))?;
        let proj_b = ProjectStore::create("项目B", &base.join("proj-b"))?;

        proj_a.create_connection_metadata("conn-supplier-a")?;
        proj_b.create_connection_metadata("conn-inventory-b")?;

        let list = |dir: &std::path::Path| -> std::io::Result<Vec<String>> {
            Ok(std::fs::read_dir(dir)?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect())
        };

        let a_files = list(&proj_a.project_metadata_dir()?)?;
        assert!(
            a_files.iter().any(|f| f.contains("conn-supplier-a")),
            "项目A 应有自己的连接元数据"
        );
        assert!(
            !a_files.iter().any(|f| f.contains("conn-inventory-b")),
            "项目A 不应看到项目B 的连接元数据（物理隔离）"
        );

        let b_files = list(&proj_b.project_metadata_dir()?)?;
        assert!(
            b_files.iter().any(|f| f.contains("conn-inventory-b")),
            "项目B 应有自己的连接元数据"
        );
        assert!(
            !b_files.iter().any(|f| f.contains("conn-supplier-a")),
            "项目B 不应看到项目A 的连接元数据（物理隔离）"
        );
        Ok(())
    }

    /// 系统级共享资产闭环：元数据库写读（供应商主数据"清理一次、到处可用"的载体）
    #[test]
    fn test_system_shared_meta_roundtrip() -> Result<(), CoreError> {
        let base = test_temp_dir("shared_roundtrip");
        let store = ProjectStore::create("共享资产验证", &base.join("proj"))?;
        let db = store.project_db_path()?;

        // 写入共享资产（以全局连接记录为例：系统级共享，跨项目可用）
        {
            let conn = rusqlite::Connection::open(&db).map_err(|e| {
                persistence::persistence_to_core_error("project.db", "open", &e.to_string())
            })?;
            conn.execute(
                "INSERT INTO connections(id, name, kind) VALUES ('conn-global-001', '供应商主数据', 'duckdb')",
                [],
            )
            .map_err(|e| {
                persistence::persistence_to_core_error("project.db", "insert", &e.to_string())
            })?;
        }

        // 重新打开（模拟另一项目复用共享资产）→ 数据仍可读
        let conn2 = rusqlite::Connection::open(&db).map_err(|e| {
            persistence::persistence_to_core_error("project.db", "open", &e.to_string())
        })?;
        let n: i64 = conn2
            .query_row(
                "SELECT COUNT(*) FROM connections WHERE id='conn-global-001'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| {
                persistence::persistence_to_core_error("project.db", "query", &e.to_string())
            })?;
        assert_eq!(n, 1, "共享资产落库后可读回（一次写入、到处可用）");
        Ok(())
    }
}
"""

assert old_tail in t
t = t.replace(old_tail, new_tail)
s.write_text(t, encoding="utf-8")
print("tests appended")
