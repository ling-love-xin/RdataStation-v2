//! 分析内核 · 通道与连接引用。
//!
//! 三个消费方（洞察取样 / 结果集二次分析 / 编辑器联邦通道）过去各自决定「这份源
//! 在哪跑」：编辑器拼 `FederatedPlan`、洞察翻成 `SampleSource`、加速通道另起一条。
//! 本模块只做一件事——把这件事**说清楚**，让三处共用同一套词。
//!
//! 只放类型与判定，不碰 I/O：装配（`plan_for`）与开会话（`AnalysisKernel::open`）
//! 在后续切片里加，这样这一刀可以被纯单测钉住。

/// 一次分析跑在哪。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisChannel {
    /// 进程级 DuckDB 内存单例。默认档：二次分析、导出、默认关开关时的洞察。
    ///
    /// 注意它与 `federation::manager::GLOBAL_DUCKDB` 是**两个不同的单例**，
    /// 不要合并——共用一条全局连接会让「取消洞察」误伤编辑器查询（已实测）。
    Local,
    /// 显式联邦：按 `owner` 建/取一条 `FederatedSession`，多源挂进来。
    /// 编辑器联邦通道直接落这里。
    Federated { owner: String },
}

impl AnalysisChannel {
    /// 是否要建联邦会话。
    pub fn needs_session(&self) -> bool {
        matches!(self, Self::Federated { .. })
    }

    /// 会话 owner（Local 没有 owner）。owner 目前口径 = `conn_id`。
    pub fn owner(&self) -> Option<&str> {
        match self {
            Self::Local => None,
            Self::Federated { owner } => Some(owner),
        }
    }

    /// 入口侧「用联邦查询」开关 + 可挂源数量 → 通道。
    ///
    /// 口径（方案裁决 #3）：洞察 / 分析**默认本地**，开关默认关，且只有 ≥2 个可挂源
    /// 时开关才出现；因此「开关亮着但源不足 1 个」不该被当作联邦——这里直接回落 Local，
    /// 而不是建一条挂不上东西的会话。
    pub fn from_toggle(switch_on: bool, mountable_sources: usize, owner: &str) -> Self {
        if switch_on && mountable_sources >= 1 && !owner.is_empty() {
            Self::Federated {
                owner: owner.to_string(),
            }
        } else {
            Self::Local
        }
    }
}

/// 连接引用的两种身份——这条区分是本模块存在的核心原因。
///
/// 归档资源表里**有** `source_connection_id`（`analytics_resource/src/model/kind.rs:136-139`，
/// `Option<String>`），但那记的是**归档当时**用的连接：
///
/// - 连接可能已被删除 / 改过配置 → 拿它去执行取样会得到「连接不存在」；
/// - 实测口径：这台机器上唯一的项目归档行该字段为 `NULL`，全局资源表 0 行。
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

#[cfg(test)]
mod tests {
    use super::{AnalysisChannel, ConnRef};

    #[test]
    fn local_has_no_owner_and_no_session() {
        let channel = AnalysisChannel::Local;
        assert!(!channel.needs_session());
        assert_eq!(channel.owner(), None);
    }

    #[test]
    fn federated_carries_owner_and_needs_session() {
        let channel = AnalysisChannel::Federated {
            owner: "conn-1".into(),
        };
        assert!(channel.needs_session());
        assert_eq!(channel.owner(), Some("conn-1"));
    }

    #[test]
    fn toggle_off_is_local_even_with_two_sources() {
        assert_eq!(AnalysisChannel::from_toggle(false, 2, "conn-1"), AnalysisChannel::Local);
    }

    #[test]
    fn toggle_on_without_sources_falls_back_to_local() {
        // 开关亮着但一个可挂源都没有：建会话只会立刻失败，不如直接本地
        assert_eq!(AnalysisChannel::from_toggle(true, 0, "conn-1"), AnalysisChannel::Local);
    }

    #[test]
    fn toggle_on_with_source_builds_federated() {
        assert_eq!(
            AnalysisChannel::from_toggle(true, 1, "conn-1"),
            AnalysisChannel::Federated {
                owner: "conn-1".into()
            }
        );
    }

    #[test]
    fn empty_owner_never_builds_federated() {
        // owner 是会话缓存键：空 owner 会把不同面板的会话串成同一条
        assert_eq!(AnalysisChannel::from_toggle(true, 2, ""), AnalysisChannel::Local);
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
}
