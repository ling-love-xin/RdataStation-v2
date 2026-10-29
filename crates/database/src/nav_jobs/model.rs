//! 后台任务的**数据模型**：任务参数 / 结果 / 常量 / 小工具。
//! 无状态、无 I/O；调度与执行见 [`worker`]，搜索命中模型见 [`hits`]。

use crate::model::NavPath;
use property::ObjectProperties;

pub struct ColumnTarget {
    pub catalog: String,
    pub schema: String,
    pub table: String,
}

/// 导航子节点加载结果（回传主线程）。
pub struct LoadResult {
    pub key: String,
    pub conn_id: String,
    pub project_root: Option<String>,
    pub path: NavPath,
    /// 本块结果的起始下标（0 = 首屏；>0 = 「加载更多」追加页）。
    pub offset: usize,
    /// 这是一次「定位」的结果：值为目标在**全量**里的绝对位次。
    ///
    /// 与「加载更多」的区别必须靠它区分（两者 `offset` 都 > 0）：
    /// 加载更多是**追加**语义（并到已有行后面），定位是**换窗**语义
    /// （当前只展示这一窗，界顶那行说清「已定位到第 N 条」并可回到开头）。
    pub jumped_to: Option<usize>,
    /// S2-② 迟到 stale guard：**执行时**的连接配置指纹快照（`None` = 连接未注册 /
    /// 不可共享，跳过指纹比对）。回填前与当前指纹比对：配置已变更 → 丢弃这次写回。
    pub conn_fingerprint: Option<String>,
    pub result: Result<crate::navigator_service::NavPage, String>,
}

/// 属性加载结果（回传主线程）。
pub struct PropsResult {
    pub key: String,
    pub result: Result<ObjectProperties, String>,
}

/// 生成 SQL 结果（回传主线程）。
pub struct SqlGenResult {
    pub key: String,
    pub result: Result<String, String>,
}

/// 连接测试结果（回传主线程）。
pub struct TestConnResult {
    pub conn_id: String,
    pub name: String,
    pub result: Result<String, String>,
}

/// S3-④ 行估计结果（回传主线程）。
///
/// `estimates` = `(表名, 估计行数)` 列表（`None` 值 = 该表无可用统计 / 驱动没给）；
/// **空列表** = 整批失败 / 超时 / 驱动不支持——视图侧据此收掉「…」占位
/// （不显示 0、不报红字，见 B 档文档 §1.4.4）。
pub struct RowEstimateResult {
    pub conn_id: String,
    pub catalog: String,
    pub schema: String,
    /// 发起批次序号（视图侧 `estimate_seq`）：回填时小于当前值 = 过期批次（被更新 /
    /// 刷新顶掉），丢弃不写回。
    pub seq: u64,
    pub estimates: Vec<(String, Option<u64>)>,
    /// D12：表数据段大小（字节）回填结果，与 `estimates` 同批次、同表名匹配口径；
    /// 驱动不支持 / 本次未给时为空或 `None`，徽标安静消失。
    pub sizes: Vec<(String, Option<u64>)>,
}

/// 解析「表名 + 数值」两列结果为 `(名, 数值)` 对：首列须为 `Text`；第二列 `Int` /
/// `Float` / `Text` 三型兼容（`reltuples` / `TABLE_ROWS` / `DATA_LENGTH` /
/// `pg_total_relation_size` 口径各异，联邦回传也可能给文本）；`NULL` / 非数值解析失败
/// → `None`（徽标安静消失，不显 0）。行估计与表大小共用本解析口径。
pub(super) fn parse_name_value_pairs(rows: Vec<Vec<shared::models::Value>>) -> Vec<(String, Option<u64>)> {
    rows.into_iter()
        .filter_map(|row| {
            let name = match row.first() {
                Some(shared::models::Value::Text(n)) => n.clone(),
                _ => return None,
            };
            let value = match row.get(1) {
                Some(shared::models::Value::Int(i)) => Some(*i as u64),
                Some(shared::models::Value::Float(f)) => Some(*f as u64),
                Some(shared::models::Value::Text(t)) => t.parse::<u64>().ok(),
                _ => None,
            };
            Some((name, value))
        })
        .collect()
}

/// 导航建连结果（回传主线程）。
///
/// 由宿主（`workbench::services::nav_runtime`）在**后台线程**完成建连后提交：
/// 不可达主机（mysql / PG 等）的建连含超时重试，绝不能在 UI 主线程同步等待；
/// 视图侧经 [`drain_connect_results`] 轮询取走并回填状态（成功 → 预热；失败 → 提示 + 回滚）。
pub struct ConnectResult {
    pub conn_id: String,
    pub ok: bool,
    pub message: String,
}

/// 单个类别文件夹一次最多预取的表数量（避免大 schema 触发长时间后台 IO）。
pub const PREFETCH_BATCH: usize = 20;

/// S3-④ 行估计触发表数下限：表名单 ≥ 该值才排估计任务（小 schema 不值得一次
/// 目录级统计查询；对齐 B 档文档 §1.4.2「阈值 20 张」）。
pub const ROW_ESTIMATE_MIN_TABLES: usize = 20;

/// 一页导航对象数。
///
/// **为什么直接用 UI 的首批渲染常量**：分页的两侧必须同尺寸——取回一页、渲染窗口
/// 就长一页；若各用各的常量，要么取回 200 条只能显示一半，要么“加载更多”永远按不完。
pub const PAGE_SIZE: usize = workbench_shell::ui::NAV_FOLDER_PAGE_SIZE;

/// 单个导航任务的**内省超时**。
///
/// 为什么必须有：worker 是**单线程串行**，一条永不返回的内省（服务端锁等待 / 半开的 TCP /
/// 被长事务挡住的 `pg_attribute`）会让后面所有展开排队等它 —— 界面看着就是「卡住」，
/// 而 worker 其实活着（与 panic 那一路不同）。超时只结束**等待**：sqlx 的查询无法取消，
/// 那条连接要等服务端返回才回到池里，但队列能继续走、界面能拿到一条可读的错误。
///
/// 取值：与 sqlx 池默认的 `acquire_timeout`（30 s）同量级。正常内省都在毫秒级
/// （真机：PG 61 列 13-48 ms、MySQL 51 列 43 ms），只有真出事才会撞上。
pub const JOB_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 超时错误的文案（界面直接显示这一句）。
pub(super) fn timeout_error(what: &str) -> String {
    format!(
        "{what}超时（{} 秒未返回）：可以重试；若反复超时，检查服务端是否有长事务 / 锁等待",
        JOB_TIMEOUT.as_secs()
    )
}
