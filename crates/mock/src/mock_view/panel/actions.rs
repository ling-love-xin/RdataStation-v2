// ==================== 动作方法 ====================

use super::*;

impl MockPanel {
    /// 表配置（目标表名 / 列 / 行数种子）**保留**：那是用户的配置，跨项目可以继续用。
    pub fn forget_generated(&mut self, cleared: usize, cx: &mut Context<Self>) {
        self.results.clear();
        self.current = 0;
        // 排序是按临时表重查来的：临时表都没了，那份取样也就没意义了
        // 排序 / 重查取样是按临时表重查来的：临时表都没了，那份取样也就没意义了
        // （行数档是用户的偏好，跨项目留着）
        self.preview_cache = None;
        self.scenario_source = None;
        self.last_relations.clear();
        self.landed = None;
        // 另一个项目 = 另一个分析库：上一项目的落库记录、「追加到既有表」候选在这里都不成立
        self.landed_tables.clear();
        self.existing_tables.clear();
        // 连接清册也随作用域变（全局 + 该项目的 `P_` / `GP_`）：宿主在切项目后会重读，
        // 真没重读也不打紧——导入对话框在清册为空时会自己拉一次
        self.sources.clear();
        self.error = None;
        // 旧表 tab 不自动关（D35：关不关 tab 是用户的窗口布局）：它们得能说清自己为什么没结果，
        // 而「这一轮没有它的结果」在切项目后是假话（不是这一轮的事，是另一个项目的事）。
        self.project_switched = true;
        // 没清到东西就不打扰用户（切项目很常见，每次都报一句是噪声）
        self.outcome = (cleared > 0).then(|| {
            format!("已切换项目：清掉 {cleared} 张 mock 临时表，生成结果已作废（请重新生成）")
        });
        cx.notify();
    }

    /// 上一次清空结果是不是切项目造成的（旧 tab 的文案据此区分「另一个项目」与「这一轮」）。
    ///
    /// 什么时候落下：`forget_generated`（只由切项目触发）置位；下一次生成成功后清掉。
    pub fn results_dropped_by_project_switch(&self) -> bool {
        self.project_switched
    }

    /// 打开中央「Mock 数据」详情 tab（表配置 tab：字段清单 + 预览）。
    pub fn open_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_detail_for(DetailTarget::Config(self.config_id), window, cx);
    }

    /// 打开（或聚焦）某个详情 tab：表配置，或某张结果表。
    pub fn open_detail_for(
        &mut self,
        target: DetailTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.host.open_detail(target, window, cx);
    }

    /// 打开某张**结果表**的详情 tab（右 Dock 结果区点一行 / 「查看详情」走这条）。
    pub fn open_table_detail(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_detail_for(DetailTarget::Table(table.to_string()), window, cx);
    }

    /// 「查看详情」看谁：有结果就看当前表（tab 就是那张表），没结果看表配置。
    pub fn current_detail_target(&self) -> DetailTarget {
        self.results
            .get(self.current)
            .map(|info| DetailTarget::Table(info.table_name.clone()))
            .unwrap_or(DetailTarget::Config(self.config_id))
    }

    /// 中央某个 tab 被激活：把它对应的表设为「当前表」（出口作用于它）。
    ///
    /// 表不在本轮结果里（tab 是上一轮留下的）就什么都不做：`current` 不该指向不存在的表。
    pub fn focus_table(&mut self, table: &str, cx: &mut Context<Self>) {
        if let Some(index) = self
            .results
            .iter()
            .position(|info| info.table_name == table)
        {
            if index != self.current {
                self.current = index;
                cx.notify();
            }
        }
    }

    // ==================== 生成历史（后台读写；渲染只读状态） ====================

    /// 重读生成历史（打开面板 / 切换项目时调用）。
    ///

    /// 导航右键「生成 Mock 数据」：按**源库表**预填目标表名并导入其结构。
    ///
    /// 这正是 v1 的主路径（源库结构 → 造新数据），不再依赖「分析库已有同名表」。
    pub fn preset_from_source(&mut self, request: SchemaRequest, cx: &mut Context<Self>) {
        self.draft.table_name = request.table.clone();
        self.set_table_pending(request.table.clone());
        match self.host.import_columns(&request) {
            Ok(columns) => {
                self.draft.columns = columns;
                self.next_id = self.draft.columns.iter().map(|c| c.id).max().unwrap_or(0) + 1;
                self.results.clear();
                self.current = 0;
                self.landed = None;
                self.error = None;
                self.outcome = Some(format!(
                    "已从 {}.{}.{} 导入 {} 列",
                    request.catalog,
                    request.schema,
                    request.table,
                    self.draft.columns.len()
                ));
            }
            Err(e) => {
                self.outcome = None;
                self.landed = None;
                self.error = Some(e);
            }
        }
        cx.notify();
    }

    /// 生成到内存临时表（**后台任务**：立即返回，进度与结果由轮询回填）。
    ///
    /// 不在事件路径里写库：真正落库是出口的事（不变式 I5）。
    pub fn run_generate(&mut self, cx: &mut Context<Self>) {
        let table = self.draft().table_name.clone();
        let rows = self.draft().options.rows;
        tracing::info!(
            table = %table,
            rows = rows,
            "【探针】Mock「生成」on_click 触发（生成到内存临时表）"
        );
        self.start_job(MockJobKind::Generate, cx);
    }

    /// 场景模板：把模板**载入为可编辑的工作副本**（不立即生成）。
    ///
    /// 为何不选即生成：用户要能先看到「本次要生成哪几张表」、并把表间关系调好，

    /// 切换当前选中的结果表（场景生成后不止一张）。
    pub fn select_result(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.results.len() && index != self.current {
            self.current = index;
            cx.notify();
        }
    }

    /// S3：追加前先看目标表现状——打开确认对话框（行数 / 隐含主键 / 最大值），
    /// 确认后再追加。
    ///
    /// 状态在**打开时**读一次：`append_target_status` 是宿主查分析库的只读操作，
    /// 对话框期间库被改的概率与影响都小，「确认前看一眼现状」的用途不受影响。
    pub fn open_append_dialog(
        &mut self,
        table: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        // 隐含主键 = 表配置里 AutoIncrement 生成器的列（v2 不建主键 DDL，自增列就是主键）
        let pk_hint = self
            .draft
            .columns
            .iter()
            .find(|c| matches!(c.def.generator, GeneratorConfig::AutoIncrement { .. }))
            .map(|c| c.def.name.clone());
        let status = match &pk_hint {
            Some(pk) => self.host.append_target_status(&table, Some(pk)),
            None => self.host.append_target_status(&table, None),
        };
        *self.append_pick.borrow_mut() = Some(AppendPick {
            table: table.clone(),
            status,
        });

        let pick = self.append_pick.clone();
        let panel = cx.entity();
        let table_clone = table.clone();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let fg = theme.colors.foreground;
            let current = pick.borrow().clone().unwrap_or(AppendPick {
                table: table_clone.clone(),
                status: None,
            });

            let mut body = div().v_flex().gap_2();
            // 目标表现状：行数 / 隐含主键 / 最大值
            match &current.status {
                Some(status) => {
                    let pk_text = match (&status.pk_column, status.max_pk) {
                        (Some(pk), Some(max)) => format!("{pk} 最大 {max}"),
                        (Some(pk), None) => format!("{pk}（空表）"),
                        (None, _) => "（未识别到自增主键列）".to_string(),
                    };
                    body = body.child(kv(
                        theme,
                        "目标表现状",
                        div()
                            // 值用主色：这是这一行的信息主体，不是标签的注解
                            .text_xs()
                            .text_color(fg)
                            .child(format!(
                                "{} 行 · {pk_text}",
                                with_thousands(status.row_count.max(0) as u64)
                            )),
                    ));
                    // 自增起点预告：本次生成的自增列会从 MAX + 1 接续
                    if let (Some(pk), Some(max)) = (&status.pk_column, status.max_pk) {
                        body = body.child(info_line(
                            theme,
                            format!("{pk} 从 {} 开始接续（MAX + 1）", max + 1),
                        ));
                    }
                }
                None => {
                    body = body.child(warning_line(
                        theme,
                        "读不到目标表现状：将按现有行数接续自增，唯一冲突会整批拒绝。",
                    ));
                }
            }

            let cancel_panel = panel.clone();
            let ok_panel = panel.clone();
            let table_for_ok = current.table.clone();
            dialog
                .title(format!("追加到 {}", current.table))
                .child(body)
                .footer(
                    DialogFooter::new()
                        .child(
                            button("mock-append-cancel", "取消", ActionKind::Soft).on_click(
                                move |_, window, app| {
                                    let _ = cancel_panel.update(app, |panel, _cx| {
                                        *panel.append_pick.borrow_mut() = None;
                                    });
                                    window.close_dialog(app);
                                },
                            ),
                        )
                        .child(
                            button("mock-append-ok", "确认追加", ActionKind::Main).on_click(
                                move |_, window, app| {
                                    let _ = ok_panel.update(app, |panel, cx| {
                                        panel.append_table(table_for_ok.clone(), cx);
                                    });
                                    window.close_dialog(app);
                                },
                            ),
                        ),
                )
        });
    }

    /// 出口：追加到**项目**分析库既有表（显式选择；后台任务：生成 + 追加在一次任务里完成）。
    pub fn append_table(&mut self, table: String, cx: &mut Context<Self>) {
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        *self.append_pick.borrow_mut() = None;
        self.start_job(MockJobKind::AppendTo(table), cx);
    }
}
