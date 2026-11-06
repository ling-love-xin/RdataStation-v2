//! 标签对话框（原型 §3.1「标签与分组」里的标签那一半；数据与 §2.2 的标签筛选同源）。
//!
//! 三件事：勾选 / 取消这条存档的标签、现场新建一个（建完直接打上）、把改动**一次提交**
//! （进出算成 add / remove 差集，宿主只收到两个 id 列表）。
//!
//! 两条纪律与另几个对话框一致：
//! 1. **输入实体在开窗前建好**（`open_dialog` 的 builder 是 `Fn`，每帧重建）；
//! 2. **校验从输入值推导**，不是标志位——每帧重算，用户改回去提示自然消失。
//!
//! 行数据与执行都在宿主（本文件不碰库、不认识 `AnalyticsResourceStore`）；宿主可在动作完成后
//! 重取一次并 `set_options` / `set_selected` 换行，所以新建完不必关窗重开。

pub(crate) use std::cell::{Cell, RefCell};
pub(crate) use std::rc::Rc;

pub(crate) use gpui_kit::base::{Disableable as _, StyledExt};
pub(crate) use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
pub(crate) use gpui_kit::component::checkbox::Checkbox;
pub(crate) use gpui_kit::component::dialog::{DialogButtonProps, DialogFooter};
pub(crate) use gpui_kit::component::input::{Input, InputState};
pub(crate) use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
pub(crate) use gpui_kit::component::scroll::ScrollableElement as _;
pub(crate) use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
pub(crate) use gpui_kit::prelude::FluentBuilder as _;
pub(crate) use gpui_kit::*;

pub(crate) use crate::ui;

/// 一行可选标签（**宿主快照**：对话框只渲染与勾选）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagChoice {
    pub id: String,
    pub name: String,
    /// 有多少条存活存档在用（`0` = 还没人用，新建的标签就是这个状态）。
    pub count: usize,
}

/// 打开对话框所需的全部信息（宿主在取数线程上备好）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagDialogSeed {
    /// 存档显示名（单选时的标题用）。
    pub resource_name: String,
    /// 本次编辑**作用于几条存档**（`1` = 单选；`> 1` = 多选批量，标题改显「N 项」）。
    pub target_count: usize,
    /// 全部存活标签（按名字升序）。
    pub options: Vec<TagChoice>,
    /// **所有**目标都挂着的标签 id（= 并集里“全有”的那部分，显示为勾上）。
    pub selected: Vec<String>,
    /// 只有**一部分**目标挂着的标签 id（显示为「部分」；点一下 = 让所有目标都挂上）。
    pub partial: Vec<String>,
}

/// 对话框提交的动作（执行一律回宿主）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagDialogEvent {
    /// 提交这次勾选：`add` / `remove` 是相对开窗时的差集（两个都空就不该提交）。
    Apply {
        add: Vec<String>,
        remove: Vec<String>,
    },
    /// 新建一个标签并**直接打上**（一次动作做完，不必先建再去勾）。
    CreateAndTag { name: String },
    /// 重命名一个标签（**补 v1 缺失的能力**；只改显示名，不动它的关联）。
    RenameTag { id: String, name: String },
    /// 删除一个标签（关联一并清掉——服务层同一事务里做）。
    DeleteTag { id: String },
}

/// 对话框状态：**宿主也持一份克隆**（动作完成后换行与收放忙态）。
#[derive(Clone)]
pub struct TagDialogState {
    options: Rc<RefCell<Vec<TagChoice>>>,
    /// 当前勾选（开窗时 = 所有目标都挂着的标签）。
    selected: Rc<RefCell<Vec<String>>>,
    /// 开窗时的勾选（算差集用；动作成功后由宿主重置为新的已挂集合）。
    baseline: Rc<RefCell<Vec<String>>>,
    /// 当前处于「部分」（只有一部分目标挂着）的标签；点一下即提升为勾选。
    partial: Rc<RefCell<Vec<String>>>,
    /// 开窗时的「部分」集：**算差集用**（它不在 `baseline` 里，但不等于“没有——取消时得从所有目标上摘掉”）。
    baseline_partial: Rc<RefCell<Vec<String>>>,
    /// 提交中：按钮一律置灰（避免连点两次，那会提交两次差集）。
    busy: Rc<Cell<bool>>,
    /// 项目只读：勾选与新建全部置灰（判定与面板 / 详情同一口径，由宿主机注入）。
    read_only: Rc<Cell<bool>>,
    /// 动作期间的提示（"正在打标签…"）；`None` = 不显示。
    note: Rc<RefCell<Option<String>>>,
}

impl Default for TagDialogState {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl TagDialogState {
    /// 用初始勾选建状态（`selected` 同时进 baseline）；单选场景的便捷入口。
    pub fn new(selected: Vec<String>) -> Self {
        Self::new_batch(selected, Vec::new())
    }

    /// 多选场景：除了「全有」的勾选，还带一份「只有一部分目标有」的标签。
    pub fn new_batch(selected: Vec<String>, partial: Vec<String>) -> Self {
        Self {
            options: Rc::new(RefCell::new(Vec::new())),
            selected: Rc::new(RefCell::new(selected.clone())),
            baseline: Rc::new(RefCell::new(selected)),
            partial: Rc::new(RefCell::new(partial.clone())),
            baseline_partial: Rc::new(RefCell::new(partial)),
            busy: Rc::new(Cell::new(false)),
            read_only: Rc::new(Cell::new(false)),
            note: Rc::new(RefCell::new(None)),
        }
    }

    pub fn set_options(&self, options: Vec<TagChoice>) {
        // 勾选里已经被删掉的标签要清掉（词典是权威：库里的标签没了，勾着也没意义）。
        let alive: Vec<String> = options.iter().map(|option| option.id.clone()).collect();
        self.selected.borrow_mut().retain(|id| alive.contains(id));
        self.baseline.borrow_mut().retain(|id| alive.contains(id));
        self.partial.borrow_mut().retain(|id| alive.contains(id));
        self.baseline_partial
            .borrow_mut()
            .retain(|id| alive.contains(id));
        *self.options.borrow_mut() = options;
    }

    pub fn options(&self) -> Vec<TagChoice> {
        self.options.borrow().clone()
    }

    /// 换一份勾选，并把 baseline 对齐到它（动作成功后宿主调用：新状态即新的比较基准）。
    pub fn reset_selection(&self, selected: Vec<String>) {
        self.reset_batch(selected, Vec::new())
    }

    /// 多选版的 [`Self::reset_selection`]：连「部分」一起换掉。
    pub fn reset_batch(&self, selected: Vec<String>, partial: Vec<String>) {
        *self.selected.borrow_mut() = selected.clone();
        *self.baseline.borrow_mut() = selected;
        self.partial.borrow_mut().clear();
        self.partial.borrow_mut().extend(partial.iter().cloned());
        *self.baseline_partial.borrow_mut() = partial;
    }

    /// 勾选 / 取消（行内勾选框走它：`Checkbox` 交出的是**新值**，所以这里按值写而不是翻转）。
    pub fn set_checked(&self, tag_id: &str, on: bool) {
        let mut selected = self.selected.borrow_mut();
        match (on, selected.iter().position(|id| id == tag_id)) {
            (true, None) => selected.push(tag_id.to_string()),
            (false, Some(index)) => {
                selected.remove(index);
            }
            // 值没变（连点两下 / 与状态已同步）：不重复写，免得打乱勾选顺序。
            _ => {}
        }
    }

    pub fn toggle(&self, tag_id: &str) {
        let mut selected = self.selected.borrow_mut();
        match selected.iter().position(|id| id == tag_id) {
            Some(index) => {
                selected.remove(index);
            }
            None => selected.push(tag_id.to_string()),
        }
    }

    /// 「部分」标签点一下：提升为**全有**（= 给缺的那些目标补上）。
    ///
    /// 为何单选那套 `toggle` 不够用：多选时一个标签可能是「部分有」，布尔翻转会把
    /// 「本来部分有」的标签一下推成「无」——那是用户没要求的**批量摘除**。三态里
    /// 第一下总是「让所有目标都有」，第二下才清。
    pub fn promote_partial(&self, tag_id: &str) {
        self.partial.borrow_mut().retain(|id| id != tag_id);
        if !self.is_selected(tag_id) {
            self.selected.borrow_mut().push(tag_id.to_string());
        }
    }

    /// 行内勾选框（三态版）：勾上 = 全有（部分 → 全有）；取消 = 全无。
    pub fn set_checked_three_way(&self, tag_id: &str, on: bool) {
        if on {
            self.promote_partial(tag_id);
            return;
        }
        // 取消就把两个集合都清掉：它现在是「全无」，不再是「部分」。
        self.partial.borrow_mut().retain(|id| id != tag_id);
        self.selected.borrow_mut().retain(|id| id != tag_id);
    }

    pub fn is_selected(&self, tag_id: &str) -> bool {
        self.selected.borrow().iter().any(|id| id == tag_id)
    }

    /// 只有一部分目标挂着这个标签（显示「部分」；它不是勾选态）。
    pub fn is_partial(&self, tag_id: &str) -> bool {
        self.partial.borrow().iter().any(|id| id == tag_id)
    }

    pub fn selected(&self) -> Vec<String> {
        self.selected.borrow().clone()
    }

    pub fn partial(&self) -> Vec<String> {
        self.partial.borrow().clone()
    }

    /// 本次改动的差集（`(add, remove)`）：两个都空 = 没有可提交的东西。
    ///
    /// 多选的语义（“让勾选对**所有目标**生效”）：
    /// - `add` = 现在勾着、但开窗时不是全有的 → 给缺的那些补上（幂等，已挂的重复打不报错）；
    /// - `remove` = 开窗时**全有或部分有**、现在既没勾也不在「部分」里的 → 从所有目标上摘掉。
    ///   两个减项都是必需的：开窗时的「部分」不算进 remove，就漏了「部分 → 取消」这一类；
    ///   而**没动过**的「部分」又得排除在外（否则开窗那一刻就凭空多出一批待摘标签）。
    pub fn diff(&self) -> (Vec<String>, Vec<String>) {
        let baseline = self.baseline.borrow();
        let baseline_partial = self.baseline_partial.borrow();
        let selected = self.selected.borrow();
        let partial = self.partial.borrow();
        let add = selected
            .iter()
            .filter(|id| !baseline.contains(id))
            .cloned()
            .collect();
        let remove = baseline
            .iter()
            .chain(baseline_partial.iter())
            .filter(|id| !selected.contains(id) && !partial.contains(id))
            .cloned()
            .collect();
        (add, remove)
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

/// 打开标签对话框（宿主入口）。
///
/// `name_input` 由调用方**在开窗前**建好（builder 是 `Fn`，见模块头注释）。
/// `on_action` 在用户点「应用」或「新建并打上」时回调一次；是否忙由宿主说了算。

mod menu;
mod open;

#[cfg(test)]
mod tests;

pub use self::open::open_tag_dialog;

pub fn open_tag_rename_dialog(
    window: &mut Window,
    cx: &mut App,
    tag_id: String,
    name_input: Entity<InputState>,
    on_confirm: impl Fn(String, String, &mut Window, &mut App) + 'static,
) {
    let on_confirm: Rc<dyn Fn(String, String, &mut Window, &mut App)> = Rc::new(on_confirm);
    let id_for_ok = tag_id.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let value = name_input.read(cx).value().trim().to_string();
        let hint = if value.is_empty() {
            Some("标签名不能为空")
        } else {
            None
        };
        let can_save = hint.is_none();

        let confirm_for_button = {
            let on_confirm = on_confirm.clone();
            let input = name_input.clone();
            let id = id_for_ok.clone();
            move |window: &mut Window, cx: &mut App| {
                let name = input.read(cx).value().trim().to_string();
                if name.is_empty() {
                    return;
                }
                on_confirm(id.clone(), name, window, cx);
            }
        };
        let confirm_for_ok = confirm_for_button.clone();

        let mut body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(Input::new(&name_input));
        body = body.child(
            div()
                .w_full()
                .text_xs()
                .text_color(if hint.is_some() {
                    theme.colors.warning
                } else {
                    theme.colors.muted_foreground
                })
                .child(hint.unwrap_or("只改显示名：已打了这个标签的存档不动")),
        );

        dialog
            .title("重命名标签")
            .w(cx.theme().font_size * ui::TAG_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("tag-rename-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "tag-rename-cancel".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("tag-rename-save")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .debug_selector(|| "tag-rename-save".to_string())
                            .label("保存")
                            .disabled(!can_save)
                            .on_click(move |_, window, cx| {
                                confirm_for_button(window, cx);
                                window.close_dialog(cx);
                            }),
                    ),
            )
            .on_ok(move |_, window, cx| {
                confirm_for_ok(window, cx);
                true
            })
            .on_cancel(|_, _, _| true)
    });
}
