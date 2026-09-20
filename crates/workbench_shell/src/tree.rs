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

use gpui_kit::component::Icon;
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
