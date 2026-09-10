# RdataStation 后端实现文档

> 版本：v1.1
> 最后更新：2026-05-11
> 状态：✅ 已同步最新模块

---

本文档记录 RdataStation 项目后端核心功能的实现细节，包括内存防护、错误处理、持久化服务和性能监控。

---

## 目录

1. [内存防护增强](#1-内存防护增强)
   - 1.1 LRU 缓存淘汰服务
   - 1.2 连接池内存防护
   - 1.3 统一内存防护管理器
2. [错误处理增强](#2-错误处理增强)
   - 2.1 缓存错误域
   - 2.2 错误分类体系
3. [持久化服务](#3-持久化服务)
   - 3.1 SQL 模板持久化
   - 3.2 工作台上下文持久化
4. [性能监控](#4-性能监控)
   - 4.1 性能指标收集
   - 4.2 系统健康监控
5. [Tauri Commands 接口](#5-tauri-commands-接口)
6. [架构对齐说明](#6-架构对齐说明)

---

## 1. 内存防护增强

### 1.1 LRU 缓存淘汰服务

**文件位置**: `src-tauri/src/core/cache/lru_cache.rs`

#### 核心设计

```rust
/// 内存压力级别
pub enum MemoryPressure {
    Normal,      // 正常状态，不淘汰
    Moderate,    // 中等压力，淘汰 25%
    Critical,    // 高压力，淘汰 50%
}
```

#### 内存检测实现

| 平台    | 检测方式                               |
| ------- | -------------------------------------- |
| Linux   | 读取 `/proc/meminfo` 获取 MemAvailable |
| macOS   | 使用 `sysctl` 获取 vm_statistics       |
| Windows | 使用简单启发式（默认 Normal）          |

#### 关键方法

```rust
impl MemoryPressure {
    /// 检测当前系统内存压力
    pub fn detect() -> Self;

    /// 获取淘汰比例
    pub fn eviction_ratio(&self) -> f64;
}

impl<K, V> LruCache<K, V> {
    /// 内存压力感知淘汰
    pub fn memory_pressure_eviction(&mut self) -> usize;

    /// 强制淘汰指定比例
    pub fn force_evict(&mut self, ratio: f64) -> usize;
}
```

#### 内存估算

```rust
pub trait MemoryEstimate {
    fn estimate_memory_bytes(&self) -> usize;
}
```

已实现类型：

- `String` - 字符串长度 + 32 字节开销
- `Vec<String>` - 每个元素长度 + 32 字节
- `SchemaObject` - 固定 200 字节估算

---

### 1.2 连接池内存防护

**文件位置**: `src-tauri/src/core/driver/smart_pool.rs`

#### 自适应缩容

```rust
impl SmartPool {
    /// 检查是否因内存压力需要缩容
    async fn should_scale_down_for_memory(&self) -> bool;

    /// 内存压力感知缩容
    pub async fn memory_pressure_scale_down(&self) -> Result<u32, CoreError>;
}
```

#### 缩容策略

| 内存压力 | 缩容目标     | 空闲连接要求            |
| -------- | ------------ | ----------------------- |
| Normal   | 不缩容       | -                       |
| Moderate | 当前大小 / 2 | 空闲连接 > 当前大小 / 3 |
| Critical | 最小连接数   | 无要求                  |

#### 淘汰优先级

1. 优先淘汰空闲连接（`in_use = false`）
2. 按最后使用时间排序（LRU）
3. 保留最小连接数配置

---

### 1.3 统一内存防护管理器

**文件位置**: `src-tauri/src/core/cache/memory_guard.rs`

#### MemoryGuard 结构

```rust
pub struct MemoryGuard {
    config: MemoryGuardConfig,
    cache_manager: Arc<RwLock<CacheManager>>,
    stats: RwLock<MemoryStats>,
    last_check: RwLock<Instant>,
}
```

#### 配置项

| 配置项                | 默认值 | 说明             |
| --------------------- | ------ | ---------------- |
| `enabled`             | true   | 是否启用内存防护 |
| `check_interval_secs` | 60     | 检查间隔（秒）   |
| `max_cache_entries`   | 1000   | 最大缓存条目数   |
| `max_pool_size`       | 20     | 最大连接池大小   |

#### 检查与淘汰逻辑

```rust
impl MemoryGuard {
    /// 检查内存压力并执行淘汰
    pub async fn check_and_evict(&self) -> Result<usize, String>;

    /// 获取内存统计信息
    pub async fn get_stats(&self) -> MemoryStats;
}
```

#### 压力响应

| 压力级别 | 缓存淘汰比例     | 触发条件                     |
| -------- | ---------------- | ---------------------------- |
| Normal   | 0%（仅检查大小） | 缓存条目 > max_cache_entries |
| Moderate | 25%              | 系统内存使用 > 75%           |
| Critical | 50%              | 系统内存使用 > 90%           |

---

## 2. 错误处理增强

### 2.1 缓存错误域

**文件位置**: `src-tauri/src/core/error.rs`

#### CacheError 枚举

```rust
pub enum CacheError {
    Miss { key: String },
    Full { capacity: usize },
    InvalidKey { key: String, reason: String },
    InvalidValue { reason: String },
    Timeout { operation: String, duration_ms: u64 },
    Internal { reason: String },
    Serialization { reason: String },
}
```

#### 错误代码映射

| 错误类型                  | 错误代码            | 可重试 |
| ------------------------- | ------------------- | ------ |
| CacheError::Miss          | CACHE_MISS          | 否     |
| CacheError::Full          | CACHE_FULL          | 是     |
| CacheError::InvalidKey    | CACHE_INVALID_KEY   | 否     |
| CacheError::InvalidValue  | CACHE_INVALID_VALUE | 否     |
| CacheError::Timeout       | CACHE_TIMEOUT       | 是     |
| CacheError::Internal      | CACHE_INTERNAL      | 是     |
| CacheError::Serialization | CACHE_SERIALIZE     | 否     |

### 2.2 错误分类体系

```rust
pub enum ErrorCategory {
    Common,
    Connection,
    Database,
    Storage,
    Cache,      // 新增
    Plugin,
}
```

#### CoreError 完整结构

```rust
pub enum CoreError {
    Common(CommonError),
    Connection(ConnectionError),
    Database(DatabaseError),
    Storage(StorageError),
    Cache(CacheError),           // 新增
    Plugin { domain, code, message },
}
```

---

## 3. 持久化服务

### 3.1 SQL 模板持久化

**文件位置**: `src-tauri/src/core/persistence/sql_template_store.rs`

#### SqlTemplate 结构

```rust
pub struct SqlTemplate {
    pub id: String,              // UUID
    pub name: String,
    pub content: String,
    pub db_type: Option<String>, // mysql/postgresql/sqlite/duckdb/None
    pub category: String,        // 查询/DML/DDL
    pub description: Option<String>,
    pub tags: Option<String>,    // 逗号分隔
    pub is_builtin: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}
```

#### 内置模板

| ID                   | 名称         | 分类 |
| -------------------- | ------------ | ---- |
| builtin_select_all   | 查询所有记录 | 查询 |
| builtin_count        | 统计记录数   | 查询 |
| builtin_create_table | 创建表       | DDL  |
| builtin_insert       | 插入记录     | DML  |
| builtin_update       | 更新记录     | DML  |
| builtin_delete       | 删除记录     | DML  |

#### 数据库表结构

```sql
CREATE TABLE sql_templates (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    content TEXT NOT NULL,
    db_type TEXT,
    category TEXT NOT NULL,
    description TEXT,
    tags TEXT,
    is_builtin INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
```

#### CRUD 操作

| 方法                | 说明                          |
| ------------------- | ----------------------------- |
| `save()`            | 保存模板（INSERT OR REPLACE） |
| `get_by_id()`       | 根据 ID 获取                  |
| `get_all()`         | 获取所有模板                  |
| `get_by_category()` | 按分类获取                    |
| `get_by_db_type()`  | 按数据库类型获取              |
| `delete()`          | 删除（仅用户模板）            |
| `get_categories()`  | 获取所有分类                  |

---

### 3.2 工作台上下文持久化

**文件位置**: `src-tauri/src/core/persistence/workbench_context_store.rs`

#### WorkbenchLayout 结构

```rust
pub struct WorkbenchLayout {
    pub id: String,
    pub connection_id: String,
    pub panel_config: String,      // JSON 布局配置
    pub active_panel: String,
    pub sidebar_visible: bool,
    pub bottom_bar_visible: bool,
    pub updated_at_ms: u64,
}
```

#### EditorContext 结构

```rust
pub struct EditorContext {
    pub id: String,
    pub connection_id: String,
    pub content: String,
    pub cursor_position: usize,
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    pub updated_at_ms: u64,
}
```

#### 数据库表结构

```sql
-- 工作台布局表
CREATE TABLE workbench_layouts (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL,
    panel_config TEXT NOT NULL,
    active_panel TEXT NOT NULL,
    sidebar_visible INTEGER NOT NULL DEFAULT 1,
    bottom_bar_visible INTEGER NOT NULL DEFAULT 1,
    updated_at_ms INTEGER NOT NULL
);

-- 编辑器上下文表
CREATE TABLE editor_contexts (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL,
    content TEXT NOT NULL,
    cursor_position INTEGER NOT NULL DEFAULT 0,
    selection_start INTEGER,
    selection_end INTEGER,
    updated_at_ms INTEGER NOT NULL
);
```

---

## 4. 性能监控

### 4.1 性能指标收集

**文件位置**: `src-tauri/src/core/performance/monitor.rs`

#### PerformanceMetrics 结构

```rust
pub struct PerformanceMetrics {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_response_time_ms: f64,
    pub avg_response_time_ms: f64,
    pub max_response_time_ms: f64,
    pub min_response_time_ms: f64,
    pub active_requests: u64,
    pub cache_hit_rate: f64,
    pub estimated_memory_mb: f64,
}
```

#### PerformanceMonitor 方法

| 方法                      | 说明                          |
| ------------------------- | ----------------------------- |
| `record_request_start()`  | 记录请求开始，返回 request_id |
| `record_request_end()`    | 记录请求完成，更新指标        |
| `update_cache_hit_rate()` | 更新缓存命中率                |
| `update_memory_usage()`   | 更新内存使用估算              |
| `get_metrics()`           | 获取当前指标                  |
| `uptime()`                | 获取运行时间                  |
| `reset_metrics()`         | 重置所有指标                  |

### 4.2 系统健康监控

#### 健康状态判定

| 状态     | 缓存命中率 | 平均响应时间 | 活跃请求数 |
| -------- | ---------- | ------------ | ---------- |
| healthy  | > 80%      | < 100ms      | < 50       |
| degraded | > 50%      | < 500ms      | < 100      |
| critical | ≤ 50%      | ≥ 500ms      | ≥ 100      |

---

## 5. Tauri Commands 接口

### 5.1 内存管理 Commands

**文件**: `src-tauri/src/commands/memory_commands.rs`

| Command             | 输入       | 输出                | 说明                     |
| ------------------- | ---------- | ------------------- | ------------------------ |
| `get_memory_stats`  | -          | MemoryStatsResponse | 获取内存统计             |
| `cleanup_memory`    | -          | usize               | 清理内存，返回清理条目数 |
| `force_evict_cache` | ratio: f64 | usize               | 强制淘汰缓存             |

### 5.2 SQL 模板 Commands

**文件**: `src-tauri/src/commands/sql_template_commands.rs`

| Command                         | 输入                   | 输出                     | 说明             |
| ------------------------------- | ---------------------- | ------------------------ | ---------------- |
| `create_sql_template`           | CreateSqlTemplateInput | SqlTemplateResponse      | 创建模板         |
| `get_all_sql_templates`         | -                      | Vec<SqlTemplateResponse> | 获取所有模板     |
| `get_sql_templates_by_category` | category: String       | Vec<SqlTemplateResponse> | 按分类获取       |
| `get_sql_templates_by_db_type`  | db_type: String        | Vec<SqlTemplateResponse> | 按数据库类型获取 |
| `delete_sql_template`           | template_id: String    | bool                     | 删除模板         |
| `get_sql_template_categories`   | -                      | Vec<String>              | 获取所有分类     |

### 5.3 性能监控 Commands

**文件**: `src-tauri/src/commands/performance_commands.rs`

| Command                     | 输入 | 输出                       | 说明             |
| --------------------------- | ---- | -------------------------- | ---------------- |
| `get_performance_metrics`   | -    | PerformanceMetricsResponse | 获取性能指标     |
| `reset_performance_metrics` | -    | -                          | 重置性能指标     |
| `get_system_health`         | -    | HealthStatusResponse       | 获取系统健康状态 |

---

## 6. 架构对齐说明

### 6.1 前后端职责划分

| 功能     | 前端职责                    | 后端职责                 |
| -------- | --------------------------- | ------------------------ |
| 内存防护 | UI 组件卸载、事件监听器清理 | LRU 缓存淘汰、连接池缩容 |
| 错误处理 | 错误展示、用户提示          | 错误分类、重试逻辑       |
| 缓存管理 | 前端状态缓存                | 元数据缓存、内存压力感知 |
| 持久化   | 本地文件读写                | SQLite 系统数据库        |

### 6.2 内存防护分层策略

```
┌─────────────────────────────────────────────────────┐
│                    前端层 (Vue 3)                     │
│  - 组件卸载时清理引用                                 │
│  - 事件监听器自动移除                                 │
│  - 大数据集虚拟滚动                                   │
└─────────────────────────────────────────────────────┘
                          │
                    tauri.invoke
                          │
┌─────────────────────────────────────────────────────┐
│                  后端层 (Rust Core)                   │
│  - LRU 缓存淘汰（内存压力感知）                       │
│  - 连接池自适应缩容                                   │
│  - MemoryGuard 统一协调                              │
└─────────────────────────────────────────────────────┘
```

### 6.3 错误处理流程

```
前端请求
    │
    ▼
Tauri Command
    │
    ▼
Service Layer
    │
    ▼
Core Module ──► CoreError
    │                │
    ▼                ▼
Result<T, E>   ErrorCategory
    │                │
    ▼                ▼
JSON Response   Retryable?
```

### 6.4 数据持久化架构

```
┌─────────────────────────────────────────────────────┐
│                  系统级数据 (SQLite)                   │
│  - 连接信息 (connection_store)                        │
│  - SQL 历史 (history_store)                           │
│  - SQL 模板 (sql_template_store)                      │
│  - 工作台上下文 (workbench_context_store)             │
│  - 元数据缓存 (metadata_cache)                        │
└─────────────────────────────────────────────────────┘
                          │
┌─────────────────────────────────────────────────────┐
│                  项目级数据 (JSON)                     │
│  - 项目配置 (config/*.json)                           │
│  - 项目连接 (project_connection_store)                │
│  - 项目元数据 (project.db)                            │
└─────────────────────────────────────────────────────┘
```

---

## 附录

### A. 文件清单

| 文件                                          | 说明             |
| --------------------------------------------- | ---------------- |
| `core/cache/lru_cache.rs`                     | LRU 缓存实现     |
| `core/cache/memory_guard.rs`                  | 内存防护管理器   |
| `core/cache/metadata_cache.rs`                | 元数据缓存       |
| `core/cache/cache_manager.rs`                 | 缓存管理器       |
| `core/driver/smart_pool.rs`                   | 智能连接池       |
| `core/error.rs`                               | 错误定义         |
| `core/persistence/sql_template_store.rs`      | SQL 模板存储     |
| `core/persistence/workbench_context_store.rs` | 工作台上下文存储 |
| `core/performance/monitor.rs`                 | 性能监控         |
| `commands/memory_commands.rs`                 | 内存管理命令     |
| `commands/sql_template_commands.rs`           | SQL 模板命令     |
| `commands/performance_commands.rs`            | 性能监控命令     |

### B. 依赖清单

| 依赖             | 版本 | 用途              |
| ---------------- | ---- | ----------------- |
| rusqlite         | 0.32 | SQLite 数据库操作 |
| serde/serde_json | 1.0  | 序列化/反序列化   |
| tokio            | 1.44 | 异步运行时        |
| uuid             | 1.x  | UUID 生成         |
| thiserror        | 1.0  | 错误类型定义      |
| anyhow           | 1.0  | 错误处理简化      |

### C. 测试覆盖

| 模块                  | 测试状态    |
| --------------------- | ----------- |
| LRU Cache             | ✅ 单元测试 |
| MemoryGuard           | ⏳ 集成测试 |
| SqlTemplateStore      | ⏳ 单元测试 |
| WorkbenchContextStore | ⏳ 单元测试 |
| PerformanceMonitor    | ✅ 单元测试 |
| 四库连接集成          | ✅ 集成测试 |
| DriverRegistry        | ✅ 集成测试 |
| ConnectionManager     | ✅ 集成测试 |
| PersistenceHelpers    | ✅ 单元测试 |

---

_文档版本: 1.1_
_最后更新: 2026-05-11_
