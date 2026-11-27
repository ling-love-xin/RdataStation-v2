# 分析源登记（`analysis_sources`）规格

**状态**：草案 · 2026-11-25 · 待用户验收后再动代码。
**表名**：`analysis_sources`（暂定，用户 2026-11-25 指定）。
**适用范围**：数据对象面板「源」组。**表组 / 文件组、数据库导航本体都不在本规格内。**
**上游口径**：`data-objects-invariants.md` 第 4 条（源的定义）、第 37 条（生产面 / 消费面）、
第 55 条（源清单归属 —— 本规格更正其事实陈述）。

---

## 1. 源是什么（只用数据对象里的定义）

铁律第 4 条：**源 = 挂到分析实例上的外部来源**，按连接成树，连接行是树根，展开才列下层。

本规格把它收成三条判据，逐条对应现有的"不占源段"：

| 不进源段 | 归哪 | 依据 |
|---|---|---|
| 项目分析库 `analytics` | 表段，小标 `资产 vN` / `未登记` | 第 4 条、第 8 条 |
| 全局分析库 `global` | 表段，小标 `未登记 · 全局` | 第 8 条 |
| `f_*` 按需视图 | 文件段（`reference` 按需） | 第 15、16 条 |
| `tmp_q_*` / `tmp_l_*` | 表段，小标 `结果集` / `载入` | 第 12 条 |
| `tmp_i_*` / mock 产物 | 面板外 | 第 12 条 |

**「分析源」= 源段的对象**，不是整块面板的内容（2026-11-25 用户口径）。面板内容域是
**项目 ∪ 全局**，但"分析源"只指源段。

## 2. 内容域矩阵（项目 ∪ 全局）—— 防止再被理解成"整块都项目级"

| 段 | 内容 | 真相源 | 域 | SQL 引用 |
|---|---|---|---|---|
| 源 | 外连接直查、数据库文件挂载 | 登记（项目库）∩ `duckdb_databases()` | 登记项目级；被引用的连接本身可为全局连接 | ATTACH 别名 |
| 表 | 项目分析库的表 | `analytics.main.*` | 项目 | `analytics.main.<表>` |
| 表 | 全局分析库的表 | `global.main.*` | **全局** | `global.main.<表>` |
| 表 | 结果集 / 载入 | `tmp_q_*` / `tmp_l_*` | 会话 | `tmp_q_xxx` |
| 文件 | 项目资产的结构化文件 | 资产库（项目） | 项目 | `f_*` / 挂载 |
| 文件 | 全局资产的结构化文件 | 资产库（全局） | **全局** | `f_*` |

**全局项不需要登记行**：全局分析库、全局资产由 DuckDB + 资产库现查，靠小标区分
（`ArchiveKind::File` / `AnalyticsTable`，见 `analytics_resource/src/model/kind.rs:45`；
`RowScope::Project` / `Global`，见同文件 `:7`）。

## 3. 两半真相

「源」的判定不再靠单一来源，而是**意图 ∩ 事实**：

- **意图（持久，项目级）**：项目库 `.RSmeta/project.db` 的 `analysis_sources` 表。
- **能力（驱动常量，2026-11-27 起）**：`AccelKind::from_db_type` 认得的驱动才可被 DuckDB 直连；
  **连接上已没有 `use_duckdb_fed` 开关**（字段已删），「要不要当源」只由项目登记表达。
- **事实（运行态）**：进程 DuckDB 的 `duckdb_databases()`。
  已用过的列只有 `database_name`、`type`：
  `engine/src/analysis/global.rs:112`（按 `database_name` 核对）、
  `store/src/migration/duckdb.rs:136`（读 `type`）。
  **`path` / `read_only` 两列本规格不写死**，实施前真机确认。

```sql
-- 事实侧（每次渲染前的取数，只读）
SELECT database_name, type FROM duckdb_databases();
```

## 4. 表结构

迁移已落地：`crates/store/migrations/project_meta/034_analysis_sources.sql`（建表）+
`035_analysis_sources_alias_domain.sql`（把 `alias` 唯一性**按档位分域**：`mount_kind <> 'accel'`
才唯一，加速档的 alias 只是展示名，两条加速源都叫 `rds_src` 也能并存）。
（同批风格对齐 `033_network_two_layer.sql`：TEXT 主键 + `TIMESTAMP DEFAULT CURRENT_TIMESTAMP`。）

```sql
-- 迁移版本：034
-- 数据库类型：SQLite（项目库 .RSmeta/project.db）
-- 作用：分析源登记（数据对象面板「源」组的意图侧；事实侧见 duckdb_databases()）
-- 更新时间：2026-11-25

CREATE TABLE IF NOT EXISTS analysis_sources (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,          -- 展示名（连接名 / 文件名，未经归一化）
    kind          TEXT NOT NULL,          -- conn | dbfile
    connection_id TEXT,                   -- kind=conn：引用既有连接（凭据不另存，铁律第 7 条）
    path          TEXT,                   -- kind=dbfile：本体路径（绝对）
    alias         TEXT NOT NULL,          -- SQL 别名 = duckdb_databases().database_name
    mount_kind    TEXT NOT NULL,          -- federation | accel | attach_file
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_analysis_sources_alias ON analysis_sources(alias);
CREATE INDEX IF NOT EXISTS idx_analysis_sources_kind ON analysis_sources(kind);
```

字段取舍（每条都有理由，不是省字段）：

| 字段 | 有 / 无 | 理由 |
|---|---|---|
| `alias` | **有**，唯一 | 它就是 SQL 里的引用名；铁律第 29 条要求面板显示实际引用名 |
| `mount_kind` | **有** | 三种挂载的入口 API、卸载方式、别名自由度都不同（见 §6、§7） |
| `scope` | **无** | 会话级不入库（§5）；被引用的连接自带归属域，从 `connections` 读，不复制一份 |
| `表清单` | **无** | 现查 `duckdb_tables()` / `duckdb_views()` 按 `database_name` 过滤，缓存一份就是两份真相 |
| `mounted_at` / `状态` | **无** | 状态是运行态事实，写进库就必然与实例漂移 |

## 5. 会话级为什么不入库

会话级（铁律第 6 条）= 切项目清。若写进项目库：

1. 存的是**注定要被清掉**的行；
2. 进程崩 / 换机器后留下孤儿行，下次打开项目看到"未挂载的项目级源"，其实是上次的临时挂载——**假记录**；
3. 项目级文件源本来就不走本表（第 6 条：项目级 = 归档成 `copy` 快照、生成资产）。

所以：**`analysis_sources` 只存项目级意图**。会话级只活在实例里，对账时表现为"事实有、意图无"，
行上标「会话级 · 未登记」，给「登记为项目源 / 卸载」。

## 6. 别名规则

用户 2026-11-25 定：**默认取第一个 —— 连接名直取，撞名加 `_2`**。

这套规则**已经实现在 engine 里**，登记侧直接复用三个纯函数
（`engine/src/duckdb/federation/registry.rs`），不另写一套：

| 函数 | 位置 | 实际行为 |
|---|---|---|
| `sanitize_alias(name)` | `:300` | 转小写；非 ASCII 字母数字 → `_`；合并连续下划线并去首尾；空 → `db`；首字符非字母 → 前缀 `s`；撞保留字 → 缀 `_src` |
| `unique_alias(base, taken)` | `:342` | `orders` → `orders_2` → `orders_3` …（`_x` 兜底） |
| `validate_alias(alias)` | `:356` | 非空、无首尾空白、字母/下划线开头、仅 ASCII 字母数字下划线、非保留字 |

保留字表：`federation/registry.rs:25` `RESERVED_ALIASES`（`memory` / `system` / `temp` / `main` /
`rds_src` + join / 查询族 SQL 关键字；注释记录过真机踩坑：`ATTACH … AS left` 直接语法错）。

**例外一处（必须写进规格，否则实现会撒谎）**：`mount_kind = accel` 的 ATTACH 别名是**固定常量**
`accel::SOURCE_ALIAS = "rds_src"`（`accel.rs:49`），不是按源命名。所以：

- `federation`：别名自由（`FederatedSource.alias` 就是调用方给的，`registry.rs` 全套校验在用）；
- `attach_file`：别名自由（`file_view::load_database_as(path, alias)`，`file_view.rs:164`）；
- `accel`：**别名不自由**。登记行的 `alias` 对它是"期望名 / 展示名"，实例里恒为 `rds_src`。
  是否把 `SOURCE_ALIAS` 改成按源别名，属于 engine 的改动，**本规格不做**，列入 §12 待确认。

改名成本：DuckDB 没有 `ALTER DATABASE … RENAME`，改别名 = `DETACH` + `ATTACH`。
现成入口：`federation::session::set_primary(owner, alias)`、
`file_view::load_database_as`（内部先 `USE memory; DETACH`）、
`accel` 侧需 `drop_session(conn_id)` + `ensure_on_instance(&AccelSource)` 重挂
（2026-11-26 起加速档落在分析实例，不再自建会话）。
**已挂载状态下改别名必须提示"正在跑的查询会断"。**

文件段的 `f_*` 视图名仍由 `file_view::alias_for(path)`（`file_view.rs:22`）派生，**不进本表**。

## 7. 三态对账与动作

定义：`I` = 登记表行集合（按 `alias` 匹配），`F` = `duckdb_databases()` 的 `database_name` 集合
（剔除 `analytics` / `global` / 内部 catalog `memory` / `system` / `temp` / `main`）。

| 态 | 判据 | 行上文案 | 动作 |
|---|---|---|---|
| 已挂载 · 项目级 | `a ∈ I ∩ F` | 别名 + 挂载种类 | 卸载 / 改名（重挂）/ 属性 |
| 已挂载 · 会话级（未登记） | `a ∈ F \ I` | 「会话级 · 未登记」 | 登记为项目源 / 卸载 |
| 未挂载 · 项目级 | `a ∈ I \ F` | 「未挂载」 | 挂载 / 移除登记 |

另有一种**不在三态里**的行（2026-11-25 落地时补定）：已连接、没挂、也没登记——它只是
「可挂的源」，行上写「未挂载」（**不带 scope**）。scope 得等用户真挂了 / 登记了才出现，
否则界面是在替用户做主。

**未挂载照样列一行**（跟铁律第 4 条"注册过但没连上的照样列"同一形态，只是判据从
"注册标记"改成本表行）。不另造顶部提示条、不另造子组——一行一个源，动作就在右键里
（第 42 条：行内不摆按钮）。

### 7.1 能力与登记的关系（2026-11-27 改口径：能力归驱动，启用归登记）

**连接上不再有开关**（`use_duckdb_fed` 已删，模型 / 存储 / 对话框 / 草稿箱一并清理）。
两件事各归各的：

| 问题 | 由谁回答 | 落点 |
|---|---|---|
| 这个驱动能不能挂进 DuckDB | 驱动常量 `AccelKind::from_db_type` / `local_accel_support` | 代码，不落库 |
| 这条连接要不要当源 / 当哪一档 | 项目登记 `analysis_sources.mount_kind` | 项目库（意图清单，唯一持久状态） |
| 这条源此刻挂上了没有 | 分析实例的 catalog 事实（`snapshot()` / `duckdb_databases()`） | 运行态，不落库 |

- **登记是唯一入口**：面板「加源」走 `ensure_registration`（幂等，已存在不重复插）；
  对话框保存不再写任何开关值，只在删除连接时清理 Secret。
- **Secret 只按能力注册**：驱动有对应 DuckDB 扩展就注册，与"当不当源"无关（注册 ≠ 启用）。
- 三条执行通道的判据：
  源库直连（不经 DuckDB，无需判据）；本地加速要 `mount_kind == Accel`；
  联邦要**至少一个**「登记过（任意档）∪ 本文档绑定的连接」（2026-11-26 起单源放开；
  全都不满足才拦，并逐条点名 —— 见 `editor_channels::accelerated_availability` /
  `federated_availability`）。

## 8. 挂载 / 卸载 / 列举的入口映射

| `mount_kind` | 挂载 | 卸载 | 列举表 / 视图（叶子来源） |
|---|---|---|---|
| `federation` | `instance.ensure_session(owner, "联邦")` → 返回的 `AnalysisSession::mount_sources(&sources)`（会话真值只有一份，见 `analysis-instance-sessions.md`） | `instance.close_session(id)` / `federation::session::drop_session(owner)` | `federation::session::list_tables_for(owner, alias)` |
| `accel` | `accel::ensure_on_instance(&AccelSource)`（落分析实例，别名固定 `rds_src`） | `accel::drop_session(conn_id)` | `instance.list_mounted_tables_for_conn(conn_id)` |
| `attach_file` | `file_view::load_database_as(conn, path, alias)` / `load_database_global` | `USE memory; DETACH "<alias>"`（默认 catalog 指向该别名时必须先 `USE memory`） | 按 `database_name` 过滤 `duckdb_tables()/duckdb_views()` |

> 2026-11-26 更正：旧表里的 `federation::session::ensure_session` / `accel::ensure_session`
> 已不存在（前者改由分析实例拥有会话，后者改名 `ensure_on_instance`）。
> 铁律第 37 条原记的「网络连接行缺挂载入口」缺口也已关闭，见下。

> 2026-11-26 更正（会话角色）：`ensure_session` 对同一 owner 是 1:1 复用，label 只在第一个建会话
> 的路径写一次（先开全局分析库 → 那条会话 label 停在「全局分析」，之后联邦执行复用它）。
> 所以「这条算不算联邦 / 分析会话」的判据是 `AnalysisInstance::is_analysis_session`：**挂着源**，
> 且 owner 不是 `process` 也不是 `accel:<连接 id>`；`label` 只当展示名。照 label 过滤的
> 源段列举与会话面板（`federation::session::owners()`）已一并改掉。

> 2026-11-25 已补齐：`components/data_objects_load.rs` 的 `mount_network_source` /
> `unmount_network_source`（凭据取法与 `editor/src/ports/editor_exec/conn.rs::accel_source`
> 同一口径：`url_override` 优先、退回 `ConnectionInfo.url`），源段右键「挂载 / 卸载」接上；
> 意图侧的「登记为项目源 / 移除登记」（`register_connection_source` / `unregister_source`）
> 是独立菜单项，不跟挂载合成一步。

## 9. 渲染：只借导航的视觉与结构语言

用户 2026-11-25 口径：**`源` 的视觉与结构语言参考数据导航**，但**不抽导航组件**、
不碰 `NavView` / `NavHost`（`database/src/nav_view.rs:763`、`nav_host.rs:114`；
后者 271 行 / 40+ 方法，唯一实现是 `workbench/src/components/nav_host.rs:60`；
源段行是导航语义（连接/断开/DDL/右键），与"选表"不是一套）。

借用的是这些，逐项对应导航侧实现：

- 行形态：图标 + 名字 + 类型徽标 + `≈N` 行数（`database/src/nav_view/rows.rs:2244 render_nav_node`）；
- 缩进：`ui::TREE_INDENT`（`workbench_shell/src/ui.rs:158`，0.875rem = 14px）逐层；
- 展开箭头：`nav_view/primitives.rs` 的 disclosure 形态；
- 层：**别名（=catalog）→ schema → 表 / 视图**（见 §10 冲突项）；
- 叶子数据**一律现查实例**：`duckdb_tables()` / `duckdb_views()` 按 `database_name = 别名`
  （`accel.rs:651`、`file_view.rs:237`、`federation/session.rs:470` 已是同一口径）。

**交互（铁律第 42 条，2026-11-25 用户口径）**：源段的表 / 视图叶子行**右侧不摆任何按钮**，
连接行也不摆；「选入工作集 / 移除」、复制引用名、归档到分析库…、载入内存…、洞察取样…、属性…、
挂载 / 卸载 **全部走右键菜单**。

现状要删的一处：`data_objects_tab.rs:2386-2399` 的 `data-object-srctable-pick-<key>`（「选用 / 已选」）。
理由是行的宽度：源段三层树，每层 `TREE_INDENT`（14px）再加一层，窄面板（`RowTier::DropChannelAndPill`，
< 318px）下名字只剩 120px 上下，行尾按钮吃的正是名字。

不缓存的理由同 §4：缓存一次就要处理失效，而这里一次查询就够（单别名、按需展开）。

## 10. 与铁律现有条文的冲突与改法（需用户点头后我再改文档）

### 10.1 第 4 条 vs 第 37 条：源段几层

- 第 4 条：「**源段按连接成树**：连接行是树根，展开才列 **catalog / schema / 表**」（三层）。
- 第 37 条：「**不共用树**：源段是**扁平**的可分析叶子（表 + 视图），**不带类别文件夹**、不带 DDL」（两层）。
- 现状代码：连接 → 表（两层，`data_objects_tab.rs:2180 source_leaf_row` / `:2333 source_table_leaf_row`）。

**建议的调和读法**（本规格按它写）：两条不矛盾——第 37 条禁止的是**类别文件夹**（表/视图分组）与 DDL，
不是 schema 层。DuckDB 挂载后的引用本身就是 `别名.schema.表`，别名即 catalog。
所以源段 = **别名 → schema → 表/视图**（不套「表 / 视图」文件夹），需要把第 37 条那句
"扁平"改成"不带类别文件夹"的准确措辞。

### 10.2 第 55 条：「源清单是资源模块的职责」—— 事实有误，必须更正

已核实：资源模块只有**两类**（`analytics_resource/src/model/kind.rs:45`）：

- `File`（本体在 `{项目}/resources/`）、`AnalyticsTable`（本体是 `analytics.duckdb` 的持久表），
- 外加一个**不写入**的兼容档 `TableRef`（"引用"），且它刻意不进筛选菜单（`kind.rs:65`）。
- **没有"源"这一类**。

而且全仓没有任何模块持有"源清单"：连接清单在 `store` 的 `connections` 表 + 导航视图；
「能不能被直连」现在只由**驱动常量**回答（`AccelKind`），连接记录上已无开关字段
（2026-11-27 删 `use_duckdb_fed`），清单仍只有 `analysis_sources` 一处。

**推论**：新建 `analysis_sources` 不是"造第二份真相"，而是**第一次把源清单落地**。
第 55 条那句要改成事实陈述：「资源模块只有 文件 / 分析库表 两档，不涉源；源清单归
`analysis_sources`（项目库）」。

### 10.3 需要同步的编辑清单（2026-11-25 已按用户批准落实到铁律）

1. 第 4 条：判据从"注册过（`use_duckdb_fed`）"→ **"登记 ∩ `duckdb_databases()`"**，并补三态；层数写明；
2. 第 37 条：删掉"扁平"歧义，改为"不带类别文件夹、不带 DDL"，并写明不共用 `NavView` / `NavHost`；
3. 第 6 条（原文在文件第 55 行，不是"第 55 条"）：按 §10.2 更正事实；
4. 新增第 42 条：源段叶子行右侧不摆按钮、动作走右键；
5. 「变更记录」补一条（2026-11-25 源段定稿）。

## 11. 验收口径

- **纯函数单测**：别名默认（`sanitize_alias` + `unique_alias`，含中文连接名 → `db` / `db_2`）、
  三态判定（`I ∩ F` / `F \ I` / `I \ F`，含剔除内部 catalog 的用例）。
- **真机测量**（沿用 `panels/data_objects_tab/layout_probe.rs` 的工具）：
  窄面板 260 / 340 / 440 / 680 四档下，**别名 → schema → 表 三层缩进**后的名字可用宽；
  与现有 `RowTier` 阈值（410 / 318）是否仍成立，若因缩进变化需重算，出实测数后再改常量。
- 挂载 / 卸载各一条真机用例：挂上去 → `duckdb_databases()` 出现别名 → 卸下来 → 消失 → 重开项目仍是"未挂载"。

## 12. 待确认（不在本规格内定）

1. ~~`duckdb_databases()` 的 `path` / `read_only` 两列在 1.5.6 下的确切列名与可用性（真机验）~~ —— **已定（2026-11-26，DuckDB 2.0.1 真机）**：列名是 **`path`** 与 **`readonly`**（不是 `read_only`），**没有** `storage_version` 列；整表列为 `database_name, database_oid, path, comment, tags, internal, type, readonly, encrypted, cipher, options`；
2. `accel` 档固定别名 `rds_src` 是否要改成按源别名（动 engine，需单独评估多源并发场景）；
3. 表名 `analysis_sources` 暂定；
4. 「已挂载 · 会话级」行的默认动作是"登记为项目源"还是"卸载"优先。

### 12.1 别名唯一性按档位分域（2026-11-25 第 6 条，已落地）

`alias` 的**语义三档不同**，全表唯一是错的（迁移 `035_analysis_sources_alias_domain.sql`）：

| 档位 | `alias` 是什么 | 唯一性 |
|---|---|---|
| `federation` | DuckDB 里的 schema 名（`database_name`） | 与同档互斥（全局唯一索引） |
| `attach_file` | 挂载名 | 同上 |
| `accel` | **只是展示名**（ATTACH 固定 `rds_src`，一次一库） | 允许重复；**不占**挂载名 |

据此：`taken_aliases` 只把"别名即挂载名"的两档算占用（否则第二条加速源的展示名会
白白抢掉联邦想要的 `xxx_src`）；`insert` / `update_alias` 的"别名已被占用"报错
只可能来自那两档。identity 判重对加速档看 `connection_id` / `path`，不看 `alias`。

## 13. 明确不做（避免复活已否决项）

- 不做"其他资产（不可分析）"段；
- 不做项目级持久源清单之外的第二份清单（本表就是唯一一份）；
- 文件不做第三种取数模式（只有 `copy` / `reference`）；
- `tmp_i_` / `tmp_m_` / `tmp_p_` 不进面板；
- 不把动作收进「…」菜单；
- 不改导航本体、不抽 `NavView` / `NavHost`。

---

**变更记录**

| 日期 | 内容 |
|---|---|
| 2026-11-25 | 初稿：源 = 登记 ∩ `duckdb_databases()`；`analysis_sources` 只存项目级；别名复用 `registry` 三函数；列出铁律第 4 / 37 / 55 条改法 |
| 2026-11-25 | 用户批准三处改法并补一条：源段表/视图叶子行右侧不摆按钮、动作走右键（铁律第 42 条）。铁律已同步，规格 §9 / §7 跟着改。另更正前稿把"第 55 行"写成"第 55 条"——铁律只有 1–42 条，那句话是第 6 条的子句 |
| 2026-11-25 | 落地三段：①`analysis_sources` 登记表 + `data_objects` 三态对账（含「已连接 / 未挂 / 未登记 = 未挂载（不带 scope）」与「登记了但连接不在清单里照样出一行」）；②源段三级树 + 叶子右键化（第 42 条）；③右键「挂载 / 卸载」接上 `accel::ensure_session` / `drop_session`，登记 / 移除登记为独立菜单项——§8 记的缺口关闭。测试：workbench `data_objects` 49 passed、store `analysis_sources` 9 passed、`ui_contract` 10 passed |
| 2026-11-25 | 落地三段：①`analysis_sources` 登记表 + `data_objects` 三态对账（含「已连接 / 未挂 / 未登记 = 未挂载（不带 scope）」与「登记了但连接不在清单里照样出一行」）；②源段三级树 + 叶子右键化（第 42 条）；③右键「挂载 / 卸载」接上 `accel::ensure_session` / `drop_session`，登记 / 移除登记为独立菜单项——§8 记的缺口关闭。测试：workbench `data_objects` 49 passed、store `analysis_sources` 9 passed、`ui_contract` 10 passed |
| 2026-11-26 | 三态收口（用户拍板）：① `use_duckdb_fed` = 能力 / 加速开关（`None` / `false` / `true`），**有值即登记**；② 修正"联邦标记"说法（§7.1 / §10.2）；③ §8 入口映射的旧 API 名更正（`federation::session::ensure_session` → `instance.ensure_session` + `mount_sources`；`accel::ensure_session` → `accel::ensure_on_instance`）；④ 迁移 034 + 035 记为已落地；⑤ 加速档不占源段行（`accel:` 行与 `SourceRef::Accel` 已删，`eec47963`） |
