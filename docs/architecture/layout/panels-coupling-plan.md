# 面板跨模块耦合治理计划（P0/P1/P2）

状态：**S1–S4 已完成**（2026-09-16）；**A1–A3 已完成**（`crates/workbench_shell` 已建，仅承 `ui.rs`）；
**A4/A5 待拍板**——实测发现原计划的下沉对象会造成新的循环依赖，依据见 §9「实测订正」。
原始依据见 `panels-modules.md` §3（12 个 `Shared` 字段跨模块）与 §4（18 处事件路径同步 I/O）。

## 1. 目标与验收口径

目标：**跨模块写入点从"任意字段"收敛为"少数端口方法"**，`Shared` 只保留宿主级状态。

| 验收项 | 口径 |
| --- | --- |
| `Shared` 字段收敛 | 原始 35 → 当前 31（S1/S2a/S2b 后）；计划终态 25（含 `editor_*` 交 EditorService） |
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
| `editor_sql` / `editor_dirty` | ~~仅 editor.rs~~ | — | ✅ **S2 遗留已接（B11/B12，2026-09-16）**：随 `EditorService` 契约一起换实现——草稿 = **未命名编辑器文档**，M1 桥改读 `EditorShared`（`components/project_host.rs`）；两个字段已删 | — |
| `nav_for` / `nav_tables` | **仅 editor.rs** | 仅 editor.rs（+ 宿主 Quick Open 读表名快照） | **✅ S1 已收回 `EditorPanel`** | 外部失效改为 `Shared::invalidate_nav_cache()`（§3 戳） |
| `sql_for` | 仅 editor.rs | 仅 editor.rs | **✅ S1 已收回 `EditorPanel`** | 外部失效改为 `Shared::invalidate_sql_result()` |
| `property_target` | nav（5 处） | editor | **`EditorBridge::show_properties(PropertyRequest)`** | nav → editor 请求 |
| `open_edit` | nav（2 处） | — | **✅ S2a 已端口化**：`EditorBridge::edit_connection(id)` | 数据字段已删，配对 request 字段一起删 |
| `new_connection_request` | nav（2 处） | — | **✅ S2a 已端口化**：`EditorBridge::new_connection()` | 同上 |
| `editor_set` | nav（2 处） | — | **✅ S2b 已端口化**：`EditorBridge::insert_sql(sql)`（编辑区入私有缓冲，渲染期 `set_value`）→ **B11/B12 再收一步**：改走 `Shared::request_query(QueryRequest)`，旧 SQL 框已删（见下方 B11/B12 行） | 事件路径拿不到窗口，故不立即写输入框 |
| `property_target` | nav（5 处） | — | **✅ S2b 已端口化**：`EditorBridge::show_properties(request)`；数据归 `EditorPanel`，入队与 loading 置位移到事件路径 | render 不再入队 |
| `scratchpad_search` | scratchpad（2 处） | editor | **`EditorBridge::show_search_results(view)`** | scratchpad → editor 投递 |
| `scratchpad_pump_request` | editor（1 处） | mod / scratchpad | **`ScratchpadBridge::ensure_pump()`** | editor → scratchpad 请求 |
| `open_file_request` | scratchpad | 宿主（`view.rs`） | **✅ S3b：已收为私有字段 + `request_open_in_editor` / `take_open_in_editor` 方法对**（生产端拿不到 `Window`、消费端必须有 `Window`，故保留一帧延迟；端口签名满足不了两边） | 外部不再能直写字段 |
| `focus_nav_search` | — | — | **✅ S3b：已从 `Shared` 删除**——实测它只由 nav 自己写读（`view.rs` 本来就走 `SidebarPanel::focus_nav_search`），已改为面板私有字段 `nav_search_focus_pending` | 字段寄存的直接证据 |

| `editor_clear` | 宿主 | `components/project_host.rs` | ✅ **已删（B11/B12）**：`ProjectEditorBridge::clear` 改由宿主关闭**未命名编辑器文档**（`WorkbenchView::close_untitled_editor_documents`） | 旧 SQL 框已删，不再需要“清空输入框”命令 |

## 3. 三类桥 + 一个宿主端口

桥是**强类型方法集合**，持有 `Rc<dyn Fn(...)>` 或 `WeakEntity`，在面板构造期注入（与既有接线口径一致）：

```rust
/// 导航/草稿箱调用（提供方：EditorPanel）；装配入口 `panels::install_editor_bridge`。
/// `S2a` 已落地前两项，其余待 `S2b`。
EditorBridge {
    fn edit_connection(&self, id: &str, window: &mut Window, cx: &mut App);  // ✅ S2a
    fn new_connection(&self, window: &mut Window, cx: &mut App);             // ✅ S2a
    fn insert_sql(&self, sql: &str, cx: &mut App);                                // ✅ S2b（缓冲）
    fn show_properties(&self, request: PropertyRequest, cx: &mut App);       // ✅ S2b
    fn show_search_results(&self, view: ScratchpadSearchView, cx: &mut App); // S2c（待迁）
}

/// 编辑区调用（提供方：SidebarPanel / 草稿箱）
ScratchpadBridge { fn ensure_pump(&self, cx: &mut App); }

/// 宿主调用（提供方：SidebarPanel）
NavBridge { fn focus_search(&self, window: &mut Window, cx: &mut App); }

/// 面板调用（提供方：WorkbenchView）
HostBridge：`open_file_request` 最终未做成端口——改为**私有字段 + 方法对**（生产端 `Enter`/右键路径拿不到 `Window`，`open_in_editor` 必须有 `Window`）。
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
| **S2b** ✅ | `editor_set` → `insert_sql`（编辑区私有缓冲：`set_value` 需要 `Window`，而「生成 SQL」的排空路径拿不到窗口）、`property_target` → `show_properties`；nav 7 处改为调端口；`Shared` 新增 4 个便利方法（`edit_connection` / `new_connection` / `insert_sql` / `show_properties`，"端口未装配时静默丢弃"的容错收在一处） | `Shared` 字段 33 → 31；面板 16 项 + 契约 6 项 + `dialog_host_layer` 4 项 + `db_navigator` 2 项全绿 |
| **S2c** | `scratchpad_search`（双向数据交接：scratchpad 写、编辑区渲染、scratchpad 自读）——需先确定展示状态归谁，再定端口形状 | 剩余 4 个字段（含 S3 的 3 个信号）归零 |
| **S3** | 反向端口，拆两步： | — |
| **S3a** ✅ | `scratchpad_pump_request` → `ScratchpadBridge::ensure_pump`（装配入口 `panels::install_scratchpad_bridge`）；删除侧栅 `Render` 里的 take 块（**又一个 render 内副作用回到事件路径**） | `Shared` 字段 31 → 30；面板 16 项 + 契约 6 项 + `dialog_host_layer` 4 项全绿 |
| **S3b** ✅ | `focus_nav_search` 已删（面板私有字段）；`open_file_request` 收为私有 + 方法对（`request_/take_open_in_editor`），两个 scratchpad 写点与 `view.rs` 读点改走方法 | 面板 16 项 + 契约 7 项全绿；`Shared` pub 字段 31 → 29（另 1 个私有） |
| **S4** | ✅ 已加 `ui_contract` 契约 4（`Shared` 字段白名单，新增字段必须显式登记）；**待做**：删除已退化为"仅触发重绘"的 `SidebarEvent::{EditConnection, NewConnectionRequest}` | 契约测试 7 项通过；§3 表格与实际一致 |
| **B11** ✅ | 编辑器对外接口：`Shared::request_query(QueryRequest{conn_id, sql, run})`（私有字段 + 方法对）+ 宿主 `WorkbenchView::open_query_document`（复用/新建绑定该连接的文档 → 注入 SQL → 可选自动执行）· 导航「查看数据」`run = true`（**关闭 M4 遗留的“查看数据不自动执行”**）、「生成 SQL」`run = false`、拖拽同路 | `Shared` 字段 29 → 27；导航 4 处改调请求；面板单测 + `dialog_host_layer` 4 项全绿 |
| **B12** ✅ | **删掉旧 `EditorPanel` 的 SQL 区块**（Textarea + 内联执行闭包 + 结果表 + 历史列表 + `use_duckdb_fed` 门控，约 250 行）+ 其状态（`sql_textarea`/`query_result`/`sql_history`/`last_executed`/`sql_for`/`pending_sql`/`_sql_sub`）与 `apply_nav_drag`/`insert_sql`/`clear_sql`；**SQL 编辑器只剩 `crates/editor`**；旧路径的“项目只读模式拦截执行”移交给宿主侧待接（记入 1b 余项） | 同上 + `Shared` 字段再减 `editor_dirty`/`editor_sql`/`editor_clear`/`result_epoch` 4 个；契约 4 白名单同步 |

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
| 外壳 crate（A1–A3） | `crates/workbench_shell/`（包名 `rds-workbench-shell`） |

## 7. P1 执行清单（未做，逐点机械可执行）

**18 处事件路径 `block_on`（阻塞 UI 线程）**：

| 文件 | 位置 | 目标形态 |
| --- | --- | --- |
| `panels/nav.rs` | `commit_copy_connection` / `share_connection_to_project` / `unshare_connection_from_project` / `delete_connection`（4 处，各自 `Runtime::new()`） | 在 `nav_jobs` 增 4 个 job 种类 + drain，照现有 `enqueue_properties` / `drain_props_results` 写法 |
| `panels/scratchpad_panel.rs` | `commit_scratchpad_edit`（6）/ `delete_scratchpad_selection`（3）/ `undo_scratchpad_delete` / `restore_scratchpad_trash` / `remove_scratchpad_reference` / `apply_scratchpad_relink` / `open_scratchpad_location`（共 14） | 入 `scratchpad_jobs` 的 job（`enqueue_import` / `enqueue_paste` 同形）+ 在 `apply_scratchpad_ops` 回填 |

注意两点：① `Shared::scratchpad_store()` 目前**每次操作新建一个 tokio Runtime**，迁移时应改为 worker 内共享运行时；② 操作后需重跑加载（现有代码就在 `block_on` 后紧接 `request_scratchpad_load`），迁移后放到 apply 阶段。

## 8. P2 就绪度评估（结论：**未就绪**，先拍一个前置决策）

实测 `panels/nav.rs` 对 workbench 内部的依赖面（次数）：

| 依赖 | 次数 |
| --- | --- |
| `crate::services::nav_runtime` | 33 |
| `shared.notice` | 33 |
| `shared.*` 其余（connections / selected / driver_catalog / project_root 等） | ~15 |
| `crate::services::{data_source_service 4, workspace_loader 1, nav_jobs 1}` | 6 |
| `crate::ui` / `crate::view` / `crate::components::group_form_dialog` | 3 |

且 `crates/database` 与 `crates/scratchpad` **目前都不依赖 gpui-kit**（纯 service crate）。

**阻塞点**：视图搬进特性 crate 后，这些依赖会变成 **workbench ↔ 特性 crate 的反向依赖**（workbench 已依赖 database/scratchpad）→ 循环依赖，按 `rds-architecture` 硬约束不允许。

前置是「把面板状态与视图依赖下沉」，三种选型（需拍板）：

| 选型 | 内容 | 代价 |
| --- | --- | --- |
| **A** | 新建中间 crate（如 `workbench-shell`）：`Shared` + `ui.rs` + `nav_runtime` 等下沉，workbench 与特性 crate 都依赖它 | 低（解循环），但不减耦合 |
| **B** | 把 `Shared` 拆为「宿主级 + 特性级」，特性级状态随视图进特性 crate（nav 的 ~70 处 `shared.*` 逐处改自持 + 端口） | 高，但最干净（与本次 S1/S2 同源） |
| **C** | 不下沉，接受视图留在 workbench | 零成本，失去“特性 crate 自带 view”的统一形态 |

建议 **A + B 混合**：先用 A 解循环（低成本），再逐特性做 B（nav → database、scratchpad → scratchpad）。

## 9. A 步拆解（可执行规格）

目标：新建 `crates/workbench-shell`（包名 `rds-workbench-shell`），承接「面板状态 + 视图共用资产」，
使 workbench 与特性 crate 都能依赖它，从而解开循环。

**关键手法：下沉 + `pub use` 重导**——与本次拆模块同一招，下游 `crate::ui::` / `crate::panels::Shared` 
等路径保持可用，**改动面为零**。

| 步 | 动作 | 验收 |
| --- | --- | --- |
| A1 | 新建 crate 骨架（`Cargo.toml` + `lib.rs` + `ui.rs`）；workbench 的 `src/ui.rs`（169 行常量）纯位移迁入 | ✅ 已做：`crates/workbench_shell/`（包名 `rds-workbench-shell`，依赖键 `workbench_shell`） |
| A2 | workbench：`Cargo.toml` 加依赖；`lib.rs` 把 `pub mod ui;` 换成 `pub use workbench_shell::ui;` | ✅ 已做：全仓 `crate::ui::*` / `rds_workbench::ui::*` 零改动 |
| A3 | `ui_contract.rs` 的 `include_str!` 路径改为指向新 crate | ✅ **无需改动**（订正）：契约 1 是 `use rds_workbench::ui::*` 直接读常量（非 include_str），且契约 2a/2b 的清单本来就不含 `ui.rs`（它正当地写着 `px(1.)`） |
| A4 | 同手法下沉 `Shared` + 纯数据枚举（`LeftPanel` / `RightPanel` / `SidebarMode` / `ConnectionItem`） | ⛔ **会造环，待拍板**——见下方「实测订正」 |
| A5 | 同手法下沉 `services/nav_runtime.rs`（478 行） | ⛔ **同样会造环**——见下方「实测订正」 |
| A6 | 反向依赖校验：`cargo tree` 无环 | ✅ 已做（A1–A3 范围）：`cargo tree -p rds-workbench-shell --depth 1` = **仅 gpui-kit**，零内部依赖；`workbench → workbench_shell` 单向 |

**注意事项**（实测所得，避免重蹈）：

1. `ui.rs` 可能用 `gpui_kit` 的 `rems()` → 新 crate 需要 `gpui-kit` 依赖（架构约束允许 Feature/crate 依赖 UI 基础设施）。
2. `Shared` 里 `editor_clear` / `host_redraw` / `open_mock_detail` 是 `Rc<dyn Fn(&mut Window, &mut App)>` → 新 crate 必须依赖 `gpui-kit`（`Window` / `App` 类型）。
3. 下沉后 `Shared` 不可再引用 workbench 内部项（如 `crate::view::ConnectionItem`、`crate::services::nav_runtime::DriverMeta`）——这些必须一并下沉或在 `Shared` 里改类型别名。
4. 每次只下沉一个模块并立即 `cargo check`，不要一次搬完再编（本会话已多次验证这个节奏最省时间）。

### 实测订正：A4/A5 会造出新的环（2026-09-16）

`sink Shared + nav_runtime` 的前提是「它们不依赖特性 crate」，**实测不成立**。把 `panels/shared.rs`
的导入与 `nav_runtime` 的引用面摊开后，会新增三条环（均为 `特征 crate → shell → 特征 crate`）：

| # | 边 | 证据 | 后果 |
| --- | --- | --- | --- |
| 1 | shell → `scratchpad` | `use scratchpad::ScratchpadStore;`（`Shared::scratchpad_store`）；`EditorBridge::show_search_results` 的载荷是 `ScratchpadSearchView`（现 `panels/scratchpad_panel.rs`） | P2 后 `scratchpad → shell → scratchpad` |
| 2 | shell → nav（P2 后的 `database`） | `EditorBridge::show_properties(PropertyRequest)`，`PropertyRequest` 现属 `panels/nav.rs`；`driver_catalog: HashMap<String, nav_runtime::DriverMeta>` | P2 后 `database → shell → database` |
| 3 | shell → `database` / `connection`（经 nav_runtime） | `nav_runtime` 引 `crate::services::{nav_store, data_source_service, connection_service}`（三者依赖 `database` / `connection` / `engine`），而 `nav_runtime` 自身又依 `database::model` | A5 直接成环 |

另两条不算环但需一并处理（属 A4 的「算位移」范围）：
`Shared` 还引用 `insight::{InsightTarget, InsightView}`、`mock::mock_view::{MockDetailView, MockPanel, SchemaRequest}`；
以及 `crate::view::{ConnectionItem, LeftPanel, RightPanel, SidebarMode}`（后者确实可直接下沉）。

**订正结论：A4/A5 不是「搬文件」而是设计题。`Shared` 是宿主状态，特性视图本不该看见它。**

仓库里已有**已验证的正例**（`crates/mock` / `crates/insight`）：视图归特性 crate，**不拿 `Shared`**，
而是拿**本 crate 定义的宿主 trait**——`crates/mock/src/mock_view.rs:355` 的 `pub trait MockHost`，
`MockPanel::new(host: Rc<dyn MockHost>, cx)`，workbench 在 `components/mock_host.rs` 实现并注入；
`mock` / `insight` 均**不依赖 workbench**（`Cargo.toml` 实测）。

因此推荐把 A4/A5 换成 **A'**：

| 步 | 内容 |
| --- | --- |
| A'1 | 在 `crates/database` 定 `pub trait NavHost`（宿主能力：读连接列表 / 选中 / 提示 / 时间片；请求编辑连接 / 插入 SQL / 打开属性面板 / 归组与排序的落库）——形状照 `MockHost` |
| A'2 | workbench 实现 `NavHost`（现 `panels/nav.rs` 的 `shared.*` / `nav_runtime::*` 调用点移入实现体），注册到 `SidebarPanel` |
| A'3 | `crates/database` 加 `gpui-kit` + `workbench_shell` 依赖，视图搬入（`panels/nav.rs` → `database/src/nav_view.rs`） |
| A'4 | 同样处理草稿箱：`crates/scratchpad` 得 `pub trait ScratchpadHost`，视图搬入 |

**A' 前置已做（2026-09-16）**：把 `ConnectionItem` / `LeftPanel` / `RightPanel` / `SidebarMode`
下沉到 `crates/workbench_shell/src/model.rs`（workbench 侧 `crate::view::{...}` 重导，路径不变）。
理由：`NavHost` 的签名要能命名「连接条目」与「面板枚举」，而这几个类型原本定义在 `workbench` 里——
不下沉则 `database` 无法定义 trait。

**A' 前置之二（2026-09-16）：驱动目录下沉至 `engine`（不是 `database`）。**

原先 `navigation` 的驱动目录（`driver id → type_id / 显示名`）实现在
`workbench::services::nav_runtime::{DriverMeta, driver_catalog}`，但：

1. 它读的是 `drivers` 表（`engine::persistence::driver_store`），**本来就是引擎侧知识**；
2. 消费方有两个 crate：`database`（连接行徽标）与 `workbench`（编辑器属性面板 `panels/editor.rs`）；
3. 放 `database` 会让 `database` 为一段查询新增 `rusqlite` 依赖，而 `engine` 已有。

故落到 `engine::persistence::driver_catalog::{DriverMeta, load}`（`persistence/mod.rs` 重导为
`DriverMeta` / `load_driver_catalog`），`nav_runtime` 不再持有该实现。
与 `engine::driver::metadata::DriverMetadata` 的分工：后者是内置驱动的**静态描述**（代码写死），
前者是库里**已注册的驱动行**（含导出的外部驱动）。

顺带修掉一个副作用：读目录改用 `SQLITE_OPEN_READ_ONLY` 打开全局库，全局库未初始化时
不再在用户目录里凭空建出一个空 `global.db`。

**A' 前置之三（2026-09-16）：导航视图状态存储下沉至 `engine`。**

`workbench::services::nav_store::NavStore`（读写 `navigator_state` 表）与它依赖的
`database::model::NavState` 一并落到 `engine::persistence::navigator_state`
（`NavigatorStateStore` / `NavState`；`database::model` 重导旧名）。
理由：它是纯持久化（读写一张表），视图下沉后 `database` 不应为此再引一份 `rusqlite`；
`NavState` 也随之与存储同层，不再是「数据库域模型兼行映射」。

**A' 前置之四（2026-09-16）：组织元数据 / 导航状态的**门面**搬进 `database`，于是「分组存储 trait」不需要了。**

原计划让 `NavHost` 再挂一个 17 方法的 `GroupStore` trait（因为 `database` 不该直连
`workbench::services::nav_runtime`）。落地时发现这层抽象多余：那 24 处调用（标签 / 分组 / 排序 /
导航状态）只依赖 `engine::persistence` + 一个 `project_root`，**不需要宿主参与**。
故直接搬为 [`database::nav_store`](../../../crates/database/src/nav_store.rs) 的自由函数
（形如 `list_groups(root: Option<&Path>)`），`nav.rs` 的 24 处调用点已改为
`database::nav_store::*`（`nav_runtime` 只留连接生命周期：`connect_entry` /
`disconnect_entry` / `is_connected` / `test_entry` / `build_connect_request`）。

**A' 前置之五（2026-09-16）：后台任务模块 `nav_jobs` 搬进 `database`。**

19 处调用只在导航视图里，且该模块只依 `engine` + `database` 自身（`NavigatorService` /
`property_panel` / `sql_gen`）。唯一的外部依赖是工作线程里的
`nav_runtime::test_entry` —— 工作线程拿不到 `Rc<dyn NavHost>`（不能跨线程），
所以端口用**函数指针**表达这个需求：

```rust
pub type ConnectionProbe = fn(conn_id: &str, project_root: Option<&str>) -> Result<String, String>;
// NavHost::connection_probe(&self) -> ConnectionProbe
```

实现以无状态函数返回（内部自取服务单例与进程级桥接运行时），天然 `Send`。
这比“把 `DataSourceService` / `ConnectionService` 搬下引擎层”小得多；后者是另一笔账（见下方「待办」）。

依赖面审计（`panels/nav.rs`，4867 行）——决定 `NavHost` 要盖什么：

| 类别 | 处数 | 去向 |
| --- | --- | --- |
| `shared.notice` | 33 | `NavHost::notice(msg)` |
| `shared.{connections, selected, driver_catalog, project_root}` | 11 | `driver_catalog` 已下沉 `engine`（见上）；其余为 `NavHost` 只读访问器 |
| 编辑器端口（`show_properties` / `request_query` / `new_connection` / `edit_connection`） | 12 | 直接复用既有 `EditorBridge`（已端口化） |
| `nav_runtime::*` | 34 | 纯存储类（24 处）**已搬** `database::nav_store`（见上）；连/断类 → `NavHost` |
| `nav_jobs::*` | 19 | ✅ **已搬** `database::nav_jobs`（2026-09-16） |
| `settings::SettingsService::*`（视图偏好 8 处） | 8 | `NavHost`（宿主自持设置访问，避免新增 `database → settings` 依赖） |
| `mock::mock_view::SchemaRequest` | 1 | `NavHost::open_mock_panel(..)`（避免新增 `database → mock`） |
| `crate::components::{group_form_dialog, cache_dialog}` | 2 | `NavHost`（对话框需 `Window`，只有宿主有） |
| `database::*` | 7 | 无需处理（本来就是目标 crate） |

> 结论：`NavHost` ≈ **宿主状态访问器（11）+ 提示（1）+ 连接开关（2）+ 视图偏好（3）+ 宿主命令（属性 / SQL / 新建连接 / 编辑连接 / 分组表单 / 缓存对话框 / Mock）**，
> 约 20 个方法；其余全部随视图进 `database`。**不再需要 `group_store()` 与 `GroupStore` trait**（见前置之四）。

代价：A' 比原 A4/A5 大（nav 的 ~74 处 `shared.*` + 34 处 `nav_runtime::*` + 19 处 `nav_jobs::*` 要逐处归位），
但它**同时完成 P2**，且不再需要「把 `Shared` 下沉」这个本身就矛盾的动作；`Shared` 留在 workbench，
shell 只承 `ui.rs` + 纯数据模型（`model.rs`：面板枚举 / 边栏模式 / 连接条目）。

若不想动 trait 面，另一条路是 §8 的**选型 B 前半段**：先把 `Shared` 中专属特性的字段
（`driver_catalog` / `editor_sql` / `editor_dirty` / `editor_clear` / `mock_*` / `insight_panel` /
`open_mock_detail`）收回各自模块，`Shared` 缩到纯宿主级（不含任何特性类型）后再下沉——
但这一步与 A'1/A'2 的工作量重叠，且下完仍要解决 `EditorBridge` 的两处特性载荷。
