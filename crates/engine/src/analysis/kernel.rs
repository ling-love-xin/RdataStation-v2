//! 分析内核 · 三档收口（`open`）。
//!
//! 分工边界（`duckdb/federation/registry.rs:8` 写死的既有约定：**引擎不读连接库、
//! 不解密口令**）：
//! - **宿主**负责把「连接 id → 运行时连接串」组装成 `AccelSource` / `FederatedSource`
//!   （凭据只在这一步出现）；
//! - **内核**只负责「哪一档 → 拿哪条会话」，不碰凭据，也不替宿主决定别名。
//!
//! 所以 `PreparedMounts` 里的每一条都**带着它对应的 `MountSpec`**，而不是一串裸源：
//! 「计划说要挂 A（别名 a1）」和「宿主给的源别名是 a2」必须当场对上账，否则查询会在
//! 一条名字不对的 catalog 上跑，报的还是「表不存在」这种误导性错误。

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use duckdb::Connection;
use shared::error::{CommonError, CoreError};
use shared::models::QueryResult;

use crate::duckdb::accel::{self, AccelSession, AccelSource};
use crate::duckdb::federation::registry::FederatedSource;
use crate::duckdb::federation::session::FederatedSession;
use crate::duckdb::instance::{self, AnalysisSession};
use crate::services::duckdb_service::DuckDbService;

use super::channel::AnalysisChannel;
use super::mount::{MountSpec, SessionPlan};

/// 一条联邦源：计划里的挂载键 + 宿主组装出来的源。
pub struct PreparedFederated {
    pub mount: MountSpec,
    pub source: FederatedSource,
}

/// 一条加速源：计划里的挂载键 + 宿主组装出来的源。
pub struct PreparedAccel {
    pub mount: MountSpec,
    pub source: AccelSource,
}

/// 宿主已组装好的源。**顺序必须与 `SessionPlan.mounts` 一致**（`open` 逐条比对挂载键）。
#[derive(Default)]
pub struct PreparedMounts {
    federated: Vec<PreparedFederated>,
    accel: Option<PreparedAccel>,
}

impl PreparedMounts {
    /// 源库档用：什么都不准备。
    pub fn none() -> Self {
        Self::default()
    }

    pub fn accelerated(mount: MountSpec, source: AccelSource) -> Self {
        Self {
            federated: Vec::new(),
            accel: Some(PreparedAccel { mount, source }),
        }
    }

    pub fn federated(entries: Vec<(MountSpec, FederatedSource)>) -> Self {
        Self {
            federated: entries
                .into_iter()
                .map(|(mount, source)| PreparedFederated { mount, source })
                .collect(),
            accel: None,
        }
    }

    fn is_empty(&self) -> bool {
        self.federated.is_empty() && self.accel.is_none()
    }
}

/// 进程内单例上一条 SQL 的「正在跑」标志（`run_sql_on` 要求一个 `AtomicBool`）。
///
/// 单例本身是 `Mutex` 串行的，一条连接上不可能有两条 SQL 并发；用**同一个**静态标志
/// 正是这个事实的写法——不需要按调用方各发一个。
static PROCESS_RUNNING: AtomicBool = AtomicBool::new(false);

/// 内核交出去的会话句柄。四条执行面各一种，**不含隐藏的第五种**。
pub enum SessionHandle {
    /// 源库直连：没有 DuckDB 会话，SQL 交回驱动。
    None,
    /// 进程内 DuckDB 单例（导出 / 临时表 / 洞察取样）。
    ProcessDb(Arc<Mutex<Connection>>),
    Accelerated(Arc<AccelSession>),
    Federated(Arc<FederatedSession>),
    /// 分析实例上的一条会话（2026-11-26 起本地加速 / 联邦都落这里）。
    ///
    /// 与 `Federated` / `Accelerated` 的区别不是"类型不同"，而是**不再各建一条内存库**：
    /// catalog（底座 + 源别名）由实例共享，`TEMP` 仍是本会话的。`label` 保留原来的
    /// 档位词（`accelerated` / `federated`），历史面板与日志不必改口径。
    Instance {
        session: Arc<AnalysisSession>,
        label: &'static str,
    },
}

impl std::fmt::Debug for SessionHandle {
    /// 手写 `Debug`：两种会话都握着一把 `Connection`（不可推导），且连接串里有口令；
    /// 这里只报档位，不打印任何源信息。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle")
            .field("channel", &self.channel_label())
            .finish()
    }
}

impl SessionHandle {
    /// 档位名（历史 / 日志 / 面板标签用，与 `executor` 的 channel 字段同词）。
    pub fn channel_label(&self) -> &'static str {
        match self {
            Self::None => "source",
            Self::ProcessDb(_) => "process",
            Self::Accelerated(_) => "accelerated",
            Self::Federated(_) => "federated",
            Self::Instance { label, .. } => label,
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
            Self::ProcessDb(conn) => {
                let guard = conn
                    .lock()
                    .map_err(|e| invalid(format!("DuckDB 单例锁中毒: {e}")))?;
                accel::run_sql_on(&guard, &PROCESS_RUNNING, sql)
            }
            Self::Accelerated(session) => session.run(sql),
            Self::Federated(session) => session.run(sql),
            Self::Instance { session, .. } => session.run(sql),
        }
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
        AnalysisChannel::ProcessDuckDb => {
            if !prepared.is_empty() {
                return Err(invalid(
                    "进程内 DuckDB 不挂载外部源：要挂源请走本地加速或联邦".to_string(),
                ));
            }
            let conn = DuckDbService::get_or_create_duckdb()?;
            Ok(SessionHandle::ProcessDb(conn))
        }
        AnalysisChannel::Accelerated => {
            if !prepared.federated.is_empty() {
                return Err(invalid(
                    "本地加速档只接受一条源：收到联邦源清单，请改用联邦通道".to_string(),
                ));
            }
            let prepared_accel = prepared.accel.as_ref().ok_or_else(|| {
                invalid("本地加速档缺预组装源：宿主没有为计划里的源准备 AccelSource".to_string())
            })?;
            let expected = plan
                .mounts
                .first()
                .ok_or_else(|| invalid("本地加速档的计划没有源".to_string()))?;
            check_same_mount(expected, &prepared_accel.mount, "本地加速")?;
            // 本地加速也落**同一个分析实例**：一条会话 + 挂这一条源，不再另起内存库。
            // 别名按源唯一、幂等判重、`USE` 都收在 `accel::ensure_on_instance` 一处——
            // 编辑器里直跑的加速档走的也是它，两边各写一遍迟早给出两套挂载事实。
            let (session, _alias) =
                accel::ensure_on_instance(&prepared_accel.source).map_err(invalid)?;
            Ok(SessionHandle::Instance {
                session,
                label: "accelerated",
            })
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
            // 逐条对账：既防顺序错位，也防别名被宿主悄悄改掉
            for (index, (expected, given)) in plan.mounts.iter().zip(&prepared.federated).enumerate() {
                check_same_mount(expected, &given.mount, &format!("第 {} 个源", index + 1))?;
                check_alias(expected, &given.source)?;
            }
            let sources: Vec<FederatedSource> = prepared
                .federated
                .iter()
                .map(|entry| entry.source.clone())
                .collect();
            // 联邦档同样落**同一个分析实例**：按 owner 取会话，把源挂进这条会话。
            // 好处是 `tmp_q_`（结果集临时表）与源在同一个实例里，`tmp_q_ JOIN 源` 才成立
            // （第 ⑪ 条遗留的真因是"两份实例之间没有 JOIN 这回事"）。
            let instance = instance::instance().map_err(invalid)?;
            let session = instance.ensure_session(owner, "联邦").map_err(invalid)?;
            let mounted = session.mount_sources(&sources).map_err(invalid)?;
            // 主源只在"它确实挂上了"时切：挂不上就保持默认 catalog，
            // 由快照里的原因告诉用户，而不是让整条查询跑在一个不存在的 catalog 上
            if let Some(primary) = plan.primary.as_deref() {
                let ready = mounted.iter().any(|entry| {
                    entry.alias() == primary && entry.is_ready()
                });
                if ready {
                    session.use_catalog(primary).map_err(invalid)?;
                }
            }
            Ok(SessionHandle::Instance {
                session,
                label: "federated",
            })
        }
    }
}

/// 计划里的挂载键必须与宿主给的挂载键**完全一致**（同一种源 + 同一个身份）。
fn check_same_mount(expected: &MountSpec, given: &MountSpec, who: &str) -> Result<(), CoreError> {
    if expected == given {
        return Ok(());
    }
    Err(invalid(format!(
        "{who}：计划要挂 {expected:?}，宿主给的却是 {given:?}"
    )))
}

/// 连接源的别名必须与计划一致：别名就是 SQL 里 `ATTACH` 后能看到的 catalog 名。
fn check_alias(expected: &MountSpec, source: &FederatedSource) -> Result<(), CoreError> {
    let Some(alias) = expected.alias() else {
        return Ok(());
    };
    if alias == source.alias {
        return Ok(());
    }
    Err(invalid(format!(
        "别名对不上：计划写 {alias}，宿主给的是 {}（SQL 里的限定名会落到错 catalog）",
        source.alias
    )))
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

    fn accel_plan() -> crate::analysis::mount::SessionPlan {
        plan_for(
            &AnalysisChannel::Accelerated,
            &[MountSpec::sql_connection("c1")],
            None,
            "t",
        )
        .unwrap()
    }

    fn fed_plan(mounts: Vec<MountSpec>) -> crate::analysis::mount::SessionPlan {
        plan_for(
            &AnalysisChannel::Federated {
                owner: "c1".into(),
            },
            &mounts,
            None,
            "t",
        )
        .unwrap()
    }

    fn duckdb_source(conn_id: &str, alias: &str) -> FederatedSource {
        FederatedSource::new(conn_id, alias, "duckdb", r"D:\x.duckdb").expect("组装源")
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
        let err = open(
            &source_plan(),
            &PreparedMounts::accelerated(MountSpec::sql_connection("c1"), accel),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("不该带任何预组装源"));
    }

    #[test]
    fn accelerated_plan_without_prepared_source_errors() {
        let err = open(&accel_plan(), &PreparedMounts::none()).unwrap_err();
        assert!(format!("{err}").contains("缺预组装源"));
    }

    #[test]
    fn accelerated_plan_rejects_federated_sources() {
        let entries = vec![(
            MountSpec::sql_connection("c1"),
            duckdb_source("c1", "c1"),
        )];
        let err = open(&accel_plan(), &PreparedMounts::federated(entries)).unwrap_err();
        assert!(format!("{err}").contains("只接受一条源"));
    }

    #[test]
    fn federated_plan_rejects_accel_source() {
        let plan = fed_plan(vec![MountSpec::sql_connection("c1")]);
        let accel = AccelSource::new("c1", "duckdb", r"D:\x.duckdb").expect("组装源");
        let err = open(
            &plan,
            &PreparedMounts::accelerated(MountSpec::sql_connection("c1"), accel),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("只接受联邦源清单"));
    }

    #[test]
    fn federated_plan_checks_source_count() {
        // 计划 2 个源，宿主只给了 1 个 → 会话里少一个源，查询必「表不见了」
        let plan = fed_plan(vec![
            MountSpec::sql_connection("c1"),
            MountSpec::sql_connection("c2"),
        ]);
        let entries = vec![(
            MountSpec::sql_connection("c1"),
            duckdb_source("c1", "c1"),
        )];
        let err = open(&plan, &PreparedMounts::federated(entries)).unwrap_err();
        assert!(format!("{err}").contains("计划 2 个，宿主给了 1 个"));
    }

    #[test]
    fn federated_plan_detects_mount_mismatch() {
        // 计划第 1 个是连接，宿主第 1 个给了文件 → 顺序或源类型错了，必须当场拒
        let plan = fed_plan(vec![
            MountSpec::sql_connection("c1"),
            MountSpec::sql_connection("c2"),
        ]);
        let entries = vec![
            (
                MountSpec::duckdb_file(r"D:\a.duckdb"),
                duckdb_source("c1", "c1"),
            ),
            (
                MountSpec::sql_connection("c2"),
                duckdb_source("c2", "c2"),
            ),
        ];
        let err = open(&plan, &PreparedMounts::federated(entries)).unwrap_err();
        assert!(format!("{err}").contains("第 1 个源"));
    }

    #[test]
    fn federated_plan_detects_alias_mismatch() {
        // 计划要求别名 sales，宿主给的是 s1 → 限定名会落到错 catalog
        let plan = plan_for(
            &AnalysisChannel::Federated {
                owner: "c1".into(),
            },
            &[MountSpec::sql_connection_as("c1", "sales")],
            None,
            "t",
        )
        .unwrap();
        let entries = vec![(
            MountSpec::sql_connection_as("c1", "sales"),
            duckdb_source("c1", "s1"),
        )];
        let err = open(&plan, &PreparedMounts::federated(entries)).unwrap_err();
        assert!(format!("{err}").contains("别名对不上"));
    }

    #[test]
    fn session_handle_none_reports_source_channel() {
        // 四条执行面各一种句柄，标签与 executor 的 channel 字段同词
        assert_eq!(SessionHandle::None.channel_label(), "source");
        assert!(!SessionHandle::None.has_session());
    }

    #[test]
    fn process_channel_opens_the_process_singleton_and_runs() {
        let plan = plan_for(&AnalysisChannel::ProcessDuckDb, &[], None, "tmp_i_x").unwrap();
        let handle = open(&plan, &PreparedMounts::none()).unwrap();
        assert_eq!(handle.channel_label(), "process");
        assert!(handle.has_session());
        // 真跑：这正是洞察取样要的形态（表在单例里、SQL 在单例上执行）
        handle
            .run("CREATE OR REPLACE TEMP TABLE _kernel_process_probe(a INT)")
            .expect("建表");
        let result = handle
            .run("SELECT count(*) FROM _kernel_process_probe")
            .expect("查询");
        assert_eq!(result.rows.len(), 1, "应有一行 count");
    }

    #[test]
    fn process_channel_rejects_mounted_sources() {
        let plan = plan_for(&AnalysisChannel::ProcessDuckDb, &[], None, "t").unwrap();
        let accel = AccelSource::new("c1", "duckdb", r"D:\x.duckdb").expect("组装源");
        let err = open(
            &plan,
            &PreparedMounts::accelerated(MountSpec::sql_connection("c1"), accel),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("不挂载外部源"));
    }
}
