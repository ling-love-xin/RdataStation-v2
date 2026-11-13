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

**对象树与搜索**：三个根内都是可折叠树（现状 `render_connected` 是单列逐层追加），深度上限 3，列不做节点（改到右侧元信息）。结构与名字是两条路径：树解决「我知道在哪」，跨根搜索解决「我知道叫什么」（匹配档位：前缀 > 子串 > 子序列）。**搜索最小单位是表 / 视图，不搜列名**（用户裁决）。复用 `workbench_shell::tree` 的行原语（`active_bar` / `disclosure_slot` / `disclosure_icon` / `indent_*` / `RowHeight`）与 `ui::TREE_INDENT`，**不复用 `tree::Tree`**——外壳已裁决「这一行长什么样归各面板」，picker 是第 5 个消费方。详见 `prototypes/entry/06-object-tree.html`。

- 编辑器：三档常驻工具栏（现有），不改；
- 洞察 / 二次分析：③ 是确认对话框里的**高级选项**，默认关；打开才展开「源清单摘要 + 主源 + 门控原因 + 远端扫描代价」。
- 原型：`docs/architecture/federation/prototypes/entry-prototype.html`。

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
