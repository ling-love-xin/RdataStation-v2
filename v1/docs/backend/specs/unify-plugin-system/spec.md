# 统一插件系统 Spec

> 版本：v1.0 | 日期：2026-05-19 | 状态：Draft

## Why

当前 RdataStation 存在两套割裂的扩展机制：前端 VSCode 式 `ExtensionModule`（8 个内置扩展，`package.json` 元数据）和 后端 Extism Wasm `PluginManager`（API 全为 stub）。两者之间没有统一的插件描述格式、没有 IPC 通道、没有权限体系。数据库驱动只能在编译期注册，无法动态安装。这使得"第三方插件生态"无法建立。

## What Changes

- 定义 `rdata-plugin.toml` 统一 Manifest 格式，替代前端 `package.json` + 后端硬编码 `PluginMetadata`
- 迁移 8 个内置扩展的 `package.json` → `rdata-plugin.toml`
- 实现 Rust 侧 Manifest 解析器 + TypeScript 侧 Manifest 类型定义
- 实现 PluginHost 统一加载器（Loader），支持发现内置/用户/项目本地插件
- 实现 PluginHost 生命周期管理（Lifecycle），统一 activate/deactivate/崩溃恢复
- 实现 Rust 侧 Wasm Host Functions（`host_db_query` / `host_db_metadata` / `host_duckdb_query`），使 Wasm 插件能访问数据库
- 实现前端 `database.query()` API 真正对接后端 DBI
- 实现 ScopedStorage（命名空间隔离的插件专属存储）
- 改造 `DriverRegistry` 支持运行时注册（`register_wasm_driver`）
- 定义 `.rdata-plugin` 分发格式（tar.gz 包）
- 定义插件 API 分层模型（Level 0/1/2/3）
- **BREAKING**: 前端扩展 `package.json` 格式废弃，改为 `rdata-plugin.toml`
- **BREAKING**: `ExtensionContext` 接口重构，新增 `storage`、`logging`、`events` 等 API

## Impact

- Affected specs: 无（首次 spec）
- Affected code:
  - `src/core/extension-host.ts` — 重构 Loader + Lifecycle
  - `src/core/panel-registry.ts` — 保持，无变化
  - `src/core/command-registry.ts` — 保持，无变化
  - `src/core/builtin-extensions.ts` — 改为从 manifest 加载
  - `src/extensions/core/types.ts` — 新增 PluginManifest / PluginContext 类型
  - `src/extensions/builtin/*/package.json` — 废弃，新增 `rdata-plugin.toml`
  - `src/extensions/builtin/*/extension.ts` — 适配新 context
  - `src-tauri/src/adapters/wasm/mod.rs` — 新增 host functions
  - `src-tauri/src/adapters/wasm/api.rs` — 实现 WasmPluginApi
  - `src-tauri/src/adapters/wasm/extism.rs` — 扩展 extism host functions
  - `src-tauri/src/core/driver/registry/mod.rs` — 新增 runtime 注册
  - `src-tauri/src/core/services/` — 新增 plugin_bridge 服务
  - `src-tauri/src/core/models.rs` — 无变化（Arrow 契约不变）
  - `src-tauri/Cargo.toml` — 无变化（toml/serde 已有）

---

## ADDED Requirements

### Requirement: 统一插件 Manifest 格式

系统 SHALL 定义 `rdata-plugin.toml` 作为所有插件（前端/后端/驱动）的唯一元数据描述文件。格式应兼容 TOML v0.8+，支持 `[plugin]`、`[capabilities]`、`[permissions]`、`[contributes]`、`[dependencies]` 五个顶级段。

#### Scenario: 纯前端 UI 面板插件

- **GIVEN** 一个只包含 Vue 面板、无 Wasm 组件的插件
- **WHEN** Loader 解析其 `rdata-plugin.toml`
- **THEN** `capabilities.frontend.entry` 指向 `dist/extension.js`，`capabilities.wasm` 不存在
- **AND** `contributes.panels` 声明面板 ID/标题/位置/图标

#### Scenario: 纯后端 Wasm 分析插件

- **GIVEN** 一个只包含 Wasm 模块、无前端 UI 的数据分析插件
- **WHEN** Loader 解析其 `rdata-plugin.toml`
- **THEN** `capabilities.wasm.entry` 指向 `.wasm` 文件，`capabilities.frontend` 不存在
- **AND** `permissions` 段声明 `wasm:db_query` 等能力

#### Scenario: 全栈驱动插件

- **GIVEN** 一个同时包含前端配置 UI 和后端 Wasm 驱动的 PostgreSQL 插件
- **WHEN** Loader 解析其 `rdata-plugin.toml`
- **THEN** `capabilities.frontend` 和 `capabilities.wasm` 均存在
- **AND** `contributes.drivers` 声明数据库连接能力和 JSON Schema 路径

### Requirement: Manifest 解析器

系统 SHALL 提供 Rust 侧 `ManifestParser::parse(path: &Path) -> Result<PluginManifest>` 函数，解析 `rdata-plugin.toml` 并返回强类型结构体。TypeScript 侧 SHALL 提供对应的 `PluginManifest` 类型接口定义。

#### Scenario: 合法 manifest 解析成功

- **WHEN** `ManifestParser::parse("plugins/postgres/rdata-plugin.toml")` 被调用
- **AND** TOML 格式正确且所有必需字段完整
- **THEN** 返回 `Ok(PluginManifest)` 包含完整解析数据

#### Scenario: 非法 manifest 解析失败

- **WHEN** manifest 缺少 `[plugin].id` 字段
- **THEN** 返回 `Err(CoreError)` 包含明确错误信息和行号

#### Scenario: 版本不兼容

- **WHEN** manifest 中 `engines.rdatastation = "^0.6.0"` 但当前应用版本为 `0.5.0`
- **THEN** 返回 `Err` 包含版本不兼容说明

### Requirement: PluginHost Loader 插件加载器

系统 SHALL 实现 `PluginHost.loader`，按优先级扫描三个目录：项目本地 `.rdata/plugins/` > 用户安装 `{data_dir}/plugins/` > 内置 `resources/builtin/`。对每个发现到的 `rdata-plugin.toml` 解析、验证、依赖排序、权限审查。

#### Scenario: 发现并加载用户安装插件

- **GIVEN** 用户在 `{data_dir}/plugins/com.example.formatter/` 安装了插件
- **WHEN** 应用启动
- **THEN** Loader 发现该插件、解析 manifest、验证权限、标记为 `Discovered`
- **AND** 同名 ID 的项目本地插件会覆盖用户安装版本

#### Scenario: 加载失败不阻塞启动

- **GIVEN** 一个 manifest 损坏的第三方插件
- **WHEN** 应用启动
- **THEN** 该插件被标记为 `Error`，记录错误日志
- **AND** 其他正常插件不受影响，应用正常启动

#### Scenario: 依赖缺失时提示

- **GIVEN** 插件声明 `dependencies = ["com.example.sql-engine"]` 但该依赖未安装
- **WHEN** Loader 解析依赖
- **THEN** 该插件标记为 `Error`，原因记录为 "Dependency missing: com.example.sql-engine"

### Requirement: PluginHost Lifecycle 生命周期管理

系统 SHALL 实现插件生命周期状态机：`Discovered → Loaded → Active ⇄ Error → Inactive`。Wasm 插件崩溃后 30s 冷却、最多重试 3 次、超过则永久禁用。前端扩展崩溃由 Vue ErrorBoundary 捕获。

#### Scenario: 前端扩展激活成功

- **GIVEN** 一个合法的前端扩展
- **WHEN** `Lifecycle.activate("com.example.panel")` 被调用
- **THEN** 扩展的 `activate(context)` 函数被执行
- **AND** 扩展状态变为 `Active`
- **AND** `context.panels.register()` 注册的面板出现在 dockview 布局中

#### Scenario: Wasm 插件崩溃恢复

- **GIVEN** 一个已激活的 Wasm 分析插件
- **WHEN** 插件在执行分析时触发 Wasm trap（如内存越界）
- **THEN** 插件状态变为 `Error`
- **AND** 30s 后自动重试激活
- **AND** 3 次重试均失败后，状态变为 `Inactive`，发送用户通知

### Requirement: Wasm Host Functions

系统 SHALL 在 Extism 运行时中注册以下 Host Functions，使 Wasm 插件能通过标准接口访问数据库：

- `host_db_query(plugin_id, conn_id, sql) → Arrow IPC Stream bytes`
- `host_db_metadata(plugin_id, conn_id, catalog, schema, kind) → JSON`
- `host_duckdb_load(plugin_id, table_name, arrow_bytes) → ()`
- `host_duckdb_query(plugin_id, sql) → Arrow IPC Stream bytes`

每个 Host Function 必须校验调用方 `plugin_id` 的权限，记录资源使用计量。

#### Scenario: Wasm 插件执行 SQL 查询

- **GIVEN** 一个已授权的 Wasm 数据分析插件
- **WHEN** 插件调用 `host_db_query(plugin_id, conn_id, "SELECT COUNT(*) FROM users")`
- **THEN** Host Function 校验权限通过
- **AND** 通过 DBI 执行查询，返回 Arrow IPC Stream 字节
- **AND** 资源使用计量更新（CPU 时间 +1）

#### Scenario: 未授权插件被拒绝

- **GIVEN** 一个未声明 `wasm:db_query` 权限的插件
- **WHEN** 插件尝试调用 `host_db_query`
- **THEN** 返回 `PermissionError`，不执行查询

### Requirement: 前端 PluginContext API

系统 SHALL 重新设计 `ExtensionContext` 为 `PluginContext`，按四级分层注入 API：

- **Level 0（默认）**: `logging`、`storage`、`subscriptions`
- **Level 1（前端）**: `panels`、`commands`、`menus`
- **Level 2（数据，需声明）**: `database.query`、`database.metadata`
- **Level 3（系统，需授权）**: `system.fetch`、`system.fs`

#### Scenario: 插件使用 ScopedStorage

- **GIVEN** 插件 ID 为 `com.example.panel`
- **WHEN** 调用 `context.storage.set("favoriteQuery", "SELECT 1")`
- **THEN** 数据写入 SQLite 的 `plugin_storage` 表，键前缀 `com.example.panel:favoriteQuery`
- **AND** 其他插件无法读取该键（命名空间隔离）

#### Scenario: 插件使用 database.query

- **GIVEN** 插件 manifest 声明了 `capabilities.frontend` 且权限包含 `data:query`
- **WHEN** 调用 `context.database.query(connId, sql)`
- **THEN** 通过 Bridge 将请求路由到 Rust 侧 DBI
- **AND** 返回 Arrow Stream 供前端渲染（对接 AG Grid）

### Requirement: 驱动运行时注册

系统 SHALL 改造 `DriverRegistry`，新增 `runtime: HashMap<String, Arc<dyn DriverFactory>>` 字段。插件安装时通过 `register_wasm_driver()` 将 Wasm 驱动工厂注册到运行时表。运行时驱动的查询优先级高于编译期内置驱动。

#### Scenario: 动态注册第三方数据库驱动

- **GIVEN** 用户安装了 `com.example.postgres-driver` 插件
- **WHEN** 插件激活时调用后台 `register_wasm_driver`
- **THEN** 该驱动出现在 `get_available_drivers` 返回列表中
- **AND** 用户可在"添加连接"下拉菜单中看到该数据库类型

#### Scenario: 运行时驱动覆盖内置驱动

- **GIVEN** 内置 MySQL 驱动存在，用户安装了增强版 MySQL 驱动（同 ID）
- **WHEN** 查询驱动 `mysql`
- **THEN** 返回运行时注册的增强版（运行时优先）

### Requirement: 插件分发格式

系统 SHALL 定义 `.rdata-plugin` 为 tar.gz 归档格式，内部固定结构：根目录包含 `rdata-plugin.toml`，`dist/` 目录包含前端产物，`target/wasm32-wasi/release/` 包含 Wasm 文件，`schemas/` 包含 JSON Schema，`assets/` 包含图标。

#### Scenario: 安装 .rdata-plugin 包

- **GIVEN** 用户拖拽 `postgres-driver-1.0.0.rdata-plugin` 到应用窗口
- **WHEN** PluginHost 解压到 `plugins/com.example.postgres-driver/`
- **THEN** 解析 manifest → 权限确认对话框 → 激活插件 → 驱动可用

---

## MODIFIED Requirements

### Requirement: ExtensionContext 重构为 PluginContext

将现有 `ExtensionContext` 接口（`src/extensions/core/types.ts`）重构为 `PluginContext`：

- **新增**: `pluginId`、`manifest`、`storage`、`logging`、`events`
- **保留改造**: `commands`、`panels`(原 `window`)、`database`（新增真实实现）
- **移除**: `workspace`（合并到 `system.fs`）、`configuration`（合并到 `storage`）
- **新增分层权限**: `system.fetch`、`system.fs` 需权限声明

### Requirement: 内置扩展 package.json 迁移

将 8 个内置扩展的 `package.json` 废弃，新增等价 `rdata-plugin.toml`：

1. `extensions/builtin/connection/package.json` → `connection/rdata-plugin.toml`
2. `extensions/builtin/database/package.json` → `database/rdata-plugin.toml`
3. `extensions/builtin/query/package.json` → `query/rdata-plugin.toml`
4. `extensions/builtin/workbench/package.json` → `workbench/rdata-plugin.toml`
5. `extensions/builtin/analytics-resource/package.json` → `analytics-resource/rdata-plugin.toml`
6. `extensions/builtin/mysql-driver/package.json` → `mysql-driver/rdata-plugin.toml`
7. `extensions/builtin/scratchpad/package.json` → `scratchpad/rdata-plugin.toml`
8. `extensions/builtin/settings/package.json` → `settings/rdata-plugin.toml`

## REMOVED Requirements

_无移除项（首次正式 spec）_
