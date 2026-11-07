//! 状态重建：参数变化时重建输入控件、提交创建。

use super::*;


impl GeneratorLibraryState {
    /// 按 base 当前选中重建参数行（身份变化由 `Confirm` 订阅驱动；旧输入不保留——
    /// 换生成器 = 换参数集，沿用旧值反而误导）。
    pub(crate) fn rebuild_params(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.create.as_mut() else {
            return;
        };
        draft.params.clear();
        draft._subs.clear();
        let Some(config) = draft.effective_config(cx) else {
            draft.preview.clear();
            cx.notify();
            return;
        };
        let spec = generator_catalog::spec_of(&config);
        let fields: Vec<ParamField> = spec.params.iter().copied().collect();
        // 每个参数一个订阅：Change → 写回表配置值 + 刷样例。Bool 用 Switch（不再退化成文本框）。
        for field in fields {
            let key = field.key.to_string();
            match field.kind {
                ParamKind::Complex => {
                    let initial = complex_param_text(&config, field.key);
                    let state = cx.new(|cx| TextareaState::new(window, cx));
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |state, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            state.commit_complex(&key_for_closure, &text, cx);
                            state.refresh_preview(cx);
                        },
                    );
                    draft._subs.push(_sub);
                    draft.params.push(DraftParam::Complex { key, state });
                }
                ParamKind::Bool => {
                    let initial = param_text(&config, field.key) == "true";
                    draft.params.push(DraftParam::Bool {
                        key,
                        value: initial,
                    });
                }
                kind => {
                    let initial = param_text(&config, field.key);
                    let state = cx.new(|cx| InputState::new(window, cx));
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |state, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            state.commit_scalar(&key_for_closure, &text, kind, cx);
                            state.refresh_preview(cx);
                        },
                    );
                    draft._subs.push(_sub);
                    draft.params.push(DraftParam::Scalar { key, kind, state });
                }
            }
        }
        cx.notify();
    }

    /// 标量参数 Change：校验值合法性（值存在输入框里，`patched_config` 现读现 patch）。
    pub(crate) fn commit_scalar(&mut self, key: &str, text: &str, kind: ParamKind, cx: &mut Context<Self>) {
        let Some(draft) = self.create.as_mut() else {
            return;
        };
        let Some(config) = draft.effective_config(cx) else {
            return;
        };
        draft.error = if patch_param(&config, key, text, kind).is_none() {
            Some(format!("参数 {key} 的值不合法"))
        } else {
            None
        };
        cx.notify();
    }

    /// 复杂参数 Change：解析成功清错、失败记错（值存在 Textarea 里，保存时现读）。
    pub(crate) fn commit_complex(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        if let Some(draft) = self.create.as_mut() {
            draft.error = match parse_complex_param(key, text) {
                Ok(_) => None,
                Err(reason) => Some(reason),
            };
        }
        cx.notify();
    }

    /// 样例预览：2 条示意值（固定种子，catch_unwind 兜底空参数区间 panic）。
    pub(crate) fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.create.as_mut() else {
            return;
        };
        let Some(config) = draft.patched_config(cx) else {
            draft.preview.clear();
            cx.notify();
            return;
        };
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let locale = draft.locale.clone();
        let mut rng = fake::rand::rngs::StdRng::seed_from_u64(SAMPLE_PREVIEW_SEED);
        let samples: Vec<String> = (0..2)
            .map(|row| {
                catch_unwind(AssertUnwindSafe(|| {
                    crate::generators::generate_cell(&config, &mut rng, row, &locale)
                }))
                .unwrap_or_else(|_| "（参数区间为空，无法采样）".to_string())
            })
            .collect();
        draft.preview = samples.join("，");
        cx.notify();
    }
}