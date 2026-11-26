# 资源库模块完整审计（2026-11-27）

> 审计对象：`crates/analytics_resource`（项目名 `rds-analytics-resource`）。
> 审计基线：`feat/duckdb-ownership-m6-slim` 合流 `main` 后，HEAD `dcdf13b9`（`8f5ff73f` 为 merge commit），领先 `main` 19、落后 0。
> 方式：分层与调用链静态追踪 + 全量测试 + clippy + 设计文档对照。除合流后修正一条写反的测试断言外，**未改动业务代码**。
> 证据口径：标注「已确认」指源码路径可闭合；「未运行时复现」指由代码推演成立、尚未造数据跑一遍。

## 1. 结论

模块的整体骨架是成立的：`model → payload → store → service → jobs → 视图` 的依赖方向基本守住，删除语义（文件解锁回草稿箱、表型软删/永久删、唯一回收站入口、全局条目并入）已经落到服务层硬闸门，而不是只靠 UI 隐藏按钮。合流后测试与编译基线全绿。

但资产身份模型有一个必须先修的洞：**项目侧把「显示名」当成版本链身份，却没有唯一性和类型校验**。正常操作（先后归档两个同名文件、结果集与文件同名）即可制造同名资产；随后「解锁」又按名字清链，会把同名资产一起删掉。这不是边界特例，是名称模型本身没有收口。

| 等级 | 数量 | 概述 |
| --- | --- | --- |
| P0 | 2 | 显示名可重复且被当作身份；解锁按名清链会误删同名资产 |
| P1 | 4 | 引用型解锁 API 缺口；全局任意版本无 UI；文件+派生表未成组；引擎测试默认并发基线不稳 |
| P2 | 5 | 跨介质补偿缺口；回收站 scope 死分支；文档漂移；分层倒置；clippy/体量债 |

**建议先做 P0 的两刀，再谈功能补全。** 在这两个问题修掉之前，不建议继续扩展资产动作面。

## 2. 合流与验证基线

### 2.1 合流

- 三处 engine 冲突按「main 的 `AnalysisInstance` 底座 + 本分支的 owner 隔离与全局写闸门」合并：
  - `analysis/global.rs`：`ensure_base` → `global:{owner}` 会话 → `ensure_alias_attached`；
  - `duckdb/instance.rs`：会话四字段并集（`role` / `default_catalog` / `primary` / `primary_note`），`use_catalog` 走 `run_with_timeout` 并补 `set_default_catalog`；
  - `tests/analysis_global_probe.rs`：取 main 整版。
- 合流后补回 main 侧被自动合并漏掉的 `control_has_database`，并修正 `global.rs` 顶部与 `open_global_session` 仍写「根连接」的过期注释。
- 合并提交 `8f5ff73f`；随后 `dcdf13b9` 收口注释与一条测试断言。

### 2.2 基线

| 套件 | 结果 |
| --- | --- |
| `cargo check -p rds-engine -p rds-analytics-resource --lib -j 2` | 通过（仅 workspace 既有依赖告警） |
| `cargo test -p rds-engine --lib -j 2 -- --test-threads=1` | 347 passed |
| `cargo test -p rds-engine --lib`（默认并发） | 5 failed（见 P1-4） |
| `cargo test -p rds-analytics-resource --lib` | 291 passed |
| `cargo test -p rds-analytics-resource --test panel_window` | 29 passed |
| `cargo test -p rds-analytics-resource --test dialog_window` | 11 passed（按 `trash` 过滤为 1） |
| `cargo check -p rds-workbench --lib -j 1` | 通过（仅 `data_objects/mod.rs` 既有未使用告警） |

测试总数为 **331**（291 + 29 + 11），README 里的 318 是旧快照。

## 3. 规模与结构

- 166 个 `.rs`、36,929 行、约 1.65 MB。
- 最大文件：`global/tests.rs` 75.7 KB、`present/tests.rs` 34.8 KB、`global/distribute.rs` 31.3 KB、`global/delete.rs` 28.9 KB、`detail_view/body_sections.rs` 27.6 KB、`service/trash.rs` 26.8 KB。
- 目录职责：

| 目录 | 职责 | 审计判断 |
| --- | --- | --- |
| `model` | 纯领域类型、判据 | 基本纯；`ORIGIN_RESOURCES` 依赖 `payload` 常量（P2-4） |
| `payload` | 文件本体、版本副本、路径守卫、回收站搬运 | 边界守卫完整，未见越界写 |
| `store` | SQLite 登记、版本快照、回收站行、分发回执 | 最需要收口的是名称/链语义（P0） |
| `service` | 归档、取回、删除、解锁、还原、裁剪 | 产品规则闸门主要在这里，方向正确 |
| `jobs` | 异步作业、快照装配、回执 | 失败语义大多有说明，少数跨介质补偿不齐（P2-1） |
| `global` | 全局升级、分发、回收站、引用、版本 | 逻辑最重；有对 `resource_view` 的倒置依赖（P2-4） |
| `present` / `view_model` | 行 DTO、文案、格式化 | 本身干净，但被 `global` 反向借用 |
| `resource_view` / `detail_view` / `dialogs` | 面板、详情、对话框 | 无裸 hex / 裸 px，动作判据与 UI 一致 |
| `resource_host` | 视图动作 → 作业/后端端口 | 回收站端口转接已闭环 |

依赖方向没有形成硬循环，但有**名义分层与真实依赖不一致**的地方，见 P2-4。

## 4. 主要发现

### P0-1 显示名可重复，却被当作资产身份（链主键）

**已确认。**

证据链：

1. 写入口不校验名字唯一：`store/resource/write.rs::insert_archive` 直接插入；迁移 `007_analytics_resources.sql` 只有普通索引 `idx_ar_name`，没有唯一约束。
2. 归档只避让**路径**，不避让名字：`service/archive.rs::archive_new` 仅用 `find_archive_by_rel_path` 预检；`resource_host/mod.rs::archive_seed` 只调 `free_rel_path`；`request_archive_from_file` 的默认名是文件名去扩展名。
3. 读侧把同名当同一链：`store/resource/read.rs::list_alive_resources` 按 `name` 去重、`latest_alive_by_name` 取同名最新行；`table_asset.rs::register` 命中同名行就 `append_table_version`，**不校验类型**。
4. `append_table_version` 只更新 `analytics_table/content_hash/version/parent_version_id`，不清 `file_rel_path`、不改 `payload_mode`；文件行被接上表版本后会变成「kind 仍是 file、本体是文件、解析却按 AnalyticsTable」的混合行。

可触发路径（正常 UI 操作，不是构造脏数据）：

- 先后归档 `A/orders.csv`、`B/orders.csv`：第二次路径被自动改成 `orders-2.csv`，但默认显示名仍是 `orders`，两条存活行同名。
- 已有一个文件资产 `orders`，再把名为 `orders` 的结果集「升级为资产」：会接到文件行上，文件资产被改成表型混合行。

影响：

- 「数据对象」等按 `list_alive_resources` 取数的面只显示一条，另一条被静默隐藏。
- 同名文件/表被错误合并成一条版本链，之后的删除、pin、版本回退都指向错误资产。
- 为 P0-2 的误删提供前置条件。

建议修法（按最小改动排序）：

1. **项目作用域加「存活名唯一」约束**：服务层在同一事务内预检 + 迁移补部分唯一索引（`scope='project' AND deleted_at IS NULL`）。全局不能照搬，全局是一版一行。
2. 归档对话框在名字冲突时自动追加 `-2` 并明确告知，和路径避让同一套口径；不要静默保留重名。
3. `table_asset::register` 接链前校验类型档：文件链不接表版本，表链不接文件版本；跨类型同名直接报冲突或另起链。
4. 补三条回归：同目录两次同名文件归档、文件与结果集同名、重命名制造同名。

**为什么是 P0**：数据不会立刻丢，但资产身份会被错误合并，且后续删除存在连带丢失；属于「正常操作可触发、后果不可逆」。

### P0-2 解锁「按名字清链」，会连带删除同名资产

**已确认。**

证据链：

- `service/unlock.rs::unlock_file_to_drafts` 在把当前内容复制回草稿箱后，调用 `purge_name_chain(&resource.name, "project")`。
- `store/resource/trash.rs::purge_name_chain` 的查询是 `WHERE name = ?1 AND scope = ?2`，**包含软删行**，逐行 `hard_delete_row` / `purge_deleted_row`。
- 项目侧版本是同一行上的 `version + 1`（`commit_new_version` / `append_table_version`），不是一版一行；因此同名多条存活行是**不同资产**，不是版本链。`purge_name_chain` 的注释「同一条版本链」在项目作用域不成立。

影响：用户对 `orders` 执行「解除锁定并退回草稿箱」时，另一个同名 `orders` 资产（甚至已在回收站里的同名条目）会被永久删除，且其 `versions/<id>` 副本目录不会跟着清理，留下孤儿副本。这是**静默数据丢失**。

建议修法：

- 解锁只按 **resource id** 删当前资产及其版本快照/副本：`hard_delete_row(id)` + `remove_all_version_copies(id)`；不要经过 name。
- 若未来确实需要「同名链成组删」，链关系必须落成显式列（如 `parent_resource_id` / `chain_key`），不能拿显示名当链键。
- 回归：两条同名不同 `file_rel_path` 的资产，解锁其一后另一条仍在、其版本副本仍在；同名软删条目不被连带永久删除。

### P1-1 引用型文件没有可用的「解锁」路径，且文档把删除口径写宽了

**已确认（代码路径），未运行时复现。**

- `PayloadMode::Reference` 的 `file_rel_path` 是规范化后的**绝对**源路径（`service/archive.rs::registration_key`）。
- `unlock_file_to_drafts` 却走 `payload.resolve(rel)`，该函数明确拒绝绝对路径；引用型调用会得到「不允许绝对路径 / 根相对路径」的底层错误，而不是可执行动作或清晰拒绝。
- UI 侧已把引用型排除在 unlock 之外（`detail_view/actions.rs` 的 `is_file_asset = kind==File && !is_reference`；`resource_host/delete_actions.rs` 只对 `Copy` 解锁），改为「只删登记、源文件不动」的回收站路径。因此当前不是用户死路，但服务 API 与 UI 口径不一致。
- README 的能力表写「文件型资产不可直接删除」，没有给引用型这个例外，和 `service/trash.rs::trash_one` 的 Reference 分支矛盾。

建议：`unlock_file_to_drafts` 开头显式拒绝 Reference 并给出「引用型请走移入回收站（只删登记）」的可读理由；或者补一条 `resolve_source` + 复制到草稿的实现。文档同步补例外。

### P1-2 全局任意版本恢复只有后端，没有 UI

**已确认。**

- 后端：`global/promote.rs::restore_global_file_version` / `restore_global_table_version` 接受任意版本号。
- UI：`detail_view/actions.rs` 只有「恢复上一版文件…」「恢复上一版表…」，`resource_host/detail_actions.rs` 也只构造上一版确认框。
- 这是已知待办（全局任意版本选择 UI），不是新发现，但仍应作为 P1 功能缺口记录：版本区已经能看到历史，却只能回退一格。

建议：详情版本时间线行内加「恢复此版本」；全局文件/表共用同一选择器，确认框写清「会生成新版本，不原地换回」。

### P1-3 文件资产与派生表仍是两条独立行，删除/pin 不成组

**已确认（设计待办 §10.5）。**

- 文件资产 + 派生表在全局产生两条行；当前解锁、回收站、pin 都按单条处理。
- 只删文件会留下派生表引用/孤儿表；只删表会留下文件资产。
- 不能靠显示名成组（见 P0），应按 `origin_kind='global_file'` 或显式来源键成组。

建议：先把「同源组」落成查询可用的键，再让 unlock / trash / purge / pin 在组级别前置校验；没有组键之前不要做批量删除。

### P1-4 引擎测试默认并发基线不稳

**已确认（合流后实测）。**

- 默认并发首跑 5 挂：`duckdb::federation::session` 的 4 条 + `global_init` 的同键冲突；全部顺序跑通过（347）。
- 其中一个失败是测试断言本身写反（清空 `bases` 后断言指纹「非空」，实现明确返回空串）；已修正。
- 其余是测试隔离问题：同一进程内共享 `RDS_HOME` / 全局底座状态，并发时互相打开同一个 `analytics.duckdb`，触发「同一文件只能一条根连接」。
- 这会让「默认 `cargo test`」红，不能作为可接受的验收基线。

建议：给这些用例加串行锁或每用例独立 `RDS_HOME`；如果短期不改，CI 与文档必须明确 `--test-threads=1`，不能把默认并发当绿。

### P2-1 跨介质两步写入的补偿不齐

- `global/distribute.rs` 表型分支在 `register_project_table` 失败后会 `drop_project_table_if_exists`，文件分支在 `register_project_file` 失败后**不删刚写的 `resources/` 文件**，留下孤儿文件。
- `global/promote.rs` 创建全局表后若 `register_global` 失败，**不 DROP 刚建的表**（只有写表失败和 restore 路径有清理），留下不可见孤儿表。
- `unlock_file_to_drafts` 的清链是逐行事务，P0-2 修完后单 id 路径会自然变简单；但在修之前中途失败会留下部分删除。

建议：把「先建后登记」的失败补偿统一成一个小助手，登记失败一律按介质回滚；补偿失败必须进日志与回执，不能只 `let _ =`。

### P2-2 回收站 `TrashScope::Global` 已成死分支

- `jobs/archive/trash.rs::load_trash` 固定返回 `TrashScope::Project`，全局行并入同一清单。
- `dialogs/trash/mod.rs` 仍保留 `Global` 变体、标题/空态/清空确认分支；`open.rs` 的 `scope == Global` 描述永远不可达。
- 属于合并后未收口的半状态：不造成错误，但会误导后续维护者以为还有独立全局回收站入口。

建议：删除 `Global` 变体与分支，或者真正保留两种 scope（目前产品口径是前者）。

### P2-3 文档与代码漂移

- README 能力表仍写「面板头有四项」，实际 `HeaderMenuAction` 只有三项（回收站已撤）；`resource_host/mod.rs` 与 `scratchpad_view/chrome.rs` 注释仍写四项。
- README 验证段仍是 318 项旧快照，实际 331（291 + 29 + 11）。
- README 编号跳号（缺 `6.`）。
- 「文件型资产不可直接删除」未写引用型例外（P1-1）。
- `analysis/global.rs` 的历史注释已在本轮修正；后续涉及底座口径的注释仍要跟着 `AnalysisInstance` 走。

建议：文档回写和 P0 修复同批做，避免「代码改了、口径还停在上一版」。

### P2-4 分层倒置（无硬循环，但方向不干净）

- `global/list.rs::to_global_rows` 直接构造 `resource_view::ArchiveRow` 并调 `present::format_size/format_relative_time`：业务/领域层反向依赖视图 DTO。
- `present/rows.rs` / `present/snapshot.rs` / `present/trash.rs` 依赖 `global::DispatchUsage` / `GlobalTrashEntry`；`present/format.rs` 又依赖 `resource_view::row_tail`。`present` 事实上是「跨层适配层」，不是纯格式化层。
- `model/types.rs::ORIGIN_RESOURCES` 取 `payload::RESOURCES_DIR_NAME`：领域常量依赖文件层。

建议：把全局行 DTO 组装移到 `present` 或 `view_model`；`present` 只依赖 `model` + `view_model`；`ORIGIN_RESOURCES` 的字面量源头放到 `model`，由 `payload` 引用它，而不是反过来。

### P2-5 代码体量与风格债

- clippy 常用档 0 error；告警以风格类为主：`collapsible_if` 129 条、复杂类型 27 条、参数过多 5 条等。未见 correctness 级别告警。
- 非测试代码没有 `unsafe`；`unwrap` 仅出现在测试模块；生产 `expect` 只有少数「刚赋值必为 Some」的局部不变量，风险低。
- UI 侧未发现裸 hex / 裸 px / `rgb()`，主题 token 口径守住了。
- 大文件集中在测试（`global/tests.rs` 75.7 KB）与两个业务文件（`global/distribute.rs` 31.3 KB、`global/delete.rs` 28.9 KB）。测试外置做过一轮，但 `global/tests.rs` 仍是单文件巨石。
- `list_alive_resources` 用 `Vec` 做 `seen.iter().any` 去重，规模大时是 O(n²)；当前资产量下可接受，属于低优先级。

建议：不为了风格分单独开重构刀；跟随 P0/P1 改动顺手收掉触碰到的 `collapsible_if`，测试文件按「升级/分发/回收站/引用」再分一次。

## 5. 与设计文档的关键偏差

| 口径 | 文档 | 代码 | 判断 |
| --- | --- | --- | --- |
| 文件型删除 | 文件型不可直接删除，只能解锁 | Copy 文件硬闸门；Reference 可移入回收站（只删登记） | 文档缺例外（P1-1） |
| 回收站入口 | 唯一入口在草稿箱栏，面板头已撤 | 代码三项菜单，端口转接完整 | 代码对，注释/README 旧（P2-3） |
| 全局回收站 | 只收表型 | `trash_global` 对 `kind=file && file_rel_path` 明确拒绝 | 一致 |
| 全局文件删除 | 只能解锁回项目草稿箱 | `delete_global` 已撤，只剩 `trash_global`/`purge_global`，文件被闸门拒绝 | 一致 |
| 全局条目清单 | 并入同一对话框 | `load_trash` 三路合并且时间倒序 | 一致，但 scope 死分支未清（P2-2） |
| 同名链 | 文档按「版本链」叙述 | 项目侧一资产一行；同名多行是不同资产 | 语义没有落成显式键（P0） |
| 验证基线 | 318 项 | 331 项（合流后） | 文档过期（P2-3） |

## 6. 建议实施顺序

1. **P0-1 名称身份收口**：唯一性约束 + 归档/重命名/登记三处预检 + 跨类型拒绝；补同名回归。  
   验收：两条同名资产不能同时存活；若产品选择自动改名，则 UI 必须显示改名结果；文件与结果集同名不再合并。
2. **P0-2 解锁按 id**：去掉 `purge_name_chain(name)` 调用，改单 id 删除；补「解锁一条、另一条同名仍在」的用例。  
   验收：同名资产解锁互不影响，回收站同名条目不被连带清空。
3. **P1 功能面**：引用型解锁口径（实现或明确拒绝）、全局任意版本选择、文件+派生表成组键。  
   验收：三条都先落「动作判据 + 服务层闸门 + 回归」再上 UI。
4. **P1-4 测试隔离**：串行锁或独立 RDS_HOME；修完后默认并发必须全绿，才能把 README 的验证口径写成默认命令。
5. **P2 清理**：TrashScope、文档、分层、clippy 顺手项。不要单独开大体量重构刀。

## 7. 审计边界

- 已覆盖：模块结构、分层依赖、归档/取回/删除/解锁/回收站/全局升级/分发/版本恢复的代码路径、测试与文档口径、UI 契约扫描、clippy。
- 未覆盖：真机多项目并发操作、十万级以上资产的列表性能、Windows 网络盘/跨卷搬运的现场行为、历史存量数据的同名分布。
- P0 的同名场景是源码路径推演（**未运行时复现**）；修复时应先补一条最小集成测试把现状钉住，再改行为。
- 本轮只改了合流必需的三处 engine 冲突与一条写反的测试断言；审计发现均未落代码。
