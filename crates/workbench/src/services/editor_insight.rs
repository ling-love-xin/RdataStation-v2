//! 【M8】把结果集的「洞察此列」接到洞察面板（端口注入）。
//!
//! 分工：编辑器只说「这一列 + 产生它的 SQL + 在哪条连接上」（[`editor::shared::InsightColumnRequest`]），
//! 翻译成 `SampleSource` 是宿主的事——editor crate **不依赖 insight**（两者都是特性 crate）。
//!
//! **不物化结果集**（决策在案，见 `insight-dev-plan.md` §10 #6）：洞察侧拿这段 SQL 重跑一句
//! 带 `LIMIT` 的取样（`SOURCE_SAMPLE_LIMIT`），所以：
//! - 不需要执行期建 `tmp_q_*` 表，也就没有「结果集被丢弃时回收临时表」这套生命周期；
//! - 列类型不在这里猜（编辑器手里只有字符串化的行）——取样落到 DuckDB 临时表后由洞察侧定类型，
//!   目标头在数据回来之前只显示来源，不显示类型。
//!
//! 入口只在**能取样**时出现（成功 + 有列 + 绑定连接 + SQL 是只读查询，见
//! `editor::store::ResultEntry::can_insight_column`），所以这里不必再做防御性判断。

use std::rc::Rc;

use editor::shared::EditorShared;

use crate::panels::Shared;

/// 注入「洞察此列」端口（`WorkbenchView::new` 构造期调用一次）
pub fn attach(editor: &EditorShared, workbench: &Shared) {
    let workbench = workbench.clone();
    editor.attach_insight_column(Rc::new(move |request, cx| {
        let source = insight::SampleSource::new(
            request.connection.clone(),
            request.sql.clone(),
            request.label.clone(),
        );
        // 类型留空：编辑器手里没有真类型（行数据是字符串化的），取样后由洞察侧从临时表读出
        // 真类型——不编一个看起来像真的值（面板侧对空类型只显示来源）
        workbench.open_insight_source_column(source, request.column.clone(), String::new(), cx);
    }));
}
