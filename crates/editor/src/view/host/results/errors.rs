//! 错误标记与跳转（apply_error_marks / jump_to_error_site / reveal_search_target）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use gpui_kit::base::input::{Diagnostic, DiagnosticSeverity, Position};
use crate::diagnostics;

impl EditorHostPanel {
    /// 诊断。三源必须走同一条重建流程：内核的诊断集合只有 `clear` + `push`，分开各写一份
    /// 就会互相擦掉（“执行出错了，词法诊断没了”那种抳不到的怪象）。
    pub(crate) fn apply_error_marks(&mut self, reason: Option<String>, cx: &mut Context<Self>) {
        self.error_reason = reason;
        let site = self.error_site.clone();
        let reason = self.error_reason.clone();
        let lexical = self.scan_diagnostics.clone();
        let lsp_diags = self.lsp_diagnostics.clone();
        self.editor.update(cx, |state, cx| {
            // 文本先取：`diagnostics_mut` 借的就是同一份状态，两个借用不能同时活着
            let text = state.value().to_string();
            let Some(diagnostics) = state.diagnostics_mut() else {
                return;
            };
            diagnostics.clear();
            // 打字期诊断（词法层能证明的那些；来源标“语法”以与执行错误区分）
            for one in &lexical {
                let ((start_line, start_column), (end_line, end_column)) = one.positions(&text);
                diagnostics.push(
                    Diagnostic::new(
                        Position::new(start_line as u32, start_column as u32)
                            ..Position::new(end_line as u32, end_column as u32),
                        one.message.clone(),
                    )
                    .with_severity(DiagnosticSeverity::Error)
                    .with_source("语法")
                    .with_code(one.code),
                );
            }
            // LSP 语言服务诊断（内核有 `From<lsp_types::Diagnostic>`，source 一并带过来）
            for one in &lsp_diags {
                diagnostics.push(Diagnostic::from(one.clone()));
            }
            if let (Some(site), Some(reason)) = (site, reason) {
                let (end_line, end_column) = site.end();
                let range = Position::new(
                    site.line.saturating_sub(1) as u32,
                    site.column.saturating_sub(1) as u32,
                )
                    ..Position::new(
                        end_line.saturating_sub(1) as u32,
                        end_column.saturating_sub(1) as u32,
                    );
                diagnostics.push(
                    Diagnostic::new(range, reason)
                        .with_severity(DiagnosticSeverity::Error)
                        .with_source("执行"),
                );
            }
            cx.notify();
        });
    }

    /// 【B6】把光标送到出错处并聚焦
    ///
    /// 回填发生在没有窗口的轮询里，所以借构造时存下的窗口句柄。窗口已经没了、或者这次更新
    /// 落在另一个窗口更新里面（headless 测试就是这样）时，这一步只是没做成——不 panic：
    /// 位置已经画在诊断与状态栏里了。
    pub(crate) fn jump_to_error_site(&mut self, cx: &mut Context<Self>) {
        let Some(site) = self.error_site.clone() else {
            return;
        };
        let range = site.range();
        let editor = self.editor.clone();
        let _ = self.window.update(cx, move |_view, window, app| {
            editor.update(app, |state, cx| {
                state.set_selected_range(range, cx);
                state.focus(window, cx);
            });
        });
    }

    /// 跳转的**落地入口**：手上有窗口时直接做（错误卡片上那个按钮，以及测试）
    ///
    /// 真机的自动跳转走上面的窗口句柄路径（回填发生在没有窗口的后台轮询里）；这条入口是
    /// 按下去就有窗口的情况，也是测试能驱动的那一半（与对话框“真点击”同一口径，架构 §12 #29）。
    #[cfg_attr(not(test), allow(dead_code))] // 生产错误卡按钮走队列 LocateError→jump_to_error_site；本入口测试直接调
    pub(crate) fn jump_to_error_site_with(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(site) = self.error_site.clone() else {
            return;
        };
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(site.range(), cx);
            state.focus(window, cx);
        });
    }

    /// M5 命中跳行：把光标/选区移到 `line`（1 基行号）行，并聚焦面板。
    ///
    /// `span`（行内命中字节区间，来自草稿箱内容搜索）有值时**选中命中文本**；
    /// 无值时光标落在行首。行号越界 / 区间非法时静默退化为「光标放行首」，再不行就只聚焦。
    /// 与 `jump_to_error_site_with` 同口径（`set_selected_range` 会滚到可见处）。
    pub fn reveal_search_target(
        &mut self,
        line: u32,
        span: Option<(usize, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let full = self.editor.read(cx).text().to_string();
        let Some(line_start) = diagnostics::offset_for_line_col(&full, line as usize, 1) else {
            return;
        };
        let range = match span {
            Some((s, e)) if s < e => line_start + s..line_start + e,
            _ => line_start..line_start,
        };
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            state.focus(window, cx);
        });
    }

}
