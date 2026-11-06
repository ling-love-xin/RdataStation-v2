//! 标签对话框取数：全量标签 + 这批判目标各自已挂的。

use engine::persistence::project_db::ProjectDatabaseManager;

use super::*;
use crate::dialogs::tag::TagDialogSeed;

/// 取标签词典 + 这批目标各自已挂的 → 对话框取数结果（工作线程上执行）。
///
/// 多选的两份集合：**全有**（交集，显示为勾上）与**部分**（并集 − 交集，显示为「部分」）。
/// 交集为空说明这批目标没有共同标签；并集为空说明都没打过标签——两种都不算错误。
pub(crate) async fn load_tag_rows(job: &TagListJob) -> Result<TagRows, String> {
    let manager = ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|e| format!("打开项目库失败：{e}"))?;
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());

    let tags = store
        .list_tags(Some("project"))
        .await
        .map_err(|e| format!("读取标签失败：{e}"))?;
    let counts = store.tag_usage_counts().await.unwrap_or_default();

    // 逐目标取已挂标签：并集入 seen，交集用“出现过几次 = 目标数”判定。
    let mut first: Option<Vec<String>> = None;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut hit_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let total = job.targets.len();
    for target in &job.targets {
        let ids: Vec<String> = store
            .get_tags_for_resource(&target.id)
            .await
            .map_err(|e| format!("读取存档标签失败：{e}"))?
            .into_iter()
            .map(|tag| tag.id)
            .collect();
        for id in &ids {
            seen.insert(id.clone());
            *hit_counts.entry(id.clone()).or_insert(0) += 1;
        }
        let set: std::collections::HashSet<String> = ids.into_iter().collect();
        first = Some(match first {
            Some(prev) => prev.into_iter().filter(|id| set.contains(id)).collect(),
            None => set.into_iter().collect(),
        });
    }
    let selected: Vec<String> = first.unwrap_or_default();
    let partial: Vec<String> = seen
        .into_iter()
        .filter(|id| hit_counts.get(id).copied().unwrap_or(0) < total && !selected.contains(id))
        .collect();
    let resource_name = match job.targets.as_slice() {
        [one] => one.name.clone(),
        many => format!("{} 项", many.len()),
    };

    Ok(TagRows {
        targets: job.targets.clone(),
        seed: TagDialogSeed {
            resource_name,
            target_count: total,
            options: crate::present::tag_options(&tags, &counts)
                .into_iter()
                .map(|option| TagChoice {
                    id: option.id,
                    name: option.name,
                    count: option.count,
                })
                .collect(),
            selected,
            partial,
        },
    })
}
