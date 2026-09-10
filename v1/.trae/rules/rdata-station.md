本文件是 Trae CN 中使用的 RdataStation 项目技能配置 的 Markdown 文档化版本，用于团队理解与维护，同时为AI提供明确的项目规范，确保生成代码贴合项目架构、技术栈与编码标准，无需重复说明项目规则。

一、项目核心定位

- 项目名称：RdataStation

- 项目类型：Tauri 桌面数据库管理工具

- 开发语言：主要语言 Rust（最新稳定版），次要语言 TypeScript + Vue 3（均为最新稳定版）

- 核心使命：打造新一代跨平台数据库管理工具，轻量、高效、可扩展，支撑 10 年生命周期，对标并超越 DBeaver / DataGrip / Navicat。

- 架构风格：采用四层微内核沙箱架构，全程解耦，核心不做扩展，插件沙箱隔离，架构层级如下：

Rust Core（微内核）→ Tauri Host → Wasm Plugin → UI

- 长期约束：

✅ 接口遵循语义化版本控制（SemVer），确保10年向前兼容

✅ 内存控制：MVP 核心 < 150MB，插件 ≤ 500MB（可配置）

✅ 启动速度 < 1.5 秒，优化冗余启动逻辑

✅ 核心与插件严格隔离，插件崩溃不影响主程序稳定性

二、技术栈规范（最新稳定版，支持可升级）

说明：所有依赖均使用当前最新稳定版，升级策略明确，禁止主版本升级（避免破坏兼容性），允许小版本、补丁版本升级（获取安全更新与功能优化）。

1. Rust（核心层）

项目

版本 / 策略

补充说明

Edition

2021（最新稳定版，支持升级至2024）

可通过cargo fix --edition命令升级，确保兼容性

Toolchain

stable（最新稳定版，可通过rustup update升级）

使用rustup管理工具链，支持灵活切换与更新

Tokio

1.43（最新稳定版，升级策略：✅ 允许 minor，❌ 禁止 major）

异步运行时核心，确保数据库操作高效并发

sqlx

0.8.2（最新稳定版，升级策略：✅ 允许 minor，❌ 禁止 major）

提供异步数据库连接与编译期查询检查，提升代码健壮性

Serde

1.0（最新稳定版，升级策略：✅ 允许 minor，❌ 禁止 major）

数据序列化/反序列化核心依赖，保证类型安全

thiserror

1.0（最新稳定版，升级策略：✅ 允许 minor，❌ 禁止 major）

自定义错误类型，配合anyhow实现统一错误处理

anyhow

1.0（最新稳定版，升级策略：✅ 允许 minor，❌ 禁止 major）

简化错误处理逻辑，避免冗余代码

禁用

deno / lyze / lapce_core

无实质作用，避免引入冗余依赖、影响性能

2. Tauri（桌面层）

项目

配置

补充说明

版本

2.0.0（最新稳定版）

跨平台桌面框架，替代Electron，降低内存占用，支持移动平台扩展

升级策略

允许 patch / minor 版本升级

优先获取安全补丁与兼容性优化，不升级主版本避免架构变更

约束

优先使用 Tauri 原生 API

减少自定义跨平台适配代码，提升稳定性与开发效率

3. 前端（UI 层）

技术

版本（最新稳定版）

升级策略

补充说明

Vue

3.5（最新稳定版）

允许 minor / patch 升级

响应式系统优化，内存占用降低，支持响应式Props解构等新特性

TypeScript

6.0（最新稳定版）

允许 minor / patch 升级

完善空值合并与真值检查，提升代码健壮性

Pinia

3.0（最新稳定版）

允许 minor / patch 升级

Vue3 状态管理核心，轻量高效，替代Vuex

ag-Grid

35.3（最新稳定版）

允许 minor / patch 升级

企业级虚拟滚动表格，支持大数据量高效渲染，适配数据库查询结果展示

CodeMirror 6

6.x（最新稳定版）

允许 minor / patch 升级

SQL 编辑核心，轻量级代码编辑器，支持语法高亮、代码提示、多语言扩展

dockview-vue

6.1（最新稳定版）

允许 minor / patch 升级

前端 UI 底座，仿 VSCode 布局，支持拖拽面板、分割布局、侧边栏/底栏/中心区域

4. Wasm & 插件

- 标准：WASI 0.2（最新稳定版，允许 minor 版本升级）

- 传输格式：Apache Arrow（零拷贝，最新稳定版，允许 minor 升级），确保Rust与插件数据交互高效

- Python 插件：wasi-python 0.12（最新稳定版，允许 minor 升级），支持内置轻量Python环境与本地环境绑定

三、目录结构与职责

核心目录遵循“抽象层+实现层”设计，确保架构清晰、可扩展，所有文件职责明确，禁止随意移动或修改目录层级。

1. Core 根目录（src-tauri/src/core）

文件

职责

lib.rs

Core 对外统一入口，暴露标准化API，确保接口向前兼容

mod.rs

module 聚合，统一管理核心模块，简化导入逻辑

command.rs

Tauri Command 调度，统一处理前端与Rust核心的通信

connection_manager.rs

数据库连接生命周期管理，包括连接创建、复用、关闭，统一调度连接池

error.rs

CoreError 统一错误定义，结合thiserror与anyhow实现标准化错误处理

models.rs

定义核心数据模型（QueryResult / Row / Value），确保数据格式统一

sql/

SQL 处理基础能力统一封装模块，是 sqlglot-rust 在项目中的唯一接入点；提供 SQL 解析、Builder、格式化、方言转换等能力

2. driver（抽象层）

文件

职责

traits.rs

定义数据库操作核心接口（Database / Transaction / Stream），所有数据库实现需遵循该接口

mod.rs

re-export trait，简化抽象层接口导入，统一对外暴露

3. driver/native/（数据库实现层）

文件

职责

pool.rs

统一 Pool 抽象，封装sqlx连接池，提供标准化连接复用能力，支持多数据库适配

mysql.rs

MySQL 数据库实现，使用sqlx驱动，遵循driver::traits::Database接口

postgres.rs

PostgreSQL 数据库实现，使用sqlx驱动，遵循driver::traits::Database接口

sqlite.rs

SQLite 数据库实现，使用rusqlite官方驱动，遵循driver::traits::Database接口

duckdb.rs

DuckDB 数据库实现，使用duckdb-rs官方驱动，遵循driver::traits::Database接口

### 数据库驱动选型规范

| 数据库类型 | 驱动选择  | 原因                               | 版本      |
| ---------- | --------- | ---------------------------------- | --------- |
| MySQL      | sqlx      | 异步、编译期检查、连接池           | 0.8       |
| PostgreSQL | sqlx      | 异步、编译期检查、连接池           | 0.8       |
| SQLite     | rusqlite  | 官方Rust驱动，同步API，bundled特性 | 0.32      |
| DuckDB     | duckdb-rs | 官方Rust驱动，分析型数据库         | 1.10502.0 |

**驱动选型原则：**

- ✅ **网络数据库**（MySQL/PostgreSQL）：使用sqlx，支持异步和连接池
- ✅ **本地文件数据库**（SQLite/DuckDB）：优先使用官方Rust驱动（rusqlite/duckdb-rs）
- ✅ **新数据库支持**：优先调研官方Rust驱动，若无则使用sqlx或其他成熟方案
- ❌ 禁止混用多个驱动实现同一数据库类型

4. persistence（系统数据）

文件

职责

connection_store.rs

存储最近使用的数据库连接信息，实现连接快速复用（系统级）

history_store.rs

存储SQL执行历史，支持历史记录查询、复用，提升开发效率（系统级）

5. project（项目数据）

文件

职责

models.rs

项目模型定义，支持版本化、本地/远程路径、DuckLake 预留

store.rs

项目存储管理，负责项目创建、加载、保存

### 项目（Project）架构

```
Project（项目）
├── SQLite (meta/project.db)      - 元数据索引、事务性信息
├── DuckDB (analytics/data.duckdb) - 分析数据、版本载体
└── Config (config/*.json)        - 连接配置、SQL文件
```

### 配置分层

| 类型       | 归属   | 示例                                                     |
| ---------- | ------ | -------------------------------------------------------- |
| **系统级** | 全局   | 主题、快捷键、最近项目列表                               |
| **项目级** | 项目内 | 连接信息、SQL文件、DuckDB本地文件、联邦查询配置、SQL历史 |

### 版本化支持

所有核心模型支持 `Versioned<T>` 包装器：

- 版本链（parent/child）
- 用户标识（created_by）- DuckLake 预留
- 数据校验（checksum）

### 存储路径支持

| 类型             | 路径示例                | 说明               |
| ---------------- | ----------------------- | ------------------ |
| **本地**         | `/path/to/project/`     | 现阶段实现         |
| **网络（预留）** | `ducklake://project-id` | 后续 DuckLake 支持 |

四、编码规范（AI 强制执行）

所有代码必须遵循以下规范，AI生成代码时需严格执行，禁止出现不符合规范的代码，确保代码一致性、可维护性与安全性。

1. Rust 编码规范

- ❌ 禁止 unwrap()/ expect()（生产代码），避免运行时崩溃

- ✅ 必须使用 CoreError 统一错误处理，结合anyhow简化错误传递

- ✅ 所有数据库实现必须实现 driver::traits::Database 接口，确保统一适配

- ✅ Tauri Command 只能调用 ConnectionManager，禁止直接访问 datasource 层，解耦核心逻辑

- ✅ 核心逻辑禁止 unsafe 代码，确保内存安全，避免潜在风险

- ✅ 命名规范：变量/函数/模块用snake_case，结构体/枚举用PascalCase，常量用UPPER_SNAKE_CASE

- ✅ 注释规范：核心函数、结构体、接口必须添加文档注释（///），复杂逻辑添加行内注释（//）

- ✅ 代码格式化：严格遵循rustfmt规范，导入包按“标准库 → 第三方依赖 → 本地模块”排序

2. 前端编码规范

- ❌ 禁止在 Vue 组件中写业务逻辑，业务逻辑统一放在hooks或utils中

- ✅ 所有数据交互必须通过 tauri.invoke 调用Rust核心接口，禁止直接操作数据库

- ✅ 组件 / hooks / utils 严格分离，结构清晰，便于维护与复用

- ✅ 命名规范：变量/函数用camelCase，组件用PascalCase，常量用UPPER_SNAKE_CASE

- ✅ 代码规范：遵循ESLint、Prettier规范，避免冗余代码，Vue组件优先使用<script setup>语法

- ✅ 性能优化：ag-Grid启用虚拟滚动，合理使用Pinia状态管理，避免内存泄漏

3. 插件编码规范

- ✅ 插件只能通过 Apache Arrow 格式与 Rust 核心通信，实现零拷贝，提升数据传输效率

- ✅ 插件崩溃不得影响主程序，严格遵循WASI规范与沙箱隔离要求

- ✅ Python插件遵循PEP8规范，优先使用Pandas、Matplotlib实现数据分析功能

- ✅ 插件体积优化，避免冗余依赖，确保插件加载速度≤0.5秒

五、测试代码组织铁律

1. mod.rs 的绝对底线（零例外）

mod.rs 文件中禁止包含任何测试函数、测试模块、#[cfg(test)] 块。

mod.rs 只做三件事：

- 声明子模块（mod parser; 等）
- 重新导出关键类型（pub use ...;）
- 定义模块级常量或类型（仅在必要时）

2. 私有方法测试必须内嵌（Rust 语言硬约束）

私有函数（未标记 pub 的函数）的单元测试，只能放在定义该函数的同一个源文件内部，使用 #[cfg(test)] 标注。

内嵌测试应放在文件底部，用清晰的注释分隔：

```rust
// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_internal() { ... }
}
```

3. 公共 API 的复杂测试优先外移

公开函数（标记 pub fn 的函数）的单元测试，如果满足以下任一条件，应放在 src-tauri/tests/ 目录下：

| 条件                                       | 说明                               |
| ------------------------------------------ | ---------------------------------- |
| 源码文件超过 500 行                        | 文件已足够大，测试代码会降低可读性 |
| 测试需要复杂的 Mock、fixtures 或多步骤场景 | 复杂测试独立存放更清晰             |
| 单个测试函数超过 30 行                     | 测试本身已足够复杂，宜独立管理     |
| 测试代码总量超过 100 行                    | 总测试体量已不适合内嵌             |

如果以上条件均不满足，公共 API 测试可以内嵌在源文件中——这是完全可接受的做法，不是错误。

4. 文件拆分原则

当一个源文件超过 800 行，且包含大量私有方法和测试时，优先考虑将文件拆分为多个子模块，而非纠结测试放哪里。

拆分后：

- 每个子文件专注单一职责，行数回归合理范围
- 私有方法测试继续内嵌在对应子文件中
- 公共 API 测试外移到 src-tauri/tests/ 目录

5. 测试文件命名与函数命名规范

| 规范项           | 格式                     | 示例                           |
| ---------------- | ------------------------ | ------------------------------ |
| 集成测试目录     | src-tauri/tests/         | tests/                         |
| 测试文件名       | <功能>\_tests.rs         | driver_registry_tests.rs       |
| 测试函数名       | test\_<功能描述>         | test_register_duplicate_driver |
| 测试模块名       | tests                    | mod tests { ... }              |
| 内嵌测试位置     | 源文件底部，#[cfg(test)] | #[cfg(test)] mod tests { ... } |
| 全局集成测试位置 | src-tauri/tests/         | tests/integration_full_flow.rs |

6. 禁止事项

| 禁止行为                     | 说明                                 |
| ---------------------------- | ------------------------------------ |
| 在 mod.rs 中放置测试         | 唯一的零例外规则                     |
| 在外部测试文件中测试私有函数 | Rust 语言直接禁止                    |
| 在 src/main.rs 中放置测试    | 入口文件不可被 #[cfg(test)] 模块污染 |
| 测试函数出现在源码中间       | 测试必须放在独立的 mod tests 块中    |
| 测试代码与业务代码混排       | 测试与源码之间必须有清晰的分隔注释   |

7. 违规检查清单

| 检查项                      | 方法                                              |
| --------------------------- | ------------------------------------------------- |
| mod.rs 是否包含测试？       | 搜索所有 mod.rs 中的 #[cfg(test)] 和 fn test\_    |
| 公共 API 测试是否合理外移？ | 检查超 500 行的源文件是否仍有大量公共 API 测试    |
| 私有方法测试是否误放？      | 检查 tests/ 目录中是否误放了私有函数测试          |
| 文件是否过长？              | 检查是否有超 800 行的大型源文件，评估是否需要拆分 |
| 测试命名是否规范？          | 检查测试文件名是否遵循 <功能>\_tests.rs 格式      |
| 是否在源码中间插入了测试？  | 检查测试代码是否出现在业务代码中间                |

六、Prompt 模板（AI 行为控制）

AI处理以下场景时，需严格遵循对应模板，确保生成代码贴合项目结构与规范，无需额外提示。

1. 新增数据库

请在 core/datasource/下新增 {db_name}.rs，实现 driver::traits::Database 接口，包含连接创建、SQL执行、事务处理等核心方法，并通过 connection_manager.rs 注册该数据库实现，确保与现有架构兼容，错误处理使用CoreError，禁止使用unwrap()。

2. 修复编译错误

请基于现有 core 目录结构与编码规范修复错误，不得移动文件、不得改变架构层级，优先保证错误处理符合规范，避免引入unsafe代码，确保修复后代码可直接编译运行，且不破坏接口兼容性。

3. 新增 Tauri Command

请在 core/command/下新增 Command，命名遵循snake_case规范，只允许调用 connection_manager 提供的接口，禁止直接访问 datasource 层，确保Command类型安全，错误处理使用CoreError，同时在lib.rs中暴露该Command，支持前端通过tauri.invoke调用。

4. 新增 SQL 处理能力

所有 SQL 解析、构建、格式化、方言转换需求统一通过 `core/sql::SqlEngine` 静态方法调用，禁止业务模块直接 `use sqlglot_rust`。如需新增 SQL 能力，先在 core/sql/engine.rs 中定义方法签名，再在对应子模块（parser/builder/formatter/transpiler）中实现。

七、补充说明

- 本Skill适用于Trae CN AI编辑器，启用后AI将全程遵循上述规范，生成可直接编译、贴合项目需求的代码，无需重复说明项目规则。

- 技术栈升级需遵循既定策略，升级前需测试兼容性，确保不破坏核心功能与接口兼容性，升级后更新本Skill对应版本信息。

- 所有新增功能、修复均需符合项目架构与长期约束，确保项目轻量、高效、可扩展，支撑10年生命周期。
