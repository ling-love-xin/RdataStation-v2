//! 洞察面板的**动作类**宿主接线（Phase 4 收尾）：Schema 报告的下钻与导出。
//!
//! 取数 / 写库那半边在 `insight::jobs::attach`（不需要窗口与系统对话框）；
//! 这半边要窗口（系统保存对话框）、工作台状态（连接列表 / 状态栏回执 / 右 Dock 面板）
//! 与洞察的取样入口（`Shared::open_insight_source_table`），所以落在宿主侧——与
//! `resource_host` / `scratchpad_host` 同例：特性 crate 发事件，宿主持有实现。
//!
//! 回执一律走状态栏（`Shared::say`）：导出是「菜单一点、对话框一关」的动作，
//! 结果说在状态栏够了，不往 280px 的面板里再塞一行提示。

use gpui_kit::{App, Context, Entity, Subscription, Window};
// `open_alert_dialog` 是 WindowExt 提供的方法（gpui-component 的扩展 trait）
use gpui_kit::component::WindowExt as _;

use analytics_resource::model::{
    ArchiveBinding, ArchiveKind, ArchiveRequest, KeepVersions, PayloadMode,
};
use insight::insight_view::InsightView;
use insight::{InsightEvent, InsightService, SampleSource, SchemaExportFormat};

use crate::panels::Shared;
use crate::services::resource_jobs;

/// 全量统计确认弹窗（资源治理 2）：订阅 `FullProfileConfirmRequested`。
///
/// 与 [`attach`] 分开的原因：确认弹窗要 `Window`（`open_alert_dialog`），而
/// `attach` 的订阅闭包只有 `App`。这里用 `subscribe_in` 拿到窗口：
/// - 设置「全量统计前确认」开着 → 弹确认框（说明大表全量慢且占内存）；
/// - 关着 → 不弹，直接 `confirm_full` 执行。
///
/// 行数预估不做：文案说明「大表全量会慢并占用内存」，避免再开 COUNT 通道。
/// 返回的订阅**必须被装配方持有**（drop 即取消订阅）。
pub fn attach_dialogs<H: 'static>(
    panel: &Entity<InsightView>,
    window: &Window,
    cx: &mut Context<H>,
) -> Subscription {
    let panel_weak = panel.downgrade();
    cx.subscribe_in(
        panel,
        window,
        move |_host, _panel, event: &InsightEvent, window, cx| {
            if let InsightEvent::FullProfileConfirmRequested { .. } = event {
                let confirm = settings::SettingsService::full_scan_confirm(cx);
                if !confirm {
                    // 设置关闭确认：直接执行全量（面板 scope 已就绪，`confirm_full` 是幂等的）
                    if let Some(panel) = panel_weak.upgrade() {
                        panel.update(cx, |view, cx| view.confirm_full(cx));
                    }
                    return;
                }
                // 外层是 `FnMut`（可能被多次调用）：弹窗闭包只能拿走 clone
                let panel_weak_dialog = panel_weak.clone();
                window.open_alert_dialog(cx, move |alert, _window, _cx| {
                    let panel = panel_weak_dialog.clone();
                    alert
                        .confirm()
                        .title("全量统计")
                        .description(
                            "将对当前表进行全量统计：大表耗时较长，且表会完整导入分析引擎临时表（占用内存）。取样结果将被替换。确定继续？",
                        )
                        .on_ok(move |_window, _cx, cx| {
                            if let Some(panel) = panel.upgrade() {
                                panel.update(cx, |view, cx| view.confirm_full(cx));
                            }
                            true
                        })
                });
            }
        },
    )
}

/// 装配 / 设置变更时把设置页 `insight.*` 推给洞察侧（资源治理 1/3/4）。
///
/// - 内存上限：应用到分析引擎连接（`SET memory_limit`，即时生效）；设置页后续
///   变更由 `install_insight_memory_limit_sink` 的 sink 走同一函数（幂等）。
/// - 面板偏好：`status_usage`（工具栏内存显示 + 轮询开关）与 `work_set_limit`
///   （集合卡工作集阈值）经 `InsightView::apply_insight_prefs` 推送。
///   右栏面板与中间工作页共用同一 `InsightView` 实体（D65 双栏单源），推一次即可。
pub fn apply_insight_prefs(shared: &Shared, cx: &mut App) {
    engine::duckdb::DuckDBManager::apply_memory_limit_mb(
        settings::SettingsService::memory_limit_mb(cx),
    );
    let status_usage = settings::SettingsService::status_usage(cx);
    let work_set_limit = settings::SettingsService::work_set_limit(cx).max(0) as usize;
    let Some(panel) = shared.insight_panel.borrow().as_ref().and_then(|w| w.upgrade()) else {
        return;
    };
    panel.update(cx, |view, cx| {
        view.apply_insight_prefs(status_usage, work_set_limit, cx);
    });
}

/// 订阅面板事件并落地两个宿主动作。
///
/// 返回的订阅**必须被装配方持有**（drop 即取消订阅）。
pub fn attach<H: 'static>(
    panel: &Entity<InsightView>,
    shared: &Shared,
    cx: &mut Context<H>,
) -> Subscription {
    let shared = shared.clone();
    cx.subscribe(
        panel,
        move |_host, panel, event: &InsightEvent, cx| match event {
            InsightEvent::TableDrilldownRequested {
                conn_id,
                database,
                schema,
                table,
            } => drilldown(&shared, conn_id, database, schema, table, cx),
            InsightEvent::SchemaExportRequested {
                format,
                file_stem,
                content,
            } => export_report(&shared, *format, file_stem, content, cx),
            InsightEvent::TableReportExportRequested {
                file_stem, content, ..
            } => export_markdown_report(&shared, file_stem, content, cx),
            InsightEvent::CompareRequested { key } => {
                shared.request_insight_compare(key, cx);
            }
            InsightEvent::ArchiveRequested { key } => {
                archive_from_insight(&shared, &panel, key, cx);
            }
            _ => {}
        },
    )
}

/// 洞察表归档到资产库（T20）：画像快照 JSON → `resources/insight/` File 档。
///
/// 数据流：面板只报业务键 → 这里从集合条目取临时表名与展示名 → 洞察服务把
/// 表级画像快照导出为 JSON（`{项目}/.RSmeta/insight-archive/`）→ 走资产库
/// `ArchiveService`（copy 进 `resources/insight/` + 登记 + 版本链）。
/// 每次归档生成新时间戳 → 内容指纹不同 → 资产库自动开新版本（审计长在版本上）。
///
/// 快照导出要抢 DuckDB 锁，放后台线程；归档入队与回执回主线程。
fn archive_from_insight(
    shared: &Shared,
    panel: &Entity<InsightView>,
    key: &str,
    cx: &mut App,
) {
    let Some(root) = shared.project_root() else {
        shared.say("洞察：归档需要先打开项目", cx);
        return;
    };
    if shared.project_ui.borrow().read_only {
        shared.say("洞察：项目为只读，不能归档", cx);
        return;
    }
    let Some((temp_table, label)) = panel
        .read(cx)
        .insight_set()
        .iter()
        .find(|entry| entry.key == key)
        .and_then(|entry| entry.temp_table.clone().map(|t| (t, entry.label.clone())))
    else {
        shared.say("洞察：该表尚未完成取样，无法归档", cx);
        return;
    };

    let shared2 = shared.clone();
    // 保留策略在主线程读好随闭包带入：async 里拿不到 GPUI global 设置
    let keep_versions = KeepVersions::from_setting(settings::SettingsService::keep_versions(cx));
    cx.spawn(async move |cx| {
        let (path, name) =
            match InsightService::export_table_snapshot(&root, &temp_table, &label) {
                Ok(export) => export,
                Err(error) => {
                    let message = InsightService::describe_error(&error).message;
                    let _ = cx.update(|cx| {
                        shared2.say(format!("洞察：归档失败（{message}）"), cx);
                    });
                    return;
                }
            };
        resource_jobs::enqueue_archive(
            root.clone(),
            false,
            ArchiveRequest {
                source_path: path,
                rel_path: format!("insight/{name}.json"),
                name: format!("洞察·{name}"),
                alias: None,
                kind: ArchiveKind::File,
                payload_mode: PayloadMode::Copy,
                analysis: None,
                binding: ArchiveBinding {
                    promoted_from: None,
                    source_connection_id: None,
                    source_table: Some(name.clone()),
                },
                tags: vec!["洞察".to_string()],
                group_id: None,
                keep_versions: None,
                existing_resource_id: None,
            },
            keep_versions,
            false,
        );
        let _ = cx.update(|cx| {
            shared2.refresh_resources(cx);
            shared2.say(format!("洞察：已归档「{name}」到资产库"), cx);
        });
    })
    .detach();
}

/// 下钻：点中的源表 → 洞察目标（D58 的源取样通道）。
///
/// **不建临时表**：洞察侧自己取 500 行样本（D59）——这里只给「在哪条连接上查哪张表」，
/// 限定名按驱动加引号（与导航树「查看统计」共用 `Shared::insight_sample_sql`）。
fn drilldown(
    shared: &Shared,
    conn_id: &str,
    database: &str,
    schema: &str,
    table: &str,
    cx: &mut App,
) {
    let sql = shared.insight_sample_sql(conn_id, &[database, schema, table]);
    // 来源标签：与导航树同一写法（空段略去），快照的 `entity_source` 会用它
    let label = [database, schema, table]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".");
    let source = SampleSource::new(conn_id.to_string(), sql, label);
    shared.open_insight_source_table(source, table.to_string(), cx);
}

/// 导出：系统保存对话框（`rfd`）→ 写文件 → 状态栏回执。
///
/// 内容由**面板算好**（导出与界面同源，D36）：宿主只挑路径与写盘，不懂格式。
/// 用户取消不算失败——什么都不做（与另存为同一口径）。
fn export_report(
    shared: &Shared,
    format: SchemaExportFormat,
    file_stem: &str,
    content: &str,
    cx: &mut App,
) {
    let file_name = format!("{file_stem}.{}", format.extension());
    let Some(path) = rfd::FileDialog::new()
        .set_title(format!("导出 Schema 报告（{}）", format.label()))
        .set_file_name(&file_name)
        .add_filter(format.label(), &[format.extension()])
        .save_file()
    else {
        return;
    };
    match std::fs::write(&path, content) {
        Ok(()) => shared.say(format!("洞察：已导出 Schema 报告 → {}", path.display()), cx),
        Err(error) => shared.say(format!("洞察：导出失败（{error}）"), cx),
    }
}

/// 表画像 EDA 报告导出（缺口 1：`TableReportExportRequested` 宿主落地）。
///
/// 与 Schema 导出同一交互口径（D36）：用户选路径（取消 = 静默成功），
/// 写盘失败才报状态栏。文件固定 `.md`。
fn export_markdown_report(shared: &Shared, file_stem: &str, content: &str, cx: &mut App) {
    let file_name = format!("{file_stem}.md");
    let Some(path) = rfd::FileDialog::new()
        .set_title("导出洞察报告")
        .set_file_name(&file_name)
        .add_filter("Markdown", &["md"])
        .save_file()
    else {
        return;
    };
    match std::fs::write(&path, content) {
        Ok(()) => shared.say(format!("洞察：已导出报告 → {}", path.display()), cx),
        Err(error) => shared.say(format!("洞察：导出失败（{error}）"), cx),
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：不通配导入（`use gpui_kit::*` 会把 `test` 属性宏带进来，
    // 与 `#[gpui_kit::test]` 展开出的裸 `#[test]` 自相残杀）。
    use gpui_kit::{AppContext as _, Subscription, TestAppContext};

    use insight::{InsightTarget, InsightView};

    use super::attach;
    use crate::panels::Shared;
    use crate::view::{ConnectionItem, RightPanel, SidebarMode};

    /// 宿主壳：只为持有订阅（drop 即取消，测试里不能让它提前消失）。
    struct Harness {
        _sub: Subscription,
    }

    /// 连接条目（测试用：只填取样 SQL 判定要用的字段）。
    fn conn(id: &str, driver: &str) -> ConnectionItem {
        ConnectionItem {
            id: id.to_string(),
            name: id.to_string(),
            driver: driver.to_string(),
            connected: true,
            host: None,
            port: None,
            database: None,
            schema: None,
            description: None,
            use_duckdb_fed: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// 下钻：点报告里的源表名 → 面板打开并指向**源取样**（D58）——
    /// 不建临时表，只给「在哪条连接上查哪张表」，限定名按驱动加引号。
    #[gpui_kit::test]
    fn drilldown_opens_the_panel_with_a_driver_quoted_source(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let shared = Shared::new();
        shared.connections.borrow_mut().push(conn("G_1", "mysql"));

        let (panel, _host) = cx.update(|cx| {
            let panel = cx.new(InsightView::new);
            *shared.insight_panel.borrow_mut() = Some(panel.downgrade());
            let panel_for_host = panel.clone();
            let shared_for_host = shared.clone();
            let host = cx.new(move |cx| Harness {
                _sub: attach(&panel_for_host, &shared_for_host, cx),
            });
            (panel, host)
        });

        // 下钻的起点：目标是一份结构报告（连接 / 库 / schema 都在目标里）
        cx.update(|cx| {
            panel.update(cx, |view, cx| {
                view.set_target(
                    InsightTarget::Schema {
                        conn_id: "G_1".into(),
                        database: "mall".into(),
                        schema: Some(String::new()),
                    },
                    cx,
                );
            });
        });
        cx.update(|cx| {
            panel.update(cx, |view, cx| view.request_table_drilldown("order", cx));
        });

        assert_eq!(
            shared.right_mode.get(),
            SidebarMode::Expanded,
            "下钻要把右 Dock 展开到洞察"
        );
        assert_eq!(shared.active_right.get(), RightPanel::Insight);
        panel.read_with(cx, |view, _| match view.target() {
            Some(InsightTarget::SourceTable { source, table_name }) => {
                assert_eq!(
                    source.sql, "SELECT * FROM `mall`.`order`",
                    "限定名按驱动加引号（MySQL 系反引号）"
                );
                assert_eq!(source.label(), "mall.order", "来源标签空段略去");
                assert_eq!(table_name, "order");
            }
            other => panic!("下钻该把目标换成源取样，实际是：{other:?}"),
        });
    }
}
