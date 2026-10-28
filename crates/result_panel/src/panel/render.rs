//! 渲染域（`panel/` 第二块）：主 `render` 组装（投影状态 → 元素树根）。


use gpui_kit::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::toolbar::ToolbarGroup;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder as _;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）
use ::shared::CurrentTier;

use crate::error_card::{self};
use crate::export;
use crate::grid::{self as result_grid, ResultToolbar};
use crate::ports::{Presentation, ResultRequest};
use crate::script_card::{self};
use crate::sets::{self as result_sets};
use crate::ui;
use super::data::ResultPanel;

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
                self.script_card.clone().map(|script| {
                    script_card::render(&script, cx).into_any_element()
                })
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
            let actions = result_sets::TabActions {
                // 「⋯」按钮：进入重命名态（事件路径；request 立即通知面板 → 宿主 drain）
                on_rename: Some(std::rc::Rc::new(move |index, _w, app| {
                    queue_rename.request(ResultRequest::BeginRename(index), app);
                })),
                on_toggle_pin: Some(std::rc::Rc::new(move |index, app| {
                    queue_pin.request(ResultRequest::TogglePin(index), app);
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
        // 【1.3 对齐】下发源库 / 注释两个 Switch 从筛选组拆出，交给工具栏右段
        // Toolbar 统一容器（与对比 / 分析 / 复制同基线），筛选组只留输入与清除。
        // 【1.2】筛选框升级：常态 180px 输入框内嵌 ▶ 运行（RunFilterAsQuery，
        // 用 `select * from (原SQL) where 词` 替换当前结果集重查）；展开按钮
        // （Maximize2）打开顶宽筛选条（filter_expanded，输入框移到工具栏下方全宽）。
        // 展开态下工具栏筛选组不再渲染输入框（同一 InputState 实体一帧只能渲染
        // 一处，双渲染会元素 id 冲突），只留展开按钮（toggled 高亮指示展开态）。
        let filter = div()
            .h_flex()
            .items_center()
            .gap_1()
            .debug_selector(|| "editor-result-filter".to_string())
            .when(!self.filter_expanded, |row| {
                let run_queue = self.queue.clone();
                row.child(
                    div()
                        .w(rems(ui::RESULT_FILTER_WIDTH))
                        .child(
                            Input::new(&self.filter_input).suffix(
                                Button::new("editor-result-filter-run")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Play)
                                    .debug_selector(|| "editor-result-filter-run".to_string())
                                    .tooltip("用 select * from (原SQL) where 词 重查（替换当前结果集）")
                                    .on_click(move |_, _window, app| {
                                        run_queue.request(ResultRequest::RunFilterAsQuery, app);
                                    }),
                            ),
                        ),
                )
            })
            .child({
                let expand_queue = self.queue.clone();
                Button::new("editor-result-filter-expand")
                    .ghost()
                    .small()
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
            .when(!self.filter_text.trim().is_empty() && !self.filter_expanded, |row| {
                let queue = self.queue.clone();
                row.child(
                    Button::new("editor-result-filter-clear")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-filter-clear".to_string())
                        .label("✕")
                        .on_click(move |_, _window, app| {
                            queue.request(ResultRequest::ClearFilter, app);
                        }),
                )
            })
            .into_any_element();

        // 【B15】「下发源库」开关：开着时应用筛选会重查（拼 WHERE 到原查询）
        let pushdown = {
            let queue = self.queue.clone();
            Switch::new("editor-result-pushdown")
                .checked(self.pushdown)
                .label("下发源库")
                .on_click(move |checked, _window, app| {
                    let open = *checked;
                    queue.request(ResultRequest::SetPushdown { open }, app);
                })
                .into_any_element()
        };
        // 列注释开关（对齐 DBeaver：列浏览器里显示列注释）
        let comments = {
            let queue = self.queue.clone();
            Switch::new("editor-result-show-comments")
                .checked(self.show_column_comments)
                .label("注释")
                .on_click(move |checked, _window, app| {
                    let open = *checked;
                    queue.request(ResultRequest::SetShowColumnComments { open }, app);
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
                .small()
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
        let commit = (self.pending_changes > 0).then(|| {
            let queue = self.queue.clone();
            let submitting = self.committing;
            let commit_btn = Button::new("editor-result-commit")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-commit".to_string())
                .disabled(submitting)
                .label(format!("提交变更 ({})", self.pending_changes))
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::CommitPending, app);
                });
            let queue = self.queue.clone();
            let discard = Button::new("editor-result-discard")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-discard".to_string())
                .label("撤销变更")
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
                .small()
                .debug_selector(|| "editor-result-copy".to_string())
                .label("复制")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::CopyTsv, app);
                })
                .into_any_element()
        });
        let refresh = (self.result_sql.is_some() && !self.result_is_analysis).then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-refresh")
                .ghost()
                .small()
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
                                queue.request(ResultRequest::Export {
                                    format,
                                    all: matches!(scope, export::ExportScope::All),
                                }, app);
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
                .small()
                .button(
                    Button::new("editor-result-analysis-btn")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-analysis".to_string())
                        .label("⚗ 分析"),
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
            insert_row,
            duplicate_row,
            delete_row,
            undo,
            redo,
        } = controls;
        // 【四行布局】状态栏动作控件 = 解构出的移动字段重组（more / fetch_all / 行级 /
        // 撤销重做 / 导出 / 刷新；`..Default` 兜底其余槽位）。导出 / 刷新 2026-10-28
        // 从工具栏移入状态栏（用户口径：执行信息与导出刷新都归结果集状态栏）
        let status_controls = result_grid::ResultControls {
            more,
            fetch_all,
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
            Some(CurrentTier::InMemory) if toolbar.fact_bytes > 0 => {
                Some(format!("内存 {}", result_grid::bytes_text(toolbar.fact_bytes)))
            }
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
            row.child(div().flex_1().min_w_0().h_full().child(self.render_text_view(cx)))
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
            row = row.child(div().flex_1().min_w_0().h_full().child(result_pane));
            row = row.child(self.render_right_sidebar(cx));
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

        div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .debug_selector(|| "editor-result-root".to_string())
            // 第一行：结果集标签条（跨全宽；含 C1 重命名条）
            .children(tabs)
            // 第二行：工具栏（跨全宽）
            .child(toolbar_el)
            // 【1.2】筛选展开条：点击筛选组展开按钮后，工具栏下方顶宽输入框 + ▶ + ✕
            .children(self.filter_expanded.then(|| self.render_filter_expand_bar(cx)))
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