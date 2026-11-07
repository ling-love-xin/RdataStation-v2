//! 参数区重建：生成器参数变化时重建输入控件。

use super::*;

impl MockDetailView {
    pub(crate) fn rebuild_params(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let generator = draft.def.generator.clone();
        let spec = generator_catalog::spec_of(&generator);
        let fields: Vec<ParamField> = spec.params.iter().copied().collect();
        // 换生成器时旧参数**一律不沿用**：`length` / `min` 这类同名字段在新生成器下含义
        // 可能完全不同（甚至越界），沿用会让人误以为「参数没切换成功」。
        // 只有同一生成器重建（改列名重推 / 参数校验失败重画）才复用输入框。
        let same_generator = draft.param_generator.as_deref() == Some(spec.name);
        draft.param_generator = Some(spec.name.to_string());
        let mut previous: HashMap<String, ParamInput> = if same_generator {
            draft.params.drain(..).map(|p| (p.key.clone(), p)).collect()
        } else {
            draft.params.clear();
            HashMap::new()
        };
        let mut next = Vec::with_capacity(fields.len());
        for field in fields {
            if let Some(item) = previous.remove(field.key) {
                next.push(item);
                continue;
            }
            let key = field.key.to_string();
            let (widget, _sub) = match field.kind {
                // 集合 / 加权选项：多行文本，改一次解析一次（解析失败保留上一个有效值）
                ParamKind::Complex => {
                    let state = cx.new(|cx| TextareaState::new(window, cx));
                    let initial = complex_param_text(&generator, field.key);
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |view, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            view.commit_complex_param(&key_for_closure, &text, cx);
                            // 参数变化 → 样例预览同步刷新（commit 已写回生成器或记下解析错误）
                            view.refresh_sample_preview(cx);
                        },
                    );
                    (ParamWidget::Complex(state), _sub)
                }
                kind => {
                    let initial = param_text(&generator, field.key);
                    let state = cx.new(|cx| InputState::new(window, cx));
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |view, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            let mut changed = false;
                            if let Some(draft) = view.draft.as_mut() {
                                if let Some(patched) =
                                    patch_param(&draft.def.generator, &key_for_closure, &text, kind)
                                {
                                    draft.def.generator = patched;
                                    draft.confidence = "manual".to_string();
                                    draft.sample_value.clear();
                                    changed = true;
                                }
                            }
                            if changed {
                                // 参数变化 → 样例预览同步刷新（事件路径，不在 render 做）
                                view.refresh_sample_preview(cx);
                                cx.notify();
                            }
                        },
                    );
                    (ParamWidget::Scalar(state), _sub)
                }
            };
            next.push(ParamInput { key, widget, _sub });
        }
        if let Some(draft) = self.draft.as_mut() {
            draft.params = next;
            // 参数行重建 = 生成器身份变了（换生成器 / 恢复智能默认）：旧参数的报错不再适用
            draft.param_error = None;
        }
        cx.notify();
    }

    /// 复杂参数（集合 / 加权选项）编辑：解析成功就写回生成器；失败则**保留上一个有效值**
    /// 并记下原因（只在对话框里就地提示，不把无效内容写进生成器）。
    pub(crate) fn commit_complex_param(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        match parse_complex_param(key, text) {
            Ok(value) => {
                if let Some(patched) = patch_param_value(&draft.def.generator, key, value) {
                    draft.def.generator = patched;
                    draft.confidence = "manual".to_string();
                    draft.sample_value.clear();
                }
                draft.param_error = None;
            }
            Err(reason) => draft.param_error = Some((key.to_string(), reason)),
        }
        cx.notify();
    }

    /// 按**当前输入框值**生成 2 条样例（参数实时预览）。
    ///
    /// 参数订阅里每次 Change 都会调它（见 `rebuild_params`）：生成器配置已由
    /// `patch_param` 实时写回，所以这里直接读 `draft.def.generator` 即可。
    /// 固定种子保证样例稳定可读（与最终生成的随机种子无关，纯示意）；
    /// 点「重新生成」时批次 +1，换一批（每批内部仍稳定）。
    pub(crate) fn refresh_sample_preview(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let locale = self.panel.read(cx).draft().options.locale.clone();
        // 固定种子保证「同一批参数看到的样例不变」（输入参数不会让示例乱跳）；
        // 点「重新生成」时批次 +1 → 换一批。种子按批次平移，每批都仍可复现
        let mut rng = StdRng::seed_from_u64(SAMPLE_PREVIEW_SEED.wrapping_add(draft.sample_round));
        // 生成器对「空参数区间」（如 min==max）会由 fake crate 断言 panic：
        // 预览是示意，不允许把异常带进事件路径，`catch_unwind` 兜底成占位文案
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let samples: Vec<String> = (0..2)
            .map(|row| {
                catch_unwind(AssertUnwindSafe(|| {
                    crate::generators::generate_cell(&draft.def.generator, &mut rng, row, &locale)
                }))
                .unwrap_or_else(|_| "（参数区间为空，无法采样）".to_string())
            })
            .collect();
        draft.sample_preview = samples.join("，");
    }

    /// 字段清单（列 + 生成器 + 参数摘要 + 编辑 / 智能 / 删除）。
    /// 字段区：当前搜索词下要画的列（保序；空词 = 全部）。搜索词归面板（单一权威）。
    ///
    /// 匹配规则在 [`column_matches`]（列名或生成器标签，子串、大小写不敏感）：
    /// 「搜索词一变就重画」的订阅在面板那边（`ensure_inputs`）。
    pub(crate) fn visible_columns(
        &self,
        panel: &MockPanel,
        id: u64,
        cx: &App,
    ) -> Vec<MockColumnSpec> {
        let query = panel.column_filter_query(id, cx);
        panel
            .config_for(id)
            .columns
            .iter()
            .filter(|column| {
                column_matches(
                    &query,
                    &column.def.name,
                    generator_catalog::spec_of(&column.def.generator).label,
                    &column_type_label(&column.def.data_type),
                    &column.confidence,
                )
            })
            .cloned()
            .collect()
    }
}