//! 表头渲染：表名/行数/种子/语言一行 + 生成按钮 + 设计头 + 进度线 + 状态文案。

use super::*;

impl MockPanel {
    /// 四个参数**同一行**：它们同属「这张表怎么造」的一次性设定，表名独占一行会把表头撑到
    /// 两行高。每个控件前都带短标签——没有标签时两个数字框加一个下拉根本分不清谁是谁
    /// （标签沿用 `form_line` 的口径：不加冒号，靠位置关系读）。
    pub(crate) fn render_target_rows(&self, id: u64, cx: &mut Context<Self>) -> Div {
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
    pub(crate) fn render_generate_button(&self, id: u64, cx: &mut Context<Self>) -> AnyElement {
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
    pub(crate) fn row_progress_overlay(
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
    pub(crate) fn job_status_text(&self) -> Option<String> {
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
}
