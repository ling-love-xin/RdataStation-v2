//! 查询解析 / 门槛 / 命中区间 / 评分排序 / 高亮切段。

use crate::quick_open::model::{
    Action, Mode, Row, RowKind, filter_ranked, markup_segments, match_span, parse,
};

#[test]
fn parse_reads_mode_from_first_char_only() {
    assert_eq!(parse("order").mode, Mode::Default);
    assert_eq!(parse("order").needle, "order");
    assert_eq!(parse(">新建").mode, Mode::Command);
    assert_eq!(parse(">新建").needle, "新建");
    assert_eq!(parse("> 新建 查询 ").needle, "新建 查询");
    assert_eq!(parse("#渠道").mode, Mode::FullText);
    // 前缀不是首字符 → 当普通词
    assert_eq!(parse("订单 > 明细").mode, Mode::Default);
    assert_eq!(parse("   >撤销").mode, Mode::Command);
    // 全角符号不当前缀（输入法误触发防护）
    assert_eq!(parse("＞撤销").mode, Mode::Default);
}

#[test]
fn async_ready_threshold_depends_on_mode() {
    // 名称档（默认）：≥ 2 字（中文按字符计数，不按字节）
    assert!(!parse("o").async_ready());
    assert!(parse("or").async_ready());
    assert!(parse("订单").async_ready());
    // 内容档（`#`）：trigram 下 ≥ 3 字（2 字不足一个 trigram，必无命中）
    assert!(!parse("#渠").async_ready());
    assert!(!parse("#渠道").async_ready());
    assert!(parse("#渠道与").async_ready());
    // 命令档不发元数据搜索
    assert!(!parse(">新建").async_ready());
    assert!(!parse(">").async_ready());
    // 门槛自描述（UI 用它算“还差几个字”）
    assert_eq!(parse("o").min_len(), 2);
    assert_eq!(parse("#渠").min_len(), 3);
}

#[test]
fn match_span_reports_byte_range_and_tolerates_non_ascii_case() {
    assert_eq!(match_span("order_items", "items"), Some((6, 11)));
    assert_eq!(match_span("OrderItems", "order"), Some((0, 5)));
    assert_eq!(match_span("订单明细", "明细"), Some((6, 12)));
    assert_eq!(match_span("orders", ""), Some((0, 0)));
    assert_eq!(match_span("orders", "zzz"), None);
    // 非 ASCII 大小写折叠后长度变化 → 放弃定位
    assert_eq!(match_span("İstanbul", "istanbul"), None);
}

#[test]
fn rank_prefers_exact_then_prefix_then_word_start_then_substring() {
    let ranked = |needle: &str| {
        filter_ranked(
            ["order_items", "pre_order", "order", "order_id"]
                .iter()
                .map(|t| Row {
                    key: (*t).to_string(),
                    kind: RowKind::Command,
                    title: (*t).to_string(),
                    match_text: (*t).to_string(),
                    secondary: String::new(),
                    snippet: None,
                    why: None,
                    locate: None,
                    action: Action::OpenSettings,
                    secondary_action: None,
                })
                .collect(),
            needle,
        )
        .into_iter()
        .map(|r| r.title)
        .collect::<Vec<_>>()
    };
    assert_eq!(
        ranked("order"),
        vec!["order", "order_id", "order_items", "pre_order"]
    );
    // 词首档：`items` 只命中 order_items（下划线之后）
    assert_eq!(ranked("items"), vec!["order_items"]);
    // 空词 → 全部保留（排序退化为长度 + 字典序）
    assert_eq!(ranked("").len(), 4);
}

#[test]
fn markup_segments_splits_highlight_marks() {
    assert_eq!(
        markup_segments("a<mark>渠道</mark>b"),
        vec![
            ("a".to_string(), false),
            ("渠道".to_string(), true),
            ("b".to_string(), false)
        ]
    );
    assert_eq!(markup_segments("plain"), vec![("plain".to_string(), false)]);
    assert_eq!(
        markup_segments("<mark>x</mark>"),
        vec![("x".to_string(), true)]
    );
    assert!(markup_segments("").is_empty());
    // 不闭会的标记退化为普通文本（不 panic）
    assert_eq!(
        markup_segments("a<mark>b"),
        vec![("a".to_string(), false), ("b".to_string(), false)]
    );
}
