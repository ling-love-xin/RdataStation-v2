//! 「引用」页的数据模型与纯函数（对齐 DBeaver `ReferencesResultsContainer`）。
//!
//! 取数在宿主侧（`MetadataService::list_constraints`，后台任务里跑）；本模块只做
//! **约束 → 引用键**的纯映射与文案，测试直接钉这里，不碰 UI。
//!
//! 与 DBeaver 的口径对应：
//! - 只收**出向外键**（本表列 → 目标表列）。DBeaver 还有入向（`getAllReferences`），
//!   那要扫 schema 内其它表的约束（N 次往返），本轮不做，见
//!   `docs/architecture/editor/result-panel-references-plan.md`。
//! - 收键的过滤条件是 DBeaver `ReferencesResultsContainer.java:298` 那条
//!   「该关联引用的属性集合 ⊆ 结果集可见属性」。
//! - 键文案同 `RefKeyLabelProvider.getText`（`:516-527`）：`目标表 (约束名)`。

use shared::meta::ConstraintDetail;

/// 一条引用键：本表的一组外键列引向目标表的一组列。
///
/// 对齐 DBeaver `ReferenceKey` 里出向那一半的字段（`isReference` 恒 false 故不设该字段；
/// `targetEntity` / `refAssociation` 在这里落成 `table` / `constraint`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceKey {
    /// 目标表名（驱动口径；同 schema 时通常不带 schema 前缀）
    pub table: String,
    /// 约束名；驱动没给名字时为空串（文案退回只显示目标表）
    pub constraint: String,
    /// 本表参与该外键的列
    pub columns: Vec<String>,
    /// 目标表被引用的列（驱动没报时为空数组）
    pub target_columns: Vec<String>,
}

/// 「引用」页的清单状态（render 只读本状态，取数不在 render 里发生）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ReferencesState {
    /// 还没读过（页签从未被激活，或结果集刚换过）
    #[default]
    NotLoaded,
    /// 宿主已发起取数
    Loading,
    /// 读到了（空数组 = 「无引用」）
    Loaded(Vec<ReferenceKey>),
    /// 读失败（宿主未绑定连接这类本仓自身原因；驱动不支持外键时按 DBeaver
    /// 的口径静默成 `Loaded([])`，见 `docs/architecture/editor/result-panel-references-plan.md`）
    Failed(String),
}

impl ReferencesState {
    /// 清单为空时的文案：**照 DBeaver** `ResultSetMessages.refs_no_refs_text`（「无引用」）。
    pub const NO_REFERENCES: &'static str = "无引用";

    /// 取数中的文案
    pub const LOADING: &'static str = "正在读取外键…";
}

/// 约束类型是否外键。
///
/// 与 `crates/database/src/er/model.rs::is_foreign_key_constraint`（`:251-254`）同一口径：
/// 各家驱动字符串不一（多为 `FOREIGN KEY`，少数简写 `FK`），
/// 但都不该在这里各自解释一遍。
fn is_foreign_key_constraint(constraint_type: &str) -> bool {
    let t = constraint_type.trim().to_ascii_uppercase();
    t.contains("FOREIGN") || t == "FK"
}

/// 从表约束里挑出引用键（纯函数）。
///
/// 三条过滤（任一不满足就跳过）：
/// 1. 类型是外键；
/// 2. 驱动报了目标表（`referenced_table` 非空）——只知道「有外键」、拿不到目标的
///    桥接驱动给不出可展示的键（与 `ColumnDetail::is_foreign_key` 的取舍同源）；
/// 3. 该外键的**全部**本表列都在结果集可见列里（DBeaver `:298` 同一条规则）；
///    列名按**忽略大小写**比对（约束来自驱动、可见列来自结果集，两边大小写不保证一致）。
///
/// 结果按下标顺序排一遍（目标表 → 约束名 → 本表列）：DBeaver 按实体遍历序、不排序，
/// 这里排序是为了同一份数据每次渲染顺序稳定（否则面板会随加载批次跳动）。
pub fn keys_from_constraints(constraints: &[ConstraintDetail], visible: &[String]) -> Vec<ReferenceKey> {
    let mut keys: Vec<ReferenceKey> = Vec::new();
    for constraint in constraints {
        if !is_foreign_key_constraint(&constraint.constraint_type) {
            continue;
        }
        let Some(target) = constraint
            .referenced_table
            .as_deref()
            .map(str::trim)
            .filter(|table| !table.is_empty())
        else {
            continue;
        };
        if constraint.column_names.is_empty() {
            continue;
        }
        if !constraint
            .column_names
            .iter()
            .all(|column| visible.iter().any(|visible| visible.eq_ignore_ascii_case(column)))
        {
            continue;
        }
        keys.push(ReferenceKey {
            table: target.to_string(),
            constraint: constraint.name.trim().to_string(),
            columns: constraint.column_names.clone(),
            target_columns: constraint.referenced_columns.clone(),
        });
    }
    keys.sort_by(|a, b| {
        a.table
            .cmp(&b.table)
            .then_with(|| a.constraint.cmp(&b.constraint))
            .then_with(|| a.columns.cmp(&b.columns))
    });
    keys
}

/// 引用键文案（DBeaver `RefKeyLabelProvider.getText`：`目标表 (关联名)`）。
pub fn key_label(key: &ReferenceKey) -> String {
    if key.constraint.is_empty() {
        key.table.clone()
    } else {
        format!("{} ({})", key.table, key.constraint)
    }
}

/// 列映射文案：`本表列 → 目标表.目标列`（驱动没报目标列时只到表名）。
///
/// 这一行是本仓对 DBeaver「引用行数据」的**如实替代**：目标列拿不到就不编列名。
pub fn mapping_text(key: &ReferenceKey) -> String {
    if key.target_columns.is_empty() || key.target_columns.len() != key.columns.len() {
        return format!("{} → {}", key.columns.join(", "), key.table);
    }
    key.columns
        .iter()
        .zip(key.target_columns.iter())
        .map(|(column, target)| format!("{column} → {}.{target}", key.table))
        .collect::<Vec<_>>()
        .join("，")
}

/// 一条引用行查询的原料：本表列 + 当前行该列的值 + 驱动报的类型名。
///
/// 值是结果集里的**原始文本**（驱动返回的口径，不是渲染后的`—`）；
/// `data_type` 拿不到时按字符串处理（宁可多一层引号，也不要让数字列被当成文本
/// 或反过来把文本列裸拼进 SQL）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceValue {
    /// 本表列名（与 [`ReferenceKey::columns`] 同序对应）
    pub column: String,
    /// 该列的原始值文本
    pub value: String,
    /// 驱动报的类型名（`ColumnAnnotation::data_type`；`None` = 拿不到）
    pub data_type: Option<String>,
}

/// 值是否「看起来是 NULL」。驱动与各库的文本口径不一（`NULL` / `null` / 空串），
/// 三种都按 NULL 处理：外键列上的空值确实表示「没有引用」。
fn is_null_text(value: &str) -> bool {
    let v = value.trim();
    v.is_empty() || v.eq_ignore_ascii_case("null")
}

/// 类型名是否数值族（可以裸拼，不加引号）。
///
/// 只认驱动类型名里的关键片段：数值列若被包成字符串，PostgreSQL / SQL Server 会直接
/// 报「operator does not exist: integer = text」——这不是用户能自己绕过去的错。
fn is_numeric_type(data_type: &str) -> bool {
    let t = data_type.to_ascii_uppercase();
    [
        "INT", "SERIAL", "NUM", "DEC", "MONEY", "FLOAT", "REAL", "DOUBLE", "NUMBER",
    ]
    .iter()
    .any(|key| t.contains(key))
}

/// 值是否是一个裸数值字面量（数值列上宁可退回加引号，也不把 `abc` 裸拼进 SQL）。
fn looks_numeric(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return false;
    }
    let mut chars = v.chars().peekable();
    if matches!(chars.peek(), Some('+') | Some('-')) {
        chars.next();
    }
    let mut digits = 0usize;
    let mut dots = 0usize;
    for ch in chars {
        match ch {
            '0'..='9' => digits += 1,
            '.' if dots == 0 => dots += 1,
            _ => return false,
        }
    }
    digits > 0
}

/// 字符串字面量：单引号按 SQL 标准**双写**转义（`O'Brien` → `O''Brien`）。
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 一个值 → SQL 字面量（纯函数，可单独钉）。
///
/// 三档口径：
/// 1. NULL（空串 / `NULL`）→ `NULL`（调用方负责拼 `IS NULL`，`= NULL` 永远不成立）；
/// 2. 数值列 + 值确实是数字 → 裸拼；数值列但值不是数字 → 加引号（不静默产出坏 SQL）；
/// 3. 其余一律加引号（含布尔、日期：这两族各库字面量写法差异大，引号版本在
///    PostgreSQL / MySQL / SQL Server / SQLite 上都能隐式转换）。
pub fn sql_literal(value: &str, data_type: Option<&str>) -> String {
    if is_null_text(value) {
        return "NULL".to_string();
    }
    let raw = value.trim();
    match data_type.map(str::trim).filter(|t| !t.is_empty()) {
        Some(kind) if is_numeric_type(kind) && looks_numeric(raw) => raw.to_string(),
        _ => quote(raw),
    }
}

/// 生成「引用行」查询：`SELECT * FROM 目标表 WHERE 目标列 = 值 [AND …]`。
///
/// 对齐 DBeaver `ReferencesResultsContainer.refreshKeyValues`（`:382-432`）的语义：
/// 按**当前行的值**去查目标表。本仓走 `B′`（把这条 SQL 交给编辑器**另开一份结果集**），
/// 而不是在面板里嵌套第二份结果视图——理由见 `result-panel-references-plan.md` §5。
///
/// 返回 `None` 的情形（都不编半条 SQL）：
/// - 目标表为空；
/// - 目标列没报 / 与参与列数量对不上（不知道往哪查）；
/// - 值列表与参与列数量对不上（调用方少给了某一列的值）。
pub fn reference_rows_sql(key: &ReferenceKey, values: &[ReferenceValue]) -> Option<String> {
    let table = key.table.trim();
    if table.is_empty() {
        return None;
    }
    if key.target_columns.is_empty() || key.target_columns.len() != key.columns.len() {
        return None;
    }
    if values.len() != key.columns.len() {
        return None;
    }
    let mut clauses: Vec<String> = Vec::with_capacity(values.len());
    for (target, value) in key.target_columns.iter().zip(values.iter()) {
        if value.column.is_empty() {
            return None;
        }
        let literal = sql_literal(&value.value, value.data_type.as_deref());
        if literal == "NULL" {
            clauses.push(format!("{target} IS NULL"));
        } else {
            clauses.push(format!("{target} = {literal}"));
        }
    }
    Some(format!("SELECT * FROM {table} WHERE {}", clauses.join(" AND ")))
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        ReferenceKey, ReferenceValue, ReferencesState, key_label, keys_from_constraints,
        mapping_text, reference_rows_sql, sql_literal,
    };
    use shared::meta::ConstraintDetail;

    fn constraint(
        name: &str,
        kind: &str,
        columns: &[&str],
        target: Option<&str>,
        target_columns: &[&str],
    ) -> ConstraintDetail {
        ConstraintDetail {
            name: name.to_string(),
            table_name: "orders".to_string(),
            constraint_type: kind.to_string(),
            column_names: columns.iter().map(|c| c.to_string()).collect(),
            referenced_table: target.map(|t| t.to_string()),
            referenced_columns: target_columns.iter().map(|c| c.to_string()).collect(),
            update_rule: None,
            delete_rule: None,
        }
    }

    #[test]
    fn keys_keep_only_foreign_keys_whose_columns_are_all_visible() {
        let visible: Vec<String> = ["id", "customer_id"].iter().map(|c| c.to_string()).collect();
        let constraints = vec![
            // 合格：外键 + 目标表 + 列都在结果里
            constraint("fk_customer", "FOREIGN KEY", &["customer_id"], Some("customers"), &["id"]),
            // 主键：不是外键
            constraint("pk_orders", "PRIMARY KEY", &["id"], None, &[]),
            // 外键但驱动没报目标表：给不出可展示的键
            constraint("fk_unknown", "FOREIGN KEY", &["customer_id"], None, &[]),
            // 外键但引用了结果里没有的列：DBeaver 的过滤条件不过（`:298`）
            constraint("fk_shipping", "FOREIGN KEY", &["ship_address_id"], Some("addresses"), &["id"]),
        ];
        let keys = keys_from_constraints(&constraints, &visible);
        assert_eq!(keys.len(), 1, "只应留下 customer_id 那一条：{keys:?}");
        assert_eq!(keys[0].table, "customers");
        assert_eq!(keys[0].constraint, "fk_customer");
        assert_eq!(keys[0].columns, vec!["customer_id".to_string()]);
        assert_eq!(keys[0].target_columns, vec!["id".to_string()]);
    }

    #[test]
    fn keys_match_visible_columns_case_insensitively() {
        // 约束来自驱动（常是库里的大小写），可见列来自结果集（连接层可能已规整）
        let visible = vec!["CUSTOMER_ID".to_string()];
        let constraints = vec![constraint(
            "fk_customer",
            "FK",
            &["customer_id"],
            Some("customers"),
            &["id"],
        )];
        let keys = keys_from_constraints(&constraints, &visible);
        assert_eq!(keys.len(), 1, "大小写不一致不该把键丢掉：{keys:?}");
    }

    #[test]
    fn keys_are_ordered_stably_for_the_ui() {
        let visible: Vec<String> = ["a_id", "b_id"].iter().map(|c| c.to_string()).collect();
        let constraints = vec![
            constraint("fk_b", "FOREIGN KEY", &["b_id"], Some("zebra"), &["id"]),
            constraint("fk_a", "FOREIGN KEY", &["a_id"], Some("alpha"), &["id"]),
        ];
        let keys = keys_from_constraints(&constraints, &visible);
        assert_eq!(keys[0].table, "alpha", "按目标表名排序：{keys:?}");
        assert_eq!(keys[1].table, "zebra", "按目标表名排序：{keys:?}");
    }

    #[test]
    fn empty_input_yields_no_keys() {
        assert!(keys_from_constraints(&[], &["id".to_string()]).is_empty());
        let constraints = vec![constraint("fk", "FOREIGN KEY", &["id"], Some("t"), &["id"])];
        assert!(
            keys_from_constraints(&constraints, &[]).is_empty(),
            "没有可见列时任何外键都不该收（DBeaver 同口径）"
        );
    }

    #[test]
    fn key_label_matches_dbeaver_shape() {
        let key = ReferenceKey {
            table: "customers".to_string(),
            constraint: "fk_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            target_columns: vec!["id".to_string()],
        };
        assert_eq!(key_label(&key), "customers (fk_customer)");

        let unnamed = ReferenceKey {
            constraint: String::new(),
            ..key
        };
        assert_eq!(key_label(&unnamed), "customers", "没约束名时退回只显示目标表");
    }

    #[test]
    fn mapping_text_names_columns_only_when_the_driver_reported_them() {
        let key = ReferenceKey {
            table: "customers".to_string(),
            constraint: "fk_customer".to_string(),
            columns: vec!["customer_id".to_string(), "region_id".to_string()],
            target_columns: vec!["id".to_string(), "region_id".to_string()],
        };
        assert_eq!(
            mapping_text(&key),
            "customer_id → customers.id，region_id → customers.region_id"
        );

        // 驱动没报目标列 / 列数对不上：只到表名，不编列名
        let partial = ReferenceKey {
            target_columns: vec!["id".to_string()],
            ..key
        };
        assert_eq!(mapping_text(&partial), "customer_id, region_id → customers");
    }

    #[test]
    fn states_start_unloaded_and_share_dbeaver_empty_text() {
        assert_eq!(ReferencesState::default(), ReferencesState::NotLoaded);
        assert_eq!(ReferencesState::NO_REFERENCES, "无引用");
    }

    fn value(column: &str, value: &str, data_type: Option<&str>) -> ReferenceValue {
        ReferenceValue {
            column: column.to_string(),
            value: value.to_string(),
            data_type: data_type.map(|t| t.to_string()),
        }
    }

    /// 值 → 字面量：NULL 认三种文本；数值列裸拼、数值列上的非数字退回加引号；
    /// 其余（布尔 / 日期 / 文本）统一加引号；单引号按标准双写。
    #[test]
    fn sql_literals_are_typed_and_escaped() {
        assert_eq!(sql_literal("NULL", None), "NULL");
        assert_eq!(sql_literal("null", None), "NULL");
        assert_eq!(sql_literal("", Some("integer")), "NULL");

        assert_eq!(sql_literal("42", Some("INTEGER")), "42", "数值列裸拼");
        assert_eq!(sql_literal("-3.5", Some("numeric(10,2)")), "-3.5");
        assert_eq!(
            sql_literal("abc", Some("INTEGER")),
            "'abc'",
            "数值列上不是数字的值不能裸拼（会产出坏 SQL）"
        );

        assert_eq!(sql_literal("true", Some("boolean")), "'true'", "布尔按字符串，各库都能隐式转换");
        assert_eq!(sql_literal("2026-11-18", Some("date")), "'2026-11-18'");
        assert_eq!(sql_literal("O'Brien", None), "'O''Brien'", "单引号双写");
        assert_eq!(sql_literal(" 7 ", Some("bigint")), "7", "裸拼时去掉两端空白");
    }

    /// 引用行 SQL：单列 / 多列 / NULL 走 `IS NULL` / 缺目标列或值数量不符时拒绝生成。
    #[test]
    fn reference_rows_sql_covers_the_three_shapes() {
        let key = ReferenceKey {
            table: "customers".to_string(),
            constraint: "fk_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            target_columns: vec!["id".to_string()],
        };
        let sql = reference_rows_sql(&key, &[value("customer_id", "7", Some("bigint"))])
            .expect("一列外键应能生成 SQL");
        assert_eq!(sql, "SELECT * FROM customers WHERE id = 7");

        let composite = ReferenceKey {
            columns: vec!["region_id".to_string(), "code".to_string()],
            target_columns: vec!["region_id".to_string(), "code".to_string()],
            ..key.clone()
        };
        let sql = reference_rows_sql(
            &composite,
            &[
                value("region_id", "3", Some("int")),
                value("code", "A1", Some("varchar")),
            ],
        )
        .expect("复合外键应能生成 SQL");
        assert_eq!(
            sql,
            "SELECT * FROM customers WHERE region_id = 3 AND code = 'A1'"
        );

        // 空值：`= NULL` 永远不成立，必须写 IS NULL
        let sql = reference_rows_sql(&key, &[value("customer_id", "NULL", Some("bigint"))])
            .expect("空值也能生成（IS NULL 分支）");
        assert_eq!(sql, "SELECT * FROM customers WHERE id IS NULL");

        // 驱动没报目标列：不知道往哪查，不编
        let unknown = ReferenceKey {
            target_columns: Vec::new(),
            ..key.clone()
        };
        assert_eq!(reference_rows_sql(&unknown, &[value("customer_id", "7", None)]), None);

        // 值数量与参与列不符（调用方少取了一列）：宁可不生成
        assert_eq!(
            reference_rows_sql(&composite, &[value("region_id", "3", Some("int"))]),
            None,
            "少给一列的值时不该拼出半条条件"
        );

        // 目标表为空
        let no_table = ReferenceKey {
            table: String::new(),
            ..key
        };
        assert_eq!(reference_rows_sql(&no_table, &[value("customer_id", "7", None)]), None);
    }
}
