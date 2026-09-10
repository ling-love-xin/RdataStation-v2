# 项目模块完整架构文档

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

---

## 一、模块概述

### 1.1 核心定位

项目模块是 RdataStation 桌面数据库管理工具的核心工作单元管理系统，负责：

- **项目生命周期管理**：创建、加载、打开、关闭、删除、重命名
- **项目配置管理**：项目配置的读取、更新、持久化
- **项目存储管理**：项目级数据（连接、SQL历史、工作台状态）的持久化
- **项目验证**：项目结构完整性检查
- **最近项目缓存**：高性能的最近项目列表管理

### 1.2 设计原则

| 原则           | 描述                   | 实现方式                              |
| -------------- | ---------------------- | ------------------------------------- |
| **解耦**       | 命令层与核心逻辑分离   | Tauri Command → Core Store → Database |
| **缓存优先**   | 减少数据库查询         | 30秒TTL内存缓存                       |
| **错误分类**   | 结构化错误处理         | `ProjectError` 枚举 + 错误码          |
| **路径一致性** | 支持跨电脑项目迁移     | 智能路径处理 + 冲突解决               |
| **可扩展**     | 支持未来 DuckLake 协同 | `ProjectPath` 抽象（Local/Remote）    |

---

## 二、架构设计

### 2.1 分层架构

```
┌─────────────────────────────────────────────────────────────────┐
│                        Tauri Host Layer                         │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    lib.rs (Builder)                        │  │
│  │  - manage(ProjectState)                                    │  │
│  │  - invoke_handler(generate_handler![...])                  │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Command Layer (Tauri)                      │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              project_commands.rs                           │  │
│  │  ┌─────────────┐ ┌──────────────┐ ┌───────────────────┐  │  │
│  │  │ 生命周期命令 │ │ 配置管理命令  │ │  项目存储命令     │  │  │
│  │  │             │ │              │ │                   │  │  │
│  │  │ create_     │ │ get_project  │ │ init_project_     │  │  │
│  │  │ project     │ │ config       │ │ store             │  │  │
│  │  │ open_       │ │ update_      │ │ close_project_    │  │  │
│  │  │ project_*   │ │ project_     │ │ store             │  │  │
│  │  │ delete_     │ │ config       │ │ save_project_     │  │  │
│  │  │ project     │ │              │ │ store_*           │  │  │
│  │  │ rename_     │ │              │ │ get_project_      │  │  │
│  │  │ project     │ │              │ │ store_*           │  │  │
│  │  └─────────────┘ └──────────────┘ └───────────────────┘  │  │
│  │  ┌─────────────┐ ┌──────────────┐ ┌───────────────────┐  │  │
│  │  │ 验证命令     │ │ 缓存管理命令  │ │  系统级命令       │  │  │
│  │  │             │ │              │ │                   │  │  │
│  │  │ validate_   │ │ get_recent_  │ │ get_all_projects  │  │  │
│  │  │ project     │ │ projects     │ │                   │  │  │
│  │  │ validate_   │ │ add_recent_  │ │                   │  │  │
│  │  │ project_full│ │ project      │ │                   │  │  │
│  │  └─────────────┘ └──────────────┘ └───────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Core Layer (Pure Rust)                     │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              core/project/                                 │  │
│  │  ┌─────────────┐ ┌──────────────┐                         │  │
│  │  │ models.rs   │ │ store.rs     │                         │  │
│  │  │             │ │              │                         │  │
│  │  │ - Project   │ │ - Project    │                         │  │
│  │  │ - Project   │ │   Store      │                         │  │
│  │  │   Info      │ │ - Project    │                         │  │
│  │  │ - Project   │ │   Manager    │                         │  │
│  │  │   Config    │ │              │                         │  │
│  │  │ - Project   │ │              │                         │  │
│  │  │   Path      │ │              │                         │  │
│  │  │ - Project   │ │              │                         │  │
│  │  │   Status    │ │              │                         │  │
│  │  └─────────────┘ └──────────────┘                         │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │              core/persistence/                             │  │
│  │  ┌─────────────┐ ┌──────────────┐ ┌───────────────────┐  │  │
│  │  │ global_db   │ │ project_     │ │ project_db        │  │  │
│  │  │ .rs         │ │ store.rs     │ │ .rs               │  │  │
│  │  │             │ │              │ │                   │  │  │
│  │  │ - Global    │ │ - Project    │ │ - Project         │  │  │
│  │  │   Database  │ │   Store      │ │   Database        │  │  │
│  │  │   Manager   │ │   (连接/历史  │ │   Manager         │  │  │
│  │  │             │ │    /状态)     │ │   (SQLite+DuckDB) │  │  │
│  │  └─────────────┘ └──────────────┘ └───────────────────┘  │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Database Layer                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │   SQLite     │  │   DuckDB     │  │   File System        │  │
│  │              │  │              │  │                      │  │
│  │ - 全局元数据  │  │ - 分析数据    │  │ - project.json       │  │
│  │ - 项目索引    │  │ - 查询结果    │  │ - .RSmeta/           │  │
│  │ - 连接历史    │  │ - 缓存数据    │  │ - meta/project.db    │  │
│  │ - SQL历史    │  │              │  │ - analytics/          │  │
│  └──────────────┘  └──────────────┘  │   data.duckdb        │  │
│                                      └──────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 数据流

```
前端请求
    │
    ▼
Tauri Command (project_commands.rs)
    │
    ├─ 检查缓存 → 命中 → 返回
    │
    ├─ 调用 Core Store
    │       │
    │       ├─ 加载 project.json
    │       ├─ 打开 SQLite (meta/project.db)
    │       └─ 打开 DuckDB (analytics/data.duckdb)
    │
    ├─ 调用 Global DB Manager
    │       │
    │       ├─ SQLite 连接池 (全局元数据)
    │       └─ DuckDB 连接 (分析数据)
    │
    └─ 更新缓存 → 返回响应
```

---

## 三、核心数据结构

### 3.1 项目信息 (ProjectInfo)

```rust
pub struct ProjectInfo {
    pub id: String,                      // UUID
    pub name: String,                    // 项目名称
    pub description: Option<String>,     // 项目描述
    pub path: ProjectPath,               // 项目路径（支持本地/远程）
    pub status: ProjectStatus,           // 项目状态
    pub created_at: DateTime<Utc>,       // 创建时间
    pub updated_at: DateTime<Utc>,       // 更新时间
    pub last_opened_at: Option<DateTime<Utc>>, // 最后打开时间
    pub version_count: u32,              // 版本计数
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProjectPath {
    Local { path: PathBuf },             // 本地项目路径
    Remote { url: String, project_id: String }, // 远程项目（DuckLake）
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectStatus {
    Active,                              // 活跃
    Archived,                            // 已归档
    Syncing,                             // 同步中
    Offline,                             // 离线
}
```

### 3.2 项目配置 (ProjectConfig)

```rust
pub struct ProjectConfig {
    pub name: String,                    // 项目名称
    pub description: Option<String>,     // 项目描述
    pub version: String,                 // 配置版本
    pub connections: Vec<ConnectionRef>, // 连接引用
    pub queries: Vec<QueryRef>,          // 查询引用
    pub settings: HashMap<String, String>, // 自定义设置
}
```

### 3.3 项目存储 (ProjectStore)

```rust
pub struct ProjectStore {
    pub db_manager: Arc<ProjectDatabaseManager>, // 项目数据库管理器
}

// 项目数据库管理器
pub struct ProjectDatabaseManager {
    sqlite_pool: Arc<GlobalSqlitePool>,  // SQLite 连接池
    duckdb_conn: Arc<GlobalDuckdbConnection>, // DuckDB 连接
}
```

### 3.4 错误类型 (ProjectError)

```rust
pub enum ProjectError {
    NotFound(String),           // 项目不存在
    InvalidPath(String),        // 路径无效
    InvalidStructure(String),   // 结构不完整
    PathConflict(String),       // 路径冲突
    Database(String),           // 数据库错误
    Io(String),                 // IO 错误
    OperationFailed(String),    // 操作失败
}

// 错误码映射
impl ProjectError {
    pub fn code(&self) -> &'static str {
        match self {
            ProjectError::NotFound(_) => "PROJECT_NOT_FOUND",
            ProjectError::InvalidPath(_) => "PROJECT_INVALID_PATH",
            ProjectError::InvalidStructure(_) => "PROJECT_INVALID_STRUCTURE",
            ProjectError::PathConflict(_) => "PROJECT_PATH_CONFLICT",
            ProjectError::Database(_) => "PROJECT_DATABASE_ERROR",
            ProjectError::Io(_) => "PROJECT_IO_ERROR",
            ProjectError::OperationFailed(_) => "PROJECT_OPERATION_FAILED",
        }
    }
}
```

---

## 四、命令清单

### 4.1 项目生命周期命令

| 命令                      | 输入                 | 输出                    | 描述                     |
| ------------------------- | -------------------- | ----------------------- | ------------------------ |
| `create_project`          | `CreateProjectInput` | `CreateProjectResponse` | 创建新项目               |
| `create_and_save_project` | `CreateProjectInput` | `ProjectInfoResponse`   | 创建并保存到全局数据库   |
| `open_project_by_id`      | `id: String`         | `ProjectInfoResponse`   | 根据 ID 打开项目         |
| `open_project_by_path`    | `path: String`       | `ProjectInfoResponse`   | 根据路径打开项目         |
| `delete_project`          | `project_id: String` | `()`                    | 删除项目（从全局数据库） |
| `rename_project`          | `RenameProjectInput` | `()`                    | 重命名项目               |

### 4.2 项目配置命令

| 命令                    | 输入                       | 输出            | 描述         |
| ----------------------- | -------------------------- | --------------- | ------------ |
| `get_project_config`    | `path: String`             | `ProjectConfig` | 获取项目配置 |
| `update_project_config` | `UpdateProjectConfigInput` | `()`            | 更新项目配置 |

### 4.3 项目验证命令

| 命令                    | 输入                 | 输出                      | 描述             |
| ----------------------- | -------------------- | ------------------------- | ---------------- |
| `validate_project`      | `project_id: String` | `bool`                    | 验证项目基本结构 |
| `validate_project_full` | `project_id: String` | `ProjectValidationResult` | 验证项目完整性   |

### 4.4 最近项目命令

| 命令                  | 输入                   | 输出                       | 描述             |
| --------------------- | ---------------------- | -------------------------- | ---------------- |
| `get_recent_projects` | `limit: Option<usize>` | `Vec<ProjectInfoResponse>` | 获取最近项目列表 |
| `add_recent_project`  | `project_id: String`   | `()`                       | 添加到最近项目   |

### 4.5 系统级项目命令

| 命令               | 输入                 | 输出                       | 描述         |
| ------------------ | -------------------- | -------------------------- | ------------ |
| `get_all_projects` | -                    | `Vec<ProjectInfoResponse>` | 获取所有项目 |
| `update_project`   | `UpdateProjectInput` | `()`                       | 更新项目信息 |

### 4.6 项目存储命令

| 命令                                 | 输入                   | 输出                       | 描述           |
| ------------------------------------ | ---------------------- | -------------------------- | -------------- |
| `init_project_store`                 | `project_path: String` | `()`                       | 初始化项目存储 |
| `close_project_store`                | -                      | `()`                       | 关闭项目存储   |
| `save_project_store_connection`      | `StoredConnection`     | `()`                       | 保存连接       |
| `get_project_store_connections`      | -                      | `Vec<StoredConnection>`    | 获取所有连接   |
| `get_project_store_connection`       | `id: String`           | `Option<StoredConnection>` | 获取单个连接   |
| `delete_project_store_connection`    | `id: String`           | `()`                       | 删除连接       |
| `save_project_store_sql_history`     | `SqlHistoryRecord`     | `()`                       | 保存 SQL 历史  |
| `get_project_store_sql_history`      | `connection_id, limit` | `Vec<SqlHistoryRecord>`    | 获取 SQL 历史  |
| `save_project_store_workbench_state` | `WorkbenchState`       | `()`                       | 保存工作台状态 |
| `get_project_store_workbench_state`  | -                      | `Option<WorkbenchState>`   | 获取工作台状态 |

---

## 五、关键技术实现

### 5.1 缓存机制

#### 5.1.1 最近项目缓存

```rust
struct RecentProjectsCache {
    cache: RwLock<Option<(Vec<ProjectInfoResponse>, std::time::Instant)>>,
    ttl: std::time::Duration,
}

// 全局单例（30秒TTL）
fn get_recent_projects_cache() -> &'static RecentProjectsCache {
    use std::sync::OnceLock;
    static CACHE: OnceLock<RecentProjectsCache> = OnceLock::new();
    CACHE.get_or_init(|| RecentProjectsCache::new(30))
}
```

**缓存策略：**

- **TTL**: 30秒
- **失效时机**:
  - `add_recent_project` 后
  - `delete_project` 后
  - `update_project` 后
  - `rename_project` 后
  - `update_project_config` 后

**性能提升：**

- 首次查询：~50ms（数据库查询）
- 缓存命中：<1ms（内存读取）
- 缓存命中率：~80%（典型使用场景）

### 5.2 错误处理模式

#### 5.2.1 统一错误转换

所有项目命令使用统一的错误处理模式：

```rust
// 标准模式
let global_db = crate::core::migration::get_global_db_manager()
    .ok_or_else(|| ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())?;

let result = global_db.some_operation()
    .await
    .map_err(|e| ProjectError::Database(format!("操作失败: {}", e)).to_string())?;
```

#### 5.2.2 错误分类

| 错误类型           | 使用场景   | 示例                              |
| ------------------ | ---------- | --------------------------------- |
| `NotFound`         | 项目不存在 | `open_project_by_id` 找不到项目   |
| `InvalidPath`      | 路径无效   | `open_project_by_path` 路径不存在 |
| `InvalidStructure` | 结构不完整 | 缺少 `.RSmeta` 目录               |
| `PathConflict`     | 路径冲突   | 创建项目时路径已存在              |
| `Database`         | 数据库错误 | SQLite/DuckDB 操作失败            |
| `Io`               | IO 错误    | 文件读写失败                      |
| `OperationFailed`  | 操作失败   | 通用操作失败                      |

### 5.3 路径一致性处理

#### 5.3.1 智能保存策略

```rust
pub async fn save_project_info_smart(
    &self,
    id: &str,
    name: &str,
    description: Option<&str>,
    path: &str,
    status: &str,
    last_opened_at: Option<&str>,
) -> Result<(), CoreError> {
    // 1. 检查 ID 是否存在
    // 2. 检查路径是否冲突
    // 3. 智能处理：
    //    - 如果 ID 存在且路径相同 → 更新
    //    - 如果 ID 存在但路径不同 → 更新路径（支持跨电脑迁移）
    //    - 如果 ID 不存在 → 插入
}
```

#### 5.3.2 跨电脑项目迁移

当用户从其他电脑拷贝项目时：

1. **检测路径变化**：比较 `project.json` 中的路径与实际路径
2. **更新全局数据库**：使用实际打开的路径更新记录
3. **保持项目 ID**：确保项目 ID 不变，只更新路径

### 5.4 项目验证

#### 5.4.1 基本验证 (`validate_project`)

```rust
pub async fn validate_project(project_id: String) -> Result<bool, String> {
    // 1. 检查路径存在性
    // 2. 检查 .RSmeta 目录
    // 3. 返回布尔值
}
```

#### 5.4.2 完整验证 (`validate_project_full`)

```rust
pub struct ProjectValidationResult {
    pub is_valid: bool,           // 是否有效
    pub path_exists: bool,        // 路径是否存在
    pub meta_dir_exists: bool,    // .RSmeta 目录是否存在
    pub project_json_valid: bool, // project.json 是否有效
    pub errors: Vec<String>,      // 错误列表
}

pub async fn validate_project_full(project_id: String) -> Result<ProjectValidationResult, String> {
    // 1. 检查路径存在性
    // 2. 检查 .RSmeta 目录
    // 3. 检查 project.json 文件（解析验证）
    // 4. 返回详细验证结果
}
```

### 5.5 项目存储初始化

```rust
pub async fn init_project_store(
    project_path: String,
    state: State<'_, ProjectState>,
) -> Result<(), String> {
    // 1. 关闭旧的项目存储
    {
        let mut guard = state.store.lock().await;
        if let Some(store) = guard.take() {
            let _ = store.db_manager.close().await;
        }
    }

    // 2. 等待文件句柄释放（200ms）
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // 3. 打开新的项目存储
    let path = PathBuf::from(&project_path);
    let store = ProjectStore::new(&path).await.map_err(|e| e.to_string())?;

    // 4. 更新状态
    let mut guard = state.store.lock().await;
    *guard = Some(store);

    Ok(())
}
```

---

## 六、数据库设计

### 6.1 全局 SQLite（rdata_station.global.sqlite）

#### 6.1.1 project_info 表

```sql
CREATE TABLE IF NOT EXISTS project_info (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    path TEXT NOT NULL,
    status TEXT DEFAULT 'active',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_opened_at TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_project_info_last_opened
    ON project_info(last_opened_at DESC);
```

| 字段             | 类型      | 描述                                        |
| ---------------- | --------- | ------------------------------------------- |
| `id`             | TEXT      | 项目 UUID                                   |
| `name`           | TEXT      | 项目名称                                    |
| `description`    | TEXT      | 项目描述                                    |
| `path`           | TEXT      | 项目路径                                    |
| `status`         | TEXT      | 项目状态（active/archived/syncing/offline） |
| `created_at`     | TIMESTAMP | 创建时间                                    |
| `updated_at`     | TIMESTAMP | 更新时间                                    |
| `last_opened_at` | TIMESTAMP | 最后打开时间                                |

### 6.2 项目 SQLite（meta/project.db）

#### 6.2.1 connections 表

```sql
CREATE TABLE IF NOT EXISTS connections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    driver TEXT NOT NULL,
    host TEXT,
    port TEXT,
    database TEXT,
    username TEXT,
    password_encrypted TEXT,
    options TEXT,
    is_active TEXT,
    created_at TEXT,
    updated_at TEXT
);
```

#### 6.2.2 sql_history 表

```sql
CREATE TABLE IF NOT EXISTS sql_history (
    id TEXT PRIMARY KEY,
    connection_id TEXT,
    sql_text TEXT NOT NULL,
    execution_time_ms INTEGER,
    rows_affected INTEGER,
    error_message TEXT,
    is_favorite INTEGER DEFAULT 0,
    created_at TEXT
);
```

#### 6.2.3 workbench_state 表

```sql
CREATE TABLE IF NOT EXISTS workbench_state (
    id TEXT PRIMARY KEY,
    layout TEXT,
    open_panels TEXT,
    active_panel_id TEXT,
    updated_at TEXT
);
```

### 6.3 项目 DuckDB（analytics/data.duckdb）

用于分析查询和查询结果缓存，具体表结构根据业务需求定义。

---

## 七、项目目录结构

### 7.1 标准项目结构

```
/path/to/project/
├── .RSmeta/                    # 项目元数据目录
│   ├── project.json            # 项目配置
│   ├── meta/
│   │   └── project.db          # 项目 SQLite（连接、历史、状态）
│   └── analytics/
│       └── data.duckdb         # 项目 DuckDB（分析数据）
├── config/                     # 配置文件目录
│   └── *.json                  # 连接配置、SQL文件等
└── ...                         # 用户文件
```

### 7.2 project.json 格式

```json
{
  "name": "My Project",
  "description": "Project description",
  "version": "1.0.0",
  "connections": [
    {
      "id": "conn-uuid",
      "name": "MySQL Connection",
      "driver": "mysql"
    }
  ],
  "queries": [],
  "settings": {}
}
```

---

## 八、前端交互规范

### 8.1 项目加载流程

```typescript
// 1. 获取最近项目列表
const recentProjects = await invoke('get_recent_projects', { limit: 10 })

// 2. 打开项目
const project = await invoke('open_project_by_path', { path: '/path/to/project' })

// 3. 初始化项目存储
await invoke('init_project_store', { project_path: project.path })

// 4. 加载项目连接
const connections = await invoke('get_project_store_connections')

// 5. 加载工作台状态
const workbenchState = await invoke('get_project_store_workbench_state')
```

### 8.2 项目切换流程

```typescript
// 1. 关闭当前项目存储
await invoke('close_project_store')

// 2. 打开新项目
const project = await invoke('open_project_by_id', { id: 'new-project-id' })

// 3. 初始化新项目存储
await invoke('init_project_store', { project_path: project.path })

// 4. 更新 UI
currentProject.value = project
```

### 8.3 错误处理

```typescript
try {
  await invoke('open_project_by_path', { path })
} catch (error) {
  // 解析错误码
  const match = error.match(/\[(.*?)\]/)
  const errorCode = match ? match[1] : 'UNKNOWN'

  switch (errorCode) {
    case 'PROJECT_NOT_FOUND':
      // 显示项目不存在提示
      break
    case 'PROJECT_INVALID_PATH':
      // 显示路径无效提示
      break
    case 'PROJECT_INVALID_STRUCTURE':
      // 显示结构不完整提示
      break
    default:
    // 显示通用错误
  }
}
```

---

## 九、性能优化

### 9.1 缓存策略

| 缓存类型     | TTL  | 失效条件   | 命中率 |
| ------------ | ---- | ---------- | ------ |
| 最近项目列表 | 30秒 | 项目操作后 | ~80%   |

### 9.2 数据库优化

- **SQLite 连接池**：3个连接，支持并发读写
- **WAL 模式**：支持并发读写，提升性能
- **索引优化**：`last_opened_at` 降序索引，加速最近项目查询

### 9.3 文件句柄管理

- **延迟释放**：项目切换时等待 200ms，确保文件句柄释放
- **连接池复用**：项目存储使用连接池，避免频繁打开/关闭

---

## 十、安全考虑

### 10.1 密码存储

- 密码使用加密存储（`password_encrypted` 字段）
- 不在日志中输出密码
- 不通过 API 返回明文密码

### 10.2 路径验证

- 验证项目路径存在性
- 验证项目结构完整性
- 防止路径遍历攻击

### 10.3 错误信息

- 错误消息不包含敏感信息
- 使用错误码而非详细错误信息
- 日志记录详细错误，前端只显示友好提示

---

## 十一、测试策略

### 11.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_project() {
        // 测试项目创建
    }

    #[tokio::test]
    async fn test_open_project_by_path() {
        // 测试路径打开
    }

    #[tokio::test]
    async fn test_validate_project() {
        // 测试项目验证
    }
}
```

### 11.2 集成测试

- 测试完整的项目生命周期
- 测试跨电脑项目迁移
- 测试缓存失效逻辑

### 11.3 性能测试

- 测试最近项目查询性能
- 测试项目存储初始化性能
- 测试并发项目操作

---

## 十二、未来规划

### 12.1 短期优化

- [ ] 添加项目统计信息命令
- [ ] 优化项目存储初始化性能（智能等待策略）
- [ ] 添加项目导入/导出功能

### 12.2 中期规划

- [ ] 支持项目模板
- [ ] 支持项目批量操作
- [ ] 添加项目搜索功能

### 12.3 长期规划

- [ ] DuckLake 远程项目支持
- [ ] 项目协同编辑支持
- [ ] 项目版本历史

---

## 十三、常见问题

### 13.1 项目路径变更后无法打开

**原因**：全局数据库中的路径与实际路径不一致

**解决方案**：

1. 使用 `open_project_by_path` 命令，使用实际路径打开
2. 系统会自动更新全局数据库中的路径记录

### 13.2 项目结构不完整

**原因**：缺少 `.RSmeta` 目录或 `project.json` 文件

**解决方案**：

1. 使用 `validate_project_full` 命令检查具体缺失项
2. 手动创建缺失的目录和文件
3. 或重新创建项目

### 13.3 项目存储初始化失败

**原因**：文件句柄未释放

**解决方案**：

1. 等待几秒后重试
2. 检查是否有其他进程占用项目文件
3. 重启应用

---

## 十四、参考文档

- [项目模块优化分析报告](./PROJECT_MODULE_OPTIMIZATION.md)
- [后端架构文档](./ARCHITECTURE.md)
- [迁移系统文档](./MIGRATION_SYSTEM.md)

---

_文档结束_
