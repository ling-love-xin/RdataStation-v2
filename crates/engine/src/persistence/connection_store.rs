//! 连接信息存储模块
//!
//! 负责最近使用的数据库连接信息的持久化存储。
//! 存储格式为 JSON，便于人工查看和调试。

use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn system_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 最大保存的连接数量
pub const MAX_CONNECTIONS: usize = 20;

/// 连接信息
///
/// 存储单个数据库连接的元数据信息
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// 连接唯一标识
    pub id: String,
    /// 连接显示名称
    pub name: String,
    /// 数据库类型（mysql, postgres, sqlite, duckdb 等）
    pub db_type: String,
    /// 连接 URL
    pub url: String,
    /// 连接标识（用于关联后端连接管理器）
    pub conn_id: Option<String>,
    /// 数据库服务器版本
    pub server_version: Option<String>,
    /// 最后使用时间（Unix 时间戳，秒）
    pub last_used: u64,
    /// 创建时间（Unix 时间戳，秒）
    pub created_at: u64,
    /// 连接描述
    pub description: Option<String>,
    /// 驱动 ID
    pub driver_id: Option<String>,
    /// 环境 ID
    pub environment_id: Option<String>,
    /// 认证配置 ID
    pub auth_config_id: Option<String>,
    /// 认证方式
    pub auth_method: Option<String>,
    /// 网络配置 ID
    pub network_config_id: Option<String>,
    /// 驱动属性 JSON
    pub driver_properties: Option<String>,
    /// 高级选项 JSON
    pub advanced_options: Option<String>,
}

impl ConnectionInfo {
    /// 创建新的连接信息
    ///
    /// # Arguments
    ///
    /// * `id` - 连接唯一标识
    /// * `name` - 连接显示名称
    /// * `db_type` - 数据库类型
    /// * `url` - 连接 URL
    ///
    /// # Returns
    ///
    /// 返回新的 ConnectionInfo 实例
    pub fn new(id: String, name: String, db_type: String, url: String) -> Self {
        let now = system_time_secs();

        Self {
            id,
            name,
            db_type,
            url,
            conn_id: None,
            server_version: None,
            last_used: now,
            created_at: now,
            description: None,
            driver_id: None,
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: None,
            driver_properties: None,
            advanced_options: None,
        }
    }

    /// 更新最后使用时间
    pub fn touch(&mut self) {
        self.last_used = system_time_secs();
    }

    /// 获取连接的年龄（从创建到现在的时间，秒）
    pub fn age_secs(&self) -> u64 {
        let now = system_time_secs();
        now.saturating_sub(self.created_at)
    }

    /// 获取距离上次使用的时间（秒）
    pub fn idle_secs(&self) -> u64 {
        let now = system_time_secs();
        now.saturating_sub(self.last_used)
    }
}

/// 连接存储
///
/// 管理最近使用的数据库连接列表，支持添加、删除、查询等操作
pub struct ConnectionStore {
    /// 存储文件路径
    path: PathBuf,
    /// 连接列表
    connections: Vec<ConnectionInfo>,
    /// 是否已修改（用于延迟保存优化）
    modified: bool,
}

impl ConnectionStore {
    /// 创建新的连接存储
    ///
    /// # Arguments
    ///
    /// * `path` - 存储文件路径
    ///
    /// # Returns
    ///
    /// 返回新的 ConnectionStore 实例
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            connections: Vec::new(),
            modified: false,
        }
    }

    /// 从文件加载连接信息
    ///
    /// # Returns
    ///
    /// 如果加载成功返回 Ok(())，否则返回 IO 错误
    pub fn load(&mut self) -> std::io::Result<()> {
        if !self.path.exists() {
            // 文件不存在，初始化为空列表
            self.connections = Vec::new();
            self.modified = false;
            return Ok(());
        }

        let mut file = File::open(&self.path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        if contents.trim().is_empty() {
            self.connections = Vec::new();
        } else {
            // 手动解析 JSON（不依赖 serde）
            self.connections = Self::parse_connections_json(&contents)?;
        }

        self.modified = false;
        Ok(())
    }

    /// 保存连接信息到文件
    ///
    /// # Returns
    ///
    /// 如果保存成功返回 Ok(())，否则返回 IO 错误
    pub fn save(&self) -> std::io::Result<()> {
        // 确保父目录存在
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let json = Self::serialize_connections(&self.connections);
        let mut file = File::create(&self.path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?; // 确保数据写入磁盘

        Ok(())
    }

    /// 添加或更新连接
    ///
    /// 如果连接已存在则更新，否则添加新连接。
    /// 新连接会被添加到列表开头（最近使用）。
    ///
    /// # Arguments
    ///
    /// * `connection` - 连接信息
    pub fn add_connection(&mut self, mut connection: ConnectionInfo) {
        // 移除已存在的相同 ID 的连接
        self.connections.retain(|c| c.id != connection.id);

        // 更新最后使用时间
        connection.touch();

        // 添加到列表开头（最近使用）
        self.connections.insert(0, connection);

        // 限制连接数量
        if self.connections.len() > MAX_CONNECTIONS {
            self.connections.truncate(MAX_CONNECTIONS);
        }

        self.modified = true;
    }

    /// 删除连接
    ///
    /// # Arguments
    ///
    /// * `id` - 连接 ID
    ///
    /// # Returns
    ///
    /// 如果删除成功返回 true，否则返回 false
    pub fn remove_connection(&mut self, id: &str) -> bool {
        let len_before = self.connections.len();
        self.connections.retain(|c| c.id != id);
        let removed = self.connections.len() < len_before;

        if removed {
            self.modified = true;
        }

        removed
    }

    /// 获取所有连接（按最近使用排序）
    pub fn get_connections(&self) -> &[ConnectionInfo] {
        &self.connections
    }

    /// 获取指定连接
    ///
    /// # Arguments
    ///
    /// * `id` - 连接 ID
    ///
    /// # Returns
    ///
    /// 如果连接存在返回 Some(&ConnectionInfo)，否则返回 None
    pub fn get_connection(&self, id: &str) -> Option<&ConnectionInfo> {
        self.connections.iter().find(|c| c.id == id)
    }

    /// 获取指定连接的可变引用
    pub fn get_connection_mut(&mut self, id: &str) -> Option<&mut ConnectionInfo> {
        self.connections.iter_mut().find(|c| c.id == id)
    }

    /// 更新连接的最后使用时间
    ///
    /// 同时将连接移动到列表开头（标记为最近使用）
    ///
    /// # Arguments
    ///
    /// * `id` - 连接 ID
    ///
    /// # Returns
    ///
    /// 如果连接存在并更新成功返回 true，否则返回 false
    pub fn touch_connection(&mut self, id: &str) -> bool {
        if let Some(pos) = self.connections.iter().position(|c| c.id == id) {
            // 移除并重新插入到开头
            let mut conn = self.connections.remove(pos);
            conn.touch();
            self.connections.insert(0, conn);
            self.modified = true;
            true
        } else {
            false
        }
    }

    /// 清空所有连接
    pub fn clear(&mut self) {
        if !self.connections.is_empty() {
            self.connections.clear();
            self.modified = true;
        }
    }

    /// 获取连接数量
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// 检查是否已修改
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// 按数据库类型筛选连接
    ///
    /// # Arguments
    ///
    /// * `db_type` - 数据库类型
    ///
    /// # Returns
    ///
    /// 返回匹配类型的连接列表
    pub fn get_connections_by_type(&self, db_type: &str) -> Vec<&ConnectionInfo> {
        self.connections
            .iter()
            .filter(|c| c.db_type == db_type)
            .collect()
    }

    /// 搜索连接
    ///
    /// 根据名称或 URL 进行模糊搜索
    ///
    /// # Arguments
    ///
    /// * `query` - 搜索关键词
    ///
    /// # Returns
    ///
    /// 返回匹配条件的连接列表
    pub fn search(&self, query: &str) -> Vec<&ConnectionInfo> {
        let query_lower = query.to_lowercase();
        self.connections
            .iter()
            .filter(|c| {
                c.name.to_lowercase().contains(&query_lower)
                    || c.url.to_lowercase().contains(&query_lower)
                    || c.db_type.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    // ========== JSON 序列化/反序列化（手动实现，不依赖 serde） ==========

    /// 解析连接列表 JSON
    fn parse_connections_json(json: &str) -> std::io::Result<Vec<ConnectionInfo>> {
        let mut connections = Vec::new();

        // 简单的 JSON 解析：期望格式为 [{...}, {...}]
        let json = json.trim();
        if json == "[]" || json.is_empty() {
            return Ok(connections);
        }

        // 移除外层的方括号
        let json = json.strip_prefix('[').unwrap_or(json);
        let json = json.strip_suffix(']').unwrap_or(json);

        // 按对象分割
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
                        if let Ok(conn) = Self::parse_connection_json(obj_str) {
                            connections.push(conn);
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(connections)
    }

    /// 解析单个连接 JSON 对象
    fn parse_connection_json(json: &str) -> std::io::Result<ConnectionInfo> {
        let mut id = String::new();
        let mut name = String::new();
        let mut db_type = String::new();
        let mut url = String::new();
        let mut last_used = 0u64;
        let mut created_at = 0u64;

        // 提取字符串字段值
        let extract_string = |json: &str, key: &str| -> Option<String> {
            let pattern = format!("\"{}\":", key);
            if let Some(pos) = json.find(&pattern) {
                let start = pos + pattern.len();
                let rest = &json[start..];
                // 跳过空白字符
                let rest = rest.trim_start();
                if rest.starts_with('"') {
                    // 找到字符串结束位置（使用字节索引）
                    let mut end = 1;
                    let mut escape = false;
                    let bytes = rest.as_bytes();
                    while end < bytes.len() {
                        if escape {
                            escape = false;
                            end += 1;
                        } else if bytes[end] == b'\\' {
                            escape = true;
                            end += 1;
                        } else if bytes[end] == b'"' {
                            break;
                        } else {
                            end += 1;
                        }
                    }
                    // 使用 from_utf8 安全转换
                    return String::from_utf8(bytes[1..end].to_vec()).ok();
                }
            }
            None
        };

        // 提取数字字段值
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

        if let Some(v) = extract_string(json, "id") {
            id = v;
        }
        if let Some(v) = extract_string(json, "name") {
            name = v;
        }
        if let Some(v) = extract_string(json, "db_type") {
            db_type = v;
        }
        if let Some(v) = extract_string(json, "url") {
            url = v;
        }
        let server_version = extract_string(json, "server_version");
        let description = extract_string(json, "description");
        let driver_id = extract_string(json, "driver_id");
        let environment_id = extract_string(json, "environment_id");
        let auth_config_id = extract_string(json, "auth_config_id");
        let auth_method = extract_string(json, "auth_method");
        let network_config_id = extract_string(json, "network_config_id");
        let driver_properties = extract_string(json, "driver_properties");
        let advanced_options = extract_string(json, "advanced_options");
        let conn_id = extract_string(json, "conn_id");
        if let Some(v) = extract_u64(json, "last_used") {
            last_used = v;
        }
        if let Some(v) = extract_u64(json, "created_at") {
            created_at = v;
        }

        // 验证必填字段
        if id.is_empty() || db_type.is_empty() || url.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Missing required fields",
            ));
        }

        Ok(ConnectionInfo {
            id,
            name,
            db_type,
            url,
            conn_id,
            server_version,
            last_used,
            created_at,
            description,
            driver_id,
            environment_id,
            auth_config_id,
            auth_method,
            network_config_id,
            driver_properties,
            advanced_options,
        })
    }

    /// 序列化连接列表为 JSON
    pub fn serialize_connections(connections: &[ConnectionInfo]) -> String {
        if connections.is_empty() {
            return "[]".to_string();
        }

        let mut result = String::from("[\n");
        for (i, conn) in connections.iter().enumerate() {
            result.push_str("  {\n");
            result.push_str(&format!(
                "    \"id\": \"{}\",\n",
                Self::escape_json(&conn.id)
            ));
            result.push_str(&format!(
                "    \"name\": \"{}\",\n",
                Self::escape_json(&conn.name)
            ));
            result.push_str(&format!(
                "    \"db_type\": \"{}\",\n",
                Self::escape_json(&conn.db_type)
            ));
            result.push_str(&format!(
                "    \"url\": \"{}\",\n",
                Self::escape_json(&conn.url)
            ));
            if let Some(ref sv) = conn.server_version {
                result.push_str(&format!(
                    "    \"server_version\": \"{}\",\n",
                    Self::escape_json(sv)
                ));
            }
            if let Some(ref cid) = conn.conn_id {
                result.push_str(&format!(
                    "    \"conn_id\": \"{}\",\n",
                    Self::escape_json(cid)
                ));
            }
            result.push_str(&format!("    \"last_used\": {},\n", conn.last_used));
            result.push_str(&format!("    \"created_at\": {}", conn.created_at));
            if let Some(ref desc) = conn.description {
                result.push_str(",\n");
                result.push_str(&format!(
                    "    \"description\": \"{}\"",
                    Self::escape_json(desc)
                ));
            }
            if let Some(ref did) = conn.driver_id {
                result.push_str(",\n");
                result.push_str(&format!(
                    "    \"driver_id\": \"{}\"",
                    Self::escape_json(did)
                ));
            }
            if let Some(ref eid) = conn.environment_id {
                result.push_str(",\n");
                result.push_str(&format!(
                    "    \"environment_id\": \"{}\"",
                    Self::escape_json(eid)
                ));
            }
            if let Some(ref aid) = conn.auth_config_id {
                result.push_str(",\n");
                result.push_str(&format!(
                    "    \"auth_config_id\": \"{}\"",
                    Self::escape_json(aid)
                ));
            }
            if let Some(ref nid) = conn.network_config_id {
                result.push_str(",\n");
                result.push_str(&format!(
                    "    \"network_config_id\": \"{}\"",
                    Self::escape_json(nid)
                ));
            }
            if let Some(ref dp) = conn.driver_properties {
                result.push_str(",\n");
                result.push_str(&format!(
                    "    \"driver_properties\": \"{}\"",
                    Self::escape_json(dp)
                ));
            }
            if let Some(ref ao) = conn.advanced_options {
                result.push_str(",\n");
                result.push_str(&format!(
                    "    \"advanced_options\": \"{}\"",
                    Self::escape_json(ao)
                ));
            }
            result.push('\n');
            result.push_str("  }");
            if i < connections.len() - 1 {
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

/// 全局连接存储实例
static GLOBAL_STORE: Lazy<Mutex<ConnectionStore>> = Lazy::new(|| {
    let path = get_default_store_path();
    let mut store = ConnectionStore::new(path);
    // 尝试加载已有数据
    let _ = store.load();
    Mutex::new(store)
});

/// 获取默认存储路径
fn get_default_store_path() -> PathBuf {
    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("RdataStation");

    // 确保目录存在
    let _ = std::fs::create_dir_all(&app_dir);

    app_dir.join("recent_connections.json")
}

/// 连接记录（用于服务层）
#[derive(Debug, Clone)]
pub struct ConnectionRecord {
    pub name: String,
    pub db_type: String,
    pub url: String,
    pub last_used_at: chrono::DateTime<chrono::Utc>,
    pub description: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
}

/// 最近连接保存输入参数
pub struct RecentConnectionInput<'a> {
    pub name: &'a str,
    pub db_type: &'a str,
    pub url: &'a str,
    pub conn_id: Option<&'a str>,
    pub description: Option<&'a str>,
    pub driver_id: Option<&'a str>,
    pub environment_id: Option<&'a str>,
    pub auth_config_id: Option<&'a str>,
    pub auth_method: Option<&'a str>,
    pub network_config_id: Option<&'a str>,
    pub driver_properties: Option<&'a str>,
    pub advanced_options: Option<&'a str>,
}

/// 保存最近连接
pub fn save_recent_connection(input: RecentConnectionInput<'_>) -> Result<(), std::io::Error> {
    let mut store = GLOBAL_STORE
        .lock()
        .map_err(|_| std::io::Error::other("Failed to lock store"))?;

    let id = format!("{}-{}", input.db_type, input.url);
    let now = system_time_secs();
    let conn = ConnectionInfo {
        id,
        name: input.name.to_string(),
        db_type: input.db_type.to_string(),
        url: input.url.to_string(),
        conn_id: input.conn_id.map(|s| s.to_string()),
        server_version: None,
        last_used: now,
        created_at: now,
        description: input.description.map(|s| s.to_string()),
        driver_id: input.driver_id.map(|s| s.to_string()),
        environment_id: input.environment_id.map(|s| s.to_string()),
        auth_config_id: input.auth_config_id.map(|s| s.to_string()),
        auth_method: input.auth_method.map(|s| s.to_string()),
        network_config_id: input.network_config_id.map(|s| s.to_string()),
        driver_properties: input.driver_properties.map(|s| s.to_string()),
        advanced_options: input.advanced_options.map(|s| s.to_string()),
    };

    store.add_connection(conn);
    store.save()
}

fn mask_password_in_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let prefix = &url[..scheme_end + 3];
        let rest = &url[scheme_end + 3..];
        if let Some(at_pos) = rest.find('@') {
            let auth = &rest[..at_pos];
            let host_part = &rest[at_pos..];
            if let Some(colon_pos) = auth.find(':') {
                let username = &auth[..colon_pos];
                return format!("{}{}:******{}", prefix, username, host_part);
            }
            return format!("{}******{}", prefix, host_part);
        }
    }
    url.to_string()
}

fn url_has_plaintext_password(url: &str) -> bool {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        if let Some(at_pos) = rest.find('@') {
            let auth = &rest[..at_pos];
            return auth.contains(':') && !auth.contains("******");
        }
    }
    false
}

/// 获取最近连接列表（含旧数据迁移：明文密码 URL → 脱敏 URL）
pub fn get_recent_connections() -> Result<Vec<ConnectionRecord>, std::io::Error> {
    let mut store = GLOBAL_STORE
        .lock()
        .map_err(|_| std::io::Error::other("Failed to lock store"))?;

    store.load()?;

    let mut migrated = false;

    for conn_info in store.connections.iter_mut() {
        if url_has_plaintext_password(&conn_info.url) {
            conn_info.url = mask_password_in_url(&conn_info.url);
            migrated = true;
        }
    }

    if migrated {
        let _ = store.save();
    }

    let records: Vec<ConnectionRecord> = store
        .get_connections()
        .iter()
        .map(|c| ConnectionRecord {
            name: c.name.clone(),
            db_type: c.db_type.clone(),
            url: c.url.clone(),
            last_used_at: chrono::DateTime::from_timestamp(c.last_used as i64, 0)
                .unwrap_or_else(chrono::Utc::now),
            description: c.description.clone(),
            driver_id: c.driver_id.clone(),
            environment_id: c.environment_id.clone(),
            auth_config_id: c.auth_config_id.clone(),
            auth_method: c.auth_method.clone(),
            network_config_id: c.network_config_id.clone(),
            driver_properties: c.driver_properties.clone(),
            advanced_options: c.advanced_options.clone(),
        })
        .collect();

    Ok(records)
}

/// 删除最近连接记录
pub fn remove_recent_connection(name: &str) -> Result<(), std::io::Error> {
    let mut store = GLOBAL_STORE
        .lock()
        .map_err(|_| std::io::Error::other("Failed to lock store"))?;

    // 找到匹配的连接 ID
    let conn_id_to_remove: Option<String> = store
        .get_connections()
        .iter()
        .find(|conn| conn.name == name)
        .map(|conn| conn.id.clone());

    // 如果有匹配的连接，删除它
    if let Some(id) = conn_id_to_remove {
        store.remove_connection(&id);
    }

    store.save()
}

// 测试已外移至 src-tauri/tests/connection_store_tests.rs
// 原因: 源码超 500 行，测试体量超 100 行，按项目规范外移
