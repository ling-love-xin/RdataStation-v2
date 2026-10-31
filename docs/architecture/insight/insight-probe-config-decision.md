# 洞察探查配置与取数流程（决策记录）

> 状态：ADOPTED（2026-10-31，语义已与用户三轮对齐，拍板齐全）
> 关联：`insight-user-guide.md` §2.1、`insight-architecture.md`
> 本文件固化探查确认窗口 / 取数建表 / 自动评估 / 取样档 / 中断语义，作为实施与后续维护的依据。

## 1. 背景

原链路：进入洞察 → 固定取样 500 行建临时表 → 表探查（元数据 + 缺失/唯一率）→ 用户手动点「评估全表」才逐列回填完整统计与检查器。问题：

- 取样行数/列范围不可配置，无法从根源上控制资源（大表无论是否需要都取 500 行、全列计算）；
- 「评估全表」必须手动触发，迷你分布图（`7316c356`）挂在评估数据上，取样后看不到图；
- 无中断语义：评估中途打断后无法只保留已完成部分并补算。

## 2. 已确认语义（C）

| # | 语义 |
| --- | --- |
| C1 | **探查范围由用户定，系统只给默认值**：数据量（全量 / 行数 N / 百分比 P%）+ 探查列（全部或子集） |
| C2 | **取样 = 取数 = 建表**：确认窗口的行数/百分比落到「建临时表」层（`LIMIT N` / `TABLESAMPLE P%`）；该表是迷你统计 / 评估 / 检查器的**唯一基数**，根源上减少资源贯穿全链路 |
| C3 | **进入洞察弹确认窗口**：数据量 + 探查列 + 提示区（**必含「探查全部」**一键）；取消 = 不建表、不改状态（幂等） |
| C4 | **未选择列在 EDA 上不存在**（列分布不渲染，非灰态） |
| C5 | **探查中断列 → 灰态「未完成」卡**，可补算 |
| C6 | **EDA 有重新探查入口**：走同一确认窗口，预填上次配置 |
| C7 | **迷你统计 A 方案**：histogram + Top3 + 布尔占比并入取样链路，取样档即有迷你图；评估仍算完整 `stats_detail` + 检查器 |

## 3. 拍板记录（2026-10-31）

| 项 | 决策 |
| --- | --- |
| R1 过滤条件 | **采纳**：取数可带可选 WHERE 条件（根源减资源的延伸） |
| R2 采样方式 | **采纳（用户选择）**：行数（LIMIT，前 N 行、确定）vs 百分比（TABLESAMPLE reservoir 随机采样、均匀有代表性），UI 说明差异，可配 seed 可复现 |
| R3 源数据变更检测 | **不做**：洞察 = 快照语义，查看此时的情况，不涉及链式检测 |
| R4 耗时预估 | 渐进校准：每次取数/列探查计时，累计每列平均耗时，确认窗口显示「预计约 N 秒」（越用越准） |
| R5 取数中不闪空 | 重新取数保留旧画像 + 顶部阶段条，新表建好后一次性替换 |
| R6 互斥操作 | 取数/探查/评估进行中，其余重操作按钮禁用（jobs 串行语义 + UI 禁用态） |
| R7 检查器与质量卡 | 只统计已探查列（与「未选列不存在」一致） |
| R8 默认值 | 行数 500 / 百分比 5% / 列=全部；上次配置记忆（会话级） |

问答定稿（本轮确认）：

- **Q1 默认评估触发**：进入 EDA **自动触发评估全表**（确认窗口显示预计耗时与规模）；
- **Q2 取样实现**：**查询级过滤**（不建子表，SQL 里 LIMIT + 列投影，轻量每次现算）；
- **Q3 迷你统计定位**：**并入取样链路**（取样档随取样计算迷你统计，取样即有迷你图）。

## 4. 语义模型 v4（流程）

```
进入洞察 / 重新取数
   │
   ▼
确认窗口（数据量：全量/行数/百分比 + 快捷预设 + 采样方式 + 列集合/快速组 + WHERE + 规模预估 + 提示区含「探查全部」）
   │ 确认（取消=幂等，不建表不改状态）
   ▼
取数建表 T（LIMIT N 或 TABLESAMPLE P%，可带 WHERE/seed）── 阶段条，保留旧画像（R5）
   │
   ▼
EDA 默认 = 评估全表（自动，对 T 的全部选中列：完整 stats_detail + 检查器，列级 x/y 进度）
   │ 提示：「评估全表 = 确认窗口生成的 N 行 × K 列」（不是源库全表）
   ├─ 取样档（查询级过滤：行 LIMIT + 字段投影）→ 迷你统计（C7）→ 取样即有迷你图
   ├─ 列状态：✓已探查 / ⟳中断灰态卡（C5）/ 未选列不存在（C4）
   └─ 回环：⟳重新取数（改数据量→重跑建表→级联重评估）· 重新探查（改列→表不动补算/剔除/补中断列）
```

## 5. 确认窗口规格

1. **数据量块**：全量 / 行数 N / 百分比 P%（RadioGroup 切换）；快捷预设入口：**全量 / 常用行数（500/1000/5000）/ 常用百分比（5%/10%/25%）**；默认 500 行 / 5%，预填上次配置。
2. **采样方式块**（行数/百分比差异说明）：行数 = 前 N 行（LIMIT，确定、可预期）；百分比 = 随机采样（TABLESAMPLE reservoir，均匀、有代表性，带 seed 可复现）。
3. **列块**：全部列 / 自定义列（List + Checkbox）；快速组：全部 / 仅数值 / 仅文本 / 仅问题列（缺失率或唯一率高）。
4. **WHERE 块**（可选）：过滤条件文本，取数时并入建表 SQL。
5. **规模预估**：N 行 × K 列 = M 单元 + 预计耗时（R4 渐进校准）。
6. **提示区**（必含「探查全部」= 全量 + 全部列）：数据量越大探查越慢，建议按需限制；限制范围是后续评估与检查器的基数。
7. **底部**：取消 / 确认探查（GPUI 标准组件：RadioGroup / List / Checkbox / Button / Separator）。

## 6. 入口分工

| 入口 | 配置 | 动作 | 代价 |
| --- | --- | --- | --- |
| 重新取数 | 只配数据量（行数/百分比 [+WHERE] [+采样方式]） | 重跑建表 → 表变 → 级联重评估当前列集合（R5 不闪空） | 重（必须） |
| 重新探查 | 只配列范围 | 表不动，补算新增列 / 剔除取消列；可只补中断列（C5） | 轻 |
| 评估全表（⧉） | —（受取数基数约束） | 对 T 的全部选中列逐列完整统计 + 检查器，列级进度 | 重（现有，进入时自动） |

> **#26 实施取舍（2026-10-31）**：两入口收敛为**一个「重新探查」入口**（EDA 列区顶部），
> 复用同一确认窗口且**数据量 + 列 + WHERE 全可配**——「重新取数」语义由窗口内改数据量达成
> （确认 → 重跑建表 → 级联重评估，R5 不闪空）。未做「只显数据量 / 只显列」的双窗口变体：
> 全配置窗口一次确认即覆盖两种诉求，交互更少歧义；「表不动、只补/剔除列」的轻量增量
> 留待需要时（现有单列补算已覆盖中断列，C5）。

## 7. 取样档语义（查询级过滤）

- **取样 = 对评估全表的二次取样**：行子集 + **字段子集**（取样不仅指数据量，还有字段）；
- 实现：查询级过滤（不建子表）：SQL 里行 LIMIT + 列投影，轻量、每次现算、不复现（Q2 定稿）；
- 迷你统计（histogram / Top3 / 布尔占比）并入取样链路（C7 / Q3）：取样档随取样计算，取样即有迷你图；全量表评估产物与取样档互不覆盖。

## 8. 列状态三态

| 状态 | 显示 | 来源 |
| --- | --- | --- |
| 已探查 | 正常卡 + 迷你图 | 自动评估（全量表）或迷你统计（取样档） |
| 中断列 | 灰态「未完成」卡（虚线框 + ⟳ 可补算） | 评估中断（取消令牌停止派发后续列，已完成列保留） |
| 未选择列 | **不存在**（不渲染） | 确认窗口列集合之外 |

## 9. 实施落点（代码映射）

| 改动 | 位置 |
| --- | --- |
| ProbeConfig（rows/percent/columns/where/seed/sample_mode）+ 面板状态 + 上次配置 | `crates/insight/src/model/`、`workspace/eda.rs` |
| 取数建表参数化（LIMIT/TABLESAMPLE/WHERE/seed + 计时） | `service/sampling.rs` `profile_source_table`、`insight_engine.rs` |
| 确认窗口 UI（快捷预设/采样方式/列选择/WHERE/规模/提示区） | `workspace/eda.rs`（GPUI 标准组件） |
| 自动评估全表 + 「评估全表 = 确认窗口生成的表」提示 | `jobs/table.rs` `request_table_evaluation` 入口、`view/table.rs`、`view/overview.rs` |
| 取样档查询级过滤 + 迷你统计并入取样链路 | `workspace/eda.rs` `render_col_mini_vis`、`service/sampling.rs` |
| 未选列不渲染 / 中断列灰态 / 取消令牌 | `workspace/eda.rs`、`jobs/table.rs` |
| 重新取数 / 重新探查两入口（复用确认窗口） | `view/header.rs` / `view/table.rs` 工具栏 |

## 10. 验证与测试

- 确认窗口：快捷预设切换 / 采样方式差异说明 / 取消幂等（不建表不改状态）；
- 取数：LIMIT N / TABLESAMPLE P%（含 seed 可复现）/ WHERE 过滤 / 计时；
- 自动评估：进入 EDA 即自动评估、列级进度、「评估全表 = 确认窗口生成的 N 行 × K 列」提示；
- 取样档：查询级过滤（行+字段子集）+ 迷你图即时可见；全量/取样切换不互相覆盖；
- 中断：取消令牌停止派发、已完成列保留、灰态卡可补算；
- 未选列不在列分布渲染；检查器/质量卡只统计已探查列。

## 11. 实施进度（2026-10-31 滚动更新）

| 任务 | 状态 | 说明 |
| --- | --- | --- |
| #19 决策文档 | ✅ | 本文档 |
| #20 ProbeConfig 模型 + 面板状态 | ✅ | `model/probe.rs`（ProbeAmount / ProbeColumns / ProbeSampleMode / seed / filter / sample_default / full）+ `InsightPanelState::ProbeConfirm`（五态）；rds-insight 单测绿 |
| #21 取数建表参数化 | ✅ | `persistence.rs` `sample_source_to_analysis_table(source, &ProbeConfig)` + `build_source_sample_sql` / `build_duckdb_sample_sql`（LIMIT N / TABLESAMPLE P% + seed + WHERE + 列投影）+ `count_source_rows` + `probe_source_columns`（源库 `SELECT * … LIMIT 0` / DuckDB `DESCRIBE` 取列结构，失败默认全部列） |
| #22 确认窗口 UI + 接线 | ✅ | `InsightEvent` 扩展（ProfileRequested 带 config / 新增 ProbeColumnsRequested）；`ProbeDialogState`（amount_mode 全量/行数/百分比 + 快捷预设 + 采样方式 + 列候选 + WHERE + seed + parse_notice + 上次配置预填）；`set_target` 分流（新目标弹窗 / 已登记直接取数 `set_target_loaded`）；`workspace/probe_dialog.rs` 渲染（数据量 RadioGroup + 7 快捷预设 + 采样差异说明 + 列 Checkbox + 过滤 + 规模预估 + 提示区必含「探查全部」+ 取消/确认）；`render_insight_content` 接 `ProbeConfirm` 分支；窗口级测试 `source_target_renders_probe_dialog_until_confirmed`（渲染 + 探查全部入口 + 确认关闭） |
| #23 自动评估全表 + 提示语义 | ✅ | Q1：`set_table_profile` 探查回填后自动续跑评估（判别：progress None && quality None && 列明细全 None，评估回填/已评估不重复触发）；建议②：`view/table.rs` 评估入口下提示「评估全表 = 确认窗口生成的表（N 行 × K 列）；取样是对评估全表的取样，不仅数据量还有字段」；jobs 测试断言更新（探查后自动进入评估并完成） |
| #24 取样档查询级过滤 + 迷你统计 | ✅ | Q2 语义切换：`request_sample` 由「重建取样表」改为对**当前评估全表**发 `SampleInsightRequested`（查询级过滤，不建子表、不重取源）；`confirm_full` 由「重取源全量建表」改为「切全量口径 + 触发评估」（全量 = 评估确认窗口生成的表，`FullProfileRequested` 分支保留兼容）；C7：新 `ColumnMiniStats`（数值 8 柱直方图 / 文本时间 Top3 / 布尔占比）随取样现算回填 `TableColumnView::mini_stats`（`compute_sample_mini_stats`，`SAMPLE_MINI_LIMIT=500`，列 CAST 成统一类型避免 DECIMAL/时间转换坑），与评估产物互不覆盖；`render_col_mini_vis` 按 scope 分流（全量读 stats_detail/histogram，取样读 mini_stats）；toolbar tooltip 更新；jobs 新测试 `sample_insight_fills_mini_stats_without_touching_eval` |
| #25 未选列不存在 + 中断列灰态 + 取消令牌 | ✅ | C4：建表 SQL 列投影（`build_source_sample_sql` / `build_duckdb_sample_sql` 加 `column_projection`，子集列双引号内引号翻倍转义；未选列不进临时表 → EDA 内省自然不存在）+ 4 单测；C5：取消令牌（`InsightView::eval_cancel: Arc<AtomicBool>`，`cancel_table_evaluation` 置位、`request_table_evaluation` 重置、jobs 逐列循环派发前检查）、中断列灰态（`TableColumnView::interrupted` + `mark_interrupted`，未完成列标记、已完成列保留、不 finish 保持 quality None）、单列补算（新事件 `ColumnEvaluateRequested` + `request_column_evaluation`，`with_column_detail` 回填清除标记）；视图：表 Tab 评估中「取消」按钮、EDA 列卡「未完成」灰态 + 「补算」入口；模型 2 单测 + jobs 1 补算测试 |
| #26 重新探查入口 | ✅ | C6：EDA 列区顶部「重新探查」按钮 → 复用确认窗口（`last_probe_config` 预填）；确认走「不闪空」分支（已登记目标 + 已有画像 → 保留旧载荷 + 阶段条说明，发 `ProfileRequested` 重跑建表，新表就绪 jobs 一次性替换，R5）；死代码 `ProbeAmountMode::of_amount` 删除（预填已由 `ProbeDialogState::new` 直接还原） |
| #27 编译测试与提交 | ⏳ 未开始 | 全量测试已随 #26 全绿（261+14+4）；提交待办见 §11 尾部 |

验证：`cargo test -p rds-insight` = 261 lib + 14 jobs/lib + 4 ui_contract 全绿（2026-10-31，含 C4 列投影、C5 取消/中断/补算链路、C6 重新探查入口后）。

> **#25 核对结论（2026-10-31）**：C4 缺口属实——#21 时 `build_source_sample_sql` / `build_duckdb_sample_sql` 仍拼 `SELECT *`，
> 列子集未进建表 SQL；本项在建表层补列投影后，未选列不进分析临时表，EDA 上不存在（无需再在渲染侧过滤）。
> 原「#21 含列投影」表述与 #25 前置注记「理论上已满足」均更正为「#25 在建表 SQL 层补投影」。

> **#25 实施记录（2026-10-31）**：见上方 §11 #25 行——C4 在建表 SQL 层补列投影（#21 时未落地，
> 决策文档「#21 含列投影」表述更正）；C5 取消令牌 / 中断列灰态 / 单列补算已在
> `jobs/table.rs` 逐列评估循环 + `TableColumnView::interrupted` + 新事件
> `ColumnEvaluateRequested` 落地并全绿。

> **#24 语义重构点（已实施，2026-10-31）**：`request_sample` 已改为对**当前确认窗口生成的评估表**
> 做查询级过滤现算（行 LIMIT + 列投影 + 迷你统计，不建子表、不重取源）；
> `confirm_full` 已改为切全量口径 + 触发评估（不再重取源全量建表），全量 = 评估确认窗口生成的表。


