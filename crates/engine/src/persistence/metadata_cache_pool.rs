/**
 * 连接元数据缓存连接池 (Metadata Cache Connection Pool)
 *
 * 属于 SmartPool 体系中的「连接元数据 SQLite」—— 每个数据库连接独享一个缓存 SQLite 文件。
 *
 * 设计理由：
 * - 高频读写：元数据浏览时树节点展开、刷新都会触发密集读写
 * - **消除热路径固定开销**：`MetadataCacheManager::open()` 每次都「开文件 + 5 条 PRAGMA +
 *   一次迁移校验」。缓存命中理应是毫秒级，这份开销在每层展开时白付一次
 *   ——大 schema（10 万+ 表）浏览时尤其明显，这也是 L1 之外必须池化的原因
 *
 * 形态（与初版不同，改动理由见下）：
 * - **同步**（`std::sync::Mutex`）：唯一使用方 `NavCache::open` 是同步函数，且可能不在
 *   tokio 运行时内（`mock_generator` 等宿主路径）。初版用 `tokio::sync::Mutex` + 信号量 +
 *   「运行时内归还」，在同步上下文里要么 panic（`Handle::current()`）要么无法归还
 *   —— 这正是它写完却一直零调用的原因
 * - 池空时**按需新建连接**，不阻塞、不排队：池的语义是「复用空闲连接」，不是背压闸门
 *
 * 架构归属：SmartPool（守护系统内置库）
 *   ├── 应用级 SQLite（global.db）          → GlobalSqlitePool
 *   ├── 项目级 SQLite（project.db）          → ProjectSqlitePool
 *   ├── 连接元数据 SQLite（每连接一个）        → MetadataCachePool  ← 本模块
 *   ├── 应用级 DuckDB（analytics.duckdb）    → GlobalDuckdbConnection
 *   └── 项目级 DuckDB（project.analytics.duckdb）→ ProjectDuckdbConnection
 */
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::Connection;

use shared::error::{CommonError, CoreError, StorageError};

/// 池注册表：**按缓存文件路径**索引。
///
/// 不按 `conn_id`：同一 conn_id 在不同项目根下会指向不同文件（项目搬迁、测试各自建根），
/// 按路径索引天然避开「拿到别处的池」这类串味；两个 conn_id 指向同一文件时共享池也是对的
/// （文件型库的多 id 别名场景）。
static POOL_REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Arc<MetadataCachePool>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<PathBuf, Arc<MetadataCachePool>>> {
    POOL_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 连接元数据缓存连接池（每缓存文件一个池）。
pub struct MetadataCachePool {
    db_path: PathBuf,
    /// 空闲连接。取用时弹出、归还时压回；为空则按需新建。
    idle: Arc<Mutex<Vec<Connection>>>,
}

impl MetadataCachePool {
    /// 获取或创建指定缓存文件的池。
    ///
    /// 创建时**一次性**跑迁移（`MetadataCacheManager::ensure_schema`）并预热 `warm` 个连接；
    /// 之后所有取用都不再校验表结构 —— 这是把固定开销挪出热路径的关键。
    pub fn get_or_create(
        db_path: PathBuf,
        warm: usize,
    ) -> Result<Arc<Self>, CoreError> {
        let mut reg = registry().lock().map_err(|_| {
            CoreError::common(CommonError::General(
                "metadata cache pool registry poisoned".to_string(),
            ))
        })?;

        if let Some(existing) = reg.get(&db_path) {
            return Ok(Arc::clone(existing));
        }

        let pool = Arc::new(Self::create(db_path.clone(), warm)?);
        reg.insert(db_path, Arc::clone(&pool));
        Ok(pool)
    }

    fn create(db_path: PathBuf, warm: usize) -> Result<Self, CoreError> {
        // 目录与表结构在这里一次性就绪（幂等）。
        crate::persistence::metadata_cache::ensure_schema_at(&db_path)?;

        let mut idle = Vec::with_capacity(warm.max(1));
        for _ in 0..warm.max(1) {
            idle.push(open_connection(&db_path)?);
        }

        Ok(Self {
            db_path,
            idle: Arc::new(Mutex::new(idle)),
        })
    }

    /// 取出一个连接（守卫 Drop 时自动归还；池空则按需新建）。
    pub fn acquire(self: &Arc<Self>) -> Result<PooledMetadataConnection, CoreError> {
        let recycled = {
            let mut idle = self.idle.lock().map_err(|_| {
                CoreError::common(CommonError::General(
                    "metadata cache pool poisoned".to_string(),
                ))
            })?;
            idle.pop()
        };

        let conn = match recycled {
            Some(conn) => conn,
            None => open_connection(&self.db_path)?,
        };

        Ok(PooledMetadataConnection {
            conn: Some(conn),
            idle: Arc::clone(&self.idle),
        })
    }

    /// 缓存文件路径。
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// 空闲连接数（诊断用）。
    pub fn idle_count(&self) -> usize {
        self.idle.lock().map(|idle| idle.len()).unwrap_or(0)
    }

    /// 丢弃某缓存文件的池。
    ///
    /// **删除缓存文件前必须调用**：池里握着打开的文件句柄，Windows 上会让删除失败。
    pub fn drop_pool(db_path: &Path) {
        let Ok(mut reg) = registry().lock() else {
            return;
        };
        if let Some(pool) = reg.remove(db_path) {
            if let Ok(mut idle) = pool.idle.lock() {
                idle.clear();
            }
        }
    }
}

/// 打开一个已配置好的缓存连接（PRAGMA 与 `MetadataCacheManager::open` 保持一致）。
fn open_connection(path: &Path) -> Result<Connection, CoreError> {
    let conn = Connection::open(path).map_err(|e| persistence_err("open", e))?;

    conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))
        .map_err(|e| persistence_err("set_wal_mode", e))?;

    // mmap 失败只告警（部分文件系统不支持），与既有实现口径一致。
    if let Err(e) = conn.execute("PRAGMA mmap_size=268435456", []) {
        tracing::warn!("Failed to set mmap_size on metadata cache pool: {}", e);
    }

    conn.execute("PRAGMA cache_size=-2000", [])
        .map_err(|e| persistence_err("set_cache_size", e))?;
    conn.execute("PRAGMA foreign_keys=ON", [])
        .map_err(|e| persistence_err("set_foreign_keys", e))?;
    conn.execute("PRAGMA synchronous=NORMAL", [])
        .map_err(|e| persistence_err("set_synchronous", e))?;

    Ok(conn)
}

fn persistence_err(operation: &str, e: rusqlite::Error) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "metadata_cache".to_string(),
        operation: operation.to_string(),
        reason: e.to_string(),
    })
}

/// 池化连接守卫：Drop 时归还到池。
///
/// 实现 `Deref` / `DerefMut` 到 `rusqlite::Connection`，因此可直接当连接用
/// （`MetadataCacheOps` 就靠这一点把池化连接接进既有的 90 个方法）。
pub struct PooledMetadataConnection {
    conn: Option<Connection>,
    idle: Arc<Mutex<Vec<Connection>>>,
}

impl PooledMetadataConnection {
    /// 内部连接引用（与既有调用点兼容）。
    pub fn inner(&self) -> Result<&Connection, CoreError> {
        self.conn.as_ref().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "connection already returned to pool".to_string(),
            ))
        })
    }

    /// 内部连接可变引用。
    pub fn inner_mut(&mut self) -> Result<&mut Connection, CoreError> {
        self.conn.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "connection already returned to pool".to_string(),
            ))
        })
    }
}

impl Deref for PooledMetadataConnection {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        // 守卫生命周期内 `conn` 恒为 `Some`（只有 Drop 取走）。
        self.conn
            .as_ref()
            .expect("pooled connection is taken only on drop")
    }
}

impl DerefMut for PooledMetadataConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        self.conn
            .as_mut()
            .expect("pooled connection is taken only on drop")
    }
}

impl Drop for PooledMetadataConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            match self.idle.lock() {
                Ok(mut idle) => idle.push(conn),
                Err(_) => tracing::warn!("Failed to return metadata cache connection to pool"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_mdc_pool_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("conn_test.sqlite")
    }

    /// 池化取用是同步的（不需要 tokio 运行时），且归还后可再次取到**同一个**文件句柄池。
    #[test]
    fn acquire_and_return() {
        let db = temp_db("acquire");
        let pool = MetadataCachePool::get_or_create(db.clone(), 1).expect("建池");

        assert_eq!(pool.idle_count(), 1, "预热 1 个连接");
        {
            let guard = pool.acquire().expect("取连接");
            // 池化连接可直接当 rusqlite::Connection 用（Deref）
            let n: i64 = guard
                .query_row("SELECT COUNT(*) FROM schemata", [], |r| r.get(0))
                .expect("池化连接应可查询（表结构已迁移）");
            assert_eq!(n, 0);
            assert_eq!(pool.idle_count(), 0, "取出后空闲数为 0");
        }
        assert_eq!(pool.idle_count(), 1, "Drop 后归还");

        let _ = std::fs::remove_dir_all(db.parent().expect("父目录"));
    }

    /// 同一路径两次 `get_or_create` 返回同一个池（不重复建、不重复迁移）。
    #[test]
    fn same_path_shares_one_pool() {
        let db = temp_db("same");
        let a = MetadataCachePool::get_or_create(db.clone(), 1).expect("建池");
        let b = MetadataCachePool::get_or_create(db.clone(), 1).expect("复用池");
        assert!(Arc::ptr_eq(&a, &b));

        let _ = std::fs::remove_dir_all(db.parent().expect("父目录"));
    }

    /// 池空则按需新建（不阻塞、不报错）——池是复用器不是背压闸门。
    #[test]
    fn acquire_beyond_warm_creates_on_demand() {
        let db = temp_db("ondemand");
        let pool = MetadataCachePool::get_or_create(db.clone(), 1).expect("建池");

        let g1 = pool.acquire().expect("第一个");
        let g2 = pool.acquire().expect("池空时按需新建");
        assert_eq!(pool.idle_count(), 0);
        drop(g1);
        drop(g2);
        assert_eq!(pool.idle_count(), 2, "两个都归还");

        let _ = std::fs::remove_dir_all(db.parent().expect("父目录"));
    }

    /// `drop_pool` 释放句柄 —— 删除缓存文件前必须能删掉（Windows 语义）。
    #[test]
    fn drop_pool_releases_file_for_deletion() {
        let db = temp_db("drop");
        let pool = MetadataCachePool::get_or_create(db.clone(), 1).expect("建池");
        {
            let _guard = pool.acquire().expect("占住连接");
        }
        MetadataCachePool::drop_pool(&db);

        std::fs::remove_file(&db).expect("释放池后应能删除缓存文件");
        assert!(!db.exists());

        let _ = std::fs::remove_dir_all(db.parent().expect("父目录"));
    }
}
