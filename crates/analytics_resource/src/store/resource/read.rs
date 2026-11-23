//! 单行读取、按本体路径反查与文件型清单。

use super::*;

impl AnalyticsResourceStore {
    pub async fn get_resource_by_id(&self, id: &str) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        Self::get_resource_by_id_on(conn.inner()?, id)
    }

    /// 列出**全部存活资产**，每个名字只留**最新版本**（按名字升序）。
    ///
    /// 给「数据对象」这类**只想读**的调用方用：资产参与分析只取最新版本（要用旧数据
    /// 先在资产模块回退），所以这里不做版本枚举。多版本同名的行按 `version` 降序取首行
    /// 去重——不用 `MAX(version)` 子查询是为了避免把「同名不同 type」的判定写进 SQL，
    /// 那属于登记层的语义，不该在清单层再抄一遍。
    pub async fn list_alive_resources(&self) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE deleted_at IS NULL ORDER BY name ASC, version DESC"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;
        let rows = stmt
            .query_map([], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        let all: Vec<AnalyticsResource> = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| persistence_err("select", e))?;

        // 同名多版本：排好序后每个名字只留第一行（= 最新版本）
        let mut out: Vec<AnalyticsResource> = Vec::with_capacity(all.len());
        let mut seen: Vec<String> = Vec::new();
        for row in all {
            if seen.iter().any(|name| name == &row.name) {
                continue;
            }
            seen.push(row.name.clone());
            out.push(row);
        }
        Ok(out)
    }

    /// 同名链上的**当前行**（未删除的最新一版；`None` = 这个名字还没有资产）。
    ///
    /// 「升级为资产」靠它判断是首次登记还是接续同名链的版本（`table_asset`）；
    /// 与 `list_alive_resources` 的去重口径一致：只看未删除行。
    pub async fn latest_alive_by_name(
        &self,
        name: &str,
    ) -> Result<Option<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE name = ?1 AND deleted_at IS NULL ORDER BY version DESC LIMIT 1"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;
        let mut rows = stmt
            .query_map(rusqlite::params![name], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        match rows.next() {
            Some(row) => Ok(Some(row.map_err(|e| persistence_err("select", e))?)),
            None => Ok(None),
        }
    }

    /// 按**本体表名**找当前版本的那条资产行（「登记为资产」的幂等判据）。
    ///
    /// 一张表只该有一条资产（`append_table_version` 改的是同一行的 `analytics_table`），
    /// 所以命中即"这张表已有身份"，不该再登记一次。若有历史脏数据同表两个名字，
    /// 按版本降序取最新那条。
    pub async fn latest_alive_by_analytics_table(
        &self,
        table: &str,
    ) -> Result<Option<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE analytics_table = ?1 AND deleted_at IS NULL ORDER BY version DESC LIMIT 1"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;
        let mut rows = stmt
            .query_map(rusqlite::params![table], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        match rows.next() {
            Some(row) => Ok(Some(row.map_err(|e| persistence_err("select", e))?)),
            None => Ok(None),
        }
    }

    /// 单连接版本的读取：供"读 → 写"必须在同一事务内完成的路径复用。
    ///
    /// v1 的 `update_resource` 先占一条连接，调 `get_resource_by_id` 又占一条，存快照再占一条
    /// （池只有 3 条）——一次逻辑操作最坏同时持两条连接，并发时会互相等对方归还。
    pub(crate) fn get_resource_by_id_on(
        conn: &rusqlite::Connection,
        id: &str,
    ) -> Result<AnalyticsResource, CoreError> {
        let sql = format!("SELECT {RESOURCE_COLUMNS} FROM analytics_resources WHERE id = ?");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;
        let resource = stmt
            .query_row(rusqlite::params![id], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        Ok(resource)
    }

    /// 按本体路径查存档（索引 020 的局部唯一索引保证至多一条）。
    ///
    /// 两个用途：归档前的目标占用检测；索引修复里的"有记录无本体"检测。
    pub async fn find_archive_by_rel_path(
        &self,
        rel_path: &str,
    ) -> Result<Option<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE file_rel_path = ? AND deleted_at IS NULL LIMIT 1"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;

        match stmt.query_row(rusqlite::params![rel_path], map_resource_row) {
            Ok(resource) => Ok(Some(resource)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(persistence_err("select", e)),
        }
    }

    /// 列出全部"有本体路径"的存活存档（索引修复的比对基准；按本体路径升序）。
    pub async fn list_file_archives(&self) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE file_rel_path IS NOT NULL AND deleted_at IS NULL ORDER BY file_rel_path ASC"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;
        let rows = stmt
            .query_map([], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| persistence_err("select", e))
    }
}
