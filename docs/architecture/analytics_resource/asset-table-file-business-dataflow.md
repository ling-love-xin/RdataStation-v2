# 表型 / 文件型资产：业务线与数据流

状态：2026-11-26 梳理。依据当前 `feat/global-ref-solidify-backend` 代码，以及
`rds-v2-analysis` 单实例句柄探针结论（同进程一个 DuckDB 文件只能有一条根连接）。

## 1. 先统一四种介质

| 介质 | 位置 | 存什么 |
| --- | --- | --- |
| 项目元数据 | `<项目>/.RSmeta/project.db`（SQLite） | 资产行、版本快照、分组、标签、回收站登记 |
| 全局元数据 | `<RDS_HOME>/data/system/global.db`（SQLite） | 全局登记行、全局版本快照、引用 pin、分发回执 |
| 项目分析库 | `<项目>/.RSmeta/analytics.duckdb` | 项目表型资产的本体表 |
| 全局分析库 | `<RDS_HOME>/data/system/analytics.duckdb` | 全局表型资产的本体表 |
| 项目文件区 | `<项目>/.RSmeta/resources/` | 文件型资产的本体副本；历史副本在 `resources/versions/<id>/<v>/` |

下面用 `[元]` 表示 SQLite，`[项表]` 表示项目 DuckDB，`[全表]` 表示全局 DuckDB，
`[文件]` 表示 `resources/` 文件系统。

## 2. 两种“引用”不要混

| 说法 | 含义 | 落在哪 |
| --- | --- | --- |
| 文件引用模式 `payload_mode=reference` | 文件本体不复制进 `resources/`，只登记源路径 + 指纹；打开时校验源是否变了 | 项目文件型 |
| 全局引用资产 `kind=table_ref, origin_kind=global` | 项目只有一条指向全局表的资产行，不落数据；按项目版本 pin 全局版本 | 全局 → 项目 |

前者是“文件本体留原位”，后者是“项目指向全局表”。两条业务线都会出现“引用”这个词，但轴不同。

## 3. 表型资产

### 3.1 主线

```text
查询结果 / 临时表
  │  落成项目持久表 res_<描述>_<时间戳>
  ▼
[项表] 一张持久表 ───────────────┐
  │  登记 v1 + 版本快照             │
  ▼                               │
[元] 项目表型资产行                 │
  ├─ 打开 / 统计 ──► [项表] SELECT * FROM <analytics_table>
  ├─ 再保存同名 ──► [项表] 新表 + [元] v(n+1)，旧表留在库里
  ├─ 回退 vN ─────► [元] 新版本指向 vN 快照里的旧表名（不搬数据）
  └─ 升级到全局 ──► [全表] 新表 + [元] 全局登记 vN
                       ├─ 分发到 B ──► [项表] 新表 + [元] B 资产 v1
                       └─ 引用到 B ──► [元] B 的 table_ref + 全局 pin（无数据）
                                          └─ 固化 vN ──► [项表] CTAS + [元] B 独立资产 v1
```

### 3.2 逐段数据流

#### T1 生成并登记

1. 查询结果 / 临时表由持久化作业写成项目分析库里的 `res_<描述>_<时间戳>` 表。
2. `register_existing_table` 在项目 SQLite 写一行：
   - `kind` 落 `analysis`（历史 CHECK），但 `analytics_table` 非空，读取面
     `ArchiveKind::resolve` 还原为 `AnalyticsTable`；
   - `readonly=1`、`payload_mode=copy`、`analytics_table=<表名>`；
   - 同时写 v1 版本快照。
3. 介质：`[项表]` 写 + `[元]` 写。文件区不参与。

#### T2 新版本

- 同名再升级 = 再落一张新表，资产行 `version+1`，`analytics_table` 指向新表；
  旧表**不删**，旧表名写进 vN 快照。
- 项目表型当前没有按 `KeepVersions` 裁剪旧表；旧表会留在项目分析库里。
- 介质：`[项表]` 写新表 + `[元]` 写版本。

#### T3 回退

- 读 vN 快照里的表名，追加一个新版本，把 `analytics_table` 指回旧表。
- 不复制数据、不删当前表；所以回退后还能再回退回来。
- 旧表如果被手工删了，存在性预检会报 `Missing`；拿不到句柄时跳过预检。
- 介质：只动 `[元]`（正常路径）。

#### T4 打开 / 统计

- 项目表：`SampleSource::on_duckdb("SELECT * FROM <analytics_table>")`。
- 全局表：`SampleSource::on_global(<table>)`，当前走短命只读挂载；单实例收口后
  直接 `rds_global`。
- 介质：`[项表]` / `[全表]` 只读。

#### T5 升级到全局

1. 从项目分析库整表导出；当前实现因两份连接无法同时持文件，走 parquet 中转。
2. 全局库建 `res_<描述>_<时间戳>` 表（G2：一版一张表）。
3. `global.db` 写 `scope='global'` 登记行 + 版本快照 + `origin_kind='project'` /
   `origin_ref={"resource_id":...,"version":N}`。
4. 若带 `KeepVersions`，裁掉更早的全局表，但**登记和版本快照都留**；被引用 pin 的
   版本跳过。
5. 介质：`[项表]` 读 + `[全表]` 写 + `[元]` 写。

#### T6 分发到项目 B（复制）

1. 读全局登记行（当前版或指定版），定位全局实际表名。
2. 当前实现：全局连接 `COPY ... TO parquet` → 项目连接 `CREATE TABLE AS SELECT read_parquet(...)`。
3. B 的项目 SQLite 写一条独立 `AnalyticsTable` 资产 v1，`origin_kind='global'`、
   `origin_ref={"instance":...,"version":N}`。
4. `global.db` 写一条分发回执（哪个项目、哪一版）。
5. 失败时删掉刚建的项目表，避免“表在、资产不在”。
6. 介质：`[全表]` 读 + `[项表]` 写 + `[元]` 写 + 临时 parquet 文件。

> 单实例收口后：第 2 步换成同实例 `CREATE TABLE analytics.<target> AS SELECT * FROM rds_global.<table>`，
> parquet 中转整段删除。

#### T7 引用到项目 B（不复制）

1. 校验全局版本表和实际表名存在；文件型全局行拒绝（`file_rel_path` 非空）。
2. B 的项目 SQLite 写 `kind='table_ref'`、`analytics_table=NULL`、`readonly=1`、
   `origin_kind='global'`、`origin_ref={"instance":global_id,"version":N}`、
   `source_table=<全局实际表名>`。
3. `global.db` 的 `analytics_global_refs` 按
   `(project_root, project_resource_id, project_version)` 写一条 pin。
4. 同一引用资产再选一个新版本：先保存当前行快照 → 更新行 → `version+1` →
   `parent_version_id` → pin 新版本。
5. 介质：只动 `[元]`，不碰任何数据。
6. 打开：直接读 `rds_global.<source_table>`。

#### T8 固化引用版本

1. 从引用资产当前行或 vN 快照读 `origin_ref`，得到全局名 + 全局版本。
2. 读该版本的全局实际表名；读引用资产的别名、分组、标签。
3. 在 B 的项目分析库建独立表（CTAS），在 B 的 SQLite 写一条新的
   `AnalyticsTable` 资产 v1，复制别名 / 分组 / 标签，`origin_ref` 记“固化自全局 vN”。
4. 原引用资产和版本链保留；只释放 vN 这个项目版本的 pin。
5. 失败时项目表回滚，引用和 pin 原样保留。
6. 介质：`[全表]` 读 + `[项表]` 写 + `[元]` 写（项目 + 全局 pin）。

> 版本链保留口径（2026-11-26 用户确认）：引用快照和版本链不砍。固化只新增一条本地资产，
> 原引用版本仍在；建议给版本行补 `solidified_resource_id` 标记，打开已固化版本时路由到本地资产。

#### T9 删除 / 回收站

- 项目表型移入回收站：只软删登记行；回收站清单从“软删登记行”补出（registry-only）。
- 永久删除项目表型：清登记行 + 版本 + 标签 / 分组；**分析库里的表不动**。
- 删除全局资产：
  1. 先查活跃引用 pin，有则拒绝；
  2. 逐张 `DROP` 全局各版本表；
  3. 事务内清全局登记、版本快照、分发回执；
  4. 已分发到项目的表 / 文件不受影响。
- 全局历史裁剪：只删旧表，不删登记；被 pin 的版本跳过。

## 4. 文件型资产

### 4.1 主线

```text
草稿 / 外部文件
  │  copy：复制进 resources/<rel>
  │  reference：只登记源路径 + 指纹
  ▼
[文件] 受管副本（或源位置） ── 打开时校验指纹
  │  登记 v1 + 版本快照
  ▼
[元] 项目文件型资产行
  ├─ 打开 / 统计 ──► 数据文件：DuckDB file reader；文本：编辑器只读
  ├─ 取回 ─────────► [文件] 复制一份到草稿箱，本体不动
  ├─ 再归档同名 ───► [文件] 旧内容留 versions/<id>/<v>/，本体替换，v(n+1)
  ├─ 还原 vN ──────► [文件] 历史副本 → 新版本（reference 模式不允许覆盖源）
  └─ 升级到全局 ───► 数据文件：全局表；脚本：全局内容表
                       ├─ 分发到 B（数据文件）──► [项表] 表 + [元] 表型资产
                       ├─ 分发到 B（脚本）────► [文件] resources/<rel> + [元] 文件型资产
                       └─ 引用到 B ───────────► 仅数据文件/表型全局行可引用
```

### 4.2 逐段数据流

#### F1 归档

- `copy`：把源文件复制进 `<项目>/.RSmeta/resources/<rel>`，归档后只读；源文件保留为工作副本。
- `reference`：不复制，只登记源绝对路径、文件名、指纹；本体留原位。
- SQLite 写 v1：`kind='file'`、`file_rel_path`（copy）或 `source_path`（reference）、
  `content_hash`、`file_size`、`payload_mode`、`readonly=1`，同时写版本快照。
- 介质：`[文件]`（copy 写）+ `[元]` 写。

#### F2 再归档 / 新版本

1. 先把当前本体留成历史副本：
   - copy 从 `resources/<rel>` 复制到 `resources/versions/<id>/<当前版本>/`；
   - reference 从源绝对路径复制。
2. 暂存新内容 → 索引事务提交新版本（`version+1`，旧行写进快照）→ 替换本体。
3. 按 `KeepVersions` 裁剪历史内容副本；版本元数据永久保留。
4. 介质：`[文件]` 写（历史副本 + 本体）+ `[元]` 写。

#### F3 打开 / 统计

- 数据文件（CSV / Parquet / Excel / JSON 等白名单）：`SampleSource::duckdb_file(path)`，
  由 DuckDB 直接读文件，不复制。
- 文本 / 脚本：编辑器只读打开。
- `reference` 模式打开前校验源文件指纹；不一致置 `reference_stale`，不打开，
  让用户选“接受当前内容”或“从历史还原”。
- 介质：`[文件]` 读 / `[项表]` 不参与（DuckDB 读文件是临时扫描）。

#### F4 取回

- 从 `resources/` 当前本体（copy）或源位置（reference）复制一份到草稿箱目录。
- 本体不动、仍只读；取回产物是工作副本。
- 介质：`[文件]` 读 + `[文件]` 写（草稿箱）。

#### F5 还原历史版本

- 仅 copy 模式可还原：
  1. 当前内容先留成历史副本；
  2. 目标历史副本进暂存；
  3. 索引提交新版本（`Restored`）；
  4. 替换本体。
- reference 模式拒绝覆盖源；只能“取回该版本为草稿”。
- 介质：`[文件]` 读 + `[文件]` 写 + `[元]` 写。

#### F6 升级到全局

文件型升级**不保存文件本体**，统一投影成全局表：

| 项目文件形态 | 全局形态 | 建表方式 |
| --- | --- | --- |
| 数据文件（CSV / TSV / Parquet / Excel / JSON / NDJSON） | 全局表 | `CREATE TABLE AS SELECT * FROM read_xxx(file)` |
| 脚本 / 文本（≤ 16 MiB） | 全局内容表：`filename / content / content_hash` 三列单行 | 先读文本，再 `CREATE TABLE` + `INSERT` |

- 全局登记行是表型（`analytics_table` 非空）；脚本行额外保留 `file_rel_path`，
  作为分发时重建文件的目标路径。
- `global.db` 记 `origin_kind='project'` + 项目版本。
- 介质：`[文件]` 读 + `[全表]` 写 + `[元]` 写。

#### F7 分发到项目 B

按全局行形态分流：

- `file_rel_path` 非空（脚本内容表）：从全局表读 `filename/content`，在 B 的
  `resources/<rel>` 重建文件，B 写一条 **File 型**资产 v1。
- `file_rel_path` 为空（数据文件映射表 / 表型）：在 B 建项目表，B 写一条
  **AnalyticsTable** 资产 v1。
- 两条都记 `origin_kind='global'` 和分发回执。
- 介质：`[全表]` 读 + `[文件]` 写或 `[项表]` 写 + `[元]` 写。

#### F8 引用到项目 B

- 只接受“没有 `file_rel_path` 的全局表型行”：
  - 由项目表升级来的全局行：可引用；
  - 由数据文件升级来的全局行：可引用（在 B 看来就是表）；
  - 由脚本升级来的内容表：拒绝引用（文件型引用后置）。
- 引用本身不复制文件、不复制表，只写项目 `table_ref` + 全局 pin。
- 介质：只动 `[元]`。

#### F9 删除 / 回收站

- copy 模式软删除：payload 搬进项目回收站（带 manifest），登记行软删。
- reference 模式软删除：没有实体 manifest，只在登记行侧进回收站；源文件不动。
- 永久删除 copy：删回收站 manifest 对应文件 + 登记行 / 版本；reference 只清登记。
- 全局删除：只删全局表 + 全局登记，已分发到项目的文件不受影响。
- 保留策略：文件裁剪的是 `resources/versions/<id>/<v>/` 历史副本，版本元数据保留。

### 4.3 全局文件的可恢复性与版本边界（当前实现）

**先说结论：全局没有文件本体，只有表。**

| 项目侧来源 | 全局落点 | 原始文件字节还在吗 |
| --- | --- | --- |
| 数据文件（CSV / Parquet / Excel / JSON 等） | 全局表（`CREATE TABLE AS SELECT * FROM read_xxx`） | **不在**；只剩解析后的数据 |
| 文本脚本（sql / py / md / yaml 等，≤ 16 MiB） | 全局内容表 `filename / content / content_hash` 单行 | **在**；文本内容原样存 `content` |

全局目录只有 `data/system/{global.db, analytics.duckdb}`，没有 `system/resources/`；设计上明确
不采用“全局再存一份文件副本”（见 `analytics-resource-prototype-design.md` §13.5 方案 B）。

**能追溯什么**

- 全局行有 `version`、`content_hash`、`file_size`、`source_path`（项目侧绝对路径）、
  `definition_sql`（数据文件的建表 SQL / 脚本的一句描述）、`origin_kind='project'` +
  `origin_ref`（项目资源 id + 版本）。
- 全局版本快照在 `global.db` 的 `analytics_resource_versions`，与项目侧同构。
- 分发回执在 `analytics_resource_dispatch`，能看出哪一版被哪个项目取过。

**能恢复什么**

- 文本脚本：可以从内容表重新 `fs::write` 成文件，内容可恢复。
- 数据文件：只能恢复成“表 / 数据”，不能恢复原始 CSV 排版、Excel 多 sheet / 样式、JSON 空白、
  Parquet 编码与元数据。
- 版本：旧版本的全局表**默认保留**（一版一张表），`read_global_row(name, Some(version))`
  能按版本取到老行与老表；全局裁剪只删被策略裁掉的表，登记与快照保留。
- 但**没有全局版本回退 UI / 全局文件版本恢复入口**：全局面板只列活跃当前行，旧版行是
  `deleted_at` 软删状态；`delete_global` 在无活跃引用时会把同名链所有版本表一起删。

**如果要求是“全局文件也能像项目文件一样版本恢复”**，当前设计不够，需要先定恢复目标：

| 目标 | 需要补的东西 |
| --- | --- |
| 恢复成“数据表 / 文本” | 保留现有表投影；补全局版本浏览 + “以旧版为源新建一版”（表用 CTAS，脚本用内容表重建） |
| 恢复成“原始文件字节” | 给全局加 `{system}/resources/` + `resources/versions/<id>/<v>/`；全局文件资产按 File 档登记，表投影可选或作为伴随资产 |

两条路线不能混着做：一旦要求原始字节可恢复，文件升级到全局就不能只建表，必须同时保存文件副本；
否则只能承诺“数据可恢复”，不能承诺“文件可恢复”。

**这条口径的来源（避免再被零散记录带偏）**

| 记录 | 说了什么 | 性质 |
| --- | --- | --- |
| `analytics-resource-prototype-design.md` §13.5 | 脚本走内容表（方案 A 定稿）；全局 `{system}/resources/` 方案 B **不采用** | 当时的正式裁决 |
| `crates/analytics_resource/src/global/list.rs:70` | 注释写“全局行的本体在全局 `resources/` 下” | **陈旧注释**，代码里没有该目录 |
| `M6-终审问题清单-2026-11-12.md` §7 | 全局实例 ≈85%，已落地的是删除 / 来源链 / 表型判据 | 没有全局文件版本恢复 |
| `analytics_resource-产品语义补全建议-2026-11-12.md` §1.2 | 全局只搬表 / 文件 / 登记，不带语义 | 也没有文件副本 |

所以“全局文件可恢复、可追溯、可版本恢复”目前是**需求缺口**，不是已实现能力；
其中“可追溯”有一半（元数据、版本号、来源、分发回执），“可恢复”只有脚本文本和表数据，
“文件级版本恢复”为零。落地设计见 `global-file-asset-design.md`。

## 5. 表型 / 文件型对照

| 维度 | 表型 | 文件型 |
| --- | --- | --- |
| 本体位置 | 项目 / 全局 DuckDB 表 | `resources/` 受管文件，或源位置（reference） |
| 身份 | `analytics_table` 非空；内容指纹 = 表名 sha256 | `file_rel_path` / `source_path`；内容指纹 = 文件字节 sha256 |
| 新建 | 查询结果 / 临时表落成持久表后登记 | 归档复制或只登记源 |
| 新版本 | 一版一张表，旧表留库 | 旧内容留 `versions/<id>/<v>/`，本体替换 |
| 回退 | 改登记指向旧表，不动数据 | 从历史副本生成新版本（仅 copy） |
| 打开 | 洞察取样 `SELECT * FROM 表` | 数据文件走 DuckDB file reader；文本进编辑器只读 |
| 升级全局 | 整表复制成全局表 | 数据文件映射成表；脚本变内容表 |
| 全局形态 | 表 | 也是表（不保存文件本体） |
| 分发到 B | 建项目表 | 脚本重建文件；数据文件建表 |
| 全局引用 | 支持 | 仅数据文件/表型全局行支持；脚本内容表不支持 |
| 固化 | CTAS 成独立本地表 | 后置；文件引用后置 |
| 永久删除 | 只清登记，表留库 | copy 删文件；reference 只清登记 |
| 裁剪 | 全局表按 KeepVersions；项目表当前不裁 | 历史内容副本按 KeepVersions |

## 6. 单实例收口后，表型链会变成什么样

前提：`AnalysisInstance` 独占项目/全局分析库，项目库挂 RW，全局库挂 RW + 用户会话写守卫。

| 当前步骤 | 收口后 |
| --- | --- |
| 升级到全局：项目连接导出 parquet → 全局连接读 | 同实例 `CREATE TABLE rds_global... AS SELECT * FROM analytics...` |
| 分发：全局 `COPY` parquet → 项目 `read_parquet` | 同实例反向 CTAS |
| 固化：读全局表 → parquet → 项目建表 | 同实例 CTAS |
| 引用打开：短命挂载 `rds_global` | 直接 `SELECT * FROM rds_global...` |
| 删除 / 裁剪全局表：独立全局写连接 | 实例系统会话 `DROP TABLE rds_global...` |

文件型链基本不变：文件本体仍在 `resources/`，读取仍经 DuckDB file reader；
变化只在“文件升级成全局表 / 从全局表重建文件”这两步也走同一实例。

## 7. 当前尚未落地的边界

- 文件型全局引用 / 固化；
- 全局文件本体存储与全局文件版本恢复（当前只有表投影 / 内容表）；
- 全局版本浏览与回退 UI；
- 固化标记持久化（建议 `solidified_resource_id`）；
- 项目表型历史表的 `KeepVersions` 裁剪（当前只裁全局表和文件历史副本）。
