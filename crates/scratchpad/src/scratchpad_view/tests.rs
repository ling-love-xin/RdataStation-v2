//! 草稿箱面板单测（自 `scratchpad_view.rs` 整体搬出，仅整体减一级缩进）。
//!
//! 搬出的理由：测试与生产代码混在一屏里，改实现要跨过两百多行测试才看到下一个函数。
//! 窗口级交互（真点击、真轮询）不在这一层，见 `scratchpad-user-guide.md` §9 验收清单。

/// M5 草稿箱：面板内的纯函数语义（排序 / 压平 / 模板后缀 / 请求消费 / 搜索结果映射）。
///
/// 窗口级交互（真点击、真轮询）不在这一层，见 `scratchpad-user-guide.md` §9 验收清单。
use super::{
    ScratchpadEntry, ScratchpadEntryKind, ScratchpadSearchView, ScratchpadSort, ScratchpadTemplate,
    flatten_scratchpad, join_scratchpad_rel, scratchpad_apply_template_ext,
    scratchpad_entry_matches, scratchpad_shows_dirty_dot, scratchpad_size_label,
    scratchpad_sort_entries, scratchpad_split_name, search_view_from_payload,
};
use crate::jobs::SearchPayload;
use crate::{DiffLineKind, scratchpad_view::scratchpad_diff_marker};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn file(name: &str, size: u64, modified: &str) -> ScratchpadEntry {
    ScratchpadEntry {
        name: name.to_string(),
        path: PathBuf::from(format!("/p/{name}")),
        kind: ScratchpadEntryKind::File,
        size,
        modified_at: Some(modified.to_string()),
        children: None,
    }
}

fn folder(name: &str, modified: &str, kids: Option<Vec<ScratchpadEntry>>) -> ScratchpadEntry {
    ScratchpadEntry {
        name: name.to_string(),
        path: PathBuf::from(format!("/p/{name}")),
        kind: ScratchpadEntryKind::Folder,
        size: 0,
        modified_at: Some(modified.to_string()),
        children: kids,
    }
}

#[test]
fn diff_markers_follow_unified_diff() {
    assert_eq!(scratchpad_diff_marker(DiffLineKind::Added), "+");
    assert_eq!(scratchpad_diff_marker(DiffLineKind::Removed), "-");
    assert_eq!(
        scratchpad_diff_marker(DiffLineKind::Unchanged),
        " ",
        "未变行占位一个空格，左侧行号列不会错位"
    );
}

#[test]
fn dirty_dot_marks_dirty_files_only() {
    let mut dirty = HashSet::new();
    dirty.insert("/p/a.sql".to_string());

    assert!(
        scratchpad_shows_dirty_dot(&dirty, &file("a.sql", 1, "0")),
        "编辑器里改过的文件要打点"
    );
    assert!(
        !scratchpad_shows_dirty_dot(&dirty, &file("b.sql", 1, "0")),
        "没改过的文件不打点"
    );
    assert!(
        !scratchpad_shows_dirty_dot(&dirty, &folder("a.sql", "0", None)),
        "文件夹不画脏点（即使路径命中）"
    );
    assert!(
        !scratchpad_shows_dirty_dot(&HashSet::new(), &file("a.sql", 1, "0")),
        "没有脏文档时谁都不打点"
    );
}

#[test]
fn template_suffix_rules() {
    // 无后缀：补模板后缀。
    assert_eq!(
        scratchpad_apply_template_ext("note", ScratchpadTemplate::Sql),
        "note.sql"
    );
    // 上一次由模板补的后缀：随模板切换替换。
    assert_eq!(
        scratchpad_apply_template_ext("note.md", ScratchpadTemplate::Sql),
        "note.sql"
    );
    // 用户自写的后缀：尊重不动。
    assert_eq!(
        scratchpad_apply_template_ext("note.txt", ScratchpadTemplate::Sql),
        "note.txt"
    );
    // 空白模板：不补也不剥。
    assert_eq!(
        scratchpad_apply_template_ext("note", ScratchpadTemplate::Blank),
        "note"
    );
    assert_eq!(
        scratchpad_apply_template_ext("note.sql", ScratchpadTemplate::Blank),
        "note"
    );
}

#[test]
fn name_split_and_relative_join() {
    assert_eq!(
        scratchpad_split_name("a.sql"),
        ("a".to_string(), ".sql".to_string())
    );
    // 前导点的隐藏名不当作后缀（面板不展示隐藏项，但函数本身应一致）。
    assert_eq!(
        scratchpad_split_name(".env"),
        (".env".to_string(), String::new())
    );
    assert_eq!(join_scratchpad_rel("", "a.sql"), "a.sql");
    assert_eq!(join_scratchpad_rel("dir/", "a.sql"), "dir/a.sql");
}

#[test]
fn sort_keeps_folders_first_and_follows_direction() {
    let mut rows = vec![
        file("b.sql", 200, "2026-01-02T00:00:00Z"),
        folder("dir", "2026-01-03T00:00:00Z", None),
        file("a.sql", 100, "2026-01-04T00:00:00Z"),
    ];
    scratchpad_sort_entries(&mut rows, ScratchpadSort::Name, false);
    let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["dir", "a.sql", "b.sql"],
        "文件夹恒在前，其余按名称升序"
    );

    scratchpad_sort_entries(&mut rows, ScratchpadSort::Size, true);
    let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["dir", "b.sql", "a.sql"],
        "大小降序（文件夹仍在前）"
    );
}

#[test]
fn flatten_respects_expand_and_filter() {
    let tree = vec![folder(
        "dir",
        "t",
        Some(vec![file("a.sql", 1, "t"), file("b.csv", 2, "t")]),
    )];
    let loaded = HashMap::new();

    // 未展开：只出行本身。
    let mut out = Vec::new();
    flatten_scratchpad(
        &tree,
        0,
        &HashSet::new(),
        &loaded,
        ScratchpadSort::Name,
        false,
        "",
        &mut out,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, 0, "根层级缩进为 0");

    // 展开：子项深度 +1。
    let mut expanded = HashSet::new();
    expanded.insert("/p/dir".to_string());
    let mut out = Vec::new();
    flatten_scratchpad(
        &tree,
        0,
        &expanded,
        &loaded,
        ScratchpadSort::Name,
        false,
        "",
        &mut out,
    );
    assert_eq!(out.len(), 3);
    assert_eq!(out[1].0, 1);

    // 过滤命中子项：父目录保留（且强制展开），命中项在内。
    let mut out = Vec::new();
    flatten_scratchpad(
        &tree,
        0,
        &HashSet::new(),
        &loaded,
        ScratchpadSort::Name,
        false,
        "b.csv",
        &mut out,
    );
    let names: Vec<&str> = out.iter().map(|(_, e)| e.name.as_str()).collect();
    assert_eq!(names, vec!["dir", "b.csv"]);
}

#[test]
fn entry_matches_self_or_loaded_subtree() {
    let mut loaded = HashMap::new();
    loaded.insert("/p/dir".to_string(), vec![file("deep.csv", 1, "t")]);
    let dir = folder("dir", "t", None);

    assert!(scratchpad_entry_matches(&dir, "", &loaded), "空过滤全通过");
    assert!(scratchpad_entry_matches(&dir, "di", &loaded), "自身命中");
    assert!(
        scratchpad_entry_matches(&dir, "deep", &loaded),
        "懒加载子树命中"
    );
    assert!(!scratchpad_entry_matches(&dir, "nope", &loaded));
}

#[test]
fn size_label_uses_readable_units() {
    assert_eq!(scratchpad_size_label(512), "512 B");
    assert_eq!(scratchpad_size_label(2048), "2.0 KB");
    assert_eq!(scratchpad_size_label(3 * 1024 * 1024), "3.0 MB");
}

#[test]
fn search_payload_maps_into_view() {
    let payload = SearchPayload {
        scanned: 4,
        truncated: true,
        matches: vec![crate::SearchMatch {
            file: "a.sql".to_string(),
            line_number: 7,
            line_content: "select id".to_string(),
            before_context: vec!["before".to_string()],
            after_context: vec!["after".to_string()],
            match_spans: vec![(7, 9)],
        }],
        replaced: None,
    };
    let view: ScratchpadSearchView =
        search_view_from_payload("id".to_string(), true, false, payload);
    assert_eq!(view.query, "id");
    assert!(view.is_regex && !view.case_sensitive);
    assert_eq!(view.scanned, 4);
    assert!(view.truncated);
    assert_eq!(view.hits.len(), 1);
    // 命中区间与上下文逐字段带过来（高亮靠它们渲染）。
    assert_eq!(view.hits[0].spans, vec![(7, 9)]);
    assert_eq!(view.hits[0].line, 7);
    assert_eq!(view.hits[0].before, vec!["before".to_string()]);
    assert_eq!(view.hits[0].after, vec!["after".to_string()]);
}
