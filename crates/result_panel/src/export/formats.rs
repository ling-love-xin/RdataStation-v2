//! 导出格式与菜单（纯数据面）——自 export.rs 拆分（2026-10-29）
//!
//! 格式枚举（[`ExportFormat`]）/ 行定位依据（[`RowKey`]）/ 导出范围（[`ExportScope`]）/
//! 菜单项（[`menu_items`] / [`menu_items_for`]）与表结构端口（[`TableKeysPort`]）。
//! 编码与表名解析见 `encode` / `table_name`；模块说明见父模块 `crate::export`。

use std::rc::Rc;
/// 【B7 切片三】表结构端口：查「连接 + 表名 → 键列」（主键；唯一列留待以后）
///
/// 结果区导出 `SQL Updates` 需要键列来定位行；结果集自己不知道主键
/// （[`ResultEntry`] 只有列名与展示文本），键列由宿主给（导航侧 L2 缓存，
/// workbench 实现见 `workbench::services::editor_table_keys`）。
///
/// **查不到 = `None`**（面板不猜“拿第一列当键”，菜单不摆 `SQL Updates`）：
/// `UPDATE … WHERE 随便一列` 猜错的代价是改错行，这是本模块“宁可回绝也不猜”的口径。
pub trait TableKeysPort {
    /// 该连接下表的主键列名（按列序）；查不到 / 表不认识 = `None`
    fn primary_key_columns(&self, conn_id: &str, table: &str) -> Option<Vec<String>>;
}

pub type TableKeysHandle = Rc<dyn TableKeysPort>;

/// 导出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// 逗号分隔文本（RFC 4180 的转义规则）
    Csv,
    /// 对象数组（`[{"列": "值"}, …]`）
    Json,
    /// `INSERT INTO … VALUES (…);`（每 200 行一条语句）
    Insert,
    /// 【B7 切片三】`UPDATE … SET … WHERE 键 = 值;`（每行一条；**键列必须由调用方给**）
    SqlUpdates,
    /// 【B7 切片三】只出谓词：`WHERE (… AND …) OR (…)`（贴进任何语句都对得上）
    WhereClause,
    /// 【B7 切片二】列式二进制（DuckDB `COPY … (FORMAT parquet)`；内核自带，离线可用）
    Parquet,
    /// 【B7 切片二】Excel 工作簿（DuckDB `COPY … (FORMAT xlsx)`；要 `excel` 扩展）
    Xlsx,
}

impl ExportFormat {
    /// **有网格就能给**的几档（也是 `menu_items` 的顺序：文本三档在前，DuckDB 两档在后）
    ///
    /// 【B7 切片三】的两档**不在**这里：它们要“行定位依据”与调用方的明确表态（见
    /// [`ExportExtras`]），摆进这份列表就等于替所有调用方改菜单。
    pub const ALL: [ExportFormat; 5] = [
        Self::Csv,
        Self::Json,
        Self::Insert,
        Self::Parquet,
        Self::Xlsx,
    ];

    /// 【B7 切片三】改库用的两档（菜单里插在 `Insert` 之后、DuckDB 两档之前）
    pub const SQL_TEXT: [ExportFormat; 2] = [Self::SqlUpdates, Self::WhereClause];

    pub fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Json => "JSON",
            Self::Insert => "INSERT",
            Self::SqlUpdates => "SQL Updates",
            Self::WhereClause => "WHERE 子句",
            Self::Parquet => "Parquet",
            Self::Xlsx => "XLSX",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            // 三档 SQL 文本共用一个扩展名（与 sqlab 同口径）：`orders.sql`
            Self::Insert | Self::SqlUpdates | Self::WhereClause => "sql",
            Self::Parquet => "parquet",
            Self::Xlsx => "xlsx",
        }
    }

    /// 给文件对话框用的说明（与 `label` 不同：这里说清“导出来的是什么”）
    pub fn detail(self) -> &'static str {
        match self {
            Self::Csv => "逗号分隔文本",
            Self::Json => "对象数组（值按展示文本；NULL 是 null）",
            Self::Insert => "INSERT 语句（每 200 行一条）",
            Self::SqlUpdates => "UPDATE 语句（每行一条；要表结构里的键列）",
            Self::WhereClause => "WHERE 谓词（按键列或全部列匹配这些行）",
            Self::Parquet => "Parquet（列式，数字列保留数值类型）",
            Self::Xlsx => "Excel 工作簿（要 excel 扩展，首次装一次）",
        }
    }

    /// 这一档是不是**走 DuckDB** 落盘（文本档不是）：面板据此选“同步写文件”还是“后台 COPY”
    pub fn is_duckdb_backed(self) -> bool {
        matches!(self, Self::Parquet | Self::Xlsx)
    }

    /// 这一档要不要「行定位依据」（键列）
    ///
    /// 要而没给就**不摆在菜单里**（见 [`menu_items_for`]），编码层也会回绝
    /// （见 [`encode_rows`]）——`UPDATE` 定位不准的代价是改错行。
    pub fn requires_row_key(self) -> bool {
        matches!(self, Self::SqlUpdates)
    }
}

/// 行定位依据：哪几列能把一行认出来（`SQL Updates` 与“按键的 `WHERE 子句`”的前提）
///
/// 产品里**没有主键元数据**（`ResultEntry` 只有列名与展示文本），所以这里不接受“猜一个键”。
/// 键列由调用方给（表结构里读、或让用户选），**列名按结果集里的原样写、大小写敏感**：
/// 结果里没有这列（比如 SQL 没 SELECT 它）就是 [`RowKey::indices`] 的 `None`，
/// 编码层据此回绝，不静默降级成“全部列”。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowKey {
    columns: Vec<String>,
}

impl RowKey {
    /// 复合键（顺序就是 `WHERE` 里 `AND` 的顺序）；空列表不是键，返回 `None`
    pub fn new<I, S>(columns: I) -> Option<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let columns: Vec<String> = columns.into_iter().map(Into::into).collect();
        (!columns.is_empty()).then_some(Self { columns })
    }

    /// 单列键（最常见的一档）
    pub fn single(column: impl Into<String>) -> Self {
        Self {
            columns: vec![column.into()],
        }
    }

    /// 键列名（按给入顺序）
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// 键列在**结果列**里的下标；任一列名找不到就返回 `None`
    pub(crate) fn indices(&self, columns: &[String]) -> Option<Vec<usize>> {
        self.columns
            .iter()
            .map(|name| columns.iter().position(|column| column == name))
            .collect()
    }
}

/// 导出范围（原型 §2.4 的「仅已抓取 / 抓全量」）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportScope {
    /// 只导出已经抓到的行（不发新查询）
    Fetched,
    /// 先把剩下的段抓完，再导出（会重跑查询，见 B5b 的取舍）
    All,
}

impl ExportScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fetched => "仅已抓取",
            Self::All => "抓全量后导出",
        }
    }
}

/// 导出菜单里的一项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportMenuItem {
    pub label: String,
    /// 可点项的（格式，范围）；`None` = 分组标题（不可点）
    pub action: Option<(ExportFormat, ExportScope)>,
    /// 这项前面要不要加一条分隔线
    pub separator_before: bool,
}

/// 导出菜单的项（**纯函数**：界面只负责把这份列表画出来）
///
/// 「抓全量」那一组只在 `has_more` 时出现——已经抓完了再摆就是骗人（点下去只是一次多余的重跑）。
/// `rows_text` 是已抓行数的展示文本（千分位由调用方格式化）。
///
/// 这里只给 [`ExportFormat::ALL`] 那几档（“有网格就能给”）；【B7 切片三】的两档要能力面，
/// 走 [`menu_items_for`]。
pub fn menu_items(has_more: bool, rows_text: &str) -> Vec<ExportMenuItem> {
    menu_items_for(has_more, rows_text, &ExportExtras::default())
}

/// 【B7 切片三】附加能力面：菜单「自动长出」的哪几档由它决定（界面不逐档硬编码）
///
/// 为什么要有这层而不是直接往 [`ExportFormat::ALL`] 里塞：`SQL Updates` / `WHERE 子句`
/// 是**改库用**的文本，而且 `SQL Updates` 还缺不了键列。摆出来 = 替用户表态“我要拿它去
/// 改数据”，得由调用方（面板 + 它手里的表结构信息）说。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportExtras {
    /// 该不该摆「改库用」的 SQL 文本档（`SQL Updates` / `WHERE 子句`）
    pub sql_text: bool,
    /// 行定位依据（键列）：有才摆 `SQL Updates`
    ///
    /// 没有键列时**不摆**——没有键的 `UPDATE` 只能把某一列当键（猜），而猜错的代价是
    /// 改错行；宁可让用户先去表结构里认一下键列（或直接用 INSERT 档）。
    pub key: Option<RowKey>,
}

/// 菜单的项（**纯函数** + 能力面）：`extras` 说“这一层还能给什么”
///
/// 菜单内容完全由「`ALL` 的次序 + 能力面」推出来：加一档就改格式枚举与这里的条件，
/// 面板不需要认识具体档位。
pub fn menu_items_for(
    has_more: bool,
    rows_text: &str,
    extras: &ExportExtras,
) -> Vec<ExportMenuItem> {
    let mut formats: Vec<ExportFormat> = Vec::with_capacity(ExportFormat::ALL.len() + 2);
    for format in ExportFormat::ALL {
        // SQL 文本两档插在 `Insert` 之后（三档 SQL 挨着）；DuckDB 两档仍压尾
        if format == ExportFormat::Parquet && extras.sql_text {
            for extra in ExportFormat::SQL_TEXT {
                // 要键而没给键的档**不摆**（点了也只能给一份“回绝”文本）
                if extra.requires_row_key() && extras.key.is_none() {
                    continue;
                }
                formats.push(extra);
            }
        }
        formats.push(format);
    }

    let mut items = Vec::with_capacity(formats.len() * 2 + 2);
    if has_more {
        items.push(ExportMenuItem {
            label: format!("已抓取 {rows_text} 行"),
            action: None,
            separator_before: false,
        });
    }
    let group = |items: &mut Vec<ExportMenuItem>, scope: ExportScope| {
        for format in formats.iter() {
            items.push(ExportMenuItem {
                label: format.label().to_string(),
                action: Some((*format, scope)),
                separator_before: false,
            });
        }
    };
    group(&mut items, ExportScope::Fetched);
    if has_more {
        items.push(ExportMenuItem {
            label: "抓全量后导出（会重跑查询）".to_string(),
            action: None,
            separator_before: true,
        });
        group(&mut items, ExportScope::All);
    }
    items
}

#[cfg(test)]
mod tests {
    // 安全模式：显式列举（避免与 gpui-kit 测试宏冲突）
    use super::{ExportExtras, ExportFormat, ExportScope, RowKey};
    use crate::export::{default_file_name, encode};
    use crate::store::ResultEntry;

    /// 结果集（`sql` 决定默认表名，`rows` 就是网格里那些字符串）
    fn entry(columns: &[&str], rows: &[&[&str]]) -> ResultEntry {
        ResultEntry::success(
            workbench_shell::model::DocumentId::new("doc-1"),
            "SELECT * FROM orders".to_string(),
            12,
            false,
            columns.iter().map(|c| c.to_string()).collect(),
            rows.iter()
                .map(|row| row.iter().map(|v| v.to_string()).collect())
                .collect(),
        )
    }
    #[test]
    fn menu_items_grow_with_a_next_segment() {
        use super::menu_items;

        let fetched: Vec<(String, Option<(ExportFormat, ExportScope)>)> =
            menu_items(false, "1,000")
                .into_iter()
                .map(|item| (item.label, item.action))
                .collect();
        assert_eq!(
            fetched,
            vec![
                (
                    "CSV".to_string(),
                    Some((ExportFormat::Csv, ExportScope::Fetched))
                ),
                (
                    "JSON".to_string(),
                    Some((ExportFormat::Json, ExportScope::Fetched))
                ),
                (
                    "INSERT".to_string(),
                    Some((ExportFormat::Insert, ExportScope::Fetched))
                ),
                (
                    "Parquet".to_string(),
                    Some((ExportFormat::Parquet, ExportScope::Fetched))
                ),
                (
                    "XLSX".to_string(),
                    Some((ExportFormat::Xlsx, ExportScope::Fetched))
                ),
            ],
            "已经抓完时只给“仅已抓取”五项（不摆一个多余的重跑入口）"
        );

        let items = menu_items(true, "1,000");
        let labels: Vec<String> = items.iter().map(|item| item.label.clone()).collect();
        assert_eq!(
            labels,
            vec![
                "已抓取 1,000 行",
                "CSV",
                "JSON",
                "INSERT",
                "Parquet",
                "XLSX",
                "抓全量后导出（会重跑查询）",
                "CSV",
                "JSON",
                "INSERT",
                "Parquet",
                "XLSX",
            ],
            "还有下一段时：先说清已抓多少，再给两档范围"
        );
        assert!(items[6].separator_before, "“抓全量”那一组前面要有分隔线");
        assert!(
            items[0].action.is_none() && items[6].action.is_none(),
            "两个分组标题不可点"
        );
        assert_eq!(
            items[7].action,
            Some((ExportFormat::Csv, ExportScope::All)),
            "第二组指向“抓全量”"
        );
    }

    #[test]
    fn format_metadata_is_complete() {
        let labels: Vec<&str> = ExportFormat::ALL.iter().map(|f| f.label()).collect();
        assert_eq!(labels, ["CSV", "JSON", "INSERT", "Parquet", "XLSX"]);
        let extensions: Vec<&str> = ExportFormat::ALL.iter().map(|f| f.extension()).collect();
        assert_eq!(extensions, ["csv", "json", "sql", "parquet", "xlsx"]);
        assert_eq!(ExportScope::Fetched.label(), "仅已抓取");
        assert_eq!(ExportScope::All.label(), "抓全量后导出");
    }

    /// 【B7 切片二】走 DuckDB 的两档：本地不编码（空串），并在菜单里**与文本三档并列**
    #[test]
    fn duckdb_formats_are_marked_and_not_encoded_locally() {
        let entry = entry(&["id"], &[&["1"]]);
        for format in [ExportFormat::Parquet, ExportFormat::Xlsx] {
            assert!(format.is_duckdb_backed(), "{} 要走 DuckDB", format.label());
            assert!(
                encode(&entry, format, "orders").is_empty(),
                "{} 不该在本地编码（空串 = 没东西可写）",
                format.label()
            );
            assert_eq!(
                default_file_name(Some("SELECT * FROM orders"), 1, format),
                format!("orders.{}", format.extension()),
                "默认文件名要跟着扩展名走"
            );
            assert!(!format.detail().is_empty(), "文件对话框说明不能空着");
        }
        for format in [
            ExportFormat::Csv,
            ExportFormat::Json,
            ExportFormat::Insert,
            ExportFormat::SqlUpdates,
            ExportFormat::WhereClause,
        ] {
            assert!(!format.is_duckdb_backed(), "{} 是本地编码", format.label());
        }
    }

    /// 菜单是**数据驱动**的：摆了哪些档只由「`ALL` 的次序 + 能力面」推出来
    ///
    /// `menu_items`（host.rs 今天读的那份）继续只给五档：接线时换成 `menu_items_for`，
    /// 菜单自己就长出这两档（面板不用认识具体档位）。
    #[test]
    fn sql_text_formats_grow_the_menu_only_with_capability() {
        use super::menu_items_for;

        let labels = |extras: &ExportExtras| -> Vec<String> {
            menu_items_for(false, "1,000", extras)
                .into_iter()
                .map(|item| item.label)
                .collect()
        };
        assert_eq!(
            labels(&ExportExtras::default()),
            ["CSV", "JSON", "INSERT", "Parquet", "XLSX"],
            "没有能力面时与 `menu_items` 一模一样（既有接线不受影响）"
        );
        assert_eq!(
            labels(&ExportExtras {
                sql_text: true,
                key: None,
            }),
            ["CSV", "JSON", "INSERT", "WHERE 子句", "Parquet", "XLSX"],
            "`WHERE 子句` 不要键：说了要 SQL 文本档就摆"
        );
        assert_eq!(
            labels(&ExportExtras {
                sql_text: true,
                key: Some(RowKey::single("id")),
            }),
            [
                "CSV",
                "JSON",
                "INSERT",
                "SQL Updates",
                "WHERE 子句",
                "Parquet",
                "XLSX"
            ],
            "有键列才摆 `SQL Updates`（没键就不摆一个只能回绝的入口）"
        );
        assert_eq!(
            labels(&ExportExtras {
                sql_text: false,
                key: Some(RowKey::single("id")),
            }),
            ["CSV", "JSON", "INSERT", "Parquet", "XLSX"],
            "`sql_text` 是这两档的总开关：只给键不算表态"
        );

        // 两档要跟着**两组范围**各长一遍，分组标题仍然不可点
        let items = menu_items_for(
            true,
            "1,000",
            &ExportExtras {
                sql_text: true,
                key: Some(RowKey::single("id")),
            },
        );
        assert_eq!(items.len(), 2 + 7 * 2, "两个标题 + 七档 × 两组范围");
        assert_eq!(
            items[4].action,
            Some((ExportFormat::SqlUpdates, ExportScope::Fetched))
        );
        assert_eq!(
            items[5].action,
            Some((ExportFormat::WhereClause, ExportScope::Fetched))
        );
        assert_eq!(
            items[12].action,
            Some((ExportFormat::SqlUpdates, ExportScope::All)),
            "第二组同样长出这两档"
        );
        assert!(items[8].separator_before, "“抓全量”那组前要有分隔线");
        assert!(items[0].action.is_none() && items[8].action.is_none());
    }
}
