//! 内核端口契约（#20）：分析模式「内核执行源」的宿主接口
//!
//! ## 一句话
//!
//! editor 不探测内核：`[`KernelsPort`]` 回答「本机有哪些内核可用」（注册表快照），
//! 宿主（workbench）在启动时把 `rds_kernel::detect_kernels()` 的结果注进来。
//! editor 侧只做两件事：
//!
//! 1. **渲染**：菜单 / 指示器按快照列出（ready 可选、未 ready 置灰 + 原因）；
//! 2. **执行前解析**：`[`resolve_kernel`]` 纯函数把「单元语言 + 执行源 + 会话默认」
//!    解析成一个可启动的内核（显式指定 > 上次用过的 > 注册表第一个可用）。
//!
//! ## 为什么默认是「上次用过的内核」
//!
//! 用户拍板：默认执行源 = 当前会话 / 上次用过的，可手动换。`Session.default_kernel`
//! 就是这个记忆（随 `.rdsnote` 落盘跟随文件）；执行成功后回写，菜单「默认（上次用过的）」
//! 始终指向它。

use crate::cell::{CellLanguage, ExecutionSource};

/// 内核注册表端口（宿主注入；未注入 = 空列表，菜单如实置灰）
pub trait KernelsPort {
    /// 可用内核快照（渲染路径可调：实现必须是内存读，不做 I/O / 探测）
    fn list(&self) -> Vec<rds_kernel::KernelSpec>;
}

/// 单元语言 → 内核语言（可执行内核类单元；SQL / Markdown 不在内核档）
pub fn kernel_language_of(language: CellLanguage) -> Option<rds_kernel::KernelLanguage> {
    match language {
        CellLanguage::Python => Some(rds_kernel::KernelLanguage::Python),
        CellLanguage::R => Some(rds_kernel::KernelLanguage::R),
        CellLanguage::Julia => Some(rds_kernel::KernelLanguage::Julia),
        CellLanguage::Sql | CellLanguage::Markdown => None,
    }
}

/// 解析一个内核类单元的**实际执行内核**（纯函数；不改状态、不做 I/O）
///
/// 优先级（与执行源菜单的选项一一对应）：
/// 1. 显式指定 `ExecutionSource::Kernel(id)`：注册表按 id 找；找不到 / 未 ready → Err（给原因）
/// 2. 会话默认（`session_default` 是上次用过、且属于该语言）：直接用（未 ready → Err）
/// 3. 注册表**第一个 ready 的同语言内核**（venv 优先，注册表顺序保证）
/// 4. 一个都没有 → Err（“本机没有可用的 {语言} 内核”，附注册表里未 ready 的原因）
pub fn resolve_kernel(
    specs: &[rds_kernel::KernelSpec],
    source: Option<&ExecutionSource>,
    session_default: Option<&str>,
    language: CellLanguage,
) -> Result<rds_kernel::KernelSpec, String> {
    let Some(kernel_language) = kernel_language_of(language) else {
        return Err(format!("{} 单元不跑内核", language.label()));
    };
    let ready = |spec: &rds_kernel::KernelSpec| {
        spec.language == kernel_language && spec.ready
    };

    // 1. 显式指定：id 精确匹配
    if let Some(ExecutionSource::Kernel(id)) = source {
        let Some(spec) = specs.iter().find(|spec| &spec.id == id) else {
            return Err(format!("内核「{id}」不在注册表里（重启后可能已失效）"));
        };
        if !spec.ready {
            return Err(format!(
                "内核「{}」不可用：{}",
                spec.display_name,
                spec.note.unwrap_or("未就绪")
            ));
        }
        return Ok(spec.clone());
    }

    // 2. 会话默认（上次用过的、且语言匹配）
    if let Some(default_id) = session_default {
        if let Some(spec) = specs.iter().find(|spec| spec.id == default_id) {
            if ready(spec) {
                return Ok(spec.clone());
            }
            // 默认内核失效（被卸载 / venv 挪走）→ 落到第 3 步，不硬报错（用户没显式指定）
        }
    }

    // 3. 注册表第一个 ready 的同语言内核
    specs
        .iter()
        .find(|spec| ready(spec))
        .cloned()
        .ok_or_else(|| {
            let notes: Vec<String> = specs
                .iter()
                .filter(|spec| spec.language == kernel_language)
                .map(|spec| {
                    spec.note
                        .map(|n| format!("{}：{n}", spec.display_name))
                        .unwrap_or_else(|| spec.display_name.clone())
                })
                .collect();
            let lang_label = kernel_language.label();
            let hint = if notes.is_empty() {
                format!("本机没有检测到 {lang_label} 解释器")
            } else {
                format!("本机没有可用的 {lang_label} 内核：{}", notes.join("；"))
            };
            hint
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, language: rds_kernel::KernelLanguage, ready: bool) -> rds_kernel::KernelSpec {
        rds_kernel::KernelSpec {
            id: id.to_string(),
            display_name: id.to_string(),
            language,
            command: id.to_string(),
            ready,
            note: if ready { None } else { Some("未装内核包") },
        }
    }

    /// 注册表顺序 = 优先序：第一个 ready 同语言内核胜出（venv 在系统 python 前）
    #[test]
    fn first_ready_kernel_wins_for_the_language() {
        let specs = vec![
            spec("python-venv", rds_kernel::KernelLanguage::Python, false),
            spec("python", rds_kernel::KernelLanguage::Python, true),
        ];
        let resolved = resolve_kernel(&specs, None, None, CellLanguage::Python).unwrap();
        assert_eq!(resolved.id, "python");
    }

    /// 显式指定的 id 精确匹配（不受顺序影响）
    #[test]
    fn explicit_kernel_id_wins() {
        let specs = vec![
            spec("python", rds_kernel::KernelLanguage::Python, true),
            spec("r", rds_kernel::KernelLanguage::R, false),
        ];
        let resolved = resolve_kernel(
            &specs,
            Some(&ExecutionSource::Kernel("r".to_string())),
            None,
            CellLanguage::R,
        )
        .unwrap_err();
        assert!(resolved.contains("不可用"), "{resolved}");
    }

    /// 会话默认（上次用过的）优先于注册表顺序；失效则静默落到第一个可用
    #[test]
    fn session_default_takes_precedence_and_falls_back_when_stale() {
        let specs = vec![
            spec("python-venv", rds_kernel::KernelLanguage::Python, true),
            spec("python", rds_kernel::KernelLanguage::Python, true),
        ];
        // 上次用过系统 python → 这次还用系统 python（venv 排在前面也不抢）
        let resolved = resolve_kernel(&specs, None, Some("python"), CellLanguage::Python).unwrap();
        assert_eq!(resolved.id, "python");
        // 默认内核被卸载 → 回退到第一个可用，不硬报错
        let stale = vec![
            spec("python-venv", rds_kernel::KernelLanguage::Python, true),
            spec("python", rds_kernel::KernelLanguage::Python, false),
        ];
        let resolved = resolve_kernel(&stale, None, Some("python"), CellLanguage::Python).unwrap();
        assert_eq!(resolved.id, "python-venv");
    }

    /// 一个都没有 → 错误信息带上每个未 ready 内核的原因（用户知道装什么）
    #[test]
    fn no_ready_kernel_reports_each_reason() {
        let specs = vec![
            spec("python", rds_kernel::KernelLanguage::Python, false),
            spec("r", rds_kernel::KernelLanguage::R, false),
        ];
        let err = resolve_kernel(&specs, None, None, CellLanguage::Python).unwrap_err();
        assert!(err.contains("Python"), "{err}");
        assert!(err.contains("未装内核包"), "{err}");
    }

    /// SQL / Markdown 不是内核类单元（调用方不该走到这）
    #[test]
    fn non_kernel_languages_are_rejected() {
        let err = resolve_kernel(&[], None, None, CellLanguage::Sql).unwrap_err();
        assert!(err.contains("不跑内核"), "{err}");
    }
}
