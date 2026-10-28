//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use std::rc::Rc;

use gpui_kit::{
    Entity, Focusable as _, TestAppContext, VisualTestContext,
};

use crate::channel::{ChannelAvailability, ChannelAvailabilitySet, ChannelsPort, ExecChannel};
use crate::completion::{Candidate, CandidateKind, Catalog};
use crate::connection::SchemaInfo;
use crate::execution::{self, QueryData, QueryRunner};
use crate::export::ExportFormat;
use crate::model::EditorMode;
use crate::project::{ProjectPort, ProjectState};
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::sources::{SourceRow, SourceState, SourcesPort, SourcesSnapshot};
use crate::view::host::EditorHostPanel;

use super::{
    bind_editor_keys, open_panel, run_statement, shared_with_channel_recorder,
    shared_with_document, shared_with_runner, shared_with_toolbar_runner,
};
use super::dialogs::dialog_button_rendered;
use super::results_b::{attach_export_picker, export_temp_path};

use super::analysis::{FakeSchemas, schema_doc};
use super::dialogs::{FakeConnections, option};

// ══════════════════════════════════════════════════════════════════════

/// 假通道端口：可用性由用例说了算（真门控在宿主：`services/editor_channels.rs`）
struct FakeChannels {
    accelerated: ChannelAvailability,
    federated: ChannelAvailability,
}

impl FakeChannels {
    /// 两档都通（用来验“选得到、切得动”的那条路）
    fn open() -> Self {
        Self {
            accelerated: ChannelAvailability::ok(),
            federated: ChannelAvailability::ok(),
        }
    }

    /// 两档都不通（各自的理由由调用方给）
    fn blocked(accelerated: &str, federated: &str) -> Self {
        Self {
            accelerated: ChannelAvailability::blocked(accelerated),
            federated: ChannelAvailability::blocked(federated),
        }
    }
}

impl ChannelsPort for FakeChannels {
    fn availability(&self, _conn_id: Option<&str>) -> ChannelAvailabilitySet {
        ChannelAvailabilitySet {
            accelerated: self.accelerated.clone(),
            federated: self.federated.clone(),
        }
    }
}

/// 菜单项 -> (文案, 能不能点, 是不是当前档)
fn channel_menu_rows(
    panel: &Entity<EditorHostPanel>,
    cx: &mut VisualTestContext,
) -> Vec<(String, bool, bool)> {
    cx.update(|_window, cx| {
        panel
            .read(cx)
            .channel_menu()
            .into_iter()
            .map(|item| (item.label, item.available, item.current))
            .collect()
    })
}

/// 【T1.6】源清单：「源清单 ▾」只在联邦档出现，内容来自宿主端口（快照 → 行/动作）
#[gpui_kit::test]
fn the_sources_picker_shows_up_in_the_federated_channel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.attach_channels(Rc::new(FakeChannels::open()));
    shared.attach_sources(Rc::new(FakeSources));
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 绑定连接（源清单属于“这个连接上的联邦会话”）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_orders".to_string()), cx)
        })
    });

    // 「源清单 ▾」只在联邦档出现（源库档没有挂载这回事，不摆按了没用的入口）
    let picker_present = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds("editor-sources").is_some()
    };
    assert!(!picker_present(cx), "源库档不该有「源清单 ▾」");

    // 切到联邦：清单能读出来（两行 + 主源 + 回退说明）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_channel(ExecChannel::Federated, cx)
        });
    });
    let snapshot = cx
        .update(|_window, cx| panel.read(cx).sources_snapshot())
        .expect("联邦档该有清单");
    assert_eq!(snapshot.sources.len(), 2, "{snapshot:?}");
    assert_eq!(snapshot.primary.as_deref(), Some("mysql_src"));
    assert!(snapshot.note.is_some(), "回退说明要带过来");
    assert!(picker_present(cx), "联邦档该有「源清单 ▾」");

    // 动作：换主源要留痕（旁路线程 + 回执；回执由泵取回，这里先看“正在…”那句）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.run_source_action(
                execution::SourceAction::SetPrimary {
                    alias: "pg_warehouse".to_string(),
                },
                cx,
            );
        });
    });
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("动作要留痕");
    assert!(message.contains("pg_warehouse"), "{message}");
}

/// 假源清单端口：一份固定的快照（不改也不动）
struct FakeSources;

impl SourcesPort for FakeSources {
    fn snapshot(&self, _conn_id: &str) -> Option<SourcesSnapshot> {
        Some(SourcesSnapshot {
            sources: vec![
                SourceRow {
                    alias: "mysql_src".to_string(),
                    kind_label: "MySQL".to_string(),
                    state: SourceState::Ready { tables: 42 },
                    primary: true,
                },
                SourceRow {
                    alias: "pg_warehouse".to_string(),
                    kind_label: "PostgreSQL".to_string(),
                    state: SourceState::Ready { tables: 7 },
                    primary: false,
                },
            ],
            primary: Some("mysql_src".to_string()),
            note: Some("主源 oracle_prod 不可用，已改用 mysql_src".to_string()),
        })
    }
}

/// 菜单门控：不可用项**保留形态 + 行尾给原因**，当前项打勾（原型 §2.2 / §5.7）
#[gpui_kit::test]
fn the_channel_menu_greys_out_with_reasons(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.attach_channels(Rc::new(FakeChannels::blocked(
        "该连接未开启本地加速",
        "尚未注册外部源",
    )));
    let (panel, cx) = open_panel(cx, &shared, &id);

    let rows = channel_menu_rows(&panel, cx);
    assert_eq!(rows.len(), 3, "三档都在菜单里（互斥靠选项，不是靠藏）");
    assert_eq!(rows[0].0, "源库");
    assert!(rows[0].1 && rows[0].2, "源库是当前档且可用");
    assert_eq!(rows[1].0, "本地加速（该连接未开启本地加速）");
    assert!(!rows[1].1, "不可用要置灰");
    assert_eq!(rows[2].0, "联邦（尚未注册外部源）");
    assert!(!rows[2].1);
}

/// 未绑定连接 → 源库档仍可用（执行器会回退到当前活动连接）；绑了没连上的连接 → 置灰给原因
#[gpui_kit::test]
fn the_source_channel_follows_the_connection_state(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    // 未绑定：不清不白地说“不可用”是错的——1a 口径确实是跟随当前活动连接
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);
    let rows = channel_menu_rows(&panel, cx);
    assert!(rows[0].1, "未绑定不算源库不可用：{}", rows[0].0);

    // 绑了一个没有运行态的连接：源库档置灰 + 原因
    let (shared2, id2, _seen2, _seen_conn2) = shared_with_runner("select 1;", EditorMode::Sql);
    shared2.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_down", "P", "down",
    )])));
    shared2.update(|service| service.set_connection(&id2, Some("P_down".to_string())));
    let (panel2, cx) = open_panel(cx, &shared2, &id2);
    let rows = channel_menu_rows(&panel2, cx);
    assert_eq!(rows[0].0, "源库（连接未建立）");
    assert!(!rows[0].1, "连接没起来就不该说源库档能用");
}

/// 没接通道端口：后两档都不可用，原因如实说“尚未接入”（而不是看起来能选）
#[gpui_kit::test]
fn without_a_channel_port_the_snapshot_channels_are_unavailable(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    let rows = channel_menu_rows(&panel, cx);
    assert!(!rows[1].1 && !rows[2].1, "没接端口就不给选");
    assert!(rows[1].0.contains("尚未接入通道能力"), "{}", rows[1].0);
}

/// 【B13】切到加速档：后续结果记住通道（徽标），切回源库后旧结果标灰 + 顶部一行提示
#[gpui_kit::test]
fn switching_channels_stamps_results_and_greys_the_old_ones(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.attach_channels(Rc::new(FakeChannels::open()));
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 源库档跑一份
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert_eq!(
        shared.results().sets(&id)[0].channel,
        ExecChannel::Source,
        "源库档跑的结果要记住自己是源库档"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_badges_for_test(cx)),
        ["源库"]
    );

    // 切到本地加速：文档属性变了（这是“在哪儿跑”的真值）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_channel(ExecChannel::Accelerated, cx)
        });
    });
    assert_eq!(shared.service().channel_for(&id), ExecChannel::Accelerated);
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("切换要留痕");
    assert!(message.contains("执行位置已切到本地加速"), "{message}");
    assert!(
        message.contains("只读"),
        "非源库档要说清能力边界（源库只读）：{message}"
    );
    // 切完那一刻旧结果就已经是“旧”的了（不用等下一次执行）
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_badges_for_test(cx)),
        ["源库·旧"]
    );
    let notice = cx
        .update(|_window, cx| panel.read(cx).result_notice_for_test(cx))
        .expect("要有顶部提示");
    assert!(notice.contains("旧结果来自源库"), "{notice}");

    // 加速档再跑一份：它带的是加速徽标（不是源库）
    run_statement(cx, &panel, "select 2", execution::ResultPlacement::NewSet);
    let sets = shared.results().sets(&id).to_vec();
    assert_eq!(sets.len(), 2);
    assert_eq!(sets[1].channel, ExecChannel::Accelerated);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_badges_for_test(cx)),
        ["源库·旧", "加速"]
    );

    // 切回源库：角色对调（“旧”跟着当前通道走，不写死在某一档上）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_channel(ExecChannel::Source, cx));
    });
    assert_eq!(shared.service().channel_for(&id), ExecChannel::Source);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_badges_for_test(cx)),
        ["源库", "加速·旧"]
    );
}

/// 【B13】文档的“执行位置”要**真的**传到执行器（与连接绑定同一口径：不分流就是装饰）
#[gpui_kit::test]
fn the_document_channel_reaches_the_execution_port(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, channels) = shared_with_channel_recorder("select 1;");
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.attach_channels(Rc::new(FakeChannels::open()));
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 默认是源库档
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert_eq!(
        channels.lock().expect("锁").as_slice(),
        [ExecChannel::Source],
        "默认档要如实传到执行器"
    );

    // 切到本地加速之后再执行：同一个文档、不同通道
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_channel(ExecChannel::Accelerated, cx)
        });
    });
    run_statement(cx, &panel, "select 2", execution::ResultPlacement::Replace);
    assert_eq!(
        channels.lock().expect("锁").as_slice(),
        [ExecChannel::Source, ExecChannel::Accelerated],
        "切档后执行器收到的通道要跟着变"
    );
}

/// 【B13】「重新挂载源库」：只在本地跑的两档出现，回执要让用户看得见
#[gpui_kit::test]
fn refreshing_the_source_reports_back_from_the_side_thread(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    /// 假执行器：只关心“被要求重新挂载几次”
    struct RefreshRunner {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl QueryRunner for RefreshRunner {
        fn run(
            &self,
            _connection: Option<&str>,
            _schema: Option<&str>,
            _channel: ExecChannel,
            _sql: &str,
            _options: execution::RunOptions,
        ) -> Result<QueryData, String> {
            Ok(QueryData::default())
        }

        fn refresh_sources(
            &self,
            _connection: Option<&str>,
            _channel: ExecChannel,
            _alias: Option<&str>,
        ) -> Result<String, String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok("已重新挂载源库（新表可见了）".to_string())
        }
    }

    let shared = EditorShared::new();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    shared.attach_runner(std::sync::Arc::new(RefreshRunner {
        calls: calls.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled("select 1;", EditorMode::Sql))
        .id()
        .clone();
    shared.attach_channels(Rc::new(FakeChannels::open()));
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 源库档：没有“挂载”这回事，直说而不是默默不发
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.refresh_source(cx)));
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留原因");
    assert!(message.contains("不是本地加速档"), "{message}");
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "源库档不该去打扰执行器"
    );

    // 切到加速档：同一个动作现在真的有东西可挂
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_channel(ExecChannel::Accelerated, cx)
        });
    });
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.refresh_source(cx)));
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).refresh_pending_for_test()),
        1,
        "请出去就要记一笔（否则轮询泵不知道要等回执）"
    );
    // 回执在旁路线程上产生：轮询泵（生产中就是它）取回来
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.drain_exec_results(cx)));
        let done = cx.update(|_window, cx| {
            panel.read(cx).refresh_pending_for_test() == 0
                && panel
                    .read(cx)
                    .message
                    .as_deref()
                    .is_some_and(|text| text.contains("已重新挂载源库"))
        });
        if done {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "重新挂载的回执迟迟没回来"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    // 菜单项本身也只在本地跑的那两档存在（纯函数）
    assert!(crate::channel::refresh_item_label(ExecChannel::Source).is_none());
    assert!(crate::channel::refresh_item_label(ExecChannel::Accelerated).is_some());
}

/// 【B13】加速档拒绝**作用源库的写语句**，原因可读；读语句照跑
#[gpui_kit::test]
fn the_snapshot_channel_refuses_source_writes_with_a_reason(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _seen_conn) = shared_with_runner("update t set a = 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.attach_channels(Rc::new(FakeChannels::open()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_channel(ExecChannel::Accelerated, cx)
        });
    });

    let submitted = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement("update t set a = 1".to_string()),
                execution::ResultPlacement::Replace,
                cx,
            )
        })
    });
    assert!(!submitted, "写语句不该被提交");
    assert!(
        seen.lock().expect("锁").is_empty(),
        "被拒的语句不许打扰执行器（更不能跑一半）"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留原因");
    assert!(message.contains("不能写源库"), "{message}");
    assert!(message.contains("请切回源库"), "{message}");
    // 文案只说“外部源只读”，不说机制（L2 源没有 `READ_ONLY`，说得太具体就成了假话）
    assert!(message.contains("外部源只读"), "{message}");

    // 读语句照跑（加速档就是用来跑分析的）
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert_eq!(seen.lock().expect("锁").len(), 1);
}

/// 项目只读端口（测试用：固定值——锁是宿主的真值，端口只负责递过来）
pub(crate) struct FakeProject {
    pub(crate) read_only: bool,
}

impl ProjectPort for FakeProject {
    fn state(&self) -> ProjectState {
        ProjectState {
            read_only: self.read_only,
        }
    }
}

/// 项目为只读模式：写语句在提交前就被拒（不打扰执行器），读语句照跑；状态栏说「连接只读」
#[gpui_kit::test]
fn a_read_only_project_refuses_writes_but_not_reads(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _seen_conn) = shared_with_runner("update t set a = 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.attach_channels(Rc::new(FakeChannels::open()));
    // 项目锁（标题栏那个）：写源库对象的语句一律拒
    shared.attach_project(Rc::new(FakeProject { read_only: true }));
    let (panel, cx) = open_panel(cx, &shared, &id);

    let submitted = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement("update t set a = 1".to_string()),
                execution::ResultPlacement::Replace,
                cx,
            )
        })
    });
    assert!(!submitted, "项目只读时写语句不该被提交");
    assert!(
        seen.lock().expect("锁").is_empty(),
        "被拒的语句不许打扰执行器（更不能跑一半）"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留原因");
    assert!(message.contains("项目为只读模式"), "{message}");

    // 状态栏：项目锁落在「连接只读」那一维；「编辑器只读」那一维不受影响（两维互不蕴含）
    let flags = cx.update(|_window, cx| panel.read(cx).read_only_flags());
    assert!(flags.connection, "项目锁 = 连接只读维度");
    assert!(!flags.editor, "项目锁不影响能不能改文本");

    // 读语句照跑：项目只读不是“不能分析”，只是“不能改数据”
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert_eq!(seen.lock().expect("锁").len(), 1, "SELECT 该发出去");
}

/// 没接项目端口（宿主不关心项目态）：不拦——与 1b 之前的行为一致，不假装锁着
#[gpui_kit::test]
fn without_the_project_port_writes_are_not_blocked(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _seen_conn) = shared_with_runner("update t set a = 1;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "update t set a = 1",
        execution::ResultPlacement::Replace,
    );
    assert_eq!(
        seen.lock().expect("锁").len(),
        1,
        "未接端口 = 不拦（与现状一致）"
    );
    let flags = cx.update(|_window, cx| panel.read(cx).read_only_flags());
    assert!(!flags.connection, "没锁就不该说“连接只读”");
}

/// 补全的门控：SQL 模式开、文本模式关（能力表说了算）；未接端口时目录空但**关键字仍在**
#[gpui_kit::test]
fn completion_is_gated_by_mode_and_falls_back_to_keywords(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (sql_shared, sql_id, _seen, _seen_conn) =
        shared_with_runner("select * from ord", EditorMode::Sql);
    assert!(
        sql_shared.completion_enabled(&sql_id),
        "SQL 模式该开补全（候选端口没接 = 只给关键字）"
    );
    assert!(
        sql_shared.completion_catalog(&sql_id).is_empty(),
        "未接端口 → 没有元数据候选（如实，不假装有）"
    );

    let (text_shared, text_id, _seen, _seen_conn) = shared_with_runner("hello", EditorMode::General);
    assert!(
        !text_shared.completion_enabled(&text_id),
        "文本模式不与数据库通信，也不该补全"
    );
}

/// 【B13】通道不可用时就算被程序叫到也不切（菜单置灰之外的**第二道闸**）
#[gpui_kit::test]
fn an_unavailable_channel_cannot_be_switched_to(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.attach_channels(Rc::new(FakeChannels::blocked(
        "该连接未开启本地加速",
        "尚未注册外部源",
    )));
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_channel(ExecChannel::Federated, cx)
        });
    });
    assert_eq!(
        shared.service().channel_for(&id),
        ExecChannel::Source,
        "不可用就不切：界面不许停在“看着能用、一按就错”的状态"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留原因");
    assert!(message.contains("联邦不可用"), "{message}");
    assert!(message.contains("尚未注册外部源"), "{message}");
}

/// 【B13】通道随会话恢复：可用就恢复，已失效就**回退源库并说清原因**
#[gpui_kit::test]
fn the_channel_comes_back_with_the_session_or_falls_back_with_a_reason(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.attach_channels(Rc::new(FakeChannels::open()));
    let (panel, cx) = open_panel(cx, &shared, &id);

    let mut session = crate::session::SavedSession {
        id: "d:/sql/a.sql".to_string(),
        path: Some("D:/sql/a.sql".to_string()),
        mode: EditorMode::Sql,
        channel: ExecChannel::Accelerated,
        connection: None,
        content: "select 1;".to_string(),
        cursor: 0,
        selection: None,
        toolbar: None,
        schema: None,
        tab_size: None,
        hard_tabs: None,
        result_column_order: None,
    };
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.restore_session(&session, window, cx));
    });
    assert_eq!(
        shared.service().channel_for(&id),
        ExecChannel::Accelerated,
        "通道与模式同类：会话里存着就该回来"
    );

    // 已失效：回退源库 + 原因（不静静地接着用那一档）
    let (shared2, id2, _seen2, _seen_conn2) = shared_with_runner("select 1;", EditorMode::Sql);
    shared2.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared2.attach_channels(Rc::new(FakeChannels::blocked(
        "分析引擎尚未就绪",
        "尚未注册外部源",
    )));
    let (panel2, cx) = open_panel(cx, &shared2, &id2);
    session.channel = ExecChannel::Accelerated;
    cx.update(|window, cx| {
        panel2.update(cx, |panel, cx| panel.restore_session(&session, window, cx));
    });
    assert_eq!(shared2.service().channel_for(&id2), ExecChannel::Source);
    let message = cx
        .update(|_window, cx| panel2.read(cx).message.clone())
        .expect("回退也要留痕");
    assert!(message.contains("已失效"), "{message}");
    assert!(message.contains("回退源库"), "{message}");
}

/// 【B13】会话快照要带上通道（存的是短码：重启后按码认回来）
#[gpui_kit::test]
fn the_session_snapshot_carries_the_channel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);
    // 未命名文档不存会话（重启后认不回来）——通道也不例外
    assert!(
        cx.update(|_window, cx| panel.read(cx).session_snapshot(cx))
            .is_none(),
        "未命名文档没有稳定标识，不该存"
    );

    // 有路径的文档：快照里带通道码
    let (shared2, id2) = shared_with_document("D:/sql/channel.sql", "select 1;");
    shared2.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared2.attach_channels(Rc::new(FakeChannels::open()));
    let (panel2, cx) = open_panel(cx, &shared2, &id2);
    cx.update(|_window, cx| {
        panel2.update(cx, |panel, cx| {
            panel.set_channel(ExecChannel::Accelerated, cx)
        });
    });

    let snapshot = cx
        .update(|_window, cx| panel2.read(cx).session_snapshot(cx))
        .expect("有路径就该存会话");
    assert_eq!(snapshot.channel, ExecChannel::Accelerated);
    assert_eq!(snapshot.channel.code(), "accelerated");
}

/// 【B1 余项】连接绑定随会话回来：还在列表里就恢复；**不在就回退并说原因**
#[gpui_kit::test]
fn the_connection_binding_comes_back_with_the_session(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let session = |connection: Option<&str>| crate::session::SavedSession {
        id: "d:/sql/a.sql".to_string(),
        path: Some("D:/sql/a.sql".to_string()),
        mode: EditorMode::Sql,
        channel: ExecChannel::Source,
        connection: connection.map(str::to_string),
        content: "select 1;".to_string(),
        cursor: 0,
        selection: None,
        toolbar: None,
        schema: None,
        tab_size: None,
        hard_tabs: None,
        result_column_order: None,
    };

    // ① 连接还在列表里 → 绑定真的写回文档
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.restore_session(&session(Some("P_orders")), window, cx)
        });
    });
    assert_eq!(
        shared.service().connection_for(&id).as_deref(),
        Some("P_orders"),
        "重启后应该还连着原来那条连接（而不是默默变成“跟随当前连接”）"
    );

    // ② 连接已被删 / 不在当前项目 → 回退未绑定 + 可读原因
    let (shared2, id2, _seen2, _seen_conn2) = shared_with_runner("select 1;", EditorMode::Sql);
    shared2.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    let (panel2, cx) = open_panel(cx, &shared2, &id2);
    cx.update(|window, cx| {
        panel2.update(cx, |panel, cx| {
            panel.restore_session(&session(Some("P_gone")), window, cx)
        });
    });
    assert_eq!(
        shared2.service().connection_for(&id2),
        None,
        "不存在的绑定不能留着（否则执行时才发现发错了地方）"
    );
    let message = cx
        .update(|_window, cx| panel2.read(cx).message.clone())
        .expect("回退要留痕");
    assert!(message.contains("P_gone"), "{message}");
    assert!(message.contains("跟随当前连接"), "{message}");

    // ③ 没接连接端口（测试与嵌入场景）→ **不校验也不丢**（宁可不猜，也不抹掉用户的绑定）
    let (shared3, id3, _seen3, _seen_conn3) = shared_with_runner("select 1;", EditorMode::Sql);
    let (panel3, cx) = open_panel(cx, &shared3, &id3);
    cx.update(|window, cx| {
        panel3.update(cx, |panel, cx| {
            panel.restore_session(&session(Some("P_orders")), window, cx)
        });
    });
    assert_eq!(
        shared3.service().connection_for(&id3).as_deref(),
        Some("P_orders")
    );
}

/// 【B1 余项】会话快照要带上连接绑定（存进 `editor_contexts.connection_id`）
#[gpui_kit::test]
fn the_session_snapshot_carries_the_connection_binding(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document("D:/sql/bind.sql", "select 1;");
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 没绑就是 None（“跟随当前连接”也是一个**真值**，不能被当成“忘了存”）
    let snapshot = cx
        .update(|_window, cx| panel.read(cx).session_snapshot(cx))
        .expect("有路径就该存会话");
    assert_eq!(snapshot.connection, None);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_orders".to_string()), cx)
        })
    });
    let snapshot = cx
        .update(|_window, cx| panel.read(cx).session_snapshot(cx))
        .expect("有路径就该存会话");
    assert_eq!(snapshot.connection.as_deref(), Some("P_orders"));
}

/// 【T2c-6】工具栏布局随会话走：改布局 → 快照带编码串 → 恢复后原样回来
#[gpui_kit::test]
fn the_toolbar_layout_rides_along_with_the_session(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document("D:/sql/toolbar.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 默认布局也能进快照（改动即存，不设“值得存”阈值）
    let snapshot = cx
        .update(|_window, cx| panel.read(cx).session_snapshot(cx))
        .expect("有路径就该存会话");
    assert_eq!(
        snapshot.toolbar.as_deref(),
        Some(
            "ExecRun,ExecAll,~ExecCurrent,~ExecSelection,~ExecBatch,ExecNewSet,|,Explain,Translate,Template,|,TxAutoCommit,Format,History"
        ),
        "快照携带默认布局的编码串"
    );

    // 改布局：隐藏 Format、折叠执行簇、加一条分隔符、移动 History
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_toolbar_visible(crate::toolbar::ToolbarItemId::Format, false, cx);
            panel.toggle_toolbar_collapsed(crate::toolbar::ToolbarItemId::ExecRun, cx);
            panel.insert_toolbar_separator(cx);
            panel.move_toolbar_item(crate::toolbar::ToolbarItemId::History, crate::toolbar::MoveDir::Left, cx);
        });
    });
    let snapshot = cx
        .update(|_window, cx| panel.read(cx).session_snapshot(cx))
        .expect("有路径就该存会话");
    let code = snapshot
        .toolbar
        .as_deref()
        .expect("改动后的布局要进快照")
        .to_string();

    // 新面板（模拟重启）：恢复会话 → 布局原样回来
    let (panel2, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| {
        panel2.update(cx, |panel, cx| {
            panel.restore_session(&snapshot, window, cx)
        });
    });
    let restored = cx
        .update(|_window, cx| panel2.read(cx).toolbar_layout_for_test());
    let expected = crate::toolbar::ToolbarLayout::decode(&code).expect("快照串必可解码");
    assert_eq!(restored, expected, "恢复后布局与改动时一致");
    assert!(
        !restored.is_visible(crate::toolbar::ToolbarItemId::Format),
        "隐藏的 Format 恢复后仍隐藏"
    );
    assert!(
        restored.is_collapsed(crate::toolbar::ToolbarItemId::ExecRun),
        "折叠的执行簇恢复后仍折叠"
    );

    // 旧会话没有工具栏字段（None）→ 不覆盖：保持面板默认
    let mut legacy = crate::session::SavedSession {
        id: "d:/sql/toolbar.sql".to_string(),
        path: Some("D:/sql/toolbar.sql".to_string()),
        mode: EditorMode::Sql,
        channel: ExecChannel::Source,
        connection: None,
        content: "select 1;".to_string(),
        cursor: 0,
        selection: None,
        toolbar: None,
        schema: None,
        tab_size: None,
        hard_tabs: None,
        result_column_order: None,
    };
    let (panel3, cx) = open_panel(cx, &shared, &id);
    legacy.toolbar = None;
    cx.update(|window, cx| {
        panel3.update(cx, |panel, cx| panel.restore_session(&legacy, window, cx));
    });
    assert_eq!(
        cx.update(|_window, cx| panel3.read(cx).toolbar_layout_for_test()),
        crate::toolbar::ToolbarLayout::default_layout(),
        "旧会话没存布局 → 恢复用默认"
    );
}

/// 【T2c-1 余项】schema 绑定随会话走：在列表里就恢复；**不在就回退连接默认并说原因**
#[gpui_kit::test]
fn the_schema_binding_comes_back_with_the_session(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let session = |schema: Option<&str>| crate::session::SavedSession {
        id: "d:/sql/a.sql".to_string(),
        path: Some("D:/sql/a.sql".to_string()),
        mode: EditorMode::Sql,
        channel: ExecChannel::Source,
        connection: Some("P_orders".to_string()),
        content: "select 1;".to_string(),
        cursor: 0,
        selection: None,
        toolbar: None,
        schema: schema.map(str::to_string),
        tab_size: None,
        hard_tabs: None,
        result_column_order: None,
    };

    // ① schema 在列表里 → 绑定真的写回文档
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.attach_schemas(Rc::new(FakeSchemas(vec![SchemaInfo {
        catalog: Some("rdb".to_string()),
        schema: "analytics".to_string(),
    }])));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.restore_session(&session(Some("analytics")), window, cx)
        });
    });
    assert_eq!(
        schema_doc(&shared, &id).as_deref(),
        Some("analytics"),
        "重启后应该还绑着原来的 schema"
    );

    // ② schema 已被删 / 换库 → 回退未绑定 + 可读原因
    let (shared2, id2, _seen2, _seen_conn2) = shared_with_runner("select 1;", EditorMode::Sql);
    shared2.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared2.attach_schemas(Rc::new(FakeSchemas(vec![SchemaInfo {
        catalog: Some("rdb".to_string()),
        schema: "public".to_string(),
    }])));
    let (panel2, cx) = open_panel(cx, &shared2, &id2);
    cx.update(|window, cx| {
        panel2.update(cx, |panel, cx| {
            panel.restore_session(&session(Some("analytics")), window, cx)
        });
    });
    assert_eq!(
        schema_doc(&shared2, &id2),
        None,
        "不存在的 schema 绑定不能留着（执行时会发错库）"
    );
    let message = cx
        .update(|_window, cx| panel2.read(cx).message.clone())
        .expect("回退要留痕");
    assert!(message.contains("analytics"), "{message}");
    assert!(message.contains("连接默认"), "{message}");

    // ③ 没接 schema 端口 → 不校验也不丢（宁可不猜，也不抹掉用户的绑定）
    let (shared3, id3, _seen3, _seen_conn3) = shared_with_runner("select 1;", EditorMode::Sql);
    shared3.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    let (panel3, cx) = open_panel(cx, &shared3, &id3);
    cx.update(|window, cx| {
        panel3.update(cx, |panel, cx| {
            panel.restore_session(&session(Some("analytics")), window, cx)
        });
    });
    assert_eq!(
        schema_doc(&shared3, &id3).as_deref(),
        Some("analytics"),
        "没有列表可查就不该动手抹"
    );
}

/// 【T2c-4 余项】缩进随会话走：快照带宽度与制表符，恢复后文档状态 + 内核双写
#[gpui_kit::test]
fn the_indent_rides_along_with_the_session(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document("D:/sql/indent.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 默认（#48 设置默认：宽度 4、空格缩进）
    let snapshot = cx
        .update(|_window, cx| panel.read(cx).session_snapshot(cx))
        .expect("有路径就该存会话");
    assert_eq!(snapshot.tab_size, Some(4));
    assert_eq!(snapshot.hard_tabs, Some(false));

    // 改成 4 + 制表符
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_hard_tabs(true, cx);
            panel.set_tab_size(4, cx);
        });
    });
    let snapshot = cx
        .update(|_window, cx| panel.read(cx).session_snapshot(cx))
        .expect("有路径就该存会话");
    assert_eq!(snapshot.tab_size, Some(4));
    assert_eq!(snapshot.hard_tabs, Some(true));

    // 新面板（模拟重启）：恢复会话 → 缩进原样回来
    let (panel2, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| {
        panel2.update(cx, |panel, cx| {
            panel.restore_session(&snapshot, window, cx)
        });
    });
    assert_eq!(shared.document_tab_size(&id), 4, "文档级宽度恢复");
    assert!(shared.document_hard_tabs(&id), "文档级制表符恢复");
    // 双写不 panic：恢复后照常渲染（内核 set_tab_size / set_hard_tabs 走通）
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 【B9 切片二】保补全端口的假实现：一份目录 + 一批模板（都是内存读）
pub(crate) struct FakeCompletion {
    pub(crate) catalog: Catalog,
    pub(crate) templates: Vec<crate::completion::TemplateSnippet>,
}

impl crate::completion::CompletionPort for FakeCompletion {
    fn catalog(
        &self,
        conn_id: Option<&str>,
        _channel: ExecChannel,
        _schema: Option<&str>,
    ) -> Catalog {
        // 与真实实现同一口径：未绑定连接不给候选
        match conn_id {
            Some(_) => self.catalog.clone(),
            None => Catalog::default(),
        }
    }

    fn templates(&self) -> Vec<crate::completion::TemplateSnippet> {
        self.templates.clone()
    }

    /// 【T2a-2】测试夹具：`orders` 表的主键是 `id`（让编辑判据真的放行——探测链路
    /// 是真实走通的：结果入库前 `probe_primary_key` 会从这里取主键列）
    fn primary_key(
        &self,
        _conn_id: Option<&str>,
        _channel: ExecChannel,
        table: &str,
    ) -> Option<Vec<String>> {
        (table == "orders").then(|| vec!["id".to_string()])
    }
}

fn snippet(name: &str, content: &str) -> crate::completion::TemplateSnippet {
    crate::completion::TemplateSnippet {
        id: format!("builtin_{name}"),
        name: name.to_string(),
        content: content.to_string(),
        description: None,
    }
}

/// 【B9 切片二】`Ctrl+Space` 手动补全：候选真的弹出来（走真按键路径）
#[gpui_kit::test]
fn ctrl_space_presents_the_completion_menu(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("select * from ord", EditorMode::Sql);
    // 端口只在**绑定了连接**时给候选（未绑定 = 不猜是哪条连接）——所以这里先绑一条
    shared.update(|service| service.set_connection(&id, Some("P_orders".to_string())));
    shared.attach_completion(Rc::new(FakeCompletion {
        catalog: Catalog {
            objects: vec![Candidate::new("public.orders", CandidateKind::Table)],
            columns: vec![(
                "public.orders".to_string(),
                "total".to_string(),
                Some("numeric".to_string()),
            )],
            truncated: false,
        },
        templates: vec![] as Vec<crate::completion::TemplateSnippet>,
    }));
    let (panel, cx) = open_panel(cx, &shared, &id);
    // 光标停在句尾（新开的文档光标默认在 0；手动补全是**在光标处**算上下文的）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_caret_for_test(17, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-space");

    let (open, query, labels) =
        cx.update(|_window, cx| panel.read(cx).completion_menu_for_test(cx));
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(open, "快捷键要真的把候选弹出来（message={message:?}）");
    assert_eq!(query, "ord", "要替换的是光标前那个词");
    assert!(
        labels.contains(&"public.orders".to_string()),
        "表位的候选里要有那张表：{labels:?}"
    );
}

/// 【B9 切片二】手动补全的拒绝要说清原因（文本模式 / 只读；不静静地什么都不发生）
#[gpui_kit::test]
fn ctrl_space_refuses_with_a_reason_outside_sql_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("hello", EditorMode::General);
    shared.attach_completion(Rc::new(FakeCompletion {
        catalog: Catalog::default(),
        templates: vec![] as Vec<crate::completion::TemplateSnippet>,
    }));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-space");

    let (open, _, _) = cx.update(|_window, cx| panel.read(cx).completion_menu_for_test(cx));
    assert!(!open, "文本模式不解析 SQL：不该弹补全");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留一句为什么");
    assert!(message.contains("文本模式"), "{message}");
}

/// 【B9 切片二】插入模板：内容落进文本，且 `{table}` 被选中（接着敲就直接覆写）
#[gpui_kit::test]
fn inserting_a_template_writes_it_and_selects_the_placeholder(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("", EditorMode::Sql);
    shared.attach_completion(Rc::new(FakeCompletion {
        catalog: Catalog::default(),
        templates: vec![snippet("查询所有记录", "SELECT * FROM {table};")],
    }));
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.insert_template(0, window, cx)));

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert_eq!(text, "SELECT * FROM {table};", "模板内容原样插进光标处");
    let selection = cx.update(|_window, cx| panel.read(cx).selected_range_for_test(cx));
    assert_eq!(
        &text[selection.start..selection.end],
        "{table}",
        "占位符要被选中（用户接着敲表名就覆写它）"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("插入要有回执");
    assert!(message.contains("查询所有记录"), "{message}");
}

/// 只读文档插模板：拒绝并说明（与其它写文本的动作同一口径）
#[gpui_kit::test]
fn inserting_a_template_into_a_readonly_document_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen, _seen_conn) = shared_with_runner("", EditorMode::Sql);
    shared.attach_completion(Rc::new(FakeCompletion {
        catalog: Catalog::default(),
        templates: vec![snippet("查询所有记录", "SELECT * FROM {table};")],
    }));
    let (panel, cx) = open_panel(cx, &shared, &id);
    shared.update(|service| service.set_read_only(&id, crate::model::ReadOnly::editor_only()));

    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.insert_template(0, window, cx)));

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(text.is_empty(), "只读文档不该被写进去");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留原因");
    assert!(message.contains("只读"), "{message}");
}

/// 写语句 / 失败没有网格：导出要回绝得可读（不能抓一个空网格去写文件）
#[gpui_kit::test]
fn exporting_without_a_grid_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("insert into t values (1)");
    let path = export_temp_path("nogrid", "csv");
    std::fs::remove_file(&path).ok();
    attach_export_picker(&shared, Some(path.clone()));
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "insert into t values (1)",
        execution::ResultPlacement::Replace,
    );
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result(ExportFormat::Csv, cx)
        });
    });

    assert!(!path.exists(), "写语句没有可导出的网格");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留原因");
    assert!(message.contains("没有可导出的结果集"), "{message}");
    // 范围也一并确认：导出菜单不在这份结果上出现
    assert!(
        !dialog_button_rendered(cx, "editor-result-export"),
        "没有网格就不摆导出"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// B7 切片二：Parquet / XLSX（经 DuckDB 落盘）
