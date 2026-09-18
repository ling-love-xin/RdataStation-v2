# 联邦查询模块 · 模块入口

> 本文只提炼**特点 / 边界 / 代码地图 / 硬约束**；细节一律指向本目录内文档，**不复制设计**。
> 状态：**第一期已能真机使用（2026-09-18）**——`registry` / `session` 已落地，执行路径（联邦档）
> 与门控已接；源清单浮层（`源清单 ▾`）与「用作联邦源」标记/存储待做。
> 进度与验收见 `federation-dev-plan.md`；凭据与 Secret 的实测台账见架构 §8。

## 1. 模块特点

### 产品行为

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **只读是硬边界** | 外部源一律 `ATTACH … (READ_ONLY)`；本地临时对象（分析用的“变量”）照常允许 | 架构 D1 |
| **联邦 = 一条连接上的“挂载状态”** | 不是独立连接：连接的主人是分析会话（1c），临时对象因此天然共享 | 架构 D2 |
| **主源语义** | 未限定表名**只在主源解析**；跨源写 `<别名>.<schema>.<表>`；两源同名时报错不猜 | 架构 D3、原型 §4 |
| **三层策略** | L1 官方 scanner（mysql/pg/sqlite）→ L2 社区 scanner（mssql/oracle_scanner…）→ L3 桥接兜底（拉数 → 临时表） | 架构 §2 |
| **扩展显式管理** | 关掉 DuckDB 的自动安装/自动加载（实测默认全开，会静默联网）；装到应用目录、可离线预置 | 架构 D4 |
| **凭据随连接串进 `ATTACH`** | 扫描器**不认** Secret（真机实测）；口令只在内存里传，出引擎前一律脱敏（`accel::scrub_credentials`） | 架构 D11 |
| **会话按源清单缓存** | 同一个连接上的文档共享一条联邦会话；换主源不重建，源清单变了才重建 | 架构 D12 |
| **状态如实** | 挂不上的源留在清单里并带原因；一致性（非事务快照）与代价（扫描 / 拉取行数）都能看见 | 架构 D8、原型 §5 |
| **部分可用好过整体失败** | 一个源挂不上，其余源照常可用；查询用到它时给带源名的可读错误 | 架构 D6 |

### 工程与文档

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **不新建 crate** | 无独立状态与生命周期、使用方只有 editor（经 engine）→ 在 `engine/duckdb/` 下独立成目录 | 架构 D9 |
| **复用而不是另起** | 会话基建复用 `accel.rs`（`MOUNT_LOCK` / 旁路回执 / 写保护双保险）；临时表复用 `analysis.rs`；扩展复用 `extensions.rs` | 架构 §3 |
| **选型有实测** | 三层选型的依据是 `crates/engine/tests/duckdb_extensions_probe.rs` 的实跑结论（不是文献） | 该探针 |
| **先文档后实现** | 本目录四份文档先行；每一期的验收口径写死在 `federation-dev-plan.md` | 本文档目录 |

## 2. 边界

**做**：多源挂载与别名 · 主源与表名解析 · 只读跨源查询（join / union / 派生表）· 按源刷新表清单 ·
源清单与状态呈现 · L3 桥接（拉数 → 临时表）。

**不做**：

- **跨源写**（写源库对象）——写语义未定义就不开放；本地临时对象不受限；
- **跨源事务 / 快照一致**——各源各自时刻的数据，界面要如实标“非事务一致”；
- **自动联网装扩展**——一律显式装、状态可见（默认值会静默下载，必须关）；
- **ODBC 通道**——DuckDB 1.5.5 没有 ODBC 扫描器；自己写桥等于重做 L3，不做；
- **ADBC**——可选层，等“两种 scanner 都没有、但有 ADBC 驱动”的库出现再接。

**第一期的一条硬口径**：联邦档要求**两个以上**已连接、开启「DuckDB 本地加速」的连接——
联邦与本地加速的区别就是“跨源”，只有一个源时两者是同一件事（门控与执行路径同一口径，
原因写在两处行尾）。

## 3. 代码地图

| 职责 | 落点（✅ = 已有，🟡 = 待做） |
| --- | --- |
| 模块入口与硬约束 | `crates/engine/src/duckdb/federation/mod.rs`（✅） |
| 旧实现（四类源 / 进程内状态） | `.../federation/legacy.rs`（✅ 迁入，标“已被取代”，待 `session.rs` 覆盖后退役） |
| 源登记与状态快照 | `.../federation/registry.rs`（✅ 别名三件套 / 挂载状态 / 会话快照） |
| 多源会话与挂载 | `.../federation/session.rs`（✅ 多源只读挂载 + 进程内会话缓存 + 按源刷新） |
| 联邦档执行路径 | `crates/workbench/src/services/editor_exec.rs`（✅ 源清单组装 + 三档分流 + 历史带参与源） |
| 联邦档门控 | `crates/workbench/src/services/editor_channels.rs`（✅ 真值；源清单浮层待做） |
| 凭据台账与脱敏 | `crates/engine/tests/federation_credentials_probe.rs`（✅）+ `accel::scrub_credentials` |
| L3 桥接（拉数 → 临时表） | `.../federation/bridge.rs`（🟡 第二期） |
| 单源加速会话（复用对象） | `crates/engine/src/duckdb/accel.rs`（✅） |
| 扩展安装与状态 | `crates/engine/src/duckdb/extensions.rs`（✅） |
| 临时表生命周期 | `crates/engine/src/duckdb/{analysis.rs, temp_table.rs}`（✅） |
| 通道档位（编辑器侧） | `crates/editor/src/channel.rs`（✅） |
| 选型实测探针 | `crates/engine/tests/duckdb_extensions_probe.rs`（✅） |

## 4. 改这个模块前必须遵守

1. **只读**：外部源只用 `READ_ONLY`；写拒绝在引擎侧与编辑器侧**各一道**（双保险）。
2. **不新造连接模型**：多源挂载是 `accel.rs` 会话模型的推广；`MOUNT_LOCK`、旁路回执、
   `USE` 解析表名都照用，不另写一套。
3. **主源唯一**：未限定名只在主源解析；同名表必须报错。
4. **扩展显式管理**：`autoinstall_known_extensions` / `autoload_known_extensions` 关掉；
   `extension_directory` 钉在应用数据目录（`SET` 已实测生效，扩展落 `<目录>/v<内核版本>/`）。
5. **资源边界**：会话设 `memory_limit` / `temp_directory` / `max_temp_directory_size`。
6. **取消要传到源库**：DuckDB `InterruptHandle` + 各源驱动 `cancel`，只断一边就是假象。
7. **凭据只在内存里传，出错就脱敏**：`ATTACH` 串**带凭据**（扫描器不认 Secret）；
   错误文本出引擎前过 `accel::scrub_credentials`；`ConnectionInfo.url`（脱敏）**不能**喂给 `ATTACH`。
8. **状态如实、命名如实**：徽标写“联邦”不写“快照”；桥接数据标“拉取于 HH:MM 的副本”。
9. **零裸值**（视图层）：颜色走主题 token、尺寸进 `ui.rs`（契约测试会拦）。
10. 注释与文档用简体中文，说明意图与取舍（不复述代码）。
11. `cargo` 命令固定 `-j 2`。

## 5. 测试与验证

```sh
# 选型实测（联网一次；装到 target/duckdb-extension-probe/，不碰 ~/.duckdb）
cargo test -p rds-engine -j 2 --test duckdb_extensions_probe -- --nocapture --test-threads=1

# 模块单测（源登记 / 会话缓存 / 别名 / 脱敏）
cargo test -p rds-engine -j 2 --lib -- federation

# 凭据路径真机探针（脱敏串为何不行 · Secret 为何不靠它 · 错误文本脱敏）
RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
  cargo test -p rds-engine -j 2 --test federation_credentials_probe -- --nocapture --test-threads=1

# 多源真机：MySQL + SQLite 同会话跨源
RDS_TEST_MYSQL_URL='…' RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil' \
  cargo test -p rds-engine -j 2 --test federation_probe -- --nocapture --test-threads=1

# 联邦档执行路径（工作台侧：组装 / 门控）
cargo test -p rds-workbench -j 2 --lib -- services::editor
```

## 6. 文档地图

| 文档 | 回答什么 |
| --- | --- |
| `README.md`（本文） | 这个模块是什么、边界在哪、代码在哪、不能碰什么 |
| `federation-prototype-design.md` | 长什么样：源清单 / 主源 / 错误呈现 / 代价与一致性怎么给用户看 |
| `federation-architecture.md` | 为什么这样设计：三层选型（含实测证据）、关键决策、已知问题权威清单 |
| `federation-dev-plan.md` | 怎么落地：分期与任务表、验收口径、进度记录 |

## 7. 下一步

第一期剩余：**「用作联邦源」标记的存储**（现在按“已连接 + 开启本地加速 + 驱动可挂”组装源清单，
见 dev-plan T1.2）+ **源清单浮层**（`源清单 ▾`：失败行带原因、按源重挂、设为主源）。
第二期（L3 桥接）任务清单见 `federation-dev-plan.md` §2。
