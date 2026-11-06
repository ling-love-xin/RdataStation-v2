//! 回收站对话框（原型 §4.4）。
//!
//! 回收站只有一套（模块硬约束 5）：它就是**项目级**的 `.RSmeta/trash`，草稿箱与资产库共用。
//! 于是这个对话框第一件要说清的事是"这里只有自己的"——`origin` 不是本模块的条目**不列成行**，
//! 只在下方给一句说明（列成行的话，用户点「还原」必然被我方服务层的 origin 校验拒绝，
//! 等于自找一次错误，而那句话本来就能说清为什么不在这里）。
//!
//! 第二件事是"清空"的口径：**只清自己名下的条目**。整仓清空会把别人的东西一起删掉，
//! 那不是清空是越权（`ProjectTrash::empty_origin` 就是为这条存在的）。
//!
//! 分工与另四个对话框一致：**行数据与动作执行都在宿主**（本文件不碰文件系统、不认识
//! `ProjectTrash`），这里只渲染与派发动作；动作完成后宿主重取一次并 `set_rows` 换行，
//! 所以还原 / 永久删除 / 清空之后不必关窗重开。

pub(crate) use std::cell::{Cell, RefCell};
pub(crate) use std::rc::Rc;

pub(crate) use gpui_kit::base::{Disableable as _, StyledExt};
pub(crate) use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
pub(crate) use gpui_kit::component::dialog::{DialogButtonProps, DialogFooter};
pub(crate) use gpui_kit::component::scroll::ScrollableElement as _;
pub(crate) use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
pub(crate) use gpui_kit::prelude::FluentBuilder as _;
pub(crate) use gpui_kit::*;

pub(crate) use crate::model::PayloadMode;
pub(crate) use crate::ui;

/// 列表最多列多少条：回收站条目只增不减（谁也不会常来清），超出的**明说**，
/// 不静默截断，也不为这个对话框手搓虚拟化（行数真成问题时再上 `List`）。
pub const MAX_TRASH_ROWS: usize = 100;


/// 别的模块留在项目级回收站里的条目（**只做说明，不列行**）。
///
/// 装作没有会比说清楚更让人误会"我删的东西怎么不见了"：一句话就能回答
/// "为什么这里的条目比我在资产库里删的多"。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignTrash {
    /// 模块显示名（宿主组装：本 crate 不认识别的模块的名字）。
    pub module_label: String,
    pub count: usize,
}

/// 打开对话框所需的全部信息（宿主在取数线程上备好）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashDialogSeed {
    /// 只含本模块（`origin = "resources"`）的条目，按删除时间倒序。
    pub rows: Vec<TrashRow>,
    /// 其他模块的条目统计（可能为空）。
    pub foreign: Vec<ForeignTrash>,
}

/// 对话框动作（执行一律回宿主）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashAction {
    /// 还原一条：本体回原位、登记行复活。
    Restore { trash_id: String },
    /// 永久删除一条（不可恢复，故要确认）。
    Purge { trash_id: String },
    /// 清空**本模块的**条目（别人的条目不动）。
    Empty,
}

/// 对话框状态：**宿主也持一份克隆**（动作完成后 `set_rows` 换掉行数据、`set_busy` 收放）。
#[derive(Clone)]
pub struct TrashDialogState {
    rows: Rc<RefCell<Vec<TrashRow>>>,
    foreign: Rc<RefCell<Vec<ForeignTrash>>>,
    /// 动作进行中：按钮一律置灰（避免连点两次还原，那会连搬两次本体）。
    busy: Rc<Cell<bool>>,
    /// 项目只读：还原 / 永久删除 / 清空全部置灰（判定与面板 / 详情同一口径，由宿主机注入）。
    read_only: Rc<Cell<bool>>,
    /// 动作期间的提示（"正在还原…"）；`None` = 不显示。
    note: Rc<RefCell<Option<String>>>,
}

impl Default for TrashDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl TrashDialogState {
    pub fn new() -> Self {
        Self {
            rows: Rc::new(RefCell::new(Vec::new())),
            foreign: Rc::new(RefCell::new(Vec::new())),
            busy: Rc::new(Cell::new(false)),
            read_only: Rc::new(Cell::new(false)),
            note: Rc::new(RefCell::new(None)),
        }
    }

    /// 换一批行（宿主在取数回来 / 动作完成后重取时调用）。
    pub fn set_rows(&self, rows: Vec<TrashRow>) {
        *self.rows.borrow_mut() = rows;
    }

    pub fn rows(&self) -> Vec<TrashRow> {
        self.rows.borrow().clone()
    }

    pub fn set_foreign(&self, foreign: Vec<ForeignTrash>) {
        *self.foreign.borrow_mut() = foreign;
    }

    pub fn foreign(&self) -> Vec<ForeignTrash> {
        self.foreign.borrow().clone()
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

mod open;
mod render;
#[cfg(test)]
mod tests;

pub use self::open::{open_trash_dialog, open_trash_dialog_with};

pub use crate::view_model::dialog_rows::TrashRow;
