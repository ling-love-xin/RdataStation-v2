# 资产库（M6）收口方案：问题点 · 解决方案 · 实施方案 · 优劣（2026-10-25）

> **落地状态注记（2026-10-28 回写）**：批1–4（§2 八项）已全部实现并通过验证（详见 gap-review §5）；追加的 gpui-kit 组件化 P1–P5 已处理完毕——P1a/P1c/P3 落地（`detail_view.rs` / `resource_view.rs`）、P2a/P2b/P4/P5 实证评估不落地、P1b 确认已用 `Tag`，提交 `ecbf0f21`。验证：`rds-analytics-resource` lib **170 passed** / dialog_window **10** / panel_window **25**，0 failed。原型 HTML 同步见 §批5（本文档对应内容已并入 `analytics-resource-gap-review.md` §3/§5）。

> 用途：对 `crates/analytics_resource` 与宿主接线（`crates/workbench` 资源面板 / 宿主 / job 层）现存问题一次性盘点、分级、出方案，经裁决后按批次实现。
> 证据口径：所有「现状」均来自本轮工作区源码实读；定位锚点 = `文件:符号`（行号随代码演进会漂，以符号名为准）。规格依据 = `analytics-resource-prototype-design.md` §12.1.1 / §12.2.1 / §13。
> 基线（盘点时实测）：`cargo check --workspace` 绿；`cargo test -p rds-analytics-resource --lib` **169 passed**；`cargo test -p rds-workbench --lib` **96 passed**。
> 状态口径：**本轮实现**（M6 内闭环、规格就绪）/ **外部阻塞**（依赖其他 crate 或后续批次）/ **待决策**（规格就绪但需用户拍板）/ **跨 crate 低优先**（列入后续批次，本轮不动）。

## 0. 结论（一页）

1. 数据层（`PayloadMode` / 迁移 023 / `copy_in` / `resolve_source` / 服务层分模式分支 / 双实例服务层 `promote` · `promote_overwrite` · `distribute` · `distribute_overwrite`）已就位；本轮缺口集中在 **UI 接线、冲突询问对话框、详情与空态分流、失效处置** 四类。
2. 本轮实现 **8 项**：A1 升级/分发冲突三选对话框；B1 归档对话框 copy/reference 单选；B4 详情面板 reference 差异；B2 reference 打开指纹校验 + 失效处置；B3 移入回收站确认 + 永久删除模式文案；A5 全局实例空态分流；A4 全局行轻量详情 + 实例行 + 动作收敛；D2 索引异常首次进入顶部一次性提示条。
3. **不做项**分三类如实标注：外部阻塞（E1 分析表升级 → M8；D4/D7/D8 跨 crate）、待决策（E2 data 视图；E3 全局清单口径；A6 项目行「已升级」标记；D3 详情最近 3 版）、跨 crate 低优先（D1/D5/D6）。
4. 实施顺序按「模型层 → 对话框 → 宿主动作 → 视图收尾 → 原型 HTML 同步 → 文档回写 → 全仓验证分批提交」推进（见 §4）。

## 1. 问题全景表

| 编号 | 问题点 | 现状证据（符号定位） | 规格依据 | 本轮裁决 |
| --- | --- | --- | --- | --- |
| A1 | 升级 / 分发同名冲突只回 Failed 文案，覆盖 / 改名 / 取消询问对话框缺失 | `resource_jobs.rs::run_promote`（冲突 → `Failed` +「覆盖询问对话框待补」）；`run_distribute` 同款；`global.rs::promote_overwrite` / `distribute_overwrite` 已就绪未接线 | §13.3 冲突「覆盖 / 改名 / 取消」；§13.6 同 | **本轮实现** |
| B1 | 归档对话框无 copy/reference 模式控件；三条入口硬编码 `Copy` | `dialogs/archive.rs::ArchiveDialogSeed/ArchiveDialogResult` 无模式字段；`resource_host.rs` 草稿单选 / 草稿多选 / 本地文件三处 `PayloadMode::Copy`；`service.rs::archive_new` 已分模式 | §12.1.1 ① | **本轮实现** |
| B4 | 详情模型不带模式 / 源路径 / 实例；reference 路径、大小、只读文案与 copy 混用 | `detail_view.rs::ArchiveDetail`（无 `payload_mode` / `source_path` / `scope`）；`present.rs::to_detail` 未带入；`detail_rows` 路径行与只读文案固定 copy 口径 | §12.1.1 末段「详情面板差异」 | **本轮实现** |
| B2 | reference 打开无指纹校验；失效条与「接受当前内容 / 从历史还原」处置缺失 | `resource_host.rs::request_open`（File 档直接打开，无校验）；`payload.rs::content_hash` / `resolve_source` 已存在；`indexer.rs` 已跳过 reference 扫描（懒校验口径一致） | §12.1.1 ② | **本轮实现** |
| B3 | 移入回收站无确认框；回收站条目不带模式，永久删除 / 还原文案 copy/reference 混用 | `resource_host.rs::request_delete`（直接 `enqueue_trash`，无确认）；`dialogs/trash.rs::TrashRow` 无模式字段；永久删除文案固定「本体将从回收站里彻底删除」；服务层 `move_to_trash` / 还原 / purge 已分模式 | §12.1.1 ③ | **本轮实现** |
| A5 | 空态文案与归档按钮未按实例分流；切到全局空列表仍摆「从草稿箱 / 本地文件归档」 | `resource_view.rs::render_empty`（固定两按钮）；`self.instance: InstanceFilter` 已存在 | §13.2 空态「全局分析库暂无资源表」 | **本轮实现** |
| A4 | 全局行不进详情映射，点选详情空态；基本信息无「实例」行；动作区未按实例收敛 | `present.rs::build_snapshot`（details 只装项目 resource）；`selected_detail` 从 `snapshot.details` 取，全局行 = `None`；行菜单全局项已置灰（`resource_view.rs` 1179 / 1192 / 1230 / 1302） | §13.2「详情面板基本信息区加实例行」 | **本轮实现** |
| D2 | 索引异常首次进入无顶部一次性提示条（仅状态行「修复…」按钮） | `resource_view.rs` 状态行 `has_issues` 口径（`counts.has_issues()`，全局实例下隐藏入口） | 原型 §5 落地注 | **本轮实现**（纯视图层，会话内不重复弹） |
| E1 | 分析表型存档升级被阻塞（读取 / 重建路径未接） | `resource_host.rs::request_promote_to_global`（无 `payload_rel_path` → notice「随 M8 后续批次接入」）；`request_view_stats` 同款拦截 | §13.3 | **外部阻塞**：M8 批次，面板拦截 notice 已就位 |
| E2 | A2 data 视图（analysis 档打开 = 桥接结果集二次分析）未落地 | 仅 file 档报告视图（T20）；`prototype-design.md` §12.2.1 规格就绪 | §12.2.1 | **待决策**：本期做 / 不做由用户拍板；不做则文档保持「当前仅 file 档」标注 |
| E3 | 全局清单口径：现状 = 全局 SQLite 镜像登记表；旧草案 §13.2 = `SHOW TABLES` 全库扫描 + 内部表白名单 | `global.rs::list_global_resources`（`scope='global' AND deleted_at IS NULL` 登记查询） | §13.2【待确认】 | **待决策**：本轮维持镜像表现状，口径裁决留档 |
| A6 | 项目行无「已升级」标记；`promoted_from` 未记全局升级关系 | 升级成功后项目行无回写；`global.rs::register_global` 单向记来源 | §13.3「项目行保留并标记已升级」 | **待决策 / P2**：需迁移 027 + 回写 + 行尾徽标；本轮不做，重复升级由 A1 冲突拦截兜底 |
| D1 | 空库「草稿箱有 N 个文件值得归档」建议行 | 需草稿箱侧给可归档计数（跨 crate） | 原型 §5 | **跨 crate 低优先** |
| D3 | 详情版本区「最近 3 条」明细 | 快照只带 `history_count` 计数；版本明细仅版本对话框按需加载 | 原型 §3.1 | **待决策 / P2**：需「每存档最近 N 版」批量取数 + 快照带行，工作量中等 |
| D5 | 行内 hover 常驻动作 | `resource_view.rs` 注释「hover 版随详情面板批」 | 原型 §2.3 | **跨批次**：归详情面板动作集批 |
| D6 | 回收站自动清理策略（TTL）/ 条目「在原位置打开」 | `dialogs/trash.rs`；策略属产品决策 | 原型 §4.4 | **待决策**：TTL 策略产品拍板；「在原位置打开」小项留后续 |
| D4 | 取回侧记 `derived_from_resource_id` | 需草稿箱侧加字段（M5） | 原型 §4.2 | **外部阻塞**：M5 批次 |
| D7 | 详情面板宽度记忆（`h_resizable`） | 依赖 M4 属性面板接线；当前固定 `RIGHT_DOCK_WIDTH` | 原型 §3.1 | **外部阻塞**：M4 批次 |
| D8 | 编辑器「以结果区打开数据文件（只读）」API | editor crate 侧能力；现由洞察取样入口承担 | 原型 §2.3 / dev-plan P4.4 | **外部阻塞**：editor 批次 |

## 2. 本轮实现项详案

### A1 升级 / 分发同名冲突三选对话框（覆盖 / 改名 / 取消）

**现状证据**

- `resource_jobs.rs::run_promote`：`InstanceOpError::Conflict { name }` → `OpOutcome::Failed`，文案「全局已有同名资源…覆盖询问对话框待补」；`run_distribute` 同款。
- 服务层覆盖路径已就绪：`global.rs::promote_overwrite`（删全局旧表 + 软删旧登记后重走 `promote`，`overwritten=true`）；`global.rs::distribute_overwrite`（删项目旧表 / 旧文件 + 软删项目登记后重走 `distribute`）。
- 回执事件路径：`panels/resources.rs::apply_resource_op`（`&mut Context<Self>`，可经 `cx.window()` 开窗），现有 `Promoted` / `Distributed` / `Failed` 分支已就位。
- 分发项目侧重名检测现状：`register_project_table` / `register_project_file` 的 `next_version` 查询只按 `name` 取 `MAX(version)+1`，未先探「同名未删登记 / 项目表存在」；冲突拦截靠 `distribute` 开头的 DuckDB 表存在性与文件存在性探测，与 `promote` 的「SQLite 登记 + DuckDB 表」双保险不同构。

**问题点**

1. 冲突被降级成 `Failed` 状态栏文案，服务层覆盖路径空转；用户只能手动改名重发，覆盖诉求没有出口。
2. 分发项目侧缺登记层重名检测，脏状态（表在 / 登记不在、登记在 / 表不在）下行为不确定。
3. 分发改名无通道：当前 `distribute` 项目侧名字 = 全局 `name`，不支持目标改名。

**候选方案与优劣**

| 方案 | 做法 | 优点 | 缺点 / 风险 | 裁决 |
| --- | --- | --- | --- | --- |
| ① 独立 `OpOutcome::Conflict` 变体 + 事件路径三选对话框（覆盖 / 改名 / 取消） | job 冲突不再转 `Failed`，带回操作上下文；事件路径拦截后开窗；覆盖 → `*_overwrite`；改名 → 新名重排原路径；取消 → 仅关窗 | 语义清晰；上下文可回传；与归档冲突体验一致；服务层已备的覆盖路径接通 | 新增变体 + 两个 enqueue 加 `overwrite` / `target_name` 参数；改名后二次冲突需再弹（有界循环，用户取消即止） | **采纳**（规格 §13.3 / §13.6 定稿） |
| ② 维持 `Failed` 文案，提示用户手动改名重发 | 零结构改动 | 改动最小 | 违反「不静默覆盖」裁决的对称要求；覆盖路径空转；体验断裂 | 否决 |
| ③ 工作线程内直接覆盖 | 冲突时自动走 overwrite | 交互最少 | 违反「冲突必须询问、不静默覆盖」既定裁决 | 否决 |

**实施方案（落点与步骤）**

1. **job 层**（`crates/workbench/src/services/resource_jobs.rs`）：
   - 新增 `ConflictOp { Promote, Distribute }` 与 `OpOutcome::Conflict { op, name, context: ConflictContext }`；`ConflictContext` 升级带回 `PromoteRequest` 全量上下文，分发带回全局 `name`。
   - `enqueue_promote(root, request, overwrite: bool)`；`enqueue_distribute(root, name, target_name: Option<String>, overwrite: bool)`。
   - `run_promote`：`Conflict` → `OpOutcome::Conflict`；`overwrite=true` 调 `gstore.promote_overwrite(&request)`；`run_distribute` 同构，`overwrite=true` 调 `distribute_overwrite`。
   - 分发项目侧重名检测补齐双保险（与 `promote` 同构）：SQLite 未删登记存在性 + DuckDB 表 / 文件存在性，四种脏状态组合均返回 `Conflict`。
2. **服务层**（`crates/analytics_resource/src/global.rs`）：
   - `distribute` / `distribute_overwrite` 增 `target: Option<&str>`（`None` = 沿用全局名）；项目表名、文件 rel_path、登记 `name` 统一用 target；`register_project_table` / `register_project_file` 同步收 target 参数。
   - 项目侧重名检测：`name = target`（或全局名）口径下查未删登记 + 探表 / 文件存在。
3. **事件路径**（`crates/workbench/src/panels/resources.rs::apply_resource_op`）：
   - 拦截 `OpOutcome::Conflict`：`cx.window()` 开三选对话框（复用 `open_alert_dialog` 组件形态，确认框用法见 `dialogs/trash.rs:411-437` 实证）。
   - 文案：升级「全局已有同名「X」」；分发「本项目已有同名「X」」。三按钮：**覆盖**（danger 中性，确认后 `enqueue_*` 带 `overwrite=true`）/ **改名**（弹出命名输入框，新名非空后重排原路径；二次冲突再弹同一对话框）/ **取消**（关窗）。
   - 覆盖成功回执：`Promoted { overwritten }` / `Distributed { overwritten }`，回执文案区分「已覆盖旧版本」与「已升级 / 已分发」。
4. **宿主**（`resource_host.rs`）：`request_promote_to_global` / `request_distribute_from_global` 保留默认（`overwrite=false`、`target=None`）；覆盖 / 改名重发由事件路径对话框回调触发，不经过行菜单二次入口。

**验证**

- 单测：`run_promote` / `run_distribute` 冲突 → `Conflict` 变体；`overwrite=true` 走覆盖路径；分发改名 target 落项目表名 / 文件名 / 登记名一致；脏状态四组合均判冲突。
- 窗口测试：三选对话框渲染（覆盖 / 改名 / 取消三按钮就位）；改名输入框校验空名拦截。
- 手动路径：同名升级 → 弹窗三选；覆盖后全局版本接续旧链；改名后新名独立成链。

### B1 归档对话框 copy / reference 模式单选

**现状证据**

- `dialogs/archive.rs::ArchiveDialogSeed`（`source_label` / `rel_path` / `name` / `source_connection` / `conflict` / `groups` / `group_id` / `initial_tags`）与 `ArchiveDialogResult`（`name` / `alias` / `tags` / `group_id` / `keep_versions`）均无模式字段。
- `resource_host.rs` 三处入口硬编码 `PayloadMode::Copy`：草稿单选（`request_archive_from_drafts` 单选分支）、草稿多选、本地文件（`request_archive_from_file`）；洞察归档 T20 固定 copy（规格 §12.1.1 ① 末段：快照不提供 reference）。
- 服务层已分模式：`service.rs::archive_new`（copy 走 `copy_in`；reference 登记源绝对路径、不搬本体）；`archive_into_existing` reference 分支从源路径存版本副本；`payload.rs::copy_in` / `resolve_source` 就位。

**问题点**

1. 数据层两档语义已落地，UI 没有选择面：用户无法在归档时选 reference 省盘。
2. 模式切换后的表单呈现（目标位置 vs 源位置、note 文案）缺失。
3. 多选归档无逐个模式选择的位置（规格：多选跟随默认 copy）。

**候选方案与优劣**

| 方案 | 做法 | 优点 | 缺点 / 风险 | 裁决 |
| --- | --- | --- | --- | --- |
| ① 对话框内两枚 `Radio` 单选（copy 默认 / reference），切换表单呈现与 note | seed 增 `allow_reference` / `source_abs_path`；result 增 `payload_mode`；模式状态住 `Rc<RefCell<PayloadMode>>`（与 `group_id` 同构，适配 `Fn` 重建） | 一次说清两档差异；与规格 §12.1.1 ① 一致；组件复用 `Radio`（仓库先例 `insight/src/view/multi.rs:251`） | seed / result / inputs 三处加字段；表单切换重渲染 | **采纳** |
| ② 归档入口拆两个按钮（「复制归档…」/「引用归档…」） | 表单最简，模式在入口选定 | 对话框状态少一层 | 草稿箱 / 本地文件两个入口 × 两档菜单膨胀；用户先选档再开窗，路径绕；规格明确要求对话框内单选 | 否决 |

**实施方案（落点与步骤）**

1. **对话框模型**（`dialogs/archive.rs`）：
   - `ArchiveDialogSeed` 增：`allow_reference: bool`、`source_abs_path: PathBuf`、`default_mode: PayloadMode`。
   - `ArchiveDialogResult` 增：`payload_mode: PayloadMode`。
   - `ArchiveDialogInputs` 增：`mode: Rc<RefCell<PayloadMode>>`；`build_inputs` 按 `seed.default_mode` 预填；`submit_archive` 回传当前模式。
2. **表单渲染**（`open_archive_dialog_with`）：
   - 「归档模式」分区两枚 `Radio`：copy「复制进存档目录」/ reference「引用源文件（省盘）」；`allow_reference=false` 时 reference 禁用 + 说明「洞察快照不支持引用」。
   - `readonly_block` 按模式切换：copy 显示「目标位置」`resources/<rel>` + note「草稿保留为工作副本，可继续编辑；再次归档将生成新版本」；reference 显示「源位置」只读绝对路径（`source_abs_path`）+ note「本体留原位，只存元数据（地址 + 指纹 + 版本）；源变更后快照失效」。
   - 切换只改呈现与 note，确认按钮文案不变（「归档」/ 冲突时「改名归档」）。
3. **宿主入口**（`resource_host.rs`）：
   - 草稿单选、本地文件：`allow_reference=true`，`source_abs_path` = 源绝对路径，`default_mode=Copy`；`ArchiveRequest.payload_mode` 随 `result.payload_mode`。
   - 草稿多选：固定 copy（无逐个选择面），维持现状。
   - 洞察归档（T20 路径）：`allow_reference=false`，固定 copy，不动。
4. **规格边界**：reference 仅对 File 档有意义；Analysis 档（数据文件探测）恒为 copy——对话框在 `kind=Analysis` 候选时禁用 reference（与 `ArchiveRequest` 注释口径一致）。

**验证**

- 对话框窗口测试：copy / reference 两态表单呈现与 note 文案；reference 禁用态（洞察归档种子）；submit 带回正确 `payload_mode`。
- 单测：`submit_archive` 模式字段往返；`parse_tags` / 保留策略既有测试不回归。
- 手动路径：本地文件归档选 reference → 登记行 `payload_mode='reference'` + `source_path`；`resources/` 下无副本。

### B4 详情面板 reference 差异（模式 / 源路径 / 大小 / 只读文案）

**现状证据**

- `detail_view.rs::ArchiveDetail`：无 `payload_mode` / `source_path` / `scope`；`payload_rel_path` 单字段承担 copy 路径。
- `present.rs::to_detail`：未带模式与源路径；`detail_rows` 基本信息「只读」行固定「归档后不可写，需取回编辑」；路径行固定 `payload_rel_path` 口径。
- 行模型 `models.rs::AnalyticsResource` 已含 `payload_mode` / `source_path` / `scope` 列，数据不缺，缺装配。

**问题点**

1. reference 行在详情里显示的是 copy 口径路径与文案，用户误以为本体在 `resources/`。
2. 源文件大小未展示；实例归属（项目 / 全局）无行（与 A4 共用字段）。

**候选方案与优劣**

| 方案 | 做法 | 优点 | 缺点 / 风险 | 裁决 |
| --- | --- | --- | --- | --- |
| ① 详情模型加 `payload_mode` / `source_path` / `scope`，`to_detail` 从行模型带入；`detail_rows` 按模式分派路径 / 大小 / 只读文案 | 纯数据装配，渲染期零 I/O（大小由宿主装配前读好推来，与 `previews` 同纪律） | 信息与本体位置一致；字段一次补齐供 A4 复用 | 装配层多带三个字段；测试构造点同步补字段 | **采纳**（规格 §12.1.1 末段） |
| ② 渲染期读源文件算大小 / 解析路径 | 模型少带字段 | 违反「详情面板纯渲染、零 I/O」纪律（`detail_view.rs` 模块头注释）；源不存在时渲染期报错不可控 | 否决 |

**实施方案（落点与步骤）**

1. **模型**（`detail_view.rs::ArchiveDetail`）：增 `payload_mode: PayloadMode`、`source_path: Option<String>`、`scope: RowScope`。
2. **装配**（`present.rs::to_detail`）：从 `resource.payload_mode` / `source_path` / `scope` 带入；reference 大小 = 宿主装配前读源文件 `format_size`（读不到给 `—`，不产生行或产生 `—` 行，与现有 `size_label` 空串口径对齐）。
3. **渲染**（`detail_view.rs::detail_rows`）：
   - copy：「物理存储路径」= `resources/<rel>`；「大小」= 副本大小；只读行 =「归档后不可写，需取回编辑」。
   - reference：「物理存储路径」= 源绝对路径（`source_path`）；「大小」= 源文件大小（读不到 `—`）；只读行 =「引用型：本体在源位置，源变更后快照失效」。
   - 「实例」行随 A4 同批加入（项目 / 全局）。
4. **测试构造点**：`detail_view.rs` 与 `present.rs` 既有测试 fixture 补新字段默认值（Copy / None / Project）。

**验证**

- 单测：`detail_rows` copy / reference 两态行文案与路径分派；`to_detail` 字段往返。
- 窗口测试：reference 存档详情面板路径 / 大小 / 只读文案正确。

### B2 reference 打开指纹校验 + 失效处置条（接受当前内容 / 从历史还原）

**现状证据**

- `resource_host.rs::request_open`：File 档直接 `request_open_in_editor_read_only(path)`，无指纹校验；`payload_path`（322-338）只按 `payload_rel_path` 经 `PayloadStore::resolve` 解析，未处理 reference 源路径。
- 校验工具已就位：`payload.rs::content_hash`（482，字节 sha256）、`payload.rs::resolve_source`（169，绝对路径 + 存在性守卫）；`indexer.rs:141-143` 扫描已跳过 reference 行（懒校验口径一致，不进索引异常统计）。
- 再归档服务路径已就位：`service.rs::archive_into_existing`（368-379）reference 分支从源绝对路径存旧版本副本；`request_version_history` 端口已存在（`ResourcesHost::request_version_history`）。

**问题点**

1. reference 打开时不校验指纹：源被改后用户读到的是新内容，面板仍显示旧版本快照，版本语义失真。
2. 失效后没有处置入口：规格要求「接受当前内容」（当前源生成新版本）与「从历史还原」（版本历史对话框）两个动作，现状详情面板只有 copy 口径的打开 / 取回 / 删除。
3. 源不存在（被删 / 移动）需要复用 Missing 口径，不能静默打开失败。

**候选方案与优劣**

| 方案 | 做法 | 优点 | 缺点 / 风险 | 裁决 |
| --- | --- | --- | --- | --- |
| ① 打开动作懒校验（规格 §12.1.1 ② 定稿）：`request_open` 对 reference 先 `resolve_source` → 读源算 sha256 与登记指纹比对；一致打开，不一致不打开并置失效槽位，详情面板顶部 warning 条 + 两动作 | 不进索引状态统计（与 `indexer.rs` 跳过口径一致）；触发点 = 用户主动打开，语义贴合 reference 本质；校验工具复用现有 `content_hash` | 打开路径多一次读文件算哈希；失效状态需宿主持槽位按 `resource_id` 推给详情面板；切换项目 / 关闭面板要清槽位 | **采纳** |
| ② 索引扫描期校验 reference 源指纹 | 失效能在状态行集中暴露 | 违反规格「懒执行（打开时才算）」；reference 源在项目外，扫描期不可达；会把每条引用报成异常，污染 `has_issues` 统计 | 否决 |

**实施方案（落点与步骤）**

1. **宿主路径解析**（`resource_host.rs::payload_path`）：按详情 `payload_mode` 分派——copy 走 `PayloadStore::resolve`（现状）；reference 走 `PayloadStore::resolve_source(&source_path)`；拿不到给回执，不静默。
2. **打开校验**（`resource_host.rs::request_open`）：
   - reference 档：`resolve_source` 定位源 → `content_hash(&path)` 与 `detail.content_hash` 比对。
   - 一致 → 正常打开（编辑器只读，与 copy 同路径）。
   - 源不存在 → 复用 Missing 口径（状态行 / 详情缺失提示），不置失效条。
   - 存在但指纹不一致 → **不打开**；宿主在共享状态槽位置 `reference_stale: {resource_id}`，刷新详情面板；回执一句「源已变更，快照失效」。
3. **失效条与动作**（`detail_view.rs` 详情头部，`alert_line` 同族样式）：
   - stale 行顶部 warning 描边条「源已变更，快照失效」，两枚动作按钮：
     - **接受当前内容** → 新增宿主端口 `ResourcesHost::accept_reference_current(detail, window, cx)`：以当前源绝对路径为 `source_path`、`existing_resource_id = detail.id` 走再归档（reference 分支，`archive_into_existing` 服务路径），新指纹 / 新版本落库；成功后清 stale 槽位、刷新快照。
     - **从历史还原** → 复用 `request_version_history(detail.id, …)`（版本对话框内挑一版取回，既有路径）。
4. **状态纪律**：stale 槽位按 `resource_id` 存于宿主共享状态；项目切换 / 面板关闭 / 接受当前内容成功后清除；**不进** `counts.has_issues()` 与索引修复对话框（规格：懒校验不进索引统计）。
5. **模型配套**：`ArchiveDetail` 增 `payload_mode` / `source_path` / `scope`（与 B4 同批落地）；stale 判定由宿主槽位按 id 注入详情渲染，不进快照持久层。

**验证**

- 单测：`resolve_source` 绝对路径守卫 / 不存在报错（既有）；再归档 reference 分支旧版本副本落盘（服务层既有测试口径）。
- 窗口测试：stale 条渲染 + 两动作按钮就位；一致指纹不出现失效条。
- 手动路径：归档 reference → 改源文件 → 打开 → 不打开 + 失效条；「接受当前内容」→ 版本 +1、指纹更新；「从历史还原」→ 版本对话框打开；源删除 → Missing 口径提示。

### B3 移入回收站确认 + 回收站条目模式文案区分（含服务层 reference 软删路径）

**现状证据**

- `resource_host.rs::request_delete`（733-753）：只读守卫后直接 `enqueue_trash`，**无确认对话框**。
- 服务层 `service.rs::move_to_trash`（618-704）：reference 分支**拒绝**移入回收站（656-664，报错「不支持移入回收站；删除请走删除登记记录」）；copy 搬本体进 `ProjectTrash` + 软删登记行。
- 回收站清单现状：`resource_jobs.rs::load_trash`（1071-1089）只读 `PayloadStore::trash().list()` 文件条目；`present.rs::build_trash_snapshot`（146-185）构造 `TrashRow`（无模式字段）；`dialogs/trash.rs::TrashRow`（34-47）无 `payload_mode`；永久删除确认文案（418）固定「本体将从回收站里彻底删除，不可恢复；登记记录一并删除」。
- 还原 / 永久删除服务路径：`restore_archive_from_trash`（717-782，`trash.restore` 搬回本体 + `undelete_archive`）；`purge_archive`（818-839，`trash.purge` 删本体 + `purge_deleted_row`）；均按文件条目 `trash_id` 工作。

**问题点**

1. 移入回收站是不可逆动作前的关键确认，现状直接执行；copy / reference 的本体命运不同，文案必须区分（规格 §12.1.1 ③）。
2. 服务层拒绝 reference 进回收站，与规格「reference 可移入回收站（只删登记行，本体留原位）」不一致；reference 目前没有合规的删除通道。
3. 回收站清单缺 reference 软删条目；永久删除 / 还原文案对 reference 错误描述本体命运。

**候选方案与优劣**

| 方案 | 做法 | 优点 | 缺点 / 风险 | 裁决 |
| --- | --- | --- | --- | --- |
| ① 规格闭环：reference 进同一回收站通道——`move_to_trash` reference 分支只软删登记行（不搬本体）；清单合并「文件条目（copy）∪ 软删 reference 登记行」；还原 = 复活登记行；永久删除 = 删登记行 + 版本元数据；UI 确认与文案按模式区分 | 与规格 §12.1.1 ③ 完全一致；copy / reference 同一回收站入口，用户认知统一；reference 获得合规删除通道 | 清单双来源合并 + 排序；`TrashAction` 需按条目类型分派；`TrashRow` 增模式 / resource_id；服务层三个方法（trash / restore / purge）补 reference 分支 | **采纳** |
| ② reference 删除走独立「删除登记记录」按钮，不进回收站 | 不动现有回收站文件条目通道，改动面小 | 与规格「行右键移入回收站对 reference 型可用」不一致；回收站里看不到 reference 条目，用户找不到已删的引用；两套删除入口认知分裂 | 否决 |
| ③ 维持现状（reference 拒绝删除） | 零改动 | reference 存档无法删除，成为永久幽灵行；规格缺口 | 否决 |

**实施方案（落点与步骤）**

1. **服务层**（`crates/analytics_resource/src/service.rs`）：
   - `move_to_trash`：reference 分支移除拒绝；改为只 `soft_delete_archive(&resource.id)`（本体留原位，不产生 `ProjectTrash` 文件条目）；copy 分支维持现状（搬本体 + 软删登记行）。
   - `restore_archive_from_trash`：reference 条目（无文件 manifest，按 `resource_id` 分派）→ 只 `undelete_archive`，不调 `trash.restore`；copy 分支维持现状。
   - `purge_archive`：reference 条目 → 只 `purge_deleted_row` + 清版本元数据，不调 `trash.purge`；copy 分支维持现状。
   - 取数辅助：新增「软删 reference 登记行」查询（`deleted_at IS NOT NULL AND payload_mode='reference'`），供回收站清单合并。
2. **模型与装配**：
   - `dialogs/trash.rs::TrashRow` 增 `payload_mode: PayloadMode`、`resource_id: String`、`has_payload: bool`（copy = true，reference = false）。
   - `present.rs::build_trash_snapshot` 收两类来源（文件条目 + 软删 reference 行），按删除时间倒序合并；reference 行 `original_label` = 源路径或显示名，`size_label` 留空。
3. **job 层**（`resource_jobs.rs`）：
   - `load_trash`：除 `trash().list()` 外，查软删 reference 登记行并入 seed；`TrashAction::Restore/Purge` 按条目类型分派到服务层对应分支。
4. **宿主确认**（`resource_host.rs::request_delete`）：
   - 事件路径先开 `open_alert_dialog` 确认框：单选 copy 文案「将删除 `resources/` 副本与登记记录，之后可从回收站还原」；reference 文案「本体留在源位置，只删除登记记录，之后可从回收站还原登记行」；多选按模式分组说明（如「2 项副本 + 1 项引用，将一并移入回收站」）。
   - 确认后 `enqueue_trash`；取消不关窗动作。
5. **回收站对话框文案**（`dialogs/trash.rs`）：
   - 永久删除确认：copy「本体将从回收站里彻底删除，不可恢复；登记记录一并删除」；reference「本体留在源位置不动；仅删除登记记录与版本元数据，不可恢复」。
   - 还原 hint：copy「从回收站还原副本到原位」；reference「仅还原登记记录（本体从未移动）」。
   - 清空文案按混合口径说明。
6. **详情面板危险区**（`detail_view.rs` 动作区）：`can_delete` 判据与 hint 按模式区分——reference 可删（规格：同一判据），hint「本体留原位，只删除登记记录」；copy 维持「本体与登记记录一起进项目级回收站」。

**验证**

- 单测：reference `move_to_trash` 后本体仍在源位置、登记行软删；还原后登记行复活、本体未动；永久删除后登记行与版本元数据清除、源文件仍在；copy 路径既有行为不回归。
- 窗口测试：确认框两态文案；回收站行模式列 / 文案；永久删除确认两态。
- 手动路径：reference 移入回收站 → 清单出现条目（无大小 / 引用标记）→ 还原复活；永久删除 → 源文件仍在；copy 移入 / 还原 / 永久删除全流程不回归。

### A5 全局实例空态分流文案与按钮

**现状证据**

- `resource_view.rs::render_empty`（2370-2410）：固定「还没有任何存档」+ 引导文案 +「从草稿箱归档…」/「从本地文件归档…」两按钮；`self.instance: InstanceFilter`（All / Project / Global）已存在于视图状态。
- `render_no_match`（2416+）：无匹配态独立，不在本项范围。

**问题点**

切到「全局」实例且全局为空时，仍展示项目归档入口；点了在全局口径下无效（全局不能直接归档，只能由项目行升级而来），引导动作与实例语义矛盾。

**候选方案与优劣**

| 方案 | 做法 | 优点 | 缺点 / 风险 | 裁决 |
| --- | --- | --- | --- | --- |
| ① `render_empty` 按 `self.instance` 三态分流 | Global →「全局实例暂无资源」+ 说明「项目里的存档可通过右键『升级到全局…』共享到全局」，不摆归档按钮；Project / All 维持现状 | 引导动作与实例语义一致；改动只在渲染层分支 | 三态文案要各自对齐原型 §13.2 空态口径 | **采纳** |
| ② 全局空态复用项目文案 | 零分支改动 | 按钮在全局口径下无效，误导用户；与 §13.2 空态规格不符 | 否决 |

**实施方案（落点与步骤）**

1. `render_empty` 开头按 `self.instance` 分流：
   - `InstanceFilter::Global`：标题「全局实例暂无资源」；副文案「项目里的存档可通过右键『升级到全局…』共享到全局，供其他项目分发」；不渲染两个归档按钮（只读模式判断不再适用）。
   - `InstanceFilter::Project` / `InstanceFilter::All`：维持现状文案与两按钮（`read_only` 置灰口径不变）。
2. 状态行计数口径现状已分流（`resource_view.rs:2305-2315`），本项不动。

**验证**

- 窗口测试：Global 空态无归档按钮、文案正确；Project / All 空态两按钮就位、只读置灰不回归。
- 手动路径：项目有存档、全局为空 → 切「全局」看到全局空态引导；切「项目」看到项目列表。

### A4 全局行轻量详情 + 「实例」行 + 动作区收敛到分发

**现状证据**

- `present.rs::build_snapshot`（540-580）：details 映射只装项目 `resources` 循环；`SnapshotInputs.global_rows: Vec<ArchiveRow>` 已是行模型，原始 `AnalyticsResource` 字段在装配时丢失。
- `selected_detail`（`resource_view.rs:1891-1894`）：从 `snapshot.details.get(id)` 取，全局行不在映射 → `None` → 详情面板空态。
- 行菜单全局项已置灰：`resource_view.rs:1179`（取回）、1192（重命名）、1230（打标）、1302（删除）；1125 分发项仅 `is_global` 可用。
- 详情动作区（`detail_view.rs:576-680`）：`can_open` / `can_checkout` / `can_delete` 均按项目口径判据，未区分 scope。

**问题点**

1. 点全局行详情空态，用户看不到名称 / 版本 / 实例 / 指纹等基本信息（规格 §13.2：详情基本信息区加「实例」行）。
2. 详情动作区未按实例收敛：全局行理论上不应出现打开 / 取回 / 升级 / 打标 / 删除按钮（行菜单已置灰，详情面板是第二处入口，现状未同步）。

**候选方案与优劣**

| 方案 | 做法 | 优点 | 缺点 / 风险 | 裁决 |
| --- | --- | --- | --- | --- |
| ① 全局行构造轻量 `ArchiveDetail` 进 details 映射；`detail_rows` 加「实例」行；动作区按 `scope` 收敛到「分发到本项目…」 | 点选有反馈；信息完整；动作口径与行菜单一致；复用既有详情面板骨架 | 快照输入要补全局原始行；全局 detail 无标签 / 分组 / 预览（缺省值即可）；动作区增加 scope 判据 | **采纳** |
| ② 全局行不进详情，点选只高亮 | 装配层零改动 | 规格未兑现；点选无反馈，与项目行体验割裂 | 否决 |

**实施方案（落点与步骤）**

1. **快照输入**：
   - `resource_jobs.rs::load_global_rows` 改为返回 `(Vec<ArchiveRow>, Vec<AnalyticsResource>)`（行模型 + 原始行）；`SnapshotInputs` 增 `global_resources: Vec<AnalyticsResource>`。
   - `build_snapshot` 循环全局原始行：构造轻量 `ArchiveDetail`（`scope = Global`、`readonly = true`、`kind` 按 `resource.kind`、`size_label` 按 `file_size` / 规模、`content_hash` / `source_table` 带入；`tags` / `group` / `preview` 缺省），插入 `details`；`history_label` 给「全局共享层，版本链随全局登记」口径或空串（不查项目版本计数）。
2. **详情行**（`detail_view.rs::detail_rows`）：基本信息首行后追加「实例」= 项目实例 / 全局实例；全局行「只读」行文案 =「全局共享层：只读，分发到项目后可在项目侧编辑」。
3. **动作区收敛**（`detail_view.rs` 576-680）：
   - `detail.scope == Global` 时：隐藏打开 / 取回 / 移入回收站 / 升级按钮；只摆「分发到本项目…」（复用 `request_distribute_from_global(detail.name, …)` 端口）；不摆标签 / 分组编辑入口（全局行标签现状为空）。
   - 项目行维持现状判据。
4. **详情头部**：全局行不显示「＋ 标签」「＋ 别名」等项目侧编辑入口（与动作区同一 scope 判据）。

**验证**

- 单测：全局 detail 进 details 映射；`detail_rows` 实例行文案；动作区 `scope` 判据矩阵（Global 仅分发 / Project 全量）。
- 窗口测试：点全局行 → 详情面板显示基本信息 + 实例行 + 仅分发按钮；点项目行 → 现状动作集不回归。

### D2 索引异常首次进入顶部一次性提示条（纯视图层）

**现状证据**

- 状态行异常口径：`resource_view.rs:2302` `counts.has_issues()`；`2316` 全局实例下隐藏「修复…」入口；`render_empty` 与状态行之间无顶部提示条。
- 原型 §5 落地注：索引异常首次进入给顶部一次性提示条。

**问题点**

异常只在状态行计数与「修复…」按钮上，首次进入面板的用户注意不到异常存在；状态行入口弱化色，发现率低。

**候选方案与优劣**

| 方案 | 做法 | 优点 | 缺点 / 风险 | 裁决 |
| --- | --- | --- | --- | --- |
| ① 视图持会话内 `announced` 槽位：`has_issues` 为真且本会话未宣布过 → 顶部 warning 提示条（「索引发现 N 条异常，可在状态行『修复…』处理」），可关闭，宣布后置位；不持久化 | 首次进入可见；不重复打扰；不进设置项 / 跨会话状态，实现最省 | 会话内记忆（刷新面板 / 切项目后仍不重复弹，本轮口径：会话级槽位）；跨会话记忆留后续 | **采纳** |
| ② 持久化「已读」标记（设置项记项目 / 全局） | 跨会话不重复弹 | 原型未要求跨会话记忆；引入设置项读写与版本迁移，成本高于收益 | 否决（留后续如产品提出） |

**实施方案（落点与步骤）**

1. `ResourceView` 增 `announced_index_issue: Rc<Cell<bool>>`（或 `Cell`，视图状态内）。
2. 面板渲染区顶部：`has_issues && !announced` 时渲染 warning 描边条（复用只读提示条样式），文案带异常条数；右侧「知道了」关闭按钮 → 置 `announced = true`；状态行「修复…」按钮保留现状入口。
3. 口径：仅项目实例（`instance != Global`）显示；异常清零后条不出现（即使未宣布也不弹空条）；切项目不清槽位（会话内一次性，规格未要求按项目重置）。

**验证**

- 窗口测试：异常存在首次渲染出条；关闭后不重现；无异常不出条；全局实例不出条。
- 手动路径：制造一条缺失本体 → 进面板顶部出条 → 关闭后状态行「修复…」仍可处理。

## 3. 本轮不做项（阻塞 / 跨 crate / 待决策）与理由

| 编号 | 项 | 现状证据 | 不做理由 / 依赖批次 | 重启条件 |
| --- | --- | --- | --- | --- |
| E1 | 分析表型存档升级到全局 | `resource_host.rs::request_promote_to_global` 无 `payload_rel_path` → notice「随 M8 后续批次接入」 | 依赖分析表读取 / 重建路径（M8 洞察批次）；服务层 `promote` 的 DataFile / Script 两形态不覆盖 Analysis 档 | M8 分析表读取路径接通后，补 Analysis 档 `PromoteKind` 分支与入口解锁 |
| E2 | A2 data 视图（analysis 档打开 = 桥接结果集二次分析） | 仅 file 档报告视图（T20）；`prototype-design.md` §12.2.1 规格就绪 | **待用户拍板**本期做 / 不做；不做则文档保持「当前仅 file 档」标注 | 用户决策通过后按 §12.2.1 规格实现 data 档归档 / 打开 / 共享版本链 |
| E3 | 全局清单口径裁决：镜像登记表 vs `SHOW TABLES` 全库扫描 + 内部表白名单 | `global.rs::list_global_resources` = `scope='global' AND deleted_at IS NULL` 登记查询；§13.2 旧草案 = 全库扫描口径【待确认】 | **待决策**；本轮维持镜像表现状（与版本链 / 登记绑定，查询确定性高） | 产品 / 架构裁决口径后，替换取数查询并补内部表白名单 |
| A6 | 项目行「已升级」标记 + `promoted_from` 回写全局关系 | 升级成功后项目行无回写；`register_global` 单向记来源 | 需迁移 027 加列 + 升级成功回写 + 行尾徽标渲染；本轮 P2。重复升级由 A1 冲突拦截兜底（全局同名 → 冲突询问） | 排期 P2：迁移 + 回写路径 + 行徽标；重复升级提示已由 A1 覆盖 |
| D1 | 空库「草稿箱有 N 个文件值得归档」建议行 | 需草稿箱侧给可归档计数 | 跨 `scratchpad` crate 协作计数；M6 不反向认识草稿箱内部结构（依赖方向纪律） | 草稿箱侧开放计数接口后接入空态建议行 |
| D3 | 详情版本区「最近 3 条」明细 | 快照只带 `history_count` 计数；版本明细仅版本对话框按需加载 | **待决策 / P2**：需「每存档最近 N 版」批量取数 + 快照带行 + 详情渲染；工作量中等，优先级低于 A/B 组 | 排期后：store 最近 N 版查询 + snapshot 带行 + 详情版本分区渲染（超出「查看全部」引导） |
| D5 | 行内 hover 常驻动作 | `resource_view.rs` 注释「hover 版随详情面板批」 | 归详情面板动作集批次；与详情动作区收敛（A4）同批考虑 | 详情面板动作集稳定后补行内 hover 动作 |
| D6 | 回收站自动清理 TTL 策略；条目「在原位置打开」 | `dialogs/trash.rs` 无 TTL 策略 | TTL 属产品决策（保留期 / 超期清理口径）；「在原位置打开」是 reveal 小项，留后续 | 产品拍板 TTL 后补策略；reveal 端口可随详情面板批顺带接入 |
| D4 | 取回侧记 `derived_from_resource_id` | 需草稿箱侧加字段 | M5 草稿箱批次；M6 只产出取回请求，血缘回写由 M5 登记 | M5 草稿箱加字段后打通取回 → 草稿血缘链 |
| D7 | 详情面板宽度记忆（`h_resizable`） | 当前固定 `RIGHT_DOCK_WIDTH` | 依赖 M4 属性面板可缩放接线 | M4 面板缩放能力上线后接宽度记忆 |
| D8 | 编辑器「以结果区打开数据文件（只读）」API | editor crate 侧能力缺失；现由洞察取样入口承担（`request_open` 注释在案） | editor 批次；M6 不越过宿主造编辑器 API | editor 开放结果区只读打开 API 后，替换 File / Analysis 打开路径 |

## 4. 实施顺序与批次划分

> 依赖纪律：模型 / 装配层先行，对话框其次，宿主动作第三，视图收尾最后；每批完成后 `cargo check` + 对应 crate 单测通过再进下一批。

| 批次 | 内容 | 涉及符号落点 | 前置依赖 |
| --- | --- | --- | --- |
| 批 1 · 模型与装配 | B4 详情字段（`payload_mode` / `source_path` / `scope`）+ `to_detail` 带入；A4 全局原始行进 `SnapshotInputs` 与轻量 detail 装配；B3 `TrashRow` 模式 / resource_id / has_payload 字段 | `detail_view.rs::ArchiveDetail`、`present.rs::to_detail` / `build_snapshot` / `SnapshotInputs`、`dialogs/trash.rs::TrashRow`、`resource_jobs.rs::load_global_rows` | 无（基线绿即可开工） |
| 批 2 · 对话框 | B1 归档模式单选（seed / result / inputs / 表单 / readonly_block）；B3 移入回收站确认框 + 回收站永久删除 / 还原文案；A1 冲突三选对话框组件形态 | `dialogs/archive.rs`、`dialogs/trash.rs`、`resource_view.rs` trait 端口 | 批 1 模型字段就位 |
| 批 3 · 宿主动作与服务层 | B2 打开指纹校验 + stale 槽位 + 接受当前内容端口；A1 `OpOutcome::Conflict` + enqueue overwrite / target_name 接线 + 服务层 `distribute` target 参数与双保险冲突检测；B3 服务层 reference 软删 / 复活 / 永久删登记行分支 + 回收站清单双来源合并 | `resource_host.rs`、`resource_jobs.rs`（`OpOutcome` / `run_promote` / `run_distribute` / `run_trash` / `load_trash`）、`global.rs::distribute` / `distribute_overwrite`、`service.rs::move_to_trash` / `restore_archive_from_trash` / `purge_archive` | 批 2 对话框模型就位 |
| 批 4 · 视图收尾 | A5 全局空态分流；D2 异常首次提示条；A4 动作区 scope 收敛 + 实例行渲染；B4 详情行模式分派文案 | `resource_view.rs::render_empty` / 状态行 / 动作区、`detail_view.rs::detail_rows` / 动作区 / `alert_line` | 批 3 状态槽位与端口就位 |
| 批 5 · 原型与文档 | 原型 HTML 同步：B1 reference 模式态、A5 全局空态、A4 全局详情、B2 失效条、A1 冲突对话框；§3 过时文本 4 条修订；gap-review 缺口 #6 / #8 状态回写 + §5 补本轮验证记录 | `analytics-resource-prototype.html`、`prototype-design.md`、`analytics-resource-gap-review.md`、本方案文档 | 批 1-4 代码定型 |
| 批 6 · 验证与提交 | 全仓 `cargo check --workspace`；`cargo test -p rds-analytics-resource --lib`；`cargo test -p rds-workbench --lib`；窗口测试；按 feat / docs 分批提交；验证日志落 `.rds/tmp/` | — | 批 5 完成 |

## 5. 验证计划（批 6 执行口径）

| 项 | 命令 / 方式 | 通过标准 |
| --- | --- | --- |
| 全仓编译 | `cargo check --workspace` | 无新增 error；既有 unused import 警告不扩大范围 |
| M6 单测 | `cargo test -p rds-analytics-resource --lib` | 全绿；新增 B1/B2/B3/B4/A4 模型与装配单测随批落地 |
| 宿主单测 | `cargo test -p rds-workbench --lib` | 全绿；新增 A1 冲突变体 / overwrite / 分发改名、B3 回收站动作分派单测随批落地 |
| 窗口测试 | M6 既有 25 面板窗口 + 10 对话框窗口测试口径回归；新增对话框两态 / 三选窗口用例 | 全绿 |
| 原型自检 | `shot.py` 跑 `analytics-resource-prototype.html`，桌面 / 移动两视口复核新增状态 | 无 console error；新增状态截图与规格一致 |
| 手动路径抽检 | 见各条目「验证」段手动路径清单 | 主路径全通；失败路径有回执、不静默 |

## 6. 待用户拍板清单（实现前不阻塞批 1-4 的 M6 内闭环项）

1. **E2 A2 data 视图**：本期做 / 不做（§12.2.1 规格就绪）。
2. **E3 全局清单口径**：维持镜像登记表（现状）/ 改 `SHOW TABLES` 全库扫描 + 内部表白名单（§13.2 旧草案）。
3. **A6 项目行「已升级」标记**：本轮 P2 不做，是否提前到本期（需迁移 027 + 回写 + 徽标）。
4. **D3 详情最近 3 版明细**：是否纳入本期（快照带最近 N 版 + 详情渲染）。
5. **D6 回收站 TTL 自动清理**：保留期口径（如 30 天 / 不自动清理），产品决策。
