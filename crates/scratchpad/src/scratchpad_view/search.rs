//! 内容搜索 / 替换与冲突 Diff：**落中央编辑区**的两个面板（视图类型 + 渲染）。
//!
//! 与侧栏的分工（架构决策 D7）：草稿箱只投载荷（`ScratchpadHost::show_diff` /
//! `Shared::scratchpad_search`），重结果由编辑区渲染——240px 的侧栏装不下带上下文的命中列表。
//! 因此本模块的两个渲染函数是**跨 crate 公开面**（`pub`，由 workbench 的编辑器调用）。

use super::*;

/// 内容搜索命中项（视图模型）。
#[derive(Clone)]
pub(super) struct ScratchpadSearchHit {
    pub(super) file: String,
    pub(super) line: usize,
    pub(super) content: String,
    /// 行内命中区间（字节偏移，来自后端 `SearchMatch::match_spans`）。
    pub(super) spans: Vec<(usize, usize)>,
    pub(super) before: Vec<String>,
    pub(super) after: Vec<String>,
}

/// 内容搜索结果（落中央编辑区；侧栏发起写入，编辑区读取渲染）。
#[derive(Clone)]
pub struct ScratchpadSearchView {
    pub query: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub(super) scanned: usize,
    pub(super) truncated: bool,
    pub(super) hits: Vec<ScratchpadSearchHit>,
}

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

/// 把后台搜索任务的载荷转成结果视图。
///
/// 搜索/替换任务的开关与查询词由任务回传，避免再从侧栏输入框反向读状态。
pub(super) fn search_view_from_payload(
    query: String,
    is_regex: bool,
    case_sensitive: bool,
    payload: scratchpad_jobs::SearchPayload,
) -> ScratchpadSearchView {
    ScratchpadSearchView {
        query,
        is_regex,
        case_sensitive,
        scanned: payload.scanned,
        truncated: payload.truncated,
        hits: payload
            .matches
            .into_iter()
            .map(|m| ScratchpadSearchHit {
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

/// 内容搜索结果面板（中央编辑区）：头部（查询/命中数/开关标记）+ 命中列表（含上下文）。
pub fn render_scratchpad_search_pane(
    search: &ScratchpadSearchView,
    theme: &gpui_kit::component::Theme,
    match_bg: Hsla,
    replace_input: Option<&Entity<InputState>>,
    replace_filled: bool,
    // 点命中标题 → 打开该文件（原型 §4.3：点击命中跳转到文件）。参 = 模块内相对路径。
    on_open_hit: std::rc::Rc<dyn Fn(String, &mut gpui_kit::Window, &mut App)>,
    on_clear: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
    on_replace_all: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
) -> Div {
    let fg = theme.colors.foreground;
    let muted = theme.colors.muted_foreground;
    let border = theme.colors.border;
    let bg = theme.colors.background;
    let primary = theme.colors.primary;

    let flags = {
        let mut f = Vec::new();
        if search.is_regex {
            f.push("正则");
        }
        if search.case_sensitive {
            f.push("区分大小写");
        }
        if search.truncated {
            f.push("已截断");
        }
        if f.is_empty() {
            String::new()
        } else {
            format!(" · {}", f.join(" · "))
        }
    };

    let header = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(format!("内容搜索：{}", search.query)),
        )
        .child(div().text_xs().text_color(muted).child(format!(
            "{} 处匹配 · 扫描 {} 个文件{}",
            search.hits.len(),
            search.scanned,
            flags
        )))
        .child(div().flex_1())
        .child(
            div()
                .id("sp-search-close")
                .cursor_pointer()
                .text_xs()
                .text_color(primary)
                .child("关闭")
                .on_click(on_clear),
        );

    let mut list = div().v_flex().w_full().gap_1();
    if search.hits.is_empty() {
        list = list.child(div().text_xs().text_color(muted).child("无匹配"));
    }
    for hit in &search.hits {
        let open_hit = on_open_hit.clone();
        let hit_file = hit.file.clone();
        let mut group = div().v_flex().w_full().gap_0p5().child(
            div()
                .id(format!("sp-hit-{}-{}", hit.file, hit.line))
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .cursor_pointer()
                .hover(move |s| s.text_color(primary))
                .on_click(move |_, window, app| open_hit(hit_file.clone(), window, app))
                .child(format!("{} · 行 {}", hit.file, hit.line)),
        );
        for line in &hit.before {
            group = group.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("    {line}")),
            );
        }
        group = group.child(
            div()
                .h_flex()
                .items_center()
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .gap_0p5()
                .child(div().flex_none().text_xs().text_color(muted).child(">"))
                .child(scratchpad_hit_line(&hit.content, &hit.spans, match_bg, fg)),
        );
        for line in &hit.after {
            group = group.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("    {line}")),
            );
        }
        list = list.child(group);
    }

    // 替换栏（原型 §4.5）：预览计数 + 全部替换；无替换内容时按钮禁用。
    let unique_files = {
        let mut files: Vec<&str> = search.hits.iter().map(|h| h.file.as_str()).collect();
        files.sort_unstable();
        files.dedup();
        files.len()
    };
    let mut replace_row = div().h_flex().items_center().gap_2().w_full().child(
        div()
            .flex_none()
            .text_xs()
            .text_color(muted)
            .child("替换为"),
    );
    if let Some(input) = replace_input {
        replace_row = replace_row.child(div().flex_1().min_w_0().child(Input::new(input)));
    } else {
        replace_row = replace_row.child(div().flex_1());
    }
    replace_row = replace_row
        .child(div().flex_none().text_xs().text_color(muted).child(format!(
            "将替换 {} 处 · {} 个文件",
            search.hits.len(),
            unique_files
        )))
        .child(
            Button::new("sp-search-replace")
                .small()
                .label("全部替换")
                .disabled(!replace_filled)
                .on_click(on_replace_all),
        );

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
        .child(replace_row)
        .child(div().max_h(rems(16.)).overflow_hidden().child(list))
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

/// 差异行前缀（`similar` 的口径：`+` 新增 / `-` 删除 / 空格原样）。
pub(super) fn scratchpad_diff_marker(kind: DiffLineKind) -> &'static str {
    match kind {
        DiffLineKind::Added => "+",
        DiffLineKind::Removed => "-",
        DiffLineKind::Unchanged => " ",
    }
}

/// 命中高亮行：按字节区间把 `content` 切成「普通段 + 命中段」，命中段用命中底色。
///
/// 区间非法（越界 / 非 char 边界 / 重叠）时退化为整体纯文本。
pub(super) fn scratchpad_hit_line(
    content: &str,
    spans: &[(usize, usize)],
    match_bg: Hsla,
    fg: Hsla,
) -> Div {
    let plain = || {
        div()
            .min_w_0()
            .text_xs()
            .text_color(fg)
            .child(content.to_string())
    };
    if spans.is_empty() {
        return plain();
    }
    let mut row = div()
        .h_flex()
        .items_center()
        .min_w_0()
        .text_xs()
        .text_color(fg);
    let mut cursor = 0usize;
    for &(start, end) in spans {
        if start < cursor
            || end < start
            || end > content.len()
            || !content.is_char_boundary(start)
            || !content.is_char_boundary(end)
        {
            return plain();
        }
        if start > cursor {
            row = row.child(div().flex_none().child(content[cursor..start].to_string()));
        }
        row = row.child(
            div()
                .flex_none()
                .rounded_sm()
                .bg(match_bg)
                .child(content[start..end].to_string()),
        );
        cursor = end;
    }
    if cursor < content.len() {
        row = row.child(div().flex_none().child(content[cursor..].to_string()));
    }
    row
}
