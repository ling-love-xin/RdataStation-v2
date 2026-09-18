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
- 默认保留最近 5 份**内容副本**，超出只删副本，**版本行永久保留**——界面以"副本缺失"标注，不是错误。
- 保留策略有两个入口：设置页「资产库 › 历史内容保留」（`resources.keep_versions`：`0` = 只留元数据、`-1` = 全部保留）与归档对话框的「保留历史内容」（本次覆盖，留空 = 跟随设置）。领域类型是 [`KeepVersions`](src/model.rs)；`All` 在裁剪那一步是“不动作”，与 `0`（只留元数据）是两回事。

### 6. 边界与协作

- **归档由上游发起**（草稿箱右键 / 本地文件选择）：M6 **只收 `PathBuf` + 元数据**（`ArchiveRequest`），不认识 `ScratchpadStore` 等上游类型。依赖方向：`scratchpad → analytics_resource → engine, shared`。
- 变更事件 `ResourcesChanged { reason, resource_id }`：替代 v1 那个"发了没人听"的无载荷事件；**发/收两端必须同批落地**。
- 回收站只有一套：统一走项目级 `ProjectTrash`（`origin = "resources"`），跨模块还原必须被拒。

## 代码结构

| 文件 | 职责 | 状态 |
| --- | --- | --- |
| `src/lib.rs` | `AnalyticsResourceStore`（索引层入口）+ 模块声明 | ✅ |
| `src/model.rs` | 领域语义类型：`ArchiveKind` / `ReproductionStrength` / `ArchiveStatus` / `ArchiveBinding` / **`KeepVersions`（历史内容保留策略；设置项的有符号数在这里收成类型）** / `ArchiveRequest` / `CheckoutRequest` | ✅ Phase 0 + P2.4 |
| `src/payload.rs` | 本体层：`resolve` 守卫、归档搬运（跨设备兜底）、只读标记、sha256 指纹、**`file_size`（登记用的字节数）**、历史副本与裁剪（`store_version_copy` / `prune_version_copies`）、**版本副本查询与使用**（`version_copies` / `version_copy_file` / `delete_version_copy` / `copy_version_out` / `restore_version_copy`）、`replace_payload` / `move_payload_out`、`rel_path_taken` / `free_rel_path`（重名避让，命名规则只此一处） | ✅ Phase 0 + Phase 3 第一刀 |
| `src/models.rs` | 持久层行模型（v1 搬运 + 迁移 020 的 9 个新列） | ✅ |
| `src/service.rs` | 归档服务：归档（首次）/ 再归档（指纹未变即幂等、变了才增版本）/ 取回（检出）/ **还原到历史版本**（用旧内容生成新版本；无副本拒、指纹相同幂等）/ **取回历史版本** / **撤销归档**（本体移回 + 硬删行）/ **移入回收站**（批量，本体进 `ProjectTrash` + 登记行软删）/ **从回收站还原**（`origin` 校验 + 同名避让后登记行跟改）/ **按原相对路径还原**（索引修复那条）/ **永久删除与清空**（只动本模块的条目）编排 + **历史内容保留策略**（`KeepVersions`：全留 / 只留元数据 / 保留最近 n 份，两处裁剪合并为 `prune_copies`）+ `ResourcesChanged` 广播 | ✅ Phase 0 + Phase 3 + P0.8 + P2.4 |
| `src/indexer.rs` | 索引修复：`IndexRepair::{scan, adopt_file, accept_current_content, remove_orphan_record}`（扫描只报告、修复靠人工确认） | ✅ Phase 0（UI 已接：`dialogs/index_repair.rs`） |
| `src/resource.rs` | 登记 CRUD / 分页 / 搜索 / 排序（统一行映射 `map_resource_row` + 事务化 `update_resource`）+ 归档专用写入（`insert_archive` / `update_archive_content` / `find_archive_by_rel_path`；**体积与指纹同批写**，写入前夹紧到 `i32` 上限）+ 回收站索引侧（`soft_delete_archive` / `undelete_archive` / `find_deleted_archive_by_rel_path` / `purge_deleted_row` / `purge_all_deleted`）+ `hard_delete_row`（撤销归档与索引修复共用；两者都**连标签与分组关联一起删**——外键无 CASCADE，不清就删不掉） | ✅ 搬运 + 边界修复 + 新列接入 + P0.8 + P2.3 |
| `src/folder.rs` | 分组（v1 为自引用文件夹，按设计**降级为单层分组**）：建 / 改名 / 删除（**删分组不删存档**，成员回未分组）+ 移动语义（一个资源只在一个分组）+ `folders_by_resource`（一次查完） | ✅ 搬运 + Phase 2 第二刀 |
| `src/tag.rs` | 标签 CRUD（含 **`rename_tag` / `delete_tag`——补 v1 缺失的两项**，删标签连关联一起清）+ 双向查询 + **`tags_by_resource` / `tag_usage_counts`**（一次查完，避免 N+1） | ✅ 搬运 + Phase 2 第一刀 |
| `src/version.rs` | 版本历史；`save_resource_version_on` 支持在调用方事务内写快照；`version_counts()` 一次查完各存档的历史版本数（详情面板用）；`get_resource_versions()` 给版本历史对话框 | ⚠️ 仍为写前快照语义（指纹版本重写见 P0.9） |
| `src/models.rs` | 持久层行模型（v1 搬运；逐步并入 `model.rs`） | ✅ |
| `src/helpers.rs` | 时间双格式解析（RFC3339 / SQLite `CURRENT_TIMESTAMP`） | ✅ |
| `src/tests.rs` | 存储层回归（t001–t016，测试库跑齐 007 + 020） | ✅ 16 项 |
| `src/commands.rs` | Action 声明（`RequestArchive` / `OpenSelected` / `CheckoutSelected` / `DeleteSelected` / `SelectAllRows` / `FocusSearch` / `ClearSearch`，面板均已接处理器）；快捷键在 app 层绑到 `analytics-resource` context（`Ctrl+F` / `Esc` / `Delete` / `Ctrl+A`） | ✅ Phase 1 |
| `src/resource_view.rs` | 左 Dock 面板：**面板头（标题图标 + `＋ ▾` 归档入口 + `⋯` 四项：重建索引… / 打开资源目录 / 回收站… / 刷新）** / **工具栏（搜索·筛选·排序）** / 提示行 / **行列表（`list::List`：虚拟化 + 组件化 hover/选中/漫游；行自己接管点击：单击选中、`Ctrl` 切换、`Shift` 区间、双击打开——`classify_row_click`；`Ctrl+A` 全选、多选高亮自绘）** / **行右键菜单（打开·查看统计·取回·版本历史·复制路径·在系统中显示·移入回收站；多选时单选项置灰、删除项带数量）** / **加载态（3 行骨架 + 状态行前缀）** / **归档撤销栏（状态行上方，5 秒窗口）** / 状态行 / **两种空态（空库 vs 无匹配）**；快照带逐行详情（`selected_detail()`，右栏「存档详情」取它）；宿主动作经 `ResourcesHost`（菜单动作抽成 `HeaderMenuAction` + `dispatch_header_action`，弹层点不到也能测） | ✅ Phase 1 九刀（`F2` 重命名待重命名入口） |
| `src/filter.rs` | 工具栏**数据层**（纯函数，零 GPUI 依赖）：`ResourcesFilter`（关键字 / 种类 / **标签（id 多选，并集）** / 只看异常；`toggle_kind` 把"全选"规范化为不限、`toggle_tag` / `drop_tag` 管标签条件）+ `SortField`/`SortOrder`（**五个排序键：名称 / 归档时间 / 更新时间 / 大小 / 版本**，`ALL` / `label` / `short_label` / `arrow` / `flipped` / **`key`（落盘）/ `from_key` / `default_order`（每列的惯例方向）**）+ `apply_view`（比的是行上的**原始值**——时间戳与字节数；同键名称兜底且不随方向翻转，缺值（无体积 / 无归档时间）两个方向都排最后）+ **`build_visible_items`（分组折叠区：全部分组 → 未分组 → 各分组；计数从当前可见行现算）**；`is_empty()` 决定面板显示哪一种空态 | ✅ Phase 1 + Phase 2 第一 / 二 / 四 / 六刀 |
| `src/detail_view.rs` | 详情面板内容层：`ArchiveDetail` 快照 + `detail_rows`（基本信息 / 来源 / **版本（分区总是出现）** / 组织）+ `alert_line`（只在需处理时出现）+ `render_detail`（只读信息区 + **标签分区（chips + × + 「＋ 标签」）** + **版本区的「查看全部…」入口** + **动作区：打开（只读）/ 取回（检出）…** + **危险区：移入回收站**） | ✅ Phase 1 + Phase 3 第一刀 + P0.8 + Phase 2 第一刀（内容预览随后续批次） |
| `src/dialogs/archive.rs` / `checkout.rs` / `pick.rs` / **`version.rs`** / **`index_repair.rs`** / **`trash.rs`** / **`tag.rs`** / **`group.rs`** | 归档确认 / 取回（检出）/ 草稿多选 / **版本历史** / **索引修复** / **回收站** / **标签** / **分组名**对话框：种子（宿主备好的来源、字典与只读信息）+ 表单（名称 / 标签 / 保留份数；文件名 / 是否打开；勾选列表；版本表格 + 选中后动作栏；三分组 + 行内动作；条目表格 + 行内动作 + 清空；标签勾选 + 新建并打上 + 行内 ⋯（重命名 / 删除）；分组名单输入）+ 校验与解析（纯函数）+ `open_*_dialog`；后五者的状态可被宿主换行（`set_rows` / `set_options`）与收放忙态。回收站对话框**只列 `origin = "resources"` 的条目**，别人的只给一句说明 | ✅ Phase 1 + Phase 3 + P0.8 + Phase 2 三刀（执行由宿主注入的 `on_submit` / `on_action` 接手） |
| `src/present.rs` | 呈现层（纯函数，零 I/O 零 GPUI）：`format_size` / `format_scale` / `format_relative_time` / `format_timestamp` / `tail_for` / `to_row` / **`to_detail`** / **`tag_chips` / `tag_options`**（标签行 → 视图层的 chip / 筛选字典项）/ **`build_version_rows`**（当前版本 + 历史版本合成行、相邻版本差异）/ **`build_repair_rows`**（扫描报告 → 三分组修复行）/ **`build_trash_snapshot`**（回收站条目 → 本模块行 + 别人条目的统计）/ `build_snapshot`——索引行 → 面板快照（含字段优先级尾巴、逐行详情、标签与计数口径） | ✅ Phase 1 + Phase 3 + Phase 2 第一刀 |
| `src/ui.rs` | 视图尺寸常量（与 `workbench/ui.rs` 同源同值，但**在本 crate 声明**：依赖方向不允许反向读 workbench） | ✅ 16 项 |

> v1 的 `src/recycle.rs`（软删除表）与 `src/recycle_bin_dialog.rs`（占位）**已随 P0.8 删除**：回收站统一走项目级 `engine::persistence::trash::ProjectTrash`（一套回收站，模块硬约束 5）。

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
| **归档 / 取回 / 再归档闭环**【Phase 0】`ArchiveService`：本体 move + 登记 + 指纹版本 + 事件；索引失败回滚本体；取回产出可写工作副本 | — |
| **索引修复**【Phase 0 + Phase 3 第二刀 + P0.8】`IndexRepair`：三类孤儿（有文件无记录 / 有记录无本体 / 指纹不匹配）的扫描与人工确认修复；**对话框已接**（`dialogs/index_repair.rs` + `present::build_repair_rows` + 宿主接线）：三分组、行内动作（补登固定文件型 / **从回收站还原**（按原相对路径找条目）/ 删记录走确认 / 接受当前内容 / 打开版本历史），修完自动重扫换行 | 按目录批量补登 |
| **版本历史**【Phase 3 第一刀】`dialogs/version.rs` + `present::build_version_rows` + 宿主接线：当前版本与历史行同列（副本缺失行露出来）；**还原 = 生成新版本**（不原地回滚）、**取回该版本为草稿**、**删除内容副本**（`AlertDialog` 确认）；动作后宿主换行，不关窗；还原后的裁剪按设置项（见下行） | 详情面板的"最近 3 条"明细（需批量取数） |
| **回收站**【P0.8】`engine::persistence::trash::ProjectTrash`（上提 + 中性化：来源是字符串 `origin`、不返回 M5 类型）+ `dialogs/trash.rs` + 宿主接线：移入回收站（单选 / 多选批量，本体进 `.RSmeta/trash`、登记行**软删**——还原能恢复别名 / 标签 / 指纹）/ 还原（`origin` 校验、同名避让并同步登记路径）/ 永久删除 / 清空（`empty_origin`，**只动本模块的条目**）；索引修复的「从回收站还原」走 `restore_archive_by_rel_path` | 保留策略与回收站的联动（例如按时间自动清）；回收站的“打开原位置”入口 |
| **面板**【Phase 1 + Phase 2 第一 / 二 / 三刀】`ResourcesPanel`：面板头（标题图标 + `＋ ▾` / `⋯` 四项）/ 工具栏（搜索·筛选（含**标签维**）·排序）/ **`List` 虚拟化行（kind 图标 + 强度徽标 + 尾部字段）+ 右键菜单** / **批量多选（单击 / `Ctrl` / `Shift` / 双击手势 + `Ctrl+A`，多选时单选项置灰、删除带数量）** / **分组折叠区（会话级折叠）** / 状态行 / 两种空态；`present.rs` 把索引行转成快照（含逐行详情与标签）；**workbench 接线已落**（`panels/resources.rs` 装配 + `services/resource_jobs.rs` 后台取数 + `components/resource_host.rs` 端口）；**右栏「存档详情」已接**（`RightPanel::Archive`，宿主观察面板实体做选中联动） | `F2` 重命名（待重命名入口）、标题点击折叠（需与 M4/M5 面板头一起做）、折叠状态持久化（P2.4 后半） |
| **详情面板**【Phase 1 + P0.8 + Phase 2 第一刀】`detail_view.rs` 只读信息区（基本信息 / 来源 / 版本 / 组织 + 需处理提示条）+ **标签分区（chips：× 去标 / ＋ 标签开对话框）** + 动作区（打开（只读）/ 取回）+ **危险区（移入回收站）**；右 Dock 转发渲染与空态；版本数与标签都由存储层一次查完 | 内容预览、头部可编辑（改显示名 / 别名）、分组选择（Phase 2 下一刀） |
| **只读三重守卫**【Phase 1】①应用守卫（写入 `resources/` 直接拒，Phase 0）②**编辑器只读打开**（`editor::persist::open_file_read_only`，经 `OpenInEditorRequest` 带只读维度）③文件系统只读属性（辅助，失败只警告） | 本体异常时的修复入口（随索引修复对话框） |
| **归档 / 取回**【Phase 1】`dialogs/{archive,checkout,pick}.rs`：对话框 + 校验 + 冲突提示（`resources/x-2.sql`）；workbench 侧真执行（`resource_host` + `resource_jobs` 的 `Archive` / `Checkout` 作业 + 重名避让 + 回执 + 顺手打开）；**草稿箱入口**（面板头 `＋ ▾` + 草稿多选，来源连接与出处自动带出）；**归档可撤销**（`undo_archive` + 5 秒撤销栏） | 草稿箱右键入口、分组 / 别名字段（Phase 2）、`Ctrl+Z` |
| **工具栏数据层**【Phase 1 + Phase 2 第一 / 四 / 六刀】`filter.rs`：搜索（名称 + 尾部，大小写不敏感）/ 种类多选（全选 = 不限）/ **标签多选（id、并集，带用量）** / 只看需处理 / **五个排序键**（名称 / 归档时间 / 更新时间 / 大小 / 版本：同键翻转方向、同键名称兜底、**缺值两个方向都排最后**；比的是原始时间戳与字节数，不是格式化尾巴）；**选过的排序会被记住**（`resources.default_sort`：面板点排序 → 宿主写设置 → 下次构造期注入） | 搜索匹配别名 / 标签 / 来源表（新字段进 `ArchiveRow`，P2.3 余项） |
| **标签**【Phase 2 第一 / 三刀】`tag.rs`（改名 / 删除补齐 + 批量查询）+ 打标 / 去标 + **标签对话框**（勾选 + 新建并打上 + 差集提交 + 行内 **⋯：重命名 / 删除**）+ 详情面板 chips（× 即去标）+ **筛选菜单的标签维**，标签字典随快照下发（不另查库） | 标签颜色（现不使用用户填的 hex；颜色一律 token）、按标签排序、批量打标签（多选态） |
| **分组折叠区 + 管理**【Phase 2 第二 / 三 / 七刀】`folder.rs`（建 / 改名 / 删 + 移动语义）+ **列表按分组分区渲染**（全部分组 → 未分组 → 各分组，头带 2px 色条与计数）+ **折叠 / 展开**（**记忆到设置**：`resources.collapsed_groups` 按项目分桶；点一次写回全集）+ **管理入口**：行菜单「移动到分组 ›」（多选也走这条）+ 分组头右键（重命名 / 删除 / 新建）；计数从**当前可见行**现算 | **拖拽到分组头**（单独一刀，沿用 M5 行拖拽） |
| 领域类型（kind / 强度 / 状态 / 归档凭证）与本体层（守卫 / 搬运 / 只读 / 指纹 / 历史副本与裁剪 / 遍历） | — |
| **历史内容保留**【P2.4】`KeepVersions`（全留 / 只留元数据 / 保留最近 n 份）：设置页「资产库 › 历史内容保留」（`resources.keep_versions`，含 `-1` = 全部保留）+ 归档对话框本次覆盖（同样接 `-1`）；宿主主线程读设置 → 转领域类型 → 随归档 / 版本还原作业带入 | 目前只有这两条路径会裁剪；将来新增写副本的路径必须同样接上 |
| **默认排序 + 分组折叠记忆**【P2.4】`resources.default_sort`（面板里点排序即写回；“默认排序” = 上次用的那个）与 `resources.collapsed_groups`（折叠态，按项目分桶）；两者都是构造期注入 + 用户动作写回（注入不写，避免回声） | 默认分组（语义待拍板）；两项的位置都在设置架构 §14（Q7）的待确认视野里 |
| 回收站语义（项目级一套 + `origin` 归属；软删登记行保归属，永久删除连关联一起清） | 回收站的自动清理策略（按时间 / 容量） |
| 迁移 020 + 新列接入（写入 + 读取 + 按本体路径查重） | `kind` 过滤的**存储层**入口（面板已能按 kind 筛可见行） |
| 行映射从 v1 的 4 份收敛为 1 份；测试库跑齐 007 + 020 | — |
| 继承缺陷修复：分页除零与负数、`LIKE` 转义、连接嵌套、更新无事务、影响 0 行不报错、`parent_version_id` 语义、JSON 解析双策略、乱码副本名 | — |

## 设计与验证

- 设计（权威）：`docs/architecture/analytics_resource/` —— `README.md`（模块入口）· `analytics-resource-architecture.md`（语义裁决与数据流）· `analytics-resource-prototype-design.md` + `analytics-resource-prototype.html`（原型）· `analytics-resource-dev-plan.md`（进度与任务）· `analytics-resource-user-guide.md`（使用手册）。
- 验证：`cargo test -p rds-analytics-resource -j 2` → **124 项单测**（16 存储 + 5 领域 + 13 本体 + 18 归档服务 + 7 索引修复 + 16 筛选/排序/分区 + 9 面板 + 5 详情 + 14 呈现 + 8 对话框（归档 5 / 取回 2 / 草稿多选 1）+ 3 版本对话框 + 3 索引修复对话框 + 3 回收站对话框 + 3 标签对话框 + 1 分组对话框）+ `tests/panel_window.rs` **18 项面板窗口测试** + `tests/dialog_window.rs` **9 项对话框窗口测试**；编辑器侧 `cargo test -p rds-editor --lib -j 2` **217 项**（含 `persist` 的只读打开用例）；`cargo check -p rds-workbench --all-targets -j 2`、`cargo check -p rds-app -j 2` 与 `cargo check -p rds-analytics-resource --all-targets -j 2` 零告警。
- **命令约定**：全量编译/测试必须限制并发（`cargo test-all` / `cargo check-all` 别名，含 `-j 2` 与 `RUST_MIN_STACK`）——并发链接重型 crate 会耗尽内存（DuckDB 已改动态链接）。
