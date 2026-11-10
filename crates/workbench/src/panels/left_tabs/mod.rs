//! 左 dock 模块面板（真面板化）：数据源导航 / 草稿箱 / 资产库 / 插件。
//!
//! 每个模块 = 独立 dock tab（实现 gpui-kit `Panel` trait），dock 自动生成 tab 条：
//! 点击 tab 同面切换，拖拽可分栏并排显示多个模块。内容仍是各 crate 自持视图
//! （`NavView` / `ScratchpadView` / `ResourcesPanel`），每个面板一个文件，只做薄壳 + 面板协议：
//! - `tab_name` / `title`：tab 条展示（带活动栏同款图标）；
//! - `set_active`：dock 切 tab 时把活动栏高亮同步回 `Shared::active_left`；
//! - `closable = false`：模块 tab 是布局的一部分，不给 ✕ 关闭（避免重建逻辑）。

use gpui_kit::EventEmitter;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::component::empty::{
    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::hover_card::HoverCard;
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{ActiveTheme, IconName, IndexPath, Sizable, Size};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use crate::view::LeftPanel;

use analytics_resource::resource_view::ResourcesPanel;
use database::nav_view::NavView;
use plugin::ContributionCounts;
use plugin::PluginInfo;
use plugin::PluginKind;
use plugin::wasm::PluginState;
use scratchpad::ScratchpadView;

use super::Shared;


mod database_nav;
mod plugin_tab;
mod resources;
mod scratch;

pub use database_nav::DatabaseNavPanel;
pub use plugin_tab::PluginPanel;
pub(crate) use plugin_tab::uninstall_confirm_description;
pub use resources::ResourcePanelTab;
pub use scratch::ScratchPanel;

#[cfg(test)]
mod tests;
