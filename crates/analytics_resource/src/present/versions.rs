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
/// `payload_ok` = 当前本体在位（当前版本行的“可还原内容”就是本体）；`file_versions` =
/// 有 manifest 的文件版本索引——两者都是宿主在取数线程上查过文件系统 / 库才有的，不在渲染期算。
///
/// 历史行的大小 / 指纹只能从快照 JSON 里取（表结构如此）；解析失败就**留空**，
/// 不拿当前版本的值顶替——那会伪造历史。
pub fn build_version_rows(
    current: &AnalyticsResource,
    versions: &[crate::store::rows::ResourceVersion],
    file_versions: &[crate::store::file_asset::FileAssetVersion],
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
    // 表型资产没有副本文件：它能不能回退，取决于**快照里记没记本体表名**
    // （回退 = 把表名指回旧表，表还在项目库里，不需要副本）。
    let is_table = crate::ArchiveKind::resolve(&current.kind, current.analytics_table.as_deref())
        == crate::ArchiveKind::AnalyticsTable;
    let is_global_ref = crate::ArchiveKind::from_db_str(&current.kind)
        == crate::ArchiveKind::TableRef
        && current.origin_kind.as_deref() == Some(crate::model::ORIGIN_KIND_GLOBAL);
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
            has_copy: if is_table {
                crate::table_asset::snapshot_table(&version.snapshot).is_some()
            } else {
                file_versions.iter().any(|file| file.version == version.version)
            },
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
            is_table,
            is_global_ref,
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
    version_delta_label(
        previous_version,
        previous.size,
        previous.hash.as_deref(),
        current.size,
        current.hash.as_deref(),
    )
}

/// 差异文案的实参版：版本对话框的表格行与**详情时间线的节点**共用这一份。
///
/// 两处各写一份，文案迟早漂（一处说“大小+1.2 KB”、另一处忘了比指纹）。
pub(crate) fn version_delta_label(
    previous_version: i32,
    previous_size: Option<i32>,
    previous_hash: Option<&str>,
    size: Option<i32>,
    hash: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let (Some(now), Some(before)) = (size, previous_size) {
        let delta = now - before;
        parts.push(match delta {
            0 => "大小不变".to_string(),
            delta if delta > 0 => format!("大小+{}", format_size(Some(delta))),
            delta => format!("大小-{}", format_size(Some(-delta))),
        });
    }
    if let (Some(now), Some(before)) = (hash, previous_hash) {
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
