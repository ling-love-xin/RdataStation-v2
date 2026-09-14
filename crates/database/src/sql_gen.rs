//! rds-database — 数据源导航「生成 SQL」：由列信息生成 INSERT / UPDATE / DELETE 模板。
//!
//! 纯函数（无 I/O、无方言依赖），便于单测；标识符不加引号（跨方言中性），
//! 值统一用 `NULL` 占位，生成后由用户在编辑区替换。
//!
//! 设计对应 `docs/architecture/database/database-navigator-prototype-design.md` §6.2
//! （表 / 视图右键「生成 INSERT/UPDATE/DELETE」）。

/// 生成模板所需的列信息（只用名称与主键标记）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmlColumn {
    pub name: String,
    pub primary: bool,
}

/// 生成的 DML 种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmlKind {
    Insert,
    Update,
    Delete,
}

impl DmlKind {
    /// 菜单项 / 提示文案。
    pub fn label(self) -> &'static str {
        match self {
            Self::Insert => "生成 INSERT",
            Self::Update => "生成 UPDATE",
            Self::Delete => "生成 DELETE",
        }
    }
}

/// 限定名（`catalog.schema.name`，跳过空段、相同段去重）。
///
/// 无独立 Schema 层的驱动（MySQL / SQLite / DuckDB）导航把 schema 传成 catalog，
/// 去重后避免出现 `db.db.name`。
pub fn qualified_name(catalog: Option<&str>, schema: Option<&str>, name: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(c) = catalog {
        if !c.is_empty() {
            parts.push(c);
        }
    }
    if let Some(s) = schema {
        if !s.is_empty() && Some(s) != catalog {
            parts.push(s);
        }
    }
    parts.push(name);
    parts.join(".")
}

/// 主键列的 `WHERE` 条件；无主键时退化为恒假（避免生成会误伤全表的语句）。
fn where_expr(keys: &[&str]) -> String {
    if keys.is_empty() {
        "1 = 0".to_string()
    } else {
        keys.iter()
            .map(|k| format!("{k} = NULL"))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

/// 生成 DML 模板。
///
/// - `INSERT`：全列 + `NULL` 占位；
/// - `UPDATE`：`SET` 列出**非主键列**（全是主键时退回全列），`WHERE` 用主键；
/// - `DELETE`：`WHERE` 用主键；
/// - 无主键：`WHERE 1 = 0` + 注释提示；
/// - 列信息为空：只给一行注释（不生成半个语句）。
pub fn dml_template(qualified: &str, columns: &[DmlColumn], kind: DmlKind) -> String {
    if columns.is_empty() {
        return format!("-- {qualified}：未加载到列信息，无法{}", kind.label());
    }

    let all: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    let keys: Vec<&str> = columns
        .iter()
        .filter(|c| c.primary)
        .map(|c| c.name.as_str())
        .collect();
    let non_keys: Vec<&str> = columns
        .iter()
        .filter(|c| !c.primary)
        .map(|c| c.name.as_str())
        .collect();

    let mut out = vec![format!(
        "-- 由数据源导航生成 · {}（{} 列）",
        qualified,
        columns.len()
    )];
    if keys.is_empty() {
        out.push("-- 注意：未检测到主键，WHERE 已退化为恒假，请自行补全过滤条件".to_string());
    }

    match kind {
        DmlKind::Insert => {
            let placeholders = vec!["NULL"; all.len()].join(", ");
            out.push(format!("INSERT INTO {qualified} ({})", all.join(", ")));
            out.push(format!("VALUES ({placeholders});"));
        }
        DmlKind::Update => {
            let set_cols = if non_keys.is_empty() { all } else { non_keys };
            let sets = set_cols
                .iter()
                .map(|c| format!("{c} = NULL"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!("UPDATE {qualified}"));
            out.push(format!("   SET {sets}"));
            out.push(format!(" WHERE {};", where_expr(&keys)));
        }
        DmlKind::Delete => {
            out.push(format!("DELETE FROM {qualified}"));
            out.push(format!(" WHERE {};", where_expr(&keys)));
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(spec: &[(&str, bool)]) -> Vec<DmlColumn> {
        spec.iter()
            .map(|(name, primary)| DmlColumn {
                name: name.to_string(),
                primary: *primary,
            })
            .collect()
    }

    #[test]
    fn qualified_name_dedupes_repeated_segment() {
        assert_eq!(
            qualified_name(Some("mall"), Some("mall"), "user"),
            "mall.user"
        );
        assert_eq!(
            qualified_name(Some("postgres"), Some("public"), "t"),
            "postgres.public.t"
        );
        assert_eq!(qualified_name(None, Some("main"), "t"), "main.t");
        assert_eq!(qualified_name(Some("main"), None, "t"), "main.t");
        assert_eq!(qualified_name(None, None, "t"), "t");
    }

    #[test]
    fn insert_lists_all_columns_with_null_placeholders() {
        let sql = dml_template(
            "db.t",
            &cols(&[("id", true), ("name", false)]),
            DmlKind::Insert,
        );
        assert!(sql.contains("INSERT INTO db.t (id, name)"), "{sql}");
        assert!(sql.contains("VALUES (NULL, NULL);"), "{sql}");
    }

    #[test]
    fn update_sets_non_key_columns_and_filters_by_key() {
        let sql = dml_template(
            "db.t",
            &cols(&[("id", true), ("name", false), ("age", false)]),
            DmlKind::Update,
        );
        assert!(sql.contains("UPDATE db.t"), "{sql}");
        assert!(sql.contains("SET name = NULL, age = NULL"), "{sql}");
        assert!(sql.contains("WHERE id = NULL;"), "{sql}");
    }

    #[test]
    fn delete_filters_by_key() {
        let sql = dml_template(
            "db.t",
            &cols(&[("id", true), ("name", false)]),
            DmlKind::Delete,
        );
        assert!(sql.contains("DELETE FROM db.t"), "{sql}");
        assert!(sql.contains("WHERE id = NULL;"), "{sql}");
    }

    #[test]
    fn missing_primary_key_falls_back_to_false_predicate() {
        let sql = dml_template(
            "db.t",
            &cols(&[("a", false), ("b", false)]),
            DmlKind::Delete,
        );
        assert!(sql.contains("WHERE 1 = 0;"), "{sql}");
        assert!(sql.contains("未检测到主键"), "{sql}");
    }

    #[test]
    fn composite_primary_key_joins_with_and() {
        let sql = dml_template(
            "db.t",
            &cols(&[("a", true), ("b", true), ("v", false)]),
            DmlKind::Delete,
        );
        assert!(sql.contains("WHERE a = NULL AND b = NULL;"), "{sql}");
        // 全主键的 UPDATE 退回全列，避免出现空的 SET。
        let upd = dml_template("db.t", &cols(&[("a", true), ("b", true)]), DmlKind::Update);
        assert!(upd.contains("SET a = NULL, b = NULL"), "{upd}");
    }

    #[test]
    fn empty_columns_yields_comment_only() {
        let sql = dml_template("db.t", &[], DmlKind::Insert);
        assert!(sql.starts_with("-- "), "{sql}");
        assert!(!sql.contains("INSERT INTO"), "{sql}");
    }
}
