# RdataStation 文档中心

> 版本：v3.2
> 最后更新：2026-05-19
> 状态：✅ v0.5.0 网络连接功能（SSH隧道 + SSL/TLS + 代理）后端核心完成

---

## 快速导航

| 我想...              | 查看文档                                                                       | 说明                             |
| -------------------- | ------------------------------------------------------------------------------ | -------------------------------- |
| 了解项目架构         | [后端架构](./backend/ARCHITECTURE.md)                                          | 四层微内核、六域错误、驱动架构   |
| 开始后端开发         | [后端文档](./backend/README.md)                                                | Rust Core + Tauri                |
| 开始前端开发         | [前端文档](./frontend/INDEX.md)                                                | Vue 3 + TypeScript               |
| 了解 SQL 编辑器      | [SQL 编辑器](./frontend/sql-editor/README.md)                                  | 编辑器设计与优化                 |
| 了解结果集模块       | [结果集文档](./frontend/query-result/README.md)                                | 数据展示、过滤、导出             |
| 了解洞察系统         | [洞察文档](./frontend/insight/README.md)                                       | 数据洞察与分析                   |
| 了解分析资源管理器   | [分析资源](./frontend/analytics-resource/README.md)                            | 统一资源管理                     |
| 了解 Mock 数据生成器 | [Mock 设计](./frontend/mock/mock-data-generator-design.md)                     | 106 种生成器 + 6 场景模板        |
| 了解数据库导航       | [导航器文档](./navigator/README.md)                                            | IVM 增量视图设计                 |
| 了解标题栏设计       | [标题栏文档](./frontend/title-bar/README.md)                                   | 标题栏/状态栏重构                |
| 了解连接管理         | [连接模态框](./frontend/connection/connection-modal.md)                        | 新建数据库连接页面               |
| 了解 Phase 2 规划    | [Phase 2 规划](./frontend/connection/add-datasource-phase2-enterprise-plan.md) | 导入/导出/模板/批量编辑/健康监控 |
| 了解网络连接设计     | [网络连接设计](./backend/CONNECTION-METHOD-DESIGN.md)                          | SSH隧道 + SSL/TLS + 代理         |
| 了解网络配置 UI      | [网络配置 UI](./frontend/NETWORK-CONFIG-UI-DESIGN.md)                          | 网络配置前端设计                 |
| 查看竞品对比         | [竞品对比](./COMPARISON.md)                                                    | vs DBeaver / DataGrip            |
| 查看变更日志         | [变更日志](./CHANGELOG.md)                                                     | 版本变更记录                     |
| 查看任务进度         | [任务清单](./backend/TASKS.md)                                                 | 开发任务追踪                     |
| 查看联调测试方案     | [联调测试方案](./project-integration-test-plan.md)                             | 四库连接验证 + 全模块测试策略    |
| 查看四库连接测试     | [四库连接测试](../src-tauri/tests/four_db_connection_tests.rs)                 | MySQL/PG/SQLite/DuckDB 集成      |

---

## 文档结构

```
docs/
├── README.md                           # 📍 文档中心（本文档）
├── CHANGELOG.md                        # 变更日志
├── COMPARISON.md                       # 竞品对比分析
│
├── project-integration-test-plan.md    # 项目模块联调测试方案
│
├── backend/                            # ⚙️ 后端文档
│   ├── README.md                       # 后端文档索引
│   ├── ARCHITECTURE.md                 # 后端架构设计
│   ├── implementation.md               # 后端实现说明
│   ├── CONNECTION-METHOD-DESIGN.md     # 网络连接方式设计（SSH/SSL/Proxy）
│   ├── PROJECT_MODULE_ARCHITECTURE.md  # 项目模块架构
│   ├── PROJECT_MODULE_OPTIMIZATION.md  # 项目模块优化
│   ├── MIGRATION_SYSTEM.md             # 数据库迁移系统
│   ├── SCHEMA_CHANGELOG.md             # Schema 变更日志
│   ├── TASKS.md                        # 任务清单
│   ├── LOGGING_MODULE.md               # 日志模块设计
│   ├── METADATA-CACHE-TEST-REPORT.md   # 元数据缓存测试报告
│   ├── ANALYTICS_RESOURCE_SCHEMA.md    # 分析资源 Schema
│   ├── ANALYTICS_RESOURCE_MANAGER_DESIGN.md  # 分析资源后端设计
│   ├── SCRATCHPAD_DESIGN.md            # 草稿本设计
│   ├── SCRATCHPAD_PROGRESS.md          # 草稿本进度
│   ├── SCRATCHPAD_SCHEMA.md            # 草稿本 Schema
│   └── TODO_CONNECTION_CLASSIFICATION.md
│
├── frontend/                           # 🌐 前端文档
│   ├── INDEX.md                        # 前端文档索引
│   ├── ARCHITECTURE.md                 # 前端架构
│   ├── COMPONENTS.md                   # 组件规范
│   ├── DEVELOPMENT-GUIDE.md            # 开发指南
│   ├── CONFIG-SYSTEM.md                # 配置系统设计
│   ├── CONFIG-API.md                   # 配置 API 参考
│   ├── CONFIG-PROGRESS.md              # 配置开发进度
│   ├── API-INTERFACE.md                # 前端 API 接口
│   ├── DESIGN-SYSTEM-IMPLEMENTATION.md # 设计系统实现
│   ├── LAYOUT.md                       # 布局设计
│   ├── SCRATCHPAD.md                   # 草稿本前端
│   ├── PLUGIN-ARCHITECTURE-REFACTOR.md # 插件架构重构
│   ├── NETWORK-CONFIG-UI-DESIGN.md     # 网络配置 UI 设计
│   │
│   ├── sql-editor/                     # 📝 SQL 编辑器
│   │   ├── README.md                   # 编辑器文档索引
│   │   ├── design.md                   # 编辑器设计
│   │   └── optimization-plan.md        # 架构优化计划
│   │
│   ├── query-result/                   # 📊 结果集
│   │   ├── README.md                   # 结果集文档索引
│   │   ├── design.md                   # 需求设计
│   │   ├── architecture-v2.md          # V2 架构
│   │   ├── api-v2.md                   # V2 接口契约
│   │   ├── optimization-plan.md        # 优化计划
│   │   ├── optimization-progress.md    # 优化进度
│   │   └── audit/                      # 审查报告
│   │       ├── v3.md
│   │       ├── v4.md
│   │       ├── v4.1.md ~ v4.6.md
│   │
│   ├── insight/                        # 🔍 洞察系统
│   │   ├── README.md                   # 洞察文档索引
│   │   ├── system-plan.md              # 实施总体规划
│   │   ├── design.md                   # 设计方案
│   │   ├── prototype.md                # 原型设计
│   │   ├── rule-format.md              # 规则格式
│   │   ├── api-reference.md            # API 参考
│   │   └── dev-progress.md             # 开发进度
│   │
│   ├── analytics-resource/             # 📂 分析资源管理器
│   │   ├── README.md                   # 分析资源索引
│   │   ├── implementation.md           # 实现文档
│   │   ├── progress.md                 # 开发进度
│   │   ├── api-reference.md            # API 参考
│   │   ├── integration.md              # 前端集成指南
│   │   ├── settings.md                 # 设置功能
│   │   └── manager-design.md           # 前端设计方案
│   │
│   ├── title-bar/                      # 🪟 标题栏
│   │   ├── README.md                   # 标题栏文档索引
│   │   ├── progress.md                 # 开发进度
│   │   ├── audit-report.md             # 审查报告
│   │   ├── settings-integration.md     # 设置集成
│   │   ├── settings-implementation.md  # 设置实现
│   │   ├── statusbar-audit.md          # 状态栏审查
│   │   ├── statusbar-audit-2026-05-10.md
│   │   └── statusbar-settings-audit-2026-05-10.md
│   │
│   ├── mock/                           # 🎲 Mock 数据生成器
│   │   ├── mock-data-generator-design.md  # 完整设计文档
│   │   └── mock-persistence-layer.md      # 持久化层设计
│   │
│   └── connection/                     # 🔌 连接管理
│       ├── connection-modal.md         # 新建连接页面
│       └── add-datasource-phase2-enterprise-plan.md # Phase 2 企业级便利功能规划
│
├── navigator/                          # 🧭 导航器文档
│   ├── README.md                       # 导航器索引 + 已知问题
│   ├── 01-ARCHITECTURE.md              # IVM 架构设计
│   ├── 02-DATAFLOW.md                  # 数据流设计
│   ├── 03-INTERFACES.md                # 接口规范
│   ├── 04-IMPLEMENTATION.md            # 实施步骤
│   ├── 05-OPTIMIZATION.md              # 优化策略
│   ├── 06-CACHE-OPTIMIZATION.md        # 缓存优化
│   ├── database-navigator.md           # 数据库导航模块
│   ├── database-navigator-optimizations.md  # 导航栏优化
│   └── frontend-backend-alignment.md   # 前后端对齐
│
└── SQL/                                # SQL 脚本
    └── 元数据.sql
```

---

## 技术栈

### 后端 (Rust Core)

| 技术         | 版本      | 用途             |
| ------------ | --------- | ---------------- |
| Rust Edition | 2021      | 开发语言         |
| Tokio        | 1.44.x    | 异步运行时       |
| Tauri        | 2.10.x    | 桌面框架         |
| sqlx         | 0.8.x     | MySQL/PostgreSQL |
| rusqlite     | 0.32.x    | SQLite           |
| duckdb-rs    | 1.10502.x | DuckDB           |
| russh        | 0.49.x    | SSH 隧道         |
| native-tls   | 0.2.x     | SSL/TLS 加密     |

### 前端 (Vue 3 + TS)

| 技术          | 版本   | 用途       |
| ------------- | ------ | ---------- |
| Vue           | 3.5.x  | 响应式框架 |
| TypeScript    | 5.8.x  | 类型安全   |
| Vite          | 6.x    | 构建工具   |
| dockview-vue  | 6.0.x  | IDE 布局   |
| naive-ui      | latest | 组件库     |
| AG Grid       | 33.x   | 表格引擎   |
| Monaco Editor | 0.52.x | SQL 编辑器 |
| Pinia         | 2.3.x  | 状态管理   |

---

## 架构概览

```
┌─────────────────────────────────────────────────────────────┐
│                    RdataStation 四层微内核架构                │
├─────────────────────────────────────────────────────────────┤
│  Rust Core（微内核）  ──►  Tauri Host  ──►  Wasm Plugin  ──►  UI│
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐     │
│  │   DBI    │  │  Driver  │  │Services  │  │Persistence│    │
│  │ (统一入口)│  │ (抽象层) │  │ (业务层) │  │ (持久化) │     │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘     │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 相关资源

| 资源              | 路径                                     |
| ----------------- | ---------------------------------------- |
| AI 开发规范       | `.trae/rules/`                           |
| 前端技能规范      | `.trae/skills/frontend-enterprise-spec/` |
| Rust 后端详细文档 | `src-tauri/src/docs/`                    |
| SQL 迁移脚本      | `docs/SQL/`                              |

---

## 文档版本

| 版本 | 日期       | 说明                                                                                      |
| ---- | ---------- | ----------------------------------------------------------------------------------------- |
| v3.3 | 2026-05-28 | 新增 Phase 2 企业级便利功能规划文档（导入/导出/模板/批量编辑/健康监控/撤销重做/代码收敛） |
| v3.2 | 2026-05-19 | v0.5.0 网络连接功能：SSH隧道(russh) + SSL/TLS(native-tls)，后端核心完成，设计文档就位     |
| v3.1 | 2026-05-11 | 版本号校准、新增联调测试方案、四库连接测试                                                |
| v3.0 | 2026-05-10 | 文档结构重整：按模块归类、创建子目录                                                      |
| v2.5 | 2026-05-09 | Mock 数据生成器模块全部完成（10 Phase）                                                   |
| v2.4 | 2026-05-08 | SQL 编辑器架构优化全部完成                                                                |
| v2.3 | 2026-05-08 | SQL 编辑器架构优化全部完成（4 Phase）                                                     |
| v2.2 | 2026-05-08 | SQL 编辑器架构优化计划制定                                                                |
| v2.1 | 2026-05-06 | 添加 V7 增量同步竞品对比文档                                                              |
| v2.0 | 2026-05-03 | 创建文档中心，优化索引结构                                                                |
| v1.0 | 2026-04-23 | 初始文档架构                                                                              |

---

## 维护

- **文档维护者**：RdataStation 开发团队
- **更新频率**：随架构变更同步更新
- **反馈渠道**：提交 Issue 或联系架构组
