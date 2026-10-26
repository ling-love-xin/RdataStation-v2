# 数据库导航栏现状台账与遗留清单（B0–B6 收口 · 2026-10-26）

> **状态**：B0–B6 可插拔底座与破坏性动作 staged 模型已全部入库（HEAD `4ef7dea1`）；本文档替代 2026-10-24 旧 B 档待办口径，记录**当前现状**、动作与能力覆盖、耦合核对结论、与 DBeaver 态势、遗留挂账与另案清单。
> **证据口径**：本地源码以 `crates/...:行号` 引用；竞品证据见 `docs/analysis/competitor-analysis/<文件>.md:行号`（gitignore，PowerShell 读取需 `-Encoding UTF8`）；立项方案历史稿见 `database-nav-b-tier-proposal.md`（B0 档案，保留不回写）。
> **边界**：本文档只覆盖导航模块（`crates/database` 导航链路 + `crates/workbench` 导航宿主接线）；连接新建/编辑对话框属 M3，SQL 编辑器 / Mock / 洞察 / DuckDB 分析资产本体属各自模块。
> **未决口径**：rename 是否纳入 staged（决策 R1-a1，维持直接提交、不入 staged、本批不补确认框；评审可改判，回退成本见 `b6-design.md` §6.2，落 `.rds/tmp/` 不入库）。

---

## 1. 收口状态总览

### 1.1 B0–B6 批次与提交对照

| 批次 | commit | 主题 | 范围 |
| --- | --- | --- | --- |
| B0 | `ef53489c` | docs(数据库导航): 归档 B 档方案状态、遗留问题全量清单与导航索引 | 基线文档入库，未动代码 |
| B1 | `86ac1025` | refactor(数据库导航): L1 族档案注册表，收敛七处对象身份映射 | folder / 对象类型 / 图标 / 属性类别等身份 match 全部收敛为 L1 查表 |
| B2 | `aaab3763` | feat(数据库导航): L2 操作能力矩阵与对象类型门控替换连接布尔粗判定 | 族 × 类型 × 操作 `OpSet` 静态能力集，门控三态 |
| B3 | `ab7a1d8f` | feat(数据库导航): L3 方言 SPI 注册表，迁移 DDL 与标识符生成族分支 | rename / 引号 / 行估计 / 大小 SQL 下沉四族 provider + fallback |
| B4 | `eb83ebc9` | feat(数据库导航): L4 动作注册表驱动菜单装配，命令与快捷键同源声明 | 菜单遍历注册表 `entries_for`，视图层零类型 match |
| B5-D7 | `0be87a2e` | feat(数据库导航): D7 树内三态空态占位行 | 筛选 / 搜索 / 双态不可选不进漫游，More 文案 = 可见 / 共计数 |
| B5-D8 | `43b51ddd` | perf(数据库导航): D8 六类结果 100ms 合并窗口单次重绘 | `apply` 拆 core 保留全部副作用 |
| B5-D9 | `ac82d307` | fix(数据库导航): D9 刷新 diff-merge 保留展开态并补清残留簿记 | 刷新路径 diff-merge，展开态跨刷新保留 |
| B5-D10 | `124fe08c` | feat(数据库导航): D10 Quick Open 新增 Goto 跳转档（Ctrl+Shift+O） | 仅元数据过滤列、1000 上限字典序，Enter 回树定位、Alt+Enter 次级属性面板 |
| B5-D11 | `ff70a82a` | feat(nav): 搜索支持前缀/包含匹配模式并接入注释命中维度 | 匹配模式开关 + 注释候选并入去重 + why 三态 |
| B5-D12 | `c84ca5f8` | feat(nav): 接入表大小/主机名/注释/子计数行内统计元素并按族档案门控 | `data_length` 字段 + L3 `table_size_sql` + 行内元素档案开关 |
| B5-D13 | `75ef6817` | feat(nav): 对象名过滤器暴露正则开关并接通 glob/正则双匹配分支 | per-conn + 全局 mask，exclude 优先于 include |
| B6-1 | `ebb82c18` | feat(数据库导航): 导航破坏性 DDL：L3 方言 SPI 补 drop/truncate 四族矩阵 | trait 补 `drop_sql` / `truncate_table_sql`，四族 + fallback |
| B6-2 | `30b88a49` | 主题 token：staged 删除态语义角色明暗两套三处同步 | `staged.drop` 明暗两套 + TokenMap 访问器 + 回退 `theme.colors.danger` |
| B6-3 | `b4cf9161` | staged 状态机与破坏性动作注册表登记（视图未接线） | `StagedOp` / `EnqueueOutcome` / 去重归并纯函数；注册表追加两条 Manage 动作 |
| B6-4 | `647d8f5a` | feat(数据库导航): staged 破坏性动作菜单接线与单项确认对话框 | Delete 入队、Ctrl+Z 撤销全部、行级删除线、单项确认、菜单两臂 |
| B6-5 | `4ef7dea1` | feat(数据库导航): staged 批量提交执行与汇总确认对话框 | Ctrl+S / 面板头「提交 N」、按连接分组、逐条独立执行、部分失败保留、汇总 notice |

### 1.2 本模块当前覆盖边界

- **做**：数据源组织视图（分组 / 标签 / 归属域 facet / 过滤 / 搜索 / Goto 跳转）、对象树懒加载与两级缓存、连接 / 断开 / 刷新、属性面板入口、查看数据 / 生成 SQL / Mock / 统计 / 结构洞察入口、破坏性动作（删除表 / 视图 / 序列、清空表）staged 缓冲与批量提交。
- **不做**：连接新建 / 编辑对话框本体；SQL 编辑器 / Mock / 洞察 / 分析资产本体；新建表 / 视图 / 序列 / 触发器向导；行值搜索；Set active catalog/schema；多 Transfer 拖拽与文件拖入（边界排除项见 §5.4、§6.5）。

---

## 2. 四层可插拔底座现状

### 2.1 分层与依赖方向

```text
┌─────────────────────────────────────────────────────────────┐
│ 视图层 rows.rs / chrome.rs / primitives.rs / nav_view.rs      │  只渲染 + 挂事件，不 match kind、不写方言
├─────────────────────────────────────────────────────────────┤
│ L4 动作注册表  nav/actions/registry.rs                        │
│   动作描述符：适用类型 / 门控操作 / 分组顺序 / 快捷键 / 靶类型  │
├─────────────────────────────────────────────────────────────┤
│ L2 能力矩阵  nav/capability.rs                                 │
│   族 × 对象类型 × 操作静态位集；连接态作为公共前置叠加          │
├─────────────────────────────────────────────────────────────┤
│ L3 方言 SPI  nav/dialect/（postgresql / mysql / sqlite / duckdb）│
│   引号 / 限定名 / rename / drop / truncate / 行估计 / 大小 SQL  │
├─────────────────────────────────────────────────────────────┤
│ L1 族档案  nav/family_profile.rs（静态编译期注册）              │
│   层级形状 / 文件夹归属 / 图标 / 搜索范围 / 行内统计元素开关   │
├─────────────────────────────────────────────────────────────┤
│ 引擎层  DriverRegistry / Database / MetadataBrowser（既有复用）│
└─────────────────────────────────────────────────────────────┘
```

依赖方向硬约束：视图 → L4 → L2/L3 → L1 → engine，**不得反向**：L1 不知道动作、L3 不知道菜单、L4 不写方言 SQL；nav 底座层不反向依赖 nav_view 视图层。注册表查找键 = 驱动族 id（`dialect_of` / `family_of` 统一归一别名：`postgres|pg|postgres_native → postgresql`）。

### 2.2 L1 族档案（`crates/database/src/nav/family_profile.rs`）

- 结构：`DbFamilyProfile`（族 id / 别名 / 层级形态 / 类型全集 / 文件夹顺序）、`TypeProfile`、`FolderProfile`、`SearchScopeProfile`、`InlineStatsProfile`；`FamilyRegistry` 静态注册表 + 未知族回退通用档案。
- 查表收敛点（替代旧 match 分支，对应耦合清单 C7/C8/C9/C11/C12/C13）：
  - `folder_to_shape(folder) -> (NodeShape, bool)`：文件夹 → 展示形状 / 子节点有无；
  - `folder_to_property_kind(folder)` / `object_kind_to_property_kind(kind)` / `property_kind_to_folder(kind)` / `object_kind_to_folder(kind)` / `folder_to_object_kind(folder)`：身份类别互查；
  - `search_hit_node_kind(kind)`：搜索命中 → 节点变体；
  - `object_kind_icon(kind) -> Option<&'static str>`：对象类型 → 图标 key；
  - `family_search_scope(type_id)` / `family_inline_profile(type_id)`：搜索维度开关、行内统计元素开关。
- 新增数据库类型时，L1 只补一条族档案条目 + 类型全集声明；六处身份映射不再需要改视图 / 服务层 match。

### 2.3 L2 能力矩阵（`crates/database/src/nav/capability.rs`）

- `NavOp` 动作枚举 21 变体（`:23-49`）：本地动作（ShowProperties / CopyName / CopyQualified / OpenSql / InsightPanel）、回源读取动作（ViewData / GenDml / RefreshProps / MockData / Stats / InsightSchema）、破坏性（Rename / Drop / Truncate）、预留位（CreateObject / SetActive / SearchChildren / Description / Columns / ObjectSource / RoutineSource，暂不消费）。
- `OpSet(u128)` 位集（`:72-97`）：`add` / `has` / `contains` / `is_empty`，纯函数可比较。
- `family_ops(family_id, kind) -> OpSet`（`:151`）：族档案未声明类型返回空集（安全网）；本地动作对档案声明类型开放；回源动作按类型挂位。类型适用性矩阵见 `:140-148` 注释表。
- 门控三态（`nav/actions/registry.rs:160-166` `gate`）：`op = None` 连接前置；本地动作断连可用；回源动作 = 已连接 && `ops.has(op)`，替代旧「只看 connected」布尔粗门控。
- 身份映射纯函数：`nav_node_object_kind(kind)`（`:103`）、`property_kind_to_object_kind(kind)`（`:122`）。

### 2.4 L3 方言 SPI（`crates/database/src/nav/dialect/`）

- `trait DialectSpi`（`dialect/mod.rs:37`）方法：`family`、`quote_ident`、`qualified_sql`、`rename_sql`、`drop_sql`、`truncate_table_sql`、`row_estimate_sql`、`table_size_sql`（后两者未覆写默认 `None`）。
- provider：`postgresql.rs` / `mysql.rs` / `sqlite.rs` / `duckdb.rs` + `FallbackDialect`（不支持项恒 `Err`，安全默认）；`dialect_of(type_id) -> &'static dyn DialectSpi`（`:210`）注册表分派，别名归一 + 未知回退。
- `crates/database/src/sql_gen.rs` 保持薄包装：`rename_object_sql`（`:382`）、`drop_object_sql`（`:398`）、`truncate_table_sql`（`:407`）、`row_estimate_sql`（`:423`）均委托方言；`table_data.rs:39-40` 无方言上下文兼容入口走 fallback `dialect_of("")`，列 / 排序标识符（`:69/:88/:97/:103`）全部 `dialect.quote_ident(...)`。
- 新增族：实现 trait 方法 + 注册表追加；`sql_gen` / `table_data` 不改族分支。

### 2.5 L4 动作注册表（`crates/database/src/nav/actions/registry.rs`）

- 静态 `ENTRIES` 13 条描述符（`:169-332`）；`registry() -> &'static NavActionRegistry`（`:335`，`OnceLock`）。
- `entries_for(kind, connected, ops)`（`:116-139`）：`applies_to` 类型白名单过滤 → `gate` 门控 → 分组序 `ActionGroup` → 组内 `order` 稳定排序；视图层 `rows.rs:2554` 只遍历产出序列装配菜单，不再写类型 `if`。
- 描述符字段：`id` / `slug` / `label` / `group` / `order` / `applies` / `op` / `target` / `shortcut` / `destructive`。
- 靶可用性（property / qualified / 路径靶是否 `Some`）属视图运行时判定，在 `rows.rs` 闭包 match 臂内保留 `if let Some` 口径（`:2562-2563` 注释）；注册表只声明类型白名单，不表达运行时靶条件。

### 2.6 视图层纪律

- `render` 纯读：加载 / 属性 / 预热 / 预取全部走 `nav_jobs` 后台工作线程 + 结果队列回填；`render` 路径不做 I/O、不写 `Shared`。
- 组件选型禁手搓：菜单 `PopupMenu`、按钮 `Button`、对话框 `AlertDialog` / `Dialog`、开关 `switch::Switch` 全部用 gpui-kit 标准组件；颜色一律主题 token / 产品语义 token，零裸 hex；结构尺寸进 `workbench_shell/src/ui.rs`，裸 `px(` 被 `ui_contract` 拦下。
- 回调不重入 `update` 当前实体；两阶段开窗（Context 内取快照 → App 作用域 `open_alert_dialog` → OK 回调回写）口径见 `b6-design.md` §9.4 偏差 3。

---

## 3. 动作与能力覆盖矩阵

### 3.1 L4 注册表 13 条动作（B6 收口口径）

| 分组 | 动作 id | 菜单标签 | 适用类型 | 操作位 | 靶类型 | 快捷键 | 破坏性 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Primary | ShowProperties | 查看属性 | 全部可寻址类型 | ShowProperties | PropertyRef | NavOpenProperties（F4 / Enter） | 否 |
| Primary | ViewData | 查看数据（LIMIT 200） | Table / View | ViewData | TableOnly | — | 否 |
| Primary | OpenSql | 在 SQL 编辑器中打开 | 全部可寻址类型 | OpenSql | None | — | 否 |
| Primary | InsightPanel | 查看洞察 | 全部可寻址类型 | InsightPanel | None | — | 否 |
| Generate | GenDml | 生成 SQL | Table | GenDml | TableOnly | — | 否 |
| Generate | MockData | 生成 Mock 数据 | Table / View | MockData | TableOnly | — | 否 |
| Generate | Stats | 查看统计 | Table / View | Stats | TableOnly | — | 否 |
| Generate | InsightSchema | 结构洞察 | Table / View / Schema | InsightSchema | SchemaTarget | — | 否 |
| Object | CopyName | 复制名称 | 全部可寻址类型 | CopyName | None | — | 否 |
| Object | CopyQualified | 复制限定名 | Table / View / Routine / Sequence / Trigger | CopyQualified | QualifiedName | — | 否 |
| Object | RefreshProps | 刷新元数据 | Table / View / Schema | `op = None`（连接前置） | None | — | 否 |
| Manage | DeleteObject | 删除… | Table / View / Sequence | Drop | PropertyRef | Delete（@ database-nav） | **是** |
| Manage | EmptyTable | 清空… | Table | Truncate | PropertyRef | — | **是** |

> `destructive` 位 B6 仅登记与测试断言；菜单渲染暂不读取该位（D3 口径），后续可据此统一 Danger 视觉与确认强度。

### 3.2 键盘绑定（`crates/app/src/main.rs:286-310`，`database-nav` context）

| 键位 | 动作 | 说明 |
| --- | --- | --- |
| `up` / `down` | NavUp / NavDown | 上下移动选中行 |
| `right` / `left` | NavExpand / NavCollapse | 展开 / 收起 |
| `f4` / `enter` | NavOpenProperties | 查看属性 |
| `f2` | NavRename | 行内重命名（连接行走编辑对话框；对象行走 L3 rename DDL，直接提交） |
| `home` / `end` | NavHome / NavEnd | 跳到首行 / 末行 |
| `pageup` / `pagedown` | NavPageUp / NavPageDown | 翻页（固定 10 行步长） |
| `escape` | NavClearSearch | 清除搜索框 |
| `alt-up` / `alt-down` | NavReorderUp / NavReorderDown | 本地归组 / 排序移动 |
| `delete` | NavDeleteToStaged | 选中对象加入待提交删除清单（单项确认对话框） |
| `ctrl-z` | NavRevertAllStaged | 撤销全部待提交变更（单层撤销，回到未入队状态） |
| `ctrl-s` | NavCommitStaged | 提交待提交清单（先弹汇总确认，逐条独立执行、部分失败保留） |

> 键位 context 隔离：`ctrl-s` 在 editor 更深 context 先拿（编辑器保存不被抢占）；`delete` 在 scratchpad / analytics-resource 更深 context；`ctrl-z` 在结果网格内部。深 context 先拿，互不抢占（`main.rs:306-307` 注释口径）。

### 3.3 staged 破坏性动作模型（B6 落地口径）

- **入队范围**：仅 Drop（删除表 / 视图 / 序列）与 Truncate（清空表）；Mock / Stats / ViewData / GenDml / InsightSchema 等回源读操作与本地动作不进 staged 族（D5）。
- **状态机**（`nav_view.rs`）：`StagedOp`（`:277`，key / conn_id / driver / catalog / schema / parent / object_name / object_kind / op / `error: Option<String>`）；`EnqueueOutcome`（`:295`，Added / Replaced / Ignored）；`staged_ops: Vec<StagedOp>`（`:481`）。访问器：`staged_lookup`（`:486`）、`staged_count`（`:491`）、`enqueue_staged`（`:501`）、`dismiss_staged`（`:524`）、`clear_staged`（`:529`）、`commit_plan` 快照（`:534`）、`mark_staged_failed`（`:540`）、`group_by_conn`（`:548`）。
- **去重归并**：去重键 `(conn_id, schema, object_name, op)`；已入队 Drop 再入 Truncate → Ignored；已入队 Truncate 再入 Drop → Replaced 为 Drop；同操作重复入队 → Ignored 幂等。
- **双层确认**：右键菜单 / Delete 键 → 单项确认对话框（命令式标题 + 限定名 + 不可恢复警告一行，OK「确认加入待提交」Danger）→ 入队；Ctrl+S / 面板头「提交 N」→ 汇总确认对话框（标题「提交 N 项变更」+ 全量滚动清单：动作标签 + 限定名；OK「提交 N 项变更」Danger）。已入队对象的破坏性菜单切换为「从待提交移除」，单项撤销不弹确认。
- **执行模型**：`commit_plan` 快照 → 按 conn_id 分组（首次出现顺序、组内保序）→ 后台 `cx.spawn` 串行逐条 `execute_query` → 成功移除、失败保留附 `error`；整批跑完一次批量回写，涉及连接去重逐个 `refresh_connection_after_filter` → 汇总 notice（成功 X / 失败 Y，失败逐条限定名 + 错误，超 5 条折叠）。**不包整批事务**（MySQL InnoDB DDL 隐式提交不可回滚，四驱动事务口径不成立）；不采用首条失败即中止；断连不清空集合；会话内不落库。
- **视觉**：命中待提交集合的行标题 `.line_through()` + `staged.drop` 弱 danger 色 + 通用「待删除」徽标段；面板头「提交 N」ghost 按钮仅在 `staged_count > 0` 时渲染，空集不占槽位。明暗对比度已本地复算 ≥4.5:1（`b6-design.md` §9.2）。
- **rename 口径（D1 / R1-a1，暂定）**：维持 F2 行内 Enter 直接提交，不入 staged、不注册 L4 菜单、不挂 `family_ops`；`NavOp::Rename` 预留位不动；本批不补确认框与预览 SQL，挂 v2 偏好体系。评审可改判 R1-a2（直接提交 + 非 Danger 单项确认，落点 `editors.rs:commit_object_rename` 前置）或 R1-b（纳入 staged 统一缓冲，需另起子批评估）。

### 3.4 B5 已落地能力现状

| 维度 | 现状 | 落点 |
| --- | --- | --- |
| 占位节点三态 | 加载中 / 过滤空 / 搜索空 / 双态不可选占位行；不可选不进键盘漫游；More 文案 = 可见 / 共计数 | `nav_view/primitives.rs` 占位决策纯函数 + `rows.rs` 渲染 |
| 事件 100ms 合并 | 六类后台结果经 100ms 窗口合并单次重绘；`apply` 拆 core 保留全部副作用 | `nav_jobs.rs` 泵通道 + `actions.rs` |
| 刷新保留展开态 | diff-merge 复用已存在节点，展开态 / 选中态跨刷新保留；刷新后清残留簿记 | `navigator_service.rs` 刷新路径 + `nav_view.rs` 状态 |
| Goto 跳转档 | Quick Open `>` 命令档之外的元数据跳转档；仅元数据（跳过列 / 方法）；1000 上限字典序；Enter 回树定位、Alt+Enter 次级属性面板 | `workbench/quick_open` + 导航命中桥接 |
| 搜索维度 | 前缀 / 包含匹配模式开关；注释候选并入去重；命中 why 三态（名称 / 注释 / 定义预留）；定义语料链路预留未采集 | `nav_jobs.rs` + L1 `SearchScopeProfile` |
| 行内统计元素 | 行估计（PG / MySQL 目录 SQL；SQLite / DuckDB 返回 None）、表大小、主机名、注释、子计数；按 `InlineStatsProfile` 档案门控 | L3 `table_size_sql` + `nav_jobs.rs` 双查询 + `primitives.rs` 格式化 |
| 对象名过滤器 | per-conn + 全局两层 `ObjectMaskRule { include, exclude, use_regex }`；glob / 正则双分支；exclude 优先于 include；include 空 = 全量候选；只过滤已加载对象子集，`child_total` 保持原始计数 | `settings/model.rs` + `primitives.rs` 纯函数 + `navigator_service.rs` / `nav_jobs.rs` 管线插入点 |

---

## 4. 耦合清单 C1–C18 收口核对（B7 逐项对照当前源码实证）

> 原始耦合表见 `.rds/tmp/nav-pluggable-foundation-proposal.md` §2.2（不入库）；本表为 B7 收口结论。

| # | 原耦合点 | 现状归属层 | 结论 | 实证锚点 |
| --- | --- | --- | --- | --- |
| C1 | `engine/src/driver/registry/descriptors.rs` 各工厂描述符 | 引擎层 | **保留**：驱动包内职责，不纳入导航底座改造 | `engine/src/driver/registry/descriptors.rs` |
| C2 | `sql_gen.rs` rename driver 四族匹配 | L3 | **已收敛**：`rename_object_sql` 经 `dialect_of(driver)` 委托 | `sql_gen.rs:382` |
| C3 | 引号双写 + schema 前缀 if 分支 | L3 | **已收敛**：标识符引号走 dialect；展示限定名 `qualified_name` 留展示层；无方言兼容入口走 fallback `dialect_of("")` | `sql_gen.rs:42`；`table_data.rs:39-40` |
| C4 | NavFolder 五分支 × driver 嵌套 DDL | L3 | **已收敛**：`drop_object_sql` → `dialect.drop_sql(...)`；`truncate_table_sql` → `dialect.truncate_table_sql(...)` | `sql_gen.rs:398`、`:407` |
| C5 | row_estimate 二元 driver 匹配 | L3 | **已收敛**：`row_estimate_sql` → `dialect.row_estimate_sql(schema)` | `sql_gen.rs:423` |
| C6 | `table_data.rs` 私有硬编码双引号 | L3 | **已收敛**：列 / 排序标识符全部 `dialect.quote_ident(...)`；旧硬编码双引号仅存无方言兼容入口 fallback | `table_data.rs:69/:88/:97/:103` |
| C7 | `navigator_service.rs` folder → NavNodeKind 五分支 | L1 | **已收敛**：`folder_to_shape(folder)` 查表；match 只把 L1 声明的 `NodeShape` 映射到固定 `NavNodeKind` 展示变体（五形状一一对应，非 folder 五分支硬编码） | `navigator_service.rs:719-728` |
| C8 | folder → PropertyKind 五分支 | L1 | **已收敛**：`folder_to_property_kind(folder)` 查表 | `navigator_service.rs:743` |
| C9 | NodeKind → 图标十一分支 | L1 | **已收敛**：`object_kind.and_then(family_profile::object_kind_icon)` 查表 | `nav_view/primitives.rs:451` |
| C10 | NodeKind → ObjectRef 靶二分支 | L4 | **部分收敛**：`nav_data_target` 仍只 Table / View 构造 ObjectRef、其余 None——数据动作语义上仅对表 / 视图有意义，非开放耦合；L4 `TargetKind` 已声明动作靶类型，新增类型时由注册表 `applies` 门控 | `nav_view/primitives.rs:546-558` |
| C11 | ObjectKind → NodeKind 搜索命中三分支 | L1 | **已收敛**：`family_profile::search_hit_node_kind(r.kind)` 查表 | `nav_view/primitives.rs:711` |
| C12 | PropertyKind → NavFolder 五分支 | L1 | **已收敛**：`family_profile::property_kind_to_folder(prop.kind)` 查表 | `nav_view/chrome.rs:1200` |
| C13 | ObjectKind → PropertyKind 八分支 | L1 | **已收敛**：`family_profile::object_kind_to_property_kind(object.kind)` 查表 | `model.rs:345` |
| C14 | NavFolder → `supports_*` 四分支 | L2 | **已收敛**：`supports_object_folder` 经 `db.as_metadata_browser()` 桥接，内部 match folder 映射到 browser 能力位（Tables 恒 true；Views / Routines / Sequences / Triggers 查 browser 位） | `metadata_service.rs:99-115` |
| C15 | 菜单按 `data_like` / target 条件 push | L4 | **已收敛**：`action_registry().entries_for(kind, obj_connected, &obj_ops)` 遍历注册表装配菜单；视图闭包内 match 只把固定 13 个动作 id 映射到 on_click 处理器，靶可用性 `if let Some` 保留运行时判定 | `nav_view/rows.rs:2554`；注释 `:2562-2563` |
| C16 | 门控只看 `connected` 布尔 | L2 + L4 | **已收敛**：`gate(op, connected, ops)` 三态——`op = None` 连接前置；本地动作断连可用；回源动作 `connected && ops.has(op)` | `nav/actions/registry.rs:160-166` |
| C17 | `metadata_service.rs` 桥接 `as_metadata_browser` | L2 | **复用既有通道**：内省能力位走同一桥接入口 | `metadata_service.rs:50/:77/:90/:105/:125/:158/:172/:203/:217/:250/:269` |
| C18 | `engine/src/driver/traits.rs` MetadataBrowser 内省能力位 | 引擎层 | **保留**：驱动包内覆写默认位合理，不纳入导航底座 | `engine/src/driver/traits.rs` |

**核对结论**：C2–C9、C11–C16 共 14 项已收敛到对应底座层；C10 部分收敛（数据靶类型语义固定，开放扩展留待新增类型时由 L4 声明）；C1 / C17 / C18 属引擎层合理保留。新增数据库类型时，导航 / 应用层不再需要补菜单、门控、图标、身份映射、DDL 生成五套分支。

---

## 5. 与 DBeaver 态势对照（B0–B6 后复核）

> 旧 27 维度判定与证据见 `.rds/tmp/nav-pluggable-foundation-proposal.md` §3（不入库）；本节只列 B0–B6 收口后的态势分组。DBeaver 证据行号见 `docs/analysis/competitor-analysis/nav-analysis-dbeaver.md`（gitignore）。

### 5.1 我方领先（保持优势）

- **本地索引 / FTS 搜索**：DBeaver 无本地索引、全靠驱动 SQL 兜底，大 schema 是短板；我方 L1 内存 + L2 每连接 SQLite + 索引 scoped 并行搜索。
- **分页粒度**：200 条 / 页 + `child_total` 独立于过滤后子集，比 DBeaver 整段 LazyExpander 更细。
- **并发调度**：per-conn 信号量 = 1 + 全局软帽 K=3 + 独立 Prefetch 信号量 + 用户动作优先级插队；同连接串行、跨连接并行（B-2 真机 V1–V5 已过闸，K=3 定稿）。
- **属性源可插拔**：`PropertyProvider` 按族 × 类型分派，新增类型只加子模块，先于导航层完成同构解耦。
- **两级元数据缓存**：L1 内存 + L2 每连接 SQLite，cache-aside；L3 实时内省不缓存。

### 5.2 持平

- **分组 / 本地组织模型**：我方分组 + 标签是独立组织模型，语义不同于 DBeaver 收藏标记，不照搬。
- **模型层与状态分离**：基本结构与 DBeaver `DBNModel` 同级；模型层独立抽象挂后续评估。
- **行内重命名交互强度**：DBeaver `INLINE_RENAME_ENABLED=false` 默认关闭、走模态输入框；我方对象重命名走确认 + DDL 执行，强度等价（其自身也不做行内编辑）。

### 5.3 本轮已补齐（B1–B6 落地，旧判定 ❌ / ⛔ → 已收敛）

| 维度 | 落地批次 |
| --- | --- |
| 菜单动作贡献机制（注册表声明适用类型 / 门控 / 分组 / 顺序 / 快捷键） | B4（C15） |
| 族 × 类型 × 操作能力门控（替代连接布尔粗判定） | B2（C16） |
| 方言 / DDL 生成 SPI（族 provider 自带语句，内核不写死族分支） | B3 + B6-1（C2–C6） |
| 族档案身份映射（层级形状 / 文件夹归属 / 图标 / 搜索命中 / 属性类别互查） | B1（C7–C9、C11–C13） |
| 搜索注释维度 + 匹配模式（前缀 / 包含） | B5-D11 |
| 快速跳转对话框（1000 上限、跳过列 / 方法、选中定位回树） | B5-D10 |
| 占位节点体系（加载 / 过滤空 / 搜索空 / 不可选） | B5-D7 |
| 事件 100ms 批量合并窗口 | B5-D8 |
| 刷新 diff-merge 保留展开态 | B5-D9 |
| 统计后台化与行内元素（大小 / 主机名 / 注释 / 子计数，按族档案门控） | B5-D12 |
| 过滤器正则开关 + glob / 正则双匹配 | B5-D13 |
| 破坏性动作 staged 缓冲 + 双层确认 + 批量提交 + 部分失败保留 | B6 |

### 5.4 仍落后 / 缺口项（挂账，未落地）

| 维度 | 现状缺口 | 归属 |
| --- | --- | --- |
| 层级元模型可扩展 | 文件夹类别仍为 `NavFolder` 五内置值；族自定义分组、父档案继承 + 路径注入 / 替换未实现 | 后续底座扩展（L1 档案开放字符串 key 决策未定） |
| 对象类型全集 | Index / PrimaryKey / ForeignKey 只进表详情，不进树一级节点；Routine 重载签名未进身份结构 | 后续底座扩展 |
| 新建对象贡献 | 菜单无 create 项；无新建表 / 视图 / 序列 / 触发器向导；`CreateObject` 仅预留位 | 边界待评估（向导本体是否纳入导航模块） |
| 行内重命名确认框 / 预览 SQL | rename 现状直接提交，无确认对话框、无脚本预览（DBeaver `showScript` 可选预览） | v2 偏好体系（§6.1） |
| 搜索 SPI 驱动侧 | 无驱动侧搜索能力声明；定义采集链路未接入 | D11 遗留（§6.4） |
| 结果页回树定位契约 | 结果页 / 洞察面板「Show In > Navigator」契约未抽象；`ObjectRef` 身份已统一可寻址 | 后续底座动作规范 |
| 上下文限定搜索 / 类型复选面板 / 选中自动打开编辑器 / Goto 本地正则开关 UI | D10 遗留子项 | §6.4 |
| 过滤器类型四态 / 仅已连接开关 / 掩码历史 | D13 遗留子项 | §6.4 |
| 菜单 Danger 文字色 | vendor `menu_item.rs` 无 variant / color API；破坏性菜单项视觉未区分 | 上游组件受限（§6.2） |
| 多级撤销栈 / 整体丢弃快捷键 / 预览 SQL / 断连未提交量提示 | staged 单层撤销；无 `lost_work` 提示 | v2 产品功能（§6.1） |
| 行值搜索 / Set active catalog/schema / 多 Transfer 拖拽与文件拖入 | 明确边界排除，L4 预留 `SetActive` 位 | §6.5 |

---

## 6. 遗留挂账清单（本模块不碰，分类挂账）

### 6.1 v2 产品功能（偏好体系 / 全量编辑 staged 落地后评估）

- rename 确认对话框 / 预览 SQL（DBeaver `showScript`；zqlz `confirm_destructive` 全局开关可参照）。
- Routine / Schema / Trigger / Column 破坏性操作：Routine drop 缺函数签名（PG `DROP FUNCTION name(args)` 重载歧义，`PropertyRef` 不带参数列表）；Schema drop 带 CASCADE 误删整库风险最高；Trigger 依附父表方言差异大；Column 属 ALTER TABLE 结构编辑另案。
- 多级撤销栈、`⇧⌘⌫` 整体丢弃快捷键、`⌥⌘P` 预览 SQL、断连 / 切换前未提交量提示（tusk `lost_work` 范式）。
- 整包事务提交、生产环境 / 只读连接标签追加环境警告（待环境标签体系落地后接入）。
- 跨连接并行提交：v1 组内 / 组间均串行；连接池 per-conn 信号量 K=3 已就位，跨连接并发提交待真机容量复核后另起子批。
- 跨重启持久化 staged 集合（当前为会话内运行时态，不落库；待面板状态持久化体系统一评估）。
- DROP 可选子句：CASCADE / RESTRICT / IF EXISTS、PG `ONLY` / `RESTART IDENTITY` / `CASCADE`、MySQL `SET FOREIGN_KEY_CHECKS = 0/1` force 前后挂语句；SQLite `DELETE FROM` 后 `VACUUM`。
- `staged.add` / `staged.rename` 色 token（引入新建 / 改名 staged 时再补三处同步）。
- 偏好体系另案：D11 搜索状态跨重启持久化；D12 用户可调行内元素开关；D13 过滤器类型四态与仅已连接开关；DBeaver maxResults、类型复选面板、掩码历史 20 条；D8 事件窗口时长用户可调。

### 6.2 上游组件受限项

- **默认焦点 Cancel**：kit 0.8.0 未暴露「默认焦点按钮 / Cancel 默认动作」API，DBeaver `setDefaultReply(CANCEL)`（`NavigatorNodesDeletionConfirmations.java:110`）不可复刻；Enter 恒触发 OK 不分焦点（`gpui-base/src/dialog.rs:89-94`；`DialogButtonProps:47-56` 无焦点位字段）。当前防护 = 双层把关 + 清单列示 + Danger 按钮 + 既有三处删除对话框同口径；待上游组件改进后接入。
- **菜单 Danger 文字色**：`vendor/gpui-component/src/menu/menu_item.rs` 无 variant / color API；v2 经 children 槽位挂自定义 Element 覆写 `text_color`。
- **headless 对话框不可点击**：单项 / 汇总对话框真点击路径不在自动化测试覆盖内；测试口径 = 断言动作枚举 / 门控纯函数 + 直接调确认回调入队函数；交付前人工走查两条路径（菜单 / Delete 键 → 单项确认 → 入队 → 汇总确认 → 提交）。

### 6.3 真机复核挂账（需真机端点，另会话执行）

- SQLite / DuckDB `DROP VIEW` 运行时支持位；既有 `ALTER VIEW` rename 真机未复核注释同批。
- PG `pg_total_relation_size` 含索引与 toast、MySQL `DATA_LENGTH` 仅数据段，跨族大小口径不做严格等价，真机实测纠正；`parse_name_value_pairs` 负 Int 溢出（与现有行估计同口径）未修正，负估计值归一 None 挂账。
- MySQL `TABLE_ROWS` / `DATA_LENGTH` 估计口径真机走查（InnoDB 估计 vs MyISAM 近似精确）。
- staged 后台真执行链路（executor 真查询 / 结果回写编排）不在 headless 断言覆盖内；真机执行留待另会话探针复核，沿用 B-2 真机挂账口径。
- B-2 真机 V1–V5 已过闸定稿 K=3；后续新增调度任务族复用既有 per-conn 信号量分层，真机复核随新任务族另会话进行。

### 6.4 B5 未闭环技术债

- **D7**：Loading 独立占位行形态、点号动画、占位行默认动作 / 右键菜单；存量测试桥 loading 残留压住占位断言，待测试桥补后台回填后更新为「出现一条 FilterEmpty 占位」断言。
- **D8**：connect 失败 notice 批量化（现状即时提示保留，不并入窗口）。
- **D9**：render 期 `ensure_nav_loaded` 写 `loading` 的既有技术债（gpui-kit-dev skill 已登记为反例）另案治理，本批不扩面。
- **D10**：从当前选中容器解包的上下文限定搜索；类型复选面板；选中后自动打开 SQL 编辑器；Goto 本地正则开关 UI（与 D13 通道共用 `CompiledMask`，浮层入口待接）；不做 DBeaver `hasMoreResults` 后台续拉。
- **D11**：定义采集链路（`NodeInfo` 增 `definition` → 四族批量采集视图 / 例程源码 → 缓存先查后增量更新 → 先定「定义是否进 L2 缓存」架构决策 → FTS 定义候选接入）单独立项；DBeaver 第三档 LIKE 通配表达式模式。
- **D12**：行估计 + 大小合并单条三列 SQL 减少一轮往返；统计进度条、classFormatMap 按类缓存 formatter、节点行内动作图标组、大小信息 L2 跨会话缓存。
- **D13**：规则级编译缓存——当前渲染期每次现编 `compile_mask` 纯函数无缓存；profile 显示高频编译开销后，再在 NavView 状态内按规则快照缓存 `CompiledMask`、规则变更事件路径刷新缓存，render 回调内不允许入队任务；坏正则条目静默丢弃、该条不生效且无 UI 提示，后续可在对话框内做逐条 pattern 编译校验与错误提示。

### 6.5 另案缺口（不属导航底座，全程不碰、不提交）

- B-2-E 旧 7 个池文件死代码清理；
- 跨 crate 千分位四份重复实现抽取合并（`result_panel/grid.rs:2846`、`editor/history.rs:257`、`analytics_resource/present.rs:71`、`mock/mock_view.rs:1057`）；
- 行值搜索、Set active catalog/schema、新建中对象跨刷新保留、多 Transfer 拖拽与文件拖入（边界排除项，随新建对象动作族一并评估）；
- analytics_resource / insight / workbench resource 面板半成品改动（另案 63 项，明细见 §8.2）。

---

## 7. B7 验证基线（2026-10-26 实测）

### 7.1 命令与计数

| 命令 | 结果 | 日志（`.rds/tmp/`） |
| --- | --- | --- |
| `cargo check --workspace --all-targets -j 1` | Finished 2m40s，EXIT=0；9 条存量 warning，B6 改动文件零新增 | `b7-check-workspace.log` |
| `cargo test -p rds-engine --lib -j 1` | **656 passed / 0 failed / 24 ignored**（24 ignored = 真机 MySQL/PG 服务依赖 + 100k 规模基线，既有口径非失败） | `b7-test-core.log` |
| `cargo test -p rds-property --lib -j 1` | **6 passed / 0 failed** | `b7-test-core.log` |
| `cargo test -p rds-workbench-shell --lib -j 1` | **10 passed / 0 failed** | `b7-test-core.log` |
| `cargo test -p rds-database --lib -j 1` | **334 passed / 0 failed / 0 ignored** | `b7-test-database.log` |
| `cargo test -p rds-workbench --lib -j 1 -- --test-threads=1` | **105 passed / 0 failed / 0 ignored**（串行线程） | `b7-test-workbench-lib.log` |
| `cargo test -p rds-workbench --test ui_contract -j 1` | **9 passed / 0 failed** | `b7-test-ui-contract.log` |

合计 **1120 passed / 0 failed / 24 ignored**（656 + 6 + 10 + 334 + 105 + 9）。

### 7.2 存量警告归属（9 条，非导航模块引入，本批不处理）

| # | 文件:行号 | 警告 | 归属 |
| --- | --- | --- | --- |
| 1 | `crates/app/src/main.rs:24` | unused imports `RedoResultEdit` / `UndoResultEdit` | editor 命令块存量（bin + test duplicate 各一行） |
| 2 | `crates/workbench/tests/history_editor_entry.rs:11` | unused import `AppContext` | 另案存量 |
| 3 | `crates/editor/src/view/signature_help.rs:185` | unused `signature_view` | editor crate 存量 |
| 4 | `crates/editor/src/view/host.rs:6777` | dead_code `apply_filter` / `clear_filter` | editor crate 存量 |
| 5 | `crates/editor/src/view/signature_help.rs:188` | `substr_sig` 未用 | editor crate 存量 |
| 6 | `crates/kernel/src/registry.rs:206` | unused `venv` | kernel 存量 |
| 7 | `crates/plugin/src/sidecar/supervisor.rs:1482` | unused_mut | plugin 存量 |
| 8 | `crates/plugin/src/sidecar/supervisor.rs:1537` | unused_mut | plugin 存量 |
| 9 | `crates/engine/src/persistence/metadata_cache.rs:4699` | unused_mut | engine 存量 |

### 7.3 ui_contract 登记核对

B6 全部复用既有登记文件（`nav_view.rs` / `primitives.rs` / `rows.rs` / `chrome.rs` / `editors.rs` / `actions.rs` / `dnd.rs`，`workbench/tests/ui_contract.rs` 尺寸清单 `:142-200` 与色值清单 `:345-404` 两处已覆盖）；B6 未新增独立视图子模块，免补登记。`tests.rs` 为测试模块，不在扫描面；本批无新增裸 `px(` / 裸 hex / 结构尺寸常量。

---

## 8. 工程口径与边界

### 8.1 测试命令口径

```powershell
# 导航模块回归（串行，本机内存基线紧张；stderr 进度流被 PowerShell 误判 NativeCommandError，
# exit 1/101 不代表编译失败，以 Finished / test result: 为准）
cargo test -p rds-database --lib -j 1
cargo test -p rds-engine --lib -j 1
cargo test -p rds-workbench --lib -j 1 -- --test-threads=1
cargo test -p rds-workbench --test ui_contract -j 1

# 全工作区编译守卫（含全部 target）
cargo check --workspace --all-targets -j 1
```

### 8.2 另案 63 项边界（全程不碰、不纳入提交）

提交前 `git status --short` 复核，逐路径精确 `git add`，禁止 `git add .`：

- `crates/analytics_resource/`：12 M src + 2 M tests + `?? src/dialogs/conflict.rs`；
- `crates/insight/`：29 M src + `tests/column_profile_e2e.rs` M + `tests/ui_contract.rs` M；
- `crates/shared/src/models.rs` M：另案 A0 水位探针临时测试模块（`watermark_probe_tests`，A1 落地后移除）；
- `crates/workbench/src/`：`components/resource_host.rs`、`panels/mod.rs`、`panels/resources.rs`、`panels/shared.rs`、`services/resource_jobs.rs` 五处 M；`crates/workbench/tests/ui_contract.rs` M（**注意：此文件属另案，不得 add**）；
- 文档：`docs/architecture/insight/` 七篇 M、`docs/architecture/layout/panels-modules.md` M、`?? docs/architecture/analytics_resource/analytics-resource-m6-finalization-plan.md`、`?? docs/architecture/insight/insight-cleanup-2026-10.md`。

### 8.3 运维备忘

- **rmeta 增量失效（E0463 / E0460）**：先确认无残留 cargo / rustc 进程持锁，`cargo clean -p rds-engine -p rds-database` 后重跑。
- **提交口径**：主题前缀 `feat/refactor/fix/docs(数据库导航):`；正文简体中文，说明动作与对标项；注释简体中文；编译产物 target 删除直接不进回收站。
- **过程产物**：验证日志、一次性脚本、探针落 `.rds/tmp/`（已 gitignore，不入库）；竞品研究落 `docs/analysis/`（整目录 gitignore，PowerShell 读取需 `-Encoding UTF8`）。
- **文档分工**：本台账记录现状与遗留；立项方案历史稿见 `database-nav-b-tier-proposal.md`；模块总入口见 `README.md`；设计决策与落码偏差见 `.rds/tmp/b6-design.md` §9（不入库）。
