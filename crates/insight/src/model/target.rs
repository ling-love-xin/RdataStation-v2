//! 洞察模型 · 取样来源与洞察目标（自 model.rs 拆分，纯位移）

use super::*;
/// 取样来源：洞察的入口五花八门（导航树 / 分析存档 / 草稿箱 / 编辑器结果集 /
/// CSV·Parquet·Excel 这类**文件**），但它们都能归约成「跑一段只读查询，取一份样本」。
///
/// **只装定位信息，不装数据**（D20 同一立场）：样本由接缝侧（`jobs`）执行，落在
/// `tmp_i_` 分析临时表上（建表即登记，TTL / 上限 / 回收都是现成的）。
///
/// `sql` 由**入口**构造（只有它知道该源的方言与引号规则）；抽样口径（`LIMIT`）
/// 不写在入口里——洞察统一包一层 `SELECT * FROM (…) AS … LIMIT n`，
/// 免得每个入口各写一次「取多少行」（口径一处分：「501 行样本」这种错最费时间）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleSource {
    /// 样本跑在哪条连接上。
    ///
    /// `None` = 跑在 **DuckDB 内存库**上——文件类（CSV / Parquet / Excel，靠 DuckDB 扩展
    /// 直接读）与已 `ATTACH` 进来的库表走这条：它们不属于任何「源库连接」，
    /// 但同样是可以分析的数据（用户口径：凡 DuckDB 能分析的都能洞察）。
    pub conn_id: Option<String>,
    /// 只读取样查询（不带 `LIMIT` 也可以：外层会再包一层）
    pub sql: String,
    /// 人类可读的来源描述（面板副标题 / 快照来源）：如「analytics.orders」
    pub label: String,
}

impl SampleSource {
    /// 源库连接上的取样（导航树 / 结果集 / 存档里的表）。
    pub fn new(
        conn_id: impl Into<String>,
        sql: impl Into<String>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            conn_id: Some(conn_id.into()),
            sql: sql.into(),
            label: label.into(),
        }
    }

    /// **DuckDB 内存库**上的取样：文件（CSV / Parquet / Excel）与已 `ATTACH` 的库表。
    pub fn on_duckdb(sql: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            conn_id: None,
            sql: sql.into(),
            label: label.into(),
        }
    }

    /// 文件来源：按扩展名选 DuckDB 的读取函数（与 `engine::file_reader_function`
    /// 同一处定义），路径做单引号转义。
    ///
    /// 不认识的扩展名直接报错，而不是猜一个读取器——猜错时用户看到的是
    /// 一句与「这个格式不支持」毫无关系的解析错误。
    pub fn duckdb_file(
        path: impl AsRef<std::path::Path>,
        label: impl Into<String>,
    ) -> Result<Self, shared::error::CoreError> {
        let path = path.as_ref();
        let display = path.display().to_string();
        let reader = engine::file_reader_function(&display).ok_or_else(|| {
            shared::error::CoreError::common(shared::error::CommonError::General(format!(
                "这个格式还不能分析：{display}（支持 CSV / Parquet / Excel / JSON）"
            )))
        })?;
        let escaped = display.replace('\'', "''");
        Ok(Self::on_duckdb(
            format!("SELECT * FROM {reader}('{escaped}')"),
            label,
        ))
    }

    /// 来源描述（面板副标题与快照 `entity_source` 用它）。
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 样本要跑在哪条连接上；`None` = DuckDB 内存库。
    pub fn conn_id(&self) -> Option<&str> {
        self.conn_id.as_deref()
    }
}

/// 面板当前指向的分析目标。
///
/// 只装**定位信息**（临时表名 / 连接 ID / 列名），不装数据：洞察不自己取数（D20），
/// 取数由宿主在收到目标后发起。带 [`SampleSource`] 的两个变体是同一个规则的延伸：
/// 入口只给「去哪个表取样本」，样本表由接缝侧统一建。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsightTarget {
    /// 列画像：结果表（临时表）中的一列
    Column {
        temp_table: String,
        column: String,
        data_type: String,
    },
    /// 表探查：结果表整体
    Table {
        temp_table: String,
        table_name: String,
    },
    /// 源表列画像：先取样，其余与 [`InsightTarget::Column`] 完全同路
    SourceColumn {
        source: SampleSource,
        column: String,
        data_type: String,
    },
    /// 源表表探查：先取样，其余与 [`InsightTarget::Table`] 完全同路
    SourceTable {
        source: SampleSource,
        table_name: String,
    },
    /// 多列分析
    MultiColumn {
        temp_table: String,
        columns: Vec<String>,
    },
    /// 结构目标（Schema 健康报告）。`database` 是必需的：分析器的表清单要按
    /// `table_catalog` 过滤（`information_schema` 里同名表可能属于不同库）。
    Schema {
        conn_id: String,
        database: String,
        schema: Option<String>,
    },
}

impl InsightTarget {
    /// 该目标默认落到哪个 Tab（入口点击后直接切过去）
    pub fn default_tab(&self) -> PanelTab {
        match self {
            InsightTarget::Column { .. } | InsightTarget::SourceColumn { .. } => PanelTab::Column,
            InsightTarget::Table { .. } | InsightTarget::SourceTable { .. } => PanelTab::Table,
            InsightTarget::MultiColumn { .. } => PanelTab::MultiColumn,
            InsightTarget::Schema { .. } => PanelTab::Schema,
        }
    }

    /// 目标头主标题
    pub fn title(&self) -> String {
        match self {
            InsightTarget::Column { column, .. } | InsightTarget::SourceColumn { column, .. } => {
                column.clone()
            }
            InsightTarget::Table { table_name, .. }
            | InsightTarget::SourceTable { table_name, .. } => table_name.clone(),
            InsightTarget::MultiColumn { columns, .. } => format!("{} 列", columns.len()),
            InsightTarget::Schema { schema, .. } => {
                schema.clone().unwrap_or_else(|| "全部结构".into())
            }
        }
    }

    /// 目标头副标题（定位信息：列的声明类型 / 临时表名 / 取样来源）
    pub fn detail(&self) -> Option<String> {
        match self {
            InsightTarget::Column {
                data_type,
                temp_table,
                ..
            } => Some(format!("{data_type} · {temp_table}")),
            InsightTarget::Table { temp_table, .. } => Some(temp_table.clone()),
            InsightTarget::SourceColumn {
                source, data_type, ..
            } => {
                // 类型可能在取样前还不知道（如编辑器结果集入口：行数据只有字符串化后的值）——
                // 那就只报来源，不摆一个空段
                let data_type = data_type.trim();
                if data_type.is_empty() {
                    Some(format!("来源 {}", source.label()))
                } else {
                    Some(format!("{data_type} · 来源 {}", source.label()))
                }
            }
            InsightTarget::SourceTable { source, .. } => Some(format!("来源 {}", source.label())),
            InsightTarget::MultiColumn { temp_table, .. } => Some(temp_table.clone()),
            InsightTarget::Schema { conn_id, .. } => Some(conn_id.clone()),
        }
    }

    /// 目标**自带**的临时表名；源目标还没有样本表（要取样后才有）→ 空串。
    ///
    /// 取数请求只认它：临时表三兄弟都指向临时表，视图因此不必逐个 `match`。
    /// 面板要拿「当前该分析哪张临时表」请用 `PanelData::sample_table()`——
    /// 源目标那里已经解析出了样本表。
    pub fn temp_table(&self) -> &str {
        match self {
            InsightTarget::Column { temp_table, .. }
            | InsightTarget::Table { temp_table, .. }
            | InsightTarget::MultiColumn { temp_table, .. } => temp_table,
            InsightTarget::SourceColumn { .. }
            | InsightTarget::SourceTable { .. }
            | InsightTarget::Schema { .. } => "",
        }
    }

    /// 源目标的取样来源（临时表目标为 `None`）。
    pub fn source(&self) -> Option<&SampleSource> {
        match self {
            InsightTarget::SourceColumn { source, .. }
            | InsightTarget::SourceTable { source, .. } => Some(source),
            _ => None,
        }
    }

    /// 展示用的表名（列 / 多列目标没有独立表名，退回临时表名）
    pub fn table_name(&self) -> String {
        match self {
            InsightTarget::Table { table_name, .. }
            | InsightTarget::SourceTable { table_name, .. } => table_name.clone(),
            _ => self.temp_table().to_string(),
        }
    }
}