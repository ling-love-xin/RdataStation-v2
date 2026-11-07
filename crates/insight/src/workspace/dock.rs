//! 中央 Dock 面板协议：EventEmitter / Focusable / BasePanel / ComponentPanel + focus_insight_workspace_tab。

use super::*;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent};

impl EventEmitter<PanelEvent> for InsightWorkspacePanel {}

impl Focusable for InsightWorkspacePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for InsightWorkspacePanel {
    fn panel_name(&self) -> &'static str {
        "insight-workspace"
    }

    fn on_added_to(
        &mut self,
        group: gpui::WeakEntity<TabGroup>,
        window: &mut Window,
        _cx: &mut gpui::Context<Self>,
    ) {
        self.group = Some(group);
        // 存窗口句柄：探查弹窗的开要在事件路径发生（那时没有窗口）
        self.window = Some(window.window_handle());
    }

    fn on_removed(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) {
        // 中间洞察 tab 被 ✕ 关闭：面板随 Dock 移除而 drop（宿主只持弱句柄），
        // 下次入口再打开时按「未建」重建——目标集合（`opened`）会清空，
        // 与对比候选「会话内已进入洞察」口径一致（关掉工作页 = 放弃本轮候选）。
        self.group = None;
    }
}

impl ComponentPanel for InsightWorkspacePanel {
    /// Dock tab 标题 = 当前洞察目标表名（参考 VS Code「tab 标题 = 打开对象名」约束），
    /// 未打开目标时回退「洞察」。
    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(
            self.view
                .read(cx)
                .target()
                .map(|t| SharedString::from(t.title()))
                .unwrap_or_else(|| SharedString::from("洞察")),
        )
    }

    fn title(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let name = self
            .view
            .read(cx)
            .target()
            .map(|t| SharedString::from(t.title()))
            .unwrap_or_else(|| SharedString::from("洞察"));
        // 标题栏内超长表名省略（vscode 约束：长标题省略 + hover 提示全名）
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .max_w(rems(ui::INSIGHT_WS_TITLE_MAX_WIDTH))
            .overflow_hidden()
            .text_ellipsis()
            .child(name)
    }
}

/// 把中间洞察 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// **必须在实体更新之外调用**（与 `focus_property_tab` 同因：`TabGroup` 切 tab 会回读
/// 面板实体，在更新闭包内调用会 double lease panic）。选中按面板 `entity_id` 定位。
pub fn focus_insight_workspace_tab(
    panel: &Entity<InsightWorkspacePanel>,
    window: &mut Window,
    cx: &mut App,
) {
    let (focus_handle, group) = {
        let view = panel.read(cx);
        (view.focus_handle.clone(), view.group.clone())
    };
    window.focus(&focus_handle, cx);
    let Some(group) = group else {
        return;
    };
    let me = panel.entity_id();
    group
        .update(cx, |group, cx| {
            let ix = group
                .panels()
                .iter()
                .position(|p| p.view().entity_id() == me);
            if let Some(ix) = ix {
                group.select_tab(ix, window, cx);
            }
        })
        .ok();
}
