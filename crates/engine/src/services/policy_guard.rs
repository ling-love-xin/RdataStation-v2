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
//! - 优先级：连接级覆盖（尚未落地）> 环境策略；本模块当前只有环境一层；
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
}

impl ResolvedPolicies {
    pub fn is_empty(&self) -> bool {
        self.environment.is_none()
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
}

fn policy_error(code: &str, sql: &str, message: String) -> CoreError {
    CoreError::database(DatabaseError::query(
        format!("[{code}] {message}"),
        sql.to_string(),
    ))
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
