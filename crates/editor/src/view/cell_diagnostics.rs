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
use crate::language_service::{self, Diagnostic, Request};

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
/// 来源标「语法」、严重度统一 Error、`code` 原样带给内核——与主编辑器
/// （`view/host/results/errors.rs::apply_error_marks`）的词法层同一口径，
/// 同一个错在主编辑器与单元里长得一样。
pub fn apply(
    state: &mut EditorState,
    found: &[Diagnostic],
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
    cx.notify();
}
