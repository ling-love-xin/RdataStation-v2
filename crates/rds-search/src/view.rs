//! 搜索面板渲染（VS Code 形态）：按文件分组 + 命中数角标 + 行号命中行 + 替换栏。
//!
//! 第二阶段迁入（用户点名 2026-10-10）：`type_icon` 由调用方注入（scratchpad /
//! 命令面板各自提供文件类型图标），与任何宿主视图完全解耦。
//! 视图模型由调用方从后端 [`crate::SearchResult`] 组装（见 `search_match_to_view` 模式）。

use gpui_kit::base::StyledExt as _;
use gpui_kit::*;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _, Theme};
use gpui_kit::component::scroll::ScrollableElement as _;
use workbench_shell::ui::ICON_SIZE_SM;

/// 搜索命中项（视图模型）。
#[derive(Clone)]
pub struct SearchHit {
    /// 模块内相对路径（调用方拼成绝对路径）。
    pub file: String,
    /// 1 基行号。
    pub line: usize,
    /// 命中行内容。
    pub content: String,
    /// 行内命中区间（字节偏移，来自后端 `SearchMatch::match_spans`）。
    pub spans: Vec<(usize, usize)>,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// 点命中后的跳转目标（跨 crate：调用方消费后打开文件并落光标/选区）。
///
/// `file` 是模块内相对路径；`line` 是 1 基行号；`span` 是行内命中字节区间（首个命中）。
#[derive(Debug, Clone)]
pub struct SearchHitTarget {
    pub file: String,
    pub line: u32,
    pub span: Option<(usize, usize)>,
}

/// 搜索结果显示（调用方从后端 `SearchResult` 组装）。
#[derive(Clone)]
pub struct SearchView {
    pub query: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub scanned: usize,
    pub truncated: bool,
    pub hits: Vec<SearchHit>,
}

/// 命中高亮行：按字节区间把 `content` 切成「普通段 + 命中段」，命中段用命中底色。
///
/// 区间非法（越界 / 非 char 边界 / 重叠）时退化为整体纯文本。
/// `pub`：侧栏搜索面板（scratchpad 侧栏结果树）与中央结果视图共用同一高亮口径。
pub fn hit_line(content: &str, spans: &[(usize, usize)], match_bg: Hsla, fg: Hsla) -> Div {
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

/// 内容搜索结果视图（中央编辑区；形态对标 VS Code 右侧「搜索: query」结果标签页）。
///
/// 头部 = `搜索: query` 标题 + 统计行（`N 文件中有 M 个结果` + 「在编辑器中打开」+「关闭」）；
/// 主体 = 按文件分组命中列表（文件头：类型图标 + 文件名 + 命中数角标；子行：行号 + ":" + 命中行）。
/// 无卡片边框（VS Code 结果视图平铺形态）；替换输入/高级选项在**侧栏搜索面板**（VSCode 左面板）。
///
/// `type_icon`：文件名 → svg 图标路径（如 `"icons/file-code.svg"`），由调用方注入。
pub fn render_search_pane(
    search: &SearchView,
    theme: &Theme,
    match_bg: Hsla,
    type_icon: impl Fn(&str) -> String,
    // 点命中标题 → 打开该文件并跳到命中行。
    on_open_hit: std::rc::Rc<
        dyn Fn(SearchHitTarget, &mut Window, &mut App),
    >,
    on_clear: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let fg = theme.colors.foreground;
    let muted = theme.colors.muted_foreground;
    let primary = theme.colors.primary;
    let border = theme.colors.border;

    // 去重文件数（VS Code 统计口径：`N 文件中有 M 个结果`）。
    let mut files: Vec<&str> = Vec::new();
    for hit in &search.hits {
        if !files.contains(&hit.file.as_str()) {
            files.push(hit.file.as_str());
        }
    }
    let file_count = files.len();

    let open_in_editor = {
        // 在 render 时克隆首个命中（`search` 是借用参数，闭包不能捕获引用 —— E0521）。
        let first_target: Option<SearchHitTarget> = search.hits.first().map(|hit| SearchHitTarget {
            file: hit.file.clone(),
            line: hit.line as u32,
            span: hit.spans.first().copied(),
        });
        let on_open = on_open_hit.clone();
        move |_: &ClickEvent, window: &mut Window, app: &mut App| {
            if let Some(target) = first_target.clone() {
                on_open(target, window, app);
            }
        }
    };
    // 头部（2026-10-10 VSCode 对齐）：tab 标题已承担「搜索: query」，内容区**不再重复大标题**；
    // 首行 = 统计行（原型 es-stats「5 个结果 - 3 文件」措辞，2026-10-11 对齐）+ 操作按钮。
    let stats = if search.hits.is_empty() {
        format!("无结果 - {} 文件", file_count)
    } else {
        format!("{} 个结果 - {} 文件", search.hits.len(), file_count)
    };
    let header = div()
        .v_flex()
        .w_full()
        .gap_1()
        .pb_2()
        .border_b_1()
        .border_color(border)
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(stats),
                )
                .child(div().flex_1())
                .child(
                    Button::new("sp-search-open")
                        .ghost()
                        .small()
                        .label("在编辑器中打开")
                        .on_click(open_in_editor),
                )
                .child(
                    Button::new("sp-search-close")
                        .ghost()
                        .small()
                        .label("关闭")
                        .on_click(on_clear),
                ),
        );

    let mut list = div().v_flex().w_full().gap_1();
    if search.hits.is_empty() {
        list = list.child(div().text_xs().text_color(muted).child("无匹配"));
    }
    // 按文件分组（保持命中首次出现顺序）——VS Code 搜索面板形态：文件头（类型图标 + 文件名
    // + 命中数角标）+ 缩进的命中明细（行号 + 命中行内容）。点击文件头跳该文件首个命中。
    let mut file_order: Vec<&str> = Vec::new();
    let mut groups: std::collections::HashMap<&str, Vec<&SearchHit>> =
        std::collections::HashMap::new();
    for hit in &search.hits {
        if !groups.contains_key(hit.file.as_str()) {
            file_order.push(hit.file.as_str());
        }
        groups.entry(hit.file.as_str()).or_default().push(hit);
    }
    for file in file_order {
        let hits = groups.get(file).map(|v| v.as_slice()).unwrap_or(&[]);
        // 详细定位：文件头显示**完整相对路径**（VS Code 全局搜索结果的文件头带目录层级），
        // 命中明细行在文件头下方缩进（与文件头对齐关系一眼可辨）。
        let base = std::path::Path::new(file)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| file.to_string());
        let first_target = SearchHitTarget {
            file: hits[0].file.clone(),
            line: hits[0].line as u32,
            span: hits[0].spans.first().copied(),
        };
        let open_first = on_open_hit.clone();
        let icon_path = type_icon(&base);
        let mut group = div().v_flex().w_full().gap_0p5();
        // 文件头（原型 es-file，2026-10-11 对齐）：类型微标 + 完整路径（链接式，点击
        // 打开该文件首个命中）+ 右侧类型 tag（SQL / YAML / …）；去掉命中数徽标（原型无）。
        let ext_tag = std::path::Path::new(&base)
            .extension()
            .map(|e| e.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        group = group.child(
            div()
                .id(format!("sp-file-{file}"))
                .h_flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .hover(move |s| s.text_color(primary))
                .on_click(move |_, window, app| {
                    open_first(first_target.clone(), window, app)
                })
                .child(
                    Icon::empty()
                        .path(icon_path)
                        .size(rems(ICON_SIZE_SM))
                        .flex_none(),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(file.to_string()),
                )
                .child(
                    div()
                        .flex_none()
                        .px_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(border)
                        .text_xs()
                        .text_color(muted)
                        .child(ext_tag),
                ),
        );
        let mut hits_block = div().v_flex().w_full().gap_0p5();
        for hit in hits {
            let open_hit = on_open_hit.clone();
            let hit_target = SearchHitTarget {
                file: hit.file.clone(),
                line: hit.line as u32,
                span: hit.spans.first().copied(),
            };
            let line_num = hit.line.to_string();
            let line_row = div()
                .id(format!("sp-line-{}-{}", hit.file, hit.line))
                .h_flex()
                .items_start()
                .gap_1()
                .w_full()
                .min_w_0()
                .pl_3()
                .cursor_pointer()
                .hover(move |s| s.text_color(primary))
                .on_click(move |_, window, app| open_hit(hit_target.clone(), window, app))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(line_num),
                )
                .child(
                    div().flex_none().text_xs().text_color(muted).child(":"),
                )
                .child(hit_line(&hit.content, &hit.spans, match_bg, fg));
            hits_block = hits_block.child(line_row);
        }
        group = group.child(hits_block);
        list = list.child(group);
    }

    div()
        .v_flex()
        .w_full()
        .gap_1()
        // VS Code 结果视图平铺形态：无卡片边框（替代旧版 border+bg 卡片，用户截图对比）。
        .child(header)
        .child(div().max_h(rems(32.)).overflow_y_scrollbar().child(list))
}
