// 安全模式：测试模块不通配导入。
use super::body_sections::{move_ids, move_label};
use super::{ArchiveDetail, ArchiveTagChip, alert_line, detail_rows, kind_label, short_hash};
use crate::model::{ArchiveKind, ArchiveStatus, PayloadMode, RowScope};

fn detail(status: ArchiveStatus, kind: ArchiveKind) -> ArchiveDetail {
    ArchiveDetail {
        assertion: None,
        lifecycle: Default::default(),
        id: "ar_1".to_string(),
        name: "dau_report".to_string(),
        alias: None,
        kind,
        version: 2,
        status,
        readonly: true,
        payload_mode: PayloadMode::Copy,
        source_path: None,
        scope: RowScope::Project,
        size_label: "1.2 KB".to_string(),
        modified_label: "3 天前".to_string(),
        archived_label: "3 天前".to_string(),
        promoted_from: Some("scratchpad/dau.sql".to_string()),
        source_connection_id: Some("conn_1".to_string()),
        source_table: None,
        content_hash: Some("0123456789abcdef0123".to_string()),
        payload_rel_path: Some("dau.sql".to_string()),
        history_label: "1 个历史版本".to_string(),
        recent_versions: Vec::new(),
        groups: Vec::new(),
        group_id: None,
        tags: Vec::new(),
        group: None,
        // 预览缺省 = 仅元信息（各用例需要时用结构体更新语法盖掉）。
        preview: crate::preview::Preview::default(),
    }
}

#[test]
fn short_hash_truncates_and_tolerates_missing() {
    assert_eq!(short_hash(Some("0123456789abcdef0123")), "0123456789ab");
    assert_eq!(short_hash(None), "—");
    assert_eq!(short_hash(Some("")), "—");
}

#[test]
fn rows_skip_empty_values_but_keep_fingerprint_placeholder() {
    let rows = detail_rows(&detail(ArchiveStatus::Normal, ArchiveKind::File));
    let basic = rows
        .iter()
        .find(|(title, _)| title == "基本信息")
        .expect("基本信息");
    // 只读说明必须出现（用户最常问"为什么不能编辑"）。
    assert!(
        basic
            .1
            .iter()
            .any(|(label, value)| *label == "只读" && value.contains("取回"))
    );

    let source = rows
        .iter()
        .find(|(title, _)| title == "来源")
        .expect("来源");
    assert!(source.1.iter().any(|(label, _)| *label == "来源草稿"));
    // 没有来源表 → 不产生该行；指纹则恒有（无值给破折号）。
    assert!(!source.1.iter().any(|(label, _)| *label == "来源表"));
    assert!(source.1.iter().any(|(label, _)| *label == "内容指纹"));
}

#[test]
fn organization_section_keeps_only_the_group() {
    let rows = detail_rows(&detail(ArchiveStatus::Normal, ArchiveKind::File));
    // 未分组也**照样**给出「组织」区：这一区下面挂着「移动…」入口，
    // 没有它就没地方把未分组的存档移进某个组。
    let ungrouped = rows
        .iter()
        .find(|(title, _)| title == "组织")
        .expect("未分组也要有组织区");
    assert!(
        ungrouped
            .1
            .iter()
            .any(|(label, value)| *label == "分组" && value == "未分组")
    );

    let mut with_group = detail(ArchiveStatus::Normal, ArchiveKind::File);
    // 标签不再走（标签, 值）两列（chips 可交互，在 `render_tag_section` 里渲染）。
    with_group.tags = vec![ArchiveTagChip {
        id: "at_1".to_string(),
        name: "报表".to_string(),
    }];
    with_group.group = Some("报表".to_string());
    with_group.group_id = Some("af_1".to_string());
    let rows = detail_rows(&with_group);
    let org = rows
        .iter()
        .find(|(title, _)| title == "组织")
        .expect("组织");
    assert!(
        org.1
            .iter()
            .any(|(label, value)| *label == "分组" && value == "报表")
    );
    assert!(
        !org.1.iter().any(|(label, _)| *label == "标签"),
        "标签不进文本行"
    );
}

/// 全局行没有项目分组归属：不给「组织」区（不给用户一个点了会错的入口）。
#[test]
fn global_rows_have_no_organization_section() {
    let mut global = detail(ArchiveStatus::Normal, ArchiveKind::File);
    global.scope = RowScope::Global;
    let rows = detail_rows(&global);
    assert!(!rows.iter().any(|(title, _)| title == "组织"));
}

#[test]
fn alerts_only_for_actionable_states() {
    assert!(alert_line(&detail(ArchiveStatus::Normal, ArchiveKind::File)).is_none());
    assert!(
        alert_line(&detail(ArchiveStatus::ContentChanged, ArchiveKind::File))
            .expect("内容已变应提示")
            .contains("内容已变")
    );
    assert!(
        alert_line(&detail(ArchiveStatus::Missing, ArchiveKind::File))
            .expect("缺失应提示")
            .contains("本体缺失")
    );
    // 引用型常态就要提示（复现强度最弱，不等失效）。
    assert!(
        alert_line(&detail(ArchiveStatus::Normal, ArchiveKind::TableRef))
            .expect("引用应提示")
            .contains("源表可能已变更")
    );
}

#[test]
fn kind_labels_are_shared_wording() {
    assert_eq!(kind_label(ArchiveKind::File), "受管文件");
    assert_eq!(kind_label(ArchiveKind::Analysis), "分析表");
    assert_eq!(kind_label(ArchiveKind::TableRef), "远端引用");
}

#[test]
fn move_targets_the_whole_selection_when_there_is_one() {
    // 单选：面板选集里就焦点行这一条，动它。
    let single = move_ids("ar_1", &["ar_1".to_string()]);
    assert_eq!(single, vec!["ar_1".to_string()]);
    // 多选：动整集（不是只动详情面板显示的那条）。
    let multi = move_ids(
        "ar_2",
        &["ar_1".to_string(), "ar_2".to_string(), "ar_3".to_string()],
    );
    assert_eq!(multi.len(), 3);
    assert!(multi.contains(&"ar_2".to_string()));
    // 没有选集（crate 单测 / 无宿主场景）：退回眼前这一条。
    assert_eq!(move_ids("ar_9", &[]), vec!["ar_9".to_string()]);
}

#[test]
fn move_label_counts_only_when_more_than_one() {
    assert_eq!(move_label(1), "移动…");
    // 数量要上按钮：详情面板只显示焦点行，不标条数会让人以为只动这一条。
    assert_eq!(move_label(3), "移动 3 条…");
}
