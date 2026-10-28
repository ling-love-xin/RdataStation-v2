//! 不可见字符检测（②③：隐藏字符 / 控制字符标注）
//!
//! ## 为什么单独一层
//!
//! 写 SQL（尤其 Oracle）时最常见的「看着正常、一跑就错」来自不可见字符：零宽字符
//! （ZWSP / 软连字符）、全角空格、BOM、行内控制字符。这些字符在内核里照常显示（多数
//! 渲染成空白 / 无），用户无从察觉。本模块给出**哪些字节区间需要高亮标注**，纯函数、
//! 可穷举测试；视图层在「显示隐藏字符」开启时用它喂 `RangeDecoration`。
//!
//! ## 收录范围（VSCode `unicodeHighlight` 的收敛版）
//!
//! - **零宽与连字控制**：U+200B ZWSP / U+200C ZWNJ / U+200D ZWJ / U+2060 词连接符 /
//!   U+FEFF（文首 BOM 或文内 ZWSP）
//! - **特殊空格**：U+00A0 NBSP / U+1680 古爱尔兰空格 / U+2000–U+200A 各种空格 /
//!   U+202F 窄 NBSP / U+205F 中空格 / U+3000 全角空格
//! - **双向控制**：U+202A–U+202E / U+2066–U+2069（RTL 混淆代码的攻击面）
//! - **控制字符**：C0（U+0000–U+001F，**排除** `\t` `\n` `\r`——它们有正常语义）+
//!   C1（U+007F–U+009F）

use std::ops::Range;

/// 不可见字符的类别（双栏配置面板的粒度：用户可自定义每类显示 / 隐藏）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenCategory {
    /// 零宽与连字控制（ZWSP / ZWNJ / ZWJ / 词连接符 / BOM）
    ZeroWidth,
    /// 特殊空格（NBSP / 古爱尔兰空格 / 各种 Unicode 空格 / 全角空格）
    SpecialSpaces,
    /// 双向控制（RTL 混淆代码的攻击面）
    Bidi,
    /// 控制字符（C0 排除 `\t` `\n` `\r` + C1）
    Control,
}

/// 全部类别（面板左栏顺序）
pub fn all_categories() -> [HiddenCategory; 4] {
    [
        HiddenCategory::ZeroWidth,
        HiddenCategory::SpecialSpaces,
        HiddenCategory::Bidi,
        HiddenCategory::Control,
    ]
}

/// 类别的位掩码位（0..3，存进文档 `invisible_mask`）
pub fn category_bit(cat: HiddenCategory) -> u8 {
    match cat {
        HiddenCategory::ZeroWidth => 1 << 0,
        HiddenCategory::SpecialSpaces => 1 << 1,
        HiddenCategory::Bidi => 1 << 2,
        HiddenCategory::Control => 1 << 3,
    }
}

/// 全部类别位掩码（¶ 总开关「全显示」口径）
pub const FULL_MASK: u8 = 0b0000_1111;

/// 单个字符属于哪个类别（不属于不可见集合返回 `None`）
pub fn category_of(ch: char) -> Option<HiddenCategory> {
    let cp = ch as u32;
    let c1 = 0x007F..=0x009F;
    match cp {
        // 零宽 + 连字控制（ZWSP / ZWNJ / ZWJ / 词连接符）
        0x200B..=0x200D | 0x2060 | 0xFEFF => Some(HiddenCategory::ZeroWidth),
        // 特殊空格（NBSP / 古爱尔兰空格 / 各种 Unicode 空格 / 全角空格）
        0x00A0 | 0x1680 | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000 => {
            Some(HiddenCategory::SpecialSpaces)
        }
        // 双向控制（RTL 混淆）
        0x202A..=0x202E | 0x2066..=0x2069 => Some(HiddenCategory::Bidi),
        // 控制字符（C0 排除 \t(09) \n(0A) \r(0D)：09/0A/0D 分列于上述 range 间隙，天然排除）
        _ if (0x0000..=0x0008).contains(&cp)
            || (0x000B..=0x000C).contains(&cp)
            || (0x000E..=0x001F).contains(&cp)
            || c1.contains(&cp) =>
        {
            Some(HiddenCategory::Control)
        }
        _ => None,
    }
}

/// 单个字符是否属于「不可见 / 需标注」集合（任一类别命中）
pub fn is_hidden(ch: char) -> bool {
    category_of(ch).is_some()
}

/// 隐藏字符的字节区间（不重叠、按出现顺序；**全部类别**——兼容既有调用）
pub fn hidden_ranges(text: &str) -> Vec<Range<usize>> {
    hidden_ranges_filtered(text, &all_categories())
}

/// 隐藏字符的字节区间，只标**启用类别**命中的字符（双栏自定义面板的渲染口径）
pub fn hidden_ranges_filtered(text: &str, enabled: &[HiddenCategory]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut byte = 0usize;
    for ch in text.chars() {
        let start = byte;
        byte += ch.len_utf8();
        if let Some(cat) = category_of(ch) {
            if enabled.contains(&cat) {
                ranges.push(start..byte);
            }
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(text: &str, needle: char) -> Option<Range<usize>> {
        let offset = text.find(needle)?;
        let start = text[..offset].len();
        Some(start..start + needle.len_utf8())
    }

    #[test]
    fn zero_width_chars_are_hidden() {
        for ch in ['\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'] {
            let text = format!("select {ch} 1");
            let ranges = hidden_ranges(&text);
            assert_eq!(ranges, vec![range(&text, ch).unwrap()], "{ch:?} 应在列表里");
        }
    }

    #[test]
    fn special_spaces_are_hidden() {
        for ch in ['\u{00A0}', '\u{1680}', '\u{2000}', '\u{200A}', '\u{202F}', '\u{205F}', '\u{3000}'] {
            let text = format!("a{ch}b");
            let ranges = hidden_ranges(&text);
            assert_eq!(ranges, vec![range(&text, ch).unwrap()], "{ch:?} 应在列表里");
        }
    }

    #[test]
    fn control_chars_are_hidden_but_not_tab_newline_cr() {
        // 控制字符 C0/C1 全标
        for cp in [0x0001u32, 0x000B, 0x001F, 0x007F, 0x0085, 0x009F] {
            let ch = char::from_u32(cp).unwrap();
            let text = format!("a{ch}b");
            let ranges = hidden_ranges(&text);
            assert_eq!(ranges.len(), 1, "U+{cp:04X} 应被标出");
        }
        // 正常的 \t \n \r 不标
        for ch in ['\t', '\n', '\r'] {
            let text = format!("a{ch}b");
            assert!(hidden_ranges(&text).is_empty(), "{ch:?} 有正常语义，不应标");
        }
    }

    #[test]
    fn plain_ascii_and_cjk_have_no_hidden_chars() {
        assert!(hidden_ranges("select * from 用户表 where id = 1;").is_empty());
        assert!(hidden_ranges("").is_empty());
    }

    #[test]
    fn ranges_are_in_byte_offsets_and_cover_each_hit_once() {
        // 两个 ZWSP + 一个全角空格：三个不重叠区间，字节偏移正确
        let text = "a\u{200B}b\u{3000}c\u{200B}d";
        let ranges = hidden_ranges(text);
        assert_eq!(ranges.len(), 3);
        let mut prev_end = 0;
        for r in &ranges {
            assert!(r.start >= prev_end, "区间不重叠");
            assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end), "区间在字符边界");
            prev_end = r.end;
        }
        assert_eq!(ranges[0], 1..4); // a + ZWSP(3字节)
        assert_eq!(ranges[1], 5..8); // b + 全角空格(3字节)
        assert_eq!(ranges[2], 9..12); // c + ZWSP(3字节)
    }

    #[test]
    fn hidden_inside_multibyte_text_keeps_absolute_offsets() {
        // 前面有一段 3 字节中文，ZWSP 的偏移要算上它
        let text = "中文\u{200B}x";
        let ranges = hidden_ranges(text);
        assert_eq!(ranges, vec![6..9]); // "中文" 占 6 字节
    }
}
