# 资产库 / 分析存档模块（M6）· 模块入口

> **一句话**：把「值得留存、需被引用、要能复现」的分析产物，从工作区转成**只读、有版本、带来源**的正式存档——回答的不是"我能看到什么数据"（M4），也不是"我正在做什么"（M5），而是**"我留下了什么，它当时长什么样"**。
>
> 本文只提炼**特点 / 边界 / 代码地图 / 硬约束**；细节一律指向本目录内文档，**不复制设计**。
> 状态：**设计定稿（2026-09-15）；Phase 0 三批 + Phase 1 十二刀已落地**——归档 → 取回 → 再归档（指纹版本）闭环 + 变更事件 + 索引修复（三类孤儿）已可用（`ArchiveService` / `IndexRepair`）；面板（面板头 **`＋ ▾` 双入口（草稿箱 / 本地文件）** / **工具栏搜索·筛选·排序** / **`List` 虚拟化行 + kind 图标 + 右键菜单（打开·取回·复制路径·在系统中显示·移入回收站）** / 状态行 / **加载态（骨架）** / **归档撤销栏** / 两种空态）、**Action 与快捷键（`Ctrl+F` / `Esc` / `Delete`）**、**存档详情接入右栏（`RightPanel::Archive`，选中联动 + 打开 / 取回动作 + 指纹复制）**、**归档 / 取回对话框与真执行（含草稿多选、重名避让、回执与顺手打开、来源连接自动带出）**、**归档撤销（`undo_archive`，5 秒窗口）**、**只读三重守卫（编辑器只读打开，P1.6）**、呈现层（索引行 → 面板快照，含逐行详情）与 **workbench 接线（面板挂载 + 后台取数 + 端口回执）** 均已落地。逐项证据见 `analytics-resource-dev-plan.md` §0 进度记录。
> 仍待：**草稿箱右键**侧的归档入口（面板头 `＋ ▾` 那条已落） · 版本历史 / 回收站 / 索引修复三个对话框（Phase 3；回收站需先上提 `ProjectTrash`，P0.8） · 批量多选（含 `F2` / `Ctrl+A`） · 废弃 `recycle.rs` → `ProjectTrash` · 版本保留策略接设置项。
>
> **边界**：本模块拥有**归档与取回 / 存档登记 / 版本与内容指纹 / 标签与分组 / 检索 / 资源侧回收站 / 索引修复**。连接与内省属 M3/M4；工作区文件读写属 M5；DuckDB 计算属 M2；Mock 生成属 M7；洞察计算属 M8；**项目级 → 系统级提升属 M1**（另立设计）。本模块**不自己取数、不自己计算、不改工作区文件**——只做搬运 + 登记 + 冻结 + 检索。

## 1. 模块特点

### 产品行为

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **存档 = 归档凭证** | 一条存档的不可替代部分是**来源 + 代码 + 内容指纹**三件套，不是"一行记录指向某处" | 架构 §2.3 |
| **复现强度必须可见** | `已归档`（强）/ `分析表`（中）/ `引用`（弱）在列表与详情常显——用户必须一眼知道半年后还打不打得开 | 原型 §1 原则 1、§6 |
| **本体与显示名分离** | 重命名只改显示名，不动物理路径（避免破坏外部引用与 git 历史） | 原型 §1 原则 2、§11 决策 7 |
| **只读是常态** | 面板**不提供"编辑资源"**；要改就走"取回（检出）"，改完再归档为新版本（与 git 签出→改→提交同构） | 原型 §1 原则 3、§4.2 |
| **归档入口来自上游** | 不存在"凭空新建存档"：入口是草稿箱右键 + 面板头下拉（来自草稿箱 / 本地文件） | 原型 §2.1、§4.1 |
| **版本只追加** | 回滚 = 用旧内容生成新版本；不做原地回滚、不做行级 diff | 原型 §4.3 |
| **异常必须可处理** | 本体缺失 / 内容已变 / 索引不一致都有检测、呈现与人工修复入口（状态行常显计数） | 原型 §4.5、§5 |

### 语义与数据

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **三种 kind，本体各不相同**（语义取 C 模型） | `file` → `{project}/resources/`；`analysis` → `.RSmeta/analytics.duckdb`；`table_ref` → 远端 | 架构 §2.2 |
| **第一期只做 `file`** | 上游（草稿箱归档）是唯一确定入口；`analysis` 第二期；`table_ref` 最后或不承诺 | 架构 §2.2、开发方案 §0 决策 3 |
| **文件系统是本体权威，登记表是索引** | 索引可重建；孤儿（有文件无记录 / 有记录无文件 / 指纹不匹配）必须可修 | 架构 §3、§7.2 |
| **版本由内容指纹触发** | `content_hash` 未变**不产生新版本**；无 hash 不得递增版本（v1 是"写前元数据快照"，改个别名也消耗版本号） | 架构 §5.1 |
| **历史内容默认留 5 份** | 超出只删内容副本，版本行永久保留；`keepVersions` 可配（`0` = 只留元数据） | 架构 §5.2 |
| **回收站统一项目级** | 走 `ProjectTrash`（`origin = "resources"`），v1 的 `analytics_recycle_bin` 表弃用 | 架构 §7.1 |
| **`scope` 派生只读** | 住哪个库 = 什么作用域；禁止手填（v1 的"global"只是标签，数据一直在项目库） | 架构 §4.3 |
| **`config` JSON 降级** | 核心字段进独立列；`config` 只放 kind 专属扩展，且必须在 UI 有入口 | 架构 §4.2 |
| **标签为主 + 单层分组** | 标签多值、跨 kind 通用、扶正为主组织方式；文件夹降级为单层分组，不做自引用树 | 原型 §2.4、§11 决策 4 |

### 架构与约束

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **依赖只向下** | `scratchpad → analytics_resource → engine, shared`；M6 **不依赖 M5**（归档入参是 `PathBuf` + 元数据，不是 `ScratchpadStore` 类型） | 架构 §6.1 |
| **上游发起、本模块执行** | 归档由知道"我有什么"的一方发起（M5），落点与登记由本模块负责 | 架构 §6.1 |
| **事件带 reason** | `ResourcesChanged { reason, resource_id }`，替代 v1 那个"发了没人听"的无载荷事件；**发/收两端同批落地** | 架构 §6.2 |
| **归档顺序：先本体、后索引、失败回滚** | v1 是"先建记录、再删草稿"（第二步失败留下重复且无幂等键） | 架构 §6.3 |
| **只读三重守卫** | 应用守卫（写入 API 拒绝）+ 编辑器只读打开 + 文件系统属性（辅助，失败只警告） | 架构 §6.3、原型 §4.6 |
| **render 期零 I/O** | 列表/详情只读内存快照，I/O 一律后台任务 + 结果回填（沿用 M4 `nav_jobs` 模式） | 原型 §5 |
| **零裸值 / 组件不手搓** | 颜色走主题 token（四档语义色足够，不新增产品 token）；尺寸进 `ui.rs`（新增 4 项）；列表/对话框/菜单用 gpui-kit 组件 | 原型 §6/§7/§8 |
| **视觉通道预算** | kind 图标一律 `muted`，**强度徽标是行内唯一色块**（一行只允许一个颜色信号） | 原型 §6 |

### 工程与文档

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **地基先于功能** | Phase 0 先修 `engine` 连接池（`busy_timeout` / `acquire` 超时 / 归还语义）+ 目录常量收敛 + 迁移 020，再谈 UI | 开发方案 §2 |
| **v1 遗产分级对待** | 留用 store 层约 55%；`recycle.rs` 419 行整体作废；`version.rs` 重写；前端仅"行为契约"可复用（组件树是空的） | 开发方案 §1.1、原型 §10 |
| **v1 前端是空的** | `AnalyticsResourceManager.vue` 仅 122 行占位卡片列表 + 新建弹窗；其余 15 个组件全部未接线，`initStore()` 无生产调用方 | 原型 §10 |
| **文档先于实现** | 本轮出模块入口 + 架构（语义裁决）+ 原型设计 + 交互稿 + 开发方案；实现按 Phase 0–5 推进 | `analytics-resource-dev-plan.md` |

## 2. 边界

- **做**：归档（草稿/本地文件 → 受管只读本体）、取回（检出工作副本）、存档登记与索引、内容指纹与版本、标签与单层分组、检索与排序、资源侧回收站、索引修复（三类孤儿）。
- **不做**：数据源连接与内省（M3/M4）、工作区文件读写（M5）、DuckDB 计算与临时表（M2）、Mock 生成（M7）、洞察计算与报告（M8）、项目级→系统级提升（M1）、多级文件夹树与页码分页、`config` 手工编辑、版本行级 diff、自动索引修复、归档本体原地编辑、v1 设计文档从未实现的 17 条命令（`extract_table` / `generate_sql_reference` / `check_delete_safe` / `cleanup_expired` …）。

## 3. 代码地图（括号内为现状）

| 想改 | 去哪 |
| --- | --- |
| 领域类型（`ArchiveKind` / `ReproductionStrength` / `ArchiveStatus` / `ArchiveBinding` / 归档与取回请求） | `crates/analytics_resource/src/model.rs`（现状：✅ Phase 0 已实现） |
| 本体层（`resources/` 定位、越界拒绝、move、只读、指纹、历史副本与裁剪） | `crates/analytics_resource/src/payload.rs`（现状：✅ Phase 0 已实现，`PayloadStore`；`rel_path_taken` / `free_rel_path` 提供重名避让） |
| 归档服务（归档 / 取回 / 再归档 / 撤销 + 变更事件） | `crates/analytics_resource/src/service.rs`（现状：✅ Phase 0 已实现 `ArchiveService`；`undo_archive` 提供 5 秒反悔窗口） |
| 索引修复（三类孤儿检测与修复） | `crates/analytics_resource/src/indexer.rs`（现状：✅ Phase 0 已实现 `IndexRepair`） |
| 登记 CRUD / 分页 / 搜索 / 排序 | `crates/analytics_resource/src/resource.rs`（现状：✅ 搬运 + 边界修复：统一行映射、分页夹紧、`LIKE` 转义、事务化更新；`hard_delete_row` 供撤销与索引修复） |
| 分组（单层） | `crates/analytics_resource/src/folder.rs`（现状：✅ 搬运，含树字段待去掉） |
| 标签与双向查询 | `crates/analytics_resource/src/tag.rs`（现状：✅ 搬运，补改名/删除） |
| 版本（内容指纹版本） | `crates/analytics_resource/src/version.rs`（现状：⚠️ 仍写前快照，但已支持在调用方事务内写快照 + 返回快照行 id；`version_counts()` 批量给详情用） |
| 回收站 | `crates/analytics_resource/src/recycle.rs`（现状：⚠️ **整体作废**，改走 `ProjectTrash`） |
| 左 Dock 面板（列表 / 工具栏 / 状态行） | `crates/analytics_resource/src/resource_view.rs`（现状：✅ Phase 1 八刀；`list::List` 虚拟化 + kind 图标 + 行右键菜单（含复制路径 / 在系统中显示）已落；**加载态（骨架 + 状态行前缀）**已落；**面板头（标题图标 + `＋ ▾` + `⋯` 四项）**已落；批量多选待下一批） |
| 工具栏规则（搜索 / 筛选 / 排序） | `crates/analytics_resource/src/filter.rs`（现状：✅ 纯函数 + 单测；标签维待 Phase 2） |
| 详情属性面板（内容层） | `crates/analytics_resource/src/detail_view.rs`（现状：✅ 只读信息区 + **动作区（打开（只读）/ 取回…）**；内容预览 / 危险区随各自批次） |
| 归档 / 取回对话框 | `crates/analytics_resource/src/dialogs/{archive,checkout,pick}.rs`（现状：✅ 表单 + 校验 + 冲突提示；**草稿多选**（pick）；执行由宿主注入的 `on_submit` 接手） |
| 详情面板的落点（右 Dock 档位） | `crates/workbench/src/panels/right.rs`（`RightPanel::Archive`：转发渲染 + 空态）、`crates/workbench/src/view.rs`（观察资产库面板 → 唤醒右栏） |
| 呈现层（索引行 → 面板快照） | `crates/analytics_resource/src/present.rs`（现状：✅ Phase 1；含 `to_detail` / `format_timestamp`——宿主桥只需"取数 → 调它 → 推快照"） |
| 版本历史 / 回收站 / 分组 / 标签对话框 | `src/{version_view,recycle_view,folder_view,tag_view}.rs`（现状：`recycle_bin_dialog.rs` 3 行占位） |
| 归档 / 取回 / 索引修复对话框 | `src/dialogs/{archive,checkout,index_repair}.rs`（现状：未创建） |
| Action 与快捷键 | `src/commands.rs` + `crates/app/src/main.rs`（现状：6 个 Action 均有面板处理器；`Ctrl+F` / `Esc` / `Delete` 已绑到 `analytics-resource` context；`↑↓` / `Enter` 由列表组件提供） |
| 左 Dock 装配（面板实体 + 渲染转发） | `crates/workbench/src/panels/resources.rs`（现状：✅ 已接线：构造期建实体 + 宿主端口 + 轮询回填） |
| 取数后台任务（列表 + 索引健康 + 版本数） | `crates/workbench/src/services/resource_jobs.rs`（现状：✅ 单工作线程 + 结果槽；指纹扫描在工作线程上） |
| 宿主端口实现 | `crates/workbench/src/components/resource_host.rs`（现状：✅ 归档 = 文件选择 → 对话框 → 入队；取回 = 对话框 → 入队；**打开 = 编辑器只读打开本体**；**打开资源目录 / 刷新**是真实现；移入回收站 / 回收站 / 索引修复仍给明确回执） |
| 编辑器只读来源（三重守卫之二） | `crates/editor/src/persist.rs`（`open_file_read_only`）、`crates/workbench/src/panels/shared.rs`（`OpenInEditorRequest` 带只读维度）、`crates/workbench/src/view.rs`（`open_in_editor_with`） |
| 占位渲染 | ✅ 已下线（`render_resources_placeholder` 随接线删除） |
| 迁移（加 9 列） | `crates/engine/migrations/project_meta/020_analytics_resource_archive.sql`（现状：✅ 已新增，含库层契约测试 t016；**不改 007**） |
| 项目级回收站（上提 + 中性化） | `crates/scratchpad/src/trash.rs` → `crates/engine/`（现状：M5 类型泄漏，待中性化） |
| 连接池（`busy_timeout` / `acquire` 超时 / 归还语义） | `crates/engine/src/persistence/project_db.rs`（现状：⚠️ 三个缺陷，见架构 §13.2） |
| 尺寸常量 | `crates/analytics_resource/src/ui.rs`（现状：✅ 8 项；**不在 `workbench/ui.rs`**——依赖方向不允许视图反向读 workbench） |
| 契约测试范围 | `crates/workbench/tests/ui_contract.rs`（显式文件清单，**不含本 crate 视图**；本 crate 的尺寸/裸值约束暂由自身单测 + 评审保证） |
| 接线（workspace 别名 / workbench 依赖 / 渲染入口） | `Cargo.toml` 的 `[workspace.dependencies]`（✅ 别名已加）、`crates/workbench/Cargo.toml`（✅ 依赖已加）、面板渲染入口（✅ 已接） |
| crate 入口文档 | `crates/analytics_resource/README.md`（现状：✅ 已补，特点提炼 + 代码结构 + 能力状态） |

数据链路：`scratchpad（发起归档）→ analytics_resource::service（编排）→ payload（本体：resources/ 或 analytics.duckdb）+ store（索引：project.db）`；删除 → `ProjectTrash`。**无 HTTP / IPC 层。**

## 4. 改这个模块前必须遵守

1. **文件系统是本体权威**：`resources/` 有什么就是有什么；登记表只是索引，且必须能重建。
2. **归档后本体不可写**：写入 API 在路径落入 `resources/` 时直接拒绝（错误文案指向"先取回"）；不提供"编辑资源"入口。
3. **版本必须绑定内容指纹**：`content_hash` 未变不得递增版本；无 hash 的版本行非法。
4. **归档顺序固定**：先 move 本体 → 再写索引 → 索引失败回滚本体；**不得**先写索引后搬文件。
5. **回收站只有一套**：一律走项目级 `ProjectTrash`；跨模块还原必须被拒（`origin` 校验）。
6. **`scope` 只读**：由存储位置派生，禁止作为可编辑字段或筛选维度落地。
7. **核心语义不进 `config`**：新字段先进独立列，再考虑 `config` 扩展位。
8. **M6 不依赖 M5**：归档入参用 `PathBuf` + 元数据；依赖方向 `scratchpad → analytics_resource`。
9. **事件发/收同批落地**：新增事件必须同时有生产方与消费方（v1 的 `analytics-resource-changed` 是反例）。
10. **索引修复必须人工确认**：不静默导入、不静默删除。
11. **render 零 I/O**：后台任务 + 结果回填（M4 `nav_jobs` 模式）。
12. **零裸值 / 组件不手搓 / 稳定 id / 简体中文注释**：id 永不变，不做下标键；`cargo` 命令固定 `-j 2`。

## 5. 测试与验证

```sh
cargo test -p rds-analytics-resource --lib -j 2
cargo test -p rds-scratchpad --lib -j 2      # 回收站上提后的回归
cargo test -p rds-engine --lib -j 2          # 连接池与目录常量
cargo test -p rds-workbench --test ui_contract -j 2
cargo check --workspace --all-targets -j 2
```

- **当前基线（2026-09-17）**：`79 单测 + 4 对话框窗口测试 + 11 面板窗口测试`全绿（`cargo check --all-targets` 零告警）；编辑器侧 `cargo test -p rds-editor --lib` **217 项**（含只读打开）。
- 测试场景 T1–T16 见 `analytics-resource-dev-plan.md` §9（归档回滚 / 指纹未变不增版本 / 历史裁剪 / 跨模块还原被拒 / 三类孤儿 / 越界写入 / 跨设备 move 等）。
- **基线**：v1 的 15 个存储用例改造后全绿且不得减少。
- 真机矩阵：明暗主题 × 三类 kind × 异常三态；平台矩阵：Windows（只读属性最弱）/ macOS / Linux（大小写敏感）。

## 6. 文档地图

| 文档 | 什么时候读它 |
| --- | --- |
| `analytics-resource-architecture.md` | **为什么这样设计 / 怎么运转**：语义裁决书（D1–D12）/ 三种 kind 与复现强度 / 归档凭证三件套 / 存储布局与真相源规则 / 数据模型与对 v1 `007` 的处置 / 版本语义 / 归档取回契约（含顺序与回滚）/ 回收站与索引修复 / 分层与接线缺口 / 降级矩阵 / 已知问题 19 条 |
| `analytics-resource-prototype-design.md` | **长什么样 / 怎么交互**：面板解剖（头 / 工具栏 / 行解剖 / 分组 / 状态行）/ 详情面板 / 归档取回版本回收站修复五条交互 / 状态与空态矩阵 / 主题映射与尺寸常量（含对话框宽度）/ GPUI 落点 / **§10 与 V1 的逐项对照** |
| `analytics-resource-prototype.html` | 可交互示意稿（明暗双主题；12 个场景：默认 / 归档 / 取回 / 版本 / 回收站 / 索引修复 / 空库 / 无结果 / 异常态 / 筛选排序 / 右键菜单 / 只读被拒） |
| `analytics-resource-dev-plan.md` | **做什么、做到哪**：已确认决策 / 现状盘点（55% 可留、`recycle.rs` 作废）/ Phase 0–5 任务与落点 / §8 明确不做 / T1–T16 测试场景 / R1–R9 风险 / 验证命令 / 实现位置映射 |
| `analytics-resource-user-guide.md` | **怎么用**：能力与期次标注 / 入口 / 界面导览（行怎么读 · 复现强度三档 · 状态行）/ 典型流程（归档 → 取回 → 再归档 · 版本 · 回收站 · 索引修复）/ 只读与数据安全 / FAQ 排查 / USIT 验收清单 |
| `../overview.md` | M6 定位与 crate 依赖方向（§九大模块与 crate 对应） |
| `../scratchpad/scratchpad-dev-plan.md` | Phase D（归档/取回的上游，D1–D6 是本模块落点的另一半） |
| `../scratchpad/scratchpad-prototype-design.md` | 项目目录结构约定（`resources/` / `scratchpad/` / `.RSmeta/`）与"模块独立根目录"决策 |
| `../editor/editor-prototype-design.md` | 编辑器只读来源之一即"分析资源锁定"（§1.4）——本模块落地后需接上 |
| `../ui/ui-design-spec.md` | 三层约束（主题 token / 尺寸常量 / 组件规格） |

> 五件套已齐（模块入口 / 原型设计 / 交互稿 / 架构 / 开发方案 / 使用手册）。若要对外交付或培训，可基于手册 §3–§4 补充录屏 / 动图。

## 7. 下一步

| 类别 | 项 |
| --- | --- |
| 已拍板（不阻塞） | 语义 C 模型 · 命名（资产库 / 分析存档 / 归档 / 取回）· 三种 kind 与第一期范围 · 指纹版本 · 项目级回收站 · `scope` 派生 · 标签为主 + 单层分组 · 只读常态（开发方案 §0） |
| Phase 0（先做，无 UI） | ✅ 已落地（crate 内）：crate 入口文档 · workspace 别名 · 迁移 020 + 新列接入 · 领域类型 · 本体层 · **归档/取回/再归档闭环 + 变更事件** · **索引修复（三类孤儿）** · 行映射 4 份→1 份 · 9 项继承缺陷修复 · `mod tests` 接线（此前未编译）｜⬜ 待续（需跨 crate 或后续阶段）：engine 连接池修复（`busy_timeout` / `acquire` 超时）· `.RSmeta` 常量去重 · `ProjectTrash` 上提中性化与 `recycle.rs` 废弃 · 版本保留策略接入设置项 · 测试改走 `engine::migration`（详单见开发方案 §0） |
| Phase 1 | ✅ 面板骨架 · ✅ 行渲染（含 **kind 图标**）· ✅ 工具栏（搜索 / 筛选 / 排序）· ✅ 两种空态 · ✅ **列表虚拟化（`list::List`）+ 行右键菜单** · ✅ 详情面板内容层 · ✅ 呈现层 · ✅ 术语收尾（`f93d560`）· ✅ **workbench 接线（面板挂载 + 快照桥）** · ✅ **Action 与快捷键（`Ctrl+F` / `Esc` / `Delete`）** · ✅ **存档详情接入右栏（`RightPanel::Archive` + 选中联动）** · ✅ **归档 / 取回对话框与真执行（含重名避让与回执）** · ✅ **归档撤销栏（`undo_archive`，5 秒窗口）** · ✅ **只读三重守卫（编辑器只读打开，P1.6）** · ✅ **面板头 `＋ ▾` 双入口 + 从草稿箱归档（草稿多选对话框）** · ✅ **面板头 `⋯` 四项 + 标题图标**；⬜ 草稿箱**右键**侧入口 · ⬜ 标题点击折叠（随“自绘面板头统一”批） · ⬜ 批量多选（含 `F2` / `Ctrl+A`） |
| Phase 2 | 标签（补改名/删除）+ 单层分组 + 搜索筛选排序 + 设置项 + 多选批量 |
| Phase 3 | 版本历史 + 历史内容保留 + 回收站对话框 + 索引修复对话框 + 异常态呈现 |
| Phase 4 | `analysis` 档（DuckDB 表）+ M7 Mock 产物 / M5 编辑器结果两处上游接入 |
| Phase 5（不承诺） | `table_ref` 档 · M1 系统级提升衔接 · 依赖追踪 · FTS5 |
| 待确认 | 视图归属（本方案取"入 crate"，若拍板"留 workbench"则文件落点平移，架构 §8.2） |
