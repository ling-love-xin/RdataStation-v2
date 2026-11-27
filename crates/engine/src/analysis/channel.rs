//! 分析内核 · 通道与连接引用。
//!
//! 三个消费方（洞察取样 / 结果集二次分析 / 编辑器执行通道）过去各自决定「这份源
//! 在哪跑」：编辑器拼三档、洞察翻 `SampleSource`、加速通道另起一条。本模块只做一件事
//! ——把这件事**说清楚**，让三处共用同一套词。
//!
//! **执行面是四条，用户能选的是三档**（2026-11-15 用户口径澄清 + 源码核对）：
//!
//! | 执行面 | 谁执行 | 挂载 | 源码 |
//! | --- | --- | --- | --- |
//! | `Source` | 源驱动本身 | 不挂 | `editor/src/ports/editor_exec` 源库档 |
//! | `ProcessDuckDb` | 进程内 DuckDB 单例 | 不挂（表本就在这个单例里） | `services/duckdb_service.rs:13` |
//! | `Accelerated` | 分析实例的一条会话，单源只读挂载 | 1 个源 | `duckdb/accel.rs::ensure_on_instance` |
//! | `Federated` | 分析实例的一条会话，多源 | N 个源 | `kernel::open` 的 `Federated` 分支 |
//!
//! 用户可选的是「源库直连 / 本地加速 / 联邦」三档；`ProcessDuckDb` 不是可选档，而是
//! **内部底座**——导出、临时表（`tmp_q_` / `tmp_i_`）、洞察取样都跑在它上面
//! （`insight/src/insight_engine_internal/mod.rs:59-67` 原文「进程级单例…临时表在同一进程内
//! 全局可见」）。它必须单列：否则「洞察取样跑在哪」又回到各处自己拼。

/// 一次分析跑在哪。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisChannel {
    /// 源库直连：SQL 原样交给驱动执行，**不挂任何源**。
    Source,
    /// 进程内 DuckDB 单例：导出 / 临时表 / 洞察取样。**不挂源**——要读的表
    /// （结果集 `tmp_q_`、取样 `tmp_i_`）本来就是在这个单例里建的。
    ProcessDuckDb,
    /// 本地加速：把**一个**源只读挂进分析实例的一条会话（`accel::ensure_on_instance`）。
    ///
    /// 会话 owner 由调用方决定（2026-11-27 口径，D17）：编辑器 = 文档 id；内核 / 洞察 = 源级
    /// `accel:{conn_id}`（它们手里没有文档身份）。`ATTACH` 是**实例级**的，会话只负责 `USE`。
    Accelerated,
    /// 显式联邦：按 `owner` 建/取 `FederatedSession`，多个源挂进来。
    /// 编辑器联邦档、洞察「多源联邦」开关打开时落这里。
    Federated { owner: String },
}

impl AnalysisChannel {
    /// 是否需要**按 owner / 源建的会话**（源库直连与进程单例都不要）。
    pub fn needs_session(&self) -> bool {
        matches!(self, Self::Accelerated | Self::Federated { .. })
    }

    /// 是否在 DuckDB 上执行（源库直连不在）。
    pub fn runs_in_duckdb(&self) -> bool {
        !matches!(self, Self::Source)
    }

    /// 联邦会话的 owner（只有 `Federated` 有；加速档的 owner 由调用方给——编辑器是文档 id，
    /// 内核是 `accel:{conn_id}`，见 [`Self::Accelerated`] 的注释）。
    pub fn owner(&self) -> Option<&str> {
        match self {
            Self::Federated { owner } => Some(owner),
            _ => None,
        }
    }

    /// 通道名（日志 / 历史面板的分档标签，与 `editor` 侧 channel 字段同词）。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::ProcessDuckDb => "process",
            Self::Accelerated => "accelerated",
            Self::Federated { .. } => "federated",
        }
    }

    /// 「用联邦查询」开关 + 可挂源数量 → 最终通道。
    ///
    /// `base` 是入口原本选的通道（源库直连 或 本地加速）。开关是**附加项**：
    /// 打开且确有可挂源时升级为联邦，否则保持原档——而不是把整条查询改道，
    /// 用户看到的执行通道应当是他选的那个。
    ///
    /// 口径（方案裁决 #3，2026-11-26 更新）：**单源也照样联邦**——点联邦通道就加载底座，
    /// 单源 + 可卸底座时就等同本地加速；数量不是区别。`owner` 为空则不建会话
    /// （空 owner 会让不同面板的会话串成同一条）。
    pub fn from_toggle(
        base: AnalysisChannel,
        switch_on: bool,
        mountable_sources: usize,
        owner: &str,
    ) -> AnalysisChannel {
        if switch_on && mountable_sources >= 1 && !owner.is_empty() {
            Self::Federated {
                owner: owner.to_string(),
            }
        } else {
            base
        }
    }
}

/// 连接引用的两种身份——这条区分是本模块存在的核心原因之一。
///
/// 归档资源表里**有** `source_connection_id`（`analytics_resource/src/model/kind.rs:136-139`，
/// `Option<String>`），但那记的是**归档当时**用的连接：
///
/// - 连接可能已被删除 / 改过配置 → 拿它去执行取样会得到「连接不存在」；
/// - 实测口径：这台机器上唯一的项目归档行该字段为 `NULL`，全局资源表 0 行。
/// - 而全局连接本身是存在的：`global.db` 的 `global_connections` 1 行
///   （`G_conn_SQLite_Example`），项目库 `connections` 里另有 4 条 `GP_` 前缀共享连接。
///   → 解析 `Live` 必须对**合并后的连接清单**做，不能只查其中一处。
///
/// 所以取样必须拿 `Live`（当前连接列表里确实存在、可连），`Historical` 只用于
/// 溯源展示与「原来源」提示。混用这两种 id 是「点一下没反应 / 报连接不存在」的成因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnRef {
    /// 当前可连的连接：可以做取样 / 建会话。
    Live(String),
    /// 归档记录里的来源连接：只做溯源，不可执行。
    Historical(String),
}

impl ConnRef {
    /// 能否拿它去执行查询。
    pub fn is_executable(&self) -> bool {
        matches!(self, Self::Live(_))
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Live(id) | Self::Historical(id) => id,
        }
    }
}

/// 把归档记录里的 `source_connection_id` 解析成 [`ConnRef`]。
///
/// **`live_ids` 必须是合并后的连接清单**（`global.db` 的 `global_connections` +
/// 项目库的 `connections`）：只查其中一处会把「全局连接存在、项目里没有」这种正常情况
/// 判成 `Historical`，也让 `GP_` 前缀的共享连接在另一侧查不到。这条是从实测纠正来的，
/// 不是保守写法。
///
/// 返回值：
/// - `None` —— 归档行**没有记录**来源连接（实测唯一那行就是 `NULL`）：无从溯源，
///   调用方应展示「未记录来源」而不是编一个 id；
/// - `Some(ConnRef::Live)` —— 确在当前清单里，可以取样 / 建会话；
/// - `Some(ConnRef::Historical)` —— 记过但已不在清单（连接被删 / 改了名 / 属于别的机器），
///   只做溯源提示，**不可执行**。
pub fn classify_conn(historical_id: Option<&str>, live_ids: &[&str]) -> Option<ConnRef> {
    let id = historical_id.map(str::trim).filter(|id| !id.is_empty())?;
    if live_ids.iter().any(|live| *live == id) {
        Some(ConnRef::Live(id.to_string()))
    } else {
        Some(ConnRef::Historical(id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_conn, AnalysisChannel, ConnRef};

    #[test]
    fn source_channel_needs_no_session() {
        let channel = AnalysisChannel::Source;
        assert!(!channel.needs_session());
        assert_eq!(channel.owner(), None);
        assert_eq!(channel.label(), "source");
    }

    #[test]
    fn accelerated_channel_needs_session_but_has_no_federation_owner() {
        // 加速档的缓存键是源连接 id，由 accel 自己管；它不是联邦 owner
        let channel = AnalysisChannel::Accelerated;
        assert!(channel.needs_session());
        assert_eq!(channel.owner(), None);
        assert_eq!(channel.label(), "accelerated");
    }

    #[test]
    fn federated_carries_owner_and_needs_session() {
        let channel = AnalysisChannel::Federated {
            owner: "conn-1".into(),
        };
        assert!(channel.needs_session());
        assert_eq!(channel.owner(), Some("conn-1"));
        assert_eq!(channel.label(), "federated");
    }

    #[test]
    fn toggle_off_keeps_the_base_channel() {
        assert_eq!(
            AnalysisChannel::from_toggle(AnalysisChannel::Source, false, 3, "conn-1"),
            AnalysisChannel::Source
        );
        assert_eq!(
            AnalysisChannel::from_toggle(AnalysisChannel::Accelerated, false, 3, "conn-1"),
            AnalysisChannel::Accelerated
        );
    }

    #[test]
    fn toggle_on_with_one_source_upgrades_to_single_source_federation() {
        // 单源联邦是合法形态（2026-11-26 用户口径）：点联邦就挂底座，单源也照样建联邦会话
        assert_eq!(
            AnalysisChannel::from_toggle(AnalysisChannel::Accelerated, true, 1, "conn-1"),
            AnalysisChannel::Federated {
                owner: "conn-1".into()
            }
        );
    }

    #[test]
    fn toggle_on_with_two_sources_upgrades_to_federated() {
        assert_eq!(
            AnalysisChannel::from_toggle(AnalysisChannel::Source, true, 2, "conn-1"),
            AnalysisChannel::Federated {
                owner: "conn-1".into()
            }
        );
    }

    #[test]
    fn empty_owner_never_upgrades() {
        assert_eq!(
            AnalysisChannel::from_toggle(AnalysisChannel::Source, true, 3, ""),
            AnalysisChannel::Source
        );
    }

    #[test]
    fn historical_conn_is_not_executable() {
        let historical = ConnRef::Historical("conn-gone".into());
        assert!(!historical.is_executable());
        assert_eq!(historical.id(), "conn-gone");
    }

    #[test]
    fn live_conn_is_executable() {
        let live = ConnRef::Live("conn-1".into());
        assert!(live.is_executable());
        assert_eq!(live.id(), "conn-1");
    }

    /// 解析必须对**合并后的清单**做：全局库里那条 `G_` 与项目库里那条 `GP_` 都算 `Live`。
    /// （实测事实：前者只在 `global_connections`，后者只在项目 `connections`。）
    #[test]
    fn classify_conn_resolves_against_the_merged_list() {
        let merged = ["G_conn_SQLite_Example", "GP_real_pg_20260912"];
        // 全局连接：只出现在 global_connections
        let global = classify_conn(Some("G_conn_SQLite_Example"), &merged);
        assert_eq!(
            global,
            Some(ConnRef::Live("G_conn_SQLite_Example".into())),
            "全局连接在合并清单里就是 Live"
        );
        assert!(global.expect("有值").is_executable());
        // 项目里的共享连接：只出现在 connections
        assert_eq!(
            classify_conn(Some("GP_real_pg_20260912"), &merged),
            Some(ConnRef::Live("GP_real_pg_20260912".into()))
        );
        // 记过但已不在清单 → 只溯源
        let gone = classify_conn(Some("P_conn_deleted"), &merged).expect("有值");
        assert!(!gone.is_executable(), "不在清单里的连接不可执行");
        assert_eq!(gone.id(), "P_conn_deleted");
        // 归档行没记录来源（实测那行就是 NULL）→ 不编 id
        assert_eq!(classify_conn(None, &merged), None);
        // 空白串与 None 同义（库里有历史脏数据写成空串的情况）
        assert_eq!(classify_conn(Some("   "), &merged), None);
        // 两侧都空（项目没有连接、全局也没有）时不会把任何 id 说成 Live
        assert_eq!(
            classify_conn(Some("conn-x"), &[]),
            Some(ConnRef::Historical("conn-x".into()))
        );
    }
}
