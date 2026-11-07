use crate::models::{
    ColumnDataType, ColumnDef, ColumnDependency, GeneratorConfig, ScenarioTemplate, TemplateTable,
};

macro_rules! col {
    ($name:expr, $data_type:expr, $gen:expr) => {
        ColumnDef {
            name: $name.to_string(),
            data_type: $data_type,
            generator: $gen,
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        }
    };
    ($name:expr, $data_type:expr, $gen:expr, nullable) => {
        ColumnDef {
            name: $name.to_string(),
            data_type: $data_type,
            generator: $gen,
            nullable_ratio: 0.3,
            unique: false,
            dependency: None,
        }
    };
    ($name:expr, $data_type:expr, $gen:expr, unique) => {
        ColumnDef {
            name: $name.to_string(),
            data_type: $data_type,
            generator: $gen,
            nullable_ratio: 0.0,
            unique: true,
            dependency: None,
        }
    };
}

// 模板数据子模块
mod data;
// 导入模板函数
use data::*;


pub fn get_builtin_templates() -> Vec<ScenarioTemplate> {
    vec![
        ecommerce_template(),
        hr_template(),
        blog_template(),
        finance_template(),
        social_media_template(),
        company_template(),
    ]
}

pub fn get_template_by_id(id: &str) -> Option<ScenarioTemplate> {
    get_builtin_templates().into_iter().find(|t| t.id == id)
}

/// 空白场景模板（工具栏「▦ 新增场景」的起点）：0 张表，从「＋ 加表」开始。
///
/// 不放进 `get_builtin_templates`（那不是用户可选模板，是「从零开始」的入口）；
/// 只在面板的「新增场景」动作里按需构造。
pub fn blank_scene_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:blank".to_string(),
        name: "空白场景".to_string(),
        description: "从 0 张表开始的自定义多表生成：加表 / 加关系后一次生成。".to_string(),
        category: "custom".to_string(),
        locale: "zh-CN".to_string(),
        tables: Vec::new(),
        seed: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::error::CoreError;

    #[test]
    fn test_all_templates_exist() {
        let templates = get_builtin_templates();
        assert_eq!(templates.len(), 6);
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"builtin:ecommerce"));
        assert!(ids.contains(&"builtin:hr"));
        assert!(ids.contains(&"builtin:blog"));
        assert!(ids.contains(&"builtin:finance"));
        assert!(ids.contains(&"builtin:social_media"));
        assert!(ids.contains(&"builtin:company"));
    }

    #[test]
    fn test_get_template_by_id_found() -> Result<(), CoreError> {
        let t = get_template_by_id("builtin:ecommerce")
            .ok_or_else(|| CoreError::from("template not found: builtin:ecommerce"))?;
        assert_eq!(t.name, "电商系统");
        assert!(!t.tables.is_empty());
        Ok(())
    }

    #[test]
    fn test_get_template_by_id_not_found() {
        assert!(get_template_by_id("nonexistent").is_none());
    }

    #[test]
    fn test_ecommerce_template_has_four_tables() -> Result<(), CoreError> {
        let t = get_template_by_id("builtin:ecommerce")
            .ok_or_else(|| CoreError::from("template not found: builtin:ecommerce"))?;
        assert_eq!(t.tables.len(), 4);
        let table_names: Vec<&str> = t.tables.iter().map(|tb| tb.name.as_str()).collect();
        assert!(table_names.contains(&"users"));
        assert!(table_names.contains(&"products"));
        assert!(table_names.contains(&"orders"));
        assert!(table_names.contains(&"order_items"));
        Ok(())
    }

    #[test]
    fn test_hr_template_has_tables() {
        let t = get_template_by_id("builtin:hr").unwrap();
        assert!(!t.tables.is_empty());
        assert_eq!(t.locale, "zh_cn");
    }

    #[test]
    fn test_all_template_columns_have_valid_types() {
        for template in get_builtin_templates() {
            for table in &template.tables {
                assert!(
                    !table.columns.is_empty(),
                    "{}::{} has no columns",
                    template.id,
                    table.name
                );
                for col in &table.columns {
                    assert!(
                        !col.name.is_empty(),
                        "{}.{} column has empty name",
                        table.name,
                        col.name
                    );
                    assert!(
                        col.nullable_ratio >= 0.0 && col.nullable_ratio <= 1.0,
                        "{}.{} nullable_ratio out of range: {}",
                        table.name,
                        col.name,
                        col.nullable_ratio
                    );
                }
            }
        }
    }

    // ==================== 表间引用（关系自检） ====================

    /// 名字里带 `_id` 但**故意不是引用**的列（改了就得同步改这里，并写清理由）。
    ///
    /// 这些列是「标识」而不是「指向另一张表的行」（如税号）。
    const NOT_A_REFERENCE: &[(&str, &str, &str)] = &[("builtin:company", "companies", "tax_id")];

    /// 模板里声明的引用都要能解析成取值域（父表 / 父列存在、父列可算域）。
    ///
    /// 这是「关系能生成」的前提：解析不了的引用在生成前就被拦下，
    /// 内置模板更不应该存在解析不了的引用。
    #[test]
    fn every_declared_reference_resolves() {
        for template in get_builtin_templates() {
            let domains = crate::engine::MockEngine::resolve_reference_domains(&template)
                .unwrap_or_else(|e| panic!("{} 的引用解析失败: {e}", template.id));
            let declared = declared_references(&template).len();
            assert!(
                !domains.is_empty() || declared == 0,
                "{} 声明了 {declared} 条引用，却一条域也没解析出来",
                template.id
            );
            for domain in &domains {
                assert!(domain.count > 0, "{domain:?} 的行数为 0");
            }
        }
    }

    /// 引用列的 `generator` 取值域必须**落在父域内**。
    ///
    /// 为何要管这个：场景生成走父域采样，但**单表生成没有父表上下文**，按它自己的
    /// `generator` 取值——那时产出的值也得落在父表可能的取值范围内，
    /// 否则同一列在两种生成方式下值域不同，用户换个入口就看到不一样的数据。
    #[test]
    fn reference_generators_stay_inside_the_parent_domain() {
        for template in get_builtin_templates() {
            let domains = crate::engine::MockEngine::resolve_reference_domains(&template)
                .expect("引用解析应当成功");
            for (child_table, column, parent_table, parent_column) in declared_references(&template)
            {
                let domain = domains
                    .iter()
                    .find(|d| d.table == parent_table && d.column == parent_column)
                    .unwrap_or_else(|| panic!("{} 的父域缺失", template.id));
                let GeneratorConfig::RandomInt { min, max } = column.generator else {
                    panic!(
                        "{}.{} 是引用列，生成器需是整数区间（单表生成时按它取值），实际: {:?}",
                        child_table, column.name, column.generator
                    );
                };
                assert!(
                    i64::from(min) >= domain.first && i64::from(max) <= domain.last(),
                    "{}.{} 的取值域 {}..{} 超出了父域 {}..{}（{}）",
                    child_table,
                    column.name,
                    min,
                    max,
                    domain.first,
                    domain.last(),
                    domain.label()
                );
            }
        }
    }

    /// 名字里带 `_id` 的列（除自带主键 `id`）**要么声明为引用，要么在白名单里**。
    ///
    /// 为何要这条：引用一旦漏声明，生成出来的就是「看着像外键、实际是随机数」的列，
    /// 关联查询会落空——这类问题只能在评审时看出来。让测试顶出来，
    /// 逼作者在「声明引用」与「明确写进白名单」之间选一个。
    #[test]
    fn every_id_column_is_declared_or_explicitly_not_a_reference() {
        let mut undeclared = Vec::new();
        for template in get_builtin_templates() {
            for table in &template.tables {
                for col in &table.columns {
                    if col.name == "id" || !col.name.ends_with("_id") || is_reference(col) {
                        continue;
                    }
                    let allowed = NOT_A_REFERENCE.iter().any(|(id, table_name, col_name)| {
                        *id == template.id && *table_name == table.name && *col_name == col.name
                    });
                    if !allowed {
                        undeclared.push(format!("{}.{}.{}", template.id, table.name, col.name));
                    }
                }
            }
        }
        assert!(
            undeclared.is_empty(),
            "这些 `*_id` 列既没声明引用、也不在 NOT_A_REFERENCE 白名单里：{undeclared:?}"
        );
    }

    /// 白名单里不能有幽灵条目（列已改名 / 已删 / 其实已经是引用）。
    #[test]
    fn the_not_a_reference_allowlist_has_no_stale_entries() {
        for (id, table_name, col_name) in NOT_A_REFERENCE {
            let template = get_template_by_id(id).unwrap_or_else(|| panic!("{id} 不存在"));
            let table = template
                .tables
                .iter()
                .find(|t| t.name == *table_name)
                .unwrap_or_else(|| panic!("{id} 里没有表 {table_name}"));
            let column = table
                .columns
                .iter()
                .find(|c| c.name == *col_name)
                .unwrap_or_else(|| panic!("{id}.{table_name} 里没有列 {col_name}"));
            assert!(
                !is_reference(column),
                "{id}.{table_name}.{col_name} 已声明为引用，应从白名单里删掉"
            );
        }
    }

    /// 列是否声明了跨表引用。
    fn is_reference(col: &ColumnDef) -> bool {
        col.dependency.is_some()
    }

    /// 模板里全部 `(子表, 列, 父表, 父列)` 引用声明。
    fn declared_references(template: &ScenarioTemplate) -> Vec<(&str, &ColumnDef, &str, &str)> {
        let mut out = Vec::new();
        for table in &template.tables {
            for col in &table.columns {
                let Some(dep) = col.dependency.as_ref() else {
                    continue;
                };
                out.push((
                    table.name.as_str(),
                    col,
                    dep.ref_table.as_str(),
                    dep.ref_column.as_str(),
                ));
            }
        }
        out
    }

    #[test]
    fn test_ecommerce_users_has_username_email() -> Result<(), CoreError> {
        let t = get_template_by_id("builtin:ecommerce")
            .ok_or_else(|| CoreError::from("template not found: builtin:ecommerce"))?;
        let users = t
            .tables
            .iter()
            .find(|tb| tb.name == "users")
            .ok_or_else(|| CoreError::from("table 'users' not found"))?;
        let names: Vec<&str> = users.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"username"));
        assert!(names.contains(&"email"));
        Ok(())
    }
}
