//! SQL 历史记录存储模块
//!
//! 负责 SQL 执行历史的持久化存储，支持查询、筛选、统计等功能。
//! 存储格式为 JSON，便于人工查看和调试。

use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn system_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn system_time_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 最大保存的历史记录数量
const MAX_HISTORY_RECORDS: usize = 500;

/// 默认返回的历史记录数量
const DEFAULT_LIMIT: usize = 100;

/// SQL 历史记录
///
/// 存储单条 SQL 执行记录的元数据
#[derive(Debug, Clone)]
pub struct HistoryRecord {
    pub id: String,
    pub sql: String,
    pub db_type: String,
    pub connection_id: String,
    pub timestamp: u64,
    pub duration_ms: u64,
    pub success: bool,
    pub error_message: Option<String>,
    pub rows_affected: Option<u64>,
    pub rows_returned: Option<u64>,
}

impl HistoryRecord {
    /// 创建新的历史记录
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL 语句
    /// * `db_type` - 数据库类型
    /// * `connection_id` - 连接 ID
    ///
    /// # Returns
    ///
    /// 返回新的 HistoryRecord 实例
    pub fn new(sql: String, db_type: String, connection_id: String) -> Self {
        let now = system_time_millis();

        Self {
            id: Self::generate_id(),
            sql,
            db_type,
            connection_id,
            timestamp: now,
            duration_ms: 0,
            success: false,
            error_message: None,
            rows_affected: None,
            rows_returned: None,
        }
    }

    /// 生成唯一 ID
    fn generate_id() -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let timestamp = system_time_nanos();

        let mut hasher = DefaultHasher::new();
        timestamp.hash(&mut hasher);
        std::process::id().hash(&mut hasher);

        format!("{:x}", hasher.finish())
    }

    /// 标记执行成功
    ///
    /// # Arguments
    ///
    /// * `duration_ms` - 执行耗时（毫秒）
    pub fn mark_success(&mut self, duration_ms: u64) {
        self.success = true;
        self.duration_ms = duration_ms;
    }

    /// 标记执行失败
    ///
    /// # Arguments
    ///
    /// * `error_message` - 错误信息
    /// * `duration_ms` - 执行耗时（毫秒）
    pub fn mark_failed(&mut self, error_message: String, duration_ms: u64) {
        self.success = false;
        self.error_message = Some(error_message);
        self.duration_ms = duration_ms;
    }

    /// 设置影响的行数
    pub fn set_rows_affected(&mut self, count: u64) {
        self.rows_affected = Some(count);
    }

    /// 设置返回的行数
    pub fn set_rows_returned(&mut self, count: u64) {
        self.rows_returned = Some(count);
    }

    /// 获取执行时间（本地时间字符串）
    pub fn formatted_time(&self) -> String {
        // 简化的格式化，实际项目中可以使用 chrono
        let secs = self.timestamp / 1000;
        format!("{} ({} ms)", secs, self.duration_ms)
    }

    /// 获取 SQL 预览（前 100 个字符）
    pub fn sql_preview(&self) -> String {
        if self.sql.len() > 100 {
            format!("{}...", &self.sql[..100])
        } else {
            self.sql.clone()
        }
    }
}

/// 历史记录筛选条件
#[derive(Debug, Default, Clone)]
pub struct HistoryFilter {
    /// 数据库类型筛选
    pub db_type: Option<String>,
    /// 连接 ID 筛选
    pub connection_id: Option<String>,
    /// 只显示成功的记录
    pub success_only: bool,
    /// 只显示失败的记录
    pub failed_only: bool,
    /// 时间范围开始（Unix 时间戳，毫秒）
    pub start_time: Option<u64>,
    /// 时间范围结束（Unix 时间戳，毫秒）
    pub end_time: Option<u64>,
    /// 搜索关键词（匹配 SQL 内容）
    pub keyword: Option<String>,
}

impl HistoryFilter {
    /// 创建空的筛选条件
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置数据库类型筛选
    pub fn with_db_type(mut self, db_type: String) -> Self {
        self.db_type = Some(db_type);
        self
    }

    /// 设置连接 ID 筛选
    pub fn with_connection_id(mut self, connection_id: String) -> Self {
        self.connection_id = Some(connection_id);
        self
    }

    /// 只显示成功的记录
    pub fn success_only(mut self) -> Self {
        self.success_only = true;
        self.failed_only = false;
        self
    }

    /// 只显示失败的记录
    pub fn failed_only(mut self) -> Self {
        self.failed_only = true;
        self.success_only = false;
        self
    }

    /// 设置时间范围
    pub fn with_time_range(mut self, start: u64, end: u64) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    /// 设置搜索关键词
    pub fn with_keyword(mut self, keyword: String) -> Self {
        self.keyword = Some(keyword);
        self
    }

    /// 检查记录是否匹配筛选条件
    fn matches(&self, record: &HistoryRecord) -> bool {
        // 数据库类型筛选
        if let Some(ref db_type) = self.db_type {
            if record.db_type != *db_type {
                return false;
            }
        }

        // 连接 ID 筛选
        if let Some(ref conn_id) = self.connection_id {
            if record.connection_id != *conn_id {
                return false;
            }
        }

        // 成功/失败筛选
        if self.success_only && !record.success {
            return false;
        }
        if self.failed_only && record.success {
            return false;
        }

        // 时间范围筛选
        if let Some(start) = self.start_time {
            if record.timestamp < start {
                return false;
            }
        }
        if let Some(end) = self.end_time {
            if record.timestamp > end {
                return false;
            }
        }

        // 关键词搜索
        if let Some(ref keyword) = self.keyword {
            let keyword_lower = keyword.to_lowercase();
            if !record.sql.to_lowercase().contains(&keyword_lower) {
                return false;
            }
        }

        true
    }
}

/// SQL 历史存储
///
/// 管理 SQL 执行历史记录列表，支持添加、查询、筛选等操作
pub struct HistoryStore {
    /// 存储文件路径
    path: PathBuf,
    /// 历史记录列表（按时间倒序，最新的在前）
    records: Vec<HistoryRecord>,
    /// 是否已修改（用于延迟保存优化）
    modified: bool,
}

impl HistoryStore {
    /// 创建新的历史存储
    ///
    /// # Arguments
    ///
    /// * `path` - 存储文件路径
    ///
    /// # Returns
    ///
    /// 返回新的 HistoryStore 实例
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            records: Vec::new(),
            modified: false,
        }
    }

    /// 从文件加载历史记录
    ///
    /// # Returns
    ///
    /// 如果加载成功返回 Ok(())，否则返回 IO 错误
    pub fn load(&mut self) -> std::io::Result<()> {
        if !self.path.exists() {
            self.records = Vec::new();
            self.modified = false;
            return Ok(());
        }

        let mut file = File::open(&self.path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        if contents.trim().is_empty() {
            self.records = Vec::new();
        } else {
            self.records = Self::parse_history_json(&contents)?;
        }

        self.modified = false;
        Ok(())
    }

    /// 保存历史记录到文件
    ///
    /// # Returns
    ///
    /// 如果保存成功返回 Ok(())，否则返回 IO 错误
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let json = Self::serialize_history(&self.records);
        let mut file = File::create(&self.path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;

        Ok(())
    }

    /// 添加历史记录
    ///
    /// 新记录会被添加到列表开头（最新的在前）。
    ///
    /// # Arguments
    ///
    /// * `record` - 历史记录
    pub fn add_record(&mut self, record: HistoryRecord) {
        self.records.insert(0, record);

        // 限制记录数量
        if self.records.len() > MAX_HISTORY_RECORDS {
            self.records.truncate(MAX_HISTORY_RECORDS);
        }

        self.modified = true;
    }

    /// 获取所有历史记录
    ///
    /// # Arguments
    ///
    /// * `limit` - 最大返回数量（默认 100）
    ///
    /// # Returns
    ///
    /// 返回历史记录列表
    pub fn get_records(&self, limit: Option<usize>) -> &[HistoryRecord] {
        let limit = limit.unwrap_or(DEFAULT_LIMIT).min(self.records.len());
        &self.records[..limit]
    }

    /// 根据 ID 获取历史记录
    ///
    /// # Arguments
    ///
    /// * `id` - 记录 ID
    ///
    /// # Returns
    ///
    /// 如果记录存在返回 Some(&HistoryRecord)，否则返回 None
    pub fn get_record(&self, id: &str) -> Option<&HistoryRecord> {
        self.records.iter().find(|r| r.id == id)
    }

    /// 筛选历史记录
    ///
    /// # Arguments
    ///
    /// * `filter` - 筛选条件
    /// * `limit` - 最大返回数量
    ///
    /// # Returns
    ///
    /// 返回匹配筛选条件的历史记录列表
    pub fn filter_records(
        &self,
        filter: &HistoryFilter,
        limit: Option<usize>,
    ) -> Vec<&HistoryRecord> {
        let limit = limit.unwrap_or(DEFAULT_LIMIT);
        self.records
            .iter()
            .filter(|r| filter.matches(r))
            .take(limit)
            .collect()
    }

    /// 获取指定数据库类型的历史记录
    ///
    /// # Arguments
    ///
    /// * `db_type` - 数据库类型
    /// * `limit` - 最大返回数量
    ///
    /// # Returns
    ///
    /// 返回匹配类型的历史记录列表
    pub fn get_records_by_db_type(
        &self,
        db_type: &str,
        limit: Option<usize>,
    ) -> Vec<&HistoryRecord> {
        let filter = HistoryFilter::new().with_db_type(db_type.to_string());
        self.filter_records(&filter, limit)
    }

    /// 获取指定连接的历史记录
    ///
    /// # Arguments
    ///
    /// * `connection_id` - 连接 ID
    /// * `limit` - 最大返回数量
    ///
    /// # Returns
    ///
    /// 返回匹配连接的历史记录列表
    pub fn get_records_by_connection(
        &self,
        connection_id: &str,
        limit: Option<usize>,
    ) -> Vec<&HistoryRecord> {
        let filter = HistoryFilter::new().with_connection_id(connection_id.to_string());
        self.filter_records(&filter, limit)
    }

    /// 搜索历史记录
    ///
    /// 根据 SQL 内容进行模糊搜索
    ///
    /// # Arguments
    ///
    /// * `query` - 搜索关键词
    /// * `limit` - 最大返回数量
    ///
    /// # Returns
    ///
    /// 返回匹配条件的历史记录列表
    pub fn search(&self, query: &str, limit: Option<usize>) -> Vec<&HistoryRecord> {
        let filter = HistoryFilter::new().with_keyword(query.to_string());
        self.filter_records(&filter, limit)
    }

    /// 获取最近的 SQL 语句（去重）
    ///
    /// # Arguments
    ///
    /// * `limit` - 最大返回数量
    ///
    /// # Returns
    ///
    /// 返回去重后的 SQL 语句列表
    pub fn get_recent_sql(&self, limit: Option<usize>) -> Vec<String> {
        let limit = limit.unwrap_or(10);
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for record in &self.records {
            let sql_normalized = record.sql.trim().to_lowercase();
            if seen.insert(sql_normalized.clone()) && !sql_normalized.is_empty() {
                result.push(record.sql.clone());
                if result.len() >= limit {
                    break;
                }
            }
        }

        result
    }

    /// 获取执行统计信息
    ///
    /// # Returns
    ///
    /// 返回 (总记录数, 成功数, 失败数, 平均执行时间)
    pub fn get_statistics(&self) -> (usize, usize, usize, u64) {
        let total = self.records.len();
        let success_count = self.records.iter().filter(|r| r.success).count();
        let failed_count = total - success_count;

        let avg_duration = if total > 0 {
            self.records.iter().map(|r| r.duration_ms).sum::<u64>() / total as u64
        } else {
            0
        };

        (total, success_count, failed_count, avg_duration)
    }

    /// 删除指定记录
    ///
    /// # Arguments
    ///
    /// * `id` - 记录 ID
    ///
    /// # Returns
    ///
    /// 如果删除成功返回 true，否则返回 false
    pub fn remove_record(&mut self, id: &str) -> bool {
        let len_before = self.records.len();
        self.records.retain(|r| r.id != id);
        let removed = self.records.len() < len_before;

        if removed {
            self.modified = true;
        }

        removed
    }

    /// 清空所有历史记录
    pub fn clear(&mut self) {
        if !self.records.is_empty() {
            self.records.clear();
            self.modified = true;
        }
    }

    /// 获取记录数量
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 检查是否已修改
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    // ========== JSON 序列化/反序列化（手动实现，不依赖 serde） ==========

    /// 解析历史记录列表 JSON
    fn parse_history_json(json: &str) -> std::io::Result<Vec<HistoryRecord>> {
        let mut records = Vec::new();

        let json = json.trim();
        if json == "[]" || json.is_empty() {
            return Ok(records);
        }

        let json = json.strip_prefix('[').unwrap_or(json);
        let json = json.strip_suffix(']').unwrap_or(json);

        let mut depth = 0;
        let mut start = 0;
        let mut in_string = false;
        let mut escape_next = false;

        for (i, c) in json.char_indices() {
            if escape_next {
                escape_next = false;
                continue;
            }

            match c {
                '\\' if in_string => escape_next = true,
                '"' => in_string = !in_string,
                '{' if !in_string => {
                    if depth == 0 {
                        start = i;
                    }
                    depth += 1;
                }
                '}' if !in_string => {
                    depth -= 1;
                    if depth == 0 {
                        let obj_str = &json[start..=i];
                        if let Ok(record) = Self::parse_record_json(obj_str) {
                            records.push(record);
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(records)
    }

    /// 解析单条历史记录 JSON 对象
    fn parse_record_json(json: &str) -> std::io::Result<HistoryRecord> {
        let extract_string = |json: &str, key: &str| -> Option<String> {
            let pattern = format!("\"{}\":", key);
            if let Some(pos) = json.find(&pattern) {
                let start = pos + pattern.len();
                let rest = &json[start..];
                let rest = rest.trim_start();
                #[allow(clippy::manual_strip)]
                if rest.starts_with('"') {
                    let mut end = 1;
                    let mut escape = false;
                    for c in rest[1..].chars() {
                        if escape {
                            escape = false;
                            end += 1;
                        } else if c == '\\' {
                            escape = true;
                            end += 1;
                        } else if c == '"' {
                            break;
                        } else {
                            end += 1;
                        }
                    }
                    return Some(rest[1..end].to_string());
                }
            }
            None
        };

        let extract_u64 = |json: &str, key: &str| -> Option<u64> {
            let pattern = format!("\"{}\":", key);
            if let Some(pos) = json.find(&pattern) {
                let start = pos + pattern.len();
                let rest = &json[start..];
                let rest = rest.trim_start();
                let end = rest
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(rest.len());
                return rest[..end].parse().ok();
            }
            None
        };

        let extract_bool = |json: &str, key: &str| -> Option<bool> {
            let pattern = format!("\"{}\":", key);
            if let Some(pos) = json.find(&pattern) {
                let start = pos + pattern.len();
                let rest = &json[start..];
                let rest = rest.trim_start();
                if rest.starts_with("true") {
                    return Some(true);
                } else if rest.starts_with("false") {
                    return Some(false);
                }
            }
            None
        };

        let id = extract_string(json, "id").unwrap_or_default();
        let sql = extract_string(json, "sql").unwrap_or_default();
        let db_type = extract_string(json, "db_type").unwrap_or_default();
        let connection_id = extract_string(json, "connection_id").unwrap_or_default();

        if id.is_empty() || sql.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Missing required fields",
            ));
        }

        Ok(HistoryRecord {
            id,
            sql,
            db_type,
            connection_id,
            timestamp: extract_u64(json, "timestamp").unwrap_or(0),
            duration_ms: extract_u64(json, "duration_ms").unwrap_or(0),
            success: extract_bool(json, "success").unwrap_or(false),
            error_message: extract_string(json, "error_message"),
            rows_affected: extract_u64(json, "rows_affected"),
            rows_returned: extract_u64(json, "rows_returned"),
        })
    }

    /// 序列化历史记录列表为 JSON
    fn serialize_history(records: &[HistoryRecord]) -> String {
        if records.is_empty() {
            return "[]".to_string();
        }

        let mut result = String::from("[\n");
        for (i, record) in records.iter().enumerate() {
            result.push_str("  {\n");
            result.push_str(&format!(
                "    \"id\": \"{}\",\n",
                Self::escape_json(&record.id)
            ));
            result.push_str(&format!(
                "    \"sql\": \"{}\",\n",
                Self::escape_json(&record.sql)
            ));
            result.push_str(&format!(
                "    \"db_type\": \"{}\",\n",
                Self::escape_json(&record.db_type)
            ));
            result.push_str(&format!(
                "    \"connection_id\": \"{}\",\n",
                Self::escape_json(&record.connection_id)
            ));
            result.push_str(&format!("    \"timestamp\": {},\n", record.timestamp));
            result.push_str(&format!("    \"duration_ms\": {},\n", record.duration_ms));
            result.push_str(&format!("    \"success\": {},\n", record.success));

            if let Some(ref err) = record.error_message {
                result.push_str(&format!(
                    "    \"error_message\": \"{}\",\n",
                    Self::escape_json(err)
                ));
            }

            if let Some(rows) = record.rows_affected {
                result.push_str(&format!("    \"rows_affected\": {},\n", rows));
            }

            if let Some(rows) = record.rows_returned {
                result.push_str(&format!("    \"rows_returned\": {},\n", rows));
            }

            // 移除最后一个逗号
            if result.ends_with(",\n") {
                result.pop();
                result.pop();
                result.push('\n');
            }

            result.push_str("  }");
            if i < records.len() - 1 {
                result.push(',');
            }
            result.push('\n');
        }
        result.push(']');
        result
    }

    /// 转义 JSON 字符串中的特殊字符
    fn escape_json(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                '"' => "\\\"".to_string(),
                '\\' => "\\\\".to_string(),
                '\n' => "\\n".to_string(),
                '\r' => "\\r".to_string(),
                '\t' => "\\t".to_string(),
                c => c.to_string(),
            })
            .collect()
    }
}

// ==================== 全局便捷函数 ====================

use once_cell::sync::Lazy;
use std::sync::Mutex;

/// 全局历史存储实例
static GLOBAL_HISTORY_STORE: Lazy<Mutex<HistoryStore>> = Lazy::new(|| {
    let path = get_default_history_path();
    let mut store = HistoryStore::new(path);
    // 尝试加载已有数据
    let _ = store.load();
    Mutex::new(store)
});

/// 获取默认历史存储路径
fn get_default_history_path() -> PathBuf {
    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("RdataStation");

    // 确保目录存在
    let _ = std::fs::create_dir_all(&app_dir);

    app_dir.join("sql_history.json")
}

/// SQL 历史记录（用于服务层）
#[derive(Debug, Clone)]
pub struct SqlHistoryRecord {
    pub id: String,
    pub sql: String,
    pub conn_id: Option<String>,
    pub db_type: Option<String>,
    pub executed_at: chrono::DateTime<chrono::Utc>,
    pub duration_ms: Option<u64>,
    pub success: Option<bool>,
    pub error_message: Option<String>,
    pub rows_affected: Option<u64>,
    pub rows_returned: Option<u64>,
}

/// 保存 SQL 历史
pub fn save_sql_history(sql: &str, conn_id: Option<&str>) -> Result<(), std::io::Error> {
    let mut store = GLOBAL_HISTORY_STORE
        .lock()
        .map_err(|_| std::io::Error::other("Failed to lock store"))?;

    // 使用默认的数据库类型和连接 ID
    let db_type = "unknown".to_string();
    let connection_id = conn_id.unwrap_or("unknown").to_string();

    let mut record = HistoryRecord::new(sql.to_string(), db_type, connection_id);
    record.mark_success(0); // 标记为成功，耗时未知

    store.add_record(record);
    store.save()
}

/// 获取 SQL 历史列表
pub fn get_sql_history(limit: usize) -> Result<Vec<SqlHistoryRecord>, std::io::Error> {
    let mut store = GLOBAL_HISTORY_STORE
        .lock()
        .map_err(|_| std::io::Error::other("Failed to lock store"))?;

    store.load()?;

    let records: Vec<SqlHistoryRecord> = store
        .get_records(Some(limit))
        .iter()
        .map(|r| SqlHistoryRecord {
            id: r.id.clone(),
            sql: r.sql.clone(),
            conn_id: if r.connection_id == "unknown" {
                None
            } else {
                Some(r.connection_id.clone())
            },
            db_type: if r.db_type == "unknown" {
                None
            } else {
                Some(r.db_type.clone())
            },
            executed_at: chrono::DateTime::from_timestamp((r.timestamp / 1000) as i64, 0)
                .unwrap_or_else(chrono::Utc::now),
            duration_ms: Some(r.duration_ms),
            success: Some(r.success),
            error_message: r.error_message.clone(),
            rows_affected: r.rows_affected,
            rows_returned: r.rows_returned,
        })
        .collect();

    Ok(records)
}

/// 搜索 SQL 历史
pub fn search_sql_history(
    keyword: &str,
    limit: usize,
) -> Result<Vec<SqlHistoryRecord>, std::io::Error> {
    let mut store = GLOBAL_HISTORY_STORE
        .lock()
        .map_err(|_| std::io::Error::other("Failed to lock store"))?;

    store.load()?;

    let results = store.search(keyword, Some(limit));

    let records: Vec<SqlHistoryRecord> = results
        .into_iter()
        .map(|r| SqlHistoryRecord {
            id: r.id.clone(),
            sql: r.sql.clone(),
            conn_id: if r.connection_id == "unknown" {
                None
            } else {
                Some(r.connection_id.clone())
            },
            db_type: if r.db_type == "unknown" {
                None
            } else {
                Some(r.db_type.clone())
            },
            executed_at: chrono::DateTime::from_timestamp((r.timestamp / 1000) as i64, 0)
                .unwrap_or_else(chrono::Utc::now),
            duration_ms: Some(r.duration_ms),
            success: Some(r.success),
            error_message: r.error_message.clone(),
            rows_affected: r.rows_affected,
            rows_returned: r.rows_returned,
        })
        .collect();

    Ok(records)
}

/// 清空 SQL 历史
pub fn clear_sql_history() -> Result<(), std::io::Error> {
    let mut store = GLOBAL_HISTORY_STORE
        .lock()
        .map_err(|_| std::io::Error::other("Failed to lock store"))?;

    store.clear();
    store.save()
}

/// 删除单条 SQL 历史
pub fn remove_sql_history(id: &str) -> Result<(), std::io::Error> {
    let mut store = GLOBAL_HISTORY_STORE
        .lock()
        .map_err(|_| std::io::Error::other("Failed to lock store"))?;

    store.remove_record(id);
    store.save()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_history_record_new() {
        let record = HistoryRecord::new(
            "SELECT * FROM users".to_string(),
            "mysql".to_string(),
            "conn-1".to_string(),
        );

        assert!(!record.id.is_empty());
        assert_eq!(record.sql, "SELECT * FROM users");
        assert_eq!(record.db_type, "mysql");
        assert_eq!(record.connection_id, "conn-1");
        assert!(!record.success);
    }

    #[test]
    fn test_history_record_mark_success() {
        let mut record = HistoryRecord::new(
            "SELECT 1".to_string(),
            "postgres".to_string(),
            "conn-1".to_string(),
        );

        record.mark_success(150);
        assert!(record.success);
        assert_eq!(record.duration_ms, 150);
        assert!(record.error_message.is_none());
    }

    #[test]
    fn test_history_record_mark_failed() {
        let mut record = HistoryRecord::new(
            "INVALID SQL".to_string(),
            "mysql".to_string(),
            "conn-1".to_string(),
        );

        record.mark_failed("Syntax error".to_string(), 50);
        assert!(!record.success);
        assert_eq!(record.duration_ms, 50);
        assert_eq!(record.error_message, Some("Syntax error".to_string()));
    }

    #[test]
    fn test_history_store_add_and_get() {
        let mut store = HistoryStore::new(PathBuf::from("/tmp/test_history.json"));

        let record = HistoryRecord::new(
            "SELECT * FROM users".to_string(),
            "mysql".to_string(),
            "conn-1".to_string(),
        );

        store.add_record(record);

        assert_eq!(store.len(), 1);
        assert!(store.is_modified());
    }

    #[test]
    fn test_history_store_filter() {
        let mut store = HistoryStore::new(PathBuf::from("/tmp/test_history.json"));

        store.add_record(HistoryRecord::new(
            "SELECT * FROM users".to_string(),
            "mysql".to_string(),
            "conn-1".to_string(),
        ));

        store.add_record(HistoryRecord::new(
            "SELECT * FROM orders".to_string(),
            "postgres".to_string(),
            "conn-2".to_string(),
        ));

        // 按数据库类型筛选
        let filter = HistoryFilter::new().with_db_type("mysql".to_string());
        let results = store.filter_records(&filter, None);
        assert_eq!(results.len(), 1);
        assert!(results[0].sql.contains("users"));

        // 按关键词搜索
        let filter = HistoryFilter::new().with_keyword("orders".to_string());
        let results = store.filter_records(&filter, None);
        assert_eq!(results.len(), 1);
        assert!(results[0].sql.contains("orders"));
    }

    #[test]
    fn test_history_store_search() {
        let mut store = HistoryStore::new(PathBuf::from("/tmp/test_history.json"));

        store.add_record(HistoryRecord::new(
            "SELECT * FROM users WHERE id = 1".to_string(),
            "mysql".to_string(),
            "conn-1".to_string(),
        ));

        store.add_record(HistoryRecord::new(
            "INSERT INTO orders VALUES (1, 'test')".to_string(),
            "postgres".to_string(),
            "conn-2".to_string(),
        ));

        let results = store.search("users", None);
        assert_eq!(results.len(), 1);
        assert!(results[0].sql.contains("users"));
    }

    #[test]
    fn test_history_store_recent_sql() {
        let mut store = HistoryStore::new(PathBuf::from("/tmp/test_history.json"));

        // 添加重复 SQL
        for _ in 0..3 {
            store.add_record(HistoryRecord::new(
                "SELECT * FROM users".to_string(),
                "mysql".to_string(),
                "conn-1".to_string(),
            ));
        }

        store.add_record(HistoryRecord::new(
            "SELECT * FROM orders".to_string(),
            "mysql".to_string(),
            "conn-1".to_string(),
        ));

        let recent = store.get_recent_sql(Some(10));
        assert_eq!(recent.len(), 2); // 去重后只有 2 条
    }

    #[test]
    fn test_history_store_statistics() {
        let mut store = HistoryStore::new(PathBuf::from("/tmp/test_history.json"));

        let mut record1 = HistoryRecord::new(
            "SELECT 1".to_string(),
            "mysql".to_string(),
            "conn-1".to_string(),
        );
        record1.mark_success(100);
        store.add_record(record1);

        let mut record2 = HistoryRecord::new(
            "SELECT 2".to_string(),
            "mysql".to_string(),
            "conn-1".to_string(),
        );
        record2.mark_failed("Error".to_string(), 50);
        store.add_record(record2);

        let (total, success, failed, avg) = store.get_statistics();
        assert_eq!(total, 2);
        assert_eq!(success, 1);
        assert_eq!(failed, 1);
        assert_eq!(avg, 75); // (100 + 50) / 2
    }

    #[test]
    fn test_json_serialization() {
        let mut store = HistoryStore::new(PathBuf::from("/tmp/test_history.json"));

        let mut record = HistoryRecord::new(
            "SELECT * FROM \"users\"".to_string(),
            "postgres".to_string(),
            "conn-1".to_string(),
        );
        record.mark_success(100);
        record.set_rows_returned(10);
        store.add_record(record);

        let json = HistoryStore::serialize_history(store.get_records(None));
        assert!(json.contains("\"sql\""));
        assert!(json.contains("SELECT * FROM \\\"users\\\"")); // 转义的引号
        assert!(json.contains("\"success\": true"));
        assert!(json.contains("\"rows_returned\": 10"));
    }
}
