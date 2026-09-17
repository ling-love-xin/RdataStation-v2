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

use analytics_resource::dialogs::version::VersionAction;
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
        let panel = cx.new(|cx| ResourcesPanel::new(host, cx));
        // 登记弱句柄：右侧「存档详情」要拿它的选中项（宿主级弱句柄，与 mock / insight 同例）。
        *shared.resources_panel.borrow_mut() = Some(panel.downgrade());
        panel
    }

    /// 请求一次列表刷新（**事件路径**）：入队 + 启动轮询回填。
    ///
    /// 未打开项目时不入队：没有项目就没有资产库（面板按空态呈现）。
    pub(crate) fn request_resources_refresh(&self, cx: &mut Context<Self>) {
        let Some(root) = self.shared.project_root() else {
            return;
        };
        let read_only = self.shared.project_ui.borrow().read_only;
        // 入队即置"加载中"：首个快照到达前状态行给提示、空态不闪（原型 §5）。
        let panel = self.resources_panel.clone();
        panel.update(cx, |panel, cx| panel.set_loading(true, cx));
        resource_jobs::enqueue_refresh(root, read_only);
        self.ensure_resources_pump(cx);
    }

    /// 执行一个版本历史动作（版本对话框的动作栏经此入队）。
    ///
    /// 忙态由对话框自己置上（按钮立即置灰），这里只负责校验与送作业；
    /// 只读项目下拒绝并把忙态收掉——否则那三个按钮会一直转不出结果。
    pub(crate) fn request_version_action(
        &self,
        resource_id: &str,
        name: &str,
        action: VersionAction,
        cx: &mut Context<Self>,
    ) {
        let read_only = self.shared.project_ui.borrow().read_only;
        let Some(root) = self.shared.project_root() else {
            self.finish_version_action("还没有打开项目".to_string(), cx);
            return;
        };
        if read_only {
            self.finish_version_action("项目为只读模式，不能改版本".to_string(), cx);
            return;
        }
        resource_jobs::enqueue_version_action(
            root,
            read_only,
            resource_id.to_string(),
            name.to_string(),
            action,
        );
        self.ensure_resources_pump(cx);
    }

    /// 收掉对话框的忙态并给状态栏回执（入队被挡、取数 / 动作失败都走它）。
    fn finish_version_action(&self, message: String, cx: &mut Context<Self>) {
        {
            let flow = self.shared.version_dialog.borrow();
            if let Some(session) = flow.session.as_ref() {
                session.state.set_busy(false);
                session.state.set_note(Some(message.clone()));
            }
        }
        *self.shared.notice.borrow_mut() = Some(format!("资产库：{message}"));
        self.shared.notify_host(cx);
        cx.notify();
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
                // 动作回执先于快照：它可能带着“顺手打开”（打开要在事件路径发出去）。
                if let Some(outcome) = resource_jobs::drain_op() {
                    if weak
                        .update(cx, |this, cx| this.apply_resource_op(outcome, cx))
                        .is_err()
                    {
                        return;
                    }
                }
                if let Some(result) = resource_jobs::drain_snapshot() {
                    if weak
                        .update(cx, |this, cx| this.apply_resources_snapshot(result, cx))
                        .is_err()
                    {
                        return;
                    }
                }
                if let Some(result) = resource_jobs::drain_versions() {
                    if weak
                        .update(cx, |this, cx| this.apply_versions(result, cx))
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
        // 无论成败都要收掉"加载中"：失败时停在加载态比给出提示更糟（用户会一直等）。
        panel.update(cx, |panel, cx| panel.set_loading(false, cx));
        match result {
            Ok(snapshot) => panel.update(cx, |panel, cx| {
                panel.set_notice(None, cx);
                panel.set_snapshot(snapshot, cx);
            }),
            Err(err) => panel.update(cx, |panel, cx| panel.set_notice(Some(err), cx)),
        }
    }

    /// 回填一份版本历史取数结果（开窗 / 刷新行都由它驱动）。
    ///
    /// 两条路：**已有会话**（同一个存档）→ 直接换行，对话框自己就更新了（还原完不必关窗重开）；
    /// **没有会话** → 置 `pending`，下一次侧栏渲染开窗（开窗要 `Window`，而这里没有）。
    fn apply_versions(
        &mut self,
        result: Result<resource_jobs::VersionRows, String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(rows) => {
                let mut opened = false;
                {
                    let mut flow = self.shared.version_dialog.borrow_mut();
                    if let Some(session) = flow.session.as_ref() {
                        if session.resource_id == rows.resource_id {
                            session.state.set_busy(false);
                            session.state.set_note(None);
                            session.state.set_rows(rows.seed.rows.clone());
                            opened = true;
                        }
                    }
                    if !opened {
                        flow.pending = Some(rows);
                    }
                }
                if !opened {
                    // 侧栏自己重绘一帧就能开窗；宿主也要重绘（状态栏回执一并到位）。
                    self.shared.notify_host(cx);
                    cx.notify();
                }
            }
            Err(err) => {
                self.finish_version_action(format!("读取版本历史失败：{err}"), cx);
            }
        }
    }

    /// 动作回执：状态栏文案 +（取回时）顺手打开 + 失败原样转述。
    ///
    /// 文案在**这里**组装（不在工作线程）：换算相对路径要项目根，写状态栏要 `Shared`——
    /// 两者都只在这个线程上。
    fn apply_resource_op(&mut self, outcome: resource_jobs::OpOutcome, cx: &mut Context<Self>) {
        // 撤销凭据随动作更新：**只有刚归档成功的那一次**留下窗口（5 秒后自走），
        // 其余动作（含失败与再次归档）都把上一个窗口收掉——两个并存的"反悔"只会让人选错。
        let undo = match &outcome {
            resource_jobs::OpOutcome::Archived { undo, .. } => undo.clone(),
            _ => None,
        };
        let panel = self.resources_panel.clone();
        panel.update(cx, |panel, cx| panel.set_undo(undo, cx));

        let message = match outcome {
            resource_jobs::OpOutcome::Archived {
                name,
                version,
                rel_path,
                undo: _,
            } => format!("资产库：已归档「{name}」v{version} → {rel_path}"),
            resource_jobs::OpOutcome::CheckedOut {
                dest,
                version,
                open_after,
            } => {
                // 展示用相对路径（相对项目根）：状态栏窄，绝对路径会把后半截挤掉。
                let shown = self
                    .shared
                    .project_root()
                    .and_then(|root| dest.strip_prefix(&root).ok().map(|rel| rel.to_path_buf()))
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|| dest.to_string_lossy().to_string());
                if open_after {
                    self.shared.request_open_in_editor(dest);
                }
                format!(
                    "资产库：已取回 {shown}（v{version} 的工作副本）；改完再归档将生成 v{}",
                    version + 1
                )
            }
            resource_jobs::OpOutcome::Undone { name } => {
                format!("资产库：已撤销归档「{name}」（本体已回到原位置）")
            }
            resource_jobs::OpOutcome::VersionActionDone { note } => format!("资产库：{note}"),
            resource_jobs::OpOutcome::Failed { action, reason } => {
                format!("资产库：{action}失败：{reason}")
            }
        };
        *self.shared.notice.borrow_mut() = Some(message);
        self.shared.notify_host(cx);
    }

    /// 转发渲染：视图（含滚动与空态）都在 crate 内，本面板不做二次包装。
    pub(super) fn render_resources_panel(&mut self, _cx: &mut Context<Self>) -> Div {
        let panel = self.resources_panel.clone();
        div().v_flex().size_full().min_h_0().child(panel)
    }
}
