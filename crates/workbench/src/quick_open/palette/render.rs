//! 浮层渲染：实体懒装配 + 遮罩 / 面板 / 键盘 Action 接线（render 即权威同步点）。

use gpui_kit::TestSupportExt as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use crate::commands::QuickOpenLocate;
use crate::quick_open::delegate::QuickOpenDelegate;
use crate::quick_open::model;
use crate::ui;

use super::QuickOpenPalette;

impl QuickOpenPalette {
    /// 懒创建输入框与结果列表（首帧渲染；两者都需要 `&mut Window`）。
    pub(super) fn ensure_entities(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder("搜索对象、命令…（> 命令 · # 全文）")
            });
            self._input_sub = Some(cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => this.refresh(window, cx),
                    InputEvent::PressEnter { shift, .. } => {
                        // ↵ 打开并关闭；Shift+↵ 保留面板（Ctrl+↵ 的后台打开待编辑器支持）。
                        let keep_open = *shift;
                        this.confirm(keep_open, window, cx);
                    }
                    _ => {}
                },
            ));
            self.input = Some(input);
        }
        if self.list.is_none() {
            let palette = cx.entity().downgrade();
            let list = cx.new(|cx| {
                ListState::new(QuickOpenDelegate::new(palette), window, cx).selectable(true)
            });
            self.list = Some(list);
        }
    }
}

impl Render for QuickOpenPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.shared.quick_open.get() {
            return div().into_any_element();
        }
        self.ensure_entities(window, cx);
        self.consume_pending_open(window, cx);

        let theme = cx.theme().clone();
        let input = self.input.clone().expect("input lazy init");
        let list = self.list.clone().expect("list lazy init");
        // 与 `refresh` 同一口径：Goto 档锁定模式，前缀不切档。
        let raw = input.read(cx).value();
        let query = match self.mode_override {
            Some(mode) => model::Query {
                mode,
                needle: model::parse(&raw).needle,
            },
            None => model::parse(&raw),
        };
        let rows = list.read(cx).delegate().row_count();
        // 词长达不到本档门槛：本地源照常，这里给还差几个字的提示。
        let hint_right = if !query.needle.is_empty() && !query.async_ready() {
            let short = query.min_len().saturating_sub(query.needle.chars().count());
            format!("再输入 {short} 个字符开始搜索元数据")
        } else if self.searching {
            "元数据搜索中…".to_string()
        } else {
            format!("{rows} 条")
        };
        let mut input_row = div().h_flex().items_center().gap_2();
        input_row = input_row.child(
            div().flex_1().min_w_0().child(
                Input::new(&input)
                    // 搜索语境：左侧图标 + 有内容时的清空按钮（清空走普通编辑路径，
                    // 会发 Change → 自动刷新，无需自管）。
                    .prefix(Icon::new(IconName::Search).size_4())
                    .cleanable(true),
            ),
        );
        // 档位 chip：**常驻**（默认档也显示 `#` / `>` 两枚弱化入口——前缀口诀不再是
        // 唯一发现途径）；当前档高亮，点击进入 / 退出（词保留，见 `toggle_mode`）。
        // Goto 档只显示「⌖ 跳转」退出入口（进入 Goto 是 `Ctrl+Shift+O` 键位专属）。
        let chip_specs: &[(&str, &str, &str, model::Mode)] = if query.mode == model::Mode::Goto {
            &[("qo-chip-goto", "⌖", "跳转", model::Mode::Goto)]
        } else {
            &[
                ("qo-chip-fulltext", "#", "全文", model::Mode::FullText),
                ("qo-chip-command", ">", "命令", model::Mode::Command),
            ]
        };
        for (chip_id, glyph, label, chip_mode) in chip_specs {
            let active = query.mode == *chip_mode;
            let mut chip = crate::quick_open::delegate::badge(format!("{glyph} {label}"), &theme)
                .id((*chip_id).to_string())
                .test_support()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.toggle_mode(*chip_mode, window, cx)
                }));
            if active {
                chip = chip
                    .text_color(theme.colors.foreground)
                    .bg(theme.colors.secondary);
            }
            input_row = input_row.child(chip);
        }

        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            // 遮罩与前景是**兄弟**：点遮罩空白才关闭；点在面板上 `mouse_down` 不再冒泡到
            // 遮罩层（兄弟不冒泡），否则按下即把面板关掉，随后的 `click` 确认就失效了。
            .child(
                div()
                    .id("qo-overlay")
                    .test_support()
                    .absolute()
                    .inset_0()
                    .bg(theme.colors.overlay)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close(cx)),
                    ),
            )
            .child(
                div()
                    // 示意 v5：面板水平居中、顶部距标题栏 42px。
                    // 命中面板矩形内时遮挡后方遮罩 hitbox：否则遮罩 `on_mouse_down`
                    // 会把面板关掉、随后的行 `click` 落空（鼠标确认失效，Enter 不受影响）。
                    .relative()
                    .occlude()
                    .mt(rems(ui::QUICK_OPEN_PANEL_TOP))
                    .w(rems(ui::QUICK_OPEN_PANEL_WIDTH))
                    .max_h(rems(ui::QUICK_OPEN_PANEL_MAX_HEIGHT))
                    .v_flex()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.popover)
                    .shadow_lg()
                    .child(input_row)
                    .child(List::new(&list).max_h(rems(ui::QUICK_OPEN_LIST_MAX_HEIGHT)))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .h(rems(ui::QUICK_OPEN_HINT_HEIGHT))
                            .border_t_1()
                            .border_color(theme.colors.border)
                            .px_1()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("↑↓ 选择 · ↵ 打开 · Shift+↵ 保留面板 · Esc 关闭")
                            .child(div().ml_auto().child(hint_right)),
                    ),
            )
            // 键盘：焦点在输入框里，单行 Input 不处理 ↑↓ / Esc（会 propagate），
            // 因此这三个 Action 在浮层根上接住；Enter 由输入框事件给出（含 Shift 三态）。
            .on_action(
                cx.listener(|this, _: &MoveUp, window, cx| this.move_selection(-1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveDown, window, cx| this.move_selection(1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &Escape, _window, cx| this.close(cx)))
            .on_action(
                cx.listener(|this, _: &QuickOpenLocate, window, cx| {
                    this.confirm_locate(window, cx)
                }),
            )
            .into_any_element()
    }
}
