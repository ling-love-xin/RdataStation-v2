//! 动作行状态格 + 集合任务进度行 + 场景模板菜单 + 预览行数菜单。

use super::*;

impl MockPanel {
    /// 动作行右侧的状态格：进度条 + 一句消息（进行中 / 失败 / 结果摘要）。
    ///
    /// 它替代原来各占一行的三块：单表进度行（进度条 + 阶段 + 取消）、失败原因 + 重试、
    /// 结果摘要（临时表 / 行数 / 耗时）。三者都是「这次动作的进展与结果」，贴在「生成」
    /// 右侧最省一次视线移动；空闲时只占空位（`flex_1`），按钮位置不跳。
    pub(crate) fn render_action_status(&self, cx: &mut Context<Self>) -> Div {
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
    pub(crate) fn render_job_row(&mut self, scope: JobRowScope, cx: &mut Context<Self>) -> Div {
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
}
