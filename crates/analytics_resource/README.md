# rds-analytics-resource — M6 资产库 / 分析存档

> 本文件是 crate 的 **README 级入口**：只提炼模块特点与代码结构，完整设计以 `docs/architecture/analytics_resource/` 为准（架构约定：crate 内不复制设计文档）。

## 一句话定位

资产库是**项目的分析存档**：把「值得留存、需被引用、要能复现」的分析产物，从工作区转成**只读、有版本、带来源**的正式资产。

> 三个模块的分工：M4 看数据（活水镜像）→ M5 干活（工作区）→ **M6 留证据（冻结的存档 + 归档凭证）**。

## 模块特点

### 1. 两档 kind：本体在「受管文件」或「分析库表」——2026-11-16 收口

| kind | 本体在哪 | 复现强度 | 状态 |
| --- | --- | --- | --- |
| `file` | `{项目}/resources/`（**copy 默认**：复制进受管目录 + 只读，草稿箱那份保留为工作副本；**reference**：本体留原位、只存元数据） | **强**：内容冻结，重跑得同解（reference：读源 + 指纹校验） | ✅ 第一期 |
| `analytics_table` | **分析库里的持久表**（项目侧 `{项目}/.RSmeta/analytics.duckdb`；全局侧 `<RDS_HOME>/data/system/analytics.duckdb`），表名登记在 `analytics_table` 列 | **强**（与 `file` 同级）：表在库内即冻结；取样走 `SampleSource::on_duckdb`，不按路径读 | ✅ 表名列已落；打开 / 查看统计已通 |
| ~~`table_ref`~~ | ~~远端（不在本机）~~ | ~~**弱**~~ | ❌ 划掉：跨文件表引用属编辑器 / 联邦层语义（代码保留兼容读取，**不进筛选菜单**） |

> **2026-11-16 口径收口**：`analysis` 档不再存在。它的本体一直是 `resources/` 下的受管文件，与
> `file` 是同一个东西——「数据文件带结构指纹」是**能力**，不是分类，现在由 `definition_sql`
> 是否非空表达（另有三列 `origin_kind` / `origin_ref` / `structured` 记来源与结构化事实）。
> 历史行的 `kind = 'analysis'`：读面 `ArchiveKind::from_db_str` 直接读成 `File`（旧库不迁移也不会
> 认错档），迁移 `project_meta/032`（镜像 `global/035`）再把库里的串归一为 `file`。
> 结构指纹行仍**不参与字节指纹比对**（`indexer/scan.rs` 按 `definition_sql` 跳过），否则每一行
> 都会在索引修复里被报成「内容已变」。
>
> **库层 `CHECK` 仍是三值**（`file` / `analysis` / `table_ref`）：改它要重建表，而迁移执行器把
> 整段 SQL 包在一个事务里、`PRAGMA foreign_keys` 在事务内关不掉（见 `031` 迁移注释）。
> 所以表型资产落库时 `kind` 仍写 `analysis`（`ArchiveKind::db_write_str`），
> **权威判据是 `analytics_table` 列非空**（`ArchiveKind::resolve`）。

> **2026-11-12 降级说明（保留）**：旧版把 `analysis` 标成「定义冻结，数据可重建」，但
> `definition_sql` 全仓只被登记与算指纹、**没有任何执行点**。对主打消除业务误解的模块，界面不能
> 承诺做不到的事；`FINGERPRINT_HINT` 写明「只作登记留痕，不自动重建或校验」。将来要兑现重建，
> 需单独开工并牵动 M8 与 DuckDB 计算层。

界面上**档位徽标常显**（`已归档` / `分析库表` / `引用`）——用户要一眼知道本体在哪；
徽标只描述形态，不承诺重建能力。

> **File 档归档粒度（2026-10-13 裁决）**：两档都做、默认 copy。copy = 本体复制进 `resources/`（版本冻结、回溯读副本永不失效，草稿箱 = 工作副本 checkout 语义）；reference = 只存元数据（地址 + 文件名 + 指纹 + 版本记录，本体留原位，打开校验指纹，不一致提示"源已变更，快照失效"）。不做真 diff；血缘任何模式都有（`source_path` + `source_fingerprint` + 版本链）；purge 一起清除。

### 2. 归档凭证 = 来源 + 代码 + 内容指纹

这三样是 M6 不可替代的部分（也是与"书签册"的分水岭）：

| 要素 | 列 / 位置 | 回答的问题 |
| --- | --- | --- |
| 来源 | `promoted_from` / `source_connection_id` / `source_table` | 这结论是用什么数据得出的 |
| 上游 | `origin_kind` / `origin_ref` | 这一版是从哪一条上游搬来的（**只记一跳**） |

> **来源链 2026-11-16（`origin_kind` + `origin_ref`）**：跨项目只允许「项目 A → 全局 → 项目 B」。
> 各层**版本各自编号**（A 的 v5 → 全局 v1 → B 的 v1），每一行只记**直接上游**那一跳：
> 全局行 `project` + `{"resource_id","version"}`，项目行（分发落盘）`global` + `{"instance","version"}`。
> 拼整条链是读取侧的事（[`model::origin`](src/model/origin.rs) 拼成一句「项目存档 arp_1 的 v5」，
> 详情「来源」区多一行「上游」）；不在行里冗余整条链——冗余会随上游新增版本而过期。
> `origin_ref` 解析失败 / 缺 id 一律降级为不画那一行，不硬报错。

> **`promoted_from` 一字段两义（2026-11-07 记录）**：草稿归档写来源草稿的相对路径（`scratchpad/<rel>`），
> 升级到全局 / 分发落项目写 `global:<全局实例 id>`。读取面按前缀分开解释
> （[`PromotedFrom`](src/model/promoted_from.rs)）：详情「来源」区据此显示「来源草稿」+ 路径、
> 或「来源全局实例」+ id（前缀是库里的约定，不露给用户）。真要拆成两列时，
> `global:` 前缀就是可回填的迁移线索（需新迁移 + 改列名清单 + 改 `starts_with("global:")` 断言）。
| 代码 | 本体文件本身；带结构指纹的行另有 `definition_sql`（**只登记留痕、不执行**） | 怎么算的 |
| 内容指纹 | `content_hash`（sha256） | 还是不是当初那份 |

> **来源口径（2026-10-13 裁决）**：草稿箱 = **文件地址 + 文件名**；数据库 = **连接身份指纹 + 表**（复用 `engine::persistence::metadata_identity::ConnectionIdentity::fingerprint`，不新造）。

### 3. 目录与真相源

```text
{项目}/
├── resources/                       ← 本体（可见、归档后只读）
└── .RSmeta/
    ├── project.db                   ← 索引（登记 / 版本 / 标签 / 分组）
    ├── resources/versions/<id>/<v>/ ← 历史内容副本（按 keep 份数裁剪）
    └── trash/                       ← 项目级回收站（与草稿箱共用，origin = "resources"）
```

- **文件系统是本体权威，登记表是索引**；索引可重建，三类孤儿（有文件无记录 / 有记录无文件 / 指纹不匹配）由 `indexer` 检测、**人工确认**修复（不做静默导入 / 删除）。
- 内部态（`.RSmeta/**` 与任何点前缀路径）不得经 API 触达——`PayloadStore::resolve` 是唯一入口守卫。

### 4. 只读是常态，修改走「取回（检出）」

| 层 | 手段 |
| --- | --- |
| 应用守卫（硬约束） | `PayloadStore::ensure_writable` 对 `resources/` 下的写入直接拒绝并给出可操作提示 |
| 编辑器 | 以只读模式打开（编辑器既有的"分析资源锁定"来源） |
| 文件系统 | 只读属性（辅助；Windows / 网络盘不可靠，失败只警告） |

取回的坑已处理：**Windows 下 `CopyFile` 会继承源文件的只读属性**，`copy_out` 会显式清除，否则"取回的文件改不了"。

### 5. 版本由内容指纹触发，历史内容有保留上限

- **版本 = 内容 + 体积 + 元数据快照**（结构 / 指纹 / 来源 / 配方 / 时间）；名称 / 别名 / 标签 / 分组是**活状态不进版本**（改它们不涨版本、不发 `ChangeReason` 事件，保存后刷新即可）；版本区展示全部元信息；版本链补展示 / 检索（列表 / 相邻差异 / 血缘 / 按版本·时间·来源过滤）。

- `content_hash` 未变**不产生新版本**（否则"改个别名"也会消耗一个版本号——v1 的行为）。
- 版本表是**写前快照**语义：当前版本在资源行上、不进版本表；`parent_version_id` 指向本次写入的快照行（v1 写的是资源自身 id，无信息量）。
- 默认保留最近 5 份**内容副本**，超出只删副本，**版本行永久保留**——界面以"副本缺失"标注，不是错误。
- 保留策略有两个入口：设置页「资产库 › 历史内容保留」（`resources.keep_versions`：`0` = 只留元数据、`-1` = 全部保留）与归档对话框的「保留历史内容」（本次覆盖，留空 = 跟随设置）。领域类型是 [`KeepVersions`](src/model.rs)；`All` 在裁剪那一步是“不动作”，与 `0`（只留元数据）是两回事。

### 7. 全局实例：一版一张表（G2，2026-11-16）

跨项目只允许「项目 A → 全局 → 项目 B」，**每层版本各自编号**（A 的 v5 升级成全局 v1，再下发成 B 的 v1）。

全局侧按版本铺表：每次升级落一张**新表** `res_<描述>_<yyyymmddHHMMSS>`，历史版本的表**不删**——
「回到 v3 看数据」才有东西可看。登记行的 `name` 仍是资产名（列表 / 筛选 / 同名链版本号都靠它），
表名单独记在 `analytics_table` 列（[`global/table_name.rs`](src/global/table_name.rs)）：

| 场景 | 表名从哪来 |
| --- | --- |
| 新升级 | `res_<slug>_<时间戳>`；同一秒撞名时追加 `_v<版本>` |
| 读取（分发 / 打开 / 统计） | 一律按登记行 `analytics_table` 反查 |
| 迁移 036 之前的老行 | `analytics_table = name`（那时表名就等于资产名），读取面另有 `name` 回退 |

项目侧的 `res_` 持久表名走同一条口径（**保留中文**，`engine` 的
`sanitize_persistent_description`）：临时表名那把 ASCII 尺子只留给 `tmp_q_` / `tmp_i_`
（它们要以裸名内联进 SQL、也不给人看），而持久表名要 `persistent_table_label` 反解成
「订单明细 · 2026-11-15 10:30:00」——描述被抹成 `tmp` 的话，一屏几十张表全叫同一个名字。

连带的口径变化：**升级冲突只剩登记层**（表名带时间戳，「全局表被同名对象占用」不再可能）；
**覆盖式升级不再删旧表**（只软删旧登记），版本链按同名接续；全局行的双击 / 查看统计走
`SampleSource::on_global`（短命会话挂 `rds_global`），因为它的本体在另一个库文件里。

代价要说清：一版一张表 = 每版一份全量副本，全局库随版本数线性增长；**裁剪交给 `KeepVersions`**
那套策略（`prune_version_tables`，升级成功后按设置执行，见「能力状态」）：
只删更早版本的**表**，当前版本那张是本体、永远留，登记行与版本快照一条不删——被清掉的那版
在分发时报「已被保留策略清理」，元数据仍在、版本链还看得见。

**全局引用（2026-11-25）**：项目侧不再只能“分发复制一份”，也可以只记一条
`kind='table_ref'` + `origin_kind='global'` 的引用资产，按项目版本 pin 全局版本
（`global/refs.rs`、迁移 `global/038_analytics_global_refs.sql`）。引用行 `analytics_table`
为空、`source_table` 记 pin 的全局实际表名，打开 / 查看统计走 `SampleSource::on_global`；
`name` 是全局本名不可改，项目内只能改 `alias`。版本对话框对引用资产提供「固化为本地表」：
复制选中的全局版本为独立 `analytics_table` 资产，原引用保留，只释放该版 pin。全局删除 /
裁剪遇到活跃 pin 会跳过或拒绝；引用资产被永久删除时释放其全部 pin。

**表型存档的升级**（本体是项目分析库里的一张表，而不是文件）走另一条路：入口只给表名
（`PromoteRequest::source_table`），工作线程用项目写连接 `COPY <表> TO parquet`
（**不带 LIMIT，整表搬**）→ 再按数据文件升级。绕这一步是因为两个 DuckDB 都是单写者，
Windows 单句柄下互相 `ATTACH` 会撞「文件被占用」（分发侧同一条限制）。临时 parquet
用完即删，`COPY` 失败也删；升级是读，项目侧那张表原样留着。

### 8. 边界与协作

- **归档由上游发起**（草稿箱右键 / 本地文件选择）：M6 **只收 `PathBuf` + 元数据**（`ArchiveRequest`）与宿主预取好的 `DraftCandidate`，不认识 `ScratchpadStore` 等上游类型。依赖方向：`workbench / app → analytics_resource → engine, shared`；2026-11-12 收口后本 crate **不再依赖 scratchpad**（草稿箱扫描与标签预取由 `ResourceHostBackend::draft_candidates` 注入）。
- 变更事件 `ResourcesChanged { reason, resource_id }`：替代 v1 那个"发了没人听"的无载荷事件；**发/收两端必须同批落地**。
- 回收站只有一套：统一走项目级 `ProjectTrash`（`origin = "resources"`），跨模块还原必须被拒。
- 删除语义（2026-11-26 定稿）：**文件型资产不可直接删除**，只能 `unlock_file_to_drafts` 解锁退回草稿箱（只退当前版，历史副本随解锁清理），随后从草稿箱删除；表型分**逻辑删除**（进项目回收站，表保留）与**永久删除**（只在回收站执行）。项目回收站是唯一回收站入口（草稿箱栏；跨模块聚合 UI 待落地）。全局文件同样只能解锁退回项目草稿箱，全局回收站只收表型。

## 代码结构（2026-11-12 按真实目录回写）

> 旧版此表描述的是单文件时代（`src/model.rs` / `src/payload.rs` / `src/service.rs` …），
> 列出的 14 个文件有 13 个已不存在。下表以当前源码为准。

| 路径 | 职责 |
| --- | --- |
| `src/lib.rs` | 模块声明与对外公开面；只有被其它 crate 真实引用的模块是 `pub mod`，其余 `pub(crate)` |
| `src/model/` | 领域类型：`kind.rs`（种类 / 强度 / 状态）、`types.rs`（请求 / 回执）、`affordance.rs`（九个动作判据）、`promoted_from.rs`、`timeline.rs`（履历事件）、`semantics.rs`（业务断言 / 效力 / `semantic_hash`） |
| `src/payload/` | 本体层：`paths.rs`（越界守卫 / 回收站）、`transfer.rs`（搬运 / 只读 / 历史副本 / **stage → commit → discard** 暂存替换）、`inspect.rs`（sha256 / 体积）、`versions.rs` |
| `src/store/` | 索引层：`resource/{mod,read,write,trash}.rs`（登记 / 事务化写入 / 永久删除）、`rows.rs`（行模型与列映射）、`version.rs`、`folder.rs`、`tag.rs`、`timeline.rs`、`semantics.rs`、`refs.rs` |
| `src/service/` | 编排：`archive.rs`（首次 / 再归档）、`checkout.rs`（取回 / 还原）、`trash.rs`、`semantics.rs`（断言 / 效力门面）、`mod.rs`（标签 / 分组 / 履历埋点） |
| `src/jobs/` | 后台作业：`mod.rs`（通道 + `catch_unwind` + pending 守恒）、`archive/`、`version.rs`、`index.rs`、`refresh.rs`（快照 + 主动校验 + 暂存重放）、`global.rs`、`rename.rs`、`trash.rs` |
| `src/indexer/` | 三类孤儿扫描与人工确认修复（`scan.rs` / `repair.rs`） |
| `src/global/` | 全局实例双库：`register` / `list` / `promote` / `distribute` / `sql` / `table_name`（一版一张表的表名） |
| `src/present/` · `src/view_model/` | 纯函数呈现层与视图快照（零 I/O 零 GPUI） |
| `src/resource_view/` · `src/detail_view/` · `src/dialogs/` · `src/resource_host/` | 面板 / 详情 / 对话框 / 宿主动作适配 |
| `src/filter/` · `src/analysis.rs` · `src/preview.rs` · `src/dnd/` · `src/ui.rs` | 筛选数据层 / 分析表指纹 / 预览 / 拖拽 / 尺寸常量 |
| `src/tests/` · `src/**/tests.rs` · `tests/{panel_window,dialog_window}.rs` | 存储回归、服务、索引、窗口测试 |

**分层纪律**：`present` / `view_model` 是纯函数，视图不自己取数；`resource_host` 是唯一宿主边界，
由上游注入能力，本 crate 不认识草稿箱 / 工作台的内部类型。登记行、版本快照、履历事件同事务写入
（`BEGIN IMMEDIATE`）；本体替换走同卷暂存 + 原子 `rename`，索引先提交、替换失败保留暂存待重放。

依赖方向：`analytics_resource → engine, shared`（视图层另依赖 `gpui-kit` / `workbench_shell`）；
上游 `workbench` / `app` 通过 `resource_host` 注入宿主能力，**不存在反向类型依赖**。

## 迁移

迁移文件真实位置是 `crates/store/migrations/project_meta/`（旧版写成 `crates/engine/migrations/`，已失效）。

| 文件 | 内容 |
| --- | --- |
| `007_analytics_resources.sql` | v1 原样：7 张表 + 索引 + 两个 `updated_at` 触发器 |
| `020_analytics_resource_archive.sql` | 9 个语义列（`kind` / `content_hash` / `file_rel_path` / …）+ `file_rel_path` 部分唯一索引 + kind / 指纹索引；**不改 007**，老库直接升级 |
| `023_analytics_resource_payload_mode.sql` | `payload_mode`（copy 默认 / reference）+ `source_path` |
| `024_analytics_resource_tags_drop_tag_fk.sql` | 标签关联外键调整 |
| `025_scratchpad_tags.sql` | 草稿箱标签（编号归 scratchpad，**不是**本模块） |
| `027_analytics_resource_timeline.sql` | `analytics_resource_events`：资产履历事件（刻意无外键） |
| `028_analytics_resource_semantics.sql` | `analytics_resource_assertions` + `analytics_resource_lifecycle`：业务断言与效力 |
| `029_analytics_resource_refs.sql` | `analytics_resource_refs`：资产谱系 |
| `030_analytics_resource_checkouts.sql` | `analytics_resource_checkouts`：取回凭据（工作副本 → 来源存档 / 版本，支撑可回溯升版） |

> 编号跨目录先到先得：`project_meta` 没有 026（被 `global/026_analytics_resource_global.sql` 占用），
> 019 归 M8、021 / 022 归导航树。**027 / 028 / 029 / 030 已被本模块占用**（030 = 取回凭据），规划中的
> A6「已升级标记」需顺延到 **031**。四组新表都刻意不加外键：撤销归档会硬删资源行，
> 但履历 / 口径 / 谱系必须留下；清理走永久删除路径（见 `store/resource/trash.rs`）。

## 能力状态

> 2026-11-13 逐条对账：旧版「待补」列约有 15 条其实早已落地（F2 / 拖拽 / 折叠持久化 / 内容预览 / 默认分组 / 搜索别名标签 / 相邻差异 / 365 天清理等），已改标为「—」并注明落点；**仍挂着的都是明确后续刀 / 外部依赖 / 运营层取舍**，不是主链路缺口。

| 已实现 | 待补 |
| --- | --- |
| **取回凭据 / 可回溯升版**【2026-11-13】迁移 030 `analytics_resource_checkouts`：取回（含取回历史版本）记下「工作副本路径 → 来源存档 / 版本」；归档作业在 worker 里反查，命中即回到原存档升版（不改任何 `ArchiveRequest` 构造点、事件路径不做 I/O）；结案后时间线合成「从 vN 取回 → 生成 vM」节点，reference 档与已软删资源有安全边界 | —（版本回退 = 还原为当前版本、只追加不覆盖历史，已可用） |
| **产品语义层**【2026-11-12 五批】业务断言（选填，指标名 / 口径定义 / 业务时间窗 / 结论摘要）+ `semantic_hash` 与 `content_hash` 并列 + 效力四态（候选 / 正式 / 废弃 / 被取代，与物理完整性**正交**）+ 语义搜索 + 详情「业务口径」分区与效力条 + 主动校验（时间窗过期去重）+ 谱系 `archive_refs` | L5 治理度量（引用计数 / 认证率）属运营层、待产品定义；**取回→再归档回连已打通**（迁移 030），表单不预填但口径按 `resource_id` 保留 |
| **版本时间线**【2026-11-12】`analytics_resource_events` 落库 13 类事件；归档 / 再归档 / 取回 / 还原 / 撤销 / 口径变更 / 效力变更 / 谱系按时间合并展示；事件与索引同事务 | —（365 天保留期已落地；现共 13 类事件，含「取回后再归档 · 升版」回溯节点） |
| **作业通道与写入可靠性**【2026-11-12】`spawn` / `Runtime::new` 失败打日志、worker 每个 Job `catch_unwind`、`JobSender::send` 失败归还 pending 并写失败回执；登记 / 快照 / 索引 / 事件同事务；本体 staging 原子替换 | —（staging 刷新时幂等重放已落地）；进程崩溃到下次打开之间仍有窗口，刻意留人工确认、不静默覆盖 |
| **归档 / 取回 / 再归档闭环**【Phase 0 + 2026-10-13 第一刀】`ArchiveService`：本体 copy（默认，草稿保留为工作副本）+ 登记 + 指纹版本 + 事件；索引失败回滚副本；取回产出可写工作副本；**File 档两档粒度已落**：`PayloadMode`（copy / reference）+ 迁移 023 + `copy_in`/`resolve_source`/`copy_out_source` + 服务层按模式分派（归档 / 再归档 / 取回 / 撤销）；reference 的 UI 入口与打开校验留后续刀 | reference 的历史版本还原仍拒绝（走「取回该版本」）；**取回 → 改 → 回原档升 vN+1 已打通**（030 取回凭据） |
| **索引修复**【Phase 0 + Phase 3 第二刀 + P0.8】`IndexRepair`：三类孤儿（有文件无记录 / 有记录无本体 / 指纹不匹配）的扫描与人工确认修复；**对话框已接**（`dialogs/index_repair.rs` + `present::build_repair_rows` + 宿主接线）：三分组、行内动作（补登固定文件型 / **从回收站还原**（按原相对路径找条目）/ 删记录走确认 / 接受当前内容 / 打开版本历史），修完自动重扫换行 | 按目录批量补登（后续刀） |
| **版本历史**【Phase 3 第一刀】`dialogs/version.rs` + `present::build_version_rows` + 宿主接线：当前版本与历史行同列（副本缺失行露出来）；**行本体走 `list::ListItem`**（hover / 选中由组件承担，不再手搓）；**还原 = 生成新版本**（不原地回滚）、**取回该版本为草稿**、**删除内容副本**（`AlertDialog` 确认）；动作后宿主换行，不关窗；还原后的裁剪按设置项（见下行） | 按版本 / 时间 / 来源过滤检索（后续刀）；「最近 3 条」、相邻版本差异、血缘均已落地 |
| **文件资产解锁**【2026-11-26】`service/unlock.rs` + `global/unlock.rs`：文件型不能直接删除，`unlock_file_to_drafts` / `unlock_global_file_to_project` 把当前内容退回（全局则退回本项目）草稿箱（同名按 `-2/-3` 避让），再清资产本体 / 全局本体、登记链与全部历史副本；`trash_one` 与 `trash_global` 对文件型硬拒绝；作业 `UnlockFile` / `UnlockGlobalFile` + 详情危险区与右键入口 | 草稿箱扫描联动（当前靠扫描发现新文件） |
| **回收站**【P0.8】`engine::persistence::trash::ProjectTrash`（上提 + 中性化：来源是字符串 `origin`、不返回 M5 类型）+ `dialogs/trash.rs` + 宿主接线：移入回收站（单选 / 多选批量，本体进 `.RSmeta/trash`、登记行**软删**——还原能恢复别名 / 标签 / 指纹）/ 还原（`origin` 校验、同名避让并同步登记路径）/ 永久删除 / 清空（`empty_origin`，**只动本模块的条目**）；索引修复的「从回收站还原」走 `restore_archive_by_rel_path` | 回收站 TTL 自动清理、「打开原位置」入口（后续刀） |
| **面板**【Phase 1 + Phase 2 第一 / 二 / 三刀 + 2026-09-20 UI 收尾】`ResourcesPanel`：面板头（标题图标 + `＋ ▾` / `⋯` 四项）/ 工具栏（搜索·筛选（含**标签维**）·排序）/ **`List` 虚拟化行（kind 图标 + 强度徽标 + 尾部字段）+ 右键菜单** / **批量多选（单击 / `Ctrl` / `Shift` / 双击手势 + `Ctrl+A`，多选时单选项置灰、删除带数量）** / **分组折叠区（会话级折叠）** / 状态行 / 两种空态；`present.rs` 把索引行转成快照（含逐行详情与标签）；**workbench 接线已落**（`panels/resources.rs` 装配 + `services/resource_jobs.rs` 后台取数 + `components/resource_host.rs` 端口）；**右栏「存档详情」已接**（`RightPanel::Archive`，宿主观察面板实体做选中联动） | —（F2 重命名、标题点击折叠、折叠状态持久化均已落地） |
| **行态与控件口径**【2026-09-20 UI 收尾 / 开发方案 V1–V12】：行距钉在 `ui::ROW_HEIGHT`（24px，`list_row()` 压掉 `ListItem` 默认 `py_1`——组件只量一个样本行定全局行距）；悬停不覆盖选中；分组头展开指示走共用原语 `tree::disclosure_slot` + `disclosure_icon`（字符 `▸/▾` 退场）+ 色条 `primary`（与 M4 同角色）；**停在分组头上按 `Enter` 也能折叠 / 展开**；撤销 / 复制 / chip `×` 一律 `Button::ghost().xsmall()`（键盘可达 + 命中区 20px）；加载骨架走组件 `Skeleton`；固定描边统一 `ui::HAIRLINE` | 行尾「相对时间 → 悬浮显示绝对时间」要等 div 级 tooltip 基建（后续刀） |
| **详情面板**【Phase 1 + P0.8 + Phase 2 第一刀】`detail_view.rs` 只读信息区（基本信息 / 来源 / 版本 / 组织 + 需处理提示条）+ **标签分区（chips：× 去标 / ＋ 标签开对话框）** + 动作区（打开（只读）/ 取回）+ **危险区（移入回收站）**；右 Dock 转发渲染与空态；版本数与标签都由存储层一次查完 | —（内容预览、头部可编辑、分组选择均已落地） |
| **只读三重守卫**【Phase 1】①应用守卫（写入 `resources/` 直接拒，Phase 0）②**编辑器只读打开**（`editor::persist::open_file_read_only`，经 `OpenInEditorRequest` 带只读维度）③文件系统只读属性（辅助，失败只警告） | —（本体异常修复入口已随索引修复对话框落地） |
| **归档 / 取回**【Phase 1 + Phase 2 第八刀】`dialogs/{archive,checkout,pick}.rs`：对话框 + 校验 + 冲突提示（`resources/x-2.sql`）；**归档表单带显示名 / 别名 / 分组 / 标签 / 保留份数**，且这几项**真的落库**（标签按名找、找不到就建；分组走移动语义；附属项失败写进回执，不回滚归档）；workbench 侧真执行（`resource_host` + `resource_jobs` 的 `Archive` / `Checkout` 作业 + 重名避让 + 回执 + 顺手打开）；**草稿箱入口**（面板头 `＋ ▾` + 草稿多选，来源连接与出处自动带出）；**归档可撤销**（`undo_archive` + 5 秒撤销栏） | 批量归档逐项改名、`Ctrl+Z`（后续刀）；草稿箱右键入口与默认分组已落地 |
| **工具栏数据层**【Phase 1 + Phase 2 第一 / 四 / 六刀】`filter.rs`：搜索（名称 + 尾部，大小写不敏感）/ 种类多选（全选 = 不限）/ **标签多选（id、并集，带用量）** / 只看需处理 / **五个排序键**（名称 / 归档时间 / 更新时间 / 大小 / 版本：同键翻转方向、同键名称兜底、**缺值两个方向都排最后**；比的是原始时间戳与字节数，不是格式化尾巴）；**选过的排序会被记住**（`resources.default_sort`：面板点排序 → 宿主写设置 → 下次构造期注入） | —（搜索别名 / 标签 / 来源表已落地，另加业务口径语义搜索） |
| **标签**【Phase 2 第一 / 三刀】`tag.rs`（改名 / 删除补齐 + 批量查询）+ 打标 / 去标 + **标签对话框**（行内真 `Checkbox` 勾选 + 新建并打上 + 差集提交 + 行内 **⋯：重命名 / 删除**）+ 详情面板 chips（× 即去标，是语义按钮）+ **筛选菜单的标签维**，标签字典随快照下发（不另查库） | 标签颜色、按标签排序、批量打标签（后续刀） |
| **分组折叠区 + 管理**【Phase 2 第二 / 三 / 七刀】`folder.rs`（建 / 改名 / 删 + 移动语义）+ **列表按分组分区渲染**（全部分组 → 未分组 → 各分组，头带 2px 色条与计数）+ **折叠 / 展开**（**记忆到设置**：`resources.collapsed_groups` 按项目分桶；点一次写回全集）+ **管理入口**：行菜单「移动到分组 ›」（多选也走这条）+ 分组头右键（重命名 / 删除 / 新建）；计数从**当前可见行**现算 | —（拖拽到分组头已落地） |
| 领域类型（kind / 强度 / 状态 / 归档凭证）与本体层（守卫 / 搬运 / 只读 / 指纹 / 历史副本与裁剪 / 遍历） | — |
| **历史内容保留**【P2.4】`KeepVersions`（全留 / 只留元数据 / 保留最近 n 份）：设置页「资产库 › 历史内容保留」（`resources.keep_versions`，含 `-1` = 全部保留）+ 归档对话框本次覆盖（同样接 `-1`）；宿主主线程读设置 → 转领域类型 → 随归档 / 版本还原作业带入 | 将来新增写副本的路径必须同样接上 `KeepVersions`（约定） |
| **默认排序 + 分组折叠记忆**【P2.4】`resources.default_sort`（面板里点排序即写回；“默认排序” = 上次用的那个）与 `resources.collapsed_groups`（折叠态，按项目分桶）；两者都是构造期注入 + 用户动作写回（注入不写，避免回声） | —（默认分组已落地；设置架构 §14 Q7 的历史待确认项已关闭） |
| 回收站语义（项目级一套 + `origin` 归属；软删登记行保归属，永久删除连关联一起清） | 回收站自动清理策略（按时间 / 容量，后续刀） |
| 迁移 020 + 新列接入（写入 + 读取 + 按本体路径查重） | `kind` 过滤的**存储层**入口（面板已能按 kind 筛可见行，SQL 未接，后续刀） |
| **全局一版一张表**【刀 4 + 2026-11-25】`global/table_name.rs`：升级落 `res_<描述>_<时间戳>`、旧版表保留；`analytics_table` 记实际表名（迁移 `global/036` 给老行回填成资产名，读取面另有 `name` 回退）；升级冲突只剩登记层，覆盖只软删登记；全局表型行打开走 `SampleSource::on_global`；**全局实例删除已落**（`global/delete.rs` + 详情危险区 + 列表右键「从全局实例删除…」，见下） | — |
| **表型 `KeepVersions`**【刀 6】`prune_version_tables`：升级成功后按设置里的保留策略裁掉更早版本的**表**；当前版本那张是「本体」永远留，`keep` 数历史份数（`MetadataOnly` 只留当前一张）；**登记行与版本快照一条不删**；内容被清的那版分发时报「已被保留策略清理」而不是 DuckDB Catalog Error；裁剪失败只记日志，不回滚升级 | — |
| **表型动作判据 + 回收站闭环**【2026-11-25】`ActionContext` 增 `has_table`，「有本体」= 文件或分析库表：打开 / 升级 / 详情删除走 `has_body`，取回只认文件本体（表型置灰并给「去 SQL 里改数据」提示）；回收站 `tbl:` 条目只软删登记、分析库表保留，还原 / 永久删除按 `has_payload` 分派提示；**还原前只读挂载项目分析库预检表是否还在**，撞锁拿不到就不拦还原 | — |
| **全局实例删除 → 回收站**【2026-11-26】`trash_global` 只软删当前活跃行、表与版本快照保留，manifest 落 `data/trash/global/`；`restore_global` 先校验表在再复活登记；`purge_global` 才逐张 `DROP` 同名各版表并清单 / 登记 / 分发回执；UI 动作改「移入全局回收站…」；旧 `delete_global` 暂保留硬删语义兼容既有调用 | 全局回收站列表 / 恢复入口 UI、TTL |
| **全局文件本体与版本恢复**【2026-11-26】`global/payload.rs`：升级时把原文件复制进 `data/system/resources/<rel>`，sha256 / 体积按实际字节；历史副本落 `resources/versions/<asset_key>/vN/`；`restore_global_file_version(name, N)` 校验副本 hash 后以旧字节再生成新版本（不原地回滚）；`KeepVersions` 同步裁副本（当前版与被 pin 的版本不裁）。详情全局行在文件本体存在且版本 > 1 时提供「恢复上一版文件…」；脚本档 `kind = file`，数据文件仍 `kind = analysis` | 任意版本选择 UI、独立文件资产行 / 派生表拆两条、旧表资产不伪造本体 |
| **全局文件回收站**【2026-11-26】`trash_global` 把当前本体与历史副本移入 `data/trash/global/<id>/<time>/payload/`，`restore_global` 移回，`purge_global` 清掉；manifest 增 `file_rel_path` / `payload_dir`，旧条目靠默认值兼容；`list_global_trash` 按条目最新时间列出，`empty_global_trash` 逐条永久删除并跳过被 pin 的条目，返回 `(purged, skipped)` | 回收站列表 / 恢复 / 永久删除 UI、TTL、清空遇 pin 的文案 |
| **全局表历史版本恢复**【2026-11-26】`restore_global_table_version(name, N)` 从快照取旧表名，校验旧表还在后 `CREATE TABLE rds_global.<新表> AS SELECT * FROM rds_global.<旧表>`，软删旧活跃行、登记新版本（版本号 +1）；旧表 / 旧登记 / 快照全留；旧表被裁或快照缺表名明确拒绝；文件型走 `restore_global_file_version`；详情全局表型行在版本 > 1 时提供「恢复上一版表…」 | 任意版本选择 UI |
| **来源链浏览 + 分发谱系**【刀 3 + 2026-11-25】`origin_kind` + `origin_ref` 记**直接上游**那一跳（全局行 `project` + 资源 id/版本；项目行 `global` + 实例/版本），详情「来源」区多一行「上游」；详情动作区「查看来源链…」按 id 在面板快照里逐跳往上追（最多 8 跳、防环；上游不在快照时如实灰显并注明可能已删除或在别的项目）；分发成功后补记 `derived_from`（项目产物 → 全局上游，失败只告警不回滚分发） | 升级路径尚无 `derived_from`（产物落在全局库，项目侧没有对应行可挂） |
| **表型升级**【刀 5c + 2026-11-26】本体是项目分析库里一张表的存档也能升级：所有权收口后项目库与全局库同根，项目会话直接 `CREATE TABLE rds_global.<版本表> AS SELECT * FROM analytics.<源表>`；冲突 / 覆盖 / 版本号 / 登记 / 裁剪与文件型升级同口径；不再导出 parquet、不再先锁进程单例 | — |
| **数据对象面板的单一升级路径**【刀 7 / 刀 12】场景 3 在面板里闭环：会话表（结果集 / 洞察产物）→「存为表」（整表搬进项目库，成功顺手重取面板清单）。面板上**只有这一条升级路径**——「升级」的唯一含义是临时对象 → 项目持久表；全局升级入口在资产库：详情动作区与列表右键「升级到全局…」（`detail_actions::request_promote_to_global`），刀 12 撤掉了面板里那个第二个入口（同一动作两处语义会互相打脸） | 面板不再提供全局升级；文件型与表型资产都走资产库那两条入口 |
| 行映射从 v1 的 4 份收敛为 1 份；测试库跑齐 007 + 020 | —（测试库现跑齐 007 + 020 + 023 + 024 + 027–030） |
| 继承缺陷修复：分页除零与负数、`LIKE` 转义、连接嵌套、更新无事务、影响 0 行不报错、`parent_version_id` 语义、JSON 解析双策略、乱码副本名 | — |

## 设计与验证

- 设计（权威）：`docs/architecture/analytics_resource/` —— `README.md`（模块入口）· `analytics-resource-architecture.md`（语义裁决与数据流）· `analytics-resource-prototype-design.md` + `analytics-resource-prototype.html`（原型）· `analytics-resource-dev-plan.md`（进度与任务）· `analytics-resource-user-guide.md`（使用手册）。**这些是阶段快照**：其中的测试数、迁移编号与待办状态停在 2026-10 / 11，当前状态以本 README、`D:\RdataStation\RDS\M6-终审问题清单-2026-11-12.md` 为准。
- 验证（2026-11-25 实测）：`cargo check -p rds-analytics-resource --all-targets -j 1` 零错误零警告；`cargo test -p rds-analytics-resource -j 1` → **279 单测 + 10 对话框 + 29 面板 = 318 项全绿**；下游 `cargo test -p rds-workbench -j 1 --lib` → **219 项全绿**，`--test ui_contract` → 10 项全绿，`cargo test -p rds-ui-guard -j 1` → 4 项全绿。全仓 `cargo test --workspace` 链接耗时较长；**CI 不跑产品测试**（`ci.yml` 只做 `clippy-all` 与单个 ui-guard 用例），合并前必须本地跑 `cargo test-all`。
- **命令约定**：全量编译/测试必须限制并发（`cargo test-all` / `cargo check-all` 别名，含 `-j 2` 与 `RUST_MIN_STACK`）——并发链接重型 crate 会耗尽内存（DuckDB 已改动态链接）。
