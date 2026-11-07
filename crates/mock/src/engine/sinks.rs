// ==================== 出口方法 ====================

use super::*;

impl MockEngine {
    /// 将临时表保存到用户表配置本
    ///
    /// 按 `mock_{表名}_{时间戳}.{ext}` 写入给定目录，返回**落盘文件路径**
    /// （供面板显示具体位置）。
    pub fn save_to_scratchpad(
        temp_table_name: &str,
        display_name: &str,
        format: &MockExportFormat,
        scratchpad_dir: &str,
    ) -> MockResult<String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        // 文件名用用户原始输入的表名（显示名），而不是清理后的临时表名
        let base_name = display_name.trim();
        let ext = match format {
            MockExportFormat::Csv => "csv",
            MockExportFormat::Parquet => "parquet",
            MockExportFormat::Xlsx => "xlsx",
            MockExportFormat::SqlInsert => "sql",
            MockExportFormat::Table => "duckdb",
        };
        let file_name = format!("mock_{}_{}.{}", base_name, timestamp, ext);
        let output_path = format!("{}/{}", scratchpad_dir, file_name);

        Self::export(temp_table_name, format, Some(&output_path), None)?;
        Ok(output_path)
    }
}

