# 表资产快照模型设计（v2）

> 状态：设计定稿，2026-11-27。**旧表方案整体废弃，不做迁移、不做双写、不留兼容回退。**
> 关系：本文取代 `global-file-asset-design.md` / `global-reference-design.md` 中「表资产」的部分；
> 文件资产口径以 `file-asset-cas-design.md` 为准。
> 一句话：**表资产不指向一张活表，而是指向不可变快照；活表只是草稿。**

## 0. 决策记录

| 编号 | 决策 | 说明 |
| --- | --- | --- |
| T1 | **表资产必须冻结** | 归档/登记即生成不可变快照；`analytics_resources` 不允许再指向用户可写的活表 |
| T2 | **稳定资产 id** | `asset_id` 永不变；显示名只用于展示与 UX 查重，物理表名由 id 生成，不随改名变化 |
| T3 | **快照可被多版本共享** | 恢复 / fork / 引用只新增版本行指向同一快照，不复制数据；GC 按引用计数 |
| T4 | **编辑必须走租约** | 表资产本身只读；编辑 = checkout 出工作表 + lease，再归档时消费租约 |
| T5 | **表版本受保留策略约束** | 与文件不同，表快照体积大；当前版本、被 pin 的版本、租约基准、fork 来源永不裁 |
| T6 | **全局并发只 fork 不覆盖** | 发布带 `base_version`；base 落后于 head 时只能 fork / 取消 |
| T7 | **归档 schema 只读** | 快照放 `rds_archive`，SQL 写闸拒绝任何 DDL / DML 指向该 schema |
| T8 | **存为表与归档分离** | `存为表` 只建普通 `analytics` 表；显式归档才进 `rds_archive` 并消费源表 |
| T9 | **旧方案直接删除** | 不保留旧「活表指针」、旧 JSON 版本快照、旧 name 引用链；新代码只认新模型 |

## 1. 旧方案为何整体废弃

旧表方案的事实口径：

- `analytics_resources.version` 是当前版本；`analytics_resource_versions` 存整行 JSON 快照。
- 全局表「一版一张表」：物理表名 `res_<显示名>_<时间戳>`，登记行的 `analytics_table` 指过去。
- 项目表由 `table_asset::register` 登记；`analytics_table` **直接指向项目分析库里的活表**。
- `table_fingerprint` 只哈希表名，不哈希数据。
- 全局保留按 `global_name` 找同名链；引用按 `global_name + global_version` pin。

这套方案有六个根因问题，修补没有意义：

1. **冻结承诺不成立**：项目表资产指向活表，下一次 `UPDATE` 就改了“已归档”的数据，版本号却不动。
2. **内容指纹不覆盖数据**：`table_fingerprint` 只看表名；同名表被覆盖后指纹不变。
3. **身份跟着显示名走**：rename、同名冲突、跨项目引用都只能靠 name 字符串。
4. **恢复即改指针**：项目表回退把 `analytics_table` 指回旧表；旧表被覆盖或裁掉，历史就指向错误数据。
5. **没有编辑边界**：表资产没有等价于文件 lease 的“从哪版出去、改完回哪版”的凭据。
6. **快照与活表混在同一 schema**：资产表、用户表、工作表混在 `analytics`，无法做只读守卫与安全 GC。

因此本文不保留任何旧路径：**新代码只读写快照模型；旧表资产实现、旧 JSON 版本快照、旧 name 引用链全部删除。**

## 2. 目标与非目标

### 2.1 目标

1. 表内容一旦归档就不可变；任何数据变化只能生成新版本。
2. 版本身份稳定：`asset_id + version`，与显示名、物理表名解耦。
3. 恢复 / fork / 引用默认零复制：共享快照，靠引用计数保活。
4. 编辑有明确租约：checkout 出工作表，再归档消费 lease。
5. 项目 / 全局同一套表模型；全局引用按 `asset_id + version` pin。
6. 保留策略可执行：裁版本时同时裁物理快照，但绝不裁仍被引用的对象。
7. 只读边界可执行：`rds_archive` 只能 `SELECT`，写入只发生在普通表与工作表。

### 2.2 非目标

- 不把表导出成 Parquet 再入 CAS：M6 已删掉 parquet 中转，本文不重新引入。
- 不做表级字节去重：同内容的两张物理快照可以同时存在；逻辑层用 `manifest_id` 识别同内容。
- 不做行级 merge / rebase；冲突只有 fork。
- 不做整库快照 / DuckDB checkpoint 级备份。
- **不做旧表资产迁移、双读、双写、兼容列或回退路径。**
- 不把用户普通工作表自动搬进归档 schema；`存为表` 与 `归档为资产` 必须分开。

## 3. 核心模型

```text
Table Manifest（逻辑内容，可跨实例复用）
  manifest_id = sha256(canonical(schema + data_fingerprint + stats))
  schema_hash / data_fingerprint / row_count / column_count
        │
        ▼
Snapshot（某个实例里的一份物理表）
  snapshot_id, manifest_id, physical_schema, physical_table, state
        │
        ▼
Version（资产版本）
  asset_id, version, snapshot_id, manifest_id, parent_version
        │
        ▼
Head（当前版本）
  analytics_resources.id = asset_id, version = 当前版本
        │
        ▼
Lease（编辑租约）
  asset_id, base_version, work_schema, work_table, state
```

四个关键区分：

- **manifest 是逻辑内容**：同 schema、同数据指纹的两份物理表，`manifest_id` 相同，可跨项目/全局识别“内容一样”。
- **snapshot 是物理实例**：项目库一份、全局库一份，各自有 `snapshot_id` 与物理表名；物理对象可被多个版本共享。
- **version 是资产链**：`(asset_id, version)` 唯一；恢复 v1 生成 v3 时，v3 可以直接指向 v1 的 snapshot，不复制数据。
- **lease 是编辑边界**：只有 checkout 出来的工作表可写；归档消费 lease 后，工作表不再属于资产。

### 3.1 不变量

1. 一个 `asset_id` 同时只有一个 head 版本（`analytics_resources.version`）。
2. 一个 `(asset_id, version)` 只对应一个 snapshot；一个 snapshot 可被多个版本引用。
3. `table_asset_versions` 是表资产版本的唯一权威；表资产不再写 `analytics_resource_versions` JSON 快照。
4. 任何 live 版本引用的 snapshot 必须 `state='live'`；物理表缺失时先标 `missing`，读取明确报错，不静默回退。
5. `rds_archive` 里的表不接受 DDL/DML；写入只发生在 `analytics`（普通表）与 `rds_work`（工作表）。
6. 删除资产只软删登记；物理快照由 GC 按引用计数回收，不在删除路径直接 `DROP`。

## 4. 物理布局

| 位置 | schema | 表名 | 用途 |
| --- | --- | --- | --- |
| 项目分析库 | `analytics` | 用户表名 | 普通持久表 / `存为表` 的落点，可写 |
| 项目分析库 | `rds_archive` | `arc_<snapshot12>` | 项目表资产快照，只读 |
| 项目分析库 | `rds_work` | `wrk_<asset12>_<lease8>` | checkout 出来的工作表，可写 |
| 全局分析库 | `rds_archive` | `arc_<snapshot12>` | 全局表资产快照，只读 |

- `snapshot12 = snapshot_id` 去掉 `snap_` 后的前 12 位；物理表名不含显示名，rename 不影响。
- `rds_work` 只在项目库存在；全局表资产不能直接编辑，必须先 checkout 到某个项目。
- `rds_archive` / `rds_work` 不出现在数据对象面板的普通表清单里；资产库详情通过元数据定位。
- 不导出 Parquet：快照就是 DuckDB 原生表，跨库发布用 `CREATE TABLE ... AS SELECT`（同一 DuckDB 实例内是本地拷贝）。

### 4.1 只读守卫

- SQL 闸门：任何 `INSERT/UPDATE/DELETE/CREATE/DROP/ALTER` 指向 `rds_archive` 一律拒绝，提示「表资产只读，请取回为工作表」。
- 查看：`SELECT` 放行；会话视图 / 结果网格只读。
- 归档 schema 不暴露给普通表枚举；`SHOW TABLES` 过滤。
- 文件资产的只读守卫已有；本文只补表 schema 的同类守卫。

## 5. 元数据表

项目库与全局库各一份同构表；全局侧表名相同（不同 SQLite 文件）。

```sql
CREATE TABLE table_manifests (
    manifest_id       TEXT PRIMARY KEY,
    schema_hash       TEXT NOT NULL,
    data_fingerprint  TEXT NOT NULL,
    row_count         INTEGER NOT NULL,
    column_count      INTEGER NOT NULL,
    schema_json       TEXT NOT NULL,   -- 规范化列定义：name/type/null/default/ordinal
    created_at        TEXT NOT NULL
);

CREATE TABLE table_snapshots (
    snapshot_id       TEXT PRIMARY KEY,
    manifest_id       TEXT NOT NULL,
    instance          TEXT NOT NULL CHECK (instance IN ('project', 'global')),
    physical_schema   TEXT NOT NULL,
    physical_table    TEXT NOT NULL,
    state             TEXT NOT NULL CHECK (state IN ('live', 'missing')),
    created_at        TEXT NOT NULL,
    verified_at       TEXT,
    FOREIGN KEY (manifest_id) REFERENCES table_manifests(manifest_id)
);

CREATE UNIQUE INDEX ux_table_snapshot_physical
    ON table_snapshots (physical_schema, physical_table);

CREATE TABLE table_asset_versions (
    asset_id        TEXT NOT NULL,
    version         INTEGER NOT NULL,
    snapshot_id     TEXT NOT NULL,
    manifest_id     TEXT NOT NULL,
    parent_version  INTEGER,
    created_at      TEXT NOT NULL,
    PRIMARY KEY (asset_id, version),
    FOREIGN KEY (snapshot_id) REFERENCES table_snapshots(snapshot_id),
    FOREIGN KEY (manifest_id) REFERENCES table_manifests(manifest_id)
);

CREATE INDEX idx_tav_snapshot ON table_asset_versions (snapshot_id);

CREATE TABLE table_checkout_leases (
    lease_id         TEXT PRIMARY KEY,
    asset_id         TEXT NOT NULL,
    base_version     INTEGER NOT NULL,
    work_schema      TEXT NOT NULL,
    work_table       TEXT NOT NULL,
    state            TEXT NOT NULL CHECK (state IN ('active', 'consumed', 'abandoned')),
    created_at       TEXT NOT NULL,
    closed_at        TEXT,
    consumed_version INTEGER
);

CREATE UNIQUE INDEX ux_table_active_lease
    ON table_checkout_leases (asset_id)
    WHERE state = 'active';

CREATE UNIQUE INDEX ux_table_active_work
    ON table_checkout_leases (work_schema, work_table)
    WHERE state = 'active';
```

### 5.1 与 `analytics_resources` 的关系

- `analytics_resources.id` 就是稳定 `asset_id`；**一个资产只有一条活跃登记行**，版本不再靠新 id 行表示。
- 当前版本：`analytics_resources.version`；对应快照由 `table_asset_versions(asset_id, version)` 查。
- `analytics_table` 只表示**当前快照的物理表定位**，由归档事务同步更新；它不再指向用户活表，也不作为版本权威。
- 表资产不写 `analytics_resource_versions`；表历史只来自 `table_asset_versions`。
- 全局表资产同样一条活跃行一个 `asset_id`，不存在「一版一行」或按 name 归并的旧形态。

## 6. manifest 与指纹

### 6.1 schema_hash

- 对 `PRAGMA table_info` / DuckDB `DESCRIBE` 的列定义做规范化：列名、类型、是否可空、默认值、ordinal 顺序。
- 规范化 JSON 后 `sha256`，得到 `schema_hash`。

### 6.2 data_fingerprint

表没有天然行序，因此用**顺序无关**的快速指纹：

```text
per_row = hash(规范化行值)        -- DuckDB 64 位 hash
data_fingerprint = sha256(
    count(rows) || sum(per_row as 128bit) || bit_xor(per_row) || min(per_row) || max(per_row)
)
```

- 只用于变更检测与 manifest 身份，**不冒充加密哈希**；精确差异仍然直接比较两个快照。
- 行序不同但内容相同 → 指纹相同（表本身没有行序语义，这是有意的）。
- 空表：count=0，sum/xor=0，仍生成合法指纹。
- 实现前先用一条 DuckDB smoke SQL 验证 `hash` / `bit_xor` 聚合在 DuckDB 2.0 可用；不可用则退化为「有序导出流式 sha256」，并在文档标注代价。

### 6.3 manifest_id

```text
manifest_id = sha256(canonical({
  schema_hash,
  data_fingerprint,
  row_count,
  column_count
}))
```

- 同 schema、同数据的两份物理快照共享 manifest_id；项目与全局可据此判断“内容一致”。
- `manifest_id` 不包含物理表名、snapshot_id、创建时间，避免同一内容因实例不同而分裂。

## 7. 操作流程

### 7.1 存为表（普通持久表，不归档）

- 结果集 / 临时对象 → `CREATE TABLE analytics.<name> AS ...`。
- 只建普通表，不自动登记资产，不产生 `table_asset_versions`。
- 用户随后可以显式「归档为表资产」。

### 7.2 归档表资产

输入：项目 `analytics` 里的既有表，或查询结果临时表。

1. 校验源表存在；读取 schema，生成目标 `snapshot_id = snap_<uuid>` 与物理名 `arc_<uuid12>`。
2. `CREATE TABLE rds_archive.arc_xxx AS SELECT * FROM <source>`（同库 CTAS；源表此时仍在）。
3. 对快照计算 `schema_hash` / `data_fingerprint` / `manifest_id` / 行数列数。
4. 一个 SQLite 事务内：插入 `table_manifests`、`table_snapshots`、`table_asset_versions`、更新 head 登记行（首版）或追加版本。
5. 事务成功后再消费源表：`DROP TABLE <source>`；失败只记 note，不回滚归档。
6. 崩溃语义：物理快照先于元数据落地，元数据失败 → 快照成为孤儿，由 GC 回收；元数据成功、源表未删 → 源表是游离副本，资产仍只读，修复任务提示清理。

> 为什么先复制后删源：SQLite 与 DuckDB 无共同事务。先 CTAS 再提交元数据，崩溃最多留下孤儿快照，不会出现「登记指向不存在的表」。
> 代价是归档瞬间需要 2 倍空间；超大表后续可评估同库 `ALTER TABLE ... SET SCHEMA` 的移动优化，但不放进第一版。

### 7.3 查看 / 只读打开

- head -> `table_asset_versions` -> `snapshot` -> 物理表；只读 SQL / 结果网格。
- 快照 `state='missing'` 或物理表不存在：明确报「该版本快照已缺失/被裁剪」，不拿当前版本冒充。
- 全局表资产通过短命只读挂载查看；项目引用通过 `origin_ref` 的 `global_asset_id + version` 找全局快照。

### 7.4 checkout（表取回为工作表）

1. 先查活跃租约；已有 active 直接拒绝，提示打开已有工作表或 fork。
2. `CREATE TABLE rds_work.wrk_xxx AS SELECT * FROM rds_archive.arc_xxx`。
3. 插入 `table_checkout_leases`（`base_version` = 当前版本，state=active）。
4. 租约写失败 → 删除刚建的工作表，返回错误。
5. 返回工作表名、base 版本、快照指纹；UI 详情显示「已取回：工作表 …（基准 vN）」。

### 7.5 再归档（消费租约）

1. 按 `asset_id` 找 active lease；没有租约不得给已有资产追加版本。
2. 校验 `base_version` = 租约基准；工作表存在。
3. 对工作表生成新快照（同 7.2 的 CTAS + 指纹）。
4. 若 `manifest_id` 与基准版本相同：幂等，不追加版本；消费租约、删工作表，返回当前版本。
5. 否则事务内追加 `(asset_id, version+1)`，`parent_version = base_version`；消费租约、删工作表。
6. 失败：保留工作表与租约，用户可修复后重试；不产生半版本。

### 7.6 恢复历史版本

- 只追加版本行，快照共享：`v4 -> snapshot(v1)`，不复制数据。
- `parent_version` 记当前 head 版本；head 前移，旧版本行与旧快照保留。
- 若目标快照已 missing，明确拒绝；不把“当前内容”当作旧版。
- 因为快照不可变，“多个版本共享同一物理表”是合法状态；GC 不能删仍被任何版本引用的快照。

### 7.7 fork

- 新建 `asset_id`，v1 直接引用当前快照（同实例零复制；跨实例复制一份物理表 + 同 manifest）。
- `asset_forks` 记 `parent_asset_id / parent_version / child_asset_id / child_version / created_at`。
- 新资产有独立 head、独立版本链、独立 lease；原资产 head 不动。

### 7.8 全局发布

- 输入：项目表资产的某个版本 + `base_global_version`。
- 全局 head 不存在 → 新建全局资产 v1（`asset_id` 新生成）。
- base 匹配 → 复制快照到全局 `rds_archive`，追加全局版本；manifest 若同内容则复用。
- base 落后 → `StaleBase`，只允许 fork 成新全局资产；不覆盖 head。
- 分叉面：详情 / 发布失败弹「发布基准已过期」，填新名后以父 head 零复制建 v1；父 head 不动。
- 全局发布后，项目侧仍保留自己的快照与版本，不共享物理表。

### 7.9 分发 / 引用

- **分发**：全局快照复制到项目 `rds_archive`，新建项目资产 v1（独立链）；用于“拿走一份独立数据”。
- **引用**：项目登记 `table_ref`，`origin_ref = {global_asset_id, version}`；不复制数据，读全局快照。
- 引用 pin 住 `(global_asset_id, version)`；GC 跳过被 pin 的快照版本。
- 固化引用版本 = 把该全局快照复制成项目独立资产 v1，并释放 pin。

### 7.10 回收站 / 永久删除 / GC

- 回收站：只软删登记行；快照、版本、lease 全留。
- 永久删除：删除版本行、标签、引用登记；物理快照交给 GC，不在删除事务里 `DROP`。
- GC 两条：
  1. **元数据 GC**：没有 head、没有版本、没有活跃引用、没有 lease 的资产，清残留登记。
  2. **物理 GC**：`rds_archive` 中没有任何 `table_snapshots` 行引用的物理表，超过 grace 后 `DROP`；`state='missing'` 的快照只清元数据。
- GC 必须可重入、可断点；只删“无引用”对象，任何不确定都保留。

## 8. 保留策略

- 表资产与文件资产不同：表版本默认受保留策略约束（体积大），不是全量保留。
- 沿用现有 `KeepVersions` 语义：
  - `All`：全留。
  - `Keep(n)`：保留当前 head + 最近 n 份历史版本行。
  - `MetadataOnly`：只保留当前 head 与受保护版本行，其余版本行删除；物理快照交给 GC 按引用回收。
- 永不裁的四类：
  1. 当前 head 版本；
  2. 被 `analytics_global_refs` 活跃 pin 的全局版本；
  3. 活跃 lease 的 `base_version`（直到 consumed / abandoned）；
  4. fork 来源版本（至少保留到 fork 资产存在）。
- 裁剪只删版本行引用；物理表是否回收由 GC 按引用计数决定。多个版本共享快照时，删一个版本不会删物理表。

## 9. 稳定身份与引用

- `asset_id` 永不变；显示名可改，名称唯一只作为 UX 约束（同一 scope 内活跃 head 不重名）。
- 物理表名不含显示名；rename 只更新登记行。
- `analytics_global_refs` 直接按 `global_asset_id + global_version` 登记与 pin；不再有 `global_name` 引用列，也不存在按 name 回填的路径。
- `parent_resource_id` 在表资产中不承载版本链；版本链只看 `parent_version`。

## 10. 落地顺序（无迁移）

### 刀 0：替换 schema 与模型

- 新增 `table_manifests` / `table_snapshots` / `table_asset_versions` / `table_checkout_leases`（项目库 + 全局库同构）。
- `table_asset_versions` 的 `asset_id` 直接引用 `analytics_resources(id)`；表资产只保留一条活跃登记行。
- 删除旧表资产的 JSON 快照写入与「活表指针」语义；`analytics_table` 只作为当前快照物理定位。
- 重建 `kind` 的 CHECK，允许表型写真实落库值；删除 `db_write_str` 折回 `analysis` 的兼容逻辑。
- 不建立旧数据回填任务；旧表资产视为不存在，新表资产从 v1 开始。

### 刀 1：快照 store 与纯模型

- `table_asset/manifest.rs`：schema 规范化、`manifest_id`、数据指纹。
- `store/table_asset.rs`：manifest / snapshot / version / lease 的增删查；同资产唯一 active lease。
- 纯单元测试：manifest 稳定、指纹变更、版本共享、租约唯一、引用计数查询。

### 刀 2：归档 / checkout / 再归档 / 恢复

- `service/table_asset.rs`：建立快照、消费源表、checkout 工作表、再归档消费 lease、恢复共享快照。
- 崩溃语义按 7.2：快照先落地，元数据后提交；孤儿快照交给 GC。
- 删除 `table_asset::register` 的活表登记实现，调用方改到新 API。
- `存为表` 改为普通表持久化，不再自动登记表资产。

### 刀 3：全局发布 / fork / 分发 / 引用

- `global/promote.rs`：按 `base_version` 追加全局版本；冲突只 fork。
- `global/refs.rs` / `global/distribute.rs`：引用按 `global_asset_id + version`；分发复制快照为项目资产 v1。
- `analytics_global_refs` 直接新表结构，不保留 name 引用列。

### 刀 4：只读守卫、保留与 GC

- SQL 闸门拒绝 `rds_archive` 的 DDL/DML；表枚举过滤 `rds_archive` / `rds_work`。
- `KeepVersions` 作用于表版本；当前 / pin / lease / fork 来源不裁。
- `jobs/table_gc.rs`：清理孤儿物理表与无引用快照。

### 刀 5：UI 与对比

- 详情 / 取回对话框显示「已取回：工作表 …（基准 vN）」。
- 版本历史从 `table_asset_versions` 读；对比先做 schema + 行数，再做行级 diff。
- 资产列表不再展示旧「活表」行；`存为表` 与「归档为资产」两个入口分开。

> 旧代码删除与调用方切换在同一刀内完成；**不保留 feature flag、不保留双读分支、不保留旧数据兼容。**

## 11. 验收用例

1. 归档一张表后，源表被消费；资产只读，物理表在 `rds_archive`。
2. 归档后另建同名活表并修改数据：资产内容、指纹、版本全不变。
3. checkout 后修改工作表再归档 → 版本 +1，`parent_version` 正确；工作表消失。
4. 无租约给已有资产追加版本 → 拒绝。
5. 恢复 v1 → 生成 v3，v3 与 v1 共享同一 snapshot，不复制数据；v2 仍可读。
6. 两个资产同名不同 id：互不影响；rename 不改物理表名。
7. 全局发布 base 落后 → Conflict，只能 fork；fork 新资产 v1 引用同一 snapshot。
8. 项目引用 pin 住全局 v2：裁剪跳过 v2；释放 pin 后 GC 可回收快照。
9. 保留策略 `MetadataOnly`：除 head 与受保护版本外版本行被删，快照由 GC 回收，恢复明确报错。
10. `INSERT/UPDATE/DELETE` 指向 `rds_archive` 被 SQL 闸门拒绝；`SELECT` 放行。
11. 归档中途崩溃：不存在“live 版本指向缺失快照”；孤儿物理表可被 GC。
12. 新库 schema 不包含旧「活表指针 / 一版一行 / name 引用」写入路径；表型 `kind` 直接写真实落库值。
13. rename 只改显示名：物理快照、版本、引用、pin 全部不变；同名只做显示层查重。
14. 两个版本共享快照时删其中一个版本，物理表不消失；删最后一个版本后 GC 才回收。

## 12. 实施位置映射（计划）

| 设计决策 | 计划落点 | 状态 |
| --- | --- | --- |
| 快照 / manifest / 版本 / 租约表 | `crates/store/migrations/project_meta/038_table_asset_snapshots.sql`、`crates/store/migrations/global/041_table_asset_snapshots.sql`（schema 重置，不做数据迁移） | 已实现 |
| 表快照 manifest 与指纹 | `crates/analytics_resource/src/table_asset/manifest.rs`（新模块） | 已实现 |
| 表版本 / 租约 store | `crates/analytics_resource/src/store/table_asset.rs`（新模块） | 已实现 |
| 归档 / checkout / 再归档 / 恢复 | `crates/analytics_resource/src/service/table_asset.rs`（新模块） | 已实现 |
| 归档 schema 只读守卫 | `crates/sql/src/guard.rs` + `crates/engine/src/duckdb/instance.rs::guard_writes`；表枚举 `workbench/source_picker` 过滤 `arc_/wrk_` | 已实现 |
| 全局发布 / fork / 分发 / 引用 | `global/promote.rs`、`global/table_asset.rs`、`dialogs/stale_base.rs`、`panels/resources.rs` | 表型升级、`base_global_version` 校验、`StaleBase` → 分叉对话框、零复制恢复、分发与引用检查已接快照；引用按 `global_asset_id + version` 登记 |
| 保留与 GC | `global/sql.rs`、`global/delete.rs`、`table_asset/gc.rs`、`global/gc.rs`、`jobs/table_gc.rs` | 裁剪 / purge 只清引用行；后台 GC 按 24h grace 回收无引用快照 |
| 删除旧表资产语义 | `table_asset.rs`（旧 `register` / `table_fingerprint`）、`global/promote.rs` 旧表链、`analytics_global_refs` 旧 name 列 | 已删除；全局表/数据文件均走快照，引用按 asset_id |
| `存为表` 与归档拆分 | `crates/workbench/src/services/persist_jobs.rs`、数据对象面板入口 | 已实现 |
| 版本历史 / 恢复入口 | `present/detail.rs`、`present/snapshot.rs`、`detail_view/actions.rs`、`detail_actions.rs` | 全局表版本链走 `table_asset_versions`、全局文件链走 `file_asset_versions`，都内联详情；时间线选中版可恢复为任意历史版本 |
| 版本对比（schema / 行） | `table_asset/diff.rs`（新模块）、`jobs/version.rs`、`dialogs/version_diff.rs`、`dialogs/version/render.rs` | 项目侧版本对话框已放开表型对比：schema 列变化 + 有界行级差异（`EXCEPT ALL`，每侧最多 200 行，另算真实差异行数）；文件仍走文本 diff。全局表 / 全局文件暂无独立版本对话框，只提供详情时间线与任意版本恢复 |

> 本文是设计定稿，上表是落码映射（状态列区分已实现与仍留）。落地时直接替换旧表资产路径，**不建立旧数据迁移、不保留双读双写或 feature flag**；每刀完成后同步 `README.md` 与 `file-asset-cas-implementation-plan.md` 的边界说明。

### 12.1 全局侧落地明细

- 全局表资产登记：一条活跃 `kind='analysis'` 行持稳定 `arg_`，版本链在 `table_asset_versions`，物理快照在全局 `rds_archive.arc_*`（`global/table_asset.rs`）。
- 升级：`promote_table_from_project` 支持 `schema.table` 源（项目表资产传 `rds_archive.arc_*`）；数据文件走 `promote_file_as_table_asset`，同样冻结成全局快照；同 manifest 复用全局快照，新内容才追加版本。
- 旧路径已删除：`promote` / `promote_overwrite` / `register_global`、`read_global_row` 的单行回退、`prune_legacy_version_tables` 全部移除。
- 恢复：`restore_global_table_version` 新增版本直接指向旧 `snapshot_id`，不复制数据。
- 分发 / 引用：按 `physical_schema` 拼三部分限定名；表存在性检查按 schema。
- 回收站：trash 只软删登记，快照保留；purge 只清版本 / fork / 引用索引，物理快照与 manifest 留给 GC。
- 引用：按 `global_asset_id + version` 登记；显示名可改，固化按 asset_id 解析当前名。
- 验证：`rds-analytics-resource` 全量测试（见 `.rds/tmp/analytics-test.log` 及后续复跑）；`rds-workbench` / `rds-sql` / `rds-engine` 编译与相关测试见各轮日志。

已接通：全局发布 `base_global_version` → `StaleBase` → 分叉对话框；`rds_archive` SQL 只读闸门；`arc_/wrk_` 枚举过滤；独立表快照 GC；全局表 / 全局文件版本链内联详情与任意历史版本恢复；项目表版本对比（schema + 行级 diff）。

仍留：全局表 / 全局文件没有独立版本历史对话框（按 §13.4 的入口收敛，只在详情时间线看最近版本）；表 diff 的行级差异对超大表仍是全表 `EXCEPT ALL`，未加超时 / 采样阈值。

### 12.2 版本对比与全局文件历史（2026-11-27 收口）

- **表版本对比**：`table_asset/diff.rs::table_version_diff` 先解析两版 `TableManifest.schema_json`，比较列新增 / 删除 / 类型·可空·默认值变化；仅当两侧列名与类型可逐列对齐时，才用 `SELECT … EXCEPT ALL SELECT …` 取旧版独有 / 新版独有，单侧最多列出 `ROW_DIFF_LIMIT = 200` 行，另用 `count(*)` 得到真实差异行数。快照 `Missing` 或物理表不存在会明确报错，不静默当成“无差异”；同 manifest 直接拒绝，不伪造 patch。
- **表版本对比接线**：`jobs/version.rs::load_version_diff` 按 `ArchiveKind` 分流。表走 `table_asset_versions` → 两版 manifest / snapshot，并通过 `open_service_with_manager` 保活 `ProjectDatabaseManager`，借项目 DuckDB 连接读快照；文件沿用原 manifest 文本 + `unified_patch`。`dialogs/version_diff.rs` 增加 `VersionDiffKind` 与摘要行，`dialogs/version/render.rs` 去掉表型禁对比的判断。
- **全局文件版本历史**：`global/file.rs::global_file_versions` 读 `file_asset_versions` 全链（升序）；刷新时 `load_global_view` 同时取全局表与全局文件两套版本索引，`present::DetailExtras.file_versions` 只对全局文件行填实际链；详情时间线按版本降序取最近 3 条，`content_pruned` 决定节点是否可还原，「可还原副本」由未裁剪版本组成。
