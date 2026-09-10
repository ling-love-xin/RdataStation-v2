# -*- coding: utf-8 -*-
import pathlib

# ========== 1) import_export.rs：file!() 相对路径 -> 绝对路径 ==========
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\import_export.rs")
t = f.read_text(encoding="utf-8")
old = """    #[test]
    fn test_validate_file_path_exists() {
        // 使用当前文件作为测试
        let result = ImportExportManager::validate_file_path(file!());
        assert!(result.is_ok());
    }"""
new = """    #[test]
    fn test_validate_file_path_exists() {
        // 使用当前文件作为测试（file!() 为相对路径，cargo test 工作目录是 crate 根，
        // 拼 CARGO_MANIFEST_DIR 得到绝对路径）
        let current_file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/duckdb/import_export.rs");
        let result = ImportExportManager::validate_file_path(&current_file.to_string_lossy());
        assert!(result.is_ok());
    }"""
assert old in t, "validate test not found"
t = t.replace(old, new)
f.write_text(t, encoding="utf-8")
print("import_export.rs: absolute path in test")

# ========== 2) manager.rs：round_robin / maintenance 断言按 Windows 单连接调整 ==========
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\manager.rs")
t = f.read_text(encoding="utf-8")

old2 = """    #[test]
    fn test_read_conn_round_robin() -> Result<(), CoreError> {
        let (manager, db_path) = setup_test_db_unique("read_conn")?;

        // 获取 2 * DEFAULT_READ_POOL_SIZE 次读取连接
        let mut conn_ptrs = Vec::new();
        for _ in 0..(DEFAULT_READ_POOL_SIZE * 2) {
            let conn = manager.read_conn();
            conn_ptrs.push(conn as *const Connection);
        }

        // 应该轮询到不同的连接
        let unique: std::collections::HashSet<_> = conn_ptrs.iter().collect();
        assert_eq!(
            unique.len(),
            DEFAULT_READ_POOL_SIZE,
            "轮询应覆盖所有读取连接"
        );

        cleanup_test_db(&db_path);
        Ok(())
    }"""
new2 = """    #[test]
    fn test_read_conn_round_robin() -> Result<(), CoreError> {
        let (manager, db_path) = setup_test_db_unique("read_conn")?;

        // 获取 2 * DEFAULT_READ_POOL_SIZE 次读取连接
        let mut conn_ptrs = Vec::new();
        for _ in 0..(DEFAULT_READ_POOL_SIZE * 2) {
            let conn = manager.read_conn();
            conn_ptrs.push(conn as *const Connection);
        }

        // 轮询语义：Windows 单连接模式下所有读取连接回退到唯一连接（池为空），
        // 非 Windows 多连接池模式应轮询覆盖全部读取连接
        #[cfg(windows)]
        {
            let unique: std::collections::HashSet<_> = conn_ptrs.iter().collect();
            assert_eq!(unique.len(), 1, "Windows 单连接模式应复用唯一连接");
            assert_eq!(
                conn_ptrs[0],
                manager.write_conn() as *const Connection,
                "单连接模式读取连接应等于写连接"
            );
        }
        #[cfg(not(windows))]
        {
            let unique: std::collections::HashSet<_> = conn_ptrs.iter().collect();
            assert_eq!(
                unique.len(),
                DEFAULT_READ_POOL_SIZE,
                "轮询应覆盖所有读取连接"
            );
        }

        cleanup_test_db(&db_path);
        Ok(())
    }"""
assert old2 in t, "round_robin test not found"
t = t.replace(old2, new2)

old3 = """        // 维护连接不应与写入连接相同
        assert_ne!(
            conn1 as *const Connection,
            manager.write_conn() as *const Connection
        );"""
new3 = """        // 维护连接与写入连接的关系：Windows 单连接模式二者相同（复用唯一连接），
        // 非 Windows 多连接模式下二者不同
        #[cfg(windows)]
        assert_eq!(
            conn1 as *const Connection,
            manager.write_conn() as *const Connection
        );
        #[cfg(not(windows))]
        assert_ne!(
            conn1 as *const Connection,
            manager.write_conn() as *const Connection
        );"""
assert old3 in t, "maintenance unique assert not found"
t = t.replace(old3, new3)

f.write_text(t, encoding="utf-8")
print("manager.rs: tests adapted to windows single-conn mode")
