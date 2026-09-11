//! 项目锁：防止同一项目被多个应用实例同时写入。
//!
//! 采用**操作系统级文件锁**（`std::fs::File::try_lock`）：进程退出（含崩溃）时由
//! 内核自动释放，不存在"残留锁文件导致永久无法打开"的问题。
//!
//! 锁与占用者信息分两个文件：
//! - `project.lock`（OS 独占锁，内容不参与判定）；
//! - `project.lock.owner`（占用者 pid / 时间，仅供 UI 展示，无锁、可自由读）。
//!
//! 之所以不把占用者信息写进被锁文件：Windows 上独占字节锁会同时阻止其他句柄读取
//! 锁定区间，导致占用方信息读不到。语义见原型设计 §1.1 / §8。

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use shared::error::CoreError;

/// 锁文件名（位于项目 `.RSmeta/` 下）。
pub const LOCK_FILE_NAME: &str = "project.lock";
/// 占用者信息文件名（与锁文件同目录）。
pub const OWNER_FILE_NAME: &str = "project.lock.owner";

/// 内部元数据目录名（与 `store.rs` 保持一致，避免各写各的）。
const META_DIR: &str = ".RSmeta";

/// 锁占用者信息（用于 UI 提示，不参与锁判定——判定由 OS 文件锁完成）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub acquired_at: DateTime<Utc>,
}

/// 获取锁的结果。
pub enum AcquireOutcome {
    /// 获取成功；句柄存活期间持锁，`Drop` 即释放。
    Acquired(ProjectLock),
    /// 已被其他实例占用（携带占用者信息）。
    Busy(LockInfo),
}

/// 锁持有句柄。持有期间独占；`Drop`（关闭文件）由 OS 释放锁。
pub struct ProjectLock {
    #[allow(dead_code)]
    file: File,
    lock_path: PathBuf,
    owner_path: PathBuf,
}

impl ProjectLock {
    /// 项目锁文件路径：`{root}/.RSmeta/project.lock`。
    pub fn lock_path(root: &Path) -> PathBuf {
        root.join(META_DIR).join(LOCK_FILE_NAME)
    }

    /// 占用者信息文件路径：`{root}/.RSmeta/project.lock.owner`。
    pub fn owner_path(root: &Path) -> PathBuf {
        root.join(META_DIR).join(OWNER_FILE_NAME)
    }

    /// 尝试获取项目写锁。
    ///
    /// - `Ok(Acquired)`：成功，持有句柄；
    /// - `Ok(Busy)`：已被其他实例占用（调用方据此弹出「只读打开 / 仍要打开」）；
    /// - `Err`：文件系统错误。
    pub fn acquire(root: &Path) -> Result<AcquireOutcome, CoreError> {
        let lock_path = Self::lock_path(root);
        let owner_path = Self::owner_path(root);
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                engine::persistence::io_to_core_error(e, parent, "create lock parent dir")
            })?;
        }

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| engine::persistence::io_to_core_error(e, &lock_path, "open project lock"))?;

        match file.try_lock() {
            Ok(()) => {
                write_owner(&owner_path);
                Ok(AcquireOutcome::Acquired(ProjectLock {
                    file,
                    lock_path,
                    owner_path,
                }))
            }
            Err(TryLockError::WouldBlock) => Ok(AcquireOutcome::Busy(read_owner(&owner_path))),
            Err(TryLockError::Error(e)) => {
                Err(engine::persistence::io_to_core_error(e, &lock_path, "lock project"))
            }
        }
    }

    /// 只读探测：不获取锁，仅返回当前占用者（`None` = 空闲）。
    ///
    /// 供选择器展示项目卡片时判断"是否被其他实例占用"。
    pub fn probe(root: &Path) -> Result<Option<LockInfo>, CoreError> {
        let lock_path = Self::lock_path(root);
        if !lock_path.exists() {
            return Ok(None);
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| engine::persistence::io_to_core_error(e, &lock_path, "open project lock"))?;

        match file.try_lock() {
            Ok(()) => {
                // 能加锁 → 说明无人占用；立刻释放探测锁。
                let _ = file.unlock();
                Ok(None)
            }
            Err(TryLockError::WouldBlock) => Ok(Some(read_owner(&Self::owner_path(root)))),
            Err(TryLockError::Error(e)) => {
                Err(engine::persistence::io_to_core_error(e, &lock_path, "probe project lock"))
            }
        }
    }

    /// 主动释放并清理锁文件（正常关闭项目时调用；`Drop` 亦会释放，此处额外删文件）。
    pub fn release(self) -> Result<(), CoreError> {
        let ProjectLock {
            file,
            lock_path,
            owner_path,
        } = self;
        let _ = file.unlock();
        drop(file);
        let _ = std::fs::remove_file(&owner_path);
        if lock_path.exists() {
            std::fs::remove_file(&lock_path).map_err(|e| {
                engine::persistence::io_to_core_error(e, &lock_path, "remove project lock")
            })?;
        }
        Ok(())
    }
}

/// 写入占用者信息（覆盖旧内容）。
fn write_owner(path: &Path) {
    let info = LockInfo {
        pid: std::process::id(),
        acquired_at: Utc::now(),
    };
    if let Ok(json) = serde_json::to_string(&info) {
        if let Ok(mut f) = File::create(path) {
            let _ = writeln!(f, "{json}");
        }
    }
}

/// 读取占用者信息；缺失/损坏时降级为 pid=0（UI 仅提示，不参与判定）。
fn read_owner(path: &Path) -> LockInfo {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(s.trim()).ok())
        .unwrap_or(LockInfo {
            pid: 0,
            acquired_at: Utc::now(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_project_lock_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn acquire_then_busy_then_free() {
        let root = temp_dir("basic");

        // 首次获取成功。
        let first = match ProjectLock::acquire(&root).expect("acquire") {
            AcquireOutcome::Acquired(lock) => lock,
            AcquireOutcome::Busy(_) => panic!("首次获取不应被占用"),
        };

        // 第二次获取（同进程新句柄）应判定为占用，且能读到占用者 pid。
        match ProjectLock::acquire(&root).expect("second acquire") {
            AcquireOutcome::Busy(info) => assert_eq!(info.pid, std::process::id()),
            AcquireOutcome::Acquired(_) => panic!("第二次获取不应成功"),
        }

        // probe 也应报告占用。
        assert!(ProjectLock::probe(&root).expect("probe").is_some());

        drop(first);
    }

    #[test]
    fn release_then_reacquire() {
        let root = temp_dir("release");
        let lock = match ProjectLock::acquire(&root).expect("acquire") {
            AcquireOutcome::Acquired(lock) => lock,
            AcquireOutcome::Busy(_) => panic!("首次获取不应被占用"),
        };
        lock.release().expect("release");

        assert!(ProjectLock::probe(&root).expect("probe").is_none());
        match ProjectLock::acquire(&root).expect("re-acquire") {
            AcquireOutcome::Acquired(_) => {}
            AcquireOutcome::Busy(_) => panic!("释放后应可重新获取"),
        }
    }
}
