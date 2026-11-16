//! 「新建洞察」对话框：`source_picker` 内核 + **洞察物化策略**。
//!
//! 来源浏览 / 单选 / 确认骨架全部由 [`super::source_picker`] 提供；
//! 本文件只定义洞察的出口——选中来源后构造 `insight::SampleSource` 并
//! **双栏同时**打开（右栏 + 中间页 EDA，D65 双栏单源语义：
//! `Shared::open_insight_source_table`；文件类用 `SampleSource::duckdb_file` 自动选读取器）。
//!
//! 洞察裁剪掉 `SourceKind::ResultSet`（那是结果集二次分析的专属来源）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use insight::SampleSource;

use crate::components::source_picker::{self, Picked, PickerConfig, PickerState, SourceKind};
use crate::panels::Shared;

/// 洞察启用且按用户拍板顺序排列的来源（无结果集）。
const INSIGHT_KINDS: &[SourceKind] = &[
    SourceKind::Connected,
    SourceKind::Scratchpad,
    SourceKind::Analysis,
    SourceKind::AnalysisGlobal,
    SourceKind::External,
    SourceKind::InsightSample,
];

/// 打开「新建洞察」对话框（宿主侧入口，`insight_actions` 订阅 `NewInsightRequested` 后调用）。
///
/// 连接列表、草稿 / 分析表的预载与提示行生成都在内核 `open_source_picker` 里完成。
pub fn open_new_insight_dialog(shared: &Shared, window: &mut Window, cx: &mut App) {
    let config = PickerConfig {
        title: "新建洞察",
        kinds: INSIGHT_KINDS,
        confirm_prefix: "分析",
        // 多源入口：一次可选多个来源（面板按来源开标签页，见 model::InsightTarget::SourceTables）
        multi: true,
    };
    let strategy = Rc::new(
        |shared: &Shared, state: &Rc<RefCell<PickerState>>, window: &mut Window, cx: &mut App| {
            confirm(shared, state, window, cx);
        },
    );
    source_picker::open_source_picker(shared, config, strategy, window, cx);
}

/// 洞察物化策略：把选中目标逐个转成取样来源，然后**双栏同时**打开并关闭对话框。
///
/// 单选（绝大多数入口）= 单源表目标；多选（本对话框 `multi: true`）= 一个面板 +
/// 来源标签页（`InsightTarget::SourceTables`）。失败（文件无法读取）写回底部提示
/// 并保持对话框，不关闭。
fn confirm(shared: &Shared, state: &Rc<RefCell<PickerState>>, window: &mut Window, cx: &mut App) {
    let picks = state.borrow().picked_all();
    let mut sources: Vec<(SampleSource, String)> = Vec::new();
    for picked in picks {
        match picked {
            Picked::Db(conn_id, table) => {
                // 无 schema 层时 schema 为空串：insight_sample_sql 会滤掉空段
                let parts = [
                    table.catalog.as_str(),
                    table.schema.as_str(),
                    table.name.as_str(),
                ];
                let sql = shared.insight_sample_sql(&conn_id, &parts);
                let label = format!("{}.{}.{}", table.catalog, table.schema, table.name);
                sources.push((SampleSource::new(conn_id, sql, label), table.name.clone()));
            }
            Picked::Draft(rel, abs) => match SampleSource::duckdb_file(abs, rel.clone()) {
                Ok(source) => sources.push((source, rel)),
                Err(e) => {
                    state
                        .borrow_mut()
                        .set_note(Some(format!("文件无法读取：{e}")));
                    window.refresh();
                    return;
                }
            },
            Picked::Analysis(table) => {
                let label = format!("analytics.{table}");
                let source =
                    SampleSource::on_duckdb(format!("SELECT * FROM \"{table}\""), label.clone());
                sources.push((source, label));
            }
            Picked::AnalysisGlobal(table) => {
                // 全局分析库是跨项目共用、只读；取样由持久层另开短命会话（`on_global`），
                // 结果搬进进程单例的登记版 `tmp_i_`，下游读法与其它来源一致。
                let label = format!("全局 · {table}");
                let source = SampleSource::on_global(table, label.clone());
                sources.push((source, label));
            }
            Picked::InsightSample(table) => {
                // 洞察产物已经在进程单例里：取样即「再取一份」，去重口径与单例读取一致
                let label = table.clone();
                let source =
                    SampleSource::on_duckdb(format!("SELECT * FROM \"{table}\""), label.clone());
                sources.push((source, label));
            }
            Picked::External(path) => {
                let label = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "外部文件".to_string());
                match SampleSource::duckdb_file(path, label.clone()) {
                    Ok(source) => sources.push((source, label)),
                    Err(e) => {
                        state
                            .borrow_mut()
                            .set_note(Some(format!("文件无法读取：{e}")));
                        window.refresh();
                        return;
                    }
                }
            }
            // 洞察不启用结果集来源；走到这里是内核状态异常，忽略即可。
            Picked::ResultSet(_) => {}
        }
    }

    // 来源去重：同一张表被点两次（或「项目资产库」与「洞察产物」指到同一张）时
    // 合成一个来源，否则面板会开出两个内容完全相同的标签页。按整条来源比对
    // （SQL + 连接 + 标签），不用 `dedup_by`——那只去掉**相邻**重复。
    let mut unique: Vec<(SampleSource, String)> = Vec::new();
    for (source, table_name) in sources {
        if !unique.iter().any(|(seen, _)| seen == &source) {
            unique.push((source, table_name));
        }
    }

    match unique.len() {
        0 => return,
        1 => {
            let (source, table_name) = unique.remove(0);
            shared.open_insight_source_table(source, table_name, cx);
        }
        n => {
            let all: Vec<SampleSource> = unique.into_iter().map(|(s, _)| s).collect();
            shared.open_insight_source_tables(all, format!("{n} 个来源"), cx);
        }
    }
    window.close_dialog(cx);
}
