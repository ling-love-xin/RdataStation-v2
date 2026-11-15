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
    /// 这条来源是不是**全局库里的表**（刀 7 · B）。
    ///
    /// 全局库（`<RDS_HOME>/data/system/global.db`）是 **SQLite** 文件、有自己的写入方，
    /// 所以不能 ATTACH 进进程单例。取样时另开一条**短命会话**只读挂它（别名 `rds_global`），
    /// 取完把结果搬进单例（`tmp_i_`，建完即登记）——`sql` 里写 `rds_global.<表>`。
    pub global: bool,
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
            global: false,
        }
    }

    /// **DuckDB 内存库**上的取样：文件（CSV / Parquet / Excel）与已 `ATTACH` 的库表。
    pub fn on_duckdb(sql: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            conn_id: None,
            sql: sql.into(),
            label: label.into(),
            global: false,
        }
    }

    /// **全局库**里的表（刀 7 · B）：`table` 是裸表名，SQL 由这里拼成
    /// `SELECT * FROM rds_global.<表>`（别名是内核固定常量，入口不该自己起名）。
    pub fn on_global(table: impl Into<String>, label: impl Into<String>) -> Self {
        let table = table.into();
        let quoted = table.replace('"', "\"\"");
        Self {
            conn_id: None,
            sql: format!("SELECT * FROM rds_global.\"{quoted}\""),
            label: label.into(),
            global: true,
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
        ))    }

    /// 来源描述（面板副标题与快照 `entity_source` 用它）。
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 样本要跑在哪条连接上；`None` = DuckDB 内存库。
    pub fn conn_id(&self) -> Option<&str> {
        self.conn_id.as_deref()
    }
}

/// 一次洞察分析要看的**一组来源**（联邦 / 多源取样的落点）。
///
/// 为什么另起一个类型而不把 `Vec` 塞进 `SampleSource`：单源是绝大多数入口的现状
/// （导航树点一张表），把「一个源」和「一组源」混成一个类型会让每个入口都去判断
/// 「我这是不是只有一个」，而它们根本不该关心这件事。
///
/// **本段的取样语义（口径已定）**：多源 = **每源各取一份样本**，各出一张 `tmp_i_` 表，
/// 不做跨源 join——取样是**取数据**，关联是**分析**。联邦会话真正被用来做 join 的情形
/// 留给「按 SQL 分析」那条链，不在这里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleSet {
    /// 会话 owner（联邦档按它取会话；空 owner 一律不建会话，见
    /// `engine::analysis::channel::AnalysisChannel::from_toggle`）
    pub owner: String,
    /// 来源清单（至少 1 个；空清单 = 调用方的错，服务层会拒）
    pub sources: Vec<SampleSource>,
    /// 用户是否打开了「联邦 / 多源」开关
    ///
    /// 注意它**不是**「一定要建联邦会话」：口径是「开关 + ≥2 源 + 有 owner」才升级
    /// （`from_toggle` 只升不降）；否则仍按单源走，行为与今天完全一致。
    pub federated_toggle: bool,
}

impl SampleSet {
    pub fn new(owner: impl Into<String>, sources: Vec<SampleSource>) -> Self {
        Self {
            owner: owner.into(),
            sources,
            federated_toggle: false,
        }
    }

    /// 打开 / 关闭多源开关（链式，便于入口一行写完）
    pub fn with_federated(mut self, on: bool) -> Self {
        self.federated_toggle = on;
        self
    }

    /// 这次分析该走哪个执行通道（通道判定只此一处，避免各入口各判一次）
    pub fn channel(&self) -> engine::analysis::channel::AnalysisChannel {
        engine::analysis::channel::AnalysisChannel::from_toggle(
            engine::analysis::channel::AnalysisChannel::ProcessDuckDb,
            self.federated_toggle,
            self.sources.len(),
            &self.owner,
        )
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
    /// 多源表探查：**一个面板 + 来源标签页**（多源取样的展示面）。
    ///
    /// 与 `SourceTable` 的区别只在「还有别的来源」：`active` 是当前标签页下标，
    /// 其余解析（取样、画像、下钻）与单源表目标完全同路——切标签页 = 换 `active`。
    /// 各来源的样本表由接缝侧回填进 `PanelData::source_samples`（按同一顺序对齐），
    /// 所以本变体自己不装临时表名。
    SourceTables {
        sources: Vec<SampleSource>,
        /// 当前标签页（始终 `< sources.len()`；构造时由 `new` 夹住）
        active: usize,
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
    /// 多源目标（≥2 个来源才值得开标签页；1 个来源的调用方应直接用 `SourceTable`）。
    ///
    /// 少于 2 个来源时返回 `None`：让入口立刻看出「这不该走多源路径」，
    /// 而不是在面板上画一排只有一个的标签。
    pub fn source_tables(
        sources: Vec<SampleSource>,
        table_name: impl Into<String>,
    ) -> Option<Self> {
        if sources.len() < 2 {
            return None;
        }
        Some(Self::SourceTables {
            sources,
            active: 0,
            table_name: table_name.into(),
        })
    }

    /// 切到第 `index` 个来源（越界返回 `None`；非多源目标也返回 `None`）。
    pub fn with_active_source(&self, index: usize) -> Option<Self> {
        match self {
            InsightTarget::SourceTables {
                sources, table_name, ..
            } if index < sources.len() => Some(InsightTarget::SourceTables {
                sources: sources.clone(),
                active: index,
                table_name: table_name.clone(),
            }),
            _ => None,
        }
    }

    /// 多源目标：全部来源 + 当前下标。
    pub fn multi_sources(&self) -> Option<(&[SampleSource], usize)> {
        match self {
            InsightTarget::SourceTables {
                sources, active, ..
            } => Some((sources.as_slice(), *active)),
            _ => None,
        }
    }

    /// 该目标默认落到哪个 Tab（入口点击后直接切过去）
    pub fn default_tab(&self) -> PanelTab {
        match self {
            InsightTarget::Column { .. } | InsightTarget::SourceColumn { .. } => PanelTab::Column,
            InsightTarget::Table { .. }
            | InsightTarget::SourceTable { .. }
            | InsightTarget::SourceTables { .. } => PanelTab::Table,
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
            // 多源：标题给「当前这一份」的来源名，第几份由 detail 报
            InsightTarget::SourceTables { sources, active, .. } => sources
                .get(*active)
                .map(|s| s.label().to_string())
                .unwrap_or_default(),
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
            // 多源：第几份 / 共几份 + 当前来源的表名（标签页本身只显示来源名）
            InsightTarget::SourceTables {
                sources,
                active,
                table_name,
            } => Some(format!(
                "{}/{} 来源 · {table_name}",
                active + 1,
                sources.len()
            )),
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
            // 多源一样的道理：样本表由接缝按来源下标回填，不装在目标里
            | InsightTarget::SourceTables { .. }
            | InsightTarget::Schema { .. } => "",
        }
    }

    /// 源目标的取样来源（临时表目标为 `None`）。
    pub fn source(&self) -> Option<&SampleSource> {
        match self {
            InsightTarget::SourceColumn { source, .. }
            | InsightTarget::SourceTable { source, .. } => Some(source),
            InsightTarget::SourceTables { sources, active, .. } => sources.get(*active),
            _ => None,
        }
    }

    /// 展示用的表名（列 / 多列目标没有独立表名，退回临时表名）
    pub fn table_name(&self) -> String {
        match self {
            InsightTarget::Table { table_name, .. }
            | InsightTarget::SourceTable { table_name, .. }
            | InsightTarget::SourceTables { table_name, .. } => table_name.clone(),
            _ => self.temp_table().to_string(),
        }
    }
}
