//! 结果集标签条（B2；B5 在这里接通道徽标与血缘摘要）
//!
//! 结果集列表在 [`crate::store::ResultStore`]（唯一权威），本文件只把它**投影**成标签：
//! `结果 N` + 通道徽标 + 失败时的 `danger` 点。**不持有数据**——点击只回传下标，由宿主去改选中项。
//!
//! 标签条只有一份结果时不画（一份结果不需要切换器）：这条取舍在宿主
//! （`EditorHostPanel::render`），因为“结果区该出现哪些区块”属面板判断。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Size, Sizable as _};
use gpui_kit::*;

use crate::channel::ExecChannel;
use crate::store::ResultEntry;
use crate::ui;

/// 一个结果集标签的展示数据（**纯计算**：渲染路径不临时算这些）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultSetTab {
    /// 标签文案（`结果 N`）
    pub label: String,
    /// 失败的结果集（标签带 `danger` 点）——原型 §2.4
    pub failed: bool,
    /// 【B13】这份结果是在哪个通道上跑出来的（标签上的徽标）
    pub channel: ExecChannel,
    /// 【B13】它是不是“旧结果”（来自当前通道**以外**的通道）
    ///
    /// 原型 §5.7 规则 2：切通道后旧结果**保留但标灰**——不删用户的东西，但也不能
    /// 让它看起来像刚跑出来的。
    pub stale: bool,
}

impl ResultSetTab {
    /// 徽标文案：`源库` / `加速` / `联邦`；旧结果缀 `·旧`
    ///
    /// 不能只靠颜色表达“旧”：色差在浅色主题与低对比屏上不够看，而“这份数据不是
    /// 我这个通道跑出来的”是**能不能信它**的事（原型 §5.7 那张能力表）。
    pub fn badge(&self) -> String {
        if self.stale {
            format!("{}·旧", self.channel.badge())
        } else {
            self.channel.badge().to_string()
        }
    }
}

/// 结果集列表 → 标签（纯函数；血缘摘要接在这里）
///
/// `current` = 文档当前的执行通道：与它不同的结果集标 `stale`（旧结果标灰）。
pub fn tabs(sets: &[ResultEntry], current: ExecChannel) -> Vec<ResultSetTab> {
    sets.iter()
        .enumerate()
        .map(|(index, entry)| ResultSetTab {
            label: format!("结果 {}", index + 1),
            failed: entry.failed(),
            channel: entry.channel,
            stale: entry.channel != current,
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
    // 【B13】徽标两档颜色：当前通道的结果用主色（“这就是现在这档”），
    // 旧结果用静色（标灰）；颜色只是辅助，白话在徽标文案（`·旧`）与顶部提示里
    let primary = cx.theme().colors.primary;
    let muted = cx.theme().colors.muted_foreground;
    let children: Vec<Tab> = tabs
        .iter()
        .map(|tab| {
            // 徽标与失败点合并成一个后缀：`Tab::suffix` 只有一个位置（调两次是覆盖，不是叠加）
            let mut suffix = div()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(if tab.stale { muted } else { primary })
                .child(SharedString::from(tab.badge()));
            if tab.failed {
                // 失败的结果集要在一排标签里看得出来（否则“哪句错了”要靠逐个点开）
                suffix = suffix.child(
                    div()
                        .size(rems(ui::STATUS_DOT_SIZE))
                        .rounded_full()
                        .bg(danger),
                );
            }
            Tab::new()
                .label(SharedString::from(tab.label.clone()))
                .suffix(suffix)
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
    use crate::channel::ExecChannel;
    use crate::model::DocumentId;
    use crate::store::ResultEntry;

    fn ok(sql: &str) -> ResultEntry {
        ok_on(sql, ExecChannel::Source)
    }

    fn ok_on(sql: &str, channel: ExecChannel) -> ResultEntry {
        ResultEntry::success(
            DocumentId::new("doc-1"),
            sql.to_string(),
            5,
            false,
            vec!["n".to_string()],
            vec![vec!["1".to_string()]],
        )
        .with_channel(channel)
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
        let rendered = tabs(
            &[ok("select 1"), failed("select boom"), ok("select 3")],
            ExecChannel::Source,
        );

        let labels: Vec<&str> = rendered.iter().map(|tab| tab.label.as_str()).collect();
        assert_eq!(labels, ["结果 1", "结果 2", "结果 3"]);
        let failed: Vec<bool> = rendered.iter().map(|tab| tab.failed).collect();
        assert_eq!(failed, [false, true, false]);
    }

    /// 【B13】标签必带通道徽标；来自别的通道的标 `·旧`（原型 §5.7 规则 2）
    #[test]
    fn tabs_carry_the_channel_badge_and_mark_the_old_ones() {
        let sets = [
            ok_on("select 1", ExecChannel::Source),
            ok_on("select 2", ExecChannel::Accelerated),
        ];
        // 当前在源库：第一份是“当前的”，第二份是旧结果
        let first = tabs(&sets, ExecChannel::Source);
        assert_eq!(first[0].badge(), "源库");
        assert!(!first[0].stale);
        assert_eq!(first[1].badge(), "加速·旧", "旧结果要看得出来不是这档跑的");
        assert!(first[1].stale);

        // 切到加速档：角色反过来——徽标不撒谎，只是“旧”的归属变了
        let second = tabs(&sets, ExecChannel::Accelerated);
        assert_eq!(second[0].badge(), "源库·旧");
        assert_eq!(second[1].badge(), "加速");

        // 联邦徽标短码也是两字（与 `label` 不同：标签里挤不下“本地加速”）
        let federated = tabs(&[ok_on("select 3", ExecChannel::Federated)], ExecChannel::Source);
        assert_eq!(federated[0].badge(), "联邦·旧");
    }

    /// 没有结果就没有标签（宿主据此不画标签条）
    #[test]
    fn no_results_means_no_tabs() {
        assert!(tabs(&[], ExecChannel::Source).is_empty());
    }
}
