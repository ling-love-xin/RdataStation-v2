//! 草稿箱冲突 Diff 与文件属性：**落中央编辑区**的面板（视图类型 + 渲染）。
//!
//! 与侧栏的分工（架构决策 D7）：草稿箱只投载荷（`ScratchpadHost::show_diff` /
//! `Shared::scratchpad_file_info`），重结果由编辑区渲染。
//! 内容搜索面板（按文件分组 + 替换栏）已随第二阶段迁入独立 crate
//! [`content_search::view`]（用户点名 2026-10-10 抽 crate，命令面板可复用）；
//! 草稿箱只负责把后端结果组装成 [`content_search::view::SearchView`]（见 `search_view_from_payload`）。

use super::*;

use super::primitives::scratchpad_absolute_time;
use content_search::view::{SearchHit, SearchView};

/// 冲突 Diff 面板的载荷（草稿箱经宿主端口投给中央编辑区）。
///
/// 「左 = 磁盘，右 = 编辑器里的未保存内容」：左侧是“外面变成了什样”，
/// 右侧是“我手上是什么”，两侧都标行号。
pub struct ScratchpadDiffView {
    /// 模块内相对路径（标题用）。
    pub relative_path: String,
    /// 行级差异（`diff_with_content` 的结果）。
    pub diff: DiffResult,
}

/// 文件属性面板的载荷（跨 crate：workbench 宿主 overlay 弹窗渲染）。
///
/// 用户点名（2026-10-10）：树行 meta 只留大小，时间挪进属性面板；后期 SQL 文件
/// 还要知道「连接哪个数据库」，`db_connection` 即为该能力预留（分析文件类型实现后填充）。
#[derive(Clone)]
pub struct ScratchpadFileInfoView {
    /// 模块内相对路径。
    pub relative_path: String,
    /// 文件名（展示主文本）。
    pub name: String,
    /// 类型标签（文件 / 文件夹）。
    pub kind_label: String,
    /// 扩展名（无扩展名空串；文件夹为空串）。
    pub extension: String,
    /// 可读大小（文件夹为空串）。
    pub size_label: String,
    /// 修改时间（RFC3339；面板内转相对时间）。
    pub modified_at: Option<String>,
    /// 创建时间（RFC3339；面板内转相对时间）。
    pub created_at: Option<String>,
    /// SQL 文件的数据库连接描述（后期分析文件类型实现后填充；现在 `None`）。
    pub db_connection: Option<String>,
}

/// 把后台搜索任务的载荷转成结果视图（组装 `content_search::view::SearchView`）。
///
/// 搜索/替换任务的开关与查询词由任务回传，避免再从侧栏输入框反向读状态。
pub(super) fn search_view_from_payload(
    query: String,
    is_regex: bool,
    case_sensitive: bool,
    payload: scratchpad_jobs::SearchPayload,
) -> SearchView {
    SearchView {
        query,
        is_regex,
        case_sensitive,
        scanned: payload.scanned,
        truncated: payload.truncated,
        hits: payload
            .matches
            .into_iter()
            .map(|m| SearchHit {
                file: m.file,
                line: m.line_number,
                content: m.line_content,
                spans: m.match_spans,
                before: m.before_context,
                after: m.after_context,
            })
            .collect(),
    }
}


/// 冲突 Diff 面板（中央编辑区）：左=磁盘 / 右=编辑器缓冲，逐行标行号与增删。
///
/// 与搜索结果面板同一投影：草稿箱只投载荷（`ScratchpadHost::show_diff`），渲染在编辑区。
/// 消解冲突的两个动作（照磁盘重载 / 忽略）留在左侧草稿箱的冲突条上——它们要改编辑器
/// 与草稿箱自己的状态，面板只负责把差异看清楚。
pub fn render_scratchpad_diff_pane(
    view: &ScratchpadDiffView,
    theme: &gpui_kit::component::Theme,
    on_clear: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
) -> Div {
    let fg = theme.colors.foreground;
    let muted = theme.colors.muted_foreground;
    let border = theme.colors.border;
    let bg = theme.colors.background;
    let danger = theme.colors.danger;
    let success = theme.colors.success;

    let header = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(fg)
                .child(format!(
                    "冲突（{}）· {} → {}",
                    view.relative_path, view.diff.left_label, view.diff.right_label
                )),
        )
        .child(
            Button::new("sp-diff-close")
                .small()
                .ghost()
                .label("关闭")
                .on_click(on_clear),
        );

    let mut list = div().v_flex().w_full().gap_0p5();
    for line in &view.diff.lines {
        let color = match line.kind {
            DiffLineKind::Added => success,
            DiffLineKind::Removed => danger,
            DiffLineKind::Unchanged => muted,
        };
        let left = line
            .line_number_left
            .map(|n| n.to_string())
            .unwrap_or_else(|| "·".to_string());
        let right = line
            .line_number_right
            .map(|n| n.to_string())
            .unwrap_or_else(|| "·".to_string());
        list = list.child(
            div()
                .h_flex()
                .gap_2()
                .w_full()
                .text_xs()
                .text_color(color)
                .child(div().flex_none().child(format!(
                    "{} {:>5} {:>5}",
                    scratchpad_diff_marker(line.kind),
                    left,
                    right
                )))
                .child(div().flex_1().min_w_0().child(line.content.clone())),
        );
    }

    div()
        .v_flex()
        .w_full()
        .gap_1()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(bg)
        .child(header)
        .child(div().max_h(rems(16.)).overflow_hidden().child(list))
}

/// 文件属性键值行（右键 → 属性；宿主用 gpui-kit `Dialog` 包一层）。
///
/// 用户点名（2026-10-10）：树行 meta 只留大小，修改/创建时间与连接信息都在这里。
/// `db_connection` 为后期「SQL 文件连接哪个数据库」预留（分析文件类型实现后填充，
/// 现在是占位 `—`）。
pub fn render_scratchpad_file_info_pane(
    view: &ScratchpadFileInfoView,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let fg = theme.colors.foreground;
    let muted = theme.colors.muted_foreground;

    // RFC3339 → 绝对时间（对齐原型 705-706：修改时间 / 创建时间显示绝对时间戳；
    // 相对时间只用于树行——树行已只留大小，时间全在这里）；缺省显示 `—`。
    let time_label = |ts: &Option<String>| {
        ts.as_deref()
            .and_then(scratchpad_absolute_time)
            .unwrap_or_else(|| "—".to_string())
    };
    let db = view.db_connection.clone().unwrap_or_else(|| "—".to_string());

    let rows = [
        ("类型", view.kind_label.clone()),
        ("路径", view.relative_path.clone()),
        (
            "大小",
            if view.size_label.is_empty() {
                "—".to_string()
            } else {
                view.size_label.clone()
            },
        ),
        ("修改时间", time_label(&view.modified_at)),
        ("创建时间", time_label(&view.created_at)),
        ("数据库连接", db),
    ];

    let mut list = div().v_flex().w_full().gap_0p5();
    for (label, value) in rows {
        list = list.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .text_xs()
                .child(div().flex_none().w(rems(4.5)).text_color(muted).child(label))
                .child(div().flex_1().min_w_0().text_color(fg).child(value)),
        );
    }

    list
}

/// 差异行前缀（`similar` 的口径：`+` 新增 / `-` 删除 / 空格原样）。
pub(super) fn scratchpad_diff_marker(kind: DiffLineKind) -> &'static str {
    match kind {
        DiffLineKind::Added => "+",
        DiffLineKind::Removed => "-",
        DiffLineKind::Unchanged => " ",
    }
}

