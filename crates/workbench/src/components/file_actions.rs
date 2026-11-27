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
    ArchiveBinding, ArchiveKind, ArchiveRequest, PayloadMode,
};
use gpui_kit::App;

use crate::panels::Shared;
use crate::services::resource_jobs;

/// 导出的目的地（铁律 E 组第 24 条：导出与「保存到草稿」是同一条通道两个目的地）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportDest {
    /// 系统资源管理：全局目录下的 `results/`
    Global,
    /// 项目子目录 `{项目}/scratchpad/`
    Scratchpad,
}

impl ExportDest {
    pub fn verb(self) -> &'static str {
        match self {
            ExportDest::Global => "导出",
            ExportDest::Scratchpad => "存到草稿",
        }
    }
}

/// 引用名（`"表名"`，见 `data_objects::sql_ident`）→ 裸表名。
///
/// 表名本身可能带引号（`odd"name`）：标识符里的 `""` 必须还原成 `"`，
/// 否则导出的是另一张（不存在的）表。
pub fn bare_table_name(sql_ref: &str) -> String {
    let names = table_ref_parts(sql_ref);
    names.last().cloned().unwrap_or_default()
}

/// 引用名 → 可执行的限定名（`"rds_archive"."arc_1"` → `rds_archive.arc_1`）。
pub fn table_ref_name(sql_ref: &str) -> String {
    table_ref_parts(sql_ref).join(".")
}

fn table_ref_parts(sql_ref: &str) -> Vec<String> {
    sql_ref
        .trim()
        .split('.')
        .map(|part| {
            part.strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(part)
                .replace("\"\"", "\"")
        })
        .collect()
}

/// 把一行对象按 SQL 引用名写到**指定文件**（导出窗口的自定义目录用）。
///
/// 与 [`export_table_to`] 共用同一条写盘路径，只是落点由调用方给全（含扩展名）。
/// 写盘**不在 UI 线程**（铁律 E 组第 25 条）：校验完就交给 [`export_jobs`] 的后台线程，
/// 结果由宿主回执泵经状态栏说出。返回是否**已入队**——窗口据此关不关。
pub fn export_table_to_path(
    shared: &Shared,
    sql_ref: &str,
    path: &Path,
    verb: &str,
    cx: &mut App,
) -> bool {
    let table = table_ref_name(sql_ref);
    if table.trim().is_empty() {
        shared.say_error(format!("{verb}：目标表名为空"), cx);
        return false;
    }
    if path.to_str().is_none() {
        shared.say_error(format!("{verb}：路径含非 UTF-8 字符"), cx);
        return false;
    }
    crate::services::export_jobs::start(table, path.to_path_buf(), verb.to_string());
    shared.say(format!("{verb}：正在后台写 {}", path.display()), cx);
    true
}

/// 文件名只留安全字符（表名里可能有引号 / 空格 / 中文）。
///
/// 导出窗口的「文件名」预填与 [`export_table_to`] 的默认名共用这一份口径：
/// 两处各写一个清洗规则，早晚会一个留中文一个折成下划线。
pub fn export_file_stem(table: &str) -> String {
    table
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// 把一行对象（按它的 SQL 引用名）导出到 `dest`，格式由 `format` 定（csv / parquet / json）。
///
/// 两个目的地 + 三种格式 = 铁律第 24/25 条的完整口径；引擎侧三种都能写，
/// 这里只负责定落点、拼文件名与回执。
pub fn export_table_to(
    shared: &Shared,
    sql_ref: &str,
    dest: ExportDest,
    format: engine::duckdb::export::ExportFormat,
    cx: &mut App,
) {
    let table = table_ref_name(sql_ref);
    let stem = bare_table_name(sql_ref);
    if table.trim().is_empty() {
        shared.say_error(format!("{}：目标表名为空", dest.verb()), cx);
        return;
    }
    let dir = match dest {
        ExportDest::Global => crate::services::query_export::default_export_dir(),
        ExportDest::Scratchpad => {
            let Some(root) = shared.project_root() else {
                shared.say_error("存到草稿：当前没有打开的项目", cx);
                return;
            };
            root.join("scratchpad")
        }
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        shared.say_error(format!("{}：创建目录失败（{error}）", dest.verb()), cx);
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!(
        "{}_{secs}.{}",
        export_file_stem(&stem),
        format.extension()
    ));
    let _ = export_table_to_path(shared, sql_ref, &path, dest.verb(), cx);
}

/// 外部文件存为快照时，副本在 `resources/` 下的落点与显示名。
///
/// 单独摘成纯函数：这两件事是「存为快照」的全部口径（放哪、叫什么），
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

/// 把加源的外部文件存为快照（`Copy`）：写副本 + 生成资产，只读带版本。
pub fn archive_file_as_snapshot(shared: &Shared, path: &str, cx: &mut App) {
    let Some(root) = shared.project_root() else {
        shared.say_error("存快照：当前没有打开的项目", cx);
        return;
    };
    let Some((rel_path, name)) = snapshot_placement(path) else {
        shared.say_error("存快照：这个文件没有可用的文件名", cx);
        return;
    };
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
            existing_resource_id: None,
        },
        // 与洞察导出一致：探结构（归档侧按扩展名走 `file_reader_function`）
        true,
        None,
    );
    shared.say(format!("正在把「{name}」存为快照…"), cx);
    shared.refresh_resources(cx);
}

#[cfg(test)]
mod tests {
    use super::{bare_table_name, export_file_stem, snapshot_placement};

    /// 引用名（`"表名"`）→ 裸表名：引号要脱掉，里面的 `""` 要还原（否则导出另一张表）
    #[test]
    fn quoted_references_come_back_as_table_names() {
        assert_eq!(bare_table_name("\"tmp_q_orders\""), "tmp_q_orders");
        assert_eq!(bare_table_name("\"odd\"\"name\""), "odd\"name");
        // 没引号的（不该出现，但别 panic）
        assert_eq!(bare_table_name("plain"), "plain");
    }

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

    /// 导出文件名只留安全字符：引号 / 空格 / 点折成下划线，中文与 `-` `_` 保留
    /// （导出窗口预填与默认名共用这一份，两处写两套规则早晚一个留中文一个全折）
    #[test]
    fn export_stem_keeps_letters_digits_and_underscores() {
        assert_eq!(export_file_stem("\"tmp_q_orders\""), "_tmp_q_orders_");
        assert_eq!(export_file_stem("odd name.csv"), "odd_name_csv");
        assert_eq!(export_file_stem("经营月报-2026"), "经营月报-2026");
        assert_eq!(export_file_stem(""), "");
    }
}
