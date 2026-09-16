# 资产库 / 分析存档模块（M6）· 开发方案（Phase 0–5）

> 状态：**设计定稿（2026-09-15）；Phase 0 三批 + Phase 1 三刀已落地（仅 crate 内）**——归档/取回/再归档闭环 + 变更事件 + 索引修复已可用，**66 单测 + 5 窗口测试全绿**（详见 §0 进度记录） · 关联文件：`analytics-resource-architecture.md`（语义裁决与数据流）、`analytics-resource-prototype-design.md`（原型与交互规格）、`analytics-resource-prototype.html`（交互稿）、`README.md`（模块入口）
> 前置：v1 行为蓝本 `v1/backend/src/core/persistence/analytics_resource_store/`（9 文件 2237 行）+ `v1/docs/backend/ANALYTICS_RESOURCE_MANAGER_DESIGN.md`；v1 前端 `v1/frontend/extensions/builtin/analytics-resource/`（**仅占位卡片列表**，见 `analytics-resource-prototype-design.md` §10）
> 上游：`../scratchpad/scratchpad-dev-plan.md` Phase D（归档/取回 D1–D6，本方案是其落点的另一半）
> 复用 `connection-dev-plan.md` / `scratchpad-dev-plan.md` 的推进方式：Phase 划分 → 文件落点 → 验收 → 测试场景 → 风险
> **范围**：分析存档的归档/取回/登记/版本/组织/检索/回收站/索引修复。**不含**连接与内省（M3/M4）、工作区文件读写（M5）、DuckDB 计算（M2）、Mock 生成（M7）、洞察计算（M8）、项目级→系统级提升（M1）。

## 0. 进度记录（最近在前）

### 2026-09-17 — Phase 1 第七刀：存档详情接入右栏（crate + workbench）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.3（续）✅ | 右侧新增面板档位「存档详情」（`RightPanel::Archive`）：只做转发渲染 + 空态，视图仍在 crate 内（`detail_view::render_detail`）——档位名/图标属外壳数据 | `crates/workbench_shell/src/model.rs`、`crates/workbench/src/panels/right.rs` |
| 数据链 ✅ | 详情与行**同一次取数产出**：`build_snapshot` 增 `history_counts` 入参，为每行落一份 `ArchiveDetail`（`ResourcesSnapshot.details`），面板按选中行 id 取（`selected_detail()`）——详情不在渲染期补取，也不额外开库 | `src/present.rs`、`src/resource_view.rs`、`crates/workbench/src/services/resource_jobs.rs` |
| 版本数一次查完 ✅ | `AnalyticsResourceStore::version_counts()`（`GROUP BY resource_id`）：逐行查会把一次刷新变成 N+1 次查询；不在结果里的行 = 无历史版本（写前快照语义下当前版本不进版本表） | `src/version.rs` |
| 时间口径 ✅ | 详情用**绝对时间**（`format_timestamp`，`%Y-%m-%d %H:%M`），行上仍用相对时间：列表窄要扫得快，详情是看"归档凭证"的地方要精确值；时区与 `connector.rs` 同口径（UTC，本地化是全局议题） | 同上 |
| 空值不空行 ✅ | "版本"分区在无历史版本时**整节不出现**（与其它分区同一口径）：一排"（无）"除了占地方没有信息量 | `src/detail_view.rs` |
| 联动 ✅ | 宿主观察**资产库面板实体**（不是 `SidebarPanel`：子实体的 `notify` 不级联到父面板）→ 右栏正显示存档详情时唤醒它重渲染；句柄存 `WorkbenchView` 私有字段。面板之间不互订，跨 crate 的视图也无从知道对方存在 | `crates/workbench/src/view.rs`、`crates/workbench/src/panels/resources.rs` |
| 契约 ✅ | `Shared` 新增 `resources_panel` 弱句柄进白名单（与 `mock_panel` / `insight_panel` 同例） | `crates/workbench/tests/ui_contract.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **69 单测 + 7 窗口测试全绿**（+2 详情单测）；`cargo test -p rds-workbench --test ui_contract -j 2` → 7 项全绿 | — |

**取舍记录**：详情落在**右 Dock 的档位**里，而不是原型 §3.1 的"编辑区右侧 20rem 属性面板"（`PROPERTY_PANEL_*`）——右 Dock 已有"面板档位"这套现成机制（切换/图标/快捷键/宽度记忆都由外壳管），而属性面板那条路要等 M4 的 `h_resizable` 容器接进编辑区。两者将来若合并，宽度口径按 `RIGHT_DOCK_WIDTH`（17.5rem）与 `DETAIL_PANEL_DEFAULT_WIDTH`（20rem）取一，**不要两份并存**。

**已知缺口**（本轮没做，均记在下）：

1. **关闭项目后左栏仍显示上一个项目的存档**（详情同理）——刷新只挂在"打开/切换项目"上（`project_host::on_opened`），宿主没有 close 回调可挂；要修需先给项目宿主补一个 `on_closed` 或让面板在无项目时清空快照。
2. 详情面板只做**只读信息区**：动作按钮（取回 / 版本历史 / 打标签 / 移入回收站）、内容预览与危险区随对话框批接入（原型 §3.1 的其余分节）。

**未落地**：五个对话框、只读三重守卫（编辑器侧）、批量多选（含 `F2` / `Ctrl+A`）。

> 注：`Cargo.lock` **未随本刀提交**——工作树里它还含其它模块的在途改动，一并提交会混入别人的 WIP。

### 2026-09-16 — Phase 1 第六刀：Action 与快捷键（crate + app 层）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.8 ✅ | 新增 `FocusSearch`（`Ctrl+F` 聚焦工具栏搜索框）与 `ClearSearch`（`Esc` 只清搜索词——种类 / 只看需处理留在菜单里，误清会让人以为筛选坏了）两个 Action + 面板内处理器；app 层把它们与已有的 `DeleteSelected` 绑到 `analytics-resource` context | `src/commands.rs`、`src/resource_view.rs`、`crates/app/src/main.rs`（+ `Cargo.toml` 依赖） |
| 命中路径修复 ✅ | 面板根元素补 `.track_focus(&self.focus_handle)`：**没有它面板不在 dispatch path 上**，app 层绑的键与 `.on_action` 根本落不到（编辑器面板已踩过同一个坑）——窗口测试派发 `ClearSearch` 时暴露并修正 | `src/resource_view.rs` |
| 行漫游与打开 ✅ | `↑↓` 与 `Enter` 不自己声明：列表组件的选中通道与 `confirm` 已接在 `ArchiveListDelegate` 上，两套键语义会打架 | 同上 |
| 窗口测试 ✅ | +1 项：聚焦面板 → 派发 `ClearSearch` → 断言只清搜索词、菜单条件保留（走生产入口：app 层的 `Esc` 就是派发它） | `tests/panel_window.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **67 单测 + 7 窗口测试全绿**；`cargo check -p rds-app -j 2` 零告警 | — |

**未落地**：`F2` 重命名（随重命名入口）、`Ctrl+A` 全选与批量动作（随多选批）、详情面板接入、五个对话框。

> 注：`Cargo.lock` **未随本刀提交**——工作树里它还含其它模块的在途改动（`rds-workbench-shell` 等），一并提交会混入别人的 WIP；下一次 `cargo` 构建会自动补上本刀的依赖边。

### 2026-09-16 — Phase 1 第五刀：列表虚拟化 + 行右键菜单（crate 内）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.1（续）✅ | 行列表从“手搓 `Button` 行 + `overflow_y_scrollbar`”换成 **`list::List`**（虚拟化 + 组件化 hover / 选中 / 键盘漫游）：`ArchiveListDelegate` 持有可见行的副本——`render_item` 在列表渲染期被调用，而那一刻面板正被借用，回头读面板的行集合会直接 panic（与 mock 的生成器搜索委托同例） | `src/resource_view.rs` |
| P1.2（续）✅ | 行的**动作入口改为右键菜单**（原型 §3.2 前三项）：打开（只读）/ 取回（检出）…（本体异常与只读项目禁用）/ ── / 移入回收站；行本身只承载信息——240px 里常驻按钮会把尾部字段挤没 | 同上 |
| P1.2（续）✅ | **kind 图标**：`FileText` / `Table` / `ExternalLink`（取自完整 Lucide 目录，组件子集没有表格形；枚举由 gpui-kit-assets 构建脚本按 svg 文件名生成，**编译通过即资产存在**，不会静默为空），一律 `muted`（颜色信号留给复现强度徽标） | 同上 |
| 选中语义 ✅ | 面板仍是语义权威（`selected` = 行 id）：组件回传的选中经 `set_selected_index` 落回面板；面板的选中在**渲染期镜像**回列表（`sync_list_selection`），镜像期间立标志禁止回写——否则就是“更新正在被更新的实体”（GPUI 直接 panic，实测踩到并已用窗口测试钉住） | 同上 |
| 窗口测试 ✅ | +1 项：宿主来回改选中并逐帧渲染**不重入**；原 3 项条件 / 排序用例照旧绿 | `tests/panel_window.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **67 单测 + 6 窗口测试全绿**（kind 图标映射单测 +1）；`cargo check --workspace --all-targets -j 2` 零告警 | — |

**取舍记录**：行内「打开 / 取回 / 移入回收站」三个按钮**下线**（改写进右键菜单）：原实现只在选中行出现，仍要占一行宽度；它们的正式替代是原型 §2.3 的 hover 版与详情面板的动作按钮，本批先保住“行的信息密度”。

**未落地**：详情面板接入、五个对话框、Action 与按键绑定（`Ctrl+F` / `↑↓` / `Enter` / `F2` / `Delete` / `Ctrl+A` / `Esc`）、行内 hover 动作、批量多选。

### 2026-09-16 — Phase 1 第四刀：workbench 接线（面板挂载 + 快照桥）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.1/P0.1（续）✅ | 左 Dock「资产库」分支由占位换成真面板：**构造期**创建 `ResourcesPanel` 实体 + 注入宿主端口，渲染只转发（`render_resources_placeholder` 下线） | `crates/workbench/src/panels/resources.rs`（新）、`panels/mod.rs` |
| 取数桥 ✅ | `services::resource_jobs`：单工作线程 + tokio（`ProjectDatabaseManager::open` → `list_file_archives` → `IndexRepair::scan` → `present::build_snapshot`）；结果槽只留最新一份；`enqueue_refresh` / `drain_snapshot` / `has_pending` | `crates/workbench/src/services/resource_jobs.rs`（新） |
| 宿主端口 ✅ | `components::resource_host`：`ResourcesHost` 实现——动作类请求给**明确回执**（状态栏提示“尚未接入 + 缺什么”），不接半条链路 | `crates/workbench/src/components/resource_host.rs`（新） |
| 触发点 ✅ | **事件路径**三处：活动栏切到资产库 · Quick Open「打开资产库」· 项目打开/切换（`project_host::on_opened`）——render 不发起任务（与 nav 面板“render 内入队”的既有债刻意区分） | `crates/workbench/src/view.rs`、`components/project_host.rs` |
| 回填 ✅ | `ensure_resources_pump`（照 `ensure_scratchpad_pump`）：60 ms 轮询 `drain_snapshot` → 推给面板实体；失败只给提示并**保留上一份列表** | `panels/resources.rs` |
| 契约 ✅ | 新面板模块登记进 `ui_contract` 的尺寸 / 颜色两份清单（契约 2c 强制：漏登会让两份契约对该文件静默失效） | `crates/workbench/tests/ui_contract.rs` |
| 验证 | 见下文 | — |

**两处刻意的克制**：

1. **不新增 `Shared` 字段**：面板实体句柄与轮询任务都在 `SidebarPanel` 的私有字段里，宿主经 `WorkbenchView::request_resources_refresh` 转一手（`Shared` 字段白名单契约因此无需变动）；
2. **`UntrackedFile` 不进行**：它没有资源行可标（未登记的东西不是存档），留给索引修复对话框呈现；本批只把 `缺失` / `内容已变` 折成行状态。

**已知成本**：每次刷新都会重算全部本体指纹（`IndexRepair::scan`）。它发生在工作线程上，但文件多时会慢；若将来成为瓶颈，先在 `indexer` 加“只查存在性”的快路径，**不要**删掉指纹比对（那会让“内容已变”静默失效）。

**未落地**：五个对话框（归档 / 取回 / 版本 / 回收站 / 索引修复）、编辑器只读打开（P1.6）、Action 按键绑定、虚拟化列表、右键菜单、行图标、`ProjectTrash` 上提（P0.8）。

### 2026-09-16 — Phase 1 第三刀：工具栏（搜索 / 筛选 / 排序）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.1（续）✅ | 工具栏（高 2rem）：搜索框（`Input`，占满剩余宽 + 自带清空钮）· `筛选 ▾`（种类多选 / 只看需处理，按钮上标"菜单条件个数"）· `排序 ▾`（名称 / 版本）；空库时也渲染（版式不随"有没有存档"上下跳） | `src/resource_view.rs`、`src/ui.rs`（+2 常量） |
| P1.2/P2.3（部分）✅ | **两种空态分开**：空库（条件为空 → 引导归档）vs 无匹配（条件非空 → 给"清空筛选"）；可见行 = 筛选 → 排序，**在事件路径算好**（`view_rows`），render 只读 | 同上 |
| 数据层 ✅ | `ArchiveKind::{ALL, label}`（菜单候选顺序与文案的单一来源）；`ResourcesFilter::{toggle_kind, has_kind, menu_dims}`（**全选规范化为不限**：不留"看起来在筛选"的等价条件，否则空库会被显示成"没有匹配"）+ `SortOrder::arrow` | `src/model.rs`、`src/filter.rs` |
| 选中语义 ✅ | 悬空选中改在 `refresh_view_rows` 里清：**筛选与排序也会让行消失**，只盯快照会留下指向"看不见的行"的选中态 | `src/resource_view.rs` |
| 窗口测试 ✅ | +3 项：条件改变可见行且被筛掉的选中被清、排序同键翻转 / 换键保持方向、无匹配态渲染 + 清空筛选（断言**输入框与条件同一次改**） | `tests/panel_window.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **66 单测 + 5 窗口测试全绿**（本刀 +3 单测 +3 窗口）；`cargo check -p rds-analytics-resource --all-targets -j 2` 零告警 | — |

**三条刻意的克制**（避免"看起来支持但算错"）：

1. **状态行仍报库口径计数**（不随筛选变化）：`缺失` / `索引异常` 是"修复…"入口的存在理由，被筛选隐掉就成"看起来没问题的库"；命中数靠"筛选 N"徽标 + 输入框可见。
2. **排序仍只有名称 / 版本**——行上没有 `size_bytes` / `updated_epoch`，拿格式化后的尾巴（`1.2 KB` vs `900 B`）比较会静默排错（见 `filter.rs` 模块头）。
3. **搜索只匹配显示名 + 尾部字段**：原型口径里的别名 / 标签 / 来源表需要 `ArchiveRow` 带这些列（Phase 2）。

**实现期踩到的一个坑**（已写进代码注释）：`InputState::set_value` **不会**发 `InputEvent::Change`（gpui-component 内部注释明说）——程序性改词（清空筛选 / 宿主预填）必须自己同步条件，否则"输入框里的字"与"实际生效的条件"会静默不一致。故 `set_query` / `clear_filter` 是唯一入口，且有窗口测试锁住。

**未落地**：虚拟化列表（`list::List`）、右键菜单、行图标（`IconName` 子集未核实）、详情面板接入、五个对话框、Action 按键绑定（app 层）、**workbench 桥**。

### 2026-09-16 — 补记：Phase 1 接线前半与 `present.rs` 呈现层（追记两笔已提交但未入档的落点）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P0.1（续）✅ | workbench 依赖 `analytics_resource`（workspace 别名已就位）；活动栏标签与 Quick Open 文案 `资源分析` → **资产库**（提交 `f93d560`） | `crates/workbench/Cargo.toml`、`crates/workbench/src/view.rs` |
| P1.x ✅ | 呈现层：索引行 → `ArchiveRow` / `ArchiveCounts` / `ResourcesSnapshot`——`format_size`（< 1 KB 不给 `0.0 KB`）/ `format_scale`（千分位）/ `format_relative_time`（时钟回拨显"刚刚"而不是负值）/ `tail_for`（按 kind 的字段优先级）/ `build_snapshot`（异常态压过 kind）；宿主桥只剩"取数 → 调它 → 推快照"（提交 `51918d4`） | `src/present.rs` |
| 说明 | 这两笔提交当时未入 §0 与代码地图（同一窗口期内 `README.md` 还把 `present.rs` 漏在代码地图外），本刀一并补齐——文档落后代码的窗口期正是最容易丢线索的时候 | 本文件 + 模块 `README.md` |

### 2026-09-15 — Phase 1 第二刀：详情面板 + 工具栏数据层（crate 内）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.3（部分）✅ | 详情面板只读信息区：`ArchiveDetail` 快照 + `detail_rows`（基本信息含**只读说明** / 来源含**指纹缩略** / 版本 / 组织）+ `alert_line`（只在需处理时出现：缺失 / 内容已变 / **引用型常态就提示**）+ `render_detail`；空值不产生行（指纹除外，无值给破折号）。**动作按钮随对话框批接入**（不提前摆点不动的入口） | `src/detail_view.rs`、`src/ui.rs`（标签列宽） |
| P2.3（数据层）✅ | 工具栏规则独立成模块（面板与后续菜单共用）：`ResourcesFilter`（关键字（名称+尾部，大小写不敏感）/ 种类 / 只看异常）+ `SortField`/`SortOrder`（`flipped`、`label`）+ `apply_view`（筛选→排序；同键名称兜底且不随方向翻转）；`is_empty()` 决定面板显示"还没有存档"还是"没有匹配" | `src/filter.rs` |
| 窗口测试 ✅ | `tests/panel_window.rs`：空态 + 只读提示可渲染、**仅渲染不触发宿主动作**（断言）、快照推送驱动行与计数、宿主驱动选中、**行消失后悬空选中被清掉** | `crates/analytics_resource/tests/` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **58 单测 + 2 窗口测试全绿**；`cargo check` 零告警 | — |

**两条刻意的克制**（避免"看起来支持但算错"）：

1. 排序只支持行上**真实存在**的键（名称 / 版本）——按大小或归档时间排序会去比较格式化后的字符串（`1.2 KB` vs `900 B`）而静默排错，需先给 `ArchiveRow` 补 `size_bytes` / `updated_epoch`；
2. 窗口测试的宿主用替身只记录调用，不接服务层。

**未落地**：搜索框与筛选/排序菜单控件（数据层已就位，待 `Input` / `DropdownMenu`）、虚拟化列表（`list::List`）、右键菜单、五个对话框、详情面板动作、快捷键绑定、行图标（`IconName` 子集未核实）。

### 2026-09-15 — Phase 1 第一刀：面板骨架（crate 内）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P1.1/P1.2（部分）✅ | `ResourcesPanel`（`BasePanel` + `Panel` + `Focusable`）：面板头（标题 + 归档入口）、提示行（只读/通知分色）、行列表（显示名 / 版本徽标（v1 不显）/ **复现强度徽标** / 尾部字段 / 选中态）、状态行（异常时给"修复…"）、空态；宿主动作经 `ResourcesHost` 注入（5 个请求方法），面板**不自己取数** | `src/resource_view.rs`、`src/ui.rs` |
| 纯函数与单测 | `strength_badge` / `badge_tone`（`引用`=warning：复现最弱必须显眼）/ `row_tail`（字段优先级）/ `ArchiveCounts::line`（零桶不显示）+ 4 项单测 | 同上 |
| 接线 | `Cargo.toml` 加 `gpui-kit` 依赖 + dev-dependencies `test-support`（窗口测试待下一批） | `crates/analytics_resource/Cargo.toml` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **49 项全绿**；`cargo check` 零告警 | — |

**实现期踩到的两个坑**（已写进代码注释，供后续视图参考）：

1. **edition 2024 的 RPIT 会捕获入参生命周期**——区域渲染函数写 `-> impl IntoElement` 会借住 `cx`，连续调两个区域函数即"重复可变借用"。返回类型改具体（`Div` / `AnyElement`）后消失；
2. `overflow_y_scrollbar` 返回的不是 `Div`（滚动包装类型），带滚动的区域必须返回 `AnyElement`。

**未落地（下一批）**：搜索框与筛选/排序菜单、虚拟化列表（`list::List`，当前行用 `Button`）、右键菜单、详情面板、五个对话框、Action 与快捷键、行图标（`IconName` 子集未核实，先不引入）、`workbench` 侧接线（依赖 + 渲染入口，属跨 crate）。

### 2026-09-15 — Phase 0 第三批：索引修复（`indexer.rs`）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P0.11 ✅ | `IndexRepair`：`scan`（只读，**不改任何状态**）报告三类差异——有文件无记录 / 有记录无本体 / 指纹不匹配；三类修复动作均需人工确认：`adopt_file`（补登，指纹现算、不搬文件、来源留空）、`accept_current_content`（指纹换实际值 + 版本 +1 + **重新加回只读**）、`remove_orphan_record`（本体都没了，直接删行、不进回收站） | `src/indexer.rs`（新） |
| 零件 | `PayloadStore::list_files`（递归遍历本体目录、跳过隐藏项、`/` 分隔排序）、`AnalyticsResourceStore::{list_file_archives, remove_orphan_record}` | `src/{payload,resource}.rs` |
| 测试 | +8 项（7 索引修复 + 1 本体遍历）：三类差异各一、补登后转干净、重复补登被拒、接受当前内容后版本/指纹/只读三态正确且写前快照保留、本体不存在时拒绕 `accept_current_content` | `src/indexer.rs`、`src/payload.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **45 项全绿**；`cargo check` 零告警 | — |

**两条诚实语义**（写进实现与注释，不做表面修复）：

1. `accept_current_content` 产生的历史版本**只有元数据、没有内容副本**——旧内容在外部被覆盖时已经没了，界面按"副本缺失"呈现，而不是假装能还原；
2. "有记录无本体"的另一个动作**从回收站还原**依赖 `ProjectTrash`（P0.8），本期只提供"删除记录"，还原动作待 P0.8 接入。

**仍余**：`recycle.rs` 废弃（P0.8，跨 crate）、版本保留策略接设置项、`kind` 过滤/列表展示（Phase 1/2）、视图四处占位（Phase 1）。

### 2026-09-15 — Phase 0 第二批：归档 / 取回闭环

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P0.10（部分）✅ | `ArchiveService`：`archive`（首次归档：指纹 → 本体 move → 写登记，**索引失败回滚本体**）、`archive_into_existing`（再归档：指纹未变即**幂等**返回；变了才"旧内容留副本 → 写前快照 → 覆盖本体 → 版本 +1 → 按 keepVersions 裁剪"）、`checkout`（取回复制；拒绝落在 `resources/` 内的目标）+ `ResourcesChanged { reason, resource_id }` 广播（无订阅者不报错） | `src/service.rs`（新） |
| P0.7（续）✅ | 新列接入：`AnalyticsResource` 增 9 字段 + `RESOURCE_COLUMNS` / `map_resource_row` 同步；新增 `insert_archive`（归档专用写入，不走 v1 通用入口）、`update_archive_content`、`find_archive_by_rel_path`（归档前占用检测）；**行映射从 4 份收敛为 1 份**（`recycle.rs` / `tag.rs` 改调 `map_resource_row`，JOIN 用新助手 `qualified_resource_columns`） | `src/{models,resource,recycle,tag}.rs` |
| 测试 | 测试库改跑齐 007 + 020（此前只跑 007 → 测试库与生产库表结构不一致）；新增 7 项归档服务用例（含**故障注入**：用 SQLite 触发器让写索引必失败，断言本体回滚） | `src/tests.rs`、`src/service.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **37 项全绿**（16 存储 + 4 领域 + 10 本体 + 7 归档服务） | — |

**本轮定下的两条接口约定**（原型与手册已如此描述，此处落到代码）：

1. `save_resource_version` 现**返回快照行 id**，供资源行的 `parent_version_id` 指向本次写前快照；
2. `CheckoutRequest.dest_path` 由调用方给**绝对路径**——M6 不认识上游 `scratchpad/` 的目录结构（依赖方向 `scratchpad → analytics_resource`），只把自己的 `resources/` 管住。

**仍余**：`indexer.rs`（三类孤儿）、废弃 `recycle.rs`（P0.8，跨 crate）、版本保留策略接入设置项、`kind` 过滤/列表展示（Phase 1/2）。

### 2026-09-15 — Phase 0 首切片（仅 crate 内）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| P0.1（部分） | 接线三件：crate 入口文档 ✅、workspace 别名 ✅（`analytics_resource = { path = …, package = "rds-analytics-resource" }`，惰性条目）；workbench 依赖 ⬜（属 Phase 1） | `crates/analytics_resource/README.md`、`Cargo.toml` |
| P0.4 ✅ | 迁移 `project_meta/020_analytics_resource_archive.sql`：9 个语义列（`kind`/`content_hash`/`file_rel_path`/`readonly`/`promoted_from`/`source_connection_id`/`source_table`/`definition_sql`/`archived_at`）+ `file_rel_path` 部分唯一索引（软删行不参与）+ kind/指纹索引；**不改 007** | `crates/engine/migrations/project_meta/` |
| P0.5 ✅ | 领域类型：`ArchiveKind` / `ReproductionStrength` / `ArchiveStatus` / `ArchiveBinding` / `ArchiveRequest` / `CheckoutRequest` / `CheckoutOutcome`（含 4 项单测） | `src/model.rs` |
| P0.6 ✅ | 本体层 `PayloadStore`：`resolve` 越界守卫（拒绝对/根相对路径、`..`、点前缀）、归档搬运（`rename` → 跨设备复制兜底、目标存在即拒绝）、只读标记、sha256 指纹、历史副本与裁剪（含 8 项单测） | `src/payload.rs` |
| P0.7（部分） | 已修：分页除零与负数、`LIKE` 转义、连接嵌套（新增 `get_resource_by_id_on`）、更新无事务（`BEGIN IMMEDIATE`）、影响 0 行不报错、`parent_version_id` 语义（指向快照行）、JSON 解析双策略（统一宽容 + warn）、乱码副本名；**待做**：新列接入（kind 过滤 / 指纹回填 / `file_rel_path` 唯一性） | `src/resource.rs`、`src/version.rs` |
| P0.9（部分） | `save_resource_version_on`：在调用方事务内写快照、返回快照行 id；并发冲突不再被静默吞（裸 `INSERT` 替代 `INSERT OR IGNORE`）。**待做**：指纹触发版本 + `keepVersions` 保留策略落库 | `src/version.rs` |
| P0.12（部分） | `mod tests` 补声明（**此前 560 行用例在 v2 从未编译**，`cargo test` 报 0 项）；新增 t016 库层契约测试（列 / 默认值 / `CHECK` 生效）。**待做**：测试改走 `engine::migration` 公共入口（现仍 `include_str!` 直执两段 SQL） | `src/lib.rs`、`src/tests.rs` |
| 验证 | `cargo test -p rds-analytics-resource -j 2` → **28 项全绿**（16 存储 + 4 领域 + 8 本体）；`cargo check -p rds-analytics-resource -j 2` 零告警 | — |

**本轮修正的两处「搬运期遗漏」**（属实修，不只是改文档）：

1. `crates/analytics_resource/src/tests.rs`（560 行）在 Round 11 搬运时**未在 `lib.rs` 声明 `mod tests`**，因此在 v2 从未被编译——文档里"15 项基线"实际是"0 项"。
2. v1 的 `t015_concurrent_update_same_resource` 断言"3 条版本（original + 2 updates）"，在写前快照 + `UNIQUE(resource_id, version)` 语义下**任何并发交错都不可满足**（两次更新最多落两条写前快照，当前版本不进表）。已按真实不变式重写：两次更新各留一条快照（v1/v2）、资源行版本号单调递增到 3。

**未做（需跨 crate 或属后续阶段，均已留档）**：engine 连接池 `busy_timeout` / `acquire` 超时（P0.2）、`.RSmeta` 常量去重（P0.3，现 5 处各自声明 + 1 处字面量）、`ProjectTrash` 上提中性化与 `recycle.rs` 废弃（P0.8）、`service.rs` / `indexer.rs`（P0.10/P0.11）、视图四处占位（Phase 1）。

### 已确认决策（2026-09-15）

| # | 决策 | 出处 |
| --- | --- | --- |
| 1 | **语义取 C（混合模型）**：按 kind 分本体，统一对外"归档凭证"语义 | 架构 §0 D1/D2 |
| 2 | **命名**：模块 = 资产库；实体 = 分析存档；动作 = 归档 / 取回；"提升"一词只给 M1 | 架构 §2.4 |
| 3 | 三种 kind（`file` / `analysis` / `table_ref`）；**第一期只做 `file`** | 架构 §2.2 |
| 4 | 版本以 `content_hash` 触发，默认保留最近 5 份历史内容 | 架构 §5 |
| 5 | 回收站统一项目级 `ProjectTrash`，`origin = "resources"` | 架构 §7.1 |
| 6 | `scope` 改派生只读；`config` 降级为扩展位 | 架构 §4.2/§4.3 |
| 7 | 标签为主 + 单层分组；不做多级文件夹树 | 原型 §11 |
| 8 | 归档后只读，修改走取回；面板不提供"编辑资源" | 原型 §1 |
| 9 | 视图入本 crate（对齐 `overview.md`），若后续拍板"留 workbench"，§12 文件落点平移 | 架构 §8.2 |

## 1. 现状盘点

### 1.1 后端：持久层逐字搬运（可用，但带 12 项缺陷）

| 项 | 结论 |
| --- | --- |
| 可用资产 | `store` 层约 1300 行（`resource` 462 / `folder` 207 / `tag` 314 / `version` 83 / `models` 110 / `helpers` 30 / `tests` 560），**约 55% 可直接留用** |
| 逐字搬运的证据（Round 11 当时） | `diff --strip-trailing-cr` 对比 v1：**只差 `use` 路径一行**；`007_analytics_resources.sql` 与 v1 **完全相同**。注：Phase 0 首切片后就地修了 `resource.rs` / `version.rs`，这两个文件已不再逐字一致 |
| 必须作废 | `recycle.rs`（419 行）整体让位给 `ProjectTrash`；`version.rs` 重写为内容指纹版本 |
| 继承缺陷 | 12 项，逐条见架构 §13.1（`permanent_delete` 不彻底 / `total_pages` 除零 / 无事务 / 连接嵌套 / `parent_version_id` 恒等自身 id / 恢复丢归属 / 乱码 `(鍓湰)` 等） |

### 1.2 视图与命令：四处占位

| 文件 | 行数 | 状态 |
| --- | --- | --- |
| `src/model.rs` | 3 | 占位 |
| `src/commands.rs` | 3 | 占位 |
| `src/resource_view.rs` | 3 | 占位 |
| `src/recycle_bin_dialog.rs` | 3 | 占位 |
| `workbench/src/panels/` | `render_resources_placeholder`（当前 5846 起）| 占位文案仍是 v1 语义（"数据源连接引用 / DuckDB 分析表"）|

### 1.3 接线缺口（不补则视图永远落不了地）

| 缺口 | 落点 |
| --- | --- |
| workspace 未声明别名 | `Cargo.toml` `[workspace.dependencies]`（当前 41–52 行，行号会漂移） |
| workbench 未依赖 | `crates/workbench/Cargo.toml` |
| crate 无入口文档 | `crates/analytics_resource/README.md`（`project` / `scratchpad` 均有） |

### 1.4 底座缺陷（属 `engine`，但 M6 会被它卡死）

连接池 `Drop` 丢连接 + `acquire` 无限自旋 + 无 `busy_timeout`（架构 §13.2 #13/#14）——M6 的 `update_resource` 一次操作占 2 条连接而池只有 3 条，**Phase 0 必须先在 engine 层修掉**。

## 2. Phase 0 — 地基（无 UI，可独立验收）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P0.1 | 接线三件：workspace 别名 + workbench 依赖 + crate README | `Cargo.toml`、`crates/workbench/Cargo.toml`、`crates/analytics_resource/README.md` | `cargo check --workspace --all-targets -j 2` 零告警 |
| P0.2 | **engine 连接池修复**：`busy_timeout`、`acquire` 超时返回 `Err`（不再无限自旋）、归还语义不再静默丢连接 | `crates/engine/src/persistence/project_db.rs` | 单测：池压满后 `acquire` 在超时后报错而非挂起；并发写不再 `database is locked` |
| P0.3 | 项目元数据目录常量**收敛为单点定义**（拼写已在连接模块 C22 ③ 统一为 `.RSmeta`，残的是去重：现 5 处各自声明——`project::store::RS_META_DIR_NAME`、`project::lock::META_DIR`、`engine::connection_org_store::RS_META_DIR_NAME`、`scratchpad::store::META_DIR_NAME` + `engine::project_db` 字面量）；单一来源放 `engine`（`project → engine` 方向已定） | `crates/engine/src/…` + 各消费方 | 全仓 grep 无重复字面量/常量声明；Linux 大小写敏感场景有回归测试（参 `insight::rule_registry` 的 `test_project_rules_dir_uses_canonical_meta_dir` 写法） |
| P0.4 | 迁移 `project_meta/020_analytics_resource_archive.sql`（**建文件前重新核对编号**：编号先到先得，019 已被 insight 规则索引占用）：加 `kind` / `content_hash` / `file_rel_path` / `readonly` / `promoted_from` / `source_connection_id` / `source_table` / `definition_sql` / `archived_at`；**不改 007** | `crates/engine/migrations/project_meta/` | 迁移幂等；老库升级后旧行 `kind` 默认 `file`、`content_hash` 为空（首次打开标 `待指纹`） |
| P0.5 | 领域类型：`ArchiveKind` / `ArchiveSource` / `ArchiveStatus`（正常/缺失/内容已变）/ 请求响应结构 | `crates/analytics_resource/src/model.rs` | 单测：kind 与状态序列化稳定 |
| P0.6 | 本体层 `payload.rs`：`resources/` 定位与越界拒绝（含 `.RSmeta` 与点前缀）、move 与跨设备 copy 兜底、只读设置、`sha256` 指纹、历史副本读写 | `crates/analytics_resource/src/payload.rs` | 单测：越界路径全部被拒；只读设置失败只警告；指纹对同一内容稳定 |
| P0.7 | store 改造：加列读写、kind 过滤、**修 12 项继承缺陷**（尤其 `total_pages` 除零、`page_size ≤ 0`、`LIKE` 转义、事务化、连接不再嵌套、`created_by`/乱码） | `src/{resource,folder,tag}.rs` | v1 的 15 个用例全绿（改为走迁移系统）；新增边界用例 |
| P0.8 | **`ProjectTrash` 上提 + 中性化**：类型去 M5 化（`TrashEntryKind`）、`restore` 按 `origin` 分派目标根、归属移到 `engine`（第二个使用方已成立） | `crates/scratchpad/src/trash.rs` → `crates/engine/src/…` | M5 现有回收站测试全绿；M6 可删除→还原往返 |
| P0.9 | 版本重写：`content_hash` 触发、`parent_version_id` 语义修正（或删列）、历史内容按 `keepVersions` 保留/裁剪 | `src/version.rs` | 单测：**hash 未变不产生新版本**；hash 变则 +1 且旧内容仍在 |
| P0.10 | 服务门面 `service.rs`：归档/取回/检索/修复编排 + `ResourcesChanged` 事件（含 `reason`） | `src/service.rs` | 单测：归档全链路含回滚；事件载荷正确 |
| P0.11 | 索引修复 `indexer.rs`：三类孤儿检测与**人工确认**后的修复动作 | `src/indexer.rs` | 单测：有文件无记录 / 有记录无文件 / 指纹不匹配 三态各一用例 |
| P0.12 | 测试改道：`tests.rs` 的 `include_str!("…007…")` 改为走 `engine::migration` 公共入口 | `src/tests.rs` | 新增 020 后测试自动带出新列 |

## 3. Phase 1 — 能用的资产库（面板 + 归档/取回闭环）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P1.1 | 面板骨架：面板头（标题 + `＋▾` + `⋯`）、工具栏（搜索 / 筛选 / 排序）、行列表（虚拟化）、底部状态行 | `src/resource_view.rs`、`workbench/src/panels/` | 切换活动栏可见；`>100` 项流畅；状态行计数正确 |
| P1.2 | 行渲染：kind 图标（`muted`）+ 显示名 + 版本徽标（v1 不显示）+ **强度徽标** + 尾部字段（按字段优先级规则） | `src/resource_view.rs` | 三类 kind 行可区分；240px 无异常折行（溢出省略 + tooltip） |
| P1.3 | 详情属性面板（右侧，默认 20rem，宽度记忆）：基本信息 / 来源 / 版本摘要 / 标签与分组 / 内容预览 / 危险区；`file` 型首版 | `src/detail_view.rs`、`workbench/src/panels/` | 选中行切换联动；只读锁标记与"需取回编辑"提示常显 |
| P1.4 | **归档入口**：草稿箱右键「归档为存档…」+ 面板头「从草稿箱归档…」；确认对话框（显示名 / 目标位置只读 / 分组 / 标签 / 来源连接自动带出 / 保留历史 / 冲突处理） | `src/dialogs/archive.rs`、`crates/scratchpad/src/…`（发起） | 归档后草稿消失、资源只读、两侧面板同步刷新（事件链路通） |
| P1.5 | **取回（检出）**：右键 → 对话框（目标名 / 目标目录 / 是否打开）→ 复制到草稿箱 + 草稿侧记 `derived_from_resource_id` | `src/dialogs/checkout.rs`、`scratchpad` | 本体不动；重复取回自动改名避让 |
| P1.6 | 只读三重守卫：写入 API 拒绝（应用守卫）+ 编辑器只读打开 + 文件系统属性（辅助） | `payload.rs`、`workbench` EditorPanel | 任何写入路径返回明确错误文案；编辑器以只读态打开 |
| P1.7 | 术语与入口收尾：活动栏标签 `资源分析`→**资产库**；Quick Open 文案同步；删占位渲染；`LeftPanel::Resources` 图标复核 | `workbench/src/view.rs`、`panels/` | 全仓无"资源分析"作为模块名出现；无占位文案残留 |
| P1.8 | Action 与快捷键：`Ctrl+F` / `↑↓` / `Enter` / `F2` / `Delete` / `Ctrl+A` / `Esc`；`Ctrl+Shift+A`（草稿箱上下文） | `src/commands.rs`、`crates/app/src/main.rs` | 窗口测试：漫游与打开/删除分支 |

## 4. Phase 2 — 组织与检索

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P2.1 | 标签：新建/改名/删除（**补 v1 缺失的改名与删除**）、打标/去标、按标签检索、chips 渲染 | `src/tag.rs`、`src/tag_view.rs` | 同名（未删）拒绝；删除标签清关联 |
| P2.2 | 分组：单层分组的新建/改名/删除/移动（含批量移动与拖拽到分组头） | `src/folder.rs`（语义为分组）、`src/folder_view.rs` | 折叠状态持久化；空分组可见 |
| P2.3 | 搜索与筛选：名称 / 别名 / 标签 / 来源表；筛选三维（kind / 强度 / 标签）；排序（名称 / 归档时间 / 更新时间 / 大小 / 版本） | `src/resource.rs`、`src/resource_view.rs` | 转义 `%`/`_`；非法排序字段回退；`page_size ≤ 0` 不再 panic |
| P2.4 | 设置项：`keepVersions` / 默认排序 / 默认分组 → `settings.json`（**不用 localStorage**，对照 v1） | `crates/settings`、`src/service.rs` | 重启后保持 |
| P2.5 | 多选与批量：批量打标签 / 批量移动 / 批量删除（含数量提示） | `src/resource_view.rs`、`src/commands.rs` | 多选态菜单按数量自适应（v1 的缺陷） |

## 5. Phase 3 — 版本与恢复

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P3.1 | 版本历史对话框：版本表 + 相邻差异摘要 + **选中版本动作栏**（还原为当前版本 / 取回该版本为草稿 / 删除该版本内容副本） | `src/version_view.rs` | 不提供行级 diff；还原生成新版本而非覆盖 |
| P3.2 | 历史内容保留策略落地：`keepVersions`（默认 5，`0` = 只留元数据）+ 副本缺失标记 | `src/payload.rs`、`src/version.rs` | 裁剪只删副本，版本行保留；副本缺失有徽标 |
| P3.3 | 回收站对话框（仅 `origin = "resources"`）+ 跨模块条目禁用提示 + 撤销条 | `src/recycle_view.rs` | 永久删除真删 payload；跨模块还原被拒 |
| P3.4 | 索引修复对话框：三分组 + 动作；状态行异常段可点 | `src/dialogs/index_repair.rs` | 三类问题各可修复；**无自动修复路径** |
| P3.5 | 异常态呈现：本体缺失（灰显 + `danger` 点 + 横幅）、内容已变（`warning` 徽标） | `src/resource_view.rs`、`src/detail_view.rs` | 缺失项不隐藏、仍可删除/还原 |

## 6. Phase 4 — `Analysis` 档（DuckDB 表）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P4.1 | `analysis` 型本体层：表/视图存在性、`definition_sql` 采集（`SHOW`/`duckdb_tables`）、行数×列数、结构摘要指纹 | `src/payload.rs`（analysis 分支） | 指纹对结构变化敏感、对行数变化策略明确（见风险 R4） |
| P4.2 | 上游接入：**M7 Mock 产物**归档（对齐 `mock_persist_as_asset` 的文档/实现落差，二选一并同步文档） | `crates/mock`、`src/service.rs` | Mock 生成后可一键归档，指纹与定义可查 |
| P4.3 | 上游接入：**M5 编辑器结果落库后归档** | `workbench` EditorPanel、`src/service.rs` | 归档后可"重新执行定义"复算 |
| P4.4 | 打开路径：`analysis` 型双击 → 结果表格（复用编辑器结果区组件） | `workbench` | 只读呈现 |

## 7. Phase 5 — 待定（不承诺）

| 项 | 前置条件 |
| --- | --- |
| `TableRef` 档（远端表引用） | 先回答"用户真的需要这个书签吗"；若做，必须带失效警告条与"立即校验" |
| M1 系统级提升（项目→系统级） | 等 M1 设计落地；本模块已把 `scope` 改为派生只读，届时只需按库位置派生 |
| 依赖追踪（v1 设计文档的 `resource_references`） | 先有真实消费方（"删除前检查被谁引用"），否则不建表 |
| FTS5 全文检索 | 先量：条目 > 1000 且 `LIKE` 实测不可用再做 |

## 8. 明确不做

| 项 | 原因 |
| --- | --- |
| 多级文件夹树 / 面包屑 / 手动排序号 | v1 是残的（无改名/删除/移动/排序），投入产出比差；标签 + 单层分组已够 |
| 页码分页（v1 的 10/20/50/100） | 桌面应用语义：滚动 + 虚拟列表，不做翻页 |
| 前端 LRU + TTL 缓存 | 本地 SQLite 无 IPC 成本；v1 该实现本身有缺陷 |
| 拖拽到 SQL 编辑器 | 有"取回→打开"路径，复杂度不值 |
| `config` JSON 手工编辑 | 字段进表单、扩展进详情面板 |
| 版本行级 diff | 文本 diff 归编辑器 |
| 自动索引修复（静默导入/删除） | 会让"存档"变成用户没同意过的东西 |
| 归档本体的原地编辑 | 一旦允许，`content_hash` 与版本失去意义 |
| v1 设计文档中从未实现的 17 条命令（`extract_table` / `generate_sql_reference` / `check_delete_safe` / `cleanup_expired` …） | 无消费方、无真实需求；**不承诺** |

## 9. 测试场景

| # | 场景 | 期望 |
| --- | --- | --- |
| T1 | 归档普通 `.sql` 草稿 | 文件移至 `resources/`、只读、登记行 `kind=file`、`content_hash` 非空、v1、事件发出 |
| T2 | 归档目标已存在 | 询问改名/取消，**不静默覆盖**；取消后源文件仍在草稿箱 |
| T3 | 归档第 4 步成功、第 6 步失败（模拟索引写失败） | 本体 move 回滚，草稿箱恢复原状 |
| T4 | 取回 | 草稿出现副本、本体未变、再次归档版本 +1 |
| T5 | 归档内容未变（取回后原样再归档） | **不产生新版本** |
| T6 | `keepVersions = 5` 且已 7 个版本 | 只保留最近 5 份**内容副本**，7 个版本行全在 |
| T7 | 删除 → 回收站 | 本体进 `.RSmeta/trash/`、`origin = "resources"`、登记行移除；还原可回原路径 |
| T8 | 永久删除 | payload 真删（对照 v1 的"只删回收站行"缺陷） |
| T9 | 跨模块还原 | 草稿箱条目在资产库还原被拒，提示"请在草稿箱还原" |
| T10 | 本体缺失（手工删文件） | 行灰显 + `danger` 点；详情横幅；可"删除记录"/"从回收站还原" |
| T11 | 指纹不匹配（手工改文件） | `warning` 徽标"内容已变"；可"接受当前内容（生成新版本）" |
| T12 | 有文件无记录（手工放文件进 `resources/`） | 重建索引**列出**候选，用户确认后才补登 |
| T13 | 越界写入 | 经 API 写 `resources/` 或 `.RSmeta/**` 一律被拒，错误文案指向"先取回" |
| T14 | 搜索边界 | 输入 `%` 不命中全表；`page_size = 0` / `page = -1` 不 panic；非法排序字段回退 |
| T15 | 跨设备归档（`rename` 失败） | 退回复制 + 删除，结果一致（可用不同卷的临时目录模拟） |
| T16 | 非 ASCII 名 / 超长名 / 含空格名 | 归档、取回、还原全链路正常；分组头与列表显示正确 |

**基线**：v1 的 15 个存储用例全绿（改造后不得减少），新增用例随 Phase 落地。

## 10. 风险

| # | 风险 | 对策 |
| --- | --- | --- |
| R1 | 双真相源（文件系统 + 索引）不一致 | 明确"文件系统权威"；三类孤儿都有检测与人工修复入口；归档按"先本体、后索引、失败回滚"顺序（架构 §6.3） |
| R2 | 归档是对用户不可逆的动作（草稿从工作区消失） | 底部撤销条（复用 M5）+ 归档确认对话框明示"文件将移动到 resources/ 并变为只读" |
| R3 | 只读属性在 Windows/网络盘不可靠 | 应用层守卫为主，属性为辅；设置失败只警告（不阻塞归档） |
| R4 | `Analysis` 型指纹语义含混（表数据会变，结构不变） | 第一期不做；第二期先定"指纹覆盖定义+结构，行数只作元信息"并写进 UI 文案 |
| R5 | 面板塞不下（240px）信息 | 字段优先级规则 + tooltip；必要时放宽 Dock 起步宽（需同步 `ui.rs` 与契约测试） |
| R6 | 与 M5 归档发起方的耦合 | M6 只接受 `PathBuf` + 元数据入参，不依赖 `ScratchpadStore` 类型；依赖方向 `scratchpad → analytics_resource` |
| R7 | 事件链路再次"发了没人听"（v1 教训） | 事件必须带 `reason`，且**发/收两端同批落地**；验收含"两侧面板同步刷新" |
| R8 | 历史内容副本导致磁盘膨胀 | 默认只留 5 份；裁剪只删副本；详情面板显示"历史内容占用" |
| R9 | engine 池缺陷未修完就做 M6 | P0.2 是 Phase 1 的硬前置（否则 UI 会卡死，且难定位） |

## 11. 验证命令

```sh
# 模块回归
cargo test -p rds-analytics-resource --lib -j 2
cargo test -p rds-scratchpad --lib -j 2     # 回收站上提后的回归
cargo test -p rds-engine --lib -j 2         # 池修复与目录常量

# 契约（零裸色 / 零裸 px）
cargo test -p rds-workbench --test ui_contract -j 2

# 全工作区守卫
cargo check --workspace --all-targets -j 2

# 真机
cargo run -p rds-app -j 2
```

- 真机矩阵：明暗主题 × 三类 kind × 异常三态（缺失 / 内容已变 / 引用未校验）。
- 平台矩阵：Windows（只读属性最弱）/ macOS / Linux（大小写敏感 + 无只读属性语义差异）。

## 12. 实现位置映射（决策 → 文件）

| 决策 / 能力 | 文件 |
| --- | --- |
| 语义裁决、kind 模型、数据流 | `analytics-resource-architecture.md` §0/§2/§6 |
| 面板 / 列表 / 状态行 | `crates/analytics_resource/src/resource_view.rs` |
| 详情属性面板 | `crates/analytics_resource/src/detail_view.rs` |
| 版本历史 / 回收站 / 分组 / 标签对话框 | `src/{version_view,recycle_view,folder_view,tag_view}.rs` |
| 归档 / 取回 / 索引修复对话框 | `src/dialogs/{archive,checkout,index_repair}.rs` |
| 领域类型 | `src/model.rs` |
| 本体层（fs + 只读 + 指纹 + 历史副本） | `src/payload.rs` |
| 索引层（登记 / 分组 / 标签 / 版本） | `src/{resource,folder,tag,version}.rs` |
| 服务门面 + 事件 | `src/service.rs` |
| 索引修复 | `src/indexer.rs` |
| Action / 快捷键 | `src/commands.rs` + `crates/app/src/main.rs`（`analytics-resource` context） |
| 左 Dock 装配（仅协议） | `crates/workbench/src/{view.rs,panels/}` |
| 迁移 | `crates/engine/migrations/project_meta/020_analytics_resource_archive.sql` |
| 项目级回收站（上提后） | `crates/engine/src/…`（现 `crates/scratchpad/src/trash.rs`） |
| 尺寸常量 | `crates/workbench_shell/src/ui.rs`（新增「资产库（M6）专用尺寸」4 项，见原型 §7） |
| 契约测试范围 | `crates/workbench/tests/ui_contract.rs` |
