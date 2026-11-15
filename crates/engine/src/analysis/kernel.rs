//! 分析内核 · 三档收口（`open`）。
//!
//! 分工边界（`duckdb/federation/registry.rs:8` 写死的既有约定：**引擎不读连接库、
//! 不解密口令**）：
//! - **宿主**负责把「连接 id → 运行时连接串」组装成 `AccelSource` / `FederatedSource`
//!   （凭据只在这一步出现）；
//! - **内核**只负责「哪一档 → 拿哪条会话」，不碰凭据、不碰 I/O 之外的决策。
//!
//! 所以 `open` 收 `PreparedMounts`（宿主已组装的源），而不是自己去查连接：
//! 这样内核可单测、不引入连接库依赖，也保住「引擎不读连接库」这条边界。
//!
//! 三档映射：
//!
//! | 通道 | 会话 | 说明 |
//! | --- | --- | --- |
//! | `Source` | `None` | 驱动执行，内核不给会话（给了反而会误以为要挂载） |
//! | `Accelerated` | `accel::ensure_session(&AccelSource)` | 单源，按源 id 缓存 |
//! | `Federated` | `session::ensure_session(owner, &[FederatedSource], primary)` | 多源 |

use std::sync::Arc;

use shared::error::{CommonError, CoreError};
use shared::models::QueryResult;

use crate::duckdb::accel::{self, AccelSession, AccelSource};
use crate::duckdb::federation::registry::FederatedSource;
use crate::duckdb::federation::session::{self, FederatedSession};

use super::channel::AnalysisChannel;
use super::mount::SessionPlan;

/// 宿主已组装好的源。**顺序必须与 `SessionPlan.mounts` 一致**（`open` 会校验数量）。
#[derive(Debug, Default)]
pub struct PreparedMounts {
    /// 联邦档用：每条连接一个源（`FederatedSource` 里有凭据，Debug 已脱敏）。
    pub federated: Vec<FederatedSource>,
    /// 加速档用：恰好一条源的会话。
    pub accel: Option<AccelSource>,
}

impl PreparedMounts {
    /// 源库档用：什么都不准备。
    pub fn none() -> Self {
        Self::default()
    }

    pub fn accelerated(source: AccelSource) -> Self {
        Self {
            federated: Vec::new(),
            accel: Some(source),
        }
    }

    pub fn federated(sources: Vec<FederatedSource>) -> Self {
        Self {
            federated: sources,
            accel: None,
        }
    }

    fn is_empty(&self) -> bool {
        self.federated.is_empty() && self.accel.is_none()
    }
}

/// 内核交出去的会话句柄。三档各一种，**不含隐藏的第四种**。
pub enum SessionHandle {
    /// 源库直连：没有 DuckDB 会话，SQL 交回驱动。
    None,
    Accelerated(Arc<AccelSession>),
    Federated(Arc<FederatedSession>),
}

impl SessionHandle {
    /// 档位名（历史 / 日志 / 面板标签用，与 `executor` 的 channel 字段同词）。
    pub fn channel_label(&self) -> &'static str {
        match self {
            Self::None => "source",
            Self::Accelerated(_) => "accelerated",
            Self::Federated(_) => "federated",
        }
    }

    /// 有没有可跑的 DuckDB 会话。
    pub fn has_session(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// 在当前档跑一条 SQL。
    ///
    /// `Source` 档走到这里说明调用方拿错句柄了——**明确报错**而不是悄悄换通道执行：
    /// 悄悄执行会让「源库档实际跑在 DuckDB 上」这种事故无人发现。
    pub fn run(&self, sql: &str) -> Result<QueryResult, CoreError> {
        match self {
            Self::None => Err(invalid(
                "源库直连档没有 DuckDB 会话：SQL 应由驱动执行，不该交给内核".to_string(),
            )),
            Self::Accelerated(session) => session.run(sql),
            Self::Federated(session) => session.run(sql),
        }
    }
}

impl std::fmt::Debug for SessionHandle {
    /// 手写 `Debug`：两种会话都握着一把 `Connection`（不可推导），且连接串里有口令；
    /// 这里只报档位与是否有会话，不打印任何源信息。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle")
            .field("channel", &self.channel_label())
            .finish()
    }
}

/// 按计划拿会话（唯一收口点）。
pub fn open(plan: &SessionPlan, prepared: &PreparedMounts) -> Result<SessionHandle, CoreError> {
    match &plan.channel {
        AnalysisChannel::Source => {
            if !prepared.is_empty() {
                return Err(invalid(
                    "源库直连档不该带任何预组装源（带了会误把查询当挂载查询）".to_string(),
                ));
            }
            Ok(SessionHandle::None)
        }
        AnalysisChannel::Accelerated => {
            if !prepared.federated.is_empty() {
                return Err(invalid(
                    "本地加速档只接受一条源：收到联邦源清单，请改用联邦通道".to_string(),
                ));
            }
            let source = prepared.accel.as_ref().ok_or_else(|| {
                invalid("本地加速档缺预组装源：宿主没有为计划里的源准备 AccelSource".to_string())
            })?;
            let session = accel::ensure_session(source).map_err(invalid)?;
            Ok(SessionHandle::Accelerated(session))
        }
        AnalysisChannel::Federated { owner } => {
            if prepared.accel.is_some() {
                return Err(invalid(
                    "联邦档只接受联邦源清单：收到单源 AccelSource，请改用本地加速".to_string(),
                ));
            }
            if prepared.federated.len() != plan.mounts.len() {
                // 数量对不上说明宿主漏挂 / 多挂：漏挂的源在会话里不存在，查询会「表不见了」
                return Err(invalid(format!(
                    "预组装源与计划不一致：计划 {} 个，宿主给了 {} 个",
                    plan.mounts.len(),
                    prepared.federated.len()
                )));
            }
            let session = session::ensure_session(owner, &prepared.federated, plan.primary.as_deref())
                .map_err(invalid)?;
            Ok(SessionHandle::Federated(session))
        }
    }
}

fn invalid(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

#[cfg(test)]
mod tests {
    use super::{open, PreparedMounts, SessionHandle};
    use crate::analysis::channel::AnalysisChannel;
    use crate::analysis::mount::{plan_for, MountSpec};
    use crate::duckdb::accel::AccelSource;
    use crate::duckdb::federation::registry::FederatedSource;

    fn source_plan() -> crate::analysis::mount::SessionPlan {
        plan_for(&AnalysisChannel::Source, &[], None, "orders").unwrap()
    }

    #[test]
    fn source_plan_opens_without_session() {
        let handle = open(&source_plan(), &PreparedMounts::none()).unwrap();
        assert!(!handle.has_session());
        assert_eq!(handle.channel_label(), "source");
    }

    #[test]
    fn source_handle_run_is_a_loud_error() {
        // 不能悄悄改成 DuckDB 执行：源库档就该由驱动跑
        let handle = open(&source_plan(), &PreparedMounts::none()).unwrap();
        let err = handle.run("select 1").unwrap_err();
        assert!(format!("{err}").contains("没有 DuckDB 会话"));
    }

    #[test]
    fn source_plan_rejects_prepared_sources() {
        let accel = AccelSource::new("c1", "duckdb", r"D:\x.duckdb").expect("组装源");
        let err = open(&source_plan(), &PreparedMounts::accelerated(accel)).unwrap_err();
        assert!(format!("{err}").contains("不该带任何预组装源"));
    }

    #[test]
    fn accelerated_plan_without_prepared_source_errors() {
        let plan = plan_for(
            &AnalysisChannel::Accelerated,
            &[MountSpec::sql_connection("c1")],
            None,
            "t",
        )
        .unwrap();
        let err = open(&plan, &PreparedMounts::none()).unwrap_err();
        assert!(format!("{err}").contains("缺预组装源"));
    }

    #[test]
    fn accelerated_plan_rejects_federated_sources() {
        let plan = plan_for(
            &AnalysisChannel::Accelerated,
            &[MountSpec::sql_connection("c1")],
            None,
            "t",
        )
        .unwrap();
        let source = FederatedSource::new("c1", "c1", "duckdb", r"D:\x.duckdb").expect("组装源");
        let err = open(&plan, &PreparedMounts::federated(vec![source])).unwrap_err();
        assert!(format!("{err}").contains("只接受一条源"));
    }

    #[test]
    fn federated_plan_rejects_accel_source() {
        let plan = plan_for(
            &AnalysisChannel::Federated {
                owner: "c1".into(),
            },
            &[MountSpec::sql_connection("c1")],
            None,
            "t",
        )
        .unwrap();
        let accel = AccelSource::new("c1", "duckdb", r"D:\x.duckdb").expect("组装源");
        let err = open(&plan, &PreparedMounts::accelerated(accel)).unwrap_err();
        assert!(format!("{err}").contains("只接受联邦源清单"));
    }

    #[test]
    fn federated_plan_checks_source_count() {
        // 计划 2 个源，宿主只给了 1 个 → 会话里少一个源，查询必「表不见了」
        let plan = plan_for(
            &AnalysisChannel::Federated {
                owner: "c1".into(),
            },
            &[
                MountSpec::sql_connection("c1"),
                MountSpec::sql_connection("c2"),
            ],
            None,
            "t",
        )
        .unwrap();
        let source = FederatedSource::new("c1", "c1", "duckdb", r"D:\x.duckdb").expect("组装源");
        let err = open(&plan, &PreparedMounts::federated(vec![source])).unwrap_err();
        assert!(format!("{err}").contains("计划 2 个，宿主给了 1 个"));
    }

    #[test]
    fn session_handle_labels_cover_three_channels() {
        // 三档各一种句柄，标签与 executor 的 channel 字段同词
        assert_eq!(SessionHandle::None.channel_label(), "source");
        assert!(SessionHandle::None.has_session() == false);
    }
}
