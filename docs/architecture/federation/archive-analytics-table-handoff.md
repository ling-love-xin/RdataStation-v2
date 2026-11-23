# 存档模块待修正：缺「本体 = 项目 analytics.duckdb 持久表」这一档

> **状态（2026-11-23 复核）：本文所列缺口已实现，本文降级为历史记录。**
> 不要按下面的「改动清单 A–H」重新做一遍——照着做的结果会是重复命名、重复加列。
> 现行实现位置见文末「状态复核」；唯一残留债是 `kind` 列的 CHECK 收口。

> 结论先说（2026-11-16 撰写时）：**当时存档模块的三档（`file` / `analysis` / `table_ref`）都不覆盖「本体是项目
> `analytics.duckdb` 里的一张持久表」**，所以「临时表 / 临时结果集升级成资产」这个动作
> 在登记层无处落。引擎侧搬表的能力是齐的，缺的是存档侧的本体类型与登记字段。

本文只列**事实与改动点**，不做实现；所有代码位置均为 2026-11-16 实测行号。

---

## 一、现状事实（代码为准）

### 1. 本体种类只有三种

- `crates/analytics_resource/src/model/kind.rs:40-48`：`ArchiveKind = File | Analysis | TableRef`。
- `kind.rs:68-74` 落库字符串：`"file"` / `"analysis"` / `"table_ref"`。
- `kind.rs:80-86` `from_db_str`：**未知值回退 `File`**（刻意如此：新增种类时旧版本读新行不整表失败）。
- `kind.rs:93-98` `strength`：`File | Analysis => Strong`，`TableRef => Weak`；注释 `kind.rs:90-92` 明确写过「`Analysis` 与 `File` 同级（都是内容冻结的受管文件）」，且**定义从不执行、数据也不会重建**。

### 2. `analysis` 档的本体是「数据文件」，不是分析库表

- `kind.rs:44-45` 原文：分析表「本体同样是**受管文件快照**，额外登记定义 / 列结构指纹；定义不执行、不重建数据」。
- `crates/analytics_resource/src/resource_host/mod.rs:159-164` `archive_kind_for`：扩展名能被 `engine::duckdb::file_reader_function` 认出（CSV / Parquet / Excel / JSON）→ 判 `Analysis`，否则 `File`。**判定依据是文件扩展名**，与「本体在哪」无关。

### 3. 登记表里没有「表名」这一概念位

- `crates/analytics_resource/src/store/rows.rs:7-45` `AnalyticsResource` 相关列：`kind`、`file_rel_path`（本体在 `resources/` 下的相对路径）、`source_table`（`schema.table` 字符串，引用型用）、`version`、`config`(JSON)、`row_count` / `column_count` / `file_size`。
- 没有一列专门表示「本体是项目分析库里哪张表」。

### 4. 取数分派：两种本体都在按「文件」读

- `crates/analytics_resource/src/resource_host/detail_actions.rs`：
  - 打开（双击）分支：`Analysis` 走 `SampleSource::duckdb_file(&path, …)`（按文件路径读）。
  - 查看统计分支：`File` 走 `duckdb_file(path)`；`TableRef` 走 `insight_sample_sql(conn_id, parts)`；`Analysis` 同 `File`（2026-11-16 已把原先那句「本体是 analytics.duckdb、随后续批次接」的错注释改为事实）。

### 5. 引擎侧搬表能力已齐（不用新写）

- `crates/engine/src/duckdb/temp_table/promote.rs:136-140` `persistent_table_name(desc)` → `res_<描述>_<YYYYMMDDHHMMSS>`，**每落一次一张新表**（天然一版一表）。
- 同文件 `:142-150` 注释给出硬约束：`tmp_q_` 在进程内存单例，项目 `analytics.duckdb` 运行期被按次开关握着，**两边不能同时 ATTACH**（实测见 `crates/engine/tests/analysis_db_exclusive_probe.rs`），所以只能**搬运**，跨实例用 Parquet 中转（类型不丢，不装箱）。
- `write_table_across_connections`（源表 → 目标表，返回行数）、`promote_with_conn` + `PromoteMode` 均已存在。

### 6. 结论（缺口一句话）

> 「本体是 `analytics.duckdb` 里的持久表」这类资产**没有类型位**，因此
> 「临时表 / 临时结果集 → 升级成资产」在存档侧只能被迫退化成 `file`（导出文件）或
> `table_ref`（只记指向），两者都与目标口径不符。

---

## 二、目标口径（用户已拍板，2026-11-16）

1. **只有临时表与临时结果集可以升级成资产**；升级入口在**数据导航栏 / 草稿箱**等既有入口，**不在「数据对象」面板**，也不在结果集分析对话框。
2. **资产对象只能是数据对象**；连接、账号这类信息不能成为资产。
3. 升级 = 把数据写成**项目 `analytics.duckdb` 里的持久表**（不是导出成文件再当资产）。
4. 资产**只读**、**名字不带前缀**、**有版本**；要用旧版本数据**在资产管理处回退**；参与分析**只取最新版本**。
5. 分析 / 联邦**不许丢数据**；**只有洞察**可采样（`LIMIT 500` → `tmp_i_`）。
6. 顺带：`res_` 前缀只是分析库内部标识（结果集落库），不是资产命名。

---

## 三、改动清单（存档模块内）

| # | 改点 | 位置 | 要点 |
|---|---|---|---|
| A | 新增本体类型 | `model/kind.rs` | 建议 `ArchiveKind::AnalyticsTable`，落库字符串 `analytics_table`；`from_db_str` 里显式匹配，未知仍回退 `File`；`strength` 归 `Strong`（本体冻结在本项目内）。 |
| B | 旧档改名，避免撞名 | `detail_view/mod.rs:57`、`present/*`、`model/mod.rs:25,52` | 现有 `Analysis` 对外标签是「分析表」；新档若叫「分析库表」会混淆。建议 `Analysis` 改称「数据文件」，新档称「分析库表」。**不改历史数据的含义**。 |
| C | 登记「表名」 | `store/rows.rs` + `store/resource/*` | 三选一：① 复用 `source_table` 存裸表名（语义漂移，`TableRef` 已用它存 `schema.table`，不建议）；② 写进 `config` JSON（如 `{"analytics_table":"res_…"}`，不动 schema，但需读写约定与校验）；③ 新增一列（要迁移 020/023 之后一版）。**建议 ②**，代价最小、可回退。 |
| D | 版本与回退 | 复用 `persistent_table_name` | 一版一张表（`res_<name>_<stamp>`）；资产行 `version = N` 对应某张表；「回退」= 把「当前版本」指向另一张**已存在的**表，**不删数据**。 |
| E | 升级动作 | `resource_host/*` + 引擎 `promote_with_conn` | `tmp_q_` / `tmp_i_` 或源连接里的表 → 项目 `analytics.duckdb` 新表；返回新表名 → 登记成新档。命名走引擎，不在 UI 拼表名。 |
| F | 取数分派 | `resource_host/detail_actions.rs` | 新档走 `insight::SampleSource::on_duckdb(SELECT * FROM "<表名>")`（与洞察「分析表」入口同口径）；`File` / `Analysis` 保持 `duckdb_file`；`TableRef` 不变。 |
| G | 归档判定不再按扩展名一刀切 | `resource_host/mod.rs:159-164` | 现在按扩展名把「能读的数据文件」判成 `Analysis`；新档加入后，判定要改成「本体来源」：来自临时表/分析库表 → 新档；来自文件路径 → `File` / `Analysis`。 |
| H | 联动处（改 A 必同步） | 见下 | 少改一处就会出现「筛不出来 / 数不对 / 标签空白」。 |

H 的具体清单（都在 `crates/analytics_resource/src`）：

- `filter/*`（按种类筛选与计数）
- `present/snapshot.rs:47`（`ArchiveKind::Analysis => counts.analysis += 1` 这类分支）
- `present/format.rs:84`、`present/detail.rs:36,99`（尾部与详情按种类取字段）
- `detail_view/mod.rs:57`（`kind_label`）、`detail_view/rows.rs:129`（状态 × 种类 的说明文案）
- `model/mod.rs:25,52`（种类全集与 strength 断言）
- `service/mod.rs:237`（种类 → 字符串映射）
- `dnd.rs` / `policy` 一带若按种类分支，也要过一遍

## 四、验收清单（可直接核对）

1. 对 `tmp_q_订单` 升级后：项目 `analytics.duckdb` 里多一张 `res_订单_<时间戳>` 表；资产行 `kind = analytics_table`、`version = 1`、本体记的是**表名**；展示名**不含前缀**。
2. 在洞察里对这条资产取数成功（走 `on_duckdb`，不按文件路径）。
3. 再升级一次 → `version = 2`、分析库里多一张新表；回退到 v1 后仍能取到 v1 的数据（两张表都在，没被覆盖或删除）。
4. 连接、账号不会出现在资产列表（资产对象只能是数据对象）。
5. 资产不可编辑、不可在分析侧删除；删除只走资产模块回收站。
6. 旧 `analysis` 行（数据文件型）行为不变：仍按文件读、标签改为「数据文件」但语义不动。
7. 结果集分析 / 联邦无行数上限；洞察有采样行数文案。

## 五、已知约束与坑（别踩）

- **不能 ATTACH**：临时表在进程内存单例、项目分析库被按次开关，两边同时开会撞「另一个程序正在使用此文件」（实测 `analysis_db_exclusive_probe`）；只能搬，中转用 Parquet。
- **`from_db_str` 未知值回退 `File`**：如果新档字符串写错（例如写成 `analytics-table`），旧行会静默变成文件型——上线前用一条真库行核对。
- **`strength` 只有两档**：新档归 `Strong` 即可，不要再造中档（`kind.rs:90-92` 已经因为「承诺做不到的事」撤过一次中档）。
- **`res_` 前缀是引擎内部标识**：不要拿它当展示名，也不要在资产侧再拼一次前缀。
- **旧的 `analysis` 行不能改语义**：改语义等于让历史数据换含义，只能新增档。

---

## 六、状态复核（2026-11-23）

逐条核对代码后的结论：**A–H 全部已实现**，且「升级成资产」的登记闭环也在 2026-11-23 补齐。

| 条目 | 实现位置（代码为准） |
|---|---|
| A 本体类型 | `model/kind.rs`：`ArchiveKind::{File, AnalyticsTable, TableRef}`；`label` = 「文件 / 分析库表 / 引用」；`resolve(kind, analytics_table)` 是权威判据 |
| B 旧档改名 | 旧 `Analysis` 变体已直接撤掉（不再是"改名"，而是只保留「文件」与「分析库表」两档），不存在撞名 |
| C 登记表名 | 迁移 031 加 `analytics_table` 列 + 局部索引 `idx_ar_analytics_table`；全局侧 034；读取走 `store/rows.rs` 的 `analytics_table` 字段 |
| D 版本一版一表 | `engine::duckdb::temp_table::promote::persistent_table_name`；全局 `global/promote.rs` 逐版记 `analytics_table`、历史表不删 |
| E 升级动作 | 「存为表」`components/result_set_actions.rs::persist_to_project` → `services/persist_jobs.rs`（worker 落库）→ **2026-11-23 新增** `analytics_resource::table_asset::register_at` 登记成表型资产 |
| F 取数分派 | `resource_host/detail_actions.rs`：`AnalyticsTable` 走 `SampleSource::on_duckdb`（`open_analytics_table` / 统计分支），不按文件路径 |
| G 归档判定 | 迁移 031 引入 `structured` 列（「能不能被 DuckDB 读成表」是能力事实），档位由本体来源决定，不再靠扩展名二次猜 |
| H 联动 | `filter/*`、`present/{snapshot,format,detail,rows,trash}.rs`、`detail_view/{mod,rows,body_sections}.rs`、`model/mod.rs`、`view_model/{row,detail}.rs`、`resource_view/{row_meta,tests}.rs`、`dnd.rs` 均已按 `ArchiveKind::resolve` 分派 |

**2026-11-23 补的那一段**（`b8c60421`）：`persist_jobs` 建完表只提示、不登记，导致「库里有表、
资产列表里没有」；现在同一 worker 线程上顺手调用 `table_asset::register` —— 首次 v1、
同名再升级 v2（旧表名进版本快照，回退 = 指回去），显示名用结果集展示名（不带 `res_` 前缀）。
单测在 `crates/analytics_resource/src/tests/table_asset.rs`，端到端在
`crates/workbench/tests/persist_result_table.rs`。

**唯一残留债**：`kind` 列的 CHECK 仍只认 `file` / `analysis` / `table_ref`，所以
`db_write_str` 把表型折回 `analysis`、读取靠 `resolve` 还原。迁移 031 的注释写清了为什么推迟
（迁移执行器整包事务、表被 5 处外键引用）——要收口得安排一个事务外的重建窗口，不是顺手能做的事。
