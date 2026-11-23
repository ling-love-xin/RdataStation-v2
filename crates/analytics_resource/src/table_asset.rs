//! 「临时表 / 临时结果集 → 升级为资产」的登记闭环。
//!
//! 为什么需要这一层：引擎侧早就能把 `tmp_q_` / `tmp_i_` 搬成项目分析库里的持久表
//! （`workbench::services::persist_jobs`），但**只写表、不登记**——于是项目库里有一张
//! `res_<描述>_<时间戳>`，资产列表里却没有这一行，`version` 链、只读、回退、参与分析
//! 取最新版本全都无从谈起。
//!
//! 口径（用户已定）：
//! - 升级 = 把数据写成项目 `analytics.duckdb` 的持久表，**不是**导出文件再当资产；
//! - 资产只读、名字**不带前缀**（`res_` 是分析库内部标识，不外露）、有版本；
//! - 一版一张表：登记行只记**当前版本**的表名，旧表名留在版本快照里；
//!   回退 = 把快照里的表名指回来（两张表都在，不覆盖不删除）；
//! - 同名再升级 → 版本接续（`version = n+1`），不另起一行。

use std::path::Path;
use std::sync::Arc;

use sha2::{Digest as _, Sha256};

use engine::persistence::ProjectSqlitePool;
use shared::error::{CommonError, CoreError, StorageError};

use crate::model::{ArchiveBinding, ArchiveKind, NewArchiveInput, PayloadMode, TimelineEventKind};
use crate::{AnalyticsResource, AnalyticsResourceStore};

/// 项目库位置（与 `engine::migration` 的建库位置同一处）。
const PROJECT_META_DB: &str = ".RSmeta/project.db";

/// 一次登记请求：本体已经在项目分析库里，这里只负责登记。
#[derive(Debug, Clone)]
pub struct TableAssetRequest {
    /// 显示名（**不带 `res_` 前缀**；同名链按它接续版本）。
    pub name: String,
    /// 本体的实际表名（`res_<描述>_<时间戳>`）。
    pub analytics_table: String,
    /// 行列数（可空：拿不到就给 `None`，不假装 0）。
    pub row_count: Option<i32>,
    pub column_count: Option<i32>,
}

/// 登记结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableAssetOutcome {
    pub resource_id: String,
    pub name: String,
    /// 本次登记后的版本号（首次 = 1，同名再升级 = n+1）。
    pub version: i32,
    /// `true` = 接续了同名链（产生新版本）；`false` = 首次登记。
    pub created_new_version: bool,
}

impl TableAssetOutcome {
    /// 状态栏回执（宿主只有一句位置，不各自拼）。
    pub fn notice(&self) -> String {
        format!("已升级为资产「{}」v{}", self.name, self.version)
    }
}

/// 表型资产的内容身份：**表名**的 sha256。
///
/// 表型资产的「内容」就是那张表；一版一张表，表名即版本身份。不用文件字节指纹、
/// 也不用定义指纹（`definition_sql` 对表型是空的）——拿文件口径去套，只会让
/// 「内容已变」误报。
///
/// 回退也用它：把表名指回旧表后，指纹自然跟着旧表名，不需要真去比数据。
pub(crate) fn table_fingerprint(table: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"analytics_table:");
    hasher.update(table.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 「登记为资产」的两种结果（对**已存在的表**，不搬数据、不涨版本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExistingTableOutcome {
    /// 这张表原先没有身份，本次登记成 v1。
    Registered(TableAssetOutcome),
    /// 这张表已经是某条资产的当前版本：不造第二条，直接把那条交回去（口径：定位而非重复登记）。
    AlreadyCurrent(TableAssetOutcome),
}

impl ExistingTableOutcome {
    pub fn outcome(&self) -> &TableAssetOutcome {
        match self {
            Self::Registered(outcome) | Self::AlreadyCurrent(outcome) => outcome,
        }
    }

    /// 状态栏回执：两种结果说法不同（"已登记" vs "已登记过，带你去看"）。
    pub fn notice(&self) -> String {
        match self {
            Self::Registered(outcome) => format!("已登记为资产「{}」v{}", outcome.name, outcome.version),
            Self::AlreadyCurrent(outcome) => format!(
                "「{}」已是资产 v{} 的当前版本，没有重复登记",
                outcome.name, outcome.version
            ),
        }
    }
}

/// 登记一张**已经在项目分析库里**的表（数据对象面板「登记为资产」）。
///
/// 与 [`register`] 的分工：`register` 服务「升级」（数据刚被搬成新表，可能是同名链的
/// 新版本）；本函数服务「这张表本来就在库里、只是没有资产身份」——所以它**不改版本、
/// 不写快照、不搬数据**，命中已登记的表就原样返回那条资产。
pub async fn register_existing(
    store: &AnalyticsResourceStore,
    request: TableAssetRequest,
) -> Result<ExistingTableOutcome, CoreError> {
    let name = request.name.trim().to_string();
    let table = request.analytics_table.trim().to_string();
    if name.is_empty() {
        return Err(CoreError::common(CommonError::General(
            "登记为资产需要显示名".to_string(),
        )));
    }
    if table.is_empty() {
        return Err(CoreError::common(CommonError::General(
            "登记为资产需要本体表名（项目分析库里的那张表）".to_string(),
        )));
    }
    // 幂等：这张表已经有身份 → 交回那条资产，不造第二条、不涨版本。
    if let Some(row) = store.latest_alive_by_analytics_table(&table).await? {
        return Ok(ExistingTableOutcome::AlreadyCurrent(TableAssetOutcome {
            resource_id: row.id,
            name: row.name,
            version: row.version,
            created_new_version: false,
        }));
    }
    let content_hash = table_fingerprint(&table);
    let input = NewArchiveInput {
        resource_type: "analysis".to_string(),
        name: name.clone(),
        alias: None,
        kind: ArchiveKind::AnalyticsTable,
        content_hash: content_hash.clone(),
        definition_sql: None,
        row_count: request.row_count,
        column_count: request.column_count,
        // 表型资产没有 `resources/` 下的本体文件（写库侧会把空串转成 NULL）
        file_rel_path: String::new(),
        payload_mode: PayloadMode::Copy,
        source_path: None,
        file_size: None,
        structured: true,
        binding: ArchiveBinding {
            promoted_from: None,
            source_connection_id: None,
            source_table: Some(table.clone()),
        },
        scope: "project".to_string(),
    };
    let detail = format!("登记项目分析库里已有的表 {table}");
    let resource = store
        .insert_table_archive_with_event(input, &table, Some(&detail))
        .await?;
    Ok(ExistingTableOutcome::Registered(outcome(resource, false)))
}

/// 宿主入口（数据对象面板「登记为资产」）：按项目根开库后登记。
pub async fn register_existing_at(
    project_root: &Path,
    request: TableAssetRequest,
) -> Result<ExistingTableOutcome, CoreError> {
    let db_path = project_root.join(PROJECT_META_DB);
    let pool = Arc::new(ProjectSqlitePool::new(db_path, 2).await?);
    let store = AnalyticsResourceStore::new(pool);
    register_existing(&store, request).await
}

/// 在**给定 store** 上登记（测试与宿主共用；`register_at` 只是它的一层开库封装）。
pub async fn register(
    store: &AnalyticsResourceStore,
    request: TableAssetRequest,
) -> Result<TableAssetOutcome, CoreError> {
    let name = request.name.trim().to_string();
    let table = request.analytics_table.trim().to_string();
    if name.is_empty() {
        return Err(CoreError::common(CommonError::General(
            "升级为资产需要显示名（结果集展示名）".to_string(),
        )));
    }
    if table.is_empty() {
        return Err(CoreError::common(CommonError::General(
            "升级为资产需要本体表名（项目分析库里的那张表）".to_string(),
        )));
    }
    let content_hash = table_fingerprint(&table);

    match store.latest_alive_by_name(&name).await? {
        Some(row) => {
            // 同名再升级：旧版本整行进快照（与文件型同一形态），登记行改指新表。
            // 存整行而不是只存表名：版本历史/详情都按 `AnalyticsResource` 解析快照，
            // 只存表名会让历史版本行显示成"没内容"，回退也读不出该指哪张表。
            let snapshot = serde_json::to_string(&row).map_err(|error| {
                CoreError::storage(StorageError::Serialization {
                    format: "JSON".to_string(),
                    reason: error.to_string(),
                })
            })?;
            let detail = format!("升级结果集，本体改指 {table}");
            let resource = store
                .append_table_version(
                    &row.id,
                    row.version,
                    &table,
                    &snapshot,
                    &content_hash,
                    TimelineEventKind::Updated,
                    Some(&detail),
                )
                .await?;
            Ok(outcome(resource, true))
        }
        None => {
            let input = NewArchiveInput {
                resource_type: "analysis".to_string(),
                name: name.clone(),
                alias: None,
                kind: ArchiveKind::AnalyticsTable,
                content_hash: content_hash.clone(),
                definition_sql: None,
                row_count: request.row_count,
                column_count: request.column_count,
                // 表型资产没有 `resources/` 下的本体文件（写库侧会把空串转成 NULL）
                file_rel_path: String::new(),
                payload_mode: PayloadMode::Copy,
                source_path: None,
                file_size: None,
                structured: true,
                binding: ArchiveBinding {
                    promoted_from: None,
                    source_connection_id: None,
                    // 来源表 = 结果集在内存里的那张表（出处凭证只记「数据从哪来」）
                    source_table: Some(table.clone()),
                },
                scope: "project".to_string(),
            };
            let detail = format!("升级结果集为资产，本体 {table}");
            let resource = store
                .insert_table_archive_with_event(input, &table, Some(&detail))
                .await?;
            Ok(outcome(resource, false))
        }
    }
}

/// 从版本快照里取本体表名（回退要靠它）。
///
/// 两种写法都要认：新写法是整行 `AnalyticsResource` 序列化；2026-11-23 之前那条
/// 短快照只记了 `{"analytics_table": ...}`，老数据不能因为换了形态就回退不了。
pub(crate) fn snapshot_table(snapshot: &serde_json::Value) -> Option<String> {
    if let Ok(record) = serde_json::from_value::<AnalyticsResource>(snapshot.clone()) {
        if let Some(table) = record.analytics_table.filter(|table| !table.is_empty()) {
            return Some(table);
        }
    }
    snapshot
        .get("analytics_table")
        .and_then(|value| value.as_str())
        .filter(|table| !table.is_empty())
        .map(str::to_string)
}

/// 宿主入口：按项目根打开项目库后登记。
///
/// 项目库**必须已迁移**（生产由应用启动完成；测试用 `create_test_store` + `register`）。
/// 这里不自行跑迁移：迁移会重建表，不该在用户点「升级为资产」的路径上发生。
pub async fn register_at(
    project_root: &Path,
    request: TableAssetRequest,
) -> Result<TableAssetOutcome, CoreError> {
    let db_path = project_root.join(PROJECT_META_DB);
    let pool = Arc::new(ProjectSqlitePool::new(db_path, 2).await?);
    let store = AnalyticsResourceStore::new(pool);
    register(&store, request).await
}

fn outcome(resource: AnalyticsResource, created_new_version: bool) -> TableAssetOutcome {
    TableAssetOutcome {
        resource_id: resource.id,
        name: resource.name,
        version: resource.version,
        created_new_version,
    }
}
