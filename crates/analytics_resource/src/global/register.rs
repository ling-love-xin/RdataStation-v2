//! 项目表 / 文件登记与全局登记。

use super::*;

impl GlobalInstanceStore {
    /// 读全局脚本内容表首行（分发重建文件用）。
    pub(super) async fn read_script_content(&self, name: &str) -> Result<String, CoreError> {
        self.with_duckdb(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT \"content\" FROM {} LIMIT 1;",
                    quote_ident(name)
                ))
                .map_err(|e| duckdb_err("prepare_read_script", &e.to_string()))?;
            stmt.query_row([], |row| row.get::<_, String>(0))
                .map_err(|e| duckdb_err("read_script_content", &e.to_string()))
        })
        .await
    }

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
        // 与全局侧 `register_global` 同口径（2026-11-07 统一）。
        let next_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM analytics_resources WHERE name = ?1 AND scope = 'project'",
                rusqlite::params![name],
                |row| row.get::<_, i32>(0),
            )
            .map_err(sqlite_err("next_project_version"))? + 1;

        let id = format!("arp_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count,
                file_size, version, parent_version_id, parent_resource_id, source_query,
                created_at, updated_at, created_by, deleted_at,
                kind, content_hash, file_rel_path, readonly, promoted_from,
                source_connection_id, source_table, definition_sql, archived_at,
                payload_mode, source_path
            ) VALUES (
                ?1, 'analysis', ?2, NULL, 'null', 'project', ?3, ?4, NULL,
                ?5, NULL, NULL, NULL, ?6, ?6, NULL, NULL,
                'analysis', ?7, NULL, 1, ?8, NULL, ?2, ?9, ?6,
                'copy', NULL
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
                format!("global:{}", global_id),
                definition_sql,
            ],
        )
        .map_err(sqlite_err("insert_project_table"))?;

        // 项目版本快照（版本 = 接续项目内同名链；快照记全局来源，回推"这次分发的是哪一版全局"）。
        let snapshot = serde_json::json!({
            "kind": "analysis",
            "distributed_from": format!("global:{}", global_id),
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
        conn.execute(
            r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count,
                file_size, version, parent_version_id, parent_resource_id, source_query,
                created_at, updated_at, created_by, deleted_at,
                kind, content_hash, file_rel_path, readonly, promoted_from,
                source_connection_id, source_table, definition_sql, archived_at,
                payload_mode, source_path
            ) VALUES (
                ?1, 'file', ?2, NULL, 'null', 'project', NULL, NULL, ?3,
                ?4, NULL, NULL, NULL, ?5, ?5, NULL, NULL,
                'file', ?6, ?7, 1, ?8, NULL, NULL, NULL, ?5,
                'copy', NULL
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
                format!("global:{}", global_id),
            ],
        )
        .map_err(sqlite_err("insert_project_file"))?;

        let snapshot = serde_json::json!({
            "kind": "file",
            "distributed_from": format!("global:{}", global_id),
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

    /// 全局登记 + 版本快照（升级落点）。
    ///
    /// 版本号取**同名（`scope = 'global'`）的最大值 + 1**，不过滤 `deleted_at`：
    /// 覆盖路径先软删旧登记，若只看未删行，覆盖就退化成"从 v1 重开"（版本链断掉）。
    /// 登记 id 每次都换新（`arg_<uuid>`）——版本链靠 `name` + `version` 串，不靠 id。
    pub(super) fn register_global(
        &self,
        name: &str,
        req: &PromoteRequest,
        row_count: Option<i32>,
        column_count: Option<i32>,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        self.with_sqlite(|conn| {
            let next_version: i32 = conn
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM analytics_resources WHERE name = ?1 AND scope = 'global'",
                    rusqlite::params![name],
                    |row| row.get::<_, i32>(0),
                )
                .map_err(sqlite_err("next_global_version"))? + 1;
            let id = format!("arg_{}", uuid::Uuid::new_v4().simple());
            let now = Utc::now().to_rfc3339();
            // 数据文件 / 分析表：本体 = 全局表，重建定义 = 建表 SQL（可复现）。
            let definition_sql = match req.kind {
                PromoteKind::DataFile => file_read_expr(&req.source_abs_path)
                    .map(|expr| {
                        format!(
                            "CREATE TABLE {} AS SELECT * FROM {};",
                            quote_ident(name),
                            expr
                        )
                    }),
                PromoteKind::Script => Some(format!(
                    "内容表（方案 A，§13.5）：{}（filename / content / content_hash），分发时重建为 resources/{}",
                    quote_ident(name),
                    req.rel_path.as_deref().unwrap_or("")
                )),
            };

            conn.execute(
                r#"
                INSERT INTO analytics_resources (
                    id, resource_type, name, alias, config, scope, row_count, column_count,
                    file_size, version, parent_version_id, parent_resource_id, source_query,
                    created_at, updated_at, created_by, deleted_at,
                    kind, content_hash, file_rel_path, readonly, promoted_from,
                    source_connection_id, source_table, definition_sql, archived_at,
                    payload_mode, source_path
                ) VALUES (
                    ?1, ?2, ?3, NULL, 'null', 'global', ?4, ?5, ?6,
                    ?7, NULL, NULL, NULL, ?8, ?8, NULL, NULL,
                    'analysis', ?9, ?10, 1, NULL, NULL, NULL, ?11, ?8,
                    'copy', ?12
                )
                "#,
                rusqlite::params![
                    &id,
                    "analysis",
                    name,
                    row_count,
                    column_count,
                    req.file_size,
                    next_version,
                    now,
                    req.content_hash,
                    req.rel_path,
                    definition_sql,
                    req.source_abs_path.to_string_lossy().replace('\\', "/"),
                ],
            )
            .map_err(sqlite_err("insert_global"))?;

            // 全局版本快照（与项目侧 `save_resource_version_on` 同构）。
            let snapshot = serde_json::json!({
                "kind": "analysis",
                "content_hash": req.content_hash,
                "row_count": row_count,
                "column_count": column_count,
                "definition_sql": definition_sql,
                "source": req.source_abs_path.to_string_lossy().to_string(),
                "promoted_at": now,
            })
            .to_string();
            let version_id = format!("argv_{}", uuid::Uuid::new_v4().simple());
            conn.execute(
                "INSERT INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![&version_id, &id, next_version, snapshot, now],
            )
            .map_err(sqlite_err("insert_global_version"))?;

            Ok(PromoteOutcome {
                resource_id: id,
                name: name.to_string(),
                version: next_version,
                row_count,
                column_count,
                overwritten: false,
            })
        })
        .map_err(InstanceOpError::Core)
    }

}
