# 文件资产 CAS 模型实施计划

> 依据：`file-asset-cas-design.md`（2026-11-27 定稿）。
> 目标：按“CAS + manifest + version + ref + lease”重做文件资产，先落项目侧闭环，再推全局 file-first。
> 约束：一刀一 commit；每阶段可独立回滚；不改表资产的既有语义。

## 0. 实施边界

本轮先做**项目侧文件资产闭环**：

- 新归档走 CAS，文件版本按字节哈希；
- 归档消费草稿；
- 取回生成 lease，再归档按 lease 升版；
- 文件元数据（`file-meta` sidecar）随 manifest 存档，取回时回填；
- 旧 `copy` 路径保留兼容，逐步迁移。

暂不在本轮强推：

- 全局 file-first 的行拆分与全局 fork UI；
- 二进制全局发布；
- 表投影的自动创建；
- 文件版本任意两版对比 UI。

它们进入后续里程碑，避免一次改动同时动项目/全局/UI 三条链路。

## 1. 里程碑

| 里程碑 | 内容 | 验收 |
| --- | --- | --- |
| M0 | 基线确认 | 当前分支测试基线通过；记录改动前状态 |
| M1 | CAS + manifest 基础层 | 同一文件两次入库只存一份 blob；manifest 可还原字节；坏 hash 拒绝 |
| M2 | 文件资产元数据与 lease 表 | 迁移可重跑；一张资产一个 active lease；manifest 可按版本查询 |
| M3 | 归档 / 取回 / 再归档接入 | 归档消费草稿；字节变化必升版；无 lease 不得升版；元数据随版本往返 |
| M4 | 全局 file-first | 全局文件行 `kind=file`、无 `analytics_table`；派生表独立；base 冲突只 fork |
| M5 | 查看 / 对比 / 迁移 / UI | 只读表视图、任意两版对比、旧数据回填、动作文案切换 |

## 2. M1：CAS + manifest 基础层

### 2.1 数据结构

```rust
ManifestEntry {
    path: String,          // 逻辑文件名
    kind: EntryKind,       // Content | FileMeta
    hash: String,          // sha256(bytes)
    size: u64,
    executable: bool,
}

FileManifest {
    schema_version: u32,
    entries: Vec<ManifestEntry>,
    root_hash: String,     // sha256(canonical entries)
    content_hash: String,  // Content entry 的 hash
    file_size: u64,
    meta_hash: Option<String>,
}
```

### 2.2 代码落点

- `crates/analytics_resource/src/payload/manifest.rs`
- `crates/analytics_resource/src/payload/cas.rs`
- `PayloadStore` 增加 `cas()` 访问器（路径为 `.RSmeta/cas/`）。

### 2.3 行为

- `CasStore::put_file(source)`：流式 sha256；写 staging；原子 rename 到 `blobs/<aa>/<hash>`；同 hash 直接复用。
- `CasStore::put_json(file_meta)`：文件元数据序列化为 canonical JSON，作为 `FileMeta` entry 入库。
- `CasStore::materialize(manifest, dest)`：逐 entry 复制，校验 hash；失败清理半成品。
- `CasStore::read_manifest(id)`：从 `manifests/<aa>/<id>.json` 读，校验 root hash。
- 第一版不做 CDC 分块；接口保留 `chunks/`，后续替换 `put_file` 内部实现不改变 manifest 语义。

### 2.4 验收

- 同内容两个文件入库：`blobs` 只有一个对象。
- 改一个字节：`content_hash` 变；manifest ID 变。
- 篡改 blob 后 materialize：报 hash mismatch，不落半成品。

## 3. M2：文件资产元数据与 lease

### 3.1 迁移

新增 `crates/store/migrations/project_meta/036_file_asset_cas.sql`：

```sql
CREATE TABLE IF NOT EXISTS file_asset_versions (
  resource_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  manifest_id TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  meta_hash TEXT,
  parent_version INTEGER,
  content_pruned INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  PRIMARY KEY (resource_id, version)
);

CREATE TABLE IF NOT EXISTS checkout_leases (
  lease_id TEXT PRIMARY KEY,
  resource_id TEXT NOT NULL,
  base_scope TEXT NOT NULL,
  base_version INTEGER NOT NULL,
  dest_rel TEXT NOT NULL,
  state TEXT NOT NULL,
  created_at TEXT NOT NULL,
  closed_at TEXT,
  consumed_version INTEGER
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_active_lease
  ON checkout_leases(resource_id, base_scope)
  WHERE state = 'active';
```

- 同步 `schema_sql.rs` 的测试建库清单。
- 全局侧同结构后续单独迁移，不混在本刀。

### 3.2 Store API

- `insert_file_asset_version_on(conn, ...)`：归档 v1 与再归档共用，事务内写。
- `file_asset_version(resource_id, version)`。
- `list_file_asset_versions(resource_id)`。
- `active_lease(resource_id)` / `create_lease` / `consume_lease` / `abandon_lease`。
- `lease_by_dest(dest_rel)`：按工作副本反查，兼容现有取回凭据语义。

### 3.3 验收

- 同一资产第二次 `create_lease` 返回 Conflict，提示已有 lease。
- lease 消费后 `active` 消失；历史记录保留。
- 旧库没有新表时，迁移能补建；旧资产无 `file_asset_versions` 行不报错。

## 4. M3：归档 / 取回 / 再归档

### 4.1 归档

1. `ArchiveKind::File` 一律算字节哈希；`AnalysisFacts` 只写 `structured/schema_hash`，不再当 `content_hash`。
2. `PayloadMode::Copy` 的新归档：
   - CAS 入库（content + file_meta）；
   - 物化 `resources/<rel>`；
   - 登记行 + `file_asset_versions` 同一事务；
   - 事务成功后删除草稿；删除失败记 note，不回滚。
3. `PayloadMode::Reference` 本轮保持旧路径，不强行 CAS（源不在 M6 管辖）。
4. `Analysis` 表资产保持旧路径，不进入 CAS。

### 4.2 取回

1. 先查 active lease；存在则返回 Conflict（提示打开已有草稿 / fork）。
2. 有 manifest：从 CAS 物化到 `dest`，校验 hash；回执带 `file_meta`。
3. 无 manifest：回退旧 `copy_out`，lease 仍创建，保证后续再归档能校验 base。
4. 写 `checkout_receipt` 与 `checkout_leases`；lease 是权威，receipt 兼容旧 UI。

### 4.3 再归档

1. 按 `existing_resource_id` 找 active lease；有 lease 时校验 `base_version == current.version`。
2. base 不匹配：返回 Conflict；本轮不做自动 fork，由调用方决定。
3. 无 lease 的旧路径：保留兼容，但记 warn（M4/M5 再收紧）。
4. CAS 新 manifest → 事务写 v+1、lease consumed → 物化 resources → 删草稿。

### 4.4 元数据联动

- 归档侧**不新增 `ArchiveRequest` 字段**：服务按 `ArchiveBinding.promoted_from` 定位草稿的集中 sidecar
  （`{project}/.RSmeta/scratchpad/meta/{相对目录}/{文件名}.meta.json`），本地文件用 `rds-file-meta` 默认同目录 sidecar；
  这样保持 `scratchpad → analytics_resource` 的依赖方向，不让 M6 认识草稿箱内部类型。
- CAS manifest 写入 `FileMeta` entry；`content_hash` 不含元数据，`manifest_id` 含。
- `CheckoutOutcome` 增加 `file_meta: Option<file_meta::FileMeta>`；`run_checkout` 落回草稿集中的 sidecar。
- `CheckoutOutcome.file_meta` 缺省时回退旧复制路径，不阻塞取回。

### 4.5 验收

- 归档后草稿不存在；再取回才出现工作副本。
- 同一 CSV 只改行数据：`content_hash` 变，v2 产生。
- 无 lease 直接 `archive(existing_id)`：在严格模式下拒绝；兼容模式下 warn 并保留旧行为。
- 修改书签后归档：`manifest_id` 变，`content_hash` 不变，版本记录 `meta_hash`。
- 取回 v1：草稿 sidecar 恢复为 v1 的元数据。

## 5. M4：全局 file-first

1. 全局文件行 `kind=file`，`analytics_table=NULL`；现有 `analysis + file` 混合行迁移为文件 + 可选派生表。
2. `global/promote.rs` 改为发布 `manifest` 到全局 CAS；项目资产保留。
3. 发布必须带 `base_global_version`；不匹配返回 Conflict，只能 fork。
4. `global checkout` 写项目 lease（`base_scope=global`）；再归档可选“提交全局 / 另存项目”。
5. 全局二进制只读打开/取回/分发，不建表。
6. 全局回收站：`trash` 可隐藏，`purge` 前必须无 active lease。

验收：

- 全局发布 v1（名字空闲）成功；并发发布 base 落后只 fork。
- 全局二进制发布后无 `analytics_table`。
- 项目删除不影响已发布全局版本。

## 6. M5：查看、对比、迁移与 UI

1. 查看路由改读 `content_class + duckdb_readable`，不再按扩展名猜。
2. 结构化文件只读表格 / 会话 SQL；非结构化只读编辑器；二进制信息页。
3. 版本对比支持任意两版；文本 unified diff、表格 schema/row diff、二进制元信息。
4. 旧 `versions/<id>/vN` 不再回填：M5 直接删除旧副本读写路径；只有 manifest 的版本可还原，缺失版本标 `content_pruned`。
5. UI 动作文案：
   - 删除文件不再自动“解锁退回草稿”；
   - “取回（检出）”显示 lease 状态；
   - 全局“解锁”改为“取回/分发”，不销毁资产。
6. 停写旧版本副本，并删除旧副本模块；不做“只读保留一个周期”的兼容。

## 7. 风险与回滚

| 风险 | 处理 |
| --- | --- |
| CAS 写成功、DB 失败 | 对象成为不可达；GC 清理；草稿保留 |
| DB 成功、物化失败 | 资产可读但 `resources` 缺失；下次打开/修复时从 manifest 重建 |
| 归档后删草稿失败 | 记 `pending_cleanup`；不回滚；重试删除 |
| 旧版本副本与 manifest 不一致 | 以 manifest 为准；回填时校验 sha256，不一致拒绝切换 |
| 旧测试依赖“归档保留草稿” | 按新语义更新断言；旧行为不再作为验收 |
| 全局行拆分影响现有分发 | 先做项目侧；全局迁移单独一刀，保留旧表型兼容 |

回滚：

- 新表只增不删；M5 起不再提供旧 `copy_in/copy_out` 兼容开关。
- manifest 是唯一版本读路径；缺失直接报错并标 `content_pruned`，不回退旧副本。
- M5 删除旧 `versions/` 代码路径，不做回填或过渡保留（用户 2026-11-27 明确“不要兼容”）。

## 8. 验证矩阵

| 用例 | 断言 |
| --- | --- |
| CAS 去重 | 同内容两次入库，blob 数不增 |
| 字节变更 | 只改行数据也升版 |
| 归档消费 | 归档成功后源草稿不存在 |
| 取回 lease | 同一资产第二次取回被拒 |
| 再归档 base | base 落后返回 Conflict，不覆盖 head |
| 元数据往返 | 书签/连接/视图随 manifest 存档，取回恢复 |
| 崩溃注入 | DB 失败草稿保留；物化失败可从 manifest 重建 |
| 旧资产拒绝 | 无 manifest 的旧资产取回/还原明确报错，不静默回退 |
| 全局发布 | base 匹配追加，不匹配 fork |

## 9. 实施状态

- [x] 设计定稿（`file-asset-cas-design.md`）
- [x] M1 CAS + manifest（`payload/cas.rs`、`payload/manifest.rs`，含去重 / 篡改 / 元数据用例）
- [x] M2 迁移 + store（`036_file_asset_cas.sql`、`store/file_asset.rs`；两张表 `ON DELETE CASCADE`）
- [x] M3 归档 / 取回 / 再归档（项目侧闭环：归档消费草稿、字节哈希、CAS manifest、lease 校验与消费）
- [x] M3 元数据联动（归档读草稿/本地 sidecar → manifest；取回交回 `FileMeta` 并落 sidecar）
- [x] M4 全局 file-first（全局发布 / fork / 取回 lease / 分发 / 回收站 / 历史 manifest 恢复；项目文件删除改走回收站）
- [ ] M5 查看 / 对比 / 迁移 / UI

### 9.1 验证

#### M3（2026-11-27）

- `cargo check -p rds-analytics-resource -p rds-workbench --lib -j 2` 通过。
- `cargo test -p rds-analytics-resource --lib -j 2`：303 passed（含新增 `t132_file_meta_roundtrip_and_single_active_lease`、`t208_structured_file_with_manifest_still_checks_bytes`）。
- 旧用例按新语义更新：归档消费草稿（t101/t106）、撤销把本体送回原草稿（t120）、文件壳默认全保留但显式策略仍生效（t130）、文件字节哈希（t133）。
- 结构化文件带 manifest 时仍参与索引字节比对（t208）；只有无 manifest 的旧结构化行继续跳过。

#### M4（2026-11-27）

- `cargo check -p rds-analytics-resource -p rds-workbench --lib -j 2` 通过。
- `cargo test -p rds-analytics-resource --lib -j 2 --no-fail-fast`：308 passed，0 failed。
- 新增 `g090`–`g095`：file-first 无表与 base 守卫、二进制无表、全局取回 lease 与 purge 守卫、fork 复用 manifest、分发物化项目文件、历史 manifest 恢复生成新 head。
- `g031` 改为文件可直接进全局回收站、还原后再显式解锁；项目侧 `t129` 同步改为「删除 = 进回收站，还原后仍可显式解锁退草稿」。
- 全局回收站根目录改由 `system_dir.parent()/trash/global` 推导，并行用例各自隔离，生产路径仍是 `{RDS_HOME}/data/trash/global`。

#### M5 第一刀（2026-11-27，进行中）

- 删除 `service/unlock.rs`、`global/unlock.rs`、`global/payload.rs`，移除 unlock 作业 / `OpOutcome` / 宿主动作与确认框。
- 全局 `promote` 不再写旧文件 staging / `resources/versions/<asset>/vN` 副本，`PromoteKind` 只剩 `DataFile` / `File`；`register_global` 去掉 `GlobalStagedFile`。
- 全局 `distribute` 对无 manifest 的文件行直接拒绝；全局 `restore_global_file_version` 只认 manifest；`purge_global` 连带清 `file_asset_versions` / `global_checkout_leases` / `global_publish_links` / `asset_forks`。
- 删除 `g003/g005/g009/g010/g011/g012/g025/g026/g027/g031` 旧脚本/旧文件负载用例；`g028` 改用 `publish_file` 验证文件回收站。
- `cargo check -p rds-analytics-resource --all-targets -j 2` 通过；`cargo test -p rds-analytics-resource --lib -j 2 --no-fail-fast`：297 passed，0 failed。

#### M5 第二刀（2026-11-27）

- 删除 `payload/versions.rs`：项目侧不再写 `resources/versions/<id>/vN` 副本，也不再裁剪副本。
- 再归档不再“旧内容留副本”；版本还原 / 历史取回 / 版本对比 / 「用 vN 分析」全部改走 manifest + CAS。
- 版本历史“可还原”判据改为 `file_asset_versions`；`VersionAction::DeleteCopy` 与 UI 按钮删除。
- `KeepVersions` 从文件路径撤出，只保留表型 `prune_version_tables`。
- 测试：`cargo test -p rds-analytics-resource --lib -j 2 --no-fail-fast` 294 passed；`cargo check -p rds-workbench --lib -j 2` 通过。

### 9.2 与计划的偏差

- `ArchiveRequest` 未新增 `file_meta` 字段：服务按 `promoted_from` 字符串约定直接读 sidecar，避免把完整元数据穿过 UI 请求层，也避免破坏 `DraftCandidate` 的 `Eq`。
- 文件资产默认全保留；M5 第二刀已删除 `payload/versions.rs` 与 `prune_copies`，显式 `KeepVersions` 只作用于表型。
- 归档删除草稿失败当前只写回执 note，未落 `pending_cleanup` 列（M5 再补持久化重试）。
- 全局文件发布仍是「一版一行、新 `resource_id`」，版本链靠 `parent_resource_id` 接续；设计中的稳定单 asset id 未落，作为后续迁移项。
- 全局发布暂不自动建派生表：文件行 `analytics_table=NULL`；旧 `analysis + file` 混合行迁移、派生表独立留到 M5。
- 全局取回的 UI/job 入口尚未接；当前列表/详情只提供分发与回收站，`checkout_file` 已在服务层可用。
