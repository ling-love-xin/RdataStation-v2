//! 洞察模型 · 关联规则与多列视图（自 model.rs 拆分，纯位移）

use super::*;
// ==================== 多列分析视图模型（Phase 3.2 / 3.3） ====================

/// 「多列」Tab 的一条候选规则。
///
/// 列清单与规则清单都是**数据**（从服务层来），选中的列/规则是**面板状态**
/// （住 `InsightView`），所以这里只放展示与判定需要的东西。
#[derive(Debug, Clone, PartialEq)]
pub struct MultiRuleView {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 期望的列类型族（`applies_to`），长度 = 需要的列数
    pub applies_to: Vec<String>,
    /// 顺序参数（除 `table` 外）：`col1` / `col2` …
    pub column_params: Vec<String>,
    /// `single`（默认）/ `list`
    pub result_type: Option<String>,
}

impl MultiRuleView {
    /// 从 `list_insight_rules` 的 JSON 转视图模型（缺关键字段则视为不可用）。
    ///
    /// 服务层返回的是 JSON（v1 起就如此，供前端直接吃），转换放在这里：
    /// 类型化后上层就不必到处 `["name"].as_str().unwrap_or(...)`。
    pub fn from_rule_json(value: &serde_json::Value) -> Option<Self> {
        let id = value.get("id")?.as_str()?.to_string();
        let column_params: Vec<String> = value
            .get("parameters")
            .and_then(|p| p.as_array())
            .map(|params| {
                params
                    .iter()
                    .filter_map(|p| p.as_str())
                    // `table` 由面板自己填（恒为当前临时表），不占用户的列位
                    .filter(|p| *p != "table")
                    .map(|p| p.to_string())
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            id,
            name: value.get("name")?.as_str()?.to_string(),
            description: value
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or_default()
                .to_string(),
            applies_to: value
                .get("applies_to")
                .and_then(|a| a.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|t| t.as_str())
                        .map(|t| t.to_string())
                        .collect()
                })
                .unwrap_or_default(),
            column_params,
            result_type: value
                .get("result_type")
                .and_then(|r| r.as_str())
                .map(|r| r.to_string()),
        })
    }

    /// 该规则需要几列
    pub fn arity(&self) -> usize {
        self.column_params.len()
    }

    /// 选中的列是否满足规则的类型要求。
    ///
    /// `Any` 放行；`applies_to` 比参数少时不补全（按位比较，缺位视为不限制）。
    /// 不做「类型不符就报错」——SQL 自己会拒，界面上先把不可用的选法标出来就够了。
    pub fn accepts(&self, kinds: &[ColumnKind]) -> bool {
        if kinds.len() != self.arity() {
            return false;
        }
        kinds
            .iter()
            .enumerate()
            .all(|(ix, kind)| match self.applies_to.get(ix) {
                None => true,
                Some(expected) => {
                    expected.eq_ignore_ascii_case("any")
                        || expected.eq_ignore_ascii_case(kind.family_name())
                }
            })
    }

    /// 列表里那一行右侧的类型提示（如 `数值 · 数值`）
    pub fn types_hint(&self) -> String {
        if self.applies_to.is_empty() {
            return "通用".to_string();
        }
        self.applies_to.join(" · ")
    }
}

/// 多列规则的结果（两种形态：单值键值行 / 列表表格）
#[derive(Debug, Clone, PartialEq)]
pub enum MultiResultView {
    /// 一行一个字段（如相关系数 / 协方差 / 样本量）
    Single(Vec<KeyValueRow>),
    /// 表格（如交叉频次表）
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

/// 键值行（字段名来自规则输出，是**动态字符串**，所以不复用 `StatRow`——
/// 后者的 `label` 是 `&'static str`，专给固定词汇的统计行）
#[derive(Debug, Clone, PartialEq)]
pub struct KeyValueRow {
    pub label: String,
    pub value: String,
}

impl MultiResultView {
    /// 从领域执行结果转视图模型（按**数据形态**分派，不看 `result_type`）。
    ///
    /// 为什么按形态：`result_type` 是规则的声明，而真实返回才是事实——
    /// 两者不一致时按事实渲染，不会出现「声明 list 却只拿到一个数」时的空白表。
    pub fn from_execution(result: &ExecutionResult) -> Self {
        match &result.data {
            serde_json::Value::Array(items) => Self::table_from_items(items),
            serde_json::Value::Object(map) => Self::Single(
                map.iter()
                    .map(|(key, value)| KeyValueRow {
                        label: field_label(key).to_string(),
                        value: json_cell(value),
                    })
                    .collect(),
            ),
            other => Self::Single(vec![KeyValueRow {
                label: "结果".to_string(),
                value: json_cell(other),
            }]),
        }
    }

    /// 列表结果 → 表格：表头按**首次出现顺序**收集（各行键不一致时不丢列）
    fn table_from_items(items: &[serde_json::Value]) -> Self {
        let mut headers: Vec<String> = Vec::new();
        for item in items {
            if let Some(map) = item.as_object() {
                for key in map.keys() {
                    if !headers.contains(key) {
                        headers.push(key.clone());
                    }
                }
            }
        }
        if headers.is_empty() {
            // 空结果：给空表格（视图据此说「没有数据」），而不是假造一行
            return Self::Table {
                headers: Vec::new(),
                rows: Vec::new(),
            };
        }
        let rows = items
            .iter()
            .map(|item| {
                headers
                    .iter()
                    .map(|h| {
                        item.get(h)
                            .map(json_cell)
                            .unwrap_or_else(|| "—".to_string())
                    })
                    .collect()
            })
            .collect();
        Self::Table {
            headers: headers.iter().map(|h| field_label(h).to_string()).collect(),
            rows,
        }
    }

    /// 空结果（没跑过 / 规则未返回行）
    pub fn is_empty(&self) -> bool {
        match self {
            MultiResultView::Single(rows) => rows.is_empty(),
            MultiResultView::Table { rows, .. } => rows.is_empty(),
        }
    }
}

/// 输出字段名 → 中文标签（内置规则用到的那些）。
///
/// 只映射**已知的**字段名：规则的 `json_name` 是对外契约，认不出来的就原样展示——
/// 猜一个中文名比露个英文名更坏（用户对着不确定的词更没法搜）。
/// 将来若把中文名写进规则 schema（`[[output]] label`），这里就该退休。
pub fn field_label(json_name: &str) -> &str {
    match json_name {
        "correlation" => "相关系数",
        "covariance" => "协方差",
        "regression_slope" => "回归斜率",
        "regression_intercept" => "回归截距",
        "sample_size" => "样本量",
        "row_value" => "行",
        "col_value" => "列",
        "count" => "计数",
        other => other,
    }
}

/// JSON 单元格 → 展示文案（数值仍走 `fmt_num`：`1.5` 不写成 `1.5000`）
pub fn json_cell(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "—".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.as_f64().map(fmt_num).unwrap_or_else(|| n.to_string()),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 质量门控 → 提示行（这一波多列规则自带 `quality` 时才有）
///
/// 通过的检查不占行：提示区只用来说「哪里不对」，全通过时是空的。
pub fn quality_notes(report: Option<&QualityReport>) -> Vec<QualityNote> {
    let Some(report) = report else {
        return Vec::new();
    };
    report
        .checks
        .iter()
        .filter(|check| !check.passed)
        .map(|check| QualityNote {
            level: NoteLevel::Warning,
            text: match &check.message {
                message if !message.trim().is_empty() => message.clone(),
                _ => match check.actual {
                    Some(actual) => format!(
                        "{} 未过 {}（实际 {}）",
                        check.field,
                        check.rule,
                        fmt_num(actual)
                    ),
                    None => format!("{} 未过 {}", check.field, check.rule),
                },
            },
        })
        .collect()
}

/// 「多列」Tab 的数据：列清单 + 候选规则 + 最近一次结果
#[derive(Debug, Clone, PartialEq)]
pub struct MultiColumnView {
    /// 临时表名（**数据来源**：面板据此判断这份数据是否属于当前目标）
    pub temp_table: String,
    /// 逻辑表名（展示用）
    pub table_name: String,
    /// 临时表的**真实**列元数据（v1 的 `availableColumns` 恒空是该项从未跑通的根因）
    pub columns: Vec<TableColumnView>,
    /// `category = multi` 的规则
    pub rules: Vec<MultiRuleView>,
    /// 最近一次执行结果（没跑过为 `None`）
    pub result: Option<MultiResultView>,
    /// 最近一次执行的质量门控提示
    pub notes: Vec<QualityNote>,
}

impl MultiColumnView {
    /// 空视图（面板初始态：没取过数）
    pub fn empty() -> Self {
        Self {
            temp_table: String::new(),
            table_name: String::new(),
            columns: Vec::new(),
            rules: Vec::new(),
            result: None,
            notes: Vec::new(),
        }
    }

    /// 这份数据是不是当前这个临时表的（是就不必重新取数）
    pub fn is_for(&self, temp_table: &str) -> bool {
        !self.temp_table.is_empty() && self.temp_table == temp_table
    }

    pub fn from_profile(
        profile: &TableProfile,
        table_name: &str,
        rules: Vec<MultiRuleView>,
    ) -> Self {
        let table = TableProfileView::from_profile(profile, table_name);
        Self {
            temp_table: profile.table_name.clone(),
            table_name: table.table_name,
            columns: table.columns,
            rules,
            result: None,
            notes: Vec::new(),
        }
    }

    pub fn column(&self, name: &str) -> Option<&TableColumnView> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// 选中的列（按给定顺序）对应的类型族：供 [`MultiRuleView::accepts`] 判定
    pub fn kinds_of(&self, selected: &[String]) -> Vec<ColumnKind> {
        selected
            .iter()
            .filter_map(|name| self.column(name).map(|c| c.kind))
            .collect()
    }

    /// 写下一次执行结果（保留列清单与规则清单）
    pub fn with_result(&self, result: MultiResultView, notes: Vec<QualityNote>) -> Self {
        let mut next = self.clone();
        next.result = Some(result);
        next.notes = notes;
        next
    }
}