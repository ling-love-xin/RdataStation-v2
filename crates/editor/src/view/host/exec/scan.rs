//! scan 域（795-830 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。



use super::*;

impl EditorHostPanel {
    /// 启动结果轮询（已有存活任务时不重复启动）
    ///
    /// 与 `workbench` 的导航任务同一模式：**后台等、主线程回填**，空闲即退出。
    /// 执行期间顺带 `cx.notify()`：状态栏的耗时累加跟着这个节拍走（不另起一个定时器）。
    /// 【B18】提交一次后台扫描（**每次文本变更**一次；打开文档与重新加载各一次）
    ///
    /// 修订号只升不降（回绕也不怕：比较的是与当前值相等）；范围由档位的降级表回答
    /// （`EditorShared::scan_scope`），不在这里另写判据。
    pub(crate) fn submit_scan(&mut self, text: String, cx: &mut Context<Self>) {
        let scope = self.shared.scan_scope(&self.document);
        let channel = self
            .with_document(|doc| doc.channel())
            .unwrap_or_else(Default::default);
        // 【#54 B】语义诊断目录快照 + 方言：宿主按文档注入过就给（随快照进扫描线程），
        // 没注入 = 空目录 + Ansi（语义段跳过，词法诊断照常）。按文档取：多文档各绑
        // 各连接，扫描不跨文档串目录。
        let catalog = self
            .shared
            .catalog_snapshot_for(&self.document)
            .unwrap_or_else(|| std::sync::Arc::new(crate::completion::Catalog::default()));
        let dialect = self.sql_dialect_for_document();
        self.scan_revision = self.scan_revision.wrapping_add(1);
        self.scanner
            .submit(self.scan_revision, text, channel, scope, catalog, dialect);
        self.scan_pending = true;
        // 扫描不是“执行”，但它也要人按节拍把回执取回来——共用同一条轮询泵
        self.ensure_exec_pump(cx);
    }

    /// 【#54】文档绑定连接的 SQL 方言（未绑定 / 驱动未知 → Ansi 兜底，解析失败返回
    /// None 安全）。hover / 跳转 / 语义诊断共用这一个口径。
    pub(crate) fn sql_dialect_for_document(&self) -> engine::sql::SqlDialect {
        self.with_document(|doc| doc.connection().map(str::to_string))
            .flatten()
            .and_then(|conn_id| {
                self.shared
                    .connection_options()
                    .iter()
                    .find(|option| option.id == conn_id)
                    .and_then(|option| crate::connection::sql_dialect(&option.db_type))
            })
            .unwrap_or(engine::sql::SqlDialect::Ansi)
    }

}
