use super::*;


/// 摘要超过该字符数（或含换行）时，UI 提供「详情」展开（按字符数，避免多字节截断）。
pub(crate) const RESULT_SUMMARY_MAX_CHARS: usize = 80;

/// 摘要是否需要「详情」入口（过长或含换行）。
pub(crate) fn result_needs_detail(summary: &str) -> bool {
    summary.chars().count() > RESULT_SUMMARY_MAX_CHARS || summary.contains('\n')
}

/// 处理「新建文件…」选中的路径：返回 `(结果行文案, 是否成功, 写回地址的值)`。
///
/// **语义（与「打开文件…」严格分离）**：
/// - 路径已存在 → 不创建、不引用、也不碰原文件（避免误损已有库），提示换名或用「打开文件…」；
/// - 路径不存在 → 按驱动族创建：DuckDB 建出有效库文件（写文件头，立即可连）；
///   SQLite 等保持空文件（首次连接由驱动初始化结构）。
///
/// 为何不做“已存在则直接引用”：那会让「新建」看起来就是打开（真机反馈“duckdb 的新建
/// 为什么还是打开功能”）；两个按钮职责必须互斥。
pub(crate) fn create_new_db_file(
    path: &std::path::Path,
    driver_id: &str,
) -> (String, bool, Option<String>) {
    let value = path.to_string_lossy().to_string();
    let is_duckdb = driver_id.eq_ignore_ascii_case("duckdb");
    if path.exists() {
        let nonempty = std::fs::metadata(path)
            .map(|m| m.len() > 0)
            .unwrap_or(true);
        if nonempty {
            return (
                format!("该文件已存在，未创建：{value}；请换一个文件名，或用「打开文件…」引用它"),
                false,
                None,
            );
        }
        // 0 字节：SQLite 对空文件合法（首次连接初始化）；DuckDB 不认空文件（见
        // `engine::duckdb::initialize_duckdb_file` 注释），删除后由下面重建。
        if !is_duckdb {
            return (
                format!("已存在空文件，将直接引用它（首次连接由驱动初始化结构）：{value}"),
                true,
                Some(value),
            );
        }
    }
    match create_file_for_driver(is_duckdb, path) {
        Ok(()) => (format!("已新建数据库文件：{value}"), true, Some(value)),
        Err(e) => (format!("新建数据库文件失败：{e}"), false, None),
    }
}

/// 按驱动族创建数据库文件：DuckDB 必须建出**有效库文件**（写入文件头，立即可连），
/// 其余文件型驱动（SQLite 等）保持「空文件 + 首次连接由驱动初始化」的既有契约。
fn create_file_for_driver(is_duckdb: bool, path: &std::path::Path) -> Result<(), String> {
    if is_duckdb {
        engine::duckdb::initialize_duckdb_file(path).map_err(|e| e.to_string())
    } else {
        std::fs::File::create(path)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

pub(crate) fn is_bool_text(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "true" | "false" | "1" | "0" | "yes" | "no"
    )
}

pub(crate) fn parse_bool_text(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes"
    )
}

pub(crate) fn insert_json_path(
    root: &mut serde_json::Map<String, serde_json::Value>,
    path: &[&str],
    value: serde_json::Value,
) {
    match path {
        [] => {}
        [only] => {
            root.insert((*only).to_string(), value);
        }
        [head, rest @ ..] => {
            let entry = root
                .entry((*head).to_string())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let serde_json::Value::Object(map) = entry {
                insert_json_path(map, rest, value);
            }
        }
    }
}

pub(crate) fn read_json_path<'a>(
    value: &'a serde_json::Value,
    path: &[&str],
) -> Option<&'a serde_json::Value> {
    match path {
        [] => Some(value),
        [head, rest @ ..] => read_json_path(value.get(*head)?, rest),
    }
}

/// 文件型（SQLite / DuckDB）连接的落库前清洗：清掉无意义的凭据 / 网络 / TLS 字段。
///
/// 输入对象可能带着上个数据库类型的残留（如从 MySQL 切到 SQLite），这些字段落到库里
/// 就是脏数据（还会让“引用认证配置”在同一连接上产生歧义）；策略覆盖等文件型仍有效的
/// 选项保留。
pub(crate) fn strip_file_db_noise(input: &mut DataSourceSaveInput) {
    input.username = None;
    input.password = None;
    input.auth_config_id = None;
    input.auth_method = None;
    input.network_config_id = None;
    let Some(raw) = input.advanced_options.as_deref() else {
        return;
    };
    let Ok(mut adv) = serde_json::from_str::<serde_json::Value>(raw) else {
        // 非法 JSON 原样保留（解析失败不静默丢数据，上层已有其它校验）。
        return;
    };
    if let Some(obj) = adv.as_object_mut() {
        obj.remove("ssl");
        obj.remove("network_chain");
    }
    input.advanced_options = Some(adv.to_string());
}

/// 重建编辑回读地址（DataSource 无 url 字段，由 host/port/database 重拼）。
///
/// 文件型返回**裸路径**（与地址输入框语义一致，见 `normalize_file_db_path`）。
pub(crate) fn reconstruct_url(ds: &DataSource) -> String {
    if crate::services::data_source_service::is_file_db_driver(&ds.db_type) {
        let raw = ds
            .database
            .clone()
            .or_else(|| ds.host.clone())
            .unwrap_or_default();
        return crate::services::data_source_service::normalize_file_db_path(&ds.db_type, &raw);
    }
    let auth = ds
        .username
        .as_deref()
        .map(|u| format!("{}@", u))
        .unwrap_or_default();
    match (&ds.host, ds.port) {
        (Some(h), Some(p)) => format!(
            "{}://{}{}:{}/{}",
            ds.db_type,
            auth,
            h,
            p,
            ds.database.clone().unwrap_or_default()
        ),
        (Some(h), None) => format!(
            "{}://{}{}/{}",
            ds.db_type,
            auth,
            h,
            ds.database.clone().unwrap_or_default()
        ),
        _ => format!(
            "{}://{}{}",
            ds.db_type,
            ds.database.clone().unwrap_or_default(),
            ""
        ),
    }
}

/// 暂存条目显示用的类型 id：**当前条目（光标位）取正在编辑的表单**，其余取快照。
///
/// 真机反馈：从 MySQL 切到 SQLite 后，表单已是 SQLite、条目还显示 mysql 图标——
/// 因为草稿只在“切换条目 / 保存 / 关闭”时写回，显示不能等写回。
pub(crate) fn staging_display_type_id(draft_type_id: &str, live_type_id: Option<&str>) -> String {
    match live_type_id {
        Some(live) if !live.trim().is_empty() => live.to_string(),
        _ => draft_type_id.to_string(),
    }
}

/// 作用域标签（UI ↔ ConnectionScope）。
pub(crate) fn scope_label(s: &ConnectionScope) -> &'static str {
    match s {
        ConnectionScope::Global => "仅全局",
        ConnectionScope::Project => "仅项目",
        ConnectionScope::GlobalAndProject => "全局+项目",
    }
}

pub(crate) fn scope_from_label(l: &str) -> ConnectionScope {
    match l {
        "仅项目" => ConnectionScope::Project,
        "全局+项目" => ConnectionScope::GlobalAndProject,
        _ => ConnectionScope::Global,
    }
}

/// 规则 1（决策 #110）：新建对话框的默认作用域 =「仅项目」。
///
/// 渲染回退与新建空草稿的语义都以它为单一来源：作用域未显式选择（草稿为空、无编辑回读）
/// 时显示「仅项目」；地址预填项目根（文件型）也只在该作用域下有意义。
pub(crate) fn default_new_scope() -> &'static str {
    SCOPE_LABELS[1]
}
