//! 【对齐 DBeaver Metadata 面板「Generate DDL」】按结果集列生成建表 DDL 的纯函数。
//!
//! **DBeaver 的口径**（取证）：
//! - `panel/metadata/MetaDataPanel.java:203-215` 的 `contributeActions` 只贡献一条命令
//!   `org.jkiss.dbeaver.ui.editors.sql.generate.ddl.by.resultSet`
//!   （文案 `ResultSetMessages.generate_ddl_by_result_set_name`，图标 `DBIcon.SQL_TEXT`，
//!   恒 `enabled=true`）。
//! - 处理器 `SQLGeneratorHandlerResultSetDDL.execute` 把当前结果集塞进
//!   `SQLGeneratorDDLFromResultSet`，再由 `SQLGeneratorDDLFromResultSet.generateSQL`
//!   走**数据迁移的映射机制**（`DatabaseMappingContainer` + `mapping.refreshMappingType(create)`）
//!   算出「按这些列建一张能装下这份结果集的新表」的 DDL，最后 `DatabaseTransferUtils.
//!   generateTargetTableDDL` + `SQLUtils.generateScript` 出**方言脚本**，弹 `ViewSQLDialog` 展示。
//!   也就是说：它生成的是**新表**的 DDL，不是来源表的 DDL。
//! - 两个「宁给说明不给 SQL」的分支：动态元数据 → `sql_generator_nonsql_text`；
//!   没有目标容器 → `sql_generator_no_obj_container_text`；生成的 DDL 为空 →
//!   `sql_generator_no_ddl_text`。
//!
//! **本仓口径**（有意差异都写在注释里）：
//! - 没有迁移 / 映射机制，也就不做「值类型 → 方言类型」的猜测转换：**列类型照驱动
//!   口径**（`ColumnAnnotation.data_type` 优先，退到 `ColumnType.name`）。驱动没报类型的
//!   列（表达式列 / 动态元数据），若值族认得出来则按族给一个通用类型并在行尾注明
//!   「按值族推断」；连值族都认不出（`ColumnKind::Unknown`）则**整份不出 SQL**，
//!   返回说明——与 DBeaver 的 `nonsql_text` 分支同态度（不编一个假类型出来）。
//! - 标识符按原名直出（不套引号）：引号字符随方言变，而本层不知道目标方言
//!   （DBeaver 走 `SQLUtils` 的方言引用规则）。名字含空格 / 保留字时需要用户自己加引号。
//! - 表名由调用方给：调用方取「所有已注解列一致的来源表裸名」，否则用占位名。

use crate::store::ColumnKind;

/// 生成 DDL 用的列信息（由面板从网格 delegate 的列 / 类型 / 注解装配）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdlColumn {
    /// 列名（照结果集的列名，不改大小写）
    pub name: String,
    /// 驱动口径的类型名（`int4` / `numeric(38,10)` …）；`None` = 驱动没报
    pub type_name: Option<String>,
    /// 类型名是不是「按值族推断」出来的（真则行尾注明，读的人知道这不是驱动原话）
    pub inferred: bool,
    /// 可空性（`None` = 元数据没给，则不写 `NOT NULL` 也不写 `NULL`）
    pub nullable: Option<bool>,
    /// 默认值表达式（驱动原话；`None` = 没有默认值）
    pub default_value: Option<String>,
}

/// 生成结果：要么是 SQL 文本，要么是一句「为什么出不了 SQL」
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdlOutcome {
    /// 可用 SQL（末行 `);`，列之间逗号换行）
    Sql(String),
    /// 出不了 SQL 的说明（面板把它原样显示在预览浮层里）
    Unavailable(String),
}

/// 按值族给通用类型名（驱动没报类型时用；**不含精度**，因为精度无从得知）
fn generic_type(kind: ColumnKind) -> Option<&'static str> {
    match kind {
        ColumnKind::Integer => Some("INTEGER"),
        ColumnKind::Float => Some("DOUBLE PRECISION"),
        ColumnKind::Decimal => Some("DECIMAL"),
        ColumnKind::Boolean => Some("BOOLEAN"),
        ColumnKind::Text => Some("TEXT"),
        ColumnKind::Timestamp => Some("TIMESTAMP"),
        ColumnKind::Date => Some("DATE"),
        ColumnKind::Time => Some("TIME"),
        ColumnKind::Uuid => Some("UUID"),
        ColumnKind::Json => Some("JSON"),
        // 数组没有跨库通用的写法（`TEXT[]` / `ARRAY<TEXT>` 互不通用）：不兜底，
        // 让上游「有驱动原始名就原样用」那条路径去出 `text[]`；真没名字就按
        // 「编不出类型」报 `Unavailable`，不编一个错语法
        ColumnKind::Array => None,
        ColumnKind::Binary => Some("BLOB"),
        ColumnKind::Unknown => None,
    }
}

/// 生成 `CREATE TABLE <table> (...)`。
///
/// 失败（返回 `Unavailable`）的三种情形，都与 DBeaver 的「说明分支」同态度：
/// 1. 一列都没有；
/// 2. 有列既没驱动类型、值族也认不出 —— 不编类型；
/// 3. 表名与列名同为空白（调用方本不该传，属于防御）。
pub fn create_table_ddl(table: &str, columns: &[DdlColumn]) -> DdlOutcome {
    if columns.is_empty() {
        return DdlOutcome::Unavailable(
            "结果集还没有列，无法生成 DDL".to_string(),
        );
    }
    let table = table.trim();
    if table.is_empty() {
        return DdlOutcome::Unavailable("表名为空，无法生成 DDL".to_string());
    }

    let mut lines: Vec<String> = Vec::with_capacity(columns.len());
    let mut missing: Vec<String> = Vec::new();
    for column in columns {
        let name = column.name.trim();
        if name.is_empty() {
            return DdlOutcome::Unavailable("结果集里有空列名，无法生成 DDL".to_string());
        }
        let type_name = column
            .type_name
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let Some(type_name) = type_name else {
            missing.push(name.to_string());
            continue;
        };
        let mut line = format!("  {name} {type_name}");
        if column.nullable == Some(false) {
            line.push_str(" NOT NULL");
        }
        if let Some(default) = column
            .default_value
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            line.push_str(" DEFAULT ");
            line.push_str(default);
        }
        if column.inferred {
            line.push_str(" -- 类型按值族推断（驱动未报）");
        }
        lines.push(line);
    }

    if !missing.is_empty() {
        return DdlOutcome::Unavailable(format!(
            "这些列既没有驱动类型、也认不出值族，不能编类型出来：{}（对齐 DBeaver 的\
             `sql_generator_nonsql_text`，此时它也只给说明）",
            missing.join(" / ")
        ));
    }

    let mut sql = String::with_capacity(64 + lines.iter().map(|l| l.len() + 3).sum::<usize>());
    sql.push_str("CREATE TABLE ");
    sql.push_str(table);
    sql.push_str(" (\n");
    for (index, line) in lines.iter().enumerate() {
        sql.push_str(line);
        if index + 1 < lines.len() {
            sql.push(',');
        }
        sql.push('\n');
    }
    sql.push_str(");\n");
    DdlOutcome::Sql(sql)
}

/// 列的「可生成类型名」：驱动口径优先，否则按值族推断（`inferred = true`）。
///
/// 放在这里而不是调用方：推断规则只有一份，面板只负责取素材。
pub fn resolve_type(
    driver_type: Option<&str>,
    kind: ColumnKind,
) -> Option<(String, bool)> {
    if let Some(name) = driver_type.map(str::trim).filter(|text| !text.is_empty()) {
        return Some((name.to_string(), false));
    }
    generic_type(kind).map(|name| (name.to_string(), true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, type_name: Option<&str>) -> DdlColumn {
        DdlColumn {
            name: name.to_string(),
            type_name: type_name.map(str::to_string),
            inferred: false,
            nullable: None,
            default_value: None,
        }
    }

    /// 基本形状：列逐行、逗号只在列间、末行 `);`
    #[test]
    fn generates_one_line_per_column() {
        let outcome = create_table_ddl(
            "orders",
            &[column("id", Some("INTEGER")), column("amount", Some("NUMERIC(10,2)"))],
        );
        let DdlOutcome::Sql(sql) = outcome else {
            panic!("两列都有类型，应该出 SQL：{outcome:?}");
        };
        assert_eq!(
            sql,
            "CREATE TABLE orders (\n  id INTEGER,\n  amount NUMERIC(10,2)\n);\n"
        );
    }

    /// 可选约束：`NOT NULL` 只在元数据明确说不可空时写；默认值照驱动原话直出
    #[test]
    fn writes_not_null_only_when_metadata_says_not_nullable() {
        let mut id = column("id", Some("bigint"));
        id.nullable = Some(false);
        let mut note = column("note", Some("text"));
        note.nullable = Some(true);
        let mut created = column("created_at", Some("timestamptz"));
        created.default_value = Some("now()".to_string());

        let DdlOutcome::Sql(sql) = create_table_ddl("t", &[id, note, created]) else {
            panic!("三列都有类型");
        };
        assert!(sql.contains("  id bigint NOT NULL,"), "{sql}");
        assert!(sql.contains("  note text,"), "可空列不写约束：{sql}");
        assert!(
            sql.contains("  created_at timestamptz DEFAULT now()\n);"),
            "默认值照驱动原话：{sql}"
        );
        assert!(!sql.contains("NULL,") || sql.contains("NOT NULL"), "{sql}");
    }

    /// 推断出来的类型必须带行尾说明（不冒充驱动口径）
    #[test]
    fn inferred_types_are_marked() {
        let mut expr = column("expr_col", Some("DOUBLE PRECISION"));
        expr.inferred = true;
        let DdlOutcome::Sql(sql) = create_table_ddl("t", &[expr]) else {
            panic!("推断出了类型，应该出 SQL");
        };
        assert!(
            sql.contains("  expr_col DOUBLE PRECISION -- 类型按值族推断（驱动未报）\n);"),
            "{sql}"
        );
    }

    /// 认不出类型的列 → 整份不出 SQL（与 DBeaver `nonsql_text` 分支同态度）
    #[test]
    fn refuses_instead_of_inventing_a_type() {
        let outcome = create_table_ddl("t", &[column("id", Some("int")), column("x", None)]);
        let DdlOutcome::Unavailable(reason) = outcome else {
            panic!("有列没类型就不该出 SQL");
        };
        assert!(reason.contains('x'), "说明里要点名哪一列：{reason}");
    }

    /// 边界：没有列 / 空表名都不崩，且给说明
    #[test]
    fn refuses_on_empty_input() {
        assert!(matches!(
            create_table_ddl("t", &[]),
            DdlOutcome::Unavailable(_)
        ));
        assert!(matches!(
            create_table_ddl("  ", &[column("id", Some("int"))]),
            DdlOutcome::Unavailable(_)
        ));
    }

    /// 类型解析：驱动口径优先（`inferred=false`），否则按值族（`inferred=true`），
    /// `Unknown` 且无驱动类型 → `None`（不猜）
    #[test]
    fn resolves_type_from_driver_then_kind() {
        assert_eq!(
            resolve_type(Some(" int4 "), ColumnKind::Integer),
            Some(("int4".to_string(), false))
        );
        assert_eq!(
            resolve_type(Some(""), ColumnKind::Text),
            Some(("TEXT".to_string(), true))
        );
        assert_eq!(resolve_type(None, ColumnKind::Decimal), Some(("DECIMAL".to_string(), true)));
        assert_eq!(resolve_type(None, ColumnKind::Unknown), None);
    }
}
