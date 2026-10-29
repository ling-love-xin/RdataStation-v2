//! 【M8】把结果集的「洞察此列」接到洞察面板（端口注入）。
//!
//! 分工：编辑器只说「这一列 + 产生它的 SQL + 在哪条连接上」（[`crate::shared::InsightColumnRequest`]），
//! 翻译成 `SampleSource` 是宿主的事——editor crate **不依赖 insight**（两者都是特性 crate）。
//!
//! **不物化结果集**（决策在案，见 `insight-dev-plan.md` §10 #6）：洞察侧拿这段 SQL 重跑一句
//! 带 `LIMIT` 的取样（`SOURCE_SAMPLE_LIMIT`），所以：
//! - 不需要执行期建 `tmp_q_*` 表，也就没有「结果集被丢弃时回收临时表」这套生命周期；
//! - 列类型不在这里猜（编辑器手里只有字符串化的行）——取样落到 DuckDB 临时表后由洞察侧定类型，
//!   目标头在数据回来之前只显示来源，不显示类型。
//!
//! 入口只在**能取样**时出现（成功 + 有列 + 绑定连接 + SQL 是只读查询，见
//! `crate::store::ResultEntry::can_insight_column`），所以这里不必再做防御性判断。
//!
//! P2-a：经 [`crate::host::EditorHost::open_insight_source_column`] 转发
//! （`SampleSource` 翻译在 workbench 的宿主实现里），不再依赖 `Shared` 具体类型。

use std::rc::Rc;

use crate::host::EditorHost;
use crate::shared::EditorShared;

/// 注入「洞察此列」端口（`WorkbenchView::new` 构造期调用一次）
pub fn attach(editor: &EditorShared, host: Rc<dyn EditorHost>) {
    editor.attach_insight_column(Rc::new(move |request, cx| {
        host.open_insight_source_column(request, cx);
    }));
}
