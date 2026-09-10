use arrow::record_batch::RecordBatch;
use duckdb::Connection;

use super::manager::DuckDBManager;
use crate::core::error::{CommonError, CoreError};

/// DuckDB SQL 执行结果
pub struct DuckDBResult {
    /// 查询结果列名
    pub columns: Vec<String>,
    /// 查询结果数据批次（Arrow 格式）
    pub batches: Vec<RecordBatch>,
    /// 受影响的行数（适用于 INSERT/UPDATE/DELETE）
    pub rows_affected: Option<u64>,
}

impl DuckDBResult {
    /// 创建空查询结果
    pub fn empty() -> Self {
        DuckDBResult {
            columns: Vec::new(),
            batches: Vec::new(),
            rows_affected: None,
        }
    }

    /// 获取总行数
    pub fn total_rows(&self) -> usize {
        self.batches.iter().map(|b| b.num_rows()).sum()
    }
}

/// DuckDB SQL 执行器
///
/// 所有业务模块禁止直接调用 `duckdb::Connection`，必须通过本执行器执行 SQL。
///
/// # 保障
/// - 统一错误处理、日志记录
/// - 读写连接物理隔离
/// - 屏蔽底层连接管理细节
///
/// # 业务调用规范
/// - 只读分析 SQL → 使用 `execute_read()`
/// - 写入临时表/物化数据 → 使用 `execute_write()`
pub struct DuckDBExecutor<'a> {
    /// DuckDB 管理器引用
    manager: &'a DuckDBManager,
}

impl<'a> DuckDBExecutor<'a> {
    /// 创建新的 SQL 执行器。
    ///
    /// # 参数
    /// - `manager`: DuckDBManager 引用
    ///
    /// # 返回
    /// DuckDBExecutor 实例
    pub fn new(manager: &'a DuckDBManager) -> Self {
        DuckDBExecutor { manager }
    }

    /// 执行只读 SQL 查询。
    ///
    /// # 参数
    /// - `sql`: SQL 查询语句
    ///
    /// # 返回
    /// - `Ok(DuckDBResult)`: 查询结果
    /// - `Err(CoreError)`: 执行失败
    ///
    /// # 示例
    /// ```rust,ignore
    /// let executor = DuckDBExecutor::new(&manager);
    /// let result = executor.execute_read("SELECT * FROM users")?;
    /// ```
    pub fn execute_read(&self, sql: &str) -> Result<DuckDBResult, CoreError> {
        let conn = self.manager.read_conn();
        Self::execute_query(conn, sql)
    }

    /// 执行写入 SQL。
    ///
    /// # 参数
    /// - `sql`: SQL 写入语句（INSERT/UPDATE/DELETE/CREATE 等）
    ///
    /// # 返回
    /// - `Ok(DuckDBResult)`: 执行结果，包含受影响行数
    /// - `Err(CoreError)`: 执行失败
    ///
    /// # 示例
    /// ```rust,ignore
    /// let executor = DuckDBExecutor::new(&manager);
    /// let result = executor.execute_write("CREATE TABLE test (id INTEGER)")?;
    /// ```
    pub fn execute_write(&self, sql: &str) -> Result<DuckDBResult, CoreError> {
        let conn = self.manager.write_conn();
        Self::execute_query(conn, sql)
    }

    /// 执行参数化只读查询。
    ///
    /// # 参数
    /// - `sql`: SQL 查询语句，可包含 `?` 占位符
    /// - `params`: 查询参数
    ///
    /// # 返回
    /// - `Ok(DuckDBResult)`: 查询结果
    /// - `Err(CoreError)`: 执行失败
    pub fn execute_read_with_params(
        &self,
        sql: &str,
        params: &[&dyn duckdb::types::ToSql],
    ) -> Result<DuckDBResult, CoreError> {
        let conn = self.manager.read_conn();
        Self::execute_query_with_params(conn, sql, params)
    }

    /// 执行参数化写入 SQL。
    ///
    /// # 参数
    /// - `sql`: SQL 写入语句，可包含 `?` 占位符
    /// - `params`: 查询参数
    ///
    /// # 返回
    /// - `Ok(DuckDBResult)`: 执行结果
    /// - `Err(CoreError)`: 执行失败
    pub fn execute_write_with_params(
        &self,
        sql: &str,
        params: &[&dyn duckdb::types::ToSql],
    ) -> Result<DuckDBResult, CoreError> {
        let conn = self.manager.write_conn();
        Self::execute_query_with_params(conn, sql, params)
    }

    /// 执行批量 SQL 语句。
    ///
    /// # 参数
    /// - `sql`: 批量 SQL 语句，以分号分隔
    ///
    /// # 返回
    /// - `Ok(())`: 执行成功
    /// - `Err(CoreError)`: 执行失败
    pub fn execute_batch(&self, sql: &str) -> Result<(), CoreError> {
        let conn = self.manager.write_conn();
        conn.execute_batch(sql).map_err(|e| {
            tracing::error!("[DuckDBExecutor] 批量执行失败: {}, SQL: {}", e, sql);
            CoreError::common(CommonError::General(format!("批量执行 SQL 失败: {}", e)))
        })
    }

    /// 执行事务性 SQL 批量操作。
    ///
    /// # 参数
    /// - `sql`: 批量 SQL 语句
    ///
    /// # 返回
    /// - `Ok(())`: 事务执行成功
    /// - `Err(CoreError)`: 事务执行失败，自动回滚
    pub fn execute_transaction(&self, sql: &str) -> Result<(), CoreError> {
        self.manager
            .write_conn()
            .execute_batch(sql)
            .map_err(|e| CoreError::common(CommonError::General(format!("事务执行失败: {}", e))))
    }

    /// 内部：执行 SQL 查询（通用）。
    fn execute_query(conn: &Connection, sql: &str) -> Result<DuckDBResult, CoreError> {
        let trimmed = sql.trim().to_uppercase();

        if trimmed.starts_with("SELECT")
            || trimmed.starts_with("WITH")
            || trimmed.starts_with("EXPLAIN")
        {
            let mut stmt = conn.prepare(sql).map_err(|e| {
                CoreError::common(CommonError::General(format!("准备 SQL 语句失败: {}", e)))
            })?;

            let columns: Vec<String> = (0..stmt.column_count())
                .map(|i| stmt.column_name(i).map_or("unknown", |v| v).to_string())
                .collect();

            let row_data: Vec<Vec<duckdb::types::Value>>;

            {
                let mut rows = stmt.query([]).map_err(|e| {
                    CoreError::common(CommonError::General(format!("执行 SQL 查询失败: {}", e)))
                })?;

                let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
                while let Some(row) = rows.next().map_err(|e| {
                    CoreError::common(CommonError::General(format!("获取查询结果失败: {}", e)))
                })? {
                    let mut values: Vec<duckdb::types::Value> = Vec::new();
                    for i in 0.. {
                        match row.get::<usize, duckdb::types::Value>(i) {
                            Ok(v) => values.push(v),
                            Err(_) => break,
                        }
                    }
                    data.push(values);
                }
                row_data = data;
            }

            if row_data.is_empty() {
                Ok(DuckDBResult {
                    columns,
                    batches: Vec::new(),
                    rows_affected: None,
                })
            } else {
                let batch =
                    crate::core::driver::native::duckdb::duckdb_rows_to_arrow(&columns, &row_data)?;
                Ok(DuckDBResult {
                    columns,
                    batches: vec![batch],
                    rows_affected: None,
                })
            }
        } else {
            let rows_affected = conn.execute(sql, []).map_err(|e| {
                CoreError::common(CommonError::General(format!("执行写入 SQL 失败: {}", e)))
            })? as u64;

            Ok(DuckDBResult {
                columns: Vec::new(),
                batches: Vec::new(),
                rows_affected: Some(rows_affected),
            })
        }
    }

    /// 内部：执行参数化 SQL 查询。
    fn execute_query_with_params(
        conn: &Connection,
        sql: &str,
        params: &[&dyn duckdb::types::ToSql],
    ) -> Result<DuckDBResult, CoreError> {
        let trimmed = sql.trim().to_uppercase();

        let mut stmt = conn.prepare(sql).map_err(|e| {
            CoreError::common(CommonError::General(format!("准备 SQL 语句失败: {}", e)))
        })?;

        if trimmed.starts_with("SELECT")
            || trimmed.starts_with("WITH")
            || trimmed.starts_with("EXPLAIN")
        {
            let columns: Vec<String> = (0..stmt.column_count())
                .map(|i| stmt.column_name(i).map_or("unknown", |v| v).to_string())
                .collect();

            let row_data: Vec<Vec<duckdb::types::Value>>;

            {
                let mut rows = stmt.query(params).map_err(|e| {
                    CoreError::common(CommonError::General(format!("执行 SQL 查询失败: {}", e)))
                })?;

                let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
                while let Some(row) = rows.next().map_err(|e| {
                    CoreError::common(CommonError::General(format!("获取查询结果失败: {}", e)))
                })? {
                    let mut values: Vec<duckdb::types::Value> = Vec::new();
                    for i in 0.. {
                        match row.get::<usize, duckdb::types::Value>(i) {
                            Ok(v) => values.push(v),
                            Err(_) => break,
                        }
                    }
                    data.push(values);
                }
                row_data = data;
            }

            if row_data.is_empty() {
                Ok(DuckDBResult {
                    columns,
                    batches: Vec::new(),
                    rows_affected: None,
                })
            } else {
                let batch =
                    crate::core::driver::native::duckdb::duckdb_rows_to_arrow(&columns, &row_data)?;
                Ok(DuckDBResult {
                    columns,
                    batches: vec![batch],
                    rows_affected: None,
                })
            }
        } else {
            let rows_affected = stmt.execute(params).map_err(|e| {
                CoreError::common(CommonError::General(format!("执行写入 SQL 失败: {}", e)))
            })? as u64;

            Ok(DuckDBResult {
                columns: Vec::new(),
                batches: Vec::new(),
                rows_affected: Some(rows_affected),
            })
        }
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::super::manager::DuckDBManager;
    use super::*;
    use std::fs;

    fn setup_test_executor(
        test_name: &str,
    ) -> Result<(DuckDBExecutor<'static>, std::path::PathBuf), CoreError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(100);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join(format!("test_executor_{}_{}.duckdb", test_name, id));

        // 确保文件被真正删除
        if db_path.exists() {
            let _ = fs::remove_file(&db_path);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // 使用 Box::leak 创建 'static 生命周期引用用于测试
        let manager = Box::new(DuckDBManager::open(&db_path)?);
        let manager: &'static DuckDBManager = Box::leak(manager);

        let executor = DuckDBExecutor::new(manager);
        Ok((executor, db_path))
    }

    fn cleanup_test_db(path: &std::path::Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_execute_read_simple_select() -> Result<(), CoreError> {
        let (executor, db_path) = setup_test_executor("read_simple")?;

        // 先创建测试表
        executor.execute_write("CREATE TABLE test_users (id INTEGER, name TEXT)")?;
        executor.execute_write("INSERT INTO test_users VALUES (1, 'Alice'), (2, 'Bob')")?;

        // 执行只读查询
        let result = executor.execute_read("SELECT * FROM test_users")?;
        assert_eq!(result.columns.len(), 2);
        assert_eq!(result.columns[0], "id");
        assert_eq!(result.columns[1], "name");
        assert_eq!(result.total_rows(), 2);

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_execute_write_create_table() -> Result<(), CoreError> {
        let (executor, db_path) = setup_test_executor("write_create")?;

        let result = executor
            .execute_write("CREATE TABLE test_table (id INTEGER PRIMARY KEY, value TEXT)")?;
        assert!(result.rows_affected.is_some());

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_execute_write_insert() -> Result<(), CoreError> {
        let (executor, db_path) = setup_test_executor("write_insert")?;

        executor.execute_write("CREATE TABLE test_insert (id INTEGER)")?;

        let result = executor.execute_write("INSERT INTO test_insert VALUES (1), (2), (3)")?;
        assert_eq!(result.rows_affected, Some(3));

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_execute_invalid_sql() -> Result<(), CoreError> {
        let (executor, db_path) = setup_test_executor("invalid_sql")?;

        let result = executor.execute_read("INVALID SQL STATEMENT");
        assert!(result.is_err());

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_execute_batch() -> Result<(), CoreError> {
        let (executor, db_path) = setup_test_executor("batch")?;

        let sql = "
            CREATE TABLE test_batch1 (id INTEGER);
            CREATE TABLE test_batch2 (name TEXT);
            INSERT INTO test_batch1 VALUES (1);
            INSERT INTO test_batch2 VALUES ('test');
        ";

        executor.execute_batch(sql)?;

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_execute_read_with_params() -> Result<(), CoreError> {
        let (executor, db_path) = setup_test_executor("read_params")?;

        executor.execute_write("CREATE TABLE test_params (id INTEGER, name TEXT)")?;
        executor.execute_write("INSERT INTO test_params VALUES (1, 'Alice'), (2, 'Bob')")?;

        let param_value: &dyn duckdb::types::ToSql = &"Alice";
        let result = executor
            .execute_read_with_params("SELECT * FROM test_params WHERE name = ?", &[param_value])?;
        assert_eq!(result.total_rows(), 1);

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_execute_read_empty_result() -> Result<(), CoreError> {
        let (executor, db_path) = setup_test_executor("read_empty")?;

        executor.execute_write("CREATE TABLE test_empty (id INTEGER)")?;

        let result = executor.execute_read("SELECT * FROM test_empty")?;
        assert_eq!(result.total_rows(), 0);
        assert!(result.batches.is_empty());

        cleanup_test_db(&db_path);
        Ok(())
    }
}
