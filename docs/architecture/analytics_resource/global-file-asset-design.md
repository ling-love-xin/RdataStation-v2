# 全局文件资产与版本恢复设计（草案）

> 状态：草案，2026-11-26。本文翻案 `analytics-resource-prototype-design.md` §13.5 的“方案 B 不采用”，
> 把全局文件从“只投影成表”改为“文件本体 + 可选派生表”。
>
> 实施进度（2026-11-26，第一版）：
> - 已落：`data/system/resources/` 本体、`versions/<asset_key>/vN/` 历史副本、
>   sha256 / 体积按实际字节、文件版本恢复（生成新版本）、文件本体进全局回收站 / 恢复 / 永久删除。
> - 当前形态：数据文件仍登记为 `kind = analysis` 并保留派生表，本体附加在同一行（服务版本恢复）；
>   脚本登记为 `kind = file`。**独立文件资产行 + 派生表拆成两条资产**、文件 `KeepVersions`、
>   回收站列表 / 恢复 UI 仍待做。旧表资产不伪造本体。
>
> 实施进度（2026-11-26，第二版删除语义）：
> - 全局文件**不再进回收站**：`unlock_global_file_to_project(name, drafts_root)` 把当前内容
>   复制到目标项目草稿箱（同名 `-2/-3` 避让），清历史副本后 `purge_global`；`trash_global`
>   对 `kind = file` 硬拒绝。全局回收站只收表型。UI 危险区改为「解除锁定并退回草稿箱…」。
> - 项目侧同口径见 `duckdb-ownership-m6-slimming.md` §9.9。
> - 项目回收站唯一入口在草稿箱栏（跨模块聚合 UI 待落地；资产库面板头入口暂留）。
>
> 前置事实：现状全局只有 `data/system/{global.db, analytics.duckdb}`，没有 `data/system/resources/`；
> 数据文件升级后只剩表，脚本升级后只剩内容表。详见
> `asset-table-file-business-dataflow.md` §4.3。

## 1. 目标与非目标

### 1.1 目标

1. **全局文件有本体**：文件升级到全局时复制原始字节，不再只解析成表。
2. **文件级可追溯**：每个版本有 hash、大小、原始文件名、来源项目/资源/版本、升级时间。
3. **文件级版本恢复**：能从历史版本副本生成新版本，不原地回滚、不销毁历史。
4. **表仍可共享**：数据文件可派生一张全局表，继续支持查询、表引用、表分发。
5. **脚本不丢字节**：文本脚本按文件保存；不再依赖“内容表”作为唯一载体。
6. **旧数据不假装可恢复**：已升级成表的全局行保持“表型（无原文件）”，不伪造文件副本。
7. **全局表与项目表同删除语义**：删除只软删登记、保留表；恢复预检表；永久删除才 `DROP`。

### 1.2 非目标

- 不做全局文件引用（项目引用文件后置；v1 仍只引用表）。
- 不支持白名单之外的二进制（png / zip / docx 等）；仍在前置入口拒绝。
- 不把现有全局表资产迁移成文件资产；它们没有原始字节，迁移没有意义。
- 不做全局文件回收站 TTL 产品策略；本文只定义删除守卫与落点，TTL 另行定。
- 不做项目 ↔ 项目直传；跨项目仍只走“项目 A → 全局 → 项目 B”。

## 2. 现状与缺口

| 项 | 现状 | 缺口 |
| --- | --- | --- |
| 全局目录 | `data/system/{global.db, analytics.duckdb}` | 没有 `resources/` |
| 数据文件升级 | `CREATE TABLE AS SELECT * FROM read_xxx(file)` | 原始字节丢失 |
| 脚本升级 | 内容表 `filename / content / content_hash` | 只有文本，没有文件本体；版本恢复无入口 |
| 全局版本 | 表一版一张，旧表默认保留 | 只有表版本；文件版本不存在 |
| 全局版本 UI | 列表只列活跃行 | 没有版本浏览 / 回退 |
| 全局删除 | 无活跃引用时删同名链所有表 | 文件资产删除语义未定义 |

## 3. 目录布局

```text
<RDS_HOME>/data/
├── system/
│   ├── global.db
│   ├── analytics.duckdb
│   └── resources/
│       ├── <rel>                         # 当前文件（相对路径与项目侧同名）
│       └── versions/
│           └── <asset_id>/
│               ├── v1/<filename>         # 每个版本一份原始字节
│               ├── v2/<filename>
│               └── ...
└── trash/
    └── global/
        └── <asset_id>/<timestamp>/       # RDS 自管全局回收站；不是 Windows 回收站
```

约定：

- `resources/<rel>` 是**当前版本**的权威副本；`versions/<asset_id>/<v>/` 是历史副本。
- 写入先落 `resources/.staging/<uuid>`，校验 hash 后再原子 rename 到目标路径；失败不留半成品。
- `asset_id` 是全局文件资产的稳定 id，版本目录按它组织；表资产仍是一版一张表，不套这个目录。
- 路径安全化复用项目侧 `payload/paths` 的规则：拒绝 `..`、绝对路径、盘符逃逸、Windows 保留名。
- 全局回收站与项目回收站是**同一套应用语义、两个后端**：项目走 `.RSmeta/trash/`，全局走
  `data/trash/global/`；都不调用 Windows 回收站（`SHFileOperation` / Shell `Recycle`）。

`global_paths.rs` 需要新增：

- `get_global_resources_dir()`
- `get_global_resource_version_dir(asset_id, version)`
- `get_global_trash_dir()` → `data/trash/global`

## 4. 数据模型

### 4.1 两种全局资产形态

| 形态 | `kind` | 本体 | 版本模型 | 能否被表引用 |
| --- | --- | --- | --- | --- |
| 全局文件资产 | `file` | `system/resources/<rel>` | **一条稳定行 + 版本快照 + 版本目录** | 否（文件引用后置） |
| 全局表资产 | `analysis` | 全局 DuckDB 表 | 一版一行 + 一版一张表（现状） | 是 |

文件资产不写 `analytics_table`；派生表是**另一条** `kind='analysis'` 的全局表资产，避免
`ArchiveKind::resolve(kind, analytics_table)` 把文件行误判成表。

### 4.2 全局文件资产字段

| 字段 | 值 | 说明 |
| --- | --- | --- |
| `id` | `arg_<uuid>` | 稳定，不随版本变 |
| `resource_type` | `file` | 与项目文件资产同口径 |
| `kind` | `file` | 决定 UI 路由与判据 |
| `scope` | `global` | 全局实例 |
| `name` | 全局本名 | 不可在项目内改；项目只能起别名 |
| `version` | 当前版本号 | 每次升级 / 恢复 +1 |
| `file_rel_path` | `resources/` 下的相对路径 | 当前文件位置 |
| `payload_mode` | `copy` | 全局文件必须是受管副本 |
| `readonly` | `1` | 全局资产只读 |
| `content_hash` | 当前版本 sha256 | 打开 / 分发前校验 |
| `file_size` | 当前版本字节数 | 展示与守卫 |
| `source_path` | 项目侧原始绝对路径 | **只作追溯**，换机/换项目后不可依赖 |
| `origin_kind` | `project` | 来源项目 |
| `origin_ref` | 项目资源 id + 版本 | 直接上游一跳 |
| `definition_sql` | NULL | 文件资产不写建表 SQL |

版本快照 `analytics_resource_versions.snapshot` 至少包含：

```json
{
  "kind": "file",
  "global_version": 3,
  "file_rel_path": "resources/reports/q3.csv",
  "filename": "q3.csv",
  "content_hash": "sha256:...",
  "file_size": 12345,
  "source_project_resource_id": "arp_...",
  "source_project_version": 2,
  "restored_from_version": 1,
  "created_at": "..."
}
```

`restored_from_version` 只在“从历史版本恢复”时出现；普通升级不写。

### 4.3 派生表资产

数据文件（白名单：csv / tsv / txt / parquet / xlsx / xls / json / ndjson）在文件资产落定后，
可再建一条全局表资产：

| 字段 | 值 |
| --- | --- |
| `kind` | `analysis` |
| `analytics_table` | 本版实际表名 |
| `origin_kind` | `global_file` |
| `origin_ref` | `{"asset_id":"arg_...","version":N}`，指向文件资产版本 |
| `definition_sql` | `CREATE TABLE ... AS SELECT * FROM read_xxx('data/system/resources/...')` |
| `structured` | `1` |

派生表仍按现有“一版一张表”登记；它的版本号与文件资产版本号**对齐但不强绑定**：
文件 vN 对应派生表 vN；派生表可重建，文件版本不可丢。

脚本默认**不建派生表**。全局文件资产本身可只读打开、预览、分发；需要全文检索时再走
“按需建内容表”的后续动作，不把内容表当唯一本体。

## 5. 升级流程

### 5.1 数据文件（项目文件资产 → 全局）

1. 前置校验扩展名、大小、项目只读、源文件存在。
2. 解析目标 `rel_path`；与全局已有文件 / 登记冲突时，走同名冲突框（覆盖 / 改名 / 取消）。
3. 复制到 `resources/.staging/<uuid>`，计算 sha256，校验后原子 rename 到 `resources/<rel>`。
4. 写全局文件资产行 + v1 版本快照；`content_hash` / `file_size` 用真实字节量。
5. 若 `build_table = true`（默认 true）：用全局文件路径建派生表，登记 `kind='analysis'` +
   `origin_kind='global_file'` + `origin_ref`。
6. 派生表建失败：**不回滚文件资产**；文件行保留，回执提示“文件已升级，派生表失败，可重试”。
   这比“表失败就把文件也删掉”更符合“文件是本体”的语义。

### 5.2 文本脚本

1. 前置校验文本白名单、大小上限（沿用当前 16 MiB）。
2. 复制原文件到 `system/resources/<rel>`，写文件资产 + 版本快照。
3. 不建内容表；全局行 `kind='file'`，打开走文件读取（文本编辑器只读）。
4. 分发时复制文件到项目 `resources/<rel>`，登记项目 File 资产。

### 5.3 项目表资产（项目分析表 → 全局）

保持现状：只建全局表资产，一版一张表；没有文件本体，也不承诺文件恢复。
UI 与文档明确标“表型（无原文件）”，避免用户误以为可以还原成文件。

## 6. 版本恢复

### 6.1 文件资产：恢复到 vN

**与项目文件型版本的关系：同构，不是另起一套。**

| 维度 | 项目文件型（现状） | 全局文件资产（本文） |
| --- | --- | --- |
| 当前本体 | `.RSmeta/resources/<rel>` | `data/system/resources/<rel>` |
| 历史副本 | `.RSmeta/resources/versions/<resource_id>/<v>/` | `data/system/resources/versions/<asset_id>/<v>/` |
| 资产身份 | 一条资源行，`version` 递增 | 一条资源行，`version` 递增 |
| 版本触发 | `content_hash` 变化才涨版本 | 内容变化才涨版本；同 hash 再升级不重复建版本 |
| 恢复语义 | 用旧副本生成新版本，不原地回滚 | 同左 |
| 副本裁剪 | 默认保留最近 5 份，版本行保留 | 同一套 `KeepVersions` 口径 |
| reference 模式 | 支持；不存历史副本，不能覆盖源，只能取回 | **不支持**；全局文件一律 `copy`，必须可恢复 |
| 只读 | `copy` 档内容只读 | 全局资产只读，同左 |

文本脚本在这套模型下就是普通文件：line ending / BOM / 编码按字节原样保存，不再经过
`String` 内容表；恢复也是“旧字节 → 新版本”。

语义：**用旧副本生成新版本，不原地回滚、不删除 vN 之后的版本。**

1. 校验资产是 `kind='file'`、vN 的 `versions/<asset_id>/<N>/<filename>` 存在且 hash 匹配。
2. 把旧副本复制到 `resources/.staging/<uuid>`。
3. 把当前文件先确保已进入其版本目录（正常升级路径已做；缺失则先补一份，补不了则拒绝恢复）。
4. 原子替换 `resources/<rel>` 为旧副本内容。
5. 资源行 `version = MAX(version) + 1`；写版本快照，带 `restored_from_version = N`。
6. 若有派生表：按新文件重建派生表，登记新表版本；旧派生表按保留策略裁剪，不立即删。
7. 回执写明“已从 vN 恢复为 v(N+1)”，并给“查看版本”入口。

约束：

- 恢复不改变 `name` / `file_rel_path`；改名仍走独立动作。
- 恢复不释放任何 `analytics_global_refs` pin；旧版本仍可被引用。
- 若 vN 的历史副本已被裁剪，返回“该版本内容副本已裁剪，无法恢复”，不静默用当前内容冒充。

### 6.2 表资产：恢复到 vN

保持现有“一版一张表”模型，推荐：

1. 校验旧表存在；
2. `CREATE TABLE <新表名> AS SELECT * FROM <旧表名>`；
3. 登记新版本行，`definition_sql` 指向新表；
4. 旧表保留，裁剪交给 `KeepVersions`。

不推荐“把当前指针改回旧表行”：旧表可能被裁剪，且多行共享一张表会让删除守卫复杂化。
CTAS 的代价是复制数据；对超大表可后续再加“只读别名 + 引用计数”的优化，不放进第一版。

### 6.3 与 pin 的关系

- 表资产被项目引用时，`analytics_global_refs` 按 `global_name + global_version` pin。
- 派生表版本被 pin 时，**它依赖的文件版本也必须视为被 pin**：裁剪文件版本前要检查
  `origin_ref` 指向该文件版本的派生表是否有活跃引用。
- 文件资产本身暂不能被项目引用；没有 pin 的文件版本按 `KeepVersions` 裁剪。

## 7. 分发与引用

### 7.1 分发文件资产

1. 读取指定版本 `versions/<asset_id>/<v>/<filename>`（不是当前文件）。
2. 冲突三查：项目登记名、项目 `resources/<rel>` 文件、项目分析表（仅派生表分发时查）。
3. 复制到项目 `resources/<rel>`；登记项目 File 资产：
   - `readonly = 1`
   - `payload_mode = 'copy'`
   - `promoted_from = 'global:<asset_id>'`
   - 版本快照带 `global_version` / `content_hash`
4. 回执：“已从全局分发文件「X」vN 到本项目”。

### 7.2 分发派生表

沿用现有表分发链路；额外写 `derived_from` 指向文件资产版本，便于来源链显示
“项目表 ← 全局表 ← 全局文件 ← 项目文件”。

### 7.3 引用

- v1 仍只允许引用**表资产**；文件资产行 `can_reference = false`。
- 引用派生表时，pin 记在派生表行；文件版本的保护见 §6.3。
- 固化派生表 = 现有 `solidify_ref_version`，复制成项目本地表；文件资产不参与固化。

## 8. 删除、裁剪与守卫

### 8.1 统一语义：全局删除 = 进全局回收站

全局和项目的表是同一套逻辑：**删除只软删登记，表本体留在库里；恢复只复活登记；永久删除才清登记 / 版本 / 表。**
文件资产多一步 payload 搬运；表资产没有 payload 可搬。

`delete_global` 需要拆成两个动作：

| 动作 | 行为 |
| --- | --- |
| `trash_global` | 活跃 pin 检查 → 软删登记（文件资产再移 payload）→ 写 `data/trash/global/<asset_id>/<timestamp>/manifest.json` |
| `purge_global` | 从回收站永久删除：文件资产清 payload；表资产 `DROP` 同名各版表；再清版本快照 / 登记 / 分发回执 |

现有“从全局实例删除…”按钮应改为“移入全局回收站…”；真正的 `DROP` 只出现在回收站的“永久删除”。

### 8.1.1 文件资产

1. 有活跃 `analytics_global_refs` pin（直接 pin 文件，或 pin 其派生表）→ 返回 `Referenced`，不移入。
2. 有派生表资产 → 派生表随文件一起软删登记、**保留表**，不立即 `DROP`。
3. 文件当前副本与所有版本目录移入 `data/trash/global/<asset_id>/<timestamp>/`，写 manifest。
4. 全局文件登记行置 `deleted_at`，版本快照保留。
5. 恢复：payload 移回 `data/system/resources/<rel>`，登记去掉 `deleted_at`，派生表登记一并复活；
   恢复前校验当前文件 hash 与快照一致。
6. 永久删除：清 payload、版本快照、派生表（含各版表）与分发回执。

**不走 Windows 回收站。** 全程用 `fs::rename` / 受控 move + 应用 manifest；
不调用 `SHFileOperation`、Shell `Recycle`、`Microsoft.VisualBasic.FileIO` 的回收站重载。

### 8.1.2 表资产

与项目表型完全同构，但有一个全局特有的细节：**全局表是一版一行，历史版本行本来就是
`deleted_at`（覆盖时软删）**。因此“回收站态”不能只看 `deleted_at`，要以 manifest / `trash_id` 为准。

1. 有活跃 pin → `Referenced`，不移入。
2. 移入 = 只对**当前活跃行**置 `deleted_at`；历史版本行保持原状；全局 DuckDB 的表、
   版本快照全部保留；manifest 记 `asset_id / name / version / analytics_table / 各版表名`。
3. 恢复 = 按 manifest 找到当前活跃行，复活登记；**先校验当前 `analytics_table` 还在**；
   表已被手工 `DROP` 时拒绝恢复，提示“表已不在，请永久删除这条登记”。
4. 永久删除 = 逐张 `DROP` 同名链各版表 → 删版本快照、全局登记、分发回执。

这就是现在 `delete_global` 里的“删表 + 清登记”逻辑，只是把它从“删除动作”挪到“永久删除”动作。

### 8.1.3 回收站条目

- 文件资产：`trash/<asset_id>/<timestamp>/payload/...` + `manifest.json`
- 表资产：`trash/<asset_id>/<timestamp>/manifest.json`（没有 payload 目录）
- 两类共用一个 `data/trash/global/` 根，但 `manifest.kind` 区分 `file` / `analysis`；
  恢复与永久删除按 `kind` 分派，不混走。

### 8.2 KeepVersions

- 文件资产：保留最近 N 个版本的 `versions/<id>/<v>/` 目录；当前版本永远保留；
  被 pin 的版本跳过；版本行 / 快照永久保留。
- 表资产：沿用现有 `prune_version_tables`；被 pin 的版本跳过。
- **裁剪不是用户删除**：它按保留策略直接删历史副本 / 旧表，不进回收站；
  否则回收站会变成第二备份库，且 TTL 无法收敛。用户显式删除资产才进回收站。
- 裁剪失败只记日志，不回滚已完成的升级 / 恢复。

### 8.3 完整性

- 打开 / 分发 / 恢复前校验 `content_hash`；不一致时标“文件已变”，给“接受当前内容（生成新版本）”
  或“从历史恢复”两条路，不静默继续。
- 当前文件缺失但版本副本在：允许“从最近版本重建当前文件”，不直接判资产损坏。

## 9. 迁移与兼容

### 9.1 新增迁移

建议 `crates/store/migrations/global/039_global_file_assets.sql`：

- 无新表；复用 `analytics_resources` / `analytics_resource_versions`。
- `kind` 允许 `file`（如果现有 CHECK 限制，需要放宽；先核对 `global/026` 的约束）。
- `origin_kind` 允许 `global_file`（派生表行用）。
- 可选加列 `restored_from_version INTEGER`（若不想只存 snapshot JSON）；建议先不加，
  从版本快照读，减少迁移面。

### 9.2 旧数据处理

- 现有全局 `kind='analysis'` 行保持原样，标“表型（无原文件）”。
- 现有脚本内容表：可提供一次性“导出为全局文件资产”的迁移动作（读 `content` 写文件），
  但不自动执行；用户确认后逐条迁移。
- 现有全局表资产不能被引用为文件；文件引用后置。

### 9.3 代码影响面

| 区域 | 改动 |
| --- | --- |
| `global_paths.rs` | 新增 resources / versions / trash 路径 |
| `global/register.rs` | 新增 `register_global_file`（文件行 + 版本快照） |
| `global/mod.rs` | `read_global_row(Some(v))` 需支持“文件稳定行 + 版本快照”取版本；表资产仍按旧行取 |
| `global/promote.rs` | 文件路径改为“复制文件 + 可选派生表”；表路径保持 |
| `global/distribute.rs` | 新增文件分发；表分发加 `derived_from` |
| `global/delete.rs` | 拆 `trash_global`（软删登记 / 文件移 payload）与 `purge_global`（真删 payload、表、登记） |
| `global/list.rs` | 文件行不再带 `payload_rel_path` 误注释；按 `kind` 分流 |
| `model/kind.rs` | `ArchiveKind::resolve` 确认 `kind=file` 优先，不被 `analytics_table` 覆盖 |
| UI | 全局文件行详情 / 版本对话框“恢复此版本” / 分发文件或表 |

## 10. UI 契约

### 10.1 全局列表 / 详情

- 文件行显示“文件”徽标；详情“本体”区显示 `data/system/resources/<rel>`，不显示分析表。
- 派生表行显示“表”徽标，来源链显示“全局文件 vN”。
- 文件行动作：打开（只读）、分发文件…、移入全局回收站…、查看版本；**没有“引用到本项目”**。
- 派生表行动作：沿用现有表动作，增加“查看来源文件”；删除同样走“移入全局回收站…”。

### 10.2 版本对话框

- 文件资产：每行显示版本、时间、hash 前 8 位、大小、是否有副本；
  动作“恢复此版本…”（危险度低，非破坏性）；已裁剪副本的行置灰并说明。
- 表资产：沿用现有版本行；“恢复此版本”走 CTAS；引用行仍只显示固化。

### 10.3 全局回收站

- 入口：全局列表筛选「回收站」或独立「全局回收站」面板；与项目回收站并列，不共用后端。
- 每行：名称、`kind`（文件 / 表）、最后版本、删除时间、payload 是否完整、是否有 pin。
- 动作：恢复（文件 payload 移回 `data/system/resources/<rel>`；表只复活登记并预检表存在；
  两者登记行都去掉 `deleted_at`，版本号不变）／永久删除（文件清 payload；表 `DROP` 各版表；
  再清版本快照、派生表与分发回执）。
- 已进回收站的资产不能再被引用 / 分发 / 打开；恢复后才回到正常列表。表资产恢复失败时
  明确提示“表已不在”，不制造 Missing 行。

## 11. 验收用例

1. 数据文件升级 → 全局文件资产 v1 + 派生表 v1；原文件字节与 hash 一致。
2. 脚本升级 → 全局文件资产 v1；内容表不再作为唯一本体。
3. 文件 v2 覆盖 → v1 副本仍在；恢复 v1 → 生成 v3，v2 仍可查。
4. 恢复时 v1 副本已裁剪 → 明确失败，不冒充当前内容。
5. 派生表被项目引用 → 文件 vN 不能被裁剪；删除文件资产返回 `Referenced`。
6. 分发文件 v1 到项目 → 项目 `resources/<rel>` 字节与 hash 一致；再分发 v2 走同名冲突。
7. 删除全局文件资产 → 派生表随文件软删；payload 进 `data/trash/global/`，登记只置 `deleted_at`；
   从全局回收站恢复后文件与派生表都回来；永久删除才清登记、快照与派生表。
8. 旧全局表资产 → 仍可分发/引用；UI 标“表型（无原文件）”，不提供文件恢复。
9. 全局表资产移入回收站 → 只软删登记，表与版本快照保留；恢复时表在则成功，表被手工 DROP
   则明确失败；永久删除才 `DROP` 各版表并清登记。
10. `list.rs:70` 的“全局 resources”注释已修正，代码与文档同口径。

## 12. 待拍板

1. **文件资产版本模型**：本文推荐“一条稳定行 + 版本快照 + 版本目录”；表资产保持“一版一行”。
   若要求全局所有资产统一“一版一行”，文件目录要改成按 `name + version` 组织，恢复链会更绕。
2. **脚本是否保留内容表**：本文推荐不保留，文件资产即本体；若搜索/预览需要，再按需建派生内容表。
3. **数据文件是否默认建派生表**：本文推荐默认建；用户可关，关后只共享文件。
4. **全局回收站 TTL**：本文已把落点定为 `data/trash/global/`（RDS 自管，不走 Windows 回收站）；
   保留多久、是否自动清理另行定，首版只做“恢复 / 永久删除”。
5. **文件引用**：本文明确后置；若要做，需要单独设计项目引用行的打开 / 校验 / pin 语义。

---

## 附：与现有文档的关系

- `analytics-resource-prototype-design.md` §13.5：本文取代其“方案 B 不采用”的结论。
- `asset-table-file-business-dataflow.md` §4.3：本文是其“路线 B”的落地设计。
- `global-reference-design.md`：表引用 / 固化 / pin 继续有效；文件资产不进入引用链。
