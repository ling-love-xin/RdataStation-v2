# 联邦查询公共入口 · 交接说明

> 用途：原会话的上下文即将丢弃，本文件让新会话**不必重新调查**就能接着干。
> 更新时间：2026-11-13。仓库 `D:\RdataStation\RDS\RdataStation-v2`，分支 `main`。

## 0. 一句话现状

方案与 6 份原型**已落盘**，产品裁决已收敛；**未改任何 Rust 代码**、**未 git 提交**。下一步只有两件事：`git add` 这批文档，或直接进入 Phase 5 的第一个最小窗口用例。

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
| `docs/architecture/federation/federation-entry-design.md` | 方案 + 5 阶段开发计划 + Oracle 第三方插件 ATTACH 契约（§4 已含三分法与全局段） | 已落盘，未提交 |
| `prototypes/entry/README.md` | 索引 + 根3/叶5 映射 + 对象树与搜索 + 未画清单 | 已落盘，未提交 |
| `prototypes/entry/00-overview.html` | 四屏总览 | 已落盘，未提交 |
| `prototypes/entry/01-source-picker.html` | **三分法 tab + 可折叠对象树 + 跨根搜索 + 最近使用 + 右侧元信息 + 资产库三段（含全局）** | 已落盘，未提交 |
| `prototypes/entry/02-insight-dialog.html` | 取样参数 + 会话三态 | 已落盘，未提交 |
| `prototypes/entry/03-result-analysis.html` | 分析菜单 / 自定义 SQL / 上限 / 会话三态 | 已落盘，未提交 |
| `prototypes/entry/04-editor-channel.html` | 三档 + 门控 + 写拒绝 | 已落盘，未提交 |
| `prototypes/entry/05-session-panel.html` | 会话头 + 7 种源状态 + 生命周期 | 已落盘，未提交 |
| `prototypes/entry/06-object-tree.html` | 树规范：复用清单 / 搜索档位 / 键盘 / 懒加载 / 与 M4-M6 差异 / Phase 5 切片 / §09 全局实例 | 已落盘，未提交 |

## 3. 未完成（按优先级）

1. **提交**：以上 9 个文件均未 `git add`。用户偏好「一刀一 commit」，建议拆两个：`docs(federation): 入口方案与开发计划` + `docs(federation): 入口原型（6 模块）`。
2. **全局表读取通道**（唯一未闭合的技术缺口）：`generate_attach_global_sql` 只有 `legacy.rs` 自身与测试调用，**生产无调用点**，所以本地单例 / 联邦会话都没挂全局库，选中 `G_` 表会 `table does not exist`。补之前先实测「单写者连接 + 只读 ATTACH 能否共存」（同一用例里持 `GlobalDatabaseManager::duckdb_conn()` 再开 `ATTACH '…' (READ_ONLY)`，看是否报锁）。
3. **5 条待拍板**（写在设计文档 §9）：洞察联邦开关的显示条件、远端扫描代价是否进确认框、Oracle 第三方插件是否走 `allow_unsigned_extensions`、二次分析默认档是否记住上次、owner 是否保持 `conn_id`。
4. **原型 README 自列 5 处未画**：首次挂载进度与取消、扩展安装进度与设置页、洞察样本会话重建失效提示条、联邦档写语句被拒对话框、结果集「表已失效可重新分析」条目态。
5. **开发计划 P0–P5 未启动**，无 Rust 改动。
6. 旁支：A1（Python 单元语言服务接线）已提交但**真机视觉验收至今无人工反馈**。

## 4. 关键事实（免重查，均本轮实读）

### 来源选择器
- `crates/workbench/src/components/source_picker.rs`：模块注释 `:1-18` 列五类；`SourceKind` `:42-53`；`label()` `:56-64`（`Analysis` → 「资源分析」）；`PickerState` `:79-100`（全是**扁平索引字段**）；`has_target()` `:129-135`（`ResultSet` 恒 false）；`open_source_picker` `:379-420`（打开即预载 conns/drafts/analysis，三种 note `:397-405`）。
- 三层浏览是**单列逐层追加**（`render_connected` `:514+`），不是树。
- `collect_analysis_tables` `:350-373`：路径 `ProjectDatabaseManager::analysis_db_path(&root)`，无项目回落 `global_analysis_db_path()`；SQL `WHERE table_schema='main'`（`:364`）——**限定不能去**，否则 `temp` schema 进入。
- `collect_drafts` `:304-321`：`list_local_entries(u32::MAX)` 递归拍平。

### 资产库 / M6
- `crates/analytics_resource/src/model/kind.rs`：`RowScope{Project,Global}` `:7-13`；`ArchiveKind{File,Analysis,TableRef}` `:36-44`（`File` 本体 `{项目}/resources/`，`Analysis` 本体 `{项目}/.RSmeta/analytics.duckdb` 且注释「第二期」）；`label()` `:55-61`（文件 / 分析表 / 引用）。
- `crates/analytics_resource/src/global/mod.rs:10-26` 双实例拓扑：全局表 `{system}/analytics.duckdb`（`G_` 前缀）+ 全局登记 `{system}/global.db`（`scope='global'`）；**项目 → 全局 → 项目是唯一法定路径，项目之间禁止直传**；**全局 DuckDB 单写者**，只能复用 `GlobalDatabaseManager::duckdb_conn()`，不得另开文件连接。
- `LeftPanel::Resources` → 「资产库」（`workbench_shell/src/model.rs:69`）。

### 树原语
- `crates/workbench_shell/src/tree.rs`：`active_bar` `:38`、`disclosure_slot` `:55`、`disclosure_icon` `:65`、`count_badge` `:85`、`indent_spacer` `:105`、`indent_rem` `:112`、`RowHeight` `:123`；`:9-14` 明确**不抽通用树组件**；`:23-24` 槽宽 10px 是缩进算式一部分，不得改。
- `crates/workbench_shell/src/ui.rs:130`：`TREE_INDENT = 0.875`。
- 四个消费方（M4 导航 / M5 草稿箱 / M6 资产库 / M7 Mock）已统一 `disclosure_icon`；picker 将是第 5 个。

### 联邦会话（方案的基础）
- `engine/src/duckdb/federation/session.rs`：`FederatedSession` `:49`、`open` `:79`、`run` `:132`、`interrupt` `:266`、`l2_write_refusal` `:295`、`open_configured()` `:342`、`source_fingerprint` `:485`（**不含主源**）、`ensure_session` `:509`（owner+fingerprint 双命中 `:633`）。
- `manager.rs:18` 内存单例 `GLOBAL_DUCKDB`（全局 `Mutex`）——共用一条全局连接会与「取消洞察误伤编辑器」冲突，这是分层会话的物理依据。
- `temp_table/temp_table.rs:41-66` 前缀族 `tmp_q_`/`tmp_i_`/`tmp_m_`/`tmp_p_`；`Insight` 登记为用户不可见。
- `collect_analysis_tables` 与 `mock_generator::analysis_tables` 同查法。

## 5. 禁令与踩过的坑

- **不要**把 `SourceKind` 扩成 3 个或删枚举；三个根是**视图分组**，`group_of(kind)` 纯函数即可，`PickerConfig.kinds` / `has_target` / `confirm_prefix` 不动。
- **不要**让 picker 直接 `Connection::open` 全局分析库（单写者）。现状代码正是直接 open，接入时必须换通道。
- **不要**去掉 `WHERE table_schema='main'`。
- **不要**把 `tmp_i_`（洞察样本）列进资产库/结果集列表，它登记为用户不可见。
- 结果集两种档位的可见范围必须区分（本机 vs 仅会话），否则点了报 `table does not exist`。
- Shell 只用 PowerShell 7；过程产物进 `.rds/tmp/`；文档不进仓库根目录。
- 原型视觉 token 取自 `docs/architecture/insight/prototypes/workspace/workspace.html` 的 `:root`，不自造色板。
- 写 HTML 时注意标签配对（本会话出现过 `<b>` 被 `</span>` 闭合、多余 `</code>`）。
- `Edit` 若报 tool timeout，**不要重发**：先检查文件是否已写入（本次一次超时的编辑实际未落盘，靠文件大小与章节编号确认）。

## 6. 恢复上下文的最短路径

1. 读本文件 §1、§3。
2. 读 `prototypes/entry/README.md`（分类映射 + 未画清单）。
3. 读 `06-object-tree.html` §02 复用清单 + §07 落地切片 + §09 全局实例。
4. 需要动代码时再读 `federation-entry-design.md` §6（5 阶段）与 `source_picker.rs` 对应行号。
