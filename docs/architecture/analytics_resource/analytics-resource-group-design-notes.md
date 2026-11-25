# 资产库分组设计：讨论纪要与待分析清单

> 状态：**讨论纪要，未落代码**。本文件为 2026-11-25 一次对话的结论沉淀，供另开会话做细粒度设计。
> 数据形态、文件行号均来自本仓库源码实读（2026-11-25，`main`）；标「建议」的条目是待你拍板的设计取向，不是既有事实。

## 1. 背景与范围

- 本轮 UI 统一主线已推进到「分组（Group）体系」，涉及三个模块：**数据导航栏**、**草稿箱**、**资产库（资源库）**。
- 用户已认可方案 **A**：资产库的分组管理**只管自己的资产分组**（`analytics_folders`），不把连接分组混进同一个视图。
- 用户提出的新形态（本文待细化的主体）：点资产库工具栏「分组」→ 面板**左侧出现一个分组栏（sheet/rail）**，该栏同时承担 ①决定资产列表显示哪些分组 ②分组管理 ③每个分组下方可展开「类似草稿箱的目录结构」。
- 用户同时提出疑问：**标签与分组的职责是否重复**，标签到底针对分组还是针对最小单元（表/文件）。

本会话**已落地**的相关代码只有一处（与本文设计无关，但同一功能域）：

- `9065f5e0` `fix(nav): 恢复右键「编辑分组…」入口（连描述一起改）` —— 导航分组右键现在两项并存：「重命名分组」= 组名原格就地编辑；「编辑分组…」= 名称 + 描述表单，两者共用 `save_group_form`。

## 2. 现状事实（已实读，可作为设计的硬前提）

### 2.1 三个模块其实是「两份数据、三处入口」

| 模块 | 分组存储 | 成员 | 现有管理入口 |
|---|---|---|---|
| 数据导航栏 | `connection_groups(id,name,description)` + `connection_group_members`（多对多 + 组内排序，**项目级**） | 数据库连接 | ①组头右键：重命名 / 编辑分组… / 新建 / 上移 / 下移 / 删除；②面板头「管理分组」弹窗（列表 + 成员数 + 重命名 + 合并到… + 删除） |
| 资产库 | `analytics_folders`（`af_<uuid>`，**单层**、同名去重、拒绝父分组） | 存档资源（文件 / 分析库表 / 引用） | 只有分散动作：行与详情「移动到分组 ›」、新建/重命名/删除分组的小对话框；**没有集中列表** |
| 草稿箱 | **没有自己的分组表** | — | 无。它的「移动到分组」实质是**归档时选资产分组**（`crates/scratchpad/src/scratchpad_view.rs:586` 注释写在归档口径里） |

- 「资产库分组 ≈ 导航 + 草稿箱 的合集」的成因：资产库的成员来自两处归档来源（连接对象、草稿文件），归档时分组/标签随来源继承；但**分组表本身只有 `analytics_folders` 一份**。
- 因此本设计的实际对象是「两份分组、一个管理屏」，不是「三份分组合并」。

### 2.2 资产分组（`analytics_folders`）结构

`crates/analytics_resource/src/store/folder.rs`：

- 行字段（`map_folder_row`）：`id / name / scope / parent_folder_id / sort_order / color / icon / created_at / updated_at / deleted_at`。
  - `parent_folder_id` 是**历史遗留列**：`create_folder` 显式拒绝非空父分组（源码报「分组是单层的（没有子分组）——不能指定父分组」）。
  - `color` / `icon` 目前存在但界面上未用于分组栏。
- 读写 API：`create_folder` / `rename_folder` / `delete_folder` / `get_folder_by_id` / `list_folders(scope, ...)` / `add_resource_to_folder` / `remove_resource_from_folder` / `clear_resource_folder` / `folders_by_resource`（`resource_id → folder_id` **单值映射**）。
- **删除语义已核实**（`delete_folder:134-173`，单事务）：分组行写 `deleted_at`（软删）+ **`DELETE FROM analytics_resource_folder WHERE folder_id = ?`**——成员一律退回「未分组」，**没有「成员迁到另一个分组」的选项**（这与导航侧「管理分组」弹窗的「合并到…」不同）。
- 同名约束按 `scope`（`folder_name_taken(name, scope)`），不按父分组（因为单层）。

### 2.3 筛选语义（改动时必须保住的部分）

- 分组筛选是**多选并集**，不是单选：`crates/analytics_resource/src/resource_view/filters.rs:78` 注释「勾选/取消一个分组（多选是并集；`GROUP_UNGROUPED` 是『未分组』伪 id）」，常量在 `crate::filter::{GROUP_ALL, GROUP_UNGROUPED}`。
- 工具栏「分组」按钮（`crates/analytics_resource/src/resource_view/panel_render/chrome.rs:218`，`debug_selector = archive-group`）当前是**下拉菜单**：分组字典逐项带勾 + 分隔线 + 「未分组」，点勾即 `panel.toggle_group`。
- 工具栏「标签」按钮（同文件 `:266`，`archive-tag`）是**同一个条件**（`filter.tags`）的第二个入口，与「筛选」菜单的标签节互相同步；标签菜单项文案为 `名称（N）`。

### 2.4 列表里的分组呈现

- 分组头组件：`crates/analytics_resource/src/resource_view/list_delegate/group_header.rs`，`list_id = archive-group-{key}`，2px 分组色条（宽取 `ui::GROUP_BAR_WIDTH`，即外壳 `NAV_GROUP_BAR_WIDTH`），可与导航分组头共用 `workbench_shell::tree::GroupHeader` 口径。
- 分组折叠状态已持久化：宿主按项目分桶写 `resources.collapsed_groups`（`resource_view/mod.rs:186` 附近注释，传的是**当前全集**而非增量）。

### 2.5 资源自身已带层级线索（决定「二级」怎么来的关键）

- 资源档位只有三档（`crates/analytics_resource/src/model/kind.rs:45` `ArchiveKind`）：`File`（受管文件，本体在 `{项目}/resources/`）/ `AnalyticsTable`（`analytics.duckdb` 持久表）/ `TableRef`（远端引用，兼容读，不进筛选菜单）。
- 资源绑定里带来源：`ArchiveBinding { source_connection_id, source_table, ... }`（同文件 `:175` 起）；文件本体走 `source_path`。
- 分组字典项**不带计数**：`GroupOption { id, name }`（`crates/analytics_resource/src/view_model/row.rs:63`）；对照标签字典 `TagOption`（同文件 `:73`）**带 `count`**（存活存档中用到该标签的条数）。→ 分组栏要显示「N 条」得新增数据。

### 2.6 标签的归属粒度

- `tags_by_resource`（`crates/analytics_resource/src/store/tag.rs:303`）返回 `resource_id → 标签集合`：**标签挂在最小单元（每个表/文件）上，可 0..n 个，可重叠**。
- 分组是**单值**（`folders_by_resource`），**互斥**。

## 3. 讨论中形成的设计取向（建议，待确认）

### 3.1 分组栏：工具栏那个按钮改成「左栏开关」

- 点工具栏「分组」→ 展开/收起面板左侧的分组栏（**建议**宽 168、行高 22，与导航分组头同档），不再弹下拉菜单。筛选与管理从此只有这一个入口，不必两处勾选同步。
- 栏结构：`全部` / `未分组` → 各分组行 → 底部 `+ 新建分组`。
- 行内：折叠箭头 + 分组名 + 计数（如「12 条」）+ 勾选框。
  - **名字点击 = 折叠/展开子树；勾选框 = 控制可见性（保留多选并集语义，空选 = 全部）**。
- 管理不放在栏里排按钮（168 宽排不下），走**行右键菜单**，项与导航分组头对齐：重命名 / 编辑分组… / 上移 / 下移 / 删除（删除时提供「成员转到哪个分组」，等价于合并）。
- 已有可复用资产：导航侧「管理分组」弹窗（`crates/workbench/src/components/group_manager_dialog.rs:94` `open_group_manager_dialog`）与分组表单（`group_form_dialog.rs`，字段 = 名称（必填）+ 描述（可选），`GroupFormSeed` 在 `workbench_shell::model`）。

### 3.2 分组下的「目录结构」：先做虚拟二级，不开放手工建子目录

- 二级不新建表、不加拖拽规则，直接用资源自带的层级：
  - 表 → 二级 = **来源连接**（`source_connection_id`），行 = `source_table`；
  - 文件 → 二级 = **所在目录**（`source_path` 的目录部分）；
  - 引用（`TableRef`）不单独建层，并入现有档位展示。
- 观感与草稿箱同构（草稿箱的树本来就是项目目录结构），但**不可手工增删目录**。
- 若将来确要手工嵌套，成本清单（**不建议本轮做**）：`parent_folder_id` 启用 + 同名唯一性改按父分组 + 拖拽环路检查（子分组拖进自己子孙）+ 删父分组时「连子带成员删 / 子分组上提」二选一 + 归档默认落点 + `filter/snapshot/collapsed_groups` 里所有按分组 ID 的查询改成树遍历。

### 3.3 标签 vs 分组：不重复，维度不同

| | 分组 | 标签 |
|---|---|---|
| 是什么 | **位置**（放在哪一格） | **属性**（是什么 / 归谁） |
| 基数 | 一个资源 ∈ 0 或 1 个分组（互斥） | 一个资源 ∈ 0..n 个标签（可重叠） |
| 用途 | 组织树、归档落点 | 跨分组检索（重要 / 待复核 / 客户A） |
| 影响面 | 只影响该资源的所在位置 | 筛选项、标签计数 |

结论（建议）：**标签只针对最小单元（表/文件），不给分组挂标签。** 给分组挂标签会立刻要回答「新成员是否继承分组标签 / 改分组标签是否改成员 / 成员自身标签是否保留」，等于引入两套继承与覆盖规则。

界面落法：**分组在左栏（决定树），标签在工具栏 chips（决定筛）**，互不嵌入。

## 4. 待决问题（做细化分析时优先回答）

1. **分组栏默认展开还是收起？** 资产库常驻右侧 dock，窄面板下左栏会压缩列表；建议默认收起、点按钮展开。
2. **分组行「点名字即折叠」是否符合直觉？** 当前选择是折叠，筛选交给勾选框；需要考虑「点名字即筛选（单选）」的替代方案。
3. **文件二级显示目录名还是相对项目根的相对路径？** 建议显示相对路径（如 `data/mock`），否则两个同名 `mock` 目录会合并成一行。
4. **分组行计数要不要显示、算什么？** 现在 `GroupOption` 不带计数，需新增（可用 `folders_by_resource` 现算）；是否也显示「表 N / 文件 M」分离计数。
5. **「全部」与「未分组」在栏里的位置与语义**：全部 = 清空勾选（看全部）；未分组 = 伪分组，不落表。
6. **虚拟二级与多选并集筛选如何共存**：勾选两个分组时，二级是分别成树还是合并？建议分别成树（各自一棵）。
7. **折叠状态持久化范围**：现有 `resources.collapsed_groups` 记的是分组级折叠；二级节点折叠要另存（key 形如 `group:{id}/source:{conn}`），需定 key 编码与版本兼容。
8. **分组栏与列表分组头是否重复**：左栏已有分组树时，列表内是否还需要分组头（`archive-group-{key}` 那套）；建议保留列表分组头（它承载拖拽落点与默认分组标记），左栏只做导航与筛选。
9. **`color` / `icon` 字段是否启用**：表里有、界面没用；启用则要定取色来源（禁裸 hex，走 `cx.theme().colors.*` 或分组自带色值的合法性）。
10. **合并动作是否要补**：现状 `delete_folder` 在删分组的同时清空成员映射（成员退成「未分组」），**没有「合并到…」**。若栏里要做与导航同款的「删除时选择成员去向」，需新写一条「批量改 `analytics_resource_folder` 的 `folder_id`」路径（`folders_by_resource` 那套表是每资源一行，批量更新可行，但要有事务与撤销口径）。

## 5. 落地清单（细化分析确认后按此开工）

1. 新增左栏组件（建议名 `resource_view::group_rail`）：行高 22、宽 168、可折叠；数据 = 分组字典（需扩 `GroupOption` 带 count）。
2. 工具栏 `archive-group` 按钮行为改为开关；原下拉菜单逻辑迁入栏内勾选框；`archive-tag` 保持下拉（chips 形态）。
3. 虚拟二级渲染：`list_delegate` 增加二级 key（`archive-group-{gid}-{source|dir}`），复用 `GroupHeader` 与现有折叠持久化。
4. 行右键菜单：接 `group_form_dialog`（重命名/编辑）+ 删除确认 + 上移/下移（`sort_order`）。
5. 单测：栏开关后的筛选并集不变；二级归属正确（表按 `source_connection_id`、文件按 `source_path` 目录）；折叠持久化 key 稳定。

## 6. 源码索引（再次进入时先读这些）

| 用途 | 位置 |
|---|---|
| 分组 CRUD | `crates/analytics_resource/src/store/folder.rs`（`create_folder:38`、`rename_folder:101`、`delete_folder:134`、`list_folders:193`、`add_resource_to_folder:243`、`folders_by_resource:300`） |
| 分组模型/字典 | `crates/analytics_resource/src/view_model/row.rs:63`（`GroupOption`）、`:73`（`TagOption` 带 count） |
| 资源档位/绑定 | `crates/analytics_resource/src/model/kind.rs:45`（`ArchiveKind`）、`:175`（`ArchiveBinding`） |
| 筛选语义 | `crates/analytics_resource/src/resource_view/filters.rs:28,78` |
| 工具栏按钮 | `crates/analytics_resource/src/resource_view/panel_render/chrome.rs:218`（分组）、`:266`（标签） |
| 列表分组头 | `crates/analytics_resource/src/resource_view/list_delegate/group_header.rs` |
| 现有分组管理弹窗（导航侧，可复用形态） | `crates/workbench/src/components/group_manager_dialog.rs:94` |
| 分组表单（名称+描述） | `crates/workbench/src/components/group_form_dialog.rs`；`GroupFormSeed` 在 `crates/workbench_shell/src/model.rs` |
| 导航分组存储（对照用） | `crates/store/src/persistence/connection_org_store.rs` |
| 共用树原语 | `crates/workbench_shell/src/tree.rs`（`GroupHeader`、`group_count`） |
| UI 常量 | `crates/workbench_shell/src/ui.rs`（`ROW_HEIGHT_COMPACT=1.375`=22、`NAV_GROUP_BAR_WIDTH`、`GROUP_COUNT_GAP` 等） |

## 7. 与本轮其它已落地改动的边界

- 分组计数已统一为纯文本 `tree::group_count`（无胶囊底/圆角），`tree::count_badge` 已删；分组名改名有两条路径（原格就地 / 表单），提交同为 `save_group_form`。
- 资产库内已有的「分组色条 2px」「列表分组头」不在本次改造范围内，本设计只新增左栏与二级，不改这两处的视觉。
