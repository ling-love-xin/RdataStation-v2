//! 文本编码与换行（2c：编码 / 换行重设的数据层）
//!
//! ## 为什么单独一层
//!
//! 编辑内核的文本模型统一 UTF-8 + `\n`（见 T2c-4 完成记录）：**磁盘上的编码差异
//! （GBK / UTF-8 BOM）与换行差异（CRLF）属于文件 IO 层，不属于编辑内核**。本模块收
//! 「检测 / 解码 / 编码 / 换行规范化」的纯函数，`persist`（读写盘）与面板事件（切换）
//! 从这里取行为；单测不需要临时目录。
//!
//! ## 语义（与 VSCode 对齐的简化版）
//!
//! - **加载**：读字节 → 按 BOM / 合法性猜编码 → 解码成 Unicode 进内核；换行按内容检测
//!   （含 `\r\n` = CRLF，否则 LF），文档记下检测到的 `SaveFormat`。
//! - **保存**：按文档当前 `SaveFormat` 生成字节（换行先规范化，再按编码编码）。
//! - **切换**：只改文档级 `SaveFormat` 并标脏（保存会产出不同字节 = 未保存的改动）；
//!   内核文本不动（它始终是 Unicode）。

use std::ops::Range;

/// 文本编码（编辑器可保存的编码集合）
///
/// 编码选择刻意收敛到这几档：UTF-8（默认）· UTF-8 BOM · GBK · UTF-16 LE/BE——
/// 覆盖 Windows 上 Oracle SQL 脚本最常见的几类（GBK / 带 BOM / UTF-16）。
/// 更宽的猜测（Shift-JIS 等）将来用 `chardetng` 扩展，见 `decode_bytes` 的兜底链。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// UTF-8 无 BOM（默认；内核原生编码）
    Utf8,
    /// UTF-8 带 BOM（`EF BB BF`；Windows 记事本「另存为 UTF-8」的产物）
    Utf8Bom,
    /// GBK（GB2312 超集；中文 Windows 老文件常见）
    Gbk,
    /// UTF-16 小端（带 BOM；Windows 记事本「Unicode」）
    Utf16Le,
    /// UTF-16 大端（带 BOM；旧 Mac / 某些 Windows 工具）
    Utf16Be,
}

impl TextEncoding {
    /// 菜单里的全名
    pub fn label(&self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 (BOM)",
            Self::Gbk => "GBK",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
        }
    }

    /// 状态栏短标（右段一行放不下长名）
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Gbk => "GBK",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
        }
    }

    /// 从设置文本解析编码（与设置模块 `editor.default_encoding` 落盘值一一对应；
    /// 未知文本返回 `None`，由调用方按「拒绝不猜」处理）。
    pub fn from_setting(text: &str) -> Option<Self> {
        match text {
            "UTF-8" => Some(Self::Utf8),
            "UTF-8-BOM" => Some(Self::Utf8Bom),
            "GBK" => Some(Self::Gbk),
            "UTF-16LE" => Some(Self::Utf16Le),
            "UTF-16BE" => Some(Self::Utf16Be),
            _ => None,
        }
    }

    /// 按本编码把文本编成字节（含 BOM 的编码会带上 BOM）
    pub fn encode(&self, text: &str) -> Vec<u8> {
        match self {
            Self::Utf8 => text.as_bytes().to_vec(),
            Self::Utf8Bom => {
                let mut bytes = Vec::with_capacity(text.len() + 3);
                bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
                bytes.extend_from_slice(text.as_bytes());
                bytes
            }
            Self::Gbk => encoding_rs::GBK.encode(text).0.into_owned(),
            // UTF-16 一律带 BOM 写（手动拼前缀，同 `Utf8Bom` 分支的写法）：无 BOM 的 UTF-16
            // 字节（ASCII 区）本身是合法 UTF-8（带 NUL），`decode_bytes` 只认 BOM 才判 UTF-16——
            // 不带 BOM 会静默误判成 UTF-8（乱码且不可逆）。
            //
            // 注意**不能**用 `encoding_rs::UTF_16LE.encode`：WHATWG 编码标准里 UTF-16LE/BE 的
            // 输出编码是 UTF-8（`output_encoding()` 决定），`encode` 实际返回 UTF-8 字节。
            // 这里用标准库 `encode_utf16()` 手写（代理对语义与 UTF-16 解码端一致）。
            Self::Utf16Le => {
                let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
                bytes.extend_from_slice(&[0xFF, 0xFE]);
                for unit in text.encode_utf16() {
                    bytes.extend_from_slice(&unit.to_le_bytes());
                }
                bytes
            }
            Self::Utf16Be => {
                let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
                bytes.extend_from_slice(&[0xFE, 0xFF]);
                for unit in text.encode_utf16() {
                    bytes.extend_from_slice(&unit.to_be_bytes());
                }
                bytes
            }
        }
    }
}

impl Default for TextEncoding {
    fn default() -> Self {
        Self::Utf8
    }
}

/// 换行风格（保存时按它生成行尾）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// `\n`（内核默认）
    Lf,
    /// `\r\n`（Windows）
    Crlf,
}

impl LineEnding {
    /// 菜单 / 状态栏共用短名
    pub fn label(&self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
        }
    }

    /// 从文本检测：含 `\r\n` 就是 CRLF（lone `\r` 罕见，不算）
    pub fn detect(text: &str) -> Self {
        if text.contains("\r\n") {
            Self::Crlf
        } else {
            Self::Lf
        }
    }

    /// 从设置文本解析换行（`"LF"` / `"CRLF"`；未知返回 `None`）。
    pub fn from_setting(text: &str) -> Option<Self> {
        match text {
            "LF" => Some(Self::Lf),
            "CRLF" => Some(Self::Crlf),
            _ => None,
        }
    }

    /// 把文本规范到目标换行（幂等：已是目标换行则原样返回）
    ///
    /// - `Lf`：`\r\n` → `\n`，lone `\r` → `\n`
    /// - `Crlf`：`\n` → `\r\n`（先归一 CRLF，避免 `\r\r\n` 双写）
    pub fn normalize(&self, text: &str) -> String {
        let without_crlf = text.replace("\r\n", "\n");
        match self {
            Self::Lf => without_crlf.replace('\r', "\n"),
            Self::Crlf => without_crlf.replace('\n', "\r\n"),
        }
    }
}

impl Default for LineEnding {
    fn default() -> Self {
        Self::Lf
    }
}

/// 保存格式（编码 + 换行）：文档级权威值
///
/// 切换编码 / 换行 = 换掉这里的 `encoding` / `eol` 并标脏；保存时 `persist` 按它生成字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveFormat {
    pub encoding: TextEncoding,
    pub eol: LineEnding,
}

impl Default for SaveFormat {
    fn default() -> Self {
        Self {
            encoding: TextEncoding::Utf8,
            eol: LineEnding::Lf,
        }
    }
}

impl SaveFormat {
    /// 换掉编码，其余（换行）不动
    pub fn with_encoding(&self, encoding: TextEncoding) -> Self {
        Self {
            encoding,
            eol: self.eol,
        }
    }

    /// 换掉换行，其余（编码）不动
    pub fn with_eol(&self, eol: LineEnding) -> Self {
        Self {
            encoding: self.encoding,
            eol,
        }
    }
}

/// 解码磁盘字节 → (Unicode 文本, 判定编码)
///
/// 判定链（**不收 chardetng，编码集合刻意收敛**，见 `TextEncoding` 注释）：
/// 1. BOM：UTF-8 `EF BB BF` / UTF-16 LE `FF FE` / UTF-16 BE `FE FF` → 直接按 BOM 解码；
/// 2. 无 BOM：严格 UTF-8 能解 → UTF-8；
/// 3. 否则按 GBK 兜底（中文 Windows 老脚本最常见；GBK 解码极少报错）。
///
/// 对「未知编码」不装懂：GBK 兜底能开大多数中文文件，个别真乱码交给用户在菜单里手动重设。
pub fn decode_bytes(bytes: &[u8]) -> (String, TextEncoding) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let (text, _, _) = encoding_rs::UTF_8.decode(&bytes[3..]);
        return (text.into_owned(), TextEncoding::Utf8Bom);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (text, _, _) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
        return (text.into_owned(), TextEncoding::Utf16Le);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (text, _, _) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
        return (text.into_owned(), TextEncoding::Utf16Be);
    }
    let (text, _, had_errors) = encoding_rs::UTF_8.decode(bytes);
    if !had_errors {
        return (text.into_owned(), TextEncoding::Utf8);
    }
    let (text, _, _) = encoding_rs::GBK.decode(bytes);
    (text.into_owned(), TextEncoding::Gbk)
}

/// 按目标格式把文本编成字节（换行先规范化，再按编码编码）
///
/// `persist` 保存的最终一步；`LineEnding::normalize` 幂等，混行尾文件也能收敛。
pub fn encode(text: &str, format: SaveFormat) -> Vec<u8> {
    let normalized = format.eol.normalize(text);
    format.encoding.encode(&normalized)
}

/// 隐藏字符的字节区间（②③：不可见字符标注；`hidden` 模块的检测在视图层用）
///
/// 这里只给「替换字符 / 编码相关」的纯语义：把 Range 生成挪给 `hidden.rs`（含测试），
/// 本函数仅为 `encode` 之后的字节级断言提供参照——不导出，避免两处口径漂移。
#[allow(dead_code)]
fn _placeholder(_text: &str) -> Vec<Range<usize>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_texts_round_trip_encodings_and_line_endings() {
        // #48：设置模块落盘文本 → 枚举解析（未知文本拒绝，不猜）
        for (text, want) in [
            ("UTF-8", TextEncoding::Utf8),
            ("UTF-8-BOM", TextEncoding::Utf8Bom),
            ("GBK", TextEncoding::Gbk),
            ("UTF-16LE", TextEncoding::Utf16Le),
            ("UTF-16BE", TextEncoding::Utf16Be),
        ] {
            assert_eq!(TextEncoding::from_setting(text), Some(want), "{text}");
        }
        assert_eq!(TextEncoding::from_setting("Shift-JIS"), None);
        assert_eq!(LineEnding::from_setting("LF"), Some(LineEnding::Lf));
        assert_eq!(LineEnding::from_setting("CRLF"), Some(LineEnding::Crlf));
        assert_eq!(LineEnding::from_setting("lf"), None);
    }

    #[test]
    fn encodings_round_trip_text() {
        let text = "select '中文' from 表;";
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Utf8Bom,
            TextEncoding::Gbk,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
        ] {
            let bytes = encoding.encode(text);
            let (decoded, detected) = decode_bytes(&bytes);
            assert_eq!(decoded, text, "{} 解码后应还原原文", encoding.label());
            assert_eq!(detected, encoding, "{} 应被正确识别", encoding.label());
        }
    }

    #[test]
    fn utf8_bom_keeps_a_bom_prefix() {
        let bytes = TextEncoding::Utf8Bom.encode("abc");
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "UTF-8 BOM 要带 EF BB BF");
        assert_eq!(&bytes[3..], b"abc");
    }

    #[test]
    fn decode_bytes_strips_bom_but_reports_utf16() {
        let le = [0xFF, 0xFE, b'a', 0x00, b'b', 0x00];
        let (text, enc) = decode_bytes(&le);
        assert_eq!(text, "ab");
        assert_eq!(enc, TextEncoding::Utf16Le);

        let be = [0xFE, 0xFF, 0x00, b'a', 0x00, b'b'];
        let (text, enc) = decode_bytes(&be);
        assert_eq!(text, "ab");
        assert_eq!(enc, TextEncoding::Utf16Be);
    }

    #[test]
    fn utf16_encode_always_writes_a_bom() {
        // 无 BOM 的 UTF-16 LE 字节（ASCII 区）是合法 UTF-8（带 NUL），`decode_bytes` 会
        // 误判成 UTF-8；所以编码端必须带 BOM 写，往返才可逆（回归：曾踩过一次）。
        let le = TextEncoding::Utf16Le.encode("ab");
        assert_eq!(&le[..2], &[0xFF, 0xFE], "UTF-16 LE 要带 FF FE");
        let (text, enc) = decode_bytes(&le);
        assert_eq!(text, "ab");
        assert_eq!(enc, TextEncoding::Utf16Le);

        let be = TextEncoding::Utf16Be.encode("ab");
        assert_eq!(&be[..2], &[0xFE, 0xFF], "UTF-16 BE 要带 FE FF");
        let (text, enc) = decode_bytes(&be);
        assert_eq!(text, "ab");
        assert_eq!(enc, TextEncoding::Utf16Be);
    }

    #[test]
    fn non_utf8_bytes_fall_back_to_gbk() {
        // "中" 的 GBK 字节（0xD6 0xD0）不是合法 UTF-8
        let gbk = [0xD6, 0xD0, 0xB9, 0xFA]; // 中国
        let (text, enc) = decode_bytes(&gbk);
        assert_eq!(enc, TextEncoding::Gbk);
        assert_eq!(text, "中国");
    }

    #[test]
    fn valid_utf8_without_bom_is_utf8() {
        let (text, enc) = decode_bytes("select 1".as_bytes());
        assert_eq!(enc, TextEncoding::Utf8);
        assert_eq!(text, "select 1");
    }

    #[test]
    fn empty_bytes_default_to_utf8() {
        let (text, enc) = decode_bytes(&[]);
        assert_eq!(enc, TextEncoding::Utf8);
        assert_eq!(text, "");
    }

    #[test]
    fn line_ending_detects_crlf() {
        assert_eq!(LineEnding::detect("a\nb"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\r\nb"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("a\r\nb\nc"), LineEnding::Crlf);
        // lone \r 不算 CRLF
        assert_eq!(LineEnding::detect("a\rb"), LineEnding::Lf);
    }

    #[test]
    fn line_ending_normalize_is_idempotent_and_handles_mixed() {
        // → LF：CRLF 与 lone CR 都收敛成 \n
        assert_eq!(LineEnding::Lf.normalize("a\r\nb\rc\n"), "a\nb\nc\n");
        assert_eq!(LineEnding::Lf.normalize("a\nb\n"), "a\nb\n");
        // → CRLF：先归一 CRLF 再全换，不产生 \r\r\n
        assert_eq!(LineEnding::Crlf.normalize("a\r\nb\nc"), "a\r\nb\r\nc");
        assert_eq!(LineEnding::Crlf.normalize("a\r\nb\r\n"), "a\r\nb\r\n");
        // 幂等：已是目标换行原样返回
        let lf = "a\nb\n";
        assert_eq!(LineEnding::Lf.normalize(lf), lf);
        let crlf = "a\r\nb\r\n";
        assert_eq!(LineEnding::Crlf.normalize(crlf), crlf);
    }

    #[test]
    fn encode_applies_eol_then_encoding() {
        let format = SaveFormat {
            encoding: TextEncoding::Utf8,
            eol: LineEnding::Crlf,
        };
        let bytes = encode("a\nb", format);
        assert_eq!(bytes, b"a\r\nb");
    }

    #[test]
    fn save_format_default_is_utf8_lf() {
        let f = SaveFormat::default();
        assert_eq!(f.encoding, TextEncoding::Utf8);
        assert_eq!(f.eol, LineEnding::Lf);
    }
}
