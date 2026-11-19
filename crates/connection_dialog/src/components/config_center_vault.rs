//! 配置中心「密钥来源」域（原型 ⑦ 密码管理 · 三档选择 + 切档）
//!
//! 三档语义与代价见 `shared::vault_source` 的模块头；本文件只负责**把它摆出来并让人切**：
//! - 当前档位 + 三档各自的代价（切之前就该看清）；
//! - 切档按钮（当前档禁用；系统凭据档在非 Windows 上直接拦下，不假装能切）；
//! - 主密码输入（切到 master 档要口令；库已存在时是解锁，不存在时是**新建**）；
//! - 结果行（重加密条数 / 失败原因），失败时档位不变（旧钥仍能解开全部密文）。

use std::rc::Rc;

use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Theme;
use gpui_kit::*;

use engine::services::vault_service::{self, VaultStatus};
use shared::i18n::tr;
use shared::vault_source::VaultSource;

/// 密钥域的界面状态（页面持有；密码输入是实体，其余就地读写）。
pub struct VaultView {
    /// 右栏当前是否显示这一域（导航第 4 项）
    pub open: bool,
    status: VaultStatus,
    password: Entity<InputState>,
    busy: bool,
    notice: Option<String>,
}

impl VaultView {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let password = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            // 掩码：主密码不按明文留在屏幕上
            state.set_masked(true, window, cx);
            state.set_placeholder(tr("主密码（切到主密码档时填写）"), window, cx);
            state
        });
        Self {
            open: false,
            status: vault_service::status(),
            password,
            busy: false,
            notice: None,
        }
    }

    /// 当前档位（导航 chip 与测试读它）。
    pub fn current(&self) -> VaultSource {
        self.status.source
    }

    /// 重读状态（切完档 / 重新进入这一域时调一次；不查库，只读本地文件）。
    pub fn refresh(&mut self) {
        self.status = vault_service::status();
    }

    /// 切档（同步执行：重加密是本地 SQLite 写，量级是「几十~几百行 JSON」）。
    ///
    /// 失败时**只写结果行**、不动任何状态：档位文件与库都由 store 侧保证要么全换要么没换。
    fn switch_to(
        &mut self,
        target: VaultSource,
        notify: &Rc<dyn Fn(&mut App)>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.busy || self.status.source == target {
            return;
        }
        if matches!(target, VaultSource::Os) && !self.status.os_supported {
            self.notice = Some(tr("当前平台不支持系统凭据档（DPAPI 仅 Windows）").to_string());
            notify(cx);
            return;
        }
        let password = self.password.read(cx).value().trim().to_string();
        if matches!(target, VaultSource::Master) && password.is_empty() {
            self.notice = Some(tr("切到主密码档需要填写主密码").to_string());
            notify(cx);
            return;
        }
        self.busy = true;
        notify(cx);
        let outcome = match crate::services::rt::runtime() {
            Some(rt) => rt
                .block_on(vault_service::switch_source(
                    target,
                    if password.is_empty() {
                        None
                    } else {
                        Some(password.as_str())
                    },
                ))
                .map_err(|e| e.to_string()),
            None => Err(crate::services::rt::RT_UNAVAILABLE.to_string()),
        };
        self.busy = false;
        self.refresh();
        self.notice = Some(match outcome {
            Ok(report) if report.unchanged => {
                format!("已是「{}」档：主密码已校验，未改写任何密文", target.label())
            }
            Ok(report) => {
                let projects = report
                    .projects
                    .iter()
                    .map(|(name, n)| format!("{name} {n} 条"))
                    .collect::<Vec<_>>();
                let project_part = if projects.is_empty() {
                    "项目库无可改写内容".to_string()
                } else {
                    format!("项目库 {}", projects.join(" / "))
                };
                format!(
                    "已切到「{}」：全局库 认证 {} · 网络 {} · 资源 {}；{project_part}",
                    target.label(),
                    report.global.auth,
                    report.global.network,
                    report.global.resources
                )
            }
            Err(reason) => format!("切档失败（档位未变）：{reason}"),
        });
        // 密码用一次就清掉：不留在输入框里给下一个人看
        self.password
            .update(cx, |state, cx| state.set_value("", window, cx));
        notify(cx);
    }

    /// 右栏正文（面包屑在页级，这里只有卡片）。
    pub fn render(
        &self,
        notify: &Rc<dyn Fn(&mut App)>,
        view: &Rc<std::cell::RefCell<VaultView>>,
        theme: &Theme,
    ) -> Div {
        let current = self.status.source;
        let current_line = div()
            .id("vault-current")
            .test_support()
            .text_sm()
            .child(format!("当前档位：{}", current.label()));

        let mut rows = div().v_flex().gap_2();
        for source in VaultSource::ALL {
            let is_current = source == current;
            let usable = !(matches!(source, VaultSource::Os) && !self.status.os_supported);
            let note = if is_current {
                "当前".to_string()
            } else if !usable {
                "本平台不支持".to_string()
            } else if matches!(source, VaultSource::Master) {
                if self.status.master_initialized {
                    if self.status.master_unlocked {
                        "已建立（本次已解锁）".to_string()
                    } else {
                        "已建立（需口令解锁）".to_string()
                    }
                } else {
                    "尚未建立（切过去即新建）".to_string()
                }
            } else {
                "可用".to_string()
            };
            let view_click = view.clone();
            let notify_click = notify.clone();
            let mut btn = Button::new(format!("vault-switch-{}", source.key()))
                .label(if is_current {
                    tr("当前档")
                } else {
                    tr("切到此档")
                })
                .debug_selector({
                    let key = source.key().to_string();
                    move || format!("vault-switch-{key}")
                });
            btn = if is_current {
                btn.ghost()
            } else {
                btn.primary()
            };
            if is_current || self.busy || !usable {
                btn = btn.disabled(true);
            }
            let row = div()
                .v_flex()
                .gap_1()
                .rounded_md()
                .border_1()
                .border_color(theme.colors.border)
                .px_3()
                .py_2()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_sm().child(tr(source.label())))
                        .child(
                            div()
                                .text_xs()
                                .text_color(if is_current {
                                    theme.colors.success
                                } else {
                                    theme.colors.muted_foreground
                                })
                                .child(note),
                        )
                        .child(div().flex_1())
                        .child(btn.on_click(move |_, window, app| {
                            view_click
                                .borrow_mut()
                                .switch_to(source, &notify_click, window, app);
                        })),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(tr(source.hint())),
                );
            rows = rows.child(row);
        }

        let notice = self.notice.clone().map(|line| {
            div()
                .id("vault-notice")
                .test_support()
                .p_2()
                .rounded_sm()
                .bg(theme.colors.muted)
                .text_xs()
                .child(line)
        });

        div()
            .v_flex()
            .gap_3()
            .child(current_line)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(tr("主密码")),
                    )
                    .child(Input::new(&self.password).flex_1()),
            )
            .child(rows)
            .children(notice)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(tr(
                        "切档会把认证 / 网络 / 资源三张表的密文整体换钥（项目库先、全局库后、档位文件最后）；失败则档位不变。已建立的连接不受影响，重新解密时按新钥读取。",
                    )),
            )
    }
}
