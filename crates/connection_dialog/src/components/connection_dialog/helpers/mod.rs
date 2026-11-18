use super::*;
use shared::i18n::tr;

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

/// Header 标签列宽（rem）：按最长标签「驱动类型」（4 字）定，其余标签（名称 / 备注 / 地址）
/// 共享同一列宽，保证各行控件起点对齐——这正是这列存在的目的；只给长标签加宽会让驱动行的
/// Select 比其它行靠右，视觉磕绊比标签后的留白更明显。
pub(crate) const LABEL_W: f32 = 3.25;

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

/// 对话框左右留给遮罩的余量（rem）：窗口比设计宽度窄时，先把这圈余量让出去。
const DIALOG_SIDE_MARGIN: f32 = 2.;

/// 对话框宽度（**rem 值**）：设计基准 [`crate::ui::DIALOG_EDIT_WIDTH`] 与窗口可用宽度取小。
///
/// 与 [`dialog_row_height`] 同一个理由：设计宽度 61.25rem 是按 980px 定的，
/// 小屏（或高缩放）上窗口逻辑宽度可能只有 700px——固定宽度会让对话框左右溢出、
/// 「保存并退出」被推到屏幕外。下限 32rem：再窄也只是内容区内挤，不压成一条缝。
pub(crate) fn dialog_width(viewport_w_rem: f32) -> Rems {
    let budget = viewport_w_rem - DIALOG_SIDE_MARGIN;
    rems(crate::ui::DIALOG_EDIT_WIDTH.min(budget).max(32.))
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
    // 作用域 =「仅项目」却没有项目：落库目标不存在，先拦在保存之前。
    needs_project: bool,
    has_project: bool,
) -> Option<&'static str> {
    if !has_type {
        Some(tr("请先在左侧选择数据库类型"))
    } else if driver_value.trim().is_empty() {
        Some(tr("请选择驱动实现"))
    } else if needs_project && !has_project {
        Some(tr("请先选择项目（作用域为「仅项目」）"))
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

// ===== 地址口径（字段权威，URI 派生）=====

/// 地址的唯一口径：**结构化字段（host / port / db）是权威，URI 是派生展示**。
///
/// 为什么收成一处：URI 与字段曾各是一份真相源，来回编辑互相覆盖——用户改完端口再改 URI，
/// 谁最后落笔谁赢，落库值取决于手速。现在 UI 侧 URI 只读（由本函数每帧重建），
/// 粘贴整条连接串走「从剪贴板导入」一次性把字段填出来。
///
/// 规则（`rebuild_url_from_fields`）：
/// - 文件型 / 未选驱动：地址就是路径或用户原样输入，原样返回；
/// - 主机为空 → 原样返回（不把半成品输入抹成非法 URL）；
/// - URI 还是空的（新表单）→ 用驱动 `type_id` 补 scheme（`postgres://` 等），
///   否则 `rewrite_url_authority` 认不出 scheme，字段填了地址栏也永远是空的；
/// - 端口空 → 驱动声明的默认端口；数据库空 → 去掉路径段。
pub(crate) fn url_from_fields(
    driver: Option<&Driver>,
    url: &str,
    host: &str,
    port: &str,
    db: &str,
) -> String {
    let Some(d) = driver else {
        return url.to_string();
    };
    if d.is_file || host.trim().is_empty() {
        return url.to_string();
    }
    let seeded;
    let base = if url.contains("://") {
        url
    } else {
        seeded = format!("{}://", d.type_id);
        seeded.as_str()
    };
    crate::services::data_source_service::rebuild_url_from_fields(
        &d.id,
        base,
        host,
        port,
        db,
        d.default_port,
    )
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
/// 内部索引保持不变（0 常规 / 1 网络 / 4 高级），收敛只发生在**呈现层**：
/// - 「能力」(2) 与「驱动属性」(3) 是工程概念，并入「高级」页尾（见 `render` 的高级分支）；
/// - 文件型驱动（SQLite / DuckDB）按原型隐藏「网络」。
///
/// 于是网络型三页、文件型两页；旧草稿里的索引 2 / 3 由 [`normalize_tab_index`]
/// 归一到 4，不会打到一个空白页上。
pub(crate) fn dialog_tab_defs(is_file_db: bool) -> Vec<(&'static str, usize)> {
    if is_file_db {
        vec![(tr("常规"), 0), (tr("高级"), 4)]
    } else {
        vec![(tr("常规"), 0), (tr("网络"), 1), (tr("高级"), 4)]
    }
}

/// 内部 Tab 索引 → TabBar 的**可见下标**（隐藏「网络」后两者不相等，决策 #85）。
///
/// 已收敛的 2 / 3 先归一到「高级」，再查可见下标——否则旧草稿会落到第 0 页。
pub(crate) fn visible_tab_index(defs: &[(&'static str, usize)], active: usize) -> usize {
    let target = match active {
        2 | 3 => 4,
        ix => ix,
    };
    defs.iter().position(|(_, ix)| *ix == target).unwrap_or(0)
}

/// 内部 Tab 索引规范化（渲染入口与草稿恢复都先过这一道，避免旧索引打到空白页）。
///
/// - 2（能力）/ 3（驱动属性）→ 4（高级）：内容已并入高级页尾；
/// - 文件型的 1（网络）→ 0（常规）：那一页根本不存在；
/// - 越界 → 4，夹在最后一个真实页上。
pub(crate) fn normalize_tab_index(active: usize, is_file_db: bool) -> usize {
    match active {
        1 if is_file_db => 0,
        2 | 3 => 4,
        ix if ix <= 4 => ix,
        _ => 4,
    }
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

/// 下拉选项标签：驱动短名 + 来源（`内置` / 插件短码）。
///
/// 为什么加来源：同一族可以有多个实现（`postgresql` 族下插件版与内置版短名
/// 都接近 "PostgreSQL"），只看短名分不清哪个来自插件、哪个是内核自带。
/// 归属列 `plugin_code`（迁移 032）是真源：`None` = 内置，`Some(code)` = 该插件声明。
pub(crate) fn driver_menu_label(driver: &Driver) -> String {
    let short = driver_short_name(&driver.name);
    match driver.plugin_code.as_deref() {
        Some(code) if !code.trim().is_empty() => format!("{short} · {}", plugin_short_code(code)),
        _ => format!("{short} · 内置"),
    }
}

/// 插件 code 的显示短码：`rds.driver.postgres-py` → `postgres-py`。
///
/// 实现收敛在 `shared::utils::string`（插件管理页的驱动标签也用它）：两处显示的短码
/// 必须是同一个词，否则「插件声明的驱动」与「下拉里的驱动」还是对不上号。
pub(crate) use shared::utils::string::plugin_short_code;

/// 驱动实现类型的说法（信息条用）：**按 `drivers.driver_kind` 说话，不一律叫「原生连接」**。
///
/// 为什么不能硬编码：`sidecar` 驱动是插件进程、`native` 是内核内置客户端库，
/// 两者排障路径完全不同（前者看插件日志、后者看连接参数）。信息条说错，
/// 用户会往错的方向查。
pub(crate) fn driver_kind_label(kind: &str) -> &'static str {
    match kind.trim() {
        "native" => tr("原生连接"),
        "sidecar" => tr("插件驱动"),
        "file" => tr("文件型"),
        "" => tr("未声明实现类型"),
        _ => tr("其它实现"),
    }
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
