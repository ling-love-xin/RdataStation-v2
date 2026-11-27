# Catalog / Context：SQL 编辑器里的 DuckDB 联邦分析

> 定位：从内核到界面的**唯一骨架文档**。目标是让「SQL 编辑器里跨源查询」成立，
> 数据对象面板只是这套骨架的**只读视图**。
>
> 与既有实现的关系：本文是重新设计的模型；存量代码按 §10 的映射被吸收或替换，
> 不以旧条文为前提。

---

## 0. 一句话

**DuckDB 自己就是唯一真值。**我们只持久化「从哪来 + 要不要自动挂」，
运行态一律向实例问（`duckdb_databases()` + 会话挂载记录），任何一层想自己记一份
"挂了什么"，就是漂移的起点。

---

## 1. 三层归属

| 层 | 载体 | 拥有 | 生命周期 | 绝不拥有 |
|---|---|---|---|---|
| 实例 | 进程内一个 DuckDB `Database` | catalog 集合（ATTACH 结果）、扩展、内存上限 | 进程 | 用户意图 |
| 会话 | 每条执行通道一条 `Connection` | search_path、`TEMP`、`USE` 当前库、中断句柄 | 文档 / 面板 / 洞察 / mock | catalog |
| 语句 | 一次 `run` | SQL 文本与结果 | 一次执行 | 任何持久状态 |

由此得到两条必须写进产品文案的事实：

- **catalog 是实例级**：A 编辑器挂的库，B 编辑器也能用限定名查；
- **未限定名是会话级**：B 能不能写裸表名，取决于它自己的 search_path。

---

## 2. Catalog

### 2.1 类型（`engine::duckdb::catalog`，K0 已落地）

```rust
enum CatalogOrigin { Network { conn_id, driver }, File { path }, Base { path } }
enum CatalogHolder { Session(owner), Conn(conn_id) }
enum CatalogState  { Ready { tables }, Failed(String) }   // 原话不翻译
struct CatalogInfo { alias, origin, state, holders: Vec<CatalogHolder> }
```

`catalog::snapshot()` 把三处事实归一成一个别名一行：

| 事实来源 | 位置 | 说明 |
|---|---|---|
| 会话挂载 | `AnalysisSession::mounted_sources()` | 含挂载那一刻的失败原话 |
| 加速档挂载 | `AnalysisInstance::mounted_conns()` | 按 `conn_id` 记，别名固定 `rds_src` |
| 库文件索引 | `file_view::catalog_files_snapshot()` | **唯一**自维护冗余：别名 → 原路径 |
| 底座 | `federation::session::bases()` | 跟着会话走，不是用户加的源 |

合并规则：同一别名只出一行；持有者取并集并排序；**状态先到优先**——
会话记录最可信，不能被后来的"挂上了"盖掉，否则挂载失败会被静默说成可用。

### 2.2 唯一自维护的东西

`duckdb_databases()` 只说别名，说不出源路径。所以「别名 → 库文件路径」必须自己存，
但**写入口只有两个方向**：

- `load_database_global` / `load_database_as_global` 成功后记账；
- `detach_database_global` 成功后抹掉。

不放进 `load_database` / `detach_database`：那两个吃调用方给的连接（可能是临时连接或测试），
记进进程索引会把别人的挂载算到本进程头上。

### 2.3 别名纪律

- 别名是**用户 SQL 契约**：一旦给出并出现在文档里，永不被系统改写。
- 冲突 = 失败 + 报占用者。后到者自动改名（`x_2`）是禁止的：那会让同一份 SQL 在不同时刻
  指向不同的库。
- 保留名（`memory` / `analytics` / `rds_global` / …）不允许当源别名。

---

## 3. Context（会话）

```rust
struct Context { owner: String, aliases: BTreeSet<String>, primary: Option<String> }
```

- `enter()` 下发 `USE` / `SET search_path`；退出恢复。
- **主源是 search_path 的函数**，不是配置项：`search_path = 主源 || 底座 || memory`。
  切主源 = 改一条 search_path，零额外状态。
- 临时表钉在 `memory.rds_tmp`：实例共享、可跨文档查，归属可追。

---

## 4. 执行路径

1. **不改写 SQL。** 不做"把未限定名补成别名"的文本重写：CTE、子查询、动态 SQL 全脆。
   DuckDB 原生支持 `别名.schema.表`，交给它。
2. **发送前静态预检**（抽取在 `rds-sql::table_paths`，判定在 `editor::exec::preflight`）：
   - 抽出所有表引用：顶层 FROM/JOIN、派生表内部、CTE body、UNION 两侧、
     `INSERT … SELECT` / `UPDATE … FROM` / `DELETE … USING` / CTAS / 表达式里的子查询；
     CTE 名与派生表别名不算表，表函数（`read_csv` / `range`）不算表；
   - 未限定：命中 0 → 报"没有这张表"（附现有别名）；命中 ≥2 → **主源（search_path 首位）
     命中时直接放行**（那就是解析结果），主源没命中才报歧义并附候选（`a.orders` / `b.orders`）；
   - 限定：首段是已知别名 → 校验别名可用与表存在；首段不是别名时两段当 `schema.table`
     放行（`main` / `rds_tmp` / 用户 schema 本地分不清），三段以上报 catalog 不存在；
   - 跨源同名列 join：类型降级（DECIMAL→DOUBLE 等）只警告不阻断（未做）；
   - 事实不足（无实例 / 列不出表）→ 只保证能解析，不报存在性（宁可让 DuckDB 报原话）。
3. **错误单一出口**：连接失败、ATTACH 失败、别名冲突都在挂载时报；解析失败在预检时报。
   运行期只有"真正执行出错"一种，透传原话。

---

## 5. 持久化：意图清单

```rust
struct SourceIntent { id, origin, alias, auto_attach: bool, primary: bool }
```

- 重启 / 打开项目时 `replay`：按清单 ATTACH；失败的**保留行**并标不可用，不删。
- 「未挂载」的唯一正确来源 = 有意向、没事实。
- 删除行 = 显式撤销意图，不触发任何回填。
- 主源就是 `primary = true` 的那一行，重启可恢复。

**实现落点（K3 第二刀，2026-11-27）**

- 登记行仍在 `analysis_sources`（034）；主源是**单值**意图，另开**单行表**
  `analysis_primary`（`singleton = 1` + `CHECK`，迁移 036）。不用"给登记表加 `is_primary` 列"
  是因为 `ALTER TABLE` 与 `ensure_tables` 兜底有执行顺序冲突（老项目库会在迁移时直接打不开），
  而 `CREATE TABLE IF NOT EXISTS` 天然幂等；"至多一条"由库的 `PRIMARY KEY`/`CHECK` 保证，
  不需要应用层维护互斥。
- 写：`analysis_sources::set_primary`（只接受登记过的非加速档别名；加速档挂载名固定 `rds_src`，
  没有"选一个"这回事）。删登记行时顺带清掉指向它的主源（不许留"解析到不存在 catalog"的空悬意图）。
- 读：`analysis_primary` → `analysis_sources::publish_project(root)` 同时发布两张进程级快照
  （连接 → 别名 / 主源）+ 当前项目根；执行线程（`FederatedSourcePort`）只认快照。
- replay 的形态：**不存会话状态**。会话每次执行都按计划重建（`kernel::open` 幂等），
  `federated_plan` 组装完再把"登记主源"覆盖上去（`apply_registered_primary`）——所以
  "重启后恢复主源"与"平时执行"是同一条路，不需要额外的恢复代码。
- 覆盖规则：登记主源在本次清单里 → 用它（并留一条"主源按项目登记"说明）；
  不在 → **保留原主源**（文档连接的派生结果）并点名那条坏登记，不悄悄换源。


---

## 6. 并发与生命周期

- ATTACH 是实例级，需要实例级锁；DETACH 时若某会话 search_path 仍指向它，DuckDB 会拒绝。
- `CatalogRegistry` 记引用计数（引用该别名的 Context 数）。有人持有时 DETACH **拒绝并如实报
  "N 个编辑器正在用"**，不静默打断正在跑的查询。
- 关项目：先摘 search_path，再按引用计数逐个 DETACH；`rds_tmp` 不跟着清（可查、可手动清）。

---

## 7. 数据对象面板：只读视图

面板的职责是回答"**此刻能解析到什么**"，不是"管理挂载"：

- 零状态、零持久化，只调 `catalog::snapshot()` 与枚举能力接口。
- 两段，维度是**可用性**：
  - **参与分析（可解析）**：catalog 已挂且未被会话遮蔽 → catalog / schema / table 三级；
  - **可加入（尚未生效）**：有意向未挂（自动挂失败 / 手动卸）、有能力未登记。
- 一行一个**源**（不是一次挂载），所以同一源不会出两行。
- 小标只表达一件事：`可解析` / `意向未生效（原因）` / `可加入`；连接健康度用行首图标，不塞小标。
- 实现落点（K2 首刀已落）：会话行**归属连接在清单里**时并入该连接行（挂载事实与失败原话一并搬过去）；
  会话行只在「说不出归属」（没有 `conn_id` 的老调用方）时兜底各出一行，身份才带 owner。

---

## 8. 不变量（七条，所有争论按此裁决）

1. catalog 事实只归 DuckDB；我们只冗余"别名 → 路径"，单点维护。
2. 别名永不被系统改写；冲突即失败。
3. 意图清单是唯一持久状态；其余全部派生，重启可 replay。
4. 未限定名只由 search_path 解析，不改写 SQL。
5. 源只读（`READ_ONLY`）；写入只发生在项目库 / 全局库 / `rds_tmp`。
6. 面板只读快照，不写任何状态。
7. 失败只有一个出口：预检 / 挂载 / 执行，其他层不翻译错误。

---

## 9. 刀次与验收

| 刀 | 内容 | 行为变化 | 验收 |
|---|---|---|---|
| **K0a** ✅ | `catalog.rs`：`CatalogOrigin/Holder/State/Info` + `snapshot()`；`file_view` 加路径索引（两写入口） | **零**（只新增 API） | `cargo check -p rds-engine`；三个纯函数单测（同行合并 / 失败不被盖 / 底座不算源） |
| **K0b** ✅ | 面板读文件索引取代 `Shared.loaded_files`；该字段整条删除（不留兼容层）；指纹里删掉空的「洞察样本」维度 | 零（输出逐字不变） | `cargo check -p rds-workbench --lib/--tests` 通过；文件索引逐条清理（删视图即忘） |
| **K1** ✅ | 抽取 `rds-sql::table_paths`（递归全树、CTE/派生表/表函数排除）+ 判定 `editor::exec::preflight`（不存在 / 多源歧义给候选 / 未知 catalog / 源不可用）；联邦与加速两档发送前过闸 | 报错更早更准 | 表驱动：`rds-sql` 9 条抽取 + `editor` 13 条判定（含主源优先、内置库不参与歧义） |
| K2（进行中） | 面板改"源记录 + 可用性分段"，会话行并入。首刀：会话源带 `conn_id`，归属连接在清单里 → 并入连接行（含失败原话）；无归属会话行才兜底 | 重复行消失（已有性质测试「同连接两文档 = 一行」） | 待做：两段分组（可解析 / 意向未生效 / 可加入） |
| K3（进行中） | 意图清单 replay + 主源持久化；引用计数拒绝 DETACH。**已落两刀（2026-11-27）**：① 别名冲突改拒绝（`MountRegistry::reserve` 报错并点名占用者，同源重挂幂等）；② 主源持久化（`analysis_primary` 单行表 + 036；`publish_project` 发布两张快照；`apply_registered_primary` 让登记主源赢过文档连接，会话重建即 replay） | 重启不丢主源；冲突从静默改名变报错 | K3①：engine lib 355 passed；K3②：store 13 条 / editor 组装 18 条全过。待做：引用计数拒绝 DETACH；headless probe（restart 后 catalog 与 primary 恢复） |

K0a 是唯一前置，且行为零变化，可先合。

---

## 10. 与存量代码的映射

| 存量 | 处理 |
|---|---|
| `panels/shared.rs` 的 `loaded_files`（UI 进程内存） | K0b 已整条删除；读侧走 `file_view::file_entries()`（视图与 catalog 两类） |
| `instance.rs::MountRegistry::reserve` 的 `_2` 兜底 | **已删（2026-11-27）**：冲突即报错并点名占用者；同一条源重复申请幂等（与联邦路径 `alias_owner` 同口径） |
| `mounted_sources()` / `mounted_conns()` | 保留为内部事实源，外部只读 `catalog::snapshot()` |
| `analysis_sources`（项目库登记行） | 直接充当 §5 的意图清单（034）；**主源**另存单行表 `analysis_primary`（036，"至多一条"由库保证） |
| 主源只在会话内存（`AnalysisSession::primary`） | K3② 已改：会话里的 `search_path` 只是运行态，意图写 `analysis_primary`，组装时覆盖、重建即 replay |
| 面板 `Object::rank()` 按身份类型分档 | K2 换成"可用性"维度：可解析 → 意向未生效 → 可加入 |
| 联邦路径的 `alias_owner` 预检 | 保留，作为 §2.3 别名纪律的现成实现 |

---

## 11. 未定项（不写进实现，等拍）

1. 「驱动具备直连能力」与「登记」是否继续耦合在一个开关里？建议拆开：能力是常量，
   登记是显式动作。
2. 源只读是否**强制**：现阶段全部 `READ_ONLY`；将来若允许写源，要单独设计（事务边界、
   失败回滚、锁）。
3. Oracle / SQL Server 的驱动形态（ODBC 还是原生扩展）；`READ_ONLY` + 类型映射要另行实测。

---

## 12. 变更记录

- 2026-11-27：立稿；K0a 落地（`catalog.rs` + `file_view` 路径索引）。
- 2026-11-27：K3 首刀（别名冲突即拒绝）；K3 次刀（主源进 `analysis_primary`，`apply_registered_primary` 接管主源优先）。
