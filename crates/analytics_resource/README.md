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
| `src/payload.rs` | 本体层：`resolve` 守卫、归档搬运（跨设备兜底）、只读标记、sha256 指纹、历史副本与裁剪、`replace_payload` / `move_payload_out` | ✅ Phase 0 |
| `src/models.rs` | 持久层行模型（v1 搬运 + 迁移 020 的 9 个新列） | ✅ |
| `src/service.rs` | 归档服务：归档（首次）/ 再归档（指纹未变即幂等、变了才增版本）/ 取回（检出）编排 + `ResourcesChanged` 广播 | ✅ Phase 0 |
| `src/indexer.rs` | 索引修复：`IndexRepair::{scan, adopt_file, accept_current_content, remove_orphan_record}`（扫描只报告、修复靠人工确认） | ✅ Phase 0 |
| `src/resource.rs` | 登记 CRUD / 分页 / 搜索 / 排序（统一行映射 `map_resource_row` + 事务化 `update_resource`）+ 归档专用写入（`insert_archive` / `update_archive_content` / `find_archive_by_rel_path`） | ✅ 搬运 + 边界修复 + 新列接入 |
| `src/folder.rs` | 分组（v1 为自引用文件夹，按设计**降级为单层分组**） | ✅ 搬运（改名/删除待补） |
| `src/tag.rs` | 标签 CRUD + 双向查询 | ✅ 搬运（改名/删除待补） |
| `src/version.rs` | 版本历史；`save_resource_version_on` 支持在调用方事务内写快照 | ⚠️ 仍为写前快照语义（指纹版本重写见 P0.9） |
| `src/recycle.rs` | v1 回收站（软删除 / 恢复 / 永久删除） | ⚠️ **待废弃**：改走 `ProjectTrash`（P0.8）。已知缺陷：`permanent_delete` 只删回收站行、主表与版本行永久残留 |
| `src/models.rs` | 持久层行模型（v1 搬运；逐步并入 `model.rs`） | ✅ |
| `src/helpers.rs` | 时间双格式解析（RFC3339 / SQLite `CURRENT_TIMESTAMP`） | ✅ |
| `src/tests.rs` | 存储层回归（t001–t016，测试库跑齐 007 + 020） | ✅ 16 项 |
| `src/commands.rs` / `resource_view.rs` / `recycle_bin_dialog.rs` | Action / 左 Dock 面板 / 回收站对话框 | ⬜ 占位（Phase 1） |
| `src/resource_view.rs` | 左 Dock 面板：面板头 / 提示行 / 行列表（强度徽标 + 尾部字段 + 选中）/ 状态行 / 空态；宿主动作经 `ResourcesHost` | ✅ Phase 1 第一刀（搜索·筛选·虚拟列表·右键菜单待下一批） |
| `src/filter.rs` | 工具栏**数据层**（纯函数，零 GPUI 依赖）：`ResourcesFilter`（关键字 / 种类 / 只看异常）+ `SortField`/`SortOrder` + `apply_view`（筛选→排序，同键名称兜底且不随方向翻转）；`is_empty()` 决定面板显示哪一种空态 | ✅ Phase 1 |
| `src/detail_view.rs` | 详情面板内容层：`ArchiveDetail` 快照 + `detail_rows`（基本信息 / 来源 / 版本 / 组织）+ `alert_line`（只在需处理时出现）+ `render_detail` 只读渲染 | ✅ Phase 1 |
| `src/ui.rs` | 视图尺寸常量（与 `workbench/ui.rs` 同源同值，但**在本 crate 声明**：依赖方向不允许反向读 workbench） | ✅ |

依赖方向：`analytics_resource → engine, shared`。视图层按 Phase 1 落地（届时依赖 `gpui-kit`；视图归属以 `docs/architecture/analytics_resource/analytics-resource-architecture.md` §8.2 为准）。

## 迁移

| 文件 | 内容 |
| --- | --- |
| `crates/engine/migrations/project_meta/007_analytics_resources.sql` | v1 原样：7 张表 + 索引 + 两个 `updated_at` 触发器 |
| `crates/engine/migrations/project_meta/020_analytics_resource_archive.sql` | **本轮新增**：9 个语义列（`kind` / `content_hash` / `file_rel_path` / `readonly` / `promoted_from` / `source_connection_id` / `source_table` / `definition_sql` / `archived_at`）+ `file_rel_path` 部分唯一索引 + kind/指纹索引。**不改 007**，老库直接升级（旧行默认 `kind = 'file'`、指纹为空） |

> 编号先到先得：019 已被 M8 洞察规则索引占用，故取 020；建文件前重新核对过。

## 能力状态

| 已实现 | 待补 |
| --- | --- |
| **归档 / 取回 / 再归档闭环**【Phase 0】`ArchiveService`：本体 move + 登记 + 指纹版本 + 事件；索引失败回滚本体；取回产出可写工作副本 | 面板与对话框（Phase 1） |
| **索引修复**【Phase 0】`IndexRepair`：三类孤儿（有文件无记录 / 有记录无本体 / 指纹不匹配）的扫描与人工确认修复；"从回收站还原"待 P0.8 | 版本保留策略接入设置项 |
| 领域类型（kind / 强度 / 状态 / 归档凭证）与本体层（守卫 / 搬运 / 只读 / 指纹 / 历史副本与裁剪 / 遍历） | 废弃 `recycle.rs` → `ProjectTrash`（P0.8，跨 crate） |
| 迁移 020 + 新列接入（写入 + 读取 + 按本体路径查重） | `kind` 过滤 / 列表按存档展示（Phase 1/2） |
| 行映射从 v1 的 4 份收敛为 1 份；测试库跑齐 007 + 020 | — |
| 继承缺陷修复：分页除零与负数、`LIKE` 转义、连接嵌套、更新无事务、影响 0 行不报错、`parent_version_id` 语义、JSON 解析双策略、乱码副本名 | — |

## 设计与验证

- 设计（权威）：`docs/architecture/analytics_resource/` —— `README.md`（模块入口）· `analytics-resource-architecture.md`（语义裁决与数据流）· `analytics-resource-prototype-design.md` + `analytics-resource-prototype.html`（原型）· `analytics-resource-dev-plan.md`（进度与任务）· `analytics-resource-user-guide.md`（使用手册）。
- 验证：`cargo test -p rds-analytics-resource -j 2` → **58 项单测**（16 存储 + 4 领域 + 11 本体 + 7 归档服务 + 7 索引修复 + 4 面板 + 5 详情 + 4 筛选/排序）+ `tests/panel_window.rs` **2 项窗口测试**；`cargo check -p rds-analytics-resource -j 2` 零告警。
- **命令约定**：全量编译/测试必须限制并发（`cargo test-all` / `cargo check-all` 别名，含 `-j 2` 与 `RUST_MIN_STACK`）——并发链接 DuckDB 静态库会耗尽内存。
