//! 版本历史对话框（原型 §4.3）。
//!
//! 版本**只追加**（原型 §11 决策 9）：还原 = 用旧内容生成新版本，不覆盖历史、不做行级
//! diff（文本 diff 归编辑器）。这个对话框回答三件事：还有哪些版本、哪一版的内容副本
//! 已经不在了、把选中的那一版拿去干嘛。
//!
//! 分工与另三个对话框一致：**行数据与动作执行都在宿主**（本文件不碰文件系统、不认识
//! `ArchiveService`），这里只做选中与确认。行数据可以被宿主在动作完成后**替换**
//! （`VersionDialogState::set_rows`）——所以还原完不必关窗重开，列表自己就换新了。

pub(crate) use std::cell::{Cell, RefCell};
pub(crate) use std::rc::Rc;

pub(crate) use gpui_kit::base::{Disableable as _, StyledExt};
pub(crate) use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
pub(crate) use gpui_kit::component::dialog::DialogFooter;
pub(crate) use gpui_kit::component::list::ListItem;
pub(crate) use gpui_kit::component::scroll::ScrollableElement as _;
pub(crate) use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
pub(crate) use gpui_kit::prelude::FluentBuilder as _;
pub(crate) use gpui_kit::*;

pub(crate) use crate::ui;

/// 列表最多列多少条：版本行**永久保留**（保留策略只裁内容副本，不删版本行），
/// 一个频繁再归档的存档累积上百行是正常的——再多只列最新的一批并**明说**，
/// 不静默截断，也不为这个对话框手搓虚拟化（行数真成问题时再上 `List`）。
pub const MAX_VERSION_ROWS: usize = 50;

/// 一行版本（**宿主快照**：对话框只渲染与选中，不做计算、不查文件系统）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    /// 版本号（列表按它降序：新的在上）。
    pub version: i32,
    /// 是否当前版本（本体上的那一版）。当前版本没有"还原 / 取回该版本"动作
    /// ——它就是本体，取回走主界面的「取回（检出）…」。
    pub is_current: bool,
    /// 版本时间（绝对时间：版本历史是查凭证的地方，相对时间不够用）。
    pub time_label: String,
    /// 大小（可能为空：缺 `file_size` 的旧快照）。
    pub size_label: String,
    /// 内容指纹前 12 位（可能为空：无指纹的旧行）。
    pub hash_short: String,
    /// 内容副本在不在（当前版本 = 本体在位；历史行 = `.RSmeta` 下有副本）。
    pub has_copy: bool,
    /// 与**上一版**（版本号 -1）的差异：`较 v1 +1.2 KB · 指纹已变`；最旧一版为空。
    ///
    /// 不做行级 diff，只给"大小变了多少、指纹是否变"这两个能一眼扫的值。
    pub delta_label: String,
}

/// 打开对话框所需的全部信息（宿主在取数线程上备好）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionDialogSeed {
    /// 存档显示名（标题用）。
    pub name: String,
    /// 当前版本号（标题副文案用）。
    pub current_version: i32,
    /// 版本行（降序，含当前版本那一行）。
    pub rows: Vec<VersionRow>,
}

/// 动作栏的动作（选中某一行后由用户点出；执行一律回宿主）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionAction {
    /// 还原为当前版本：用这一版的内容生成**新版本**（不覆盖历史）。
    Restore { version: i32 },
    /// 取回该版本为草稿（复制一份可写工作副本，本体不动）。
    CheckoutDraft { version: i32 },
    /// 删除该版本的内容副本（**保留版本行**；删了不可恢复，所以要确认）。
    DeleteCopy { version: i32 },
    /// 与当前版本对比（只读查看，不入动作回执、不写库；文本 diff 查看器 `version_diff`）。
    Diff { version: i32 },
}

impl VersionAction {
    /// 版本号（四种动作都要它）。
    pub fn version(self) -> i32 {
        match self {
            Self::Restore { version }
            | Self::CheckoutDraft { version }
            | Self::DeleteCopy { version }
            | Self::Diff { version } => version,
        }
    }
}

/// 对话框状态：**宿主也持一份克隆**（动作完成后 `set_rows` 换掉行数据、`set_busy` 收放）。
#[derive(Clone)]
pub struct VersionDialogState {
    rows: Rc<RefCell<Vec<VersionRow>>>,
    selected: Rc<RefCell<Option<i32>>>,
    /// 动作进行中：按钮一律置灰（避免连点两次还原，那会连着生成两个版本）。
    busy: Rc<Cell<bool>>,
    /// 项目只读：三个动作全部置灰（判定与面板 / 详情同一口径：由宿主机注入）。
    read_only: Rc<Cell<bool>>,
    /// 动作期间的提示（"正在还原 v1…"）；`None` = 不显示。
    note: Rc<RefCell<Option<String>>>,
}

impl Default for VersionDialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionDialogState {
    pub fn new() -> Self {
        Self {
            rows: Rc::new(RefCell::new(Vec::new())),
            selected: Rc::new(RefCell::new(None)),
            busy: Rc::new(Cell::new(false)),
            read_only: Rc::new(Cell::new(false)),
            note: Rc::new(RefCell::new(None)),
        }
    }

    /// 换一批行（宿主在取数回来 / 动作完成后再取数后调用）。
    ///
    /// 选中项若在新行里不存在就清掉：留着会指向一个已经不在列表里的版本。
    pub fn set_rows(&self, rows: Vec<VersionRow>) {
        // 先把选中值取出来（`Copy`）：`if let Some(..) = *self.selected.borrow()` 会把借用
        // 延续到整个块，块内再 `borrow_mut` 就是“RefCell already borrowed”（已踩）。
        let selected = *self.selected.borrow();
        if let Some(selected) = selected {
            if !rows.iter().any(|row| row.version == selected) {
                *self.selected.borrow_mut() = None;
            }
        }
        *self.rows.borrow_mut() = rows;
    }

    pub fn rows(&self) -> Vec<VersionRow> {
        self.rows.borrow().clone()
    }

    pub fn set_selected(&self, version: Option<i32>) {
        *self.selected.borrow_mut() = version;
    }

    pub fn selected(&self) -> Option<i32> {
        *self.selected.borrow()
    }

    /// 选中的那一行（动作栏据此决定按钮可用性）。
    pub fn selected_row(&self) -> Option<VersionRow> {
        let selected = self.selected()?;
        self.rows
            .borrow()
            .iter()
            .find(|row| row.version == selected)
            .cloned()
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

pub use self::open::{open_version_dialog, open_version_dialog_with};
