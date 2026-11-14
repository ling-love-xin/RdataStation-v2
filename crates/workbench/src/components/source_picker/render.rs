//! 行渲染：三个根共用的一套「一行长什么样」。
//!
//! 边界（对齐 `workbench_shell::tree` 的边界声明）：
//! - **本模块只负责画**：给定 `Row` 列表 + 展开态，产出元素树；不决定哪些行可见
//!   （那是 `TreeState::visible_rows` 的事），也不做任何 I/O（render 是纯读路径）；
//! - 行的几何（缩进步长 / 展开槽宽 / 行高 / 激活条）一律走 `workbench_shell::tree`
//!   共用原语与 `ui` 常量，不在这里另立一套；
//! - 颜色一律 `cx.theme()`，没有裸色值。
//!
//! 这一刀**复用** `scratchpad` 的扩展名→图标映射（`scratchpad_type_icon` 是 `pub`，注释里
//! 就写明「供 workbench 作 `type_icon` 注入」）：在 workbench 侧再复制一份必然漂移。
//! 目录 / 表 / 连接用 Lucide 通用图标（`folder` / `table` / `database`），都是本仓
//! 已经在用的路径（`scratchpad_type_icon` 与它旁边的文件夹注释），不引未验证的名字。

use std::rc::Rc;

use gpui_kit::assets::IconNamed;
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use workbench_shell::{tree, ui};

use super::node::{NodeId, NodeKind, Row};
use super::state::TreeState;

/// 行点击回调。`window` 由回调自己 `refresh()`——渲染层不持有 `App`。
pub type RowAction = Rc<dyn Fn(&NodeId, &mut Window)>;

/// 图标路径（`IconName` 之外的那一类：`scratchpad` 给的是裸路径）。
///
/// 包一层是为了让 `Icon::new(...)` 吃得下——它要的是 `IconNamed`，不是 `&str`。
struct IconPath(SharedString);

impl IconNamed for IconPath {
    fn path(self) -> SharedString {
        self.0
    }
}

/// 一行的类型图标：目录 / 连接 / 表 / 临时表给通用标，文件走扩展名映射。
fn row_icon(row: &Row) -> IconPath {
    let path = match row.kind {
        NodeKind::Group | NodeKind::Folder => "icons/folder.svg",
        NodeKind::Conn => "icons/database.svg",
        NodeKind::Table | NodeKind::Temp => "icons/table.svg",
        NodeKind::File => scratchpad::scratchpad_view::scratchpad_type_icon(&row.label),
    };
    IconPath(SharedString::from(path))
}

/// 超过这个行数才切虚拟视口。
///
/// 常见的库是几十到一两百个对象；短列表保持「有多高画多高」（对话框跟着内容长），
/// 观感与虚拟化前**完全一致**。只有大库（深层 schema 展开后上千行）才值得上视口。
pub const VIRTUALIZE_AFTER: usize = 60;

/// 是否该切虚拟视口（纯函数，便于钉住阈值语义）。
pub fn needs_virtual_list(visible: usize) -> bool {
    visible > VIRTUALIZE_AFTER
}

/// 虚拟视口的高度（rem）：行数×行高，但不超过对话框列表上限。
///
/// 虚拟列表必须先拿到**确定高度**才能算可见区间；只给 `max_h` 的话它会退化成
/// 「整段铺开」，虚拟化就白做了。所以这里算出确切高度，短列表才走另一条路。
pub fn virtual_viewport_rem(visible: usize) -> f32 {
    (visible as f32 * ui::ROW_HEIGHT).min(ui::VIS_DIALOG_LIST_MAX_H)
}

/// 画一行（普通列表与虚拟列表共用，避免两条路各画一套导致漂移）。
///
/// `expanded` / `selected` 由调用方从树态取出传入——不在这里借 `TreeState`：
/// 虚拟列表的回调里已经握着对 `PickerState` 的借用，再借一次就是 `RefCell` 双借用 panic。
pub fn row_element(
    row: &Row,
    expanded: bool,
    selected: bool,
    on_toggle: &RowAction,
    on_select: &RowAction,
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let muted = colors.muted_foreground;
    let hover_bg = colors.list_hover;
    let selected_bg = colors.list_active;
    let primary = colors.primary;

    // 可展开但不可确认（组 / 连接 / 目录）与「可确认但被显式禁用」（全局 G_ 表）不同：
    // 后者必须看起来「在、但不能用」，而不是像导航层一样淡掉（§09 缺口）。
    let dimmed = row.kind.is_confirmable() && !row.selectable;
    let expandable = row.kind.is_expandable();

    let mut el = div()
        .id(ElementId::Name(SharedString::from(format!(
            "source-picker-row-{}",
            row.id.key()
        ))))
        .relative()
        .h_flex()
        .items_center()
        .w_full()
        .h(rems(ui::ROW_HEIGHT))
        .gap_1()
        .rounded_sm()
        .when(selected, |d| d.bg(selected_bg))
        // 悬停只在未选中时生效（不得盖掉选中底色）——与导航树、`ui-design-spec` §4 同一口径
        .when(!selected, move |d| d.hover(move |s| s.bg(hover_bg)))
        .on_click({
            let id = row.id.clone();
            let toggle = on_toggle.clone();
            let select = on_select.clone();
            move |_, window: &mut Window, _cx: &mut App| {
                dispatch(expandable, &id, window, &toggle, &select)
            }
        });

    // 选中行左侧 2px 激活条（绝对定位，行必须 `.relative()`）
    if selected {
        el = el.child(tree::active_bar(primary));
    }

    // 缩进：`depth × TREE_INDENT`（子元素形式，与草稿箱同一套）
    el = el.child(tree::indent_spacer(ui::TREE_INDENT, row.depth as usize));

    // 展开指示：槽宽固定 10px（缩进算式的一部分，不得改）；无下一层时留同宽空位
    el = el.child(if expandable {
        tree::disclosure_slot().child(tree::disclosure_icon(expanded, muted))
    } else {
        tree::disclosure_slot()
    });

    // 类型图标：与标签同一行的词法位置（目录 / 连接 / 表 / 文件），尺寸走 ui 常量
    el = el.child(
        Icon::new(row_icon(row))
            .size(rems(ui::ICON_SIZE_SM))
            .text_color(muted),
    );

    // 标签用 `Button::ghost()` 而不是裸文本：行是可点元素，必须键盘可达
    // （`track_focus` + `tab_stop` 由 Button 自带；规范 §04 / gpui-kit-dev 组件选型）
    el = el.child(
        div().flex_1().min_w_0().child(
            Button::new(ElementId::Name(SharedString::from(format!(
                "source-picker-label-{}",
                row.id.key()
            ))))
            .ghost()
            .small()
            .label(SharedString::from(row.label.clone()))
            .disabled(dimmed)
            .on_click({
                let id = row.id.clone();
                let toggle = on_toggle.clone();
                let select = on_select.clone();
                move |_, window: &mut Window, _cx: &mut App| {
                    dispatch(expandable, &id, window, &toggle, &select)
                }
            }),
        ),
    );

    // 搜索结果行的路径上下文（§03）：`数据库 / mysql_src / analytics`
    if let Some(crumb) = &row.breadcrumb {
        el = el.child(
            div()
                .flex_none()
                .max_w(rems(12.))
                .text_xs()
                .text_color(muted)
                .text_ellipsis()
                .child(crumb.clone()),
        );
    }

    // 右侧元信息 / 行尾 pill（驱动名、可见范围、大小…）
    if let Some(meta) = &row.meta {
        el = el.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(meta.clone()),
        );
    }

    el.into_any_element()
}

/// 把可见行画成一个滚动列表（短列表路径：不虚拟化，高度随内容）。
///
/// `on_toggle`（可展开行的点击）与 `on_select`（目标行的点击）分开：前者改展开态、
/// 后者改选中，副作用完全不同，混在一个回调里迟早出错。
pub fn render_rows(
    rows: &[Row],
    tree: &TreeState,
    on_toggle: RowAction,
    on_select: RowAction,
    cx: &mut App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let muted = colors.muted_foreground;

    let visible = tree.visible_rows(rows);
    let mut list = div()
        .v_flex()
        .w_full()
        .gap_1()
        .max_h(rems(ui::VIS_DIALOG_LIST_MAX_H))
        .overflow_y_scrollbar();

    if visible.is_empty() {
        let hint = if tree.is_searching() {
            "没有匹配的对象"
        } else {
            "这里还没有可用的对象"
        };
        list = list.child(div().text_xs().text_color(muted).child(hint));
    }

    for row in visible.iter() {
        let expanded = tree.is_expanded(&row.id);
        let selected = tree.selected() == Some(&row.id);
        list = list.child(row_element(
            row, expanded, selected, &on_toggle, &on_select, cx,
        ));
    }

    list.into_any_element()
}

/// 行点击的分派：可展开行改展开态，目标行改选中。
///
/// 抽成函数是为了两处（整行 / 行内 Button）共用同一套判断——用 `Rc<dyn Fn>` 中转
/// 会在 `&mut Window` 的生命周期上撞「`FnOnce` 不够泛化」的编译错。
fn dispatch(
    expandable: bool,
    id: &NodeId,
    window: &mut Window,
    toggle: &RowAction,
    select: &RowAction,
) {
    if expandable {
        toggle(id, window);
    } else {
        select(id, window);
    }
}

#[cfg(test)]
mod tests {
    use super::super::node::{NodeKind, Row};

    /// 图标选择按 `NodeKind` 分档：目录类同标、连接给库标、表/临时表给表标。
    /// 文件走扩展名映射（CSV 与 parquet 都是「表」形态，无扩展名才落到通用文件）。
    #[test]
    fn icons_follow_the_row_kind() {
        let folder = Row::new(
            super::super::node::NodeId::Path("f".into()),
            1,
            NodeKind::Folder,
            "草稿箱",
        );
        assert_eq!(super::row_icon(&folder).0.as_ref(), "icons/folder.svg");

        let conn = Row::new(
            super::super::node::NodeId::Path("c".into()),
            1,
            NodeKind::Conn,
            "mysql_src",
        );
        assert_eq!(super::row_icon(&conn).0.as_ref(), "icons/database.svg");

        let table = Row::new(
            super::super::node::NodeId::Path("t".into()),
            3,
            NodeKind::Table,
            "orders",
        );
        assert_eq!(super::row_icon(&table).0.as_ref(), "icons/table.svg");

        let csv = Row::new(
            super::super::node::NodeId::Path("数据/a.csv".into()),
            2,
            NodeKind::File,
            "a.csv",
        );
        assert_eq!(super::row_icon(&csv).0.as_ref(), "icons/table.svg");

        let unknown = Row::new(
            super::super::node::NodeId::Path("x.zzz".into()),
            2,
            NodeKind::File,
            "x.zzz",
        );
        assert_eq!(super::row_icon(&unknown).0.as_ref(), "icons/file.svg");
    }

    /// 虚拟化阈值：60 行以内走普通列表（观感不变），以上切虚拟视口。
    /// 视口高度 = 行数 × 行高，但夹在对话框列表上限内——虚拟列表要确定高度才算区间。
    #[test]
    fn virtual_list_kicks_in_only_for_long_lists() {
        use workbench_shell::ui;

        assert!(!super::needs_virtual_list(0));
        assert!(!super::needs_virtual_list(60));
        assert!(super::needs_virtual_list(61));

        assert_eq!(super::virtual_viewport_rem(10), 10. * ui::ROW_HEIGHT);
        assert_eq!(
            super::virtual_viewport_rem(2000),
            super::virtual_viewport_rem(usize::MAX)
        );
        assert_eq!(
            super::virtual_viewport_rem(usize::MAX),
            ui::VIS_DIALOG_LIST_MAX_H
        );
    }

    /// 只有导航层的三种行可展开；表 / 文件 / 临时表是叶子（否则 chevron 会骗人）。
    #[test]
    fn only_navigation_kinds_are_expandable() {
        assert!(NodeKind::Group.is_expandable());
        assert!(NodeKind::Conn.is_expandable());
        assert!(NodeKind::Folder.is_expandable());
        assert!(!NodeKind::Table.is_expandable());
        assert!(!NodeKind::File.is_expandable());
        assert!(!NodeKind::Temp.is_expandable());
    }

    /// 叶子行的可点击性来自 `Row::is_confirmable`：禁用行既不可确认、也不该被选中。
    #[test]
    fn disabled_row_is_not_confirmable() {
        let row = Row::new(
            super::super::node::NodeId::Path("g/orders".into()),
            3,
            NodeKind::Table,
            "G_orders",
        )
        .disabled();
        assert!(!row.is_confirmable());
    }
}
