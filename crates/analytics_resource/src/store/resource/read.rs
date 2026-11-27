//! 单行读取、按本体路径反查与文件型清单。

use super::*;

impl AnalyticsResourceStore {
    pub async fn get_resource_by_id(&self, id: &str) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        Self::get_resource_by_id_on(conn.inner()?, id)
    }

    /// 列出**全部存活资产行**（按 id 身份，每个 head 一行；不再按显示名去重）。
    ///
    /// 刷新链路与数据对象都要看见同类资产：文件、表、`table_ref` 各自有独立身份，
    /// 按 name 去重会把同名但不同 id 的资产吞掉（表行没有 `file_rel_path`，更不会
    /// 出现在 [`Self::list_file_archives`] 里）。版本枚举在各自的版本表里做，
    /// 这一层只回答“当前活着哪些资产”。
    pub async fn list_alive_resources(&self) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE deleted_at IS NULL ORDER BY name ASC, id ASC"
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

    /// 同名链上的**当前行**（未删除的最新一版；`None` = 这个名字还没有资产）。
    ///
    /// 「升级为资产」靠它判断是首次登记还是接续同名链的版本（`table_asset`）。
    /// 这是**按显示名选链**的兼容入口：同名但不同 id 的资产仍可能并存，
    /// 新代码不应再把它当作身份判定（身份一律用 id）。
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

    /// 某显示名下的**全部存活行**（按版本降序）。
    ///
    /// 身份是 id，不是名字：存量里可能因历史操作留下同名行，服务层做“新名字唯一”
    /// 检查与选链时都要看到全部候选，不能只拿最新一行假装没有歧义。
    pub async fn alive_by_name(&self, name: &str) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE name = ?1 AND deleted_at IS NULL ORDER BY version DESC, id ASC"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;
        let rows = stmt
            .query_map(rusqlite::params![name], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| persistence_err("select", e))
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
