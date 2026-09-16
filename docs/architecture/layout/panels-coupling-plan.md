# 面板跨模块耦合治理计划（P0/P1）

状态：待执行。依据见 `panels-modules.md` §3（12 个 `Shared` 字段跨模块）与 §4（18 处事件路径同步 I/O）。

## 1. 目标与验收口径

目标：**跨模块写入点从"任意字段"收敛为"少数端口方法"**，`Shared` 只保留宿主级状态。

| 验收项 | 口径 |
| --- | --- |
| `Shared` 字段收敛 | 30 → 18（宿主级） |
| 跨模块直写归零 | `grep "shared\.\(open_edit\|editor_set\|new_connection_request\|scratchpad_search\|property_target\|scratchpad_pump_request\|focus_nav_search\|open_file_request\)"` 命中 0（面板模块之间） |
| 行为不变 | 面板单测 16 项 + 契约测试 6 项 + 集成测试全绿；无 UI 文案/尺寸/交互差异 |
| 防回退 | 新增 `ui_contract` 契约：`Shared` 字段白名单（新增字段需显式登记并说明归属） |

## 2. 字段级去向

写/读列取自 2026-09-16 的 `grep` 实测（`shared.<field>` 的写入构造点与引用文件）。

| 字段 | 写入方 | 读取方 | 去向 | 形态 |
| --- | --- | --- | --- | --- |
| `notice` | editor / nav / scratchpad | 三方 + `view.rs` | **保留 `Shared`** | 三类面板共用的状态栏提示，宿主级 |
| `selected` / `connections` | editor / nav | editor / nav + 宿主 | **保留 `Shared`** | 连接列表与选中，宿主级 |
| `driver_catalog` | nav（加载） | nav / editor | **保留 `Shared`** | 只读快照（随组织数据一次性加载） |
| `project` / `project_ui` | 宿主（`view.rs`） | editor / scratchpad | **保留 `Shared`** | M1 项目会话与 UI 状态 |
| `active_left` / `active_right` / `*_mode` / `quick_open` / `settings_open` | 宿主 | 宿主 / mod.rs | **保留 `Shared`** | 布局与三模式（`rds-layout` 口径） |
| `mock_panel` / `mock_detail` / `insight_panel` / `open_mock_detail` / `host_redraw` | 宿主 | 宿主 / right.rs | **保留 `Shared`** | 宿主级弱句柄与命令 |
| `editor_sql` / `editor_dirty` | **仅 editor.rs** | editor.rs + `components/project_host.rs`（M1 `ProjectEditorBridge`） | **S2 处理**：随 `EditorService` 契约一起换实现，本步不动 | 现在搬会连带改 M1 桥 |
| `nav_for` / `nav_tables` | **仅 editor.rs** | 仅 editor.rs（+ 宿主 Quick Open 读表名快照） | **✅ S1 已收回 `EditorPanel`** | 外部失效改为 `Shared::invalidate_nav_cache()`（§3 戳） |
| `sql_for` | 仅 editor.rs | 仅 editor.rs | **✅ S1 已收回 `EditorPanel`** | 外部失效改为 `Shared::invalidate_sql_result()` |
| `property_target` | nav（5 处） | editor | **`EditorBridge::show_properties(PropertyRequest)`** | nav → editor 请求 |
| `open_edit` | nav（2 处） | — | **✅ S2a 已端口化**：`EditorBridge::edit_connection(id)` | 数据字段已删，配对 request 字段一起删 |
| `new_connection_request` | nav（2 处） | — | **✅ S2a 已端口化**：`EditorBridge::new_connection()` | 同上 |
| `editor_set` | nav（1 处） | editor / mod | **`EditorBridge::insert_sql(conn_id, sql)`** | nav → editor 请求 |
| `scratchpad_search` | scratchpad（2 处） | editor | **`EditorBridge::show_search_results(view)`** | scratchpad → editor 投递 |
| `scratchpad_pump_request` | editor（1 处） | mod / scratchpad | **`ScratchpadBridge::ensure_pump()`** | editor → scratchpad 请求 |
| `open_file_request` | scratchpad | 宿主（`view.rs`） | **`HostBridge::open_in_editor(path)`** | scratchpad → 宿主请求 |
| `focus_nav_search` | 宿主 action（nav 1 处消费） | nav | **`NavBridge::focus_search()`** | 宿主 → nav 请求 |

`editor_clear`（宿主命令闭包）与其它已由宿主持有的项不在本次范围。

## 3. 三类桥 + 一个宿主端口

桥是**强类型方法集合**，持有 `Rc<dyn Fn(...)>` 或 `WeakEntity`，在面板构造期注入（与既有接线口径一致）：

```rust
/// 导航/草稿箱调用（提供方：EditorPanel）；装配入口 `panels::install_editor_bridge`。
/// `S2a` 已落地前两项，其余待 `S2b`。
EditorBridge {
    fn edit_connection(&self, id: &str, window: &mut Window, cx: &mut App);  // ✅ S2a
    fn new_connection(&self, window: &mut Window, cx: &mut App);             // ✅ S2a
    fn insert_sql(&self, conn_id: &str, sql: &str, cx: &mut App);            // S2b
    fn show_properties(&self, request: PropertyRequest, cx: &mut App);       // S2b
    fn show_search_results(&self, view: ScratchpadSearchView, cx: &mut App); // S2b
}

/// 编辑区调用（提供方：SidebarPanel / 草稿箱）
ScratchpadBridge { fn ensure_pump(&self, cx: &mut App); }

/// 宿主调用（提供方：SidebarPanel）
NavBridge { fn focus_search(&self, window: &mut Window, cx: &mut App); }

/// 面板调用（提供方：WorkbenchView）
HostBridge { fn open_in_editor(&self, path: PathBuf, cx: &mut App); } // 已有 open_file_request 的替代
```

依据（不必重新论证，仓库内已有先例）：

- `crates/editor/src/shared.rs` 的 `EditorShared` + `services/{editor_exec, editor_files, editor_connections, editor_session}.rs` 就是端口接线；
- `rds-architecture` skill 硬约束：Feature 间协作走 **command / event / shared service**，不走共享可变字段；
- `SidebarEvent` 已证明"枚举事件"适合**宿主订阅**的少数场景；端口更适合**点对点调用 + 需要窗口句柄**的场景（本表全部属于后者）。

## 4. 迁移步骤（每步独立可编译、可验证）

| 步 | 内容 | 验收 |
| --- | --- | --- |
| **S1** ✅ | 3 个字段（`nav_for` / `nav_tables` / `sql_for`）收回 `EditorPanel`；外部失效改为 `Shared::{invalidate_nav_cache, invalidate_sql_result}` 两个戳，编辑区在 `sync_shared_epochs` 消费；Quick Open 改用 `EditorPanel::nav_table_names()` 快照 | 面板单测 16 项 + 契约 6 项全绿；`Shared` 字段 30 → 27 |

> S1 实测订正：这三个字段**并非只有 `editor.rs` 读写**——`view.rs`（切换连接 / Quick Open）
> 与 `components/{project_host,mock_host}.rs` 也在清/读它们。此前结论偏差源于审计范围只扫了 `panels/`；
> 因此 S1 必须连带改这 3 个外部文件（已改），而不是"纯字段搬家"。
| **S2** | editor 侧端口化（拆两步） | — |
| **S2a** ✅ | `EditorBridge { edit_connection, new_connection }` + 装配函数 `panels::install_editor_bridge`（生产宿主与同构测试宿主共用一份接线）；nav 4 处改为调端口；删掉 `open_edit` / `new_connection_request` 字段与编辑区 render 的两处 take（副作用回到事件路径） | `Shared` 字段 27 → 25；`dialog_host_layer` 4 项全绿（两个入口测试改走端口） |
| **S2b** | 其余 3 个请求：`editor_set`（→ `insert_sql`）、`property_target`（→ `show_properties`）、`scratchpad_search`（→ `show_search_results`）；`property_target` 的调用点在键盘路径，需把 `window` 透过去 | 5 个请求字段全归零；§1 验收命令命中 0 |
| **S3** | 反向桥：`ScratchpadBridge::ensure_pump`、`NavBridge::focus_search`、`HostBridge::open_in_editor` | 同上；`scratchpad_pump_request` / `focus_nav_search` / `open_file_request` 归零 |
| **S4** | `ui_contract` 加 `Shared` 字段白名单契约；更新 `panels-modules.md` §3 与本文档状态 | 契约测试通过；§3 表格与实际一致 |

## 5. 与 P1（同步 I/O 后台化）、P2（视图下沉）的顺序

推荐顺序 **S1 → S2/S3 → P2 → P1**，理由：

- **S1 / S2a 已完成**：缓存/归属字段回归编辑区；两个对话框请求走端口（`install_editor_bridge`）；
  遗留：`SidebarEvent::{EditConnection, NewConnectionRequest}` 现在只剩"触发重绘"，清理归 S4；
- **S2/S3 必须在 P2 之前**：视图下沉（`nav.rs` → `crates/database`）时，若协作还靠共享字段，就会连带搬走半个 `Shared`；端口化之后只搬端口；
- **P1 放在 P2 之后**：`block_on` 的后台化目标形态是"特性 crate 内的 jobs + 面板 drain"。草稿箱已有 `services/scratchpad_jobs.rs`，而它按 P2 会迁入 `crates/scratchpad`——先做 P1 等于把这段代码写两遍（nav 的 4 处同理，属 `crates/database`）。

若希望**先拿到可见收益**（消除点击卡顿）再谈结构，可把 P1 提前，但需接受 18 处 I/O 迁移会被 P2 再搬一次；此时建议 P1 只做 scratchpad 的 14 处（路径最短、正例最近）。

## 6. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 宿主级状态（收敛后） | `crates/workbench/src/panels/shared.rs` |
| `EditorBridge` / `ScratchpadBridge` / `NavBridge` / `HostBridge` | `crates/workbench/src/panels/editor.rs`、`scratchpad_panel.rs`、`nav.rs`、`view.rs`（构造期注入，宿主在 `init_workspace` 装配） |
| 端口接线先例 | `crates/editor/src/shared.rs`、`crates/workbench/src/services/editor_*.rs` |
| 后台任务形态（P1） | `crates/workbench/src/services/scratchpad_jobs.rs`、`nav_jobs.rs` |
| `Shared` 字段白名单契约（S4） | `crates/workbench/tests/ui_contract.rs` |
