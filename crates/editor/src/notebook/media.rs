//! Jupyter mime bundle → 原生输出 的纯函数分发器（#14）
//!
//! kernel 层把一次 Display / ExecuteResult 的 mime bundle（[`jupyter_protocol::Media`]）
//! 原样回传；本模块决定「渲染哪个 / 渲染成什么」。本项目无 DOM/CSS/JS，所以按
//! **原生可渲染且最富**的优先级取一个 mime 转成 [`crate::cell::Output`]：
//!
//! 1. `image/png` / `image/jpeg` / `image/gif`（matplotlib 主力）→ [`Output::Image`]
//! 2. `text/html`（pandas DataFrame 默认）→ 解析成表格 → [`Output::TableData`]
//!    （HTML 不是表格 / 解析失败 → 回退同一 bundle 里的 `text/plain`）
//! 3. `text/markdown` → [`Output::Markdown`]
//! 4. `text/plain` → [`Output::Text`]
//! 5. 其余（Latex / Svg / Json / Plotly / Widget 等）→ 忽略（如实：本项目渲染不了，
//!    不假装显示）
//!
//! 一个 bundle 只产一条输出（Jupyter 约定取 richest 渲染一个），但一次执行可以有
//! 多条（stdout 流 + 多个 display），由上层按到达顺序 accumulate。

use base64::Engine as _;
use jupyter_protocol::{Media, MediaType};

use crate::cell::Output;

/// mime bundle → 输出（0..=1 条：取最富且原生可渲染的）
pub fn media_to_outputs(media: &Media) -> Vec<Output> {
    // 1. 图片优先（matplotlib 出图；pandas 同时给 html + png 时也显示图——
    //    本项目的 HTML 只能降级成表格，图的信息密度更高）
    if let Some(image) = first_image(media) {
        return vec![image];
    }
    // 2. HTML：能解析成表格就用表格；否则回退同一 bundle 的 text/plain
    if let Some(html) = media
        .content
        .iter()
        .find_map(|m| match m {
            MediaType::Html(h) => Some(h.clone()),
            _ => None,
        })
    {
        if let Some((columns, rows)) = html_table_to_table_data(&html) {
            return vec![Output::TableData { columns, rows }];
        }
        return media
            .content
            .iter()
            .find_map(|m| match m {
                MediaType::Plain(p) => Some(Output::Text(p.clone())),
                _ => None,
            })
            .into_iter()
            .collect();
    }
    // 3. Markdown（GitHub 风格源码，TextView::markdown 渲染）
    if let Some(md) = media
        .content
        .iter()
        .find_map(|m| match m {
            MediaType::Markdown(s) => Some(s.clone()),
            _ => None,
        })
    {
        return vec![Output::Markdown(md)];
    }
    // 4. 纯文本兜底
    media
        .content
        .iter()
        .find_map(|m| match m {
            MediaType::Plain(p) => Some(Output::Text(p.clone())),
            _ => None,
        })
        .into_iter()
        .collect()
}

/// 找 bundle 里第一张可渲染的位图（png / jpeg / gif）。
///
/// Jupyter 协议的 `image/*` mime data 是 **base64 字符串**（不是字节），先解码；
/// 解码失败（脏数据）跳过该 mime，不 panic。
fn first_image(media: &Media) -> Option<Output> {
    use base64::engine::general_purpose::STANDARD as BASE64;
    for m in &media.content {
        let (encoded, mime) = match m {
            MediaType::Png(s) => (s, "image/png"),
            MediaType::Jpeg(s) => (s, "image/jpeg"),
            MediaType::Gif(s) => (s, "image/gif"),
            _ => continue,
        };
        if let Ok(bytes) = BASE64.decode(encoded) {
            return Some(Output::Image { bytes, mime: mime.to_string() });
        }
    }
    None
}

/// 轻量 HTML 表格解析（pandas DataFrame 的 `text/html` 是目标；结构简单：
/// 一张 `<table>` + `<thead><tr><th>` + `<tbody><tr><td>`，不做通用 HTML 引擎）
///
/// 返回 `(列名, 行)`；第一行 `<th>` 当列名。解析失败 / 不是表格 = `None`
/// （调用方回退 text/plain）。
fn html_table_to_table_data(html: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let table_start = html.find("<table")?;
    let table_rest = &html[table_start..];
    let table_end = table_rest.find("</table>")?;
    let table = &table_rest[..table_end + "</table>".len()];

    let mut columns: Option<Vec<String>> = None;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut rest = table;

    while let Some(tr_pos) = rest.find("<tr") {
        let row_open_end = rest[tr_pos..].find('>')? + tr_pos + 1;
        let row_rest = &rest[row_open_end..];
        let tr_close = row_rest.find("</tr>")?;
        let row_html = &row_rest[..tr_close];

        // 行内单元格：`<th>` / `<td>`（行内样式 / 标签一并剥掉）
        let mut cells: Vec<String> = Vec::new();
        let mut cell_rest = row_html;
        while let Some(td_pos) = cell_rest.find("<t") {
            let tag_open_end = cell_rest[td_pos..].find('>')? + td_pos + 1;
            let cell_rest2 = &cell_rest[tag_open_end..];
            let td_close = cell_rest2.find("</t")?;
            cells.push(strip_tags(&cell_rest2[..td_close]));
            // 跳过 `</td>` / `</th>`（`</t` 后至少还有 3 个字符）
            cell_rest = &cell_rest2[td_close + 4..];
        }

        if cells.is_empty() {
            break;
        }
        match columns {
            None => columns = Some(cells),
            Some(_) => rows.push(cells),
        }
        // 前进到下一行：跳过 `</tr>`
        rest = &row_rest[tr_close + "</tr>".len()..];
    }

    Some((columns.unwrap_or_default(), rows))
}

/// 剥掉行内 HTML 标签并解码常见实体（够 pandas 输出用；不引 HTML 引擎）
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jupyter_protocol::MediaType;

    fn media(parts: Vec<MediaType>) -> Media {
        Media { content: parts }
    }

    /// 图片 bundle（matplotlib）：png（base64）+ plain → 出图
    #[test]
    fn image_bundle_prefers_png_over_plain() {
        // "iVBORw0KGgo=" = 合法 base64（PNG 签名占位；渲染不发生在纯函数层）
        let m = media(vec![
            MediaType::Plain("1".to_string()),
            MediaType::Png("iVBORw0KGgo=".to_string()),
        ]);
        let out = media_to_outputs(&m);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Output::Image { bytes, mime } => {
                assert_eq!(mime, "image/png");
                assert_eq!(bytes.as_slice(), b"\x89PNG\r\n\x1a\n", "base64 解码成原始字节");
            }
            other => panic!("应出 Image，实际 {other:?}"),
        }
    }

    /// HTML bundle（pandas DataFrame）：解析成表格；空行数时列名也要在
    #[test]
    fn html_bundle_becomes_table_data() {
        let html = concat!(
            "<table border=\"1\" class=\"dataframe\">",
            "<thead><tr style=\"text-align: right;\"><th></th><th>a</th><th>b</th></tr></thead>",
            "<tbody><tr><td>0</td><td>1</td><td>2</td></tr>",
            "<tr><td>1</td><td>3</td><td>4</td></tr></tbody>",
            "</table>"
        );
        let m = media(vec![MediaType::Html(html.to_string())]);
        let out = media_to_outputs(&m);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Output::TableData { columns, rows } => {
                assert_eq!(columns.len(), 3, "第一行 th 当列名");
                assert_eq!(columns[1], "a");
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0][1], "1");
                assert_eq!(rows[1][2], "4");
            }
            other => panic!("应出 TableData，实际 {other:?}"),
        }
    }

    /// HTML 不是表格（如 div 页面）→ 回退同 bundle 的 text/plain
    #[test]
    fn non_table_html_falls_back_to_plain() {
        let m = media(vec![
            MediaType::Html("<div>not a table</div>".to_string()),
            MediaType::Plain("fallback".to_string()),
        ]);
        let out = media_to_outputs(&m);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], Output::Text("fallback".to_string()));
    }

    /// Markdown + plain：Markdown 更富，取 Markdown
    #[test]
    fn markdown_wins_over_plain() {
        let m = media(vec![
            MediaType::Plain("**x**".to_string()),
            MediaType::Markdown("**x**".to_string()),
        ]);
        let out = media_to_outputs(&m);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], Output::Markdown("**x**".to_string()));
    }

    /// 只有 plain：文本输出
    #[test]
    fn plain_only_becomes_text() {
        let m = media(vec![MediaType::Plain("hello".to_string())]);
        let out = media_to_outputs(&m);
        assert_eq!(out, vec![Output::Text("hello".to_string())]);
    }

    /// 渲染不了的 mime（plotly / widget / json）→ 空（如实忽略）
    #[test]
    fn unrenderable_mimes_are_ignored() {
        let m = media(vec![
            MediaType::Plotly(serde_json::json!({})),
            MediaType::Json(serde_json::json!({"a": 1})),
            MediaType::Svg("<svg></svg>".to_string()),
        ]);
        assert!(media_to_outputs(&m).is_empty());
    }

    /// 行内标签与实体剥离：pandas 的样式 / 空转义
    #[test]
    fn html_cells_strip_tags_and_entities() {
        let html = concat!(
            "<table><thead><tr><th>名称</th><th>值</th></tr></thead>",
            "<tbody><tr><td style=\"color:red\">A&amp;B</td><td>1&nbsp;2</td></tr></tbody></table>"
        );
        let (columns, rows) = html_table_to_table_data(html).expect("能解析");
        assert_eq!(columns, vec!["名称".to_string(), "值".to_string()]);
        assert_eq!(rows[0][0], "A&B");
        assert_eq!(rows[0][1], "1 2");
    }
}
