//! 资产库（M6）面板装配：左 Dock「资产库」分支。
//!
//! 视图与状态在 `rds-analytics-resource` crate（`resource_view::ResourcesPanel`，与
//! mock / insight 同例）。本模块只做三件事：
//!
//! 1. **构造期**创建面板实体（无 I/O）+ 注入宿主端口（`components::resource_host`）；
//! 2. 事件路径入队刷新（`request_resources_refresh`），后台任务跑完由轮询回填；
//! 3. 转发渲染（面板自己拥有滚动与空态）。
//!
//! 取数在 `services::resource_jobs` 的工作线程上：`IndexRepair::scan` 要算本体指纹，
//! 不能放在事件路径（`panels-modules.md` §4 的 P1 同源问题）。**render 不发起任务**
//! ——入队点只有「激活面板 / 打开或切换项目 / 归档等变更之后」，与 `nav` 面板
//! `render 内入队` 的既有债刻意区分开。

use std::time::Duration;

use gpui_kit::base::StyledExt;
use gpui_kit::*;

use analytics_resource::resource_view::{ResourcesPanel, ResourcesSnapshot};

use super::SidebarPanel;
use crate::services::resource_jobs;

impl SidebarPanel {
    /// 创建资产库面板实体（在 `SidebarPanel::new` 里调用，无 I/O）。
    pub(super) fn build_resources_panel(
        shared: &super::Shared,
        cx: &mut Context<Self>,
    ) -> Entity<ResourcesPanel> {
        let host = crate::components::resource_host::build_host(shared);
        cx.new(|cx| ResourcesPanel::new(host, cx))
    }

    /// 请求一次列表刷新（**事件路径**）：入队 + 启动轮询回填。
    ///
    /// 未打开项目时不入队：没有项目就没有资产库（面板按空态呈现）。
    pub(crate) fn request_resources_refresh(&self, cx: &mut Context<Self>) {
        let Some(root) = self.shared.project_root() else {
            return;
        };
        let read_only = self.shared.project_ui.borrow().read_only;
        resource_jobs::enqueue_refresh(root, read_only);
        self.ensure_resources_pump(cx);
    }

    /// 轮询回填（`ensure_scratchpad_pump` 同一形态）：结果就绪就推给面板实体，
    /// 无待办时再多等一拍后退出（避免空转）。
    fn ensure_resources_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.resources_pump.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(Duration::from_millis(60)).await;
                if let Some(result) = resource_jobs::drain_snapshot() {
                    if weak
                        .update(cx, |this, cx| this.apply_resources_snapshot(result, cx))
                        .is_err()
                    {
                        return;
                    }
                }
                if !resource_jobs::has_pending() {
                    // 多等一拍：入队与轮询之间可能还隔着一次点击。
                    executor.timer(Duration::from_millis(120)).await;
                    if !resource_jobs::has_pending() {
                        break;
                    }
                }
            }
        });
        *self.resources_pump.borrow_mut() = Some(task);
    }

    /// 回填：成功推快照；失败给提示但**保留上一份列表**（取数失败不该清空用户正看的内容）。
    fn apply_resources_snapshot(
        &mut self,
        result: Result<ResourcesSnapshot, String>,
        cx: &mut Context<Self>,
    ) {
        let panel = self.resources_panel.clone();
        match result {
            Ok(snapshot) => panel.update(cx, |panel, cx| {
                panel.set_notice(None, cx);
                panel.set_snapshot(snapshot, cx);
            }),
            Err(err) => panel.update(cx, |panel, cx| panel.set_notice(Some(err), cx)),
        }
    }

    /// 转发渲染：视图（含滚动与空态）都在 crate 内，本面板不做二次包装。
    pub(super) fn render_resources_panel(&mut self, _cx: &mut Context<Self>) -> Div {
        let panel = self.resources_panel.clone();
        div().v_flex().size_full().min_h_0().child(panel)
    }
}
