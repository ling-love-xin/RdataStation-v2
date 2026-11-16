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

use std::path::PathBuf;

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

/// 分组**按 DuckDB 容器**排（2026-11-16 定稿）：面板回答的是「哪些对象已经在
/// 可以分析的位置上」，不是「业务上算什么」。资产 / 项目 / 全局 / 结果集这些
/// 业务身份降为行内标签，不再各占一段——同一个容器里的表回答同一件事。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// 联邦会话里已挂上的源（ATTACH 进来的库）
    MountedSources,
    /// 本项目分析库（`<项目>/.RSmeta/analytics.duckdb`）：落库的 `res_`、项目自建表、
    /// 落在项目库里的资产，都是这一个 duckdb 文件里的对象
    ProjectLibrary,
    /// 全局分析库（`<RDS_HOME>/data/system/analytics.duckdb`）
    GlobalLibrary,
    /// 外部文件（草稿箱 / 导入的文件；能加载进 DuckDB 的才在这里）
    ExternalFiles,
    /// 临时表（进程内存单例里的 `tmp_q_`）
    TemporaryTables,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Self::MountedSources => "已挂载的源",
            Self::ProjectLibrary => "本项目分析库",
            Self::GlobalLibrary => "全局分析库",
            Self::ExternalFiles => "外部文件",
            Self::TemporaryTables => "临时表",
        }
    }

    /// 段落顺序由枚举自身给出（`Ord` 派生自上到下），避免视图里再排一次。
    pub fn order(self) -> u8 {
        match self {
            Self::MountedSources => 0,
            Self::ProjectLibrary => 1,
            Self::GlobalLibrary => 2,
            Self::ExternalFiles => 3,
            Self::TemporaryTables => 4,
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
}

impl Group {
    /// 行首图标：**段决定形状**——扫一眼就知道这是源表、分析库表还是临时表，
    /// 不用去读通道文案（原型 07 的 14px 图标列）
    pub fn icon(self) -> gpui_kit::assets::IconName {
        use gpui_kit::assets::IconName;
        match self {
            Self::MountedSources => IconName::Database,
            Self::ProjectLibrary => IconName::Table,
            Self::GlobalLibrary => IconName::Layers,
            Self::ExternalFiles => IconName::FileSpreadsheet,
            Self::TemporaryTables => IconName::Clock,
        }
    }
}

impl Object {
    /// 升序排序键：先分组、再名字（同名按身份兜底，保证稳定）。
    pub fn sort_key(&self) -> (u8, String, String) {
        (self.group.order(), self.name.clone(), self.key.clone())
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
/// 连接不在这里列：连接的表要逐层 introspect，属于面板自己的懒加载；
/// 这里只带**连接数量**给分组标题用。
pub struct Collected {
    pub conn_count: usize,
    pub drafts: Vec<(String, PathBuf)>,
    pub analysis_tables: Vec<String>,
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
}

impl Collected {
    /// 只填连接数的小构造（测试与「只想知道有几条连接」的调用方用）。
    pub fn empty() -> Self {
        Self {
            conn_count: 0,
            drafts: Vec::new(),
            analysis_tables: Vec::new(),
            global_tables: Vec::new(),
            result_sets: Vec::new(),
            insight_samples: Vec::new(),
            loaded_files: Vec::new(),
            assets: Vec::new(),
            session_sources: Vec::new(),
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
            group: Group::MountedSources,
            pill: "源",
            version: None,
        });
    }

    // 分析库里的两类表必须分开：`res_` 是「结果集落库」留下的，与项目自建表
    // 在用户心里不是一回事（前者可删、后者可能是他自己建的）
    // 注意返回顺序是 (普通表, 持久表)，写反会让两段整体错位
    let (own, persistent) = split_persistent_tables(&collected.analysis_tables);
    for name in persistent {
        out.push(Object {
            key: format!("persistent:{name}"),
            name: persistent_table_label(&name),
            note: "可删除释放空间".to_string(),
            channel: Channel::Local,
            life: Life::Project,
            state: State::Ready,
            group: Group::ProjectLibrary,
            pill: "结果集落库",
            version: None,
        });
    }
    for name in own {
        out.push(Object {
            key: format!("project:{name}"),
            name,
            note: "项目自建".to_string(),
            channel: Channel::Local,
            life: Life::Project,
            state: State::Ready,
            group: Group::ProjectLibrary,
            pill: "项目自建",
            version: None,
        });
    }

    for asset in &collected.assets {
        out.push(Object {
            key: format!("asset:{}", asset.name),
            name: asset.name.clone(),
            // 只读 + 最新版本：两件事都写在副标题，避免用户以为能选旧版
            note: match asset.detail.is_empty() {
                true => format!("只读 · 最新 v{}", asset.version),
                false => format!("只读 · 最新 v{} · {}", asset.version, asset.detail),
            },
            channel: Channel::Asset,
            life: Life::Managed,
            state: State::Ready,
            group: Group::ProjectLibrary,
            pill: "资产",
            version: Some(asset.version),
        });
    }

    for name in &collected.global_tables {
        out.push(Object {
            key: format!("global:{name}"),
            name: name.clone(),
            note: "跨项目 · 只读".to_string(),
            channel: Channel::Local,
            life: Life::Project,
            state: State::Ready,
            group: Group::GlobalLibrary,
            pill: "全局",
            version: None,
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
            group: Group::TemporaryTables,
            pill: "结果集",
            version: None,
        });
    }

    // 洞察产物（`tmp_i_`）**不进面板**：它是抽取的样，不是全量；生命周期属于洞察
    // 会话。要留就由用户在洞察里「保留到项目库」，落成持久表后再从这里看。

    for (rel, abs) in &collected.drafts {
        out.push(Object {
            key: format!("draft:{rel}"),
            name: rel.clone(),
            note: abs.to_string_lossy().to_string(),
            channel: Channel::File,
            life: Life::Project,
            state: State::Ready,
            group: Group::ExternalFiles,
            pill: "草稿",
            version: None,
        });
    }

    // 已加载进进程单例的文件视图：视图名就是 SQL 里能直接用的那个（`f_<ascii>`），
    // 文件名放 note，用户才知道它对应哪份文件
    for (alias, path) in &collected.loaded_files {
        out.push(Object {
            key: format!("fileview:{alias}"),
            name: alias.clone(),
            note: path.clone(),
            channel: Channel::File,
            life: Life::External,
            state: State::Ready,
            group: Group::ExternalFiles,
            pill: "已加载",
            version: None,
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

/// 已选对象 → 洞察取样来源（面板「分析已选」的取数口径，**纯函数**）。
///
/// 返回 `(来源, 跳过原因)`：能给出**完整取数描述**的对象才转成来源，
/// 转不了的老实说为什么——「点了没反应」比「说清楚不支持」糟得多。
///
/// 目前能转的：项目分析库表 / 全局分析库表 / 结果集 / 洞察产物。
/// 转不了的：资产（面板只知道名字与版本，不知道本体是表还是文件）、
/// 文件（草稿行有路径但这里没按扩展名选读取器）、联邦源（别名下是哪张表要等会话快照）。
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
            // 资产：本体是 `resources/` 下的受管文件，读它 = 按扩展名交给 DuckDB 读
            Some(("asset", name)) => {
                let row = assets.iter().find(|a| a.name == *name);
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

/// 这一行能不能「存到项目分析库」。
///
/// 只有**进程单例里已经是表**的两组可以：结果集（`tmp_q_`）与洞察产物（`tmp_i_`）。
/// 落库就是整表搬一份，不需要重建；源库表 / 文件 / 资产各有各的路（连接下推 / reader
/// 函数 / 只读受管），别把它们塞进这条通道——那会给「只读资产」写出一份副本，
/// 用户再也分不清哪份是本体。
pub fn can_persist(object: &Object) -> bool {
    // 临时表才谈得上「存为表」：项目库/全局库里的对象本来就已经持久了
    object.group == Group::TemporaryTables && object.can_analyze()
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
/// 资产清单暂由调用方填：它要过资产模块的登记层（本项目 `analytics_resources`），
/// 那套入口还没从这里可达，缺的就是空——不猜一个查询塞进去。
pub fn collect(shared: &Shared) -> Collected {
    let conn_count = shared
        .connections
        .borrow()
        .iter()
        .filter(|c| c.connected)
        .count();
    let drafts = super::source_picker::collect_drafts(shared);
    let project = shared.project.borrow().clone();
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

    Collected {
        conn_count,
        drafts,
        analysis_tables,
        global_tables,
        result_sets,
        insight_samples,
        loaded_files: shared.loaded_files.borrow().clone(),
        assets: Vec::new(),
        session_sources,
    }
}

#[cfg(test)]
mod tests {
    // 刻意不用 `use super::*`：会把 gpui 的 `test` 属性宏带进作用域。
    use super::{
        AssetRow, Channel, Collected, Group, Life, Object, SessionSource, State,
        objects,
    };
    use std::path::PathBuf;

    fn sample() -> Collected {
        Collected {
            conn_count: 14,
            drafts: vec![("数据/a.parquet".into(), PathBuf::from("C:/p/a.parquet"))],
            analysis_tables: vec![
                "res_订单明细_20261115103000".into(),
                "数据修复".into(),
            ],
            global_tables: vec!["G_月度经营".into()],
            result_sets: vec!["tmp_q_结果_3".into()],
            insight_samples: vec!["tmp_i_orders_sample".into()],
            loaded_files: Vec::new(),
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
        }
    }

    /// 分组顺序是「越靠上越正在用」：会话在最前，文件在最后。
    #[test]
    fn groups_follow_the_declared_order() {
        let rows = objects(&sample());
        let orders: Vec<u8> = rows.iter().map(|o| o.group.order()).collect();
        let mut sorted = orders.clone();
        sorted.sort();
        assert_eq!(orders, sorted, "行必须按分组顺序连续排列");
        assert_eq!(rows.first().map(|o| o.group), Some(Group::MountedSources));
        // 临时表排最后：它寿命最短，也最该被处理掉
        assert_eq!(rows.last().map(|o| o.group), Some(Group::TemporaryTables));
    }

    /// `res_` 表单独成段且换成可读名；项目自建表留在「项目分析库」段、名字原样。
    #[test]
    fn persistent_tables_are_split_from_project_owned_ones() {
        let rows = objects(&sample());
        let persistent = rows
            .iter()
            .find(|o| o.key.starts_with("persistent:"))
            .unwrap();
        assert_eq!(persistent.name, "订单明细 · 2026-11-15 10:30:00");
        // 身份仍是原名：面板上改了显示，删除 / 分析仍按原名定位
        assert_eq!(persistent.key, "persistent:res_订单明细_20261115103000");

        // 分组已经不按「落库 / 自建」分（都在本项目分析库里），行内标签负责说清是哪一类
        let own = rows
            .iter()
            .find(|o| o.key.starts_with("project:"))
            .unwrap();
        assert_eq!(own.name, "数据修复");
        assert_eq!(own.pill, "项目自建");
        assert_eq!(persistent.pill, "结果集落库");
    }

    /// 资产行必须标出「最新版本」且只读，并且不给版本选择（模型层没有这个字段）。
    #[test]
    fn asset_rows_carry_latest_version_and_are_read_only() {
        let rows = objects(&sample());
        let asset = rows.iter().find(|o| o.pill == "资产").unwrap();
        assert_eq!(asset.group, Group::ProjectLibrary, "资产的本体在项目分析库里");
        assert_eq!(asset.version, Some(3));
        assert!(asset.note.contains("只读"));
        assert_eq!(asset.channel, Channel::Asset);
        assert_eq!(asset.life, Life::Managed);
        // 资产可以参与分析
        assert!(asset.can_analyze());
    }

    /// 临时对象是会话级：失效时仍留在清单里，但不再给「分析」动作。
    #[test]
    fn session_objects_are_session_lived_and_stale_rows_are_not_analyzable() {
        let rows = objects(&sample());
        let result = rows.iter().find(|o| o.group == Group::TemporaryTables).unwrap();
        assert_eq!(result.life, Life::Session);

        let stale = Object {
            state: State::Stale("会话已重建"),
            ..result.clone()
        };
        assert!(!stale.can_analyze(), "失效行不该还能点分析");
        assert!(stale.state.label().contains("会话已重建"));
    }

    /// 联邦源挂载失败：留在最上面的会话段，并把原因写在副标题。
    #[test]
    fn failed_session_sources_stay_in_the_session_group_with_reason() {
        let rows = objects(&sample());
        let failed = rows
            .iter()
            .find(|o| matches!(o.state, State::Failed(_)))
            .unwrap();
        assert_eq!(failed.group, Group::MountedSources);
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
        let session_rows: Vec<&Object> =
            rows.iter().filter(|o| o.group == Group::MountedSources).collect();
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
        assert!(keys.contains(&"asset:orders"));
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
            "asset:月度经营快照".to_string(),
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

    /// 只有「进程单例里已经是表」的两组能走落库通道；给人看的名字要去掉临时前缀。
    #[test]
    fn only_session_tables_can_be_persisted_and_labels_drop_the_temp_prefix() {
        let rows = objects(&sample());
        for object in &rows {
            let expected = object.group == Group::TemporaryTables;
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

    /// 软上限：8 个不提示，第 9 个才提示；提示里不出现「不能」。
    #[test]
    fn soft_limit_warns_without_refusing() {
        assert!(super::picked_limit_note(8).is_none(), "8 个还算清");
        let note = super::picked_limit_note(9).expect("第 9 个要提示");
        assert!(note.contains('9'), "{note}");
        assert!(note.contains("8"), "要说清分界在哪：{note}");
        assert!(note.contains("仍可继续加"), "软上限不是硬拦：{note}");
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
            &["asset:月度经营快照".into()],
            &c.assets,
            Some(dir.as_path()),
        );
        assert_eq!(sources.len(), 1, "有本体的资产要能分析：{skipped:?}");
        assert!(sources[0].conn_id.is_none(), "资产读在 DuckDB 上");

        // 没路径（引用型 / 旧行）：不猜，如实跳过
        c.assets[0].payload_rel_path = None;
        let (sources, skipped) =
            super::analysis_sources(&rows, &["asset:月度经营快照".into()], &c.assets, Some(dir.as_path()));
        assert!(sources.is_empty(), "{sources:?}");
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("资产"), "{skipped:?}");
    }
}

#[cfg(test)]
mod temp_source_tests {
    use super::temp_source_for;

    #[test]
    fn maps_prefixes_to_sources() {
        assert_eq!(temp_source_for("tmp_q_orders"), Some(engine::duckdb::temp_table::TempTableSource::Query));
        assert_eq!(temp_source_for("tmp_i_eda"), Some(engine::duckdb::temp_table::TempTableSource::Insight));
        assert_eq!(temp_source_for("res_orders_2026"), None);
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

