use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use duckdb::Connection;
use shared::error::{CommonError, CoreError};

use crate::sql::SqlEngine;

/// 内存库（`Connection::open_in_memory`）的 catalog 名（DuckDB 固定叫 `memory`）。
///
/// [`TempTableManager::list_by_source`] 用它把「属于本库的表」与 `ATTACH` 进来的文件库表分开，
/// 避免清理时误删文件库里的同名表。
const IN_MEMORY_CATALOG: &str = "memory";

/// DuckDB 错误 → `CoreError`（本模块只做临时表维度的错误包装）。
fn db_error(e: duckdb::Error) -> CoreError {
    CoreError::common(CommonError::General(format!("DuckDB error: {e}")))
}

/// 临时表来源枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TempTableSource {
    /// 查询结果转入分析 (q)
    Query,
    /// 洞察中间计算 (i)
    Insight,
    /// Mock 数据生成 (m)
    Mock,
    /// 插件临时数据 (p)
    Plugin,
}

impl TempTableSource {
    /// 获取来源缩写字母
    ///
    /// # 返回
    /// 单个字母缩写: q/i/m/p
    pub fn abbreviation(&self) -> &str {
        match self {
            TempTableSource::Query => "q",
            TempTableSource::Insight => "i",
            TempTableSource::Mock => "m",
            TempTableSource::Plugin => "p",
        }
    }

    /// 获取用户可见性
    ///
    /// # 返回
    /// true 表示用户可见，false 表示不可见
    pub fn is_user_visible(&self) -> bool {
        !matches!(self, TempTableSource::Insight)
    }

    /// 该来源的临时表**命名前缀**（清理时按这些前缀识别）。
    ///
    /// 两套命名共存：v2 规范名 `tmp_{缩写}_…`（[`TempTableManager::generate_name`] 产出），
    /// 以及 v1 沿用至今的 `temp_mock_…`（mock 保持 v1 表名是明确决策，见 mock 架构 D2）。
    /// 所以清理必须按**多前缀**匹配，只认一套会漏掉另一套。
    pub fn prefixes(&self) -> &'static [&'static str] {
        match self {
            TempTableSource::Query => &["tmp_q_"],
            TempTableSource::Insight => &["tmp_i_"],
            TempTableSource::Mock => &["tmp_m_", "temp_mock_"],
            TempTableSource::Plugin => &["tmp_p_"],
        }
    }
}

/// 临时表配置
///
/// 不同类型临时表的清理策略
pub struct TempTableConfig {
    /// TTL 时间（秒），None 表示无限制
    pub ttl_secs: Option<u64>,
    /// 数量上限，None 表示无限制
    pub max_count: Option<usize>,
}

impl TempTableConfig {
    /// 获取洞察中间表配置
    pub fn insight() -> Self {
        TempTableConfig {
            ttl_secs: Some(1800), // 30分钟
            max_count: Some(100),
        }
    }

    /// 获取查询结果表配置
    pub fn query() -> Self {
        TempTableConfig {
            ttl_secs: None,
            max_count: None,
        }
    }

    /// 获取 Mock 临时表配置
    pub fn mock() -> Self {
        TempTableConfig {
            ttl_secs: None,
            max_count: None,
        }
    }

    /// 获取插件临时表配置
    pub fn plugin() -> Self {
        TempTableConfig {
            ttl_secs: None,
            max_count: None,
        }
    }
}

/// 临时表登记的数量概览（按来源分档）。
///
/// 用途是**可观测**：登记表只增不减时（K16 的病症），除了这张概览没有别的可见信号。
/// 权威来源仍是库本身（[`TempTableManager::list_by_source`]），这里只是计数。
pub struct TempTableStats {
    pub query: usize,
    pub insight: usize,
    pub mock: usize,
    pub plugin: usize,
}

impl TempTableStats {
    /// 已认得的表合计（不含名字不带任何来源前缀的表）。
    pub fn total(&self) -> usize {
        self.query + self.insight + self.mock + self.plugin
    }
}

/// 生成一个**唯一**的临时表名：`tmp_{缩写}_{描述}_{紧凑时间戳}_{8 位随机}`。
///
/// 为什么需要随机尾巴：表名要内联进 SQL，而[`TempTableManager::generate_name`] 只到**秒**——
/// 同一秒建两张同描述的表会撞名（`CREATE TABLE` 直接失败）。查询结果表与洞察中间表都走这里。
///
/// 前缀由 [`TempTableSource::prefixes`] 决定：TTL / 上限 / 按来源清理全靠它识别，
/// 自己拼一个别的名字（历史上的 `rs_<uuid>`）等于把那些机制全关掉。
pub fn generate_unique_name(source: TempTableSource, description: &str) -> String {
    let prefix = source
        .prefixes()
        .first()
        .copied()
        .unwrap_or("tmp_x_");
    let desc = sanitize_description(description);
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    let uniq = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}{desc}_{stamp}_{}", &uniq[..8])
}

/// 描述里只留字母 / 数字 / 下划线（表名要内联进 SQL，虽有引号也不值得冒险）。
fn sanitize_description(description: &str) -> String {
    let cleaned: String = description
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "tmp".to_string()
    } else {
        trimmed
    }
}

/// 表名的 SQL 引用（`"name"`；内部双引号双写）。
pub(crate) fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 删除一张**属于指定来源**的临时表（前缀守卫），并同步登记表。
///
/// 与 [`TempTableManager::drop_by_source`] 的分工：本函数删**一张**（结果集被丢弃 /
/// 替换时只丢它自己那一张），那个删**一批**（项目切换 / 关闭时的清场）。
///
/// 幂等：表不存在也算成功；但名字**必须**落在该来源的前缀下——放开前缀就等于给了一把
/// 「传什么名字就删什么表」的刀。
pub fn drop_temp_table(
    conn: &Connection,
    source: TempTableSource,
    name: &str,
) -> Result<(), CoreError> {
    if !source.prefixes().iter().any(|p| name.starts_with(p)) {
        return Err(CoreError::common(CommonError::General(format!(
            "拒绝删除非本来源的临时表：{name}（{:?} 只允许这些前缀：{}）",
            source,
            source.prefixes().join(" / ")
        ))));
    }

    conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", quote_ident(name)))
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "删除临时表 {name} 失败: {e}"
            )))
        })?;
    // 登记表同步摘掉：留着会让「登记数」与库里的实际张数对不上
    super::manager::DuckDBManager::temp_table_manager().unregister(name);
    Ok(())
}

/// 临时表管理器
///
/// 负责临时表命名、TTL 清理、数量上限管理。
///
/// # 命名规则
/// 格式：`tmp_{来源缩写}_{描述}_{紧凑时间戳}`
///
/// # 清理规则
/// - 洞察中间表 (tmp_i_): TTL 30分钟，上限100，惰性清理
/// - 查询结果表 (tmp_q_): 无 TTL/上限，项目关闭清理
/// - Mock 临时表 (tmp_m_): 无 TTL/上限，项目关闭清理
/// - 插件临时表 (tmp_p_): 无 TTL/上限，插件卸载清理
pub struct TempTableManager {
    /// 临时表注册表: 表名 -> 创建时间
    registry: Arc<RwLock<HashMap<String, Instant>>>,
    /// 全局 DuckDB 临时表上限
    global_max_tables: usize,
}

impl TempTableManager {
    /// 创建新的临时表管理器。
    ///
    /// # 参数
    /// - `global_max_tables`: 全局 DuckDB 临时表上限（默认50）
    pub fn new(global_max_tables: usize) -> Self {
        TempTableManager {
            registry: Arc::new(RwLock::new(HashMap::new())),
            global_max_tables,
        }
    }

    /// 执行惰性清理：移除过期的临时表。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接（用于实际执行 DROP TABLE）
    ///
    /// # 返回
    /// 清理的表名列表
    ///
    /// # 注意
    /// 此方法应在调用方主动调用时执行清理，而不是使用后台线程
    pub fn perform_lazy_cleanup(&self, conn: &Connection) -> Vec<String> {
        let tables_to_drop = {
            let mut reg = self.registry.write().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let mut to_drop = Vec::new();

            // 检查洞察中间表 TTL
            let ttl = Duration::from_secs(1800); // 30分钟
            for (name, time) in reg.iter() {
                if name.starts_with("tmp_i_") && now.duration_since(*time) >= ttl {
                    to_drop.push(name.clone());
                }
            }

            // 从注册表中移除
            for name in &to_drop {
                reg.remove(name);
            }

            to_drop
        };

        // 实际执行 DROP TABLE
        if !tables_to_drop.is_empty() {
            tracing::info!(
                "[TempTableManager] 清理 {} 个过期临时表",
                tables_to_drop.len()
            );

            for table_name in &tables_to_drop {
                let sql = format!("DROP TABLE IF EXISTS {}", table_name);
                if let Err(e) = conn.execute(&sql, []) {
                    tracing::warn!("[TempTableManager] 清理临时表 {} 失败: {}", table_name, e);
                }
            }
        }

        tables_to_drop
    }

    /// 生成临时表名称。
    ///
    /// # 参数
    /// - `source`: 表来源类型
    /// - `description`: 描述标识
    ///
    /// # 返回
    /// 符合命名规则的临时表名
    ///
    /// # 示例
    /// ```rust,ignore
    /// let name = manager.generate_name(TempTableSource::Query, "orders");
    /// // 返回: tmp_q_orders_20260512143025
    /// ```
    pub fn generate_name(&self, source: TempTableSource, description: &str) -> String {
        let now = chrono::Local::now();
        let timestamp = now.format("%Y%m%d%H%M%S");
        format!(
            "tmp_{}_{}_{:04}",
            source.abbreviation(),
            description,
            timestamp
        )
    }

    /// 注册临时表。
    ///
    /// # 参数
    /// - `table_name`: 临时表名
    ///
    /// # 注意
    /// 注册时触发洞察中间表的惰性清理
    pub fn register(&self, table_name: &str) {
        let mut registry = self.registry.write().unwrap_or_else(|e| e.into_inner());
        registry.insert(table_name.to_string(), Instant::now());
        drop(registry);

        // 注册后触发惰性清理（针对洞察中间表）
        self.lazy_cleanup_insight_tables();
    }

    /// 注销临时表。
    ///
    /// # 参数
    /// - `table_name`: 临时表名
    pub fn unregister(&self, table_name: &str) {
        if let Ok(mut registry) = self.registry.write() {
            registry.remove(table_name);
        }
    }

    /// 获取临时表数量。
    ///
    /// # 返回
    /// 当前注册的临时表总数
    pub fn count(&self) -> usize {
        self.registry.read().map(|r| r.len()).unwrap_or(0)
    }

    /// 获取指定前缀的临时表数量。
    ///
    /// # 参数
    /// - `prefix`: 表名前缀（如 "tmp_i_"）
    ///
    /// # 返回
    /// 匹配前缀的临时表数量
    pub fn count_by_prefix(&self, prefix: &str) -> usize {
        self.registry
            .read()
            .map(|r| r.keys().filter(|name| name.starts_with(prefix)).count())
            .unwrap_or(0)
    }

    /// 按来源统计登记表数量（日志 / 诊断；见 [`TempTableStats`]）。
    ///
    /// 多前缀的来源（mock 同时有 `tmp_m_` 与 `temp_mock_`）按前缀**去重**计数，
    /// 不会因为命中两套前缀而被算两次（前缀之间互不包含，故“去重”就是一次匹配）。
    pub fn stats(&self) -> TempTableStats {
        let registry = self.registry.read().unwrap_or_else(|e| e.into_inner());
        let count = |source: TempTableSource| {
            registry
                .keys()
                .filter(|name| source.prefixes().iter().any(|p| name.starts_with(p)))
                .count()
        };
        TempTableStats {
            query: count(TempTableSource::Query),
            insight: count(TempTableSource::Insight),
            mock: count(TempTableSource::Mock),
            plugin: count(TempTableSource::Plugin),
        }
    }

    /// 惰性清理洞察中间表。
    ///
    /// 新建表时触发，扫描过期表自动 DROP，超上限则淘汰最旧表。
    ///
    /// # 返回
    /// 被清理的表名列表
    pub fn lazy_cleanup_insight_tables(&self) -> Vec<String> {
        let config = TempTableConfig::insight();
        let prefix = "tmp_i_";

        let mut registry = self.registry.write().unwrap_or_else(|e| e.into_inner());

        let now = Instant::now();
        let mut cleaned = Vec::new();

        // 收集所有洞察中间表
        let mut insight_tables: Vec<_> = registry
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, &time)| (name.clone(), time))
            .collect();

        // 按时间排序
        insight_tables.sort_by_key(|(_, time)| *time);

        // 清理过期表
        if let Some(ttl_secs) = config.ttl_secs {
            let ttl = Duration::from_secs(ttl_secs);
            let expired: Vec<_> = insight_tables
                .iter()
                .filter(|(_, time)| now.duration_since(*time) >= ttl)
                .map(|(name, _)| name.clone())
                .collect();

            for name in &expired {
                registry.remove(name);
                cleaned.push(name.clone());
            }

            // 更新 insight_tables 列表
            insight_tables.retain(|(name, _)| !expired.contains(name));
        }

        // 检查数量上限，淘汰最旧表
        if let Some(max_count) = config.max_count {
            while insight_tables.len() > max_count {
                if let Some((oldest_name, _)) = insight_tables.first() {
                    registry.remove(oldest_name);
                    cleaned.push(oldest_name.clone());
                    insight_tables.remove(0);
                } else {
                    break;
                }
            }
        }

        cleaned
    }

    /// 清理指定前缀的所有临时表。
    ///
    /// # 参数
    /// - `prefix`: 表名前缀
    ///
    /// # 返回
    /// 被清理的表名列表
    pub fn cleanup_by_prefix(&self, prefix: &str) -> Vec<String> {
        let mut registry = self.registry.write().unwrap_or_else(|e| e.into_inner());

        let tables_to_remove: Vec<_> = registry
            .keys()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect();

        for name in &tables_to_remove {
            registry.remove(name);
        }

        tables_to_remove
    }

    /// 清理所有临时表。
    ///
    /// # 返回
    /// 被清理的表名列表
    pub fn cleanup_all(&self) -> Vec<String> {
        let mut registry = self.registry.write().unwrap_or_else(|e| e.into_inner());

        let tables: Vec<_> = registry.keys().cloned().collect();
        registry.clear();
        tables
    }

    /// 清理指定插件的所有临时表。
    ///
    /// # 参数
    /// - `plugin_id`: 插件 ID
    ///
    /// # 返回
    /// 被清理的表名列表
    pub fn cleanup_plugin_tables(&self, plugin_id: &str) -> Vec<String> {
        let prefix = format!("tmp_p_{}_", plugin_id);
        self.cleanup_by_prefix(&prefix)
    }

    /// 列出连接上属于某来源的临时表（**以库里的实际表为准**，不只看注册表）。
    ///
    /// 为什么不能只看注册表：注册表只在「新建时」写，进程里可能有没登记的表
    /// （旧版本建的、或上一次清理漏掉的），按前缀查库才是权威。
    /// 限定 `catalog = memory` 且 `schema = main`：`ATTACH` 进来的文件库表不是临时表，
    /// 绝不能当成待清理对象。
    pub fn list_by_source(
        conn: &Connection,
        source: TempTableSource,
    ) -> Result<Vec<String>, CoreError> {
        let prefixes = source.prefixes();
        let sql = SqlEngine::build_select(
            "information_schema.tables",
            &["table_name", "table_schema", "table_catalog"],
            None,
        );
        let mut stmt = conn.prepare(&sql).map_err(db_error)?;
        let mut names = Vec::new();
        let mut rows = stmt.query([]).map_err(db_error)?;
        while let Some(row) = rows.next().map_err(db_error)? {
            let name: String = row.get(0).unwrap_or_default();
            let schema: String = row.get(1).unwrap_or_default();
            let catalog: String = row.get(2).unwrap_or_default();
            if schema != "main" || catalog != IN_MEMORY_CATALOG {
                continue;
            }
            if prefixes.iter().any(|prefix| name.starts_with(prefix)) {
                names.push(name);
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    /// 删除连接上属于某来源的全部临时表（返回被删表名），并同步注册表。
    ///
    /// 表名取自库（见 [`Self::list_by_source`]）；注册表里同前缀的名字一并摘掉，
    /// 否则会留下指向已删表的死名（`count_by_prefix` 会一直报高）。
    pub fn drop_by_source(
        &self,
        conn: &Connection,
        source: TempTableSource,
    ) -> Result<Vec<String>, CoreError> {
        let prefixes = source.prefixes();
        let mut targets = Self::list_by_source(conn, source)?;
        {
            let registry = self.registry.read().unwrap_or_else(|e| e.into_inner());
            for name in registry.keys() {
                if prefixes.iter().any(|prefix| name.starts_with(prefix))
                    && !targets.contains(name)
                {
                    targets.push(name.clone());
                }
            }
        }
        targets.sort();

        let mut dropped = Vec::new();
        for name in &targets {
            let sql = SqlEngine::build_drop_table(name, true);
            match conn.execute(&sql, []) {
                Ok(_) => {
                    self.unregister(name);
                    dropped.push(name.clone());
                }
                Err(e) => {
                    tracing::warn!("[TempTableManager] 清理临时表 {} 失败: {}", name, e)
                }
            }
        }
        Ok(dropped)
    }

    /// 获取全局 DuckDB 临时表上限。
    ///
    /// # 返回
    /// 全局临时表上限数量
    #[allow(dead_code)]
    pub fn global_max_tables(&self) -> usize {
        self.global_max_tables
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;
    use crate::duckdb::manager::DuckDBManager;
    use crate::sql::ColumnDefInfo;

    #[test]
    fn test_temp_table_source_abbreviation() {
        assert_eq!(TempTableSource::Query.abbreviation(), "q");
        assert_eq!(TempTableSource::Insight.abbreviation(), "i");
        assert_eq!(TempTableSource::Mock.abbreviation(), "m");
        assert_eq!(TempTableSource::Plugin.abbreviation(), "p");
    }

    #[test]
    fn test_temp_table_source_visibility() {
        assert!(!TempTableSource::Insight.is_user_visible());
        assert!(TempTableSource::Query.is_user_visible());
        assert!(TempTableSource::Mock.is_user_visible());
        assert!(TempTableSource::Plugin.is_user_visible());
    }

    #[test]
    fn test_generate_table_name_format() {
        let manager = TempTableManager::new(50);
        let name = manager.generate_name(TempTableSource::Query, "orders");

        assert!(name.starts_with("tmp_q_orders_"));
        assert_eq!(name.len(), "tmp_q_orders_".len() + 14); // prefix + timestamp (YYYYMMDDHHMMSS)
    }

    #[test]
    fn test_register_and_unregister() {
        let manager = TempTableManager::new(50);
        assert_eq!(manager.count(), 0);

        manager.register("tmp_q_test_20260512143025");
        assert_eq!(manager.count(), 1);

        manager.unregister("tmp_q_test_20260512143025");
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_temp_table_source_prefixes() {
        // mock 两套命名都要认（v2 规范名 + v1 沿用的 temp_mock_）
        assert!(TempTableSource::Mock.prefixes().contains(&"tmp_m_"));
        assert!(TempTableSource::Mock.prefixes().contains(&"temp_mock_"));
        assert_eq!(TempTableSource::Insight.prefixes().len(), 1);
        assert!(TempTableSource::Insight.prefixes().contains(&"tmp_i_"));
    }

    /// 按来源列 / 删临时表：以库为准（两套命名都要认），且不碰别的来源的表。
    #[test]
    fn test_list_and_drop_by_source_reads_the_database() {
        let conn = Connection::open_in_memory().expect("内存库");
        let cols = vec![ColumnDefInfo {
            name: "id".to_string(),
            data_type: "INTEGER".to_string(),
            unique: false,
            nullable: true,
        }];
        for name in ["tmp_m_a", "temp_mock_b", "tmp_i_c", "unrelated"] {
            conn.execute_batch(&SqlEngine::build_create_table(name, &cols, false))
                .expect("建表");
        }

        assert_eq!(
            TempTableManager::list_by_source(&conn, TempTableSource::Mock).expect("列出"),
            ["temp_mock_b".to_string(), "tmp_m_a".to_string()],
            "两套 mock 命名都要列出，别的来源与无关表不列"
        );

        let manager = TempTableManager::new(50);
        manager.register("tmp_m_a");
        manager.register("temp_mock_b");
        manager.register("tmp_i_c");

        let dropped = manager
            .drop_by_source(&conn, TempTableSource::Mock)
            .expect("删除");
        assert_eq!(dropped, ["temp_mock_b".to_string(), "tmp_m_a".to_string()]);
        assert!(
            TempTableManager::list_by_source(&conn, TempTableSource::Mock)
                .expect("列出")
                .is_empty(),
            "删完应当为空"
        );
        assert_eq!(
            TempTableManager::list_by_source(&conn, TempTableSource::Insight).expect("列出"),
            ["tmp_i_c".to_string()],
            "别的来源不受影响"
        );
        assert_eq!(manager.count(), 1, "注册表只摘被删的那两个");
    }

    /// `ATTACH` 进来的文件库表**不是**临时表：即使名字带 `temp_mock_` 也不能被当清理对象。
    #[test]
    fn test_list_by_source_ignores_attached_databases() {
        let conn = Connection::open_in_memory().expect("内存库");
        let dir = std::env::temp_dir().join(format!("rds_tt_attached_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录");
        let file_db = dir.join("other.duckdb");

        // 文件库里放一张叫 temp_mock_x 的表（正是 mock 临时表的命名风格）
        {
            let file_conn = Connection::open(&file_db).expect("文件库");
            file_conn
                .execute_batch("CREATE TABLE temp_mock_x (id INTEGER);")
                .expect("建表");
        }
        conn.execute_batch(&SqlEngine::build_attach_database(
            &file_db.to_string_lossy(),
            "other_db",
        ))
        .expect("挂载");

        let listed = TempTableManager::list_by_source(&conn, TempTableSource::Mock).expect("列出");
        assert!(
            listed.is_empty(),
            "挂载进来的文件库表不应算作临时表: {listed:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_count_by_prefix() {
        let manager = TempTableManager::new(50);

        manager.register("tmp_i_col_amount_20260512143030");
        manager.register("tmp_i_col_price_20260512143031");
        manager.register("tmp_q_orders_20260512143025");

        assert_eq!(manager.count_by_prefix("tmp_i_"), 2);
        assert_eq!(manager.count_by_prefix("tmp_q_"), 1);
        assert_eq!(manager.count_by_prefix("tmp_m_"), 0);
    }

    /// K16 ④：登记概览要按来源分档，且 mock 的两套前缀（`tmp_m_` / `temp_mock_`）不能算两次。
    #[test]
    fn test_stats_counts_by_source() {
        let manager = TempTableManager::new(50);

        manager.register("tmp_q_orders_20260512143025");
        manager.register("tmp_i_col_amount_20260512143030");
        manager.register("tmp_i_col_price_20260512143031");
        manager.register("tmp_m_users_20260512143030");
        manager.register("temp_mock_users");
        manager.register("tmp_p_plugin_20260512143035");
        // 不带任何来源前缀的表：既不属于任何一档，也不该被硬塞进某一档
        manager.register("unrelated_table");

        let stats = manager.stats();
        assert_eq!(stats.query, 1);
        assert_eq!(stats.insight, 2);
        assert_eq!(stats.mock, 2, "两套 mock 前缀都要算上");
        assert_eq!(stats.plugin, 1);
        assert_eq!(stats.total(), 6, "合计不含名字不带前缀的表");
    }

    #[test]
    fn test_cleanup_by_prefix() {
        let manager = TempTableManager::new(50);

        manager.register("tmp_i_table1_20260512143030");
        manager.register("tmp_i_table2_20260512143031");
        manager.register("tmp_q_table3_20260512143025");

        let cleaned = manager.cleanup_by_prefix("tmp_i_");
        assert_eq!(cleaned.len(), 2);
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_cleanup_all() {
        let manager = TempTableManager::new(50);

        manager.register("tmp_q_test1_20260512143025");
        manager.register("tmp_m_test2_20260512143030");
        manager.register("tmp_p_test3_20260512143035");

        let cleaned = manager.cleanup_all();
        assert_eq!(cleaned.len(), 3);
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_cleanup_plugin_tables() {
        let manager = TempTableManager::new(50);

        manager.register("tmp_p_plugin1_sql_20260512143040");
        manager.register("tmp_p_plugin1_data_20260512143041");
        manager.register("tmp_p_plugin2_sql_20260512143042");

        let cleaned = manager.cleanup_plugin_tables("plugin1");
        assert_eq!(cleaned.len(), 2);
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_lazy_cleanup_insight_tables_ttl() {
        let manager = TempTableManager::new(50);

        // 插入过期表
        let mut registry = manager.registry.write().unwrap();
        let expired_time = Instant::now()
            .checked_sub(Duration::from_secs(1801))
            .unwrap();
        registry.insert("tmp_i_expired_20260512140000".to_string(), expired_time);
        registry.insert("tmp_i_fresh_20260512143000".to_string(), Instant::now());
        drop(registry);

        let cleaned = manager.lazy_cleanup_insight_tables();
        assert_eq!(cleaned.len(), 1);
        assert!(cleaned.contains(&"tmp_i_expired_20260512140000".to_string()));
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_lazy_cleanup_insight_tables_max_count() {
        let manager = TempTableManager::new(50);

        // 插入超过上限的表（上限100）
        let mut registry = manager.registry.write().unwrap();
        for i in 0..105 {
            let name = format!("tmp_i_test_{:04}_20260512140000", i);
            let time = Instant::now() + Duration::from_secs(i as u64);
            registry.insert(name, time);
        }
        drop(registry);

        let cleaned = manager.lazy_cleanup_insight_tables();
        assert_eq!(cleaned.len(), 5);
        assert_eq!(manager.count_by_prefix("tmp_i_"), 100);
    }

    /// K16：统一的命名口——前缀跟来源走，同秒同描述不撞名（结果集表与洞察表共用）。
    #[test]
    fn test_generate_unique_name_prefixes_and_stays_unique() {
        for (source, prefix) in [
            (TempTableSource::Query, "tmp_q_"),
            (TempTableSource::Insight, "tmp_i_"),
            (TempTableSource::Mock, "tmp_m_"),
            (TempTableSource::Plugin, "tmp_p_"),
        ] {
            let name = generate_unique_name(source, "result");
            assert!(
                name.starts_with(prefix),
                "{source:?} 的表名应落 {prefix} 前缀下（回收机制以此识别）: {name}"
            );
        }

        let a = generate_unique_name(TempTableSource::Query, "result");
        let b = generate_unique_name(TempTableSource::Query, "result");
        assert_ne!(a, b, "同一秒建两张同描述的表不得撞名");
    }

    /// 定向删除：名字必须属于该来源，否则拒——否则这就是一把「传什么名字就删什么表」的刀。
    #[test]
    fn test_drop_temp_table_refuses_other_sources() {
        let conn = Connection::open_in_memory().expect("内存连接");
        let err = drop_temp_table(&conn, TempTableSource::Query, "tmp_i_someone_else")
            .expect_err("跨来源的表名应被拒");
        let text = err.to_string();
        assert!(text.contains("tmp_q_"), "报错要列出允许的前缀：{text}");
    }

    /// 定向删除的正路：表真的没、登记也同步摘掉（否则「登记数」与库里张数会对不上）。
    #[test]
    fn test_drop_temp_table_drops_and_unregisters() {
        let conn = Connection::open_in_memory().expect("内存连接");
        let name = generate_unique_name(TempTableSource::Query, "drop_check");
        conn.execute_batch(&format!("CREATE TABLE {} (id INTEGER)", quote_ident(&name)))
            .expect("建表");
        DuckDBManager::register_temp_table(&name);
        // 用**全名**当前缀断言：并行跑的用例可能也登记了 tmp_q_ 表，不能用前缀计数
        assert_eq!(
            DuckDBManager::temp_table_manager().count_by_prefix(&name),
            1,
            "建完应当被登记"
        );

        drop_temp_table(&conn, TempTableSource::Query, &name).expect("删表");
        let still_there = conn
            .query_row(
                "SELECT COUNT(*) FROM information_schema.tables
                 WHERE table_catalog = 'memory' AND table_schema = 'main' AND table_name = ?",
                duckdb::params![name],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0);
        assert_eq!(still_there, 0, "表应已删除");
        assert_eq!(
            DuckDBManager::temp_table_manager().count_by_prefix(&name),
            0,
            "登记表也要同步摘掉"
        );
    }
}
