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

use crate::components::source_picker::{
    self, PickerConfig, PickerState, SourceKind,
};
use crate::panels::Shared;

/// 洞察启用且按用户拍板顺序排列的来源（无结果集）。
const INSIGHT_KINDS: &[SourceKind] = &[
    SourceKind::Connected,
    SourceKind::Scratchpad,
    SourceKind::Analysis,
    SourceKind::External,
];

/// 打开「新建洞察」对话框（宿主侧入口，`insight_actions` 订阅 `NewInsightRequested` 后调用）。
///
/// 连接列表、草稿 / 分析表的预载与提示行生成都在内核 `open_source_picker` 里完成。
pub fn open_new_insight_dialog(shared: &Shared, window: &mut Window, cx: &mut App) {
    let config = PickerConfig {
        title: "新建洞察",
        kinds: INSIGHT_KINDS,
        confirm_prefix: "分析",
    };
    let strategy = Rc::new(
        |shared: &Shared, state: &Rc<RefCell<PickerState>>, window: &mut Window, cx: &mut App| {
            confirm(shared, state, window, cx);
        },
    );
    source_picker::open_source_picker(shared, config, strategy, window, cx);
}

/// 洞察物化策略：按来源构造取样来源并**双栏同时**打开，然后关闭对话框。
///
/// 失败（文件无法读取）写回底部提示并保持对话框，不关闭。
fn confirm(
    shared: &Shared,
    state: &Rc<RefCell<PickerState>>,
    window: &mut Window,
    cx: &mut App,
) {
    let s = state.borrow();
    match s.current_tab() {
        SourceKind::Connected => {
            // 表候选与三层浏览都在内核；此处经公开访问器取选中目标。
            let Some((conn_id, table)) = s.picked_table() else { return };
            // 无 schema 层时 schema 为空串：insight_sample_sql 会滤掉空段
            let parts = [table.catalog.as_str(), table.schema.as_str(), table.name.as_str()];
            let sql = shared.insight_sample_sql(&conn_id, &parts);
            let label = format!("{}.{}.{}", table.catalog, table.schema, table.name);
            let source = SampleSource::new(conn_id, sql, label);
            shared.open_insight_source_table(source, table.name.clone(), cx);
        }
        SourceKind::Scratchpad => {
            let Some((rel, abs)) = s.picked_draft() else { return };
            match SampleSource::duckdb_file(abs.clone(), rel.clone()) {
                Ok(source) => {
                    shared.open_insight_source_table(source, rel.clone(), cx);
                }
                Err(e) => {
                    state.borrow_mut().set_note(Some(format!("文件无法读取：{e}")));
                    window.refresh();
                    return;
                }
            }
        }
        SourceKind::Analysis => {
            let Some(table) = s.picked_analysis() else { return };
            let label = format!("analytics.{table}");
            let source = SampleSource::on_duckdb(
                format!("SELECT * FROM \"{table}\""),
                label.clone(),
            );
            shared.open_insight_source_table(source, label, cx);
        }
        SourceKind::External => {
            let Some(path) = s.picked_external() else { return };
            let label = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "外部文件".to_string());
            match SampleSource::duckdb_file(path.clone(), label.clone()) {
                Ok(source) => shared.open_insight_source_table(source, label, cx),
                Err(e) => {
                    state.borrow_mut().set_note(Some(format!("文件无法读取：{e}")));
                    window.refresh();
                    return;
                }
            }
        }
        SourceKind::ResultSet => {
            // 洞察不启用结果集来源；走到这里是内核状态异常，忽略即可。
        }
    }
    window.close_dialog(cx);
}
