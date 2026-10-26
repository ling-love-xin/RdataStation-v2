//! 结果存储（**结果集的唯一权威**）
//!
//! 视图只是投影：结果网格从 `ResultStore` 读列与行，自己不缓存数据（架构 §4 状态地图、
//! 不变式"结果只有 `ResultStore`"）。这样将来加第二个视图（内联输出 / 导出 / 洞察）
//! 时不会出现"两个视图各存一份、对不上"。
//!
//! ## 每份文档一份**结果集列表**（B2）
//!
//! 原型 §2.4：「每次执行产生一个结果集」——批量执行每句一个、重查与分析派生新结果集
//! 而不就地覆盖。因此每份文档记的是 `Vec<ResultEntry>` + **当前选中项**：
//!
//! - [`ResultPlacement::Replace`]：替换**当前选中**的那一份（普通执行的既有语义）；
//! - [`ResultPlacement::NewSet`]：追加一份新的，**选中项不动**（原型 §4.4：别动用户在看的
//!   那一份，新的放旁边）——首次执行没有可保留的选中项时才落到新的一份上。
//!
//! 上限 [`MAX_RESULT_SETS`]（原型 §2.4：上限 5、超出淘汰最旧）淘汰的是**最旧的未选中项**：
//! 正在看的那一份永远不会被自己的下一个结果挤掉。
//!
//! ## 列类型（本切片）
//!
//! [`ResultEntry::column_types`] 记**每列的语义类型**（[`ColumnKind`]）：渲染据此决定数字右对齐、
//! 布尔 / 时间用等宽字体、表头显示类型小标签。三条口径：
//!
//! - **值本身不动**：仍按 B5 的展示文本（`NULL` 是字面量 `"NULL"`），类型只**追加**信息，
//!   TSV / 导出 / 筛选 / 排序 / 复制全部照旧；
//! - **缺类型 = 与今天完全一致**：类型缺失时渲染走「无类型」这一档（左对齐、不等宽、表头不加标签），
//!   不会因为“不知道类型”就错位；
//! - **今天驱动还填不上**：整条执行路径（`QueryData` → 这里的 `rows`）目前只带列名与行，驱动的
//!   `column_types` 在 `workbench::services::editor_exec::to_data` 就被丢掉了，所以真实结果集
//!   现在一律是**空 vec**。形状先立住：上游接上之后只需 [`ResultEntry::with_column_types`] 一处填值。

use ::shared::string::tsv_row;
use ::shared::FactFrame;
use std::sync::Arc;

use crate::changes::{ChangeSet, EditHistory};
use crate::export::RowKey;
use crate::rowset::{LocalRowSet, RowHistory};
use workbench_shell::model::{ColumnAnnotation, DocumentId, ExecChannel};

/// 结果落到哪里（B2）：执行族里「批量执行」与「在新结果标签中执行」的差别只在这一维
///
/// 2026-10-23 自 rds-editor 的 `execution.rs` 下移：它是「结果集放哪」的执行语义，
/// 与结果存储强绑定，editor 侧经 `crate::store` 重导出保持路径不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultPlacement {
    /// 替换当前选中的结果集（1a 口径：一次执行一份结果）
    Replace,
    /// 追加一个新结果集，**原结果集仍保持选中**（原型 §4.4：V1 常驻 `＋` 按钮的语义——
    /// “别动我正在看的东西，新的那份放旁边”）
    NewSet,
    /// 【B5b】把这一段**接在选中那份后面**（取下一段：不新开、不替换）
    ///
    /// 接的前提是列形状一致（同一份结果的下一段）；形状变了就当一份新结果落下。
    Append,
}

/// 【A2】一份结果的「形成方式」原料（刷新 ⟳ 时按它重查，保持用户看到的形态）
///
/// 下发筛选 / 排序下发的结果集，其 `sql` 仍是**原 SQL**（改写发生在执行器里，
/// 回填不拿改写后的语句）——原料不随结论回去的话，刷新会退回全量原始查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pushdown {
    /// 下发筛选：当初的筛选词 + 结果集的列（执行器拼 `WHERE` 用）
    Filtered { filter: String, columns: Vec<String> },
    /// 排序下发：当初点的列与方向
    SortedDown { column: String, descending: bool },
}

impl Pushdown {
    /// 从执行作业的下发载荷还原（`ExecJob` 的两个下发字段互斥：一次只有其一）
    pub fn from_job(
        filtered: &Option<(String, Vec<String>)>,
        sorted_down: &Option<(String, bool)>,
    ) -> Option<Self> {
        filtered
            .as_ref()
            .map(|(filter, columns)| Pushdown::Filtered {
                filter: filter.clone(),
                columns: columns.clone(),
            })
            .or_else(|| {
                sorted_down.as_ref().map(|(column, descending)| Pushdown::SortedDown {
                    column: column.clone(),
                    descending: *descending,
                })
            })
    }
}

/// 每份文档保留的结果集上限（原型 §2.4；超出淘汰最旧的**未选中**项）
pub const MAX_RESULT_SETS: usize = 5;

/// 一列的**语义类型**
///
/// 为什么不直接把驱动的类型名摆给渲染用：驱动报的是**方言名**（`bigint` / `Int64` /
/// `NUMBER(38,0)` / `numeric(38,10)` / `timestamp with time zone`），而渲染要回答的只有
/// 三类问题——“数字吗”（右对齐）、“要不要等宽”（布尔 / 时间 / UUID）、“按什么比大小”（将来）。
/// 两件事分开：名字照原样显示（用户看的是库里的类型），类别只用来做渲染决策。
///
/// **认不出就是 [`ColumnKind::Unknown`]**：宁可少着色，也不能猜错——`Unknown` 的渲染
/// 与“没有类型”完全一样。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColumnKind {
    /// 整数（`bigint` / `int4` / `Int64` / `serial` …）
    Integer,
    /// 浮点（`float8` / `double precision` / `Float64` / `real` …）
    Float,
    /// 定点小数（`numeric(38,10)` / `decimal` / `NUMBER` / `money` …）
    Decimal,
    Boolean,
    Text,
    /// 带时刻的时间戳（`timestamp` / `timestamptz` / `datetime` …）
    Timestamp,
    Date,
    Time,
    Uuid,
    Json,
    Binary,
    /// 驱动没报 / 类型名认不出（渲染与没有类型完全一致）
    #[default]
    Unknown,
}

impl ColumnKind {
    /// 从驱动 / 引擎报的类型名解析
    ///
    /// 认的是名字里的**词**而不是整串：四类库加 Arrow 的写法各不相同，而带参数
    /// （`varchar(255)`）、带后缀（`int unsigned`）、复合名（`timestamp with time zone`）
    /// 都得落进同一档。词级匹配而不是子串匹配也是**防误判**：`point` / `interval` 里
    /// 没有独立的 `int` 词，不会因为“看着像”被当成整数列。
    pub fn parse(name: &str) -> Self {
        for word in name
            .trim()
            .to_ascii_lowercase()
            .split(|ch: char| !ch.is_ascii_alphanumeric())
        {
            if word.is_empty() {
                continue;
            }
            // 容器 / 结构型的**内层**类型不是这一列的语义：`Dictionary(Int32, Utf8)` 的
            // `int32` 只是编码参数，整列判 `Unknown`（否则词典编码的文本列会被右对齐）。
            if matches!(
                word,
                "dictionary"
                    | "list"
                    | "struct"
                    | "map"
                    | "union"
                    | "extension"
                    | "interval"
                    | "duration"
            ) {
                return Self::Unknown;
            }
            if let Some(kind) = Self::from_word(word) {
                return kind;
            }
        }
        Self::Unknown
    }

    /// 单个词 → 类别（顺序即优先级：`timestamp` / `datetime` 要先于 `date` / `time`）
    fn from_word(word: &str) -> Option<Self> {
        // 整数族：列全（`int unsigned` 这种由前一个词命中；`uint64` 是 Arrow 的无符号族）
        const INTEGER_WORDS: [&str; 25] = [
            "int",
            "int2",
            "int4",
            "int8",
            "int16",
            "int32",
            "int64",
            "int128",
            "uint",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "integer",
            "smallint",
            "bigint",
            "tinyint",
            "mediumint",
            "serial",
            "bigserial",
            "smallserial",
            "serial2",
            "serial4",
            "serial8",
            "year",
        ];
        const TEXT_WORDS: [&str; 12] = [
            "text",
            "varchar",
            "nvarchar",
            "char",
            "nchar",
            "character",
            "string",
            "str",
            "utf8",
            "largeutf8",
            "clob",
            "citext",
        ];

        match word {
            "timestamp" | "timestamptz" | "datetime" | "datetime2" | "smalldatetime" => {
                Some(Self::Timestamp)
            }
            // `date` / `date32` / `date64`：`datetime` 已在上面拦下，不会走到这里
            w if w.starts_with("date") => Some(Self::Date),
            // `time` / `time64`（Arrow 带精度后缀）
            w if w.starts_with("time") => Some(Self::Time),
            "bool" | "boolean" => Some(Self::Boolean),
            "uuid" => Some(Self::Uuid),
            w if w.starts_with("json") => Some(Self::Json),
            w if w.contains("binary") || w.contains("blob") || w == "bytea" || w == "bytes" => {
                Some(Self::Binary)
            }
            w if w.starts_with("decimal") || w.starts_with("numeric") => Some(Self::Decimal),
            "number" | "money" | "dec" => Some(Self::Decimal),
            w if w.starts_with("float") || w.contains("double") => Some(Self::Float),
            "real" => Some(Self::Float),
            w if INTEGER_WORDS.contains(&w) => Some(Self::Integer),
            w if TEXT_WORDS.contains(&w) || w.starts_with("varchar") || w.starts_with("char") => {
                Some(Self::Text)
            }
            _ => None,
        }
    }

    /// 是数字吗（**右对齐的判据**：个位对齐才看得出一列的数量级差）
    pub fn is_numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Float | Self::Decimal)
    }

    /// 要不要用等宽字体渲染
    ///
    /// 这几类的**字形**靠等宽才对得整齐：`true` / `false`（同列落在同一个位置）、时间戳的
    /// 日期部分、UUID 的分段。不引新依赖：字体取主题的 `mono_font_family`
    /// （主题已经按平台挑了一个装得上的等宽字体）。
    pub fn is_monospace(self) -> bool {
        matches!(
            self,
            Self::Boolean | Self::Timestamp | Self::Date | Self::Time | Self::Uuid
        )
    }
}

/// 一列的类型：驱动报的**原始名字** + 解析出的**语义类别**
///
/// 两个都留：名字进表头（用户看的是库里的类型，不是我们的枚举），类别给渲染做决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnType {
    /// 驱动 / 引擎报的原始类型名（表头照原样显示：`bigint` / `numeric(38,10)`）
    pub name: String,
    /// 语义类别（[`ColumnKind::parse`] 的结果；认不出 = `Unknown`）
    pub kind: ColumnKind,
}

impl ColumnType {
    /// 从驱动的类型名造一条（解析**一次**就够：列数远小于格数）
    pub fn parse(name: impl Into<String>) -> Self {
        let name = name.into();
        let kind = ColumnKind::parse(&name);
        Self { name, kind }
    }
}

/// 一次执行在结果区的完整记录（= 一个结果集）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultEntry {
    pub document: DocumentId,
    /// 实际执行的 SQL（结果区标题与历史用；结果集标签的悬停摘要也用它）
    pub sql: String,
    /// 执行耗时（毫秒，驱动给的真实值）
    pub elapsed_ms: u64,
    /// 结果是否被截断（超出驱动行数上限）
    pub truncated: bool,
    /// 写语句的真实影响行数（B5；驱动没报就是 `None`，界面不编造）
    pub affected_rows: Option<u32>,
    /// 这份结果是**在哪个连接上**跑出来的（B5 结果工具栏要显示它；`None` = 没绑定）
    pub connection: Option<String>,
    /// 【B13】这份结果是**在哪个通道上**跑出来的（标签徽标 + 切通道后标灰都读它）
    pub channel: ExecChannel,
    /// 【B5b】这一段拿满了没有（拿满 = 可能还有下一段；界面据此摆「取下一段」）
    pub has_more: bool,
    /// 【B10】结果集标签的自定义标题（`None` = 界面按序号给“结果 N”）
    ///
    /// 用在“这份结果横竖不是普通查询输出”的场合（执行计划 / **分析**）——
    /// 用户看到标签就知道自己在看什么，而不用去悬停摘要里找。
    pub title: Option<String>,
    /// 【B15】这份结果是**本地 DuckDB 分析**的产物
    ///
    /// 为什么要有这个标记：分析结果基于**当时那份已抓到的行**，重跑它的 SQL 没有意义
    /// （临时表早就不在了）——界面据此**不摆「⟳ 刷新」**，而不是摆一个点了就错的按钮。
    pub analysis: bool,
    /// 【B15】血缘摘要：这份数据是**怎么来的**（原查询 / 下发筛选 / 排序下发 / 取下一段 / 本地分析）
    ///
    /// 原型 §2.4 要求结果集携带来源摘要：进到第三份结果后，用户得能看出哪份是重查、
    /// 哪份是本地分析（只靠标签序号是看不出来的）。界面在有自定义标题时用标题（执行计划 / 分析）。
    pub lineage: Option<String>,
    /// 【A2】这份结果的「形成方式」原料：刷新（⟳）时按它重查，**保持形态**
    ///
    /// 下发筛选 / 排序下发的结果集，其 `sql` 仍是**原 SQL**（改写发生在执行器里，
    /// 回填不拿改写后的语句）——不记原料的话，「⟳ 刷新」会退回全量原始查询，
    /// 用户看着筛选结果点刷新却丢了筛选。`None` = 普通执行（刷新 = 原 SQL 重跑）。
    pub pushdown: Option<Pushdown>,
    /// 【T2a-2】行标识：这份结果来源表的主键列（探测结果；`None` = 未探测 / 探测不到）
    ///
    /// 两条不变式：
    /// - `None` = 没探测到 → 判据层（`result_editable` 的 `NoRowKey`）把整份结果**置为只读**，
    ///   不编造行标识（写回靠主键 WHERE 定位行，猜一个键就是改错行的风险）；
    /// - `Some(vec![])` 不会出现：探测不到主键直接 `None`（有主键才有 `Some`）。
    ///   探测经宿主导航缓存（`CompletionPort::primary_key`，内存读），失败结果 / 写语句
    ///   （没有可编辑的行）不探测。
    pub key_columns: Option<Vec<String>>,
    /// 列名（失败时为空）
    pub columns: Vec<String>,
    /// 行数据（已字符串化；失败时为空）
    pub rows: Vec<Vec<String>>,
    /// **每列的类型**（与 `columns` 同序；失败时为空）
    ///
    /// 两条不变式：
    /// - **空 vec = 驱动没报类型**（今天所有真实结果都是这一档）→ 界面按“无类型”渲染，
    ///   行为与没有这个字段时**完全一致**；
    /// - 非空时**与 `columns` 等长**，这一列没报就是 `None`（[`ResultEntry::with_column_types`] 保证）。
    pub column_types: Vec<Option<ColumnType>>,
    /// 【P1】**每列的元数据注释**（血缘 + 表列元数据；与 `columns` 同序；失败时为空）
    ///
    /// 两条不变式：
    /// - **空 vec = 宿主没报注释**（未接注释端口 / 表达式列）→ 表头只画列名，与今天一致；
    /// - 非空时**与 `columns` 等长**，这一列没注释就是 `None`（[`ResultEntry::with_column_annotations`] 保证）。
    pub column_annotations: Vec<Option<ColumnAnnotation>>,
    /// 失败原因；`None` = 成功
    pub error: Option<String>,
}

impl ResultEntry {
    /// 这份结果能不能做「洞察此列」（列头右键的入口只在这时为真）
    ///
    /// 三条要满足：
    /// - 成功（失败的结果没有可分析的列）
    /// - 有列（写语句 / DDL 的结论是影响行数，没有列）
    /// - SQL 是**只读查询**：口径与引擎一致（用 `SqlEngine` 的语句分类，不自写判断），
    ///   避免把 INSERT / DDL 重放一遍
    ///
    /// **连接不在这里管**：文档可以未绑定（执行时跟随当前活动连接），而“活动连接是谁”
    /// 只有执行器知道（`QueryRunner::active_connection`）——那一半由面板合起来判定。
    pub fn can_insight_column(&self) -> bool {
        self.error.is_none() && !self.columns.is_empty() && Self::is_read_only_query(&self.sql)
    }

    /// SQL 是不是只读查询（走引擎的语句分类：口径只此一处）
    fn is_read_only_query(sql: &str) -> bool {
        let (statement_type, _normalized) =
            engine::SqlEngine::parse_and_route(sql, engine::SqlDialect::Ansi);
        matches!(statement_type, engine::SqlStatementType::Select)
    }

    /// 成功的执行结果
    pub fn success(
        document: DocumentId,
        sql: String,
        elapsed_ms: u64,
        truncated: bool,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    ) -> Self {
        Self {
            document,
            sql,
            elapsed_ms,
            truncated,
            affected_rows: None,
            connection: None,
            // 认不出通道时按源库算（默认档；真实通道由 `entry_from` 从结论里带上）
            channel: ExecChannel::default(),
            // 一次拿完的路径（非分段）没有“下一段”可言；分段抓取由 `has_more` 另行标
            has_more: truncated,
            title: None,
            analysis: false,
            lineage: None,
            // 【T2a-2】主键探测在回执路径回填（`host` 的 `probe_primary_key`）：构造时如实未知
            key_columns: None,
            columns,
            rows,
            // 类型由上游补（`with_column_types`）：默认空 = “没报”，渲染与今天一致
            column_types: Vec::new(),
            // 【P1】注释由回执路径补（`with_column_annotations`）：默认空 = “没报”
            column_annotations: Vec::new(),
            // 【A2】形成原料由回执路径补（`entry_from` 的 `with_pushdown`）：默认 None = 普通执行
            pushdown: None,
            error: None,
        }
    }

    /// 带上每列的类型（驱动的 `column_types` 原样给；与 `columns` 同序）
    ///
    /// - 空切片 = 驱动没报 → 保持空（不是“一列一个空类型”：界面据此区分“没报”与“报了但认不出”）；
    /// - 名字比列数少 → 缺的填 `None`；比列数多 → 多的丢掉（形状以 `columns` 为准）。
    pub fn with_column_types(mut self, names: &[String]) -> Self {
        if !names.is_empty() && !self.columns.is_empty() {
            self.column_types = (0..self.columns.len())
                .map(|index| {
                    names
                        .get(index)
                        .map(|name| name.trim())
                        .filter(|name| !name.is_empty())
                        .map(ColumnType::parse)
                })
                .collect();
        }
        self
    }

    /// 【P1】带上每列的元数据注释（宿主端口回填；与 `columns` 同序）
    ///
    /// - 空切片 = 宿主没报 → 保持空（不是“一列一个 None”：界面据此区分“没报”与“报了但没注释”）；
    /// - 注释比列数少 → 缺的填 `None`；比列数多 → 多的丢掉（形状以 `columns` 为准）。
    pub fn with_column_annotations(mut self, annotations: &[Option<ColumnAnnotation>]) -> Self {
        if !annotations.is_empty() && !self.columns.is_empty() {
            self.column_annotations = (0..self.columns.len())
                .map(|index| annotations.get(index).cloned().flatten())
                .collect();
        }
        self
    }

    /// 【T2a-2】这份结果的行定位依据：主键列里**结果集真选中的那几列**（过滤后非空才给）
    ///
    /// 为什么过滤：写回生成的 `UPDATE … WHERE key=…` 要拿**本行结果里的值**绑到键上，
    /// 结果没 SELECT 主键列（比如 `SELECT name FROM orders`）就没有可绑的值——
    /// 返回 `None`，判据层（`NoRowKey`）如实置只读，而不是生成一条定位不到行的 SQL。
    pub fn row_key(&self) -> Option<RowKey> {
        let key_columns = self.key_columns.as_deref()?;
        let present = key_columns
            .iter()
            .filter(|name| self.columns.iter().any(|column| column == *name))
            .cloned();
        RowKey::new(present)
    }

    /// 第 `col` 列（**数据列**下标）的类型；没报 / 越界 = `None`
    pub fn column_type(&self, col: usize) -> Option<&ColumnType> {
        self.column_types.get(col).and_then(Option::as_ref)
    }

    /// 第 `col` 列的语义类别（没报 = `Unknown`：渲染口径与“没有类型”一致）
    pub fn column_kind(&self, col: usize) -> ColumnKind {
        self.column_type(col)
            .map_or(ColumnKind::Unknown, |column| column.kind)
    }

    /// 带上写语句的影响行数（结果区显示「影响 N 行」；没有就不要填）
    pub fn with_affected_rows(mut self, affected_rows: Option<u32>) -> Self {
        self.affected_rows = affected_rows;
        self
    }

    /// 带上这份结果的来源连接（结果工具栏显示它；`None` = 当时没绑定连接）
    pub fn with_connection(mut self, connection: Option<String>) -> Self {
        self.connection = connection;
        self
    }

    /// 【B13】带上这份结果的来源通道（标签徽标与“切通道即失效”的标灰读它）
    pub fn with_channel(mut self, channel: ExecChannel) -> Self {
        self.channel = channel;
        self
    }

    /// 【B5b】标上“这一段拿满了没有”（「取下一段」能不能按就靠它）
    pub fn with_has_more(mut self, has_more: bool) -> Self {
        self.has_more = has_more;
        self
    }

    /// 【B15】标记为分析结果（本地 DuckDB 产物；界面据此不摆“刷新”）
    pub fn with_analysis(mut self, analysis: bool) -> Self {
        self.analysis = analysis;
        self
    }

    /// 【B15】贴血缘摘要（“原查询” / “下发筛选” / …；工具栏的来源段读它）
    pub fn with_lineage(mut self, lineage: impl Into<String>) -> Self {
        self.lineage = Some(lineage.into());
        self
    }

    /// 【A2】带上「形成方式」原料（下发筛选 / 排序下发的结果集在刷新时按它重查，
    /// 保持用户看到的形态；普通执行不填 = 刷新跑原 SQL）
    pub fn with_pushdown(mut self, pushdown: Option<Pushdown>) -> Self {
        self.pushdown = pushdown;
        self
    }

    /// 【B10】给结果集贴一个标题（如「执行计划」；不贴就是“结果 N”）
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// 失败的执行
    pub fn failure(document: DocumentId, sql: String, error: String, elapsed_ms: u64) -> Self {
        Self {
            document,
            sql,
            elapsed_ms,
            truncated: false,
            affected_rows: None,
            connection: None,
            channel: ExecChannel::default(),
            has_more: false,
            title: None,
            analysis: false,
            lineage: None,
            key_columns: None,
            columns: Vec::new(),
            rows: Vec::new(),
            column_types: Vec::new(),
            // 【P1】失败结果没有列可注释，如实空着
            column_annotations: Vec::new(),
            // 【A2】失败结果没有可保持的形态（不能重查），同样如实 None
            pushdown: None,
            error: Some(error),
        }
    }

    /// 这份结果还能不能取下一段（原型 §2.4 的 ⑦：分页 / 取下一段）
    pub fn can_fetch_more(&self) -> bool {
        self.error.is_none() && self.has_more
    }

    /// 【B5b】这一段还能不能接在这份结果后面（列形状一致 / 这一份本身是成功的）
    ///
    /// 判据单独拿出来是为了**先判后搬**：形状对不上就不该先把整段行拷一遍再发现接不上，
    /// 对得上则可以直接把行**移**过来（10 万行 × N 列的深拷贝省在“取下一段”这条路上）。
    pub fn accepts_columns(&self, columns: &[String]) -> bool {
        self.error.is_none() && self.columns == columns
    }

    /// 【B5b】把新抓到的这一段接在后面（取下一段）
    ///
    /// 调用方必须先过 [`ResultEntry::accepts_columns`]（形状不一致时这句会写进错的行形状里）。
    /// 行按**移动**接收（不拷贝）；耗时取**累加**（抓取这份结果总共花了多久）；`has_more`
    /// 以最后一段为准。
    pub fn append_rows(&mut self, rows: Vec<Vec<String>>, elapsed_ms: u64, has_more: bool) {
        self.rows.extend(rows);
        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);
        self.has_more = has_more;
    }

    /// 这一份原本没带类型、而新的一段带来了就补上（**只在真缺时补**，不覆盖已有的）
    ///
    /// 为什么是“补”而不是“换”：类型是**列**的属性，分段抓取的每一段列形状都相同
    /// （[`ResultEntry::accepts_columns`]），后一段的类型不会比前一段更权威。
    fn adopt_column_types(&mut self, types: Vec<Option<ColumnType>>) {
        if self.column_types.is_empty() && !types.is_empty() {
            self.column_types = types;
        }
    }

    /// 结果行数（真实值：来自行数据，不读驱动的 `total_rows` 字段，见架构 §12 #21）
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 【T2a】提交成功后把这一格的新值写进行数据（**本地面应用**）
    ///
    /// 与"重查刷新"的区别：本地应用不丢排序 / 筛选 / 选中，且回执已确认落库成功。
    /// 行 / 列越界返回 `false`（调用方应已校验过；宁可不改也不改错行）。
    pub fn apply_cell(&mut self, row: usize, column: usize, value: impl Into<String>) -> bool {
        let Some(row_data) = self.rows.get_mut(row) else {
            return false;
        };
        let Some(cell) = row_data.get_mut(column) else {
            return false;
        };
        *cell = value.into();
        true
    }

    /// 是否可渲染成网格（成功且有列）
    pub fn has_grid(&self) -> bool {
        self.error.is_none() && !self.columns.is_empty()
    }

    /// 是不是失败的结果集（结果集标签的 `danger` 点用它）
    pub fn failed(&self) -> bool {
        self.error.is_some()
    }

    /// 是不是「只有影响行数、没有结果集」的写语句（结果区要换成一句文案）
    pub fn affects_rows_only(&self) -> bool {
        self.error.is_none() && self.columns.is_empty() && self.affected_rows.is_some()
    }

    /// 结果集文本（TSV：表头一行 + 每行一条；复制到剪贴板用）
    ///
    /// 逐格转义走 `shared::string::tsv_cell`（制表符 / 换行 / 双引号才包裹并双写内部引号）
    /// ——不转义的话带制表符的值会把列错开（Excel / DBeaver 都按这个写法读）。
    /// 这份规则**只有 shared 一份实现**：结果网格的右键复制与 mock 预览取样都调它。
    ///
    /// 导出的是**已抓到的行**（被截断的那份就只有前若干行）；没有网格时返回空串
    /// （调用方应先看 [`ResultEntry::has_grid`]）。其余导出格式属 B7（`persist.rs`）。
    pub fn to_tsv(&self) -> String {
        if self.columns.is_empty() {
            return String::new();
        }
        let mut text = tsv_row(&self.columns);
        for row in &self.rows {
            text.push('\n');
            text.push_str(&tsv_row(row));
        }
        text
    }

    /// 结果区状态行（真实值，无占位文案）
    pub fn summary(&self) -> String {
        if let Some(error) = &self.error {
            return format!("执行失败：{error}");
        }
        // 写语句没有结果集，只能报影响行数（B5）
        if let Some(affected) = self.affected_rows {
            let mut text = format!("影响 {affected} 行 · {} ms", self.elapsed_ms);
            if self.truncated {
                text.push_str(" · 已截断");
            }
            return text;
        }
        let mut text = format!("{} 行 × {} 列 · {} ms", self.row_count(), self.columns.len(), self.elapsed_ms);
        if self.truncated {
            text.push_str(" · 已截断");
        }
        text
    }
}

/// 【A2】事实档载体状态（L0 事实层；方案 §4.1 三态状态机）
///
/// 与 [`ResultEntry`] 平行存放在 `DocumentResults.facts`，**不进 `ResultEntry`**：
/// `Arc<FactFrame>` 深拷贝代价高，挂进 `ResultEntry` 的 `Clone` / `PartialEq` 会触发
/// 逐值比较与整帧深拷贝。本枚举同样不 derive `Clone`，槽位整体替换 / 淘汰。
///
/// - [`FactState::None`]：失败结果 / 写语句 / 加速档 / 联邦档 / JSON 往返 —— 显示层走
///   internal 猜型降级档，界面与报告显式标注；
/// - [`FactState::InMemory`]：内存档，持有共享事实帧；触发式物化（分析 / 带类型导出 /
///   IronCalc 快照）后常驻复用，重复分析不重建；
/// - [`FactState::OnDuckDb`]：表档（B2 接通）：常驻表已物化，帧上挂表名且 batches 已
///   释放，只保留 schema / 列名 / 行数供元数据与类型族访问；表档上的本地交互一律 SQL
///   下推，不把表读回 Rust 堆（方案 §4.1 表档边界）。
///
/// 【A3】分段追加不改动帧的 `Arc` 所有权：可追加分支把旧段 + 新段 batches 合并后重建帧
/// （[`FactFrame::new`] 统一做形状校验；校验失败整档作废 `None`），不要求 `FactFrame: Clone`。
#[derive(Debug)]
pub enum FactState {
    None,
    InMemory(Arc<FactFrame>),
    OnDuckDb(Arc<FactFrame>),
}

impl FactState {
    /// 无事实档起步态（A2：上游 A3 尚未接入事实帧，入槽一律 `None`）
    pub const fn none() -> Self {
        Self::None
    }

    /// 从回执事实候选落档：`Some` → 内存档；`None` → 无事实档（显示矩阵降级档）
    pub fn from_fact(fact: Option<Arc<FactFrame>>) -> Self {
        match fact {
            Some(frame) => Self::InMemory(frame),
            None => Self::None,
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// 内存档事实帧引用；表档 / 无事实档返回 `None`
    pub fn in_memory(&self) -> Option<&FactFrame> {
        match self {
            Self::InMemory(frame) => Some(frame),
            _ => None,
        }
    }

    /// 表档事实帧引用；内存档 / 无事实档返回 `None`
    pub fn on_duckdb(&self) -> Option<&FactFrame> {
        match self {
            Self::OnDuckDb(frame) => Some(frame),
            _ => None,
        }
    }

    /// 统一取事实帧的 `Arc` 句柄（内存档 / 表档都给；无事实档 = `None`）。
    /// 调用方克隆 `Arc` 出借用边界后只读 schema / 列名 / 行数 / 表名；取行列值必须先区分
    /// 载体（表档不回读 Rust 堆）。
    pub fn fact(&self) -> Option<&Arc<FactFrame>> {
        match self {
            Self::InMemory(frame) | Self::OnDuckDb(frame) => Some(frame),
            Self::None => None,
        }
    }

    /// 常驻表名（`None` = 尚未物化；内存档挂表名后也会回这里）。
    pub fn temp_table(&self) -> Option<&str> {
        self.fact().and_then(|frame| frame.temp_table())
    }

    /// 当前载体标签（状态栏 / 右栏占用可见性展示用；方案 §4.1 三态对齐）。
    pub fn tier(&self) -> ::shared::CurrentTier {
        match self {
            Self::None => ::shared::CurrentTier::None,
            Self::InMemory(_) => ::shared::CurrentTier::InMemory,
            Self::OnDuckDb(_) => ::shared::CurrentTier::OnDuckDb,
        }
    }

    /// 已占用的事实档估算字节：内存档按列式 buffers 估算；表档 batches 已释放，
    /// Rust 堆占用报 0（表内占用归 DuckDB 侧统计，不在这里估）；无事实档报 0。
    pub fn estimated_bytes(&self) -> usize {
        match self {
            Self::InMemory(frame) => frame.estimated_bytes(),
            _ => 0,
        }
    }
}

/// 【A3】可追加段事实档合并（方案跳 2：Append 可追加分支新段 batches 并入现有帧）
///
/// - 新段 `None`（无 batches 回执）：旧档原样保留，不抹掉已有事实帧；
/// - 旧档 `None` + 新段 `Some`：直接挂新帧；
/// - 旧内存档 + 新段 `Some`：两段 batches 合并后由 [`FactFrame::new`] 统一形状校验，
///   校验失败整档作废 `None`（不保留可能错位的旧帧）；
/// - 旧表档 + 新段 `Some`：第一期保守落**替换**——不向已有常驻表 appender 追加
///   （表档追加需复用原表 schema 与 Appender 句柄，属后置细化项）；新段 `None` 时表档保留。
fn merge_fact_state(current: &mut FactState, incoming: Option<Arc<FactFrame>>) {
    let Some(incoming) = incoming else {
        return;
    };
    match current {
        FactState::InMemory(old) => {
            let mut batches = old.batches().to_vec();
            batches.extend(incoming.batches().iter().cloned());
            match FactFrame::new(old.columns().to_vec(), batches) {
                Ok(merged) => *current = FactState::InMemory(Arc::new(merged)),
                Err(_) => *current = FactState::none(),
            }
        }
        FactState::None => *current = FactState::InMemory(incoming),
        FactState::OnDuckDb(_) => *current = FactState::InMemory(incoming),
    }
}

/// 一份文档的结果集（列表 + 选中项 + 对比项）
#[derive(Debug)]
struct DocumentResults {
    document: DocumentId,
    sets: Vec<ResultEntry>,
    /// 【T2a-3】每份结果集的**未提交变更**（与 `sets` 同序同增减；选中项切换不清空——
    /// 用户切标签看别的结果，回来编辑还在。清空只发生在结果被替换 / 淘汰 / 已提交时）
    changes: Vec<ChangeSet>,
    /// 【撤销】每份结果集的**编辑历史**（撤销 / 重做双栈；与 `changes` 同序同增减——
    /// 历史属于“这次抓到的数据”，结果被替换 / 淘汰时作废，切标签不清空）
    histories: Vec<EditHistory>,
    /// 【#28 行级叠加层】每份结果集的本地行集合 + 删除标记（与 `sets` 同序同增减；
    /// 生命周期同格级变更集：结果被替换 / 淘汰时作废，切标签不清空）
    locals: Vec<LocalRowSet>,
    /// 【#28】每份结果集的**行级历史**（新增 / 复制 / 删除双栈；与格级 `histories`
    /// 并列、互不混栈；同序同增减，结果作废时清空）
    row_histories: Vec<RowHistory>,
    /// 【A2】每份结果集的事实档状态（与 `sets` 同序同增减；替换 / 淘汰时随结果作废，
    /// 切标签不清空）。渲染第一期不读它，分析 / 导出按它选保真路径（A3 接线后生效）
    facts: Vec<FactState>,
    /// 当前选中的结果集下标（始终指向 `sets` 中的有效位置；`sets` 非空时必有）
    active: usize,
    /// 【并排对比】对比选中的结果集下标（`None` = 未开对比；`Some` ≠ `active` 恒成立）
    ///
    /// 对比模式把 `active` 与 `compare` 两份结果并排渲染。切换 `active` 时若切到
    /// `compare` 本身（自己对比自己没有意义），对比项自动清空；`compare` 指向的那份
    /// 被淘汰时也自动清空（它不在了，对比无从谈起）。
    compare: Option<usize>,
}

/// 结果集集合（按文档）
#[derive(Debug, Default)]
pub struct ResultStore {
    documents: Vec<DocumentResults>,
}

impl ResultStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一条结果（落位见 [`ResultPlacement`]）
    ///
    /// `fact` 为回执事实帧候选（方案跳 1：无 batches 回执为 `None`）；分段追加时同形状并入
    /// 当前事实帧，形状不一致随替换分支作废。
    pub fn push(
        &mut self,
        entry: ResultEntry,
        placement: ResultPlacement,
        fact: Option<Arc<FactFrame>>,
    ) {
        let document = entry.document.clone();
        let slot = match self
            .documents
            .iter_mut()
            .find(|slot| slot.document == document)
        {
            Some(slot) => slot,
            None => {
                self.documents.push(DocumentResults {
                    document,
                    sets: Vec::new(),
                    changes: Vec::new(),
                    histories: Vec::new(),
                    locals: Vec::new(),
                    row_histories: Vec::new(),
                    facts: Vec::new(),
                    active: 0,
                    compare: None,
                });
                self.documents.last_mut().expect("刚插入")
            }
        };

        match placement {
            // 替换当前选中的那一份（第一次执行时为唯一的一份）
            ResultPlacement::Replace => {
                if let Some(current) = slot.sets.get_mut(slot.active) {
                    *current = entry;
                    // 【T2a-3】结果被覆盖：旧变更集随旧结果一起作废（新结果是新数据）
                    if let Some(changes) = slot.changes.get_mut(slot.active) {
                        *changes = ChangeSet::new();
                    }
                    // 【撤销】同理：旧结果的编辑历史随结果作废
                    if let Some(history) = slot.histories.get_mut(slot.active) {
                        history.clear();
                    }
                    // 【#28】本地叠加层与行级历史随旧结果一起作废
                    if let Some(local) = slot.locals.get_mut(slot.active) {
                        local.clear();
                    }
                    if let Some(row_history) = slot.row_histories.get_mut(slot.active) {
                        row_history.clear();
                    }
                    // 【A3】旧事实档随旧结果作废：新结果是新数据，旧帧没有可归属的对象；
                    // 新回执带事实帧候选则挂上，无候选（DML / 加速档 / 联邦档）走降级档。
                    if let Some(fact_state) = slot.facts.get_mut(slot.active) {
                        *fact_state = FactState::from_fact(fact.clone());
                    }
                } else {
                    slot.sets.push(entry);
                    slot.changes.push(ChangeSet::new());
                    slot.histories.push(EditHistory::new());
                    slot.locals.push(LocalRowSet::new());
                    slot.row_histories.push(RowHistory::new());
                    slot.facts.push(FactState::from_fact(fact));
                    slot.active = 0;
                }
            }
            // 追加一份新的，选中项**不动**；只有“还没有任何结果”时新结果才需要被选中
            ResultPlacement::NewSet => {
                slot.sets.push(entry);
                slot.changes.push(ChangeSet::new());
                slot.histories.push(EditHistory::new());
                slot.locals.push(LocalRowSet::new());
                slot.row_histories.push(RowHistory::new());
                slot.facts.push(FactState::from_fact(fact));
                if slot.sets.len() == 1 {
                    slot.active = 0;
                }
            }
            // 【B5b】接在选中那份后面（取下一段）：列形状不一致时落成一份新结果
            ResultPlacement::Append => {
                // 失败的那次取段不入库（调用方应当已经把它变成一句提示）：
                // 已经抓到的行是用户的成果，不能因为“再抓失败”就清掉
                if entry.failed() {
                    return;
                }
                // **先判形状，再搬行**：对得上就把这一段**移**进去（换掉原先无条件
                // `entry.rows.clone()` 的整段深拷贝），对不上则整条 entry 原封不动落下。
                let appendable = slot
                    .sets
                    .get(slot.active)
                    .is_some_and(|current| current.accepts_columns(&entry.columns));
                if appendable {
                    let current = slot.sets.get_mut(slot.active).expect("刚判过这一份在");
                    current.append_rows(entry.rows, entry.elapsed_ms, entry.has_more);
                    current.adopt_column_types(entry.column_types);
                    // 【A3】事实档同形状并入：旧段 + 新段 batches 合并重建；新段无 batches 回执保留旧档
                    if let Some(fact_state) = slot.facts.get_mut(slot.active) {
                        merge_fact_state(fact_state, fact);
                    }
                } else if let Some(current) = slot.sets.get_mut(slot.active) {
                    *current = entry;
                    // 同上：形状对不上落成替换，旧变更作废
                    if let Some(changes) = slot.changes.get_mut(slot.active) {
                        *changes = ChangeSet::new();
                    }
                    // 【撤销】旧编辑历史随替换作废
                    if let Some(history) = slot.histories.get_mut(slot.active) {
                        history.clear();
                    }
                    // 【#28】本地叠加层与行级历史随替换作废
                    if let Some(local) = slot.locals.get_mut(slot.active) {
                        local.clear();
                    }
                    if let Some(row_history) = slot.row_histories.get_mut(slot.active) {
                        row_history.clear();
                    }
                    // 【A3】形状对不上 = 新结果：旧帧不再归属本段，挂新回执事实候选
                    if let Some(fact_state) = slot.facts.get_mut(slot.active) {
                        *fact_state = FactState::from_fact(fact.clone());
                    }
                } else {
                    slot.sets.push(entry);
                    slot.changes.push(ChangeSet::new());
                    slot.histories.push(EditHistory::new());
                    slot.locals.push(LocalRowSet::new());
                    slot.row_histories.push(RowHistory::new());
                    slot.facts.push(FactState::from_fact(fact));
                    slot.active = 0;
                }
            }
        }
        evict_oldest(slot);
    }

    /// 某文档的结果集（按产生顺序；没有则空切片）
    pub fn sets(&self, document: &DocumentId) -> &[ResultEntry] {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .map(|slot| slot.sets.as_slice())
            .unwrap_or(&[])
    }

    /// 某文档结果集的条数
    pub fn set_count(&self, document: &DocumentId) -> usize {
        self.sets(document).len()
    }

    /// 某文档当前选中的结果集（结果网格显示的就是这一份）
    pub fn active(&self, document: &DocumentId) -> Option<&ResultEntry> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.sets.get(slot.active))
    }

    /// 某文档当前选中的下标（结果集标签条用它画选中态）
    pub fn active_index(&self, document: &DocumentId) -> Option<usize> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document && !slot.sets.is_empty())
            .map(|slot| slot.active)
    }

    /// 【A2】指定文档第 `index` 份结果的事实档状态（越界 / 文档不存在 = `None`）。
    /// 渲染第一期不读它；A3 接线后分析 / 带类型导出按它选保真路径。
    pub fn fact_at(&self, document: &DocumentId, index: usize) -> Option<&FactState> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.facts.get(index))
    }

    /// 【A2】当前选中结果的事实档状态（与 `active` 同槽位同序）
    pub fn active_fact(&self, document: &DocumentId) -> Option<&FactState> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.facts.get(slot.active))
    }

    /// 【B2】物化回写：把常驻表名挂到指定槽位事实帧（分析 / 带类型导出直灌成功后，
    /// 由主线程回填路径调用；重复分析据此复用同一张表、不重建）。
    ///
    /// - 内存档帧：重建一份挂表名的新帧替换槽位句柄（`Arc` 内帧不可变，不能原地改）；
    /// - 表档帧：表名以既有句柄为准，不再二次变更；
    /// - 无事实档 / 越界返回 `false`。
    pub fn set_fact_table(&mut self, document: &DocumentId, index: usize, table: String) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(state) = slot.facts.get_mut(index) else {
            return false;
        };
        match state {
            FactState::None => false,
            FactState::OnDuckDb(_) => true,
            FactState::InMemory(old) => {
                let Ok(mut frame) =
                    FactFrame::new(old.columns().to_vec(), old.batches().to_vec())
                else {
                    return false;
                };
                frame.set_temp_table(table);
                *state = FactState::InMemory(Arc::new(frame));
                true
            }
        }
    }

    /// 【B2】把指定槽位内存档提升为表档：挂常驻表名后释放 batches，状态切 [`FactState::OnDuckDb`]。
    /// 只对内存档生效；无事实档 / 已是表档返回 `false`（已是表档视为成功复用）。
    pub fn promote_fact_to_table(
        &mut self,
        document: &DocumentId,
        index: usize,
        table: String,
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(state) = slot.facts.get_mut(index) else {
            return false;
        };
        match state {
            FactState::None => false,
            FactState::OnDuckDb(_) => true,
            FactState::InMemory(old) => {
                let Ok(mut frame) =
                    FactFrame::new(old.columns().to_vec(), old.batches().to_vec())
                else {
                    return false;
                };
                frame.set_temp_table(table);
                frame.release_batches();
                *state = FactState::OnDuckDb(Arc::new(frame));
                true
            }
        }
    }

    /// 选中某个结果集（标签条点击；下标越界 = 不改，返回是否真的改了）
    ///
    /// **变更集不因切换选中而清空**：变更集按结果集存（`changes` 与 `sets` 同序），
    /// 换标签看别的结果、再切回来，这份的编辑还在——清空只发生在结果被覆盖 / 淘汰 / 提交后。
    pub fn select(&mut self, document: &DocumentId, index: usize) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        if index >= slot.sets.len() || index == slot.active {
            return false;
        }
        slot.active = index;
        // 【并排对比】切到对比项本身 = 自己对比自己，对比失效
        if slot.compare == Some(index) {
            slot.compare = None;
        }
        true
    }

    /// 【并排对比】当前对比选中的下标（`None` = 未开对比；与 `active` 恒不同）
    pub fn compare_index(&self, document: &DocumentId) -> Option<usize> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.compare)
    }

    /// 【并排对比】对比选中的那份结果（并排渲染读它；未开对比 = `None`）
    pub fn compare_entry(&self, document: &DocumentId) -> Option<&ResultEntry> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.compare.and_then(|index| slot.sets.get(index)))
    }

    /// 【并排对比】把第 `index` 份结果设为对比项（标签条「设为对比」；返回是否真的改了）
    ///
    /// 约束：不能是 `active` 本身（自己对比自己没有意义）、下标必须有效。
    /// 同一份结果集作为对比项**没有先决条件**——失败 / 写语句的结果也可并排对照
    /// （错误信息并排看也是对比的一部分），不摆网格与否由渲染层各自决定。
    pub fn select_compare(&mut self, document: &DocumentId, index: usize) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        if index >= slot.sets.len() || index == slot.active {
            return false;
        }
        slot.compare = Some(index);
        true
    }

    /// 【并排对比】点同一份 = 取消对比（标签条再次点击；返回是否真的清掉了）
    pub fn toggle_compare(&mut self, document: &DocumentId, index: usize) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        if slot.compare == Some(index) {
            slot.compare = None;
            return true;
        }
        if index < slot.sets.len() && index != slot.active {
            slot.compare = Some(index);
            return true;
        }
        false
    }

    /// 【T2a-3】某份结果集的**未提交变更**（只读：UI 标脏格 / 变更计数都读它）
    pub fn change_set(&self, document: &DocumentId, index: usize) -> Option<&ChangeSet> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.changes.get(index))
    }

    /// 【T2a-3】变更某份结果集的变更集（单元格提交 / Esc 撤销 / 提交回执标记都走这里）
    ///
    /// 返回是否真的改到（文档或结果集下标不存在 = `false`，调用方据此决定要不要报错）。
    pub fn mutate_change_set(
        &mut self,
        document: &DocumentId,
        index: usize,
        update: impl FnOnce(&mut ChangeSet),
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(changes) = slot.changes.get_mut(index) else {
            return false;
        };
        update(changes);
        true
    }

    /// 某份结果集的编辑历史（撤销 / 重做栈；没有则 `None`）
    pub fn history(&self, document: &DocumentId, index: usize) -> Option<&EditHistory> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.histories.get(index))
    }

    /// 【撤销】变更某份结果集的编辑历史（编辑提交入栈 / 撤销 / 重做 / 结果作废清空都走这里）
    ///
    /// 与 [`Self::mutate_change_set`] 同口径：文档或结果集下标不存在 = `false`。
    pub fn mutate_history(
        &mut self,
        document: &DocumentId,
        index: usize,
        update: impl FnOnce(&mut EditHistory),
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(history) = slot.histories.get_mut(index) else {
            return false;
        };
        update(history);
        true
    }

    /// 【#28】某份结果集的本地叠加层（本地行 + 删除标记；只读投影 / 计数用）
    pub fn local_rows(&self, document: &DocumentId, index: usize) -> Option<&LocalRowSet> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.locals.get(index))
    }

    /// 【#28】变更某份结果集的本地叠加层（新增 / 复制 / 删除标记都走这里）
    ///
    /// 与 [`Self::mutate_change_set`] 同口径：文档或结果集下标不存在 = `false`。
    pub fn mutate_local_rows(
        &mut self,
        document: &DocumentId,
        index: usize,
        update: impl FnOnce(&mut LocalRowSet),
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(local) = slot.locals.get_mut(index) else {
            return false;
        };
        update(local);
        true
    }

    /// 【#28】某份结果集的行级历史（新增 / 复制 / 删除撤销重做双栈；没有则 `None`）
    pub fn row_history(&self, document: &DocumentId, index: usize) -> Option<&RowHistory> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.row_histories.get(index))
    }

    /// 【#28】变更某份结果集的行级历史（行级动作入栈 / 撤销 / 重做 / 结果作废清空）
    ///
    /// 与 [`Self::mutate_history`] 同口径：文档或结果集下标不存在 = `false`。
    pub fn mutate_row_history(
        &mut self,
        document: &DocumentId,
        index: usize,
        update: impl FnOnce(&mut RowHistory),
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(history) = slot.row_histories.get_mut(index) else {
            return false;
        };
        update(history);
        true
    }

    /// 【T2a】把某一格的新值写进**指定**结果集的行数据（本地应用；返回是否真的改到）
    ///
    /// 提交回执后调用：`index` = 结果集标签条的下标（不是选中项），列按数据列下标。
    /// 行 / 列越界或结果集不存在返回 `false`——调用方（面板）据此决定要不要报错。
    pub fn apply_cell_edit(
        &mut self,
        document: &DocumentId,
        index: usize,
        row: usize,
        column: usize,
        value: impl Into<String>,
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(entry) = slot.sets.get_mut(index) else {
            return false;
        };
        entry.apply_cell(row, column, value)
    }

    /// 丢弃某文档的全部结果（文档关闭时调用——结果不跟着已关闭的文档留着）
    pub fn clear(&mut self, document: &DocumentId) {
        self.documents.retain(|slot| &slot.document != document);
    }

    /// 已记录结果的文档数（不是结果集总数）
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
}

/// 超过上限就淘汰**最旧的未选中**结果集（选中项保留，选中下标跟着修正）
fn evict_oldest(slot: &mut DocumentResults) {
    while slot.sets.len() > MAX_RESULT_SETS {
        let victim = (0..slot.sets.len())
            .find(|index| *index != slot.active)
            .expect("选中项只有一个，上限 > 1 时必有可淘汰项");
        slot.sets.remove(victim);
        // 【T2a-3】变更集跟着结果集一起淘汰（被淘汰那份的编辑没有可提交的对象了）
        slot.changes.remove(victim);
        // 【撤销】编辑历史跟着结果集一起淘汰（同理）
        slot.histories.remove(victim);
        // 【#28】本地叠加层与行级历史跟着结果集一起淘汰（同理）
        slot.locals.remove(victim);
        slot.row_histories.remove(victim);
        // 【A2】事实档跟着结果集一起淘汰（被淘汰那份的帧没有可归属的结果集了）
        slot.facts.remove(victim);
        if victim < slot.active {
            slot.active -= 1;
        }
        // 【并排对比】对比项跟着被淘汰 / 前移（对比对象不在了就清掉）
        match slot.compare {
            Some(compare) if compare == victim => slot.compare = None,
            Some(compare) if compare > victim => slot.compare = Some(compare - 1),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{ColumnKind, FactState, MAX_RESULT_SETS, ResultEntry, ResultPlacement, ResultStore};
    use ::shared::FactFrame;
    use std::sync::Arc;
    use workbench_shell::model::DocumentId;

    fn entry(document: &str, sql: &str, rows: usize) -> ResultEntry {
        ResultEntry::success(
            DocumentId::new(document),
            sql.to_string(),
            12,
            false,
            vec!["n".to_string()],
            (0..rows).map(|i| vec![i.to_string()]).collect(),
        )
    }

    fn active_sql(store: &ResultStore, document: &str) -> Option<String> {
        store
            .active(&DocumentId::new(document))
            .map(|entry| entry.sql.clone())
    }

    /// 【T2a-2】`row_key`：主键列里只留**结果真选中的列**；没探测 / 键列不在结果里 = `None`
    #[test]
    fn row_key_keeps_only_present_key_columns() {
        let mut entry = entry("doc-1", "select id, name from orders", 1);
        entry.columns = vec!["id".to_string(), "name".to_string()];

        // 未探测（构造默认）：没有行定位依据 → None（判据 NoRowKey 置只读）
        assert_eq!(entry.row_key(), None, "没探测 = None，不猜键");

        // 探测出主键：结果里有该列 → Some
        entry.key_columns = Some(vec!["id".to_string()]);
        assert_eq!(
            entry.row_key(),
            Some(crate::export::RowKey::single("id")),
            "主键列在结果里 → 行标识成立"
        );

        // 复合键：都选中才算（AND 顺序 = 探测顺序）
        entry.key_columns = Some(vec!["a".to_string(), "id".to_string()]);
        assert_eq!(
            entry.row_key().map(|key| key.columns().to_vec()),
            Some(vec!["id".to_string()]),
            "结果里没有的键列（a）被过滤掉；只剩 id 仍可定位行"
        );

        // 结果没 SELECT 主键（select name）→ None（写回没有可绑定的值）
        entry.columns = vec!["name".to_string()];
        assert_eq!(entry.row_key(), None, "键列不在结果里 = 无法生成 WHERE 绑定");

        // 探测到“空主键”不出现（约定 Some(vec![]) 不产生）——防御断言
        entry.columns = vec!["id".to_string()];
        entry.key_columns = Some(vec![]);
        assert_eq!(entry.row_key(), None);
    }

    /// 【M8】「洞察此列」的入场条件：四条全满足才给入口。
    ///
    /// 为什么写死到谓词级：这个入口会**重跑用户的 SQL**（带 LIMIT 取样），所以
    /// “能不能重放”的判据不能各写一份——门店、菜单、面板三处读的都是它。
    #[test]
    fn can_insight_column_requires_success_columns_connection_and_a_select() {
        let bound = |sql: &str| {
            let mut entry = entry("doc-1", sql, 3);
            entry.connection = Some("G_1".to_string());
            entry
        };

        assert!(
            bound("select id, amount from orders").can_insight_column(),
            "成功 + 有列 + 绑连接 + 只读查询：给入口"
        );
        assert!(
            bound("WITH t AS (SELECT 1 AS n) SELECT n FROM t").can_insight_column(),
            "CTE 也是只读查询"
        );

        assert!(
            !bound("insert into orders values (1)").can_insight_column(),
            "写语句不给入口：不能把用户的写语句重放一遍"
        );
        assert!(
            !bound("drop table orders").can_insight_column(),
            "DDL 更不给"
        );

        let mut unbound = entry("doc-1", "select 1", 3);
        unbound.connection = None;
        assert!(
            unbound.can_insight_column(),
            "未绑定连接也可以（执行时跟随当前活动连接）——连接由面板/执行器合起来判定"
        );
        assert!(
            !ResultEntry::is_read_only_query("update t set n = 1"),
            "写语句依旧不给"
        );

        let mut failed = bound("select 1");
        failed.error = Some("boom".to_string());
        assert!(!failed.can_insight_column(), "失败的结果没有可分析的列");

        let mut no_columns = bound("select 1");
        no_columns.columns.clear();
        assert!(!no_columns.can_insight_column(), "没有列就没有“此列”");
    }

    #[test]
    fn replacing_the_same_document_keeps_one_result() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-1", "select 2", 3), ResultPlacement::Replace, None);

        assert_eq!(store.set_count(&DocumentId::new("doc-1")), 1, "同一次执行不叠加");
        assert_eq!(active_sql(&store, "doc-1"), Some("select 2".to_string()));
        assert_eq!(
            store.active(&DocumentId::new("doc-1")).map(|e| e.row_count()),
            Some(3)
        );
    }

    #[test]
    fn documents_keep_their_own_results() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-2", "select 2", 2), ResultPlacement::Replace, None);
        assert_eq!(store.len(), 2);
        assert_eq!(active_sql(&store, "doc-2"), Some("select 2".to_string()));
    }

    #[test]
    fn closing_a_document_drops_its_result() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.clear(&DocumentId::new("doc-1"));
        assert!(store.is_empty());
        assert!(store.active(&DocumentId::new("doc-1")).is_none());
    }

    #[test]
    fn failures_are_recorded_with_their_reason_and_no_grid() {
        let mut store = ResultStore::new();
        store.push(
            ResultEntry::failure(
                DocumentId::new("doc-1"),
                "select boom".to_string(),
                "驱动报错：boom".to_string(),
                3,
            ),
            ResultPlacement::Replace,
            None,
        );
        let latest = store.active(&DocumentId::new("doc-1")).expect("有记录");
        assert!(!latest.has_grid(), "失败没有网格");
        assert!(latest.failed(), "失败要能被标签条标红");
        assert_eq!(latest.row_count(), 0);
        assert!(latest.summary().contains("boom"), "{}", latest.summary());
    }

    #[test]
    fn summary_reports_real_numbers_only() {
        let mut entry = entry("doc-1", "select 1", 4);
        entry.elapsed_ms = 9;
        assert_eq!(entry.summary(), "4 行 × 1 列 · 9 ms");

        entry.truncated = true;
        assert!(entry.summary().contains("已截断"), "{}", entry.summary());
    }

    /// 写语句：状态行报影响行数，不当成「0 行 0 列」的结果集
    #[test]
    fn writes_report_affected_rows_instead_of_a_grid() {
        let entry = ResultEntry::success(
            DocumentId::new("doc-1"),
            "insert into t values (1)".to_string(),
            7,
            false,
            Vec::new(),
            Vec::new(),
        )
        .with_affected_rows(Some(3));

        assert!(entry.affects_rows_only(), "只有影响行数");
        assert!(!entry.has_grid(), "写语句没结果集");
        assert_eq!(entry.summary(), "影响 3 行 · 7 ms");
    }

    /// 复制成 TSV：表头 + 行；制表符 / 换行 / 引号要转义（不转义会把列错开）
    #[test]
    fn tsv_quotes_cells_that_would_break_the_shape() {
        let mut entry = entry("doc-1", "select 1", 0);
        entry.columns = vec!["id".to_string(), "note".to_string()];
        entry.rows = vec![
            vec!["1".to_string(), "plain".to_string()],
            vec!["2".to_string(), "two\tcells".to_string()],
            vec!["3".to_string(), "line\nbreak".to_string()],
            vec!["4".to_string(), "say \"hi\"".to_string()],
        ];
        assert_eq!(
            entry.to_tsv(),
            "id\tnote\n1\tplain\n2\t\"two\tcells\"\n3\t\"line\nbreak\"\n4\t\"say \"\"hi\"\"\""
        );
    }

    /// 没有网格就没有可复制的文本（空串，不是一行空表头）
    #[test]
    fn tsv_is_empty_without_a_grid() {
        let failed = ResultEntry::failure(
            DocumentId::new("doc-1"),
            "select boom".to_string(),
            "驱动报错：boom".to_string(),
            3,
        );
        assert_eq!(failed.to_tsv(), "");

        let write = ResultEntry::success(
            DocumentId::new("doc-1"),
            "delete from t".to_string(),
            1,
            false,
            Vec::new(),
            Vec::new(),
        )
        .with_affected_rows(Some(0));
        assert_eq!(write.to_tsv(), "");
    }

    /// 影响行数与来源连接都是"有就给、没就不填"的真值
    #[test]
    fn affected_rows_and_connection_are_optional_truth() {
        let plain = entry("doc-1", "select 1", 1);
        assert_eq!(plain.affected_rows, None);
        assert_eq!(plain.connection, None);

        let tagged = entry("doc-1", "select 1", 1)
            .with_affected_rows(Some(0))
            .with_connection(Some("conn-1".to_string()));
        assert_eq!(tagged.affected_rows, Some(0), "零行也是真值，不等于没有");
        assert_eq!(tagged.connection.as_deref(), Some("conn-1"));
    }

    /// 【B5b】取下一段：接在后面、耗时累加、`has_more` 以最后一段为准、**不新开结果集**
    #[test]
    fn appending_a_segment_extends_the_same_result() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");

        let mut first = entry("doc-1", "select n from t", 0);
        first.columns = vec!["n".to_string()];
        first.rows = vec![vec!["1".to_string()], vec!["2".to_string()]];
        first.elapsed_ms = 5;
        first.has_more = true;
        store.push(first, ResultPlacement::Replace, None);
        assert!(
            store.active(&document).expect("有结果").can_fetch_more(),
            "第一段拿满了 → 还能取下一段"
        );

        let mut second = entry("doc-1", "select n from t", 0);
        second.columns = vec!["n".to_string()];
        second.rows = vec![vec!["3".to_string()], vec!["4".to_string()]];
        second.elapsed_ms = 3;
        second.has_more = false;
        store.push(second, ResultPlacement::Append, None);

        assert_eq!(store.set_count(&document), 1, "取下一段不新开结果集");
        let active = store.active(&document).expect("有结果");
        assert_eq!(active.row_count(), 4, "两段接起来");
        assert_eq!(active.elapsed_ms, 8, "耗时累加（抓这份结果花了多久）");
        assert!(
            !active.can_fetch_more(),
            "最后一段没拿满 → 没有下一段了"
        );
    }

    /// 列形状变了就不接（当一份新结果落下），不把两行的列错开
    #[test]
    fn appending_a_mismatched_shape_replaces_instead_of_mixing_columns() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");

        let mut first = entry("doc-1", "select n from t", 0);
        first.columns = vec!["n".to_string()];
        first.rows = vec![vec!["1".to_string()]];
        store.push(first, ResultPlacement::Replace, None);

        let mut other = entry("doc-1", "select n from t", 0);
        other.columns = vec!["n".to_string(), "m".to_string()];
        other.rows = vec![vec!["1".to_string(), "2".to_string()]];
        store.push(other, ResultPlacement::Append, None);

        let active = store.active(&document).expect("有结果");
        assert_eq!(active.row_count(), 1, "形状不一致时不接");
        assert_eq!(active.columns, ["n".to_string(), "m".to_string()]);
    }

    /// 新结果集落位：追加一份，用户在看的旧那份**仍然选中**（原型 §4.4）
    #[test]
    fn a_new_set_keeps_the_previous_one_selected() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "first", 1), ResultPlacement::NewSet, None);
        store.push(entry("doc-1", "second", 2), ResultPlacement::NewSet, None);

        assert_eq!(store.set_count(&document), 2);
        assert_eq!(store.active_index(&document), Some(0), "选中项不动");
        assert_eq!(active_sql(&store, "doc-1"), Some("first".to_string()));

        let sqls: Vec<String> = store.sets(&document).iter().map(|e| e.sql.clone()).collect();
        assert_eq!(sqls, ["first".to_string(), "second".to_string()], "按产生顺序");
    }

    /// 首次执行没有可保留的选中项：新结果集就是被选中的那份
    #[test]
    fn the_first_set_is_selected_even_when_it_lands_as_a_new_set() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "first", 1), ResultPlacement::NewSet, None);

        assert_eq!(store.active_index(&document), Some(0));
        assert_eq!(active_sql(&store, "doc-1"), Some("first".to_string()));
    }

    /// 批量：每句一份，用户可以点开任意一份（选中下标跟着走）
    #[test]
    fn selecting_a_set_moves_the_active_one() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        for sql in ["s1", "s2", "s3"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet, None);
        }

        assert!(store.select(&document, 2), "点标签要能切过去");
        assert_eq!(store.active_index(&document), Some(2));
        assert_eq!(active_sql(&store, "doc-1"), Some("s3".to_string()));

        assert!(!store.select(&document, 9), "越界不改状态");
        assert!(!store.select(&document, 2), "点当前项不算变化（不必重绘）");
        assert!(!store.select(&DocumentId::new("doc-x"), 0), "没有这份文档");
        assert_eq!(store.active_index(&document), Some(2));
    }

    /// 上限 5：超出淘汰**最旧的未选中**项，正在看的那份不会被自己的下一个结果挤掉
    #[test]
    fn the_oldest_unselected_set_is_evicted_at_the_cap() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        for sql in ["s1", "s2", "s3"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet, None);
        }
        // 用户切到第 2 份（下标 1），随后反复执行（Replace 落到选中项，不增加条数）
        assert!(store.select(&document, 1));
        store.push(entry("doc-1", "replace-active", 1), ResultPlacement::Replace, None);
        assert_eq!(store.set_count(&document), 3);

        for sql in ["s4", "s5", "s6"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet, None);
        }

        assert_eq!(store.set_count(&document), MAX_RESULT_SETS, "上限生效");
        let sqls: Vec<String> = store.sets(&document).iter().map(|e| e.sql.clone()).collect();
        assert!(
            sqls.contains(&"replace-active".to_string()),
            "选中项必须留下：{sqls:?}"
        );
        assert!(!sqls.contains(&"s1".to_string()), "最旧的先走：{sqls:?}");
        assert_eq!(
            active_sql(&store, "doc-1"),
            Some("replace-active".to_string()),
            "淘汰后选中项还是原来那份"
        );
    }

    /// 列类型：驱动报的类型名 → 语义类别（表驱动）
    ///
    /// 名字是**方言**的：四类库加 Arrow 的写法各不相同（`bigint` / `Int64` /
    /// `numeric(38,10)` / `timestamp with time zone`），而认不出来时必须回到 `Unknown`
    /// ——宁可少着色，不能猜错（猜错就是错位与错字体）。
    #[test]
    fn column_kinds_follow_the_driver_type_names() {
        let cases: &[(&str, ColumnKind)] = &[
            ("bigint", ColumnKind::Integer),
            ("int4", ColumnKind::Integer),
            ("Int64", ColumnKind::Integer),
            ("int unsigned", ColumnKind::Integer),
            ("serial", ColumnKind::Integer),
            ("UInt32", ColumnKind::Integer),
            ("numeric(38,10)", ColumnKind::Decimal),
            ("DECIMAL(18,2)", ColumnKind::Decimal),
            ("NUMBER(38,0)", ColumnKind::Decimal),
            ("money", ColumnKind::Decimal),
            ("float8", ColumnKind::Float),
            ("double precision", ColumnKind::Float),
            ("Float64", ColumnKind::Float),
            ("real", ColumnKind::Float),
            ("bool", ColumnKind::Boolean),
            ("BOOLEAN", ColumnKind::Boolean),
            ("varchar(255)", ColumnKind::Text),
            ("character varying", ColumnKind::Text),
            ("Utf8", ColumnKind::Text),
            ("LargeUtf8", ColumnKind::Text),
            ("timestamp with time zone", ColumnKind::Timestamp),
            ("timestamp without time zone", ColumnKind::Timestamp),
            ("TIMESTAMP(6)", ColumnKind::Timestamp),
            ("timestamptz", ColumnKind::Timestamp),
            ("datetime", ColumnKind::Timestamp),
            ("date", ColumnKind::Date),
            ("Date32", ColumnKind::Date),
            ("time", ColumnKind::Time),
            ("Time64(Nanosecond)", ColumnKind::Time),
            ("uuid", ColumnKind::Uuid),
            ("jsonb", ColumnKind::Json),
            ("bytea", ColumnKind::Binary),
            ("LargeBinary", ColumnKind::Binary),
            ("VARBINARY(16)", ColumnKind::Binary),
            // 认不出就是 Unknown：`point` / `interval` 里没有独立的 int 词，不能“看着像”就当成整数
            ("point", ColumnKind::Unknown),
            ("interval", ColumnKind::Unknown),
            ("oid", ColumnKind::Unknown),
            ("", ColumnKind::Unknown),
            ("   ", ColumnKind::Unknown),
            // 容器型的内层类型不是这一列的语义（词典编码的文本列不该被右对齐）
            ("Dictionary(Int32, Utf8)", ColumnKind::Unknown),
            ("List(Int64)", ColumnKind::Unknown),
        ];
        for (name, expected) in cases {
            assert_eq!(ColumnKind::parse(name), *expected, "类型名 {name:?}");
        }
    }

    /// 渲染决策只有两条，且**未知档两条都是“不做”**——那就是“类型缺失时与今天完全一致”
    #[test]
    fn only_numbers_right_align_and_only_the_time_family_is_monospace() {
        let cases: &[(ColumnKind, bool, bool)] = &[
            (ColumnKind::Integer, true, false),
            (ColumnKind::Float, true, false),
            (ColumnKind::Decimal, true, false),
            (ColumnKind::Boolean, false, true),
            (ColumnKind::Timestamp, false, true),
            (ColumnKind::Date, false, true),
            (ColumnKind::Time, false, true),
            (ColumnKind::Uuid, false, true),
            (ColumnKind::Text, false, false),
            (ColumnKind::Json, false, false),
            (ColumnKind::Binary, false, false),
            (ColumnKind::Unknown, false, false),
        ];
        for (kind, numeric, monospace) in cases {
            assert_eq!(kind.is_numeric(), *numeric, "{kind:?} 该不该右对齐");
            assert_eq!(kind.is_monospace(), *monospace, "{kind:?} 该不该等宽");
        }
    }

    /// 类型与列**同序对齐**：名字少了填 `None`、多了丢掉；空切片 = 驱动没报（保持空）
    #[test]
    fn column_types_align_with_the_columns() {
        let mut three = ResultEntry::success(
            DocumentId::new("doc-1"),
            "select 1".to_string(),
            5,
            false,
            vec!["id".to_string(), "amount".to_string(), "note".to_string()],
            vec![vec!["1".to_string(); 3]],
        );
        three = three.with_column_types(&[" bigint ".to_string(), "numeric(38,10)".to_string()]);

        assert_eq!(three.column_types.len(), 3, "与列等长（缺的填 None）");
        assert_eq!(three.column_kind(0), ColumnKind::Integer);
        assert_eq!(three.column_kind(1), ColumnKind::Decimal);
        assert_eq!(
            three.column_kind(2),
            ColumnKind::Unknown,
            "少报的那列 = 没类型"
        );
        assert!(three.column_type(2).is_none());
        assert!(three.column_type(9).is_none(), "越界不 panic");
        assert_eq!(
            three.column_type(0).map(|kind| kind.name.as_str()),
            Some("bigint"),
            "表头显示的是驱动报的**原始名字**（首尾空白修掉）"
        );

        // 名字比列多：多的丢掉（形状以 columns 为准）
        let mut one = entry("doc-1", "select n from t", 1);
        one = one.with_column_types(&["bigint".to_string(), "text".to_string()]);
        assert_eq!(one.column_types.len(), 1);
        assert_eq!(one.column_kind(0), ColumnKind::Integer);

        // 空名字当成没报（不产生一个“名字是空串的类型”）
        let mut blank = entry("doc-1", "select n from t", 1);
        blank = blank.with_column_types(&["  ".to_string()]);
        assert_eq!(blank.column_types, vec![None]);
        assert_eq!(blank.column_kind(0), ColumnKind::Unknown);
    }

    /// 没带类型的那一档就是今天：`column_types` 为空、类别一律 `Unknown`、名字取不到
    #[test]
    fn a_result_without_types_stays_typeless() {
        let plain = entry("doc-1", "select n from t", 2);
        assert!(
            plain.column_types.is_empty(),
            "驱动没报就是空 vec，不是一列一个空类型"
        );
        assert!(plain.column_type(0).is_none());
        assert_eq!(plain.column_kind(0), ColumnKind::Unknown);

        // 显式传空切片也不能“造”出类型来（真实路径今天就是这样）
        let still_plain = entry("doc-1", "select n from t", 2).with_column_types(&[]);
        assert!(still_plain.column_types.is_empty());
        assert_eq!(still_plain.column_kind(0), ColumnKind::Unknown);

        // 失败的结果没有列，也没有类型（分析 / 写语句同理：columns 为空时不产生类型）
        let failed = ResultEntry::failure(
            DocumentId::new("doc-1"),
            "select boom".to_string(),
            "驱动报错：boom".to_string(),
            3,
        );
        assert!(failed.column_types.is_empty());
        assert_eq!(failed.column_kind(0), ColumnKind::Unknown);

        let mut write = entry("doc-1", "insert into t values (1)", 0);
        write.columns.clear();
        write.rows.clear();
        let write = write.with_column_types(&["bigint".to_string()]);
        assert!(write.column_types.is_empty(), "没有列就没有列类型");
    }

    /// 取下一段：目标本来没带类型、这一段带了就补上；已有类型不被后一段覆盖
    ///
    /// （类型是**列**的属性，分段抓取的列形状相同——后一段的类型不比前一段更权威。）
    #[test]
    fn appending_a_segment_adopts_types_only_when_the_target_has_none() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select n from t", 2),
            ResultPlacement::Replace,
            None,
        );
        assert!(
            store
                .active(&document)
                .expect("有结果")
                .column_types
                .is_empty()
        );

        store.push(
            entry("doc-1", "select n from t", 2).with_column_types(&["bigint".to_string()]),
            ResultPlacement::Append,
            None,
        );
        let active = store.active(&document).expect("有结果");
        assert_eq!(active.row_count(), 4, "这一段真的接在后面了");
        assert_eq!(
            active.column_kind(0),
            ColumnKind::Integer,
            "第一段没类型、这一段带了 → 补上"
        );

        store.push(
            entry("doc-1", "select n from t", 2).with_column_types(&["text".to_string()]),
            ResultPlacement::Append,
            None,
        );
        assert_eq!(
            store.active(&document).expect("有结果").column_kind(0),
            ColumnKind::Integer,
            "已有类型不被后一段覆盖"
        );
    }

    // ===== 【并排对比】对比选中 =====

    /// 对比项与选中项恒不同：设成 active 本身被拒、取消后能再设
    #[test]
    fn compare_cannot_be_the_active_set_itself() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);

        assert!(
            !store.select_compare(&document, 0),
            "active（0）不能当自己的对比项"
        );
        assert_eq!(store.compare_index(&document), None);

        assert!(store.select_compare(&document, 1), "另一份可以做对比项");
        assert_eq!(store.compare_index(&document), Some(1));
        assert!(store.compare_entry(&document).is_some());
    }

    /// 切换选中到对比项本身 → 对比自动清空（自己对比自己没有意义）
    #[test]
    fn selecting_the_compare_set_clears_compare() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        assert!(store.select_compare(&document, 1));

        assert!(store.select(&document, 1), "切到对比项本身");
        assert_eq!(store.compare_index(&document), None, "自己对比自己失效");

        assert!(store.select(&document, 0), "切回主选中");
        assert!(store.select_compare(&document, 1), "还能再设回来");
    }

    /// 再点同一份 = 取消对比（toggle 语义）；点别的份 = 换对比项
    #[test]
    fn toggle_compare_switches_or_clears() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        store.push(entry("doc-1", "select 3", 3), ResultPlacement::NewSet, None);

        assert!(store.toggle_compare(&document, 2), "第一次点 = 设上");
        assert_eq!(store.compare_index(&document), Some(2));
        assert!(store.toggle_compare(&document, 2), "再点同一份 = 取消");
        assert_eq!(store.compare_index(&document), None);
        assert!(store.toggle_compare(&document, 1), "点别的份 = 换成它");
        assert_eq!(store.compare_index(&document), Some(1));
        assert!(!store.toggle_compare(&document, 99), "越界不改");
    }

    /// 淘汰：对比项被挤掉 → 清空；对比项位置前移 → 跟着减
    #[test]
    fn evicting_the_compare_set_clears_or_rewinds_it() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        // 灌满到上限（MAX_RESULT_SETS），active 保持第 0 份（Replace 不换份）
        for i in 0..MAX_RESULT_SETS {
            store.push(
                entry("doc-1", &format!("select {i}"), i + 1),
                if i == 0 {
                    ResultPlacement::Replace
                } else {
                    ResultPlacement::NewSet
                },
                None,
            );
        }
        assert!(store.select_compare(&document, 1), "对比第 1 份");
        // 再落一份：淘汰最旧的未选中（第 1 份），对比项被挤掉 → 清空
        store.push(
            entry("doc-1", "select new", 1),
            ResultPlacement::NewSet,
            None,
        );
        assert_eq!(store.compare_index(&document), None, "对比对象被淘汰 = 对比失效");

        // 对比第 2 份；再落一份淘汰第 2 份之前的（active=0 保留）→ 第 2 份前移为 1
        assert!(store.select_compare(&document, 2));
        store.push(
            entry("doc-1", "select newer", 1),
            ResultPlacement::NewSet,
            None,
        );
        assert_eq!(
            store.compare_index(&document),
            Some(1),
            "淘汰发生在对比项之前 → 对比项下标前移 1"
        );
    }

    /// 【撤销】编辑历史与变更集同序：新结果带空历史，替换作废旧历史
    #[test]
    fn histories_track_sets_lifecycle() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        // 首次执行：历史存在且为空
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(store.history(&document, 0).is_some_and(|h| !h.can_undo()));

        // 追加新结果：历史与 sets 同长
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        assert_eq!(store.sets(&document).len(), 2);
        assert!(store.history(&document, 1).is_some_and(|h| !h.can_undo()));

        // 替换选中那份：历史作废（旧编辑没有可撤销的对象了）
        store.mutate_history(&document, 0, |h| {
            h.push(crate::changes::CellChange::pending(0, 0, "a", "b"));
        });
        assert!(store.history(&document, 0).is_some_and(|h| h.can_undo()));
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(
            store.history(&document, 0).is_some_and(|h| !h.can_undo()),
            "替换后历史清空"
        );
    }

    /// 【撤销】mutate_history 与 mutate_change_set 同口径：文档 / 下标不存在返回 false
    #[test]
    fn mutate_history_refuses_unknown_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(!store.mutate_history(&document, 7, |_| {}), "下标越界 = false");
        assert!(
            !store.mutate_history(&DocumentId::new("nope"), 0, |_| {}),
            "文档不存在 = false"
        );
        assert!(store.mutate_history(&document, 0, |_| {}), "命中 = true");
    }

    /// 【#28】本地叠加层与变更集同序：新结果带空集合，替换作废旧叠加层
    #[test]
    fn locals_track_sets_lifecycle() {
        use crate::rowset::LocalRow;

        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        // 首次执行：本地叠加层存在且为空
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(store
            .local_rows(&document, 0)
            .is_some_and(|set| set.is_empty()));

        // 追加新结果：叠加层与 sets 同长
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        assert_eq!(store.sets(&document).len(), 2);
        assert!(store.local_rows(&document, 1).is_some_and(|set| set.is_empty()));

        // 往第 0 份写本地行 + 删除标记
        store.mutate_local_rows(&document, 0, |set| {
            let id = set.next_id();
            set.push_local(LocalRow::new(id, vec!["".to_string()]));
            set.mark_deleted(0);
        });
        assert!(store
            .local_rows(&document, 0)
            .is_some_and(|set| !set.is_empty()));

        // 替换选中那份：本地叠加层作废（新数据是新结果，旧本地行没有可归属的对象）
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(
            store
                .local_rows(&document, 0)
                .is_some_and(|set| set.is_empty()),
            "替换后本地叠加层清空"
        );
    }

    /// 【#28】行级历史与格级历史同生命周期：替换作废旧行级历史
    #[test]
    fn row_histories_track_sets_lifecycle() {
        use crate::rowset::RowOp;

        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(store
            .row_history(&document, 0)
            .is_some_and(|h| !h.can_undo()));

        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        assert_eq!(store.sets(&document).len(), 2);
        assert!(store.row_history(&document, 1).is_some_and(|h| !h.can_undo()));

        store.mutate_row_history(&document, 0, |h| {
            h.push(RowOp::InsertLocal {
                local_id: 1,
                anchor: None,
            });
        });
        assert!(store.row_history(&document, 0).is_some_and(|h| h.can_undo()));

        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(
            store
                .row_history(&document, 0)
                .is_some_and(|h| !h.can_undo()),
            "替换后行级历史清空"
        );
    }

    /// 【#28】mutate_local_rows 与 mutate_change_set 同口径：文档 / 下标不存在返回 false
    #[test]
    fn mutate_local_rows_refuses_unknown_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(!store.mutate_local_rows(&document, 7, |_| {}), "下标越界 = false");
        assert!(
            !store.mutate_local_rows(&DocumentId::new("nope"), 0, |_| {}),
            "文档不存在 = false"
        );
        assert!(store.mutate_local_rows(&document, 0, |_| {}), "命中 = true");
    }

    /// 【#28】mutate_row_history 同上口径
    #[test]
    fn mutate_row_history_refuses_unknown_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(!store.mutate_row_history(&document, 7, |_| {}), "下标越界 = false");
        assert!(
            !store.mutate_row_history(&DocumentId::new("nope"), 0, |_| {}),
            "文档不存在 = false"
        );
        assert!(store.mutate_row_history(&document, 0, |_| {}), "命中 = true");
    }

    /// 【A2】事实档与结果集同生命周期：新结果入槽为 None 档，替换 / 追加保持同序同长
    #[test]
    fn facts_track_sets_lifecycle() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");

        // 首次执行：事实档存在且为 None（A2 尚未接线内存档构造路径）
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(store.fact_at(&document, 0).is_some_and(|fact| fact.is_none()));
        assert!(store.active_fact(&document).is_some_and(|fact| fact.is_none()));

        // 追加新结果：事实档与 sets 同长同序
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        assert_eq!(store.set_count(&document), 2);
        assert!(store.fact_at(&document, 1).is_some_and(|fact| fact.is_none()));

        // 替换选中那份：事实档回 None 档（旧帧随旧结果作废；槽位仍有效、不越界）
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(store.fact_at(&document, 0).is_some_and(|fact| fact.is_none()));
        assert!(store.active_fact(&document).is_some_and(|fact| fact.is_none()));
    }

    /// 【A2】fact_at / active_fact 越界与未知文档口径同 sets 系列访问器
    #[test]
    fn fact_access_refuses_unknown_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);

        assert!(store.fact_at(&document, 3).is_none(), "下标越界 = None");
        assert!(
            store.fact_at(&DocumentId::new("nope"), 0).is_none(),
            "文档不存在 = None"
        );
        assert!(store.active_fact(&DocumentId::new("nope")).is_none());
    }

    /// 【A2/B2】None 档不带事实帧、占用报 0；表档持释放后的帧，Rust 堆占用同样报 0。
    #[test]
    fn none_and_table_states_report_no_rust_heap_bytes() {
        let none = FactState::none();
        assert!(none.is_none());
        assert!(none.in_memory().is_none());
        assert!(none.on_duckdb().is_none());
        assert_eq!(none.estimated_bytes(), 0);

        let mut frame = fact_frame(&[1, 2]);
        frame.set_temp_table("tmp_q_result_9".to_string());
        frame.release_batches();
        let table = FactState::OnDuckDb(Arc::new(frame));
        assert!(!table.is_none());
        assert!(table.in_memory().is_none());
        assert!(table.on_duckdb().is_some());
        assert_eq!(table.estimated_bytes(), 0, "表档 batches 已释放，Rust 堆占用报 0");
        assert_eq!(table.temp_table(), Some("tmp_q_result_9"));
        assert_eq!(
            table.on_duckdb().expect("表档帧").row_count(),
            2,
            "行数缓存随表档保留"
        );
    }

    /// 【B2】`set_fact_table` 把常驻表名挂到内存档帧；重复调用复用同一句柄。
    #[test]
    fn set_fact_table_attaches_table_name_to_in_memory_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );

        assert!(store.set_fact_table(&document, 0, "tmp_q_result_3".to_string()));
        let fact = store.active_fact(&document).expect("有事实档");
        assert_eq!(fact.temp_table(), Some("tmp_q_result_3"));
        assert!(fact.in_memory().is_some(), "挂表名不释放 batches，仍为内存档");

        // 无事实档 / 越界 = false
        assert!(!store.set_fact_table(&document, 7, "tmp_q_result_x".to_string()));
        assert!(!store.set_fact_table(&DocumentId::new("nope"), 0, "tmp_q_result_x".to_string()));
    }

    /// 【B2】`promote_fact_to_table`：内存档挂表名后释放 batches，状态切表档；
    /// 已是表档复用成功，无事实档拒绝提升。
    #[test]
    fn promote_fact_to_table_releases_batches_and_switches_tier() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );

        assert!(store.promote_fact_to_table(&document, 0, "tmp_q_result_5".to_string()));
        let fact = store.active_fact(&document).expect("有事实档");
        assert!(fact.on_duckdb().is_some(), "状态切表档");
        assert!(fact.in_memory().is_none());
        assert_eq!(fact.temp_table(), Some("tmp_q_result_5"));
        assert_eq!(fact.estimated_bytes(), 0, "表档 Rust 堆占用归零");
        assert_eq!(
            fact.on_duckdb().expect("表档帧").row_count(),
            2,
            "释放后行数保留"
        );

        // 已是表档：再提升视为成功复用，不报错
        assert!(store.promote_fact_to_table(&document, 0, "tmp_q_result_5".to_string()));
    }

    /// 【B2】无事实档槽位拒绝提升；新结果入槽后旧表档随旧结果作废。
    #[test]
    fn promote_refuses_none_fact_slot_and_resets_on_replace() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(
            !store.promote_fact_to_table(&document, 0, "tmp_q_result_x".to_string()),
            "无事实档不能提升"
        );

        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );
        store.promote_fact_to_table(&document, 0, "tmp_q_result_8".to_string());
        // 无候选回执替换：表档帧随旧结果作废回 None
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(store.active_fact(&document).is_some_and(|fact| fact.is_none()));
    }

    /// 【A2】淘汰最旧结果集时事实档平行缩短：不存在“set 没了、fact 残留”的错位
    #[test]
    fn facts_evict_with_sets_at_cap() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        for sql in ["s1", "s2", "s3", "s4", "s5", "s6"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet, None);
        }
        assert_eq!(store.set_count(&document), MAX_RESULT_SETS);
        assert!(store.fact_at(&document, MAX_RESULT_SETS - 1).is_some());
        assert!(store.fact_at(&document, MAX_RESULT_SETS).is_none());
    }

    /// 【A3】Replace 挂入内存事实档；再以无候选回执替换 → 旧帧随旧结果作废回 None
    #[test]
    fn fact_slot_replaces_in_memory_frame_on_new_result() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );
        let active = store.active_fact(&document).expect("有事实档");
        assert!(active.in_memory().is_some(), "回执带 batches → 内存档");
        assert_eq!(active.in_memory().expect("内存档").row_count(), 2);

        // 再替换为无候选回执（DML / 加速档 / 联邦档）：旧帧随旧结果作废
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(store.active_fact(&document).is_some_and(|fact| fact.is_none()));
    }

    /// 【A3】Append 同形状两段：事实帧 batches 合并、行数累加；新段无候选回执保留旧帧
    #[test]
    fn fact_slot_merges_batches_when_appending_same_shape() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Append,
            Some(Arc::new(fact_frame(&[3, 4]))),
        );
        let active = store.active_fact(&document).expect("有事实档");
        assert_eq!(
            active.in_memory().expect("内存档").row_count(),
            4,
            "两段 batches 合并累加"
        );

        // 新段无 batches 回执：旧帧原样保留，不清空
        store.push(entry("doc-1", "select 1", 2), ResultPlacement::Append, None);
        assert_eq!(
            store
                .active_fact(&document)
                .expect("有事实档")
                .in_memory()
                .expect("内存档保留")
                .row_count(),
            4
        );
    }

    /// 【A3】Append 形状对不上落替换分支：旧事实帧作废，新候选无 batches → None 档
    #[test]
    fn fact_slot_discards_frame_when_append_shape_mismatches() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );
        let mut other = entry("doc-1", "select x", 1);
        other.columns = vec!["x".to_string()];
        store.push(other, ResultPlacement::Append, None);
        assert!(
            store.active_fact(&document).is_some_and(|fact| fact.is_none()),
            "形状对不上 = 新结果，旧帧不再归属本段"
        );
    }

    /// 【A3】测试 fixture：单列 `n` 整数事实帧（与 `entry` 的列名对齐，两段可合并）
    fn fact_frame(values: &[i64]) -> FactFrame {
        use arrow::array::{ArrayRef, Int64Array, RecordBatch};
        use arrow::datatypes::{Field, Schema};

        let array: ArrayRef = Arc::new(Int64Array::from(values.to_vec()));
        let schema = Arc::new(Schema::new(vec![Field::new(
            "n",
            array.data_type().clone(),
            true,
        )]));
        let batch = RecordBatch::try_new(schema, vec![array]).expect("构造 RecordBatch");
        FactFrame::new(vec!["n".to_string()], vec![batch]).expect("构造事实档")
    }
}
