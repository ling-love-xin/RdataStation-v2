//! Mock 数据生成视图（M7 自持视图）。
//!
//! # 语义（对齐 v1 主路径）
//!
//! **造新数据**：用户命名目标表（`table_name`）+ 组织列定义（导入源库结构 / 手工加列 / 改列名与类型）
//! → 生成到**内存临时表** `temp_mock_*` → 再由**显式出口**落地：
//!
//! | 出口 | 语义 |
//! | --- | --- |
//! | 打开这张表（右 Dock 清单点一行） | 中央「Mock · {表}」tab：列（草稿可编辑 / 结果只读）+ 预览表格 |
//! | 持久化为项目分析库表 | 在**项目**分析库（`{项目}/.RSmeta/analytics.duckdb`）**新建**表（已存在则报错，引导改用「追加」） |
//! | 追加到既有表 | **显式**选择既有表；主键自增起点接续表内行数 |
//! | 保存到草稿箱 | `{项目}/scratchpad/mock/mock_*.{ext}`（时间戳命名，只读项目禁写；必须落在草稿箱扫描的 scratchpad 树下） |
//! | 另存为 | CSV / Parquet / Xlsx / SQL INSERT 文件（调用方在系统对话框选路径） |
//!
//! **生成不写库**：`generate` 只产临时表 + 预览（内存 DuckDB），一切落库与落盘都由出口按钮触发。
//! 这是与 v1 语义对齐的关键——「生成」不是「写入」。
//!
//! # 排版（D18 / D38：管理表 vs 设计这张表）
//!
//! - **右 Dock（17.5rem 起步，可拖拽调宽，[`MockPanel`]）**：表清单（唯一管理入口：状态点 + 表名 + 行数，
//!   点一行切 / 开它的 tab；关系挂在子表行下）+ 集合动作（生成 N 张表 / 退出场景）+ 出口按钮组 + 出口反馈
//!   + **折叠**的生成历史 / 用户模板；
//! - **中央 tab（[`MockDetailView`]）**：这张表的设计与生成状态——表名 / 行数 / 种子 / 语言、生成、
//!   进度与取消、失败原因与重试、列（编辑走对话框）、预览；结果表 tab 只读（列 + 预览 + 跨表后果）。
//!
//! **状态单点**：单表任务的进度 / 取消 / 失败在中央那张表的表头，集合任务（场景 N 张表）的在右 Dock
//! 场景清单下（两者互斥）；清单行只给状态点与百分比。两处状态同源（详情视图持有面板实体，单一权威）。
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
// `.when(..)`（条件挂样式 / 事件）：组件库把 `FluentBuilder` 放在 prelude，通配导入不含它
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::table::{
    Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState,
};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
// 清单行的选中标识条（2px + 上下内缩）与 M4 导航 / M5 草稿箱同一份共用原语，
// 不在本 crate 手搓绝对定位的 div。
use workbench_shell::tree;

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
use crate::ui;

// TSV 转义（剪贴板 / 导出的唯一实现，与结果集网格共用）
use ::shared::string::{tsv_cell, tsv_row};

// ---- 子模块（原 mock_view.rs 按职责拆分） ----
mod contract;
mod detail;
mod generator_search;
mod job;
mod panel;
mod pure;

#[cfg(test)]
mod tests;

// ---- 对外 re-export（workbench 等 crate 经 `mock::mock_view::*` 引用，保持原顶层 API） ----
pub use contract::{
    AppendTargetStatus, MockColumnSpec, MockDraft, MockGenInfo, MockHost, MockPreview,
    MockRunOptions, PreviewSort, SchemaRequest, SchemaSource,
};
pub use detail::{DetailTarget, MockDetailView, focus_detail_tab, status_chip_text};
pub use job::{MockJobDone, MockJobKind, MockJobPhase, MockJobProgress, MockJobState};
pub use panel::MockPanel;

// ---- 内部共享 re-export（兄弟子模块用 `use super::*;` 继承；tests 用 `super::X` 路径引用） ----
pub(crate) use contract::PreviewCache;
pub(crate) use detail::render_status_cell;
pub(crate) use generator_search::{GeneratorSearchDelegate, column_matches};
pub(crate) use panel::{complex_form_line, form_line};
pub(crate) use pure::{
    COLUMN_TYPES, FILE_FORMATS, LOCALES, PREVIEW_LIMIT_CHOICES, PREVIEW_ROWS, RECENT_GENERATORS,
    column_type_label, complex_param_hint, complex_param_text, default_generator_for,
    generator_menu, locale_label, mock_file_name, new_column_spec, param_text, parse_complex_param,
    parse_percent_ratio, parse_rows, parse_seed, patch_param, patch_param_value, summarize_params,
    validate_table_name, with_thousands,
};

// ---- 仅供测试引用的 re-export（`super::X` 路径只在 `#[cfg(test)]` 的 tests.rs 出现，
//      故 lib 编译不导出，避免 unused-import 噪音） ----
#[cfg(test)]
pub(crate) use detail::{ParamWidget, PreviewSnapshot, PreviewTableDelegate};
#[cfg(test)]
pub(crate) use generator_search::search_generators;
#[cfg(test)]
pub(crate) use panel::{
    HistoryReply, JobRowScope, RelationPick, ScenarioRelation, TableStatus, builtin_scenario_choices,
};
#[cfg(test)]
pub(crate) use pure::work_week_text;

