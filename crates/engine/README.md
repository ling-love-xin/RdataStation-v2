# rds-engine — M2 双引擎与统一数据访问底座

> 本文件是 crate 的 **README 级入口**：只提炼模块特点与代码地图，完整设计以 `docs/architecture/` 为准（架构约定：crate 内不复制设计文档）。

## 一句话定位

engine 是**数据层与服务层的底座**：对上给 Feature crate 一套「连库 / 取元数据 / 跑 SQL / 存元数据」的能力，对下把**四类原生驱动 + DuckDB 分析引擎 + sqlglot 解析器**收在本 crate 内，并把「这条 SQL 怎么跑」收敛到 `SqlService` 一个入口。

## 模块特点

### 1. 分层严格向下：`services → driver → native`

```
commands ──► services ──► driver ──► native
```

（分层图原文见 `src/driver/mod.rs` 模块头。2026-09-19 之前这里写的是 `services → dbi → driver`，
而 `dbi` 层实测**零调用**（2124 行里只有一个静态工具方法活）、已删除——台账与理由见
`docs/architecture/data-layer-wiring-matrix.md` §5.3。）

- `services/`：执行侧服务（统一执行入口 / 解析 / DuckDB 专用 / 执行编排 / 快照）
- `driver/`：连接抽象层——trait + 注册表 + 连接池 + 内省；`native/` 是具体实现，`jdbc/` `wasm/` 是扩展入口
- 各层只允许依赖**更下层**；Feature crate 一律从 `engine` 顶层的再导出进入（`src/lib.rs` 是再导出清单）

### 2. `src/sql/` 是 sqlglot-rust 的唯一接入点

- 模块头即硬约束：「所有对底层 SQL 解析、生成、构建、优化、方言转换能力的调用，均通过此模块间接进行。业务模块不直接依赖 sqlglot-rust 的 API。」
- 对外只经 `SqlEngine` 门面 + `pub use` 的 `split_statements` / `highlight_spans`；`builder` / `formatter` / `parser` / `transpiler` 都是**私有**子模块
- **语句切分是自研的**（`split.rs` 词法级状态机），不用 sqlglot：切分要能在「文本还没写完」时工作，解析式切分会直接失败

### 3. 双层 × 双引擎的落点

| 层 | 引擎 | 内容 |
| --- | --- | --- |
| 系统级（共享） | `global.sqlite` + `shared.duckdb` | 连接模板、主数据、分析资产、插件、洞察规则、全局设置 |
| 项目级（物理隔离） | `project.sqlite` + `analysis.duckdb` | 项目名册之外的一切项目内数据 |

迁移资源因此分四个目录：`migrations/{global, project_meta, project_analysis, connection_metadata}`（当前最新：`global/024`、`project_meta/020`）。

### 4. 迁移是 SQL 资产，不是代码

- 加载 / 按版本执行 / 已应用追踪在 `src/migration/`；`global_init` 负责系统目录与全局库路径的确定
- **新增表 = 新增迁移文件**，不改已发布文件（M8 的 `insight_rule_index` 就是这样落在 `global/024` + `project_meta/019`）
- 库文件路径的唯一来源是 `migration::global_init::get_global_*_path`——不要在别处拼路径

### 5. 第三方能力「先实测，再封装」

新能力（血缘 / 类型标注 / 差异 / 下推 / 计划 …）**不允许**拿文档签名当依据直接接线。流程是：

```
台账标 ⚪ 的候选 ──► tests/ 里写探针跑真实 SQL ──► 回写台账的「行为级事实」
                                                     │
                                          确认产品需要 ▼
                              提升为 engine::sql::* 公开 API + 断言式单测
```

- 台账权威：`docs/architecture/editor/editor-prototype-design.md` §7.4（逐项带源码证据与实测结论）
- 提升前后的分界很清楚：**探针只断言「可再解析 / 不 panic / 文档化边界」这类与语义无关的不变量**，其余打印报告；精确断言属于 `sql/` 内的单测

## 代码结构

| 路径 | 职责 |
| --- | --- |
| `src/services/` | `sql_service`（统一执行入口：连接管理 + 缓存 + 历史）、`sql_parser_service`（解析 / 语句类型）、`duckdb_service`（临时表 / 行转 Arrow / 列洞察数据）、`execution_service`（串行并发编排）、`snapshot_service`、`result_types`、`connection_probe`（测试连接） |
| `src/driver/` | `traits.rs` + `registry` / `router.rs` / `factory.rs`（注册与构造）、`smart_pool` / `standard_pool`（连接池）、`introspection.rs` / `metadata.rs`（元数据）、`native/`（duckdb · mysql · postgres · sqlite，各带连接池）、`jdbc/` `wasm/` `missing_driver.rs` |
| `src/duckdb/` | 分析引擎封装：连接池、临时表、联邦查询（`federation/`）、本地加速（`accel.rs`）、导入导出、FTS、计划分析（`explain.rs`）、文件读取映射（`file_reader.rs`）、`snapshot.rs`、`metrics.rs` |
| `src/sql/` | SQL 原语（**sqlglot 唯一接入点**）：`engine.rs`（`SqlEngine` 门面）、`parser.rs`、`split.rs`（自研切分）、`highlight.rs`、`builder.rs`、`formatter.rs`、`transpiler.rs` |
| `src/persistence/` | 元数据持久化（SQLite）：连接 / 历史 / 日志 / 驱动 / 插件 / 网络档案（凭据加密）/ 环境变量 / SQL 模板 / 项目库与全局库（模块头记有本模块的 SQL 安全约定）+ **项目级回收站**（`trash.rs`：`ProjectTrash`，与具体模块无关，来源是 `origin` 字符串；M5 草稿箱与 M6 资产库共用，各模块界面按来源取用） |
| `src/cache/` | 多级缓存：LRU、查询缓存、元数据缓存、内存护栏、minicatalogs |
| `src/migration/` | 迁移系统 + `global_init`（系统目录 / 全局库路径 / 启动装配） |
| `src/logging/` | 统一日志：配置、记录、查询分页、统计 |
| `src/connection_manager.rs` | 运行期连接登记（`ConnId` → `ConnectionInfo`）；入口 `get_connection_manager` |
| `migrations/` | 迁移 SQL 资产（四目录，见特点 3） |
| `tests/` | **第三方能力探针**，不是模块测试（见下节） |

## `tests/` 里装的是什么（容易误读，先看这节）

`crates/engine/tests/` 里**不是**模块的集成测试，而是「第三方能力探针」这一独立类别：

| 文件 | 探什么 | 怎么跑 |
| --- | --- | --- |
| `sqlglot_capabilities.rs` | sqlglot-rust 的待用能力（作用域 / 血缘 / 类型标注 / 差异与差异不变量 / 下推 / 限定与展开 / 本地计划 / 转译单条限制 / 格式化注释保真） | `cargo test -p rds-engine --test sqlglot_capabilities -j 2 -- --nocapture --test-threads=1` |
| `transaction_affinity.rs` | 事务会话亲和（`BEGIN` 与后续语句是否落在同一物理连接）；4 类驱动 × 三档场景：临时表可见性 / 驱动级事务 / 并发亲和 | 同上，文件头列了所需环境变量；未设置则逐项跳过 |

- 之所以能放在这里：架构硬约束是「`crates/engine` 是 sqlglot-rust 的唯一接入点，其它 crate 不得直接引 sqlglot」——探针需要直接调 sqlglot 的**未封装**能力，唯一合法的落点就是 engine 自己的测试树
- 因此**不要**把探针改名为 `sql.rs`：`tests/sql.rs` 应留给「`engine::sql::*` 公开 API 的对外集成测试」，而今天这一层由 `src/sql/*` 内的单测覆盖（61 项：split 26 / highlight 13 / parser 8 / formatter 7 / builder 5 / transpiler 2）

## 依赖与改前必守

1. 依赖方向：`engine → shared`、`engine → connection`（驱动工厂与注册表引用连接层，见 `Cargo.toml` 注释）；**engine 不得依赖任何 Feature crate**
2. `sqlglot-rust` 只允许出现在本 crate；其它 crate 需要新能力时，先按「先实测再封装」提升为 `engine::sql::*`
3. 改表结构必须**新增**迁移文件；库文件路径只从 `migration::global_init` 取
4. 全量编译与测试必须限制并发（`cargo check-all` / `cargo test-all` 别名已含 `-j 2`）：并发链接重型 crate 会耗尽内存（DuckDB 已改动态链接）（`LNK1102` / `STATUS_STACK_BUFFER_OVERRUN`）
5. 需要外部服务的用例用 `#[ignore = "需要运行中的 MySQL 服务"]` 或环境变量守卫，默认 `cargo test` 不得依赖外部服务（现基线：`--lib` 24 项忽略）

## 测试与验证

| 目标 | 命令 | 基线 |
| --- | --- | --- |
| 库单测 | `cargo test -p rds-engine --lib -j 2` | **452 通过 / 24 忽略** |
| 8 个真机探针 | `cargo test -p rds-engine --tests -j 2` | 32 通过（无端点时逐项跳过） |
| sqlglot 探针 | `cargo test -p rds-engine --test sqlglot_capabilities -j 2 -- --nocapture --test-threads=1` | 10 通过（报告式输出） |
| 事务探针 | `cargo test -p rds-engine --test transaction_affinity -j 2` | 12 通过（无端点时逐项跳过） |
| 全量 | `cargo test-all` | 以重测为准（2026-09-19 第四次复跑：**85 个目标 2005 通过 / 51 忽略 / 0 失败**；含本机诊断 `zz_fixture_probe` 1 项，项目自身套件为 84 目标 / 2004 项。全量台账见 `../../docs/architecture/module-status.md`） |

## 文档地图

| 主题 | 权威文档 |
| --- | --- |
| 三层架构 / 双层数据 / 依赖方向 | `docs/architecture/overview.md` |
| SQL 原语与 sqlglot 能力台账（§7.4） | `docs/architecture/editor/editor-prototype-design.md`（探针输出回写此处） |
| 编辑器侧的 SQL 已知问题（§12） | `docs/architecture/editor/editor-architecture.md` |
| 元数据管线（driver → introspection → 缓存） | `docs/architecture/database/database-navigator-architecture.md` |
| 驱动 / 连接 / 隧道 / 凭据 | `docs/architecture/connection/connection-dialog-architecture.md` |
| 版本唯一入口与升级流程 | `docs/architecture/dependencies/dependency-strategy.md` |
| v1 → v2 迁移映射 | `docs/migration/v1-to-v2-mapping.md` |

## 已知与注意

- `src/persistence/mod.rs` 模块头列出本模块的 SQL 安全约定（系统查询用 PRAGMA / 运行时查询一律 `?N` 参数绑定 / 标识符用 `quote_identifier`），**改任何 store 前先读它**
- 新增依赖必须写进根 `Cargo.toml` 的 `[workspace.dependencies]`，crate 内只写 `dep.workspace = true`
