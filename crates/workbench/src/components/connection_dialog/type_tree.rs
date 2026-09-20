//! 侧栏「数据库类型」列表（gpui-kit `list::List` 组件化，决策 #107）。
//!
//! 为什么换掉自绘行：自绘可点行只有 `cursor_pointer` + `on_click`，没有键盘导航 / 焦点 / a11y
//! （技能里点名过这类回归）。`List` 自带：
//! - 小节结构（`sections_count` + `render_section_header`）——分类正好是小节，空分类由组件跳过
//!   （与旧实现里 `if matched.is_empty() { continue }` 的语义一致）；
//! - ↑↓ 选择、Enter 确认、Esc 取消（`Cancel` 会 `cx.propagate()`，不会拦掉对话框自己的 Esc）；
//! - 滚动到选中项、虚拟化、空态槽（`render_empty`）。
//!
//! 行用 `ListItem`（组件行）：悬停 = `list.hover`、选中 = `list.active` 底 + `list.active.border`
//! 描边。原型 §5 把「选中项底色 / 左边条」映射到的正是这两个 token（`#E4E4E4` / `#C25B46`，
//! 与旧的两处自绘取值完全相同），所以这是**同一套配色**、只是改由组件画。

use super::*;

use gpui_kit::component::IndexPath;
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};

/// 类型分类（顺序即侧栏显示顺序；与 `data_source_types.category` 对齐）。
pub(crate) const TYPE_CATEGORIES: [(&str, &str); 4] = [
    ("relational", "关系型"),
    ("file-based", "文件型"),
    ("analytics", "分析型"),
    ("nosql", "NoSQL"),
];

/// 类型树委托：小节 = 分类，行 = 类型。
pub struct TypeTreeDelegate {
    /// 对话框状态：确认（点击 / Enter）时走与鼠标路径**同一个入口** `select_type`。
    dialog: Rc<ConnectionDialogState>,
    /// 面板实体 + 宿主桥（选中后要重绘：对话框层挂在宿主 render 上）。
    entity: Entity<crate::panels::EditorPanel>,
    shared: Shared,
    /// 过滤后的分类小节（标签 + 行）。
    sections: Vec<(&'static str, Vec<DataSourceType>)>,
    /// 驱动目录快照（判断「暂无驱动」；不参与指纹，随 sync 一起换）。
    drivers: Vec<Driver>,
    /// 当前选中类型 id（画选中态用）。
    selected: String,
    /// 组件记录的行号（`set_selected_index` 写入；确认时用来反查类型 id）。
    active_index: Option<IndexPath>,
    /// 上次同步用的查询词（区分空态文案：搜不到 vs 没加载到）。
    filter: String,
    /// 同步指纹：目录 / 过滤词 / 选中项都没变就不重建（渲染期每帧调用）。
    fingerprint: String,
}

impl TypeTreeDelegate {
    pub(crate) fn new(
        dialog: Rc<ConnectionDialogState>,
        entity: Entity<crate::panels::EditorPanel>,
        shared: Shared,
    ) -> Self {
        Self {
            dialog,
            entity,
            shared,
            sections: Vec::new(),
            drivers: Vec::new(),
            selected: String::new(),
            active_index: None,
            filter: String::new(),
            fingerprint: String::new(),
        }
    }

    /// 渲染期同步（**权威同步点**，与 `url_placeholder_for` 等同一约定）：
    /// 目录 / 过滤词 / 选中项变化时才重建小节。返回 true = 有变化。
    pub(crate) fn sync(
        &mut self,
        types: &[DataSourceType],
        drivers: &[Driver],
        filter: &str,
        selected: &str,
    ) -> bool {
        let mut fp = String::with_capacity(types.len() * 12 + 32);
        fp.push_str(filter);
        fp.push('\u{1}');
        fp.push_str(selected);
        for t in types {
            fp.push('\u{1}');
            fp.push_str(&t.id);
        }
        if fp == self.fingerprint {
            return false;
        }
        self.fingerprint = fp;
        self.filter = filter.trim().to_string();
        self.selected = selected.to_string();
        self.drivers = drivers.to_vec();
        self.sections = TYPE_CATEGORIES
            .iter()
            .map(|(cat, label)| {
                let items = types
                    .iter()
                    .filter(|t| t.category == *cat)
                    .filter(|t| type_matches_filter(t, drivers, filter))
                    .cloned()
                    .collect();
                (*label, items)
            })
            .collect();
        true
    }

    /// 选中类型在列表里的下标（已被过滤掉 → None：此时 Enter 不该选中任何东西）。
    pub(crate) fn selected_path(&self) -> Option<IndexPath> {
        self.sections
            .iter()
            .enumerate()
            .find_map(|(section, (_, items))| {
                items
                    .iter()
                    .position(|t| t.id == self.selected)
                    .map(|row| IndexPath::new(row).section(section))
            })
    }

    /// 当前选中的行（确认时用）。
    fn type_id_at(&self, ix: IndexPath) -> Option<String> {
        self.sections
            .get(ix.section)
            .and_then(|(_, items)| items.get(ix.row))
            .map(|t| t.id.clone())
    }
}

impl ListDelegate for TypeTreeDelegate {
    type Item = ListItem;

    fn sections_count(&self, _: &App) -> usize {
        self.sections.len()
    }

    fn items_count(&self, section: usize, _: &App) -> usize {
        self.sections
            .get(section)
            .map(|(_, items)| items.len())
            .unwrap_or(0)
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let (label, items) = self.sections.get(section)?;
        // 小节头高度必须恒定（组件按同一高度测量）：写死 1.25rem 而不是让文字撑。
        Some(
            div()
                .w_full()
                .h(rems(1.25))
                .px_2()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(cx.theme().colors.muted_foreground)
                .child(format!("{label}（{}）", items.len())),
        )
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let t = self.sections.get(ix.section)?.1.get(ix.row)?.clone();
        let has_driver = type_has_driver(&self.drivers, &t.id);
        let icon = t
            .icon
            .clone()
            .filter(|i| !i.trim().is_empty())
            .unwrap_or_else(|| "🗄".to_string());
        // ElementId 用业务键（类型 id）：列表重排 / 过滤不会把行状态串到别的行（#17/#86）。
        let mut row = ListItem::new(ElementId::Name(SharedString::from(format!(
            "type-{}",
            t.id
        ))))
        // 行高与旧实现一致（1.75rem）：委托里所有行必须同高（组件只量一行）。
        .h(rems(1.75))
        .px_2()
        .rounded(rems(0.375))
        .text_xs()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap(rems(0.375))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child(icon),
                )
                .child(
                    div()
                        .text_color(if has_driver {
                            cx.theme().colors.foreground
                        } else {
                            cx.theme().colors.muted_foreground
                        })
                        .child(t.name.clone()),
                ),
        );
        if !has_driver {
            // 无可用驱动：置灰 + 行尾「暂无驱动」（点击不切换由 `select_type` 守卫）；
            // `disabled(true)` 同时让组件不把它当作可选中行。
            row = row.disabled(true).suffix(|_, cx| {
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("暂无驱动")
            });
        }
        Some(row)
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        // 空白侧栏分不清「搜不到」与「没加载到」：两种文案分开给（窗口用例按 id 断言可见性）。
        div()
            .id("conn-type-empty")
            .test_support()
            .w_full()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(if self.filter.is_empty() {
                "类型目录为空（未读到 data_source_types）"
            } else {
                "没有匹配的类型 / 驱动"
            })
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.active_index = ix;
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(ix) = self.active_index else {
            return;
        };
        let Some(type_id) = self.type_id_at(ix) else {
            return;
        };
        // 与点击路径同一个入口：`select_type` 自带「无可用驱动不切换 + 结果行给原因」的守卫。
        self.dialog.select_type(&type_id, window, cx);
        let _ = self.entity.update(cx, |_, cx| cx.notify());
        self.shared.notify_host(cx);
    }
}
