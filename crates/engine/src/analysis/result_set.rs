//! 分析内核 · 结果集（`tmp_q_`）取数面。
//!
//! 结果集二次分析的入口是「表已经在库里了，我拿它再分析一次」。所以它**不需要挂载**
//! 任何源——结果集就是进程单例里的 `tmp_q_` 临时表（`DuckDbService::create_duckdb_temp_table`
//! 建表即登记）。把这条也收进内核的意义不在于开会话，而在于**把「结果集在哪」说死一处**：
//! 它不在导航树连的源上，不在加速会话里，就在那个进程级内存单例里。
//!
//! 由此得出两条口径，也解释了两个历史疑问：
//!
//! 1. **通道 = `ProcessDuckDb`**：不是 `Accelerated`——加速会话是另一条连接，看不到
//!    单例里的 `tmp_q_`（表不存在），把它当加速档处理是反的；
//! 2. **列表来源 = 库本身**：走 `TempTableManager::list_by_source(conn, Query)`，
//!    它按前缀查 `information_schema`，并以库里实际表为准（注册表只在「新建时」写，
//!    直接读注册表会漏掉旧版本建的、上一次清理漏掉的表）。

use shared::error::{CommonError, CoreError};

use crate::duckdb::temp_table::{TempTableManager, TempTableSource};
use crate::services::duckdb_service::DuckDbService;

use super::channel::AnalysisChannel;
use super::kernel::{self, PreparedMounts, SessionHandle};
use super::mount::plan_for;

/// 列出当前可做二次分析的结果集表（`tmp_q_` 前缀，按名排序）。
///
/// 返回空列表是**正常态**（还没跑过查询），不是错误；真正的错误只有「连不上内存库」。
pub fn list_result_sets() -> Result<Vec<String>, CoreError> {
    let conn = DuckDbService::get_or_create_duckdb()?;
    let guard = conn
        .lock()
        .map_err(|e| invalid(format!("DuckDB 单例锁中毒: {e}")))?;
    TempTableManager::list_by_source(&guard, TempTableSource::Query)
}

/// 结果集二次分析的会话：进程单例句柄（在它上面直接读 `tmp_q_` 表）。
///
/// `label` 只用于计划的展示名（历史面板 / 日志），不参与执行。
pub fn open_for_result_set(label: &str) -> Result<SessionHandle, CoreError> {
    let plan = plan_for(&AnalysisChannel::ProcessDuckDb, &[], None, label)?;
    kernel::open(&plan, &PreparedMounts::none())
}

fn invalid(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

#[cfg(test)]
mod tests {
    use super::{list_result_sets, open_for_result_set};

    #[test]
    fn result_set_channel_is_the_process_singleton() {
        let handle = open_for_result_set("结果集").expect("拿句柄");
        // 这条断言是口径本身：结果集不在加速/联邦会话里，就在进程单例上
        assert_eq!(handle.channel_label(), "process");
        assert!(handle.has_session());
    }

    #[test]
    fn listing_never_panics_without_any_result_set() {
        // 空列表是正常态（还没跑过查询）；这里只钉「不报错、不 panic」
        assert!(list_result_sets().is_ok());
    }

    #[test]
    fn a_query_temp_table_shows_up_in_the_list() {
        let handle = open_for_result_set("结果集").expect("拿句柄");
        let table = crate::duckdb::generate_unique_name(crate::duckdb::TempTableSource::Query, "rs");
        handle
            .run(&format!("CREATE TABLE {table} AS SELECT 1 AS a"))
            .expect("建结果集表");
        let listed = list_result_sets().expect("列表");
        assert!(listed.contains(&table), "列表应含 {table}，实际 {listed:?}");
        // 收尾：别把表留给别的用例（单例是进程级的）
        handle
            .run(&format!("DROP TABLE IF EXISTS {table}"))
            .expect("删表");
    }
}
