//! 改造前的历史位置。
//!
//! **只给一次性迁移用**（见 [`crate::migrate`]）；新代码一律走
//! [`crate::config_dir`] / [`crate::data_dir`] / [`crate::log_dir`] / [`crate::temp_dir`] /
//! [`crate::extensions_dir`]。
//!
//! 改造前的事实（`crates/settings`、`crates/engine`、`crates/shared` 里散落的拼路径）：
//!
//! | 内容 | 位置 |
//! | --- | --- |
//! | 全局设置 | `%APPDATA%/RdataStation/settings.json` |
//! | 全局数据（global.db / analytics.duckdb / system/ / metadata/） | `dirs::data_dir()/RdataStation`（Windows 下与设置**同**目录） |
//! | 密钥库（encryption-salt / machine-id） | `dirs::data_local_dir()/RdataStation`（Windows = `%LOCALAPPDATA%`） |
//! | DuckDB 扩展 | `~/.rdatastation/duckdb/extensions` |
//! | 回退位置（上面取不到时） | `%TEMP%/RdataStation` |

use std::path::PathBuf;

/// 旧布局的应用目录名（`RdataStation`）。
const LEGACY_APP_DIR_NAME: &str = "RdataStation";
/// 旧 DuckDB 扩展位置的相对路径（小写，与旧代码里的字面量一致）。
const LEGACY_HOME_EXTENSIONS: &str = ".rdatastation/duckdb/extensions";

/// 漫游应用数据目录：`%APPDATA%/RdataStation`（设置与全局数据都在这里）。
pub fn roaming_app_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(LEGACY_APP_DIR_NAME))
}

/// 本地应用数据目录：`%LOCALAPPDATA%/RdataStation`（密钥库在这里）。
pub fn local_app_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(LEGACY_APP_DIR_NAME))
}

/// 旧临时回退目录：`%TEMP%/RdataStation`。
///
/// 取的是**重定向前**的 `TEMP`（`install_process_temp_dir` 已把进程的 `TEMP` 改成新根）；
/// 若迁移发生在重定向之前，直接用当前 `TEMP`。
pub fn temp_app_dir() -> Option<PathBuf> {
    let base = crate::previous_temp()
        .or_else(|| std::env::var_os("TEMP").map(PathBuf::from))
        .or_else(|| std::env::var_os("TMP").map(PathBuf::from))?;
    Some(base.join(LEGACY_APP_DIR_NAME))
}

/// 旧 DuckDB 扩展目录：`~/.rdatastation/duckdb/extensions`。
pub fn home_extensions_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(LEGACY_HOME_EXTENSIONS))
}
