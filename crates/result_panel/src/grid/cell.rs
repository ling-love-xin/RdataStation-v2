//! 单元格渲染域（`grid/` 第七块）：类型感知样式（`CellStyle` / `format_cell_text`）+
//! 展示裁剪 / JSON / BLOB 摘要 + `TableDelegate::render_td`。

use gpui_kit::*;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::table::TableState;
use gpui_kit::base::TreeItem;

use std::borrow::Cow;

use crate::store::ColumnKind;
use super::delegate::{CELL_DISPLAY_LIMIT, ResultGridDelegate};
use super::status::format_integer_cell;
use super::text::result_cell_tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use workbench_shell::product_tokens;
use workbench_shell::result_style::{AlignPref, ColorRolePref, NumberFormatPref, TypeFamily, TypeStylePrefs};
/// 一格的渲染决策（**纯函数**产出就是这一份；渲染只负责把它落到元素上）
///
/// 为什么单独拿出来：`Unknown` 这一档必须是**今天的样式**，而“没带类型就一个像素不变”
/// 这句话只有被断言过才算数——渲染函数里测不到（gpui 不暴露算出来的对齐与字体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellStyle {
    /// 文本对齐（数字右对齐：个位对齐才看得出一列的数量级差）
    pub(crate) align: TextAlign,
    /// 用等宽字体吗（布尔 / 时间族；字体族从主题取，不引新依赖）
    pub(crate) monospace: bool,
    /// 按族前景色角色（`Inherit` = 不染色，跟随普通前景；渲染时映射到主题语义色）
    pub(crate) color_role: ColorRolePref,
}

/// 列类型 → 类型族（与设置页的五族口径一致；未知类型按文本渲染）
fn type_family(kind: ColumnKind) -> TypeFamily {
    match kind {
        ColumnKind::Integer | ColumnKind::Float | ColumnKind::Decimal => TypeFamily::Numeric,
        ColumnKind::Timestamp | ColumnKind::Date | ColumnKind::Time => TypeFamily::Temporal,
        ColumnKind::Boolean => TypeFamily::Boolean,
        ColumnKind::Binary => TypeFamily::Blob,
        ColumnKind::Text | ColumnKind::Json | ColumnKind::Uuid | ColumnKind::Unknown => {
            TypeFamily::Text
        }
    }
}

/// 列类型 + 全局偏好 → 这一列所有格子的样式
///
/// 【类型样式】对齐覆盖：`AlignPref::Inherit` = 跟随类型默认（数字右对齐 / 其余左对齐）；
/// 其余显式覆盖。前景色角色直接进 `CellStyle`，渲染时按角色取主题语义色（零裸色值）。
pub(crate) fn cell_style(kind: ColumnKind, prefs: &TypeStylePrefs) -> CellStyle {
    let default_align = if kind.is_numeric() {
        TextAlign::Right
    } else {
        TextAlign::Left
    };
    let align = match prefs.align_for(type_family(kind)) {
        AlignPref::Left => TextAlign::Left,
        AlignPref::Center => TextAlign::Center,
        AlignPref::Right => TextAlign::Right,
        AlignPref::Inherit => default_align,
    };
    CellStyle {
        align,
        monospace: kind.is_monospace(),
        color_role: prefs.color_for(type_family(kind)),
    }
}

/// 【类型样式】单元格显示文本的纯函数：NULL 占位 / 布尔映射 / 数字格式。
///
/// 输入**原始驱动文本**（`text == "NULL"` 判定在这里做，与渲染灰斜体口径同一处）；
/// 只影响显示——复制 / 导出 / 编辑 / 悬停 tooltip 仍用原始值。
/// JSON 美化与 BLOB 摘要在渲染层先于本函数处理，不进这里（职责单一）。
pub fn format_cell_text(kind: ColumnKind, text: &str, prefs: &TypeStylePrefs) -> String {
    if text == "NULL" {
        return prefs.null_placeholder.rendered().to_string();
    }
    if kind == ColumnKind::Boolean {
        let (yes, no) = prefs.bool_display.rendered();
        return match text {
            "true" => yes.to_string(),
            "false" => no.to_string(),
            // 驱动给了其它形态（如 `1`/`0`、大小写变体）：保持原文，不猜映射
            _ => text.to_string(),
        };
    }
    if kind.is_numeric() {
        return match prefs.number_format {
            NumberFormatPref::Raw => text.to_string(),
            // 千分位只对整数值分组（小数 / 异常样本回退原文；与既有 format_integer_cell 同口径）
            NumberFormatPref::Grouped => format_integer_cell(text).unwrap_or_else(|| text.to_string()),
            NumberFormatPref::Fixed2 => text
                .parse::<f64>()
                .map(|v| format!("{v:.2}"))
                .unwrap_or_else(|_| text.to_string()),
            NumberFormatPref::Scientific => text
                .parse::<f64>()
                .map(|v| format!("{v:e}"))
                .unwrap_or_else(|_| text.to_string()),
        };
    }
    text.to_string()
}
/// 单元格展示文本字符级裁剪：超过 `limit` 个字符时取前 `limit` 个字符并补省略号 `…`。
///
/// 按 `char` 边界取前若干个，不会裁断中文 / emoji / 组合字符的多字节序列；省略号不占
/// `limit` 额度（与 dbdelve `clip_to` 同口径：300 字符 + 一个尾巴符号，机制借鉴
/// GPL-3.0 dbdelve，仅参考思路未复制代码）。未达上限原样返回。
///
/// **只遍历到第 `limit` 个字符**（`char_indices().nth(limit)`），不先 `chars().count()`
/// 数全串——大值（JSONB / PostGIS 几何，可达数百万字符）每帧渲染成本因此有硬上界，
/// 不随值长线性涨（dbdelve `clip` 同思路：每帧每可见格裁剪，扫描有界）。
pub(crate) fn clip_cell_display(value: &str, limit: usize) -> String {
    match value.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &value[..end]),
        None => value.to_string(),
    }
}

/// 【P1-4】格内 JSON 美化展示：仅当文本以 `{` / `[` 开头且能解析成 JSON 时，
/// 返回缩进格式化串；非 JSON / 解析失败一律 `None`（按普通文本走裁剪，不猜）。
///
/// 只改**展示**：复制 / 悬停 / 右键 / 导出 / 编辑仍取原始 `text`（见 `render_td`）。
/// 裸标量（数字 / 布尔 / 引号串）本来就短，不进这条路径；解析失败不抛错、不 panic。
pub(crate) fn pretty_json_display(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !matches!(trimmed.chars().next(), Some('{') | Some('[')) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// 【dbdelve 借鉴】单元格展示串：短值先 JSON 美化 / 类型格式化（可读性），**超长值
/// 跳过解析、零拷贝借原文**——`serde_json` 全串解析随值长线性涨，大 JSON / 几何文档
/// 每帧重跑成本不可控；机制借鉴 GPL-3.0 dbdelve 的 `CLIP_SCAN_LIMIT` 思路（仅参考
/// 思路未复制代码）。只探测到 `CELL_DISPLAY_LIMIT` 处即知长短，不遍历全串；调用方
/// 随后统一 `clip_cell_display` 收尾（超长分支借出的是原文，裁剪 O(limit) 零拷贝）。
pub(crate) fn cell_display_text<'a>(
    text: &'a str,
    kind: ColumnKind,
    prefs: &TypeStylePrefs,
) -> Cow<'a, str> {
    let short = text.char_indices().nth(CELL_DISPLAY_LIMIT).is_none();
    if short {
        match pretty_json_display(text) {
            Some(pretty) => Cow::Owned(pretty),
            None => Cow::Owned(format_cell_text(kind, text, prefs)),
        }
    } else {
        Cow::Borrowed(text)
    }
}

/// 【P2】BLOB 单元格的摘要文案：`hex → BLOB · N B`
///
/// Arrow 的 display 把二进制序列化成**小写 hex**（每字节两位，见 `arrow-cast` 的
/// `DisplayIndex for GenericBinaryArray`），所以 `len / 2` = 字节数。非偶数 / 含
/// 非 hex 字符的串（驱动 / 上游异常）退回 `None`，按原文本显示，不猜。
pub(crate) fn blob_summary(text: &str) -> Option<String> {
    let hex = text.strip_prefix("0x").unwrap_or(text);
    if hex.len() >= 2
        && hex.len() % 2 == 0
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        Some(format!("BLOB · {} B", hex.len() / 2))
    } else {
        None
    }
}
/// 单元格渲染（2026-10-29 目录拆分：由 `TableDelegate::render_td` 薄转发至此，
/// Rust trait impl 不可分块）
pub(crate) fn render_td(
    delegate: &mut ResultGridDelegate,
    row_ix: usize,
    col_ix: usize,
    window: &mut Window,
    cx: &mut Context<TableState<ResultGridDelegate>>,
) -> impl IntoElement {
        // 【T2a-4】正在编辑的这一格：画编辑框，不画普通文本（渲染位仍是原生 render_td）
        if delegate.is_editing(row_ix, col_ix) {
            return delegate.render_edit_cell(row_ix, col_ix, window, cx).into_any_element();
        }
        let text = delegate.cell(row_ix, col_ix);
        // 【P2-4】转置视图每行对应一个不同的原列，类型按「行」取（字段名列是纯文本）；
        // 普通视图按列取。数值右对齐 / BLOB 摘要 / 等宽因此都随原列走
        let kind = if delegate.transposed {
            match col_ix {
                1 => ColumnKind::Unknown,
                _ => delegate
                    .transpose_columns()
                    .get(row_ix)
                    .and_then(|d| delegate.column_types.get(*d))
                    .and_then(Option::as_ref)
                    .map_or(ColumnKind::Unknown, |column| column.kind),
            }
        } else {
            delegate.column_kind(col_ix)
        };
        let style = cell_style(kind, &delegate.type_style);
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let foreground = theme.colors.foreground;
        // 字体族从主题取（不引新依赖）：只在真要等宽的那几类上取一次
        let mono_family = style.monospace.then(|| theme.mono_font_family.clone());
        // 【类型样式】按族前景色角色 → 主题语义色（零裸色值；`Inherit` = 不染色）。
        // NULL / 被删行的灰字优先级高于染色（它们是「非数据」语义，先灰再谈颜色）。
        let role_color = match style.color_role {
            ColorRolePref::Info => Some(theme.colors.info),
            ColorRolePref::Success => Some(theme.colors.success),
            ColorRolePref::Warning => Some(theme.colors.warning),
            ColorRolePref::Danger => Some(theme.colors.danger),
            ColorRolePref::Primary => Some(theme.colors.primary),
            ColorRolePref::Muted => Some(theme.colors.muted_foreground),
            ColorRolePref::Inherit => None,
        };

        // 行号槽永远是灰的（它不是数据）
        if ResultGridDelegate::is_row_number(col_ix) {
            let is_local = delegate.row_is_local(row_ix);
            let is_deleted = delegate.is_deleted_row(row_ix);
            let local_bg = is_local
                .then(|| product_tokens::get(cx).result_local_row_background(theme));
            return div()
                .px_2()
                .h_full()
                .truncate()
                .text_xs()
                .text_color(muted)
                .when_some(local_bg, |cell, bg| cell.bg(bg))
                .when(is_deleted, |cell| cell.line_through())
                // 用例按它量行高（`.id()` 不登记坐标）：这是「表格密度」那条口径的回归哨兵
                .debug_selector(move || format!("editor-result-rowno-{row_ix}"))
                .child(SharedString::from(text))
                .into_any_element();
        }

        // `NULL` 是 SQL 里的一个真实值：灰 + 斜体，与字符串 "NULL" 区分开（原型 §2.4）
        let is_null = text == "NULL";
        // 【#28-B4】行身份渲染标记：本地新增 / 复制行铺专用底色；被删底层行灰字 + 删除线，
        // 保留在网格内可见（对齐 DBeaver 默认删除呈现；撤销可恢复）。两类身份互斥。
        let is_deleted = delegate.is_deleted_row(row_ix);
        let is_local = delegate.row_is_local(row_ix);
        let local_bg = is_local
            .then(|| product_tokens::get(cx).result_local_row_background(theme));
        // 【P2】二进制列：格子里不铺一长串 hex，给「BLOB · 大小」摘要；
        // 悬停 / 查看完整值仍然给完整 hex（那是看全它的路）
        let is_blob = !is_null && kind == ColumnKind::Binary;
        let display = if is_blob {
            blob_summary(&text).unwrap_or_else(|| text.clone())
        } else {
            // 【dbdelve 借鉴】展示派生统一走 `cell_display_text`：短值 JSON 美化 →
            // 类型格式化；超长值跳过解析零拷贝借原文（解析成本有上界），再统一裁剪。
            clip_cell_display(
                &cell_display_text(&text, kind, &delegate.type_style),
                CELL_DISPLAY_LIMIT,
            )
        };
        // 【B14】右键菜单要用“哪个单元格”：组件库只公开了 `right_clicked_row`，
        // 没有单元格访问器，所以在单元格自己身上记一笔（事件从最内层派发，不会丢）
        let table = cx.entity().clone();
        let mut cell = div()
            // 悬停提示要求元素有 id：gpui 的**流式** `tooltip` 声明在
            // `StatefulInteractiveElement` 上（没 id 的裸 `div` 只有直调 `Interactivity` 的写法），
            // 组件库自己的表头 / 行头单元格也是这么挂的
            .id((
                "editor-result-cell",
                row_ix * (delegate.columns.len() + 1) + col_ix,
            ))
            .px_2()
            .truncate()
            .text_xs()
            // 被删行灰字（与 NULL 同走 muted；NULL 斜体口径不变）；其余按类型样式角色染色
            .text_color(if is_null || is_deleted {
                muted
            } else {
                role_color.unwrap_or(foreground)
            })
            // 【#28-B4】本地行底色（先铺，脏格 / 冲突格后续底色覆盖其上）
            .when_some(local_bg, |cell, bg| cell.bg(bg))
            // 【T2a-3】脏格（未提交变更）：浅警告底色，一眼看出哪些格子会被写回
            .when(delegate.is_dirty(row_ix, col_ix), |cell| {
                cell.bg(theme.colors.warning.opacity(0.16))
            })
            // 【T2a-6】冲突格（提交没写进去）：danger 底色，与脏格区分——优先级高于脏格
            // （冲突格先被标过 dirty 的可能性存在：同一格再编辑后提交，两段底色会叠）
            .when(delegate.is_conflict(row_ix, col_ix), |cell| {
                cell.bg(theme.colors.danger.opacity(0.16))
            })
            // 【类型感知】数字右对齐；其余左对齐
            .text_align(style.align)
            // 【类型感知】布尔 / 时间族用主题的等宽字体（两列放一起时字形对得齐）
            .when_some(mono_family, |cell, family| cell.font_family(family))
            .when(is_null, |cell| cell.italic())
            // 【#28-B4】被删行整格删除线（颜色跟随文字色，gpui-pre 无独立删除线色 builder）
            .when(is_deleted, |cell| cell.line_through())
            // 用例按它点/量真单元格（`Ctrl+C` 复制选中、行距口径都从这里进去）
            .debug_selector(move || format!("editor-result-cell-{row_ix}-{col_ix}"))
            .on_mouse_down(MouseButton::Right, move |_, _window, app| {
                table.update(app, |state, _cx| {
                    state.delegate_mut().set_context_cell(Some((row_ix, col_ix)));
                });
            });
        // 取值可能很宽（JSON / 长文本），`truncate` 之后就只剩悬停这一条看全的路
        if !text.is_empty() {
            let full = text.clone();
            cell = cell.tooltip(move |window, cx| result_cell_tooltip(full.clone(), window, cx));
        }
        cell.child(SharedString::from(display)).into_any_element()
    }

    // ========== 值查看器内容分派（纯函数；事件路径重建树，render 只摆） ==========

    /// 每节点最多展开子项数（大字段截断：数组 / 对象超限时追加「… 还有 N 项」占位）
    pub(crate) const JSON_TREE_CHILD_LIMIT: usize = 500;
    /// hex 转储每行字节数（offset · hex · ASCII 三栏）
    const HEX_DUMP_BYTES_PER_LINE: usize = 16;

    /// 值查看器内容模式（按列类型 + 值形态分派）
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum ViewerMode {
        /// 普通文本：等宽滚动块
        Text,
        /// 可展开 JSON 树（`tree::Tree` 组件）
        Json,
        /// 十六进制转储（offset · hex · ASCII 三栏，分页）
        Hex,
        /// 图片预览（格式 + 字节数；解码走事件路径）
        Image { format: &'static str, size: usize },
    }

    /// 值查看器分派（轻量判定；真正的 JSON 校验在事件路径 `json_tree_items`）：
    /// - Binary：图片魔数命中 → Image，否则 → Hex
    /// - 其余：值以 `{` / `[` 开头（trim 后）→ Json，否则 → Text
    pub(crate) fn classify_value(kind: ColumnKind, text: &str) -> ViewerMode {
        match kind {
            ColumnKind::Binary => match detect_image_format(text) {
                Some(format) => ViewerMode::Image {
                    format,
                    size: text.len() / 2,
                },
                None => ViewerMode::Hex,
            },
            _ => {
                let t = text.trim_start();
                if t.starts_with('{') || t.starts_with('[') {
                    ViewerMode::Json
                } else {
                    ViewerMode::Text
                }
            }
        }
    }

    /// 图片魔数检测（hex 文本前缀，大小写不敏感）：PNG / JPEG / GIF / WebP
    pub(crate) fn detect_image_format(hex: &str) -> Option<&'static str> {
        let h = hex.trim();
        let has = |prefix: &str| {
            h.get(..prefix.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
        };
        if has("89504e47") {
            Some("PNG")
        } else if has("ffd8ff") {
            Some("JPEG")
        } else if has("47494638") {
            Some("GIF")
        } else if has("52494646")
            && h.get(16..24).is_some_and(|s| s.eq_ignore_ascii_case("57454250"))
        {
            // WebP 容器：'RIFF'(0..8) + 4 字节大小(8..16) + 'WEBP'(16..24)
            Some("WebP")
        } else {
            None
        }
    }

    /// hex 文本 → 字节（偶数位才认；末尾不完整忽略）
    pub(crate) fn hex_to_bytes(hex: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        let mut iter = hex.bytes();
        loop {
            let Some(hi) = iter.next() else { break };
            let Some(lo) = iter.next() else { break };
            if let (Some(hi), Some(lo)) = ((hi as char).to_digit(16), (lo as char).to_digit(16)) {
                bytes.push((hi * 16 + lo) as u8);
            }
            // 非法字节对：跳过（保持后续对齐，不静默吞掉整个串）
        }
        bytes
    }

    /// hex 转储分页：每行 `[offset] xx xx … |ascii`（16 字节一行，offset 8 位十六进制）
    pub(crate) fn hex_lines(bytes: &[u8], offset: usize, limit: usize) -> Vec<String> {
        (0..limit)
            .filter_map(|i| {
                let base = offset + i * HEX_DUMP_BYTES_PER_LINE;
                if base >= bytes.len() {
                    return None;
                }
                let end = (base + HEX_DUMP_BYTES_PER_LINE).min(bytes.len());
                let chunk = &bytes[base..end];
                let hex = chunk
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let ascii: String = chunk
                    .iter()
                    .map(|b| if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' })
                    .collect();
                Some(format!("[{base:08x}] {hex:<48} |{ascii}"))
            })
            .collect()
    }

    /// JSON 文本 → 可展开树（嵌套 `TreeItem`；每节点子项上限 `JSON_TREE_CHILD_LIMIT`，
    /// 超出追加「… 还有 N 项」占位；顶层预展开 2 层）。非法 JSON → `None`。
    pub(crate) fn json_tree_items(text: &str) -> Option<Vec<TreeItem>> {
        let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
        fn build(value: &serde_json::Value, key: &str, path: &str, depth: usize) -> TreeItem {
            match value {
                serde_json::Value::Object(map) => {
                    let mut item =
                        TreeItem::new(path.to_string(), format!("{key} 对象（{} 项）", map.len()))
                            .expanded(depth < 2);
                    for (k, v) in map.iter().take(JSON_TREE_CHILD_LIMIT) {
                        item = item.child(build(v, k, &format!("{path}.{k}"), depth + 1));
                    }
                    if map.len() > JSON_TREE_CHILD_LIMIT {
                        item = item.child(TreeItem::new(
                            format!("{path}.__trunc__"),
                            format!("… 还有 {} 项", map.len() - JSON_TREE_CHILD_LIMIT),
                        ));
                    }
                    item
                }
                serde_json::Value::Array(arr) => {
                    let mut item = TreeItem::new(
                        path.to_string(),
                        format!("{key} 数组 [{}]", arr.len()),
                    )
                    .expanded(depth < 2);
                    for (i, v) in arr.iter().take(JSON_TREE_CHILD_LIMIT).enumerate() {
                        item = item.child(
                            build(v, &i.to_string(), &format!("{path}[{i}]"), depth + 1),
                        );
                    }
                    if arr.len() > JSON_TREE_CHILD_LIMIT {
                        item = item.child(TreeItem::new(
                            format!("{path}.__trunc__"),
                            format!("… 还有 {} 项", arr.len() - JSON_TREE_CHILD_LIMIT),
                        ));
                    }
                    item
                }
                _ => TreeItem::new(path.to_string(), format!("{key}: {value}")),
            }
        }
        Some(vec![build(&value, "$", "$", 0)])
    }
