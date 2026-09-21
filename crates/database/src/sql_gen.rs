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

// --- 针对 DDL 的追加（2026-09-21）---

/// 生成 `CREATE TABLE` 的**合成**文本（无 I/O、无方言依赖）。
///
/// ## 为什么还要「合成」（源版优先）
///
/// 属性面板的 DDL 是三分：**能取源就取源**（`Database::get_table_ddl` —— MySQL
/// `SHOW CREATE TABLE`、SQLite `sqlite_master.sql`、DuckDB `duckdb_tables().sql`，
/// 表与视图都给），取不到才走这里的合成。**PostgreSQL 恒定走这一条**：它没有
/// `SHOW CREATE TABLE` 的等价物，要拿原文得自己用 `pg_get_*def` 拼装 —— 那已经是合成。
/// 参考实现里 zqlz 的做法也是这个三分。合成所需的三样（列 / 约束 / 索引）属性面板本来就
/// 已经拉过了，所以合成是**纯函数**：不动驱动 trait（取源另走 `Database`）、不新增 I/O、
/// 可单测。
///
/// ## 诚实边界（写进首行注释，不假装完整）
///
/// 合成不出来的东西：分区 / 存储引擎 / 字符集与排序规则 / 表级选项 / `CHECK` 表达式
/// （`ConstraintDetail` 不带表达式）/ 视图定义 / 非唯一索引。它们在首行注释里如实列出。
/// **这些恰好是源版里本来就有的** —— 所以面板能取源时会明确标成「DDL（源版）」。
///
/// ## 输入
///
/// 直接吃驱动已有的结构体（`properties_panel` 同一份数据），不做中间映射层。
pub fn create_table_ddl(
    qualified: &str,
    columns: &[engine::driver::traits::ColumnDetail],
    constraints: &[engine::driver::traits::ConstraintDetail],
    indexes: &[engine::driver::traits::IndexDetail],
) -> String {
    if columns.is_empty() {
        return format!("-- {qualified}：未加载到列信息，无法合成 DDL");
    }

    let mut lines = vec![
        format!(
            "-- {qualified}：由目录信息**合成**的 DDL（不含分区 / 存储引擎 / 字符集 / \
             表级选项 / CHECK 表达式 / 非唯一索引）"
        ),
        format!("CREATE TABLE {qualified} ("),
    ];

    // body 的每一项是 `(语句, 可选注释)` —— **不把注释拼进语句里**。
    // 为什么：行内注释是 `-- …`，它会把后面的逗号一起注释掉。上一版就是拼在一起，
    // 于是 `ON DELETE CASCADE  -- fk_name,` 里的逗号落进了注释 → 生成的是语法错的 DDL
    // （列上有注释时同样踩，只是要两列以上才看得出来）。所以逗号拼在注释**之前**。
    let mut body: Vec<(String, Option<String>)> = columns
        .iter()
        .map(|c| {
            let mut piece = format!("{} {}", c.name, c.data_type);
            if !c.nullable {
                piece.push_str(" NOT NULL");
            }
            if let Some(d) = c.default_value.as_deref().filter(|s| !s.trim().is_empty()) {
                piece.push_str(&format!(" DEFAULT {d}"));
            }
            let comment = c
                .comment
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            (piece, comment)
        })
        .collect();

    // 主键来源三档（**顺序有意义**）：
    //
    // 1. 约束里的 `PRIMARY KEY` —— 最准，且带约束名；
    // 2. 索引里的 `is_primary` —— 有的驱动只在内省索引时露主键；
    // 3. **列上的 `is_primary_key`** —— 最后一档，但它是当下**唯一六个驱动都填了的**：
    //    `list_constraints` 六个驱动**一个都没实现**（`get_constraints` 全部委派给它，
    //    落到 trait 默认空实现），所以前三档里前两档当下恒为空。
    //    列上的标记走 `get_table_detail`，那条路径是活的（PG 用
    //    `information_schema.table_constraints` 现算，MySQL 用 `PRI` 标记）。
    let pk_cols: Option<Vec<String>> = constraints
        .iter()
        .find(|c| c.constraint_type.eq_ignore_ascii_case("PRIMARY KEY"))
        .map(|c| c.column_names.clone())
        .filter(|v| !cols_are_empty(v))
        .or_else(|| {
            indexes
                .iter()
                .find(|i| i.is_primary && !cols_are_empty(&i.column_names))
                .map(|i| i.column_names.clone())
        })
        .or_else(|| {
            let from_columns: Vec<String> = columns
                .iter()
                .filter(|c| c.is_primary_key)
                .map(|c| c.name.clone())
                .collect();
            (!from_columns.is_empty()).then_some(from_columns)
        });

    if let Some(cols) = pk_cols {
        body.push((format!("PRIMARY KEY ({})", cols.join(", ")), None));
    }

    for c in constraints {
        let kind = c.constraint_type.to_ascii_uppercase();
        if kind.contains("PRIMARY") || cols_are_empty(&c.column_names) {
            continue;
        }
        let cols = c.column_names.join(", ");
        if kind.contains("FOREIGN") {
            let Some(target) = c.referenced_table.as_deref().filter(|s| !s.is_empty()) else {
                continue; // 引用表未知：写半个外键还不如不写
            };
            let ref_cols = c.referenced_columns.join(", ");
            let mut piece = format!("FOREIGN KEY ({cols}) REFERENCES {target}");
            if !ref_cols.is_empty() {
                piece.push_str(&format!(" ({ref_cols})"));
            }
            // `NO ACTION` 是 SQL 的默认动作，PG 会把它报成 `'a'`；打出来只是噪音
            // （DBeaver 也不打）。其余四档都带上。
            if let Some(r) = fk_action(c.update_rule.as_deref()) {
                piece.push_str(&format!(" ON UPDATE {r}"));
            }
            if let Some(r) = fk_action(c.delete_rule.as_deref()) {
                piece.push_str(&format!(" ON DELETE {r}"));
            }
            let comment = (!c.name.is_empty()).then(|| c.name.clone());
            body.push((piece, comment));
        } else if kind.contains("UNIQUE") {
            body.push((format!("UNIQUE ({cols})"), None));
        }
        // `CHECK` 有意跳过：`ConstraintDetail` 不带表达式，合成不了。
    }

    for (i, (sql, comment)) in body.iter().enumerate() {
        let is_last = i + 1 == body.len();
        let mut line = format!("  {sql}{}", if is_last { "" } else { "," });
        if let Some(c) = comment {
            // 逗号已经在上面拼好了，注释在**最后**（行内注释只用 `--`：三种方言都认，
            // MySQL 的行内 `COMMENT '…'` 不是标准语法）
            line.push_str(&format!("  -- {c}"));
        }
        lines.push(line);
    }
    lines.push(");".to_string());
    lines.join("\n")
}

/// 外键动作：`NO ACTION` 是默认值，返回 `None`（不打进 DDL）。
fn fk_action(rule: Option<&str>) -> Option<&str> {
    match rule.map(str::trim) {
        None | Some("") => None,
        Some(r) if r.eq_ignore_ascii_case("NO ACTION") => None,
        Some(r) => Some(r),
    }
}

fn cols_are_empty(cols: &[String]) -> bool {
    cols.is_empty() || cols.iter().all(|c| c.trim().is_empty())
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

    // ==================== DDL 合成 ====================

    fn col(
        name: &str,
        ty: &str,
        nullable: bool,
        default: Option<&str>,
    ) -> engine::driver::traits::ColumnDetail {
        engine::driver::traits::ColumnDetail {
            name: name.to_string(),
            data_type: ty.to_string(),
            nullable,
            is_primary_key: false,
            is_foreign_key: false,
            default_value: default.map(str::to_string),
            comment: None,
            extra: Default::default(),
            references: None,
            ordinal: 0,
        }
    }

    fn cons(
        name: &str,
        kind: &str,
        cols: &[&str],
        ref_table: Option<&str>,
        ref_cols: &[&str],
    ) -> engine::driver::traits::ConstraintDetail {
        engine::driver::traits::ConstraintDetail {
            name: name.to_string(),
            table_name: "t".to_string(),
            constraint_type: kind.to_string(),
            column_names: cols.iter().map(|s| s.to_string()).collect(),
            referenced_table: ref_table.map(str::to_string),
            referenced_columns: ref_cols.iter().map(|s| s.to_string()).collect(),
            update_rule: None,
            delete_rule: None,
        }
    }

    fn idx(name: &str, cols: &[&str], primary: bool) -> engine::driver::traits::IndexDetail {
        engine::driver::traits::IndexDetail {
            name: name.to_string(),
            table_name: "t".to_string(),
            column_names: cols.iter().map(|s| s.to_string()).collect(),
            is_unique: primary,
            is_primary: primary,
            index_type: None,
            comment: None,
        }
    }

    #[test]
    fn ddl_lists_columns_with_type_not_null_and_default() {
        let sql = create_table_ddl(
            "public.t",
            &[
                col("id", "int4", false, None),
                col("name", "varchar(20)", true, Some("'x'")),
            ],
            &[],
            &[],
        );
        assert!(sql.contains("CREATE TABLE public.t ("), "{sql}");
        assert!(sql.contains("  id int4 NOT NULL"), "{sql}");
        assert!(sql.contains("  name varchar(20) DEFAULT 'x'"), "{sql}");
        assert!(sql.trim_end().ends_with(");"), "{sql}");
        // 首行如实说明合成边界，不假装完整
        assert!(sql.contains("合成"), "{sql}");
    }

    #[test]
    fn ddl_takes_primary_key_from_constraints_first() {
        let sql = create_table_ddl(
            "t",
            &[col("a", "int4", false, None), col("b", "int4", false, None)],
            &[cons("t_pkey", "PRIMARY KEY", &["a", "b"], None, &[])],
            &[],
        );
        assert!(sql.contains("PRIMARY KEY (a, b)"), "{sql}");
    }

    #[test]
    fn ddl_falls_back_to_primary_index_when_no_constraint() {
        // 有的驱动只在内省索引时露主键：回退看 is_primary
        let sql = create_table_ddl(
            "t",
            &[col("a", "int4", false, None)],
            &[],
            &[idx("t_pkey", &["a"], true)],
        );
        assert!(sql.contains("PRIMARY KEY (a)"), "{sql}");
    }

    #[test]
    fn ddl_falls_back_to_column_primary_flag_last() {
        // 最后一道回退：列上的 `is_primary_key`。它是当下**唯一六个驱动都填了的**
        // —— `list_constraints` 六个驱动一个都没实现，所以前两档恒空。
        let mut pk = col("id", "int4", false, None);
        pk.is_primary_key = true;
        let sql = create_table_ddl("t", &[pk, col("v", "text", true, None)], &[], &[]);
        assert!(sql.contains("PRIMARY KEY (id)"), "{sql}");
    }

    #[test]
    fn ddl_prefers_explicit_constraint_over_column_flag() {
        // 两档都给时以约束为准（带约束名的那份更可信，且能表达组合主键顺序）
        let mut pk = col("id", "int4", false, None);
        pk.is_primary_key = true;
        let sql = create_table_ddl(
            "t",
            &[pk],
            &[cons("t_pkey", "PRIMARY KEY", &["id", "other"], None, &[])],
            &[],
        );
        assert!(sql.contains("PRIMARY KEY (id, other)"), "{sql}");
    }

    #[test]
    fn ddl_emits_foreign_key_with_rules() {
        let mut fk = cons("fk_u", "FOREIGN KEY", &["uid"], Some("users"), &["id"]);
        fk.delete_rule = Some("CASCADE".to_string());
        fk.update_rule = Some("RESTRICT".to_string());
        let sql = create_table_ddl("t", &[col("uid", "int4", true, None)], &[fk], &[]);
        assert!(
            sql.contains("FOREIGN KEY (uid) REFERENCES users (id)"),
            "{sql}"
        );
        assert!(sql.contains("ON UPDATE RESTRICT"), "{sql}");
        assert!(sql.contains("ON DELETE CASCADE"), "{sql}");
        assert!(sql.contains("-- fk_u"), "{sql}");
    }

    #[test]
    fn ddl_skips_foreign_key_without_target_table() {
        // 引用表未知：宁可不写，也不写出一个指向空名字的外键
        let sql = create_table_ddl(
            "t",
            &[col("uid", "int4", true, None)],
            &[cons("fk_u", "FOREIGN KEY", &["uid"], None, &[])],
            &[],
        );
        assert!(!sql.contains("FOREIGN KEY"), "{sql}");
    }

    #[test]
    fn ddl_emits_unique_and_skips_check() {
        let sql = create_table_ddl(
            "t",
            &[
                col("email", "text", true, None),
                col("age", "int4", true, None),
            ],
            &[
                cons("uq_email", "UNIQUE", &["email"], None, &[]),
                cons("ck_age", "CHECK", &["age"], None, &[]),
            ],
            &[],
        );
        assert!(sql.contains("UNIQUE (email)"), "{sql}");
        // CHECK 表达式不在 ConstraintDetail 里，合成不了就**不写**（也不写错）。
        // 注：首行注释里就有「CHECK」字样，所以要按行判。
        assert!(
            !sql.lines().any(|l| l.trim_start().starts_with("CHECK")),
            "{sql}"
        );
    }

    #[test]
    fn ddl_empty_columns_yields_comment_only() {
        let sql = create_table_ddl("t", &[], &[], &[]);
        assert!(sql.starts_with("-- "), "{sql}");
        assert!(!sql.contains("CREATE TABLE"), "{sql}");
    }

    #[test]
    fn ddl_single_column_has_no_trailing_comma() {
        let sql = create_table_ddl("t", &[col("id", "int4", false, None)], &[], &[]);
        assert!(sql.contains("  id int4 NOT NULL\n);"), "{sql}");
    }

    /// 回归（真机输出抳出来的）：行内注释是 `-- …`，拼在语句尾巴上会把**逗号一起注释掉**，
    /// 生成的是语法错的 DDL（`ON DELETE CASCADE  -- fk_name,`）。逗号必须在注释**之前**。
    #[test]
    fn ddl_puts_the_comma_before_an_inline_comment() {
        let mut a = col("a", "int4", false, None);
        a.comment = Some("主键".to_string());
        let sql = create_table_ddl("t", &[a, col("b", "text", true, None)], &[], &[]);
        let line = sql
            .lines()
            .find(|l| l.contains("a int4"))
            .expect("列 a 那一行");
        let comma = line.find(',').expect("非末列要有逗号");
        let mark = line.find("--").expect("要有行内注释");
        assert!(comma < mark, "逗号落在注释里了：{line}");
        assert!(line.trim_end().ends_with("主键"), "{line}");
    }

    /// 外键的约束名也走同一条规矩（它是 `-- 名称` 形式的注释）
    #[test]
    fn ddl_fk_name_is_a_comment_after_the_comma() {
        let sql = create_table_ddl(
            "t",
            &[col("uid", "int4", true, None), col("v", "text", true, None)],
            &[
                cons("fk_u", "FOREIGN KEY", &["uid"], Some("users"), &["id"]),
                cons("uq_v", "UNIQUE", &["v"], None, &[]),
            ],
            &[],
        );
        let line = sql.lines().find(|l| l.contains("FOREIGN KEY")).unwrap();
        let comma = line.find(',').expect("后面还有 UNIQUE，要有逗号");
        let mark = line.find("-- fk_u").expect("要有约束名");
        assert!(comma < mark, "逗号落在注释里了：{line}");
    }

    /// `NO ACTION` 是 SQL 的默认动作（PG 把它报成 `'a'`），不打进 DDL —— 只去噪音。
    #[test]
    fn ddl_omits_default_no_action_but_keeps_others() {
        let mut fk = cons("fk_u", "FOREIGN KEY", &["uid"], Some("users"), &["id"]);
        fk.update_rule = Some("NO ACTION".to_string());
        fk.delete_rule = Some("CASCADE".to_string());
        let sql = create_table_ddl("t", &[col("uid", "int4", true, None)], &[fk], &[]);
        assert!(
            !sql.contains("ON UPDATE"),
            "默认的 NO ACTION 不应打出来：{sql}"
        );
        assert!(sql.contains("ON DELETE CASCADE"), "{sql}");
    }
}
