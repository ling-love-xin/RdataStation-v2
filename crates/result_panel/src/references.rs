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

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        ReferenceKey, ReferencesState, key_label, keys_from_constraints, mapping_text,
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
}
