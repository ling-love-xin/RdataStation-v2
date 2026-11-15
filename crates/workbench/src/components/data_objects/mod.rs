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

/// 分组：面板里的段落顺序（越靠上越「正在用」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// 联邦会话里已挂上的源
    Session,
    /// 项目分析库里本次落库留下的持久表（`res_`）
    Persistent,
    /// 项目分析库里其它表
    ProjectTables,
    /// 资产（只读，最新版本）
    Assets,
    /// 全局分析表（跨项目）
    GlobalTables,
    /// 结果集（`tmp_q_`）
    ResultSets,
    /// 洞察产物（`tmp_i_`）
    InsightProducts,
    /// 草稿箱与外部文件
    Files,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Self::Session => "本次会话",
            Self::Persistent => "持久表（结果集落库）",
            Self::ProjectTables => "项目分析库",
            Self::Assets => "资产（最新版本）",
            Self::GlobalTables => "全局分析表",
            Self::ResultSets => "结果集",
            Self::InsightProducts => "洞察产物",
            Self::Files => "文件",
        }
    }

    /// 段落顺序由枚举自身给出（`Ord` 派生自上到下），避免视图里再排一次。
    pub fn order(self) -> u8 {
        match self {
            Self::Session => 0,
            Self::Persistent => 1,
            Self::ProjectTables => 2,
            Self::Assets => 3,
            Self::GlobalTables => 4,
            Self::ResultSets => 5,
            Self::InsightProducts => 6,
            Self::Files => 7,
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
    /// 资产的最新版本（其它类型为 None）
    pub version: Option<u32>,
}

impl Object {
    /// 升序排序键：先分组、再名字（同名按身份兜底，保证稳定）。
    pub fn sort_key(&self) -> (u8, String, String) {
        (self.group.order(), self.name.clone(), self.key.clone())
    }

    /// 能不能直接「分析」——失效与失败的行不给动作，只给「为什么」。
    pub fn can_analyze(&self) -> bool {
        self.state.is_usable()
    }
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
    /// 资产 `(名字, 最新版本)`；空则说明还没接上资产清单
    pub assets: Vec<(String, u32)>,
    /// 联邦会话里已挂上的源 `(名字, 状态描述)`
    pub session_sources: Vec<(String, Option<String>)>,
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
            assets: Vec::new(),
            session_sources: Vec::new(),
        }
    }
}

/// 原始清单 → 行模型（纯函数：不读盘、不查库，便于钉住身份与分组规则）。
pub fn objects(collected: &Collected) -> Vec<Object> {
    let mut out = Vec::new();

    for (name, why) in &collected.session_sources {
        out.push(Object {
            key: format!("session:{name}"),
            name: name.clone(),
            note: why.clone().unwrap_or_else(|| "已挂载".to_string()),
            channel: Channel::Federated,
            life: Life::Session,
            state: match why {
                Some(why) => State::Failed(why.clone()),
                None => State::Ready,
            },
            group: Group::Session,
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
            group: Group::Persistent,
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
            group: Group::ProjectTables,
            version: None,
        });
    }

    for (name, version) in &collected.assets {
        out.push(Object {
            key: format!("asset:{name}"),
            name: name.clone(),
            // 只读 + 最新版本：两件事都写在副标题，避免用户以为能选旧版
            note: format!("只读 · 最新 v{version}"),
            channel: Channel::Asset,
            life: Life::Managed,
            state: State::Ready,
            group: Group::Assets,
            version: Some(*version),
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
            group: Group::GlobalTables,
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
            group: Group::ResultSets,
            version: None,
        });
    }

    for name in &collected.insight_samples {
        out.push(Object {
            key: format!("insight:{name}"),
            name: name.clone(),
            note: "洞察产物 · 可再分析".to_string(),
            channel: Channel::Process,
            life: Life::Session,
            state: State::Ready,
            group: Group::InsightProducts,
            version: None,
        });
    }

    for (rel, abs) in &collected.drafts {
        out.push(Object {
            key: format!("draft:{rel}"),
            name: rel.clone(),
            note: abs.to_string_lossy().to_string(),
            channel: Channel::File,
            life: Life::Project,
            state: State::Ready,
            group: Group::Files,
            version: None,
        });
    }

    out.sort_by_key(|o| o.sort_key());
    out
}

/// 现取一次（面板打开 / 刷新时调）。
///
/// 资产清单与联邦会话源暂由调用方填（两处都要先拿到对应模块的清单入口），
/// 这里先给出其余全部来源；不猜没有的 API，缺的就是空。
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

    Collected {
        conn_count,
        drafts,
        analysis_tables,
        global_tables,
        result_sets,
        insight_samples,
        assets: Vec::new(),
        session_sources: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    // 刻意不用 `use super::*`：会把 gpui 的 `test` 属性宏带进作用域。
    use super::{objects, Channel, Collected, Group, Life, Object, State};
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
            assets: vec![("月度经营快照".into(), 3)],
            session_sources: vec![
                ("mysql_src".into(), None),
                ("oracle_src".into(), Some("扩展未安装".into())),
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
        assert_eq!(rows.first().map(|o| o.group), Some(Group::Session));
        assert_eq!(rows.last().map(|o| o.group), Some(Group::Files));
    }

    /// `res_` 表单独成段且换成可读名；项目自建表留在「项目分析库」段、名字原样。
    #[test]
    fn persistent_tables_are_split_from_project_owned_ones() {
        let rows = objects(&sample());
        let persistent = rows.iter().find(|o| o.group == Group::Persistent).unwrap();
        assert_eq!(persistent.name, "订单明细 · 2026-11-15 10:30:00");
        // 身份仍是原名：面板上改了显示，删除 / 分析仍按原名定位
        assert_eq!(persistent.key, "persistent:res_订单明细_20261115103000");

        let own = rows.iter().find(|o| o.group == Group::ProjectTables).unwrap();
        assert_eq!(own.name, "数据修复");
    }

    /// 资产行必须标出「最新版本」且只读，并且不给版本选择（模型层没有这个字段）。
    #[test]
    fn asset_rows_carry_latest_version_and_are_read_only() {
        let rows = objects(&sample());
        let asset = rows.iter().find(|o| o.group == Group::Assets).unwrap();
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
        let result = rows.iter().find(|o| o.group == Group::ResultSets).unwrap();
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
        assert_eq!(failed.group, Group::Session);
        assert_eq!(failed.note, "扩展未安装");
        assert!(!failed.can_analyze());
    }

    /// 同一张表在两份分析库里时必须能区分（身份带前缀），否则工作集里会互相顶掉。
    #[test]
    fn identities_are_unique_across_sources() {
        let mut c = sample();
        c.analysis_tables = vec!["orders".into()];
        c.global_tables = vec!["orders".into()];
        c.assets = vec![("orders".into(), 1)];
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
}
