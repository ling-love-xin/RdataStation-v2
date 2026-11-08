//! 结果分组与上限：按模式组装各组行、空组不出现、超上限截断记账。

use super::*;
use super::scoring::{filter_ranked, rank_row};
use super::sources::{connection_rows, meta_key};

/// 命令目录转发：目录本体在 [`crate::quick_open::commands`]（那里有 id / keywords / 动作），
/// 本模块只做「行 + 匹配」这一层（`filter_ranked` 要认识 `match_text`）。
use crate::quick_open::commands::command_rows;

/// 组装结果分组：**空组不出现**（组头也不渲染）。
///
/// 元数据（跨连接名称档）由宿主从后台回填后以 `meta` 传入；本函数是纯函数，不做 I/O。
/// 元数据是核心：搜索中也要出组头（否则「搜不到」与「还在搜」分不清）。
pub fn build_groups(q: &Query, sources: &Sources<'_>) -> Vec<Group> {
    let groups = match q.mode {
        Mode::Command => non_empty("命令", filter_ranked(command_rows(), &q.needle), "")
            .into_iter()
            .collect(),
        // Phase 1：内容档（FTS）——引擎侧已完成匹配与排序，这里**不再按标题过滤**：
        // 否则「只在注释里命中」的行会被「标题不含该词」误杀。`why` 标签区分两者。
        Mode::FullText => {
            let note = if sources.meta_searching && q.async_ready() {
                "搜索中…"
            } else {
                ""
            };
            non_empty(
                "元数据全文（注释 / 定义）",
                fulltext_rows(sources.meta, &q.needle),
                note,
            )
            .into_iter()
            .collect()
        }
        Mode::Goto => {
            let note = if sources.meta_searching && q.async_ready() {
                "搜索中…"
            } else {
                ""
            };
            non_empty(
                "跳转到对象（表 · 视图 · 例程 · 序列 · 触发器）",
                goto_rows(sources.meta, &q.needle),
                note,
            )
            .into_iter()
            .collect()
        }
        Mode::Default => {
            let mut groups = Vec::new();
            let note = if sources.meta_searching && q.async_ready() {
                "搜索中…"
            } else {
                ""
            };
            groups.extend(non_empty(
                "元数据（表 · 视图 · 列）",
                filter_ranked(meta_rows(sources.meta), &q.needle),
                note,
            ));
            // 文件排在元数据之后、连接之前（原型设计 §7.2 的固定顺序）：
            // 对象是「查数库」的主线，文件是「手边的草稿」。
            groups.extend(non_empty(
                "文件（草稿箱）",
                filter_ranked(file_rows(sources.files), &q.needle),
                &sources.files_note,
            ));
            groups.extend(non_empty(
                "连接",
                filter_ranked(connection_rows(sources.connections), &q.needle),
                "",
            ));
            groups.extend(non_empty(
                "命令",
                filter_ranked(command_rows(), &q.needle),
                "",
            ));
            groups
        }
    };
    cap_groups(groups, q.mode)
}

fn non_empty(title: &'static str, rows: Vec<Row>, note: &str) -> Option<Group> {
    if rows.is_empty() {
        None
    } else {
        Some(Group {
            title,
            note: note.to_string(),
            rows,
            hidden: 0,
        })
    }
}

/// 施加渲染上限：默认档单组 ≤ `QUICK_OPEN_MAX_ROWS_PER_GROUP`、总计 ≤ `QUICK_OPEN_MAX_ROWS`；
/// Goto 档单组 / 总计 ≤ `QUICK_OPEN_GOTO_MAX_ROWS`（跳转对话框面向全库对象，预算比默认档宽）。
///
/// 被截的条数记在 `hidden` 上（组尾一行「还有 N 条」），**不静默丢**：
/// 后台侧另有上限（单连接 50 / 总计 300），那是防止把渲染拖垮的硬闸，
/// 这里是呈现层的闸，两者都不是「结果少」的借口。
fn cap_groups(mut groups: Vec<Group>, mode: Mode) -> Vec<Group> {
    let (budget_cap, per_group) = if mode == Mode::Goto {
        (
            workbench_shell::ui::QUICK_OPEN_GOTO_MAX_ROWS,
            workbench_shell::ui::QUICK_OPEN_GOTO_MAX_ROWS,
        )
    } else {
        (
            workbench_shell::ui::QUICK_OPEN_MAX_ROWS,
            workbench_shell::ui::QUICK_OPEN_MAX_ROWS_PER_GROUP,
        )
    };
    let mut budget = budget_cap;
    for group in &mut groups {
        let allowed = budget.min(per_group).min(group.rows.len());
        group.hidden = group.rows.len() - allowed;
        group.rows.truncate(allowed);
        budget = budget.saturating_sub(allowed);
    }
    groups
}

/// 文件行（次级信息 = 所在目录；没有目录的（模块根下）留空）。
///
/// 匹配面用**相对路径**而不是文件名：敲 `drafts/notes` 或只敲目录名也能命中
/// （高亮仍按文件名算——见 `Row::match_text`）。
fn file_rows(files: &[FileObject]) -> Vec<Row> {
    files
        .iter()
        .map(|file| Row {
            key: format!("file:{}", file.relative_path),
            kind: RowKind::File,
            title: file.name.clone(),
            match_text: file.relative_path.clone(),
            secondary: file.folder.clone(),
            snippet: None,
            why: None,
            locate: None,
            action: Action::OpenDocument(file.path.clone()),
            secondary_action: None,
        })
        .collect()
}

/// 元数据行（次级信息 = 连接名 · 驱动）。
fn meta_rows(meta: &[MetaObject]) -> Vec<Row> {
    meta.iter()
        .map(|m| Row {
            key: meta_key(&m.object),
            kind: m.kind,
            title: m.title.clone(),
            match_text: m.title.clone(),
            secondary: format!("{} · {}", m.request.conn_label, m.request.driver),
            snippet: None,
            why: None,
            locate: Some(m.object.clone()),
            action: Action::ShowProperties(Box::new(m.request.clone())),
            secondary_action: None,
        })
        .collect()
}

/// Goto 档元数据行：**不含列**（列是表的从属细节，跳转对话框面向可独立寻址的对象）；
/// 主动作 = 回树定位（`RevealMetadata`），次级动作 = 属性面板（`ShowProperties`，`Alt+Enter`）。
///
/// 排序不用默认档的匹配评分档：跳转场景按限定名字典序排列更符合「按名字找对象」的预期，
/// 命中过滤仍走 `rank_row`（词不达意的行不出现），排序由 [`sort_goto`] 接管。
fn goto_rows(meta: &[MetaObject], needle: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = meta
        .iter()
        .filter(|m| m.kind != RowKind::Column)
        .map(|m| Row {
            key: meta_key(&m.object),
            kind: m.kind,
            title: m.title.clone(),
            match_text: m.title.clone(),
            secondary: format!("{} · {}", m.request.conn_label, m.request.driver),
            snippet: None,
            why: None,
            locate: Some(m.object.clone()),
            action: Action::RevealMetadata(m.object.clone()),
            secondary_action: Some(Action::ShowProperties(Box::new(m.request.clone()))),
        })
        .collect();
    rows.retain(|row| rank_row(row, needle).is_some());
    sort_goto(&mut rows);
    rows
}

/// Goto 档排序：限定名 ignoreCase 字典序；同名并列时按类别序兜底（表 → 视图 → 例程 →
/// 序列 → 触发器 → 模式），最后再按原文标题稳定兜底。
fn sort_goto(rows: &mut [Row]) {
    rows.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| goto_kind_order(a.kind).cmp(&goto_kind_order(b.kind)))
            .then_with(|| a.title.cmp(&b.title))
    });
}

/// Goto 档类别兜底顺序（数值小者靠前）；列已在 [`goto_rows`] 过滤，其余未列类别排最后。
fn goto_kind_order(kind: RowKind) -> u8 {
    match kind {
        RowKind::Table => 0,
        RowKind::View => 1,
        RowKind::Routine => 2,
        RowKind::Sequence => 3,
        RowKind::Trigger => 4,
        RowKind::Schema => 5,
        _ => 9,
    }
}

/// 内容档行：多带 snippet（含命中标记）与「为什么命中」标签。
///
/// 不按标题过滤：引擎已经把「注释 / 数据类型命中」的对象排好了，标题过滤会把它们错杀。
fn fulltext_rows(meta: &[MetaObject], needle: &str) -> Vec<Row> {
    let needle_lower = needle.to_lowercase();
    meta.iter()
        .map(|m| {
            let matched_name =
                !needle_lower.is_empty() && m.title.to_lowercase().contains(&needle_lower);
            Row {
                key: meta_key(&m.object),
                kind: m.kind,
                title: m.title.clone(),
                match_text: m.title.clone(),
                secondary: format!("{} · {}", m.request.conn_label, m.request.driver),
                snippet: m.snippet.clone(),
                why: Some(if matched_name { "名称" } else { "内容" }),
                locate: Some(m.object.clone()),
                action: Action::ShowProperties(Box::new(m.request.clone())),
                secondary_action: None,
            }
        })
        .collect()
}
