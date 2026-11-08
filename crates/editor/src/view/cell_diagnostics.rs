//! 分析模式逐单元的语义诊断（§15.14）
//!
//! ## 为什么必须逐单元
//!
//! 笔记模式下主编辑器的正文是**单元列表**（`.rdsnote` 的 JSON 结构），主 state 上的
//! 诊断扫的是那段 JSON——对某个单元的 SQL 毫无意义。能力表对分析模式给 `diagnostics: true`，
//! 落地点只能是**单元自己的内核**（与逐单元补全 `view::completion::install` 同一结论）。
//!
//! ## 口径与主编辑器同源
//!
//! 只用两条已验证的纯函数入口，不另造一套判据：
//! - [`language_service::diagnostics`]：词法层（未闭合字符串/注释/美元引用、括号不平衡）
//!   + 语义层（未知表/列，需要目录快照；目录为空自动跳过，与无目录退化同一口径）
//! - 目录快照来自宿主推给本文档的那一份（`EditorShared::catalog_snapshot_for`），
//!   与补全、主编辑器诊断同一来源
//!
//! ## 为什么不在 Change 回调里直接改内核
//!
//! Change 事件由该单元编辑器自己发出，此刻它**正在 update 中**；回调里再 `update` 同一
//! 实体会 panic（`cannot update … while it is already being updated`）。所以回调把
//! 「算好的诊断」推迟到当前更新结束后（`cx.defer`）再写进内核。

use gpui_kit::base::input::{Diagnostic as KernelDiagnostic, DiagnosticSeverity, EditorState, Position};

use crate::completion::Catalog;
use crate::diagnostics::ErrorSite;
use crate::language_service::{self, Diagnostic, Request};

/// 一次单元执行失败留下的位点（**面板持有**，不是内核状态）
///
/// 为什么要有它：单元执行错误与打字期诊断是两条来源，但内核的诊断集合只有
/// `clear` + `push`——两条来源各写各的就会互相擦掉（重跑成功后旧错误还挂着，
/// 或者一敲键执行错误就消失）。所以执行错误存成一条"记忆"，与词法诊断在
/// [`apply`] 里合并重建（与主编辑器 `apply_error_marks` 同一纪律）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecMark {
    /// 出错的位点（**文档坐标系**：1 基行列 + 0 基字节；由 `site_in_document` 定位）
    pub site: ErrorSite,
    /// 驱动给的原因原文（不加工，用户要能对上日志）
    pub reason: String,
}

/// 一段单元正文 → 诊断（**纯函数**：可离线断言，不碰内核、不碰 App）
///
/// `cursor` / `selection` 给 0 与 `None`：诊断的判据是整篇文本，不看光标（与后台扫描
/// `support::scan::run` 同一填法）。`channel` 用默认值——诊断只看文本与目录，不看通道。
pub fn diagnostics_for(
    text: &str,
    catalog: &Catalog,
    dialect: engine::sql::SqlDialect,
) -> Vec<Diagnostic> {
    language_service::diagnostics(&Request {
        text,
        cursor: 0,
        selection: None,
        catalog,
        connection: None,
        channel: Default::default(),
        dialect,
    })
}

/// 把诊断重建进单元内核（内核只有 `clear` + `push`，所以必须一处重建，别分开各写一份）
///
/// **三源**在这个入口合并：词法/语义（`found`，源「语法」）、执行错误（`exec`，源「执行」）、
/// 语言服务（`lsp`，源「语言服务」，来自 `textDocument/publishDiagnostics`）。三条来源要是
/// 各写一遍 `clear`，后写的会擦掉先写的。严重度：语法 / 执行统一 Error（与主编辑器
/// `apply_error_marks` 同一口径）；语言服务用服务端给的 severity，`None` 按 Warning（
/// 服务端不写 severity 时不该被当成错误，但也不该看不见）。
///
/// 【A1】`lsp` 的 range 直接用：单元是独立内核实例，服务端返回的就是该单元正文坐标，
/// 不存在「文档行号偏移」的换算（这一点此前被误判过）。
pub fn apply(
    state: &mut EditorState,
    found: &[Diagnostic],
    exec: Option<&ExecMark>,
    lsp: &[lsp_types::Diagnostic],
    cx: &mut gpui_kit::Context<EditorState>,
) {
    let text = state.value().to_string();
    let Some(diagnostics) = state.diagnostics_mut() else {
        return;
    };
    diagnostics.clear();
    for one in found {
        let ((start_line, start_column), (end_line, end_column)) = one.positions(&text);
        diagnostics.push(
            KernelDiagnostic::new(
                Position::new(start_line as u32, start_column as u32)
                    ..Position::new(end_line as u32, end_column as u32),
                one.message.clone(),
            )
            .with_severity(DiagnosticSeverity::Error)
            .with_source("语法")
            .with_code(one.code),
        );
    }
    // 【A1】语言服务：range 已是 0 基行列（LSP 口径），直接进内核
    for one in lsp {
        diagnostics.push(
            KernelDiagnostic::new(
                Position::new(one.range.start.line, one.range.start.character)
                    ..Position::new(one.range.end.line, one.range.end.character),
                one.message.clone(),
            )
            .with_severity(
                one.severity
                    .map(Into::into)
                    .unwrap_or(DiagnosticSeverity::Warning),
            )
            .with_source("语言服务"),
        );
    }
    if let Some(mark) = exec {
        // 位点是 1 基行列（给人看的口径），内核 `Position` 要 0 基
        let (end_line, end_column) = mark.site.end();
        diagnostics.push(
            KernelDiagnostic::new(
                Position::new(
                    mark.site.line.saturating_sub(1) as u32,
                    mark.site.column.saturating_sub(1) as u32,
                )
                    ..Position::new(
                        end_line.saturating_sub(1) as u32,
                        end_column.saturating_sub(1) as u32,
                    ),
                mark.reason.clone(),
            )
            .with_severity(DiagnosticSeverity::Error)
            .with_source("执行"),
        );
    }
    cx.notify();
}
