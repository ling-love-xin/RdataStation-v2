# 资产模块「文档 / 表」路线与数据线（现码反推）

状态：2026-11-27，基于 `feat/duckdb-ownership-m6-slim` 的 `399ddf10` 代码实读；只做审计，未改业务代码。
配套：首轮静态审计 `analytics-resource-audit-2026-11-27.md`；业务流旧稿 `asset-table-file-business-dataflow.md`；全局文件设计稿 `global-file-asset-design.md`。

**口径假设**：本轮把「文档」按现码的**文件型资产**理解，即项目 Copy / Reference 文件、全局脚本、全局数据文件；「表」按表型资产理解，即项目分析表、全局表、`table_ref` 引用行。
如果你说的「文档」是设计文档，或要新增一种真正的「文档型资产」，第 0～4 节的文件型路线需要整段替换，第 5～7 节的表和跨层结论仍可复用。

标记：
- **确认** = 代码直接读到；
- **推断** = 按本仓调用图推出（外部 API / REST / 脚本直接调服务层不在覆盖范围）；
- **设计态** = 设计稿写了、现码未落地。

---

## 0. 结论摘要

1. **文档主链要分开看**：项目 **Copy** 文件闭环（归档、取回、再归档、历史副本、版本还原、解锁、回收站、永久删除）；项目 **Reference** 文件只闭环到“归档 / 打开 / 取回 / 再归档 / 软删”，**升级全局与解锁都不可用**（绝对路径守卫拒绝）。归档是**复制**进 `resources/`，不是 move；源文件保留为工作副本。
2. **表主链在“登记之后”被读取面截断**。`jobs/refresh.rs::run_refresh` 只调 `store.list_file_archives()`，SQL 是 `WHERE file_rel_path IS NOT NULL`；表型行与 `table_ref` 行的 `file_rel_path` 都是 NULL，因此**不进项目面板快照**。
3. 项目表型资产、`table_ref` 引用行在资产库面板不可见；资产库的“升级到全局 / 引用固化 / 删除”动作都要求先在面板选中行，所以这些动作在 UI 上不可达（**推断**，但调用面只剩“面板行 → 详情/右键”这一条）。
4. 数据对象面板不是第二个逃生口：它从**资产面板当前可见行**取 `assets`（`assets_from_panel` 调 `view_rows()`）。表型行既不在面板里，也不会出现在数据对象面板的资产段。
5. 由此，**新环境里“项目表 → 全局纯表”这条链没有 UI 入口**。全局纯表是 `reference_to_project` 的唯一合法对象；没有纯表，引用 / 固化 / 全局表回收站整条链在 UI 上只剩旧数据或测试数据可用。
6. 全局数据文件是**双形态行**：落库 `kind=analysis`（按表分发），但带 `file_rel_path`（UI 按文件解锁、按表显示）。同一行存在三套判据打架，详见 §1、§4。
7. 全局文件设计稿要的是「文件行 + 独立派生表行」，现码是**一条混合行**；脚本文本设计稿要的是“文件本体、不建内容表”，现码仍建内容表。任务 #43 仍在进行中，不能把设计态当已实现。
8. 331 个测试全绿并不覆盖这个断点：面板测试用**手搓快照行**，没有一条“真实 store 里同时有文件行和表型行 → refresh 后都可见”的集成测试。

---

## 1. 三套判据并存

现码同时用三种方式回答“这条资产是什么”：

| 判据 | 位置 | 作用 |
| --- | --- | --- |
| 落库 `kind` | `analytics_resources.kind`：`file` / `analysis` / `table_ref` | 分发形态、全局删除守卫 |
| `ArchiveKind::resolve` | `analytics_table` 非空 → `AnalyticsTable`，否则按 `kind` | 列表徽标、详情类型、动作文案、部分服务分支 |
| 本体列是否存在 | `file_rel_path` / `payload_rel_path` / `payload_mode` | 全局危险区“文件 vs 表”、引用是否开放、分发文件/表 |

### 1.1 类型矩阵（现码行为）

| 形态 | 落库 `kind` | `analytics_table` | `file_rel_path` | `resolve` 显示 | 分发形态 | 危险区（UI） |
| --- | --- | --- | --- | --- | --- | --- |
| 项目 Copy 文件 | `file` | NULL | `resources/<rel>` | File | 按文件升级/分发 | 解锁回草稿箱（不可直接删） |
| 项目 Reference 文件 | `file` | NULL | 外部源绝对路径 | File | **不能升级全局**（绝对路径被 `resolve` 拒绝） | 移入回收站（只软删登记，源不动） |
| 项目表 | `analysis` | 表名 | NULL | AnalyticsTable | 表 | 移入回收站 / 永久删除（可见时） |
| 项目表引用 | `table_ref` | NULL | NULL | TableRef | 不复制数据 | 移入回收站 / 固化（可见时） |
| 全局脚本 | `file` | 内容表名 | 有 | **AnalyticsTable** | 文件 | 解锁回项目草稿箱 |
| 全局数据文件（现码） | `analysis` | 全局表名 | 有 | AnalyticsTable | **表** | 解锁回项目草稿箱 |
| 全局纯表 | `analysis` | 全局表名 | NULL | AnalyticsTable | 表 | 移入全局回收站 |
| 全局文件资产（设计态） | `file` | NULL | 有 | File | 文件 | 解锁/回收站待定 |

读法：
- **项目侧**按 `kind` 分文件/表；表型用 `analytics_table` 认身份。
- **全局 UI** 实际按 `payload_rel_path.is_some()` 分“文件型全局资产 / 表型全局资产”（`detail_view/actions.rs` 的 `is_file_global`）；所以**全局数据文件在 UI 走文件动作**，尽管它分发时走表分支。
- **全局列表徽标**又按 `ArchiveKind::resolve`，带 `analytics_table` 就显示“分析库表”；所以同一行会“徽标是表、危险区是文件”。
- 设计稿的口径是「`kind=file` 且不写 `analytics_table`」，现码没有做到；这是三套判据的根因。

---

## 2. 文档（文件型）路线

### 2.1 项目 Copy 归档

```text
草稿箱 / 本地文件 / 取回后的工作副本
  → ArchiveService::archive
  → sha256 + 体积
  → registration_key(mode, rel, source)
      copy: resources/<rel>
  → payload.copy_in（复制 + 设只读；源文件保留）
  → insert_archive_with_event
      project.db 行 + 时间线事件（同事务）
  → 可选 probe_analysis_facts
      只写 structured / definition_sql / 行列数，不建派生表
```

- 冲突预检只查 `find_archive_by_rel_path`，**不查显示名**；两个不同路径可以用同一个显示名。
- 写库失败会删掉刚复制出来的副本并返回错误；只读标记失败只告警，不阻断。
- `copy_in` 是 copy 语义；`archive_in` 才是 move 语义，留给历史/回滚路径。业务旧稿 F1 的“复制进 resources、源保留”是对的。
- 文件本体在 `<项目>/.RSmeta/resources/<rel>`；历史副本在 `resources/versions/<resource_id>/vN/`。
- 如果探测到表结构，`structured=1`，但**文件仍然是文件型资产**；需要表就另走“存为表”登记，不会自动生成派生表。

### 2.2 项目 Reference 归档

```text
外部源文件
  → 规范化绝对路径 = registration_key
  → 不复制，file_rel_path = source_path = 绝对源路径
  → payload_mode = reference
  → 行 + 事件
```

- 同一外部源重复引用由 `registration_key` + 局部唯一索引拦住，不再靠相对路径预检。
- 再归档时 `store_version_copy_from` 把**旧源内容**复制进 `versions/<id>/vN`；本体始终留在外部。
- 打开时懒校验源指纹；不一致置 `reference_stale`，不打开。
- 历史还原被拒绝：覆盖外部源风险不可控；出口是“取回该版本为草稿”。
- 升级到全局实际不可用：详情把绝对源路径作为 `rel_path` 传给全局 `stage_and_commit`，`PayloadStore::resolve` 明确拒绝绝对路径；UI 只检查了拼接后的源存在，没有把 `rel_path` 归一成相对路径。Reference 文件因此没有跨项目共享出口。
- 服务端 `unlock_file_to_drafts` 仍按 `PayloadStore::resolve(rel)` 解析，而 reference 的 `rel` 是绝对路径，**API 调用必失败**；UI 已按 `is_reference` 隐藏解锁按钮，所以是口径不一致而非 UI 断点。首轮审计 P1 仍成立。

### 2.3 取回 / 编辑 / 再归档

```text
资产（copy 或 reference）
  → checkout：复制一份到指定工作副本（通常在 scratchpad）
  → 用户编辑工作副本
  → archive(existing_resource_id = 当前资产)
      hash 未变：幂等返回，只补标签/分组
      hash 变了：
        store_version_copy（当前本体 → versions/<id>/<v>）
        snapshot 当前整行
        stage 新内容 → commit_new_version（version+1）
        原子替换本体 → prune 历史副本
```

- `checkout` 只复制，本体不动、只读；目标不得落在 `resources/` 内。
- 再归档不校验显示名，靠 `existing_resource_id` 锁定版本链。

### 2.4 版本还原（仅 Copy）

```text
目标 vN 副本
  → 校验副本存在、hash 与当前不同（相同则幂等）
  → 当前内容先留副本
  → 旧副本进 staging
  → commit_new_version（version+1，事件 Restored）
  → 替换本体
```

- 语义是“用旧内容生成新版本”，不原地回滚、不删历史。
- reference 不支持；表型走另一条（见 §3.2）。
- 当前 UI 只提供“恢复上一版”，后端 `restore_global_file_version` / `restore_version` 支持任意版本；项目侧任意版本 UI 仍缺。

### 2.5 解锁 / 回收站 / 永久删除

| 动作 | Copy 文件 | Reference 文件 |
| --- | --- | --- |
| 直接移入回收站 | 拒绝（文件型不能直接删） | 允许：只软删登记，源不动 |
| 解锁回草稿箱 | 复制当前本体到 `scratchpad/` → 清登记链 → 删本体与历史副本 | UI 不提供；API 因绝对路径解析失败 |
| 永久删除 | 搬本体进 `.RSmeta/trash/`，清登记/版本 | 只清登记/版本，源不动 |

**P0 遗留（首轮审计已记）**：`unlock_file_to_drafts` 用 `purge_name_chain(name, "project")` 清链。该查询按 `name + scope` 匹配，包含软删行；同名不同资产会被一并清掉，并留下孤儿版本副本。显示名不是资产身份，但解锁把它当身份用了。

---

## 3. 表路线

### 3.1 生成与登记

```text
查询结果 / 临时表 / 连接表
  → 落成项目 analytics.duckdb 持久表 res_<描述>_<时间戳>
  → table_asset::register 或 register_existing
      project.db 行：
        kind = analysis
        analytics_table = 实际表名
        file_rel_path = NULL
        payload_mode = copy（历史默认值，不代表有本体）
        readonly = 1
        content_hash = sha256("analytics_table:" + 表名)
  → 版本 v1 快照
```

真实入口：
- 结果集“存为表”：`workbench/services/persist_jobs.rs` 写表后**顺手登记**；
- 已有表“登记为资产”：`register_existing`，幂等，不涨版本；
- 连接/查询结果“归档成表”：`workbench/services/archive_jobs.rs` 写表后登记。

注意：
- 表型内容指纹是**表名 hash**，不是数据字节；对同一张表原地改数据不会被识别为“内容已变”。
- `register` 用 `latest_alive_by_name(name)` 找同名链；如果最新同名行是**文件型**，`append_table_version` 会把表名写到文件行上，留下“文件本体 + 表解析”混合行。首轮审计 P0 仍成立。

### 3.2 版本链

```text
同名再登记
  → 旧整行进 analytics_resource_versions
  → 行 version+1，analytics_table 指向新表
  → 旧表不删（一版一张表）
```

回退：
```text
vN 快照里的 analytics_table
  → 追加一个新版本，行指回旧表
  → 不复制数据、不删当前表
```

- 项目表当前没有 `KeepVersions` 裁剪；旧表会一直留在项目分析库。
- 表型删除/还原都只动登记，不动表。

### 3.3 可见性断点（本轮最硬的结论）

事实链（确认）：

1. `jobs/refresh.rs::run_refresh` 的取数：
   ```rust
   let rows = store.list_file_archives().await?;
   ```
2. `list_file_archives` 的 SQL：
   ```sql
   WHERE file_rel_path IS NOT NULL AND deleted_at IS NULL
   ```
   注释写明它只是**索引修复的比对基准**。
3. `present/snapshot.rs::build_snapshot` 只遍历传入的 `resources` 生成项目行；`global_resources` 只生成全局详情，`global_rows` 只装全局行。
4. 表型行、`table_ref` 行的 `file_rel_path` 都是 NULL → 不进 `resources` → 不进项目面板行。
5. `workbench/panels/data_objects_tab.rs::assets_from_panel` 从资产面板 `view_rows()` 取资产；表型行不在面板，数据对象面板的“资产段 / 资产表名匹配 / 升级到全局”也拿不到它。
6. 表型升级全局的唯一入口是资产详情动作 / 列表右键（`enqueue_promote`）；两者都要求先有行。

效果（推断，调用图闭合）：
- 项目表登记成功后**没有任何 UI 列表能看到它**；
- 数据对象面板也不会把它标成“资产”，不会出现“升级到全局”；
- 新环境无法产生“全局纯表”；
- 没有全局纯表，`reference_to_project` 的“引用到本项目”按钮没有可用对象；
- 没有引用行（或引用行不可见），`request_solidify_ref` 的“固化当前版本”没有可用对象；
- 全局回收站只收无 payload 的表型行，新环境也没有对象。

**旁证**：`list_alive_resources()` 仍存在，但生产代码**零调用**（只有测试）；它按 name 去重，也不能直接拿来当面板查询。面板需要的是“全部存活行、不去重”的新查询，索引修复继续用 `list_file_archives`。

**测试缺口**：`tests/panel_window.rs` 的种类计数用 `row(...)` 手搓快照；`data_objects` 的资产测试手工塞 `assets`。没有一条集成测试从真实 `project.db` 经 `run_refresh` 到面板行，所以断点没被测试抓住。

### 3.4 表 → 全局

纯表升级（`promote_table_from_project`）：

```text
项目 analytics.<table>
  → CREATE TABLE rds_global.<版本表> AS SELECT * FROM analytics.<table>
  → 全局行：
      scope=global
      kind=analysis
      analytics_table=<版本表>
      file_rel_path=NULL
      origin_kind=project
      origin_ref={"resource_id":...,"version":N}
  → 版本快照
```

- 一版一张表；旧版本表默认保留。
- `register_global` 失败时当前实现不回滚刚建的表（首轮审计 P2）。
- 当前 UI 入口不可达（§3.3）。

### 3.5 分发 / 引用 / 固化

**分发（复制）**：
```text
全局行（kind=analysis）
  → 项目 CREATE TABLE analytics.<target> AS SELECT * FROM rds_global.<source>
  → 项目新行：
      kind=analysis
      analytics_table=<target>
      origin_kind=global
      origin_ref={"instance":...,"version":N}
      version = 项目内同名 MAX(version)+1
  → 分发回执
```
- 目标重名先探测“登记 / 表 / 文件”三层；冲突走覆盖/改名。
- 建表失败或登记失败会 `drop_project_table_if_exists` 回滚新表。

**引用（不复制）**：
```text
全局纯表（无 payload）
  → 项目行：
      kind=table_ref
      payload_mode=reference
      analytics_table=NULL
      file_rel_path=NULL
      source_table=全局实际表名
      origin_kind=global
  → global.db analytics_global_refs 写 pin（按项目 resource_id + 项目 version）
```
- 同全局名已有引用行 → 保存快照 → `append_ref_version` → version+1 → pin 新版本。
- 引用打开走全局表只读取样。
- 只对“无 `file_rel_path` 的全局表”开放；带 payload 的全局数据文件在 UI 被禁用（`payload_rel_path.is_some()`）。

**固化**：
```text
引用行当前版本 / vN 快照
  → 读 origin_ref 得到全局名 + 版本
  → 在项目 CTAS 成独立表
  → 新项目表型资产 v1，复制别名/分组/标签
  → 释放该引用版本的 pin；原引用行与版本链保留
```
- 作业层 `run_solidify` 真实接到 `solidify_ref_version`；不是死代码。
- 但入口按钮挂在 `TableRef` 详情上，引用行不可见 → **按钮不可达**（推断）。
- 引用若一直无法固化/删除，pin 会长期占住；`purge_global` 因 `active_ref_count>0` 整体拒绝。当前只有“永久删除引用行（`purge_archive` / `empty_trash`）”会释放 pin，而删除入口同样要求先选中不可见的行。

### 3.6 删除 / 回收站

| 对象 | 移入回收站 | 永久删除 |
| --- | --- | --- |
| 项目表 | 软删登记，表留分析库 | 清登记/版本/标签/分组，**不 DROP 表** |
| 项目引用 | 软删登记，无本体 | 清登记；同时释放该资源全部 pin（`purge_archive` / `empty_trash`） |
| 全局纯表 | 移动（无 payload）+ manifest，软删活跃行 | 删各版本表 + 清登记/快照/回执 |
| 全局数据文件 | UI 不提供；服务层 `trash_global` 允许（`kind=analysis`） | 服务层可 purge：删表 + payload |

项目回收站清单 `list_deleted_archives` 取全部软删行，所以表型/引用**一旦被删除**会出现在回收站；但它们从未在常规面板出现过，形成“只能从回收站第一次看见”的倒挂。

---

## 4. 全局层的双形态

### 4.1 全局脚本（现码）

```text
项目文件（脚本/文本，≤16MiB）
  → 复制到 data/system/resources/<rel>
  → CREATE TABLE rds_global.<内容表> (filename, content, content_hash) + INSERT
  → 全局行 kind=file，analytics_table=内容表，file_rel_path=...
  → origin_kind=project + 版本快照
```

- 分发时 `global_kind == "file"` → 从内容表读 filename/content，在项目 `resources/<rel>` 重建文件，登记**项目 File 行**。
- `ArchiveKind::resolve` 因 `analytics_table` 非空把它显示成“分析库表”；`detail_view` 按 `payload_rel_path` 走文件动作；服务层 `trash_global` 按 `kind=="file"` 拒绝回收站。三层描述不一致但 UI 危险区仍能解锁，属于文案/徽标错位。

### 4.2 全局数据文件（现码）

```text
项目数据文件
  → 旧版本的全局文件副本
  → 当前文件 stage 到 data/system/resources/<rel>
  → CREATE TABLE rds_global.<版本表> AS SELECT * FROM read_xxx(<global file>)
  → 一条全局行：
      kind=analysis
      analytics_table=<版本表>
      file_rel_path=<全局文件相对路径>
      definition_sql=CTAS SQL
```

- 共享形态是**表**（`distribute` 按 `kind` 走表分支，不看 `file_rel_path`）。
- UI 危险区按 payload 存在判“文件型全局资产”，只给“解除锁定并退回草稿箱”，不给回收站。
- 服务层 `trash_global` 只看 `kind`，会允许这条 `analysis` 行进回收站并搬 payload；与 UI 相反。
- 引用被 `payload_rel_path.is_some()` 禁用；所以数据文件全局行既不能引用，也不能从 UI 进回收站。
- 版本恢复 `restore_global_file_version` 用旧 payload 重新 promote，生成新版本/新表，旧版本保留；UI 只给“恢复上一版文件”。

### 4.3 全局解锁 / 回收站

**解锁（UI 唯一文件出口）**：
```text
全局文件行（有 payload）
  → 检查活跃 pin（有则拒绝）
  → 当前 payload 复制到项目 scratchpad
  → 删除全局版本副本
  → purge_global：删表/登记/快照/回执
```
- 解锁后项目只得到草稿文件，不自动登记项目资产；用户要再归档。
- 数据文件也走这条，意味着“共享的表”会随解锁一起被清掉；只有无 pin 时允许。

**回收站**：
- `trash_global` 对 `kind=="file" && file_rel_path.is_some()` 硬拒绝，提示先解锁；
- 对 `kind=analysis` 的混合行允许，搬 payload + versions 到 `data/trash/global/<asset_id>/<stamp>/`，manifest 记录表名；
- `restore_global` 复活行并搬回 payload；`purge_global` 先查 `active_ref_count(name)`，有 pin 整体拒绝，然后删各版本表 + payload + 登记；
- 所以“全局回收站只收表型”在 UI 成立（因为 payload 行走解锁），在服务层不成立（混合行可进）。

### 4.4 设计态与现码的差距

| 项 | `global-file-asset-design.md` | 现码 |
| --- | --- | --- |
| 数据文件升级 | 文件行 `kind=file`（无表名）+ 独立派生表行 `kind=analysis` | **一条混合行** `kind=analysis` + 表名 + 文件 |
| 脚本升级 | 文件本体，**不建内容表**；`kind=file` 无 `analytics_table` | 仍建内容表，`analytics_table` 非空 |
| 全局文件删除 | 修订口径：文件不进回收站，解锁；基础章节又写“进回收站” | 文件拒绝回收站，解锁；混合数据文件服务层可进 |
| 文件引用 | 后置，v1 只引用表 | 带 payload 的行一律禁用引用 |
| 回收站 UI | 独立全局回收站面板（待做） | 全局面板目前只有详情危险区；全局回收站列表 UI 未做 |

任务 #43「全局文件资产与版本恢复」仍是 in_progress，以上应按“设计态”记录，不应写进用户手册当作已实现。

---

## 5. 跨层数据线

### 5.1 只允许 A → 全局 → B

`model/origin.rs` 写明不允许项目直传；每行只记**直接上游一跳**：

| 行 | `origin_kind` | `origin_ref` |
| --- | --- | --- |
| 全局行（项目 A 升级） | `project` | `{"resource_id":"arp_x","version":5}` |
| 项目行（全局分发/引用） | `global` | `{"instance":"arg_x","version":1}` |
| 项目草稿归档 | 空 | 空（来源在 `promoted_from`） |

整条链由读取侧逐跳拼，不在行里冗余全链。

### 5.2 数据线矩阵

| 起点 | 中转 | 终点 | 复制什么 | 版本落点 |
| --- | --- | --- | --- | --- |
| A Copy/Reference 文件（脚本） | 全局脚本行 | B 文件 | 全局文件本体 + 内容表重建 | A 项目版本、全局版本、B v1 |
| A Copy/Reference 文件（数据文件） | 全局混合行 | B 表 | 文件副本 + CTAS 表 | 三层各自版本 |
| A 项目表 | 全局纯表 | B 表 | CTAS | 三层各自版本 |
| A 项目表 | 全局纯表 | B 引用 | 不复制，只登记 + pin | B 引用版本 + 全局 pin |
| B 引用 | — | B 本地表（固化） | CTAS | B 新资产 v1，原引用保留 |

### 5.3 名称 / 版本 / pin 三条链

- **项目文件链**：同一 `resource_id`，`version` 递增；显示名可变（alias），不是身份。
- **项目表链**：同一 `resource_id`，`version` 递增；每版一张表，旧表留库；同名再登记按 `latest_alive_by_name` 接续。
- **全局链**：同一 `name`，每版一条新 `id`（`arg_`），`version` 递增；读取按 name+version。
- **引用 pin**：`(project_root, project_resource_id, project_version)` → 全局 name + global version；固化或永久删除引用行才释放。
- **分发回执**：`analytics_resource_dispatch` 记录哪一版被哪个项目取过；不承担版本链。

---

## 6. 断点与风险清单

### P0（表路线不可用 / 可能误删）

1. **面板取数只取文件行**：`run_refresh` → `list_file_archives` 排除表型/引用；表资产在资产库和数据对象面板全不可见。取数层把“索引修复基准”误用成“面板清单”。
2. **表 → 全局入口不可达**：`enqueue_promote` 只从详情动作/右键来；表行不可见；数据对象面板的 `assets` 又依赖面板行。新环境无法产生纯表全局资产（推断，调用面闭合）。
3. **引用生命周期不可管理**：引用创建按钮可达（全局纯表），但 `table_ref` 行不可见 → 固化/删除不可达；pin 长期占用，全局 purge 被拒。
4. **表登记可按显示名接到文件行**：`latest_alive_by_name` 命中文件行 → `append_table_version` 留下混合行。
5. **解锁按名清链**：`purge_name_chain` 连带删除同名其他资产并留孤儿版本副本。

### P1（口径矛盾 / API 与 UI 不一致）

6. **全局数据文件三种身份**：徽标=表（`resolve`）、共享=表（`kind`）、危险区=文件（payload）；服务层允许回收站、UI 只给解锁。
7. **Reference 解锁 API 必失败**：`unlock_file_to_drafts` 用 `resolve(rel)`，reference 的 `rel` 是绝对路径；UI 已隐藏按钮，但服务层没有守卫。
8. **Reference 升级全局也必失败**：`request_promote_to_global` 把 `payload_rel_path`（绝对源路径）作为 `rel_path` 传给 `stage_and_commit`，最终 `PayloadStore::resolve` 明确拒绝绝对路径。UI 只检查了 `root.join("resources").join(abs)` 的文件存在，没有预检 rel 合法性；Reference 文件因此没有跨项目共享出口（只能本地打开/取回/软删）。
9. **全局脚本徽标与删除守卫错位**：`resolve` 显示表，`trash_global` 按 `kind=file` 拒绝；目前靠 UI 的 payload 判定绕开，不是统一判据。
10. **全局文件设计未落地**：单混合行 vs 文件行+派生表行；脚本内容表 vs 文件本体。`global-file-asset-design.md` 的 §4/§5/§8 与现码不一致。
11. **表型内容指纹是表名**：原地改表数据不会被识别；“内容已变”只对“换表名”有效。

### P2（语义/文档/测试）

12. **旧业务流文档过期**：`asset-table-file-business-dataflow.md` 的 T5/T6 仍写 parquet 中转；F6 写“文件型升级不保存文件本体”；F8 写“数据文件全局行可引用”；F9 写“copy 模式软删除、payload 搬进项目回收站”；4.3 整节已被 `global-file-asset-design.md` 取代。现码已改为“Copy 文件不能直接删、先解锁”。需要回写或标注废弃。
13. **`global-file-asset-design.md` 内部冲突**：修订头写“文件不进回收站”，§8.1 又写“全局删除 = 进回收站”。现码跟修订头，但正文未同步。
14. **面板测试不碰真实 store**：手搓 `ArchiveRow`，无法发现表行缺失。
15. **数据对象资产依赖面板当前筛选**：`assets_from_panel` 读 `view_rows()`，用户切换实例/筛选会改变数据对象看到的资产集合；应读未筛选的 canonical 列表。
16. **项目表永久删除不 DROP 表**：可能是“登记删除不等于数据删除”的刻意口径，但用户看到“永久删除”会预期数据消失；需要明确定义。
17. **“文件 + 派生表”没有成组模型**：项目 Copy 文件不建派生表；全局现码一条混合行；设计稿两条行。三种做法并存。
18. **删除确认文案把表型算进 copy**：`delete_actions` 用 `mode == Copy && !unlock` 统计 `copy_count`，表型（`payload_mode=copy`、无文件）也会被算进去，文案却说“删除 resources/ 副本”。用户看到的说明与本体位置不符。

---

## 7. 需要你重新定的语义（按影响排序）

| # | 问题 | 现码行为 | 需要拍板 |
| --- | --- | --- | --- |
| S1 | “文档”与“表”按什么分？ | 项目按 `kind`；全局 UI 按 payload；徽标按 `analytics_table` | 按“本体所有权”分，还是按“能否读成表”分？同一数据文件允许同时是文档和表吗？ |
| S2 | 资产库面板应该展示哪些行？ | 只展示 `file_rel_path` 非空的项目行 + 全部全局行 | 表型/引用是否进项目面板？还是只进数据对象面板？还是两处都进？ |
| S3 | 表型资产登记后在哪里被管理？ | 现码两处都看不到（缺陷） | 资产库、数据对象面板、或独立“表资产”区；必须给一个可达入口 |
| S4 | 项目表 → 全局的唯一入口放哪？ | 资产详情/右键；实际不可达 | 资产库、数据对象面板，还是两处都放？ |
| S5 | 全局数据文件是文件还是表？ | 一条混合行：分发按表、UI 按文件、徽标按表 | 1) 文件资产 + 派生表两条行；2) 一条行加 `body_kind/share_kind`；3) 只当表，不保留文件恢复 |
| S6 | 全局文件/脚本能不能进回收站？ | 文件不能，只能解锁；混合数据文件服务层能、UI 不能 | 统一“文件解锁、表回收站”，还是文件也进全局回收站？ |
| S7 | `table_ref` 是否保留？ | 可创建，但不可见、不可固化/删除 | 补全生命周期（可见 + 固化 + 删除 + 释放 pin），还是暂停创建、只留后端能力？ |
| S8 | 表型永久删除是否 DROP 表？ | 不 DROP，只清登记 | “删除资产”是否等于“删除数据”？要不要二次确认 + 明确文案？ |
| S9 | 显示名是身份还是标签？ | 归档只查路径不查名；表登记/全局链按名接续；解锁按名清链 | 显示名唯一？改名是否影响版本链？同名不同资产如何区分？ |
| S10 | “文件 + 派生表”是一条资产还是两条？ | 项目文件不建派生表；全局混合一行；设计稿两条 | 成组管理（一个资产两种形态），还是两条独立资产 + 来源链？ |
| S11 | 全局版本恢复 UI 做到哪一步？ | 后端支持任意版本；UI 只有“恢复上一版” | 只上一版，还是版本列表选 N？文件与表是否同一交互？ |
| S12 | 分发到 B 的版本语义？ | 新建项目行，版本 = 项目内同名 `MAX+1` | 每次分发都新建一行（当前），还是更新已有资产版本链？ |
| S13 | Reference 文件的定位？ | 只能本地打开/取回/软删；升级全局和解锁都被绝对路径守卫拒绝 | 它是“本地只读指针”就明说不可共享；若要共享，必须先把源路径归一成受管相对路径或给全局单独开绝对源通道 |

---

## 8. 一个可讨论的语义模型（供纠偏，不是实现方案）

现码把“是什么（kind）”“本体在哪（payload）”“怎么共享（file/table）”“能不能引用（pin）”压进少量列，导致三套判据互相打架。建议拆成两个正交轴，再由轴推导动作：

**轴 A：本体所有权**
- `ManagedFile`：本体在 `resources/`，可解锁、可文件版本恢复；
- `ExternalFile`：本体在外部源，只能软删登记、取回、不可覆盖还原；
- `ProjectTable`：本体在项目 `analytics.duckdb`；
- `GlobalTable`：本体在全局 `analytics.duckdb`；
- `NoBodyRef`：只有指向全局表的引用，无本地本体。

**轴 B：共享形态**
- `File`：分发/引用按文件；
- `Table`：分发/引用按表；
- `FileWithDerivedTable`：文件是本体，派生表是伴随共享形态（需要决定一行还是两行）。

动作从轴推导，而不是从 `payload_rel_path` 猜：
- 危险区：`ManagedFile` → 解锁；`ExternalFile` → 软删登记；`ProjectTable/GlobalTable` → 回收站；`NoBodyRef` → 删除引用 + 释放 pin。
- 引用：只有 `GlobalTable` / `FileWithDerivedTable` 的派生表可引用；`ManagedFile` 本身不可引用。
- 面板：所有 registry 行都可见，用“本体所有权”徽标区分；索引修复单独用文件行查询。
- 数据对象：读 canonical 全量列表，不读面板筛选结果。

这组轴是否能覆盖你想要的“文档 / 表”语义，需要你定；如果定下来，现有 `kind` 可以保留为兼容列，但不再承担动作判据。

---

## 9. 证据索引（按文件:行）

- 面板只取文件行：`crates/analytics_resource/src/jobs/refresh.rs:124-127`
- 文件行 SQL：`crates/analytics_resource/src/store/resource/read.rs:142-147`
- 全量存活行（生产零调用）：`crates/analytics_resource/src/store/resource/read.rs:17-45`
- 快照只遍历 `resources`：`crates/analytics_resource/src/present/snapshot.rs:36-102`；全局行单独：`:106-136`
- 数据对象从面板取资产：`crates/workbench/src/panels/data_objects_tab.rs:411`、`:601-619`
- 表登记按名接续：`crates/analytics_resource/src/table_asset.rs:191-214`
- 表型行无本体：`crates/analytics_resource/src/table_asset.rs:217-242`
- Copy 归档是复制 + 只读：`crates/analytics_resource/src/payload/transfer.rs:56-87`
- reference 登记键 = 绝对源：`crates/analytics_resource/src/service/archive.rs:17-21`、`:100-103`
- 绝对路径守卫：`crates/analytics_resource/src/payload/paths.rs:40-57`
- reference 升级全局把绝对路径当 rel：`crates/analytics_resource/src/resource_host/detail_actions.rs:228-258`
- 全局 stage 再走 `resolve`：`crates/analytics_resource/src/global/payload.rs:60-97`
- 再归档版本副本：`crates/analytics_resource/src/service/archive.rs:192-206`
- 表型还原走另一条：`crates/analytics_resource/src/service/checkout.rs:67-82`
- reference 还原拒绝：`crates/analytics_resource/src/service/checkout.rs:74-82`
- 解锁按名清链：`crates/analytics_resource/src/service/unlock.rs:71-75`
- 解锁拒绝非 File：`:38-47`；reference 绝对路径解析：`:49-55`
- 引用删除释放 pin：`crates/analytics_resource/src/service/trash.rs:430-451`；调用点 `:359`、`:421`
- Copy 文件删除硬闸门：`crates/analytics_resource/src/service/trash.rs:55-71`
- 删除确认文案把表型算进 copy：`crates/analytics_resource/src/resource_host/delete_actions.rs:19-41`
- 全局危险区按 payload 分文件/表：`crates/analytics_resource/src/detail_view/actions.rs:71-78`、`:194-228`
- 引用作业真实调用：`crates/analytics_resource/src/jobs/global.rs:372-416`；固化：`:420-466`
- 引用服务：`crates/analytics_resource/src/global/refs.rs:38`、`:183`、`:230`
- 全局分发按 kind 分流：`crates/analytics_resource/src/global/distribute.rs:149-151`
- 全局登记 kind 映射：`crates/analytics_resource/src/global/register.rs:237-243`
- 全局文件升级单行：`crates/analytics_resource/src/global/promote.rs:172-190`
- 全局文件删除拒绝：`crates/analytics_resource/src/global/delete.rs:237-247`
- 全局文件版本恢复：`crates/analytics_resource/src/global/promote.rs:230-...`
- 设计稿差距：`docs/architecture/analytics_resource/global-file-asset-design.md` §4.1/§4.3/§5.1/§5.2/§8.1
- 旧业务流过期点：`docs/architecture/analytics_resource/asset-table-file-business-dataflow.md` T5/T6、F6/F7/F8、§4.3
