# RdataStation 项目模块联调测试方案

> 版本：v1.0
> 编制日期：2026-05-11
> 状态：✅ 已发布
> 适用范围：RdataStation 全栈项目（Rust Core + Tauri Host + Vue 3 UI）

---

## 目录

1. [测试范围与目标](#一测试范围与目标)
2. [测试环境配置要求](#二测试环境配置要求)
3. [测试策略](#三测试策略)
4. [测试用例设计原则与具体示例](#四测试用例设计原则与具体示例)
5. [测试执行流程](#五测试执行流程)
6. [缺陷管理流程](#六缺陷管理流程)
7. [测试进度安排](#七测试进度安排)
8. [测试资源需求](#八测试资源需求)
9. [风险评估与应对措施](#九风险评估与应对措施)
10. [测试通过判定标准](#十测试通过判定标准)

---

## 一、测试范围与目标

### 1.1 项目架构概览

RdataStation 采用**四层微内核沙箱架构**，模块层次如下：

```
┌─────────────────────────────────────────────────┐
│  Vue 3 UI Layer (dockview-vue + naive-ui)        │
│  ├── extensions/builtin/                         │
│  │   ├── workbench       (主工作台布局)           │
│  │   ├── database         (数据库导航器)          │
│  │   ├── connection       (连接管理界面)          │
│  │   ├── query            (SQL编辑器/结果展示)    │
│  │   ├── analytics-resource (分析资源管理)        │
│  │   ├── scratchpad       (草稿管理)             │
│  │   └── settings         (系统设置)             │
│  └── stores/              (Pinia状态管理)         │
├─────────────────────────────────────────────────┤
│  Tauri Command Bridge (IPC层)                    │
│  ├── connection_commands    ├── sql_commands      │
│  ├── result_commands        ├── project_commands  │
│  ├── driver_commands        ├── metadata_commands │
│  ├── mock_commands          ├── scratchpad_cmds   │
│  ├── performance_commands   ├── system_commands   │
│  └── ...其他commands                               │
├─────────────────────────────────────────────────┤
│  Rust Core Layer (微内核)                        │
│  ├── driver/               数据库驱动抽象层       │
│  │   ├── traits.rs         (Database/DbPool/Transaction) │
│  │   ├── native/           原生驱动实现           │
│  │   │   ├── mysql.rs/pool.rs                     │
│  │   │   ├── postgres.rs/pool.rs                  │
│  │   │   ├── sqlite.rs/pool.rs                    │
│  │   │   └── duckdb.rs/pool.rs                    │
│  │   ├── jdbc/             JDBC桥接              │
│  │   ├── wasm/             Wasm驱动适配           │
│  │   └── connection/       连接配置/工厂/连接器   │
│  ├── services/             核心业务服务           │
│  │   ├── connection_service    ├── sql_service    │
│  │   ├── execution_service    ├── result_service  │
│  │   ├── persistence_service  ├── insight_engine  │
│  │   └── ...                                      │
│  ├── sql/                  SQL处理引擎            │
│  │   ├── engine.rs         (统一入口)             │
│  │   ├── parser.rs                               │
│  │   ├── builder.rs                              │
│  │   ├── formatter.rs                            │
│  │   └── transpiler.rs                           │
│  ├── dbi/                  数据库交互接口         │
│  ├── persistence/          持久化存储             │
│  │   ├── connection_store     ├── history_store   │
│  │   ├── project_store        ├── global_db       │
│  │   └── analytics_resource_store/               │
│  ├── cache/                缓存管理               │
│  ├── mock/                 模拟数据生成           │
│  ├── insight/              数据洞察引擎           │
│  ├── migration/            数据库迁移             │
│  ├── logging/              日志系统               │
│  ├── performance/          性能监控               │
│  └── utils/                工具函数               │
├─────────────────────────────────────────────────┤
│  Data Sources (MySQL/PostgreSQL/SQLite/DuckDB)   │
└─────────────────────────────────────────────────┘
```

### 1.2 测试模块清单

| 模块编号 | 模块名称                                      | 所属层级    | 核心功能                                                                     | 关联模块           |
| -------- | --------------------------------------------- | ----------- | ---------------------------------------------------------------------------- | ------------------ |
| M01      | `core::error`                                 | Core        | 统一错误容器、错误域定义                                                     | 所有模块           |
| M02      | `core::models`                                | Core        | QueryResult/Value/Arrow数据模型                                              | M03-M06, M11       |
| M03      | `core::driver::traits`                        | Core        | Database/DbPool/Transaction/MetadataBrowser trait                            | M04-M09            |
| M04      | `core::driver::native::mysql`                 | Core        | MySQL驱动实现                                                                | M03, M10, M11      |
| M05      | `core::driver::native::postgres`              | Core        | PostgreSQL驱动实现                                                           | M03, M10, M11      |
| M06      | `core::driver::native::sqlite`                | Core        | SQLite驱动实现                                                               | M03, M10, M11      |
| M07      | `core::driver::native::duckdb`                | Core        | DuckDB驱动实现（分析引擎）                                                   | M03, M10, M11      |
| M07a     | `core::duckdb`                                | Core        | DuckDB分析引擎（连接池/临时表/联邦/导入导出/FTS/EXPLAIN/扩展/性能监控/快照） | M07, M15, M17      |
| M08      | `core::driver::jdbc`                          | Core        | JDBC桥接驱动                                                                 | M03, M10           |
| M09      | `core::driver::wasm`                          | Core        | Wasm插件驱动                                                                 | M03, M10           |
| M10      | `core::driver::connection`                    | Core        | 连接配置/工厂/连接器/流式连接                                                | M03-M09            |
| M11      | `core::services::connection_service`          | Service     | 连接生命周期管理                                                             | M03-M10, C01       |
| M12      | `core::services::sql_service`                 | Service     | SQL解析与执行服务                                                            | M02, M03, M17, C02 |
| M13      | `core::services::execution_service`           | Service     | SQL执行调度                                                                  | M11, M12, M14      |
| M14      | `core::services::result_service`              | Service     | 查询结果管理                                                                 | M02, M13, C03      |
| M15      | `core::services::duckdb_service`              | Service     | DuckDB专项服务                                                               | M07, M25           |
| M16      | `core::services::persistence_service`         | Service     | 持久化统一入口                                                               | M18-M26            |
| M17      | `core::sql`                                   | Core        | SQL解析/构建/格式化/转译                                                     | M12                |
| M18      | `core::persistence::connection_store`         | Persistence | 连接信息持久化                                                               | M11, M25           |
| M19      | `core::persistence::history_store`            | Persistence | SQL执行历史                                                                  | M12, M25           |
| M20      | `core::persistence::project_store`            | Persistence | 项目存储管理                                                                 | M25, C04           |
| M21      | `core::persistence::global_db`                | Persistence | 全局数据库（系统配置）                                                       | M18-M26            |
| M22      | `core::persistence::metadata_cache`           | Persistence | 元数据缓存持久化                                                             | M12, M27           |
| M23      | `core::persistence::analytics_resource_store` | Persistence | 分析资源（SQL/图表/版本）                                                    | M25, C07           |
| M24      | `core::persistence::sql_template_store`       | Persistence | SQL模板存储                                                                  | M12, C14           |
| M25      | `core::project`                               | Core        | 项目模型与状态                                                               | M20, M21           |
| M26      | `core::persistence::workbench_context_store`  | Persistence | 工作台上下文状态                                                             | W01                |
| M27      | `core::cache`                                 | Core        | 查询/元数据/LRU缓存                                                          | M12, M14, M22      |
| M28      | `core::insight`                               | Core        | 数据洞察规则引擎                                                             | M07, M15           |
| M29      | `core::mock`                                  | Core        | 模拟数据生成引擎                                                             | M07, C09           |
| M30      | `core::migration`                             | Core        | 数据库Schema迁移                                                             | M21                |
| M31      | `core::logging`                               | Core        | 日志记录/脱敏/订阅                                                           | 所有模块           |
| M32      | `core::performance`                           | Core        | 性能监控                                                                     | 所有模块           |
| M33      | `core::scratchpad`                            | Core        | 草稿管理                                                                     | C16                |
| M34      | `core::utils`                                 | Core        | 工具函数（hash/string/time）                                                 | 所有模块           |
| M35      | `core::crypto`                                | Core        | 加密功能（密码/连接加密）                                                    | M11, M18           |
| M36      | `core::arrow`                                 | Core        | Arrow数据处理                                                                | M02, M14           |
| M37      | `core::dbi`                                   | Core        | 数据库交互统一接口                                                           | M03-M09            |

### 1.3 Tauri Command 分支模块

| 编号 | Command模块                   | 对应服务                 | 前端调用场景            |
| ---- | ----------------------------- | ------------------------ | ----------------------- |
| C01  | `connection_commands`         | connection_service       | 连接创建/编辑/删除/测试 |
| C02  | `sql_commands`                | sql_service              | SQL执行/取消/历史       |
| C03  | `result_commands`             | result_service           | 结果集分页/导出/过滤    |
| C04  | `project_commands`            | project_store            | 项目创建/打开/切换      |
| C05  | `driver_commands`             | driver::registry         | 驱动注册/扫描/元数据    |
| C06  | `metadata_commands`           | Database trait           | Schema树/表详情/列信息  |
| C07  | `analytics_resource_commands` | analytics_resource_store | 资源CRUD/版本/标签      |
| C08  | `cache_warming_commands`      | cache                    | 缓存预热/状态/清理      |
| C09  | `mock_commands`               | mock                     | 模拟数据生成/模板/历史  |
| C10  | `memory_commands`             | performance              | 内存监控/阈值/告警      |
| C11  | `logging_commands`            | logging                  | 日志配置/级别/导出      |
| C12  | `navigator_commands`          | driver traits            | 数据库导航树加载        |
| C13  | `performance_commands`        | performance              | 性能指标/报告           |
| C14  | `sql_template_commands`       | sql_template_store       | SQL模板CRUD             |
| C15  | `sql_parser_commands`         | sql::engine              | SQL分析/格式化/转译     |
| C16  | `scratchpad_commands`         | scratchpad               | 草稿创建/编辑/持久化    |
| C17  | `system_commands`             | 系统信息                 | 系统参数/版本           |
| C18  | `port_commands`               | port_negotiation         | 端口协商                |

### 1.4 前端关键模块

| 编号 | 前端模块                 | 路径                                  | 核心功能             |
| ---- | ------------------------ | ------------------------------------- | -------------------- |
| W01  | WorkbenchLayout          | extensions/builtin/workbench          | dockview-vue全局布局 |
| W02  | SqlEditorPanel           | extensions/builtin/workbench          | Monaco SQL编辑器     |
| W03  | QueryResultPanel         | extensions/builtin/workbench          | AG Grid结果展示      |
| W04  | DatabaseNavigator        | extensions/builtin/database           | 虚拟滚动数据库树     |
| W05  | AddDataSourceDialog      | extensions/builtin/connection         | 数据源创建对话框     |
| W06  | AnalyticsResourceManager | extensions/builtin/analytics-resource | 分析资源管理         |
| W07  | SettingsPanel            | extensions/builtin/settings           | 设置面板             |
| W08  | ScratchpadPanel          | extensions/builtin/scratchpad         | 草稿面板             |
| W09  | stores/useAppStore       | stores                                | 全局应用状态         |
| W10  | stores/useMockStore      | stores                                | Mock数据状态         |
| W11  | stores/config            | stores                                | 配置状态管理         |

### 1.5 核心交互链路（测试重点）

```
连接管理链路：
  UI(AddDataSourceDialog) → C01 → M11(connection_service)
  → M10(connection/factory) → M03-M09(具体Driver) → DataSource

SQL执行链路：
  UI(SqlEditorPanel) → C02 → M12(sql_service)
  → M11(获取连接) → M03(Database::query)
  → M02(QueryResult) → M36(Arrow转换) → C03 → UI(QueryResultPanel)

Schema浏览链路：
  UI(DatabaseNavigator) → C06 → M03(Database::list_databases/tables/columns)
  → UI(虚拟树渲染)

持久化链路：
  UI/Service → M16(persistence_service) → M18-M26(Store层)
  → M21(global_db) → SQLite(系统DB)

缓存链路：
  M12(sql_service) → M27(cache) → M22(metadata_cache)
  → M21(global_db) 或 内存缓存

数据洞察链路：
  UI → M28(insight) → M07(duckdb) → 分析结果

模拟数据链路：
  UI(MockPanel) → C09 → M29(mock) → M07(duckdb) → 模拟数据集
```

### 1.6 测试目标

| 目标 | 描述           | 量化指标                                   |
| ---- | -------------- | ------------------------------------------ |
| G1   | 接口稳定性验证 | 所有Tauri Command调用成功率 ≥ 99%          |
| G2   | 数据传递准确性 | Arrow格式序列化/反序列化零数据丢失         |
| G3   | 功能协作完整性 | 核心业务链路端到端通过率 100%              |
| G4   | 错误处理健壮性 | 所有异常路径覆盖，错误码明确可追溯         |
| G5   | 系统性能       | MVP核心内存 < 150MB，冷启动 < 1.5s         |
| G6   | 多数据库兼容性 | MySQL/PG/SQLite/DuckDB四库全覆盖           |
| G7   | 架构约束遵守   | 零违规（无unwrap/无跨层调用/无mod.rs测试） |

---

## 二、测试环境配置要求

### 2.1 硬件环境

| 项目     | 最低配置                             | 推荐配置   |
| -------- | ------------------------------------ | ---------- |
| CPU      | 4核 x86_64                           | 8核 x86_64 |
| 内存     | 8 GB                                 | 16 GB      |
| 磁盘     | 20 GB 可用空间                       | 50 GB SSD  |
| 操作系统 | Windows 10 / macOS 12 / Ubuntu 22.04 | 同最低     |

### 2.2 软件环境

| 软件              | 版本            | 用途                    |
| ----------------- | --------------- | ----------------------- |
| Rust              | stable (1.85+)  | 后端编译与测试          |
| Node.js           | 20 LTS          | 前端依赖管理            |
| pnpm              | 9.x             | 前端包管理              |
| MySQL             | 8.0+            | 测试目标数据库          |
| PostgreSQL        | 16+             | 测试目标数据库          |
| SQLite            | 3.45+ (bundled) | 测试目标数据库          |
| DuckDB            | 1.x (bundled)   | 测试目标数据库+分析引擎 |
| Git               | 2.45+           | 版本管理                |
| VS Code / Trae CN | 最新稳定版      | IDE                     |

### 2.3 数据库测试实例配置

> **重要**：所有测试数据库实例使用独立端口，避免与生产环境冲突。

| 数据库     | 测试实例        | 端口 | 测试账号             | 测试数据库 |
| ---------- | --------------- | ---- | -------------------- | ---------- |
| MySQL      | localhost       | 3307 | test_user / Test@123 | rdata_test |
| PostgreSQL | localhost       | 5433 | test_user / Test@123 | rdata_test |
| SQLite     | :memory: / 文件 | N/A  | N/A                  | :memory:   |
| DuckDB     | :memory: / 文件 | N/A  | N/A                  | :memory:   |

### 2.4 测试数据库Schema

```sql
-- MySQL/PostgreSQL 测试表结构
CREATE TABLE test_users (
    id INTEGER PRIMARY KEY AUTO_INCREMENT,
    name VARCHAR(100) NOT NULL,
    email VARCHAR(200) UNIQUE,
    age INTEGER,
    salary DECIMAL(12,2),
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    metadata JSON
);

CREATE TABLE test_orders (
    id INTEGER PRIMARY KEY AUTO_INCREMENT,
    user_id INTEGER NOT NULL REFERENCES test_users(id),
    product_name VARCHAR(200) NOT NULL,
    quantity INTEGER NOT NULL,
    price DECIMAL(10,2) NOT NULL,
    order_date TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 测试数据（各表不少于100行）
```

### 2.5 环境变量配置

```bash
# .env.test（测试环境专用）
RUST_LOG=debug
RUST_BACKTRACE=1
TEST_MYSQL_URL=mysql://test_user:Test@123@localhost:3307/rdata_test
TEST_PG_URL=postgres://test_user:Test@123@localhost:5433/rdata_test
TEST_SQLITE_PATH=:memory:
TEST_DUCKDB_PATH=:memory:
```

### 2.6 构建与运行命令

```bash
# Rust 后端测试
cd src-tauri
cargo test --lib                          # 单元测试
cargo test --test '*'                     # 集成测试
cargo clippy -- -D warnings               # Lint检查
cargo fmt --check                         # 格式检查

# 前端测试
cd src
pnpm run lint                             # ESLint
pnpm run format --check                   # Prettier
pnpm run typecheck                        # TypeScript类型检查

# 全栈构建验证
pnpm tauri build --debug                  # 调试构建
```

---

## 三、测试策略

### 3.1 测试金字塔分层

```
              ╱╲
             ╱ E2E ╲          端到端测试 (10%)
            ╱────────╲
           ╱ 集成测试  ╲       集成测试 (30%)
          ╱────────────╲
         ╱  接口/服务测试  ╲    API/服务测试 (40%)
        ╱────────────────╲
       ╱   单元测试 (Rust)  ╲  单元测试 (20%)
      ╱────────────────────╲
```

### 3.2 单元测试（Unit Test）— 占比 20%

**目标**：验证单个函数/方法的逻辑正确性

**范围**：

- `core::error` — 错误域创建、错误码、Display、retryable判断
- `core::models` — QueryResult序列化/反序列化、Value类型转换
- `core::sql::parser` — SQL解析（SELECT/INSERT/UPDATE/DELETE/DDL）
- `core::sql::builder` — SQL构建
- `core::sql::formatter` — SQL格式化
- `core::sql::transpiler` — SQL方言转译
- `core::utils` — hash/string/time工具函数
- `core::driver::connection::config` — 连接配置解析与校验
- `core::cache::lru_cache` — LRU淘汰算法

**工具**：`#[cfg(test)] mod tests`、`cargo test --lib`

**约束**：遵循项目测试代码组织铁律 — mod.rs中禁止测试、私有函数内嵌测试、公共API复杂测试外移

### 3.3 接口/服务测试（API/Service Test）— 占比 40%

**目标**：验证各Service层的业务逻辑和Command层的IPC通信

**范围（Rust端）**：

| 测试模块               | 测试内容                                                   | 测试文件                                |
| ---------------------- | ---------------------------------------------------------- | --------------------------------------- |
| connection_service     | 连接创建/连接池获取/连接关闭/ping/重连                     | `tests/connection_service_tests.rs`     |
| sql_service            | SQL解析→执行→结果返回 全流程                               | `tests/sql_service_tests.rs`            |
| execution_service      | SQL执行调度/取消/超时                                      | `tests/execution_service_tests.rs`      |
| result_service         | 结果分页/过滤/导出/截断                                    | `tests/result_service_tests.rs`         |
| persistence_service    | CRUD操作/lazy加载/事务一致性                               | `tests/persistence_service_tests.rs`    |
| duckdb_service         | 联邦查询/外部表注册/分析查询                               | `tests/duckdb_service_tests.rs`         |
| duckdb_analysis_engine | 连接池/临时表/联邦/导入导出/FTS/EXPLAIN/扩展/性能监控/快照 | `tests/duckdb_analysis_engine_tests.rs` |
| insight_engine         | 规则注册/执行/评分                                         | `tests/insight_engine_tests.rs`         |
| mock_engine            | 模板解析/数据生成/历史记录                                 | `tests/mock_engine_tests.rs`            |
| migration_manager      | schema迁移/版本管理/回滚                                   | `tests/migration_tests.rs`              |
| cache_manager          | 读写/淘汰/过期/内存守卫                                    | `tests/cache_manager_tests.rs`          |
| logging                | 日志记录/脱敏/级别过滤                                     | `tests/logging_tests.rs`                |
| scratchpad             | 草稿创建/编辑/状态管理                                     | `tests/scratchpad_tests.rs`             |

**验证方式**：

```rust
// 示例：Service层测试模式
#[tokio::test]
async fn test_connection_service_create_and_query() {
    // 1. 初始化测试数据库
    // 2. 调用 connection_service 创建连接
    // 3. 调用 sql_service 执行查询
    // 4. 验证 QueryResult 结构完整性
    // 5. 清理资源
}
```

### 3.4 集成测试（Integration Test）— 占比 30%

**目标**：验证跨模块调用链路的正确性、数据在不同层级间传递的准确性

#### 3.4.1 纵向集成（关键业务链路）

| 链路编号 | 链路名称        | 路径                                                                    | 测试重点                                 |
| -------- | --------------- | ----------------------------------------------------------------------- | ---------------------------------------- |
| IL-01    | 连接-查询链路   | UI → C01 → M11 → M10 → M04-M07 → DataSource → M02 → M36 → C03 → UI      | 连接建立、SQL执行、Arrow序列化、结果渲染 |
| IL-02    | Schema浏览链路  | UI → C06 → M03(Database::list_databases) → M03 → UI                     | 懒加载、虚拟滚动、节点展开               |
| IL-03    | SQL编辑器链路   | UI(W02) → C15 → M17 → UI                                                | 语法高亮、代码提示、格式化、转译         |
| IL-04    | 模拟数据链路    | UI → C09 → M29 → M07 → M02 → UI                                         | 模板解析、数据生成、持久化               |
| IL-05    | 缓存-查询链路   | M12 → M27 → M22 → M21                                                   | 缓存命中/未命中、预热、失效              |
| IL-06    | 数据洞察链路    | UI → M28 → M07 → 分析结果                                               | 规则执行、评分计算                       |
| IL-07    | 持久化-恢复链路 | M11 → M18 → M21                                                         | 连接配置花式保存/恢复                    |
| IL-08    | 项目切换链路    | UI → C04 → M25 → M20                                                    | 项目上下文切换、状态重置                 |
| IL-09    | 分析资源链路    | UI → C07 → M23 → M21                                                    | 资源CRUD/版本管理/标签                   |
| IL-10    | 事务链路        | M03(Database::begin_transaction) → Transaction::query → commit/rollback | 事务ACID                                 |

#### 3.4.2 横向集成（多数据库兼容性）

| 测试编号 | 测试内容         | 覆盖数据库             |
| -------- | ---------------- | ---------------------- |
| HI-01    | 基础CRUD操作     | MySQL/PG/SQLite/DuckDB |
| HI-02    | 事务支持         | MySQL/PG/SQLite/DuckDB |
| HI-03    | Schema元数据读取 | MySQL/PG/SQLite/DuckDB |
| HI-04    | 参数化查询       | MySQL/PG               |
| HI-05    | 流式查询         | MySQL/PG/DuckDB        |
| HI-06    | Arrow格式导出    | DuckDB                 |
| HI-07    | 联邦查询         | DuckDB                 |

#### 3.4.3 连接方式集成

| 测试编号 | 测试内容                          |
| -------- | --------------------------------- |
| CI-01    | 直连（Standard）— MySQL/PG        |
| CI-02    | SSH隧道连接                       |
| CI-03    | SSL/TLS加密连接                   |
| CI-04    | 代理连接                          |
| CI-05    | JDBC桥接连（JVM管理）             |
| CI-06    | DuckDB加速（DuckDB Acceleration） |

### 3.5 端到端测试（E2E Test）— 占比 10%

**目标**：模拟真实用户操作，验证从前端UI到后端数据源的完整流程

| 编号   | 场景                   | 步骤                                                                            |
| ------ | ---------------------- | ------------------------------------------------------------------------------- |
| E2E-01 | 新建连接并执行查询     | 打开连接表单→填写MySQL配置→测试连接→保存→打开SQL编辑器→输入SELECT→执行→查看结果 |
| E2E-02 | Schema浏览与表数据预览 | 展开数据库树→选择表→查看列详情→预览表数据→分页浏览                              |
| E2E-03 | SQL编辑器全流程        | 输入SQL→语法高亮检查→格式化→方言转译→执行→查看结果→导出CSV                      |
| E2E-04 | 项目创建与切换         | 新建项目→创建连接→切换项目→验证连接隔离→验证历史隔离                            |
| E2E-05 | 分析资源管理           | 保存SQL为资源→添加标签→版本回滚→回收站恢复                                      |
| E2E-06 | 模拟数据生成           | 选择表→选择模板→配置参数→生成数据→预览→插入目标表                               |
| E2E-07 | 数据洞察分析           | 选择表→运行洞察→查看评分→查看列分析                                             |
| E2E-08 | 多数据库联邦查询       | DuckDB注册MySQL外部表→SELECT跨库JOIN→结果验证                                   |
| E2E-09 | 草稿管理               | 创建草稿→编辑→保存→切换草稿→删除                                                |
| E2E-10 | 连接池监控             | 开多个查询→查看连接池状态→关闭连接→状态更新                                     |

### 3.6 非功能测试

| 类型           | 内容                                          | 工具/方法                      |
| -------------- | --------------------------------------------- | ------------------------------ |
| 性能测试       | 大结果集渲染（10万行）、SQL执行耗时、内存占用 | `cargo bench`、Chrome DevTools |
| 内存测试       | MVP核心 < 150MB、大结果集内存峰值             | `memory_commands`、系统监控    |
| 启动测试       | 冷启动时间 < 1.5s                             | 计时脚本                       |
| 并发测试       | 多连接池并发请求、多Tab并发查询               | tokio多任务                    |
| 长时间运行测试 | 持续运行4小时，监控内存泄漏                   | 自动化脚本                     |
| 安全测试       | SQL注入防护、密码加密存储、日志脱敏           | 渗透测试用例                   |

---

## 四、测试用例设计原则与具体示例

### 4.1 设计原则

1. **分层独立原则**：每层测试可独立运行，不依赖其他层的测试结果
2. **错误路径优先原则**：每个功能需覆盖正常路径 + 至少3种异常路径
3. **边界值覆盖原则**：空值/NULL/极大值/极小值/超长字符串
4. **数据契约验证原则**：验证IPC传输的Arrow格式完整性
5. **可复现原则**：测试数据可重置，测试结果可重复验证
6. **架构红线原则**：每个测试需验证不违反架构约束

### 4.2 测试用例模板

```markdown
| 字段         | 内容                  |
| ------------ | --------------------- |
| **用例ID**   | TC-{模块}-{编号}      |
| **所属模块** | 模块编号              |
| **测试层级** | 单元/接口/集成/E2E    |
| **测试目标** | 描述要验证的功能点    |
| **前置条件** | 环境、数据、状态要求  |
| **测试步骤** | 详细的操作步骤        |
| **预期结果** | 明确的预期输出        |
| **实际结果** | 执行后填写            |
| **状态**     | Pass / Fail / Blocked |
| **关联缺陷** | 缺陷ID                |
```

### 4.3 具体测试用例示例

#### 4.3.1 单元测试用例

**TC-ERROR-001：错误域分离与错误码验证**

| 字段         | 内容                                                                                                                                                                                      |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **用例ID**   | TC-ERROR-001                                                                                                                                                                              |
| **所属模块** | M01 (core::error)                                                                                                                                                                         |
| **测试层级** | 单元测试                                                                                                                                                                                  |
| **测试目标** | 验证CoreError五个错误域能正确创建并返回对应的错误码                                                                                                                                       |
| **前置条件** | 无                                                                                                                                                                                        |
| **测试步骤** | 1. 分别创建Common/Connection/Database/Storage/Cache错误<br>2. 调用 `.code()` 方法<br>3. 调用 `.category()` 方法<br>4. 调用 `.is_retryable()` 方法<br>5. 调用 `Display`                    |
| **预期结果** | 1. 每个错误域返回正确的错误码前缀（COMMON*/CONN*/DB*/STORE*/CACHE\_）<br>2. category()返回正确枚举<br>3. Timeout/Network/PoolError返回is_retryable=true<br>4. Display包含错误码和描述信息 |

**TC-SQL-001：SQL解析器识别语句类型**

| 字段         | 内容                                                                                                                                                                                                     |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **用例ID**   | TC-SQL-001                                                                                                                                                                                               |
| **所属模块** | M17 (core::sql)                                                                                                                                                                                          |
| **测试层级** | 单元测试                                                                                                                                                                                                 |
| **测试目标** | 验证SqlEngine正确解析不同SQL语句类型                                                                                                                                                                     |
| **前置条件** | 无                                                                                                                                                                                                       |
| **测试步骤** | 1. 输入 `SELECT * FROM users`<br>2. 输入 `INSERT INTO users VALUES (1, 'test')`<br>3. 输入 `UPDATE users SET name='a' WHERE id=1`<br>4. 输入 `DELETE FROM users`<br>5. 输入 `CREATE TABLE test (id INT)` |
| **预期结果** | parse_and_route()返回正确的(stmt_type, normalized_sql)<br>stmt_type分别为 Select/Insert/Update/Delete/CreateTable                                                                                        |

#### 4.3.2 接口测试用例

**TC-CONN-001：连接创建生命周期**

| 字段         | 内容                                                                                                                                                                                                            |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **用例ID**   | TC-CONN-001                                                                                                                                                                                                     |
| **所属模块** | M11/C01 (connection_service, connection_commands)                                                                                                                                                               |
| **测试层级** | 接口测试                                                                                                                                                                                                        |
| **测试目标** | 验证连接从创建到关闭的完整生命周期                                                                                                                                                                              |
| **前置条件** | MySQL测试实例运行中                                                                                                                                                                                             |
| **测试步骤** | 1. 调用connection_service.create_connection(config)<br>2. 调用 ping() 验证连接存活<br>3. 调用 pool_status() 获取连接池状态<br>4. 调用acquire()获取Database实例<br>5. 执行SELECT 1<br>6. 调用 close() 关闭连接池 |
| **预期结果** | 1. create返回conn_id<br>2. ping返回Ok(())<br>3. pool_status返回非零连接数<br>4. acquire成功<br>5. 查询返回正确结果<br>6. is_closed()=true                                                                       |

**TC-CONN-002：连接失败场景——认证错误**

| 字段         | 内容                                                                                                                                     |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| **用例ID**   | TC-CONN-002                                                                                                                              |
| **所属模块** | M11 (connection_service)                                                                                                                 |
| **测试层级** | 接口测试                                                                                                                                 |
| **测试目标** | 验证使用错误密码时返回明确的ConnectionError::AuthenticationFailed                                                                        |
| **前置条件** | MySQL/PG测试实例运行中                                                                                                                   |
| **测试步骤** | 1. 使用正确的host/port但错误的password创建连接<br>2. 捕获返回的CoreError                                                                 |
| **预期结果** | 1. 返回CoreError::Connection(ConnectionError::AuthenticationFailed)<br>2. error.code() = "CONN_AUTH_FAILED"<br>3. is_retryable() = false |

**TC-CONN-003：连接失败场景——超时**

| 字段         | 内容                                                                               |
| ------------ | ---------------------------------------------------------------------------------- |
| **用例ID**   | TC-CONN-003                                                                        |
| **所属模块** | M11                                                                                |
| **测试层级** | 接口测试                                                                           |
| **测试目标** | 验证连接不可达主机时返回超时错误                                                   |
| **前置条件** | 无                                                                                 |
| **测试步骤** | 1. 使用不可达IP(192.0.2.1)创建连接<br>2. 设置较短超时<br>3. 捕获超时错误           |
| **预期结果** | 1. 返回CoreError::Connection(ConnectionError::Timeout)<br>2. is_retryable() = true |

#### 4.3.3 集成测试用例

**TC-IL-001：完整SQL执行链路（MySQL）**

| 字段         | 内容                                                                                                                                                                     |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **用例ID**   | TC-IL-001                                                                                                                                                                |
| **所属模块** | M11-M12-M13-M14-M04-M02-M36                                                                                                                                              |
| **测试层级** | 集成测试                                                                                                                                                                 |
| **测试目标** | 验证从SQL输入到QueryResult的完整链路                                                                                                                                     |
| **前置条件** | MySQL测试实例，test_users表有数据                                                                                                                                        |
| **测试步骤** | 1. connection_service创建MySQL连接<br>2. sql_service.parse_and_execute("SELECT \* FROM test_users LIMIT 10")<br>3. 检查返回的QueryResult                                 |
| **预期结果** | 1. QueryResult.columns 包含所有列名<br>2. QueryResult.batches 非空且为Arrow RecordBatch<br>3. total_rows() = 10<br>4. is_read_only = true<br>5. 序列化为JSON后前端可解析 |

**TC-IL-002：完整SQL执行链路（PostgreSQL）**

| 字段         | 内容                                                                                                                                                                                |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **用例ID**   | TC-IL-002                                                                                                                                                                           |
| **所属模块** | M11-M12-M13-M14-M05-M02-M36                                                                                                                                                         |
| **测试层级** | 集成测试                                                                                                                                                                            |
| **测试目标** | 验证PG参数化查询防注入                                                                                                                                                              |
| **前置条件** | PG测试实例                                                                                                                                                                          |
| **测试步骤** | 1. 创建PG连接<br>2. query_with_params("SELECT \* FROM test_users WHERE name = $1", vec![Value::Text("test".into())])<br>3. query_with_params注入尝试 "'; DROP TABLE test_users; --" |
| **预期结果** | 1. 正常返回匹配行<br>2. 注入字符串被当作普通值处理，不执行DROP TABLE<br>3. 表仍然存在                                                                                               |

**TC-IL-003：事务链路**

| 字段         | 内容                                                                                                                                                     |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **用例ID**   | TC-IL-003                                                                                                                                                |
| **所属模块** | M03(Transaction) - M04/M05/M06/M07                                                                                                                       |
| **测试层级** | 集成测试                                                                                                                                                 |
| **测试目标** | 验证事务的BEGIN/COMMIT/ROLLBACK                                                                                                                          |
| **前置条件** | 各数据库测试实例                                                                                                                                         |
| **测试步骤** | 1. begin_transaction()<br>2. INSERT INTO test_users ...<br>3. ROLLBACK<br>4. 查询验证数据未插入<br>5. 再次INSERT<br>6. COMMIT<br>7. 查询验证数据已持久化 |
| **预期结果** | 1. ROLLBACK后INSERT回滚<br>2. COMMIT后数据持久化<br>3. 事务状态转换正确                                                                                  |

**TC-IL-004：多数据库跨库查询（DuckDB联邦）**

| 字段         | 内容                                                                                                                                 |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| **用例ID**   | TC-IL-004                                                                                                                            |
| **所属模块** | M07(DuckDB) - M04(MySQL)                                                                                                             |
| **测试层级** | 集成测试                                                                                                                             |
| **测试目标** | 验证DuckDB联邦查询能Join外部MySQL表                                                                                                  |
| **前置条件** | MySQL和DuckDB均可用                                                                                                                  |
| **测试步骤** | 1. DuckDB instance 通过ATTACH注册MySQL<br>2. 执行 SELECT d._, m._ FROM duckdb_table d JOIN mysql_table m ON d.id=m.id<br>3. 验证结果 |
| **预期结果** | 1. 跨库JOIN返回正确结果<br>2. Arrow格式数据无损                                                                                      |

#### 4.3.4 E2E测试用例

**TC-E2E-001：新建MySQL连接→执行查询→查看结果**

| 字段         | 内容                                                                                                                                                                                                                                                                                                                |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **用例ID**   | TC-E2E-001                                                                                                                                                                                                                                                                                                          |
| **所属模块** | W01-W02-W03-W04-W05 + C01+C02+C03+C06 + M11-M12-M04                                                                                                                                                                                                                                                                 |
| **测试层级** | E2E                                                                                                                                                                                                                                                                                                                 |
| **测试目标** | 验证用户从创建连接到查看查询结果的完整UI流程                                                                                                                                                                                                                                                                        |
| **前置条件** | MySQL测试实例运行                                                                                                                                                                                                                                                                                                   |
| **测试步骤** | 1. 点击"新建连接"→选择MySQL<br>2. 填写Host/Port/User/Password/Database<br>3. 点击"测试连接"→验证成功提示<br>4. 保存连接<br>5. 在导航器中双击连接展开Schema树<br>6. 选择test_users表→右键"SELECT TOP 100"<br>7. SQL编辑器自动填入SELECT语句<br>8. 点击"执行"或Ctrl+Enter<br>9. 查看QueryResultPanel中AG Grid展示结果 |
| **预期结果** | 1. 测试连接返回成功<br>2. Schema树正确展示 databases/schemas/tables/columns<br>3. 执行后返回数据在AG Grid中正确渲染<br>4. 状态栏显示行数和执行时间<br>5. 结果列与实际表结构一致                                                                                                                                     |

### 4.4 边界值与异常测试矩阵

| 维度          | 测试场景                   | 预期行为                               |
| ------------- | -------------------------- | -------------------------------------- |
| 空SQL         | 执行空字符串               | 返回CommonError::InvalidArgument       |
| NULL值列      | 查询含NULL的列             | Arrow正确表示null，前端展示为NULL      |
| 超大结果集    | 查询返回100万行            | 分批返回batch，内存可控，不OOM         |
| 超长SQL       | SQL语句 > 100KB            | 正常解析执行                           |
| 并发连接      | 同时创建20个连接           | 连接池正常工作，无死锁                 |
| 连接中断      | 执行中kill数据库连接       | 返回ConnectionError，可重试            |
| 查询超时      | 设置CancellationToken      | task被取消，资源释放                   |
| 无效表名      | SELECT \* FROM nonexistent | DatabaseError::TableNotFound           |
| 语法错误      | SELECT \* FORM users       | DatabaseError::Syntax with line/column |
| JSON超大字段  | JSON列 > 10MB              | 正确序列化，不截断                     |
| Unicode/Emoji | 列名含中文/emoji           | Schema树正确渲染                       |
| 并发写入      | 多事务同时INSERT           | 无死锁，约束正确                       |

---

## 五、测试执行流程

### 5.1 总体流程

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  1.测试计划  │ → │  2.环境准备  │ → │  3.用例设计  │ → │  4.用例评审  │
└─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘
       ↓                                                       ↓
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  8.测试报告  │ ← │  7.回归测试  │ ← │  6.缺陷修复  │ ← │  5.执行测试  │
└─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘
```

### 5.2 每日测试循环

```
09:00  环境检查与数据重置
09:30  冒烟测试（核心链路快速验证）
10:00  新功能测试/回归测试
12:00  午休
13:30  继续测试执行
16:00  缺陷汇总与确认
17:00  测试日报输出
17:30  当日问题与开发沟通
```

### 5.3 测试轮次

| 轮次 | 名称       | 内容                    | 准入条件             | 准出条件           |
| ---- | ---------- | ----------------------- | -------------------- | ------------------ |
| R1   | 单元测试轮 | 所有Rust单元测试        | 代码编译通过         | 单测覆盖率 ≥ 80%   |
| R2   | 接口测试轮 | 所有Service/Command测试 | R1通过，测试环境就绪 | 所有接口用例Pass   |
| R3   | 集成测试轮 | 纵向+横向集成链路       | R2通过               | 所有集成用例Pass   |
| R4   | E2E测试轮  | 完整用户场景            | R3通过，前端构建成功 | 所有E2E用例Pass    |
| R5   | 性能测试轮 | 性能/内存/启动          | R4通过               | 性能指标达标       |
| R6   | 回归测试轮 | 缺陷修复后回归          | 缺陷修复提交         | 回归通过，无新问题 |

### 5.4 CI/CD集成

```yaml
# 提交后自动触发
on: [push, pull_request]

jobs:
  unit-test:
    runs-on: ubuntu-latest
    steps:
      - cargo test --lib
      - cargo clippy -- -D warnings
      - cargo fmt --check

  frontend-check:
    steps:
      - pnpm run lint
      - pnpm run format --check
      - pnpm run typecheck

  integration-test:
    needs: [unit-test]
    services: [mysql, postgres]
    steps:
      - cargo test --test '*'

  build-check:
    needs: [unit-test, frontend-check]
    steps:
      - pnpm tauri build --debug
```

---

## 六、缺陷管理流程

### 6.1 缺陷生命周期

```
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│  新建    │ → │  确认    │ → │  分配    │ → │  修复中  │ → │  待验证  │
│ (New)    │   │(Confirmed)│   │(Assigned)│   │(In Fix)  │   │(To Verify)│
└──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘
                                                                    ↓
┌──────────┐   ┌──────────┐   ┌──────────┐                   ┌──────────┐
│  关闭    │ ← │  已验证  │ ← │  重新打开 │ ←─────────────── │  验证失败 │
│ (Closed) │   │(Verified)│   │(Reopened) │                   │(Failed)  │
└──────────┘   └──────────┘   └──────────┘                   └──────────┘
```

### 6.2 缺陷严重等级

| 等级 | 名称 | 定义                         | 处理时效           |
| ---- | ---- | ---------------------------- | ------------------ |
| S0   | 致命 | 系统崩溃、数据丢失、安全漏洞 | 立即修复，阻塞发布 |
| S1   | 严重 | 核心功能不可用、数据损坏     | 24小时内修复       |
| S2   | 一般 | 功能异常但有替代方案、UI错误 | 3个工作日内修复    |
| S3   | 轻微 | 文案错误、UI微调、优化建议   | 下个迭代修复       |

### 6.3 缺陷提交规范

```markdown
**标题**: [模块] 简短描述

**环境**:

- OS: Windows 11
- Rust: 1.85.0
- 数据库: MySQL 8.0

**复现步骤**:

1. 创建MySQL连接
2. 执行SQL: INSERT INTO ...
3. 观察结果

**预期结果**: QueryResult.affected_rows = 1

**实际结果**: QueryResult.affected_rows = None

**附件**: 截图/日志/复现SQL

**严重等级**: S1
**关联模块**: M04(MysqlDriver), M02(QueryResult)
```

### 6.4 缺陷统计指标

| 指标         | 目标值                           |
| ------------ | -------------------------------- |
| 缺陷发现率   | 每轮测试 ≥ 预期缺陷数的90%       |
| 缺陷修复率   | S0/S1修复率 100%，S2修复率 ≥ 95% |
| 缺陷重开率   | < 5%                             |
| 平均修复时长 | S1 < 8小时，S2 < 2天             |
| 缺陷遗留数   | 发布时 S0/S1 = 0，S2 ≤ 3         |

---

## 七、测试进度安排

### 7.1 总体甘特图

```
Week  │   W1    │   W2    │   W3    │   W4    │
──────┼─────────┼─────────┼─────────┼─────────┤
环境  │████     │         │         │         │
单测  │████████ │         │         │         │
接口  │  ███████│██       │         │         │
集成  │         │█████████│██       │         │
E2E   │         │    █████│███████  │         │
性能  │         │         │   ██████│██       │
回归  │         │         │      ███│█████████│
报告  │         │         │         │     ████│
──────┼─────────┼─────────┼─────────┼─────────┤
```

### 7.2 详细时间表

#### 第1周：环境搭建 + 单元测试

| 日期  | 任务                                                   | 产出             | 负责人  |
| ----- | ------------------------------------------------------ | ---------------- | ------- |
| Day 1 | 测试环境搭建（MySQL/PG/Docker）、测试数据库Schema创建  | 可用测试环境     | 后端Dev |
| Day 2 | Rust单元测试编写 — M01(error)、M02(models)、M34(utils) | 单测用例         | QA      |
| Day 3 | Rust单元测试编写 — M17(sql: parser/builder/formatter)  | 单测用例         | QA      |
| Day 4 | Rust单元测试编写 — M27(cache: lru_cache)、M10(config)  | 单测用例         | QA      |
| Day 5 | 单元测试执行、缺陷修复、覆盖率报告                     | 单测覆盖率 ≥ 80% | QA+Dev  |

#### 第2周：接口测试 + 集成测试（上）

| 日期  | 任务                                                   | 产出         |
| ----- | ------------------------------------------------------ | ------------ |
| Day 1 | M11(connection_service)接口测试 —4种数据库连接生命周期 | 接口测试用例 |
| Day 2 | M12(sql_service)接口测试 — 参数化查询、取消、超时      | 接口测试用例 |
| Day 3 | M13(execution_service)+M14(result_service)接口测试     | 接口测试用例 |
| Day 4 | M16(persistence_service)接口测试 — 各类Store CRUD      | 接口测试用例 |
| Day 5 | M28(insight)+M29(mock)+M30(migration)接口测试          | 接口测试用例 |

#### 第3周：集成测试（下）+ E2E测试

| 日期  | 任务                                          | 产出         |
| ----- | --------------------------------------------- | ------------ |
| Day 1 | IL-01~IL-05 核心链路集成测试                  | 集成测试通过 |
| Day 2 | IL-06~IL-10 链路 + HI-01~HI-07 多数据库兼容性 | 集成测试通过 |
| Day 3 | CI-01~CI-06 连接方式集成测试                  | 集成测试通过 |
| Day 4 | E2E-01~E2E-05 端到端场景                      | E2E用例      |
| Day 5 | E2E-06~E2E-10 端到端场景 + 缺陷回归           | 全部用例执行 |

#### 第4周：性能测试 + 回归测试 + 测试报告

| 日期  | 任务                                   | 产出       |
| ----- | -------------------------------------- | ---------- |
| Day 1 | 性能测试：大结果集、内存监控、启动时间 | 性能报告   |
| Day 2 | 并发测试、长时间运行测试、架构红线检查 | 稳定性报告 |
| Day 3 | 缺陷回归验证、剩余缺陷评估             | 缺陷清零   |
| Day 4 | 最终回归测试（全量用例）               | 回归通过   |
| Day 5 | 测试报告编写、发布评估                 | 测试报告   |

### 7.3 里程碑

| 里程碑            | 时间     | 准入标准                          |
| ----------------- | -------- | --------------------------------- |
| M1 - 环境就绪     | W1 Day 1 | 4种数据库实例可连接、测试数据就绪 |
| M2 - 单测完成     | W1 Day 5 | 单元测试覆盖率 ≥ 80%、clippy通过  |
| M3 - 接口测试完成 | W2 Day 5 | 所有Service接口用例Pass           |
| M4 - 集成测试完成 | W3 Day 3 | 所有集成链路用例Pass、4库兼容通过 |
| M5 - E2E测试完成  | W3 Day 5 | 所有E2E场景Pass                   |
| M6 - 性能达标     | W4 Day 2 | 内存 < 150MB、启动 < 1.5s         |
| M7 - 发布就绪     | W4 Day 5 | 测试报告通过、S0/S1缺陷清零       |

---

## 八、测试资源需求

### 8.1 人力资源

| 角色           | 人数 | 职责                         | 技能要求                |
| -------------- | ---- | ---------------------------- | ----------------------- |
| 测试负责人     | 1    | 方案制定、进度管理、报告编写 | 全栈经验、熟悉Rust+Vue  |
| 后端测试工程师 | 1-2  | Rust单元/接口/集成测试编写   | Rust/tokio/sqlx/serde   |
| 前端测试工程师 | 1    | Vue组件测试、E2E场景         | Vue3/Playwright/AG Grid |
| 开发配合       | 1-2  | 缺陷修复、代码Review         | Rust+Vue全栈            |

### 8.2 工具与环境

| 类别     | 工具/资源                          | 用途               |
| -------- | ---------------------------------- | ------------------ |
| IDE      | VS Code + rust-analyzer            | Rust开发与调试     |
| IDE      | Trae CN                            | AI辅助编码         |
| 数据库   | Docker (MySQL+PG)                  | 测试实例快速部署   |
| 测试框架 | Rust内置 `#[test]` + `tokio::test` | 后端测试           |
| Mock     | `mockall` / 手写mock               | 隔离外部依赖       |
| 内存分析 | `heaptrack` / `valgrind`           | 内存泄漏检测       |
| 性能测试 | `cargo bench` / `criterion`        | 基准测试           |
| 前端E2E  | Playwright / WebDriver             | 前端自动化测试     |
| CI/CD    | GitHub Actions                     | 自动化测试流水线   |
| 项目管理 | GitHub Issues + Projects           | 任务跟踪、缺陷管理 |
| 文档     | Markdown + VS Code                 | 测试文档编写       |

### 8.3 预算估算

| 项目        | 说明                         | 估算      |
| ----------- | ---------------------------- | --------- |
| 云资源      | Docker测试数据库实例（按需） | 低        |
| CI/CD       | GitHub Actions免费额度       | 免费      |
| 工具License | 开源工具，无需付费           | 免费      |
| 人力        | 4周 × 3-4人                  | 12-16人周 |

---

## 九、风险评估与应对措施

### 9.1 风险矩阵

| 风险编号 | 风险描述                           | 概率 | 影响 | 等级 |
| -------- | ---------------------------------- | ---- | ---- | ---- |
| R01      | MySQL/PostgreSQL测试实例不可用     | 中   | 高   | 🔴   |
| R02      | Arrow格式序列化兼容问题（跨平台）  | 中   | 高   | 🔴   |
| R03      | DuckDB联邦查询在不同OS下行为不一致 | 低   | 中   | 🟡   |
| R04      | dockview-vue组件与naive-ui版本兼容 | 低   | 中   | 🟡   |
| R05      | JVM管理（JDBC桥接）在不同环境异常  | 中   | 中   | 🟡   |
| R06      | 大结果集导致内存溢出               | 低   | 高   | 🟡   |
| R07      | 测试覆盖率不足（边缘场景未覆盖）   | 中   | 中   | 🟡   |
| R08      | 缺陷修复引入新问题（回归）         | 高   | 中   | 🔴   |
| R09      | 开发进度延迟导致测试时间压缩       | 中   | 高   | 🔴   |
| R10      | Wasm插件沙箱兼容性                 | 低   | 低   | 🟢   |

### 9.2 应对措施

#### R01 — 测试实例不可用

- **预防**：使用Docker Compose一键部署，脚本化环境搭建
- **应急**：SQLite/DuckDB本地文件模式兜底，确保本地测试不受网络影响
- **责任人**：后端Dev

#### R02 — Arrow序列化兼容

- **预防**：在CI中增加Windows/macOS/Linux三平台构建验证
- **应急**：降级为JSON序列化传输（增加转换层）
- **责任人**：后端Dev

#### R08 — 缺陷修复引入回归

- **预防**：每次修复后执行关联模块的全量回归测试
- **应急**：Git bisect定位引入commit，紧急回滚
- **责任人**：QA + Dev

#### R09 — 进度延迟压缩测试时间

- **预防**：测试左移，开发阶段即编写单元测试
- **应急**：优先保证核心链路（IL-01~IL-05）和E2E-01/E2E-02覆盖，非核心模块降级为冒烟测试
- **责任人**：测试负责人

### 9.3 应急方案

如遇以下严重阻塞情况，启动应急方案：

| 阻塞情况             | 应急方案                                           |
| -------------------- | -------------------------------------------------- |
| 某数据库驱动无法测试 | 跳过该DB，用SQLite替代验证核心逻辑，标记为已知限制 |
| CI环境不稳定         | 转为本地手动执行，保留测试日志供审查               |
| 关键缺陷阻塞超过48h  | 升级为团队攻关，必要时调整发布范围                 |

---

## 十、测试通过判定标准

### 10.1 硬性通过标准（必须满足）

| 编号 | 标准                                       | 验证方法                                         |
| ---- | ------------------------------------------ | ------------------------------------------------ |
| H01  | Rust代码零`unwrap()`/`expect()`            | `grep -r "unwrap()\|expect(" src-tauri/src/`     |
| H02  | 零`mod.rs`中包含`#[cfg(test)]`或`fn test_` | `grep -r "cfg(test)\|fn test_" **/mod.rs`        |
| H03  | 所有`Database` trait实现包含`meta()`方法   | 代码审查                                         |
| H04  | QueryResult内部包含`Vec<RecordBatch>`      | 代码审查                                         |
| H05  | Tauri Command不直接调用datasource层        | 代码审查                                         |
| H06  | 业务模块不直接`use sqlglot_rust`           | `grep "use sqlglot_rust" --exclude="core/sql/*"` |
| H07  | `cargo clippy -- -D warnings`零告警        | CI脚本                                           |
| H08  | `cargo fmt --check`零差异                  | CI脚本                                           |
| H09  | `pnpm run lint`零错误                      | CI脚本                                           |
| H10  | `pnpm run format --check`零差异            | CI脚本                                           |
| H11  | Rust单元测试覆盖率 ≥ 80%                   | `cargo tarpaulin`                                |
| H12  | 核心链路IL-01~IL-05 100%通过               | 测试报告                                         |
| H13  | S0致命缺陷 = 0                             | 缺陷统计                                         |
| H14  | S1严重缺陷 = 0                             | 缺陷统计                                         |
| H15  | MVP内存 < 150MB                            | 实际运行监控                                     |
| H16  | 冷启动时间 < 1.5s                          | 计时测量                                         |

### 10.2 柔性通过标准（基本满足）

| 编号 | 标准             | 目标值 | 最低值 |
| ---- | ---------------- | ------ | ------ |
| S01  | 接口测试通过率   | ≥ 95%  | ≥ 90%  |
| S02  | 集成测试通过率   | ≥ 95%  | ≥ 90%  |
| S03  | E2E场景通过率    | 100%   | ≥ 90%  |
| S04  | S2一般缺陷遗留数 | ≤ 3    | ≤ 5    |
| S05  | 接口测试用例数   | ≥ 80   | ≥ 60   |
| S06  | 集成测试用例数   | ≥ 30   | ≥ 20   |
| S07  | E2E用例数        | ≥ 10   | ≥ 8    |

### 10.3 发布决策矩阵

| 硬性标准        | 柔性标准   | 决策                            |
| --------------- | ---------- | ------------------------------- |
| 全部满足        | 目标值     | ✅ 可发布                       |
| 全部满足        | 最低值以上 | ⚠️ 有条件发布（附已知问题清单） |
| 1-2项未满足     | 任意       | 🔴 延期发布，修复后重测         |
| 3项及以上未满足 | 任意       | 🔴 不可发布，需重新评估范围     |

---

## 附录

### A. 已有测试文件清单

| 文件                                 | 测试内容                     | 行数 |
| ------------------------------------ | ---------------------------- | ---- |
| `tests/driver_registry_tests.rs`     | DriverRegistry注册/查找/工厂 | —    |
| `tests/persistence_helpers_tests.rs` | Persistence辅助函数          | —    |
| `tests/connection_manager_tests.rs`  | ConnectionManager生命周期    | —    |
| `tests/driver_integration.rs`        | 驱动集成测试                 | —    |

### B. 待新建测试文件清单

| 文件名                               | 对应模块    | 优先级 |
| ------------------------------------ | ----------- | ------ |
| `tests/connection_service_tests.rs`  | M11         | 🔴 高  |
| `tests/sql_service_tests.rs`         | M12         | 🔴 高  |
| `tests/execution_service_tests.rs`   | M13         | 🟡 中  |
| `tests/result_service_tests.rs`      | M14         | 🟡 中  |
| `tests/persistence_service_tests.rs` | M16         | 🔴 高  |
| `tests/duckdb_service_tests.rs`      | M15         | 🟡 中  |
| `tests/insight_engine_tests.rs`      | M28         | 🟢 低  |
| `tests/mock_engine_tests.rs`         | M29         | 🟢 低  |
| `tests/migration_tests.rs`           | M30         | 🟡 中  |
| `tests/cache_manager_tests.rs`       | M27         | 🟡 中  |
| `tests/logging_tests.rs`             | M31         | 🟢 低  |
| `tests/scratchpad_tests.rs`          | M33         | 🟢 低  |
| `tests/sql_engine_tests.rs`          | M17         | 🔴 高  |
| `tests/multi_db_integration.rs`      | HI-01~HI-07 | 🔴 高  |
| `tests/federation_tests.rs`          | TC-IL-004   | 🟡 中  |
| `tests/transaction_tests.rs`         | TC-IL-003   | 🟡 中  |

### C. 测试文件命名规范

```
格式：<功能描述>_tests.rs
示例：
  ✅ connection_service_tests.rs — 使用 <模块名>_tests
  ✅ driver_registry_tests.rs — 使用 <功能名>_tests
  ✅ multi_db_integration.rs — 跨模块集成测试
  ❌ test_connection.rs — 禁止 test_ 前缀
  ❌ connection_test.rs — 禁止 _test 后缀（Rust约定为 _tests）
```

### D. 参考文档

| 文档               | 路径                                      |
| ------------------ | ----------------------------------------- |
| 架构红线与编码规范 | `.trae/rules/common-rules.md`             |
| 项目技能配置       | `.trae/rules/rdata-station.md`            |
| 技术栈约束         | `.trae/rules/technical-rules.md`          |
| 前端规范           | `.trae/rules/frontend-enterprise-spec.md` |
| Git提交规范        | `.trae/rules/git-commit-message.md`       |
| Driver Trait定义   | `src-tauri/src/core/driver/traits.rs`     |
| CoreError定义      | `src-tauri/src/core/error.rs`             |
| QueryResult模型    | `src-tauri/src/core/models.rs`            |
| 已有测试           | `src-tauri/tests/`                        |

---

> **文档维护**：本方案随项目迭代持续更新。每次新增/修改模块后，同步更新本方案中对应的测试模块清单和测试用例。
>
> **版本历史**：
> | 版本 | 日期 | 变更说明 |
> |------|------|----------|
> | v1.0 | 2026-05-11 | 初始版本，覆盖全部37个Core模块、18个Command分支、11个前端模块 |
