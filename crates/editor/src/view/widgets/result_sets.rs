//! 结果集标签条（B2；B5 在这里接通道徽标与血缘摘要）
//!
//! 结果集列表在 [`crate::store::ResultStore`]（唯一权威），本文件只把它**投影**成标签：
//! `结果 N` + 失败时的 `danger` 点。**不持有数据**——点击只回传下标，由宿主去改选中项。
//!
//! 标签条只有一份结果时不画（一份结果不需要切换器）：这条取舍在宿主
//! （`EditorHostPanel::render`），因为“结果区该出现哪些区块”属面板判断。

use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Size, Sizable as _};
use gpui_kit::*;

use crate::store::ResultEntry;
use crate::ui;

/// 一个结果集标签的展示数据（**纯计算**：渲染路径不临时算这些）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultSetTab {
    /// 标签文案（`结果 N`）
    pub label: String,
    /// 失败的结果集（标签带 `danger` 点）——原型 §2.4
    pub failed: bool,
}

/// 结果集列表 → 标签（纯函数；B5 的通道徽标 / 血缘摘要接在这里）
pub fn tabs(sets: &[ResultEntry]) -> Vec<ResultSetTab> {
    sets.iter()
        .enumerate()
        .map(|(index, entry)| ResultSetTab {
            label: format!("结果 {}", index + 1),
            failed: entry.failed(),
        })
        .collect()
}

/// 渲染标签条（`on_pick` 收下标；`active` 是当前选中的结果集）
pub fn render(
    tabs: &[ResultSetTab],
    active: usize,
    on_pick: impl Fn(&usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let danger = cx.theme().colors.danger;
    let children: Vec<Tab> = tabs
        .iter()
        .map(|tab| {
            let mut rendered = Tab::new().label(SharedString::from(tab.label.clone()));
            if tab.failed {
                // 失败的结果集要在一排标签里看得出来（否则“哪句错了”要靠逐个点开）
                rendered = rendered.suffix(
                    div()
                        .size(rems(ui::STATUS_DOT_SIZE))
                        .rounded_full()
                        .bg(danger),
                );
            }
            rendered
        })
        .collect();

    div()
        // 结构尺寸走常量表；TabBar 自身的高度由组件库按 size 档位决定（Small = 24px）
        .h(rems(ui::RESULT_TABS_HEIGHT))
        // 测试按选择器断言原型 §2.4 的竖向顺序（⑤ 在 ⑥ 之上）
        .debug_selector(|| "editor-result-tabs".to_string())
        .child(
            TabBar::new("editor-result-sets")
                .segmented()
                .with_size(Size::Small)
                .selected_index(active)
                .children(children)
                .on_click(on_pick),
        )
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::tabs;
    use crate::model::DocumentId;
    use crate::store::ResultEntry;

    fn ok(sql: &str) -> ResultEntry {
        ResultEntry::success(
            DocumentId::new("doc-1"),
            sql.to_string(),
            5,
            false,
            vec!["n".to_string()],
            vec![vec!["1".to_string()]],
        )
    }

    fn failed(sql: &str) -> ResultEntry {
        ResultEntry::failure(
            DocumentId::new("doc-1"),
            sql.to_string(),
            "驱动报错：boom".to_string(),
            3,
        )
    }

    /// 标签按产生顺序编号，失败项带标记（批量跑的三个结果要一眼看出哪个错了）
    #[test]
    fn labels_are_numbered_in_order_and_failures_are_marked() {
        let tabs = tabs(&[ok("select 1"), failed("select boom"), ok("select 3")]);

        let labels: Vec<&str> = tabs.iter().map(|tab| tab.label.as_str()).collect();
        assert_eq!(labels, ["结果 1", "结果 2", "结果 3"]);
        let failed: Vec<bool> = tabs.iter().map(|tab| tab.failed).collect();
        assert_eq!(failed, [false, true, false]);
    }

    /// 没有结果就没有标签（宿主据此不画标签条）
    #[test]
    fn no_results_means_no_tabs() {
        assert!(tabs(&[]).is_empty());
    }
}
