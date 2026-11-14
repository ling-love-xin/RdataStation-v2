//! 扩展清单：把**三份各自独立的事实**摆到一起
//!
//! 只列一个「已装扩展」列表是没有用的——用户看不出问题。真正要看的是这三份对不对得上：
//!
//! 1. **登记表**（`global.sqlite` 的 `duckdb_extensions`）：我们以为装了什么；
//! 2. **DuckDB 自报**（`duckdb_extensions()`）：内核在**这份扩展目录**里认出了什么；
//! 3. **磁盘**（`<RDS_HOME>/extensions` 下的 `*.duckdb_extension`）：文件真的在不在。
//!
//! 三者两两不一致时，各自对应一种真实故障：
//!
//! - 登记有、磁盘没 → 能力会**在用到时报错**（这是最要命的一种，用户此时还以为能用）；
//! - 登记的内核版本 ≠ 现在跑的 → 升级 DuckDB 之后没重装，`LOAD` 会失败；
//! - 磁盘有、DuckDB 没报 → 文件放错了目录 / 版本不匹配，等于白放；
//! - 磁盘有、没登记 → 来路不明却**已经有执行权**（`autoload_known_extensions=true`）。
//!
//! 这份清单**只报告，不动手**。改状态、装、撤都是后续动作，先让「不一致」可见。

use duckdb::Connection;

use crate::persistence::extension_store::{DuckdbExtensionRow, list_extensions};

/// 一致性偏差（一条记录只标**最要紧**的那一个，界面按这个顺序解释）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionDrift {
    /// 登记说装了，磁盘上找不到文件 → 用到时必然失败
    RegisteredMissingOnDisk,
    /// 登记的内核版本与当前不一致 → 升级后没重装
    KernelMismatch { registered: String, running: String },
    /// 文件在，但 `duckdb_extensions()` 没报 → 目录或版本不对
    NotReportedByDuckdb,
    /// 文件在且能用，但从没登记过 → 有执行权却来路不明
    UnregisteredOnDisk,
}

impl ExtensionDrift {
    /// 排障顺序：越靠前越该先处理
    pub fn severity(&self) -> u8 {
        match self {
            Self::RegisteredMissingOnDisk => 1,
            Self::KernelMismatch { .. } => 2,
            Self::NotReportedByDuckdb => 3,
            Self::UnregisteredOnDisk => 4,
        }
    }

    /// 一句话说清「怎么了 + 会怎样」（不写实现细节，用户不需要知道 SQL）
    pub fn label(&self) -> String {
        match self {
            Self::RegisteredMissingOnDisk => {
                "登记有、磁盘上没有文件——这个能力现在不可用".to_string()
            }
            Self::KernelMismatch {
                registered,
                running,
            } => format!(
                "登记的是 {registered}，现在是 {running}——升级后需要重装"
            ),
            Self::NotReportedByDuckdb => {
                "文件在，但 DuckDB 没认出它（目录或版本不匹配）".to_string()
            }
            Self::UnregisteredOnDisk => {
                "从没登记过，却已经在扩展目录里（等于拿到了执行权）".to_string()
            }
        }
    }
}

/// 清单里的一行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionEntry {
    pub name: String,
    /// 登记的内核版本；没有登记时取磁盘 / DuckDB 自报的
    pub kernel_version: String,
    pub platform: String,
    /// `official` / `community` / `unknown`（磁盘捡到的、无登记的只能是 unknown）
    pub source: String,
    /// 权威路径（DuckDB 自报的 install_path，或磁盘扫描到的）
    pub install_path: Option<String>,
    pub size_bytes: Option<i64>,
    pub sha256: Option<String>,
    pub registered: bool,
    /// `duckdb_extensions()` 报 `installed = true`
    pub reported: bool,
    pub on_disk: bool,
    pub drift: Option<ExtensionDrift>,
    pub last_error: Option<String>,
}

/// 一次清单读取的结果
#[derive(Debug, Clone, Default)]
pub struct ExtensionInventory {
    pub entries: Vec<ExtensionEntry>,
    /// 读某一源失败时的说明（部分失败不吞掉，也不让整个清单变成空）
    pub notes: Vec<String>,
}

impl ExtensionInventory {
    /// 有几条对不上（界面标题行用）
    pub fn drift_count(&self) -> usize {
        self.entries.iter().filter(|e| e.drift.is_some()).count()
    }
}

/// DuckDB 自报的一行
struct ReportedExtension {
    name: String,
    installed: bool,
    install_path: Option<String>,
}

/// 磁盘上捡到的一个文件
struct DiskExtension {
    name: String,
    path: String,
    size_bytes: Option<i64>,
    kernel_version: String,
    platform: String,
}

/// 读一次完整清单（同步；调用方保证在后台线程上跑）
///
/// 三份来源各自独立失败：某一源读不出来时记进 `notes` 继续，不整单报错——
/// 只读到一份也比「整页空白」有用。
pub fn inventory() -> Result<ExtensionInventory, String> {
    let mut notes = Vec::new();

    let reported = match query_reported() {
        Ok(rows) => rows,
        Err(error) => {
            notes.push(error);
            Vec::new()
        }
    };
    let disk = scan_disk();
    let registered = match read_registered() {
        Ok(rows) => rows,
        Err(error) => {
            notes.push(error);
            Vec::new()
        }
    };

    Ok(ExtensionInventory {
        entries: assemble(reported, &disk, &registered),
        notes,
    })
}

/// 查 `duckdb_extensions()`：**需要一条活连接**，而插件管理页是常驻 tab、项目引擎可能没启动，
/// 所以用一条临时内存连接（`configure_connection` 已经把扩展目录指到同一处）。
fn query_reported() -> Result<Vec<ReportedExtension>, String> {
    let conn = Connection::open_in_memory().map_err(|error| format!("开临时内存连接失败：{error}"))?;
    crate::duckdb::DuckDBManager::configure_connection(&conn)
        .map_err(|error| format!("配置临时连接失败：{error}"))?;

    let mut stmt = conn
        .prepare("SELECT extension_name, installed, install_path FROM duckdb_extensions()")
        .map_err(|error| format!("查扩展清单失败：{error}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ReportedExtension {
                name: row.get(0)?,
                installed: row.get(1)?,
                install_path: row.get(2)?,
            })
        })
        .map_err(|error| format!("读扩展清单失败：{error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读扩展清单失败：{error}"))
}

/// 扫扩展目录：递归找 `*.duckdb_extension`
///
/// 不假设目录布局（`v<内核>/<平台>/` 是 DuckDB 现在建的样子，不是契约），
/// 所以按文件后缀找；内核版本与平台仍从路径解析，解析不出记 `unknown`。
fn scan_disk() -> Vec<DiskExtension> {
    let root = paths::extensions_dir();
    let mut out = Vec::new();
    walk(&root, 0, &mut out);
    out
}

fn walk(dir: &std::path::Path, depth: usize, out: &mut Vec<DiskExtension>) {
    // 三层足够：`<ext>/v1.1.5/windows_amd64/x.duckdb_extension` 正好到底
    if depth > 3 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, depth + 1, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".duckdb_extension") else {
            continue;
        };
        let (kernel_version, platform) = super::parse_layout(&path)
            .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));
        out.push(DiskExtension {
            name: stem.to_string(),
            size_bytes: std::fs::metadata(&path)
                .ok()
                .map(|meta| meta.len() as i64),
            path: path.to_string_lossy().to_string(),
            kernel_version,
            platform,
        });
    }
}

/// 读登记表（同步；`acquire_sync` 自建短命 runtime，后台线程里也能用）
fn read_registered() -> Result<Vec<DuckdbExtensionRow>, String> {
    let Some(db) = crate::migration::get_global_db_manager() else {
        return Err("global.db 未初始化（宿主启动流程未完成）".to_string());
    };
    let pool = db.sqlite_pool();
    let conn = pool.acquire_sync().map_err(|error| error.to_string())?;
    let conn_ref = conn.inner().map_err(|error| error.to_string())?;
    if !crate::persistence::extension_store::table_exists(conn_ref).map_err(|e| e.to_string())? {
        return Err("duckdb_extensions 表不存在（迁移 031 未跑）".to_string());
    }
    list_extensions(conn_ref).map_err(|error| error.to_string())
}

/// 三份来源按扩展名合并（**纯函数**，全部偏差判定都在这里，便于单测）
fn assemble(
    reported: Vec<ReportedExtension>,
    disk: &[DiskExtension],
    registered: &[DuckdbExtensionRow],
) -> Vec<ExtensionEntry> {
    let mut names: Vec<String> = Vec::new();
    for name in reported
        .iter()
        .map(|row| row.name.as_str())
        .chain(disk.iter().map(|row| row.name.as_str()))
        .chain(registered.iter().map(|row| row.extension_name.as_str()))
    {
        if !names.iter().any(|known| known == name) {
            names.push(name.to_string());
        }
    }
    names.sort();

    names
        .into_iter()
        .map(|name| {
            // 同一扩展可能有多版本登记：取最近登记的一行
            let reg = registered
                .iter()
                .filter(|row| row.extension_name == name)
                .max_by(|a, b| a.installed_at.cmp(&b.installed_at));
            let rep = reported.iter().find(|row| row.name == name);
            let file = disk.iter().find(|row| row.name == name);

            let reported_kernel = rep
                .and_then(|row| row.install_path.as_deref())
                .and_then(|path| super::parse_layout(std::path::Path::new(path)))
                .map(|(kernel, _)| kernel);

            let kernel_version = reg
                .map(|row| row.kernel_version.clone())
                .or(reported_kernel.clone())
                .or_else(|| file.map(|row| row.kernel_version.clone()))
                .unwrap_or_else(|| "unknown".to_string());
            let platform = reg
                .map(|row| row.platform.clone())
                .or_else(|| file.map(|row| row.platform.clone()))
                .or_else(|| {
                    rep.and_then(|row| row.install_path.as_deref())
                        .and_then(|path| super::parse_layout(std::path::Path::new(path)))
                        .map(|(_, platform)| platform)
                })
                .unwrap_or_else(|| "unknown".to_string());

            let on_disk = file.is_some();
            let is_reported = rep.is_some_and(|row| row.installed);

            let drift = judge(reg.is_some(), on_disk, is_reported, reg, reported_kernel.as_deref());

            ExtensionEntry {
                name: name.clone(),
                kernel_version,
                platform,
                source: reg
                    .map(|row| row.source.clone())
                    .unwrap_or_else(|| "unknown".to_string()),
                install_path: rep
                    .and_then(|row| row.install_path.clone())
                    .or_else(|| file.map(|row| row.path.clone())),
                size_bytes: reg
                    .and_then(|row| row.size_bytes)
                    .or_else(|| file.and_then(|row| row.size_bytes)),
                sha256: reg.and_then(|row| row.sha256.clone()),
                registered: reg.is_some(),
                reported: is_reported,
                on_disk,
                drift,
                last_error: reg.and_then(|row| row.last_error.clone()),
            }
        })
        .collect()
}

/// 偏差判定（纯函数；一条只标最要紧的那个）
fn judge(
    registered: bool,
    on_disk: bool,
    reported: bool,
    reg: Option<&DuckdbExtensionRow>,
    reported_kernel: Option<&str>,
) -> Option<ExtensionDrift> {
    if registered && !on_disk {
        return Some(ExtensionDrift::RegisteredMissingOnDisk);
    }
    // 内核版本不一致只在两边都真拿到版本时判（`unknown` 不参与比较，否则全是假阳性）
    if let (Some(reg), Some(running)) = (reg, reported_kernel)
        && reg.kernel_version != "unknown"
        && running != "unknown"
        && reg.kernel_version != running
    {
        return Some(ExtensionDrift::KernelMismatch {
            registered: reg.kernel_version.clone(),
            running: running.to_string(),
        });
    }
    if on_disk && !reported {
        return Some(ExtensionDrift::NotReportedByDuckdb);
    }
    if on_disk && !registered {
        return Some(ExtensionDrift::UnregisteredOnDisk);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{DiskExtension, ExtensionDrift, ReportedExtension, assemble, judge};
    use crate::persistence::extension_store::DuckdbExtensionRow;

    fn registered(name: &str, kernel: &str, source: &str) -> DuckdbExtensionRow {
        let mut row = DuckdbExtensionRow::installed(name, kernel, "windows_amd64", source);
        row.file_path = Some(format!(
            r"C:\rds\extensions\{kernel}\windows_amd64\{name}.duckdb_extension"
        ));
        row.installed_at = "2026-11-14 01:00:00".to_string();
        row
    }

    fn disk(name: &str, kernel: &str) -> DiskExtension {
        DiskExtension {
            name: name.to_string(),
            path: format!(
                r"C:\rds\extensions\{kernel}\windows_amd64\{name}.duckdb_extension"
            ),
            size_bytes: Some(1_000),
            kernel_version: kernel.to_string(),
            platform: "windows_amd64".to_string(),
        }
    }

    fn reported(name: &str, kernel: &str) -> ReportedExtension {
        ReportedExtension {
            name: name.to_string(),
            installed: true,
            install_path: Some(format!(
                r"C:\rds\extensions\{kernel}\windows_amd64\{name}.duckdb_extension"
            )),
        }
    }

    #[test]
    fn aligned_triple_has_no_drift() {
        let entries = assemble(
            vec![reported("excel", "v1.1.5")],
            &[disk("excel", "v1.1.5")],
            &[registered("excel", "v1.1.5", "official")],
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].drift, None);
        assert!(entries[0].registered && entries[0].reported && entries[0].on_disk);
    }

    #[test]
    fn registered_but_file_gone_is_the_worst_case() {
        let entries = assemble(
            vec![],
            &[],
            &[registered("excel", "v1.1.5", "official")],
        );
        assert_eq!(
            entries[0].drift,
            Some(ExtensionDrift::RegisteredMissingOnDisk)
        );
    }

    #[test]
    fn kernel_mismatch_is_reported_with_both_versions() {
        let entries = assemble(
            vec![reported("excel", "v1.1.6")],
            &[disk("excel", "v1.1.6")],
            &[registered("excel", "v1.1.5", "official")],
        );
        assert_eq!(
            entries[0].drift,
            Some(ExtensionDrift::KernelMismatch {
                registered: "v1.1.5".to_string(),
                running: "v1.1.6".to_string(),
            })
        );
    }

    #[test]
    fn file_on_disk_that_duckdb_ignores() {
        let entries = assemble(vec![], &[disk("oracle_scanner", "v1.1.5")], &[]);
        assert_eq!(
            entries[0].drift,
            Some(ExtensionDrift::NotReportedByDuckdb),
            "磁盘有、DuckDB 没报，比「没登记」更该先查（文件可能放错目录）"
        );
    }

    #[test]
    fn file_that_works_but_was_never_registered() {
        let entries = assemble(
            vec![reported("mysql", "v1.1.5")],
            &[disk("mysql", "v1.1.5")],
            &[],
        );
        assert_eq!(
            entries[0].drift,
            Some(ExtensionDrift::UnregisteredOnDisk)
        );
        assert_eq!(entries[0].source, "unknown", "没登记就不知道来源，不能默认 official");
    }

    #[test]
    fn unknown_kernel_never_produces_a_fake_mismatch() {
        // `unknown` 只屏蔽「版本比较」这一条，不屏蔽「有没有登记过」
        assert_eq!(
            judge(false, true, true, None, Some("v1.1.5")),
            Some(ExtensionDrift::UnregisteredOnDisk),
            "没登记却能用，仍要报来路不明"
        );
        let mut row = registered("excel", "unknown", "official");
        row.kernel_version = "unknown".to_string();
        assert_eq!(
            judge(true, true, true, Some(&row), Some("v1.1.5")),
            None,
            "登记的内核版本是 unknown 时不参与比较，否则会全是假阳性"
        );
    }

    #[test]
    fn severity_orders_the_drift_kinds() {
        assert!(
            ExtensionDrift::RegisteredMissingOnDisk.severity()
                < ExtensionDrift::KernelMismatch {
                    registered: "a".into(),
                    running: "b".into()
                }
                .severity()
        );
        assert!(
            ExtensionDrift::NotReportedByDuckdb.severity()
                < ExtensionDrift::UnregisteredOnDisk.severity()
        );
    }
}
