//! 洞察规则索引：把「磁盘上的规则文件」与「库里的索引 / 启停状态」对齐。
//!
//! # 为什么需要索引
//!
//! 规则正文是文件（可 diff / 可进 git / 可手工编辑），但**只有**文件时：
//! 界面无从枚举、无法启停、解析错误无处展示（只写 `tracing::warn!`，
//! 用户看到的现象是「规则莫名其妙不见了」）。索引表补上后半截，
//! 正文仍以文件为唯一真相源。
//!
//! # 分工
//!
//! | 函数 / 类型 | 职责 | 是否触碰数据库 |
//! | --- | --- | --- |
//! | [`scan_scope_dir`] | 读磁盘：哈希 + 试解析 → 索引行（**含解析失败的文件**） | 否 |
//! | [`plan_index`] | 合并磁盘现状与库中状态（保留用户启停、保留内置抑制记录） | 否 |
//! | [`RuleIndexStore`] | 索引表的读写与启停开关 | 是 |
//!
//! 前两者是纯逻辑（只依赖文件系统），单测可直接断言；数据库细节集中在 `RuleIndexStore`。
//!
//! # 两个容易搞错的口径
//!
//! 1. **扫描必须覆盖解析失败的文件**：索引的价值恰在于把「静默跳过的坏规则」变可见。
//!    若沿用注册表（只持有可解析的规则），坏规则依旧不可见。
//! 2. **文件消失 ≠ 删除索引行**：转为 `missing` 状态保留，让界面能说「规则文件不见了」，
//!    而不是让规则无声无息地从列表里消失。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::persistence::{GlobalSqlitePool, ProjectSqlitePool};
use shared::error::{CommonError, CoreError, StorageError};

use crate::rule::RuleScope;
use crate::rule_registry::parse_rule_toml;
use crate::rule_types::RuleFile;

/// 索引行的加载状态（与迁移里的 CHECK 约束一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleLoadStatus {
    /// 文件在、解析通过。
    Ok,
    /// 文件在、解析失败（`load_error` 存错误原文）。
    Invalid,
    /// 索引里有记录，但文件已不在（被删或移走）。
    Missing,
}

impl RuleLoadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RuleLoadStatus::Ok => "ok",
            RuleLoadStatus::Invalid => "invalid",
            RuleLoadStatus::Missing => "missing",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "ok" => Some(RuleLoadStatus::Ok),
            "invalid" => Some(RuleLoadStatus::Invalid),
            "missing" => Some(RuleLoadStatus::Missing),
            _ => None,
        }
    }
}

/// 索引中的一行（对应 `insight_rule_index` 一条记录）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleIndexEntry {
    /// 规则 `meta.id`。
    pub rule_id: String,
    /// 归属层：`Global`（全局库）或 `Project`（项目库）。`Builtin` 不入库。
    pub scope: RuleScope,
    pub category: String,
    pub name: String,
    pub version: String,
    /// 规则文件路径（相对作用域目录，便于项目整体迁移后仍有效）。
    pub source_path: String,
    /// 正文 SHA256（增量判据：内容没变就不重解析）。
    pub checksum: String,
    /// 是否启用。**内置规则靠项目库里的一条 `enabled = false` 记录被禁用。**
    pub enabled: bool,
    pub load_status: RuleLoadStatus,
    /// 解析错误原文（`load_status == Invalid` 时有值）。
    pub load_error: Option<String>,
}

impl RuleIndexEntry {
    /// 文件是否存在（`Missing` 表示索引里有、磁盘上没有）。
    pub fn is_present(&self) -> bool {
        self.load_status != RuleLoadStatus::Missing
    }
}

// ==================== 规则目录与新建规则（Phase 2.3 / 2.4） ====================

/// 某一层的规则目录（内置层没有目录 → `None`）。
///
/// 路径拼装只在 `rule_registry` 一处（`.RSmeta` 的拼写有多个历史版本），
/// 本函数只做「作用域 → 目录」派发，免得界面 / 接缝 / 同步器各拼一次字符串。
pub fn rule_dir(project_root: Option<&Path>, scope: RuleScope) -> Option<PathBuf> {
    match scope {
        RuleScope::Builtin => None,
        RuleScope::Project => project_root.map(crate::rule_registry::get_project_rules_dir),
        RuleScope::Global => engine::migration::get_system_dir()
            .ok()
            .map(|dir| crate::rule_registry::get_global_rules_dir(&dir)),
    }
}

/// 新建模板的文件名前缀（第二个起追加序号：`new-rule`、`new-rule-2`…）。
const NEW_RULE_STEM: &str = "new-rule";

/// 新建规则：建目录（不存在时）+ 写一份**能解析但暂不生效**的模板，返回文件路径。
///
/// 两条都是硬要求：
/// - **能解析**：空文件会以 `invalid` 落索引，用户第一眼看到的就是一条红色错误；
/// - **暂不生效**：`applies_to = []` —— 列级规则按类型族匹配，空列表永不命中，
///   用户填上类型族（或 `Any`）之前它不参与任何分析。
///
/// 文件名与 `meta.id` 都按序号避让：注册表里同名 id 是**后者覆盖前者**，
/// 两份模板同 id 会让用户编辑的那一条被另一条悄悄顶掉。
pub fn create_rule_file(
    project_root: Option<&Path>,
    scope: RuleScope,
) -> Result<PathBuf, CoreError> {
    let dir = rule_dir(project_root, scope).ok_or_else(|| {
        CoreError::common(CommonError::General(format!(
            "{}规则目录不可用（未打开项目或系统目录取不到）",
            scope.label()
        )))
    })?;
    std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, "create_rule_dir", e))?;
    let (path, id) = free_rule_path(&dir);
    std::fs::write(&path, new_rule_template(&id))
        .map_err(|e| io_err(&path, "write_rule_template", e))?;
    Ok(path)
}

/// 目录里第一个可用的模板文件名（连同它的规则 id）。
fn free_rule_path(dir: &Path) -> (PathBuf, String) {
    for n in 1..=999u32 {
        let stem = if n == 1 {
            NEW_RULE_STEM.to_string()
        } else {
            format!("{NEW_RULE_STEM}-{n}")
        };
        let path = dir.join(format!("{stem}.rule.toml"));
        if !path.exists() {
            return (path, stem);
        }
    }
    // 极端情况（目录里已有上千份模板）：退回进程内唯一名，不阻塞用户
    let stem = format!("{NEW_RULE_STEM}-{}", std::process::id());
    (dir.join(format!("{stem}.rule.toml")), stem)
}

/// 新规则的模板正文。
///
/// 骨架取 `insight-user-guide.md` §4.6 示例 A（对外契约里的可照抄版本），
/// 只把 `applies_to` 留空、`id` 换成序号——注释里说清改哪两处就生效。
pub fn new_rule_template(id: &str) -> String {
    format!(
        r#"# 新建的洞察规则（模板）
#
# 让它生效只需两步：
#   1. 把 applies_to 改成要参与的类型族（["Any"] 表示所有列）
#   2. 把 template 里的 SQL 改成自己的查询（{{{{table}}}} / {{{{col}}}} 是内置占位符）
# 字段全表与可照抄的示例见 docs/architecture/insight/insight-user-guide.md §4。

[meta]
id = "{id}"
name = "新建规则"
description = "待填写"
version = "1.0"
category = "column"
# 空列表 = 暂不参与任何分析（不写死一个会在所有列上跑的类型）
applies_to = []
builtin = false

[query]
template = """
SELECT
    COUNT(*) AS total
FROM "{{table}}"
"""
parameters = ["table"]
result_type = "single"

[[output]]
sql_name = "total"
json_name = "total_count"
value_type = "i64"
"#
    )
}

fn io_err(path: &Path, operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::io(
        path.display().to_string(),
        operation,
        reason.to_string(),
    ))
}

/// 扫描某作用域的规则目录，产出索引行。
///
/// `scope` 必须是 `Global` 或 `Project`（`Builtin` 由 `include_dir` 编译期已知、不入库，
/// 传入时返回空表）。目录不存在返回空表——项目没有自有规则是常态。
pub fn scan_scope_dir(dir: &Path, scope: RuleScope) -> Vec<RuleIndexEntry> {
    if scope == RuleScope::Builtin || !dir.is_dir() {
        return Vec::new();
    }

    let mut out = Vec::new();
    collect_dir(dir, dir, scope, &mut out);
    out.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
    out
}

fn collect_dir(root: &Path, dir: &Path, scope: RuleScope, out: &mut Vec<RuleIndexEntry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_dir(root, &path, scope, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
            if let Some(row) = scan_one_file(root, &path, scope) {
                out.push(row);
            }
        }
    }
}

/// 读取单个规则文件并产出索引行；读不出内容时返回 `None`（连内容都没有，无从索引）。
fn scan_one_file(root: &Path, path: &Path, scope: RuleScope) -> Option<RuleIndexEntry> {
    let bytes = std::fs::read(path).ok()?;
    let checksum = sha256_hex(&bytes);
    let rel_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    match std::str::from_utf8(&bytes) {
        Ok(content) => match parse_rule_toml(content) {
            Ok(rule) => Some(entry_from_rule(rule, scope, rel_path, checksum)),
            Err(e) => Some(RuleIndexEntry {
                // 解析失败时拿不到 meta.id，用文件名兜底，保证界面仍能看到这条坏规则。
                rule_id: file_stem(path),
                scope,
                category: String::new(),
                name: file_stem(path),
                version: String::new(),
                source_path: rel_path,
                checksum,
                enabled: true,
                load_status: RuleLoadStatus::Invalid,
                load_error: Some(e.to_string()),
            }),
        },
        Err(e) => Some(RuleIndexEntry {
            rule_id: file_stem(path),
            scope,
            category: String::new(),
            name: file_stem(path),
            version: String::new(),
            source_path: rel_path,
            checksum,
            enabled: true,
            load_status: RuleLoadStatus::Invalid,
            load_error: Some(format!("规则文件不是合法 UTF-8：{}", e)),
        }),
    }
}

fn entry_from_rule(
    rule: RuleFile,
    scope: RuleScope,
    source_path: String,
    checksum: String,
) -> RuleIndexEntry {
    RuleIndexEntry {
        rule_id: rule.meta.id,
        scope,
        category: rule.meta.category,
        name: rule.meta.name,
        version: rule.meta.version,
        source_path,
        checksum,
        enabled: true,
        load_status: RuleLoadStatus::Ok,
        load_error: None,
    }
}

/// 文件名（去扩展名），用于解析失败时的兜底标识。
fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown-rule".to_string())
}

/// 把「磁盘现状」与「库中已有状态」合并成待写入的索引行。
///
/// 三条规则：
/// 1. **磁盘为准**：每条扫描结果都进索引（含解析失败的），`source_path` / `checksum` /
///    `load_status` 取磁盘事实。
/// 2. **启停以用户为准**：`enabled` 沿用库中同名记录；库中没有则默认启用。
/// 3. **库里多出来的行按两种情况保留**：
///    - `rule_id ∈ builtin_ids` → 这是**内置规则的抑制记录**（用户在项目里关掉了某条内置规则），
///      原样保留且状态保持 `Ok`——内置规则本身没问题，只是本项目不用它；
///    - 其余 → 规则文件被删/移走，转 `Missing` 保留，让界面能显式提示而不是无声消失。
pub fn plan_index(
    scanned: Vec<RuleIndexEntry>,
    existing: &[RuleIndexEntry],
    builtin_ids: &HashSet<String>,
) -> Vec<RuleIndexEntry> {
    let mut prev_enabled: HashMap<(&str, RuleScope), bool> = HashMap::new();
    for row in existing {
        prev_enabled.insert((row.rule_id.as_str(), row.scope), row.enabled);
    }

    let mut planned: Vec<RuleIndexEntry> = scanned
        .into_iter()
        .map(|mut row| {
            if let Some(&enabled) = prev_enabled.get(&(row.rule_id.as_str(), row.scope)) {
                row.enabled = enabled;
            }
            row
        })
        .collect();

    // 用 owned 集合：下面还要往 `planned` 里 push，借不了它的 &str。
    let now_on_disk: HashSet<(String, RuleScope)> = planned
        .iter()
        .map(|r| (r.rule_id.clone(), r.scope))
        .collect();

    for row in existing {
        if now_on_disk.contains(&(row.rule_id.clone(), row.scope)) {
            continue;
        }
        let mut carried = row.clone();
        if !builtin_ids.contains(&row.rule_id) {
            // 文件没了：保留记录但改状态，别让它无声消失
            carried.load_status = RuleLoadStatus::Missing;
        }
        planned.push(carried);
    }

    planned.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
    planned
}

/// 规则文件正文的 SHA256。
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

// ==================== 索引表读写 ====================

/// 索引表所在的 SQLite 库。
///
/// 全局层与项目层是**两个物理库**（与 `id_prefix` 的双库约定一致），
/// 但表结构同构、读写语句完全相同，故在此做一层薄封装而不是两套实现。
pub enum IndexPool {
    Global(Arc<GlobalSqlitePool>),
    Project(Arc<ProjectSqlitePool>),
}

/// 指定作用域的索引表读写。
pub struct RuleIndexStore {
    pool: IndexPool,
    scope: RuleScope,
}

impl RuleIndexStore {
    /// 全局层索引（落 `global.db`）。
    pub fn global(pool: Arc<GlobalSqlitePool>) -> Self {
        Self {
            pool: IndexPool::Global(pool),
            scope: RuleScope::Global,
        }
    }

    /// 项目层索引（落 `{项目}/.RSmeta/project.db`）。
    pub fn project(pool: Arc<ProjectSqlitePool>) -> Self {
        Self {
            pool: IndexPool::Project(pool),
            scope: RuleScope::Project,
        }
    }

    pub fn scope(&self) -> RuleScope {
        self.scope
    }

    /// 读出本作用域的全部索引行。
    pub async fn load(&self) -> Result<Vec<RuleIndexEntry>, CoreError> {
        match &self.pool {
            IndexPool::Global(pool) => {
                let conn = pool.acquire().await?;
                read_rows(conn.inner()?, self.scope)
            }
            IndexPool::Project(pool) => {
                let conn = pool.acquire().await?;
                read_rows(conn.inner()?, self.scope)
            }
        }
    }

    /// 用给定行**整体替换**本作用域的索引（单事务：清空 + 写入）。
    ///
    /// 整体替换而非逐行 upsert：索引是磁盘现状的投影，逐行合并会留下
    /// 「已删除文件」的陈旧行；事务保证界面不会看到中间态（清空后未写入）。
    pub async fn replace(&self, rows: &[RuleIndexEntry]) -> Result<(), CoreError> {
        match &self.pool {
            IndexPool::Global(pool) => {
                let mut conn = pool.acquire().await?;
                write_rows(conn.inner_mut()?, self.scope, rows)
            }
            IndexPool::Project(pool) => {
                let mut conn = pool.acquire().await?;
                write_rows(conn.inner_mut()?, self.scope, rows)
            }
        }
    }

    /// 设置某条规则的启用状态（不存在则插入一条抑制记录）。
    ///
    /// 这是**唯一的用户写入入口**：界面只改 `enabled`，不改规则正文，
    /// 也不直接改其他列（其余列由同步器从磁盘投影）。
    pub async fn set_enabled(&self, rule_id: &str, enabled: bool) -> Result<(), CoreError> {
        match &self.pool {
            IndexPool::Global(pool) => {
                let conn = pool.acquire().await?;
                set_enabled_on(conn.inner()?, self.scope, rule_id, enabled)
            }
            IndexPool::Project(pool) => {
                let conn = pool.acquire().await?;
                set_enabled_on(conn.inner()?, self.scope, rule_id, enabled)
            }
        }
    }

    /// 被禁用的规则 id 列表（装配规则集时据此摘除）。
    pub async fn disabled_ids(&self) -> Result<Vec<String>, CoreError> {
        let rows = self.load().await?;
        Ok(rows
            .into_iter()
            .filter(|r| !r.enabled)
            .map(|r| r.rule_id)
            .collect())
    }
}

fn read_rows(
    conn: &rusqlite::Connection,
    scope: RuleScope,
) -> Result<Vec<RuleIndexEntry>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT rule_id, category, name, version, source_path, checksum,
                    enabled, load_status, load_error
             FROM insight_rule_index WHERE scope = ?1 ORDER BY rule_id",
        )
        .map_err(sqlite_err("prepare_read_rule_index"))?;

    let rows = stmt
        .query_map(rusqlite::params![scope.as_str()], |row| {
            let status_raw: String = row.get(7)?;
            Ok(RuleIndexEntry {
                rule_id: row.get(0)?,
                scope,
                category: row.get(1)?,
                name: row.get(2)?,
                version: row.get(3)?,
                source_path: row.get(4)?,
                checksum: row.get(5)?,
                enabled: row.get::<_, i64>(6)? != 0,
                // 库里的值受 CHECK 约束，理论上不会出现未知状态；兜底当 Ok 以免整表读失败
                load_status: RuleLoadStatus::parse(&status_raw).unwrap_or(RuleLoadStatus::Ok),
                load_error: row.get(8)?,
            })
        })
        .map_err(sqlite_err("query_read_rule_index"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_err("read_rule_index_row"))?;

    Ok(rows)
}

fn write_rows(
    conn: &mut rusqlite::Connection,
    scope: RuleScope,
    rows: &[RuleIndexEntry],
) -> Result<(), CoreError> {
    let tx = conn
        .transaction()
        .map_err(sqlite_err("begin_rule_index_tx"))?;

    tx.execute(
        "DELETE FROM insight_rule_index WHERE scope = ?1",
        rusqlite::params![scope.as_str()],
    )
    .map_err(sqlite_err("clear_rule_index"))?;

    for row in rows {
        tx.execute(
            "INSERT OR REPLACE INTO insight_rule_index
                (rule_id, scope, category, name, version, source_path, checksum,
                 enabled, load_status, load_error, loaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, CURRENT_TIMESTAMP)",
            rusqlite::params![
                &row.rule_id,
                scope.as_str(),
                &row.category,
                &row.name,
                &row.version,
                &row.source_path,
                &row.checksum,
                if row.enabled { 1 } else { 0 },
                row.load_status.as_str(),
                row.load_error.as_deref(),
            ],
        )
        .map_err(sqlite_err("insert_rule_index"))?;
    }

    tx.commit().map_err(sqlite_err("commit_rule_index_tx"))?;
    Ok(())
}

/// 幂等置位：行不存在时插入一条**最小抑制记录**（只填 id 与状态，其余列由下次同步补齐）。
fn set_enabled_on(
    conn: &rusqlite::Connection,
    scope: RuleScope,
    rule_id: &str,
    enabled: bool,
) -> Result<(), CoreError> {
    let changed = conn
        .execute(
            "UPDATE insight_rule_index SET enabled = ?3 WHERE scope = ?1 AND rule_id = ?2",
            rusqlite::params![scope.as_str(), rule_id, if enabled { 1 } else { 0 }],
        )
        .map_err(sqlite_err("set_rule_enabled"))?;

    if changed == 0 && !enabled {
        // 抑制记录：专为「库中没有该行」的规则（典型是内置规则）而写。
        // 正文与来源由同步器在下一次扫描时补齐；此处只需让「禁用」这一事实落库。
        conn.execute(
            "INSERT OR IGNORE INTO insight_rule_index
                (rule_id, scope, category, name, version, source_path, checksum,
                 enabled, load_status, load_error, loaded_at)
             VALUES (?1, ?2, '', ?1, '', '', '', 0, 'ok', NULL, CURRENT_TIMESTAMP)",
            rusqlite::params![rule_id, scope.as_str()],
        )
        .map_err(sqlite_err("insert_rule_suppression"))?;
    }

    Ok(())
}

fn sqlite_err(op: &'static str) -> impl Fn(rusqlite::Error) -> CoreError {
    move |e| {
        CoreError::storage(shared::error::StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: op.to_string(),
            reason: e.to_string(),
        })
    }
}

// ==================== 同步编排 ====================

/// 一次同步的结果摘要（供日志与界面展示）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncOutcome {
    /// 索引行总数（含缺失与解析失败的）。
    pub total: usize,
    /// 解析通过且文件在场的条数。
    pub ok: usize,
    /// 解析失败的条数（`load_error` 已落库，界面可直接展示）。
    pub invalid: usize,
    /// 文件已不在的条数。
    pub missing: usize,
    /// 当前被禁用的条数。
    pub disabled: usize,
}

impl SyncOutcome {
    fn from_rows(rows: &[RuleIndexEntry]) -> Self {
        let mut outcome = SyncOutcome {
            total: rows.len(),
            ..Default::default()
        };
        for row in rows {
            match row.load_status {
                RuleLoadStatus::Ok => outcome.ok += 1,
                RuleLoadStatus::Invalid => outcome.invalid += 1,
                RuleLoadStatus::Missing => outcome.missing += 1,
            }
            if !row.enabled {
                outcome.disabled += 1;
            }
        }
        outcome
    }
}

/// 把一个项目的规则索引与磁盘对齐，并把启停结果推给规则装配侧。
///
/// 完整链路：
/// ```text
/// 扫描三层目录 → plan_index（保留用户启停）→ 写库 → apply_disabled_rules
///     → 该项目注册表缓存失效 → 下次 registry_for 按新启停集合重建
/// ```
/// 最后一步是**必需**的：索引只有被消费才有意义，否则用户点了「关闭该规则」
/// 而规则照旧参与分析（索引变成只写不读的装饰）。
///
/// `global_store` / `project_store` 允许为 `None`（例如尚未打开项目时只同步全局层），
/// 两层的磁盘扫描与合并逻辑完全相同，只是落在不同库。
pub async fn sync_project_rules(
    project_root: Option<&Path>,
    global_store: Option<&RuleIndexStore>,
    project_store: Option<&RuleIndexStore>,
    builtin_ids: &HashSet<String>,
) -> Result<SyncOutcome, CoreError> {
    let mut all_rows: Vec<RuleIndexEntry> = Vec::new();

    // 全局层：{系统目录}/insight-rules/
    if let Some(store) = global_store {
        match engine::migration::get_system_dir() {
            Ok(system_dir) => {
                let dir = crate::rule_registry::get_global_rules_dir(&system_dir);
                let planned = plan_index(
                    scan_scope_dir(&dir, RuleScope::Global),
                    &store.load().await?,
                    builtin_ids,
                );
                store.replace(&planned).await?;
                all_rows.extend(planned);
            }
            Err(e) => tracing::warn!("System dir unavailable, skipping global rule index: {}", e),
        }
    }

    // 项目层：{项目}/.RSmeta/insight-rules/
    if let (Some(store), Some(root)) = (project_store, project_root) {
        let dir = crate::rule_registry::get_project_rules_dir(root);
        let planned = plan_index(
            scan_scope_dir(&dir, RuleScope::Project),
            &store.load().await?,
            builtin_ids,
        );
        store.replace(&planned).await?;
        all_rows.extend(planned);
    }

    let outcome = SyncOutcome::from_rows(&all_rows);
    crate::apply_disabled_rules(
        project_root,
        all_rows
            .iter()
            .filter(|r| !r.enabled)
            .map(|r| r.rule_id.clone()),
    );

    tracing::info!(
        "Insight rule index synced: {} rule(s) ({} invalid, {} missing, {} disabled)",
        outcome.total,
        outcome.invalid,
        outcome.missing,
        outcome.disabled
    );

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const OK_RULE: &str = r#"
[meta]
id = "demo-rule"
name = "示例规则"
category = "column"
applies_to = ["Numeric"]
version = "1.0"
builtin = false

[query]
template = "SELECT 1"
parameters = []
"#;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_rule_index_{}_{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn entry(id: &str, scope: RuleScope, enabled: bool) -> RuleIndexEntry {
        RuleIndexEntry {
            rule_id: id.to_string(),
            scope,
            category: "column".to_string(),
            name: id.to_string(),
            version: "1.0".to_string(),
            source_path: format!("{}.rule.toml", id),
            checksum: "c".to_string(),
            enabled,
            load_status: RuleLoadStatus::Ok,
            load_error: None,
        }
    }

    #[test]
    fn test_scan_reads_valid_and_invalid_files() {
        let dir = temp_dir("scan");
        std::fs::write(dir.join("ok.rule.toml"), OK_RULE).expect("write ok");
        std::fs::write(dir.join("bad.rule.toml"), "this is not toml {{{").expect("write bad");

        let rows = scan_scope_dir(&dir, RuleScope::Project);
        assert_eq!(rows.len(), 2, "合法与非法文件都要进索引");

        let ok = rows.iter().find(|r| r.rule_id == "demo-rule").expect("合法规则");
        assert_eq!(ok.load_status, RuleLoadStatus::Ok);
        assert_eq!(ok.category, "column");
        assert_eq!(ok.name, "示例规则");
        assert_eq!(ok.source_path, "ok.rule.toml", "路径应相对作用域目录");
        assert_eq!(ok.checksum.len(), 64, "应为 SHA256 十六进制");
        assert!(ok.enabled, "新扫描默认启用");

        // 解析失败时拿不到 meta.id，用**文件名**兜底（`bad.rule.toml` → `bad.rule`，
        // 仅剥最后一个扩展名，保证同目录下多个 .v2.toml 之类不会撞成同名）。
        let bad = rows
            .iter()
            .find(|r| r.rule_id == "bad.rule")
            .expect("非法规则以文件名兜底");
        assert_eq!(bad.load_status, RuleLoadStatus::Invalid);
        assert!(bad.load_error.is_some(), "必须记下错误原文");
        assert!(
            bad.load_error.as_deref().unwrap().contains("TOML parse error"),
            "错误原文应可读: {:?}",
            bad.load_error
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scan_ignores_builtin_scope_and_missing_dirs() {
        let dir = temp_dir("skip");
        std::fs::write(dir.join("a.rule.toml"), OK_RULE).expect("write");
        assert!(
            scan_scope_dir(&dir, RuleScope::Builtin).is_empty(),
            "内置层由 include_dir 编译期已知，不入库"
        );
        assert!(
            scan_scope_dir(&dir.join("nope"), RuleScope::Project).is_empty(),
            "目录不存在应返回空表而不是报错"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scan_recurses_subdirs() {
        let dir = temp_dir("recurse");
        std::fs::create_dir_all(dir.join("column")).expect("mkdir");
        std::fs::write(dir.join("column/ok.rule.toml"), OK_RULE).expect("write");
        let rows = scan_scope_dir(&dir, RuleScope::Project);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_path, "column/ok.rule.toml");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 作用域 → 目录的派发只有这一处（`.RSmeta` 的拼写有多个历史版本）
    #[test]
    fn test_rule_dir_dispatch() {
        let dir = rule_dir(Some(Path::new("/p")), RuleScope::Project).expect("项目层目录");
        assert!(dir.to_string_lossy().contains(".RSmeta"), "{dir:?}");
        assert!(dir.to_string_lossy().ends_with("insight-rules"), "{dir:?}");
        assert_eq!(
            rule_dir(Some(Path::new("/p")), RuleScope::Builtin),
            None,
            "内置层内嵌在二进制里，没有目录"
        );
        assert_eq!(
            rule_dir(None, RuleScope::Project),
            None,
            "无项目就没有项目层目录"
        );
    }

    /// 新建规则的模板：**能解析**（否则用户第一眼看到的是一条红错），
    /// 且因 `applies_to` 为空而暂不参与分析。
    #[test]
    fn test_new_rule_template_parses_and_stays_inert() {
        let text = new_rule_template("new-rule");
        let parsed = parse_rule_toml(&text).expect("模板必须能解析");
        assert_eq!(parsed.meta.id, "new-rule");
        assert!(!parsed.meta.builtin);
        assert!(
            parsed.meta.applies_to.is_empty(),
            "空 applies_to = 未填之前不参与任何列的分析"
        );
        assert!(
            crate::builtin_registry()
                .all_rules()
                .iter()
                .all(|r| r.meta.id != parsed.meta.id),
            "模板 id 不得与内置规则撞车"
        );
    }

    /// 文件名与 id 都避让已有文件：两份模板同 id 会让用户编辑的那条被另一条默默顶掉
    #[test]
    fn test_create_rule_file_avoids_existing_names() -> Result<(), CoreError> {
        let root = temp_dir("create_rule");
        let first = create_rule_file(Some(&root), RuleScope::Project)?;
        let second = create_rule_file(Some(&root), RuleScope::Project)?;
        assert_ne!(first, second, "第二次新建不得覆盖第一份模板");
        assert!(first.exists() && second.exists());
        assert!(first.to_string_lossy().ends_with("new-rule.rule.toml"), "{first:?}");
        assert!(
            second.to_string_lossy().ends_with("new-rule-2.rule.toml"),
            "{second:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    /// 用户的启停选择必须活过同步（磁盘现状覆盖不了它）。
    #[test]
    fn test_plan_preserves_user_enabled_choice() {
        let scanned = vec![entry("demo-rule", RuleScope::Project, true)];
        let existing = vec![entry("demo-rule", RuleScope::Project, false)];

        let planned = plan_index(scanned, &existing, &HashSet::new());
        assert_eq!(planned.len(), 1);
        assert!(!planned[0].enabled, "库中被禁用的规则同步后仍应禁用");
    }

    /// 内置规则的抑制记录（项目库里 enabled=false，但磁盘上没有对应文件）必须保留。
    #[test]
    fn test_plan_keeps_builtin_suppression_records() {
        let scanned = vec![entry("demo-rule", RuleScope::Project, true)];
        let existing = vec![entry("null-check", RuleScope::Project, false)]; // 指向内置规则
        let builtin_ids: HashSet<String> = ["null-check".to_string()].into_iter().collect();

        let planned = plan_index(scanned, &existing, &builtin_ids);
        assert_eq!(planned.len(), 2, "抑制记录不应被同步清掉");

        let suppression = planned.iter().find(|r| r.rule_id == "null-check").unwrap();
        assert!(!suppression.enabled);
        assert_eq!(
            suppression.load_status,
            RuleLoadStatus::Ok,
            "内置规则本身没坏，只是本项目禁用它"
        );
    }

    /// 规则文件被删除：索引保留该行但转 `missing`，不无声消失。
    #[test]
    fn test_plan_marks_removed_file_as_missing() {
        let scanned: Vec<RuleIndexEntry> = vec![];
        let existing = vec![entry("gone-rule", RuleScope::Project, true)];

        let planned = plan_index(scanned, &existing, &HashSet::new());
        assert_eq!(planned.len(), 1, "行应保留");
        assert_eq!(planned[0].load_status, RuleLoadStatus::Missing);
        assert!(!planned[0].is_present());
    }

    /// 同名规则跨作用域互不干扰（全局层的启停不影响项目层）。
    #[test]
    fn test_plan_scopes_are_independent() {
        let scanned = vec![entry("same-id", RuleScope::Project, true)];
        let existing = vec![entry("same-id", RuleScope::Global, false)];

        let planned = plan_index(scanned, &existing, &HashSet::new());
        let project_row = planned
            .iter()
            .find(|r| r.scope == RuleScope::Project)
            .expect("项目层行");
        assert!(project_row.enabled, "全局层的禁用不应波及项目层");
        let global_row = planned
            .iter()
            .find(|r| r.scope == RuleScope::Global)
            .expect("全局层行");
        assert!(!global_row.enabled);
    }

    /// 索引表的真实读写：整体替换 + 启停开关 + 禁用清单。
    #[tokio::test]
    async fn test_index_store_roundtrip() -> Result<(), CoreError> {
        let root = temp_dir("store");
        let db = engine::persistence::ProjectDatabaseManager::open(&root, 2).await?;
        let store = RuleIndexStore::project(db.sqlite_pool());

        // 初次为空
        assert!(store.load().await?.is_empty());

        let rows = vec![
            entry("rule-a", RuleScope::Project, true),
            RuleIndexEntry {
                load_status: RuleLoadStatus::Invalid,
                load_error: Some("第 3 行未知字段 `outputs`".to_string()),
                ..entry("rule-b", RuleScope::Project, true)
            },
        ];
        store.replace(&rows).await?;

        let loaded = store.load().await?;
        assert_eq!(loaded.len(), 2);
        let b = loaded.iter().find(|r| r.rule_id == "rule-b").unwrap();
        assert_eq!(b.load_status, RuleLoadStatus::Invalid);
        assert_eq!(
            b.load_error.as_deref(),
            Some("第 3 行未知字段 `outputs`"),
            "校验错误原文必须能回读（这是索引存在的核心价值）"
        );

        // 禁用一条 + 对库中不存在的规则写抑制记录（内置规则场景）
        store.set_enabled("rule-a", false).await?;
        store.set_enabled("null-check", false).await?;
        let disabled = store.disabled_ids().await?;
        assert!(disabled.contains(&"rule-a".to_string()));
        assert!(disabled.contains(&"null-check".to_string()));

        // 整体替换后退回一条并重启一条
        store.set_enabled("rule-a", true).await?;
        store.replace(&rows).await?;
        assert!(
            store
                .load()
                .await?
                .iter()
                .all(|r| r.enabled),
            "replace 会重置启停状态，故它只应由同步器在校准磁盘现状时调用"
        );

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    /// 端到端：扫描 → 合并 → 写库 → 应用启停 → 规则集生效。
    ///
    /// 这一条同时验证 F1 类问题的防线：**解析失败的规则必须能被看见**，
    /// 而不是像以前那样只写一行 warn 日志。
    #[tokio::test]
    async fn test_sync_project_rules_end_to_end() -> Result<(), CoreError> {
        // 本测试会写进程级禁用集合缓存，必须与同类的缓存测试串行。
        let _guard = crate::tests::rule_state_guard();

        let root = temp_dir("sync_e2e");
        let db = engine::persistence::ProjectDatabaseManager::open(&root, 2).await?;
        let store = RuleIndexStore::project(db.sqlite_pool());

        let rules_dir = crate::rule_registry::get_project_rules_dir(&root);
        std::fs::create_dir_all(&rules_dir).expect("mkdir rules dir");
        std::fs::write(rules_dir.join("demo.rule.toml"), OK_RULE).expect("write demo");
        std::fs::write(rules_dir.join("broken.rule.toml"), "definitely not toml {{")
            .expect("write broken");

        let builtin = crate::builtin_rule_ids();
        let first = sync_project_rules(Some(&root), None, Some(&store), &builtin).await?;
        assert_eq!(first.total, 2, "两个文件都要进索引");
        assert_eq!(first.ok, 1);
        assert_eq!(first.invalid, 1, "坏规则必须被索引到（而不是静默跳过）");
        assert_eq!(first.missing, 0);

        let rows = store.load().await?;
        let bad = rows
            .iter()
            .find(|r| r.load_status == RuleLoadStatus::Invalid)
            .expect("索引里应有坏规则");
        assert!(
            bad.load_error
                .as_deref()
                .is_some_and(|e| e.contains("TOML parse error")),
            "错误原文应落库供界面展示: {:?}",
            bad.load_error
        );

        // 禁用一条自定义规则 → 再同步 → 启停被保留，且规则集里不再有它
        store.set_enabled("demo-rule", false).await?;
        let second = sync_project_rules(Some(&root), None, Some(&store), &builtin).await?;
        assert_eq!(second.disabled, 1, "同步不得抹掉用户的禁用");
        assert!(
            !crate::with_rules(Some(&root), |r| Ok(r.get("demo-rule").is_some()))?,
            "被禁用的规则不应出现在生效规则集里"
        );

        // 文件被删 → 转 missing 保留（不无声消失）
        std::fs::remove_file(rules_dir.join("demo.rule.toml")).expect("remove demo");
        let third = sync_project_rules(Some(&root), None, Some(&store), &builtin).await?;
        assert_eq!(third.missing, 1, "文件没了应转 missing 而不是删行");
        assert_eq!(third.total, 2, "总数不变：missing 行仍在索引里");

        crate::clear_disabled_rules_cache();
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
