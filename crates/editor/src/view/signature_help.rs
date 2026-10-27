//! SQL 签名帮助 Dialog（#54 C4 上层接线）
//!
//! ## 为什么自建
//!
//! 内核 LSP 槽位里**没有** SignatureProvider（`input/editor/lsp/` 下 8 个模块逐数过：
//! code_actions / completions / definitions / document_colors / hover / overlay /
//! semantic_tokens，无签名槽位）——签名帮助只能宿主自建。
//!
//! ## 方案（首版，见 `sql-semantic-remaining-design.md` §二）
//!
//! - 触发：`Ctrl+Shift+Space` 手动（命令 [`crate::commands::SignatureHelp`]）；
//!   「输入 `(` 自动弹」留二期（要监听输入事件 + 去抖，避免打字时闪）
//! - 识别：纯函数 [`current_call`] 找「光标前的函数调用」（标识符 + 紧跟 `(`，
//!   数光标前括号内的逗号 → 当前参数位）
//! - 数据：engine 内置签名目录 [`builtin_signatures`]（约 50 个常用函数；在线系统表
//!   查询 DBeaver 式完整目录留给后续）。**不编造**：查不到的名字返回 None
//! - UI：独立 Dialog（复用 `open_dialog` 基建），标题 = 函数签名，正文 = 参数表 +
//!   当前参数高亮 + 中文描述。不做跟随光标的 Popover（要光标像素 + 滚动跟随，复杂度高）

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{ActiveTheme, WindowExt as _};
use gpui_kit::*;

use crate::ui;
use engine::sql::{FunctionSignature, ParamInfo, builtin_signatures};

/// 签名帮助视图数据（纯数据，便于单测）
pub struct SignatureView {
    /// 函数名（原文大小写）
    pub name: String,
    /// 格式化签名：`substr(x, start[, length])`
    pub signature: String,
    pub params: Vec<ParamInfo>,
    /// 当前参数位（0 基；括号后无逗号 = 0）
    pub active: usize,
    pub description: Option<String>,
}

/// 识别 `offset` 处的函数调用 → 签名（内置目录）
///
/// 规则（对齐 VSCode 签名帮助的「最内层调用」语义）：从光标往前找**最近** `(` 作为
/// 调用括号；括号前（跳过空白）必须是标识符 → 函数名；数括号内光标前的逗号 →
/// 当前参数位（嵌套括号内的逗号不计数）；名字查 [`builtin_signatures`]（不区分
/// 大小写）。查不到 / 不是调用 → `None`（不编造签名）。
///
/// 字符串 / 注释排除（A 方案，2026-10-27）：找到的调用括号如果落在字符串或
/// 注释内（`engine::sql::highlight::context_kind_at` 判定，与高亮同一词法器），
/// 不识别为调用——修复首版「`SELECT 'substr('` 误弹」限制。判定括号而非光标，
/// 覆盖「光标在字符串闭合引号后、最近 `(` 在字符串内」的位置。
pub fn current_call(sql: &str, offset: usize) -> Option<SignatureView> {
    use engine::sql::ContextKind;

    let offset = sql.floor_char_boundary(offset.min(sql.len()));
    let head = &sql[..offset];

    // 1) 从光标往前找最近 `(`（函数调用括号）
    let open_at = head.rfind('(')?;
    // 1b) 调用括号若在字符串 / 注释内，不是调用（词法判定与高亮同源）
    if engine::sql::context_kind_at(sql, open_at) != ContextKind::Code {
        return None;
    }
    // 2) 括号前（跳过空白）必须是标识符 → 函数名（`substr (` 带空格也认）
    let before = &head[..open_at];
    let name_start = before
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .map(|i| {
            i + before[i..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let name: String = before[name_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return None;
    }

    // 3) 数 open_at..offset 里逗号（深度 1 内：嵌套括号内不算）
    let mut depth = 1usize;
    let mut active = 0usize;
    for c in sql[open_at + 1..offset].chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            ',' if depth == 1 => active += 1,
            _ => {}
        }
    }

    // 4) 查内置目录（大小写不敏感）；查不到 = 不编造
    let signatures = builtin_signatures();
    let sig = signatures
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(&name))?;
    let view = signature_view(sig, active);
    Some(view)
}

/// 「输入 `(` 自动弹」判定（#54-C4 二期）：光标前一字符是 `(`（刚输入左括号，
/// 或光标停在调用括号后）且 [`current_call`] 识别出函数调用 → 弹。
///
/// 为什么不需要文本 diff / 去抖：打字中光标紧随参数 / 逗号 / 函数名，前导字符
/// 不是 `(`，判定天然不触发——「刚输入 `(`」与「在调用内继续输入」可由
/// 光标前一字符直接区分。
pub fn auto_call(sql: &str, offset: usize) -> Option<SignatureView> {
    let offset = sql.floor_char_boundary(offset.min(sql.len()));
    if offset == 0 {
        return None;
    }
    if sql[..offset].chars().last() != Some('(') {
        return None;
    }
    current_call(sql, offset)
}

/// FunctionSignature → SignatureView（格式化签名 + 当前参数位）
fn signature_view(sig: &FunctionSignature, active: usize) -> SignatureView {
    let params: Vec<ParamInfo> = sig.params.clone();
    let mut rendered = String::from(&sig.name);
    rendered.push('(');
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            rendered.push_str(", ");
        }
        match (&p.name, &p.type_name) {
            (Some(n), Some(t)) => rendered.push_str(&format!("{n}: {t}")),
            (Some(n), None) => rendered.push_str(n),
            (None, Some(t)) => rendered.push_str(t),
            (None, None) => rendered.push_str("?"),
        }
        if let Some(d) = &p.default {
            rendered.push_str(&format!(" = {d}"));
        }
    }
    rendered.push(')');
    SignatureView {
        name: sig.name.clone(),
        signature: rendered,
        params,
        active,
        description: sig.description.clone(),
    }
}

/// 打开签名帮助对话框（宿主在命令处理里调：读文档文本 + 光标 → [`current_call`]）
pub fn open_signature_help(
    window: &mut Window,
    cx: &mut App,
    view: SignatureView,
) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        // 参数表：当前参数位用主色高亮（VSCode 签名帮助同款口径），其余 muted
        let body = div()
            .v_flex()
            .w_full()
            .gap_1()
            .py_1()
            .child(SharedString::from(view.signature.clone()))
            .children(view.params.iter().enumerate().map(|(i, p)| {
                let (label, color) = if i == view.active {
                    (format!("参数 {}：", i + 1), theme.colors.primary)
                } else {
                    (format!("参数 {}：", i + 1), theme.colors.muted_foreground)
                };
                let detail = match (&p.name, &p.type_name, &p.default) {
                    (Some(n), Some(t), Some(d)) => format!("{n}（{t}，默认 {d}）"),
                    (Some(n), Some(t), None) => format!("{n}（{t}）"),
                    (Some(n), None, None) => n.clone(),
                    (None, Some(t), None) => t.clone(),
                    _ => String::new(),
                };
                div()
                    .text_xs()
                    .text_color(color)
                    .child(format!("{label}{detail}"))
            }))
            .into_any_element();
        let body = if let Some(desc) = &view.description {
            div()
                .v_flex()
                .w_full()
                .gap_2()
                .child(body)
                .child(div().text_xs().text_color(theme.colors.muted_foreground).child(desc.clone()))
                .into_any_element()
        } else {
            body
        };
        dialog
            .title("签名帮助")
            .w(theme.font_size * ui::SIGNATURE_HELP_DIALOG_WIDTH)
            .child(body)
            .footer(div().child("Esc 关闭 · 再次 Ctrl+Shift+Space 刷新"))
    });
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{auto_call, current_call};

    #[test]
    fn identifies_call_and_active_parameter() {
        // 光标在 `substr(a, b` 的 b 后：第 2 个参数（active=1）；名字/签名 = 目录标准名
        let view = current_call("SELECT substr(a, b", 19).expect("识别出调用");
        assert_eq!(view.name, "SUBSTR");
        assert_eq!(view.active, 1);
        assert_eq!(view.signature, "SUBSTR(str, start, length)");
        assert_eq!(view.description.as_deref(), Some("子串"));
    }

    #[test]
    fn no_call_when_no_open_paren() {
        assert!(current_call("SELECT substr", 13).is_none(), "没 ( 就不是调用");
        assert!(current_call("SELECT 1", 8).is_none(), "不是标识符调用");
    }

    #[test]
    fn nested_parens_identify_innermost_call() {
        // 光标在 `concat(a, substr(b, c` 的 c 后：按 VSCode「最内层调用」语义，
        // 识别的是 **SUBSTR**（最近一个 `(`），内层括号的逗号计为 substr 的参数位
        let view = current_call("SELECT concat(a, substr(b, c", 27).expect("识别出最内层调用");
        assert_eq!(view.name, "SUBSTR");
        assert_eq!(view.active, 1);
    }

    #[test]
    fn commas_outside_inner_parens_count_for_outer() {
        // 光标在 `concat(a, b` 的 b 后（无嵌套）：识别 CONCAT，外层第 2 参数
        let view = current_call("SELECT concat(a, b", 17).expect("识别出 concat");
        assert_eq!(view.name, "CONCAT");
        assert_eq!(view.active, 1);
    }

    #[test]
    fn unknown_function_yields_none() {
        // 不编造：内置目录查不到的名字不产签名
        assert!(current_call("SELECT no_such_fn(a", 22).is_none());
    }

    #[test]
    fn auto_call_triggers_only_right_after_open_paren() {
        // 输入 `(` 后光标紧随其后：识别出调用
        let view = auto_call("SELECT substr(", 14).expect("刚输入 ( 应触发");
        assert_eq!(view.name, "SUBSTR");
        assert_eq!(view.active, 0);
        // 继续输入参数：光标前一字符不是 `(`，不再触发
        assert!(auto_call("SELECT substr(x", 15).is_none(), "参数输入中不触发");
        assert!(auto_call("SELECT substr(x,", 16).is_none(), "逗号输入中不触发");
        // 光标在开头 / 空文本：不触发
        assert!(auto_call("", 0).is_none());
        assert!(auto_call("SELECT 1", 8).is_none(), "前导字符不是 ( 不触发");
        // 字符串 / 注释内的调用括号已排除（A 方案：词法判定，修复首版误弹）
        // "SELECT 'substr('" —— 光标 16 在闭合引号后，最近 ( 在字符串内
        assert!(current_call("SELECT 'substr('", 16).is_none(), "字符串内的 ( 不识别");
        // "SELECT -- substr(\n1" —— 最近 ( 在行注释内（光标 18 在 1 后）
        assert!(current_call("SELECT -- substr(\n1", 18).is_none(), "注释内的 ( 不识别");
    }
}
