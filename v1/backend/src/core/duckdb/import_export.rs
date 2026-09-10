use std::path::Path;

use duckdb::Connection;

use crate::core::error::{CommonError, CoreError};

/// 数据格式类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataFormat {
    /// CSV 格式
    CSV,
    /// Parquet 格式
    Parquet,
    /// JSON 格式
    JSON,
    /// Excel 格式
    Excel,
}

impl std::fmt::Display for DataFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataFormat::CSV => write!(f, "csv"),
            DataFormat::Parquet => write!(f, "parquet"),
            DataFormat::JSON => write!(f, "json"),
            DataFormat::Excel => write!(f, "xlsx"),
        }
    }
}

/// 导入配置
pub struct ImportConfig {
    /// 是否自动检测类型
    pub auto_detect: bool,
    /// 是否有表头
    pub has_header: bool,
    /// 分隔符（CSV 专用）
    pub delimiter: Option<char>,
    /// 额外的 DuckDB COPY 选项
    pub extra_options: Vec<String>,
}

impl Default for ImportConfig {
    fn default() -> Self {
        ImportConfig {
            auto_detect: true,
            has_header: true,
            delimiter: None,
            extra_options: Vec::new(),
        }
    }
}

/// 导出配置
pub struct ExportConfig {
    /// 是否包含表头
    pub header: bool,
    /// 压缩方式（可选）
    pub compression: Option<String>,
    /// 额外的 DuckDB COPY 选项
    pub extra_options: Vec<String>,
}

impl Default for ExportConfig {
    fn default() -> Self {
        ExportConfig {
            header: true,
            compression: None,
            extra_options: Vec::new(),
        }
    }
}

/// 数据导入导出管理器
///
/// 负责：
/// - COPY TO 导出数据
/// - read_csv_auto/read_parquet 导入数据
///
/// # 导入方式
/// - CSV: `read_csv_auto('file.csv')`
/// - Parquet: `read_parquet('file.parquet')`
/// - JSON: `read_json_auto('file.json')`
///
/// # 导出方式
/// - `COPY (query) TO 'file.csv' (HEADER, DELIMITER ',')`
/// - `COPY (query) TO 'file.parquet'`
pub struct ImportExportManager;

impl ImportExportManager {
    /// 执行 CSV 导入到目标表。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `file_path`: CSV 文件路径
    /// - `config`: 导入配置
    /// - `table_name`: 目标表名
    ///
    /// # 返回
    /// - `Ok(u64)`: 导入行数
    /// - `Err(CoreError)`: 导入失败
    pub fn import_csv_to_table(
        &self,
        conn: &Connection,
        file_path: &str,
        config: &ImportConfig,
        table_name: &str,
    ) -> Result<u64, CoreError> {
        let select_sql = Self::generate_import_csv_sql(file_path, config);
        let create_sql = Self::generate_import_to_table_sql(&select_sql, table_name);

        tracing::info!("[ImportExportManager] CSV 导入 SQL: {}", create_sql);

        let rows = conn
            .execute(&create_sql, [])
            .map_err(|e| CoreError::common(CommonError::General(format!("CSV 导入失败: {}", e))))?;

        tracing::info!(
            "[ImportExportManager] CSV 导入完成: {} 行到表 '{}'",
            rows,
            table_name
        );

        Ok(rows as u64)
    }

    /// 执行 Parquet 导入到目标表。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `file_path`: Parquet 文件路径
    /// - `table_name`: 目标表名
    ///
    /// # 返回
    /// - `Ok(u64)`: 导入行数
    /// - `Err(CoreError)`: 导入失败
    pub fn import_parquet_to_table(
        &self,
        conn: &Connection,
        file_path: &str,
        table_name: &str,
    ) -> Result<u64, CoreError> {
        let select_sql = Self::generate_import_parquet_sql(file_path);
        let create_sql = Self::generate_import_to_table_sql(&select_sql, table_name);

        tracing::info!("[ImportExportManager] Parquet 导入 SQL: {}", create_sql);

        let rows = conn.execute(&create_sql, []).map_err(|e| {
            CoreError::common(CommonError::General(format!("Parquet 导入失败: {}", e)))
        })?;

        tracing::info!(
            "[ImportExportManager] Parquet 导入完成: {} 行到表 '{}'",
            rows,
            table_name
        );

        Ok(rows as u64)
    }

    /// 执行导出查询结果为 CSV 文件。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `query`: 要导出的查询 SQL
    /// - `file_path`: 导出文件路径
    /// - `config`: 导出配置
    ///
    /// # 返回
    /// - `Ok(u64)`: 导出行数
    /// - `Err(CoreError)`: 导出失败
    pub fn export_to_csv(
        &self,
        conn: &Connection,
        query: &str,
        file_path: &str,
        config: &ExportConfig,
    ) -> Result<u64, CoreError> {
        let sql = Self::generate_export_csv_sql(query, file_path, config);
        tracing::info!("[ImportExportManager] CSV 导出 SQL: {}", sql);

        let rows = conn
            .execute(&sql, [])
            .map_err(|e| CoreError::common(CommonError::General(format!("CSV 导出失败: {}", e))))?;

        tracing::info!("[ImportExportManager] CSV 导出完成: {} 行", rows);

        Ok(rows as u64)
    }

    /// 执行导出查询结果为 Parquet 文件。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `query`: 要导出的查询 SQL
    /// - `file_path`: 导出文件路径
    /// - `config`: 导出配置
    ///
    /// # 返回
    /// - `Ok(u64)`: 导出行数
    /// - `Err(CoreError)`: 导出失败
    pub fn export_to_parquet(
        &self,
        conn: &Connection,
        query: &str,
        file_path: &str,
        config: &ExportConfig,
    ) -> Result<u64, CoreError> {
        let sql = Self::generate_export_parquet_sql(query, file_path, config);
        tracing::info!("[ImportExportManager] Parquet 导出 SQL: {}", sql);

        let rows = conn.execute(&sql, []).map_err(|e| {
            CoreError::common(CommonError::General(format!("Parquet 导出失败: {}", e)))
        })?;

        tracing::info!("[ImportExportManager] Parquet 导出完成: {} 行", rows);

        Ok(rows as u64)
    }

    /// 执行导出查询结果为 JSON 文件。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `query`: 要导出的查询 SQL
    /// - `file_path`: 导出文件路径
    /// - `config`: 导出配置
    ///
    /// # 返回
    /// - `Ok(u64)`: 导出行数
    /// - `Err(CoreError)`: 导出失败
    pub fn export_to_json(
        &self,
        conn: &Connection,
        query: &str,
        file_path: &str,
        config: &ExportConfig,
    ) -> Result<u64, CoreError> {
        let sql = Self::generate_export_json_sql(query, file_path, config);
        tracing::info!("[ImportExportManager] JSON 导出 SQL: {}", sql);

        let rows = conn.execute(&sql, []).map_err(|e| {
            CoreError::common(CommonError::General(format!("JSON 导出失败: {}", e)))
        })?;

        tracing::info!("[ImportExportManager] JSON 导出完成: {} 行", rows);

        Ok(rows as u64)
    }

    /// 生成导入 CSV 的 SQL 语句。
    ///
    /// # 参数
    /// - `file_path`: CSV 文件路径
    /// - `config`: 导入配置
    ///
    /// # 返回
    /// SELECT 语句，用于读取 CSV 数据
    pub fn generate_import_csv_sql(file_path: &str, config: &ImportConfig) -> String {
        let mut options = Vec::new();

        if config.auto_detect {
            options.push("auto_detect=true".to_string());
        }

        if config.has_header {
            options.push("header=true".to_string());
        }

        if let Some(delimiter) = config.delimiter {
            options.push(format!("delimiter='{}'", delimiter));
        }

        options.extend(config.extra_options.clone());

        let options_str = if options.is_empty() {
            String::new()
        } else {
            format!(", {}", options.join(", "))
        };

        let escaped_path = file_path.replace('\'', "''");
        format!(
            "SELECT * FROM read_csv_auto('{}'{})",
            escaped_path, options_str
        )
    }

    /// 生成导入 Parquet 的 SQL 语句。
    ///
    /// # 参数
    /// - `file_path`: Parquet 文件路径
    ///
    /// # 返回
    /// SELECT 语句，用于读取 Parquet 数据
    pub fn generate_import_parquet_sql(file_path: &str) -> String {
        let escaped_path = file_path.replace('\'', "''");
        format!("SELECT * FROM read_parquet('{}')", escaped_path)
    }

    /// 生成导入 JSON 的 SQL 语句。
    ///
    /// # 参数
    /// - `file_path`: JSON 文件路径
    /// - `config`: 导入配置
    ///
    /// # 返回
    /// SELECT 语句，用于读取 JSON 数据
    pub fn generate_import_json_sql(file_path: &str, config: &ImportConfig) -> String {
        let mut options = Vec::new();

        if config.auto_detect {
            options.push("auto_detect=true".to_string());
        }

        options.extend(config.extra_options.clone());

        let options_str = if options.is_empty() {
            String::new()
        } else {
            format!(", {}", options.join(", "))
        };

        format!(
            "SELECT * FROM read_json_auto('{}'{})",
            file_path, options_str
        )
    }

    /// 生成将查询结果导入到表的 SQL 语句。
    ///
    /// # 参数
    /// - `query`: 源查询 SQL
    /// - `table_name`: 目标表名
    ///
    /// # 返回
    /// CREATE TABLE AS 语句
    pub fn generate_import_to_table_sql(query: &str, table_name: &str) -> String {
        format!("CREATE TABLE {} AS {}", table_name, query)
    }

    /// 生成导出 CSV 的 SQL 语句。
    ///
    /// # 参数
    /// - `query`: 要导出的查询 SQL
    /// - `file_path`: 导出文件路径
    /// - `config`: 导出配置
    ///
    /// # 返回
    /// COPY TO 语句
    pub fn generate_export_csv_sql(query: &str, file_path: &str, config: &ExportConfig) -> String {
        let mut options = Vec::new();

        if config.header {
            options.push("HEADER".to_string());
        }

        if let Some(ref compression) = config.compression {
            options.push(format!("COMPRESSION '{}'", compression));
        }

        options.extend(config.extra_options.clone());

        let options_str = if options.is_empty() {
            String::new()
        } else {
            format!(" ({})", options.join(", "))
        };

        format!("COPY ({}) TO '{}'{}", query, file_path, options_str)
    }

    /// 生成导出 Parquet 的 SQL 语句。
    ///
    /// # 参数
    /// - `query`: 要导出的查询 SQL
    /// - `file_path`: 导出文件路径
    /// - `config`: 导出配置
    ///
    /// # 返回
    /// COPY TO 语句
    pub fn generate_export_parquet_sql(
        query: &str,
        file_path: &str,
        config: &ExportConfig,
    ) -> String {
        let mut options = Vec::new();

        if let Some(ref compression) = config.compression {
            options.push(format!("COMPRESSION '{}'", compression));
        }

        options.extend(config.extra_options.clone());

        let options_str = if options.is_empty() {
            String::new()
        } else {
            format!(" ({})", options.join(", "))
        };

        format!("COPY ({}) TO '{}'{}", query, file_path, options_str)
    }

    /// 生成导出 JSON 的 SQL 语句。
    ///
    /// # 参数
    /// - `query`: 要导出的查询 SQL
    /// - `file_path`: 导出文件路径
    /// - `config`: 导出配置
    ///
    /// # 返回
    /// COPY TO 语句
    pub fn generate_export_json_sql(query: &str, file_path: &str, config: &ExportConfig) -> String {
        let mut options = vec!["FORMAT 'json'".to_string()];

        if let Some(ref compression) = config.compression {
            options.push(format!("COMPRESSION '{}'", compression));
        }

        options.extend(config.extra_options.clone());

        let options_str = if options.is_empty() {
            String::new()
        } else {
            format!(" ({})", options.join(", "))
        };

        format!("COPY ({}) TO '{}'{}", query, file_path, options_str)
    }

    /// 根据文件路径自动检测数据格式。
    ///
    /// # 参数
    /// - `file_path`: 文件路径
    ///
    /// # 返回
    /// 数据格式，无法识别时返回 None
    pub fn detect_format(file_path: &str) -> Option<DataFormat> {
        let path = Path::new(file_path);
        let extension = path.extension()?.to_str()?.to_lowercase();

        match extension.as_str() {
            "csv" => Some(DataFormat::CSV),
            "parquet" => Some(DataFormat::Parquet),
            "json" => Some(DataFormat::JSON),
            "xlsx" | "xls" => Some(DataFormat::Excel),
            _ => None,
        }
    }

    /// 验证文件路径是否存在且可读。
    ///
    /// # 参数
    /// - `file_path`: 文件路径
    ///
    /// # 返回
    /// - `Ok(())`: 文件存在且可读
    /// - `Err(CoreError)`: 文件不存在或不可读
    pub fn validate_file_path(file_path: &str) -> Result<(), CoreError> {
        let path = Path::new(file_path);

        if !path.exists() {
            return Err(CoreError::common(CommonError::General(format!(
                "文件不存在: {}",
                file_path
            ))));
        }

        if !path.is_file() {
            return Err(CoreError::common(CommonError::General(format!(
                "路径不是文件: {}",
                file_path
            ))));
        }

        Ok(())
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_format_display() {
        assert_eq!(format!("{}", DataFormat::CSV), "csv");
        assert_eq!(format!("{}", DataFormat::Parquet), "parquet");
        assert_eq!(format!("{}", DataFormat::JSON), "json");
        assert_eq!(format!("{}", DataFormat::Excel), "xlsx");
    }

    #[test]
    fn test_generate_import_csv_sql_default() {
        let config = ImportConfig::default();
        let sql = ImportExportManager::generate_import_csv_sql("data.csv", &config);
        assert!(sql.contains("read_csv_auto"));
        assert!(sql.contains("auto_detect=true"));
        assert!(sql.contains("header=true"));
    }

    #[test]
    fn test_generate_import_csv_sql_custom_delimiter() {
        let config = ImportConfig {
            delimiter: Some(';'),
            ..Default::default()
        };
        let sql = ImportExportManager::generate_import_csv_sql("data.csv", &config);
        assert!(sql.contains("delimiter=';'"));
    }

    #[test]
    fn test_generate_import_parquet_sql() {
        let sql = ImportExportManager::generate_import_parquet_sql("data.parquet");
        assert_eq!(sql, "SELECT * FROM read_parquet('data.parquet')");
    }

    #[test]
    fn test_generate_import_json_sql() {
        let config = ImportConfig::default();
        let sql = ImportExportManager::generate_import_json_sql("data.json", &config);
        assert!(sql.contains("read_json_auto"));
        assert!(sql.contains("auto_detect=true"));
    }

    #[test]
    fn test_generate_import_to_table_sql() {
        let sql = ImportExportManager::generate_import_to_table_sql(
            "SELECT * FROM read_csv_auto('data.csv')",
            "my_table",
        );
        assert_eq!(
            sql,
            "CREATE TABLE my_table AS SELECT * FROM read_csv_auto('data.csv')"
        );
    }

    #[test]
    fn test_generate_export_csv_sql_default() {
        let config = ExportConfig::default();
        let sql = ImportExportManager::generate_export_csv_sql(
            "SELECT * FROM users",
            "output.csv",
            &config,
        );
        assert!(sql.contains("COPY (SELECT * FROM users) TO 'output.csv'"));
        assert!(sql.contains("HEADER"));
    }

    #[test]
    fn test_generate_export_csv_sql_with_compression() {
        let config = ExportConfig {
            compression: Some("gzip".to_string()),
            ..Default::default()
        };
        let sql = ImportExportManager::generate_export_csv_sql(
            "SELECT * FROM users",
            "output.csv.gz",
            &config,
        );
        assert!(sql.contains("COMPRESSION 'gzip'"));
    }

    #[test]
    fn test_generate_export_parquet_sql() {
        let config = ExportConfig::default();
        let sql = ImportExportManager::generate_export_parquet_sql(
            "SELECT * FROM users",
            "output.parquet",
            &config,
        );
        assert!(sql.contains("COPY (SELECT * FROM users) TO 'output.parquet'"));
    }

    #[test]
    fn test_generate_export_json_sql() {
        let config = ExportConfig::default();
        let sql = ImportExportManager::generate_export_json_sql(
            "SELECT * FROM users",
            "output.json",
            &config,
        );
        assert!(sql.contains("FORMAT 'json'"));
    }

    #[test]
    fn test_detect_format() {
        assert_eq!(
            ImportExportManager::detect_format("data.csv"),
            Some(DataFormat::CSV)
        );
        assert_eq!(
            ImportExportManager::detect_format("data.parquet"),
            Some(DataFormat::Parquet)
        );
        assert_eq!(
            ImportExportManager::detect_format("data.json"),
            Some(DataFormat::JSON)
        );
        assert_eq!(
            ImportExportManager::detect_format("data.xlsx"),
            Some(DataFormat::Excel)
        );
        assert_eq!(ImportExportManager::detect_format("data.txt"), None);
    }

    #[test]
    fn test_validate_file_path_exists() {
        // 使用当前文件作为测试
        let result = ImportExportManager::validate_file_path(file!());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_path_not_exists() {
        let result = ImportExportManager::validate_file_path("/nonexistent/file.csv");
        assert!(result.is_err());
    }
}
