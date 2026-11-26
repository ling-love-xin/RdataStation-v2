//! 进程级 DuckDB 根连接。
//!
//! **硬约束**：同一个 DuckDB 文件在一个进程里只能有一条根连接；第二条连接即使是
//! `READ_ONLY` 也会失败（`File is already open ... same PID`，2026-11-26 真机探针）。
//! 所以项目分析库 / 全局分析库 / 分析实例不能再各开各的，必须共用这一条内存根连接，
//! 文件库通过 `ATTACH` 挂上来：
//!
//! ```text
//! memory（根，main）
//! ├── analytics    ← 当前项目 .RSmeta/analytics.duckdb（RW）
//! └── rds_global   ← data/system/analytics.duckdb（RW）
//! ```
//!
//! 需要「默认 catalog 是项目库」的旧调用方（`ProjectDatabaseManager`）拿
//! [`DuckDbRoot::handle`] 的克隆并 `USE analytics`；全局同理 `USE rds_global`。
//! `USE` 是会话级设置，不会改根连接或其他句柄的默认 catalog。
//!
//! 这里是**纯连接层**：不跑迁移、不配置内存上限、不认业务语义。迁移仍由
//! `store::persistence::{project_db, global_db}` 在拿到的句柄上执行。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use duckdb::Connection;

/// 项目分析库的固定 catalog 别名。
pub const PROJECT_ALIAS: &str = "analytics";
/// 全局分析库的固定 catalog 别名。
pub const GLOBAL_ALIAS: &str = "rds_global";

struct RootState {
    conn: Connection,
    project: Option<PathBuf>,
    global: Option<PathBuf>,
}

/// 进程内唯一的 DuckDB 根连接。
pub struct DuckDbRoot {
    state: Mutex<RootState>,
}

static ROOT: OnceLock<Mutex<Option<Arc<DuckDbRoot>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Arc<DuckDbRoot>>> {
    ROOT.get_or_init(|| Mutex::new(None))
}

/// 取（必要时建立）进程根连接，并确保**已存在**的底座文件挂上。
///
/// 不会创建缺失的库文件：项目刚建、全局库还没初始化时，底座就是“缺席”，
/// 第一次写再走 [`ensure_write`] 建库。已挂同一路径是幂等；换项目先 `DETACH` 旧路径。
pub fn ensure(project: Option<&Path>, global: Option<&Path>) -> Result<Arc<DuckDbRoot>, String> {
    ensure_inner(project, global, false)
}

/// 写路径专用：底座文件不存在时允许 `ATTACH` 建库。
///
/// 只有真正要写项目 / 全局分析库的调用方（`ProjectDatabaseManager::open` 写模式、
/// `GlobalDatabaseManager::new`）才配用；分析实例打开时仍走 [`ensure`]，不产生建库副作用。
pub fn ensure_write(
    project: Option<&Path>,
    global: Option<&Path>,
) -> Result<Arc<DuckDbRoot>, String> {
    ensure_inner(project, global, true)
}

fn ensure_inner(
    project: Option<&Path>,
    global: Option<&Path>,
    create_missing: bool,
) -> Result<Arc<DuckDbRoot>, String> {
    let mut guard = slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(root) = guard.as_ref() {
        root.ensure_bases(project, global, create_missing)?;
        return Ok(root.clone());
    }
    let root = Arc::new(DuckDbRoot::open()?);
    root.ensure_bases(project, global, create_missing)?;
    *guard = Some(root.clone());
    Ok(root)
}

/// 当前根连接（没有就是 `None`；**不做 I/O**）。
pub fn current() -> Option<Arc<DuckDbRoot>> {
    slot()
        .lock()
        .ok()?
        .as_ref()
        .map(|root| root.clone())
}

/// 建一条**不登记**的根连接（测试 / 独立实例用）。
///
/// 生产只应有 [`ensure`] 那一条进程根连接；测试里多个 `AnalysisInstance::open_tagged`
/// 若都共用进程根，`main` 里的持久表会跨用例泄漏（2026-11-26 engine 库 4 个失败的真因）。
/// 这里给测试一条独立的 `memory` 根，底座照常挂。
pub fn isolated(
    project: Option<&Path>,
    global: Option<&Path>,
) -> Result<Arc<DuckDbRoot>, String> {
    let root = Arc::new(DuckDbRoot::open()?);
    root.ensure_bases(project, global, false)?;
    Ok(root)
}

/// 放掉进程根连接（测试 / 退出用；还有句柄存活时文件句柄要等句柄先 drop）。
pub fn reset() {
    let mut guard = slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = guard.take();
}

impl DuckDbRoot {
    /// 建一条内存根连接；文件底座由 [`Self::ensure_bases`] 挂。
    fn open() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|error| format!("开 DuckDB 根连接失败：{error}"))?;
        Ok(Self {
            state: Mutex::new(RootState {
                conn,
                project: None,
                global: None,
            }),
        })
    }

    /// 确保请求的底座挂在根连接上（幂等；换项目先摘旧）。
    fn ensure_bases(
        &self,
        project: Option<&Path>,
        global: Option<&Path>,
        create_missing: bool,
    ) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(path) = project {
            match state.project.as_ref() {
                Some(current) if same_path(current, path) => {}
                Some(_) => {
                    // 换项目：必须先摘旧，否则 `analytics` 会静默指向上一个项目。
                    state
                        .conn
                        .execute_batch(&format!("DETACH {PROJECT_ALIAS}"))
                        .map_err(|error| {
                            format!(
                                "摘除旧项目分析库失败（旧会话可能还在用）：{error}"
                            )
                        })?;
                    state.project = None;
                }
                None => {}
            }
            if state.project.is_none() && (create_missing || path.is_file()) {
                attach(&state.conn, PROJECT_ALIAS, path, create_missing)?;
                state.project = Some(path.to_path_buf());
            }
        }

        if let Some(path) = global {
            match state.global.as_ref() {
                Some(current) if same_path(current, path) => {}
                Some(_) => {
                    state
                        .conn
                        .execute_batch(&format!("DETACH {GLOBAL_ALIAS}"))
                        .map_err(|error| {
                            format!("摘除旧全局分析库失败（旧会话可能还在用）：{error}")
                        })?;
                    state.global = None;
                }
                None => {}
            }
            if state.global.is_none() && (create_missing || path.is_file()) {
                attach(&state.conn, GLOBAL_ALIAS, path, create_missing)?;
                state.global = Some(path.to_path_buf());
            }
        }

        Ok(())
    }

    /// 复制一条**控制连接**（默认 catalog 仍是 `main`，不跑用户查询）。
    pub fn control(&self) -> Result<Connection, String> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state
            .conn
            .try_clone()
            .map_err(|error| format!("复制 DuckDB 控制连接失败：{error}"))
    }

    /// 复制一条会话连接，并把默认 catalog 指到 `alias`。
    ///
    /// 用途：旧 Manager 的写入句柄（`ProjectDatabaseManager` / `GlobalDatabaseManager`）
    /// 仍然按未限定表名写，必须让它们落到各自的库，而不是根内存库的 `main`。
    pub fn handle(&self, alias: &str) -> Result<Connection, String> {
        if alias != PROJECT_ALIAS && alias != GLOBAL_ALIAS {
            return Err(format!("不认识的底座别名：{alias}"));
        }
        let conn = self.control()?;
        conn.execute_batch(&format!("USE {alias}"))
            .map_err(|error| format!("把会话默认 catalog 切到 {alias} 失败：{error}"))?;
        Ok(conn)
    }

    /// 已挂的项目库路径（只读快照；没挂就是 `None`）。
    pub fn project_path(&self) -> Option<PathBuf> {
        self.state.lock().ok()?.project.clone()
    }

    /// 已挂的全局库路径（只读快照；没挂就是 `None`）。
    pub fn global_path(&self) -> Option<PathBuf> {
        self.state.lock().ok()?.global.clone()
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn attach(conn: &Connection, alias: &str, path: &Path, create_missing: bool) -> Result<(), String> {
    if !create_missing && !path.is_file() {
        return Err(format!("底座文件不存在：{}", path.display()));
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("建底座目录 {} 失败：{error}", parent.display()))?;
    }
    let literal = path.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("ATTACH '{literal}' AS {alias} (READ_WRITE)"))
        .map_err(|error| format!("挂载底座 {alias}（{}）失败：{error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "rds_duckdb_root_{tag}_{}_{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("临时目录");
        root
    }

    #[test]
    fn project_and_global_share_one_root_and_handles_use_their_catalog() {
        let root = temp_root("share");
        let project = root.join("project/.RSmeta/analytics.duckdb");
        let global = root.join("system/analytics.duckdb");

        let host = ensure_write(Some(&project), Some(&global)).expect("建根连接");
        let project_handle = host.handle(PROJECT_ALIAS).expect("项目句柄");
        project_handle
            .execute_batch("CREATE TABLE project_probe AS SELECT 1 AS a")
            .expect("项目句柄写项目库");

        let global_handle = host.handle(GLOBAL_ALIAS).expect("全局句柄");
        global_handle
            .execute_batch("CREATE TABLE global_probe AS SELECT 2 AS b")
            .expect("全局句柄写全局库");

        let control = host.control().expect("控制连接");
        let count: i64 = control
            .query_row("SELECT count(*) FROM analytics.project_probe", [], |row| row.get(0))
            .expect("从根连接读项目表");
        assert_eq!(count, 1);
        let count: i64 = control
            .query_row("SELECT count(*) FROM rds_global.global_probe", [], |row| row.get(0))
            .expect("从根连接读全局表");
        assert_eq!(count, 1);

        // 同一根连接上的跨库 CTAS（M6 的目标形态）。
        control
            .execute_batch(
                "CREATE TABLE analytics.copied AS SELECT * FROM rds_global.global_probe",
            )
            .expect("同实例跨库 CTAS");

        drop(project_handle);
        drop(global_handle);
        drop(control);
        drop(host);
        let _ = std::fs::remove_dir_all(&root);
    }
}
