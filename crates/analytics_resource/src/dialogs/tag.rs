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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::{DialogButtonProps, DialogFooter};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui;

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
pub fn open_tag_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: TagDialogSeed,
    state: TagDialogState,
    name_input: Entity<InputState>,
    on_action: impl Fn(TagDialogEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    // 种子灌进状态：之后一切渲染与提交都只读这一个来源。
    if state.options().is_empty() {
        state.set_options(seed.options.clone());
        state.reset_batch(seed.selected.clone(), seed.partial.clone());
    }
    let on_action: Rc<dyn Fn(TagDialogEvent, &mut Window, &mut App)> = Rc::new(on_action);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);
    let resource_name = seed.resource_name.clone();
    let target_count = seed.target_count;
    let batch = target_count > 1;
    // 标题：单选报名字（与其它对话框同一形态）；多选报**项数**（那是本次编辑真正的作用域）。
    let title = if batch {
        format!("标签 · {target_count} 项")
    } else {
        format!("标签 · {resource_name}")
    };

    // 新建（按钮或回车走同一条）：建完直接打上，对话框不关（宿主会把新词典推回来）。
    let dispatch_create = {
        let state = state.clone();
        let on_action = on_action.clone();
        let input = name_input.clone();
        move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_string();
            if name.is_empty() || state.busy() || state.read_only() {
                return;
            }
            state.set_busy(true);
            state.set_note(Some("正在新建…".to_string()));
            input.update(cx, |input, cx| input.set_value("", window, cx));
            on_action(TagDialogEvent::CreateAndTag { name }, window, cx);
        }
    };
    // 应用（按钮与回车共用）：只提交差集，没改动就什么都不做。
    let dispatch_apply = {
        let state = state.clone();
        let on_action = on_action.clone();
        move |window: &mut Window, cx: &mut App| {
            let (add, remove) = state.diff();
            if add.is_empty() && remove.is_empty() {
                return;
            }
            state.set_busy(true);
            state.set_note(Some("正在打标签…".to_string()));
            on_action(TagDialogEvent::Apply { add, remove }, window, cx);
        }
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let options = state.options();
        let busy = state.busy();
        let read_only = state.read_only();
        let note = state.note();
        let can_edit = !busy && !read_only;
        let selected_count = state.selected().len();
        let name_value = name_input.read(cx).value().trim().to_string();
        let can_create = can_edit && !name_value.is_empty();

        let mut list = div().v_flex().w_full();
        if options.is_empty() {
            list = list.child(
                div()
                    .w_full()
                    .py_2()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("还没有任何标签——在下面输入一个名字即可新建"),
            );
        }
        for (index, option) in options.iter().enumerate() {
            let checked = state.is_selected(&option.id);
            let partial = state.is_partial(&option.id);
            // 行点击与勾选框都走**三态**：部分 → 全有 → 无（多选下布尔翻转会误删，见 `promote_partial`）。
            let dispatch = {
                let state = state.clone();
                let tag_id = option.id.clone();
                move |_: &ClickEvent, window: &mut Window, _cx: &mut App| {
                    if state.is_partial(&tag_id) {
                        state.promote_partial(&tag_id);
                    } else {
                        state.toggle(&tag_id);
                    }
                    window.refresh();
                }
            };
            // 行内勾选框走真 `Checkbox`（与草稿选择对话框同一写法）：
            // 自绘 `✓` 字符既没有键盘焦点，也拿不到"勾上/未勾"的语义（a11y 只剩一个文本节点）。
            let check_dispatch = {
                let state = state.clone();
                let tag_id = option.id.clone();
                move |on: &bool, window: &mut Window, cx: &mut App| {
                    // 勾选框在行内：不让点击冒泡到行的 `on_click`（否则同一击翻转两次 = 没反应）。
                    cx.stop_propagation();
                    state.set_checked_three_way(&tag_id, *on);
                    window.refresh();
                }
            };
            // 行尾的「⋯」：改名 / 删除（v1 缺的两项，入口就在它们的词典里）。
            let menu = {
                let state = state.clone();
                let on_action = on_action.clone();
                let tag_id = option.id.clone();
                let tag_name = option.name.clone();
                let can_edit = can_edit;
                // `dropdown_menu` 的回调拿的是 `Context<PopupMenu>`（不是 `App`），签名要照着写。
                move |menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>| {
                    let rename_dispatch = {
                        let state = state.clone();
                        let on_action = on_action.clone();
                        let tag_id = tag_id.clone();
                        let tag_name = tag_name.clone();
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            if !can_edit {
                                return;
                            }
                            // 输入实体在开窗前建好（builder 是 `Fn`，见模块头注释）。
                            let input = cx.new(|cx| {
                                InputState::new(window, cx).placeholder("标签名")
                            });
                            let value = tag_name.clone();
                            input.update(cx, |input, cx| input.set_value(value, window, cx));
                            let state = state.clone();
                            let on_action = on_action.clone();
                            let tag_id = tag_id.clone();
                            open_tag_rename_dialog(
                                window,
                                cx,
                                tag_id,
                                input,
                                move |id, name, window, cx| {
                                    state.set_busy(true);
                                    state.set_note(Some("正在重命名…".to_string()));
                                    on_action(TagDialogEvent::RenameTag { id, name }, window, cx);
                                },
                            );
                        }
                    };
                    let delete_dispatch = {
                        let state = state.clone();
                        let on_action = on_action.clone();
                        let tag_id = tag_id.clone();
                        let tag_name = tag_name.clone();
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            if !can_edit {
                                return;
                            }
                            let state = state.clone();
                            let on_action = on_action.clone();
                            let tag_id = tag_id.clone();
                            let name = tag_name.clone();
                            window.open_alert_dialog(cx, move |alert, _window, _cx| {
                                let state = state.clone();
                                let on_action = on_action.clone();
                                let tag_id = tag_id.clone();
                                alert
                                    .confirm()
                                    .title(format!("删除标签「{name}」？"))
                                    .description(
                                        "标签会从所有存档上摘掉（关联一并清除），但存档本身不受影响。",
                                    )
                                    .button_props(
                                        DialogButtonProps::default()
                                            .ok_text("删除")
                                            .ok_variant(ButtonVariant::Danger)
                                            .show_cancel(true),
                                    )
                                    .on_ok(move |_, window, cx| {
                                        state.set_busy(true);
                                        state.set_note(Some("正在删除标签…".to_string()));
                                        on_action(
                                            TagDialogEvent::DeleteTag { id: tag_id.clone() },
                                            window,
                                            cx,
                                        );
                                        true
                                    })
                            });
                        }
                    };
                    let mut menu = menu;
                    if can_edit {
                        menu = menu.item(
                            PopupMenuItem::new("重命名…")
                                .disabled(!can_edit)
                                .on_click(rename_dispatch),
                        );
                        menu = menu.item(
                            PopupMenuItem::new("删除")
                                .disabled(!can_edit)
                                .on_click(delete_dispatch),
                        );
                    }
                    menu
                }
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("tag-choice-{}", option.id)))
                    .debug_selector({
                        let id = option.id.clone();
                        move || format!("tag-choice-{id}")
                    })
                    .when(can_edit, |row| row.cursor_pointer())
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    // 勾选底统一到 `list_active`（与面板 / 其它对话框的"选中底"同一个角色）：
                    // 对话框内不用 `accent.opacity(_)` 这类品牌淡色当状态底。
                    .when(checked, |row| row.bg(theme.colors.list_active))
                    // 悬停不覆盖勾选（导航侧 V11 口径）：勾上的行保持勾选底。
                    .when(can_edit && !checked, |row| {
                        row.hover(|s| s.bg(theme.colors.list_hover))
                    })
                    .when(can_edit, |row| row.on_click(dispatch))
                    .child(
                        Checkbox::new(SharedString::from(format!("tag-check-{}", option.id)))
                            .debug_selector({
                                let id = option.id.clone();
                                move || format!("tag-check-{id}")
                            })
                            .checked(checked)
                            .disabled(!can_edit)
                            .on_click(check_dispatch),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(theme.colors.foreground)
                            .child(option.name.clone()),
                    )
                    // 「部分」：只有一部分目标挂着（多选才有这个态）。`Checkbox` 没有半选态，
                    // 所以用一枚小字说清楚——它**不是**勾选，点一下才是「让所有目标都挂上」。
                    .when(partial, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.warning)
                                .child("部分"),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("{}", option.count)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("tag-more-{}", option.id)))
                            .ghost()
                            .xsmall()
                            .debug_selector({
                                let id = option.id.clone();
                                move || format!("tag-more-{id}")
                            })
                            .label("⋯")
                            .disabled(!can_edit)
                            .dropdown_menu(menu),
                    ),
            );
            let _ = index;
        }

        let mut body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("共 {} 个标签 · 已选 {selected_count} 个", options.len())),
                    )
                    .when_some(note, |bar, note| {
                        bar.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.warning)
                                .child(note),
                        )
                    }),
            )
            .child(
                list.max_h(rems(ui::TAG_LIST_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .into_any_element(),
            )
            // 新建行：输入 + 「新建并打上」（回车同一条路）。
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(Input::new(&name_input)))
                    .child(
                        Button::new("tag-create")
                            .small()
                            .debug_selector(|| "tag-create".to_string())
                            .label("新建并打上")
                            .disabled(!can_create)
                            .on_click({
                                let dispatch_create = dispatch_create.clone();
                                move |_, window, cx| dispatch_create(window, cx)
                            }),
                    ),
            );

        // 应用：只提交差集（回车与按钮同一条路）。
        let apply_dispatch = {
            let dispatch_apply = dispatch_apply.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                dispatch_apply(window, cx);
                window.close_dialog(cx);
            }
        };
        let (add_count, remove_count) = {
            let (add, remove) = state.diff();
            (add.len(), remove.len())
        };
        let has_changes = add_count + remove_count > 0;

        body = body.child(
            div()
                .w_full()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(if has_changes {
                    format!("本次改动：加 {add_count} 个 · 去 {remove_count} 个")
                } else {
                    "勾选标签后「应用」——改动在提交前不落库".to_string()
                }),
        );
        // 多选时把作用域与三态语义**写清楚**（不靠用户猜「部分」是什么意思）。
        if batch {
            body = body.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!(
                        "作用于选中的 {target_count} 项：勾选 = 全部打上，取消 = 全部摘掉；「部分」= 只有一部分项有它（点一下让全部都有）。"
                    )),
            );
        }

        dialog
            .title(title.clone())
            .w(cx.theme().font_size * ui::TAG_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("tag-close")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "tag-close".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("tag-apply")
                            .small()
                            .debug_selector(|| "tag-apply".to_string())
                            .label(if has_changes { "应用" } else { "关闭" })
                            .disabled(busy)
                            .on_click(apply_dispatch),
                    ),
            )
            .on_close({
                // `Rc` 不是 `Copy`：`Fn` 闭包（每帧重建）里要 clone 一份再 move 进去。
                let on_close = on_close.clone();
                move |_, _window, cx| on_close(cx)
            })
            .on_ok({
                // Enter = 应用（与主按钮同一条路）；新建走它自己的按钮。
                let dispatch_apply = dispatch_apply.clone();
                move |_, window, cx| {
                    dispatch_apply(window, cx);
                    true
                }
            })
            .on_cancel(|_, _, _| true)
    });
}

/// 打开「重命名标签」小对话框（从标签对话框的行菜单来）。
///
/// `name_input` 由调用方**在开窗前**建好并预填当前名字（同一条纪律，见模块头注释）。
/// `on_confirm(id, name, …)` 只在名字真的改了且不空时回调；回调后关窗。
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

#[cfg(test)]
mod tests {
    // 安全模式：测试模块不通配导入（会与 `gpui_kit` 的 `test` 宏展开自相残杀）。
    use super::{TagChoice, TagDialogState};

    fn choice(id: &str) -> TagChoice {
        TagChoice {
            id: id.to_string(),
            name: format!("标签{id}"),
            count: 1,
        }
    }

    #[test]
    fn diff_is_relative_to_the_baseline() {
        let state = TagDialogState::new(vec!["at_1".to_string()]);
        state.set_options(vec![choice("at_1"), choice("at_2"), choice("at_3")]);

        // 勾上一个新的、去掉原来的 → (add, remove) 各一个。
        state.toggle("at_2");
        state.toggle("at_1");
        assert_eq!(
            state.diff(),
            (vec!["at_2".to_string()], vec!["at_1".to_string()])
        );

        // 勾回去「at_1」：只剩 at_2 是新增。
        state.toggle("at_1");
        assert_eq!(state.diff(), (vec!["at_2".to_string()], Vec::new()));

        // 再取消 at_2：回到开窗时的状态，没有改动可提交。
        state.toggle("at_2");
        assert_eq!(state.diff(), (Vec::new(), Vec::new()));
    }

    /// 行内勾选框按**值**写（不是翻转）：重复写同一值不改变勾选顺序，也不会重复入列。
    #[test]
    fn set_checked_writes_by_value_and_is_idempotent() {
        let state = TagDialogState::new(Vec::new());
        state.set_options(vec![choice("at_1"), choice("at_2")]);

        state.set_checked("at_1", true);
        state.set_checked("at_2", true);
        assert_eq!(
            state.selected(),
            vec!["at_1".to_string(), "at_2".to_string()]
        );
        assert_eq!(state.diff().0.len(), 2, "两枚都是新增");

        // 重复写：不重复入列、不重置顺序。
        state.set_checked("at_1", true);
        assert_eq!(
            state.selected(),
            vec!["at_1".to_string(), "at_2".to_string()]
        );

        // 写 false 只摘掉这一枚；已经没勾的再写 false 不报错。
        state.set_checked("at_1", false);
        assert_eq!(state.selected(), vec!["at_2".to_string()]);
        state.set_checked("at_1", false);
        assert_eq!(state.selected(), vec!["at_2".to_string()]);
    }

    /// 多选三态：部分 → 全有（补打）→ 无（摘掉）；**部分集必须算进差集**。
    ///
    /// 为何单独铉：把「部分有」当布尔翻转，会让用户想“让全部都有”的那一下
    /// 反而把已挂的那些摘掉（一次没被要求的批量删除）。
    #[test]
    fn batch_toggle_promotes_partial_then_clears_and_the_diff_is_batch_correct() {
        let state = TagDialogState::new_batch(
            vec!["at_1".to_string()], // 两项都挂着
            vec!["at_2".to_string()], // 只有一项挂着
        );
        state.set_options(vec![choice("at_1"), choice("at_2"), choice("at_3")]);
        assert!(state.is_selected("at_1"));
        assert!(state.is_partial("at_2"));
        assert!(!state.is_partial("at_1"), "全有的不是部分");
        assert_eq!(state.diff(), (Vec::new(), Vec::new()), "开窗时无改动");

        // 点一下「部分」：提升为全有 → 差集里出现在 add（宿主会给缺的补上）
        state.promote_partial("at_2");
        assert!(!state.is_partial("at_2"));
        assert!(state.is_selected("at_2"));
        assert_eq!(state.diff(), (vec!["at_2".to_string()], Vec::new()));

        // 再点一下（现在是全有）：变成无 → 出现在 remove（宿主会从所有目标上摘掉）
        state.toggle("at_2");
        assert!(!state.is_selected("at_2"));
        let (add, remove) = state.diff();
        assert!(add.is_empty(), "取消不是新增：{add:?}");
        assert_eq!(
            remove,
            vec!["at_2".to_string()],
            "“部分 → 取消”也要摘掉（只算 baseline 会漏掉它）"
        );

        // 取消一个本来就是全有的 → 也在 remove 里（顺序：baseline 在前，baseline_partial 在后）
        state.toggle("at_1");
        assert_eq!(state.diff().1, vec!["at_1".to_string(), "at_2".to_string()]);

        // 勾一个全新的 → 只进 add
        state.toggle("at_3");
        assert_eq!(
            state.diff(),
            (
                vec!["at_3".to_string()],
                vec!["at_1".to_string(), "at_2".to_string()]
            )
        );
    }

    /// 行内勾选框的三态版：勾上 = 全有（含从部分提升）；取消 = 全无（部分也一并清）。
    #[test]
    fn three_way_checkbox_writes_full_have_or_none() {
        let state = TagDialogState::new_batch(Vec::new(), vec!["at_1".to_string()]);
        state.set_options(vec![choice("at_1")]);

        state.set_checked_three_way("at_1", true);
        assert!(state.is_selected("at_1") && !state.is_partial("at_1"));
        assert_eq!(state.diff().0, vec!["at_1".to_string()]);

        state.set_checked_three_way("at_1", false);
        assert!(!state.is_selected("at_1") && !state.is_partial("at_1"));
        assert_eq!(
            state.diff().1,
            vec!["at_1".to_string()],
            "取消后不能还原成“部分”（否则用户会看到它又自己回来了）"
        );
    }

    /// 多选重置：新基准含「部分」集，不会重复提交同一份改动。
    #[test]
    fn reset_batch_moves_both_baselines() {
        let state = TagDialogState::new_batch(Vec::new(), vec!["at_1".to_string()]);
        state.set_options(vec![choice("at_1"), choice("at_2")]);
        state.promote_partial("at_1");
        assert_eq!(state.diff().0, vec!["at_1".to_string()]);

        // 宿主动作完成后回推新状态：at_1 现在是全有，at_2 变成部分
        state.reset_batch(vec!["at_1".to_string()], vec!["at_2".to_string()]);
        assert!(state.is_selected("at_1") && state.is_partial("at_2"));
        assert_eq!(state.diff(), (Vec::new(), Vec::new()));
    }

    /// 标签被删（词典里没了）：勾选跟着清掉，否则会提交一个指向不存在标签的 add。
    #[test]
    fn set_options_drops_selections_that_no_longer_exist() {
        let state = TagDialogState::new(vec!["at_1".to_string()]);
        state.set_options(vec![choice("at_2")]);
        assert_eq!(state.selected(), Vec::<String>::new());
        assert_eq!(state.diff(), (Vec::new(), Vec::new()));
    }

    /// 动作成功后宿主重置选择：新状态即新基准，不会重复提交同一份改动。
    #[test]
    fn reset_selection_moves_the_baseline() {
        let state = TagDialogState::new(Vec::new());
        state.set_options(vec![choice("at_1")]);
        state.toggle("at_1");
        assert_eq!(state.diff().0, vec!["at_1".to_string()]);

        state.reset_selection(vec!["at_1".to_string()]);
        assert!(state.is_selected("at_1"));
        assert_eq!(state.diff(), (Vec::new(), Vec::new()));
    }
}
