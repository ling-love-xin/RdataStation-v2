//! 渲染计算服务（`render_document` 轨，第一刀）。
//!
//! 借鉴 dodo 的 `mermaid source → SVG`（`crates/dodo-mermaid` mermaid-rs-renderer）：
//! **渲染计算全部在 wasm 插件侧**（Extism 沙箱内），宿主只做「选渲染器 → 传源文本 →
//! 收产物」。与 SSH 的 sidecar 轨互补：一个管「纯计算」（source → 产物，无 I/O），
//! 一个管「持续连接」（shell / 端口转发，双向字节流）。
//!
//! ## 契约
//!
//! wasm 插件在 `manifest.capabilities.wasm.render_document` 里声明：
//!
//! ```toml
//! [capabilities.wasm]
//! entry = "render.wasm"
//! render_document = { function = "render", formats = ["mermaid"], output_format = "svg" }
//! ```
//!
//! 宿主调 `render_document` 时：按 `plugin_id` 定位插件 → 校验声明与格式 →
//! `ExtismPluginManager::call_plugin(id, function, source)` → 产物字节按 UTF-8 收。
//! **函数签名约定**：input = 源文本字节，output = 产物（SVG/HTML）字节。
//! 不需要 host function（纯计算），所以与 `allowed_host_functions` 正交。
//!
//! ## 第一刀边界
//!
//! - 多渲染器并存时宿主按「声明顺序第一个匹配」选（`renderer_for` 纯函数，可测）。
//! - 真实 wasm 资产（把 mermaid-rs-renderer 编成 wasm）属于 P4 真机验收；
//!   本模块单测覆盖**选择器与错误路径**，调用层走 Extism 现成 API（集成级再验）。

use crate::manager::{LoadedPlugin, PluginManager};
use crate::manifest::RenderDocumentSpec;

// ============================================================================
// 错误
// ============================================================================

/// 渲染计算失败。调用方（宿主 UI / 服务）按此给用户可读文案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// 没有任何已加载插件声明支持该格式的渲染。
    NoRenderer(String),
    /// 目标插件不存在（未加载 / 未激活）。
    PluginNotFound(String),
    /// 目标插件没声明 `capabilities.wasm.render_document`。
    NotDeclared(String),
    /// 插件声明了渲染，但格式不在它的 `formats` 列表里。
    FormatMismatch {
        plugin_id: String,
        format: String,
        declared: Vec<String>,
    },
    /// wasm 调用失败（Extism 层错误；插件已被标 Error）。
    CallFailed {
        plugin_id: String,
        function: String,
        detail: String,
    },
    /// 插件返回的不是 UTF-8 文本（产物应为 SVG / HTML 文本）。
    NonUtf8(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::NoRenderer(fmt) => write!(f, "没有支持「{fmt}」渲染的插件"),
            RenderError::PluginNotFound(id) => write!(f, "插件 {id} 未加载"),
            RenderError::NotDeclared(id) => {
                write!(f, "插件 {id} 未声明 capabilities.wasm.render_document")
            }
            RenderError::FormatMismatch {
                plugin_id,
                format,
                declared,
            } => write!(
                f,
                "插件 {plugin_id} 不支持「{format}」（声明：{}）",
                if declared.is_empty() {
                    "任意".to_string()
                } else {
                    declared.join(" / ")
                }
            ),
            RenderError::CallFailed {
                plugin_id,
                function,
                detail,
            } => write!(f, "插件 {plugin_id} 的 {function} 调用失败：{detail}"),
            RenderError::NonUtf8(id) => write!(f, "插件 {id} 返回的产物不是 UTF-8 文本"),
        }
    }
}

impl std::error::Error for RenderError {}

// ============================================================================
// 选择器（纯函数，单测直测）
// ============================================================================

/// 在已加载插件里挑一个能渲染 `format` 的渲染器（第一个匹配，声明顺序）。
///
/// 匹配条件：wasm 轨 + 声明了 `render_document` + `formats` 空（任意）或含 `format`。
/// 返回插件 id；无匹配返回 `None`。
pub fn renderer_for(plugins: &[LoadedPlugin], format: &str) -> Option<String> {
    plugins.iter().find_map(|p| {
        let spec = p
            .manifest
            .capabilities
            .wasm
            .as_ref()
            .and_then(|w| w.render_document.as_ref())?;
        let accepts = spec.formats.is_empty() || spec.formats.iter().any(|f| f == format);
        accepts.then(|| p.id.clone())
    })
}

// ============================================================================
// 服务入口
// ============================================================================

/// 指定插件渲染一份文档源文本。
///
/// `source` = 文档源（如 mermaid 文本）；`format` 与插件声明的 `formats` 匹配。
pub fn render_document(
    manager: &PluginManager,
    plugin_id: &str,
    source: &str,
    format: &str,
) -> Result<String, RenderError> {
    let spec = resolve_spec(manager, plugin_id, format)?;
    let bytes = manager
        .wasm_manager()
        .call_plugin(plugin_id, &spec.function, source.as_bytes())
        .map_err(|e| RenderError::CallFailed {
            plugin_id: plugin_id.to_string(),
            function: spec.function.clone(),
            detail: e.to_string(),
        })?;
    String::from_utf8(bytes).map_err(|_| RenderError::NonUtf8(plugin_id.to_string()))
}

/// 只解析契约、不调用（宿主可先用它预览「谁能渲染什么」）。
pub fn resolve_spec(
    manager: &PluginManager,
    plugin_id: &str,
    format: &str,
) -> Result<RenderDocumentSpec, RenderError> {
    let plugins = manager.list_loaded_plugins();
    let plugin = plugins
        .iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| RenderError::PluginNotFound(plugin_id.to_string()))?;
    let spec = plugin
        .manifest
        .capabilities
        .wasm
        .as_ref()
        .and_then(|w| w.render_document.as_ref())
        .ok_or_else(|| RenderError::NotDeclared(plugin_id.to_string()))?;
    let declared = spec.formats.clone();
    let accepts = declared.is_empty() || declared.iter().any(|f| f == format);
    if !accepts {
        return Err(RenderError::FormatMismatch {
            plugin_id: plugin_id.to_string(),
            format: format.to_string(),
            declared,
        });
    }
    Ok(spec.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    fn wasm_plugin(id: &str, formats: Vec<String>) -> LoadedPlugin {
        let manifest: PluginManifest = toml::from_str(&format!(
            r#"
                [plugin]
                id = "{id}"
                name = "{id}"
                version = "0.1.0"
                publisher = "test"
                engines = {{ rdatastation = "^0.1.0" }}
                [capabilities.wasm]
                entry = "{id}.wasm"
                render_document = {{ formats = {formats:?} }}
            "#
        ))
        .unwrap();
        LoadedPlugin {
            id: id.to_string(),
            manifest,
            install_path: Default::default(),
            status: crate::manager::LoadStatus::Loaded,
        }
    }

    fn sidecar_plugin(id: &str) -> LoadedPlugin {
        let manifest: PluginManifest = toml::from_str(&format!(
            r#"
                [plugin]
                id = "{id}"
                name = "{id}"
                version = "0.1.0"
                publisher = "test"
                engines = {{ rdatastation = "^0.1.0" }}
            "#
        ))
        .unwrap();
        LoadedPlugin {
            id: id.to_string(),
            manifest,
            install_path: Default::default(),
            status: crate::manager::LoadStatus::Loaded,
        }
    }

    #[test]
    fn renderer_for_finds_the_first_matching_format() {
        let plugins = vec![
            sidecar_plugin("driver-a"),
            wasm_plugin("render-a", vec!["mermaid".into()]),
            wasm_plugin("render-b", vec!["dot".into(), "mermaid".into()]),
        ];
        assert_eq!(renderer_for(&plugins, "mermaid").as_deref(), Some("render-a"));
        assert_eq!(renderer_for(&plugins, "dot").as_deref(), Some("render-b"));
    }

    #[test]
    fn renderer_for_prefers_empty_formats_as_wildcard() {
        let plugins = vec![
            wasm_plugin("any", vec![]),
            wasm_plugin("mermaid-only", vec!["mermaid".into()]),
        ];
        assert_eq!(renderer_for(&plugins, "text").as_deref(), Some("any"));
        // 顺序在前先胜出
        let plugins = vec![
            wasm_plugin("mermaid-only", vec!["mermaid".into()]),
            wasm_plugin("any", vec![]),
        ];
        assert_eq!(renderer_for(&plugins, "mermaid").as_deref(), Some("mermaid-only"));
    }

    #[test]
    fn renderer_for_skips_non_wasm_and_non_declared() {
        // no-declare 手工构造为「无 render_document」
        let mut p = wasm_plugin("no-declare", vec![]);
        p.manifest.capabilities.wasm.as_mut().unwrap().render_document = None;
        let plugins = vec![sidecar_plugin("driver-a"), p];
        assert_eq!(renderer_for(&plugins, "mermaid"), None);
        assert_eq!(renderer_for(&[], "mermaid"), None);
    }

    #[test]
    fn resolve_spec_errors_map_to_render_errors() {
        // 无插件
        let mgr = PluginManager::new(Default::default());
        assert_eq!(
            resolve_spec(&mgr, "nope", "mermaid"),
            Err(RenderError::PluginNotFound("nope".into()))
        );
        // 格式匹配语义（accepts）已由 renderer_for 测试覆盖：
        // formats 空 = 任意；非空 = 精确匹配、第一个胜出。
    }

    #[test]
    fn manifest_render_document_section_parses() {
        let toml = r#"
            [plugin]
            id = "mermaid-render"
            name = "mermaid-render"
            version = "0.1.0"
            publisher = "test"
            engines = { rdatastation = "^0.1.0" }
            [capabilities.wasm]
            entry = "render.wasm"
            render_document = { function = "to_svg", formats = ["mermaid"], output_format = "svg" }
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        let spec = m.capabilities.wasm.unwrap().render_document.unwrap();
        assert_eq!(spec.function, "to_svg");
        assert_eq!(spec.formats, vec!["mermaid"]);
        assert_eq!(spec.output_format, "svg");
    }

    #[test]
    fn manifest_render_document_defaults_when_omitted() {
        let toml = r#"
            [plugin]
            id = "mermaid-render"
            name = "mermaid-render"
            version = "0.1.0"
            publisher = "test"
            engines = { rdatastation = "^0.1.0" }
            [capabilities.wasm]
            entry = "render.wasm"
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        let w = m.capabilities.wasm.unwrap();
        assert!(w.render_document.is_none());
        // 声明了但缺字段 → 默认 function = render / output_format = svg
        let toml = r#"
            [plugin]
            id = "mermaid-render"
            name = "mermaid-render"
            version = "0.1.0"
            publisher = "test"
            engines = { rdatastation = "^0.1.0" }
            [capabilities.wasm]
            entry = "render.wasm"
            render_document = { formats = ["mermaid"] }
        "#;
        let m: PluginManifest = toml::from_str(toml).unwrap();
        let spec = m.capabilities.wasm.unwrap().render_document.unwrap();
        assert_eq!(spec.function, "render");
        assert_eq!(spec.output_format, "svg");
    }
}
