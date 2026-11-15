# 联邦查询公共入口 · 交接说明

> 用途：原会话的上下文即将丢弃，本文件让新会话**不必重新调查**就能接着干。
> 更新时间：2026-11-14。仓库 `D:\RdataStation\RDS\RdataStation-v2`，分支 **`main`**（`feat/federation-entry` 已以 `--no-ff` 合并为 `f87ff9d3` 并删除，保险 tag `pre-merge-federation-entry` → `d38d5572`）。`main` 领先 `origin/main` 22 个提交，**未推送**。

## 0. 一句话现状

方案与 6 份原型**已进 main**；来源选择器 Phase 5 的 **14 刀已全部进 main**（`crates/workbench/src/components/source_picker/`，10 文件约 2850 行，`cargo check` 零告警、全量 138 项测试通过）。**卡点不在代码，在执行侧**：资产库剩下的「本项目归档文件」「全局共享」两段都缺执行支持，画出来就是「能选不能执行」。

## 1. 已定裁决（不要再问，直接执行）

| # | 裁决 | 依据 |
| --- | --- | --- |
| 1 | 入口 = 洞察 / 结果集二次分析 / 编辑器联邦通道的**公共入口**；目标是共享资源 | 用户三轮确认；「共用减少资源」是入口核心 |
| 2 | 共享**会话**，不共享一条全局连接（ATTACH 是连接级；`interrupt` 是连接级单值，共用会让取消洞察误伤编辑器查询） | 第二轮裁决 |
| 3 | 编辑器按**执行通道**显式启动联邦；洞察 / 分析**默认本地**，只给一个默认关的「用联邦查询」开关，仅 ≥2 可挂源时出现 | 第三、四轮 |
| 4 | `AnalysisSession = Local \| Federated{owner}`；**owner 取 `conn_id` 是推荐值，尚未拍板**（候选「面板级 owner」，见 §3 待拍板） | 第三轮 |
| 5 | 两种形态：洞察走**形态 I**（联邦取样 → 搬回本地 `tmp_i_`）；结果集二次分析走**形态 II**（建表与 SQL 都在联邦会话） | 第三轮 |
| 6 | 来源分类**三分法**：数据库 / DuckDB / 文件。原因：根分类对应三种**读取路径** | 第七轮 |
| 7 | **结果集并入 DuckDB 组**，不单开 tab | 第七轮 |
| 8 | **「资源分析」正名为「资产库」，挂文件根下**（本体是 `resources/` 与 `analytics.duckdb`，都是本机持久物） | 第八轮 |
| 9 | **不等 M6 二期**，`ArchiveKind::Analysis` 直接按完整语义呈现（版本 / 归档态 / 只读） | 第九轮 |
| 10 | **搜索最小单位是表 / 视图**，不搜列名、不搜分组名 | 第九轮 |
| 11 | **全局实例必须进树**，作为项目之间的桥梁；与本项目两段**并列不混排** | 第九轮 |
| 12 | 对象树**复用行原语**（`workbench_shell::tree`），**不复用 `tree::Tree`** | 第八轮 + 源码裁决 |

## 2. 产物清单

| 路径 | 内容 | 状态 |
| --- | --- | --- |
| `docs/architecture/federation/federation-entry-design.md` | 方案 + 5 阶段开发计划 + Oracle 第三方插件 ATTACH 契约（§4 已含三分法与全局段） | **已进 main** |
| `prototypes/entry/README.md` | 索引 + 根3/叶5 映射 + 对象树与搜索 + 未画清单 | **已进 main** |
| `prototypes/entry/00-overview.html` | 四屏总览 | **已进 main** |
| `prototypes/entry/01-source-picker.html` | 三分法 tab + 可折叠对象树 + 跨根搜索 + 最近使用 + 右侧元信息 + 资产库三段（含全局） | **已进 main** |
| `prototypes/entry/02-insight-dialog.html` | 取样参数 + 会话三态 | **已进 main** |
| `prototypes/entry/03-result-analysis.html` | 分析菜单 / 自定义 SQL / 上限 / 会话三态 | **已进 main** |
| `prototypes/entry/04-editor-channel.html` | 三档 + 门控 + 写拒绝 | **已进 main** |
| `prototypes/entry/05-session-panel.html` | 会话头 + 7 种源状态 + 生命周期 | **已进 main** |
| `prototypes/entry/06-object-tree.html` | 树规范：复用清单 / 搜索档位 / 键盘 / 懒加载 / 与 M4-M6 差异 / Phase 5 切片 / §09 全局实例 | **已进 main**（`:170` 裸尖括号已转义修复） |

## 3. 已落地：14 刀（分支 `feat/federation-entry`，按序）

| # | 提交 | 内容 |
| --- | --- | --- |
| 1 | `109b5616` | 拆目录模块（100% rename，纯搬文件） |
| 2 | `1894b182` | 三分法分组 `group_of` / `grouped`（根 3 / 叶 5，7 项单测） |
| 3 | `96a8299d` | 行模型 `NodeId` / `NodeKind` / `Row`（`MAX_DEPTH=3`、列不做节点、面包屑、`disabled()`） |
| 4 | `de0c2f8d` | 跨根搜索 `rank` / `search`（前缀 100 > 子串 70 > 子序列 40，上限 50 + refine 提示） |
| 5 | `eb91f4de` | 树态 `TreeState`（展开 / 搜索词 / 选中 / 最近使用 `RECENT_CAP=5`；`visible_rows` 折叠拍平） |
| 6 | `1dbc0760` | 行构造 `rows.rs`（文件行身份保留**原始相对路径**，目录行用规范化 `\` 键；超深度折叠并交代面包屑） |
| 7 | `22cc8c0b` | 行渲染 `render.rs`（`render_rows` + `dispatch`；几何全走 `workbench_shell::tree` 与 `ui` 常量；标签用 `Button::ghost().small()` 保键盘可达） |
| 8 | `dbcb1a19` | 数据库根改**懒加载树**：`NodeId` 增 `Conn`/`DbCatalog`/`DbSchema`；`db.rs` 的 `DbCache`（**空结果也记**，避免反复 introspect）；删掉 7 个扁平索引字段 |
| 9 | `d92d67d3` | 入口改三分法：tab 由 5 叶子换成 3 根；状态改 `picked: Option<Picked>`；`current_tab()` 由**选中的目标**决定（修掉老实现「按 tab 分派会答错」）；三个消费方调用零改动；结果集叶子显式置灰 |
| 10 | `8b06570f` | 拆壳：`load.rs`（I/O）+ `dialog.rs`（壳与行拼接），`mod.rs` 871 → 320 行（单文件瘦身目标达成） |
| 11 | `0610e059` | 搜索框 / 键盘 / 最近使用接上：对话框抬成 `Render` 实体 `PickerShell`（`InputState` 订阅要求 `Context<T>`）；`TreeState::step_selection` 新增 |
| 12 | `14369131` | 资产库清单改**只读连接**（原 `Connection::open` 与单写者模型冲突，且 DuckDB 的 open 会**创建**缺失库 → 打开一次选择器就在项目里平白多一个空 `analytics.duckdb`） |
| 13 | `a3ddf53b` | 行加类型图标（复用 `scratchpad_type_icon`，不另建映射）+ 打开即展开首根 |
| 14 | `49b9f3b9` | 大列表切虚拟视口（> `VIRTUALIZE_AFTER=60` 行走 `v_virtual_list`；短列表观感不变） |

模块现状（10 文件约 2850 行）：`dialog.rs` 592、`state.rs` 332、`rows.rs` 321、`db.rs` 309、`render.rs` 273、`mod.rs` 244、`load.rs` 217、`search.rs` 213、`node.rs` 205、`group.rs` 139。

## 4. 未完成（按优先级）

1. **全局表读取通道**（唯一未闭合的技术缺口，本轮已实测，**不是补一处 attach 就通吃**）：
   - 枚举侧早已成品：`analytics_resource/src/global/{mod.rs:34 GlobalInstanceStore, list.rs:9 list_global_resources}`，生产用点在 `jobs/refresh.rs:258-267`。
   - 执行侧**三条面都没有**全局 attach：① 进程级内存单例（`execution_service.rs:60/113` → `duckdb_service.rs:14` → `DuckDBManager::get_or_create_in_memory()`，二次分析 / 结果集 / 导出都在此）；② 联邦会话（`session.rs:381 attach_source` 是「挂载唯一实现」，`registry.rs:274-290 attach_sql()` 对 L1 源一律 `(READ_ONLY)`，全局库作为本地 duckdb 文件接进来**几乎零新增代码**，但只覆盖会话内）；③ 项目分析库（`project_db.rs:405-418` 只读口，另有 3 处直接 `Connection::open` 旁路：`project/src/store.rs:838`、`workbench/services/overview.rs:110`、`mock_generator.rs:68`）。
   - **洞察结构上取不到全局表**：取样按 `conn_id` 下推（`insight/jobs/table.rs:312-320` → `schema_report_view(conn_id, …)`；`insight_engine_internal/profile.rs:12` 原话「源库表先取样再探查……这里既没有也不需要源库连接信息」）。全局表没有 conn_id → 形态 I 不可达。
   - 结论：至少需 A（内存单例）+ B（联邦会话）两条；洞察仍需改形态 II 或造伪 conn_id。
2. **资产库剩余两段的取舍（待用户拍板）**：拆 `SourceKind::AnalysisGlobal` 按入口裁剪（`PickerConfig.kinds` 本就支持）/ 先做 A+B 再画段 / 冻结全局段。**本项目归档文件段同理**：`Picked` 只有 `Db/Draft/Analysis/External` 四变体，三个消费方只认这四个，新增目标类型＝能选不能执行。
3. **键盘语义已由源码定案（不必再人工试 Esc/↓）**，只剩三件真要手测的：
   - 键位派发顺序实测：**key binding 先于 `on_key_down` 派发**（`gpui-pre-0.3.11/src/window.rs:6412-6427` 先跑 `match_result.bindings`，被接住即 `return`；`finish_dispatch_key_event`（`:6484`）里的 `dispatch_key_down_up_event` 才跑监听）。单行 Input 绑了 `escape/up/down/left/right/pageup/pagedown/tab`（`vendor/gpui-base/src/input/base/state.rs:265-301`），但**单行模式不处理 ↑↓/Esc，会 propagate**（本仓 `quick_open/palette/render.rs:171-172` 的注释即此口径，那里正是把 `MoveUp/MoveDown/Escape` 挂在浮层根上接住）。
   - **Enter 是例外**：输入框自己处理并转成 `InputEvent::PressEnter { secondary, shift }`（枚举在 `state.rs:137-147`）。所以根上的 `on_key_down("enter")` 在搜索框聚焦时**永远收不到**——这就是实测到的「敲完搜索词回车没反应」。**已在 `dialog.rs` 修**：`PickerShell::new` 改收 `window`，订阅从 `cx.subscribe` 换成 `cx.subscribe_in`，在 `InputEvent::PressEnter`（仅无修饰）里 `push_recent` + 调 `strategy`。
   - 仍需手测（headless 测不到）：① 最近使用块选中后是否真出现、点它能否点亮确认按钮；② 大列表（>60 行）虚拟视口的滚轮与滚动条（`VirtualList` 用内部滚动句柄，未接 `track_scroll`）；③ 打开后焦点是否真落在搜索框（`window.focus(&handle, cx)` 两参，headless 无法断言真实焦点）。
4. **视觉验收从未做过**（助手无法看 UI）：行高 24px 内放 `Button::ghost().small()` 是否挤、面包屑是否过长、元信息是否该折叠。
5. **4 条待拍板**（设计文档 §9 剩余）：洞察联邦开关显示条件（推荐仅 ≥2 源）、远端扫描代价是否进确认框、Oracle 第三方插件是否走 `allow_unsigned_extensions`、二次分析默认档是否记住上次（推荐 `Local`）。另 `owner` 是否保持 `conn_id`（§1 #4）。
6. **原型 README 自列 5 处未画**：首次挂载进度与取消、扩展安装进度与设置页、洞察样本会话重建失效提示条、联邦档写语句被拒对话框、结果集「表已失效可重新分析」条目态。
7. **合并已完成**：`f87ff9d3`（`--no-ff`，零冲突），分支 `feat/federation-entry` 已删（was `d38d5572`）。合并走的是临时 worktree `.rds/tmp/wt-merge-main`（主工作区当时被他方未提交改动挡住，不能切分支），合并前打 tag `pre-merge-federation-entry`。**注意**：合并把并行会话在该分支上的 3 个提交（`65e439d3` DuckDB 扩展准入三处 / `fcf0ff7a` 痕迹区分 / `d38d5572` 插件页三方对照）一并带入 main——它们提交在本分支上，按分支合并必然同行。
8. **全量测试已解除阻塞并跑通**：他方在途改动（DuckDB 扩展管理的 `extension_store.rs` E0597 等）已由其自己修好，`cargo test -p rds-workbench --lib` 现在 **138 passed / 0 failed**（picker 65 项在内），`cargo check` 零告警。
9. **一处归属问题（仅记录，不建议动历史）**：Enter 修复的代码落在 `c70d6cd3`（并行会话「给没登记的 DuckDB 扩展两条出口」）里，而非独立提交——他们用 `commit -a` 把当时工作区里我这份未提交的 `dialog.rs` 一并卷走了。内容无误、main 上已生效；若要拆成独立提交需改写他方提交，风险大于收益，**不做**。

## 5. 关键事实（免重查，均本轮实读）

### 来源选择器（已拆目录，旧 `source_picker.rs` 行号全部作废）
- 对外契约在 `source_picker/mod.rs`：`SourceKind`（5 叶 + `label()`，`Analysis` 已正名「资产库」）、`TableCandidate`、`Picked`（`Db`/`Draft`/`Analysis`/`External`）、`PickerState`（`group`/`kinds`/`conns`/`db_cache`/`drafts`/`analysis_tables`/`picked`/`note`/`tree` + `has_target`/`selected_label`/`current_tab`/四个 `picked_*`）、`PickerConfig`、`ConfirmStrategy`。
- `state.rs:142` `visible_rows<'a>(&self, rows: &'a [Row]) -> Vec<&'a Row>`（**返回引用**，虚拟列表回调要 `'static` 时必须先落成自有 `Vec<Row>`）。
- `db.rs` 的 `DbCache` 深度规则：连接(1) → 单 catalog 时 schema(2) → 表(3)；多 catalog 时 catalog(2) → 表(3) 且 schema 折进标签；无 schema 层时表挂(2)。
- `collect_analysis_tables` 走 `ProjectDatabaseManager::analysis_db_path(&root)`，无项目回落 `global_analysis_db_path()`；SQL `WHERE table_schema='main'`（**限定不能去**，否则 `temp` schema 进入）；连接改 `Config::default().access_mode(AccessMode::ReadOnly)` + `open_with_flags`（照 `driver/src/native/duckdb.rs:250-258`）。
- 键盘：`dialog.rs` 的 `PickerShell::new(state, shared, config, strategy, search, window, cx)`——因为 Enter 必须订阅 `InputEvent::PressEnter`（`cx.subscribe_in`），而订阅要 `window`。↑↓←→/Esc 仍由根 `on_key_down` 接（单行 Input 会 propagate），这个分工是本轮实测定下来的，别改回「全都靠 on_key_down」。

### 资产库 / M6
- `crates/analytics_resource/src/model/kind.rs`：`RowScope{Project,Global}` `:7-13`；`ArchiveKind{File,Analysis,TableRef}` `:36-44`（`File` 本体 `{项目}/resources/`）；`label()` `:55-61`。
- `crates/analytics_resource/src/global/mod.rs:10-26` 双实例拓扑：全局表 `{system}/analytics.duckdb`（`G_` 前缀）+ 全局登记 `{system}/global.db`（`scope='global'`）；**项目 → 全局 → 项目是唯一法定路径**；**全局 DuckDB 单写者**，只能复用 `GlobalDatabaseManager::duckdb_conn()`。
- `LeftPanel::Resources` → 「资产库」（`workbench_shell/src/model.rs:69`）。

### 树原语
- `crates/workbench_shell/src/tree.rs`：`active_bar` `:38`、`disclosure_slot` `:55`、`disclosure_icon` `:65`、`count_badge` `:85`、`indent_spacer` `:105`、`indent_rem` `:112`、`RowHeight` `:123`、`row_sizes(count, rem_size, f) -> Rc<Vec<Size<Pixels>>>`；`:9-14` 明确**不抽通用树组件**；`:23-24` 槽宽 10px 是缩进算式一部分，不得改。
- `crates/workbench_shell/src/ui.rs:130`：`TREE_INDENT = 0.875`；`:240-244` 图标尺寸 `ICON_SIZE_XS/SM/MD`。
- 五个消费方（M4 导航 / M5 草稿箱 / M6 资产库 / M7 Mock / **picker**）已统一 `disclosure_icon`。

### 联邦会话（方案的基础）
- `engine/src/duckdb/federation/session.rs`：`FederatedSession` `:49`、`open` `:79`、`run` `:132`、`interrupt` `:266`、`l2_write_refusal` `:295`、`open_configured()` `:342`、`source_fingerprint` `:485`、`ensure_session` `:509`、`attach_source` `:381`（**挂载唯一实现**，失败记 `MountState::Failed` 不拖垮会话）。
- `manager.rs:18` 内存单例 `GLOBAL_DUCKDB`（全局 `Mutex`）——共用一条全局连接会与「取消洞察误伤编辑器」冲突，这是分层会话的物理依据。
- `temp_table.rs:539` `list_by_source(conn, source)` 按传入连接查 `information_schema`（`catalog=memory`、`schema=main`），可枚举会话内表。

## 6. 禁令与踩过的坑

- **不要**把 `SourceKind` 扩成 3 个或删枚举；三个根是**视图分组**，`group_of(kind)` 纯函数即可，`PickerConfig.kinds` / `has_target` / `confirm_prefix` 不动。
- **不要**让 picker 直接 `Connection::open` 全局分析库（单写者）。已改为只读连接 + `open_with_flags`，新代码沿用这个口子。
- **不要**去掉 `WHERE table_schema='main'`。
- **不要**把 `tmp_i_`（洞察样本）列进资产库/结果集列表，它登记为用户不可见。
- **改源码只用 `Edit` / `Write`**：本轮曾用 PowerShell `Get-Content | -replace | Set-Content` 改 `dialog.rs`，把 UTF-8 整体写坏、文件塌成一行。
- 测试模块**禁止** `use super::*`（会把 gpui 的 `test` 属性宏带进来 → `recursion limit reached`），显式列举导入。
- `Sizable` 在 `gpui_kit::component`（不在 `base`）；`window.focus(&handle, cx)` 是**两参**；`Icon::new(...)` 要 `IconNamed`（裸路径要包一层）。
- `v_virtual_list` 在 gpui-component 的 **crate 根**再导出（`lib.rs:121`），`component::virtual_list` 是私有模块。
- `scratchpad_type_icon` 的可用路径是 `scratchpad::scratchpad_view::scratchpad_type_icon`（`:108` 已 re-export）。
- **不要**在虚拟列表回调里借 `PickerState`（外层已握着借用 → `RefCell` 双借用 panic）。
- Shell 只用 PowerShell 7；过程产物进 `.rds/tmp/`；文档不进仓库根目录。native exe 的 stderr 会让 `$LASTEXITCODE` 非零，**不代表命令失败**，按 stdout 判断。
- 原型视觉 token 取自 `docs/architecture/insight/prototypes/workspace/workspace.html` 的 `:root`，不自造色板。
- 写 HTML 时注意标签配对（`.rds/tmp/check_html_balance.py` 可复用：`python <path> <files>`）。

## 7. 恢复上下文的最短路径

1. 读本文件 §1、§4。
2. `git log --oneline main..HEAD` + Read `source_picker/{mod,dialog,render}.rs` 拿到当前真实签名。
3. 读 `prototypes/entry/README.md`（分类映射 + 未画清单）与 `06-object-tree.html` §02 复用清单 + §07 落地切片 + §09 全局实例。
4. 复查执行侧证据（§4 第 1 条的行号）后再谈全局段。

## 8. 公共内核（分支 `feat/federation-kernel`，隔离 worktree）

> 2026-11-15。**物理隔离**：worktree `D:\RdataStation\RDS\wt-kernel`、分支 `feat/federation-kernel`（基点 `b145f1d7`）、`CARGO_TARGET_DIR=D:\RdataStation\RDS\target-kernel`、数据根 `D:\RdataStation\RDS\home-kernel`（与 `main` 不争 target / 数据文件）。

### 8.1 已落地（新增不接线，main 无消费方受影响）

| commit | 内容 | 验证 |
| --- | --- | --- |
| `dc342fd6` | 设计文档 `federation-kernel-design.md`（三段 + 7 刀） | —— |
| `d98ef2c4` / `9395a679` | 刀 1 `analysis/channel.rs`：`AnalysisChannel{Source, Accelerated, Federated{owner}}` + `ConnRef{Live, Historical}` + `from_toggle` | 27→34 项 |
| `80f37c86` / `9395a679` | 刀 2 `analysis/mount.rs`：`MountSpec` / `SessionPlan` / `plan_for`（Source 禁挂、Accelerated 恰 1 源且拒 `DuckDbTable`、Federated 去重 + primary 校验） | 同上 |
| `d5021b49` | 刀 3 `analysis/kernel.rs`：`PreparedMounts` + `SessionHandle{None, Accelerated, Federated}` + `open` | `--lib` 267 全绿 |
| `b141c607` | 三档**真跑**用例 `tests/analysis_kernel_probe.rs`（真 DuckDB 文件，含跨源 join） | 3 passed |

### 8.2 口径更正（本轮实测，覆盖旧说法）

1. **执行通道是三档**：源库直连（驱动执行、不挂源）/ 本地加速（`accel::ensure_session`，**单源**只读挂载，L2 驱动在此注册会话级 Secret）/ 联邦（`federation::ensure_session`，多源）。早先「Local」把前两档混成一个，已改。
2. **conn_id 要分两种**：归档资源表**有** `source_connection_id`（`analytics_resource/src/model/kind.rs:136-139`，`Option<String>`，取自草稿 `file_meta.last_connection_id`），但它是**归档当时**的历史 id、可能悬空；洞察取样（`resource_host/detail_actions.rs:84-96`、`insight/jobs/table.rs:312-320`）要的是**当前可连**的 conn。实测：`home-kernel/data/system/global.db` 的 `analytics_resources` **0 行**；`A1\.RSmeta\project.db` 唯一 1 行且该字段 `NULL`。
3. **全局连接是存在的**：`global.db` 的 `global_connections` **1 行**（`G_conn_SQLite_Example`，driver=sqlite）；项目库 `connections` **14 行**，其中 4 条 `GP_` 前缀共享连接（duckdb / mysql / pg / sqlite）。→ 解析 `ConnRef::Live` 必须对**合并后的清单**（`global_connections` + 项目 `connections`）做，只查一处会把共享连接误判为不可执行。
4. **三套单例 / 会话库不可合并**：进程级内存单例（`execution_service.rs:60/113 → duckdb_service.rs:14`）、accel 会话（按源 id）、federation 会话（按 owner + `manager.rs:18 GLOBAL_DUCKDB`）。合并会踩「取消洞察误伤编辑器」。
5. **`engine --lib` 基线**：本轮实测 **267**（旧文档里的 428 是更早、且统计口径不同的数字，别拿它当 lib 基线）。

### 8.3 环境坑（隔离 worktree 专属，已解）

- `.cargo/config.toml` 用 `force=true` 把 `TEMP/TMP/TMPDIR` 钉在 `.rds/tmp`（相对路径）。新 worktree 没有 `.rds` → `link.exe` 报 `LNK1104`。**解**：建 `wt-kernel\.rds\tmp`。
- `third_party/duckdb/1.5.6`（`duckdb.dll` 36.7 MB + `.lib` 2 MB + 头文件）未入库，worktree 缺失 → `build.rs` 找不到库。**解**：从主仓库 robocopy 进去（保持自包含）。
- 首次全量编译 **8 分 13 秒**（独立 target）。

### 8.4 剩余刀与阻点

- 刀 4-1（`56482c22`）：`MountSpec::SqlConnection` 带可选 `alias`（`sql_connection_as`）；`SessionPlan` 带 `notes`（`with_notes`）；`PreparedMounts` 改为**携带挂载键**（联邦 `Vec<(MountSpec, FederatedSource)>`、加速 `(MountSpec, AccelSource)`）；`open` 逐条比对挂载键并校验别名一致。验证 `analysis::` 49 + probe 3。
- 刀 4-2（`2361c064`）：编辑器 `federated_session` 改走 `plan_for + open`（新纯函数 `session_plan_for`），保留宿主的别名生成与 notes 文案。验证 `cargo check -p rds-editor --lib` 通过、`cargo test -p rds-editor --lib` **786 passed / 0 failed**。
- 刀 5（洞察）：受「开关显示条件」未决干扰；`SampleSource` 仍是 conn_id 下推形态，切内核前要先定「洞察拿 plan 还是拿 sample spec」。
- 刀 6（结果集二次分析）：默认档按建议取 `Accelerated`（结果集建表本来就在 DuckDB 里），**待你最终点头**。
- 刀 7（全局表）：A+B / 拆 `AnalysisGlobal` / 冻结 未决。

### 8.5 编译崩溃坑（2026-11-15 新踩）

`cargo check -p rds-editor --lib -j 2` 在编译 `toml` / `windows` 时崩溃：
`rustc exited with code 0xc0000409 (STATUS_STACK_BUFFER_OVERRUN)`——正是 `.cargo/config.toml` 里
`RUST_MIN_STACK` 要防的那类，且 `-j 2` 并发下仍会复现。**可用口径**：

```powershell
$env:CARGO_TARGET_DIR="D:\RdataStation\RDS\target-kernel"
$env:RUST_MIN_STACK="268435456"   # 显式给，不依赖 config 是否生效
cargo check -p rds-editor --lib -j 1
```

`-j 1` + 显式 `RUST_MIN_STACK` 一次通过（6 分 14 秒）。

### 8.6 结果集二次分析：入口链路已通，持久化被实测改路线

**已落地（三刀）**

| commit | 内容 |
| --- | --- |
| `d6ee516c` | `engine/src/duckdb/temp_table/promote.rs`：`write_temp_table_to_file_db(db_path, source, target, PromoteMode{Create,Replace})`（ATTACH → CTAS → DETACH），`temp_table::promote` 2 passed |
| `e7568ffb` | `result_panel/src/grid/menu.rs`：`ContextAction::AnalyzeResultSet` + `ContextTarget.analyze` + delegate `on_analyze_result_set`（`analyze_result_set_available` = 钩子在 + 有行，**不要求绑定连接**），`rds-result-panel` 260 passed |
| `6c556eff` | 编辑器 `analyze_result_set`（当前结果 → `ResultService::create_duckdb_temp_table` 物化成登记版 `tmp_q_`，网格字符串行经 `cell_json` 桥接）→ `EditorHost::open_insight_result_set` → workbench `result_set_actions::analyze_table`；`rds-editor` 786、`rds-workbench` 140、`insight_editor_entry` 2 passed |

链式再分析的实现依据：`tmp_q_` 与洞察取样的 `tmp_i_` 同在**进程内存单例**，同连接可见，
所以「分析结果集 → 产出结果集 → 再分析」只需一句 `SELECT * FROM 表`，不需要搬行数据。

**定性陷阱（实测，务必区分）**：`ContextTarget.insight`（洞察此列）要求结果**绑定连接**
（取样要回源库）；`ContextTarget.analyze`（分析此结果集）**不要求**——物化成 `tmp_q_` 是纯
本地动作。两者混用会让本地拼出来的结果永远拿不到整表入口。

**持久化（刀 6-②）被真跑改路线**（`ff998df6`，`crates/engine/tests/analysis_db_exclusive_probe.rs`）：

```text
ATTACH 失败：挂载目标库失败: IO Error: Cannot open file "...\.RSmeta\analytics.duckdb":
  另一个程序正在使用此文件，进程无法访问。
  File is already open in ...analysis_db_exclusive_probe.exe (PID 24516)
经项目连接写表：成功
```

- 项目写模式打开后，`ProjectDuckdbConnection` 持有一条长连接（`project_db.rs:276-278` 原文：Windows 上
  DuckDB 对同一文件仅允许一个连接句柄）；
- 于是**任何「第二连接 + ATTACH」都写不进项目分析库**——`promote` 原语只适用于「自己拥有的独立文件库」；
- 唯一可行：经 `ProjectDatabaseManager::duckdb_conn()` 那条既有写连接写，即数据要从内存单例
  **跨库搬运**（`tmp_q_` 读行 → 项目连接写入），不是 ATTACH 复制。
- 注意「既有写连接」在运行期是**按次开关**（`open_project_store`），所以 ② 的正确形态是：
  **一次项目库会话内完成「读单例行 → 建表 → 写行」**，而不是先 ATTACH 再指望锁一直可拿。

**关于 mock 落库（`workbench/src/services/mock_generator.rs`）的正确结论**（`crates/workbench/tests/mock_persist_exclusive.rs` 实测）：

```text
第二连接打开分析库：失败 → IO Error: Cannot open file "...\rds_mock_persist_open_*.RSmeta\analytics.duckdb":
  另一个程序正在使用此文件，进程无法访问。File is already open in ...(PID 26152)
```

即 `open_analysis_db` 那一步在「有 manager 活着」时必失败。**但运行期并不是长期持有**：
全仓 `ProjectDatabaseManager` 的使用都是**按次开关**（`connection_dialog/src/services/data_source_service.rs`
的 `open_project_store` 每次调用开一个，用完即释放；insight 的 store 同样按次开），
workbench 里没有任何字段长期持有 project manager。所以：

- mock 落库 / 追加**在常态下可以成功**（我先前「必然失败」的推断过强，此处更正）；
- 失败条件是**并发窗口**：正好有一次项目库操作（连接保存 / 洞察索引 / 资产操作）持着连接时，
  `open_analysis_db` 会撞锁报「另一个程序正在使用此文件」；
- 因此它属于**需要收敛的脆弱点**（应改为「复用当次已打开的 manager 连接」或加一次可读重试），
  不是「已经坏了」。验证前不要改它的实现。

### 8.7 结果集持久化：全链路已通（`2e5bb575` + `c2084233`）

**两条刀**

| commit | 内容 |
| --- | --- |
| `2e5bb575` | engine `promote::persistent_table_name(desc)` → `res_<描述>_<yyyymmddHHMMSS>`；`temp_table::sanitize_description` 改 `pub(crate)` 共用；用例钉住「不落在 `tmp_q_`/`tmp_i_`/`tmp_m_`/`tmp_p_` 任一临时族下」（否则 TTL 30 分钟会把它当临时表清掉），`temp_table::promote` 4 passed |
| `c2084233` | 菜单项「持久化为项目表（N 行）」→ 编辑器 `persist_result_set` → `EditorHost::persist_result_set` → workbench `result_set_actions::persist_to_project` → `services/persist_jobs`（worker + 单槽 + `WorkbenchView` 0.5s 泵回状态栏）；真库用例 1 passed |

**为什么是「一个物化 + 两个去处」**：`analyze_result_set` 与 `persist_result_set` 共用
`materialize_active_result_set("分析"/"持久化")`——同一份结果先物化成登记版 `tmp_q_`，
要么交给洞察取样（临时、随 TTL 走），要么搬进项目分析库（持久）。抽成一处是为了让「空结果 /
无行 / 物化失败」三条失败口径只有一份。

**搬运原语**：`write_table_across_connections(source, dest, source_table, target_table, Create)`
（源 `COPY … TO parquet` → 目标 `CREATE TABLE … AS SELECT * FROM read_parquet`）。
不用 ATTACH 的理由见 §8.6 实测；用 Parquet 而非 CSV 是为了 DECIMAL / 日期 / 带引号文本原样过一遍
（`tmp_q_probe` 的 `DECIMAL(10,4)` 落库后类型仍是 DECIMAL，已断言）。

**锁顺序（写死一处）**：先锁内存单例（源），再锁项目写连接（目标）。反过来的实现若在别处
出现，就会与这里凑成死锁；`persist_jobs` 注释里写明了「只此一处同时持两把锁」。

**新实测硬约束**：`persist_result_table` 内部自建 tokio runtime 并 `block_on`，
**不得在 async 上下文调用**——首次把用例写成 `#[tokio::test]` 直接 panic
`Cannot start a runtime from within a runtime`；口径是 worker 线程（普通 std 线程）+ 用例 `#[test]`。
`mock_generator` 那条链（`open_analysis_db` 新开连接）也有同类顺序问题，但它是**按次开关**下的
并发窗口，见 §8.6 更正后的结论，未改其实现。

**并发窗口的处理**：项目库打开撞锁做 `150ms × 3` 重试（`open_project_manager`），
仍失败才把「项目分析库正被别的操作占用」交给用户；撞名则提示换描述或删旧表——
不静默 `Replace`（覆盖别人的持久表不该是顺手的默认）。

**尚未做（需拍板）**：持久表的**资源归档**入口（用户口径「后续可能会进去」）；
持久表列表（当前只在状态栏告知表名，没有可视化列表）。




