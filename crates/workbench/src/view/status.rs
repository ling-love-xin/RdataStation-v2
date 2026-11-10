//! 状态栏纯函数部件（Mock 指示、侧栏开关、LSP 指示、隐藏态还原）。
//!
//! 自 `view.rs` 拆分（行为不变）。

use super::*;

/// 状态栏的 Mock 任务指示（有任务在跑时才给）：`◐ Mock 生成中 40%`（出口类任务给阶段名）
/// 与一个可取消任务尾部的「取消」。
///
/// 为何放在状态栏、而不与 D38 的「状态单点」相冲：D38 管的是**读数放在哪**（进度详情仍在
/// 它自己的中央表头 / 右 Dock 清单下），而这里解决另一个问题——面板被切走、中央 tab 被关掉时
/// **任务还在跑却看不见也取消不了**。所以它只给「在跑 + 取消」，不重复进度条。
///
/// 状态取自面板实体（单一权威）而不是往 `Shared` 再存一份：读别的面板实体在宿主 render 里
/// 已有先例（右栏「存档详情」读资产库面板的选中项）。
pub(super) fn mock_status_chip(panel: Entity<MockPanel>, cx: &mut App) -> Div {
    // 颜色先取出来：`cx.theme()` 借了 cx，而下面读面板 / 挂回调还要用 cx
    let (fg, hover_bg) = {
        let theme = cx.theme();
        (theme.colors.primary_foreground, theme.colors.primary_active)
    };
    let (status, cancellable, cancel_requested) = {
        let view = panel.read(cx);
        // 文案 / 百分比 / ETA / 能不能取消一次拿全（口径都在 mock crate，宿主只摆位置）
        let Some(status) = view.job_status() else {
            return div();
        };
        (status, status.cancellable, status.cancel_requested)
    };

    let mut chip = div()
        .h_flex()
        .items_center()
        .gap_1()
        .px_1()
        .rounded_sm()
        .text_color(fg)
        .child(div().child(status.label()))
        // 迷你进度条：切走中央 tab / 整个 Mock 面板时，任务还在跑——
        // 状态栏是唯一一直在那儿的地方，走到哪了在这里看
        .child(
            div()
                .w(mock::ui::STATUS_BAR_BAR_WIDTH)
                .flex_none()
                .child(job_progress_bar(
                    "status-bar",
                    status.percent(),
                    status.indeterminate(),
                    JobBarSize::Mini,
                    // 状态栏是品牌珊瑚底：条取配对前景色（组件默认的强调色在珊瑚底上读不清）
                    Some(fg),
                    cx,
                )),
        );
    // 剩余时间：与百分比是同一件事的两头（走到哪了 / 还要多久）
    if let Some(text) = status.eta.and_then(eta_text_compact) {
        chip = chip.child(div().child(text));
    }
    if cancellable {
        // 出口类任务不可取消（DuckDB / 文件系统内没有中断点），所以那一档只给文字
        let mut cancel = div()
            .id("status-mock-cancel")
            .role(gpui::Role::Button)
            .px_1()
            .rounded_sm()
            .text_color(fg)
            .child(if cancel_requested {
                "正在取消…"
            } else {
                "取消"
            });
        if !cancel_requested {
            cancel = cancel
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .on_click(move |_, _, app| {
                    panel.update(app, |panel, cx| panel.cancel_job(cx));
                });
        }
        chip = chip.child(cancel);
    }
    chip
}

/// 状态栏开关按钮（自绘，对齐设计稿 .sb-btn）：图标 + 文字 + hover 高亮。
pub(super) fn sb_toggle(
    id: &'static str,
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    fg: Hsla,
    hover_bg: Hsla,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_1()
        .px_1()
        .rounded_sm()
        .cursor_pointer()
        .text_color(fg)
        .hover(move |s| s.bg(hover_bg))
        .child(Icon::new(icon).size_3p5())
        .child(div().child(label.into()))
        .on_click(move |_, _, app| on_click(app))
}

/// 状态栏 LSP 指示的（图标，文案）（**纯函数，可单测**）
///
/// 状态优先级：**就绪 > 启动中 > 拦住（停止 / 启动失败 / 断开）> 未启动**。
/// 图标用完整 Lucide 目录（组件子集里没有 `server-*` 系列）。
pub fn lsp_status_chip(
    running: bool,
    initialized: bool,
    block: Option<&LspBlock>,
) -> (CatalogIcon, String) {
    if running && initialized {
        (CatalogIcon::Server, "语言服务：就绪".to_string())
    } else if running {
        (CatalogIcon::ServerCog, "语言服务：启动中".to_string())
    } else {
        match block {
            Some(LspBlock::Manual) => (CatalogIcon::ServerOff, "语言服务：已停止".to_string()),
            Some(LspBlock::StartFailed(reason)) => (
                CatalogIcon::ServerOff,
                format!("语言服务：未启动（{reason}）"),
            ),
            Some(LspBlock::Crashed(reason)) => (
                CatalogIcon::ServerCrash,
                format!("语言服务：已断开（{reason}）"),
            ),
            None => (CatalogIcon::ServerOff, "语言服务：未启动".to_string()),
        }
    }
}

/// LSP 管理器浮层内容：状态 + 在用面板数 + 拦住原因 + 启动 / 停止
///
/// 从 `entity` **现读**端口状态（浮层每次重绘都重读，启动 / 停止后立刻反映）；`cx` 是
/// `Context<PopoverState>`，只当 `App` 用（与编辑器工具栏定制面板同一约定）。
pub(super) fn render_lsp_manager(entity: &Entity<WorkbenchView>, cx: &mut App) -> AnyElement {
    let Some(lsp) = entity.read(cx).lsp.clone() else {
        return div().into_any_element();
    };
    let running = lsp.is_running();
    let (icon, label) = lsp_status_chip(running, lsp.is_initialized(), lsp.block().as_ref());
    let count = lsp.active_document_count();
    let reason = match lsp.block() {
        Some(LspBlock::StartFailed(reason)) | Some(LspBlock::Crashed(reason)) => Some(reason),
        _ => None,
    };
    let muted = cx.theme().colors.muted_foreground;

    let mut body = div()
        .v_flex()
        .gap_2()
        .p_3()
        .w(rems(ui::STATUS_LSP_MANAGER_WIDTH));
    body = body.child(
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(Icon::new(icon).size_3p5())
            .child(div().text_sm().child("语言服务（LSP）")),
    );
    body = body.child(div().text_sm().child(label));
    body = body.child(
        div()
            .text_xs()
            .text_color(muted)
            .child(format!("在用面板：{count}")),
    );
    if let Some(reason) = reason {
        body = body.child(div().text_xs().text_color(muted).child(reason));
    }
    // 面板为空时提醒：服务会在空闲回收时自动停（下次需要再启）
    if count == 0 && running {
        body = body.child(
            div()
                .text_xs()
                .text_color(muted)
                .child("无面板使用，稍后会自动停止"),
        );
    }

    let start_entity = entity.clone();
    let stop_entity = entity.clone();
    body = body.child(
        div()
            .h_flex()
            .gap_2()
            .child(
                Button::new("lsp-manager-start")
                    .small()
                    .label("启动")
                    .disabled(running)
                    .on_click(move |_, _, app| {
                        if let Some(lsp) = start_entity.read(app).lsp.clone() {
                            lsp.start();
                        }
                        start_entity.update(app, |view, cx| {
                            view.sync_lsp_epoch();
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("lsp-manager-stop")
                    .small()
                    .label("停止")
                    .disabled(!running)
                    .on_click(move |_, _, app| {
                        if let Some(lsp) = stop_entity.read(app).lsp.clone() {
                            lsp.stop();
                        }
                        stop_entity.update(app, |view, cx| {
                            view.sync_lsp_epoch();
                            cx.notify();
                        });
                    }),
            ),
    );
    body.into_any_element()
}

/// 完全隐藏切换的纯状态转移：返回 `(新模式, 新快照)`。
///
/// - 非隐藏态 → 隐藏：记录当前模式到快照；
/// - 隐藏态 → 恢复：按快照还原（快照异常为 Hidden 时兜底展开）。
///
/// 纯函数，供状态栏开关与契约测试共用。
pub fn toggle_hidden_mode(mode: SidebarMode, snapshot: SidebarMode) -> (SidebarMode, SidebarMode) {
    if mode == SidebarMode::Hidden {
        (restore_snapshot(snapshot), snapshot)
    } else {
        (SidebarMode::Hidden, mode)
    }
}

/// 切换单侧「完全隐藏」：隐藏前记录当前模式快照，恢复时按快照还原，
/// 保证「收起」等状态在完全隐藏 / 恢复往返后不丢失。
pub(super) fn toggle_sidebar_hidden(
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    is_left: bool,
    app: &mut App,
) {
    let (mode, snapshot) = if is_left {
        (&shared.left_mode, &shared.left_mode_before_hidden)
    } else {
        (&shared.right_mode, &shared.right_mode_before_hidden)
    };
    let (next_mode, next_snapshot) = toggle_hidden_mode(mode.get(), snapshot.get());
    mode.set(next_mode);
    snapshot.set(next_snapshot);
    entity.update(app, |_, cx| cx.notify());
}

/// 还原快照模式；快照异常为 Hidden 时兜底为展开（防御性）。
pub(crate) fn restore_snapshot(mode: SidebarMode) -> SidebarMode {
    if mode == SidebarMode::Hidden {
        SidebarMode::Expanded
    } else {
        mode
    }
}
