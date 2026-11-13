//! 行构造：把各数据源**已有的取数结果**转成 `Row` 列表。
//!
//! 这一层刻意不碰渲染、也不碰数据加载，只做「数据 → 行」的纯映射，因而可单测；
//! 下一步 `render_rows` 只负责把行画出来。这样视觉改造与取数可以各切一刀。
//!
//! 深度约定（`06-object-tree.html` §01：深度上限 3）：
//! ```text
//! 文件(0) → 草稿箱(1) → 数据(2) → a.parquet(3)
//! ```
//! 超过上限的目录层不再成行，其余下的文件压成一行，`label` 用剩余相对路径，
//! `breadcrumb` 给出被折叠的那几层——否则用户在树上看到的路径与文件管理器不一致。
//!
//! 身份约定：文件行的 `NodeId::Path` 是**原始相对路径**（`collect_drafts()` 给的那个串），
//! 确认时调用方拿它去 `drafts` 表反查绝对路径；只有目录行用规范化后的 `\` 连接键
//! （目录不可确认，键只用于展开态与选中）。

use std::path::{Path, PathBuf};

use super::group::{grouped, SourceGroup};
use super::node::{ArchiveScope, NodeId, NodeKind, Row, MAX_DEPTH};
use super::SourceKind;

/// 三个根的组行（depth 0）。顺序与 `grouped` 一致，空组不出现。
pub fn root_rows(kinds: &[SourceKind]) -> Vec<Row> {
    grouped(kinds)
        .into_iter()
        .map(|(group, _)| Row::new(group_id(group), 0, NodeKind::Group, group.title()))
        .collect()
}

/// 根组的稳定身份。用手写前缀而非 `Debug`，避免改枚举名时悄悄改掉身份。
fn group_id(group: SourceGroup) -> NodeId {
    let key = match group {
        SourceGroup::Database => "source-group:database",
        SourceGroup::DuckDb => "source-group:duckdb",
        SourceGroup::File => "source-group:file",
    };
    NodeId::Path(key.to_string())
}

/// 草稿箱子树。`base_depth` = 「草稿箱」那一行所在的深度。
///
/// 入参就是 `collect_drafts()` 的产物（相对路径 → 绝对路径，已按格式过滤并递归拍平）。
pub fn draft_rows(base_depth: u8, drafts: &[(String, PathBuf)]) -> Vec<Row> {
    debug_assert!(base_depth < MAX_DEPTH, "草稿箱的行不可能已在深度上限上");
    let mut tree: Vec<BuildNode> = Vec::new();
    for (rel, _abs) in drafts {
        let segs = normalize_segments(rel);
        if !segs.is_empty() {
            insert(&mut tree, &segs, rel);
        }
    }
    let mut rows = Vec::new();
    emit(&tree, base_depth + 1, "", &mut rows);
    rows
}

/// 资产库「分析表」段（§05：读 `information_schema.tables`，限定 `main` schema）。
///
/// `depth` 由调用方给：本项目段与全局段并列，深度相同、`scope` 不同。
pub fn analysis_table_rows(depth: u8, tables: &[String], scope: ArchiveScope) -> Vec<Row> {
    debug_assert!(depth <= MAX_DEPTH);
    let mut rows: Vec<Row> = tables
        .iter()
        .map(|t| {
            Row::new(
                NodeId::AnalysisTable {
                    scope,
                    table: t.clone(),
                },
                depth,
                NodeKind::Table,
                t.clone(),
            )
        })
        .collect();
    rows.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then_with(|| a.label.cmp(&b.label))
    });
    rows
}

/// 「外部文件 · 已引用」的单行（一次性引用，文件可能已被移走）。
pub fn external_row(depth: u8, path: &Path) -> Row {
    debug_assert!(depth <= MAX_DEPTH);
    let label = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    let mut row = Row::new(
        NodeId::Path(path.to_string_lossy().to_string()),
        depth,
        NodeKind::File,
        label,
    );
    if let Some(parent) = path.parent() {
        let crumb = parent.to_string_lossy().to_string();
        if !crumb.is_empty() {
            row = row.with_breadcrumb(crumb);
        }
    }
    row
}

// ---- 内部：草稿路径树的构建与拍平 --------------------------------------

struct BuildNode {
    name: String,
    /// 文件行的原始相对路径（目录行为 `None`）
    original: Option<String>,
    children: Vec<BuildNode>,
}

impl BuildNode {
    fn is_file(&self) -> bool {
        self.original.is_some()
    }
}

/// 草稿相对路径的段落：平台分隔符 `/` 与 `\` 都认（`ScratchpadEntry.path` 是 `PathBuf`）。
fn normalize_segments(rel: &str) -> Vec<String> {
    rel.split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn insert(list: &mut Vec<BuildNode>, segs: &[String], original: &str) {
    let name = &segs[0];
    let idx = match list.iter().position(|n| n.name == *name) {
        Some(i) => i,
        None => {
            list.push(BuildNode {
                name: name.clone(),
                original: None,
                children: Vec::new(),
            });
            list.len() - 1
        }
    };
    if segs.len() == 1 {
        list[idx].original = Some(original.to_string());
    } else {
        insert(&mut list[idx].children, &segs[1..], original);
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}\\{name}")
    }
}

fn emit(list: &[BuildNode], depth: u8, rel_prefix: &str, rows: &mut Vec<Row>) {
    let mut ordered: Vec<&BuildNode> = list.iter().collect();
    // 目录在前、同类按名称字典序（大小写不敏感），保证每次打开顺序一致
    ordered.sort_by(|a, b| {
        a.is_file()
            .cmp(&b.is_file())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });

    for node in ordered {
        let rel = join(rel_prefix, &node.name);
        match node.original.as_ref() {
            Some(original) => {
                // 叶子：走原始相对路径作为身份
                rows.push(file_row(original.clone(), &node.name, depth, rel_prefix));
            }
            None if depth >= MAX_DEPTH => {
                // 已到深度上限：这一层目录不再成行，其下文件压成一行
                for (original, label) in folded_files(node, &rel) {
                    rows.push(file_row(original, &label, depth, &rel));
                }
            }
            None => {
                let mut row = Row::new(
                    NodeId::Path(rel.clone()),
                    depth,
                    NodeKind::Folder,
                    node.name.clone(),
                );
                let crumb = breadcrumb(rel_prefix);
                if !crumb.is_empty() {
                    row = row.with_breadcrumb(crumb);
                }
                rows.push(row);
                emit(&node.children, depth + 1, &rel, rows);
            }
        }
    }
}

fn file_row(original: String, label: &str, depth: u8, breadcrumb_from: &str) -> Row {
    debug_assert!(depth <= MAX_DEPTH);
    let mut row = Row::new(NodeId::Path(original), depth, NodeKind::File, label.to_string());
    let crumb = breadcrumb(breadcrumb_from);
    if !crumb.is_empty() {
        row = row.with_breadcrumb(crumb);
    }
    row
}

/// 面包屑文本：`数据\sub` → `数据 / sub`
fn breadcrumb(rel: &str) -> String {
    rel.split('\\')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// 被折叠目录下的全部文件：`(原始相对路径, 剩余相对标签)`。
fn folded_files(node: &BuildNode, prefix: &str) -> Vec<(String, String)> {
    fn walk(n: &BuildNode, prefix: &str, label: &str, out: &mut Vec<(String, String)>) {
        for child in &n.children {
            let rel = join(prefix, &child.name);
            let sub_label = if label.is_empty() {
                child.name.clone()
            } else {
                format!("{label}\\{}", child.name)
            };
            match child.original.as_ref() {
                Some(original) => out.push((original.clone(), sub_label)),
                None => walk(child, &rel, &sub_label, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(node, prefix, "", &mut out);
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(rel: &str) -> (String, PathBuf) {
        (
            rel.to_string(),
            PathBuf::from(format!("C:/proj/scratchpad/{rel}")),
        )
    }

    #[test]
    fn root_rows_follow_group_order_and_skip_empty_groups() {
        let rows = root_rows(&[SourceKind::Connected, SourceKind::Scratchpad]);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["数据库", "文件"], "无 DuckDB 叶子时不出现空组");
        assert!(rows
            .iter()
            .all(|r| r.kind == NodeKind::Group && !r.is_confirmable()));
        assert!(rows.iter().all(|r| r.depth == 0));
    }

    #[test]
    fn single_file_sits_one_level_below_the_group() {
        let rows = draft_rows(1, &[draft("a.parquet")]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "a.parquet");
        assert_eq!(rows[0].depth, 2);
        assert_eq!(rows[0].kind, NodeKind::File);
        assert!(rows[0].is_confirmable());
        assert!(rows[0].breadcrumb.is_none(), "直接在草稿箱下，没有上级面包屑");
        assert_eq!(
            rows[0].id,
            NodeId::Path("a.parquet".into()),
            "文件行用原始相对路径做身份，确认时据此反查绝对路径"
        );
    }

    #[test]
    fn nested_folder_appears_and_file_gets_breadcrumb() {
        let rows = draft_rows(1, &[draft("数据\\a.parquet")]);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["数据", "a.parquet"]);
        assert_eq!(rows[0].kind, NodeKind::Folder);
        assert_eq!(rows[1].breadcrumb.as_deref(), Some("数据"));
    }

    #[test]
    fn beyond_depth_limit_is_folded_into_one_row() {
        // 草稿箱(1) → 数据(2) → sub(3) 已到上限，文件不能有第 4 层
        let rows = draft_rows(1, &[draft("数据\\sub\\a.parquet")]);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["数据", "a.parquet"]);
        assert_eq!(rows[1].depth, MAX_DEPTH, "文件压在深度上限那一层");
        assert_eq!(
            rows[1].breadcrumb.as_deref(),
            Some("数据 / sub"),
            "被折叠的层要在面包屑里交代清楚"
        );
        assert!(rows.iter().all(|r| r.depth <= MAX_DEPTH), "不得突破深度上限");
    }

    #[test]
    fn same_folder_is_reused_across_files() {
        let rows = draft_rows(1, &[draft("数据\\a.parquet"), draft("数据\\b.parquet")]);
        let folders: Vec<&Row> = rows
            .iter()
            .filter(|r| r.kind == NodeKind::Folder)
            .collect();
        assert_eq!(folders.len(), 1, "同一目录只出现一次");
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn folders_precede_files_and_order_is_stable() {
        let rows = draft_rows(
            1,
            &[draft("b.parquet"), draft("A\\x.parquet"), draft("a.parquet")],
        );
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["A", "x.parquet", "a.parquet", "b.parquet"]);
    }

    #[test]
    fn both_separators_are_understood() {
        let rows = draft_rows(1, &[draft("数据/sub/a.parquet")]);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["数据", "a.parquet"]);
        assert_eq!(
            rows[1].id,
            NodeId::Path("数据/sub/a.parquet".into()),
            "身份保留原始串，不做分隔符改写"
        );
    }

    #[test]
    fn analysis_tables_are_sorted_and_scoped() {
        let project = analysis_table_rows(
            3,
            &["orders".to_string(), "customers".to_string()],
            ArchiveScope::Project,
        );
        let labels: Vec<&str> = project.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["customers", "orders"]);
        assert!(project.iter().all(|r| r.kind == NodeKind::Table));

        // 同名表分属两个库：项目副本与全局原表必须能区分
        let global = analysis_table_rows(3, &["orders".to_string()], ArchiveScope::Global);
        assert_ne!(
            global[0].id, project[1].id,
            "同名分析表在两个库里不能撞身份"
        );
    }

    #[test]
    fn global_table_keeps_g_prefix_verbatim() {
        let global = analysis_table_rows(3, &["G_orders".to_string()], ArchiveScope::Global);
        assert_eq!(global[0].label, "G_orders", "全局表直接显示 G_ 前缀，不另加符号");
    }

    #[test]
    fn external_row_uses_file_name_as_label() {
        let row = external_row(3, Path::new("C:/data/sales.parquet"));
        assert_eq!(row.label, "sales.parquet");
        assert_eq!(row.kind, NodeKind::File);
        assert_eq!(row.breadcrumb.as_deref(), Some("C:/data"));
    }
}
