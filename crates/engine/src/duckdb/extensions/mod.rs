//! DuckDB 扩展的**准入入口**（第 1 步：把散在三处的装扩展收敛到这里）
//!
//! ## 为什么要有这个模块
//!
//! 扩展装在三个地方、各拼一份 SQL、各写一份几乎逐字重复的错误文案：
//!
//! - `duckdb/accel.rs::install_and_load`（本地加速会话）
//! - `duckdb/federation/session.rs`（联邦会话，复用上面那个函数）
//! - `services/execution_service.rs::ensure_excel_extension`（XLSX 导出，且它已经是
//!   「先 LOAD、不得已才 INSTALL」的正确顺序，而 `accel` 是每次都 INSTALL）
//!
//! 三者都**不登记**：装完之后「这台机器上装了什么」只能靠扫扩展目录猜。
//!
//! ## 两条硬约定
//!
//! 1. **对外按「能力」说话，对内按「扩展名」记账**。调用点关心的是「我要不要能读
//!    xlsx」，不是「装 excel 还是 excel_scanner」；而登记的主键必须是
//!    `(扩展名, 内核版本, 平台)`——磁盘上那三个才算一个文件。
//! 2. **内核版本与平台不自己猜**，一律从 `duckdb_extensions()` 给出的 `install_path`
//!    里解析（形如 `<extensions>/v1.1.5/windows_amd64/excel.duckdb_extension`）。
//!    自己按 `cfg!(windows)` 拼平台串是猜的，DuckDB 的平台命名（`windows_amd64`）
//!    与 rustc 的 `x86_64-pc-windows-msvc` 不是一套东西。
//!
//! ## 与插件的关系
//!
//! 没有任何关系。扩展是进程内 dlopen 的单个文件，不进进程池、没有清单、没有贡献点、
//! 不走 checksums.json（宿主自己算 sha256）。两者只共享「全局装、项目引用」的心智。

use std::io::Read;
use std::path::{Path, PathBuf};

use duckdb::Connection;
use sha2::{Digest, Sha256};

mod inventory;
mod reconcile;

pub use inventory::{ExtensionDrift, ExtensionEntry, ExtensionInventory, inventory};
pub use reconcile::{ReconcileAction, ReconcileOutcome, apply as reconcile_extension};

/// 「我需要某个能力」——调用点只需要说这个，不用记扩展叫什么
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtensionAbility {
    /// XLSX 导出 / 文件探测
    Excel,
    /// MySQL 联邦扫描
    FederationMysql,
    /// PostgreSQL 联邦扫描
    FederationPostgres,
    /// SQLite 联邦扫描
    FederationSqlite,
    /// Oracle 联邦扫描（**社区扩展**）
    FederationOracle,
}

impl ExtensionAbility {
    /// 该能力对应的 DuckDB 扩展名（`LOAD` / `INSTALL` 用的就是它）
    pub fn extension(self) -> &'static str {
        match self {
            Self::Excel => "excel",
            Self::FederationMysql => "mysql",
            Self::FederationPostgres => "postgres",
            Self::FederationSqlite => "sqlite",
            Self::FederationOracle => "oracle_scanner",
        }
    }

    /// 从哪个仓库装：`None` = 官方仓库；`Some("community")` = `INSTALL … FROM community`
    pub fn repository(self) -> Option<&'static str> {
        match self {
            Self::FederationOracle => Some("community"),
            _ => None,
        }
    }

    /// 面向用户的来源口径（登记表 `source` 列）
    pub fn source(self) -> &'static str {
        if self.repository().is_some() {
            "community"
        } else {
            "official"
        }
    }

    /// 界面文案
    pub fn label(self) -> &'static str {
        match self {
            Self::Excel => "Excel 读写",
            Self::FederationMysql => "MySQL 联邦扫描",
            Self::FederationPostgres => "PostgreSQL 联邦扫描",
            Self::FederationSqlite => "SQLite 联邦扫描",
            Self::FederationOracle => "Oracle 联邦扫描",
        }
    }
}

/// 一次成功准入的结果（够调用方与登记表用，不含连接）
#[derive(Debug, Clone)]
pub struct LoadedExtension {
    pub name: String,
    /// 来源口径（`official` / `community`），来自能力声明
    pub source: &'static str,
    /// `duckdb_extensions().install_path` 给出的权威路径（拿不到时为 `None`）
    pub install_path: Option<PathBuf>,
    /// 从 `install_path` 解析出来；解析失败时为 `"unknown"`（不阻断加载，但要在面板上看得见）
    pub kernel_version: String,
    pub platform: String,
    /// 这次是不是刚 `INSTALL` 装下来的（`false` = 本来就在，走的离线 `LOAD`）
    pub freshly_installed: bool,
}

/// 装 / 加载一个扩展（幂等：已装的走 `LOAD`，缺文件才 `INSTALL`）
///
/// 顺序刻意是「先 `LOAD`、不行再 `INSTALL`」：`INSTALL` 会联网（企业内网里是「莫名卡住」），
/// 而已经装好的机器上 `LOAD` 必定成功——日常路径不该付出一次网络往返。
///
/// # 返回
/// - `Ok(LoadedExtension)`：能用（`freshly_installed` 区分「本来就有」与「刚装的」）
/// - `Err(String)`：带扩展目录的可读原因（离线时该把文件放哪，直接写进文案）
pub fn ensure(conn: &Connection, ability: ExtensionAbility) -> Result<LoadedExtension, String> {
    let name = ability.extension();

    // 1) 离线路径：已装就直接加载（无网络副作用）
    if conn.execute_batch(&format!("LOAD {name}")).is_ok() {
        return Ok(describe(conn, ability, false));
    }

    // 2) 缺文件：显式装一次（社区扩展要带 FROM community）
    let install_sql = match ability.repository() {
        Some(repository) => format!("INSTALL {name} FROM {repository}; LOAD {name}"),
        None => format!("INSTALL {name}; LOAD {name}"),
    };
    conn.execute_batch(&install_sql).map_err(|error| {
        format!(
            "需要 DuckDB 扩展 {name}，加载失败：{error}（首次需要能访问 DuckDB 扩展源；\
             离线时请先把 {name}.duckdb_extension 放进 {}）",
            paths::extensions_dir().display()
        )
    })?;
    Ok(describe(conn, ability, true))
}

/// 查权威 install_path 并从路径解析内核版本 / 平台，组装结果
fn describe(conn: &Connection, ability: ExtensionAbility, freshly_installed: bool) -> LoadedExtension {
    let name = ability.extension();
    let install_path = install_path_of(conn, name);
    let (kernel_version, platform) = install_path
        .as_deref()
        .and_then(parse_layout)
        .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));
    LoadedExtension {
        name: name.to_string(),
        source: ability.source(),
        install_path,
        kernel_version,
        platform,
        freshly_installed,
    }
}

/// 从 `duckdb_extensions()` 读 `install_path`（拿不到就 `None`，不猜路径）
fn install_path_of(conn: &Connection, name: &str) -> Option<PathBuf> {
    let mut stmt = conn
        .prepare("SELECT install_path FROM duckdb_extensions() WHERE extension_name = ?1")
        .ok()?;
    let mut rows = stmt.query([name]).ok()?;
    let row = rows.next().ok()??;
    let path: Option<String> = row.get(0).ok()?;
    path.map(PathBuf::from)
}

/// 从扩展路径解析 `(内核版本, 平台)`
///
/// 形如 `<extensions>/v1.1.5/windows_amd64/excel.duckdb_extension`
/// → `("v1.1.5", "windows_amd64")`。
///
/// 格局不对时返回 `None`（由调用方填 `unknown`）。平台的判据是**带下划线**：
/// DuckDB 的平台串都是 `windows_amd64` / `linux_arm64_blob` / `osx_arm64` 这种形态，
/// 而随手给两层目录（如 `<extensions>` 本身被当成文件路径）解析出来的第二层
/// 一定是 `extensions` 之类的普通目录名，不带下划线——这条能把它挡掉。
fn parse_layout(path: &Path) -> Option<(String, String)> {
    let platform = path.parent()?.file_name()?.to_str()?.to_string();
    let kernel = path.parent()?.parent()?.file_name()?.to_str()?.to_string();
    if !platform.contains('_') {
        return None;
    }
    Some((kernel, platform))
}

/// 慢指纹：文件内容 sha256（小写十六进制）+ 字节数
///
/// 只在「刚安装」「登记里没这行」「文件长度变了」「用户点校验」时调。
/// 不在每次建连环上算——几十 MB 的文件每建一个连接读一遍，代价是白付的。
///
/// **长度比对不是完整性判定**：等长替换查不出来，能查出来的只有「和上次登记的不一样」。
/// 进程内扩展的安全边界本就落在「谁把文件放进了扩展目录」，这里只负责让「不一样」可见。
pub fn sha256_of(path: &Path) -> std::io::Result<(String, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        hasher.update(&buf[..read]);
    }
    Ok((hex::encode(hasher.finalize()), total))
}

/// 把一次准入结果 best-effort 落到 `duckdb_extensions` + `extension_operations`
///
/// 留痕失败不能把已经成功的加载变成失败（用户要的是「能跑」），但必须告警——
/// 静默是这个项目明确不接受的做法。
pub fn record_best_effort(actor: &str, loaded: &LoadedExtension) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::debug!(
            extension = %loaded.name,
            "无 tokio runtime，扩展登记跳过（本次加载不受影响）"
        );
        return;
    };
    let loaded = loaded.clone();
    let actor = actor.to_string();
    handle.spawn(async move {
        if let Err(error) = write_records(&actor, &loaded).await {
            tracing::warn!(
                extension = %loaded.name,
                %error,
                "扩展登记写入失败（本次加载不受影响）"
            );
        }
    });
}

async fn write_records(actor: &str, loaded: &LoadedExtension) -> Result<(), String> {
    use crate::persistence::extension_store::{
        DuckdbExtensionRow, ExtensionOperation, record_operation, upsert_extension,
    };

    let Some(db) = crate::migration::get_global_db_manager() else {
        return Err("global.db 未初始化".to_string());
    };
    let sqlite = db
        .sqlite_pool()
        .acquire()
        .await
        .map_err(|e| format!("acquire global sqlite: {e}"))?;
    let conn = sqlite
        .inner()
        .map_err(|e| format!("global sqlite conn: {e}"))?;
    if !crate::persistence::extension_store::table_exists(conn).map_err(|e| e.to_string())? {
        return Err("duckdb_extensions 表不存在（迁移 031 未跑）".to_string());
    }

    // 慢指纹只在「首次登记」「刚安装」「文件长度变了」时算：日常 LOAD 只刷新
    // verified_at，不读几十 MB 的文件内容（否则每建一个连接都要付一次全文件读取）
    let existing = crate::persistence::extension_store::find_extension(conn, &loaded.name)
        .ok()
        .flatten();
    let path_text = loaded
        .install_path
        .as_deref()
        .map(|p| p.to_string_lossy().to_string());
    // 与上次登记比：路径同一个、长度一样 → 视为没动过（等长替换查不出来，见 `sha256_of` 注释）
    let looks_unchanged = match (&path_text, existing.as_ref()) {
        (Some(now), Some(old)) => {
            old.file_path.as_deref() == Some(now.as_str())
                && old
                    .size_bytes
                    .zip(loaded.install_path.as_deref().and_then(|p| std::fs::metadata(p).ok()))
                    .is_some_and(|(old_size, meta)| meta.len() as i64 == old_size)
        }
        _ => false,
    };
    let need_hash = loaded.freshly_installed || existing.is_none() || !looks_unchanged;

    let mut row = DuckdbExtensionRow::installed(
        &loaded.name,
        &loaded.kernel_version,
        &loaded.platform,
        // 刚装的按能力声明记来源；已有文件走 LOAD 时沿用旧行（来源是历史事实，不该被覆盖成 current 猜测）
        if loaded.freshly_installed {
            loaded.source
        } else {
            existing
                .as_ref()
                .map(|e| e.source.as_str())
                .unwrap_or(loaded.source)
        },
    );
    if let Some(path) = loaded.install_path.as_deref() {
        row = row.with_path(path.to_string_lossy().to_string());
        if need_hash {
            if let Ok((sha, size)) = sha256_of(path) {
                row = row.with_sha256(sha, size as i64);
            }
        } else if let Some(old) = &existing {
            // 沿用旧指纹：upsert 把所有列都写成 excluded，不带上的话校验值会被抹成 NULL
            row.sha256 = old.sha256.clone();
            row.size_bytes = old.size_bytes;
        }
    } else {
        row.last_error = Some("拿不到 install_path（duckdb_extensions 未报该扩展）".to_string());
        row.state = "mismatch".to_string();
    }

    let incomplete = loaded.install_path.is_none();
    upsert_extension(conn, &row).map_err(|e| e.to_string())?;

    // leave 一条可回溯的痕迹：装下来的记 `install`，本来就在的记 `load`
    let op = if incomplete {
        ExtensionOperation::fail(
            actor,
            "load",
            "拿不到 install_path，登记不完整（duckdb_extensions 未报该扩展）",
        )
    } else if loaded.freshly_installed {
        ExtensionOperation::ok(actor, "install")
    } else {
        ExtensionOperation::ok(actor, "load")
    };
    let op = op
        .extension(&loaded.name)
        .build(&loaded.kernel_version, &loaded.platform);
    record_operation(conn, &op).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // 安全模式：不通配导入
    use super::{ExtensionAbility, parse_layout, sha256_of};
    use std::path::PathBuf;

    #[test]
    fn abilities_map_to_the_expected_extensions() {
        assert_eq!(ExtensionAbility::Excel.extension(), "excel");
        assert_eq!(ExtensionAbility::FederationMysql.extension(), "mysql");
        assert_eq!(ExtensionAbility::FederationPostgres.extension(), "postgres");
        assert_eq!(ExtensionAbility::FederationSqlite.extension(), "sqlite");
        assert_eq!(
            ExtensionAbility::FederationOracle.extension(),
            "oracle_scanner",
            "Oracle 走社区扩展，名字与库类型不同"
        );
    }

    #[test]
    fn only_oracle_comes_from_the_community_repository() {
        assert_eq!(
            ExtensionAbility::FederationOracle.repository(),
            Some("community")
        );
        assert_eq!(ExtensionAbility::FederationOracle.source(), "community");
        for ability in [
            ExtensionAbility::Excel,
            ExtensionAbility::FederationMysql,
            ExtensionAbility::FederationPostgres,
            ExtensionAbility::FederationSqlite,
        ] {
            assert_eq!(ability.repository(), None, "{ability:?} 应走官方仓库");
            assert_eq!(ability.source(), "official");
        }
    }

    #[test]
    fn layout_is_parsed_from_the_install_path() {
        let path =
            PathBuf::from("/rds/extensions/v1.1.5/windows_amd64/excel.duckdb_extension");
        assert_eq!(
            parse_layout(&path),
            Some(("v1.1.5".to_string(), "windows_amd64".to_string()))
        );

        // 格局不对（少一层目录）→ 不猜，返回 None
        let flat = PathBuf::from("/rds/extensions/excel.duckdb_extension");
        assert_eq!(parse_layout(&flat), None);
    }

    #[test]
    fn sha256_notices_a_changed_file() {
        let dir = std::env::temp_dir().join(format!("rds-ext-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let file = dir.join("fake.duckdb_extension");

        std::fs::write(&file, b"v1-payload").expect("写");
        let (sha_before, len_before) = sha256_of(&file).expect("sha");

        std::fs::write(&file, b"v2-payload-longer").expect("改写");
        let (sha_after, len_after) = sha256_of(&file).expect("sha");

        assert_ne!(sha_before, sha_after, "sha256 必须能反映内容变化");
        assert_eq!(len_before, 10);
        assert_eq!(len_after, 17);
        assert_eq!(sha_before.len(), 64, "sha256 应是 64 位小写十六进制");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：`duckdb_extensions()` 必须暴露 `install_path`
    ///
    /// 这条是「不自己拼 `v<x>/<platform>`」的前提——列名一变，登记就会全部退化成
    /// `unknown`，而那时现象是「面板上一片 unknown」，很难反查到根因。内存库跑，
    /// 不联网。
    #[test]
    fn duckdb_extensions_exposes_install_path() {
        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        let mut stmt = conn
            .prepare("SELECT * FROM duckdb_extensions()")
            .expect("prepare");
        // 列名只能从**已执行**的语句上取（`Statement::column_names` 在未执行时 panic）
        let rows = stmt.query([]).expect("query");
        let columns: Vec<String> = rows
            .as_ref()
            .map(|statement| statement.column_names())
            .unwrap_or_default();
        assert!(
            columns.iter().any(|c| c == "install_path"),
            "duckdb_extensions() 少了 install_path，实际列：{columns:?}"
        );
    }
}
