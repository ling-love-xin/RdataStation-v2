//! 洞察服务层（M8）：服务门面与编排，不实现统计算法。
//!
//! # 分层
//!
//! | 内容 | 位置 | 说明 |
//! | --- | --- | --- |
//! | 统计算法 | `crate::insight_engine` / `quality_scorer` / `schema_analyzer` | 纯计算 |
//! | **服务门面** | 本文件 [`InsightService`] | 把「取规则集 + 调用算法」合成一步，供视图层使用 |
//! | 规则索引同步 | [`indexer`] | 扫描磁盘 → 合并 → 写库 → 应用启停 |
//! | 快照持久化编排 | [`persistence`] | 快照保存 / 历史 / 清理 / 存储统计 / 表级评估 |
//! | 目录监听 | [`watcher`] | 内容哈希轮询触发重载 |
//!
//! # 门面为什么存在
//!
//! 基础统计本身由 TOML 规则驱动，因此每次分析都要先拿到**当前项目的规则集**
//! （[`crate::with_rules`]）。若把这步留给调用方，每个视图入口都要重复
//! 「取缓存 → 加读锁 → 传引用」三行样板，且容易漏传 `project_root` 而用错项目的规则。
//! 门面把这一步收在一处。
//!
//! 归属变更（M8 Phase 0 / 0.2）：本门面的洞察方法自
//! `workbench/src/services/result_service.rs` 迁入；结果集相关方法（执行 / 导出 /
//! 单元格回写）留在 workbench——它们不属于洞察。

pub mod indexer;
pub mod persistence;
pub mod rule_trust;
pub mod watcher;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use engine::persistence::ProjectDatabaseManager;
use shared::error::{CommonError, CoreError};

use crate::model::types::{ColumnInsightFull, ColumnStats, QualityScore, TableQuality};
use crate::model::{
    ColumnProfileView, HistoryView, MultiColumnView, MultiResultView, MultiRuleView, QualityNote,
    TableProfileView, HISTORY_PAGE_SIZE,
};
use crate::rule::RuleScope;
use crate::rule_types::RuleMeta;
use crate::rule_view::{build_rules_data, RuleDataInput, RulesData};
use crate::schema_view::SchemaReportView;
use crate::service::indexer::{rule_dir, sync_project_rules, RuleIndexStore, SyncOutcome};
use crate::service::rule_trust::RuleTrust;
use crate::{with_rules, ExecutionResult};

pub use persistence::{
    batch_evaluate_columns, cleanup_old_insight_snapshots, get_column_insight_history,
    get_insight_storage_stats, get_insight_version_detail, profile_column_from_table,
    save_column_insight_snapshot,
};

/// 错误 → 面板可展示的语义。
///
/// 面板的「错误态」只需要两件事：给人看的文案、要不要给「重试」。因此不直接展示
/// [`CoreError`] 的 `Display`（它带 `[code]` 内部错误码前缀）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsightErrorInfo {
    pub message: String,
    /// 值得重试吗？瞬时的（并发配额 / 连接抖动）为真；结果集没了重试也没用，为假。
    pub retryable: bool,
}

/// 洞察服务门面。
pub struct InsightService;

impl InsightService {
    // ==================== 列画像 ====================

    /// 列画像全量结果（统计 + 样本 + 数值列直方图）。`project_root` 决定使用哪一层规则。
    pub fn get_column_insight_full(
        project_root: Option<&Path>,
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnInsightFull, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::get_column_insight_full(registry, temp_table, column_name)
        })
    }

    /// 列基础统计（不含样本与直方图），更轻。
    pub fn get_column_insights(
        project_root: Option<&Path>,
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnStats, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::get_column_insights(registry, temp_table, column_name)
        })
    }

    /// 列画像 → **面板视图模型**：一次调用拿齐「取当前项目的规则集 + 算 + 映射」。
    ///
    /// 阻塞：底层要抢 DuckDB 全局锁与并发配额（D12 / D13）。因此调用方负责把它放到
    /// 后台线程，算完再把结果回填面板（渲染路径零 I/O）。
    ///
    /// 直方图在 `ColumnInsightFull::histogram` 上（不在 `NumericStats` 里），
    /// 由 [`ColumnProfileView::from_domain`] 一并消费——所以面板不必分两次取数。
    pub fn profile_column_view(
        project_root: Option<&Path>,
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnProfileView, CoreError> {
        let full = Self::get_column_insight_full(project_root, temp_table, column_name)?;
        Ok(ColumnProfileView::from_domain(&full))
    }

    // ==================== 源取样（统一入口契约）====================

    /// 源目标列画像：**取样 → 分析临时表 → 画像**，返回（样本表名, 视图）。
    ///
    /// 「凡能喂给 DuckDB 的数据都能洞察」的落点：导航树 / 分析存档 / 草稿箱 /
    /// 编辑器结果集都只需给出「连接 + 只读查询 + 标签」，其余在这里完成。
    ///
    /// 返回样本表名是必需的：面板保存快照 / 开多列 / 下钻要接着用**同一份样本**。
    /// 阻塞（要跑源库查询 + DuckDB 分析），调用方负责放后台。
    pub fn profile_source_column(
        project_root: Option<&Path>,
        source: &crate::model::SampleSource,
        column_name: &str,
    ) -> Result<(String, ColumnProfileView), CoreError> {
        block_on(persistence::profile_source_column(
            project_root,
            source,
            column_name,
        ))
    }

    /// 源目标表探查：取样 → 内省，返回（样本表名, 视图）。
    pub fn profile_source_table(
        source: &crate::model::SampleSource,
        table_name: &str,
    ) -> Result<(String, TableProfileView), CoreError> {
        block_on(persistence::profile_source_table(source, table_name))
    }

    // ==================== 表探查（Phase 3.1） ====================

    /// 表探查（Tab「表」）：临时表内省 → 视图模型。
    ///
    /// **不跑规则**：表探查只看元数据与行数；逐列规则统计是「评估全表」的事
    /// （那一步很贵，必须由用户显式发起，进度也要可见）。
    ///
    /// `project_root` 目前不被使用（表探查不依赖规则集），保留形参是为了与
    /// 列画像门面保持一致的调用口径——将来表级取样规则要分项目时不用改签名。
    pub fn profile_table_view(
        project_root: Option<&Path>,
        temp_table: &str,
        table_name: &str,
    ) -> Result<TableProfileView, CoreError> {
        let _ = project_root;
        let profile = crate::insight_engine::get_temp_table_profile(temp_table)?;
        Ok(TableProfileView::from_profile(&profile, table_name))
    }

    // ==================== 多列分析（Phase 3.2 / 3.3） ====================

    /// 「多列」Tab 的数据：**真实列元数据** + `category = multi` 的规则清单。
    ///
    /// 列清单与表探查同源（同一个 `get_temp_table_profile`）：v1 的 `availableColumns`
    /// 恒空是「多列分析从未跑通」的根因，这里不再另存一份列清单。
    pub fn multi_column_view(
        project_root: Option<&Path>,
        temp_table: &str,
        table_name: &str,
    ) -> Result<MultiColumnView, CoreError> {
        let profile = crate::insight_engine::get_temp_table_profile(temp_table)?;
        let rules = Self::list_multi_rules(project_root)?;
        Ok(MultiColumnView::from_profile(&profile, table_name, rules))
    }

    /// `category = multi` 的规则 → 视图模型（不可解析的跳过：宁少不假）
    pub fn list_multi_rules(project_root: Option<&Path>) -> Result<Vec<MultiRuleView>, CoreError> {
        let raw = Self::list_insight_rules(project_root, Some("multi"))?;
        Ok(raw
            .iter()
            .filter_map(MultiRuleView::from_rule_json)
            .collect())
    }

    /// 跑一条多列规则：参数拼装 → 执行（已持连接）→ 结果与门控转视图模型。
    ///
    /// 阻塞（抢 DuckDB 全局锁与并发配额），调用方负责放后台。
    pub fn run_multi_rule(
        project_root: Option<&Path>,
        temp_table: &str,
        rule_id: &str,
        columns: &[String],
    ) -> Result<(MultiResultView, Vec<QualityNote>), CoreError> {
        let parameters = {
            let registry = crate::registry_for(project_root);
            let guard = registry.read().map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "规则注册表锁定失败：{}",
                    e
                )))
            })?;
            let rule = guard.get(rule_id).ok_or_else(|| {
                CoreError::common(CommonError::General(format!("规则 '{}' 不存在", rule_id)))
            })?;
            rule_params(&rule.query.parameters, temp_table, columns)?
        };

        let duckdb = crate::insight_engine::get_or_create_duckdb()?;
        let conn = duckdb.lock().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
        })?;
        let result = with_rules(project_root, |registry| {
            crate::insight_engine::execute_insight_rule(registry, rule_id, &conn, &parameters)
        })?;

        let view = MultiResultView::from_execution(&result);
        let notes = crate::model::quality_notes(result.quality.as_ref());
        Ok((view, notes))
    }

    // ==================== 结构洞察（Phase 4） ====================

    /// Schema 健康报告 → 面板视图模型（Phase 4 的门面：补上唯一缺的命令等价物）。
    ///
    /// 分析器本身是异步的（要走源库内省），而服务门面是**同步**口径
    /// （与 `profile_column_view` 一致）：这里用阻塞桥接上，调用方负责放后台。
    ///
    /// `database` 由调用方给出：表清单要按 `table_catalog` 过滤，
    /// 否则同名表会跨库混在一起。
    pub fn schema_report_view(
        conn_id: String,
        database: &str,
        schema: &str,
    ) -> Result<SchemaReportView, CoreError> {
        let report = block_on(crate::schema_analyzer::SchemaAnalyzer::analyze(
            conn_id, database, schema,
        ))?;
        Ok(SchemaReportView::from_report(&report))
    }

    // ==================== 快照历史（Phase 5.1 / 5.2） ====================

    /// 保存当前列的一次快照（正文 + 元数据双写），返回刷新后的历史。
    ///
    /// **重取一次领域画像再存**：面板手里只有视图模型，而快照正文存的是领域结果
    /// （`ColumnInsightFull`）——把视图模型反向拼回去是不可能的，也不应该。
    ///
    /// `entity_source` 记录「这份快照是哪来的」：源目标传来源描述（如
    /// `analytics.orders`），临时表目标只能说临时表名（如实写，不编）。
    pub fn save_column_snapshot(
        project_root: Option<&Path>,
        temp_table: &str,
        column: &str,
        source_label: Option<&str>,
    ) -> Result<HistoryView, CoreError> {
        let root = project_root.ok_or_else(no_project)?;
        let full = Self::get_column_insight_full(Some(root), temp_table, column)?;
        let stores = block_on(crate::store::ProjectInsightStores::open(root))?;
        let entity_source = match source_label {
            Some(label) => format!("{label} · {column}"),
            None => format!("temp_table={temp_table}"),
        };
        block_on(stores.save_column_snapshot(
            &full,
            Some(&entity_source),
            Some(full.stats.total_count as i32),
            None,
        ))?;
        // 读回历史**复用同一个库句柄**：DuckDB 在同一个进程里对同一份文件只允许一个实例，
        // 第二次 open 会报「文件已被占用」——那会表现成「快照写进去了，却提示保存失败」
        // （实测到的就是这个：正文与元数据双双落库，但调用方收到 `open` 错误）。
        read_history(&stores, column)
    }

    /// 读一列的历次快照 + 存储用量。
    pub fn column_history_view(
        project_root: Option<&Path>,
        column: &str,
    ) -> Result<HistoryView, CoreError> {
        let root = project_root.ok_or_else(no_project)?;
        let stores = block_on(crate::store::ProjectInsightStores::open(root))?;
        read_history(&stores, column)
    }

    /// 对比某一版与**最新一版**（Phase 5.2）。
    ///
    /// 方向固定为「选中版本 → 最新版本」：这个 Tab 问的是「和上次比变了什么」，
    /// 两个方向都能选只是多一个状态（还要多一套「谁是基准」的文案）。
    ///
    /// 返回值仍是**整份历史**（列表 + 对比）：对比面板与列表同属一个载荷，
    /// 面板只管「有没有对比」——这样刷新列表时不会留下指向旧「当前」的对比（D43）。
    pub fn compare_column_snapshots(
        project_root: Option<&Path>,
        column: &str,
        baseline_version: &str,
    ) -> Result<HistoryView, CoreError> {
        let root = project_root.ok_or_else(no_project)?;
        let stores = block_on(crate::store::ProjectInsightStores::open(root))?;
        let entries = history_entries(&stores, column)?;

        let baseline = entries
            .iter()
            .find(|entry| entry.version_id == baseline_version)
            .ok_or_else(|| {
                CoreError::common(CommonError::General(format!(
                    "这一版已不在历史里（找不到 {}）",
                    baseline_version.chars().take(8).collect::<String>()
                )))
            })?;
        let latest = entries.first().ok_or_else(|| {
            CoreError::common(CommonError::General("这一列还没有快照".to_string()))
        })?;
        if latest.version_id == baseline.version_id {
            return Err(CoreError::common(CommonError::General(
                "最新一版没有更新的版本可比".to_string(),
            )));
        }

        // 两侧正文都读出来比：比的是**存下来的那份结论**，不是现在重算的
        let diff = crate::model::VersionDiffView::between(
            &baseline.parse_insight()?,
            &baseline.created_at,
            &latest.parse_insight()?,
            &latest.created_at,
        )
        .with_baseline_version(&baseline.version_id);

        Ok(history_view_of(&stores, column, entries).with_diff(diff))
    }

    /// 清理 `days` 天前的快照（Phase 5.3）：正文与元数据**成对删**，返回刷新后的历史。
    ///
    /// `days` 由调用方给（接缝传 `model::SNAPSHOT_RETENTION_DAYS`）：服务不持策略，
    /// 但界面上写的天数与实际切档必须是同一个值（那个常量就是这层约定的落脚点）。
    ///
    /// 两侧条数都带回去：对不上是半写的信号（D16），不能只报一个数就说「清理完成」。
    pub fn cleanup_old_snapshots(
        project_root: Option<&Path>,
        column: &str,
        days: i64,
    ) -> Result<HistoryView, CoreError> {
        let root = project_root.ok_or_else(no_project)?;
        let stores = block_on(crate::store::ProjectInsightStores::open(root))?;
        let (body_removed, meta_removed) = block_on(cleanup_old_insight_snapshots(
            days as i32,
            &stores.storage,
            &stores.meta,
        ))?;
        let outcome = crate::model::CleanupOutcome {
            days,
            body_removed: body_removed.max(0) as usize,
            meta_removed,
        };
        // 读回列表同样**复用同一个库句柄**（D41：同进程对同一项目库不得重叠 open）
        Ok(read_history(&stores, column)?.with_cleanup(outcome))
    }

    /// 错误 → 面板可展示的语义（文案 + 是否可重试）。
    ///
    /// 识别方式是**按消息内容**匹配：引擎侧的 DuckDB 错误还没有结构化分类，
    /// 这是权宜。一旦 engine 给出 typed error（如 `TableNotFound` / `ConnectionLost`），
    /// 这里应改为匹配类型而不是字符串。
    pub fn describe_error(err: &CoreError) -> InsightErrorInfo {
        let raw = strip_error_code(&err.to_string());
        let lower = raw.to_lowercase();

        // 并发配额（D12）：瞬时，值得重试。文案直接用引擎侧那一份，
        // 避免「引擎一套、面板又一套」的两份文案。
        if raw == crate::insight_engine::ERR_TOO_MANY_CONCURRENT {
            return InsightErrorInfo {
                message: raw,
                retryable: true,
            };
        }

        // 结果集没了（临时表被重建 / 会话结束 / 超时清理）：重试无意义——
        // 用户必须重新执行那条查询。
        if hits_any(
            &lower,
            &[
                "does not exist",
                "no such table",
                "catalog error",
                "不存在",
                "binder error",
            ],
        ) {
            return InsightErrorInfo {
                message: "结果集已失效或已过期，请重新执行查询".into(),
                retryable: false,
            };
        }

        // 连接抖动：多为瞬时，值得重试。
        if hits_any(
            &lower,
            &[
                "connection",
                "socket",
                "network",
                "timeout",
                "timed out",
                "连接",
                "超时",
            ],
        ) {
            return InsightErrorInfo {
                message: "连接不可用，请检查数据源后重试".into(),
                retryable: true,
            };
        }

        // 兵底：原文照给（往往直接可定位），但不主动承诺重试。
        InsightErrorInfo {
            message: raw,
            retryable: false,
        }
    }

    // ==================== 质量评分 ====================

    pub fn compute_column_quality(stats: &ColumnInsightFull) -> QualityScore {
        crate::quality_scorer::compute_column_quality(stats)
    }

    pub fn compute_table_quality(table_name: &str, stats_list: &[ColumnInsightFull]) -> TableQuality {
        crate::quality_scorer::compute_table_quality(table_name, stats_list)
    }

    // ==================== 规则 ====================

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

// ==================== 规则索引库（项目层 / 全局层） ====================

/// 两层索引库的开启与读写：界面与接缝都不必知道池、路径与开启方式的差异
/// （项目库要现开现用，全局库是进程单例）。
///
/// 两层都允许缺失：拿不到系统目录就只剩项目层，未打开项目就只剩全局层——
/// 内置层不依赖任何库，照常可用。
struct IndexStores {
    project: Option<RuleIndexStore>,
    global: Option<RuleIndexStore>,
}

impl IndexStores {
    fn open(project_root: Option<&Path>) -> Result<Self, CoreError> {
        let project = match project_root {
            Some(root) => Some(RuleIndexStore::project(
                block_on(ProjectDatabaseManager::open(root, 2))?.sqlite_pool(),
            )),
            None => None,
        };
        let global = engine::migration::get_global_db_manager()
            .map(|manager| RuleIndexStore::global(manager.sqlite_pool()));
        Ok(Self { project, global })
    }

    fn store(&self, scope: RuleScope) -> Option<&RuleIndexStore> {
        match scope {
            RuleScope::Project => self.project.as_ref(),
            RuleScope::Global => self.global.as_ref(),
            RuleScope::Builtin => None,
        }
    }

    /// 写入落在哪一层的索引：规则所在层；内置规则写**抑制记录**到项目层，
    /// 无项目时才落全局层（两层都能被 `plan_index` 原样保留）。
    fn write_scope(&self, scope: RuleScope) -> RuleScope {
        match scope {
            RuleScope::Builtin if self.project.is_some() => RuleScope::Project,
            RuleScope::Builtin => RuleScope::Global,
            other => other,
        }
    }

    /// 磁盘 → 索引：扫描、合并（保留用户启停）、写库、推启停集合。
    /// 完成后清掉陈旧标记——规则管理视图是本标记的**唯一消费点**（D23）。
    fn sync(&self, project_root: Option<&Path>) -> Result<SyncOutcome, CoreError> {
        let outcome = block_on(sync_project_rules(
            project_root,
            self.global.as_ref(),
            self.project.as_ref(),
            &crate::builtin_rule_ids(),
        ))?;
        watcher::clear_index_stale();
        Ok(outcome)
    }

    /// 两层索引行 + 内置层元信息 → 对话框视图模型。
    fn snapshot(&self, project_root: Option<&Path>) -> Result<RulesData, CoreError> {
        let project_rows = match &self.project {
            Some(store) => block_on(store.load())?,
            None => Vec::new(),
        };
        let global_rows = match &self.global {
            Some(store) => block_on(store.load())?,
            None => Vec::new(),
        };
        // 内置层正文内嵌在二进制里：界面只需要展示字段，不解析正文（避开重复的 TOML 解析）
        let builtin: Vec<RuleMeta> = crate::builtin_registry()
            .all_rules()
            .iter()
            .map(|rule| rule.meta.clone())
            .collect();

        // 信任门（Q1 ③）：未信任时项目层**没装配**，所以横幅的数据不能从索引行推——
        // 它来自装配侧（`registry.pending_project()`）与信任记录本身。
        let (pending, trust_declined) = match project_root {
            Some(root) => {
                let pending = crate::registry_for(Some(root))
                    .read()
                    .ok()
                    .and_then(|registry| registry.pending_project().cloned());
                let declined = matches!(
                    crate::project_rule_trust(root),
                    rule_trust::RuleTrust::Declined
                );
                (pending, declined)
            }
            None => (None, false),
        };

        Ok(build_rules_data(RuleDataInput {
            project_dir: rule_dir(project_root, RuleScope::Project),
            global_dir: rule_dir(project_root, RuleScope::Global),
            project_rows,
            global_rows,
            builtin,
            pending,
            trust_declined,
        }))
    }
}

/// 同步口径的阻塞桥。
///
/// 服务门面是**同步**的（与 `profile_column_view` 一致：调用方负责放后台），
/// 而库访问是异步的；这里只做这一件转换。
///
/// **runtime 是进程级单例，不是每次调用新建**（K19 的修根）：门面驱动的是**宿主建的
/// 连接池**，而池里连接的 I/O 任务与创建它的 runtime 绑定。临时 runtime 一 drop，
/// 那条连接的后台任务可能一并被杀，而池还以为它是好的——下一条语句在「僵尸连接」
/// 上一直等到超时才重建。真机实测（PostgreSQL，交替对照）：每次新建时门面路径下一条
/// 语句稳定等 **30.2s**，直调分析器（宿主 runtime）只要 ~110ms。改成进程级单例后两边
/// 都是一百毫秒级。
///
/// 用多线程 runtime 而不是 current_thread：池的空闲回收 / 连接保活是后台任务，
/// 只在 `block_on` 期间推进是不够的。两条 worker 线程常驻，代价可接受。
fn block_on<T>(
    future: impl std::future::Future<Output = Result<T, CoreError>>,
) -> Result<T, CoreError> {
    static RUNTIME: std::sync::OnceLock<Result<tokio::runtime::Runtime, String>> =
        std::sync::OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rds-insight-block-on")
            .enable_all()
            .build()
            .map_err(|e| format!("创建异步运行时失败：{e}"))
    });
    match runtime {
        Ok(runtime) => runtime.block_on(future),
        Err(message) => Err(CoreError::common(CommonError::General(message.clone()))),
    }
}

/// 需要项目目录的操作（快照落 `{项目}/.RSmeta/project.db` + `analysis.duckdb`）
fn no_project() -> CoreError {
    CoreError::common(CommonError::General(
        "该操作需要先打开项目（快照存在项目目录下）".to_string(),
    ))
}

/// 从**已打开**的库句柄读历史 + 存储用量（保存后的读回与单纯读取共用一处口径）。
///
/// 开库归调用方：本模块一律**开一次、用完即弃**（与宿主现有做法一致：
/// 资源目录 / Mock 历史 / 连接列表都是开→用→放），但**同一次操作里不得重叠开两次**
/// ——DuckDB 在同一进程里对同一份文件只允许一个实例，重叠 open 必失败。
fn read_history(
    stores: &crate::store::ProjectInsightStores,
    column: &str,
) -> Result<HistoryView, CoreError> {
    let entries = history_entries(stores, column)?;
    Ok(history_view_of(stores, column, entries))
}

/// 读历次快照（顺序由存储层的 `ORDER BY` 定：最新在前）
fn history_entries(
    stores: &crate::store::ProjectInsightStores,
    column: &str,
) -> Result<Vec<crate::store::InsightVersionEntry>, CoreError> {
    block_on(
        stores
            .storage
            .columns
            .get_history(column, Some(HISTORY_PAGE_SIZE)),
    )
}

/// 历史条目 + 存储用量 → 视图模型。
///
/// 存储统计失败不影响历史可用：它是底部的参考信息，不是结论。
/// 拿不到就整行不显示（`stats: None`），也不编一个 0 出来。
fn history_view_of(
    stores: &crate::store::ProjectInsightStores,
    column: &str,
    entries: Vec<crate::store::InsightVersionEntry>,
) -> HistoryView {
    let stats = block_on(stores.storage.columns.get_storage_stats()).ok();
    HistoryView::from_entries(column, &entries, stats.as_ref())
}

/// 规则参数 → 实际取值。
///
/// 两条约定（与 `insight-user-guide.md` §4.5 对外口径一致）：
/// - `table` 恒为当前临时表名，不占用户的列位；
/// - 其余参数（`col1` / `col2` …）按**选择顺序**对应选中的列——所以顺序就是语义，
///   列数不匹配时直接报错而不是少传一个参数（少传会让 SQL 悄悄变成另一个查询）。
pub fn rule_params(
    parameters: &[String],
    temp_table: &str,
    columns: &[String],
) -> Result<HashMap<String, String>, CoreError> {
    let column_params: Vec<&String> = parameters.iter().filter(|p| p.as_str() != "table").collect();
    if column_params.len() != columns.len() {
        return Err(CoreError::common(CommonError::General(format!(
            "该规则需要 {} 列，实际选了 {} 列",
            column_params.len(),
            columns.len()
        ))));
    }
    let mut params: HashMap<String, String> = HashMap::new();
    params.insert("table".to_string(), temp_table.to_string());
    for (name, column) in column_params.iter().zip(columns.iter()) {
        params.insert((*name).clone(), column.clone());
    }
    Ok(params)
}

/// 去掉 `Display` 的 `[code] ` 前缀（`CoreError` 的 Display 形如 `[C001] 文案`）。
/// 面板展示的是给人的文案，不展示内部错误码。
fn strip_error_code(text: &str) -> String {
    if let (Some(0), Some(end)) = (text.find('['), text.find(']')) {
        if end + 1 < text.len() {
            return text[end + 1..].trim_start().to_string();
        }
    }
    text.to_string()
}

fn hits_any(haystack_lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack_lower.contains(n))
}

// ==================== 引擎结果取数 ====================

/// 引擎执行结果 → 「列名 + 行」（**进程内**直读，不经 JSON 契约序列化）。
///
/// 为什么不走 `serde_json::to_value(&QueryResult)`：那是**契约层**的扁平序列化
/// （只输出 `columns` / `rows` / …，Arrow `batches` 不参与），而 native 驱动
/// （MySQL / PostgreSQL / SQLite / DuckDB）只填 `batches`、`rows` 字段恒为空。
/// 照 v1 的 `json["batches"][0]["rows"]` 读，拿到的永远是「有列名、零行」——
/// 洞察取样本时会静默变成空表（真机踩过：结构洞察恒报 0 张表）。
/// 进程内直读 `to_rows()` 才是权威。
pub fn result_columns_and_rows(
    result: &shared::models::QueryResult,
) -> (Vec<String>, Vec<Vec<serde_json::Value>>) {
    let columns = result.columns.clone();
    let rows = result
        .to_rows()
        .into_iter()
        .map(|row| row.into_iter().map(value_to_json).collect())
        .collect();
    (columns, rows)
}

/// 引擎的 [`shared::models::Value`] → 建 DuckDB 临时表用的 JSON 值。
///
/// `Bytes` 走有损 UTF-8：临时表要的是「能算的样本」，二进制列在洞察里只能当文本看
/// （真二进制统计本就没有意义），比整列变 `NULL` 有用。
fn value_to_json(value: shared::models::Value) -> serde_json::Value {
    use shared::models::Value;
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(v) => serde_json::Value::Bool(v),
        Value::Int(v) => serde_json::Value::Number(v.into()),
        Value::Float(v) => serde_json::Number::from_f64(v)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Text(v) => serde_json::Value::String(v),
        Value::Bytes(v) => serde_json::Value::String(String::from_utf8_lossy(&v).to_string()),
    }
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::{result_columns_and_rows, rule_params, strip_error_code, InsightService};
    use crate::insight_engine::ERR_TOO_MANY_CONCURRENT;
    use crate::service::rule_trust::RuleTrust;
    use shared::error::{CommonError, ConnectionError, CoreError, DatabaseError};
    use shared::models::QueryResult;

    /// 回归（真机踩过）：行在 Arrow `batches` 里（native 驱动就是这种形态）。
    ///
    /// 曾经走 `serde_json::to_value(&QueryResult)` 再读 `["batches"]` —— 契约序列化
    /// 不含 `batches`，于是永远「有列名、零行」，结构洞察恒报 0 张表。
    #[test]
    fn rows_come_from_arrow_batches_without_a_json_round_trip() {
        use arrow::array::{Int64Array, StringArray};
        use arrow::datatypes::{DataType, Field, Schema};
        use arrow::record_batch::RecordBatch;
        use std::sync::Arc;

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, true),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from(vec![1, 2])),
                Arc::new(StringArray::from(vec![Some("a"), None])),
            ],
        )
        .expect("构造批");
        let result = QueryResult::from_batches(vec!["id".into(), "name".into()], vec![batch]);

        let (columns, rows) = result_columns_and_rows(&result);
        assert_eq!(columns, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(rows.len(), 2, "行必须来自 batches（走 JSON 契约序列化会是 0 行）");
        assert_eq!(rows[0][0], serde_json::json!(1));
        assert_eq!(rows[0][1], serde_json::json!("a"));
        assert_eq!(rows[1][1], serde_json::Value::Null, "NULL 要原样保留");
    }

    /// 信任门需要项目根：无项目时给出可读错误，而不是去写一个不相干的记录。
    #[test]
    fn decide_rules_trust_requires_a_project() {
        let err = InsightService::decide_project_rules_trust(None, RuleTrust::Trusted)
            .expect_err("无项目时应报错");
        assert!(
            err.to_string().contains("项目"),
            "报错要说清要项目根: {err}"
        );
    }

    #[test]
    fn error_code_prefix_is_not_shown_to_users() {
        assert_eq!(strip_error_code("[C001] 出错了"), "出错了");
        assert_eq!(strip_error_code("无前缀"), "无前缀");
        // 中括号不在开头时不动它（文案里可能有引用）
        assert_eq!(strip_error_code("列 [amount] 不存在"), "列 [amount] 不存在");
        assert_eq!(strip_error_code("[C001]"), "[C001]");
    }

    #[test]
    fn concurrency_error_reuses_engine_text_and_is_retryable() {
        let err = CoreError::Common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()));
        let info = InsightService::describe_error(&err);
        assert_eq!(info.message, ERR_TOO_MANY_CONCURRENT);
        assert!(info.retryable, "并发配额是瞬时的，应给重试入口");
        assert!(
            !info.message.contains('['),
            "不得把内部错误码展示给用户：{}",
            info.message
        );
    }

    #[test]
    fn missing_result_set_is_not_retryable() {
        let err = CoreError::Database(DatabaseError::Query {
            sql: "SELECT * FROM t_result_7".into(),
            reason: "Catalog Error: Table with name t_result_7 does not exist".into(),
            position: None,
            location: None,
        });
        let info = InsightService::describe_error(&err);
        assert_eq!(info.message, "结果集已失效或已过期，请重新执行查询");
        assert!(!info.retryable, "结果集没了，重试不会变好——要重新执行查询");
    }

    #[test]
    fn connection_trouble_is_retryable() {
        let err = CoreError::Connection(ConnectionError::Refused {
            conn_id: "G_1".into(),
            reason: "connection refused".into(),
        });
        let info = InsightService::describe_error(&err);
        assert_eq!(info.message, "连接不可用，请检查数据源后重试");
        assert!(info.retryable);
    }

    #[test]
    fn unknown_error_keeps_wording_without_promising_retry() {
        let err = CoreError::Common(CommonError::General("规则 numeric-stats 执行失败".into()));
        let info = InsightService::describe_error(&err);
        assert_eq!(info.message, "规则 numeric-stats 执行失败", "原文往往自己就能定位");
        assert!(!info.retryable);
    }

    /// 参数拼装：`table` 恒为临时表名，其余按选择顺序对位
    #[test]
    fn rule_params_map_columns_in_order() {
        let params = rule_params(
            &["table".into(), "col1".into(), "col2".into()],
            "t_result_1",
            &["amount".into(), "qty".into()],
        )
        .expect("两列匹配两个参数");
        assert_eq!(params["table"], "t_result_1");
        assert_eq!(params["col1"], "amount", "顺序就是语义");
        assert_eq!(params["col2"], "qty");

        // 只有一个列参数时也一样（不依赖叫 col1 还是 col）
        let single = rule_params(&["table".into(), "col".into()], "t", &["x".into()]).unwrap();
        assert_eq!(single["col"], "x");
        assert_eq!(single.len(), 2, "不该多出参数");
    }

    /// 列数不匹配：直接报错，不少传参数——少传会让 SQL 静默变成另一个查询
    #[test]
    fn rule_params_reject_arity_mismatch() {
        let err = rule_params(
            &["table".into(), "col1".into(), "col2".into()],
            "t",
            &["amount".into()],
        )
        .expect_err("只选一列不该通过");
        assert!(err.to_string().contains("需要 2 列"), "报错要说清差多少：{err}");

        let err = rule_params(&["table".into(), "col".into()], "t", &[]).expect_err("没选列");
        assert!(err.to_string().contains("需要 1 列"), "{err}");
    }
}
