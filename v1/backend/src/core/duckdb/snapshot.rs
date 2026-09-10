use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::core::error::{CommonError, CoreError};

/// 快照信息
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    /// 快照名称
    pub name: String,
    /// 快照文件路径
    pub path: PathBuf,
    /// 创建时间（Unix 时间戳）
    pub created_at: u64,
    /// 快照大小（字节）
    pub size_bytes: u64,
    /// 备注
    pub description: Option<String>,
}

impl SnapshotInfo {
    /// 从文件系统加载快照信息。
    pub fn from_path(path: &Path) -> Result<Self, CoreError> {
        let metadata = std::fs::metadata(path).map_err(|e| {
            CoreError::common(CommonError::General(format!("获取快照元数据失败: {}", e)))
        })?;

        let created_at = metadata
            .modified()
            .unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        Ok(SnapshotInfo {
            name,
            path: path.to_path_buf(),
            created_at,
            size_bytes: metadata.len(),
            description: None,
        })
    }
}

/// 快照管理器
///
/// 负责 DuckDB 数据库的快照创建、恢复、删除和列表管理。
///
/// # 快照策略
/// - 快照是数据库文件的完整副本
/// - 快照文件名格式: `{db_name}_snapshot_{timestamp}.duckdb`
/// - 快照存储在同一目录的 `snapshots/` 子目录中
pub struct SnapshotManager {
    /// 快照存储目录
    snapshot_dir: PathBuf,
    /// 最大快照数量（默认10）
    max_snapshots: usize,
}

impl SnapshotManager {
    /// 创建新的快照管理器。
    ///
    /// # 参数
    /// - `db_path`: DuckDB 数据库文件路径
    /// - `max_snapshots`: 最大快照数量
    pub fn new<P: AsRef<Path>>(db_path: P, max_snapshots: usize) -> Result<Self, CoreError> {
        let db_path = db_path.as_ref();
        let db_dir = db_path
            .parent()
            .ok_or_else(|| CoreError::common(CommonError::General("数据库路径无效".to_string())))?;

        let snapshot_dir = db_dir.join("snapshots");

        // 确保快照目录存在
        std::fs::create_dir_all(&snapshot_dir).map_err(|e| {
            CoreError::common(CommonError::General(format!("创建快照目录失败: {}", e)))
        })?;

        Ok(SnapshotManager {
            snapshot_dir,
            max_snapshots,
        })
    }

    /// 创建数据库快照。
    ///
    /// # 参数
    /// - `db_path`: 源数据库文件路径
    /// - `description`: 快照描述（可选）
    ///
    /// # 返回
    /// - `Ok(SnapshotInfo)`: 创建的快照信息
    /// - `Err(CoreError)`: 创建失败
    pub fn create_snapshot<P: AsRef<Path>>(
        &self,
        db_path: P,
        description: Option<String>,
    ) -> Result<SnapshotInfo, CoreError> {
        let db_path = db_path.as_ref();

        if !db_path.exists() {
            return Err(CoreError::common(CommonError::General(format!(
                "数据库文件不存在: {}",
                db_path.display()
            ))));
        }

        // 生成快照文件名
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let db_stem = db_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "database".to_string());

        let snapshot_name = format!("{}_snapshot_{}.duckdb", db_stem, timestamp);
        let snapshot_path = self.snapshot_dir.join(&snapshot_name);

        // 复制数据库文件
        std::fs::copy(db_path, &snapshot_path)
            .map_err(|e| CoreError::common(CommonError::General(format!("创建快照失败: {}", e))))?;

        tracing::info!(
            "[SnapshotManager] 创建快照: {} -> {}",
            db_path.display(),
            snapshot_path.display()
        );

        // 清理旧快照
        self.cleanup_old_snapshots()?;

        SnapshotInfo::from_path(&snapshot_path).map(|mut info| {
            info.description = description;
            info
        })
    }

    /// 从快照恢复数据库。
    ///
    /// # 参数
    /// - `snapshot_path`: 快照文件路径
    /// - `target_path`: 目标数据库文件路径
    ///
    /// # 返回
    /// - `Ok(())`: 恢复成功
    /// - `Err(CoreError)`: 恢复失败
    pub fn restore_snapshot<P: AsRef<Path>>(
        &self,
        snapshot_path: P,
        target_path: P,
    ) -> Result<(), CoreError> {
        let snapshot_path = snapshot_path.as_ref();
        let target_path = target_path.as_ref();

        if !snapshot_path.exists() {
            return Err(CoreError::common(CommonError::General(format!(
                "快照文件不存在: {}",
                snapshot_path.display()
            ))));
        }

        // 确保目标目录存在
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::common(CommonError::General(format!("创建目录失败: {}", e)))
            })?;
        }

        // 复制快照文件到目标路径
        std::fs::copy(snapshot_path, target_path)
            .map_err(|e| CoreError::common(CommonError::General(format!("恢复快照失败: {}", e))))?;

        tracing::info!(
            "[SnapshotManager] 恢复快照: {} -> {}",
            snapshot_path.display(),
            target_path.display()
        );

        Ok(())
    }

    /// 删除指定快照。
    ///
    /// # 参数
    /// - `snapshot_path`: 快照文件路径
    ///
    /// # 返回
    /// - `Ok(())`: 删除成功
    /// - `Err(CoreError)`: 删除失败
    pub fn delete_snapshot<P: AsRef<Path>>(&self, snapshot_path: P) -> Result<(), CoreError> {
        let snapshot_path = snapshot_path.as_ref();

        if !snapshot_path.exists() {
            return Err(CoreError::common(CommonError::General(format!(
                "快照文件不存在: {}",
                snapshot_path.display()
            ))));
        }

        std::fs::remove_file(snapshot_path)
            .map_err(|e| CoreError::common(CommonError::General(format!("删除快照失败: {}", e))))?;

        tracing::info!("[SnapshotManager] 删除快照: {}", snapshot_path.display());

        Ok(())
    }

    /// 列出所有快照（按创建时间降序）。
    ///
    /// # 返回
    /// - `Ok(Vec<SnapshotInfo>)`: 快照列表
    /// - `Err(CoreError)`: 列出失败
    pub fn list_snapshots(&self) -> Result<Vec<SnapshotInfo>, CoreError> {
        if !self.snapshot_dir.exists() {
            return Ok(Vec::new());
        }

        let mut snapshots = Vec::new();

        for entry in std::fs::read_dir(&self.snapshot_dir).map_err(|e| {
            CoreError::common(CommonError::General(format!("读取快照目录失败: {}", e)))
        })? {
            let entry = entry.map_err(|e| {
                CoreError::common(CommonError::General(format!("读取目录条目失败: {}", e)))
            })?;

            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "duckdb") {
                if let Ok(info) = SnapshotInfo::from_path(&path) {
                    snapshots.push(info);
                }
            }
        }

        // 按创建时间降序排序
        snapshots.sort_by_key(|s| std::cmp::Reverse(s.created_at));

        Ok(snapshots)
    }

    /// 清理超出最大数量的旧快照。
    fn cleanup_old_snapshots(&self) -> Result<(), CoreError> {
        let snapshots = self.list_snapshots()?;

        if snapshots.len() > self.max_snapshots {
            // 保留最新的 max_snapshots 个，删除其余的
            let to_delete = &snapshots[self.max_snapshots..];

            for snapshot in to_delete {
                tracing::info!("[SnapshotManager] 清理旧快照: {}", snapshot.path.display());
                std::fs::remove_file(&snapshot.path).map_err(|e| {
                    CoreError::common(CommonError::General(format!("清理旧快照失败: {}", e)))
                })?;
            }

            tracing::info!(
                "[SnapshotManager] 清理了 {} 个旧快照，保留 {} 个",
                to_delete.len(),
                self.max_snapshots
            );
        }

        Ok(())
    }

    /// 删除所有快照。
    ///
    /// # 返回
    /// - `Ok(usize)`: 删除的快照数量
    /// - `Err(CoreError)`: 删除失败
    pub fn delete_all_snapshots(&self) -> Result<usize, CoreError> {
        let snapshots = self.list_snapshots()?;
        let count = snapshots.len();

        for snapshot in &snapshots {
            let _ = std::fs::remove_file(&snapshot.path);
        }

        tracing::info!("[SnapshotManager] 删除了 {} 个快照", count);

        Ok(count)
    }

    /// 获取快照存储目录。
    pub fn snapshot_dir(&self) -> &Path {
        &self.snapshot_dir
    }

    /// 获取最大快照数量。
    pub fn max_snapshots(&self) -> usize {
        self.max_snapshots
    }

    /// 计算快照总大小（字节）。
    pub fn total_size_bytes(&self) -> Result<u64, CoreError> {
        let snapshots = self.list_snapshots()?;
        Ok(snapshots.iter().map(|s| s.size_bytes).sum())
    }

    /// 估算备份所需时间（基于文件大小和经验值）。
    ///
    /// # 参数
    /// - `db_path`: 数据库文件路径
    ///
    /// # 返回
    /// 预估时间（秒）
    pub fn estimate_backup_time<P: AsRef<Path>>(&self, db_path: P) -> Duration {
        let db_path = db_path.as_ref();
        if let Ok(metadata) = std::fs::metadata(db_path) {
            let size_bytes = metadata.len();
            // 假设复制速度为 100MB/s
            let speed_bytes_per_sec = 100 * 1024 * 1024;
            let seconds = size_bytes as f64 / speed_bytes_per_sec as f64;
            Duration::from_secs(seconds.ceil() as u64 + 1) // +1 秒缓冲
        } else {
            Duration::from_secs(1)
        }
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_db() -> Result<(PathBuf, PathBuf), CoreError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CoreError::common(CommonError::General(format!("时间获取失败: {}", e))))?
            .as_nanos();
        let temp_dir = std::env::temp_dir();
        let db_dir = temp_dir.join(format!("test_snapshot_{}", timestamp));

        let _ = fs::remove_dir_all(&db_dir);
        fs::create_dir_all(&db_dir).map_err(|e| {
            CoreError::common(CommonError::General(format!("创建测试目录失败: {}", e)))
        })?;

        let db_path = db_dir.join("test.duckdb");
        fs::write(&db_path, "fake duckdb content").map_err(|e| {
            CoreError::common(CommonError::General(format!("创建测试数据库失败: {}", e)))
        })?;

        Ok((db_path, db_dir))
    }

    fn cleanup_test_db(db_dir: &Path) {
        let _ = fs::remove_dir_all(db_dir);
    }

    #[test]
    fn test_create_and_list_snapshot() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        let manager = SnapshotManager::new(&db_path, 5)?;

        let snapshot = manager.create_snapshot(&db_path, Some("测试快照".to_string()))?;

        assert!(snapshot.name.contains("test_snapshot_"));
        assert!(snapshot.path.exists());
        assert!(snapshot.description.is_some());

        let snapshots = manager.list_snapshots()?;
        assert_eq!(snapshots.len(), 1);

        cleanup_test_db(&db_dir);
        Ok(())
    }

    #[test]
    fn test_restore_snapshot() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        let manager = SnapshotManager::new(&db_path, 5)?;

        let snapshot = manager.create_snapshot(&db_path, None)?;

        let restore_path = db_dir.join("restored.duckdb");
        manager.restore_snapshot(&snapshot.path, &restore_path)?;

        assert!(restore_path.exists());

        cleanup_test_db(&db_dir);
        Ok(())
    }

    #[test]
    fn test_delete_snapshot() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        let manager = SnapshotManager::new(&db_path, 5)?;

        let snapshot = manager.create_snapshot(&db_path, None)?;

        manager.delete_snapshot(&snapshot.path)?;
        assert!(!snapshot.path.exists());

        cleanup_test_db(&db_dir);
        Ok(())
    }

    #[test]
    fn test_cleanup_old_snapshots() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        let manager = SnapshotManager::new(&db_path, 2)?;

        // 创建 3 个快照
        manager.create_snapshot(&db_path, None)?;
        std::thread::sleep(std::time::Duration::from_secs(1));
        manager.create_snapshot(&db_path, None)?;
        std::thread::sleep(std::time::Duration::from_secs(1));
        manager.create_snapshot(&db_path, None)?;

        let snapshots = manager.list_snapshots()?;
        assert_eq!(snapshots.len(), 2); // 应该只保留 2 个

        cleanup_test_db(&db_dir);
        Ok(())
    }

    #[test]
    fn test_delete_all_snapshots() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        let manager = SnapshotManager::new(&db_path, 5)?;

        manager.create_snapshot(&db_path, None)?;
        manager.create_snapshot(&db_path, None)?;

        let count = manager.delete_all_snapshots()?;
        assert_eq!(count, 2);

        let snapshots = manager.list_snapshots()?;
        assert_eq!(snapshots.len(), 0);

        cleanup_test_db(&db_dir);
        Ok(())
    }

    #[test]
    fn test_estimate_backup_time() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        let manager = SnapshotManager::new(&db_path, 5)?;

        let time = manager.estimate_backup_time(&db_path);
        assert!(time.as_secs() >= 1);

        cleanup_test_db(&db_dir);
        Ok(())
    }

    #[test]
    fn test_total_size_bytes() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        let manager = SnapshotManager::new(&db_path, 5)?;

        manager.create_snapshot(&db_path, None)?;

        let total = manager.total_size_bytes()?;
        assert!(total > 0);

        cleanup_test_db(&db_dir);
        Ok(())
    }

    #[test]
    fn test_snapshot_info_from_path() -> Result<(), CoreError> {
        let (db_path, db_dir) = setup_test_db()?;
        let manager = SnapshotManager::new(&db_path, 5)?;

        let snapshot = manager.create_snapshot(&db_path, Some("测试".to_string()))?;

        let info = SnapshotInfo::from_path(&snapshot.path)?;
        assert_eq!(info.size_bytes, snapshot.size_bytes);
        assert!(info.created_at > 0);

        cleanup_test_db(&db_dir);
        Ok(())
    }
}
