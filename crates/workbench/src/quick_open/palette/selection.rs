//! 选中态：宿主权威业务键、↑↓ 漫游、宿主 → 列表镜像。
//!
//! 选中以**业务键**为准（`selected_key`），组件的选中索引只是渲染锚点；
//! 镜像期（[`mirror_selection`]）以 `begin_host_sync` / `end_host_sync` 守卫
//! 屏蔽组件回调回写，避免「更新正在被更新的实体」。

use gpui_kit::component::list::ListState;
use gpui_kit::component::IndexPath;
use gpui_kit::*;

use crate::quick_open::delegate::QuickOpenDelegate;

use super::QuickOpenPalette;

impl QuickOpenPalette {
    /// 委托回传的选中变化（鼠标点击 / 组件漫游）。
    pub(crate) fn set_selection(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if self.selected_key == key {
            return;
        }
        self.selected_key = key;
        cx.notify();
    }

    /// ↑↓：在结果上漫游（焦点在输入框，键盘由本视图接住）。
    pub(super) fn move_selection(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(list) = self.list.clone() else {
            return;
        };
        let mut moved: Option<String> = None;
        list.update(cx, |state, cx| {
            let from = state
                .selected_index()
                .or_else(|| state.delegate().first_index());
            let Some(from) = from else {
                return;
            };
            let Some(target) = state.delegate().step_index(from, delta) else {
                return;
            };
            state.delegate_mut().begin_host_sync();
            state.set_selected_index(Some(target), window, cx);
            state.delegate_mut().end_host_sync();
            moved = state.delegate().selected_key();
            state.scroll_to_selected_item(window, cx);
        });
        if let Some(key) = moved {
            self.selected_key = Some(key);
            cx.notify();
        }
    }
}

/// 把宿主的选中镜像进列表（只在真的不一致时才动）。
///
/// 镜像期间组件回调**不回写**本视图（`begin_host_sync` / `end_host_sync` 守卫）：
/// 镜像常发生在渲染期，回写就是「更新正在被更新的实体」。
pub(super) fn mirror_selection(
    state: &mut ListState<QuickOpenDelegate>,
    target: Option<IndexPath>,
    window: &mut Window,
    cx: &mut Context<ListState<QuickOpenDelegate>>,
) {
    if state.selected_index() == target {
        return;
    }
    state.delegate_mut().begin_host_sync();
    state.set_selected_index(target, window, cx);
    state.delegate_mut().end_host_sync();
    state.scroll_to_selected_item(window, cx);
}
