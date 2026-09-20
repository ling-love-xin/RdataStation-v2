//! `Analysis` 档的指纹口径与事实快照（原型 §1 原则 2；开发方案 §6 P4.1 与风险 R4）。
//!
//! **裁决（R4，2026-09-20 落地）**：分析表的指纹 = 「**定义** + **列结构**」，**行数只作元信息**。
//!
//! - **行数不进指纹**：分析表的数据会随上游增长，“每多一行就报一次内容已变”没有信息量，
//!   反而把真正该看的“结构变了”淹掉；
//! - **结构（列名 + 类型，保持声明顺序）进指纹**：加列 / 删列 / 改类型 / 换顺序都该被看见；
//! - **定义（`definition_sql`）进指纹**：同结构但换了算法（比如换了过滤条件）就是另一份存档。
//!
//! 边界：本模块只做**口径与拼装**（纯函数，零 I/O）。**把事实读出来是宿主的事**——
//! M6 不碰 DuckDB（`resources/` 里的文件是本体，读它的 schema 属于数据访问层）；
//! 采集侧（`read_csv_auto` / `DESCRIBE` 之类）按本模块的 [`AnalysisFacts`] 拼好推给服务层。

use sha2::{Digest as _, Sha256};

/// 一列的结构（列名 + 类型）：**顺序有意义**（`SELECT *` 的列序就是它）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
}

impl ColumnSpec {
    pub fn new(name: impl Into<String>, data_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data_type: data_type.into(),
        }
    }
}

/// 一次采集到的分析表事实（宿主读出来后推给服务层）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnalysisFacts {
    /// 重建定义（配方）：`analysis` 型的“这份东西是怎么算出来的”。
    pub definition_sql: Option<String>,
    /// 列结构（保持声明顺序）。
    pub columns: Vec<ColumnSpec>,
    /// 行数（**只作元信息**：进详情与行尾的“规模”，不进指纹）。
    pub row_count: Option<i64>,
}

impl AnalysisFacts {
    /// 用**探到的文件事实**拼一份分析表事实（engine `probe_file` 的回形 → M6 的口径）。
    ///
    /// 为何要这层转：engine 那边只认「列名 + 类型」的元组（它不能依赖 M6 的
    /// `ColumnSpec`——依赖方向是单向的），转换放在这里，两边的形状各归各。
    pub fn from_probe(
        definition_sql: Option<String>,
        columns: Vec<(String, String)>,
        row_count: Option<i64>,
    ) -> Self {
        Self {
            definition_sql,
            columns: columns
                .into_iter()
                .map(|(name, data_type)| ColumnSpec { name, data_type })
                .collect(),
            row_count,
        }
    }

    /// 列数（`i32`：与库里的列宽一致；列数过 `i32` 是不可能的，直接截断）。
    pub fn column_count(&self) -> i32 {
        self.columns.len().min(i32::MAX as usize) as i32
    }

    /// 结构摘要：一行一列 `列名\t类型`，**保持声明顺序**。
    ///
    /// 为什么保留原始大小写：DuckDB 的标识符大小写不敏感，但**显示**要与用户看到的一致；
    /// 归一化（全小写）会把 `OrderID` 与 `orderid` 当成同一件事——那是另一种口径，不该悄悄替用户决定。
    pub fn structure_digest(&self) -> String {
        self.columns
            .iter()
            .map(|column| format!("{}\t{}", column.name, column.data_type))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 指纹：`sha256(定义 + 结构摘要)`（行数不进——见模块头）。
    ///
    /// 两段之间放一个 NUL 分隔：不留分隔符时“定义末尾”与“结构开头”的拼接会撞上
    /// （`...SELECT a` + `b\tINT` 与 `...SELECT ab` + `\tINT` 会得到同一个串）。
    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.definition_sql.as_deref().unwrap_or("").as_bytes());
        hasher.update([0u8]);
        hasher.update(self.structure_digest().as_bytes());
        hex::encode(hasher.finalize())
    }
}

/// 「内容指纹」那一行给分析表的补充说明（详情面板 tooltip / UI 文案，单一来源）。
pub const FINGERPRINT_HINT: &str = "分析表指纹 = 定义 + 列结构；行数变化不算内容变化";

/// 分析表“内容已变”的**准确**文案：与文件型的“内容已变”分开说。
///
/// 文件型的“内容已变”是字节级；分析表看的是结构与定义——写“文件被改过”会指错方向。
pub const STRUCTURE_CHANGED_NOTE: &str = "结构或定义与归档时不同";

#[cfg(test)]
mod tests {
    use super::{AnalysisFacts, ColumnSpec, FINGERPRINT_HINT, STRUCTURE_CHANGED_NOTE};

    fn facts(definition: &str, columns: &[(&str, &str)], rows: Option<i64>) -> AnalysisFacts {
        AnalysisFacts {
            definition_sql: Some(definition.to_string()),
            columns: columns
                .iter()
                .map(|(name, ty)| ColumnSpec::new(*name, *ty))
                .collect(),
            row_count: rows,
        }
    }

    /// 结构摘要：一行一列、保序；空结构给空串（不是换行）。
    #[test]
    fn digest_keeps_declared_order_and_case() {
        let one = facts("select 1", &[("id", "INTEGER"), ("Name", "VARCHAR")], None);
        assert_eq!(one.structure_digest(), "id\tINTEGER\nName\tVARCHAR");
        assert_eq!(AnalysisFacts::default().structure_digest(), "");
        assert_eq!(AnalysisFacts::default().column_count(), 0);

        // 换顺序 = 换结构（摘要是顺序敏感的）。
        let swapped = facts("select 1", &[("Name", "VARCHAR"), ("id", "INTEGER")], None);
        assert_ne!(one.structure_digest(), swapped.structure_digest());
        assert_ne!(one.fingerprint(), swapped.fingerprint());
    }

    /// R4 的核心：**行数不进指纹**，而列 / 类型 / 定义 / 顺序都进。
    #[test]
    fn fingerprint_covers_definition_and_structure_but_not_row_count() {
        let base = facts("select * from t", &[("id", "INTEGER")], Some(10));
        let grown = facts("select * from t", &[("id", "INTEGER")], Some(10_000));
        let unknown = facts("select * from t", &[("id", "INTEGER")], None);
        assert_eq!(
            base.fingerprint(),
            grown.fingerprint(),
            "行数变化不算内容变化"
        );
        assert_eq!(
            base.fingerprint(),
            unknown.fingerprint(),
            "没读到行数也一样"
        );

        let renamed = facts("select * from t", &[("order_id", "INTEGER")], Some(10));
        let retyped = facts("select * from t", &[("id", "BIGINT")], Some(10));
        let added = facts(
            "select * from t",
            &[("id", "INTEGER"), ("name", "VARCHAR")],
            Some(10),
        );
        let redefined = facts(
            "select * from t where id > 0",
            &[("id", "INTEGER")],
            Some(10),
        );
        for (label, other) in [
            ("列名", renamed),
            ("类型", retyped),
            ("加列", added),
            ("定义", redefined),
        ] {
            assert_ne!(
                base.fingerprint(),
                other.fingerprint(),
                "{label}变化应改指纹"
            );
        }

        // 没定义（旧行 / 手写分析）也给得出指纹：空定义 + 结构。
        let no_definition = AnalysisFacts {
            definition_sql: None,
            columns: vec![ColumnSpec::new("id", "INTEGER")],
            row_count: None,
        };
        assert_eq!(no_definition.fingerprint().len(), 64, "sha256 十六进制");
    }

    /// 文案是单一来源：两句话各不相同，且都不提“行数变了”（那不算内容变化）。
    #[test]
    fn from_probe_maps_the_engine_shape_into_the_fingerprint_shape() {
        let facts = AnalysisFacts::from_probe(
            Some("SELECT * FROM read_csv_auto('a.csv')".to_string()),
            vec![
                ("id".to_string(), "BIGINT".to_string()),
                ("name".to_string(), "VARCHAR".to_string()),
            ],
            Some(3),
        );
        assert_eq!(facts.column_count(), 2);
        assert_eq!(facts.structure_digest(), "id\tBIGINT\nname\tVARCHAR");
        assert_eq!(facts.row_count, Some(3));
        // 同结构两次探测（行数不同）→ 同一指纹（这正是 R4 要的口径）。
        let again = AnalysisFacts::from_probe(
            facts.definition_sql.clone(),
            vec![
                ("id".to_string(), "BIGINT".to_string()),
                ("name".to_string(), "VARCHAR".to_string()),
            ],
            Some(999),
        );
        assert_eq!(facts.fingerprint(), again.fingerprint());
    }

    #[test]
    fn notes_say_what_the_fingerprint_really_covers() {
        assert!(FINGERPRINT_HINT.contains("行数"));
        assert!(STRUCTURE_CHANGED_NOTE.contains("结构"));
        assert_ne!(FINGERPRINT_HINT, STRUCTURE_CHANGED_NOTE);
    }
}
