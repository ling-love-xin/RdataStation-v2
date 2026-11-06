//! 索引修复对话框（原型 §4.5）。
//!
//! 三类差异分组呈现，每组给**行内动作**（与版本历史对话框不同：这里一行最多两个动作、
//! 行内容短，动作列装得下）。
//!
//! 硬原则与 `indexer` 一致：**不做自动修复**。扫描只报告，这里的每个按钮都是
//! "用户明确点了才动"；对话框关掉什么都不会变。
//!
//! 与另几个对话框同分工：**行数据与动作执行都在宿主**（本文件不碰文件系统、不认识
//! `IndexRepair`），行数据可在动作完成后被宿主换掉（`set_rows`），所以修完一项
//! 不必关窗重开。

pub(crate) use std::cell::{Cell, RefCell};
pub(crate) use std::rc::Rc;

pub(crate) use gpui_kit::base::{Disableable as _, StyledExt};
pub(crate) use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
pub(crate) use gpui_kit::component::dialog::DialogFooter;
pub(crate) use gpui_kit::component::scroll::ScrollableElement as _;
pub(crate) use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
pub(crate) use gpui_kit::prelude::FluentBuilder as _;
pub(crate) use gpui_kit::*;

pub(crate) use crate::ui;

/// 每组最多列多少行：再多只列前 N 并**明说**（扫描一次可能报出成百个未登记文件）。
pub const MAX_REPAIR_ROWS_PER_GROUP: usize = 50;




/// 对话框状态：**宿主也持一份克隆**（动作完成后 `set_rows` 换掉行数据、`set_busy` 收放）。
#[derive(Clone)]
pub struct RepairDialogState {
    rows: Rc<RefCell<Vec<RepairRow>>>,
    /// 动作进行中：按钮一律置灰（避免连点两次"接受当前内容"，那会连着生成两个版本）。
    busy: Rc<Cell<bool>>,
    /// 项目只读：写类动作置灰（判定与面板 / 详情同一口径，由宿主机注入）。
    read_only: Rc<Cell<bool>>,
    /// 动作期间的提示（"正在补登…"）；`None` = 不显示。
    note: Rc<RefCell<Option<String>>>,
}

impl Default for RepairDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl RepairDialogState {
    pub fn new() -> Self {
        Self {
            rows: Rc::new(RefCell::new(Vec::new())),
            busy: Rc::new(Cell::new(false)),
            read_only: Rc::new(Cell::new(false)),
            note: Rc::new(RefCell::new(None)),
        }
    }

    /// 换一批行（宿主在动作完成后再扫一次之后调用）。
    pub fn set_rows(&self, rows: Vec<RepairRow>) {
        *self.rows.borrow_mut() = rows;
    }

    pub fn rows(&self) -> Vec<RepairRow> {
        self.rows.borrow().clone()
    }

    pub fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
    }

    pub fn busy(&self) -> bool {
        self.busy.get()
    }

    /// 打开前由宿主机告知项目是否只读（对话框自己不判定项目状态）。
    pub fn set_read_only(&self, read_only: bool) {
        self.read_only.set(read_only);
    }

    pub fn read_only(&self) -> bool {
        self.read_only.get()
    }

    pub fn set_note(&self, note: Option<String>) {
        *self.note.borrow_mut() = note;
    }

    pub fn note(&self) -> Option<String> {
        self.note.borrow().clone()
    }
}

/// 行内动作（执行一律回宿主）。
///
/// `OpenVersions` 是**跳转**而不是修复：它只打开版本历史对话框，改什么由用户在那边决定
/// （"从历史还原"要挑是哪一版——那是版本历史的活，不在这里重复一遍挑选 UI）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairAction {
    /// 补登为存档（`resources/` 里只可能是文件，故固定文件型；来源无从得知，留空）。
    Adopt { rel_path: String },
    /// 删除孤儿登记（本体没了）。
    DeleteRecord { resource_id: String },
    /// 接受当前内容：指纹换成实际值、版本 +1（旧内容不可得，那一版历史只留元数据）。
    AcceptContent { resource_id: String },
    /// 从回收站还原：本体被删过（进了项目级回收站），把它搬回期望位置并复活登记行。
    ///
    /// 手上只有**期望相对路径**（这一组行就是“登记在、本体不在”），由服务层去回收站里
    /// 按 `original_rel_path` 找条目——找不到就直说只能删记录。
    RestoreFromTrash { rel_path: String },
    /// 打开版本历史（挑一版还原）。
    OpenVersions { resource_id: String },
}

mod open;
mod render;
#[cfg(test)]
mod tests;

pub use self::open::{open_index_repair_dialog, open_index_repair_dialog_with};

pub use crate::view_model::dialog_rows::{RepairGroup, RepairRow};
