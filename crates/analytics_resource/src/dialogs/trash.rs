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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::{DialogButtonProps, DialogFooter};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui;

/// 列表最多列多少条：回收站条目只增不减（谁也不会常来清），超出的**明说**，
/// 不静默截断，也不为这个对话框手搓虚拟化（行数真成问题时再上 `List`）。
pub const MAX_TRASH_ROWS: usize = 100;

/// 一行回收站条目（**宿主快照**：对话框只渲染与派发动作）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRow {
    /// 回收站条目 id（还原 / 永久删除要用）。
    pub trash_id: String,
    /// 原显示名（文件名 / 目录名）。
    pub name: String,
    /// 原位置（相对项目根，如 `resources/reports/dau.sql`）——"还原到哪"就靠它回答。
    pub original_label: String,
    /// 文件 / 目录（目录的"大小"没有意义，靠这一列区分）。
    pub kind_label: String,
    /// 删除时间（绝对时间：回收站是查凭证的地方，相对时间不够用）。
    pub time_label: String,
    /// 大小（目录为空）。
    pub size_label: String,
}

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

/// 打开回收站对话框（宿主入口）。
///
/// `on_action` 在用户点行内动作或「清空回收站」时回调一次；**是否忙由宿主说了算**
/// （宿主 `set_busy(true)` 打开，动作完成、新行推回来后 `set_busy(false)`）。
/// `on_close` 在对话框关闭后调用（宿主据此清掉会话记账）。
pub fn open_trash_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: TrashDialogSeed,
    state: TrashDialogState,
    on_action: impl Fn(TrashAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    open_trash_dialog_with(window, cx, seed, state, on_action, on_close);
}

/// 同上，但用调用方建好的状态（宿主持它做后续更新；窗口测试从这条缝进来）。
pub fn open_trash_dialog_with(
    window: &mut Window,
    cx: &mut App,
    seed: TrashDialogSeed,
    state: TrashDialogState,
    on_action: impl Fn(TrashAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    // 种子行灌进状态：之后一切渲染与动作都只读这一个来源（与版本历史同一个形态）。
    if state.rows().is_empty() {
        state.set_rows(seed.rows.clone());
        state.set_foreign(seed.foreign.clone());
    }
    let on_action: Rc<dyn Fn(TrashAction, &mut Window, &mut App)> = Rc::new(on_action);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let rows = state.rows();
        let foreign = state.foreign();
        let busy = state.busy();
        let read_only = state.read_only();
        let note = state.note();
        let can_write = !busy && !read_only;
        let shown = rows.len().min(MAX_TRASH_ROWS);

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
                            .child(format!("共 {} 项", rows.len())),
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
            );

        if rows.is_empty() {
            body = body.child(
                div()
                    .w_full()
                    .py_2()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("回收站是空的"),
            );
        } else {
            let mut list = div().v_flex().w_full();
            list = list.child(header_line(&theme));
            for (index, row) in rows.iter().take(shown).enumerate() {
                list = list.child(trash_line(
                    &theme,
                    row,
                    index,
                    can_write,
                    state.clone(),
                    on_action.clone(),
                ));
            }
            if rows.len() > shown {
                list = list.child(
                    div()
                        .w_full()
                        .py_1()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!(
                            "还有 {} 项未列出（一次最多列 {MAX_TRASH_ROWS} 项）",
                            rows.len() - shown
                        )),
                );
            }
            body = body.child(
                list.max_h(rems(ui::TRASH_LIST_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .into_any_element(),
            );
        }

        // 别人的条目：只解释，不给动作（还原归它自己的模块，删它更是越权）。
        for item in &foreign {
            body = body.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!(
                        "另有 {} 条「{}」的条目共用一个回收站——请在那个模块的界面里处理",
                        item.count, item.module_label
                    )),
            );
        }

        let empty_dispatch = {
            let state = state.clone();
            let on_action = on_action.clone();
            let count = rows.len();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let state = state.clone();
                let on_action = on_action.clone();
                window.open_alert_dialog(cx, move |alert, _window, _cx| {
                    let state = state.clone();
                    let on_action = on_action.clone();
                    alert
                        .confirm()
                        .title("清空资产库的回收站？")
                        .description(format!(
                            "将彻底删除资产库的 {count} 项（本体不再保留，不可恢复）；\
                             其他模块共用的条目不会被删。"
                        ))
                        .button_props(
                            DialogButtonProps::default()
                                .ok_text("清空")
                                .ok_variant(ButtonVariant::Danger)
                                .show_cancel(true),
                        )
                        .on_ok(move |_, window, cx| {
                            state.set_busy(true);
                            state.set_note(Some("正在清空…".to_string()));
                            on_action(TrashAction::Empty, window, cx);
                            true
                        })
                });
            }
        };

        dialog
            .title("回收站")
            .w(cx.theme().font_size * ui::TRASH_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("trash-empty")
                            .with_variant(ButtonVariant::Danger)
                            .small()
                            .debug_selector(|| "trash-empty".to_string())
                            .label("清空回收站")
                            .disabled(!can_write || rows.is_empty())
                            .on_click(empty_dispatch),
                    )
                    .child(
                        Button::new("trash-close")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "trash-close".to_string())
                            .label("关闭")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .on_close({
                // `Rc` 不是 `Copy`：`Fn` 闭包（每帧重建）里要 clone 一份再 move 进去。
                let on_close = on_close.clone();
                move |_, _window, cx| on_close(cx)
            })
            .on_cancel(|_, _, _| true)
    });
}

/// 表头（列名与下面各行的列宽一一对应）。
fn header_line(theme: &Theme) -> Div {
    div()
        .h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .border_b(px(1.0))
        .border_color(theme.colors.border)
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(div().w(rems(ui::TRASH_COL_NAME)).flex_none().child("名称"))
        .child(div().flex_1().min_w_0().child("原位置"))
        .child(div().w(rems(ui::TRASH_COL_TIME)).flex_none().child("删除时间"))
        .child(div().w(rems(ui::TRASH_COL_SIZE)).flex_none().child("大小"))
        .child(div().w(rems(ui::TRASH_COL_ACTION)).flex_none().child(""))
}

/// 一行回收站条目：名称（+ 类型）/ 原位置 / 时间 / 大小 + 行内动作。
///
/// 动作摆在行里而不是"选中后出现的动作栏"：这里一行最多两个动作、行内容短，
/// 装得下；再说"选中"在回收站里没有别的用处。
fn trash_line(
    theme: &Theme,
    row: &TrashRow,
    index: usize,
    can_write: bool,
    state: TrashDialogState,
    on_action: Rc<dyn Fn(TrashAction, &mut Window, &mut App)>,
) -> impl IntoElement {
    let muted = theme.colors.muted_foreground;

    let restore = {
        let state = state.clone();
        let on_action = on_action.clone();
        let trash_id = row.trash_id.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            state.set_busy(true);
            state.set_note(Some("正在还原…".to_string()));
            on_action(TrashAction::Restore { trash_id: trash_id.clone() }, window, cx);
        }
    };
    let purge = {
        let state = state.clone();
        let on_action = on_action.clone();
        let trash_id = row.trash_id.clone();
        let name = row.name.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            // 永久删除不可恢复（本体不留在任何地方）：先过确认框。
            let state = state.clone();
            let on_action = on_action.clone();
            let trash_id = trash_id.clone();
            // 外层闭包是 `Fn`（按钮可能被点多次）：要移进内层的东西每帧各拿一份。
            let name = name.clone();
            window.open_alert_dialog(cx, move |alert, _window, _cx| {
                let state = state.clone();
                let on_action = on_action.clone();
                let trash_id = trash_id.clone();
                alert
                    .confirm()
                    .title(format!("永久删除「{name}」？"))
                    .description("本体将从回收站里彻底删除，不可恢复；登记记录一并删除。")
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text("永久删除")
                            .ok_variant(ButtonVariant::Danger)
                            .show_cancel(true),
                    )
                    .on_ok(move |_, window, cx| {
                        state.set_busy(true);
                        state.set_note(Some("正在永久删除…".to_string()));
                        on_action(TrashAction::Purge { trash_id: trash_id.clone() }, window, cx);
                        true
                    })
            });
        }
    };

    div()
        .h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_sm()
        .hover(|s| s.bg(theme.colors.accent.opacity(0.3)))
        .child(
            div()
                .w(rems(ui::TRASH_COL_NAME))
                .flex_none()
                .min_w_0()
                .h_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .text_ellipsis()
                        .text_color(theme.colors.foreground)
                        .child(row.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(row.kind_label.clone()),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_ellipsis()
                .text_color(muted)
                .child(row.original_label.clone()),
        )
        .child(
            div()
                .w(rems(ui::TRASH_COL_TIME))
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(row.time_label.clone()),
        )
        .child(
            div()
                .w(rems(ui::TRASH_COL_SIZE))
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(row.size_label.clone()),
        )
        .child(
            div()
                .w(rems(ui::TRASH_COL_ACTION))
                .flex_none()
                .h_flex()
                .items_center()
                .gap_1()
                .child(
                    Button::new(("trash-restore", index))
                        .ghost()
                        .small()
                        .debug_selector(move || format!("trash-restore-{index}"))
                        .label("还原")
                        .disabled(!can_write)
                        .on_click(restore),
                )
                .child(
                    Button::new(("trash-purge", index))
                        .ghost()
                        .small()
                        .debug_selector(move || format!("trash-purge-{index}"))
                        .label("永久删除")
                        .disabled(!can_write)
                        .on_click(purge),
                ),
        )
}

#[cfg(test)]
mod tests {
    // 安全模式：测试模块不通配导入（会与 `gpui_kit` 的 `test` 宏展开自相残杀，`resource_view` 踩过）。
    use super::{ForeignTrash, TrashDialogState, TrashRow};

    fn row(id: &str) -> TrashRow {
        TrashRow {
            trash_id: id.to_string(),
            name: format!("dau_{id}.sql"),
            original_label: format!("resources/dau_{id}.sql"),
            kind_label: "文件".to_string(),
            time_label: "2026-09-18 10:00".to_string(),
            size_label: "1.2 KB".to_string(),
        }
    }

    /// 行数据可被宿主整体替换（动作完成后重取）：换行不保留旧的 id（那是指向已不在列表里的东西）。
    #[test]
    fn set_rows_replaces_the_previous_snapshot() {
        let state = TrashDialogState::new();
        state.set_rows(vec![row("a"), row("b")]);
        assert_eq!(state.rows().len(), 2);

        state.set_rows(vec![row("c")]);
        assert_eq!(state.rows().len(), 1);
        assert_eq!(state.rows()[0].trash_id, "c");
    }

    /// 别人的条目只挂说明，不混进行列表（混进来等于摆一个必然被拒的「还原」）。
    #[test]
    fn foreign_entries_are_kept_out_of_the_row_list() {
        let state = TrashDialogState::new();
        state.set_rows(vec![row("a")]);
        state.set_foreign(vec![ForeignTrash {
            module_label: "草稿箱".to_string(),
            count: 2,
        }]);
        assert_eq!(state.rows().len(), 1);
        assert_eq!(state.foreign()[0].count, 2);
        assert_eq!(state.foreign()[0].module_label, "草稿箱");
    }

    #[test]
    fn busy_and_read_only_gate_actions() {
        let state = TrashDialogState::new();
        assert!(!state.busy() && !state.read_only());
        state.set_busy(true);
        state.set_read_only(true);
        assert!(state.busy() && state.read_only());
    }
}
