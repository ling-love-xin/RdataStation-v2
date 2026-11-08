//! 渲染域（`panel/` 第二块）：主 `render` 组装（投影状态 → 元素树根）。

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::toolbar::ToolbarGroup;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）
use ::shared::CurrentTier;

use super::data::ResultPanel;
use crate::error_card::{self};
use crate::export;
use crate::grid::{self as result_grid, ResultToolbar};
use crate::ports::{PageTarget, Presentation, ResultRequest};
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
                    .label("复制")
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
                            .tooltip("用 select * from (原SQL) where 词 重查（替换当前结果集）")
                            .on_click(move |_, _window, app| {
                                run_queue.request(ResultRequest::RunFilterAsQuery, app);
                            }),
                    )
            })
            .when(
                !self.filter_text.trim().is_empty() && !self.filter_expanded,
                |row| {
                    let queue = self.queue.clone();
                    row.child(
                        Button::new("editor-result-filter-clear")
                            .ghost()
                            .xsmall()
                            .debug_selector(|| "editor-result-filter-clear".to_string())
                            .label("✕")
                            .on_click(move |_, _window, app| {
                                queue.request(ResultRequest::ClearFilter, app);
                            }),
                    )
                },
            )
            .into_any_element();

        // 【B15】「下发源库」开关：开着时应用筛选会重查（拼 WHERE 到原查询）
        // 【DBeaver 对齐 2026-11-08】不再用 `Switch`：DBeaver 那条筛选条上的等价物是带
        // 按下态的 CHECK `ToolItem`（`ResultSetFilterPanel.java:356` 的
        // `openFindReplaceButton`），不是开关。这里改为图标 toggle（`toggled` = 按下态）
        // + tooltip，保住「按下即开」的可读性，又不让一个开关占掉一整段文字宽度。
        let pushdown = {
            let queue = self.queue.clone();
            let checked = self.pushdown;
            Button::new("editor-result-pushdown")
                .ghost()
                .xsmall()
                .toggled(checked)
                .icon(IconName::DatabaseZap)
                .tooltip("下发源库：开着时应用筛选会拼 WHERE 重查源库")
                .debug_selector(|| "editor-result-pushdown".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::SetPushdown { open: !checked }, app);
                })
                .into_any_element()
        };
        // 列注释开关（对齐 DBeaver：列浏览器里显示列注释）
        let comments = {
            let queue = self.queue.clone();
            let checked = self.show_column_comments;
            Button::new("editor-result-show-comments")
                .ghost()
                .xsmall()
                .toggled(checked)
                .icon(IconName::FileText)
                .tooltip("显示列注释（列头第二行）")
                .debug_selector(|| "editor-result-show-comments".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(
                        ResultRequest::SetShowColumnComments { open: !checked },
                        app,
                    );
                })
                .into_any_element()
        };

        // 【右缘竖条】右侧辅助栏开合开关已移到最右缘竖条顶部（`render_right_edge_bar`，
        // 单入口；2026-10-28 落地），状态栏右端不再挂右栏开关，`ResultControls.value_sidebar`
        // 与 `render_status_row.right_switch` 均传 `None`。

        // 【并排对比】工具栏开关：两份以上结果才摆（一份结果没有可比的；对比栏
        // 数据只读，编辑 / 取段 / 洞察等动作仍只作用于主栏）
        let compare = (self.tabs.len() >= 2).then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-compare")
                .ghost()
                .xsmall()
                .toggled(self.compare_mode)
                .icon(IconName::Columns2)
                .tooltip("并排对比")
                .debug_selector(|| "editor-result-compare".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::ToggleCompareMode, app);
                })
                .into_any_element()
        });

        // 【T2a-7】批量提交/回滚对（参考 DBeaver）：有**待提交**变更才摆；提交中置灰
        // 【DBeaver 对齐 2026-11-08】去掉文字标签改图标 + tooltip（计数进 tooltip）：
        // DBeaver 那条工具条上没有带文字标签的按钮；待提交条数只在有变更时才存在，
        // 悬停一眼能看到确数，不再用文字撑宽度。
        let commit = (self.pending_changes > 0).then(|| {
            let queue = self.queue.clone();
            let submitting = self.committing;
            let pending = self.pending_changes;
            let commit_btn = Button::new("editor-result-commit")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-commit".to_string())
                .disabled(submitting)
                .icon(IconName::Save)
                .tooltip(format!("提交 {pending} 项变更到源库"))
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::CommitPending, app);
                });
            let queue = self.queue.clone();
            let discard = Button::new("editor-result-discard")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-discard".to_string())
                .icon(IconName::RotateCcw)
                .tooltip("撤销全部未提交变更（回滚本地改动）")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::RollbackPending, app);
                });
            // 提交/撤销成对：2026-10-28 从手搓 `div().h_flex().gap_1()` 迁
            // `toolbar::ToolbarGroup`（组件统一化 §3 容器层反例修复）。外层无样式观测壳
            // div（仅 debug_selector 锚点）：ToolbarGroup 是 RenderOnce 无 debug_selector，
            // 测试靠壳锚点断言“按钮组在不在”（观测专用，不承载任何布局/交互语义）
            div()
                .debug_selector(|| "editor-result-commit-group".to_string())
                .child(
                    ToolbarGroup::new("editor-result-commit-group")
                        .child(commit_btn)
                        .child(discard),
                )
                .into_any_element()
        });

        // 动作：有网格才摆复制（没东西可复制就不摆）；有 SQL 就摆重跑（原位刷新）
        let copy = self.result_can_copy.then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-copy")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-copy".to_string())
                .icon(IconName::Copy)
                .tooltip("复制整份结果为 TSV（含表头）")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::CopyTsv, app);
                })
                .into_any_element()
        });
        let refresh = (self.result_sql.is_some() && !self.result_is_analysis).then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-refresh")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-refresh".to_string())
                .label("⟳ 刷新")
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
                    .label("获取数据")
                    .tooltip("获取下一段数据")
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::FetchMore, app);
                    })
                    .into_any_element()
            });
        // 【DBeaver 对齐】获取全部数据：只在这份结果真还有下一段时摆（一次性抓全量）
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
                    .label("获取全部数据")
                    .tooltip("一次性抓取全部行")
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::FetchAll, app);
                    })
                    .into_any_element()
            });

        // 【分页】状态栏翻页三钮（首页 / 上页 / 末页）：**纯视图滚动**——不重查源库，
        // 因此不需要精确总数（「末页」= 已抓到的最后一行，语义是"到底了"而不是
        // "库里的第 N 页"）。下页由现有「获取数据」真钮（`more`）承担：它只在还有下一段
        // 时出现，语义正好衔接。0 行结果集不注入，回退禁用占位。
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
        let (first_page, prev_page, last_page) = if has_rows {
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
                    "editor-result-page-last",
                    IconName::SkipForward,
                    "末页（滚到已抓到的最后一行）",
                    PageTarget::Last,
                )),
            )
        } else {
            (None, None, None)
        };

        // 【#28-B3】状态栏行级动作钮：新增 / 复制 / 删除一行。只读结果集（`!editable`）
        // 不注入，回退 grid.rs 的禁用占位钮；复制 / 删除必须有选中行才可用。
        // 选中行号取状态行 1 基记录（格选中与行选中都记行；0 基交给宿主翻译身份）。
        let selected_view_row = self
            .status
            .as_ref()
            .and_then(|status| status.selected_row)
            .map(|row| row.saturating_sub(1));
        let insert_row = self.editable.then(|| {
            let queue = self.queue.clone();
            let anchor = selected_view_row;
            Button::new("editor-result-insert-row")
                .ghost()
                .small()
                .icon(IconName::Plus)
                .tooltip("在下方插入新行")
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
                .tooltip("复制此行")
                .disabled(row.is_none())
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
                .tooltip("删除此行")
                .disabled(row.is_none())
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
                .tooltip("撤销")
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
                .tooltip("重做")
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
                        .label("⤓ 导出"),
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

        // 【B15】分析：对选中那份结果的**已抓行**跑聚合 SQL（本地 DuckDB）。
        // 菜单项由编辑器投影（`AnalysisItem`）给出；没网格就不摆。
        let analysis = (!self.analysis_items.is_empty()).then(|| {
            let queue = self.queue.clone();
            let items = self.analysis_items.clone();
            DropdownButton::new("editor-result-analysis")
                .xsmall()
                .button(
                    Button::new("editor-result-analysis-btn")
                        .ghost()
                        .xsmall()
                        .debug_selector(|| "editor-result-analysis".to_string())
                        .icon(IconName::Sigma)
                        .tooltip("分析当前结果（对已抓行跑聚合）"),
                )
                .dropdown_menu(move |menu, _window, _app| {
                    let mut menu = menu;
                    for item in items.iter() {
                        let queue = queue.clone();
                        let sql = item.sql.clone();
                        menu = menu.item(PopupMenuItem::new(item.label.clone()).on_click(
                            move |_, _window, app| {
                                let sql = sql.clone();
                                queue.request(ResultRequest::Analysis { sql }, app);
                            },
                        ));
                    }
                    let queue = queue.clone();
                    menu.separator().item(
                        PopupMenuItem::new(crate::analysis::CUSTOM_LABEL).on_click(
                            move |_, _window, app| {
                                queue.request(ResultRequest::AnalysisCustom, app);
                            },
                        ),
                    )
                })
                .into_any_element()
        });

        let controls = result_grid::ResultControls {
            filter: Some(filter),
            value_sidebar: None,
            compare,
            commit,
            analysis,
            copy,
            export,
            refresh,
            more,
            fetch_all,
            first_page,
            prev_page,
            last_page,
            insert_row,
            duplicate_row,
            delete_row,
            undo: Some(undo),
            redo: Some(redo),
        };
        // 【DBeaver 对齐】工具栏横跨整个顶部（从主 pane 里抽出来）
        // 解构 controls，取出工具栏需要的字段；状态栏（面板级第四行）与主 pane
        // 各拿一份：AnyElement 不实现 Clone，状态动作按钮只渲染一处——pane 内已不再
        // 读它们（grid.rs render 只渲染网格），故移动进状态栏，pane.controls 传 default。
        let result_grid::ResultControls {
            filter,
            value_sidebar: _,
            compare,
            commit,
            analysis,
            copy,
            export,
            refresh,
            more,
            fetch_all,
            first_page,
            prev_page,
            last_page,
            insert_row,
            duplicate_row,
            delete_row,
            undo,
            redo,
        } = controls;
        // 【四行布局】状态栏动作控件 = 解构出的移动字段重组（more / fetch_all / 翻页 /
        // 行级 / 撤销重做 / 导出 / 刷新；`..Default` 兜底其余槽位）。导出 / 刷新 2026-10-28
        // 从工具栏移入状态栏（用户口径：执行信息与导出刷新都归结果集状态栏）
        let status_controls = result_grid::ResultControls {
            more,
            fetch_all,
            first_page,
            prev_page,
            last_page,
            insert_row,
            duplicate_row,
            delete_row,
            undo,
            redo,
            export,
            refresh,
            ..Default::default()
        };
        // 【2026-10-28 微调】SQL 预览：工具栏最左侧可点击元素（Button 自带 hover 高亮 +
        // 手型，提示可点击）。点击**弹出窗口**显示完整原始 SQL（kit Dialog + 只读
        // Textarea）；hover 仍给全文 tooltip。
        let sql_preview = toolbar.sql.clone().map(|sql| {
            let preview: SharedString = if sql.len() > 60 {
                format!("{}…", &sql[..60]).into()
            } else {
                sql.as_str().into()
            };
            let queue = self.queue.clone();
            Button::new("editor-result-sql-preview")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-sql-preview".to_string())
                .label(SharedString::from(format!("SQL: {}", preview)))
                .tooltip("点击查看完整 SQL（弹出窗口）")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::OpenSqlDialog, app);
                })
                .into_any_element()
        });
        // 工具栏：右栏开关已移状态栏右端，这里不再摆（None）；导出/刷新已移状态栏
        let toolbar_el = result_grid::render_toolbar(
            sql_preview,
            filter,
            Some(pushdown),
            Some(comments),
            compare,
            commit,
            analysis,
            copy,
            cx,
        );
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
            row = row.child(
                main_area
                    .child(div().flex_1().min_w_0().min_h_0().child(result_pane))
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
        let status_row = show_status.then(|| {
            result_grid::render_status_row(
                self.status.as_ref(),
                status_controls,
                self.status.as_ref().and(Some(&self.row_count_input)),
                elapsed_ms,
                Some(left_switch),
                // 【2026-10-28 微调】右侧开关回状态栏右端（三态：收起 / 窄展开 / 宽展开）
                Some(right_switch),
                status_memory,
                status_affected,
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
            .children(
                self.filter_expanded
                    .then(|| self.render_filter_expand_bar(cx)),
            )
            // 第三行：主体
            .child(body)
            // 第四行：状态栏
            .children(status_row)
            // 【2026-10-28 微调】SQL 预览弹窗**最后挂**（绘制顺序在全部内容之上，
            // 浮层才能盖住工具栏 / 主体；遮罩 absolute inset_0 铺满面板根）
            .children(self.sql_dialog_open.then(|| self.render_sql_dialog(cx)))
            .into_any_element()
    }
}
