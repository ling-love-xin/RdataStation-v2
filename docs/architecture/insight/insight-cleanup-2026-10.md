# 洞察模块收口盘点清单（2026-10-25）

> **2026-10-30 追加注记（本清单正文仍保留 2026-10-25 当时口径，作历史记录）**：右栏洞察面板已**对齐 EDA 原型重构**——移除 6-Tab 条，右栏 = 洞察表集合 `it-list` + 选中目标下钻 `drillzone`；工具栏按原型重排（新建洞察最右）；下钻区按目标类型自适应（表 / 列 / schema / 多列）。正文中关于「六 Tab」「多列 Tab 空态引导」「摘要 Tab」「目标头」「保存入口在历史 Tab」的表述均为重构前现状；现行口径以 `insight-prototype-design.md`（§2/§2.1/§4 已同步）与 `insight-dev-plan.md` §0/§11 为准。多列聚合统计（GROUP BY / 求和 / 均值 / 计数）为**后续实现**（右栏占位，原型同款）。原 B1-a（多列 Tab 空态引导）已被 2026-10-30 的聚合占位取代；旧 Tab 视角渲染（`view/{overview,table,multi,schema,history}.rs` 的 `render_*`）标 `#[allow(dead_code)]` 保留待中间页 / 资产库复用。
> **盘点口径**：代码（`crates/insight/` + `crates/workbench/src/components/insight_actions.rs` + `crates/editor/src/view/host.rs`）× 权威文档（`insight-architecture.md` §11/§12、`insight-dev-plan.md` §10）× 最新原型 `insight-eda-prototype.html`（旧 `insight-prototype.html` / `insight-workspace-prototype.html` 不再作基准）。
> **分级**：A 类 = 事实缺陷 / 规范违规 / 文档滞后，方案明确、无产品决策，建议本轮直接修；B 类 = 需拍板的产品 / 技术取舍，每项附方案对比与建议；C 类 = 二期 / 等依赖 / 重构单列，本轮不动、只登记口径。
> **收口实测基线**（A6 去评级化后，2026-10-25 实测）：`rds-insight --lib` **244 passed** · `--test column_profile_e2e` **14 passed** · `--test ui_contract` **4 passed**；`cargo fmt -p rds-insight --check` exit 0；`cargo check -p rds-insight --all-targets` EXIT=0、零 error 零 warning。跨 crate 同批分 target 实测：`rds-settings --lib` **32 passed**（旧 31，新增 insight 设置测试）· `rds-engine --lib duckdb::manager::tests` **11 passed**（669 filtered，仅 manager 子集）· `rds-workbench --test ui_contract` **9 passed**。

> **收口结果状态（2026-10-25 更新）**：
> - **已落地并实测全绿**：A1（K22 `loading_note` 残留）、A2（`workspace/mod.rs` 裸 px 改 rem + 契约清单补登）、A3（`set.rs` 挤行拆分）、B1-a（多列 Tab 空态引导）、B4-a（列选择弹层内搜索框）、B7-a（跳列建议行换 `Button` 键盘可达）、**A6 去评级化（方案 A，见下节）**。
> - **本轮拍板口径**：B2 Kendall τ、B3 分组聚合透视、B5 解析级静态门、B4-b 虚拟列表、B4-b 均后置二期，已登记 dev-plan §10；C1/C3 归 M6 P4.1。
> - **未闭环**：仅余 B6 K21 debug 右键闪退待真机复现定性（未复现前不改 vendor）。跨 crate 统一回归已完成：settings 32 / engine manager 11 / workbench ui_contract 9 全绿。A5 已实测：insight 零告警零错误，范围外 9 条历史告警只记录不改写（详见 A5 节）。

### A6 决策记录 · 去评级化（方案 A，用户拍板）

- **起因**：A4 盘点原预设"旧四维加权评分 + 五档等级模型已废"，逐文件 grep + 读码后发现该判断错误——旧模型完整存活并在生产链路渲染，与既定"会话级 EDA 事实画像"定位冲突。勘查全消费面后提交三方案，用户拍板**方案 A**。
- **方案 A 落地口径**：
  1. 撤除列卡 / 表卡 / 摘要列卡的总分环、五档等级（优秀 / 良好 / 一般 / 较差 / 差）与加权合成分；四维只留纯事实量（完整率 / 唯一率 / 类型一致 / 分布倾斜改为事实计数条），**不合成总分**；
  2. 检查器四态（命中 / 未命中 / 不适用 / 排除）照旧呈现，作为事实命中计数；
  3. 历史快照旧字段只读兼容、不重算；Schema 结构健康分与 Grade **独立保留**，不参与表 / 列画像口径；
  4. `quality_scorer.rs` 全保留：`Grade` 枚举、`compute_column_quality` / `compute_table_quality`（旧领域函数 + 历史快照兼容读取）、`compute_column_checks`（新检查器，生产链路使用）；模型层旧领域类型（`QualityDimension` / `QualityScore` / `TableQuality` / `ColumnQualityEntry`）作为契约与快照 JSON schema 兼容保留，UI 视图模型不再消费。
- **保留边界**（勿在后续清理中误删）：`service/mod.rs` 旧质量包装门面、`service/persistence.rs` 空表兜底与历史快照批量评估、`schema_analyzer.rs` / `schema_view.rs` / `view/schema.rs` 的 Schema 健康分链路、`grade_color` 函数本体（仅供结构报告域调用）。

## 0. 现状一句话

会话级 EDA 探索主线（L1 概览 / L2 列级 / L3 关系）、三源统一取样、集合管理、多表对比、关系排查、归档、资源治理四项设置、检查器四态、DuckDB-only 科研统计均已落地；事件链路 18 个 `InsightEvent` 在 `jobs/mod.rs::handle_event` 全覆盖、宿主接缝无悬空事件。A6 收口后表 / 列画像只呈现事实与检查器命中，不再合成评级。A5 告警实测与跨 crate 统一回归（settings 32 / engine manager 11 / workbench 契约 9）已全绿；剩余仅 **B6 K21 待真机复现**。

---

## A 类 · 本轮必修（方案明确，建议直接实施）

### A1 · K22：切换目标后 `loading_note` 残留

> **收口状态**：✅ 已实施并实测——`insight_view.rs::set_target` 内补 `self.loading_note = None`；契约测试随 insight 三测全绿（244/14/4）。

- **问题实证**：`crates/insight/src/insight_view.rs:453-464` `set_target` 重置了 tab / target / data / scope / compare_target / state，**唯独没清 `loading_note`**（字段 239 行；`set_loading_note` 629 行）。从源目标切到另一目标时，上一目标的「正在取样 / 基于 N 行采样」提示可能挂在新目标上——诚实性问题。
- **方案**：`set_target` 内补 `self.loading_note = None;`，与载荷清理同处（注释里「折叠偏好不在此列」的口径继续沿用）。
- **实施步骤**：
  1. `insight_view.rs::set_target` 在 `self.state = Loading` 附近加一行清空；
  2. `insight_view/tests.rs` 补契约测试：先 `set_target` 触发取样提示 → 再 `set_target` 到另一目标 → 断言 `loading_note().is_none()`；
  3. `cargo check -p rds-insight` + `cargo test -p rds-insight --lib`。
- **优劣**：改动 1 行代码 + 1 个测试；无行为副作用（`loading_note` 本就是随目标生命周期的暂态文本）；不改事件协议。
- **风险**：低。需先读 `tests.rs` 尾部现有目标切换用例，避免破坏既有断言。

### A2 · 契约守卫漏登 `workspace/mod.rs`，导致裸 `px(240.)` 绕过检查

> **收口状态**：✅ 已实施并实测——`ui.rs` 登记 `INSIGHT_WS_TITLE_MAX_WIDTH` 常量，`workspace/mod.rs` 裸 `px(240.)` 改 `rems(...)`；`ui_contract.rs::view_sources()` 补登 `workspace/mod.rs`；契约守卫 4 项全绿（原记录「9 项」为旧基线，现行 4 项）。

- **问题实证**：
  - `crates/insight/src/workspace/mod.rs:3964` `.max_w(px(240.))`（中间工作页标题栏超长表名省略宽度，上下文 3953-3968 `title`）——全 crate 裸 `px` 仅此一处，裸色值 0 命中。
  - 根因：`crates/insight/tests/ui_contract.rs::view_sources()`（101-136 行）只显式登记 `insight_view.rs` / `schema_view.rs` / `rule_view/mod.rs` + `view/` 目录递归；**`workspace/mod.rs`（4157 行，本 crate 最大视图文件）漏登**，尺寸 / 颜色契约对它静默失效。这正是该处裸 `px` 能进库的原因。
- **方案**（两步，缺一不可）：
  1. `crates/insight/src/ui.rs` 登记倍率常量：`INSIGHT_WS_TITLE_MAX_WIDTH: f32 = 15.0`（240px ÷ 16px 主题字号 = 15rem，注释写明换算与 VS Code 长标题省略口径）；`workspace.rs:3964` 改 `.max_w(rems(ui::INSIGHT_WS_TITLE_MAX_WIDTH))`。
  2. `ui_contract.rs::view_sources()` 显式清单补 `workspace/mod.rs` 一行 `include_str!("../src/workspace.rs")`；守卫测试 `every_view_module_is_registered_in_the_manifests` 会强制覆盖。
- **实施步骤**：改 ui.rs → 改 workspace.rs → 补契约清单 → 跑 `cargo test -p rds-insight --test ui_contract`（现行基线 4 项）确认常量契约 + 裸 px / 裸色守卫对 workspace.rs 生效。
- **优劣**：常量进 `ui.rs` 单一来源，与现有 `INSIGHT_WS_*` 系列同口径；补登记后后续 workspace.rs 新增违规写法会被守卫拦下，不再漏网。无 UI 视觉变化（15rem = 240px 等值换算）。
- **风险**：低。注意 `max_w` 接受 `Pixels`，`rems()` 返回 `Pixels`，类型匹配。

### A3 · `view/set.rs:16` 两条 `use` 挤一行（rustfmt 格式欠账）

> **收口状态**：✅ 已实施并实测——两条 use 拆行分组；`cargo fmt -p rds-insight --check` exit 0。

- **问题实证**：`crates/insight/src/view/set.rs:16`
  `use gpui_kit::component::{IconName, Sizable as _, Theme};use gpui_kit::prelude::FluentBuilder as _;`
  两条 use 挤在同一行；无功能影响，仅格式不整洁，`cargo fmt --check` 会报。
- **方案**：拆成两行，按 crate 分组顺序排好。
- **实施步骤**：拆分 → `cargo fmt -p rds-insight`（或仅对该文件）→ `cargo check -p rds-insight`。
- **优劣**：零行为变化；顺带扫一遍 insight crate 其余文件是否有同类挤行（grep `;use ` 同模式）。
- **风险**：极低。

### A4 · 文档口径全面滞后（README / 架构 / 开发方案 / 用户手册 / 展示稿）

> **收口状态（2026-10-25）**：本节文档回写已完成——README / architecture / dev-plan / user-guide / showcase.md / showcase.html / extension-notes 均已刷新到现行口径（18 条规则、六 Tab、检查器四态、基线 244/14/4）。下表为盘点时的问题实证原貌，保留作过程记录；其中"加权模型已废"按 A6 实况修正为：**UI 呈现已撤除，旧模型代码作为历史快照兼容读取保留**（详见文首 A6 决策记录）。

- **问题实证**（已逐文件 grep 核实）：
  | 文件:行 | 旧口径 | 现状 |
  | --- | --- | --- |
  | `README.md:3` | 「四维质量评分」 | 检查器四态命中计数，无评级 |
  | `README.md:32` | 「质量评分四维加权 .35/.25/.20/.25、阈值 85/70/50/30」 | UI 呈现已撤除；旧加权模型代码仅作历史快照兼容读取，不进表 / 列画像视图模型 |
  | `README.md:56` | 「四维质量评分（列 + 表聚合）」 | 检查器四态 |
  | `README.md:6` | 「内置 16 条」「测试 227 项 + 集成 14」 | 18 条（列 9 · 多列 6 · 表 3）；实测基线 lib 244 / e2e 14 / ui_contract 4 |
  | `README.md:20` | 「右栏轻量统计 + 底栏四 Tab 合并为单个右 Dock」 | 右栏六 Tab + 中间 EDA 工作页（D63-D66） |
  | `README.md:79` | 「洞察面板（五 Tab）」「五 Tab 全部落地」 | 六 Tab：Overview 摘要 / Column / Table / MultiColumn / Schema / History |
  | `README.md:91` | 契约范围只写 `crates/workbench/tests/ui_contract.rs` | 漏 insight 自有 `crates/insight/tests/ui_contract.rs` |
  | `README.md:140` | 基线 227 / 集成 14 / analysis 8 / engine 452 | insight 实测 244 / e2e 14 / ui_contract 4；analysis 与 engine 跨 crate 基线待 A5 回归实测回填 |
  | `README.md:150` | 「§4.8 内置规则 16 条一览」 | 18 条 |
  | `README.md:152-153` | showcase「五个 Tab」 | 六 Tab + 中间工作页 |
  | `insight-architecture.md:121,380,457` | 「16 条」 | 18 条 |
  | `insight-architecture.md:394` | 基线 218 项 + 集成 13 | 实测 lib 244 + e2e 14 |
  | `insight-architecture.md:450` | 「五态 / 五 Tab」、片段清单缺 `overview.rs` / `set.rs` / `workspace/mod.rs` | 六 Tab + 中间工作页，片段清单补全 |
  | `insight-architecture.md:467` | K2「待删除确认」 | 重复目录已删（`e3684d67`），`crates/engine/insight-rules/` 实测不存在，仅余 `crates/insight/insight-rules/` |
  | `insight-architecture.md:486` | K21「机制已查明，未修」 | 状态与任务 #16「completed」矛盾，见 B6；本轮先按真机复核结论回写 |
  | `insight-architecture.md:487` | K22「待修」 | A1 修完后改 ✅ |
  | `insight-dev-plan.md:30,44,62` | 「227 项」 | 实测 lib 244 |
  | `insight-dev-plan.md:1066` | 「最近核对 2026-09-18」 | 刷新为 2026-10-25 |
  | `insight-dev-plan.md:1068-1075` §10 清单 | 未登记最近落地项 | 补登：集合模型 T18 / 关系排查 T19 / 归档 T20 / 资源治理设置 T38-T41 / 中间工作页 D63-D66 T15 / 科研统计 T24 / 检查器四态 T28-T31 / 四缺口 T33-T37；已完成行从施工单删除（§10 约定：做完即删行） |
  | `view/overview.rs:3-11,65` 注释 | 「质量评分四维 / 四维条」旧口径 | 实际四统计条 = 需关注 / 可空 / 主键 / 未识别（检查器四态口径），注释同步修正 |

- **缺失内容补登**（README 模块特点 / 代码地图 / 架构实现映射两节都要补）：中间 EDA 工作页（工具栏 + 概览五卡 + 值得关注跳列 + 列分布 + 缺失矩阵 + 关联三段 + 样本段）、已洞察表集合管理、多表对比三态、跨表值重叠关系排查、归档到资产库、资源治理四项设置（内存上限 / 全量两段确认 / 工作集阈值 / 内存显示开关）、检查器四态、科研统计量（Spearman / Cramér's V / 偏度 / 超额峰度）、两类报告导出（JSON Schema 报告 / Markdown 画像报告）、字段筛选器（类型下拉 + 列多选弹层）。
- **方案**：实现收尾、测试数字实测出来后统一回写；规则数 18、Tab 数 6、文件清单等代码事实先行回写，测试计数待实测填充。
- **实施步骤**：
  1. A1-A3 代码改完、测试数字实测后，按上表逐文件改 README / architecture / dev-plan / user-guide / showcase；
  2. §10 清单删除已完成行、补登新落地项；K2 标已删、K22 改 ✅；K21 待 B6 结论回写；
  3. `view/overview.rs` 注释口径修正（随 A2 同批）；
  4. 回写后 grep 复扫 `16 条` / `227` / `五 Tab` / `四维质量评分`，确认无残留旧口径。
- **优劣**：纯文档工作，零代码风险；口径与代码事实对齐后避免后续文档再度漂移。工作量约占本轮 1/3，务必放在代码收口之后（数字以实测为准，不预先写 242）。
- **风险**：低。`insight-user-guide.md` / `insight-showcase.md` 同批核对，勿漏。

### A5 · `cargo check --all-targets` 实测定位编译告警（含 editor 侧两个测试辅助方法）

> **收口状态**：✅ **已实测（2026-10-25）**。insight crate 自身 `cargo check -p rds-insight --all-targets` **EXIT=0、Finished、零 warning 零 error**（日志 `.rds/tmp/insight-check-a5-insight.log`）；insight 范围内**无可清扫的未用 import / dead_code / unused_mut**。全仓已编译段未出现任何 `error[E...]` / `error:`；下列告警全部落在 insight 边界之外，按「只改被点名范围」原则**只记录、不改写**，归各自 crate 专项。

- **insight 实测结论（权威）**：
  - `cargo check -p rds-insight --all-targets -j 2` → `Finished dev profile`，退出码 0；日志仅含 `Checking rds-insight`，无附着 warning。
  - 范围对照：`crates/insight/` 源码与 `crates/workbench/src/components/insight_actions.rs` 本轮 A1/A2/A3/B1-a/B4-a/B7-a/A6 改动后，编译器未产生任何新告警。

- **editor 两个 cfg 测试辅助方法（原盘点存疑点，本轮实证）**：
  - `host.rs:6869 open_value_preview_with_window`、`:6911 value_preview_for_test` 均带 `#[cfg(any(test, feature = "test-support"))]`（6868 / 6910）；生产路径 6852 调 `open_value_preview_with_window_impl`（6892）。本轮全仓编译对这两条**未附着 dead_code 告警**——生产 lib 单元被条件排除，现状保留正确，不删不改。
  - `host.rs:6777 apply_filter` / `:7057 clear_filter` 报 dead_code 属**有意预留**：已带 `#[cfg_attr(not(test), allow(dead_code))]`，注释注明「面板筛选路径自洽；本入口保留给测试与阶段 3 直接调」，生产不命中告警、仅 test target 下提示；非缺陷，保留。

- **范围外历史告警清单（本轮日志实证，非本轮引入，未改动）**：

  | # | 位置 | 类型 | 所属 target | 性质判定 | 处置 |
  | --- | --- | --- | --- | --- | --- |
  | 1 | `crates/kernel/src/registry.rs:206` | unused variable `venv` | kernel lib test | 历史欠债（测试内未用环境变量） | 记录，归 kernel 专项 |
  | 2 | `crates/engine/src/persistence/metadata_cache.rs:4632` | unused mut `ops` | engine lib test | 历史欠债（测试辅助变量） | 记录，归 engine 专项 |
  | 3 | `crates/plugin/src/sidecar/supervisor.rs:1482` | unused mut `pr` | plugin lib test | 历史欠债（首轮观测；本轮增量编译跳过该 target，留待全仓复跑确认） | 记录，归 plugin 专项 |
  | 4 | `crates/plugin/src/sidecar/supervisor.rs:1537` | unused mut `pr` | plugin lib test | 历史欠债（同上） | 记录，归 plugin 专项 |
  | 5 | `crates/app/src/main.rs:24` | unused imports `RedoResultEdit` / `UndoResultEdit` | app bin（bin test 重复 1 次） | 历史欠债：未用动作导入 | 记录，归 app 专项 |
  | 6 | `crates/workbench/tests/history_editor_entry.rs:11` | unused import `AppContext` | workbench test | 测试内未用 trait 导入 | 记录，归 workbench 专项 |
  | 7 | `crates/editor/src/view/signature_help.rs:185` | unused import `signature_view` | editor lib test | test fixture 内未用 | 记录，归 editor 专项 |
  | 8 | `crates/editor/src/view/host.rs:6777` / `:7057` | dead_code `apply_filter` / `clear_filter` | editor lib test | **有意预留**（见上），非欠债 | 保留不改 |
  | 9 | `crates/editor/src/view/signature_help.rs:188` | unused fn `substr_sig` | editor lib test | test fixture 未用辅助函数 | 记录，归 editor 专项 |

- **进程退出码说明（避免误读为编译失败）**：target 清空后全量 `cargo check --workspace --all-targets` 在独立分离进程中运行，两次均在编译 editor / workbench test target 前后被会话回收，日志停在 `rds-editor (lib test) generated 3 warnings`、未见 `Finished`，脚本读到 `$LASTEXITCODE = -1`。**这不是 cargo 编译失败**：cargo 失败退出码为 101 且必写 `error[E0xxx]` / `error:`；本轮日志 errors 段为空、已编译 crate 全部通过。insight 结论以单独命令完整 Finished 为准；范围外告警若需归零，另立跨 crate 专项分 target 清扫。

- **方案与边界**：本轮只对 insight 范围内告警负责（已归零）；范围外 1/2/3/4/5/6/7/9 均为一行级 `cargo fix` 可解的历史欠债，但跨出洞察模块边界，不在本轮擅自改写，避免收口夹带无关 crate 改动。
- **风险**：低。insight 无代码改动；范围外条目保持现状，不影响 insight 编译与测试。

---

## B 类 · 待拍板（附方案对比、优劣与建议）

### B1 · §10 #9：多列 Tab 在样本表解析前点击，静默不发请求

> **收口状态**：✅ 已拍板 **B1-a** 并落地——样本表缺失时多列面板渲染空态引导，Tab 切换不发隐式请求；随 A6 批次测试全绿。

- **现状**：`insight_view.rs:1112-1118`，`PanelTab::MultiColumn` 取 `self.data.sample_table(&target)`：`Some` → 发 `MultiColumnRequested`；`None` → `None`，界面无任何反馈。源目标还没看过列 / 表时，点多列 Tab 什么都不会发生。
- **方案对比**：
  | 方案 | 做法 | 优 | 劣 |
  | --- | --- | --- | --- |
  | B1-a · 置灰 + 显式提示（**建议**） | 样本表缺失时多列面板渲染空态引导「请先在列画像 / 表探查中完成取样，再做多列分析」；Tab 切换不发隐式请求 | 状态诚实、不重复取数；用户动作与数据流一致；实现小（复用现有空态 `muted_line` 模式） | 用户需先点一次列 / 表 Tab |
  | B1-b · 隐式自取一次样 | 切到多列 Tab 发现无样本表，自动代发一次取样请求再取多列 | 操作步数少 | 隐式触发后台取数，与「render 不发起副作用、用户显式动作驱动」口径冲突；可能与当前正在进行的取样并发；状态来源说不清 |
- **建议**：B1-a。与 Overview「不触发取数、只用已加载载荷渲染」（`insight_view.rs:1086`）同一哲学。
- **量级**：小（视图空态文案 + 条件渲染，1 个测试用例）。

### B2 · Kendall τ：是否补第三个秩相关系数

> **收口状态**：⏸ 已拍板 **B2-b 后置**——遵守「只处理 DuckDB 可提供统计量」口径，Kendall τ 需 Rust 侧秩相关算法，已登记 dev-plan §10 二期项，本轮不实现。

- **现状**：关联方法已落地 Pearson（内置规则）+ Spearman（`spearman-correlation`）；原型关联方法为 Pearson / Spearman / Kendall 三选。`workspace.rs:2032` 注释「Kendall τ DuckDB 无直接函数，后置」。Cramér's V 已覆盖分类关联。
- **方案对比**：
  | 方案 | 做法 | 优 | 劣 |
  | --- | --- | --- | --- |
  | B2-a · 本轮 Rust 侧实现（**建议后置**） | 从样本列读成对值到 Rust，算 Kendall τ-b（含并列校正），走现有规则输出映射 | 凑齐原型三方法；科研场景相关性判断更完整 | DuckDB-only 原则被打破（统计逻辑进 Rust）；200+ 列两两组合时 Rust 侧全量两两比较的性能 / 内存要单独设计；测试面增加 |
  | B2-b · 继续后置（**建议**） | 文档 §10 登记缺口：Kendall τ 需 Rust 侧秩相关实现，列二期；现状 Pearson + Spearman 已覆盖单调 / 线性两类秩相关判断 | 模块保持 DuckDB 单引擎口径；本轮不摊薄主线收口 | 原型三方法缺一项，文档要如实标注 |
- **建议**：B2-b。用户已拍板「只处理 DuckDB 可以提供的统计量」，Kendall 属 Rust 侧算法扩展，与该口径冲突；等二期科研分析能力统一规划时再做（连同正态检验、方差分析等）。
- **量级**：若做 = 中（秩相关算法 + 两两组合选择器 + 样本上限 + 测试）；后置 = 仅文档登记。

### B3 · DBeaver 式多列聚合（GROUP BY / 求和 / 均值 / 计数）本轮是否做

> **收口状态**：⏸ 已拍板 **B3-b 后置**——分组聚合透视属交互式查询分析，归将来 Jupyter / 分析模式；已登记 dev-plan §10 二期项，本轮聚焦画像 / 关系 / 对比主线。

- **现状**：原型 `insight-eda-prototype.html:526-527` 标注「后续占位」；代码无 `group_by` 聚合实现。现状多列 Tab 是规则驱动的列对相关性 / 列联表，不是分组聚合透视。
- **方案对比**：
  | 方案 | 做法 | 优 | 劣 |
  | --- | --- | --- | --- |
  | B3-a · 本轮实现 | 选分组列 + 度量列 + 聚合函数（count / sum / avg / min / max），发 DuckDB `GROUP BY` 查询，结果表渲染（参照 DBeaver 右侧值查看 / 分组面板） | 融合 EDA 与表格统计能力，探索期「按类别看均值分布」诉求直接满足 | 属另一类能力：查询构造器 + 结果表 + 分组 / 度量选择器，交互面不小；与画像快照 / 报告口径如何衔接要重新设计；本轮工作量膨胀 |
  | B3-b · 维持原型占位，登记二期（**建议**） | §10 新增预留项「分组聚合透视（DBeaver 式）」；本轮聚焦画像 / 关系 / 对比主线 | 模块边界清晰：洞察 = 对此刻数据出画像结论；分组聚合 = 交互式查询分析，归将来 Jupyter / 分析模式 | 探索期分组聚合诉求暂不满足 |
- **建议**：B3-b。与既有定位一致：审计 / 评级 / 归档归 M6，交互式分组聚合归将来分析模式；本轮在 dev-plan §10 登记预留项与触发条件。
- **量级**：做 = 大（独立子面板 + 查询事件 + 结果表 + 设置默认行数）；后置 = 文档一行。

### B4 · 字段筛选弹层 200+ 列：体验补强到什么程度

> **收口状态**：✅ 已拍板 **B4-a 并落地**——弹层内补独立搜索框，与类型过滤叠加（双滤 AND）；⏸ **B4-b 虚拟列表后置**，待实测出现千列级场景再上虚拟化，已登记 dev-plan §10。

- **现状**：`workspace.rs:2408 render_field_picker`：顶部类型下拉（全部 / 数值 / 文本 / 时间 / 布尔 / 未识别）+ 列多选 Popover（trigger 显示「列 N/总数」；全选 / 反选 + Checkbox 全量列表；首次点选把隐式全选物化为显式清单 2602）。**弹层内部无独立搜索框**，200+ 列全量 Checkbox 滚动、无虚拟列表。
- **方案对比**：
  | 方案 | 做法 | 优 | 劣 |
  | --- | --- | --- | --- |
  | B4-a · 弹层内补一个搜索框（**建议先做**） | Popover content 顶部加 `Input`（复用 `InputState` 惰性创建模式 2434 / 2439 同款），按列名前缀 / 子串过滤 Checkbox 列表；与类型过滤叠加（双滤 AND，对齐 1282 行现有双滤口径） | 改动小、复用现有组件与状态模式；200 列场景定位速度数量级提升；不引入虚拟化复杂度 | 超长列表仍需滚动，无行回收 |
  | B4-b · 虚拟列表 | 弹层内换 `v_virtual_list` / `VirtualList` 渲染 Checkbox | 千列级内存友好 | 弹层内虚拟列表 + 选中态持久化 + 搜索过滤的状态链路改造较大；200 列量级收益不显著；独立测试面 |
  | B4-c · 维持现状 | 仅靠类型下拉 + 外层列分布搜索框 | 零改动 | 大业务表（200+ 列）弹层长滚、定位困难，是已识别的体验缺口 |
- **建议**：B4-a 本轮做（小工作量、高收益）；B4-b 列入 §10 预留，待实测出现千列级场景再上虚拟化。
- **量级**：B4-a = 小；B4-b = 中。

### B5 · 规则 SQL 静态门：是否从关键字黑名单升级为解析级策略检查（§10 #10）

> **收口状态**：⏸ 已拍板 **B5-b 后置**——黑名单 + 信任门 + 字段白名单已收口；解析级策略检查属独立安全专项，单列下期，设计记录见 `insight-extension-notes.md` §6.4。

- **现状**：`rule_registry.rs` 关键字黑名单（多语句 / DDL / DML / 文件与远端表函数 / PRAGMA / `read_csv_auto` / `sqlite_attach` / `copy` 等）；项目规则另有信任门 D53。`crates/engine/src/sql` 已有解析能力。
- **方案对比**：
  | 方案 | 做法 | 优 | 劣 |
  | --- | --- | --- | --- |
  | B5-a · 本轮升级解析级 | 复用 engine SQL 解析器，按 AST 白名单门控（只允许 SELECT 单语句、只读表引用、禁调用黑名单函数 / 写谓词 / 子查询写侧），错误文案给 AST 定位 | 比关键字名单严谨（绕写变体 / 注释拼接 / 大小写变形漏报率下降） | 中量级：解析器接缝、错误模型、白名单边界（CTE / 子查询 / 标量函数白名单）都要定；现有 18 条内置规则 + 测试要全量回归 |
  | B5-b · 维持现状，下期加固（**建议**） | 黑名单 + 信任门 + 解析期门控（D48/D49 字段白名单）已收口；§10 保留可选加强行 | 本轮不摊薄主线；信任门已堵「不可信项目」主路径 | 黑名单理论上有漏报变体 |
- **建议**：B5-b。安全门的加固属独立专项，与本轮 UI / 口径收口不同域；单列下期，附 `insight-extension-notes.md` §6.4 已有的设计记录。
- **量级**：做 = 中；后置 = 文档保留。

### B6 · K21 debug 右键闪退：现状复核 + 修复方向三选一

> **收口状态**：⏳ **待真机复现定性**——本轮未在 debug 构建复现前改动代码或 vendor；架构 §11 K21 维持「机制已查明、待真机复核」记录，与任务 #16 状态矛盾已在文档标注以 K 表为准。复现后按 B6-a → B6-b → B6-c 顺序推进。

- **现状矛盾**：架构 §11 K21 记「机制已查明（2026-10-18），未修」；任务 #16 却标 `completed`。本轮代码复核：`crates/database/src/nav_view/chrome.rs:951` 仍有 `.track_focus(&self.focus_handle)`、`:1077` `this.focus_handle.focus(window, cx)`、`:42` input focus handle；代码侧未见右键前 blur 痕迹。机制（摘要记录）：双常驻 track_focus——树主体 `chrome.rs:951` + PopupMenu 根 `vendor/gpui-component/src/menu/popup_menu.rs:1472`；右键打开菜单路径 `context_menu.rs:176-181` build_menu focus 与 popup_menu.rs:999 / :1030 submenu/parent focus 同帧双 claim；vendor 已有 blur 补丁（context_menu.rs:410-420）未拦住（blur 只清右键事件帧，PopupMenu focus 在下一帧 deferred 回调）。
- **前置动作**：debug 构建真机复现（导航树表 / 视图节点右键），确认 release / debug 现状是否仍 panic、`set_focus called more than once in a single frame` 栈帧是否与 K21 记录一致。**未复现前不改 vendor。**
- **方案对比**：
  | 方案 | 做法 | 优 | 劣 |
  | --- | --- | --- | --- |
  | B6-a · 业务侧右键入口前 blur（**建议先试**） | 三个右键入口（查看统计 / 结构洞察 / 查看洞察）构建菜单前主动 blur 树主体 focus_handle，再由菜单获取焦点 | 改动限业务侧、不碰 vendor；机制上先拆同帧双 claim；回滚容易 | 只盖住入口路径；PopupMenu 内部 submenu 切换的双 claim 若复现则拦不住 |
  | B6-b · vendor 补丁增强 | `context_menu.rs:410-420` blur 逻辑前移：build_menu focus 菜单前再清一次主体焦点 | 在焦点 claim 链路上游堵，覆盖面全 | 改 vendor 补丁，升级 gpui 版本要重放；要对照 0.8.0 源码确认补丁位置仍有效 |
  | B6-c · 改树主体 track_focus 条件 | `chrome.rs:951` 主体 track_focus 改为条件挂载（参照 K18 思路），右键菜单弹出期间不参与 a11y focus claim | 从根上减少常驻双 claim 源 | 导航树键盘可达性要重新验收（方向键 / Tab 焦点序列不能退）；回归面在 database crate |
- **建议**：先真机复现定性 → B6-a 最小改动试一轮；真机不崩 / 不复发即收口；若 B6-a 拦不住再评估 B6-b / B6-c。任务 #16 状态与架构文档矛盾一并回写（A4）。
- **量级**：B6-a = 小；B6-b = 中；B6-c = 中 + 键盘可达回归测试。
- **风险**：debug-only panic（release 仅 `log::warn`），不阻塞日常使用，但阻塞 debug 开发与真机截图场景。

### B7 · 裸 `Div` 数据行点击：键盘可达性是否本轮补齐

> **收口状态**：✅ 已拍板 **B7-a 并落地**——`workspace/mod.rs` 值得关注跳列建议行换标准 `Button`，自带 `track_focus` / `tab_stop` / KeyUp 激活。**实证排除**：`view/history.rs:317` 版本对比行用的是标准 `ListItem`（自带 track_focus / tab_stop / 键盘 / a11y），不是欠账，不改。

- **现状**（规范要求：可点元素优先 `Button`；确需自绘必须 `track_focus` + `tab_stop(true)` + 稳定 `ElementId`，禁用下标键）：
  - `workspace.rs:1216` 值得关注跳列建议行（注释 1174 已登记「行级导航用 div.on_click，数据行非标准控件」）；
  - `view/history.rs:317` 版本对比行 `row.on_click(...)`——**复核后排除**：标准 `ListItem` 自带 track_focus / tab_stop / 键盘 / a11y，非欠账；
  - 其余 `on_click` 挂载点已核：工具栏 / 菜单项 / Checkbox / Radio 均为标准组件（`workspace.rs:591/604/626/683`、`set.rs:37-51`、`view/mod.rs:310`、`multi.rs:83/191/256`、`header.rs:45/59` 等）。
- **方案对比**：
  | 方案 | 做法 | 优 | 劣 |
  | --- | --- | --- | --- |
  | B7-a · 本轮补齐跳列裸行（**已落地**） | 1216 跳列行换标准 `Button`（自带 `track_focus` + `tab_stop` + KeyUp 激活）；317 历史行经实证为标准 ListItem，不动 | 键盘可达 / 屏幕阅读器口径闭环；消除规范欠账 | 小工作量；Button 焦点态样式复用组件 hover 态 |
  | B7-b · 维持现状，登记技术债（**备选**） | 注释里补「待补键盘可达性」，记入 §10 | 本轮不动视图结构 | 规范欠账延续；键盘用户无法操作版本对比 / 跳列 |
- **建议**：B7-a，与 A1-A3 同批（两处而已，风险小）；跳列行现有 `ScrollAnchor` 不受影响。
- **量级**：小（两处 + 对应 headless 键盘测试骨架，参照 `crates/project/src/ui/tests.rs::picker_controls_activate_from_the_keyboard`）。

---

## C 类 · 二期 / 等依赖 / 重构单列（本轮不动，仅文档登记口径）

| # | 项 | 现状证据 | 口径 |
| --- | --- | --- | --- |
| C1 | K6 表级 / Schema 报告快照 | 两表零写入者；`save_table_quality` / `save_schema_insight` 零调用者 | 二期，UI 与资产模型归 M6；diff 计算继续用洞察 `VersionDiffView::between`，存储归 `store`，将来连同一并搬（dev-plan §10 #4） |
| C2 | §10 #3 结果集临时表定向回收 | `drop_temp_table(Query)` 零生产调用；结果集复用宿主 `tmp_q_` 临时表，生命周期归宿主 | 维持：洞察侧无可回收对象；将来结果集由洞察独立物化时再接定向回收 |
| C3 | §10 #7 分析表型存档洞察（`kind = Analysis`） | `can_view_stats` 对该 kind 返回 false；本体层在 M6 P4.1 | 等 M6 P4.1 本体命名 / 存在性 / `definition_sql` 定后再接，工作量小 |
| C4 | 超大文件重构单列 | A6 裁剪后实测行数（2026-10-25）：`model.rs` 2713 · `workspace/mod.rs` 4042 · `rule_view/mod.rs` 1545 · `insight_view/tests.rs` 1699 · `jobs/mod.rs` 1449 · `insight_engine.rs` 1250 · `insight_view.rs` 1035 | 属独立重构任务（纯位移不改语义），按 §11 位移先例单独排期；本轮不拆，避免收口夹带结构性风险；排期前重新实测行数 |
| C5 | K18 `RenderHint` 解析后无人消费 | `RuleFile.render` 仅入类型与校验，视图按数据形态渲染 | 定案保留（`deny_unknown_fields` 兼容原因）；将来做图表可视化时消费；明确不做则删字段 + 迁移 |
| C6 | 列语义标签第三档 | 已拍板进下期规划 | 本轮只支持现有类型识别（数值 / 文本 / 时间 / 布尔 / 未识别） |

---

## 实施顺序与回归计划

> **执行进度（2026-10-25）**：步骤 2 代码收口（A1/A2/A3/B7-a/B1-a/B4-a/A6）✅ 完成，insight 三测 244/14/4 全绿；步骤 5 文档回写 ✅ 完成；步骤 3 A5 告警实测 ✅ 完成（insight 零告警，范围外欠债只记录）；步骤 4 跨 crate 统一回归 ✅ 完成（settings 32 / engine manager 11 / workbench 契约 9 全绿）。**剩余**：步骤 1 B6 待真机复现。

1. **B6 真机复现定性**（前置，决定 B6 路线；不阻塞 A1-A3）。
2. ✅ **代码收口**（A1 → A2 → A3 → B7-a → B1-a → B4-a → A6 去评级化），每处改完 `cargo check -p rds-insight`；insight lib 零 error 零 warning。
3. ✅ **全量编译告警实测**（A5）：`cargo check -p rds-insight --all-targets` EXIT=0 零告警；全仓已编译段无 error；范围外 9 条历史告警归类记录（见 A5 节表），不在本轮跨 crate 改写。
4. ✅ **统一测试回归**（target 清空后分 target 短命令实测，避免全量长编译被回收）：
   - ✅ `cargo test -j 2 -p rds-settings --lib`（**32 passed / 0 failed**，旧基线 31，+1 为 insight 节设置测试）
   - ✅ `cargo test -j 2 -p rds-engine --lib duckdb::manager::tests`（**11 passed / 0 failed**，669 filtered；仅 manager 子集，全量未本轮重跑；附 1 条范围外 `unused_mut` 历史告警，见 A5 节 #2）
   - ✅ `cargo test -j 2 -p rds-insight --lib`（244 passed）+ `--test column_profile_e2e`（14 passed）+ `--test ui_contract`（4 passed）
   - ✅ `cargo test -j 2 -p rds-workbench --test ui_contract`（**9 passed / 0 failed**，与旧基线 9 一致）
   - **本轮回归顺带修复**：workbench 契约 `shared_fields_are_whitelisted` 拦截到两个早已在 `Shared` 落地但白名单漏登的宿主级字段——`conflict_dialog`（M6 冲突对话框流转，与 tag/repair/trash/version 同形态）、`reference_stale`（M6 B2 reference 失效槽位）；已按字母序补登 `crates/workbench/tests/ui_contract.rs` 白名单，并同步 `panels-modules.md` §3 同类注记（均不跨模块，不进主耦合表）。另：首轮 workbench 测试报 `E0425 quoted_qualified` 系上一轮进程被回收留下的陈旧增量缓存误报，单独 `cargo check -p rds-database --lib` 干净 Finished 零错误后复跑即通过，当前源码无此问题。
   - 新增用例随 A1 / B1-a / B4-a / B7-a 落地；insight 与跨 crate 基线均已回填 README / architecture / 本清单。
5. ✅ **文档回写**（A4）：README / architecture / dev-plan / user-guide / showcase.md / showcase.html / extension-notes 已按上表逐条刷新；§10 清单删已完成行、补登新落地项；K2 ✅ / K22 ✅；K21 待 B6 结论回写。
6. ✅ insight 侧收尾复扫：grep 旧口径无残留；`cargo fmt -p rds-insight --check` exit 0。外壳裸 px / 裸色守卫随步骤 4 `rds-workbench --test ui_contract` 9 项全绿一并通过。
7. ✅ **注释口径复扫（2026-10-25 收尾段）**：
   - **规则数**：`crates/insight/src/rule.rs:8` 与 `lib.rs:13` 模块文档表仍写「内置 16 条」，实测 `insight-rules/` = 列 9 · 多列 6 · 表 3 = **18 条**（与 `rule_types::BUILTIN_RULE_COUNT = 18` 及 `test_builtin_count_matches_constant` 断言一致），两处注释已对齐 18；本清单 A2 步骤描述残留的契约「9 项」旧基线对齐为现行 **4 项**。
   - **Tab 数与片段清单**：`insight_view/tests.rs:357`、`workspace.rs:6 / :788`、`view/mod.rs:4` 残留「五 Tab」旧计数，现行 `PanelTab::ALL` = **六 Tab**（Overview / Column / Table / MultiColumn / Schema / History，`model.rs:134`），四处注释对齐为六 Tab；`view/mod.rs` 片段清单补登 `overview.rs`（摘要 Tab）与 `set.rs`（已洞察表集合管理卡 + 关系排查，body 顶部、不属任一 Tab），与 `view/` 目录 8 个文件一一对应。
   - **卡片数与状态态数（第二轮复扫）**：
     - 概览卡：`workspace.rs:8 / :3503`、`ui.rs:24`、`tests/ui_contract.rs:31` 残留「四卡」旧计数；现行概览 = **五卡**（总行数 / 列数 / 缺失率 / 唯一率 / 值得关注，`workspace.rs:979` 注释 + 5 个 `summary_card` 调用实测），四处对齐五卡。
     - 面板状态：`insight_view.rs:18` 残留「五态」；现行 `InsightPanelState` = **四态**（Empty / Loading / Error / Data，`model.rs:397`），对齐四态。
     - 规则对话框：`rule_view.rs:1655` 残留「四态」；现行 `RulesDialogState` = **三态**（Loading / Ready / Error，`rule_view.rs:391`），测试体覆盖 Loading→Error→Ready 三帧，对齐三态。
     - 列 Tab 期次提示：`insight_view/tests.rs:382`「其余四个 Tab」是旧五 Tab 计数，六 Tab 遍历 `PanelTab::ALL` 后为其余五个，对齐五个。
     - 保留未动：列画像「四区」（基础统计 / 分布 / 质量 / 样本，`model.rs:1669-1675`）与结构报告「四区」（`schema_view.rs:22`）是分区数，现行正确；`quality_scorer.rs` 四维权重属 A6 保留边界。
   - **保留边界**：`quality_scorer.rs` 的 `Grade` / 五档文案 / 四维权重 / 阈值单测、`model/types.rs:125-161` 旧领域类型属 A6 保留边界（历史快照 JSON 只读兼容 + Schema 结构健康分独立域），生产画像链路不调用，**未删除**；`jobs/mod.rs` / `model.rs` 中「不合成质量总分 / 等级」为现行正确声明，保留。
   - **校验**：两轮 `cargo fmt -p rds-insight --check` 均 exit 0；两轮独立分离进程复跑三测全量均为 `--lib` **244 passed / 0 failed**、`--test column_profile_e2e` **14 passed / 0 failed**、`--test ui_contract` **4 passed / 0 failed**，doc-test 1 ignored（与基线一致、零回归）；第二轮增量编译 Finished 33.86s 零 error 零 warning。本轮改动仅限模块文档注释与计数说明，未触及运行时代码。

## 已拍板结论汇总（2026-10-25 收口）

- **B1** 多列 Tab 空态行为：✅ 采纳 **B1-a** 置灰 + 提示，已落地。
- **B2** Kendall τ：⏸ 采纳 **B2-b** 后置，文档登记缺口（遵守 DuckDB-only 口径）。
- **B3** 分组聚合透视：⏸ 采纳 **B3-b** 后置，§10 登记预留，归将来分析模式。
- **B4** 列选择器补强：✅ 采纳 **B4-a** 弹层内搜索框（已落地）；⏸ B4-b 虚拟列表后置。
- **B5** 规则静态门解析级升级：⏸ 采纳 **B5-b** 维持黑名单，下期安全专项。
- **B6** K21 右键闪退：⏳ 先真机复现，复现后 B6-a 业务侧 blur 起步；未复现前不改 vendor。
- **B7** 裸行键盘可达性：✅ 采纳 **B7-a**，跳列行换标准 Button；历史行经实证为标准 ListItem，不改。
- **B8** DuckDB 社区扩展用于科研统计：⏸ **本期不引入，登记 §10 #17 预留**。EDA 核心画像（高频路径）继续纯 DuckDB 原生聚合，不依赖外部扩展；科研统计增强（正态检验 / ANOVA / Kendall τ / 效应量）归二期科研分析模式。前置条件：引擎版本 ABI 对齐、扩展 bundle 离线可用、加载失败降级不阻塞主流程。选型实测（stats_duck / duckfn_statrs / Rust 手写三选一）在二期启动时做。

---

## 结构瘦身拆分收口（2026-10-30，分支 refactor/insight-slimming）

大文件按职责域拆子模块（**纯位移，不改定位、不违反既有 skill / 决策**），独立分支验证无误后合主。

| 原文件 | 新结构 | 拆分依据 |
| --- | --- | --- |
| `workspace.rs`（3997 行） | `workspace/` = mod.rs + toolbar / overview / eda / rel / sample / compare / helpers / markdown（8 子模块） | 提交 `e291570a`；函数 85/85 |
| `model.rs`（3225 行） | `model/` = mod.rs + sets / target / panel / columns / profile / multi / history / column_profile（8 子模块） | 提交 `2b317a98`；函数 140/140，内容守恒缺失 0 行 |
| `jobs.rs`（1554 行） | `jobs/` = mod.rs（接缝 + 事件分发）+ profile / rules / table / history（请求职责域） | 提交 `4d5b0687`；函数 38/38 |
| `rule_view.rs`（1781 行） | `rule_view/` = mod.rs（聚合）+ data（视图模型）+ view（对话框渲染） | 提交 `4d5b0687`；函数 70/70 |
| `service/mod.rs`（1087 行） | InsightService 方法按职责域下沉 sampling / analyze / rules 子文件；use 区改 `pub(crate) use` 供子模块继承 | 提交 `4d5b0687`；函数 134/134 |

- **保留边界**（未动）：`service/mod.rs` 旧质量包装门面、`service/persistence.rs` 空表兜底与历史快照批量评估、`store` 表/结构报告存取（D64 未来接入点）、`quality_scorer.rs` 全保留、18 条内置规则、检查器四态、ui_contract 契约。
- **死代码重核结论**：早期候选（`InsightService` 质量门面 3 个 / persistence 4 个 / store 2 个）经全仓 grep 重核**全部落在 A6 保留边界 / D64 未来接入 / 公开 API 契约内**，均不可删；全仓零调用扫描仅剩测试属性宏注册的测试函数，**无真实死代码可删**。唯一清理：`model/sets.rs` 未使用的 `use super::*`（warning 清零，提交 `74620dc4`）。
- **契约守卫**：`ui_contract.rs` 补登 `rule_view/` 三个文件（裸 px / 裸色守卫不静默失效）。
- **验证**：`cargo test -p rds-insight -j 2`（共享 target）= **244 lib + 14 e2e + 4 ui_contract 全绿**，1 ignored 既存；rds-insight **零 warning**；与拆分基线完全一致、零回归。
