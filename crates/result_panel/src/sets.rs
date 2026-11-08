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
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, Size};
use gpui_kit::*;

use crate::store::ResultEntry;
use crate::ui;
use workbench_shell::model::ExecChannel;

/// 【C1】钉住图标（lucide pin）。`gpui-component::IconName` 兼容宏只暴露
/// `default-icons.txt` 子集（无 pin），而 `Icon::data` 吃内联 SVG 字节——直接用它。
const PIN_ICON_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/></svg>"#;

/// 【DBeaver 对齐】结果集标签的类型图标：DBeaver 每个结果 tab 都挂
/// `SQLEditor.IMG_DATA_GRID`（`MultiTabsQueryResultsContainer.java:73`），一排骨并排时
/// 靠「图标 + 序号」扫视比纯文字快。
///
/// 走**资产路径**而不是 `IconName`：`gpui-component::IconName` 只保留
/// `default-icons.txt` 的 106 个兼容图标（其中没有任何 table / grid），而应用层注册的是
/// `gpui_kit::assets::AllAssets`（全量 Lucide 1818 个），所以按路径直接引用即可
/// ——与 `workbench_shell::model::LeftPanel::icon` 同一口径。
const RESULT_SET_ICON: &str = "icons/table.svg";


/// 【C1】标签条每个 tab 的「⋯」菜单动作回调（宿主侧传入；对比栏传 `None` = 不画菜单按钮）
#[derive(Default, Clone)]
pub struct TabActions {
    /// 用户点了「重命名」：宿主进入重命名输入态（弹输入框），输入确认后再发 RenameSet
    pub on_rename: Option<Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>>,
    /// 用户点了「钉住 / 取消钉住」：宿主直接切 store.pinned
    /// （签名带 `&mut App`：按钮点击经 [`ResultActionQueue::request`] 立即通知面板 → 宿主 drain）
    pub on_toggle_pin: Option<Rc<dyn Fn(usize, &mut App) + 'static>>,
    /// 【DBeaver 对齐】用户点了标签上的关闭按钮：宿主从 store 移除该结果集
    /// （`None` = 不画关闭按钮，对比栏就是这种）
    pub on_close: Option<Rc<dyn Fn(usize, &mut App) + 'static>>,
    /// 【DBeaver 对齐】标签拖拽重排：`(被拖的下标, 落点下标)`
    /// （`None` = 标签不可拖，对比栏就是这种）
    pub on_reorder: Option<Rc<dyn Fn(usize, usize, &mut Window, &mut App) + 'static>>,
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
    /// 【DBeaver 对齐】产生这份结果的**原始 SQL**：标签宽度有限、文案只有「结果 N」，
    /// 悬停提示里给出原始 SQL 才能确认「这个标签对应哪条语句」
    /// （`MultiTabsQueryResultsContainer.java:96-103` 的 `setToolTipText(toolTip)`）。
    pub sql: String,
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
/// `sets` 吃**引用切片**（条目内聚在 store 槽位里，[`crate::store::ResultStore::sets`]
/// 收集引用返回）；`pinned` 与 `sets` 同序（空切片 = 全部未钉住；宿主侧提供）。
pub fn tabs(sets: &[&ResultEntry], current: ExecChannel, pinned: &[bool]) -> Vec<ResultSetTab> {
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
            sql: entry.sql.clone(),
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
                    Button::new(SharedString::from(format!(
                        "editor-result-tab-rename-{index}"
                    )))
                    .ghost()
                    .small()
                    .icon(IconName::Ellipsis)
                    .debug_selector(|| format!("editor-result-tab-rename-{index}"))
                    .tooltip("重命名结果集".to_string())
                    .on_click(move |_e, w, cx| on_rename(idx, w, cx)),
                );
            }
            // 【DBeaver 对齐】后缀最右端是关闭按钮（对应它的 `resultsTab.setShowClose(true)`）。
            // 钉住的结果集同样可关：钉住挡的是自动淘汰，不是用户主动关。
            if let Some(on_close) = actions.on_close.clone() {
                let idx = index;
                suffix = suffix.child(
                    Button::new(SharedString::from(format!(
                        "editor-result-tab-close-{index}"
                    )))
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .debug_selector(|| format!("editor-result-tab-close-{index}"))
                    .tooltip("关闭这个结果集".to_string())
                    .on_click(move |_e, _w, cx| on_close(idx, cx)),
                );
            }
            // 【DBeaver 对齐】prefix = 结果集类型图标 + （钉住时）小图钉按钮。
            // 两者的顺序按 DBeaver：图标贴在最左，钉住是状态标记紧随其后。
            let mut prefix = div()
                .h_flex()
                .items_center()
                .gap_1()
                .debug_selector(|| format!("editor-result-tab-prefix-{index}"))
                .child(
                    // 类型图标（视觉识别用；与徽标文案分工：图标给一眼定位，文字给准确归类）
                    Icon::default()
                        .path(RESULT_SET_ICON)
                        .with_size(Size::Small),
                );
            if entry_tab.pinned {
                let pin_click = actions.on_toggle_pin.clone();
                let pin_button = Button::new(SharedString::from(format!(
                    "editor-result-tab-unpin-{index}"
                )))
                .ghost()
                .small()
                .icon(Icon::default().data(PIN_ICON_SVG));
                let pin_button = match pin_click {
                    Some(on_toggle_pin) => {
                        let idx = index;
                        pin_button.on_click(move |_e, _w, cx| on_toggle_pin(idx, cx))
                    }
                    None => pin_button,
                };
                prefix = prefix.child(pin_button);
            }
            let mut tab_el = Tab::new()
                .label(SharedString::from(entry_tab.label.clone()))
                .prefix(prefix)
                .suffix(suffix);
            // 【DBeaver 对齐】悬停显示原始 SQL（空 SQL 不挂，避免一个空白气泡）
            if !entry_tab.sql.is_empty() {
                tab_el = tab_el.tooltip(SharedString::from(entry_tab.sql.clone()));
            }
            tab_el
        })
        .collect();

    let mut tab_bar = TabBar::new("editor-result-sets")
        // 【DBeaver 对齐】tab 样式和 SQL 编辑器一致（TabVariant::Tab，不是 Folder）；
        // size 也跟着编辑器走——dock 的 `TabBar::new("tab-bar")` 没写 size（默认
        // Medium = 32px），这里若留在 Small 档就是 24px，比文件标签条矮 8px。
        .with_size(Size::Medium)
        .track_scroll(scroll_handle)
        .menu(true)
        .selected_index(active)
        .children(children)
        .on_click(on_pick);
    // 【DBeaver 对齐】拖拽重排（DBeaver 在结果集标签条上装 `TabFolderReorder`，
    // `SQLEditor.java:1518`）；没接线时不启用，标签就是普通可点标签。
    if let Some(on_reorder) = actions.on_reorder.clone() {
        tab_bar = tab_bar.on_reorder(move |from, to, window, cx| on_reorder(from, to, window, cx));
    }

    div()
        // 结构尺寸走常量表；TabBar 自身的高度由组件库按 size 档位决定（Medium = 32px）
        .h(rems(ui::RESULT_TABS_HEIGHT))
        // 测试按选择器断言原型 §2.4 的竖向顺序（⑤ 在 ⑥ 之上）
        .debug_selector(|| "editor-result-tabs".to_string())
        .child(tab_bar)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{render, tabs, ResultSetTab, TabActions};
    use crate::store::ResultEntry;
    use gpui_kit::{
        div, AssetSource as _, Context, IntoElement, ParentElement as _, Render, ScrollHandle,
        Styled as _, Window,
    };
    use workbench_shell::model::{DocumentId, ExecChannel};

    /// 只画标签条的最小宿主：几何断言需要真实排一遍（`render` 要 `&App` 取主题色）
    struct TabsHarness {
        tabs: Vec<ResultSetTab>,
        scroll: ScrollHandle,
        actions: TabActions,
    }

    impl TabsHarness {
        fn new(tabs: Vec<ResultSetTab>) -> Self {
            Self {
                tabs,
                scroll: ScrollHandle::new(),
                actions: TabActions::default(),
            }
        }

        fn with_actions(tabs: Vec<ResultSetTab>, actions: TabActions) -> Self {
            Self {
                tabs,
                scroll: ScrollHandle::new(),
                actions,
            }
        }
    }

    impl Render for TabsHarness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .w_full()
                .child(render(
                    &self.tabs,
                    0,
                    |_, _, _| {},
                    self.actions.clone(),
                    cx,
                    &self.scroll,
                ))
        }
    }

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

    /// 测试辅助：`&[ResultEntry]` → `Vec<&ResultEntry>`（`tabs` 吃引用切片）
    fn refs(sets: &[ResultEntry]) -> Vec<&ResultEntry> {
        sets.iter().collect()
    }

    /// 标签按产生顺序编号，失败项带标记（批量跑的三个结果要一眼看出哪个错了）
    #[test]
    fn labels_are_numbered_in_order_and_failures_are_marked() {
        let rendered = tabs(
            &refs(&[ok("select 1"), failed("select boom"), ok("select 3")]),
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
        let labels: Vec<String> = tabs(&refs(&sets), ExecChannel::Source, &[])
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
        let first = tabs(&refs(&sets), ExecChannel::Source, &[]);
        assert_eq!(first[0].badge(), "源库");
        assert!(!first[0].stale);
        assert_eq!(first[1].badge(), "加速·旧", "旧结果要看得出来不是这档跑的");
        assert!(first[1].stale);

        // 切到加速档：角色反过来——徽标不撒谎，只是“旧”的归属变了
        let second = tabs(&refs(&sets), ExecChannel::Accelerated, &[]);
        assert_eq!(second[0].badge(), "源库·旧");
        assert_eq!(second[1].badge(), "加速");

        // 联邦徽标短码也是两字（与 `label` 不同：标签里挤不下“本地加速”）
        let federated = tabs(
            &refs(&[ok_on("select 3", ExecChannel::Federated)]),
            ExecChannel::Source,
            &[],
        );
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
        let rendered = tabs(&refs(&sets), ExecChannel::Source, &[true, false]);
        assert_eq!(
            rendered.iter().map(|t| t.pinned).collect::<Vec<_>>(),
            [true, false, false],
            "pinned 切片越界按未钉住处理"
        );
    }

    /// 【DBeaver 对齐】标签要带着产生它的**原始 SQL**（悬停提示用）
    ///
    /// 标签文案只有「结果 N」，用户靠悬停确认这份数据是哪条语句跑出来的；
    /// 多行 SQL 也原样保留（不能在投影这一层折叠成一行——提示里要看到原文）。
    #[test]
    fn tabs_keep_the_source_sql_for_the_tooltip() {
        let sets = [
            ok("select 1 from t"),
            ok("select 2\nfrom t\nwhere x > 1"),
        ];
        let rendered = tabs(&refs(&sets), ExecChannel::Source, &[]);
        assert_eq!(rendered[0].sql, "select 1 from t");
        assert_eq!(
            rendered[1].sql, "select 2\nfrom t\nwhere x > 1",
            "多行 SQL 原样保留"
        );
    }

    /// 标签条高度必须与编辑器文件标签条同档（32px）
    ///
    /// 编辑器那边是 dock 的 `TabBar::new("tab-bar")`，**不写 size** → `Size::default()`
    /// = Medium，`TabVariant::Tab` 在 Medium 档 32px（vendor `tab/tab.rs:79-82`）。
    /// 这里原先用 `Size::Small`（24px）且外层容器也写死 24px，比文件标签条矮 8px 且零余量，
    /// 用户直接看出"结果集标题比编辑器标签矮"。`TabVariant::height` 在 vendor 里是私有
    /// 函数，只能靠真渲染量——旧实现下这条断言读到 24。
    #[gpui_kit::test]
    fn tab_strip_matches_the_editor_dock_tab_height(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let sets = [ok("select 1"), ok("select 2")];
        let (_, vcx) = cx.add_window_view(|_, _| {
            TabsHarness::new(tabs(&refs(&sets), ExecChannel::Source, &[]))
        });
        // 首帧只排位置，第二帧才是稳定几何
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });

        let bounds = vcx
            .debug_bounds("editor-result-tabs")
            .expect("标签条容器必须带 debug_selector 才量得到");
        assert_eq!(
            bounds.size.height,
            gpui_kit::px(32.),
            "结果集标签条要与编辑器文件标签条等高（Medium 档 32px），实际 {}",
            bounds.size.height
        );
    }

    /// 【DBeaver 对齐】类型图标的资产路径必须真的能在全量目录里解析到
    ///
    /// `Icon::path` 走的是字符串路径，写错在界面上只是**静默空白**（不 panic、不报错），
    /// 而 headless 测试没注册 assets，渲染层也验不出来。所以这里直接问资产源：
    /// `gpui-kit-assets` 的 `AllAssets`（应用层注册的就是它，全量 1818 个 Lucide）能否
    /// 加载出这个路径——路径拼错会返回 `Err`，路径不存在返回 `Ok(None)`，两种都能拦住。
    #[test]
    fn the_result_set_icon_path_resolves_in_the_full_catalog() {
        let loaded = gpui_kit::assets::AllAssets
            .load(super::RESULT_SET_ICON)
            .expect("资产路径拼错会返回 Err（图标名写错最常见的坑）");
        let bytes = loaded.expect("全量目录里应有 icons/table.svg（lucide 1.43 收录）");
        assert!(
            !bytes.is_empty(),
            "icons/table.svg 不应是空文件，否则 tab 图标会渲染成空白"
        );
    }

    /// 【DBeaver 对齐】每个 tab 都带关闭按钮（对应 `resultsTab.setShowClose(true)`）
    ///
    /// 没传 `on_close` 时不画（对比栏就是这种：那栏的结果不能关）；传了就有，
    /// 且按钮是真实占位的（24×24 的小按钮），不是零宽空壳。
    #[gpui_kit::test]
    fn tabs_carry_a_close_button_only_when_the_host_wires_it(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let sets = [ok("select 1"), ok("select 2")];

        // ① 没有 on_close：不画关闭按钮
        let (_, vcx) = cx.add_window_view(|_, _| {
            TabsHarness::new(tabs(&refs(&sets), ExecChannel::Source, &[]))
        });
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        assert!(
            vcx.debug_bounds("editor-result-tab-close-0").is_none(),
            "宿主没接线时不该画关闭按钮"
        );
    }

    /// 传了 `on_close`：每个 tab 都有独立的关闭按钮，宽度是真实小按钮（24px）
    #[gpui_kit::test]
    fn wired_close_buttons_take_real_space(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let sets = [ok("select 1"), ok("select 2")];
        let closed = std::rc::Rc::new(std::cell::Cell::new(Vec::<usize>::new()));
        let actions = {
            let closed = closed.clone();
            TabActions {
                on_close: Some(std::rc::Rc::new(move |index, _app| {
                    let mut log = closed.take();
                    log.push(index);
                    closed.set(log);
                })),
                ..TabActions::default()
            }
        };
        let (_, vcx) = cx.add_window_view(|_, _| {
            TabsHarness::with_actions(
                tabs(&refs(&sets), ExecChannel::Source, &[]),
                actions.clone(),
            )
        });
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });

        // `debug_bounds` 只吃 `&'static str`，所以按字面量逐个量（本用例两个 tab）
        for selector in [
            "editor-result-tab-close-0",
            "editor-result-tab-close-1",
        ] {
            let bounds = vcx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("每个 tab 都应有关闭按钮：{selector} 没登记"));
            assert!(
                bounds.size.width >= gpui_kit::px(16.),
                "关闭按钮是真实小按钮（`small()` 档 24px），{selector} 实际宽 {}",
                bounds.size.width
            );
        }
        // 接线抽查：直接用回调本身验索引归属（headless 点 Button 不稳，索引错位
        // 这种 bug 由这条 + store 用例共同覆盖）
        vcx.update(|_, app| {
            if let Some(on_close) = actions.on_close.clone() {
                on_close(1, app);
            }
        });
        assert_eq!(closed.take(), vec![1], "回调收到的是被点那份的下标");
    }
}
