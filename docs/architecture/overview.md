# v2 架构说明

## 定位

RdataStation v2 = **本地优先 + 查询后分析** 的数据库工作台。差异化能力：

- 双层数据架构：系统级共享资产 + 项目级物理隔离；
- 双后台引擎：SQLite 记元数据，DuckDB 做分析；
- 查询结果一键进入本地 DuckDB 二次分析、联邦查询、画像、mock。

## 三层架构（v1 四层 → v2 三层）

```
┌────────────────────────────────────────────────────────────┐
│ 表现层 · GPUI-kit（gpui-component / gpui-base）            │
│  Dock 布局 · SQL 编辑器(Tree-sitter/LSP) · 虚拟结果网格 ·   │
│  导航树 · 属性面板 · 命令面板 · 主题                        │
└──────────────────────────────┬─────────────────────────────┘
                               ▼
┌────────────────────────────────────────────────────────────┐
│ 服务层 · Rust Feature Crates（crates/*，按业务能力组织）    │
│  workbench settings project connection database scratchpad  │
│  analytics_resource mock insight plugin                    │
│  engine（双引擎/驱动/缓存/迁移/日志）· shared（稳定能力）   │
└──────────────────────────────┬─────────────────────────────┘
                               ▼
┌────────────────────────────────────────────────────────────┐
│ 数据层 · 双层 × 双引擎                                      │
│  系统级：global.sqlite + shared.duckdb（共享资产/快照）     │
│  项目级：project.sqlite + analysis.duckdb（物理隔离）       │
│  连接级：metadata 缓存 · 会话级：DuckDB 临时表              │
└────────────────────────────────────────────────────────────┘
```

v1 的「UI(Vue) → Tauri Adapter → Rust Core → Data」四层中：

| v1 层 | v2 去向 |
| --- | --- |
| UI Layer（Vue3 + Dockview + AG Grid + CodeMirror） | GPUI-kit 表现层（重写） |
| Tauri Adapter（commands/events/state） | 删除；命令演化为服务方法 / GPUI Action |
| Rust Core（core/*） | 服务层 Feature Crates（迁移） |
| Data Layer（SQLite + DuckDB + FS） | 保留；新增 DuckDB Secret 加速通道 |

## 双层数据架构（M1）

- **系统级（共享）**：连接模板、主数据/维度表、分析资产、插件、洞察规则、全局设置。落于 `global.sqlite + shared.duckdb`。
- **项目级（物理隔离）**：每项目独立目录与独立 SQLite/DuckDB 文件，项目彼此不可见。
- **提升（promote）**：项目内资产一键提升为系统级共享资产，落库前生成不可变版本快照；项目锁定版本，分析可复现。

## 双引擎（M2）

| 负载 | 引擎 | 示例 |
| --- | --- | --- |
| 事务元数据 | SQLite（rusqlite） | 连接、历史、草稿、资源目录、洞察缓存 |
| 分析计算 | DuckDB（duckdb-rs） | 二次分析、联邦查询、画像、mock、快照 |

SQLite 保存 DuckDB 表/视图的注册信息（名称/来源/版本/血缘）；写回源库只走原生通道。

## crate 依赖方向（硬约束）

```
app ───────────────► 所有 Feature crates
Feature crates ────► engine, shared, gpui-kit（UI 基础设施）
engine ────────────► shared
shared ────────────► (gpui-base / gpui-component / 第三方)
```

- Feature 不得依赖 `app`；
- Feature 间不得直接依赖对方 view；协作走 command / event / shared service；
- Feature 可以直接依赖 `gpui-kit`：按 GPUI-kit 编码指南，同一业务能力的 model / service / view / command / dialog / workflow 应放在同一 feature crate（官方示例 `workspace/src/{workspace_view.rs, rename_dialog.rs, commands.rs}`），`workbench` 退化为工作台壳层组合；
- 只有 ≥2 个真实使用方的稳定能力才进 `shared/`；
- 依赖必须无环，并始终指向更小、更稳定的 crate。

## 九大模块与 crate 对应

| 模块 | 需求 | crate |
| --- | --- | --- |
| M1 双层数据架构 | 系统级共享 + 项目级物理隔离 | `project` |
| M2 双后台库引擎 | SQLite 元数据 + DuckDB 分析 | `engine` |
| M3 数据源连接 | 原生连接 + DuckDB Secret 本地加速 | `connection` |
| M4 数据库导航 | 对象树 + 属性面板 | `database` |
| M5 草稿箱 | VSCode 式文件管理 | `scratchpad` |
| M6 资源分析 | 分析资源目录管理 | `analytics_resource` |
| M7 Mock | 元数据驱动测试数据（只进分析引擎） | `mock` |
| M8 洞察 | 库/表/列画像 | `insight` |
| M9 插件 | Sidecar / JDBC / Python / WASM | `plugin` |
| — 工作台 | Dock/命令面板/编辑器 | `workbench` |
| — 设置 | 设置项 model/持久化/设置视图/主题切换 | `settings` |
