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
use super::node::{ArchiveScope, NodeId, NodeKind, Row, TempScope, PICKER_MAX_DEPTH};
use super::SourceKind;

/// 三个根的组行（depth 0）。顺序与 `grouped` 一致，空组不出现。
pub fn root_rows(kinds: &[SourceKind]) -> Vec<Row> {
    grouped(kinds)
        .into_iter()
        .map(|(group, _)| group_id_row(group))
        .collect()
}

/// 单个根的根行（depth 0）。
///
/// 三分法主体逐根拼子树时用：`grouped` 一次只能给全部分组，而主体需要
/// 「根行 + 该根子树」严格交替，否则 `TreeState::flatten` 的「父行在前」假设就破了。
pub fn group_id_row(group: SourceGroup) -> Row {
    Row::new(group_id(group), 0, NodeKind::Group, group.title())
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

/// 草稿箱子树。`file_depth` = 文件层所在的深度。
///
/// 树里有「文件(0) → 草稿箱(1) → 数据(2) → a.parquet(3)」这一层，草稿箱这一行由数据源
/// 提供（不是本函数产的），所以文件层深度必须由调用方给，不能用「父行 + 1」推。
///
/// 入参就是 `collect_drafts()` 的产物（相对路径 → 绝对路径，已按格式过滤并递归拍平）。
pub fn draft_rows(file_depth: u8, drafts: &[(String, PathBuf)]) -> Vec<Row> {
    debug_assert!(file_depth <= PICKER_MAX_DEPTH, "文件层已在深度上限之上");
    let mut tree: Vec<BuildNode> = Vec::new();
    for (rel, _abs) in drafts {
        let segs = normalize_segments(rel);
        if !segs.is_empty() {
            insert(&mut tree, &segs, rel);
        }
    }
    let mut rows = Vec::new();
    emit(&tree, file_depth, "", &mut rows);
    rows
}

/// 资产库「分析表」段（§05：读 `information_schema.tables`，限定 `main` schema）。
///
/// `depth` 由调用方给：本项目段与全局段并列，深度相同、`scope` 不同。
pub fn analysis_table_rows(depth: u8, tables: &[String], scope: ArchiveScope) -> Vec<Row> {
    debug_assert!(depth <= PICKER_MAX_DEPTH);
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

/// 持久表在界面上的可读名：`res_订单明细_20261115103000` → `订单明细 · 2026-11-15 10:30:00`。
///
/// 库里名字必须唯一且可排序，所以带时间戳；但树里给人看的是另一回事——一串时间戳
/// 挤满一屏，用户认不出哪张是刚才存的那份。**身份仍是库里的原名**（`NodeId` 不动），
/// 这里只换显示名；解析失败（没时间戳 / 描述为空）就原样回原名，不猜。
pub fn persistent_table_label(name: &str) -> String {
    let Some(body) = name.strip_prefix(engine::duckdb::temp_table::promote::PERSIST_PREFIX) else {
        return name.to_string();
    };
    let Some((desc, stamp)) = body.rsplit_once('_') else {
        return name.to_string();
    };
    let desc = desc.trim();
    if desc.is_empty() || stamp.len() != 14 || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return name.to_string();
    }
    let (date, time) = stamp.split_at(8);
    let (y, m, d) = (&date[0..4], &date[4..6], &date[6..8]);
    let (hh, mm, ss) = (&time[0..2], &time[2..4], &time[4..6]);
    format!("{desc} · {y}-{m}-{d} {hh}:{mm}:{ss}")
}

/// 持久表行：身份是库里的原名，显示名走 [`persistent_table_label`]。
pub fn persistent_table_rows(depth: u8, tables: &[String], scope: ArchiveScope) -> Vec<Row> {
    debug_assert!(depth <= PICKER_MAX_DEPTH);
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
                persistent_table_label(t),
            )
            .with_meta("持久表")
        })
        .collect();
    rows.sort_by(|a, b| b.label.cmp(&a.label).then_with(|| a.label.cmp(&b.label)));
    rows
}

/// 持久表判定：`res_` 前缀（与 `promote::PERSIST_PREFIX` **同一口径**，不另立常量——
/// 两处口径分开写，迟早出现「落库叫 res_、列表按别的规则找」的错位）。
pub fn is_persistent_table(name: &str) -> bool {
    name.starts_with(engine::duckdb::temp_table::promote::PERSIST_PREFIX)
}

/// 把项目分析库的表名切成「普通分析表 / 持久表」两份（顺序沿用入参顺序）。
///
/// 持久表（结果集落库的产物）与项目自建的分析表在库里同源，但用户对它们的心理模型
/// 不同：前者是「我存下来的结果集」，后者是「项目本来就有的」。混在一段里，
/// 一屏 `res_xxx_20261115...` 会把真正想找的分析表挤下去。
pub fn split_persistent_tables(tables: &[String]) -> (Vec<String>, Vec<String>) {
    let mut plain = Vec::new();
    let mut persisted = Vec::new();
    for name in tables {
        if is_persistent_table(name) {
            persisted.push(name.clone());
        } else {
            plain.push(name.clone());
        }
    }
    (plain, persisted)
}

/// DuckDB 「结果集」段：进程单例里的 `tmp_q_` 表（结果集二次分析的入口）。
///
/// 身份用 `NodeId::Temp { scope: Local }`：结果集建在本地单例，本机任何入口都能引用；
/// 用 `Path` 会与草稿相对路径撞身份（同名时选中态串行）。
pub fn result_set_rows(depth: u8, tables: &[String]) -> Vec<Row> {
    debug_assert!(depth <= PICKER_MAX_DEPTH);
    let mut rows: Vec<Row> = tables
        .iter()
        .map(|t| {
            Row::new(
                NodeId::Temp {
                    scope: TempScope::Local,
                    table: t.clone(),
                },
                depth,
                NodeKind::Table,
                t.clone(),
            )
            .with_meta("临时表")
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

/// DuckDB 「洞察产物」段：进程单例里的 `tmp_i_` 表（**再分析**入口）。
///
/// 与结果集分两段而不是混排：`tmp_q_` 是「查询跑出来的」，`tmp_i_` 是「洞察取出来的」，
/// 用户在树里选的语义不同（前者二次分析自己的结果、后者复查/深挖已有样本）。
pub fn insight_sample_rows(depth: u8, tables: &[String]) -> Vec<Row> {
    debug_assert!(depth <= PICKER_MAX_DEPTH);
    let mut rows: Vec<Row> = tables
        .iter()
        .map(|t| {
            Row::new(
                NodeId::InsightSample(t.clone()),
                depth,
                NodeKind::Table,
                t.clone(),
            )
            .with_meta("洞察样本")
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
    debug_assert!(depth <= PICKER_MAX_DEPTH);
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
            None if depth >= PICKER_MAX_DEPTH => {
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
    debug_assert!(depth <= PICKER_MAX_DEPTH);
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
    fn persistent_tables_are_kept_apart_from_project_analysis_tables() {
        let tables: Vec<String> = vec![
            "res_订单明细_20261115103000".into(),
            "数据修复".into(),
            "res_月度汇总_20261115091500".into(),
            "tmp_q_1".into(),
        ];
        let (plain, persisted) = split_persistent_tables(&tables);
        assert_eq!(plain, vec!["数据修复".to_string(), "tmp_q_1".to_string()]);
        assert_eq!(
            persisted,
            vec![
                "res_订单明细_20261115103000".to_string(),
                "res_月度汇总_20261115091500".to_string()
            ],
            "顺序沿用库里给的顺序，不在这里重排"
        );
        // 空表名 / 大小写都不该误判成持久表
        assert!(!is_persistent_table("RES_订单"));
        assert!(!is_persistent_table(""));
        assert!(is_persistent_table("res_"));
    }

    #[test]
    fn persistent_table_labels_are_human_readable_but_keep_identity() {
        assert_eq!(
            persistent_table_label("res_订单明细_20261115103000"),
            "订单明细 · 2026-11-15 10:30:00"
        );
        // 名字里本身带下划线：按**最后**一段切时间戳，描述保留原样
        assert_eq!(
            persistent_table_label("res_结果_2_订单明细_20261115103000"),
            "结果_2_订单明细 · 2026-11-15 10:30:00"
        );
        // 解析不出来就原样回原名，不猜
        assert_eq!(persistent_table_label("res_订单明细"), "res_订单明细");
        assert_eq!(persistent_table_label("res__2026111510300"), "res__2026111510300");
        assert_eq!(persistent_table_label("数据修复"), "数据修复");

        // 身份（NodeId）必须是库里原名，否则确认时反查不到表
        let rows = persistent_table_rows(3, &["res_订单明细_20261115103000".to_string()], ArchiveScope::Project);
        assert_eq!(rows[0].label, "订单明细 · 2026-11-15 10:30:00");
        assert_eq!(
            rows[0].id,
            NodeId::AnalysisTable {
                scope: ArchiveScope::Project,
                table: "res_订单明细_20261115103000".to_string()
            }
        );
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
        let rows = draft_rows(2, &[draft("a.parquet")]);
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
        let rows = draft_rows(2, &[draft("数据\\a.parquet")]);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["数据", "a.parquet"]);
        assert_eq!(rows[0].kind, NodeKind::Folder);
        assert_eq!(rows[1].breadcrumb.as_deref(), Some("数据"));
    }

    #[test]
    fn beyond_depth_limit_is_folded_into_one_row() {
        // 草稿箱(1) → 数据(2) → sub(3) 已到上限，文件不能有第 4 层
        let rows = draft_rows(2, &[draft("数据\\sub\\a.parquet")]);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["数据", "a.parquet"]);
        assert_eq!(rows[1].depth, PICKER_MAX_DEPTH, "文件压在深度上限那一层");
        assert_eq!(
            rows[1].breadcrumb.as_deref(),
            Some("数据 / sub"),
            "被折叠的层要在面包屑里交代清楚"
        );
        assert!(rows.iter().all(|r| r.depth <= PICKER_MAX_DEPTH), "不得突破深度上限");
    }

    #[test]
    fn same_folder_is_reused_across_files() {
        let rows = draft_rows(2, &[draft("数据\\a.parquet"), draft("数据\\b.parquet")]);
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
            2,
            &[draft("b.parquet"), draft("A\\x.parquet"), draft("a.parquet")],
        );
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["A", "x.parquet", "a.parquet", "b.parquet"]);
    }

    #[test]
    fn both_separators_are_understood() {
        let rows = draft_rows(2, &[draft("数据/sub/a.parquet")]);
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
    fn result_sets_use_temp_identity_and_stay_sorted() {
        let rows = result_set_rows(
            1,
            &["tmp_q_b".to_string(), "tmp_q_a".to_string()],
        );
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["tmp_q_a", "tmp_q_b"]);
        assert_eq!(rows[0].kind, NodeKind::Table);
        assert_eq!(rows[0].depth, 1);
        // 结果集必须能确认（二次分析的入口），且身份不与文件路径混用
        assert!(rows[0].is_confirmable());
        assert!(matches!(
            rows[0].id,
            NodeId::Temp {
                scope: TempScope::Local,
                ..
            }
        ));
    }

    #[test]
    fn external_row_uses_file_name_as_label() {
        let row = external_row(3, Path::new("C:/data/sales.parquet"));
        assert_eq!(row.label, "sales.parquet");
        assert_eq!(row.kind, NodeKind::File);
        assert_eq!(row.breadcrumb.as_deref(), Some("C:/data"));
    }
}
