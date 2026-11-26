use super::*;
pub(crate) use crate::model::{
    ArchiveBinding, ArchiveKind, ArchiveRequest, ArchiveUndo, CheckoutRequest, PayloadMode,
};
pub(crate) use engine::persistence::ProjectSqlitePool;
pub(crate) use std::sync::Arc;
pub(crate) use tokio::fs;

// 项目库建表 SQL 统一从 `crate::store::schema_sql` 取：此前这里手抄了一份迁移清单，
// 加一个迁移要改六处（漏一处只在某个用例里以 no such column 冒出来），见该模块注释。

/// 测试服务：独立临时项目 + 空索引库。
pub(super) async fn test_service() -> (ArchiveService, PathBuf) {
    let dir = crate::tests::temp_project_dir("archive");
    let pool = Arc::new(
        ProjectSqlitePool::new(dir.join("project.db"), 2)
            .await
            .expect("create pool"),
    );
    {
        let conn = pool.acquire().await.expect("acquire");
        let inner = conn.inner().expect("inner");
        inner
            .execute_batch(crate::store::schema_sql::PROJECT_MIGRATIONS_SQL)
            .expect("run analytics_resources migrations");
    }
    // 全局标签库注入本测试独立路径（避免共享进程级隔离根 → 跨测试同名标签冲突）。
    let store = AnalyticsResourceStore::new_for_test(pool, dir.join("global.db"));
    (ArchiveService::new(&dir, store), dir)
}

pub(super) fn archive_req(source: &std::path::Path, rel: &str, existing: Option<&str>) -> ArchiveRequest {
    ArchiveRequest {
        source_path: source.to_path_buf(),
        rel_path: rel.to_string(),
        name: "dau_report".to_string(),
        alias: None,
        kind: ArchiveKind::File,
        // 大多数服务用例是文件型归档；分析表那条路见 `archive_analysis_req`。
        payload_mode: PayloadMode::Copy,
        analysis: None,
        binding: ArchiveBinding {
            promoted_from: Some("scratchpad/dau.sql".to_string()),
            source_connection_id: Some("conn_1".to_string()),
            source_table: None,
        },
        tags: Vec::new(),
        group_id: None,
        existing_resource_id: existing.map(str::to_string),
    }
}

async fn write_draft(dir: &std::path::Path, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).await.expect("write draft");
    path
}

mod archive;
mod checkout;
mod reference;
mod semantics;
mod timeline;
mod trash;
mod versions;

/// 构造 reference 归档请求：本体留原位、只登记元数据。
pub(super) fn reference_req(source: &std::path::Path, rel: &str) -> ArchiveRequest {
    let mut req = archive_req(source, rel, None);
    req.payload_mode = PayloadMode::Reference;
    req
}
