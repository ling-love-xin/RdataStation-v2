//! Quick Open 的 `List` 委托：把行快照交给组件渲染（选中 / hover / 漫游 / 空态归组件）。
//!
//! 与 `analytics_resource::resource_view::ArchiveListDelegate` 同一模式：
//! - 行快照是**拷贝**——`render_item` 在组件渲染期被调用，那一刻宿主正在被渲染，
//!   回头读宿主字段会 panic；
//! - 选中以宿主的**业务键**为准（跨重算跟随），这里只是渲染锚点；宿主在 render 里
//!   把自己镜像进列表（`syncing_from_host` 守卫），镜像期间组件回调**不回写**宿主。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::component::{ActiveTheme, IndexPath, Theme};
use gpui_kit::*;

use crate::quick_open::model::{Action, Group, Mode, Row, match_span};
use crate::ui;
use crate::view::WorkbenchView;

/// 行快照 + 选中锚点。
pub(crate) struct QuickOpenDelegate {
    groups: Vec<Group>,
    /// 当前查询词（渲染期算高亮区间；组件自带的搜索框未启用，这里是唯一来源）。
    needle: String,
    mode: Mode,
    /// 选中的行业务键（宿主是权威，这里跟随）。
    selected_key: Option<String>,
    /// 宿主正在把选中镜像进列表：此间组件回调的 `set_selected_index` 不再回写。
    syncing_from_host: bool,
    /// 宿主动作入口（确认时交回宿主执行；宿主已销毁则静默丢弃）。
    host: WeakEntity<WorkbenchView>,
}

impl QuickOpenDelegate {
    pub(crate) fn new(host: WeakEntity<WorkbenchView>) -> Self {
        Self {
            groups: Vec::new(),
            needle: String::new(),
            mode: Mode::Default,
            selected_key: None,
            syncing_from_host: false,
            host,
        }
    }

    /// 宿主推送新一轮结果（查询变化 / 数据源变化时调用）。
    pub(crate) fn set_groups(
        &mut self,
        groups: Vec<Group>,
        needle: String,
        mode: Mode,
        selected_key: Option<String>,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.groups = groups;
        self.needle = needle;
        self.mode = mode;
        self.selected_key = selected_key;
        cx.notify();
    }

    /// 行数（宿主提示行显示）。
    pub(crate) fn row_count(&self) -> usize {
        self.groups.iter().map(|g| g.rows.len()).sum()
    }

    /// 选中的业务键（宿主移动选中后读取）。
    pub(crate) fn selected_key(&self) -> Option<String> {
        self.selected_key.clone()
    }

    /// 业务键 → 动作（宿主确认时用；宿主不认识行集合）。
    pub(crate) fn action_of(&self, key: &str) -> Option<Action> {
        self.groups
            .iter()
            .flat_map(|group| group.rows.iter())
            .find(|row| row.key == key)
            .map(|row| row.action.clone())
    }

    /// 宿主开始把选中镜像进列表（镜像期间组件回调不回写宿主）。
    pub(crate) fn begin_host_sync(&mut self) {
        self.syncing_from_host = true;
    }

    /// 镜像结束。
    pub(crate) fn end_host_sync(&mut self) {
        self.syncing_from_host = false;
    }

    /// 业务键 → 索引（宿主移动选中后同步给组件）。
    pub(crate) fn index_of(&self, key: &str) -> Option<IndexPath> {
        for (section, group) in self.groups.iter().enumerate() {
            for (row, item) in group.rows.iter().enumerate() {
                if item.key == key {
                    return Some(IndexPath::new(row).section(section));
                }
            }
        }
        None
    }

    /// 首个可选中行（打开面板 / 结果变化后落位用）。
    pub(crate) fn first_index(&self) -> Option<IndexPath> {
        let section = self.groups.iter().position(|g| !g.rows.is_empty())?;
        Some(IndexPath::new(0).section(section))
    }

    /// 从 `from` 走 `delta`（±1）个可选中行：跨组连续漫游，越界停在端点（不环绕）。
    pub(crate) fn step_index(&self, from: IndexPath, delta: isize) -> Option<IndexPath> {
        let flat = self.flatten();
        let cur = flat
            .iter()
            .position(|(section, row)| *section == from.section && *row == from.row);
        let next = match cur {
            Some(ix) => ix.checked_add_signed(delta).unwrap_or(0),
            None => 0,
        };
        let next = next.min(flat.len().saturating_sub(1));
        flat.get(next)
            .map(|(section, row)| IndexPath::new(*row).section(*section))
    }

    /// 扁平化行索引表（跨组漫游的“第 N 行”视图）。
    fn flatten(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (section, group) in self.groups.iter().enumerate() {
            for row in 0..group.rows.len() {
                out.push((section, row));
            }
        }
        out
    }

    fn row_at(&self, ix: IndexPath) -> Option<&Row> {
        self.groups.get(ix.section)?.rows.get(ix.row)
    }

    /// 行元素：类型标签 + 主文本（命中高亮）+ 右侧次级信息。
    fn row_element(
        &self,
        row: &Row,
        theme: &Theme,
        pt: &settings::product_tokens::ProductTokens,
    ) -> ListItem {
        let muted = theme.colors.muted_foreground;

        let mut title = div()
            .h_flex()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .text_color(theme.colors.foreground)
            .text_xs();
        match match_span(&row.title, &self.needle) {
            Some((start, end)) if end > start => {
                title = title
                    .child(row.title[..start].to_string())
                    .child(
                        div()
                            .rounded_sm()
                            .bg(pt.search_match_background(theme))
                            .child(row.title[start..end].to_string()),
                    )
                    .child(row.title[end..].to_string());
            }
            _ => title = title.child(row.title.clone()),
        }

        ListItem::new(SharedString::from(format!("qo-row-{}", row.key)))
            .h(rems(ui::ROW_HEIGHT))
            .py_0()
            .px_2p5()
            .gap_2()
            .text_xs()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(row.kind.label()),
                    )
                    .child(title)
                    .child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(row.secondary.clone()),
                    ),
            )
    }
}

impl ListDelegate for QuickOpenDelegate {
    type Item = ListItem;

    fn sections_count(&self, _cx: &App) -> usize {
        // 组件约定：至少 1 个 section；空结果由 `render_empty` 承担。
        self.groups.len().max(1)
    }

    fn items_count(&self, section: usize, _cx: &App) -> usize {
        self.groups.get(section).map(|g| g.rows.len()).unwrap_or(0)
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let row = self.row_at(ix)?.clone();
        let theme = cx.theme().clone();
        let pt = settings::product_tokens::get(cx);
        Some(self.row_element(&row, &theme, &pt))
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let group = self.groups.get(section)?;
        let theme = cx.theme();
        let pt = settings::product_tokens::get(cx);
        let mut head = div()
            .h_flex()
            .items_center()
            .gap_1()
            .h(rems(ui::QUICK_OPEN_GROUP_HEADER_HEIGHT))
            .px_2()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .bg(pt.quick_open_group_header(theme))
            .child(group.title);
        // 「搜索中…」等补充：与计数区分开色（info），一眼看出“还没搜完”而不是“没结果”
        if !group.note.is_empty() {
            head = head.child(div().text_color(theme.colors.info).child(group.note.clone()));
        }
        Some(
            head.child(
                div()
                    .ml_auto()
                    .child(format!("{} 条", group.rows.len())),
            ),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (title, hint) = match self.mode {
            Mode::Command => ("无匹配命令", "删掉 > 前缀回到默认搜索"),
            Mode::FullText => (
                "无匹配全文命中",
                "元数据可能没有注释文本；试试名称搜索，或去掉 # 前缀",
            ),
            Mode::Default => ("无匹配结果", "试试输入 > 搜命令 · # 搜元数据全文"),
        };
        div()
            .v_flex()
            .items_center()
            .gap_1()
            .py_6()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child(div().text_color(theme.colors.foreground).child(title))
            .child(hint)
            .into_any_element()
    }

    /// 组件把选中变化回传（鼠标点击 / 组件内漫游）：以宿主的业务键为准。
    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let key = ix.and_then(|ix| self.row_at(ix).map(|row| row.key.clone()));
        if self.syncing_from_host {
            // 宿主镜像：只更新锚点，不回写（见字段注释）。
            self.selected_key = key;
            return;
        }
        if key == self.selected_key {
            return;
        }
        self.selected_key = key.clone();
        let _ = self
            .host
            .update(cx, |view, cx| view.set_quick_open_selection(key, cx));
    }

    /// ↵ / 点击：交回宿主执行（关面板与副作用都在宿主侧）。
    fn confirm(
        &mut self,
        secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(row) = self
            .selected_key
            .as_deref()
            .and_then(|key| {
                self.groups
                    .iter()
                    .flat_map(|g| g.rows.iter())
                    .find(|r| r.key == key)
            })
            .cloned()
        else {
            return;
        };
        let action: Action = row.action;
        let _ = self.host.update(cx, |view, cx| {
            // 确认时保留面板：输入框的 Shift+↵，或列表里的 Ctrl+点击。
            // （Ctrl+↵ 的「后台打开」待编辑器支持后与 Shift 细分。）
            view.execute_quick_open_row(action, secondary, window, cx)
        });
    }
}
