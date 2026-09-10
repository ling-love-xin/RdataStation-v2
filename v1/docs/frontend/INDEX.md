# RdataStation 前端文档

> 版本：v3.3
> 更新日期：2026-05-21
> 状态：🚧 NetworkTab + GeneralTab + AuthConfigManager 已完成实施，AdvancedTab 待实施

---

## 文档结构

```
docs/frontend/
├── INDEX.md                          # 📍 本文档（总索引）
├── ARCHITECTURE.md                   # 前端架构（插件化、DDD）
├── COMPONENTS.md                     # 组件开发规范
├── DEVELOPMENT-GUIDE.md              # 快速开发指南
├── CONFIG-SYSTEM.md                  # 配置系统设计文档
├── CONFIG-API.md                     # 配置系统 API 参考
├── CONFIG-PROGRESS.md                # 配置系统开发进度
├── API-INTERFACE.md                  # 前端 API 接口定义
├── DESIGN-SYSTEM-IMPLEMENTATION.md    # 设计系统实现
├── LAYOUT.md                         # 布局设计
├── SCRATCHPAD.md                     # 草稿本前端
├── PLUGIN-ARCHITECTURE-REFACTOR.md   # 插件架构重构
├── NETWORK-CONFIG-UI-DESIGN.md       # 网络配置 UI 设计
│
├── sql-editor/                       # 📝 SQL 编辑器
│   ├── README.md                     # 编辑器文档索引
│   ├── design.md                     # 编辑器设计
│   └── optimization-plan.md          # 架构优化计划（4 Phase）
│
├── connection/                       # 🔗 连接/数据源
│   ├── connection-modal.md           # 连接弹窗设计
│   ├── add-datasource-frontend-plan.md # ⭐ 新增数据源前端开发计划（Phase 1）
│   └── add-datasource-phase2-enterprise-plan.md # ⭐ Phase 2 企业级便利功能规划

├── query-result/                     # 📊 结果集
│   ├── README.md                     # 结果集文档索引
│   ├── design.md                     # 需求文档
│   ├── architecture-v2.md            # V2 架构设计
│   ├── api-v2.md                     # V2 接口契约
│   ├── optimization-plan.md          # 架构优化计划（A~H 组 39 项）
│   ├── optimization-progress.md      # 优化进度追踪
│   └── audit/                        # 审查报告（历史）
│       ├── v3.md
│       ├── v4.md
│       └── v4.1.md ~ v4.6.md
│
├── insight/                          # 🔍 洞察系统
│   ├── README.md                     # 洞察文档索引
│   ├── system-plan.md                # 实施总体规划
│   ├── design.md                     # 设计方案
│   ├── prototype.md                  # 原型设计
│   ├── rule-format.md                # 规则格式
│   ├── api-reference.md              # API 参考
│   └── dev-progress.md               # 开发进度
│
├── analytics-resource/               # 📂 分析资源管理器
│   ├── README.md                     # 分析资源索引
│   ├── implementation.md             # 实现文档
│   ├── progress.md                   # 开发进度
│   ├── api-reference.md              # API 参考
│   ├── integration.md                # 前端集成指南
│   ├── settings.md                   # 设置功能
│   └── manager-design.md             # 前端设计方案
│
├── title-bar/                        # 🪟 标题栏
│   ├── README.md                     # 标题栏文档索引
│   ├── progress.md                   # 开发进度
│   ├── audit-report.md               # 审查报告
│   ├── settings-integration.md       # 设置集成
│   ├── settings-implementation.md    # 设置实现
│   ├── statusbar-audit.md            # 状态栏审查
│   ├── statusbar-audit-2026-05-10.md
│   └── statusbar-settings-audit-2026-05-10.md
│
├── mock/                             # 🎲 Mock 数据生成器
│   ├── mock-data-generator-design.md  # v3.3 Final，生产就绪
│   └── mock-persistence-layer.md      # v1.1，持久化层
│
└── connection/                       # 🔌 连接管理
    └── connection-modal.md           # 新建连接页面
```

---

## 核心文档

### 1. [架构文档](./ARCHITECTURE.md) — 必读

**适用对象**：架构师、高级开发者、所有团队成员

**内容**：架构概览与技术栈、完整目录结构、扩展系统（生命周期、ExtensionContext）、DDD 分层架构、插件间通信机制（EventBus）、类型系统、统一 API 层、错误处理机制、命名规范、最佳实践

### 2. [配置系统](./CONFIG-SYSTEM.md) — 配置管理

**内容**：三层配置优先级（项目覆盖 > 全局默认 > 系统硬编码）、配置项可覆盖性矩阵、Store 实例生命周期、核心 API、Schema 版本管理

### 3. [配置 API](./CONFIG-API.md) — 接口参考

**内容**：所有配置类型定义、useAppStore 完整方法签名、SaveResult 类型、JSON 文件格式规范

### 4. [配置进度](./CONFIG-PROGRESS.md) — 开发进度

**内容**：4 阶段进度条、任务清单、技术债务

### 5. [开发指南](./DEVELOPMENT-GUIDE.md) — 快速上手

**内容**：环境准备、创建新插件/组件、调用后端 API、错误处理、插件间通信、常见问题

### 6. [组件规范](./COMPONENTS.md) — 组件开发

**内容**：组件分类、标准模板、Props/Emits 规范、样式规范、状态管理、性能优化、代码审查清单

---

## 模块文档

| 模块            | 索引文档                                                       | 说明                          |
| --------------- | -------------------------------------------------------------- | ----------------------------- |
| SQL 编辑器      | [sql-editor/README.md](./sql-editor/README.md)                 | 编辑器和代码补全              |
| 结果集          | [query-result/README.md](./query-result/README.md)             | 数据展示、过滤、导出          |
| 洞察系统        | [insight/README.md](./insight/README.md)                       | 数据洞察与分析                |
| 分析资源管理器  | [analytics-resource/README.md](./analytics-resource/README.md) | 统一资源管理                  |
| 标题栏          | [title-bar/README.md](./title-bar/README.md)                   | 标题栏/状态栏重构             |
| Mock 数据生成器 | [mock/](./mock/)                                               | 106 种生成器 + 持久化         |
| 连接管理        | [connection/](./connection/)                                   | 新建连接模态框 / Phase 2 规划 |

---

## 快速查找

| 我想...                 | 查看文档                                                                                               |
| ----------------------- | ------------------------------------------------------------------------------------------------------ |
| 了解整体架构            | [架构文档](./ARCHITECTURE.md)                                                                          |
| 了解 SQL 编辑器设计     | [sql-editor/README.md](./sql-editor/README.md)                                                         |
| 了解 SQL 编辑器优化计划 | [sql-editor/optimization-plan.md](./sql-editor/optimization-plan.md)                                   |
| 了解结果集优化计划      | [query-result/optimization-plan.md](./query-result/optimization-plan.md)                               |
| 了解结果集 V2 架构      | [query-result/architecture-v2.md](./query-result/architecture-v2.md)                                   |
| 创建新插件/组件         | [开发指南](./DEVELOPMENT-GUIDE.md)                                                                     |
| 管理配置/主题/语言      | [配置系统](./CONFIG-SYSTEM.md)                                                                         |
| 调用配置 API            | [配置 API](./CONFIG-API.md)                                                                            |
| 组件 Props/Emits        | [组件规范](./COMPONENTS.md)                                                                            |
| 了解洞察系统            | [insight/README.md](./insight/README.md)                                                               |
| 了解分析资源管理器      | [analytics-resource/README.md](./analytics-resource/README.md)                                         |
| 了解 Mock 数据生成器    | [mock/mock-data-generator-design.md](./mock/mock-data-generator-design.md)                             |
| 了解联调测试方案        | [../project-integration-test-plan.md](../project-integration-test-plan.md)                             |
| 查看四库连接测试        | [../../src-tauri/tests/four_db_connection_tests.rs](../../src-tauri/tests/four_db_connection_tests.rs) |
| 了解网络配置 UI 设计    | [NETWORK-CONFIG-UI-DESIGN.md](./NETWORK-CONFIG-UI-DESIGN.md)                                           |

---

## 前端架构概览

```
src/
├── app/                    # 应用入口
├── extensions/
│   ├── core/               # 扩展系统核心（event-bus、types）
│   └── builtin/            # 内置插件
│       ├── connection/     # 连接管理
│       ├── workbench/      # SQL 工作台
│       ├── scratchpad/     # 草稿本
│       └── query/          # 查询执行
├── shared/                 # 共享资源（api、components、composables、types、utils）
└── core/                   # 核心业务（project）
```

---

## 核心设计原则

1. **插件隔离** — 通过事件总线通信，禁止直接引用其他插件 store
2. **DDD 分层** — domain/infrastructure/ui 职责清晰
3. **共享资源中心化** — 统一到 shared/ 目录
4. **命名规范** — 文件 kebab-case，组件 PascalCase

---

## 架构版本

| 版本 | 日期       | 说明                                                                                      |
| ---- | ---------- | ----------------------------------------------------------------------------------------- |
| v3.3 | 2026-05-28 | 新增 Phase 2 企业级便利功能规划文档（导入/导出/模板/批量编辑/健康监控/撤销重做/代码收敛） |
| v3.2 | 2026-05-19 | v0.5.0 网络连接功能：网络配置 UI 设计文档                                                 |
| v3.0 | 2026-05-10 | 文档结构重整：按模块归类、创建子目录                                                      |
| v2.4 | 2026-05-09 | Mock 数据生成器模块全部完成（10 Phase）                                                   |
| v2.3 | 2026-05-08 | SQL 编辑器架构优化全部完成                                                                |
| v2.2 | 2026-05-08 | SQL 编辑器架构优化全部完成（4 Phase）                                                     |
| v2.1 | 2026-05-08 | SQL 编辑器架构优化计划制定                                                                |
| v2.0 | 2026-04-23 | 插件化架构优化（DDD 分层、事件总线）                                                      |
| v1.0 | 2026-04-20 | 初始插件化架构                                                                            |

---

## 相关文档

| 文档         | 路径                                                |
| ------------ | --------------------------------------------------- |
| 后端架构文档 | `../backend/ARCHITECTURE.md`                        |
| 联调测试方案 | `../project-integration-test-plan.md`               |
| 四库连接测试 | `../../src-tauri/tests/four_db_connection_tests.rs` |
| 项目规则     | `.trae/rules/`                                      |

---

## 维护

- **文档维护者**：RdataStation 前端团队
- **更新频率**：每次架构变更时同步更新
- **反馈渠道**：提交 Issue 或联系架构组
