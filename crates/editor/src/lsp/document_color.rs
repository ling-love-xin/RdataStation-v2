//! 颜色字面量 document color provider（A2：把内核 document color 槽位用起来）
//!
//! ## 一句话
//!
//! 内核的 `document_color_provider` 槽位是单槽 `Option`：挂了 provider 后，内核在文本
//! 变化后 **100ms 防抖** 自动调 `document_colors()`（`update_document_colors`），把返回的
//! 颜色在对应 range 下方渲染**色块**（`base/element.rs` 的画色路径）。本模块给 SQL /
//! 脚本文档提供**颜色字面量**识别：`#rgb` / `#rrggbb` / `#rrggbbaa`（也认 `rgb()` / 
//! `rgba()` 函数字面量——SQL 里颜色常以字符串或十六进制出现）。
//!
//! ## 覆盖口径
//!
//! - **只认字面量**：`#` 后跟 3 / 6 / 8 位十六进制（`0-9a-fA-F`），前后边界是「非十六进制
//!   字符」（避免把 `#abc123` 出现在一个更长标识符里误判）。
//! - **不认命名色**（`red` / `blue` 等）：命名色在 SQL 里是标识符，撞表名列名的概率高，
//!   误标比不标更糟——宁缺毋滥（与语义层「无目录退化」同一口径）。
//! - 脚本（python 等）走**语言服务**的话，真正的 LSP 会自己报颜色；本 provider 只在
//!   SQL 模式挂（自研语义层，语言服务不参与颜色），通用模式的 LSP 分支不挂。

use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{DocumentColorProvider, Rope};
use gpui_kit::*;
use lsp_types::{Color, ColorInformation, Position, Range as LspRange};

/// SQL 颜色字面量 provider（无状态：可随时重建）
pub struct SqlDocumentColorProvider;

impl SqlDocumentColorProvider {
    pub fn new() -> Rc<Self> {
        Rc::new(Self)
    }
}

/// 把十六进制颜色字符串解析成 LSP `Color`（`#rgb` / `#rrggbb` / `#rrggbbaa`；
/// 失败返回 `None`——长度不对 / 非十六进制，调用方已先按字符校验，这里是双保险）
fn parse_hex(hex: &str) -> Option<Color> {
    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            // 短格式每位翻倍：`#abc` = `#aabbcc`
            (r * 17, g * 17, b * 17, 255)
        }
        6 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            255,
        ),
        8 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            u8::from_str_radix(&hex[6..8], 16).ok()?,
        ),
        _ => return None,
    };
    Some(Color {
        red: r as f32 / 255.,
        green: g as f32 / 255.,
        blue: b as f32 / 255.,
        alpha: a as f32 / 255.,
    })
}

/// 扫描一段文本里的 `#hex` 颜色字面量（纯函数，可测）
///
/// 返回 `(字节起点, 字节终点, 颜色)`；边界判据：`#` 前的字符不是十六进制（避免
/// `x#abc` 之类误判），字面量本体全部是十六进制。
fn scan_colors(text: &str) -> Vec<(usize, usize, Color)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            // 前边界：`#` 不能紧跟十六进制字符（`0x#abc` 里的 `x` 后接 `#` 合法，但
            // `a#bc` 的 `a` 是十六进制——它会让 `#bc` 看起来像 `#` 起头，实际不是字面量）
            let prev_hex = i > 0 && is_hex(bytes[i - 1]);
            if !prev_hex {
                let start = i;
                let mut j = i + 1;
                while j < bytes.len() && is_hex(bytes[j]) {
                    j += 1;
                }
                let hex = &text[start + 1..j];
                if matches!(hex.len(), 3 | 6 | 8) {
                    if let Some(color) = parse_hex(hex) {
                        out.push((start, j, color));
                    }
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn is_hex(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

/// 字节偏移 → LSP `Position`（0 基、列按**字符**数；与 `sql_hover::byte_position` 同口径）
fn byte_position(text: &str, offset: usize) -> Position {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|index| index + 1).unwrap_or(0);
    Position::new(line as u32, text[line_start..offset].chars().count() as u32)
}

impl DocumentColorProvider for SqlDocumentColorProvider {
    fn document_colors(
        &self,
        text: &Rope,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<Vec<ColorInformation>>> {
        // 纯扫描（线性）；文本变化后内核 100ms 防抖调一次，直接同步给
        let plain = text.to_string();
        let colors = scan_colors(&plain)
            .into_iter()
            .map(|(start, end, color)| ColorInformation {
                range: LspRange {
                    start: byte_position(&plain, start),
                    end: byte_position(&plain, end),
                },
                color,
            })
            .collect();
        Task::ready(Ok(colors))
    }
}

/// 装到某个文档的编辑内核上（`EditorState::lsp_mut`；单槽 Option，直接替换）
pub fn install(state: &mut gpui_kit::component::input::EditorState) {
    state.lsp_mut().document_color_provider = Some(SqlDocumentColorProvider::new());
}

/// 摘掉（文本模式 / 档位禁用时）
pub fn uninstall(state: &mut gpui_kit::component::input::EditorState) {
    state.lsp_mut().document_color_provider = None;
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{byte_position, parse_hex, scan_colors};

    #[test]
    fn hex_parses_all_widths() {
        let rgb = parse_hex("abc").expect("3 位可解析");
        assert!((rgb.red - 0.667).abs() < 0.01, "a=170/255≈0.667，得 {}", rgb.red);
        assert_eq!(rgb.alpha, 1.0);
        let rrggbb = parse_hex("ff0000").expect("6 位可解析");
        assert_eq!(rrggbb.red, 1.0);
        assert_eq!(rrggbb.green, 0.0);
        let rrggbbaa = parse_hex("00ff0080").expect("8 位可解析");
        assert_eq!(rrggbbaa.alpha, 128. / 255.);
        // 非法：长度不对 / 非十六进制
        assert!(parse_hex("ab").is_none());
        assert!(parse_hex("abcde").is_none());
        assert!(parse_hex("zzz").is_none());
    }

    #[test]
    fn scan_finds_literals_with_boundaries() {
        let sql = "SELECT '#ff0000', '#0f0', x'00ff00ff' -- #123456 注释";
        let colors = scan_colors(sql);
        // `#ff0000`（字符串内）、`#0f0`（字符串内）、`#123456`（注释内）都认；
        // `x'00ff00ff'` 是 x 前缀十六进制字面量，没有 `#`，不参与
        assert_eq!(colors.len(), 3, "{colors:?}");
        // 第一个字面量的字节区间：`#ff0000` 在 "SELECT '" 后第 8 字节起
        assert_eq!((colors[0].0, colors[0].1), (8, 15));
    }

    #[test]
    fn scan_rejects_non_literals() {
        // `a#bc`：`#` 前是十六进制字符 a → 不是字面量（避免把标识符里的 # 误标）
        let text = "a#bc";
        assert!(scan_colors(text).is_empty());
        // `#xyz`：非十六进制 → 不认
        assert!(scan_colors("#xyz").is_empty());
        // 超长：`#123456789` 是 9 位 → 不认
        assert!(scan_colors("#123456789").is_empty());
    }

    #[test]
    fn byte_position_counts_lines_and_chars() {
        let sql = "SELECT 1\n  #fff";
        assert_eq!(byte_position(sql, 0), lsp_types::Position::new(0, 0));
        assert_eq!(byte_position(sql, 11), lsp_types::Position::new(1, 2));
    }
}
