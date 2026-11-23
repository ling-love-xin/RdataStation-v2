//! 数据对象「文件段」的动作宿主：把**加源进来的外部文件**固定成快照。
//!
//! 铁律 D 组第 16 条：外部文件默认按需（`f_*` 视图，文件变了结果跟着变）；
//! 想固定就归档成快照（`payload_mode = Copy`，资源库里留一份副本，资产带版本）。
//! 这里只做「归档」这一步——归档之后资产自己会出现在文件段（`asset:` 行），
//! 加源那份视图**不删**：用户可能还要按需读原文件，清场口在切项目时统一收（F 组）。
//!
//! 与洞察 / 资源面板共用同一条归档队列（`analytics_resource::jobs::archive`），
//! 回执走状态栏——面板只有 280px，不再往里塞提示行。

use std::path::Path;

use analytics_resource::model::{
    ArchiveBinding, ArchiveKind, ArchiveRequest, KeepVersions, PayloadMode,
};
use gpui_kit::App;

use crate::panels::Shared;
use crate::services::resource_jobs;

/// 外部文件归档成快照时，副本在 `resources/` 下的落点与显示名。
///
/// 单独摘成纯函数：这两件事是「归档成快照」的全部口径（放哪、叫什么），
/// 不该等到 worker 跑起来才发现名字是空的。
pub fn snapshot_placement(path: &str) -> Option<(String, String)> {
    let file_name = Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())?;
    let stem = Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| file_name.clone());
    Some((format!("files/{file_name}"), stem))
}

/// 把加源的外部文件归档成快照（`Copy`）：写副本 + 生成资产，只读带版本。
pub fn archive_file_as_snapshot(shared: &Shared, path: &str, cx: &mut App) {
    let Some(root) = shared.project_root() else {
        shared.say_error("归档：当前没有打开的项目", cx);
        return;
    };
    let Some((rel_path, name)) = snapshot_placement(path) else {
        shared.say_error("归档：这个文件没有可用的文件名", cx);
        return;
    };
    // 保留策略在主线程读好随闭包带入：worker 里拿不到 GPUI global 设置
    let keep_versions = KeepVersions::from_setting(settings::SettingsService::keep_versions(cx));
    resource_jobs::enqueue_archive(
        root,
        false,
        ArchiveRequest {
            source_path: path.into(),
            rel_path,
            name: name.clone(),
            alias: None,
            kind: ArchiveKind::File,
            // 快照 = 副本，不是引用：这是「想固定」唯一能让文件段标「快照」的路径
            payload_mode: PayloadMode::Copy,
            // 探结构（列类型）交给归档侧：探到就把 `structured` 记 1，文件段才认它是数据
            analysis: None,
            binding: ArchiveBinding {
                promoted_from: None,
                source_connection_id: None,
                source_table: None,
            },
            tags: vec!["外部文件".to_string()],
            group_id: None,
            keep_versions: None,
            existing_resource_id: None,
        },
        keep_versions,
        // 与洞察导出一致：探结构（归档侧按扩展名走 `file_reader_function`）
        true,
        None,
    );
    shared.say(format!("正在把「{name}」归档为快照…"), cx);
    shared.refresh_resources(cx);
}

#[cfg(test)]
mod tests {
    use super::snapshot_placement;

    #[test]
    fn placement_keeps_the_file_name_and_uses_the_stem_as_label() {
        assert_eq!(
            snapshot_placement("/data/orders_2026.csv"),
            Some(("files/orders_2026.csv".to_string(), "orders_2026".to_string()))
        );
        // 中文名照搬，不折成 `f_file`（那是视图名的口径，不是资产显示名）
        assert_eq!(
            snapshot_placement("D:/data/经营月报.xlsx"),
            Some(("files/经营月报.xlsx".to_string(), "经营月报".to_string()))
        );
        assert_eq!(snapshot_placement(""), None);
    }
}
