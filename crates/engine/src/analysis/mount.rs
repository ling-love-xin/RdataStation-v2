//! 分析内核 · 「源 → 挂载计划」的唯一翻译点。
//!
//! 目的：编辑器、洞察、结果集二次分析过去各自把「用户选的源」翻译成引擎能挂的东西
//! （编辑器拼 `FederatedPlan` / 加速档拼 `AccelSource`、洞察翻 `SampleSource`），三份
//! 翻译迟早会走样。这里把它收成一处，**只做映射与校验，不做 I/O**——真正 `ATTACH`
//! 仍由 `duckdb::accel`（单源）与 `duckdb::federation::session`（多源）完成。
//!
//! 三档口径（`channel.rs` 模块文档有源码对照）：
//! - `Source` 源库直连：**不挂任何源**，SQL 交给驱动；带了挂载说明入口拿错档。
//! - `Accelerated` 本地加速：**恰好一个源**（`accel::ensure_session` 按源 id 缓存），
//!   且只能是「可挂的连接」或「DuckDB 文件」——已经躺在某个实例里的表（`DuckDbTable`）
//!   本身就是本地实例的东西，不需要再挂一次。
//! - `Federated` 联邦：≥1 个源，同连接/同文件/同表去重，`primary` 必须指向已挂源。

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

    /// 该源在会话里的名字（`primary` 校验用）。
    ///
    /// 连接 = 连接 id（`federation` 侧按 id 建 alias）；文件 = 文件主干名（`ATTACH` 后
    /// catalog 默认取 stem）；表 = 它所在的 catalog。
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

    /// 加速档能不能挂它（见模块文档第 2 条）。
    fn is_accel_mountable(&self) -> bool {
        matches!(self, Self::SqlConnection { .. } | Self::DuckDbFile { .. })
    }
}

/// 一次分析的完整装配结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlan {
    pub channel: AnalysisChannel,
    /// 要挂进会话的源；`Source` 一律为空（见模块文档）。
    pub mounts: Vec<MountSpec>,
    /// 会话默认 catalog（`federation::ensure_session` 的 `requested_primary`）。
    pub primary: Option<String>,
    /// 面板标题 / 快照来源用的可读名。
    pub label: String,
}

/// 源 → 计划。`label` 由调用方给（只有入口知道该叫什么：表名 / 文件名 / 用户点的那一行）。
pub fn plan_for(
    channel: &AnalysisChannel,
    mounts: &[MountSpec],
    primary: Option<&str>,
    label: impl Into<String>,
) -> Result<SessionPlan, CoreError> {
    let label = label.into();
    match channel {
        AnalysisChannel::Source => {
            // 源库档不进 DuckDB：有源说明调用方拿错了通道
            if !mounts.is_empty() {
                return Err(invalid(format!(
                    "源库直连不挂载任何源：给了 {} 个（要挂载请选本地加速或联邦）",
                    mounts.len()
                )));
            }
            Ok(SessionPlan {
                channel: AnalysisChannel::Source,
                mounts: Vec::new(),
                primary: None,
                label,
            })
        }
        AnalysisChannel::Accelerated => {
            match mounts.len() {
                0 => {
                    return Err(invalid(
                        "本地加速需要一个源：没有源就没有可挂的对象，请改用源库直连".to_string(),
                    ))
                }
                1 => {}
                n => {
                    return Err(invalid(format!(
                        "本地加速是单源会话：给了 {n} 个源，多源请用联邦"
                    )))
                }
            }
            let mount = &mounts[0];
            if !mount.is_accel_mountable() {
                return Err(invalid(format!(
                    "本地加速不能挂 {}：它已经在某个 DuckDB 实例里，无需再挂",
                    mount.owner_name()
                )));
            }
            Ok(SessionPlan {
                channel: AnalysisChannel::Accelerated,
                mounts: vec![mount.clone()],
                // 单源会话只有一个 catalog，不需要 primary（给了也不认）
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

    // ---- 源库直连档 ----

    #[test]
    fn source_rejects_any_mount() {
        let err = plan_for(&AnalysisChannel::Source, &single_conn(), None, "t").unwrap_err();
        assert!(format!("{err}").contains("源库直连不挂载任何源"));
    }

    #[test]
    fn source_plan_has_no_mounts_and_no_primary() {
        let plan = plan_for(&AnalysisChannel::Source, &[], None, "orders").unwrap();
        assert!(plan.mounts.is_empty());
        assert!(plan.primary.is_none());
        assert_eq!(plan.label, "orders");
        assert_eq!(plan.channel, AnalysisChannel::Source);
    }

    // ---- 本地加速档 ----

    #[test]
    fn accelerated_rejects_zero_mounts() {
        let err = plan_for(&AnalysisChannel::Accelerated, &[], None, "t").unwrap_err();
        assert!(format!("{err}").contains("本地加速需要一个源"));
    }

    #[test]
    fn accelerated_rejects_two_mounts() {
        let mounts = vec![
            MountSpec::sql_connection("conn-1"),
            MountSpec::sql_connection("conn-2"),
        ];
        let err = plan_for(&AnalysisChannel::Accelerated, &mounts, None, "t").unwrap_err();
        assert!(format!("{err}").contains("单源会话"));
    }

    #[test]
    fn accelerated_accepts_one_connection() {
        let plan = plan_for(&AnalysisChannel::Accelerated, &single_conn(), None, "t").unwrap();
        assert_eq!(plan.mounts.len(), 1);
        assert!(plan.primary.is_none());
        assert_eq!(plan.channel, AnalysisChannel::Accelerated);
    }

    #[test]
    fn accelerated_accepts_one_duckdb_file() {
        let mounts = vec![MountSpec::duckdb_file(r"D:\a\analytics.duckdb")];
        let plan = plan_for(&AnalysisChannel::Accelerated, &mounts, None, "t").unwrap();
        assert_eq!(plan.mounts.len(), 1);
    }

    #[test]
    fn accelerated_rejects_duckdb_table() {
        let mounts = vec![MountSpec::duckdb_table("memory", "main")];
        let err = plan_for(&AnalysisChannel::Accelerated, &mounts, None, "t").unwrap_err();
        assert!(format!("{err}").contains("不能挂 memory"));
    }

    // ---- 联邦档 ----

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
    fn federated_accepts_duckdb_table() {
        // 结果集二次分析（形态 II）要把结果集表挂进联邦会话，这是被允许的
        let mounts = vec![MountSpec::duckdb_table("memory", "main")];
        let plan = plan_for(&federated(), &mounts, None, "t").unwrap();
        assert_eq!(plan.mounts.len(), 1);
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
