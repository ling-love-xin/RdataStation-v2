//! Round 27：SQL 查询结果导出——把 `QueryOutput` 落盘为 CSV。
//!
//! 手写 CSV 转义（字段含逗号/引号/换行时加引号包裹、内部引号双写），
//! 无第三方 csv 依赖；默认导出到全局目录下 `results/`（按秒时间戳命名）。

use std::path::{Path, PathBuf};

use crate::services::query_runner::QueryOutput;

/// CSV 字段转义：含 `,` `"` `\r` `\n` 时加引号包裹，内部 `"` 双写。
fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// 把查询结果写出为 CSV（列头 + 数据行）。
pub fn export_csv(output: &QueryOutput, path: &Path) -> Result<(), String> {
    let mut w = String::new();
    let cols: Vec<String> = output.columns.iter().map(|c| csv_field(c)).collect();
    w.push_str(&cols.join(","));
    w.push('\n');
    for row in &output.rows {
        let fields: Vec<String> = row.iter().map(|v| csv_field(v)).collect();
        w.push_str(&fields.join(","));
        w.push('\n');
    }
    std::fs::write(path, w).map_err(|e| format!("导出失败: {e}"))
}

/// 默认导出目录：全局目录下 `results/`。
pub fn default_export_dir() -> PathBuf {
    crate::services::workspace_loader::default_global_dir().join("results")
}

/// 以秒时间戳命名导出到默认目录，返回落盘路径。
pub fn export_to_default(output: &QueryOutput) -> Result<PathBuf, String> {
    let dir = default_export_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建导出目录失败: {e}"))?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("rds_query_{secs}.csv"));
    export_csv(output, &path)?;
    Ok(path)
}
