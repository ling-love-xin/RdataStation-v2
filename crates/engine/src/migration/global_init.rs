/**
 * 全局系统初始化模块
 *
 * 负责应用启动时的全局初始化：
 * - 创建全局数据目录
 * - 初始化全局系统数据库（SQLite 连接池 + DuckDB 长连接）
 * - 执行全局系统库迁移
 * - 初始化全局配置
 */
use std::path::PathBuf;
use std::sync::OnceLock;

use shared::error::{CommonError, CoreError};
use crate::migration::{MigrationManager, MigrationType};
use crate::persistence::GlobalDatabaseManager;

/// 系统目录名称
const SYSTEM_DIR_NAME: &str = "system";

/// 全局 SQLite 数据库文件名
const GLOBAL_SQLITE_NAME: &str = "global.db";

/// 全局 DuckDB 数据库文件名
const GLOBAL_DUCKDB_NAME: &str = "analytics.duckdb";

/// Secret 存储目录名（`system/secrets`）
const SECRETS_DIR_NAME: &str = "secrets";

/// 全局系统数据库管理器实例
///
/// 使用 OnceLock 确保只初始化一次
/// 应用启动时创建，应用关闭时销毁
static GLOBAL_DB_MANAGER: OnceLock<GlobalDatabaseManager> = OnceLock::new();

/// 获取全局数据目录路径
///
/// 位置由 `paths::data_dir()` 解析：`<RDS_HOME>/data`（默认 RDS_HOME = 可执行文件所在目录）。
pub fn get_global_data_dir() -> Result<PathBuf, CoreError> {
    let app_dir = paths::data_dir();

    std::fs::create_dir_all(&app_dir).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to create global data directory: {}",
            e
        )))
    })?;

    Ok(app_dir)
}

/// 获取全局系统目录路径
///
/// 系统目录包含全局 SQLite 和 DuckDB 数据库
pub fn get_system_dir() -> Result<PathBuf, CoreError> {
    let data_dir = get_global_data_dir()?;
    let system_dir = data_dir.join(SYSTEM_DIR_NAME);

    std::fs::create_dir_all(&system_dir).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to create system directory: {}",
            e
        )))
    })?;

    Ok(system_dir)
}

/// DuckDB Secret 存储目录（`<RDS_HOME>/data/system/secrets`，不存在则建）
///
/// **两处必须用同一个目录**：凭据注册（`workbench::services::secret_integration`）与
/// 会话读取（[`crate::duckdb::manager::DuckDBManager::configure_connection`] 的
/// `secret_directory`）。否则会出现“注册成功、挂载却说找不到凭据”，在真机上表现为
/// 网络源认证失败（`ATTACH` 里的口令是脱敏的 `******`，只能靠 Secret 补）。
pub fn get_secrets_dir() -> Result<PathBuf, CoreError> {
    let dir = get_system_dir()?.join(SECRETS_DIR_NAME);
    std::fs::create_dir_all(&dir).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to create secrets directory: {}",
            e
        )))
    })?;
    Ok(dir)
}

/// 获取全局 SQLite 数据库路径
///
/// 路径：`<RDS_HOME>/data/system/global.db`
pub fn get_global_db_path() -> Result<PathBuf, CoreError> {
    let system_dir = get_system_dir()?;
    Ok(system_dir.join(GLOBAL_SQLITE_NAME))
}

/// 获取全局 DuckDB 数据库路径
///
/// 路径：`<RDS_HOME>/data/system/analytics.duckdb`
pub fn get_global_duckdb_path() -> Result<PathBuf, CoreError> {
    let system_dir = get_system_dir()?;
    Ok(system_dir.join(GLOBAL_DUCKDB_NAME))
}

/// 获取全局元数据目录路径
///
/// 路径：`<RDS_HOME>/data/metadata/global/`
pub fn get_global_metadata_dir() -> Result<PathBuf, CoreError> {
    let data_dir = get_global_data_dir()?;
    let metadata_dir = data_dir.join("metadata/global");

    std::fs::create_dir_all(&metadata_dir).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to create global metadata directory: {}",
            e
        )))
    })?;

    Ok(metadata_dir)
}

/// 初始化全局系统数据库管理器
///
/// 必须在应用启动时调用，且只能调用一次。
/// 创建 SQLite 连接池和 DuckDB 长连接。
pub async fn initialize_global_system() -> Result<(), CoreError> {
    // 内置驱动注册（DriverRegistry）：测试连接 / 连接路由都从这里取工厂。
    // 必须在任何连接尝试前完成——曾漏掉这一步，导致真机「测试连接」报
    // `CONN_DRIVER_NOT_FOUND: Driver 'sqlite' not found in registry`（注册表为空）。
    // `register_by_factory` 是幂等写入（HashMap insert），重复调用安全。
    crate::driver::AutoDriverRegistrar::auto_register();

    let sqlite_path = get_global_db_path()?;
    let duckdb_path = get_global_duckdb_path()?;

    // 执行 SQLite 迁移
    let migration_manager = MigrationManager::new();
    let applied = migration_manager.migrate(&sqlite_path, MigrationType::Global)?;

    if !applied.is_empty() {
        tracing::info!(
            "Global system SQLite initialized with {} migrations",
            applied.len()
        );
    } else {
        tracing::debug!("Global system SQLite is up to date");
    }

    // 创建全局数据库管理器（连接池）
    let manager = GlobalDatabaseManager::new(
        sqlite_path,
        duckdb_path,
        10, // SQLite 连接池大小（增加到 10 以支持更高并发）
    )
    .await?;

    // 启动时的数据同步（幂等；失败仅告警，不阻断启动）：
    // ① 存量数据迁移（凭据加密 / 标签回填）；② **驱动声明 upsert**（代码是权威，见 `driver/declaration.rs`）。
    sync_startup_state(&manager).await;

    install_global_db_manager(manager)?;

    tracing::info!("Global system initialized successfully");
    Ok(())
}

/// 启动时数据同步的统计（供启动日志与测试断言）。
///
/// 抽成返回值而不是只写日志：日志看不到的回归（“同步了但一行没写”）靠测试盯住。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupSyncReport {
    /// 明文凭据转密文的条数。
    pub creds_encrypted: usize,
    /// 连接标签回填条数。
    pub tags_backfilled: usize,
    /// 驱动声明落库结果（`None` = 这一步失败，已告警）。
    pub drivers: Option<crate::driver::declaration::DeclarationSync>,
}

/// 启动时的数据同步（幂等；失败仅告警，不阻断启动）。
///
/// 两步：
/// ① [`migrate_legacy_data`]——存量凭据加密 + 标签回填；
/// ② [`crate::driver::declaration::sync_driver_declarations`]——把**代码里的驱动声明**
///   写回 `drivers` 表（迁移里的种子降为首装兜底，见 `driver/declaration.rs` 头注）。
///
/// 抽成独立函数是为了可测：启动入口 `initialize_global_system` 解析的是真实数据目录
/// （测试不能碰），而这一步只需要一个已建好的管理器——否则「声明同步是否真的被启动
/// 路径调了」只能靠人读代码。
pub async fn sync_startup_state(manager: &GlobalDatabaseManager) -> StartupSyncReport {
    let mut report = StartupSyncReport::default();

    let (migrated, tags_migrated) = migrate_legacy_data(manager).await;
    report.creds_encrypted = migrated;
    report.tags_backfilled = tags_migrated;
    if migrated > 0 {
        tracing::info!(count = migrated, "网络档案明文凭据已加密（一次性迁移）");
    }
    if tags_migrated > 0 {
        tracing::info!(count = tags_migrated, "连接标签已回填到权威表（一次性迁移）");
    }

    match crate::driver::declaration::sync_driver_declarations(manager).await {
        Ok(sync) => {
            if !sync.skipped.is_empty() {
                tracing::warn!(
                    skipped = ?sync.skipped,
                    "部分驱动声明未落库（缺数据库族？）——这些驱动在界面/连接链路上会缺行"
                );
            }
            tracing::info!(count = sync.written, "驱动声明已同步到 drivers 表（代码是权威）");
            report.drivers = Some(sync);
        }
        Err(e) => {
            // 不阻断启动：表里已有的声明行仍然可用（只是可能与代码不一致）。
            tracing::warn!(error = %e, "驱动声明同步失败（沿用库中已有声明）");
        }
    }

    report
}

/// 启动时的一次性**存量数据迁移**（幂等；失败仅告警，不阻断启动）。
///
/// ① 存量网络档案的凭据加密——升级前写入的明文 `config` 在这里转密文
///   （读路径对明文仍兼容，下次编辑保存也会自动加密）；
/// ② 存量连接标签回填——早期数据的标签只存于连接行内 `tags` JSON，而
///   `connection_tags` 是后来引入的权威检索表；回填完成后，读取侧不再需要
///   “表里无记录 → 用行内 JSON”的兼容回退（决策 #89）。
///
/// 覆盖**全局库 + 名册里的每个项目库**；返回 `(网络档案加密条数, 标签回填条数)`。
///
/// 抽成独立函数是为了可测：启动入口 `initialize_global_system` 解析的是真实
/// 数据目录（测试不能碰），而这一步只需要一个已建好的管理器——否则“回填是否
/// 真的被启动路径调用”只能靠人读代码（测试见本文件 `mod tests`）。
pub async fn migrate_legacy_data(manager: &GlobalDatabaseManager) -> (usize, usize) {
    let mut migrated = 0usize;
    let mut tags_migrated = 0usize;
    match manager.sqlite_pool().acquire().await {
        Ok(sqlite) => match sqlite.inner() {
            Ok(conn) => {
                match crate::persistence::network_store::reencrypt_all_network_configs(conn) {
                    Ok(n) => migrated += n,
                    Err(e) => tracing::warn!(
                        error = %e,
                        "全局库网络档案凭据加密迁移失败（读路径仍兼容明文）"
                    ),
                }
                match crate::persistence::connection_org_store::backfill_all_connection_tags(conn) {
                    Ok(n) => tags_migrated += n,
                    Err(e) => tracing::warn!(
                        error = %e,
                        "全局库连接标签回填失败（该库标签读取将为空，非阻断）"
                    ),
                }
            }
            Err(e) => tracing::warn!(error = %e, "获取全局库连接失败，跳过网络档案加密迁移"),
        },
        Err(e) => tracing::warn!(error = %e, "获取全局库连接失败，跳过网络档案加密迁移"),
    }
    // 项目库：名册里的每个项目根（不存在 / 无对应表自动跳过，读路径无副作用）
    match manager.get_all_projects().await {
        Ok(projects) => {
            for p in projects {
                if p.path.trim().is_empty() {
                    continue;
                }
                match crate::persistence::network_store::reencrypt_project_network_configs(
                    std::path::Path::new(&p.path),
                ) {
                    Ok(n) => migrated += n,
                    Err(e) => tracing::warn!(
                        project = %p.path,
                        error = %e,
                        "项目库网络档案加密迁移失败（读路径仍兼容明文）"
                    ),
                }
                match crate::persistence::connection_org_store::backfill_project_connection_tags(
                    std::path::Path::new(&p.path),
                ) {
                    Ok(n) => tags_migrated += n,
                    Err(e) => tracing::warn!(
                        project = %p.path,
                        error = %e,
                        "项目库连接标签回填失败（该项目标签读取将为空，非阻断）"
                    ),
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "读取项目名册失败，跳过项目库网络档案加密迁移"),
    }
    (migrated, tags_migrated)
}

/// 注入全局库管理器（测试 / 嵌入场景；只能设置一次）。
///
/// 与 `initialize_global_system` 互斥：后到者返回错误。
/// 集成测试可用临时目录构造管理器后注入，从而覆盖
/// `DataSourceService::global()` 等生产单例路径。
pub fn install_global_db_manager(manager: GlobalDatabaseManager) -> Result<(), CoreError> {
    GLOBAL_DB_MANAGER.set(manager).map_err(|_| {
        CoreError::common(CommonError::General(
            "Global database manager already initialized".to_string(),
        ))
    })
}

/// 获取全局系统数据库管理器实例
///
/// 如果尚未初始化，将返回 None
pub fn get_global_db_manager() -> Option<&'static GlobalDatabaseManager> {
    GLOBAL_DB_MANAGER.get()
}

/// 关闭全局系统数据库管理器
///
/// 在应用退出时调用
pub async fn shutdown_global_system() -> Result<(), CoreError> {
    if let Some(manager) = GLOBAL_DB_MANAGER.get() {
        manager.close().await?;
        tracing::info!("Global database manager shut down successfully");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // 注意：不通配导入（避免引入与 `#[tokio::test]` 同名的属性宏）。
    use super::{migrate_legacy_data, sync_startup_state};
    use crate::persistence::GlobalDatabaseManager;
    use rusqlite::Connection;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_startup_mig_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// 建一个“升级前”的项目库：连接行内 `tags` JSON 有值，权威标签表为空表。
    ///
    /// 权威表**存在但无记录**：这对应“项目已经打开过（项目迁移建好表）、但启动迁移还
    /// 没跑”的真实升级现场；若连表都没有（更旧的项目库），回填会按“不建表”规则跳过，
    /// 由项目打开路径 `ProjectDatabaseManager::open` 负责（见 project_db 的回填测试）。
    fn legacy_project_root(base: &std::path::Path, conn_id: &str) -> std::path::PathBuf {
        let root = base.join("proj");
        std::fs::create_dir_all(root.join(".RSmeta")).expect("mkdir .RSmeta");
        let conn = Connection::open(root.join(".RSmeta").join("project.db")).expect("open project db");
        conn.execute_batch(
            "CREATE TABLE connections (id TEXT PRIMARY KEY, name TEXT, tags TEXT);\n             CREATE TABLE connection_tags (connection_id TEXT NOT NULL, tag TEXT NOT NULL,\n                 PRIMARY KEY (connection_id, tag));",
        )
        .expect("create tables");
        conn.execute(
            "INSERT INTO connections (id, name, tags) VALUES (?1, ?1, ?2)",
            rusqlite::params![conn_id, r#"["proj-tag"]"#],
        )
        .expect("insert legacy row");
        root
    }

    /// 启动迁移的**接线回归**：`migrate_legacy_data` 必须覆盖全局库 + 名册里的项目库，
    /// 且幂等（第二次不再写入）。本测试的存在意义是防止“回填函数写好了但启动没调”。
    #[tokio::test]
    async fn startup_migration_backfills_tags_for_global_and_project() {
        let base = temp_dir("tags");
        let manager = GlobalDatabaseManager::new(
            base.join("global.db"),
            base.join("analytics.duckdb"),
            2,
        )
        .await
        .expect("init manager");

        // 全局侧：一条带行内 JSON 标签、权威表无记录的连接。
        {
            let sqlite = manager.sqlite_pool().acquire().await.expect("acquire");
            let conn = sqlite.inner().expect("rusqlite conn");
            conn.execute(
                "INSERT INTO global_connections (id, name, driver, tags) VALUES ('G_legacy', 'legacy', 'sqlite', ?1)",
                rusqlite::params![r#"["global-tag"]"#],
            )
            .expect("insert legacy global row");
        }

        // 项目侧：登记到名册（迁移只遍历名册，未登记的项目不会被扫到）。
        let root = legacy_project_root(&base, "P_legacy");
        manager
            .save_project_info_smart(
                "p1",
                "Proj",
                None,
                &root.to_string_lossy(),
                "active",
                None,
            )
            .await
            .expect("register project");

        // 首次：全局 1 条 + 项目 1 条。
        let (_, tags_migrated) = migrate_legacy_data(&manager).await;
        assert_eq!(tags_migrated, 2, "全局 + 项目各应回填 1 条");

        // 幂等：再跑一次不再写入。
        let (_, again) = migrate_legacy_data(&manager).await;
        assert_eq!(again, 0, "回填必须是幂等的");

        manager.close().await.expect("close manager");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 启动同步的**接线回归**：`sync_startup_state`（启动入口调的同一个函数）
    /// 必须把代码里的驱动声明写回 `drivers` 表，且幂等——
    /// 「启动漏调同步」在真机上的表现是界面 / 连接链路读不到新增驱动。
    #[tokio::test]
    async fn startup_sync_writes_driver_declarations_and_is_idempotent() {
        crate::driver::AutoDriverRegistrar::auto_register();
        let base = temp_dir("drivers");
        let manager = GlobalDatabaseManager::new(
            base.join("global.db"),
            base.join("analytics.duckdb"),
            2,
        )
        .await
        .expect("init manager");

        let first = sync_startup_state(&manager).await;
        let sync = first.drivers.expect("驱动声明同步应成功");
        assert!(sync.skipped.is_empty(), "不应有跳过：{sync:?}");
        assert!(sync.written >= 6, "内置驱动至少 6 条：{sync:?}");

        // 与代码里的声明逐列比对（族 id 尤其重要：`postgres*` 的族是 `postgresql`）
        let expected = crate::driver::DriverDeclaration::all();
        {
            let sqlite = manager.sqlite_pool().acquire().await.expect("acquire");
            let conn = sqlite.inner().expect("rusqlite conn");
            for d in &expected {
                let (type_id, capabilities, props): (String, String, String) = conn
                    .query_row(
                        "SELECT type_id, capabilities, driver_properties FROM drivers WHERE id = ?1",
                        rusqlite::params![d.id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .unwrap_or_else(|e| panic!("驱动 {} 未落库：{e}", d.id));
                assert_eq!(type_id, d.type_id, "{} 的族 id", d.id);
                assert_eq!(capabilities, d.capabilities, "{} 的能力键", d.id);
                assert_eq!(props, d.driver_properties, "{} 的属性默认值", d.id);
            }
            let rows: i64 = conn
                .query_row("SELECT COUNT(*) FROM drivers", [], |r| r.get(0))
                .expect("count");
            assert_eq!(rows, expected.len() as i64, "不该多出迁移种子以外的行");
        }

        // 第二次：仍写满，但行数不变（幂等）
        let second = sync_startup_state(&manager).await;
        assert_eq!(
            second.drivers.expect("第二次同步").written,
            sync.written,
            "幂等：第二次应同样写满声明条数"
        );

        manager.close().await.expect("close manager");
        let _ = std::fs::remove_dir_all(&base);
    }
}
