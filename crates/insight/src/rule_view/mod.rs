//! 规则管理对话框（M8 Phase 2.3）。
//!
//! # 为什么是对话框而不是面板 Tab
//!
//! 三层分组 + 每行状态 + 错误原文需要宽度（280px 的右 Dock 放不下），
//! 且「配置分析器」与「看分析结果」是两类活动——与「项目设置 / 连接对话框」同口径。
//!
//! # 三条职责边界（与面板 `insight_view.rs` 完全一致）
//!
//! 1. **不做 I/O**：本视图只渲染 [`RulesData`]，取数与写库经 `jobs::attach_rules` 走后台
//!    （D20）。因此打开对话框 = 「开窗 + 发一次取数请求」，不是「自己读盘」。
//! 2. **写库只有一条路**：`RuleIndexStore::set_enabled`（索引的唯一用户写入入口），
//!    改完由接缝重新同步 → 注册表缓存失效。视图不碰规则正文，也不删文件。
//! 3. **不持色值**：语义（[`RuleRowStatus`] / 分组）→ `theme.colors.*` 的映射在渲染层。
//!
//! # 与 v1 的关键差异
//!
//! v1 的解析错误只写 `tracing::warn!`——用户在界面上只会看到「规则莫名其妙不见了」。
//! 本视图把 `load_error` 原文逐条摊开，这是规则格式（`deny_unknown_fields` 严格 schema）
//! 唯一的可见出口。

use std::collections::HashSet;
// 显式引入：`gpui_kit::*` 里也有一个 `Path`（场景路径），不写清楚会静默取错那个
use std::path::{Path, PathBuf};

use gpui_kit::base::StyledExt;
use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _, Size, Theme, WindowExt as _};
use gpui_kit::*;

use crate::rule::RuleScope;
use crate::rule_registry::PendingProjectRules;
use crate::rule_types::RuleMeta;
use crate::service::indexer::{RuleIndexEntry, RuleLoadStatus};
use crate::ui;

// ==================== 子模块（拆分：数据模型 / 视图渲染分离，纯位移） ====================
mod data;
mod view;

// 聚合 re-export：保持 `crate::rule_view::*` 路径与 lib.rs re-export 不变
pub use data::*;
pub use view::*;
#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use gpui_kit::component::{Root, WindowExt as _};
    use gpui_kit::{
        AppContext as _, Entity, IntoElement, ParentElement, Render, Styled as _, Subscription,
        TestAppContext, VisualTestContext, Window, div,
    };

    use super::{
        GROUP_ORDER, PendingProjectRules, PendingRulesView, RuleDataInput, RuleRowStatus,
        RulesData, RulesEvent, RulesView, build_rules_data,
    };
    use super::data::row_matches;
    use crate::rule::RuleScope;
    use crate::rule_types::RuleMeta;
    use crate::service::indexer::{RuleIndexEntry, RuleLoadStatus};

    fn meta(id: &str, name: &str) -> RuleMeta {
        RuleMeta {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            version: "1.0".into(),
            category: "column".into(),
            applies_to: vec!["Any".into()],
            builtin: true,
        }
    }

    fn index_row(id: &str, scope: RuleScope, enabled: bool) -> RuleIndexEntry {
        RuleIndexEntry {
            rule_id: id.into(),
            scope,
            category: "column".into(),
            name: format!("{id}-名称"),
            version: "1.2".into(),
            source_path: format!("{id}.rule.toml"),
            checksum: "c".into(),
            enabled,
            load_status: RuleLoadStatus::Ok,
            load_error: None,
        }
    }

    /// 抑制记录：同步器写的只有 id 与状态，没有来源文件
    fn suppression(id: &str, enabled: bool) -> RuleIndexEntry {
        RuleIndexEntry {
            rule_id: id.into(),
            scope: RuleScope::Project,
            category: String::new(),
            name: String::new(),
            version: String::new(),
            source_path: String::new(),
            checksum: String::new(),
            enabled,
            load_status: RuleLoadStatus::Ok,
            load_error: None,
        }
    }

    fn input() -> RuleDataInput {
        RuleDataInput {
            project_dir: Some(PathBuf::from("/p/.RSmeta/insight-rules")),
            global_dir: Some(PathBuf::from("/g/insight-rules")),
            project_rows: vec![index_row("my-rule", RuleScope::Project, true)],
            global_rows: vec![index_row("shared-rule", RuleScope::Global, true)],
            builtin: vec![
                meta("null-check", "空值检查"),
                meta("numeric-stats", "数值统计"),
            ],
            pending: None,
            trust_declined: false,
        }
    }

    #[test]
    fn groups_are_ordered_by_override_priority() {
        let data = build_rules_data(input());
        let scopes: Vec<RuleScope> = data.groups.iter().map(|g| g.scope).collect();
        assert_eq!(scopes, GROUP_ORDER.to_vec());
        assert_eq!(
            scopes,
            vec![RuleScope::Project, RuleScope::Global, RuleScope::Builtin],
            "项目在前：先让用户看到「谁赢」"
        );
        assert_eq!(data.groups[0].title, "项目规则");
        assert_eq!(data.groups[2].location, "应用内嵌");
    }

    #[test]
    fn suppression_records_disable_builtin_rules_without_showing_as_project_rules() {
        let mut input = input();
        // 用户在项目里关掉了内置规则 `null-check`
        input.project_rows.push(suppression("null-check", false));

        let data = build_rules_data(input);
        let project = data.group(RuleScope::Project).unwrap();
        assert_eq!(
            project.rows.len(),
            1,
            "抑制记录不是项目规则，不得混进项目分组"
        );
        assert_eq!(project.rows[0].id, "my-rule");

        let builtin = data.group(RuleScope::Builtin).unwrap();
        let null_check = builtin.rows.iter().find(|r| r.id == "null-check").unwrap();
        assert!(!null_check.enabled, "抑制记录应让内置规则显示为已禁用");
        let numeric = builtin
            .rows
            .iter()
            .find(|r| r.id == "numeric-stats")
            .unwrap();
        assert!(numeric.enabled);
        assert!(numeric.file.is_none(), "内置规则没有可打开的文件");
        assert_eq!(data.disabled, 1);
    }

    /// 用户完全可以写一个与内置规则**同 id** 的项目规则文件（那就是覆盖，不是抑制）
    #[test]
    fn a_project_file_with_a_builtin_id_is_a_real_rule() {
        let mut input = input();
        input
            .project_rows
            .push(index_row("null-check", RuleScope::Project, true));

        let data = build_rules_data(input);
        let project = data.group(RuleScope::Project).unwrap();
        assert_eq!(project.rows.len(), 2, "有来源文件的行就是真规则");
        assert!(
            data.group(RuleScope::Builtin)
                .unwrap()
                .rows
                .iter()
                .all(|r| r.enabled),
            "覆盖 != 禁用"
        );
    }

    #[test]
    fn invalid_and_missing_rows_are_reported_with_their_own_state() {
        let mut input = input();
        input.project_rows.push(RuleIndexEntry {
            load_status: RuleLoadStatus::Invalid,
            load_error: Some("第 3 行未知字段 `outputs`（期望 output）".into()),
            ..index_row("broken", RuleScope::Project, true)
        });
        input.global_rows.push(RuleIndexEntry {
            load_status: RuleLoadStatus::Missing,
            ..index_row("gone", RuleScope::Global, true)
        });

        let data = build_rules_data(input);
        assert_eq!(data.broken, 2);
        let broken = data
            .group(RuleScope::Project)
            .unwrap()
            .rows
            .iter()
            .find(|r| r.id == "broken")
            .unwrap();
        match &broken.status {
            RuleRowStatus::Invalid { error } => {
                assert!(error.contains("未知字段"), "错误原文必须能展示：{error}");
            }
            other => panic!("应为解析失败，实际 {other:?}"),
        }
        assert!(broken.file.is_some(), "解析失败的文件仍在，可以打开修");

        let gone = data
            .group(RuleScope::Global)
            .unwrap()
            .rows
            .iter()
            .find(|r| r.id == "gone")
            .unwrap();
        assert_eq!(gone.status, RuleRowStatus::Missing);
        assert!(gone.file.is_none(), "文件已不在，不给必然失败的开入口");
    }

    /// 信任门（Q1 ③）：信任状态是**库里的决定**，视图不得自己就地改。
    #[gpui_kit::test]
    fn pending_banner_actions_emit_decision_without_touching_local_state(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (rules, sink, cx) = open_dialog_harness(cx);
        cx.update(|window, cx| rules.update(cx, |view, cx| view.begin_load(window, cx)));

        let mut data = sample_data();
        data.pending = Some(PendingRulesView {
            ids: vec!["gated-a".into(), "gated-b".into()],
            invalid: 1,
            dir: "/p/.RSmeta/insight-rules".into(),
            declined: false,
        });
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_data(data.clone(), cx)));
        draw(cx);

        // 首次拿到带 pending 的数据 → 叠一个确认框（规则对话框仍在下面）
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "首次遇到未信任的项目规则应弹确认框"
        );

        // 两个按钮共用的那条路：只发事件，不改本视图的 pending
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.request_trust(true, cx)));
        let seen = sink.read_with(cx, |sink, _| sink.events.borrow().clone());
        assert_eq!(
            seen.last(),
            Some(&RulesEvent::TrustDecided { trusted: true })
        );
        rules.update(cx, |view, _| {
            assert!(
                view.data().pending.is_some(),
                "决定要等库里写了、重新装配后才生效（否则界面会撒谎）"
            );
            assert!(view.trust_prompted, "问过一次就不再追问");
        });

        // 第二次回填（例如 ⟳ 刷新）不该重开确认框；横幅照旧可渲染
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_data(data.clone(), cx)));
        draw(cx);
        rules.update(cx, |view, _| assert!(view.trust_prompted));

        // 改主意（拒绝）走同一条路
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.request_trust(false, cx)));
        let seen = sink.read_with(cx, |sink, _| sink.events.borrow().clone());
        assert_eq!(
            seen.last(),
            Some(&RulesEvent::TrustDecided { trusted: false })
        );
    }

    /// 搜索不该把信任门横幅过滤掉（它不属于任何分组）。
    #[test]
    fn pending_survives_filtering() {
        let mut input = input();
        input.pending = Some(PendingProjectRules {
            dir: PathBuf::from("/p/.RSmeta/insight-rules"),
            ids: vec!["a".into(), "b".into(), "c".into()],
            invalid: 0,
        });
        let data = build_rules_data(input);
        assert!(data.pending.is_some(), "有 pending 数据时应出现在横幅上");

        let filtered = data.filtered("不存在的词");
        assert!(filtered.pending.is_some(), "搜索不该弄丢横幅");
        assert_eq!(filtered.visible_rows(), 0, "但行确实被过滤了");
    }

    /// 横幅文案：超过 6 条只列前 6 条，用「等 N 条」收尾（不至于把对话框撞开）。
    #[test]
    fn pending_summary_is_bounded() {
        let view = PendingRulesView {
            ids: (0..8).map(|i| format!("rule-{i}")).collect(),
            invalid: 0,
            dir: "/p".into(),
            declined: false,
        };
        assert_eq!(view.count(), 8);
        let summary = view.summary();
        assert!(
            summary.contains("rule-0") && summary.contains("rule-5"),
            "{summary}"
        );
        assert!(!summary.contains("rule-6"), "只列前 6 条：{summary}");
        assert!(summary.contains("等 8 条"), "多的部分要收尾：{summary}");
    }

    #[test]
    fn statistics_count_every_layer() {
        let mut input = input();
        input.project_rows.push(suppression("null-check", false));
        input
            .project_rows
            .push(index_row("off", RuleScope::Project, false));
        input.builtin.push(meta("table-rows", "表行数"));

        let data = build_rules_data(input);
        assert_eq!(data.total, 6, "2 项目（my-rule / off）+ 1 全局 + 3 内置");
        assert_eq!(data.disabled, 2, "关掉的内置 + 关掉的项目规则");
        assert_eq!(data.broken, 0);
    }

    #[test]
    fn filtering_matches_name_id_and_category_but_keeps_group_skeleton() {
        let mut input = input();
        input.builtin.push(meta("table-rows", "表行数"));
        let data = build_rules_data(input);

        let hit = data.filtered("数值");
        assert_eq!(hit.group(RuleScope::Builtin).unwrap().rows.len(), 1);
        assert!(hit.group(RuleScope::Project).unwrap().rows.is_empty());
        assert_eq!(
            hit.total, data.total,
            "统计数字取全量：搜索不该让「共 N 条」变小"
        );
        assert_eq!(hit.groups.len(), 3, "分组骨架保留（空层仍显示落点与提示）");

        assert_eq!(data.filtered("  ").groups, data.groups, "空搜索词 = 不过滤");
        assert_eq!(data.filtered("MY-RULE").total, data.total);
        let by_id = data.filtered("table-rows");
        assert_eq!(by_id.visible_rows(), 1, "id 也能搜到");
    }

    #[test]
    fn row_matching_is_case_insensitive() {
        let row = index_row("My-Rule", RuleScope::Project, true);
        let view = build_rules_data(RuleDataInput {
            project_rows: vec![row],
            ..Default::default()
        })
        .groups
        .into_iter()
        .next()
        .unwrap()
        .rows
        .into_iter()
        .next()
        .unwrap();
        assert!(row_matches(&view, "my-rule"));
        assert!(row_matches(&view, "名称"));
        assert!(!row_matches(&view, "不存在"));
    }

    #[test]
    fn missing_dirs_are_reported_so_the_ui_can_offer_creation() {
        let input = RuleDataInput {
            project_dir: Some(PathBuf::from("/definitely-not-here-rds/x")),
            global_dir: None,
            ..Default::default()
        };
        let data = build_rules_data(input);
        assert!(data.group(RuleScope::Project).unwrap().dir_missing);
        assert!(!data.group(RuleScope::Builtin).unwrap().dir_missing);
        assert_eq!(
            data.group(RuleScope::Global).unwrap().location,
            "（不可用）",
            "系统目录拿不到时要说明，而不是给一个空路径"
        );
    }

    #[test]
    fn empty_data_has_three_empty_groups() {
        let data = RulesData::default();
        assert_eq!(data.groups.len(), 3);
        assert_eq!(data.visible_rows(), 0);
        assert_eq!(data.total, 0);
    }

    // ==================== 实体与窗口 ====================

    /// 记录对话框发出的事件（宿主替身；事件路径的断言都靠它）
    struct Sink {
        events: Rc<RefCell<Vec<RulesEvent>>>,
        _sub: Subscription,
    }

    impl Sink {
        fn attach(rules: &Entity<RulesView>, cx: &mut gpui_kit::Context<Self>) -> Self {
            let events: Rc<RefCell<Vec<RulesEvent>>> = Rc::new(RefCell::new(Vec::new()));
            let sink = events.clone();
            let _sub = cx.subscribe(rules, move |_this, _rules, event: &RulesEvent, _cx| {
                sink.borrow_mut().push(event.clone());
            });
            Self { events, _sub }
        }
    }

    /// 窗口根必须是组件库的 `Root`（`open_dialog` 依赖它；gpui-kit 0.7.1 起 dialog 层由 Root 自动挂载）
    struct Harness {
        rules: Entity<RulesView>,
        _sink: Entity<Sink>,
    }

    impl Render for Harness {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut gpui_kit::Context<Self>,
        ) -> impl IntoElement {
            let root = div().size_full().child(self.rules.clone());
            // gpui-kit 0.7.1：Root 自动挂载 dialog 层，测试 Harness 无需手动渲染。
            root
        }
    }

    fn open_dialog_harness(
        cx: &mut TestAppContext,
    ) -> (Entity<RulesView>, Entity<Sink>, &mut VisualTestContext) {
        // 两个实体都不需要窗口，先建好再包一层 Harness 实体当窗口根
        let rules = cx.new(RulesView::new);
        let sink = cx.new(|cx| Sink::attach(&rules, cx));
        let rules_in = rules.clone();
        let sink_in = sink.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let harness = cx.new(|_cx| Harness {
                rules: rules_in,
                _sink: sink_in,
            });
            Root::new(harness, window, cx)
        });
        (rules, sink, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn sample_data() -> RulesData {
        build_rules_data(RuleDataInput {
            project_dir: Some(PathBuf::from("/p/.RSmeta/insight-rules")),
            global_dir: Some(PathBuf::from("/g/insight-rules")),
            project_rows: vec![index_row("my-rule", RuleScope::Project, true)],
            global_rows: Vec::new(),
            builtin: vec![meta("null-check", "空值检查")],
            pending: None,
            trust_declined: false,
        })
    }

    /// ⚙ → 弹窗 + 发一次取数请求；数据回填后逐帧可渲染（三态都不 panic）
    #[gpui_kit::test]
    fn dialog_opens_requests_data_and_renders(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (rules, sink, cx) = open_dialog_harness(cx);

        cx.update(|window, cx| {
            rules.update(cx, |view, cx| view.begin_load(window, cx));
        });
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "⚙ 应弹出规则管理对话框"
        );
        assert_eq!(
            sink.read_with(cx, |sink, _| sink.events.borrow().clone()),
            vec![RulesEvent::ReloadRequested],
            "开窗就要发取数请求（视图自己不读盘）"
        );

        // 取数失败 → 错误态可渲染，重试仍是一个事件
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_error("项目库被占用", cx)));
        draw(cx);
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.request_reload(cx)));

        // 取数成功 → 数据态：三组 + 开关 + 状态行
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_data(sample_data(), cx)));
        draw(cx);
        rules.update(cx, |view, _| {
            assert_eq!(view.state(), &super::RulesDialogState::Ready);
            assert_eq!(view.visible().visible_rows(), 2, "1 项目 + 1 内置");
        });

        // 搜索（含清空）→ 事件路径重算可见行
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_query("空值", cx)));
        draw(cx);
        rules.update(cx, |view, _| {
            assert_eq!(view.visible().visible_rows(), 1);
            assert_eq!(view.data().total, 2, "统计仍取全量");
        });
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_query("", cx)));
        draw(cx);
        rules.update(cx, |view, _| assert_eq!(view.visible().visible_rows(), 2));

        // 折叠分组：Accordion 回调给的是「当前展开的下标集合」
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_open_groups(&[1], cx)));
        draw(cx);
        rules.update(cx, |view, _| {
            assert_eq!(view.groups_open(), [false, true, false])
        });
    }

    /// 开关：就地翻位（不等后台往返）+ 发事件；写入失败时列表不消失
    #[gpui_kit::test]
    fn toggling_flips_locally_and_only_emits(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (rules, sink, cx) = open_dialog_harness(cx);
        cx.update(|window, cx| rules.update(cx, |view, cx| view.begin_load(window, cx)));
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_data(sample_data(), cx)));
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| {
                view.request_toggle(RuleScope::Project, "my-rule", false, cx)
            });
        });

        let seen = sink.read_with(cx, |sink, _| sink.events.borrow().clone());
        assert_eq!(
            seen.last(),
            Some(&RulesEvent::ToggleRequested {
                rule_id: "my-rule".into(),
                scope: RuleScope::Project,
                enabled: false,
            }),
            "开关只发事件：写库由接缝在后台做"
        );
        rules.update(cx, |view, _| {
            let row = &view.visible().group(RuleScope::Project).unwrap().rows[0];
            assert!(!row.enabled, "就地翻位：不等后台往返才有即时反馈");
        });

        // 写入失败 → 一次性提示，列表照旧可见
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| view.set_notice("启停未保存：库被占用", cx))
        });
        rules.update(cx, |view, _| {
            assert!(matches!(view.state(), super::RulesDialogState::Ready));
            assert!(view.notice().is_some_and(|n| n.contains("未保存")));
            assert_eq!(view.visible().visible_rows(), 2);
        });
        draw(cx);

        // 下一次成功刷新会清掉提示
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_data(sample_data(), cx)));
        rules.update(cx, |view, _| assert!(view.notice().is_none()));
    }

    /// 新建与打开文件：都只是事件（建目录、拉起进程由接缝做）
    #[gpui_kit::test]
    fn create_and_open_are_requests_not_side_effects(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (rules, sink, cx) = open_dialog_harness(cx);
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| {
                view.request_create_rule(RuleScope::Project, cx);
                view.request_open_file(
                    PathBuf::from("/p/.RSmeta/insight-rules/my-rule.rule.toml"),
                    cx,
                );
            });
        });
        assert_eq!(
            sink.read_with(cx, |sink, _| sink.events.borrow().clone()),
            vec![
                RulesEvent::CreateRuleRequested {
                    scope: RuleScope::Project
                },
                RulesEvent::OpenFileRequested {
                    path: PathBuf::from("/p/.RSmeta/insight-rules/my-rule.rule.toml")
                },
            ]
        );
    }
}