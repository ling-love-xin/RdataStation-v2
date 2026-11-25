//! 树 / 列表**行的共用原语**：激活条、展开指示、缩进占位、行高预算。
//!
//! ## 为什么放在外壳
//!
//! 数据源导航（M4）、草稿箱（M5）、资产库（M6）三处**各写了一遍同一段**：选中行左侧 2px 条、
//! 10px 宽的展开指示槽、`depth × 步长` 的缩进、以及「基础行高 + 若干附加块」的高度估算。
//! 外壳的定位正是「面板状态 + 视图共用资产」，且两侧都已依赖它（依赖方向：feature → shell）。
//!
//! ## 边界（哪些**不**抽）
//!
//! 只收**自变量少、语义确定**的东西。**「这一行长什么样」仍归各面板**：三处的行模型
//! （枚举载荷 / 路径键 / 存档 id）、交互（三类拖拽 / 一类 / 无）、展开态归属（落库并驱动
//! 后台加载 / 纯内存 / 落设置）都不同，抽成通用树组件会打架——实测取舍见
//! `docs/architecture/layout/panels-coupling-plan.md` 与 `database-navigator-architecture.md` §10。
//!
//! ## 展开指示
//!
//! 四个消费方（导航 M4 / 草稿箱 M5 / 资产库 M6 / Mock M7）**已统一到图标**（`disclosure_icon`，
//! 12px chevron）：导航见原型 v8 修订点，草稿箱见 `scratchpad-architecture.md` D14，
//! 资产库见 `resource_view.rs::render_group_header`。旧的字符载体 `disclosure_glyph`（`▸/▾`）
//! 在三处都换掉后**已删除（2026-09-20）**——留着它就是「还有一个载体可选」的暗示。
//!
//! 槽宽（`disclosure_slot`，10px）是缩进算式的一部分：行内容 = `pl` + 槽 + `gap_1`
//! = 一个 `TREE_INDENT` 步长，换载体只动那一行调用，不得改槽宽，否则同层行标题左边缘会错位。

use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui;

/// 选中行左侧 2px 激活条（**绝对定位**；调用方所在行必须 `.relative()`）。
///
/// 上下内缩 `TREE_ACTIVE_BAR_INSET`，让色条不贴满整行高——与草稿箱 / 设置页同形。
/// 用绝对定位而不是行内元素：行内元素会挤动名称列（宽度按 `flex` 分配），
/// 而激活条应该「浮在行的左缘」。
pub fn active_bar(color: Hsla) -> Div {
    div()
        .absolute()
        .left(rems(0.))
        .top(ui::TREE_ACTIVE_BAR_INSET)
        .bottom(ui::TREE_ACTIVE_BAR_INSET)
        .w(ui::TREE_ACTIVE_BAR)
        .flex_none()
        .rounded_sm()
        .bg(color)
}

/// 展开指示槽：固定 `w_2p5`（10px）、内容居中。
///
/// **槽宽是缩进算式的一部分**（行内容 = `pl` + 槽 + `gap_1` = 一个 `TREE_INDENT` 步长），
/// 换图标 / 字符都不得改动它，否则同层行标题左边缘会错位。
/// 无子节点时留同宽空位（返回空槽即可）。
pub fn disclosure_slot() -> Div {
    div()
        .w_2p5()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
}

/// 展开指示**图标**载体（12px chevron；四个消费方统一用它）。
pub fn disclosure_icon(expanded: bool, color: Hsla) -> Icon {
    Icon::empty()
        .path(if expanded {
            "icons/chevron-down.svg"
        } else {
            "icons/chevron-right.svg"
        })
        .size(rems(ui::ICON_SIZE_XS))
        .flex_none()
        .text_color(color)
}

/// 行内**计数徽章**（对齐 VS Code `countBadge` 胶囊规格：最小 18px、圆角全、
/// 内距 4px（Tailwind `px_1`）、字号 `text_sm`（13–14px 档））。
///
/// 消费方（2026-10-12 起收口）：草稿箱树命中数角标、搜索结果树文件行命中数角标——
/// 此前两处各写一遍「`px_1 + rounded_sm + text_xs + text_color(primary) + bg(hover_bg)`」。
/// 规格来源：`docs/architecture/ui/ui-design-spec.md` §4 徽标行
/// （VS Code `countBadge` 2026-10-12 实证，见 `vscode-ui-design-system.md` §4.5）。
/// 颜色口径沿用既有消费点：`fg` = 强调色文字、`bg` = 浅底（信息类角标，非实底徽章）。
pub fn count_badge(count: usize, fg: Hsla, bg: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .min_w(rems(ui::NAV_BADGE_SIZE))
        .h(rems(ui::NAV_BADGE_SIZE))
        .px_1()
        .rounded_full()
        .text_sm()
        .text_color(fg)
        .bg(bg)
        .child(count.to_string())
}

/// 缩进占位（子元素形式）：宽度 = `depth × step`。
///
/// 用于「行的第一个子元素是缩进」的排版（草稿箱）；导航那边缩进走 `pl`，
/// 用 [`indent_rem`] 取倍率。两种写法等价，选哪种取决于行的内距怎么给。
pub fn indent_spacer(step: f32, depth: usize) -> Div {
    div().w(rems(step * depth as f32)).flex_none()
}

/// 缩进量（rem 倍率，交给 `.pl(rems(..))`）：`base + depth × step`。
///
/// `base` 取参而不是写死：导航有基础内距（`TREE_BASE_PADDING`），草稿箱为 0。
pub fn indent_rem(base: f32, step: f32, depth: usize) -> f32 {
    base + depth as f32 * step
}

/// 一行的**高度预算**（rem）：基础高 + 若干附加块。
///
/// 为什么要有个类型而不是裸 `f32`：虚拟列表（`v_virtual_list`）按给定高度定义布局、
/// **不回写实测值**，所以「估算」与「渲染」是**成对维护**的契约；把累加写成链式
/// （`RowHeight::new(base).add(subline).add(editor)`）能让「多算了哪一块」一眼看出，
/// 也把「rem 倍率 → Pixels」的换算收敛到一处（此前两处各写一遍 `rems(x).to_pixels(rem)`）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowHeight {
    rem: f32,
}

impl RowHeight {
    /// 基础行高（rem 倍率）。
    pub fn new(base: f32) -> Self {
        Self { rem: base }
    }

    /// 追加一个附加块（附加行 / 行内编辑器块，rem 倍率）。
    #[must_use]
    pub fn add(self, extra: f32) -> Self {
        Self {
            rem: self.rem + extra,
        }
    }

    /// 按条件追加：`cond` 为真才加（读起来比 `if` 包一层更顺）。
    #[must_use]
    pub fn add_if(self, cond: bool, extra: f32) -> Self {
        if cond { self.add(extra) } else { self }
    }

    /// rem 倍率（断言 / 测试用）。
    pub fn rem(self) -> f32 {
        self.rem
    }

    /// 换算成像素（基准是主题字号）。
    pub fn to_pixels(self, rem_size: Pixels) -> Pixels {
        rems(self.rem).to_pixels(rem_size)
    }
}

/// 行高表（虚拟列表的 `item_sizes`）：宽度取 0，高度由 `f(ix)` 给。
///
/// 单个入口包住「逐行算高 → `Rc<Vec<Size<Pixels>>>`」：导航与草稿箱此前各写一遍，
/// 且两处都要求调用方自己保证「行数与渲染的行数一致」。
pub fn row_sizes(
    count: usize,
    rem_size: Pixels,
    f: impl Fn(usize) -> RowHeight,
) -> Rc<Vec<Size<Pixels>>> {
    Rc::new(
        (0..count)
            .map(|ix| Size::new(Pixels::ZERO, f(ix).to_pixels(rem_size)))
            .collect(),
    )
}

/// 分组头（可折叠分区标题行）的统一原语。
///
/// ## 为什么收
///
/// 草稿箱的四个分组（草稿 / 书签 / 引用 / 回收站）与数据源导航的分组头此前各写一份，
/// 肉眼可见的差三处：行高 22 vs 24、左右内距 6 vs 4、底色 `group_box` vs
/// `sidebar_accent`（后者在各主题包里与 `list.hover` **同值**，分组头常态就长得像被 hover）。
/// 用户实机反馈 2026-11-25：**以草稿箱为准**统一，边距取「书签 / 引用 / 回收站」那一档。
///
/// ## 规格（改这里就是改全部五个分组头）
///
/// - 行高 `ui::ROW_HEIGHT_COMPACT`（1.375rem = 22）
/// - 左右内距 `px_1p5`（6）
/// - 行级 `text_xs`，组名 `MEDIUM` + 省略号
/// - 底色 `colors.group_box`（凹槽档：暗色主题比面板底暗一档，不是「亮起来」那档）
/// - 展开指示一律 [`disclosure_slot`] + [`disclosure_icon`]
///
/// ## 槽位（齐了才对齐，别在调用方再补 px / height）
///
/// - `leading`：组标识，导航用 2px 主色条；草稿箱没有
/// - 固定段：`disclosure_slot + 组名 + 弹簧`
/// - `trailing`：常驻尾部（健康点 / 计数徽标 / 批量动作）
/// - `hover_actions`：hover 才显的动作组（草稿箱的「新建 / 刷新 / 清空」）
///
/// 尾部**不**收在 `trailing` 里的东西：真正的业务动作按各自面板需要自己组按钮，
/// 这里只管位置与显隐。
#[derive(IntoElement)]
pub struct GroupHeader {
    id: SharedString,
    debug: Option<&'static str>,
    label: SharedString,
    label_max_width: Option<f32>,
    expanded: bool,
    leading: Option<AnyElement>,
    label_icon: Option<AnyElement>,
    hover_bg: Option<Rgba>,
    trailing: Vec<AnyElement>,
    hover_actions: Vec<AnyElement>,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl GroupHeader {
    /// 建一行分组头：`id` 走 `ElementId` 与 `group`（hover 动作按它显隐），
    /// `label` 传已翻译好的文案。
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            debug: None,
            label: label.into(),
            label_max_width: None,
            expanded: false,
            leading: None,
            label_icon: None,
            hover_bg: None,
            trailing: Vec::new(),
            hover_actions: Vec::new(),
            on_click: None,
        }
    }

    /// 登记 `debug_selector`（既有面板的测试按名字量这一行的高度 / 位置）。
    #[must_use]
    pub fn debug_selector(mut self, name: &'static str) -> Self {
        self.debug = Some(name);
        self
    }

    /// 展开态（决定 chevron 朝向）。
    #[must_use]
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// 组名前的语义图标（书签分组用 `Bookmark`）；导航不传。
    #[must_use]
    pub fn label_icon(mut self, el: impl IntoElement) -> Self {
        self.label_icon = Some(el.into_any_element());
        self
    }

    /// 组名最大宽（rem）：导航的分组名按内容宽并允许省略；草稿箱不传。
    #[must_use]
    pub fn label_max_width(mut self, rem: f32) -> Self {
        self.label_max_width = Some(rem);
        self
    }

    /// 组标识（导航的 2px 主色条）。
    #[must_use]
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    /// 常驻尾部元素（健康点 / 计数 / 批量动作），按调用顺序排在弹簧右侧。
    #[must_use]
    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing.push(el.into_any_element());
        self
    }

    /// hover 才显的动作组（整组一起淡入；不 hover 时占位不消失，行宽不跳）。
    /// 多次调用即按顺序累加。
    #[must_use]
    pub fn hover_action(mut self, el: impl IntoElement) -> Self {
        self.hover_actions.push(el.into_any_element());
        self
    }

    /// hover 提亮（分组头平时是凹槽档 `group_box`，指上去才亮一档）。
    /// 草稿箱不传；导航沿用既有 `list.hover` 提亮。
    #[must_use]
    pub fn hover_background(mut self, color: impl Into<Rgba>) -> Self {
        self.hover_bg = Some(color.into());
        self
    }

    /// 点击折叠 / 展开。
    #[must_use]
    pub fn on_click(
        mut self,
        f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

impl RenderOnce for GroupHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors;
        let muted = colors.muted_foreground;
        let group_id = format!("{}::group", self.id);

        let mut row = div()
            .id(self.id.clone())
            .group(group_id.clone())
            .h_flex()
            .items_center()
            .w_full()
            .gap_1()
            .text_xs()
            .h(rems(ui::ROW_HEIGHT_COMPACT))
            .px_1p5()
            .bg(colors.group_box);

        if let Some(name) = self.debug {
            row = row.debug_selector(move || name.to_string());
        }
        if let Some(color) = self.hover_bg {
            row = row.hover(move |s| s.bg(color));
        }
        if let Some(leading) = self.leading {
            row = row.child(leading);
        }
        row = row
            .child(disclosure_slot().child(disclosure_icon(self.expanded, muted)))
            .when_some(self.label_icon, |d, icon| d.child(icon))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.foreground)
                    .when_some(self.label_max_width, |d, w| d.max_w(rems(w)))
                    .child(self.label),
            )
            .child(div().flex_1());

        for el in self.trailing {
            row = row.child(el);
        }
        if !self.hover_actions.is_empty() {
            let mut actions = div()
                .flex_none()
                .invisible()
                .group_hover(group_id.clone(), |s| s.visible())
                .h_flex()
                .items_center()
                .gap_0p5();
            for el in self.hover_actions {
                actions = actions.child(el);
            }
            row = row.child(actions);
        }
        if let Some(f) = self.on_click {
            row = row.cursor_pointer().on_click(f);
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::{RowHeight, indent_rem, row_sizes};
    use gpui_kit::px;

    #[test]
    fn row_height_accumulates_blocks_and_scales_with_font_size() {
        let h = RowHeight::new(1.5)
            .add(0.5)
            .add_if(false, 3.0)
            .add_if(true, 1.0);
        assert_eq!(h.rem(), 3.0);
        // 16px 字号下 3rem = 48px；20px 字号下 = 60px（rem 基准是主题字号，不是 4px）
        assert_eq!(h.to_pixels(px(16.)), px(48.));
        assert_eq!(h.to_pixels(px(20.)), px(60.));
    }

    #[test]
    fn row_sizes_covers_every_row_with_zero_width() {
        let sizes = row_sizes(3, px(16.), |ix| RowHeight::new(1.0 + ix as f32));
        assert_eq!(sizes.len(), 3, "行数必须与渲染的行数一致");
        assert_eq!(sizes[0].height, px(16.));
        assert_eq!(sizes[2].height, px(48.));
        assert!(sizes.iter().all(|s| s.width == px(0.)), "宽度由行自己撑满");
    }

    #[test]
    fn indent_is_base_plus_depth_times_step() {
        // 导航：基础内距 0.5rem + 0.875rem × 深度；草稿箱：无基础内距。
        assert_eq!(indent_rem(0.5, 0.875, 0), 0.5);
        assert_eq!(indent_rem(0.5, 0.875, 2), 2.25);
        assert_eq!(indent_rem(0.0, 0.875, 3), 2.625);
    }
}
