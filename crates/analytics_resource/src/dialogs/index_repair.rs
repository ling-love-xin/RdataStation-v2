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

/// 每组最多列多少行：再多只列前 N 并**明说**（扫描一次可能报出成百个未登记文件）。
pub const MAX_REPAIR_ROWS_PER_GROUP: usize = 50;

/// 修复行所属的分组（与原型 §4.5 的三组一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairGroup {
    /// 有文件、无记录。
    Untracked,
    /// 有记录、无本体。
    Missing,
    /// 指纹不匹配。
    Changed,
}

impl RepairGroup {
    /// 分组顺序（原型 §4.5 自上而下，也是"越靠上越接近新增"的顺序）。
    pub const ALL: [Self; 3] = [Self::Untracked, Self::Missing, Self::Changed];

    /// 分组标题。
    pub fn title(self) -> &'static str {
        match self {
            Self::Untracked => "有文件、无记录",
            Self::Missing => "有记录、无本体",
            Self::Changed => "指纹不匹配",
        }
    }

    /// 分组说明：一句话讲清这一组是什么、动作会做什么。
    pub fn hint(self) -> &'static str {
        match self {
            Self::Untracked => "本体在 resources/ 里但登记表不认识它；补登后就是一个正常存档",
            Self::Missing => "本体不在了：删掉记录，或等回收站上提后从回收站还原",
            Self::Changed => "本体被绕过只读改过：接受当前内容（生成新版本），或到版本历史里挑一版还原",
        }
    }
}

/// 一行待修复项（**宿主快照**：对话框只渲染与派发动作，不查文件系统）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairRow {
    pub group: RepairGroup,
    /// 行标题：未登记的是文件名，另两类是存档名。
    pub title: String,
    /// 副文案：本体相对路径 / 期望位置。
    pub detail: String,
    /// 指纹对照（仅 `Changed` 行有值）。
    pub hash_detail: String,
    /// 未登记行的本体相对路径（补登要用）。
    pub rel_path: String,
    /// 另两类的存档 id（删记录 / 接受内容 / 打开版本历史要用）。
    pub resource_id: Option<String>,
}

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
    /// 打开版本历史（挑一版还原）。
    OpenVersions { resource_id: String },
}

/// 打开索引修复对话框（宿主入口）。
///
/// `on_action` 在用户点行内动作时回调一次；**是否忙由宿主说了算**（动作完成、新行推回来后
/// `set_busy(false)`）。`on_close` 在对话框关闭后调用（宿主据此清掉会话记账）。
pub fn open_index_repair_dialog(
    window: &mut Window,
    cx: &mut App,
    state: RepairDialogState,
    on_action: impl Fn(RepairAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    open_index_repair_dialog_with(window, cx, state, on_action, on_close);
}

/// 同上（窗口测试从这条缝进来）。
pub fn open_index_repair_dialog_with(
    window: &mut Window,
    cx: &mut App,
    state: RepairDialogState,
    on_action: impl Fn(RepairAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    let on_action: Rc<dyn Fn(RepairAction, &mut Window, &mut App)> = Rc::new(on_action);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let rows = state.rows();
        let busy = state.busy();
        let read_only = state.read_only();
        let note = state.note();

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
                            .child(format!("共 {} 项需要处理", rows.len())),
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
            // 干净 = 好消息，用 success 而不是警告色：对话框开着时用户可能刚修完最后一项。
            body = body.child(
                div()
                    .w_full()
                    .py_2()
                    .text_xs()
                    .text_color(theme.colors.success)
                    .child("索引与本体一致：没有需要处理的问题"),
            );
        } else {
            let mut groups = div().v_flex().w_full().gap_2();
            for group in RepairGroup::ALL {
                let in_group: Vec<&RepairRow> =
                    rows.iter().filter(|row| row.group == group).collect();
                if in_group.is_empty() {
                    continue;
                }
                let shown = in_group.len().min(MAX_REPAIR_ROWS_PER_GROUP);
                let mut section = div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.colors.foreground)
                                    .child(group.title()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(format!("{}", in_group.len())),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(group.hint()),
                    );
                for (index, row) in in_group.iter().take(shown).enumerate() {
                    section = section.child(repair_line(
                        theme,
                        row,
                        index,
                        busy,
                        read_only,
                        state.clone(),
                        on_action.clone(),
                    ));
                }
                if in_group.len() > shown {
                    section = section.child(
                        div()
                            .w_full()
                            .py_1()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!(
                                "还有 {} 项未列出（每组一次最多列 {MAX_REPAIR_ROWS_PER_GROUP} 项）",
                                in_group.len() - shown
                            )),
                    );
                }
                groups = groups.child(section);
            }
            body = body.child(
                groups
                    .max_h(rems(ui::REPAIR_LIST_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .into_any_element(),
            );
        }

        dialog
            .title("索引修复")
            .w(cx.theme().font_size * ui::REPAIR_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new().child(
                    Button::new("repair-close")
                        .with_variant(ButtonVariant::Secondary)
                        .small()
                        .debug_selector(|| "repair-close".to_string())
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

/// 一行待修复项：标题 / 副文案 / （指纹对照）+ 行内动作。
fn repair_line(
    theme: &Theme,
    row: &RepairRow,
    index: usize,
    busy: bool,
    read_only: bool,
    state: RepairDialogState,
    on_action: Rc<dyn Fn(RepairAction, &mut Window, &mut App)>,
) -> impl IntoElement {
    let muted = theme.colors.muted_foreground;
    let can_write = !busy && !read_only;
    let mut line = div()
        .h_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(theme.colors.list_hover)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .v_flex()
                .gap_0p5()
                .child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(theme.colors.foreground)
                        .child(row.title.clone()),
                )
                .child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(muted)
                        .child(row.detail.clone()),
                )
                .when(!row.hash_detail.is_empty(), |cell| {
                    cell.child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(muted)
                            .child(row.hash_detail.clone()),
                    )
                }),
        );

    match row.group {
        RepairGroup::Untracked => {
            let rel_path = row.rel_path.clone();
            let dispatch = {
                let state = state.clone();
                let on_action = on_action.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    state.set_busy(true);
                    state.set_note(Some("正在补登…".to_string()));
                    on_action(RepairAction::Adopt { rel_path: rel_path.clone() }, window, cx);
                }
            };
            line = line.child(
                Button::new(("repair-adopt", index))
                    .small()
                    .debug_selector(move || format!("repair-adopt-{index}"))
                    .label("补登为存档")
                    .disabled(!can_write)
                    .on_click(dispatch),
            );
        }
        RepairGroup::Missing => {
            let resource_id = row.resource_id.clone().unwrap_or_default();
            // 「从回收站还原」要等 `ProjectTrash` 上提（P0.8）：摆出来并说明，而不是藏起来
            // ——藏起来用户会以为"没有这个能力"，摆出来能说清"为什么现在没有"。
            line = line.child(
                Button::new(("repair-restore", index))
                    .ghost()
                    .small()
                    .disabled(true)
                    .label("从回收站还原")
                    .tooltip("等项目级回收站上提（P0.8）后才可用"),
            );
            let dispatch = {
                let state = state.clone();
                let on_action = on_action.clone();
                let resource_id = resource_id.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    // 删记录不可恢复（本体都没了，没有可还原的东西）：先过确认框。
                    let state = state.clone();
                    let on_action = on_action.clone();
                    let resource_id = resource_id.clone();
                    window.open_alert_dialog(cx, move |alert, _window, _cx| {
                        let state = state.clone();
                        let on_action = on_action.clone();
                        let resource_id = resource_id.clone();
                        alert
                            .title("删除这条登记记录？")
                            .description(
                                "本体已经不存在，删掉记录后这个存档就彻底没了（不含回收站）。",
                            )
                            .show_cancel(true)
                            .on_ok(move |_, window, cx| {
                                state.set_busy(true);
                                state.set_note(Some("正在删除记录…".to_string()));
                                on_action(
                                    RepairAction::DeleteRecord {
                                        resource_id: resource_id.clone(),
                                    },
                                    window,
                                    cx,
                                );
                                true
                            })
                    });
                }
            };
            line = line.child(
                Button::new(("repair-delete", index))
                    .ghost()
                    .small()
                    .debug_selector(move || format!("repair-delete-{index}"))
                    .label("删除记录")
                    .disabled(!can_write)
                    .on_click(dispatch),
            );
        }
        RepairGroup::Changed => {
            let resource_id = row.resource_id.clone().unwrap_or_default();
            let accept = {
                let state = state.clone();
                let on_action = on_action.clone();
                let resource_id = resource_id.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    state.set_busy(true);
                    state.set_note(Some("正在接受当前内容…".to_string()));
                    on_action(
                        RepairAction::AcceptContent {
                            resource_id: resource_id.clone(),
                        },
                        window,
                        cx,
                    );
                }
            };
            let open_versions = {
                let resource_id = resource_id.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    on_action(
                        RepairAction::OpenVersions {
                            resource_id: resource_id.clone(),
                        },
                        window,
                        cx,
                    );
                }
            };
            line = line
                .child(
                    Button::new(("repair-accept", index))
                        .small()
                        .debug_selector(move || format!("repair-accept-{index}"))
                        .label("接受当前内容")
                        .disabled(!can_write)
                        .on_click(accept),
                )
                // 跳转不受只读与忙态影响（它不改任何状态）。
                .child(
                    Button::new(("repair-versions", index))
                        .ghost()
                        .small()
                        .debug_selector(move || format!("repair-versions-{index}"))
                        .label("打开版本历史…")
                        .on_click(open_versions),
                );
        }
    }

    line
}

#[cfg(test)]
mod tests {
    use super::{RepairAction, RepairDialogState, RepairGroup, RepairRow};

    fn row(group: RepairGroup, title: &str) -> RepairRow {
        RepairRow {
            group,
            title: title.to_string(),
            detail: "resources/x.sql".to_string(),
            hash_detail: String::new(),
            rel_path: "x.sql".to_string(),
            resource_id: Some("ar_1".to_string()),
        }
    }

    #[test]
    fn groups_follow_the_prototype_order() {
        let titles: Vec<&str> = RepairGroup::ALL.iter().map(|group| group.title()).collect();
        assert_eq!(titles, vec!["有文件、无记录", "有记录、无本体", "指纹不匹配"]);
        for group in RepairGroup::ALL {
            assert!(!group.hint().is_empty(), "每组都要有一句说明");
        }
    }

    #[test]
    fn set_rows_replaces_wholesale_and_busy_is_independent() {
        let state = RepairDialogState::new();
        assert!(state.rows().is_empty());
        state.set_rows(vec![row(RepairGroup::Untracked, "a.sql")]);
        assert_eq!(state.rows().len(), 1);

        // 修完一项后宿主换一批（可能变空）：行集合整体替换，没有选中态要清理。
        state.set_rows(Vec::new());
        assert!(state.rows().is_empty());

        state.set_busy(true);
        assert!(state.busy());
        state.set_busy(false);
        assert!(!state.busy());
    }

    #[test]
    fn actions_carry_what_the_host_needs() {
        // 三类动作各带走一个字段：补登带相对路径，另两个带存档 id。
        let adopt = RepairAction::Adopt {
            rel_path: "reports/a.sql".to_string(),
        };
        let RepairAction::Adopt { rel_path } = adopt else {
            unreachable!("刚构造的就是 Adopt")
        };
        assert_eq!(rel_path, "reports/a.sql");

        let accept = RepairAction::AcceptContent {
            resource_id: "ar_9".to_string(),
        };
        let RepairAction::AcceptContent { resource_id } = accept else {
            unreachable!("刚构造的就是 AcceptContent")
        };
        assert_eq!(resource_id, "ar_9");
    }
}
