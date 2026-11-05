//! 探查确认窗口（C3：源目标进入洞察先定数据量与列；C6：重取数/重探查复用）——
//! **模态弹窗的表单体**（2026-11-02 前内嵌在中央区，用户拍板改弹出）。
//!
//! 语义对齐 `docs/architecture/insight/insight-probe-config-decision.md` §5 窗口规格：
//! 数据量块（全量 / 常用行数 / 常用百分比快捷入口）→ 采样方式块（用户选择 + 差异说明
//! + seed）→ 探查列块（全部 / 选择 + 候选清单）→ 过滤块（可选 WHERE）→ 提示区
//! （必含「探查全部」）→ 底部 取消 / 确认探查。
//!
//! 为什么是**独立实体**：对话框 builder 闭包只跑一次，纯 `Div` 是静态快照——
//! Radio 切换 / 预设点击 / 行内提示都不会刷新；实体元素（`rule_view` 同款）经
//! `observe(view)` 传播 notify，随 `InsightView` 状态逐帧重渲。
//! 弹窗的开/关由 `InsightWorkspacePanel::sync_probe_modal` 驱动（观察
//! `probe_dialog` 状态变迁，经存下的窗口句柄调 `open_dialog` / `close_dialog`）。
//!
//! 渲染只读 `view.probe_dialog()`；编辑经回调写回 `InsightView` 的确认窗口状态
//! （`probe_set_*`），输入框本身用 `use_keyed_state` 惰性创建（同 sample.rs 模式：
//! 输入权威在 InputState，Change 订阅回写 dialog）。

use super::*;
use crate::insight_view::{ProbeAmountMode, ProbeDialogState};
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::separator::Separator;
use std::cell::RefCell;

/// 探查确认弹窗的表单体（对话框架构下逐帧重渲；状态权威在 [`InsightView`]）。
pub(crate) struct ProbeDialogBody {
    /// 探查状态的唯一权威（编辑经 `probe_set_*` 写回）
    view: Entity<InsightView>,
    /// 四个输入框（行数 / 百分比 / 过滤 / 种子）的 Change 订阅（只挂一次）
    input_subs: RefCell<Vec<Subscription>>,
    focus_handle: FocusHandle,
}

impl ProbeDialogBody {
    pub(crate) fn new(view: Entity<InsightView>, cx: &mut Context<Self>) -> Self {
        // view 变（radio / 预设 / 候选列 / 行内提示全是 view 状态）→ 本体重渲
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        Self {
            view,
            input_subs: RefCell::new(Vec::new()),
            focus_handle: cx.focus_handle(),
        }
    }

    fn render_form(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        dialog: &ProbeDialogState,
        theme: &Theme,
    ) -> Div {
        let view = self.view.clone();
        // 数据量 Radio（模态化改写时 on_change 曾遗漏——点选无反应的回归，2026-11-06 补回）
        let amount_index = match dialog.amount_mode {
            ProbeAmountMode::Full => 0,
            ProbeAmountMode::RowCount => 1,
            ProbeAmountMode::Percent => 2,
        };
        let amount_group = RadioGroup::horizontal("insight-probe-amount")
            .selected_index(Some(amount_index))
            .children([
                Radio::new("insight-probe-amount-full")
                    .label("全量")
                    .checked(amount_index == 0),
                Radio::new("insight-probe-amount-rows")
                    .label("行数")
                    .checked(amount_index == 1),
                Radio::new("insight-probe-amount-pct")
                    .label("百分比")
                    .checked(amount_index == 2),
            ])
            .on_change({
                let view = view.clone();
                move |index, _, app| {
                    let mode = match *index {
                        0 => ProbeAmountMode::Full,
                        1 => ProbeAmountMode::RowCount,
                        _ => ProbeAmountMode::Percent,
                    };
                    view.update(app, |v, cx| v.probe_set_amount_mode(mode, cx));
                }
            });

        // 列集合：单选「全部列 / 选择列」
        let columns_all = matches!(dialog.columns, crate::model::ProbeColumns::All);
        let sample_index = match dialog.sample_mode {
            crate::model::ProbeSampleMode::Head => 0,
            crate::model::ProbeSampleMode::Reservoir => 1,
        };

        let sample_group = RadioGroup::horizontal("insight-probe-sample")
            .selected_index(Some(sample_index))
            .children([
                Radio::new("insight-probe-sample-head")
                    .label("顺序取前 N 行")
                    .checked(sample_index == 0),
                Radio::new("insight-probe-sample-reservoir")
                    .label("均匀随机取样")
                    .checked(sample_index == 1),
            ])
            .on_change({
                let view = view.clone();
                move |index, _, app| {
                    let mode = if *index == 0 {
                        crate::model::ProbeSampleMode::Head
                    } else {
                        crate::model::ProbeSampleMode::Reservoir
                    };
                    view.update(app, |v, cx| v.probe_set_sample_mode(mode, cx));
                }
            });

        let columns_group = RadioGroup::horizontal("insight-probe-columns")
            .selected_index(Some(if columns_all { 0 } else { 1 }))
            .children([
                Radio::new("insight-probe-cols-all")
                    .label("全部列")
                    .checked(columns_all),
                Radio::new("insight-probe-cols-subset")
                    .label("选择列")
                    .checked(!columns_all),
            ])
            .on_change({
                let view = view.clone();
                let candidates = dialog.column_candidates.clone();
                move |index, _, app| {
                    let cols = if *index == 0 {
                        crate::model::ProbeColumns::All
                    } else {
                        crate::model::ProbeColumns::Subset(candidates.clone())
                    };
                    view.update(app, |v, cx| v.probe_set_columns(cols, cx));
                }
            });

        // 快捷预设（全量 / 常用行数 / 常用百分比）：点一下即切模式 + 填值；
        // 当前值命中哪个 chip 就高亮哪个（toggled），避免顶部三选一与 chips 两层重复。
        let quick_presets: [(ProbeAmountMode, &str, &str); 7] = [
            (ProbeAmountMode::Full, "全量", ""),
            (ProbeAmountMode::RowCount, "500 行", "500"),
            (ProbeAmountMode::RowCount, "1,000 行", "1000"),
            (ProbeAmountMode::RowCount, "5,000 行", "5000"),
            (ProbeAmountMode::Percent, "5%", "5"),
            (ProbeAmountMode::Percent, "10%", "10"),
            (ProbeAmountMode::Percent, "25%", "25"),
        ];
        let is_quick_active = |mode: ProbeAmountMode, value: &str| -> bool {
            if dialog.amount_mode != mode {
                return false;
            }
            match mode {
                ProbeAmountMode::Full => true,
                ProbeAmountMode::RowCount => dialog.row_count.trim() == value,
                ProbeAmountMode::Percent => dialog.percent.trim() == value,
            }
        };

        // 输入框（渲染管线惰性创建；权威在 InputState，Change 订阅回写 dialog）
        let row_state = window.use_keyed_state(
            SharedString::from("insight-probe-rows"),
            cx,
            |window, cx| InputState::new(window, cx).placeholder("如 500"),
        );
        if dialog.row_count != row_state.read(cx).value() {
            row_state.update(cx, |state, cx| {
                state.set_value(dialog.row_count.clone(), window, cx);
            });
        }
        if self.input_subs.borrow().is_empty() {
            let sub = cx.subscribe_in(
                &row_state,
                window,
                |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let value = emitter.read(cx).value().to_string();
                    this.view
                        .update(cx, |v, cx| v.probe_set_row_count(&value, cx));
                },
            );
            self.input_subs.borrow_mut().push(sub);
        }

        let pct_state =
            window.use_keyed_state(SharedString::from("insight-probe-pct"), cx, |window, cx| {
                InputState::new(window, cx).placeholder("如 5")
            });
        if dialog.percent != pct_state.read(cx).value() {
            pct_state.update(cx, |state, cx| {
                state.set_value(dialog.percent.clone(), window, cx);
            });
        }
        if self.input_subs.borrow().len() < 2 {
            let sub = cx.subscribe_in(
                &pct_state,
                window,
                |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let value = emitter.read(cx).value().to_string();
                    this.view
                        .update(cx, |v, cx| v.probe_set_percent(&value, cx));
                },
            );
            self.input_subs.borrow_mut().push(sub);
        }

        let filter_state = window.use_keyed_state(
            SharedString::from("insight-probe-filter"),
            cx,
            |window, cx| InputState::new(window, cx).placeholder("如 status = 'active'"),
        );
        if dialog.filter != filter_state.read(cx).value() {
            filter_state.update(cx, |state, cx| {
                state.set_value(dialog.filter.clone(), window, cx);
            });
        }
        if self.input_subs.borrow().len() < 3 {
            let sub = cx.subscribe_in(
                &filter_state,
                window,
                |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let value = emitter.read(cx).value().to_string();
                    this.view.update(cx, |v, cx| v.probe_set_filter(&value, cx));
                },
            );
            self.input_subs.borrow_mut().push(sub);
        }

        let seed_state = window.use_keyed_state(
            SharedString::from("insight-probe-seed"),
            cx,
            |window, cx| InputState::new(window, cx).placeholder("如 42（可留空）"),
        );
        if dialog.seed != seed_state.read(cx).value() {
            seed_state.update(cx, |state, cx| {
                state.set_value(dialog.seed.clone(), window, cx);
            });
        }
        if self.input_subs.borrow().len() < 4 {
            let sub = cx.subscribe_in(
                &seed_state,
                window,
                |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let value = emitter.read(cx).value().to_string();
                    this.view.update(cx, |v, cx| v.probe_set_seed(&value, cx));
                },
            );
            self.input_subs.borrow_mut().push(sub);
        }

        // 规模估算（决策文档 §5：确认窗口含规模预估）：建表口径 N 行 × K 列
        let amount_label = match dialog.amount_mode {
            ProbeAmountMode::Full => "全部行".to_string(),
            ProbeAmountMode::RowCount => {
                let n = dialog.row_count.trim();
                if n.is_empty() {
                    "—（填行数）".to_string()
                } else {
                    format!("前 {n} 行")
                }
            }
            ProbeAmountMode::Percent => {
                let p = dialog.percent.trim();
                if p.is_empty() {
                    "—（填百分比）".to_string()
                } else {
                    format!("约 {p}% 随机取样")
                }
            }
        };
        let col_label = if columns_all {
            if dialog.column_candidates.is_empty() {
                "全部列".to_string()
            } else {
                format!("全部 {} 列", dialog.column_candidates.len())
            }
        } else {
            match &dialog.columns {
                crate::model::ProbeColumns::Subset(sel) => format!("{} 列", sel.len()),
                _ => "全部列".to_string(),
            }
        };

        // 底部三按钮：取消 / 全量探查 / 探查（按当前配置）。
        // 取消：关窗不动作；全量探查：强制全量+全列直接确认；探查：按当前配置确认。
        let cancel_btn = Button::new("insight-probe-cancel")
            .label("取消")
            .ghost()
            .on_click({
                let view = view.clone();
                move |_, _, app| {
                    view.update(app, |v, cx| v.cancel_probe_dialog(cx));
                }
            });
        let explore_all_btn = Button::new("insight-probe-all")
            .label("全量探查")
            .icon(gpui_kit::assets::IconName::Zap)
            .outline()
            .on_click({
                let view = view.clone();
                move |_, _, app| {
                    view.update(app, |v, cx| {
                        v.probe_quick_amount(ProbeAmountMode::Full, String::new(), cx);
                        v.probe_set_columns(crate::model::ProbeColumns::All, cx);
                        let _ = v.confirm_probe_dialog(cx);
                    });
                }
            });
        // 主按钮文案随列选择变：全部列 → "探查"；选列 → "探查 N 列"
        let confirm_label = if columns_all {
            "探查".to_string()
        } else {
            match &dialog.columns {
                crate::model::ProbeColumns::Subset(sel) if !sel.is_empty() => {
                    format!("探查 {} 列", sel.len())
                }
                _ => "探查".to_string(),
            }
        };
        let confirm_btn = Button::new("insight-probe-confirm")
            .label(confirm_label)
            .primary()
            .on_click({
                let view = view.clone();
                move |_, _, app| {
                    view.update(app, |v, cx| {
                        v.confirm_probe_dialog(cx);
                    });
                }
            });

        let parse_notice = dialog.parse_notice.clone();

        div()
            .v_flex()
            .w_full()
            .gap_2()
            .p(rems(ui::PANEL_PADDING))
            // 探查对象（Dialog 标题已是「探查确认」，内容区不再重复小标题）
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_1()
                    .text_color(theme.colors.muted_foreground)
                    .child(
                        div()
                            .flex_none()
                            .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                            .child("探查对象："),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                            .text_color(theme.colors.foreground)
                            .child(dialog.target.title()),
                    ),
            )
            // —— 数据量 ——
            .child(Self::section_title("数据量"))
            .child(amount_group)
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_1()
                    .children(quick_presets.iter().map(|(mode, label, value)| {
                        let active = is_quick_active(*mode, value);
                        Button::new(SharedString::from(format!("insight-probe-quick-{label}")))
                            .ghost()
                            .compact()
                            .toggled(active)
                            .label(label.to_string())
                            .on_click({
                                let view = view.clone();
                                let mode = *mode;
                                let value = value.to_string();
                                move |_, _, app| {
                                    view.update(app, |v, cx| {
                                        v.probe_quick_amount(mode, value.clone(), cx);
                                    });
                                }
                            })
                    })),
            )
            .when(dialog.amount_mode == ProbeAmountMode::RowCount, |this| {
                this.child(
                    div()
                        .h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                                .child("行数"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&row_state).cleanable(true)),
                        ),
                )
            })
            .when(dialog.amount_mode == ProbeAmountMode::Percent, |this| {
                this.child(
                    div()
                        .h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                                .child("百分比"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&pct_state).cleanable(true)),
                        ),
                )
            })
            .child(Separator::horizontal())
            // —— 采样方式 ——
            .child(Self::section_title("采样方式"))
            .child(sample_group)
            .child(
                div()
                    .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                    .text_color(theme.colors.muted_foreground)
                    .child(
                        if dialog.sample_mode == crate::model::ProbeSampleMode::Head {
                            SharedString::from("顺序取：确定的前 N 行（与表序一致）")
                        } else {
                            SharedString::from(
                                "均匀随机：reservoir 取样，随机但有代表性，可配种子复现",
                            )
                        },
                    ),
            )
            .when(
                dialog.amount_mode == ProbeAmountMode::Percent
                    && dialog.sample_mode == crate::model::ProbeSampleMode::Reservoir,
                |this| {
                    this.child(
                        div()
                            .h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                                    .child("种子"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&seed_state).cleanable(true)),
                            ),
                    )
                },
            )
            .child(Separator::horizontal())
            // —— 探查列 ——
            .child(Self::section_title("探查列（未选列不出现在 EDA）"))
            .child(columns_group)
            .when(!columns_all, |this| {
                // 选择列：候选清单（Checkbox 列表；无候选 = 内省中 / 失败，提示默认全部列）
                let sel = match &dialog.columns {
                    crate::model::ProbeColumns::Subset(s) => s.clone(),
                    _ => Vec::new(),
                };
                if dialog.column_candidates.is_empty() {
                    this.child(
                        div()
                            .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                            .text_color(theme.colors.muted_foreground)
                            .child("正在获取列清单…（获取失败时按全部列探查）"),
                    )
                } else {
                    // 列候选清单：**定高滚动**（`ui::INSIGHT_PROBE_COL_LIST_MAX_HEIGHT`）。
                    // 此前是全量铺开，列一多确认窗口被一路撑高、勾选还会让版式上下跳。
                    this.child(
                        div()
                            .id("insight-probe-col-list")
                            .v_flex()
                            .w_full()
                            .gap_1()
                            .max_h(rems(ui::INSIGHT_PROBE_COL_LIST_MAX_HEIGHT))
                            .overflow_y_scrollbar()
                            .children(dialog.column_candidates.iter().map(|name| {
                                let name = name.clone();
                                let view = view.clone();
                                Checkbox::new(SharedString::from(format!(
                                    "insight-probe-col-{name}"
                                )))
                                .label(name.clone())
                                .checked(sel.contains(&name))
                                .on_click(move |_, _, app| {
                                    view.update(app, |v, cx| {
                                        let current =
                                            match &v.probe_dialog().map(|d| d.columns.clone()) {
                                                Some(crate::model::ProbeColumns::Subset(s)) => {
                                                    s.clone()
                                                }
                                                _ => vec![name.clone()],
                                            };
                                        let next = if current.contains(&name) {
                                            current.into_iter().filter(|c| c != &name).collect()
                                        } else {
                                            let mut c = current;
                                            c.push(name.clone());
                                            c
                                        };
                                        v.probe_set_columns(
                                            crate::model::ProbeColumns::Subset(next),
                                            cx,
                                        );
                                    });
                                })
                            })),
                    )
                }
            })
            .child(Separator::horizontal())
            // —— 过滤条件（可选） ——
            .child(Self::section_title("过滤条件（可选 WHERE）"))
            .child(Input::new(&filter_state).cleanable(true))
            .child(
                div()
                    .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                    .text_color(theme.colors.muted_foreground)
                    .child("条件包在源查询内层，先过滤再取样。"),
            )
            .child(Separator::horizontal())
            // —— 规模预估 ——
            .child(Self::section_title("规模预估"))
            .child(
                div()
                    .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                    .text_color(theme.colors.muted_foreground)
                    .child(if dialog.filter.trim().is_empty() {
                        format!("将建分析临时表：{amount_label} × {col_label}")
                    } else {
                        format!("过滤后取样：{amount_label} × {col_label}")
                    }),
            )
            .when_some(parse_notice, |this, notice| {
                this.child(
                    div()
                        .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
                        .text_color(theme.colors.danger)
                        .child(SharedString::from(notice)),
                )
            })
            // —— 底部三按钮：取消 / 全量探查 / 探查 ——
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .pt_1()
                    .child(cancel_btn)
                    .child(div().flex_1())
                    .child(explore_all_btn)
                    .child(confirm_btn),
            )
    }

    /// 确认窗口区块标题（数据量 / 采样方式 / 探查列 / 过滤 / 规模 / 提示）
    fn section_title(label: &str) -> Div {
        div()
            .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
            .font_weight(FontWeight::SEMIBOLD)
            .child(SharedString::from(label))
    }
}

impl Focusable for ProbeDialogBody {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ProbeDialogBody {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        // 状态已不在（确认/取消与关窗之间的一帧竞态）：空盒占位，下一帧弹窗即被
        // `sync_probe_modal` 关掉——不 panic、不留残影
        let Some(dialog) = self.view.read(cx).probe_dialog().cloned() else {
            return div().into_any_element();
        };
        self.render_form(window, cx, &dialog, &theme)
            .into_any_element()
    }
}
