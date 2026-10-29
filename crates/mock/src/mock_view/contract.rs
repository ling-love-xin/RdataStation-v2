// ==================== 宿主契约 ====================

// 继承父模块（mock_view）的全部 use 绑定：std / gpui_kit / models / persistence / generator_catalog 等
use super::*;

/// 列（草稿）：列定义 + 智能映射元信息。
#[derive(Debug, Clone)]
pub struct MockColumnSpec {
    /// 稳定 id（列可增删，输入框与列表键用 id 而非下标）
    pub id: u64,
    /// 列定义
    pub def: ColumnDef,
    /// 智能映射置信度：`high` / `low` / `manual`
    pub confidence: String,
    /// 智能映射示例值（无参数时作为一行说明）
    pub sample_value: String,
}

/// 生成草稿：目标表名 + 列 + 选项。
#[derive(Debug, Clone)]
pub struct MockDraft {
    /// 目标表名（用户命名；落库 / 追加 / 文件名都基于它）
    pub table_name: String,
    /// 列清单
    pub columns: Vec<MockColumnSpec>,
    /// 生成选项
    pub options: MockRunOptions,
}

impl Default for MockDraft {
    fn default() -> Self {
        Self {
            table_name: "mock_data".to_string(),
            columns: Vec::new(),
            options: MockRunOptions::default(),
        }
    }
}

/// 生成选项（面板可编辑的三项）。
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MockRunOptions {
    /// 目标行数
    pub rows: u32,
    /// 随机种子（`None` = 随机；`Some` = 可复现）
    pub seed: Option<u32>,
    /// 语言 / 地区
    pub locale: Locale,
}

impl Default for MockRunOptions {
    fn default() -> Self {
        Self {
            rows: 1000,
            seed: None,
            locale: Locale::ZhCn,
        }
    }
}

impl MockRunOptions {
    /// 构造生成选项（`#[non_exhaustive]` 结构不开放字段字面量）。
    pub fn new(rows: u32, seed: Option<u32>, locale: Locale) -> Self {
        Self { rows, seed, locale }
    }
}

/// 生成结果（内存临时表）。
///
/// 带着**列定义**（`columns`）的目的：出口（落库 / 导出 / 草稿箱）只能凭结果自己完成——
/// 单表生成时它与草稿列一致，但场景模板生成的多张表与草稿**没有任何关系**，
/// 「拿草稿当出口规格」会在场景下建出列名对不上的表。
#[derive(Debug, Clone)]
pub struct MockGenInfo {
    /// 目标表名（单表生成 = 草稿里的目标表；场景模板 = 模板里的表名）
    pub table_name: String,
    /// 内存临时表名（`temp_mock_*`）
    pub temp_table_name: String,
    /// 生成时用的列定义（出口建表 / 校对目标列结构用）
    pub columns: Vec<ColumnDef>,
    /// 本次生成行数
    pub row_count: u32,
    /// 生成耗时（毫秒）
    pub elapsed_ms: u32,
    /// 预览（前若干行，已字符串化）
    pub preview: MockPreview,
}

/// 追加目标表现状（S3：追加确认对话框展示「行数 / 隐含主键 / 最大值」）。
///
/// 与 workbench 装配层的 [`TableAppendStatus`] 同构，但定义在视图侧：
/// `MockHost` 的实现（宿主）负责把库侧信息翻译成这个结构，视图只读它来渲染。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppendTargetStatus {
    /// 目标表现有行数
    pub row_count: i64,
    /// 自增（隐含主键）列名；目标表没有该列时为 `None`
    pub pk_column: Option<String>,
    /// 自增列现有最大值（表空为 `None`）
    pub max_pk: Option<i64>,
}

/// 预览数据（已字符串化，视图不依赖 Arrow）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MockPreview {
    /// 列名
    pub columns: Vec<String>,
    /// 行数据（与 `columns` 对齐）
    pub rows: Vec<Vec<String>>,
}

/// 生效中的排序：列名 + 方向。
///
/// 它是一个**描述**（不含数据）：取样存在 [`PreviewCache`] 里（排序与多要几行共用一处缓存）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSort {
    /// 排序列名
    pub column: String,
    /// 降序？（表头箭头点第二次 / 菜单里选的）
    pub descending: bool,
}

impl PreviewSort {
    /// 方向说明（预览标题与菜单用）：「按 id 降序」。
    pub fn label(&self) -> String {
        format!(
            "按 {} {}",
            self.column,
            if self.descending { "降序" } else { "升序" }
        )
    }
}

/// 面板缓存的那一份**重查取样**（排序 / 多要几行共用）。
///
/// 两者合成一处缓存，因为它们的失效条件完全相同：**取样变了**（重新生成 / 切项目）
/// 或**行数档变了**。`base` 存的是派生它的那份生成取样，`limit` 是当时的行数档——
/// 任一不等就当缓存不存在，不必在十几个 `results` 清空点手写清理。
/// （同名同为 seed 重建的极端情形里取样相同，而那份重查描述的也正是这份数据，仍然成立。）
#[derive(Debug, Clone)]
pub(crate) struct PreviewCache {
    /// 哪张表的结果（另一张表的 tab 不继承）
    pub(crate) table: String,
    /// 派生它的那份生成取样
    pub(crate) base: MockPreview,
    /// 重查时用的行数档
    pub(crate) limit: usize,
    /// 排序（`None` = 只是多要几行）
    pub(crate) sort: Option<PreviewSort>,
    /// 重查结果
    pub(crate) preview: MockPreview,
}

impl PreviewCache {
    /// 这份缓存是否还适用于该表当前的取样与当前行数档。
    pub(crate) fn applies_to(&self, table: &str, base: &MockPreview, limit: usize) -> bool {
        self.table == table && self.base == *base && self.limit == limit
    }
}


/// 可导入结构的连接（来源下拉用）。
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaSource {
    /// 连接 ID
    pub conn_id: String,
    /// 显示名（连接名）
    pub label: String,
    /// 连接记录里的默认数据库 / catalog（对话框预填）
    pub catalog: String,
    /// 连接记录里的默认 schema（对话框预填）
    pub schema: String,
}

/// 导入结构请求（某连接下的一张表）。
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaRequest {
    /// 连接 ID
    pub conn_id: String,
    /// Catalog / 数据库
    pub catalog: String,
    /// Schema
    pub schema: String,
    /// 表名
    pub table: String,
}

/// 宿主注入的能力（workbench 实现，见 `components/mock_host.rs`）。
///
/// 分三类：**后台任务**（生成 / 追加 / 三个出口：启动 + 进度 + 结果 + 取消）、
/// **来源与目录**（连接清单 / 既有表 / 导入结构）；另有四个视图交互钩子
/// （默认目录、只读判定、打开详情 tab、宿主重绘）。
///
/// 出口统一走 [`MockHost::start_job`]（不再有同步入口）：大行数落库 / 导出同样会阻塞，
/// 与生成同一套「工作线程 + 进度 + 一次性结果」机制，避免两套书写路径漂移。
pub trait MockHost: 'static {
    /// 启动后台任务（生成 / 生成后追加 / 三个出口），**立即返回**，不阻塞 UI。
    ///
    /// 已有任务进行中时必须返回 `Err`（由视图拦住重复点击）。
    /// 出口类任务用到的路径（分析库 / 项目根）由宿主在**调用前**解析好：
    /// 工作线程不碰宿主状态（`Shared` 里的 `Rc<RefCell<…>>` 也不能跨线程）。
    fn start_job(&self, draft: &MockDraft, kind: MockJobKind) -> Result<(), String>;
    /// 当前任务状态（UI 轮询；实现需为轻量读）。
    fn job_state(&self) -> MockJobState;
    /// 取走已完成任务的结果（**一次性**：取走后归位 Idle）；`None` = 仍在进行。
    fn take_job_done(&self) -> Option<Result<MockJobDone, String>>;
    /// 请求取消进行中的任务（引擎在批次边界响应，结果以 `Err` 回传）
    fn cancel_job(&self);
    /// 分析库既有表（追加目标候选）
    fn existing_tables(&self) -> Vec<String>;
    /// 追加目标表现状（S3）：行数 / 隐含主键列 / 最大值；表不存在或未打开项目 → `None`。
    ///
    /// `pk_hint` 是**本次生成的自增列名**（草稿里 `AutoIncrement` 生成器的列）——
    /// 它对应目标表的隐含主键（v2 不建主键 DDL，自增列就是语义上的主键）。
    fn append_target_status(&self, table: &str, pk_hint: Option<&str>) -> Option<AppendTargetStatus>;
    /// 可导入结构的连接
    fn schema_sources(&self) -> Vec<SchemaSource>;
    /// 读某表的列（源库结构）
    fn import_columns(&self, request: &SchemaRequest) -> Result<Vec<MockColumnSpec>, String>;
    /// 预览的「按列重排」：取临时表中**按该列排序后的前 `limit` 行**（`None` = 不排序，只多要几行）。
    ///
    /// 为什么是重查而不是就地重排：预览只装了前 N 行的取样，就地重排只会把这 N 行换个顺序
    /// （看着像「按值排过」，实际不是这一列的前 N 名）；「多要几行」也只能由库给。
    ///
    /// `Ok(None)` = 内存库连接正被别的任务占用：调用方保持现状——预览取样是随手动作，
    /// 不值得为它等一个不可取消的生成 / 出口任务。
    fn preview_sample(
        &self,
        temp_table: &str,
        order: Option<(&str, bool)>,
        limit: usize,
    ) -> Result<Option<MockPreview>, String>;
    /// 工具栏「刷新」：重读内存临时表的**当前行数**（`Ok(None)` = 内存库连接正忙，保持现状）。
    fn temp_table_row_count(&self, temp_table: &str) -> Result<Option<i64>, String>;
    /// 文件出口的默认目录（项目根 / 工作目录；空串表示由视图回退到当前目录）
    fn export_dir(&self) -> String;
    /// 只读项目？（落库与写文件据此拒绍）
    fn read_only(&self) -> bool;
    /// 当前项目根（生成历史与用户模板的落点）；未打开项目时为 `None`。
    ///
    /// 宿主只回答这一个问题：历史的读写全在 mock crate 内完成后台执行
    /// （见 `history`），存储细节不摊到宿主侧。
    fn project_root(&self) -> Option<PathBuf>;
    /// 打开中央「Mock 数据」详情 tab（字段 + 预览）
    /// 打开（或聚焦）中央详情 tab（`target` 决定是哪张表 / 草稿）。
    ///
    /// 宿主按 `target.key()` 去重：同一张表只有一个 tab，重复调用就是把它切到前台。
    fn open_detail(&self, target: DetailTarget, window: &mut Window, cx: &mut App);
    /// 宿主重绘 + 依赖视图刷新（导航树等）
    fn notify(&self, cx: &mut App);
}

