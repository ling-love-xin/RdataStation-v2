//! 草稿箱面板单测（自 `scratchpad_view.rs` 整体搬出，仅整体减一级缩进）。
//!
//! 搬出的理由：测试与生产代码混在一屏里，改实现要跨过两百多行测试才看到下一个函数。
//! 上半部分是纯函数语义；K10（2026-10-09）追加了**面板层窗口级测试**
//! （gpui 测试窗口 + 最小宿主）：渲染 / 选中 / 打开端口 / 只读拒绝可自动回归。
//! 真点击与真轮询仍见 `scratchpad-user-guide.md` §9 验收清单。

/// M5 草稿箱：面板内的纯函数语义（排序 / 压平 / 模板后缀 / 请求消费 / 搜索结果映射）。
///
/// 窗口级交互（真点击、真轮询）不在这一层，见 `scratchpad-user-guide.md` §9 验收清单。
use super::{
    ScratchpadEntry, ScratchpadEntryKind, ScratchpadSearchMode, ScratchpadSearchView,
    ScratchpadSort, ScratchpadTemplate, ToolbarButtonCfg, ToolbarButtonKey, archive_targets,
    flatten_scratchpad, join_scratchpad_rel, scratchpad_apply_template_ext,
    scratchpad_entry_matches, scratchpad_paste_target_of, scratchpad_shows_dirty_dot,
    scratchpad_size_label, scratchpad_sort_entries, scratchpad_split_name,
    search_view_from_payload,
};
use crate::jobs::SearchPayload;
use crate::scratchpad_view::scratchpad_jobs;
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

/// 归档点多选（与行右键「移动到分组」同口径）：点在选中的行上且选了多条 → 整个选择集。
#[test]
fn archive_targets_takes_the_whole_selection_only_when_clicked_row_is_selected() {
    let mut selected = HashSet::new();
    selected.insert("/p/b.sql".to_string());
    selected.insert("/p/a.sql".to_string());
    // 点在选中的行上：整个选择集，且排序（同一批两次点出来的顺序一致）。
    assert_eq!(
        archive_targets("/p/a.sql", &selected),
        vec!["/p/a.sql".to_string(), "/p/b.sql".to_string()]
    );
    // 点在没选中的行上：只有这一条（多选不替用户决定"顺带归档别的"）。
    assert_eq!(
        archive_targets("/p/c.sql", &selected),
        vec!["/p/c.sql".to_string()]
    );
    // 只选了一条：就是这一条。
    let mut single = HashSet::new();
    single.insert("/p/a.sql".to_string());
    assert_eq!(
        archive_targets("/p/a.sql", &single),
        vec!["/p/a.sql".to_string()]
    );
}

/// 粘贴落点（2026-11-28 接线时定的语义）：文件夹 → 自身；文件 → 父目录；
/// 根级文件 → 模块根（空串）。这三个分支决定「粘到哪」，不能靠 UI 手点验证。
#[test]
fn paste_target_puts_files_beside_them_and_folders_inside_them() {
    assert_eq!(scratchpad_paste_target_of("notes", true), "notes");
    assert_eq!(scratchpad_paste_target_of("notes/deep", true), "notes/deep");
    assert_eq!(scratchpad_paste_target_of("notes/a.sql", false), "notes");
    assert_eq!(
        scratchpad_paste_target_of("notes/deep/b.sql", false),
        "notes/deep"
    );
    assert_eq!(
        scratchpad_paste_target_of("root.sql", false),
        "",
        "根级文件没有父目录 → 落到模块根（jobs::Paste 用空串表示）"
    );
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
    // 去模板后只剩空白模板：不补后缀也不剥后缀。
    assert_eq!(
        scratchpad_apply_template_ext("note", ScratchpadTemplate::Blank),
        "note"
    );
    // 用户自写的后缀：尊重不动（模板后缀表已清空，.sql/.md 不再被当作“模板补的”剥掉）。
    assert_eq!(
        scratchpad_apply_template_ext("note.txt", ScratchpadTemplate::Blank),
        "note.txt"
    );
    assert_eq!(
        scratchpad_apply_template_ext("note.sql", ScratchpadTemplate::Blank),
        "note.sql"
    );
    assert_eq!(
        scratchpad_apply_template_ext("note.md", ScratchpadTemplate::Blank),
        "note.md"
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
        ScratchpadSearchMode::Name,
        false,
        false,
        // 无标签筛选（测试用例都走树形语义；标签维度由 primitives.rs 单测覆盖）。
        None,
        None,
        &HashMap::new(),
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
        ScratchpadSearchMode::Name,
        false,
        false,
        // 无标签筛选（测试用例都走树形语义；标签维度由 primitives.rs 单测覆盖）。
        None,
        None,
        &HashMap::new(),
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
        ScratchpadSearchMode::Name,
        false,
        false,
        // 无标签筛选（测试用例都走树形语义；标签维度由 primitives.rs 单测覆盖）。
        None,
        None,
        &HashMap::new(),
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

    assert!(
        scratchpad_entry_matches(&dir, "", ScratchpadSearchMode::Name, false, false, &loaded),
        "空过滤全通过"
    );
    assert!(
        scratchpad_entry_matches(
            &dir,
            "di",
            ScratchpadSearchMode::Name,
            false,
            false,
            &loaded
        ),
        "自身命中"
    );
    assert!(
        scratchpad_entry_matches(
            &dir,
            "deep",
            ScratchpadSearchMode::Name,
            false,
            false,
            &loaded
        ),
        "懒加载子树命中"
    );
    assert!(!scratchpad_entry_matches(
        &dir,
        "nope",
        ScratchpadSearchMode::Name,
        false,
        false,
        &loaded
    ));
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

// ===== K10（2026-10-09）：面板层窗口级自动化测试 =====
//
// 此前窗口级交互（点击 / 轮询）靠人工验收；按架构 §11「窗口测试走宿主入口」，
// 这里用 gpui 测试窗口 + 最小宿主（`TestHost`）把「渲染 / 选中 / 打开端口 /
// 只读拒绝」变成可回归的自动化断言。交互走真动作方法（`pub(super)`），不模拟点击。
// 不使用 `use gpui_kit::*` 通配导入（会把 `test` 属性宏带入作用域，见 editor 同款注释）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::{App, Pixels, TestAppContext};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window,
    div, px,
};

use crate::ScratchpadView;
use crate::host::ScratchpadHost;
use crate::jobs::LoadResult;
use crate::scratchpad_view::primitives::{ScratchpadEdit, scratchpad_type_icon};
use crate::scratchpad_view::render_scratchpad_search_pane;
use content_search::view::SearchHit as ScratchpadSearchHit;

/// 最小宿主：记录打开请求、状态栏提示与搜索结果落地（不接编辑器 / 不接洞察）。
struct TestHost {
    root: Option<std::path::PathBuf>,
    read_only: bool,
    opened: RefCell<Vec<std::path::PathBuf>>,
    notices: RefCell<Vec<String>>,
    searches: RefCell<Vec<Option<ScratchpadSearchView>>>,
    /// 接过资产库回收站没有（只影响回收站分区的「资产库归档…」入口）。
    asset_trash: bool,
    asset_trash_opened: RefCell<usize>,
}

impl ScratchpadHost for TestHost {
    fn project_root(&self) -> Option<std::path::PathBuf> {
        self.root.clone()
    }
    fn read_only(&self) -> bool {
        self.read_only
    }
    fn notice(&self, message: String, _cx: &mut App) {
        self.notices.borrow_mut().push(message);
    }
    fn notify_host(&self, _cx: &mut App) {}
    fn show_search_results(&self, view: Option<ScratchpadSearchView>, _cx: &mut App) {
        self.searches.borrow_mut().push(view);
    }
    fn open_in_editor(&self, path: std::path::PathBuf) {
        self.opened.borrow_mut().push(path);
    }
    fn can_open_asset_trash(&self) -> bool {
        self.asset_trash
    }
    fn open_asset_trash(&self, _window: &mut gpui_kit::Window, _cx: &mut App) {
        *self.asset_trash_opened.borrow_mut() += 1;
    }
}

/// 回收站分区是项目回收站的唯一入口：接过资产库的宿主在这里摆「资产库归档…」，
/// 没接过的宿主不摆（点了没反应的入口比没有更糟）。
#[gpui_kit::test]
fn trash_section_exposes_the_asset_trash_entry_only_when_host_accepts_it(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("asset_trash_entry");

    let (with_asset, cx) = {
        let host = host_with_asset_trash(root.clone());
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };
    let seq = crate::jobs::enqueue_root_load(&root, vec![]);
    cx.update(|_window, cx| {
        with_asset.update(cx, |view, cx| {
            view.apply_scratchpad_loads(vec![panel_load(seq, vec![])], cx);
            view.scratchpad.borrow_mut().trash_expanded = true;
            cx.notify();
        });
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("sp-trash-assets-entry").is_some(),
        "接过资产库的宿主要在回收站分区摆出资产库入口"
    );

    let (without_asset, cx) = {
        let host = host_with(Some(root.clone()), false);
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };
    let seq = crate::jobs::enqueue_root_load(&root, vec![]);
    cx.update(|_window, cx| {
        without_asset.update(cx, |view, cx| {
            view.apply_scratchpad_loads(vec![panel_load(seq, vec![])], cx);
            view.scratchpad.borrow_mut().trash_expanded = true;
            cx.notify();
        });
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("sp-trash-assets-entry").is_none(),
        "没接资产库的宿主不得摆出点了没反应的入口"
    );
}

/// 每个用例一个临时项目根（互不干扰；用完尽力清理）。
fn panel_root(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rds_scratchpad_panel_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

fn panel_load(seq: u64, entries: Vec<ScratchpadEntry>) -> LoadResult {
    LoadResult {
        seq,
        entries: Ok(entries),
        refs: vec![],
        trash: vec![],
        children: vec![],
    }
}

fn host_with(root: Option<std::path::PathBuf>, read_only: bool) -> Rc<TestHost> {
    Rc::new(TestHost {
        root,
        read_only,
        opened: RefCell::new(Vec::new()),
        notices: RefCell::new(Vec::new()),
        searches: RefCell::new(Vec::new()),
        asset_trash: false,
        asset_trash_opened: RefCell::new(0),
    })
}

/// 接过资产库回收站的宿主（回收站分区要摆「资产库归档…」入口）。
fn host_with_asset_trash(root: std::path::PathBuf) -> Rc<TestHost> {
    Rc::new(TestHost {
        root: Some(root),
        read_only: false,
        opened: RefCell::new(Vec::new()),
        notices: RefCell::new(Vec::new()),
        searches: RefCell::new(Vec::new()),
        asset_trash: true,
        asset_trash_opened: RefCell::new(0),
    })
}

/// K10：面板在窗口里渲染不 panic；注入条目后「↓ 选中首项 → Enter 打开」，
/// 宿主端口必须收到**绝对路径**（与生产同一动作路径 `scratchpad_open_selection`）。
#[gpui_kit::test]
fn panel_renders_and_opens_selected_file(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("k10_open");
    let host = host_with(Some(root.clone()), false);

    let (panel, cx) = {
        let host = host.clone();
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };

    // 回填模块根加载结果（等价于后台任务完成 → `apply_scratchpad_loads`）。
    // 序号不能硬编码：`add_window_view` 若渲染首帧会经 `render_scratchpad` 发起
    // 一次真实加载，`load_seq` 取的是 `enqueue_root_load` 的全局递增序号；
    // 实时调用一次取当前序号（单调递增，必然 ≥ 面板的 `load_seq`），注入才不会被
    // `newest < view.load_seq` 的防过期回填丢弃。
    let seq = scratchpad_jobs::enqueue_root_load(&root, vec![]);
    let mut entry = file("a.sql", 12, "2026-10-09");
    entry.path = root.join("a.sql");
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| {
            view.apply_scratchpad_loads(vec![panel_load(seq, vec![entry.clone()])], cx);
        });
    });

    // 键盘导航选中首项 → 打开 → 宿主端口收到路径（渲染不 panic 由测试本身保证）。
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| view.scratchpad_move(0, cx));
    });
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| view.scratchpad_open_selection(cx));
    });

    let opened = host.opened.borrow().clone();
    assert_eq!(
        opened,
        vec![root.join("a.sql")],
        "打开请求必须落到宿主端口，且路径为绝对路径"
    );
    assert!(
        host.searches.borrow().is_empty(),
        "未发起搜索时不得投递搜索结果"
    );
}

/// K10：只读项目里发起「新建」被拒绝，状态栏提示经宿主端口下发。
#[gpui_kit::test]
fn readonly_rejects_new_file(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("k10_ro");
    let host = host_with(Some(root.clone()), true);

    let (panel, cx) = {
        let host = host.clone();
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };

    cx.update(|window, cx| {
        panel.update(cx, |view, cx| {
            view.start_scratchpad_edit(ScratchpadEdit::NewFile, window, cx);
        });
    });

    let notices = host.notices.borrow().clone();
    assert!(
        notices.iter().any(|n| n.contains("只读模式")),
        "只读拒绝必须给出可见提示（经宿主端口），实际：{notices:?}"
    );
    assert!(
        host.opened.borrow().is_empty(),
        "只读模式不得产生任何打开请求"
    );
}

/// K10：命中面板（中央编辑区投影）可构建渲染载荷，且未点击时打开回调不触发。
/// 同时锚定：`ScratchpadSearchHitTarget` 携带 1 基行号 + 行内字节区间（#2 跳转载荷）。
#[gpui_kit::test]
fn search_pane_builds_with_hit_target(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("k10_search");
    let host = host_with(Some(root.clone()), false);
    let (panel, cx) = {
        let host = host.clone();
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };

    // 命中视图（数据来自 `search_view_from_payload`，这里直接构造以锚定跳转载荷）。
    let mut view = ScratchpadSearchView {
        query: "id".to_string(),
        is_regex: false,
        case_sensitive: false,
        scanned: 1,
        truncated: false,
        hits: vec![],
    };
    view.hits.push(ScratchpadSearchHit {
        file: "a.sql".to_string(),
        line: 2,
        content: "select id".to_string(),
        spans: vec![(7, 9)],
        before: vec![],
        after: vec![],
    });

    let captured: Rc<RefCell<Vec<crate::ScratchpadSearchHitTarget>>> =
        Rc::new(RefCell::new(Vec::new()));
    let on_open_hit: Rc<dyn Fn(crate::ScratchpadSearchHitTarget, &mut gpui_kit::Window, &mut App)> = {
        let captured = captured.clone();
        Rc::new(move |target, _window, _cx| captured.borrow_mut().push(target))
    };

    let _ = panel;
    cx.update(|_window, cx| {
        let theme = cx.theme();
        // 只验证「可构建」：渲染本身在中央编辑区挂载时发生，构造成功即面板载荷完整。
        // 替换栏已随 VSCode 对齐迁到侧栏搜索面板（2026-10-10），渲染函数不再接收替换参数。
        let div = render_scratchpad_search_pane(
            &view,
            &theme,
            theme.colors.warning,
            |name| scratchpad_type_icon(name).to_string(),
            on_open_hit.clone(),
            |_e, _w, _cx| {},
        );
        let _ = div;
    });

    assert!(
        captured.borrow().is_empty(),
        "未点击命中标题时跳转回调不得触发"
    );
}

/// 元素级对齐断言（用户点名：命中明细缩进对齐文件节点名，2026-10-11）：
/// 注入搜索命中 → 渲染搜索态结果树 → 用 `debug_bounds` 取两个文本容器的几何，
/// 断言命中明细文本起点 x 与文件节点名文本起点 x **完全相等**（像素级，不靠截图/OCR）。
#[gpui_kit::test]
fn search_result_hit_text_aligns_with_file_name(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("k10_align");
    let host = host_with(Some(root.clone()), false);
    let (panel, cx) = {
        let host = host.clone();
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };

    // 构造搜索命中视图（根目录文件，目录组会显示为 `scratchpad\`）。
    let mut search_view_data = ScratchpadSearchView {
        query: "id".to_string(),
        is_regex: false,
        case_sensitive: false,
        scanned: 1,
        truncated: false,
        hits: vec![],
    };
    search_view_data.hits.push(ScratchpadSearchHit {
        file: "a.sql".to_string(),
        line: 1,
        content: "select id from t".to_string(),
        spans: vec![(7, 9)],
        before: vec![],
        after: vec![],
    });

    // 先注入模块根加载成功（loaded=true、error=None；否则 render 可能因 error 提前返回，
    // 搜索面板整体不渲染）。与 `panel_renders_and_opens_selected_file` 同一回填路径。
    let seq = scratchpad_jobs::enqueue_root_load(&root, vec![]);
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| {
            view.apply_scratchpad_loads(vec![panel_load(seq, vec![])], cx);
        });
    });

    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| {
            // 搜索态整体切换（2026-10-11）：面板打开时侧栏才渲染搜索结果树。
            view.scratchpad.borrow_mut().search_panel_open = true;
            view.scratchpad.borrow_mut().search_view = Some(search_view_data);
            cx.notify();
        });
    });
    // 渲染一帧（搜索态结果树挂载）。
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });

    let (mode, advanced, err, loaded) = cx.update(|_w, cx| {
        let v = panel.read(cx).scratchpad.borrow();
        (
            v.search_mode == ScratchpadSearchMode::Content,
            v.search_advanced,
            v.error.clone(),
            v.loaded,
        )
    });
    eprintln!(
        "[align-test] mode_is_content={mode} advanced={advanced} error={err:?} loaded={loaded}"
    );
    // 注：`.id()` 不注册 debug_bounds（gpui 需 `.debug_selector()`，见 archive 面板用例）；
    // 因此此处只断言两个已挂 debug_selector 的文本锚点。

    let file_text = cx
        .debug_bounds("sp-res-file-text-a.sql")
        .expect("文件节点名文本容器必须渲染（搜索态结果树）");
    let hit_text = cx
        .debug_bounds("sp-res-hit-text-a.sql-1")
        .expect("命中明细文本容器必须渲染（搜索态结果树）");
    assert_eq!(
        file_text.origin.x, hit_text.origin.x,
        "命中明细文本起点必须与文件节点名对齐（元素级像素断言）；file_x={:?} hit_x={:?}",
        file_text.origin.x, hit_text.origin.x
    );
}

/// 搜索态四行同档（2026-11-25 用户反馈③）：搜索 / 替换 / 包含 / 排除四个输入行必须
/// **同左缘、同间距、同一输入底色**。
///
/// 历史写法：搜索行单独包一层 `bg(group_box)` + `px_1p5` + `pb_1` —— 左缘比另三行多缩 6、
/// 与替换行间距比其它行多 4；更隐蔽的是那层更暗的底色，`Input` 暗色底是半透明
/// （`input_background()` = `input` 混 30% 透明，`vendor/gpui-component/src/theme/mod.rs:503`），
/// 同一个控件叠在 `group_box` 上显 #282828、叠在 `background` 上显 #2D2D2D，同屏两种深浅。
///
/// 元素级断言（不靠截图/OCR）：四行各挂 `debug_selector`，直接量几何。
#[gpui_kit::test]
fn search_rows_share_one_gutter_and_spacing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("k39_search_rows");
    let host = host_with(Some(root.clone()), false);
    let (panel, cx) = {
        let host = host.clone();
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };

    // 模块根加载成功（渲染提前返回的分支要排除）。
    let seq = scratchpad_jobs::enqueue_root_load(&root, vec![]);
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| {
            view.apply_scratchpad_loads(vec![panel_load(seq, vec![])], cx);
        });
    });
    // 搜索态 + 展开高级选项：四行（搜索/替换/包含/排除）同时在场。
    cx.update(|window, cx| {
        panel.update(cx, |view, cx| {
            view.ensure_scratchpad_inputs(window, cx);
            {
                let mut sp = view.scratchpad.borrow_mut();
                sp.search_panel_open = true;
                sp.search_advanced = true;
            }
            cx.notify();
        });
        window.refresh();
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });

    let ids = [
        "sp-search-row",
        "sp-replace-row",
        "sp-include-row",
        "sp-exclude-row",
    ];
    let rows: Vec<(&str, gpui_kit::Bounds<gpui_kit::Pixels>)> = ids
        .iter()
        .map(|id| {
            let b = cx
                .debug_bounds(*id)
                .unwrap_or_else(|| panic!("{id} 必须渲染（搜索态四行）"));
            (*id, b)
        })
        .collect();

    // 四行都真的画出了输入框（行高非零），否则下面的一致性断言会退化成「零对零」。
    for (id, b) in &rows {
        assert!(
            b.size.height > gpui_kit::Pixels::ZERO,
            "「{id}」行必须有高度（输入框在场，height={:?}）",
            b.size.height
        );
    }
    // 左缘：四行同一档（历史写法里搜索行比另三行多 6）。
    for (id, b) in rows.iter().skip(1) {
        assert_eq!(
            b.origin.x, rows[0].1.origin.x,
            "「{id}」左缘 {} 必须与搜索行 {} 一致（搜索行不得多套一层内距）",
            b.origin.x, rows[0].1.origin.x
        );
    }
    // 间距：三处垂直间隙相等（历史写法里搜索 ↔ 替换多 4）。
    let gaps: Vec<_> = rows
        .windows(2)
        .map(|w| w[1].1.origin.y - (w[0].1.origin.y + w[0].1.size.height))
        .collect();
    assert_eq!(
        gaps[0], gaps[1],
        "搜索 ↔ 替换的间距必须与替换 ↔ 包含一致：{gaps:?}"
    );
    assert_eq!(
        gaps[1], gaps[2],
        "替换 ↔ 包含 与 包含 ↔ 排除 必须同间距：{gaps:?}"
    );
}

/// 工具栏 ⋯ 更多设置（2026-10-11 原型 644-658 对齐）：按钮可显示/隐藏、可重置默认布局。
/// 元素级断言（用户裁定的对齐验证方式）：左右分组挂 `debug_selector` 锚点，用几何宽度
/// 区分「按钮还在渲染」（宽 > 0）与「被 ⋯ 菜单隐藏后不再渲染」（空组宽归 0）。
#[gpui_kit::test]
fn toolbar_more_toggles_visibility_and_reset(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("k38_more");
    let host = host_with(Some(root.clone()), false);
    let (panel, cx) = {
        let host = host.clone();
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };

    // 注入模块根加载成功（普通视图渲染前置；与 align 测试同一回填路径）。
    let seq = scratchpad_jobs::enqueue_root_load(&root, vec![]);
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| {
            view.apply_scratchpad_loads(vec![panel_load(seq, vec![])], cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let default_w = cx
        .debug_bounds("sp-toolbar-left")
        .expect("左分组必须渲染（⋯ 菜单锚点）")
        .size
        .width;
    assert!(
        default_w > Pixels::ZERO,
        "默认左侧 5 个入口应全部渲染（宽 > 0），实测 {default_w:?}"
    );

    // 模拟 ⋯ 菜单点击切换：逐个隐藏左侧 5 个入口（与菜单 on_click 同一逻辑：
    // `entry(key).or_insert(默认).visible = !visible`；点击切换本身由 PopupMenu on_click 派发）。
    cx.update(|_w, cx| {
        panel.update(cx, |view, cx| {
            for key in [
                ToolbarButtonKey::Search,
                ToolbarButtonKey::NewFile,
                ToolbarButtonKey::NewFolder,
                ToolbarButtonKey::Import,
                ToolbarButtonKey::Reference,
            ] {
                let mut v = view.scratchpad.borrow_mut();
                let cfg = v
                    .toolbar_cfg
                    .entry(key)
                    .or_insert_with(|| ToolbarButtonCfg {
                        visible: true,
                        side: key.default_side_text().to_string(),
                        ..Default::default()
                    });
                cfg.visible = !cfg.visible;
            }
            cx.notify();
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let hidden_w = cx
        .debug_bounds("sp-toolbar-left")
        .expect("左分组必须渲染（⋯ 菜单锚点）")
        .size
        .width;
    assert_eq!(
        hidden_w,
        Pixels::ZERO,
        "隐藏左侧全部入口后左分组应为空（宽归 0），实测 {hidden_w:?}"
    );

    // 重置为默认布局：清空配置 → 全部恢复显示。
    cx.update(|_w, cx| {
        panel.update(cx, |view, cx| {
            view.scratchpad.borrow_mut().toolbar_cfg.clear();
            cx.notify();
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let reset_w = cx
        .debug_bounds("sp-toolbar-left")
        .expect("左分组必须渲染（⋯ 菜单锚点）")
        .size
        .width;
    assert_eq!(
        reset_w, default_w,
        "重置后左分组宽度应回到默认（{default_w:?}），实测 {reset_w:?}"
    );

    // 位置左/右切换（2026-10-11 接线）：把「排序」（默认右组）挪到左组 →
    // 右组宽度收窄、左组宽度增宽（排序按钮从右迁到左）。
    let right_default_w = cx
        .debug_bounds("sp-toolbar-right")
        .expect("右分组必须渲染（⋯ 锚点）")
        .size
        .width;
    cx.update(|_w, cx| {
        panel.update(cx, |view, cx| {
            let mut v = view.scratchpad.borrow_mut();
            let cfg = v
                .toolbar_cfg
                .entry(ToolbarButtonKey::Sort)
                .or_insert_with(|| ToolbarButtonCfg {
                    visible: true,
                    side: ToolbarButtonKey::Sort.default_side_text().to_string(),
                    ..Default::default()
                });
            cfg.side = "left".to_string();
            cx.notify();
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let moved_left_w = cx
        .debug_bounds("sp-toolbar-left")
        .expect("左分组必须渲染（⋯ 菜单锚点）")
        .size
        .width;
    let moved_right_w = cx
        .debug_bounds("sp-toolbar-right")
        .expect("右分组必须渲染（⋯ 锚点）")
        .size
        .width;
    assert!(
        moved_left_w > default_w,
        "排序挪到左组后左分组应变宽（{default_w:?} → {moved_left_w:?}）"
    );
    assert!(
        moved_right_w < right_default_w,
        "排序离开右组后右分组应变窄（{right_default_w:?} → {moved_right_w:?}）"
    );

    // 右侧排序/刷新与 ⋯ 本身恒显（不受左侧隐藏影响）。
    let right_w = cx
        .debug_bounds("sp-toolbar-right")
        .expect("右分组必须渲染（⋯ 锚点）")
        .size
        .width;
    assert!(
        right_w > Pixels::ZERO,
        "右侧按钮应保持渲染，实测 {right_w:?}"
    );
}

/// 工具栏左右分组分离回归（2026-11-24 用户目视反馈①）：`Toolbar` 曾被包进
/// `h_flex` 行——flex 子元素按内容收缩不铺满，中间弹簧恒 0 宽，左右两组贴在
/// 一起（⋯ 菜单的「放在左/右」因此看不出效果，面板拉伸也不重排）。修复 = 外层
/// div 改 block 布局 + `Toolbar::h_full()`（与数据导航面板头同一写法），弹簧
/// 生效后两组分居两端。断言：右组左缘与左组右缘的间距远大于组内 gap（4px）。
#[gpui_kit::test]
fn toolbar_left_and_right_groups_split_at_panel_edges(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("k39_split");
    let host = host_with(Some(root.clone()), false);
    let (panel, cx) = {
        let host = host.clone();
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };

    // 注入模块根加载成功（普通视图渲染前置；与 more 测试同一回填路径）。
    let seq = scratchpad_jobs::enqueue_root_load(&root, vec![]);
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| {
            view.apply_scratchpad_loads(vec![panel_load(seq, vec![])], cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let left = cx
        .debug_bounds("sp-toolbar-left")
        .expect("左分组必须渲染（⋯ 菜单锚点）");
    let right = cx
        .debug_bounds("sp-toolbar-right")
        .expect("右分组必须渲染（⋯ 锚点）");
    let gap = (right.origin.x - (left.origin.x + left.size.width)).as_f32();
    assert!(
        gap > 16.0,
        "左右两组应被中间弹簧分居两端（间距 > 16px，实测 {gap}px）；\
         若只剩组内 gap 量级（≤ 4px），说明 Toolbar 没铺满面板宽——弹簧没拿到空间，\
         「放在左/右」切换在视觉上也不会生效"
    );
}

/// 「草稿箱有文件但没显示」排查回归（2026-10-11 用户现场反馈）：
/// 注入根加载 + 一个文件条目 → 渲染普通视图 → 断言文件行文本 debug_bounds 高度 > 0。
/// 若 Accordion 内虚拟列表高度坍塌（高度链无约束），此处失败，直接定位显示问题。
#[gpui_kit::test]
fn drafts_accordion_tree_row_has_visible_height(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("k11_height");
    let host = host_with(Some(root.clone()), false);
    let (panel, cx) = {
        let host = host.clone();
        cx.add_window_view(move |_window, cx| ScratchpadView::new(host, cx))
    };

    // 注入模块根加载成功 + 一个文件条目（与 panel_renders_and_opens_selected_file 同路径）。
    let seq = scratchpad_jobs::enqueue_root_load(&root, vec![]);
    let mut entries = Vec::new();
    for i in 0..10 {
        let mut e = file(&format!("f{i}.md"), 12, "2026-10-09");
        e.path = root.join(format!("f{i}.md"));
        entries.push(e);
    }
    let mut d = folder("notes", "2026-10-09", None);
    d.path = root.join("notes");
    entries.push(d);
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| {
            view.apply_scratchpad_loads(vec![panel_load(seq, entries.clone())], cx);
        });
    });
    // 渲染多帧（Accordion 默认展开，spring 动画推进到稳态；虚拟列表应挂载并分配高度）。
    for _ in 0..5 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    let b = cx
        .debug_bounds("sp-row-text-f0.md")
        .expect("普通视图文件行文本必须渲染（草稿树）");
    assert!(
        b.size.height > Pixels::ZERO,
        "文件行文本必须分配可见高度（Accordion 内虚拟列表不得高度坍塌），实测 {:#?}",
        b.size
    );
}

/// 固定尺寸 Harness：模拟真实 LeftPanel（约 320×860）承载草稿箱面板。
struct SizedDraftsHarness {
    panel: Entity<ScratchpadView>,
}

impl Render for SizedDraftsHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(320.)).h(px(860.)).child(self.panel.clone())
    }
}

/// 真实侧栏尺寸复现（2026-10-11 用户截图：badge 11 但树空白）：
/// 把面板包进 320×860 固定容器（模拟真实 LeftPanel 高度链），注入 11 条目，
/// 断言文件行文本必须渲染出可见高度。若真实空白是「Accordion 内容区在窄面板
/// 下高度链断裂」所致，此测试失败。
#[gpui_kit::test]
fn drafts_tree_renders_in_real_panel_size(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = panel_root("k11_panel_size");
    let host = host_with(Some(root.clone()), false);
    let (holder, cx) = {
        let host = host.clone();
        cx.add_window_view(move |_window, cx| {
            let panel = cx.new(|cx| ScratchpadView::new(host.clone(), cx));
            SizedDraftsHarness { panel }
        })
    };
    let seq = scratchpad_jobs::enqueue_root_load(&root, vec![]);
    let mut entries = Vec::new();
    for i in 0..10 {
        let mut e = file(&format!("f{i}.md"), 12, "2026-10-09");
        e.path = root.join(format!("f{i}.md"));
        entries.push(e);
    }
    let mut d = folder("notes", "2026-10-09", None);
    d.path = root.join("notes");
    entries.push(d);
    cx.update(|_window, cx| {
        holder.update(cx, |h, cx| {
            h.panel.update(cx, |view, cx| {
                view.apply_scratchpad_loads(vec![panel_load(seq, entries.clone())], cx);
            });
        });
    });
    for _ in 0..5 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    let b = cx
        .debug_bounds("sp-row-text-f0.md")
        .expect("真实面板尺寸（320×860）下文件行必须渲染");
    assert!(
        b.size.height > Pixels::ZERO,
        "文件行在真实面板尺寸下必须可见（Accordion 内容区高度链不得断裂），实测 {:#?}",
        b.size
    );
}
