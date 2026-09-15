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
//! | 持久化为分析库表 | 在分析库**新建**表（已存在则报错，引导改用「追加」） |
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
//!   生成 / 出口按钮组 / 结果与错误；
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
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::*;

use crate::generator_catalog::{self, GeneratorCategory, GeneratorSpec, ParamField, ParamKind};
use crate::models::{ColumnDataType, ColumnDef, GeneratorConfig, Locale, MockExportFormat};
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockGenInfo {
    /// 内存临时表名（`temp_mock_*`）
    pub temp_table_name: String,
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
/// 分两组：**生成类**（`Generate` / `AppendTo`）自己产出 `MockGenInfo`；
/// **出口类**（`Persist` / `Export` / `Scratchpad`）消费上一次生成的结果，
///  поэтому把 `MockGenInfo` 带在身上——工作线程要靠它找到内存临时表，
/// 而视图侧不能把「最近一次结果」另存一份隐式状态（两处状态必然漂移）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockJobKind {
    /// 只生成到内存临时表（**不写库**）
    Generate,
    /// 生成后追加到既有分析表（自增起点按表内行数接续）
    AppendTo(String),
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
        matches!(self, Self::Generate | Self::AppendTo(_))
    }

    /// 任务开始时的阶段（进度条形态与文案据此切换；进行中时以宿主上报的阶段为准）。
    pub fn phase(&self) -> MockJobPhase {
        match self {
            Self::Generate | Self::AppendTo(_) => MockJobPhase::Generating,
            Self::Persist(_) => MockJobPhase::Writing,
            Self::Export { .. } | Self::Scratchpad { .. } => MockJobPhase::Exporting,
        }
    }

    /// 本次任务处理的行数（文案用）：生成类按草稿设定，出口类按已生成结果。
    pub fn rows_total(&self, draft: &MockDraft) -> u32 {
        match self {
            Self::Generate | Self::AppendTo(_) => draft.options.rows,
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
            Self::Persist(info) => format!("写入分析库中…（{} 行）", info.row_count),
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
            Self::Writing => "写入分析库中",
            Self::Exporting => "写出文件中",
        }
    }
}

/// 后台任务进度快照。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MockJobProgress {
    /// 当前阶段
    pub phase: MockJobPhase,
    /// 已完成批次（仅生成阶段有意义）
    pub batches_done: usize,
    /// 总批次（首次回调前为 0，表示「尚未开始」）
    pub batches_total: usize,
    /// 本次任务的行数（文案用）
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
    /// 只读项目？（落库与写文件据此拒绝）
    fn read_only(&self) -> bool;
    /// 打开中央「Mock 数据」详情 tab（字段 + 预览）
    fn open_detail(&self, window: &mut Window, cx: &mut App);
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

/// 把生成器参数按 JSON 补丁写回配置（137 变体零手工构造，见 `generator_catalog`）。
pub(crate) fn patch_param(
    config: &GeneratorConfig,
    key: &str,
    text: &str,
    kind: ParamKind,
) -> Option<GeneratorConfig> {
    let mut value = serde_json::to_value(config).ok()?;
    let outer = value.as_object_mut()?;
    let payload = match outer.len() {
        1 => outer.values_mut().next()?.as_object_mut()?,
        _ => return None,
    };
    let slot = payload.get_mut(key)?;
    let next = match kind {
        ParamKind::Int => serde_json::Value::from(text.trim().parse::<i64>().ok()?),
        ParamKind::Float => serde_json::Value::from(text.trim().parse::<f64>().ok()?),
        ParamKind::Text => serde_json::Value::from(text.trim().to_string()),
        ParamKind::Bool => serde_json::Value::from(matches!(
            text.trim().to_lowercase().as_str(),
            "true" | "1" | "是" | "yes"
        )),
        ParamKind::Complex => return None,
    };
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
            ParamKind::Complex => continue,
            ParamKind::Bool => {
                if raw.as_bool().unwrap_or(false) {
                    "是"
                } else {
                    "否"
                }
                .to_string()
            }
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

/// 按类型给一个可用默认生成器（手工加列用）。
fn default_generator_for(data_type: &ColumnDataType) -> GeneratorConfig {
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
                PopupMenuItem::new(format!("搜索生成器…（{} 项）", generator_catalog::all_specs().len()))
                    .on_click(move |_, window, app| {
                        search.update(app, |panel, cx| {
                            panel.open_generator_search(id, window, cx)
                        });
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
                        .child(div().flex_none().text_xs().text_color(muted).child(spec.name))
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
    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(spec) = self.selected.and_then(|ix| self.hits.get(ix.row).copied()) else {
            return;
        };
        let name = spec.name;
        self.panel
            .update(cx, |panel, cx| panel.set_generator(self.column_id, name, cx));
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
    /// 最近一次生成结果
    generated: Option<MockGenInfo>,
    /// 最近一次成功落库的表名（出口反馈用）
    landed: Option<String>,
    outcome: Option<String>,
    error: Option<String>,
    /// 可导入结构的连接（事件路径加载）
    sources: Vec<SchemaSource>,
    /// 分析库既有表（追加候选，事件路径加载）
    existing_tables: Vec<String>,
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

impl MockPanel {
    /// 创建面板（宿主在 `cx.new` 中调用；构造不做 I/O）。
    pub fn new(host: Rc<dyn MockHost>, _cx: &mut Context<Self>) -> Self {
        Self {
            host,
            draft: MockDraft::default(),
            next_id: 1,
            generated: None,
            landed: None,
            outcome: None,
            error: None,
            sources: Vec::new(),
            existing_tables: Vec::new(),
            table_input: None,
            table_pending: None,
            rows_input: None,
            seed_input: None,
            import_conn: Rc::new(RefCell::new(None)),
            import_catalog: None,
            import_schema: None,
            import_table: None,
            job: None,
        }
    }

    // ==================== 只读访问器（宿主 / 详情 tab / 测试） ====================

    /// 当前草稿
    pub fn draft(&self) -> &MockDraft {
        &self.draft
    }

    /// 最近一次生成结果
    pub fn gen_info(&self) -> Option<&MockGenInfo> {
        self.generated.as_ref()
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

    /// 打开中央「Mock 数据」详情 tab（字段清单 + 预览）。
    pub fn open_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.host.open_detail(window, cx);
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
                self.generated = None;
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

    /// 出口：追加到分析库既有表（显式选择；后台任务：生成 + 追加在一次任务里完成）。
    pub fn append_table(&mut self, table: String, cx: &mut Context<Self>) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入分析库", cx);
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
        if let Err(e) = self.sync_inputs(cx) {
            self.fail(e, cx);
            return;
        }
        if self.draft.columns.is_empty() {
            self.fail("请先添加列：导入源库结构，或手工加列", cx);
            return;
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
            self.generated = None;
        }
        match result {
            Ok(MockJobDone::Generated(info)) => {
                self.landed = None;
                self.error = None;
                self.outcome = Some(format!(
                    "已生成 {} 行（耗时 {} ms）→ 临时表 {}",
                    info.row_count, info.elapsed_ms, info.temp_table_name
                ));
                self.generated = Some(info);
                self.host.notify(cx);
            }
            Ok(MockJobDone::Appended { table, total_rows }) => {
                self.landed = Some(table.clone());
                self.error = None;
                self.outcome = Some(format!("已追加到 {table}（表内共 {total_rows} 行）"));
                self.host.notify(cx);
            }
            Ok(MockJobDone::Persisted { table, rows }) => {
                self.landed = Some(table.clone());
                // 新表要能立刻作为「追加到既有表」的目标
                self.existing_tables = self.host.existing_tables();
                self.succeed(format!("已在分析库新建表 {table}（{rows} 行）"), cx);
            }
            Ok(MockJobDone::Exported { message }) => self.succeed(message, cx),
            Err(e) => {
                let cancelled = e.contains("取消");
                // 同名表已存在是落库失败的常见情形：刷新清单，引导到「追加到既有表」
                if !cancelled && matches!(kind, MockJobKind::Persist(_)) && e.contains("已存在") {
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

    /// 出口：持久化为分析库新表（后台任务：大行数落库同样会阻塞界面）。
    pub fn persist_table(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.generated.clone() else {
            self.fail("请先生成（预览确认后再落库）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写入分析库", cx);
            return;
        }
        // 目标表名在 `start_job` 的 `sync_inputs` 里从输入框取（任务内部据此命名新表）
        self.start_job(MockJobKind::Persist(info), cx);
    }

    /// 出口：导出文件（调用方已选好路径；后台任务）。
    pub fn export_file(
        &mut self,
        format: &MockExportFormat,
        path: String,
        cx: &mut Context<Self>,
    ) {
        let Some(info) = self.generated.clone() else {
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
        let Some(info) = self.generated.clone() else {
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
        self.generated = None;
        self.landed = None;
        cx.notify();
    }

    /// 删列。
    pub fn remove_column(&mut self, id: u64, cx: &mut Context<Self>) {
        self.draft.columns.retain(|c| c.id != id);
        self.generated = None;
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
            self.generated = None;
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
            dialog.title("搜索生成器").child(
                div()
                    .w_full()
                    .h(rems(SEARCH_LIST_HEIGHT))
                    .child(
                        List::new(&list)
                            .search_placeholder("按名称 / 中文标签 / 分类搜索（137 项）"),
                    ),
            )
        });
    }

    /// 应用列编辑（列名 / 类型 / 生成器 / 参数 / 空值率 / 唯一）——详情 tab 的「应用」。
    pub fn apply_column(&mut self, edited: MockColumnSpec, cx: &mut Context<Self>) {
        if let Some(slot) = self.draft.columns.iter_mut().find(|c| c.id == edited.id) {
            *slot = edited;
            self.generated = None;
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
            self.generated = None;
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
        self.generated.is_some()
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
        if let (Some(pending), Some(input)) = (self.table_pending.take(), self.table_input.clone()) {
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

        let muted = cx.theme().colors.muted_foreground;
        let detail = match progress.phase {
            MockJobPhase::Generating if progress.batches_total == 0 => "准备中…".to_string(),
            MockJobPhase::Generating => format!(
                "{} / {} 批（≈{} / {} 行）",
                progress.batches_done,
                progress.batches_total,
                progress.rows_done(),
                progress.rows_total
            ),
            phase => format!("{}…（{} 行）", phase.label(), progress.rows_total),
        };
        let cancel = cancellable.then(|| {
            let entity = cx.entity();
            let mut button = Button::new("mock-cancel-job")
                .secondary()
                .xsmall()
                .label(if cancel_requested { "正在取消…" } else { "取消" });
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

        // 任务进行中：进度条（组件，不手搓）+ 批次文案 + 取消
        let job_row = self.render_job_row(cx);

        // 出口：详情 tab + 落库 + 追加 + 草稿箱 + 另存为（任务进行中全部禁用：一次只能跑一个）
        let detail = {
            let entity = cx.entity();
            Button::new("mock-open-detail")
                .secondary()
                .label("查看详情（字段与预览）")
                .w_full()
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| panel.open_detail(window, cx));
                })
        };
        let persist = {
            let entity = cx.entity();
            Button::new("mock-persist")
                .secondary()
                .label("持久化为分析库表")
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
                        return menu.item(PopupMenuItem::new("（分析库暂无表）").disabled(true));
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
                        menu = menu.item(PopupMenuItem::new(label).on_click(
                            move |_, window, app| {
                                let format = format.clone();
                                let name = mock_file_name(
                                    &entity.read(app).draft.table_name,
                                    &format,
                                );
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
                            },
                        ));
                    }
                    menu
                })
        };

        let column_count = self.draft.columns.len();
        let generated = self.generated.clone();
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
            .child(generate)
            .child(job_row);

        if let Some(info) = generated.as_ref() {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(format!(
                        "临时表 {} · {} 行 · {} ms",
                        info.temp_table_name, info.row_count, info.elapsed_ms
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
            .child(div().v_flex().gap_1().w_full().child(detail).child(persist).child(append).child(scratchpad).child(export))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("数据只写入分析库与文件，不回传源库（M7）"),
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
        panel
    }

    /// 打开「导入源库结构」对话框。
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
                            .child(actions),
                    ),
            )
    }
}

// ==================== 详情视图（中央 tab：字段 + 预览） ====================

/// 生成器参数的输入框（含订阅句柄：丢掉订阅会立即失效）。
struct ParamInput {
    key: String,
    state: Entity<InputState>,
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
}

/// Mock 详情（中央编辑区 tab）：字段清单（编辑走对话框）+ 预览表格。
///
/// 状态单一权威：持有配置面板实体，字段与预览都从它读取；编辑动作写回面板。
pub struct MockDetailView {
    panel: Entity<MockPanel>,
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
    pub fn new(panel: Entity<MockPanel>, cx: &mut Context<Self>) -> Self {
        // 状态变化即重绘：字段/预览跟随配置面板
        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
        Self {
            panel,
            draft: None,
            focus_handle: cx.focus_handle(),
            group: None,
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
            let param_rows: Vec<(String, String, Entity<InputState>)> = draft
                .params
                .iter()
                .map(|p| {
                    let label = generator_catalog::spec_of(&draft.def.generator)
                        .params
                        .iter()
                        .find(|f| f.key == p.key)
                        .map(|f| f.label.to_string())
                        .unwrap_or_else(|| p.key.clone());
                    (p.key.clone(), label, p.state.clone())
                })
                .collect();
            let complex_notes: Vec<&'static str> = generator_catalog::spec_of(&draft.def.generator)
                .params
                .iter()
                .filter(|f| f.kind == ParamKind::Complex)
                .map(|f| f.label)
                .collect();
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
                                                        draft.confidence =
                                                            "manual".to_string();
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
            for (_key, label, state) in param_rows {
                body = body.child(form_line(theme, &label, &state));
            }
            for note in complex_notes {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("{note}：复杂参数暂不内联编辑")),
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
                        Switch::new("mock-col-unique")
                            .checked(unique)
                            .on_click(move |checked, _window, app| {
                                let checked = *checked;
                                view.update(app, |view, cx| {
                                    if let Some(draft) = view.draft.as_mut() {
                                        draft.def.unique = checked;
                                    }
                                    cx.notify();
                                });
                            })
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
            .filter(|f| f.kind != ParamKind::Complex)
            .collect();
        let mut previous: HashMap<String, ParamInput> = draft
            .params
            .drain(..)
            .map(|p| (p.key.clone(), p))
            .collect();
        let mut next = Vec::with_capacity(fields.len());
        for field in fields {
            if let Some(item) = previous.remove(field.key) {
                next.push(item);
                continue;
            }
            let initial = param_text(&generator, field.key);
            let state = cx.new(|cx| InputState::new(window, cx));
            state.update(cx, |s, cx| s.set_value(initial, window, cx));
            let key = field.key.to_string();
            let key_for_closure = key.clone();
            let kind = field.kind;
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
            next.push(ParamInput { key, state, _sub });
        }
        if let Some(draft) = self.draft.as_mut() {
            draft.params = next;
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
                                .on_click(move |_, window, app| {
                                    entity.update(app, |view, cx| {
                                        view.open_column_dialog(id, window, cx);
                                    });
                                })
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
                                    .child(format!(
                                        "{unique}空值 {null_percent}% · {detail}"
                                    )),
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
}

impl Render for MockDetailView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let success = cx.theme().colors.success;
        let danger = cx.theme().colors.danger;
        let draft = self.panel.read(cx).draft().clone();
        let generated = self.panel.read(cx).gen_info().cloned();
        let outcome = self.panel.read(cx).outcome().map(|s| s.to_string());
        let error = self.panel.read(cx).error().map(|s| s.to_string());
        let landed = self.panel.read(cx).landed().map(|s| s.to_string());

        let generate = {
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
        };
        let seed = match draft.options.seed {
            Some(seed) => seed.to_string(),
            None => "随机".to_string(),
        };
        // 任务进行中：摘要行尾追加阶段（面板已有进度条与取消，这里只做一行文字同步）
        let summary = match self.panel.read(cx).job_progress() {
            Some(progress) => {
                let tail = if progress.phase.is_quantified() {
                    format!("{} {:.0}%", progress.phase.label(), progress.percent())
                } else {
                    progress.phase.label().to_string()
                };
                format!(
                    "字段（{}）· 目标表 {} · {} 行 · 种子 {seed} · {} · {tail}",
                    draft.columns.len(),
                    draft.table_name,
                    draft.options.rows,
                    locale_label(&draft.options.locale)
                )
            }
            None => format!(
                "字段（{}）· 目标表 {} · {} 行 · 种子 {seed} · {}",
                draft.columns.len(),
                draft.table_name,
                draft.options.rows,
                locale_label(&draft.options.locale)
            ),
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
            .child(generate);

        let mut body = div()
            .v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .p_3()
            .child(header);

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

        let preview_title = match generated.as_ref() {
            Some(info) => format!(
                "预览（前 {} 行）· 临时表 {} · 本次 {} 行 · {} ms",
                PREVIEW_ROWS.min(info.preview.rows.len()),
                info.temp_table_name,
                info.row_count,
                info.elapsed_ms
            ),
            None => format!(
                "预览（前 {PREVIEW_ROWS} 行）· 尚无结果——点右上「生成」"
            ),
        };
        body = body.child(div().text_xs().text_color(muted).child(preview_title));

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
}

impl ComponentPanel for MockDetailView {
    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        let table = self.panel.read(cx).draft().table_name.clone();
        Some(format!("Mock · {table}").into())
    }

    fn title(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let table = self.panel.read(cx).draft().table_name.clone();
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child(format!("Mock · {table}"))
    }
}

#[cfg(test)]
mod tests;
