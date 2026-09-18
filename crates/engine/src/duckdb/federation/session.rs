//! 联邦会话：把多个源挂到**同一条** DuckDB 连接上（见 `docs/architecture/federation/`）
//!
//! ## 与 `accel` 的关系（先说清楚，避免又长出一套）
//!
//! `accel` 是“一条源一条连接”（整库 `ATTACH … (READ_ONLY)` + `USE`）；联邦是它的**推广**：
//! 同一条连接挂**多条**源、每条一个别名，`USE` 指向**主源**。所以这里直接复用 accel 的：
//!
//! 1. [`mount_lock`]（`INSTALL` / `LOAD` 是进程级资源，并发建会话会撞文件）；
//! 2. [`run_sql_on`]（读写分流与 Arrow 转换与加速档**同一份**，同一句在两个档上形状一致）；
//! 3. 写保护双保险（`ATTACH … (READ_ONLY)` 让 DuckDB 自己拒 + 编辑器侧提交前拒）。
//!
//! ## 三条会话内规则
//!
//! - **挂不上的源不阻断**：记下原话继续挂下一个（部分可用好过整体失败）；
//! - **主源**：未限定名只在主源解析；请求的主源不可用 → 回退到第一个可用源，
//!   并留下 `primary_note`（界面上要说出来，不能悄悄换）；
//! - **只读**：源一律 `READ_ONLY` 挂载；本地临时对象（`CREATE TEMP TABLE`）照常允许。

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use duckdb::Connection;

use shared::error::CoreError;
use shared::models::QueryResult;

use super::super::accel::{mount_lock, run_sql_on};
use super::super::manager::DuckDBManager;
use super::registry::{FederatedSource, MountState, MountedSource, SessionSnapshot};

/// 一条联邦会话：DuckDB 内存库 + 一批只读挂载
pub struct FederatedSession {
    conn: Mutex<Connection>,
    /// 中断句柄（**建会话时就取出来**：真正跑查询时连接锁被占着，那时再取就晚了）
    interrupt: Arc<duckdb::InterruptHandle>,
    /// 现在有没有语句在跑（中断的“有没有可中断的”靠它，不凭猜）
    running: AtomicBool,
    mounted: Mutex<Vec<MountedSource>>,
    primary: Mutex<Option<String>>,
    primary_note: Mutex<Option<String>>,
    mounted_at: std::time::SystemTime,
}

impl std::fmt::Debug for FederatedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 连接没法 Debug：打印可读摘要（主源 + 源数），测试与日志都用它
        f.debug_struct("FederatedSession")
            .field("primary", &self.primary.lock().ok().and_then(|p| p.clone()))
            .field(
                "sources",
                &self.mounted.lock().map(|m| m.len()).unwrap_or(0),
            )
            .finish()
    }
}

impl FederatedSession {
    /// 建会话并**逐个**挂载（挂不上的记原因，不中断）
    ///
    /// 会做 I/O（首次装扩展要联网一次、`ATTACH` 要连源库），属于**事件路径**；
    /// 界面可用性看 [`Self::snapshot`]（纯内存读）。
    pub fn open(
        sources: &[FederatedSource],
        requested_primary: Option<&str>,
    ) -> Result<Self, String> {
        // 会话内重名先拦：两个源叫一个名会静默指到其中一个（那是最难查的一类错）
        let mut seen: Vec<String> = Vec::new();
        for source in sources {
            if seen.iter().any(|alias| alias == &source.alias) {
                return Err(format!("别名 {} 在这次的源里出现了两次", source.alias));
            }
            seen.push(source.alias.clone());
        }

        let conn = open_configured()?;

        // 装扩展与一批 ATTACH 串行（`INSTALL` 写目录、`LOAD` 是连接级，都要独占）
        let _mount = mount_lock();
        let mut installed: HashSet<&'static str> = HashSet::new();
        for source in sources {
            if let Some(extension) = source.kind.extension()
                && installed.insert(extension)
            {
                install_and_load(&conn, extension)?;
            }
        }

        let mut mounted = Vec::with_capacity(sources.len());
        for source in sources {
            mounted.push(mount_one(&conn, source));
        }

        let (primary, primary_note) = pick_primary(&mounted, requested_primary);
        if let Some(alias) = primary.as_deref() {
            use_catalog(&conn, alias)?;
        }

        let interrupt = conn.interrupt_handle();
        Ok(Self {
            conn: Mutex::new(conn),
            interrupt,
            running: AtomicBool::new(false),
            mounted: Mutex::new(mounted),
            primary: Mutex::new(primary),
            primary_note: Mutex::new(primary_note),
            mounted_at: std::time::SystemTime::now(),
        })
    }

    /// 在本地库上执行一句（读语句出 Arrow 批，写语句出影响行数）
    ///
    /// 源库对象的写在引擎侧就被 `READ_ONLY` 拒（这里不做第二套判定：判据只有一处）。
    pub fn run(&self, sql: &str) -> Result<QueryResult, CoreError> {
        let conn = self.lock()?;
        run_sql_on(&conn, &self.running, sql)
    }

    /// 重新挂载某个源：源库里**新建的表**要这样才看得见（数据本来就是实时的）
    ///
    /// 先 `USE memory` 再 `DETACH`：默认 catalog 指向该源时 DuckDB 拒绝卸载它。
    pub fn refresh(&self, alias: &str) -> Result<(), String> {
        let source = self
            .mounted
            .lock()
            .map_err(|_| "会话状态锁被污染".to_string())?
            .iter()
            .find(|entry| entry.alias() == alias)
            .map(|entry| entry.source.clone())
            .ok_or_else(|| format!("会话里没有源 {alias}"))?;

        let conn = self.lock().map_err(|error| error.to_string())?;
        let was_primary = self
            .primary
            .lock()
            .map(|primary| primary.as_deref() == Some(alias))
            .unwrap_or(false);

        conn.execute_batch(&format!("USE memory; DETACH {alias}"))
            .map_err(|error| format!("卸载 {alias} 失败：{error}"))?;
        let state = match conn.execute_batch(&source.attach_sql()) {
            Ok(()) => MountState::Ready {
                tables: count_tables(&conn, alias).unwrap_or(0),
            },
            Err(error) => MountState::Failed(attach_error(alias, &error)),
        };

        if let Ok(mut mounted) = self.mounted.lock()
            && let Some(entry) = mounted.iter_mut().find(|entry| entry.alias() == alias)
        {
            entry.state = state.clone();
        }
        // 重挂之后主源要 `USE` 回去（否则未限定名会解析到 memory 上）
        if was_primary && matches!(state, MountState::Ready { .. }) {
            use_catalog(&conn, alias).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// 切换主源（未限定名从此在它里面解析）
    pub fn set_primary(&self, alias: &str) -> Result<(), String> {
        let ready = {
            let mounted = self
                .mounted
                .lock()
                .map_err(|_| "会话状态锁被污染".to_string())?;
            let entry = mounted
                .iter()
                .find(|entry| entry.alias() == alias)
                .ok_or_else(|| format!("会话里没有源 {alias}"))?;
            entry.is_ready()
        };
        if !ready {
            return Err(format!("源 {alias} 当前不可用，不能设为主源"));
        }
        let conn = self.lock().map_err(|error| error.to_string())?;
        use_catalog(&conn, alias).map_err(|error| error.to_string())?;
        if let Ok(mut primary) = self.primary.lock() {
            *primary = Some(alias.to_string());
        }
        if let Ok(mut note) = self.primary_note.lock() {
            *note = None; // 用户显式选过了，回退说明就该收起来
        }
        Ok(())
    }

    /// 会话快照（**界面读这个**：纯内存，渲染路径可调）
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            primary: self.primary.lock().ok().and_then(|primary| primary.clone()),
            sources: self
                .mounted
                .lock()
                .map(|mounted| mounted.clone())
                .unwrap_or_default(),
            primary_note: self.primary_note.lock().ok().and_then(|note| note.clone()),
        }
    }

    /// 中断这条会话上正在跑的语句（三态可读，与加速档同一语义）
    pub fn interrupt(&self) -> Result<bool, String> {
        if !self.running.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(false);
        }
        self.interrupt.interrupt();
        Ok(true)
    }

    /// 会话活了多久（诊断）
    pub fn mounted_for(&self) -> std::time::Duration {
        self.mounted_at
            .elapsed()
            .unwrap_or(std::time::Duration::ZERO)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, CoreError> {
        self.conn.lock().map_err(|error| {
            CoreError::common(shared::error::CommonError::General(format!(
                "联邦会话连接锁被污染：{error}"
            )))
        })
    }
}

/// 开一条按产品纪律配置好的内存连接（扩展目录 / 内存闸 / 溢写口 / 不静默联网）
fn open_configured() -> Result<Connection, String> {
    let conn = Connection::open_in_memory().map_err(|error| format!("开联邦分析连接失败：{error}"))?;
    DuckDBManager::configure_connection(&conn)
        .map_err(|error| format!("配置联邦分析连接失败：{error}"))?;
    Ok(conn)
}

/// 装 + 加载一个扩展（失败原因里带上“离线怎么办”）
fn install_and_load(conn: &Connection, extension: &str) -> Result<(), String> {
    conn.execute_batch(&format!("INSTALL {extension}; LOAD {extension}"))
        .map_err(|error| {
            format!(
                "联邦需要 DuckDB 扩展 {extension}，安装失败：{error}（首次需要能访问 DuckDB 扩展源；离线时请先把扩展放进 {}）",
                paths::extensions_dir().display()
            )
        })
}

/// 挂一个源（**失败不抛**：状态就是结论）
fn mount_one(conn: &Connection, source: &FederatedSource) -> MountedSource {
    let state = match conn.execute_batch(&source.attach_sql()) {
        Ok(()) => MountState::Ready {
            tables: count_tables(conn, &source.alias).unwrap_or(0),
        },
        Err(error) => MountState::Failed(attach_error(&source.alias, &error)),
    };
    MountedSource {
        source: source.clone(),
        state,
    }
}

/// 挂载 / 卸载失败的原话（点名源：跨源场景里“哪个源出问题”是最重要的信息）
fn attach_error(alias: &str, error: &duckdb::Error) -> String {
    format!("源 {alias}：{error}")
}

/// 挂载时的表数量（纯展示；数不出来记 0，不因此把源判成失败）
fn count_tables(conn: &Connection, alias: &str) -> Result<usize, String> {
    conn.query_row(
        "SELECT count(*) FROM duckdb_tables() WHERE database_name = ?",
        [alias],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count as usize)
    .map_err(|error| error.to_string())
}

/// 切默认 catalog 到某个源
fn use_catalog(conn: &Connection, alias: &str) -> Result<(), String> {
    conn.execute_batch(&format!("USE {alias}"))
        .map_err(|error| format!("切主源到 {alias} 失败：{error}"))
}

/// 选主源：指定的可用就用它；否则回退到第一个可用源并**留下说明**
fn pick_primary(
    mounted: &[MountedSource],
    requested: Option<&str>,
) -> (Option<String>, Option<String>) {
    let ready: Vec<&str> = mounted
        .iter()
        .filter(|entry| entry.is_ready())
        .map(|entry| entry.alias())
        .collect();

    if let Some(want) = requested {
        if ready.contains(&want) {
            return (Some(want.to_string()), None);
        }
        let fallback = ready.first().map(|alias| (*alias).to_string());
        let note = match fallback.as_deref() {
            Some(alias) => Some(format!("主源 {want} 不可用，已改用 {alias}")),
            None => Some(format!("主源 {want} 不可用，当前没有可用源")),
        };
        return (fallback, note);
    }
    (ready.first().map(|alias| (*alias).to_string()), None)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::FederatedSession;
    use crate::duckdb::federation::registry::{FederatedSource, MountState};

    /// 造一个 DuckDB 文件源（**离线可跑**：duckdb 文件挂载不需要任何扩展）
    fn file_source(dir: &std::path::Path, name: &str, alias: &str, rows: i64) -> FederatedSource {
        let path = dir.join(format!("{name}.duckdb"));
        let conn = duckdb::Connection::open(&path).expect("建源文件");
        conn.execute_batch(&format!(
            "CREATE TABLE items AS SELECT i AS id, 'v' || i AS name FROM range({rows}) AS r(i)"
        ))
        .expect("建表");
        drop(conn);
        FederatedSource::new(name, alias, "duckdb", path.to_str().expect("路径")).expect("组装源")
    }

    fn workdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_fed_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    /// 两个源跨源 join：未限定名走主源，限定名走各自
    #[test]
    fn two_sources_join_across_with_a_primary() {
        let dir = workdir("join");
        let alpha = file_source(&dir, "alpha", "alpha", 3);
        let beta = file_source(&dir, "beta", "beta", 3);
        let session = FederatedSession::open(&[alpha, beta], Some("alpha")).expect("建会话");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.ready_count(), 2, "{snapshot:?}");
        assert_eq!(snapshot.primary.as_deref(), Some("alpha"));
        assert!(snapshot.primary_note.is_none());

        // 未限定名走主源
        let result = session.run("SELECT count(*) AS n FROM items").expect("主源可查");
        assert_eq!(result.to_rows()[0][0].to_string(), "3");

        // 跨源 join 用限定名
        let joined = session
            .run("SELECT count(*) AS n FROM alpha.items a JOIN beta.items b ON a.id = b.id")
            .expect("跨源 join");
        assert_eq!(joined.to_rows()[0][0].to_string(), "3");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 一个源挂不上：其余照常可用，失败的原因留在快照里
    #[test]
    fn a_broken_source_does_not_take_the_session_down() {
        let dir = workdir("broken");
        let good = file_source(&dir, "good", "good", 2);
        let missing = FederatedSource::new(
            "missing",
            "missing",
            "duckdb",
            dir.join("nope.duckdb").to_str().expect("路径"),
        )
        .expect("组装源");

        // 请求的主源正好是坏的那个 → 回退到可用的，并留下说明
        let session =
            FederatedSession::open(&[missing, good], Some("missing")).expect("会话仍该建起来");
        let snapshot = session.snapshot();
        assert_eq!(snapshot.ready_count(), 1, "{snapshot:?}");
        assert_eq!(snapshot.primary.as_deref(), Some("good"), "回退到可用源");
        assert!(
            snapshot.primary_note.as_deref().unwrap_or("").contains("missing"),
            "回退要说出来：{:?}",
            snapshot.primary_note
        );
        // 失败源保留在清单里，带原话
        let failed = snapshot
            .sources
            .iter()
            .find(|entry| entry.alias() == "missing")
            .expect("失败源不该消失");
        assert!(matches!(failed.state, MountState::Failed(_)), "{failed:?}");

        let result = session.run("SELECT count(*) AS n FROM items").expect("可用源照查");
        assert_eq!(result.to_rows()[0][0].to_string(), "2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 会话内重名直接拒（两个源一个名会静默指到其中一个）
    #[test]
    fn duplicate_aliases_are_refused_before_mounting() {
        let dir = workdir("dup");
        let a = file_source(&dir, "a", "same", 1);
        let b = file_source(&dir, "b", "same", 1);
        let error = FederatedSession::open(&[a, b], None).expect_err("该拒");
        assert!(error.contains("same"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 重挂某源后状态与表数不变、主源仍在它上面；切主源后未限定名跟着走
    ///
    /// 注：“源库新建的表重挂后可见”在**单进程测试里验不了**：DuckDB 文件同一进程只允许
    /// 一个连接（`File is already open in …`），会话挂着它就写不进去；那一条靠真机探针
    /// （MySQL / SQLite 源，见 `tests/duckdb_accel_probe.rs`）验。
    #[test]
    fn refresh_keeps_the_source_and_primary_switches_resolution() {
        let dir = workdir("refresh");
        let first = file_source(&dir, "first", "first", 1);
        let second = file_source(&dir, "second", "second", 2);
        let session =
            FederatedSession::open(&[first.clone(), second.clone()], Some("first")).expect("建会话");

        // 重挂：状态仍是可用、表数正确，主源没丢
        session.refresh("first").expect("重挂");
        let snapshot = session.snapshot();
        let entry = snapshot
            .sources
            .iter()
            .find(|entry| entry.alias() == "first")
            .expect("源还在");
        assert!(entry.is_ready(), "重挂后该是可用的：{entry:?}");
        assert_eq!(snapshot.primary.as_deref(), Some("first"));
        let result = session.run("SELECT count(*) AS n FROM items").expect("主源照常");
        assert_eq!(result.to_rows()[0][0].to_string(), "1");

        // 切主源：未限定名解析到 second（items 行数从 1 变 2）
        session.set_primary("second").expect("切主源");
        let snapshot = session.snapshot();
        assert_eq!(snapshot.primary.as_deref(), Some("second"));
        let result = session.run("SELECT count(*) AS n FROM items").expect("查");
        assert_eq!(result.to_rows()[0][0].to_string(), "2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 写源库对象被 DuckDB 自己拒（只读是引擎侧的第一道闸）
    #[test]
    fn writing_to_a_source_is_refused_by_the_engine() {
        let dir = workdir("readonly");
        let source = file_source(&dir, "ro", "ro", 1);
        let session = FederatedSession::open(&[source], None).expect("建会话");
        let error = session
            .run("INSERT INTO ro.items VALUES (99, 'x')")
            .expect_err("写源库该被拒");
        assert!(
            error.to_string().to_lowercase().contains("read-only")
                || error.to_string().to_lowercase().contains("read only"),
            "{error}"
        );
        // 本地临时对象照常允许（分析要用）
        session
            .run("CREATE TEMP TABLE scratch AS SELECT 1 AS n")
            .expect("本地临时表可以建");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
