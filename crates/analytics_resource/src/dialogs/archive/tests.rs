use super::{name_hint, parse_tags};

#[test]
fn tags_split_on_commas_and_spaces_keeping_order() {
    assert_eq!(
        parse_tags(" 报表, 月度  季度，报表 "),
        vec!["报表", "月度", "季度"],
        "去空、去重保序（中文逗号也算分隔符）"
    );
    assert!(parse_tags("   ").is_empty(), "全空白 = 没有标签");
    assert!(parse_tags("").is_empty());
}

/// 归档继承（规格 §7.4）的预填语义：`initial_tags` → `join(", ")` 预填 → `parse_tags` 解析，
/// 对不含分隔符的标签集合 roundtrip 保持原集合（顺序、去重）。
#[test]
fn initial_tags_roundtrip_through_comma_join() {
    let tags = vec!["报表".to_string(), "重要".to_string()];
    assert_eq!(parse_tags(&tags.join(", ")), tags);
    assert!(
        parse_tags(&Vec::<String>::new().join(", ")).is_empty(),
        "无继承标签时预填空串，提交即空"
    );
}

#[test]
fn name_hint_only_fires_on_blank() {
    assert!(name_hint("月报").is_none());
    assert_eq!(name_hint("  "), Some("显示名不能为空"));
}
