# 联邦查询 · 设计理念与架构

> 关联：`federation-prototype-design.md`（长什么样）、`federation-dev-plan.md`（怎么落地）。
> 本文 §8「已知问题」是**权威清单**：代码与其它文档与它冲突时，以它为准。

## 1. 定位

联邦查询回答一件事：**让一条 DuckDB 会话同时看到多个外部源，并在它们之间做只读的跨源查询**
（join / union / 派生表），而用户不必关心数据是怎么搬的。

它在数据层里的位置：DuckDB 是分析引擎（`shared.duckdb` / `analysis.duckdb`），联邦是**这条引擎的
外部视野**——不是新引擎、不是新连接池、也不是 ETL 工具。

## 2. 三层选型（有实测依据）

选型不是查文献定的，是 `crates/engine/tests/duckdb_extensions_probe.rs` 实跑出来的
（提交 `ad35b21`；动态链接的 libduckdb 1.5.5）：

| 层 | 覆盖 | 实测结论 |
| --- | --- | --- |
| **L1 官方 scanner** | `mysql` · `postgres` · `sqlite` · `parquet` · `json` · `httpfs` | ✅ INSTALL + LOAD 全过 |
| **L2 社区 scanner** | `mssql`（native TDS，无需 ODBC/FreeTDS）· `oracle_scanner`（无需 Oracle client）· `firebird` · `snowflake` · `bigquery` · `mongo` … | ✅ 前四个 + `firebird` 全部 INSTALL + LOAD 成功（社区扩展由 DuckDB 官方签名托管，`INSTALL … FROM community`）。**`oracle_scanner` 真机验收完成（2026-09-18，v0.2.2）**：见 §2.1 |
| **L3 桥接兜底** | 任何我们有驱动的源（国产库 / 私有协议 / 受限环境） | 依赖自家驱动与 Arrow 批（已有），**不依赖 DuckDB 扩展** |

**为什么 L2 不是“唯一路径”**：早期判断是“Oracle / SQL Server 只能拉数”，实测推翻了它——
两个库都有社区 scanner（`mssql` 支持 projection / filter / ORDER BY 下推与 SSPI/Kerberos/TLS；
`oracle_scanner` 的 `oracle_filter_pushdown` **只翻译可证明等价的谓词，其余报错**）。
于是 L3 的定位从“Oracle/MSSQL 的必经之路”降为**兜底**。

**为什么不用 ODBC 当通道**：DuckDB 1.5.5 **没有** ODBC 扫描器（社区列表里只有 ADBC 一族）。
要走 ODBC 就得自己写“ODBC 游标 → Arrow → 表”的桥——那与 L3 重复，还要额外背“用户必须装 ODBC
驱动”的分发负担与位宽 / 编码 / 线程模型的坑。**不做**。

**为什么 ADBC 只是可选层**：`adbc` / `adbc_scanner` 扩展现成（Arrow 原生、零转换），但它要求用户
机器上有**具体库的 ADBC 驱动**；对 Windows 桌面应用这是分发与运维负担。等出现“两种 scanner
都没有、但有 ADBC 驱动”的库时再接。

### 2.1 Oracle 真机验收（`oracle_scanner` 0.2.2，2026-09-18）

探针：`crates/engine/tests/oracle_probe.rs`（跑法：`RDS_TEST_ORACLE_URL='oracle://user:pass@host:1521/XEPDB1'`）。
实例：`192.168.3.138:1521` · service name **`XEPDB1`** · 普通开发账号（无 DBA 权限）。

| 问题 | 结论 |
| --- | --- |
| 凭据怎么给 | **只能走 Secret**：`oracle_query('<secret>', …)` / `ATTACH '<secret>' AS x` 的第一参数是 **secret 名**（不是连接串）；`CREATE SECRET (TYPE ORACLE, HOST/PORT/USER/PASSWORD/SERVICE_NAME)`，**会话级即可**（不必落盘） |
| 服务名 | `XEPDB1`（写成 `XE` 报 `ORA-01017`，其它名字报监听器 redirect——界面上要如实把原话给出） |
| 挂载形态 | ✅ `ATTACH '<secret>' AS <别名> (TYPE oracle_scanner)`，表挂在 **`main`** schema → **两段名 `<别名>.<表>`**；`duckdb_tables()` / `duckdb_columns()` 能看到表与列；表清单**在 ATTACH 时定型**；只列**当前账号自己的**表（账号没表时目录看着是空的） |
| 表函数 | ✅ `oracle_query('<secret>', '<Oracle SQL>')`（可读任意有权访问的对象，含 SYS 视图） |
| 只读 | ❌ **不支持 `READ_ONLY`**（扩展原话：*Oracle ATTACH does not accept option 'read_only' yet*）→ 引擎侧那道写保护**对它不成立** |
| 写 / DDL | `oracle_execute` **只接受 INSERT / UPDATE / DELETE**；DDL 要走 `oracle_call_auto(secret, 'DBMS_UTILITY.EXEC_DDL_STATEMENT', ['…'])`（探针靠它建/删自己的表） |
| 并行扫描 | `oracle_scan_parallel(secret, '<表名>', '<分片键列>', shards := N)`：**需要 `SYS.DBMS_FLASHBACK` 的 EXECUTE + 表上的 FLASHBACK 权限**——普通账号跑不了，默认路径用 `oracle_query` / 目录 |
| 过滤下推 | ⚠️ **过滤不推**（列投影会推）：`EXPLAIN` 里 `FILTER` 在扫描节点之上，要快得把谓词写进 Oracle SQL |
| 类型映射 | ⚠️ 无精度 `NUMBER` → **VARCHAR**；`NUMBER(10,2)` → `DECIMAL(10,2)`——跨源比较容易踩（`id = '2'` 才是对的） |

**对设计的影响**（落点见 §9）：L2 源的挂载与 L1 **不完全同形**——① 目标名是 Secret（引擎不拿明文口令，**Secret 在会话里建，不落盘**）；
② 不能带 `READ_ONLY`，写拒绝要靠编辑器闸门 + 只读账号 + **引擎侧会话层那一道**（T3.2 已落：`FederatedSession::write_refusal`）；
③ 限定名是**两段**（`<别名>.<表>`），提示语要按源类型区分（T3.2 已落：`federation_notice` 按 `kind.needs_secret()` 分支）。

## 3. 与邻居的关系

| 邻居 | 关系 |
| --- | --- |
| `duckdb/accel.rs`（加速档） | **同族**：accel 是“一源一条专用连接”（整库 `ATTACH … (READ_ONLY)` + `USE`）；联邦是它的推广（同一条连接挂多源）。`MOUNT_LOCK`、旁路回执（`SourceNote`）、`USE` 解析表名、写保护双保险**全部照用** |
| `duckdb/extensions.rs` | L2 用它的安装与状态；联邦只负责“什么时候装、装给谁、失败了怎么说” |
| `duckdb/analysis.rs` · `temp_table.rs` | L3 的临时表生命周期与登记（建 / 登记 / 用完即删） |
| `editor/src/channel.rs` | 三档互斥里的 `Federated` 档**已存在**；门控端口在 `services/editor_channels.rs`（现在是“尚未注册外部源”） |
| `engine/persistence/history_store.rs` | 历史已有 `channel` 字段；联邦执行要带**参与源清单**（见 D7） |
| 1c 分析会话 | **连接的主人是它**：联邦只是那条连接上的挂载状态，临时对象因此共享（见 D2） |

## 4. 关键设计决策

| # | 决策 | 理由 | 代价 / 取舍 |
| --- | --- | --- | --- |
| D1 | **一律只读**：外部源只 `ATTACH … (READ_ONLY)` | 跨源写的语义（哪个源、什么事务边界）没定义，开放就是埋雷 | 用户想“联邦结果落回源库”时要另走导出 / 复制（显式动作）；**L2 源没有这个选项**（真机：Oracle 不接受 `READ_ONLY`）——靠会话层拒绝 + 编辑器闸门 + 只读账号（见 D14） |
| D2 | **联邦不是独立连接**，是分析会话那条连接上的挂载状态 | 一条连接一套 `USE` / 临时表 / 会话变量；另起连接会让 1c 的单元看不到联邦建的表，还要再写一套共享机制 | 会话归属要与 1c 一起设计（1c 未开工，第一期先按“每文档一条”落地，留好切换点） |
| D3 | **主源语义**：未限定名只在主源解析，跨源必须限定 | 否则同名表（两个库都有 `orders`）会解析成“随便一个”，出的是**静默发错** | 用户要记住“主源”这件事——用界面显示 + 同名表报错来兜 |
| D4 | **扩展显式管理 + 离线预置** | 实测：`allow_community_extensions` / `autoinstall_known_extensions` / `autoload_known_extensions` **默认全为 `true`**——SQL 里一出现扩展函数名就**静默联网下载**；企业内网会表现为“莫名卡住” | 我们要自己写“装/查/失败原因”的最小流程；换来的是可控与可离线 |
| D5 | **资源边界进会话**：`memory_limit` / `temp_directory`（指向 `RDS_HOME`）/ `max_temp_directory_size` | 跨源 join 极易把内存打爆、把系统盘写满（桌面应用最痛的两种失败） | 大查询可能提前报“超限”——比 OOM 崩掉强，且原因可读 |
| D6 | **部分可用 + 错误归属**：挂不上的源保留并带原因；运行期错误带源名 | 一个源挂不上就整个联邦不可用，是把局部故障放大成全局故障 | 界面要区分“源的错误”与“你的 SQL 的错误”（原型 §5） |
| D7 | **L3 必须条件下推 + 行数上限** | “拉数”如果全表搬，网络与内存都是灾难；`pushdown_predicates` 台账实测可用 | 推不下去的要说清楚（“将全表拉取”），不静默 |
| D8 | **一致性如实声明**：跨源查询标“非事务一致快照”；桥接数据标“拉取于 HH:MM” | 各源各自时刻的数据，说成“一致”就是骗人 | 一段文案与一个悬停提示的成本 |
| D9 | **不新建 crate**，在 `engine/duckdb/` 下独立成目录 | 无独立状态与生命周期、使用方只有 editor（经 engine）→ 不满足建 crate 判定 | 目录边界要靠文档与 mod.rs 的硬约束维持 |
| D10 | **L2 未真机验收前不算可用** | 装得上 ≠ 连得上、下推好不好（我们没有 Oracle / MSSQL 端点） | 代码可以先写，验收口径写死“待真机” |
| D11 | **凭据随连接串进 `ATTACH`，出引擎前脱敏** | 真机实测（`tests/federation_credentials_probe.rs`）：DuckDB 1.5.5 的 **mysql / postgres 扫描器都不认 Secret**——会话级 / 持久化 / 带 scope / `ATTACH ''` 全试过；脱敏 URL（`user:******@`）只会认证失败。所以挂载必须用**运行时连接串**（`DriverConnectionConfig.url_override`） | 口令会随 `ATTACH` 进 DuckDB 内存与报错文本：引擎侧一律过 `accel::scrub_credentials`（错误 / 挂载失败原因 / 历史里的原因）；连接对话框文案也跟着改成实话 |
| D12 | **会话按源清单指纹缓存** | 同一个连接上的多份文档共享一条联邦会话（临时对象也共享）；**换主源不重建**（只是 `USE`），源清单变了才重建 | 改参与源 = 丢本地临时对象（日志里写一条，界面靠源清单告知）；两期后可优化成增量 `DETACH`/`ATTACH` |
| D15 | **源登记复用 `use_duckdb_fed`，不另开一列**（原 T1.2 计划是加一列「用作联邦源」） | 实现时确认：这个开关的语义就是“**允许 DuckDB 直连本连接**”（对话框文案原本就写着“联邦查询直连源库”），且**项目侧表里也有这一列**；再开一个近乎同义的开关只会让人猜“两个开关差在哪” | 开关同时控制本地加速与联邦源参与；文案要同时说清两件事（已改）。若将来真需要分开，再加列不迟 |
| D16 | **源从「连接记录」组装，不要求已连接** | 源是 DuckDB 自己 `ATTACH` 的（不需要应用先建连）；这正是**没有原生驱动的库**（Oracle 这类）能参与的唯一入口——应用连不上它，但 DuckDB 的 scanner 能 | 组装时现拼连接串（解密口令）；已建连的（含项目作用域 `P_`）作为第二来源补充，同一个 conn_id 只算一次 |
| D13 | **联邦档要求 ≥ 2 个源** | 联邦与本地加速的区别就是“跨源”；只有一个源时两者是同一件事，摆两个入口只会让人猜 | 门控与执行路径同一口径，各自行尾把“还差哪个源”说出来（未连上 / 未开开关 / 驱动不支持） |
| D14 | **L2 源走专用挂载路径**（与 L1 不同形） | 真机验收（§2.1）：Oracle 的凭据**只能走 Secret**（`ATTACH '<secret>'`），且**不支持 `READ_ONLY`**，限定名是**两段**（`<别名>.<表>`） | 会话里建**会话级** Secret（不落盘，`DETACH` 重挂时重建）；写拒绝：引擎侧会话层（`write_refusal`，判据保守）+ 编辑器闸门 + 只读账号；提示语按源类型给（L2 两段名）；**L2 只做联邦源、不做本地加速**（`AccelKind::local_accel_support` 直接说原因） |

## 5. 表名解析与写作规范

- 未限定名（`SELECT * FROM orders`）**只在主源**解析；
- 跨源一律 `<别名>.<schema>.<表>`（`mysql_src.orders_db.orders JOIN oracle_prod.SALES.ORDERS`）；
- 两源同名表时未限定名**报错**，提示写限定名；
- DuckDB 侧 `USE` 只切 catalog：主源切换就是 `USE <别名>`；schema 搜索路径的跨源行为
  **待验证**（§8 #8）。

这套规则要出现在三处：用户文档、错误提示、以及源清单面板的“主源：X”显示。

## 6. 资源与生命周期

| 对象 | 生命周期 | 说明 |
| --- | --- | --- |
| 联邦会话 | 跟文档 / 分析会话（D2） | 复用 `accel` 的按源缓存思路；`MOUNT_LOCK` 保证 `INSTALL`/`LOAD` 不撞 |
| 源的挂载 | 会话内，按源可重挂 | 重挂 = `DETACH` + `ATTACH`（刷新表清单；与加速档“重新挂载源库”同语义） |
| L3 临时表 | 会话内，`analysis.rs` 登记 / 惰性清理 | 会话结束即消失；要持久就走显式“导入到分析库” |
| 取消 | 查询级 | DuckDB `InterruptHandle` + 各源驱动 `cancel`（B3 已有）——只断一边就是假象 |

## 7. 失败与降级矩阵

| 触发 | 行为 | 用户可见性 |
| --- | --- | --- |
| 某源挂载失败 | 其余源照常可用 | 源清单里该源 `失败 + 原话` |
| 扩展未装 / 装失败 | 该源不可用（不自动联网装） | 门控行尾给原因（与加速档同款） |
| 查询用到不可用的源 | 报错并点名源 | `源 Oracle·prod 不可用：<原因>` |
| 跨源 join 超内存 | 报资源超限（D5） | 错误卡片 + 建议（加筛选 / 减少源 / 提高上限） |
| L3 下推不了 | 全表拉取 | 明确提示“筛选未下推，将拉取全表（上限 N 行）” |
| 结果被上限截断 | 标注截断 | “已拉 N 行 · 结果可能不完整” |
| 取消 | DuckDB + 源库都停 | 状态栏“已中断”（若源库侧停不了，如实说） |

## 8. 已知问题与后续项（**权威清单**）

| # | 级别 | 问题 | 影响 | 建议 |
| --- | --- | --- | --- | --- |
| 1 | ✅ | **L2 的社区 scanner 是否支持 `READ_ONLY` 未验** → **已验（2026-09-18）：Oracle 不支持**（`oracle_scanner` 0.2.2 原话：*does not accept option 'read_only' yet*；MSSQL 待验） | 只读第二道防线（引擎侧）对 L2 不生效 | **已实现（T3.2）**：会话层 `write_refusal`（写语句 + 提到 L2 别名或其为主源就拒）+ 编辑器闸门 + 只读账号；真机验过（`oracle_federation.rs`） |
| 2 | ✅ | **`oracle_scanner` 的用法形态未验** → **已验（2026-09-18）**：两条路都通——`ATTACH '<secret>' AS x (TYPE oracle_scanner)`（表挂 `main`，**两段名**）与表函数 `oracle_query('<secret>', '<sql>')`；凭据**只能走 Secret**（会话级，不落盘）；服务名写成 `XEPDB1` | 接线方式影响 `session.rs` 的形状 | **已实现（T3.2）**：`registry::oracle_secret_from_url`（URL → 会话级 Secret）+ `attach_source` 走 Secret 分支（不带 `READ_ONLY`）+ 扩展从 community 仓库装 |
| 3 | 🟡 | **跨源查询的扫描量不可见**（DuckDB 侧怎么拿：`EXPLAIN ANALYZE` 还是 `query_progress`） | “这次查询读了 5000 万行”这种事用户看不到 | 第一期先做资源上限，扫描量随第二期一起给 |
| 4 | 🟡 | **跨源无快照一致**（D8） | 结果可能是各源不同时刻的混合 | 界面如实声明；要一致就得物化（L3） |
| 5 | 🟡 | **扩展版本与内核版本绑定**：扩展落在 `<目录>/v<内核版本>/`，升 DuckDB 要重下 | 离线预置包要跟内核版本走 | 预置脚本与内核升级流程绑在一起（`tools/`） |
| 6 | ⚪ | **L3 下推的边界**：`pushdown_predicates` 台账实测可用，但 `unnest_subqueries` 有“语义不等价”的保留 | 复杂子查询的下推要保守 | 只对“能证明等价”的谓词下推（与 `oracle_filter_pushdown` 同一原则） |
| 7 | ⚪ | **`USE` 的跨源 schema 搜索路径行为未验** | 未限定名规则（D3）的实际表现 | 真机验收时覆盖 |
| 8 | ⚪ | **取消对 scanner 的传播深度未验** | 源库里可能还在跑 | 验收用例：联邦档跑慢查询 → 中断 → 看源库侧是否停 |
| 9 | ⚪ | **与 1c 分析会话的连接归属**（D2） | 影响临时对象共享 | 1c 开工时一并定，第一期留切换点 |
| 10 | ⚪ | **旧 `legacy.rs` 的退役** | 两套并存易误用 | `session.rs` 覆盖四类源与物化后一并退役（标“已被取代”不静默删） |
| 11 | 🟡 | **扫描器不认 Secret**（D11 的实测）：凭据只能随 `ATTACH` 串进 DuckDB | 企业内网可能不接受“口令进内存”（虽然不落库、不进日志） | 界面与文档如实说明；若将来扫描器支持 Secret，再改成 Secret 优先（探针已留台账） |
| 12 | 🟡 | **SSH / 代理后面的源**：DuckDB 自己发起到源库的连接，走不到应用内的隧道 | 这类连接当下只能走源库档（或将来 L3 桥接：应用侧拉数） | 门控/源清单里如实报“挂不上：连接超时”；L3 桥接是它真正的归宿 |
| 13 | ⚪ | **项目作用域的标记源靠运行态补充**（D16）：连接记录的全局表里没有 `P_`，所以 `P_` 连接要先在应用里建连才进得了源清单 | 项目专属连接（项目本地 sqlite 等）多一步 | 要彻底解决得把「当前项目根」交给执行器；第一期先靠运行态兜住 |
| 14 | ⚪ | **“未限定名只在主源解析”对 L1 的实际含义是「主源 catalog + 默认 schema」**：DuckDB 文件源（`main`）不写限定名能用，而 SQLite / MySQL 源的 schema 不是默认那个（真机：`SELECT … FROM blob` 报 `Table with name blob does not exist`，`sqlite_src.main.blob` 才行） | 用户可能以为“主源里所有表都能不写限定名” | 结果区那行小字与源清单底部提示都写“跨源请写 `别名.schema.表`”；L2 两段名的差异另见 §2.1 |

## 9. 实现位置映射（设计决策 → 代码）

| 决策 | 落点 |
| --- | --- |
| 模块入口与硬约束 | `crates/engine/src/duckdb/federation/mod.rs` |
| D1 只读 / D3 主源 / D5 资源 / D6 部分可用 | `.../federation/session.rs`（✅ 第一期） |
| D3 别名与重名检测 / 源清单快照 | `.../federation/registry.rs`（✅ 第一期） |
| D12 会话缓存 / 指纹 / 按源刷新 | `.../federation/session.rs`（✅ `ensure_session` / `refresh_all` / `set_primary`） |
| D11 凭据与脱敏 | `crates/engine/src/duckdb/accel.rs`（`AccelSource::new` / `scrub_credentials`）+ `crates/engine/tests/federation_credentials_probe.rs` |
| D14 L2 挂载差异（Secret / 无只读 / 两段名） | `.../federation/registry.rs`（`SourceSecret` + `oracle_secret_from_url`）+ `.../federation/session.rs`（`ensure_secret` / `attach_source` / `l2_write_refusal`，✅ T3.2）+ 扩展仓库选择 `accel::install_sql` + 真机验收 `crates/workbench/tests/oracle_federation.rs`（✅） |
| D13 门控口径 | `crates/workbench/src/services/editor_channels.rs`（`federated_availability`） |
| D15 源登记（复用开关） | `crates/workbench/src/components/connection_dialog/render.rs`（「DuckDB 直连（本地加速 / 用作联邦源）」分组） |
| D16 从记录组装源 | `crates/workbench/src/services/editor_exec.rs`（`federated_plan` / `plan_from_records`）+ `accel::normalize_scheme`（驱动 id → 扫描器 scheme） |
| 源清单（数据 / 菜单模型 / 视图 / 动作） | `crates/editor/src/sources.rs` + `editor/view/host.rs`（`render_sources_picker`）+ `workbench/src/services/editor_sources.rs` + `editor/src/execution.rs`（`SourceAction` / `SourceNote`） |
| 联邦档执行路径 / 源清单组装 | `crates/workbench/src/services/editor_exec.rs`（`federated_plan` / `run_on_federation`） |
| 历史带参与源 | `crates/engine/src/persistence/history_store.rs`（`sources`）+ `crates/editor/src/history.rs`（`sources_text`） |
| D7 下推与上限 / L3 | `.../federation/bridge.rs`（🟡 第二期） |
| D4 扩展显式管理 | `crates/engine/src/duckdb/extensions.rs` + 探针 `tests/duckdb_extensions_probe.rs` |
| 复用对象（会话 / 回执 / `MOUNT_LOCK`） | `crates/engine/src/duckdb/accel.rs` |
| Secret 目录对齐（注册与 `secret_directory` 同一处） | `crates/engine/src/migration/global_init.rs`（`get_secrets_dir`）+ `DuckDBManager::configure_connection` |
| 临时表生命周期 | `crates/engine/src/duckdb/{analysis.rs, temp_table.rs}` |
| 通道档位与门控 | `crates/editor/src/channel.rs` + `crates/workbench/src/services/editor_channels.rs` |
| 旧实现（待退役） | `.../federation/legacy.rs` |
