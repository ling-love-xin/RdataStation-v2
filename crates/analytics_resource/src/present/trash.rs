//! 回收站行：实体条目与引用条目。

use super::*;

/// 回收站快照（宿主工作线程用）：本模块的条目 + 别人条目的统计。
///
/// “别人的条目”只给 `origin` 与条数：显示名是宿主的事（crate 不认识别的模块叫什么）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashSnapshot {
    pub rows: Vec<TrashRow>,
    /// `origin` → 条数（不含本模块）。
    pub foreign: Vec<(String, usize)>,
}
/// 回收站条目 → 对话框行（**只收本模块的**：别人的条目在这里没有可做的动作）。
///
/// 排序沿用 `ProjectTrash::list` 的口径（删除时间倒序），这里不再排一次——两处各排一次，
/// 将来口径一变就会打架。
pub fn build_trash_snapshot(entries: &[TrashEntry]) -> TrashSnapshot {
    let mut rows = Vec::new();
    let mut foreign: Vec<(String, usize)> = Vec::new();
    for entry in entries {
        if entity_trash_row(entry).is_none() {
            let origin = entry.manifest.origin.clone();
            match foreign.iter_mut().find(|(o, _)| o == &origin) {
                Some((_, count)) => *count += 1,
                None => foreign.push((origin, 1)),
            }
            continue;
        }
        rows.push(entity_trash_row(entry).expect("上面已判 Some").1);
    }
    TrashSnapshot { rows, foreign }
}
/// 实体回收站条目 → 对话框行（copy 口径：本体在 `ProjectTrash`）。
///
/// 返回删除时间供双来源归并排序；非本模块条目返回 `None`（留给 `build_trash_snapshot`
/// 统计 foreign 条数）。
pub fn entity_trash_row(entry: &TrashEntry) -> Option<(DateTime<Utc>, TrashRow)> {
    let manifest = &entry.manifest;
    if manifest.origin != ORIGIN_RESOURCES {
        return None;
    }
    // “原位置”回答的是“还原到哪”：rel_path 为空（极端旧条目）时退回到文件名。
    let original_label = if manifest.original_rel_path.trim().is_empty() {
        manifest.name.clone()
    } else {
        format!("{RESOURCES_DIR_NAME}/{}", manifest.original_rel_path)
    };
    let row = TrashRow {
        trash_id: manifest.id.clone(),
        name: manifest.name.clone(),
        original_label,
        kind_label: match manifest.kind {
            TrashKind::File => "文件".to_string(),
            TrashKind::Folder => "目录".to_string(),
        },
        // 绝对时间：回收站是查凭证的地方（行上用相对时间的口径不适用于对话框）。
        time_label: format_timestamp(manifest.deleted_at),
        size_label: match manifest.kind {
            TrashKind::File => format_size(Some(manifest.size.min(i32::MAX as u64) as i32)),
            // 目录的“大小”没意义（要递归统计才有值），留空而不是写 0。
            TrashKind::Folder => String::new(),
        },
        payload_mode: PayloadMode::Copy,
        resource_id: String::new(),
        has_payload: true,
    };
    Some((manifest.deleted_at, row))
}
/// reference 软删登记行 → 回收站行（第二来源；§12.1.1 ③）。
///
/// 这类条目没有实体 manifest（本体一直在源位置），回收站清单要把它们一并列出；
/// copy 软删行已由实体条目覆盖，这里只收 reference，避免双来源重复。返回删除时间供归并。
pub fn reference_trash_rows(deleted: &[AnalyticsResource]) -> Vec<(DateTime<Utc>, TrashRow)> {
    deleted
        .iter()
        .filter(|r| PayloadMode::from_db_str(&r.payload_mode) == PayloadMode::Reference)
        .map(|r| {
            let kind = ArchiveKind::from_db_str(&r.kind);
            let original_label = r
                .source_path
                .clone()
                .or_else(|| r.file_rel_path.clone())
                .unwrap_or_else(|| r.name.clone());
            let when = r.deleted_at.unwrap_or(r.updated_at);
            let row = TrashRow {
                trash_id: format!("{TRASH_ID_REF_PREFIX}{}", r.id),
                name: r.name.clone(),
                original_label,
                kind_label: crate::detail_view::kind_label(kind).to_string(),
                time_label: format_timestamp(when),
                size_label: format_size(r.file_size),
                payload_mode: PayloadMode::Reference,
                resource_id: r.id.clone(),
                has_payload: false,
            };
            (when, row)
        })
        .collect()
}

/// 按资源的分组映射：`resource_id → folder_id`（来自 `folders_by_resource`，一次查完；
/// 单层分组下最多一条，所以是 `Option` 语义的映射而不是列表）。
pub type FolderMemberships = std::collections::HashMap<String, String>;
