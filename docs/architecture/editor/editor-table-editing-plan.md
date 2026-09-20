# SQL 编辑器模块 · 表数据编辑（编辑回写）立项

> 状态：**立项，未开工**（2026-09-21）。本文是 M4 表格能力的**边界与验收表**，不是排期承诺。
> 关联：`editor-architecture.md` §3.6（网格归属与提炼时机）/ §4（状态地图）/ §6 D23 / §11 / §12；`../driver-capability-matrix.md` §3.3（`table_editor` = `Stage::NotBuilt`）、§7 #13；`../database/database-navigator-prototype-design.md` §6.2（今天的「查看数据」是什么）。
> 本文即 `../references/README.md` §4 **R6** 的交付物（"把 dbui 的交互清单落成我们的验收表；'只读连接服务端强制'纳入设计"）。
> 许可口径（`../references/README.md` §0）：**dbui = MIT** → 抄语义（§2 的验收表来自它）；**fluxDB = GPL-3.0** → 只写思路、独立实现（§3.2）。
> 校验口径：标「已核实」的条目都回原文 / 回代码核过（依据列给到文件与章节）；标「推断」的是本文的判断，没有上游依据。

---

## 1. 定位与边界

**一句话**：这是 **M4 的表数据能力**（浏览 + 编辑 + 写回源库），不是往结果区塞的第二个功能；与 `crates/editor` 共享的**只有网格渲染层**。

"表格"在路线图里是**三件事**（架构 §3.6 已划清），本文只做第 ②：

| | ① 结果集网格 | ② 表数据浏览 / 编辑（本文） | ③ 网格渲染层 |
| --- | --- | --- | --- |
| 数据来源 | 一次执行的产物（`ResultSet`，驻 DuckDB 临时表） | 源库某表的分页抓取 | 由数据源 trait 提供 |
| 生命周期 | 结果集（会话内 / 可持久化引用） | 面板 / 标签（+ 事务） | 无状态 |
| 可写性 | **只读**（D23） | 可编辑并写回源库（行身份 + 事务） | 不适用 |
| 归属 | `crates/editor` | **`crates/database`（M4）** | 今天在 `crates/editor/src/view/results/grid.rs` |

三条硬边界（**已核实**）：

1. **不共享状态，只共享渲染**。架构 §3.6 的"永不合并"清单写明：数据源与分页策略 · 编辑缓冲与提交 · row identity / 主键解析 · 事务与冲突 · 权限与只读策略 —— 这五样**永不合并**。结果集的状态在 `ResultStore`（架构 §4 状态地图：每文档一份**结果集列表 + 选中项**），表编辑的草稿 / 行身份 / 提交态属于 M4 面板自己；两侧共享的只有"怎么画一个单元格"。
2. **结果区不变**。D23 已定：结果集只读，行内编辑与写回源库归 M4 表格能力；理由写在同一行里（V1 把表编辑塞进结果区 → 语义纠缠 + `dirtyRows` / `dirtyCells` 双轨）。
3. **能力位不许提前声明**。`table_editor` 在能力字典里是 `Stage::NotBuilt`，六个驱动已撤掉声明，并有单测盯住「未实现的键不许被声明」（`capability::tests::not_built_keys_are_claimed_by_nobody`）；功能落地才在 `descriptors.rs::capability_keys()` 里加回。依据 `../driver-capability-matrix.md` §3.3 / §7 #13。

**为什么本文放 `editor/` 而不是 `database/`**：默认放这里，因为它与结果网格共享渲染层（§3.6 的 ③），而验收表里的分页 / 行身份 / 只读三条都要在那一层对齐；M4 目录只留一句指针即可。**若产品将来把"表数据编辑"定为一等公民**（不是 SQL 的附属展示），按 §3.6 的例外条款应改立独立 `crates/dataset`（它此时确有独立状态），本文连同渲染层一起迁过去 —— 判据只有一条：**附属展示还是独立能力**（今天的口径是**附属**）。

**今天的现状（已核实）**：M4 的「查看数据」只是**注入一条 `SELECT * … LIMIT 200` 到编辑器**（`../database/database-navigator-prototype-design.md` §6.2、`database-navigator-showcase.md` §11），仓里没有表数据视图；也没有任何编辑回写（`../references/README.md` §5 逐模块对比：「结果网格（编辑回写）→ 我们等于没开」）。

---

## 2. 验收表（dbui 的语义 → 我们的验收项）

dbui 仓在项目外，目录名 `dbui-ref`（路径口径见 `../references/README.md` §0）。全部条目**已核实**：行文核到它的 `ARCHITECTURE.md`，语义核到对应源码文件与用例名。

| # | 我们的验收项 | dbui 的可验收语义（要点） | 依据文件 |
| --- | --- | --- | --- |
| A1 | 就地编辑**先暂存**，提交前不落库 | 编辑进 `RowDraft`，界面用变更气泡展示待提交项 | `dbui-ref/ARCHITECTURE.md` §Decisions worth knowing · `crates/dbui-ui/src/components/change_bubble.rs` |
| A2 | **一个事务提交整批**（编辑 + 删除） | 合成一个 `RowBatch`，`apply_changes` 在**一处** `begin()` … `commit()` 里跑完：删除失败会**连编辑一起回滚**，不留半写状态 | `crates/dbui-app/src/commands.rs::apply_changes`（文档注释原文）· `crates/dbui-driver/src/{mysql,postgres,sqlite}/mod.rs::apply_changes` |
| A3 | **整批丢弃**是一个动作（另有"有未提交改动就拦一下"的守卫） | `Discard Changes`（`⌘Z` 同路）清空整批；关闭前有 "Discard N staged changes?" 守卫 | `crates/dbui-ui/src/components/{palette.rs,close_guard.rs}` · `crates/dbui-ui/src/e2e.rs::cmd_z_discards_the_staged_batch` |
| A4 | 多行批量编辑的 **`MIXED`** 语义：留 `MIXED` 的字段**谁都不写** | 各行取值不一致的列显示 `MIXED`，它与 `NULL` / `DEFAULT` 一样是**写入记号**："框里写的就是要写进去的"，而仍读作 `MIXED` 的列**不进 `SET`** | `dbui-ref/ARCHITECTURE.md` §Decisions（"…a field still reading `MIXED` is written to nobody"）· `crates/dbui-ui/src/components/grid.rs` |
| A5 | 单行编辑与批量编辑走**同一条路** | `RowDraft` 持一串行下标，多选只是"更多下标"；暂存**整体重算而不是合并**（合并会双计，且会让 `MIXED` 列在"各行原本 staged 不同值"时被读成"丢弃已有值"） | 同上 §Decisions（"Staging is recomputed rather than merged"） |
| A6 | 整批删除**需要主键**；无主键**拒绝并说原因** | `delete_sql` 的 `pk` 为空即 `Err("table has no primary key")`；理由：UI 说不准的行就不该删（按非键列匹配会带上用户没选的行） | `crates/dbui-driver/src/sql_build.rs::delete_sql` · 用例 `sql_build.rs::tests::delete_needs_a_primary_key` · `crates/dbui-ui/src/e2e.rs::committing_a_table_with_no_primary_key_says_why` |
| A7 | 更新 / 删除**一律按主键定位**，不做"按值匹配" | `update_sql` 与 `delete_sql` 用**同一个 `WHERE` 形状**；`UPDATE` 的 `pk` 为空同样 `Err` | `crates/dbui-driver/src/sql_build.rs::{update_sql,delete_sql}`（函数文档注释） |
| A8 | 主键取值含 `NULL` 时写 `IS NULL`（不是 `= NULL`） | `value.is_null()` → `col IS NULL`；有专门用例点名「`= NULL` 匹配不到任何行，于是静默删 0 行却报成功」这个坑 | `crates/dbui-driver/src/sql_build.rs`（`update_sql` / `delete_sql` 的 NULL 分支）· 用例 `sql_build.rs::tests::a_null_key_part_becomes_is_null` |
| A9 | 外键跟随：**只跟单列外键，复合键不做** | 内省 SQL 自己把复合键滤掉（PG：`array_length(conkey, 1) = 1`；SQLite：`HAVING count(*) = 1`）；理由"一个单元格装不下整个键"，跳过去只会落在共享该部分的别的行上 | `crates/dbui-domain/src/catalog.rs::ForeignKey`（注释）· `crates/dbui-driver/src/postgres/catalog.rs` 与 `sqlite/catalog.rs` 的 `FOREIGN_KEYS` |
| A10 | **只读连接由服务端强制**（不是界面自觉）；三引擎各自的写法都要有 | MySQL `SET SESSION transaction_read_only = 1` · PostgreSQL `SET default_transaction_read_only = on` · SQLite 用**只读开库**（`SqliteConnectOptions::read_only`）。"read-only enforcement" 是**每引擎都断言**的那组属性之一 | `crates/dbui-driver/src/mysql/mod.rs:58` · `postgres/mod.rs:60` · `sqlite/mod.rs:43` · 用例 `tests/live.rs::a_read_only_connection_is_refused_by_the_server`、`tests/sqlite.rs::a_read_only_connection_refuses_writes_at_the_engine` · `ARCHITECTURE.md` §The layers |
| A11 | 分页**必须有序**：`ORDER BY` = **用户排序 + 主键**（主键跟在后面，不替换用户排序） | "对无序读取做 `LIMIT`/`OFFSET` 不是分页"——同一行能在两页都出现、另一行永远不出现；用户排序**不构成全序**（重复值上页边界正是丢行处）；列要**先读**，因为主键就是要排序的东西 | `crates/dbui-domain/src/query.rs::{order_for,SortKey}`（注释）· `crates/dbui-app/src/commands.rs::open_table` |
| A12 | "还有没有下一段"用 **`limit + 1` 探测**（多取一行再丢掉），不为它多跑一次 `COUNT(*)` | `Page::probe_limit() = limit + 1`；取回的那一行就是答案 | `crates/dbui-domain/src/query.rs::Page::probe_limit` |
| A13 | 行数用**真 `COUNT(*)`**（与探测分开），拿不到就如实留空（不编 0） | `count_sql` 生成 `SELECT count(*) FROM 表{WHERE}`，在 `open_table` 里单独一问：`row_count(..).await.ok()` | `crates/dbui-driver/src/sql_build.rs::count_sql` · `crates/dbui-app/src/commands.rs::open_table` |
| A14 | 无键表 / 视图**如实说"这一页没有定义顺序"**，不假装分页稳定 | `TableContents::is_ordered()`：有用户排序或有主键才算有序；否则不限序且界面明说 | `crates/dbui-app/src/commands.rs::TableContents::is_ordered` |
| A15 | 表名 / 列名**不能参数化**，一律引号转义，并有**敌意标识符**用例 | `TableRef::quoted` 双写引号转义；"SQL 参数不能当标识符，所以生成的语句必须把表名拼进去" | `ARCHITECTURE.md` §Decisions（"Identifiers are quoted, never bound"）· 用例 `crates/dbui-driver/src/sql_build.rs::tests::a_hostile_column_name_cannot_escape_the_order` |
| A16 | 精确数值**不走浮点**（`NUMERIC` / `DECIMAL` 保持字符串） | 那类列通常是钱，往屏幕上搬时四舍五入 = "看着像数据 bug 的显示 bug"；SQLite 是例外（它没有精确数值类型，见它的 Known limits） | `ARCHITECTURE.md` §Decisions（"Exact numerics stay strings"）· §Known limits |
| A17 | 认不出的类型**降级成"不支持（带类型名）"**，不拖垮整份结果 | 一列怪类型不该让其余四十列看不见 | `ARCHITECTURE.md` §Decisions（"Values are widened, not passed through"） |
| A18 | 引擎做不到的写法**明确拒绝**，不交给解析器报错 | SQLite 没有"逐列恢复默认值"的语法 → `Value::Default` 在 SQLite 上直接 `Err("SQLite cannot reset a column to its default")`：把原因摆在用户面前，而不是让解析器回一句 `near "DEFAULT": syntax error` | `crates/dbui-driver/src/sql_build.rs::update_sql` |
| A19 | 主键值按**它自己的类型**绑定，不"一律当文本" | 真机抓到的缺陷：绑定值全部按字符串下发 → `WHERE "id" = $1` 对着 `bigint` 计划成 `bigint = text`（PG 没有这个操作符，**每一次行编辑都失败**；MySQL 自己会转，所以一家的绿跑证明不了什么） | `dbui-ref/ARCHITECTURE.md` §What the live tests caught（"Every row edit failed on PostgreSQL"）· 用例 `crates/dbui-driver/src/sql_build.rs::tests::scalar_keys_are_bound_without_a_cast` |

> **我们还比 dbui 严一档（已核实差异）**：它的 `apply_changes` 只把 `rows_affected` **累加**，**没有**逐行校验（`crates/dbui-driver/src/postgres/mod.rs::apply_changes`）。"写了几行"这件事我们要求 `== 1`，见 §3.3 末条。

---

## 3. 冲突检测两条路

### 3.1 路 A：保守白名单（dbui）

- 行身份 = **主键值**（`[(列, 值)]`，复合键可表达）；**没有主键就拒绝**（A6 / A7）。
- 外键跟随只做单列（A9）—— 复合键连"能不能跟"都不进白名单。
- 没有任何事后校验：语句按主键定位，能影响几行就几行。
- 一句话：**能明确定位才做，不能就不做**。

### 3.2 路 B：PG 的乐观 + 物理位置确认（fluxDB，GPL-3.0 → **只写思路，独立实现**）

依据 `fluxDB-ref/crates/fluxdb-connectors/src/parts/postgres/apply_changes.rs`（**已核实**；许可口径见 `../references/README.md` §0，**不抄代码**）：

1. 行身份是**多值映照**（`RowIdentity { values: BTreeMap<列, 值> }`）→ 复合键可表达；`WHERE` 逐列写成 `列::text IS NOT DISTINCT FROM ($n)::text`（参数也按 text 绑，避开驱动侧参数类型推断的报错）。
2. **先锁再改**：`SELECT tableoid, ctid::text FROM 表 <身份 WHERE> LIMIT 2 FOR UPDATE` —— 取回行数**不等于 1 就报冲突**（0 = 目标已不在，>1 = 身份不唯一）。
3. 写入语句再叠一段**物理位置**：`AND tableoid = $oid::oid AND ctid = CAST($tid::text AS tid)`（它自己的注释写明：物理位置**只用于本事务**，不进历史、不当跨页身份）。
4. 写入后 **`affected != 1` 即冲突**（0 = 行已消失或被并发改，>1 = 命中多行）→ **整批回滚**。
5. `INSERT` 追加 `RETURNING <主键列>::text`：自增 / 序列 / 默认值生成的主键**只能由服务端给**，拿回来才构造得出真实行身份。

### 3.3 我们选哪条，为什么

**选路 A 作为唯一实现**（"能不能编辑"由主键决定），并**只借用路 B 第 4 条思路**（写入后按影响行数校验，`!= 1` 即整批回滚；**独立实现**，不引 `ctid` / `FOR UPDATE` / `RETURNING`）。理由三条：

| # | 理由 | 依据 |
| --- | --- | --- |
| 1 | **与现有闸门风格一致：宁可回绝也不猜**。项目里凡是"判不准"的地方一律回绝并说原因：转译的源方言认不出就不转（`dialect_of_known` 回 `None`）、错误位置认不出就**不给位置**（"定位错比不定位更糟"）、列类型没有就不导类型化值、项目只读 + 写源库对象在**提交前**就拒。把"身份不唯一"留成一次运行时冲突（路 B）与这条风格相反 | 架构 §11（D8b · `crates/editor/src/translate.rs`）· dev-plan §0 2026-09-17（B6：认不出就不给位置）· dev-plan §0 2026-09-18（B7 切片二余项：列类型化输出需先有列类型）· 架构 §13 #9b（项目锁 = 强只读，提交前拒） |
| 2 | **`ctid` 是 PG 独有**，另外三个内置驱动（MySQL / SQLite / DuckDB）没有对应物；把我们唯一的选择绑在 PG 上，会出现"PG 能编辑、SQLite 只能在主键上编辑"的分叉 —— 而消除这类分叉正是 D1（一个内核 + 三档能力表）要防的事 | `dbui-ref/ARCHITECTURE.md` §The layers（"Every engine-specific behaviour is proved on every engine"）· 架构 §6 D1 · §7.1（V1 复制三套模式规则产生的系统性分叉） |
| 3 | **`ctid` 不是持久身份**（本事务内的一次性锚点），而我们的页面语义是"这一屏是可复现的窗口"（D24：已抓取窗口 + 取下一段）；引入物理位置会多出一层只有 PG 才有的状态，且刷新 / 跨页后必须重取 | fluxDB `apply_changes.rs` 自己的注释（"物理位置仅用于本事务，不进入历史或跨页身份"）· 架构 §6 D24 · §3.6（分页策略永不合并） |

**要抄的那一条（思路，独立实现）**：写入后**逐条**校验影响行数，`!= 1` → 整批回滚 + 一句可读的冲突原因（"目标行已不存在或被并发修改" / "身份不唯一，命中多行"）。它引擎中立、成本低 —— 我们四个原生驱动**都已真填** `affected_rows`（`driver::utils::affected_rows_result` 与各驱动的 `execute_writing`，已核实）。

**候选 C（推断，备查）**：乐观并发（`WHERE 主键 = 旧值 AND 列 = 旧值`，思路同样取自 fluxDB 的 `WHERE` 写法 —— 把"我看到的旧值"当条件）。更严，但会把语句放大（每列一段 `AND`），且旧值里的 `NULL` 要另写 `IS NULL`。第一期不选它的理由只是成本（它是"更严"而不是"更猜"，风格上不冲突），留作将来需要"改前值校验"时的入口。

---

## 4. 与现有设计的关系（会动到谁 / 今天不要动）

### 4.1 会动到的（将来，逐条给结论）

| 目标 | 结论 | 依据 |
| --- | --- | --- |
| `crates/editor/src/store.rs` 的 `ResultEntry` / `ResultStore` | **不动**。D23 结果集只读；表编辑的草稿 / 行身份 / 提交态属于 M4 面板自己的状态。"往 `ResultEntry` 里加 `dirty` 字段"就是 V1 的 `dirtyRows` / `dirtyCells` 双轨重来 | 架构 §6 D23 · §4 状态地图（`ResultStore` 的职责是"结果集列表 + 选中项"）· §3.6 的"永不合并"清单 |
| `crates/editor/src/view/results/grid.rs`（渲染层） | **将来借，今天不抽**。它现在是"一个使用方"（结果集）；架构 §3.6 的三条提炼触发（第二个真实使用方 / trait 形状稳定一个迭代 / 共享部分够 ~500 行）**一条都还没中**，所以设计里的 `GridDataSource` / `GridEditSink` trait **尚末抽**（§11 映射表原文）。到第二刀才是纯机械移动 | 架构 §3.6（提炼触发与落点二选一）· §11（D23 行）· `crates/editor/src/view/results/mod.rs`（原生能力台账） |
| 驱动 trait：要不要加 `update_row` / `insert_row` / `delete_row` | **不要**。已核实：`Database::query_with_params` 在四个原生驱动都是**真 prepared statement**，事务走 `begin_transaction` → `Transaction::{query, commit, rollback}`，影响行数由 `driver::utils::affected_rows_result` 回填 —— 行编辑要的三样（参数化语句 / 事务 / 影响行数）**驱动面已经全有**。要补的是**上层**：编辑器的执行端口 `QueryRunner` 今天只吃文本 SQL（架构 §12 #7「参数绑定」仍是 ⚪ 余项），表编辑需要一条**参数化下发**的通道 | `crates/engine/src/driver/traits.rs`（`query_with_params` 的文档写明四驱动均为真 prepared statement）· `crates/engine/src/driver/utils.rs::affected_rows_result` · 架构 §12 #7 |
| 驱动能力位 `table_editor` | **落地时才声明**。今天 `Stage::NotBuilt` 且无人声明；功能真做完再把键加回 `descriptors.rs::capability_keys()`，并给**真机用例名**（字典里的验收引用会被 `acceptance_evidence_is_real.rs` 校验"用例名必须真实存在"） | `../driver-capability-matrix.md` §3.3 / §7 #13 · `crates/engine/src/driver/capability.rs`（`Stage` / `Acceptance` 与两条守卫测试） |
| 新增落点（未来） | `crates/database`（M4：表数据面板 + 草稿 + 提交）+ 共享渲染层（先在 `editor/src/view/results/grid.rs`，提炼触发命中后搬 `shared/` 或独立 `crates/grid`，见 §3.6 的落点二选一） | 架构 §3.6 · `crates/database/src/lib.rs`（M4 现有模块面：导航 / 属性面板 / `sql_gen.rs`） |

### 4.2 今天不要动的（清单）

- **`ResultEntry` / `ResultStore` / `view/results/**` 的结果区口径**（只读、分段抓取、导出、右键菜单、键位）—— B5 / B5b / B7 / B14 / B15 / B17 已定稿。
- **`Database` / `Transaction` trait 的方法面**（够用；加 `update_row` 是把业务语义塞进驱动层）。
- **驱动能力声明**：`table_editor` 不许提前声明（`capability` 的单测会红）。
- **`crates/engine/src/driver/*` 的内省面暂时不动**：外键跟随要的"目标表 / 目标列"确实缺（`ConstraintDetail.referenced_table` / `referenced_columns` 在 v2 里**没有任何驱动写入**，已核实），但那是刀 E3 的事，且应复用 `MetadataService` 这条唯一闸门，而不是各驱动自开一条。
- **导出路径**：`editor/src/export.rs` 里的 `INSERT` 编码是**只读产物**，不是写回通道 —— 别把两者接在一起。

---

## 5. 落地顺序（四刀，每刀给验收）

> 每刀的"怎么测"按本仓既有三层走：**纯函数单测**（最高价值）→ **headless 窗口测试**（`#[gpui_kit::test]`）→ **真机套件**（环境变量门控，四库；参考 `crates/workbench/tests/editor_exec_real.rs` 的既有做法）。契约面（新增视图文件、裸 `px(` / 裸色值）必须同步登记 `ui_contract` 的两份清单（架构 §10）。

| 刀 | 内容 | 验收怎么测 |
| --- | --- | --- |
| **E1 只读表浏览（窗口 + 行身份）** | 表数据面板：`ORDER BY`（用户排序 + 主键）、`limit + 1` 探测、真 `COUNT(*)`、无键表如实说"无定义顺序"；行身份解析：主键列（`ColumnDetail.is_primary_key`）+ 复合键识别 | ① 纯函数：语句生成（排序与主键的次序、`WHERE` 拼接、标识符引号 + **敌意表名**用例）；② 真机四库：一张千行级表逐页拼起来**不重不漏**、`has_more` 判据等于 `limit + 1`；③ 无键表 / 视图：界面明说"无定义顺序"而不是假装稳定 |
| **E2 暂存 → 一个事务提交 → 整批丢弃（单行）** | 草稿对象（值 / `NULL` / `DEFAULT` 三态）+ 一个 `RowBatch` 提交（`begin` → 逐条 → `commit`）+ 一个动作整批丢弃 | ① 纯函数：草稿合成（值 → `SET` 子句）；② 窗口（假执行器）：**暂存不碰执行器**、提交只发一次 `begin`、语句顺序与绑定值逐条断言、**删除失败时编辑一起回滚**（库内容不变）、丢弃后执行器零调用；③ 真机：改一行 → 提交 → 重读一致 |
| **E3 批量编辑 `MIXED` + 整批删除 + 外键跟随** | `MIXED` 是写入记号（留 `MIXED` 的列不进 `SET`）；批量删除按主键、无主键拒；外键跟随只做单列（要先用内省补出目标表 / 目标列） | ① 纯函数：多行草稿的 `MIXED` 合成、"整体重算不是合并"（各行原本 staged 过的列不会双计）、`MIXED` 列不出现在 `SET` 里；② 窗口：无主键表给可读拒绝（对齐 `committing_a_table_with_no_primary_key_says_why`）、复合外键**不给跳转**（对齐它的内省过滤）；③ 真机：多行改同一列 + 一批删除，事后核对 |
| **E4 只读由服务端强制 + 冲突校验 + 能力位声明** | 只读连接在**建立连接时**由引擎自己上锁（MySQL `SET SESSION transaction_read_only = 1` / PG `SET default_transaction_read_only = on` / SQLite 只读开库）；写入后 `affected != 1` → 整批回滚；`table_editor` 加回声明并给真机用例名 | ① 真机三库各一条：只读连接下写 → **被引擎拒**（不是被界面拦）；② 纯函数 + 窗口：`affected != 1` 的两种原因各一条可读文案；③ 能力面：`capability` 的单测跟着更新（`NotBuilt` 撤下），`acceptance_evidence_is_real.rs` 引用的用例名必须真实存在 |

**跨刀的公共约定（建议现在就定，省得返工）**：

- **草稿的形状照 dbui 抄**：`RowDraft { rows: Vec<行下标>, 逐列: 值 / MIXED / NULL / DEFAULT }`；`to_pending_batch()` 返回**整行最终值**（含没动的列），调用方**先清空那些行再并入**（不是合并）。
- **身份与改动分开传**：`(主键列, 旧值)` 与 `(改的列, 新值)` 两条都是 `Vec<(String, Value)>`，别合成一个 map —— 否则"没改的列"与"改成和旧值一样"分不出来。
- **只读是两道**：界面在提交前拒（给可读原因）+ **引擎在服务端拒**（最后一道）。只有前一道的那种"看着像只读"不算数（A10 的写法就是第二道，也是它会断言的那条）。

---

## 6. 实现位置映射（计划落点，**均未实现**）

| 内容 | 落点（计划） | 状态 |
| --- | --- | --- |
| 表数据面板（窗口 / 分页 / 浏览） | `crates/database/src/`（M4；新增视图文件需登记 `ui_contract` 两份清单） | ⬜ 未实现 |
| 草稿与提交（编辑缓冲 / 行身份 / 事务） | 同上 —— **不进 `crates/editor`**（见 §4.1） | ⬜ 未实现 |
| 分页与语句生成（纯函数） | 同上（文件未定；建议与 `crates/database/src/sql_gen.rs` 分开 —— 那是"生成 SQL 注入编辑器"的模板，不是写回通道） | ⬜ 未实现 |
| 网格渲染层（共享） | 今天 `crates/editor/src/view/results/grid.rs`；提炼触发命中后搬 `shared/` 或 `crates/grid`（架构 §3.6） | 现状（使用方一个） |
| 能力位 `table_editor` | `crates/engine/src/driver/{capability.rs, registry/descriptors.rs}`（`Stage::NotBuilt` → `Ready` + 声明 + 验收用例名） | 现状（未声明） |
| 真机验收 | `crates/workbench/tests/` 或 `crates/engine/tests/`（沿用环境变量门控；四库各一遍） | ⬜ 未实现 |
