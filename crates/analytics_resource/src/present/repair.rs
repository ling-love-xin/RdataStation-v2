//! 索引修复行：按问题分组排序。

use super::*;

/// 扫描报告 → 修复行（对话框用，按分组顺序排好）。
///
/// 格式化的活都在这里：指纹缩略（复用详情面板的 `short_hash`，前 12 位）与行标题 /
/// 副文案；对话框只渲染与派发动作。
///
/// 顺序：分组按 `RepairGroup::ALL`，组内按本体路径（稳定、与扫描结果同序）。
pub fn build_repair_rows(report: &crate::IndexScanReport) -> Vec<RepairRow> {
    let mut rows: Vec<RepairRow> = report
        .issues
        .iter()
        .map(|issue| match issue {
            crate::IndexIssue::UntrackedFile { rel_path } => RepairRow {
                group: RepairGroup::Untracked,
                title: file_name_of(rel_path),
                detail: format!("本体 resources/{rel_path}：存在但没有登记记录"),
                hash_detail: String::new(),
                rel_path: rel_path.clone(),
                resource_id: None,
            },
            crate::IndexIssue::MissingPayload {
                resource_id,
                name,
                rel_path,
            } => RepairRow {
                group: RepairGroup::Missing,
                title: name.clone(),
                detail: format!("期望位置 resources/{rel_path}：本体不在了"),
                hash_detail: String::new(),
                rel_path: rel_path.clone(),
                resource_id: Some(resource_id.clone()),
            },
            crate::IndexIssue::ContentChanged {
                resource_id,
                name,
                rel_path,
                expected_hash,
                actual_hash,
            } => RepairRow {
                group: RepairGroup::Changed,
                title: name.clone(),
                detail: format!("本体 resources/{rel_path}：内容与登记不符"),
                hash_detail: format!(
                    "登记 {} · 实际 {}",
                    crate::detail_view::short_hash(Some(expected_hash)),
                    crate::detail_view::short_hash(Some(actual_hash))
                ),
                rel_path: rel_path.clone(),
                resource_id: Some(resource_id.clone()),
            },
        })
        .collect();

    rows.sort_by(|a, b| {
        let group = group_order(a.group).cmp(&group_order(b.group));
        group.then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    rows
}
/// 分组顺序（`RepairGroup::ALL` 的下标）。
fn group_order(group: RepairGroup) -> usize {
    RepairGroup::ALL
        .iter()
        .position(|candidate| *candidate == group)
        .unwrap_or(usize::MAX)
}
/// 相对路径 → 文件名（未登记行的标题：路径太长，行里放不下，完整路径在副文案里）。
fn file_name_of(rel_path: &str) -> String {
    rel_path.rsplit('/').next().unwrap_or(rel_path).to_string()
}
