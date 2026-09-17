# rds-analytics-resource — M6 资产库 / 分析存档

> 本文件是 crate 的 **README 级入口**：只提炼模块特点与代码结构，完整设计以 `docs/architecture/analytics_resource/` 为准（架构约定：crate 内不复制设计文档）。

## 一句话定位

资产库是**项目的分析存档**：把「值得留存、需被引用、要能复现」的分析产物，从工作区转成**只读、有版本、带来源**的正式资产。

> 三个模块的分工：M4 看数据（活水镜像）→ M5 干活（工作区）→ **M6 留证据（冻结的存档 + 归档凭证）**。

## 模块特点

### 1. 三种 kind，本体各不相同（语义取"混合模型"）

| kind | 本体在哪 | 复现强度 | 期次 |
| --- | --- | --- | --- |
| `file` | `{项目}/resources/`（受管文件，归档后只读） | **强**：内容冻结，重跑得同解 | 第一期 |
| `analysis` | `{项目}/.RSmeta/analytics.duckdb`（靠定义重建） | 中：定义冻结，数据可重建 | 第二期 |
| `table_ref` | 远端（不在本机） | **弱**：只记"当时指向哪"，失效风险最高 | 不承诺 |

界面上**复现强度必须常显**（`已归档` / `分析表` / `引用`）——用户要一眼知道"这东西半年后还打不打得开"。

### 2. 归档凭证 = 来源 + 代码 + 内容指纹

这三样是 M6 不可替代的部分（也是与"书签册"的分水岭）：

| 要素 | 列 / 位置 | 回答的问题 |
| --- | --- | --- |
| 来源 | `promoted_from` / `source_connection_id` / `source_table` | 这结论是用什么数据得出的 |
| 代码 | 本体文件本身（`file`）或 `definition_sql`（`analysis`） | 怎么算的 |
| 内容指纹 | `content_hash`（sha256） | 还是不是当初那份 |

### 3. 目录与真相源

```text
{项目}/
├── resources/                       ← 本体（可见、归档后只读）
└── .RSmeta/
    ├── project.db                   ← 索引（登记 / 版本 / 标签 / 分组）
    ├── resources/versions/<id>/<v>/ ← 历史内容副本（按 keep 份数裁剪）
    └── trash/                       ← 项目级回收站（与草稿箱共用，origin = "resources"）
```

- **文件系统是本体权威，登记表是索引**；索引可重建，三类孤儿（有文件无记录 / 有记录无文件 / 指纹不匹配）由 `indexer` 检测、**人工确认**修复（不做静默导入 / 删除）。
- 内部态（`.RSmeta/**` 与任何点前缀路径）不得经 API 触达——`PayloadStore::resolve` 是唯一入口守卫。

### 4. 只读是常态，修改走「取回（检出）」

| 层 | 手段 |
| --- | --- |
| 应用守卫（硬约束） | `PayloadStore::ensure_writable` 对 `resources/` 下的写入直接拒绝并给出可操作提示 |
| 编辑器 | 以只读模式打开（编辑器既有的"分析资源锁定"来源） |
| 文件系统 | 只读属性（辅助；Windows / 网络盘不可靠，失败只警告） |

取回的坑已处理：**Windows 下 `CopyFile` 会继承源文件的只读属性**，`copy_out` 会显式清除，否则"取回的文件改不了"。

### 5. 版本由内容指纹触发，历史内容有保留上限

- `content_hash` 未变**不产生新版本**（否则"改个别名"也会消耗一个版本号——v1 的行为）。
- 版本表是**写前快照**语义：当前版本在资源行上、不进版本表；`parent_version_id` 指向本次写入的快照行（v1 写的是资源自身 id，无信息量）。
- 默认保留最近 5 份**内容副本**（设置项 `keepVersions`）；超出只删副本，**版本行永久保留**——界面以"副本缺失"标注，不是错误。

### 6. 边界与协作

- **归档由上游发起**（草稿箱右键 / 本地文件选择）：M6 **只收 `PathBuf` + 元数据**（`ArchiveRequest`），不认识 `ScratchpadStore` 等上游类型。依赖方向：`scratchpad → analytics_resource → engine, shared`。
- 变更事件 `ResourcesChanged { reason, resource_id }`：替代 v1 那个"发了没人听"的无载荷事件；**发/收两端必须同批落地**。
- 回收站只有一套：统一走项目级 `ProjectTrash`（`origin = "resources"`），跨模块还原必须被拒。

## 代码结构

| 文件 | 职责 | 状态 |
| --- | --- | --- |
| `src/lib.rs` | `AnalyticsResourceStore`（索引层入口）+ 模块声明 | ✅ |
| `src/model.rs` | 领域语义类型：`ArchiveKind` / `ReproductionStrength` / `ArchiveStatus` / `ArchiveBinding` / `ArchiveRequest` / `CheckoutRequest` | ✅ Phase 0 |
| `src/payload.rs` | 本体层：`resolve` 守卫、归档搬运（跨设备兜底）、只读标记、sha256 指纹、历史副本与裁剪（`store_version_copy` / `prune_version_copies`）、**版本副本查询与使用**（`version_copies` / `version_copy_file` / `delete_version_copy` / `copy_version_out` / `restore_version_copy`）、`replace_payload` / `move_payload_out`、`rel_path_taken` / `free_rel_path`（重名避让，命名规则只此一处） | ✅ Phase 0 + Phase 3 第一刀 |
| `src/models.rs` | 持久层行模型（v1 搬运 + 迁移 020 的 9 个新列） | ✅ |
| `src/service.rs` | 归档服务：归档（首次）/ 再归档（指纹未变即幂等、变了才增版本）/ 取回（检出）/ **还原到历史版本**（用旧内容生成新版本；无副本拒、指纹相同幂等）/ **取回历史版本** / **撤销归档**（本体移回 + 硬删行）编排 + `ResourcesChanged` 广播 | ✅ Phase 0 + Phase 3 第一刀 |
| `src/indexer.rs` | 索引修复：`IndexRepair::{scan, adopt_file, accept_current_content, remove_orphan_record}`（扫描只报告、修复靠人工确认） | ✅ Phase 0 |
| `src/resource.rs` | 登记 CRUD / 分页 / 搜索 / 排序（统一行映射 `map_resource_row` + 事务化 `update_resource`）+ 归档专用写入（`insert_archive` / `update_archive_content` / `find_archive_by_rel_path`）+ `hard_delete_row`（撤销归档与索引修复共用） | ✅ 搬运 + 边界修复 + 新列接入 |
| `src/folder.rs` | 分组（v1 为自引用文件夹，按设计**降级为单层分组**） | ✅ 搬运（改名/删除待补） |
| `src/tag.rs` | 标签 CRUD + 双向查询 | ✅ 搬运（改名/删除待补） |
| `src/version.rs` | 版本历史；`save_resource_version_on` 支持在调用方事务内写快照；`version_counts()` 一次查完各存档的历史版本数（详情面板用）；`get_resource_versions()` 给版本历史对话框 | ⚠️ 仍为写前快照语义（指纹版本重写见 P0.9） |
| `src/recycle.rs` | v1 回收站（软删除 / 恢复 / 永久删除） | ⚠️ **待废弃**：改走 `ProjectTrash`（P0.8）。已知缺陷：`permanent_delete` 只删回收站行、主表与版本行永久残留 |
| `src/models.rs` | 持久层行模型（v1 搬运；逐步并入 `model.rs`） | ✅ |
| `src/helpers.rs` | 时间双格式解析（RFC3339 / SQLite `CURRENT_TIMESTAMP`） | ✅ |
| `src/tests.rs` | 存储层回归（t001–t016，测试库跑齐 007 + 020） | ✅ 16 项 |
| `src/commands.rs` | Action 声明（`RequestArchive` / `OpenSelected` / `CheckoutSelected` / `DeleteSelected` / `FocusSearch` / `ClearSearch`，面板均已接处理器）；快捷键在 app 层绑到 `analytics-resource` context（`Ctrl+F` / `Esc` / `Delete`） | ✅ Phase 1 |
| `src/resource_view.rs` | 左 Dock 面板：**面板头（标题图标 + `＋ ▾` 归档入口 + `⋯` 四项：重建索引… / 打开资源目录 / 回收站… / 刷新）** / **工具栏（搜索·筛选·排序）** / 提示行 / **行列表（`list::List`：虚拟化 + 组件化 hover·选中·键盘漫游，选中以面板的行 id 为准；行首带 kind 图标）** / **行右键菜单（打开·取回·版本历史·复制路径·在系统中显示·移入回收站）** / **加载态（3 行骨架 + 状态行前缀）** / **归档撤销栏（状态行上方，5 秒窗口）** / 状态行 / **两种空态（空库 vs 无匹配）**；快照带逐行详情（`selected_detail()`，右栏「存档详情」取它）；宿主动作经 `ResourcesHost`（菜单动作抽成 `HeaderMenuAction` + `dispatch_header_action`，弹层点不到也能测） | ✅ Phase 1 八刀（批量多选待下一批） |
| `src/filter.rs` | 工具栏**数据层**（纯函数，零 GPUI 依赖）：`ResourcesFilter`（关键字 / 种类 / 只看异常；`toggle_kind` 把"全选"规范化为不限）+ `SortField`/`SortOrder`（`label` / `arrow` / `flipped`）+ `apply_view`（筛选→排序，同键名称兜底且不随方向翻转）；`is_empty()` 决定面板显示哪一种空态 | ✅ Phase 1 |
| `src/detail_view.rs` | 详情面板内容层：`ArchiveDetail` 快照 + `detail_rows`（基本信息 / 来源 / **版本（分区总是出现）** / 组织）+ `alert_line`（只在需处理时出现）+ `render_detail`（只读信息区 + **版本区的「查看全部…」入口** + **动作区：打开（只读）/ 取回（检出）…**） | ✅ Phase 1 + Phase 3 第一刀（动作接线经 `DetailActions` 注入；内容预览随后续批次） |
| `src/dialogs/archive.rs` / `checkout.rs` / `pick.rs` / **`version.rs`** | 归档确认 / 取回（检出）/ 草稿多选 / **版本历史**对话框：种子（宿主备好的来源与只读信息）+ 表单（名称 / 标签 / 保留份数；文件名 / 是否打开；勾选列表；版本表格 + 选中后动作栏）+ 校验与解析（纯函数）+ `open_*_dialog`；版本历史的状态可被宿主换行（`set_rows`）与收放忙态 | ✅ Phase 1 + Phase 3 第一刀（执行由宿主注入的 `on_submit` / `on_action` 接手） |
| `src/present.rs` | 呈现层（纯函数，零 I/O 零 GPUI）：`format_size` / `format_scale` / `format_relative_time` / `format_timestamp` / `tail_for` / `to_row` / **`to_detail`** / **`build_version_rows`**（当前版本 + 历史版本合成行、相邻版本差异）/ `build_snapshot`——索引行 → 面板快照（含字段优先级尾巴、逐行详情与计数口径） | ✅ Phase 1 + Phase 3 第一刀 |
| `src/ui.rs` | 视图尺寸常量（与 `workbench/ui.rs` 同源同值，但**在本 crate 声明**：依赖方向不允许反向读 workbench） | ✅ 8 项 |
| `src/recycle_bin_dialog.rs` | 回收站对话框 | ⬜ 占位（Phase 3） |

依赖方向：`analytics_resource → engine, shared`（视图层另依赖 `gpui-kit`；上游是 `scratchpad → analytics_resource`）。视图归属（**入本 crate**）以 `docs/architecture/analytics_resource/analytics-resource-architecture.md` §8.2 为准。

## 迁移

| 文件 | 内容 |
| --- | --- |
| `crates/engine/migrations/project_meta/007_analytics_resources.sql` | v1 原样：7 张表 + 索引 + 两个 `updated_at` 触发器 |
| `crates/engine/migrations/project_meta/020_analytics_resource_archive.sql` | **本轮新增**：9 个语义列（`kind` / `content_hash` / `file_rel_path` / `readonly` / `promoted_from` / `source_connection_id` / `source_table` / `definition_sql` / `archived_at`）+ `file_rel_path` 部分唯一索引 + kind/指纹索引。**不改 007**，老库直接升级（旧行默认 `kind = 'file'`、指纹为空） |

> 编号先到先得：019 已被 M8 洞察规则索引占用，故取 020；建文件前重新核对过。

## 能力状态

| 已实现 | 待补 |
| --- | --- |
| **归档 / 取回 / 再归档闭环**【Phase 0】`ArchiveService`：本体 move + 登记 + 指纹版本 + 事件；索引失败回滚本体；取回产出可写工作副本 | 五个对话框与动作真实现（Phase 1 对话框批） |
| **索引修复**【Phase 0】`IndexRepair`：三类孤儿（有文件无记录 / 有记录无本体 / 指纹不匹配）的扫描与人工确认修复；"从回收站还原"待 P0.8 | 版本保留策略接入设置项 |
| **版本历史**【Phase 3 第一刀】`dialogs/version.rs` + `present::build_version_rows` + 宿主接线：当前版本与历史行同列（副本缺失行露出来）；**还原 = 生成新版本**（不原地回滚）、**取回该版本为草稿**、**删除内容副本**（`AlertDialog` 确认）；动作后宿主换行，不关窗 | 详情面板的"最近 3 条"明细（需批量取数）、版本保留策略接入设置项 |
| **面板**【Phase 1】`ResourcesPanel`：面板头（标题图标 + `＋ ▾` / `⋯` 四项）/ 工具栏（搜索·筛选·排序）/ **`List` 虚拟化行（kind 图标 + 强度徽标 + 尾部字段）+ 右键菜单** / 状态行 / 两种空态；`present.rs` 把索引行转成快照（含逐行详情）；**workbench 接线已落**（`panels/resources.rs` 装配 + `services/resource_jobs.rs` 后台取数 + `components/resource_host.rs` 端口）；**右栏「存档详情」已接**（`RightPanel::Archive`，宿主观察面板实体做选中联动） | 批量多选、标题点击折叠（需与 M4/M5 面板头一起做）、五个对话框与动作真实现 |
| **详情面板**【Phase 1】`detail_view.rs` 只读信息区（基本信息 / 来源 / 版本 / 组织 + 需处理提示条）+ 动作区（打开（只读）/ 取回）；右 Dock 转发渲染与空态；版本数由存储层一次查完 | 内容预览、危险区（随各自批次） |
| **只读三重守卫**【Phase 1】①应用守卫（写入 `resources/` 直接拒，Phase 0）②**编辑器只读打开**（`editor::persist::open_file_read_only`，经 `OpenInEditorRequest` 带只读维度）③文件系统只读属性（辅助，失败只警告） | 本体异常时的修复入口（随索引修复对话框） |
| **归档 / 取回**【Phase 1】`dialogs/{archive,checkout,pick}.rs`：对话框 + 校验 + 冲突提示（`resources/x-2.sql`）；workbench 侧真执行（`resource_host` + `resource_jobs` 的 `Archive` / `Checkout` 作业 + 重名避让 + 回执 + 顺手打开）；**草稿箱入口**（面板头 `＋ ▾` + 草稿多选，来源连接与出处自动带出）；**归档可撤销**（`undo_archive` + 5 秒撤销栏） | 草稿箱右键入口、分组 / 别名字段（Phase 2）、`Ctrl+Z` |
| **工具栏数据层**【Phase 1】`filter.rs`：搜索（名称 + 尾部，大小写不敏感）/ 种类多选（全选 = 不限）/ 只看需处理 / 两种排序键（同键翻转方向、同键名称兜底） | 标签维与更多排序键（需 `ArchiveRow` 带原始值，Phase 2） |
| 领域类型（kind / 强度 / 状态 / 归档凭证）与本体层（守卫 / 搬运 / 只读 / 指纹 / 历史副本与裁剪 / 遍历） | 废弃 `recycle.rs` → `ProjectTrash`（P0.8，跨 crate） |
| 迁移 020 + 新列接入（写入 + 读取 + 按本体路径查重） | `kind` 过滤的**存储层**入口（面板已能按 kind 筛可见行） |
| 行映射从 v1 的 4 份收敛为 1 份；测试库跑齐 007 + 020 | — |
| 继承缺陷修复：分页除零与负数、`LIKE` 转义、连接嵌套、更新无事务、影响 0 行不报错、`parent_version_id` 语义、JSON 解析双策略、乱码副本名 | — |

## 设计与验证

- 设计（权威）：`docs/architecture/analytics_resource/` —— `README.md`（模块入口）· `analytics-resource-architecture.md`（语义裁决与数据流）· `analytics-resource-prototype-design.md` + `analytics-resource-prototype.html`（原型）· `analytics-resource-dev-plan.md`（进度与任务）· `analytics-resource-user-guide.md`（使用手册）。
- 验证：`cargo test -p rds-analytics-resource -j 2` → **87 项单测**（16 存储 + 5 领域 + 13 本体 + 11 归档服务 + 7 索引修复 + 6 筛选/排序 + 6 面板 + 5 详情 + 9 呈现 + 6 对话框 + 3 版本对话框）+ `tests/panel_window.rs` **11 项面板窗口测试** + `tests/dialog_window.rs` **5 项对话框窗口测试**；编辑器侧 `cargo test -p rds-editor --lib -j 2` **217 项**（含 `persist` 的只读打开用例）；`cargo check -p rds-workbench --all-targets -j 2` 与 `cargo check -p rds-analytics-resource --all-targets -j 2` 零告警。
- **命令约定**：全量编译/测试必须限制并发（`cargo test-all` / `cargo check-all` 别名，含 `-j 2` 与 `RUST_MIN_STACK`）——并发链接重型 crate 会耗尽内存（DuckDB 已改动态链接）。
