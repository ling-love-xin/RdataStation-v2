# 联邦查询公共入口 · 方案与开发计划

> 状态：**设计稿（2026-11-13），未动代码**。本文回答「洞察 / DuckDB 二次分析 / 编辑器联邦」如何共用一个分析入口与一套 DuckDB 资源。
> 上位口径：`docs/architecture/federation/README.md`（模块硬约束）、`federation-architecture.md`（决策 D1/D2/D8/D12/D14/D16）。

## 0. 一句话

把「在 DuckDB 上做分析」抽象成一个 **`AnalysisSession`**：编辑器按执行通道显式取用，洞察与二次分析默认取本地、可显式升级为联邦；三处共用同一套按 owner 缓存的会话，从而把 **`ATTACH` / 扩展 `LOAD` 从「每次进入」降到「每个 owner 一次」**，并让同一会话里的临时表互相直接读。

## 1. 现状（证据基线）

| 事实 | locator |
| --- | --- |
| 取会话入口已存在，按 `owner + 源指纹` 复用；`session_for` 是纯内存读 | `engine/src/duckdb/federation/session.rs:509`、`:562`、`:485` |
| 指纹不含主源：换主源只做 `USE`，不重建会话 | `session.rs:481-484` |
| 连接配置已统一（扩展目录 / 不静默联网 / 自动加载 / 内存闸 / 溢写口） | `manager.rs:465-473` → `rds_store::duckdb_config`；`session.rs:342-349` 也走它 |
| 三种连接形态并存：内存单例（全局 `Mutex`）/ 文件库池（1 写 + N 读 + 1 维护）/ 会话 | `manager.rs:18`、`:58-73`；`accel.rs:331`；`session.rs:49` |
| 临时表登记是进程级全局名单，不区分表建在哪条连接 | `manager.rs:21`；`temp_table.rs:41-66` |
| 洞察取样已有「源库查 → Arrow → 本地 `tmp_i_`」跨连接搬表路径 | `insight/src/service/persistence.rs:297-308` |
| 二次分析硬编码单例，且入口在 `QueryRunner` 工作线程 | `engine/src/services/execution_service.rs:113-127`；`editor/src/ports/editor_exec/runner.rs:492` |
| 编辑器联邦：三档显式（源库 / 本地加速 / 联邦），源清单 = 全部开 `use_duckdb_fed` 的连接，owner = 文档绑定 conn_id | `workbench_shell/src/model.rs:232-241`；`editor/src/ports/editor_exec/federation.rs:28`、`:36-61` |
| 洞察 / 二次分析：没有通道概念，默认就走 DuckDB | `insight_new_dialog.rs:22-27`；`result_panel/src/analysis.rs:37-52` |
| 门控真值 = ≥2 个可挂源（纯函数，已有测试钉住） | `editor/src/ports/editor_channels.rs`；测试 `:233`、`:242` |
| 洞察 `SampleSource` / `InsightTarget` 无 serde | `insight/src/model/target.rs:13`、`:89` |

**四个断点**：连接不共享（`ATTACH` 连接级）；选择模型不同（对象选择 vs 全量挂载）；模型缺「会话」维度；物化与生命周期三套。

## 2. 设计原则

1. **共享会话，不共享连接**。单例是全局 `Mutex`（`manager.rs:18`），把网络扫描塞进去会连坐洞察 EDA / Mock / 导出；且 `interrupt` 是连接级单值（`session.rs:51-54`），共享一条连接会让「取消洞察」误伤编辑器查询。
2. **取值同源，表达不同**。编辑器是「每次执行的三档选择」，洞察 / 二次分析是「一次作业的默认值 + 可选升级」。底层同一个 `AnalysisSession`，UI 不强行统一成一个控件。
3. **表在哪条连接，就在哪条连接清**。本地 `tmp_i_`/`tmp_q_` 由 TTL / 上限 / 切项目清场管；会话内 `tmp_*` 随会话销毁（源指纹变化、`drop_session`）。
4. **能力由声明决定，不由库类型硬编码**（为 Oracle 第三方插件 ATTACH 预留，见 §5）。

## 3. 模型

### 3.1 取值与动作

```
AnalysisSession =
  | Local                 // DuckDBManager::get_or_create_in_memory()
  | Federated { owner }   // ensure_session(owner, sources, primary)

动作
  run(sql) -> QueryResult                     // 读；写语句按通道语义拒（L2 走会话层 write_refusal）
  materialize(batches) -> table_name          // Arrow 直灌（复用 duckdb_service::create_temp_table_from_batches）
  materialize_rows(columns, rows) -> name     // 降级档（显示矩阵猜型）
  snapshot() -> SessionSnapshot               // 纯内存，渲染路径可调
  lease() -> LeaseToken                       // 执行租约（见 3.6）
```

`FederatedSession` 需新增 `materialize`（`conn: Mutex<Connection>` 的 guard 可 `&mut`，不改结构）。

### 3.2 owner

`owner = conn_id`。理由：它已是现成缓存键；洞察 `SampleSource.conn_id`、结果集所属文档绑定连接、编辑器文档绑定连接天然对齐 → 同一连接上的编辑器、洞察、结果集分析共享同一条会话。

### 3.3 两种接入形态（核心判断）

| 形态 | 谁用 | 为什么 |
| --- | --- | --- |
| **搬结果**：联邦会话上跑取样 SQL → Arrow 搬回本地建 `tmp_i_` → 后续分析仍在单例 | 洞察（列画像 / 表探查） | 洞察的规则 / 画像 / 直方图 / 相关全部是对**本地临时表**跑 SQL；整链搬到会话等于重写洞察引擎 |
| **同会话**：`materialize(batches)` 与分析 SQL 都在联邦会话 | 二次分析（`{table}` 自定义 SQL） | 用户 SQL 本身要跨源 join；表在别的连接上就看不见 |

附带修正：`SampleSource::on_duckdb` 注释承诺的「已 ATTACH 进来的库表」目前**没有任何代码**支撑（没有代码把库 ATTACH 到单例）。该语义由「联邦取样」兑现，`on_duckdb` 收紧为「文件类 + 本地库表」。

### 3.4 表与生命周期矩阵

| 表 | 建在哪 | 谁清 | 会话重建后 |
| --- | --- | --- | --- |
| `tmp_i_`（洞察样本） | 本地单例 | TTL / 上限 | **仍可用**（这是形态 I 的收益） |
| `tmp_q_`（本地分析） | 本地单例 | 切项目清场（`project_host.rs:198`） | 不受影响 |
| `tmp_q_`（联邦分析） | 联邦会话 | 会话销毁 | **消失**，结果集条目要标「表已失效，可重新分析」 |
| 会话内其它本地临时对象 | 联邦会话 | 会话销毁 | 消失（现有语义，`session.rs:31-32`） |

**必须一并解决**：`TempTableManager` 是全局名单（`manager.rs:21`），现状能工作只因为所有 `tmp_` 都建在单例。会话里若也建 `tmp_` 并沿用同一名单，「登记」与「删除」会指向不同连接。定案：**会话内表不进 `TempTableManager`**，由 `AnalysisSession` 自己记「表数 / 字节」并在 `snapshot()` 暴露；`TempTableManager` 只登记内存单例的表。

### 3.5 门控与降级

复用 `editor_channels` 的纯函数：`availability(owner).for_channel(Federated)`（`editor_channels.rs`，测试 `:233/:242` 已钉住真值）。

- ≥2 个开了 `use_duckdb_fed` 且驱动可挂的连接 → 允许联邦；
- 只有 1 个 → 入口**不显示**联邦开关（不是灰显），文案落到「本地加速」；
- 有源但挂不上 / 驱动不支持 → 原因点名到具体连接（沿用现有文案）。

### 3.6 并发与中断（租约）

`FederatedSession.running` 现在是单值布尔。共享后改为「执行者令牌 + 串行排队」：`lease()` 返回令牌，`cancel(owner, token)` 只中断持该令牌的那一次执行；无令牌的执行不能取消别人的语句。

### 3.7 锁纪律

- 顺序固定：**先取会话（可能 I/O，持 `SESSION_LOCK`/`mount_lock`）→ 再取连接锁（短）**；
- 禁止在持有单例锁时调 `ensure_session`；
- `ensure_session` 只允许在工作线程调用（`session.rs:504` 已声明是事件路径）。

## 4. 入口形态（UI）

一次分析作业的三个决策点：

```
① 分析对象（必选：数据库表 / DuckDB 表 / 文件）  ← 三分法；叶子在组内再分小节
② 数据量 / 过滤 / 种子                       ← 现有确认窗口（ProbeConfig）
③ 会话（默认本地 DuckDB；可选「用联邦查询」）  ← 新增，默认关，仅 ≥2 源时出现
```

**来源分类（根 3 / 叶 5）**：根分类只回答「怎么读」——数据库（驱动 + 连接）、DuckDB（本地 SQL，装的是**临时产物**）、文件（reader 函数，装的是**本机持久物**）。三个根恰好是「持久/临时 × 本机/外部」二维表里用户可感知的三格：

| | 本机 | 外部 |
| --- | --- | --- |
| 持久 | 文件：草稿 / **资产库** | 数据库：连接的表 |
| 临时 | DuckDB：结果集 / Mock / 插件 / 会话表 | — |

`SourceKind` 枚举保持 5 个不变，UI 层只用 `group_of(kind)` 做分组，`PickerConfig.kinds` / `has_target` / `confirm_prefix` 都不动。

- **结果集并入 DuckDB 组**，不单开 tab：它本来就是引擎里的临时表，洞察只要当前可见也能读。
- **「资源分析」正名为「资产库」，挂在文件根下**：`ArchiveKind::File` 的本体在 `{项目}/resources/`，`ArchiveKind::Analysis` 的本体在 `{项目}/.RSmeta/analytics.duckdb`，两者都是本机持久物，与草稿同类；DuckDB 根只装临时产物。资产库与左侧 M6 面板是**同一实体的两个视图**：面板管生命周期（分组/版本/检出），picker 只负责「选它来分析」。
- 资产库组内分三段：`本项目 · 归档文件` / `本项目 · 分析表` / `全局 · 跨项目共享`。前两段数据来自 M6 登记与 `information_schema.tables`（限定 `WHERE table_schema='main'`，否则 `temp` schema 的临时表会混进资产库）；全局段走**全局库连接**。
- **M6 二期不等**（用户 2026-11-13 裁决）：`ArchiveKind::Analysis` 直接按完整语义（版本 / 归档态 / 只读）呈现，不降级成「裸表名」。
- **全局实例必须进入**（用户裁决，理由是项目之间的桥梁）：全局表在 `{system}/analytics.duckdb`，表名带 `G_` 前缀，登记在 `{system}/global.db`（`scope='global'`）；`项目 → 全局 → 项目` 是唯一法定路径，项目之间禁止直传（`analytics_resource/src/global/mod.rs:18-19`）。两段在树里**并列不混排**——用户必须一眼看出改的是本项目副本还是全局原表。
  - **硬约束**：全局库是单写者（`GlobalDatabaseManager::duckdb_conn()` 持有），picker 不得另开 `Connection::open`；现状 `collect_analysis_tables` 正是直接 open，接全局前必须换通道。
  - **已知缺口**：`generate_attach_global_sql`（`federation/legacy.rs:336`）**没有生产调用点**，本地单例与联邦会话目前都没挂全局库，选中全局表会 `table does not exist`。补这一步前先实测「单写者连接 + 只读 ATTACH」能否共存。
- **必须保留的一处区分**：本地档 `tmp_q_` 本机可见，联邦档 `tmp_q_` 只在会话可见、会话重建即失效；列表用 pill 标注可见范围，否则用户会得到 `table does not exist`。

**对象树与搜索**：三个根内都是可折叠树（现状 `render_connected` 是单列逐层追加），深度上限 3，列不做节点（改到右侧元信息）。结构与名字是两条路径：树解决「我知道在哪」，跨根搜索解决「我知道叫什么」（匹配档位：前缀 > 子串 > 子序列）。**搜索最小单位是表 / 视图，不搜列名**（用户裁决）。复用 `workbench_shell::tree` 的行原语（`active_bar` / `disclosure_slot` / `disclosure_icon` / `indent_*` / `RowHeight`）与 `ui::TREE_INDENT`，**不复用 `tree::Tree`**——外壳已裁决「这一行长什么样归各面板」，picker 是第 5 个消费方。详见 `docs/architecture/federation/prototypes/entry/06-object-tree.html`。

- 编辑器：三档常驻工具栏（现有），不改；
- 洞察 / 二次分析：③ 是确认对话框里的**高级选项**，默认关；打开才展开「源清单摘要 + 主源 + 门控原因 + 远端扫描代价」。
- 原型：`docs/architecture/federation/prototypes/entry/`（索引见该目录 `README.md`；本阶段主图是 `01-source-picker.html` 与 `06-object-tree.html`）。

## 5. Oracle 第三方插件 ATTACH（预留契约）

现状：`AccelKind::Oracle` 的能力是**按库类型硬编码**——不支持 `READ_ONLY`、凭据走会话级 `Secret`、限定名两段、扩展从 community 仓库装（`accel.rs:53-56`、`:161-181`；`registry.rs:136` `oracle_secret_from_url`）。第三方插件（本地 `.duckdb_extension` + manifest）若沿用这套硬编码，每来一个库就要改一处 `match`。

**改为「扩展描述（ExtensionProfile）」由内置 scanner 或插件声明**：

| 字段 | 含义 | Oracle（现状） |
| --- | --- | --- |
| `source` | `Core` / `Community` / `LocalPlugin { path, manifest }` | `Community` |
| `read_only_attach` | 能否带 `READ_ONLY` | `false` |
| `secret` | 凭据形态（`None` / `OracleService`） | `OracleService`（`oracle://…/服务名` → `CREATE SECRET`） |
| `qualify_segments` | 限定名段数（1 / 2） | `2`（`<别名>.<表>`） |
| `install_sql()` | 由 profile 生成，不再散落 `match` | 带 `FROM community` |
| `extension_file` | 扩展文件名 / 版本 | `oracle_scanner` |

**三条硬约束**（都要保留在 `FederatedSource::attach_sql` 这一处，不得分散）：

1. 只读：能带就带 `READ_ONLY`，不能带则**必须有** `write_refusal` + 编辑器闸门 + 建议只读账号；判定按 `read_only_attach`，不按库类型；
2. 凭据：`secret` 非空时只走会话级 `CREATE OR REPLACE SECRET`（不落盘，重挂重建）；
3. 限定名：按 `qualify_segments` 生成补全前缀与错误提示。

**第三方插件的两个安全前提**（要用户知情，不擅自开）：

- DuckDB 默认拒绝未签名扩展；加载本地第三方扩展需显式 `allow_unsigned_extensions`——这是**安全决策**，必须在设置页显式勾选并写明风险，不能默认开；
- 扩展目录钉在应用数据目录（现有 `configure_connection` 已做），插件装到目录后仍需 `INSTALL`/`LOAD` 状态可见（失败原因要区分：未安装 / 未签名 / 版本不匹配 / 认证失败）。

## 6. 开发计划

> 依赖顺序严格自上而下；每阶段都能单独验收、单独回滚。工作量按「改动面 + 门禁」评估，不编天数。

### Phase 0 · 体检冻结（只读）

- 产出：`engine/duckdb` 体检表（连接模型 / 表生命周期 / 锁 / 过时注释）；
- 确认三件事：`legacy.rs` 是否零调用、`bridge.rs` 是否只被文档引用、`TempTableManager` 的所有调用方是否都在单例；
- 门禁：`cargo test -p rds-engine -j 2 --lib -- federation` 基线记录。

### Phase 1 · 引擎抽象（不改用户可见行为）

| 切片 | 落点 | 验收 |
| --- | --- | --- |
| `AnalysisSession` + `materialize` | `engine/src/duckdb/federation/session.rs`、新增 `analysis_session.rs` | 单测：`Local` / `Federated` 两态动作一致 |
| 会话内表读数 + 配额 | 同上 + `TempTableManager` 边界注释 | 单测：会话销毁后表数归零；内存表登记不受影响 |
| 执行租约 | `session.rs`（`running` → 令牌） | 单测：A 取消不影响 B |
| `execute_duckdb_analysis_from_batches` 增会话参数（默认 `Local`） | `engine/src/services/execution_service.rs:113` | 既有测试全绿（行为不变） |

### Phase 2 · 二次分析同会话

- 落点：`editor/src/ports/editor_exec/runner.rs:492`、`editor/src/ports/result_service.rs:54`、`result_panel/src/analysis.rs:37`；
- 行为：`AnalysisRequest` 增 `channel`；选联邦时 `materialize` + `run` 都在会话上；
- 验收：跨源 join 的自定义 SQL 能出结果；本地档逐字不变（区分性用例）。

### Phase 3 · 洞察搬结果

- 落点：`insight/src/model/target.rs:14`（加变体）、`insight/src/service/persistence.rs:85`（加分支）；
- 行为：联邦会话 `run(取样 SQL)` → `create_analysis_temp_table_arrow` 建本地 `tmp_i_`；
- 验收：断言 `tmp_i_` 在本地单例可见、联邦会话不可见（推翻"整链跑会话"的错误实现）；源指纹变化后样本仍可分析。

### Phase 4 · 扩展契约（ExtensionProfile）

- 落点：`engine/src/duckdb/accel.rs:53-181`（能力判定）、`federation/registry.rs`（`attach_sql` / `Secret`）、`extensions`/插件目录发现；
- 行为：Oracle 能力改由 profile 声明（结果与现状等价）；新增 `LocalPlugin` 来源；
- 验收：Oracle 真机用例（`RDS_TEST_ORACLE_URL`）逐条不变；新增一个"本地插件 profile"的纯单测（不联网）。

### Phase 5 · UI 落地

- 落点：`workbench/src/components/source_picker.rs`（加第三段，默认关）、`insight_new_dialog.rs`、结果集「分析 ▾」；
- 验收：门控文案、降级行为、`SourceKind::ResultSet` 对二次分析开放；视觉与原型一致。

### 分阶段门禁

```powershell
cargo test -p rds-engine -j 2 --lib -- federation
cargo test -p rds-workbench -j 2 --lib -- services::editor
cargo test -p rds-editor --lib -- --test-threads=1   # 基线 776 passed / 3 failed / 1 ignored
```

真机用例需 `RDS_TEST_MYSQL_URL` / `RDS_TEST_SQLITE_PATH` / `RDS_TEST_ORACLE_URL`。

## 7. 可证伪验收（每条能推翻一种错误机制）

1. `same_owner_shares_one_session` — 两入口先后取会话，第二次不触发 `ATTACH`；
2. `insight_sample_lands_in_local_db` — `tmp_i_` 在单例可见、会话不可见；
3. `cancel_insight_does_not_touch_editor_query` — 租约隔离；
4. `single_source_hides_federation` — 1 源时入口不出现联邦开关；
5. `source_change_keeps_insight_sample` — 会话重建后洞察样本仍可用；
6. `federated_analysis_table_lives_in_the_session` — 二次分析表在会话里（推翻"建在单例也能跨源"）。

## 8. 风险与不做

**不做**：跨源写、跨源事务、快照一致（沿 `federation/README.md` §2）；把洞察规则引擎搬到会话；把 `ATTACH` 合并进单例；给内存单例加连接池（内存库不支持多连接共享，只会多一层锁）。

**风险**：

- 会话重建会带走联邦会话里的 `tmp_q_` → 结果集条目必须能标「已失效」并可重建（Phase 2 一并做）；
- 第三方插件若要求 `allow_unsigned_extensions`，是安全面扩大，必须显式勾选 + 风险文案；
- `ensure_session` 是 I/O，只能在后台线程；UI 侧任何"先预热一下"的写法都不能进 render。

## 9. 待拍板

1. owner 口径：`conn_id`（推荐）/ 面板级 owner；
2. 二次分析默认档：`Local`（推荐）/ 记住上次；
3. 洞察的联邦开关：仅 ≥2 源时出现（推荐）/ 常驻；
4. 远端扫描代价是否在确认框显示"本次在远端扫了多少行"（README 已承诺代价可见）；
5. Oracle 第三方插件是否走 `allow_unsigned_extensions`（安全决策）。

## 10. 需同步修正的过时口径

- `crates/workbench/src/components/project_host.rs:189-208`「结果集链路没接 UI（建表方零调用者）」——B15 已接（`editor/src/ports/editor_exec/runner.rs:492`）；
- `insight/src/model/target.rs:41`「已 ATTACH 进来的库表」——无实现支撑，改由联邦取样兑现；
- `mod.rs:52` 仍导出 `FederationManager`（legacy），Phase 0 确认零调用后摘除。

## 11. 入口口径定稿（2026-11-16）

1. **本质**：联邦 / 二次分析 / 洞察都是「DuckDB 已加载对象」的**下游动作**，不是数据管理。对象只包括连接、持久表、临时表、外部文件；资产 / 项目 / 全局是业务标签，不是第四类数据。
2. **二次分析**：结果集工具栏「分析」→ 08 对话框（名字 + 去向）→ 进数据对象；**不自动开洞察**。落点：`crates/editor/src/view/dialogs/result_analyze.rs::open_result_analyze`、`crates/editor/src/view/host/results/sort.rs::finish_result_analyze`。
3. **去向只有两条**：临时（`tmp_q_…`，会话内，只能留在本项目）与持久（`res_<名字>_<时间戳>`，本项目分析库，可回溯版本）。
   **全局不在这里选**：只有资产归档处「升级到全局…」一条路（`crates/analytics_resource/src/resource_host/detail_actions.rs::request_promote_to_global`）。数据对象面板因此只保留「存为表」这一条升级路径（刀 12 撤掉了面板里的全局升级按钮）。
4. **命名**：用户输入的名字进表名。临时名的尺子只留 ASCII（`crates/engine/src/duckdb/temp_table/temp_table.rs::sanitize_description`），中文折成下划线——**对话框必须把这条说白**，并展示实际生成的表名；持久名保留中文（`sanitize_persistent_description`）。
5. **面板只显示最新版本**：一个对象一行，版本号是行尾 pill（`ArchiveRow::version`），历史版本只在资产库详情看（`history_label`）。面板不做版本展开、不做版本下拉。
6. **洞察单入口**：数据导航栏 / 草稿箱 / 分析存档 / 数据对象，四处**任选其一**进入；多源在洞察面板内用「+」追加，弹窗与数据对象面板的「加源」同一套（`01-source-picker.html`）。洞察产物（`tmp_i_`）不进数据对象面板，要留就走「保留到项目库」。
7. **面板分组按 DuckDB 容器**（已落地：`13b2211d` 改行模型、`814e26ac` 渲染 pill、`f2cfaf6a` 回写原型 07）：本项目分析库 / 全局分析库 / 已挂载的源 / 外部文件 / 临时表五段；资产、项目自建、全局、结果集、草稿降为行内 pill（`Object.pill`）。洞察产物 `tmp_i_` **不进面板**——它是抽取的样，不是全量；要留就在洞察里保留到项目库，落成持久表后再从这里看。`can_persist` 因此收紧到「临时表 + 可用」。
- **动作语义只剩一处「选用」**（2026-11-16 改口径，原 P7 的“连接行「加载」”删除）：连接不是可分析对象，连接行只是分组信息，不设动作；表 / 视图 / 文件 / 临时表一视同仁给「选用」。洞察弹窗里连接行只展开不可勾——这一条代码里已成立（`source_picker/node.rs::is_confirmable` 只放 Table|File|Temp），不需要再改。
9. **面板只承载一个对象**（已落地：`896421f6`）：行上「选用」，再点「洞察此对象」；多源在洞察面板内部按「+」加。软上限 8 只在洞察面板内部生效，面板不再触发。之前「分析已选（N）」的批量路径已删除。
10. **P8 未定位**：本轮在 `crates/database/src/nav_view` 与 `crates/workbench/src/components/source_picker` 两个候选处都没找到「DuckDB tab 给连接挂 catalog」的代码——picker 的 `group_of`（`components/source_picker/group.rs:49`）里 DuckDB 组只收 `ResultSet` / `InsightSample`，`Analysis` / `AnalysisGlobal` 归「文件」组（同文件 :112 有断言）。要改这条，得先指认实际挂 catalog 的入口（是导航树的 DuckDB 分支，还是编辑器侧栏的 DuckDB 分区）。
11. **联邦与二次分析共用同一份数据对象面板**（2026-11-16 裁决）：右侧只有 `RightPanel::DataObjects` 一个页签、`right_data_objects_tab` 一个实体（`crates/workbench/src/view/mod.rs:87`、`view/dock.rs:166`），联邦会话的源与结果集物化出来的临时表都落在这一份清单里，按容器分五段区分，不另开第二个面板。切换只影响「首次切到联邦时主动展开它一次」，避免用户切了通道却看不见可用的源。

12. **联邦的源清单就是数据对象全览（2026-11-16 用户口径；前两轮把它记成“没有加源”是误记）**：全览分两部分——① **本地 DuckDB 实例已加载的**：本项目分析库、全局分析库、外部文件、临时表；② **指向外部的**：连接 ATTACH 进来的源库与表 / 视图。07 面板的五个分组就是这两部分展开，「加源」= 往这份全览里加东西（01 的连接 tab 落 ②；文件 / DuckDB tab 落 ①）。最初设想过“全部一次性加载”，因为不合理才拆成这两部分。

13. **两条进清单的路，与真正缺的那两处（2026-11-16 核对代码后改正，前一轮说“全量挂所有连接”也是错的）**：进数据对象的只有两条路——① **自动**：本项目 / 全局分析库、本实例临时表、**开着本地加速（`use_duckdb_fed`）的连接**；② **手动**：面板「加源」走 `source_picker`，把未开加速的连接或外部文件加载进来。代码核对：自动这条已成立——宿主 `records()` 先按 `use_duckdb_fed` 过滤（`crates/workbench/src/components/editor_host.rs:151`），`federated_plan` 第二步补运行态连接时同样过滤（`editor_exec/federation.rs:51`）。真正缺的是两处：(a) **面板没有「加源」端口**——未开加速的连接、外部文件都无法据用户选择加载进会话；(b) **① 那一类在联邦会话里不可见**——联邦是独立 `open_in_memory()`（`federation/session.rs:345`），与临时表 / 分析库所在的进程单例 `GLOBAL_DUCKDB` 不是同一个实例（P4）；二次分析走的是 `GLOBAL_DUCKDB`，所以“联邦与二次分析一致”这条在实现上还没兑现。补 (b) 要 engine 动：库 `ATTACH ... (READ_ONLY)` 或 parquet 中转、外部文件建视图、临时表复制进去。

14. **「加源」的三条去向，待拍板（2026-11-16）**：picker 侧已通用——`open_source_picker(shared, PickerConfig{title,kinds,confirm_prefix,multi}, ConfirmStrategy, window, cx)`（`crates/workbench/src/components/source_picker/mod.rs:375-386`），`Picked` 七类（外部文件 / 草稿 / 资产 / 全局 / 结果集 / 洞察产物 / 库表），选中项用 `PickerState::picked_all()` 取（`insight_new_dialog.rs:56` 是现成写法）。真正要定的是**加载到哪**：(A) 加载进联邦会话——已落地 `session::add_file_view`（`a5201f8d`），切到联邦后 SQL 里能查该视图；但面板看的是 `GLOBAL_DUCKDB` 与扫盘结果，看不到这条，界面与事实会不一致；(B) 加载进进程单例 `GLOBAL_DUCKDB`——二次分析 / 洞察立刻能用（它们本来就在这个实例里），要给 engine 补一个“在单例上建视图”的入口，面板外部文件段再并入这一行；(C) 物化成临时表 `tmp_q_`——面板「临时表」段自动出现，最一致，代价是整表拷一份。

B 已落地一半（2026-11-16）：engine `duckdb::file_view::load_file_global` 在单例上建 `f_<ascii>` 视图（`c6d3a0ce`），`Shared.loaded_files` 记（视图名, 原路径）并由 `collect` 并入面板「外部文件」段（`Collected.loaded_files`）。**还差 UI 入口**：面板工具栏「加源」按钮 → `source_picker`（`kinds:[External,Scratchpad]`，`multi:true`）→ 逐项 `load_file_global` → `window.close_dialog`。

文件这条已闭环（2026-11-16）：面板工具栏「加源」（`e2f10895`）→ `source_picker` → `load_file_global` → 缓存写回，面板即时多出「已加载」行。

连接这条差最后一段：`FederatedSession::add_source` 已具备（`0c12383a`，会话开着也能追加源、重名先挡、失败留痕），但面板侧要构造 `FederatedSource{conn_id, alias, kind: AccelKind, connection_string, secret}`（`registry.rs:197`）——连接串得从 `DataSourceService` 取回再 `build_connection_url`；且会话 owner 是“文档绑定的连接”、面板是全局单例看不到，只能取 `session::owners()` 里最新那个，并在对话框顶部写明会挂到谁。建议 B：视图不拷、与二次分析同一个实例；只有需要稳定快照（源文件会变）时才走 C。

## 12. 数据对象面板 · 面板规格（2026-11-18 实施批定稿）

> 本节是右侧 `RightPanel::DataObjects` 面板（07 原型承载）的**规格权威**：把面板落成「分析引擎 DuckDB 视角的对象总清单」——写 SQL 时回答“我此刻能引用什么”。§11 的 14 条口径在本节继续有效，冲突时以本节为准（更细）。
> 实施批（刀 0–5）**已于 2026-11-18 全部落地**（逐刀记录与遗留清单见 `federation-entry-handoff.md` §9）；§12.5 为核实后定稿（不做第二条资产取数）。

### 12.1 口径一句话

**面板 = 分析引擎视角的对象清单**：只列“此刻已经被某个 DuckDB 实例（进程单例 / 联邦会话 / 本地加速会话）装进去的东西”。源库直连档下未挂载的连接**不进面板**（那是 M4 导航树的领域）；差异靠行上通道徽标表达，不做两棵树。

### 12.2 分组与树化

- 顶层是**四个形态段**（`Group`，2026-11-18 合并定稿，取代 11-16 的五容器口径）：已挂载的源 → 持久表 → 文件 → 临时表；业务身份（资产 / 项目自建 / 全局 / 结果集落库 / 已加载）是行内 pill。「持久表」合并了项目分析库与全局分析库（项目 vs 全局是标签不是形态）；「文件」只收**已加载**的（外部文件视图 / 手动加入的草稿视图）与资产·文件型——**未加载的草稿不进面板**，要「加源」手动加入。
- **树化只做一层**：「已挂载的源」段的源行可展开出**表 / 视图叶子**。挂载源包含两类（pill 区分）：
  - **联邦源**（`federation::session::owners()` 快照，pill「源」）；
  - **本地加速源**（`accel::owners()`，pill「加速」）——accel 是“一源一条专用 DuckDB 连接”，表清单照 `duckdb_tables()` 按 `rds_src` 过滤；note 提示“引用需切到对应执行通道档”。
- 叶子在**事件路径**懒加载（展开回调查引擎，结果缓存进面板字段），render 零 I/O——沿 source_picker `load_db_layer` 先例。
- 复用 `tree::{active_bar, disclosure_slot, disclosure_icon, indent_*}` 行原语与 `RowHeight`；**不引 `tree::Tree`**（06 卡 02 清单）。深度预算：段头 → 源行 → 叶子，共三层可视层级，列不做节点。

### 12.3 叶子动作：复制限定名（如实降级）

叶子唯一动作是**复制限定名**：联邦源按 kind 生成三段 `别名.schema.表`（L2 两段 `别名.表`）；加速源给 `rds_src.schema.表`。**不给「选用 / 洞察」入口**——联邦 / 加速表在独立 DuckDB 连接上，洞察取样（进程单例）够不着；等 Phase 3「洞察搬结果」落地后再开。宁可少一个入口，不摆一个点了会炸的。

### 12.4 行模型映射（`components/data_objects/mod.rs`）

| 对象 | key 前缀 | Channel | Group | pill |
| --- | --- | --- | --- | --- |
| 联邦会话源 | `session:{owner}:{alias}` | Federated | MountedSources | 源 |
| 本地加速源 | `accel:{conn_id}` | Federated | MountedSources | 加速 |
| 项目分析库自建表 | `project:` | Local | PersistentTables | 项目自建 |
| 结果集落库（`res_`） | `persistent:` | Local | PersistentTables | 结果集落库 |
| 资产·表型（只读最新版） | `asset:` | Asset | PersistentTables | 资产 |
| 资产·文件型（只读最新版） | `asset:` | Asset | Files | 资产 |
| 全局分析库（`G_`） | `global:` | Local | PersistentTables | 全局 |
| 已加载文件视图（外部文件 / 手动加入的草稿） | `fileview:` | File | Files | 已加载 |
| 结果集（`tmp_q_`） | `result:` | Process | TemporaryTables | 结果集 |

- **Mock（`tmp_m_`）与插件（`tmp_p_`）产物不进面板**（2026-11-18 用户口径）：mock 的下游是
  草稿箱 / 外部目录 / 资产归档，插件产物归插件——它们是各模块的**生成物**，不是分析链路的
  输入对象；与 `tmp_i_`（洞察样本）同一条纪律。

- **`can_persist` 收紧为按前缀**（`tmp_q_` / `tmp_i_`）：Mock / 插件表各有模块管生命周期，不从面板落库（`persist_label` 本就按前缀剥前缀）。
- **失效语义不变**：失效行留着带原因（`State::Stale/Failed`），不给动作只给“为什么”。
- **未加载的草稿不进面板**（2026-11-18 口径）：草稿是“候选文件”不是 DuckDB 对象，自动列出来与“只含已加载对象”冲突；「加源」picker 里手动选了才进来（成为 `f_<ascii>` 视图）。

### 12.5 资产行来源（单份取数，2026-11-18 核实后定稿）

资产行由**资产面板实体**供给：侧栏构造期就建好该实体并预载一次快照（`build_resources_panel` 尾部
`refresh_resources`——那条预载的注释点名就是为数据对象面板），项目切换（`project_host`）与资产增删
（归档 / 取回 / 回收站）都会推新快照；数据对象面板 refresh 时从 `view_rows()` 翻译成 `AssetRow`。
**不做第二条取数**（enqueue + drain 直达登记层）：那会造出两套真相，与“面板与对话框共用同一份取数”
的模块立身之本冲突。曾以为存在“资产面板没载过就没有行”的缺口——核实为过时信息（预载早已落地）。

### 12.6 「加源」：三分法与连接线（§11.14 收口）

「加源」弹 01 原型形态（`source_picker`，已是三分法树）。kinds 扩为 `[Connected, External, Scratchpad]`（DuckDB 根在加源语境无叶子，自动不显示）。三条去向：

| 去向 | 语义 | 状态 |
| --- | --- | --- |
| B 文件线 · 数据文件 | 进程单例建 `f_<ascii>` 视图，二次分析 / 洞察立即可用（视图行可「选用」） | ✅ 已闭环 |
| B 文件线 · 库文件 | `.duckdb` / `.sqlite` **ATTACH 成 catalog**（2026-11-18 拍板：库文件走 ATTACH；DuckDB 自带扩展与凭据管理，只读挂载与联邦源同纪律），行进「已挂载的源」段、可展开列表 | ✅ 本批落地 |
| C 连接线 | **标记 `use_duckdb_fed`（持久）**，回执写明“下一次联邦执行时挂载；源清单变化会重建会话（D12）” | ✅ 已落地 |
| D 表级全量拉取 | **不支持 scanner 的库**（国产库等）：picker 里只能选其**表 / 视图**，确认后**全量拉取**进 DuckDB 物化（2026-11-18 拍板：做分析，肯定全量） | 🟡 已拍板未实现——归第二期 L3 表级刀（拉数链路 + 大表内存闸兜底），现状对这类库标记开关也无效（`federated_plan` 不认其驱动），界面如实跳过 |
| A 会话视图 | `session::add_file_view`，联邦会话内可见 | 保留能力，加源默认不走（面板看不到会话视图，界面与事实会不一致） |

连接线**不做“立即挂进当前会话”**：会话 owner 归文档（D2），面板是全局单例不越权；标记开关与 `federated_plan` 的组装口径（D15/D16）天然一致。

### 12.7 边界（防误解）

- 洞察产物 `tmp_i_` **不进面板**（§11.7）；要留走“保留到项目库”。
- 全局库**不 ATTACH 进单例 / 会话**：全局行走短命挂载会话（`analysis::global`）+ `SampleSource::on_global`，单写者约束维持；“列得出、SQL 直查不到”的缺口等 Phase 1+ 一并处理并在行 note 如实说明。
- 「升级到全局」只在资产库详情（§11.3）；面板不设该按钮。
- 不做拖拽 / 右键 / 多选 / 重命名（06 卡 06 差异表）；工作集仍是“面板只承载一个对象”（§11.9）。

### 12.8 实现位置映射

| 规格 | 落点 |
| --- | --- |
| 行模型 / 纯函数（objects / analysis_sources / can_persist / session_actions） | `workbench/src/components/data_objects/mod.rs` |
| 面板（refresh / 渲染 / 动作 / 树化字段） | `workbench/src/panels/data_objects_tab.rs` |
| 加源对话框 | `workbench/src/components/data_objects_load.rs`（复用 `source_picker`） |
| 联邦表清单列举 | `engine/src/duckdb/federation/session.rs`（`list_tables` / `list_tables_for`） |
| 加速会话枚举与表清单 | `engine/src/duckdb/accel.rs`（`owners` / `list_tables_for`） |
| 库文件挂载与列举（ATTACH） | `engine/src/duckdb/file_view.rs`（`is_database_file` / `load_database_global` / `list_tables_global`） |
| 资产直达 | `analytics_resource::jobs::{enqueue_refresh, drain_snapshot}`（复用，不改） |
| 取数复用 | `source_picker::{analysis_tables_of, collect_drafts, split_persistent_tables, persistent_table_label}`（复用，不改） |