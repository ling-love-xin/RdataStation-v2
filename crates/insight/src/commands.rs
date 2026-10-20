//! 洞察面板的动作（M8 Phase 1）。
//!
//! **只在事件路径上**：动作由面板 `on_action` 处理（改状态 → 通知宿主），渲染路径不认识它们。
//!
//! 命名空间是 `insight`，键位在 `crates/app` 统一注册（**注册了才宣传**：没实现的动作不绑键，
//! 也不写进文档的快捷键表）。
//!
//! 键位的 key context 是 `insight`（面板根元素的 `key_context`）。
//!
//! 曾经定义过 `OpenInsight` / `ReloadInsightRules` 两个动作，但全仓库无使用点：
//! Quick Open 的「打开洞察」走 `Action::OpenRightPanel(RightPanel::Insight)`，规则重载走
//! `RulesEvent::ReloadRequested`——按「注册了才宣传」原则删除，避免误导。

use gpui_kit::*;

actions!(
    insight,
    [
        /// 重算当前目标画像（保持 Tab 与折叠态）
        InsightRefresh
    ]
);
