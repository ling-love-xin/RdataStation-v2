//! 匹配与评分：命中档位、命中区间、排序键。

use super::*;

/// 过滤 + 排序：先按匹配档，再按命中长度、标题长度，最后按标题字典序（稳定）。
pub(crate) fn filter_ranked(rows: Vec<Row>, needle: &str) -> Vec<Row> {
    let mut hits: Vec<(Rank, Row)> = rows
        .into_iter()
        .filter_map(|row| rank_row(&row, needle).map(|r| (r, row)))
        .collect();
    hits.sort_by(|(a, ra), (b, rb)| {
        a.tier
            .cmp(&b.tier)
            .then(a.span_len.cmp(&b.span_len))
            .then_with(|| ra.title.len().cmp(&rb.title.len()))
            .then_with(|| ra.title.cmp(&rb.title))
    });
    hits.into_iter().map(|(_, row)| row).collect()
}

/// 行的匹配档：`title` 与 `match_text` 各算一次，取更优的那个。
///
/// 为什么两处都要算：命令的 `match_text` 是「展示名 + 关键词」的拼接串，
/// 精确 / 前缀两档在拼接串上命中不了（`打开设置 settings` ≠ `打开设置`）；
/// 只看拼接串，会把「输入完整展示名」的精确命中降级成子串命中。
pub(crate) fn rank_row(row: &Row, needle: &str) -> Option<Rank> {
    let title = rank(&row.title, needle);
    // 补充匹配面要够长才算数：单字符对英文关键词是噪声（凡是关键词含 `s` 的命令都会命中），
    // 而展示名照旧不受限（本地源支持单字符搜——那是「连接名第一个字母」这种真实用法）。
    if needle.chars().count() < MIN_NEEDLE_LEN {
        return title;
    }
    [title, rank(&row.match_text, needle)]
        .into_iter()
        .flatten()
        .min_by_key(|r| (r.tier, r.span_len, r.span_start))
}

/// 匹配档（越小越优先）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// 完全相同。
    Exact,
    /// 名称前缀。
    Prefix,
    /// 词首命中（`snake_case` / `camelCase` 分段首字母）。
    WordStart,
    /// 子串命中（同档内按位置靠前）。
    Substring,
}

/// 一次命中的排序信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rank {
    pub tier: Tier,
    /// 命中区间长度（同档内短者优先）。
    pub span_len: usize,
    /// 命中起点（同档内靠前者优先）。
    pub span_start: usize,
}

/// 纯排序信息（不含高亮区间计算）。
fn rank(title: &str, needle: &str) -> Option<Rank> {
    let span = match_span(title, needle)?;
    let tier = if needle.is_empty() {
        Tier::Substring
    } else if title.eq_ignore_ascii_case(needle) {
        Tier::Exact
    } else if span.0 == 0 {
        Tier::Prefix
    } else if is_word_start(title, span.0) {
        Tier::WordStart
    } else {
        Tier::Substring
    };
    Some(Rank {
        tier,
        span_len: span.1.saturating_sub(span.0),
        span_start: span.0,
    })
}

/// 命中区间（字节范围；空词返回 `(0, 0)`）。
pub fn match_span(title: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return Some((0, 0));
    }
    // 大小写折叠只按 ASCII 生效；折叠后长度变化（罕见 Unicode）时放弃定位，退化为「整串可搜」。
    let (hay, ndl) = (title.to_lowercase(), needle.to_lowercase());
    if hay.len() != title.len() || ndl.len() != needle.len() {
        return if title.contains(needle) {
            Some((0, 0))
        } else {
            None
        };
    }
    hay.find(&ndl).map(|pos| (pos, pos + ndl.len()))
}

/// 命中位置是否落在某个分段的词首（`order_items` 的 `items`、`orderItems` 的 `Items`）。
fn is_word_start(title: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let prev = title[..pos].chars().next_back();
    let cur = title[pos..].chars().next();
    match (prev, cur) {
        (Some(p), Some(c)) => {
            p == '_' || p == '-' || p == ' ' || p == '.' || (p.is_lowercase() && c.is_uppercase())
        }
        _ => false,
    }
}
