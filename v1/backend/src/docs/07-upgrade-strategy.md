# 升级策略

> 版本：v1.0
> 最后更新：2026-05-09
> 状态：✅ 当前策略

## 概述

RdataStation 遵循 **语义化版本控制（SemVer）** ，采用分层升级策略，确保核心接口 10 年向前兼容，同时允许依赖安全更新和功能增强。

## 一、版本策略

### SemVer 语义

```
MAJOR.MINOR.PATCH
  │     │     └── Bug 修复、安全补丁（向后兼容）
  │     └──────── 新功能、功能增强（向后兼容）
  └────────────── 不兼容的 API 变更
```

### 项目约束

| 约束项           | 策略                      |
| ---------------- | ------------------------- |
| **Rust Edition** | ✅ 允许跨 Edition 升级    |
| **主版本升级**   | ❌ 禁止（避免破坏兼容性） |
| **Minor 升级**   | ✅ 允许（功能增强）       |
| **Patch 升级**   | ✅ 允许（安全/性能修复）  |
| **API 兼容性**   | ✅ 必须保持 10 年向前兼容 |

## 二、分层升级策略

### Rust Core 层

| 依赖库        | 当前版本  | 升级策略                      |
| ------------- | --------- | ----------------------------- |
| **Tokio**     | 1.44.x    | ✅ minor/patch，❌ major      |
| **Tauri**     | 2.10.x    | ✅ patch only，❌ minor/major |
| **sqlx**      | 0.8.x     | ✅ patch only，❌ minor/major |
| **wasmtime**  | 43.0.x    | ✅ minor/patch，❌ major      |
| **rusqlite**  | 0.32.x    | ✅ minor/patch，❌ major      |
| **duckdb-rs** | 1.10502.x | ✅ patch only，❌ minor/major |
| **Arrow**     | 53.0.x    | ✅ minor/patch，❌ major      |
| **Serde**     | 1.0.x     | ✅ minor/patch                |
| **thiserror** | 1.0.x     | ✅ minor/patch                |
| **anyhow**    | 1.0.x     | ✅ minor/patch                |

### 前端层

| 依赖库            | 当前版本 | 升级策略                 |
| ----------------- | -------- | ------------------------ |
| **Vue**           | 3.5.x    | ✅ minor/patch，❌ major |
| **TypeScript**    | 5.8.x    | ✅ minor/patch           |
| **Vite**          | 6.x      | ✅ minor/patch，❌ major |
| **naive-ui**      | latest   | ✅ minor/patch           |
| **AG Grid**       | 33.x     | ✅ minor/patch，❌ major |
| **Monaco Editor** | 0.52.x   | ✅ minor/patch           |
| **Pinia**         | 2.3.x    | ✅ minor/patch           |
| **dockview-vue**  | 5.2.x    | ✅ minor/patch，❌ major |

### Plugin / WASM 层

| 依赖库           | 当前版本 | 升级策略                 |
| ---------------- | -------- | ------------------------ |
| **WASI**         | 0.2.x    | ✅ minor                 |
| **wasmtime**     | 43.0.x   | ✅ minor/patch，❌ major |
| **Arrow (WASM)** | 53.0.x   | ✅ minor/patch           |
| **wasi-python**  | 0.12.x   | ✅ minor/patch           |

## 三、依赖升级命令规范

### Rust 后端

```bash
# ✅ 允许：精确升级单个依赖
cargo update -p tokio          # 升级 tokio 到最新兼容版本
cargo update -p serde          # 升级 serde 到最新兼容版本

# ❌ 禁止：全局升级所有依赖
cargo update                    # 可能引入不兼容变更
```

### 前端

```bash
# ✅ 允许：精确升级单个包
pnpm add vue@3.5.14            # 升级 Vue 到指定 patch 版本
pnpm add ag-grid-community@33.1.0

# ❌ 禁止：使用 npm/yarn
npm install                     # 违反 pnpm 约束
```

## 四、发布流程

### 版本号决策树

```
变更类型？
├── Bug 修复（不改变 API）
│   └── PATCH bump: 1.2.3 → 1.2.4
│
├── 新功能（向后兼容）
│   └── MINOR bump: 1.2.3 → 1.3.0
│
└── API 不兼容变更
    └── MAJOR bump: 1.2.3 → 2.0.0
        ├── 需提供迁移指南
        └── 需评估影响范围
```

### 发布检查清单

在发布新版本前，需完成：

- [ ] `cargo clippy -- -D warnings` 通过
- [ ] `cargo fmt --all -- --check` 通过
- [ ] `pnpm run lint` 通过
- [ ] `pnpm run format` 通过
- [ ] 所有单元测试通过
- [ ] `QueryResult` 内部仍包含 `RecordBatch`
- [ ] `Database::query()` 签名未变化
- [ ] `DbPool::acquire()` 返回 `Box<dyn Database>` 而非具体类型
- [ ] 启动速度 < 1.5 秒
- [ ] 内存占用 < 150MB（核心）

## 五、接口兼容性保障

### 不可变的宪法接口

以下接口**禁止修改签名**（位于 [traits.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/traits.rs)）：

```rust
// ❌ 禁止修改以下 trait 的方法签名
pub trait Database: Send + Sync {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError>;
    async fn query_with_params(...) -> Result<QueryResult, CoreError>;
    async fn query_with_cancel(...) -> Result<QueryResult, CoreError>;
    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, CoreError>;
    fn meta(&self) -> DataSourceMeta;
    async fn list_databases(&self) -> Result<Vec<String>, CoreError>;
    async fn list_schemas(&self, db: &str) -> Result<Vec<String>, CoreError>;
    async fn list_tables(...) -> Result<Vec<SchemaObject>, CoreError>;
    async fn list_columns(...) -> Result<Vec<SchemaObject>, CoreError>;
}

pub trait DbPool: Send + Sync {
    async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError>;
    async fn close(&self) -> Result<(), CoreError>;
    fn is_closed(&self) -> bool;
    fn status(&self) -> PoolStatus;
}
```

### 可扩展的扩展点

新功能应通过以下方式添加，而非修改现有接口：

1. **新增 Trait**：如 `MetadataBrowser`、`QueryRouter`
2. **默认实现**：给 `Database` trait 添加带默认实现的新方法
3. **新模块**：在 `core/` 下新增独立模块
4. **新字段**：给 struct 添加 `Option` 类型字段（向后兼容）

### IPC 接口约束

```
Database::query()
    ↓
QueryResult
└─ batches: Vec<RecordBatch>  ✅  必须保持
    ↓
Tauri Command
    ↓
JSON (自动序列化)
```

❌ **禁止**在 IPC 层做转换（直接在 Tauri Command 中返回 `Result<T, Error>`）。

## 六、存储迁移策略

### 数据库 Schema 迁移

使用 `MigrationManager` 统一管理 4 种数据库的 schema 迁移：

```rust
pub enum MigrationType {
    GlobalSqlite,    // system/global.db
    GlobalDuckDB,    // system/analytics/global.duckdb
    ProjectSqlite,   // {project}/meta/project.db
    ProjectDuckDB,   // {project}/analytics/data.duckdb
}
```

**迁移原则**：

1. **增量迁移**：每个版本只写增量 SQL
2. **不可逆检查**：迁移前检查目标版本是否高于当前版本
3. **失败回滚**：迁移失败不破坏现有数据
4. **幂等性**：相同版本迁移重复执行无副作用

### 缓存版本管理

**路径**: [cache_version_migration.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/cache_version_migration.rs)

```
cache_version 表:
├── conn_id
├── schema_version
├── last_warmed_at
└── checksum

迁移触发条件：
1. schema_version < 当前代码版本 → 执行迁移
2. checksum 不匹配 → 标记缓存失效
3. 用户手动刷新 → 重新预热
```

## 七、插件兼容性（未来）

### 插件版本契约

```
Plugin SDK Version → Plugin API Version
       │                    │
       ├── 1.x 兼容  →  API v1
       ├── 2.x 兼容  →  API v2
       └── N.x 兼容  →  API vN
```

### WASM 接口版本化

```rust
// 插件 manifest (plugin.toml)
[plugin]
api_version = "1"          # 目标 API 版本
min_runtime = "1.0.0"      # 最低运行时版本
max_runtime = "2.0.0"      # 最高运行时版本（不含）
```

## 八、技术栈生命周期

| 阶段         | 时间范围 | 策略                |
| ------------ | -------- | ------------------- |
| **MVP**      | 当前     | 4 种内置数据库稳定  |
| **扩展期**   | MVP 后   | JDBC/ODBC/WASM 插件 |
| **成熟期**   | 扩展后   | DuckLake 网络存储   |
| **长期维护** | 10 年    | API 不变，安全更新  |

## 九、升级前兼容性测试

### 必测场景

```bash
# 1. 依赖升级后编译检查
cargo check --all-targets

# 2. 接口兼容性检查
# 确保以下类型签名未变化
cargo rustdoc -- -Z unstable-options --output-format json

# 3. 存储迁移测试
# 用旧版本数据启动，检查迁移是否成功

# 4. 前端构建测试
pnpm run build
```

### 回归测试范围

| 测试类型    | 范围                                      |
| ----------- | ----------------------------------------- |
| 连接创建    | MySQL / PostgreSQL / SQLite / DuckDB 全量 |
| SQL 执行    | 基本查询 / 参数化查询 / 事务              |
| Schema 浏览 | 数据库 → Schema → 表 → 列                 |
| 历史记录    | SQL 历史 CRUD                             |
| 项目管理    | 创建 / 打开 / 关闭                        |
| 元数据缓存  | 预热 / 查询 / 失效                        |
