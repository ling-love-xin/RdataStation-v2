# 文件资产 CAS 版本模型设计（v2）

> 状态：设计定稿，2026-11-27。
> 关系：本文取代 `global-file-asset-design.md` 中“文件资产”的部分；项目文件与全局文件统一到同一套对象模型。
> 不改代码，只定义目标模型、迁移路径与验收口径。

## 0. 决策记录

| 编号 | 决策 | 说明 |
| --- | --- | --- |
| D1 | **全局文件 file-first** | 全局文件资产 `kind=file`；派生表是另一条 `kind=table` 资产，文件行不写 `analytics_table` |
| D2 | **同一资产同一项目只允许一个活跃 checkout** | 取回是一份带 `base_version` 的租约；第二个取回要么打开已有草稿，要么显式 fork |
| D3 | **全局并发修改只 fork，不静默覆盖** | 发布必须带 `base_global_version`；base 落后于 head 时只能 fork / 取消，不能覆盖 |
| D4 | **文件版本默认全保留** | 文件资产不自动裁剪；裁剪是显式动作，且要过引用/租约/派生表检查 |
| D5 | **二进制文件支持全局** | CAS 存任意字节；二进制全局文件只读打开/取回/分发，不建表、不建内容表 |

### D2 里的“活跃 checkout”是什么

`checkout` = 取回一份可写工作副本；`lease`（租约/取回凭据）是一条记录：

```text
lease_id, asset_id, base_version, dest_rel, state, created_at, closed_at, consumed_version
```

`state=active` 表示“已经取回、但还没再归档，也没放弃”。它不是 OS 文件锁，作用是把“这份草稿从哪一版来、改完该回到哪条版本链”钉住。

同一资产同一项目只允许一个 active lease，是为了避免：

- 两份草稿都从 v3 取回，先后再归档，第二份不知道父版本是谁；
- 两份草稿各自改，最后只能二选一，另一份工作丢失；
- 版本链出现两个“v4”，无法判断哪个是主线。

只读打开不产生 lease；需要第二份可写副本时，显式执行“fork 为新资产”，新资产有自己的版本链和 lease。

---

## 1. 目标与非目标

### 1.1 目标

1. 文件内容按字节寻址：`content_hash = sha256(文件字节)`，任何字节变化都能产生新版本。
2. 版本默认全保留；历史版本不依赖“完整副本目录”，而由 manifest + CAS 对象还原。
3. 归档消费草稿：归档成功后草稿不再可写；修改只能走 checkout。
4. 取回、再归档、还原、fork 都有明确的父版本和冲突规则。
5. 项目/全局同一套文件模型；全局文件是文件，表只是可选投影。
6. 任意文件类型（含二进制）可归档、可升级全局、可只读打开、可取回。
7. 旧数据可迁移；迁移不伪造不存在的文件，不把“表型（无原文件）”伪装成可恢复文件。

### 1.2 非目标

- 不做 Git 的文本 merge / rebase / cherry-pick；二进制不做自动合并。
- 不把资产库做成通用代码仓库；不做整仓 checkout。
- 不把文件内容存进 SQLite BLOB；SQLite 只存元数据。
- 不做文件级“引用后置”的复杂 pin；v2 先保证打开、取回、发布、分发。
- 不自动删除历史版本；D4 默认全保留。
- 不把旧全局表资产伪造出原文件；没有字节就明确标“表型（无原文件）”。

---

## 2. 核心模型

```text
CAS 对象库
  ├─ blob     小文件整块：sha256(bytes)
  └─ chunk    大文件分块：sha256(chunk)
        │
        ▼
Manifest（一次版本的文件清单）
  entries[] = { path, blob_or_chunks, size, hash, mode }
  root_hash = sha256(canonical(manifest))
        │
        ▼
Version（不可变版本）
  asset_id, version, parent_version, manifest_id, content_hash, schema_hash
        │
        ▼
Ref（当前头）
  (scope, asset_id) -> version
        │
        ▼
Lease（取回凭据）
  asset_id, base_version, dest_rel, state
```

### 2.1 维度拆分

不要再让一个 `kind` 同时回答“它是什么、怎么查看、能不能进 SQL、算不算表”。拆成：

| 维度 | 取值 | 用途 |
| --- | --- | --- |
| `asset_kind` | `file` / `table` / `table_ref` | 生命周期、版本、回收站、全局发布 |
| `content_class` | `tabular` / `text` / `binary` | 查看器、对比器、全局能力 |
| `duckdb_readable` | bool | 能否用 DuckDB 直接读成表 |
| `view_mode` | `grid_sql` / `editor` / `binary_info` | UI 路由 |
| 身份 | `asset_id + version` | 版本链、lease、fork、全局引用 |
| 显示名 | `name` | 仅展示，可重复；禁止按名字做身份或清链 |

`content_class` 在归档时确定：

- 探测成功且能读成表 → `tabular` + `duckdb_readable=true`；
- 不能读成表但字节是合法文本 → `text`；
- 其余 → `binary`。

`duckdb_readable` 是能力事实，不是档位；探测失败不拒绝归档，只降级为 `text/binary`。

### 2.2 文件与表的关系

- 文件资产：`asset_kind=file`，本体是 CAS 中的字节；可以有 0 或 1 个派生表。
- 派生表：`asset_kind=table`，`origin_kind=file_projection`，`origin_ref={asset_id,version}`；表是文件的投影，不是文件身份。
- 纯表资产：`asset_kind=table`，本体是项目/全局分析库中的表；没有文件本体，也不承诺文件恢复。
- `table_ref` 保持独立；不参与文件版本链。

**硬规则**：文件行的 `analytics_table` 必须为空；`ArchiveKind::resolve` 不得因为某列非空把文件判成表。表行显式 `asset_kind=table`。

---

## 3. 存储布局

### 3.1 项目侧

```text
<项目>/
├── scratchpad/                          # 唯一可写草稿区
└── .RSmeta/
    ├── project.db                       # 元数据：asset / version / lease / ref
    ├── cas/
    │   ├── blobs/aa/<sha256>            # 小文件整块
    │   ├── chunks/aa/<sha256>           # 大文件分块
    │   ├── manifests/aa/<manifest_id>.json
    │   └── staging/<uuid>/              # 写入暂存，校验后转正
    ├── resources/<rel>                  # 当前版本的物化缓存（只读，可重建）
    └── trash/                           # 项目回收站
```

### 3.2 全局侧

```text
<RDS_HOME>/data/
├── system/
│   ├── global.db
│   ├── analytics.duckdb
│   ├── cas/
│   │   ├── blobs/aa/<sha256>
│   │   ├── chunks/aa/<sha256>
│   │   ├── manifests/aa/<manifest_id>.json
│   │   └── staging/<uuid>/
│   └── resources/<rel>                  # 全局当前版本物化缓存
└── trash/global/<asset_id>/<stamp>/     # manifest + 可选 payload 缓存
```

### 3.3 真相源

- **CAS 是唯一真相源**：manifest + 对象决定一个版本的内容。
- `resources/<rel>` 是物化缓存，供外部工具/只读打开使用；丢失可由 manifest 重建。
- 旧 `versions/<asset_id>/vN/` 目录只作为迁移输入；新模型不再为每个版本写完整副本。
- 写入顺序：staging → 校验 hash → 原子 rename；DB 事务失败则对象成为不可达，交给 GC。

---

## 4. 元数据表

沿用现有 `analytics_resources` / `analytics_resource_versions` 作为资产头与版本索引，新增/补列；不把文件内容进库。

### 4.1 `analytics_resources`（资产头）

| 字段 | 说明 |
| --- | --- |
| `id` | 稳定 `asset_id`，不随版本/改名变 |
| `scope` | `project` / `global` |
| `asset_kind` | `file` / `table` / `table_ref`；文件行不得有 `analytics_table` |
| `name` | 显示名，可重复，只作标签 |
| `content_class` | `tabular` / `text` / `binary` |
| `duckdb_readable` | 归档探测结果；能力事实 |
| `current_version` | 当前版本号 |
| `current_manifest_id` | 当前 manifest；表资产为空 |
| `content_hash` | 当前版本字节 sha256（表资产可为表指纹） |
| `schema_hash` | 结构化文件/表的定义+结构指纹；文件可为空 |
| `file_size` | 当前版本字节数 |
| `deleted_at` | 软删；回收站 |
| `created_at` / `updated_at` | 时间 |

### 4.2 `analytics_resource_versions`（版本）

| 字段 | 说明 |
| --- | --- |
| `asset_id` | 稳定资产 id |
| `version` | 每资产递增；不可复用 |
| `parent_version` | 直接父版本；v1 为空 |
| `manifest_id` | 文件版本必填；表版本为空 |
| `content_hash` | 文件 = 字节 sha256；表 = 表指纹 |
| `schema_hash` | 结构指纹，可空 |
| `file_size` | 字节数 |
| `restored_from_version` | 从旧版还原时记录 |
| `content_pruned` | 显式裁剪后置 true；元数据保留，内容不可还原 |
| `snapshot` | 兼容现有 JSON 快照；新代码以列为主 |
| `created_at` | 时间 |

### 4.3 新表

```sql
file_manifests(
  manifest_id TEXT PRIMARY KEY,
  root_hash TEXT NOT NULL,
  entries_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);

checkout_leases(
  lease_id TEXT PRIMARY KEY,
  asset_id TEXT NOT NULL,
  base_scope TEXT NOT NULL,          -- project | global
  base_version INTEGER NOT NULL,
  dest_rel TEXT NOT NULL,
  state TEXT NOT NULL,               -- active | consumed | abandoned
  created_at TEXT NOT NULL,
  closed_at TEXT,
  consumed_version INTEGER
);
CREATE UNIQUE INDEX ux_active_lease
  ON checkout_leases(asset_id, base_scope, state)
  WHERE state = 'active';

asset_forks(
  fork_id TEXT PRIMARY KEY,
  parent_asset_id TEXT NOT NULL,
  parent_scope TEXT NOT NULL,
  parent_version INTEGER NOT NULL,
  child_asset_id TEXT NOT NULL,
  child_scope TEXT NOT NULL,
  created_at TEXT NOT NULL
);

global_publish_links(
  link_id TEXT PRIMARY KEY,
  global_asset_id TEXT NOT NULL,
  global_version INTEGER NOT NULL,
  source_asset_id TEXT NOT NULL,
  source_scope TEXT NOT NULL,
  source_version INTEGER NOT NULL,
  published_at TEXT NOT NULL
);
```

- `file_manifests` 只存清单，不存字节；清单条目指向 CAS。
- `checkout_leases` 的唯一索引只约束 `active`；历史记录可多条。
- `asset_forks` 记录显式 fork 的来源；不做同一 asset 的多分支 UI。

### 4.4 表资产

- 表资产继续用 `analytics_table` + 版本快照；可以保留“一版一张表”。
- 表版本不写 `manifest_id`；`content_hash` 可以是表指纹，但不得和文件字节哈希混用。
- 派生表行 `origin_kind=file_projection`、`origin_ref` 指向文件版本；文件版本被裁剪前要检查派生表是否有活跃引用。

---

## 5. 状态机

```text
DRAFT（草稿，可写）
  │ archive
  ▼
ARCHIVED（资产只读，vN）
  │ checkout
  ▼
CHECKED_OUT（草稿工作副本 + active lease）
  │ rearchive
  ├─ 成功 → ARCHIVED（vN+1，lease consumed）
  └─ 失败 / 放弃 → ARCHIVED（lease abandoned）
  │ restore
  ▼
ARCHIVED（vN+1，restored_from）
  │ publish
  ▼
GLOBAL（全局文件 vN）
  │ global checkout
  ▼
PROJECT_CHECKED_OUT（lease base=global:vN）
  ├─ 提交全局 → GLOBAL（vN+1，base 必须等于 head）
  └─ 另存项目 → PROJECT_ARCHIVED（fork / 新资产）
```

不变量：

1. `resources/` 永远只读；唯一可写面是 `scratchpad/`。
2. 文件版本按字节哈希判定；`schema_hash` 只作结构提示。
3. 版本只追加；restore 生成新版本，不改写历史。
4. 一个资产一个项目最多一个 active lease。
5. 没有 active lease 的草稿归档 = 新资产；有 lease = 原资产升版。
6. 全局发布必须带 base；base 落后只允许 fork / 取消。
7. 显示名不是身份；所有清理按 `asset_id`，不按 `name`。
8. 文件行不得有 `analytics_table`；表行不得伪装文件。

---

## 6. 操作流程

### 6.1 归档（draft → archived v1）

1. 校验源在 `scratchpad/`；项目只读时拒绝。
2. 流式读取，按阈值决定整块或 CDC 分块；每块算 sha256，写入 CAS staging。
3. 生成 manifest（单文件资产只有一条 entry）；`content_hash = sha256(文件字节)`。
4. 可选探测结构：成功写 `content_class=tabular` + `schema_hash`；失败降级 `text/binary`。
5. DB 事务：插入资产头、v1、manifest 引用、归档事件。
6. 从 CAS 物化 `resources/<rel>`，设只读；失败可重试，不回滚已提交版本。
7. 事务成功后删除草稿；删除失败记 `pending_cleanup`，重试清理，**不回滚归档**。
8. 若同名资产已存在：默认拒绝并提示；要升版必须走 checkout lease，不能凭名字直接接到旧链。

### 6.2 取回（archived → checked_out）

1. 校验无 active lease；有则返回“已有取回草稿”，给“打开已有 / 放弃后重取 / fork”三条路。
2. 解析 manifest；从 CAS 流式物化到 `scratchpad/` 目标路径；每个对象校验 hash。
3. 写 `checkout_leases`（active，base_version=当前版本）。
4. 资产库不动、版本不动；打开草稿按 `content_class` 路由。
5. 取回失败：删掉未完成的目标文件，lease 不写或写 abandoned。

### 6.3 再归档（checked_out → archived vN+1）

1. 按草稿路径找 active lease；找不到则按新资产处理（并提示“没有取回凭据，将新建资产”）。
2. 计算新 manifest；若 `content_hash` 与 base 相同：不建版本，lease 保持 active（或用户显式结束）。
3. 读当前资产 head：
   - `head == base`：正常追加 v+1，`parent=base`。
   - `head != base`：冲突。项目侧默认拒绝；用户可显式“fork 为新资产”或“以当前 head 为父强制追加”（后者要二次确认，不是默认）。
4. CAS 对象/manifest 先落；DB 事务写版本、更新 head、lease consumed；失败不改 head。
5. 物化当前 `resources`；失败可重试。
6. 删除草稿；清理失败留 pending cleanup。

### 6.4 还原（archived → archived vN+1）

1. 校验目标版本存在且 `content_pruned=false`。
2. 从目标 manifest 物化到 staging；校验 hash。
3. 当前版本内容已在 CAS 中（无需“留副本”）；写新版本 `parent=current`、`restored_from=target`。
4. 原子替换当前物化缓存；旧版本不动。
5. 若目标版本被裁剪：明确失败，不用当前内容冒充。

### 6.5 版本对比

- 任意两版，不是“只能和当前比”。
- 文件：先比 manifest（路径/哈希/大小）；不同 entry 再按 `content_class` 分：
  - `text`：UTF-8 unified diff；
  - `tabular`：schema diff + 行数/列数 + 可选行级 diff（异步任务）；
  - `binary`：只给 hash/大小/时间，不做文本 diff。
- 表：表结构 + 表版本/表名对比。
- 对比只读，不产生版本。

### 6.6 全局发布（project file → global file）

1. 前置：项目文件已归档、当前版本可读、项目非只读、无 active lease。
2. 计算全局目标名；同名全局资产存在时：
   - 名字空闲：首次发布，创建 global v1；
   - 名字已存在且带 `base_global_version`：base 必须等于 global head，追加 v+1；
   - 名字已存在但没带 base，或 base 不等于 head：`Conflict`，只给 fork / 取消，不给覆盖。
3. 将 manifest/对象提升到全局 CAS（同哈希对象可复用，不重复搬字节）；项目资产保留。
4. 写全局资产头 + global version + `global_publish_links`（source asset/version）。
5. 按需创建派生表（独立表资产，`origin_ref` 指向文件版本）；建表失败不回滚文件发布。
6. 全局文件 `kind=file`，`analytics_table` 为空；二进制不建表。

### 6.7 全局取回 / fork

- **只读打开**：不建 lease，从全局 CAS 流式读，按 `content_class` 路由。
- **全局取回**：物化到项目 `scratchpad/`，写 lease（`base_scope=global`、`base_version=global:vN`）。
- **再归档**：
  - 选择“提交全局”：base == global head → 新全局版本；否则 fork。
  - 选择“另存项目”：创建项目资产（`origin_kind=global`、`origin_ref`），走项目版本链；这是 fork，不是覆盖。
- **分发**：创建只读项目资产副本（用于离线/引用），不再提供“解锁并销毁全局”的动作。

### 6.8 删除 / 回收站

- `trash`：软删资产头；文件入项目/全局回收站；版本元数据保留。
- `restore`：恢复登记；文件从 CAS 物化；校验 hash。
- `purge`：显式永久删除；清版本元数据、lease 历史、fork 链接；GC 回收不可达对象。
- 全局资产有 active lease 时：`trash` 可只做隐藏并警告；`purge` 必须拒绝，先结束/放弃所有项目侧 lease。
- 项目资产删除不影响已发布的全局版本；`global_publish_links` 保留来源记录，来源可标记为已删除。
- 文件资产的“删除”不再自动走“解锁退回草稿箱”。要取回请用 checkout；要移出请用独立动作。

### 6.9 服务 API 契约

```text
archive_file(project, draft_rel, name) -> { asset_id, version, manifest_id, content_hash }
checkout_file(asset_id, version, dest_rel) -> { lease_id, base_version, content_hash }
rearchive_file(lease_id, draft_rel) -> { version, parent_version, manifest_id } | Conflict
abandon_checkout(lease_id) -> ()
restore_file(asset_id, target_version) -> { new_version, restored_from }
compare_versions(asset_id, left_version, right_version) -> { manifest_diff, typed_diff }
publish_global_file(asset_id, version, global_name, base_global_version?) -> { global_asset_id, global_version } | Conflict
checkout_global_file(global_asset_id, version, dest_rel) -> { lease_id, base_scope:"global", base_version }
fork_global_file(global_asset_id, version, fork_name) -> { new_global_asset_id, version }
trash_file(asset_id) -> ()
restore_file_from_trash(asset_id) -> ()
purge_file(asset_id) -> ()
```

约定：

- 所有写操作返回 `Conflict` 时只说明冲突，不自动覆盖；调用方必须选择 fork / 取消 / 显式强制。
- `archive_file` 成功即消费草稿；草稿删除失败只记 `pending_cleanup`，不回滚版本。
- `checkout_file` 不改变资产头/版本；只写 lease 与物化副本。
- `publish_global_file` 不删除项目资产；项目资产仍按项目版本链存在。

---

## 7. 版本、保留与 GC

### 7.1 默认全保留

- 文件资产：所有版本的 manifest 与对象默认保留；不自动裁剪。
- 全局文件：同上。
- 显式裁剪：用户选择版本 → 检查 active lease、派生表引用、全局 fork/publish 链接、回收站 → 清 manifest 引用与对象；版本行保留，`content_pruned=true`。
- 表资产：沿用表版本策略；派生表如果只是文件投影，可由文件重建，表版本可单独裁剪。

### 7.2 去重与压缩

- 小文件整块 blob；大文件 CDC 分块（Rabin/FastCDC），平均块大小按文件大小自适应。
- 块级 zstd 压缩；同哈希块跨版本、跨资产复用。
- 第一版可以只做整文件 CAS；CDC 作为第二阶段优化，不阻塞正确性。

### 7.3 GC

- 根集合：所有资产当前版本、所有保留版本、active lease、全局发布链接、回收站 manifest、派生表 origin。
- mark-sweep；只删不可达对象；绝不按时间扫目录。
- 写入失败留下的 staging/orphan 对象由 GC 清理；清理前校验无引用。

---

## 8. 查看与 SQL

| `content_class` | 默认查看 | SQL |
| --- | --- | --- |
| `tabular` + `duckdb_readable` | 只读表格（结果网格） | 只读 SQL 视图；会话级 `asset_<id>`，不物化 |
| `text` | 只读通用编辑器 | 否 |
| `binary` | 元信息 + 只读打开/下载 | 否 |

- 结构化文件不再“默认建表”：查看时用 `read_csv/read_parquet/...` 或会话 view；需要持久表时显式“物化为表资产”。
- SQL 视图只读；不允许 DDL/DML 指向归档资产。
- 全局文件只读；取回后才可写。
- `view_mode` 由 `content_class + duckdb_readable` 推导，不在 DB 里另存一份可漂移的状态。

---

## 9. 全局 file-first 的边界

- 全局文件行：`asset_kind=file`，`content_class` 任意，`analytics_table=NULL`。
- 派生表行：`asset_kind=table`，`origin_kind=file_projection`，`origin_ref={global_file_id,version}`。
- 二进制全局文件：只读打开、取回、分发；不建表、不建内容表。
- 全局文件版本默认全保留；全局表版本沿用表策略。
- 全局并发：base 校验 + fork；没有“覆盖式发布”的默认入口。
- 旧全局 `kind=analysis` + 文件混合行：迁移时拆成文件资产 + 可选派生表；没有原字节的旧行保持表型，不伪造文件。

---

## 10. 迁移路线

### 阶段 0：加模型，不改读写

- 迁移加 `asset_kind` / `content_class` / `duckdb_readable` / `current_manifest_id` / `manifest_id` / `parent_version` / `content_pruned`。
- 新表 `file_manifests` / `checkout_leases` / `asset_forks`。
- 旧 `content_hash` 保持；新代码双写 `content_hash_bytes`（或迁移后统一改名）。

### 阶段 1：新归档走 CAS

- 新文件归档写 CAS + manifest；仍物化 `resources/`。
- 旧 `versions/` 不再新增；M5 起旧副本读写代码与回退路径一并删除，已有目录不再作为读路径。
- `checkout` 从 manifest 物化；旧资产没有 manifest 时按「旧方案已删除」拒绝取回，要求先迁移或重新归档。

### 阶段 2：回填

- 对每个文件资产：当前本体 → blob/chunk；每个历史副本 → manifest；校验 sha256。
- 只有元数据、没有副本的版本：`content_pruned=true`，不伪造。
- 回填任务可断点续跑；失败不影响旧读路径。

### 阶段 3：切换读路径

- 打开/取回/还原只走 CAS；CAS 缺对象直接报错并标 `content_pruned`，不再回退旧副本。
- 回填完成后旧 `versions/` 直接停用并删除，不做只读过渡期，也不作为任何回退路径。

### 阶段 4：全局拆分

- 全局 `analysis` + 文件混合行 → 文件资产 + 派生表；脚本内容表 → 文件资产。
- 没有文件字节的全局表行 → 保持表型；UI 标“表型（无原文件）”。
- 同名全局资产冲突：按 id 保留，显示名冲突由 UI 解决；不再按名字清链。

### 阶段 5：删除旧语义

- 删除文件资产不再自动 unlock；`unlock_file_to_drafts` / `unlock_global_file_to_project` 连同旧解锁作业一并删除，不保留兼容入口。
- `KeepVersions` 不再作用于文件资产；文件版本默认全保留。
- 文件行不再写 `analytics_table`；`ArchiveKind::resolve` 只信 `asset_kind`。

---

## 11. 验收用例

1. 归档后草稿不存在；`resources/` 只读；checkout 才产生一份可写副本。
2. 同一 CSV 只改行数据、列结构不变 → `content_hash` 变，产生 v2；不是幂等跳过。
3. 同一资产第二次 checkout 被拒，提示已有 active lease；显式 fork 后可取回。
4. 任意两个历史版本可对比；文本 diff、表格 schema/行 diff、二进制元信息各自走对路径。
5. 恢复 v1 → 生成 v3；v1/v2 保留；不改写历史。
6. 文件 v1 的所有对象在 CAS；删除 `resources` 缓存后可重建。
7. 全局发布 v1（无 base）成功；并发发布 base 落后 → Conflict，只能 fork。
8. 全局 fork 产生新资产，`asset_forks` 记来源；原全局 head 不变。
9. 全局二进制文件发布、只读打开、取回、分发；全程没有 `analytics_table`。
10. 全局结构化文件发布后可只读 SQL 查看；派生表是独立表资产。
11. 全局文件全版本保留；显式裁剪检查 lease/派生表/fork/回收站。
12. 崩溃注入：DB 提交前崩溃 → 草稿保留、无版本；提交后物化前崩溃 → 版本可读、可重建缓存。
13. 旧 `analysis` + 文件混合行迁移后拆成文件 + 表；旧纯表行保持表型且不可文件恢复。
14. 同名两个项目文件资产互不影响；删除/取回按 id，不按 name。
15. `unlock` 不再作为删除文件资产的默认动作；删除只软删登记/进回收站。

---

## 12. 实现位置映射（计划 → 实际）

| 设计决策 | 落点 | 实现状态 |
| --- | --- | --- |
| CAS 对象/清单 | `payload/cas.rs`、`payload/manifest.rs` | 已实现（M1） |
| 版本元数据 | `store/migrations/project_meta/036_file_asset_cas.sql`、`store/migrations/global/040_global_file_asset_cas.sql`、`store/file_asset.rs` | 项目库 + 全局库均已实现 |
| 归档消费草稿 | `service/archive.rs::archive_new` / `archive_into_existing` | 已实现（草稿删除失败只记 note） |
| 字节哈希 vs 结构哈希 | `service/archive.rs`、`analysis.rs` | 已实现：`ArchiveKind::File` 一律字节 sha256，结构只作元数据 |
| checkout lease | `service/checkout.rs`、`store/file_asset.rs`、`jobs/archive/run.rs`、`detail_view/rows.rs`、`dialogs/checkout.rs` | 已实现（项目侧；receipt 仅兼容保留）；刷新快照一次取全量 active 租约，详情与取回对话框显示草稿落点 / 基准版本 |
| 再归档冲突 | `service/archive.rs::archive_into_existing`、`jobs/archive/run.rs::run_archive` | 已实现 base 校验 / lease 消费；全局 fork 已实现，项目侧 fork 未做 |
| 任意两版对比 | `jobs/version.rs::load_version_diff`、`dialogs/version.rs`、`dialogs/version_diff.rs` | 文本两版已实现（Shift+点选基准，按版本号排旧→新，两侧走 manifest）；表格 schema/行 diff、二进制元信息对比未实现 |
| 全局 file-first | `global/file.rs::publish_file`、`global/distribute.rs`、`global/promote.rs`、`global/delete.rs`、`jobs/global.rs::promote_file_first` | 已实现（M4）：文件行 `kind=file`、`analytics_table=NULL`；旧 `analysis + file` 混合行迁移留 M5 |
| 全局 fork | `global/file.rs::fork_file`、`global/040` 的 `asset_forks` | 已实现（M4）：复用 manifest、不移动 head |
| 全局取回 | `global/file.rs::checkout_file` / `consume_checkout_lease`、`jobs/global.rs::run_global_checkout`、`resource_host/detail_actions.rs` | 已实现：全局文件行进草稿箱 + 双侧租约；归档该草稿时结清项目/全局租约 |
| 全局历史恢复 | `global/file.rs::restore_file_version`、`global/promote.rs::restore_global_file_version` | 已实现：只认 manifest；无 manifest 的旧文件版本拒绝恢复（M5 删除旧副本回退） |
| 删除 / unlock 分离 | `service/trash.rs`、`resource_host/delete_actions.rs`、`detail_view/actions.rs`、`resource_view/list_delegate/menu.rs` | 文件删除统一进回收站（项目 / 全局）；unlock 服务、作业与宿主动作全部删除，不再保留显式移出能力（M5） |
| 文件默认全保留 | `service/archive.rs`、`payload/versions.rs`（已删除） | M5 已实现：停止写旧副本、删除副本裁剪模块；`KeepVersions` 只作用于表型 |
| 查看路由 | `present/detail.rs`（`duckdb_readable`）、`resource_view/row_meta.rs`、`resource_host/detail_actions.rs` | 已实现：`structured`/`duckdb_readable` 决定取表格还是编辑器；不再按扩展名猜。`content_class` 仍只存全局发布快照 |
| 旧全局文件负载 | `global/payload.rs`（已删除）、`global/promote.rs`、`global/distribute.rs`、`global/delete.rs`、`global/sql.rs` | M5 已删除：不再写 `resources/versions/<asset>/vN`、不再从旧脚本内容表重建文件、purge 连带清 manifest/lease/publish links/forks |

---

## 13. 与现有文档的关系

- `global-file-asset-design.md`：本文取代其文件资产部分；其表资产/回收站章节仍可复用。
- `asset-table-file-business-dataflow.md`：本文取代其 F6/F7/F8/F9 与 §4.3 的文件口径。
- `analytics-resource-audit-2026-11-27.md` / `asset-file-table-route-audit-2026-11-27.md`：本文是对其中文件 P0（hash、copy、unlock、全局混合行）的设计回应。
- `README.md`：落地后回写“文件 = CAS + 版本 + lease；表 = 投影/独立资产”。

---

## 14. 组合场景能力矩阵

### 14.1 位置可见性与可写性

| 位置 \ 观察者 | 项目 A | 项目 B | 全局 |
| --- | --- | --- | --- |
| 项目 A 草稿箱 | 可写 | 不可见 | 不可见 |
| 项目 B 草稿箱 | 不可见 | 可写 | 不可见 |
| 项目 A 资产库 | 只读 | 不可见 | 不可见（除非已发布） |
| 项目 B 资产库 | 不可见 | 只读 | 不可见（除非已发布） |
| 全局资产库 | 只读 | 只读 | 只读（全局无草稿） |
| 全局回收站 | 不可见 | 不可见 | 可管理 |

结论：项目 A/B 的草稿与归档资产互不穿透；跨项目可见必须经过“发布到全局”。全局本身没有可写面；要改全局文件，只能取回到某个项目的草稿箱。

### 14.2 操作能力矩阵

| 能力 | 项目草稿箱 | 项目资产库 | 全局资产库 |
| --- | --- | --- | --- |
| 新建/编辑 | 支持 | 不支持（只读） | 不支持（只读） |
| 归档为项目资产 | 支持 | — | — |
| 取回到项目草稿 | — | 支持（本项目） | 支持（落到本项目草稿） |
| 跨项目直接读取 | 不支持 | 不支持 | 支持 |
| 发布到全局 | — | 支持（带 base 校验） | — |
| 只读打开 | — | 支持 | 支持 |
| 版本对比 | 不支持（尚无版本） | 支持 | 支持 |
| 还原历史版本 | — | 支持（生成新版本） | 支持（生成新版本） |
| 删除/回收站 | 草稿可删 | 支持（软删） | 支持（软删） |
| 冲突时 fork | — | 支持（显式） | 支持（显式，不覆盖） |

### 14.3 组合场景

| 组合场景 | 支持 | 结果 / 限制 |
| --- | --- | --- |
| A 草稿 → A 归档 | 是 | 草稿被消费，生成 A 的 v1；A 资产库只读 |
| A 归档 → B 直接看到 | 否 | 项目资产不跨项目；必须发布全局，或显式导出/导入 |
| A 归档 → 发布全局 → B 只读打开 | 是 | B 从全局 CAS 读，不产生 lease |
| A 归档 → 发布全局 → B 取回 | 是 | B 草稿，lease `base_scope=global`，base 为全局版本 |
| A、B 同时取回全局 v3 | 是 | 各自项目一个 active lease；两份草稿互不影响 |
| A 发布全局 v4；B 从 v3 再归档 | 否（不静默覆盖） | B 报 Conflict；只能 fork / 取消；不能把 v4 覆盖掉 |
| A 发布全局 v4；B 显式 fork | 是 | 生成新全局资产，记录 `asset_forks`；原全局 head 不变 |
| A 全局取回后“另存项目” | 是 | 生成 A 项目资产（`origin=global`）；全局不变 |
| A 全局取回后“提交全局”且 base=head | 是 | 生成全局 vN+1；base 不匹配则 Conflict |
| A 已发布全局；A 后续再改项目资产 | 是（但全局不自动更新） | 项目 A 变化不自动回写；要显式再次发布 |
| A 删除项目资产 | 是 | 只影响 A 项目资产；已发布的全局版本独立保留 |
| 全局资产进回收站 | 是（有条件） | 列表隐藏；已有项目副本/已取回草稿不受影响；purge 前必须无 active lease |
| 全局二进制 → A/B 只读打开 | 是 | 不建表、不建内容表；只读打开/取回/分发 |
| 全局结构化文件 → B 只读 SQL | 是 | 会话级只读视图；不默认物化表 |
| A 的项目派生表 → 全局自动可见 | 否 | 文件先行；全局派生表要单独创建，项目表不自动上升 |
| A、B 同时只读打开全局 | 是 | 各自独立读，不产生 lease |
| 全局还原旧版 → A/B 看到 | 是 | 生成新全局版本；A/B 旧 lease 的 base 落后则 Conflict |

### 14.4 明确不支持的组合

- 项目 A 资产直接出现在项目 B 资产库。
- 项目 A 的修改自动同步到全局。
- 两个项目同时改同一全局文件后自动合并；二进制没有合并语义。
- 同一资产同一项目同时存在两个 active lease。
- 全局自己拥有草稿箱；全局取回必须落到某个项目。
- 文件行直接充当表资产，或表资产伪装成可恢复文件。


