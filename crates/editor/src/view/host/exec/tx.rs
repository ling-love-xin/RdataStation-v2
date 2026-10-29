//! tx 域（645-794 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。


use crate::execution::{self, ExecTarget};
use crate::store::ResultPlacement;
use crate::translate;

use super::*;

impl EditorHostPanel {
    /// 【B10】执行计划（原型 §5.1：`EXPLAIN` 当前语句 / 选区，**在当前通道上执行**）
    ///
    /// - 前缀**按方言生成**（SQLite 是 `EXPLAIN QUERY PLAN`；SQL Server / Oracle 要先改会话开关
    ///   或再查计划视图，如实说不支持）；
    /// - 方言取**当前通道**的：源库档用源库的，加速 / 联邦档用 DuckDB 的——那儿才是真要跑的引擎；
    /// - 结果落**新结果集**并贴「执行计划」标题（不抢用户正在看的那份）。
    pub(crate) fn explain_current(&mut self, cx: &mut Context<Self>) {
        if !self.execution_allowed() {
            self.set_message(Some("文本模式不与数据库通信".to_string()), cx);
            return;
        }
        let (text, selection) = self.editor_snapshot(cx);
        let sql = match execution::resolve_target(&text, selection) {
            ExecTarget::Selection(sql) | ExecTarget::Statement(sql) => sql,
            // `resolve_target` 只会给这三种；空文档 / 只有注释的文档没什么可解释的
            _ => {
                self.set_message(Some("没有可生成执行计划的语句".to_string()), cx);
                return;
            }
        };
        let dialect = if self.channel().runs_locally() {
            // 加速 / 联邦：语句（连带计划）都在 DuckDB 上跑，计划就该看 DuckDB 的
            engine::sql::SqlDialect::Duckdb
        } else {
            match self.source_dialect() {
                Ok(dialect) => dialect,
                Err(reason) => {
                    self.set_message(Some(reason), cx);
                    return;
                }
            }
        };
        let Some(explain) = engine::sql::explain_sql(dialect, &sql) else {
            self.set_message(
                Some(format!(
                    "{} 暂不支持生成执行计划（要先改会话开关 / 再查计划视图）",
                    translate::label_of(dialect)
                )),
                cx,
            );
            return;
        };
        if self.execute_labeled(
            ExecTarget::Statement(explain),
            ResultPlacement::NewSet,
            Some("执行计划".to_string()),
            cx,
        ) {
            self.set_message(Some("已提交执行计划（结果落新的结果集）".to_string()), cx);
        }
    }

    /// 【B4】请一个事务动作（开始 / 提交 / 回滚）：状态栏 TX 区的菜单调它
    pub(crate) fn tx_action(&mut self, action: execution::TxAction, cx: &mut Context<Self>) {
        match self
            .shared
            .request_transaction(self.document.clone(), action)
        {
            Ok(()) => {
                self.tx_pending += 1;
                self.set_message(Some(format!("{}…", action.label())), cx);
                self.ensure_exec_pump(cx);
            }
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 【B4】切换自动提交（下一次执行生效）
    pub(crate) fn toggle_autocommit(&mut self, cx: &mut Context<Self>) {
        self.autocommit = !self.autocommit;
        let text = if self.autocommit {
            "自动提交：开（每次执行自成一体）".to_string()
        } else {
            "自动提交：关（执行后停在事务里，待提交/回滚）".to_string()
        };
        self.set_message(Some(text), cx);
    }

    /// 【B4】用一次快照刷新 TX 区（时长从**首次看到**事务开始算）
    pub(crate) fn apply_tx_snapshot(&mut self, snapshot: execution::TxSnapshot) {
        self.tx_open = snapshot.in_transaction;
        match (snapshot.in_transaction, self.tx_since) {
            (true, None) => self.tx_since = Some(std::time::Instant::now()),
            (false, _) => self.tx_since = None,
            _ => {}
        }
    }

    /// TX 区文案（纯文本；按钮另画）：“TX 未开启” / “TX 已开启 3.4s”
    pub(crate) fn tx_text(&self) -> String {
        match (self.tx_open, self.tx_since) {
            (true, Some(since)) => {
                format!("TX 已开启 {}", result_panel::status::elapsed_text(since.elapsed()))
            }
            (true, None) => "TX 已开启".to_string(),
            (false, _) => "TX 未开启".to_string(),
        }
    }

    /// 中断当前执行（B3；原型 §5.1：入口在**状态栏 ■**）
    ///
    /// 同步只回绝“没在跑”；真正的中断在工作线程上做（端口实现允许阻塞）。
    /// 中断之后的剩余语句由通道标“已取消”（不是接着往下跑）。
    pub(crate) fn interrupt(&mut self, cx: &mut Context<Self>) {
        match self.shared.cancel_for(&self.document) {
            Ok(()) => self.set_message(Some("已请求中断…".to_string()), cx),
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 本文档现在能不能中断（状态栏据此决定要不要画■按钮）
    pub(crate) fn can_interrupt(&self) -> bool {
        self.pending > 0
    }

    /// 状态栏那行“执行中 3.4s…”读的耗时（供测试断言“跑完就不再计时”）
    pub fn elapsed_for_test(&self) -> Option<std::time::Duration> {
        self.running_since.map(|since| since.elapsed())
    }

    /// 【B4】TX 区文案（供测试断言）
    pub fn tx_text_for_test(&self) -> String {
        self.tx_text()
    }

    /// 【B4】自动提交开关的当前值
    pub fn autocommit_for_test(&self) -> bool {
        self.autocommit
    }

    /// 【B4】还没回执的事务动作数
    pub fn tx_pending_for_test(&self) -> usize {
        self.tx_pending
    }

    /// 当前模式是否允许执行（**读能力表**，不在视图里另写一份模式判断）
    ///
    /// 文本模式恒为 `false`（“不与数据库通信”是能力表里的硬约束）。
    pub(crate) fn execution_allowed(&self) -> bool {
        self.with_document(|doc| doc.capabilities().execute)
            .unwrap_or(false)
    }

    /// 本文档的文件档位提示（>50MB / ≥200MB；`None` = 不显示提示卡）
    pub fn tier_notice(&self) -> Option<&'static str> {
        self.with_document(|doc| doc.tier_notice()).flatten()
    }
}
