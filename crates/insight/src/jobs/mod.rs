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
use crate::model::{ColumnProfileView, InsightTarget, TableProfileView};
use crate::rule::RuleScope;
use crate::rule_view::{RulesEvent, RulesView};
use crate::service::InsightService;
use crate::service::rule_trust::RuleTrust;


// ==================== 子模块（拆分：按请求职责域下沉，纯位移） ====================
mod profile;
mod rules;
mod table;
mod history;

// 聚合 re-export：保持 `crate::jobs::*` 路径与外部调用面（workbench `jobs::attach`）不变
pub use profile::*;
pub use rules::*;
pub use table::*;
pub use history::*;

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
        InsightEvent::FullProfileRequested { target } => {
            // 全量口径：源目标重新取样为**不带 LIMIT** 的分析临时表
            let Some(request) = ProfileRequest::of_full(target) else {
                return;
            };
            request_profile(view, project_root, request, cx);
        }
        // 全量确认请求由宿主侧（`components::insight_actions`）弹窗后转
        // `confirm_full` → `FullProfileRequested`；本取数链不直接处理。
        InsightEvent::FullProfileConfirmRequested { .. } => {}
        InsightEvent::TableEvaluateRequested {
            temp_table,
            table_name,
        } => request_table_evaluation(
            view,
            project_root,
            temp_table.clone(),
            table_name.clone(),
            cx,
        ),
        InsightEvent::MultiColumnRequested {
            temp_table,
            table_name,
        } => request_multi_view(
            view,
            project_root,
            temp_table.clone(),
            table_name.clone(),
            cx,
        ),
        InsightEvent::MultiRunRequested {
            temp_table,
            rule_id,
            columns,
        } => request_multi_run(
            view,
            project_root,
            temp_table.clone(),
            rule_id.clone(),
            columns.clone(),
            cx,
        ),
        InsightEvent::SchemaReportRequested {
            conn_id,
            database,
            schema,
        } => request_schema_report(view, conn_id.clone(), database.clone(), schema.clone(), cx),
        InsightEvent::HistoryRequested { column } => {
            request_history(view, project_root, column.clone(), cx)
        }
        InsightEvent::VersionCompareRequested { column, version_id } => {
            request_version_compare(view, project_root, column.clone(), version_id.clone(), cx)
        }
        InsightEvent::SnapshotCleanupRequested { column } => {
            request_cleanup(view, project_root, column.clone(), cx)
        }
        InsightEvent::SnapshotSaveRequested {
            temp_table,
            column,
            source_label,
        } => request_snapshot_save(
            view,
            project_root,
            temp_table.clone(),
            column.clone(),
            source_label.clone(),
            cx,
        ),
        // 下钻要先把源表登记成临时表，那是**宿主的活**（它才知道连接与临时表约定）：
        // 这里只把请求转给宿主提供的回调，没接就只记一条日志（不是静默失败）
        InsightEvent::TableDrilldownRequested { table, .. } => {
            tracing::info!("Schema 报告下钻请求（宿主未接）: {table}");
        }
        // 导出同理：选路径与写文件都要窗口与系统对话框（工作台在
        // `components::insight_actions` 订阅同一份事件落地）。
        InsightEvent::SchemaExportRequested { format, .. } => {
            tracing::info!("Schema 报告导出请求（宿主未接）: {}", format.label());
        }
        // 表画像 Markdown 报告导出：宿主（`components::insight_actions`）订阅
        // 同一份事件选路径写盘；取数链不处理（面板已算好 content，D36）。
        InsightEvent::TableReportExportRequested { target, .. } => {
            tracing::info!("表画像报告导出请求（宿主未接）: {}", target.title());
        }
        // 右栏集合「对比」由工作台侧（`components::insight_actions`）转中间页；
        // 中间页取数链（本文件）不处理它——没有宿主订阅时静默（按钮本就不该出现）
        InsightEvent::CompareRequested { .. } => {}
        // 关系排查：取数在洞察侧（DuckDB 全局连接），不依赖宿主窗口
        InsightEvent::RelationRequested { key } => {
            request_overlap(view, key.clone(), cx);
        }
        // 归档要宿主侧（`ArchiveService` + 项目目录），由 `components::insight_actions` 落地
        InsightEvent::ArchiveRequested { .. } => {}
    }
}

/// 跨洞察表列值重叠排查（T19）：发起方（集合条目）⟷ 当前目标所在临时表。
///
/// 双方都必须是**已取样**的临时表（`temp_table` / `PanelData::sample_table`）；
/// 没就绪给行内说明，不发起无谓的 SQL（拿 label 猜表名必然查错）。
fn request_overlap(view: &Entity<InsightView>, key: String, cx: &mut App) {
    let (table_a, table_b) = {
        let snapshot = view.read(cx);
        let entry = snapshot
            .insight_set()
            .iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.temp_table.clone());
        let current = snapshot
            .target()
            .and_then(|target| snapshot.data().sample_table(target))
            .map(str::to_string);
        (entry, current)
    };
    let (Some(table_a), Some(table_b)) = (table_a, table_b) else {
        view.update(cx, |view, cx| {
            view.set_overlap_notice("当前目标或该表尚未完成取样，无法做关系排查", cx);
        });
        return;
    };
    view.update(cx, |view, cx| view.begin_overlap(cx));
    let weak = view.downgrade();
    cx.spawn(async move |cx| {
        let result = InsightService::overlap_report(&table_a, &table_b);
        let _ = weak.update(cx, |view, cx| match result {
            Ok(overlap) => view.set_overlap(overlap, cx),
            Err(error) => {
                view.set_overlap_notice(InsightService::describe_error(&error).message, cx)
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
    use crate::model::{InsightPanelState, InsightTarget, PanelTab, TableEvalProgress};
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
            Some(ProfileRequest::Column {
                temp_table: "t_result_1".into(),
                column: "amount".into(),
            })
        );
    }

    #[test]
    fn table_target_maps_to_a_table_request() {
        let target = InsightTarget::Table {
            temp_table: "t_result_1".into(),
            table_name: "orders".into(),
        };
        assert_eq!(
            ProfileRequest::of(&target),
            Some(ProfileRequest::Table {
                temp_table: "t_result_1".into(),
                table_name: "orders".into(),
            })
        );
    }

    /// 源目标也要能解析出请求（否则入口给了目标却没人取数）。
    #[test]
    fn source_targets_map_to_sampling_requests() {
        let source =
            crate::model::SampleSource::new("G_1", "SELECT * FROM `shop`.`orders`", "shop.orders");

        let col = InsightTarget::SourceColumn {
            source: source.clone(),
            column: "amount".into(),
            data_type: "DECIMAL".into(),
        };
        assert_eq!(
            ProfileRequest::of(&col),
            Some(ProfileRequest::SourceColumn {
                source: source.clone(),
                column: "amount".into(),
                full: false,
            }),
            "源列目标要走「取样 + 画像」那条路"
        );

        let table = InsightTarget::SourceTable {
            source: source.clone(),
            table_name: "orders".into(),
        };
        assert_eq!(
            ProfileRequest::of(&table),
            Some(ProfileRequest::SourceTable {
                source,
                table_name: "orders".into(),
                full: false,
            })
        );
    }

    #[test]
    fn later_phase_targets_are_not_pretended() {
        // 多列与结构属 Phase 3/4：没有生产者就不该被当成有行为
        for target in [
            InsightTarget::MultiColumn {
                temp_table: "t".into(),
                columns: vec!["a".into()],
            },
            InsightTarget::Schema {
                conn_id: "G_1".into(),
                database: "shop".into(),
                schema: None,
            },
        ] {
            assert_eq!(ProfileRequest::of(&target), None, "{target:?}");
        }
    }

    // ==================== 表探查与评估全表（Phase 3.1 / 2.2） ====================

    /// 建探针临时表（DuckDB 是进程单例，表名带用例后缀以避免串台）
    fn seed_probe_table(table: &str, columns: &str, values: &str) {
        let conn = crate::insight_engine::get_or_create_duckdb().expect("内存 DuckDB");
        let conn = conn.lock().expect("DuckDB 锁不应中毒");
        conn.execute_batch(&format!(
            "CREATE OR REPLACE TEMP TABLE \"{table}\" ({columns})"
        ))
        .expect("建探针表");
        conn.execute_batch(&format!("INSERT INTO \"{table}\" {values}"))
            .expect("插数据");
    }

    /// 表目标 → 后台内省 → 面板拿到列清单（走真实接线）
    #[gpui_kit::test]
    fn attach_turns_a_table_request_into_a_profile(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        seed_probe_table(
            "t_insight_jobs_table",
            "id INTEGER, amount DECIMAL(12,2), note VARCHAR",
            "VALUES (1, 1.5, 'a'), (2, NULL, NULL), (3, 3.5, 'c')",
        );

        let (host, panel, _sub) = cx.update(|cx| {
            let host = cx.new(TestHost::new);
            let panel = cx.new(InsightView::new);
            let sub = host.update(cx, |_host, host_cx| attach(&panel, host_cx, || None));
            (host, panel, sub)
        });
        let _host = host;

        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                panel.set_target(
                    InsightTarget::Table {
                        temp_table: "t_insight_jobs_table".into(),
                        table_name: "orders".into(),
                    },
                    cx,
                );
            });
        });
        cx.run_until_parked();

        cx.update(|cx| {
            let table = panel
                .read(cx)
                .data()
                .table
                .as_ref()
                .expect("表目标应回填表探查，而不是列画像");
            assert_eq!(table.table_name, "orders");
            assert_eq!(table.row_count, 3);
            assert_eq!(table.columns.len(), 3);
            assert_eq!(table.columns[0].kind, crate::model::ColumnKind::Numeric);
            // 探查阶段只回填缺失率 / 唯一率摘要（一次聚合查询）；列级明细（直方图 /
            // 样本 / 分布）等用户点「评估全表」才逐列串行跑。
            assert!(
                table.columns.iter().all(|c| {
                    c.stats_detail.is_none() && c.histogram.is_none() && c.sample_values.is_empty()
                }),
                "只是探查，不跑列级明细：直方图 / 样本 / 分布得等用户点「评估全表」"
            );
        });
    }

    /// 「评估全表」：逐列串行 + 真实进度，最后回填列明细与检查器事实。
    ///
    /// 进度不是假的：观察者按 notify 记录每一帧的 `progress`，必须看到**中间态**。
    #[gpui_kit::test]
    fn table_evaluation_reports_progress_and_final_facts(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        seed_probe_table(
            "t_insight_jobs_eval",
            "id INTEGER, amount DECIMAL(12,2), note VARCHAR",
            "VALUES (1, 1.5, 'a'), (2, 2.5, 'b'), (3, NULL, NULL), (4, 4.5, 'd')",
        );
        let traces: Arc<Mutex<Vec<Option<TableEvalProgress>>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = traces.clone();

        let (host, panel, _sub, _obs) = cx.update(|cx| {
            let host = cx.new(TestHost::new);
            let panel = cx.new(InsightView::new);
            let sub = host.update(cx, |_host, host_cx| attach(&panel, host_cx, || None));
            // 观察者记录进度序列（面板每次回填都会 notify）
            let seen = seen.clone();
            let panel_for_obs = panel.clone();
            // 订阅必须被**外层**持有：在闭包里 `let _obs = …` 会随闭包一起析构，
            // 评估还没开始观察就没了（本仓踩过的同一个坑）
            let obs = host.update(cx, |_host, host_cx| {
                host_cx.observe(&panel_for_obs, move |_this, panel, cx| {
                    let progress = panel
                        .read(cx)
                        .data()
                        .table
                        .as_ref()
                        .and_then(|t| t.progress);
                    seen.lock().unwrap().push(progress);
                })
            });
            (host, panel, sub, obs)
        });
        let _host = host;

        // 先探查（拿列清单），再发起评估：与用户点两次按钮同一条路径
        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                panel.set_target(
                    InsightTarget::Table {
                        temp_table: "t_insight_jobs_eval".into(),
                        table_name: "orders".into(),
                    },
                    cx,
                );
            });
        });
        cx.run_until_parked();
        traces.lock().unwrap().clear();

        cx.update(|cx| panel.update(cx, |panel, cx| panel.request_table_evaluation(cx)));
        cx.run_until_parked();

        cx.update(|cx| {
            let table = panel
                .read(cx)
                .data()
                .table
                .as_ref()
                .expect("评估后仍是表探查数据")
                .clone();
            assert!(table.progress.is_none(), "跑完进度行必须消失");
            let quality = table.quality.expect("应有表级事实摘要位");
            assert!(
                quality.checker.len() > 0,
                "评估后检查器事实应已聚合：{:?}",
                quality.checker
            );
            assert!(quality.checker_hits <= quality.checker_applicable);
            assert!(
                table.columns.iter().all(|c| c.stats_detail.is_some()),
                "每列都应回填事实明细：{:?}",
                table
                    .columns
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>()
            );
        });

        let traces = traces.lock().unwrap().clone();
        assert!(
            traces
                .iter()
                .any(|p| matches!(p, Some(p) if p.done > 0 && p.done < p.total)),
            "必须能观察到中间进度（不是只有开始与结束）：{traces:?}"
        );
        assert_eq!(
            traces.last(),
            Some(&None),
            "最后一帧应是「无进度」的完成态：{traces:?}"
        );
    }

    /// 多列分析：切到「多列」Tab → 表单取数 → 跑规则 → 结果回填（全程真实接线）
    #[gpui_kit::test]
    fn multi_column_flow_loads_the_form_and_runs_a_rule(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let table = "t_insight_jobs_multi";
        let rows: Vec<String> = (1..=8).map(|i| format!("({i}, {})", i * 2)).collect();
        seed_probe_table(
            table,
            "x INTEGER, y INTEGER",
            &format!("VALUES {}", rows.join(",")),
        );

        let (host, panel, _sub) = cx.update(|cx| {
            let host = cx.new(TestHost::new);
            let panel = cx.new(InsightView::new);
            let sub = host.update(cx, |_host, host_cx| attach(&panel, host_cx, || None));
            (host, panel, sub)
        });
        let _host = host;

        // 1) 目标 + 切到「多列」：这一下会发 MultiColumnRequested（切 Tab 即取数）
        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                panel.set_target(
                    InsightTarget::Table {
                        temp_table: table.into(),
                        table_name: "pairs".into(),
                    },
                    cx,
                );
                panel.set_tab(crate::model::PanelTab::MultiColumn, cx);
            });
        });
        cx.run_until_parked();

        cx.update(|cx| {
            let view = panel
                .read(cx)
                .data()
                .multi
                .as_ref()
                .expect("多列表单应已回填");
            assert_eq!(view.table_name, "pairs");
            let names: Vec<&str> = view.columns.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(names, vec!["x", "y"]);
            assert!(
                view.rules.iter().any(|r| r.id == "pearson-correlation"),
                "内置多列规则应在候选里：{:?}",
                view.rules.iter().map(|r| r.id.clone()).collect::<Vec<_>>()
            );
        });

        // 2) 选两列 + 一条规则 → 执行 → 结果回填
        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                panel.toggle_multi_column("x", cx);
                panel.toggle_multi_column("y", cx);
                panel.set_multi_rule("pearson-correlation", cx);
                panel.run_multi(cx);
            });
        });
        cx.run_until_parked();

        cx.update(|cx| {
            let view = panel.read(cx).data().multi.as_ref().unwrap().clone();
            assert!(!panel.read(cx).multi_running(), "跑完要解除「分析中」");
            let Some(crate::model::MultiResultView::Single(rows)) = view.result else {
                panic!("Pearson 应给出单值结果：{:?}", view.result);
            };
            let corr = rows
                .iter()
                .find(|r| r.label == "相关系数")
                .map(|r| r.value.clone())
                .expect("应有 correlation 字段（展示为「相关系数」）");
            assert_eq!(corr, "1", "y = 2x 是完全线性相关：{rows:?}");
            assert!(view.notes.is_empty());
        });
    }

    /// 历史：面板「保存」→ 接缝开项目库 → 双写 → 回填历史；失败只挂行内提示。
    ///
    /// 这条用例是「保存入口可达」的兜底：v1 的保存函数没有任何调用方（前端根本点不到），
    /// 所以这里钉住「点得到，且点了真落库、真的长一版」。
    #[gpui_kit::test]
    fn attach_saves_a_snapshot_and_reads_the_history_back(cx: &mut TestAppContext) {
        // 取数会写进程级规则集缓存（按项目根键控）：与同类的缓存测试串行
        let _guard = crate::tests::rule_state_guard();
        cx.update(gpui_kit::init);
        let table = "t_insight_jobs_history";
        seed_probe_table(table, "amount DECIMAL(12,2)", "VALUES (1.5), (2.5), (NULL)");
        let root = temp_project("history");
        let root_for_host = root.clone();

        let (host, panel, _sub) = cx.update(|cx| {
            let host = cx.new(TestHost::new);
            let panel = cx.new(InsightView::new);
            let sub = host.update(cx, |_host, host_cx| {
                attach(&panel, host_cx, move || Some(root_for_host.clone()))
            });
            (host, panel, sub)
        });
        let _host = host;

        // 宿主装配时会做这两件事：告知项目已打开 + 递一个列目标进来
        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                panel.set_project_open(true, cx);
                panel.set_target(
                    InsightTarget::Column {
                        temp_table: table.into(),
                        column: "amount".into(),
                        data_type: "DECIMAL(12,2)".into(),
                    },
                    cx,
                );
            });
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(panel.read(cx).data().column.is_some(), "列画像应已回填");
        });

        // 切到「历史」：库里还没有快照 → 空列表（而**不是**错误态、也不是骨架）
        cx.update(|cx| panel.update(cx, |panel, cx| panel.set_tab(PanelTab::History, cx)));
        cx.run_until_parked();
        cx.update(|cx| {
            let panel = panel.read(cx);
            let history = panel.data().as_history().expect("切过去应回填历史载荷");
            assert!(history.is_empty(), "首版之前是空列表");
            assert_eq!(panel.state(), &InsightPanelState::Data);
            assert!(!panel.history_saving());
        });

        // 保存：接缝重取领域画像 + 双写，回填后的列表多一版
        cx.update(|cx| panel.update(cx, |panel, cx| panel.request_snapshot_save(cx)));
        assert!(
            panel.read_with(cx, |panel, _| panel.history_saving()),
            "点了保存就该立刻进入「保存中」（按钮置灰的依据）"
        );
        cx.run_until_parked();
        cx.update(|cx| {
            let panel = panel.read(cx);
            let history = panel.data().as_history().expect("保存后应回填历史");
            assert_eq!(history.entries.len(), 1, "保存应留下首版");
            assert!(history.entries[0].is_latest);
            assert!(!history.entries[0].has_parent, "首版无父版本");
            assert!(history.stats_line().is_some(), "存储用量应是真实数字");
            assert!(!panel.history_saving(), "出数后「保存中」要落回");
            assert_eq!(panel.state(), &InsightPanelState::Data);
        });

        // 再存一版：版本链串起来（新版的父版本指向上一版）。
        // 两版之间真的改一次数据，这样下面的对比才有东西可比
        {
            let conn = crate::insight_engine::get_or_create_duckdb().expect("内存 DuckDB");
            let conn = conn.lock().expect("DuckDB 锁不应中毒");
            conn.execute_batch(&format!("INSERT INTO \"{table}\" VALUES (9.5)"))
                .expect("插一行（模拟数据变了）");
        }
        cx.update(|cx| panel.update(cx, |panel, cx| panel.request_snapshot_save(cx)));
        cx.run_until_parked();
        let baseline_version = cx.update(|cx| {
            let history = panel.read(cx).data().as_history().expect("历史").clone();
            assert_eq!(history.entries.len(), 2);
            assert!(history.entries[0].is_latest && history.entries[0].has_parent);
            assert!(!history.entries[1].is_latest && !history.entries[1].has_parent);
            assert_ne!(history.entries[0].version_id, history.entries[1].version_id);
            history.entries[1].version_id.clone()
        });

        // 对比：方向固定为「选中 → 最新」，差值从**存下来的两份正文**算
        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                panel.toggle_compare_version(&baseline_version, cx)
            })
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let panel = panel.read(cx);
            assert_eq!(panel.compare_target(), Some(baseline_version.as_str()));
            let diff = panel
                .data()
                .as_history()
                .and_then(|history| history.diff.clone())
                .expect("对比结果应已回填");
            assert_eq!(diff.baseline_version, baseline_version);
            let total = diff
                .rows
                .iter()
                .find(|row| row.label == "总行数")
                .expect("应变到总行数");
            assert_eq!(
                (total.old.as_str(), total.new.as_str()),
                ("3", "4"),
                "两版之间插了一行，对比要看得见"
            );
            assert!(total.delta.is_changed());
        });

        // 结果集过期（临时表没了）：保存失败**只挂行内提示**——已有的历史必须还在，
        // 整页转错误态会让人以为快照丢了
        {
            let conn = crate::insight_engine::get_or_create_duckdb().expect("内存 DuckDB");
            let conn = conn.lock().expect("DuckDB 锁不应中毒");
            conn.execute_batch(&format!("DROP TABLE \"{table}\""))
                .expect("丢掉临时表（模拟结果集过期）");
        }
        cx.update(|cx| panel.update(cx, |panel, cx| panel.request_snapshot_save(cx)));
        cx.run_until_parked();
        cx.update(|cx| {
            let panel = panel.read(cx);
            assert!(!panel.history_saving(), "失败也要解除「保存中」");
            assert_eq!(
                panel.state(),
                &InsightPanelState::Data,
                "保存失败不抢整页错误态"
            );
            let history = panel.data().as_history().expect("历史不得被失败清掉");
            assert_eq!(history.entries.len(), 2, "失败后旧版本照旧可见");
        });

        // 清理：30 天档对刚存的两版是空操作，但回执要一路回到面板（事件 → 接缝 → 载荷）
        cx.update(|cx| panel.update(cx, |panel, cx| panel.request_cleanup(cx)));
        assert!(
            panel.read_with(cx, |panel, _| panel.history_cleaning()),
            "确认后立即进入「清理中」"
        );
        cx.run_until_parked();
        cx.update(|cx| {
            let panel = panel.read(cx);
            assert!(!panel.history_cleaning(), "出数后「清理中」要落回");
            let history = panel.data().as_history().expect("历史");
            let receipt = history.cleanup.as_ref().expect("回执应随载荷回来");
            assert_eq!(
                (receipt.body_removed, receipt.meta_removed),
                (0, 0),
                "刚存的快照不该被 30 天档删掉"
            );
            assert_eq!(history.entries.len(), 2, "空操作不该动列表");
        });

        let _ = std::fs::remove_dir_all(&root);
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
            assert_eq!(
                *calls.lock().unwrap(),
                1,
                "订阅应被触发（项目根提供者被问了一次）"
            );
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
        // 信任门（Q1 ③）：项目规则默认不装配；本用例测的是「宿主接缝 + 启停真的落到规则集」，
        // 所以先把项目设为已信任（信任门本身另有专测）
        crate::apply_project_rule_trust(&root, crate::RuleTrust::Trusted);

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
                view.data().total > crate::rule_types::BUILTIN_RULE_COUNT,
                "内置 {} 条 + 项目规则，实际 {}",
                crate::rule_types::BUILTIN_RULE_COUNT,
                view.data().total
            );
        });

        // 开关：写索引 → 重新同步 → 推进启停集合 → 注册表失效（下次取数就不再包含它）
        assert!(
            crate::with_rules(Some(&root), |registry| Ok(registry
                .get("demo-rule")
                .is_some()))
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