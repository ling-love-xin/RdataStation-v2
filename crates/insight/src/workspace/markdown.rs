//! Markdown 报告导出（表画像 → 可粘贴的 Markdown 文本）。

use super::*;


/// 导出报告正文（Markdown）：与中间页渲染**同一份事实**（D36——导出的是
/// 用户看到的这份结论，不重算）。概览 → 检查器命中 → 值得关注 → 列明细 →
/// 样本；表格单元格做 `|` 转义，避免 Markdown 表格串列。
pub(crate) fn render_markdown_report(profile: &TableProfileView) -> String {
    let mut md = String::new();
    md.push_str(&format!("# 洞察报告：{}\n\n", profile.table_name));
    md.push_str(&format!(
        "- 来源：{}\n- 行数：{}\n- 列数：{}\n\n",
        profile.source_label,
        fmt_rows(profile.row_count),
        profile.columns.len()
    ));
    // 检查器命中汇总（评估后才有；未评估不写这一节，避免「0 命中」误导）
    if let Some(q) = &profile.quality {
        let hits = q
            .checker
            .iter()
            .filter(|c| c.state == ColumnCheckState::Hit)
            .count();
        let applicable = q
            .checker
            .iter()
            .filter(|c| c.state != ColumnCheckState::NotApplicableType)
            .count();
        let misses = q
            .checker
            .iter()
            .filter(|c| c.state == ColumnCheckState::Miss)
            .count();
        md.push_str(&format!(
            "## 检查器命中\n\n{hits}/{applicable} 命中，其中未命中 {misses} 条。\n\n"
        ));
    }
    // 值得关注：检查器未命中 / 缺失 >20% / 高基数（非主键）/ 类型未识别
    let mut worth: Vec<String> = Vec::new();
    if let Some(q) = &profile.quality {
        let mut by_col: Vec<(&str, usize)> = Vec::new();
        for check in &q.checker {
            if check.state == ColumnCheckState::Miss {
                if let Some((_, n)) = by_col.iter_mut().find(|(c, _)| *c == check.column.as_str()) {
                    *n += 1;
                } else {
                    by_col.push((check.column.as_str(), 1));
                }
            }
        }
        by_col.sort_by(|a, b| b.1.cmp(&a.1));
        for (column, n) in by_col.iter().take(3) {
            worth.push(format!("{column} 检查器未命中 {n} 条"));
        }
    }
    for col in profile
        .columns
        .iter()
        .filter(|c| c.null_rate.map(|n| n > 0.2).unwrap_or(false))
        .take(3)
    {
        worth.push(format!(
            "{} 缺失率 {:.0}%",
            col.name,
            col.null_rate.unwrap_or(0.0) * 100.0
        ));
    }
    for col in profile
        .columns
        .iter()
        .filter(|c| !c.primary_key && c.unique_rate.map(|u| u > 0.9).unwrap_or(false))
        .take(3)
    {
        worth.push(format!(
            "{} 高基数（唯一率 {:.0}%）",
            col.name,
            col.unique_rate.unwrap_or(0.0) * 100.0
        ));
    }
    for col in profile
        .columns
        .iter()
        .filter(|c| c.kind == ColumnKind::Unknown)
        .take(3)
    {
        worth.push(format!("{} 类型未识别（{}）", col.name, col.data_type));
    }
    if !worth.is_empty() {
        md.push_str("## 值得关注\n\n");
        for line in worth {
            md.push_str(&format!("- {line}\n"));
        }
        md.push('\n');
    }
    // 列明细表：检查器列给出「命中/未命中/不适用」统计
    md.push_str("## 列明细\n\n| 列 | 类型 | 缺失% | 唯一% | 检查器 |\n|---|---|---|---|---|\n");
    for col in &profile.columns {
        let checker_text =
            column_checkers(profile, col)
                .iter()
                .fold((0usize, 0usize), |(hit, miss), c| match c.state {
                    ColumnCheckState::Hit => (hit + 1, miss),
                    ColumnCheckState::Miss => (hit, miss + 1),
                    _ => (hit, miss),
                });
        let checker_cell = if profile.quality.is_some() {
            format!("{}/{}", checker_text.0, checker_text.0 + checker_text.1)
        } else {
            "—".into()
        };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            escape_md(&col.name),
            escape_md(&col.data_type),
            col.null_rate
                .map(|n| format!("{:.0}", n * 100.0))
                .unwrap_or_else(|| "—".into()),
            col.unique_rate
                .map(|u| format!("{:.0}", u * 100.0))
                .unwrap_or_else(|| "—".into()),
            checker_cell,
        ));
    }
    // 样本（前 5 行）
    if let Some(sample) = &profile.sample {
        md.push_str("\n## 样本（前 5 行）\n\n|");
        for c in &sample.columns {
            md.push_str(&format!(" {} |", escape_md(c)));
        }
        md.push('\n');
        for _ in &sample.columns {
            md.push_str("---|");
        }
        md.push('\n');
        for row in &sample.rows {
            md.push('|');
            for v in row {
                md.push_str(&format!(" {} |", escape_md(v)));
            }
            md.push('\n');
        }
        md.push('\n');
    }
    md
}

/// Markdown 转义：`|` 换成全角竖线（表格 cell 不串列），换行改空格。
pub(crate) fn escape_md(text: &str) -> String {
    text.replace('|', "｜").replace(['\r', '\n'], " ")
}

/// 文本 / 时间分布条（Top 值、月度分布）：把 `TextFrequency` 转成占比条。
/// 条比例 = 该项计数 / 总计数（占比语义与数值直方图一致）。
pub(crate) fn dist_text_bars(
    theme: &Theme,
    freqs: &[crate::model::types::TextFrequency],
    limit: usize,
) -> Div {
    let total: u64 = freqs.iter().map(|f| f.count as u64).sum();
    let bins: Vec<crate::model::types::DistributionBin> = freqs
        .iter()
        .map(|f| crate::model::types::DistributionBin {
            label: f.value.clone(),
            count: f.count,
            ratio: if total > 0 {
                f.count as f64 / total as f64
            } else {
                0.0
            },
        })
        .collect();
    dist_bars(theme, &bins, limit)
}

/// 多列规则结果（关联性区共用）：Single = 键值行，Table = 表格。
pub(crate) fn render_multi_result(result: &MultiResultView, theme: &Theme) -> Div {
    let colors = theme.colors;
    match result {
        MultiResultView::Single(rows) => {
            let mut body = div().v_flex().w_full().gap_1();
            for row in rows {
                body = body.child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                .flex_none()
                                .text_xs()
                                .text_ellipsis()
                                .text_color(colors.muted_foreground)
                                .child(row.label.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(colors.foreground)
                                .child(row.value.clone()),
                        ),
                );
            }
            panel_box(theme, "规则输出", String::new(), body)
        }
        MultiResultView::Table { headers, rows } => {
            let mut head = div().h_flex().w_full().gap_2().px_2().py_1();
            for name in headers {
                head = head.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_ellipsis()
                        .text_color(colors.foreground)
                        .child(name.clone()),
                );
            }
            let mut body = div().v_flex().w_full();
            for row in rows {
                let mut line = div()
                    .h_flex()
                    .w_full()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .border_t_1()
                    .border_color(colors.border);
                for cell in row {
                    line = line.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(colors.foreground)
                            .child(cell.clone()),
                    );
                }
                body = body.child(line);
            }
            panel_box(
                &theme,
                "规则输出",
                String::new(),
                div().v_flex().w_full().child(head).child(body),
            )
        }
    }
}
