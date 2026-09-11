//! Round 29：SQL 历史记录——最近执行的 SQL 持久化到全局目录 `query_history.json`。
//!
//! 结构为简单 JSON 数组（最新在前），去重（同 SQL 移动到最前），上限 20 条；
//! 文件不存在 / 损坏时按空历史处理（load 不报错，append 会重建）。
//! 目录可注入（`*_at` 变体），便于测试与未来多项目隔离。

use std::path::{Path, PathBuf};

/// 历史文件路径：`<全局目录>/query_history.json`。
pub fn history_path() -> PathBuf {
    default_history_dir().join("query_history.json")
}

/// 默认历史目录：全局目录。
pub fn default_history_dir() -> PathBuf {
    crate::services::workspace_loader::default_global_dir()
}

/// 读取指定目录下的历史（最新在前）。文件缺失 / JSON 损坏 → 空列表。
pub fn load_history_from(dir: &Path) -> Vec<String> {
    let path = dir.join("query_history.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<Vec<String>>(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 读取默认目录历史。
pub fn load_history() -> Vec<String> {
    load_history_from(&default_history_dir())
}

/// 追加一条 SQL 到指定目录：去重（相同项移到最前）→ 插入头部 → 截断到 20 条 → 写回。
/// 返回写回后的完整历史（最新在前）。
pub fn append_history_at(dir: &Path, sql: &str) -> Result<Vec<String>, String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Ok(load_history_from(dir));
    }

    let mut hist = load_history_from(dir);
    // 去重：移除相同内容（保留原顺序）
    hist.retain(|h| h != trimmed);
    hist.insert(0, trimmed.to_string());
    if hist.len() > 20 {
        hist.truncate(20);
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let json = serde_json::to_string_pretty(&hist).map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(dir.join("query_history.json"), json)
        .map_err(|e| format!("写入历史失败: {e}"))?;
    Ok(hist)
}

/// 追加一条 SQL 到默认目录。
pub fn append_history(sql: &str) -> Result<Vec<String>, String> {
    append_history_at(&default_history_dir(), sql)
}
