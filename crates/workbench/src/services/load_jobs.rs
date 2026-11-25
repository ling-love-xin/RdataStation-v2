//! 源表「载入内存」的后台通道（铁律 H 组第 35 条 + E 组第 25 条）。
//!
//! 载入 = 把一张**已经挂在分析实例里**的源表复制成进程单例的 `tmp_l_*` 表
//! （`engine::analysis::materialize`）。这一步要碰 DuckDB 的大表搬运（会话侧
//! `COPY … TO parquet` + 单例侧 `CREATE TABLE AS`），不能摆在 UI 线程上——大表会把整窗卡住。
//!
//! 形态与 [`super::export_jobs`] 同一套路：worker 线程干活，结果进静态队列，
//! 面板一侧自己开一枚**一次性**的小泵把回执取回来（说完 / 标脏就结束，不常驻）。
//! 与导出回执分两口队列，是因为导出的泵在宿主、载入的泵在面板：混在一起会让
//! 「谁负责把它取走」变成两处都要知道的事。

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::components::data_objects::SourceRef;

/// 一次性任务号：面板用它认领**自己那次**载入的回执（并发点两次也不会拿错）。
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

/// 未取走的回执：`(任务号, 回执文案, 是否成功)`。
static OUTCOMES: Mutex<Vec<(u64, String, bool)>> = Mutex::new(Vec::new());

/// 提交一次载入。返回任务号（面板的泵按它认领回执）。
///
/// `qualified` 是引擎生成的限定名（面板叶子的「复制名」就是它），只在**拥有这张表的
/// 那条连接**上合法——所以会话要从 `source` 里取，不接受调用方另给一条。
pub fn start(
    source: SourceRef,
    source_name: String,
    table: String,
    qualified: String,
    description: String,
) -> u64 {
    let job = NEXT_JOB.fetch_add(1, Ordering::Relaxed);
    let job_for_worker = job;
    let note_base = format!("载入「{table}」");
    std::thread::Builder::new()
        .name("rds-load-table".to_string())
        // 大表 CTAS 会深递归进 DuckDB 的算子栈：与源段列举同档（16 MB）
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let outcome = load(&source, &qualified, &description).map(|landed| {
                format!("{note_base} → {landed}（会话级：进程退出即消失）")
            });
            let (note, ok) = match outcome {
                Ok(note) => (note, true),
                Err(reason) => (format!("{note_base}失败：{reason}（源：{source_name}）"), false),
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

/// 真活儿：按源种类分派到「哪条连接上有这张表」。
///
/// - 文件库（`f_*`）本就在进程单例里：一条 CTAS 拷进 `memory`；
/// - 加速 / 联邦源在各自的专用会话里：会话侧写一次 parquet 中转，再落进单例；
/// - 源库连接（还没挂）：**没有**可执行的位置——先挂进分析实例再载入，
///   这条按「说清楚为什么」返回，不装作能载。
fn load(source: &SourceRef, qualified: &str, description: &str) -> Result<String, String> {
    use engine::analysis::materialize::{load_qualified_from_session, load_select_into_process_db};
    match source {
        SourceRef::FileView { .. } => {
            load_select_into_process_db(&format!("SELECT * FROM {qualified}"), description)
                .map_err(|error| error.to_string())
        }
        SourceRef::Accel { conn_id } => {
            // 载入要在**挂着这个源的分析实例**里跑：`ATTACH` 是实例级事实，随便一条
            // 实例会话都看得见它。没挂过就是真没挂，别在取会话时悄悄补挂。
            let instance = engine::duckdb::instance::current()
                .ok_or_else(|| "分析实例尚未启动（先跑一条查询）".to_string())?;
            if instance.alias_for_conn(conn_id).is_none() {
                return Err(
                    "这个源还没挂进分析实例（重新展开它，或跑一条加速档查询再试）".to_string(),
                );
            }
            let session = instance.session_for_conn(conn_id)?;
            load_qualified_from_session(qualified, description, |sql| {
                session.run(sql).map(|_| ())
            })
            .map_err(|error| error.to_string())
        }
        SourceRef::Federated { owner, .. } => {
            let session = engine::duckdb::federation::session::session_for(owner).ok_or_else(|| {
                "联邦会话已经不在了（重新展开这个源，或重跑一条联邦查询再试）".to_string()
            })?;
            load_qualified_from_session(qualified, description, |sql| {
                session.run(sql).map(|_| ())
            })
            .map_err(|error| error.to_string())
        }
        SourceRef::Connection { .. } => Err(
            "这张表还没挂进分析实例：先在连接上开「本地加速」或联邦，挂上之后再载入"
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{take_for, OUTCOMES};

    /// 回执按任务号认领：两个任务交错完成，各自只拿到自己那条，且只播一次。
    #[test]
    fn outcomes_are_claimed_by_job_once() {
        {
            let mut outcomes = OUTCOMES.lock().unwrap_or_else(|e| e.into_inner());
            outcomes.clear();
            outcomes.push((7, "载入「a」".to_string(), true));
            outcomes.push((9, "载入「b」失败".to_string(), false));
        }
        let seven = take_for(7).expect("7 号回执应当还在");
        assert!(seven.1);
        assert!(take_for(7).is_none(), "回执只播一次");
        let nine = take_for(9).expect("9 号回执应当还在");
        assert!(!nine.1);
        assert!(take_for(9).is_none());
        assert!(take_for(11).is_none(), "没提交过的任务号不该有回执");
    }
}
