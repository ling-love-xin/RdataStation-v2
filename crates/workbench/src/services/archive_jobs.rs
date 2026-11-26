//! 源表「归档到分析库…」的后台通道（铁律 H 组第 35 条前半 + E 组第 25 条）。
//!
//! 归档 = 把一张**已经挂在分析实例里**的源表复制成项目分析库
//! （`{项目}/.RSmeta/analytics.duckdb`）里的持久表，并登记成表型资产——两步不可分
//! （铁律第 11 条：只写表不登记就是"库里有表、资产列表里没有"的半截状态）。
//!
//! 为什么不在 UI 线程做：搬运要碰两次 DuckDB（源侧 `COPY … TO parquet` + 项目库
//! `CREATE TABLE AS`），项目库写连接还要等 `ProjectDatabaseManager` 的异步句柄；
//! 大表会直接把整窗卡住。
//!
//! 形态与 [`super::load_jobs`] 一致：worker 线程干活、回执按任务号认领、面板自持一次性小泵。
//! 与「载入内存」的区别只有落点与寿命：载入落 `tmp_l_*`（会话级、不登记），
//! 归档落项目库（持久、登记资产）。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use shared::error::{CommonError, CoreError};

use crate::components::data_objects::SourceRef;

/// 一次性任务号：面板用它认领**自己那次**归档的回执（并发点两次也不会拿错）。
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

/// 未取走的回执：`(任务号, 回执文案, 是否成功)`。
static OUTCOMES: Mutex<Vec<(u64, String, bool)>> = Mutex::new(Vec::new());

/// 提交一次归档。返回任务号（面板的泵按它认领回执）。
///
/// `dest_table` 是用户**在对话框里确认过**的表名（默认 `<源>_<表>`，见
/// [`engine::analysis::materialize::archive_table_name`]）；撞名不自动改名——
/// 项目分析库里的持久表是用户的，覆盖它得由用户再说一次。
pub fn start(
    project_root: PathBuf,
    source: SourceRef,
    source_name: String,
    table: String,
    qualified: String,
    dest_table: String,
) -> u64 {
    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    let job_for_worker = job;
    let note_base = format!("归档「{table}」");
    std::thread::Builder::new()
        .name("rds-archive-table".to_string())
        // 与载入同档：大表 CTAS 会深递归进 DuckDB 的算子栈
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let outcome = archive(&project_root, &source, &qualified, &dest_table);
            let (note, ok) = match outcome {
                Ok((rows, asset)) => (
                    format!(
                        "{note_base} → 项目分析库「{dest_table}」（{rows} 行）{asset}"
                    ),
                    true,
                ),
                Err(reason) => (
                    format!("{note_base}失败：{reason}（源：{source_name}）"),
                    false,
                ),
            };
            push(job_for_worker, note, ok);
        })
        .ok();
    job
}

/// 取走某个任务号的回执（没有就 `None`）。取走即出队：回执只播一次。
pub fn take_for(job: u64) -> Option<(String, bool)> {
    let mut outcomes = OUTCOMES.lock().unwrap_or_else(|e| e.into_inner());
    let index = outcomes.iter().position(|(id, _, _)| *id == job)?;
    let (_, note, ok) = outcomes.remove(index);
    Some((note, ok))
}

fn push(job: u64, note: String, ok: bool) {
    let mut outcomes = OUTCOMES.lock().unwrap_or_else(|e| e.into_inner());
    outcomes.push((job, note, ok));
}

fn core_error(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

/// 真活儿：先在**源侧**导出中转文件，再拿项目分析库的写连接建表 + 登记资产。
///
/// 顺序固定「先源后项目」：会话 / 单例的锁先拿到，项目库写连接后拿——反过来
/// 的实现若在别处也先拿项目，就凑出死锁（与 `persist_jobs` 同一条纪律）。
///
/// 源库连接（还没挂进分析实例）没有可执行的位置：按「说清为什么」返回，不装作能归档。
fn archive(
    project_root: &std::path::Path,
    source: &SourceRef,
    qualified: &str,
    dest_table: &str,
) -> Result<(u64, String), String> {
    use engine::analysis::materialize::archive_select_into;

    let select = format!("SELECT * FROM {qualified}");
    // 每种源给一个 `run`（在自己的连接上执行 SQL）：会话 / 单例的连接都藏在私有 Mutex 里，
    // 引擎不替它们选类型，所以这里把「哪条连接」翻成闭包。
    let written: (u64, Option<i32>) = match source {
        SourceRef::FileView { .. } => {
            let singleton = engine::services::duckdb_service::DuckDbService::get_or_create_duckdb()
                .map_err(|e| format!("取内存分析库失败: {e}"))?;
            let conn = singleton
                .lock()
                .map_err(|e| format!("内存分析库锁中毒: {e}"))?;
            crate::services::persist_jobs::with_project_write_connection(project_root, |dest| {
                let rows = archive_select_into(&select, dest_table, |sql| {
                    conn.execute_batch(sql)
                        .map_err(|e| core_error(e.to_string()))
                }, dest)
                .map_err(|e| e.to_string())?;
                let columns = crate::services::persist_jobs::count_columns(dest, dest_table);
                Ok((rows, columns))
            })?
        }
        SourceRef::Accel { conn_id } => {
            // 与载入同一口径：悬在**挂着这个源的分析实例**里跑（`ATTACH` 实例级共享）
            let instance = engine::duckdb::instance::current()
                .ok_or_else(|| "分析实例尚未启动（先跑一条查询）".to_string())?;
            if instance.alias_for_conn(conn_id).is_none() {
                return Err(
                    "这个源还没挂进分析实例（重新展开它，或跑一条加速档查询再试）".to_string(),
                );
            }
            let session = instance.session_for_conn(conn_id)?;
            crate::services::persist_jobs::with_project_write_connection(project_root, |dest| {
                let rows = archive_select_into(&select, dest_table, |sql| {
                    session.run(sql).map(|_| ())
                }, dest)
                .map_err(|e| e.to_string())?;
                let columns = crate::services::persist_jobs::count_columns(dest, dest_table);
                Ok((rows, columns))
            })?
        }
        SourceRef::Federated { owner, .. } => {
            let session = engine::duckdb::instance::current()
                .and_then(|instance| instance.session_for_owner(owner))
                .ok_or_else(|| {
                    "联邦会话已经不在了（重新展开这个源，或重跑一条联邦查询再试）".to_string()
                })?;
            crate::services::persist_jobs::with_project_write_connection(project_root, |dest| {
                let rows = archive_select_into(&select, dest_table, |sql| {
                    session.run(sql).map(|_| ())
                }, dest)
                .map_err(|e| e.to_string())?;
                let columns = crate::services::persist_jobs::count_columns(dest, dest_table);
                Ok((rows, columns))
            })?
        }
        SourceRef::Connection { .. } => {
            return Err(
                "这张表还没挂进分析实例：先在连接上开「本地加速」或联邦，挂上之后再归档"
                    .to_string(),
            )
        }
    };

    let (rows, columns) = written;
    // 表写完才谈得上登记（第 11 条：两步不可分）。登记失败**不回滚表**——
    // 数据是用户的，不能因为索引没落上就把它丢掉；把两件事分开报，由回执说清。
    let asset = match crate::services::persist_jobs::register_table_asset(
        project_root,
        dest_table,
        dest_table,
        rows,
        columns,
    ) {
        Ok(outcome) => format!("，{}", outcome.notice()),
        Err(reason) => format!("；表已落，但资产没登记上：{reason}"),
    };
    Ok((rows, asset))
}

#[cfg(test)]
mod tests {
    use super::{take_for, OUTCOMES};

    /// 回执按任务号认领、只播一次（与载入同一套认领语义）。
    #[test]
    fn outcomes_are_claimed_by_job_once() {
        {
            let mut outcomes = OUTCOMES.lock().unwrap_or_else(|e| e.into_inner());
            outcomes.clear();
            outcomes.push((3, "归档「a」".to_string(), true));
            outcomes.push((5, "归档「b」失败".to_string(), false));
        }
        let three = take_for(3).expect("3 号回执应当还在");
        assert!(three.1);
        assert!(take_for(3).is_none(), "回执只播一次");
        let five = take_for(5).expect("5 号回执应当还在");
        assert!(!five.1);
        assert!(take_for(5).is_none());
        assert!(take_for(6).is_none(), "没提交过的任务号不该有回执");
    }
}
