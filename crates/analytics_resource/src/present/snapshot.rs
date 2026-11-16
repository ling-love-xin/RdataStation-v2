//! 面板快照：行、分组、标签、计数汇总。

use super::*;

/// 组装面板快照：行（按状态与种类计分）+ 计数 + 只读标志 + 标签字典。
///
/// 计数口径与状态行文案一一对应：`缺失` 与 `索引异常`（内容已变）各自计数，
/// 因为它们在上是两个不同的可点入口（都进索引修复，但处理方式不同）。
pub fn build_snapshot(inputs: SnapshotInputs<'_>) -> ResourcesSnapshot {
    let SnapshotInputs {
        resources,
        global_rows,
        global_resources,
        statuses,
        history_counts,
        recent_versions,
        tags: tags_by_resource,
        tag_dictionary,
        folders: folders_by_resource,
        group_dictionary,
        previews,
        assertions,
        lifecycles,
        dispatch_usage,
        read_only,
        now,
    } = inputs;
    let mut counts = ArchiveCounts {
        total: resources.len(),
        ..ArchiveCounts::default()
    };
    let mut rows = Vec::with_capacity(resources.len());
    let mut details = std::collections::HashMap::with_capacity(resources.len());

    for resource in resources {
        let row = to_row(
            resource,
            statuses,
            tags_by_resource,
            folders_by_resource,
            now,
        );
        match row.status {
            ArchiveStatus::Missing => counts.missing += 1,
            ArchiveStatus::ContentChanged => counts.drifted += 1,
            ArchiveStatus::Normal => match row.kind {
                ArchiveKind::File => counts.archived += 1,
                ArchiveKind::AnalyticsTable => counts.analytics_table += 1,
                ArchiveKind::TableRef => counts.table_ref += 1,
            },
        }
        let history_count = history_counts.get(&resource.id).copied().unwrap_or(0);
        // 「最近 3 条」明细：同一批取数（`recent_versions_by_resource`）按行给，
        // 没有历史的存档拿到空切片 → 详情只留「只有当前版本」一行。
        let recent = recent_versions
            .get(&resource.id)
            .map(|versions| versions.as_slice())
            .unwrap_or_default();
        // 标签 chips 就用行上那一份（同一装配来源，不会出现“行里有、详情里没有”）。
        // 预览同理：宿主没读到就给缺省（「仅元信息」），不在渲染期补读数。
        let preview = previews.get(&resource.id).cloned().unwrap_or_default();
        // 分组名从“按行的归属 + 分组字典”拼：认不出的 id 当未分组（与 `filter` 的脏数据口径一致）。
        let folder_id = folders_by_resource.get(&resource.id).cloned();
        let group = folder_id
            .as_deref()
            .and_then(|id| group_dictionary.iter().find(|group| group.id == id))
            .map(|group| group.name.clone());
        // 「移动…」菜单的选择项：「未分组」不进列表（菜单里恒有那一项）。
        let groups: Vec<DetailGroupOption> = group_dictionary
            .iter()
            .map(|group| DetailGroupOption {
                id: group.id.clone(),
                name: group.name.clone(),
            })
            .collect();
        let mut detail = to_detail(
            resource,
            row.status,
            history_count,
            recent,
            DetailExtras {
                tags: row.tags.clone(),
                group,
                group_id: folder_id,
                groups,
                preview,
                // 项目行没有「被谁用过」这回事：它的来源在 `promoted_from`。
                dispatch_label: String::new(),
            },
        );
        // 业务断言与效力随快照下发：详情面板只读，不在渲染期查库。
        detail.assertion = assertions.get(&resource.id).cloned();
        detail.lifecycle = lifecycles
            .get(&resource.id)
            .copied()
            .unwrap_or_default();
        details.insert(resource.id.clone(), detail);
        rows.push(row);
    }

    // 全局行详情（§13 收尾 A4）：轻量基本信息区——全局共享层无项目侧标签 / 分组 / 预览，
    // 历史版本计数也不进项目版本表；动作区由渲染层按 `scope` 收敛到「分发到本项目」。
    for resource in &global_resources {
        let preview = previews.get(&resource.id).cloned().unwrap_or_default();
        details.insert(
            resource.id.clone(),
            to_detail(
                resource,
                ArchiveStatus::Normal,
                0,
                &[],
                DetailExtras {
                    tags: Vec::new(),
                    group: None,
                    group_id: None,
                    groups: Vec::new(),
                    preview,
                    // 全局行才有的那一行「谁在用」：没人用过给空串（详情不画空行）。
                    dispatch_label: dispatch_detail_label(dispatch_usage.get(&resource.name)),
                },
            ),
        );
    }

    ResourcesSnapshot {
        rows,
        assertion_index: assertions
            .iter()
            .map(|(id, assertion)| (id.clone(), assertion_search_text(assertion)))
            .collect(),
        global_rows,
        counts,
        read_only,
        details,
        tags: tag_dictionary,
        groups: group_dictionary,
    }
}

/// 全局行的使用面摘要；没人用过（或没有回执表）给空串——详情不画那一行。
///
/// 时间只取 SQLite 原始串的**日期**部分：这行回答的是「最近什么时候用过」，
/// 精确到秒没有信息增量，还会把行拉长。
pub fn dispatch_detail_label(usage: Option<&crate::global::DispatchUsage>) -> String {
    let Some(usage) = usage else {
        return String::new();
    };
    if usage.project_count == 0 {
        return String::new();
    }
    let date = usage
        .last_distributed_at
        .get(..10)
        .unwrap_or(&usage.last_distributed_at);
    format!(
        "{} 个项目在用 · 最近 v{} · 累计 {} 次分发 · 最近 {}",
        usage.project_count, usage.latest_version, usage.total_dispatches, date
    )
}
