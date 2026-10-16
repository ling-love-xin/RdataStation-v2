//! RdataStation Mermaid 渲染插件（wasm / Extism PDK）。
//!
//! 导出函数 `render`：输入为 **mermaid 源文本** 或 **JSON 信封**（见 [`envelope`]），
//! 输出为 UTF-8 SVG 文本。纯计算，不申请任何 host function / 系统能力。
//!
//! 契约（宿主侧 `render_document` 第一刀已对齐）：
//! - 纯文本输入：`mermaid source`（主题走源内 `%%{init}%%` 指令，mermaid.js 语义）
//! - 信封输入：`{"source": "...", "theme": {"preset": "modern|mermaid_default|dark|forest|neutral",
//!   "overrides": {"字段名": 值}}}`——preset 缺省 `modern`；overrides 的键是
//!   `mermaid_rs_renderer::theme::Theme` 的 pub 字段名（如 `font_family` / `primary_color`），
//!   覆盖 preset（未知字段忽略，与 serde 默认行为一致）。
//! - 非法 JSON 或缺 `source` 字段 → 按纯文本处理（第一刀行为不变）。
//!
//! 字体：wasm32 无系统字体，插件在首次渲染时把随包嵌入的 DejaVu Sans 子集注入
//! vendor 补丁的 `register_font`；上游默认字体栈已含 `"DejaVu Sans"`，query 自动命中。

mod envelope;
mod fonts;

/// 诊断用 host 函数：把插件侧 panic message 送回宿主（panic 定位专用）。
/// wasm32-unknown-unknown 的 panic 是 abort（unreachable trap），默认 message 丢失；
/// 装自定义 hook 后 message 经此函数透出。生产构建可保留（宿主忽略即可）。
#[cfg(target_arch = "wasm32")]
#[::extism_pdk::host_fn("extism:host/user")]
extern "ExtismHost" {
    fn host_log(msg: String);
}

/// 安装 panic hook：panic message → host_log（仅 wasm；native 测试用标准输出）。
fn install_panic_hook() {
    #[cfg(target_arch = "wasm32")]
    std::panic::set_hook(Box::new(|info| {
        let _ = unsafe { host_log(info.to_string()) };
    }));
}

/// Extism 导出入口：mermaid 源文本或 JSON 信封 → SVG（UTF-8）。
///
/// 仅 wasm 目标编译：`plugin_fn` 展开会引用 PDK 的 wasm 导入符号
/// （load_input / store 等），native 测试二进制链接不到，故 cfg 隔离。
#[cfg(target_arch = "wasm32")]
#[::extism_pdk::plugin_fn]
pub fn render(input: String) -> ::extism_pdk::FnResult<String> {
    install_panic_hook();
    // render_inner 的 Err(String) 显式转 PDK 错误（Error = anyhow::Error，msg 构造）
    match render_inner(&input) {
        Ok(svg) => Ok(svg),
        Err(e) => Err(::extism_pdk::WithReturnCode::from(
            ::extism_pdk::Error::msg(e),
        )),
    }
}

/// 渲染纯函数（不依赖 PDK 的 wasm 导入符号，native 单测可直接调用）。
pub fn render_inner(input: &str) -> Result<String, String> {
    fonts::ensure_fonts();
    let (source, options) = envelope::parse(input);
    mermaid_rs_renderer::render_with_options(&source, options).map_err(|e| e.to_string())
}

// —— native 回归：wasm 上 panic 先在此复现（cdylib 单测需 rlib）——
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_render_flowchart() {
        let svg = render_inner("flowchart LR\n  A --> B\n").unwrap();
        assert!(svg.contains("<svg"), "svg 未产出: {svg}");
    }

    #[test]
    fn native_render_sequence() {
        let svg = render_inner("sequenceDiagram\n  Alice->>Bob: Hello\n").unwrap();
        assert!(svg.contains("<svg"), "svg 未产出: {svg}");
    }
}
