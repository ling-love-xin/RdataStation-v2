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
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
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
    /// 存档显示名（标题用）。
    pub resource_name: String,
    /// 全部存活标签（按名字升序）。
    pub options: Vec<TagChoice>,
    /// 这条存档**已经**挂着的标签 id。
    pub selected: Vec<String>,
}

/// 对话框提交的动作（执行一律回宿主）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagDialogEvent {
    /// 提交这次勾选：`add` / `remove` 是相对开窗时的差集（两个都空就不该提交）。
    Apply { add: Vec<String>, remove: Vec<String> },
    /// 新建一个标签并**直接打上**（一次动作做完，不必先建再去勾）。
    CreateAndTag { name: String },
}

/// 对话框状态：**宿主也持一份克隆**（动作完成后换行与收放忙态）。
#[derive(Clone)]
pub struct TagDialogState {
    options: Rc<RefCell<Vec<TagChoice>>>,
    /// 当前勾选（开窗时 = 已挂的标签）。
    selected: Rc<RefCell<Vec<String>>>,
    /// 开窗时的勾选（算差集用；动作成功后由宿主重置为新的已挂集合）。
    baseline: Rc<RefCell<Vec<String>>>,
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
    /// 用初始勾选建状态（`selected` 同时进 baseline）。
    pub fn new(selected: Vec<String>) -> Self {
        Self {
            options: Rc::new(RefCell::new(Vec::new())),
            selected: Rc::new(RefCell::new(selected.clone())),
            baseline: Rc::new(RefCell::new(selected)),
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
        *self.options.borrow_mut() = options;
    }

    pub fn options(&self) -> Vec<TagChoice> {
        self.options.borrow().clone()
    }

    /// 换一份勾选，并把 baseline 对齐到它（动作成功后宿主调用：新状态即新的比较基准）。
    pub fn reset_selection(&self, selected: Vec<String>) {
        *self.selected.borrow_mut() = selected.clone();
        *self.baseline.borrow_mut() = selected;
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

    pub fn is_selected(&self, tag_id: &str) -> bool {
        self.selected.borrow().iter().any(|id| id == tag_id)
    }

    pub fn selected(&self) -> Vec<String> {
        self.selected.borrow().clone()
    }

    /// 本次改动的差集（`(add, remove)`）：两个都空 = 没有可提交的东西。
    pub fn diff(&self) -> (Vec<String>, Vec<String>) {
        let baseline = self.baseline.borrow();
        let selected = self.selected.borrow();
        let add = selected
            .iter()
            .filter(|id| !baseline.contains(id))
            .cloned()
            .collect();
        let remove = baseline
            .iter()
            .filter(|id| !selected.contains(id))
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
        state.reset_selection(seed.selected.clone());
    }
    let on_action: Rc<dyn Fn(TagDialogEvent, &mut Window, &mut App)> = Rc::new(on_action);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);
    let resource_name = seed.resource_name.clone();

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
            let dispatch = {
                let state = state.clone();
                let tag_id = option.id.clone();
                move |_: &ClickEvent, window: &mut Window, _cx: &mut App| {
                    state.toggle(&tag_id);
                    window.refresh();
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
                    .when(checked, |row| row.bg(theme.colors.accent.opacity(0.3)))
                    .when(can_edit, |row| {
                        row.hover(|s| s.bg(theme.colors.list_hover))
                    })
                    .when(can_edit, |row| row.on_click(dispatch))
                    .child(
                        div()
                            .w_3()
                            .flex_none()
                            .text_xs()
                            .text_color(if checked {
                                theme.colors.foreground
                            } else {
                                theme.colors.muted_foreground
                            })
                            .child(if checked { "✓" } else { "" }),
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
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("{}", option.count)),
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

        dialog
            .title(format!("标签 · {resource_name}"))
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
