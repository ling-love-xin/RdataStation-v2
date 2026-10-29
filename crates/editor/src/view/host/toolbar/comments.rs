//! 注释 / 文件插入 / 签名与函数手册（on_toggle_comment / insert_file_contents / open_function_manual）
//!
//! 自 host/toolbar.rs 拆分（2026-10-29 Step 8）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use crate::commands::ToggleComment;
use crate::edit;

impl EditorHostPanel {
    pub(crate) fn on_toggle_comment(
        &mut self,
        _: &ToggleComment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("文档只读，无法注释".to_string()), cx);
            return;
        }
        let Some(mode) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let prefix = edit::comment_prefix(mode);
        let (text, selection) = {
            let state = self.editor.read(cx);
            (state.value().to_string(), state.selected_range())
        };

        let merged = edit::toggle_line_comment(&text, selection, prefix);
        if merged.text == text {
            // 全是空行：没有可注释的内容（不报错，也不留提示）
            return;
        }

        let new_text = merged.text.clone();
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(merged.replaced.clone(), cx);
            state.replace(merged.block.clone(), window, cx);
            state.set_selected_range(merged.selection.clone(), cx);
        });
        // 程序化写入不保证发 `Change` 事件，这里主动同步服务层（同一值重复写入是幂等的）
        self.statements = count_statements(&new_text);
        self.shared
            .update(|service| service.set_content(&self.document, new_text));
        self.set_message(None, cx);
    }

    /// 把某个文件的内容插到**光标处**（拖放落点；原型 §4.5）。
    ///
    /// 只读文档直接拒绝；读盘失败 / 空文件都给一条消息，不静默。
    pub(crate) fn insert_file_contents(
        &mut self,
        payload: &::shared::InsertFileDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("文档只读，无法插入内容".to_string()), cx);
            return;
        }
        let text = match crate::persist::load(&payload.path) {
            Ok(text) => text,
            Err(error) => {
                self.set_message(Some(format!("插入失败：{error}")), cx);
                return;
            }
        };
        if text.is_empty() {
            self.set_message(
                Some(format!("{} 是空文件，没有可插入的内容", payload.label)),
                cx,
            );
            return;
        }
        // 在光标处插入：`replace` 替换当前选区，并把光标留在插入内容之后。
        self.editor
            .update(cx, |state, cx| state.replace(text, window, cx));
        // 程序化写入不保证发 `Change` 事件 → 主动同步服务层（与行注释同一口径，幂等）。
        let new_text = self.editor.read(cx).value().to_string();
        self.statements = count_statements(&new_text);
        self.shared
            .update(|service| service.set_content(&self.document, new_text));
        self.set_message(Some(format!("已插入 {}", payload.label)), cx);
    }

    // ===== 函数手册（#54-C5）=====
    //
    // Quick Open 式中文语义搜索（输入「截取」→ SUBSTR），确认后把函数名插到光标处。
    // 数据与排序全在 engine（157 条 / 9 类静态手册），editor 只做弹层与插入。

    /// `Ctrl+Shift+Space`：签名帮助（识别光标前函数调用 → 内置签名目录 → Dialog；
    /// 识别不出 / 查不到 = 静默，不打扰）
    pub(crate) fn on_signature_help(
        &mut self,
        _: &crate::commands::SignatureHelp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_signature_help：签名帮助（Ctrl+Shift+Space）");
        let text = self.editor.read(cx).value().to_string();
        // 活动光标在前（多光标取第一个即可——签名帮助是"看当前调用"的辅助）
        let cursor = self.editor.read(cx).cursor_offsets().first().copied().unwrap_or(0);
        if let Some(view) = crate::view::signature_help::current_call(&text, cursor) {
            crate::view::signature_help::open_signature_help(window, cx, view);
        }
    }
    pub(crate) fn on_function_manual(
        &mut self,
        _: &crate::commands::FunctionManual,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_function_manual：打开函数手册对话框");
        _ = self.open_function_manual(window, cx);
    }

    /// 打开函数手册对话框（返回 ListState 实体，测试可直接驱动搜索/确认）
    ///
    /// 确认回调捕获宿主实体，把函数名交给 [`Self::insert_manual_function`]。
    pub(crate) fn open_function_manual(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<gpui_kit::component::list::ListState<
        crate::view::function_manual::ManualDelegate,
    >> {
        let entity = cx.entity();
        crate::view::function_manual::open_function_manual(
            window,
            cx,
            move |name, window, cx| {
                entity.update(cx, |panel, cx| {
                    panel.insert_manual_function(name, window, cx);
                });
            },
        )
    }

    /// 手册确认 → 光标处插入函数名 + 同步服务层（同「插入片段」口径，幂等）
    pub(crate) fn insert_manual_function(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |state, cx| state.insert(name.clone(), window, cx));
        let new_text = self.editor.read(cx).value().to_string();
        self.statements = count_statements(&new_text);
        self.shared
            .update(|service| service.set_content(&self.document, new_text));
        self.set_message(Some(format!("已插入函数 {}", name)), cx);
    }

    // ===== 模式切换（A5 的矩阵 + A9 的确认对话框）=====
    //
    // 入口是工具栏最左的**模式指示器**（原型 §2.2）：点击选目标模式，代价由 `mode::plan_switch`
    // 判定，需确认的先弹对话框；确认后本面板落地（写内容 + 改模式 + 刷新着色/只读）。
    // **禁止静默切换**：任何需要付出代价的切换都不过状态（原型 §1.3）。
}
