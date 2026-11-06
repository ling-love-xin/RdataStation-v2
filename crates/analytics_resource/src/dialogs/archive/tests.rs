use super::{KEEP_VERSIONS_MAX, name_hint, parse_keep_versions, parse_tags};
use crate::model::KeepVersions;

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
fn keep_versions_empty_means_following_settings() {
    assert_eq!(parse_keep_versions(""), Ok(None));
    assert_eq!(parse_keep_versions("  "), Ok(None));
    assert_eq!(parse_keep_versions("3"), Ok(Some(KeepVersions::Keep(3))));
    assert_eq!(
        parse_keep_versions("0"),
        Ok(Some(KeepVersions::MetadataOnly)),
        "0 = 只留元数据"
    );
    assert_eq!(
        parse_keep_versions(&KEEP_VERSIONS_MAX.to_string()),
        Ok(Some(KeepVersions::Keep(KEEP_VERSIONS_MAX)))
    );
    assert!(parse_keep_versions("abc").is_err());
    assert!(
        parse_keep_versions(&(KEEP_VERSIONS_MAX + 1).to_string()).is_err(),
        "超出上限挡在对话框，不留给服务层裁剪"
    );
}

/// `-1` = 全部保留（与设置项同一个哨兵）；其余负数不当全留静默吃掉。
#[test]
fn minus_one_means_keep_everything() {
    assert_eq!(parse_keep_versions("-1"), Ok(Some(KeepVersions::All)));
    assert!(parse_keep_versions("-2").is_err());
    assert!(parse_keep_versions("-").is_err());
}

/// 设置项字面量 ↔ 领域口径的往返（宿主在两个 crate 之间就是靠这一对转的）。
#[test]
fn keep_versions_roundtrips_the_setting_value() {
    for (setting, expected) in [
        (-1, KeepVersions::All),
        (0, KeepVersions::MetadataOnly),
        (5, KeepVersions::Keep(5)),
    ] {
        let value = KeepVersions::from_setting(setting);
        assert_eq!(value, expected);
        assert_eq!(value.to_setting(), setting, "落盘值不能被改变");
    }
    // 全留 = 不裁剪（不是“保留 0 份”）；只留元数据 = 保留 0 份。
    assert_eq!(KeepVersions::All.limit(), None);
    assert_eq!(KeepVersions::MetadataOnly.limit(), Some(0));
}

#[test]
fn name_hint_only_fires_on_blank() {
    assert!(name_hint("月报").is_none());
    assert_eq!(name_hint("  "), Some("显示名不能为空"));
}
