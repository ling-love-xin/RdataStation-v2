//! 旧布局 → 新布局的一次性迁移。
//!
//! 只在 `crates/app/src/main.rs` 启动时**显式**调用；库内不自动触发
//! （否则 `cargo test` 会把开发机的真实数据搬进测试用的数据根）。
//!
//! ## 规则
//!
//! 1. **只补不盖**：目标文件已存在就跳过——用户在新位置改过的设置 / 数据不会被旧文件回冲。
//! 2. **复制不移动**：旧目录原样留着，出问题可回滚（确认无误后用户自行删除）。
//! 3. **一次性**：迁完在数据根写标记文件，之后启动不再扫描旧位置。
//! 4. **迁移敏感**：`encryption-salt` 与 `machine-id` 是连接密码的密钥派生输入，
//!    不跟着搬 = 旧密文全部解不开（密码看起来"失效"），所以它们必须一起迁。

use std::path::{Path, PathBuf};

use crate::legacy;
use crate::{NEW_LAYOUT_DIRS, config_dir, data_dir, extensions_dir, home};

/// 迁移完成标记（位于数据根，不进 git）。
const MARKER_FILE: &str = ".migrated-from-legacy";

/// 迁移结果。
#[derive(Debug, Default, Clone)]
pub struct MigrationReport {
    /// 标记文件在位，本次没有扫描旧位置。
    pub already_done: bool,
    /// 已复制的（源 → 目标）。
    pub copied: Vec<(PathBuf, PathBuf)>,
    /// 目标已存在而跳过的条目数。
    pub skipped: usize,
    /// 失败项（迁移不阻断启动：失败只让用户暂时看不到旧数据）。
    pub errors: Vec<String>,
}

impl MigrationReport {
    pub fn is_empty(&self) -> bool {
        self.copied.is_empty() && self.errors.is_empty()
    }

    /// 一行摘要（启动日志用）。
    pub fn summary(&self) -> String {
        if self.already_done {
            return format!("旧数据迁移：已完成过，跳过（{}）", marker_path().display());
        }
        if self.copied.is_empty() && self.errors.is_empty() {
            return "旧数据迁移：未发现旧位置的数据".to_string();
        }
        let mut text = format!(
            "旧数据迁移：复制 {} 项到 {}（跳过 {} 项已存在）",
            self.copied.len(),
            home().display(),
            self.skipped
        );
        if !self.errors.is_empty() {
            text.push_str(&format!("，失败 {} 项", self.errors.len()));
            for e in self.errors.iter().take(5) {
                text.push_str(&format!("\n  - {e}"));
            }
        }
        text
    }
}

/// 把旧布局里的数据搬到新数据根（只补不盖，一次性）。
pub fn migrate_legacy_layout() -> MigrationReport {
    let mut report = MigrationReport::default();

    if marker_path().exists() {
        report.already_done = true;
        return report;
    }

    for (src, dest) in plan() {
        if !src.exists() {
            continue;
        }
        copy_item(&src, &dest, &mut report);
    }

    // 标记写在最后：中途失败也留在"未标"状态，下次启动重试（幂等）。
    if let Err(e) = std::fs::create_dir_all(home()) {
        report
            .errors
            .push(format!("创建数据根 {} 失败：{e}", home().display()));
        return report;
    }
    let mut body = String::from("旧布局数据已迁移到本目录（只补不盖，源目录保留）。\n\n");
    for (src, dest) in &report.copied {
        body.push_str(&format!("{}  ->  {}\n", src.display(), dest.display()));
    }
    if let Err(e) = std::fs::write(marker_path(), body) {
        report.errors.push(format!("写迁移标记失败：{e}"));
    }

    report
}

/// 迁移标记文件路径。
pub fn marker_path() -> PathBuf {
    home().join(MARKER_FILE)
}

/// 目标清单：旧目录顶层条目 → 新目录。
fn plan() -> Vec<(PathBuf, PathBuf)> {
    let mut items = Vec::new();

    // 漫游目录：`settings.json` 进 config，其余（global.db / system/ / metadata/ …）进 data。
    if let Some(src) = legacy::roaming_app_dir() {
        collect_top_level(&src, &data_dir(), &config_dir(), &mut items);
    }
    // 本地目录：encryption-salt / machine-id，都进 data。
    if let Some(src) = legacy::local_app_dir() {
        collect_top_level(&src, &data_dir(), &data_dir(), &mut items);
    }
    // 旧临时回退目录：旧代码在"数据目录不可用"时的落点，一并捞回来。
    if let Some(src) = legacy::temp_app_dir() {
        collect_top_level(&src, &data_dir(), &config_dir(), &mut items);
    }
    // DuckDB 扩展：`~/.rdatastation/duckdb/extensions` → `<RDS_HOME>/extensions`。
    if let Some(src) = legacy::home_extensions_dir() {
        if src.exists() && src != extensions_dir() {
            items.push((src, extensions_dir()));
        }
    }

    items
}

/// 收集 `src` 顶层条目。
///
/// 跳过新布局自己的目录名：`RDS_HOME` 万一就落在旧目录上（例如用户把 `RDS_HOME`
/// 指向 `%LOCALAPPDATA%/RdataStation`），不跳就会把 `data/` 再搬进 `data/data/`。
pub(crate) fn collect_top_level(
    src: &Path,
    data_dest: &Path,
    config_dest: &Path,
    items: &mut Vec<(PathBuf, PathBuf)>,
) {
    let Ok(entries) = std::fs::read_dir(src) else {
        return; // 旧目录不存在 = 没什么可搬
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == data_dest || path == config_dest {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if NEW_LAYOUT_DIRS.contains(&name.as_ref()) {
            continue;
        }
        let dest_root = if name == "settings.json" {
            config_dest
        } else {
            data_dest
        };
        items.push((path, dest_root.join(name.as_ref())));
    }
}

pub(crate) fn copy_item(src: &Path, dest: &Path, report: &mut MigrationReport) {
    if src.is_dir() {
        copy_dir(src, dest, report);
    } else {
        copy_file(src, dest, report);
    }
}

pub(crate) fn copy_dir(src: &Path, dest: &Path, report: &mut MigrationReport) {
    let entries = match std::fs::read_dir(src) {
        Ok(entries) => entries,
        Err(e) => {
            report.errors.push(format!("读取 {} 失败：{e}", src.display()));
            return;
        }
    };
    for entry in entries.flatten() {
        copy_item(&entry.path(), &dest.join(entry.file_name()), report);
    }
}

pub(crate) fn copy_file(src: &Path, dest: &Path, report: &mut MigrationReport) {
    if dest.exists() {
        report.skipped += 1;
        return;
    }
    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            report
                .errors
                .push(format!("创建 {} 失败：{e}", parent.display()));
            return;
        }
    }
    match std::fs::copy(src, dest) {
        Ok(_) => report.copied.push((src.to_path_buf(), dest.to_path_buf())),
        Err(e) => report
            .errors
            .push(format!("复制 {} 失败：{e}", src.display())),
    }
}
