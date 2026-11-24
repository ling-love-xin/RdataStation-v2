//! 渲染域（`panel/` 第二块）：主 `render` 组装（投影状态 → 元素树根）。

use shared::i18n::tr;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::Selectable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::separator::Separator;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）
use ::shared::CurrentTier;

use super::data::ResultPanel;
use super::events::{
    count_rows_available, count_rows_tooltip, erase_menu_labels, state_label,
};
use crate::error_card::{self};
use crate::export;
use crate::grid::{self as result_grid, ResultToolbar, RowTargetState};
use crate::ports::{FindOption, PageTarget, Presentation, ResultRequest};
use crate::script_card::{self};
use crate::sets::{self as result_sets};
use crate::ui;

impl ResultPanel {
    pub fn render(&self, cx: &App) -> AnyElement {
        // 脚本卡 / 错误卡 / 无结论分支与 SQL 分支的判定：有卡片就不画网格。
        // 【2b】脚本输出卡片（通用模式）与错误卡片同属「替掉网格」的那类卡；
        // 两者都不会与正常网格并存（互斥由编辑器投影保证）。
        let card = self
            .error_card
            .clone()
            .map(|card| {
                let queue = self.queue.clone();
                let locate = card.location.as_ref().map(|location| {
                    let label = error_card::locate_label(location);
                    let queue = queue.clone();
                    Button::new("editor-result-error-locate")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-error-locate".to_string())
                        .label(label)
                        .on_click(move |_, _window, app| {
                            queue.request(ResultRequest::LocateError, app);
                        })
                        .into_any_element()
                });
                let queue = self.queue.clone();
                let copy = Button::new("editor-result-error-copy")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-error-copy".to_string())
                    .label(tr("复制"))
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::CopyError, app);
                    })
                    .into_any_element();
                error_card::render(
                    &card,
                    error_card::ErrorCardControls {
                        locate,
                        copy: Some(copy),
                    },
                    cx,
                )
                .into_any_element()
            })
            .or_else(|| {
                self.script_card
                    .clone()
                    .map(|script| script_card::render(&script, cx).into_any_element())
            });

        let mut toolbar = self.toolbar.clone().unwrap_or(ResultToolbar {
            elapsed_ms: None,
            ..Default::default()
        });
        // 【DBeaver 对齐】工具栏最左侧显示 SQL 预览（tooltip 看全文）
        toolbar.sql = self.result_sql.clone();
        // 【原型对齐】结果集标签条始终显示（即使只有 1 个结果集）：
        // 单个结果集时也显示名称（用户可重命名），对齐 DBeaver 口径。
        let tabs = (!self.tabs.is_empty()).then(|| {
            let queue_pick = self.queue.clone();
            let queue_rename = self.queue.clone();
            let queue_pin = self.queue.clone();
            let queue_close = self.queue.clone();
            let queue_move = self.queue.clone();
            let queue_compare = self.queue.clone();
            let grid_analyze = self.grid.clone();
            let actions = result_sets::TabActions {
                // 「⋯」按钮：进入重命名态（事件路径；request 立即通知面板 → 宿主 drain）
                on_rename: Some(std::rc::Rc::new(move |index, _w, app| {
                    queue_rename.request(ResultRequest::BeginRename(index), app);
                })),
                on_toggle_pin: Some(std::rc::Rc::new(move |index, app| {
                    queue_pin.request(ResultRequest::TogglePin(index), app);
                })),
                // 【DBeaver 对齐】标签上的关闭按钮：移除该结果集（宿主 drain 后调 store）
                on_close: Some(std::rc::Rc::new(move |index, app| {
                    queue_close.request(ResultRequest::CloseSet(index), app);
                })),
                // 【DBeaver 对齐】拖拽重排：把拖起的第 from 份搬到第 to 位
                on_reorder: Some(std::rc::Rc::new(move |from, to, _w, app| {
                    queue_move.request(ResultRequest::MoveSet { from, to }, app);
                })),
                // 【T8 标签右键】分析：取委托里的 hook 执行（与网格右键同一入口，进 08 对话框）
                on_analyze: Some(std::rc::Rc::new(move |_index, _w, app| {
                    let hook = grid_analyze.read(app).delegate().on_analyze_result_set.clone();
                    if let Some(hook) = hook {
                        hook(app);
                    }
                })),
                // 【T8 标签右键】并排对比：把点中的那份设为对比对象
                on_compare: Some(std::rc::Rc::new(move |index, app| {
                    queue_compare.request(ResultRequest::SelectCompare(index), app);
                })),
            };
            result_sets::render(
                &self.tabs,
                self.active,
                move |index, _window, app| {
                    let index = *index;
                    queue_pick.request(ResultRequest::SelectSet(index), app);
                },
                actions,
                cx,
                &self.tabs_scroll,
            )
            .into_any_element()
        });

        // 【C1】重命名态：tab 条下方插入一个输入框条（输入框 + ✓ 提交 + ✕ 取消）
        // 【2026-10-28 微调】输入框聚焦时 Enter 提交 / Esc 取消（§8.14 已知遗留补齐）。
        // kit Input 不消费 Enter/Esc（input.rs 无 key_down 挂载），事件沿 Bubble 冒泡到
        // 本容器（div.on_key_down 在 Bubble 阶段触发）；走队列端口（CommitRename /
        // CancelRename 变体已存在），与 ✓/✕ 按钮同口径。
        let tabs = match (tabs, self.renaming) {
            (Some(tabs_el), Some(_index)) => {
                let input = self.renaming_input.clone();
                let queue_confirm = self.queue.clone();
                let queue_cancel = self.queue.clone();
                let queue_enter = self.queue.clone();
                let queue_esc = self.queue.clone();
                Some(
                    div()
                        .v_flex()
                        .child(tabs_el)
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .px_2()
                                .py_1()
                                .debug_selector(|| "editor-result-rename-bar".to_string())
                                .on_key_down(move |event, _window, app| {
                                    if event.keystroke.key == "enter" {
                                        queue_enter.request(ResultRequest::CommitRename, app);
                                    } else if event.keystroke.key == "escape" {
                                        queue_esc.request(ResultRequest::CancelRename, app);
                                    }
                                })
                                .child(
                                    div()
                                        .w(rems(ui::RESULT_FILTER_WIDTH))
                                        .children(input.as_ref().map(|i| Input::new(i))),
                                )
                                .child(
                                    Button::new("editor-result-rename-confirm")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Check)
                                        .on_click(move |_e, _w, app| {
                                            queue_confirm.request(ResultRequest::CommitRename, app);
                                        }),
                                )
                                .child(
                                    Button::new("editor-result-rename-cancel")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Close)
                                        .on_click(move |_e, _w, app| {
                                            queue_cancel.request(ResultRequest::CancelRename, app);
                                        }),
                                ),
                        )
                        .into_any_element(),
                )
            }
            (tabs, _) => tabs,
        };

        // 【呈现】文本视图（DBeaver「显示方式 → 文本」）：主区换成 TSV 纯文本
        // （可选中复制）；标签条保留（可切结果集）；侧栏让位（无选中格语义——
        // 文本没有「格」）。不提前 return：文本视图只是**第三行主体**的一种内容，
        // 四行骨架（tabs / 工具栏 / 主体 / 状态栏）统一在下方组装。

        // 【B15】本地筛选框：只作用于视图层，不重查数据库；输入走 300ms 防抖
        // （`on_filter_input`），有词时给一个 ✕ 清除；「下发源库」开关开着时应用
        // 筛选会重查（入队，结果落新结果集）。
        // 【1.3 对齐】下发源库 / 注释两个开关从筛选组拆出，交给工具栏右段
        // Toolbar 统一容器（与对比 / 分析 / 复制同基线），筛选组只留输入与清除。
        // 【DBeaver 对齐 2026-11-08】两个开关改**图标 toggle**（按下态 = 开）：
        // DBeaver 那条筛选条用的是 CHECK `ToolItem`，界面上没有任何带文字标签的
        // 开关/按钮；等价改造见下方 `pushdown` / `comments` 的构造注释。
        // 【1.2】筛选框升级：常态 180px 输入框内嵌 ▶ 运行（RunFilterAsQuery，
        // 用 `select * from (原SQL) where 词` 替换当前结果集重查）；展开按钮
        // （Maximize2）打开顶宽筛选条（filter_expanded，输入框移到工具栏下方全宽）。
        // 展开态下工具栏筛选组不再渲染输入框（同一 InputState 实体一帧只能渲染
        // 一处，双渲染会元素 id 冲突），只留展开按钮（toggled 高亮指示展开态）。
        // 【DBeaver 对齐 2026-11-08】筛选区外框（`CompositeBorderPainter` 等价物）：
        // DBeaver 的 `filterComposite` 挂 `new CompositeBorderPainter(...)`
        // （`ResultSetFilterPanel.java:182`），`CompositeBorderPainter.paintControl` 沿外框
        // `gc.drawRectangle(0, 0, bounds.width - 1, bounds.height - 1)` 画**一整圈 1px**
        // （`CompositeBorderPainter.java:34-39`）。它圈住的是「活动对象 / 展开 / 筛选文本 /
        // 执行 / 历史」五个子控件，也就是**整个筛选区是一组**。本仓对应结构是
        // 「筛选输入 + 运行（suffix）+ 展开 + 清除」，所以由这个容器自己画边框。
        //
        // 底色：DBeaver 文本框底是列表底（`UIStyles.getDefaultTextBackground()` →
        // `COLOR_LIST_BACKGROUND`，`UIStyles.java:131-133`），而外层面板是 widget 灰底；
        // 本仓即 `theme.colors.background`（面板白）压在工具栏的 `tab_bar` 灰底上。
        //
        // 高度对齐 DBeaver：`Input.xsmall()` 20px（`Size::input_h` 的 `h_5`）+
        // 上下 1px 边框 + 上下各 1px 内距 = 24px，落在 30px 工具栏里上下各 3px。
        // DBeaver 那边是 `filterComposite marginHeight 2 + MIN_FILTER_TEXT_HEIGHT 20
        // + marginHeight 2` = 24px，外层 `marginHeight 3`（`ResultSetFilterPanel.java:103/164/175`）
        // → 总高 30px，与本仓逐段相等。
        //
        // 宽度 `flex_1`：DBeaver 左列是 `GridData(FILL_HORIZONTAL)`（`:180`），右列 ToolBar
        // 才是内容宽；此前固定 11.25rem(180px) 撑不满一行，看着"没对齐"。
        //
        // 【DBeaver 对齐 2026-11-08 二次】**不加圆角**：`CompositeBorderPainter.paintControl`
        // 用 `gc.drawRectangle(0, 0, w - 1, h - 1)` 画的是**直角矩形**
        // （`CompositeBorderPainter.java:34-39`），SWT 的 composite 本身也没有圆角。
        // 此前挂 `rounded(theme.radius)` 会把四角磨圆，框形与 DBeaver 不一致。
        let theme = cx.theme();
        // 【DBeaver 对齐 2026-11-17】筛选框第 1 格 = 对象图标 +「显示 SQL」固定文案。
        //
        // DBeaver 的 `filterComposite` 是 5 列 GridLayout（`ResultSetFilterPanel.java:174`），
        // 第 1 列 `ActiveObjectPanel`（`:184`）画的不是 SQL 原文：`getActiveSourceQuery(true)`
        // 在「数据容器是表且当前没有筛选条件」时返回固定文案
        // `sql_editor_resultset_filter_panel_show_sql_label`（`:577-578`，
        // zh 资源 `ResultSetMessages_zh.properties:746` = 「显示 SQL」），
        // 绘制时先画对象图标再画这段文字（`ActiveObjectPanel.paint:1043-1049`），
        // 文本宽度按 `maxTextLength = compactMode ? 16 : 24` 个字符封顶（`:996`）。
        // 完整 SQL 只在点击后的弹层里出现（`ActiveObjectPanel` 双击开 popup，`:924`）。
        //
        // 本仓此前把「SQL: 前 60 字符」摆成工具栏最左的独立按钮，长度随 SQL 变化，
        // 会把筛选输入压窄；改成固定文案后宽度恒定（与 DBeaver 一致），
        // 原文只走 `editor-result-sql-dialog`。点击仍用单击（用户口径：点击就弹窗）。
        let sql_chip: Option<AnyElement> = toolbar.sql.is_some().then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-sql-preview")
                .ghost()
                // 20px 档：与筛选框内其它子控件同高（框内高 20、外圈 24，见下方注释）
                .xsmall()
                .debug_selector(|| "editor-result-sql-preview".to_string())
                .icon(IconName::Table2)
                .label(tr("显示 SQL"))
                .tooltip(tr("点击查看完整 SQL（弹出窗口）"))
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::OpenSqlDialog, app);
                })
                .into_any_element()
        });
        let filter = div()
            .h_flex()
            .items_center()
            // 列间无间距：DBeaver `filterComposite` 是 `GridLayout(compactMode ? 4 : 5, false)`
            // + `horizontalSpacing = 0` + `verticalSpacing = 0`
            // （`ResultSetFilterPanel.java:174-179`），列与列紧贴，只靠控件自身宽度分离。
            // 此前 `gap_1`(4px) 多出四条 4px 缝，比 DBeaver 松一档。
            .flex_1()
            .min_w_0()
            .bg(theme.colors.background)
            .border_1()
            .border_color(theme.colors.input)
            // 内距 1px：DBeaver 的 `filterComposite` 有 `marginHeight/marginWidth = 2`
            // （`ResultSetFilterPanel.java:175-176`），它那圈框线是 `paintControl` 画的、
            // 不占布局，所以「框到内容」的留白只有那 2px。本仓边框是布局里的 1px，
            // 再补 1px 内距才等价于它的 2px，总量 24px 才对得上。
            .p(ui::HAIRLINE)
            .debug_selector(|| "editor-result-filter".to_string())
            // 第 1 列：对象图标 +「显示 SQL」（`ActiveObjectPanel`，见上文注释）
            .children(sql_chip)
            // 第 2 列：展开切换，放在文本列**左侧**。DBeaver 的 `FilterExpandPanel` 正是
            // `GridLayout(5)` 的第 2 列（`ResultSetFilterPanel.java:186`），`GridData` 为
            // `HORIZONTAL_ALIGN_BEGINNING | FILL_VERTICAL` + `widthHint = 图标宽 + 4`
            // （`:1303-1306`）；它非 hover 时画灰度图、hover 才画实图（`:1309-1317`）。
            // 本仓用 `Button::ghost()` 的常规 hover 底表达同一语义（「平时不抢眼，
            // 指过去才亮」）；灰度/实图双份资源要自定义绘制入口，P1 不做。
            //
            // 这一列不随 `filter_expanded` 分支：展开态下工具栏只剩它（点它收起），
            // 与 DBeaver 展开后工具条只剩图标钮同理。
            .child({
                let expand_queue = self.queue.clone();
                Button::new("editor-result-filter-expand")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Maximize2)
                    // `selected` 出按下态、`toggled` 只写 a11y（0.9.0 实测，
                    // 见 gpui-component `button.rs:513-571`）：DBeaver 这颗是 CHECK，
                    // 展开态必须看得见按下去
                    .selected(self.filter_expanded)
                    .toggled(self.filter_expanded)
                    .debug_selector(|| "editor-result-filter-expand".to_string())
                    .tooltip(if self.filter_expanded {
                        "收起筛选（回到工具栏输入框）"
                    } else {
                        "展开筛选（顶宽输入框）"
                    })
                    .on_click(move |_, _window, app| {
                        expand_queue.request(ResultRequest::ToggleFilterExpand, app);
                    })
            })
            .when(!self.filter_expanded, |row| {
                let run_queue = self.queue.clone();
                row
                    .child(
                        div().flex_1().min_w_0().child(
                            // 【2026-11-08 高度对齐】Input 默认 Medium 档 = 32px，在 40px 工具栏里
                            // 上下只剩 4px，且与同排 `Toolbar` XSmall 的 24px 按钮差 8px 高。
                            // 降到 `small`（24px）后一行控件同档；字号随之 12px → 14px（更好读）。
                            //
                            // 【DBeaver 对齐 2026-11-08 三次】档位 Small → XSmall：
                            // DBeaver 筛选文本控件被 `MIN_FILTER_TEXT_HEIGHT = 20` 钉在 20px
                            // （`ResultSetFilterPanel.java:103`），本仓 Small 档是 24px，
                            // 高出一档；XSmall 的 `input_h` 正是 `h_5` = 20px，
                            // 与 DBeaver 逐像素相等。
                            //
                            // 【DBeaver 对齐】`appearance(false)` 让出 bg / rounded / 焦点环：
                            // 那圈边框现在由外层容器画（DBeaver 内部文本框本身无边框），
                            // 否则两层描边会叠成双框。焦点靠 caret 指示，与 DBeaver 的
                            // StyledText 一致（它那圈框也不随 focus 变色）。
                            //
                            // 【DBeaver 对齐 2026-11-08 四次】执行钮从 `.suffix` 拆成独立一列：
                            // DBeaver 的 `ExecutePanel` 是文本列**右侧的独立列**
                            // （`GridLayout(5)` 第 4 列，`ResultSetFilterPanel.java:174/209`），
                            // 不在文本框内部；且 `Textarea`（P3 要换的多行框）**没有 suffix 槽**，
                            // 提前改成一列，P3 时才不必再搬一次。
                            // 【P3】换成多行控件 + 固定 1 行高。DBeaver 的 `filtersTextViewer`
                            // 是 `TextViewer(SWT.MULTI)`（`ResultSetFilterPanel.java:192`），
                            // 常态被 `gd.heightHint = getLineHeight() * 1` 钉在 1 行
                            // （`togglePanelExpand`，`:1320-1327`）。
                            // 本仓 `Textarea` 的行高是常量 `LINE_HEIGHT = Rems(1.25)` = 20px
                            // （`input.rs:543`），所以 `.h(rems(1.25))` 就是 1 行、且与
                            // `MIN_FILTER_TEXT_HEIGHT = 20` 逐像素相等；容器仍 24px。
                            Textarea::new(&self.filter_input)
                                .xsmall()
                                .appearance(false)
                                .h(rems(1.25)),
                        ),
                    )
                    .child(
                        // 第 4 列：执行。DBeaver 同列是 `ExecutePanel`（`ToolItemPanel` 子类，
                        // `heightHint = MIN_FILTER_TEXT_HEIGHT`，`:1351`），紧贴文本列右侧。
                        Button::new("editor-result-filter-run")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Play)
                            .debug_selector(|| "editor-result-filter-run".to_string())
                            .tooltip(tr("用 select * from (原SQL) where 词 重查（替换当前结果集）"))
                            .on_click(move |_, _window, app| {
                                run_queue.request(ResultRequest::RunFilterAsQuery, app);
                            }),
                    )
            })
            // 【筛选历史】DBeaver `filterComposite` 的第 5 列是 `HistoryPanel`
            // （`ResultSetFilterPanel.java:174/214`）：一个下拉箭头，点开是历史筛选表
            // `createFilterHistoryPanel`（`:1207`），选中一条即回到那个口径。
            // 列序上它紧跟「执行」列（`ExecutePanel`，`:209`），**在 `filterToolbar`
            // 之前**——也就是先于擦除 / 筛选设置 / 后退 / 前进那一排 ToolItem。
            // 2026-11-21 从「筛选设置之后」移回这里（此前顺序与 DBeaver 相反）。
            // 空历史不摆入口（DBeaver 列常在，但点了只有一张空表；本仓口径是「能力没有就不摆」）。
            .when(!self.filter_expanded && !self.filter_history.is_empty(), |row| {
                let items: Vec<String> = self.filter_history.to_vec();
                let queue = self.queue.clone();
                row.child(
                    Button::new("editor-result-filter-history")
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronDown)
                        .debug_selector(|| "editor-result-filter-history".to_string())
                        .tooltip(tr("历史筛选（选中一条即回到那个口径）"))
                        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                            let mut menu = menu;
                            for text in items.iter() {
                                let label = text.clone();
                                let text = text.clone();
                                let queue = queue.clone();
                                menu = menu.item(PopupMenuItem::new(label).on_click(
                                    move |_, _window, app| {
                                        queue.request(ResultRequest::ApplyFilter(text.clone()), app);
                                    },
                                ));
                            }
                            menu
                        }),
                )
            })
            // 【2026-11-18 结构对齐】清除钮改成 DBeaver 的口径：**常驻 + 空词禁用**
            // （`filtersClearButton` 建出来就 `setEnabled(false)`，`:319-327`；有筛选后才
            // 由 `enablePanelControls` 打开，`:449-454`）。此前是「有词才出现」，位置会
            // 随打字左右跳一格，而 DBeaver 那一列是固定的。
            //
            // 形态：DBeaver 那颗是 `SWT.DROP_DOWN`——本体清筛选词，右侧箭头弹
            // `EraseItemListener` 菜单（`ResultSetViewer.java:1525-1545`：清筛选设置 /
            // 显示隐藏列 / 重置颜色 / transformers / pins / 全部重置）。本仓按「能力没有
            // 就不摆」只留两条真有落点的：「删除所有过滤 / 排序」、显示全部列
            // （见 `erase_menu_labels`）；条目恒非空，箭头常在、宽度不跳。
            //
            // 主点击 = 「删除所有过滤 / 排序」（DBeaver 的 `executeDefaultAction:1549-1551`
            // 就是 `clearDataFilter(true)`），tooltip 同该命令说明
            // （`ResultSetFilterPanel.java:319-325`）。**恒可用**：DBeaver 的
            // `enablePanelControls` 把它无条件 `setEnabled(true)`（`:449-454`，
            // 旧条件 `hasFilters() || hasOrdering() || 词非空` 已被注释掉）。
            .when(!self.filter_expanded, |row| {
                let clear_queue = self.queue.clone();
                let clear = Button::new("editor-result-filter-clear")
                    .ghost()
                    .xsmall()
                    .debug_selector(|| "editor-result-filter-clear".to_string())
                    .label("✕")
                    .tooltip(tr("删除所有过滤 / 排序"))
                    .on_click(move |_, _window, app| {
                        clear_queue.request(ResultRequest::ClearFilterAndOrdering, app);
                    });
                let labels = erase_menu_labels(&self.filter_text, self.column_hidden.len());
                let menu_queue = self.queue.clone();
                row.child(
                    DropdownButton::new("editor-result-filter-erase")
                        .button(clear)
                        .menu_accessibility_label("清除选项")
                        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                            let mut menu = menu;
                            if labels.is_empty() {
                                return menu.item(PopupMenuItem::new(tr("暂无可清除项")).disabled(true));
                            }
                            for label in labels.iter() {
                                let label = *label;
                                let queue = menu_queue.clone();
                                menu = menu.item(PopupMenuItem::new(label).on_click(
                                    move |_, _window, app| {
                                        // 条目与请求一一对应（清单由 `erase_menu_labels` 定）
                                        let request = match label {
                                            "删除所有过滤 / 排序" => {
                                                ResultRequest::ClearFilterAndOrdering
                                            }
                                            _ => ResultRequest::ShowAllColumns,
                                        };
                                        queue.request(request, app);
                                    },
                                ));
                            }
                            menu
                        }),
                )
            })
            // 【筛选设置】DBeaver 筛选条右列第一颗 `filtersCustomButton`
            // （`ResultSetFilterPanel.java:330-338`：FILTER 图标、`SWT.DROP_DOWN`，**恒建**
            // ——`compactMode` 下 clear / history 都不建，这一颗仍然在）。主点击开
            // `FilterSettingsDialog`（列约束表 + 自定义 WHERE/ORDER 两页）。
            //
            // 本仓落点：打开「筛选设置」浮层（见 `dialog.rs::render_filter_settings`）。
            // 与 DBeaver 的差异在册：浮层里 WHERE 一栏只读回显（筛选框本体才是它的输入源），
            // 真正新增的能力是**自定义 ORDER BY**（本仓此前只有按列排序下发）。
            //
            // 箭头菜单两项对齐 `CustomFilterListener.fillDropDownMenu`
            // （`ResultSetFilterPanel.java:1561-1571`）：`CMD_FILTER_EDIT_SETTINGS`（= 主点击
            // 那个动作）+ `CMD_FILTER_SAVE_SETTING` + `CMD_FILTER_RESET_SETTING`。
            // DBeaver 后两项在 `dataContainer instanceof DBSEntity` 的门控下；本仓没有
            // 实体这一层，故恒摆（存档自己也允许 `table` 为空，见 `SavedFilterSetting`）。
            .child({
                let settings_queue = self.queue.clone();
                let open = Button::new("editor-result-filter-settings")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Funnel)
                    .debug_selector(|| "editor-result-filter-settings".to_string())
                    .tooltip(tr("筛选设置（自定义条件与排序）"))
                    .on_click(move |_, _window, app| {
                        settings_queue.request(ResultRequest::OpenFilterSettings, app);
                    });
                let menu_queue = self.queue.clone();
                DropdownButton::new("editor-result-filter-settings-menu")
                    .button(open)
                    .menu_accessibility_label("筛选设置选项")
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                        let mut menu = menu;
                        let entries = [
                            ("编辑筛选设置", ResultRequest::OpenFilterSettings),
                            ("保存过滤器设置", ResultRequest::SaveFilterSetting),
                            ("重置过滤器设置", ResultRequest::ResetFilterSetting),
                        ];
                        for (label, request) in entries {
                            let queue = menu_queue.clone();
                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                move |_, _window, app| {
                                    queue.request(request.clone(), app);
                                },
                            ));
                        }
                        menu
                    })
            })
            // 【状态历史】后退 / 前进：对齐 DBeaver 的一对 `SWT.DROP_DOWN` 按钮
            // `historyBackButton` / `historyForwardButton`（`ResultSetFilterPanel.java:343-351`）。
            // 语义（`ResultSetViewer.java`）：历史项 = 一份「需要重查的状态」，后退 / 前进
            // 都按目标状态**重跑数据泵**（`navigateHistory`，`:4050-4063`）；启用态
            // 后退仅 `pos > 0`、前进仅 `pos < size - 1`（`:456-475`），tooltip = 目标状态描述。
            //
            // 形态：`SWT.DROP_DOWN` = **分裂按钮**（主体点击走一格 + 右侧 caret 弹菜单），
            // 对齐用组件的 `DropdownButton`（左半是带图标的主体按钮，右半是 caret，ghost
            // 变体下两半连成一个控件）。菜单内容 = 该方向**全部可达状态**
            // （DBeaver `:1456-1481`），点哪条就跳哪条（`NavigateHistoryTo`）。
            .when(!self.filter_expanded, |row| {
                let step_queue = self.queue.clone();
                let hint = self.history_hint(-1);
                let can_back = self.history_can_back();
                let step = Button::new("editor-result-history-back-step")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronLeft)
                    .debug_selector(|| "editor-result-history-back-step".to_string())
                    .tooltip(hint)
                    .on_click(move |_, _window, app| {
                        step_queue.request(ResultRequest::NavigateHistory(-1), app);
                    });
                let items = self.history_menu_items(-1);
                let menu_queue = self.queue.clone();
                row.child(
                    DropdownButton::new("editor-result-history-back")
                        .button(step)
                        .disabled(!can_back)
                        .menu_accessibility_label("后退历史")
                        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                            let mut menu = menu;
                            for (pos, sql) in items.iter() {
                                let label = state_label(sql);
                                let pos = *pos;
                                let queue = menu_queue.clone();
                                menu = menu.item(PopupMenuItem::new(label).on_click(
                                    move |_, _window, app| {
                                        queue.request(ResultRequest::NavigateHistoryTo(pos), app);
                                    },
                                ));
                            }
                            menu
                        }),
                )
            })
            .when(!self.filter_expanded, |row| {
                let step_queue = self.queue.clone();
                let hint = self.history_hint(1);
                let can_forward = self.history_can_forward();
                let step = Button::new("editor-result-history-forward-step")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronRight)
                    .debug_selector(|| "editor-result-history-forward-step".to_string())
                    .tooltip(hint)
                    .on_click(move |_, _window, app| {
                        step_queue.request(ResultRequest::NavigateHistory(1), app);
                    });
                let items = self.history_menu_items(1);
                let menu_queue = self.queue.clone();
                row.child(
                    DropdownButton::new("editor-result-history-forward")
                        .button(step)
                        .disabled(!can_forward)
                        .menu_accessibility_label("前进历史")
                        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                            let mut menu = menu;
                            for (pos, sql) in items.iter() {
                                let label = state_label(sql);
                                let pos = *pos;
                                let queue = menu_queue.clone();
                                menu = menu.item(PopupMenuItem::new(label).on_click(
                                    move |_, _window, app| {
                                        queue.request(ResultRequest::NavigateHistoryTo(pos), app);
                                    },
                                ));
                            }
                            menu
                        }),
                )
            })
            // 【结构对齐】后退 / 前进组与查找钮之间补一条竖分隔线：
            // DBeaver 在这个位置插 `UIUtils.createToolBarSeparator(filterToolbar, SWT.NONE)`
            // （`ResultSetFilterPanel.java:354`），把「筛选 / 历史」与「查找」分成两组。
            // 用标准 `Separator::vertical()`（gpui-kit-dev skill 的容器层表格禁止手搓分隔线），
            // 宽 1px / 高 24px 与右段 Toolbar 的组间线同档。展开态不摆：那时
            // clear / settings / history / back / forward 全收起，线条两侧没有可分的两组。
            .when(!self.filter_expanded, |row| {
                row.child(
                    // 壳只承载 debug 观测锚点与固定高度：20px 与同排按钮同档
                    // （DBeaver 的 separator 在 ToolBar 内也是按钮高），高于它会把
                    // 筛选区外框从 24px 撑到 28px（已由 `rendered_ui_elements_survive` 钉住）。
                    div()
                        .h(rems(1.25))
                        .flex_none()
                        .debug_selector(|| "editor-result-filter-history-sep".to_string())
                        .child(
                            Separator::vertical()
                                .w(ui::HAIRLINE)
                                .h_full()
                                .into_any_element(),
                        ),
                )
            })
            // 【结果内查找】查找栏开关：DBeaver 筛选条工具条上的 `openFindReplaceButton`
            // 是 **SWT.CHECK**（`ResultSetFilterPanel.java:356`，图标 `UIIcon.FIND_REPLACE`，
            // 命令 `EDIT_FIND_AND_REPLACE`）：按下打开查找浮层、再按关闭（`:361-385`）。
            // 本仓用 `toggled` 表达按下态，浮层换成工具栏下方一行（`render_find_bar`）。
            .child({
                let find_queue = self.queue.clone();
                Button::new("editor-result-find-toggle")
                    .ghost()
                    .xsmall()
                    .selected(self.find_open)
                    .toggled(self.find_open)
                    .icon(IconName::Search)
                    .debug_selector(|| "editor-result-find-toggle".to_string())
                    .tooltip(tr("在结果里查找（回车 = 下一处）"))
                    .on_click(move |_, _window, app| {
                        find_queue.request(ResultRequest::ToggleFind, app);
                    })
            })
            .into_any_element();

        // 【T1 工具栏收口 2026-11-24】「下发源库」删除（基线 §13.4：执行通道非源库时
        // 拼 WHERE 重查没有语义，筛选本身就是在 SQL 上包一层 where）；列注释开关
        // 迁到结果集设置（DBeaver 对应项是「列头附加内容」三档，§14.2）。

        // 【T1 工具栏收口 2026-11-24】并排对比 / 批量提交撤销 / 复制整份结果 三处按钮
        // 全部退出工具栏：对比 → 结果集标签页右键（§12.2、§21.1）；提交撤销 → 状态栏
        // Commit/Reject 段与 ctrl-s（§10.1）；复制 → ctrl-c（§10.1，DBeaver 同键位）。
        // 【T2a-7 / T1 收口】批量提交撤销对已退出工具栏，改由状态栏 Commit/Reject 段承担。
        //
        // 【复制】已退出工具栏，改由 ctrl-c（DBeaver 同键位）。
        let refresh = (self.result_sql.is_some() && !self.result_is_analysis).then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-refresh")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-refresh".to_string())
                .label(tr("⟳ 刷新"))
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::Refresh, app);
                })
                .into_any_element()
        });
        // 【B5b】获取数据：只在这份结果真还有下一段时摆（状态行⑦里）
        let more = self
            .status
            .as_ref()
            .is_some_and(|status| status.has_more)
            .then(|| {
                let queue = self.queue.clone();
                Button::new("editor-result-more")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-more".to_string())
                    .label(tr("获取数据"))
                    .tooltip(tr("获取下一段数据"))
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::FetchMore, app);
                    })
                    .into_any_element()
            });
        // 【DBeaver 对齐】获取全部数据：只在这份结果真还有下一段时摆（一次性抓全量）。
        // 【第 72 批】按钮发 `AskFetchAll` 而不是 `FetchAll`：DBeaver 点下去第一件事是
        // `CONFIRM_RS_FETCH_ALL` 确认（`ResultSetViewer.readAllData:4433-4440`），
        // 确认过才真取。与 `CTRL+SHIFT+=` 走同一条入口。
        let fetch_all = self
            .status
            .as_ref()
            .is_some_and(|status| status.has_more)
            .then(|| {
                let queue = self.queue.clone();
                Button::new("editor-result-fetch-all")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-fetch-all".to_string())
                    .label(tr("获取全部数据"))
                    .tooltip(tr("一次性抓取全部行"))
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::AskFetchAll, app);
                    })
                    .into_any_element()
            });

        // 【分页】状态栏翻页四钮（首页 / 上页 / 下页 / 末页）：**纯视图滚动**——不重查源库，
        // 因此不需要精确总数（「末页」= 已抓到的最后一行，语义是"到底了"而不是
        // "库里的第 N 页"）。四钮与 DBeaver `row.first/previous/next/last` 一一对应，
        // 位置在取下一段（`more` ≈ `fetch.page`）与取全部（`fetch_all` ≈ `fetch.all`）之前
        // （`plugin.xml` 的 `navCmds` 组，1470-1512）。0 行结果集不注入，回退禁用占位。
        let page_btn = |id: &'static str,
                        icon: IconName,
                        tooltip: &'static str,
                        target: PageTarget| {
            let queue = self.queue.clone();
            Button::new(id)
                .ghost()
                .small()
                .icon(icon)
                .tooltip(tooltip)
                .debug_selector(move || id.to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::GoPage(target), app);
                })
                .into_any_element()
        };
        let has_rows = self
            .status
            .as_ref()
            .is_some_and(|status| status.total_rows > 0);
        let (first_page, prev_page, next_page, last_page) = if has_rows {
            (
                Some(page_btn(
                    "editor-result-page-first",
                    IconName::SkipBack,
                    "首页（滚到第 1 行）",
                    PageTarget::First,
                )),
                Some(page_btn(
                    "editor-result-page-prev",
                    IconName::ChevronLeft,
                    "上一页（上移一屏）",
                    PageTarget::Prev,
                )),
                Some(page_btn(
                    "editor-result-page-next",
                    IconName::ChevronRight,
                    "下一页（下移一屏；到底且还有下一段时顺带取数）",
                    PageTarget::Next,
                )),
                Some(page_btn(
                    "editor-result-page-last",
                    IconName::SkipForward,
                    "末页（滚到已抓到的最后一行）",
                    PageTarget::Last,
                )),
            )
        } else {
            (None, None, None, None)
        };

        // 【#28-B3】状态栏行级动作钮：新增 / 复制 / 删除一行。只读结果集（`!editable`）
        // 不注入，回退 grid.rs 的禁用占位钮；复制 / 删除必须有选中行才可用。
        // 【第 69 批】选中行号 / 行删除态 / 可用态统一走 [`ResultPanel::row_target_state`]
        // （键盘入口问同一段，见 §9.52）：三钮与键位不会再各算一套。
        let RowTargetState {
            row: selected_view_row,
            row_deleted,
            actions: row_actions,
        } = self.row_target_state(cx);
        // 【第 57 批·状态栏「修改」钮接线】DBeaver 状态条 `row.edit` 默认可见
        // （`plugin.xml:1386`），handler `CMD_ROW_EDIT` → `openValueEditor(false)`
        // （`ResultSetHandlerMain.java:207-211`，外部值编辑器）。本仓没有「值编辑器 + 写回」
        // 通道，落点沿用本仓既有口径（`grid/status.rs:220` 的占位文案就是
        // 「修改选中格（双击单元格进入编辑）」）——走同族 `CMD_ROW_EDIT_INLINE`
        // （`openValueEditor(true)`）的同一条链路：只发 `BeginEditCell`，
        // 面板 emit `TableEvent::DoubleClickedCell`，与双击完全同路。
        // 只读结果集不给（与行级动作同一门控 `self.editable`）；记录视图给但置灰（见下）。
        // 【第 58 批·记录视图门控】DBeaver 的 `PROP_EDITABLE` 对 `row.edit` / `row.edit.inline`
        // 都要求 `getActivePresentation() instanceof IResultSetEditor` 且当前属性非空
        // （`ResultSetPropertyTester.java:127-131`）；记录模式是 `PlainTextPresentation` 的变体
        // （`PlainTextPresentation.java:63` 只实现 `IResultSetDisplayFormatProvider` / `DBPAdaptable`，
        // **不实现** `IResultSetEditor`）⇒ 那边记录模式下这颗钮是**置灰**，不是「可点但没效果」。
        // 本仓记录视图（转置）本来就只读（`grid/edit.rs:113-120` 的 `begin_edit` 直接拒），
        // 所以跟着置灰，别让点击静默落空。
        //
        // 同一段里 `"inline"` 分支还要 `getCurrentAttribute() != null`（`:132-133`）：没有当前
        // 属性时内嵌编辑关闭。本仓「修改」钮落的就是 inline 链路（第 57 批），所以没有选中格 /
        // 停在行号槽（组件列 0）时同样置灰——`selection_pos` 由 `SelectCell` / `ClearSelection`
        // 事件维护（`lifecycle.rs:365-410`，两条都 `cx.notify()`），灰态跟随选中实时更新。
        let edit_row = self.editable.then(|| {
            let queue = self.queue.clone();
            let transposed = self.transposed;
            // 「修改」钮看的是**当前格**（DBeaver `getCurrentAttribute()`，`PROP_EDITABLE` 的
            // `inline` 分支 `:132-133`），所以「有没有数据格」与「这一行是否已删」都取
            // `selection_pos`——不取 `status.selected_row`（那是行级动作的 `currentRow`，
            // 两件不同的事，见 §9.49）。
            let selected = self.selection_pos;
            let picked = selected.is_some_and(|(_, col)| col > 0);
            let cell_deleted = selected
                .is_some_and(|(row, _)| self.grid.read(cx).delegate().is_deleted_row(row));
            // 【第 62 批】呈现 + 属性这两条走 [`crate::grid::can_edit_cell`]（与右键
            // 「修改单元格」同一式子；`PROP_EDITABLE` 的三条条件，见该函数注释）。
            //
            // 【第 66 批】再叠一条行状态：已标记删除的行上灰掉，与右键 edit 段的
            // 「修改单元格」同一个口径（那边是 `disabled: target.is_deleted`）。DBeaver 的
            // `"edit"` 分支本身不看行状态（`:127-136`），但它有 `CMD_CELL_RESET` 把
            // `STATE_REMOVED` 恢复成 `STATE_NORMAL`（`ResultSetModel.resetCellValue:572-577`）；
            // 本仓没有那条通道，`begin_edit` 直接拒绝已删行，所以只能灰（§9.49）。
            let disabled =
                !crate::grid::can_edit_cell(self.editable, transposed, picked) || cell_deleted;
            Button::new("editor-result-edit-cell")
                .ghost()
                .small()
                .icon(IconName::Pencil)
                .disabled(disabled)
                .tooltip(if transposed {
                    "记录视图是只读的：切回网格视图后可编辑单元格"
                } else if !picked {
                    "先在网格里选中一个数据单元格"
                } else if cell_deleted {
                    "这一行已标记删除；撤销删除后才能修改"
                } else {
                    "修改当前选中单元格（进入行内编辑）"
                })
                .debug_selector(|| "editor-result-edit-cell".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::BeginEditCell, app);
                })
                .into_any_element()
        });
        let insert_row = self.editable.then(|| {
            let queue = self.queue.clone();
            let anchor = selected_view_row;
            Button::new("editor-result-insert-row")
                .ghost()
                .small()
                .icon(IconName::Plus)
                .tooltip(tr("在下方插入新行"))
                .disabled(!row_actions.add)
                .debug_selector(|| "editor-result-insert-row".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::InsertRowBelow(anchor), app);
                })
                .into_any_element()
        });
        let duplicate_row = self.editable.then(|| {
            let queue = self.queue.clone();
            let row = selected_view_row;
            Button::new("editor-result-duplicate-row")
                .ghost()
                .small()
                .icon(IconName::CopyPlus)
                .tooltip(tr("复制此行"))
                .disabled(!row_actions.copy)
                .debug_selector(|| "editor-result-duplicate-row".to_string())
                .on_click(move |_, _window, app| {
                    if let Some(row) = row {
                        queue.request(ResultRequest::DuplicateRow(row), app);
                    }
                })
                .into_any_element()
        });
        let delete_row = self.editable.then(|| {
            let queue = self.queue.clone();
            let row = selected_view_row;
            Button::new("editor-result-delete-row")
                .ghost()
                .small()
                .icon(IconName::Trash)
                // 已删行上删除是空操作，灰掉并说清原因（撤销删除后才恢复）
                .tooltip(if row_deleted {
                    "这一行已标记删除；撤销删除后才能再删"
                } else {
                    "删除此行"
                })
                .disabled(!row_actions.delete)
                .debug_selector(|| "editor-result-delete-row".to_string())
                .on_click(move |_, _window, app| {
                    if let Some(row) = row {
                        queue.request(ResultRequest::DeleteRow(row), app);
                    }
                })
                .into_any_element()
        });
        // 撤销 / 重做组合钮：格级栈优先、空走行级栈（宿主分派时判定）；常驻注入，
        // 栈空由 `can_undo` / `can_redo` 置灰，槽位不消失。
        let undo = {
            let queue = self.queue.clone();
            Button::new("editor-result-undo")
                .ghost()
                .small()
                .icon(IconName::Undo2)
                .tooltip(tr("撤销"))
                .disabled(!self.can_undo)
                .debug_selector(|| "editor-result-undo".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::UndoAction, app);
                })
                .into_any_element()
        };
        let redo = {
            let queue = self.queue.clone();
            Button::new("editor-result-redo")
                .ghost()
                .small()
                .icon(IconName::Redo2)
                .tooltip(tr("重做"))
                .disabled(!self.can_redo)
                .debug_selector(|| "editor-result-redo".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::RedoAction, app);
                })
                .into_any_element()
        };

        // 【B7】导出：格式 × 范围。菜单项由 `export::menu_items_for` 给（纯函数），
        // 静态上下文（连接 / 键列 / 行数）在 sync 时由编辑器算好——render 期只读
        let export = self.export_ctx.as_ref().map(|ctx| {
            let queue = self.queue.clone();
            let ctx = ctx.clone();
            DropdownButton::new("editor-result-export")
                .small()
                .button(
                    Button::new("editor-result-export-btn")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-export".to_string())
                        .label(tr("⤓ 导出")),
                )
                .dropdown_menu(move |menu, _window, _app| {
                    let mut menu = menu;
                    // 【B7 切片三】附加上下文：`SQL Updates` 要有**键列**才摆
                    // （键列由宿主在 sync 时查表结构端口算好；查不到 = 不摆，不猜
                    // 「拿第一列当键」——改错行的代价太高）
                    let key = ctx.key.clone();
                    let extras = export::ExportExtras {
                        sql_text: true,
                        key,
                    };
                    for item in export::menu_items_for(ctx.has_more, &ctx.rows_text, &extras) {
                        if item.separator_before {
                            menu = menu.separator();
                        }
                        let Some((format, scope)) = item.action else {
                            menu = menu.item(PopupMenuItem::label(item.label));
                            continue;
                        };
                        let queue = queue.clone();
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| {
                                queue.request(
                                    ResultRequest::Export {
                                        format,
                                        all: matches!(scope, export::ExportScope::All),
                                    },
                                    app,
                                );
                            },
                        ));
                    }
                    menu
                })
                .into_any_element()
        });

        // 【T1 工具栏收口 2026-11-24】预置聚合下拉已退出工具栏：统一入口是结果集标签页
        // 右键「分析这份结果集…」（进 08 对话框，§13.2）；预置聚合项的去留见 Q19。

        // 【计算行数】状态行中段那颗钮（DBeaver `rowCountLabel:2155-2180`：
        // `ActiveStatusMessage` + `UIIcon.COMPILE`，`isActionEnabled = hasData()`
        // ——没有网格就置灰）。算回来的数字**就地**顶掉「计算行数」四个字，
        // 再点一次重算（DBeaver 点已算过的标签也是重算）。
        // 本地分析结果另置灰：它跑在桥接过来的那份行上，临时表早就不在了（见 `count_rows_available`）。
        let count_rows = {
            let queue = self.queue.clone();
            let enabled = count_rows_available(self.status.is_some(), self.result_is_analysis);
            let note = self.row_count_note.clone();
            let tooltip = count_rows_tooltip(self.result_is_analysis, note.is_some());
            let button = Button::new("editor-result-count-rows")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-count-rows".to_string())
                .disabled(!enabled)
                .tooltip(tooltip)
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::CountRows, app);
                });
            let button = match note {
                Some(text) => button.label(text),
                None => button.icon(IconName::Calculator),
            };
            Some(button.into_any_element())
        };
        let controls = result_grid::ResultControls {
            // 【筛选条显隐】对齐 DBeaver `resultset.show.filterPanel`：关掉时整条不摆
            // （`updateFilterPanelVisibility:2002-2012` 对 `ResultSetFilterPanel` 整块
            // `setVisible`；本仓筛选条就是 `render_toolbar` 的左段 `filter` 实参）
            filter: self.show_filter_panel.then_some(filter),
            value_sidebar: None,
            compare: None,
            commit: None,
            analysis: None,
            copy: None,
            export,
            refresh,
            more,
            fetch_all,
            first_page,
            prev_page,
            next_page,
            last_page,
            edit_row,
            insert_row,
            duplicate_row,
            delete_row,
            undo: Some(undo),
            redo: Some(redo),
            // 「计算行数」只由状态行消费（工具栏不放）；这里先挂上，随后解构进状态行
            count_rows,
        };
        // 【DBeaver 对齐】工具栏横跨整个顶部（从主 pane 里抽出来）
        // 解构 controls，取出工具栏需要的字段；状态栏（面板级第四行）与主 pane
        // 各拿一份：AnyElement 不实现 Clone，状态动作按钮只渲染一处——pane 内已不再
        // 读它们（grid.rs render 只渲染网格），故移动进状态栏，pane.controls 传 default。
        let result_grid::ResultControls {
            filter,
            value_sidebar: _,
            compare: _,
            commit: _,
            analysis: _,
            copy: _,
            export,
            refresh,
            more,
            fetch_all,
            first_page,
            prev_page,
            next_page,
            last_page,
            edit_row,
            insert_row,
            duplicate_row,
            delete_row,
            undo,
            redo,
            // 状态行要的那颗钮（弹到上面的 status_controls 里）
            count_rows: count_rows_el,
        } = controls;
        // 【四行布局】状态栏动作控件 = 解构出的移动字段重组（more / fetch_all / 翻页 /
        // 行级 / 撤销重做 / 导出 / 刷新；`..Default` 兜底其余槽位）。导出 / 刷新 2026-10-28
        // 从工具栏移入状态栏（用户口径：执行信息与导出刷新都归结果集状态栏）
        let status_controls = result_grid::ResultControls {
            more,
            fetch_all,
            first_page,
            prev_page,
            next_page,
            last_page,
            edit_row,
            insert_row,
            duplicate_row,
            delete_row,
            undo,
            redo,
            export,
            refresh,
            count_rows: count_rows_el,
            ..Default::default()
        };
        // 工具栏：右栏开关已移状态栏右端，这里不再摆（None）；导出/刷新已移状态栏
        // SQL 入口已并入筛选框第 1 格（「显示 SQL」，见上方 `sql_chip`），工具栏左段不再单摆
        let toolbar_el = result_grid::render_toolbar(filter, cx);
        // 状态栏右段耗时取自工具栏投影；先取出再移动 `toolbar` 进 pane
        let elapsed_ms = toolbar.elapsed_ms;
        // 【状态栏三项】右段只显示 内存 / 查询时间 / 查询条数（用户口径 2026-10-28）。
        // 内存文案在这里从 toolbar 投影（取完就随 toolbar 移进 pane，不能再读）：
        // 内存档报列式占用；表档只标载体；无事实档不摆段。
        let status_memory = match toolbar.fact_tier {
            Some(CurrentTier::InMemory) if toolbar.fact_bytes > 0 => Some(format!(
                "内存 {}",
                result_grid::bytes_text(toolbar.fact_bytes)
            )),
            Some(CurrentTier::InMemory) => Some("内存档".to_string()),
            Some(CurrentTier::OnDuckDb) | Some(CurrentTier::None) | None => None,
        };
        // 【状态栏三项】写语句影响行数（网格行数由 status 提供；这里补写语句口径）
        let status_affected = toolbar.affected_rows;
        // 【四行布局】状态行渲染条件放宽：网格有状态行；写语句 / 失败只有执行信息
        // （affected_rows / failed / elapsed_ms）也给状态栏——用户口径：导出 / 刷新 /
        // 执行信息都在状态栏，写语句也要能刷新（须在 `toolbar` 移动进 pane 之前算）
        let show_status = self.status.is_some()
            || toolbar.affected_rows.is_some()
            || toolbar.failed
            || toolbar.elapsed_ms.is_some();

        let result_pane = result_grid::render(
            &self.grid,
            result_grid::ResultPane {
                toolbar,
                // 状态行已抽到面板级第四行（render_status_row），pane 内不再渲染
                status: None,
                // 状态动作按钮只由 render_status_row 消费，pane 不需要第二份
                controls: result_grid::ResultControls::default(),
                card,
                // 标签条已提到面板级第一行（跨全宽），主 pane 不再渲染
                tabs: None,
                notice: self.notice.clone(),
                row_count_input: None,
                ..Default::default()
            },
            cx,
        )
        .into_any_element();

        // 【四行布局】（2026-10-28 拍板）：
        //   第一行 tabs（跨全宽）→ 第二行工具栏（跨全宽）→ 第三行主体（左栏|内容|右栏）
        //   → 第四行状态栏（跨全宽，左端左栏开关 / 右端右栏开关）。
        // 左右栏 DBeaver 式：默认展开（对齐 DBeaver 常驻），状态栏两端开关可完全收起
        // （收起 = 不占宽度）。对比模式双栏，左右辅助栏让位（三栏会把网格挤没，
        // 辅助栏信息在双栏里都有）。

        // 【侧栏】左栏开关（状态栏最左）：左侧栏 = 网格 / 文本 / 记录竖条。
        // 收起态图标用 PanelLeftOpen（点击动作 = 展开），展开态 PanelLeft（当前状态）。
        let left_switch = {
            let queue = self.queue.clone();
            Button::new("editor-result-left-sidebar-toggle")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-left-sidebar-toggle".to_string())
                .selected(self.show_left_sidebar)
                .toggled(self.show_left_sidebar)
                .icon(if self.show_left_sidebar {
                    IconName::PanelLeft
                } else {
                    IconName::PanelLeftOpen
                })
                .tooltip(if self.show_left_sidebar {
                    "收起左栏（网格 / 文本 / 记录）"
                } else {
                    "展开左栏（网格 / 文本 / 记录）"
                })
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::ToggleLeftSidebar, app);
                })
                .into_any_element()
        };

        // 【2026-10-28 微调】右侧开关（状态栏右端）：三态循环（收起 → 窄展开 → 宽展开 →
        // 收起；比左侧开关多一个展开态）。图标：PanelRight（收起）/ PanelRightDashed（窄展开，
        // 只显右缘竖条图标条）/ PanelRightOpen（宽展开，竖条 + 右栏内容）。
        let right_switch = {
            let queue = self.queue.clone();
            let (icon, toggled, tooltip) = match (self.edge_bar_visible, self.show_value_sidebar) {
                (false, _) => (IconName::PanelRight, false, "展开右栏（图标条）"),
                (true, false) => (IconName::PanelRightDashed, true, "展开右栏（内容）"),
                (true, true) => (IconName::PanelRightOpen, true, "收起右栏"),
            };
            Button::new("editor-result-right-sidebar-toggle")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-right-sidebar-toggle".to_string())
                // 三态循环里「栏有内容 / 只有图标条」都算按下（收起态不亮）
                .selected(toggled)
                .toggled(toggled)
                .icon(icon)
                .tooltip(tooltip)
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::ToggleValueSidebar, app);
                })
                .into_any_element()
        };

        // 第三行主体：文本视图 / 普通视图 / 对比模式 三态
        let body = if self.presentation == Presentation::Text && !self.compare_mode {
            // 文本视图：左栏（可收起）+ 文本主区；右栏让位（无选中格语义）
            let mut row = div()
                .h_flex()
                .items_stretch()
                .w_full()
                .h_full()
                .min_h_0()
                .debug_selector(|| "editor-result-text-view".to_string());
            // 【侧栏丝滑】左右栏容器**始终挂载**（内部宽度补间 + 按 show 挂内容），
            // 不再条件渲染——收起 = 宽度平滑收缩，展开 = 宽度露出内容
            row = row.child(self.render_vertical_bar(cx));
            row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_text_view(cx)),
            )
            .into_any_element()
        } else if !self.compare_mode {
            // 普通视图：左栏（可收起）+ 主 pane + 右栏（可收起）+ 右缘竖条（常驻）
            let mut row = div()
                .h_flex()
                .items_stretch()
                .w_full()
                .h_full()
                .min_h_0()
                .debug_selector(|| "editor-result-sidebars".to_string());
            row = row.child(self.render_vertical_bar(cx));
            // 【R5 布局方向切换】主 pane 与右栏的外层容器按方向换 flex 方向：
            // 横排 = 主 pane 在左、右栏在右；竖排 = 主 pane 在上、右栏满宽在下
            // （对齐 DBeaver `viewerSash.setOrientation(verticalLayout ? VERTICAL : HORIZONTAL)`）。
            // 左缘竖条与右缘竖条都在这一层、贯穿整高，不随方向移动（与 DBeaver 竖条位置一致）。
            let mut main_area = div().flex_1().min_w_0().min_h_0();
            main_area = if self.vertical_layout {
                main_area.v_flex().items_stretch()
            } else {
                main_area.h_flex().items_stretch()
            };
            // 【R5 面板最大化】对齐 DBeaver `togglePanelsMaximize` = `viewerSash.hideUp()`：
            // 最大化期间**网格整块不渲染**，右栏独占 main_area（横排满宽 / 竖排满高）。
            // 注意这里不是「把右栏拖宽」——开合态（`show_value_sidebar`）保持原值，
            // 还原后回到用户原本的开合。
            let maximized = self.panel_maximized;
            // 【D10-6】网格键盘导航（DBeaver `LightGrid.java:3738-3841` 的 `onKeyDown`）：
            // 四向 + Home/End 挪焦点格，Shift 保留锚点扩选。事件从「焦点在网格上」的
            // 元素冒泡到这里（组件自己没挂键处理），所以先判焦点归属再接管——编辑框 /
            // 筛选框 / 侧栏有焦点时一个键都不抢。Home/End 的行端语义对齐 DBeaver 的
            // `ctrlPressed || columns.size() == 1`：只有一列数据时行也跟着甩到两端。
            let queue_key = self.queue.clone();
            let grid_key = self.grid.clone();
            let grid_container = div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .debug_selector(|| "editor-result-main-grid".to_string())
                .on_key_down(move |event: &KeyDownEvent, window, app| {
                    if !grid_key.read(app).focus_handle(app).is_focused(window) {
                        return;
                    }
                    let shift = event.keystroke.modifiers.shift;
                    let ctrl = event.keystroke.modifiers.control;
                    let one_column = grid_key.read(app).delegate().visible_column_count() <= 1;
                    let step = match event.keystroke.key.as_str() {
                        "left" => result_grid::GridKeyStep::Left,
                        "right" => result_grid::GridKeyStep::Right,
                        "up" => result_grid::GridKeyStep::Up,
                        "down" => result_grid::GridKeyStep::Down,
                        "home" => result_grid::GridKeyStep::Home {
                            row_to_start: ctrl || one_column,
                        },
                        "end" => result_grid::GridKeyStep::End {
                            row_to_end: ctrl || one_column,
                        },
                        _ => return,
                    };
                    queue_key.request(ResultRequest::StepFocus { step, extend: shift }, app);
                });
            row = row.child(
                main_area
                    .children((!maximized).then(|| grid_container.child(result_pane)))
                    .child(self.render_right_sidebar(cx)),
            );
            // 【右缘竖条】三态开关控制可见性（`edge_bar_visible`）；有可复制语义
            // （有网格结果）时才挂（与右栏页签同条件）
            row = row.children(
                (self.edge_bar_visible && self.result_can_copy)
                    .then(|| self.render_right_edge_bar(cx)),
            );
            row.into_any_element()
        } else {
            // 【并排对比】对比模式双栏：左栏 = 主选中（上面组装好的主 pane），
            // 右栏 = 对比选中（自己的标签条 + 只读网格）。右栏数据由编辑器
            // `sync_compare_grid` 提前拷进 `compare_grid`（render 只读网格状态）。
            // 右栏不接 filter/transpose/commit/analysis/export/refresh/more——那些
            // 动作的语义都归主栏，并排对照不需要第二套动作。
            let compare_tabs = (self.tabs.len() >= 2).then(|| {
                let queue = self.queue.clone();
                let compare_active = self.compare_active;
                result_sets::render(
                    &self.tabs,
                    compare_active,
                    move |index, _window, app| {
                        let index = *index;
                        queue.request(ResultRequest::SelectCompare(index), app);
                    },
                    result_sets::TabActions::default(),
                    cx,
                    &self.tabs_scroll,
                )
                .into_any_element()
            });
            // 对比栏的工具栏 / 状态行：编辑器在 `sync_compare_grid` 时投影好存进
            // `compare_toolbar` / `compare_status`（与主栏同口径；只读，互动真值不需要）
            let compare_pane = self.compare_grid.as_ref().map(|compare_grid| {
                result_grid::render(
                    compare_grid,
                    result_grid::ResultPane {
                        toolbar: self.compare_toolbar.clone().unwrap_or_default(),
                        status: self.compare_status.clone(),
                        controls: result_grid::ResultControls::default(),
                        card: None,
                        tabs: compare_tabs,
                        notice: None,
                        row_count_input: None,
                        // 对比栏是同一面板里的第二个结果区：换名，避免与主栏撞选择器
                        pane_selector: "editor-result-compare-grid-pane",
                        grid_selector: "editor-result-compare-grid",
                    },
                    cx,
                )
                .into_any_element()
            });
            match compare_pane {
                // 双栏：中间一条竖分隔线（结构元素用 `ui::HAIRLINE`），两栏各占一半
                Some(compare_pane) => div()
                    .h_flex()
                    .items_stretch()
                    .w_full()
                    .h_full()
                    .min_h_0()
                    .debug_selector(|| "editor-result-compare-layout".to_string())
                    .child(
                        div()
                            .h_flex()
                            .items_stretch()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .border_r(ui::HAIRLINE)
                            .debug_selector(|| "editor-result-compare-main".to_string())
                            // 对比模式竖条只挂主栏左侧：视图方式切换只作用于主栏，对比栏
                            // 只读；竖条 = 左栏，跟随 `show_left_sidebar` 可收起（宽度动画在
                            // render_vertical_bar 内部，此处始终挂载）
                            .child(self.render_vertical_bar(cx))
                            .child(div().flex_1().min_w_0().h_full().child(result_pane)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .debug_selector(|| "editor-result-compare-pane".to_string())
                            .child(compare_pane),
                    )
                    .into_any_element(),
                // 无对比选中（对比开关开着但没选中可比对象）：退化为单主栏
                None => div()
                    .h_flex()
                    .items_stretch()
                    .w_full()
                    .h_full()
                    .min_h_0()
                    .debug_selector(|| "editor-result-sidebars".to_string())
                    .child(self.render_vertical_bar(cx))
                    .child(div().flex_1().min_w_0().h_full().child(result_pane))
                    .into_any_element(),
            }
        };

        // 第四行：状态栏（跨全宽，横跨左右栏之下；左端左栏开关、右端右栏开关）。
        // 文本视图也有 status（行数等），故状态栏在三种主体形态下都渲染。
        // 【四行布局】状态行渲染条件放宽：网格有状态行；写语句 / 失败只有执行信息
        // （affected_rows / failed / elapsed_ms）也给状态栏——用户口径：导出 / 刷新 /
        // 执行信息都在状态栏，写语句也要能刷新（cell_edit_esc 回归教训同源）
        // 【工具栏配置】状态行末端的 ▾：勾选要显示的命令项。
        // DBeaver 是右键工具条 → Configure；本仓没有右键工具条，用末端一颗 ▾ 菜单承载
        // 同一件事（否则把 undo/redo/fetch.page 按 DBeaver 默认隐藏，就等于永久失去入口）。
        let status_config = {
            let queue = self.queue.clone();
            let items = result_grid::status_config_items(&self.hidden_status_commands);
            Some(
                DropdownButton::new("editor-result-status-config")
                    .button(
                        Button::new("editor-result-status-config-button")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Settings2)
                            .tooltip(tr("工具栏配置（勾选要显示的命令）"))
                            .debug_selector(|| "editor-result-status-config".to_string()),
                    )
                    .menu_accessibility_label("工具栏配置")
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                        let mut menu = menu;
                        for (command, label, checked) in items.iter() {
                            let command = *command;
                            let label = *label;
                            let checked = *checked;
                            let queue = queue.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label).checked(checked).on_click(
                                    move |_, _window, app| {
                                        queue.request(
                                            ResultRequest::SetStatusCommandVisible {
                                                command,
                                                visible: !checked,
                                            },
                                            app,
                                        );
                                    },
                                ),
                            );
                        }
                        menu
                    })
                    .into_any_element(),
            )
        };

        // 【结果集设置】状态行最右端的齿轮：DBeaver 状态栏 `ConfigAction`
        // （`ResultSetViewer.java:5402-5418`）打开结果集首选项页；本仓推一条请求，
        // 由宿主派发「打开设置并定位到结果集节」的动作（面板不依赖 settings crate）。
        let status_settings = {
            let queue = self.queue.clone();
            Some(
                Button::new("editor-result-status-preferences")
                    .ghost()
                    .small()
                    .icon(IconName::Settings)
                    .tooltip(tr("结果集设置"))
                    .debug_selector(|| "editor-result-status-preferences".to_string())
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::OpenResultPanelSettings, app);
                    })
                    .into_any_element(),
            )
        };

        let status_row = show_status.then(|| {
            // 【选中统计】三项开关决定的统计段（纯函数算；全关 / 空串 → None，不摆）
            let selection_stat = self
                .status
                .as_ref()
                .map(|status| result_grid::selection_statistics(status, self.selection_stat_prefs))
                .filter(|text| !text.is_empty());
            result_grid::render_status_row(
                self.status.as_ref(),
                status_controls,
                self.status.as_ref().and(Some(&self.row_count_input)),
                Some(&self.segment_input),
                selection_stat,
                elapsed_ms,
                Some(left_switch),
                // 【2026-10-28 微调】右侧开关回状态栏右端（三态：收起 / 窄展开 / 宽展开）
                Some(right_switch),
                status_memory,
                status_affected,
                &self.hidden_status_commands,
                status_config,
                status_settings,
                cx,
            )
            .into_any_element()
        });

        // 【R5 比例持久化】松手落盘点：拖拽期 `right_sidebar_dragging` 为真，
        // 此时 mouse_up 冒泡到面板根 → 入队 `EndResizeRightSidebar`，由宿主把
        // 当前宽度写进全局偏好项（拖拽过程中改的只是内存宽度，不写盘）。
        let end_resize_queue = self.queue.clone();
        let resizing_right_sidebar = self.right_sidebar_dragging;
        div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .debug_selector(|| "editor-result-root".to_string())
            // 【第 60 批】面板级键位上下文 + 焦点落点（文本态下网格不渲染时仍能切表示）
            .key_context(super::RESULT_PANEL_CONTEXT)
            .track_focus(&self.focus_handle)
            .when(resizing_right_sidebar, |el| {
                // 手柄拖拽只由左键发起，所以松手也只听左键
                el.on_mouse_up(MouseButton::Left, move |_, _window, app| {
                    end_resize_queue.request(ResultRequest::EndResizeRightSidebar, app);
                })
            })
            // 第一行：结果集标签条（跨全宽；含 C1 重命名条）
            .children(tabs)
            // 第二行：工具栏（跨全宽）
            .child(toolbar_el)
            // 【1.2】筛选展开条：点击筛选组展开按钮后，工具栏下方顶宽输入框 + ▶ + ✕
            // 【筛选条显隐】整条关掉时展开条一并收起（展开条是筛选区的延伸）
            .children(
                (self.show_filter_panel && self.filter_expanded)
                    .then(|| self.render_filter_expand_bar(cx)),
            )
            // 【结果内查找】查找栏（工具栏下方一行；与展开条互斥场景由调用方口径定：
            // 两者都开时先筛选条后查找栏，读起来仍是「筛选 → 找」）
            .children(self.find_open.then(|| self.render_find_bar(cx)))
            // 第三行：主体
            .child(body)
            // 第四行：状态栏
            .children(status_row)
            // 【2026-10-28 微调】SQL 预览弹窗**最后挂**（绘制顺序在全部内容之上，
            // 浮层才能盖住工具栏 / 主体；遮罩 absolute inset_0 铺满面板根）
            .children(self.sql_dialog_open.then(|| self.render_sql_dialog(cx)))
            // 【筛选设置】浮层同样最后挂（与 SQL 弹窗同层的模态遮罩）
            .children(
                self.filter_settings_open
                    .then(|| self.render_filter_settings(cx)),
            )
            // 【第 72 批】取全部确认浮层：与上面两层同范式、最后挂（它只可能在
            // 用户刚点过按钮的瞬间出现，与另两层不会同时开）
            .children(
                self.fetch_all_confirm_open
                    .then(|| self.render_fetch_all_confirm(cx)),
            )
            // 【第 75 批】跳列浮层：同范式最后挂（只在键位刚按下的那一刻出现）
            .children(
                self.goto_column_open
                    .then(|| self.render_goto_column(cx)),
            )
            .into_any_element()
    }

    /// 【结果内查找】查找栏：查找词输入框 + 三口径开关 + 「N 个匹配」+ 上一处 /
    /// 下一处 / 关闭。
    ///
    /// 对齐 DBeaver：结果集的查找是 `ResultsetFindReplaceOverlay`（浮层，
    /// `ResultsetFindReplaceOverlay.java:47`），带 Prev / Next 与命中数
    /// （`SpreadsheetFindReplaceTarget`）；入口是筛选条上的 CHECK 型
    /// `openFindReplaceButton`（`ResultSetFilterPanel.java:356`）；行内三颗口径开关是
    /// `FindReplaceOverlay` 的 Regex / Case / WholeWord 三颗 `SWT.CHECK`。
    ///
    /// 本仓把「浮层」落成工具栏下方一行：浮层要绝对定位覆盖网格，会把网格行/列几何
    /// 与格内命中高亮一起绑进来；而「找出并选中该格」（`find_step` 用
    /// `set_selected_cell` + `scroll_to_row/col`）已经完成了读取那一半。
    ///
    /// 未做（在册）：替换（DBeaver 走 `SpreadsheetFindReplaceTarget.replaceSelection`
    /// 改单元格值；本仓值编辑走「待提交」通道，批量替换另开一批）、格内区间高亮。
    pub(crate) fn render_find_bar(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let prev_queue = self.queue.clone();
        let next_queue = self.queue.clone();
        let close_queue = self.queue.clone();
        let only_queue = self.queue.clone();
        let only_on = self.find_filter_only;
        let hits: SharedString = if self.find_query.trim().is_empty() {
            "".into()
        } else if self.find_hits == 0 {
            "无匹配".into()
        } else {
            format!("{} 个匹配", self.find_hits).into()
        };
        let nav_disabled = self.find_hits == 0;
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
            .debug_selector(|| "editor-result-find-bar".to_string())
            .child(
                // 查找词框：与筛选框同档（XSmall 20px）+ 无边框，两行控件高度一致不跳。
                // 探针挂在包装 div 上：`Input`（RenderOnce）没有 `debug_selector`。
                div()
                    .flex_1()
                    .min_w_0()
                    .debug_selector(|| "editor-result-find-input".to_string())
                    .child(
                        Input::new(&self.find_input)
                            .xsmall()
                            .appearance(false),
                    ),
            )
            .child(
                // 三颗口径开关：DBeaver `FindReplaceOverlay` 的 `createRegexSearchButton:726`
                // / `createCaseSensitiveButton:740` / `createWholeWordsButton:751`（都是
                // `SWT.CHECK`，摆在本行输入框之后、`selectAllButton` 之前）。本仓没有
                // `FIND_REPLACE_MATCH_REGEX/_CASE/_WORD` 图标名 → 用它们惯用的视觉记号
                // 「.*」/「Aa」/「W」作 label。勾正则时全词被禁用（`isAvailable`）。
                div()
                    .h_flex()
                    .flex_none()
                    .gap_1()
                    .children({
                        let criteria = self.find_criteria;
                        FindOption::ALL.into_iter().map(move |option| {
                            let queue = self.queue.clone();
                            let on = option.get(criteria);
                            let blocked = option == FindOption::WholeWord && criteria.regex;
                            div()
                                .flex_none()
                                // `Button` 是 RenderOnce，自身 id 进不了 `debug_bounds`
                                .debug_selector(move || format!("{}-wrap", option.id()))
                                .child(
                                    Button::new(option.id())
                                        .ghost()
                                        .xsmall()
                                        .label(option.label())
                                        .selected(on)
                                        .toggled(on)
                                        .disabled(blocked)
                                        .tooltip(option.tooltip())
                                        .on_click(move |_, _window, app| {
                                            queue.request(
                                                ResultRequest::SetFindOption(option, !on),
                                                app,
                                            );
                                        }),
                                )
                        })
                    }),
            )
            .child(
                // 「只显示匹配行」：DBeaver 的 `selectAllButton`（SWT.CHECK，图标
                // `FIND_REPLACE_FIND_ALL_FILTER`）。勾上把查找词当临时行过滤。
                // `Checkbox` 是 RenderOnce，自身 id 进不了 `debug_bounds`，探针挂外层 div。
                div()
                    .flex_none()
                    .debug_selector(|| "editor-result-find-only-wrap".to_string())
                    .child(
                        Checkbox::new(ElementId::Name(SharedString::from(
                            "editor-result-find-only",
                        )))
                        .checked(only_on)
                        .label(tr("仅显示匹配"))
                        .accessibility_label("只显示匹配行")
                        .on_click(move |_checked, _window, app| {
                            only_queue.request(ResultRequest::SetFindFilterOnly(!only_on), app);
                        }),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme.colors.muted_foreground)
                    .child(hits),
            )
            .child(
                Button::new("editor-result-find-prev")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronUp)
                    .disabled(nav_disabled)
                    .debug_selector(|| "editor-result-find-prev".to_string())
                    .tooltip(tr("上一处匹配"))
                    .on_click(move |_, _window, app| {
                        prev_queue.request(ResultRequest::FindPrev, app);
                    }),
            )
            .child(
                Button::new("editor-result-find-next")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronDown)
                    .disabled(nav_disabled)
                    .debug_selector(|| "editor-result-find-next".to_string())
                    .tooltip(tr("下一处匹配（回车同效）"))
                    .on_click(move |_, _window, app| {
                        next_queue.request(ResultRequest::FindNext, app);
                    }),
            )
            .child(
                Button::new("editor-result-find-close")
                    .ghost()
                    .xsmall()
                    // 与筛选行清除钮同一符号，读作「关掉这一行」
                    .label("✕")
                    .debug_selector(|| "editor-result-find-close".to_string())
                    .tooltip(tr("关闭查找栏"))
                    .on_click(move |_, _window, app| {
                        close_queue.request(ResultRequest::CloseFind, app);
                    }),
            )
            .into_any_element()
    }
}
