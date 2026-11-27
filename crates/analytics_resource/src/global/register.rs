//! 项目表 / 文件登记与全局登记。

use super::*;
use crate::model::GLOBAL_PREFIX;

impl GlobalInstanceStore {
    /// 项目登记：Analysis 档（建表分发）。
    pub(super) async fn register_project_table(
        &self,
        project: &ProjectDuckConn,
        name: &str,
        global_id: &str,
        global_version: i32,
        content_hash: Option<&str>,
        definition_sql: Option<&str>,
        row_count: Option<i32>,
        column_count: Option<i32>,
    ) -> Result<DistributeOutcome, CoreError> {
        let pool = project.1.clone();
        let conn = pool.acquire().await?;
        let conn = conn.inner()?;

        // 版本号按**同名链**接续：查询不过滤 `deleted_at`——覆盖路径会先软删旧登记，
        // 若只看未删行，"用新内容替换同名资源"就成了"第一次分发"（版本退回 v1、历史断开）。
        let next_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM analytics_resources WHERE name = ?1 AND scope = 'project'",
                rusqlite::params![name],
                |row| row.get::<_, i32>(0),
            )
            .map_err(sqlite_err("next_project_version"))? + 1;

        let id = format!("arp_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        // 来源链（全局 → B）：只记直接上游那一跳，B 自己的版本是 `next_version`。
        let origin_ref = crate::model::global_ref(global_id, global_version);
        conn.execute(
            r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count,
                file_size, version, parent_version_id, parent_resource_id, source_query,
                created_at, updated_at, created_by, deleted_at,
                kind, content_hash, file_rel_path, readonly, promoted_from,
                source_connection_id, source_table, definition_sql, archived_at,
                payload_mode, source_path, origin_kind, origin_ref, analytics_table,
                structured
            ) VALUES (
                ?1, 'analysis', ?2, NULL, 'null', 'project', ?3, ?4, NULL,
                ?5, NULL, NULL, NULL, ?6, ?6, NULL, NULL,
                'analysis', ?7, NULL, 1, ?8, NULL, ?2, ?9, ?6,
                'copy', NULL, 'global', ?10, ?2, 1
            )
            "#,
            rusqlite::params![
                &id,
                name,
                row_count,
                column_count,
                next_version,
                now,
                content_hash,
                format!("{GLOBAL_PREFIX}{global_id}"),
                definition_sql,
                &origin_ref,
            ],
        )
        .map_err(sqlite_err("insert_project_table"))?;

        // 本体表名跟着登记行一起写（`analytics_table = ?2`）：不写的话这一行会被
        // `ArchiveKind::resolve` 读成「归档文件」，B 端双击打不开、查看统计也没入口——
        // 而本体明明就在项目分析库里。`?2` 就是刚建出来的那张表（改名分发时是避让后的名字）。
        //
        // 项目版本快照（版本 = 接续项目内同名链；快照记全局来源，回推"这次分发的是哪一版全局"）。
        let snapshot = serde_json::json!({
            "kind": "analysis",
            "analytics_table": name,
            "structured": 1,
            "distributed_from": format!("{GLOBAL_PREFIX}{global_id}"),
            "global_version": global_version,
            "content_hash": content_hash,
            "row_count": row_count,
            "column_count": column_count,
            "distributed_at": now,
        })
        .to_string();
        conn.execute(
            "INSERT INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![format!("arvp_{}", uuid::Uuid::new_v4().simple()), &id, next_version, snapshot, now],
        )
        .map_err(sqlite_err("insert_project_version"))?;

        Ok(DistributeOutcome {
            resource_id: id,
            name: name.to_string(),
            version: next_version,
            shape: DistributeShape::Table,
            row_count,
        })
    }

    /// 项目登记：File 档（脚本分发重建文件）。
    pub(super) async fn register_project_file(
        &self,
        project: &ProjectDuckConn,
        name: &str,
        rel_path: &str,
        global_id: &str,
        global_version: i32,
        content_hash: Option<&str>,
        file_size: Option<i64>,
    ) -> Result<DistributeOutcome, CoreError> {
        let pool = project.1.clone();
        let conn = pool.acquire().await?;
        let conn = conn.inner()?;

        // 同名链接续（不过滤 `deleted_at`，理由同 `register_project_table`）。
        let next_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM analytics_resources WHERE name = ?1 AND scope = 'project'",
                rusqlite::params![name],
                |row| row.get::<_, i32>(0),
            )
            .map_err(sqlite_err("next_project_version"))? + 1;

        let id = format!("arp_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        // 来源链（全局 → B）：只记直接上游那一跳，B 自己的版本是 `next_version`。
        let origin_ref = crate::model::global_ref(global_id, global_version);
        conn.execute(
            r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count,
                file_size, version, parent_version_id, parent_resource_id, source_query,
                created_at, updated_at, created_by, deleted_at,
                kind, content_hash, file_rel_path, readonly, promoted_from,
                source_connection_id, source_table, definition_sql, archived_at,
                payload_mode, source_path, origin_kind, origin_ref
            ) VALUES (
                ?1, 'file', ?2, NULL, 'null', 'project', NULL, NULL, ?3,
                ?4, NULL, NULL, NULL, ?5, ?5, NULL, NULL,
                'file', ?6, ?7, 1, ?8, NULL, NULL, NULL, ?5,
                'copy', NULL, 'global', ?9
            )
            "#,
            rusqlite::params![
                &id,
                name,
                file_size,
                next_version,
                now,
                content_hash,
                rel_path,
                format!("{GLOBAL_PREFIX}{global_id}"),
                &origin_ref,
            ],
        )
        .map_err(sqlite_err("insert_project_file"))?;

        let snapshot = serde_json::json!({
            "kind": "file",
            "distributed_from": format!("{GLOBAL_PREFIX}{global_id}"),
            "global_version": global_version,
            "content_hash": content_hash,
            "rel_path": rel_path,
            "distributed_at": now,
        })
        .to_string();
        conn.execute(
            "INSERT INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![format!("arvp_{}", uuid::Uuid::new_v4().simple()), &id, next_version, snapshot, now],
        )
        .map_err(sqlite_err("insert_project_version"))?;

        Ok(DistributeOutcome {
            resource_id: id,
            name: name.to_string(),
            version: next_version,
            shape: DistributeShape::File,
            row_count: None,
        })
    }

    // ==================== 内部 ====================
}
