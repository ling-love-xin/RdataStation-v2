# 全局引用资产设计

状态：已定稿（2026-11-25），按版本实现的代码分批落地。

## 1. 结论

全局是共享目录，项目是引用书签。项目 B 不复制全局数据，只登记一条**引用资产**，按版本 pin 全局表；查看走全局只读，用户选中的版本可以**固化**成项目内独立本地表。

已锁定的口径：

- 全局资产本名不可改；只允许项目侧改引用别名。
- 引用只支持表型资产；文件型引用后置。
- 每个项目引用版本单独 pin 一个全局版本，保留和固化都按版本，不按资产整体。
- 只有被引用的全局版本保留；未引用的旧版仍可由 `KeepVersions` 裁剪。
- 项目引用资产挂项目分组/标签；未引用的全局资产不能进项目分组/标签。
- 固化选中的版本 = 新建一条独立本地表资产，原引用资产保留。
- 整项目导出、文件引用、全局版本回退 UI 后置。

## 2. 项目引用资产

v1 不新增 `kind` 值，沿用 `table_ref` 落库（避免重建 `kind` CHECK 的迁移），用 `origin_kind='global'` 和 `config` 区分“全局引用”与历史远端引用。

建议的项目行：

```json
{
  "resource_type": "table",
  "name": "全局资产名",
  "alias": "项目内别名",
  "scope": "project",
  "kind": "table_ref",
  "payload_mode": "reference",
  "readonly": 1,
  "origin_kind": "global",
  "origin_ref": "{\"instance\":\"全局行 id\",\"version\":2}",
  "config": "{\"global_name\":\"orders\",\"global_version\":2}"
}
```

- `name` 保持全局本名，作为引用身份；`alias` 是项目内改名。
- 引用版本只记当前 pin；项目版本快照记同一组 `global_name/global_version`。
- 引用行没有 `analytics_table`，不落本地数据。
- 打开/查看时按 `global_name + global_version` 找全局实际表名，短命只读挂载。

## 3. 引用登记与保留

全局库新增引用登记表，按项目引用版本一行：

```sql
analytics_global_refs(
  id, project_root, project_resource_id, project_version,
  global_name, global_version, created_at, released_at
)
```

唯一键 `(project_root, project_resource_id, project_version)`。

- 创建/更新 pin 时 upsert 并清空 `released_at`。
- 固化选中版本、删除引用版本、删除引用资产时，只释放对应行。
- 裁剪 `KeepVersions` 时跳过 `released_at IS NULL` 的 `global_name + global_version`。
- `delete_global` 发现该全局名还有活跃引用时拒绝删除，提示先固化或解除引用。
- 项目删除/回收站清理要释放该项目的全部登记；双库无共同事务，按补偿/GC 处理。

## 4. 固化

用户在引用资产的版本列表里选 `vN`：

1. 校验 `global_name + global_version` 的表仍在。
2. 用现有 parquet 中转路径把该全局表复制到项目分析库。
3. 新建独立 `AnalyticsTable` 本地资产，版本从 `v1` 开始。
4. 默认复制引用资产的别名、分组、标签；显示名加“（已固化）”，允许改名。
5. `origin_ref` 记“固化自全局 vN”。
6. 原引用资产保留，`vN` 标记“已固化”，释放该版本 pin；其他版本 pin 不动。
7. 失败则保留原引用和 pin，不产生半成品本地资产。

固化后的本地资产不再依赖全局；该全局版本在没有其他 pin 后可以正常裁剪。

## 5. UI 口径

- 全局视图 = 全局目录（公共只读）+ 本项目引用层。
- 项目引用行显示别名，副标题显示 `全局名 · vN`。
- 未引用全局行只提供“引用到本项目”，不提供分组/标签。
- “全部”视图按 `global_name + global_version` 去重。
- 版本列表允许对引用版本执行“固化”；已固化版本不再允许当引用回退。
- 只读项目只能查看；改别名、打标签、创建引用、更新 pin、固化都按写项目元数据禁止。

## 6. 后置

- 整项目导出（当前只做单表固化）。
- 文件型引用/固化。
- 全局版本浏览与回退 UI。
- 现有分发副本迁移为引用。
- 标签定义从全局 TagTable 迁到项目库的迁移方案。

## 7. 实现位置映射

| 设计决策 | 代码位置 |
| --- | --- |
| 全局引用登记表 | `crates/store/migrations/global/038_analytics_global_refs.sql`（原 037，与网络两层撞号后改 038） |
| 登记 API / 查询 | `crates/analytics_resource/src/global/refs.rs` |
| 裁剪守卫 | `crates/analytics_resource/src/global/sql.rs` |
| 删除守卫 | `crates/analytics_resource/src/global/delete.rs` |
| 项目引用资产行 | `crates/analytics_resource/src/global/refs.rs`：`reference_to_project`（首建 v1 / 同引用追加版本） |
| 固化动作 | `crates/analytics_resource/src/global/refs.rs`：`solidify_ref_version`；底层 `distribute.rs`：`distribute_version` |
| 回归证据 | `crates/analytics_resource/src/global/tests.rs`：`g021`（引用→追加→固化选中 v1）、`g022`（固化失败保留 pin/引用） |
| UI 引用入口/版本动作 | 待落：`resource_view` / `detail_view` |
