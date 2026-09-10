# 数据库导航模块 - 前后端对齐文档

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

> 本文档描述数据库导航模块的前后端功能对齐、职责划分、接口契约

---

## 📋 目录

- [1. 架构分层原则](#1-架构分层原则)
- [2. 前后端职责划分](#2-前后端职责划分)
- [3. 功能对齐映射表](#3-功能对齐映射表)
- [4. 内存防护分层策略](#4-内存防护分层策略)
- [5. 接口契约定义](#5-接口契约定义)
- [6. 数据流设计](#6-数据流设计)
- [7. 错误处理策略](#7-错误处理策略)
- [8. 性能优化对齐](#8-性能优化对齐)
- [9. 开发规范对齐](#9-开发规范对齐)

---

## 1. 架构分层原则

### 1.1 分层模型

```
┌─────────────────────────────────────────────────────────────┐
│                    前端 (Vue 3 + TS)                         │
├─────────────────────────────────────────────────────────────┤
│  UI 层        │ 组件渲染、用户交互、状态展示                  │
├─────────────────────────────────────────────────────────────┤
│  Composable   │ 业务逻辑封装、状态管理、事件处理              │
├─────────────────────────────────────────────────────────────┤
│  Store 层     │ 本地状态缓存、响应式数据                      │
├─────────────────────────────────────────────────────────────┤
│  API 层       │ Tauri invoke 封装、重试、类型转换             │
├═════════════════════════════════════════════════════════════┤
│                   IPC 边界 (Tauri Commands)                   │
├═════════════════════════════════════════════════════════════┤
│  Command 层   │ 输入校验、DTO 转换、服务调用                  │
├─────────────────────────────────────────────────────────────┤
│  Service 层   │ 业务逻辑、连接管理、元数据服务                │
├─────────────────────────────────────────────────────────────┤
│  Driver 层    │ 数据库驱动、SQL 执行、结果返回                │
├─────────────────────────────────────────────────────────────┤
│  数据库       │ MySQL / PostgreSQL / SQLite / DuckDB         │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 职责划分原则

| 层级     | 职责                           | 技术栈       | 关注点                       |
| -------- | ------------------------------ | ------------ | ---------------------------- |
| **前端** | UI 展示、用户交互、本地状态    | Vue 3 + TS   | 响应速度、用户体验、内存占用 |
| **后端** | 业务逻辑、数据持久化、连接管理 | Rust + Tauri | 数据一致性、资源管理、性能   |

---

## 2. 前后端职责划分

### 2.1 为什么前端需要内存泄漏防护？

**前端内存泄漏场景**：

| 场景              | 原因                   | 影响         |
| ----------------- | ---------------------- | ------------ |
| 组件频繁创建/销毁 | Vue 组件生命周期管理   | 累积内存占用 |
| Map/Set 无限增长  | 缓存未设置上限         | 页面卡顿     |
| 事件监听器未清理  | onUnmounted 未调用清理 | 内存泄漏     |
| 定时器未清除      | setInterval 未清理     | 后台任务累积 |

**前端内存防护实现**：

```typescript
// use-memory-protection.ts
export function useMemoryProtection() {
  const MAX_CACHE_SIZE = 1000
  const MAX_NOTIFICATIONS = 50

  function enforceCacheLimit(cache: Map<string, unknown>) {
    if (cache.size > MAX_CACHE_SIZE) {
      // 淘汰最旧的缓存项
      const oldestKey = cache.keys().next().value
      cache.delete(oldestKey)
    }
  }

  onUnmounted(() => {
    // 组件卸载时清理所有资源
    cleanup()
  })
}
```

### 2.2 后端内存防护策略

**后端内存泄漏场景**：

| 场景           | 原因             | 影响           |
| -------------- | ---------------- | -------------- |
| 连接池未释放   | 连接未正确关闭   | 数据库连接耗尽 |
| 缓存无限增长   | 元数据缓存未淘汰 | 内存溢出       |
| 文件句柄未关闭 | 文件操作异常     | 文件锁定       |
| Arc 循环引用   | 引用计数未归零   | 内存泄漏       |

**后端内存防护实现**：

```rust
// 后端缓存淘汰策略
use std::collections::HashMap;
use std::sync::Mutex;

pub struct MetadataCache {
    cache: Mutex<HashMap<String, CacheEntry>>,
    max_size: usize,
}

impl MetadataCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            max_size,
        }
    }

    pub fn insert(&self, key: String, value: Vec<String>) {
        let mut cache = self.cache.lock().unwrap();

        // 超过上限时淘汰最旧的条目
        if cache.len() >= self.max_size {
            if let Some(oldest_key) = cache.keys().next().cloned() {
                cache.remove(&oldest_key);
            }
        }

        cache.insert(key, CacheEntry {
            data: value,
            created_at: std::time::Instant::now(),
        });
    }
}
```

### 2.3 前后端内存防护对比

| 维度         | 前端                     | 后端                     |
| ------------ | ------------------------ | ------------------------ |
| **防护对象** | Vue 组件状态、DOM 引用   | 连接池、缓存、文件句柄   |
| **实现方式** | Composable + onUnmounted | Rust 所有权 + Drop trait |
| **触发时机** | 组件卸载、页面切换       | 连接关闭、缓存淘汰       |
| **限制策略** | Map 大小限制、数组截断   | LRU 缓存、连接池大小限制 |
| **监控方式** | performance API          | Rust metrics crate       |

---

## 3. 功能对齐映射表

### 3.1 核心功能对齐

| 功能             | 前端实现                    | 后端实现                    | 接口契约                          | 状态      |
| ---------------- | --------------------------- | --------------------------- | --------------------------------- | --------- |
| **连接管理**     |                             |                             |                                   |           |
| 创建连接         | `use-database-navigator.ts` | `connect_database` command  | `ConnectDatabaseInput`            | ✅ 已对齐 |
| 获取连接列表     | `loadGlobalConnections()`   | `get_connections` command   | `Vec<ConnectionInfoResponse>`     | ✅ 已对齐 |
| 断开连接         | `disconnectConnection()`    | `close_connection` command  | `conn_id: String`                 | ✅ 已对齐 |
| 切换连接         | `switch_connection()`       | `switch_connection` command | `conn_id: String`                 | ✅ 已对齐 |
| **元数据服务**   |                             |                             |                                   |           |
| 获取数据库列表   | `loadDatabases()`           | `list_databases` command    | `conn_id: String`                 | ✅ 已对齐 |
| 获取 Schema 列表 | `loadSchemas()`             | `list_schemas` command      | `conn_id, db_name`                | ✅ 已对齐 |
| 获取表列表       | `loadTables()`              | `list_tables` command       | `conn_id, db_name, schema`        | ✅ 已对齐 |
| 获取列信息       | `loadColumns()`             | `list_columns` command      | `conn_id, db_name, schema, table` | ✅ 已对齐 |
| **元数据缓存**   |                             |                             |                                   |           |
| 获取缓存状态     | `getMetadataCacheStatus()`  | `get_metadata_cache_status` | `connection_id, db_name`          | ✅ 已对齐 |
| 刷新缓存         | `refreshMetadataCache()`    | `refresh_metadata_cache`    | `connection_id`                   | ✅ 已对齐 |
| 清除缓存         | `clearMetadataCache()`      | `clear_metadata_cache`      | `connection_id`                   | ✅ 已对齐 |
| **导航状态**     |                             |                             |                                   |           |
| 保存状态         | `saveNavigatorState()`      | `save_navigator_state`      | `SaveNavigatorStateInput`         | ✅ 已对齐 |
| 加载状态         | `loadNavigatorState()`      | `load_navigator_state`      | `connection_id: String`           | ✅ 已对齐 |

### 3.2 增强功能对齐

| 功能           | 前端实现                        | 后端实现                       | 接口契约              | 状态        |
| -------------- | ------------------------------- | ------------------------------ | --------------------- | ----------- |
| **内存防护**   |                                 |                                |                       |             |
| 缓存大小限制   | `use-memory-protection.ts`      | `MetadataCache::new(max_size)` | 配置参数              | ⚠️ 需对齐   |
| 组件清理       | `onUnmounted(cleanup)`          | `Drop` trait 实现              | 自动                  | ✅ 已对齐   |
| **错误处理**   |                                 |                                |                       |             |
| 错误边界       | `use-error-boundary.ts`         | `Result<T, CoreError>`         | 统一错误类型          | ⚠️ 需对齐   |
| 自动重试       | `invokeWithRetry()`             | 后端重试逻辑                   | 重试次数配置          | ⚠️ 需对齐   |
| **拖拽支持**   |                                 |                                |                       |             |
| 拖拽数据       | `use-drag-drop.ts`              | 无需后端支持                   | 纯前端功能            | ✅ 前端独立 |
| **上下文记忆** |                                 |                                |                       |             |
| 保存上下文     | `use-context-memory.ts`         | `save_navigator_state`         | localStorage + SQLite | ✅ 已对齐   |
| **快捷键**     |                                 |                                |                       |             |
| 快捷键注册     | `use-keyboard-shortcuts.ts`     | 无需后端支持                   | 纯前端功能            | ✅ 前端独立 |
| **SQL 模板**   |                                 |                                |                       |             |
| 内置模板       | `use-sql-templates.ts`          | 可选后端存储                   | `SqlTemplate` 结构    | 🟡 可选     |
| 自定义模板     | `addCustomTemplate()`           | `save_sql_template` command    | 待实现                | 🔴 待实现   |
| **数据字典**   |                                 |                                |                       |             |
| 导出 Markdown  | `use-data-dictionary-export.ts` | 可选后端服务                   | 纯前端功能            | ✅ 前端独立 |
| 导出 HTML      | `generateHtml()`                | 可选后端服务                   | 纯前端功能            | ✅ 前端独立 |
| **性能监控**   |                                 |                                |                       |             |
| 前端性能       | `performance-monitor.ts`        | 后端性能监控                   | 待实现                | 🔴 待实现   |
| 后端性能       | 无                              | `metrics` crate                | 待实现                | 🔴 待实现   |

---

## 4. 内存防护分层策略

### 4.1 前端内存防护（已实现）

**文件**: `src/extensions/builtin/database/ui/composables/use-memory-protection.ts`

**防护策略**：

```typescript
// 1. 缓存大小限制
const MAX_CACHE_SIZE = 1000
const MAX_NOTIFICATIONS = 50
const MAX_SEARCH_HISTORY = 50

// 2. 组件卸载清理
onUnmounted(() => {
  cleanup() // 清理所有回调
  clearAllIntervals() // 清理所有定时器
  clearAllTimeouts() // 清理所有延时器
})

// 3. 数组截断
function trimNotifications(notifications: unknown[]) {
  if (notifications.length > MAX_NOTIFICATIONS) {
    notifications.splice(0, notifications.length - MAX_NOTIFICATIONS)
  }
}

// 4. Map 淘汰
function enforceCacheLimit(cache: Map<string, unknown>) {
  if (cache.size > MAX_CACHE_SIZE) {
    const oldestKey = cache.keys().next().value
    cache.delete(oldestKey)
  }
}
```

### 4.2 后端内存防护（需实现）

**文件**: `src-tauri/src/core/services/cache_service.rs`（新建）

**防护策略**：

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 缓存条目
struct CacheEntry {
    data: serde_json::Value,
    created_at: Instant,
    access_count: u64,
}

/// 带淘汰策略的缓存
pub struct ManagedCache {
    cache: Mutex<HashMap<String, CacheEntry>>,
    max_size: usize,
    ttl: Duration,
}

impl ManagedCache {
    pub fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            max_size,
            ttl,
        }
    }

    /// 插入缓存（带淘汰）
    pub fn insert(&self, key: String, value: serde_json::Value) {
        let mut cache = self.cache.lock().unwrap();

        // 淘汰过期条目
        self.evict_expired(&mut cache);

        // 超过上限时淘汰最少访问的条目
        if cache.len() >= self.max_size {
            self.evict_lru(&mut cache);
        }

        cache.insert(key, CacheEntry {
            data: value,
            created_at: Instant::now(),
            access_count: 0,
        });
    }

    /// 淘汰过期条目
    fn evict_expired(&self, cache: &mut HashMap<String, CacheEntry>) {
        let expired_keys: Vec<String> = cache.iter()
            .filter(|(_, entry)| entry.created_at.elapsed() > self.ttl)
            .map(|(key, _)| key.clone())
            .collect();

        for key in expired_keys {
            cache.remove(&key);
        }
    }

    /// 淘汰最少访问的条目
    fn evict_lru(&self, cache: &mut HashMap<String, CacheEntry>) {
        if let Some(least_accessed_key) = cache.iter()
            .min_by_key(|(_, entry)| entry.access_count)
            .map(|(key, _)| key.clone())
        {
            cache.remove(&least_accessed_key);
        }
    }
}

impl Drop for ManagedCache {
    fn drop(&mut self) {
        // 缓存销毁时清理所有资源
        self.cache.lock().unwrap().clear();
    }
}
```

### 4.3 前后端内存防护对比

| 维度         | 前端                       | 后端                         |
| ------------ | -------------------------- | ---------------------------- |
| **实现位置** | `use-memory-protection.ts` | `cache_service.rs`           |
| **防护对象** | Vue 组件状态、Map/Set      | 连接池、元数据缓存           |
| **淘汰策略** | FIFO（先进先出）           | LRU + TTL（最少使用 + 过期） |
| **触发时机** | 组件卸载、插入新项         | 插入新项、定时清理           |
| **配置参数** | `MAX_CACHE_SIZE = 1000`    | `max_size`, `ttl`            |
| **清理方式** | `onUnmounted` 钩子         | `Drop` trait                 |

---

## 5. 接口契约定义

### 5.1 连接管理接口

```typescript
// 前端类型定义
interface ConnectDatabaseInput {
  db_type: string // 数据库类型
  url: string // 连接 URL
  name?: string // 连接名称
  connection_type?: string // "global" | "project"
  project_id?: string // 项目 ID（项目连接需要）
}

interface ConnectionInfoResponse {
  id: string
  name: string
  db_type: string
  url: string
  connection_type: string // "global" | "project"
  project_id?: string
  status: string
  is_active: boolean
  created_at_ms: number
}
```

```rust
// 后端类型定义
#[derive(serde::Deserialize)]
pub struct ConnectDatabaseInput {
    pub db_type: String,
    pub url: String,
    pub name: Option<String>,
    pub connection_type: Option<String>,
    pub project_id: Option<String>,
}

#[derive(serde::Serialize)]
pub struct ConnectionInfoResponse {
    pub id: String,
    pub name: String,
    pub db_type: String,
    pub url: String,
    pub connection_type: String,
    pub project_id: Option<String>,
    pub status: String,
    pub is_active: bool,
    pub created_at_ms: u64,
}
```

### 5.2 元数据缓存接口

```typescript
// 前端类型定义
interface CacheStatusResponse {
  is_valid: boolean
  last_sync: string | null
  stats: {
    table_count: number
    column_count: number
    index_count: number
  } | null
}
```

```rust
// 后端类型定义
#[derive(serde::Serialize)]
pub struct CacheStatusResponse {
    pub is_valid: bool,
    pub last_sync: Option<String>,
    pub stats: Option<CacheStats>,
}

#[derive(serde::Serialize)]
pub struct CacheStats {
    pub table_count: usize,
    pub column_count: usize,
    pub index_count: usize,
}
```

### 5.3 导航状态接口

```typescript
// 前端类型定义
interface NavigatorState {
  expandedKeys: string[]
  selectedKeys: string[]
  filterConfig: Record<string, unknown>
  lastConnectionId: string | null
  timestamp: number
}
```

```rust
// 后端类型定义
#[derive(serde::Deserialize)]
pub struct SaveNavigatorStateInput {
    pub connection_id: String,
    pub expanded_keys: Vec<String>,
    pub selected_keys: Vec<String>,
    pub filter_config: Option<serde_json::Value>,
}

#[derive(serde::Serialize)]
pub struct LoadNavigatorStateResponse {
    pub expanded_keys: Vec<String>,
    pub selected_keys: Vec<String>,
    pub filter_config: Option<serde_json::Value>,
}
```

---

## 6. 数据流设计

### 6.1 连接创建流程

```
前端 (Vue)
  │
  │ 1. 用户填写连接表单
  ▼
use-database-navigator.ts
  │
  │ 2. 调用 Store 保存连接
  ▼
database-navigator-store.ts
  │
  │ 3. 调用 API 层
  ▼
navigator-api.ts
  │
  │ 4. invoke('connect_database', input)
  ▼
┌─────────────────────────────────────┐
│          IPC 边界 (Tauri)            │
├─────────────────────────────────────┤
│ 5. command.rs 输入校验              │
│ 6. DTO 转换                         │
└─────────────────────────────────────┘
  │
  ▼
ConnectionService::connect_with_type()
  │
  │ 7. 创建数据库连接
  ▼
Driver::connect()
  │
  │ 8. 返回连接信息
  ▼
前端接收响应 → 更新本地状态 → 刷新 UI
```

### 6.2 元数据查询流程

```
前端 (Vue)
  │
  │ 1. 用户展开连接节点
  ▼
tree-data-builder.ts
  │
  │ 2. 检查本地缓存
  ├─ 缓存命中 → 直接返回
  └─ 缓存未命中 → 调用 API
  │
  ▼
navigator-api.ts
  │
  │ 3. invoke('list_tables', { conn_id, db_name, schema })
  ▼
┌─────────────────────────────────────┐
│          IPC 边界 (Tauri)            │
├─────────────────────────────────────┤
│ 4. navigator_commands.rs            │
│ 5. 获取连接                         │
└─────────────────────────────────────┘
  │
  ▼
Database::list_tables()
  │
  │ 6. 执行元数据查询
  ▼
数据库返回结果
  │
  │ 7. 更新本地缓存
  ▼
前端接收响应 → 构建树节点 → 渲染 UI
```

---

## 7. 错误处理策略

### 7.1 前端错误处理

```typescript
// use-error-boundary.ts
export function useErrorBoundary() {
  function withErrorHandling<T>(fn: () => Promise<T>): Promise<T> {
    return fn().catch(error => {
      handleError(error)
      throw error
    })
  }

  function withRetry<T>(fn: () => Promise<T>, maxRetries = 3, delayMs = 1000): Promise<T> {
    return fn().catch(async error => {
      for (let i = 0; i < maxRetries; i++) {
        try {
          await sleep(delayMs * (i + 1))
          return await fn()
        } catch (retryError) {
          if (i === maxRetries - 1) {
            throw retryError
          }
        }
      }
      throw error
    })
  }
}
```

### 7.2 后端错误处理

```rust
// core/error.rs
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Query failed: {0}")]
    QueryFailed(String),

    #[error("No active connection")]
    NoActiveConnection,

    #[error("Cache error: {0}")]
    CacheError(String),
}

// commands/connection_commands.rs
pub async fn connect_database(
    input: ConnectDatabaseInput,
) -> Result<ConnectDatabaseResponse, String> {
    // 输入校验
    if input.url.is_empty() {
        return Err("Database URL cannot be empty".into());
    }

    // 业务逻辑
    let service = ConnectionService::new(get_connection_manager().clone());
    let (conn_id, db) = service.connect_with_type(...)
        .await
        .map_err(|e| e.to_string())?; // 统一错误转换

    Ok(ConnectDatabaseResponse { ... })
}
```

### 7.3 错误处理对齐

| 维度         | 前端            | 后端                            |
| ------------ | --------------- | ------------------------------- |
| **错误类型** | `Error`         | `CoreError`                     |
| **错误捕获** | `try/catch`     | `Result<T, E>`                  |
| **错误转换** | `error.message` | `.map_err(\|e\| e.to_string())` |
| **重试策略** | 指数退避        | 可配置重试                      |
| **用户提示** | 通知系统        | 日志记录                        |

---

## 8. 性能优化对齐

### 8.1 前端性能优化

| 优化项     | 实现方式            | 效果             |
| ---------- | ------------------- | ---------------- |
| 元数据缓存 | 内存 Map + 状态检查 | 减少 API 调用    |
| 虚拟滚动   | NTree 内置支持      | 大数据量流畅渲染 |
| 防抖保存   | debounce 300ms      | 减少存储操作     |
| 懒加载     | 按需加载子节点      | 减少初始加载     |
| 内存防护   | 缓存大小限制        | 防止内存泄漏     |

### 8.2 后端性能优化

| 优化项     | 实现方式        | 效果           |
| ---------- | --------------- | -------------- |
| 连接池     | sqlx Pool       | 复用连接       |
| 元数据缓存 | `MetadataCache` | 减少数据库查询 |
| 异步执行   | Tokio 运行时    | 高并发处理     |
| 缓存淘汰   | LRU + TTL       | 控制内存占用   |
| 查询优化   | 编译期检查      | 减少运行时错误 |

### 8.3 性能指标对齐

| 指标       | 前端目标 | 后端目标 | 测量方式                    |
| ---------- | -------- | -------- | --------------------------- |
| 响应时间   | < 200ms  | < 100ms  | performance.now() / Instant |
| 内存占用   | < 100MB  | < 50MB   | Chrome DevTools / heaptrack |
| CPU 使用   | < 10%    | < 5%     | Task Manager / top          |
| 缓存命中率 | > 80%    | > 90%    | 自定义指标                  |

---

## 9. 开发规范对齐

### 9.1 前端开发规范

```typescript
// ✅ 正确：强类型
interface Props {
  connectionId: string
  schemaName: string
  tableName: string
}

// ❌ 错误：使用 any
interface Props {
  data: any
}

// ✅ 正确：错误处理
try {
  await invoke('connect_database', input)
} catch (error) {
  handleError(error)
}

// ❌ 错误：忽略错误
await invoke('connect_database', input)
```

### 9.2 后端开发规范

```rust
// ✅ 正确：强类型
#[derive(serde::Deserialize)]
pub struct ConnectDatabaseInput {
    pub db_type: String,
    pub url: String,
}

// ❌ 错误：使用 serde_json::Value
pub async fn connect_database(input: serde_json::Value) { }

// ✅ 正确：错误处理
let result = service.connect().await
    .map_err(|e| e.to_string())?;

// ❌ 错误：unwrap
let result = service.connect().await.unwrap();
```

### 9.3 接口对齐检查清单

- [ ] 前端类型与后端 DTO 字段一致
- [ ] 前端错误处理与后端错误类型对应
- [ ] 前端缓存策略与后端缓存策略协调
- [ ] 前端重试次数与后端重试次数匹配
- [ ] 前端超时时间与后端超时时间协调

---

## 附录

### A. Tauri Commands 清单

| Command                     | 文件                         | 功能             | 状态 |
| --------------------------- | ---------------------------- | ---------------- | ---- |
| `connect_database`          | `connection_commands.rs`     | 创建连接         | ✅   |
| `get_connections`           | `connection_commands.rs`     | 获取连接列表     | ✅   |
| `close_connection`          | `connection_commands.rs`     | 关闭连接         | ✅   |
| `switch_connection`         | `connection_commands.rs`     | 切换连接         | ✅   |
| `list_databases`            | `navigator_commands.rs`      | 获取数据库列表   | ✅   |
| `list_schemas`              | `navigator_commands.rs`      | 获取 Schema 列表 | ✅   |
| `list_tables`               | `navigator_commands.rs`      | 获取表列表       | ✅   |
| `list_columns`              | `navigator_commands.rs`      | 获取列信息       | ✅   |
| `save_navigator_state`      | `navigator_commands.rs`      | 保存导航状态     | ✅   |
| `load_navigator_state`      | `navigator_commands.rs`      | 加载导航状态     | ✅   |
| `get_metadata_cache_status` | `metadata_cache_commands.rs` | 获取缓存状态     | ✅   |
| `refresh_metadata_cache`    | `metadata_cache_commands.rs` | 刷新缓存         | ✅   |
| `clear_metadata_cache`      | `metadata_cache_commands.rs` | 清除缓存         | ✅   |

### B. 前端 Composables 清单

| Composable                | 文件                            | 功能         | 状态 |
| ------------------------- | ------------------------------- | ------------ | ---- |
| `useDatabaseNavigator`    | `use-database-navigator.ts`     | 导航核心逻辑 | ✅   |
| `useWorkbenchIntegration` | `use-workbench-integration.ts`  | 工作台集成   | ✅   |
| `useConnectionHealth`     | `use-connection-health.ts`      | 连接健康监控 | ✅   |
| `useNotification`         | `use-notification.ts`           | 统一通知系统 | ✅   |
| `useBatchOperations`      | `use-batch-operations.ts`       | 批量操作支持 | ✅   |
| `useAdvancedSearch`       | `use-advanced-search.ts`        | 智能搜索增强 | ✅   |
| `useMemoryProtection`     | `use-memory-protection.ts`      | 内存泄漏防护 | ✅   |
| `useErrorBoundary`        | `use-error-boundary.ts`         | 错误边界处理 | ✅   |
| `useDragDrop`             | `use-drag-drop.ts`              | 拖拽支持     | ✅   |
| `useContextMemory`        | `use-context-memory.ts`         | 上下文记忆   | ✅   |
| `useKeyboardShortcuts`    | `use-keyboard-shortcuts.ts`     | 快捷操作面板 | ✅   |
| `useSqlTemplates`         | `use-sql-templates.ts`          | SQL 模板库   | ✅   |
| `useDataDictionaryExport` | `use-data-dictionary-export.ts` | 数据字典导出 | ✅   |
| `useAccessibility`        | `use-accessibility.ts`          | 无障碍访问   | ✅   |

---

> 文档版本: 1.0.0
> 最后更新: 2026-04-25
> 维护者: RdataStation 开发团队
