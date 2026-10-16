//! JSON 信封解析：把宿主传入的 input 归约为 (source, RenderOptions)。
//!
//! 设计取舍：
//! - 非 JSON / 缺 `source` → 整个输入按纯文本处理（与宿主 render 第一刀的
//!   `input = source.as_bytes()` 契约兼容，老调用零改动）。
//! - `theme.preset` 五档映射上游预设构造器；`theme.overrides` 用 serde 字段合并：
//!   preset Theme 序列化为 JSON 对象 → overrides 逐键覆盖 → 反序列化回 Theme
//!   （上游 `Theme` 本身实现 `Serialize`/`Deserialize`，字段全 pub；未知键被 serde 忽略）。
//! - overrides 里给 `font_family` 时也允许——但默认字体栈已含 DejaVu Sans，
//!   宿主一般无需显式指定。

use serde::Deserialize;
use serde_json::Value;

use mermaid_rs_renderer::{RenderOptions, Theme};

/// 信封结构；`theme` 缺省即默认 modern 主题。
#[derive(Debug, Deserialize)]
struct Envelope {
    source: String,
    #[serde(default)]
    theme: Option<ThemeRef>,
}

#[derive(Debug, Deserialize)]
struct ThemeRef {
    /// 预设名：modern | mermaid_default | dark | forest | neutral
    #[serde(default)]
    preset: Option<String>,
    /// 对 `Theme` pub 字段的覆盖（键 = 字段名）
    #[serde(default)]
    overrides: Option<std::collections::HashMap<String, Value>>,
}

/// 把 input 归约为 (source, options)。永不失败：解析不了就当纯文本。
pub fn parse(input: &str) -> (String, RenderOptions) {
    match serde_json::from_str::<Envelope>(input) {
        Ok(envelope) => (envelope.source, options(envelope.theme)),
        Err(_) => (input.to_string(), RenderOptions::default()),
    }
}

fn options(theme: Option<ThemeRef>) -> RenderOptions {
    match theme {
        Some(theme) => {
            let mut base = preset_theme(theme.preset.as_deref());
            if let Some(overrides) = theme.overrides {
                if !overrides.is_empty() {
                    // preset 序列化 → 覆盖 → 反序列化；未知字段被 serde 忽略
                    if let Ok(mut value) = serde_json::to_value(&base) {
                        if let Value::Object(ref mut map) = value {
                            for (key, val) in overrides {
                                map.insert(key, val);
                            }
                        }
                        if let Ok(merged) = serde_json::from_value::<Theme>(value) {
                            base = merged;
                        }
                    }
                }
            }
            RenderOptions {
                theme: base,
                ..RenderOptions::default()
            }
        }
        None => RenderOptions::default(),
    }
}

fn preset_theme(name: Option<&str>) -> Theme {
    match name {
        Some("mermaid_default") => Theme::mermaid_default(),
        Some("dark") => Theme::dark(),
        Some("forest") => Theme::forest(),
        Some("neutral") => Theme::neutral(),
        // 未知预设名回退 modern（与 RenderOptions::default 一致）
        _ => Theme::modern(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        let (source, options) = parse("flowchart LR\n  A --> B\n");
        assert_eq!(source, "flowchart LR\n  A --> B\n");
        assert_eq!(options.theme.font_family, Theme::modern().font_family);
    }

    #[test]
    fn envelope_parses_source() {
        let (source, _) = parse(r#"{"source": "pie\n  \"A\" : 1"}"#);
        assert_eq!(source, "pie\n  \"A\" : 1");
    }

    #[test]
    fn envelope_preset_dark() {
        let (_, options) = parse(r#"{"source": "x", "theme": {"preset": "dark"}}"#);
        assert_eq!(options.theme.font_family, Theme::dark().font_family);
        assert_eq!(options.theme.background, Theme::dark().background);
    }

    #[test]
    fn envelope_overrides_merge_onto_preset() {
        let (_, options) = parse(
            r##"{"source": "x", "theme": {"preset": "modern", "overrides": {"primary_color": "#123456"}}}"##,
        );
        assert_eq!(options.theme.primary_color, "#123456");
        // 未覆盖字段保持 preset 值
        assert_eq!(options.theme.font_family, Theme::modern().font_family);
    }

    #[test]
    fn envelope_unknown_fields_are_ignored() {
        let (_, options) = parse(
            r#"{"source": "x", "theme": {"preset": "neutral", "overrides": {"no_such_field": 42}}}"#,
        );
        assert_eq!(options.theme.font_family, Theme::neutral().font_family);
    }

    #[test]
    fn unknown_preset_falls_back_to_modern() {
        let (_, options) = parse(r#"{"source": "x", "theme": {"preset": "bogus"}}"#);
        assert_eq!(options.theme.font_family, Theme::modern().font_family);
    }

    #[test]
    fn invalid_json_is_treated_as_plain_text() {
        let (source, _) = parse("{not json");
        assert_eq!(source, "{not json");
    }
}
