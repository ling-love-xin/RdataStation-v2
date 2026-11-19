//! 环境策略解析与执行前拦截（原型 ⑨「执行语义矩阵」的第一批消费点）。
//!
//! 现状事实（2026-11-10 实测，本轮未变）：`environment_policies` 的 5 类策略此前只有
//! 展示消费者（对话框 / 属性面板 / 快照复制），执行侧零消费——生产环境配了「只读」，
//! 跑 UPDATE 不会有任何拦截。本模块把「策略 → 生效点」接通最要紧的三层：
//!
//! | 策略项 | policy_type | 生效点 | 本模块 |
//! |---|---|---|---|
//! | 只读连接 | security `read_only` | `SqlService::execute` 执行前 | ✅ 拦截 |
//! | DDL 阻断 | security `block_ddl` | 同上 | ✅ 拦截 |
//! | 查询超时 | performance `timeout_s` | `run_plain` / 事务超时管道 | ✅ 覆盖（环境值优先于未指定） |
//! | 结果行数上限 | performance `max_rows` | `QueryResult::truncate` | ✅ 截断 + `truncated` |
//! | schema 白/黑名单 | schema `include`/`exclude` | 导航内省 | ⏳ 仅解析，未接内省 |
//! | 审计 / UI 徽标 | audit / ui | log_store / 状态栏 | ⏳ 仅解析 |
//!
//! 口径（与原型 Tab2 一致，不许漂）：
//! - **未绑定环境 = 显式「无策略」**，不是默认 prod；不会误伤临时连接；
//! - 优先级：**连接级覆盖 > 环境策略**（`advanced_options.policy_overrides` 列出的策略类型
//!   在本连接上不生效；见 [`ResolvedPolicies::apply_overrides`]）；
//! - 策略值在**每次执行**时解析（原型提到的「建连快照」需连接对象持有解算结果，
//!   在连接层改造前先按执行时解析，语义更保守：改策略对下一次执行即生效）；
//! - 不在驱动层改写 SQL：拦截发生在语句分类之后、下发之前。
//!
//! `policy_config` 的 JSON 键（本模块定义的唯一口径；旧数据里 `NULL` = 模板未配置）：

use shared::error::{CoreError, DatabaseError};

use crate::sql::SqlStatementType;

/// 解算后的环境策略（默认全放行）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedPolicies {
    /// 绑定的环境名（错误消息里要写清楚「哪条策略拦的」）。
    pub environment: Option<String>,
    pub read_only: bool,
    pub block_ddl: bool,
    /// 结果行数上限（超出即截断）。
    pub max_rows: Option<usize>,
    /// 查询超时（毫秒）。
    pub timeout_ms: Option<u64>,
    pub schema_include: Vec<String>,
    pub schema_exclude: Vec<String>,
    pub audit: bool,
    /// DML 需确认（UPDATE / DELETE 执行前要用户点头）
    pub dml_confirm: bool,
}

impl ResolvedPolicies {
    pub fn is_empty(&self) -> bool {
        self.environment.is_none()
    }

    /// 连接级覆盖：把列出的 `policy_type` 在本连接上**不生效**（连接 > 环境）。
    ///
    /// 语义来自对话框高级页（`advanced_options.policy_overrides`，值是策略类型）：开关默认关
    /// （「默认」）、打开才写进数组并标「已覆盖」。环境策略本来就默认生效，所以「覆盖」只能
    /// 是**这条连接不吃这条环境策略**——反过来（强制启用）在开关默认关时无法表达。
    ///
    /// 覆盖按**整类**停用（`security` 同时关掉只读 / DDL 阻断 / 写确认）：UI 是按策略类型
    /// 给的开关，没有更细的粒度，这里也不凭空发明。
    pub fn apply_overrides(&mut self, override_types: &[String]) {
        for policy_type in override_types {
            match policy_type.trim().to_ascii_lowercase().as_str() {
                "security" => {
                    self.read_only = false;
                    self.block_ddl = false;
                    self.dml_confirm = false;
                }
                "performance" => {
                    self.max_rows = None;
                    self.timeout_ms = None;
                }
                "schema" => {
                    self.schema_include.clear();
                    self.schema_exclude.clear();
                }
                "audit" => self.audit = false,
                // `ui`（徽标）等展示类策略没有执行侧字段，覆盖 = 无事可做
                _ => {}
            }
        }
    }

    /// 执行前拦截：只读 / DDL 阻断。
    ///
    /// 放行清单按「只读语义」判定，而不是只认 SELECT：`SHOW` / `EXPLAIN` / `DESCRIBE`
    /// / `WITH … SELECT` 都不写数据；DDL 在只读档里也一并拦（它比 DDL 阻断更严）。
    pub fn check_statement(
        &self,
        stmt: &SqlStatementType,
        sql: &str,
    ) -> Result<(), CoreError> {
        if self.is_empty() {
            return Ok(());
        }
        let env = self.environment.as_deref().unwrap_or("?");
        let is_ddl = matches!(stmt, SqlStatementType::Ddl);
        // 只读放行按**语句分类 + 首关键字**双重判定：分类器只认 SELECT/INSERT/UPDATE/
        // DELETE/DDL，`SHOW` / `EXPLAIN` / `DESCRIBE` / `PRAGMA` 会落到 Unknown——不能
        // 因为分类器不认识就把只读查询也一起拦掉。
        let read_only_stmt = matches!(stmt, SqlStatementType::Select)
            || matches!(stmt, SqlStatementType::Unknown) && is_read_only_keyword(sql);
        if self.read_only && !read_only_stmt {
            return Err(policy_error(
                "ENV_POLICY_READ_ONLY",
                sql,
                format!("环境「{env}」为只读：只允许 SELECT / SHOW / EXPLAIN / DESCRIBE"),
            ));
        }
        if self.block_ddl && is_ddl {
            return Err(policy_error(
                "ENV_POLICY_DDL_BLOCKED",
                sql,
                format!("环境「{env}」禁止 DDL（CREATE / ALTER / DROP / TRUNCATE）"),
            ));
        }
        Ok(())
    }

    /// 这条语句是否需要「写确认」（原型 ⑨ 矩阵第 8 行：DML 需确认）。
    ///
    /// 只认 UPDATE / DELETE——INSERT 是追加、TRUNCATE 属于 DDL（由 `block_ddl` 管），
    /// 都不走这条。是否需要确认是**事实**，弹不弹窗由 UI 决定（引擎侧只做兜底拒绝）。
    pub fn dml_needs_confirm(&self, stmt: &SqlStatementType) -> bool {
        self.dml_confirm
            && matches!(
                stmt,
                SqlStatementType::Update | SqlStatementType::Delete
            )
    }

    /// 超时取值：调用方显式给的优先；没给才用环境值。
    pub fn timeout_or(&self, requested: Option<u64>) -> Option<u64> {
        requested.or(self.timeout_ms)
    }

    /// 行数上限：环境值与全局上限取小。
    pub fn row_cap(&self, global: usize) -> usize {
        match self.max_rows {
            Some(n) if n > 0 => n.min(global),
            _ => global,
        }
    }

    /// 该 schema 是否对导航可见（原型 ⑨ Tab2「schema 白/黑名单」）。
    ///
    /// - `include` 非空 = **白名单**：只放行命中项；
    /// - `exclude` 命中 = 隐藏（黑名单优先于「没写白名单」的默认全放）；
    /// - 两者都空 = 全放（未配置策略的库不受影响）。
    ///
    /// 模式只支持 `*` 通配（与 UI 里的提示一致）；大小写敏感——PG 的 schema 名就是
    /// 大小写敏感的，这里不替用户「猜」。
    pub fn schema_visible(&self, schema: &str) -> bool {
        if self.schema_include.is_empty() && self.schema_exclude.is_empty() {
            return true;
        }
        let hit = |patterns: &[String]| patterns.iter().any(|p| glob_match(p, schema));
        if !self.schema_include.is_empty() && !hit(&self.schema_include) {
            return false;
        }
        !hit(&self.schema_exclude)
    }
}

/// 写确认请求：UI 弹窗直接拿这三项事实，不再自己解析 SQL。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmlConfirm {
    /// 环境展示名（弹窗要写清「哪个环境」）
    pub environment: String,
    /// 语句种类（`UPDATE` / `DELETE`）
    pub statement: &'static str,
    /// 语句摘要（前 120 字符；不把整段 SQL 塞进对话框）
    pub preview: String,
}

impl DmlConfirm {
    /// 由策略 + 语句造一个确认请求（`None` = 不需要确认）。
    pub fn for_statement(
        policies: &ResolvedPolicies,
        stmt: &SqlStatementType,
        sql: &str,
    ) -> Option<Self> {
        if !policies.dml_needs_confirm(stmt) {
            return None;
        }
        let statement = match stmt {
            SqlStatementType::Update => "UPDATE",
            SqlStatementType::Delete => "DELETE",
            _ => return None,
        };
        Some(Self {
            environment: policies.environment.clone().unwrap_or_default(),
            statement,
            preview: sql.trim().chars().take(120).collect(),
        })
    }
}

/// DML 待确认的兜底错误（UI 会先弹窗；这条错误是「没人问过用户就要写」的拒绝）。
pub fn dml_confirm_error(environment: &str, sql: &str) -> CoreError {
    policy_error(
        "ENV_POLICY_DML_CONFIRM",
        sql,
        format!("环境「{environment}」的写操作（UPDATE / DELETE）需要确认后才能执行"),
    )
}

fn policy_error(code: &str, sql: &str, message: String) -> CoreError {    CoreError::database(DatabaseError::query(
        format!("[{code}] {message}"),
        sql.to_string(),
    ))
}

/// `*` 通配匹配（仅此一种元字符；其余字符按字面比较，大小写敏感）。
fn glob_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == text;
    }
    let mut rest = text;
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else {
        return pattern == text;
    };
    if !rest.starts_with(first) {
        return false;
    }
    rest = &rest[first.len()..];
    let mut pieces = parts.peekable();
    while let Some(part) = pieces.next() {
        if part.is_empty() {
            continue;
        }
        if pieces.peek().is_none() {
            // 最后一段：必须落在结尾
            return rest.ends_with(part);
        }
        match rest.find(part) {
            Some(idx) => rest = &rest[idx + part.len()..],
            None => return false,
        }
    }
    true
}

/// 首关键字是否属于只读语法（用于分类器落到 `Unknown` 的语句）。
fn is_read_only_keyword(sql: &str) -> bool {
    let first = sql
        .trim_start_matches(|c: char| c.is_whitespace() || c == '(')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(';')
        .to_ascii_uppercase();
    matches!(
        first.as_str(),
        "SELECT" | "SHOW" | "EXPLAIN" | "DESCRIBE" | "DESC" | "PRAGMA" | "VALUES" | "TABLE"
    )
}

/// 从 `environment_policies` 行解算（`enabled = false` 的策略跳过；坏 JSON 跳过该条）。
///
/// 这是**纯函数**，便于单测：不需要数据库即可验证「哪条策略落在哪个字段」。
pub fn resolve_from_rows(
    environment: Option<String>,
    rows: &[(String, Option<String>, bool)],
) -> ResolvedPolicies {
    let mut p = ResolvedPolicies {
        environment,
        ..Default::default()
    };
    if p.environment.is_none() {
        return p;
    }
    for (policy_type, config, enabled) in rows {
        if !enabled {
            continue;
        }
        let Some(raw) = config.as_deref() else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
            continue;
        };
        match policy_type.trim().to_ascii_lowercase().as_str() {
            "security" => {
                p.read_only |= v.get("read_only").and_then(|x| x.as_bool()).unwrap_or(false);
                p.block_ddl |= v.get("block_ddl").and_then(|x| x.as_bool()).unwrap_or(false);
                // 写确认：原型 Tab1 的安全策略块里是「DML 需确认」开关。两个键都认
                // （`confirm_dml` 为规范键，`dml_confirm` 兼容手写 JSON）。
                p.dml_confirm |= v
                    .get("confirm_dml")
                    .or_else(|| v.get("dml_confirm"))
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
            }
            "performance" => {
                if let Some(n) = v.get("max_rows").and_then(|x| x.as_u64()) {
                    p.max_rows = Some(n as usize);
                }
                if let Some(s) = v.get("timeout_s").and_then(|x| x.as_u64()) {
                    p.timeout_ms = Some(s.saturating_mul(1000));
                }
            }
            "schema" => {
                p.schema_include = string_list(v.get("include"));
                p.schema_exclude = string_list(v.get("exclude"));
            }
            "audit" => {
                p.audit |= v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false);
            }
            _ => {}
        }
    }
    p
}

fn string_list(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|i| i.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// 按连接的环境 id 读取策略并解算（无环境 / 环境不存在 / 库未初始化 → 全放行）。
///
/// 每条 SQL 都查一次库会把 2 次 SQLite 读塞进热路径（SELECT 也走这里），所以按
/// 环境 id 做一个**短 TTL 进程缓存**（5 秒）：改策略最多 5 秒后生效——比「每次执行都
/// 两次读」省得多，也不会让改策略像没生效（原型要求的「建连快照」更保守，见模块头）。
pub async fn resolve_for_environment(
    environment_id: Option<&str>,
) -> ResolvedPolicies {
    let Some(env_id) = environment_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return ResolvedPolicies::default();
    };
    const TTL: std::time::Duration = std::time::Duration::from_secs(5);
    if let Ok(cache) = policy_cache().lock() {
        if let Some((at, p)) = cache.get(env_id) {
            if at.elapsed() < TTL {
                return p.clone();
            }
        }
    }
    let Some(gdb) = crate::migration::get_global_db_manager() else {
        return ResolvedPolicies::default();
    };
    let env_name = gdb
        .list_environments()
        .await
        .ok()
        .and_then(|envs| envs.into_iter().find(|e| e.id == env_id).map(|e| e.name));
    let rows = match gdb.list_environment_policies(env_id).await {
        Ok(list) => list,
        Err(_) => return ResolvedPolicies::default(),
    };
    let flat: Vec<(String, Option<String>, bool)> = rows
        .into_iter()
        .map(|p| (p.policy_type, p.policy_config, p.enabled))
        .collect();
    let resolved = resolve_from_rows(env_name, &flat);
    if let Ok(mut cache) = policy_cache().lock() {
        cache.insert(
            env_id.to_string(),
            (std::time::Instant::now(), resolved.clone()),
        );
    }
    resolved
}

type PolicyCache = std::sync::Mutex<
    std::collections::HashMap<String, (std::time::Instant, ResolvedPolicies)>,
>;

fn policy_cache() -> &'static PolicyCache {
    static CACHE: std::sync::OnceLock<PolicyCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 从连接的高级选项读 `policy_overrides`（纯函数：坏 JSON / 非字符串项一律忽略）。
///
/// 与对话框写入端同口径：值是 `environment_policies.policy_type`（`security` / `performance` /
/// `schema` / `audit` / `ui`）。旧版本存过 6 项布尔键，不匹配任何类型，自然被忽略。
pub fn policy_overrides(advanced_options: Option<&str>) -> Vec<String> {
    advanced_options
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|v| v.get("policy_overrides").and_then(|x| x.as_array()).cloned())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str())
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// 按连接解算策略：**连接级覆盖 > 环境策略**。
///
/// 环境那层走按环境 id 的 5 秒缓存；覆盖是连接自己的字段（调用方手里已有），
/// 所以只多做一次 JSON 解析，不加缓存（也就不会因“刚保存的覆盖”延迟生效）。
pub async fn resolve_for_connection(
    environment_id: Option<&str>,
    advanced_options: Option<&str>,
) -> ResolvedPolicies {
    let mut policies = resolve_for_environment(environment_id).await;
    let overrides = policy_overrides(advanced_options);
    policies.apply_overrides(&overrides);
    policies
}

// ===== 审计（原型 ⑨ Tab2 `audit.enabled`）=====
//
// 落点 = 全局库 `app_logs`（`store::logging` 的 LogStore），target 固定 `audit.sql`：
// 审计与运行日志同一套留存/清理策略，不另建表；`fields` 里带 conn / environment / outcome
// 供过滤。**不阻塞执行**：写成 tokio 任务，写失败只打 warn（审计库不可用不该让 SQL 失败）。

/// 写一条 SQL 审计（`outcome` = `allowed` / `denied`；`detail` = 错误码等补充）。
pub fn audit_sql(
    conn_key: &str,
    environment: Option<&str>,
    sql: &str,
    outcome: &str,
    detail: Option<&str>,
) {
    use rds_store::logging::get_log_store;
    let Some(log_store) = get_log_store() else {
        return;
    };
    let record = build_audit_record(conn_key, environment, sql, outcome, detail);
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        if let Err(e) = log_store.flush_records(&[record]).await {
            tracing::warn!(error = %e, "SQL 审计写入失败");
        }
    });
}

/// 构造审计记录（纯函数，便于单测断言口径）。
pub fn build_audit_record(
    conn_key: &str,
    environment: Option<&str>,
    sql: &str,
    outcome: &str,
    detail: Option<&str>,
) -> rds_store::logging::record::LogRecord {
    use rds_store::logging::record::{LogLevel, LogRecord};
    let level = if outcome == "denied" {
        LogLevel::Warn
    } else {
        LogLevel::Info
    };
    let snippet: String = sql.chars().take(500).collect();
    let fields = serde_json::json!({
        "conn": conn_key,
        "environment": environment,
        "outcome": outcome,
        "detail": detail,
    })
    .to_string();
    LogRecord {
        id: 0,
        timestamp: chrono::Utc::now().to_rfc3339(),
        level,
        target: "audit.sql".to_string(),
        message: format!("[{outcome}] env={} {snippet}", environment.unwrap_or("-")),
        fields: Some(fields),
        file: None,
        line: None,
        session_id: rds_store::logging::session_id(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: SqlStatementType = SqlStatementType::Select;

    fn rows3() -> Vec<(String, Option<String>, bool)> {
        vec![
            (
                "security".into(),
                Some(r#"{"read_only":true,"block_ddl":true}"#.into()),
                true,
            ),
            (
                "performance".into(),
                Some(r#"{"max_rows":1000,"timeout_s":30}"#.into()),
                true,
            ),
            (
                "schema".into(),
                Some(r#"{"include":["orders","report*"],"exclude":["*_tmp"]}"#.into()),
                true,
            ),
        ]
    }

    #[test]
    fn audit_record_marks_denied_as_warn_and_truncates_sql() {
        use rds_store::logging::record::LogLevel;
        let long = "S".repeat(600);
        let rec = build_audit_record("conn-1", Some("prod"), &long, "denied", Some("ENV_POLICY_READ_ONLY"));
        assert_eq!(rec.target, "audit.sql");
        assert_eq!(rec.level, LogLevel::Warn, "被拦语句应为 Warn");
        let fields: serde_json::Value =
            serde_json::from_str(rec.fields.as_deref().unwrap()).unwrap();
        assert_eq!(fields["outcome"], "denied");
        assert_eq!(fields["environment"], "prod");
        assert_eq!(fields["conn"], "conn-1");
        assert_eq!(fields["detail"], "ENV_POLICY_READ_ONLY");
        assert_eq!(rec.message.chars().filter(|c| *c == 'S').count(), 500, "SQL 片段限 500 字符");
        assert!(rec.message.contains("env=prod"));
        let ok = build_audit_record("conn-1", None, "SELECT 1", "allowed", None);
        assert_eq!(ok.level, LogLevel::Info);
        assert!(ok.message.contains("env=-"));
    }

    #[test]
    fn dml_confirm_covers_update_and_delete_only() {
        let p = resolve_from_rows(
            Some("staging".into()),
            &[(
                "security".into(),
                Some(r#"{"confirm_dml":true}"#.into()),
                true,
            )],
        );
        assert!(p.dml_confirm);
        assert!(p.dml_needs_confirm(&SqlStatementType::Update));
        assert!(p.dml_needs_confirm(&SqlStatementType::Delete));
        assert!(!p.dml_needs_confirm(&SqlStatementType::Insert), "追加不算");
        assert!(!p.dml_needs_confirm(&SqlStatementType::Select));
        assert!(!p.dml_needs_confirm(&SqlStatementType::Ddl), "DDL 归 block_ddl 管");

        let confirm = DmlConfirm::for_statement(
            &p,
            &SqlStatementType::Update,
            "update orders set total = 0 where id = 1",
        )
        .expect("UPDATE 应要求确认");
        assert_eq!(confirm.environment, "staging");
        assert_eq!(confirm.statement, "UPDATE");
        assert!(confirm.preview.starts_with("update orders"));

        // 未配置 = 不需要确认（未绑定环境也走这条）
        let off = resolve_from_rows(Some("staging".into()), &[]);
        assert!(!off.dml_confirm);
        assert!(DmlConfirm::for_statement(&off, &SqlStatementType::Delete, "delete from t").is_none());
    }

    #[test]
    fn connection_overrides_turn_off_only_the_listed_policy_type() {
        let mut p = resolve_from_rows(
            Some("prod".into()),
            &[
                (
                    "security".into(),
                    Some(r#"{"read_only":true,"block_ddl":true,"confirm_dml":true}"#.into()),
                    true,
                ),
                (
                    "performance".into(),
                    Some(r#"{"max_rows":500,"timeout_s":30}"#.into()),
                    true,
                ),
                (
                    "schema".into(),
                    Some(r#"{"exclude":["tmp_*"]}"#.into()),
                    true,
                ),
            ],
        );
        // 只覆盖 security：读写类闸全关，行数 / 超时 / schema 仍按环境生效
        p.apply_overrides(&["security".to_string()]);
        assert!(!p.read_only && !p.block_ddl && !p.dml_confirm);
        assert_eq!(p.max_rows, Some(500));
        assert_eq!(p.timeout_ms, Some(30_000));
        assert!(!p.schema_visible("tmp_1"), "没被覆盖的那类不受影响");

        // 覆盖 performance：行数 / 超时都退回“无策略”，其余保留
        let mut q = resolve_from_rows(
            Some("prod".into()),
            &[(
                "performance".into(),
                Some(r#"{"max_rows":500,"timeout_s":30}"#.into()),
                true,
            )],
        );
        q.apply_overrides(&["PERFORMANCE".to_string()]); // 大小写不敏感
        assert_eq!(q.max_rows, None);
        assert_eq!(q.timeout_ms, None);
    }

    #[test]
    fn policy_overrides_reading_ignores_bad_json_and_unknown_items() {
        assert!(policy_overrides(None).is_empty());
        assert!(policy_overrides(Some("not json")).is_empty());
        let list = policy_overrides(Some(
            r#"{"ssl":{"mode":"require"},"policy_overrides":[" Security ","",3,"audit"]}"#,
        ));
        assert_eq!(list, vec!["security".to_string(), "audit".to_string()]);
    }

    #[test]
    fn schema_visibility_honours_include_exclude_glob() {
        let p = resolve_from_rows(
            Some("prod".into()),
            &[(
                "schema".into(),
                Some(r#"{"include":["public","biz_*"],"exclude":["biz_tmp*"]}"#.into()),
                true,
            )],
        );
        assert!(p.schema_visible("public"));
        assert!(p.schema_visible("biz_orders"));
        assert!(!p.schema_visible("biz_tmp_1"), "exclude 优先于 include");
        assert!(!p.schema_visible("pg_catalog"), "不在白名单即隐藏");
        let all = resolve_from_rows(Some("prod".into()), &[]);
        assert!(all.schema_visible("anything"), "空名单 = 全放");
        // 通配只在 * 处生效：无 * 的写死名单按字面比较
        let literal = resolve_from_rows(
            Some("prod".into()),
            &[(
                "schema".into(),
                Some(r#"{"exclude":["tmp"]}"#.into()),
                true,
            )],
        );
        assert!(!literal.schema_visible("tmp"));
        assert!(literal.schema_visible("tmp_2"), "无 * 不得前缀匹配");
    }

    #[test]
    fn no_environment_means_no_policy() {
        let p = resolve_from_rows(None, &rows3());
        assert!(p.is_empty());
        assert!(!p.read_only && !p.block_ddl);
        assert_eq!(p.row_cap(10_000), 10_000, "无环境不套任何上限");
        assert_eq!(p.timeout_or(None), None);
    }

    #[test]
    fn security_and_performance_fields_land() {
        let p = resolve_from_rows(Some("prod".into()), &rows3());
        assert_eq!(p.environment.as_deref(), Some("prod"));
        assert!(p.read_only && p.block_ddl);
        assert_eq!(p.max_rows, Some(1000));
        assert_eq!(p.timeout_ms, Some(30_000));
        assert_eq!(p.schema_exclude, vec!["*_tmp".to_string()]);
        assert_eq!(p.row_cap(10_000), 1000, "环境值与全局取小");
        assert_eq!(
            p.timeout_or(Some(500)),
            Some(500),
            "调用方显式超时优先于环境"
        );
    }

    #[test]
    fn disabled_or_null_policies_are_ignored() {
        let rows = vec![
            ("security".to_string(), Some(r#"{"read_only":true}"#.into()), false),
            ("security".to_string(), None, true),
            ("security".to_string(), Some("not-json".into()), true),
        ];
        let p = resolve_from_rows(Some("prod".into()), &rows);
        assert!(!p.read_only, "未启用 / 空配置 / 坏 JSON 都不得生效");
        assert!(p.is_empty() == false);
    }

    #[test]
    fn read_only_blocks_writes_but_allows_reads() {
        let p = resolve_from_rows(
            Some("prod".into()),
            &[(
                "security".into(),
                Some(r#"{"read_only":true}"#.into()),
                true,
            )],
        );
        assert!(p.check_statement(&ALL, "SELECT 1").is_ok());
        assert!(
            p.check_statement(&SqlStatementType::Unknown, "SHOW TABLES").is_ok(),
            "分类器不认识的只读语句（SHOW）应放行"
        );
        let err = p
            .check_statement(&SqlStatementType::Update, "UPDATE t SET a=1")
            .expect_err("只读必须拦 UPDATE");
        let msg = err.to_string();
        assert!(msg.contains("ENV_POLICY_READ_ONLY"), "{msg}");
        assert!(msg.contains("prod"), "错误消息要写明环境：{msg}");
    }

    #[test]
    fn ddl_block_only_blocks_ddl() {
        let p = resolve_from_rows(
            Some("staging".into()),
            &[(
                "security".into(),
                Some(r#"{"block_ddl":true}"#.into()),
                true,
            )],
        );
        assert!(p.check_statement(&SqlStatementType::Select, "SELECT 1").is_ok());
        assert!(
            p.check_statement(&SqlStatementType::Update, "UPDATE t SET a=1")
                .is_ok(),
            "DDL 阻断不拦 DML"
        );
        let err = p
            .check_statement(&SqlStatementType::Ddl, "DROP TABLE t")
            .expect_err("DDL 必须拦");
        assert!(err.to_string().contains("ENV_POLICY_DDL_BLOCKED"));
    }
}
