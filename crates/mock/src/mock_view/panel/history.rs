// ==================== 历史/模板 ====================

use super::*;
use crate::history;

impl MockPanel {
    /// 只投一次后台读：开项目库是文件 I/O，事件路径上做会冻 UI，渲染期更不行。
    pub fn refresh_history(&mut self, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::List, cx);
    }

    /// 删一条历史（删除与重读在同一件后台动作里完成）。
    pub fn delete_history(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::Delete(task_id), cx);
    }

    /// 重放一条历史：把当时的表名 / 行数 / 种子 / 语言与列配置写回表配置。
    ///
    /// **不自动生成**：重放是「把配置摆回来」，跑不跑由用户定（同模板应用口径）。
    pub fn replay_history(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::Replay(task_id), cx);
    }

    /// 把当前表配置存成用户模板（名字由保存对话框给，空名在这里拦住）。
    ///
    /// 两道门都落在动作本身而不只在对话框上：对话框只是入口之一，
    /// 校验在这里才能保证「不管谁调都不会存出一份套不出东西的模板」。
    pub fn save_template(&mut self, name: String, cx: &mut Context<Self>) {
        if self.draft.columns.is_empty() {
            self.history_error =
                Some("先加列（或导入结构）再存模板：空配置存下来没有意义".to_string());
            cx.notify();
            return;
        }
        if name.trim().is_empty() {
            self.history_error = Some("模板名不能为空".to_string());
            cx.notify();
            return;
        }
        let draft = self.draft.clone();
        self.spawn_history(HistoryTask::SaveTemplate { name, draft }, cx);
    }

    /// 应用一个用户模板：把它存的行数 / 种子 / 语言与列写回表配置（**不动目标表名**）。
    pub fn apply_template(&mut self, template_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::ApplyTemplate(template_id), cx);
    }

    /// 删一个用户模板（删除与重读在同一件后台动作里完成）。
    pub fn delete_template(&mut self, template_id: String, cx: &mut Context<Self>) {
        self.spawn_history(HistoryTask::DeleteTemplate(template_id), cx);
    }

    /// 把面板要做的一件事交给后台执行器，完成后经弱句柄回填。
    ///
    /// 未打开项目时不去跑后台：历史没有落点，直接给一句可读的原因
    /// （而不是一个看起来像「本项目没有记录」的空列表）。
    pub(crate) fn spawn_history(&mut self, task: HistoryTask, cx: &mut Context<Self>) {
        let Some(root) = self.host.project_root() else {
            // 未打开项目：历史没有落点，给一句可读的原因（而不是看起来像「本项目没有记录」）。
            // 只有「重读列表」才清空：删记录 / 重放失败时，已显示的列表比一片空白有用。
            if matches!(task, HistoryTask::List) {
                self.history.clear();
                self.templates.clear();
                self.history_loaded = true;
            }
            self.history_loading = false;
            self.history_error = Some("未打开项目：生成历史与模板随项目保存".to_string());
            cx.notify();
            return;
        };
        self.history_loading = true;
        self.history_error = None;
        let work = cx.background_executor().spawn(async move {
            // 项目库的打开与读写要 tokio 运行时（GPUI 后台线程不带）：`history::drive` 自己进一个
            match task {
                HistoryTask::List => history::drive(history::list(&root, history::HISTORY_LIMIT))
                    .map(HistoryReply::Listed),
                HistoryTask::Delete(id) => history::drive(history::run(
                    &root,
                    HistoryAction::DeleteTask(id),
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::Record { draft, run } => history::drive(history::run(
                    &root,
                    HistoryAction::Record { draft, run },
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::SaveTemplate { name, draft } => history::drive(history::run(
                    &root,
                    HistoryAction::SaveTemplate { name, draft },
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::DeleteTemplate(id) => history::drive(history::run(
                    &root,
                    HistoryAction::DeleteTemplate(id),
                    history::HISTORY_LIMIT,
                ))
                .map(HistoryReply::Listed),
                HistoryTask::ApplyTemplate(id) => {
                    history::drive(history::template_detail(&root, &id))
                        .map(|detail| HistoryReply::TemplateApplied(Box::new(detail)))
                }
                HistoryTask::Replay(id) => history::drive(history::detail(&root, &id))
                    .map(|detail| HistoryReply::Replayed(Box::new(detail))),
            }
        });
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let reply = work.await;
            // 面板可能已关闭：弱句柄升级失败就丢弃结果（不 panic）
            let _ = weak.update(cx, |panel, cx| panel.accept_history(reply, cx));
        })
        .detach();
        cx.notify();
    }

    /// 回填后台结果（`pub(crate)`：窗口测试直接喂它，不等真实后台）。
    pub(crate) fn accept_history(
        &mut self,
        reply: Result<HistoryReply, String>,
        cx: &mut Context<Self>,
    ) {
        self.history_loading = false;
        match reply {
            Ok(HistoryReply::Listed(snapshot)) => {
                self.history = snapshot.tasks;
                self.templates = snapshot.templates;
                self.history_loaded = true;
                self.history_error = None;
            }
            Ok(HistoryReply::Replayed(detail)) => {
                self.apply_replay(*detail, cx);
                self.history_loaded = true;
                self.history_error = None;
            }
            Ok(HistoryReply::TemplateApplied(detail)) => {
                let (template, columns) = *detail;
                self.apply_template_config(&template, &columns);
                self.history_loaded = true;
                self.history_error = None;
            }
            // 失败不改列表：能看到的旧列表比一片空白有用
            Err(reason) => self.history_error = Some(reason),
        }
        cx.notify();
    }

    /// 应用模板落地：行数 / 种子 / 语言与列换成模板里那一套，**目标表名不动**
    /// （纯状态动作，测试直接调）。
    pub(crate) fn apply_template_config(
        &mut self,
        template: &MockUserTemplate,
        columns: &[MockTemplateColumn],
    ) {
        let mut draft = history::draft_of_template(template, columns);
        // 模板不存表名：用户正在写的目标表名保住（换表名是另一件事）
        draft.table_name = self.draft.table_name.clone();
        let rows = draft.options.rows;
        let count = draft.columns.len();
        self.next_id = count as u64 + 1;
        self.draft = draft;
        // 旧结果作废：表配置已是另一套配置，临时表还是上一套的
        self.results.clear();
        self.current = 0;
        self.landed = None;
        self.error = None;
        self.outcome = Some(format!(
            "已应用模板 {}（{} 行 · {count} 列；种子与语言一并写入）",
            template.name,
            with_thousands(rows as u64)
        ));
    }

    /// 重放落地：表配置整体换成历史里的那一套（纯状态动作，测试直接调）。
    ///
    /// S5：场景历史（`scene_id` 有值）不落表配置——多表配置存不进单表列行，
    /// 重放按模板 id 重新取内置模板并套用当时的种子（编辑过的关系不随历史恢复）。
    pub(crate) fn apply_replay(&mut self, detail: MockGenerationDetail, cx: &mut Context<Self>) {
        if let Some(scene_id) = detail.task.scene_id.clone() {
            let seed = detail.task.seed.map(|seed| seed as u32);
            let Some(mut template) = crate::templates::get_template_by_id(&scene_id) else {
                self.fail(
                    format!("场景模板不存在：{scene_id}（可能已从内置清单移除）"),
                    cx,
                );
                return;
            };
            template.seed = seed;
            let name = template.name.clone();
            self.load_scenario(template, cx);
            self.outcome = Some(format!(
                "已重放场景 {name}（seed {}）：确认后再点生成",
                seed.map(|s| s.to_string())
                    .unwrap_or_else(|| "随机".to_string())
            ));
            return;
        }
        let draft = history::draft_of_detail(&detail);
        let table = draft.table_name.clone();
        let columns = draft.columns.len();
        self.next_id = draft
            .columns
            .iter()
            .map(|column| column.id)
            .max()
            .unwrap_or(0)
            + 1;
        self.draft = draft;
        self.set_table_pending(self.draft.table_name.clone());
        // 旧结果作废：表配置已是另一套配置，临时表还是上一套的
        self.results.clear();
        self.current = 0;
        self.landed = None;
        self.error = None;
        self.outcome = Some(format!(
            "已重放 {table} 的配置（{columns} 列）：确认后再点生成"
        ));
    }
}
