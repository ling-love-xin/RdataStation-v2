//! 结果集标签条（B2；B5 在这里接通道徽标与血缘摘要）
//!
//! 结果集列表在 [`crate::store::ResultStore`]（唯一权威），本文件只把它**投影**成标签：
//! `结果 N` + 通道徽标 + 失败时的 `danger` 点。**不持有数据**——点击只回传下标，由宿主去改选中项。
//!
//! 标签条只有一份结果时不画（一份结果不需要切换器）：这条取舍在宿主
//! （`EditorHostPanel::render`），因为“结果区该出现哪些区块”属面板判断。

use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Size, Sizable as _};
use gpui_kit::*;

use crate::store::ResultEntry;
use crate::ui;
use workbench_shell::model::ExecChannel;

/// 【C1】钉住图标（lucide pin）。`gpui-component::IconName` 兼容宏只暴露
/// `default-icons.txt` 子集（无 pin），而 `Icon::data` 吃内联 SVG 字节——直接用它。
const PIN_ICON_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/></svg>"#;

/// 【C1】标签条每个 tab 的「⋯」菜单动作回调（宿主侧传入；对比栏传 `None` = 不画菜单按钮）
#[derive(Default, Clone)]
pub struct TabActions {
    /// 用户点了「重命名」：宿主进入重命名输入态（弹输入框），输入确认后再发 RenameSet
    pub on_rename: Option<Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>>,
    /// 用户点了「钉住 / 取消钉住」：宿主直接切 store.pinned
    pub on_toggle_pin: Option<Rc<dyn Fn(usize) + 'static>>,
}

/// 一个结果集标签的展示数据（**纯计算**：渲染路径不临时算这些）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultSetTab {
    /// 标签文案（`结果 N`；用户重命名后是自定义别名）
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
    /// 【C1】这份结果是否被用户钉住（标签 prefix 画小图钉；淘汰时跳过）
    pub pinned: bool,
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
/// `pinned` 与 `sets` 同序（空切片 = 全部未钉住；宿主侧 [`crate::store::ResultStore`] 提供）。
pub fn tabs(sets: &[ResultEntry], current: ExecChannel, pinned: &[bool]) -> Vec<ResultSetTab> {
    sets.iter()
        .enumerate()
        .map(|(index, entry)| ResultSetTab {
            // 【B10】有自定义标题就用它（如「执行计划」/ 用户重命名别名），否则按序号
            label: entry
                .title
                .clone()
                .unwrap_or_else(|| format!("结果 {}", index + 1)),
            failed: entry.failed(),
            channel: entry.channel,
            stale: entry.channel != current,
            // 【C1】钉住标记：pinned 切片越界按未钉住处理（防御投影错位）
            pinned: pinned.get(index).copied().unwrap_or(false),
        })
        .collect()
}

/// 渲染标签条（`on_pick` 收下标；`active` 是当前选中的结果集）
///
/// `scroll_handle`：结果集标签会随查询累积增多（溢出），由宿主实体持有并传给
/// TabBar（`track_scroll` 让点击标签滚入视野；`menu(true)` 溢出时收敛为「更多」菜单）。
///
/// `actions`：tab 上的「⋯」菜单与钉住按钮回调；对比栏传 [`TabActions::default()`]
/// （不画 ⋯ 按钮、不画钉住图标）。
pub fn render(
    tabs: &[ResultSetTab],
    active: usize,
    on_pick: impl Fn(&usize, &mut Window, &mut App) + 'static,
    actions: TabActions,
    cx: &App,
    scroll_handle: &ScrollHandle,
) -> impl IntoElement {
    let danger = cx.theme().colors.danger;
    // 【B13】徽标两档颜色：当前通道的结果用主色（“这就是现在这档”），
    // 旧结果用静色（标灰）；颜色只是辅助，白话在徽标文案（`·旧`）与顶部提示里
    let primary = cx.theme().colors.primary;
    let muted = cx.theme().colors.muted_foreground;
    let children: Vec<Tab> = tabs
        .iter()
        .enumerate()
        .map(|(index, entry_tab)| {
            // 徽标与失败点合并成一个后缀：`Tab::suffix` 只有一个位置（调两次是覆盖，不是叠加）
            let mut suffix = div()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(if entry_tab.stale { muted } else { primary })
                .child(SharedString::from(entry_tab.badge()));
            if entry_tab.failed {
                // 失败的结果集要在一排标签里看得出来（否则“哪句错了”要靠逐个点开）
                suffix = suffix.child(
                    div()
                        .size(rems(ui::STATUS_DOT_SIZE))
                        .rounded_full()
                        .bg(danger),
                );
            }
            // 【C1】后缀末尾追加「⋯」小按钮：点击进重命名态（compare 栏 actions 全 None 时不画）
            if let Some(on_rename) = actions.on_rename.clone() {
                let idx = index;
                suffix = suffix.child(
                    Button::new(SharedString::from(format!("editor-result-tab-rename-{index}")))
                        .ghost()
                        .small()
                        .icon(IconName::Ellipsis)
                        .on_click(move |_e, w, cx| on_rename(idx, w, cx)),
                );
            }
            // 【C1】prefix：钉住时画小图钉（点击取消钉住；未钉住时不占位，避免每个 tab 都挤）
            let mut tab_el = Tab::new()
                .label(SharedString::from(entry_tab.label.clone()))
                .suffix(suffix);
            if entry_tab.pinned {
                let pin_click = actions.on_toggle_pin.clone();
                let pin_button = Button::new(SharedString::from(format!("editor-result-tab-unpin-{index}")))
                    .ghost()
                    .small()
                    .icon(Icon::default().data(PIN_ICON_SVG));
                let pin_button = match pin_click {
                    Some(on_toggle_pin) => {
                        let idx = index;
                        pin_button.on_click(move |_e, _w, _cx| on_toggle_pin(idx))
                    }
                    None => pin_button,
                };
                tab_el = tab_el.prefix(pin_button);
            }
            tab_el
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
                .track_scroll(scroll_handle)
                .menu(true)
                .selected_index(active)
                .children(children)
                .on_click(on_pick),
        )
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::tabs;
    use crate::store::ResultEntry;
    use workbench_shell::model::{DocumentId, ExecChannel};

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
            &[],
        );

        let labels: Vec<&str> = rendered.iter().map(|tab| tab.label.as_str()).collect();
        assert_eq!(labels, ["结果 1", "结果 2", "结果 3"]);
        let failed: Vec<bool> = rendered.iter().map(|tab| tab.failed).collect();
        assert_eq!(failed, [false, true, false]);
    }

    /// 【B10】有自己的标题就用它（如「执行计划」），否则按序号——
    /// 用户看标签就知道那份结果是什么，而不用去悬停摘要里找
    #[test]
    fn a_custom_title_replaces_the_numbered_label() {
        let sets = [
            ok("select 1"),
            ok("EXPLAIN select 1").with_title("执行计划"),
        ];
        let labels: Vec<String> = tabs(&sets, ExecChannel::Source, &[])
            .into_iter()
            .map(|tab| tab.label)
            .collect();
        assert_eq!(labels, ["结果 1".to_string(), "执行计划".to_string()]);
    }

    /// 【B13】标签必带通道徽标；来自别的通道的标 `·旧`（原型 §5.7 规则 2）
    #[test]
    fn tabs_carry_the_channel_badge_and_mark_the_old_ones() {
        let sets = [
            ok_on("select 1", ExecChannel::Source),
            ok_on("select 2", ExecChannel::Accelerated),
        ];
        // 当前在源库：第一份是“当前的”，第二份是旧结果
        let first = tabs(&sets, ExecChannel::Source, &[]);
        assert_eq!(first[0].badge(), "源库");
        assert!(!first[0].stale);
        assert_eq!(first[1].badge(), "加速·旧", "旧结果要看得出来不是这档跑的");
        assert!(first[1].stale);

        // 切到加速档：角色反过来——徽标不撒谎，只是“旧”的归属变了
        let second = tabs(&sets, ExecChannel::Accelerated, &[]);
        assert_eq!(second[0].badge(), "源库·旧");
        assert_eq!(second[1].badge(), "加速");

        // 联邦徽标短码也是两字（与 `label` 不同：标签里挤不下“本地加速”）
        let federated = tabs(&[ok_on("select 3", ExecChannel::Federated)], ExecChannel::Source, &[]);
        assert_eq!(federated[0].badge(), "联邦·旧");
    }

    /// 没有结果就没有标签（宿主据此不画标签条）
    #[test]
    fn no_results_means_no_tabs() {
        assert!(tabs(&[], ExecChannel::Source, &[]).is_empty());
    }

    /// 【C1】pinned 切片投影到标签：越界按未钉住；命中的标签带 pinned=true
    #[test]
    fn pinned_slice_projects_into_tabs() {
        let sets = [ok("select 1"), ok("select 2"), ok("select 3")];
        let rendered = tabs(&sets, ExecChannel::Source, &[true, false]);
        assert_eq!(
            rendered.iter().map(|t| t.pinned).collect::<Vec<_>>(),
            [true, false, false],
            "pinned 切片越界按未钉住处理"
        );
    }
}
