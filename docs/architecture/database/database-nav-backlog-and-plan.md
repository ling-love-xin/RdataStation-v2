# 数据库导航栏遗留问题全量清单（问题点 / 解决方案 / 实施方案 / 优劣）

> 状态：2026-10-24 定稿。范围 = 数据导航栏模块一次性完善（tag 系统 / 已保存查询 / 草稿箱消费已明确排除，另会话交接，commit `8b3663e3`）。
> 证据口径：本地源码以 `crates/...:行号` 引用；竞品证据见 `docs/analysis/competitor-analysis/<文件>.md:行号`（gitignore，PowerShell 读取需 UTF8）。
> 拍板状态：B 档文档 §6 的 20 项待拍板已由用户授权全按「我方建议」执行；staged 提交模型为**用户明确要求实现**（推翻 B 档 §4.6「v1 仅预留边界」建议）。

## 0. 状态总览

| 编号 | 议题 | 状态 | 优先级 |
| --- | --- | --- | --- |
| S3-④ | 统计估计后台回填（PG/MySQL，L2 回写，徽标） | **已实现 + 198 测试全过，未提交** | — |
| B-1 | 连接过滤器 v1（per-conn + 全局 glob mask） | **实现中**（settings 模型字段待落盘） | 高 |
| S4-N6 | staged 提交模型（drop/truncate 草稿缓冲） | **待实现**（用户拍板） | 高 |
| B-2 | 按连接分片并发（per-conn 信号量 + 软帽） | **调度代码已在 HEAD**，缺真机探针 V1–V5 验证报告 | 中（真机前置） |
| 收尾 | 提交隔离 + ui_contract 复跑 + 真机缺口记录 | 待办 | 高 |

---

## 1. B-1 连接过滤器 v1

### 问题点

现状过滤体系只有**连接级 facet**（归属域 / 类型 / 驱动 / 标签，`nav_view.rs:448-507`），**没有对象名过滤**：无法按名字批量隐藏/只看某些表（如只留 `orders*`、排除 `temp_*`）。DBeaver 有 include/exclude 双向标记 + 全局过滤器（`nav-analysis-dbeaver.md:67,219,228`）；搜索命中精筛点已具备叠加条件（`nav_jobs.rs:1260,1270`）。

### 解决方案

两层 mask + 纯函数过滤，与 facet AND 叠加：

- **per-connection mask**：`Navigator.object_masks: BTreeMap<conn_id, ObjectMaskRule>`，跟随连接配置跨会话保留；
- **面板全局 mask**：`Navigator.global_mask: ObjectMaskRule`，面板偏好跨项目保留（同 facet 档，architecture.md §11 #8）；
- **匹配语义（v1 仅 glob）**：`ObjectMaskRule { include: Vec<String>, exclude: Vec<String>, use_regex: bool }`；`*` 任意多字符 / `?` 单字符，大小写不敏感；exclude 优先于 include；include 空 = 全量候选；
- **作用域边界**：只过滤已加载进 `children` 的对象子集（表/视图/例程/序列/触发器），`child_total` 保持原始计数（分页判断依据）；catalog/schema 层、列层不进 mask（列层价值低语义混杂，B-1-D 拍板）；
- **管线插入点**：树路径 = `NavigatorService` 组装 `NavPage` 后纯函数过滤；搜索命中 = 现有三档精筛后追加 per-conn + 全局 retain；
- **UI 入口**：连接右键「对象名过滤器…」（本地配置动作，断连不置灰）+ 面板头「筛选 ▾」全局入口 + 树顶过滤口径文案。

### 实施方案（分步）

1. `crates/settings/src/model.rs`：新增 `ObjectMaskRule`（derive Default/Serialize）+ `Navigator.object_masks` / `global_mask`（`#[serde(default)]`），**手动 `impl Default` 同步补字段**（`model.rs:412-425`）；`SettingsService` 加访问器；
2. `crates/database/src/nav_view/primitives.rs`：纯函数 `nav_glob_match(pattern, name) -> bool`（递归 `*`/`?`，两侧 `to_lowercase()`，`?` 不做路径分隔符排除；结构参照 `content_search/src/lib.rs:60-76` 递归骨架，不跨 crate 调）+ `nav_mask_passes(rule, name) -> bool` + **规则矩阵单测**；
3. `crates/database/src/nav_host.rs`：仿 `visible_databases`（`:144-152`）加 `object_mask(conn_id)` / `set_object_mask(...)` + 全局读写端口；
4. `crates/workbench/src/components/nav_host.rs`：两侧互转（仿 `:129-149,201-216`）；
5. `crates/database/src/navigator_service.rs`：组装 `NavPage` 后按连接规则过滤 children；
6. `crates/database/src/nav_jobs.rs`：搜索命中三档精筛（`:1252-1270`）后追加 mask retain（按 `hit.conn_id:197`）；
7. `crates/database/src/nav_view/rows.rs`：连接右键菜单加「对象名过滤器…」入口（仿「数据库可见性…」`:1702-1713` 位置，**不置灰**）+ 对话框（include/exclude 多行输入、清空/重置；v1 不暴露正则开关）；面板头「筛选 ▾」弹层全局入口 + 树顶口径文案；
8. `cargo check -p rds-engine -p rds-database -p rds-workbench` + 单测 + ui_contract 复跑。

### 优劣

| 方案 | 优 | 劣 |
| --- | --- | --- |
| glob v1（本方案） | 零新依赖（database 现有仅 regex）；纯函数易钉矩阵；`*`/`?` 覆盖绝大多数命名场景 | 无正则；无 `schema.object` 两段匹配（v2 补） |
| 正则 v1 并存 | 表达力强 | 两套语义同期上线，规则模型与 UI 复杂翻倍（B-1-A 拍板挂 v2） |
| per-conn + 全局两层 | 单连接精准 + 面板兜底；断连连接照常过滤缓存对象（B-1-E） | 两层规则语义叠加需文档说清 |
| mask 作用列层 | 可藏敏感列 | 列由表层进入、列名过滤价值低、语义混杂（B-1-D 拍板不作用） |
| 隐藏 N 条计数提示 | 用户感知过滤力度 | 大 schema 分页模型下计数语义复杂（v2 补，v1 只给树顶口径文案） |

---

## 2. S4-N6 staged 提交模型（用户拍板实现）

### 问题点

破坏性对象操作（drop/truncate）目前**无任何缓冲**：`staged` / `undo` / `redo` 在 `crates/database/src` 零命中（`nav_view/editors.rs` 四个编辑器全部直接提交：分组 `:13,288,336`、标签 `:158,251`、复制模板 `:191,350,375`、分组重命名 `:381`）。误触 drop 无撤销路径。tusk 竞品有成熟 staged 模型：行级着色（绿=新增、蓝=改名、红=删除线）、⌘S 统一提交、历史撤销（`nav-analysis-zqlz-tusk.md:124,198-200`）。

### 解决方案（边界）

- **进 staged 的**：破坏性对象操作 = **drop / truncate**（源库不可逆写操作）。行级红删除线 + 待提交集合 + ⌘S 统一提交 + 撤销；
- **保持直接提交的**：行内重命名（分组/标签/复制模板/连接）与本地组织元数据（归组/排序/共享/模板复制）——写本地 SQLite、失败可重试、无服务端不可逆语义，进草稿只增状态复杂度（对照 blanco 只读/生产环境二次确认先例，`nav-analysis-fluxdb-blanco.md:170,173,204`）；
- **行级着色 token**：新增/改名/删除三态走 rds-theme **三处同步**（`product-tokens.json` → `product_tokens.rs` TokenMap/访问器 → 消费方），禁裸 hex；
- **提交入口**：⌘S（全局命令）+ 面板内显式「提交 N 项变更」按钮；
- **撤销栈**：单层撤销（回到提交前状态）起步，多级栈深度挂 v2。

### 实施方案（分步）

1. 状态结构：`NavViewState` 新增 `staged_ops: Vec<StagedOp>`（`StagedOp { conn_id, schema, object, kind: Drop|Truncate, qualifier }`）——先确认操作入口（对象菜单「删除表…/清空表…」目前不存在，需先加菜单项 + 对话框）；
2. rds-theme 三处同步：注册 `staged.drop` / `staged.add` / `staged.rename` 三色 token（dark/light）；
3. 行渲染：命中 `staged_ops` 的对象行加删除线 + 弱色（复用分区/继承徽标槽位范式，`rows.rs:2172-2188`）；
4. 提交路径：⌘S 触发批量执行（逐条 `Database::query`，`traits.rs:442`）→ 成功清空 staged + 刷新元数据；失败项保留 + 报错明细（部分失败回滚口径）；
5. 撤销：⌘Z 清空 staged（提交前）/ 提交后走既有刷新（无事务回滚——drop/truncate 无回滚语义，撤销 = 阻止提交，非回滚已提交）；
6. 单测：staged 状态机（入队/着色/提交/撤销/部分失败）。

### 优劣

| 方案 | 优 | 劣 |
| --- | --- | --- |
| 仅 drop/truncate 进 staged（本方案） | 防误删价值最高；本地组织元数据保持直接提交不增复杂度 | 破坏性操作入口需新建；部分失败回滚口径需定义 |
| 全量对象操作进 staged | 统一交互心智（tusk 范式） | 直接提交的四编辑器全部改造，状态复杂度大涨，收益低 |
| 提交后回滚（真 undo） | 最强保护 | drop/truncate 语义不可逆，真回滚需事务 + 快照，成本远超价值 |
| 单层撤销起步 | 状态机简单 | 连续多次 staged 只能整体撤销（v2 补栈） |

---

## 3. B-2 按连接分片并发

### 问题点

调度现状 = **全局串行**：`LOAD_CONCURRENCY = 1`（`nav_jobs.rs:41-57`），所有 `LoadChildren` / `LoadProperties` / `GenerateDml` 全局排队；两连接同时展开时第二连接必须等第一连接完成。竞品全部不用串行队列（navop 会话复用池 `nav-analysis-navop.md:62`；blanco 连接池化 `nav-analysis-fluxdb-blanco.md:139-140`），但也没有一家实现优先级队列——这是我们的机会。

### 解决方案

per-conn 信号量 = 1 + 全局软帽 K=3 + 回收钩子（**代码已在 HEAD，由 B 档架构会话完成**）：

- `conn_sems: Mutex<HashMap<ConnId, Arc<Semaphore>>>`，同连接元数据通道唯一（LoadChildren / LoadProperties / GenerateDml 共用）；
- Warm/WarmCompletion/RowEstimate 维持 `try_acquire` 让位（忙即跳）；
- PrefetchColumns 独立 `PREFETCH_SEM=2`；SearchIndex 独立 8 块 scoped 线程（不变）；
- `disconnect`（`nav_host.rs:163`）与 `delete_connection`（`:195`）挂 `drop_conn_sem` 回收条目；
- 驱动层单连接 Mutex 不变；不引入连接池。

### 实施方案（分步）

1. **真机探针 V1–V5**（`crates/engine/src/bin/probe_b2.rs` 临时 bin，`pg_probe.rs:1-62` 同范式）：V1 两连接并发 LoadChildren（墙钟 ≈ 较慢者单连接水平）；V2 同连接第二任务排队不饿死、User 优先级可插队；V3 混合在途线程/Runtime 有上界；V4 长事务/锁等待下超时可观测；V5 半开断连不挂死；
2. **端点**：PG 真机 `192.168.3.138:5432/warehouse_supply_chain`（已知）；**MySQL 连接串待用户提供**；
3. 验证报告落 `docs/analysis/`（gitignore）；探针归档后即删；
4. 通过 → 维持/按 V3 数据定软帽 K；任一判据失败 → 回退全局串行，现象登记 architecture.md §11。

### 优劣

| 方案 | 优 | 劣 |
| --- | --- | --- |
| per-conn=1 + 软帽 K=3 | 跨连接并行收益明确（两连接同时展开互不等待）；软帽防线程/Runtime 失控；真机闸门前置防回归 | 同连接内仍是 Mutex 串行（驱动层单连接决定，无法避免） |
| 维持全局串行（现状） | 最稳、已真机验证 | 多连接场景下第二连接展开体验差（竞品普遍并行） |
| 直接并发放开（无软帽） | 收益最大化 | navop 负向证据：无上限无优先级无序竞争（`nav-analysis-navop.md:65,90`）；线程失控风险 |
| 恢复连接池 | 查询并发能力 | 2026-10-08 真机教训（30s acquire_timeout 静默卡死），用户已拍板弃池（`factory.rs:138-159`） |

---

## 4. 收尾项

### 4.1 提交隔离（S3-④ + B-1 + S4-N6）

**问题点**：工作区有并行会话未提交改动（B 档文档 + README 索引 +133 行、`crates/plugin/*`、`result_panel/src/grid.rs` 等）。S3-④ 改动 13 文件 + L2 回写 2 文件未提交。

**实施方案**：`git status --short` 核对 → 显式文件清单 `git add`（S3-④ 提交 = 设置/数据库/engine 三层 13+2 文件）→ 中文提交信息（`feat(数据库导航): …` 格式）→ 逐批提交（S3-④ → B-1 → S4-N6），不跨改未授权模块。

### 4.2 ui_contract 复跑

**问题点**：S3-④ 后未复跑（后台任务结果未取回，工具通道当时故障）。

**实施方案**：`cargo test -p rds-workbench --test ui_contract`（校验 ui.rs 常量、无裸 px/色值）+ `cargo test -p rds-database --lib`（198 基线）+ `cargo test -p rds-engine --lib`。

### 4.3 真机缺口记录（已知，不阻塞本批）

- **MySQL `TABLE_ROWS`**：属新写 SQL（全仓仅 PG reltuples 先例 `property_providers/pg.rs:247-248`），B 档 §1.4.3 要求真机跑通再接入；本批按授权直接接入，**缺口记录**：待 MySQL 真机端点提供后走查数值口径（InnoDB 估计 vs MyISAM 近似精确）；
- **B-2 探针 MySQL 连接串**待用户提供；
- **B-2-E** 旧 7 个池文件死代码清理（另立批次，不阻塞）；
- **跨 crate 千分位重复实现 4 份**（`result_panel/grid.rs:2846` / `editor/history.rs:257` / `analytics_resource/present.rs:71` / `mock/mock_view.rs:1057`）抽取合并属另案，不在本批。

---

## 5. 批次顺序（本会话执行）

| 顺序 | 批次 | 验证 | 提交 |
| --- | --- | --- | --- |
| 1 | **B-1 v1 完成**（settings 落盘 → 纯函数 → 接线 → UI） | `cargo check -p rds-database -p rds-workbench` + 规则矩阵单测 | `feat(数据库导航): 连接对象名过滤器 v1` |
| 2 | **S4-N6 staged**（状态 → token → 渲染 → 提交/撤销 → 菜单入口） | check + 状态机单测 | `feat(数据库导航): 破坏性操作 staged 提交` |
| 3 | **收尾**（ui_contract + 全量测试 + S3-④ 提交 + 缺口记录回写 B 档） | 全量测试全绿 | 逐批提交 |

> B-2 真机探针 = 另会话事项（需要用户提供 MySQL 连接串 + 真机时段）；本会话只保证调度代码不与本批改动冲突（已在 HEAD，不重做）。
