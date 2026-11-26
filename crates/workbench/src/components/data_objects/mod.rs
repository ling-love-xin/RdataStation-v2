//! 数据对象清单：所有可参与分析的对象，归一成一份可排序、可分组的行模型。
//!
//! 面板（右侧 Dock）与对话框（`source_picker`）读**同一份取数**，只是呈现不同：
//! 面板常驻、行上带「通道 + 状态」，对话框是一次性的模态树。这里只做**归一**，
//! 不做 I/O 之外的判断，也不碰渲染——渲染分支写在各视图里。
//!
//! ## 为什么单独成模块
//!
//! 现状是「每种来源各自在对话框里拼行」，面板要复用时只能抄一遍取数与身份规则，
//! 两份一旦漂移就会出现「对话框里有、面板里没有」这种最难解释的差异。所以先抽
//! 出与视图无关的一层：**对象 = 身份 + 名字 + 通道 + 寿命 + 状态**，谁渲染都读它。
//!
//! ## 与资产模块的边界
//!
//! 资产是**受管成果**（有版本、只读、名字不带前缀），它也可以参与分析，但只取
//! **最新版本**；要用旧版本数据得先在资产管理里回退。所以资产行**不给版本选择**
//! 控件，只在行上标 `v{n}`，让用户知道自己读的是哪一版。

use insight::SampleSource;
use std::collections::HashMap;

use crate::panels::Shared;

use super::source_picker::{analysis_tables_of, persistent_table_label, split_persistent_tables};

/// 行宽预算（面板窄到多少该丢哪列）：纯函数 + 实测常量，见模块注释。
pub mod layout;

/// 这份数据「怎么读」——决定执行路径，也决定物化与错误类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// 连接里的表，SQL 下推
    Source,
    /// 进程单例里的临时表（`tmp_q_` / `tmp_i_`）
    Process,
    /// 本机分析库（`analytics.duckdb`，含全局那份）
    Local,
    /// reader 函数读文件（草稿箱 / 外部文件）
    File,
    /// 受管资产（只读，只取最新版本）
    Asset,
    /// 联邦会话里挂上的源
    Federated,
    /// 本地加速会话挂上的源（一源一条专用 DuckDB 连接）
    Accel,
}

impl Channel {
    /// 行上显示的一小串（面板窄，不能写整句）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Source => "→ 源库",
            Self::Process => "→ 进程 DuckDB",
            Self::Local => "→ 本地分析库",
            Self::File => "→ 文件",
            Self::Asset => "→ 资产",
            Self::Federated => "→ 联邦",
            Self::Accel => "→ 本地加速",
        }
    }

    /// 悬浮 / 详情里的一句话解释。
    pub fn hint(self) -> &'static str {
        match self {
            Self::Source => "按连接的驱动下推查询，数据留在源库",
            Self::Process => "进程内的临时表，会话重建即消失",
            Self::Local => "本机 DuckDB 分析库里的表，长期存在",
            Self::File => "用 reader 函数直接读文件，不复制",
            Self::Asset => "受管资产，只读且只取最新版本",
            Self::Federated => "挂进联邦会话，跨源查询共享一次会话",
            Self::Accel => "挂进本地加速专用连接，未限定名在该连接的加速档里解析",
        }
    }
}

/// 这份数据能活多久——决定「失效」怎么表达。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Life {
    /// 不受我们控制（源库表 / 外部文件）
    External,
    /// 随会话或进程消失
    Session,
    /// 项目持久（分析库里的表）
    Project,
    /// 资产模块管（版本 / 回收站）
    Managed,
}

impl Life {
    pub fn label(self) -> &'static str {
        match self {
            Self::External => "外部",
            Self::Session => "会话级",
            Self::Project => "项目持久",
            Self::Managed => "受管",
        }
    }
}

/// 可用性。失效不是删掉，而是**留着并说清为什么**——用户才知道能不能救。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Ready,
    /// 表还在清单里，但底层对象已不在（会话重建 / 表被删）
    Stale(&'static str),
    /// 从没挂上去过（联邦源挂载失败、扩展缺失）
    Failed(String),
}

impl State {
    pub fn label(&self) -> String {
        match self {
            Self::Ready => "可用".to_string(),
            Self::Stale(why) => format!("已失效 · {why}"),
            Self::Failed(why) => why.clone(),
        }
    }

    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// 源段一行的**挂载三态**（规格 §3）：登记（项目库意图）× 挂载（运行态事实）。
///
/// 为什么不是一个布尔：两半会各自独立地变——切项目登记没了、重开会话挂载没了。
/// 只留「已挂载 / 未挂载」一列就分不出「从没挂过」和「挂过、会话被清掉了」，
/// 而这两种在动作上完全不同（前者要挂，后者要重开）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountState {
    /// 登记了，且这一刻在实例里
    MountedProject,
    /// 没登记，但这一刻在实例里（会话级加源：切项目即清）
    MountedSession,
    /// 登记了，但这一刻不在实例里（意图还在，会话得重开）
    Unmounted,
}

impl MountState {
    /// 两半对账（**纯函数**）。判据只有一个：别名在不在 `duckdb_databases()`；
    /// 登记侧来自项目库 `analysis_sources`。
    pub fn from_facts(registered: bool, mounted: bool) -> Self {
        match (registered, mounted) {
            (true, true) => Self::MountedProject,
            (true, false) => Self::Unmounted,
            (false, true) => Self::MountedSession,
            // 既没登记也没挂上：面板里本不该有这种行（它既没意图也没事实）。
            // 真出现了（登记被删、会话也没了）就按「未挂载 · 项目级」说——允许重新挂；
            // 冒充「已挂载 · 会话级」等于把一个不在实例里的东西说成在。
            (false, false) => Self::Unmounted,
        }
    }

    /// 行内小标原文（用户口径，别缩写：三种状态的差别就是这几个字）。
    pub fn label(self) -> &'static str {
        match self {
            Self::MountedProject => "已挂载 · 项目级",
            Self::MountedSession => "已挂载 · 会话级",
            Self::Unmounted => "未挂载 · 项目级",
        }
    }
}

/// 分组按**对象形态**排（2026-11-24 由六段并为**三段**：源 / 表 / 文件）：
/// 段回答「这是什么形态的东西、怎么读」，业务身份（资产 / 未登记 / 结果集 / 全局 /
/// 自建 / 落库 / 联邦 / 加速 / 文件库）全是行内小标。
///
/// 合并路径（取代 2026-11-18 的六段口径）：
/// 已挂载的源 + 源库表 → 源；未登记 + 表（资产）+ 临时表 → 表；文件 → 文件。
/// 段内次序不靠名字排序碰运气，由 [`Object::rank`] 定（正在用的在上）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// 源：连接（树根，可展开列表）、挂进 DuckDB 的库（联邦 / 加速 / 文件库）。
    /// **未挂载的源不给展开**——没有表可列，行上只给「挂载」（铁律 B 组第 4 条）。
    Source,
    /// 表：库里现有的表，三种身份同段靠小标区分：资产当前版本（`资产 vN`）、
    /// 未登记（`未登记` / `未登记 · 全局`）、临时结果集（`结果集 · 会话级`）。
    Table,
    /// 文件：结构化文件（`f_*` 视图与资产·文件型）。未加载的草稿不在这里。
    File,
}

impl Group {
    /// 段的固定顺序（**唯一权威**）。
    ///
    /// 此前顺序有三份：枚举声明（`Ord` 派生）、`order()` 的 match、视图里手写的
    /// chips 数组——加一段时只要漏改一处，排序与筛选就会各说各话。现在只留这一份：
    /// 排序直接用派生 `Ord`（声明顺序 = 段顺序），chips 遍历本常量，
    /// `groups_list_all_in_ord_order` 用例保证「常量顺序 = `Ord` 顺序」不被写歪。
    pub const ALL: [Self; 3] = [Self::Source, Self::Table, Self::File];

    pub fn title(self) -> &'static str {
        match self {
            Self::Source => "源",
            Self::Table => "表",
            Self::File => "文件",
        }
    }
}

/// 面板里的一行（也用于搜索与工作集，故带稳定身份）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// 稳定身份（业务键）：同名不同来源必须能区分
    pub key: String,
    pub name: String,
    /// 副标题（驱动 / 表数 / 路径 / 版本说明）
    pub note: String,
    pub channel: Channel,
    pub life: Life,
    pub state: State,
    pub group: Group,
    /// 业务身份（「资产」「项目自建」「全局」「结果集」「草稿」）：只作行内标签，
    /// 不参与分组——分组按 DuckDB 容器（见 [`Group`]）。
    pub pill: &'static str,
    /// 资产的最新版本（其它类型为 None）
    pub version: Option<u32>,
    /// 这一行在 SQL 里怎么引用（**带双引号的实际标识符**，铁律 G 第 30 条）。
    ///
    /// 只给"已经在分析实例里可见"的对象（`tmp_q_` / `analytics.main` 表 / `f_*`）；
    /// 源库表、联邦源要带连接上下文才拼得出，这里给 `None`——宁可空着，也不给一个
    /// 抄下去就跑不通的引用名。
    pub sql_ref: Option<String>,
    /// 源行行尾**唯一**的动作位（铁律第 42 条：叶子零按钮；源行只留这一个位）。
    pub source_action: Option<SourceAction>,
    /// 这一行点开是「去选表」而不是「列自家挂载的表」（登记注册入口）。
    ///
    /// 无 ATTACH 能力的已登记连接：登记已在项目源表里，但联邦里没有它的 catalog——
    /// 展开若走会话内省，只会得到一句「没挂载」。改为在这条**活连接**上开选源器，
    /// 由用户选表后载入临时表 / 落项目归档（`data_objects_load` 同一套确认动作）。
    pub expand_via: Option<ExpandVia>,
}

/// 源行展开的**另一条路**：不列自家挂载的表，而是去那条活连接上选表。
///
/// 只有「登记过、但没有 ATTACH 能力、当前连着」的连接才给——其余源行的展开
/// 仍走会话内省（`load_source_children`），两条路不重叠。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandVia {
    /// 在这条连接上开选源器（连接必须活着，否则选不出表）
    Picker { conn_id: String },
}

/// 源行行尾那一个动作位的形态（目前只有一种：挂载 / 卸载开关）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceAction {
    /// 有 ATTACH 能力的连接：一个位，挂上 ⇄ 卸下（= 启用 / 关闭它的联邦 catalog）。
    ///
    /// `path` 有值 = 文件库（本体是一个文件，挂的是路径）；`None` = 网络连接。
    /// 两种都走同一个位，动作在渲染侧分派（`mount_file_database` / `mount_network_source`）。
    ToggleMount {
        conn_id: String,
        path: Option<String>,
        mounted: bool,
    },
}

impl Group {
    /// 行首图标：**段决定形状**——扫一眼就知道这是源表、持久表还是临时表，
    /// 不用去读通道文案（原型 07 的 14px 图标列）
    pub fn icon(self) -> gpui_kit::assets::IconName {
        use gpui_kit::assets::IconName;
        match self {
            // 源是一个"库"（连接 / 挂载的库）：扫一眼与表、文件分开
            Self::Source => IconName::Database,
            Self::Table => IconName::Table,
            Self::File => IconName::FileSpreadsheet,
        }
    }
}

impl Object {
    /// 段内次序：同一段里「正在用的在上」。
    ///
    /// 三段之后同段混了多种身份（会话挂载 / 连接 / 资产 / 未登记 / 结果集），
    /// 只按名字排会把会话挂的源和连接打散，用户就找不到"刚刚挂上的那个"。
    /// 所以把段内次序写死在这里，`sort_key` 直接用；名字只在同档内比较。
    pub fn rank(&self) -> u8 {
        match self.group {
            Group::Source => match self.key.split(':').next() {
                Some("session") => 0,
                Some("fileview") => 1,
                // 源库连接（`conn:`）：也没挂任何东西，排在已挂载的后面
                _ => 2,
            },
            Group::Table => match self.key.split(':').next() {
                Some("asset") => 0,
                Some("persistent" | "project" | "global") => 1,
                // 结果集（`result:`）：寿命最短，排最后
                _ => 2,
            },
            Group::File => 0,
        }
    }

    /// 升序排序键：先分组、再段内档、再名字（同名按身份兜底，保证稳定）。
    ///
    /// 分组维度直接用 [`Group`] 的派生 `Ord`（= 声明顺序）：不再经 `order() -> u8`
    /// 转一道，免得「枚举加在哪」与「数字写几」各自为政。
    pub fn sort_key(&self) -> (Group, u8, String, String) {
        (self.group, self.rank(), self.name.clone(), self.key.clone())
    }

    /// 行首图标（失效行改警告形状：不靠删除线也能看出它不能点）
    pub fn icon(&self) -> gpui_kit::assets::IconName {
        use gpui_kit::assets::IconName;
        if self.can_analyze() {
            self.group.icon()
        } else {
            IconName::CircleAlert
        }
    }

    /// 能不能直接「分析」——失效与失败的行不给动作，只给「为什么」。
    pub fn can_analyze(&self) -> bool {
        self.state.is_usable()
    }

    /// 能不能进工作集（= 能不能作为「一个来源」递给洞察）。
    ///
    /// 与 [`Self::can_analyze`] 的差别在**一级行**：源库连接（`conn:`）本身不是数据对象，
    /// 它是「表的位置」——选一个连接进工作集只会得到一条转不了的来源。
    /// 能选的是它下面展开出来的表（`srctable:`）。
    pub fn can_pick(&self) -> bool {
        self.can_analyze() && !self.key.starts_with("conn:")
    }

    /// 行内小标（铁律 G 第 30 条）。
    ///
    /// 三段之后同段混着多种身份，小标就是**唯一不靠名字猜**的依据：
    /// 源段的叶子写 `库表`（数据还在源库），表段的表型资产把版本写进小标
    /// （`资产 vN`，它读的是资产的最新版本），其余沿用 [`Object::pill`] 的业务身份
    /// （`未登记` / `未登记 · 全局` / `结果集` / `联邦源` / `加速源` / `文件库` / `已加载`）。
    pub fn pill_text(&self) -> String {
        match (self.group, self.version) {
            // 源段的**叶子**（`srctable:`）才是「源库里的表」；源段的一级行是连接 /
            // 挂载的库，它们本身不是数据对象，保持自己的身份
            (Group::Source, _) if self.key.starts_with("srctable:") => "库表".to_string(),
            (Group::Table, Some(version)) => format!("资产 v{version}"),
            _ => self.pill.to_string(),
        }
    }

    /// 状态列写什么：**能用时写寿命，不能用时写原因**。
    ///
    /// 正常行一律显示「可用」等于白占一列（每行都一样的信息量）；用户真正要判断的是
    /// 「它会不会自己消失」（[`Life`]：外部 / 会话级 / 项目持久 / 受管）与
    /// 「它现在为什么用不了」（[`State`] 的原因）。所以两件事共用同一格：
    /// 可用 → 寿命；失效 / 失败 → 原话原因（那才是要动手解决的东西）。
    pub fn availability_text(&self) -> String {
        match self.state {
            State::Ready => self.life.label().to_string(),
            _ => self.state.label(),
        }
    }

    /// 状态列的**行上短码**（[`Self::availability_text`] 是全文，去属性窗口 / 右键菜单看）。
    ///
    /// 为什么必须分两档：整句原因实测 209px（12px 字号、30 字），面板 380px 宽时
    /// 状态格只有 40px 的下限位，整句会把状态块顶到行外 25px、把行尾按钮挤出可视区
    /// （`panels/data_objects_tab/layout_probe.rs` 量出来的数，不是估算）。
    /// 所以行上只放判据：能用 → 寿命；不能用 → **原因的第一段**（超过 6 字退化成
    /// 「失败」，宁可少说也不在窄面板里撑行）。
    pub fn availability_chip(&self) -> String {
        match &self.state {
            State::Ready => self.life.label().to_string(),
            State::Stale(_) => "已失效".to_string(),
            State::Failed(why) => {
                let head = why
                    .split(['·', '，', ',', '：', ':'])
                    .next()
                    .unwrap_or(why)
                    .trim();
                if !head.is_empty() && head.chars().count() <= 6 {
                    head.to_string()
                } else {
                    "失败".to_string()
                }
            }
        }
    }
}

/// 联邦会话里的一条源（面板「本次会话」段用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSource {
    /// 会话主人（文档 / 面板身份）：同一个别名可能在多个会话里同时存在，身份必须带它
    pub owner: String,
    pub alias: String,
    /// 挂上时的说明（表数量之类）
    pub note: String,
    /// 挂载失败的原话（引擎侧不翻译，界面也不改写）
    pub failed: Option<String>,
}

/// 源库连接（「源」段的一级行）。
///
/// 只带**展示信息**：表清单要逐层 introspect，属于面板展开时的懒加载，
/// 取数时不该顺手查一遍（打开面板就查所有连接 = 卡在 introspect 上）。
///
/// 收不收进面板由 `fed` 定（= 已注册成分析源，铁律 B 组第 4 条）：注册了但这一刻
/// 没连上的库也照样列（行上写「未连接」，不给展开）；没注册的一律不列——
/// 那是数据库导航栏的对象，不是分析源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnRow {
    pub id: String,
    pub name: String,
    /// 驱动 · 主机（可空段略去）
    pub note: String,
    /// 运行时是否已连上（连不上就没有表可列，行不可展开）
    pub connected: bool,
    /// 驱动 id（判「文件库」用：sqlite / duckdb 的本体是一个文件，不是网络库）
    pub driver: String,
    /// 库名 / 文件路径（`database` 字段；文件库驱动时是裸路径）
    pub database: Option<String>,
    /// **有没有 ATTACH 能力**（= 这个驱动能不能被 DuckDB 挂进联邦库）。
    ///
    /// 两类源行靠它分流（铁律第 42 条）：能力者行尾给「挂载 / 卸载」一个动作位；
    /// 无能力且已登记的只给小标「已登记」，**不给挂载**（永远挂不上）。
    /// 判据统一走 `AccelKind::from_db_type`，与连接对话框的开关门控同源。
    pub attach_capable: bool,
    /// 登记状态 = 项目库 `analysis_sources` 里有这一条（「项目级意图」）。
    ///
    /// **只认这一份**：`use_duckdb_fed` 是"能力/加速开关"，不是"我要注册这个源"
    /// （2026-11-25 用户口径）。登记了但连接不在清单里的，走 `reg:<id>` 行。
    pub registered: bool,
}

/// 资产清单里的一行（只读，只给最新版本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRow {
    pub name: String,
    pub version: u32,
    /// 资产面板那行尾部字段（行数 / 大小之类），原样带过来，不在这里重算
    pub detail: String,
    /// 本体在 `resources/` 下的相对路径（`None` = 引用型 / 旧行没登记）
    pub payload_rel_path: Option<String>,
    /// 存档库里的行 id：升级到全局时写进 `origin_ref`，作为全局版本的「上游」。
    pub resource_id: String,
    /// 表型资产在项目 DuckDB 里的实际表名（`None` = 文件型）。
    pub analytics_table: Option<String>,
}

/// 一次取数的结果（面板与对话框共用）。
///
/// 连接行只带展示信息（表清单由面板展开时懒加载，见 [`ConnRow`]）。
pub struct Collected {
    /// 已连接的源库连接（`connected` 为真的那些）
    pub connections: Vec<ConnRow>,
    /// 当前项目的分析库表清单。
    ///
    /// ⚠️ 只有 [`Self::project_open`] 为真时这份清单才是**权威全集**：没开项目时它必然是
    /// 空的（没有库可查），此时不能拿它判「某张表不在了」——否则每个表型资产都会被误标失效。
    pub analysis_tables: Vec<String>,
    /// 有没有打开项目（决定 `analysis_tables` 能不能当「表在不在」的判据）
    pub project_open: bool,
    pub global_tables: Vec<String>,
    pub result_sets: Vec<String>,
    /// 「载入内存」的源表副本（`tmp_l_*`，会话级；铁律第 12/35 条）
    pub loaded_tables: Vec<String>,
    pub insight_samples: Vec<String>,
    /// 「加源」加载进进程单例的文件视图：`(视图名, 原路径)`。视图不物化，
    /// 所以这里只记名字与出处，面板按它列行
    pub loaded_files: Vec<(String, String)>,
    /// 资产清单（由资产面板的已载行填；空 = 资产面板还没载过）
    pub assets: Vec<AssetRow>,
    /// 联邦会话里已挂上的源
    pub session_sources: Vec<SessionSource>,
    /// 本地加速会话已挂上的源（`owner` = 源连接 id；`alias` 已翻译成连接显示名）
    pub accel_sources: Vec<SessionSource>,
    /// 项目库 `analysis_sources` 的登记行（源段的「意图侧」）。
    ///
    /// 只存**项目级**意图；会话级加源不写库（切项目即清，写进去只会留假记录）。
    /// 老连接（只有 `use_duckdb_fed` 标记、还没写进新表）由 [`ConnRow::registered`] 兜着。
    pub registered_sources: Vec<engine::persistence::analysis_sources::AnalysisSourceRow>,
}

impl Collected {
    /// 只填连接数的小构造（测试与「只想知道有几条连接」的调用方用）。
    pub fn empty() -> Self {
        Self {
            connections: Vec::new(),
            analysis_tables: Vec::new(),
            // 没开项目：`analysis_tables` 不是权威全集，别拿它判失效
            project_open: false,
            global_tables: Vec::new(),
            result_sets: Vec::new(),
            loaded_tables: Vec::new(),
            insight_samples: Vec::new(),
            loaded_files: Vec::new(),
            assets: Vec::new(),
            session_sources: Vec::new(),
            accel_sources: Vec::new(),
            registered_sources: Vec::new(),
        }
    }
}

/// 原始清单 → 行模型（纯函数：不读盘、不查库，便于钉住身份与分组规则）。
pub fn objects(collected: &Collected) -> Vec<Object> {
    let mut out = Vec::new();

    // 登记侧的两把索引：按别名 / 本体路径各要一眼能查。
    // 只读借用，不克隆行——每次刷新都要过一遍，几百条时不值得复制。
    // （按连接 id 的那把在源段收口后已无消费方，2026-11-26 随告警清掉。）
    let registered_aliases: Vec<&str> = collected
        .registered_sources
        .iter()
        .map(|row| row.alias.as_str())
        .collect();
    let registered_paths: Vec<&str> = collected
        .registered_sources
        .iter()
        .filter_map(|row| row.path.as_deref())
        .collect();

    // 源段的排位：**已挂上的会话在最前**（正在生效的事实），分析底座紧随（常驻、不可卸载），
    // 然后才是候选（连接 / 文件库）。

    for source in &collected.session_sources {
        // 挂载事实 = 会话真起来了（失败态不算挂上）；登记侧按别名查
        let registered = registered_aliases.iter().any(|alias| alias == &source.alias);
        let pill = match source.failed.is_some() {
            // 挂载失败既不是「已挂载」也不该冒充三态里的任何一格：它就是失败
            true => "挂载失败",
            false => MountState::from_facts(registered, true).label(),
        };
        out.push(Object {
            // 身份带 owner：同名别名可能在多个会话里同时存在
            key: format!("session:{}:{}", source.owner, source.alias),
            name: source.alias.clone(),
            note: match &source.failed {
                Some(reason) => reason.clone(),
                // 小标现在是三态（规格 §3），挂载方式挪进副标题——不然用户分不清
                // 这个“别名”是联邦挂的、还是加源加出来的
                None => format!("联邦会话 · {} · {}", source.owner, source.note),
            },
            channel: Channel::Federated,
            life: Life::Session,
            state: match &source.failed {
                Some(reason) => State::Failed(reason.clone()),
                None => State::Ready,
            },
            group: Group::Source,
            pill,
            version: None,
            // 联邦源的表引用是 `<别名>.<schema>.<表>`，只给别名不算引用名
            sql_ref: None,
            expand_via: None,
            source_action: None,
        });
    }

    // 分析底座（`analytics` / `rds_global`）：常驻挂载、**不允许卸载**（铁律第 42 条），
    // 所以不给动作位；但必须在源段可见——用户要能确认"归档表 / 全局库已经在联邦里"，
    // 而不是切到联邦后凭记忆猜。路径由宿主发布（`analysis_bases::sync`），面板不查盘。
    // 排在会话之后：会话是"这次挂上的"，底座是"一直踩着的地基"。
    for base in engine::duckdb::federation::session::bases() {
        let global = base.alias == engine::duckdb::federation::session::GLOBAL_BASE_ALIAS;
        out.push(Object {
            key: format!("base:{}", base.alias),
            name: base.alias.clone(),
            note: if global {
                "全局分析库 · 只读挂载".to_string()
            } else {
                format!("项目分析库 · {}", base.path)
            },
            channel: Channel::Federated,
            life: Life::Project,
            state: State::Ready,
            group: Group::Source,
            // 静态串（`Object.pill` 是 `&'static str`）：不临时拼
            pill: "底座",
            version: None,
            sql_ref: None,
            // 底座不允许卸载：行尾没有动作位
            expand_via: None,
            source_action: None,
        });
    }

    // 本地加速源**不再出源段行**（2026-11-25 用户口径）：加速 = 一次一个库的 ATTACH，
    // 只在执行面存在，不进数据对象面板、也不算「已挂载」事实（铁律第 42 条末段）。

    // 源段的一级行 = 一条**已注册成分析源**的连接（`use_duckdb_fed`，零 I/O）。
    // 表清单不在这里——展开某个连接时才 introspect（面板 `load_source_children`），
    // 打开面板就查所有连接是不可接受的（N 个连接 × 3 层同步查询）。
    //
    // 注册了但没连上的也要列（铁律 B 组第 4 条）：用户刚切了项目、或连接暂时断了，
    // 源不会从清单里"蒸发"——行上写「未连接」，不给展开、不给选用。
    //
    // 文件库（sqlite / duckdb）还多一档：连上了但**没挂进分析实例**。本体路径不在
    // `loaded_files` 里就说明没挂——同样不给展开（没有表可列），只给「挂载」。
    // 挂没挂由路径判（见 [`unmounted_file_sources`]），不看连接状态：驱动能连上 ≠ 已 ATTACH。
    //
    // 网络连接同理（铁律 B 组第 4 条 + H 组第 37 条）：挂进分析实例的唯一凭据是**加速会话**
    // （`accel` 按 conn_id 登记，`Collected::accel_sources` 的 `owner` 就是 conn_id）。
    // 没有会话 = 实例里查不到它的表，展开只会得到一句报错——不如事先就说「未挂载」，
    // 行首不给展开指示（`State::Failed` 让 `can_analyze` 变假，chevron 换空位）。
    let unmounted: Vec<(String, String)> = unmounted_file_sources(collected);
    for conn in &collected.connections {
        let connected = conn.connected;
        let key = format!("conn:{}", conn.id);
        let file_path = file_source_path(conn);
        let not_mounted = unmounted
            .iter()
            .find(|(row_key, _)| row_key == &key)
            .map(|(_, path)| path.clone());
        // 登记过就把别名写进副标题：树的第一层是别名，行标题是连接名，不写就对不上
        let registered_alias = collected
            .registered_sources
            .iter()
            .find(|row| row.connection_id.as_deref() == Some(conn.id.as_str()))
            .map(|row| row.alias.clone());
        // 网络连接挂没挂 = **登记的别名在联邦/分析会话里有没有成事实**（会话行按别名列，
        // 不按连接 id）。加速会话不算（第 42 条末段）。
        let session_mounted = registered_alias.as_deref().is_some_and(|alias| {
            collected
                .session_sources
                .iter()
                .any(|source| source.alias == alias && source.failed.is_none())
        });
        let note = match (&file_path, &not_mounted) {
            // 文件库把本体路径写在副标题上：用户要确认"挂的是哪一个文件"
            (Some(path), _) => format!("文件库 · {path}"),
            _ => conn.note.clone(),
        };
        // 挂载事实：连上了 + 真挂进实例（文件库看 `loaded_files` 里有没有这个路径，
        // 网络连接只认**联邦 / 分析会话**——加速会话不算（第 42 条末段））。
        let mounted = connected
            && not_mounted.is_none()
            && (file_path.is_some() || session_mounted);
        // 小标（第 42 条 / 2026-11-25）：
        // - 无 ATTACH 能力但已登记 → 「已登记」（永远挂不上，只等导航里活连接后选表）；
        // - 有能力者走三态；「已连接、没挂、也没登记」只写「未挂载」（不给 scope）。
        let pill: &'static str = if !conn.attach_capable {
            if conn.registered {
                "已登记"
            } else {
                // 无能力且没登记不该进源段（`collect` 的过滤挡掉了），兜底也说清楚
                "未挂载"
            }
        } else if !conn.registered && !mounted {
            "未挂载"
        } else {
            MountState::from_facts(conn.registered, mounted).label()
        };
        // 登记过就把别名写进副标题：树的第一层是别名，行标题是连接名，不写就对不上
        let note = match registered_alias {
            Some(alias) => format!("{note} · 别名 {alias}"),
            None => note,
        };
        let state = if !connected {
            State::Failed("未连接 · 连上后可展开选表".to_string())
        } else if !conn.attach_capable {
            // 无 ATTACH 能力、已登记：活连接就能展开选表（选中的表走右键「载入临时表 /
            // 项目归档」进 DuckDB，产物是表段行，不是 catalog）
            State::Ready
        } else if not_mounted.is_some() {
            State::Failed("未挂载 · 右键挂载后可展开选表".to_string())
        } else if file_path.is_none() && !session_mounted {
            // 网络连接：没有联邦/分析会话就是没挂。展开它只会得到一句「先挂上」——
            // 那不该做成一个能点开才报错的行
            State::Failed("未挂载 · 右键「挂载」后才能展开选表".to_string())
        } else {
            State::Ready
        };
        out.push(Object {
            key,
            name: conn.name.clone(),
            note,
            channel: Channel::Source,
            life: Life::External,
            state,
            group: Group::Source,
            pill,
            version: None,
            // 连接本身不是数据对象：它下面展开的表才拼得出引用名
            sql_ref: None,
            // 行尾只有这一个位（第 42 条）：有 ATTACH 能力且已登记的连接给挂载 / 卸载；
            // 无能力（永远挂不上）、没登记（挂了也只是会话级）的都不给
            // 挂不上（没有 ATTACH 能力）但已登记、且这条连接活着：展开 = 去它身上选表
            expand_via: (!conn.attach_capable && conn.registered && conn.connected)
                .then(|| ExpandVia::Picker { conn_id: conn.id.clone() }),
            source_action: (conn.attach_capable && conn.registered).then(|| {
                SourceAction::ToggleMount {
                    conn_id: conn.id.clone(),
                    path: file_path.clone(),
                    mounted,
                }
            }),
        });
    }

    // 只剩登记、连连接行都没有的源：照样列一行（规格 §7「未挂载 · 项目级」）。
    // 动作在右键里（挂载 / 移除登记），行上不给展开——实例里没有东西可列。
    for row in unmounted_registrations(collected) {
        let note = match (row.connection_id.as_deref(), row.path.as_deref()) {
            (Some(id), _) => format!("连接 {id} 不在清单里 · 登记为源"),
            (None, Some(path)) => format!("文件库 · {path}"),
            (None, None) => "登记为源".to_string(),
        };
        let channel = match row.mount_kind {
            engine::persistence::analysis_sources::AnalysisMountKind::Accel => Channel::Accel,
            engine::persistence::analysis_sources::AnalysisMountKind::Federation => {
                Channel::Federated
            }
            _ => Channel::Source,
        };
        out.push(Object {
            // 身份用登记行 id：这些行没有连接/路径可当身份
            key: format!("reg:{}", row.id),
            name: row.alias.clone(),
            note,
            channel,
            life: Life::External,
            state: State::Failed("未挂载 · 右键「挂载」后才能展开选表".to_string()),
            group: Group::Source,
            pill: MountState::Unmounted.label(),
            version: None,
            sql_ref: None,
            expand_via: None,
            source_action: None,
        });
    }

    // 分析库里的两类表必须分开：`res_` 是「结果集落库」留下的，与项目自建表
    // 在用户心里不是一回事（前者可删、后者可能是他自己建的）
    // 注意返回顺序是 (普通表, 持久表)，写反会让两段整体错位
    //
    // 资产·表型的本体也在这一个库里（`information_schema` 会把它们一起列出）：
    // 不排除的话同一张表出两行，其中一行还会错标「项目自建」——它们只从资产行进
    // （pill「资产」、只读、带版本），归属由 M6 登记说了算
    let asset_tables: Vec<String> = collected
        .assets
        .iter()
        .filter_map(|asset| asset.analytics_table.clone())
        .collect();
    let (own, persistent) = split_persistent_tables(&collected.analysis_tables);
    let own = own
        .into_iter()
        .filter(|name| !asset_tables.contains(name))
        .collect::<Vec<_>>();
    let persistent = persistent
        .into_iter()
        .filter(|name| !asset_tables.contains(name))
        .collect::<Vec<_>>();
    for name in persistent {
        out.push(Object {
            key: format!("persistent:{name}"),
            name: persistent_table_label(&name),
            note: "可删除释放空间".to_string(),
            channel: Channel::Local,
            life: Life::Project,
            state: State::Ready,
            group: Group::Table,
            pill: "未登记",
            version: None,
            // 项目分析库挂在分析实例上，表名裸名可见（与「登记为资产」同一口径）
            sql_ref: Some(sql_ident(&name)),
            expand_via: None,
            source_action: None,
        });
    }
    for name in own {
        // 引用名先算：`name` 会在结构体里被 move 掉
        let sql_ref = Some(sql_ident(&name));
        out.push(Object {
            key: format!("project:{name}"),
            name,
            note: "项目自建 · 未登记".to_string(),
            channel: Channel::Local,
            life: Life::Project,
            state: State::Ready,
            group: Group::Table,
            pill: "未登记",
            version: None,
            sql_ref,
            expand_via: None,
            source_action: None,
        });
    }

    // 资产按**本体形态**分派（2026-11-18 口径：资产对象全部进面板）：
    // 表型本体在项目分析库里 → 持久表段；文件型本体在 `resources/` → 文件段。
    // pill 都是「资产」，行上标注只读 + 最新版本
    for asset in &collected.assets {
        // 铁律 D 组第 14 条：非结构化文件（sql / py / md…）不是数据，不进文件段。
        // 表型资产不受这条限制（本体是库里的表，不是文件）。
        if asset.analytics_table.is_none() && !file_asset_is_analytic(asset) {
            continue;
        }
        // 表型资产：存档登记还在，本体表却查不到了（表被删 / 换了项目）就是**真失效**。
        // 判据只取项目分析库清单，且必须「项目开着」才算权威（没项目时清单必然空）。
        let table_gone = match (asset.analytics_table.as_deref(), collected.project_open) {
            (Some(table), true) => !collected.analysis_tables.iter().any(|name| name == table),
            _ => false,
        };
        out.push(Object {
            // 身份用存档库的行 id（`resource_id`），不用名字：名字可改、可重名，
            // 用它当身份会让两份同名资产在工作集里互相顶掉（选中一个等于选中两个），
            // 改个名也会让「已选」凭空消失
            key: format!("asset:{}", asset.resource_id),
            name: asset.name.clone(),
            // 只读 + 最新版本：两件事都写在副标题，避免用户以为能选旧版。
            // 文件型再补取数模式（铁律 D 组第 15 条）：有副本 = 快照，无副本 = 按需（读源位置）。
            note: {
                let mode = match asset.analytics_table.is_some() {
                    // 表型没有「文件模式」这回事
                    true => String::new(),
                    false if asset.payload_rel_path.is_some() => " · 快照".to_string(),
                    false => " · 按需".to_string(),
                };
                match asset.detail.is_empty() {
                    true => format!("只读 · 最新 v{}{mode}", asset.version),
                    false => format!("只读 · 最新 v{} · {}{mode}", asset.version, asset.detail),
                }
            },
            channel: Channel::Asset,
            life: Life::Managed,
            state: if table_gone {
                State::Stale("分析库里的表已不在（被删或换了项目）")
            } else {
                State::Ready
            },
            group: if asset.analytics_table.is_some() {
                Group::Table
            } else {
                Group::File
            },
            pill: "资产",
            version: Some(asset.version),
            // 表型资产才有 SQL 引用（文件型本体是 `resources/` 下的副本，不是库里的表）
            sql_ref: asset.analytics_table.as_deref().map(sql_ident),
            expand_via: None,
            source_action: None,
        });
    }

    for name in &collected.global_tables {
        out.push(Object {
            key: format!("global:{name}"),
            name: name.clone(),
            note: "跨项目 · 只读 · 全局资产".to_string(),
            channel: Channel::Local,
            life: Life::Project,
            state: State::Ready,
            group: Group::Table,
            pill: "未登记 · 全局",
            version: None,
            // 全局分析库同样挂在分析实例上，裸名可见
            sql_ref: Some(sql_ident(&name)),
            expand_via: None,
            source_action: None,
        });
    }

    for name in &collected.result_sets {
        out.push(Object {
            key: format!("result:{name}"),
            name: name.clone(),
            note: "结果集临时表".to_string(),
            channel: Channel::Process,
            life: Life::Session,
            state: State::Ready,
            group: Group::Table,
            pill: "结果集",
            version: None,
            sql_ref: Some(sql_ident(&name)),
            expand_via: None,
            source_action: None,
        });
    }

    // 「载入内存」的源表副本（铁律第 12/35 条）：与结果集同段、同寿命档，
    // 但小标写「载入」——它**不是查询产物**，是用户显式要的一份源表副本；
    // 也因此不开放「升级为资产」（第 11 条只给 `tmp_q_`）。
    for name in &collected.loaded_tables {
        out.push(Object {
            key: format!("load:{name}"),
            name: name.clone(),
            note: "源表副本 · 载入内存".to_string(),
            channel: Channel::Process,
            life: Life::Session,
            state: State::Ready,
            group: Group::Table,
            pill: "载入",
            version: None,
            sql_ref: Some(sql_ident(&name)),
            expand_via: None,
            source_action: None,
        });
    }

    // Mock（`tmp_m_`）与插件（`tmp_p_`）产物**不进面板**（2026-11-18 用户口径）：
    // mock 的下游是草稿箱 / 外部目录 / 资产归档，插件产物归插件——它们是各模块的
    // 生成物，不是分析链路的输入对象；与 `tmp_i_`（洞察样本）同一条纪律。

    // 洞察产物（`tmp_i_`）**不进面板**：它是抽取的样，不是全量；生命周期属于洞察
    // 会话。要留就由用户在洞察里「保留到项目库」，落成持久表后再从这里看。

    // 注册成分析源的文件库连接的本体路径：下面判「catalog 行要不要让位给连接行」用它。
    // 一次算出来（而不是每行遍历一遍连接清单）：路径比较还带斜杠/大小写归一。
    let fed_file_paths: Vec<String> = collected
        .connections
        .iter()
        .filter_map(file_source_path)
        .collect();

    // 已加载进进程单例的文件，按**加载形态**分派（2026-11-18 拍板：库文件走 ATTACH）：
    // - 数据文件（csv / parquet / excel…）是 `f_` 视图 → 文件段，视图名即 SQL 里
    //   能直接用的名字，路径放 note；
    // - 数据库文件（.duckdb / .sqlite）ATTACH 成了 catalog → 「已挂载的源」段
    //   （与网络源同一段：都是挂进来的库），行可展开列其表 / 视图（复用源行叶子）。
    //   按路径扩展判定——加载那一刻是什么形态，这里就是什么形态
    for (alias, path) in &collected.loaded_files {
        let is_database = engine::duckdb::file_view::is_database_file(path);
        // 文件库既是连接又是 catalog：注册成分析源的连接已经占了源段那一行（挂载后
        // conn 行就能展开选表），这里不要再给同一路径单出一行「文件库」——
        // 两行同名同源，用户分不清该点哪个。没有对应连接的（加源直接挑的文件库）照出。
        let owned_by_connection = is_database
            && fed_file_paths
                .iter()
                .any(|registered| same_file(registered, path));
        if owned_by_connection {
            continue;
        }
        out.push(Object {
            key: format!("fileview:{alias}"),
            name: alias.clone(),
            // 加源进来的文件是 `f_*` 视图、**不物化**：文件改了下次查询跟着变，
            // 所以标「按需」（铁律 D 组第 16 条）。想固定就归档成快照。
            note: format!("按需 · {path}"),
            channel: Channel::File,
            life: Life::External,
            state: State::Ready,
            group: if is_database {
                Group::Source
            } else {
                Group::File
            },
            pill: if is_database {
                // 库文件挂成 catalog：三态小标（登记过 = 项目级；加源随手挂的 = 会话级）
                let registered = registered_paths
                    .iter()
                    .any(|known| same_file(known, path));
                MountState::from_facts(registered, true).label()
            } else {
                "已加载"
            },
            version: None,
            // 文件视图是 `f_*` 裸名；库文件是 catalog，引用要 `别名.schema.表`，不在这里编
            sql_ref: (!is_database).then(|| sql_ident(alias)),
            expand_via: None,
            source_action: None,
        });
    }

    out.sort_by_key(|o| o.sort_key());
    out
}

/// 工作集软上限（**软**：超过只提示代价，不硬拦）。
///
/// 多源洞察一次全取各源样本，源越多首次取样越慢；8 是「一次还说清」的量级。
/// 用户仍可继续加——拦下来等于替他决定要不要等。
pub const PICKED_SOFT_LIMIT: usize = 8;

/// 工作集增删（**纯函数**）：再点一次同一个对象 = 移除，否则追加。
///
/// 抽出来是因为它是「多选」的唯一判据：面板回调只负责借用与通知，序列语义在这里
/// 用测试钉住（软上限提示按 `len()` 生效，判据写错会让提示链整体变成死代码）。
///
/// 顺序按**加入先后**保留：tray 与洞察来源顺序都读它，重排会让用户认不出自己刚加的那个。
pub fn toggle_in_place(picked: &mut Vec<String>, key: &str) {
    match picked.iter().position(|item| item == key) {
        Some(index) => {
            picked.remove(index);
        }
        None => picked.push(key.to_string()),
    }
}

/// 超过软上限时的提示（`None` = 没超，不提示）
pub fn picked_limit_note(count: usize) -> Option<String> {
    (count > PICKED_SOFT_LIMIT).then(|| {
        format!(
            "工作集已有 {count} 个对象（超过 {PICKED_SOFT_LIMIT} 个后首次取样会明显变慢）；\
             仍可继续加，觉得慢再收窄"
        )
    })
}

/// 字节数转人读串：面板头部显示 DuckDB 内存占用用，一位小数够说明量级。
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} B"),
        _ => format!("{value:.1} {}", UNITS[unit]),
    }
}

/// 临时表名反推来源：`drop_temp_table` 会按来源前缀校验，传错来源直接拒。
///
/// 前缀不借用字符串常量而逐个 `prefixes()` 比对——前缀改了这里跟着改。
pub fn temp_source_for(name: &str) -> Option<engine::duckdb::temp_table::TempTableSource> {
    use engine::duckdb::temp_table::TempTableSource;
    [
        TempTableSource::Query,
        TempTableSource::Insight,
        TempTableSource::Mock,
        TempTableSource::Plugin,
        TempTableSource::Load,
    ]
    .into_iter()
    .find(|source| {
        source
            .prefixes()
            .iter()
            .any(|prefix| name.starts_with(prefix))
    })
}

/// 会话行上能做的动作（后端已齐，`engine::duckdb::federation::session`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAction {
    /// 按同一份描述重挂这个源（表清单可能变了）
    RefreshSource { owner: String, alias: String },
    /// 换主源：未限定名只在主源里解析
    SetPrimary { owner: String, alias: String },
    /// 关掉整个会话（后端没有「断单个源」，别在界面上发明一个）
    DropSession { owner: String },
}

impl SessionAction {
    pub fn label(&self) -> String {
        match self {
            Self::RefreshSource { alias, .. } => format!("重挂源：{alias}"),
            Self::SetPrimary { alias, .. } => format!("设为主源：{alias}"),
            Self::DropSession { .. } => "关闭这个会话".to_string(),
        }
    }
}

/// 某个会话行可用的动作（**纯函数**）：设为主源只给**可用的**行——
/// 不可用的源设不成主源（引擎也会拒），与其点一下拿一句报错，不如不给入口。
pub fn session_actions(key: &str, state: &State) -> Vec<SessionAction> {
    let Some((owner, alias)) = key
        .strip_prefix("session:")
        .and_then(|rest| rest.rsplit_once(':'))
    else {
        return Vec::new();
    };
    let (owner, alias) = (owner.to_string(), alias.to_string());
    let mut actions = vec![SessionAction::RefreshSource {
        owner: owner.clone(),
        alias: alias.clone(),
    }];
    if matches!(state, State::Ready) {
        actions.push(SessionAction::SetPrimary {
            owner: owner.clone(),
            alias,
        });
    }
    actions.push(SessionAction::DropSession { owner });
    actions
}

/// 源行展开时要去的连接（**纯函数**：面板据此分派「去哪列举表」）。
///
/// 可展开的源行只有三类：联邦源（`session:{owner}:{alias}`）、数据库文件（`fileview:{alias}`）、
/// 源库连接（`conn:{conn_id}`）；其它 key 一律 `None`——表 / 文件 / 资产行没有下一层。
///
/// 本地加速**没有**自己的源行：它是执行通道（`use_duckdb_fed = true` 时那条连接走加速），
/// 不占源段，所以也不需要 `accel:` 这种 key。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    /// 联邦会话里的源（owner = 会话主人，alias = 挂载别名）
    Federated { owner: String, alias: String },
    /// ATTACH 进进程单例的数据库文件（alias = 挂载别名）
    FileView { alias: String },
    /// **还没进 DuckDB** 的源库连接：展开要逐层 introspect（catalog → schema → 表）
    Connection { conn_id: String },
}

/// 从行 key 解出 [`SourceRef`]（与 `session_actions` 同一套 `rsplit_once` 口径：
/// owner 里可能带冒号——文档名，按**最后一个**冒号切）。
pub fn source_ref(key: &str) -> Option<SourceRef> {
    if let Some(conn_id) = key.strip_prefix("conn:") {
        return (!conn_id.is_empty()).then(|| SourceRef::Connection {
            conn_id: conn_id.to_string(),
        });
    }
    if let Some(alias) = key.strip_prefix("fileview:") {
        // ATTACH 进单例的数据库文件：表清单走同一份列举实现
        return (!alias.is_empty()).then(|| SourceRef::FileView {
            alias: alias.to_string(),
        });
    }
    let rest = key.strip_prefix("session:")?;
    let (owner, alias) = rest.rsplit_once(':')?;
    (!owner.is_empty() && !alias.is_empty()).then(|| SourceRef::Federated {
        owner: owner.to_string(),
        alias: alias.to_string(),
    })
}

/// 源库表（连接里的表）的身份：`srctable:{conn}|{catalog}|{schema}|{table}`。
///
/// 为什么分隔符用 `|` 不用 `:`：`key.split_once(':')` 只有一处（前缀）、余下四段里
/// catalog / schema / table 都可能含冒号，用 `|` 让「段数」与「段内容」互不干扰。
pub fn source_table_key(conn: &str, catalog: &str, schema: &str, table: &str) -> String {
    format!("srctable:{conn}|{catalog}|{schema}|{table}")
}

/// 反解 [`source_table_key`]：`None` = 不是源库表身份（或缺段）。
pub fn source_table_parts(key: &str) -> Option<(&str, &str, &str, &str)> {
    let rest = key.strip_prefix("srctable:")?;
    let mut parts = rest.split('|');
    let conn = parts.next()?;
    let catalog = parts.next()?;
    let schema = parts.next()?;
    let table = parts.next()?;
    // 多出来的 `|` = 段数不对（表名里带 `|` 会走到这里）：宁可不认，也不猜
    if parts.next().is_some() || conn.is_empty() || table.is_empty() {
        return None;
    }
    Some((conn, catalog, schema, table))
}

/// 源库表叶子行（展开某个连接后按表清单逐条造）：`note` 写所在 catalog / schema。
pub fn source_table_object(conn: &str, catalog: &str, schema: &str, table: &str) -> Object {
    let scope = [catalog, schema]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".");
    Object {
        key: source_table_key(conn, catalog, schema, table),
        name: table.to_string(),
        note: if scope.is_empty() {
            "源库表".to_string()
        } else {
            scope
        },
        channel: Channel::Source,
        life: Life::External,
        state: State::Ready,
        group: Group::Source,
        pill: "库表",
        version: None,
        // 源库表要在这条连接上查（`SELECT … FROM <catalog>.<schema>.<表>`），
        // 引用名必须带连接上下文，不是这里能给的一句话
        sql_ref: None,
        expand_via: None,
        source_action: None,
    }
}

/// 已挂载源的叶子按 schema 分组（三级树第二层）：返回 `(schema 名, 组内下标)`。
///
/// 按**首见顺序**切段（引擎列举时已按 schema、名字排好，这里不再重排——再排一次
/// 就可能与「复制限定名」抄走的顺序对不上）。`indices` 指向入参本身，调用方按它取
/// 元素，不为了一屏几百张表每帧克隆一遍清单。
pub fn schema_groups(tables: &[engine::duckdb::accel::MountedTable]) -> Vec<(String, Vec<usize>)> {
    let mut order: Vec<String> = Vec::new();
    let mut buckets: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, table) in tables.iter().enumerate() {
        // 空 schema 归到 `main`：DuckDB 的外源目录里 schema 可能为空串（sqlite 等），
        // 画一条空名分组行等于让用户点一个没有名字的东西
        let label = match table.schema.is_empty() {
            true => "main".to_string(),
            false => table.schema.clone(),
        };
        if !buckets.contains_key(&label) {
            order.push(label.clone());
        }
        buckets.entry(label).or_default().push(index);
    }
    order
        .into_iter()
        .map(|label| {
            let indices = buckets.remove(&label).unwrap_or_default();
            (label, indices)
        })
        .collect()
}

/// 源库连接（`srctable:` 行）按 catalog + schema 分组：同一棵三级树的第二层。
///
/// 层名在 catalog 与 schema 都有时写 `catalog.schema`（源库的 catalog 就是库名，
/// 少了它分不清 `orders.main` 与 `bi.main`）。反解不出身份的行走 `main`——
/// 与其丢一行，不如把它挂在一个说明得通的分组下。
pub fn conn_schema_groups(children: &[Object]) -> Vec<(String, Vec<usize>)> {
    let mut order: Vec<String> = Vec::new();
    let mut buckets: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, child) in children.iter().enumerate() {
        let label = source_table_parts(&child.key)
            .map(
                |(_, catalog, schema, _)| match (catalog.is_empty(), schema.is_empty()) {
                    (false, false) => format!("{catalog}.{schema}"),
                    (false, true) => catalog.to_string(),
                    (true, false) => schema.to_string(),
                    (true, true) => "main".to_string(),
                },
            )
            .unwrap_or_else(|| "main".to_string());
        if !buckets.contains_key(&label) {
            order.push(label.clone());
        }
        buckets.entry(label).or_default().push(index);
    }
    order
        .into_iter()
        .map(|label| {
            let indices = buckets.remove(&label).unwrap_or_default();
            (label, indices)
        })
        .collect()
}

/// 已选对象 → 洞察取样来源（面板「分析已选」的取数口径，**纯函数**）。
///
/// 返回 `(来源, 跳过原因)`：能给出**完整取数描述**的对象才转成来源，
/// 转不了的老实说为什么——「点了没反应」比「说清楚不支持」糟得多。
///
/// 目前能转的：项目分析库表 / 全局分析库表 / 结果集 / 洞察产物。
/// 转不了的：资产（面板只知道名字与版本，不知道本体是表还是文件）、
/// 文件（草稿行有路径但这里没按扩展名选读取器）、联邦源与加速源（表在各自的
/// 专用连接上，进程单例取样够不着——等公共入口 Phase 3「洞察搬结果」）。
/// 这几类都能在 02 对话框里选——那是它们的正路，面板不抢。
pub fn analysis_sources(
    objects: &[Object],
    picked: &[String],
    assets: &[AssetRow],
    resources_dir: Option<&std::path::Path>,
) -> (Vec<SampleSource>, Vec<String>) {
    let mut sources: Vec<SampleSource> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for key in picked {
        let Some(object) = objects.iter().find(|o| &o.key == key) else {
            continue;
        };
        let source = match key.split_once(':') {
            Some(("project", table)) => Some(SampleSource::on_duckdb(
                format!("SELECT * FROM \"{table}\""),
                format!("analytics.{table}"),
            )),
            Some(("global", table)) => {
                Some(SampleSource::on_global(table, format!("全局 · {table}")))
            }
            // 结果集 `tmp_q_` 与洞察产物 `tmp_i_` 都在进程单例里：取样即「再取一份」
            Some(("result", table)) | Some(("insight", table)) => Some(SampleSource::on_duckdb(
                format!("SELECT * FROM \"{table}\""),
                object.name.clone(),
            )),
            // 已加载的**数据文件视图**也在单例上,同样能取样;库文件挂的是 catalog
            // (在 MountedSources 段),没有"整库取样"这回事,落到兜底分支老实说
            Some(("fileview", alias)) if object.group == Group::File => Some(
                SampleSource::on_duckdb(format!("SELECT * FROM \"{alias}\""), object.name.clone()),
            ),
            // 资产：本体是 `resources/` 下的受管文件，读它 = 按扩展名交给 DuckDB 读。
            // 身份段是 `resource_id`（见 `objects()` 里资产行的 key），按 id 找本体：
            // 按名字找会在重名时拿错那一份
            Some(("asset", id)) => {
                let row = assets.iter().find(|a| a.resource_id == *id);
                match (row.and_then(|r| r.payload_rel_path.as_ref()), resources_dir) {
                    (Some(rel), Some(dir)) => {
                        match SampleSource::duckdb_file(dir.join(rel), object.name.clone()) {
                            Ok(source) => Some(source),
                            Err(e) => {
                                skipped.push(format!("{}（{}）", object.name, e));
                                continue;
                            }
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        match source {
            Some(source) => {
                // 去重按整条来源（同一张表被两种身份指到时合成一条）
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
            None => {
                // 跳过的原因要说**为什么取不了数**，不要拿行内小标凑数：
                // 小标现在是挂载三态（`已挂载 · 会话级`），说不了「整库没有取样这回事」
                let reason = if key.starts_with("asset:") {
                    "资产本体不可读"
                // 库文件挂的是 catalog，整库取样不成立：要在源段里按库表选
                } else if matches!(key.split_once(':'), Some(("fileview", _))) {
                    "整库文件，按库表取样"
                } else if object.group == Group::Source {
                    "源上还没有可读的表"
                } else {
                    "这类行不能直接取样"
                };
                skipped.push(format!("{}（{}）", object.name, reason))
            }
        }
    }

    (sources, skipped)
}

/// 文件型资产能不能当文件参与分析（铁律 D 组第 14 条）。
///
/// 判据与归档侧的 `structured` **同源**：`engine::duckdb::file_reader_function`
/// （csv / parquet / excel / json 认得；sql / py / md 认不得）。有副本时按副本扩展名判；
/// 引用型资产拿不到本体路径（`payload_rel_path = None`），这里**不猜**——保留并标
/// 「按需」，让"读源位置"这条语义仍然可用，宁可多一行也不静默吞掉用户的东西。
pub fn file_asset_is_analytic(asset: &AssetRow) -> bool {
    match asset.payload_rel_path.as_deref() {
        Some(rel) => engine::duckdb::file_reader_function(rel).is_some(),
        None => true,
    }
}

/// 这一行是不是「加源进来的外部文件」（`f_*` 视图，按需读原文件）。
///
/// 只认 `fileview:` 前缀且落在文件段：库文件（`.duckdb/.sqlite`）虽然也叫 `f_*`，
/// 但它在源段（catalog，不是视图），不该给「归档为快照」——那是把库文件当数据文件折一份，
/// 语义不对。
pub fn can_archive_snapshot(object: &Object) -> bool {
    object.key.starts_with("fileview:") && object.group == Group::File
}

/// `fileview:` 行背后的视图别名（用来回查它的绝对路径）。
pub fn fileview_alias(key: &str) -> Option<&str> {
    key.strip_prefix("fileview:")
}

/// 文件库连接（sqlite / duckdb）的本体路径：`None` = 网络库，或没填 `database`。
pub fn file_source_path(conn: &ConnRow) -> Option<String> {
    if !connection_dialog::services::data_source_service::is_file_db_driver(&conn.driver) {
        return None;
    }
    let database = conn.database.as_deref()?.trim();
    if database.is_empty() {
        return None;
    }
    Some(
        connection_dialog::services::data_source_service::normalize_file_db_path(
            &conn.driver,
            database,
        ),
    )
}

/// 两条路径是不是同一个文件（Windows 上盘符 / 大小写不同也算同一个）。
fn same_file(left: &str, right: &str) -> bool {
    left.replace('\\', "/")
        .eq_ignore_ascii_case(&right.replace('\\', "/"))
}

/// 源清单里「注册成分析源、但**还没挂进分析实例**」的文件库行：`(行 key, 本体路径)`。
///
/// 挂没挂的判据只有一条：这个路径在不在 `loaded_files` 里（加源 / 挂载都会写这一份）。
/// 没挂的源不可展开（铁律 B 组第 4 条）——面板据此把 chevron 换空位、只给「挂载」。
pub fn unmounted_file_sources(collected: &Collected) -> Vec<(String, String)> {
    file_source_rows(collected, false)
}

/// 已经挂进分析实例的文件库行：`(行 key, 本体路径)`（面板据此给「卸载 / 重新挂载」）。
pub fn mounted_file_sources(collected: &Collected) -> Vec<(String, String)> {
    file_source_rows(collected, true)
}

/// 文件库连接按挂载状态分组（`want_mounted` 选要哪一边）。
///
/// 两个对外函数共用这一份判据：卸载 / 挂载是同一件事的两种状态，若各写一套比较逻辑，
/// 早晚出现「挂载里有、卸载里没有」的行——那行就永远卸不掉。
/// 登记在项目库、但这一刻**连行都没能出**的源（规格 §7 的 `I \ F` 那一格）。
///
/// 为什么不跟 `ConnRow` 一起出：这些登记指向的连接可能已经不在清单里
/// （连接被删、切了项目、文件库路径改了名），`Collected` 里根本没有它的连接行。
/// 只靠事实侧成行，登记侧就永远看不见——用户重启后会发现源"蒸发"了，
/// 那正是铁律第 4 / 42 条要防的事。
///
/// 已经被事实行代表的登记不回在这里（按别名认，挂载三态由此对得上）：
/// - 别名在分析实例的加速档里 → 那条连接自己会出行，写着「已挂载 · 项目级」；
/// - 别名在没失败的联邦会话里 → `session:` 行同理；
/// - 连接本身在 `connections` 里 → `conn:` 行会自己说「未挂载 · 项目级」。
pub fn unmounted_registrations(
    collected: &Collected,
) -> Vec<&engine::persistence::analysis_sources::AnalysisSourceRow> {
    collected
        .registered_sources
        .iter()
        .filter(|row| {
            let alias = row.alias.as_str();
            if collected
                .accel_sources
                .iter()
                .any(|source| source.alias == alias)
            {
                return false;
            }
            if collected
                .session_sources
                .iter()
                .any(|source| source.alias == alias && source.failed.is_none())
            {
                return false;
            }
            match (row.connection_id.as_deref(), row.path.as_deref()) {
                // 连接登记：连接还在清单里就不归这里管（conn 行自己会说未挂载）
                (Some(id), _) => !collected.connections.iter().any(|conn| conn.id == id),
                // 文件库登记：本体路径还挂着某条连接就不归这里管
                (None, Some(path)) => !collected.connections.iter().any(|conn| {
                    file_source_path(conn)
                        .map(|seen| same_file(&seen, path))
                        .unwrap_or(false)
                }),
                (None, None) => true,
            }
        })
        .collect()
}

fn file_source_rows(collected: &Collected, want_mounted: bool) -> Vec<(String, String)> {    collected
        .connections
        .iter()
        .filter_map(|conn| {
            let path = file_source_path(conn)?;
            let mounted = collected
                .loaded_files
                .iter()
                .any(|(_, seen)| same_file(seen, &path));
            (mounted == want_mounted).then(|| (format!("conn:{}", conn.id), path))
        })
        .collect()
}

/// SQL 标识符的转义口径（与 `result_set_actions::select_sql_for_table` 同一规则：
/// 表名可能带引号，`tmp_i_` 的描述段来自用户）。
pub fn sql_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 这一行能不能「存到项目分析库」。
///
/// 只有**结果集**（`tmp_q_`，进程单例里的表）可以：落库就是整表搬一份，不需要重建。
/// 源库表 / 文件 / 资产各有各的路（连接下推 / reader 函数 / 只读受管），别把它们塞进
/// 这条通道——那会给「只读资产」写出一份副本，用户再也分不清哪份是本体。
/// 判据收在 **key 前缀**而不是整段：段里将来再进新对象（Mock / 插件曾被误放过又撤出，
/// 2026-11-18）也不会误放开这条通道。
pub fn can_persist(object: &Object) -> bool {
    object.key.starts_with("result:") && object.can_analyze()
}

/// 这一行能不能被「清理临时对象」删掉。
///
/// 口径 = **本面板可弃的会话级对象**：结果集（`result:` → `tmp_q_`）与载入
/// （`load:` → `tmp_l_`）。两者都是「删了要重跑 / 重新载入才有」的占用，没有留存价值；
/// 洞察取样（`tmp_i_`）与 mock 产物归各自模块管，从这里删会让它们的登记指向空表。
///
/// 与 [`can_persist`] 的关系（2026-11-24 调整）：载入档**只能删、不能升级**
/// （第 11 条只给 `tmp_q_`），所以「进 / 出对称」不再是两者的等式；仍然守住的是
/// **不得多删**——清理集合之外的 key 一律不给这颗按钮。
pub fn can_clear(object: &Object) -> bool {
    object.key.starts_with("result:") || object.key.starts_with("load:")
}

/// 落库时给人看的名字：去掉临时前缀（`tmp_q_` / `tmp_i_`），别让表名里再出现一层 `tmp_`。
pub fn persist_label(table: &str) -> String {
    table
        .strip_prefix("tmp_q_")
        .or_else(|| table.strip_prefix("tmp_i_"))
        .unwrap_or(table)
        .to_string()
}

/// 现取一次（面板打开 / 刷新时调）。
///
/// 资产清单由调用方填：资产面板实体启动即构造并预载快照（`build_resources_panel` 尾部
/// `refresh_resources`），项目切换与资产增删都会推新——面板 refresh 时从它的 `view_rows`
/// 翻译即可，取数只有这一份（这里不另开连接查库，避免两套真相）。
pub fn collect(shared: &Shared) -> Collected {
    let project = shared.project.borrow().clone();
    let project_open = project.is_some();
    // 源段的**意图侧**：项目库 `analysis_sources` 的登记行（规格 §3）。
    // 读路径只开一次 SQLite、不建库、不造项目骨架（见 `list_in_project`）；
    // 读失败就当没有——登记是可选信息，不该把整个面板拖垮。
    let registered_sources = project
        .as_ref()
        .and_then(|p| engine::persistence::analysis_sources::list_in_project(&p.root).ok())
        .unwrap_or_default();
    // 执行侧（联邦计划，工作线程）拿不到项目根，靠这份进程级快照认别名；
    // 每次都整体换掉：切项目后不许留上个项目的键（`publish_aliases` 先清后填）
    engine::persistence::analysis_sources::publish_aliases(&registered_sources);
    // 底座也跟着清单同步一次：切项目不重启时，`analytics` / `rds_global` 的路径必须跟着换
    // （会话指纹含底座，下一次开联邦会话就会重建）
    crate::services::analysis_bases::sync(project.as_ref().map(|p| p.root.as_path()));
    let registered_conn_ids: Vec<&str> = registered_sources
        .iter()
        .filter_map(|row| row.connection_id.as_deref())
        .collect();

    // 源段收两类连接（铁律第 42 条末段 / 2026-11-25 用户口径）：
    // 1. `use_duckdb_fed` 有值 = 有 ATTACH 能力且已声明当 DuckDB 源 → **必须出**；
    // 2. 开关为空（没有 ATTACH 能力）但**已进项目分析源表** → 也出（小标「已登记」，
    //    不给挂载，只在导航里是活连接时可展开选表）。
    // 其余一律不进：源段不做探索搜查（"可挂但没挂也没登记"的走「增源」进来）。
    let connections: Vec<ConnRow> = shared
        .connections
        .borrow()
        .iter()
        // 源段出行按三态：`use_duckdb_fed` **有值**（true/false）= 能被 DuckDB 直连，必须出；
        // `None` = 不可直连，只有已登记（历史遗留）才兜底出行。
        .filter(|item| item.use_duckdb_fed.is_some() || registered_conn_ids.iter().any(|id| *id == item.id))
        .map(|item| ConnRow {
            id: item.id.clone(),
            name: item.name.clone(),
            note: match item.host.as_deref().filter(|host| !host.is_empty()) {
                Some(host) => format!("{} · {}", item.driver, host),
                None => item.driver.clone(),
            },
            connected: item.connected,
            driver: item.driver.clone(),
            database: item.database.clone(),
            // 能力判据只走引擎那一份（`AccelKind::from_db_type`），UI 不另判一套；
            // 这里手上有的是驱动 id（`mysql_native` / `postgres` / `sqlite` / `duckdb` / `oracle`），
            // 它按前缀认驱动，与 `db_type` 同一套规则
            attach_capable: engine::duckdb::accel::AccelKind::from_db_type(&item.driver).is_ok(),
            // 登记只认项目库那一行：`use_duckdb_fed` 是能力/加速开关，**不是**选表的意图
            registered: registered_conn_ids.iter().any(|id| *id == item.id),
        })
        .collect();
    let analysis_tables = project
        .as_ref()
        .map(|p| {
            analysis_tables_of(
                &engine::persistence::project_db::ProjectDatabaseManager::analysis_db_path(&p.root),
            )
        })
        .unwrap_or_default();
    let global_tables =
        engine::analysis::global::global_tables("C_data_objects").unwrap_or_default();
    let result_sets = engine::analysis::result_set::list_result_sets().unwrap_or_default();
    let loaded_tables = engine::analysis::result_set::list_loaded_tables().unwrap_or_default();
    let insight_samples = engine::analysis::result_set::list_insight_samples().unwrap_or_default();
    // 联邦源：直接读**分析实例上的会话**（2026-11-26 单实例收口）。
    //
    // 以前读 `federation::session::owners()` 那份门面缓存，但它已经没有生产写入通道
    // （`kernel::open` 只开实例会话 + `mount_sources`），面板因此永远看不到挂上的源。
    // 现在按 `label == "联邦"` 认会话、逐会话取挂载快照；主源来自 `use_catalog` 的记录。
    let session_sources = engine::duckdb::instance::current()
        .map(|instance| {
            let mut rows = Vec::new();
            for info in instance.sessions() {
                if info.label != "联邦" {
                    continue;
                }
                let Some(session) = instance.session(&info.id) else {
                    continue;
                };
                let primary = session.primary_alias();
                for mounted in session.mounted_sources() {
                    let alias = mounted.alias().to_string();
                    let (note, failed) = match &mounted.state {
                        engine::duckdb::federation::registry::MountState::Ready { tables } => (
                            format!(
                                "{} 张表 · {}",
                                tables,
                                if primary.as_deref() == Some(alias.as_str()) {
                                    "主源"
                                } else {
                                    "已挂载"
                                }
                            ),
                            None,
                        ),
                        engine::duckdb::federation::registry::MountState::Failed(reason) => {
                            (String::new(), Some(reason.clone()))
                        }
                    };
                    rows.push(SessionSource {
                        owner: info.owner.clone(),
                        alias,
                        note,
                        failed,
                    });
                }
            }
            rows
        })
        .unwrap_or_default();

    // 挂在分析实例上的源连接：树根写**实际别名**（冲突让位后可能是 `rds_src_2`），
    // 连接名进 note。用户要认「这是哪条连接」，但树根必须与 `duckdb_databases()`
    // 里那个 catalog 名一致，否则三级树的第一层就跟事实对不上。
    let accel_sources = {
        let connections = shared.connections.borrow();
        engine::duckdb::instance::current()
            .map(|instance| instance.mounted_conns())
            .unwrap_or_default()
            .into_iter()
            .map(|(conn_id, alias, kind)| {
                let conn_name = connections
                    .iter()
                    .find(|item| item.id == conn_id)
                    .map(|item| item.name.clone())
                    .unwrap_or_else(|| conn_id.clone());
                SessionSource {
                    owner: conn_id,
                    alias,
                    note: format!("{conn_name} · {} · 分析实例挂载", kind.label()),
                    failed: None,
                }
            })
            .collect()
    };

    Collected {
        connections,
        analysis_tables,
        project_open,
        global_tables,
        result_sets,
        loaded_tables,
        insight_samples,
        loaded_files: shared.loaded_files.borrow().clone(),
        assets: Vec::new(),
        session_sources,
        accel_sources,
        registered_sources,
    }
}

#[cfg(test)]
mod tests {
    // 刻意不用 `use super::*`：会把 gpui 的 `test` 属性宏带进作用域。
    use super::{
        can_archive_snapshot, fileview_alias, objects, AssetRow, Channel, Collected, ConnRow,
        Group, Life, MountState, Object, SessionSource, State,
    };
    use engine::persistence::analysis_sources::{
        AnalysisMountKind, AnalysisSourceKind, AnalysisSourceRow,
    };

    fn registered_row(id: &str, name: &str, conn: Option<&str>, alias: &str) -> AnalysisSourceRow {
        AnalysisSourceRow {
            id: id.to_string(),
            name: name.to_string(),
            kind: AnalysisSourceKind::Conn,
            connection_id: conn.map(str::to_string),
            path: None,
            alias: alias.to_string(),
            mount_kind: AnalysisMountKind::Accel,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn sample() -> Collected {
        Collected {
            connections: vec![ConnRow {
                id: "C_mysql_9".into(),
                name: "订单库".into(),
                note: "MySQL · 10.0.0.8".into(),
                connected: true,
                driver: "mysql".into(),
                database: Some("orders".into()),
                attach_capable: true,
                registered: true,
            }],
            analysis_tables: vec!["res_订单明细_20261115103000".into(), "数据修复".into()],
            // sample 代表「项目开着」的常态：`analysis_tables` 是权威全集
            project_open: true,
            global_tables: vec!["G_月度经营".into()],
            result_sets: vec!["tmp_q_结果_3".into()],
            loaded_tables: vec!["tmp_l_orders_shop_20261124120000_ab12cd34".into()],
            insight_samples: vec!["tmp_i_orders_sample".into()],
            loaded_files: vec![
                ("f_sales".into(), "D:/数据/sales_2026q1.csv".into()),
                ("f_warehouse".into(), "D:/数据/warehouse.duckdb".into()),
            ],
            assets: vec![AssetRow {
                name: "月度经营快照".into(),
                version: 3,
                detail: "1,204 行 · 18.2 MB".into(),
                payload_rel_path: Some("assets/月度经营快照.parquet".into()),
                resource_id: "arp_demo".into(),
                // 文件型资产：没有表名，`promote_target` 会拒（本项目只接表型）
                analytics_table: None,
            }],
            session_sources: vec![
                SessionSource {
                    owner: "编辑器 · 分析.sql".into(),
                    alias: "mysql_src".into(),
                    note: "42 张表 · 主源".into(),
                    failed: None,
                },
                SessionSource {
                    owner: "编辑器 · 分析.sql".into(),
                    alias: "oracle_src".into(),
                    note: String::new(),
                    failed: Some("扩展未安装".into()),
                },
            ],
            accel_sources: vec![SessionSource {
                owner: "C_mysql_9".into(),
                // 加速档的 catalog 名固定是 `rds_src`；连接名进 note（见 `collect`）
                alias: "rds_src".into(),
                note: "订单库 · MySQL · 本地加速会话".into(),
                failed: None,
            }],
            // 一条项目级登记：订单库以 `mysql_src` 挂进联邦会话（sample 的会话行里有它）
            registered_sources: vec![registered_row(
                "src_1",
                "订单库",
                Some("C_mysql_9"),
                "mysql_src",
            )],
        }
    }

    /// 分组顺序是「越靠上越正在用」：源在最前，文件在最后。
    #[test]
    fn groups_follow_the_declared_order() {
        let rows = objects(&sample());
        let groups: Vec<Group> = rows.iter().map(|o| o.group).collect();
        let mut sorted = groups.clone();
        sorted.sort();
        assert_eq!(groups, sorted, "行必须按分组顺序连续排列");
        // 源段在最前：会话挂载、连接、文件库都在这里
        assert_eq!(rows.first().map(|o| o.group), Some(Group::Source));
        // 文件段在最后
        assert_eq!(rows.last().map(|o| o.group), Some(Group::File));
        // 三段都要真出现过（并成三段后不许某段永远是空的）
        for group in Group::ALL {
            assert!(
                rows.iter().any(|o| o.group == group),
                "{} 段没有行，这条断言失去意义",
                group.title()
            );
        }
    }

    /// 段内次序：源段会话挂载在最上、连接在最下；表段资产在未登记之前、结果集最后。
    #[test]
    fn rank_puts_the_objects_in_use_first_within_a_group() {
        // 表型资产要真出现在 sample 里，才能验「表段从资产开始」——sample 默认只有
        // 文件型资产（它在文件段），拿它当表段第一行会让断言变成假的
        let mut c = sample();
        c.analysis_tables.push("arp_客户画像_v2".into());
        c.assets.push(AssetRow {
            name: "客户画像".into(),
            version: 2,
            detail: String::new(),
            payload_rel_path: None,
            resource_id: "arp_2".into(),
            analytics_table: Some("arp_客户画像_v2".into()),
        });
        let rows = objects(&c);
        let ranks_of = |group: Group| {
            rows.iter()
                .filter(|o| o.group == group)
                .map(|o| o.rank())
                .collect::<Vec<_>>()
        };
        let mut source = ranks_of(Group::Source);
        let sorted = source.clone();
        source.sort();
        assert_eq!(source, sorted, "源段内次序要被 rank 排好：{:?}", source);
        // 段内第一档必须是会话挂载（正在用的那个），最后一档是连接
        assert_eq!(source.first(), Some(&0), "会话挂载排第一档");
        assert_eq!(source.last(), Some(&3), "源库连接排最后一档");

        let mut table = ranks_of(Group::Table);
        assert_eq!(table[0], 0, "表段从资产开始");
        table.sort();
        assert_eq!(ranks_of(Group::Table), table, "表段内次序要被 rank 排好");
        assert!(
            rows.iter()
                .find(|o| o.key.starts_with("result:"))
                .is_some_and(|o| o.rank() == 2),
            "结果集寿命最短，排表段最后"
        );
    }

    /// 段顺序的唯一权威 = `Group::ALL`，它必须与派生 `Ord`（声明顺序）完全一致。
    ///
    /// 视图的筛选 chips 直接遍历这个常量；一旦有人把新段插在声明中间却忘了改常量
    /// （或反过来），排序（`Ord`）与筛选（`ALL`）就会各说各话——这条测试当场拦住。
    #[test]
    fn groups_list_all_in_ord_order() {
        let all = Group::ALL;
        // 一段不漏
        for group in all {
            assert_eq!(
                Group::ALL.iter().filter(|item| **item == group).count(),
                1,
                "{} 在 ALL 里重复了",
                group.title()
            );
        }
        // 顺序与 Ord 一致（严格递增）
        for pair in all.windows(2) {
            assert!(
                pair[0] < pair[1],
                "{} 与 {} 的顺序与声明相反",
                pair[0].title(),
                pair[1].title()
            );
        }
    }

    /// `res_` 表换可读名；落库 / 自建 / 全局同在「表」段，靠小标区分身份。
    #[test]
    fn unregistered_tables_share_the_table_group_and_carry_tags() {
        let rows = objects(&sample());
        let persistent = rows
            .iter()
            .find(|o| o.key.starts_with("persistent:"))
            .unwrap();
        assert_eq!(persistent.name, "订单明细 · 2026-11-15 10:30:00");
        // 身份仍是原名：面板上改了显示，删除 / 分析仍按原名定位
        assert_eq!(persistent.key, "persistent:res_订单明细_20261115103000");

        // 落库 / 自建 / 全局同段（都是库里现有的表），小标负责说清是哪一类
        assert_eq!(persistent.group, Group::Table);
        let own = rows.iter().find(|o| o.key.starts_with("project:")).unwrap();
        assert_eq!(own.group, Group::Table);
        let global = rows.iter().find(|o| o.key.starts_with("global:")).unwrap();
        assert_eq!(global.group, Group::Table);
        assert_eq!(own.name, "数据修复");
        assert_eq!(own.pill, "未登记");
        assert_eq!(persistent.pill, "未登记");
        assert_eq!(global.pill, "未登记 · 全局");
    }

    /// 资产·表型只从资产行进：`information_schema` 会把它与自建表一起列出，
    /// 不排除的话同一张表出两行、其中一行错标「项目自建」——SQL 解析上它确实
    /// 不冲突（同名靠 catalog 区分），但**面板清单**上两行会让人以为有两份数据。
    #[test]
    fn asset_backed_tables_appear_only_as_asset_rows() {
        let mut c = sample();
        c.analysis_tables = vec!["数据修复".into(), "arp_客户画像_v2".into()];
        c.assets.push(AssetRow {
            name: "客户画像".into(),
            version: 2,
            detail: String::new(),
            payload_rel_path: None,
            resource_id: "arp_2".into(),
            analytics_table: Some("arp_客户画像_v2".into()),
        });
        let rows = objects(&c);
        // 表名只出现一次：资产行（不在「项目自建」里再列一遍）
        let hits: Vec<&Object> = rows
            .iter()
            .filter(|o| o.name == "arp_客户画像_v2" || o.key == "asset:arp_2")
            .collect();
        assert_eq!(hits.len(), 1, "同一张表只该有一行：{hits:?}");
        assert_eq!(hits[0].pill, "资产", "归属由 M6 登记说了算：{hits:?}");
        // 自建表不受影响
        assert!(rows.iter().any(|o| o.key == "project:数据修复"));
    }

    /// 资产身份用 `resource_id`，不用名字。
    ///
    /// 名字可改、可重名：拿名字当 key 时两份同名资产会合成一个身份（选中一份等于
    /// 两份都选中），取样还可能读到另一份的本体；改名则让「已选」凭空消失。
    #[test]
    fn asset_identity_is_the_resource_id_not_the_name() {
        let mut c = sample();
        c.assets = vec![
            AssetRow {
                name: "月度经营".into(),
                version: 2,
                detail: String::new(),
                payload_rel_path: Some("assets/a.parquet".into()),
                resource_id: "arp_a".into(),
                analytics_table: None,
            },
            AssetRow {
                name: "月度经营".into(),
                version: 5,
                detail: String::new(),
                payload_rel_path: Some("assets/b.parquet".into()),
                resource_id: "arp_b".into(),
                analytics_table: None,
            },
        ];
        let rows = objects(&c);
        let assets: Vec<&Object> = rows.iter().filter(|o| o.pill == "资产").collect();
        assert_eq!(assets.len(), 2, "同名两份资产必须各占一行：{assets:?}");
        assert_eq!(assets[0].key, "asset:arp_a");
        assert_eq!(assets[1].key, "asset:arp_b");
        assert_ne!(assets[0].key, assets[1].key);

        // 取样按身份找本体：选 arp_b 就得读 b 那份，不能按名字读成 a
        let dir = std::env::temp_dir();
        let picked = vec!["asset:arp_b".to_string()];
        let (sources, skipped) =
            super::analysis_sources(&rows, &picked, &c.assets, Some(dir.as_path()));
        assert_eq!(sources.len(), 1, "{skipped:?}");
        assert!(
            sources[0].sql.contains("b.parquet"),
            "按身份取本体：{}",
            sources[0].sql
        );
    }

    /// 资产行必须标出「最新版本」且只读，并且不给版本选择（模型层没有这个字段）。
    /// 表型 / 文件型资产都进面板（2026-11-18 口径），按本体形态分派段。
    #[test]
    fn asset_rows_carry_latest_version_and_are_read_only() {
        let mut c = sample();
        // 表型资产（本体是项目分析库里的表）
        c.assets.push(AssetRow {
            name: "客户画像".into(),
            version: 2,
            detail: String::new(),
            payload_rel_path: None,
            resource_id: "arp_2".into(),
            analytics_table: Some("arp_客户画像_v2".into()),
        });
        let rows = objects(&c);
        let asset = rows.iter().find(|o| o.key == "asset:arp_demo").unwrap();
        // 文件型（本体在 resources/）→ 文件段
        assert_eq!(asset.group, Group::File, "文件型资产按本体进文件段");
        let table_asset = rows.iter().find(|o| o.key == "asset:arp_2").unwrap();
        // 表型（本体在项目分析库）→ 表段
        assert_eq!(table_asset.group, Group::Table, "表型资产按本体进表段");
        assert_eq!(asset.version, Some(3));
        assert!(asset.note.contains("只读"));
        // 有副本（`payload_rel_path` 有值）= 快照；表型没有文件模式这一说
        assert!(
            asset.note.contains("快照"),
            "有副本的文件资产标快照：{}",
            asset.note
        );
        assert!(
            !table_asset.note.contains("快照") && !table_asset.note.contains("按需"),
            "表型资产不该带文件模式：{}",
            table_asset.note
        );
        assert_eq!(asset.channel, Channel::Asset);
        assert_eq!(asset.life, Life::Managed);
        // 资产可以参与分析
        assert!(asset.can_analyze());
    }

    /// 铁律 D 组第 14 条：非结构化文件不是数据，不进文件段；引用型不猜、标「按需」。
    #[test]
    fn non_structured_files_stay_out_of_the_files_group() {
        let mut c = sample();
        c.assets.push(AssetRow {
            name: "分析.sql".into(),
            version: 1,
            detail: String::new(),
            payload_rel_path: Some("assets/分析.sql".into()),
            resource_id: "arp_sql".into(),
            analytics_table: None,
        });
        c.assets.push(AssetRow {
            name: "外链.csv".into(),
            version: 1,
            detail: String::new(),
            payload_rel_path: None,
            resource_id: "arp_ref".into(),
            analytics_table: None,
        });
        let rows = objects(&c);
        assert!(
            rows.iter().all(|o| o.key != "asset:arp_sql"),
            "sql 资产不该出现在文件段"
        );
        let reference = rows.iter().find(|o| o.key == "asset:arp_ref").unwrap();
        assert!(
            reference.note.contains("按需"),
            "引用型要标按需：{}",
            reference.note
        );
    }

    /// 只有文件段里的 `f_*` 视图给「归档为快照」；库文件在源段（catalog），不给。
    #[test]
    fn only_file_views_offer_a_snapshot() {
        let mut c = sample();
        c.loaded_files = vec![
            ("f_orders".into(), "D:/data/orders.csv".into()),
            ("f_warehouse".into(), "D:/data/warehouse.duckdb".into()),
        ];
        let rows = objects(&c);
        let file = rows.iter().find(|o| o.key == "fileview:f_orders").unwrap();
        assert_eq!(file.group, Group::File);
        assert!(can_archive_snapshot(file));
        assert_eq!(fileview_alias(&file.key), Some("f_orders"));
        let database = rows
            .iter()
            .find(|o| o.key == "fileview:f_warehouse")
            .unwrap();
        assert_eq!(database.group, Group::Source);
        assert!(!can_archive_snapshot(database), "库文件是源，不该折成快照");
        // 资产行不在这条动作里：它们已经是快照/按需的既成事实，没有再折一次的道理
        let asset = rows.iter().find(|o| o.key == "asset:arp_demo").unwrap();
        assert!(!can_archive_snapshot(asset));
    }

    /// 小标（铁律 G 第 30 条）：源段叶子写 `库表`，表段表型资产写 `资产 vN`，
    /// 其余沿用业务身份。这些区分是**同表两段出现、同段多种身份**时唯一不靠猜的依据。
    #[test]
    fn pills_distinguish_a_table_that_appears_in_both_groups() {
        // 源段的叶子：数据还在源库，选它 = 从源库取样
        let leaf = super::source_table_object("C_mysql_9", "orders_db", "public", "orders");
        assert_eq!(leaf.pill_text(), "库表");
        // 源段的一级行是连接，不是表：保持「源连接」身份
        let conn = Object {
            key: "conn:C_mysql_9".into(),
            pill: "源连接",
            ..leaf.clone()
        };
        assert_eq!(conn.pill_text(), "源连接");
        // 表段：同样叫 orders 的那条读的是资产的最新版本
        let asset = Object {
            key: "asset:arp_1".into(),
            name: "orders".into(),
            group: Group::Table,
            pill: "资产",
            version: Some(4),
            ..leaf.clone()
        };
        assert_eq!(asset.pill_text(), "资产 v4");
        // 非表型资产（文件段）没有版本语义，沿用业务身份
        let file_asset = Object {
            group: Group::File,
            version: Some(1),
            ..asset.clone()
        };
        assert_eq!(file_asset.pill_text(), "资产");
    }

    /// 行上的 `sql_ref` 只在「分析实例里裸名可见」的对象上给值：
    /// 结果集 / 项目库表 / 全局库表 / 表型资产 / `f_*` 视图有；源库表、联邦源、加速源、
    /// 文件库（catalog）没有（要带连接上下文才拼得出，宁可空着也不给错的）。
    #[test]
    fn sql_references_only_cover_bare_name_objects() {
        let mut c = sample();
        c.loaded_files = vec![
            ("f_orders".into(), "D:/data/orders.csv".into()),
            ("f_warehouse".into(), "D:/data/warehouse.duckdb".into()),
        ];
        c.assets.push(AssetRow {
            name: "客户画像".into(),
            version: 1,
            detail: String::new(),
            payload_rel_path: None,
            resource_id: "arp_2".into(),
            analytics_table: Some("res_客户画像_1".into()),
        });
        let rows = objects(&c);
        let by = |key: &str| rows.iter().find(|o| o.key == key).unwrap();
        for key in ["fileview:f_orders", "asset:arp_2", "asset:arp_demo"] {
            if key == "asset:arp_demo" {
                // 文件型资产没有 SQL 引用（本体是 `resources/` 下的副本）
                assert_eq!(by(key).sql_ref, None, "{key}");
                continue;
            }
            assert!(by(key).sql_ref.is_some(), "{key} 应当有引用名");
        }
        // 库文件是 catalog：`别名.schema.表` 才查得到，行上不给
        assert_eq!(by("fileview:f_warehouse").sql_ref, None);
        // 源库连接行、结果集都按各自口径
        assert_eq!(by("conn:C_mysql_9").sql_ref, None);
        assert!(by("result:tmp_q_结果_3")
            .sql_ref
            .as_deref()
            .is_some_and(|s| s.starts_with('"')));
    }

    /// 临时对象是会话级：失效时仍留在清单里，但不再给「分析」动作。
    #[test]
    fn session_objects_are_session_lived_and_stale_rows_are_not_analyzable() {
        let rows = objects(&sample());
        let result = rows.iter().find(|o| o.key.starts_with("result:")).unwrap();
        assert_eq!(result.group, Group::Table, "临时表落在表段，不单独成段");
        assert_eq!(result.pill_text(), "结果集");
        assert_eq!(result.life, Life::Session);

        let stale = Object {
            state: State::Stale("会话已重建"),
            ..result.clone()
        };
        assert!(!stale.can_analyze(), "失效行不该还能点分析");
        assert!(stale.state.label().contains("会话已重建"));
    }

    /// 联邦源挂载失败：留在源段，并把原因写在副标题。
    #[test]
    fn failed_session_sources_stay_in_the_session_group_with_reason() {
        let rows = objects(&sample());
        let failed = rows
            .iter()
            .find(|o| matches!(o.state, State::Failed(_)))
            .unwrap();
        assert_eq!(failed.group, Group::Source);
        assert_eq!(failed.pill, "挂载失败", "挂载失败的行不许冒充三态里的任何一格");
        assert_eq!(failed.note, "扩展未安装");
        assert!(!failed.can_analyze());
    }

    /// 同一个别名挂在两个会话里时，必须能分开（身份带 owner）。
    ///
    /// 不带 owner 的话两行 key 一样，工作集里点一个就等于点了两个。
    #[test]
    fn same_alias_in_two_sessions_keeps_distinct_identities() {
        let mut c = sample();
        c.session_sources = vec![
            SessionSource {
                owner: "编辑器 · a.sql".into(),
                alias: "mysql_src".into(),
                note: "42 张表 · 主源".into(),
                failed: None,
            },
            SessionSource {
                owner: "编辑器 · b.sql".into(),
                alias: "mysql_src".into(),
                note: "18 张表 · 已挂载".into(),
                failed: None,
            },
        ];
        let rows = objects(&c);
        // 按 `session:` 键前缀滤（段里还有加速源行，不能按整段数）
        let session_rows: Vec<&Object> = rows
            .iter()
            .filter(|o| o.key.starts_with("session:"))
            .collect();
        assert_eq!(session_rows.len(), 2);
        assert_ne!(session_rows[0].key, session_rows[1].key);
        // 界面上仍显示同一个别名（用户认的是别名，不是 owner）
        assert_eq!(session_rows[0].name, "mysql_src");
        // 主源标记要带在副标题里
        assert!(session_rows[0].note.contains("主源"));
    }

    /// 同一张表在两份分析库里时必须能区分（身份带前缀），否则工作集里会互相顶掉。
    #[test]
    fn identities_are_unique_across_sources() {
        let mut c = sample();
        c.analysis_tables = vec!["orders".into()];
        c.global_tables = vec!["orders".into()];
        c.assets = vec![AssetRow {
            name: "orders".into(),
            version: 1,
            detail: String::new(),
            payload_rel_path: None,
            resource_id: "arp_orders".into(),
            analytics_table: None,
        }];
        c.result_sets = vec!["orders".into()];
        let rows = objects(&c);
        let keys: Vec<&str> = rows.iter().map(|o| o.key.as_str()).collect();
        assert!(keys.contains(&"project:orders"));
        assert!(keys.contains(&"global:orders"));
        assert!(keys.contains(&"asset:arp_orders"));
        assert!(keys.contains(&"result:orders"));
        let unique: std::collections::BTreeSet<&str> = keys.iter().copied().collect();
        assert_eq!(unique.len(), keys.len(), "身份必须全局唯一");
    }

    /// 「载入内存」的表进表段：小标「载入」、会话级、**不给升级为资产**（铁律第 11/35 条）。
    #[test]
    fn loaded_tables_are_session_rows_without_upgrade() {
        let c = sample();
        let rows = objects(&c);
        let loaded = rows
            .iter()
            .find(|row| row.key.starts_with("load:"))
            .expect("载入表要进表段");
        assert_eq!(loaded.pill_text(), "载入");
        assert_eq!(loaded.group, Group::Table);
        assert_eq!(loaded.life, Life::Session);
        assert!(!super::can_persist(loaded), "第 11 条只给 tmp_q_ 升级");
        assert!(loaded.sql_ref.is_some(), "进程单例里的表裸名可见");
    }

    /// 「分析已选」：能转的转成来源，转不了的说清原因，不许静默丢。
    #[test]
    fn analysis_sources_convert_what_they_can_and_name_what_they_cannot() {
        let mut c = sample();
        c.analysis_tables = vec!["orders".into()];
        c.global_tables = vec!["G_月度经营".into()];
        c.result_sets = vec!["tmp_q_结果_3".into()];
        let rows = objects(&c);
        let picked = vec![
            "project:orders".to_string(),
            "global:G_月度经营".to_string(),
            "result:tmp_q_结果_3".to_string(),
            "asset:arp_demo".to_string(),
        ];
        let (sources, skipped) = super::analysis_sources(&rows, &picked, &c.assets, None);

        assert_eq!(sources.len(), 3, "三类可转的来源都要在：{:?}", sources);
        // 项目分析库表在 DuckDB 会话里直接读
        assert!(sources[0].sql.contains("\"orders\""));
        assert_eq!(sources[0].label, "analytics.orders");
        // 全局库表走 `on_global`（别名由内核拼，入口不起名）
        assert!(sources[1].sql.starts_with("SELECT * FROM rds_global."));
        assert!(sources[1].global);
        // 结果集在进程单例里
        assert!(sources[2].sql.contains("tmp_q_结果_3"));

        assert_eq!(skipped.len(), 1, "资产转不了要说出来：{skipped:?}");
        assert!(skipped[0].contains("资产"), "{skipped:?}");

        // 同一张表被两种身份指到：只留一条，别开两个一样的标签页
        let dup = super::analysis_sources(
            &rows,
            &["result:tmp_q_结果_3".into(), "result:tmp_q_结果_3".into()],
            &c.assets,
            None,
        );
        assert_eq!(dup.0.len(), 1, "{:?}", dup.0);
    }

    /// 只有结果集（`result:`）能走落库通道；给人看的名字要去掉临时前缀。
    /// 判据用 key 前缀而不是整段：段里将来再进新对象（Mock / 插件曾被误放过，
    /// 2026-11-18 撤出）也不会误放开这条通道。
    #[test]
    fn only_result_sets_can_be_persisted_and_labels_drop_the_temp_prefix() {
        let rows = objects(&sample());
        assert!(
            rows.iter().any(|o| o.key == "result:tmp_q_结果_3"),
            "sample 要有结果集行：{rows:?}"
        );
        for object in &rows {
            let expected = object.key.starts_with("result:");
            assert_eq!(
                super::can_persist(object),
                expected,
                "{} 该不该能落库判错了",
                object.key
            );
        }
        assert_eq!(super::persist_label("tmp_q_结果_3"), "结果_3");
        assert_eq!(super::persist_label("tmp_i_orders_sample"), "orders_sample");
        assert_eq!(super::persist_label("orders"), "orders");
    }

    /// 「升级为资产」与「清理」必须对称：都只认 `result:`——进 / 出两个动作同一口径，
    /// 谁单独放宽都会留下"能进不能出"或"能删不该删"的对象。
    #[test]
    fn clear_covers_results_and_loads_but_persist_only_covers_results() {
        let rows = objects(&sample());
        // 能升级的必然也能清理（不能出现「能进不能出」）
        for object in &rows {
            if super::can_persist(object) {
                assert!(super::can_clear(object), "{} 能升级就要能清理", object.key);
            }
        }
        // 载入档反过来：只能清理、不能升级（第 11 条只给 `tmp_q_`）
        let loaded = rows
            .iter()
            .find(|o| o.key.starts_with("load:"))
            .expect("样本里要有载入表");
        assert!(super::can_clear(loaded));
        assert!(!super::can_persist(loaded));
    }

    /// 已加载文件按形态分派：库文件（ATTACH 成 catalog）进「已挂载的源」段、
    /// 可展开（source_ref 认它）；数据文件视图进「文件」段、能转洞察取样来源。
    #[test]
    fn loaded_files_split_by_form_database_vs_view() {
        let rows = objects(&sample());
        let db_file = rows
            .iter()
            .find(|o| o.key == "fileview:f_warehouse")
            .expect("库文件要有行");
        assert_eq!(db_file.group, Group::Source, "ATTACH 的库是源");
        assert_eq!(db_file.pill, "已挂载 · 会话级", "加源随手挂的库文件没登记：会话级");
        assert!(
            super::source_ref(&db_file.key).is_some(),
            "库文件行可展开列表"
        );
        let view = rows
            .iter()
            .find(|o| o.key == "fileview:f_sales")
            .expect("数据文件视图要有行");
        assert_eq!(view.group, Group::File);
        assert_eq!(view.pill, "已加载");

        // 视图能转取样来源（它在单例上）；库文件 catalog 转不了，老实进跳过清单
        let picked = vec![
            "fileview:f_sales".to_string(),
            "fileview:f_warehouse".to_string(),
        ];
        let (sources, skipped) = super::analysis_sources(&rows, &picked, &sample().assets, None);
        assert_eq!(sources.len(), 1, "{sources:?}");
        assert!(sources[0].sql.contains("f_sales"), "{sources:?}");
        assert_eq!(skipped.len(), 1, "库文件整库取样不成立：{skipped:?}");
        assert!(
            skipped[0].contains("整库"),
            "跳过原因要说清是整库（不是把挂载三态当原因）：{skipped:?}"
        );
    }

    /// 本地加速会话**不产生源段行**（第 42 条末段）：加速一次一个库，只在执行面存在。
    #[test]
    fn accel_sessions_do_not_produce_source_rows() {
        let rows = objects(&sample());
        assert!(
            rows.iter().all(|o| !o.key.starts_with("accel:")),
            "加速度会话不该出现在数据对象面板"
        );
    }

    /// 三级树的第二层：叶子按 schema 切段，同一 schema 的连续下标归一组。
    /// 乱序输入也按**首见顺序**分组，不按名字重排。
    #[test]
    fn schema_groups_split_leaves_by_schema_in_first_seen_order() {
        use engine::duckdb::accel::MountedTable;
        let table = |schema: &str, name: &str| MountedTable {
            schema: schema.into(),
            name: name.into(),
            is_view: false,
            qualified: name.into(),
        };
        let leaves = vec![
            table("main", "orders"),
            table("main", "users"),
            table("sales", "deals"),
            // 空 schema 归到 main（sqlite 等外源可能没有 schema 名）
            table("", "notes"),
        ];
        let groups = super::schema_groups(&leaves);
        assert_eq!(groups[0].0, "main");
        assert_eq!(groups[0].1, vec![0, 1, 3], "main 组要含空 schema 那张");
        assert_eq!(groups[1].0, "sales");
        assert_eq!(groups[1].1, vec![2]);

        // 首见顺序：先出现 sales 就把 sales 排第一
        let reversed = vec![table("sales", "a"), table("main", "b")];
        assert_eq!(super::schema_groups(&reversed)[0].0, "sales");
    }

    /// 源库连接的表行（`srctable:`）按 catalog + schema 分组：`orders.main` 与
    /// `bi.main` 必须是两组，不能合成一个 `main`。
    #[test]
    fn conn_schema_groups_keep_catalog_in_the_label() {
        use super::source_table_object;
        let rows = vec![
            source_table_object("C_1", "orders", "main", "t1"),
            source_table_object("C_1", "orders", "main", "t2"),
            source_table_object("C_1", "bi", "main", "t3"),
        ];
        let groups = super::conn_schema_groups(&rows);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "orders.main");
        assert_eq!(groups[0].1, vec![0, 1]);
        assert_eq!(groups[1].0, "bi.main");
        assert_eq!(groups[1].1, vec![2]);
    }

    /// 三态 = 登记 × 挂载（规格 §3）。四种组合只有三种会被摆到界面上；
    /// 「既没登记也没挂上」要按「未挂载 · 项目级」说，不许冒充「已挂载 · 会话级」。
    #[test]
    fn mount_state_joins_registry_and_facts() {
        assert_eq!(
            MountState::from_facts(true, true).label(),
            "已挂载 · 项目级"
        );
        assert_eq!(
            MountState::from_facts(false, true).label(),
            "已挂载 · 会话级"
        );
        assert_eq!(
            MountState::from_facts(true, false).label(),
            "未挂载 · 项目级"
        );
        assert_eq!(
            MountState::from_facts(false, false).label(),
            "未挂载 · 项目级",
            "两半都没有时不冒充「已挂载 · 会话级」"
        );
    }

    /// 没登记、会话在：源段只说「未挂载」（没登记就没有 scope 可宣称）。
    #[test]
    fn mounted_without_registry_is_session_level() {
        let mut c = sample();
        c.registered_sources.clear();
        c.connections[0].registered = false;

        let conn = objects(&c)
            .into_iter()
            .find(|o| o.key == "conn:C_mysql_9")
            .expect("连接行要在");
        assert_eq!(
            conn.pill, "未挂载",
            "没登记就不知道自己挂在哪、也不该冒充「项目级」"
        );
    }

    /// 无 ATTACH 能力但已登记：小标只写「已登记」，不给挂载（挂不上）。
    #[test]
    fn registered_without_attach_capability_says_registered() {
        let mut c = sample();
        c.connections[0].attach_capable = false;
        let conn = objects(&c)
            .into_iter()
            .find(|o| o.key == "conn:C_mysql_9")
            .expect("已登记的无能力源也要出");
        assert_eq!(conn.pill, "已登记");
        assert_eq!(conn.group, Group::Source);
    }

    /// 登记指向的连接已经不在清单里：源不许「蒸发」，照样出一行「未挂载 · 项目级」。
    #[test]
    fn registrations_without_a_connection_row_still_show_unmounted() {
        let mut c = sample();
        c.connections.clear();
        c.accel_sources.clear();
        // 会话里那条同别名的源也要清：不清的话这份登记已经被"会话已挂上"解释掉了，
        // 测的就不是"连接不在清单里"这一格
        c.session_sources.clear();

        let row = objects(&c)
            .into_iter()
            .find(|o| o.key == "reg:src_1")
            .expect("登记行要在");
        assert_eq!(row.group, Group::Source);
        assert_eq!(row.pill, "未挂载 · 项目级");
        assert!(matches!(row.state, State::Failed(_)), "{:?}", row.state);
        assert!(row.note.contains("C_mysql_9"), "{}", row.note);
    }

    /// 已连接、没挂、也没登记：这一行是可挂的源，scope 还没定，别冒充「项目级」。
    #[test]
    fn connected_but_unmounted_and_unregistered_says_plain_unmounted() {
        let mut c = sample();
        c.registered_sources.clear();
        c.accel_sources.clear();
        c.connections[0].registered = false;

        let conn = objects(&c)
            .into_iter()
            .find(|o| o.key == "conn:C_mysql_9")
            .expect("连接行要在");
        assert_eq!(conn.pill, "未挂载", "没登记就不说项目级");
    }

    /// 源行 key → 展开目标：联邦源按最后一个冒号切（owner 可能带冒号）；
    /// 其它行没有下一层，`accel:` 已经不是源行的 key 了。
    #[test]
    fn source_ref_parses_source_kinds_and_rejects_flat_rows() {
        assert_eq!(
            super::source_ref("session:编辑器 · a.sql:mysql_src"),
            Some(super::SourceRef::Federated {
                owner: "编辑器 · a.sql".into(),
                alias: "mysql_src".into(),
            })
        );
        assert!(
            !objects(&sample())
                .iter()
                .any(|o| o.key.starts_with("accel:")),
            "加速档不占源段：不许再出 `accel:` 行"
        );
        assert_eq!(
            super::source_ref("accel:C_mysql_9"),
            None,
            "加速档没有自己的源行，`accel:` 前缀不再被认"
        );
        assert_eq!(
            super::source_ref("fileview:f_warehouse"),
            Some(super::SourceRef::FileView {
                alias: "f_warehouse".into()
            })
        );
        assert_eq!(
            super::source_ref("conn:C_mysql_9"),
            Some(super::SourceRef::Connection {
                conn_id: "C_mysql_9".into()
            }),
            "源库连接必须可展开（这一层是懒加载表的入口）"
        );
        assert_eq!(super::source_ref("conn:"), None, "空 id 不是连接");
        assert_eq!(super::source_ref("project:orders"), None);
        assert_eq!(super::source_ref("result:tmp_q_1"), None);
        assert_eq!(super::source_ref("accel:"), None, "空 id 不是源");
    }

    /// 「源库表」段：连接行零 I/O 进清单，能展开；叶子身份用 `|` 分段，
    /// 且反解必须还原（叶子的取样 / 选用全靠这一对函数）。
    #[test]
    fn source_connections_are_listed_and_table_keys_round_trip() {
        let rows = objects(&sample());
        let conn = rows
            .iter()
            .find(|o| o.key == "conn:C_mysql_9")
            .expect("已连接的连接要有一行");
        assert_eq!(conn.group, Group::Source);
        assert_eq!(conn.pill, "已挂载 · 项目级", "登记 + 挂上 = 项目级");
        assert_eq!(conn.channel, Channel::Source, "读法是连接下推");
        assert_eq!(conn.life, Life::External, "表在源库上，不在我们管的库里");
        assert!(conn.note.contains("MySQL"), "副标题要写驱动：{}", conn.note);

        // 注册了但没连上的源：照样进清单，写「未连接」、不给展开也不给选用
        let mut offline = sample();
        offline.connections.push(ConnRow {
            id: "C_dw_offline".into(),
            name: "离线数仓".into(),
            note: "PostgreSQL · 10.0.0.99".into(),
            connected: false,
            driver: "postgres".into(),
            database: Some("dw".into()),
            attach_capable: true,
            registered: true,
        });
        let rows = objects(&offline);
        let down = rows
            .iter()
            .find(|o| o.key == "conn:C_dw_offline")
            .expect("已注册的源即使没连上也要有一行");
        assert_eq!(down.group, Group::Source);
        assert!(!down.can_analyze(), "没连上不给动作");
        assert!(!down.can_pick(), "没连上的连接不进工作集");
        assert!(
            down.availability_text().contains("未连接"),
            "状态列要写清为什么：{}",
            down.availability_text()
        );

        // 叶子：`note` 写所在 catalog / schema，身份能反解回四段
        let leaf = super::source_table_object("C_mysql_9", "shop", "sales", "orders");
        assert_eq!(leaf.group, Group::Source);
        assert_eq!(leaf.pill, "库表");
        assert_eq!(leaf.name, "orders");
        assert_eq!(leaf.note, "shop.sales");
        assert_eq!(
            super::source_table_parts(&leaf.key),
            Some(("C_mysql_9", "shop", "sales", "orders"))
        );
        // 段里带冒号也不影响反解（分隔符是 `|` 的理由）
        let odd = super::source_table_object("C_1", "db:main", "", "v:tmp");
        assert_eq!(
            super::source_table_parts(&odd.key),
            Some(("C_1", "db:main", "", "v:tmp")),
            "带冒号的表名必须能还原：{}",
            odd.key
        );
        // 缺段 / 段数不对一律不认（宁可跳过，也不猜一个别的表）
        assert_eq!(super::source_table_parts("srctable:C_1|shop"), None);
        assert_eq!(super::source_table_parts("srctable:C_1|a|b|c|d"), None);
        assert_eq!(super::source_table_parts("project:orders"), None);
    }

    /// 状态列：**可用时写寿命**（`Life` 不是死字段），不可用时写原因。
    #[test]
    fn availability_column_shows_life_then_reason() {
        let rows = objects(&sample());
        for object in &rows {
            let text = object.availability_text();
            if object.can_analyze() {
                assert_eq!(text, object.life.label(), "{} 可用时该写寿命", object.key);
            } else {
                assert_eq!(
                    text,
                    object.state.label(),
                    "{} 不可用时该写原因",
                    object.key
                );
            }
        }
        // 四种寿命都要真出现过：文案里有，才算「参与渲染」
        for expected in [
            "外部",     // 源库连接 / 已加载文件
            "会话级",   // 临时表 / 联邦源 / 加速源
            "项目持久", // 分析库表
            "受管",     // 资产
        ] {
            assert!(
                rows.iter()
                    .any(|o| o.can_analyze() && o.life.label() == expected),
                "sample 里没有「{expected}」的行，这条断言失去意义"
            );
        }
    }

    /// 行上状态**短码**：再窄也不撑行；全文仍留在属性窗口。
    ///
    /// 这条用例钉的是「行上不放整句」这个决定本身——整句实测 209px（12px / 30 字），
    /// 面板 380px 宽时状态格只有 40px 下限位，整句会把状态块顶到行外 25px
    /// （`panels/data_objects_tab/layout_probe.rs` 的实测）。
    #[test]
    fn availability_chip_stays_short_on_the_row() {
        for object in &objects(&sample()) {
            let chip = object.availability_chip();
            assert!(
                chip.chars().count() <= 6,
                "{} 的短码超过 6 字：{chip}",
                object.key
            );
            if object.can_analyze() {
                assert_eq!(
                    chip,
                    object.availability_text(),
                    "{} 可用行的短码就是寿命，不需要退化",
                    object.key
                );
            }
        }

        // 实测那条最宽的整句：短码只取第一段
        let mut down = objects(&sample())
            .into_iter()
            .find(|o| !o.can_analyze())
            .expect("sample 里应有不可用行");
        down.state = State::Failed("未挂载 · 右键「挂载」后才能展开选表".to_string());
        assert_eq!(down.availability_chip(), "未挂载");
        assert!(
            down.availability_text().len() > down.availability_chip().len() * 4,
            "全文才是长的那份（属性窗口读它）"
        );

        // 没有分隔符的长原因：不猜哪一段是重点，直接退化成「失败」
        down.state = State::Failed("这一行背后到底怎么了暂时说不清".to_string());
        assert_eq!(down.availability_chip(), "失败");
    }

    /// 表型资产的本体表从分析库里消失 = 真失效（`State::Stale` 的生产路径）。
    #[test]
    fn table_assets_go_stale_when_their_backing_table_disappears() {
        let mut c = sample();
        c.analysis_tables.push("arp_客户画像_v2".into());
        c.assets.push(AssetRow {
            name: "客户画像".into(),
            version: 2,
            detail: String::new(),
            payload_rel_path: None,
            resource_id: "arp_2".into(),
            analytics_table: Some("arp_客户画像_v2".into()),
        });
        let rows = objects(&c);
        let asset = rows.iter().find(|o| o.key == "asset:arp_2").unwrap();
        assert!(asset.can_analyze(), "本体表在，资产就可用");

        // 表没了（被删 / 换了项目）：行留着、标失效、不给动作
        c.analysis_tables.retain(|name| name != "arp_客户画像_v2");
        let rows = objects(&c);
        let stale = rows.iter().find(|o| o.key == "asset:arp_2").unwrap();
        assert!(
            matches!(stale.state, State::Stale(_)),
            "本体表不在了要标失效：{:?}",
            stale.state
        );
        assert!(!stale.can_analyze());
        assert!(!stale.can_pick(), "失效行不能进工作集");
        assert!(
            stale.availability_text().contains("已失效"),
            "状态列要写出原因：{}",
            stale.availability_text()
        );

        // 没开项目时分析库清单必然为空、不是权威全集：不能拿它判「表不在」
        c.project_open = false;
        let rows = objects(&c);
        let asset = rows.iter().find(|o| o.key == "asset:arp_2").unwrap();
        assert!(asset.can_analyze(), "没项目就无从判断，不该误标失效");
    }

    /// 软上限：8 个不提示，第 9 个才提示；提示里不出现「不能」。
    #[test]
    fn soft_limit_warns_without_refusing() {
        assert!(super::picked_limit_note(8).is_none(), "8 个还算清");
        let note = super::picked_limit_note(9).expect("第 9 个要提示");
        assert!(note.contains('9'), "{note}");
        assert!(note.contains("8"), "要说清分界在哪：{note}");
        assert!(note.contains("仍可继续加"), "软上限不是硬拦：{note}");
    }

    /// 工作集是**多选**：再点同一个对象 = 移除；反复点不能重复计入。
    ///
    /// 单选实现（先 `clear()` 再 push）会让 `picked_limit_note` 永远拿不到 >8 的计数，
    /// 提示链整体变成死代码——这条测试就是钉住「计数真的会长」。
    #[test]
    fn picking_toggles_and_accumulates_across_objects() {
        let mut picked: Vec<String> = Vec::new();
        super::toggle_in_place(&mut picked, "project:orders");
        super::toggle_in_place(&mut picked, "global:orders");
        super::toggle_in_place(&mut picked, "result:tmp_q_1");
        assert_eq!(
            picked,
            vec!["project:orders", "global:orders", "result:tmp_q_1"],
            "按加入先后累积，不重排"
        );

        // 再点一次第二个 = 只移除它，其余保持原序
        super::toggle_in_place(&mut picked, "global:orders");
        assert_eq!(picked, vec!["project:orders", "result:tmp_q_1"]);

        // 攒到 9 个时提示必须真的出现（单选实现这里恒为 None）
        for index in 0..9 {
            super::toggle_in_place(&mut picked, &format!("project:t{index}"));
        }
        assert!(
            super::picked_limit_note(picked.len()).is_some(),
            "多选攒到 9 个要触发软上限提示：{}",
            picked.len()
        );
    }

    /// 会话行动作：可用的行给三个，失败的行不给「设为主源」。
    #[test]
    fn session_actions_offer_set_primary_only_for_ready_rows() {
        let ready = super::session_actions("session:编辑器 · a.sql:mysql_src", &State::Ready);
        assert_eq!(ready.len(), 3, "{ready:?}");
        assert!(matches!(ready[1], super::SessionAction::SetPrimary { .. }));
        // owner 里带冒号（文档名）时也要切得对：按最后一个冒号切
        match &ready[0] {
            super::SessionAction::RefreshSource { owner, alias } => {
                assert_eq!(owner, "编辑器 · a.sql");
                assert_eq!(alias, "mysql_src");
            }
            other => panic!("{other:?}"),
        }

        let failed =
            super::session_actions("session:doc:oracle_src", &State::Failed("ORA-12541".into()));
        assert_eq!(failed.len(), 2, "失败行不给设主源：{failed:?}");
        assert!(
            !failed
                .iter()
                .any(|a| matches!(a, super::SessionAction::SetPrimary { .. })),
            "{failed:?}"
        );

        // 非会话行的 key 不该产出动作（防止误把普通表当会话）
        assert!(super::session_actions("project:orders", &State::Ready).is_empty());
    }

    /// 资产带本体路径 + 给了资源目录时能直接读；缺路径时如实说转不了。
    #[test]
    fn assets_become_file_sources_only_when_their_payload_is_known() {
        let mut c = sample();
        let rows = objects(&c);

        // 有本体路径：交给 DuckDB 按扩展名选读取器（这里只断言它被收下了）
        let dir = std::env::temp_dir();
        let (sources, skipped) = super::analysis_sources(
            &rows,
            &["asset:arp_demo".into()],
            &c.assets,
            Some(dir.as_path()),
        );
        assert_eq!(sources.len(), 1, "有本体的资产要能分析：{skipped:?}");
        assert!(sources[0].conn_id.is_none(), "资产读在 DuckDB 上");

        // 没路径（引用型 / 旧行）：不猜，如实跳过
        c.assets[0].payload_rel_path = None;
        let (sources, skipped) = super::analysis_sources(
            &rows,
            &["asset:arp_demo".into()],
            &c.assets,
            Some(dir.as_path()),
        );
        assert!(sources.is_empty(), "{sources:?}");
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("资产"), "{skipped:?}");
    }
}

#[cfg(test)]
mod temp_source_tests {
    use super::{
        mounted_file_sources, objects, temp_source_for, unmounted_file_sources, Collected, ConnRow,
    };

    #[test]
    fn maps_prefixes_to_sources() {
        assert_eq!(
            temp_source_for("tmp_q_orders"),
            Some(engine::duckdb::temp_table::TempTableSource::Query)
        );
        assert_eq!(
            temp_source_for("tmp_i_eda"),
            Some(engine::duckdb::temp_table::TempTableSource::Insight)
        );
        assert_eq!(temp_source_for("res_orders_2026"), None);
    }

    /// 文件库源：本体路径进了 `loaded_files` 才算「挂上」；没挂的行不可用（面板据此不给展开），
    /// 判据看路径、不看连接状态——驱动连得上 ≠ 已经 ATTACH 进分析实例。
    #[test]
    fn file_sources_are_unmounted_until_their_path_is_loaded() {
        let mut c = Collected::empty();
        c.connections.push(ConnRow {
            id: "C_lite".into(),
            name: "销售.duckdb".into(),
            note: "duckdb".into(),
            connected: true,
            driver: "duckdb".into(),
            database: Some("D:/data/销售.duckdb".into()),
            attach_capable: true,
            registered: true,
        });
        let row = objects(&c)
            .into_iter()
            .find(|o| o.key == "conn:C_lite")
            .expect("文件库源要进源段");
        assert!(!row.can_analyze(), "没挂的文件库不可展开选表");
        assert!(
            row.state.label().contains("未挂载"),
            "{}",
            row.state.label()
        );
        assert_eq!(
            unmounted_file_sources(&c),
            vec![("conn:C_lite".to_string(), "D:/data/销售.duckdb".to_string())]
        );

        // 路径进了 loaded_files（斜杠 / 大小写不同也算同一个文件）→ 可用，且不在未挂清单
        c.loaded_files
            .push(("f_sales".into(), "d:\\data\\销售.duckdb".into()));
        let row = objects(&c)
            .into_iter()
            .find(|o| o.key == "conn:C_lite")
            .expect("文件库源要进源段");
        assert!(row.can_analyze(), "挂上之后要能展开");
        assert!(unmounted_file_sources(&c).is_empty());
        // 反过来取：已挂清单里正好是这一行——卸载/重挂的菜单靠它出现，
        // 两边判据互斥（同一行不该既算「未挂」又算「已挂」）
        assert_eq!(
            mounted_file_sources(&c),
            vec![("conn:C_lite".to_string(), "D:/data/销售.duckdb".to_string())]
        );

        // 网络库没有本体路径：不进「未挂」清单，但**挂没挂改看加速会话**（第 4 / 37 条）
        c.loaded_files.clear();
        c.connections[0].driver = "mysql".into();
        c.connections[0].database = Some("orders".into());
        assert!(unmounted_file_sources(&c).is_empty());
        let row = objects(&c)
            .into_iter()
            .find(|o| o.key == "conn:C_lite")
            .expect("网络源也要进源段");
        assert!(
            !row.can_analyze(),
            "没有加速会话 = 没挂进分析实例：行首不给展开指示"
        );
        assert!(
            row.state.label().contains("未挂载"),
            "{}",
            row.state.label()
        );
        assert!(
            row.state.label().contains("挂载"),
            "状态字要指出去哪挂（右键「挂载」，加速已不进面板）：{}",
            row.state.label()
        );

        // 加速会话起来了（`accel_sources.owner` 就是 conn_id）：**也不算挂载事实**
        // （2026-11-25 口径：加速 = 一次一个库的 ATTACH，不进面板），所以这一行仍不可展开。
        // 能展开的是「登记 + 活连接」那条路（`expand_via`），不是加速。
        c.accel_sources.push(super::SessionSource {
            owner: "C_lite".into(),
            alias: "商城库".into(),
            note: "MySQL · 本地加速会话".into(),
            failed: None,
        });
        let row = objects(&c)
            .into_iter()
            .find(|o| o.key == "conn:C_lite")
            .expect("网络源也要进源段");
        assert!(
            !row.can_analyze(),
            "加速会话不构成展开依据：面板里这条连接仍然没有自己的表可列"
        );
    }
}

#[cfg(test)]
mod memory_format_tests {
    use super::format_bytes;

    #[test]
    fn formats_bytes_by_magnitude() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.0 GB");
    }
}