// ==================== 尺寸常量（`crate::ui`：结构尺寸单一来源） ====================

// 继承父模块（mock_view）的全部 use 绑定
use super::*;

// 行数 / 种子 / 参数输入宽、预览格宽等七个结构尺寸已登记到 `crates/mock/src/ui.rs`
// （与外壳同源的走重导出）。视图内只引用 `ui::X`，不再就地声明。

/// 单次生成行数上限（误输入护栏）
const MAX_ROWS: u32 = 1_000_000;
/// 预览显示行数上限
pub(crate) const PREVIEW_ROWS: usize = 10;

/// 预览行数档（面板菜单；生成时只读了 [`PREVIEW_ROWS`] 行，超出即重查临时表）。
///
/// 上限 200：预览是**看一眼数据长什么样**，不是浏览全表——要看全请落库 / 导出后查那张表；
/// 而每次重查的 `LIMIT` 越大代价越大（宽表 200 行已经不止一屏）。
pub(crate) const PREVIEW_LIMIT_CHOICES: [usize; 5] = [PREVIEW_ROWS, 25, 50, 100, 200];

/// 13 种语言（面板下拉用）
pub(crate) const LOCALES: [Locale; 13] = [
    Locale::ZhCn,
    Locale::En,
    Locale::JaJp,
    Locale::ZhTw,
    Locale::FrFr,
    Locale::DeDe,
    Locale::ItIt,
    Locale::PtBr,
    Locale::PtPt,
    Locale::NlNl,
    Locale::ArSa,
    Locale::TrTr,
    Locale::FaIr,
];

/// 文件出口格式（「另存为」与「保存到草稿箱」共用；`Table` 是引擎内部用法，不在此列）。
pub(crate) const FILE_FORMATS: [(&str, MockExportFormat); 4] = [
    ("CSV", MockExportFormat::Csv),
    ("Parquet", MockExportFormat::Parquet),
    ("Xlsx", MockExportFormat::Xlsx),
    ("SQL INSERT", MockExportFormat::SqlInsert),
];

/// 语言显示名。
pub fn locale_label(locale: &Locale) -> &'static str {
    match locale {
        Locale::ZhCn => "简体中文",
        Locale::En => "英语",
        Locale::JaJp => "日语",
        Locale::ZhTw => "繁体中文",
        Locale::FrFr => "法语",
        Locale::DeDe => "德语",
        Locale::ItIt => "意大利语",
        Locale::PtBr => "葡萄牙语（巴西）",
        Locale::PtPt => "葡萄牙语",
        Locale::NlNl => "荷兰语",
        Locale::ArSa => "阿拉伯语",
        Locale::TrTr => "土耳其语",
        Locale::FaIr => "波斯语",
    }
}

/// 可选列类型（13 种，与 `ColumnDataType` 一一对应）。
pub const COLUMN_TYPES: [ColumnDataType; 13] = [
    ColumnDataType::Integer,
    ColumnDataType::BigInt,
    ColumnDataType::Float,
    ColumnDataType::Double,
    ColumnDataType::Decimal {
        precision: 18,
        scale: 2,
    },
    ColumnDataType::Boolean,
    ColumnDataType::Varchar { length: None },
    ColumnDataType::Text,
    ColumnDataType::Date,
    ColumnDataType::DateTime,
    ColumnDataType::Timestamp,
    ColumnDataType::Uuid,
    ColumnDataType::Blob,
];

/// 类型显示名（下拉与清单共用）。
pub fn column_type_label(data_type: &ColumnDataType) -> String {
    data_type.to_duckdb_type()
}

// ==================== 纯逻辑（无窗口依赖，可单测） ====================

/// 解析行数输入：只接受正整数，上限 [`MAX_ROWS`]。
pub(crate) fn parse_rows(text: &str) -> Result<u32, String> {
    match text.trim().parse::<u32>() {
        Ok(n) if n > 0 => Ok(n.min(MAX_ROWS)),
        _ => Err("行数需为正整数".to_string()),
    }
}

/// 解析种子输入：空串表示随机（`None`）。
pub(crate) fn parse_seed(text: &str) -> Result<Option<u32>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed
        .parse::<u32>()
        .map(Some)
        .map_err(|_| "种子需为 0~4294967295 的整数，留空则随机".to_string())
}

/// 解析空值率输入（按百分比编辑）：0~100 → 0.0~1.0；非法返回 `None`（保留原值）。
pub(crate) fn parse_percent_ratio(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .map(|percent| (percent / 100.0).clamp(0.0, 1.0))
}

/// 校验目标表名（非空、字母数字与下划线、不以数字开头）。
pub(crate) fn validate_table_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("表名不能为空".to_string());
    }
    if !trimmed.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("表名只能包含字母、数字与下划线".to_string());
    }
    if trimmed.chars().next().is_some_and(|c| c.is_numeric()) {
        return Err("表名不能以数字开头".to_string());
    }
    Ok(trimmed.to_string())
}

/// 集合类参数最多多少项（防手滑贴进几万行；这些是取值集合，不是数据）。
const MAX_COMPLEX_ITEMS: usize = 1000;

/// 复杂参数（集合 / 加权选项）的文本 → JSON 值。
///
/// 格式（与 [`complex_param_text`] 成对）：
/// - `values`（外键取值 / 序列取值）：一行一个值，空行忽略；
/// - `choices`（加权选项）：一行「值, 权重」（半角/全角逗号或制表符分隔，取**最后**一个分隔符，
///   所以值里可以带逗号）。
///
/// 校验不通过**不猜**：返回带行号的可读原因，调用方保留上一个有效值并就地提示。
/// 为何必须拦住空集合与全零权重：生成期对它们会直接 panic（见 `MockEngine` 的 `generator_param_problem`）。
pub(crate) fn parse_complex_param(key: &str, text: &str) -> Result<serde_json::Value, String> {
    let lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line.trim()))
        .filter(|(_, line)| !line.is_empty())
        .collect();
    // 日历列表允许清空（“今年没有额外假日”是合法配置）；取值集合不允许——空集合抽不出值
    if lines.is_empty() && !matches!(key, "skip_dates" | "work_dates") {
        return Err("至少要有一个值".to_string());
    }
    if lines.len() > MAX_COMPLEX_ITEMS {
        return Err(format!(
            "最多 {MAX_COMPLEX_ITEMS} 项（当前 {} 项）",
            lines.len()
        ));
    }

    match key {
        // 工作日历的两个日期列表：允许留空（空 = 没有例外日子），不在“至少一个值”之列
        "skip_dates" | "work_dates" => Ok(serde_json::Value::Array(
            lines
                .iter()
                .map(|(_, line)| serde_json::Value::from((*line).to_string()))
                .collect(),
        )),
        "values" => Ok(serde_json::Value::Array(
            lines
                .iter()
                .map(|(_, line)| serde_json::Value::from((*line).to_string()))
                .collect(),
        )),
        "choices" => {
            let mut choices = Vec::with_capacity(lines.len());
            let mut has_positive = false;
            for (line_no, line) in lines {
                let Some((name, weight_text)) = split_choice(line) else {
                    return Err(format!("第 {line_no} 行「{line}」：期望「值, 权重」"));
                };
                let Ok(weight) = weight_text.parse::<f64>() else {
                    return Err(format!("第 {line_no} 行「{weight_text}」：权重需为数字"));
                };
                if !weight.is_finite() || weight < 0.0 {
                    return Err(format!(
                        "第 {line_no} 行「{weight_text}」：权重需为不小于 0 的数字"
                    ));
                }
                has_positive |= weight > 0.0;
                choices.push(serde_json::Value::Array(vec![
                    serde_json::Value::from(name),
                    serde_json::Value::from(weight),
                ]));
            }
            if !has_positive {
                return Err("至少一个权重要大于 0（否则无法抽样）".to_string());
            }
            Ok(serde_json::Value::Array(choices))
        }
        other => Err(format!("暂不支持编辑该参数：{other}")),
    }
}

/// 加权选项的一行：`值, 权重`（分隔符取**最后一个**，值里可以带逗号 / 全角逗号 / 制表符）。
fn split_choice(line: &str) -> Option<(&str, &str)> {
    let (index, separator) = line
        .char_indices()
        .filter(|(_, c)| matches!(c, ',' | '，' | '\t'))
        .last()?;
    let name = line[..index].trim();
    let weight = line[index + separator.len_utf8()..].trim();
    (!name.is_empty() && !weight.is_empty()).then_some((name, weight))
}

/// 复杂参数的初值文本（JSON 值 → 多行文本，与 [`parse_complex_param`] 成对）。
pub(crate) fn complex_param_text(config: &GeneratorConfig, key: &str) -> String {
    let Some(serde_json::Value::Array(items)) =
        payload_of(config).and_then(|p| p.get(key).cloned())
    else {
        return String::new();
    };
    match key {
        "choices" => items
            .iter()
            .map(|item| {
                let name = item.get(0).and_then(|v| v.as_str()).unwrap_or_default();
                let weight = item.get(1).and_then(|v| v.as_f64()).unwrap_or_default();
                format!("{name}, {weight}")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => items
            .iter()
            .map(|item| item.as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// 复杂参数的格式提示（对话框里贴在多行输入下方）。
pub(crate) fn complex_param_hint(key: &str) -> &'static str {
    match key {
        "choices" => "每行「值, 权重」，如「北京, 3」；权重越大越容易被选中",
        // 日历列表允许留空（空 = 没有例外日子），提示里写明格式
        "skip_dates" => {
            "每行一个日期，如「2026-10-01」；这些日子不算工作日（留空则只按工作周判定）"
        }
        "work_dates" => "每行一个日期，如「2026-10-10」；调休上班的日子，优先于工作周与跳过日期",
        _ => "每行一个值，如「已发货」",
    }
}

/// 把生成器参数按 JSON 补丁写回配置（143 变体零手工构造，见 `generator_catalog`）。
pub(crate) fn patch_param(
    config: &GeneratorConfig,
    key: &str,
    text: &str,
    kind: ParamKind,
) -> Option<GeneratorConfig> {
    let next = match kind {
        ParamKind::Int => serde_json::Value::from(text.trim().parse::<i64>().ok()?),
        ParamKind::Float => serde_json::Value::from(text.trim().parse::<f64>().ok()?),
        ParamKind::Text => serde_json::Value::from(text.trim().to_string()),
        ParamKind::Bool => serde_json::Value::from(matches!(
            text.trim().to_lowercase().as_str(),
            "true" | "1" | "是" | "yes"
        )),
        // 复杂参数走 `parse_complex_param`（需要校验与行号提示），不在这里续写解析
        ParamKind::Complex => parse_complex_param(key, text).ok()?,
    };
    patch_param_value(config, key, next)
}

/// 把已解析好的 JSON 值写进生成器对应字段（复杂参数的入口）。
pub(crate) fn patch_param_value(
    config: &GeneratorConfig,
    key: &str,
    next: serde_json::Value,
) -> Option<GeneratorConfig> {
    let mut value = serde_json::to_value(config).ok()?;
    let outer = value.as_object_mut()?;
    let payload = match outer.len() {
        1 => outer.values_mut().next()?.as_object_mut()?,
        _ => return None,
    };
    let slot = payload.get_mut(key)?;
    *slot = next;
    serde_json::from_value(value).ok()
}

/// 取变体载荷对象（外部标记枚举的唯一键）。
fn payload_of(config: &GeneratorConfig) -> Option<serde_json::Map<String, serde_json::Value>> {
    let value = serde_json::to_value(config).ok()?;
    let outer = value.as_object()?;
    if outer.len() != 1 {
        return None;
    }
    let key = outer.keys().next()?.clone();
    outer.get(&key)?.as_object().cloned()
}

/// 生成器参数摘要（一行说清：`最小值 1 · 最大值 100`）。
/// 工作周掩码（7 位，周一~周日）翻成人话：`1111100` → 「周一~周五」。
///
/// 只用于展示：掩码非法时原样返回（生成前的护栏会拦住非法值并给出原因）。
pub(crate) fn work_week_text(mask: &str) -> String {
    const NAMES: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];
    let bytes = mask.as_bytes();
    if bytes.len() != 7 || !bytes.iter().all(|b| *b == b'0' || *b == b'1') {
        return mask.to_string();
    }
    let work: Vec<&str> = bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'1')
        .map(|(i, _)| NAMES[i])
        .collect();
    match work.len() {
        0 => "（无上班日）".to_string(),
        7 => "每天".to_string(),
        _ => {
            // 从第一个上班日往后数连续上班天数：等于总数就是一段区间（含跨周日的环）
            let start = bytes.iter().position(|b| *b == b'1').unwrap_or(0);
            let run = (0..7)
                .take_while(|i| bytes[(start + i) % 7] == b'1')
                .count();
            if run == work.len() {
                let end = (start + work.len() - 1) % 7;
                if work.len() == 1 {
                    format!("周{}", NAMES[start])
                } else {
                    format!("周{}~周{}", NAMES[start], NAMES[end])
                }
            } else {
                work.iter()
                    .map(|day| format!("周{day}"))
                    .collect::<Vec<_>>()
                    .join("、")
            }
        }
    }
}

pub(crate) fn summarize_params(config: &GeneratorConfig) -> String {
    let spec = generator_catalog::spec_of(config);
    if spec.params.is_empty() {
        return String::new();
    }
    let Some(payload) = payload_of(config) else {
        return String::new();
    };
    let mut parts = Vec::new();
    // 工作日历一上就是 7 个参数：只在开关打开时才展开，「关着的开关」与「空列表」
    // 不占摘要位置——否则字段卡片那一行会被「工作周 … · 跳过日期 0 项 · 上班日期 0 项」撑爆
    let calendar_on = payload
        .get("workdays_only")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let hours_on = payload
        .get("work_hours_only")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    for field in spec.params {
        let Some(raw) = payload.get(field.key) else {
            continue;
        };
        if matches!(field.key, "workdays_only" | "work_hours_only") && raw.as_bool() != Some(true) {
            continue;
        }
        if matches!(field.key, "work_week" | "skip_dates" | "work_dates") && !calendar_on {
            continue;
        }
        if matches!(field.key, "work_hour_start" | "work_hour_end") && !hours_on {
            continue;
        }
        // 窗口是默认值时不重复占位：开关的标签已经写着「仅工作时段」
        if (field.key == "work_hour_start" && raw.as_str() == Some("09:00"))
            || (field.key == "work_hour_end" && raw.as_str() == Some("18:00"))
        {
            continue;
        }
        if matches!(field.key, "skip_dates" | "work_dates")
            && raw.as_array().map(|items| items.is_empty()).unwrap_or(true)
        {
            continue;
        }
        let text = match field.kind {
            ParamKind::Complex => {
                let count = raw.as_array().map(|items| items.len()).unwrap_or(0);
                format!("{count} 项")
            }
            ParamKind::Bool => if raw.as_bool().unwrap_or(false) {
                "是"
            } else {
                "否"
            }
            .to_string(),
            // 工作周掩码：`1111100` 这种人看不懂，翻成「周一~周五」
            ParamKind::Text if field.key == "work_week" => {
                work_week_text(raw.as_str().unwrap_or_default())
            }
            _ => match raw {
                serde_json::Value::Null => "未设".to_string(),
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        };
        parts.push(format!("{} {}", field.label, text));
    }
    parts.join(" · ")
}

/// 参数初值文本（JSON 载荷值 → 输入框字符串）。
pub(crate) fn param_text(config: &GeneratorConfig, key: &str) -> String {
    payload_of(config)
        .and_then(|payload| payload.get(key).cloned())
        .map(|raw| match raw {
            serde_json::Value::String(s) => s,
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

/// 按类型给一个可用默认生成器（手工加列用；历史重放时也用它兜底）。
pub(crate) fn default_generator_for(data_type: &ColumnDataType) -> GeneratorConfig {
    match data_type {
        ColumnDataType::Integer | ColumnDataType::BigInt => {
            GeneratorConfig::RandomInt { min: 1, max: 1000 }
        }
        ColumnDataType::Float | ColumnDataType::Double => GeneratorConfig::RandomFloat {
            min: 0.0,
            max: 1000.0,
            precision: 2,
        },
        ColumnDataType::Decimal { .. } => GeneratorConfig::RandomDecimal {
            min: 0.0,
            max: 1000.0,
            scale: 2,
        },
        ColumnDataType::Boolean => GeneratorConfig::Boolean { ratio: 50 },
        _ => GeneratorConfig::Sentence { min: 1, max: 3 },
    }
}

/// 新建列的默认定义（手工加列用）：类型默认生成器，置信度 `manual`。
pub(crate) fn new_column_spec(id: u64, name: String, data_type: ColumnDataType) -> MockColumnSpec {
    let generator = default_generator_for(&data_type);
    MockColumnSpec {
        id,
        def: ColumnDef {
            name,
            data_type,
            generator,
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        },
        confidence: "manual".to_string(),
        sample_value: String::new(),
    }
}

/// 数据行数的千分位文案（`161000` → `161,000`）。
///
/// 场景模板的合计行数可达六位（社交平台 161,000 行），不加分隔难以一眼读量；
/// 与 `analytics_resource::present` / `insight::model` 各自保留一份同口径小助手的做法一致
/// （只在文案上用，不引入依赖）。
///
/// 注意：只用于**数据行数**；`第 N 行` 这类**行号**不要过它。
pub(crate) fn with_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 导出文件名（含扩展名）。
pub(crate) fn mock_file_name(table_name: &str, format: &MockExportFormat) -> String {
    let ext = match format {
        MockExportFormat::Csv => "csv",
        MockExportFormat::Parquet => "parquet",
        MockExportFormat::Xlsx => "xlsx",
        MockExportFormat::SqlInsert => "sql",
        MockExportFormat::Table => "duckdb",
    };
    format!("{table_name}.{ext}")
}

/// 「最近使用」的生成器最多记几条（菜单只放最前面这几条，多了也翻不动）。
pub(crate) const RECENT_GENERATORS: usize = 5;

/// 生成器选择菜单（按分类分子菜单）——菜单路径：知道「属于哪类」时最快。
///
/// 详情 tab 的字段行专用（列编辑对话框里生成器是只读展示，见 D19）；
/// 选中即写回**配置面板**（状态单一权威）。菜单自上而下（B7）：
///
/// 0. **生成器库…**——双视角浏览 + 分组「＋」自定义生成器（§9 第四轮）；
/// 1. **自定义**（本会话自定义生成器，空则不显示）——点按解包 base 写回列；
/// 2. **最近使用**（本会话点过的，最多 [`RECENT_GENERATORS`] 条）——常用的不必再翻分类；
/// 3. 「搜索生成器…」——只记得名字时的第二条路径；
/// 4. 15 类子菜单：当前生成器打勾，**智能映射推荐**的那项标「推荐」。
pub(crate) fn generator_menu(
    panel: Entity<MockPanel>,
    id: u64,
    current: String,
    recent: Vec<String>,
    recommended: String,
    user_generators: Vec<UserGenerator>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, cx| {
        let mut menu = menu;
        // 0. 生成器库（双视角浏览 + 自定义入口；自定义管理的主路径）
        let library = panel.clone();
        menu = menu
            .item(
                PopupMenuItem::new("生成器库…").on_click(move |_, window, app| {
                    library.update(app, |panel, cx| panel.open_generator_library(id, window, cx));
                }),
            )
            .separator();
        // 1. 自定义区（空则不显示）：项 = 自定义名 + 数据类徽标；点击解包 base 写回列
        if !user_generators.is_empty() {
            menu = menu.label("自定义");
            for ug in user_generators.iter() {
                let panel = panel.clone();
                let ug_id = ug.id.clone();
                let ug_label = format!("{} · {}", ug.name, column_type_label(&ug.data_type));
                menu = menu.item(
                    PopupMenuItem::new(ug_label).on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| {
                            panel.apply_user_generator(id, &ug_id, cx)
                        });
                    }),
                );
            }
            menu = menu.separator();
        }
        if !recent.is_empty() {
            menu = menu.label("最近使用");
            for name in recent.iter() {
                let Some(spec) = generator_catalog::all_specs()
                    .iter()
                    .copied()
                    .find(|spec| spec.name == name.as_str())
                else {
                    continue;
                };
                let panel = panel.clone();
                let picked = spec.name;
                menu = menu.item(
                    PopupMenuItem::new(generator_menu_label(picked, spec.label, &recommended))
                        .checked(current == picked)
                        .on_click(move |_, _, app| {
                            panel.update(app, |panel, cx| panel.set_generator(id, picked, cx));
                        }),
                );
            }
            menu = menu.separator();
        }
        let search = panel.clone();
        menu = menu
            .item(
                PopupMenuItem::new(format!(
                    "搜索生成器…（{} 项）",
                    generator_catalog::all_specs().len()
                ))
                .on_click(move |_, window, app| {
                    search.update(app, |panel, cx| panel.open_generator_search(id, window, cx));
                }),
            )
            .separator();
        for category in GeneratorCategory::ALL {
            let panel = panel.clone();
            let recommended = recommended.clone();
            let current = current.clone();
            menu = menu.submenu(category.label(), window, cx, move |sub, _window, _cx| {
                let mut sub = sub;
                for spec in generator_catalog::all_specs()
                    .iter()
                    .copied()
                    .filter(|s| s.category == category)
                {
                    let panel = panel.clone();
                    let name = spec.name;
                    sub = sub.item(
                        PopupMenuItem::new(generator_menu_label(name, spec.label, &recommended))
                            .checked(current == name)
                            .on_click(move |_, _, app| {
                                panel.update(app, |panel, cx| panel.set_generator(id, name, cx));
                            }),
                    );
                }
                sub
            });
        }
        menu
    }
}

/// 菜单项文案：推荐项加后缀（组件没有「右侧说明」的位置，标在标签里最省事）。
fn generator_menu_label(name: &str, label: &str, recommended: &str) -> String {
    // A2：语言敏感的生成器在菜单里标「随语言偏好」——值跟随面板的语言下拉，
    // 不保证所选语言都有母语数据（fake 对缺数据的组合回落英文）。
    let locale_note = if is_locale_sensitive(name) {
        " · 随语言偏好"
    } else {
        ""
    };
    if name == recommended {
        format!("{label}（推荐）{locale_note}")
    } else {
        format!("{label}{locale_note}")
    }
}

/// A2：语言敏感生成器的 spec 名（与 [`crate::generators`] 里 `locale_faker!` 的分支一一对应）。
///
/// 值取自 fake 的 locale 数据：姓名 / 电话 / 城市 / 街道 / 公司名等有母语数据，
/// 其余生成器（数值 / 时间 / UUID 等）与语言无关。
fn is_locale_sensitive(name: &str) -> bool {
    matches!(
        name,
        "name"
            | "first_name"
            | "last_name"
            | "phone_number"
            | "cell_number"
            | "city"
            | "street_name"
            | "company_name"
    )
}

