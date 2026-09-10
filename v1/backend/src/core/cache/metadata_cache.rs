//! 元数据缓存
//!
//! 专门用于缓存数据库元数据（数据库列表、表结构、列信息等）

use std::time::Duration;

use super::{CachePolicy, CacheStats, LruCache, MemoryEstimate};
use crate::core::driver::{ColumnDetail, ConstraintDetail, IndexDetail};
use crate::core::{DataSourceMeta, SchemaObject};

/// 元数据缓存键
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum MetadataCacheKey {
    /// Catalog 列表
    Catalogs { conn_id: String },
    /// Schema 列表
    Schemas { conn_id: String, database: String },
    /// 表列表
    Tables {
        conn_id: String,
        database: String,
        schema: Option<String>,
    },
    /// 列列表
    Columns {
        conn_id: String,
        database: String,
        schema: Option<String>,
        table: String,
    },
    /// 视图列表
    Views {
        conn_id: String,
        database: String,
        schema: Option<String>,
    },
    /// 存储过程列表
    Procedures {
        conn_id: String,
        database: String,
        schema: Option<String>,
    },
    /// 函数列表
    Functions {
        conn_id: String,
        database: String,
        schema: Option<String>,
    },
    /// 索引列表
    Indexes {
        conn_id: String,
        database: String,
        schema: Option<String>,
        table: String,
    },
    /// 约束列表
    Constraints {
        conn_id: String,
        database: String,
        schema: Option<String>,
        table: String,
    },
    /// 序列列表
    Sequences {
        conn_id: String,
        database: String,
        schema: Option<String>,
    },
    /// 触发器列表
    Triggers {
        conn_id: String,
        database: String,
        schema: Option<String>,
    },
    /// 数据源元数据
    DataSourceMeta { conn_id: String },
    /// 过程/函数 DDL 源码
    RoutineSource {
        conn_id: String,
        database: String,
        schema: Option<String>,
        name: String,
        kind: String,
    },
}

impl MetadataCacheKey {
    /// 创建 Catalog 列表缓存键
    pub fn catalogs(conn_id: impl Into<String>) -> Self {
        Self::Catalogs {
            conn_id: conn_id.into(),
        }
    }

    /// 创建 Schema 列表缓存键
    pub fn schemas(conn_id: impl Into<String>, database: impl Into<String>) -> Self {
        Self::Schemas {
            conn_id: conn_id.into(),
            database: database.into(),
        }
    }

    /// 创建表列表缓存键
    pub fn tables(
        conn_id: impl Into<String>,
        database: impl Into<String>,
        schema: Option<String>,
    ) -> Self {
        Self::Tables {
            conn_id: conn_id.into(),
            database: database.into(),
            schema,
        }
    }

    /// 创建列列表缓存键
    pub fn columns(
        conn_id: impl Into<String>,
        database: impl Into<String>,
        schema: Option<String>,
        table: impl Into<String>,
    ) -> Self {
        Self::Columns {
            conn_id: conn_id.into(),
            database: database.into(),
            schema,
            table: table.into(),
        }
    }

    /// 创建存储过程列表键
    pub fn procedures(
        conn_id: impl Into<String>,
        database: impl Into<String>,
        schema: Option<String>,
    ) -> Self {
        Self::Procedures {
            conn_id: conn_id.into(),
            database: database.into(),
            schema,
        }
    }

    /// 创建函数列表键
    pub fn functions(
        conn_id: impl Into<String>,
        database: impl Into<String>,
        schema: Option<String>,
    ) -> Self {
        Self::Functions {
            conn_id: conn_id.into(),
            database: database.into(),
            schema,
        }
    }

    /// 创建序列列表键
    pub fn sequences(
        conn_id: impl Into<String>,
        database: impl Into<String>,
        schema: Option<String>,
    ) -> Self {
        Self::Sequences {
            conn_id: conn_id.into(),
            database: database.into(),
            schema,
        }
    }

    /// 创建触发器列表键
    pub fn triggers(
        conn_id: impl Into<String>,
        database: impl Into<String>,
        schema: Option<String>,
    ) -> Self {
        Self::Triggers {
            conn_id: conn_id.into(),
            database: database.into(),
            schema,
        }
    }

    /// 创建过程/函数源码键
    pub fn routine_source(
        conn_id: impl Into<String>,
        database: impl Into<String>,
        schema: Option<String>,
        name: impl Into<String>,
        kind: impl Into<String>,
    ) -> Self {
        Self::RoutineSource {
            conn_id: conn_id.into(),
            database: database.into(),
            schema,
            name: name.into(),
            kind: kind.into(),
        }
    }

    /// 获取连接 ID
    pub fn conn_id(&self) -> &str {
        match self {
            Self::Catalogs { conn_id } => conn_id,
            Self::Schemas { conn_id, .. } => conn_id,
            Self::Tables { conn_id, .. } => conn_id,
            Self::Columns { conn_id, .. } => conn_id,
            Self::Views { conn_id, .. } => conn_id,
            Self::Indexes { conn_id, .. } => conn_id,
            Self::Procedures { conn_id, .. } => conn_id,
            Self::Functions { conn_id, .. } => conn_id,
            Self::Constraints { conn_id, .. } => conn_id,
            Self::Sequences { conn_id, .. } => conn_id,
            Self::Triggers { conn_id, .. } => conn_id,
            Self::DataSourceMeta { conn_id } => conn_id,
            Self::RoutineSource { conn_id, .. } => conn_id,
        }
    }
}

/// 元数据缓存值
#[derive(Debug, Clone)]
pub enum MetadataCacheValue {
    /// 字符串列表（数据库名、Schema名等）
    StringList(Vec<String>),
    /// Schema 对象列表（表、列等）
    SchemaObjects(Vec<SchemaObject>),
    /// 列详细信息列表
    ColumnDetails(Vec<ColumnDetail>),
    /// 索引详情列表
    IndexDetails(Vec<IndexDetail>),
    /// 约束详情列表
    ConstraintDetails(Vec<ConstraintDetail>),
    /// 数据源元数据
    DataSourceMeta(DataSourceMeta),
    /// 过程/函数 DDL 源码
    RoutineSource(String),
}

impl MemoryEstimate for MetadataCacheValue {
    fn estimate_memory_bytes(&self) -> usize {
        match self {
            MetadataCacheValue::StringList(list) => {
                list.iter().map(|s| s.len() + 32).sum::<usize>()
            }
            MetadataCacheValue::SchemaObjects(objects) => objects.len() * 200,
            MetadataCacheValue::ColumnDetails(columns) => columns.len() * 250,
            MetadataCacheValue::IndexDetails(indexes) => indexes.len() * 200,
            MetadataCacheValue::ConstraintDetails(constraints) => constraints.len() * 220,
            MetadataCacheValue::DataSourceMeta(_) => 200,
            MetadataCacheValue::RoutineSource(s) => s.len() + 32,
        }
    }
}

/// 元数据缓存
pub struct MetadataCache {
    /// 内部 LRU 缓存
    cache: LruCache<MetadataCacheKey, MetadataCacheValue>,
    /// 默认 TTL
    default_ttl: Duration,
}

impl MetadataCache {
    /// 创建新的元数据缓存
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: LruCache::with_policy(capacity, CachePolicy::LRU(capacity)),
            default_ttl: Duration::from_secs(300), // 默认 5 分钟
        }
    }

    /// 创建带 TTL 的缓存
    pub fn with_ttl(capacity: usize, ttl: Duration) -> Self {
        Self {
            cache: LruCache::with_policy(capacity, CachePolicy::LRU(capacity)),
            default_ttl: ttl,
        }
    }

    // ==================== 获取方法 ====================

    /// 获取 Catalog 列表
    pub fn get_catalogs(&mut self, conn_id: &str) -> Option<Vec<String>> {
        let key = MetadataCacheKey::catalogs(conn_id);
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::StringList(list) => Some(list),
            _ => None,
        })
    }

    /// 获取 Schema 列表
    pub fn get_schemas(&mut self, conn_id: &str, database: &str) -> Option<Vec<String>> {
        let key = MetadataCacheKey::schemas(conn_id, database);
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::StringList(list) => Some(list),
            _ => None,
        })
    }

    /// 获取表列表
    pub fn get_tables(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
    ) -> Option<Vec<SchemaObject>> {
        let key = MetadataCacheKey::tables(conn_id, database, schema.map(|s| s.to_string()));
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::SchemaObjects(list) => Some(list),
            _ => None,
        })
    }

    /// 获取列列表
    pub fn get_columns(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Option<Vec<SchemaObject>> {
        let key =
            MetadataCacheKey::columns(conn_id, database, schema.map(|s| s.to_string()), table);
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::SchemaObjects(list) => Some(list),
            _ => None,
        })
    }

    /// 获取列详细信息（ColumnDetail）
    pub fn get_columns_detail(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Option<Vec<ColumnDetail>> {
        let key =
            MetadataCacheKey::columns(conn_id, database, schema.map(|s| s.to_string()), table);
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::ColumnDetails(list) => Some(list),
            _ => None,
        })
    }

    /// 获取存储过程列表
    pub fn get_procedures(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
    ) -> Option<Vec<SchemaObject>> {
        let key = MetadataCacheKey::procedures(conn_id, database, schema.map(|s| s.to_string()));
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::SchemaObjects(list) => Some(list),
            _ => None,
        })
    }

    /// 获取函数列表
    pub fn get_functions(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
    ) -> Option<Vec<SchemaObject>> {
        let key = MetadataCacheKey::functions(conn_id, database, schema.map(|s| s.to_string()));
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::SchemaObjects(list) => Some(list),
            _ => None,
        })
    }

    /// 获取序列列表
    pub fn get_sequences(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
    ) -> Option<Vec<SchemaObject>> {
        let key = MetadataCacheKey::sequences(conn_id, database, schema.map(|s| s.to_string()));
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::SchemaObjects(list) => Some(list),
            _ => None,
        })
    }

    /// 获取触发器列表
    pub fn get_triggers(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
    ) -> Option<Vec<SchemaObject>> {
        let key = MetadataCacheKey::triggers(conn_id, database, schema.map(|s| s.to_string()));
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::SchemaObjects(list) => Some(list),
            _ => None,
        })
    }

    /// 获取索引列表
    pub fn get_indexes(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Option<Vec<IndexDetail>> {
        let key = MetadataCacheKey::Indexes {
            conn_id: conn_id.to_string(),
            database: database.to_string(),
            schema: schema.map(|s| s.to_string()),
            table: table.to_string(),
        };
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::IndexDetails(list) => Some(list),
            _ => None,
        })
    }

    /// 获取约束列表
    pub fn get_constraints(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Option<Vec<ConstraintDetail>> {
        let key = MetadataCacheKey::Constraints {
            conn_id: conn_id.to_string(),
            database: database.to_string(),
            schema: schema.map(|s| s.to_string()),
            table: table.to_string(),
        };
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::ConstraintDetails(list) => Some(list),
            _ => None,
        })
    }

    /// 获取数据源元数据
    pub fn get_data_source_meta(&mut self, conn_id: &str) -> Option<DataSourceMeta> {
        let key = MetadataCacheKey::DataSourceMeta {
            conn_id: conn_id.to_string(),
        };
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::DataSourceMeta(meta) => Some(meta),
            _ => None,
        })
    }

    /// 获取过程/函数 DDL 源码
    pub fn get_routine_source(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        name: &str,
        kind: &str,
    ) -> Option<String> {
        let key = MetadataCacheKey::routine_source(
            conn_id,
            database,
            schema.map(|s| s.to_string()),
            name,
            kind,
        );
        self.cache.get(&key).and_then(|v| match v {
            MetadataCacheValue::RoutineSource(s) => Some(s),
            _ => None,
        })
    }

    // ==================== 设置方法 ====================

    /// 设置 Catalog 列表
    pub fn set_catalogs(&mut self, conn_id: &str, catalogs: Vec<String>) {
        let key = MetadataCacheKey::catalogs(conn_id);
        let value = MetadataCacheValue::StringList(catalogs);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置 Schema 列表
    pub fn set_schemas(&mut self, conn_id: &str, database: &str, schemas: Vec<String>) {
        let key = MetadataCacheKey::schemas(conn_id, database);
        let value = MetadataCacheValue::StringList(schemas);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置表列表
    pub fn set_tables(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        tables: Vec<SchemaObject>,
    ) {
        let key = MetadataCacheKey::tables(conn_id, database, schema.map(|s| s.to_string()));
        let value = MetadataCacheValue::SchemaObjects(tables);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置列列表
    pub fn set_columns(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        table: &str,
        columns: Vec<SchemaObject>,
    ) {
        let key =
            MetadataCacheKey::columns(conn_id, database, schema.map(|s| s.to_string()), table);
        let value = MetadataCacheValue::SchemaObjects(columns);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置列详细信息（ColumnDetail）
    pub fn set_columns_detail(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        table: &str,
        columns: Vec<ColumnDetail>,
    ) {
        let key =
            MetadataCacheKey::columns(conn_id, database, schema.map(|s| s.to_string()), table);
        let value = MetadataCacheValue::ColumnDetails(columns);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置存储过程列表
    pub fn set_procedures(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        procedures: Vec<SchemaObject>,
    ) {
        let key = MetadataCacheKey::procedures(conn_id, database, schema.map(|s| s.to_string()));
        let value = MetadataCacheValue::SchemaObjects(procedures);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置函数列表
    pub fn set_functions(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        functions: Vec<SchemaObject>,
    ) {
        let key = MetadataCacheKey::functions(conn_id, database, schema.map(|s| s.to_string()));
        let value = MetadataCacheValue::SchemaObjects(functions);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置索引列表
    pub fn set_indexes(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        table: &str,
        indexes: Vec<IndexDetail>,
    ) {
        let key = MetadataCacheKey::Indexes {
            conn_id: conn_id.to_string(),
            database: database.to_string(),
            schema: schema.map(|s| s.to_string()),
            table: table.to_string(),
        };
        let value = MetadataCacheValue::IndexDetails(indexes);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置约束列表
    pub fn set_constraints(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        table: &str,
        constraints: Vec<ConstraintDetail>,
    ) {
        let key = MetadataCacheKey::Constraints {
            conn_id: conn_id.to_string(),
            database: database.to_string(),
            schema: schema.map(|s| s.to_string()),
            table: table.to_string(),
        };
        let value = MetadataCacheValue::ConstraintDetails(constraints);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置序列列表
    pub fn set_sequences(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        sequences: Vec<SchemaObject>,
    ) {
        let key = MetadataCacheKey::sequences(conn_id, database, schema.map(|s| s.to_string()));
        let value = MetadataCacheValue::SchemaObjects(sequences);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置触发器列表
    pub fn set_triggers(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        triggers: Vec<SchemaObject>,
    ) {
        let key = MetadataCacheKey::triggers(conn_id, database, schema.map(|s| s.to_string()));
        let value = MetadataCacheValue::SchemaObjects(triggers);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    /// 设置数据源元数据
    pub fn set_data_source_meta(&mut self, conn_id: &str, meta: DataSourceMeta) {
        let key = MetadataCacheKey::DataSourceMeta {
            conn_id: conn_id.to_string(),
        };
        let value = MetadataCacheValue::DataSourceMeta(meta);
        // 数据源元数据缓存时间更长
        let ttl = self.default_ttl * 2;
        self.cache.put_with_ttl(key, value, Some(ttl));
    }

    /// 设置过程/函数 DDL 源码
    pub fn set_routine_source(
        &mut self,
        conn_id: &str,
        database: &str,
        schema: Option<&str>,
        name: &str,
        kind: &str,
        source: String,
    ) {
        let key = MetadataCacheKey::routine_source(
            conn_id,
            database,
            schema.map(|s| s.to_string()),
            name,
            kind,
        );
        let value = MetadataCacheValue::RoutineSource(source);
        self.cache.put_with_ttl(key, value, Some(self.default_ttl));
    }

    // ==================== 管理方法 ====================

    /// 清除指定连接的所有缓存
    pub fn invalidate_connection(&mut self, conn_id: &str) {
        // 收集需要移除的键
        let keys_to_remove: Vec<_> = self
            .cache
            .keys()
            .into_iter()
            .filter(|k| k.conn_id() == conn_id)
            .collect();

        for key in keys_to_remove {
            self.cache.remove(&key);
        }
    }

    /// 清除所有缓存
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// 获取统计信息
    pub fn stats(&self) -> &CacheStats {
        self.cache.stats()
    }

    /// 清理过期条目
    pub fn cleanup_expired(&mut self) -> usize {
        self.cache.cleanup_expired()
    }

    /// 获取当前大小
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// 强制淘汰指定比例的缓存
    ///
    /// # 参数
    /// * `ratio` - 淘汰比例 (0.0 - 1.0)
    pub fn force_evict(&mut self, ratio: f64) -> usize {
        self.cache.force_evict(ratio)
    }

    /// 内存压力感知淘汰
    pub fn memory_pressure_eviction(&mut self) -> usize {
        self.cache.memory_pressure_eviction()
    }

    /// 估算内存使用量（字节）
    pub fn estimated_memory_usage(&self) -> usize {
        // 粗略估算：每个 SchemaObject 约 200 字节，每个 String 约 50 字节
        let mut total = 0;
        for key in self.cache.keys() {
            match key {
                MetadataCacheKey::Catalogs { .. } => total += 100,
                MetadataCacheKey::Schemas { .. } => total += 150,
                MetadataCacheKey::Tables { .. } => total += 500,
                MetadataCacheKey::Columns { .. } => total += 1000,
                MetadataCacheKey::Views { .. } => total += 500,
                MetadataCacheKey::Indexes { .. } => total += 300,
                MetadataCacheKey::Constraints { .. } => total += 300,
                MetadataCacheKey::Sequences { .. } => total += 200,
                MetadataCacheKey::Triggers { .. } => total += 200,
                MetadataCacheKey::Procedures { .. } => total += 300,
                MetadataCacheKey::Functions { .. } => total += 300,
                MetadataCacheKey::DataSourceMeta { .. } => total += 200,
                MetadataCacheKey::RoutineSource { .. } => total += 200,
            }
        }
        total
    }
}

/// 元数据缓存配置
#[derive(Debug, Clone)]
pub struct MetadataCacheConfig {
    /// 缓存容量
    pub capacity: usize,
    /// 默认 TTL
    pub default_ttl: Duration,
    /// 数据库列表 TTL
    pub databases_ttl: Duration,
    /// Schema 列表 TTL
    pub schemas_ttl: Duration,
    /// 表列表 TTL
    pub tables_ttl: Duration,
    /// 列列表 TTL
    pub columns_ttl: Duration,
    /// 是否启用缓存
    pub enabled: bool,
}

impl Default for MetadataCacheConfig {
    fn default() -> Self {
        Self {
            capacity: 1000,
            default_ttl: Duration::from_secs(600),    // 10 分钟
            databases_ttl: Duration::from_secs(3600), // 1 小时（数据库列表极少变化）
            schemas_ttl: Duration::from_secs(1800),   // 30 分钟
            tables_ttl: Duration::from_secs(600),     // 10 分钟
            columns_ttl: Duration::from_secs(3600),   // 1 小时（列结构稳定）
            enabled: true,
        }
    }
}
