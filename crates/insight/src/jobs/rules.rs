//! 洞察接缝 · 规则管理请求（自 jobs.rs 拆分，纯位移）

use super::*;
// ==================== 规则管理（Phase 2.3） ====================

/// 把规则管理对话框接上宿主：取数 / 写库 / 新建 / 打开文件都在接缝里完成。
///
/// 宿主仍然只写一行，且依然**不必知道事件形状**。
/// 订阅回调里只提交后台任务、**不回头改对话框状态**——回调是在 `emit` 的内层执行的，
/// 这时候改同一个实体就是重入（`cannot update … while it is already being updated`）。
pub fn attach_rules<H: 'static>(
    panel: &Entity<InsightView>,
    host_cx: &mut Context<H>,
    project_root: impl Fn() -> Option<PathBuf> + 'static,
) -> Subscription {
    let rules = panel.read(host_cx).rules_view().clone();
    let subscribed = rules.clone();
    host_cx.subscribe(
        &subscribed,
        move |_this, _emitter, event: &RulesEvent, cx| {
            let root = project_root();
            handle_rules_event(&rules, root, event, cx);
        },
    )
}

/// 规则事件 → 执行动作。
pub fn handle_rules_event(
    view: &Entity<RulesView>,
    project_root: Option<PathBuf>,
    event: &RulesEvent,
    cx: &mut App,
) {
    match event {
        RulesEvent::ReloadRequested => request_rules(view, project_root, cx),
        RulesEvent::ToggleRequested {
            rule_id,
            scope,
            enabled,
        } => request_rule_toggle(view, project_root, *scope, rule_id, *enabled, cx),
        RulesEvent::CreateRuleRequested { scope } => {
            request_create_rule(view, project_root, *scope, cx)
        }
        RulesEvent::OpenFileRequested { path } => open_in_system_editor(path),
        RulesEvent::TrustDecided { trusted } => {
            request_rules_trust(view, project_root, *trusted, cx)
        }
    }
}

/// 信任门的回答：写库 + 重装注册表 + 回填列表（三件事都在接缝里，视图不碰库）。
pub fn request_rules_trust(
    view: &Entity<RulesView>,
    project_root: Option<PathBuf>,
    trusted: bool,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let state = if trusted {
        RuleTrust::Trusted
    } else {
        RuleTrust::Declined
    };
    let task = cx.background_executor().spawn(async move {
        InsightService::decide_project_rules_trust(project_root.as_deref(), state)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |view, cx| match result {
            Ok(data) => view.set_data(data, cx),
            // 写库失败时不改信任状态：回填真值（仍是未信任），只提一句
            Err(err) => view.set_notice(
                format!(
                    "信任状态未保存：{}",
                    InsightService::describe_error(&err).message
                ),
                cx,
            ),
        });
    })
    .detach();
}

/// 取数：同步索引 → 组装视图模型（阻塞段全在后台执行器上）。
pub fn request_rules(view: &Entity<RulesView>, project_root: Option<PathBuf>, cx: &mut App) {
    let weak = view.downgrade();
    let task = cx
        .background_executor()
        .spawn(async move { InsightService::rules_data(project_root.as_deref()) });
    cx.spawn(async move |cx| {
        let result = task.await;
        // 对话框可能早已关掉：弱句柄升级失败就丢弃结果（不 panic）
        let _ = weak.update(cx, |view, cx| match result {
            Ok(data) => view.set_data(data, cx),
            Err(err) => view.set_error(InsightService::describe_error(&err).message, cx),
        });
    })
    .detach();
}

/// 开关：写索引 → 重新同步 → 回填新数据。
///
/// 失败走 [`RulesView::set_notice`] 而不是整体错误态：列表本身是好的，
/// 只是这一次没存上；整页变错误会让用户以为规则列表坏了。
pub fn request_rule_toggle(
    view: &Entity<RulesView>,
    project_root: Option<PathBuf>,
    scope: RuleScope,
    rule_id: &str,
    enabled: bool,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let rule_id = rule_id.to_string();
    let task = cx.background_executor().spawn(async move {
        // 写库与重读在同一个后台任务里：中间没有可观察的中间态
        InsightService::toggle_rule(project_root.as_deref(), scope, &rule_id, enabled)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |view, cx| match result {
            Ok(data) => view.set_data(data, cx),
            Err(err) => view.set_notice(
                format!(
                    "启停未保存：{}",
                    InsightService::describe_error(&err).message
                ),
                cx,
            ),
        });
    })
    .detach();
}

/// 新建规则：建目录 + 写模板 → 刷新列表 → 在系统编辑器中打开。
pub fn request_create_rule(
    view: &Entity<RulesView>,
    project_root: Option<PathBuf>,
    scope: RuleScope,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        let path = InsightService::create_rule_file(project_root.as_deref(), scope)?;
        let data = InsightService::rules_data(project_root.as_deref())?;
        Ok::<_, shared::error::CoreError>((path, data))
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |view, cx| match result {
            Ok((path, data)) => {
                view.set_data(data, cx);
                open_in_system_editor(&path);
            }
            Err(err) => view.set_notice(
                format!(
                    "新建规则失败：{}",
                    InsightService::describe_error(&err).message
                ),
                cx,
            ),
        });
    })
    .detach();
}

/// 在**系统默认应用**里打开规则文件。
///
/// 规则正文是项目里的 TOML（可 diff、可进 git），系统编辑器是正确的落点：
/// 本应用没有 TOML 语法支持，塞进 SQL 编辑器只会得到一屏纯文本。
/// 只拉起进程、不等待——个别桌面环境下 `start` / `xdg-open` 会卡几秒。
pub fn open_in_system_editor(path: &Path) {
    // 单测里不真的拉起外部进程：会弹出编辑器，`cmd.exe` 的输出还会混进测试日志。
    // 「要打开哪个文件」由调用方的事件断言盖住，这里只跳过最后那一步进程启动。
    if cfg!(test) {
        tracing::debug!("跳过系统编辑器（测试构建）：{}", path.display());
        return;
    }

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        // `start` 的第一个参数是窗口标题：给空标题，否则带空格的路径会被当成标题
        command.args(["/C", "start", ""]).arg(path);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };

    if let Err(e) = command.spawn() {
        tracing::warn!("无法在系统编辑器中打开 {}：{}", path.display(), e);
    }
}