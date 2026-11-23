//! 启动解锁窗（主密码档）。
//!
//! 语义：密钥来源切到「主密码」档后，库钥只在**输入口令**后进内存（`shared::vault_source`
//! 的解锁缓存）。没有这一步，下次启动时 master 档加密的凭据一律解不开——所以启动门禁只有两个
//! 出口：**解锁** 或 **退出应用**。
//!
//! 刻意不做的事：
//! - 不给「稍后再说」的降级入口：降级进去只会让连接静默失败，比拦住更糟；
//! - 不把失败次数当安全边界：本地口令爆破挡不住重启进程，三次上限是防**误触连点**，
//!   文案里如实写清。

use std::rc::Rc;

use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Root};
use gpui_kit::*;
use shared::i18n::tr;
use shared::vault_source::{self, VaultSource};

/// 误触上限（不是安全边界，见文件头）。
const MAX_ATTEMPTS: u32 = 3;

/// 本次启动是否需要先解锁（主密码档 + 已建立 + 尚未解锁）。
pub fn needs_unlock() -> bool {
    vault_source::current_source() == VaultSource::Master
        && vault_source::master_initialized()
        && !vault_source::master_unlocked()
}

/// 打开解锁窗；成功回调里由调用方打开主窗口。
pub fn open_unlock_window(cx: &mut App, on_unlocked: Rc<dyn Fn(&mut App)>) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(440.), px(250.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(tr("RdataStation · 主密码").to_string().into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        // 解锁窗也是应用窗口：材质按当前设置来，避免「解锁窗不透明 → 主窗透明」的跳变。
        settings::SettingsService::apply_window_material(window, cx);
        let view = cx.new(|cx| VaultUnlockView::new(window, cx, on_unlocked.clone()));
        cx.new(|cx| Root::new(view, window, cx))
    })
    .expect("failed to open unlock window");
}

struct VaultUnlockView {
    password: Entity<InputState>,
    error: Option<String>,
    attempts: u32,
    on_unlocked: Rc<dyn Fn(&mut App)>,
}

impl VaultUnlockView {
    fn new(window: &mut Window, cx: &mut App, on_unlocked: Rc<dyn Fn(&mut App)>) -> Self {
        let password = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_masked(true, window, cx);
            state.set_placeholder(tr("主密码"), window, cx);
            state
        });
        Self {
            password,
            error: None,
            attempts: 0,
            on_unlocked,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.attempts >= MAX_ATTEMPTS {
            return;
        }
        let value = self.password.read(cx).value().trim().to_string();
        if value.is_empty() {
            self.error = Some(tr("请先输入主密码").to_string());
            cx.notify();
            return;
        }
        match vault_source::unlock_master(&value) {
            Ok(()) => {
                self.password
                    .update(cx, |state, cx| state.set_value("", window, cx));
                (self.on_unlocked)(&mut *cx);
                window.remove_window();
            }
            Err(_) => {
                self.attempts += 1;
                self.error = Some(if self.attempts >= MAX_ATTEMPTS {
                    format!(
                        "{}（已连续 {MAX_ATTEMPTS} 次失败：本次不再尝试，重启应用可重试）",
                        tr("主密码不正确")
                    )
                } else {
                    format!(
                        "{}（第 {} / {MAX_ATTEMPTS} 次）",
                        tr("主密码不正确"),
                        self.attempts
                    )
                });
                self.password
                    .update(cx, |state, cx| state.set_value("", window, cx));
                cx.notify();
            }
        }
    }
}

impl Render for VaultUnlockView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let locked_out = self.attempts >= MAX_ATTEMPTS;
        let this = cx.entity().clone();
        let this_quit = cx.entity().clone();

        div()
            .v_flex()
            .size_full()
            .gap_3()
            .p_4()
            .bg(theme.colors.background)
            .child(div().text_lg().child(tr("密钥来源已切到「主密码」档")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(tr(
                        "此档的库钥只在内存里：输入主密码解开后，认证 / 网络 / 资源三张表的密文才可读。口令不会落盘，也不会写日志。",
                    )),
            )
            .child(Input::new(&self.password))
            .children(self.error.clone().map(|line| {
                div()
                    .id("vault-unlock-error")
                    .test_support()
                    .text_xs()
                    .text_color(theme.colors.danger)
                    .child(line)
            }))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        Button::new("vault-unlock-submit")
                            .label(tr("解锁"))
                            .primary()
                            .disabled(locked_out)
                            .on_click(move |_, window, cx| {
                                this.update(cx, |view, cx| view.submit(window, cx));
                            }),
                    )
                    .child(
                        Button::new("vault-unlock-quit")
                            .label(tr("退出应用"))
                            .ghost()
                            .on_click(move |_, _window, cx| {
                                let _ = this_quit;
                                cx.quit();
                            }),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(tr(
                        "忘记主密码：master 档加密的凭据不可恢复（可用备份的 vault-master-key 文件覆盖 data 目录同名文件后重启）。",
                    )),
            )
    }
}
