//! 编辑器主机面板 —— 面板操作自由函数（results 辅助 / save·close 流程）
//!
//! 自 host/mod.rs 拆分（2026-10-29 Step 5）：`impl EditorHostPanel` 尾部与 trait 实现按职责迁出，mod.rs 收敛为核心状态与调度。
//!
use super::*;

pub(crate) fn group_of_first(groups: &[crate::toolbar::ToolbarGroup]) -> crate::toolbar::ToolbarItemId {
    groups.first().map(|g| g.head).unwrap_or(crate::toolbar::ToolbarItemId::ExecRun)
}

/// 【T2c-5】最后一个组的组头（组移动↑↓ 的右边缘判定）
pub(crate) fn group_of_last(groups: &[crate::toolbar::ToolbarGroup]) -> crate::toolbar::ToolbarItemId {
    groups.last().map(|g| g.head).unwrap_or(crate::toolbar::ToolbarItemId::ExecRun)
}

/// 执行结论 → 结果记录（视图模型转换，不在这做任何 I/O）
/// 【T2a-2】结果集的行标识探测：从结果 SQL 猜表名 → 宿主导航缓存查主键列（内存读）
///
/// 返回 `None` 的四种情况（都如实置只读，不编造行标识）：
/// - 失败结果 / 写语句（没有可编辑的行）——不探测，省一次查询；
/// - 未绑定连接（`entry.connection` 为空）——不知道去哪张表；
/// - 宿主没接补全端口 / 端口不支持探测——如实没有行标识；
/// - 缓存没这张表 / 表没有主键——探测结果为空，同样给 `None`。
pub(crate) fn probe_primary_key(panel: &EditorHostPanel, entry: &ResultEntry) -> Option<Vec<String>> {
    if entry.error.is_some() || entry.columns.is_empty() {
        return None;
    }
    let Some(connection) = entry.connection.as_deref() else {
        return None;
    };
    let table = export::default_table_name(Some(&entry.sql), 0);
    panel
        .shared
        .primary_key_for(Some(connection), entry.channel, &table)
        .filter(|columns| !columns.is_empty())
}

/// 【P1】结果集字段注释：血缘（sqlglot 投影映射）+ 表列元数据（宿主导航缓存，内存读）
///
/// 与 [`probe_primary_key`] 同口径：失败结果 / 写语句 / 未绑定连接不探测；
/// 宿主没接注释端口 = 全 `None`（渲染只画列名）。长度与 `entry.columns` 一致。
pub(crate) fn annotate_columns(panel: &EditorHostPanel, entry: &ResultEntry) -> Vec<Option<ColumnAnnotation>> {
    if entry.error.is_some() || entry.columns.is_empty() {
        return vec![];
    }
    let Some(connection) = entry.connection.as_deref() else {
        return vec![None; entry.columns.len()];
    };
    panel
        .shared
        .column_annotations_for(Some(connection), entry.channel, &entry.sql, entry.columns.len())
}

/// 执行结论 → 结果条目；同时回执事实帧候选（方案跳 1：无 batches 回执为 `None`）。
///
/// 事实档不挂进 [`ResultEntry`]（避免连带进它的 `Clone` / 等价比较与深拷贝），由入槽点
/// 单独传给 `ResultStore::push` 落到平行事实槽位。
pub(crate) fn entry_from(
    outcome: execution::ExecOutcome,
) -> (ResultEntry, Option<std::sync::Arc<::shared::FactFrame>>) {
    let connection = outcome.connection.clone();
    // 【B13】通道也随结论回来：标签星徽标与“切通道后标灰”靠它
    let channel = outcome.channel;
    // 【B15】血缘：失败也照样带（“这句话是下发筛选发出去的”对排查同样有用）
    let lineage = outcome.lineage;
    // 【A2】形成原料（下发筛选 / 排序下发）：刷新 ⟳ 按它重查，保持形态
    let pushdown = outcome.pushdown;
    match outcome.result {
        Ok(data) => {
            let fact = data.fact;
            let entry = ResultEntry::success(
                outcome.document,
                outcome.sql,
                data.elapsed_ms,
                data.truncated,
                data.columns,
                data.rows,
            )
            .with_affected_rows(data.affected_rows)
            .with_has_more(data.has_more)
            // 【Q6】列类型随结果一起进结果集（空 = 驱动报不出 → 网格走“无类型档”）
            .with_column_types(&data.column_types)
            .with_connection(connection)
            .with_channel(channel)
            .with_analysis(outcome.analysis)
            .with_lineage(lineage)
            .with_pushdown(pushdown);
            (entry, fact)
        }
        Err(error) => {
            let entry = ResultEntry::failure(outcome.document, outcome.sql, error, 0)
                .with_connection(connection)
                .with_channel(channel)
                .with_lineage(lineage)
                .with_pushdown(pushdown);
            (entry, None)
        }
    }
}

/// 网格空态文案（没有网格的结果：失败原因 / 写语句都不留空白面板）
///
/// 写语句的影响行数已经在工具栏报了（`affected 3 行 · 35 ms`），网格这再说一遍
/// 只是重复，所以这里只回答“网格为什么是空的”。
pub(crate) fn empty_text(entry: &ResultEntry) -> String {
    if entry.affects_rows_only() {
        "写语句没有结果集".to_string()
    } else {
        entry.summary()
    }
}

/// 把**一批**打开文档的会话全部落库（E3：窗口退出兜底）
///
/// 与单份的 [`EditorHostPanel::save_session_now`] 同一口径（未命名 / 空文档跳过；
/// 失败只打日志不弹窗）——差别只在「一次存一整批」：窗口关闭时**没有第二次机会**。
///
/// 为什么放在编辑器侧：这是编辑器语义（哪些文档值得存、空文档怎么算），
/// 宿主只负责「在窗口真的关之前叫我一声」。
pub fn save_sessions_for(panels: &[Entity<EditorHostPanel>], cx: &mut App) {
    for panel in panels {
        panel.update(cx, |panel, cx| panel.save_session_now(cx));
    }
}

/// 关闭一份文档的面板（**宿主调用**，比如 `Ctrl+W`）
///
/// 为什么是宿主的活：[`Panel::on_removed`] 会回读面板（可见性 / 可关闭性），
/// 而动作处理器本身就在面板的 `update` 里——从那里发起关闭是重入，GPUI 会直接 panic。
/// 宿主不在面板的 `update` 中，所以只有它能安全地让容器移除面板。
///
/// **脏文档在这里被拦下**（返回 `false`，并在面板状态栏留原因）：Dock 没有“关闭前否决”
/// 钩子（架构 §12 #18），这个判据就是安全网。要真正关掉脏文档，调用方必须先走三态确认
/// （[`dialogs::open_close_confirm`]）再调 [`close_document_now`]。
///
/// 文档本身由 [`EditorHostPanel::on_removed`] 从 `EditorService` 里关闭——
/// “一个面板 = 一份文档”的对应关系只有一处。
pub fn close_document_in_dock(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    if panel.read(cx).is_dirty() {
        panel.update(cx, |panel, cx| panel.refuse_close_when_dirty(cx));
        return false;
    }
    close_document_now(area, panel, window, cx)
}

/// 直接关闭（**不做脏检查**）：调用方已就未保存改动拿到用户选择
///
/// 只应有两条调用路径：文档本来就干净（走 [`close_document_in_dock`]）、
/// 用户在关闭确认里选了“不保存”（走 [`request_close_document`]）。
pub fn close_document_now(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    area.update(cx, |area, cx| area.remove_panel(panel, window, cx));
    true
}

/// 关闭请求的完整流程（宿主键位 / 标签关闭都走它）
///
/// 干净 → 直接关；脏 → 三态确认（保存 / 不保存 / 取消）。流程放在这里而不是宿主，
/// 是因为它是**编辑器语义**（写盘、二次确认、失败分支）——宿主只负责“把面板从 Dock 上摘掉”
/// 与“弹系统文件对话框”（路径选择端口注入）。
pub fn request_close_document(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    if !panel.read(cx).is_dirty() {
        close_document_in_dock(area, panel, window, cx);
        return;
    }

    let title = panel.read(cx).title_text();
    let dialog_panel = panel.clone();
    let dialog_area = area.clone();
    dialogs::open_close_confirm(window, cx, title, move |choice, window, cx| {
        resolve_close_choice(&dialog_area, dialog_panel.clone(), choice, window, cx);
    });
}

/// 用户在“关闭前确认”上的选择落地（对话框回调与测试都走这里）
///
/// 与模式切换的 `confirm_mode_switch` 同一口径：弹窗只负责收集选择，
/// 落地只有一个入口——否则“对话框里点了保存”与“测试里走保存分支”会变成两段代码。
pub fn resolve_close_choice(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    choice: dialogs::CloseChoice,
    window: &mut Window,
    cx: &mut App,
) {
    match choice {
        // 取消：什么都不做（小改动常来自误触关闭，不该顺手丢掉）
        dialogs::CloseChoice::Cancel => {}
        dialogs::CloseChoice::Discard => {
            close_document_now(area, panel, window, cx);
        }
        dialogs::CloseChoice::Save => {
            save_then_close(area.clone(), panel, window, cx);
        }
    }
}

/// 保存当前文档（`Ctrl+Shift+S`，**不关文档**）：选路径 → 写盘；返回是否真写盘
///
/// 失败只落到状态栏（这里没在关闭流程上，不该用弹框拦住用户的手）。
pub fn request_save_as(panel: &Entity<EditorHostPanel>, cx: &mut App) -> bool {
    match pick_and_save_as(panel, cx) {
        Ok(()) => {
            panel.update(cx, |panel, cx| panel.clear_message(cx));
            true
        }
        Err(SaveAsFailure::Cancelled) => {
            // 用户取消：不是错误，但也不留上一次的提示
            panel.update(cx, |panel, cx| panel.clear_message(cx));
            false
        }
        Err(failure) => {
            let message = failure.message();
            panel.update(cx, |panel, cx| panel.set_message(Some(message), cx));
            false
        }
    }
}

/// 另存为的四种结局（调用方据此决定“关不关 / 弹不弹二次确认”）
///
/// “取消”单独成一类而不是 `None`：**用户取消不是失败**，不该弹二次确认、也不该报“未接入”。
pub(crate) enum SaveAsFailure {
    /// 宿主没接系统文件对话框（端口未注入）
    NoPicker,
    /// 用户取消
    Cancelled,
    /// 写盘失败（占用 / 权限 / 磁盘）
    Io(persist::PersistError),
    /// 文档已关闭（面板可能多活一帧）
    Gone,
}

impl SaveAsFailure {
    /// 状态栏文案（取消不产生文案，由调用方单独处理）
    fn message(&self) -> String {
        match self {
            Self::NoPicker => "未接入系统文件对话框，无法另存为".to_string(),
            Self::Cancelled => String::new(),
            Self::Io(error) => format!("另存为失败：{error}"),
            Self::Gone => "文档已关闭".to_string(),
        }
    }
}

impl From<persist::PersistError> for SaveAsFailure {
    fn from(error: persist::PersistError) -> Self {
        Self::Io(error)
    }
}

/// 另存为的“选路径 + 写盘”核心（不含失败后的交互）
///
/// 路径选择走宿主注入的端口（[`EditorShared::pick_save_path`]）：编辑器**不依赖** `rfd`。
pub(crate) fn pick_and_save_as(panel: &Entity<EditorHostPanel>, cx: &mut App) -> Result<(), SaveAsFailure> {
    let (current, mode) = {
        let read = panel.read(cx);
        (read.document_path(), read.document_mode())
    };
    let Some(mode) = mode else {
        return Err(SaveAsFailure::Gone);
    };
    if !panel.read(cx).shared.has_save_path_picker() {
        return Err(SaveAsFailure::NoPicker);
    }

    let default_name = mode.default_file_name().to_string();
    let picked = panel.read(cx).shared.pick_save_path(current, default_name);
    let Some(path) = picked else {
        return Err(SaveAsFailure::Cancelled);
    };

    panel
        .update(cx, |panel, cx| panel.save_as(&path, cx))
        .map(|_| ())?;
    Ok(())
}

/// 保存后关闭（关闭三态的“保存”分支）
///
/// 三种结果分别处理：写盘成功 → 关；未命名 → 先另存为（用户取消就不关）；
/// 写盘失败（占用 / 权限 / 磁盘）→ 二次确认（重试 / 另存为 / 取消），
/// **绝不把脏文档当干净关掉**。
pub(crate) fn save_then_close(
    area: Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    match panel.update(cx, |panel, cx| panel.save(cx)) {
        Ok(_) => {
            close_document_now(&area, panel, window, cx);
        }
        Err(persist::PersistError::Untitled(_)) => {
            // 未命名：没有路径可写，先选一个（取消就不关）
            pick_save_as_then_close(area, panel, window, cx);
        }
        Err(error) => {
            let title = panel.read(cx).title_text();
            let reason = error.to_string();
            open_save_failure(&area, &panel, title, reason, window, cx);
        }
    }
}

/// 另存为后关闭（“保存”分支里未命名文档 / 用户选“另存为”的路径）
pub(crate) fn pick_save_as_then_close(
    area: Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    match pick_and_save_as(&panel, cx) {
        Ok(()) => {
            close_document_now(&area, panel, window, cx);
        }
        Err(SaveAsFailure::Cancelled) => {}
        Err(SaveAsFailure::Io(error)) => {
            let title = panel.read(cx).title_text();
            let reason = error.to_string();
            open_save_failure(&area, &panel, title, reason, window, cx);
        }
        Err(failure) => {
            let message = failure.message();
            panel.update(cx, |panel, cx| panel.set_message(Some(message), cx));
        }
    }
}

/// 写盘失败后的二次确认（重试 / 另存为 / 取消）
///
/// 三条路都保留：文件被占用 / 短暂无权限时“重试”最省事；目标路径本身不行时“另存为”能绕开；
/// “取消”后文档仍是脏的（真实状态不粉饰）。
pub(crate) fn open_save_failure(
    area: &Entity<DockArea>,
    panel: &Entity<EditorHostPanel>,
    title: String,
    reason: String,
    window: &mut Window,
    cx: &mut App,
) {
    let retry_area = area.clone();
    let retry_panel = panel.clone();
    let as_area = area.clone();
    let as_panel = panel.clone();
    let cancel_panel = panel.clone();
    dialogs::open_save_failure_confirm(window, cx, title, reason, move |choice, window, cx| {
        match choice {
            dialogs::SaveFailureChoice::Cancel => {
                // 取消保存：文档仍是脏的（未保存状态是真实状态，不该粉饰）
                cancel_panel.update(cx, |panel, cx| panel.clear_message(cx));
            }
            dialogs::SaveFailureChoice::Retry => {
                save_then_close(retry_area.clone(), retry_panel.clone(), window, cx);
            }
            dialogs::SaveFailureChoice::SaveAs => {
                pick_save_as_then_close(as_area.clone(), as_panel.clone(), window, cx);
            }
        }
    });
}

/// 语句数：走 `engine::sql::split` 的**词法级**切分（不是 `split(';')`）
///
/// 只在内容变化时调一次（构造 / 输入回写 / 重新加载），渲染期只读缓存值。
pub(crate) fn count_statements(text: &str) -> usize {
    engine::sql::split_statements(text).len()
}

/// 折叠按能力与档位装 / 关（**唯一开关处**：新建、切模式、整篇换文本都走它）
///
/// 真值在 [`EditorShared::folding_enabled`]（能力表 + 大文件档位）；这里只把那个判断落到内核上，不另写一套。
/// 装不上就**真关开关**（`set_folding(false)`）：大文件档位下折叠每键要重扫全文，只留一个判据
/// 而不关开关就是把那份代价白付了。文本取自**内核当下值**（不另传参数），两处调用点因此不会
/// 把“服务层的文本”和“屏幕上的文本”搞混。
pub(crate) fn sync_fold(
    shared: &EditorShared,
    document: &DocumentId,
    state: &mut EditorState,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) {
    if shared.folding_enabled(document) {
        let text = state.value().to_string();
        let indent_folding = shared.indent_folding();
        let open_keywords = shared.fold_open_keywords();
        let close_keywords = shared.fold_close_keywords();
        fold::install(state, &text, indent_folding, &open_keywords, &close_keywords, cx);
    } else {
        state.set_folding(false, window, cx);
    }
}
