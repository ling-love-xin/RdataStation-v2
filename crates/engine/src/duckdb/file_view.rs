//! 外部文件加载进**进程单例** DuckDB（`GLOBAL_DUCKDB`）
//!
//! 数据对象面板的「加源」走这里：二次分析与洞察本来就在这个实例里跑，
//! 把文件建成视图放进来，两处立刻都能查，不用各自 COPY 一份。
//!
//! 视图不物化：文件改了，下次查询结果跟着变。要“稳定快照”才需要物化成
//! 临时表（`tmp_q_`），那是另一条路。

use duckdb::Connection;
use shared::error::CommonError;
use std::sync::{Mutex, OnceLock};

use crate::CoreError;

/// 加载进来的文件：数据文件建出来的是 `f_*` 视图，数据库文件 ATTACH 成 catalog。
/// 两者都钉着绝对路径，面板要按路径回答"这一行是哪个文件"。
struct FileEntry {
    alias: String,
    path: String,
    /// `true` = ATTACH 进来的 catalog；`false` = 建出来的视图
    catalog: bool,
}

/// 「别名 → 原路径」的**唯一**索引。
///
/// `duckdb_databases()` 只说别名、说不出源路径，文件视图连别名都要自己记——所以这份
/// 冗余是必须的。但写入口收敛到 global 入口与清场口，别处一律只读：
///
/// - `load_file_global` / `load_file_as_global` 成功后记账（视图）；
/// - `load_database_global` / `load_database_as_global` 成功后记账（catalog）；
/// - `detach_database_global` 成功后忘掉；
/// - `clear_file_views` 每真删掉一条视图就忘掉对应别名。
///
/// 不放进吃调用方连接的 `load_file` / `load_database` / `detach_database`：那些可能跑在
/// 临时连接上（测试、「用 vN 分析」），记进进程索引会把别人的挂载算到本进程头上。
static FILE_INDEX: OnceLock<Mutex<Vec<FileEntry>>> = OnceLock::new();

fn file_index() -> &'static Mutex<Vec<FileEntry>> {
    FILE_INDEX.get_or_init(|| Mutex::new(Vec::new()))
}

/// 记一条（同名覆盖：`load_*_as` 本身是覆盖语义，同一别名只留最新那份路径）
fn note_file_entry(alias: &str, path: &str, catalog: bool) {
    let Ok(mut index) = file_index().lock() else {
        return;
    };
    index.retain(|entry| entry.alias != alias);
    index.push(FileEntry {
        alias: alias.to_string(),
        path: path.to_string(),
        catalog,
    });
}

fn forget_file_entry(alias: &str) {
    if let Ok(mut index) = file_index().lock() {
        index.retain(|entry| entry.alias != alias);
    }
}

/// 此刻本进程加载进来的全部文件（视图与 catalog 都算）：`(别名, 原路径)`。
///
/// 面板按 `is_database_file(path)` 分流：库文件进源段、数据文件进文件段。
pub fn file_entries() -> Vec<(String, String)> {
    file_index()
        .lock()
        .map(|index| {
            index
                .iter()
                .map(|entry| (entry.alias.clone(), entry.path.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// 只数文件视图（面板指纹用：纯计数，不克隆路径）
pub fn file_view_count() -> usize {
    file_index()
        .lock()
        .map(|index| index.iter().filter(|entry| !entry.catalog).count())
        .unwrap_or_default()
}

/// 只取 catalog 那一半（catalog 快照用）
pub fn catalog_files_snapshot() -> Vec<(String, String)> {
    file_index()
        .lock()
        .map(|index| {
            index
                .iter()
                .filter(|entry| entry.catalog)
                .map(|entry| (entry.alias.clone(), entry.path.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// 文件视图的命名前缀：面板靠它把「我加载进来的文件」与别的视图区分开
pub const FILE_VIEW_PREFIX: &str = "f_";

/// 文件名 → 视图名（`f_<ascii 小写>`）
///
/// 视图名要能直接写进 SQL，所以只留 ASCII 字母数字，其余折成下划线：
/// 这是表名尺子的同一套口径（`temp_table::sanitize_description`），中文名折掉是
/// 已知代价，界面上要把“实际视图名”显示出来。
pub fn alias_for(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("file");
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        format!("{FILE_VIEW_PREFIX}file")
    } else {
        format!("{FILE_VIEW_PREFIX}{trimmed}")
    }
}

/// 在一个连接上把文件挂成视图（返回实际使用的视图名）
pub fn load_file(conn: &Connection, path: &str) -> Result<String, CoreError> {
    let alias = alias_for(path);
    let sql = crate::duckdb::federation::session::file_view_sql(path, &alias)
        .map_err(|error| CoreError::common(CommonError::General(error)))?;
    conn.execute_batch(&sql)
        .map_err(|error| CoreError::common(CommonError::General(format!("加载文件失败：{error}"))))?;
    Ok(alias)
}

/// 清掉连接上全部**文件视图**（`f_*`），返回清掉的个数。
///
/// 只认 `main` schema 里**当前库**的 `VIEW`：挂载进来的数据库文件是 **catalog**
/// （`ATTACH`，见 [`load_database`]），不在这个清单里——那是「源」，切项目时该不该留
/// 由源的作用域决定（铁律 B 组），不是这里的事。所以这条只回答"加源进来的文件还在不在"。
///
/// 用途：项目切换清场（铁律 F 组）。`f_*` 视图钉着上一项目的文件路径，换项目还留着
/// 就是拿旧项目的文件算新项目的数。
pub fn clear_file_views(conn: &Connection) -> Result<usize, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_type = 'VIEW' AND table_schema = 'main' \
               AND table_catalog = current_database() AND table_name LIKE 'f\\_%' ESCAPE '\\'",
        )
        .map_err(|error| {
            CoreError::common(CommonError::General(format!("列文件视图失败：{error}")))
        })?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| {
            CoreError::common(CommonError::General(format!("列文件视图失败：{error}")))
        })?
        .filter_map(Result::ok)
        .collect();
    let mut cleared = 0;
    for name in names {
        let quoted = name.replace('"', "\"\"");
        // 单条失败不该挡住其余视图（一张坏了不能连坐整个清场口），记一笔继续
        match conn.execute_batch(&format!("DROP VIEW IF EXISTS \"{quoted}\"")) {
            Ok(()) => {
                forget_file_entry(&name);
                cleared += 1;
            }
            Err(error) => tracing::warn!("[duckdb] 删文件视图 {name} 失败: {error}"),
        }
    }
    Ok(cleared)
}

/// 在**进程单例**上清文件视图，**非阻塞**（`None` = 锁被占，留给下一次）。
///
/// 切项目在 UI 线程上跑，后台任务可能正持着内存库连接；排队等锁 = 界面假死到那个
/// 任务结束（与 mock / 结果集两条清场口同一纪律）。
pub fn try_clear_file_views_global() -> Option<usize> {
    let guard = crate::duckdb::manager::DuckDBManager::global().try_lock().ok()?;
    match clear_file_views(&guard) {
        Ok(count) => Some(count),
        Err(error) => {
            tracing::warn!("[duckdb] 清文件视图失败: {error}");
            None
        }
    }
}

/// 在进程单例上加载（事件路径调用：首次读文件会有 I/O）
pub fn load_file_global(path: &str) -> Result<String, CoreError> {
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| CoreError::common(CommonError::General("内存库连接锁被污染".to_string())))?;
    let alias = load_file(&conn, path)?;
    note_file_entry(&alias, path, false);
    Ok(alias)
}

/// 用**指定别名**把文件挂成视图（历史版本「用 vN 分析」用）。
///
/// 为什么要单独一个入口：历史副本与原文件同名，`alias_for` 会算出同一个视图名，
/// 直接 `load_file` 就把**当前版本**的视图覆盖掉了（用户会以为自己看的是当前版本）。
/// 调用方给全别名（`f_<stem>_v<N>`），覆盖语义与 [`load_file`] 相同。
pub fn load_file_as(conn: &Connection, path: &str, alias: &str) -> Result<(), CoreError> {
    let sql = crate::duckdb::federation::session::file_view_sql(path, alias)
        .map_err(|error| CoreError::common(CommonError::General(error)))?;
    conn.execute_batch(&sql)
        .map_err(|error| CoreError::common(CommonError::General(format!("加载文件失败：{error}"))))
}

/// 在进程单例上用指定别名建视图（事件路径调用：首次读文件会有 I/O）
pub fn load_file_as_global(path: &str, alias: &str) -> Result<(), CoreError> {
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| CoreError::common(CommonError::General("内存库连接锁被污染".to_string())))?;
    load_file_as(&conn, path, alias)?;
    note_file_entry(alias, path, false);
    Ok(())
}

/// 是不是**数据库文件**（`.duckdb` / `.sqlite`）：这类不走 reader 建视图，走
/// [`load_database`] `ATTACH` 挂成 catalog——表在库里，挂进来就能按
/// `别名.schema.表` 引用（2026-11-18 用户拍板：库文件走 ATTACH）。
pub fn is_database_file(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("duckdb") | Some("sqlite") | Some("sqlite3") | Some("db")
    )
}

/// 把数据库文件 `ATTACH` 成连接上的 catalog（返回挂载别名，`f_` 前缀与视图同一套）
///
/// - `.duckdb` 文件是内核原生能力，直接 `ATTACH … (READ_ONLY)`；
/// - `.sqlite` 要 `sqlite` 扩展（与联邦源同一套装机路径，可离线预置），带 `TYPE sqlite`；
/// - 只读挂载与联邦源同一纪律：文件是外部的，不从这条连接写它。
pub fn load_database(conn: &Connection, path: &str) -> Result<String, CoreError> {
    let alias = alias_for(path);
    load_database_as(conn, path, &alias)?;
    Ok(alias)
}

/// 用**指定别名**挂载数据库文件（历史版本「用 vN 分析」用）。
///
/// 与 [`load_file_as`] 同一理由：历史副本与原库同名，自动别名会撞上**当前版本**的
/// catalog，`ATTACH` 前的同名清理会顺手把当前版本卸掉。别名由调用方给全。
pub fn load_database_as(conn: &Connection, path: &str, alias: &str) -> Result<(), CoreError> {
    let is_sqlite = std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("sqlite")
                || ext.eq_ignore_ascii_case("sqlite3")
                || ext.eq_ignore_ascii_case("db")
        });
    if is_sqlite {
        // 装扩展在 mount_lock 内做（INSTALL 写扩展目录，并发会撞文件——与联邦同规）
        let _mount = crate::duckdb::accel::mount_lock();
        crate::duckdb::accel::ensure_extension(conn, crate::duckdb::accel::AccelKind::Sqlite)
            .map_err(|reason| CoreError::common(CommonError::General(reason)))?;
    }
    let quoted_alias = alias.replace('"', "\"\"");
    // 同名重复挂载先卸旧的：`ATTACH` 对已存在的别名直接报错（不像
    // `CREATE OR REPLACE VIEW` 天然幂等）。换路径重挂 = 覆盖语义，与视图重建同口径；
    // `DETACH` 不存在（首次挂载）会报错，忽略即可。默认 catalog 指向该别名时
    // DuckDB 拒绝卸载——单例的默认 catalog 恒为 memory，不走那个分支
    let _ = conn.execute_batch(&format!("USE memory; DETACH \"{quoted_alias}\""));
    let quoted_path = path.replace('\'', "''");
    let sql = if is_sqlite {
        format!("ATTACH '{quoted_path}' AS \"{quoted_alias}\" (TYPE sqlite, READ_ONLY)")
    } else {
        format!("ATTACH '{quoted_path}' AS \"{quoted_alias}\" (READ_ONLY)")
    };
    conn.execute_batch(&sql).map_err(|error| {
        CoreError::common(CommonError::General(format!("挂载数据库文件失败：{error}")))
    })?;
    Ok(())
}

/// 在进程单例上用指定别名挂载数据库文件（事件路径调用）
pub fn load_database_as_global(path: &str, alias: &str) -> Result<(), CoreError> {
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| CoreError::common(CommonError::General("内存库连接锁被污染".to_string())))?;
    load_database_as(&conn, path, alias)?;
    note_file_entry(alias, path, true);
    Ok(())
}

/// 在进程单例上挂载数据库文件（事件路径调用）
pub fn load_database_global(path: &str) -> Result<String, CoreError> {
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| CoreError::common(CommonError::General("内存库连接锁被污染".to_string())))?;
    let alias = load_database(&conn, path)?;
    note_file_entry(&alias, path, true);
    Ok(alias)
}

/// 把一个已挂载的 catalog 从连接上卸掉（`DETACH`）
///
/// 与 [`load_database`] 对称：卸载只动**挂载关系**，不碰磁盘上的库文件（只读挂载，
/// 本来也不会写它）。别名要先 `USE memory` 再 `DETACH`——默认 catalog 不能卸。
///
/// 别名没挂上时 DuckDB 会报错，原话透出去（调用方要能分清"没挂"与"卸失败"）。
pub fn detach_database(conn: &Connection, alias: &str) -> Result<(), CoreError> {
    let quoted = alias.replace('"', "\"\"");
    conn.execute_batch(&format!("USE memory; DETACH \"{quoted}\""))
        .map_err(|error| {
            CoreError::common(CommonError::General(format!("卸载「{alias}」失败：{error}")))
        })
}

/// 在进程单例上卸载 catalog（事件路径调用）
///
/// **引用计数拒绝（不变量 §6）**：还有文档会话挂着这个别名时不许摘——`ATTACH` 是实例级事实，
/// 摘了那些文档会当场报 catalog 不存在，正在跑的查询也会断。检查是纯内存读，先于拿连接锁。
pub fn detach_database_global(alias: &str) -> Result<(), CoreError> {
    if let Some(instance) = crate::duckdb::instance::current() {
        let holders = instance.catalog_holders(alias, &[]);
        if let Some(refusal) = crate::duckdb::instance::detach_refusal(alias, &holders) {
            return Err(CoreError::common(CommonError::General(refusal)));
        }
    }
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| CoreError::common(CommonError::General("内存库连接锁被污染".to_string())))?;
    detach_database(&conn, alias)?;
    forget_file_entry(alias);
    Ok(())
}

/// 列出**进程单例**上某个挂载别名（数据库文件）里的表 / 视图
///
/// 与联邦 / 加速源同一份列举实现（`duckdb_tables()` / `duckdb_views()` 按别名过滤，
/// 限定名三段）——数据对象面板「源行展开」不分挂的是网络源还是本机库文件。
pub fn list_tables_global(
    alias: &str,
) -> Result<Vec<crate::duckdb::accel::MountedTable>, String> {
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| "内存库连接锁被污染".to_string())?;
    crate::duckdb::accel::list_mounted_tables(
        &conn,
        alias,
        crate::duckdb::accel::AccelKind::DuckDb,
    )
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        alias_for, clear_file_views, detach_database, is_database_file, load_database,
        load_database_global, load_file, load_file_as, list_tables_global,
    };
    use std::path::PathBuf;

    fn workdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_file_view_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    #[test]
    fn the_alias_keeps_ascii_and_folds_the_rest() {
        assert_eq!(alias_for("/tmp/orders_2026.csv"), "f_orders_2026");
        assert_eq!(alias_for("/tmp/经营月报.xlsx"), "f_file");
    }

    #[test]
    fn a_csv_is_loadable_into_the_instance() {
        let dir = workdir("csv");
        let path = dir.join("orders.csv");
        std::fs::write(&path, "id,name\n1,a\n2,b\n").expect("写 csv");

        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        let alias = load_file(&conn, path.to_str().expect("路径")).expect("加载");
        assert_eq!(alias, "f_orders");
        // 视图建好了就能查：查不存在会报错，这里只看“不报错”
        conn.execute_batch("SELECT count(*) FROM f_orders")
            .expect("视图应能查");
    }

    /// 清文件视图：删掉 `f_*`，**不动** ATTACH 进来的库文件（铁律 F 组清场口）。
    ///
    /// 两件事一次钉住：视图真被删（再查报错）、库文件仍是 catalog 仍可查。
    #[test]
    fn clearing_file_views_drops_views_but_spares_attached_catalogs() {
        let dir = workdir("clear");
        let csv = dir.join("orders.csv");
        std::fs::write(&csv, "id,name\n1,a\n").expect("写 csv");
        let dbfile = dir.join("warehouse.duckdb");
        {
            let conn = duckdb::Connection::open(&dbfile).expect("建源库文件");
            conn.execute_batch("CREATE TABLE facts AS SELECT i AS id FROM range(3) AS r(i)")
                .expect("建表");
        }

        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        let view = load_file(&conn, csv.to_str().expect("路径")).expect("加载");
        let catalog = load_database(&conn, dbfile.to_str().expect("路径")).expect("挂载");
        assert_eq!(clear_file_views(&conn).expect("清视图"), 1);
        assert!(
            conn.execute_batch(&format!("SELECT count(*) FROM \"{view}\"")).is_err(),
            "文件视图应当已删：{view}"
        );
        assert!(
            conn.execute_batch(&format!("SELECT count(*) FROM \"{catalog}\".main.facts"))
                .is_ok(),
            "库文件是源，不该被这次清场带走：{catalog}"
        );
        // 幂等：没有视图可清时就是 0，不报错
        assert_eq!(clear_file_views(&conn).expect("再清"), 0);
    }

    /// 库文件按扩展分流：duckdb / sqlite 是库（ATTACH），csv 是数据（视图）
    #[test]
    fn database_files_are_recognized_by_extension() {
        assert!(is_database_file("D:/data/sales.duckdb"));
        assert!(is_database_file("D:/data/old.sqlite"));
        assert!(is_database_file("D:/data/x.db"));
        assert!(!is_database_file("D:/data/sales.csv"));
        assert!(!is_database_file("D:/data/report.parquet"));
    }

    /// 库文件在**任意连接**上可挂（连接级形态先行验证，不依赖进程单例）
    #[test]
    fn a_duckdb_file_attaches_on_a_connection() {
        let dir = workdir("attach_conn");
        let source = dir.join("warehouse.duckdb");
        {
            let conn = duckdb::Connection::open(&source).expect("建源库文件");
            conn.execute_batch("CREATE TABLE facts AS SELECT i AS id FROM range(3) AS r(i)")
                .expect("建表");
        }
        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        let alias = load_database(&conn, source.to_str().expect("路径")).expect("挂载");
        assert_eq!(alias, "f_warehouse");
        conn.execute_batch("SELECT count(*) FROM f_warehouse.main.facts")
            .expect("限定名可查");

        // 同名重挂（不同目录的同名文件 → 同一个别名）：ATTACH 不是幂等的，
        // 靠先 DETACH 实现覆盖语义——重挂后指向新文件
        let other_dir = dir.join("elsewhere");
        std::fs::create_dir_all(&other_dir).expect("建目录");
        let second = other_dir.join("warehouse.duckdb");
        std::fs::copy(&source, &second).expect("复制一份同名库");
        let alias_again =
            load_database(&conn, second.to_str().expect("路径")).expect("重挂要能成");
        assert_eq!(alias_again, "f_warehouse", "别名稳定（覆盖语义）");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 卸载与挂载对称：卸掉之后限定名查不到，且**不碰磁盘**上的库文件（只读挂载）。
    /// 别名没挂上时要把原因说清楚（不能静默当成功，面板会据此以为"已经卸了"）。
    #[test]
    fn detaching_a_catalog_removes_the_mount_but_keeps_the_file() {
        let dir = workdir("detach");
        let source = dir.join("warehouse.duckdb");
        {
            let conn = duckdb::Connection::open(&source).expect("建源库文件");
            conn.execute_batch("CREATE TABLE facts AS SELECT i AS id FROM range(3) AS r(i)")
                .expect("建表");
        }
        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        let alias = load_database(&conn, source.to_str().expect("路径")).expect("挂载");
        conn.execute_batch(&format!("SELECT count(*) FROM \"{alias}\".main.facts"))
            .expect("挂上后限定名可查");

        detach_database(&conn, &alias).expect("卸载");
        assert!(
            conn.execute_batch(&format!("SELECT count(*) FROM \"{alias}\".main.facts"))
                .is_err(),
            "卸掉后不该还能按别名查：{alias}"
        );
        assert!(source.is_file(), "卸载不删磁盘上的库文件");

        // 再卸一次：别名已不在，报错要带别名（面板据此说"没挂"）
        let again = detach_database(&conn, &alias).expect_err("重复卸载要报错");
        assert!(format!("{again}").contains(&alias), "{again}");

        let _ = std::fs::remove_dir_all(dir);
    }

    /// 历史版本「用 vN 分析」的别名纪律：副本与原文件同名，靠显式别名并列成两行，
    /// 谁也不能把谁顶掉（`alias_for` 自动别名会算出同一个名字）。
    #[test]
    fn an_explicit_alias_keeps_the_current_view_alive() {
        let dir = workdir("alias_two_views");
        let source = dir.join("dau.csv");
        std::fs::write(&source, "day,users\n1,10\n2,20\n").expect("写源文件");
        let version_dir = dir.join("versions").join("3");
        std::fs::create_dir_all(&version_dir).expect("建版本目录");
        let copy = version_dir.join("dau.csv");
        std::fs::copy(&source, &copy).expect("复制副本");

        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        let current = load_file(&conn, source.to_str().expect("路径")).expect("当前版本视图");
        assert_eq!(current, "f_dau");
        load_file_as(&conn, copy.to_str().expect("路径"), "f_dau_v3").expect("历史版本视图");

        // 两行并存：当前版本还在，历史版本也挂了（内容一样，验的是"都在"）
        conn.execute_batch(&format!("SELECT count(*) FROM \"{current}\""))
            .expect("当前版本视图还在");
        conn.execute_batch("SELECT count(*) FROM \"f_dau_v3\"")
            .expect("历史版本视图也挂了");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 进程单例路径：挂载后面板的列举口能看到表（含可粘贴的限定名）
    #[test]
    fn a_duckdb_file_mounts_and_lists_on_the_singleton() {
        let dir = workdir("attach_global");
        let source = dir.join("archive.duckdb");
        {
            let conn = duckdb::Connection::open(&source).expect("建源库文件");
            conn.execute_batch("CREATE TABLE dims AS SELECT i AS id FROM range(2) AS r(i)")
                .expect("建表");
        }
        let path = source.to_str().expect("路径");
        let alias = load_database_global(path).expect("挂到单例");
        assert_eq!(alias, "f_archive");
        let tables = list_tables_global("f_archive").expect("列举");
        let dims = tables.iter().find(|t| t.name == "dims").expect("有 dims");
        assert_eq!(dims.schema, "main");
        assert!(dims.qualified.contains("f_archive"), "{}", dims.qualified);
        // 收尾：单例是进程级的，别把挂载留给其他用例
        {
            let singleton = crate::duckdb::manager::DuckDBManager::global();
            let guard = singleton.lock().expect("单例锁");
            let _ = guard.execute_batch("DETACH f_archive");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
