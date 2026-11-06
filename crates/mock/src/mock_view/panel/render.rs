// 继承父模块（panel）与 mock_view 的全部 use 绑定
use super::*;

impl MockPanel {
    /// 表头参数行：表名 / 行数 / 种子 / 语言（表配置可改）。
    ///
    /// 四个参数**同一行**：它们同属「这张表怎么造」的一次性设定，表名独占一行会把表头撑到
    /// 两行高。每个控件前都带短标签——没有标签时两个数字框加一个下拉根本分不清谁是谁
    /// （标签沿用 `form_line` 的口径：不加冒号，靠位置关系读）。
    fn render_target_rows(&self, id: u64, cx: &mut Context<Self>) -> Div {
        let hint = text_hint(cx.theme());
        // 标签是「这个框是什么」，比它标的那个值更弱一档——此前标签与值同色，
        // 四个控件摆一排全靠位置猜谁是谁。
        let label = |text: &'static str| div().flex_none().text_xs().text_color(hint).child(text);
        // 这份配置自己的三只输入框（还没建 = 空，表头只画语言）
        let inputs = self.config_inputs.get(&id);

        let mut row = div().h_flex().items_center().gap_2().w_full();
        // 表名：唯一会变长的（表名可长），吃掉剩余宽度
        if let Some(input) = inputs.map(|inputs| inputs.table.clone()) {
            row = row.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .child(label("表名"))
                    .child(div().flex_1().min_w_0().child(Input::new(&input))),
            );
        }
        // 行数 / 种子：定宽数字框（值与标签都短，不需要伸缩）
        if let Some(input) = inputs.map(|inputs| inputs.rows.clone()) {
            row = row.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .flex_none()
                    .child(label("行数"))
                    .child(
                        div()
                            .w(rems(ui::NUM_INPUT_WIDTH))
                            .flex_none()
                            .child(Input::new(&input)),
                    ),
            );
        }
        if let Some(input) = inputs.map(|inputs| inputs.seed.clone()) {
            row = row.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .flex_none()
                    .child(label("种子"))
                    .child(
                        div()
                            .w(rems(ui::NUM_INPUT_WIDTH))
                            .flex_none()
                            .child(Input::new(&input)),
                    ),
            );
        }
        let locale = self.config_for(id).options.locale.clone();
        row = row.child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .flex_none()
                .child(label("语言"))
                .child({
                    let entity = cx.entity();
                    let config_id = id;
                    dropdown_button("mock-locale", locale_label(&locale))
                        // caret 由原语开（见 `style::dropdown_button`）：菜单藏在 caret 后面
                        // 才是「这里能展开」的信号，漏挂它就只是个看不出有菜单的按钮
                        .dropdown_menu(move |menu, _window, _cx| {
                            let mut menu = menu;
                            for candidate in LOCALES {
                                let chosen = candidate.clone();
                                let is_current = chosen == locale;
                                let entity = entity.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(locale_label(&chosen))
                                        .checked(is_current)
                                        .on_click(move |_, _, app| {
                                            let chosen = chosen.clone();
                                            entity.update(app, |panel, cx| {
                                                // 写回**这张 tab 的那份**配置：语言是配置项，
                                                // 不能因为活动指针指向别处就写错表
                                                panel.config_mut(config_id).options.locale = chosen;
                                                cx.notify();
                                            });
                                        }),
                                );
                            }
                            menu
                        })
                }),
        );
        row
    }

    /// 单表生成按钮（三态）：无结果 → 主按钮「生成」；有结果 → 收成小按钮「重新生成」
    /// （避免"要不要重跑"的分歧）；进行中 → 禁用「生成中…」。
    ///
    /// 它和四个出口同属「这张表的动作」，与它们排在同一条动作行里（生成 → 落地闭环），
    /// 不再单独占一行（原 `render_generate_row`）——表头省下的一行给列区与预览。
    fn render_generate_button(&self, id: u64, cx: &mut Context<Self>) -> AnyElement {
        let running = self.is_running();
        let generating = self.is_generating();
        let has_result = self.has_result();
        let entity = cx.entity();
        let config_id = id;
        // 生成过一次之后收成 Soft：这张表的主行动让位给四个出口（生成的目的就是落地）
        let mut generate_btn = if has_result {
            button("mock-detail-generate", "重新生成", ActionKind::Soft).xsmall()
        } else {
            button(
                "mock-detail-generate",
                if generating { "生成中…" } else { "生成" },
                ActionKind::Main,
            )
        };
        if !running {
            generate_btn = generate_btn.on_click(move |_, _, app| {
                entity.update(app, |panel, cx| {
                    // 生成的是这张 tab 的表：先切活动指针，任务读的就是这一份配置
                    panel.focus_config(config_id);
                    panel.run_generate(cx)
                });
            });
        }
        generate_btn.into_any_element()
    }

    /// 这张表的**设计与生成状态**（画在中央 tab 的表头，决策 D38）。
    ///
    /// 为何在中央：表名 / 行数 / 种子 / 语言与列定义同属「这张表怎么造」，进度 / 取消 / 失败
    /// 与重试从属于同一次生成——它们属于这张表，不属于「有哪些表」那份清单（那是右 Dock 的事）。
    pub(crate) fn render_design_head(
        &mut self,
        id: u64,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.ensure_inputs_for(id, window, cx);

        let mut head = div()
            .v_flex()
            .gap_2()
            .w_full()
            .child(self.render_target_rows(id, cx));

        if self.scenario.is_some() {
            // 场景态：单表生成让位给「生成 N 张表」（在右 Dock），这里只说明表配置的用途
            head = head.child(hint_line(
                cx.theme(),
                "这是表配置：把表名 / 行数 / 列调好，再去右侧「＋ 加表」把它加进本次生成",
            ));
        } else {
            // 动作行（生成 + 四个出口）：它和「表名 / 行数 / 列」同属这张表的闭环，
            // 从预览段上移到这里——原来画在预览上方，出口竖排又把预览挤下去，
            // 每次落地都要往下找。上移后生成 → 落地的读法连成一条。
            // 进度 / 结果 / 失败也在这条动作行右侧（`render_action_status`）：
            // 它们说的是「这次动作」的进展与结果，原来各占一行，现在贴着按钮读。
            head = head.child(self.render_sinks(id, key, window, cx));
        }

        head
    }

    /// 清单行（表名那一行）**下缘 2px 的进度线**：进行中的那张表才画。
    ///
    /// 为什么不画在行内：清单行只有 ~24px 高，且同时承载选中底 / 悬停底 / 左缘标识条，
    /// 行内再塞一条进度条会把表名挤没；压到下缘 2px 则不占行内布局、不与任何底色叠色，
    /// 长表名截断也不影响读数。集合任务（场景 N 张表 / 批量落库）逐表推进是
    /// 「哪些过了、正在过哪张」，用三态（满线 / 扫光 / 不画）说，比挤一个百分比准。
    fn row_progress_overlay(
        &self,
        key: &str,
        table: &str,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let state = self.row_progress(table, index)?;
        let color = match state {
            // 已经过完了：换成功色，与「还在过」的强调色区分开
            RowProgress::Done => cx.theme().colors.success,
            _ => cx.theme().colors.primary,
        };
        Some(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .child(row_progress_line(key, state, color, cx)),
        )
    }

    /// 单表任务的一句话读数（阶段 + 批次/行数 + 剩余时间）。
    ///
    /// 抽出来是因为它同时喂两处：右 Dock 的集合任务行（`render_job_row`）与中央动作行
    /// 右侧的状态格（`render_action_status`）。两处口径必须一致——同一个任务旁边
    /// 一处说「3 / 5 批」、另一处说「60%」，用户会当成两个任务。
    fn job_status_text(&self) -> Option<String> {
        let (progress, by_table_detail) = {
            let job = self.job.as_ref()?;
            (job.progress, job.kind.by_table())
        };
        let detail = match progress.phase {
            MockJobPhase::Generating if progress.batches_total == 0 => "准备中…".to_string(),
            MockJobPhase::Generating if by_table_detail => format!(
                "{} / {} 张表（每张表逐个处理）",
                progress.batches_done, progress.batches_total
            ),
            MockJobPhase::Generating => format!(
                "{} / {} 批（≈{} / {} 行）",
                progress.batches_done,
                progress.batches_total,
                with_thousands(progress.rows_done() as u64),
                with_thousands(progress.rows_total as u64)
            ),
            phase if by_table_detail => format!(
                "{}…（{} / {} 张表）",
                phase.label(),
                progress.batches_done,
                progress.batches_total
            ),
            phase => format!(
                "{}…（{} 行）",
                phase.label(),
                with_thousands(progress.rows_total as u64)
            ),
        };
        // 剩余时间与批次读数同一行：它们说的是同一件事的两头（走到哪了 / 还要多久），
        // 分开两行会让人先读到一个、再去找另一个
        let tail = self
            .job_eta()
            .and_then(eta_text)
            .map(|text| format!(" · {text}"))
            .unwrap_or_default();
        Some(format!("{detail}{tail}"))
    }

    /// 动作行右侧的状态格：进度条 + 一句消息（进行中 / 失败 / 结果摘要）。
    ///
    /// 它替代原来各占一行的三块：单表进度行（进度条 + 阶段 + 取消）、失败原因 + 重试、
    /// 结果摘要（临时表 / 行数 / 耗时）。三者都是「这次动作的进展与结果」，贴在「生成」
    /// 右侧最省一次视线移动；空闲时只占空位（`flex_1`），按钮位置不跳。
    fn render_action_status(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let slot = div()
            .h_flex()
            .items_center()
            .gap_2()
            .flex_1()
            .min_w_0()
            .justify_end();

        // 1. 进行中：迷你进度条 + 读数 + 取消（只有生成类可取消：出口类强中断会留半张表 / 半个文件）
        if self.job_row_scope() == Some(JobRowScope::Single) {
            let running = self.job.as_ref().map(|job| {
                (
                    job.progress.percent(),
                    !job.progress.phase.is_quantified(),
                    job.kind.generates(),
                    job.cancel_requested,
                )
            });
            if let Some((percent, indeterminate, cancellable, cancelling)) = running {
                let text = self.job_status_text().unwrap_or_default();
                let cancel = cancellable.then(|| {
                    let entity = cx.entity();
                    let label = if cancelling {
                        "正在取消…"
                    } else {
                        "取消"
                    };
                    let mut btn = button("mock-cancel-job", label, ActionKind::Soft).xsmall();
                    if !cancelling {
                        btn = btn.on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.cancel_job(cx));
                        });
                    }
                    btn
                });
                return slot
                    .child(
                        // 动作行只有一行高：用 4px 的迷你档（6px 的表头档会把按钮撑歪）
                        div()
                            .flex_none()
                            .w(rems(ui::ACTION_STATUS_BAR_WIDTH))
                            .child(job_progress_bar(
                                "job-single-inline",
                                percent,
                                indeterminate,
                                JobBarSize::Mini,
                                None,
                                cx,
                            )),
                    )
                    .child(card_note(&theme, text).flex_1().min_w_0().text_ellipsis())
                    .children(cancel);
            }
        }

        // 2. 失败：生成类给「重试」；出口类失败有结果，重跑一次生成没有意义
        let has_result = self.has_result();
        if let Some(err) = self.error.clone() {
            let prefix = if has_result {
                "操作失败："
            } else {
                "生成失败："
            };
            let retry = (!has_result).then(|| {
                let entity = cx.entity();
                button("mock-detail-retry", "重试", ActionKind::Soft)
                    .xsmall()
                    .disabled(self.is_running())
                    .on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| panel.run_generate(cx));
                    })
            });
            return slot
                .child(
                    // 原语给色与字号，收缩 / 截断在这里补：右侧还要挂「重试」按钮
                    error_line(&theme, format!("{prefix}{err}"))
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis(),
                )
                .children(retry);
        }

        // 3. 收尾后的成功消息（生成 / 落库 / 导出 / 草稿箱共用同一句 `outcome`）
        if let Some(text) = self.outcome.clone() {
            return slot.child(
                success_line(&theme, text)
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis(),
            );
        }

        // 4. 有结果但没有一句话：把「这次的代价」摆出来（临时表 / 行数 / 耗时）
        if let Some(info) = self.current_info() {
            let text = format!(
                "临时表 {} · {} 行 · {} ms",
                info.temp_table_name,
                with_thousands(u64::from(info.row_count)),
                info.elapsed_ms
            );
            return slot.child(card_note(&theme, text).flex_1().min_w_0().text_ellipsis());
        }

        // 空闲且没有任何消息：留空位，不写占位文案（占位会被读成「还没开始」）
        slot
    }

    /// 集合任务（右 Dock 场景清单下）的进度行：`Progress` + 阶段文案 + 取消。
    ///
    /// 单表任务不再走这里——它并进了中央动作行右侧的状态格（`render_action_status`），
    /// 那里有「生成」按钮可贴。集合任务画在右 Dock，没有动作行，仍是进度条独占一行。
    ///
    /// - 生成类有批次粒度回调 → 定量进度条 + 取消（引擎在批次边界响应）；
    /// - 出口类（写入分析库 / 写文件）跑在 DuckDB 与文件系统内部，拿不到中间进度 →
    ///   不定量动画；也不给取消（强中断会留下半张表 / 半个文件）。
    ///
    /// 空闲时返回一个空占位（保持后续元素的排列稳定）。
    fn render_job_row(&mut self, scope: JobRowScope, cx: &mut Context<Self>) -> Div {
        if self.job_row_scope() != Some(scope) {
            return div();
        }
        let Some(status) = self.job_status_text() else {
            return div();
        };
        let (percent, indeterminate, cancellable, cancelling) = match self.job.as_ref() {
            Some(job) => (
                job.progress.percent(),
                !job.progress.phase.is_quantified(),
                job.kind.generates(),
                job.cancel_requested,
            ),
            None => return div(),
        };
        let cancel = cancellable.then(|| {
            let entity = cx.entity();
            let label = if cancelling {
                "正在取消…"
            } else {
                "取消"
            };
            let mut btn = button("mock-cancel-job", label, ActionKind::Soft).xsmall();
            if !cancelling {
                btn = btn.on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.cancel_job(cx));
                });
            }
            btn
        });

        div()
            .v_flex()
            .gap_1()
            .w_full()
            .child(job_progress_bar(
                // 两个作用域只会同时渲染一个（`job_row_scope`），但 id 仍分开：
                // 切作用域时若共用 id，扫光的动画相位会被另一处接着用
                match scope {
                    JobRowScope::Single => "job-single",
                    JobRowScope::Collection => "job-collection",
                },
                percent,
                // 出口阶段（写库 / 写文件）拿不到中间进度 → 不定量滑动
                indeterminate,
                JobBarSize::Head,
                None,
                cx,
            ))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(card_note(cx.theme(), status))
                    .children(cancel),
            )
    }

    /// 场景模板菜单按钮（中央表头的生成行与空态卡片共用；`id` 分开是避免同帧撞 id）。
    pub(crate) fn render_scenario_button(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let running = self.is_running();
        let templates = self.scenario_templates.clone();
        let entity = cx.entity();
        button(id, "场景模板", ActionKind::Soft)
            .xsmall()
            .dropdown_caret(true)
            .disabled(running)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if templates.is_empty() {
                    return menu.item(PopupMenuItem::new("（没有可用的场景模板）").disabled(true));
                }
                for choice in templates.iter() {
                    let id = choice.id.clone();
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(choice.menu_label()).on_click(
                        move |_, _, app| {
                            let id = id.clone();
                            entity.update(app, |panel, cx| panel.open_scenario(&id, cx));
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }

    /// 预览标题行右侧的「行数」菜单（预览要显示多少行）。
    ///
    /// 为何是固定档而不是输入框：行数是**重查代价**的旋钮（`LIMIT N`），档位够用就不必解析输入、
    /// 也不必管非法值；同时把「预览多少行」放到看得见的地方（生成时只读 [`PREVIEW_ROWS`] 行，
    /// 想要更多是**重查**临时表，不是就地多显示几行）。
    pub(crate) fn render_preview_limit_menu(
        &self,
        key: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entity = cx.entity();
        let current = self.preview_limit;
        dropdown_button(
            format!("mock-preview-limit-{key}"),
            format!("行数 {current}"),
        )
        .dropdown_menu(move |menu, _window, _cx| {
            let mut menu = menu;
            for choice in PREVIEW_LIMIT_CHOICES {
                let entity = entity.clone();
                menu = menu.item(
                    PopupMenuItem::new(format!("{choice} 行"))
                        .checked(choice == current)
                        .on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.set_preview_limit(choice, cx));
                        }),
                );
            }
            menu
        })
        .into_any_element()
    }

    /// 批量起手按钮（预设 / 导入结构）：与「场景模板」同属一列——把这张表的列**起手**。
    /// 放在操作行左侧（紧跟场景模板），与「＋加列」（单个新增，永远最右）分开。
    pub(crate) fn render_batch_source_buttons(&self, id: u64, cx: &mut Context<Self>) -> Div {
        let import = {
            let entity = cx.entity();
            let config_id = id;
            button("mock-import-schema", "导入结构", ActionKind::Soft)
                .xsmall()
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.focus_config(config_id);
                        panel.open_import_dialog(window, cx);
                    });
                })
        };
        // 字段预设：常用字段（电话/地址/姓名等）一行打包，从分组菜单一键加列。
        // 与「导入结构」（源库批量）同属批量起手；应用后是普通列可再编辑
        let presets = {
            let entity = cx.entity();
            let config_id = id;
            button("mock-preset-column", "预设", ActionKind::Soft)
                .xsmall()
                .icon(IconName::Folder)
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for category in PresetCategory::ALL {
                        let items: Vec<FieldPreset> = builtin_presets()
                            .into_iter()
                            .filter(|p| p.category == category)
                            .collect();
                        if items.is_empty() {
                            continue;
                        }
                        // 组标题：disabled 只读行，不参与点击
                        menu = menu.item(PopupMenuItem::new(category.label()).disabled(true));
                        for preset in items {
                            let entity = entity.clone();
                            let id = preset.id;
                            menu = menu.item(PopupMenuItem::new(preset.label).on_click(
                                move |_, _, app| {
                                    let Some(preset) = preset_by_id(id) else {
                                        return;
                                    };
                                    entity.update(app, |panel, cx| {
                                        panel.focus_config(config_id);
                                        panel.add_preset_column(&preset, cx);
                                    });
                                },
                            ));
                        }
                    }
                    menu
                })
        };
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(presets)
            .child(import)
    }

    /// ＋加列：列区主行动，永远在操作行最右。
    pub(crate) fn render_add_column_button(&self, id: u64, cx: &mut Context<Self>) -> Div {
        let entity = cx.entity();
        let config_id = id;
        // 列区的主行动；「＋」走组件图标（与工具栏「新增 Mock 表」同一 `Plus`），不手拼字符
        let add = button("mock-add-column", "加列", ActionKind::Main)
            .xsmall()
            .icon(IconName::Plus)
            .on_click(move |_, _, app| {
                entity.update(app, |panel, cx| {
                    panel.focus_config(config_id);
                    let index = panel.draft.columns.len() + 1;
                    panel.add_column(format!("column_{index}"), ColumnDataType::Integer, cx);
                });
            });
        div().h_flex().items_center().child(add)
    }

    /// 表清单：**唯一的管理入口**（D38）——单表 / 本次生成 / 结果表三态共用一个渲染器。
    ///
    /// 行 = 状态点 + 表名 + 行数；点一行 = 切 / 开那张表的中央 tab（选中行就是当前表）。
    /// 关系挂在**子表**行下的 `↳` 子行（真身是列上的 `dependency`），不再单开一段。
    fn render_table_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let success = cx.theme().colors.success;
        let warning = cx.theme().colors.warning;
        let primary = cx.theme().colors.primary;
        let danger = cx.theme().colors.danger;
        let running = self.is_running();
        // 一次性取主题（clone）：循环里还要可变借用 `cx`（`cx.entity()` 等），
        // 不能一路把 `cx.theme()` 的借用攥在手里。
        // 选中 / 悬停的色与卡片形态都收在 `row_card*` 原语里（与 M4 导航同口径：
        // 选中走 `list_active` 族，不当场挑 `accent`）
        let theme = cx.theme().clone();

        let mut list = div().v_flex().gap_1().w_full();

        // 清单搜索框：按表名过滤行（与原型「搜索表名…」对齐；空搜索 = 全显示）。
        // 搜索词只影响显示，不影响下标语义（index 仍是 results 里的真实位置）。
        let search = self.search_input.clone();
        list = list.child(
            div().w_full().child(
                search
                    .map(|input| div().child(Input::new(&input)))
                    .unwrap_or_else(|| div()),
            ),
        );
        let query = self
            .search_input
            .clone()
            .map(|input| input.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();

        // ── 结果表：每张结果表一个中央 tab；状态点说「落了没 / 缺不缺父表」──
        if !self.results.is_empty() {
            let count = self.results.len();
            let current_index = self.current.min(count.saturating_sub(1));
            list = list.child(section(
                &theme,
                format!("结果表（{count}）· 点一行切到它的 tab"),
            ));
            for (index, info) in self.results.iter().enumerate() {
                if !query.is_empty() && !info.table_name.to_lowercase().contains(&query) {
                    continue;
                }
                let selected = index == current_index;
                let name = info.table_name.clone();
                let (icon, status, color) = match self.table_status(&name) {
                    TableStatus::Persisted => {
                        (Some(IconName::CircleCheck), "已落库".to_string(), success)
                    }
                    TableStatus::NotPersisted => {
                        (Some(IconName::Check), "已生成".to_string(), success)
                    }
                    TableStatus::DanglingParent => {
                        (Some(IconName::Info), "引用的表未落库".to_string(), warning)
                    }
                    // 生成中的那一行把百分比说出口（D38 的 `◐ orders 40%`）：
                    // 集合任务不敢保证看到的是哪一张，但单表任务停在哪个百分位一眼就能读
                    TableStatus::Generating => (
                        Some(IconName::RefreshCw),
                        format!("生成中 {:.0}%", self.job_percent()),
                        primary,
                    ),
                    TableStatus::Failed => {
                        (Some(IconName::CircleX), "生成失败".to_string(), danger)
                    }
                    TableStatus::Idle => (None, "未生成".to_string(), muted),
                };
                let open = {
                    let entity = cx.entity();
                    let click_name = name.clone();
                    // 键盘可达：行内左对齐文本不宜换 `Button`（其内容容器固定 `justify_center`，
                    // 换上去会破坏左对齐与选中加粗），按 skill 的自绘下限补焦点——
                    // `use_keyed_state` 每行一个独立焦点句柄（与 Button 同款），Enter/Space 激活。
                    // id 必须是稳定业务键（表名），不能随选中态变，否则一换选中就丢焦点。
                    let open_handle = window
                        .use_keyed_state(
                            ElementId::Name(SharedString::from(format!(
                                "mock-result-open-{click_name}"
                            ))),
                            cx,
                            |_, cx| cx.focus_handle(),
                        )
                        .read(cx)
                        .clone();
                    let entity_kb = entity.clone();
                    let click_kb = name.clone();
                    div()
                        .id(ElementId::Name(SharedString::from(format!(
                            "mock-result-open-{click_name}"
                        ))))
                        .role(gpui::Role::Button)
                        // 不再 `flex_1`：名字取自然宽度，好让状态紧跟其后；
                        // 长表名靠 `min_w_0` + `text_ellipsis` 收缩，状态那格永远留得住
                        .min_w_0()
                        .text_xs()
                        .font_weight(if selected {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_ellipsis()
                        .cursor_pointer()
                        .track_focus(&open_handle)
                        .tab_stop(true)
                        .child(name.clone())
                        .on_click(move |_, window, app| {
                            let name = click_name.clone();
                            entity.update(app, |panel, cx| {
                                panel.select_result(index, cx);
                                panel.open_table_detail(&name, window, cx);
                            });
                        })
                        .on_key_down(move |event: &KeyDownEvent, window, app| {
                            // Enter / Space 与点击同一动作（gpui 可点元素在 KeyUp 激活，
                            // 这里在 KeyDown 直接触发同一入口，行为与 Button 的键盘点击等价）
                            if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                                let name = click_kb.clone();
                                entity_kb.update(app, |panel, cx| {
                                    panel.select_result(index, cx);
                                    panel.open_table_detail(&name, window, cx);
                                });
                            }
                        })
                };
                // 清单卡片（v_flex）：上面一行是这张表的信息，下面挂它的关系与下缘进度线。
                // 卡片化之前这是贴在面板上的几行字，只有选中才临时上色——平时行与行之间没有边界
                let mut card = if selected {
                    row_card_selected(&theme)
                } else {
                    row_card_interactive(&theme)
                };
                card =
                    card.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            // 表名 + 状态同处一栏：状态紧跟表名，一眼读出「这张表现在什么态」。
                            // 行数仍在行尾——它是量纲，不是状态，不该夹在名字和状态中间。
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .flex_1()
                                    .min_w_0()
                                    .child(open)
                                    .child(render_status_cell(icon, &status, color)),
                            )
                            .child(div().flex_none().text_xs().text_color(muted).child(format!(
                                "{} 行",
                                with_thousands(u64::from(info.row_count))
                            ))),
                    );
                // 关系挂在**这这张卡的卡内**（`pl_4` 仍表达从属）：卡片化之后清单的视觉
                // 单位是「一张表 = 一张卡」，关系属于这张表，不该掉到卡片外面单独成行
                for relation in self.outgoing_relations(&name).iter() {
                    let range = self.relation_range(relation);
                    card = card.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .pl_4()
                            .text_xs()
                            .text_color(text_hint(&theme))
                            .child(div().flex_1().min_w_0().text_ellipsis().child(format!(
                                "↳ {} → {}.{}",
                                relation.child_column,
                                relation.parent_table,
                                relation.parent_column
                            )))
                            .children(range.map(|range| div().flex_none().child(range))),
                    );
                }
                // 下缘 2px 进度线：这张表走到哪了（切走中央 tab 也还看得见）
                if let Some(line) =
                    self.row_progress_overlay(&format!("result-{name}"), &name, index, cx)
                {
                    card = card.child(line);
                }
                list = list.child(card);
            }
            return list;
        }

        // ── 本次 Mock 表：场景工作副本（还没有结果）——行尾「编辑 / ✕」，关系挂子表行下 ──
        if let Some(template) = self.scenario.clone() {
            // 空白场景（0 张表）用原型专门的引导文案，别让「本次 Mock 表（0）」空着不知所云
            let heading = if template.tables.is_empty() {
                "空白场景 · 0 张表——从「＋ 加表」开始".to_string()
            } else {
                format!(
                    "本次 Mock 表（{}）· {}",
                    template.tables.len(),
                    template.name
                )
            };
            list = list.child(section(&theme, heading));
            let relations = relations_of(&template);
            // `enumerate` 的序号是**集合里的真实位置**（搜索过滤只影响显示，不改它）：
            // 逐表推进的进度线按它判断「这张过了没有」
            for (index, table) in template.tables.iter().enumerate() {
                if !query.is_empty() && !table.name.to_lowercase().contains(&query) {
                    continue;
                }
                let name = table.name.clone();
                let edit = {
                    let entity = cx.entity();
                    let click_name = name.clone();
                    let mut btn = button(
                        SharedString::from(format!("mock-scenario-table-edit-{click_name}")),
                        "编辑",
                        ActionKind::Quiet,
                    )
                    .xsmall()
                    .disabled(running);
                    if !running {
                        btn = btn.on_click(move |_, window, app| {
                            let name = click_name.clone();
                            entity.update(app, |panel, cx| {
                                panel.open_table_dialog(&name, window, cx);
                            });
                        });
                    }
                    btn
                };
                let remove = {
                    let entity = cx.entity();
                    let click_name = name.clone();
                    let mut btn = icon_button(
                        SharedString::from(format!("mock-scenario-table-remove-{click_name}")),
                        IconName::Close,
                        "从本次生成移除这张表",
                    )
                    .disabled(running);
                    if !running {
                        btn = btn.on_click(move |_, _, app| {
                            let name = click_name.clone();
                            entity.update(app, |panel, cx| {
                                panel.remove_scenario_table(&name, cx);
                            });
                        });
                    }
                    btn
                };
                // 行焦点句柄：行内操作是悬停才显形的，键盘用户没有悬停，
                // 焦点落到行上时也要显形。按业务键（表名）缓存句柄——每帧新建会丢焦点。
                let row_handle = window
                    .use_keyed_state(
                        ElementId::Name(SharedString::from(format!("mock-scenario-row-{name}"))),
                        cx,
                        |_, cx| cx.focus_handle(),
                    )
                    .read(cx);
                let row_focused = row_handle.is_focused(window);
                let row_active =
                    self.hovered_scenario_table.as_deref() == Some(name.as_str()) || row_focused;
                let hover_border = theme.colors.ring;
                // 清单卡片（v_flex）：卡片化之后清单的视觉单位是「一张表 = 一张卡」
                let mut card = row_card(&theme)
                    // 元素 id 是挂 hover 事件的前提（无 id 的元素收不到 hover）
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-scenario-row-{name}"
                    ))))
                    // 行可被 Tab 到（行内操作悬停才显形，键盘得先落到行上）
                    .track_focus(&row_handle)
                    .tab_stop(true)
                    .on_hover({
                        let entity = cx.entity();
                        let hover_name = name.clone();
                        move |&hovering, _, app| {
                            entity.update(app, |panel, cx| {
                                let next = hovering.then(|| hover_name.clone());
                                if panel.hovered_scenario_table != next {
                                    panel.hovered_scenario_table = next;
                                    cx.notify();
                                }
                            });
                        }
                    });
                // 卡内第一行：表名 + 状态 + 行数 + 行操作
                card = card
                    // 悬停 / 键盘聚焦：卡片描边转 ring。卡片底已经是卡片表面色，
                    // 再拿 `list_hover` 换底等于没换（亮主题下它与卡片底同为 #f5f5f5）
                    .when(row_active, move |c| c.border_color(hover_border))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            // 表名 + 状态同栏（同上）：场景里的表还没生成，状态一律「待生成」
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_xs()
                                            .text_ellipsis()
                                            .child(name.clone()),
                                    )
                                    .child(render_status_cell(None, "待生成", muted)),
                            )
                            .child(div().flex_none().text_xs().text_color(muted).child(format!(
                                "{} 行",
                                with_thousands(u64::from(table.row_count))
                            )))
                            .child(if row_active {
                                // 显形：鼠标在这行上 / 键盘焦点在这行上
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .flex_none()
                                    .child(edit)
                                    .child(remove)
                            } else {
                                // 不显形：占位容器保持宽度，行数那一格不会跟着按钮左右跳
                                div().flex_none().w(rems(ui::ROW_OPS_WIDTH))
                            }),
                    );
                // 关系收进这张卡的卡内（`pl_4` 仍表达从属；行尾 ✕ 删关系）
                for relation in relations.iter().filter(|r| r.child_table == name) {
                    let range = self.relation_range(relation);
                    let remove_relation = {
                        let entity = cx.entity();
                        let child_table = relation.child_table.clone();
                        let child_column = relation.child_column.clone();
                        icon_button(
                            SharedString::from(format!(
                                "mock-relation-remove-{child_table}-{child_column}"
                            )),
                            IconName::Close,
                            "删除这条关系",
                        )
                        .on_click(move |_, _, app| {
                            let child_table = child_table.clone();
                            let child_column = child_column.clone();
                            entity.update(app, |panel, cx| {
                                panel.remove_relation(&child_table, &child_column, cx);
                            });
                        })
                    };
                    card = card.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .pl_4()
                            .text_xs()
                            .text_color(text_hint(&theme))
                            .child(div().flex_1().min_w_0().text_ellipsis().child(format!(
                                "↳ {} → {}.{}",
                                relation.child_column,
                                relation.parent_table,
                                relation.parent_column
                            )))
                            .children(range.map(|range| div().flex_none().child(range)))
                            .child(remove_relation),
                    );
                }
                // 下缘 2px 进度线：场景生成是逐表推进的，这条线说「轮到 / 过了哪张」
                if let Some(line) =
                    self.row_progress_overlay(&format!("scenario-{name}"), &name, index, cx)
                {
                    card = card.child(line);
                }
                list = list.child(card);
            }

            // 自定义多表：把当前表配置加进来（导入结构 / 列编辑都在单表态里做完）
            let add_table = {
                let entity = cx.entity();
                // 场景清单的主行动：把调好的表配置收进本次生成
                let mut btn = button(
                    "mock-scenario-table-add",
                    "＋ 加表（当前表配置）",
                    ActionKind::Main,
                )
                .xsmall()
                .disabled(running);
                if !running {
                    btn = btn.on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            panel.add_draft_to_scenario(cx);
                        });
                    });
                }
                btn
            };
            let add_relation = {
                let entity = cx.entity();
                let mut btn = button("mock-add-relation", "＋ 加关系", ActionKind::Soft)
                    .xsmall()
                    .disabled(running);
                if !running {
                    btn = btn.on_click(move |_, window, app| {
                        entity.update(app, |panel, cx| {
                            panel.open_relation_dialog(window, cx);
                        });
                    });
                }
                btn
            };
            list = list
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .child(add_table)
                        .child(add_relation),
                )
                .child(hint_line(
                    &theme,
                    "关系只在本次多表生成内成立（不读已落数据、不改已生成的表 / 文件）；行尾是取值域：父列自增参数 × 父表行数，改父表行数它跟着变。",
                ))
                .child(hint_line(
                    &theme,
                    "加进来的表取表配置的表名 / 行数 / 列：先在中央那张表配置 tab 里调好，再加进本次生成。",
                ));
            return list;
        }

        // ── 单表态（还没有结果）：**每一份表配置各占一行** ──
        // 多配置之后右面板必须能看见、也能切回全部配置：只画活动那一份时，
        // 开过第二个「＋」就再也回不到前一份（用户 2026-11-05 的反馈）。
        let configs = self.config_rows();
        list = list.child(section(
            &theme,
            format!("表配置（{}）· 点一行切到它的 tab", configs.len()),
        ));
        for (id, name, columns, _rows) in configs {
            if !query.is_empty() && !name.to_lowercase().contains(&query) {
                continue;
            }
            let selected = id == self.config_id;
            // 状态按**表名**查：这份配置若已经生成过结果，名字就对得上。
            // 百分比只对活动那份有意义（面板级任务指针指向它）。
            let (icon, status, color) = match self.table_status(&name) {
                TableStatus::Generating if selected => (
                    Some(IconName::RefreshCw),
                    format!("生成中 {:.0}%", self.job_percent()),
                    primary,
                ),
                TableStatus::Generating => (None, "生成中".to_string(), primary),
                TableStatus::Persisted => {
                    (Some(IconName::CircleCheck), "已落库".to_string(), success)
                }
                TableStatus::NotPersisted => (Some(IconName::Check), "已生成".to_string(), success),
                TableStatus::DanglingParent => {
                    (Some(IconName::Info), "引用的表未落库".to_string(), warning)
                }
                TableStatus::Failed => (Some(IconName::CircleX), "生成失败".to_string(), danger),
                TableStatus::Idle => (None, "未生成".to_string(), muted),
            };
            // 活动那份沿用稳定 id `mock-draft-open`（宿主用例与键盘焦点都指着它）；
            // 其余按配置 id 生成，避免多行重复 ElementId（同帧重复 id 会撞 a11y 校验）。
            let row_id = if selected {
                SharedString::from("mock-draft-open")
            } else {
                SharedString::from(format!("mock-config-open-{id}"))
            };
            let open_handle = window
                .use_keyed_state(ElementId::Name(row_id.clone()), cx, |_, cx| {
                    cx.focus_handle()
                })
                .read(cx)
                .clone();
            let entity = cx.entity();
            let entity_kb = entity.clone();
            let open = div()
                .id(row_id)
                .role(gpui::Role::Button)
                // 自绘可点行：test_support 让 headless 测试可观察（与导航行同口径）
                .test_support()
                .min_w_0()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_ellipsis()
                .cursor_pointer()
                .track_focus(&open_handle)
                .tab_stop(true)
                .child(name.clone())
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.open_detail_for(DetailTarget::Config(id), window, cx);
                    });
                })
                .on_key_down(move |event: &KeyDownEvent, window, app| {
                    if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                        entity_kb.update(app, |panel, cx| {
                            panel.open_detail_for(DetailTarget::Config(id), window, cx);
                        });
                    }
                });
            let mut card = if selected {
                row_card_selected(&theme)
            } else {
                row_card_interactive(&theme)
            }
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .child(open)
                            .child(render_status_cell(icon, &status, color)),
                    )
                    // 列数是**这份配置**最能区分彼此的一件事（行数常常一样）
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{columns} 列")),
                    ),
            );
            // 下缘 2px 进度线：只有活动那份在跑任务
            if selected {
                if let Some(line) = self.row_progress_overlay("draft", &name, 0, cx) {
                    card = card.child(line);
                }
            }
            list = list.child(card);
        }
        list
        // 状态图例整行删掉：图例解释 6 种态，而实际态已经直接跟在表名旁（图标 + 文字），
        // 再单独列一遍是拿说明挤占 280px 面板的清单空间。状态文字本身就说清了含义。
    }

    /// 这张表引用的父表里，还有没落库的吗（`⚠ 引用的表未落库` 的数据来源）。
    ///
    /// 只看**本会话的**落库记录（`landed_tables`）：项目库里本来就有同名表不算它落过。

    /// 右 Dock 的面板主体：表清单 + 集合动作 + 出口 + 出口反馈（D38）。
    fn render_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let running = self.is_running();
        let generating = self.is_generating();

        // 集合级动作：生成 N 张表 / 退出场景（它属于这一批，不属于某张表 → 留在管理侧，D38）
        let scenario_actions = {
            let mut row = div().h_flex().items_center().gap_2().w_full();
            if let Some(template) = self.scenario.clone() {
                let start = {
                    let entity = cx.entity();
                    // 场景态的主行动：这一批就这一个填色按钮
                    let label = if generating {
                        "生成中…".to_string()
                    } else {
                        format!("生成 {} 张表", template.tables.len())
                    };
                    let mut btn = button("mock-scenario-generate", label, ActionKind::Main);
                    if !running {
                        btn = btn.on_click(move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                // 点生成 = 这一轮数据重新验收：先清掉上一轮残留的搜索过滤，
                                // 免得新清单被旧关键词挡住（曾误判「只生成了一张表」）
                                if let Some(search) = panel.search_input.clone() {
                                    search.update(cx, |state, cx| state.set_value("", window, cx));
                                }
                                panel.run_scenario(cx);
                            });
                        });
                    }
                    btn
                };
                // S5：场景种子（留空 = 随机；填了 = 整组可复现，历史行标注 seed）
                let seed_box = self
                    .scenario_seed_input
                    .clone()
                    .map(|input| {
                        div()
                            .w(rems(ui::NUM_INPUT_WIDTH))
                            .flex_none()
                            .child(Input::new(&input))
                    })
                    .unwrap_or_else(|| div().flex_none());
                let exit = {
                    let entity = cx.entity();
                    let mut btn = button("mock-scenario-exit", "退出场景", ActionKind::Soft)
                        .disabled(running);
                    if !running {
                        btn = btn.on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.close_scenario(cx));
                        });
                    }
                    btn
                };
                row = row
                    .child(div().flex_1().min_w_0().child(start))
                    .child(seed_box)
                    .child(div().flex_none().child(exit));
            }
            row
        };

        // ① 表清单：唯一的管理入口（单表 / 本次生成 / 结果表三态共用一个渲染器）
        let table_list = self.render_table_list(window, cx);

        // 关系闭包里还没落库的：摆出来 + 一键依次落（免得漏落，留下悬空引用）
        let pending_row = {
            let pending = self.pending_relation_tables();
            if pending.is_empty() {
                div()
            } else {
                // 出口区（批量）的主行动：与单表的「持久化」同口径
                let persist_btn = {
                    let entity = cx.entity();
                    let mut btn = button(
                        "mock-persist-related",
                        format!("落库这 {} 张", pending.len()),
                        ActionKind::Main,
                    )
                    .xsmall()
                    .disabled(running);
                    if !running {
                        btn = btn.on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.persist_related(cx));
                        });
                    }
                    btn
                };
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        warning_line(
                            cx.theme(),
                            format!("还有 {} 张没落库：{}", pending.len(), pending.join("、")),
                        )
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis(),
                    )
                    .child(persist_btn)
            }
        };

        // 多表结果（场景生成）：与「落库这 N 张」对称的批量草稿箱出口。
        // 单表结果（1 张）不显示——单表走出口区「保存到草稿箱」即可。
        // 没项目根不显示：草稿箱只写项目目录（与单表出口同口径）。
        let batch_row = {
            let count = self.results.len();
            let has_root = self.host.project_root().is_some();
            if count > 1 && has_root && !running {
                let entity = cx.entity();
                let mut btn = button(
                    "mock-scratchpad-all",
                    format!("全部存草稿箱（{} 张）", count),
                    ActionKind::Soft,
                )
                .xsmall();
                btn = btn.on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.scratchpad_all(cx));
                });
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .text_ellipsis()
                            .child("场景结果一键导出（CSV）："),
                    )
                    .child(btn)
            } else {
                div()
            }
        };

        // 任务进行中：进度条（组件，不手搓）+ 量纲文案 + 取消
        // 集合任务的进度与取消（单表任务的在中央表头，见 D38）
        let job_row = self.render_job_row(JobRowScope::Collection, cx);

        // 单表出口（落库 / 追加 / 草稿箱 / 另存为）已随表上移中央详情 tab（见 `render_sinks`）；
        // 这里只保留集合级动作、进度与边界说明。
        let read_only = self.host.read_only();

        // 边界状态一行说清（未打开项目 / 只读 / 常驻 M7）：原来三行常驻压缩成一条，
        // 只有当前生效的那条露面，完整句进 HoverCard（280px 面板里三行长说明 = 半屏清单）
        let project_open = self.host.project_root().is_some();
        let status_line = {
            let (main, detail) = if !project_open {
                (
                    "未打开项目：落库 / 追加 / 草稿箱不可用",
                    "未打开项目：落库 / 追加 / 草稿箱不可用（纯生成、预览与另存为不受影响）",
                )
            } else if read_only {
                (
                    "只读模式：不允许落库与写文件",
                    "只读模式：不允许落库与写文件（仍可生成预览）",
                )
            } else {
                (
                    "数据只写入项目分析库与文件，不回传源库（M7）",
                    "数据只写入项目分析库（{项目}/.RSmeta/analytics.duckdb）与文件，不回传源库（M7）",
                )
            };
            HoverCard::new("mock-sink-status-hint")
                .trigger(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .w_full()
                        // info 图标走 `info_hint_icon` 原语（尺寸 / 弱化色的单一来源）：
                        // 悬停明细的记号全模块同一档，用户才认得出「这里能悬停看详情」
                        .child(info_hint_icon(cx.theme()))
                        .child(card_note(cx.theme(), main)),
                )
                .content(move |_, _, _cx| {
                    div()
                        .p_2()
                        .text_xs()
                        .max_w(ui::STATUS_TOOLTIP_MAX_WIDTH)
                        .child(detail)
                })
        };

        // 不再自带 `p_2`：面板滚动容器已经给过一遍内距，这里再给就是 16px——
        // 而工具栏是 8px（`PANEL_PADDING`），清单的左边缘比工具栏右错 8px，
        // 一眼看上去就是没排过的表配置（工具栏有分隔线压着，错开格外显眼）
        div()
            .v_flex()
            .gap_2()
            .w_full()
            .child(table_list)
            .child(pending_row)
            .child(batch_row)
            // ② 集合级动作与它的进度：单表任务的在中央表头（D38）
            .child(scenario_actions)
            .child(job_row)
            // ③ 单表出口已随表上移中央 tab（生成 → 落地闭环，见 §9）；这里只留集合级与边界说明
            .child(status_line)
    }

    /// 单表出口（落库 / 追加 / 草稿箱 / 另存为 + 就地反馈）：作用于「当前 tab 那张表」，
    /// 随表上移中央详情 tab（生成 → 落地闭环，2026-10-31）；右面板只留集合级出口。
    /// `window` 仅透传给「另存为」的系统保存对话框（下拉项闭包自取，无需本函数直接使用）。
    pub(crate) fn render_sinks(
        &self,
        id: u64,
        key: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        // 出口是「这张表」的动作：点之前先把活动指针切到这张 tab 对应的配置，
        // 否则会把上一张表的现场落出去。
        let config_id = id;

        let running = self.is_running();
        // 项目级出口（落库 / 追加 / 草稿箱）都要项目根：没打开项目时直接不可点，
        // 而不是点了才由任务层拒绝（「点了报错不如直接不可点」，与生成前禁用同一口径）。
        let sinks_ready = self.sinks_ready();
        let persist = {
            let entity = cx.entity();
            // 出口区唯一的 Main：其余三个是「换个去向」，只有这个是「落到项目里」。
            // 此前注释写着「视觉上给 primary」，代码挂的却是 `.secondary()`——
            // 口头的层级不算数，画出来的才是（四个同形态时，用户只能读文字分辨）
            button(
                format!("mock-persist-{key}"),
                "持久化到项目分析库",
                ActionKind::Main,
            )
            .disabled(running || !sinks_ready)
            .on_click(move |_, _, app| {
                entity.update(app, |panel, cx| {
                    panel.focus_config(config_id);
                    panel.persist_table(cx);
                });
            })
        };
        let append = {
            let entity = cx.entity();
            let tables = self.existing_tables.clone();
            button(
                format!("mock-append-{key}"),
                "追加到既有表",
                ActionKind::Soft,
            )
            .dropdown_caret(true)
            .disabled(running || !sinks_ready)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if tables.is_empty() {
                    return menu.item(PopupMenuItem::new("（项目分析库暂无表）").disabled(true));
                }
                for name in tables.iter() {
                    let table = name.clone();
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                        move |_, window, app| {
                            let table = table.clone();
                            entity.update(app, |panel, cx| {
                                panel.focus_config(config_id);
                                panel.open_append_dialog(table, window, cx);
                            });
                        },
                    ));
                }
                menu
            })
        };
        let scratchpad = {
            let entity = cx.entity();
            button(
                format!("mock-save-scratchpad-{key}"),
                "保存到草稿箱",
                ActionKind::Soft,
            )
            .dropdown_caret(true)
            .disabled(running || !sinks_ready)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for (label, format) in FILE_FORMATS {
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            panel.focus_config(config_id);
                            panel.save_scratchpad(&format, cx)
                        });
                    }));
                }
                menu
            })
        };
        let export = {
            let entity = cx.entity();
            button(format!("mock-export-{key}"), "另存为", ActionKind::Soft)
                .dropdown_caret(true)
                .disabled(running || !self.has_result())
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for (label, format) in FILE_FORMATS {
                        let entity = entity.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, app| {
                                let format = format.clone();
                                let name = entity.read(app).export_file_name(&format);
                                let dir = entity.read(app).host.export_dir();
                                let dir = if dir.trim().is_empty() {
                                    PathBuf::from(".")
                                } else {
                                    PathBuf::from(dir)
                                };
                                // 系统保存对话框（异步回传）：取消则不动
                                let receiver = app.prompt_for_new_path(&dir, Some(&name));
                                let entity = entity.clone();
                                window
                                    .spawn(app, async move |cx| {
                                        let Ok(Ok(Some(path))) = receiver.await else {
                                            return;
                                        };
                                        let path = path.to_string_lossy().to_string();
                                        cx.update(|_, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.export_file(&format, path, cx)
                                            });
                                        })
                                        .ok();
                                    })
                                    .detach();
                            }));
                    }
                    menu
                })
        };
        let landed = self.landed.clone();

        div()
            .v_flex()
            .gap_1()
            .w_full()
            // 动作行：生成 + 四个出口横排（生成 → 落地，一条动作链走完）。
            // 原来四个出口是 `w_full` 竖排，独占 4 行高度把预览往下挤；横排后
            // 表头只多一行，且「生成在前、落地在后」的顺序本身就是这条链的读法。
            // 竖向 `Separator` 撤掉：它把一条动作链切成两截，而按钮文案已各自说了去向。
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(self.render_generate_button(id, cx))
                    .child(persist)
                    .child(append)
                    .child(scratchpad)
                    .child(export)
                    // 进度条 / 失败重试 / 结果摘要都在「生成」右侧这一个状态格里：
                    // 它们说的是「这次动作」的进展与结果，贴着按钮读，不再各占一行
                    .child(self.render_action_status(cx)),
            )
            // 落库目标是「落到哪张表」，不是状态：它留成动作行下面单独一句
            .children(landed.map(|table| {
                div()
                    .text_xs()
                    .text_color(fg)
                    .child(format!("落库目标：{table}"))
            }))
    }

    /// 打开「导入源库结构」对话框。
    /// 标题栏下方的工具栏（2026-10-18 对齐原型）：新增 Mock 表（`Plus`）/ 引用结构（`HardDrive`）/
    /// 新增场景（`LayoutDashboard`）/ 刷新（`RefreshCw`）。
    ///
    /// 按钮语义全部是 `Button`（自带 hover / 焦点 / 键盘）；标题栏只留标题，
    /// 「＋」从标题栏工具栏迁到这里（原型同构：mock-toolbar 在面板头下方一行）。
    /// 行高走 `CONTROL_HEIGHT_SM`（与数据导航筛选行同档），底部分隔线用 `HAIRLINE`。
    fn render_toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let running = self.is_running();
        let entity = cx.entity();

        let new_table = {
            let entity = entity.clone();
            icon_button(
                "mock-toolbar-new-table",
                IconName::Plus,
                "新增 Mock 表：开一个中央 tab 配字段与名称",
            )
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| panel.new_table_config(window, cx));
            })
        };
        // ⇄ 引用结构 = 原型 conn-pop：先弹连接列表选来源，再进对话框（库 / schema 按连接预填）。
        // 来源在 `open_import_dialog` 里填充，render 只读内存里的 `sources`（不触发 I/O）；
        // 还没有来源时菜单给一句说明 + 「在对话框里选择连接…」兜底（保持同一弹出组件形态）。
        let import = {
            let entity = entity.clone();
            let sources = self.sources.clone();
            icon_button(
                "mock-toolbar-import",
                IconName::HardDrive,
                "引用数据库结构：选连接读表结构",
            )
            // 显式 `small`：`dropdown_menu` 返回 `DropdownMenuPopover`（无 Sizable，
            // 进不了 Toolbar 的 size 托管槽），与 Toolbar 托管的按钮保持同档
            .small()
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if sources.is_empty() {
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new("（暂无数据源）").disabled(true));
                    return menu.item(PopupMenuItem::new("在对话框里选择连接…").on_click(
                        move |_, window, app| {
                            let entity = entity.clone();
                            entity.update(app, |panel, cx| {
                                panel.open_import_dialog(window, cx);
                            });
                        },
                    ));
                }
                for source in sources.iter() {
                    let entity = entity.clone();
                    let conn_id = source.conn_id.clone();
                    menu = menu.item(PopupMenuItem::new(source.label.clone()).on_click(
                        move |_, window, app| {
                            let entity = entity.clone();
                            let conn_id = conn_id.clone();
                            entity.update(app, |panel, cx| {
                                panel.open_import_for(&conn_id, window, cx);
                            });
                        },
                    ));
                }
                menu
            })
        };
        let new_scene = {
            let entity = entity.clone();
            icon_button(
                "mock-toolbar-new-scene",
                IconName::LayoutDashboard,
                "新增场景：从 0 张表开始的工作副本",
            )
            .disabled(running)
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.new_blank_scene(cx));
            })
        };
        let refresh = {
            let entity = entity.clone();
            icon_button(
                "mock-toolbar-refresh",
                IconName::RefreshCw,
                "刷新：重读临时表与生成状态",
            )
            .disabled(self.results.is_empty())
            .on_click(move |_, _, app| {
                entity.update(app, |panel, cx| panel.refresh_tables(cx));
            })
        };
        // 工具栏右侧的清单状态提示（对齐原型 mockToolHint：单表态「N 张表配置」；
        // 场景 / 结果态显示本批表数——比原型残留的「1 张表配置」更贴合当前清单）。
        let hint = if self.results.is_empty() {
            match &self.scenario {
                Some(template) => format!("{} 张表", template.tables.len()),
                None => "1 张表配置".to_string(),
            }
        } else {
            format!("{} 张结果", self.results.len())
        };

        // 工具栏走 `toolbar::Toolbar`（roving 方向键焦点 + toolbar a11y 语义 + 统一 size 托管）；
        // 外层只画高度 / 内距 / 底部分隔线（规范：背景边框高度外层自己画）。
        div()
            .w_full()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .px(rems(ui::PANEL_PADDING))
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(
                Toolbar::new("mock-toolbar")
                    .children([new_table, new_scene, refresh])
                    // import 是 `DropdownMenuPopover` 包装（无 Sizable），走非托管 content 槽
                    .content(import)
                    .content(card_note(cx.theme(), hint)),
            )
    }

    /// 底部折叠段：生成历史 + 用户模板（默认收起）。
    ///
    /// 为何收起来：面板天天用的是上面那份「有哪些表」清单，历史与模板是偶尔翻一次的落盘记录；
    /// 展开状态存实体（不是每帧派生的），展开体走 `Collapsible`（不手搮揭示动画）。
    fn render_folds(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let border = cx.theme().colors.border;
        let open = self.fold_open;
        let toggle = {
            let entity = cx.entity();
            let label = if self.history_loaded {
                format!(
                    "生成历史（{}）· 用户模板（{}）",
                    self.history.len(),
                    self.templates.len()
                )
            } else {
                "生成历史 · 用户模板".to_string()
            };
            // 展开 / 收起箭头走图标（`ChevronDown` / `ChevronRight`），不用字形字符
            button("mock-fold-toggle", label, ActionKind::Quiet)
                .xsmall()
                .icon(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| {
                        panel.fold_open = !panel.fold_open;
                        cx.notify();
                    });
                })
        };

        let body = div()
            .v_flex()
            .gap_1()
            .w_full()
            .p_2()
            .border_t_1()
            .border_color(border)
            // 段序与折叠标题一致（「生成历史 · 用户模板」，见原型 §2 / 使用手册 §3）：
            // 展开后第一眼读到的就是标题先说的那一段
            .child(self.render_history(window, cx))
            // S5 起场景运行也记入历史（标注：scene_id + seed + 场景名，不落单表列行），
            // 所以这里没有「场景不进历史」的说明行——那条口径已随 S5 反转
            .child(self.render_templates(window, cx));

        div()
            .v_flex()
            .w_full()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(border)
            // `Collapsible` 是「揭盖」容器：触发行是普通子元素，内容走 `content`（收起时不渲染）；
            // `motion_id` 给「展开 / 收起」一段 spring 揭示动效（0.7.1 `collapsible.rs` 的 `reveal`），
            // 不给则收起 / 展开直接切，观感上是「啪」一下
            .child(toggle)
            .child(
                Collapsible::new()
                    .open(open)
                    .motion_id(ElementId::Name("mock-fold-reveal".into()))
                    .content(body),
            )
    }

    /// 用户模板段：把当前配置存下来 / 套用已有模板 / 删除。
    ///
    /// 与历史段共用同一次后台读（`HistorySnapshot`）；「保存」走对话框问名字，
    /// 空配置与空名都在事件路径上拦住（不给静默失败）。
    fn render_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // 主题一次 clone：下面循环里还要可变借用 `cx`（`cx.entity()` / `use_keyed_state`）
        let theme = cx.theme().clone();
        let fg = theme.colors.foreground;

        let header = {
            let entity = cx.entity();
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(div().flex_1().min_w_0().child(section(
                    &theme,
                    format!("用户模板（{}）", self.templates.len()),
                )))
                .child(
                    button("mock-template-save", "保存为模板…", ActionKind::Quiet)
                        .xsmall()
                        .on_click(move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                panel.open_save_template_dialog(window, cx);
                            });
                        }),
                )
        };

        let mut section = div().v_flex().gap_1().w_full().child(header);
        // 首次读取中：历史段已经说「读取中…」，这里不重复
        if self.history_loading && !self.history_loaded {
            return section;
        }
        if self.history_loaded && self.templates.is_empty() {
            // 空态走 `empty::Empty`（自带居中布局 + 虚线框 + 图标变体），不手搓居中文字块
            return section.child(
                Empty::new().header(
                    EmptyHeader::new()
                        // 模板空态给语义图标（`EmptyMedia` Icon 变体 = 圆角底框 + 图标），
                        // 不再用空框默认位
                        .media(
                            EmptyMedia::new()
                                .with_variant(EmptyMediaVariant::Icon)
                                .child(Icon::new(IconName::Folder).size(rems(ui::PANEL_ICON_SIZE))),
                        )
                        .title(EmptyTitle::new().child("还没有保存的模板"))
                        .description(
                            EmptyDescription::new().child("把当前列配置存下来，下次一键套用"),
                        ),
                ),
            );
        }

        for template in self.templates.clone() {
            let rows = template.row_count.max(0) as u64;
            let summary = template.description.clone().unwrap_or_default();
            let meta = if summary.is_empty() {
                format!("{} 行", with_thousands(rows))
            } else {
                format!("{} 行 · {summary}", with_thousands(rows))
            };
            let apply = {
                let entity = cx.entity();
                let id = template.id.clone();
                button(
                    SharedString::from(format!("mock-template-apply-{id}")),
                    "应用",
                    ActionKind::Quiet,
                )
                .xsmall()
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.apply_template(id, cx));
                })
            };
            let delete = {
                let entity = cx.entity();
                let id = template.id.clone();
                danger_text_button(
                    SharedString::from(format!("mock-template-delete-{id}")),
                    "删除",
                    cx.theme(),
                )
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.delete_template(id, cx));
                })
            };

            // 模板卡片：与历史卡片、列卡片同走 `card` 原语（表面色 + 内距 + 卡内分隔）。
            // 操作是悬停 / 聚焦才显形的——一排卡片上每张都常驻两个按钮，
            // 摆满一屏就是一屏灰按钮，那是表配置不是产品。
            let card_handle = window
                .use_keyed_state(
                    ElementId::Name(SharedString::from(format!(
                        "mock-template-card-{}",
                        template.id
                    ))),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx);
            let card_focused = card_handle.is_focused(window);
            let active = self.hovered_template == Some(template.id.clone()) || card_focused;
            section = section.child(
                card_interactive(&theme)
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-template-{}",
                        template.id
                    ))))
                    .track_focus(&card_handle)
                    .tab_stop(true)
                    .when(card_focused, |c| c.border_color(theme.colors.ring))
                    .on_hover({
                        let entity = cx.entity();
                        let hover_id = template.id.clone();
                        move |&hovering, _, app| {
                            entity.update(app, |panel, cx| {
                                let next = hovering.then(|| hover_id.clone());
                                if panel.hovered_template != next {
                                    panel.hovered_template = next;
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(template.name.clone()),
                            )
                            .child(if active {
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .flex_none()
                                    .child(apply)
                                    .child(delete)
                            } else {
                                div().flex_none().w(rems(ui::ROW_OPS_WIDTH))
                            }),
                    )
                    .child(card_divider(&theme))
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_hint(&theme))
                            .text_ellipsis()
                            .child(meta),
                    ),
            );
        }
        section
    }

    /// 生成历史段：最近若干次运行（重放 / 删除都在事件路径上交给后台）。
    ///
    /// 列表与错误分开呈现：读失败时**保留旧列表**（能看到的旧数据比一片空白有用）。
    /// 行按 `created_at` 倒序（存储层已排），每行的 id 用任务 id——用得下标会在
    /// 「最新在前」的插入下把 hover 等按 id 记的状态串行。
    fn render_history(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // 主题一次 clone：下面循环里还要可变借用 `cx`（`cx.entity()` / `use_keyed_state`）
        let theme = cx.theme().clone();
        let fg = theme.colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let danger = cx.theme().colors.danger;
        let success = cx.theme().colors.success;

        let header = {
            let entity = cx.entity();
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(div().flex_1().min_w_0().child(section(
                    &theme,
                    format!("生成历史（最近 {} 次）", history::HISTORY_LIMIT),
                )))
                .child(
                    button("mock-history-refresh", "刷新", ActionKind::Quiet)
                        .xsmall()
                        .on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.refresh_history(cx));
                        }),
                )
        };

        let mut section = div().v_flex().gap_1().w_full().child(header);
        if let Some(error) = self.history_error.clone() {
            section = section.child(error_line(&theme, error));
        }
        if self.history_loading && !self.history_loaded {
            // 不确定加载给一个 `Spinner`（小号转圈），文字保留便于弱网可读与测试断言
            return section.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(Spinner::new().small())
                    .child(div().text_xs().text_color(muted).child("读取中…")),
            );
        }
        if self.history_loaded && self.history.is_empty() {
            if self.history_error.is_none() {
                section = section.child(
                    Empty::new().header(
                        EmptyHeader::new()
                            // 历史空态给语义图标（收件箱 = 记录的去处）
                            .media(
                                EmptyMedia::new()
                                    .with_variant(EmptyMediaVariant::Icon)
                                    .child(
                                        Icon::new(IconName::Inbox).size(rems(ui::PANEL_ICON_SIZE)),
                                    ),
                            )
                            .title(EmptyTitle::new().child("还没有生成记录"))
                            .description(
                                EmptyDescription::new().child("跑一次生成后，记录会出现在这里"),
                            ),
                    ),
                );
            }
            return section;
        }

        for task in self.history.clone() {
            let rows = task.generated_rows.unwrap_or(task.row_count).max(0);
            let rows = with_thousands(rows as u64);
            let stamp = task
                .created_at
                .as_deref()
                .map(history::time_label)
                .unwrap_or_default();
            let stamp = (!stamp.is_empty())
                .then(|| div().flex_none().text_xs().text_color(muted).child(stamp));
            // 状态文案按语义取色：失败的原因原文展出来（截断由布局管）
            let (status_text, status_color) = match task.status.as_str() {
                // S5：场景历史行标注「场景 X · seed Y」（模板名已作为行标题展示）
                "success" if task.scene_id.is_some() => (
                    format!(
                        "成功 · seed {}",
                        task.seed
                            .map(|seed| seed.to_string())
                            .unwrap_or_else(|| "随机".to_string())
                    ),
                    success,
                ),
                "success" if task.save_format.as_deref() == Some(history::SAVE_FORMAT_TABLE) => {
                    (format!("成功 · {rows} 行 · 已追加"), success)
                }
                "success" => (format!("成功 · {rows} 行"), success),
                _ => (
                    task.error_message
                        .clone()
                        .unwrap_or_else(|| "失败".to_string()),
                    danger,
                ),
            };

            let replay = {
                let entity = cx.entity();
                let id = task.id.clone();
                button(
                    SharedString::from(format!("mock-history-replay-{id}")),
                    "重放",
                    ActionKind::Quiet,
                )
                .xsmall()
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.replay_history(id, cx));
                })
            };
            let delete = {
                let entity = cx.entity();
                let id = task.id.clone();
                danger_text_button(
                    SharedString::from(format!("mock-history-delete-{id}")),
                    "删除",
                    cx.theme(),
                )
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.delete_history(id, cx));
                })
            };

            // 历史卡片：与模板卡片、列卡片同走 `card` 原语。
            // 操作（重放 / 删除）悬停 / 聚焦才显形——同一模块里三种清单必须同一口径。
            let card_handle = window
                .use_keyed_state(
                    ElementId::Name(SharedString::from(format!("mock-history-card-{}", task.id))),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx);
            let card_focused = card_handle.is_focused(window);
            let active = self.hovered_history.as_deref() == Some(task.id.as_str()) || card_focused;
            section = section.child(
                card_interactive(&theme)
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-history-{}",
                        task.id
                    ))))
                    .track_focus(&card_handle)
                    .tab_stop(true)
                    .when(card_focused, |c| c.border_color(theme.colors.ring))
                    .on_hover({
                        let entity = cx.entity();
                        let hover_id = task.id.clone();
                        move |&hovering, _, app| {
                            entity.update(app, |panel, cx| {
                                let next = hovering.then(|| hover_id.clone());
                                if panel.hovered_history != next {
                                    panel.hovered_history = next;
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(task.table_name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(text_hint(&theme))
                                    .child(format!("{rows} 行")),
                            ),
                    )
                    .child(card_divider(&theme))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .children(stamp)
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_xs()
                                            .text_color(status_color)
                                            .text_ellipsis()
                                            .child(status_text),
                                    ),
                            )
                            .child(if active {
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .flex_none()
                                    .child(replay)
                                    .child(delete)
                            } else {
                                div().flex_none().w(rems(ui::ROW_OPS_WIDTH))
                            }),
                    ),
            );
        }
        section
    }

    /// 「保存为模板」对话框：只问名字——行数 / 种子 / 语言 / 列 / 生成器参数都取当前配置。
    ///
    /// 校验本身在 [`MockPanel::save_template`]；这里提前拒空配置，只是为了别弹一个
    /// 必然失败的框。名字默认填「{目标表名} 配置」。
    pub fn open_save_template_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft.columns.is_empty() {
            self.history_error =
                Some("先加列（或导入结构）再存模板：空配置存下来没有意义".to_string());
            cx.notify();
            return;
        }
        let name = self
            .template_name
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("模板名")))
            .clone();
        let suggestion = format!("{} 配置", self.draft.table_name.trim());
        name.update(cx, |state, cx| state.set_value(suggestion, window, cx));

        let panel = cx.entity();
        let columns = self.draft.columns.len();
        let rows = with_thousands(self.draft.options.rows as u64);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let body = div()
                .v_flex()
                .gap_2()
                .child(hint_line(
                    theme,
                    format!(
                        "存下当前配置：{rows} 行 · {columns} 列（含生成器与参数）。目标表名不存——套用时按当时的输入走。"
                    ),
                ))
                .child(form_line(theme, "模板名", &name));
            let panel_save = panel.clone();
            let input = name.clone();
            dialog.title("保存为模板").child(body).footer(
                DialogFooter::new()
                    .child(
                        button("mock-template-dialog-cancel", "取消", ActionKind::Soft).on_click(
                            move |_, window, app| {
                                window.close_dialog(app);
                            },
                        ),
                    )
                    .child(
                        button("mock-template-dialog-ok", "保存", ActionKind::Main).on_click(
                            move |_, window, app| {
                                let value = input.read(app).value().to_string();
                                panel_save.update(app, |panel, cx| panel.save_template(value, cx));
                                window.close_dialog(app);
                            },
                        ),
                    ),
            )
        });
    }

    /// ⇄ 连接菜单选中：记下连接后直接进导入对话框（库 / schema 由对话框按连接记录预填，
    /// 见 [`Self::open_import_dialog`] 的 `panel_match` 分支）——对应原型 conn-pop 的
    /// 「选一个连接读它的库表结构」。
    pub fn open_import_for(&mut self, conn_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        *self.import_conn.borrow_mut() = Some(conn_id.to_string());
        self.open_import_dialog(window, cx);
    }

    pub fn open_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sources.is_empty() {
            self.sources = self.host.schema_sources();
        }
        let panel_match = self
            .import_conn
            .borrow()
            .clone()
            .and_then(|id| self.sources.iter().find(|s| s.conn_id == id).cloned());
        let catalog = self
            .import_catalog
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("数据库")))
            .clone();
        let schema = self
            .import_schema
            .get_or_insert_with(|| {
                cx.new(|cx| InputState::new(window, cx).placeholder("schema（可留空）"))
            })
            .clone();
        let table = self
            .import_table
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("表名")))
            .clone();
        if let Some(source) = panel_match.as_ref() {
            let (catalog_value, schema_value, table_value) = (
                source.catalog.clone(),
                source.schema.clone(),
                self.draft.table_name.clone(),
            );
            catalog.update(cx, |s, cx| s.set_value(catalog_value, window, cx));
            schema.update(cx, |s, cx| s.set_value(schema_value, window, cx));
            table.update(cx, |s, cx| s.set_value(table_value, window, cx));
        }

        let panel = cx.entity();
        // `Rc` 包裹：dialog builder 每帧重建，内层菜单闭包逐个 clone 只增计数
        let sources = Rc::new(self.sources.clone());
        let selected = self.import_conn.clone();
        let catalog_input = catalog.clone();
        let schema_input = schema.clone();

        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let current = selected.borrow().clone();
            let mut body = div().v_flex().gap_2().child(hint_line(
                theme,
                "从源库读取表结构（列名 / 类型 / 可空 / 主键）并自动推断生成器；不读取数据本身。",
            ));
            // 连接：菜单选择（可见触发按钮）——走 `kv` 原语，与下面三行表单同起一条右端基线
            let conn_row = kv(theme, "连接", {
                let selected = selected.clone();
                let catalog_input = catalog_input.clone();
                let schema_input = schema_input.clone();
                let sources = sources.clone();
                let current_label = current
                    .as_deref()
                    .and_then(|id| sources.iter().find(|s| s.conn_id == id))
                    .map(|s| s.label.clone());
                dropdown_button(
                    "mock-import-conn",
                    match current_label {
                        Some(label) => label,
                        None => "选择连接".to_string(),
                    },
                )
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    if sources.is_empty() {
                        return menu.item(PopupMenuItem::new("（暂无数据源）").disabled(true));
                    }
                    for candidate in sources.iter() {
                        let source = candidate.clone();
                        let selected = selected.clone();
                        let catalog_input = catalog_input.clone();
                        let schema_input = schema_input.clone();
                        menu = menu.item(PopupMenuItem::new(candidate.label.clone()).on_click(
                            move |_, window, app| {
                                // 选中连接：记下 conn_id，并把连接记录的库 / schema 预填进输入框
                                *selected.borrow_mut() = Some(source.conn_id.clone());
                                let catalog = source.catalog.clone();
                                let schema = source.schema.clone();
                                catalog_input.update(app, |s, cx| s.set_value(catalog, window, cx));
                                schema_input.update(app, |s, cx| s.set_value(schema, window, cx));
                            },
                        ));
                    }
                    menu
                })
            });
            body = body.child(conn_row);
            body = body
                .child(form_line(theme, "数据库 / catalog", &catalog))
                .child(form_line(theme, "schema", &schema))
                .child(form_line(theme, "表名", &table));

            let panel_ok = panel.clone();
            let selected_ok = selected.clone();
            dialog.title("导入源库结构").child(body).footer(
                DialogFooter::new()
                    .child(
                        button("mock-import-cancel", "取消", ActionKind::Soft).on_click(
                            move |_, window, app| {
                                window.close_dialog(app);
                            },
                        ),
                    )
                    .child(button("mock-import-ok", "导入", ActionKind::Main).on_click(
                        move |_, window, app| {
                            panel_ok.update(app, |panel, cx| {
                                let request = SchemaRequest {
                                    conn_id: selected_ok.borrow().clone().unwrap_or_default(),
                                    catalog: panel
                                        .import_catalog
                                        .as_ref()
                                        .map(|i| i.read(cx).value().to_string())
                                        .unwrap_or_default(),
                                    schema: panel
                                        .import_schema
                                        .as_ref()
                                        .map(|i| i.read(cx).value().to_string())
                                        .unwrap_or_default(),
                                    table: panel
                                        .import_table
                                        .as_ref()
                                        .map(|i| i.read(cx).value().to_string())
                                        .unwrap_or_default(),
                                };
                                if request.conn_id.is_empty() {
                                    panel.fail("请选择连接", cx);
                                    return;
                                }
                                if request.table.trim().is_empty() {
                                    panel.fail("请填写表名", cx);
                                    return;
                                }
                                panel.preset_from_source(request, cx);
                            });
                            window.close_dialog(app);
                        },
                    )),
            )
        });
    }
}

/// 表单行（标签 + 输入）。
pub(crate) fn form_line(
    theme: &gpui_kit::component::Theme,
    label: &str,
    input: &Entity<InputState>,
) -> Div {
    // 走 `kv`（标签-值行）这一支原语：它与卡片里的「生成器名 / 产出类型」是同一结构，
    // 此前各写一份的后果是标签色分了两档（这里 muted_foreground、那里再淡一档）
    kv(
        theme,
        label,
        div()
            .w(rems(ui::PARAM_INPUT_WIDTH))
            .flex_none()
            .child(Input::new(input)),
    )
}

/// 复杂参数的一行：标签 + 多行文本（全宽）+ 格式提示。
///
/// 多行不能用 `form_line` 的「标签 | 定宽输入」横排：值集合一行一个，横排窄框放不下。
pub(crate) fn complex_form_line(
    theme: &gpui_kit::component::Theme,
    label: &str,
    hint: &str,
    state: &Entity<TextareaState>,
) -> Div {
    div()
        .v_flex()
        .gap_1()
        .w_full()
        .child(
            div()
                .text_xs()
                .text_color(text_hint(theme))
                .child(label.to_string()),
        )
        .child(Textarea::new(state).h(rems(ui::COMPLEX_INPUT_HEIGHT)))
        // 格式提示是「填错了才会回来读」的那一行：比标签还淡一档，不抢输入区的视线
        .child(
            div()
                .text_xs()
                .text_color(text_hint(theme))
                .child(hint.to_string()),
        )
}

impl Render for MockPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let actions = self.render_actions(window, cx);
        // 右 Dock 内容区自身可滚动（Dock 的 `#tab-content` 不产生滚动，见布局规格）
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .bg(cx.theme().colors.background)
            .child(
                div()
                    .id("mock-panel-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .w_full()
                            .p_2()
                            // Dock 的面板头已经写着「Mock 生成」：面板内不再重复一个标题，
                            // 工具栏（＋⇄▦⟳）直接接在标题栏下面（D38 + 2026-10-18 对齐原型）
                            .child(self.render_toolbar(window, cx))
                            .child(actions)
                            .child(self.render_folds(window, cx)),
                    ),
            )
    }
}
