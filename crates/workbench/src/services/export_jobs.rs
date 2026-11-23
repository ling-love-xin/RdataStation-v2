//! 导出写盘的**后台通道**（铁律 E 组第 25 条：写盘类动作走 worker + 状态栏回执）。
//!
//! 为什么必须离开 UI 线程：导出是 `COPY … TO`，几十万行以上要在引擎里跑几秒到几十秒；
//! 在事件回调里同步跑就是整窗卡死（连"取消"都点不动）。职责因此拆成两半：
//!
//! - [`start`]：UI 线程只做**参数校验与入队**，真正的写盘在独立线程；
//! - [`take_outcomes`]：宿主回执泵（`WorkbenchView::ensure_source_kids_pump` 那口 0.3 s
//!   的循环）在 UI 线程取走结果，经 `Shared::say` / `say_error` 进状态栏。
//!
//! 为什么用全局槽位而不是任务返回值：导出窗口不是实体（没有 `Context` / `Window`），
//! 回调返回后拿不到能在异步里 `update` 的句柄；回执泵是现成的 UI 线程落点，
//! 与 `source_kids_jobs` / `resource_jobs` 同一形态（后台写、泵里取）。

use std::path::PathBuf;
use std::sync::Mutex;

/// 一次导出的结果（写盘线程 → 回执泵）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOutcome {
    /// 状态栏文案（成功 / 失败各自组织好，泵里不再拼）。
    pub note: String,
    /// `true` = 写成了；`false` = 失败（走 `say_error`）。
    pub ok: bool,
}

static OUTCOMES: Mutex<Vec<ExportOutcome>> = Mutex::new(Vec::new());

/// 入队一次导出（**立即返回**，写盘在后台线程）。
///
/// `table` 是裸表名（引用名已剥掉库/引号），`path` 是带扩展名的目标文件。
/// 目录由调用方先建好（建目录失败属于参数错误，该在关窗前挡住）。
pub fn start(table: String, path: PathBuf, verb: String) {
    let path_text = path.display().to_string();
    std::thread::spawn(move || {
        let outcome = match engine::duckdb::export::export_table_global(&table, &path_text) {
            Ok(_) => ExportOutcome {
                note: format!("{verb}：{path_text}"),
                ok: true,
            },
            Err(error) => ExportOutcome {
                note: format!("{verb}失败（{path_text}）：{error}"),
                ok: false,
            },
        };
        push(outcome);
    });
}

fn push(outcome: ExportOutcome) {
    match OUTCOMES.lock() {
        Ok(mut queue) => queue.push(outcome),
        Err(poisoned) => {
            // 锁被污染也要把这条回执留下：写完的文件是真的，静默才是更坏的失败。
            tracing::warn!("[export] 回执队列锁被污染，仍尝试写入");
            poisoned.into_inner().push(outcome);
        }
    }
}

/// 取走当前全部回执（**先取空再逐条说**，与 `source_kids_jobs` 同纪律）。
pub fn take_outcomes() -> Vec<ExportOutcome> {
    match OUTCOMES.lock() {
        Ok(mut queue) => std::mem::take(&mut *queue),
        Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
    }
}

#[cfg(test)]
mod tests {
    use super::{take_outcomes, ExportOutcome};

    /// 队列是"取空"语义：泵每拍取一次，不能重复播报同一条回执。
    #[test]
    fn outcomes_are_drained_once() {
        let _ = take_outcomes();
        super::push(ExportOutcome {
            note: "导出：/tmp/x.csv".to_string(),
            ok: true,
        });
        let first = take_outcomes();
        assert_eq!(first.len(), 1);
        assert!(first[0].ok);
        assert!(take_outcomes().is_empty(), "同一条回执不能播报两次");
    }
}
