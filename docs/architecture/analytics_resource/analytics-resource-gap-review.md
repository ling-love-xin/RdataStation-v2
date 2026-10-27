# 资产库（M6）现状评估与差距清单（2026-10-22 · 10-22 复核更新）

> 用途：原型 / 语义裁决 / 实现现状三方对账，可直接进 B 组待办决策记录。
> 证据口径：所有「现状」均来自工作区源码与文档实读；`文件:行号` 为定位锚点（行号随代码演进会漂，以符号名为准）。评估基准 = `analytics-resource-prototype-design.md`（2026-09-15 定稿，415 行）+ `analytics-resource-architecture.md`（含 2026-10-13 裁决）+ `analytics-resource-dev-plan.md`（进度至 2026-10-13）+ 工作区未提交改动（T18–T21）。
> 状态口径：✅ 已满足 / 🟢 已实现（未提交，编译+测试已验证）/ 🟡 部分 · 未落地 / 🔴 缺口 / ⚪ 文本过时。
> **2026-10-22 复核**：T18–T21 洞察联动代码已修复括号失配并通过验证（cargo check 6 crate 全绿；rds-insight 234 单测 + 14 e2e + 4 契约、rds-analytics-resource 25 面板窗口、rds-engine temp_table 保护、rds-workbench 全套件含修复后的 insight_editor_entry / insight_source_real 均绿）；修复内容：`workspace.rs` 括号失配 + 两测试文件 7 处旧签名调用补 `full` 参数（见下 §5）。
> **2026-10-22 复核②**：T18–T21 已提交（`002e2c98` feat + `2c81280d` docs），全仓 `cargo check/test --workspace` 全绿。§13 双实例（全局/项目分析库）规格已定稿（2026-10-22 用户澄清：升级 = 表 + 版本信息进全局 DuckDB；脚本方案 A 定稿；跨项目唯一法定路径 = 项目→全局→项目，项目↔项目禁止直传），实现评估见缺口 #8。

## 0. 结论（一页）

1. **原型主体仍然成立**：面板 / 工具栏 / 行列表 / 分组折叠 / 详情 / 五对话框 / 只读守卫 / 事件 / 索引修复 / 回收站均已落地，原型逐节标注的落地率约 90%，剩余未落地项多为低优先收尾。
2. **原型已不能完整覆盖现在的需求**，缺口三类：
   - 🟢 **洞察联动（T18–T21）**：原型定稿时不存在「已洞察表集合 / 对比 / 关系排查 / 洞察归档」这一交互面，属新需求——**2026-10-22 已实现并通过编译 + 测试**（未提交，见 §5 验证记录）；
   - 🔴 **2026-10-13 裁决的 UI 尾项**：File 档 copy/reference 两档粒度已拍板，但「归档对话框选模式 / reference 打开校验指纹 / reference 删除入口」未落地（dev-plan 自述「留后续刀」）；
   - ⚪ **原型文本三段已过时**：归档序列图（move→copy）、撤销语义（移回→删副本）、TableRef 规格（已划掉）。
3. **另**：全局标签（rds-tag，0.2.3 抽出 `crates/tag`）把标签定义上移全局库、跨 M4/M6/M5 共享——交互形态不变，但作用域语义超出原型假设，需补注。

## 1. 原型已覆盖、尚未落地（收尾项，低优先）

| # | 状态 | 项 | 定位 | 说明 / 依赖 |
| --- | --- | --- | --- | --- |
| 1 | 🟡 | 空库「草稿箱有 N 个文件值得归档」建议行 | `resource_view.rs` 空态；原型 §5 | 需草稿箱侧给「可归档文件数」，两 crate 协作；原型 §5 落地注已记 |
| 2 | 🟡 | 索引异常首次进入的顶部一次性提示条 | `resource_view.rs`；原型 §5 | 原型 §5 落地注已记 |
| 3 | 🟡 | 详情版本区「最近 3 条」明细 | `detail_view.rs`；原型 §3.1 | 需「每存档最近 N 版」批量取数；dev-plan 已记 |
| 4 | 🟡 | 取回侧记 `derived_from_resource_id` | 宿主取回路径；原型 §4.2 | 需草稿箱侧加字段，属 M5；dev-plan 已记 |
| 5 | 🟡 | 行内 hover 动作 | `resource_view.rs`；原型 §2.3 | 原型自述「归给详情面板批」 |
| 6 | 🟡 | 回收站自动清理策略 / 条目「在原位置打开」 | `dialogs/trash.rs`；原型 §4.4 | dev-plan 已记 |
| 7 | 🟡 | 详情面板宽度记忆（h_resizable） | `detail_view.rs`；原型 §3.1 | 依赖 M4 属性面板接线；当前暂按 `RIGHT_DOCK_WIDTH` |
| 8 | 🟡 | 编辑器「以结果区打开数据文件（只读）」API | 编辑器侧；原型 §2.3 / dev-plan P4.4 | analysis 档打开现由洞察取样入口承担，见 `resource_host.rs::request_open` |

## 2. 原型未覆盖 / 裁决后未补（核心缺口）——T18–T21 已实现，其余仍开

| # | 状态 | 项 | 定位 | 说明 |
| --- | --- | --- | --- | --- |
| 1 | 🟢 | **已洞察表集合（T18）**：右栏「已洞察表」集合条目（`InsightSetEntry`：源表 / 结果集 / 文件三来源）、宿主推送 `set_insight_set` | `crates/insight/src/model.rs`（`InsightSetEntry` / `InsightSetKind`）、`crates/insight/src/insight_view.rs`（`set_insight_set`）、`crates/insight/src/workspace.rs`（`to_set_entry` / `insight_set_entries`）、`crates/insight/src/view/set.rs`（集合卡渲染） | 会话工作集概念，原型无此交互面；已实现（未提交） |
| 2 | 🟢 | **右栏「对比」请求 → 中间页切对比形态（T18）** | `crates/workbench/src/components/insight_actions.rs`（`CompareRequested`）、`crates/workbench/src/view.rs`（`consume_insight_compare`）、`panels/shared.rs`（`pending_insight_compare`） | 跨右栏 ↔ 中间页的勾选 + 形态切换；已实现（未提交） |
| 3 | 🟢 | **关系排查（T19）**：两洞察临时表同名列值域交集占比 | `crates/insight/src/insight_engine.rs`（`compute_column_overlap` + 单测）、`crates/insight/src/service/mod.rs`（`overlap_report`）、`crates/insight/src/jobs.rs`（`request_overlap`） | 语义 = 值重叠（非外键）；已实现（未提交） |
| 4 | 🟢 | **洞察归档（T20，File 档）**：画像快照 JSON → 资产库 File 档 copy + 版本链 | `crates/insight/src/service/mod.rs`（`export_table_snapshot`，快照带 `generated_at` 时间戳）、`crates/workbench/src/components/insight_actions.rs`（`archive_from_insight`：Copy 档 + `binding.source_table` + 标签「洞察」）、`resource_jobs::enqueue_archive` | 每次归档带时间戳 → 指纹必变 → 必开新版本（有意，靠 `keep_versions` 兜底）；已实现（未提交） |
| 5 | 🔴 | **A2 data 视图（analysis 档）**：快照 = 画像结构化数据 + 配方（取样查询 + 规则集），打开 = 桥接结果集可二次分析 | 未落地；原型 §12.2.1（规格已补全） | 当前只有 file 档 JSON；「打开即二次分析」闭环未通，**规格 §12.2.1 已就绪，待用户决策本期做 / 不做** |
| 6 | ✅ | **File 档 copy/reference 归档 UI**：归档对话框「选模式」、reference 打开校验指纹反馈、reference 删除入口 | 已实现（M6 收口批）：`dialogs/archive.rs` 增 `ArchiveDialogSeed.allow_reference/source_abs_path` + result `payload_mode`，表单 copy/reference 单选切换呈现；`resource_host.rs` `request_open` reference 懒校验指纹（`content_hash` vs 登记），失效置 stale 槽位 + 详情顶部「接受当前内容 / 从历史还原」两动作；`move_to_trash` reference 分支只软删登记行（本体留原位）+ 回收站清单合并软删 reference 行 | 数据层（`PayloadMode` / 迁移 023 / `copy_in` / `resolve_source`）已就位；UI 规格 §12.1.1 已兑现 |
| 7 | 🟢 | **临时表保护名单（T21）**：集合内表不参与 TTL / 上限清理 | `crates/engine/src/duckdb/temp_table.rs`（`protect` / `unprotect` / `is_protected`，TTL/上限均跳过 + 3 个单测）、`crates/insight/src/insight_view.rs`（`set_insight_set` 内同步） | engine 层机制，保护「会话工作集」不被 30 分钟 TTL 清掉；已实现（未提交） |
| 8 | ✅ | **双实例（全局 / 项目分析库）资源展示 + 升级 / 分发（§13）**：展示两实例资源表；项目 → 全局升级（分析表 / 数据文件映射出表 / 脚本内容表）；全局 → 项目分发（跨项目唯一法定路径） | 规格：原型 §13（2026-10-22 定稿）；**M6 收口批已全部实现**：①engine 全局迁移 026 + 服务层 `global.rs`（升级 = 数据文件映射出表 / 脚本内容表 + 全局登记 + 版本链；分发 = Parquet 中转 + 项目登记；`promote_overwrite` / `distribute_overwrite` / `distribute(target)`）——g001–g006 单测绿；②宿主接线：File 档右键「升级到全局…」+ `PromoteJob/DistributeJob` + 回执；③展示层：全局实例列表 / 实例筛选 / scope 徽标 + 全局行轻量详情（实例行 + 动作收敛到「分发到本项目…」） + 全局空态分流；④升级/分发同名冲突三选对话框（覆盖 / 改名 / 取消，`OpOutcome::Conflict` + `*_overwrite`）。**仍开**：E1 分析表型升级（M8 批次阻塞）、A6 项目行「已升级」标记（P2，重复升级由 A1 冲突拦截兜底）。全局登记结构已按「全局 SQLite 镜像表」落地 |
| 8 | 🟢 | **全局标签作用域**：定义上移全局库、跨模块共享 | `crates/tag/`（`TagTable::open_global` / `SCOPE_GLOBAL`）、`crates/analytics_resource/src/tag.rs`（`ensure_tags_migrated` 存量迁移） | 交互规格（chips / 筛选 / 批量）仍适用；「标签定义在全局库」超出原型假设，规格在 `docs/architecture/tag/tag-system-spec.md` |

## 3. 原型文本已过时（需修订，随本次原型更新一并处理）

| # | 位置 | 旧文本（摘） | 现状（源码实证） |
| --- | --- | --- | --- |
| 1 | §4.1 归档序列图 | `S->>S: 刷新树（草稿已消失）` | 2026-10-13 起默认 copy：草稿保留为工作副本（`service.rs::archive_new` 走 `copy_in`） |
| 2 | §4.1 撤销语义 | 「撤回 = 本体移回草稿箱 + 删除登记行」 | copy：删 `resources/` 副本 + 删登记行；reference：只删登记行（`service.rs::undo_archive`，869–913） |
| 3 | §3.1 / §5 / §6 的 `TableRef` 规格 | 引用强度徽标 / 失效警告条 / 「立即校验」 | `ArchiveKind::TableRef` 已划掉（2026-10-13），保留兼容读取回退 `file`（`model.rs::from_db_str`） |
| 4 | 状态行 / 各处测试数 | 127 / 152 等 | 实际 162 单测 + 25 面板窗口 + 10 对话框窗口（2026-10-13 实测） |

## 4. 建议排序（供 B 组决策）

| 优先级 | 项 | 理由 |
| --- | --- | --- |
| P0 | 提交 T18–T21 洞察联动（§2 缺口 1–4、7）+ 验证记录 | 代码已实现且编译 / 测试全绿；未提交状态使风险悬置（与 EDA 中间页同批，提交前跑全仓 `cargo test --workspace -j 2`） |
| P1 | copy/reference 归档 UI（缺口 #6） | 用户已拍板「两档都做」，数据层就位，UI 是唯一未兑现部分 |
| P1 | A2 data 视图决策（缺口 #5：本期做 / 不做） | 决定洞察归档的完整形态；不做则需在文档标注「当前仅 file 档」 |
| P2 | 原型文档修订（§3 的 4 条过时文本 + 新增增量规格节） | 本次已随评估一并落地（`prototype-design.md` §12） |
| P3 | 收尾项 §1 各条 | 低优先，随各模块接线顺带完成 |

## 5. 2026-10-22 验证记录（编译 + 测试）

| 项 | 结果 | 说明 |
| --- | --- | --- |
| `cargo check`（rds-workbench / rds-insight / rds-analytics-resource / rds-engine / rds-editor / rds-database） | ✅ 全绿 | 仅 rds-editor 1 个 dead_code warning（`open_value_preview_with_window` / `value_preview_for_test`） |
| `cargo test -p rds-insight` | ✅ 234 单测 + 14 e2e + 4 契约 + doctests | 全绿，含 `compute_column_overlap` / `export_table_snapshot` 单测 |
| `cargo test -p rds-analytics-resource` | ✅ 25 面板窗口测试 | 全绿（对话框窗口由面板测试覆盖） |
| `cargo test -p rds-engine temp_table` | ✅ 通过 | 含 `test_lazy_cleanup_skips_protected_tables` / `test_lazy_cleanup_max_count_skips_protected` / `test_perform_lazy_cleanup_skips_protected` |
| `cargo test -p rds-workbench` | ✅ 全套件绿 | 含修复后的 `insight_editor_entry`（2）/ `insight_source_real`（2）/ ui_contract（9）等 |
| 修复项 1 | `crates/insight/src/workspace.rs` 括号失配 | EDA 中间页大改造（+1513/-340）引入，`impl InsightWorkspacePanel` 未闭合；用户侧已修，编译恢复 |
| 修复项 2 | 7 处旧签名调用 | `profile_source_column` / `profile_source_table` 新增 `full: bool` 参数后，`insight_editor_entry.rs:238` 与 `insight_source_real.rs:153/183/196/335/354` 仍用旧签名 → 本轮补 `false`（保持取样语义），编译通过 |
| 复核②（双实例，§13）| `cargo check --workspace` ✅ / `cargo test -p rds-analytics-resource --lib` ✅ 169（163 存量 + g001–g006）/ `cargo test -p rds-workbench --lib` ✅ 90 | 双实例服务层 + 接线后全绿；提交 `505e1c9d`（10 files +1637） |
| 复核③（M6 收口批，2026-10-28）| `cargo test -p rds-analytics-resource --lib` ✅ **170** / `--test dialog_window` ✅ **10** / `--test panel_window` ✅ **25**（含空态 / 无匹配 / 骨架屏 / 只读提示等断言）| 批1–4（A1 冲突三选、B1 copy/reference 归档单选、B2 失效条两动作、B3 回收站模式文案、A5 全局空态分流、A4 全局轻量详情 + 动作收敛、D2 首次提示条）全落地后视图收尾；提交批次见 m6-finalization-plan §4 |
