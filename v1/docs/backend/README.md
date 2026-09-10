# RdataStation 后端文档索引

> 版本：v2.2
> 最后更新：2026-05-19
> 状态：✅ v0.5.0 网络连接功能（SSH隧道 + SSL/TLS + 代理）后端核心完成

---

## 文档列表

### 架构与设计

| 文档                                                               | 说明                                                                           |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| [ARCHITECTURE.md](./ARCHITECTURE.md)                               | 后端架构设计（目录结构、核心模块、DuckDB 功能）                                |
| [FILE-RESPONSIBILITY.md](./FILE-RESPONSIBILITY.md)                 | 📋 后端文件职责 + 全方面审计报告（架构/设计/代码/接口/文档/前后端对齐/连通性） |
| [implementation.md](./implementation.md)                           | 后端实现说明（内存防护、错误处理、持久化服务）                                 |
| [CONNECTION-METHOD-DESIGN.md](./CONNECTION-METHOD-DESIGN.md)       | 🔌 网络连接方式设计：SSH隧道 + SSL/TLS + 代理（v0.5.0 核心已完成）             |
| [PROJECT_MODULE_ARCHITECTURE.md](./PROJECT_MODULE_ARCHITECTURE.md) | 项目模块架构设计（连接分类、元数据缓存）                                       |
| [PROJECT_MODULE_OPTIMIZATION.md](./PROJECT_MODULE_OPTIMIZATION.md) | 项目模块优化                                                                   |
| [MIGRATION_SYSTEM.md](./MIGRATION_SYSTEM.md)                       | 数据库迁移系统                                                                 |
| [LOGGING_MODULE.md](./LOGGING_MODULE.md)                           | 日志模块设计                                                                   |

### Schema 与缓存

| 文档                                                                           | 说明                                          |
| ------------------------------------------------------------------------------ | --------------------------------------------- |
| [SCHEMA_CHANGELOG.md](./SCHEMA_CHANGELOG.md)                                   | Schema 变更日志                               |
| [METADATA-CACHE-TEST-REPORT.md](./METADATA-CACHE-TEST-REPORT.md)               | 元数据缓存测试报告                            |
| [ANALYTICS_RESOURCE_SCHEMA.md](./ANALYTICS_RESOURCE_SCHEMA.md)                 | 分析资源模块后端 Schema                       |
| [ANALYTICS_RESOURCE_MANAGER_DESIGN.md](./ANALYTICS_RESOURCE_MANAGER_DESIGN.md) | 分析资源后端设计方案                          |
| [DATA-SOURCE-MODULE.md](./DATA-SOURCE-MODULE.md)                               | 新增数据源模块：架构 · 设计 · 开发 · 接口文档 |

### 草稿本 (Scratchpad)

| 文档                                               | 说明          |
| -------------------------------------------------- | ------------- |
| [SCRATCHPAD_DESIGN.md](./SCRATCHPAD_DESIGN.md)     | 草稿本设计    |
| [SCRATCHPAD_PROGRESS.md](./SCRATCHPAD_PROGRESS.md) | 草稿本进度    |
| [SCRATCHPAD_SCHEMA.md](./SCRATCHPAD_SCHEMA.md)     | 草稿本 Schema |

### 任务

| 文档                                                                     | 说明         |
| ------------------------------------------------------------------------ | ------------ |
| [TASKS.md](./TASKS.md)                                                   | 开发任务清单 |
| [TODO_CONNECTION_CLASSIFICATION.md](./TODO_CONNECTION_CLASSIFICATION.md) | 连接分类待办 |

---

## 快速导航

### 架构相关

- [整体架构](./ARCHITECTURE.md#二架构风格)
- [目录结构](./ARCHITECTURE.md#三目录结构)
- [DBI 设计](./ARCHITECTURE.md#41-dbi-统一数据访问层)
- [驱动层设计](./ARCHITECTURE.md#42-数据库驱动层)
- [DuckDB 功能](./ARCHITECTURE.md#五duckdb-核心功能)

### 开发相关

- [任务清单](./TASKS.md)
- [已完成任务](./TASKS.md#一已完成任务-)
- [进行中任务](./TASKS.md#二进行中任务-)
- [待完成任务](./TASKS.md#三待完成任务-)

---

## 相关资源

| 资源              | 路径                              |
| ----------------- | --------------------------------- |
| 项目级架构        | `../architecture/overview.md`     |
| Rust 后端源码文档 | `../../src-tauri/src/docs/`       |
| 前端架构          | `../frontend/ARCHITECTURE.md`     |
| 导航栏架构        | `../navigator/01-ARCHITECTURE.md` |

---

## 维护

- **文档维护者**：RdataStation 后端团队
- **更新频率**：随架构变更同步更新
