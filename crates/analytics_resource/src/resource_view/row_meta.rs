//! 视图模型与行级纯函数（资源面板的「数据 + 判定」部分，不依赖面板实体）。
//!
//! 拆分自 `resource_view/mod.rs`（P1）：`ArchiveRow` / 分组 / 标签 / 计数 /
//! 实例维度等快照结构，以及强度徽标、行尾、行点击归一化等**纯函数**都在这里；
//! 不接触 `ResourcesPanel` 实体，便于单测。
//!
//! 对外符号契约保持 `crate::resource_view::X` 不变（`mod.rs` 里 `pub use` 承接）。

use gpui_kit::assets::IconName as CatalogIcon;
use gpui_kit::{ClickEvent, Modifiers};

use engine::file_reader_function;

use crate::detail_view::ArchiveDetail;
use crate::model::{ArchiveKind, ArchiveStatus, TagTarget};

// ==================== 视图模型（纯数据，便于单测） ====================

pub use crate::view_model::row::{
    ArchiveCounts, ArchiveRow, GroupOption, InstanceFilter, ResourcesSnapshot, TagOption,
};

/// 复现强度徽标文案（原型 §3.2：**行内唯一的颜色信号**）。
pub fn strength_badge(kind: ArchiveKind, status: ArchiveStatus) -> &'static str {
    match status {
        ArchiveStatus::Missing => "缺失",
        ArchiveStatus::ContentChanged => "内容已变",
        ArchiveStatus::Normal => match kind {
            ArchiveKind::File => "已归档",
            ArchiveKind::Analysis => "分析表",
            ArchiveKind::TableRef => "引用",
        },
    }
}

/// 徽标色调（纯枚举；渲染层再映射到主题色，便于单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Success,
    Info,
    Warning,
    Danger,
}

/// 徽标色调：异常态压过 kind；`引用`与 `内容已变` 同为警示。
pub fn badge_tone(kind: ArchiveKind, status: ArchiveStatus) -> BadgeTone {
    match (status, kind) {
        (ArchiveStatus::Missing, _) => BadgeTone::Danger,
        (ArchiveStatus::ContentChanged, _) => BadgeTone::Warning,
        (ArchiveStatus::Normal, ArchiveKind::File) => BadgeTone::Success,
        (ArchiveStatus::Normal, ArchiveKind::Analysis) => BadgeTone::Info,
        (ArchiveStatus::Normal, ArchiveKind::TableRef) => BadgeTone::Warning,
    }
}

/// kind 图标（形状区分，**不靠颜色**——原型 §6 的视觉通道预算：一行只允许一个颜色信号，
/// 而那个信号已经给了复现强度徽标）。
///
/// 三个形状：文件 = 文档形；分析表 = 表格形；引用 = 外链形。
///
/// `pub(crate)`：详情面板头部用同一套图标（同一口径只此一份）。
pub(crate) fn kind_icon(kind: ArchiveKind) -> CatalogIcon {
    match kind {
        ArchiveKind::File => CatalogIcon::FileText,
        ArchiveKind::Analysis => CatalogIcon::Table,
        ArchiveKind::TableRef => CatalogIcon::ExternalLink,
    }
}

/// 尾部字段：字段优先级为 显示名 > 强度徽标 > 版本徽标 > 相对时间 > 大小/规模。
///
/// 240px 面板放不下的信息一律省略号 + tooltip（渲染层补），**不缩行高、不做两行行**。
pub fn row_tail(detail: &str, modified: &str, version: i32) -> String {
    match (detail.is_empty(), modified.is_empty()) {
        (false, false) => format!("{detail} · {modified}"),
        (false, true) => detail.to_string(),
        (true, false) => modified.to_string(),
        // 两者都缺时给出版本，避免尾部整段空白。
        (true, true) => format!("v{version}"),
    }
}

/// 行右键菜单里「编辑标签」那一项的文案（多选时带项数）。
///
/// 为何抽成纯函数：菜单内部的项读不到（`PopupMenu` 不暴露），而文案是**用户看到的契约**
/// （“这一下会改几条”），所以把可断言的那一半拎出来（与 `dispatch_header_action` 同法）。
pub fn tag_entry_label(multi_count: usize) -> String {
    if multi_count > 1 {
        format!("编辑标签（{multi_count} 项）…")
    } else {
        "编辑标签…".to_string()
    }
}

/// 「编辑标签」要交给宿主的目标集（纯函数）。
///
/// 规则与删除 / 移动完全一致：**这一行在多选集里**→ 整选集（按可见行顺序，回执与测试都要确定性）；
/// 否则只给这一行。抽出来的理由：菜单里那一句只有真点开才能验，而“到底送了几项”
/// 是这张批量功能最容易错的地方（漏送 = 只改了用户点中的那一条）。
pub fn tag_targets(
    rows: &[&ArchiveRow],
    multi_ids: &std::collections::HashSet<String>,
    row: &ArchiveRow,
) -> Vec<TagTarget> {
    let multi_selected = multi_ids.len() > 1 && multi_ids.contains(&row.id);
    if multi_selected {
        rows.iter()
            .filter(|candidate| multi_ids.contains(&candidate.id))
            .map(|candidate| TagTarget {
                id: candidate.id.clone(),
                name: candidate.name.clone(),
            })
            .collect()
    } else {
        vec![TagTarget {
            id: row.id.clone(),
            name: row.name.clone(),
        }]
    }
}

/// 「默认分组」标记文案（分组头上的小字；也是窗口测试的锚点）。
pub const DEFAULT_GROUP_MARKER: &str = "默认";

/// 分组头菜单里那一项的文案（纯函数，带单测）：已是默认分组时是“取消”，否则是“设为”。
pub fn default_group_entry_label(is_default: bool) -> &'static str {
    if is_default {
        "取消归档默认分组"
    } else {
        "设为归档默认分组"
    }
}

/// 行是否可重命名：判据统一在 [`crate::model::can_rename`]（四个入口共用——
/// 多选 / 只读项目 / 全局行都挡）。这里不再自写一份：`F2` 与右键菜单曾各写一套，
/// 结果全局行按 `F2` 会开窗，随后报「还没有打开项目」。

/// 这条存档能否「查看统计」（M8 洞察）：判定按种类分，三类的可分析性来源不同。
///
/// - **受管文件**：本体得是 DuckDB 读得动的数据文件——扩展名口径只有一处
///   （[`engine::file_reader_function`]：CSV / Parquet / Excel / JSON）；
///   `.sql` / `.md` 这类文本不是数据，不给入口。
/// - **远端引用**：有来源连接与表名就能取样（取样 SQL 由宿主按驱动拼，不在面板）。
/// - **分析表**：本体是项目内的 `analytics.duckdb`，要 ATTACH + 重建定义——随后续批次接。
///
/// 缺失的行一律不给：本体不在，取不到数。
pub fn can_view_stats(detail: &ArchiveDetail) -> bool {
    if detail.status == ArchiveStatus::Missing {
        return false;
    }
    match detail.kind {
        ArchiveKind::File => detail
            .payload_rel_path
            .as_deref()
            .and_then(file_reader_function)
            .is_some(),
        ArchiveKind::TableRef => {
            detail.source_connection_id.is_some() && detail.source_table.is_some()
        }
        ArchiveKind::Analysis => false,
    }
}

/// 行点击归一化后的语义（修饰键与点击数 → 四种行为；纯数据便于单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowClick {
    /// 无修饰：单选这一行。
    Select,
    /// `Ctrl` / `Cmd`：切换这一行的多选状态。
    Toggle,
    /// `Shift`：从锚点到这一行的区间选择。
    Range,
    /// 双击：打开（只读）。
    Open,
}

/// 归一化行点击（原型 §2.3：单击选中 / `Ctrl`·`Shift` 多选 / 双击打开）。
///
/// 判定顺序：**双击优先**（双击必然也带 click_count ≥ 2）——打开是更强的意图；
/// `Shift` 优先于 `Ctrl`（两个都按时按区间处理，与多数桌面应用一致）。
pub fn classify_row_click(event: &ClickEvent) -> RowClick {
    classify_click(event.click_count(), event.modifiers())
}

/// [`classify_row_click`] 的纯函数内核（事件取值与判定分开：判定可单测）。
pub(crate) fn classify_click(click_count: usize, modifiers: Modifiers) -> RowClick {
    if click_count >= 2 {
        return RowClick::Open;
    }
    if modifiers.shift {
        return RowClick::Range;
    }
    // gpui-pre 0.3.10 起 `secondary()` 需 `&App` 参数；本函数为纯函数（无 cx），
    // 改用编译期平台判断实现相同语义（macOS = Command 键，其余 = Ctrl）。
    let secondary = if cfg!(target_os = "macos") {
        modifiers.platform
    } else {
        modifiers.control
    };
    if secondary {
        return RowClick::Toggle;
    }
    RowClick::Select
}

/// 行点击对选择集的变换（纯函数）：`multi` 始终按可见行顺序维护，`anchor` 是区间锚点。
///
/// 返回**焦点行**（= 最后点的那条；选择集为空时 `None`）。`order` 是当前可见行顺序。
pub(crate) fn apply_row_click(
    multi: &mut Vec<String>,
    anchor: &mut Option<String>,
    order: &[String],
    id: &str,
    click: RowClick,
) -> Option<String> {
    match click {
        RowClick::Select | RowClick::Open => {
            *multi = vec![id.to_string()];
            *anchor = Some(id.to_string());
        }
        RowClick::Toggle => {
            if let Some(index) = multi.iter().position(|candidate| candidate == id) {
                // 取消最后一条 = 回到无选中（与多数列表一致）。
                multi.remove(index);
            } else {
                multi.push(id.to_string());
                sort_multi_by_order(multi, order);
            }
            *anchor = Some(id.to_string());
        }
        RowClick::Range => {
            let target = order.iter().position(|candidate| candidate == id);
            let from = anchor
                .as_deref()
                .and_then(|anchor| order.iter().position(|candidate| candidate == anchor));
            match (from, target) {
                (Some(from), Some(target)) => {
                    let (low, high) = if from <= target {
                        (from, target)
                    } else {
                        (target, from)
                    };
                    *multi = order[low..=high].to_vec();
                }
                // 锚点不在了（被筛掉 / 从没点过）：退化为单选。
                _ => {
                    *multi = vec![id.to_string()];
                    *anchor = Some(id.to_string());
                }
            }
        }
    }
    (!multi.is_empty()).then(|| id.to_string())
}

/// 按可见行顺序排序（不在列表里的排最后：集合语义，但顺序影响批量回执与测试）。
fn sort_multi_by_order(multi: &mut Vec<String>, order: &[String]) {
    multi.sort_by_key(|id| {
        order
            .iter()
            .position(|candidate| candidate == id)
            .unwrap_or(usize::MAX)
    });
}
