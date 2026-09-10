# RdataStation Code Wiki

> 最后更新: 2026-05-20
>
> 版本: v0.5.0

---

## 目录

1. [项目概述](#项目概述)
2. [技术栈](#技术栈)
3. [整体架构](#整体架构)
4. [后端架构（Rust Core）](#后端架构rust-core)
5. [前端架构（Vue 3）](#前端架构vue-3)
6. [核心模块详解](#核心模块详解)
7. [数据库设计](#数据库设计)
8. [插件系统](#插件系统)
9. [开发指南](#开发指南)
10. [常见问题](#常见问题)

---

## 项目概述

### 什么是 RdataStation?

RdataStation 是一个**本地优先、不上云**的数据库桌面管理工具。它专注于解决"SQL查询结果之后的分析"问题，将数据库连接、SQL编辑、查询结果展示与深度分析（基于 DuckDB）无缝集成在一个工具中。

### 核心特性

- **本地优先**: 所有数据存储在本地，无云同步、无遥测
- **DuckDB 分析引擎**: 内置 DuckDB 作为分析引擎，支持对查询结果进行二次分析
- **联邦查询**: 跨多个数据库源的联合查询能力
- **项目隔离**: 四层架构（系统级、项目级、连接级）确保数据安全隔离
- **插件生态**: 支持 WASM 轻量级插件和 Sidecar 重量级扩展
- **元数据缓存**: 智能缓存机制，提升导航性能

### 设计哲学

```
取数立本，分析明道；数不虚取，析不妄断。
```

### 项目状态

🟡 **早期开发阶段** - 核心架构已完成，功能迭代中

---

## 技术栈

### 后端 (Rust)

| 技术       | 版本         | 用途                  |
| ---------- | ------------ | --------------------- |
| Rust       | 2021 Edition | 主要开发语言          |
| Tokio      | 1.44.1       | 异步运行时            |
| Tauri      | 2.10.3       | 桌面应用框架          |
| SQLx       | 0.8.3        | MySQL/PostgreSQL 驱动 |
| Rusqlite   | 0.32.1       | SQLite 驱动           |
| DuckDB-rs  | 1.10502.0    | DuckDB 官方驱动       |
| Arrow      | 58.1.0       | 数据传输格式          |
| Extism     | 1.21.0       | WASM 插件运行时       |
| Russh      | 0.49.0       | SSH 隧道支持          |
| Native-TLS | 0.2.13       | SSL/TLS 支持          |

### 前端 (Vue 3)

| 技术         | 版本   | 用途         |
| ------------ | ------ | ------------ |
| Vue          | 3.5.x  | UI 框架      |
| TypeScript   | 6.0.x  | 类型系统     |
| Vite         | 8.x    | 构建工具     |
| Pinia        | 3.0.x  | 状态管理     |
| Naive UI     | 2.44.x | UI 组件库    |
| Dockview-vue | 6.1.x  | IDE 布局引擎 |
| AG Grid      | 35.3.x | 数据表格     |
| CodeMirror 6 | 6.x    | 代码编辑器   |
| ECharts      | 6.x    | 数据可视化   |

---

## 整体架构

### 四层微内核架构

```
┌─────────────────────────────────────────────────────────┐
│                    UI Layer (Vue 3)                     │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   Workbench  │  │  Extensions  │  │   Settings   │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────┐
│                 Tauri Adapter Layer                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   Commands   │  │    Events    │  │    State     │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────┐
│                  Rust Core Layer                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │    Driver    │  │     DBI      │  │   Services   │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │    Cache     │  │   DuckDB     │  │ Persistence  │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────┐
│                  Data Layer                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │    SQLite    │  │    DuckDB    │  │  File System │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### 数据分层策略

| 数据类型   | 存储引擎            | 用途                           |
| ---------- | ------------------- | ------------------------------ |
| 事务性     | SQLite              | 连接信息、SQL 历史、项目元数据 |
| 分析性     | DuckDB              | 查询结果、数据分析、临时表     |
| 配置性     | JSON                | 用户偏好、编辑器设置           |
| 元数据缓存 | SQLite (每连接独立) | 表结构、列信息、索引           |

### 项目组织结构

```
RdataStation/
├── src-tauri/              # Rust 后端
│   ├── src/
│   │   ├── core/           # 核心业务逻辑
│   │   ├── adapters/       # 适配层 (Tauri/WASM/Sidecar)
│   │   ├── commands/       # Tauri 命令
│   │   └── api/            # API 定义
│   ├── migrations/         # 数据库迁移
│   ├── insight-rules/      # 数据分析规则
│   └── Cargo.toml
├── src/                    # Vue 3 前端
│   ├── app/                # 应用入口
│   ├── core/               # 核心框架
│   ├── extensions/         # 扩展（内置插件）
│   ├── shared/             # 共享组件
│   └── adapters/           # 适配器
├── docs/                   # 文档
└── package.json
```

---

## 后端架构（Rust Core）

### Core 模块组织

核心模块位于 [`src-tauri/src/core/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/)：

```
core/
├── mod.rs                  # 核心模块入口，统一导出
├── driver/                 # 数据库驱动抽象与实现
│   ├── traits.rs           # 核心 Trait 定义 (Database, Transaction)
│   ├── native/             # 原生驱动实现
│   │   ├── mysql.rs
│   │   ├── postgres.rs
│   │   ├── sqlite.rs
│   │   └── duckdb.rs
│   ├── connection/         # 连接管理
│   │   ├── config.rs       # 连接配置 (SSL/SSH/Proxy)
│   │   ├── connector.rs    # 连接器
│   │   └── factory.rs      # 连接工厂
│   ├── registry/           # 驱动注册
│   └── router.rs           # 数据源路由
├── dbi/                    # 统一数据访问接口
├── duckdb/                 # DuckDB 分析引擎集成
├── cache/                  # 多级缓存系统
│   ├── metadata_cache.rs   # 元数据缓存
│   ├── lru_cache.rs        # LRU 缓存
│   └── cache_manager.rs    # 缓存管理器
├── persistence/            # 持久化层
│   ├── connection_store.rs # 连接存储
│   └── analytics_resource_store/ # 分析资源存储
├── project/                # 项目管理
├── insight/                # 数据洞察
├── logging/                # 日志系统
├── migration/              # 数据库迁移
├── plugin/                 # 插件系统
├── error.rs                # 统一错误类型
└── models.rs               # 核心数据模型
```

### 核心 Trait 定义 ([`driver/traits.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/driver/traits.rs))

#### Database Trait

所有数据库驱动必须实现的核心 trait：

```rust
#[async_trait::async_trait]
pub trait Database: Send + Sync {
    /// 执行 SQL 查询，返回 Arrow 格式结果
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError>;

    /// 执行非查询 SQL (INSERT/UPDATE/DELETE)
    async fn execute(&self, sql: &str) -> Result<u64, CoreError>;

    /// 开始事务
    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, CoreError>;

    /// 获取元数据浏览器
    fn metadata_browser(&self) -> Option<&dyn MetadataBrowser>;
}
```

#### MetadataBrowser Trait

提供统一的数据库元数据浏览能力：

```rust
#[async_trait::async_trait]
pub trait MetadataBrowser: Send + Sync {
    async fn get_catalogs(&self) -> Result<Vec<NodeInfo>, CoreError>;
    async fn get_schemas(&self, catalog: &str) -> Result<Vec<NodeInfo>, CoreError>;
    async fn get_tables(&self, catalog: &str, schema: &str) -> Result<Vec<NodeInfo>, CoreError>;
    async fn get_table_detail(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<NodeDetail, CoreError>;
}
```

### 连接管理架构

#### 四层连接配置 ([`driver/connection/config.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/driver/connection/config.rs))

支持多种连接方式：

1. **直连**: 直接连接数据库
2. **SSL/TLS**: 加密连接
3. **SSH 隧道**: 通过 SSH 跳板机连接
4. **SOCKS/HTTP 代理**: 通过代理连接

```rust
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: Option<String>,
    pub connection_method: ConnectionMethod,
}

pub enum ConnectionMethod {
    Direct,
    Ssl(SslConfig),
    Ssh(SshConfig),
    Proxy(ProxyConfig),
}
```

### DuckDB 分析引擎 ([`core/duckdb/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/duckdb/))

DuckDB 在 RdataStation 中扮演多重角色：

1. **临时表管理**: 查询结果自动转为 DuckDB 临时表
2. **二次分析**: 对临时表执行聚合、窗口函数等复杂查询
3. **联邦查询**: 跨多个数据源的联合查询
4. **数据洞察**: 快速生成列统计信息

核心组件：

- [`DuckDBManager`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/duckdb/manager.rs): DuckDB 生命周期管理
- [`TempTableManager`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/duckdb/temp_table.rs): 临时表管理
- [`FederationManager`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/duckdb/federation.rs): 联邦查询

### 缓存系统 ([`core/cache/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/cache/))

多级缓存策略：

```
L1: 内存 LRU Cache (快速访问)
    ↓
L2: SQLite 元数据缓存 (持久化)
    ↓
L3: 源数据库 (原始数据)
```

核心组件：

- [`MetadataCache`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/cache/metadata_cache.rs): 元数据缓存
- [`CacheManager`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/cache/cache_manager.rs): 统一缓存管理
- 支持增量更新、缓存预热、版本迁移

### Tauri 命令 ([`commands/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/commands/))

所有前端调用的后端接口均通过 Tauri 命令暴露：

| 命令模块                                                                                                                                                         | 功能       |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| [`connection_commands.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/commands/connection_commands.rs)                 | 连接管理   |
| [`sql_commands.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/commands/sql_commands.rs)                               | SQL 执行   |
| [`metadata_commands.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/commands/metadata_commands.rs)                     | 元数据访问 |
| [`project_commands.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/commands/project_commands.rs)                       | 项目管理   |
| [`analytics_resource_commands.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/commands/analytics_resource_commands.rs) | 分析资源   |

---

## 前端架构（Vue 3）

### 前端目录结构

```
src/
├── app/                    # 应用入口
│   ├── App.vue
│   ├── MainLayout.vue
│   ├── main.ts
│   └── router.ts
├── core/                   # 核心框架
│   ├── project/            # 项目核心
│   │   ├── stores/         # Pinia 存储
│   │   └── types.ts        # 类型定义
│   ├── extension-host.ts   # 扩展宿主
│   ├── panel-registry.ts   # 面板注册
│   └── command-registry.ts # 命令注册
├── extensions/             # 扩展（内置插件）
│   ├── builtin/
│   │   ├── workbench/      # 工作台
│   │   ├── connection/     # 连接管理
│   │   ├── database/       # 数据库导航
│   │   ├── query/          # 查询执行
│   │   ├── analytics-resource/ # 分析资源
│   │   ├── scratchpad/     # 草稿箱
│   │   └── settings/       # 设置
│   └── extension.ts
├── shared/                 # 共享组件
│   └── components/
└── adapters/               # 适配器
```

### 核心扩展系统

RdataStation 采用插件化架构，所有功能都作为扩展实现：

```typescript
// 扩展清单结构 (rdata-plugin.toml)
;[plugin]
name = 'workbench'
version = '0.1.0'
description = '工作台核心扩展'[contributes.panels]
'sql-editor' = { component = 'EditorPanel' }
'query-result' = { component = 'QueryResultPanel' }[contributes.commands]
'sql.execute' = { handler = 'executeSql' }
```

### 状态管理（Pinia）

主要 Store：

| Store                  | 位置                                                                                                                                                                                                                                    | 用途         |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| ProjectStore           | [`core/project/stores/project.ts`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/core/project/stores/project.ts)                                                                                           | 项目状态管理 |
| ConnectionStore        | [`extensions/builtin/connection/ui/stores/connection-store.ts`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/connection/ui/stores/connection-store.ts)                                 | 连接状态     |
| DatabaseNavigatorStore | [`extensions/builtin/database/ui/stores/database-navigator-store.ts`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/database/ui/stores/database-navigator-store.ts)                     | 数据库导航   |
| AnalyticsResourceStore | [`extensions/builtin/analytics-resource/ui/stores/analytics-resource-store.ts`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/analytics-resource/ui/stores/analytics-resource-store.ts) | 分析资源     |

### Dockview 布局系统

使用 [`dockview-vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/core/dockview-types.ts) 实现 VS Code 风格的布局：

```
┌─────────────────────────────────────────────────────────┐
│  Title Bar (Menu + Project Switcher + Settings)         │
├──────────┬──────────────────────────┬───────────────────┤
│          │                          │                   │
│  Left    │    Center Area           │   Right Sidebar   │
│ Sidebar  │   (Panels/Tabs)          │                   │
│          │                          │                   │
├──────────┴──────────────────────────┴───────────────────┤
│  Bottom Panel (Output / Logs / ...)                     │
└─────────────────────────────────────────────────────────┘
```

### 主要内置扩展

#### 1. Workbench Extension ([`extensions/builtin/workbench/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/workbench/))

核心工作台，包含：

- SQL 编辑器面板 ([`EditorPanel.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorPanel.vue))
- 查询结果面板 ([`QueryResultPanel.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/workbench/ui/components/panels/QueryResultPanel.vue))
- 数据洞察面板 ([`ColumnInsightsPanel.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/workbench/ui/components/panels/ColumnInsightsPanel.vue))
- 工作台标题栏 ([`WorkbenchTitleBar.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/workbench/ui/components/WorkbenchTitleBar.vue))

#### 2. Connection Extension ([`extensions/builtin/connection/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/connection/))

连接管理，包含：

- 添加数据源对话框 ([`AddDataSourceDialog.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue))
- 网络配置标签页 ([`NetworkTab.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/connection/ui/components/tabs/NetworkTab.vue))
- 动态表单渲染 ([`DynamicFormRenderer.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/connection/ui/components/DynamicFormRenderer.vue))

#### 3. Database Extension ([`extensions/builtin/database/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/database/))

数据库导航，包含：

- 虚拟树组件 ([`virtual-tree.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/database/ui/components/virtual-tree.vue))
- 数据预览 ([`DataPreview.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/database/ui/components/data-preview/DataPreview.vue))
- 缓存预热状态 ([`cache-warming-status.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/database/ui/components/cache-warming-status.vue))

---

## 核心模块详解

### 项目管理模块

#### 项目结构

```
{project_path}/
├── meta/
│   ├── project.db              # SQLite 项目元数据库
│   └── connection_metadata/    # 连接元数据缓存
│       └── conn_{id}.sqlite
├── analytics/
│   └── data.duckdb             # DuckDB 分析数据库
└── config/
    ├── connections.json        # 连接配置
    ├── settings.json           # 项目设置
    └── layout.json             # 工作台布局
```

#### ProjectStore 状态

```typescript
interface ProjectState {
  currentProject: Project | null
  connections: Connection[]
  activeConnectionId: string | null
  workbenchState: WorkbenchState
}
```

### 查询结果处理

#### 三种结果模式

1. **即时过滤**: 前端过滤，零延迟（适合小数据量）
2. **SQL 过滤**: 拼接 WHERE 子句重新查询（类似 DBeaver）
3. **DuckDB 分析**: 转入 DuckDB 临时表，支持复杂分析

#### QueryResultPanel ([`extensions/builtin/workbench/ui/components/panels/QueryResultPanel.vue`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/workbench/ui/components/panels/QueryResultPanel.vue))

核心功能：

- AG Grid 数据展示
- 快速搜索与过滤
- DuckDB 分析输入框
- 列统计信息展示
- 结果持久化

### 元数据缓存系统

#### 缓存策略

- **懒加载**: 按需加载，不一次性加载所有元数据
- **增量更新**: 检测变化，只更新变更部分
- **预热机制**: 智能预加载常用表
- **FTS 索引**: SQLite FTS5 全文搜索支持

#### 缓存数据结构

```sql
-- tables 表
CREATE TABLE tables (
    id TEXT PRIMARY KEY,
    conn_id TEXT NOT NULL,
    schema_name TEXT,
    table_name TEXT NOT NULL,
    table_type TEXT,
    comment TEXT,
    row_count INTEGER,
    created_at TIMESTAMP,
    updated_at TIMESTAMP
);

-- columns 表
CREATE TABLE columns (
    id TEXT PRIMARY KEY,
    conn_id TEXT NOT NULL,
    schema_name TEXT,
    table_name TEXT NOT NULL,
    column_name TEXT NOT NULL,
    data_type TEXT,
    is_nullable BOOLEAN,
    column_default TEXT,
    comment TEXT,
    ordinal_position INTEGER,
    is_primary_key BOOLEAN,
    is_foreign_key BOOLEAN
);
```

---

## 数据库设计

### 四层数据库架构

```
系统级 (System Level)
├── global.db (SQLite)
│   ├── 全局连接信息
│   ├── 全局设置
│   └── 最近连接记录
├── global_metadata/
│   └── conn_{id}.sqlite (每连接独立)
└── analytics.duckdb (DuckDB)
    ├── 查询缓存
    └── 全局分析数据

项目级 (Project Level)
├── meta/
│   ├── project.db (SQLite)
│   │   ├── 项目连接信息
│   │   ├── SQL 历史
│   │   └── 项目设置
│   └── connection_metadata/
│       └── conn_{id}.sqlite
└── analytics/
    └── data.duckdb (DuckDB)
        ├── 项目分析数据
        └── 持久化结果集

连接级 (Connection Level)
└── 用户指定的数据库服务器
```

### 迁移系统 ([`core/migration/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/migration/))

数据库版本管理，支持自动迁移：

- [`global_init.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/migration/global_init.rs): 全局数据库初始化
- [`migrations/global/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/migrations/global/): 全局迁移脚本
- [`migrations/project_meta/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/migrations/project_meta/): 项目迁移脚本

---

## 插件系统

### 两种扩展方式

#### 1. WASM 轻量级插件 (Extism)

- 沙箱运行，安全隔离
- 支持 Rust/Go/Python/JavaScript 等多语言
- 适合：SQL 格式化、数据脱敏、代码生成

#### 2. Sidecar 重量级扩展

- 独立进程管理
- 适合：JDBC 桥接、Python 分析环境
- 按需启动，空闲时不占用资源
- 崩溃不影响主程序

### 插件清单格式 (rdata-plugin.toml)

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "我的插件"
authors = ["Your Name"]

[plugin.permissions]
"sql.query" = true
"file.read" = ["*.csv"]

[contributes.panels]
"my-panel" = { title = "我的面板", component = "MyPanel" }

[contributes.commands]
"my-plugin.do-something" = { handler = "doSomething" }
```

### 扩展宿主 ([`core/extension-host.ts`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/core/extension-host.ts))

管理插件生命周期：

- 加载 → 激活 → 运行 → 停用 → 卸载

---

## 开发指南

### 环境要求

- Rust 1.75+
- Node.js 20+
- pnpm 9+
- Tauri CLI 2.x

### 快速开始

```bash
# 克隆项目
git clone https://github.com/ling-love-xin/RdataStation.git
cd RdataStation

# 安装依赖
pnpm install

# 开发模式运行
pnpm tauri dev

# 构建生产版本
pnpm tauri build
```

### 代码规范

#### Rust 规范

- 禁止 `unwrap()` / `expect()`，使用 `CoreError`
- 遵循架构红线：无循环依赖、无层级越界
- 使用 `cargo fmt` 和 `cargo clippy`

#### TypeScript/Vue 规范

- 禁止 `any` 类型
- 使用 Composition API + `<script setup>`
- 遵循 ESLint 和 Prettier 规范

### 测试

```bash
# 前端测试
pnpm test

# Rust 测试
cd src-tauri
cargo test
```

---

## 常见问题

### Q: 如何添加新的数据库驱动？

A: 在 [`core/driver/native/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/driver/native/) 下新增驱动实现，实现 `Database` 和 `MetadataBrowser` trait，然后在 `auto_register.rs` 中注册。

### Q: 如何创建新的扩展？

A: 在 [`src/extensions/builtin/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/) 下创建新目录，包含 `extension.ts` 和 `rdata-plugin.toml` 清单文件。

### Q: 元数据缓存如何更新？

A: 支持手动刷新和增量更新。在数据库导航中右键点击连接，选择"刷新元数据"。

### Q: DuckDB 临时表会持久化吗？

A: 默认不持久化。如需持久化，点击查询结果面板的"持久化"按钮，数据将保存到项目级 DuckDB。

---

## 相关文档

- 项目 README: [`README.md`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/README.md)
- 中文 README: [`README.zh-CN.md`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/README.zh-CN.md)
- 架构文档: [`docs/architecture/overview.md`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/docs/architecture/overview.md)
- 后端架构: [`docs/backend/ARCHITECTURE.md`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/docs/backend/ARCHITECTURE.md)
- 前端架构: [`docs/frontend/ARCHITECTURE.md`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/docs/frontend/ARCHITECTURE.md)
- 项目规则: [`.trae/rules/`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/.trae/rules/)

---

## 版本历史

| 版本   | 日期       | 说明                         |
| ------ | ---------- | ---------------------------- |
| v0.5.0 | 2026-05-20 | 网络连接功能 (SSH/SSL/Proxy) |
| v0.4.0 | 2026-05-18 | Vite 8 升级，前端重构        |
| v0.3.0 | 2026-05-12 | 代码质量优化，测试覆盖       |
| v0.2.0 | 2026-05-03 | DuckDB 分析引擎集成          |
| v0.1.0 | 2026-04-24 | 初始版本，基础架构           |

---

_本文档持续更新中，如有疑问请提交 Issue 或 PR。_
