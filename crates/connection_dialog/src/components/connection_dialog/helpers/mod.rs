use shared::i18n::tr;
use super::*;

/// 打开管理器覆盖层（嵌套 Dialog；kind: 0 认证 / 1 网络 / 2 环境）。
/// 组件默认集（`IconName`）不含数据库 / 锁 / 盾牌等图标；应用注册的是全量
/// Lucide 目录（`gpui_kit::assets::AllAssets`），故按资产路径直接加载。
pub(crate) fn lucide(path: &'static str) -> Icon {
    Icon::empty().path(path)
}

/// 设置输入框的值（仅在变化时写入，避免无谓的 notify / 渲染）。
pub(crate) fn set_input_value(
    target: &Entity<InputState>,
    value: String,
    window: &mut Window,
    cx: &mut App,
) {
    if target.read(cx).value().as_ref() != value.as_str() {
        target.update(cx, |s, cx| s.set_value(value, window, cx));
    }
}

/// 设置字符串型 Select 的选中值（空字符串 → 清空选中；暂存列表载入用）。
pub(crate) fn set_select_value(
    sel: &Entity<SelectState<SearchableVec<SharedString>>>,
    value: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let v = value.trim().to_string();
    sel.update(cx, |s, cx| {
        if v.is_empty() {
            s.set_selected_index(None, window, cx);
        } else {
            s.set_selected_value(&SharedString::from(v), window, cx);
        }
    });
}

// ===== UI 尺寸约束（数值登记在 `crate::ui`，见 docs/architecture/ui/ui-design-spec.md）=====
//
// 颜色一律 `theme.colors` token，尺寸一律引用 `ui.rs` 常量或 Tailwind 尺度方法；
// 本模块只做别名（保持既有调用点可读），不再自己声明数值。

pub(crate) use crate::ui::{
    DIALOG_BADGE_HEIGHT as BADGE_H, DIALOG_BADGE_WIDTH as BADGE_W, DIALOG_DRIVER_WIDTH as DRIVER_W,
    DIALOG_FORM_LABEL_WIDTH as LABEL_COL_W, DIALOG_PROJECT_WIDTH as PROJECT_W,
    DIALOG_ROW_HEIGHT as ROW_H, DIALOG_STAGING_HEIGHT as STAGING_H, GAP_LG, GAP_MD, GAP_SM,
};

/// Header 标签列宽（rem）：刚好容纳两字标签（名称 / 备注 / 驱动 / 地址），
/// 不让标签与控件之间留下过大的空白（真机反馈：间距过大）。
pub(crate) const LABEL_W: f32 = 1.75;

/// Header 统一标签列（固定宽度，保证各行标签左对齐，减少视觉磕绊）。
pub(crate) fn header_label(theme: &Theme, text: &'static str) -> Div {
    header_label_state(theme, text, false)
}

/// 同上，但可标出「这个字段还差着」：只换颜色（`danger`），**不加图标 / 不加行**——
/// Header 是三行定高结构，插东西会推挤后续元素（布局不跳动的硬约束）。
pub(crate) fn header_label_state(theme: &Theme, text: &'static str, missing: bool) -> Div {
    let color = if missing {
        theme.colors.danger
    } else {
        theme.colors.muted_foreground
    };
    div()
        .w(rems(LABEL_W))
        .flex_shrink_0()
        .text_xs()
        .text_color(color)
        .child(text)
}

// ===== 侧栏类型筛选与行高（纯函数：渲染与键盘路径共用同一份判据）=====

/// 类型树过滤判据：命中类型名 / 类型 id / 该类型下的**驱动名**。
///
/// 抽出来是因为它有两个消费点：树的渲染，以及「搜索框回车选中第一个匹配」。
/// 两处各写一份迟早会出现“能搜到却选不中”。
pub(crate) fn type_matches_filter(t: &DataSourceType, drivers: &[Driver], filter: &str) -> bool {
    let needle = filter.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    t.name.to_lowercase().contains(&needle)
        || t.id.to_lowercase().contains(&needle)
        || drivers
            .iter()
            .any(|d| d.type_id == t.id && d.name.to_lowercase().contains(&needle))
}

/// 过滤后第一个可选中类型的 id（键盘路径：搜索框回车）。
///
/// 优先返回**有启用驱动**的类型——键盘路径不该把用户送到一条注定被拒的路上
/// （`select_type` 对“暂无可用驱动”是不切换的）；若命中项全都无驱动，则退回第一个命中项，
/// 让 `select_type` 给出「暂无可用驱动」的原因行（而不是回车后什么都没发生）。
pub(crate) fn first_selectable_type(
    types: &[DataSourceType],
    drivers: &[Driver],
    filter: &str,
) -> Option<String> {
    let matched: Vec<&DataSourceType> = types
        .iter()
        .filter(|t| type_matches_filter(t, drivers, filter))
        .collect();
    matched
        .iter()
        .find(|t| type_has_driver(drivers, &t.id))
        .or_else(|| matched.first())
        .map(|t| t.id.clone())
}

/// 对话框行以外的垂直开销估算（标题 / 内距 / footer；实测约 5.5rem）。
///
/// 只用于矮窗口的**提前夹住**，不需要精确：Dialog 的纵向位置是 `viewport_h / 10`（组件行为），
/// 所以真正要保证的是「`viewport_h/10 + 开销 + 行高 + 开销` 不超屏」。
const DIALOG_CHROME_HEIGHT: f32 = 5.5;

/// 两列行的实际行高（**rem 值**）：设计基准 [`crate::ui::DIALOG_BODY_HEIGHT`] 与窗口可用高度取小。
///
/// 参参是“窗口可用高度按 rem 计”（调用点传 `viewport_height / rem_size`）——只做纯数运算，
/// 所以能直接单测（不需要窗口）。
///
/// 为什么需要：基准 32.5rem = 520px，加上标题 / footer 后对话框总高约 610px，
/// 而纵向位置是窗口高 / 10——在 768 高的小屏（再叠 125% 缩放 → 逻辑高 ~614px）上
/// 会把 footer 顶到屏幕外。夹住后由内容区（`flex_1`）与侧栏（`h_full`）自己让位，
/// 不靠调用方记得。下限 16rem：再矮的窗口也只是内部滚动，不会压成一条缝。
pub(crate) fn dialog_row_height(viewport_h_rem: f32) -> Rems {
    let budget = viewport_h_rem * 0.9 - DIALOG_CHROME_HEIGHT;
    rems(crate::ui::DIALOG_BODY_HEIGHT.min(budget).max(16.))
}

// ===== 最小可保存集（缺口提示）=====

/// 还差什么才能测试 / 保存：按顺序给出**第一个**缺口，全齐则 `None`。
///
/// 判据与 `ClonedDialogState::collect` 逐项一致（类型/驱动 → 名称 → 地址），三处消费它：
/// 保存 / 测试的拦截文案、Header 标签标红、以及后续若要给按钮置灰。
/// 抽成纯函数是为了让它可单测——散在渲染里就没人能拦住判据漂移。
pub(crate) fn save_blocker(
    has_type: bool,
    driver_value: &str,
    name: &str,
    url: &str,
    is_file_db: bool,
) -> Option<&'static str> {
    if !has_type {
        Some(tr("请先在左侧选择数据库类型"))
    } else if driver_value.trim().is_empty() {
        Some(tr("请选择驱动实现"))
    } else if name.trim().is_empty() {
        Some(tr("请填写连接名称"))
    } else if url.trim().is_empty() {
        Some(if is_file_db {
            tr("请选择或新建数据库文件（常规 → 连接设置）")
        } else {
            tr("请填写连接 URI")
        })
    } else {
        None
    }
}

// ===== 标签文本 ↔ JSON（UI 逗号分隔 ⇄ 落库 / 回读）=====

/// 标签文本 → JSON 数组（支持中英文逗号；空列表返回 None，避免写入空串）。
pub(crate) fn tags_to_json(text: &str) -> Option<String> {
    let mut list: Vec<String> = Vec::new();
    for part in text.split([',', '，']) {
        let t = part.trim();
        if !t.is_empty() && !list.iter().any(|x| x == t) {
            list.push(t.to_string());
        }
    }
    if list.is_empty() {
        None
    } else {
        serde_json::to_string(&list).ok()
    }
}

/// JSON 数组 → 标签文本（回读回显；解析失败返回空串）。
pub(crate) fn tags_from_json(json: Option<&str>) -> String {
    json.and_then(|t| serde_json::from_str::<Vec<String>>(t).ok())
        .map(|list| list.join(", "))
        .unwrap_or_default()
}

// ===== 对话框 Tab 定义与可见下标映射（#15：TabBar 迁移后的可测接缝）=====

/// Tab 定义（显示名, **内部 Tab 索引**）。
///
/// 文件型驱动（SQLite / DuckDB）按原型隐藏「网络」Tab——内部索引保持不变，
/// 所以 `active_tab` 与内容分支（0 常规 / 1 网络 / 2 能力 / 3 驱动属性 / 4 高级）不受影响。
pub(crate) fn dialog_tab_defs(is_file_db: bool) -> Vec<(&'static str, usize)> {
    if is_file_db {
        vec![("常规", 0), ("能力", 2), ("驱动属性", 3), ("高级", 4)]
    } else {
        vec![
            ("常规", 0),
            ("网络", 1),
            ("能力", 2),
            ("驱动属性", 3),
            ("高级", 4),
        ]
    }
}

/// 内部 Tab 索引 → TabBar 的**可见下标**（隐藏「网络」后两者不相等，决策 #85）。
pub(crate) fn visible_tab_index(defs: &[(&'static str, usize)], active: usize) -> usize {
    defs.iter().position(|(_, ix)| *ix == active).unwrap_or(0)
}

// ===== 数据库类型 / 驱动展示（Header 去噪 + 条目类型徽标）=====

/// 驱动实现短名：`MySQL (sqlx)` → `sqlx`；无括号（或括号内为空）时返回原值。
///
/// 数据库类型已在左侧栏选定，Header 下拉只需呈现实现差异（sqlx / Official /
/// rusqlite / duckdb-rs），避免每项重复携带类型名。
/// 同一类型下驱动短名唯一（如 MySQL 下 `sqlx` 与 `Official`），跨类型同名不冲突——
/// 下拉选项按当前类型过滤。
pub(crate) fn driver_short_name(name: &str) -> String {
    let trimmed = name.trim();
    for (open, close) in [('(', ')'), ('（', '）')] {
        if let Some(start) = trimmed.find(open) {
            if let Some(rel_end) = trimmed[start + open.len_utf8()..].find(close) {
                let inner =
                    trimmed[start + open.len_utf8()..start + open.len_utf8() + rel_end].trim();
                if !inner.is_empty() {
                    return inner.to_string();
                }
            }
        }
    }
    trimmed.to_string()
}

/// 解析驱动声明的属性默认值（`drivers.driver_properties`：SQL 对象 → **按键排序**的键值列表）。
///
/// 空 / 非法 JSON / 空键 → 空列表（**不造默认值**）：属性页的默认值必须来自驱动声明
/// （`descriptors.rs` 里各驱动只声明该客户端库真认的键），UI 编造一个键就是“写了不生效”
/// 或“写了就连不上”（能力矩阵 §2.1 的「未知参数」列）。
/// 非字符串值按 JSON 文本给出（与落库路径 `connection_service::apply_driver_properties` 同口径）。
pub(crate) fn driver_property_defaults(json: Option<&str>) -> Vec<(String, String)> {
    let Some(map) = json.and_then(|s| {
        serde_json::from_str::<std::collections::BTreeMap<String, serde_json::Value>>(s).ok()
    }) else {
        return Vec::new();
    };
    map.into_iter()
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, v)| {
            let value = match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            (k, value)
        })
        .collect()
}

/// 属性行「去向」提示的严重程度（决定颜色：Info = 弱化 / Warn = 警告 / Danger = 危险）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropertyNoteLevel {
    Info,
    Warn,
    Danger,
}

/// 属性行下方的「去向」提示：这个键到底会怎样（`None` = 按原样下发且无需说明）。
///
/// 判据全部来自引擎的 [`engine::driver::property_spec`]（依据客户端库源码，唯一真相源）——
/// 这里只把判决翻译成一句人话，不自己判断键的真伪（§15：UI 不造数据）。
pub(crate) fn property_note(driver_id: &str, key: &str) -> Option<(String, PropertyNoteLevel)> {
    match engine::driver::driver_property_verdict(driver_id, key) {
        engine::driver::PropertyVerdict::Delivered {
            param,
            label,
            note,
            caution,
        } => {
            let renamed = param != key.trim();
            if !renamed && note.is_none() && caution.is_none() {
                // 原名直通、没有要说明的 → 不必给提示（不啰嗝）
                return None;
            }
            let mut text = if renamed {
                format!("下发为 {param}")
            } else {
                "会下发".to_string()
            };
            if let Some(l) = label {
                text = format!("{l} · {text}");
            }
            // 副作用（会覆盖别的设置）按警告色；中性说明按弱化色
            match (caution, note) {
                (Some(c), _) => Some((format!("{text}（{c}）"), PropertyNoteLevel::Warn)),
                (None, Some(n)) => Some((format!("{text}（{n}）"), PropertyNoteLevel::Info)),
                (None, None) => Some((text, PropertyNoteLevel::Info)),
            }
        }
        engine::driver::PropertyVerdict::DriverSide {
            applied_as,
            label,
            note,
            caution,
        } => {
            let renamed = applied_as != key.trim();
            let mut text = if renamed {
                format!("由驱动应用为 {applied_as}")
            } else {
                "由驱动应用".to_string()
            };
            if let Some(l) = label {
                text = format!("{l} · {text}");
            }
            match (caution, note) {
                (Some(c), _) => Some((format!("{text}（{c}）"), PropertyNoteLevel::Warn)),
                (None, Some(n)) => Some((format!("{text}（{n}）"), PropertyNoteLevel::Info)),
                (None, None) => Some((text, PropertyNoteLevel::Info)),
            }
        }
        engine::driver::PropertyVerdict::Unknown { effect } => Some(match effect {
            engine::driver::UnknownEffect::Ignored => (
                "当前实现不认这个键，不会被应用（写了不生效）".to_string(),
                PropertyNoteLevel::Warn,
            ),
            engine::driver::UnknownEffect::ConnectionError => (
                "当前实现不认这个键：连接会因未知参数**直接报错**".to_string(),
                PropertyNoteLevel::Danger,
            ),
        }),
        engine::driver::PropertyVerdict::Unclassified => Some((
            "未收录该驱动的属性规格，去向未知（按原样下发）".to_string(),
            PropertyNoteLevel::Info,
        )),
    }
}

/// 指定数据库类型下的启用驱动（保持 drivers 目录顺序）。
pub(crate) fn enabled_drivers_of_type(drivers: &[Driver], type_id: &str) -> Vec<Driver> {
    drivers
        .iter()
        .filter(|d| d.enabled && d.type_id == type_id)
        .cloned()
        .collect()
}


mod capability;
mod fields;
mod files;
mod summary;
#[cfg(test)]
mod tests;

pub(crate) use capability::*;
pub(crate) use fields::*;
pub(crate) use files::*;
pub(crate) use summary::*;