//! 工具栏动作入口（sync_mode / on_save / on_format / trigger_completion）
//!
//! 自 host/toolbar.rs 拆分（2026-10-29 Step 8）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use crate::commands::{FormatDocument, SaveDocument};
use crate::model::EditorMode;
use crate::view::completion;
use crate::view::highlight;

impl EditorHostPanel {
    pub fn sync_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let read_only = self.editor_read_only();
        // 语义着色类型：模式切换后按新模式与文档扩展名重算（脚本分派 / CSV 不上色）
        let highlight_kind =
            self.with_document(|doc| highlight::kind_for_document(doc.path(), doc.mode()))
                .unwrap_or(highlight::HighlightKind::None);

        // 【LSP】模式切换时重新向宿主要服务（插件加载 / 宿主判定变了能接上；没变 = 无操作）
        self.sync_lsp_session(cx);

        self.editor.update(cx, |state, cx| {
            state.set_readonly(read_only, cx);
            if highlight::is_enabled(mode) {
                highlight::install(state, cx, highlight_kind);
            } else {
                // 未启用的模式（当前只剩分析）：摘掉着色器，不再取新 token。内核的
                // token 缓存没有公开的清入口，摘除后旧 token 要等下一次文本变更才失效。
                state.lsp_mut().semantic_tokens_provider = None;
            }
            // B9：补全按能力表装 / 摘（文本模式不接，分析模式留 1c 逐单元）
            self.sync_completion_provider(state);
            // B17：折叠按能力表与档位装 / 关（分析模式能力位为 false → 真关）
            sync_fold(&self.shared, &self.document, state, window, cx);
        });
        cx.notify();
        // 模式变了 = 语言 / 环境 / 能力徽标全变：通知宿主刷新全局段
        cx.emit(EditorHostEvent::StatusChanged);
    }

    /// `Ctrl+S`：保存；未命名 / 写盘失败 → 状态栏给出原因
    pub(crate) fn on_save(
        &mut self,
        _: &SaveDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_save：保存文档（Ctrl+S）");
        // 【格式化增强】保存时自动格式化（设置开）：先格式化再落盘——内核 Change 事件
        // 会把新文本写回服务层，`save` 拿到的就是格式化后的内容。
        // 格式化内部的提示消息会被下面的成功 / 失败分支覆盖，不打扰保存反馈。
        if self.format_on_save_enabled(cx) {
            self.format_document(window, cx);
        }
        match self.save(cx) {
            Ok(_) => {
                // A12：保存时顺便把会话（光标/选区/模式）落库——用户按 Ctrl+S 是最自然的时机
                self.save_session_now(cx);
                self.set_message(None, cx);
            }
            Err(error) => self.set_message(Some(error.to_string()), cx),
        }
    }

    /// `Ctrl+Shift+F`：格式化（整篇 / 选区）
    ///
    /// 动作本身极薄：格式化是**文档级**动作，真伪都由 [`Self::format_document`] 判定
    /// （只读拒绝 / 文本模式拒绝 / 解析不了的语句原样保留）。
    pub(crate) fn on_format(
        &mut self,
        _: &FormatDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_format：格式化文档（Ctrl+Shift+F）");
        self.format_document(window, cx);
    }

    /// `Ctrl+Space`：手动请一次补全（B9 切片二）
    ///
    /// 打字触发那条路由内核管（`is_completion_trigger`）；这条是**用户明确要候选**——
    /// 所以候选与打字那条**同一份**（`view/completion.rs::items_at`），只是替换范围改由
    /// 我们算（光标前的标识符）。拒绝的理由要说出来（文本模式 / 只读 / 没候选）。
    pub(crate) fn on_trigger_completion(
        &mut self,
        _: &crate::commands::TriggerCompletion,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_trigger_completion：手动请求补全（Ctrl+Space）");
        self.trigger_completion(cx);
    }

    /// 手动补全的落地（动作与测试都走它）
    pub(crate) fn trigger_completion(&mut self, cx: &mut Context<Self>) {
        if !self.shared.completion_enabled(&self.document) {
            // 三档拒绝理由分开说：文本模式 / 只读 / 大文件档位——“按了没反应”是不允许的
            let reason = self
                .with_document(|doc| {
                    if doc.mode() != EditorMode::Sql {
                        Some("文本模式不解析 SQL（补全是 SQL 模式的能力）".to_string())
                    } else if !doc.read_only().can_edit() {
                        Some("文档只读，无法补全".to_string())
                    } else if doc.tier().disables_completion() {
                        Some("文件太大（超大文件档位关掉了补全）".to_string())
                    } else {
                        None
                    }
                })
                .flatten()
                .unwrap_or_else(|| "当前不可补全".to_string());
            self.set_message(Some(reason), cx);
            return;
        }
        let (text, offset) = {
            let state = self.editor.read(cx);
            (state.value().to_string(), state.cursor())
        };
        let (start, query, items) =
            completion::items_at(&self.shared, &self.document, &text, offset);
        if items.is_empty() {
            self.set_message(
                Some("没有可补的候选（元数据可能还没载好，或光标处认不出上下文）".to_string()),
                cx,
            );
            return;
        }
        self.editor.update(cx, |state, cx| {
            state.present_completion_items(start, query, items, cx);
        });
        self.set_message(None, cx);
    }

}
