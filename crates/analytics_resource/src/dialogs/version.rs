//! 版本历史对话框（原型 §4.3）。
//!
//! 版本**只追加**（原型 §11 决策 9）：还原 = 用旧内容生成新版本，不覆盖历史、不做行级
//! diff（文本 diff 归编辑器）。这个对话框回答三件事：还有哪些版本、哪一版的内容副本
//! 已经不在了、把选中的那一版拿去干嘛。
//!
//! 分工与另三个对话框一致：**行数据与动作执行都在宿主**（本文件不碰文件系统、不认识
//! `ArchiveService`），这里只做选中与确认。行数据可以被宿主在动作完成后**替换**
//! （`VersionDialogState::set_rows`）——所以还原完不必关窗重开，列表自己就换新了。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui;

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
}

impl VersionAction {
    /// 版本号（三种动作都要它）。
    pub fn version(self) -> i32 {
        match self {
            Self::Restore { version }
            | Self::CheckoutDraft { version }
            | Self::DeleteCopy { version } => version,
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
        self.rows.borrow().iter().find(|row| row.version == selected).cloned()
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

/// 打开版本历史对话框（宿主入口）。
///
/// `on_action` 在用户点动作栏时回调一次（带版本号）；**是否忙由宿主说了算**：
/// 宿主把 `state.set_busy(true)` 打开，动作完成、新行推回来后 `set_busy(false)`。
/// `on_close` 在对话框关闭后调用（宿主据此清掉"当前开着的那一个"的记账）。
pub fn open_version_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: VersionDialogSeed,
    state: VersionDialogState,
    on_action: impl Fn(VersionAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    open_version_dialog_with(window, cx, seed, state, on_action, on_close);
}

/// 同上，但用调用方建好的状态（宿主持它做后续更新；窗口测试从这条缝进来）。
pub fn open_version_dialog_with(
    window: &mut Window,
    cx: &mut App,
    seed: VersionDialogSeed,
    state: VersionDialogState,
    on_action: impl Fn(VersionAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    // 种子行灌进状态：之后一切渲染与动作都只读这一个来源。
    if state.rows().is_empty() {
        state.set_rows(seed.rows.clone());
    }
    let on_action: Rc<dyn Fn(VersionAction, &mut Window, &mut App)> = Rc::new(on_action);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);
    let name = seed.name.clone();
    let current_version = seed.current_version;

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let rows = state.rows();
        let selected = state.selected();
        let busy = state.busy();
        let read_only = state.read_only();
        let note = state.note();
        let shown = rows.len().min(MAX_VERSION_ROWS);

        let mut list = div().v_flex().w_full();
        list = list.child(header_line(theme));
        for row in rows.iter().take(shown) {
            list = list.child(version_line(
                theme,
                row,
                selected == Some(row.version),
                state.clone(),
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
                        "还有 {} 个更早的版本未列出（一次最多列 {MAX_VERSION_ROWS} 个）",
                        rows.len() - shown
                    )),
            );
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
                            .child(format!("共 {} 个版本", rows.len())),
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
                // 行多时滚动（带滚动的区域返回的不是 `Div`）。
                list.max_h(rems(ui::VERSION_LIST_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .into_any_element(),
            );

        if let Some(row) = state.selected_row() {
            body = body.child(action_bar(
                theme,
                &row,
                busy,
                read_only,
                state.clone(),
                on_action.clone(),
            ));
        }

        dialog
            .title(format!("版本历史 · {name}"))
            .w(cx.theme().font_size * ui::VERSION_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new().child(
                    Button::new("version-close")
                        .with_variant(ButtonVariant::Secondary)
                        .small()
                        .debug_selector(|| "version-close".to_string())
                        .label("关闭")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
            // 副标题位摆当前版本号：列表第一行也有，但这里回答"我现在在第几版"更快。
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("当前版本 v{current_version}")),
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
    let muted = theme.colors.muted_foreground;
    div()
        .h_flex()
        .w_full()
        .items_center()
        .gap_3()
        .px_2()
        .py_1()
        .border_b(px(1.0))
        .border_color(theme.colors.border)
        .text_xs()
        .text_color(muted)
        .child(div().w(rems(ui::VERSION_COL_VERSION)).flex_none().child("版本"))
        .child(div().w(rems(ui::VERSION_COL_TIME)).flex_none().child("时间"))
        .child(div().w(rems(ui::VERSION_COL_SIZE)).flex_none().child("大小"))
        .child(div().w(rems(ui::VERSION_COL_HASH)).flex_none().child("指纹"))
        .child(div().flex_1().min_w_0().child("变化"))
        .child(div().w(rems(ui::VERSION_COL_COPY)).flex_none().child("副本"))
}

/// 一行版本（点击 = 选中；选中后动作栏才出现——768px 里塞不下每行三个按钮）。
fn version_line(
    theme: &Theme,
    row: &VersionRow,
    selected: bool,
    state: VersionDialogState,
) -> impl IntoElement {
    let (foreground, muted, danger, success) = {
        let colors = theme.colors;
        (
            colors.foreground,
            colors.muted_foreground,
            colors.danger,
            colors.success,
        )
    };
    let version = row.version;
    let toggle = {
        let state = state.clone();
        move |_: &ClickEvent, _window: &mut Window, _cx: &mut App| {
            state.set_selected(Some(version));
        }
    };

    div()
        .id(SharedString::from(format!("version-row-{version}")))
        // 调试选择器：窗口测试按它定位并真点（选中态只有这一处落笔）。
        .debug_selector(move || format!("version-row-{version}"))
        .h_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .gap_3()
        .px_2()
        .py_1()
        .cursor_pointer()
        .text_xs()
        .when(selected, |line| line.bg(theme.colors.list_active))
        .hover(|style| style.bg(theme.colors.list_hover))
        .on_click(toggle)
        .child(
            div()
                .h_flex()
                .w(rems(ui::VERSION_COL_VERSION))
                .flex_none()
                .items_center()
                .gap_1()
                .child(div().text_color(foreground).child(format!("v{version}")))
                .when(row.is_current, |cell| {
                    cell.child(div().text_color(success).child("当前"))
                }),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_TIME))
                .flex_none()
                .text_ellipsis()
                .text_color(muted)
                .child(row.time_label.clone()),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_SIZE))
                .flex_none()
                .text_ellipsis()
                .text_color(muted)
                .child(row.size_label.clone()),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_HASH))
                .flex_none()
                .text_ellipsis()
                .text_color(muted)
                .child(row.hash_short.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .text_color(muted)
                .child(row.delta_label.clone()),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_COPY))
                .flex_none()
                .text_ellipsis()
                .child(if row.has_copy {
                    div().text_color(muted).child("在位")
                } else {
                    div().text_color(danger).child("副本缺失")
                }),
        )
}

/// 动作栏：选中一行后才出现（原型 §4.3 的取舍：表格装不下每行三个按钮）。
fn action_bar(
    theme: &Theme,
    row: &VersionRow,
    busy: bool,
    read_only: bool,
    state: VersionDialogState,
    on_action: Rc<dyn Fn(VersionAction, &mut Window, &mut App)>,
) -> Div {
    let muted = theme.colors.muted_foreground;
    let version = row.version;
    // 当前版本不走这两个动作（它就是本体）：取回走详情面板的「取回（检出）…」。
    let can_act = !row.is_current && row.has_copy && !busy && !read_only;
    let hint = if read_only {
        "项目处于只读模式：还原 / 取回 / 删副本都是写操作"
    } else if row.is_current {
        "这是当前版本（本体上的内容）：取回请用详情面板的「取回（检出）…」"
    } else if !row.has_copy {
        "该版本只留了元数据（内容副本已被保留策略裁剪），不能还原或取回"
    } else {
        "还原 = 用这一版的内容生成新版本；取回 = 复制一份可写草稿（本体不动）"
    };

    let restore = {
        let state = state.clone();
        let on_action = on_action.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            state.set_selected(Some(version));
            state.set_busy(true);
            state.set_note(Some(format!("正在还原 v{version}…")));
            on_action(VersionAction::Restore { version }, window, cx);
        }
    };
    let checkout = {
        let state = state.clone();
        let on_action = on_action.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            state.set_busy(true);
            state.set_note(Some(format!("正在取回 v{version}…")));
            on_action(VersionAction::CheckoutDraft { version }, window, cx);
        }
    };
    // 删副本不可恢复（不进回收站）：先过一个确认框，再交给宿主。
    let delete = {
        let state = state.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let state = state.clone();
            let on_action = on_action.clone();
            window.open_alert_dialog(cx, move |alert, _window, _cx| {
                let state = state.clone();
                let on_action = on_action.clone();
                alert
                    .title(format!("删除 v{version} 的内容副本？"))
                    .description(
                        "版本记录会保留（列表里仍能看到这一版），但它的内容将无法再还原或取回。\
                         副本不入回收站，删除后不可恢复。",
                    )
                    .show_cancel(true)
                    .on_ok(move |_, window, cx| {
                        state.set_busy(true);
                        state.set_note(Some(format!("正在删除 v{version} 的内容副本…")));
                        on_action(VersionAction::DeleteCopy { version }, window, cx);
                        true
                    })
            });
        }
    };

    div()
        .v_flex()
        .w_full()
        .gap_1()
        .border_t(px(1.0))
        .border_color(theme.colors.border)
        .pt_2()
        .child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .child(
                    Button::new("version-restore")
                        .small()
                        .debug_selector(|| "version-restore".to_string())
                        .label(if row.is_current {
                            "当前版本".to_string()
                        } else {
                            format!("还原为当前版本（生成 v{}）", row.version + 1)
                        })
                        .disabled(!can_act)
                        .on_click(restore),
                )
                .child(
                    Button::new("version-checkout")
                        .ghost()
                        .small()
                        .debug_selector(|| "version-checkout".to_string())
                        .label("取回该版本为草稿")
                        .disabled(!can_act)
                        .on_click(checkout),
                )
                .child(
                    Button::new("version-delete-copy")
                        .ghost()
                        .small()
                        .debug_selector(|| "version-delete-copy".to_string())
                        .label("删除内容副本")
                        .disabled(!can_act)
                        .on_click(delete),
                ),
        )
        .child(div().text_xs().text_color(muted).child(hint))
}

#[cfg(test)]
mod tests {
    use super::{VersionAction, VersionDialogState, VersionRow};

    fn row(version: i32, is_current: bool) -> VersionRow {
        VersionRow {
            version,
            is_current,
            time_label: "2026-09-17 08:00".to_string(),
            size_label: "1.2 KB".to_string(),
            hash_short: "0123456789ab".to_string(),
            has_copy: true,
            delta_label: String::new(),
        }
    }

    #[test]
    fn set_rows_clears_selection_that_no_longer_exists() {
        let state = VersionDialogState::new();
        state.set_rows(vec![row(2, true), row(1, false)]);
        state.set_selected(Some(1));

        // 动作后新行里没有 v1 了（被裁 / 列表变了）：选中必须清掉。
        state.set_rows(vec![row(3, true), row(2, false)]);
        assert_eq!(state.selected(), None);

        state.set_selected(Some(2));
        state.set_rows(vec![row(3, true), row(2, false)]);
        assert_eq!(state.selected(), Some(2), "还在的行保留选中");
    }

    #[test]
    fn selected_row_follows_the_selection() {
        let state = VersionDialogState::new();
        state.set_rows(vec![row(2, true), row(1, false)]);
        assert!(state.selected_row().is_none(), "没选中就没有行");

        state.set_selected(Some(1));
        let selected = state.selected_row().expect("selected row");
        assert_eq!(selected.version, 1);
        assert!(!selected.is_current);
    }

    #[test]
    fn action_carries_its_version() {
        assert_eq!(VersionAction::Restore { version: 3 }.version(), 3);
        assert_eq!(VersionAction::CheckoutDraft { version: 1 }.version(), 1);
        assert_eq!(VersionAction::DeleteCopy { version: 7 }.version(), 7);
    }
}
