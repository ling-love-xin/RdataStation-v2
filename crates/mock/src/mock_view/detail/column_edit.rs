//! 列编辑收集：从对话框控件读取列配置。

use super::*;

impl MockDetailView {
    pub(crate) fn collect_column_edit(&mut self, cx: &App) -> Option<MockColumnSpec> {
        let draft = self.draft.take()?;
        let name = draft.name_input.read(cx).value().trim().to_string();
        let ratio = parse_percent_ratio(&draft.null_input.read(cx).value())
            .unwrap_or(draft.def.nullable_ratio);
        let mut def = draft.def.clone();
        def.name = name;
        def.nullable_ratio = ratio;
        Some(MockColumnSpec {
            id: draft.id,
            def,
            confidence: draft.confidence.clone(),
            sample_value: draft.sample_value.clone(),
            // 自定义项身份随列一起写回：左栏精确高亮 / 右栏显示自定义名都靠它
            user_generator: draft.picked_user.clone(),
        })
    }

    /// #7 应用并新增下一列：应用当前表配置 → 按当前列名 +2 去重新建列（沿用 §8.3 字段预设命名策略）
    /// → 立即打开新列的列编辑对话框（连续录入）。旧对话框的关闭由按钮回调先执行
    ///（`close_dialog` 需要 `&mut App`），本方法只做「应用 + 加列 + 重开对话框」。
    pub(crate) fn apply_and_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 先把活动指针切到这张 tab 的配置：下面的应用 / 加列都按「当前配置」写
        let config_id = self.config_id();
        // 先取当前列名与类型（collect 会 take 表配置，之后读不到）
        let (base_name, base_type) = self
            .draft
            .as_ref()
            .map(|d| {
                (
                    d.name_input.read(cx).value().trim().to_string(),
                    d.def.data_type.clone(),
                )
            })
            .unwrap_or((String::new(), ColumnDataType::Varchar { length: None }));
        let edited = self.collect_column_edit(cx);
        if let Some(edited) = edited {
            let panel = self.panel.clone();
            panel.update(cx, |panel, cx| {
                if let Some(id) = config_id {
                    panel.focus_config(id);
                }
                panel.apply_column(edited, cx)
            });
        }
        // 新列名去重（旧名 + `_n`，空列名给 `column_n`）；类型沿用当前列类型
        let new_name = {
            let panel = self.panel.read(cx);
            let mut n = 2u32;
            let mut candidate = if base_name.is_empty() {
                "column_2".to_string()
            } else {
                format!("{base_name}_2")
            };
            while panel
                .draft()
                .columns
                .iter()
                .any(|c| c.def.name == candidate)
            {
                n += 1;
                candidate = if base_name.is_empty() {
                    format!("column_{n}")
                } else {
                    format!("{base_name}_{n}")
                };
            }
            candidate
        };
        let new_id = {
            let panel = self.panel.clone();
            panel.update(cx, |panel, cx| {
                if let Some(id) = config_id {
                    panel.focus_config(id);
                }
                panel.add_column(new_name.clone(), base_type, cx);
                panel
                    .draft()
                    .columns
                    .last()
                    .map(|c| c.id)
                    .unwrap_or_default()
            })
        };
        self.open_column_dialog(new_id, window, cx);
    }

    /// 表配置是否有未应用修改（取消 / ESC 丢失防护判定）。
    ///
    /// 口径：列名 / 产出类型 / 生成器配置 / 空值率 / 唯一 / 任一参数输入框的值，
    /// 任一项与打开时快照（`original*`）不同即 dirty。
    pub(crate) fn draft_is_dirty(draft: &ColumnDraft, cx: &App) -> bool {
        if draft.name_input.read(cx).value().to_string() != draft.original.name {
            return true;
        }
        if draft.def.data_type != draft.original.data_type {
            return true;
        }
        if draft.def.generator != draft.original.generator {
            return true;
        }
        if draft.null_input.read(cx).value().to_string() != draft.original_null {
            return true;
        }
        if draft.def.unique != draft.original.unique {
            return true;
        }
        for (index, p) in draft.params.iter().enumerate() {
            let current = match &p.widget {
                ParamWidget::Scalar(state) => state.read(cx).value().to_string(),
                ParamWidget::Complex(state) => state.read(cx).value().to_string(),
            };
            if current
                != draft
                    .original_params
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or("")
            {
                return true;
            }
        }
        false
    }

    /// 弹出「放弃未应用的修改？」确认框（#6）：确认后清空表配置并关闭列编辑对话框。
    ///
    /// 取消按钮与 dialog `on_cancel`（ESC / 遮罩）共用。确认框叠在列编辑对话框之上
    ///（vendor 对话框支持栈式多实例）；OK 里连续两次 `close_dialog`——先关确认框自身、
    /// 再关列编辑对话框（`close_dialog_internal` 按「最后活跃」依次弹出）。
    pub(crate) fn confirm_discard(
        view: &Entity<MockDetailView>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = view.clone();
        window.open_alert_dialog(cx, move |alert, _window, _cx| {
            let view_for_ok = view.clone();
            alert
                .title("放弃未应用的修改？")
                .description("列编辑有未应用修改，取消后这些修改将丢失。")
                .show_cancel(true)
                .ok_text("放弃修改")
                .cancel_text("继续编辑")
                .on_ok(move |_, window, cx| {
                    view_for_ok.update(cx, |view, _cx| view.draft = None);
                    window.close_dialog(cx); // 关闭确认框自身
                    window.close_dialog(cx); // 再关闭列编辑对话框
                    true
                })
        });
    }
}