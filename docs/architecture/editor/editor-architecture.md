# SQL 编辑器模块 · 设计理念与架构（三模式：文本 / SQL / 分析）

> 状态：**设计稿（待确认，2026-09-15）** · 本文回答**为什么这样设计 / 怎么运转**：概念模型、分层与依赖、状态所有权、数据流、决策取舍、降级容错、测试策略、实现映射、已知问题。
> 长什么样 / 怎么交互 → `editor-prototype-design.md`；做到哪 / 下一步 → `editor-dev-plan.md`；模块入口 → `README.md`。
> 本文的"现状盘点"（§7）与"已知问题"（§12）是**权威清单**，含逐条代码证据。

---

## 1. 定位与边界

**一句话**：把「写 SQL → 连库执行 → 看结果 → 继续分析」这件事做成一个**键盘优先、状态可信、可渐进升级到笔记**的编辑环境——文本模式是它的退化形态，分析模式是它的进化形态。

| | 做 | 不做 |
| --- | --- | --- |
| 编辑 | 文本缓冲、高亮、补全、诊断、查找替换、多文档、脏状态与保存 | 通用 IDE 能力（Git 集成、多根工作区、终端、调试器） |
| 执行 | 连接绑定、执行（选区/当前语句/全部/批量）、取消、超时、事务、结果集、历史 | 数据建模／DDL 向导／表设计器（属 M4/M6 或未来模块） |
| 分析 | 单元（Cell）模型、会话（Session）、内联输出、`.rdsnote` 持久化 | Jupyter 全协议前端；Python/Rust 内核第一期不做（只留扩展位） |
| 边界 | 连接的新建/编辑属 M3；对象树与内省属 M4；Mock 属 M7；洞察属 M8 | **不复制**上述模块的设计与文档，只经服务/命令协作 |

模块与 crate 映射（`docs/architecture/overview.md` 的九大模块之外，"工作台"与"设置"是横切模块）：

| 关注点 | crate | 说明 |
| --- | --- | --- |
| 编辑器内核 + 三模式视图 + 执行编排 | **`crates/editor`（新建，见 §3.2）** | 本期主体 |
| 单文件 SQL 编辑器（历史现状） | `crates/workbench/src/panels/editor.rs::EditorPanel` | 1a/1b 完成后按 §3.3 收编或退役 |
| SQL 执行/解析/转译/缓存/历史存储 | `engine` | 已迁移，多数接线未做（§7） |
| 元数据（补全数据源） | `database`（`MetadataService`） | 已迁移，零消费 |
| 单元/会话持久化 | `engine::persistence::workbench_context_store`（+ 新表） | 已有 `EditorContext` 表，需扩展 |

---

## 2. 概念模型

### 2.1 核心实体

```
                       ┌──────────────┐
                       │  Workbench   │  五段布局 / 活动栏 / Dock / Quick Open
                       └──────┬───────┘
                              │ 装入中央区
        ┌─────────────────────┴──────────────────────┐
        │            EditorHost（宿主，每文档一份）      │
        │  模式 / 只读 / 脏 / 连接绑定 / 光标 / 选区     │
        └───────┬──────────────────────────────┬──────┘
                │                              │
        ┌───────▼────────┐            ┌────────▼─────────┐
        │   Document     │            │    NoteBook      │
        │  （文本/SQL）   │            │  cells: [CellId] │
        └───────┬────────┘            └────────┬─────────┘
                │                              │ 1..n
                │                     ┌────────▼─────────┐
                │                     │       Cell       │  id / kind / source
                │                     │  state / run_seq │
                │                     └────────┬─────────┘
                │                              │ 0..n
                │                     ┌────────▼─────────┐
                │                     │     Output       │  kind + 引用/预览
                │                     └──────────────────┘
                │
        ┌───────▼───────────────────────────────────────┐
        │  ResultStore（结果单权威）                     │
        │  (document_id, connection_id, run_id) → ResultSet
        └───────┬───────────────────┬───────────────────┘
                │ 视图 A            │ 视图 B
        ┌───────▼──────┐    ┌───────▼────────┐
        │ 结果面板(Dock)│    │ 单元内联输出    │
        └──────────────┘    └────────────────┘

        ┌──────────────────────────────────────────────┐
        │ Session（内核抽象）                           │
        │   SqlSession（本期） / PythonSession / …（后续）│
        │   execute / interrupt / shutdown / namespace  │
        └──────────────────────────────────────────────┘
```

### 2.2 术语表（本项目内固定，避免同物异名）

| 术语 | 含义 |
| --- | --- |
| **文档（Document）** | 一份可编辑的文本资产：文本模式与 SQL 模式共用；键是 `document_id`（稳定，不是路径） |
| **笔记（NoteBook）** | 单元序列 + 会话绑定，落 `.rdsnote` |
| **单元（Cell）** | 笔记的最小执行单位：`cell_id` + 种类 + 源 + 输出 + 执行态 |
| **输出（Output）** | 单元的一次执行产物；**只存引用 + 预览**，不存全量数据 |
| **会话（Session / Kernel）** | 执行环境：SQL 会话 = 一个连接 + 一个 DuckDB 分析命名空间；Python/Rust 会话为后续内核类型 |
| **结果集（ResultSet）** | 一次语句执行的结果数据（列 + 行引用 + 耗时 + 截断信息）；`engine::services::result_types::ResultSet` 已定义 |
| **模式（Mode）** | 文档/笔记的能力档：`Text` / `Sql` / `Analysis`；由**能力表**驱动 |
| **当前语句** | 光标所在语句（由词法级切分定位），是 SQL 模式默认执行目标 |
| **stale** | 单元输出与当前会话状态不一致（改了源未重跑、会话重启、依赖单元变化） |

### 2.3 不变式（Invariant）

1. **单一权威**：任一状态只有一个可写位置——执行入口只有 `EditorService::execute`；结果只有 `ResultStore`；脏状态只有 `EditorHost.dirty`。
2. **稳定标识**：`document_id` / `cell_id` / `result_id` 一经生成永不变，**不作为任何排序/下标语义使用**（V1 用 `filePath` 派生 panel id 并在另存为后失配，是反面案例）。
3. **位置用锚点不用行号**：诊断、输出归属、书签一律锚在位置（偏移/锚点），编辑时随文本漂移。
4. **render 零 I/O**：加载/执行/内省一律走后台任务 + 结果队列（`nav_jobs` 模式），UI 线程只读内存。
5. **零裸值**：颜色取主题 token、尺寸取 `ui.rs` 常量（视图层 `px(` 会被契约测试拦下）。
6. **降级不静默**：任何降级（无连接、元数据不可用、格式化失败、大文件裁剪）都必须有可见说明与恢复入口。

---

## 3. 分层、依赖与 crate 归属

### 3.1 分层

```
表现层   gpui-kit（input::editor / highlighter / table / dock / menu / status_bar）
   ▲
服务层   crates/editor（本期新建）：model / mode / service / execution / completion / session / commands / view
   ▲
底层     engine（SqlService / SqlEngine / history_store / query_cache / workbench_context_store）
         database（MetadataService 内省）· shared（错误类型 / Arrow / 工具）
```

依赖方向：`editor → engine, database, shared, gpui-kit`（Feature → engine/shared/UI 基础设施，符合 `docs/architecture/overview.md` 硬约束）；`workbench → editor`（壳层装配）；**editor 不依赖 workbench**。

### 3.2 为什么独立成 `crates/editor`

按 `settings-crate-design.md` §1 的判定四条：

| 判定项 | 结论 |
| --- | --- |
| 独立状态与生命周期 | ✅ 多文档、模式、会话、结果集，独立于窗口与项目生命周期 |
| 稳定公开边界 | ✅ 对外只暴露 `EditorService`（打开/关闭/执行/保存）+ 一组 GPUI Action |
| 使用方 ≥2 | ✅ ① workbench 中央区；② M4 导航「在 SQL 编辑器中打开 / 查看数据」；③ M5 草稿箱打开文件；④ M7/M8 产物预览（后续） |
| 是否有更合适的归属 | `workbench` 现已是壳层（布局/活动栏/Dock/Quick Open）且 `panels/` 达 7343 行——把编辑器本体继续塞进去会让壳层与本体互相污染 |

替代方案：先留在 `workbench` 内（改动小），代价是 1c 阶段必然要拆分（notebook 是独立状态与生命周期的典型）。**建议独立**，并在 dev-plan 的 Phase 0 完成骨架与接线。

### 3.3 与现有 `EditorPanel` 的关系（迁移而非并存）

| 现有内容（`crates/workbench/src/panels/`） | 去向 |
| --- | --- |
| SQL 区块（`sql_textarea` / `query_result` / `sql_history` / 执行闭包，L6094-6104、L6777-6998） | 迁入 `editor` 的 `view/sql_mode.rs` + `execution.rs`，**闭包内同步 I/O 全部删除** |
| 连接详情卡 / 数据库导航树（L6566-6773） | 属 M3/M4 的"连接概览"，迁出编辑器面板（回 `workbench` 的独立面板或并入 M4） |
| 属性面板宿主（L6259-6464） | 保留在 `workbench`（M4 契约：属性面板停靠编辑区右侧） |
| 连接对话框宿主（`dialog` 字段与 `request_*`，L6090-6202） | 保留在 `workbench`：它是**宿主层职责**（对话框层挂在 `WorkbenchView::render`） |
| `Shared::editor_dirty` / `editor_sql` / `editor_set` / `editor_clear` | 由 `EditorService` 提供的等价查询/命令取代（`ProjectEditorBridge` 契约不变，见 §3.4） |

### 3.4 对外契约（其它模块怎么用编辑器）

| 使用方 | 契约 | 说明 |
| --- | --- | --- |
| M4 导航「在 SQL 编辑器中打开」 | `EditorService::open_sql(conn_id, sql) -> DocumentId` | 现有 `Shared::editor_set` + `SidebarEvent::OpenSqlEditor` 的语义升级 |
| M4 「查看数据」 | `execute(OpenSql, conn_id, sql, Target::All)`（可自动执行） | 关闭 dev-plan 遗留项“查看数据仅注入不执行” |
| M4 表数据浏览 / 编辑（驱动能力位 `table_editor`，已声明零消费） | 结果集**只读**；行内编辑 + 写回源库 + row identity + 事务属 M4 | 避免重演 V1 的“结果集与表数据语义纠缠 + 脏状态双轨（`dirtyRows` vs `dirtyCells`）”；两者共享网格**渲染层**，各自持有数据（见 §3.6） |
| M5 草稿箱打开文件 | `open_path(path) -> DocumentId`（按 §1.2 规则定模式） | 草稿箱不再自己造"草稿文件模式" |
| M1 项目管理（未保存拦截） | `ProjectEditorBridge`（`is_dirty` / `sql` / `clear` / `mark_clean`） | **契约不变**，实现改由 `EditorService` 提供（M1 侧零改动） |
| M7 Mock / M8 洞察 | 输出类型 `Output::Insight` / `Output::Table` | 第二期 |
| 插件 | 不开放编辑器 API（第一期） | — |

### 3.5 计划中的模块划分

```
crates/editor/src/
├── model.rs        Document / DocumentId / EditorMode / Capabilities / ReadOnly / Dirty 基线
├── notebook.rs     NoteBook / Cell / CellKind / Output / OutputKind / run_seq / stale 判定
├── mode.rs         模式判定规则表（§1.2）+ 切换矩阵执行（§1.3）+ 能力表（唯一分叉点）
├── service.rs      EditorService：打开/关闭/激活/模式切换/保存/脏/查询（唯一权威）
├── execution.rs    执行编排：目标解析（选区/当前语句/全部/批量）+ 后台任务 + 结果回填
├── completion.rs   schema 补全（MetadataService + 关键字/函数 + 上下文判定 + 缓存）
├── session.rs      Session trait + SqlSession（DuckDB 分析命名空间 + 临时表变量）
├── store.rs        ResultStore（结果单权威：每文档一份**结果集列表 + 选中项** + 上限淘汰）
├── persist.rs      工作区上下文（光标/选区/展开态）+ `.rdsnote` 读写
├── commands.rs     Actions：ExecuteCurrent/ExecuteAll/ToggleComment/Format/…
└── view/
    ├── host.rs          EditorHost（多文档 + 标签条 + 模式分发）
    ├── text_mode.rs     文本模式（极简）
    ├── sql_mode.rs      SQL 模式（工具栏 + 编辑区 + 结果区）
    ├── notebook_view.rs 分析模式（笔记本头 + 单元 + 输出）
    └── widgets/         结果网格 / 状态栏 / 补全弹层适配 / 输出块
```

> 语句切分（「执行当前语句」与「批量执行」的基础）**不在本 crate**：它是纯 SQL 文本原语，
> 已落在 `engine/src/sql/split.rs`（`core/sql` 是 sqlglot 的唯一接入点），editor / 其它消费方直接调用。

### 3.6 网格（表格）归属与提炼时机

“表格”在路线图里是**三件事**，不能混成一个 crate：

| | ① 结果集网格 | ② 表数据浏览 / 编辑 | ③ 网格渲染层 |
| --- | --- | --- | --- |
| 数据来源 | 一次执行的产物（`ResultSet`，驻 DuckDB 临时表） | 源库某表的分页抓取 | 无关（由数据源 trait 提供） |
| 生命周期 | 结果集（会话内 / 可持久化引用） | 面板/标签（+ 事务） | 无状态 |
| 可写性 | **只读**（D23） | 可编辑并写回源库（row identity + 事务） | 不适用 |
| 归属 | `crates/editor` | `crates/database`（M4 后续项；驱动能力位 `table_editor` 已声明零消费） | 先放 `editor/src/view/widgets/grid/` |

**为什么不建独立 grid crate（现在）**：① 项目判定标准明确“**不建 crate 的对象：无独立状态的能力**”（如主题）——纯渲染+交互层无独立业务状态；② 1a/1b 期只有**一个**真实使用方，此时冻结接口基本必错；③ 重活（虚拟滚动、表格基座）已由 gpui-component 的 `table::{DataTable, TableState, TableDelegate}` 提供，自写部分只是 delegate + 交互，量级撑不起 crate 边界。

**接口先行（让将来提炼是纯机械移动）**：

```rust
trait GridDataSource {            // 谁提供数据、怎么翻段（无业务状态）
    fn columns(&self) -> &[ColumnMeta];
    /// 已知总行数（源库尚未统计完时为 None → 界面显示 "1000+"）
    fn known_row_count(&self) -> Option<u64>;
    /// 当前已抓取的窗口（大表不是“全量”，而是“已抓取的一段”）
    fn window(&self) -> RowWindow;              // { start, fetched, total: Option<u64> }
    /// 取下一段（由执行层发起后台抓取，UI 只读已抓取数据）
    fn fetch_next(&self, limit: u32) -> Result<RowWindow, CoreError>;
    /// 已抓取范围内的取数（纯内存读，render 期可用）
    fn rows(&self, offset: u64, limit: u32) -> Vec<Row>;
}

trait GridEditSink {              // 只有“可编辑网格”实现（结果集不实现 → 天然只读）
    fn identity(&self) -> RowIdentity;
    fn commit(&self, changes: &[CellChange]) -> Result<(), CoreError>;
}
```

> **分段抓取**（D24）：接口按 DBeaver 的 “fetch size / 取下一段” 语义设计——一次全量 + 截断
> 在亿行级表上不可用，且该接口形状一旦定型就难改（影响导出：需区分“仅已抓取”与“抓全量”）。

**提炼触发（三条全中才动手）**：① 第二个真实使用方出现（表数据浏览，或 Mock/洞察预览复用）；② trait 形状稳定一个迭代；③ 共享部分规模够（~500 行以上）。提炼落点二选一：`shared/`（≥2 使用方的稳定能力进 shared，且 shared 已允许依赖 gpui-base/gpui-component）或独立 `crates/grid`（若届时它已拥有自己的状态）。

**共享部件**（将来一起搬）：虚拟滚动适配 · 列头（排序/列宽/隐藏）· 单元格渲染器（NULL/数字/长文本/JSON）· 值查看器 · 复制（单元格/行/JSON/INSERT）· 分页 · 列状态持久化 · 导出菜单。
**永不合并**：数据源与分页策略 · 编辑缓冲与提交 · row identity / 主键解析 · 事务与冲突 · 权限与只读策略。

**例外**：若产品把“表数据编辑”定为一等公民（而非 SQL 的附属展示），则应建独立 `crates/dataset`（它此时确有独立状态：列宽/排序/筛选/编辑缓冲/提交状态），`editor` 与 `database` 均依赖它。判据只有一条：**表编辑是“附属展示”还是“独立能力”**——按当前文档口径（M4 只给了“查看数据”入口、`table_editor` 仅为能力位）定为**附属**。

---

## 4. 状态所有权（状态地图）

| 状态 | 存放 | 生命周期 | 谁写 | 谁读 |
| --- | --- | --- | --- | --- |
| 打开文档集合 / 活动文档 | `EditorHost`（`Shared` 化） | 应用 | `EditorService::{open,close,activate}` | 标签条、宿主、状态栏 |
| 文档内容与撤销栈 | **编辑器内核**（`input::editor::EditorState`，每文档一个 Entity） | 文档 | 内核（输入/程序化编辑） | 内核渲染、保存、执行 |
| 模式 / 只读 / 连接绑定 / 方言 | `Document` | 文档 | `EditorService::set_mode` / 连接选择器 | 能力表消费方、状态栏 |
| 脏状态 | `Document.dirty`（**与基线比较的派生量**） | 文档 | 内核 `InputEvent` → 与 `baseline` 比较 | 标签脏点、关闭确认、M1 拦截 |
| 光标 / 选区 | 内核（`EditorState` 自带） | 文档 | 内核 | 状态栏、执行目标解析 |
| 结果集 | `ResultStore`（每文档一份**结果集列表 + 选中项**；`run_id` / 血缘字段在 B5 引入） | 会话内（可持久化引用） | `execution` 回填 | 结果面板、内联输出、导出 |
| 执行态（运行中/耗时/可中断） | `Document.exec_state` / `Cell.state` | 单次执行 | `execution` | 状态栏、单元卡片、中断按钮 |
| 会话（连接 + DuckDB 命名空间 + 临时表） | `SessionRegistry`（按 `session_id`） | 应用（可显式重启） | `session` | 单元执行、变量面板 |
| 笔记结构（单元顺序 / 折叠 / 选中） | `NoteBook` | 笔记 | `EditorService::{insert,move,delete}_cell` | 笔记本视图 |
| 补全元数据缓存 | `completion`（按 `conn_id + catalog + schema`，TTL 30s） | 应用 | 补全请求 | 补全弹层 |
| 历史 | `engine::persistence::history_store`（全局 SQLite/JSON） | 应用 | 执行成功后 | 历史面板、Quick Open |
| 工作区上下文（光标/选区/模式/连接） | `engine::persistence::workbench_context_store` | 应用 | 保存/失焦/退出 | 打开文档时恢复 |

**所有权铁律**：编辑器内核**不持有**结果集与会话；`ResultStore`/`SessionRegistry` **不持有**文本。三者交叉只能经 `EditorService` 与事件。

---

## 5. 数据流

### 5.1 打开文档（含模式判定与恢复）

```
open_path / open_sql
  → mode::resolve（§1.2 规则表：显式 > 扩展名 > 入口语义）
  → 查 workbench_context_store（光标/选区/上次连接）→ 命中则回填
  → 建 Document + 内核 Entity（语言配置按模式：plaintext / sql / 单元）
  → 若 SQL 模式且已绑定连接：后台预取补全元数据（不阻塞渲染）
  → 标签条插入 + 激活
```

### 5.2 模式切换

```
切换请求（工具栏 / 命令）
  → mode::plan(from, to, doc) 返回 { 需要的确认, 内容变换, 副作用 }
  → 需要确认 → 对话框（§1.3 矩阵）→ 用户确认
  → 执行变换（文本↔SQL：零变换；SQL→分析：整篇入单单元；分析→SQL：拼接 SQL 单元）
  → 更新 Document/NoteBook + 能力表 → 重建或复用内核 Entity → 状态栏与工具栏切换
```

### 5.3 执行（SQL 模式核心链路）

```
Ctrl+Enter / 执行按钮
  → execution::resolve_target（选区 > 当前语句（split.rs 定位）> 全部）
  → 校验：SQL 非空、已绑定连接、只读策略（写语句？）、并发（同文档已有执行中？）
  → 主线程：置 exec_state = Running（状态栏 spinner + 中断按钮）
  → 后台任务（单工作线程 + mpsc，参考 services/nav_jobs.rs）：
        engine::SqlService::execute(conn_id, sql, SqlExecuteOptions{ record_history, use_cache, timeout_ms })
        （取消：cancel_query(conn_id)；超时：timeout_ms）
  → 结果回填（主线程轮询队列）：
        ResultSet → ResultStore.push(document_id, connection_id, run_id)
        历史：append（**需先修：耗时/成功/行数必须真实**）
        错误：解析位置 → 诊断 + 定位 + 聚焦（无位置则只提示）
  → exec_state = Idle；结果面板/内联输出渲染
```

### 5.4 批量执行

```
split.rs → Vec<Statement{range,text}>
  → 逐条串行调用同一个执行内核（非一个事务；失败不中断）
  → 每句一个结果集（标签 = 语句摘要 + 状态点）
  → 用户中断 → 剩余语句标"已取消"
```

### 5.5 结果渲染

```
ResultStore.get(...)   // 每个结果集携带：通道徽标（源库/加速/联邦）+ 血缘摘要
  → 视图 A（结果面板）：表头 + 虚拟滚动网格 + 工具栏（筛选/下发源库/分析/导出/刷新）+ 状态行
  → 视图 B（内联输出）：前 N 行预览 + “展开”切到视图 A
     （同一 ResultSet，不复制数据；大数据只在 DuckDB 临时表里）
```

> 结果区从 V1 的“三模式过滤器”收敛为「**筛选（本地 / 下发源库）+ DuckDB 分析**」两入口，且重查与分析**产生新结果集**（血缘）而非就地替换——详见 §5.9 / §5.10。

### 5.6 补全

```
输入触发 → completion::request(document, position)
  → 上下文判定（别名后 / FROM-JOIN 后 / SELECT 后 / WHERE 后 / 默认）
  → 元数据：缓存命中（TTL 30s）→ 命中即用；未命中 → 后台请求 MetadataService（异步，不阻塞输入）
  → 合并：别名列(4) > 表(3) > 列(2) > 函数(2) > 关键字(1)
  → 弹层渲染；元数据不可用 → 仅关键字+函数 + 一行说明
```

### 5.7 分析模式：单元执行与会话

```
Shift+Enter / 单元 ▶
  → session::get_or_create(session_id)（会话必须已绑定连接）
  → Session::execute(cell.source)：
        SqlSession：在 DuckDB 分析连接上执行；CREATE TEMP TABLE 落入会话命名空间（= 变量）
  → Output 写回 cell.outputs（引用 + 预览）+ run_seq = session.seq
  → stale 重算：本单元及"依赖其变量"的下游单元
```

### 5.8 保存 / 脏 / 恢复

```
输入事件 → 与 baseline 比较 → dirty 更新（标签脏点 + 状态栏）
Ctrl+S   → 写盘（文件型）或写 .rdsnote（笔记型）→ baseline 更新 → dirty = false
          → 保存成功 → 清除崩溃快照
外部修改 → 保存前比较 mtime → 三态（重载 / 覆盖 / 取消）
恢复     → 启动读快照（内容 + 光标 + 滚动）→ 用户确认后回填（**不可只恢复文件名，V1 的坑**）
```

### 5.9 筛选下发源库（重查）

```
结果工具栏「筛选」输入 + 「▢ 下发源库」开关（默认关 = 本地视图过滤）
  → 开关打开 → 后台任务：engine::services::execution_service::re_execute_with_filter(
                        conn_id, 原 SQL, where_clause, order_clause)
        实现：SELECT * FROM (<原SQL>) AS _result WHERE <条件> [ORDER BY …]
  → **产生新结果集**（不覆盖原结果集）：lineage = { 原结果集 id, 条件, 作用域=源库, 通道 }
  → 原结果集仍可回看；标签显示 `结果 N〔源库·筛选〕`
```

- **本地 vs 源库的语义差异必须显式提示**：本地 = 只隐藏行（不重查、看不到未抓取的行）；源库 = 重查（数据更新、能看到未抓取行，但有往返成本）。
- **待定（§12 #15）**：原查询含 `ORDER BY` / `LIMIT` 时的包裹语义。

### 5.10 DuckDB 分析（派生新结果集 / 分析单元）

```
结果工具栏「分析 ▾」
  → 后台任务：engine::services::execution_service::execute_duckdb_analysis(temp_table, sql, columns?, rows?)
        temp_table 来自结果集（ResultSet.temp_table）；“桥接”时先用当前可见行 create_duckdb_temp_table
  → **产生新结果集**：lineage = { 源临时表, 分析 SQL, 通道=DuckDB }
  分析模式下：同一次分析可“沉淀为单元”——
       单元格源 = 分析 SQL，输出 = 新结果集，临时表 = 会话变量（下游单元可引用，§5.7）
```

> **定位**：分析不是“过滤的第三种模式”（V1 如此），而是**派生新数据集**。它是“查询后分析”这条产品主线的入口：从结果区快捷发起，在分析模式下沉淀为可重跑、可引用的单元。

---

## 6. 关键设计决策

| # | 决策 | 理由 | 代价 / 取舍 |
| --- | --- | --- | --- |
| D1 | **一个内核 + 三档能力表**，不做三套编辑器 | V1 把同一套模式规则复制三处、工具栏/状态栏各写一套，产生系统性分叉 | 能力表需覆盖全部差异，新增能力要同时登记能力位 |
| D2 | 模式是**文档属性**并持久化（含连接绑定、光标） | 用户预期 `.sql` 下次打开还是 SQL 模式且连着同一库 | 需要文档元数据存储（`workbench_context_store` 扩展） |
| D3 | 模式切换**显式且有代价分级**（§1.3） | V1 静默 `changeFileType` 导致三处状态不同步 | 多一次确认交互 |
| D4 | **只读两维度分离**（编辑器只读 / 连接只读） | 二者语义完全不同却常被混为一谈（V2 现状用 `project_ui.read_only` 拦执行） | 状态栏与工具栏需各表达一次 |
| D5 | 执行入口**唯一**：`EditorService::execute(target)` | 现执行逻辑内联在 render 闭包里 → 不可测、不可复用（M4 自动执行卡在此） | 需先抽管线（dev-plan Phase 0） |
| D6 | 结果**单权威 + 多视图**（ResultStore） | V1 同一结果双写两个模型；V2 现状 `sql_for` 只做归属校验 | 需定义"内联 ↔ 面板"的切换语义 |
| D7 | 语句切分**重做为词法级**（纯函数 + 表驱动测试） | 现状 `sql.split(';')` 会把字符串/注释/`$$` 块内的分号切断 → "当前语句/批量"根本不可用 | 需覆盖各方言字面量与注释形式 |
| D8 | 格式化**重做**（替换 Debug 打印实现） | `SqlEngine::format` 现为 `format!("{:?}", stmt)`，输出不是 SQL（测试只断言非空所以没拦住） | 要么接 sqlglot generator，要么引入/自写格式化器；择一后补回归测试 |
| D9 | 历史记录**字段真实化**（耗时/成功/失败原因/行数贯通） | 现状 `save_sql_history` 硬编码 `db_type=unknown`、`mark_success(0)`、失败不写 | 需改 `SqlService::execute` 把 `elapsed_ms` 与语句类型传下去 |
| D10 | 补全走 **`database::MetadataService` 实时内省 + 会话级缓存** | V2 已有实时内省（异步）；`minicatalogs` 是空占位，不能作为数据源 | 无连接时无 schema 补全（降级为关键字/函数） |
| D11 | **不引入 LSP 作为内部协议** | 编辑器是单进程内 Rust，LSP 只会带来序列化与进程开销；LSP 只在将来接外部语言服务时当适配层 | 需自建补全/诊断数据结构（成本可控） |
| D12 | SQL 高亮改用 **sqlglot tokenizer**（`engine::sql::highlight` 产出「字节区间 + 类别」），**不引入 tree-sitter** | sqlglot 已在依赖里且自带 `tokens`（含注释与行列位）；高亮只需词法层，不必新增 grammar 包/highlights 查询/构建产物；**写不完的 SQL 也能着色**（语法树会失败，词法不会） | 词法失败（未闭合字符串）→ 返回空、退化为无高亮；函数名靠“标识符后跟 `(`”启发式；类型名用常见表兜底 |
| D13 | 多文档标签条**优先用 Dock 自带能力**（`Panel::{title, title_suffix, closable}`） | 避免重造标签条（V1 自绘 35px 标签条 + 溢出菜单 + 拖拽排序，约 300 行） | 关闭拦截钩子待查证（若不可行 → 退回自绘，见 §12 #1） |
| D14 | Analysis 的 `Session` **预留内核类型维度**，第一期只实现 SQL | SQL 的"内核"已有 90%（连接 + DuckDB 会话 + 临时表 + 取消/超时/上限） | 抽象若做错会牵动单元执行接口（故先只做 SQL 实现 + 接口固化） |
| D15 | 单元与输出**只存引用 + 预览**（DuckDB 临时表） | 与 M1「查询后分析」主线一致，避免内存与 `.rdsnote` 膨胀 | 需处理临时表生命周期与"输出已失效"的呈现 |
| D16 | 位置用**锚点/偏移**，键用**稳定 id** | V1 用路径派生 id + 行号语义 → 另存为失配、插入单元错位 | 需要一层位置映射（内核已提供锚点概念） |
| D17 | **不引入 Zed 编辑器代码**（GPL-3.0-or-later），只借鉴设计 | 许可证传染 + 包身份冲突（`gpui-pre` vs 上游 `gpui`）+ 依赖闭包 = 半个 IDE | 需自建编辑器服务层（本模块主体工作量所在） |
| D18 | 分析模式首期语言 = **SQL + Markdown** | Markdown 成本极低且是笔记必需；Python/Rust 需进程模型与运行时 | 用户在评审时可能要求提前 Python（见 §13 #5） |
| D19 | **render 零 I/O**：加载/执行/内省全走后台任务 + 轮询回填 | 项目既有纪律（M4 已验证的 `nav_jobs` 模式）；现状执行在 render 闭包里同步做文件 I/O | 需要队列与状态机（复刻 M4 成熟模式，成本可控） |
| D20 | 会话间数据交换用 **Arrow** | 项目已全链路 Arrow（`duckdb` + `arrow 58.4` 同源，`shared/src/arrow.rs` 在），天然是 SQL/DuckDB/Python 的公共语言 | 第一期只有 SQL 一个内核，接口先留签名 |
| D21 | **执行通道与执行族正交**：通道（源库 / 本地加速 / 联邦）是会话/连接级属性，用「执行位置」指示器表达，**不做成一排执行按钮** | 三者互斥、长期有效、决定能力边界；V1 把加速做成常驻执行按钮（且因硬编码从未显示），语义混乱 | 通道需持久化随文档绑定；通道能力差异需在 UI 可见（写/事务/新鲜度） |
| D22 | **结果集带通道徽标 + 血缘**，重查与分析**产生新结果集**（不就地替换） | V1 就地替换 `columns/rows`，用户丢失“原始查的是什么”；通道不明导致无法判断数据新鲜度 | 结果集数量增长（受上限 5 与淘汰约束）；需定义标签展示方式 |
| D23 | **结果集只读**；行内编辑与写回源库归 M4 表格能力 | V1 把表编辑塞进结果区，导致语义纠缠与脏状态双轨（`dirtyRows` / `dirtyCells`） | 用户若想在结果区改数据需走 SQL（DML）或去 M4 表格 |
| D24 | **结果集分段抓取**（已确认 2026-09-15）：数据源暴露「已抓取窗口 + 取下一段」，未知总数显示 `N+` | DBeaver 的大表体验基线；一次抓取 + 上限截断在亿行级表上不可用；且接口形状一旦定型难改 | 多一层窗口状态；导出需区分「仅已抓取」与「抓全量」两种语义 |

---

## 7. 现状盘点与必须先修的地基（权威）

### 7.1 V2 现状：编辑器本体基本不存在

> **本节是 2026-09-15 的开工前盘点**（保留历史证据）。其中与本模块相关的部分已逐条关闭：
> 编辑器视图 / 执行 / 结果 / 多文档 / 快捷键全部落地（见 §7 末「当前事实」与开发计划的任务表）；
> **B12（2026-09-16）已删掉旧 `EditorPanel` 的 SQL 框与内联执行闭包**，SQL 编辑器只剩 `crates/editor`。

| 层 | 现状 | 证据 |
| --- | --- | --- |
| 编辑器视图 | 一个 `Textarea`（无高亮/行号/补全），仅在连接 `use_duckdb_fed` 时出现 | `crates/workbench/src/panels/` L6094、L6809、L6779 |
| 执行 | 同步直连**全局分析库文件**（与连接无关），无取消/超时/事务/批量 | 同上 L6821-6825；`services/query_runner.rs` |
| 结果 | 定宽 `div` 表格，无虚拟化/排序/过滤/分页 | 同上 L6921-6950 |
| 历史 | 20 条 JSON 列表（点击回填） | `services/query_history.rs`（去重 + 上限 20） |
| 导出 | 仅 CSV，落 `global/results/` | `services/query_export.rs` |
| 脏状态 | `Shared::editor_dirty`（"有内容且 ≠ 上次执行"），仅服务项目切换拦截 | 同上 L6484-6496；`components/project_host.rs` |
| 快捷键 / Action | 无编辑器相关 Action（只有布局类 + 导航/草稿箱/连接对话框） | `crates/workbench/src/commands.rs`、`crates/app/src/main.rs` L80-110 |
| 多文档 | 无（中心区单个 Dock panel） | `crates/workbench/src/view.rs` L302 |

### 7.2 已迁移但零接线（本期直接复用，成本极低）

| 能力 | 落点 | 现状 |
| --- | --- | --- |
| SQL 解析/验证/转译/DDL 构建 | `engine::sql::SqlEngine`（`parser/builder/transpiler/formatter` + `services/sql_parser_service.rs`） | 无 UI 消费 |
| 查询执行（连接路由 / 取消 / 超时 / 事务 / 缓存 / 联邦） | `engine::services::sql_service.rs`（`MAX_QUERY_ROWS = 10000`，L44） | 仅 insight/persistence 内部用 |
| SQL 历史存储（过滤 / 统计 / 500 条族） | `engine::persistence::history_store.rs` | 被 `SqlService` 用（字段失真，见下） |
| SQL 模板库（6 条内置 + CRUD） | `engine::persistence::sql_template_store.rs` | 零调用 |
| 编辑器上下文（内容/光标/选区/布局） | `engine::persistence::workbench_context_store.rs` | 零调用（表已建） |
| 结果集服务（WHERE 重查 / DuckDB 分析 / 临时表 / 列洞察） | `workbench::services::result_service.rs`（`ResultService`） | 零调用（仅再导出） |
| 重查筛选 / DuckDB 分析执行 | `engine::services::execution_service.rs`（`re_execute_with_filter` / `execute_duckdb_analysis`） | 零/极少调用（门面未被 UI 消费） |
| 联邦查询（多源只读挂载 / 跨源查询） | `engine/src/duckdb/federation/`（`registry` / `session`；🟡 `legacy.rs` 待退役）+ `workbench::services::editor_exec.rs`（联邦档执行路径） | ✅ 已接（B13：编辑器「执行位置：联邦」能跑跨源；源清单浮层待做） |
| DuckDB Secret（凭据集中管理） | `connection/src/secret.rs`（`CREATE PERSISTENT SECRET` + `secret_directory`） | 已接（保存/连接时按 `use_duckdb_fed` 注册；**注意：扫描器不认 Secret**，挂载凭据走运行时连接串 + 引擎侧脱敏，见联邦架构 D11） |
| 连接级执行入口 | `workbench::services::connection_service.rs::execute_sql`（L1066-1088） | 零调用 |
| 补全元数据（实时内省） | `database::MetadataService`（`list_tables/list_columns/list_indexes/…`） | 零消费 |
| 结果集数据结构 | `engine::services::result_types.rs::ResultSet`（含 `temp_table`） | 待接 |

### 7.3 四个"假底座"（不修则功能不可用）

| # | 问题 | 证据 | 影响 | 修法 |
| --- | --- | --- | --- | --- |
| 1 | ~~**格式化输出是 Rust Debug 打印**~~ **已修（2026-09-15，P0.3；2026-09-18 加区间回填）** | `engine/src/sql/formatter.rs` 改用 sqlglot-rust 的 `generate_pretty`（**B10 起 `format_with_report` 先拿 `sql/split.rs` 的语句区间、逐条格式化后回填原位**——区间外的注释 / 空行 / 半句一个字节不动），**无新增依赖**；回归测试从“非空”升级为“不是 Debug 打印 / 结果可再次解析 / 多语句不丢句 / 解析失败原样返回 / 前导注释不丢 / 报告口径” | — | 残留：含行内 / 尾随注释的语句**不被格式化**（原样返回，生成器能力边界，见 §12 #3）；编辑器侧已如实报“N 条解析不了，原样保留” |
| 2 | ~~**语句切分朴素 `;` 切分**~~ **已修（2026-09-15，P0.4）** | 词法级状态机落在 `engine/src/sql/split.rs`（26 项表驱动测试）；`sql_parser_service::split_sql` 改为委托 | — | — |
| 3 | **事务状态是桩**：`get_transaction_status` 恒 `false`；`begin/commit/rollback` 无会话跟踪 | `engine/src/services/sql_service.rs` | 事务 UI 无从驱动。**P0.2 三组探针已实证（2026-09-15）**：顺序执行下四库亲和 + `ROLLBACK` 均真实生效；**并发下 MySQL/PG 会换物理连接**（临时表“消失”）；MySQL 的 `BEGIN` 已改走文本协议可用 | 状态机 + **per-session 独占连接**一并做（并发是常态，不能靠池的顺序巧合）；否则事务在“后台执行 + 用户操作”并发时会静默失效 |
| 4 | ~~**历史字段失真**~~ **已修（2026-09-15，P0.5）** | `SqlHistoryEntry` + `save_sql_history(_into)`：耗时/成功/失败原因/行数均真实，失败也留痕（4 项单测） | — | — |

### 7.4 顺带补齐

| 项 | 现状 | 处置 |
| --- | --- | --- |
| `affected_rows` | V2 `SqlExecuteResult` 无此字段（V1 有） | 补字段并在结果区/输出块展示（"影响 N 行"） |
| `statements_count` | `parse_sql` 恒返回 1 | 状态栏语句数改由 `split.rs` 结果给出，不再依赖该字段 |
| 系统目录预置 | `engine/src/cache/minicatalogs.rs` 为空占位 | 第一期不依赖它（补全走实时内省）；作为后续项保留 |

---

## 8. 降级与容错矩阵

| 触发条件 | 降级行为 | 用户可见性 | 恢复入口 |
| --- | --- | --- | --- |
| 未绑定连接 | 编辑可用；执行族禁用 | 工具 tip + 结果区空态文案 | 连接选择器 |
| 连接未运行 | 执行前自动建连（同 M4 口径） | 状态栏连接点转圈 | 失败给原因 + 重试 |
| 连接只读 + 写语句 | 确认后拒绝 / 直接拒绝（按策略强度） | `warning` 提示条 | 连接设置（M3） |
| 元数据不可用（无内省权限/超时） | 补全降级为关键字 + 函数 | 弹层底部一行说明 | 重试按钮 |
| 格式化失败 | 保留原文 | 状态栏提示（不静默） | — |
| 结果超行数上限 | 截断 + 标记 | 结果状态行明示 | 导出提示会导出截断集 |
| 大文件（>50MB / >200MB） | 关闭补全与折叠 / 不建编辑器 | 状态栏档位后缀 / 只读提示卡 | 只读打开 |
| 执行超时 | 中断并报超时 | 错误卡片 | 重试 / 放宽超时 |
| 会话失效（重启/连接断开） | 全部单元输出标 stale | 笔记头"N 个单元已过期" | "重跑过期单元" |
| 临时表被清理 | 输出显示"数据已失效" | 输出块提示 | 重跑单元 |

---

## 9. 性能与可观测

| 关注点 | 指标 / 手段 |
| --- | --- |
| 大脚本 | 高亮与补全按需（视口优先）；>50MB 关闭重能力；输入延迟目标 < 16ms/键 |
| 大结果集 | 虚拟滚动 + 分页（>1000 行默认分页）+ 只取预览；数据驻 DuckDB 临时表，不在 UI 内存 |
| 补全延迟 | 缓存命中 < 5ms；未命中先返回关键字/函数，元数据到达后刷新列表 |
| 执行可观测 | 每次执行记录 `elapsed_ms` / `stmt 类型` / 行数 / 是否截断 / 是否走缓存 → 落历史 + `tracing` 埋点 |
| UI 线程 | render 零 I/O 契约测试；后台任务队列长度与堆积可在日志观测（沿用 `nav_jobs` 的进度/取消原子量） |

---

## 10. 测试策略

| 层 | 目标 | 落点 |
| --- | --- | --- |
| 纯函数单测（最高价值） | 语句切分（各方言字面量/注释/`$$` 块/占位符/未闭合/行号，表驱动，✅ `engine/src/sql/split.rs`）· **词法高亮**（类别/引号包入/多字节边界/未闭合降级，✅ `engine/src/sql/highlight.rs`）· 格式化（非 Debug 打印/可再解析/多语句/失败原样返回，✅ `formatter.rs`）· 模式判定表（✅ `crates/editor/src/mode.rs`）· 能力表（✅ `model.rs`）· 执行目标解析（选区优先）· 错误位置解析（多家驱动格式）· 补全上下文判定与排序 | `crates/engine/src/sql/*.rs`、`crates/editor/src/{mode,model,execution,completion}.rs` |
| 服务层集成 | 打开→执行→结果入库→历史写入（**断言耗时/成功/行数真实**）· 结果上限淘汰 · 连接归属校验 · 模式切换矩阵（含确认分支）· 保存与脏状态 | `crates/editor/tests/`（临时目录注入模式，参考 `query_history.rs` 的 `*_at(dir)`） |
| 契约测试 | 视图层零裸色/零裸 `px(`（扩展现有 `crates/workbench/tests/ui_contract.rs` 扫描范围到 `crates/editor/src`）· 尺寸常量引用 | 现有 `ui_contract` 扩展 |
| 窗口（headless） | 三模式渲染不 panic · 执行按钮 → 结果区出现 · 模式切换确认流 · 关闭脏文档拦截 · **走生产入口**（参考 `dialog_host_layer.rs` 的教训） | `crates/editor/tests/`（`#[gpui_kit::test]`） |
| 第三方能力探针（离线·报告式） | 对「台账 ⚪ 候选」用真实 SQL 跑一遍并**打印**行为，确认后才转成断言；断言仅限与语义无关的不变量（可再解析 / 不 panic / 文档化边界）。✅ `crates/engine/tests/sqlglot_capabilities.rs`（作用域 · 血缘 · 类型标注 · 差异 · 下推 · 限定 · 本地计划 · 转译单条限制 · 格式化注释保真）· ✅ `crates/engine/tests/transaction_affinity.rs`（事务会话亲和，需真实端点） | `crates/engine/tests/` |
| 真机回归 | 4 类数据库（MySQL / PostgreSQL / SQLite / DuckDB）× 执行族 × 只读 × 大结果；明暗主题对照 | 手工清单（dev-plan §3） |

---

## 11. 实现位置映射（设计决策 → 代码）

| 设计决策 | 实现位置 |
| --- | --- |
| D1 能力表 / D2 模式属性 / D3 切换矩阵 | `crates/editor/src/{mode.rs, model.rs, service.rs}` |
| D4 只读两维度 | `model.rs`（`ReadOnly { editor, connection }`）+ `view/*` 表现 + `execution.rs` 校验 |
| D5 执行唯一入口 / D19 后台任务 | `crates/editor/src/execution.rs` + `engine::SqlService`（模式参考 `crates/workbench/src/services/nav_jobs.rs`） |
| D6 结果单权威 | `crates/editor/src/store.rs`（`ResultStore`）+ `view/widgets` 两个视图 |
| D21 执行通道（源库/加速/联邦）与门控 | `crates/editor/src/channel.rs` + `crates/workbench/src/services/{editor_exec.rs,editor_channels.rs}` + `engine/src/duckdb/{accel,federation/}` |
| D22 结果通道徽标与血缘 | `crates/editor/src/store.rs`（结果集元数据：`channel` / `lineage`） |
| D23 结果集只读 / 网格归属与提炼 | `crates/editor/src/view/widgets/grid/`（trait `GridDataSource` / `GridEditSink`）+ 架构 §3.6 |
| D24 分段抓取 | `crates/editor/src/store.rs`（结果集窗口状态）+ `view/widgets/grid/`（“取下一段”入口与 `N+` 展示） |
| Dock 标签能力（脏点 / 关闭语义） | `crates/editor/src/view/host.rs`（`Panel::{title_suffix, closable}`；关闭语义见 §13 #15）+ `crates/workbench/src/view.rs`（中央区装配） |
| 筛选下发 / DuckDB 分析 | `crates/editor/src/execution.rs` + `engine/src/services/execution_service.rs`（`re_execute_with_filter` / `execute_duckdb_analysis`） |
| D7 语句切分 | ~~`crates/editor/src/split.rs`~~ → **`crates/engine/src/sql/split.rs`**（✅ P0.4 已完成；`SqlEngine::split_statements` + `sql_parser_service::split_sql` 委托） |
| D8 格式化 | `engine/src/sql/formatter.rs`（✅ P0.3 已改用 `generate_pretty` + 往返解析回归；✅ B10 起 `format_with_report` **按语句区间原位回填**、区间外字节不动）+ 编辑器侧计划 `crates/editor/src/format.rs`（选段优先 / 光标映射 / `changes` 口径） |
| D8b 方言转译 | `engine/src/sql/transpiler.rs`（✅ B10：`transpile_with_report` **先切分再逐条转译**——整篇接口会静默丢语句）+ `engine/src/sql/script.rs`（格式化与转译共用的脚本骨架）+ `crates/editor/src/translate.rs`（目标表 / `targets_for` / 选区优先的 `plan`） |
| D8c 执行计划 | `engine/src/sql/explain.rs`（✅ B10：`explain_sql` 按方言生成前缀；SQL Server / Oracle 如实返回 `None`）+ `EditorHostPanel::explain_current`（**按通道取方言**：源库档源库的、加速 / 联邦档 DuckDB 的；结果落新结果集并贴「执行计划」标题） |
| D9 历史字段 | `engine/src/persistence/history_store.rs::save_sql_history` + `engine/src/services/sql_service.rs::execute` |
| D10 补全 | ✅ 切片一（2026-09-18）：`crates/editor/src/completion.rs`（上下文判定 + 候选挑排，纯函数）+ `crates/editor/src/view/completion.rs`（`CompletionProvider` 适配，**只给 `label`**）+ 宿主端口 `workbench/src/services/editor_completion.rs`（读 `database::cache::NavCache`，后台预载 + 内存读；按通道给限定名）；⬜ 余：`Ctrl+Space` · 模板片段 · 实时内省回稳 |
| D12 SQL 高亮 | ✅ `crates/engine/src/sql/highlight.rs`（tokenizer → 字节区间 + 类别）+ `crates/editor/src/view/`（按主题语法色板上色） |
| D13 多文档标签 | `crates/editor/src/view/host.rs`（优先 `dock` 的 `Panel::{title,title_suffix,closable}`） |
| D14/D15/D18 会话与单元 | `crates/editor/src/{session.rs, notebook.rs}` + `view/notebook_view.rs` |
| 持久化（光标/选区/模式/连接） | `engine::persistence::workbench_context_store`（扩展 `EditorContext`：加 mode / dialect / baseline 等） |
| `.rdsnote` 读写 | `crates/editor/src/persist.rs`（第二期接入项目版本链） |
| 宿主装配（中央区 / 对话框层 / 属性面板） | `crates/workbench/src/view.rs`（`init_workspace` 中央区改装配 editor 宿主）+ `components/project_host.rs`（桥不变） |
| Action 与快捷键 | `crates/editor/src/commands.rs` + `crates/app/src/main.rs`（`bind_keys`，key_context `editor` / `editor-sql` / `editor-notebook`） |
| 项目只读闸（强只读那一档） | `crates/editor/src/project.rs`（端口）+ `view/host.rs::project_write_check` + `crates/workbench/src/services/editor_project.rs`（宿主实现，✅ 2026-09-18） |
| 尺寸常量 | `crates/editor/src/ui.rs`（现状自持；`crates/workbench_shell/src/ui.rs` 已可被任何特性 crate 依赖，合并仍开放——见 §13 #6） |

---

## 12. 已知问题与后续项（权威清单）

| # | 级别 | 问题 | 影响 | 建议 |
| --- | --- | --- | --- | --- |
| 1 | ✅ | ~~**Dock 标签条的关闭拦截钩子未查证**~~（**已查证，2026-09-15**）：`Panel::closable(cx)` 是唯一闸门（静态许可，不能问用户），`DockArea` 收到 `TabGroupEvent::ClosePanel` 后直接 `remove_panel_id`，没有“关闭前询问”钩子；另见 #23（**面板自己发起关闭会重入**）。结论：D13 成立（用 Dock 标签条 + `title_suffix` 脏点），未保存确认走“拦在动作层 + 状态栏说明”，需要弹窗时再上自定义标签条 | 已不影响 D13 成立 | — |
| 2 | 🟡 | **事务会话亲和：顺序成立、并发不成立**（**已实证 2026-09-15，P0.2 / P0.2b / P0.2c**）：顺序执行下四库（MySQL/PG/SQLite/DuckDB）「临时表在事务内可见 + `ROLLBACK` 生效」全部成立；**并发执行下 MySQL/PG 的池会另开物理连接**（并发两侧之一报 `1146 表不存在` / `relation does not exist`），SQLite/DuckDB 为单句柄语义不受影响 | 事务 / 临时表**不能依赖池的巧合**：并发（后台执行 + 用户操作）会让 `BEGIN` 与后续语句落在不同物理连接 | 1b：**per-session 独占连接**（事务/会话期间 pin 住物理连接，或 `SqlService` 持有 `Box<dyn Transaction>`）；MySQL 的 `begin/commit/rollback` 已改走**文本协议**（`raw_sql`，已实证通过），不再报 1295 |
| 3 | ✅→🟡 | ~~**格式化实现待定**~~（**已定，2026-09-15（P0.3）**：用 sqlglot-rust 自带 generator，不引新依赖；**2026-09-18 B10 起**：区间原位回填 + 编辑器侧回执）| 残留（**已实测，2026-09-15**）：**注释不会丢**——行内 / 尾随注记会让 sqlglot 解析失败，而解析失败即原样返回；代价是**含行内 / 尾随注释的语句不会被格式化**（用户看到原样文本）；另：非 MySQL 目标的 `#` 注记会被改写成 `--` | ✅ **已落实（2026-09-18）**：编辑器状态栏如实报“N 条解析不了（可能是还没写完），已原样保留”（`FormatReport.kept_verbatim` → `editor/src/format.rs` 的 `changes()` 分支），不再有“以为格式化失败”的误解；将来若真需要格式化这类语句，再评估自研缩进器 |
| 4 | 🟡 | 现有 `EditorPanel` 的连接详情卡 / 导航树 / 属性面板宿主与编辑器耦在同一面板 | 收编时容易把 M3/M4 的职责带进 editor crate | 按 §3.3 表格逐项迁出，先迁"编辑器"部分，其余留 workbench |
| 5 | 🟡 | 分析模式的语言集合（是否提前 Python） | 影响 Session 抽象与进程基建 | 用户拍板（§13 #5）；默认按 D18 只做 SQL + Markdown |
| 6 | 🟡 | 尺寸常量落点：`editor` crate 自带 `ui.rs` 还是复用 workbench 的 | 影响依赖方向（editor 不应依赖 workbench） | editor 自带 `ui.rs`；跨模块共用常量上提到 `shared` 或由 gpui-kit 主题承担 |
| 7 | ⚪ | 参数绑定（`:name`）能力 | V1 有原语但无闭环 | 后续做，且优先走驱动层 prepared statement（`Database::query_with_params`） |
| 8 | ⚪ | 拖拽表到编辑器插入限定名 | M4 原型的已声明能力 | 后续（M4 `on_drag` + editor 插入接口） |
| 9 | ⚪ | `minicatalogs`（离线系统目录） | 无网络/无权限时补全更差 | 后续；先保证在线内省路径 |
| 10 | ⚪ | notebook 导出 `.ipynb` 互操作 | 生态互通 | 第二期（作为导出格式，不是主格式） |
| 11 | ⚪ | 变量浏览器（SQL 会话的临时表/视图列表） | 分析模式可用性 | 1c 后追加 |
| 12 | ⚪ | 模块文档缺口：使用手册（`editor-user-guide.md`） | 与其它模块的六件套不齐 | 实现完成后补（本轮不写，因“怎么用”需以真实实现为准） |
| 13 | ✅ | **通道可用性判定无来源**：`use_duckdb_fed` 曾只是“是否注册 Secret”的开关，而“Secret 是否注册成功 / 能否 ATTACH”没有可查询状态 | 通道门控只能靠猜，UI 给不出准确的“不可用原因” | **已关闭（2026-09-18，B13 切片二）**：门控真值来自宿主 `ChannelsPort`（连接开关 → 驱动类型 → 扩展可用性 `accel::extension_state`）+ 每个连接一条缓存的加速会话；扩展**没试过不拦**（让用户能选，执行时在工作线程真装一次），**试过且失败就把原话记下来**在菜单行尾显示（重试成功即清）。写保护落在引擎侧（`ATTACH … READ_ONLY`），不再依赖上层自觉 |
| 14 | ✅ | **加速通道新鲜度**（ATTACH 快照 vs 实时）无表达、无重建入口 | 用户可能拿旧快照做决策 | **已关闭（2026-09-18，B13 切片二/三）**：实测**数据是实时的**（源库插一行，同一 DuckDB 会话立刻看得到），但在 `ATTACH` 时定型的**表清单**是静态的——所以界面写「源库只读」而**不是“快照”**；重建入口就是「执行位置 ▾」菜单里的「重新挂载源库（刷新表清单）」（旁路线程 + 回执，不占执行位）。⬜ 另：加速会话的临时表与分析会话（1c）暂不共享 |
| 15 | 🟡 | **重查包裹语义**：`SELECT * FROM (<原SQL>) WHERE …` 遇原查询带 `ORDER BY` / `LIMIT` 时语义会变 | 重查结果与原查询不一致 | 定规则：保留外层 ORDER BY；有 LIMIT 时提示“重查将去掉 LIMIT” |
| 16 | ⚪ | 结果**血缘只到 UI 摘要级**，未落库 | 重启后无法回看“这个结果怎么来的” | 1b 时把 lineage 写入结果集元数据 |
| 17 | 🟡 | **驱动层不返回真实 `affected_rows`**（全部驱动目录无该字段写入；`from_batches` 已不再把 `total_rows` 当影响行数，现为 `None`） | DML/DDL 的“影响 N 行”无法展示，历史里写语句的 `rows_affected` 也为空 | 1b：native 驱动（mysql/postgres/sqlite/duckdb）在写路径填充 `QueryResult.affected_rows` 并补测试 |
| 18 | ⚪ | **Dock 无“关闭前否决”钩子**（已静态核实 2026-09-15）：`DockArea` 订阅 `TabGroupEvent::ClosePanel` 后直接 `remove_panel_id`；`Panel::closable(cx)` 是唯一闸门（静态许可，不能问用户）；`remove_panel` / `with_renderer` 均为 `pub` | 决定了“标签 ✕ 能否弹未保存确认”——需要自绘标签条才能做到 | 见 §13 #15：默认走“草稿兜底”（关闭即落草稿），需要弹窗时再上自定义标签条。**2026-09-16 收口（B16）**：三条关闭路径统一为两条——① `Ctrl+W` → `request_close_document`（**三态确认**）；② Dock 的关闭入口（**组件库没有每标签 ✕**，只有标签栏 ⋯ 菜单里的 `Dock.Close`）——它按 `TabGroup::is_closable` → **当前标签**的 `closable(cx)` 决定要不要摆出来，而编辑器面板的判据是 `!is_dirty()`：**脏文档那里根本没有关闭项**，也就没有“点了没反应”。自绘标签条（能在 ✕ 上弹确认）仍留在 1b 之后 |
| 19 | ✅ | **`transpile` 对脚本是「静默截断」**（**已实测，2026-09-15**；**2026-09-18 B10 已接线并规避**）：`transpile("SELECT 1; SELECT 2;", MySQL, PG)` 返回 **`Ok("SELECT 1")`**——第二条语句**无声消失**；生产路径 `SqlEngine::transpile` 同样如此，而包装它的 `sql_parser_service::transpile_sql` 还会报 `success: true` | “方言转移器”若直连生产路径，用户点一下就会**丢掉后续语句且无任何提示**（数据丢失级） | ✅ **已按此做**：`SqlEngine::transpile_report` 走 `sql/script.rs`（切分 → 逐条 `transpile_with_comments` → 原位回填），并有**“脚本一条不丢”**的回归 + **“单条接口确实会截断”**的对照测试（后者一变红就说明 sqlglot 升级后此条可重评）；旧 `transpile` 保留但已标注“只吃单条”。缓于“diff 预览”：当前是**可撤销的就地改写**（`replace_all` 进撤销栈） |
| 20 | ✅ | ~~**高亮区间偏移错误**~~（**已修 2026-09-15，读源码时发现**）：原实现按 `token.value` 回查原文，但 `read_string` / `read_quoted_identifier` 会解码转义（`'it''s'` → `it's`）→ 区间落空；字符串的 `quote_char` 恒为 `\0`（只对带引号标识符设置）→ 引号 / 注释标记取不到 | 已改为「**字符偏移 → 字节偏移**换算 + 取原文区间」（`highlight.rs::byte_offsets` / `raw_range`）：`Token::position` 是字符下标（`tokens/tokenizer.rs:69,83,137`），中文 SQL 下直接用会切坏 `&str` | 行为已固定：区间恒为 token 的**原文**（含引号、转义、`--` / `/* */` 标记），与 `value` 解码无关；回归 11 → 13 项（新增转义字符串 / 中文 SQL） |
| 21 | ✅ | **`QueryResult` 字段约定**（**实测发现并已修 2026-09-15**）：各驱动**只填 Arrow `batches`**，`rows` / `total_rows` / `column_types` 字段是默认空值（`postgres_native.rs::build_query_result` 等直接构造结构体、绕过 `from_batches`）；仅 `truncate()` 之后才由 `recompute_computed_fields` 回填 | 读 `rows` / `total_rows` **字段**的代码会“大结果有数、小结果无数”——行为随行数变化；历史行数（`sql_service.rs` 里我写的那行）就踩了此坑 | 已修：历史改用 `total_rows()`（由 batches 求和）。**1b 网格一律走 `batches` / `to_rows()`**，不新增读 `rows` 字段的代码（M7 亦踩过同一坑，见 `mock_generator.rs:290` 注释） |
| 22 | ✅ | **驱动结果保真度两处缺陷**（**实测发现并已修 2026-09-15**）：① `arrow_value_at` 只认 5 种 Arrow 数组，其余（PG 的 `int4` → Int32、MySQL 无符号 → UInt64、Float32、Decimal、Date…）落到 `format!("{:?}", array)`——**把整列 Debug 打印进每个单元格**（既显示垃圾，又是 O(n²)）；② MySQL 列类型探测 `bool` 优先，而 sqlx 能把 1/0 解成 `bool` → `COUNT(*)` 显示成 `true` | 结果网格与任何读值的功能都会显示错值——属“看着有、实际是垃圾” | 已修：值映射补 Int8/16/32/64 · UInt8/16/32/64 · Float32/64，兜底改 Arrow **单值**格式化（+3 项回归）；MySQL 数值族按**声明类型**定排行、无符号回退 `u64`（探针已实证计数可读）。**剩余**：驱动仍不填 `column_types`（网格列类型显示待办） |
| 23 | 🔴 | **面板不能从自己的 `update` 里让 Dock 移除自己**（**实测踩到 2026-09-15**）：`DockArea` 移除面板要读面板本体（可见性 / 可关闭性 / `on_removed` 回调），而动作处理器（`on_action`）本身就在面板的 `update` 中 → GPUI 直接 panic `cannot read … while it is already being updated`。更阴的是：`Context::listener` 的实现是 `view.update(…).ok()`，在真实按键路径上这个错误表现为**按键毫无反应**（静默），只有窗口测试里才能看到。`cx.defer_in` 也救不了：它的闭包第一参数就是面板自己，仍在 `update` 中 | 任何“面板自注销 / 自关闭”的设计都会踩（不只 `Ctrl+W`） | 已定：**关闭由宿主发起**——`editor::view::host::close_document_in_dock(&area, panel, window, cx)` 在宿主（workbench `WorkbenchView::close_active_editor`）里调，面板只回答“能不能关”（脏则拦下并写状态栏原因）。今后凡会读到面板本体的容器操作，都从宿主（或 `Window::defer` 而不经面板 `update`）发起 |
| 24 | ✅ | **内核已绑定的键，外层 context 收不到**（**已实测并修正结论，2026-09-15**）：内核在 `Input` context 里绑了 `ctrl-f` → `input::Search`、`ctrl-h` → `input::Replace`，**而且注册了 listener**（`gpui-base/src/input/base/state.rs:4195` 的 `on_action_search` / `on_action_replace`，`searchable` 为真时打开查找 / 替换会话）；按键派发一旦找到 listener 就 `propagate_event = false`，`dispatch_key` **不再尝试后续 binding** → 外层（面板 `editor` context）的同名绑定**收不到**。查找 / 替换的界面由组件库的 `SearchPanel`（`gpui-component/src/input/search.rs`，`CONTEXT = "SearchPanel"`）渲染为浮层 | 本条早先的结论是“**内核没有 handler**，只 grep 了会话字段 `search_session`，没看 listener 注册”----**错的**，据此曾自建一套查找栏 + 匹配器，属 **重复劳动（已撤）** | 已定：**查找 / 替换一律用内核 + 组件库能力，应用层零自建、不绑键**；判断“内核有没有这个能力”要读 `input/base/state.rs` 的 `on_action` 注册表，而不是只看某个字段的可见性。已用窗口测试钉住：`Ctrl+F` / `Ctrl+H` 之后焦点被内核交给查找框（`the_kernel_find_panel_takes_over_on_ctrl_f` / `ctrl_h_opens_the_kernel_replace_panel`） |
| 25 | 🟡 | **面板不 `track_focus` 就收不到快捷键**（**实测踩到 2026-09-15**）：`key_context` 只负责“能不能匹配绑定”，action 还要能沿 **dispatch path** 找到 listener；dispatch path 由**已渲染元素树的焦点节点向上**构成。面板根元素没 `track_focus(&self.focus_handle)` 时，焦点不在那棵子树里，快捷键就落不到 `on_action`（表现为静默无反应） | 每个自己处理键盘动作的面板都要记得写 | 已定：面板根元素 `.key_context("editor").track_focus(&self.focus_handle)`；窗口测试用 `window.focus(&handle, cx)` + `simulate_keystrokes` 验证（这类“静默”错误只有测试能抳住） |
| 26 | 🟡 | **执行通道的连接绑定**（**1a 划定范围；B1 切片一已部分关闭，2026-09-16**）：1a 时 `EngineQueryRunner` 调 `SqlService::execute(None, …)` → `DEFAULT_CONN_KEY = "active"`（**连接管理器里的活动连接**），界面上看不出“这条 SQL 会发到哪” | 多连接场景下可能发错库；与原型 §5.7「执行位置（源库/加速/联邦）」相差一档 | **已做**：绑定成为**文档属性**（`Document.connection`）+ 工具栏「连接 ▾」选择器（选中先自动建连）+ 状态栏最左连接段（`●P·orders`）+ 执行通道带连接（`QueryRunner::run(conn, sql)`，未绑定才回退到活动连接）——“发到哪”现在在界面上看得见、可改。**剩余**：绑定随会话持久化（重启后不丢；需 engine `editor_contexts` 加列）· 只读锁 ⓘ（连接侧写策略来源）· 「执行位置」三通道（**B13 切片一已完成 2026-09-17**：三档状态是一等文档属性 + 门控置灰给原因 + 随会话持久化 + 切通道旧结果标灰与提示；**切片二加速档真执行 ✅ 2026-09-18 · 切片三后半联邦档真执行 ✅ 2026-09-18**（源清单组装 / 跨源查询 / 历史带参与源；剩源清单浮层）） |
| 27 | ✅ | **执行链路真机四库实测通过**（**2026-09-15，A14**）：`editor_exec_real.rs` 走**生产路径**（`editor_exec::attach` → `EditorShared` → 工作线程 → `SqlService` → 驱动 → `batches`/`to_rows()`），四库各一次 `select 1 as n`：mysql 43ms · postgres 173ms · sqlite 1ms · duckdb 6ms，均 1 行 × 1 列、列名 `n` | 验证了执行端口抽象可用，且取数口径（不读 `rows` 字段）在四个驱动上一致 | 保持：新增驱动时把它的名字加进 `TARGETS` 表（环境变量未设自动跳过） |
| 28 | 🔴 | **同步存储 API 不能在 tokio 运行时上下文里调用**（**实测踩到 2026-09-15，A12**）：`GlobalSqlitePool::acquire_sync` 早先用 `Handle::current()`，于是——无 runtime 的线程（GPUI 主线程、普通 `#[test]`）**直接 panic**；`#[tokio::test]` 里又因嵌套 `block_on` panic（“Cannot start a runtime from within a runtime”）。而 `WorkbenchContextStore` 的整个 API 都是同步的 | 接会话持久化时“第一次真调它”就撞上；这种错误看上去像是测试环境问题，实际是接口约定缺失 | 已修：在 tokio 上下文里**返回可读错误**（而不是 panic），否则自建一个短命 current-thread runtime 驱动（`Semaphore`/`Mutex` 不绑 reactor，新 runtime 只当驱动器）。**今后接存储层要记住**：同步 API 的调用点是 GPUI 主线程（无 runtime）或工作线程，不能是 async 函数体 |
| 29 | 🟡 | **对话框要弹出来，窗口根视图必须是 `gpui_component::Root`**（**实测踩到 2026-09-16，A9**）：`Window::open_dialog` 内部走 `Root::update`，找不到 Root 就 `expect("BUG: window first layer should be a gpui_component::Root")` **直接 panic**；且对话框层要由宿主 render 调 `Root::render_dialog_layer` 才会进元素树（只“打开”不渲染 = 看不见）。两个陷阱都不报错：前者是 panic，后者表现为“点了没反应”。**附带一条测试口径**：headless 下 `debug_bounds(选择器)` 的坐标与鼠标命中测试**对不上**（同一个用例单跑能点中、全套跑必不中；已排除并行与动画两个假设），所以对话框按钮的“真点击”**不做**，改成“断言层与按钮真渲染（`debug_bounds`）+ 直接驱动落地入口（`resolve_close_choice` / `confirm_mode_switch`）” | 写窗口级对话框测试与新增对话框时必踩 | 已定：对话框均挂 `Root::render_dialog_layer`（workbench 根视图已有）· 测试夹具用 `Root::new(宿主, window, cx)` 包起来（见 `editor/src/view/tests.rs` 的 `dialog_harness`）· 落地逻辑与弹窗分开（弹窗只收集选择） |
| 30 | 🟡 | **`rfd` 的同步对话框会阻塞 UI 线程**（**1a 划定范围，2026-09-16，A9**）：`rfd::FileDialog::{pick_file, save_file}` 是阻塞调用，弹系统模态框期间 GPUI 事件循环停摆（窗口可能被系统标为无响应）；换 `AsyncFileDialog` 需要把调用点改成 `cx.spawn` 并处理“回来时文档可能已被关掉”的竞态 | 模态对话框的固有代价，不是这里引入的；但它是**卡顿感的真正来源**（比编辑器自身的任何渲染问题都明显） | 1a 接受：调用点全在事件路径且在主线程，两个函数形状不变（`services/editor_files.rs`）。将来若改为异步，只改这两个函数内部 + 加一句“回来时重新查文档是否还在” |

---

## 13. 待确认决策清单（阻塞开发的项）

| # | 决策 | 备选 | 建议 | 影响面 |
| --- | --- | --- | --- | --- |
| 1 | 多文档标签实现 | A 自绘标签条 / B Dock 自带（`title_suffix` 承载脏点） | **B**（先验证 §12 #1） | 1a 工作量 ±300 行 |
| 2 | SQL → 分析 的转换粒度 | 整篇单单元 / 按语句拆分 / 弹窗选择 | **整篇单单元** + 二次"按语句拆分" | 1c 数据迁移语义 |
| 3 | 批量执行语义 | 逐条独立 / 单事务 | **逐条独立**（DBeaver 同） | 1b 执行语义 |
| 4 | 结果集宿主 | 停靠面板 / 内联 / 两者 | SQL 模式默认**停靠面板**；分析模式内联；共用同一数据 | 1b UI 结构 |
| 5 | 分析模式首期语言 | SQL 只有 / SQL + Markdown / 加 Python | **SQL + Markdown**（Python 需 sidecar 与运行时，第二期） | 1c 范围 |
| 6 | 编辑器独立 crate | 独立 `crates/editor` / 先留在 workbench | **独立**（§3.2 判定四条成立） | Phase 0 结构 |
| 7 | 大文件分块加载 | 纳入 1a / 推到 1b 后 | 推到 1b 后 | 1a 范围 |
| 8 | 面包屑 | 保留（V1 有） / 去掉 | **去掉**，路径进状态栏 | 1a UI |
| 9 | 🟡→✅ | 只读拦截强度 | 写语句一律拒绝 / 可确认放行 | **默认确认放行，强只读模式拒绝**（两档可在连接策略里选） | 1b 语义 |
| 9b | ✅ | **落地的那一档（2026-09-18）**：**项目锁 = 强只读** → `INSERT` / `UPDATE` / `DELETE` / DDL 在提交前就被拒（`ProjectPort` + `view/host.rs::project_write_check`，判据与通道闸共用 `channel::writes_source_object`），读语句照跑；“提醒后放行”那档随连接策略（B1 余项）一起做 | `editor/src/project.rs` + `workbench/src/services/editor_project.rs` | 单测 346 / 96（✅） |
| 10 | 崩溃恢复默认行为 | 自动恢复 / 询问后恢复 | **询问后恢复**（V1 有横幅的传统） | 1b 收尾 |
| 11 | 加速 / 联邦通道遇写语句 | 直接拒绝 / 允许但仅落本地副本 | **直接拒绝源库写**（INSERT / UPDATE / DELETE / 作用源表的 DDL）+ 提示切回源库；**允许本地临时对象**（`CREATE TEMP TABLE` / 临时视图 = 会话变量，不落源库） | 1b 语义、结果徽标与历史标记 |
| 12 | 执行通道默认值 | 总是源库 / 记忆上次 | **记忆上次**（随文档绑定持久化） | 1b 持久化字段 |
| 13 | DuckDB 分析结果落点 | 新结果集 / 覆盖原结果 / 直接作为分析单元 | 1b：**新结果集**（血缘）；1c：可一键沉淀为分析单元 | 1b / 1c 边界 |
| 14 | 分段抓取的段大小与上限 | 固定 1000 行/段 / 可配置 / 继承 `MAX_QUERY_ROWS` | ✅ **已确认**：固定 1000 行/段起步；总上限仍受 `MAX_QUERY_ROWS` 约束（1b 实测后调） | 1b 数据源与结果区 |
| 15 | 标签关闭语义 | ① 草稿兜底（关闭即落草稿 + 最近关闭恢复）/ ② 自定义标签条后弹三态确认 | **①**（关闭入口单一、无绕过；② 作为体验升级后置，API 已确认可行） | 1a 的标签条与 `persist` |
