//! 分析内核 · 全局**分析库**作为会话源（刀 7 方案 B）。
//!
//! 口径（用户口径 + 实测，2026-11-15）：**分析库一律 DuckDB，其余一律 SQLite**。
//! 所以「全局表」在 `<RDS_HOME>/data/system/analytics.duckdb` 里；同目录的 `global.db`
//! 是 SQLite 元数据库（连接 / 资源归档 / 规则信任），递错了地方就查不到可分析的表。
//!
//! 为什么不把全局分析库 ATTACH 进进程内存单例（方案 A）：这台库有**自己的写入方**
//! （全局持久表的落库路径与项目侧同形），而 DuckDB 在 Windows 上对同一个文件只允许
//! 一个连接句柄（`store/src/persistence/project_db.rs:276-278` 原文）。单例是进程级长命
//! 对象，让它持有全局分析库句柄的那一刻起，写入方就会撞
//! 「另一个程序正在使用此文件」——这条失败模式已在 `analysis_db_exclusive_probe` 里真跑过。
//!
//! 所以这里走 B：**短命会话 + 只读挂载**。会话按 owner 建，用完即弃；全局分析库只在这次
//! 分析期间被读，不改变它的生命周期。
//!
//! # 边界
//!
//! - 只读：全局分析库在本会话里不可写（这是对的——写入走它自己的 manager）；
//! - 撞锁仍是可能的：会话建立那一刻若该库正好被某个写入方持着，DuckDB 会拒绝挂载。
//!   这不是「可以忽略的小概率」，而是**必须给可读原因**的常态之一（见 [`open_mounted`] 的错误包装）；
//! - 别名固定 [`GLOBAL_ALIAS`]，SQL 里写 `rds_global.<表>`。

use std::path::{Path, PathBuf};

use shared::error::{CommonError, CoreError};

use crate::duckdb::federation::registry::FederatedSource;

use super::channel::AnalysisChannel;
use super::kernel::{self, PreparedMounts, SessionHandle};
use super::mount::{plan_for, MountSpec};

/// 全局分析库在会话 SQL 里的别名（固定，不让入口自己起名——两处各算一套就会对不上）
pub const GLOBAL_ALIAS: &str = "rds_global";

/// 全局**分析**库路径：`<RDS_HOME>/data/system/analytics.duckdb`。
///
/// 路径规则复用 `migration::get_system_dir()`（仓库里唯一的系统目录解析点），这里不另写一套。
/// 对照表（实测头字节）：
///
/// | 库 | 路径 | 格式 | 用途 |
/// | --- | --- | --- | --- |
/// | 全局分析库 | `data/system/analytics.duckdb` | DuckDB（`94 2d 89 5a`） | 全局表（可分析数据） |
/// | 全局元数据库 | `data/system/global.db` | SQLite（`SQLite format 3`） | 连接 / 资源归档 / 规则信任 |
/// | 项目分析库 | `<项目>/.RSmeta/analytics.duckdb` | DuckDB | 项目持久表 |
/// | 项目元数据库 | `<项目>/.RSmeta/project.db` | SQLite | 连接 / 资源归档 |
pub fn global_analysis_db_path() -> Result<PathBuf, CoreError> {
    let dir = crate::migration::get_system_dir()
        .map_err(|e| invalid(format!("解析系统目录失败（RDS_HOME 是否设置？）: {e}")))?;
    Ok(dir.join("analytics.duckdb"))
}

/// 把任意一个库文件**只读**挂进一条短命联邦会话，返回会话句柄。
///
/// 单独抽出来是为了可测试：真全局库路径依赖 `RDS_HOME`，用例里用临时文件就能把
/// 「只读挂载 + 能查到表 + 撞锁可读」三件事钉住。
///
/// `db_type` 是**驱动标识**（不是扩展名猜的）：`global.db` 实测是 SQLite 文件
/// （头部 `SQLite format 3`），所以要按 `sqlite` 挂；临时 duckdb 文件用例按 `duckdb` 挂。
pub fn open_mounted(
    owner: &str,
    path: &Path,
    alias: &str,
    db_type: &str,
) -> Result<SessionHandle, CoreError> {
    if !path.exists() {
        return Err(invalid(format!(
            "要挂载的库不存在：{}",
            path.display()
        )));
    }
    let spec = MountSpec::duckdb_file(path);
    let plan = plan_for(
        &AnalysisChannel::Federated {
            owner: owner.to_string(),
        },
        std::slice::from_ref(&spec),
        None,
        alias,
    )?;
    let url = path.to_string_lossy().to_string();
    // 组装源失败是**配置错**（别名非法 / 驱动不认 / URL 空），照原样报；
    // 挂载失败才是运行期问题（撞锁最常见），需要换成用户能懂的说法。
    let source = FederatedSource::new(&format!("C_{alias}"), alias, db_type, &url)
        .map_err(|e| invalid(format!("组装 {} 源失败: {e}", db_type)))?;
    let prepared = PreparedMounts::federated(vec![(spec, source)]);
    let session = kernel::open(&plan, &prepared).map_err(|e| {
        let text = e.to_string();
        if text.contains("另一个程序正在使用此文件") || text.contains("already open") {
            invalid(format!("库正被其他操作占用（可能正在被写入）: {text}"))
        } else {
            invalid(format!("挂载库失败: {text}"))
        }
    })?;
    // **必须核对真的挂上了**：联邦会话的既有约定是「挂不上不阻断」（记下原话继续挂下一个），
    // 所以 `open` 成功**不等于** alias 真的存在。实测踩过：SQLite 文件按 duckdb 驱动去挂，
    // open 返回 Ok，随后查询报 `schema "rds_global" does not exist`——错误位置离原因太远。
    ensure_alias_attached(&session, alias, db_type)?;
    Ok(session)
}

/// 核对 alias 真的出现在 catalog 列表里；没出现就把「为什么」尽量说清。
///
/// 「为什么」直接从会话快照里取**原话**（`MountState::Failed`），不再另查一遍、也不翻译：
/// 挂载失败的原因只有挂载那一处知道，第二处转述只会把它变得更模糊。
fn ensure_alias_attached(
    session: &SessionHandle,
    alias: &str,
    db_type: &str,
) -> Result<(), CoreError> {
    let attached = session.run(&format!(
        "SELECT count(*) AS n FROM duckdb_databases() WHERE database_name = '{alias}'"
    ));
    let mounted = match attached {
        Ok(result) => result
            .rows
            .first()
            .and_then(|row| row.first())
            .map(|cell| match cell {
                shared::models::Value::Int(n) => *n,
                shared::models::Value::Text(text) => text.trim().parse::<i64>().unwrap_or(0),
                _ => 0,
            })
            .unwrap_or(0),
        Err(error) => {
            return Err(invalid(format!(
                "核对挂载结果失败（连 duckdb_databases() 都查不了）: {error}"
            )))
        }
    };
    if mounted > 0 {
        return Ok(());
    }
    let reason = match session {
        SessionHandle::Federated(federated) => federated
            .snapshot()
            .sources
            .iter()
            .find(|entry| entry.source.alias == alias)
            .map(|entry| match &entry.state {
                crate::duckdb::federation::registry::MountState::Failed(text) => text.clone(),
                crate::duckdb::federation::registry::MountState::Ready { tables } => {
                    format!("状态 Ready（{tables} 张表），但 catalog 里查不到")
                }
            })
            .unwrap_or_else(|| "会话里没有这条源".to_string()),
        _ => "这条句柄不是联邦会话".to_string(),
    };
    Err(invalid(format!(
        "按 {db_type} 挂载 {alias} 失败：{reason}"
    )))
}

/// 打开全局分析库会话（刀 7 · B1）：读 `<RDS_HOME>/data/system/analytics.duckdb`，
/// 别名 [`GLOBAL_ALIAS`]，驱动 **duckdb**（分析库一律 DuckDB）。
///
/// 曾经挂错过：首版按 `global.db` + sqlite 挂，那是元数据库（SQLite），里面没有可分析的表；
/// 代价是「open 成功、随后查询报 schema/表不存在」——所以 `open_mounted` 里那道
/// 「挂上之后核 catalog」的检查留着，不许删。
pub fn open_global_session(owner: &str) -> Result<SessionHandle, CoreError> {
    let path = global_analysis_db_path()?;
    if !path.exists() {
        return Err(invalid(format!(
            "全局分析库未初始化：{}（宿主启动流程还没建它）",
            path.display()
        )));
    }
    open_mounted(owner, &path, GLOBAL_ALIAS, "duckdb")
}

fn invalid(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

#[cfg(test)]
mod tests {
    use super::open_mounted;

    #[test]
    fn missing_file_is_a_readable_error_not_a_panic() {
        let path = std::env::temp_dir().join("rds_global_missing_probe.duckdb");
        let _ = std::fs::remove_file(&path);
        let err = open_mounted("C_global_probe", &path, "rds_global", "duckdb").unwrap_err();
        assert!(err.to_string().contains("不存在"), "{err}");
    }

    #[test]
    fn invalid_alias_is_rejected_before_any_mount() {
        // 别名会进 SQL（`<别名>.<表>`），非法别名必须在挂载前拒
        let dir = std::env::temp_dir().join("rds_global_alias_probe");
        std::fs::create_dir_all(&dir).expect("目录");
        let path = dir.join("g.duckdb");
        duckdb::Connection::open(&path).expect("建库");
        let err = open_mounted("C_global_probe", &path, "bad alias", "duckdb").unwrap_err();
        assert!(format!("{err}").contains("别名") || format!("{err}").contains("清"), "{err}");
    }
}
