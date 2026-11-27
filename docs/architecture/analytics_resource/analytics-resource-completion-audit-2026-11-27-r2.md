# analytics_resource 完成度复审（2026-11-27，R2）

> 审计对象：`crates/analytics_resource`（`rds-analytics-resource`）。
> 审计基线：`main` HEAD `67850b01`（`刀32：修底座指纹测试的错断言`）。
> 与上一轮基线的关系：`67850b01` 仅比 `1f18c60b` 多一处 `crates/engine/src/duckdb/federation/session.rs` 修改（`git diff --stat 1f18c60b..HEAD` 为 1 file / +3 / -2），`analytics_resource` 源码未变。
> 工作树 7 项 WIP（`scratchpad`、`data_objects/layout.rs`、docs、`tools/real-db-gate.ps1`）不计入实现事实。
> 方式：只读静态追踪 + 当前 HEAD 实测；未改代码、README 或测试。

## 1. 结论

**主链已经成形，但还不能判定为“生产完成”。**

- 文件 / 表归档、CAS、版本链、取回再归档、回收站、全局发布 / 分发 / 引用 / fork、GC 都有真实实现和较厚单测；当前 HEAD 的 348 条本 crate 测试与 228 条 `workbench` 测试全绿。
- 真正的短板不在“页面有没有入口”，而在**资产身份不变量没有覆盖所有写入口**：表归档按显示名接链、回收站还原、索引补登都能重新造出同名存活资产；其中表归档会把不同源表静默接到同一条版本链。
- 其次是**失败补偿与可达性**：表快照在元数据事务失败时没有物理回滚；全局 vN-3 及更早版本在 UI 上无法直接恢复。
- 文档仍有旧口径（`global/table_name.rs`、`global/036`、reference 入口“留后续刀”、全局历史“没有独立对话框”与后文自相矛盾）。

### 完成度评分（审计判断，不是测试覆盖率）

| 维度 | 判断 | 依据 |
| --- | --- | --- |
| 核心功能主链 | **约 88–90%** | 归档 / 表快照 / 版本 / 取回 / 回收站 / 全局发布分发 / CAS GC 均可走通 |
| 资产身份一致性 | **约 70%** | 唯一名只在 `archive_new` / `rename` 收口，表归档按名接链、还原、补登仍可绕过 |
| 可靠性与失败恢复 | **约 80–82%** | CAS GC、distribute / promote 补偿已补；表快照物理孤儿未补 |
| UI 可达性与操作一致性 | **约 84%** | 面板 / 详情 / 版本 / 回收站入口齐；全局老版本与列表删除判据有缺口 |
| 产品化 / 运营完备度 | **约 72–75%** | TTL、批量改名 / 撤销、标签颜色、回收站分组、增量刷新等仍为后续刀 |
| 文档一致性 | **约 75%** | README 仍有多处旧路径 / 旧口径 / 自相矛盾 |

**综合判断：约 82%。**
“日常能不能用”已经可以；“能不能作为稳定的生产资产库发布”还不建议，应先修 P0 的身份混链与还原绕过唯一性。

## 2. 当前 HEAD 实测

| 套件 | 结果 |
| --- | --- |
| `cargo test -p rds-analytics-resource --lib -- --test-threads=1` | **307 passed / 0 failed，67.51s** |
| `cargo test -p rds-analytics-resource --test panel_window` | **29 passed / 0 failed** |
| `cargo test -p rds-analytics-resource --test dialog_window` | **12 passed / 0 failed** |
| `cargo test -p rds-workbench --lib` | **228 passed / 0 failed** |

本 crate 当前合计 **348** 条；`workbench` 下游 **228** 条。
`rds-sql` 254 与 engine 347 未在本 HEAD 重跑；README 记录的是上一轮全绿基线。
编译仍有一个 `dead_code` 警告：`global/sql.rs::drop_global_archive_table` / `table_stats` 从未使用。

## 3. 主要发现

### P0-1 表归档按显示名接链，正常“存为表”可跨源混链

**已确认（静态路径）。**

- `service/table_asset.rs::archive_table` 在 `asset_id=None` 且 `name` 命中唯一存活表资产时，直接取该行为 `existing`（约 91–104 行）。
- 随后只校验“它是表资产”，不校验源表是否同一资产、不要求 `base_version`，直接 `commit_table_version_to_head` 追加新版本（约 156–203 行）。
- `workbench/src/services/persist_jobs.rs::archive_existing_table` 始终传 `asset_id: None`（约 353–362 行），因此正常 UI 的“存为表”就能触发。

**触发条件**：已有表资产 `A`，用户把另一张同名结果表 / 临时表也“存为表”为 `A`。
**影响**：新表的 manifest 会接到旧资产的版本链上，版本历史、对比、回退都会把两张不同的表当成同一资产。不是“同名并存”，而是**静默混链**，比单纯同名更难发现。
**建议**：`asset_id=None` 命中唯一同名资产时不要自动接链；要么要求显式 `asset_id + base_version`，要么把“存为表”改成新资产并自动避让名字。补一条“同名不同源表不得接链”的回归。

### P0-2 唯一性不变量仍可被三条写入口绕过

README 第 249 行写“显示名在存活资产内唯一”，但当前只有 `archive_new`（约 169–186 行）和 `rename_resource` 做了检查；以下入口没有：

1. **项目回收站还原**
   - `service/trash.rs::restore_archive_from_trash` 对引用型 / 表型 / 文件型都只校验“不在回收站”与本体 / 表是否存在（约 170–221、249–261 行）。
   - `store/resource/trash.rs::undelete_archive` 只 `SET deleted_at = NULL`，不查同名存活行（约 125–145 行）。
   - 正常路径“删 A → 新建同名 A → 还原旧 A”即可得到两条同名存活行。
2. **全局回收站还原**
   - `global/delete.rs::restore_global` 只按 `manifest.asset_id` 清 `deleted_at`（约 407–421 行），不检查同名全局 head。
   - 全局文件 / 表在回收站期间可被同名新资产占用；还原后同一全局名出现两条活跃行，`global_file_versions(name)` 会把它们聚成一条链（`global/file.rs` 约 129–151 行）。
3. **索引补登**
   - `indexer/repair.rs::adopt_file` 只校验同一 `rel_path` 未登记，不校验显示名是否已被存活资产占用（约 9–40 行）。
   - 默认名取文件 stem，补登一个与现有资产同名的孤儿文件即可造出同名。

**影响**：列表 / 详情按 id 仍能分开，但 `alive_by_name`、按名找资产、全局按名聚合的版本链都会产生歧义；“唯一”从模型级不变量退化成入口级补丁。
**建议**：把“存活名唯一”下沉成一个统一的 `ensure_name_free(scope, name, excluding_id)`，在 `archive_table` 接链、`undelete_archive`、`restore_global`、`adopt_file` 全部调用；能加部分唯一索引就加，不能加就把冲突显式报成 `Conflict`，不要静默复活。

### P1-1 表快照物理表在元数据事务失败时没有补偿

**已确认（代码顺序）。**

- `service/table_asset.rs::archive_table` 先在 `rds_archive` 里 `copy_table` 建物理快照（约 143 行），再 `commit_table_version_to_head` 写元数据（约 186 行）。
- 后者失败时直接 `?` 返回，没有 `DROP` 刚建的物理表。
- `table_asset/gc.rs::mark_candidates` 只从 `table_snapshots` 行出发；元数据事务回滚后没有快照行，这张物理表永远不会被 GC 看到，成为永久孤儿。
- 现有 `store::table_asset::tests::duplicate_version_rolls_back_without_orphans` 只断言 manifest 行不存在，没有检查刚建的物理表。

**建议**：`copy_table` 成功后用作用域守卫 / `match` 在 `commit` 失败时 `DROP TABLE IF EXISTS rds_archive.<physical_table>`；补一条失败注入用例，断言物理表与快照行都不存在。

### P1-2 全局 vN-3 及更早版本在 UI 上不可直接恢复

**已确认（UI 路径）。**

- 详情时间线来自 `present::DetailExtras` 的最近 3 条（README 252–254 行；`present/tests.rs::detail_carries_recent_version_lines_capped_at_three` 明确钉住上限 3）。
- `detail_view/actions.rs` 的全局“恢复历史文件 / 表版本…”只用 `ctx.selected_version`（约 78–87 行）；用户能选到的版本就是时间线里那 3 条。
- 完整全局历史对话框虽然能列出全部版本（`jobs/version.rs::load_global_versions`，`VersionRows.global=true`），但宿主把全局 scope 设为只读（`workbench/src/panels/mod.rs` `set_read_only(read_only || rows.global)`），动作栏 `can_act` 要求 `!read_only`（`dialogs/version/render.rs` 约 291 行），还原 / 取回被禁用，只剩对比。

**影响**：README 第 253 / 254 行“从全局历史恢复仍需回项目详情操作”对老版本不成立——回到详情也只能看到最近 3 条。全局 vN-3 以前的历史目前没有直接恢复入口。
**建议**：二选一：在全局历史对话框提供“恢复此版本…”（写操作由宿主路由到 `restore_global_*`），或让详情时间线支持“查看完整历史并选择版本”。补一条全局历史端到端用例。

### P2-1 列表删除判据与宿主 / 服务不一致

**已确认（判据与拒绝路径）。**

- `model/affordance.rs::can_delete_row` 对项目行一律返回 `true`，不看 `read_only`、不看本体是否缺失（约 176–179 行）；测试 `missing_project_row_keeps_only_the_cleanup_paths` 还明确断言缺失行 `can_delete_row=true`。
- `resource_host/delete_actions.rs::request_delete` 对只读项目直接拒绝（约 11–14 行）。
- `service/trash.rs::trash_one` 对没有 `file_rel_path` / 本体不在的行拒绝，要求走索引修复（约 102–117 行）。

**影响**：只读项目或本体缺失的列表行会出现“判据说能删、点下去必失败”的假入口；`read_only_project_blocks_every_writing_action` 没有覆盖 `can_delete_row`。
**建议**：让 `can_delete_row` 与宿主 / 服务同一口径（至少排除 `read_only`；缺失行改走“索引修复 / 永久删登记”的独立判据），并补对应用例。

### P2-2 全局文件版本链仍按 `name` 聚合

- 全局表按稳定 `asset_id`（`global/table_asset.rs::global_table_versions`）；全局文件按 `name`（`global/file.rs::global_file_versions`，约 129–151 行），因为文件每一版新建一条 resource 行。
- 当前 UI 禁止全局行改名（`model/affordance.rs` 测试 `global_rows_can_only_distribute`），所以短路不炸；一旦未来开放全局文件改名，历史链会断 / 混。
- 属于模型级 P2 债务，不是当前阻断项。

### P2-3 README 文档漂移

已确认的旧口径：

- 第 143 行“全局侧只在详情时间线收敛入口，没有独立版本历史对话框”，与第 252 行“全局独立版本历史与对比”及 `VersionRows.global` / 全局版本对话框代码矛盾。
- 第 224 行“reference 的 UI 入口与打开校验留后续刀”，但 `resource_host/mod.rs` 已按 `kind == File` 设 `allow_reference`，`dialogs/archive/fields.rs::mode_picker` 已有 copy/reference 单选，`detail_view/actions.rs` 已有 reference 失效条与接受当前内容动作。
- 第 180、242 行仍写 `global/table_name.rs` 与 `global/036`；当前 `global/` 下已无 `table_name.rs`，迁移编号已顺延为项目 `036_analysis_primary` + `037_file_asset_cas` … `040_table_snapshot_gc`。
- 第 229 行仍写面板头“四项”，上一轮审计已指出实际为三项（本轮未单独复核，保留为待核）。

**建议**：把 README 当作本轮 P0 修复的同批交付，不要等下一轮；能力表与代码地图分两处回写，避免只改一处继续自相矛盾。

### P2-4 测试覆盖空档（事实，不等同功能缺失）

当前 348 条单测覆盖很厚，但以下场景没有直接用例：

- 全局版本历史对话框端到端；全局 diff 的真实二进制 / 大文件路径。
- `jobs/file_gc.rs` 在项目 + 全局真实库上的集成运行（现有只覆盖 CAS 单测）。
- `global/distribute` 文件登记失败、`global/promote` 登记失败回滚快照的失败注入。
- 表资产“同名多行归档歧义”；`adopt_file` 显示名冲突；回收站还原同名冲突。
- `GlobalCheckout` 结束后草稿箱 / 资产面板刷新责任：`jobs/mod.rs` 该分支只写 `op_result`，未调用 `refresh_after_op`；是否由 scratchpad watcher 补上，尚未确认。

## 4. 与上一轮审计的闭合对照

| 上轮项 | 本轮状态 | 说明 |
| --- | --- | --- |
| P0-1 显示名可重复且被当身份 | **部分闭合** | `archive_new` / `rename` 已查唯一；表归档按名接链、还原、补登仍未覆盖 |
| P0-2 解锁按名清链误删 | **闭合** | `unlock_file_to_drafts` / `unlock_global_file_to_project` 已删除，删除统一走回收站；`purge_name_chain` 仍存在但无调用，属遗留死代码 |
| P1-1 引用型解锁 API 缺口 | **闭合** | 解锁路径已撤；引用型删除走回收站只删登记 |
| P1-2 全局任意版本恢复无 UI | **部分闭合** | 详情支持 `selected_version`，但时间线只有最近 3 条，老版本仍不可达 |
| P1-3 文件 + 派生表不成组 | **未闭合** | 仍按单条处理，README 明确留 M5 |
| P1-4 引擎测试默认并发不稳 | **未闭合** | 本 crate 仍要求 `--test-threads=1`；README 已写明 |
| P2-1 跨介质补偿不齐 | **部分闭合** | distribute 文件、promote 快照已补；表快照物理孤儿未补 |
| P2-2 `TrashScope::Global` 死分支 | **未复核** | 本轮未单独追踪；不应作为完成度主项 |
| P2-3 文档漂移 | **未闭合** | 见 P2-3 |

## 5. 完成度判断的使用方式

- 如果问题是“现在能不能用”：**可以日常用**，主链入口和大多数失败回执都在；建议按 82% 理解。
- 如果问题是“这个模块是否已经完成、可以停止投入”：**不建议**。P0-1 的表归档混链与 P0-2 的还原绕过唯一性是模型级问题，不修会在后续版本 / 对比 / 分发上持续放大。
- 修完 P0-1 + P0-2，再把 P1-1 的物理孤儿补上，主链可到约 90%；P1-2 / P2 属于体验与运营层，不应和身份不变量混在一起打分。

## 6. 建议顺序

1. **P0-1 表归档身份**：`asset_id=None` 命中同名资产时禁止自动接链；workbench “存为表”改传显式资产 id 或强制新资产 + 自动避让名；补“同名不同源表”回归。
2. **P0-2 统一唯一性闸门**：抽 `ensure_name_free`，覆盖 `archive_table`、`undelete_archive`、`restore_global`、`adopt_file`；存量同名仍显式报歧义。
3. **P1-1 快照物理回滚**：`copy_table` 后用守卫 / `match` 在 `commit` 失败时 `DROP`；补失败注入。
4. **P1-2 全局老版本**：全局历史对话框加“恢复此版本”，或详情加“完整历史选择”；补端到端。
5. **P2**：列表删除判据、全局文件链键、README 回写、测试空档，按投入产出排序，不单独开大体量重构刀。

## 7. 审计边界

- 已覆盖：模块结构、归档 / 表快照 / 版本 / 取回 / 回收站 / 全局发布分发引用 / GC / 失败补偿 / UI 判据 / README 口径 / 当前 HEAD 测试。
- 未覆盖：真机多项目并发、十万级资产列表性能、网络盘 / 跨卷搬运现场行为、历史存量同名分布。
- 本文的 P0 触发路径均为源码路径推演；未构造真实数据跑一遍。修复时应先补最小集成测试钉住现状，再改行为。
- 本轮未改代码、README、测试；工作树 WIP 未纳入实现事实。
