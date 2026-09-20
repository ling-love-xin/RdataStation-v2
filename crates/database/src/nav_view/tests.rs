//! 导航面板单测（自 `nav_view.rs` 整体搬出，**逐字未改**）。
//!
//! 搬出的理由：测试占原文 1426 行（约 19%），与生产代码混在一屏里，
//! 改实现要跨过一千多行测试才能看到下一个函数。

#[cfg(test)]
// 注意：不通配导入（`use gpui_kit::*` 会把 gpui 的 `test` 宏带入作用域）。
use super::nav_type_badge;
use super::{
    NavBadgeStatus, RevealTarget, insight_schema_target, nav_badge_pulses, nav_data_target,
    nav_kind_icon, nav_merge_page, nav_object_type_label, nav_order_members, nav_relative_time,
    nav_reorder, nav_scope_tooltip, nav_search_hit_property, nav_search_hit_ref,
    nav_search_query_ready, nav_step, nav_type_label, nav_type_short_label, parse_nav_search,
    search_hit_row_id, search_hit_row_key,
};
use crate::commands::NavClearSearch;
use crate::model::{
    NavFolder, NavNode, NavNodeKind, NavPath, NavSource, ObjectKind, ObjectRef, PropertyKind,
};
use crate::nav_rows::NavRow;
use std::collections::HashSet;
use std::time::{Duration, SystemTime};
// 窗口级验收要用的两个 trait（显式导入，不通配：`use gpui_kit::*` 会把 `test` 宏带进来）。
use gpui_kit::{AppContext as _, Focusable as _, ParentElement as _, Pixels, Styled as _};

/// 定位验收用的最小宿主：只回答定位路径真会问到的两件事（是否已连 / 项目根）。
///
/// 单独放一层子模块：31 个方法全是样板，不污染本模块的可读性。
mod stub_host {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::rc::Rc;

    use gpui_kit::{App, Window};

    use crate::model::{ObjectRef, PropertyRequest};
    use crate::nav_host::{ConnectionProbe, NavFilters, NavHost};
    use workbench_shell::model::{ConnectionItem, GroupFormSeed, QueryRequest, RightPanel};

    pub(super) struct StubNavHost {
        connected: RefCell<HashSet<String>>,
        connections: Vec<ConnectionItem>,
    }

    impl StubNavHost {
        pub(super) fn new(conn_id: &str) -> Self {
            let mut set = HashSet::new();
            set.insert(conn_id.to_string());
            Self {
                connected: RefCell::new(set),
                connections: vec![ConnectionItem {
                    id: conn_id.to_string(),
                    name: conn_id.to_string(),
                    driver: "sqlite".to_string(),
                    connected: true,
                    host: None,
                    port: None,
                    database: None,
                    schema: None,
                    description: None,
                    use_duckdb_fed: false,
                    created_at: String::new(),
                    updated_at: String::new(),
                }],
            }
        }
    }

    impl NavHost for StubNavHost {
        fn connections(&self) -> Vec<ConnectionItem> {
            self.connections.clone()
        }
        fn selected_index(&self) -> Option<usize> {
            None
        }
        fn project_root(&self) -> Option<PathBuf> {
            None
        }
        fn select_connection(&self, _index: Option<usize>, _cx: &mut App) {}
        fn notice(&self, _message: String, _cx: &mut App) {}
        fn notify_host(&self, _cx: &mut App) {}
        fn show_tags(&self, _cx: &App) -> bool {
            false
        }
        fn set_show_tags(&self, _value: bool, _cx: &mut App) {}
        fn show_scope(&self, _cx: &App) -> bool {
            false
        }
        fn set_show_scope(&self, _value: bool, _cx: &mut App) {}
        fn source_short_code(&self, _cx: &App) -> bool {
            false
        }
        fn nav_filters(&self, _cx: &App) -> NavFilters {
            NavFilters::default()
        }
        fn set_nav_filters(&self, _filters: NavFilters, _cx: &mut App) {}
        fn is_connected(&self, conn_id: &str) -> bool {
            self.connected.borrow().contains(conn_id)
        }
        fn connect(&self, _conn_id: &str) -> Result<(), String> {
            Ok(())
        }
        fn disconnect(&self, _conn_id: &str) -> Result<(), String> {
            Ok(())
        }
        fn connection_probe(&self) -> ConnectionProbe {
            |_, _| Err("stub 不做真探测".to_string())
        }
        fn reload_connections(&self, _cx: &mut App) {}
        fn copy_connection(&self, _from_id: &str, _new_name: &str) -> Result<(), String> {
            Ok(())
        }
        fn share_connection(&self, _conn_id: &str) -> Result<(), String> {
            Ok(())
        }
        fn delete_connection(&self, _conn_id: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn show_properties(&self, _request: PropertyRequest, _cx: &mut App) {}
        fn open_query(&self, _request: QueryRequest, _cx: &mut App) {}
        fn edit_connection(&self, _conn_id: &str, _window: &mut Window, _cx: &mut App) {}
        fn new_connection(&self, _window: &mut Window, _cx: &mut App) {}
        fn open_group_form(
            &self,
            _seed: GroupFormSeed,
            _window: &mut Window,
            _cx: &mut App,
            _on_submit: Rc<dyn Fn(Option<String>, String, Option<String>, &mut App)>,
        ) {
        }
        fn open_cache_dialog(&self, _window: &mut Window, _cx: &mut App) {}
        fn open_right_panel(&self, _panel: RightPanel, _cx: &mut App) {}
        fn open_mock_panel(&self, _source: Option<ObjectRef>, _cx: &mut App) {}
        fn open_insight_table(&self, _source: ObjectRef, _cx: &mut App) {}
        fn open_insight_schema(&self, _schema: ObjectRef, _cx: &mut App) {}
    }
}

/// 播种定位要用的四层（连接 → catalog → schema → 表文件夹）已加载状态。
///
/// 为什么不用真连接：真链路要驱动 + 冷启动内省；定位的**决策**只依赖这四层的存在与否，
/// 所以把状态直接摆好，测的就是决策本身（真正的取数已由 engine / database 的分页测试钉住）。
fn seed_reveal_state(
    view: &super::NavView,
    conn: &str,
    catalog: &str,
    schema: &str,
    folder_children: Vec<NavNode>,
    folder_total: Option<usize>,
) {
    let catalog_key = NavNode::child_key(conn, &[catalog]);
    let schema_key = NavNode::child_key(conn, &[catalog, schema]);
    let folder_key = NavNode::child_key(conn, &[catalog, schema, NavFolder::Tables.key()]);
    let mut s = view.nav.borrow_mut();
    s.children.insert(
        conn.to_string(),
        vec![NavNode::new(
            catalog_key.clone(),
            catalog,
            conn,
            NavNodeKind::Catalog,
            true,
        )],
    );
    s.children.insert(
        catalog_key,
        vec![NavNode::new(
            schema_key.clone(),
            schema,
            conn,
            NavNodeKind::Schema,
            true,
        )],
    );
    s.children.insert(
        schema_key,
        vec![NavNode::new(
            folder_key.clone(),
            "表",
            conn,
            NavNodeKind::Folder(NavFolder::Tables),
            true,
        )],
    );
    s.children.insert(folder_key.clone(), folder_children);
    if let Some(total) = folder_total {
        s.child_total.insert(folder_key, total);
    }
}

fn table_node(conn: &str, catalog: &str, schema: &str, name: &str) -> NavNode {
    NavNode::new(
        NavNode::child_key(conn, &[catalog, schema, name]),
        name,
        conn,
        NavNodeKind::Table { row_estimate: None },
        true,
    )
}

/// 极小 root view：只为把 `NavView` 挂进窗口（断言都在状态上，不靠它渲染）。
struct RevealHarness {
    view: gpui_kit::Entity<super::NavView>,
}

impl gpui_kit::Render for RevealHarness {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        _cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        gpui_kit::div().size_full().child(self.view.clone())
    }
}

/// 造一个挂着 `NavView` 的窗口（带 stub 宿主）。
fn open_nav_view<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
    conn: &str,
) -> (
    gpui_kit::Entity<super::NavView>,
    &'a mut gpui_kit::VisualTestContext,
) {
    cx.update(gpui_kit::init);
    let host: std::rc::Rc<dyn crate::nav_host::NavHost> =
        std::rc::Rc::new(stub_host::StubNavHost::new(conn));
    let (harness, cx) = cx.add_window_view(|_window, cx| {
        let view = cx.new(|cx| super::NavView::new(host, cx));
        RevealHarness { view }
    });
    cx.update(|_window, cx| window_draw(cx));
    let view = cx.update(|_window, cx| harness.read(cx).view.clone());
    (view, cx)
}

/// 空实现：窗口级用例不靠渲染断言，只需要窗口存在（焦点与实体生命周期）。
fn window_draw(_cx: &mut gpui_kit::App) {}

/// 重画一帧（窗口测试里推进 render）。
fn redraw(cx: &mut gpui_kit::VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 当前渲染出的**可见行顺序**（渲染期累积的键盘导航序列，与视口无关）。
fn visible_keys(view: &super::NavView) -> Vec<String> {
    view.nav_order
        .borrow()
        .iter()
        .map(|item| item.key.clone())
        .collect()
}

/// 契约（虚拟列表重构的安全网）：展开态决定**哪些行可见**，顺序 = 父在前、子随后。
///
/// 虚拟列表只能渲染「一个扁平的可见行序列」；这条用例把今天的序列钉住——
/// 重构后 List 的 item 源必须是同一个顺序（否则展开语义与键盘导航一起变）。
#[gpui_kit::test]
fn visible_rows_follow_expansion_parent_before_child(cx: &mut gpui_kit::TestAppContext) {
    let conn = "G_1";
    let (view, cx) = open_nav_view(cx, conn);
    cx.update(|_window, cx| {
        seed_reveal_state(
            &view.read(cx),
            conn,
            "shop",
            "public",
            vec![
                table_node(conn, "shop", "public", "customers"),
                table_node(conn, "shop", "public", "orders"),
            ],
            None,
        );
        // 连接未展开：只有连接行本身可见（我们播种的 catalog/schema 都还没展开）
        view.read(cx).nav.borrow_mut().expanded.clear();
    });
    redraw(cx);
    assert_eq!(
        cx.update(|_window, cx| visible_keys(&view.read(cx))),
        vec![conn.to_string()],
        "未展开时只有连接行"
    );

    // 展开连接 → catalog 行；再依次展开 catalog / schema / 表文件夹 → 对象行
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            for key in [
                conn,
                "G_1/shop",
                "G_1/shop/public",
                "G_1/shop/public/tables",
            ] {
                view.nav.borrow_mut().expanded.insert(key.to_string());
            }
            cx.notify();
        });
    });
    redraw(cx);
    assert_eq!(
        cx.update(|_window, cx| visible_keys(&view.read(cx))),
        ids(&[
            "G_1",
            "G_1/shop",
            "G_1/shop/public",
            "G_1/shop/public/tables",
            "G_1/shop/public/customers",
            "G_1/shop/public/orders",
        ]),
        "父在前、子随后，且叶子按已加载顺序"
    );
}

/// 收集（S1 的 flatten）得到的全部可见行（含分组头 / 「更多」/「已定位」）。
fn collected_rows(view: &gpui_kit::Entity<super::NavView>, cx: &mut gpui_kit::App) -> Vec<NavRow> {
    view.update(cx, |view, cx| view.collect_nav_rows(None, cx))
}

/// 行序里**可漫游**的那些（键盘 ↑↓ 走得到的序列）。
///
/// 契约：它必须与 `nav_order`（[`super::NavView::sync_nav_order`] 的投影）逐步相等。
/// 分组头是容器标题（`selectable()` 为假）；引用行 / 「更多」/「已定位」行
/// 可点可选中，所以**进**序列（旧口径漏列会让 ↓ 跳过一行看得见的行）。
fn roam_keys(rows: &[NavRow]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.selectable())
        .map(|row| row.key())
        .collect()
}

/// 一行行的键（包括分组头等不可漫游的行）。
fn all_keys(rows: &[NavRow]) -> Vec<String> {
    rows.iter().map(|row| row.key()).collect()
}

/// 往搜索框里打字。
///
/// `filter` 的唯一入口就是输入框——`render_nav` 每帧从它重算，直接改状态会被下一帧冲掉。
fn set_search_text(
    view: &gpui_kit::Entity<super::NavView>,
    text: &str,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    view.update(cx, |view, cx| {
        let input = view.nav_search.clone().expect("搜索框在首帧创建");
        let text = text.to_string();
        input.update(cx, |state, cx| state.set_value(text, window, cx));
        cx.notify();
    });
}

/// 重画一帧 + 收集一次，断言「行序的可漫游部分 = 漫游投影（`nav_order`）」。
fn assert_collect_matches_render(
    view: &gpui_kit::Entity<super::NavView>,
    cx: &mut gpui_kit::VisualTestContext,
    phase: &str,
) -> Vec<NavRow> {
    redraw(cx);
    let projected = cx.update(|_window, cx| visible_keys(&view.read(cx)));
    let rows = cx.update(|_window, cx| collected_rows(view, cx));
    assert_eq!(
        roam_keys(&rows),
        projected,
        "阶段「{phase}」：漫游投影与可见行序不一致"
    );
    rows
}

/// 虚拟列表真的把行**画到页面上**了（有元素、有非零尺寸、上下顺序对）。
///
/// 为何单独一条：状态类断言看不见这一层——行集合算对了、漫游序列也对，但列表
/// 完全可能因为高度算成 0 / 容器没高度而什么都不画（而两者都会让其它用例保持绿色）。
#[gpui_kit::test]
fn nav_tree_rows_are_laid_out_by_the_virtual_list(cx: &mut gpui_kit::TestAppContext) {
    let conn = "G_1";
    let (view, cx) = open_nav_view(cx, conn);
    cx.update(|_window, cx| {
        seed_reveal_state(
            &view.read(cx),
            conn,
            "shop",
            "public",
            vec![table_node(conn, "shop", "public", "customers")],
            None,
        );
        let mut state = view.read(cx).nav.borrow_mut();
        state.expanded.clear();
        for key in [conn, "G_1/shop"] {
            state.expanded.insert(key.to_string());
        }
    });
    redraw(cx);
    let header = cx.debug_bounds("nav-row-group").expect("分组头应被画出");
    let conn_row = cx.debug_bounds("nav-row-conn").expect("连接行应被画出");
    let tree_row = cx.debug_bounds("nav-row-tree").expect("对象行应被画出");
    assert!(
        header.size.height > Pixels::ZERO
            && conn_row.size.height > Pixels::ZERO
            && tree_row.size.height > Pixels::ZERO,
        "行高由列表按估算高度定（0 = 估出来了也没生效）"
    );
    assert!(
        conn_row.origin.y > header.origin.y && tree_row.origin.y > conn_row.origin.y,
        "自上而下：分组头 → 连接行 → 对象行"
    );
    assert!(
        conn_row.size.height > tree_row.size.height,
        "行高分级：连接行（{}rem）＞ 对象行（{}rem）",
        super::ui::NAV_ROW_CONNECTION,
        super::ui::NAV_ROW_TREE
    );
}

/// 窗口级：定位到**屏外**的行时，列表真的滚过去了（虚拟列表换来的能力）。
///
/// 为何单独一条：以前树是自绘递归，定位只能保证「选中 + 渲染窗口罩住目标」，
/// 目标仍在屏外时看上去像没动作。现在多了可编程滚动入口，这条把它钉住。
#[gpui_kit::test]
fn reveal_scrolls_a_row_that_is_out_of_view(cx: &mut gpui_kit::TestAppContext) {
    let conn = "G_1";
    let (view, cx) = open_nav_view(cx, conn);
    let last = "t59";
    cx.update(|_window, cx| {
        let tables: Vec<NavNode> = (0..60)
            .map(|i| table_node(conn, "shop", "public", &format!("t{i}")))
            .collect();
        seed_reveal_state(&view.read(cx), conn, "shop", "public", tables, None);
    });
    redraw(cx);
    let before = cx.update(|_window, cx| view.read(cx).list_scroll.base_handle().offset().y);
    assert_eq!(before, Pixels::ZERO, "还没定位时不该自己滚");

    let object = ObjectRef::new(conn, ObjectKind::Table, "shop", "public", "", last);
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| view.reveal_ref(&object, cx));
    });
    redraw(cx);

    let (selected, after) = cx.update(|_window, cx| {
        (
            view.read(cx).nav.borrow().selected_key.clone(),
            view.read(cx).list_scroll.base_handle().offset().y,
        )
    });
    assert_eq!(
        selected.as_deref(),
        Some(NavNode::child_key(conn, &["shop", "public", last]).as_str()),
        "定位应选中目标行"
    );
    assert!(
        after < Pixels::ZERO,
        "目标在屏外时要把列表滚下去（gpui 的滚动偏移向下为负，实际 {after:?}）"
    );
}

/// S1 验收：`NavView::collect_nav_rows` 与渲染期累积的 `nav_order` **逐步相等**。
///
/// 两套遍历同时存在时这条用例才有意义：渲染走旧的递归（`render_*` 边画边 push），
/// 收集走新的 flatten。只测「展开」那一种不够——分组 / 引用行 / 分页 / 定位窗口 /
/// 过滤跳支 都是各自一段独立分支，漏一条就会在切到虚拟列表后才猸出来。
#[gpui_kit::test]
fn collected_rows_match_render_order(cx: &mut gpui_kit::TestAppContext) {
    let conn = "G_1";
    let catalog_key = "G_1/shop";
    let schema_key = "G_1/shop/public";
    let folder_key = "G_1/shop/public/tables";
    let group_header = format!("group:{}", engine::persistence::UNGROUPED_SCOPE);
    let (view, cx) = open_nav_view(cx, conn);
    cx.update(|_window, cx| {
        seed_reveal_state(
            &view.read(cx),
            conn,
            "shop",
            "public",
            vec![
                table_node(conn, "shop", "public", "customers"),
                table_node(conn, "shop", "public", "orders"),
            ],
            None,
        );
        view.read(cx).nav.borrow_mut().expanded.clear();
    });
    // 第一帧：未展开——未分组头 + 连接行
    let rows = assert_collect_matches_render(&view, cx, "未展开");
    assert_eq!(all_keys(&rows), ids(&[&group_header, conn]));

    // 逐层展开：父在前、子随后
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            for key in [conn, catalog_key, schema_key, folder_key] {
                view.nav.borrow_mut().expanded.insert(key.to_string());
            }
            cx.notify();
        });
    });
    let rows = assert_collect_matches_render(&view, cx, "逐层展开");
    assert_eq!(
        all_keys(&rows),
        ids(&[
            &group_header,
            conn,
            catalog_key,
            schema_key,
            folder_key,
            "G_1/shop/public/customers",
            "G_1/shop/public/orders",
        ])
    );

    // 分页未拉完：数据侧 5 条、只加载了 2 条 → 末行是「加载更多」
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.nav
                .borrow_mut()
                .child_total
                .insert(folder_key.to_string(), 5);
            cx.notify();
        });
    });
    let rows = assert_collect_matches_render(&view, cx, "分页未拉完");
    assert!(
        matches!(rows.last(), Some(NavRow::More { .. })),
        "末行应该是「加载更多」，实际：{:?}",
        all_keys(&rows)
    );

    // 定位窗口：「已定位」行在子节点前，且这一窗里不摆「加载更多」
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.nav
                .borrow_mut()
                .jumped
                .insert(folder_key.to_string(), 3);
            cx.notify();
        });
    });
    let rows = assert_collect_matches_render(&view, cx, "定位窗口");
    let jump_at = rows
        .iter()
        .position(|row| matches!(row, NavRow::Jump { .. }))
        .expect("定位窗口应有「已定位」行");
    let folder_at = rows
        .iter()
        .position(|row| row.key() == folder_key)
        .expect("文件夹行应该在");
    assert_eq!(
        jump_at,
        folder_at + 1,
        "「已定位」行紧跟在文件夹行之后、子节点之前"
    );
    assert!(
        !rows.iter().any(|row| matches!(row, NavRow::More { .. })),
        "定位窗口里不摆「加载更多」（按条数往后翻会跳错地方）"
    );

    // 过滤：连接名不命中时整棵树会空，所以让标签命中连接；
    // 文件夹名命中但两张表不命中（分页未拉完 → 本地过滤不得把整支隐藏）。
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut state = view.nav.borrow_mut();
            state.jumped.clear();
            state.tags.insert(conn.to_string(), vec!["表".to_string()]);
            cx.notify();
        });
        set_search_text(&view, "表", window, cx);
    });
    let rows = assert_collect_matches_render(&view, cx, "过滤跳支");
    assert_eq!(
        all_keys(&rows),
        ids(&[
            &group_header,
            conn,
            catalog_key,
            schema_key,
            folder_key,
            "G_1/shop/public/tables#more",
        ]),
        "两张表不命中被跳过；分页未拉完的文件夹仍可见（否则连「加载更多」都点不到）"
    );

    // 同一连接在两个分组：主组是全亮行（带子树），另一个组只指路（引用行）。
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut state = view.nav.borrow_mut();
            state.tags.clear();
            state.child_total.clear();
            state.groups = vec![
                engine::persistence::ConnectionGroup {
                    id: "g1".to_string(),
                    name: "生产".to_string(),
                    description: None,
                    sort_order: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                engine::persistence::ConnectionGroup {
                    id: "g2".to_string(),
                    name: "备份".to_string(),
                    description: None,
                    sort_order: 1,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            ];
            state
                .membership
                .insert(conn.to_string(), vec!["g1".to_string(), "g2".to_string()]);
            state
                .group_order
                .insert("g1".to_string(), vec![conn.to_string()]);
            state
                .group_order
                .insert("g2".to_string(), vec![conn.to_string()]);
            cx.notify();
        });
        set_search_text(&view, "", window, cx);
    });
    let rows = assert_collect_matches_render(&view, cx, "双分组");
    assert_eq!(
        all_keys(&rows),
        ids(&[
            "group:g1",
            conn,
            catalog_key,
            schema_key,
            folder_key,
            "G_1/shop/public/customers",
            "G_1/shop/public/orders",
            "group:g2",
            "ref:g2:G_1",
            &group_header,
        ]),
        "全亮行只在主组出现一次；另一个组是引用行（无子树）；未分组头即使为空也在"
    );

    // 折叠主组：只剩另一个组的引用行
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.nav
                .borrow_mut()
                .collapsed_groups
                .insert("g1".to_string());
            cx.notify();
        });
    });
    let rows = assert_collect_matches_render(&view, cx, "折叠主组");
    assert_eq!(
        all_keys(&rows),
        ids(&["group:g1", "group:g2", "ref:g2:G_1", &group_header])
    );

    // 无匹配：所有行都不见了（空态由调用方另行呈现）。
    // 注意搜索词只有 1 个字：`nav_search_query_ready` 门槛是 2，否则会排一次索引搜索（要读库）。
    cx.update(|window, cx| set_search_text(&view, "z", window, cx));
    let rows = assert_collect_matches_render(&view, cx, "无匹配");
    assert!(rows.is_empty(), "无匹配时收集不到任何行");
}

/// 窗口级：`reveal_ref` 把链路**逐层展开**并选中目标，且收尾干净。
#[gpui_kit::test]
fn reveal_ref_expands_the_chain_and_selects_the_target(cx: &mut gpui_kit::TestAppContext) {
    let conn = "G_1";
    let (view, cx) = open_nav_view(cx, conn);
    let target = NavNode::child_key(conn, &["shop", "public", "orders"]);
    cx.update(|_window, cx| {
        seed_reveal_state(
            &view.read(cx),
            conn,
            "shop",
            "public",
            vec![
                table_node(conn, "shop", "public", "customers"),
                table_node(conn, "shop", "public", "orders"),
            ],
            None,
        );
    });

    let object = ObjectRef::new(conn, ObjectKind::Table, "shop", "public", "", "orders");
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| view.reveal_ref(&object, cx));
    });

    cx.update(|_window, cx| {
        let nav = view.read(cx).nav.borrow();
        assert_eq!(
            nav.selected_key.as_deref(),
            Some(target.as_str()),
            "定位应选中目标行"
        );
        for key in [
            conn,
            "G_1/shop",
            "G_1/shop/public",
            "G_1/shop/public/tables",
        ] {
            assert!(
                nav.expanded.contains(key),
                "{key} 应被展开（否则选中了也看不到）"
            );
        }
        // 可选：目标已是**已加载**的行，于是会进可见行（以前靠抬渲染窗口，
        // 现在已加载的行全进列表，虚拟滚动只画视口内那几行）
        assert!(nav.reveal.is_none(), "成功后意图要收尾，不能挂着");
        assert!(nav.reveal_note.is_none(), "成功不该留提示");
    });
    let rows = cx.update(|_window, cx| collected_rows(&view, cx));
    assert!(
        rows.iter().any(|row| row.key() == target),
        "目标行应在可见行里，否则选中了也在屏外"
    );
}

/// 窗口级：目标在**已全部加载**的文件夹里也找不到时，如实说一句并不留悬念。
#[gpui_kit::test]
fn reveal_ref_reports_when_the_object_is_missing(cx: &mut gpui_kit::TestAppContext) {
    let conn = "G_1";
    let (view, cx) = open_nav_view(cx, conn);
    cx.update(|_window, cx| {
        seed_reveal_state(
            &view.read(cx),
            conn,
            "shop",
            "public",
            vec![table_node(conn, "shop", "public", "customers")],
            // 全量已加载（total == loaded）→ 不可能再翻页，只能如实说没有
            Some(1),
        );
    });

    let object = ObjectRef::new(conn, ObjectKind::Table, "shop", "public", "", "orders");
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| view.reveal_ref(&object, cx));
    });

    cx.update(|_window, cx| {
        let nav = view.read(cx).nav.borrow();
        assert!(nav.reveal.is_none(), "找不到必须收尾（不能永远转圈）");
        let note = nav.reveal_note.clone().expect("要给一句可读说明");
        assert!(note.contains("未在索引里找到"), "说明要具体：{note}");
    });
}

/// 窗口级：大 schema（分页）下目标不在已加载窗口里 → 应发出「跳页」请求并等回执。
#[gpui_kit::test]
fn reveal_ref_asks_for_the_target_page_in_a_paged_folder(cx: &mut gpui_kit::TestAppContext) {
    let conn = "G_1";
    let (view, cx) = open_nav_view(cx, conn);
    cx.update(|_window, cx| {
        seed_reveal_state(
            &view.read(cx),
            conn,
            "shop",
            "public",
            vec![table_node(conn, "shop", "public", "customers")],
            // 数据侧还有一大截：目标可能在没取回来的那部分里
            Some(600),
        );
    });

    let object = ObjectRef::new(conn, ObjectKind::Table, "shop", "public", "", "orders");
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| view.reveal_ref(&object, cx));
    });

    cx.update(|_window, cx| {
        let nav = view.read(cx).nav.borrow();
        let target = nav.reveal.as_ref().expect("分页时要挂着待完成的定位");
        assert!(target.jumping, "应已发出跳页请求（位次 → 那一页）并等回执");
        assert!(nav.reveal_note.is_none(), "还没到放弃的时候，不该先写提示");
    });
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// 造一条搜索命中（字段就是 `nav_jobs::SearchHit` 的形状）。
fn hit(
    object_type: &str,
    name: &str,
    parent: Option<&str>,
    schema: Option<&str>,
    catalog: Option<&str>,
) -> crate::nav_jobs::SearchHit {
    crate::nav_jobs::SearchHit {
        conn_id: "G_1".into(),
        conn_label: "shop".into(),
        driver: "mysql".into(),
        object_type: object_type.into(),
        object_name: name.into(),
        parent_name: parent.map(str::to_string),
        catalog: catalog.map(str::to_string),
        schema: schema.map(str::to_string),
        snippet: None,
    }
}

/// 定位目标 → 树节点 key / 层级路径必须与 `object_nodes` 造 key 的口径**逐字一致**
/// （`conn/catalog/schema/名字`）——差一段就会「点定位没反应」或选到别的节点。
#[test]
fn reveal_target_maps_search_hits_onto_tree_keys() {
    // 表：key = conn/catalog/schema/表名；层级 = 连接 → catalog → schema → 表文件夹
    let t = RevealTarget::from_hit(&hit("table", "orders", None, Some("public"), Some("shop")))
        .expect("表可定位");
    assert_eq!(t.node_key("shop"), "G_1/shop/public/orders");
    assert_eq!(
        t.levels("shop")
            .into_iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>(),
        ids(&[
            "G_1",
            "G_1/shop",
            "G_1/shop/public",
            "G_1/shop/public/tables"
        ])
    );

    // 列：key 多一层所属表（列节点挂在表节点下，所以还要展开那张表）
    let c = RevealTarget::from_hit(&hit(
        "column",
        "amount",
        Some("orders"),
        Some("public"),
        Some("shop"),
    ))
    .expect("列可定位");
    assert_eq!(c.node_key("shop"), "G_1/shop/public/orders/amount");

    // 视图与表同 schema 但分属不同文件夹：末层必须落在 /views
    let v = RevealTarget::from_hit(&hit("view", "v_orders", None, Some("public"), Some("shop")))
        .expect("视图可定位");
    assert_eq!(
        v.levels("shop").last().map(|(k, _)| k.clone()),
        Some("G_1/shop/public/views".to_string())
    );

    // schema：目标就是 schema 节点，不再往文件夹里走（层级只到 catalog）
    let s = RevealTarget::from_hit(&hit("schema", "public", None, Some("public"), Some("shop")))
        .expect("schema 可定位");
    assert_eq!(s.node_key("shop"), "G_1/shop/public");
    assert_eq!(
        s.levels("shop").len(),
        2,
        "schema 目标只需连接与 catalog 两层"
    );

    // 例程 / 序列 / 触发器各有自己的文件夹（它们不分页，但也得能选中）
    for (kind, folder) in [
        ("routine", "routines"),
        ("sequence", "sequences"),
        ("trigger", "triggers"),
    ] {
        let t = RevealTarget::from_hit(&hit(kind, "obj", None, Some("public"), Some("shop")))
            .unwrap_or_else(|| panic!("{kind} 应可定位"));
        assert_eq!(
            t.levels("shop").last().map(|(k, _)| k.clone()),
            Some(format!("G_1/shop/public/{folder}")),
            "{kind} 应落在 {folder} 文件夹"
        );
    }
}

/// 两个入口（导航搜索结果行 / Quick Open 的 `⌥↵`）必须落到**同一个**定位目标。
///
/// 它们是两条不同的路径：一条拿字符串类别（落库口径），一条拿 `ObjectKind`（枚举口径）。
/// 映射真只写了一处的话，两条路算出的东西应当逐字段相等——不等就说明有人加了第二套判定。
#[test]
fn reveal_ref_and_hit_resolve_to_the_same_target() {
    let cases = [
        ("table", "orders", None, ObjectKind::Table),
        ("view", "v_orders", None, ObjectKind::View),
        ("column", "amount", Some("orders"), ObjectKind::Column),
        ("routine", "fn_x", None, ObjectKind::Routine),
        ("sequence", "seq_a", None, ObjectKind::Sequence),
        ("trigger", "trg_a", None, ObjectKind::Trigger),
        ("schema", "public", None, ObjectKind::Schema),
    ];
    for (type_str, name, parent, kind) in cases {
        let via_hit =
            RevealTarget::from_hit(&hit(type_str, name, parent, Some("public"), Some("shop")))
                .unwrap_or_else(|| panic!("{type_str} 从命中应可定位"));
        let via_ref = RevealTarget::from_ref(&ObjectRef::new(
            "G_1",
            kind,
            "shop",
            "public",
            parent.unwrap_or(""),
            name,
        ))
        .unwrap_or_else(|| panic!("{type_str} 从引用应可定位"));
        assert_eq!(via_hit, via_ref, "{type_str}：两条入口必须落到同一个目标");
    }
}

/// 不可定位的命中必须**当场判否**：结果行上那个入口就不摆出来，而不是点了没反应。
#[test]
fn reveal_target_refuses_unlocatable_hits() {
    assert!(
        RevealTarget::from_hit(&hit("weird_type", "x", None, Some("public"), Some("shop")))
            .is_none(),
        "类别不认识 → 拒绝"
    );
    assert!(
        RevealTarget::from_hit(&hit("table", "x", None, None, Some("shop"))).is_none(),
        "无 schema 归属 → 拒绝"
    );
    assert!(
        RevealTarget::from_hit(&hit("table", "x", None, Some(""), Some("shop"))).is_none(),
        "空 schema 等同没有（FTS 的空串不是 NULL）→ 拒绝"
    );

    // 内容档命中没有 catalog（FTS 表里没这一列）——不是拒绝理由：定位时从连接下已加载的
    // catalog 取；这条差异必须写死在测试里，否则将来会有人把它当「不可定位」过滤掉。
    let fts = RevealTarget::from_hit(&hit("table", "orders", None, Some("public"), None))
        .expect("内容档命中同样可定位");
    assert!(fts.catalog.is_empty(), "catalog 留空，由定位时解析");

    // 引用入口的拒绝面（与命中入口一致）
    assert!(
        RevealTarget::from_ref(&ObjectRef::new(
            "G_1",
            ObjectKind::Catalog,
            "shop",
            "",
            "",
            "shop"
        ))
        .is_none(),
        "catalog 层没有可定位的“那一行” → 拒绝"
    );
    assert!(
        RevealTarget::from_ref(&ObjectRef::new(
            "G_1",
            ObjectKind::Table,
            "shop",
            "",
            "",
            "t"
        ))
        .is_none(),
        "无 schema 归属 → 拒绝"
    );
}

/// 命中行的身份与树节点同构（差前缀与位次），且**同一对象两条命中也不撞车**。
///
/// 为何要单独铉这两条：行 key 既是漫游锚点（`nav_move` 按 `position` 找当前位置）
/// 又是 GPUI 的元素 id——重复的 key 会让键盘在几行之间来回弹，重复的 id 会串元素状态。
#[test]
fn search_hit_row_identity_matches_the_tree_and_stays_unique() {
    let h = hit("table", "orders", None, Some("public"), Some("shop"));
    let tree_key = NavNode::child_key("G_1", &["shop", "public", "orders"]);
    let row = NavRow::SearchHit {
        hit: Box::new(h.clone()),
        ix: 0,
    };
    assert_eq!(row.key(), search_hit_row_key(0, &h));
    assert!(
        row.key().ends_with(&tree_key),
        "命中行身份沿用统一引用（与树上那一行说的是同一个对象）：{}",
        row.key()
    );
    assert!(row.selectable(), "命中行进漫游序列（↑↓ 走得到）");
    assert_eq!(row.conn_id(), Some("G_1"), "属性面板要靠它找连接标签");
    assert!(
        row.element_id().contains(&tree_key),
        "元素 id 同样认得这个对象：{}",
        row.element_id()
    );
    assert_ne!(
        search_hit_row_key(0, &h),
        search_hit_row_key(1, &h),
        "同一条命中的两个位次必须不同 key（否则漫游锚点撞车）"
    );
    assert_ne!(search_hit_row_id(0, &h), search_hit_row_id(1, &h));
    // 索引里可能出现不可寻址的类别串（渲染得出来、定位不了）：身份仍要唯一，不能是空串。
    let weird = hit("weird_type", "x", None, None, None);
    assert!(!weird.key().is_empty());
}

/// 窗口级：`Esc`（`NavClearSearch`）只清搜索框，facet 与选中不动。
///
/// 为何要铉：与资产库的 `ClearSearch` 同一条口径——facet 在 chips 与「筛选 ▾」里看得见，
/// 顺手把它们也清了，用户会以为筛选坏了（原型设计 §6.3）。
#[gpui_kit::test]
fn clear_search_action_clears_only_the_search_box(cx: &mut gpui_kit::TestAppContext) {
    let conn = "G_1";
    let (view, cx) = open_nav_view(cx, conn);
    cx.update(|_window, cx| {
        seed_reveal_state(
            &view.read(cx),
            conn,
            "shop",
            "public",
            vec![table_node(conn, "shop", "public", "orders")],
            None,
        );
    });
    redraw(cx);

    cx.update(|window, cx| {
        set_search_text(&view, "or", window, cx);
        view.update(cx, |view, cx| {
            let mut nav = view.nav.borrow_mut();
            nav.search_query = Some("or".to_string());
            nav.search_hits = vec![hit("table", "orders", None, Some("public"), Some("shop"))];
            nav.search_searched = 1;
            nav.type_filter = Some("postgresql".to_string());
            cx.notify();
        });
    });
    redraw(cx);
    cx.update(|window, cx| {
        let handle = view.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(NavClearSearch), cx);
    });

    let (text, query, hits, type_filter) = cx.update(|_window, cx| {
        let view = view.read(cx);
        let text = view
            .nav_search
            .as_ref()
            .map(|s| s.read(cx).value().to_string());
        let nav = view.nav.borrow();
        (
            text,
            nav.search_query.clone(),
            nav.search_hits.len(),
            nav.type_filter.clone(),
        )
    });
    assert_eq!(text.as_deref(), Some(""), "搜索框要清空");
    assert_eq!(
        query, None,
        "索引搜索的查询词一起清（标题行不能永远停在旧词上）"
    );
    assert_eq!(hits, 0, "命中行随搜索词一起退场");
    assert_eq!(
        type_filter.as_deref(),
        Some("postgresql"),
        "facet 筛选不动（它在「筛选 ▾」里看得见，误清会让人以为筛选坏了）"
    );
}

/// 只有「连接中」徽标挂循环动画。
///
/// 为何要铉：循环动画每帧申请重绘，挂到稳态（已连 / 未连 / 失败）上 = 窗口永远在重绘，
/// 而截图与肉眼都看不出来。新增状态时不要顺手把它也挂上。
#[test]
fn only_the_connecting_badge_pulses() {
    assert!(nav_badge_pulses(NavBadgeStatus::Connecting));
    for quiet in [
        NavBadgeStatus::Connected,
        NavBadgeStatus::Failed,
        NavBadgeStatus::Idle,
    ] {
        assert!(
            !nav_badge_pulses(quiet),
            "稳定态不带动效（动效解释变化，不做常驻装饰）"
        );
    }
}

/// 窗口级：搜索结果行**在列表里**——进漫游序列（↑↓ 可达）、被虚拟列表真画出来、
/// 与树行共用同一片滚动区。
///
/// 为何要铉：以前结果区是虚拟列表**之外**一个自带 128px 上限的滚动块——键盘走不到、
/// 无选中反馈，`NAV_SEARCH_MAX_ROWS` = 100 条还要在那个小窗里二段滚动。
#[gpui_kit::test]
fn search_hit_rows_join_the_list_and_the_roaming_order(cx: &mut gpui_kit::TestAppContext) {
    let conn = "G_1";
    let (view, cx) = open_nav_view(cx, conn);
    cx.update(|_window, cx| {
        seed_reveal_state(
            &view.read(cx),
            conn,
            "shop",
            "public",
            vec![table_node(conn, "shop", "public", "orders")],
            None,
        );
    });
    redraw(cx);

    // 结果回填（真实路径是后台搜索结果；这里把状态摆成“刚回来”）。
    // 搜索框文字必须与 `search_query` 一致：不一致时下一帧会当成“要新搜一次”而清空结果（还要读库）。
    let hits = vec![
        hit("table", "orders", None, Some("public"), Some("shop")),
        hit(
            "column",
            "order_id",
            Some("orders"),
            Some("public"),
            Some("shop"),
        ),
    ];
    cx.update(|window, cx| {
        set_search_text(&view, "or", window, cx);
        view.update(cx, |view, cx| {
            let mut nav = view.nav.borrow_mut();
            nav.search_query = Some("or".to_string());
            nav.search_hits = hits.clone();
            nav.search_searched = 1;
            cx.notify();
        });
    });

    let rows = assert_collect_matches_render(&view, cx, "结果回填");
    let hit_keys = ids(&[
        "search:0:G_1/shop/public/orders",
        "search:1:G_1/shop/public/orders/order_id",
    ]);
    assert_eq!(
        &all_keys(&rows)[..2],
        hit_keys.as_slice(),
        "命中行排在列表最前（与旧的「结果区在树顶」同观感），且每行一个位次"
    );
    // 漫游序列真的把它们收了进去：↑↓ 从零开始往下走，第一站就是第一条命中。
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| view.nav_move(1, cx));
    });
    assert_eq!(
        cx.update(|_window, cx| view.read(cx).nav.borrow().selected_key.clone()),
        Some(hit_keys[0].clone()),
        "↑↓ 要能走到命中行（结果区不再是键盘死区）"
    );

    // 行真的被画在页面上（有元素、有非零尺寸）：状态对了但列表不画是另一回事。
    let drawn = cx
        .debug_bounds("nav-row-search-hit")
        .expect("命中行应被虚拟列表画出");
    assert!(
        drawn.size.height > Pixels::ZERO,
        "命中行高度由列表按估算高度定（0 = 估出来了也没生效）"
    );
}

/// 「结构洞察」的靶：表 / 视图用它所在的 schema，schema 节点用自己；列 / 例行与
/// catalog 节点不给（后者要跨方言的“全部 schema”语义，未定）。
#[test]
fn schema_insight_target_covers_tables_views_and_schemas() {
    let table_path = NavPath::Table {
        catalog: "shop".into(),
        schema: "public".into(),
        table: "orders".into(),
    };
    let expect = |target: Option<ObjectRef>| {
        let t = target.expect("应当给靶");
        assert_eq!(t.conn_id, "G_1");
        assert_eq!(t.catalog, "shop");
        assert_eq!(t.schema, "public");
        assert_eq!(t.kind, ObjectKind::Schema, "结构洞察的靶一律是 Schema 引用");
    };

    expect(insight_schema_target(
        &NavNodeKind::Table { row_estimate: None },
        Some(&table_path),
        "G_1",
    ));
    expect(insight_schema_target(
        &NavNodeKind::View,
        Some(&table_path),
        "G_1",
    ));
    expect(insight_schema_target(
        &NavNodeKind::Schema,
        Some(&NavPath::Schema {
            catalog: "shop".into(),
            schema: "public".into(),
        }),
        "G_1",
    ));

    // 列 / 例行（没有 schema 归属）/ catalog（跨方言语义未定）都不给
    assert!(
        insight_schema_target(
            &NavNodeKind::Column {
                data_type: "int".into(),
                nullable: false,
                primary: false,
                foreign: false,
            },
            None,
            "G_1",
        )
        .is_none()
    );
    assert!(
        insight_schema_target(
            &NavNodeKind::Catalog,
            Some(&NavPath::Catalog {
                catalog: "shop".into(),
            }),
            "G_1",
        )
        .is_none()
    );
}

/// 索引命中 → 属性定位：表带 catalog/schema，列带所属表（parent），未知类别不接。
#[test]
fn search_hit_maps_to_property_ref() {
    let hit = |object_type: &str, name: &str, parent: Option<&str>| crate::nav_jobs::SearchHit {
        conn_id: "P_conn_hit".to_string(),
        conn_label: "本地库".to_string(),
        driver: "duckdb".to_string(),
        object_type: object_type.to_string(),
        object_name: name.to_string(),
        parent_name: parent.map(str::to_string),
        catalog: Some("main".to_string()),
        schema: Some("public".to_string()),
        snippet: None,
    };

    let table = nav_search_hit_property(&hit("table", "orders", None)).expect("表命中应可定位");
    assert_eq!(table.kind, PropertyKind::Table);
    assert_eq!(table.name, "orders");
    assert_eq!(table.catalog.as_deref(), Some("main"));
    assert_eq!(table.schema.as_deref(), Some("public"));
    assert_eq!(table.parent, None);
    assert_eq!(table.source, NavSource::from_conn_id("P_conn_hit"));

    let col = nav_search_hit_property(&hit("column", "order_id", Some("orders")))
        .expect("列命中应可定位");
    assert_eq!(col.kind, PropertyKind::Column);
    assert_eq!(
        col.parent.as_deref(),
        Some("orders"),
        "列必须带所属表（属性面板靠它区分是哪张表的列）"
    );

    // 未知类别（如索引里可能出现的 index / routine）不提供属性定位
    assert!(nav_search_hit_property(&hit("index", "idx_a", None)).is_none());
}

/// 搜索词门槛：≥ 2 个字符（含空白裁剪）；单字符不打搜索（命中面过大且几乎必然还要改）。
#[test]
fn search_query_requires_two_chars() {
    assert!(!nav_search_query_ready(""));
    assert!(!nav_search_query_ready("o"));
    assert!(!nav_search_query_ready("  o  "));
    assert!(nav_search_query_ready("or"));
    assert!(nav_search_query_ready(" 订单 "));
}

/// 命中行的类别短标签（给用户看的中文；未知类别兜到「对象」）。
#[test]
fn object_type_labels_are_localized() {
    assert_eq!(nav_object_type_label("table"), "表");
    assert_eq!(nav_object_type_label("view"), "视图");
    assert_eq!(nav_object_type_label("schema"), "模式");
    assert_eq!(nav_object_type_label("column"), "列");
    assert_eq!(
        nav_object_type_label("whatever"),
        "对象",
        "未知类别不得显示成空白"
    );
}

/// Mock / 洞察表入口的引用靶：视图必须标成 `View`（曾经一律标成 `Table`）。
#[test]
fn data_target_kind_follows_the_node() {
    let target = |kind: NavNodeKind| {
        nav_data_target(
            &kind,
            "P_1",
            "shop".into(),
            "public".into(),
            "orders".into(),
        )
    };
    assert_eq!(
        target(NavNodeKind::Table { row_estimate: None })
            .expect("表应给靶")
            .kind,
        ObjectKind::Table
    );
    assert_eq!(
        target(NavNodeKind::View).expect("视图应给靶").kind,
        ObjectKind::View,
        "视图不能标成表（跨屏身份靠 kind 区分）"
    );
    assert!(target(NavNodeKind::Schema).is_none(), "非数据类节点不给靶");
}

/// `ObjectRef` 的 key 与导航树节点 key **同构**——这是搜索命中与树节点
/// 判定“是不是同一个对象”的契约（将来「在树中定位」直接靠它，不再各拼一套）。
#[test]
fn object_ref_key_matches_nav_node_key() {
    // 表：树上的 key = child_key(conn, [catalog, schema, name])
    let table = ObjectRef::table("P_1", "shop", "public", "orders");
    assert_eq!(
        table.key(),
        NavNode::child_key("P_1", &["shop", "public", "orders"])
    );

    // 列：树上的 key = child_key(conn, [catalog, schema, 表, 列])
    let column = ObjectRef::column("P_1", "shop", "public", "orders", "order_id");
    assert_eq!(
        column.key(),
        NavNode::child_key("P_1", &["shop", "public", "orders", "order_id"])
    );

    // schema：树上的 key = child_key(conn, [catalog, schema])
    let schema = ObjectRef::schema("G_1", "shop", "public");
    assert_eq!(schema.key(), NavNode::child_key("G_1", &["shop", "public"]));

    // 无 Catalog 层的驱动：导航侧把 catalog 同时当 schema（`load_folders(conn, c, c)`），
    // 因此两边都拿到重复段——关键是一致，不是好看。
    let no_schema = ObjectRef::table("G_1", "mall", "mall", "order");
    assert_eq!(
        no_schema.key(),
        NavNode::child_key("G_1", &["mall", "mall", "order"])
    );
}

/// 索引命中 → 统一引用 → 属性定位：走完这条通路后，字段与树节点那条路完全一致。
#[test]
fn search_hit_ref_agrees_with_tree_side_ref() {
    let hit = crate::nav_jobs::SearchHit {
        conn_id: "P_1".to_string(),
        conn_label: "本地库".to_string(),
        driver: "postgres".to_string(),
        object_type: "column".to_string(),
        object_name: "order_id".to_string(),
        parent_name: Some("orders".to_string()),
        catalog: Some("shop".to_string()),
        schema: Some("public".to_string()),
        snippet: None,
    };
    let from_hit = nav_search_hit_ref(&hit).expect("列命中应可寻址");
    let from_tree = ObjectRef::column("P_1", "shop", "public", "orders", "order_id");
    assert_eq!(
        from_hit.key(),
        from_tree.key(),
        "搜索与树必须给出同一个 key"
    );
    assert_eq!(from_hit, from_tree);
}

/// 分页追加：按 key 去重，并返回新增条数（追加方靠它判断“是否翻到底”）。
#[test]
fn merge_page_dedupes_by_key_and_reports_appended() {
    let node = |key: &str| {
        NavNode::new(
            key.to_string(),
            key.to_string(),
            "P_conn_merge",
            NavNodeKind::Table { row_estimate: None },
            false,
        )
    };

    let mut loaded = vec![node("a"), node("b")];
    // 与已有重叠一条（b）+ 新的一条（c）
    let appended = nav_merge_page(&mut loaded, vec![node("b"), node("c")]);
    assert_eq!(appended, 1, "只有 c 是新增");
    assert_eq!(
        loaded.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        vec!["a", "b", "c"],
        "顺序保持：已加载在前、新页在后"
    );

    // 整页重复（刷新后二次读交叠）：新增 0，长度不变
    assert_eq!(nav_merge_page(&mut loaded, vec![node("a")]), 0);
    assert_eq!(loaded.len(), 3, "重复节点不得再入列（元素 id 会冲突）");
}

#[test]
fn nav_reorder_moves_and_appends() {
    let base = ids(&["a", "b", "c"]);
    // 已在列表里：摘除后插到目标之前。
    assert_eq!(
        nav_reorder(&base, "c", Some("a")),
        Some(ids(&["c", "a", "b"]))
    );
    assert_eq!(
        nav_reorder(&base, "a", Some("c")),
        Some(ids(&["b", "a", "c"]))
    );
    // 不在列表里（新归组）：插到目标之前；无目标则追加。
    assert_eq!(
        nav_reorder(&base, "z", Some("b")),
        Some(ids(&["a", "z", "b", "c"]))
    );
    assert_eq!(
        nav_reorder(&base, "z", None),
        Some(ids(&["a", "b", "c", "z"]))
    );
    // 空容器：首个成员落在末尾。
    assert_eq!(nav_reorder(&[], "z", None), Some(ids(&["z"])));
}

#[test]
fn nav_step_swaps_neighbours_and_stops_at_bounds() {
    let base = ids(&["a", "b", "c"]);
    // 中间项上下各一步。
    assert_eq!(nav_step(&base, "b", -1), Some(ids(&["b", "a", "c"])));
    assert_eq!(nav_step(&base, "b", 1), Some(ids(&["a", "c", "b"])));
    // 末项上移 / 首项下移。
    assert_eq!(nav_step(&base, "c", -1), Some(ids(&["a", "c", "b"])));
    assert_eq!(nav_step(&base, "a", 1), Some(ids(&["b", "a", "c"])));
    // 边界：首项上移 / 末项下移 → 无变化。
    assert_eq!(nav_step(&base, "a", -1), None);
    assert_eq!(nav_step(&base, "c", 1), None);
    // 不在容器里（例如对象节点被过滤掉）→ 无变化。
    assert_eq!(nav_step(&base, "z", 1), None);
    assert_eq!(nav_step(&[], "z", 1), None);
}

#[test]
fn nav_order_members_puts_manual_first_then_names() {
    let name = |id: &str| match id {
        "P_b" => "zeta".to_string(),
        "P_c" => "Alpha".to_string(),
        "P_d" => "beta".to_string(),
        other => other.to_string(),
    };
    // 手动排序的两个按序号在前；未排的三个按名称（大小写不敏感）升序在后。
    let stored = vec![
        ("P_a".to_string(), Some(1)),
        ("P_b".to_string(), None),
        ("P_d".to_string(), Some(0)),
        ("P_c".to_string(), None),
        ("P_e".to_string(), None),
    ];
    assert_eq!(
        nav_order_members(&stored, name),
        ids(&["P_d", "P_a", "P_c", "P_e", "P_b"])
    );
    // 全未排：纯名称序。
    let stored = vec![("P_b".to_string(), None), ("P_c".to_string(), None)];
    assert_eq!(nav_order_members(&stored, name), ids(&["P_c", "P_b"]));
    // 全已排：按序号（与本传入顺序无关）。
    let stored = vec![("P_b".to_string(), Some(1)), ("P_a".to_string(), Some(0))];
    assert_eq!(nav_order_members(&stored, name), ids(&["P_a", "P_b"]));
    assert!(nav_order_members(&[], name).is_empty());
}

#[test]
fn nav_reorder_skips_no_op_moves() {
    let base = ids(&["a", "b", "c"]);
    // 落到自己身上。
    assert_eq!(nav_reorder(&base, "b", Some("b")), None);
    // 已经正好在目标之前。
    assert_eq!(nav_reorder(&base, "a", Some("b")), None);
    // 已在末尾且要追加到末尾。
    assert_eq!(nav_reorder(&base, "c", None), None);
    // 目标不在列表里（被并发删除）：退化为追加；已在末尾则不写库。
    assert_eq!(
        nav_reorder(&base, "a", Some("gone")),
        Some(ids(&["b", "c", "a"]))
    );
    assert_eq!(nav_reorder(&base, "c", Some("gone")), None);
}

#[test]
fn type_labels_prefer_the_catalog_over_the_builtin_table() {
    // 目录就绪：新增类型不必改代码就有正确名称与分类
    assert_eq!(
        nav_type_label("snowflake", Some(("Snowflake", "analytics"))),
        "Snowflake（分析型）"
    );
    // 目录里分类未知 / 为空：只给名称，不编造分类
    assert_eq!(nav_type_label("weird", Some(("WeirdDB", ""))), "WeirdDB");
    assert_eq!(
        nav_type_label("snowflake", Some(("Snowflake", "whatever"))),
        "Snowflake"
    );
    // 目录里的名称是空白 → 当没给（回退内置表）
    assert_eq!(
        nav_type_label("mysql", Some(("   ", "relational"))),
        "MySQL（关系型）"
    );

    // 目录未就绪 / 类型不在目录：内置表兜底，认不出就原样显示 id
    assert_eq!(nav_type_label("mysql", None), "MySQL（关系型）");
    assert_eq!(nav_type_label("postgresql", None), "PostgreSQL（关系型）");
    assert_eq!(nav_type_label("snowflake", None), "snowflake");

    // 短名（facet 菜单 / 筛选药丸）：目录名优先，无分类后缀
    assert_eq!(nav_type_short_label("mysql", None), "MySQL");
    assert_eq!(
        nav_type_short_label("snowflake", Some(("Snowflake", "analytics"))),
        "Snowflake"
    );
    assert_eq!(nav_type_short_label("snowflake", None), "snowflake");
}

#[test]
fn scope_tooltip_explains_each_domain() {
    // 归属域短码是系统事实但不是自解释的（原型设计 §2.3）：三档都要给一句人话。
    for (source, needle) in [
        (NavSource::Project, "项目"),
        (NavSource::Global, "全局"),
        (NavSource::Shared, "共享"),
    ] {
        let text = nav_scope_tooltip(source);
        assert!(text.starts_with("归属域："), "{text}");
        assert!(text.contains(needle), "{text}");
    }
}

#[test]
fn relative_time_reads_naturally_and_survives_clock_skew() {
    // 面板底状态行的「元数据更新于 X」文案（口径对齐 `analytics_resource::present`）。
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000_000);
    assert_eq!(nav_relative_time(now, now), "刚刚");
    assert_eq!(
        nav_relative_time(now, now - Duration::from_secs(59)),
        "刚刚"
    );
    assert_eq!(
        nav_relative_time(now, now - Duration::from_secs(60)),
        "1 分钟前"
    );
    assert_eq!(
        nav_relative_time(now, now - Duration::from_secs(5 * 60)),
        "5 分钟前"
    );
    assert_eq!(
        nav_relative_time(now, now - Duration::from_secs(3 * 3600)),
        "3 小时前"
    );
    assert_eq!(
        nav_relative_time(now, now - Duration::from_secs(2 * 86400)),
        "2 天前"
    );
    // 时钟回拨：不显示负数（给「刚刚」）。
    assert_eq!(
        nav_relative_time(now, now + Duration::from_secs(600)),
        "刚刚"
    );
}

#[test]
fn nav_kind_icons_encode_kind_by_shape_not_color() {
    // v8 修订点 1：类别靠**形状**辨认（颜色留给状态），所以每一类必须有自己的资产路径；
    // 且形状不随主题 / 状态变（展开态只换文件夹的开 / 合）。
    let table = NavNodeKind::Table { row_estimate: None };
    let view = NavNodeKind::View;
    let column = NavNodeKind::Column {
        data_type: "int".into(),
        nullable: true,
        primary: false,
        foreign: false,
    };
    let routine = NavNodeKind::Routine {
        routine_type: "FUNCTION".into(),
    };
    let shapes: Vec<(&str, &'static str)> = vec![
        ("catalog", nav_kind_icon(&NavNodeKind::Catalog, false)),
        ("schema", nav_kind_icon(&NavNodeKind::Schema, false)),
        (
            "folder-tables",
            nav_kind_icon(&NavNodeKind::Folder(NavFolder::Tables), false),
        ),
        ("table", nav_kind_icon(&table, false)),
        ("view", nav_kind_icon(&view, false)),
        ("column", nav_kind_icon(&column, false)),
        ("routine", nav_kind_icon(&routine, false)),
        ("sequence", nav_kind_icon(&NavNodeKind::Sequence, false)),
        ("trigger", nav_kind_icon(&NavNodeKind::Trigger, false)),
    ];
    let unique: HashSet<&str> = shapes.iter().map(|(_, path)| *path).collect();
    assert_eq!(
        unique.len(),
        shapes.len(),
        "每类一个形状，不允许两类共用同一资产：{shapes:?}"
    );
    // 同屏高频共现的三类不允许同形（表 / 视图 / 列）。
    assert_ne!(nav_kind_icon(&table, false), nav_kind_icon(&view, false));
    assert_ne!(nav_kind_icon(&table, false), nav_kind_icon(&column, false));
    // 连接根与 Catalog 都是「库」形状（连接行本身画徽标，不用这个图标）。
    let connection = NavNodeKind::Connection {
        source: NavSource::Project,
        driver: "postgres_native".into(),
        connected: true,
    };
    assert_eq!(
        nav_kind_icon(&connection, false),
        nav_kind_icon(&NavNodeKind::Catalog, false)
    );
    // 文件夹：展开 / 收起是两个形状（其余类别与展开态无关）。
    let folder = NavNodeKind::Folder(NavFolder::Tables);
    assert_ne!(nav_kind_icon(&folder, false), nav_kind_icon(&folder, true));
}

#[test]
fn type_badge_maps_known_types_and_falls_back() {
    // 已知类型：形状与 2 字母按映射表（原型设计 §2.3）。
    assert_eq!(
        nav_type_badge("postgresql"),
        ("icons/database.svg", "PG".into())
    );
    assert_eq!(nav_type_badge("sqlite"), ("icons/file.svg", "SQ".into()));
    assert_eq!(nav_type_badge("redis"), ("icons/braces.svg", "RD".into()));
    // 目录外类型：回退通用形状 + 类型名首 2 字母（大写）。
    let (path, letters) = nav_type_badge("snowflake");
    assert_eq!(path, "icons/database.svg");
    assert_eq!(letters, "SN");
    // 空类型 id：不做空字母，回退 `DB`。
    assert_eq!(nav_type_badge("").1, "DB");
}

#[test]
fn search_facets_parse_tokens_and_free_text() {
    let p = parse_nav_search("prod scope:global type:mysql tag:核心");
    assert_eq!(p.free, "prod");
    assert_eq!(p.source, Some(NavSource::Global));
    assert_eq!(p.db_type.as_deref(), Some("mysql"));
    assert_eq!(p.tag.as_deref(), Some("核心"));
    assert_eq!(p.active, 3);
    // `source:` 为 `scope:` 历史别名，短码亦可。
    let p = parse_nav_search("source:P driver:postgres_native");
    assert_eq!(p.source, Some(NavSource::Project));
    assert_eq!(p.driver.as_deref(), Some("postgres_native"));
    assert!(p.free.is_empty());
    // 空值 / 未识别前缀不吞字（保留在自由文本）。
    let p = parse_nav_search("tag: foo:bar");
    assert!(p.tag.is_none());
    assert_eq!(p.free, "tag: foo:bar");
    assert_eq!(p.active, 0);
}

#[test]
fn type_short_label_strips_category_suffix() {
    assert_eq!(nav_type_short_label("postgresql", None), "PostgreSQL");
    // 未知类型回退原 id。
    assert_eq!(nav_type_short_label("snowflake", None), "snowflake");
}
