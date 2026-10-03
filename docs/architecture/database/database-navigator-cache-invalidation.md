# 导航元数据缓存：缺陷记录与修复（2026-10-03）

> **一句话**：L2 缓存此前「写入即视为完整」——进程中途退出会留下「有 schema 行、但对象不完整」的半成品缓存，树显示缺失的表、搜索还能命中不存在的对象；本轮修正 `is_loaded` 语义（完整扫描才置 1）、读侧过滤半成品、写侧重扫时清残留，并记录失效策略现状。
>
> 本文是**缺陷记录**（现象 → 根因 → 证据 → 修复）与**失效策略现状**（哪些自动、哪些手动、为什么），代码改动与测试见文末映射表。缓存体系全貌见 [`metadata-cache-vs-dbeaver-datagrip.md`](metadata-cache-vs-dbeaver-datagrip.md)。

## 1. 背景：半成品缓存实证

2026-10-03 真机验证期间，检查项目库缓存文件发现一个「半成品」实例：

| 项 | 值 |
| --- | --- |
| 文件 | `D:\data\A1\A1\meta\connection_metadata\meta_5245bb07154b9192.sqlite` |
| 体积 | 454,656 B |
| `schemata` 行 | 存在（schema 已建行） |
| `tables` 行 | **0** |
| FTS 行 | **2**（`metadata_fts` 里已有对象） |
| 成因 | 扫描进行中被中断（进程退出 / 崩溃），schema 行与部分对象行已落盘，剩余对象未写入 |

关键事实：`is_loaded` 字段**一直存在**，但 `save_schema` **写入即置 1**（「已建行」被当成了「已扫描完」）。FTS 同步（`sync_fts_schema` 等）按 `is_loaded = 1` 过滤，于是**半成品 schema 也被收进搜索索引**——缓存里的东西既不全、又「看起来完整」。

## 2. 缺陷清单

### D1：L2 写入无原子性、无完成标记

- **现象**：扫描中途进程退出 → 留下 schema 行 + 部分对象行的半成品缓存。
- **根因**：`collect_objects` 的 L2 写（`put_schemas` → `put_objects` → `rebuild_index`）是多步非事务写入，且**没有任何「完整写完」的标记**；中断后没有任何读侧能区分「完整缓存」与「半成品」。
- **证据**：上表半成品实例（schema 在、表 0 行、FTS 却有 2 行）。
- **危害**：
  1. 树显示**缺失的表**（对象表部分写入，读侧命中「部分表清单」不再回源）；
  2. **跨连接搜索**命中不存在的对象（FTS 把半成品收进索引）；
  3. 刷新才能自愈，但用户不知道要刷新。

### D2：`is_loaded` 语义被误用（写入即置 1）

- **现象**：`save_schema` 建行时 `is_loaded` 直接写 `1`。
- **根因**：把「schema 行存在」当成了「该 schema 扫描完成」；而字段语义（FTS 同步的过滤条件、`sync_fts_schema` / `sync_fts_object` 的 `WHERE … is_loaded = 1`）本意是「完整内省后才可见」。
- **修复方向**：语义修正——建行写 `0`（扫描进行中），**表 / 视图 / 列写入 + 索引重建全部完成后**才置 `1`（新方法 `mark_schema_complete`）。

### D3：失效策略缺口

| 场景 | 现状 | 判定 |
| --- | --- | --- |
| 删除连接 | 缓存文件**不自动删**；唯一删除入口是「⋯ → 缓存管理…」对话框 | **有意为之**：同物理库多条连接共享 `meta_{指纹}.sqlite`，删一条连接自动删缓存会误伤仍在使用的连接；手动入口让用户看清再删（`data_source_service.rs::delete` 注释「已落盘缓存文件不受影响，由缓存管理清理」） |
| 右键刷新元数据 | `refresh_node` / `refresh_all` 以 `fresh=true` 重扫，写侧先 `prune` 该范围旧行再整块重写 | **已有**（本轮补强：半成品 schema 重扫同样先 `prune`） |
| 库对象被外部变更（表被 DROP） | 日常浏览命中缓存不检测；需显式「刷新元数据」 | **可接受**：自动变更检测要对每次浏览做代价对比，收益抵不过成本；刷新是显式、按范围的入口 |
| 半成品缓存 | **本轮修复**：读侧 `schemas()` 过滤未完成 → 视为未命中 → 回源库重扫；重扫时（未完成）先清残留再整块重写 | **自动自愈**，无需用户操作 |

## 3. 修复方案（2026-10-03 实施）

三层改动，形成「写侧标记完整 → 读侧过滤未完成 → 重扫清残留」闭环：

### 3.1 写侧：完成标记（engine）

- `save_schema`：建行 `is_loaded = 0`（「扫描进行中 / 未完成」）。
- 新增 `mark_schema_complete(catalog, schema)`：`UPDATE schemata SET is_loaded = 1`。
- 新增 `get_schema_loaded(catalog, schema)`：读 `is_loaded`（供写侧重扫判断）。

### 3.2 写侧：重扫先清残留（database）

- `collect_objects`（`navigator_service.rs`）：清旧行时机从「仅 fresh」改为 **`fresh || !is_schema_complete(catalog, schema)`**——半成品 schema 重扫必须先 `prune_schema` 清掉残留对象再整块重写；已完整的 schema 不 `prune`（表趟写完、视图趟只补视图，清旧行会误删表数据）。
- 表 / 视图两趟写入 + 索引重建后统一 `mark_schema_complete`（幂等）。

### 3.3 读侧：过滤未完成（database）

- `NavCache::schemas()`：只返回 `is_loaded = 1` 的 schema；空结果视为未命中 → 回源库重扫。
- FTS 同步（engine，无需改）：本就按 `is_loaded = 1` 过滤，语义修正后自动不再收半成品。

### 3.4 不改的部分（设计决策，记录）

- 删除连接不自动清缓存：指纹共享下自动删除会误伤（见 D3）。
- 不做自动变更检测：成本收益不划算，刷新是显式入口。

## 4. 测试

| 测试 | 断言 | 归属 |
| --- | --- | --- |
| `incomplete_schema_is_missed_until_marked` | `put_schemas` 未 mark → `schemas()` 视为未命中；mark 后命中 | `crates/database/src/cache.rs` |
| `fts_rebuild_is_schema_scoped_and_idempotent`（补 mark） | FTS 同步只收完整 schema（`is_loaded = 1`） | `crates/engine/src/persistence/metadata_cache.rs` |
| `connect_success_reloads_expanded_root` | 建连成功 → 已展开连接根自动重载入队（attempted 痕迹） | `crates/database/src/nav_view/tests.rs` |
| `connect_success_skips_collapsed_root` | 未展开连接根不自动重载 | 同上 |

基线（2026-10-03）：`rds-database --lib` **102**、`rds-engine --lib` **527**，全绿。

## 5. 实现位置映射表

| 设计决策 | 代码位置 |
| --- | --- |
| 建连成功自动重载已展开连接根（配套：连接就绪前展开吃到 `CONN_NOT_FOUND`，就绪后必须自动补载） | `crates/database/src/nav_view/actions.rs::apply_connect_results`（成功分支） |
| L2 完成标记：建行 `is_loaded=0` + `mark_schema_complete` + `get_schema_loaded` | `crates/engine/src/persistence/metadata_cache.rs` |
| 读侧过滤半成品（`schemas()` 只返回完整 schema） | `crates/database/src/cache.rs::schemas` |
| 写侧重扫先清残留 + 完成标记（幂等） | `crates/database/src/navigator_service.rs::collect_objects` |
| 删除连接清缓存入口（手动，指纹共享下不自动删） | `crates/workbench/src/components/cache_dialog.rs::delete_cache` + `crates/workbench/src/services/data_source_service.rs::delete`（注释） |
