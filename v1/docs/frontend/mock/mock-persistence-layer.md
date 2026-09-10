# Mock 数据生成器持久化层 — 设计·开发·接口 一体化文档

> 版本：v1.2
> 日期：2026-08-01
> 状态：✅ 开发完成（全命令 + Store 模式 + 数据分布可视化，前后端均已实现）
> 关联文档：[mock-data-generator-design.md](./mock-data-generator-design.md)（主设计文档 v3.0）

---

## 目录

- [一、设计文档](#一设计文档)
  - [1.1 架构定位](#11-架构定位)
  - [1.2 分层架构图](#12-分层架构图)
  - [1.3 表结构设计](#13-表结构设计)
  - [1.4 Rust 数据结构](#14-rust-数据结构)
  - [1.5 Store 模式设计](#15-store-模式设计)
  - [1.6 迁移策略](#16-迁移策略)
  - [1.7 与现有模块的关系](#17-与现有模块的关系)
- [二、开发文档](#二开发文档)
  - [2.1 文件清单](#21-文件清单)
  - [2.2 开发阶段](#22-开发阶段)
  - [2.3 开工清单](#23-开工清单)
- [三、接口文档](#三接口文档)
  - [3.1 Tauri Command 接口总览](#31-tauri-command-接口总览)
  - [3.2 命令详细规格](#32-命令详细规格)
  - [3.3 前端 API 层](#33-前端-api-层)
  - [3.4 前端集成要点](#34-前端集成要点)
  - [3.5 数据流图](#35-数据流图)

---

## 一、设计文档

### 1.1 架构定位

Mock 数据生成器持久化层在四层架构中的位置：

```
UI 层（Vue 3）
  └── MockPanel.vue → useMockStore → mock-api.ts
       │
Tauri IPC
  │
命令层（commands/）
  ├── mock_commands.rs           ← 现有：13 个生成命令（不动）
  └── mock_persistence_commands.rs  ← 🆕 新增：5 个持久化命令
       │
核心层（core/mock/）
  ├── engine.rs                  ← 现有：生成引擎（不动）
  ├── models.rs                  ← 现有：生成模型（不动）
  ├── templates.rs               ← 现有：内置模板（不动）
  ├── history.rs                 ← 现有：DuckDB 历史（保留，不破坏）
  ├── persistence.rs             ← 🆕 新增：持久化 Store
  └── mod.rs                     ← 修改：新增模块声明
       │
持久化层（core/persistence/）
  └── project_db.rs              ← 现有：ProjectSqlitePool（复用）
       │
存储层
  └── .RSMETA/project.db         ← 现有项目 SQLite（新增 4 张表）
```

**设计原则**：

- 🔒 **`mock_` 前缀隔离**：所有新表使用 `mock_` 前缀，不混入现有业务表
- 📦 **项目级存储**：表建在项目 SQLite（`.RSMETA/project.db`），项目关闭时统一清理
- 🔧 **增量增强**：不破坏现有 `history.rs`（DuckDB），新增项目级 SQLite 持久化
- 📋 **字段配置明细**：每列生成规则、参数、置信度单独记录，支持重新生成和模板复用
- 🏗️ **Store 模式**：参照 `AnalyticsResourceStore`，用 `ProjectSqlitePool` 封装 CRUD

### 1.2 分层架构图

```
┌─────────────────────────────────────────────────────────────┐
│                    前端 (Vue 3 + TS)                         │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              MockPanel.vue                            │   │
│  │  ┌─────────┐  ┌──────────┐  ┌────────────────────┐  │   │
│  │  │ 生成配置 │  │ 预览区域 │  │ 生成历史 Tab       │  │   │
│  │  │ (现有)   │  │ (现有)   │  │ 🆕 调用持久化 API │  │   │
│  │  └─────────┘  └──────────┘  └────────────────────┘  │   │
│  └──────────────────────┬───────────────────────────────┘   │
│                         │ tauri.invoke()                     │
├─────────────────────────┼───────────────────────────────────┤
│                   Tauri IPC                                  │
├─────────────────────────┼───────────────────────────────────┤
│                   Rust 后端                                  │
│                                                              │
│  ┌──────────────────────┴───────────────────────────────┐   │
│  │          commands/                                     │   │
│  │  ┌────────────────────┐ ┌───────────────────────────┐│   │
│  │  │mock_commands.rs    │ │mock_persistence_commands  ││   │
│  │  │ (13 命令, 不动)    │ │ (5 命令, 🆕)              ││   │
│  │  └────────┬───────────┘ └─────────────┬─────────────┘│   │
│  └───────────┼───────────────────────────┼──────────────┘   │
│              │                           │                   │
│  ┌───────────┴───────────────────────────┴──────────────┐   │
│  │          core/mock/                                   │   │
│  │  ┌──────────┐ ┌──────────────┐ ┌──────────────────┐  │   │
│  │  │engine.rs │ │templates.rs  │ │persistence.rs 🆕 │  │   │
│  │  │(生成引擎)│ │(内置模板)    │ │ MockGeneration   │  │   │
│  │  │          │ │              │ │ Store            │  │   │
│  │  └──────────┘ └──────────────┘ └────────┬─────────┘  │   │
│  └─────────────────────────────────────────┼────────────┘   │
│                                            │                  │
│  ┌─────────────────────────────────────────┴────────────┐   │
│  │          core/persistence/ (复用)                     │   │
│  │  ┌────────────────────┐                              │   │
│  │  │ ProjectSqlitePool  │ ← 连接池 + WAL 模式          │   │
│  │  └────────┬───────────┘                              │   │
│  └───────────┼──────────────────────────────────────────┘   │
│              │                                               │
├──────────────┼───────────────────────────────────────────────┤
│        文件系统 (.RSMETA/project.db)                          │
│  ┌──────────┴───────────────────────────────────────────┐   │
│  │            项目 SQLite (project.db)                   │   │
│  │  ┌────────────┐ ┌──────────────┐ ┌────────────────┐  │   │
│  │  │connections │ │query_history │ │analytics_res.. │  │   │
│  │  │ (现有)     │ │ (现有)       │ │ (现有)         │  │   │
│  │  └────────────┘ └──────────────┘ └────────────────┘  │   │
│  │  ┌────────────┐ ┌──────────────┐ ┌────────────────┐  │   │
│  │  │mock_gen_   │ │mock_gen_     │ │mock_user_      │  │   │
│  │  │tasks 🆕    │ │columns 🆕    │ │templates 🆕    │  │   │
│  │  └────────────┘ └──────────────┘ └────────────────┘  │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### 1.3 表结构设计

迁移脚本：`migrations/project_meta/009_mock_generation.sql`

#### 1.3.1 `mock_generation_tasks` — 生成任务历史表

```sql
CREATE TABLE IF NOT EXISTS mock_generation_tasks (
    id                TEXT PRIMARY KEY,                    -- UUID v4
    table_name        TEXT NOT NULL,                       -- 用户命名的表名
    table_alias       TEXT,                                -- DuckDB 临时表名
    row_count         INTEGER NOT NULL,                    -- 请求生成行数
    seed              INTEGER,                             -- 随机种子（NULL=随机）
    locale            TEXT DEFAULT 'ZH_CN',                -- 区域设置
    scene_id          TEXT,                                -- 场景模板 ID（内置或自定义）
    save_format       TEXT,                                -- 保存格式: table/parquet/csv
    status            TEXT DEFAULT 'success',              -- success/failed/cancelled
    error_message     TEXT,                                -- 失败时的错误信息
    generated_rows    INTEGER,                             -- 实际生成行数
    generation_time_ms INTEGER,                            -- 生成耗时(ms)
    created_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

| 字段                 | 类型    | 说明                                                                        |
| -------------------- | ------- | --------------------------------------------------------------------------- |
| `id`                 | TEXT PK | UUID v4，全局唯一                                                           |
| `table_name`         | TEXT    | 用户命名，如 `"orders"`                                                     |
| `table_alias`        | TEXT    | 实际生成的 DuckDB 临时表名，如 `"temp_mock_orders_202605081430"`            |
| `row_count`          | INTEGER | 请求生成的行数                                                              |
| `seed`               | INTEGER | 随机种子，NULL 表示完全随机                                                 |
| `locale`             | TEXT    | 区域设置，默认 `ZH_CN`                                                      |
| `scene_id`           | TEXT    | 关联模板 ID：内置模板如 `"builtin:ecommerce"`，自定义模板如 `"user:abc123"` |
| `save_format`        | TEXT    | 导出格式：`table`/`parquet`/`csv`/`xlsx`/`sql`                              |
| `status`             | TEXT    | 执行状态：`success`/`failed`/`cancelled`                                    |
| `error_message`      | TEXT    | 失败时的错误详情                                                            |
| `generated_rows`     | INTEGER | 实际生成行数（与 `row_count` 可能不同）                                     |
| `generation_time_ms` | INTEGER | 生成耗时，精度毫秒                                                          |

#### 1.3.2 `mock_generation_columns` — 任务字段配置详情表

```sql
CREATE TABLE IF NOT EXISTS mock_generation_columns (
    id                TEXT PRIMARY KEY,                    -- UUID v4
    task_id           TEXT NOT NULL,                       -- 关联任务 ID
    column_name       TEXT NOT NULL,                       -- 字段名
    column_type       TEXT NOT NULL,                       -- 数据类型
    generator         TEXT NOT NULL,                       -- 生成器名称，如 "FirstName(ZH_CN)"
    generator_params  TEXT,                                -- 生成器参数 JSON
    null_ratio        REAL DEFAULT 0,                      -- 空值比例 0.0~1.0
    is_unique         INTEGER DEFAULT 0,                   -- 是否唯一值
    is_primary_key    INTEGER DEFAULT 0,                   -- 是否主键
    is_foreign_key    INTEGER DEFAULT 0,                   -- 是否外键
    ref_table         TEXT,                                -- 外键关联表名
    ref_column        TEXT,                                -- 外键关联列名
    comment           TEXT,                                -- 字段注释
    confidence        TEXT,                                -- 智能映射置信度: high/medium/low/manual
    sort_order        INTEGER NOT NULL,                    -- 字段排列顺序（0-based）
    FOREIGN KEY (task_id) REFERENCES mock_generation_tasks(id) ON DELETE CASCADE
);
```

| 字段               | 类型    | 说明                                                  |
| ------------------ | ------- | ----------------------------------------------------- |
| `id`               | TEXT PK | UUID v4                                               |
| `task_id`          | TEXT FK | 关联 `mock_generation_tasks.id`，级联删除             |
| `column_name`      | TEXT    | 字段名，如 `"email"`                                  |
| `column_type`      | TEXT    | 数据类型，如 `"Varchar(100)"`                         |
| `generator`        | TEXT    | 生成器名称，如 `"SafeEmail(ZH_CN)"`，包含 locale 信息 |
| `generator_params` | TEXT    | JSON 格式参数，如 `{"min":1,"max":100}`               |
| `null_ratio`       | REAL    | 空值比例，0.0 表示不允许 NULL                         |
| `is_unique`        | INTEGER | 0=不唯一, 1=唯一                                      |
| `is_primary_key`   | INTEGER | 0=否, 1=是                                            |
| `is_foreign_key`   | INTEGER | 0=否, 1=是                                            |
| `ref_table`        | TEXT    | 外键引用的表名                                        |
| `ref_column`       | TEXT    | 外键引用的列名                                        |
| `comment`          | TEXT    | 字段注释/说明                                         |
| `confidence`       | TEXT    | 智能映射置信度：`high`/`medium`/`low`/`manual`        |
| `sort_order`       | INTEGER | 字段在表中的排列顺序                                  |

#### 1.3.3 `mock_user_templates` — 用户自定义模板主表

```sql
CREATE TABLE IF NOT EXISTS mock_user_templates (
    id                TEXT PRIMARY KEY,                    -- UUID v4
    name              TEXT NOT NULL,                       -- 模板名称
    description       TEXT,                                -- 模板描述
    row_count         INTEGER NOT NULL DEFAULT 1000,       -- 默认生成行数
    seed              INTEGER,                             -- 默认随机种子
    locale            TEXT DEFAULT 'ZH_CN',                -- 默认区域
    created_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 1.3.4 `mock_template_columns` — 模板字段详情表

```sql
CREATE TABLE IF NOT EXISTS mock_template_columns (
    id                TEXT PRIMARY KEY,                    -- UUID v4
    template_id       TEXT NOT NULL,                       -- 关联模板 ID
    column_name       TEXT NOT NULL,
    column_type       TEXT NOT NULL,
    generator         TEXT NOT NULL,
    generator_params  TEXT,
    null_ratio        REAL DEFAULT 0,
    is_unique         INTEGER DEFAULT 0,
    is_primary_key    INTEGER DEFAULT 0,
    is_foreign_key    INTEGER DEFAULT 0,
    ref_table         TEXT,
    ref_column        TEXT,
    comment           TEXT,
    confidence        TEXT,
    sort_order        INTEGER NOT NULL,
    FOREIGN KEY (template_id) REFERENCES mock_user_templates(id) ON DELETE CASCADE
);
```

#### 1.3.5 索引

```sql
CREATE INDEX IF NOT EXISTS idx_mock_tasks_time    ON mock_generation_tasks(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mock_columns_task  ON mock_generation_columns(task_id);
CREATE INDEX IF NOT EXISTS idx_mock_tpl_columns   ON mock_template_columns(template_id);
```

#### 1.3.6 ER 图

```
mock_generation_tasks                mock_generation_columns
┌──────────────────────┐             ┌──────────────────────────┐
│ id (PK)              │◄────────────│ task_id (FK, ON DELETE   │
│ table_name           │    1:N      │   CASCADE)               │
│ table_alias          │             │ id (PK)                  │
│ row_count            │             │ column_name              │
│ seed                 │             │ column_type              │
│ locale               │             │ generator                │
│ scene_id             │             │ generator_params (JSON)  │
│ save_format          │             │ null_ratio               │
│ status               │             │ is_unique                │
│ error_message        │             │ is_primary_key           │
│ generated_rows       │             │ is_foreign_key           │
│ generation_time_ms   │             │ ref_table / ref_column   │
│ created_at           │             │ comment                  │
│ updated_at           │             │ confidence               │
└──────────────────────┘             │ sort_order               │
                                     └──────────────────────────┘

mock_user_templates                 mock_template_columns
┌──────────────────────┐             ┌──────────────────────────┐
│ id (PK)              │◄────────────│ template_id (FK,         │
│ name                 │    1:N      │   ON DELETE CASCADE)     │
│ description          │             │ id (PK)                  │
│ row_count            │             │ (字段结构同上)            │
│ seed                 │             └──────────────────────────┘
│ locale               │
│ created_at           │
│ updated_at           │
└──────────────────────┘
```

### 1.4 Rust 数据结构

所有结构体定义在 `core/mock/persistence.rs`，使用 `serde` 序列化。

```rust
use serde::{Deserialize, Serialize};

/// 生成任务记录（对应 mock_generation_tasks 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockGenerationTask {
    pub id: String,
    pub table_name: String,
    pub table_alias: Option<String>,
    pub row_count: i64,
    pub seed: Option<i64>,
    pub locale: String,
    pub scene_id: Option<String>,
    pub save_format: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub generated_rows: Option<i64>,
    pub generation_time_ms: Option<i64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// 生成任务的字段配置（对应 mock_generation_columns 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockGenerationColumn {
    pub id: String,
    pub task_id: String,
    pub column_name: String,
    pub column_type: String,
    pub generator: String,
    pub generator_params: Option<String>,
    pub null_ratio: f64,
    pub is_unique: bool,
    pub is_primary_key: bool,
    pub is_foreign_key: bool,
    pub ref_table: Option<String>,
    pub ref_column: Option<String>,
    pub comment: Option<String>,
    pub confidence: Option<String>,
    pub sort_order: i64,
}

/// 用户自定义模板（对应 mock_user_templates 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockUserTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub row_count: i64,
    pub seed: Option<i64>,
    pub locale: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// 模板字段配置（对应 mock_template_columns 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockTemplateColumn {
    pub id: String,
    pub template_id: String,
    pub column_name: String,
    pub column_type: String,
    pub generator: String,
    pub generator_params: Option<String>,
    pub null_ratio: f64,
    pub is_unique: bool,
    pub is_primary_key: bool,
    pub is_foreign_key: bool,
    pub ref_table: Option<String>,
    pub ref_column: Option<String>,
    pub comment: Option<String>,
    pub confidence: Option<String>,
    pub sort_order: i64,
}
```

### 1.5 Store 模式设计

参照 `AnalyticsResourceStore` 模式，创建 `MockGenerationStore`：

```rust
use std::sync::Arc;
use crate::core::persistence::project_db::ProjectSqlitePool;
use crate::core::error::CoreError;

/// Mock 生成持久化 Store
pub struct MockGenerationStore {
    pool: Arc<ProjectSqlitePool>,
}

impl MockGenerationStore {
    /// 从连接池创建 Store
    pub fn new(pool: Arc<ProjectSqlitePool>) -> Self {
        Self { pool }
    }

    /// 保存生成任务及其字段配置
    pub fn save_task(
        &self,
        task: &MockGenerationTask,
        columns: &[MockGenerationColumn],
    ) -> Result<(), CoreError> { /* ... */ }

    /// 获取生成历史列表（按时间倒序）
    pub fn get_history(&self, limit: u32) -> Result<Vec<MockGenerationTask>, CoreError> { /* ... */ }

    /// 获取任务详情（含字段配置，按 sort_order 排序）
    pub fn get_detail(
        &self,
        task_id: &str,
    ) -> Result<(MockGenerationTask, Vec<MockGenerationColumn>), CoreError> { /* ... */ }

    /// 删除历史记录（级联删除字段配置）
    pub fn delete_task(&self, task_id: &str) -> Result<(), CoreError> { /* ... */ }

    /// 保存用户自定义模板
    pub fn save_template(
        &self,
        template: &MockUserTemplate,
        columns: &[MockTemplateColumn],
    ) -> Result<(), CoreError> { /* ... */ }

    /// 获取用户模板列表
    pub fn get_templates(&self) -> Result<Vec<MockUserTemplate>, CoreError> { /* ... */ }

    /// 获取模板详情
    pub fn get_template_detail(
        &self,
        template_id: &str,
    ) -> Result<(MockUserTemplate, Vec<MockTemplateColumn>), CoreError> { /* ... */ }
}
```

### 1.6 迁移策略

| 项目         | 内容                                                                                                                |
| ------------ | ------------------------------------------------------------------------------------------------------------------- |
| **迁移文件** | `migrations/project_meta/009_mock_generation.sql`                                                                   |
| **命名规范** | `NNN_description.sql`（NNN = 三位数字递增，当前 008→009）                                                           |
| **执行时机** | `ProjectDatabaseManager::open()` → `init_sqlite_tables()` → `MigrationManager::migrate(MigrationType::ProjectMeta)` |
| **幂等保证** | 全部使用 `CREATE TABLE IF NOT EXISTS` + `CREATE INDEX IF NOT EXISTS`                                                |
| **回滚策略** | 本项目不自动回滚；需要时可手动 `DROP TABLE IF EXISTS mock_*`                                                        |

### 1.7 与现有模块的关系

| 现有模块                         | 关系       | 操作                                                       |
| -------------------------------- | ---------- | ---------------------------------------------------------- |
| `core/mock/engine.rs`            | **不修改** | 生成引擎无变化                                             |
| `core/mock/models.rs`            | **不修改** | `MockConfig` / `ColumnDef` / `GeneratorConfig` 保持不变    |
| `core/mock/templates.rs`         | **互补**   | 内置模板（`builtin:*`）不变；新增用户自定义模板存储        |
| `core/mock/history.rs`           | **并行**   | DuckDB `_system.mock_history` 保留不动；新增 SQLite 持久化 |
| `commands/mock_commands.rs`      | **不修改** | 现有 13 个生成命令不变                                     |
| `core/persistence/project_db.rs` | **复用**   | `ProjectSqlitePool` 直接注入 Store                         |
| `adapters/tauri/state.rs`        | **不修改** | `AppState` 不变                                            |

---

## 二、开发文档

### 2.1 文件清单

#### 新增文件（3 个）

| #   | 文件路径                                          | 行数（估） | 职责                                          |
| --- | ------------------------------------------------- | ---------- | --------------------------------------------- |
| 1   | `migrations/project_meta/009_mock_generation.sql` | ~50        | 4 张表 + 3 索引 DDL                           |
| 2   | `core/mock/persistence.rs`                        | ~350       | 4 个 struct + `MockGenerationStore`（7 方法） |
| 3   | `commands/mock_persistence_commands.rs`           | ~120       | 5 个 Tauri 命令                               |

#### 修改文件（3 个）

| #   | 文件路径                  | 修改内容                                                   | 行数 |
| --- | ------------------------- | ---------------------------------------------------------- | ---- |
| 4   | `core/mock/mod.rs`        | 新增 `pub mod persistence;` + re-export 4 struct           | +4   |
| 5   | `lib.rs`                  | `pub mod mock_persistence_commands;` + `generate_handler!` | +5   |
| 6   | `core/persistence/mod.rs` | re-export `MockGenerationStore`（可选）                    | +1   |

#### 前端文件（不新增，修改现有 2 个）

| #   | 文件路径                     | 修改内容                          |
| --- | ---------------------------- | --------------------------------- |
| 7   | `src/shared/api/mock-api.ts` | 新增 5 个持久化 API 方法          |
| 8   | `src/stores/useMockStore.ts` | MockPanel 历史 Tab 调用持久化 API |

**合计**：新增 3 个 Rust 文件 + 修改 3 个 Rust 文件 + 修改 2 个 TS 文件 = **8 个文件**

### 2.2 开发阶段

| Phase    | 名称                | 任务                                                                   | 估时   |
| -------- | ------------------- | ---------------------------------------------------------------------- | ------ |
| P1       | SQL 建表            | 创建 `009_mock_generation.sql`，验证迁移系统能正常执行                 | 0.5h   |
| P2       | Rust 结构体 + Store | 创建 `persistence.rs`：4 struct + `MockGenerationStore`（7 CRUD 方法） | 2h     |
| P3       | Tauri 命令          | 创建 `mock_persistence_commands.rs`：5 命令 + `lib.rs` 注册            | 1h     |
| P4       | 模块注册 + 编译验证 | `mod.rs` 更新 + `cargo check` + 手动测试                               | 0.5h   |
| P5       | 前端 API 层         | `mock-api.ts` 新增方法 + `useMockStore.ts` 集成                        | 1h     |
| P6       | 前端 UI 集成        | MockPanel 历史 Tab 接入持久化 API + 降级兼容                           | 1h     |
| **合计** | —                   | —                                                                      | **6h** |

### 2.3 开工清单

按顺序执行：

- [ ] **P1.1** 创建 `migrations/project_meta/009_mock_generation.sql`（4 表 + 3 索引）
- [ ] **P2.1** 创建 `core/mock/persistence.rs`（4 struct 定义）
- [ ] **P2.2** 实现 `MockGenerationStore::save_task()`
- [ ] **P2.3** 实现 `MockGenerationStore::get_history()`
- [ ] **P2.4** 实现 `MockGenerationStore::get_detail()`
- [ ] **P2.5** 实现 `MockGenerationStore::delete_task()`
- [ ] **P2.6** 预留 `save_template()` / `get_templates()` / `get_template_detail()`（P6 后用）
- [ ] **P3.1** 创建 `commands/mock_persistence_commands.rs`
- [ ] **P3.2** 实现 `save_mock_generation_task`
- [ ] **P3.3** 实现 `get_mock_generation_history`
- [ ] **P3.4** 实现 `get_mock_generation_detail`
- [ ] **P3.5** 实现 `delete_mock_generation_task`
- [ ] **P3.6** 预留模板相关命令（`save_mock_template` 等）
- [ ] **P4.1** 修改 `core/mock/mod.rs`：新增 `pub mod persistence;`
- [ ] **P4.2** 修改 `lib.rs`：注册 `mock_persistence_commands` + `generate_handler!`
- [ ] **P4.3** `cargo check` 通过 + `cargo clippy` 无新增告警
- [ ] **P5.1** `mock-api.ts` 新增：`saveTask()`, `getHistory()`, `getDetail()`, `deleteTask()`
- [ ] **P5.2** `useMockStore.ts` 新增持久化相关 state + actions
- [ ] **P6.1** MockPanel 历史 Tab 替换数据源（DuckDB → SQLite）
- [ ] **P6.2** "确认生成"按钮成功后触发 `saveTask()`
- [ ] **P6.3** "重新生成"按钮调用 `getDetail()` 获取字段配置
- [ ] **P6.4** 降级兼容：持久化失败不影响生成流程

---

## 三、接口文档

### 3.1 Tauri Command 接口总览

| #   | 命令名                        | 方法   | 用途                     | 优先级        |
| --- | ----------------------------- | ------ | ------------------------ | ------------- |
| 1   | `save_mock_generation_task`   | POST   | 保存生成任务 + 字段配置  | 🔴 P0         |
| 2   | `get_mock_generation_history` | GET    | 获取生成历史列表         | 🔴 P0         |
| 3   | `get_mock_generation_detail`  | GET    | 获取任务详情（字段配置） | 🔴 P0         |
| 4   | `delete_mock_generation_task` | DELETE | 删除历史记录             | 🟡 P1         |
| 5   | `save_mock_template`          | POST   | 保存用户自定义模板       | 🟢 P2（预留） |
| 6   | `get_mock_templates`          | GET    | 获取用户模板列表         | 🟢 P2（预留） |
| 7   | `get_mock_template_detail`    | GET    | 获取模板详情             | 🟢 P2（预留） |

### 3.2 命令详细规格

#### 3.2.1 `save_mock_generation_task`

```
命令名：save_mock_generation_task
用途：保存一次生成任务的完整记录（任务元数据 + 每列生成器配置）
幂等性：每次调用生成新 UUID，不幂等
```

**请求**：

```rust
#[tauri::command]
pub async fn save_mock_generation_task(
    state: tauri::State<'_, AppState>,
    project_path: String,
    task: MockGenerationTask,
    columns: Vec<MockGenerationColumn>,
) -> Result<String, String>

// JavaScript 调用示例：
await invoke('save_mock_generation_task', {
    projectPath: '/path/to/project',
    task: {
        id: '550e8400-e29b-41d4-a716-446655440000',
        tableName: 'orders',
        tableAlias: 'temp_mock_orders_202605081430',
        rowCount: 10000,
        seed: 42,
        locale: 'ZH_CN',
        sceneId: 'builtin:ecommerce',
        saveFormat: 'table',
        status: 'success',
        generatedRows: 10000,
        generationTimeMs: 1523
    },
    columns: [
        {
            id: '660e8400-e29b-41d4-a716-446655440001',
            taskId: '550e8400-e29b-41d4-a716-446655440000',
            columnName: 'email',
            columnType: 'Varchar(100)',
            generator: 'SafeEmail(ZH_CN)',
            generatorParams: null,
            nullRatio: 0.0,
            isUnique: true,
            isPrimaryKey: false,
            isForeignKey: false,
            refTable: null,
            refColumn: null,
            comment: null,
            confidence: 'high',
            sortOrder: 0
        }
        // ... 更多列
    ]
})
```

**响应**：

| 场景           | 返回值                                      |
| -------------- | ------------------------------------------- |
| 成功           | `Ok(task.id)` — 返回任务 ID 字符串          |
| 项目不存在     | `Err("Project database not found: {path}")` |
| SQL 写入失败   | `Err("Failed to save task: {reason}")`      |
| 列配置写入失败 | `Err("Failed to save columns: {reason}")`   |

**实现逻辑**：

1. 打开项目 SQLite 连接（`project_path + "/.RSMETA/project.db"`）
2. 开启事务（`BEGIN TRANSACTION`）
3. 写入 `task` → `mock_generation_tasks`
4. 遍历 `columns` → 写入 `mock_generation_columns`
5. 提交事务（`COMMIT`）
6. 返回 `Ok(task.id)` 或错误信息

---

#### 3.2.2 `get_mock_generation_history`

```
命令名：get_mock_generation_history
用途：获取生成历史列表，按时间倒序
幂等性：GET 操作，幂等
```

**请求**：

```rust
#[tauri::command]
pub async fn get_mock_generation_history(
    state: tauri::State<'_, AppState>,
    project_path: String,
    limit: Option<u32>,
) -> Result<Vec<MockGenerationTask>, String>

// JavaScript 调用示例：
const history = await invoke('get_mock_generation_history', {
    projectPath: '/path/to/project',
    limit: 20  // 可选，默认 20
})
```

**响应**：

| 场景         | 返回值                                                    |
| ------------ | --------------------------------------------------------- |
| 成功         | `Ok(Vec<MockGenerationTask>)` — 按 `created_at DESC` 排序 |
| 项目不存在   | `Ok([])` — 返回空数组（不报错）                           |
| SQL 查询失败 | `Err("Failed to query history: {reason}")`                |

**实现逻辑**：

```sql
SELECT * FROM mock_generation_tasks
ORDER BY created_at DESC
LIMIT ?
```

---

#### 3.2.3 `get_mock_generation_detail`

```
命令名：get_mock_generation_detail
用途：获取任务详情 + 关联字段配置（用于"重新生成"）
幂等性：GET 操作，幂等
```

**请求**：

```rust
#[tauri::command]
pub async fn get_mock_generation_detail(
    state: tauri::State<'_, AppState>,
    project_path: String,
    task_id: String,
) -> Result<MockGenerationDetail, String>
```

**响应数据结构**：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockGenerationDetail {
    pub task: MockGenerationTask,
    pub columns: Vec<MockGenerationColumn>,
}
```

| 场景         | 返回值                                                             |
| ------------ | ------------------------------------------------------------------ |
| 成功         | `Ok(MockGenerationDetail)` — task + columns（按 `sort_order ASC`） |
| 任务不存在   | `Err("Task not found: {task_id}")`                                 |
| SQL 查询失败 | `Err("Failed to query detail: {reason}")`                          |

**实现逻辑**：

1. 根据 `task_id` 查询 `mock_generation_tasks`
2. 查询关联的 `mock_generation_columns`，按 `sort_order ASC` 排序
3. 组装 `MockGenerationDetail` 返回

---

#### 3.2.4 `delete_mock_generation_task`

```
命令名：delete_mock_generation_task
用途：删除历史任务（级联删除列配置）
幂等性：删除已不存在的记录返回成功
```

**请求**：

```rust
#[tauri::command]
pub async fn delete_mock_generation_task(
    state: tauri::State<'_, AppState>,
    project_path: String,
    task_id: String,
) -> Result<(), String>
```

**响应**：

| 场景         | 返回值                                   |
| ------------ | ---------------------------------------- |
| 成功         | `Ok(())`                                 |
| 任务不存在   | `Ok(())` — 幂等，不报错                  |
| SQL 删除失败 | `Err("Failed to delete task: {reason}")` |

**实现逻辑**：

```sql
DELETE FROM mock_generation_tasks WHERE id = ?
-- mock_generation_columns 由 ON DELETE CASCADE 自动删除
```

---

#### 3.2.5 模板相关命令（预留，P2 实现）

```rust
// 保存用户自定义模板
#[tauri::command]
pub async fn save_mock_template(
    state: tauri::State<'_, AppState>,
    project_path: String,
    template: MockUserTemplate,
    columns: Vec<MockTemplateColumn>,
) -> Result<String, String>

// 获取用户模板列表
#[tauri::command]
pub async fn get_mock_templates(
    state: tauri::State<'_, AppState>,
    project_path: String,
) -> Result<Vec<MockUserTemplate>, String>

// 获取模板详情
#[tauri::command]
pub async fn get_mock_template_detail(
    state: tauri::State<'_, AppState>,
    project_path: String,
    template_id: String,
) -> Result<(MockUserTemplate, Vec<MockTemplateColumn>), String>
```

### 3.3 前端 API 层

在现有 `src/shared/api/mock-api.ts` 中新增方法：

```typescript
// ==================== 持久化 API ====================

/** 保存生成任务 */
async function saveTask(
  projectPath: string,
  task: MockTaskInput,
  columns: MockColumnInput[]
): Promise<string> {
  return invoke('save_mock_generation_task', {
    projectPath,
    task: {
      id: task.id,
      tableName: task.tableName,
      tableAlias: task.tableAlias ?? null,
      rowCount: task.rowCount,
      seed: task.seed ?? null,
      locale: task.locale,
      sceneId: task.sceneId ?? null,
      saveFormat: task.saveFormat ?? null,
      status: task.status,
      errorMessage: task.errorMessage ?? null,
      generatedRows: task.generatedRows ?? null,
      generationTimeMs: task.generationTimeMs ?? null,
    },
    columns: columns.map(c => ({
      id: c.id,
      taskId: task.id,
      columnName: c.columnName,
      columnType: c.columnType,
      generator: c.generator,
      generatorParams: c.generatorParams ?? null,
      nullRatio: c.nullRatio ?? 0,
      isUnique: c.isUnique ?? false,
      isPrimaryKey: c.isPrimaryKey ?? false,
      isForeignKey: c.isForeignKey ?? false,
      refTable: c.refTable ?? null,
      refColumn: c.refColumn ?? null,
      comment: c.comment ?? null,
      confidence: c.confidence ?? null,
      sortOrder: c.sortOrder,
    })),
  })
}

/** 获取生成历史列表 */
async function getHistory(projectPath: string, limit?: number): Promise<MockGenerationTask[]> {
  return invoke('get_mock_generation_history', {
    projectPath,
    limit: limit ?? 20,
  })
}

/** 获取任务详情 */
async function getDetail(
  projectPath: string,
  taskId: string
): Promise<{ task: MockGenerationTask; columns: MockGenerationColumn[] }> {
  return invoke('get_mock_generation_detail', {
    projectPath,
    taskId,
  })
}

/** 删除历史任务 */
async function deleteTask(projectPath: string, taskId: string): Promise<void> {
  return invoke('delete_mock_generation_task', {
    projectPath,
    taskId,
  })
}
```

### 3.4 前端集成要点

#### 3.4.1 数据流时序

```
用户点击 "🚀 生成 Mock 数据"
  │
  ├─ 1. mockApi.generate(config)         ← 现有流程（不变）
  │    └── MockEngine::generate(config)
  │         ├── DuckDB 临时表创建
  │         ├── 批量生成 INSERT
  │         └── 返回 MockGenerateResult
  │
  ├─ 2. MockPanel 展示预览 + elapsed_ms  ← 现有流程（不变）
  │
  └─ 3. 🆕 mockApi.saveTask(projectPath, task, columns)
       └── MockGenerationStore::save_task()
            ├── BEGIN TRANSACTION
            ├── INSERT mock_generation_tasks
            ├── INSERT mock_generation_columns × N
            └── COMMIT
                │ 成功 → 静默（不打断用户）
                │ 失败 → console.warn（降级，不阻塞流程）
```

#### 3.4.2 MockPanel 历史 Tab 改造

```
当前：
  MockPanel → 历史面板
    └── mockApi.getHistory()        ← DuckDB _system.mock_history
    └── mockApi.reGenerate(id)      ← 从 DuckDB 重新生成

改造后：
  MockPanel → 历史面板
    ├── 🆕 mockApi.getHistoryV2(projectPath, 20)   ← SQLite
    ├── 🆕 mockApi.getDetail(projectPath, taskId)   ← 点击"重新生成"
    └── 🆕 mockApi.deleteTask(projectPath, taskId)  ← 删除
```

#### 3.4.3 `projectPath` 获取

```typescript
// 从现有的 project store 获取当前项目路径
const projectStore = useProjectStore()
const projectPath = projectStore.currentProject?.path
// 或
const { currentPath } = storeToRefs(projectStore)
```

#### 3.4.4 错误降级策略

```typescript
async function onGenerateSuccess(result: MockGenerateResult) {
  // 主流程：更新预览
  previewData.value = result.preview

  // 持久化：非阻塞，失败不影响主流程
  try {
    if (projectPath.value) {
      await mockApi.saveTask(projectPath.value, buildTask(result), buildColumns(config))
    }
  } catch (e) {
    console.warn('Mock 持久化失败（不影响生成结果）:', e)
    // 可选：NNotification.warning('历史记录保存失败')
  }
}
```

### 3.5 数据流图

```
┌──────────────────────────────────────────────────────────┐
│                      生成流程                             │
│                                                          │
│  MockPanel ──► useMockStore ──► mockApi.generate()       │
│                                      │                   │
│                              Tauri IPC │                  │
│                                      ▼                   │
│                              mock_generate                │
│                                      │                   │
│                              MockEngine::generate()       │
│                                      │                   │
│                              DuckDB temp_mock_*           │
│                                      │                   │
│                              返回 MockGenerateResult      │
│                                      │                   │
│  ◄── 预览显示 ──── store.previewData ◄┘                  │
│                                                          │
└──────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────┐
│                    🆕 持久化流程                           │
│                                                          │
│  MockPanel                                              │
│    │                                                     │
│    ├── onGenerateSuccess()                              │
│    │     └──🆕 mockApi.saveTask(path, task, cols)        │
│    │              │                                      │
│    │      Tauri IPC │                                    │
│    │              ▼                                      │
│    │      save_mock_generation_task                      │
│    │              │                                      │
│    │      MockGenerationStore::save_task()               │
│    │              │                                      │
│    │      SQLite mock_generation_tasks                   │
│    │      SQLite mock_generation_columns                 │
│    │              │                                      │
│    │      返回 task.id 或 静默失败                        │
│    │                                                     │
│    ├── 历史 Tab 展开                                     │
│    │     └──🆕 mockApi.getHistoryV2(path, 20)            │
│    │              │                                      │
│    │      SQLite SELECT ... ORDER BY created_at DESC     │
│    │              │                                      │
│    │      返回 Vec<MockGenerationTask>                   │
│    │                                                     │
│    └── 点击 "重新生成"                                    │
│          └──🆕 mockApi.getDetail(path, taskId)           │
│                   │                                      │
│           SQLite JOIN mock_generation_tasks + columns     │
│                   │                                      │
│           返回 (task, columns)                            │
│                   │                                      │
│           填充到 MockPanel 列编辑表单                     │
│                   │                                      │
│           用户修改 → 点击 "🚀 生成" → 回到生成流程        │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

---

## 附录 A：依赖清单

| 依赖         | 版本   | 用途                           | 来源                        |
| ------------ | ------ | ------------------------------ | --------------------------- |
| `uuid`       | 1.x    | 生成主键 UUID v4               | Cargo.toml（现有）          |
| `rusqlite`   | 0.32.x | SQLite 操作                    | Cargo.toml（现有，bundled） |
| `serde`      | 1.x    | JSON 序列化                    | Cargo.toml（现有）          |
| `serde_json` | 1.x    | `generator_params` JSON 字符串 | Cargo.toml（现有）          |
| `tauri`      | 2.10.x | Command + State 注入           | Cargo.toml（现有）          |

## 附录 B：与主设计文档的关联

| 主文档章节                                                                    | 关联方式                                          |
| ----------------------------------------------------------------------------- | ------------------------------------------------- |
| [§4 Tauri Command 接口](./mock-data-generator-design.md#4-tauri-command-接口) | 原 13 个命令不动；新增 5+ 持久化命令（本文 §3.2） |
| [§5 前端组件设计](./mock-data-generator-design.md#5-前端组件设计)             | MockPanel 历史 Tab 数据源切换（本文 §3.4）        |
| [§8 开发阶段计划](./mock-data-generator-design.md#8-分阶段开发计划)           | Phase 11 持久化层（本文 §2.2）                    |
| [§11 前后端打通分析](./mock-data-generator-design.md#11-前后端打通与入口分析) | 新增持久化命令的全链路覆盖                        |

## 附录 C：版本历史

| 版本 | 日期       | 说明                                               |
| ---- | ---------- | -------------------------------------------------- |
| v1.1 | 2026-05-09 | 开发完成：Phase 11 全部实现（7 命令 + 前后端集成） |
