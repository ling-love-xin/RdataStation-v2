//! 页签域（`panel/` 第六块）：值详情 / 统计 / 分组 / 引用 / 执行 SQL 各右栏页签。


use gpui_kit::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::table::TableDelegate;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tree::tree;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use crate::grid::cell::{classify_value, hex_lines, hex_to_bytes, ViewerMode};
use crate::ports::ResultRequest;
#[cfg(test)]
use crate::ports::RightTab;
use crate::store::ColumnKind;
use crate::ui;
use super::data::ResultPanel;

impl ResultPanel {

    pub(crate) fn render_value_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let Some((row, col)) = self.selection_pos else {
            return self.sidebar_empty("未选中单元格", cx);
        };
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let value = delegate.cell_text(row, col, cx);
        // 列名 / 类型：普通视图 = columns[col - 1]（col 含行号槽）；行号槽 / 转置
        // 给不出语义列名，标题只用位置
        let (name, type_name) = if col == 0 || self.transposed {
            (None, None)
        } else {
            let data_col = col - 1;
            (
                delegate.columns().get(data_col).cloned(),
                delegate
                    .column_types()
                    .get(data_col)
                    .and_then(|t| t.as_ref())
                    .map(|t| t.name.clone()),
            )
        };
        let title = match (&name, &type_name) {
            (Some(name), Some(type_name)) => format!("{name} · {type_name}"),
            (Some(name), None) => name.clone(),
            _ => "值".to_string(),
        };
        // 【#28-B4】位置标注：本地新增 / 复制行标「本地」，被删底层行标「已删除」；
        // 行号仍是视图行号（D6：不另造本地序号体系，身份靠标注与网格底色 / 删除线区分）。
        // 【2026-10-28 微调】第一行 = 值位置信息：行 + 列号（col 含行号槽，col==0 是行号
        // 槽本身，无列号可标；普通列标「第 N 列」）。
        let row_marker = {
            let base = if col == 0 {
                format!("第 {} 行", row + 1)
            } else {
                format!("第 {} 行 · 第 {} 列", row + 1, col)
            };
            if delegate.row_is_local(row) {
                format!("{base} · 本地")
            } else if delegate.is_deleted_row(row) {
                format!("{base} · 已删除")
            } else {
                base
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .debug_selector(|| "editor-result-value-tab".to_string())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px_2()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(title))
                    .child(SharedString::from(row_marker)),
            )
            .child({
                // 【值查看器分派】按列类型 + 值形态选内容形态：文本块 / JSON 树 / hex 转储 /
                // 图片占位（纯 render 层；JSON 树状态由事件路径 `rebuild_value_tree` 重建）
                let kind = if col == 0 {
                    ColumnKind::Unknown
                } else {
                    delegate.column_kind(col)
                };
                self.render_value_body(kind, &value, cx)
            })
            .into_any_element()
    }

    /// 【值查看器】内容区按分派模式渲染（纯 render 层；JSON 树解析在事件路径）
    fn render_value_body(&self, kind: ColumnKind, value: &str, cx: &App) -> AnyElement {
        match classify_value(kind, value) {
            ViewerMode::Text => div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .px_2()
                .py_1()
                .text_xs()
                .whitespace_normal()
                .child(SharedString::from(value))
                .into_any_element(),
            ViewerMode::Json => self.render_json_tree(cx),
            ViewerMode::Hex => self.render_hex_dump(value, cx),
            ViewerMode::Image { format, size } => {
                let theme = cx.theme();
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(SharedString::from(format!(
                        "图片预览：{format} · {size} B（解码待接线）"
                    )))
                    .into_any_element()
            }
        }
    }

    /// 【值查看器】JSON 展开树（`tree::Tree` 组件；状态由事件路径重建，这里只摆）
    fn render_json_tree(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let state = self.json_tree.clone();
        tree(&state, move |_ix, entry, selected, _window, _app| {
            // 行内结构：depth 缩进 + 展开箭头（folder 才画）+ 键值标签。
            // 展开 / 收起由 Tree 组件自身处理（点击 folder 条目切换）。
            let indent = rems(ui::JSON_TREE_INDENT_STEP * entry.depth() as f32);
            let arrow = if entry.is_folder() {
                if entry.is_expanded() { "▾" } else { "▸" }
            } else {
                ""
            };
            let item = entry.item();
            ListItem::new(ElementId::Name(item.id.clone()))
                .selected(selected)
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .pl(indent)
                        .child(
                            div()
                                .w(rems(ui::JSON_TREE_ARROW_WIDTH))
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(SharedString::from(arrow)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .child(SharedString::from(item.label.clone())),
                        ),
                )
        })
        .into_any_element()
    }

    /// 【值查看器】hex 转储（第一页 16 行 = 256 字节；render 成本有上界，翻页后续）
    fn render_hex_dump(&self, hex_text: &str, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let bytes = hex_to_bytes(hex_text);
        let lines = hex_lines(&bytes, 0, 16);
        div()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .px_2()
            .py_1()
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .children(
                lines
                    .into_iter()
                    .map(|line| div().child(SharedString::from(line))),
            )
            .into_any_element()
    }

    /// 【右栏】计算页（DBeaver「计算」）：选中列的 count / NULL / min / max / avg / sum。
    ///
    /// 作用列 = 网格选中格优先（`selection_pos`），列浏览器清单选中列（`selected_col`）兜底；
    /// 转置视图不给语义列。数据 = 列级访问器 `column_stats_for`（只取目标列取值，
    /// 不物化整行矩阵；纯函数口径与列头菜单弹层一致）。
    /// 数值统计只在列里有可解析数值样本时才填（文本 / BLOB 只计 count / NULL）。
    pub(crate) fn render_stats_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let Some(data_col) = self.inspect_column(delegate) else {
            return self.sidebar_empty("选中一个数据列查看统计", cx);
        };
        // 【P2-2】列级访问器：只取目标列取值，不再 `visible_rows()` 物化整行矩阵
        let stats = delegate.column_stats_for(data_col);
        let name = delegate
            .columns()
            .get(data_col)
            .cloned()
            .unwrap_or_else(|| "列".to_string());
        let items: Vec<(String, String)> = vec![
            ("非空".into(), stats.count.to_string()),
            ("NULL".into(), stats.nulls.to_string()),
            ("最小值".into(), fmt_stat(stats.min)),
            ("最大值".into(), fmt_stat(stats.max)),
            ("均值".into(), fmt_stat(stats.avg)),
            ("合计".into(), fmt_stat(stats.sum)),
        ];
        div()
            .flex_1()
            .min_h_0()
            .debug_selector(|| "editor-result-stats-tab".to_string())
            .child(
                div()
                    .px_2()
                    .py_1()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!("「{name}」统计"))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(items.into_iter().map(|(k, v)| kv_row(&k, &v, cx))),
            )
            .into_any_element()
    }

    /// 【右栏】分组页（DBeaver「分组」）：按选中列本地 groupby 计数。
    ///
    /// 作用列口径同计算页：网格选中格优先，清单选中列兜底。
    /// 纯函数 `group_by_count`（保留首现顺序，NULL 独立成组）；大表口径是
    /// 下发 GROUP BY 重查（见 §8.8），本地页只服务中小结果集的即时分组。
    pub(crate) fn render_group_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let Some(data_col) = self.inspect_column(delegate) else {
            return self.sidebar_empty("选中一个数据列查看分组", cx);
        };
        // 【P2-2】列级访问器：只取目标列取值，不再 `visible_rows()` 物化整行矩阵
        let groups = delegate.group_by_for(data_col);
        let name = delegate
            .columns()
            .get(data_col)
            .cloned()
            .unwrap_or_else(|| "列".to_string());
        let rows_el: Vec<AnyElement> = groups
            .iter()
            .map(|(value, count)| {
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .h_6()
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(SharedString::from(value.clone())),
                    )
                    .child(div().text_xs().text_color(muted).child(count.to_string()))
                    .into_any_element()
            })
            .collect();
        div()
            .flex_1()
            .min_h_0()
            .debug_selector(|| "editor-result-group-tab".to_string())
            .child(
                div()
                    .px_2()
                    .py_1()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!("按「{name}」分组 · {} 组", groups.len()))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(rows_el),
            )
            .into_any_element()
    }

    /// 【引用】外键引用查看页签（对齐 DBeaver 引用页签；backlog 空态）
    pub(crate) fn render_reference_tab(&self, cx: &App) -> AnyElement {
        self.sidebar_empty("选中一个列查看外键引用", cx)
    }

    /// 【1.2】「执行SQL」页签：筛选框运行后生成的 wrapped SQL 原文（只读 + 复制）。
    pub(crate) fn render_exec_sql_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let Some(sql) = self.exec_sql.clone() else {
            return self.sidebar_empty("在工具栏筛选框输入词并点 ▶ 运行，生成 SQL 会显示在这里", cx);
        };
        let copy_queue = self.queue.clone();
        div()
            .v_flex()
            .w_full()
            .h_full()
            .gap_2()
            .p_2()
            .debug_selector(|| "editor-result-exec-sql-tab".to_string())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(SharedString::from("生成的 SQL")),
                    )
                    .child(
                        Button::new("editor-result-exec-sql-copy")
                            .ghost()
                            .small()
                            .icon(IconName::Copy)
                            .label("复制")
                            .tooltip("复制生成的 SQL".to_string())
                            .on_click(move |_, _window, app| {
                                copy_queue.request(ResultRequest::CopyExecSql, app);
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .bg(theme.colors.background)
                    .p_2()
                    .rounded_md()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(SharedString::from(sql)),
            )
            .into_any_element()
    }

    /// 【侧栏】右栏各页共用的空态（居中灰字；页内容区 `flex_1`）
    pub(crate) fn sidebar_empty(&self, text: &str, cx: &App) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_1()
            .px_2()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(SharedString::from(text))
            .into_any_element()
    }


}



/// 【右栏】页签下标 → 枚举（TabBar 回调用；与 `RightTab` 定义顺序一致——
/// Browser=0 / Value=1 / Stats=2 / Group=3 / Reference=4 / ExecSql=5，
/// 对齐 DBeaver 右侧页签顺序）
/// 【2026-10-28 微调】生产横 tab 栏只显激活页签后，索引映射仅供测试用
#[cfg(test)]
pub(crate) fn right_tab_from_index(index: usize) -> RightTab {
    match index {
        1 => RightTab::Value,
        2 => RightTab::Stats,
        3 => RightTab::Group,
        4 => RightTab::Reference,
        5 => RightTab::ExecSql,
        _ => RightTab::Browser,
    }
}

/// 【右栏】键值行（计算页 / 列浏览器明细共用）：左标签右值，值超长截断防挤
pub(crate) fn kv_row(label: &str, value: &str, cx: &App) -> AnyElement {
    let muted = cx.theme().colors.muted_foreground;
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .px_2()
        .h_5()
        .text_xs()
        .child(
            div()
                .w(rems(ui::RESULT_SIDEBAR_KV_LABEL_WIDTH))
                .flex_none()
                .text_color(muted)
                .child(SharedString::from(label)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .child(SharedString::from(value)),
        )
        .into_any_element()
}

/// 【右栏】统计值格式化：整数样去尾零（1.0 → 1），否则保留两位；无数值样本 = —
pub(crate) fn fmt_stat(value: Option<f64>) -> String {
    value
        .map(|v| {
            if v.fract() == 0.0 {
                format!("{}", v as i64)
            } else {
                format!("{:.2}", v)
            }
        })
        .unwrap_or_else(|| "—".to_string())
}