# RdataStation 后端架构文档

> 版本：v2.3
> 最后更新：2026-05-27
> 状态：✅ 实际代码对齐
>
> 本文档描述 RdataStation 桌面数据库工具的后端架构设计、数据流、模块依赖规则及开发指南。

## 文档目录

| 编号 | 文档                                                                       | 说明                                               | 状态             |
| ---- | -------------------------------------------------------------------------- | -------------------------------------------------- | ---------------- |
| 01   | [架构概述](./01-architecture-overview.md)                                  | 整体架构设计、分层说明                             | ✅ v2.0          |
| 02   | [目录结构](./02-directory-structure.md)                                    | 目录组织及职责、文件说明                           | ✅ v2.0          |
| 03   | [模块依赖规则](./03-module-dependencies.md)                                | 依赖关系及约束                                     | v1.0             |
| 04   | [数据流设计](./04-data-flow.md)                                            | 请求处理流程                                       | v1.0             |
| 05   | [驱动架构](./05-driver-architecture.md)                                    | 数据库驱动设计、Registry、P0 问题                  | ✅ v2.0          |
| 06   | [存储架构](./06-storage-architecture.md)                                   | SQLite + DuckDB 双层存储                           | 🆕 v1.0          |
| 07   | [升级策略](./07-upgrade-strategy.md)                                       | 版本升级、依赖管理、接口兼容                       | 🆕 v1.0          |
| 08   | 驱动开发指南（规划中）                                                     | 新增数据库驱动开发步骤                             | 📋 待编写        |
| 09   | [开发指南](./09-development-guide.md)                                      | 开发规范及最佳实践                                 | v1.0             |
| 10   | [API 接口文档](./10-api-reference.md)                                      | Tauri 命令参考                                     | v1.0             |
| 11   | [插件系统设计](./11-plugin-system.md)                                      | Go Sidecar + Extism WASM 双引擎方案                | 📋 v1.0 远期规划 |
| 12   | [元数据缓存懒加载设计](./metadata-lazy-loading-design.md)                  | conn_id hash 生成 + L2 缓存懒加载                  | 🆕 v0.6.0        |
| 13   | [数据源模块优化 v0.6.1](./data-source-module-optimization-v0.6.1.md)       | 新建→缓存→导航树全链路优化                         | 🆕 v0.6.1        |
| 14   | [新增数据源深度分析与修复 v0.6.2](./add-datasource-deep-analysis-fixes.md) | 全链路分析 + 7项修复（暂存/事务/快照/认证/持久化） | 🆕 v0.6.2        |

## 快速开始

```rust
// 主入口：src-tauri/src/lib.rs
// 启动流程：
//   1. register_drivers()       → BuiltinDriverDiscovery::builtin_factories() × 6
//   2. initialize_global_system() → 创建全局 SQLite + DuckDB 连接
//   3. init_driver_manager()     → 同源 builtin_factories() × 6
//   4. Tauri Builder             → 注册 70+ 命令 + 启动应用
```

## 核心架构

```
Frontend (Vue3)
    │ Tauri Invoke
    ▼
commands/*.rs                    ← 按功能组织的命令模块
    │
    ▼
core/services/                   ← 业务服务层
    │ ConnectionService / ConnectionManager / SqlService
    ▼
core/driver/                     ← 驱动抽象层
    │ Database trait / DriverRegistry / DriverFactory
    ▼
driver/native/{mysql,mysql_native,postgres,postgres_native,sqlite,duckdb}.rs  ← 6 种内置驱动
```

## 核心原则

1. **分层架构**：严格分层（commands → services → driver → native），禁止反向依赖
2. **无框架依赖**：Core 层不依赖 Tauri
3. **可测试性**：所有业务逻辑可独立单元测试
4. **可扩展性**：新增数据库通过 DriverRegistry + DriverFactory
5. **Trait 约束**：Database / DbPool / Transaction 三大 trait 不可修改签名
6. **双层存储**：SQLite 事务层 + DuckDB 分析层
7. **禁止 unwrap**：所有生产代码使用 `?` 和 CoreError

## 关键设计决策

### 1. 为什么使用分层架构？

- **可测试性**：Core 层无框架依赖，可独立测试
- **可替换性**：适配器层可替换（Tauri → CLI/HTTP）
- **关注点分离**：每层职责单一

### 2. 为什么使用 Trait 抽象驱动？

```rust
trait Database {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError>;
    async fn list_databases(&self) -> Result<Vec<String>, CoreError>;
    async fn list_tables(&self, db: &str, schema: Option<&str>) -> Result<Vec<SchemaObject>, CoreError>;
}
```

统一接口，不同实现（sqlx / rusqlite / duckdb-rs），新增数据库只需实现该 trait。

### 3. 为什么 SQLite + DuckDB 双层？

```
SQLite 事务层          DuckDB 分析层
─────────────────     ─────────────────
连接信息 CRUD          联邦查询（跨库 JOIN）
SQL 历史记录            大数据集分析加速
项目元数据索引          CSV/Parquet 导入
元数据缓存              洞察引擎
WAL + 并发             列存储 + 向量化
```

### 4. 为什么锁定 4 种内置数据库？

先将 4 种核心数据库做稳定，确保：

- DriverRegistry 正确运作
- Database trait 覆盖足够场景
- 连接池和元数据缓存健壮

然后通过插件机制（JDBC/ODBC/WASM/ADBC）扩展。

## 当前架构不足（P0）

| 编号 | 问题                                         | 文件                                                                                                                                        |
| ---- | -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| P0-1 | DRIVER_FACTORY_MANAGER 重复注册（✅ 已移除） | [factory.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/factory.rs)                                   |
| P0-2 | create_database() 硬编码匹配                 | [connection_service.rs#L256](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_service.rs#L256) |
| P0-3 | to_url() 硬编码匹配                          | [registry.rs#L117](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/registry.rs#L117)                       |
| P0-4 | SchemaObject 缺少列详情                      | [traits.rs#L22](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/traits.rs#L22)                             |

## 版本历史

| 版本 | 日期       | 变更                                                                 |
| ---- | ---------- | -------------------------------------------------------------------- |
| v1.0 | 2026-04-10 | 初始架构设计                                                         |
| v1.1 | 2026-04-15 | 添加元数据服务                                                       |
| v1.2 | 2026-04-20 | 添加 DuckDB 支持                                                     |
| v1.3 | 2026-04-27 | DBI 统一数据访问层、智能连接池、DuckDB 联邦查询                      |
| v2.2 | 2026-05-11 | 四库连接测试、联调方案、文档体系更新                                 |
| v2.1 | 2026-05-10 | 新增插件系统设计文档（#11），Go Sidecar + Extism WASM 双引擎远期规划 |
| v2.0 | 2026-05-09 | **文档对齐实际代码**：纠正驅動架構、命令迁移、双层存储、升级策略     |
