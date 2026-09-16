//! RdataStation v2 洞察与规则引擎 crate（insight）
//!
//! 承载 M8 洞察模块：
//! - 规则引擎（`rule_*`，自 v1 `core/insight`）：内置/用户规则执行、注册表、热加载
//! - 分析服务（`insight_engine` / `quality_scorer` / `table_profile_service`，自 v1 `core/services`）
//! - 内置规则资产 `insight-rules/`（include_dir 编译时嵌入）
//!
//! 依赖方向：insight → engine → shared（不依赖任何业务 Feature crate）。
//!
//! # 规则作用域与注册表生命周期
//!
//! 规则分三层，后加载者整体覆盖前者（见 [`RuleScope`]）：
//! `Builtin`（内嵌，18 条）→ `Global`（`{系统目录}/insight-rules/`）→ `Project`（`{项目}/.RSmeta/insight-rules/`）。
//!
//! 注册表**按项目根缓存**（[`registry_for`]），不是进程级单例：
//! 「一实例一项目」下切换项目必须换规则集，而进程单例只能靠调用方记得手动重载——
//! v1 正是如此（在 `project_commands` 两处手动调用），v2 一期干脆漏掉了调用，
//! 结果是用户规则静默不生效。按项目根缓存后，这条「调用方义务」由类型消掉。
//!
//! 正文以文件为唯一真相源；索引与启停状态另见 `insight_rule_index` 表（`service::indexer`）。

pub mod commands;
pub mod insight_engine;
pub mod insight_view;
pub mod jobs;
pub mod model;
pub mod quality_scorer;
pub mod rule;
pub mod rule_executor;
pub mod rule_registry;
pub mod rule_types;
pub mod rule_view;
pub mod schema_analyzer;
pub mod service;
pub mod store;
pub mod table_profile_service;
pub mod ui;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use include_dir::{include_dir, Dir};
use shared::error::{CommonError, CoreError};

pub use model::types::{
    BooleanStats, ColumnInsightFull, ColumnQualityEntry, ColumnStats, ColumnStatsDetail,
    DateTimeStats, DistributionBin, ExtremeValue, NumericStats, QualityDimension, QualityScore,
    TableColumnMeta, TableProfile, TableQuality, TextFrequency, TextStats,
};
pub use store::{ProjectInsightStores, InsightMetaStore, InsightSnapshotMeta, InsightStorage};

pub use insight_engine::{detect_extremes, get_temp_table_profile};
pub use rule::{RuleLoadFailure, RuleScope, RuleSource};
pub use rule_executor::RuleExecutor;
pub use rule_registry::{
    get_global_rules_dir, get_project_rules_dir, parse_rule_toml, RuleRegistry, RULES_DIR_NAME,
};
pub use service::indexer::{
    plan_index, scan_scope_dir, sync_project_rules, RuleIndexEntry, RuleIndexStore, RuleLoadStatus,
    SyncOutcome,
};
pub use service::{InsightErrorInfo, InsightService};
pub use service::watcher::{
    clear_index_stale, index_is_stale, rules_fingerprint, set_watched_project_root, watch_dirs,
    watched_project_root, RulesWatcher, DEFAULT_POLL_INTERVAL,
};
pub use store::{
    snapshot_checksum, InsightColumnStore, InsightSchemaReportStore, InsightStorageStats,
    InsightTableReportStore, InsightVersionEntry,
};

// ===== 视图层（M8 Phase 1；归属决策 D21 = 方案 A：视图随 crate）=====
//
// 宿主（workbench）经这些类型装配右 Dock 面板：面板自身不做 I/O，取数由宿主发起。
pub use insight_view::{InsightEvent, InsightView};
pub use model::{
    ColumnKind, ColumnProfileView, DimensionView, DistributionBar, Emphasis, InsightPanelState,
    InsightTarget, KeyValueRow, MultiColumnView, MultiResultView, MultiRuleView, NoteLevel,
    PanelData, PanelTab, QualityNote, SampleCell, ScoreView, StatRow, TableColumnView,
    TableEvalProgress, TableProfileView, TableQualityView,
};
pub use service::rule_params;
pub use quality_scorer::Grade;
pub use rule_view::{
    build_rules_data, RuleDataInput, RuleGroupView, RuleRowStatus, RuleRowView, RulesData,
    RulesDialogState, RulesEvent, RulesView,
};
pub use rule_types::{
    ExecutionResult, OutputField, QualityCheck, QualityReport, QualityRule, RenderHint, RuleFile,
    RuleMeta, RuleQuery,
};
pub use schema_analyzer::{
    ForeignKeyCandidate, OrphanTable, RedundantColumn, SchemaAnalyzer, SchemaInsightReport,
    TableColumnInfo, TypeMismatch, TypeMismatchEntry,
};

pub const BUILTIN_RULES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/insight-rules");

/// 注册表缓存：`项目根 → 注册表`。
///
/// `None` 键表示「无项目」场景（仅内置 + 全局层）。缓存项在 [`reload_insight_rules`]
/// 时被整体替换；「一实例一项目」下键的数量等于本次会话打开过的项目数，不会无限增长。
type RegistryCache = RwLock<HashMap<Option<PathBuf>, Arc<RwLock<RuleRegistry>>>>;

static REGISTRIES: OnceLock<RegistryCache> = OnceLock::new();

fn registry_cache() -> &'static RegistryCache {
    REGISTRIES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 各项目的**禁用规则集**（`rule_id` 集合）。
///
/// 为什么用同步可读的静态缓存，而不是每次查库：
/// 规则装配（[`build_registry`]）与列画像计算都在**同步**上下文里（`with_rules` →
/// `get_column_insight_full`），而查库是异步的。若把整条链路改成异步，
/// 会外溢到大量同步调用方——为一份**很小且变动很少**的集合不值得。
///
/// 因此方向反过来：**异步侧（索引同步器）把结果推给这个缓存**，
/// 同步侧只读。来源是 `insight_rule_index` 中 `enabled = 0` 的记录，
/// 其中指向内置规则的记录即「在项目里禁用了某条内置规则」。
type DisabledRules = RwLock<HashMap<Option<PathBuf>, std::collections::HashSet<String>>>;

static DISABLED_RULES: OnceLock<DisabledRules> = OnceLock::new();

fn disabled_rules() -> &'static DisabledRules {
    DISABLED_RULES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 取出某项目的禁用规则快照（**克隆**后立即释放锁，不跨锁调用其他锁）。
fn disabled_snapshot(key: &Option<PathBuf>) -> std::collections::HashSet<String> {
    disabled_rules()
        .read()
        .map(|m| m.get(key).cloned().unwrap_or_default())
        .unwrap_or_default()
}

/// 缓存键：项目根做规范化，避免同一目录的多种写法（相对 / 绝对 / 带 `..`）各自建一份。
fn cache_key(project_root: Option<&Path>) -> Option<PathBuf> {
    project_root.map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
}

/// 仅加载内置规则的注册表（不触碰文件系统与用户目录）。
///
/// 用于单测与「只要内置规则」的场景：结果与机器上的用户目录内容无关，因此可断言。
pub fn builtin_registry() -> RuleRegistry {
    let mut registry = RuleRegistry::new();
    if let Err(e) = registry.load_builtin(&BUILTIN_RULES_DIR) {
        tracing::warn!("Failed to load built-in insight rules: {}", e);
    }
    registry
}

/// 组装某项目的完整规则集：内置 → 全局 → 项目，逐层覆盖，再摘掉被禁用的规则。
fn build_registry(project_root: Option<&Path>) -> RuleRegistry {
    let key = cache_key(project_root);
    let mut registry = builtin_registry();

    // 全局层：{系统目录}/insight-rules/。系统目录不可用时降级为「无全局层」——
    // 此时内置规则仍可用，不应因此让洞察整体失效。
    match engine::migration::get_system_dir() {
        Ok(system_dir) => {
            let dir = get_global_rules_dir(&system_dir);
            match registry.load_from_dir(&dir, RuleScope::Global) {
                Ok(0) => {}
                Ok(count) => tracing::info!("Loaded {} global insight rules from {}", count, dir.display()),
                Err(e) => tracing::warn!(
                    "Failed to load global insight rules from {}: {}",
                    dir.display(),
                    e
                ),
            }
        }
        Err(e) => tracing::warn!("System dir unavailable, skipping global insight rules: {}", e),
    }

    // 项目层：{项目}/.RSmeta/insight-rules/。
    if let Some(root) = project_root {
        let dir = get_project_rules_dir(root);
        match registry.load_from_dir(&dir, RuleScope::Project) {
            Ok(0) => {}
            Ok(count) => tracing::info!("Loaded {} project insight rules from {}", count, dir.display()),
            Err(e) => tracing::warn!(
                "Failed to load project insight rules from {}: {}",
                dir.display(),
                e
            ),
        }
    }

    for failure in registry.failures() {
        tracing::warn!(
            "Insight rule failed to load [{}] {}: {}",
            failure.scope.label(),
            failure.path,
            failure.error
        );
    }

    // 启停：索引表里 enabled = 0 的规则在装配时摘掉。
    // 这一步必须**在覆盖之后**做——禁用的是「生效的那一条」，与它来自哪一层无关。
    let disabled = disabled_snapshot(&key);
    if !disabled.is_empty() {
        let removed = registry.remove_rules(&disabled);
        if removed > 0 {
            tracing::info!("{} insight rule(s) disabled for this project", removed);
        }
    }

    registry
}

/// 内置规则的 id 集合。
///
/// 索引器用它区分两类「库里多出来的行」：
/// - id 属于内置规则 → 这是**用户的抑制记录**（在项目里关掉了某条内置规则），必须保留；
/// - 否则 → 规则文件被删，转 `missing` 保留。
pub fn builtin_rule_ids() -> std::collections::HashSet<String> {
    builtin_registry()
        .all_rules()
        .iter()
        .map(|r| r.meta.id.clone())
        .collect()
}

/// 取某项目的规则注册表（首次访问时按三层组装并缓存）。
///
/// `project_root` 为 `None` 表示当前无项目：只加载内置层与全局层。
pub fn registry_for(project_root: Option<&Path>) -> Arc<RwLock<RuleRegistry>> {
    let key = cache_key(project_root);

    if let Ok(cache) = registry_cache().read() {
        if let Some(existing) = cache.get(&key) {
            return existing.clone();
        }
    }

    let registry = Arc::new(RwLock::new(build_registry(project_root)));
    match registry_cache().write() {
        Ok(mut cache) => cache.entry(key).or_insert(registry).clone(),
        Err(e) => {
            tracing::warn!("Failed to acquire registry cache write lock: {}", e);
            registry
        }
    }
}

/// 在指定项目的规则集上执行闭包（内部处理缓存查找与读锁）。
///
/// 服务层与视图层统一走这个入口，避免各处重复「取表 → 加锁 → 解引用」样板，
/// 也保证锁的作用域与错误文案一致。
pub fn with_rules<T>(
    project_root: Option<&Path>,
    f: impl FnOnce(&RuleRegistry) -> Result<T, CoreError>,
) -> Result<T, CoreError> {
    let registry = registry_for(project_root);
    let guard = registry.read().map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "规则注册表锁定失败：{}",
            e
        )))
    })?;
    f(&guard)
}

/// 把某项目的禁用规则集推给同步侧缓存，并使该项目的注册表缓存失效（下次访问重建）。
///
/// 由索引同步器在写完 `insight_rule_index` 后调用：
/// `扫描磁盘 → plan_index → 写库 → apply_disabled_rules → 下次 registry_for 按新集合重建`。
/// 缓存失效是必需的——注册表是「已应用启停后的结果」，禁用集合变了它就不再有效。
pub fn apply_disabled_rules(project_root: Option<&Path>, disabled: impl IntoIterator<Item = String>) {
    let key = cache_key(project_root);
    let set: std::collections::HashSet<String> = disabled.into_iter().collect();

    match disabled_rules().write() {
        Ok(mut map) => {
            map.insert(key.clone(), set);
        }
        Err(e) => {
            tracing::warn!("Failed to update disabled rule set: {}", e);
            return;
        }
    }

    match registry_cache().write() {
        Ok(mut cache) => {
            cache.remove(&key);
        }
        Err(e) => tracing::warn!("Failed to invalidate registry cache: {}", e),
    }
}

/// 重新加载某项目的规则（丢弃缓存项后按三层重建），返回加载后的规则总数。
///
/// 适用场景：用户手工改动规则文件后的兜底刷新与排障。
/// 常规改动无需调用——目录监听（`service::watcher`）会在文件变更时触发。
pub fn reload_insight_rules(project_root: Option<&Path>) -> usize {
    let key = cache_key(project_root);
    let fresh = build_registry(project_root);
    let count = fresh.rule_count();

    match registry_cache().write() {
        Ok(mut cache) => {
            cache.insert(key, Arc::new(RwLock::new(fresh)));
            tracing::info!("Reloaded insight rules: {} rule(s) in effect", count);
        }
        Err(e) => tracing::warn!("Failed to acquire registry cache write lock: {}", e),
    }

    count
}

/// 清空**注册表**缓存（项目关闭时调用，避免缓存持有已关闭项目的路径）。
///
/// **刻意不动禁用集合**：注册表是「已应用启停后的结果」，而禁用集合是用户决策本身，
/// 生命周期不同。两者一起清会让「禁用后立即读取」这类场景在并发下不可测（测试踩过）。
/// 需要整体重置时用 [`clear_rule_caches`]。
pub fn clear_registry_cache() {
    match registry_cache().write() {
        Ok(mut cache) => cache.clear(),
        Err(e) => tracing::warn!("Failed to clear registry cache: {}", e),
    }
}

/// 清空禁用集合缓存。
pub fn clear_disabled_rules_cache() {
    match disabled_rules().write() {
        Ok(mut map) => map.clear(),
        Err(e) => tracing::warn!("Failed to clear disabled rule cache: {}", e),
    }
}

/// 整体重置规则相关的进程级缓存（注册表 + 禁用集合）。
///
/// 适用：项目关闭、切换项目后做彻底重置、测试前置清理。
pub fn clear_rule_caches() {
    clear_registry_cache();
    clear_disabled_rules_cache();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 串行化所有**会改动进程级缓存**的测试。
    ///
    /// 规则注册表缓存与禁用集合都是进程级静态量，并行跑时一个测试的清理
    /// 会在另一个测试的「写入 → 断言」之间生效（实测为约 1/5 概率的间歇失败）。
    /// 涉及写入的测试都必须先拿这把锁；只读的测试不需要。
    pub(crate) static RULE_STATE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 取测试锁（容忍中毒：其他测试 panic 不应连带失败）。
    pub(crate) fn rule_state_guard() -> std::sync::MutexGuard<'static, ()> {
        RULE_STATE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 内嵌规则数与 `BUILTIN_RULE_COUNT` 常量必须一致——
    /// 该常量是文档与测试的对齐基准，增删规则文件时若忘记改，这里会失败。
    #[test]
    fn test_builtin_count_matches_constant() {
        let registry = builtin_registry();
        assert_eq!(
            registry.rule_count(),
            rule_types::BUILTIN_RULE_COUNT,
            "内嵌规则数与 BUILTIN_RULE_COUNT 不一致（增删规则后请同步常量）"
        );
        assert!(
            registry.failures().is_empty(),
            "内置规则必须全部可解析，失败项: {:?}",
            registry.failures()
        );
    }

    /// 内置规则三层分组中全部落在 `Builtin` 层。
    #[test]
    fn test_builtin_registry_all_builtin_scope() {
        let registry = builtin_registry();
        assert_eq!(
            registry.list_by_scope(RuleScope::Builtin).len(),
            registry.rule_count()
        );
        assert_eq!(registry.scopes_present(), vec![RuleScope::Builtin]);
    }

    /// 缓存行为：同一键命中同一实例；清注册表缓存后重建。
    #[test]
    fn test_registry_cache_hit_and_clear() {
        let _guard = rule_state_guard();

        let first = registry_for(None);
        let again = registry_for(None);
        assert!(Arc::ptr_eq(&first, &again), "同一键应命中同一缓存项");

        clear_registry_cache();
        let rebuilt = registry_for(None);
        assert!(!Arc::ptr_eq(&first, &rebuilt), "清缓存后应重建注册表");
    }

    /// `with_rules` 在无项目场景下也能取到规则（内置层即可用）。
    #[test]
    fn test_with_rules_without_project() -> Result<(), CoreError> {
        let count = with_rules(None, |reg| Ok(reg.rule_count()))?;
        assert_eq!(count, rule_types::BUILTIN_RULE_COUNT);
        Ok(())
    }

    /// 禁用必须真正生效：推送禁用集后，重建的规则集里不再有该规则。
    ///
    /// 这是索引表 `enabled = 0` 的**消费端**——索引只有被消费才有意义，
    /// 否则用户点了「关闭该规则」而规则照旧参与分析。
    #[test]
    fn test_disabled_rules_are_removed_from_registry() -> Result<(), CoreError> {
        let _guard = rule_state_guard();

        // 用一个不存在的项目根，避免碰到真实项目目录；缓存键会退化为该相对路径。
        let root = std::path::PathBuf::from("__rds_test_disabled_project__");
        clear_disabled_rules_cache();

        // 基线：内置规则齐全（机器上若另有全局规则会比内置更多，故只断言下界）
        let before = with_rules(Some(&root), |reg| Ok(reg.rule_count()))?;
        assert!(before >= rule_types::BUILTIN_RULE_COUNT);
        assert!(with_rules(Some(&root), |reg| Ok(reg.get("null-check").is_some()))?);

        // 禁用一条内置规则（正是「抑制记录」的场景）
        apply_disabled_rules(Some(&root), ["null-check".to_string()]);

        let after = with_rules(Some(&root), |reg| Ok(reg.rule_count()))?;
        assert_eq!(after, before - 1, "被禁用的规则应从规则集中摘掉");
        assert!(
            !with_rules(Some(&root), |reg| Ok(reg.get("null-check").is_some()))?,
            "null-check 应不可见"
        );
        assert!(
            with_rules(Some(&root), |reg| Ok(reg.source_of("null-check").is_none()))?,
            "来源描述也要一并清理"
        );

        // 恢复启用后规则回归
        apply_disabled_rules(Some(&root), Vec::<String>::new());
        assert_eq!(
            with_rules(Some(&root), |reg| Ok(reg.rule_count()))?,
            before,
            "清空禁用集后规则应全部回来"
        );

        clear_disabled_rules_cache();
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
