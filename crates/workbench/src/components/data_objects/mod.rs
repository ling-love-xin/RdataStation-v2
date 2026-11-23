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

use crate::panels::Shared;

use super::source_picker::{
    analysis_tables_of, split_persistent_tables, persistent_table_label,
};

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
                Some("accel") => 1,
                Some("fileview") => 2,
                // 源库连接（`conn:`）：也没挂任何东西，排在已挂载的后面
                _ => 3,
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
            insight_samples: Vec::new(),
            loaded_files: Vec::new(),
            assets: Vec::new(),
            session_sources: Vec::new(),
            accel_sources: Vec::new(),
        }
    }
}

/// 原始清单 → 行模型（纯函数：不读盘、不查库，便于钉住身份与分组规则）。
pub fn objects(collected: &Collected) -> Vec<Object> {
    let mut out = Vec::new();

    for source in &collected.session_sources {
        out.push(Object {
            // 身份带 owner：同名别名可能在多个会话里同时存在
            key: format!("session:{}:{}", source.owner, source.alias),
            name: source.alias.clone(),
            note: match &source.failed {
                Some(reason) => reason.clone(),
                None => format!("{} · {}", source.owner, source.note),
            },
            channel: Channel::Federated,
            life: Life::Session,
            state: match &source.failed {
                Some(reason) => State::Failed(reason.clone()),
                None => State::Ready,
            },
            group: Group::Source,
            pill: "联邦源",
            version: None,
            // 联邦源的表引用是 `<别名>.<schema>.<表>`，只给别名不算引用名
            sql_ref: None,
        });
    }

    // 本地加速源：与联邦源同段（都是"挂进 DuckDB 的源"），pill 区分。
    // 会话存在即挂载成功（加速档建会话失败不留会话），所以这里没有失败态。
    for source in &collected.accel_sources {
        out.push(Object {
            key: format!("accel:{}", source.owner),
            name: source.alias.clone(),
            note: format!("{} · 引用需切到本地加速档", source.note),
            channel: Channel::Accel,
            life: Life::Session,
            state: State::Ready,
            group: Group::Source,
            pill: "加速源",
            version: None,
            // 加速档的表要切到该档才能引用，行上不给可直接抄的名字
            sql_ref: None,
        });
    }

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
    let unmounted: Vec<(String, String)> = unmounted_file_sources(collected);
    for conn in &collected.connections {
        let connected = conn.connected;
        let key = format!("conn:{}", conn.id);
        let file_path = file_source_path(conn);
        let not_mounted = unmounted
            .iter()
            .find(|(row_key, _)| row_key == &key)
            .map(|(_, path)| path.clone());
        let note = match (&file_path, &not_mounted) {
            // 文件库把本体路径写在副标题上：用户要确认"挂的是哪一个文件"
            (Some(path), _) => format!("文件库 · {path}"),
            _ => conn.note.clone(),
        };
        let state = if !connected {
            State::Failed("未连接 · 连上后可展开选表".to_string())
        } else if not_mounted.is_some() {
            State::Failed("未挂载 · 右键挂载后可展开选表".to_string())
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
            pill: "源连接",
            version: None,
            // 连接本身不是数据对象：它下面展开的表才拼得出引用名
            sql_ref: None,
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
            pill: if is_database { "文件库" } else { "已加载" },
            version: None,
            // 文件视图是 `f_*` 裸名；库文件是 catalog，引用要 `别名.schema.表`，不在这里编
            sql_ref: (!is_database).then(|| sql_ident(alias)),
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
    ]
    .into_iter()
    .find(|source| source.prefixes().iter().any(|prefix| name.starts_with(prefix)))
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
/// 三类源行可展开：联邦源（`session:{owner}:{alias}`）、本地加速源（`accel:{conn_id}`）、
/// 源库连接（`conn:{conn_id}`）；其它 key 一律 `None`——表 / 文件 / 资产行没有下一层。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    /// 联邦会话里的源（owner = 会话主人，alias = 挂载别名）
    Federated { owner: String, alias: String },
    /// 本地加速会话（conn_id = 源连接 id）
    Accel { conn_id: String },
    /// ATTACH 进进程单例的数据库文件（alias = 挂载别名）
    FileView { alias: String },
    /// **还没进 DuckDB** 的源库连接：展开要逐层 introspect（catalog → schema → 表）
    Connection { conn_id: String },
}

/// 从行 key 解出 [`SourceRef`]（与 `session_actions` 同一套 `rsplit_once` 口径：
/// owner 里可能带冒号——文档名，按**最后一个**冒号切）。
pub fn source_ref(key: &str) -> Option<SourceRef> {
    if let Some(conn_id) = key.strip_prefix("accel:") {
        return (!conn_id.is_empty()).then(|| SourceRef::Accel {
            conn_id: conn_id.to_string(),
        });
    }
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
    }
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
            Some(("fileview", alias)) if object.group == Group::File => {
                Some(SampleSource::on_duckdb(
                    format!("SELECT * FROM \"{alias}\""),
                    object.name.clone(),
                ))
            }
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
            None => skipped.push(format!("{}（{}）", object.name, object.pill)),
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
            &conn.driver, database,
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
    collected
        .connections
        .iter()
        .filter_map(|conn| {
            let path = file_source_path(conn)?;
            let mounted = collected
                .loaded_files
                .iter()
                .any(|(_, seen)| same_file(seen, &path));
            (!mounted).then(|| (format!("conn:{}", conn.id), path))
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
/// 与 [`can_persist`] **同一口径**（`result:` 前缀）：面板对临时对象的“进 / 出”两个
/// 动作必须对称——落库只收结果集，清理就也只能删结果集，谁单独放宽都会留下
/// “能进不能出”或“能删不该删”的对象。
pub fn can_clear(object: &Object) -> bool {
    object.key.starts_with("result:")
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
    // 源段只收**已注册成分析源**的连接（`use_duckdb_fed`）：没注册的是导航栏的对象，
    // 不是分析源（铁律 B 组第 4 条）。注册过但没连上的仍进清单 —— 行上写「未连接」，
    // 不给展开（没有表可列）。
    // 这里零 I/O（连接清单是宿主快照），表清单交给展开时懒加载
    let connections: Vec<ConnRow> = shared
        .connections
        .borrow()
        .iter()
        .filter(|item| item.use_duckdb_fed)
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
        })
        .collect();
    let project = shared.project.borrow().clone();
    let project_open = project.is_some();
    let analysis_tables = project
        .as_ref()
        .map(|p| {
            analysis_tables_of(
                &engine::persistence::project_db::ProjectDatabaseManager::analysis_db_path(&p.root),
            )
        })
        .unwrap_or_default();
    let global_tables = engine::analysis::global::global_tables("C_data_objects").unwrap_or_default();
    let result_sets = engine::analysis::result_set::list_result_sets().unwrap_or_default();
    let insight_samples = engine::analysis::result_set::list_insight_samples().unwrap_or_default();
    // 联邦会话：一次拿全（面板不知道 owner 是谁，`owners()` 正是为这种调用准备的）
    let session_sources = engine::duckdb::federation::session::owners()
        .into_iter()
        .flat_map(|(owner, snapshot)| {
            let primary = snapshot.primary.clone();
            snapshot.sources.into_iter().map(move |mounted| {
                let alias = mounted.alias().to_string();
                let (note, failed) = match &mounted.state {
                    engine::duckdb::federation::registry::MountState::Ready { tables } => (
                        format!("{} 张表 · {}", tables, if primary.as_deref() == Some(alias.as_str()) { "主源" } else { "已挂载" }),
                        None,
                    ),
                    engine::duckdb::federation::registry::MountState::Failed(reason) => {
                        (String::new(), Some(reason.clone()))
                    }
                };
                SessionSource {
                    owner: owner.clone(),
                    alias,
                    note,
                    failed,
                }
            })
        })
        .collect();

    // 本地加速会话：引擎按源连接登记，这里把连接 id 翻译成用户认得的名字
    // （连接清单是宿主快照；找不到就用 id 兜底——比显示空白强，且可定位）
    let accel_sources = {
        let connections = shared.connections.borrow();
        engine::duckdb::accel::owners()
            .into_iter()
            .map(|(conn_id, kind)| {
                let alias = connections
                    .iter()
                    .find(|item| item.id == conn_id)
                    .map(|item| item.name.clone())
                    .unwrap_or_else(|| conn_id.clone());
                SessionSource {
                    owner: conn_id,
                    alias,
                    note: format!("{} · 本地加速会话", kind.label()),
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
        insight_samples,
        loaded_files: shared.loaded_files.borrow().clone(),
        assets: Vec::new(),
        session_sources,
        accel_sources,
    }
}

#[cfg(test)]
mod tests {
    // 刻意不用 `use super::*`：会把 gpui 的 `test` 属性宏带进作用域。
    use super::{
        AssetRow, Channel, Collected, ConnRow, Group, Life, Object, SessionSource, State,
        can_archive_snapshot, fileview_alias, objects,
    };

    fn sample() -> Collected {
        Collected {
            connections: vec![ConnRow {
                id: "C_mysql_9".into(),
                name: "订单库".into(),
                note: "MySQL · 10.0.0.8".into(),
                connected: true,
                driver: "mysql".into(),
                database: Some("orders".into()),
            }],
            analysis_tables: vec![
                "res_订单明细_20261115103000".into(),
                "数据修复".into(),
            ],
            // sample 代表「项目开着」的常态：`analysis_tables` 是权威全集
            project_open: true,
            global_tables: vec!["G_月度经营".into()],
            result_sets: vec!["tmp_q_结果_3".into()],
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
                alias: "订单库".into(),
                note: "MySQL · 本地加速会话".into(),
                failed: None,
            }],
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
        assert_eq!(
            ranks_of(Group::Table),
            table,
            "表段内次序要被 rank 排好"
        );
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
            assert!(pair[0] < pair[1], "{} 与 {} 的顺序与声明相反", pair[0].title(), pair[1].title());
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
        let own = rows
            .iter()
            .find(|o| o.key.starts_with("project:"))
            .unwrap();
        assert_eq!(own.group, Group::Table);
        let global = rows
            .iter()
            .find(|o| o.key.starts_with("global:"))
            .unwrap();
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
        assert!(asset.note.contains("快照"), "有副本的文件资产标快照：{}", asset.note);
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
        assert!(reference.note.contains("按需"), "引用型要标按需：{}", reference.note);
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
        for key in [
            "fileview:f_orders",
            "asset:arp_2",
            "asset:arp_demo",
        ] {
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
        assert_eq!(failed.pill, "联邦源");
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
        let session_rows: Vec<&Object> =
            rows.iter().filter(|o| o.key.starts_with("session:")).collect();
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
    fn persist_and_clear_share_the_same_prefix_rule() {
        let rows = objects(&sample());
        for object in &rows {
            assert_eq!(
                super::can_persist(object),
                super::can_clear(object) && object.can_analyze(),
                "{} 进出两个动作的口径要一致",
                object.key
            );
        }
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
        assert_eq!(db_file.pill, "文件库");
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
        let picked = vec!["fileview:f_sales".to_string(), "fileview:f_warehouse".to_string()];
        let (sources, skipped) = super::analysis_sources(&rows, &picked, &sample().assets, None);
        assert_eq!(sources.len(), 1, "{sources:?}");
        assert!(sources[0].sql.contains("f_sales"), "{sources:?}");
        assert_eq!(skipped.len(), 1, "库文件整库取样不成立：{skipped:?}");
        assert!(skipped[0].contains("文件库"), "{skipped:?}");
    }

    /// 本地加速源进源段：pill 区分「加速源」，副标题把「要切通道」说清。
    #[test]
    fn accel_sources_join_the_source_group_with_their_own_pill() {
        let rows = objects(&sample());
        let accel = rows
            .iter()
            .find(|o| o.key == "accel:C_mysql_9")
            .expect("加速源要有行");
        assert_eq!(accel.group, Group::Source);
        assert_eq!(accel.pill, "加速源");
        assert_eq!(accel.channel, Channel::Accel);
        assert_eq!(accel.life, Life::Session);
        assert!(accel.can_analyze(), "会话在就是挂上了");
        assert!(
            accel.note.contains("本地加速档"),
            "要提示引用需切通道：{}",
            accel.note
        );
        // 显示名是连接名（用户认名字，不认 conn_id）
        assert_eq!(accel.name, "订单库");
    }

    /// 源行 key → 展开目标：联邦源按最后一个冒号切（owner 可能带冒号），加速源直取；
    /// 其它行没有下一层。
    #[test]
    fn source_ref_parses_both_source_kinds_and_rejects_flat_rows() {
        assert_eq!(
            super::source_ref("session:编辑器 · a.sql:mysql_src"),
            Some(super::SourceRef::Federated {
                owner: "编辑器 · a.sql".into(),
                alias: "mysql_src".into(),
            })
        );
        assert_eq!(
            super::source_ref("accel:C_mysql_9"),
            Some(super::SourceRef::Accel {
                conn_id: "C_mysql_9".into()
            })
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
        assert_eq!(conn.pill, "源连接");
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
                assert_eq!(text, object.state.label(), "{} 不可用时该写原因", object.key);
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
                rows.iter().any(|o| o.can_analyze() && o.life.label() == expected),
                "sample 里没有「{expected}」的行，这条断言失去意义"
            );
        }
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

        let failed = super::session_actions(
            "session:doc:oracle_src",
            &State::Failed("ORA-12541".into()),
        );
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
        let (sources, skipped) =
            super::analysis_sources(&rows, &["asset:arp_demo".into()], &c.assets, Some(dir.as_path()));
        assert!(sources.is_empty(), "{sources:?}");
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("资产"), "{skipped:?}");
    }
}

#[cfg(test)]
mod temp_source_tests {
    use super::{Collected, ConnRow, objects, temp_source_for, unmounted_file_sources};

    #[test]
    fn maps_prefixes_to_sources() {
        assert_eq!(temp_source_for("tmp_q_orders"), Some(engine::duckdb::temp_table::TempTableSource::Query));
        assert_eq!(temp_source_for("tmp_i_eda"), Some(engine::duckdb::temp_table::TempTableSource::Insight));
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
        });
        let row = objects(&c)
            .into_iter()
            .find(|o| o.key == "conn:C_lite")
            .expect("文件库源要进源段");
        assert!(!row.can_analyze(), "没挂的文件库不可展开选表");
        assert!(row.state.label().contains("未挂载"), "{}", row.state.label());
        assert_eq!(
            unmounted_file_sources(&c),
            vec![(
                "conn:C_lite".to_string(),
                "D:/data/销售.duckdb".to_string()
            )]
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

        // 网络库没有本体路径：不进「未挂」清单，也不该被标未挂
        c.loaded_files.clear();
        c.connections[0].driver = "mysql".into();
        c.connections[0].database = Some("orders".into());
        assert!(unmounted_file_sources(&c).is_empty());
        let row = objects(&c)
            .into_iter()
            .find(|o| o.key == "conn:C_lite")
            .expect("网络源也要进源段");
        assert!(row.can_analyze());
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

