# Shared 状态收窄与宿主服务下沉计划（P0 / P2 实施蓝图）

> 本文是 `panels-modules.md` §5 P0/P2 与 `panels-coupling-plan.md` A' 路线的实施蓝图。
> 数据均来自 2026-10-29 对当前 revision 的实测（源码行号/引用计数），不包含未验证的推断。
> 前置阅读：`panels-modules.md`（P0/P1/P2 定义）、`panels-coupling-plan.md` §8/§9（防环实测与 A' 模式）。

## 1. 目标

把 `workbench/src/panels/shared.rs` 中**特性 crate 的数据与状态**归还各特性 crate，让：
- workbench 只保留**宿主级装配状态**（Dock 三模式、面板句柄、端口、宿主协调通道）；
- `services/editor_*`（5253 行）与 `resource_host`/`resource_jobs`（3022 行）具备下沉条件（不再依赖 `Shared` 的特性字段）；
- 编译收益：改编辑器/资源库宿主逻辑时不再触发 workbench 全量重编。

**收益口径**（可验证）：workbench 单 crate 26,204 行 → P2-a 后 21,316（-18.6%）→ P2-b 后 18,413
（阶段 -13.6%，两阶段累计 -29.7%）→ P2-c 后 **17,049**（阶段 -7.4%，三阶段累计 **-34.9%**）。

## 2. Shared 现状（47 个字段，2026-10-29 实测）

`shared.rs` 结构：`Shared` struct（L249–395，47 字段）+ `impl Shared`（L397–900，方法）+ `impl ConnectionDialogHost for Shared`（L904–928）。

### 2.1 宿主级（保留在 Shared，约 20 个）

| 字段 | 用途 |
|---|---|
| `active_left` / `active_right` / `left_mode` / `right_mode` / `left_open_panels` / `right_open_panels` / `left_mode_before_hidden` / `right_mode_before_hidden` | 活动栏与 Dock 三模式（宿主装配，`apply_*_mode` 消费） |
| `quick_open` / `settings_open` / `layout_locked` / `prev_left_sidebar_width` / `prev_right_sidebar_width` | 面板开关与宽度同步 |
| `notice` | 状态栏回执（`say()`） |
| `editor_bridge` / `scratchpad_bridge` / `resources_bridge` | 三个宿主桥（装配期注入） |
| `host_redraw` | 宿主重绘桥（模态层挂载点） |
| `nav_panel` / `mock_panel` / `mock_details` / `insight_panel` / `insight_workspace_panel` / `resources_panel` / `property_panel` / `search_results_panel` / `metadata_search_panel` | 面板弱句柄（宿主持有） |
| `open_mock_detail` / `open_property` | 需要 window 的 tab 创建命令 |
| `project_new_request` / `project_open_request` | 连接对话框 → 宿主动作标记（已方法化：`take_project_action_request`） |
| `driver_catalog` | 驱动目录（导航/徽标/属性面板跨域共用，归宿主级） |
| `scratchpad_search` / `search_results_dismissed` / `metadata_search` / `metadata_search_dismissed` | 搜索结果全局数据源 + 标签页开关（宿主协调） |

### 2.2 特性级（迁移目标，约 27 个）

| 域 | 字段 | workbench 引用面（实测） | 归位目标 |
|---|---|---|---|
| nav | `connections` | 26 处 | `database::nav_view` 自持（`NavViewState`） |
| nav | `selected` | 13 处 | 同上 |
| nav | `nav_cache_epoch` | 5 处 | 同上（缓存失效戳随缓存数据） |
| nav | `reveal_request` | 2 处 | nav 自持（已私有 + `request_reveal`/`take_reveal` 方法化） |
| nav | `query_request` | 2 处 | nav → editor 命令（已方法化，改端口时消） |
| project | `project` | 31 处 | `project` crate（`OpenProject` 归 project） |
| project | `project_ui` | 22 处 | `project` crate（已是 `project::ui::ProjectUiState`） |
| scratchpad | `scratchpad_file_info` | 3 处 | `scratchpad` crate |
| scratchpad | `open_file_request` | 4 处 | scratchpad → editor 命令（已方法化） |
| insight | `pending_insight_target` / `pending_insight_compare` | — | `insight` crate（D65 双栏单源机制） |
| mock | `pending_temp_cleanup` | 3 处 | `mock` crate（切项目清理重试标记） |
| M6 | `version_dialog` / `version_diff_dialog` / `repair_dialog` / `trash_dialog` / `tag_dialog` / `conflict_dialog` | — | **建议不动**（见 §4） |
| M6 | `reference_stale` | — | 同上 |

## 3. 迁移顺序（按依赖与收益排序）

### 阶段 A：零风险 / 低引用面（各 1–2 小时）

> **2026-10-29 实施前修订（重要）**：原列的两个候选经逐点代码审视**均判定不迁移**，阶段 A 从执行计划中移除：
>
> 1. **`pending_temp_cleanup`（mock 域）**：置位方 `components/project_host.rs:175`（切项目时），消费方 `components/mock_host.rs:91-93`（任务收尾拍）。语义是**宿主间协调标记**——切项目时 Mock 面板实体可能尚未创建（从未打开过 Mock 面板），Shared 字段是唯一能承载"进程级待办"的位置；迁入 `MockPanel` 自持会在面板未装配时丢标记。判定：**留 Shared**，归宿主协调类（与 M6 flow 同理）。
> 2. **`scratchpad_file_info`（scratchpad 域）**：写方 `components/scratchpad_host.rs:134`（`show_scratchpad_file_info`），读方 `view.rs:2419-2438`（`render_scratchpad_file_info_popover`，宿主 overlay 渲染）。语义是**视图 → 宿主 overlay 的请求通道**，且弹窗内容由 `scratchpad::scratchpad_view::render_scratchpad_file_info_pane` 渲染——迁移需把"读方"改经面板实体弱句柄，但弹窗生命周期（每帧读 + 开窗即清）与面板挂载状态耦合，收益（1 个字段离开 Shared）远小于引入的弱句柄竞态风险。判定：**留 Shared**。
>
> **结论**：Shared 中真正干净的"特性数据"只有 nav 域（`connections`/`selected`/`nav_cache_epoch`）与 project 域（`project`/`project_ui`），即阶段 B/C。阶段 A 移除后，P0 从"四阶段"收敛为"三阶段"（B → C → D），起点即 nav 域（收益最大）。

1. ~~`pending_temp_cleanup`（mock 域，3 处引用）~~：**已修订**——宿主间协调标记，留 Shared（见上）。
2. ~~`scratchpad_file_info`（scratchpad 域，3 处引用）~~：**已修订**——请求通道，留 Shared（见上）。

### 阶段 B：nav 域（最大块，收益最高，3–5 小时）

> **2026-10-29 实施前实证修订（重要）**：本阶段的 `connections` / `selected` / `nav_cache_epoch`
> 三字段经逐点引用面实测后**判定不迁移、保留 Shared（宿主级全局状态）**，理由（证据链）：
>
> 1. **`connections` 是跨模块全局连接清单**（写方 4 处：`panels/shared.rs:924` apply_saved_connections /
>    `components/nav_host.rs:394` reload / `components/project_host.rs:228` 切项目 /
>    `panels/editor.rs:406` 连接对话框确认；读方 6 处：`components/nav_host.rs:64`（NavView 渲染）/
>    `components/editor_host.rs:34`（editor 端口）/ `components/mock_host.rs:121`（mock 刷新）/
>    `components/cache_dialog.rs:80` / `panels/shared.rs:668`（selected_connection）/
>    `panels/shared.rs:781`（insight_sample_sql 取驱动引号））——**不是 nav 视图私有数据**；
> 2. **迁入 NavView 自持会引入面板生命周期竞态**：NavView 是左 dock 面板**可被用户关闭**（weak 句柄
>    `shared.nav_panel` 变 `None`），此时写入方（连接保存 / 项目切换）若经 `nav_panel.update` 回填则
>    数据丢失，用户重开导航面板时连接清单为空；editor / mock / cache 读取方同样依赖 NavView 装配——
>    与蓝图 §3 阶段 A 修订对 `pending_temp_cleanup` / `scratchpad_file_info` 的判定逻辑同源
>    （「面板未装配时丢数据」的宿主级状态）；
> 3. **`nav_cache_epoch` 是跨模块失效信号**（写：mock/project/nav/view；读：`panels/editor.rs:101`
>    分析库缓存同步）——本质事件总线，不是 nav 数据；
> 4. **NavHost::connections() / selected_index() 保留**：它们是 NavView 渲染的合理读端口
>    （MockHost 同构），删除即断 NavView 数据源。
>
> **结论**：nav 域 3 字段归**宿主级**（与 §2.1 同类），阶段 B 从执行计划中移除。
> **替代收窄（可选，未做）**：字段私有化 + 方法化收敛（connections_snapshot() 等访问器），
> 为将来独立「连接服务」留缝——收益小（不减少行数 / 编译单位），未列入主线。
>
> ~~`connections`（26 处）+ `selected`（13 处）+ `nav_cache_epoch`（5 处）+ `reveal_request`（2 处）+ `query_request`（2 处）迁入 `database::nav_view`：~~ **已修订，见上。**

### 阶段 C：project 域（3–4 小时）

`project`（31 处）+ `project_ui`（22 处）：

1. `OpenProject` 会话归 `project` crate 自持（`ProjectHost` 持有）；
2. `project_root()` 的 31 处引用改经 `ProjectHost` trait（`components/project_host.rs` 已存在）访问；
3. `project_ui` 的 22 处引用改经 `project_host` 端口。

### 阶段 D：scratchpad / insight 域（各 2–3 小时）

- `open_file_request`（已方法化）：把「请求 → 宿主 render 消费」改为「事件路径直接调 editor 端口」（P1 语义：副作用从 render 回事件路径）；
- `pending_insight_target` / `pending_insight_compare`：D65 双栏单源机制迁移到 insight crate（面板自持 + 订阅，宿主只留 tab 激活命令）。

## 4. 明确不做：M6 对话框 flow（6 个 + reference_stale）

`version_dialog` / `version_diff_dialog` / `repair_dialog` / `trash_dialog` / `tag_dialog` / `conflict_dialog` 是**宿主协调状态**：

- 语义：工作线程无 `Window` → 先落 `pending` → 宿主 `render`（权威同步点）开窗；动作后重取换行；
- 迁移到 analytics_resource 面板自持会破坏「面板之间不互订」（发起方只有 Shared，见 `shared.rs` 各 flow 字段注释）；
- **结论**：保留在 Shared。真正的收窄动作是 P1（把「开窗」也从 render 挪到事件路径 + 端口），不是搬家。

## 5. P2-a：`services/editor_*`（5253 行）下沉 editor

> **实施状态（2026-10-29）**：✅ 已实施并提交。实施顺序按证据偏离蓝图 §7（P2-a 提前到第一阶段执行，
> 理由见 §5.4 与 §7 修订）。workbench 26,204 行 → **21,316 行（-4,888 / -18.6%）**，editor 收到 16+1
> 个端口文件（editor_* 16 个 + result_service）。`cargo check -p rds-editor -p rds-workbench` 通过
> （workbench 0 warning）；`cargo test -p rds-workbench --lib` = **47 passed / 0 failed**（editor_* 测试
> 随文件迁到 editor）；`cargo test -p rds-editor --lib` = **717 passed / 0 failed / 1 ignored**；
> `cargo check -p rds-app` 通过。

### 5.1 现状（2026-10-29 实测）

`editor_*` 16 个文件绝大多数是 **`attach` 函数**：把 workbench 侧端口实现注入 `editor::shared::EditorShared` 的 `attach_*` 槽（attach_channels / attach_completion / attach_connections / attach_schemas / attach_table_keys / attach_project / attach_runner / attach_session_store / attach_kernels / attach_sources / attach_save_path_picker / attach_export_path_picker / attach_jump / attach_history / attach_insight…）。

- **不依赖 Shared 的 7 个**：`editor_env` / `editor_exec`* / `editor_kernels` / `editor_lsp` / `editor_session` / `editor_sources` / `editor_files`（仅 `attach(&EditorShared)`）；
- **依赖 Shared 的 9 个**：`editor_catalog` / `editor_channels` / `editor_completion` / `editor_connections` / `editor_history` / `editor_insight` / `editor_project` / `editor_schema` / `editor_table_keys`——均 `use crate::panels::Shared`，读 `connections` / `project` / `project_ui`。

> `editor_exec.rs`（1711 行）虽只 `attach(&EditorShared)`，但内部引用 `crate::services::{result_service, editor_env, data_source_service}` 与 `nav_runtime`——下沉前需一并处理这些 workbench services 依赖（见 §5.3）。

### 5.2 下沉路径（A' 模式，与 MockHost/NavHost 同源）

1. editor crate 定义宿主 trait `EditorHost`（放 `editor::host`），能力面 = 9 个文件需要的 `connections()` / `project_root()` / `project_ui()` 等；
2. workbench 实现 `EditorHost`（读 Shared，放 `components/editor_host.rs`），构造时注入；
3. 9 个依赖 Shared 的文件改造：`Workbench*` → `Editor*`，`Shared` 字段 → `Rc<dyn EditorHost>`；
4. 16 个文件整体迁入 editor crate（`editor::ports`），workbench `services/mod.rs` 重导保路径；
5. 测试改造：`editor_completion` / `editor_schema` / `editor_table_keys` / `editor_project` 的测试用 `Shared::with_connections(...)` 构造 → 换 host stub（只记录调用，不接真实宿主，见 gpui-kit-dev「测试里的其他依赖用测试桥替身」）。

### 5.3 前置：editor_exec 的 workbench services 依赖（实施后修正）

蓝图初稿按早期探测量「`editor_exec` 引用 `crate::services::{result_service, editor_env, data_source_service}` 与 `nav_runtime`」，
实施前重新实测（2026-10-29）修正：

- `result_service`（178 行）：纯 facade，随 exec 一并迁入 `editor::ports::result_service`；
- `editor_env`（677 行）：不依赖 Shared，随迁（`use super::` 同目录引用迁入后仍有效）；
- `data_source_service`：**不走 `EditorHost`**。exec 的唯一跨线程需求是联邦源清单（`QueryRunner` bound 是
  `Send + Sync`，而 UI 侧实现持 `Rc<RefCell>` 的 Shared 不能跨线程）——拆成独立 trait
  `FederatedSourcePort`（`Send + Sync`），workbench 实现（`WorkbenchFederatedSource`）不持 Shared、
  直接调全局 `DataSourceService`（与原 workbench 执行线程 `block_on(service.list())` 同一条路）；
- **`nav_runtime`：0 处引用**（蓝图 §5.3 断言已过时，实施前实测无此依赖，无需处理）。

### 5.4 实施记录（2026-10-29）

**双 trait 设计（关键）**：`editor::host` 定义
- `EditorHost`（UI 线程，持 `Rc<dyn EditorHost>`）：`connections()` / `project_root()` /
  `project_read_only()` / `set_connection_connected()` / `is_connected()` / `connect_entry()` /
  `open_history()` / `open_insight_source_column(InsightColumnRequest)` / `request_reveal()`；
- `FederatedSourcePort`（`Send + Sync`，工作线程）：`list_federated_sources()` → 联邦源清单。

**迁移步骤**：
1. 16 个文件 `git mv` 到 `editor/src/ports/`（git 识别 rename 保历史）；内部 `editor::`→`crate::` 自引用替换；
2. 9 个 Shared 依赖文件 host 化：`Workbench*` 持 `Rc<dyn EditorHost>`（不再碰 `workbench::panels::Shared`）；
3. workbench `services/mod.rs` 全部 `pub use editor::ports::*` 重导——`view.rs` 的类型引用（L46-47
   WorkbenchCompletion/LspBlock）、`view.rs:837`/`resource_host.rs:526` `pick_open_path`、`view.rs:910`
   `catalog::pump` 均靠重导零改动；
4. `view.rs` assemble()：`Rc::new(WorkbenchEditorHost::new(shared.clone()))` + `federated_source()`，
   16 个 attach 调用改传 `editor_host.clone()`；
5. 依赖补齐：editor Cargo.toml +`database`（schema/table_keys/completion 的 NavCache，lib.rs 分层本已声明）/
   +`duckdb`（exec）/ +`rfd`（editor_files）/ +`uuid`（editor_env 自定义环境 id）/ dev +`arrow`（exec 测试构造 RecordBatch）；
6. 测试改造（遵 gpui-kit-dev「测试用测试桥替身」）：completion/schema/table_keys/project/channels 测试从
   `Shared::with_connections` 构造 → `StubHost`（实现 EditorHost 全方法，只记录调用）；exec 测试用
   `NoopFederatedSource`（空端口），`runner_for_test()` 保持无参签名（workbench/tests 的
   oracle_federation.rs:93 / federation_sources.rs:99 依赖它，零改动）。

**验收**：见 §5 顶部实施状态。workbench lib 测试数从 110 → 47 是**预期减少**（editor_* 的测试随文件迁到
editor crate，workbench 侧 18 个 `#[gpui_kit::test]` headless 全部保留且通过）。

**编译时间实测**：`cargo check -p rds-editor -p rds-workbench` 增量 18.91s / 11.98s / 6.86s（三次采样）。

## 6. P2-b：`resource_host` / `resource_jobs`（3022 行）下沉 analytics_resource

- `components/resource_host.rs`（原 1332 行）：构造期建 `ResourcesPanel` 实体 + 宿主端口 + 轮询回填；
- `services/resource_jobs.rs`（原 1749 行）：M6 后台任务（版本/索引/回收站/标签/冲突）；
- 资源视图已在 analytics_resource（`ResourcesPanel`），宿主服务跟进下沉（与 P2-a 同一手法：`ResourceHostBackend` trait 定义在 analytics_resource，workbench 实现注入）；
- M6 对话框 flow 留在 Shared（§4），但 flow 里引用的 `crate::services::resource_jobs::*` 行类型已随文件下沉（`VersionRows` / `IndexScanRows` / `TagRows` / `ConflictContext`）。

### 6.1 实施记录（2026-10-29，已执行并提交）

**双文件下沉 + 双向 trait 设计（与 P2-a 同构，宿主实现留在宿主 crate）**：

1. **`resource_jobs`（零依赖 workbench，先行）**：`git mv` → `analytics_resource/src/jobs.rs`；
   `analytics_resource::` → `crate::` 自引用替换（含 8 处非 use 位置残留）；依赖补齐
   `similar`（文本 diff）/ `scratchpad`（MODULE_DIR_NAME / 草稿标签）；`workbench/src/services/mod.rs`
   重导 `pub use analytics_resource::jobs as resource_jobs;`——`panels/resources.rs` / M6 对话框 flow
   的 `crate::services::resource_jobs::*` 路径零改动。
2. **`resource_host`（深度耦合 Shared 12+ 能力）**：`git mv` → `analytics_resource/src/resource_host.rs`；
   新增 **`ResourceHostBackend: 'static`** trait（约 20 个方法，全部 `owned 返回值 + &App/&mut App` 只读参数），
   `ResourceHost { backend: Rc<dyn ResourceHostBackend> }` 实现 `ResourcesHost`（面板回调）不变；
   workbench 侧新建薄层 `components/resource_host.rs`：`WorkbenchResourceBackend`（转发 Shared 能力，
   不复制任何业务逻辑）+ `build_host(shared)`（装配点 `panels/resources.rs:39` 签名零改动）。
3. **依赖补齐（无环已核）**：analytics_resource +`similar`/`scratchpad`/`settings`/`insight`/`opener`；
   settings / scratchpad / insight 均不依赖 analytics_resource，依赖方向单向 workbench → analytics_resource。
4. **过程修复的真实问题（非缓存误报）**：
   - ① 机械替换 `row_alias` 时残留孤儿代码块（`.get(resource_id)...`）导致 impl 提前闭合——已删；
   - ② `use analytics_resource::global` 函数内残留 → `use crate::global`；
   - ③ opener 依赖缺失 → Cargo.toml 补；
   - ④ **E0521（关键）**：`request_archive_from_drafts` 的 `open_draft_pick_dialog` 回调是
     `impl Fn + 'static`，多选分支里残留 `self.backend.keep_versions(cx)`（`self` 是 `&ResourceHost`，
     借用仅限方法体）→ 逃逸。修复：取值移到闭包外（`let keep_versions = ...` 在 `move` 闭包前），
     闭包内只用副本。**经验：把 `self.xxx` 调用搬进 `'static` 闭包是这类错误的唯一来源，下沉时先扫
     `self\.` 在闭包体内的残留**；
   - ⑤ settings 写方法（set_default_resource_sort / set_collapsed_groups / set_default_group）签名是
     `&mut App`（读方法 keep_versions / default_group 是 `&App`）——backend trait 按此区分，编译前核
      `settings/src/service/resources.rs` 签名，勿凭记忆。

**验证结果（全部通过）**：
- `cargo check -p rds-analytics-resource -p rds-workbench`：exit 0，无新 warning；
- `cargo test -p rds-analytics-resource --lib`：**171 passed / 0 failed**（含 jobs.rs 随迁测试
  `collision_name_keeps_extension_at_the_end`）；
- `cargo test -p rds-workbench --lib`：**46 passed / 0 failed**（原 47，-1 为 jobs 测试随迁，预期减少）；
- `cargo check -p rds-app`：exit 0（1 个既有 unused import 非本次引入）。

**行数实测**：workbench 21,316 → **18,413**（-2,903 / **-13.6%**；两阶段累计 26,204 → 18,413，**-29.7%**）。
蓝图预估 17.9k 差约 500 行：薄层新建（~190 行）+ 迁移中保留的注释与边界（~300 行），口径见 §1。

## 7. 实施顺序与验收

```
（已执行：P2-a（editor_* 下沉）→ P2-b（resource_* 下沉）→ P2-c（quick_open/model+commands 下沉），均 2026-10-29）
→ 阶段 B（nav 域：**已实证移除，见 §3 修订**）→ 阶段 C（project 域）→ 阶段 D（scratchpad/insight）
```

> 2026-10-29 修订：原阶段 A（零风险小字段）经实施前审视移除，见 §3。起点为 nav 域。
>
> **2026-10-29 实施偏离（P2-a 提前到第一阶段）**：蓝图原顺序是 B→C→D→P2-a→P2-b，实际先落
> P2-a。理由（按证据）：实测证明 P2-a 用 `EditorHost` trait 即可解耦，**不依赖先做阶段 B 的
> Shared 收窄**（§5.3 修正后 exec 的唯一跨线程依赖走 `FederatedSourcePort`，其余全走 `EditorHost`）；
> 把编译收益最大的两刀（P2-a -18.6%、P2-b 预计 -11%）先落，后续阶段 B/C/D 是特性 crate 内部的
> 状态收窄（用户口径「其他 crate 可以单独开会后调整」），不再阻塞 workbench 主线的瘦身目标。
>
> **2026-10-29 实施记录（P2-c：quick_open/model + commands 下沉 editor）**：阶段 B 经实证移除后，
> 主线继续落在「特性 crate 文件下沉」——`quick_open/model.rs`（1221 行纯数据）+ `quick_open/commands.rs`
> 下沉 `editor::quick_open`（git rename R 两文件）。要点：
> - **依赖面闭合（无环）**：model/commands 依赖 `crate::model::EditorMode`（同 crate）/
>   `workbench_shell::{model, ui}`（LeftPanel/RightPanel/QUICK_OPEN_* 常量）/ `database`（SearchHit/
>   PropertyRequest）/ `engine`（ObjectRef）/ `scratchpad`（FlatFile，editor 补依赖，scratchpad 不反依赖
>   editor）/ dev `property`（测试构造 PropertyKind，仅 serde 依赖）——editor 已覆盖全部；
> - **重导保路径**：workbench `quick_open/mod.rs` 改 `pub(crate) use editor::quick_open::model;`，
>   `view.rs`/`palette.rs`/`delegate.rs`/`tests.rs` 的 `crate::quick_open::model::*` 引用零改动；
> - **可见性**：model 全部 `pub(crate)` → `pub`（纯逻辑公开接口，可单测）；commands 保持 `pub(crate)`
>   （仅 editor 内 model 消费）；
> - **验证**：`cargo check -p rds-editor -p rds-workbench` exit 0；`cargo test -p rds-editor --lib`
>   731 passed（2 个 editor_completion 后台线程 3s 硬超时在 -j 2 并行下偶发，串行 12/12 通过，
>   P2-a 遗留测试的并发脆弱性，非本次引入）；`cargo test -p rds-workbench --lib` **30 passed**
>   （46-16，quick_open model 测试随迁）；`cargo check -p rds-app` exit 0；
> - **行数**：workbench 18,413 → **17,049**（-7.4%；三阶段累计 26,204 → 17,049，**-34.9%**）。

每阶段验收口径：
- `cargo check -p rds-workbench` 全绿（无新 warning）；
- 涉及 crate 的 headless 测试过（`cargo test -p rds-database` / `-p rds-editor` / `-p rds-mock` / `-p rds-scratchpad` / `-p rds-analytics-resource`）；
- 对外路径零改动（`lib.rs` / `services/mod.rs` / `components/mod.rs` 的 `pub use` 重导保持）；
- 一次只下沉一个模块并立即 check（`panels-coupling-plan.md` §9 注意事项 4）。

## 8. 工作量口径（诚实标注）

- 阶段 A：约 2–4 小时（含验证）；
- 阶段 B：约 3–5 小时（39 处引用 + NavHost trait 收窄）；
- 阶段 C：约 3–4 小时（53 处引用）；
- 阶段 D：约 4–6 小时；
- P2-a：约 6–10 小时（16 文件迁移 + EditorHost + 测试改造）；
- P2-b：约 4–6 小时。
- P2-c：约 1–2 小时（2 文件 git rename + editor 依赖补齐 + 重导 + 双 crate 验证，2026-10-29 实际）。
- **合计约 22–35 小时的人类工作量**，且每一步要求编译验证（本项目 -j 2 限制下每次 check 约 2–4 分钟）。
- 本计划文档落盘日期：2026-10-29；实施时若源码行号漂移，以 Grep 复核为准。
