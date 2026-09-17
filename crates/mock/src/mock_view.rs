//! Mock 数据生成视图（M7 自持视图）。
//!
//! # 语义（对齐 v1 主路径）
//!
//! **造新数据**：用户命名目标表（`table_name`）+ 组织列定义（导入源库结构 / 手工加列 / 改列名与类型）
//! → 生成到**内存临时表** `temp_mock_*` → 再由**显式出口**落地：
//!
//! | 出口 | 语义 |
//! | --- | --- |
//! | 查看详情 | 中央「Mock 数据」tab：字段清单（可编辑）+ 预览表格 |
//! | 持久化为项目分析库表 | 在**项目**分析库（`{项目}/.RSmeta/analytics.duckdb`）**新建**表（已存在则报错，引导改用「追加」） |
//! | 追加到既有表 | **显式**选择既有表；主键自增起点接续表内行数 |
//! | 保存到草稿箱 | `{项目}/mock/mock_*.{ext}`（时间戳命名，只读项目禁写） |
//! | 另存为 | CSV / Parquet / Xlsx / SQL INSERT 文件（调用方在系统对话框选路径） |
//!
//! **生成不写库**：`generate` 只产临时表 + 预览（内存 DuckDB），一切落库与落盘都由出口按钮触发。
//! 这是与 v1 语义对齐的关键——「生成」不是「写入」。
//!
//! # 排版（方案①）
//!
//! - **右 Dock（17.5rem，[`MockPanel`]）**：目标表名 / 行数·种子·语言 / 列来源（导入结构 · 手工加列）/
//!   生成 / 出口按钮组 / 结果与错误 / **生成历史**（重放配置、删除记录）；
//! - **中央 tab（[`MockDetailView`]）**：字段清单（编辑走对话框）+ 预览表格。
//!
//! 右 Dock 起步宽 17.5rem 且不可拖拽调宽（`ui::RIGHT_DOCK_WIDTH`），字段表与预览表格放不下，
//! 故按「配置与出口在右、字段与预览在中」切分；两处状态同源（详情视图持有面板实体，单一权威）。
//!
//! # 归属与边界
//!
//! 视图随 mock 能力同 crate（与 `project::ui` / `settings_view` 同例）；本 crate 不依赖 workbench，
//! 宿主能力经 [`MockHost`] 注入（workbench 侧桥接 `crates/workbench/src/components/mock_host.rs`）。
//! 渲染期零 I/O：列来源、既有表、生成、出口全部在事件路径执行。
//!
//! 对话框走 `window.open_dialog`（语义层），builder 每帧重建：**只读**面板状态，
//! 一切写入都在按钮 / 菜单回调（事件路径）里完成。三个对话框：导入结构 / 列编辑 / **生成器搜索**
//! （最后一个用 `List` + `ListState`，搜索框与虚拟化都是组件能力，不手搓）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::*;

use crate::MockEngine;
use crate::generator_catalog::{self, GeneratorCategory, GeneratorSpec, ParamField, ParamKind};
use crate::history::{self, HistoryAction, RunRecord};
use crate::models::{
    ColumnDataType, ColumnDef, ColumnDependency, GeneratorConfig, Locale, MockExportFormat,
    ReferenceDomain, ScenarioTemplate, TemplateTable,
};
use crate::persistence::{
    MockGenerationDetail, MockGenerationTask, MockTemplateColumn, MockUserTemplate,
};
use crate::schema_map::ColumnMapper;

// ==================== 宿主契约 ====================

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

/// 预览数据（已字符串化，视图不依赖 Arrow）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MockPreview {
    /// 列名
    pub columns: Vec<String>,
    /// 行数据（与 `columns` 对齐）
    pub rows: Vec<Vec<String>>,
}

/// 后台任务种类（决定任务干什么、收尾时怎么归置结果）。
///
/// 分两组：**生成类**（`Generate` / `Scenario` / `AppendTo`）自己产出结果；
/// **出口类**（`Persist` / `Export` / `Scratchpad`）消费上一次生成的结果，
/// 因此把 `MockGenInfo` 带在身上——工作线程靠它找到内存临时表、拿到目标表名与列定义，
/// 而视图侧不能把「最近一次结果」另存一份隐式状态（两处状态必然漂移）。
///
/// 出口类**不读草稿**：场景模板产出的多张表与草稿无关，出口只认结果自己（见 [`MockGenInfo`]）。
#[derive(Debug, Clone)]
pub enum MockJobKind {
    /// 只生成到内存临时表（**不写库**）
    Generate,
    /// 生成后追加到既有分析表（自增起点按表内行数接续）
    AppendTo(String),
    /// 场景模板：按工作副本一次生成多张临时表（**不写库**；逐表进度）
    ///
    /// 带的是**编辑过的模板**（关系挂在列的 `dependency` 上），不是模板 id：
    /// 用户改完关系直接生成，再去引擎按 id 重取会丢掉编辑。
    Scenario(Box<ScenarioTemplate>),
    /// 出口：把多张已生成结果**依次**落成分析库新表（每张仍走「新建」语义：同名报错不覆盖）
    PersistAll(Vec<MockGenInfo>),
    /// 出口：把已生成结果持久化为分析库**新表**（同名已存在 → `Err`）
    Persist(MockGenInfo),
    /// 出口：把已生成结果导出为文件（路径由调用方在系统对话框里选好）
    Export {
        /// 已生成结果
        info: MockGenInfo,
        /// 文件格式
        format: MockExportFormat,
        /// 目标路径
        path: String,
    },
    /// 出口：把已生成结果保存到草稿箱（`{项目}/mock/`）
    Scratchpad {
        /// 已生成结果
        info: MockGenInfo,
        /// 文件格式
        format: MockExportFormat,
    },
}

impl MockJobKind {
    /// 是否包含**生成阶段**。
    ///
    /// 两个后果：只有生成阶段能取消（引擎按批响应）；只有生成类收尾时要作废旧预览
    /// （它把临时表重建了，而出口类只读不改，临时表与预览仍彼此一致）。
    pub fn generates(&self) -> bool {
        matches!(self, Self::Generate | Self::AppendTo(_) | Self::Scenario(_))
    }

    /// 进度量纲是否按「张表」计（而非批次 / 行）。
    ///
    /// 场景生成与批量落库都是一次处理多张表，`rows_total` 为 0，面板据此把
    /// `batches_done / batches_total` 读作表计数。
    pub fn by_table(&self) -> bool {
        matches!(self, Self::Scenario(_) | Self::PersistAll(_))
    }

    /// 是否**以当前草稿为输入**（目标表 / 列 / 行数）。
    ///
    /// 只有 `Generate` 与 `AppendTo` 是：场景模板自带表与列（草稿不参与也不被改写），
    /// 三个出口只看已生成的结果（见 [`MockGenInfo`])。
    /// 面板据此决定要不要先校验草稿（表名 / 行数输入 / 至少一列）。
    pub fn uses_draft(&self) -> bool {
        matches!(self, Self::Generate | Self::AppendTo(_))
    }

    /// 任务开始时的阶段（进度条形态与文案据此切换；进行中时以宿主上报的阶段为准）。
    pub fn phase(&self) -> MockJobPhase {
        match self {
            Self::Generate | Self::AppendTo(_) | Self::Scenario(_) => MockJobPhase::Generating,
            Self::Persist(_) | Self::PersistAll(_) => MockJobPhase::Writing,
            Self::Export { .. } | Self::Scratchpad { .. } => MockJobPhase::Exporting,
        }
    }

    /// 本次任务处理的行数（文案用）：生成类按草稿设定，出口类按已生成结果。
    ///
    /// 场景模板的总行数写在模板里（草稿不参与），因此这里给 0——面板对场景任务
    /// 按「张表」报进度（见 `render_job_row`），不拿行数当量纲。
    pub fn rows_total(&self, draft: &MockDraft) -> u32 {
        match self {
            Self::Generate | Self::AppendTo(_) => draft.options.rows,
            Self::Scenario(_) | Self::PersistAll(_) => 0,
            Self::Persist(info) | Self::Export { info, .. } | Self::Scratchpad { info, .. } => {
                info.row_count
            }
        }
    }

    /// 进行中的一行文案（面板结果区）。
    pub fn running_label(&self) -> String {
        match self {
            Self::Generate => "生成中…".to_string(),
            Self::AppendTo(table) => format!("生成并追加到 {table} 中…"),
            Self::Scenario(template) => format!("按场景模板「{}」生成中…", template.name),
            Self::PersistAll(infos) => format!("落库 {} 张表中…", infos.len()),
            Self::Persist(info) => format!(
                "写入项目分析库中…（{} 行）",
                with_thousands(info.row_count as u64)
            ),
            Self::Export { path, .. } => format!("导出中…（{path}）"),
            Self::Scratchpad { .. } => "保存到草稿箱中…".to_string(),
        }
    }
}

/// 后台任务完成结果。
#[derive(Debug, Clone)]
pub enum MockJobDone {
    /// 生成完成
    Generated(MockGenInfo),
    /// 场景生成完成（多张临时表；顺序即模板里的表序）
    ScenarioGenerated {
        /// 模板名（结果区文案与来源标注）
        template_name: String,
        /// 逐表结果（含预览）
        tables: Vec<MockGenInfo>,
    },
    /// 批量落库完成：成功的表（表名 + 表内行数）与失败的表（表名 + 可读原因）
    ///
    /// 不用「全成功或全失败」：已落的表不回滚（回滚别人的数据比留下已落的多张更危险）。
    PersistedAll {
        /// 落库成功的表
        landed: Vec<(String, i64)>,
        /// 落库失败的表（同名已存在 / 写失败）
        failed: Vec<(String, String)>,
    },
    /// 生成并追加完成（表名 + 表内总行数）
    Appended {
        /// 目标表
        table: String,
        /// 追加后表内总行数
        total_rows: i64,
    },
    /// 持久化完成（新建表名 + 表内行数）
    Persisted {
        /// 新建的表名
        table: String,
        /// 新表行数
        rows: i64,
    },
    /// 导出 / 草稿箱完成（可读文案由装配层给，含落地路径）
    Exported {
        /// 结果文案
        message: String,
    },
}

/// 后台任务阶段。
///
/// 生成阶段有批次粒度回调；写入与落盘跑在 DuckDB / 文件系统内部，
/// 拿不到中间进度，所以进度条改走不定量形态（`Progress::loading`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MockJobPhase {
    /// 生成行数据
    #[default]
    Generating,
    /// 写入分析库
    Writing,
    /// 写出文件（导出 / 草稿箱）
    Exporting,
}

impl MockJobPhase {
    /// 能否给出定量百分比（只有生成阶段能）。
    pub fn is_quantified(self) -> bool {
        matches!(self, Self::Generating)
    }

    /// 阶段文案（进度行 / 详情摘要用）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Generating => "生成中",
            Self::Writing => "写入项目分析库中",
            Self::Exporting => "写出文件中",
        }
    }
}

/// 后台任务进度快照。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MockJobProgress {
    /// 当前阶段
    pub phase: MockJobPhase,
    /// 已完成「量」（单表生成 = 批次；场景模板 = 张表）。
    /// 量纲由任务种类决定（见 `MockJobKind::rows_total`：场景任务为 0 行，按张表报）。
    pub batches_done: usize,
    /// 总量（单表生成 = 总批次；场景模板 = 总表数）。首次回调前为 0，表示「尚未开始」
    pub batches_total: usize,
    /// 本次任务的行数（文案用；场景模板为 0，因为行数写在模板里、草稿不参与）
    pub rows_total: u32,
}

impl MockJobProgress {
    /// 完成百分比（0.0~100.0；非生成阶段或总批次未知时为 0）。
    pub fn percent(&self) -> f32 {
        if self.batches_total == 0 {
            return 0.0;
        }
        (self.batches_done as f32 / self.batches_total as f32 * 100.0).clamp(0.0, 100.0)
    }

    /// 已完成行数（批次粒度估算：向下取整到批）。
    ///
    /// `rows_total` 为 0 时恒为 0——场景模板任务就是这种（量纲是「张表」，不是行），
    /// 面板对这类任务改用 `batches_done / batches_total` 报「{done} / {total} 张表」。
    pub fn rows_done(&self) -> u32 {
        if self.batches_total == 0 {
            return 0;
        }
        let per_batch = (self.rows_total as f64 / self.batches_total as f64).ceil() as u32;
        (self.batches_done as u32 * per_batch).min(self.rows_total)
    }
}

/// 后台任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockJobState {
    /// 无任务
    Idle,
    /// 进行中（可取消）
    Running(MockJobProgress),
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
    /// 可导入结构的连接
    fn schema_sources(&self) -> Vec<SchemaSource>;
    /// 读某表的列（源库结构）
    fn import_columns(&self, request: &SchemaRequest) -> Result<Vec<MockColumnSpec>, String>;
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

// ==================== 尺寸常量（视图局部，rem 基准） ====================

/// 行数 / 种子输入框宽度（5rem = 80px）
const NUM_INPUT_WIDTH: f32 = 5.0;
/// 对话框内输入框宽度（9rem = 144px）
const PARAM_INPUT_WIDTH: f32 = 9.0;
/// 预览单元格宽度（9rem = 144px；配横向滚动）
const PREVIEW_CELL_WIDTH: f32 = 9.0;
/// 字段清单滚动区最大高度（16rem = 256px）
const FIELD_LIST_MAX_HEIGHT: f32 = 16.0;
/// 详情 tab 预览区最小高度（10rem = 160px）
const PREVIEW_MIN_HEIGHT: f32 = 10.0;
/// 单次生成行数上限（误输入护栏）
const MAX_ROWS: u32 = 1_000_000;
/// 预览显示行数上限
const PREVIEW_ROWS: usize = 10;
/// 集合类参数多行输入的高度（5rem = 80px ≈ 5 行）
const COMPLEX_INPUT_HEIGHT: f32 = 5.0;

/// 生成器搜索列表高度（16rem = 256px；137 项靠 `List` 虚拟化 + 自带滚动）
const SEARCH_LIST_HEIGHT: f32 = 16.0;

/// 13 种语言（面板下拉用）
const LOCALES: [Locale; 13] = [
    Locale::ZhCn,
    Locale::En,
    Locale::JaJp,
    Locale::ZhTw,
    Locale::FrFr,
    Locale::DeDe,
    Locale::ItIt,
    Locale::PtBr,
    Locale::PtPt,
    Locale::NlNl,
    Locale::ArSa,
    Locale::TrTr,
    Locale::FaIr,
];

/// 文件出口格式（「另存为」与「保存到草稿箱」共用；`Table` 是引擎内部用法，不在此列）。
const FILE_FORMATS: [(&str, MockExportFormat); 4] = [
    ("CSV", MockExportFormat::Csv),
    ("Parquet", MockExportFormat::Parquet),
    ("Xlsx", MockExportFormat::Xlsx),
    ("SQL INSERT", MockExportFormat::SqlInsert),
];

/// 语言显示名。
pub fn locale_label(locale: &Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "简体中文",
        Locale::En => "英语",
        Locale::JaJp => "日语",
        Locale::ZhTw => "繁体中文",
        Locale::FrFr => "法语",
        Locale::DeDe => "德语",
        Locale::ItIt => "意大利语",
        Locale::PtBr => "葡萄牙语（巴西）",
        Locale::PtPt => "葡萄牙语",
        Locale::NlNl => "荷兰语",
        Locale::ArSa => "阿拉伯语",
        Locale::TrTr => "土耳其语",
        Locale::FaIr => "波斯语",
    }
}

/// 可选列类型（13 种，与 `ColumnDataType` 一一对应）。
pub const COLUMN_TYPES: [ColumnDataType; 13] = [
    ColumnDataType::Integer,
    ColumnDataType::BigInt,
    ColumnDataType::Float,
    ColumnDataType::Double,
    ColumnDataType::Decimal {
        precision: 18,
        scale: 2,
    },
    ColumnDataType::Boolean,
    ColumnDataType::Varchar { length: None },
    ColumnDataType::Text,
    ColumnDataType::Date,
    ColumnDataType::DateTime,
    ColumnDataType::Timestamp,
    ColumnDataType::Uuid,
    ColumnDataType::Blob,
];

/// 类型显示名（下拉与清单共用）。
pub fn column_type_label(data_type: &ColumnDataType) -> String {
    data_type.to_duckdb_type()
}

// ==================== 纯逻辑（无窗口依赖，可单测） ====================

/// 解析行数输入：只接受正整数，上限 [`MAX_ROWS`]。
pub(crate) fn parse_rows(text: &str) -> Result<u32, String> {
    match text.trim().parse::<u32>() {
        Ok(n) if n > 0 => Ok(n.min(MAX_ROWS)),
        _ => Err("行数需为正整数".to_string()),
    }
}

/// 解析种子输入：空串表示随机（`None`）。
pub(crate) fn parse_seed(text: &str) -> Result<Option<u32>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed
        .parse::<u32>()
        .map(Some)
        .map_err(|_| "种子需为 0~4294967295 的整数，留空则随机".to_string())
}

/// 解析空值率输入（按百分比编辑）：0~100 → 0.0~1.0；非法返回 `None`（保留原值）。
pub(crate) fn parse_percent_ratio(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .map(|percent| (percent / 100.0).clamp(0.0, 1.0))
}

/// 校验目标表名（非空、字母数字与下划线、不以数字开头）。
pub(crate) fn validate_table_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("表名不能为空".to_string());
    }
    if !trimmed.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("表名只能包含字母、数字与下划线".to_string());
    }
    if trimmed.chars().next().is_some_and(|c| c.is_numeric()) {
        return Err("表名不能以数字开头".to_string());
    }
    Ok(trimmed.to_string())
}

/// 集合类参数最多多少项（防手滑贴进几万行；这些是取值集合，不是数据）。
const MAX_COMPLEX_ITEMS: usize = 1000;

/// 复杂参数（集合 / 加权选项）的文本 → JSON 值。
///
/// 格式（与 [`complex_param_text`] 成对）：
/// - `values`（外键取值 / 序列取值）：一行一个值，空行忽略；
/// - `choices`（加权选项）：一行「值, 权重」（半角/全角逗号或制表符分隔，取**最后**一个分隔符，
///   所以值里可以带逗号）。
///
/// 校验不通过**不猜**：返回带行号的可读原因，调用方保留上一个有效值并就地提示。
/// 为何必须拦住空集合与全零权重：生成期对它们会直接 panic（见 `MockEngine` 的 `generator_param_problem`）。
pub(crate) fn parse_complex_param(key: &str, text: &str) -> Result<serde_json::Value, String> {
    let lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line.trim()))
        .filter(|(_, line)| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return Err("至少要有一个值".to_string());
    }
    if lines.len() > MAX_COMPLEX_ITEMS {
        return Err(format!(
            "最多 {MAX_COMPLEX_ITEMS} 项（当前 {} 项）",
            lines.len()
        ));
    }

    match key {
        "values" => Ok(serde_json::Value::Array(
            lines
                .iter()
                .map(|(_, line)| serde_json::Value::from((*line).to_string()))
                .collect(),
        )),
        "choices" => {
            let mut choices = Vec::with_capacity(lines.len());
            let mut has_positive = false;
            for (line_no, line) in lines {
                let Some((name, weight_text)) = split_choice(line) else {
                    return Err(format!("第 {line_no} 行「{line}」：期望「值, 权重」"));
                };
                let Ok(weight) = weight_text.parse::<f64>() else {
                    return Err(format!("第 {line_no} 行「{weight_text}」：权重需为数字"));
                };
                if !weight.is_finite() || weight < 0.0 {
                    return Err(format!(
                        "第 {line_no} 行「{weight_text}」：权重需为不小于 0 的数字"
                    ));
                }
                has_positive |= weight > 0.0;
                choices.push(serde_json::Value::Array(vec![
                    serde_json::Value::from(name),
                    serde_json::Value::from(weight),
                ]));
            }
            if !has_positive {
                return Err("至少一个权重要大于 0（否则无法抽样）".to_string());
            }
            Ok(serde_json::Value::Array(choices))
        }
        other => Err(format!("暂不支持编辑该参数：{other}")),
    }
}

/// 加权选项的一行：`值, 权重`（分隔符取**最后一个**，值里可以带逗号 / 全角逗号 / 制表符）。
fn split_choice(line: &str) -> Option<(&str, &str)> {
    let (index, separator) = line
        .char_indices()
        .filter(|(_, c)| matches!(c, ',' | '，' | '\t'))
        .last()?;
    let name = line[..index].trim();
    let weight = line[index + separator.len_utf8()..].trim();
    (!name.is_empty() && !weight.is_empty()).then_some((name, weight))
}

/// 复杂参数的初值文本（JSON 值 → 多行文本，与 [`parse_complex_param`] 成对）。
pub(crate) fn complex_param_text(config: &GeneratorConfig, key: &str) -> String {
    let Some(serde_json::Value::Array(items)) =
        payload_of(config).and_then(|p| p.get(key).cloned())
    else {
        return String::new();
    };
    match key {
        "choices" => items
            .iter()
            .map(|item| {
                let name = item.get(0).and_then(|v| v.as_str()).unwrap_or_default();
                let weight = item.get(1).and_then(|v| v.as_f64()).unwrap_or_default();
                format!("{name}, {weight}")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => items
            .iter()
            .map(|item| item.as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// 复杂参数的格式提示（对话框里贴在多行输入下方）。
pub(crate) fn complex_param_hint(key: &str) -> &'static str {
    match key {
        "choices" => "每行「值, 权重」，如「北京, 3」；权重越大越容易被选中",
        _ => "每行一个值，如「已发货」",
    }
}

/// 把生成器参数按 JSON 补丁写回配置（137 变体零手工构造，见 `generator_catalog`）。
pub(crate) fn patch_param(
    config: &GeneratorConfig,
    key: &str,
    text: &str,
    kind: ParamKind,
) -> Option<GeneratorConfig> {
    let next = match kind {
        ParamKind::Int => serde_json::Value::from(text.trim().parse::<i64>().ok()?),
        ParamKind::Float => serde_json::Value::from(text.trim().parse::<f64>().ok()?),
        ParamKind::Text => serde_json::Value::from(text.trim().to_string()),
        ParamKind::Bool => serde_json::Value::from(matches!(
            text.trim().to_lowercase().as_str(),
            "true" | "1" | "是" | "yes"
        )),
        // 复杂参数走 `parse_complex_param`（需要校验与行号提示），不在这里续写解析
        ParamKind::Complex => parse_complex_param(key, text).ok()?,
    };
    patch_param_value(config, key, next)
}

/// 把已解析好的 JSON 值写进生成器对应字段（复杂参数的入口）。
pub(crate) fn patch_param_value(
    config: &GeneratorConfig,
    key: &str,
    next: serde_json::Value,
) -> Option<GeneratorConfig> {
    let mut value = serde_json::to_value(config).ok()?;
    let outer = value.as_object_mut()?;
    let payload = match outer.len() {
        1 => outer.values_mut().next()?.as_object_mut()?,
        _ => return None,
    };
    let slot = payload.get_mut(key)?;
    *slot = next;
    serde_json::from_value(value).ok()
}

/// 取变体载荷对象（外部标记枚举的唯一键）。
fn payload_of(config: &GeneratorConfig) -> Option<serde_json::Map<String, serde_json::Value>> {
    let value = serde_json::to_value(config).ok()?;
    let outer = value.as_object()?;
    if outer.len() != 1 {
        return None;
    }
    let key = outer.keys().next()?.clone();
    outer.get(&key)?.as_object().cloned()
}

/// 生成器参数摘要（一行说清：`最小值 1 · 最大值 100`）。
pub(crate) fn summarize_params(config: &GeneratorConfig) -> String {
    let spec = generator_catalog::spec_of(config);
    if spec.params.is_empty() {
        return String::new();
    }
    let Some(payload) = payload_of(config) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for field in spec.params {
        let Some(raw) = payload.get(field.key) else {
            continue;
        };
        let text = match field.kind {
            ParamKind::Complex => {
                let count = raw.as_array().map(|items| items.len()).unwrap_or(0);
                format!("{count} 项")
            }
            ParamKind::Bool => if raw.as_bool().unwrap_or(false) {
                "是"
            } else {
                "否"
            }
            .to_string(),
            _ => match raw {
                serde_json::Value::Null => "未设".to_string(),
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        };
        parts.push(format!("{} {}", field.label, text));
    }
    parts.join(" · ")
}

/// 参数初值文本（JSON 载荷值 → 输入框字符串）。
fn param_text(config: &GeneratorConfig, key: &str) -> String {
    payload_of(config)
        .and_then(|payload| payload.get(key).cloned())
        .map(|raw| match raw {
            serde_json::Value::String(s) => s,
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

/// 按类型给一个可用默认生成器（手工加列用；历史重放时也用它兜底）。
pub(crate) fn default_generator_for(data_type: &ColumnDataType) -> GeneratorConfig {
    match data_type {
        ColumnDataType::Integer | ColumnDataType::BigInt => {
            GeneratorConfig::RandomInt { min: 1, max: 1000 }
        }
        ColumnDataType::Float | ColumnDataType::Double => GeneratorConfig::RandomFloat {
            min: 0.0,
            max: 1000.0,
            precision: 2,
        },
        ColumnDataType::Decimal { .. } => GeneratorConfig::RandomDecimal {
            min: 0.0,
            max: 1000.0,
            scale: 2,
        },
        ColumnDataType::Boolean => GeneratorConfig::Boolean { ratio: 50 },
        _ => GeneratorConfig::Sentence { min: 1, max: 3 },
    }
}

/// 新建列的默认定义（手工加列用）：类型默认生成器，置信度 `manual`。
fn new_column_spec(id: u64, name: String, data_type: ColumnDataType) -> MockColumnSpec {
    let generator = default_generator_for(&data_type);
    MockColumnSpec {
        id,
        def: ColumnDef {
            name,
            data_type,
            generator,
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        },
        confidence: "manual".to_string(),
        sample_value: String::new(),
    }
}

/// 数据行数的千分位文案（`161000` → `161,000`）。
///
/// 场景模板的合计行数可达六位（社交平台 161,000 行），不加分隔难以一眼读量；
/// 与 `analytics_resource::present` / `insight::model` 各自保留一份同口径小助手的做法一致
/// （只在文案上用，不引入依赖）。
///
/// 注意：只用于**数据行数**；`第 N 行` 这类**行号**不要过它。
fn with_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 导出文件名（含扩展名）。
fn mock_file_name(table_name: &str, format: &MockExportFormat) -> String {
    let ext = match format {
        MockExportFormat::Csv => "csv",
        MockExportFormat::Parquet => "parquet",
        MockExportFormat::Xlsx => "xlsx",
        MockExportFormat::SqlInsert => "sql",
        MockExportFormat::Table => "duckdb",
    };
    format!("{table_name}.{ext}")
}

/// 生成器选择菜单（按分类分子菜单）——菜单路径：知道「属于哪类」时最快。
///
/// 详情 tab 的字段行专用（列编辑对话框里生成器是只读展示，见 D19）；
/// 选中即写回**配置面板**（状态单一权威）。菜单第一项是「搜索生成器」，
/// 给「只记得名字」的场景用（137 项靠分类翻找太慢）。
fn generator_menu(
    panel: Entity<MockPanel>,
    id: u64,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, cx| {
        let mut menu = menu;
        let search = panel.clone();
        menu = menu
            .item(
                PopupMenuItem::new(format!(
                    "搜索生成器…（{} 项）",
                    generator_catalog::all_specs().len()
                ))
                .on_click(move |_, window, app| {
                    search.update(app, |panel, cx| panel.open_generator_search(id, window, cx));
                }),
            )
            .separator();
        for category in GeneratorCategory::ALL {
            let panel = panel.clone();
            menu = menu.submenu(category.label(), window, cx, move |sub, _window, _cx| {
                let mut sub = sub;
                for spec in generator_catalog::all_specs()
                    .iter()
                    .copied()
                    .filter(|s| s.category == category)
                {
                    let panel = panel.clone();
                    let name = spec.name;
                    sub = sub.item(PopupMenuItem::new(spec.label).on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| panel.set_generator(id, name, cx));
                    }));
                }
                sub
            });
        }
        menu
    }
}

// ==================== 生成器搜索（第二条路径） ====================

/// 搜索生成器目录：按**中文标签 / 名称 / 分类名**匹配（大小写不敏感，多词之间是 AND）。
///
/// 排序（越靠前越像用户要找的）：标签前缀 → 名称前缀 → 标签包含 → 名称包含 → 分类名包含；
/// 同级保持目录顺序。空查询返回全部 137 项（对话框初态）。
///
/// 搜索是“知道大概叫什么”的路径；按分类翻菜单是“知道属于哪类”的路径，两者并存（D24）。
pub fn search_generators(query: &str) -> Vec<&'static GeneratorSpec> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut hits: Vec<(u8, usize, &'static GeneratorSpec)> = Vec::new();
    for (index, spec) in generator_catalog::all_specs().iter().copied().enumerate() {
        let label = spec.label.to_lowercase();
        let name = spec.name.to_lowercase();
        let category = spec.category.label().to_lowercase();
        let mut rank = 0u8;
        let mut matched = true;
        for term in terms.iter() {
            // 命中强弱：标签前缀 0 < 名称前缀 1 < 标签包含 2 < 名称包含 3 < 分类名 4
            let hit = if label.starts_with(term.as_str()) {
                0
            } else if name.starts_with(term.as_str()) {
                1
            } else if label.contains(term.as_str()) {
                2
            } else if name.contains(term.as_str()) {
                3
            } else if category.contains(term.as_str()) {
                4
            } else {
                matched = false;
                break;
            };
            rank = rank.max(hit);
        }
        if matched {
            hits.push((rank, index, spec));
        }
    }
    hits.sort_by_key(|(rank, index, _)| (*rank, *index));
    hits.into_iter().map(|(_, _, spec)| spec).collect()
}

/// 「搜索生成器」列表的委托：内层靠 `List` 组件（自带搜索框 / 虚拟化 / 回车与点击确认）。
///
/// 目录只有 137 项且全在内存：`perform_search` 同步过滤，不走异步搜索通道（无需 loading 占位）。
struct GeneratorSearchDelegate {
    /// 选中后写回的面板（草稿的单一权威）
    panel: Entity<MockPanel>,
    /// 目标列 id
    column_id: u64,
    /// 该列当前在用的生成器（列表里打勾；`None` = 列已被删）
    current: Option<&'static str>,
    /// 命中项（`search_generators` 的结果）
    hits: Vec<&'static GeneratorSpec>,
    /// 列表选中项（上下键 / 鼠标悬停）
    selected: Option<IndexPath>,
}

impl GeneratorSearchDelegate {
    /// 建委托：初态＝全量目录（137 项）+ 勾出该列当前的生成器。
    ///
    /// `current` 由调用方（面板自己的 `&mut self`）算好传入：本方法在面板的 update 里被调用，
    /// 那时 `read` 面板会触发「already being updated」重入 panic。
    fn new(panel: Entity<MockPanel>, column_id: u64, current: Option<&'static str>) -> Self {
        Self {
            panel,
            column_id,
            current,
            hits: search_generators(""),
            selected: None,
        }
    }

    /// 命中项（仅测试观察：生产侧不需要读它）
    #[cfg(test)]
    fn hits(&self) -> &[&'static GeneratorSpec] {
        &self.hits
    }
}

impl ListDelegate for GeneratorSearchDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.hits = search_generators(query);
        cx.notify();
        Task::ready(())
    }

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.hits.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let spec = *self.hits.get(ix.row)?;
        let muted = cx.theme().colors.muted_foreground;
        // 一行三列：中文标签（主）/ 名称 / 分类（辅）；当前在用的那项打勾
        Some(
            ListItem::new(ix.row)
                .confirmed(self.current == Some(spec.name))
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_3()
                        .w_full()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .text_ellipsis()
                                .child(spec.label),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(spec.name),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(spec.category.label()),
                        ),
                ),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
        cx.notify();
    }

    /// 点一行 / 回车：写回该列的生成器并关掉对话框。
    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(spec) = self.selected.and_then(|ix| self.hits.get(ix.row).copied()) else {
            return;
        };
        let name = spec.name;
        self.panel.update(cx, |panel, cx| {
            panel.set_generator(self.column_id, name, cx)
        });
        window.close_dialog(cx);
    }
}

// ==================== 配置面板（右 Dock） ====================

/// Mock 配置面板（右 Dock 17.5rem）：目标 + 选项 + 列来源 + 生成 + 出口。
pub struct MockPanel {
    host: Rc<dyn MockHost>,
    draft: MockDraft,
    /// 列 id 计数器（列可增删，键用 id）
    next_id: u64,
    /// 最近一次运行的**结果表**（单表生成 = 1 条；场景模板 = N 条）
    results: Vec<MockGenInfo>,
    /// 当前选中的结果表下标（出口作用于它；越界视为 0）
    current: usize,
    /// 当前结果来自哪套场景模板（`None` = 单表生成；面板据此给一句来源说明）
    scenario_source: Option<String>,
    /// 最近一次成功落库的表名（出口反馈用）
    landed: Option<String>,
    /// 本会话里已落库的表（项目库里确实有了；切项目时清空——那是另一个库）
    landed_tables: Vec<String>,
    outcome: Option<String>,
    error: Option<String>,
    /// 可导入结构的连接（事件路径加载）
    sources: Vec<SchemaSource>,
    /// 分析库既有表（追加候选，事件路径加载）
    existing_tables: Vec<String>,
    /// 内置场景模板清单（构造时算一次；见 [`ScenarioChoice`]）
    scenario_templates: Vec<ScenarioChoice>,
    /// 场景工作副本：选模板后进入**可编辑**态（改完关系再生成）；`None` = 单表态。
    ///
    /// 关系就在模板各列的 `dependency` 上（单一权威），这里不另存一份关系清单。
    scenario: Option<ScenarioTemplate>,
    /// 最近一次**场景生成**用的关系快照（结果还在，关系就还得说得出——
    /// 用户可能已经退出场景态，也可能改过工作副本）。
    last_relations: Vec<ScenarioRelation>,
    /// 「加关系」对话框里正在选的四个位置（都是名字；`None` = 未选）。
    relation_pick: Rc<RefCell<RelationPick>>,
    /// 「编辑表」对话框正在改哪张表（`None` = 没开）
    table_editing: Option<String>,
    /// 「编辑表」对话框的输入：表名 / 行数
    edit_name_input: Option<Entity<InputState>>,
    edit_rows_input: Option<Entity<InputState>>,
    table_input: Option<Entity<InputState>>,
    /// 待写入表名输入的值（事件路径置位，下一帧渲染时落地）
    table_pending: Option<String>,
    rows_input: Option<Entity<InputState>>,
    seed_input: Option<Entity<InputState>>,
    /// 导入结构对话框：选中的连接（`None` = 未选）
    import_conn: Rc<RefCell<Option<String>>>,
    import_catalog: Option<Entity<InputState>>,
    import_schema: Option<Entity<InputState>>,
    import_table: Option<Entity<InputState>>,
    /// 进行中的后台任务（`None` = 空闲；生成与追加共用）
    job: Option<MockJobWatch>,
    /// 生成历史（最近 [`history::HISTORY_LIMIT`] 条，时间倒序）
    history: Vec<MockGenerationTask>,
    /// 用户模板（与历史同一次后台读拿回，见 `HistorySnapshot`）
    templates: Vec<MockUserTemplate>,
    /// 历史与模板是否读到过（区分「还没读」与「读完了是空的」）
    history_loaded: bool,
    /// 历史与模板读写在途（后台）
    history_loading: bool,
    /// 历史与模板读写失败的原因（成功一次就清掉）
    history_error: Option<String>,
    /// 「保存为模板」对话框的名称输入框
    template_name: Option<Entity<InputState>>,
}

/// 进行中任务的视图侧状态（进度镜像 + 轮询泵句柄）。
struct MockJobWatch {
    /// 任务种类（收尾时据此分派结果）
    kind: MockJobKind,
    /// 最近一次拉到的进度
    progress: MockJobProgress,
    /// 已请求取消（按钮转「正在取消…」）
    cancel_requested: bool,
    /// 轮询泵（持句柄即存活；任务结束或面板销毁时自动停）
    _pump: Task<()>,
}

/// 交给后台执行器的一件事（读与写共用一条路径）。
#[derive(Clone)]
enum HistoryTask {
    /// 读快照（历史 + 模板）
    List,
    /// 删一条历史后读回
    Delete(String),
    /// 记一次生成运行后读回
    Record { draft: MockDraft, run: RunRecord },
    /// 把草稿存成模板后读回
    SaveTemplate { name: String, draft: MockDraft },
    /// 删一个模板后读回
    DeleteTemplate(String),
    /// 取一条历史的完整配置（重放）
    Replay(String),
    /// 取一个模板的完整配置（应用）
    ApplyTemplate(String),
}

/// 后台读的回填形态（与 [`HistoryTask`] 一一对应）。
pub(crate) enum HistoryReply {
    /// 历史与模板已刷新
    Listed(history::HistorySnapshot),
    /// 重放：拿回当时的完整配置（列表不动）
    Replayed(Box<MockGenerationDetail>),
    /// 应用模板：拿回模板与它的列（列表不动）
    TemplateApplied(Box<(MockUserTemplate, Vec<MockTemplateColumn>)>),
}

/// 内置场景模板的条目（面板菜单用）：只留名字与规模，模板本体在生成时按 id 重取。
///
/// 缓存的原因：[`crate::templates::get_builtin_templates`] 会现场构造整套列定义
/// （数百个 `ColumnDef`），每帧重建不划算——「渲染期零 I/O」同样包含「零重活」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioChoice {
    /// 模板 id（提交任务用）
    pub id: String,
    /// 模板名（菜单与来源文案用）
    pub name: String,
    /// 模板里的表数
    pub table_count: usize,
    /// 模板里的总行数（菜单预览用）
    pub total_rows: u32,
}

impl ScenarioChoice {
    /// 菜单条目文案：`零售电商（5 张表 · 12,000 行）`。
    pub fn menu_label(&self) -> String {
        format!(
            "{}（{} 张表 · {} 行）",
            self.name,
            self.table_count,
            with_thousands(self.total_rows as u64)
        )
    }
}

/// 场景态里的一条表间关系（渲染与测试用的**派生视图**：真身在列的 `dependency` 上）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioRelation {
    /// 子表（持有外键列的表）
    pub child_table: String,
    /// 子列（外键列）
    pub child_column: String,
    /// 父表（被引用的表）
    pub parent_table: String,
    /// 父列（被引用的列，目前只支持自增主键）
    pub parent_column: String,
}

impl ScenarioRelation {
    /// 文案：`orders.user_id → users.id`。
    pub fn label(&self) -> String {
        format!(
            "{}.{} → {}.{}",
            self.child_table, self.child_column, self.parent_table, self.parent_column
        )
    }
}

/// 从模板里扫出全部表间关系（真身在列的 `dependency` 上，这里是派生）。
fn relations_of(template: &ScenarioTemplate) -> Vec<ScenarioRelation> {
    template
        .tables
        .iter()
        .flat_map(|table| {
            table.columns.iter().filter_map(move |col| {
                let dep = col.dependency.as_ref()?;
                if !dep.is_foreign_key() {
                    return None;
                }
                Some(ScenarioRelation {
                    child_table: table.name.clone(),
                    child_column: col.name.clone(),
                    parent_table: dep.ref_table.clone().unwrap_or_default(),
                    parent_column: dep.ref_column.clone().unwrap_or_default(),
                })
            })
        })
        .collect()
}

/// 「加关系」对话框里正在选的四个位置（都是名字；`None` = 未选）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RelationPick {
    child_table: Option<String>,
    child_column: Option<String>,
    parent_table: Option<String>,
    parent_column: Option<String>,
}

/// 内置场景模板的轻量清单（构造面板时取一次）。
fn builtin_scenario_choices() -> Vec<ScenarioChoice> {
    crate::templates::get_builtin_templates()
        .into_iter()
        .map(|template| ScenarioChoice {
            table_count: template.tables.len(),
            total_rows: template.tables.iter().map(|t| t.row_count).sum(),
            id: template.id,
            name: template.name,
        })
        .collect()
}

impl MockPanel {
    /// 创建面板（宿主在 `cx.new` 中调用；构造不做 I/O）。
    pub fn new(host: Rc<dyn MockHost>, _cx: &mut Context<Self>) -> Self {
        Self {
            host,
            draft: MockDraft::default(),
            next_id: 1,
            results: Vec::new(),
            current: 0,
            scenario_source: None,
            landed: None,
            landed_tables: Vec::new(),
            outcome: None,
            error: None,
            sources: Vec::new(),
            existing_tables: Vec::new(),
            scenario_templates: builtin_scenario_choices(),
            scenario: None,
            last_relations: Vec::new(),
            relation_pick: Rc::new(RefCell::new(RelationPick::default())),
            table_editing: None,
            edit_name_input: None,
            edit_rows_input: None,
            table_input: None,
            table_pending: None,
            rows_input: None,
            seed_input: None,
            import_conn: Rc::new(RefCell::new(None)),
            import_catalog: None,
            import_schema: None,
            import_table: None,
            job: None,
            history: Vec::new(),
            templates: Vec::new(),
            history_loaded: false,
            history_loading: false,
            history_error: None,
            template_name: None,
        }
    }

    // ==================== 只读访问器（宿主 / 详情 tab / 测试） ====================

    /// 当前草稿
    pub fn draft(&self) -> &MockDraft {
        &self.draft
    }

    /// 最近一次生成结果
    pub fn gen_info(&self) -> Option<&MockGenInfo> {
        self.current_info()
    }

    /// 当前结果表（`current` 越界时回退到第一条；空结果返回 `None`）。
    fn current_info(&self) -> Option<&MockGenInfo> {
        self.results
            .get(self.current.min(self.results.len().saturating_sub(1)))
            .filter(|_| !self.results.is_empty())
    }

    /// 结果表清单（场景生成后不止一张；出口作用于当前选中的那张）。
    pub fn results(&self) -> &[MockGenInfo] {
        &self.results
    }

    /// 当前选中的结果表下标。
    pub fn current_result(&self) -> usize {
        self.current
    }

    /// 当前结果来自哪套场景模板（`None` = 单表生成）。
    pub fn scenario_source(&self) -> Option<&str> {
        self.scenario_source.as_deref()
    }

    /// 可用的内置场景模板（面板「场景模板 ▾」菜单；构造时算好，渲染期零重活）。
    pub fn scenario_templates(&self) -> &[ScenarioChoice] {
        &self.scenario_templates
    }

    /// 最近一次成功文案
    pub fn outcome(&self) -> Option<&str> {
        self.outcome.as_deref()
    }

    /// 最近一次失败原因
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// 最近一次成功落库的表名
    pub fn landed(&self) -> Option<&str> {
        self.landed.as_deref()
    }

    /// 可导入结构的连接
    pub fn sources(&self) -> &[SchemaSource] {
        &self.sources
    }

    /// 分析库既有表
    pub fn existing_tables(&self) -> &[String] {
        &self.existing_tables
    }

    /// 是否正在跑后台任务（生成 / 追加 / 出口）
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// 进行中的任务是否**含生成阶段**（决定「生成中…」文案与取消按钮）
    pub fn is_generating(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.kind.generates())
    }

    /// 进行中任务的种类（无任务时为 `None`）；面板与测试据此区分量纲（批次 / 张表）。
    pub fn job_kind(&self) -> Option<&MockJobKind> {
        self.job.as_ref().map(|job| &job.kind)
    }

    /// 进行中任务的进度（无任务时为 `None`）
    pub fn job_progress(&self) -> Option<MockJobProgress> {
        self.job.as_ref().map(|job| job.progress)
    }

    /// 已请求取消（按钮文案转「正在取消…」）
    pub fn cancel_requested(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.cancel_requested)
    }

    // ==================== 事件路径入口 ====================

    /// 重新加载「可导入结构的连接」与「分析库既有表」（打开面板 / 下拉刷新）。
    pub fn refresh_sources(&mut self, cx: &mut Context<Self>) {
        self.sources = self.host.schema_sources();
        self.existing_tables = self.host.existing_tables();
        cx.notify();
    }

    /// 项目已切换：作废与旧项目绑定的生成结果与出口反馈（`cleared` = 宿主刚清掉的临时表数）。
    ///
    /// 为什么必须作废：宿主在切项目时删掉了本进程的 mock 临时表（内存库是进程级单例，
    /// 不切项目就会一直在），`gen_info` 里的临时表名已经指向不存在的表——留着它，
    /// 用户下一次点「导出 / 落库」只会拿到一个难懂的「表不存在」。
    ///
    /// 草稿（目标表名 / 列 / 行数种子）**保留**：那是用户的配置，跨项目可以继续用。
    pub fn forget_generated(&mut self, cleared: usize, cx: &mut Context<Self>) {
        self.results.clear();
        self.current = 0;
        self.scenario_source = None;
        self.last_relations.clear();
        self.landed = None;
        // 另一个项目 = 另一个分析库：上一项目的落库记录、「追加到既有表」候选在这里都不成立
        self.landed_tables.clear();
        self.existing_tables.clear();
        // 连接清册也随作用域变（全局 + 该项目的 `P_` / `GP_`）：宿主在切项目后会重读，
        // 真没重读也不打紧——导入对话框在清册为空时会自己拉一次
        self.sources.clear();
        self.error = None;
        // 没清到东西就不打扰用户（切项目很常见，每次都报一句是噪声）
        self.outcome = (cleared > 0).then(|| {
            format!("已切换项目：清掉 {cleared} 张 mock 临时表，生成结果已作废（请重新生成）")
        });
        cx.notify();
    }

    /// 打开中央「Mock 数据」详情 tab（草稿 tab：字段清单 + 预览）。
    pub fn open_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_detail_for(DetailTarget::Draft, window, cx);
    }

    /// 打开（或聚焦）某个详情 tab：草稿，或某张结果表。
    pub fn open_detail_for(
        &mut self,
        target: DetailTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.host.open_detail(target, window, cx);
    }

    /// 打开某张**结果表**的详情 tab（右 Dock 结果区点一行 / 「查看详情」走这条）。
    pub fn open_table_detail(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_detail_for(DetailTarget::Table(table.to_string()), window, cx);
    }

    /// 「查看详情」看谁：有结果就看当前表（tab 就是那张表），没结果看草稿。
    pub fn current_detail_target(&self) -> DetailTarget {
        self.results
            .get(self.current)
            .map(|info| DetailTarget::Table(info.table_name.clone()))
            .unwrap_or(DetailTarget::Draft)
    }

    /// 中央某个 tab 被激活：把它对应的表设为「当前表」（出口作用于它）。
    ///
    /// 表不在本轮结果里（tab 是上一轮留下的）就什么都不做：`current` 不该指向不存在的表。
    pub fn focus_table(&mut self, table: &str, cx: &mut Context<Self>) {
        if let Some(index) = self
            .results
            .iter()
            .position(|info| info.table_name == table)
        {
            if index != self.current {
                self.current = index;
                cx.notify();
            }
        }
    }

    // ==================== 生成历史（后台读写；渲染只读状态） ====================

    /// 重读生成历史（打开面板 / 切换项目时调用）。
    ///
    /// 只投一次后台读：开项目库是文件 I/O，事件路径上做会冻 UI，渲染期更不行。
    pub fn refresh_history(&mut self, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::List, cx);
    }

    /// 删一条历史（删除与重读在同一件后台动作里完成）。
    pub fn delete_history(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::Delete(task_id), cx);
    }

    /// 重放一条历史：把当时的表名 / 行数 / 种子 / 语言与列配置写回草稿。
    ///
    /// **不自动生成**：重放是「把配置摆回来」，跑不跑由用户定（同模板应用口径）。
    pub fn replay_history(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::Replay(task_id), cx);
    }

    /// 把当前草稿存成用户模板（名字由保存对话框给，空名在这里拦住）。
    ///
    /// 两道门都落在动作本身而不只在对话框上：对话框只是入口之一，
    /// 校验在这里才能保证「不管谁调都不会存出一份套不出东西的模板」。
    pub fn save_template(&mut self, name: String, cx: &mut Context<Self>) {
        if self.draft.columns.is_empty() {
            self.history_error =
                Some("先加列（或导入结构）再存模板：空配置存下来没有意义".to_string());
            cx.notify();
            return;
        }
        if name.trim().is_empty() {
            self.history_error = Some("模板名不能为空".to_string());
            cx.notify();
            return;
        }
        let draft = self.draft.clone();
        self.spawn_history(HistoryTask::SaveTemplate { name, draft }, cx);
    }

    /// 应用一个用户模板：把它存的行数 / 种子 / 语言与列写回草稿（**不动目标表名**）。
    pub fn apply_template(&mut self, template_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::ApplyTemplate(template_id), cx);
    }

    /// 删一个用户模板（删除与重读在同一件后台动作里完成）。
    pub fn delete_template(&mut self, template_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::DeleteTemplate(template_id), cx);
    }

    /// 把面板要做的一件事交给后台执行器，完成后经弱句柄回填。
    ///
    /// 未打开项目时不去跑后台：历史没有落点，直接给一句可读的原因
    /// （而不是一个看起来像「本项目没有记录」的空列表）。
    fn spawn_history(&mut self, task: HistoryTask, cx: &mut Context<Self>) {
        let Some(root) = self.host.project_root() else {
            // 未打开项目：历史没有落点，给一句可读的原因（而不是看起来像「本项目没有记录」）。
            // 只有「重读列表」才清空：删记录 / 重放失败时，已显示的列表比一片空白有用。
            if matches!(task, HistoryTask::List) {
                self.history.clear();
                self.templates.clear();
                self.history_loaded = true;
            }
            self.history_loading = false;
            self.history_error = Some("未打开项目：生成历史与模板随项目保存".to_string());
            cx.notify();
            return;
        };
        self.history_loading = true;
        self.history_error = None;
        let work = cx.background_executor().spawn(async move {
            // 项目库的打开与读写要 tokio 运行时（GPUI 后台线程不带）：`history::drive` 自己进一个
            match task {
                HistoryTask::List => history::drive(history::list(&root, history::HISTORY_LIMIT))
                    .map(HistoryReply::Listed),
                HistoryTask::Delete(id) => history::drive(history::run(
                    &root,
                    HistoryAction::DeleteTask(id),
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::Record { draft, run } => history::drive(history::run(
                    &root,
                    HistoryAction::Record { draft, run },
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::SaveTemplate { name, draft } => history::drive(history::run(
                    &root,
                    HistoryAction::SaveTemplate { name, draft },
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::DeleteTemplate(id) => history::drive(history::run(
                    &root,
                    HistoryAction::DeleteTemplate(id),
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::ApplyTemplate(id) => {
                    history::drive(history::template_detail(&root, &id))
                        .map(|detail| HistoryReply::TemplateApplied(Box::new(detail)))
                }
                HistoryTask::Replay(id) => history::drive(history::detail(&root, &id))
                    .map(|detail| HistoryReply::Replayed(Box::new(detail))),
            }
        });
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let reply = work.await;
            // 面板可能已关闭：弱句柄升级失败就丢弃结果（不 panic）
            let _ = weak.update(cx, |panel, cx| panel.accept_history(reply, cx));
        })
        .detach();
        cx.notify();
    }

    /// 回填后台结果（`pub(crate)`：窗口测试直接喂它，不等真实后台）。
    pub(crate) fn accept_history(
        &mut self,
        reply: Result<HistoryReply, String>,
        cx: &mut Context<Self>,
    ) {
        self.history_loading = false;
        match reply {
            Ok(HistoryReply::Listed(snapshot)) => {
                self.history = snapshot.tasks;
                self.templates = snapshot.templates;
                self.history_loaded = true;
                self.history_error = None;
            }
            Ok(HistoryReply::Replayed(detail)) => {
                self.apply_replay(*detail);
                self.history_loaded = true;
                self.history_error = None;
            }
            Ok(HistoryReply::TemplateApplied(detail)) => {
                let (template, columns) = *detail;
                self.apply_template_config(&template, &columns);
                self.history_loaded = true;
                self.history_error = None;
            }
            // 失败不改列表：能看到的旧列表比一片空白有用
            Err(reason) => self.history_error = Some(reason),
        }
        cx.notify();
    }

    /// 应用模板落地：行数 / 种子 / 语言与列换成模板里那一套，**目标表名不动**
    /// （纯状态动作，测试直接调）。
    pub(crate) fn apply_template_config(
        &mut self,
        template: &MockUserTemplate,
        columns: &[MockTemplateColumn],
    ) {
        let mut draft = history::draft_of_template(template, columns);
        // 模板不存表名：用户正在写的目标表名保住（换表名是另一件事）
        draft.table_name = self.draft.table_name.clone();
        let rows = draft.options.rows;
        let count = draft.columns.len();
        self.next_id = count as u64 + 1;
        self.draft = draft;
        // 旧结果作废：草稿已是另一套配置，临时表还是上一套的
        self.results.clear();
        self.current = 0;
        self.landed = None;
        self.error = None;
        self.outcome = Some(format!(
            "已应用模板 {}（{} 行 · {count} 列；种子与语言一并写入）",
            template.name,
            with_thousands(rows as u64)
        ));
    }

    /// 重放落地：草稿整体换成历史里的那一套（纯状态动作，测试直接调）。
    pub(crate) fn apply_replay(&mut self, detail: MockGenerationDetail) {
        let draft = history::draft_of_detail(&detail);
        let table = draft.table_name.clone();
        let columns = draft.columns.len();
        self.next_id = draft
            .columns
            .iter()
            .map(|column| column.id)
            .max()
            .unwrap_or(0)
            + 1;
        self.draft = draft;
        self.table_pending = Some(self.draft.table_name.clone());
        // 旧结果作废：草稿已是另一套配置，临时表还是上一套的
        self.results.clear();
        self.current = 0;
        self.landed = None;
        self.error = None;
        self.outcome = Some(format!(
            "已重放 {table} 的配置（{columns} 列）：确认后再点生成"
        ));
    }

    /// 导航右键「生成 Mock 数据」：按**源库表**预填目标表名并导入其结构。
    ///
    /// 这正是 v1 的主路径（源库结构 → 造新数据），不再依赖「分析库已有同名表」。
    pub fn preset_from_source(&mut self, request: SchemaRequest, cx: &mut Context<Self>) {
        self.draft.table_name = request.table.clone();
        self.table_pending = Some(request.table.clone());
        match self.host.import_columns(&request) {
            Ok(columns) => {
                self.draft.columns = columns;
                self.next_id = self.draft.columns.iter().map(|c| c.id).max().unwrap_or(0) + 1;
                self.results.clear();
                self.current = 0;
                self.landed = None;
                self.error = None;
                self.outcome = Some(format!(
                    "已从 {}.{}.{} 导入 {} 列",
                    request.catalog,
                    request.schema,
                    request.table,
                    self.draft.columns.len()
                ));
            }
            Err(e) => {
                self.outcome = None;
                self.landed = None;
                self.error = Some(e);
            }
        }
        cx.notify();
    }

    /// 生成到内存临时表（**后台任务**：立即返回，进度与结果由轮询回填）。
    ///
    /// 不在事件路径里写库：真正落库是出口的事（不变式 I5）。
    pub fn run_generate(&mut self, cx: &mut Context<Self>) {
        self.start_job(MockJobKind::Generate, cx);
    }

    /// 场景模板：把模板**载入为可编辑的工作副本**（不立即生成）。
    ///
    /// 为何不选即生成：用户要能先看到「本次要生成哪几张表」、并把表间关系调好，
    /// 再开始生成（关系是这次生成的一部分，不能生成完再补——那就要去改已落地的数据了）。
    pub fn open_scenario(&mut self, template_id: &str, cx: &mut Context<Self>) {
        if self.is_running() {
            self.fail("已有任务在进行中（请等它结束或先取消）", cx);
            return;
        }
        let Some(template) = crate::templates::get_template_by_id(template_id) else {
            self.fail(format!("场景模板不存在：{template_id}"), cx);
            return;
        };
        self.load_scenario(template, cx);
    }

    /// 把**当前草稿**加进场景工作副本（自定义多表场景的入口）。
    ///
    /// 为何从草稿来：导入源库结构、智能映射、列编辑、行数 / 种子这一整套都在单表态里
    /// 现成可用——“把一张表调到满意，然后把它加进场景”比再做一个多表编辑器便宜得多，
    /// 也不破坏现有交互。
    pub fn add_draft_to_scenario(&mut self, cx: &mut Context<Self>) {
        let name = self.draft.table_name.trim().to_string();
        if name.is_empty() {
            self.fail("草稿的目标表名为空：先在「目标表名」里填一个", cx);
            return;
        }
        if self.draft.columns.is_empty() {
            self.fail("草稿还没有列：先导入源库结构或手工加列", cx);
            return;
        }
        let Some(template) = self.scenario.as_mut() else {
            self.fail("先选一套场景模板（或在模板基础上改）", cx);
            return;
        };
        if template.tables.iter().any(|table| table.name == name) {
            self.fail(
                format!("场景里已经有表 {name}：先删掉那张，或改草稿的目标表名"),
                cx,
            );
            return;
        }
        let table = TemplateTable {
            name: name.clone(),
            row_count: self.draft.options.rows,
            columns: self.draft.columns.iter().map(|c| c.def.clone()).collect(),
        };
        template.tables.push(table);
        self.error = None;
        self.outcome = Some(format!(
            "已把草稿 {name}（{} 行 · {} 列）加进本次生成",
            with_thousands(u64::from(self.draft.options.rows)),
            self.draft.columns.len()
        ));
        cx.notify();
    }

    /// 从场景工作副本里删掉一张表，**并连带清掉指向它的关系**。
    ///
    /// 不清的话，剩余引用会变成“指向模板里没有的表”——生成前校验会报错，
    /// 但那时用户已经不知道是谁指向它了。所以这里直接清掉并报出清了几条。
    pub fn remove_scenario_table(&mut self, table: &str, cx: &mut Context<Self>) {
        let Some(template) = self.scenario.as_mut() else {
            return;
        };
        let before = template.tables.len();
        template.tables.retain(|t| t.name != table);
        if template.tables.len() == before {
            return;
        }
        let mut dropped = 0usize;
        for child in template.tables.iter_mut() {
            for column in child.columns.iter_mut() {
                let points_here = column.dependency.as_ref().is_some_and(|dep| {
                    dep.is_foreign_key() && dep.ref_table.as_deref() == Some(table)
                });
                if points_here {
                    column.dependency = None;
                    dropped += 1;
                }
            }
        }
        self.error = None;
        self.outcome = Some(if dropped == 0 {
            format!("已从本次生成里删掉表 {table}")
        } else {
            format!("已删掉表 {table}，并清掉 {dropped} 条指向它的关系")
        });
        cx.notify();
    }

    /// 载入场景工作副本（[`Self::open_scenario`] 按 id 取到模板后调它；测试直接给模板）。
    pub(crate) fn load_scenario(&mut self, template: ScenarioTemplate, cx: &mut Context<Self>) {
        self.scenario = Some(template);
        *self.relation_pick.borrow_mut() = RelationPick::default();
        // 已结果及其关系快照不动：载入工作副本不等于重新生成，旧结果仍然能导出
        self.error = None;
        self.outcome = None;
        cx.notify();
    }

    /// 退出场景态（丢弃工作副本，草稿与已有结果不受影响）。
    pub fn close_scenario(&mut self, cx: &mut Context<Self>) {
        self.scenario = None;
        *self.relation_pick.borrow_mut() = RelationPick::default();
        cx.notify();
    }

    /// 场景态下的「生成」（后台任务：一次产出模板里的全部表；**不写库**）。
    pub fn run_scenario(&mut self, cx: &mut Context<Self>) {
        let Some(template) = self.scenario.clone() else {
            self.fail("先选一套场景模板", cx);
            return;
        };
        // 生成前校验引用（父表 / 父列不合适就不提交；错误在面板上就地显示）
        if let Err(e) = MockEngine::resolve_reference_domains(&template) {
            self.fail(format!("表间关系有问题：{e}"), cx);
            return;
        }
        self.start_job(MockJobKind::Scenario(Box::new(template)), cx);
    }

    /// 场景工作副本（`None` = 单表态）。
    pub fn scenario(&self) -> Option<&ScenarioTemplate> {
        self.scenario.as_ref()
    }

    /// 关系闭包里**还没落库**的表（按结果表顺序；已落库的跳过）。
    ///
    /// 为何要它：关系是跨表的，而出口以「当前表」为单位——用户很容易只落其中
    /// 几张，留下悬空的引用。这里把「还差哪几张」摆出来，让他一次落完。
    /// 空 = 当前表不参与关系，或关系里的表都落了。
    pub fn pending_relation_tables(&self) -> Vec<String> {
        if self.last_relations.is_empty() {
            return Vec::new();
        }
        let Some(current) = self.current_info().map(|info| info.table_name.clone()) else {
            return Vec::new();
        };
        // 从当前表出发沿关系双向走一遍，得到它与关系闭包（父子链）
        let mut closure = vec![current];
        loop {
            let mut added = false;
            for relation in &self.last_relations {
                for (from, to) in [
                    (&relation.child_table, &relation.parent_table),
                    (&relation.parent_table, &relation.child_table),
                ] {
                    if closure.iter().any(|name| name == from)
                        && !closure.iter().any(|name| name == to)
                    {
                        closure.push(to.clone());
                        added = true;
                    }
                }
            }
            if !added {
                break;
            }
        }
        self.results
            .iter()
            .map(|info| info.table_name.clone())
            .filter(|name| closure.iter().any(|c| c == name))
            .filter(|name| !self.landed_tables.iter().any(|landed| landed == name))
            .collect()
    }

    /// 本会话已落库的表。
    pub fn landed_tables(&self) -> &[String] {
        &self.landed_tables
    }

    /// 出口：把关系闭包里还没落库的表**依次**落成分析库新表（后台任务）。
    ///
    /// 逐张走「新建」语义：同名已存在就跳过它并报出原因（不覆盖、也不回滚别人）。
    pub fn persist_related(&mut self, cx: &mut Context<Self>) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        let pending = self.pending_relation_tables();
        if pending.is_empty() {
            self.fail("关系里没有待落库的表", cx);
            return;
        }
        let infos: Vec<MockGenInfo> = self
            .results
            .iter()
            .filter(|info| pending.contains(&info.table_name))
            .cloned()
            .collect();
        self.start_job(MockJobKind::PersistAll(infos), cx);
    }

    /// 当前结果表的**跨表后果**（用于出口提示；`None` = 它不参与任何关系）。
    ///
    /// 为何要给这句：**出口只作用于当前表**，而关系是跨表的——只落子表不落父表，
    /// 落地的数据就悬空了（而 mock 不改已落地的数据，也不会去替用户补）。
    pub fn current_relation_note(&self) -> Option<String> {
        let current = self.current_info()?.table_name.clone();
        let mut out: Vec<String> = Vec::new();
        let parents: Vec<String> = self
            .last_relations
            .iter()
            .filter(|r| r.child_table == current)
            .map(|r| {
                format!(
                    "{} → {}.{}",
                    r.child_column, r.parent_table, r.parent_column
                )
            })
            .collect();
        if !parents.is_empty() {
            let mut parents = parents;
            parents.sort();
            parents.dedup();
            out.push(format!(
                "引用了 {}：只落这张表，被引用的表不会跟着落库",
                parents.join("、")
            ));
        }
        let children: Vec<String> = self
            .last_relations
            .iter()
            .filter(|r| r.parent_table == current)
            .map(|r| format!("{}.{}", r.child_table, r.child_column))
            .collect();
        if !children.is_empty() {
            let mut children = children;
            children.sort();
            children.dedup();
            out.push(format!(
                "被 {} 引用：只落这张表，引用它的一方会落空",
                children.join("、")
            ));
        }
        (!out.is_empty()).then(|| out.join("；"))
    }

    /// 最近一次**场景生成**用的关系快照（结果还在就保留；单表生成会清掉）。
    pub fn last_relations(&self) -> &[ScenarioRelation] {
        &self.last_relations
    }
    /// 工作副本里的表间关系（派生视图：扫列上的 `dependency`，不另存清单）。
    pub fn scenario_relations(&self) -> Vec<ScenarioRelation> {
        let Some(template) = self.scenario.as_ref() else {
            return Vec::new();
        };
        relations_of(template)
    }

    /// 关系目标的**取值域**文案（`1..1,000`）；父列自增且父表在册时给出。
    ///
    /// 面板在关系行里显示它：改父表行数前后，用户能直接看到采样域变了——
    /// 这正是“域由父表行数算出”的可见回报。
    pub fn relation_range(&self, relation: &ScenarioRelation) -> Option<String> {
        let template = self.scenario.as_ref()?;
        let parent = template
            .tables
            .iter()
            .find(|t| t.name == relation.parent_table)?;
        let column = parent
            .columns
            .iter()
            .find(|c| c.name == relation.parent_column)?;
        let GeneratorConfig::AutoIncrement { start, step } = column.generator else {
            return None;
        };
        let domain = ReferenceDomain {
            table: parent.name.clone(),
            column: column.name.clone(),
            first: i64::from(start),
            step: i64::from(step),
            count: parent.row_count,
        };
        Some(format!(
            "{}..{}",
            with_thousands(domain.first.max(0) as u64),
            with_thousands(domain.last().max(0) as u64)
        ))
    }

    /// 打开「编辑表」对话框（表名 / 行数）。
    ///
    /// 改行数是场景里最常用的一步（比如把财务模板的 10 万行缩下来），而引用它的表
    /// **自动跟着变**——域由父表行数算出，不需要用户去同步任何东西。
    pub fn open_table_dialog(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self
            .scenario
            .as_ref()
            .and_then(|template| template.tables.iter().find(|t| t.name == table))
            .cloned()
        else {
            self.fail("先选一套场景模板", cx);
            return;
        };
        self.table_editing = Some(current.name.clone());
        // 上一次的错误留在面板上会让人以为是这次的输入有问题：打开就清掉
        self.error = None;
        let name_input = self
            .edit_name_input
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("表名")))
            .clone();
        let rows_input = self
            .edit_rows_input
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("行数")))
            .clone();
        name_input.update(cx, |state, cx| {
            state.set_value(current.name.clone(), window, cx)
        });
        rows_input.update(cx, |state, cx| {
            state.set_value(current.row_count.to_string(), window, cx)
        });

        let panel = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let muted = theme.colors.muted_foreground;
            let danger = theme.colors.danger;
            // 校验没过时对话框不关：错误就地显示，改完能直接再点「应用」
            let error = panel.read(cx).error.clone();
            let mut body =
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div().text_xs().text_color(muted).child(
                            "只改这张表在本次生成里的配置；已生成的预览与已落地的表都不动。",
                        ),
                    )
                    .child(form_line(theme, "表名", &name_input))
                    .child(form_line(theme, "行数", &rows_input))
                    .child(
                        div().text_xs().text_color(muted).child(
                            "引用它的表会自动跟着变：取值域由父表行数算出（改完下次生成生效）",
                        ),
                    );
            if let Some(error) = error {
                body = body.child(div().text_xs().text_color(danger).child(error));
            }
            let cancel_panel = panel.clone();
            let ok_panel = panel.clone();
            dialog.title("编辑表").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("mock-table-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(move |_, window, app| {
                                let _ = cancel_panel
                                    .update(app, |panel, _cx| panel.table_editing = None);
                                window.close_dialog(app);
                            }),
                    )
                    .child(
                        Button::new("mock-table-ok")
                            .with_variant(ButtonVariant::Primary)
                            .label("应用")
                            .on_click(move |_, window, app| {
                                let applied = ok_panel.update(app, |panel, cx| {
                                    panel.apply_table_edit(cx);
                                    panel.table_editing.is_none()
                                });
                                if applied {
                                    window.close_dialog(app);
                                }
                            }),
                    ),
            )
        });
    }

    /// 应用「编辑表」：校验 → 改工作副本 → **同步指向它的关系的 `ref_table`**。
    pub fn apply_table_edit(&mut self, cx: &mut Context<Self>) {
        let Some(original) = self.table_editing.clone() else {
            return;
        };
        // 表名走与单表路径**同一个**校验（非空 / 字母数字下划线 / 不以数字开头）：
        // 让非法名在这里就被拦住，而不是到生成时才由引擎报出来
        let name = match self
            .edit_name_input
            .as_ref()
            .map(|input| validate_table_name(&input.read(cx).value()))
        {
            Some(Ok(name)) => name,
            Some(Err(e)) => {
                self.fail(e, cx);
                return;
            }
            None => return,
        };
        let rows = match self
            .edit_rows_input
            .as_ref()
            .map(|input| parse_rows(&input.read(cx).value()))
        {
            Some(Ok(rows)) => rows,
            Some(Err(e)) => {
                self.fail(e, cx);
                return;
            }
            None => return,
        };
        let renamed = name != original;
        let Some(template) = self.scenario.as_mut() else {
            return;
        };
        if renamed && template.tables.iter().any(|t| t.name == name) {
            self.fail(format!("本次生成里已经有表 {name}"), cx);
            return;
        }
        let Some(table) = template.tables.iter_mut().find(|t| t.name == original) else {
            return;
        };
        table.name = name.clone();
        table.row_count = rows;
        let mut retargeted = 0usize;
        if renamed {
            // 改了名，指向它的引用必须跟着改——否则立即变成“指向模板里没有的表”
            for child in template.tables.iter_mut() {
                for column in child.columns.iter_mut() {
                    let points_here = column
                        .dependency
                        .as_mut()
                        .filter(|dep| dep.is_foreign_key())
                        .is_some_and(|dep| {
                            if dep.ref_table.as_deref() == Some(original.as_str()) {
                                dep.ref_table = Some(name.clone());
                                true
                            } else {
                                false
                            }
                        });
                    if points_here {
                        retargeted += 1;
                    }
                }
            }
        }
        self.table_editing = None;
        self.error = None;
        self.outcome = Some(if retargeted == 0 {
            format!(
                "已更新表 {name}（{} 行）：下次生成生效",
                with_thousands(u64::from(rows))
            )
        } else {
            format!(
                "已更新表 {name}（{} 行），并同步 {retargeted} 条指向它的引用：下次生成生效",
                with_thousands(u64::from(rows))
            )
        });
        cx.notify();
    }

    /// 打开「加关系」对话框（子表.列 → 父表.列）。
    ///
    /// 父列只列**自增列**：只有它能算出取值域，其他列在引擎侧会被拒——
    /// 把不能选的东西藏起来，比先让用户选完再报错好。
    pub fn open_relation_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(template) = self.scenario.clone() else {
            self.fail("先选一套场景模板", cx);
            return;
        };
        // 每次打开都从「未选」开始：上一次的选择带到下一次容易误改
        *self.relation_pick.borrow_mut() = RelationPick::default();
        let pick = self.relation_pick.clone();
        let panel = cx.entity();
        let template = Rc::new(template);

        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let muted = theme.colors.muted_foreground;
            let warning = theme.colors.warning;
            let current = pick.borrow().clone();
            let table_names: Vec<String> =
                template.tables.iter().map(|t| t.name.clone()).collect();
            let child_columns: Vec<String> = current
                .child_table
                .as_deref()
                .and_then(|name| template.tables.iter().find(|t| t.name == name))
                .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();
            let parent_columns: Vec<String> = current
                .parent_table
                .as_deref()
                .and_then(|name| template.tables.iter().find(|t| t.name == name))
                .map(|t| {
                    t.columns
                        .iter()
                        .filter(|c| matches!(c.generator, GeneratorConfig::AutoIncrement { .. }))
                        .map(|c| c.name.clone())
                        .collect()
                })
                .unwrap_or_default();

            // 一个下拉行：标签 + 当前值按钮 + 菜单
            let row = |id: &'static str,
                       label: &'static str,
                       current: Option<&String>,
                       placeholder: &'static str,
                       items: Vec<String>,
                       empty_note: &'static str,
                       on_pick: Rc<dyn Fn(String, &mut App)>| {
                let pick_for_menu = on_pick.clone();
                let text = match current {
                    Some(value) => format!("{value} ▾"),
                    None => format!("{placeholder} ▾"),
                };
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .w(rems(4.0))
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(label),
                    )
                    .child(
                        Button::new(id)
                            .ghost()
                            .label(text)
                            .dropdown_menu(move |menu, _window, _cx| {
                                let mut menu = menu;
                                if items.is_empty() {
                                    return menu.item(PopupMenuItem::new(empty_note).disabled(true));
                                }
                                for item in items.iter() {
                                    let value = item.clone();
                                    let pick_for_item = pick_for_menu.clone();
                                    menu = menu.item(PopupMenuItem::new(item.clone()).on_click(
                                        move |_, _, app| pick_for_item(value.clone(), app),
                                    ));
                                }
                                menu
                            }),
                    )
            };

            // 每个下拉的点击：改工作选择，再请面板重绘（对话框由窗口重绘时重建）
            let pick_child_table = pick.clone();
            let panel_child_table = panel.clone();
            let on_child_table = Rc::new(move |value: String, app: &mut App| {
                let mut pick = pick_child_table.borrow_mut();
                pick.child_table = Some(value);
                pick.child_column = None;
                drop(pick);
                panel_child_table.update(app, |_, cx| cx.notify());
            });
            let pick_child_column = pick.clone();
            let panel_child_column = panel.clone();
            let on_child_column = Rc::new(move |value: String, app: &mut App| {
                pick_child_column.borrow_mut().child_column = Some(value);
                panel_child_column.update(app, |_, cx| cx.notify());
            });
            let pick_parent_table = pick.clone();
            let panel_parent_table = panel.clone();
            let on_parent_table = Rc::new(move |value: String, app: &mut App| {
                let mut pick = pick_parent_table.borrow_mut();
                pick.parent_table = Some(value);
                pick.parent_column = None;
                drop(pick);
                panel_parent_table.update(app, |_, cx| cx.notify());
            });
            let pick_parent_column = pick.clone();
            let panel_parent_column = panel.clone();
            let on_parent_column = Rc::new(move |value: String, app: &mut App| {
                pick_parent_column.borrow_mut().parent_column = Some(value);
                panel_parent_column.update(app, |_, cx| cx.notify());
            });

            let ready = current.child_table.is_some()
                && current.child_column.is_some()
                && current.parent_column.is_some();
            let mut body = div()
                .v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("引用只在**本次多表生成**内成立：值取自父表主键的取值域（由自增参数与行数算出）。"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("不读已落地的数据，也不修改已生成的表 / 文件。"),
                )
                .child(row(
                    "mock-relation-child-table",
                    "子表",
                    current.child_table.as_ref(),
                    "选择子表",
                    table_names.clone(),
                    "（模板里没有表）",
                    on_child_table,
                ))
                .child(row(
                    "mock-relation-child-column",
                    "子列",
                    current.child_column.as_ref(),
                    "选择子列",
                    child_columns,
                    "（先选子表）",
                    on_child_column,
                ))
                .child(row(
                    "mock-relation-parent-table",
                    "父表",
                    current.parent_table.as_ref(),
                    "选择父表",
                    table_names,
                    "（模板里没有表）",
                    on_parent_table,
                ))
                .child(row(
                    "mock-relation-parent-column",
                    "父列",
                    current.parent_column.as_ref(),
                    "选择父列",
                    parent_columns.clone(),
                    "（先选父表）",
                    on_parent_column,
                ));
            if current.parent_table.is_some() && parent_columns.is_empty() {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(warning)
                        .child("这张表没有自增主键列——引用目标只能是自增列（域算得出才不用去读数据）"),
                );
            }

            let cancel_panel = panel.clone();
            let ok_panel = panel.clone();
            dialog.title("加表间关系").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("mock-relation-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(move |_, window, app| {
                                let _ = cancel_panel.update(app, |panel, cx| {
                                    *panel.relation_pick.borrow_mut() = RelationPick::default();
                                    cx.notify();
                                });
                                window.close_dialog(app);
                            }),
                    )
                    .child({
                        let mut ok = Button::new("mock-relation-ok")
                            .with_variant(ButtonVariant::Primary)
                            .label("添加")
                            .disabled(!ready);
                        if ready {
                            ok = ok.on_click(move |_, window, app| {
                                let _ = ok_panel.update(app, |panel, cx| panel.add_relation(cx));
                                window.close_dialog(app);
                            });
                        }
                        ok
                    }),
            )
        });
    }

    /// 把对话框里选好的四个位置写成一条关系（挂在子列上）。
    ///
    /// 同时把该列的 `generator` **对齐到父域**：单表生成时按它取值，
    /// 域必须落在父域内（否则同一列换个入口就产出不一样的值）。
    fn add_relation(&mut self, cx: &mut Context<Self>) {
        let pick = self.relation_pick.borrow().clone();
        let (Some(child_table), Some(child_column), Some(parent_table), Some(parent_column)) = (
            pick.child_table,
            pick.child_column,
            pick.parent_table,
            pick.parent_column,
        ) else {
            self.fail("子表 / 子列 / 父表 / 父列都要选", cx);
            return;
        };

        let mut problem: Option<String> = None;
        let mut message = String::new();
        if let Some(template) = self.scenario.as_mut() {
            let domain = template
                .tables
                .iter()
                .find(|t| t.name == parent_table)
                .and_then(|parent| {
                    let column = parent.columns.iter().find(|c| c.name == parent_column)?;
                    let GeneratorConfig::AutoIncrement { start, step } = column.generator else {
                        return None;
                    };
                    Some(ReferenceDomain {
                        table: parent_table.clone(),
                        column: parent_column.clone(),
                        first: i64::from(start),
                        step: i64::from(step),
                        count: parent.row_count,
                    })
                });
            match domain {
                None => {
                    problem = Some(format!(
                        "{parent_table}.{parent_column} 不能作引用目标（只支持自增主键列）"
                    ));
                }
                Some(domain) => {
                    let child = template
                        .tables
                        .iter_mut()
                        .find(|t| t.name == child_table)
                        .and_then(|t| t.columns.iter_mut().find(|c| c.name == child_column));
                    match child {
                        None => {
                            problem = Some(format!("子表 {child_table} 没有列 {child_column}"));
                        }
                        Some(column) => {
                            column.dependency =
                                Some(ColumnDependency::foreign_key(&parent_table, &parent_column));
                            column.generator = GeneratorConfig::RandomInt {
                                min: domain.first as i32,
                                max: domain.last() as i32,
                            };
                            message = format!(
                                "已加关系 {child_table}.{child_column} → {}",
                                domain.label()
                            );
                        }
                    }
                }
            }
        }
        // 提交前再校一遍：同一列被指向两张表之类的问题在这里就能看出来。
        // 不覆盖已有问题：用户刚做的动作报出来的原因（父列不可用）比它的下游后果更直接。
        if problem.is_none() {
            let check = match self.scenario.as_ref() {
                Some(template) => MockEngine::resolve_reference_domains(template).err(),
                None => None,
            };
            if let Some(e) = check {
                problem = Some(format!("表间关系有问题：{e}"));
            }
        }
        *self.relation_pick.borrow_mut() = RelationPick::default();
        match problem {
            Some(problem) => self.fail(problem, cx),
            None => self.succeed(message, cx),
        }
    }

    /// 删一条关系（只拆引用；该列的生成器保留，它现在是普通随机列）。
    pub fn remove_relation(
        &mut self,
        child_table: &str,
        child_column: &str,
        cx: &mut Context<Self>,
    ) {
        let mut removed = false;
        if let Some(template) = self.scenario.as_mut() {
            if let Some(column) = template
                .tables
                .iter_mut()
                .find(|t| t.name == child_table)
                .and_then(|t| t.columns.iter_mut().find(|c| c.name == child_column))
            {
                removed = column.dependency.take().is_some();
            }
        }
        if removed {
            self.outcome = Some(format!("已删除关系 {child_table}.{child_column}"));
            self.error = None;
            cx.notify();
        }
    }

    /// 切换当前选中的结果表（场景生成后不止一张）。
    pub fn select_result(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.results.len() && index != self.current {
            self.current = index;
            cx.notify();
        }
    }

    /// 出口：追加到**项目**分析库既有表（显式选择；后台任务：生成 + 追加在一次任务里完成）。
    pub fn append_table(&mut self, table: String, cx: &mut Context<Self>) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        self.start_job(MockJobKind::AppendTo(table), cx);
    }

    /// 启动后台任务（生成 / 追加 / 出口）：校验输入 → 提交 → 起轮询泵。
    fn start_job(&mut self, kind: MockJobKind, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.fail("已有任务在进行中（请等它结束或先取消）", cx);
            return;
        }
        // 草稿驱动的任务才校验草稿：场景模板与出口都不读草稿（见 `MockJobKind::uses_draft`），
        // 拿它们的输入拦截会给出与当前动作无关的错误
        if kind.uses_draft() {
            if let Err(e) = self.sync_inputs(cx) {
                self.fail(e, cx);
                return;
            }
            if self.draft.columns.is_empty() {
                self.fail("请先添加列：导入源库结构，或手工加列", cx);
                return;
            }
        }
        if let Err(e) = self.host.start_job(&self.draft, kind.clone()) {
            self.fail(e, cx);
            return;
        }
        let progress = MockJobProgress {
            phase: kind.phase(),
            batches_done: 0,
            batches_total: 0,
            rows_total: kind.rows_total(&self.draft),
        };
        self.error = None;
        self.outcome = Some(kind.running_label());
        self.job = Some(MockJobWatch {
            kind,
            progress,
            cancel_requested: false,
            _pump: self.spawn_job_pump(cx),
        });
        cx.notify();
    }

    /// 起后台任务轮询泵：每 120ms 拉一次进度 / 结果，任务收尾后自退。
    ///
    /// 用「弱句柄 + 定时器」而非 render 内轮询：任务进行中没有任何其他事件会让
    /// 面板重绘，不主动唤醒就看不到进度。
    fn spawn_job_pump(&self, cx: &mut Context<Self>) -> Task<()> {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(120)).await;
                let keep = weak
                    .update(cx, |panel, cx| panel.poll_job(cx))
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        })
    }

    /// 拉一次任务状态（返回「是否继续轮询」；窗口测试直接调它，不依赖定时器）。
    pub(crate) fn poll_job(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(kind) = self.job.as_ref().map(|job| job.kind.clone()) else {
            return false;
        };

        // 结果优先：worker 收尾时一次性「清进度 + 写结果」，不会出现两者皆无的观测空洞
        if let Some(result) = self.host.take_job_done() {
            self.job = None;
            self.finish_job(&kind, result, cx);
            return false;
        }

        match self.host.job_state() {
            MockJobState::Running(progress) => {
                if let Some(job) = self.job.as_mut() {
                    if job.progress != progress {
                        job.progress = progress;
                        cx.notify();
                    }
                }
                true
            }
            MockJobState::Idle => {
                // 结果槽空且无进行中任务：后台线程异常（崩溃 / 被杀）
                self.job = None;
                self.fail("后台生成任务异常结束（工作线程已退出）", cx);
                false
            }
        }
    }

    /// 任务收尾：写回结果（成功文案 / 落库目标）或错误。
    ///
    /// **生成类**不论成败都先作废旧结果：任务内部重建过临时表，旧预览与临时表已不一致，
    /// 留着它就会让出口拿新数据配旧预览。**出口类**只读临时表，预览仍然对得上，
    /// 不能一并作废——否则「落库完想接着导出」就没得导了。
    fn finish_job(
        &mut self,
        kind: &MockJobKind,
        result: Result<MockJobDone, String>,
        cx: &mut Context<Self>,
    ) {
        if kind.generates() {
            self.results.clear();
            self.current = 0;
        }
        // 历史在任务收尾时记（写库在后台）：出口类与取消不记，见 `RunRecord::of`
        let record = RunRecord::of(kind, &result, &self.draft);
        match result {
            Ok(MockJobDone::Generated(info)) => {
                self.landed = None;
                self.error = None;
                self.outcome = Some(format!(
                    "已生成 {} 行（耗时 {} ms）→ 临时表 {}",
                    with_thousands(info.row_count as u64),
                    info.elapsed_ms,
                    info.temp_table_name
                ));
                self.results = vec![info];
                self.current = 0;
                self.scenario_source = None;
                // 单表结果没有跨表关系可言：清掉上一轮的场景关系快照
                self.last_relations.clear();
                self.host.notify(cx);
            }
            Ok(MockJobDone::ScenarioGenerated {
                template_name,
                tables,
            }) => {
                self.landed = None;
                self.error = None;
                let total: u32 = tables.iter().map(|table| table.row_count).sum();
                self.outcome = Some(format!(
                    "已按「{template_name}」生成 {} 张表（合计 {} 行）：出口作用于当前选中的那张",
                    tables.len(),
                    with_thousands(total as u64)
                ));
                // 关系快照随结果留存：出口要按它提醒「只落这张会怎么悬空」，
                // 而工作副本随时可能被改或退出场景态
                self.last_relations = self.scenario.as_ref().map(relations_of).unwrap_or_default();
                self.results = tables;
                self.current = 0;
                self.scenario_source = Some(template_name);
                self.host.notify(cx);
            }
            Ok(MockJobDone::Appended { table, total_rows }) => {
                self.landed = Some(table.clone());
                self.error = None;
                self.outcome = Some(format!(
                    "已追加到 {table}（表内共 {} 行）",
                    with_thousands(total_rows.max(0) as u64)
                ));
                self.host.notify(cx);
            }
            Ok(MockJobDone::Persisted { table, rows }) => {
                self.landed = Some(table.clone());
                if !self.landed_tables.contains(&table) {
                    self.landed_tables.push(table.clone());
                }
                // 新表要能立刻作为「追加到既有表」的目标
                self.existing_tables = self.host.existing_tables();
                self.succeed(
                    format!(
                        "已在项目分析库新建表 {table}（{} 行）",
                        with_thousands(rows.max(0) as u64)
                    ),
                    cx,
                );
            }
            Ok(MockJobDone::PersistedAll { landed, failed }) => {
                for (table, _) in &landed {
                    if !self.landed_tables.contains(table) {
                        self.landed_tables.push(table.clone());
                    }
                }
                self.landed = landed.last().map(|(table, _)| table.clone());
                self.existing_tables = self.host.existing_tables();
                let mut parts: Vec<String> = Vec::new();
                if !landed.is_empty() {
                    parts.push(format!(
                        "已落库 {} 张：{}",
                        landed.len(),
                        landed
                            .iter()
                            .map(|(table, _)| table.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    ));
                }
                if !failed.is_empty() {
                    parts.push(format!(
                        "失败 {} 张：{}",
                        failed.len(),
                        failed
                            .iter()
                            .map(|(table, reason)| format!("{table}（{reason}）"))
                            .collect::<Vec<_>>()
                            .join("；")
                    ));
                }
                if failed.is_empty() {
                    self.succeed(parts.join("；"), cx);
                } else {
                    // 部分失败：成功的保留（不回滚别人的表），失败的原因原样摆出来
                    self.error = Some(parts.join("；"));
                    self.outcome = None;
                    self.host.notify(cx);
                    cx.notify();
                }
            }
            Ok(MockJobDone::Exported { message }) => self.succeed(message, cx),
            Err(e) => {
                let cancelled = e.contains("取消");
                // 同名表已存在是落库失败的常见情形：刷新清单，引导到「追加到既有表」
                if !cancelled && matches!(kind, MockJobKind::Persist(_)) && e.contains("已存在")
                {
                    self.existing_tables = self.host.existing_tables();
                }
                self.outcome = None;
                self.error = Some(match (kind, cancelled) {
                    // 用户主动取消：不当错误报，但要提醒临时表可能残留部分行
                    (_, true) => format!("{e}（临时表可能残留部分行，下次生成会重建）"),
                    // 追加失败与生成失败区分开：追加还多一层「写入既有表」的语义
                    (MockJobKind::AppendTo(_), false) => format!("追加失败：{e}"),
                    (_, false) => e,
                });
            }
        }
        cx.notify();
        if let Some(run) = record {
            let draft = self.draft.clone();
            self.spawn_history(HistoryTask::Record { draft, run }, cx);
        }
    }

    /// 请求取消进行中的任务：只有**含生成阶段**的任务能取消（引擎按批响应）。
    ///
    /// 出口类跑在 DuckDB / 文件系统内部，探不到中断点，强杀会留下半张表或半个文件，
    /// 所以不给取消：按钮也不渲染（见 `render_job_row`）。
    pub fn cancel_job(&mut self, cx: &mut Context<Self>) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        if !job.kind.generates() || job.cancel_requested {
            return;
        }
        job.cancel_requested = true;
        self.host.cancel_job();
        cx.notify();
    }

    /// 出口：持久化为**项目**分析库新表（后台任务：大行数落库同样会阻塞界面）。
    pub fn persist_table(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.current_info().cloned() else {
            self.fail("请先生成（预览确认后再落库）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        // 目标表名在 `start_job` 的 `sync_inputs` 里从输入框取（任务内部据此命名新表）
        self.start_job(MockJobKind::Persist(info), cx);
    }

    /// 出口：导出文件（调用方已选好路径；后台任务）。
    pub fn export_file(&mut self, format: &MockExportFormat, path: String, cx: &mut Context<Self>) {
        let Some(info) = self.current_info().cloned() else {
            self.fail("请先生成（预览确认后再导出）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写出文件", cx);
            return;
        }
        self.start_job(
            MockJobKind::Export {
                info,
                format: format.clone(),
                path,
            },
            cx,
        );
    }

    /// 出口：保存到草稿箱（`{项目}/mock/`；后台任务）。
    pub fn save_scratchpad(&mut self, format: &MockExportFormat, cx: &mut Context<Self>) {
        let Some(info) = self.current_info().cloned() else {
            self.fail("请先生成（预览确认后再保存）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写出文件", cx);
            return;
        }
        self.start_job(
            MockJobKind::Scratchpad {
                info,
                format: format.clone(),
            },
            cx,
        );
    }

    /// 手工加列（按类型给默认生成器）。
    pub fn add_column(&mut self, name: String, data_type: ColumnDataType, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let spec = new_column_spec(id, name, data_type);
        self.draft.columns.push(spec);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 删列。
    pub fn remove_column(&mut self, id: u64, cx: &mut Context<Self>) {
        self.draft.columns.retain(|c| c.id != id);
        self.results.clear();
        self.current = 0;
        self.landed = None;
        cx.notify();
    }

    /// 直接换生成器（取规格默认参数）——详情 tab 的生成器菜单。
    pub fn set_generator(&mut self, id: u64, name: &str, cx: &mut Context<Self>) {
        let Some(config) = generator_catalog::default_of(name) else {
            return;
        };
        if let Some(column) = self.draft.columns.iter_mut().find(|c| c.id == id) {
            column.def.generator = config;
            column.confidence = "manual".to_string();
            column.sample_value.clear();
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    /// 打开「搜索生成器」对话框（137 项按名称 / 中文标签 / 分类过滤）。
    ///
    /// 用 `List` 组件而不是手搜：搜索框 / 虚拟化 / 上下键 / 回车与点击确认 / 空态全是组件的，
    /// 且搜索是**同步**的（目录全在内存），不会闪 loading。
    pub fn open_generator_search(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        // 当前生成器从自身状态取：此刻面板正在被更新，不能再 `read` 自己
        let current = self
            .draft
            .columns
            .iter()
            .find(|column| column.id == id)
            .map(|column| generator_catalog::spec_of(&column.def.generator).name);
        let delegate = GeneratorSearchDelegate::new(cx.entity(), id, current);
        // `searchable` 在 `ListState` 上（搜索框是状态的一部分），占位文案在元素上
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title("搜索生成器")
                .child(div().w_full().h(rems(SEARCH_LIST_HEIGHT)).child(
                    List::new(&list).search_placeholder("按名称 / 中文标签 / 分类搜索（137 项）"),
                ))
        });
    }

    /// 应用列编辑（列名 / 类型 / 生成器 / 参数 / 空值率 / 唯一）——详情 tab 的「应用」。
    pub fn apply_column(&mut self, edited: MockColumnSpec, cx: &mut Context<Self>) {
        if let Some(slot) = self.draft.columns.iter_mut().find(|c| c.id == edited.id) {
            *slot = edited;
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    /// 按列名 + 类型重跑智能映射（「恢复智能默认」）。
    pub fn reset_column_mapping(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(slot) = self.draft.columns.iter_mut().find(|c| c.id == id) {
            let mapped = ColumnMapper::infer(&slot.def.name, &slot.def.data_type);
            slot.def.generator = mapped.generator;
            slot.def.nullable_ratio = 0.0;
            slot.def.unique = false;
            slot.confidence = mapped.confidence;
            slot.sample_value = mapped.sample_value;
            self.results.clear();
            self.current = 0;
            self.landed = None;
        }
        cx.notify();
    }

    /// 从输入框同步目标表名 / 行数 / 种子（生成与出口前调用）。
    fn sync_inputs(&mut self, cx: &App) -> Result<(), String> {
        if let Some(input) = self.table_input.as_ref() {
            self.draft.table_name = validate_table_name(&input.read(cx).value())?;
        } else {
            self.draft.table_name = validate_table_name(&self.draft.table_name)?;
        }
        if let Some(input) = self.rows_input.as_ref() {
            self.draft.options.rows = parse_rows(&input.read(cx).value())?;
        }
        if let Some(input) = self.seed_input.as_ref() {
            self.draft.options.seed = parse_seed(&input.read(cx).value())?;
        }
        Ok(())
    }

    fn succeed(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = None;
        self.outcome = Some(message);
        self.host.notify(cx);
        cx.notify();
    }

    fn fail(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.outcome = None;
        self.error = Some(message.into());
        cx.notify();
    }

    /// 生成结果是否就绪（出口按钮是否可用）
    fn has_result(&self) -> bool {
        !self.results.is_empty()
    }

    /// 「另存为」的默认文件名：取**当前结果表**的名字（场景模板下与草稿无关）。
    ///
    /// 没结果时回退到草稿的目标表名：按钮在无结果时本来就点不开（`has_result` 拦着），
    /// 这里只保证函数本身不会给出空名字。
    fn export_file_name(&self, format: &MockExportFormat) -> String {
        let table = self
            .current_info()
            .map(|info| info.table_name.clone())
            .unwrap_or_else(|| self.draft.table_name.clone());
        mock_file_name(&table, format)
    }

    /// 测试用：直接写行数输入框（校验失败路径：非法行数止于视图，不触宿主）。
    #[cfg(test)]
    pub(crate) fn set_rows_input(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.rows_input.clone() {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }

    // ==================== 渲染：配置面板 ====================

    /// 目标表名 + 行数 / 种子 / 语言。
    fn render_target(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if self.table_input.is_none() {
            let initial = self.draft.table_name.clone();
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("目标表名"));
            state.update(cx, |s, cx| s.set_value(initial, window, cx));
            self.table_input = Some(state);
        }
        if let (Some(pending), Some(input)) = (self.table_pending.take(), self.table_input.clone())
        {
            input.update(cx, |s, cx| s.set_value(pending, window, cx));
        }
        if self.rows_input.is_none() {
            let initial = self.draft.options.rows.to_string();
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("行数"));
            state.update(cx, |s, cx| s.set_value(initial, window, cx));
            self.rows_input = Some(state);
        }
        if self.seed_input.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder("随机种子"));
            self.seed_input = Some(state);
        }

        let muted = cx.theme().colors.muted_foreground;

        let mut row = div().h_flex().items_center().gap_2().w_full();
        if let Some(input) = self.table_input.clone() {
            row = row.child(div().flex_1().min_w_0().child(Input::new(&input)));
        }
        let mut nums = div().h_flex().items_center().gap_2().w_full();
        if let Some(input) = self.rows_input.clone() {
            nums = nums.child(
                div()
                    .w(rems(NUM_INPUT_WIDTH))
                    .flex_none()
                    .child(Input::new(&input)),
            );
        }
        if let Some(input) = self.seed_input.clone() {
            nums = nums.child(
                div()
                    .w(rems(NUM_INPUT_WIDTH))
                    .flex_none()
                    .child(Input::new(&input)),
            );
        }
        let locale = self.draft.options.locale.clone();
        nums = nums.child({
            let entity = cx.entity();
            Button::new("mock-locale")
                .ghost()
                .label(format!("{} ▾", locale_label(&locale)))
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for candidate in LOCALES {
                        let chosen = candidate.clone();
                        let is_current = chosen == locale;
                        let entity = entity.clone();
                        menu = menu.item(
                            PopupMenuItem::new(locale_label(&chosen))
                                .checked(is_current)
                                .on_click(move |_, _, app| {
                                    let chosen = chosen.clone();
                                    entity.update(app, |panel, cx| {
                                        panel.draft.options.locale = chosen;
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    menu
                })
        });

        div()
            .v_flex()
            .gap_1()
            .w_full()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("目标表名（新表；落库与文件名取此名）"),
            )
            .child(row)
            .child(nums)
    }

    /// 任务进行中的进度行：`Progress` 组件 + 阶段文案 + 取消按钮。
    ///
    /// - 生成类有批次粒度回调 → 定量进度条 + 取消（引擎在批次边界响应）；
    /// - 出口类（写入分析库 / 写文件）跑在 DuckDB 与文件系统内部，拿不到中间进度 →
    ///   不定量动画；也不给取消（强中断会留下半张表 / 半个文件）。
    ///
    /// 空闲时返回一个空占位（保持后续元素的排列稳定）。
    fn render_job_row(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(job) = self.job.as_ref() else {
            return div();
        };
        let progress = job.progress;
        let cancellable = job.kind.generates();
        let cancel_requested = job.cancel_requested;
        let by_table = job.kind.by_table();

        let muted = cx.theme().colors.muted_foreground;
        let detail = match progress.phase {
            MockJobPhase::Generating if progress.batches_total == 0 => "准备中…".to_string(),
            MockJobPhase::Generating if by_table => format!(
                "{} / {} 张表（每张表逐个处理）",
                progress.batches_done, progress.batches_total
            ),
            MockJobPhase::Generating => format!(
                "{} / {} 批（≈{} / {} 行）",
                progress.batches_done,
                progress.batches_total,
                with_thousands(progress.rows_done() as u64),
                with_thousands(progress.rows_total as u64)
            ),
            phase if by_table => format!(
                "{}…（{} / {} 张表）",
                phase.label(),
                progress.batches_done,
                progress.batches_total
            ),
            phase => format!(
                "{}…（{} 行）",
                phase.label(),
                with_thousands(progress.rows_total as u64)
            ),
        };
        let cancel =
            cancellable.then(|| {
                let entity = cx.entity();
                let mut button = Button::new("mock-cancel-job").secondary().xsmall().label(
                    if cancel_requested {
                        "正在取消…"
                    } else {
                        "取消"
                    },
                );
                if !cancel_requested {
                    button = button.on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| panel.cancel_job(cx));
                    });
                }
                button
            });

        div()
            .v_flex()
            .gap_1()
            .w_full()
            .child(
                Progress::new("mock-job-progress")
                    .w_full()
                    .loading(!progress.phase.is_quantified())
                    .value(progress.percent()),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(detail),
                    )
                    .children(cancel),
            )
    }

    /// 列来源 + 生成 + 出口按钮组 + 结果。
    fn render_actions(&mut self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let success = cx.theme().colors.success;
        let danger = cx.theme().colors.danger;
        let info = cx.theme().colors.info;

        // 列来源：导入源库结构 / 手工加列
        let import_btn = {
            let entity = cx.entity();
            Button::new("mock-import-schema")
                .secondary()
                .xsmall()
                .label("导入结构")
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.open_import_dialog(window, cx);
                    });
                })
        };
        let add_btn = {
            let entity = cx.entity();
            Button::new("mock-add-column")
                .secondary()
                .xsmall()
                .label("＋ 加列")
                .on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| {
                        let index = panel.draft.columns.len() + 1;
                        panel.add_column(format!("column_{index}"), ColumnDataType::Integer, cx);
                    });
                })
        };

        // 生成（后台任务：进行中时禁用，进度与取消另起一行）
        let running = self.is_running();
        let generating = self.is_generating();
        let generate = {
            let entity = cx.entity();
            let mut button = Button::new("mock-generate")
                .primary()
                .label(if generating { "生成中…" } else { "生成" })
                .w_full();
            if !running {
                button = button.on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.run_generate(cx));
                });
            }
            button
        };

        // 场景模板（内置 6 套多表一键生成）：同样是生成类任务，进行中一并禁用
        let scenario = {
            let entity = cx.entity();
            let templates = self.scenario_templates.clone();
            Button::new("mock-scenario")
                .secondary()
                .label("场景模板 ▾")
                .disabled(running)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    if templates.is_empty() {
                        return menu
                            .item(PopupMenuItem::new("（没有可用的场景模板）").disabled(true));
                    }
                    for choice in templates.iter() {
                        let id = choice.id.clone();
                        let entity = entity.clone();
                        menu = menu.item(PopupMenuItem::new(choice.menu_label()).on_click(
                            move |_, _, app| {
                                let id = id.clone();
                                entity.update(app, |panel, cx| panel.open_scenario(&id, cx));
                            },
                        ));
                    }
                    menu
                })
        };

        // 生成行：单表生成（主按钮，占满剩余宽度）+ 场景模板（多表一键生成）
        //
        // 场景态下换成「生成场景 / 退出」：两套「生成」同时摆在台上会让人分不清
        // 到底是生单表还是生一批。
        let generate_row = {
            let mut row = div().h_flex().items_center().gap_2().w_full();
            if let Some(template) = self.scenario.clone() {
                let start = {
                    let entity = cx.entity();
                    let mut button =
                        Button::new("mock-scenario-generate")
                            .primary()
                            .label(if generating {
                                "生成中…".to_string()
                            } else {
                                format!("生成 {} 张表", template.tables.len())
                            });
                    if !running {
                        button = button.on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.run_scenario(cx));
                        });
                    }
                    button
                };
                let exit = {
                    let entity = cx.entity();
                    let mut button = Button::new("mock-scenario-exit")
                        .secondary()
                        .label("退出场景")
                        .disabled(running);
                    if !running {
                        button = button.on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.close_scenario(cx));
                        });
                    }
                    button
                };
                row = row
                    .child(div().flex_1().min_w_0().child(start))
                    .child(div().flex_none().child(exit));
            } else {
                row = row
                    .child(div().flex_1().min_w_0().child(generate))
                    .child(scenario);
            }
            row
        };

        // 场景态：本次要生成的表 + 表间关系（可增删）；单表态下整体为空
        let scenario_block =
            {
                let fg = cx.theme().colors.foreground;
                let mut block = div().v_flex().gap_1().w_full();
                if let Some(template) = self.scenario.clone() {
                    let total_rows: u32 = template.tables.iter().map(|t| t.row_count).sum();
                    block = block.child(div().text_xs().text_color(muted).text_ellipsis().child(
                        format!(
                            "本次生成（{} 张表 · {} 行）· {}",
                            template.tables.len(),
                            with_thousands(total_rows as u64),
                            template.name
                        ),
                    ));
                    for table in template.tables.iter() {
                        let edit = {
                            let entity = cx.entity();
                            let name = table.name.clone();
                            let mut button = Button::new(ElementId::Name(SharedString::from(
                                format!("mock-scenario-table-edit-{name}"),
                            )))
                            .ghost()
                            .xsmall()
                            .label("编辑")
                            .disabled(running);
                            if !running {
                                button = button.on_click(move |_, window, app| {
                                    let name = name.clone();
                                    entity.update(app, |panel, cx| {
                                        panel.open_table_dialog(&name, window, cx);
                                    });
                                });
                            }
                            button
                        };
                        let remove = {
                            let entity = cx.entity();
                            let name = table.name.clone();
                            let mut button = Button::new(ElementId::Name(SharedString::from(
                                format!("mock-scenario-table-remove-{name}"),
                            )))
                            .ghost()
                            .xsmall()
                            .label("删除")
                            .disabled(running);
                            if !running {
                                button = button.on_click(move |_, _, app| {
                                    let name = name.clone();
                                    entity.update(app, |panel, cx| {
                                        panel.remove_scenario_table(&name, cx);
                                    });
                                });
                            }
                            button
                        };
                        block = block.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .w_full()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(fg)
                                        .text_ellipsis()
                                        .child(table.name.clone()),
                                )
                                .child(div().flex_none().text_xs().text_color(muted).child(
                                    format!("{} 行", with_thousands(u64::from(table.row_count))),
                                ))
                                .child(edit)
                                .child(remove),
                        );
                    }

                    // 自定义多表：把当前草稿加进来（导入结构 / 列编辑都在单表态里做完）
                    let add_table = {
                        let entity = cx.entity();
                        let mut button = Button::new("mock-scenario-table-add")
                            .secondary()
                            .xsmall()
                            .label("＋ 加表（当前草稿）")
                            .disabled(running);
                        if !running {
                            button = button.on_click(move |_, _, app| {
                                entity.update(app, |panel, cx| {
                                    panel.add_draft_to_scenario(cx);
                                });
                            });
                        }
                        button
                    };
                    block = block.child(add_table);

                    let relations = self.scenario_relations();
                    block = block.child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("表间关系（{} 条）", relations.len())),
                    );
                    if relations.is_empty() {
                        block = block.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("还没有关系：加一条，子表的列就从父表主键域取值"),
                        );
                    }
                    for relation in relations {
                        let range = self.relation_range(&relation);
                        let remove = {
                            let entity = cx.entity();
                            let child_table = relation.child_table.clone();
                            let child_column = relation.child_column.clone();
                            Button::new(ElementId::Name(SharedString::from(format!(
                                "mock-relation-remove-{}-{}",
                                child_table, child_column
                            ))))
                            .ghost()
                            .xsmall()
                            .label("删除")
                            .on_click(move |_, _, app| {
                                let child_table = child_table.clone();
                                let child_column = child_column.clone();
                                entity.update(app, |panel, cx| {
                                    panel.remove_relation(&child_table, &child_column, cx);
                                });
                            })
                        };
                        block = block.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .w_full()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(fg)
                                        .text_ellipsis()
                                        .child(relation.label()),
                                )
                                .children(range.map(|range| {
                                    div().flex_none().text_xs().text_color(muted).child(range)
                                }))
                                .child(remove),
                        );
                    }

                    let add = {
                        let entity = cx.entity();
                        let mut button = Button::new("mock-add-relation")
                            .secondary()
                            .xsmall()
                            .label("＋ 加关系")
                            .disabled(running);
                        if !running {
                            button = button.on_click(move |_, window, app| {
                                entity.update(app, |panel, cx| {
                                    panel.open_relation_dialog(window, cx);
                                });
                            });
                        }
                        button
                    };
                    block = block
                        .child(add)
                        .child(div().text_xs().text_color(muted).child(
                            "关系只在本次多表生成内成立：不读已有数据，也不改已生成的表 / 文件",
                        ));
                    block = block.child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("场景生成不记入生成历史（历史是单表配置的重放来源）"),
                    );
                }
                block
            };

        // 任务进行中：进度条（组件，不手搓）+ 量纲文案 + 取消
        let job_row = self.render_job_row(cx);

        // 出口：详情 tab + 落库 + 追加 + 草稿箱 + 另存为（任务进行中全部禁用：一次只能跑一个）
        //
        // 「查看详情」看的是**当前表**（结果表一个表一个 tab）：没结果时才回草稿。
        let detail = {
            let entity = cx.entity();
            let has_result = self.has_result();
            Button::new("mock-open-detail")
                .secondary()
                .label(if has_result {
                    "查看详情（当前表）"
                } else {
                    "查看详情（字段与预览）"
                })
                .w_full()
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        let target = panel.current_detail_target();
                        panel.open_detail_for(target, window, cx);
                    });
                })
        };
        let persist = {
            let entity = cx.entity();
            Button::new("mock-persist")
                .secondary()
                .label("持久化到项目分析库")
                .w_full()
                .disabled(running)
                .on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.persist_table(cx));
                })
        };
        let append = {
            let entity = cx.entity();
            let tables = self.existing_tables.clone();
            Button::new("mock-append")
                .secondary()
                .label("追加到既有表 ▾")
                .w_full()
                .disabled(running)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    if tables.is_empty() {
                        return menu
                            .item(PopupMenuItem::new("（项目分析库暂无表）").disabled(true));
                    }
                    for name in tables.iter() {
                        let table = name.clone();
                        let entity = entity.clone();
                        menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                            move |_, _, app| {
                                let table = table.clone();
                                entity.update(app, |panel, cx| panel.append_table(table, cx));
                            },
                        ));
                    }
                    menu
                })
        };
        let scratchpad = {
            let entity = cx.entity();
            Button::new("mock-save-scratchpad")
                .secondary()
                .label("保存到草稿箱 ▾")
                .w_full()
                .disabled(running)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for (label, format) in FILE_FORMATS {
                        let entity = entity.clone();
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.save_scratchpad(&format, cx));
                        }));
                    }
                    menu
                })
        };
        let export = {
            let entity = cx.entity();
            Button::new("mock-export")
                .secondary()
                .label("另存为 ▾")
                .w_full()
                .disabled(running)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for (label, format) in FILE_FORMATS {
                        let entity = entity.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, app| {
                                let format = format.clone();
                                let name = entity.read(app).export_file_name(&format);
                                let dir = entity.read(app).host.export_dir();
                                let dir = if dir.trim().is_empty() {
                                    PathBuf::from(".")
                                } else {
                                    PathBuf::from(dir)
                                };
                                // 系统保存对话框（异步回传）：取消则不动
                                let receiver = app.prompt_for_new_path(&dir, Some(&name));
                                let entity = entity.clone();
                                window
                                    .spawn(app, async move |cx| {
                                        let Ok(Ok(Some(path))) = receiver.await else {
                                            return;
                                        };
                                        let path = path.to_string_lossy().to_string();
                                        cx.update(|_, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.export_file(&format, path, cx)
                                            });
                                        })
                                        .ok();
                                    })
                                    .detach();
                            }));
                    }
                    menu
                })
        };

        let column_count = self.draft.columns.len();
        // 当前结果表（出口作用于它）；多张时上面给一个「当前表」选择器
        let generated = self.current_info().cloned();
        let result_count = self.results.len();
        let current_index = self.current.min(result_count.saturating_sub(1));
        let outcome = self.outcome.clone();
        let error = self.error.clone();
        let landed = self.landed.clone();
        let read_only = self.host.read_only();
        let has_result = self.has_result();
        let fg = cx.theme().colors.foreground;

        let mut panel = div()
            .v_flex()
            .gap_2()
            .w_full()
            .p_2()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(format!("列（{column_count}）")),
                    )
                    .child(import_btn)
                    .child(add_btn),
            )
            .child(generate_row)
            .child(scenario_block)
            .child(job_row);

        // 结果表：**一张表一个中央 tab**——这里列出每张表，点一行打开 / 切到它的 tab
        // （当前表 = 出口作用的那张；与实际打开的 tab 同一状态：切 tab 也会改这里）
        if result_count > 0 {
            let mut list = div().v_flex().gap_1().w_full().child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("结果表（{result_count} 张）· 点一行打开 / 切到它的 tab")),
            );
            for (index, info) in self.results.iter().enumerate() {
                let current = index == current_index;
                let name = info.table_name.clone();
                let open = {
                    let entity = cx.entity();
                    let click_name = name.clone();
                    div()
                        .id(ElementId::Name(SharedString::from(format!(
                            "mock-result-open-{click_name}"
                        ))))
                        .px_1()
                        .py_0p5()
                        .rounded(cx.theme().radius)
                        .text_xs()
                        .font_weight(if current {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(fg)
                        .cursor_pointer()
                        .child(name.clone())
                        .on_click(move |_, window, app| {
                            let name = click_name.clone();
                            entity.update(app, |panel, cx| {
                                panel.select_result(index, cx);
                                panel.open_table_detail(&name, window, cx);
                            });
                        })
                };
                list = list.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(if current { "●" } else { "○" }),
                        )
                        .child(open)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(muted)
                                .text_ellipsis()
                                .child(format!(
                                    "{} 行",
                                    with_thousands(u64::from(info.row_count))
                                )),
                        )
                        .children(current.then(|| {
                            div().flex_none().text_xs().text_color(muted).child("当前表")
                        })),
                );
            }
            let source = self.scenario_source.clone();
            panel = panel.child(list.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(match source {
                        Some(name) => {
                            format!("来自场景模板「{name}」：出口只作用于当前表——切 tab 就是切表")
                        }
                        None => "出口只作用于当前表——切 tab 就是切表".to_string(),
                    }),
            ));
        }

        if let Some(info) = generated.as_ref() {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(format!(
                        "{} · 临时表 {} · {} 行 · {} ms",
                        info.table_name,
                        info.temp_table_name,
                        with_thousands(info.row_count as u64),
                        info.elapsed_ms
                    )),
            );
        }
        panel = panel
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("出口（生成后可用）"),
            )
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .child(detail)
                    .child(persist)
                    .child(append)
                    .child(scratchpad)
                    .child(export),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("数据只写入项目分析库（{项目}/.RSmeta/analytics.duckdb）与文件，不回传源库（M7）；要进全局分析库，用资产库存档或草稿箱升级"),
            );

        if let Some(text) = outcome {
            panel = panel.child(div().text_xs().text_color(success).child(text));
        }
        if let Some(err) = error {
            panel = panel.child(div().text_xs().text_color(danger).child(err));
        }
        if let Some(table) = landed {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(fg)
                    .child(format!("落库目标：{table}")),
            );
        }
        if read_only {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(info)
                    .child("只读模式：不允许落库与写文件（仍可生成预览）"),
            );
        }
        if has_result {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("结果已就绪：点「查看详情」看字段与预览"),
            );
        }
        // 关系是跨表的，而出口只作用于当前表：把「只落这张」的后果说清楚
        if let Some(note) = self.current_relation_note() {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.warning)
                    .child(note),
            );
        }
        // 关系里还没落库的表：摆出来 + 一键依次落（免得用户漏落，留下悬空引用）
        let pending_tables = self.pending_relation_tables();
        if !pending_tables.is_empty() {
            let button = {
                let entity = cx.entity();
                let mut button = Button::new("mock-persist-related")
                    .secondary()
                    .xsmall()
                    .label(format!("落库这 {} 张", pending_tables.len()))
                    .disabled(running);
                if !running {
                    button = button.on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| panel.persist_related(cx));
                    });
                }
                button
            };
            panel = panel.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().colors.warning)
                            .text_ellipsis()
                            .child(format!(
                                "关系里还有 {} 张没落库：{}",
                                pending_tables.len(),
                                pending_tables.join("、")
                            )),
                    )
                    .child(button),
            );
        }
        panel
    }

    /// 打开「导入源库结构」对话框。
    /// 用户模板段：把当前配置存下来 / 套用已有模板 / 删除。
    ///
    /// 与历史段共用同一次后台读（`HistorySnapshot`）；「保存」走对话框问名字，
    /// 空配置与空名都在事件路径上拦住（不给静默失败）。
    fn render_templates(&mut self, cx: &mut Context<Self>) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;

        let header = {
            let entity = cx.entity();
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(muted)
                        .text_ellipsis()
                        .child(format!("用户模板（{}）", self.templates.len())),
                )
                .child(
                    Button::new("mock-template-save")
                        .ghost()
                        .xsmall()
                        .label("保存为模板…")
                        .on_click(move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                panel.open_save_template_dialog(window, cx);
                            });
                        }),
                )
        };

        let mut section = div().v_flex().gap_1().w_full().child(header);
        // 首次读取中：历史段已经说「读取中…」，这里不重复
        if self.history_loading && !self.history_loaded {
            return section;
        }
        if self.history_loaded && self.templates.is_empty() {
            return section.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("还没有保存的模板：把当前列配置存下来，下次一键套用"),
            );
        }

        for template in self.templates.clone() {
            let rows = template.row_count.max(0) as u64;
            let summary = template.description.clone().unwrap_or_default();
            let meta = if summary.is_empty() {
                format!("{} 行", with_thousands(rows))
            } else {
                format!("{} 行 · {summary}", with_thousands(rows))
            };
            let apply = {
                let entity = cx.entity();
                let id = template.id.clone();
                Button::new(ElementId::Name(SharedString::from(format!(
                    "mock-template-apply-{id}"
                ))))
                .ghost()
                .xsmall()
                .label("应用")
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.apply_template(id, cx));
                })
            };
            let delete = {
                let entity = cx.entity();
                let id = template.id.clone();
                Button::new(ElementId::Name(SharedString::from(format!(
                    "mock-template-delete-{id}"
                ))))
                .ghost()
                .xsmall()
                .label("删除")
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.delete_template(id, cx));
                })
            };

            section = section.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-template-{}",
                        template.id
                    ))))
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .p_2()
                    .border_1()
                    .border_color(border)
                    .rounded(cx.theme().radius)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(template.name.clone()),
                            )
                            .child(apply)
                            .child(delete),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(meta),
                    ),
            );
        }
        section
    }

    /// 生成历史段：最近若干次运行（重放 / 删除都在事件路径上交给后台）。
    ///
    /// 列表与错误分开呈现：读失败时**保留旧列表**（能看到的旧数据比一片空白有用）。
    /// 行按 `created_at` 倒序（存储层已排），每行的 id 用任务 id——用得下标会在
    /// 「最新在前」的插入下把 hover 等按 id 记的状态串行。
    fn render_history(&mut self, cx: &mut Context<Self>) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let danger = cx.theme().colors.danger;
        let success = cx.theme().colors.success;

        let header = {
            let entity = cx.entity();
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(muted)
                        .text_ellipsis()
                        .child(format!("生成历史（最近 {} 次）", history::HISTORY_LIMIT)),
                )
                .child(
                    Button::new("mock-history-refresh")
                        .ghost()
                        .xsmall()
                        .label("刷新")
                        .on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.refresh_history(cx));
                        }),
                )
        };

        let mut section = div().v_flex().gap_1().w_full().child(header);
        if let Some(error) = self.history_error.clone() {
            section = section.child(div().text_xs().text_color(danger).child(error));
        }
        if self.history_loading && !self.history_loaded {
            return section.child(div().text_xs().text_color(muted).child("读取中…"));
        }
        if self.history_loaded && self.history.is_empty() {
            if self.history_error.is_none() {
                section = section.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("本项目还没有生成记录"),
                );
            }
            return section;
        }

        for task in self.history.clone() {
            let rows = task.generated_rows.unwrap_or(task.row_count).max(0);
            let rows = with_thousands(rows as u64);
            let stamp = task
                .created_at
                .as_deref()
                .map(history::time_label)
                .unwrap_or_default();
            let stamp = (!stamp.is_empty())
                .then(|| div().flex_none().text_xs().text_color(muted).child(stamp));
            // 状态文案按语义取色：失败的原因原文展出来（截断由布局管）
            let (status_text, status_color) = match task.status.as_str() {
                "success" if task.save_format.as_deref() == Some(history::SAVE_FORMAT_TABLE) => {
                    (format!("成功 · {rows} 行 · 已追加"), success)
                }
                "success" => (format!("成功 · {rows} 行"), success),
                _ => (
                    task.error_message
                        .clone()
                        .unwrap_or_else(|| "失败".to_string()),
                    danger,
                ),
            };

            let replay = {
                let entity = cx.entity();
                let id = task.id.clone();
                Button::new(ElementId::Name(SharedString::from(format!(
                    "mock-history-replay-{id}"
                ))))
                .ghost()
                .xsmall()
                .label("重放")
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.replay_history(id, cx));
                })
            };
            let delete = {
                let entity = cx.entity();
                let id = task.id.clone();
                Button::new(ElementId::Name(SharedString::from(format!(
                    "mock-history-delete-{id}"
                ))))
                .ghost()
                .xsmall()
                .label("删除")
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.delete_history(id, cx));
                })
            };

            section = section.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-history-{}",
                        task.id
                    ))))
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .p_2()
                    .border_1()
                    .border_color(border)
                    .rounded(cx.theme().radius)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(task.table_name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{rows} 行")),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .children(stamp)
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_xs()
                                            .text_color(status_color)
                                            .text_ellipsis()
                                            .child(status_text),
                                    ),
                            )
                            .child(replay)
                            .child(delete),
                    ),
            );
        }
        section
    }

    /// 「保存为模板」对话框：只问名字——行数 / 种子 / 语言 / 列 / 生成器参数都取当前配置。
    ///
    /// 校验本身在 [`MockPanel::save_template`]；这里提前拒空配置，只是为了别弹一个
    /// 必然失败的框。名字默认填「{目标表名} 配置」。
    pub fn open_save_template_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft.columns.is_empty() {
            self.history_error =
                Some("先加列（或导入结构）再存模板：空配置存下来没有意义".to_string());
            cx.notify();
            return;
        }
        let name = self
            .template_name
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("模板名")))
            .clone();
        let suggestion = format!("{} 配置", self.draft.table_name.trim());
        name.update(cx, |state, cx| state.set_value(suggestion, window, cx));

        let panel = cx.entity();
        let columns = self.draft.columns.len();
        let rows = with_thousands(self.draft.options.rows as u64);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let body = div()
                .v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!(
                            "存下当前配置：{rows} 行 · {columns} 列（含生成器与参数）。\
                             目标表名不存——套用时按当时的输入走。"
                        )),
                )
                .child(form_line(theme, "模板名", &name));
            let panel_save = panel.clone();
            let input = name.clone();
            dialog.title("保存为模板").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("mock-template-dialog-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(move |_, window, app| {
                                window.close_dialog(app);
                            }),
                    )
                    .child(
                        Button::new("mock-template-dialog-ok")
                            .with_variant(ButtonVariant::Primary)
                            .label("保存")
                            .on_click(move |_, window, app| {
                                let value = input.read(app).value().to_string();
                                panel_save.update(app, |panel, cx| panel.save_template(value, cx));
                                window.close_dialog(app);
                            }),
                    ),
            )
        });
    }

    pub fn open_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sources.is_empty() {
            self.sources = self.host.schema_sources();
        }
        let panel_match = self
            .import_conn
            .borrow()
            .clone()
            .and_then(|id| self.sources.iter().find(|s| s.conn_id == id).cloned());
        let catalog = self
            .import_catalog
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("数据库")))
            .clone();
        let schema = self
            .import_schema
            .get_or_insert_with(|| {
                cx.new(|cx| InputState::new(window, cx).placeholder("schema（可留空）"))
            })
            .clone();
        let table = self
            .import_table
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("表名")))
            .clone();
        if let Some(source) = panel_match.as_ref() {
            let (catalog_value, schema_value, table_value) = (
                source.catalog.clone(),
                source.schema.clone(),
                self.draft.table_name.clone(),
            );
            catalog.update(cx, |s, cx| s.set_value(catalog_value, window, cx));
            schema.update(cx, |s, cx| s.set_value(schema_value, window, cx));
            table.update(cx, |s, cx| s.set_value(table_value, window, cx));
        }

        let panel = cx.entity();
        // `Rc` 包裹：dialog builder 每帧重建，内层菜单闭包逐个 clone 只增计数
        let sources = Rc::new(self.sources.clone());
        let selected = self.import_conn.clone();
        let catalog_input = catalog.clone();
        let schema_input = schema.clone();

        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let current = selected.borrow().clone();
            let mut body = div().v_flex().gap_2().child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("从源库读取表结构（列名 / 类型 / 可空 / 主键）并自动推断生成器；不读取数据本身。"),
            );
            // 连接：菜单选择（可见触发按钮）
            let mut conn_row = div().h_flex().items_center().gap_2().w_full();
            conn_row = conn_row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("连接"),
            );
            conn_row = conn_row.child({
                let selected = selected.clone();
                let catalog_input = catalog_input.clone();
                let schema_input = schema_input.clone();
                let sources = sources.clone();
                let current_label = current
                    .as_deref()
                    .and_then(|id| sources.iter().find(|s| s.conn_id == id))
                    .map(|s| s.label.clone());
                Button::new("mock-import-conn")
                    .ghost()
                    .label(match current_label {
                        Some(label) => format!("{label} ▾"),
                        None => "选择连接 ▾".to_string(),
                    })
                    .dropdown_menu(move |menu, _window, _cx| {
                        let mut menu = menu;
                        if sources.is_empty() {
                            return menu
                                .item(PopupMenuItem::new("（暂无数据源）").disabled(true));
                        }
                        for candidate in sources.iter() {
                            let source = candidate.clone();
                            let selected = selected.clone();
                            let catalog_input = catalog_input.clone();
                            let schema_input = schema_input.clone();
                            menu = menu.item(
                                PopupMenuItem::new(candidate.label.clone()).on_click(
                                    move |_, window, app| {
                                        // 选中连接：记下 conn_id，并把连接记录的库 / schema 预填进输入框
                                        *selected.borrow_mut() = Some(source.conn_id.clone());
                                        let catalog = source.catalog.clone();
                                        let schema = source.schema.clone();
                                        catalog_input.update(app, |s, cx| {
                                            s.set_value(catalog, window, cx)
                                        });
                                        schema_input.update(app, |s, cx| {
                                            s.set_value(schema, window, cx)
                                        });
                                    },
                                ),
                            );
                        }
                        menu
                    })
            });
            body = body.child(conn_row);
            body = body
                .child(form_line(theme, "数据库 / catalog", &catalog))
                .child(form_line(theme, "schema", &schema))
                .child(form_line(theme, "表名", &table));

            let panel_ok = panel.clone();
            let selected_ok = selected.clone();
            dialog.title("导入源库结构").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("mock-import-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(move |_, window, app| {
                                window.close_dialog(app);
                            }),
                    )
                    .child(
                        Button::new("mock-import-ok")
                            .with_variant(ButtonVariant::Primary)
                            .label("导入")
                            .on_click(move |_, window, app| {
                                panel_ok.update(app, |panel, cx| {
                                    let request = SchemaRequest {
                                        conn_id: selected_ok
                                            .borrow()
                                            .clone()
                                            .unwrap_or_default(),
                                        catalog: panel
                                            .import_catalog
                                            .as_ref()
                                            .map(|i| i.read(cx).value().to_string())
                                            .unwrap_or_default(),
                                        schema: panel
                                            .import_schema
                                            .as_ref()
                                            .map(|i| i.read(cx).value().to_string())
                                            .unwrap_or_default(),
                                        table: panel
                                            .import_table
                                            .as_ref()
                                            .map(|i| i.read(cx).value().to_string())
                                            .unwrap_or_default(),
                                    };
                                    if request.conn_id.is_empty() {
                                        panel.fail("请选择连接", cx);
                                        return;
                                    }
                                    if request.table.trim().is_empty() {
                                        panel.fail("请填写表名", cx);
                                        return;
                                    }
                                    panel.preset_from_source(request, cx);
                                });
                                window.close_dialog(app);
                            }),
                    ),
            )
        });
    }
}

/// 表单行（标签 + 输入）。
fn form_line(theme: &gpui_kit::component::Theme, label: &str, input: &Entity<InputState>) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .w(rems(PARAM_INPUT_WIDTH))
                .flex_none()
                .child(Input::new(input)),
        )
}

/// 复杂参数的一行：标签 + 多行文本（全宽）+ 格式提示。
///
/// 多行不能用 `form_line` 的「标签 | 定宽输入」横排：值集合一行一个，横排窄框放不下。
fn complex_form_line(
    theme: &gpui_kit::component::Theme,
    label: &str,
    hint: &str,
    state: &Entity<TextareaState>,
) -> Div {
    div()
        .v_flex()
        .gap_1()
        .w_full()
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(label.to_string()),
        )
        .child(Textarea::new(state).h(rems(COMPLEX_INPUT_HEIGHT)))
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(hint.to_string()),
        )
}

impl Render for MockPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let target = self.render_target(window, cx);
        let actions = self.render_actions(cx);
        // 右 Dock 内容区自身可滚动（Dock 的 `#tab-content` 不产生滚动，见布局规格）
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .bg(cx.theme().colors.background)
            .child(
                div()
                    .id("mock-panel-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg)
                                    .child("Mock 数据生成"),
                            )
                            .child(target)
                            .child(actions)
                            .child(self.render_templates(cx))
                            .child(self.render_history(cx)),
                    ),
            )
    }
}

// ==================== 详情视图（中央 tab：字段 + 预览） ====================

/// 对话框里的一行参数（标量：单行输入；集合 / 加权：多行文本 + 格式提示）。
enum ParamRowView {
    Scalar {
        label: String,
        state: Entity<InputState>,
    },
    Complex {
        label: String,
        hint: &'static str,
        state: Entity<TextareaState>,
    },
}

/// 参数输入控件（标量走单行 `Input`；集合 / 加权走多行 `Textarea`）。
enum ParamWidget {
    /// 标量参数：单行输入
    Scalar(Entity<InputState>),
    /// 集合 / 加权选项：多行文本（一行一项，格式见 [`parse_complex_param`]）
    Complex(Entity<TextareaState>),
}

/// 生成器参数的输入控件（含订阅句柄：丢掉订阅会立即失效）。
struct ParamInput {
    key: String,
    widget: ParamWidget,
    _sub: Subscription,
}

/// 列编辑对话框的工作副本（「应用」才写回草稿）。
struct ColumnDraft {
    /// 既有列的 id
    id: u64,
    /// 列名输入
    name_input: Entity<InputState>,
    def: ColumnDef,
    confidence: String,
    sample_value: String,
    params: Vec<ParamInput>,
    null_input: Entity<InputState>,
    type_label: String,
    /// 复杂参数解析失败的原因（键 + 文案）：只在对话框里就地提示，不写回生成器
    param_error: Option<(String, String)>,
}

/// 中央详情 tab 的**身份**：一张结果表一个 tab，另有草稿 tab（列定义可编辑那张）。
///
/// 为何用名字而不是下标做身份：tab 是长命的东西，而 `results` 每次生成都会重建
/// （下标会指向别的表）；名字才是用户看到的那张表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailTarget {
    /// 草稿 tab：单表态的列定义（可编辑）+ 草稿目标表的预览
    Draft,
    /// 结果表 tab：该表的列（只读）与预览
    Table(String),
}

impl DetailTarget {
    /// 宿主登记 tab 用的键（同一个键只开一个 tab）。
    pub fn key(&self) -> String {
        match self {
            Self::Draft => "draft".to_string(),
            Self::Table(name) => format!("table:{name}"),
        }
    }

    /// 日志 / 测试用的短名。
    pub fn label(&self) -> String {
        match self {
            Self::Draft => "草稿".to_string(),
            Self::Table(name) => name.clone(),
        }
    }
}

/// Mock 详情（中央编辑区 tab）：字段清单（编辑走对话框）+ 预览表格。
///
/// 状态单一权威：持有配置面板实体，字段与预览都从它读取；编辑动作写回面板。
/// **一个 tab 对应一个目标**（草稿或某张结果表，见 [`DetailTarget`]）：tab 被激活就等于
/// 把那张表设为「当前表」，出口（落库 / 追加 / 导出）跟着它走。
pub struct MockDetailView {
    panel: Entity<MockPanel>,
    /// 这个 tab 管的是哪张表 / 草稿
    target: DetailTarget,
    /// 列编辑对话框的工作副本
    draft: Option<ColumnDraft>,
    focus_handle: FocusHandle,
    /// 所在 Dock tab 组（`Panel::on_added_to` 注入；重复点「查看详情」时用于聚焦自身）
    group: Option<WeakEntity<TabGroup>>,
}

/// 把详情 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// **必须在实体更新之外调用**：`TabGroup` 切 tab 时会回读面板实体（`panel_name` /
/// `focus_handle`），若在 `update_entity(MockDetailView)` 的闭包里调用，gpui 会以
/// `cannot read … while it is already being updated` panic（double lease）。
/// 签名故意只收 `&Entity<Self>` + `&mut App`，从类型上就排除了「在自身 update 里调用」。
pub fn focus_detail_tab(detail: &Entity<MockDetailView>, window: &mut Window, cx: &mut App) {
    let (focus_handle, group) = {
        let view = detail.read(cx);
        (view.focus_handle.clone(), view.group.clone())
    };
    window.focus(&focus_handle, cx);
    let Some(group) = group else {
        // 未加入 Dock（宿主未装配 / 单测直接构造）：静默返回
        return;
    };
    let me = detail.entity_id();
    group
        .update(cx, |group, cx| {
            // 用 `view().entity_id()` 而不是 `panel_name(cx)` 找自己：后者在遍历中会读取
            // 组内每个面板实体（含自身），既多余又容易再踩租借冲突。
            let ix = group
                .panels()
                .iter()
                .position(|p| p.view().entity_id() == me);
            if let Some(ix) = ix {
                group.select_tab(ix, window, cx);
            }
        })
        .ok();
}

impl MockDetailView {
    /// 创建详情视图（持有配置面板实体，状态单一权威）。
    pub fn new(panel: Entity<MockPanel>, target: DetailTarget, cx: &mut Context<Self>) -> Self {
        // 状态变化即重绘：字段/预览跟随配置面板
        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
        Self {
            panel,
            target,
            draft: None,
            focus_handle: cx.focus_handle(),
            group: None,
        }
    }

    /// 这个 tab 管的目标（宿主建 tab / 测试用）。
    pub fn target(&self) -> &DetailTarget {
        &self.target
    }

    /// tab 标题：结果表写 `表名（N 行）`，草稿写目标表名（与旧行为一致）。
    ///
    /// 行数实时取自面板结果，所以重新生成 / 「编辑表」改完行数后标题会自己更新。
    pub fn tab_label(&self, cx: &App) -> String {
        let panel = self.panel.read(cx);
        match &self.target {
            DetailTarget::Draft => format!("Mock · {}", panel.draft().table_name),
            DetailTarget::Table(name) => {
                match panel
                    .results()
                    .iter()
                    .find(|info| &info.table_name == name)
                {
                    Some(info) => format!(
                        "Mock · {name}（{} 行）",
                        with_thousands(u64::from(info.row_count))
                    ),
                    None => format!("Mock · {name}"),
                }
            }
        }
    }

    /// 配置面板实体（宿主登记 / 测试用）。
    pub fn panel(&self) -> &Entity<MockPanel> {
        &self.panel
    }

    /// 列编辑对话框的「应用」：把工作副本合成为列规格（读全部输入框现值）。
    fn collect_column_edit(&mut self, cx: &App) -> Option<MockColumnSpec> {
        let draft = self.draft.take()?;
        let name = draft.name_input.read(cx).value().trim().to_string();
        let ratio = parse_percent_ratio(&draft.null_input.read(cx).value())
            .unwrap_or(draft.def.nullable_ratio);
        let mut def = draft.def.clone();
        def.name = name;
        def.nullable_ratio = ratio;
        Some(MockColumnSpec {
            id: draft.id,
            def,
            confidence: draft.confidence.clone(),
            sample_value: draft.sample_value.clone(),
        })
    }

    /// 打开「列编辑」对话框（列名 / 类型 / 参数 / 空值率 / 唯一 / 恢复智能默认）。
    pub fn open_column_dialog(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(column) = self
            .panel
            .read(cx)
            .draft()
            .columns
            .iter()
            .find(|c| c.id == id)
            .cloned()
        else {
            return;
        };

        let null_initial = format!("{}", (column.def.nullable_ratio * 100.0).round() as i64);
        let null_input = cx.new(|cx| InputState::new(window, cx).placeholder("0~100"));
        null_input.update(cx, |s, cx| s.set_value(null_initial, window, cx));
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("列名"));
        name_input.update(cx, |s, cx| s.set_value(column.def.name.clone(), window, cx));

        self.draft = Some(ColumnDraft {
            id,
            name_input,
            type_label: column_type_label(&column.def.data_type),
            def: column.def.clone(),
            confidence: column.confidence.clone(),
            sample_value: column.sample_value.clone(),
            params: Vec::new(),
            null_input,
            param_error: None,
        });
        // 参数输入行按当前生成器建立（生成器身份变化后由 `rebuild_params` 重建）
        self.rebuild_params(window, cx);

        let view = cx.entity();
        let panel = self.panel.clone();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let Some(draft) = view.read(cx).draft.as_ref() else {
                return dialog.title("列编辑");
            };
            let name_state = draft.name_input.clone();
            let null_state = draft.null_input.clone();
            let type_label = draft.type_label.clone();
            let unique = draft.def.unique;
            // 参数行：标量单行 / 集合多行，标签与提示都从目录派生（不手写清单）
            let param_rows: Vec<ParamRowView> = draft
                .params
                .iter()
                .map(|p| {
                    let label = generator_catalog::spec_of(&draft.def.generator)
                        .params
                        .iter()
                        .find(|f| f.key == p.key)
                        .map(|f| f.label.to_string())
                        .unwrap_or_else(|| p.key.clone());
                    match &p.widget {
                        ParamWidget::Scalar(state) => ParamRowView::Scalar {
                            label,
                            state: state.clone(),
                        },
                        ParamWidget::Complex(state) => ParamRowView::Complex {
                            label,
                            hint: complex_param_hint(&p.key),
                            state: state.clone(),
                        },
                    }
                })
                .collect();
            let param_error = draft.param_error.clone();
            let generator_label = generator_catalog::spec_of(&draft.def.generator).label;

            let mut body = div()
                .v_flex()
                .gap_2()
                .child(form_line(theme, "列名", &name_state));
            // 类型下拉
            body = body.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("类型"),
                    )
                    .child({
                        let view = view.clone();
                        Button::new("mock-col-type")
                            .ghost()
                            .label(format!("{type_label} ▾"))
                            .dropdown_menu(move |menu, _window, _cx| {
                                let mut menu = menu;
                                for candidate in COLUMN_TYPES {
                                    let label = column_type_label(&candidate);
                                    let is_current = label == type_label;
                                    let view = view.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(label).checked(is_current).on_click(
                                            move |_, _, app| {
                                                let chosen = candidate.clone();
                                                view.update(app, |view, cx| {
                                                    if let Some(draft) = view.draft.as_mut() {
                                                        draft.type_label =
                                                            column_type_label(&chosen);
                                                        draft.def.data_type = chosen;
                                                        draft.confidence = "manual".to_string();
                                                    }
                                                    cx.notify();
                                                });
                                            },
                                        ),
                                    );
                                }
                                menu
                            })
                    }),
            );
            // 生成器（只读展示；切换在字段行的菜单里，避免对话框内参数行与生成器不一致）
            body = body.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("生成器"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground)
                            .child(generator_label),
                    ),
            );
            for row in param_rows {
                body = body.child(match row {
                    ParamRowView::Scalar { label, state } => form_line(theme, &label, &state),
                    ParamRowView::Complex { label, hint, state } => {
                        complex_form_line(theme, &label, hint, &state)
                    }
                });
            }
            if let Some((_, message)) = param_error {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.danger)
                        .child(message),
                );
            }
            body = body.child(form_line(theme, "空值率（%）", &null_state));
            body = body.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("唯一值"),
                    )
                    .child({
                        let view = view.clone();
                        Switch::new("mock-col-unique").checked(unique).on_click(
                            move |checked, _window, app| {
                                let checked = *checked;
                                view.update(app, |view, cx| {
                                    if let Some(draft) = view.draft.as_mut() {
                                        draft.def.unique = checked;
                                    }
                                    cx.notify();
                                });
                            },
                        )
                    }),
            );
            body = body.child({
                let view = view.clone();
                Button::new("mock-col-reset")
                    .secondary()
                    .label("恢复智能默认")
                    .on_click(move |_, window, app| {
                        view.update(app, |view, cx| {
                            if let Some(draft) = view.draft.as_mut() {
                                let mapped =
                                    ColumnMapper::infer(&draft.def.name, &draft.def.data_type);
                                draft.def.generator = mapped.generator;
                                draft.def.nullable_ratio = 0.0;
                                draft.def.unique = false;
                                draft.confidence = mapped.confidence;
                                draft.sample_value = mapped.sample_value;
                            }
                            // 生成器身份可能变了：参数行按新生成器重建（事件路径，不能在 render 做）
                            cx.defer_in(window, |view, window, cx| {
                                view.rebuild_params(window, cx);
                            });
                            cx.notify();
                        });
                    })
            });

            let view_apply = view.clone();
            let view_cancel = view.clone();
            let panel_apply = panel.clone();
            dialog.title("列编辑").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("mock-col-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(move |_, window, app| {
                                view_cancel.update(app, |view, _cx| view.draft = None);
                                window.close_dialog(app);
                            }),
                    )
                    .child(
                        Button::new("mock-col-apply")
                            .with_variant(ButtonVariant::Primary)
                            .label("应用")
                            .on_click(move |_, window, app| {
                                let edited =
                                    view_apply.update(app, |view, cx| view.collect_column_edit(cx));
                                if let Some(edited) = edited {
                                    panel_apply
                                        .update(app, |panel, cx| panel.apply_column(edited, cx));
                                }
                                window.close_dialog(app);
                            }),
                    ),
            )
        });
    }

    /// 按当前生成器重建参数输入行（保留同名参数的输入框与订阅）。
    fn rebuild_params(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let generator = draft.def.generator.clone();
        let fields: Vec<ParamField> = generator_catalog::spec_of(&generator)
            .params
            .iter()
            .copied()
            .collect();
        let mut previous: HashMap<String, ParamInput> =
            draft.params.drain(..).map(|p| (p.key.clone(), p)).collect();
        let mut next = Vec::with_capacity(fields.len());
        for field in fields {
            if let Some(item) = previous.remove(field.key) {
                next.push(item);
                continue;
            }
            let key = field.key.to_string();
            let (widget, _sub) = match field.kind {
                // 集合 / 加权选项：多行文本，改一次解析一次（解析失败保留上一个有效值）
                ParamKind::Complex => {
                    let state = cx.new(|cx| TextareaState::new(window, cx));
                    let initial = complex_param_text(&generator, field.key);
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |view, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            view.commit_complex_param(&key_for_closure, &text, cx);
                        },
                    );
                    (ParamWidget::Complex(state), _sub)
                }
                kind => {
                    let initial = param_text(&generator, field.key);
                    let state = cx.new(|cx| InputState::new(window, cx));
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |view, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            let Some(draft) = view.draft.as_mut() else {
                                return;
                            };
                            if let Some(patched) =
                                patch_param(&draft.def.generator, &key_for_closure, &text, kind)
                            {
                                draft.def.generator = patched;
                                draft.confidence = "manual".to_string();
                                draft.sample_value.clear();
                                cx.notify();
                            }
                        },
                    );
                    (ParamWidget::Scalar(state), _sub)
                }
            };
            next.push(ParamInput { key, widget, _sub });
        }
        if let Some(draft) = self.draft.as_mut() {
            draft.params = next;
            // 参数行重建 = 生成器身份变了（换生成器 / 恢复智能默认）：旧参数的报错不再适用
            draft.param_error = None;
        }
        cx.notify();
    }

    /// 复杂参数（集合 / 加权选项）编辑：解析成功就写回生成器；失败则**保留上一个有效值**
    /// 并记下原因（只在对话框里就地提示，不把无效内容写进生成器）。
    fn commit_complex_param(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        match parse_complex_param(key, text) {
            Ok(value) => {
                if let Some(patched) = patch_param_value(&draft.def.generator, key, value) {
                    draft.def.generator = patched;
                    draft.confidence = "manual".to_string();
                    draft.sample_value.clear();
                }
                draft.param_error = None;
            }
            Err(reason) => draft.param_error = Some((key.to_string(), reason)),
        }
        cx.notify();
    }

    /// 字段清单（列 + 生成器 + 参数摘要 + 编辑 / 智能 / 删除）。
    fn render_fields(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let success = cx.theme().colors.success;
        let info = cx.theme().colors.info;

        let columns = self.panel.read(cx).draft().columns.clone();
        if columns.is_empty() {
            return div()
                .text_xs()
                .text_color(muted)
                .child("暂无列：在右 Dock「Mock 数据生成」面板点「导入结构」或「＋ 加列」。")
                .into_any_element();
        }

        let mut list = div().v_flex().gap_2().w_full();
        for column in columns {
            let spec = generator_catalog::spec_of(&column.def.generator);
            let summary = summarize_params(&column.def.generator);
            let badge = match column.confidence.as_str() {
                "high" => "high",
                "manual" => "manual",
                _ => "low",
            };
            let badge_color = match badge {
                "high" => success,
                "manual" => info,
                _ => muted,
            };
            let id = column.id;
            let label = spec.label;
            let unique = if column.def.unique { "唯一 · " } else { "" };
            let null_percent = (column.def.nullable_ratio * 100.0).round() as i64;
            let detail = if summary.is_empty() {
                column.sample_value.clone()
            } else {
                summary
            };
            let panel = self.panel.clone();
            list = list.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-field-{id}"
                    ))))
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .p_2()
                    .border_1()
                    .border_color(border)
                    .rounded(cx.theme().radius)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(column.def.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(column_type_label(&column.def.data_type)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(badge_color)
                                    .child(badge),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child({
                                let id = id;
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-gen-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .label(format!("{label} ▾"))
                                .dropdown_menu(generator_menu(self.panel.clone(), id))
                            })
                            .child({
                                let entity = cx.entity();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-edit-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .label("编辑")
                                .on_click(
                                    move |_, window, app| {
                                        entity.update(app, |view, cx| {
                                            view.open_column_dialog(id, window, cx);
                                        });
                                    },
                                )
                            })
                            .child({
                                let panel = panel.clone();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-smart-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .label("智能")
                                .on_click(move |_, _, app| {
                                    panel.update(app, |panel, cx| {
                                        panel.reset_column_mapping(id, cx)
                                    });
                                })
                            })
                            .child({
                                let panel = panel.clone();
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "mock-del-{id}"
                                ))))
                                .ghost()
                                .xsmall()
                                .label("删除")
                                .on_click(move |_, _, app| {
                                    panel.update(app, |panel, cx| panel.remove_column(id, cx));
                                })
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(muted)
                                    .text_ellipsis()
                                    .child(format!("{unique}空值 {null_percent}% · {detail}")),
                            ),
                    ),
            );
        }

        div()
            .id("mock-field-list")
            .v_flex()
            .gap_2()
            .w_full()
            .max_h(rems(FIELD_LIST_MAX_HEIGHT))
            .overflow_y_scrollbar()
            .child(list)
            .into_any_element()
    }
    /// 结果表 tab 的列清单（**只读**）：列来自这次生成（`MockGenInfo.columns`）。
    ///
    /// 为何不给编辑入口：这张表的列是「这次生成」的产物（模板或草稿的结果），
    /// 就地改列会让「结果」与「产出它的配置」分叉——要改列回草稿 tab 改，再重新生成。
    fn render_result_columns(
        &self,
        info: Option<&MockGenInfo>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let radius = cx.theme().radius;

        let mut block = div().v_flex().gap_1().w_full();
        let Some(info) = info else {
            return block.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("这一轮没有这张表的列信息（重新生成后再看）。"),
            );
        };
        block = block.child(div().text_xs().text_color(muted).child(format!(
            "列（{}）· 只读：它们是这次生成的产物；要改列请回草稿 tab 改完重新生成",
            info.columns.len()
        )));
        for def in info.columns.iter() {
            let mut meta: Vec<String> = Vec::new();
            if def.unique {
                meta.push("唯一".to_string());
            }
            if def.nullable_ratio > 0.0 {
                meta.push(format!(
                    "空值 {}%",
                    (def.nullable_ratio * 100.0).round() as i64
                ));
            }
            let reference = def
                .dependency
                .as_ref()
                .filter(|dep| dep.is_foreign_key())
                .and_then(|dep| dep.ref_table.clone().zip(dep.ref_column.clone()))
                .map(|(table, column)| format!("引用 {table}.{column}"));
            block = block.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(border)
                    .rounded(radius)
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(def.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(column_type_label(&def.data_type)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(summarize_params(&def.generator)),
                    )
                    .children(reference.map(|text| {
                        div().flex_none().text_xs().text_color(muted).child(text)
                    }))
                    .children((!meta.is_empty()).then(|| {
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(meta.join(" · "))
                    })),
            );
        }
        block
    }
}

impl Render for MockDetailView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let success = cx.theme().colors.success;
        let danger = cx.theme().colors.danger;
        // 一次取齐渲染要用的状态（读租约不能跨到 `cx` 的独占使用处）
        let (draft, generated, outcome, error, landed, scenario_source, progress) = {
            let panel = self.panel.read(cx);
            // 本 tab 看哪张表：草稿 tab 看草稿目标表，结果表 tab 看它自己
            // （与面板的「当前表」无关——切 tab 就能对照两张表的预览）
            let table = match &self.target {
                DetailTarget::Draft => panel.draft().table_name.clone(),
                DetailTarget::Table(name) => name.clone(),
            };
            (
                panel.draft().clone(),
                panel
                    .results()
                    .iter()
                    .find(|info| info.table_name == table)
                    .cloned(),
                panel.outcome().map(|s| s.to_string()),
                panel.error().map(|s| s.to_string()),
                panel.landed().map(|s| s.to_string()),
                panel.scenario_source().map(|s| s.to_string()),
                panel.job_progress(),
            )
        };

        let is_draft = matches!(self.target, DetailTarget::Draft);
        // 生成按钮只属于草稿 tab：结果表 tab 上摆一个「生成」会让人以为能只重跑这一张
        let generate = is_draft.then(|| {
            let entity = self.panel.clone();
            let running = self.panel.read(cx).is_running();
            let generating = self.panel.read(cx).is_generating();
            let mut button = Button::new("mock-detail-generate")
                .primary()
                .xsmall()
                .label(if generating { "生成中…" } else { "生成" });
            if !running {
                button = button.on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.run_generate(cx));
                });
            }
            button
        });
        // 摘要行：草稿 tab 说草稿（含进行中的阶段），结果表 tab 说这张表自己
        let summary = match &self.target {
            DetailTarget::Draft => {
                let seed = match draft.options.seed {
                    Some(seed) => seed.to_string(),
                    None => "随机".to_string(),
                };
                let base = format!(
                    "字段（{}）· 目标表 {} · {} 行 · 种子 {seed} · {}",
                    draft.columns.len(),
                    draft.table_name,
                    with_thousands(draft.options.rows as u64),
                    locale_label(&draft.options.locale)
                );
                // 任务进行中：摘要行尾追加阶段（面板已有进度条与取消，这里只做一行文字同步）
                match progress.as_ref() {
                    Some(progress) if progress.phase.is_quantified() => {
                        format!("{base} · {} {:.0}%", progress.phase.label(), progress.percent())
                    }
                    Some(progress) => format!("{base} · {}", progress.phase.label()),
                    None => base,
                }
            }
            DetailTarget::Table(name) => match generated.as_ref() {
                Some(info) => {
                    let rows = with_thousands(u64::from(info.row_count));
                    match scenario_source.as_deref() {
                        Some(source) => format!("表 {name} · {rows} 行 · 来自场景模板「{source}」"),
                        None => format!("表 {name} · {rows} 行 · 本次生成"),
                    }
                }
                None => format!("表 {name} · 这一轮没有它的结果（重新生成后再看）"),
            },
        };
        let header = div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .text_ellipsis()
                    .child(summary),
            )
            .children(generate);

        let mut body = div()
            .v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .p_3()
            .child(header);

        match &self.target {
            DetailTarget::Draft => {
                if draft.columns.is_empty() {
                    body = body.child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("暂无列：在右 Dock 面板导入源库结构或手工加列。"),
                    );
                } else {
                    body = body.child(self.render_fields(cx));
                }
            }
            // 结果表 tab：列只读（它们来自这次生成）；要改列就回草稿 tab 改，再重新生成
            DetailTarget::Table(_) => {
                body = body.child(self.render_result_columns(generated.as_ref(), cx));
            }
        }

        let preview_title = match generated.as_ref() {
            Some(info) => format!(
                "预览（前 {} 行）· {} · 临时表 {} · 本次 {} 行 · {} ms",
                PREVIEW_ROWS.min(info.preview.rows.len()),
                info.table_name,
                info.temp_table_name,
                with_thousands(info.row_count as u64),
                info.elapsed_ms
            ),
            None if is_draft => {
                format!("预览（前 {PREVIEW_ROWS} 行）· 尚无结果——点右上「生成」")
            }
            None => format!("预览（前 {PREVIEW_ROWS} 行）· 这一轮没有这张表的结果"),
        };
        body = body.child(div().text_xs().text_color(muted).child(preview_title));

        // 场景表 tab：说清它的列与预览都是这张表自己的（看别的表切 tab，不再有下拉）
        if matches!(self.target, DetailTarget::Table(_)) {
            if let Some(name) = scenario_source.as_deref() {
                body = body.child(div().text_xs().text_color(muted).child(format!(
                    "来自场景模板「{name}」：这张表的列与预览都属于它自己；看别的表切 tab（或在右 Dock 结果区点一行）"
                )));
            }
        }

        let preview = generated.as_ref().map(|info| info.preview.clone());
        match preview {
            Some(preview) if !preview.rows.is_empty() => {
                body = body.child(render_preview_table(&preview, cx));
            }
            _ => {
                body = body.child(
                    div()
                        .flex_1()
                        .min_h(rems(PREVIEW_MIN_HEIGHT))
                        .min_w_0()
                        .text_xs()
                        .text_color(muted)
                        .child("（暂无预览）"),
                );
            }
        }

        if let Some(text) = outcome {
            body = body.child(div().text_xs().text_color(success).child(text));
        }
        if let Some(err) = error {
            body = body.child(div().text_xs().text_color(danger).child(err));
        }
        if let Some(table) = landed {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(success)
                    .child(format!("落库目标：{table}")),
            );
        }
        body
    }
}

/// 预览表格：表头 + 前 N 行（固定列宽 + 横向滚动，纵向占满剩余高度）。
fn render_preview_table(
    preview: &MockPreview,
    cx: &mut Context<MockDetailView>,
) -> impl IntoElement {
    let fg = cx.theme().colors.foreground;
    let muted = cx.theme().colors.muted_foreground;
    let border = cx.theme().colors.border;

    let mut header = div()
        .h_flex()
        .w_full()
        .border_b_1()
        .border_color(border)
        .child(
            div()
                .w(rems(PREVIEW_CELL_WIDTH))
                .flex_none()
                .px_2()
                .py_1()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child("#"),
        );
    for name in preview.columns.iter() {
        header = header.child(
            div()
                .w(rems(PREVIEW_CELL_WIDTH))
                .flex_none()
                .px_2()
                .py_1()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .text_ellipsis()
                .child(name.clone()),
        );
    }

    let mut rows = div().v_flex().w_full();
    for (index, row) in preview.rows.iter().take(PREVIEW_ROWS).enumerate() {
        let mut line = div()
            .h_flex()
            .w_full()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .w(rems(PREVIEW_CELL_WIDTH))
                    .flex_none()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .child((index + 1).to_string()),
            );
        for column_index in 0..preview.columns.len() {
            line = line.child(
                div()
                    .w(rems(PREVIEW_CELL_WIDTH))
                    .flex_none()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(row.get(column_index).cloned().unwrap_or_default()),
            );
        }
        rows = rows.child(line);
    }

    div()
        .id("mock-preview-scroll")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .border_1()
        .border_color(border)
        .overflow_x_scrollbar()
        .child(div().v_flex().w_full().min_h_0().child(header).child(rows))
}

// ==================== 中央 tab 的面板协议 ====================

impl EventEmitter<PanelEvent> for MockDetailView {}

impl Focusable for MockDetailView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for MockDetailView {
    fn panel_name(&self) -> &'static str {
        "mock_detail"
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }

    /// tab 被激活 = 这张表成为「当前表」：出口（落库 / 追加 / 导出）都作用于它。
    ///
    /// 这就是「切 tab 就是切表」的落地点：用户不必再去面板里选一次。
    /// 草稿 tab 不动当前表（草稿不是结果表）。
    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if !active {
            return;
        }
        let DetailTarget::Table(name) = self.target.clone() else {
            return;
        };
        self.panel
            .update(cx, |panel, cx| panel.focus_table(&name, cx));
    }
}

impl ComponentPanel for MockDetailView {
    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(self.tab_label(cx).into())
    }

    fn title(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self.tab_label(cx);
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child(label)
    }
}

#[cfg(test)]
mod tests;
