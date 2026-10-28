//! 侧栏域（`panel/` 第四块）：文本视图 / 左缘竖条 / 列浏览器 / 右栏 / 右缘竖条 / 展开筛选条。

use std::time::Duration;

use gpui_kit::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
/// 【侧栏丝滑】宽度过渡用 `EffectTransition::width`（元素级动画，动画状态按元素 id
/// 挂在 gpui 元素树上——render 无需 `&mut App`；id 变化即重启，故用 epoch 区分每次
/// 开合，与 vendor sidebar 组件同款机制）。
use gpui_kit::base::animation::{ease_in_out_cubic, EffectTransition};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::list::List;
use gpui_kit::component::{ActiveTheme as _, Size, Sizable as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use crate::ports::{Presentation, ResultRequest, RightTab};
use crate::ui;
use super::data::ResultPanel;
use super::tabs::kv_row;

impl ResultPanel {

    pub(crate) fn render_text_view(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let mono = theme.mono_font_family.clone();
        let queue = self.queue.clone();
        let grid = self.grid.read(cx);
        let text = grid.delegate().text_presentation();
        div()
            .v_flex()
            .h_full()
            .min_h_0()
            .debug_selector(|| "editor-result-text-view-body".to_string())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .h_6()
                    .border_b(ui::HAIRLINE)
                    .child(
                        Button::new("editor-result-text-head-grid")
                            .ghost()
                            .small()
                            .debug_selector(|| "editor-result-text-head-grid".to_string())
                            .label("网格")
                            .on_click({
                                let queue = queue.clone();
                                move |_, _window, app| {
                                    queue.request(ResultRequest::SetPresentation(Presentation::Grid), app);
                                }
                            }),
                    )
                    .child(
                        Button::new("editor-result-text-head-text")
                            .ghost()
                            .small()
                            .toggled(true)
                            .label("文本"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .font_family(mono)
                            .text_xs()
                            .whitespace_normal()
                            .child(SharedString::from(text)),
                    ),
            )
            .into_any_element()
    }

    /// 【侧栏】左栏：列结构列表（列名 + 类型 + 隐藏切换）。
    ///
    /// 交互 = **单击整行切换隐藏**（隐藏列整行标灰，点击恢复）。不摆图标按钮：
    /// 眼睛类图标不在 kit 图标表，整行点击 + 标灰是同一件事，少一层控件。
    /// 隐藏 / 显示走队列交回宿主（宿主 update 网格 + 面板），与其它工具栏动作一致。
    /// 【左缘竖条】DBeaver 左缘独立窄条：网格 / 文本 / 记录三图标竖排。
    ///
    /// 状态口径（拍板「保留双轨」）：`Presentation{Grid,Text}` 与 `transposed` 仍是
    /// 两条独立状态——网格 / 文本由 `SetPresentation` 显式目标值切换；记录（转置视图）
    /// 由 `ToggleTranspose` 翻转，高亮跟随 `transposed`。转置激活时按钮下方跟上一行 /
    /// 下一行导航（记录视图里翻记录用）。文本态下记录按钮禁用（文本没有记录语义）。
    pub(crate) fn render_vertical_bar(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let is_text = self.presentation == Presentation::Text;
        let show = self.show_left_sidebar;
        let queue = self.queue.clone();
        let grid_btn = Button::new("editor-result-bar-grid")
            .ghost()
            .small()
            .icon(IconName::Table2)
            .debug_selector(|| "editor-result-bar-grid".to_string())
            .toggled(!self.transposed && !is_text)
            .tooltip("网格视图")
            .on_click(move |_, _window, app| {
                queue.request(ResultRequest::SetPresentation(Presentation::Grid), app);
            });
        let queue = self.queue.clone();
        let text_btn = Button::new("editor-result-bar-text")
            .ghost()
            .small()
            .icon(IconName::FileText)
            .debug_selector(|| "editor-result-bar-text".to_string())
            .toggled(is_text)
            .tooltip("文本视图")
            .on_click(move |_, _window, app| {
                queue.request(ResultRequest::SetPresentation(Presentation::Text), app);
            });
        let queue = self.queue.clone();
        let record_btn = Button::new("editor-result-bar-record")
            .ghost()
            .small()
            // 【记录视图图标】单行转置（# / 字段 / 值 竖排）是「一条记录的字段列表」，
            // 用 Lucide `form`（三条横档的表单条）；原 `UserRound`（人形）语义是"用户"，
            // 与记录无关，且与列浏览器里的其他图标气质不符。
            .icon(IconName::Form)
            .debug_selector(|| "editor-result-bar-record".to_string())
            .disabled(is_text)
            .toggled(self.transposed)
            .tooltip("记录视图（单行转置）")
            .on_click(move |_, _window, app| {
                queue.request(ResultRequest::ToggleTranspose, app);
            });
        // 记录视图激活时：竖条按钮下方跟上一行 / 下一行导航（与原工具栏转置组同功能，
        // 只是位置从顶部迁到竖条——动作仍是队列里的 TransposePrev / Next）。
        let record_nav = self.transposed.then(|| {
            let queue = self.queue.clone();
            let prev = Button::new("editor-result-bar-record-prev")
                .ghost()
                .small()
                .icon(IconName::ChevronUp)
                .debug_selector(|| "editor-result-bar-record-prev".to_string())
                .tooltip("上一行")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::TransposePrev, app);
                });
            let queue = self.queue.clone();
            let next = Button::new("editor-result-bar-record-next")
                .ghost()
                .small()
                .icon(IconName::ChevronDown)
                .debug_selector(|| "editor-result-bar-record-next".to_string())
                .tooltip("下一行")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::TransposeNext, app);
                });
            div()
                .v_flex()
                .gap_1()
                .items_center()
                .child(prev)
                .child(next)
                .into_any_element()
        });
        // 【侧栏丝滑】左栏收起 / 展开 = 宽度过渡（EffectTransition 元素级动画，与
        // vendor `sidebar` 组件同款机制：动画状态按 id 挂元素树，render 无需 &mut App）。
        // 容器**始终挂载**（动画驱动宽度，收起后宽 0 不占空间）、内容按 show 挂载——
        // 展开时内容随宽度露出，收起时宽度平滑收缩，消除「条件渲染瞬爆」。每次开合
        // `left_sidebar_epoch` 递增 → 动画 id 变化 → 重播；首帧（epoch=0）from==to 免动画。
        // 注：动画宽度按 gpui 默认 rem 基准（16px）换算，zoom 改 rem_size 属已知限制。
        let full = px(ui::RESULT_VERTICAL_BAR_WIDTH * 16.0);
        let (from, to) = sidebar_width_anim(self.left_sidebar_epoch, show, full);
        let wrapper = div()
            .h_full()
            .flex_none()
            .overflow_hidden()
            .border_r(ui::HAIRLINE)
            .bg(theme.colors.sidebar)
            .debug_selector(|| "editor-result-vertical-bar".to_string())
            .when(show, |el| {
                el.v_flex()
                    .items_center()
                    .gap_1()
                    .pt_2()
                    .child(grid_btn)
                    .child(text_btn)
                    .child(record_btn)
                    .children(record_nav)
            });
        EffectTransition::new(Duration::from_millis(160))
            .ease(ease_in_out_cubic)
            .width(from, to)
            .apply(
                wrapper,
                SharedString::from(format!(
                    "editor-result-left-bar-w-{}",
                    self.left_sidebar_epoch
                )),
            )
            .into_any_element()
    }

    /// 【右缘竖条】最右侧边缘面板图标条（对齐 DBeaver 最右缘）：
    /// - 顶部：右侧边栏开合开关（点击切换 show_value_sidebar）
    /// - 下方：预留面板切换图标位置（图表/统计等，backlog）
    ///
    /// 宽度与左缘竖条一致；背景 sidebar；按钮 ghost 小尺寸。
    /// 【右栏·列浏览器页签】（对齐 DBeaver metadata 面板）：顶部搜索框（kit List 内置）+
    /// 中部列清单（kit List）+ 底部元数据明细，整体作为右栏「列浏览器」页签内容。
    ///
    /// - 搜索：列名 / 类型名 / 注释忽略大小写包含匹配（List 内置搜索框 → `perform_search`）；
    ///   隐藏列保留在清单（置灰斜体）。
    /// - 清单：单击选中列（List `confirm` → 网格列高亮联动），双击定位到该列首行；
    ///   行尾眼睛 = 切换隐藏（单击眼睛不改变选中列）；冻结 / 排序 / 统计 / 复制 / 显示全部
    ///   走右键菜单（清单行上不摆图钉 / 排序箭头 / 拖拽手柄，见拍板口径）。
    /// - 明细：只读 `selected_col`（越界 / 被隐藏时兜底第一可见列）。
    pub(crate) fn render_browser_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let columns = delegate.columns().to_vec();
        let types = delegate.column_types().to_vec();
        let annotations = delegate.column_annotations().to_vec();
        let type_names: Vec<String> = types
            .iter()
            .map(|t| t.as_ref().map(|t| t.name.clone()).unwrap_or_default())
            .collect();

        // 明细兜底：当前选中列越界 / 被隐藏时退回第一可见列；一份结果无可见列 = None
        let detail_col = self
            .selected_col
            .filter(|c| *c < columns.len() && !delegate.is_hidden_column(*c))
            .or_else(|| (0..columns.len()).find(|c| !delegate.is_hidden_column(*c)));

        // 明细素材在 read 期间一次取好（标题 + 键值行）；渲染闭包不持有 delegate 借用
        let detail: Option<(String, Vec<(String, String)>)> = detail_col.and_then(|data_col| {
            // 【修复】中间态：columns 非空但 type_names 为空时越界——加边界检查
            if data_col >= columns.len() || data_col >= type_names.len() {
                return None;
            }
            let name = columns[data_col].clone();
            let type_name = type_names[data_col].clone();
            let title = if type_name.is_empty() {
                name.clone()
            } else {
                format!("{name} · {type_name}")
            };
            let mut items: Vec<(String, String)> = vec![
                (
                    "可见".into(),
                    if delegate.is_hidden_column(data_col) {
                        "隐藏".to_string()
                    } else {
                        "显示".to_string()
                    },
                ),
                (
                    "冻结".into(),
                    if delegate.is_frozen(data_col) {
                        "是".to_string()
                    } else {
                        "否".to_string()
                    },
                ),
            ];
            if let Some(annotation) = annotations.get(data_col).cloned().flatten() {
                if let Some(table) = annotation.source_table {
                    items.push(("来源表".into(), table));
                }
                if let Some(source) = annotation.source_column {
                    items.push(("来源列".into(), source));
                }
                if let Some(comment) = annotation.comment {
                    items.push(("注释".into(), comment));
                }
                if let Some(data_type) = annotation.data_type {
                    items.push(("驱动类型".into(), data_type));
                }
                if let Some(nullable) = annotation.nullable {
                    items.push(("可空".into(), if nullable { "是".into() } else { "否".into() }));
                }
                if let Some(default_value) = annotation.default_value {
                    items.push(("默认值".into(), default_value));
                }
            }
            Some((title, items))
        });

        // 清单：kit List（搜索框内置 + 键盘导航 + 滚动条）；行内容 / 眼睛 / 右键菜单 /
        // 双击定位逻辑全在 [`crate::column_browser::ColumnBrowserDelegate`]（render 期
        // 只读实体挂元素，List 内部 prepare 时由 delegate 现算过滤结果）
        let browser = self.browser_list.clone();
        // 【2026-10-28 微调】表头行：与 List 行（列名 flex_1 / 类型 flex_none / 眼睛操作列）
        // 同 padding（px_3/py_1/gap_x_1 = kit ListItem 行内边距）对齐，列浏览器呈现
        // "表格 + 内嵌操作行"形态（行渲染仍在 delegate，眼睛即行内操作）。
        let table_head = div()
            .h_flex()
            .items_center()
            .gap_x_1()
            .px_3()
            .py_1()
            .border_b(ui::HAIRLINE)
            .text_xs()
            .text_color(muted)
            .child(div().flex_1().min_w_0().child("列名"))
            .child(div().flex_none().child("类型"))
            .child(div().flex_none().child(""))
            .into_any_element();
        let list_body: AnyElement = List::new(&browser)
            .with_size(Size::Small)
            .search_placeholder("搜索列名 / 类型 / 注释…")
            .scrollbar_visible(true)
            .flex_1()
            .min_h_0()
            .into_any_element();


        let detail_body: AnyElement = match detail {
            Some((title, items)) => div()
                .v_flex()
                .h_full()
                .debug_selector(|| "editor-result-col-browser-detail".to_string())
                .child(
                    div()
                        .px_2()
                        .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                        .h_flex()
                        .items_center()
                        .text_xs()
                        .text_color(muted)
                        .text_ellipsis()
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .children(items.into_iter().map(|(k, v)| kv_row(&k, &v, cx))),
                )
                .into_any_element(),
            None => self.sidebar_empty("暂无列", cx),
        };

        // 页签内容区：宽度 / 边框 / 背景由右栏外壳统一控制，这里只排上下三段
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .v_flex()
            .min_h_0()
            .debug_selector(|| "editor-result-browser-tab".to_string())
            .child(
                div()
                    .px_2()
                    .py_1()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!("列浏览器 · {} 列", columns.len()))),
            )
            .child(table_head)
            .child(list_body)
            .child(
                div()
                    .border_t(ui::HAIRLINE)
                    .h(rems(ui::RESULT_BROWSER_DETAIL_HEIGHT))
                    .flex_none()
                    .v_flex()
                    .min_h_0()
                    .child(detail_body),
            )
            .into_any_element()
    }

    /// 【侧栏】右栏：对齐 DBeaver Data Editor 右侧辅助区，四页签——
    /// 列浏览器（metadata）/ 值详情（数值查看器）/ 计算（列统计）/ 分组（顺序对齐 DBeaver）。
    ///
    /// 页签用 kit `TabBar`（不手搓）；切页走队列交宿主 `set_right_tab`。
    /// 「引用」页留 backlog（见 result-panel.md §8.12）。计算 / 分组是纯函数
    /// （`column_stats` / `group_by_count`，不引 polars——见 result-panel.md §8.8）；
    /// 每次渲染按当前可见行现算（中小结果集可接受，大表本地聚合的缓存 / 下发
    /// GROUP BY 是后续项，记在文档）。
    pub(crate) fn render_right_sidebar(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let show = self.show_value_sidebar;
        // 【2026-10-28 微调】横 tab 栏**只显示当前点击的页签**（其他页签不显示）：
        // 页签切换靠右缘竖条图标（render_right_edge_bar），横条只当「当前页签标题 +
        // 关闭钮」使用（DBeaver 侧栏收起后只留边缘图标，展开时标题区极简）。
        let active_tab = match self.right_tab {
            RightTab::Browser => (IconName::Table2, "列浏览器"),
            RightTab::Value => (IconName::Eye, "值详情"),
            RightTab::Stats => (IconName::Sigma, "计算"),
            RightTab::Group => (IconName::Funnel, "分组"),
            RightTab::Reference => (IconName::Link, "引用"),
            RightTab::ExecSql => (IconName::Braces, "执行SQL"),
        };
        let children: Vec<Tab> = std::iter::once(active_tab)
            .map(|(icon, label)| {
                Tab::new()
                    .icon(icon)
                    .label(SharedString::from(label))
            })
            .collect();
        let tab_bar = TabBar::new("editor-result-right-tabs")
            .segmented()
            .with_size(Size::Small)
            .selected_index(0)
            .children(children)
            // 单 tab 只作标题展示；页签切换在右缘竖条（render_right_edge_bar）
            .on_click(move |_index, _window, _app| {});
        let content = match self.right_tab {
            RightTab::Browser => self.render_browser_tab(cx),
            RightTab::Value => self.render_value_tab(cx),
            RightTab::Stats => self.render_stats_tab(cx),
            RightTab::Group => self.render_group_tab(cx),
            RightTab::Reference => self.render_reference_tab(cx),
            RightTab::ExecSql => self.render_exec_sql_tab(cx),
        };
        // 【页签关闭】非默认「列浏览器」页时，行尾给一个 ×：退回默认页（不收起右栏——
        // 拍板口径：× 只退页，右栏开合仍由工具栏「列浏览器」开关控制）。
        let close = (self.right_tab != RightTab::Browser).then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-right-tab-close")
                .ghost()
                .small()
                .icon(IconName::X)
                .debug_selector(|| "editor-result-right-tab-close".to_string())
                .tooltip("关闭页签，返回列浏览器")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::SetRightTab(RightTab::Browser), app);
                })
                .into_any_element()
        });
        // 【侧栏丝滑】右栏收起 / 展开与左栏同法：宽度过渡 + 容器始终挂载（宽 0 不占
        // 空间），页签条与内容按 show 挂载（收起时无焦点残留）。开合由
        // `right_sidebar_epoch` 递增重播动画；首帧（epoch=0）免动画。
        let full = px(ui::RESULT_SIDEBAR_RIGHT_WIDTH * 16.0);
        let (from, to) = sidebar_width_anim(self.right_sidebar_epoch, show, full);
        let wrapper = div()
            .h_full()
            .flex_none()
            .overflow_hidden()
            .border_l(ui::HAIRLINE)
            .bg(theme.colors.sidebar)
            .debug_selector(|| "editor-result-value-sidebar".to_string())
            .when(show, |el| {
                el.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                        .flex_1()
                        .min_w_0()
                        .child(div().flex_1().min_w_0().child(tab_bar))
                        .children(close),
                )
                .child(content)
            });
        EffectTransition::new(Duration::from_millis(160))
            .ease(ease_in_out_cubic)
            .width(from, to)
            .apply(
                wrapper,
                SharedString::from(format!(
                    "editor-result-right-bar-w-{}",
                    self.right_sidebar_epoch
                )),
            )
            .into_any_element()
    }

    /// 【2026-10-28 微调】右缘竖条：页签快捷图标区（列浏览器 / 值详情 / 计算 / 分组 /
    /// 引用 / 执行SQL），点击切页签（右栏收起时自动展开，见 [`Self::set_right_tab`]），
    /// 当前页签高亮。开合开关**已移到状态栏右端**（`right_switch` 三态：收起 / 窄展开 /
    /// 宽展开），竖条顶部不再挂开关。
    /// - 常驻固定宽度（与左缘竖条一致 `RESULT_VERTICAL_BAR_WIDTH`）；可见性由
    ///   `edge_bar_visible` 决定（窄展开 = 只显本竖条；宽展开 = 竖条 + 右栏内容）。
    pub(crate) fn render_right_edge_bar(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        // 页签快捷图标：与右栏页签同一组图标（见 render_right_sidebar）
        let tabs = [
            (RightTab::Browser, IconName::Table2, "列浏览器"),
            (RightTab::Value, IconName::Eye, "值详情"),
            (RightTab::Stats, IconName::Sigma, "计算"),
            (RightTab::Group, IconName::Funnel, "分组"),
            (RightTab::Reference, IconName::Link, "引用"),
            (RightTab::ExecSql, IconName::Braces, "执行SQL"),
        ];
        let queue = self.queue.clone();
        let active = self.right_tab;
        div()
            .h_full()
            .flex_none()
            .w(px(ui::RESULT_VERTICAL_BAR_WIDTH * 16.0))
            .overflow_hidden()
            .border_l(ui::HAIRLINE)
            .bg(theme.colors.sidebar)
            .debug_selector(|| "editor-result-right-edge-bar".to_string())
            .v_flex()
            .items_center()
            .gap_1()
            .pt_2()
            .children(tabs.into_iter().map(|(tab, icon, label)| {
                let queue = queue.clone();
                Button::new(format!("editor-result-edge-{:?}", tab))
                    .ghost()
                    .small()
                    .debug_selector(|| format!("editor-result-edge-{:?}", tab))
                    .toggled(active == tab)
                    .icon(icon)
                    .tooltip(label)
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::SetRightTab(tab), app);
                    })
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// 【1.2】顶宽筛选条：点击筛选组展开按钮后，工具栏下方全宽输入框
    /// （suffix ▶ 运行）+ ✕ 清除 + 收起按钮 + 生成的 wrapped SQL 预览（词非空时）。
    pub(crate) fn render_filter_expand_bar(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let run_queue = self.queue.clone();
        let clear_queue = self.queue.clone();
        let collapse_queue = self.queue.clone();
        let collapse_queue2 = self.queue.clone();
        // wrapped SQL 预览：词非空且有原 SQL 时拼 `select * from (原SQL) where 词`
        let preview = (!self.filter_text.trim().is_empty() && self.result_sql.is_some()).then(|| {
            let sql = self.result_sql.clone().unwrap_or_default();
            let word = self.filter_text.trim();
            let wrapped = format!("select * from ({}) as _q where {}", sql, word);
            let preview_text: SharedString = if wrapped.len() > 80 {
                format!("{}…", &wrapped[..80]).into()
            } else {
                wrapped.into()
            };
            div()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(preview_text)
        });
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .flex_none()
            .min_h_0()
            .border_b(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .bg(theme.colors.background)
            .px_2()
            .py_1()
            .text_xs()
            .debug_selector(|| "editor-result-filter-expand-bar".to_string())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        // 【2026-10-28 微调（纠正）】输入框内两个内嵌组件：
                        // **最左侧 = 展开**（prefix：Maximize2，点击收起筛选条，标识本框为
                        // 展开态）；**最右侧 = 运行**（suffix：▶，wrapped 重查）；
                        // 输入框无边框（bordered(false)，与工具栏融为搜索条风格）。
                        Input::new(&self.filter_input)
                            .bordered(false)
                            .prefix(
                                Button::new("editor-result-filter-expand-toggle")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Maximize2)
                                    .debug_selector(|| "editor-result-filter-expand-toggle".to_string())
                                    .tooltip("收起筛选条（回到工具栏输入框）")
                                    .on_click(move |_, _window, app| {
                                        collapse_queue.request(ResultRequest::ToggleFilterExpand, app);
                                    }),
                            )
                            .suffix(
                                Button::new("editor-result-filter-expand-run")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Play)
                                    .debug_selector(|| "editor-result-filter-expand-run".to_string())
                                    .tooltip("用 select * from (原SQL) where 词 重查（替换当前结果集）")
                                    .on_click(move |_, _window, app| {
                                        run_queue.request(ResultRequest::RunFilterAsQuery, app);
                                    }),
                            ),
                    ),
            )
            .when(!self.filter_text.trim().is_empty(), |row| {
                row.child(
                    Button::new("editor-result-filter-expand-clear")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-filter-expand-clear".to_string())
                        .label("✕ 清除")
                        .on_click(move |_, _window, app| {
                            clear_queue.request(ResultRequest::ClearFilter, app);
                        }),
                )
            })
            .children(preview)
            .child(
                Button::new("editor-result-filter-expand-collapse")
                    .ghost()
                    .small()
                    .icon(IconName::Minimize)
                    .debug_selector(|| "editor-result-filter-expand-collapse".to_string())
                    .label("收起")
                    .tooltip("回到工具栏输入框".to_string())
                    .on_click(move |_, _window, app| {
                        collapse_queue2.request(ResultRequest::ToggleFilterExpand, app);
                    }),
            )
            .into_any_element()
    }


}



/// 【侧栏丝滑】展开 / 收起宽度动画的 `(from, to)` 计算（纯函数，render 调用）：
/// - `epoch == 0`（首帧）：`from == to`，无动画直接落位（面板初始展开，不能开场就来一遍滑入）
/// - `epoch > 0` 且展开：`0 → 全宽`（内容随宽度露出）
/// - `epoch > 0` 且收起：`全宽 → 0`（宽度平滑收缩）
/// 每次开合 `toggle_*_sidebar` 递增 epoch，动画 id 拼 epoch → `with_animation` 换 id 重播。
pub(crate) fn sidebar_width_anim(epoch: u64, show: bool, full: Pixels) -> (Pixels, Pixels) {
    let to = if show { full } else { px(0.) };
    let from = if epoch == 0 {
        to
    } else if show {
        px(0.)
    } else {
        full
    };
    (from, to)
}

/// 【2026-10-28 微调】右侧开关三态转移（纯函数，`toggle_value_sidebar` 调用）：
/// 收起 `(false,false)` → 窄展开 `(true,false)` → 宽展开 `(true,true)` → 收起。
/// 返回值 = `(edge_bar_visible, show_value_sidebar)`；不变量：宽展开 ⟹ 竖条可见。
pub(crate) fn next_right_edge_state(edge_bar_visible: bool, show_value_sidebar: bool) -> (bool, bool) {
    match (edge_bar_visible, show_value_sidebar) {
        (false, _) => (true, false),    // 收起 → 窄展开（只显右缘竖条图标条）
        (true, false) => (true, true),  // 窄展开 → 宽展开（右栏内容出现）
        (true, true) => (false, false), // 宽展开 → 收起（竖条与右栏一起隐去）
    }
}