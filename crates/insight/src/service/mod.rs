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

pub(crate) use std::collections::HashMap;
pub(crate) use std::path::{Path, PathBuf};

pub(crate) use engine::persistence::ProjectDatabaseManager;
pub(crate) use shared::error::{CommonError, CoreError};

pub(crate) use crate::model::types::{ColumnInsightFull, ColumnStats, QualityScore, TableQuality};
pub(crate) use crate::model::{
    ColumnProfileView, HISTORY_PAGE_SIZE, HistoryView, MultiColumnView, MultiResultView,
    MultiRuleView, QualityNote, TableProfileView,
};
pub(crate) use crate::rule::RuleScope;
pub(crate) use crate::rule_types::RuleMeta;
pub(crate) use crate::rule_view::{RuleDataInput, RulesData, build_rules_data};
pub(crate) use crate::schema_view::SchemaReportView;
pub(crate) use crate::service::indexer::{RuleIndexStore, SyncOutcome, rule_dir, sync_project_rules};
pub(crate) use crate::service::rule_trust::RuleTrust;
pub(crate) use crate::{ExecutionResult, with_rules};

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
}


// ==================== InsightService 方法按职责域下沉（纯位移） ====================
mod sampling;
mod analyze;
mod rules;

impl InsightService {

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

    // ==================== 历史质量模型门面（兼容保留，表/列画像 UI 已不调用） ====================
    // 仅历史快照批量评估（旧快照只读兼容，新快照不重算分）使用；会话级 EDA 不出总分 / 等级。

    pub fn compute_column_quality(stats: &ColumnInsightFull) -> QualityScore {
        crate::quality_scorer::compute_column_quality(stats)
    }

    pub fn compute_table_quality(
        table_name: &str,
        stats_list: &[ColumnInsightFull],
    ) -> TableQuality {
        crate::quality_scorer::compute_table_quality(table_name, stats_list)
    }

    // ==================== 规则 ====================
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
/// 进程级单例 Tokio runtime 上的 `block_on`（K19 的修根，见上注）。
///
/// `pub(crate)` 供 `jobs.rs` 复用：源库取数（`sample_source_to_analysis_table` 等 async
/// 链）跑在 gpui `background_executor`（非 Tokio）上时，驱动层 `tokio::time` 找不到
/// reactor 会直接 panic（K21 真机暴露，PG 探活 `postgres.rs:237`）——包一层 block_on
/// 把整条 async 链放回单例 runtime。
pub(crate) fn block_on<T>(
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
    let column_params: Vec<&String> = parameters
        .iter()
        .filter(|p| p.as_str() != "table")
        .collect();
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
        .map(|row| {
            row.into_iter()
                .map(shared::models::Value::to_json)
                .collect()
        })
        .collect();
    (columns, rows)
}

// ==================== 测试 ====================
#[cfg(test)]
mod tests {
    use super::{InsightService, result_columns_and_rows, rule_params, strip_error_code};
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
        assert_eq!(
            rows.len(),
            2,
            "行必须来自 batches（走 JSON 契约序列化会是 0 行）"
        );
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
        assert_eq!(
            info.message, "规则 numeric-stats 执行失败",
            "原文往往自己就能定位"
        );
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
        assert!(
            err.to_string().contains("需要 2 列"),
            "报错要说清差多少：{err}"
        );

        let err = rule_params(&["table".into(), "col".into()], "t", &[]).expect_err("没选列");
        assert!(err.to_string().contains("需要 1 列"), "{err}");
    }

    /// T20 归档快照：画像元数据导出为 JSON，落在项目 `.RSmeta/insight-archive/`，
    /// 内容可解析且带 `insight_table_snapshot` 类型标记（资产库按它识别洞察档）。
    #[test]
    fn export_table_snapshot_writes_parseable_json_under_project_meta() -> Result<(), CoreError> {
        use crate::insight_engine::get_or_create_duckdb;

        // 建表在独立作用域：`export_table_snapshot` 内部会自己抢 DuckDB 全局锁，
        // 这里若继续持锁调用就是同一线程重入 Mutex → 死锁（测试曾卡 60s 被超时杀掉）。
        {
            let duckdb = get_or_create_duckdb()?;
            let conn = duckdb.lock().expect("DuckDB 锁不应中毒");
            conn.execute_batch(
                "CREATE OR REPLACE TEMP TABLE t_archive_snapshot_probe (
                     id INTEGER NOT NULL, amount DECIMAL(12,2), note VARCHAR
                 );
                 INSERT INTO t_archive_snapshot_probe VALUES
                     (1, 1.5, 'a'), (2, NULL, NULL), (3, 9.9, 'c');",
            )
            .expect("建归档探针表");
        }

        // test-support 构建下 `paths::data_dir()` 已隔离到进程专属临时根
        let root = paths::data_dir();
        let (path, name) =
            InsightService::export_table_snapshot(&root, "t_archive_snapshot_probe", "orders")?;
        assert_eq!(name, "orders");
        assert!(path.starts_with(root.join(".RSmeta").join("insight-archive")));
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("orders_")
        );
        let text = std::fs::read_to_string(&path).expect("快照文件可读");
        let json: serde_json::Value = serde_json::from_str(&text).expect("快照是合法 JSON");
        assert_eq!(json["kind"], "insight_table_snapshot");
        assert_eq!(json["table"], "orders");
        assert_eq!(json["row_count"], 3);
        assert_eq!(json["column_count"], 3);
        assert_eq!(json["columns"][0]["name"], "id");
        assert_eq!(json["columns"][1]["nullable"], serde_json::json!(true));
        Ok(())
    }
}