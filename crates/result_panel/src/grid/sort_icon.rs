//! DBeaver 列头排序三态图标（真值 SVG **原样**内嵌，16×16）
//!
//! 真值（`dbeaver-ref/.../lightgrid/GridColumnRenderer.java`）：
//! - 未排序：`SORT_UNKNOWN`（`icons/misc/sort_unknown.svg`）——上下两支带杆箭头；
//! - `SWT.UP`（升序）：`IMAGE_ASC = SORT_DECREASE`（`sort_decrease.svg`）——
//!   **图形是向上箭头**（DBeaver 这两个图标的命名与图形方向反着来，别照名字理解）；
//! - `SWT.DOWN`（降序）：`IMAGE_DESC = SORT_INCREASE`（`sort_increase.svg`）——向下箭头。
//!
//! 三张都是 16×16、`#3387c5` 硬编码填充（`:43-46`、`:212-226` 直接 `gc.drawImage`），
//! 所以这里**不跟随主题色**——真值与明暗主题无关。
//!
//! **为什么自绘**（2026-11-27 原生排查结论）：组件 `render_sort_icon`
//! （`vendor/gpui-component/src/table/state.rs:2087-2131`）画的是 `Icon::size_3()`（12px）的
//! Lucide `sort-ascending` / `sort-descending`——那是「一实一虚两个 chevron」，
//! 与真值「单支带杆箭头」不是同一个图形；`Column` 没有图标字段、`TableState` 也没有对应
//! builder，delegate 侧改不了它（图标是 `render_th` 里 delegate 内容之后的独立 slot，
//! `:2137-2178`）。`Icon::data()` 直接吃 SVG 字节，于是既不 patch vendor、也不动资产管线。

use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::table::ColumnSort;

/// 未排序（真值 `SORT_UNKNOWN`，上下两支带杆箭头）
const UNKNOWN: &[u8] = include_bytes!("../../assets/sort_unknown.svg");
/// 升序（真值 `SWT.UP → SORT_DECREASE`，图形是**向上**箭头）
const ASCENDING: &[u8] = include_bytes!("../../assets/sort_decrease.svg");
/// 降序（真值 `SWT.DOWN → SORT_INCREASE`，图形是向下箭头）
const DESCENDING: &[u8] = include_bytes!("../../assets/sort_increase.svg");

/// 按当前排序态取真值图标（16×16，真值 `SORT_WIDTH = IMAGE_DESC.getBounds().width`）
///
/// 尺寸走 [`crate::ui::RESULT_HEADER_SORT_SLOT`]：它与 `fit.rs` 的自适应宽算的是同一个槽宽，
/// 两处不同源就会让双击自适应出来的列宽差几个像素。
pub(crate) fn sort_icon(sort: ColumnSort) -> Icon {
    let data = match sort {
        ColumnSort::Ascending => ASCENDING,
        ColumnSort::Descending => DESCENDING,
        ColumnSort::Default => UNKNOWN,
    };
    Icon::empty()
        .data(data)
        .with_size(crate::ui::RESULT_HEADER_SORT_SLOT)
}
