//! execute 域（436-644 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。


use crate::commands::ExecuteAll;
use crate::execution::{self, ExecTarget};
use crate::store::ResultPlacement;

use super::*;

impl EditorHostPanel {

    pub(crate) fn execute_preferring_selection(&mut self, cx: &mut Context<Self>) {
        let (text, selection) = self.editor_snapshot(cx);
        let target = execution::resolve_target(&text, selection);
        // 2026-10-27：主执行动作（快捷键）也是「每次执行 = 新结果 tab」——
        // 与工具栏执行按钮同口径（用户要求多次执行不挤在同一 tab）。
        self.execute(target, ResultPlacement::NewSet, cx);
    }

    /// `Ctrl+Shift+Enter`：执行全部
    pub(crate) fn on_execute_all(
        &mut self,
        _: &ExecuteAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_all(cx);
    }

    /// 执行整篇（**宿主发起的唯一入口**：导航「查看数据」打开文档后自动跑一次，B11）
    ///
    /// 与 `Ctrl+Shift+Enter` 同一条路：目标解析 → 提交 → 轮询回填。
    pub fn run_all(&mut self, cx: &mut Context<Self>) {
        let (text, _selection) = self.editor_snapshot(cx);
        let target = execution::all_target(&text);
        // 2026-10-27：执行全部同样是「每次执行 = 新结果 tab」（同主执行口径）。
        self.execute(target, ResultPlacement::NewSet, cx);
    }

    /// 提交一次执行
    ///
    /// `placement` 决定结果落到当前结果集还是新结果集（B2 的「在新结果标签中执行」/「批量执行」）；
    /// 自动提交关闭时（B4）本次执行先开一个事务。
    /// 拒绝都要留痕迹：文本模式（能力表禁止通信）、未接入执行、忙、空目标。
    ///
    /// 返回**是否真的提交了**：导出那条路要先记在途状态、再提交取段，
    /// 提交没成（忙 / 未接入）就得把在途状态撒回去（否则状态会残留到下一次）。
    pub(crate) fn execute(
        &mut self,
        target: ExecTarget,
        placement: ResultPlacement,
        cx: &mut Context<Self>,
    ) -> bool {
        self.execute_labeled(target, placement, None, cx)
    }

    /// 同 [`Self::execute`]，但给这一批回填的结果贴一个**自定义标题**（今天只有执行计划用）
    ///
    /// 标题是**按位**记的（`pending_labels`）：回填是一条语句一个结论，
    /// “这份是计划、那份是数据”不能靠一个布尔量猜。
    pub(crate) fn execute_labeled(
        &mut self,
        target: ExecTarget,
        placement: ResultPlacement,
        title: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let mode = self.document_mode();
        let allowed = self.execution_allowed();
        let channel = self.channel();
        let conn = self.bound_connection();
        let stmt_count = target.statements().len();
        let sql_preview = target
            .statements()
            .first()
            .map(|sql| sql.chars().take(80).collect::<String>())
            .unwrap_or_default();
        tracing::info!(
            target = "rds_editor::exec",
            doc = %self.document,
            mode = ?mode,
            execute_allowed = allowed,
            channel = ?channel,
            connection = ?conn,
            statements = stmt_count,
            kind = target.label(),
            sql = sql_preview,
            "提交执行：进入执行主入口"
        );
        if !allowed {
            tracing::warn!(
                target = "rds_editor::exec",
                doc = %self.document,
                mode = ?mode,
                channel = ?channel,
                connection = ?conn,
                kind = target.label(),
                "执行被拒：当前文档模式不与数据库通信（通用/分析模式无执行能力）"
            );
            self.set_message(Some("文本模式不与数据库通信".to_string()), cx);
            return false;
        }
        // 【B13】通道门控与写拒绝都在**提交之前**：注定被拒的语句不该跑一半（也不该让
        // “本地副本只读”这种事等驱动报一个谁也不懂的错）
        if let Err(reason) = self
            .channel_ready()
            .and_then(|()| self.channel_write_check(&target))
        {
            tracing::warn!(
                target = "rds_editor::exec",
                doc = %self.document,
                channel = ?channel,
                connection = ?conn,
                kind = target.label(),
                reason = %reason,
                "执行被拒：通道不可用或写语句被通道闸拦下"
            );
            self.set_message(Some(reason), cx);
            return false;
        }
        // 项目只读（标题栏的锁）：写语句同样在**提交之前**拒（旧 SQL 面板有这道闸，B12 遗失）
        if let Err(reason) = self.project_write_check(&target) {
            tracing::warn!(
                target = "rds_editor::exec",
                doc = %self.document,
                channel = ?channel,
                connection = ?conn,
                kind = target.label(),
                reason = %reason,
                "执行被拒：项目只读，写语句被项目闸拦下"
            );
            self.set_message(Some(reason), cx);
            return false;
        }
        // 【B16】参数绑定（DBeaver 式）：目标里有 `:name` → 挂起等表单，填完再替换执行。
        // 只扫**用户写的 SQL** 变体（`statements()` 为空的目标 = 内部生成，没有参数）。
        // 参数名按首次出现顺序去重（表单就按这个顺序摆）
        let names = {
            let mut seen: Vec<String> = Vec::new();
            for sql in target.statements() {
                for name in engine::sql::param_names(&sql) {
                    if !seen.contains(&name) {
                        seen.push(name);
                    }
                }
            }
            seen
        };
        if !names.is_empty() {
            tracing::info!(
                target = "rds_editor::exec",
                doc = %self.document,
                channel = ?channel,
                connection = ?conn,
                params = ?names,
                "执行挂起：参数表单待填，填完后再入队"
            );
            self.pending_bind = Some(PendingBind {
                target,
                placement,
                title,
            });
            self.open_bind_dialog(names, cx);
            return false;
        }
        // 预期回填条数 = 本次要跑的语句数（批量 > 1）：跑完这几条才算“不执行中”
        let expected = target.statements().len();
        // B4：自动提交关掉时，本次执行进事务（引擎在没有事务时会先自动开一个）；
        // B13：加速 / 联邦没有事务语义，那两档上不走事务
        let options = execution::RunOptions {
            use_transaction: !self.autocommit && channel.allows_transactions(),
        };
        match self
            .shared
            .submit(self.document.clone(), &target, placement, options)
        {
            Ok(()) => {
                tracing::info!(
                    target = "rds_editor::exec",
                    doc = %self.document,
                    channel = ?channel,
                    connection = ?conn,
                    statements = expected,
                    use_transaction = options.use_transaction,
                    kind = target.label(),
                    "执行已入队后台线程"
                );
                if self.pending == 0 {
                    // 批量期间一直计同一轮的时间：后一句不该把计时清零
                    self.running_since = Some(std::time::Instant::now());
                }
                self.pending += expected;
                self.pending_labels
                    .extend((0..expected).map(|_| title.clone()));
                self.set_message(None, cx);
                self.ensure_exec_pump(cx);
                true
            }
            Err(error) => {
                tracing::warn!(
                    target = "rds_editor::exec",
                    doc = %self.document,
                    channel = ?channel,
                    connection = ?conn,
                    kind = target.label(),
                    reason = %error.message(),
                    "执行提交被队列拒绝"
                );
                self.set_message(Some(error.message().to_string()), cx);
                false
            }
        }
    }
}
