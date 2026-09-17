//! 洞察面板的**动作类**宿主接线（Phase 4 收尾）：Schema 报告的下钻与导出。
//!
//! 取数 / 写库那半边在 `insight::jobs::attach`（不需要窗口与系统对话框）；
//! 这半边要窗口（系统保存对话框）、工作台状态（连接列表 / 状态栏回执 / 右 Dock 面板）
//! 与洞察的取样入口（`Shared::open_insight_source_table`），所以落在宿主侧——与
//! `resource_host` / `scratchpad_host` 同例：特性 crate 发事件，宿主持有实现。
//!
//! 回执一律走状态栏（`Shared::say`）：导出是「菜单一点、对话框一关」的动作，
//! 结果说在状态栏够了，不往 280px 的面板里再塞一行提示。

use gpui_kit::{App, Context, Entity, Subscription};

use insight::insight_view::InsightView;
use insight::{InsightEvent, SampleSource, SchemaExportFormat};

use crate::panels::Shared;

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
        move |_host, _panel, event: &InsightEvent, cx| match event {
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
            _ => {}
        },
    )
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
