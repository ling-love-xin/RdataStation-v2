//! 设置持久化与进程级快照（与具体设置域无关的横切基础设施）。
//!
//! - 磁盘读写：`config_dir` / `settings_path` / `load_settings` / `save_settings`
//!   （原子写：临时文件 + rename，避免进程中途退出留下半截 JSON）。
//! - 进程级快照：`CONNECTION_DEFAULTS` 供**无 `App` 的异步连接路径**读取；
//!   `LAST_SAVE_ERROR` 供界面提示上一次落盘失败原因（落盘失败不影响本进程生效）。

use std::path::PathBuf;
use std::sync::RwLock;

use crate::model::{ConnectionDefaults, Settings};

/// 进程级连接默认值快照：供**无 `App` 的异步连接路径**（`ConnectionService`）读取。
///
/// 由 `save_settings` / `SettingsService::init` 发布：async 上下文拿不到 GPUI global，
/// 但又需要「建连超时 / LAN 关 TLS」这两个参数。
static CONNECTION_DEFAULTS: RwLock<Option<ConnectionDefaults>> = RwLock::new(None);

/// 上一次落盘失败的原因（`None` = 上一次落盘成功 / 还没写过）。
///
/// 为什么要有这个槽：落盘失败**不影响本进程生效**（值已经进了 global），
/// 但用户必须看得见——否则"改了没存住"是完全静默的（架构 §13 K2）。
static LAST_SAVE_ERROR: RwLock<Option<String>> = RwLock::new(None);

/// 读取连接默认值（未发布时回退模型默认）。
pub fn connection_defaults() -> ConnectionDefaults {
    CONNECTION_DEFAULTS
        .read()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
}

/// 发布连接默认值快照（内部使用；`save_settings` / `init` 调用）。
pub(crate) fn publish_connection_defaults(defaults: &ConnectionDefaults) {
    if let Ok(mut guard) = CONNECTION_DEFAULTS.write() {
        *guard = Some(defaults.clone());
    }
}

/// 全局设置目录：`<RDS_HOME>/config`（默认 RDS_HOME = 可执行文件所在目录，即安装目录）。
///
/// 路径口径全在 [`paths`]：本 crate **不自拼** `APPDATA` / 目录名（见 `docs/architecture/runtime/data-paths.md`）。
/// 测试构建的数据根本身由 `paths` 的 `test-support` 隔离（各成员在 `[dev-dependencies]` 打开），
/// 因此这里不需要任何测试专用分支。
pub fn config_dir() -> PathBuf {
    paths::config_dir()
}

/// 配置文件路径。
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// 从指定路径加载设置；文件不存在或解析失败时回退默认值（**不覆盖**坏文件）。
pub fn load_settings_from(path: &std::path::Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

/// 从磁盘加载设置（用户配置目录）。
pub fn load_settings() -> Settings {
    load_settings_from(&settings_path())
}

/// 保存到指定路径：**原子写**（临时文件 + rename）。
///
/// 为什么不是直写：直写时进程在写一半退出，`settings.json` 就会是半截 JSON——
/// 下次启动解析失败即回退默认，等于把用户配置全丢了。
pub fn save_settings_to(path: &std::path::Path, settings: &Settings) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("配置路径没有父目录：{}", path.display()))?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| format!("序列化失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    let result = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&tmp, &text))
        .and_then(|()| std::fs::rename(&tmp, path));
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // 失败的临时文件不留在用户配置目录里。
            let _ = std::fs::remove_file(&tmp);
            Err(format!("未能写入 {}：{e}", path.display()))
        }
    }
}

/// 上一次落盘失败的原因（不清除；下一次成功后自动清空）。
pub fn last_save_error() -> Option<String> {
    LAST_SAVE_ERROR.read().ok().and_then(|g| g.clone())
}

/// 记录落盘结果。
pub(crate) fn set_last_save_error(err: Option<String>) {
    if let Ok(mut guard) = LAST_SAVE_ERROR.write() {
        *guard = err;
    }
}

/// 保存设置到磁盘（幂等）。
///
/// 失败时**不改本进程已生效的值**（已进 global），只记录原因供界面提示
/// （[`last_save_error`]）。
pub fn save_settings(settings: &Settings) -> Result<(), String> {
    // 先发布快照：本进程内的消费方（异步连接路径等）不受落盘结果影响。
    publish_connection_defaults(&settings.connection_defaults);
    let result = save_settings_to(&settings_path(), settings);
    set_last_save_error(result.as_ref().err().cloned());
    result
}

/// 落盘并忽略返回值（失败原因已记进 `LAST_SAVE_ERROR`，由设置页展示）。
///
/// 各 `set_*` 走这里：它们不因落盘失败而回滚内存值，也不向调用方改签名。
pub(crate) fn persist(settings: &Settings) {
    let _ = save_settings(settings);
}
