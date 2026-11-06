//! 版本行：历史候选与差异摘要。

use super::*;

/// 版本差异的中间态：快照与资源行两种来源先归一，再算差异。
struct Candidate {
    version: i32,
    size: Option<i32>,
    hash: Option<String>,
    time: DateTime<Utc>,
    is_current: bool,
    has_copy: bool,
}
/// 版本历史行（对话框用）：当前版本 + 历史版本，降序。
///
/// 合成两件事：
///
/// 1. **当前版本也占一行**——写前快照语义下版本表里没有当前版本（它在资源行上），
///    只列历史会让人以为“最新版不见了”；
/// 2. **相邻版本差异**（大小 ±N · 指纹是否变）——不做行级 diff，两个值就够一眼看出
///    “这一版改了多大”；
///
/// `payload_ok` = 当前本体在位（当前版本行的“副本”就是本体）；`copies` = 有内容副本的
/// 历史版本号（`PayloadStore::version_copies`）——两者都是宿主在取数线程上问过文件系统
/// 才有的，不在渲染期算。
///
/// 历史行的大小 / 指纹只能从快照 JSON 里取（表结构如此）；解析失败就**留空**，
/// 不拿当前版本的值顶替——那会伪造历史。
pub fn build_version_rows(
    current: &AnalyticsResource,
    versions: &[crate::store::rows::ResourceVersion],
    copies: &[i32],
    payload_ok: bool,
) -> Vec<VersionRow> {
    let mut candidates = vec![Candidate {
        version: current.version,
        size: current.file_size,
        hash: current.content_hash.clone(),
        time: current.archived_at.unwrap_or(current.updated_at),
        is_current: true,
        has_copy: payload_ok,
    }];
    for version in versions {
        // 快照是当时那条资源记录的序列化；字段结构随迁移演进，解析不出来不算错。
        let snapshot = serde_json::from_value::<AnalyticsResource>(version.snapshot.clone()).ok();
        candidates.push(Candidate {
            version: version.version,
            size: snapshot.as_ref().and_then(|record| record.file_size),
            hash: snapshot
                .as_ref()
                .and_then(|record| record.content_hash.clone()),
            time: version.created_at,
            is_current: false,
            has_copy: copies.contains(&version.version),
        });
    }
    candidates.sort_unstable_by(|a, b| b.version.cmp(&a.version));

    candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| VersionRow {
            version: candidate.version,
            is_current: candidate.is_current,
            time_label: format_timestamp(candidate.time),
            size_label: format_size(candidate.size),
            hash_short: crate::detail_view::short_hash(candidate.hash.as_deref()),
            has_copy: candidate.has_copy,
            // 与**上一版**（版本号 -1）比：列表里它的下一行（降序）。
            delta_label: candidates
                .get(index + 1)
                .filter(|previous| previous.version == candidate.version - 1)
                .map(|previous| version_delta(previous.version, previous, candidate))
                .unwrap_or_default(),
        })
        .collect()
}
/// 相邻两版的差异文案：`较 v1 大小+1.2 KB · 指纹已变`。
///
/// 两侧都有值才比（快照里缺字段就不编）；两样都比不出来时给空串。
fn version_delta(previous_version: i32, previous: &Candidate, current: &Candidate) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let (Some(now), Some(before)) = (current.size, previous.size) {
        let delta = now - before;
        parts.push(match delta {
            0 => "大小不变".to_string(),
            delta if delta > 0 => format!("大小+{}", format_size(Some(delta))),
            delta => format!("大小-{}", format_size(Some(-delta))),
        });
    }
    if let (Some(now), Some(before)) = (current.hash.as_deref(), previous.hash.as_deref()) {
        parts.push(
            if now == before {
                "指纹相同"
            } else {
                "指纹已变"
            }
            .to_string(),
        );
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("较 v{previous_version} {}", parts.join(" · "))
}
