//! 洞察服务门面 · 规则门面（自 service/mod.rs 拆分，纯位移）

use super::*;

impl InsightService {
    /// 执行指定 id 的规则（含 QualityRule 质量门控）。调用方需已持有 DuckDB 锁。
    pub fn execute_insight_rule(
        project_root: Option<&Path>,
        rule_id: &str,
        conn: &duckdb::Connection,
        params: &HashMap<String, String>,
    ) -> Result<ExecutionResult, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::execute_insight_rule(registry, rule_id, conn, params)
        })
    }

    /// 列出规则（可按分类过滤）。返回项含 `scope` / `scope_path`，供界面按作用域分组。
    pub fn list_insight_rules(
        project_root: Option<&Path>,
        category: Option<&str>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::list_insight_rules(registry, category)
        })
    }

    /// 列出适用于某列类型的规则。
    pub fn list_rules_for_column(
        project_root: Option<&Path>,
        column_type: &str,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::list_rules_for_column(registry, column_type)
        })
    }

    /// 重新加载规则（丢弃缓存并按三层重建），返回生效规则总数。
    ///
    /// 常规改动由目录监听（[`watcher`]）自动生效；本方法是兵底与排障入口。
    pub fn reload_insight_rules(project_root: Option<&Path>) -> usize {
        crate::reload_insight_rules(project_root)
    }

    // ==================== 规则管理（Phase 2.3 / 2.4） ====================

    /// 规则管理对话框的数据：**先同步索引再读数**。
    ///
    /// 同步是必需的：索引刷新时机与「谁在看它」对齐（D23），打开对话框就是那个时机。
    /// 同步顺带把启停集合推给装配侧、失效该项目注册表缓存——索引只有被消费才有意义。
    ///
    /// 阻塞（要开项目库 + 读写 SQLite），调用方负责放后台。
    pub fn rules_data(project_root: Option<&Path>) -> Result<RulesData, CoreError> {
        let stores = IndexStores::open(project_root)?;
        stores.sync(project_root)?;
        stores.snapshot(project_root)
    }

    /// 项目规则**信任门**：记录用户的一次决定，并返回刷新后的数据（Q1 ③ / D53）。
    ///
    /// 决定落在全局库（项目库在被信任前就是不可信输入，不能自己给自己发信任），
    /// 随后推给装配侧的缓存并失效该项目注册表——**下次取数就能看见项目层已经装上**。
    ///
    /// 阻塞（开全局库 + 写库 + 重扫索引），调用方负责放后台。
    pub fn decide_project_rules_trust(
        project_root: Option<&Path>,
        state: RuleTrust,
    ) -> Result<RulesData, CoreError> {
        let root = project_root.ok_or_else(no_project)?;
        let manager = engine::migration::get_global_db_manager().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "全局库不可用，无法保存规则信任状态".to_string(),
            ))
        })?;

        block_on(rule_trust::write(&manager.sqlite_pool(), root, state))?;
        crate::apply_project_rule_trust(root, state);

        // 信任状态变了 → 重新装配 + 重新扫索引（上面已失效缓存，这里取到的就是新集合）
        Self::rules_data(project_root)
    }

    /// 切换某条规则的启停，返回刷新后的数据。
    ///
    /// 写的是**规则所在层**的索引；内置规则没有自己的索引行，按**抑制记录**写到项目库
    /// （没有项目时才落全局库）——这正是 `plan_index` 保留抑制记录的那条路径。
    pub fn toggle_rule(
        project_root: Option<&Path>,
        scope: RuleScope,
        rule_id: &str,
        enabled: bool,
    ) -> Result<RulesData, CoreError> {
        let stores = IndexStores::open(project_root)?;
        let target = stores.write_scope(scope);
        let store = stores.store(target).ok_or_else(|| {
            CoreError::common(CommonError::General(format!(
                "{}层索引不可用，无法保存启停状态",
                target.label()
            )))
        })?;
        block_on(store.set_enabled(rule_id, enabled))?;
        stores.sync(project_root)?;
        stores.snapshot(project_root)
    }

    /// 新建规则（建目录 + 写模板），返回新文件路径供宿主打开。
    ///
    /// 目录在**首次写入时**创建（K7）：不在启动时预设空目录，也不假定用户已手工建好。
    pub fn create_rule_file(
        project_root: Option<&Path>,
        scope: RuleScope,
    ) -> Result<PathBuf, CoreError> {
        indexer::create_rule_file(project_root, scope)
    }
}
