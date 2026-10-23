# 执行上下文模型：对标 DBeaver 的连接 / 事务 / 并发归属设计

> **写作背景**：0.2.4 现象 2（多文档执行回执被空闲面板抢走、结果不回填）修复后，复盘暴露出更上层的归属粒度问题——
> 我们的物理连接、事务、取消令牌都挂在 `conn_id`（数据源连接）上跨文档共享，而成熟桌面工具（DBeaver）把这些归属到「每个编辑器独立的执行上下文」。
> 本文核实 DBeaver 真实源码实现，对照本仓现状代码实测结论，定下一版本执行上下文模型的方向、分期路线与「不做什么」。
>
> **基线**：2026-10-23（0.2.4 发布线收尾期，本文为**下一版本规划**，0.2.4 不实现其中任何改动）。
>
> **证据口径**：
> - DBeaver 部分：直接读取开源仓库 `dbeaver/dbeaver` **devel 分支源码**（非产品行为转述），出处见 §7 附录 A。
> - 本仓部分：全部为代码实测，标 ［验］，锚点见 §7 附录 B（行号以现状为准，改动前先重读文件）。
>
> **不接 plugin**；未接通能力不写成已上线；本文不构成 0.2.4 交付物。

---

## 0. 结论先行（先回答三个决策问题）

### 0.1 我们要不要做连接池？——**不做通用连接池**

| 判断 | 依据 |
| --- | --- |
| 池化解决的是后端两类问题 | ① 并发吞吐：用有限长连接复用防打爆服务端连接数；② 摊薄每次 `Driver.connect` 的握手 / 认证开销 |
| 桌面工具这两个问题都不存在 | 查询并发是个位数，无吞吐压力；空闲时没有建连频次要摊薄。池化收益约等于零 |
| 池化的代价却必须自己扛 | 连接归还复用时必须**重置会话状态**：临时表、`SET` 变量 / `search_path` / 会话级 GUC、残留事务、未关闭游标、`LAST_INSERT_ID` 等，漏清一项就污染下一个上下文；还要额外处理并发借出、归属、并发竞态 |
| DBeaver 自己也不池化 | 社区版 JDBC 层 `openConnection` 每次 `Driver.connect` 新建、`closeConnection` 直接 `connection.close()`，正是为了绕开这套 session reset 逻辑（§1.3 实证） |

**本仓取向**：Source 档（网络驱动直连）采用「上下文持有独立物理连接，关闭即释放」，不做借出-归还式复用池；Accelerated / Federated 两档是本地嵌入式会话，根本没有网络连接池概念（§0.4）。

### 0.2 源库执行照搬 DBeaver 这种经过市场检验的方案，是不是更好？——**是，推荐照搬其上下文隔离模型**

DBeaver 模型经过十余年市场检验：**每个 SQL 编辑器一个独立执行上下文，上下文各自持有一条物理连接、独立事务、独立取消，结果集归属编辑器**。它比我们现状「`conn_id` 单连接 + 单事务槽 + 全局单忙位串行」语义更干净，也正好消解我们这轮碰到的跨面板共享状态竞态。

**裁剪照搬**（§4.2）：① 不照搬「打开编辑器即建连接」→ 改为**惰性建连**；② 不照搬「关闭即真关 TCP、无空闲回收」→ 补**空闲超时回收**；③ 补上 DBeaver 已有的成熟做法：**元数据浏览连接与 SQL 执行连接分开**。

### 0.3 「我们是本地查询工具，不会涉及多连接」要拆开看

这句话里其实混了两件事，方向不同：

- **同时挂几个不同数据源**：确实少，不是设计重点。
- **同一个数据源开多个 SQL 编辑器时是否各自独立会话**：DBeaver 默认就是多条独立连接、独立事务。**这才是本文要解决的归属问题**——我们现状两个文档绑同一个 `P_real_mysql`，底层共用一条物理连接、同一个事务槽，文档 A 的 `BEGIN` 会被文档 B 的提交 / 回滚误操作，这是语义隐患。

桌面规模下独立连接数量 = 活动上下文数（个位数到十几条 TCP），MySQL / PG 默认 `max_connections` 完全够用，不需要靠池化压数量。

### 0.4 三档通道在目标模型下的归属（先给结论，理由见 §3）

| 通道 | 现状归属 ［验］ | 目标归属 | 是否需要独立网络连接 |
| --- | --- | --- | --- |
| **Source 源库**（驱动直连，可写、有事务） | `conn_id` 一条物理连接 + 一个事务槽，跨文档共享 | **per-context 独立连接 + 独立事务 + 独立取消**，生命周期随上下文 | 是；不池化，惰性建连、关闭释放、空闲回收 |
| **Accelerated 本地加速**（DuckDB 内存会话只读挂载） | `SESSIONS[conn_id]` 复用一个本地内存 DuckDB | **维持按 `conn_id` 复用本地会话** | 否；嵌入式无网络握手，不占服务端连接，无需 per-editor 独立 |
| **Federated 联邦**（本地 DuckDB 多源挂载） | `SESSIONS[owner + 源指纹]` 复用一个本地会话 | **维持按计划 owner + 源指纹复用** | 否；底层源连接由 DuckDB 扫描器管理，本轮不扩模型 |

---

## 1. DBeaver 模型源码实证

### 1.1 层级：配置 → 数据源 → 实例 → 上下文 → 会话

```
DBPDataSourceContainer（导航树里那条连接配置 / 凭据）
 └─ DBPDataSource（已连上的数据源）
      └─ DBSInstance（实例；社区版单个默认实例 defaultRemoteInstance）
           ├─ Main 上下文 executionContext（默认主连接，1 条，兜底共享）
           ├─ Metadata 上下文 metaContext（元数据浏览；可独立一条，也可复用 Main）
           └─ allContexts[] 隔离上下文清单
                └─ 每个 SQLEditor 经 openIsolatedContext 创建的上下文
                     = 1 条物理 JDBC 连接 + 独立 autoCommit / 隔离级别 / 事务 / 监听器
                          └─ openSession(...) → JDBCSession（语句 / 结果集短期句柄，复用该连接）
```

### 1.2 上下文 = 一条连接 + 独立事务状态

`JDBCExecutionContext` 实证要点（`plugins/org.jkiss.dbeaver.model.jdbc/.../JDBCExecutionContext.java`）：

- 连接字段 `private volatile Connection connection`：**一个上下文持有一条物理连接**，不是池内多条。
- 事务状态是上下文实例字段：`autoCommit`、`transactionIsolationLevel`、`transactionListeners`；`commit(session)` / `rollback(session)` 操作的就是 `this.connection`。**事务边界 = 上下文边界，上下文之间事务互不影响**。
- 构造签名 `(JDBCRemoteInstance instance, String purpose)`，上下文归属实例；`connect()` 调 `dataSource.openConnection(monitor, this, purpose)` 取连接，`disconnect()` 调 `dataSource.closeConnection(connection, purpose, ...)` 归还 / 关闭。
- 语句并发：非线程安全驱动用 `SingleThreadLock`（`ReentrantLock` 公平锁 + 持有轨迹表）保证**同一上下文内语句 / 结果集顺序执行**；线程安全驱动为 `NoOpLock`。**不同上下文（不同连接）之间不互斥，可并行**。

接口层 `DBCExecutionContext`（`plugins/org.jkiss.dbeaver.model/.../exec/DBCExecutionContext.java`）：

- `getContextId()`：上下文唯一 id，创建时刻生成、终身不变。
- `getContextName()`：命名如 `Main`、`Metadata`、`Script X`——隔离上下文按用途 / 脚本命名。
- `openSession(monitor, purpose, task)`：上下文之上再开短期 `DBCSession`，purpose 分 MAIN / META / USER / UTIL。

### 1.3 JDBC 层不做池化

`JDBCDataSource` 实证（`plugins/org.jkiss.dbeaver.model.jdbc/.../JDBCDataSource.java`）：

- `openConnection(...)` 走 `JDBCConnectionOpener` 调 `Driver.connect(url, props)` **新建一条物理连接**；无连接复用缓存、无借出/归还句柄。
- `closeConnection(connection, purpose, doRollback)`：先按需 `connection.rollback()`（有未提交事务时兜底回滚，避免关闭带事务报错），再 `connection.close()`；`closingConnections` 列表只做重复关闭去重，不是池化。
- `createConnection(monitor, context, purpose, task)` 返回的 `JDBCConnectionImpl` 只是**会话包装**，不新建物理连接。

### 1.4 每个 SQLEditor 默认持有自己的隔离上下文

`SQLEditor` 实证（`plugins/org.jkiss.dbeaver.ui.editors.sql/.../SQLEditor.java`）：

- 编辑器自身持有 `private volatile DBCExecutionContext isolatedExecutionContext`，另有 `lastExecutionContext`、`executionContextProvider` 两个兜底来源。
- 是否独立连接由偏好 `EDITOR_SEPARATE_CONNECTION`（`ALWAYS` / `NEVER` / `AUTO`）+ `isForceUseSingleConnection()` 决定：
  - `AUTO` 且非强制单连接 → `initSeparateConnection` → `OpenContextJob` → `instance.openIsolatedContext(monitor, "SQLEditor <文件名>", instance.getDefaultContext(monitor, false))` 新建隔离上下文；
  - `NEVER` / 强制单连接 → 复用默认 Main 主上下文，所有编辑器共享那一条。
- `getExecutionContext()` 优先级：`isolatedExecutionContext` → `executionContextProvider` → 强制单连接时返回默认 Main。
- 隔离上下文初始化：先从 Main 拷贝默认 catalog / schema，`commitContextTransaction` 清掉初始化事务，之后独立。
- 生命周期：`releaseExecutionContext()` → `CloseContextJob` 异步关闭上下文；`promptToSaveOnClose()` 先取消运行中的 `QueryProcessor`、关闭结果标签、结束本上下文活动事务，再释放上下文。
- 结果集归属编辑器：`queryProcessors: List<QueryProcessor>`、`curResultsContainer`、`resultTabs` 全挂在 `SQLEditor` 实例；每个 `QueryResultsContainer` 回指产生它的上下文。
- 事务状态栏：`getTransactionStatusText()` 取 `getExecutionContext()` 的 `DBCTransactionManager`（`isAutoCommit` / `QMTransactionState` / `updateCount`）——**事务状态绑定在编辑器自己的上下文上**。

### 1.5 实例维护上下文清单

`JDBCRemoteInstance` 实证（`plugins/org.jkiss.dbeaver.model.jdbc/.../JDBCRemoteInstance.java`）：

- 字段 `executionContext`（Main）、`metaContext`（Metadata）、`allContexts: List<JDBCExecutionContext>`。
- `initializeMainContext()` 建 Main；`initializeMetaContext()` 按 `META_SEPARATE_CONNECTION` 偏好决定元数据是独立连接还是复用 Main。
- `openIsolatedContext(monitor, purpose, initFrom)`：`createExecutionContext` → `connect`（物理连接）→ `addContext` 登记进 `allContexts`。
- `shutdown(monitor, keepMeta)`：遍历 `allContexts` 逐个 `close()`；`removeContext` 同步清出 Main / Meta 槽位。

---

## 2. 本仓现状（代码实测 ［验］）

### 2.1 归属粒度总览

| 层 | 现状归属 | 锚点 |
| --- | --- | --- |
| 文档属性 | 每文档一份 `connection: Option<String>` / `channel` / `schema` | `crates/editor/src/service.rs` `Document`（L29–47），访问器 L133–144，`OpenRequest` L232–244 |
| 结果集 | **已按文档分槽**：`ResultStore.documents: Vec<DocumentResults>`，`sets(document)` / `active(document)` 按文档取 | `crates/editor/src/store.rs` L606–631、L692–709 |
| 执行队列 | **全局一份** `ExecQueue`，单个 `busy: AtomicBool` + 单工作线程 + 单 `running_connection` 槽；任意文档在跑，其余文档 `submit` 直接 `SubmitError::Busy` | `crates/editor/src/execution.rs` `submit` L1177（`is_busy()` L1489、`running_connection` L1200）；`crates/editor/src/shared.rs` `exec: Rc<RefCell<Option<ExecQueue>>>` L121 |
| 物理连接 | `connections: HashMap<ConnId, DynDatabase>`：**一个 conn_id 一条，跨绑定该连接的所有文档共享**；未绑定文档回退全局 `active_conn_id` | `crates/engine/src/connection_manager.rs` L114、L122；`crates/workbench/src/services/editor_exec.rs` `resolve_conn_id` L83–91 |
| 事务 | `transactions: Mutex<HashMap<ConnId, ActiveTransaction>>`：**每个连接至多一个事务，跨文档共享**；事务动作只收 `conn_id`，无文档参数 | `connection_manager.rs` L136、`begin_transaction` L649、`commit_transaction` L671、`rollback_transaction` L689；`crates/engine/src/services/sql_service.rs` 事务动作 L846–871 |
| 执行路由 | 事务键 `tx_key = conn_key_for(conn_id)`；`in_transaction` / `auto_begin` / `query_in_transaction(tx_key, ...)` 全按连接键 | `sql_service.rs` `execute` L437，`tx_key` L515、`in_transaction` L516、`auto_begin` L517、事务语句 L531–538 |
| 取消令牌 | `cancel_tokens: HashMap<ConnId, CancellationToken>`：每连接一个在跑查询令牌；单 `running_connection` 槽记录当前作业连接 | `connection_manager.rs` L130；`execution.rs` L1200 |
| 加速会话 | `SESSIONS: HashMap<conn_id, Arc<AccelSession>>`：本地内存 DuckDB，只读挂载源库，按连接复用 | `crates/engine/src/duckdb/accel.rs` `AccelSource` L194、`SESSIONS` L453、`ensure_session` L490 |
| 联邦会话 | `SESSIONS: HashMap<key, CachedFederation>`，键 = owner + 源集合指纹，本地多源挂载会话复用 | `crates/engine/src/duckdb/federation/session.rs` L458、`ensure_session` L501 |

### 2.2 现状的三个实际问题（对应本轮现象与后续债）

1. **事务跨文档共享**：两个文档绑同一 `conn_id` 时，共享同一个事务槽；文档 A 开的事务对文档 B 可见、可被 B 的 commit / rollback 误操作；状态栏 TX 徽标 / 事务时长 / 提交回滚按钮当前按连接口径呈现，不是文档自己的状态。
2. **全局单忙位串行**：`busy` 是全应用一个原子位，文档 A 跑长查询时，文档 B 提交直接被 `Busy` 拒绝，用户体感「编辑器卡住不能执行」；不同连接之间也被迫串行。
3. **取消单槽位**：`running_connection` 单槽在多文档 / 并发下会把取消信号发给错误连接（`execution.rs` 注释 L1197–1199 已自标该隐患）；事务内语句目前也无取消 / 超时入口（驱动 `Transaction` trait 未提供，见 `editor-showcase.md` 已声明边界）。

> 结果集归属这一层我们已经做对了：`ResultStore` 按文档分槽，与 DBeaver 结果集挂编辑器同构，目标模型保持不动。

### 2.3 口径澄清：现状所谓「连接池」不是借出/归还式池

`core-design-current.md` §9 把 `ConnectionManager` 表述为「连接池 + 隧道注册表 + 空闲回收」，代码实测它本质是**连接注册表**：`HashMap<ConnId, DynDatabase>` 每连接一条 `Arc<dyn Database>`，文件型库同 URL 走别名复用；**没有并发借出、归还、上限管理与 session reset**。本仓后续文档统一称「连接注册表 + 单连接复用」，避免与 §0.1 明确「不做」的通用连接池混淆。`idle_timeout`（默认 30 分钟，`connection_manager.rs` L138）现有空闲回收字段可在阶段 3 复用。

---

## 3. 逐维度对比 DBeaver

| # | 维度 | DBeaver（源码实证） | RdataStation 现状 ［验］ | 目标取向 |
| --- | --- | --- | --- | --- |
| 1 | 物理连接归属 | 默认每 SQL 编辑器隔离上下文一条独立连接；可配共享 Main | `conn_id` 一条，跨文档共享 | Source 档 per-context 独立连接 |
| 2 | 事务归属 | 每上下文独立 `autoCommit` / 隔离级别 / 事务对象 / 监听器，独立 commit / rollback | `transactions[conn_id]` 每连接一个，跨文档共享 | per-context 独立事务 |
| 3 | 事务状态展示 | 状态栏读自己上下文的 `DBCTransactionManager` | 按 `conn_id` 口径，非文档归属 | 阶段 1 先把 UI 归属修正到文档；阶段 3 落到独立上下文 |
| 4 | 执行并发 | 不同上下文（不同连接）可并行；同上下文 `StatementLock` 串行 | 全局一份队列 + 单 `busy` 位 + 单工作线程，全应用串行，忙时拒绝其他文档 | 阶段 2 按连接分组排队；阶段 3 按上下文分组，同上下文语句锁串行 |
| 5 | 取消 | 按上下文 / 当前语句句柄发出取消信号 | 单 `running_connection` 槽，按当前作业连接发令牌 | 阶段 2 改为作业 / 连接句柄持令牌；阶段 3 归属上下文 |
| 6 | 结果集归属 | 挂编辑器（`QueryProcessor` / `QueryResultsContainer` / `resultTabs`），容器回指上下文 | `ResultStore.documents[]` 按文档分槽 | 维持现状（已对齐） |
| 7 | 生命周期 | 上下文随编辑器打开建、关闭释放；关上下文先处理活动事务（回滚 / 提示） | 连接 / 事务随 `conn_id` 常驻，与编辑器生命周期解耦；关闭文档不释放连接 | 阶段 3：上下文惰性建连、关闭释放、空闲回收、活动事务先收尾 |
| 8 | 元数据连接 | 独立 `metaContext`（可配复用 Main），导航 / 补全 / schema 浏览不占执行连接 | 导航内省与 SQL 执行共用同一连接 / 队列，存在相互阻塞风险 | 阶段 3 拆出独立只读元数据上下文 |
| 9 | 连接池化 | 不池化：新建即 `Driver.connect`，关闭即 `connection.close()` | 连接注册表，单连接复用，非池化 | 维持不池化 |
| 10 | 本地分析会话 | 无对应物（纯 JDBC 直连模型） | Accelerated 按 `conn_id`、Federated 按 owner + 指纹复用本地 DuckDB | 维持现状复用键，不纳入 per-context 独立连接 |

---

## 4. 目标模型（方案 A：DBeaver 式执行上下文，推荐）

### 4.1 核心抽象：ExecutionContext（执行上下文）

> 命名注意：**不要复用** `crates/kernel/src/session.rs` 的 `Session`（Jupyter 风格内核会话）与 `crates/plugin` 的监督 `session_id`；本文新增的 `ExecutionContext` 专指 SQL 执行环境，三者术语分开，避免架构文档混淆。`editor-architecture.md` §1 概念表中「会话（SQL 会话 = 一个连接 + 一个 DuckDB 分析命名空间）」与 `SessionRegistry` 规划，即由本阶段落到该抽象上。

一个 `ExecutionContext` 承载：

| 字段 | 内容 | 对应现状 |
| --- | --- | --- |
| `context_id` | 创建时刻生成、终身不变的唯一 id（对齐 `DBCExecutionContext.getContextId()`） | 现状无此层 |
| `document` | 归属文档 `DocumentId`（结果集已按文档分槽，上下文与文档一一对应） | 文档级 `Document.connection` 保留为「绑定的数据源配置」 |
| `source_conn_ref` | 绑定的 `conn_id` / 数据源配置（连接参数、凭据、网络档案来源不变） | `Document.connection` |
| `driver_connection` | 上下文持有的独立物理连接句柄（Source 档）；未建连为 `None`（惰性） | `connections[ConnId]` 单条 |
| `tx_state` | `auto_commit`、隔离级别、活动事务对象、开始时刻、事务监听器 | `transactions[ConnId]` |
| `cancel_token` | 本上下文当前语句的取消令牌（替代全局单槽） | `cancel_tokens[ConnId]` + `running_connection` |
| `schema_default` | 上下文默认 catalog / schema（从数据源默认或 Main 拷贝，对齐 DBeaver 初始化拷贝） | `Document.schema` |
| `lifecycle` | 状态：`Idle`（未建连）/ `Connected` / `Disconnected` / `Closed`；活动事务标记 | 现状无显式状态机 |
| `result_slot` | 回指本上下文的结果集槽（`ResultStore` 已有文档分槽，只建引用关系） | 现状隐式按文档取 |

**归属规则**：一条 Source 物理连接只属于一个上下文；上下文不向其他上下文借出连接；Accelerated / Federated 本地会话不进入该生命周期，继续按现有复用键缓存。

### 4.2 生命周期（三处桌面化裁剪，不照搬 DBeaver 打开即建连）

1. **惰性建连**：文档绑定连接后**不立即拨号**；首次执行 SQL、首次需要元数据 / 补全，或用户显式「测试连接」时才建 `driver_connection`。降低空闲连接数，也避免打开一堆空编辑器时占满服务端连接。
2. **关闭释放**：文档关闭 / 解绑连接 / 数据源移除时——先做 DBeaver 同款收尾检查：有运行中语句先取消或提示；有未提交活动事务先提示提交 / 回滚（或按配置兜底回滚，对齐 DBeaver 关闭上下文前结束活动事务的做法）；收尾后关闭物理连接并把上下文置 `Closed`。
3. **空闲回收**：复用现有 `idle_timeout`（默认 30 分钟）口径，超时未活动的非事务上下文断开物理连接、回 `Idle`，需要时再惰性重建；**有活动事务的上下文不回收**。补一个独立连接数量兜底上限（防失控），达到上限时最久未活动的空闲上下文优先回收，而不是无限新建。

### 4.3 并发与语句锁

- **不同上下文的物理连接之间可并行**（对应 DBeaver 不同 `isolated context` 不互斥）。
- **同一上下文内语句 / 结果集顺序执行**：参照 DBeaver `StatementLock`——先按驱动能力标记线程安全与否；非线程安全驱动在上下文内加公平语句锁，语句与结果集抓取期间持有；线程安全驱动退化为 `NoOpLock`。**未确认驱动线程安全前，不得把全局单工作线程换成裸多线程**（避免引入连接内竞态）。
- 取消按上下文当前语句句柄发令牌；一个上下文只允许一个在跑语句（与语句锁一致），多语句脚本在上下文内排队执行。

### 4.4 元数据上下文独立

导航树浏览、schema / 列内省、代码补全走独立只读元数据上下文（对应 DBeaver `metaContext`），与用户 SQL 执行上下文分开：元数据慢扫描不阻塞执行连接，执行长查询也不挡补全。其连接策略同样惰性、可配置独立或复用主上下文（偏好开关对齐 DBeaver `META_SEPARATE_CONNECTION` 思路，本仓第一版先默认独立）。

### 4.5 方案 B（保守档，备选）

若阶段 3 时机不成熟，可先停在方案 B：保持 `conn_id` 单连接、单事务槽跨文档共享的物理模型不变，只做**归属 UI 修正**——事务状态、TX 徽标、提交 / 回滚按钮、提示按文档隔离呈现（§5 阶段 1 内容），并在界面如实声明「当前为单连接模式，同一数据源多编辑器共享事务」。接受事务跨文档共享的限制；不同连接之间可在阶段 2 先实现并行。

### 4.6 方案 C（连接池复用，明确不推荐）

不引入借出-归还式连接池。理由见 §0.1；即使未来出现多数据源频繁开关的场景，优先用「惰性建连 + 空闲回收 + 独立连接上限」控制数量，而不是把同一条物理连接借给多个上下文轮流使用。

---

## 5. 分阶段迁移路线（0.2.4 不实现，下一版本按序推进）

> 每阶段独立可发布、可回退；前一阶段未闭环不进入下一阶段。改动锚点以实施当日代码为准，先重读对应文件。

### 阶段 0（0.2.4 内已完成）：回执按文档隔离排空

- `ExecQueue` 新增 `drain_for(document)`，`EditorShared::drain_exec_for(document)` 透传；面板泵 `drain_exec_for(&self.document)` 只认领本文档回执，非归属回执直接跳过（不入库、不探测主键 / 列注释、不弹在途标题）。
- 结果集串台问题（现象 2）在文档粒度消除；**本阶段不动物理连接、事务与并发模型**。
- 状态：三处补丁已落盘编入 0.2.4 exe，待真机复测闭环（双文档交替执行互不覆盖 + 失败错误卡片回填）。

### 阶段 1（小，下一版本前段）：事务状态归属文档级 UI 修正

- **范围**：不动物理连接模型；把事务状态读取、TX 徽标、事务时长、提交 / 回滚按钮、自动提交开关、事务提示全部绑定到当前文档视图，不再跨文档显示连接级事务状态。
- **顺带清偿同根因债**：`host.rs` 现有 `tx_notes` / `commit_notes` / `source_notes` / `export_notes` 四通道仍是「先全量 drain 再按文档 `continue` 过滤」，且 pending 计数在过滤前就 `saturating_sub(1)`，存在与现象 2 同类的跨面板丢回执风险；本阶段改为与 `drain_exec_for` 同口径：按文档隔离排空，认领后再扣 pending。
- **验收**：两个文档绑同一连接时，A 文档的 TX 状态不出现在 B 文档状态栏；四通道回执归属正确、pending 计数不错减；单测覆盖「非归属回执被跳过、不计 pending」。
- **诚实口径**：本阶段物理层事务仍共享；界面文案不宣称「每编辑器独立事务」，待阶段 3 再切换表述。

### 阶段 2（中）：队列按连接分组并发，取消按句柄

- **范围**：队列从全应用一份 `ExecQueue` 改为**按 `conn_id` 分组**——不同连接的作业可并行提交与执行；同一连接内排队串行。`busy` 位 / `running_connection` 单槽改为每连接独立忙位与作业令牌。
- 取消令牌挂到连接当前作业句柄，取代全局单槽；`cancel_tokens[ConnId]` 语义保留，增加作业级标识。
- 驱动线程安全核验先行：非线程安全驱动在连接内加语句锁；线程安全驱动可并行语句（先以源码 / 驱动文档确认，不凭经验判断）。
- **验收**：连接 A 跑长查询时，连接 B 的文档可正常提交执行；同一连接多文档按序排队、互不并发；取消只命中目标连接当前作业；现有 3161 项测试基线不回归。

### 阶段 3（大）：ExecutionContext + per-context 独立连接 + 生命周期

- 引入 §4.1 的 `ExecutionContext` 抽象与注册表；Source 档每文档上下文持有独立物理连接、独立事务、独立取消令牌。
- 落地 §4.2 生命周期（惰性建连 / 关闭释放 / 空闲回收 / 数量上限兜底 / 活动事务先收尾）。
- 落地 §4.3 语句锁与上下文内串行；落地 §4.4 独立只读元数据上下文。
- 数据源配置层（`Document.connection` 绑定 `conn_id` / 配置引用）保持稳定；历史、结果集、补全缓存等按文档 / 连接现有键关系平滑迁移。
- **验收**：同一数据源两个文档各开独立事务，A 的未提交变更 B 不可见；A 的 commit / rollback 不影响 B；上下文关闭后连接真实释放（连接数回落）；空闲超时与活动事务保护生效；元数据浏览不阻塞执行。

### 不做清单（边界硬约束）

- 不引入通用连接池 / 借出-归还复用连接（方案 C）。
- Accelerated / Federated 本地会话不拆 per-editor 独立实例，维持现有复用键。
- 本规划**不接入 plugin**；插件侧会话模型另行设计。
- 不复用 kernel `Session`、plugin `session_id` 术语；不把执行上下文与 Jupyter 内核会话混为一层。
- 0.2.4 发布线只含阶段 0；阶段 1–3 均为下一版本及以后工作。

---

## 6. 待拍板项（实施前需要明确）

| # | 问题 | 建议 |
| --- | --- | --- |
| 1 | 选方案 A（per-context 独立连接，推荐）还是方案 B（单连接 + UI 归属修正） | 推荐 A；若排期紧，先交付阶段 1 + 阶段 2，阶段 3 单独排期 |
| 2 | Source 独立连接数量上限与空闲超时时长 | 复用现有 `idle_timeout` 默认 30 分钟；连接上限先给保守兜底值（如 16），达限回收最久未活动空闲上下文，实施时按真机连接类型复核 |
| 3 | 元数据上下文独立是否并入阶段 3 同批 | 建议并入；若拆出，单独排小阶段，避免阶段 3 范围膨胀 |
| 4 | 是否提供「单连接模式」偏好开关（对齐 DBeaver NEVER / forceUseSingleConnection） | 建议阶段 3 提供，默认独立连接；给保守用户一个共享 Main 主连接的选项，也便于回归对比 |
| 5 | 事务内语句取消 / 超时缺口 | 维持现状如实声明（驱动 `Transaction` trait 无入口）；阶段 3 重构连接模型时一并评估驱动事务对象能力补全 |

---

## 7. 附录

### 附录 A：DBeaver 源码出处（`dbeaver/dbeaver` devel，2026-10-23 读取）

| 文件 | 实证要点 |
| --- | --- |
| [`plugins/org.jkiss.dbeaver.model/src/org/jkiss/dbeaver/model/exec/DBCExecutionContext.java`](https://github.com/dbeaver/dbeaver/blob/devel/plugins/org.jkiss.dbeaver.model/src/org/jkiss/dbeaver/model/exec/DBCExecutionContext.java) | 上下文接口：`getContextId` / `getContextName`（Main / Metadata / Script X）/ `getDataSource` / `openSession(purpose, task)` / 上下文失效分阶段 |
| [`plugins/org.jkiss.dbeaver.model.jdbc/src/org/jkiss/dbeaver/model/impl/jdbc/JDBCExecutionContext.java`](https://github.com/dbeaver/dbeaver/blob/devel/plugins/org.jkiss.dbeaver.model.jdbc/src/org/jkiss/dbeaver/model/impl/jdbc/JDBCExecutionContext.java) | 单条 `Connection connection`；`autoCommit` / 隔离级别 / 事务监听器为上下文字段；`commit` / `rollback` 操作本连接；`SingleThreadLock` / `NoOpLock` 语句锁 |
| [`plugins/org.jkiss.dbeaver.model.jdbc/src/org/jkiss/dbeaver/model/impl/jdbc/JDBCRemoteInstance.java`](https://github.com/dbeaver/dbeaver/blob/devel/plugins/org.jkiss.dbeaver.model.jdbc/src/org/jkiss/dbeaver/model/impl/jdbc/JDBCRemoteInstance.java) | Main / Metadata / `allContexts[]`；`openIsolatedContext` 建独立上下文；`shutdown` 遍历关闭；`initializeMetaContext` 独立 / 复用偏好 |
| [`plugins/org.jkiss.dbeaver.model.jdbc/src/org/jkiss/dbeaver/model/impl/jdbc/JDBCDataSource.java`](https://github.com/dbeaver/dbeaver/blob/devel/plugins/org.jkiss.dbeaver.model.jdbc/src/org/jkiss/dbeaver/model/impl/jdbc/JDBCDataSource.java) | `openConnection` 新建物理连接；`closeConnection` 按需回滚后直接 `close()`；无池化；`createConnection` 仅会话包装 |
| [`plugins/org.jkiss.dbeaver.ui.editors.sql/src/org/jkiss/dbeaver/ui/editors/sql/SQLEditor.java`](https://github.com/dbeaver/dbeaver/blob/devel/plugins/org.jkiss.dbeaver.ui.editors.sql/src/org/jkiss/dbeaver/ui/editors/sql/SQLEditor.java) | `isolatedExecutionContext` 字段；`EDITOR_SEPARATE_CONNECTION` 偏好；`OpenContextJob`（run 内调 `openIsolatedContext`）；`releaseExecutionContext` / `CloseContextJob`；`queryProcessors` / `curResultsContainer` / `resultTabs` 归属编辑器；事务状态读本上下文 `DBCTransactionManager` |

### 附录 B：本仓现状锚点（代码实测，行号以现状为准）

| 文件 | 符号 / 位置 | 现状口径 |
| --- | --- | --- |
| `crates/editor/src/service.rs` | `Document.connection/channel/schema`（L29–47）、访问器 L133–144、`OpenRequest` L232–244 | 文档级绑定属性 |
| `crates/editor/src/store.rs` | `ResultStore.documents` L606–631、`sets/active(document)` L692–709 | 结果集已按文档分槽 |
| `crates/editor/src/execution.rs` | `ExecQueue::submit` L1177、`is_busy()` L1489、`running_connection` L1200、`drain_for(document)`（阶段 0 新增） | 全局单队列 / 单忙位 / 单连接槽；阶段 0 已按文档隔离排空 |
| `crates/editor/src/shared.rs` | `exec: Rc<RefCell<Option<ExecQueue>>>` L121、`drain_exec_for`（阶段 0 新增） | 队列跨面板共享 |
| `crates/engine/src/connection_manager.rs` | `connections` L114、`active_conn_id` L122、`cancel_tokens` L130、`transactions` L136、`scoped` L144、`current_catalog` L151、`idle_timeout` L138、`get_connection` L317、`begin_transaction` L649、`commit_transaction` L671、`rollback_transaction` L689 | 连接注册表；事务 / 取消按 `conn_id` |
| `crates/engine/src/services/sql_service.rs` | `execute` L437、`tx_key = conn_key_for` L515、`in_transaction` L516、`auto_begin` L517、事务内语句 L531–538、事务动作 L846–871 | 事务路由按连接键 |
| `crates/workbench/src/services/editor_exec.rs` | `resolve_conn_id` L83–91、`run` L378、三档分流 L399–404 | 未绑定回退全局活动连接；Source / Accel / Federated 分流 |
| `crates/engine/src/duckdb/accel.rs` | `AccelSource` L194、`SESSIONS[conn_id]` L453、`ensure_session` L490 | 本地加速会话按连接复用 |
| `crates/engine/src/duckdb/federation/session.rs` | `SESSIONS[owner + fingerprint]` L458、`ensure_session` L501 | 联邦会话按计划与源指纹复用 |
| `crates/editor/src/view/host.rs` | `tx_notes` / `commit_notes` / `source_notes` / `export_notes` 四通道（阶段 1 清偿） | 先全量 drain 再按文档过滤、pending 提前扣减的同根因竞态 |

### 附录 C：关联文档

- 执行模式 / 三档通道能力：[`../editor/editor-architecture.md`](../editor/editor-architecture.md)、[`../editor/editor-showcase.md`](../editor/editor-showcase.md)
- 连接配置 / 凭据 / 隧道模块边界：[`connection-module-spec.md`](connection-module-spec.md)、[`connection-dialog-architecture.md`](connection-dialog-architecture.md)
- 元数据缓存对标 DBeaver / DataGrip：[`../database/metadata-cache-vs-dbeaver-datagrip.md`](../database/metadata-cache-vs-dbeaver-datagrip.md)
- 引擎横切机制现状：[`../core-design-current.md`](../core-design-current.md) §9
