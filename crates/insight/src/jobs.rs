//! 洞察的**事件接缝 + 后台取数**（M8 Phase 1）。
//!
//! # 为什么在特性 crate 内
//!
//! 耦合治理计划 `docs/architecture/layout/panels-coupling-plan.md` §5 定的目标形态是
//! 「特性 crate 内的 jobs + 面板 drain」。放这里还有一层理由：Phase 2 的「评估全表 + 进度」
//! 与 Phase 5 的快照保存都要用同一套后台形态，先用对位置就不用写两遍。
//! 宿主（workbench）侧因此只剩「提供项目根」这一行胶水（见 [`attach`]）。
//!
//! # 为什么需要后台
//!
//! 列画像要抢 DuckDB 全局锁并逐条跑规则（多次往返），在 UI 线程 `block_on` 会冻结界面。
//! 本模块把**阻塞段放到后台执行器**，跑完再把结果回填面板——面板只发 [`InsightEvent`]。
//!
//! # 线程边界
//!
//! `Shared` 里的 `Rc<RefCell<…>>` 不可跨线程，所以**项目根在提交时解析成所有权数据**；
//! 后台闭包不碰任何宿主状态。

use std::path::{Path, PathBuf};

use gpui_kit::{App, Context, Entity, Subscription};

use crate::insight_view::{InsightEvent, InsightView};
use crate::model::InsightTarget;
use crate::rule::RuleScope;
use crate::rule_view::{RulesEvent, RulesView};
use crate::service::InsightService;

/// 一次列画像请求：后台执行所需的全部输入（均为所有权数据，因此可跨线程）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRequest {
    pub temp_table: String,
    pub column: String,
}

impl ProfileRequest {
    /// 从面板目标解析出请求；**当前不支持的形态返回 `None`**。
    ///
    /// Phase 1 只做列画像：表探查 / 多列 / 结构在后续期次落地，面板侧已按 Tab 给出期次提示，
    /// 这里就不再假装有行为。
    pub fn of(target: &InsightTarget) -> Option<Self> {
        match target {
            InsightTarget::Column {
                temp_table, column, ..
            } => Some(Self {
                temp_table: temp_table.clone(),
                column: column.clone(),
            }),
            InsightTarget::Table { .. }
            | InsightTarget::MultiColumn { .. }
            | InsightTarget::Schema { .. } => None,
        }
    }
}

/// 把面板接上宿主：面板发 `ProfileRequested`，本模块在后台取数后回填。
///
/// 宿主侧只需一行，且**不必知道事件形状**：
/// ```ignore
/// let _sub = insight::jobs::attach(&panel, cx, move || shared.project_root());
/// ```
///
/// `project_root` 是**闭包而不是值**：每次请求时才解析（项目可能已切换），
/// 而解析结果（`PathBuf`）在提交任务前就脱离宿主状态，因此可以跨线程。
pub fn attach<H: 'static>(
    view: &Entity<InsightView>,
    host_cx: &mut Context<H>,
    project_root: impl Fn() -> Option<PathBuf> + 'static,
) -> Subscription {
    let panel = view.clone();
    host_cx.subscribe(view, move |_this, _emitter, event: &InsightEvent, cx| {
        let root = project_root();
        handle_event(&panel, root, event, cx);
    })
}

/// 面板事件 → 执行动作（宿主侧唯一入口；`attach` 内部也走这里）。
pub fn handle_event(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    event: &InsightEvent,
    cx: &mut App,
) {
    match event {
        InsightEvent::ProfileRequested { target } => {
            let Some(request) = ProfileRequest::of(target) else {
                return;
            };
            request_profile(view, project_root, request, cx);
        }
    }
}

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
    host_cx.subscribe(&subscribed, move |_this, _emitter, event: &RulesEvent, cx| {
        let root = project_root();
        handle_rules_event(&rules, root, event, cx);
    })
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
    }
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
                format!("启停未保存：{}", InsightService::describe_error(&err).message),
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
                format!("新建规则失败：{}", InsightService::describe_error(&err).message),
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

/// 提交一次画像请求：后台执行 → 回填面板。
///
/// 回填只走面板的公开方法（`set_profile` / `set_error`），错误经 `describe_error` 转成
/// 「给人看的文案 + 是否可重试」——面板因此永远不接触 `CoreError`。
pub fn request_profile(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    request: ProfileRequest,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        InsightService::profile_column_view(
            project_root.as_deref(),
            &request.temp_table,
            &request.column,
        )
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        // 面板可能已关闭：弱句柄升级失败就丢弃结果（不 panic）
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(profile) => panel.set_profile(profile, cx),
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_error(info.message, info.retryable, cx);
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use gpui_kit::{AppContext as _, Context, TestAppContext};

    use super::{ProfileRequest, attach, attach_rules};
    use crate::insight_view::InsightView;
    use crate::model::InsightTarget;
    use crate::rule::RuleScope;
    use crate::rule_view::{RuleRowStatus, RulesDialogState};

    /// 宿主替身：真实宿主只多持一个订阅句柄（`_sub`）
    struct TestHost;

    impl TestHost {
        fn new(_: &mut Context<Self>) -> Self {
            Self
        }
    }

    #[test]
    fn column_target_maps_to_a_request() {
        let target = InsightTarget::Column {
            temp_table: "t_result_1".into(),
            column: "amount".into(),
            data_type: "DECIMAL(12,2)".into(),
        };
        assert_eq!(
            ProfileRequest::of(&target),
            Some(ProfileRequest {
                temp_table: "t_result_1".into(),
                column: "amount".into(),
            })
        );
    }

    #[test]
    fn later_phase_targets_are_not_pretended() {
        // Phase 1 只做列画像：其余目标没有生产者就不该被当成有行为
        for target in [
            InsightTarget::Table {
                temp_table: "t".into(),
                table_name: "orders".into(),
            },
            InsightTarget::MultiColumn {
                temp_table: "t".into(),
                columns: vec!["a".into()],
            },
            InsightTarget::Schema {
                conn_id: "G_1".into(),
                schema: None,
            },
        ] {
            assert_eq!(ProfileRequest::of(&target), None, "{target:?}");
        }
    }

    /// 宿主只写一行（`attach`），所以这一行的契约必须在本 crate 内验住：
    /// 面板一发目标，宿主提供的项目根就被取用、后台取数真的跑起来并回填。
    #[gpui_kit::test]
    fn attach_turns_a_panel_request_into_a_fetch(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let root = Arc::new(Mutex::new(Some(PathBuf::from("/tmp/rds-测试项目"))));
        let root_for_host = root.clone();
        let calls = Arc::new(Mutex::new(0usize));
        let calls_for_host = calls.clone();

        // 宿主实体必须被持有：仅靠 `Subscription` 不足以让订阅者存活（实体一旦释放，
        // 订阅就被剪掉）——这正好也是真实宿主的形态（面板实体活到窗口关）。
        let (host, panel, _sub) = cx.update(|cx| {
            let host = cx.new(TestHost::new);
            let panel = cx.new(InsightView::new);
            let sub = host.update(cx, |_host, host_cx| {
                attach(&panel, host_cx, move || {
                    *calls_for_host.lock().unwrap() += 1;
                    root_for_host.lock().unwrap().clone()
                })
            });
            (host, panel, sub)
        });
        let _host = host;

        // 目标不存在：走的是真实失败路径（DuckDB 内存库），因此能证明订阅确实把请求
        // 转成了取数并回填——而不是只验了「不 panic」。
        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                panel.set_target(
                    InsightTarget::Column {
                        temp_table: "t_insight_attach_absent".into(),
                        column: "amount".into(),
                        data_type: "INTEGER".into(),
                    },
                    cx,
                );
            });
        });
        cx.update(|_cx| {
            assert_eq!(*calls.lock().unwrap(), 1, "订阅应被触发（项目根提供者被问了一次）");
        });
        cx.run_until_parked();

        cx.update(|cx| {
            let state = panel.read(cx).state();
            assert!(
                state.is_error(),
                "订阅应把请求转成取数并回填错误态，实际：{state:?}"
            );
        });
    }

    // ==================== 规则管理接缝（Phase 2.3） ====================

    /// 一条最小合法规则（能解析、且因 `applies_to = []` 暂不参与分析）
    const DEMO_RULE: &str = r#"
[meta]
id = "demo-rule"
name = "示例项目规则"
description = ""
version = "1.0"
category = "column"
applies_to = []
builtin = false

[query]
template = "SELECT COUNT(*) AS total FROM \"{table}\""
parameters = ["table"]
result_type = "single"

[[output]]
sql_name = "total"
json_name = "total_count"
value_type = "i64"
"#;

    /// 临时项目：`.RSmeta/insight-rules/` 已建好（规则目录是项目自有的部分）
    fn temp_project(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("rds_rules_jobs_{}_{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".RSmeta/insight-rules")).expect("建临时项目");
        root
    }

    /// 宿主只写一行（`attach_rules`），这一行的契约必须在本 crate 内验住：
    /// 对话框一发请求，项目根就被取用、索引真的与磁盘对齐、数据真的回填，
    /// 且**开关注得进规则集**（否则索引就成了只写不读的装饰）。
    #[gpui_kit::test]
    fn attach_rules_loads_and_toggles_through_a_real_project(cx: &mut TestAppContext) {
        // 同步索引会写进程级启停集合缓存：与同类的缓存测试串行（本仓约定）
        let _guard = crate::tests::rule_state_guard();
        cx.update(gpui_kit::init);
        let root = temp_project("attach_rules");
        std::fs::write(root.join(".RSmeta/insight-rules/demo.rule.toml"), DEMO_RULE)
            .expect("写一条项目规则");

        let root_for_host = root.clone();
        let (host, _panel, rules, _sub) = cx.update(|cx| {
            let host = cx.new(TestHost::new);
            let panel = cx.new(InsightView::new);
            let rules = panel.read(cx).rules_view().clone();
            let sub = host.update(cx, |_host, host_cx| {
                attach_rules(&panel, host_cx, move || Some(root_for_host.clone()))
            });
            (host, panel, rules, sub)
        });
        let _host = host;

        // 打开对话框的等价入口（不弹窗，直接发事件）：同步索引 → 读数 → 回填
        cx.update(|cx| rules.update(cx, |view, cx| view.request_reload(cx)));
        cx.run_until_parked();

        cx.update(|cx| {
            let view = rules.read(cx);
            assert_eq!(
                view.state(),
                &RulesDialogState::Ready,
                "取数应成功回填（实际：{:?}）",
                view.state()
            );
            let project = view.data().group(RuleScope::Project).expect("项目分组");
            assert_eq!(project.rows.len(), 1, "磁盘上的项目规则应进索引");
            assert_eq!(project.rows[0].id, "demo-rule");
            assert_eq!(project.rows[0].name, "示例项目规则");
            assert!(!project.dir_missing, "目录已存在");
            assert!(
                view.data().total > 18,
                "内置 18 条 + 项目规则，实际 {}",
                view.data().total
            );
        });

        // 开关：写索引 → 重新同步 → 推进启停集合 → 注册表失效（下次取数就不再包含它）
        assert!(
            crate::with_rules(Some(&root), |registry| Ok(registry.get("demo-rule").is_some()))
                .expect("取规则集")
        );
        cx.update(|cx| {
            rules.update(cx, |view, cx| {
                view.request_toggle(RuleScope::Project, "demo-rule", false, cx)
            });
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let row = &rules
                .read(cx)
                .data()
                .group(RuleScope::Project)
                .unwrap()
                .rows[0];
            assert!(!row.enabled, "回填的数据应反映禁用");
            assert!(
                !crate::with_rules(Some(&root), |registry| Ok(registry
                    .get("demo-rule")
                    .is_some()))
                .expect("取规则集"),
                "禁用必须一路走到规则集（否则索引只是只写不读的装饰）"
            );
        });

        let _ = std::fs::remove_dir_all(&root);
        crate::clear_disabled_rules_cache();
    }

    /// 新建规则：目录首次写入时创建（K7），模板能解析且**暂不生效**
    #[gpui_kit::test]
    fn attach_rules_creates_a_parseable_template(cx: &mut TestAppContext) {
        // 同步索引会写进程级启停集合缓存：与同类的缓存测试串行（本仓约定）
        let _guard = crate::tests::rule_state_guard();
        cx.update(gpui_kit::init);
        // 只建项目根，**不建**规则目录：目录必须由「新建」自己建出来
        let root = std::env::temp_dir().join(format!("rds_rules_create_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("建临时项目");

        let root_for_host = root.clone();
        let (host, _panel, rules, _sub) = cx.update(|cx| {
            let host = cx.new(TestHost::new);
            let panel = cx.new(InsightView::new);
            let rules = panel.read(cx).rules_view().clone();
            let sub = host.update(cx, |_host, host_cx| {
                attach_rules(&panel, host_cx, move || Some(root_for_host.clone()))
            });
            (host, panel, rules, sub)
        });
        let _host = host;

        cx.update(|cx| {
            rules.update(cx, |view, cx| {
                view.request_create_rule(RuleScope::Project, cx)
            });
        });
        cx.run_until_parked();

        cx.update(|cx| {
            let view = rules.read(cx);
            assert_eq!(view.state(), &RulesDialogState::Ready);
            let project = view.data().group(RuleScope::Project).expect("项目分组");
            assert!(!project.dir_missing, "目录应由新建自己创建");
            assert_eq!(project.rows.len(), 1);
            assert_eq!(project.rows[0].id, "new-rule");
            assert!(
                project.rows[0].status == RuleRowStatus::Ok,
                "模板必须能解析，否则用户第一眼看到的就是一条红错：{:?}",
                project.rows[0].status
            );
            assert!(project.rows[0].file.is_some(), "新建后会交给系统编辑器打开");
            assert!(
                crate::builtin_registry()
                    .all_rules()
                    .iter()
                    .all(|r| r.meta.id != "new-rule"),
                "模板 id 不得与内置规则撞车"
            );
        });

        let _ = std::fs::remove_dir_all(&root);
    }
}
