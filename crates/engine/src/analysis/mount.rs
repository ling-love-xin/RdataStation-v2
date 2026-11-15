//! 分析内核 · 「源 → 挂载计划」的唯一翻译点。
//!
//! 目的：编辑器、洞察、结果集二次分析过去各自把「用户选的源」翻译成引擎能挂的东西
//! （编辑器拼 `FederatedPlan`、洞察翻 `SampleSource`、加速通道另起一条），三份翻译
//! 迟早会走样。这里把它收成一处，**只做映射与校验，不做 I/O**——真正 `ATTACH`
//! 仍由 `duckdb::federation::session::ensure_session` 完成。
//!
//! 语义前提（方案裁决，见 `federation-entry-design.md`）：
//! - `Local` = 进程级 DuckDB 内存单例。它**不能** `ATTACH` 别人的连接，所以
//!   `Local` 计划里的 `mounts` 必须为空；数据库表在 Local 下走驱动取样（不 attach）。
//! - `Federated` = 按 owner 建/取 `FederatedSession`，源以 `ATTACH` 挂进来；
//!   一个源都没有时 `ensure_session` 会直接拒（`session.rs:503-507`），所以这里提前报，
//!   让入口能说人话而不是暴露引擎的兜底文案。

use std::path::PathBuf;

use shared::error::{CommonError, CoreError};

use super::channel::AnalysisChannel;

/// 一个源「怎么挂」——内核只认这一层，不认 UI 里的五个叶子。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MountSpec {
    /// 驱动连接（L1）：只读挂载。`conn_id` 必须是**当前可连**的 id（`ConnRef::Live`）。
    SqlConnection { conn_id: String },
    /// 本地 DuckDB 文件（项目分析库 / 全局库 / 已归档的 duckdb）。
    DuckDbFile { path: PathBuf },
    /// 已经在某个 DuckDB 实例里的表（结果集 / 先前物化的会话表）。
    DuckDbTable { catalog: String, schema: String },
}

impl MountSpec {
    pub fn sql_connection(conn_id: impl Into<String>) -> Self {
        Self::SqlConnection {
            conn_id: conn_id.into(),
        }
    }

    pub fn duckdb_file(path: impl Into<PathBuf>) -> Self {
        Self::DuckDbFile { path: path.into() }
    }

    pub fn duckdb_table(catalog: impl Into<String>, schema: impl Into<String>) -> Self {
        Self::DuckDbTable {
            catalog: catalog.into(),
            schema: schema.into(),
        }
    }

    /// 挂载点的身份键：用于去重（同一条连接 / 同一个文件 / 同一张表只挂一次）。
    fn dedup_key(&self) -> String {
        match self {
            Self::SqlConnection { conn_id } => format!("conn:{conn_id}"),
            Self::DuckDbFile { path } => format!("file:{}", path.display()),
            Self::DuckDbTable { catalog, schema } => format!("table:{catalog}.{schema}"),
        }
    }

    /// 该挂载的主人名（会话里 `ATTACH` 后的 alias 由 session 决定，这里只用于 primary 校验）。
    fn owner_name(&self) -> &str {
        match self {
            Self::SqlConnection { conn_id } => conn_id,
            Self::DuckDbTable { catalog, .. } => catalog,
            Self::DuckDbFile { path } => path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default(),
        }
    }
}

/// 一次分析的完整装配结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlan {
    pub channel: AnalysisChannel,
    /// 要挂进会话的源；`Local` 一律为空（见模块文档）。
    pub mounts: Vec<MountSpec>,
    /// 会话默认 catalog（`ensure_session` 的 `requested_primary`）。
    pub primary: Option<String>,
    /// 面板标题 / 快照来源用的可读名。
    pub label: String,
}

/// 源 → 计划。
///
/// `label` 由调用方给（只有入口知道该叫什么：表名 / 文件名 / 用户点的那一行）。
pub fn plan_for(
    channel: &AnalysisChannel,
    mounts: &[MountSpec],
    primary: Option<&str>,
    label: impl Into<String>,
) -> Result<SessionPlan, CoreError> {
    let label = label.into();
    match channel {
        AnalysisChannel::Local => {
            // Local 不下发 ATTACH：有源说明调用方拿错了通道（例如把「多源联邦」当成默认档）
            if !mounts.is_empty() {
                return Err(invalid(format!(
                    "本地通道不能挂载源：{}（联邦请显式选通道）",
                    mounts.len()
                )));
            }
            Ok(SessionPlan {
                channel: AnalysisChannel::Local,
                mounts: Vec::new(),
                primary: None,
                label,
            })
        }
        AnalysisChannel::Federated { owner } => {
            if owner.is_empty() {
                return Err(invalid("联邦通道的 owner 不能为空".to_string()));
            }
            // 去重：同一条连接被两个入口各挂一次是最常见的浪费（多一条只读副本 + 一次内省）
            let mut deduped: Vec<MountSpec> = Vec::with_capacity(mounts.len());
            for m in mounts {
                if !deduped.iter().any(|x| x.dedup_key() == m.dedup_key()) {
                    deduped.push(m.clone());
                }
            }
            if deduped.is_empty() {
                return Err(invalid(
                    "联邦查询至少需要一个源：先把源加进会话，或改用本地通道".to_string(),
                ));
            }
            if let Some(p) = primary {
                if p.is_empty() {
                    return Err(invalid("primary 给了空串".to_string()));
                }
                if !deduped.iter().any(|m| m.owner_name() == p) {
                    return Err(invalid(format!(
                        "primary={p} 不在挂载清单里：默认 catalog 指向一个没挂的源，查询会报 catalog 不存在"
                    )));
                }
            }
            Ok(SessionPlan {
                channel: AnalysisChannel::Federated {
                    owner: owner.clone(),
                },
                mounts: deduped,
                primary: primary.map(str::to_string),
                label,
            })
        }
    }
}

fn invalid(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

#[cfg(test)]
mod tests {
    use super::{plan_for, MountSpec};
    use crate::analysis::channel::AnalysisChannel;

    fn federated() -> AnalysisChannel {
        AnalysisChannel::Federated {
            owner: "conn-1".into(),
        }
    }

    fn single_conn() -> Vec<MountSpec> {
        vec![MountSpec::sql_connection("conn-1")]
    }

    #[test]
    fn local_rejects_any_mount() {
        let err = plan_for(&AnalysisChannel::Local, &single_conn(), None, "t").unwrap_err();
        assert!(format!("{err}").contains("本地通道不能挂载源"));
    }

    #[test]
    fn local_plan_has_no_mounts_and_no_primary() {
        let plan = plan_for(&AnalysisChannel::Local, &[], None, "orders").unwrap();
        assert!(plan.mounts.is_empty());
        assert!(plan.primary.is_none());
        assert_eq!(plan.label, "orders");
    }

    #[test]
    fn federated_rejects_empty_mounts() {
        let err = plan_for(&federated(), &[], None, "t").unwrap_err();
        assert!(format!("{err}").contains("至少需要一个源"));
    }

    #[test]
    fn federated_rejects_empty_owner() {
        let ownerless = AnalysisChannel::Federated { owner: String::new() };
        let err = plan_for(&ownerless, &single_conn(), None, "t").unwrap_err();
        assert!(format!("{err}").contains("owner 不能为空"));
    }

    #[test]
    fn federated_dedupes_same_connection() {
        let mounts = vec![
            MountSpec::sql_connection("conn-1"),
            MountSpec::sql_connection("conn-1"),
            MountSpec::duckdb_table("memory", "main"),
        ];
        let plan = plan_for(&federated(), &mounts, None, "t").unwrap();
        assert_eq!(plan.mounts.len(), 2);
    }

    #[test]
    fn federated_keeps_distinct_kinds_apart() {
        let mounts = vec![
            MountSpec::sql_connection("conn-1"),
            MountSpec::duckdb_file(r"D:\a\analytics.duckdb"),
            MountSpec::duckdb_table("memory", "main"),
        ];
        let plan = plan_for(&federated(), &mounts, None, "t").unwrap();
        assert_eq!(plan.mounts.len(), 3);
    }

    #[test]
    fn primary_must_point_at_a_mounted_source() {
        let err = plan_for(&federated(), &single_conn(), Some("conn-2"), "t").unwrap_err();
        assert!(format!("{err}").contains("primary=conn-2"));
    }

    #[test]
    fn primary_naming_a_mounted_connection_passes() {
        let plan = plan_for(&federated(), &single_conn(), Some("conn-1"), "t").unwrap();
        assert_eq!(plan.primary.as_deref(), Some("conn-1"));
    }

    #[test]
    fn empty_primary_is_rejected() {
        let err = plan_for(&federated(), &single_conn(), Some(""), "t").unwrap_err();
        assert!(format!("{err}").contains("primary 给了空串"));
    }

    #[test]
    fn duckdb_file_owner_name_is_file_stem() {
        // primary 用文件主干名比较（ATTACH 后 catalog 默认取 stem）
        let mounts = vec![MountSpec::duckdb_file(r"D:\data\an.duckdb")];
        assert_eq!(mounts[0].owner_name_for_test(), "an");
    }

    impl MountSpec {
        #[cfg(test)]
        fn owner_name_for_test(&self) -> &str {
            self.owner_name()
        }
    }
}
