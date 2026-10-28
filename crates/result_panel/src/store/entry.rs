//! 结果条目 + 列类型 + 落位语义——自 store.rs 拆分（2026-10-29）
//!
//! 模块文档（结果集权威 / 每文档结果集列表 / 列类型口径）见父模块 `crate::store`。

use ::shared::string::tsv_row;
use crate::export::RowKey;
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
    pub(crate) fn adopt_column_types(&mut self, types: Vec<Option<ColumnType>>) {
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

#[cfg(test)]
mod tests {
    // 安全模式：显式列举（避免与 gpui-kit 测试宏冲突）
    use super::{ColumnKind, ColumnType, ResultEntry};
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
}
