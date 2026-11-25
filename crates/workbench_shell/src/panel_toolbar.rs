//! 面板工具栏（`PanelToolbar`）：面板顶部那一行的统一容器。
//!
//! ## 为什么要有这一层
//!
//! 过去每个面板各画一条工具栏：高度各写一遍（32 / 36 混用）、左右内距各写一遍
//! （0 / 4 / 8 / 12 四种）、分隔线有的用 `border_b_1` 有的用 `border_b(HAIRLINE)`、
//! 没有动作的面板也照样占一条 36 高的空行。**同一个外壳里六个面板，六个微差**。
//!
//! 本层把「这一行的度量与槽位」收到一处：
//!
//! - 高度、左右内距、底部分隔线只在这里决定；
//! - 左槽可伸缩（搜索框 / 标题 / 面包屑），右槽固定（动作按钮）；
//! - 次行槽（`secondary`）给动作多的面板，不把主行挤爆；
//! - **全空时整条不画**（返回零高元素）——没有动作的面板不该留一条空行。
//!
//! 配色也在这里收口（对齐 Zed 工具栏）：底色用 `toolbar.background`（比 `background`
//! 更「组件档」，让面板头与内容区在视觉上分开），下缘用 `border.variant`（比面板间的
//! `border` 弱一档，避免每个面板头都像被硬切一刀）。两者都是本仓 vendor patch 新增的
//! 主题键（上游 0.10.0 没有，见 `crates/ui_guard` 的 143 键守卫与 theme skill）。
//!
//! 内层仍然是官方的 `toolbar::Toolbar`（roving 方向键焦点 + toolbar a11y 语义 + size 托管），
//! 本层只负责它外面的高度 / 内距 / 底色 / 分隔线（规范：背景与边框由宿主画）。
//!
//! ## 与 Zed 的对应
//!
//! `zed-1.29.3/crates/workspace/src/toolbar.rs` 是「pane 拥有一条 + 各 item 注册控件 +
//! 位置四档（Hidden / PrimaryLeft / PrimaryRight / Secondary）+ 无可见项整条消失」。
//! 这里是它的**声明式简化版**：调用方在一次 render 里给齐三槽，不建实体、不发事件；
//! `Hidden` 那一档由「调用方不传」表达。等真的要运行时搬槽（切换 tab 时换位置）再升级成实体。

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::*;

use crate::ui::{HAIRLINE, PANEL_HEADER_HEIGHT, PANEL_TOOLBAR_PADDING};

/// 面板工具栏的统一容器。
#[derive(IntoElement)]
pub struct PanelToolbar {
    id: SharedString,
    height: f32,
    padding: f32,
    hairline: bool,
    background: bool,
    left: Vec<AnyElement>,
    right: Vec<AnyElement>,
    secondary: Vec<AnyElement>,
}

impl PanelToolbar {
    /// 默认形态：`PANEL_HEADER_HEIGHT`（36）高、`PANEL_TOOLBAR_PADDING`（8）左右内距、
    /// `toolbar.background` 底色、1px `border.variant` 下界。
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            height: PANEL_HEADER_HEIGHT,
            padding: PANEL_TOOLBAR_PADDING,
            hairline: true,
            background: true,
            left: Vec::new(),
            right: Vec::new(),
            secondary: Vec::new(),
        }
    }

    /// 改高度（rem）。资产库那条「面板内第二行」工具条是 32，不是 36。
    pub fn height(mut self, rem: f32) -> Self {
        self.height = rem;
        self
    }

    /// 改外层左右内距（rem）。默认 8；文字左缘会自动再加 `Toolbar` 的 4。
    pub fn padding(mut self, rem: f32) -> Self {
        self.padding = rem;
        self
    }

    /// 去掉底部分隔线（工具栏下面还有自己的分组容器时用）。
    pub fn no_hairline(mut self) -> Self {
        self.hairline = false;
        self
    }

    /// 去掉底色：面板头下面紧接同底色的容器（如标题条 / 侧栏同档）时，避免两层叠色出边。
    pub fn no_background(mut self) -> Self {
        self.background = false;
        self
    }

    /// 左槽：可伸缩区（搜索框 / 标题 / 面包屑 / 说明文字）。
    pub fn left(mut self, el: impl IntoElement) -> Self {
        self.left.push(el.into_any_element());
        self
    }

    /// 右槽：动作按钮，固定宽、从右往左排。
    pub fn right(mut self, el: impl IntoElement) -> Self {
        self.right.push(el.into_any_element());
        self
    }

    /// 次行槽：动作多到主行放不下时用（对应 Zed 的 `Secondary`）。
    pub fn secondary(mut self, el: impl IntoElement) -> Self {
        self.secondary.push(el.into_any_element());
        self
    }

    /// 三槽全空：这一条不画。
    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.right.is_empty() && self.secondary.is_empty()
    }

    /// 条件构造器：`cond` 为真时才追加（避免调用方为了一个条件拆成 `let mut … ; if …`）。
    pub fn when(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond { f(self) } else { self }
    }

    /// `Option` 条件构造器：`Some(v)` 时把 `v` 交给 `f`（例如连接状态徽标可能不存在）。
    pub fn when_some<T>(self, opt: Option<T>, f: impl FnOnce(Self, T) -> Self) -> Self {
        match opt {
            Some(v) => f(self, v),
            None => self,
        }
    }
}

impl RenderOnce for PanelToolbar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let id = self.id.clone();
        let bar_id = SharedString::from(format!("{id}::bar"));
        let has_primary = !self.left.is_empty() || !self.right.is_empty();

        let mut root = div().v_flex().w_full().flex_none();
        if self.background {
            root = root.bg(theme.colors.toolbar_background);
        }

        if has_primary {
            let mut row = div()
                .h_flex()
                .items_center()
                .flex_none()
                // 行必须铺满面板宽：否则左槽的 `flex_1` 拿不到剩余空间，
                // 右槽会紧贴左槽（草稿箱 `toolbar_left_and_right_groups_split_at_panel_edges` 抓的就是这个）。
                .w_full()
                .h(rems(self.height))
                .px(rems(self.padding))
                .debug_selector(move || id.to_string());
            if self.hairline {
                // 弱一档的分隔线：面板头的下缘不该用面板之间那档 `border`，
                // 否则每个面板都像被硬切一刀（Zed 同口径，`border_variant`）。
                row = row
                    .border_b(HAIRLINE)
                    .border_color(theme.colors.border_variant);
            }

            // 左槽可伸缩、右槽固定：左右两槽都作为 `Toolbar` 的 content（不参与 size 托管，
            // 因为槽里既有按钮也有输入框；roving 焦点要等槽内控件统一改用 `child` 托管）。
            let mut bar = Toolbar::new(bar_id);
            if !self.left.is_empty() {
                bar = bar.content(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .flex_1()
                        .min_w_0()
                        .children(self.left),
                );
            }
            if !self.right.is_empty() {
                bar = bar.content(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .flex_none()
                        .children(self.right),
                );
            }
            // 内层 `toolbar::Toolbar` 自身 `flex_shrink_0`、宽度按内容算（vendor/gpui-component
            // src/toolbar.rs:251），必须在这里补 `w_full` 撑满行宽；否则左槽的 `flex_1`
            // 没有可分配的剩余空间，右槽贴不到右端。
            root = root.child(row.child(bar.w_full()));
        }

        if !self.secondary.is_empty() {
            root = root.child(
                div()
                    .h_flex()
                    .items_center()
                    .flex_none()
                    .gap_1()
                    .px(rems(self.padding))
                    .pb_2()
                    .children(self.secondary),
            );
        }

        root
    }
}
