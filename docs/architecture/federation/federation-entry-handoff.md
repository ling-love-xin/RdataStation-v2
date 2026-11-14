# 联邦查询公共入口 · 交接说明

> 用途：原会话的上下文即将丢弃，本文件让新会话**不必重新调查**就能接着干。
> 更新时间：2026-11-14。仓库 `D:\RdataStation\RDS\RdataStation-v2`，分支 **`feat/federation-entry`**（基于 `c8eb6f86`，14 个提交，**未 rebase / 未合并 / 未推送**）。

## 0. 一句话现状

方案与 6 份原型**已进 main**；来源选择器的 Phase 5 改造**已在分支上落 14 刀**（全部在 `crates/workbench/src/components/source_picker/`，`cargo check` 零告警、picker 65 项测试全过）。**卡点不在代码，在执行侧**：资产库剩下的「本项目归档文件」「全局共享」两段都缺执行支持，画出来就是「能选不能执行」。

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
3. **需要人工跑起来验的三件事**（headless 测不出）：① 打开后直接按 `↓` 是否有反应；② 点进搜索框敲字后 `Esc` 是清词还是关窗（Input 的 escape action 与根 `on_key_down` 谁先，未确证）；③ 最近使用块选中后是否真出现、点它能否点亮确认按钮。**第 14 刀新增**：大列表（>60 行）虚拟视口的滚轮与滚动条是否正常（`VirtualList` 用内部滚动句柄，未接 `track_scroll`）。
4. **视觉验收从未做过**（助手无法看 UI）：行高 24px 内放 `Button::ghost().small()` 是否挤、面包屑是否过长、元信息是否该折叠。
5. **4 条待拍板**（设计文档 §9 剩余）：洞察联邦开关显示条件（推荐仅 ≥2 源）、远端扫描代价是否进确认框、Oracle 第三方插件是否走 `allow_unsigned_extensions`、二次分析默认档是否记住上次（推荐 `Local`）。另 `owner` 是否保持 `conn_id`（§1 #4）。
6. **原型 README 自列 5 处未画**：首次挂载进度与取消、扩展安装进度与设置页、洞察样本会话重建失效提示条、联邦档写语句被拒对话框、结果集「表已失效可重新分析」条目态。
7. **合并**：分支未 rebase 到 `main`、未合并、未推送。合并前需处理工作区里他方未提交的 10 项（未提交文件清单见 `git status`），合并用 `--no-ff`。

## 5. 关键事实（免重查，均本轮实读）

### 来源选择器（已拆目录，旧 `source_picker.rs` 行号全部作废）
- 对外契约在 `source_picker/mod.rs`：`SourceKind`（5 叶 + `label()`，`Analysis` 已正名「资产库」）、`TableCandidate`、`Picked`（`Db`/`Draft`/`Analysis`/`External`）、`PickerState`（`group`/`kinds`/`conns`/`db_cache`/`drafts`/`analysis_tables`/`picked`/`note`/`tree` + `has_target`/`selected_label`/`current_tab`/四个 `picked_*`）、`PickerConfig`、`ConfirmStrategy`。
- `state.rs:142` `visible_rows<'a>(&self, rows: &'a [Row]) -> Vec<&'a Row>`（**返回引用**，虚拟列表回调要 `'static` 时必须先落成自有 `Vec<Row>`）。
- `db.rs` 的 `DbCache` 深度规则：连接(1) → 单 catalog 时 schema(2) → 表(3)；多 catalog 时 catalog(2) → 表(3) 且 schema 折进标签；无 schema 层时表挂(2)。
- `collect_analysis_tables` 走 `ProjectDatabaseManager::analysis_db_path(&root)`，无项目回落 `global_analysis_db_path()`；SQL `WHERE table_schema='main'`（**限定不能去**，否则 `temp` schema 进入）；连接改 `Config::default().access_mode(AccessMode::ReadOnly)` + `open_with_flags`（照 `driver/src/native/duckdb.rs:250-258`）。

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
