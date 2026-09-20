# Mock 数据生成（M7）— 开发方案

> 做什么、做到哪：现状盘点 / 阶段任务与落点 / 验收与测试场景 / 风险 / 验证命令 / 进度记录。
> 设计与硬约束见 `README.md` 与 `mock-architecture.md`；视觉口径见 `mock-prototype-design.md`。

## 1. 现状盘点（本轮开始前）

| 层 | 状态 | 证据 |
| --- | --- | --- |
| 引擎 / 生成器 / 模板 / 映射 / 持久化 | ✅ 100% 迁入 `crates/mock`（与 v1 逐字级一致，仅导入路径改写） | 逐文件 `diff --strip-trailing-cr`：真实差异仅 1 行导入 + 1 处导出时序改动 |
| 单元测试 | ✅ 56 项随迁 | `crates/mock/src/*.rs` 内联 `#[cfg(test)]` |
| 公开 API 集成测试 | ❌ 无。v1 那份 `tests/mock_engine_tests.rs` **不可编译**（引用已不存在的 `GeneratorConfig::type_name()`、把 `String` 型 `confidence` 当数值比较、断言恒为空的 `preview.rows`） | 见 `mock-architecture.md` §9-I0a |
| 命令层（21 个 Tauri 命令） | 🏁 按 Round 14 政策退役，服务本体在 crate 内 | `docs/migration/commands-retirement.md` |
| 视图层 | ❌ 占位：右 Dock `render_mock_placeholder` 三行静态文案；分析库页「生成 Mock」按钮 `on_click` 为空实现 | 本轮开始时 `crates/workbench/src/panels/` |
| 装配层 | ⚠️ 已有 `services/mock_generator.rs` 但**全仓只有测试调用**；且自带一份 `parse_data_type` 副本 | 同上 |
| 文档 | ❌ 零：无 `docs/architecture/mock/`、无 `crates/mock/README.md`（连接/数据库/项目/草稿箱/编辑器/洞察均有） | `docs/architecture/README.md` 缺口表未列 mock |

## 2. 本轮已完成（Phase A）

| # | 任务 | 落点 | 验收证据 |
| --- | --- | --- | --- |
| A1 | 公开 API 集成测试重建（26 项，覆盖生成/预览/映射/依赖/取消/类型/五种导出/持久化/草稿目录/模板/场景） | `crates/mock/tests/mock_engine_tests.rs`（新增目录） | `cargo test -p rds-mock` → 26/26 通过 |
| A2 | 修复 `persist_as_asset` / `export(Table)` 的非法 SQL（`CREATE TABLE t AS SELECT * FROM SELECT * FROM …`） | `crates/mock/src/engine.rs`（两处调用改传源表名）+ `crates/engine/src/sql/{builder,engine}.rs`（参数正名 `source_table` + 文档） | 新增回归 `export_table_creates_named_table_and_drops_temp`；A1 实测先红后绿 |
| A3 | 类型串收敛为唯一入口 `mock::parse_data_type` | `crates/mock/src/schema_map.rs`（公开）+ `lib.rs`（re-export）+ `crates/workbench/src/services/mock_generator.rs`（删副本） | `parse_data_type_is_public_and_loose`、`map_column_resolves_parameterized_source_types`、`crates/workbench/tests/mock_generator.rs::parse_data_type_maps_common_types` |
| A4 | 装配层写入语义明确化：**持久化（新建）** + **追加**（主键自增起点接续表内行数）+ 返回总行数 | `crates/workbench/src/services/mock_generator.rs` | `persist_creates_table_and_rejects_second_run`、`append_continues_primary_key_sequence`（累计 100 行、`id` 无重复、`MAX(id)=100`） |
| A5 | Mock 面板最小可用闭环（目标选择 / 行数 / 生成 / 结果与错误 / 只读护栏） | `crates/mock/src/mock_view.rs`（视图随 crate）+ `crates/workbench/src/components/mock_host.rs`（宿主桥）+ `crates/workbench/src/panels/`（实体懒创建 + 句柄登记） | `cargo check -p rds-workbench --all-targets` 通过；8 项窗口测试覆盖面板与对话框 |
| A6 | 接线「生成 Mock」按钮（原空实现）→ 展开右 Dock Mock 面板；导航右键入口按表名定向选表（`Shared::open_mock_panel`） | 同上 | 同上 |
| A7 | 清理与卫生：删除死代码 `_generator_of`；`crates/mock/Cargo.toml` 移除未使用 `uuid`、`tokio` 移入 `[dev-dependencies]`；新文件 rustfmt 干净 | `crates/workbench/src/services/mock_generator.rs`、`crates/mock/Cargo.toml` | `cargo check -p rds-mock --all-targets` 通过 |
| A8 | 文档补齐（本目录五件套 + 交互稿）+ crate README | `docs/architecture/mock/*`、`crates/mock/README.md` | 本文件 |
| A9 | **视图归属与对话化重构**：面板 + 状态 + 两个语义对话框沉入 mock crate（`mock_view.rs` / `mock_view/tests.rs`）；重交互改走 `Dialog`（生成器选择 / 列配置，含工作副本与「恢复智能默认」）；宿主能力改 traits 注入 | `crates/mock/src/mock_view.rs`、`crates/workbench/src/components/mock_host.rs`、`panels/` | 窗口测试：面板渲染 / 定向选表 / 生成与预览 / 只读拦截 / 对话框可打开（8 项） |
| A10 | **生成器目录穷尽派生**（143 变体分类 / 中文标签 / 参数规格 / 默认构造） | `tools/gen_mock_generator_catalog.py` → `crates/mock/src/generator_catalog.rs` | 目录自检 3 项：143 覆盖、标签与默认构造齐备、变体↔分类往返 |
| A11 | 参数编辑用 **JSON 补丁**（`patch_param`），避开 143 份「表单 → 变体」构造 | `crates/mock/src/mock_view.rs` | 单元测试：整数 / 浮点 / 文本 / 字符串 / `Option` 字段与非法输入保留原值 |
| A12 | **语义回归**：目标表是用户命名的**新表**（表名输入），不再「从分析库既有表里挑着灌数」；**生成不写库**（只产内存临时表 + 预览） | `mock_view.rs`（`MockDraft.table_name` + `run_generate`）、`services/mock_generator.rs`（`generate` vs `persist_table` 拆分） | 视图测试 `generate_produces_preview_without_touching_sinks`（三出口调用计数为零）；装配测试 `generate_does_not_write_analysis_db` |
| A13 | **四个显式出口**：新建分析库表（同名报错 + 回滚）/ 追加到既有表（显式选表、主键自增接续、缺列报错）/ 草稿箱 `{项目}/mock/` / 另存为（系统保存对话框） | `services/mock_generator.rs`（`persist_table` / `append_table` / `export_file` / `save_scratchpad`）、`mock_view.rs`（出口按钮组） | 装配测试 10 项（含 `append_continues_primary_key_sequence`：`MAX(id)=100` 且无重复） |
| A14 | **方案①排版**：右 Dock = 配置 + 出口；**中央「Mock 数据」tab** = 字段卡片 + 预览表（详情持面板实体，状态单一权威） | `mock_view.rs`（`MockDetailView` + `Panel` 协议实现）、`view.rs`（`Shared::open_mock_detail` 宿主命令 + `DockArea::add_panel(Center)`）、`panels/`（构造期创建面板实体 + 句柄） | 视图测试：详情渲染字段与预览、`focus_tab` 幂等（未加入 Dock 时静默返回） |
| A15 | **列模型解放**：增删列 / 改列名与类型 / 13 类型下拉 / 唯一值改 `Switch` / 生成器改**分类子菜单**（143 项仍在，形态从对话框改为子菜单） | `mock_view.rs`（`add_column` / `remove_column` / `ColumnDraft` / `generator_menu` / `rebuild_params`） | 视图测试：列增删、改列后旧结果作废、智能默认恢复、列编辑对话框可开 |
| A16 | **导入源库结构**（连接 / 库 / schema / 表）+ 导航右键定向：`NavCache` → `MetadataService` 的 cache-aside 取列，带置信度与示例值 | `services/mock_generator.rs`（`import_columns` / `schema_sources`）、`mock_view.rs`（`preset_from_source` / `open_import_dialog` / `SchemaRequest`）、`panels/`（右键菜单传 `SchemaRequest`） | 视图测试 `preset_from_source_imports_columns_and_table_name`；装配测试 `schema_sources_carry_connection_defaults` |
| A17 | 装配层重构：去掉临时文件中转（`insert_statements` 收在 mock crate）+ 列名规范化唯一入口 `sanitize_identifier`（临时表列名与建表列名必须同一算法） | `crates/mock/src/engine.rs`、`services/mock_generator.rs` | 集成测试 `export_sql_insert_writes_insert_statements`；**并修掉自身引入的锁重入死锁**（架构 §9-I0d） |

**本轮实测数字**：`cargo test -p rds-mock` = 93 单元（含 16 窗口）+ 26 集成测试（0 失败）；
`cargo test -p rds-workbench --test mock_generator` = 10/10；`cargo test -p rds-workbench` 全绿（除存量 `ui_contract` 欠债）；
`cargo check -p rds-mock --all-targets` / `cargo check -p rds-workbench --all-targets` 通过（零告警）。

## 3. 阶段任务

### Phase B — 字段表与生成器选择

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| B1 | 字段表：列名 / 类型 / 生成器（含置信度）/ 参数摘要 / 增删列 | ✅ 已完成（中央 tab 字段卡片） | 表可滚动（16rem 上限）、参数摘要可读 |
| B2 | 生成器选择（143 变体，按 15 分类分组） | ✅ 已完成（字段行的**分类子菜单**） | 选到的变体与 `GeneratorConfig` 一一对应（目录自检测试） |
| B3 | 参数编辑（标量：数值 / 文本 / 布尔；列名 / 类型 / 空值率 / 唯一） | ✅ 已完成（列编辑对话框）；复杂参数待外置入口 | 非法值保留原值；取消不污染目标列 |
| B4 | 行数预设与种子开关（可复现） | ✅ 已完成（行数 / 种子输入 + 校验） | 同 seed 两次生成结果一致（引擎测试兜底） |
| B5 | 生成中态与取消（`generate_with_progress` + `cancel()`） | ✅ 已完成（`services::mock_jobs` 工作线程 + 面板进度条 / 取消 + 定时泵；追加与**三个出口**同走后台） | 大行数生成时进度可见、可中断；出口报不定量进度且不可取消 |
| B6 | 预览（前 10 行） | ✅ 已完成（中央 tab 预览表 = 组件库 `DataTable`；`#` 行号槽 + 列宽可拖 + 横向滚动） | 列名与值来自真实生成结果；`TableState::dump` 可读回表头与行（`preview_table_dumps_the_sample_through_the_component_table`） |
| B7 | 生成器「推荐」标记与最近使用 | ✅ 已完成（生成器菜单顶部给「最近使用」：本会话点过的，最近在前、去重、最多 5 条；智能映射推荐的那项标「推荐」——`recommended_generator` 与导入结构同源（`ColumnMapper::infer`，列名 + 类型），只标记不写回） | 常用生成器一眼可选（翻分类与搜索之外的第三条路） |
| B8 | 生成器**搜索**（143 项按名称 / 标签） | ✅ 已完成（字段行菜单首项开「搜索生成器」对话框：`List` + `ListState`，同步过滤 + 多词 AND，无命中显空态） | 输入关键词即过滤；确认写回该列 |

### Phase C — 入口扩展

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | 场景模板选择与一键生成（6 套内置） | ✅ 已完成（面板「场景模板 ▾」菜单 + `MockJobKind::Scenario` 后台任务 + `generate_scenario_at` 装配；模板清单构造时缓存，不每帧重算） | 一键产多张表（电商模板 4 张）；逐表进度按「张表」报；结果区/详情支持**当前表**切换，出口作用于选中那张 |
| C2 | 从数据库导入结构 → 字段表 | ✅ 已完成（导入结构对话框 + 导航右键定向；`NavCache` → `MetadataService` cache-aside） | 选连接/库/schema/表 → 字段表自动填充 + 智能映射（含置信度） |
| C3 | ~~列依赖编辑器（`Expression` / `Template` / `Weighted`）~~ | ✅ A 步已完成（拍板「删空壳」）：`DependencyType` 与 `source_columns` / `expression` / `weights` 已从模型删除，`ColumnDependency { ref_table, ref_column }` 存在即跨表引用（架构 D40） | 模型与文档一致：不再有「看着能用、实际没人读」的字段；列间求值（模板 / 加权 / 算术）作为**独立特性**另行立项 |
| C4 | 用户模板保存 / 复用（`MockGenerationStore`） | ✅ 已完成（「保存为模板…」对话框 + 模板段「应用 / 删除」；套用不动目标表名） | 存→列→取→用于生成 全链路；SQLite 往返测试已补（见 §7） |
| C5 | 复杂参数（集合 / 加权）外置编辑入口 | Mock 面板 + `generator_catalog` 的 `ParamKind::Complex` | ✅ 已完成（列编辑对话框里的**多行文本**：一行一项 / 一行「值, 权重」；非法输入保留上一个有效值 + 行号提示；空集合与全零权重在**生成前**拦住） |

### Phase D — 出口与历史

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| D1 | 导出（CSV / Parquet / Xlsx / SQL INSERT）到用户选择路径 | ✅ 已完成（「另存为 ▾」+ 系统保存对话框） | 四种格式落盘且内容自检（CSV 表头 + 行数） |
| D2 | 保存到草稿箱 `{项目}/mock/` | ✅ 已完成（「保存到草稿箱 ▾」，时间戳命名；无项目报错） | 文件名 `mock_{表}_{时间戳}.{ext}`；只读项目禁写 |
| D3 | 持久化为分析资源表 | ✅ 已完成（「持久化为分析库表」：新建 + 同名报错 + 写失败回滚） | 分析库出现新表且行数正确；既有数据不被覆盖 |
| D3b | 追加到既有表（v2 新增出口） | ✅ 已完成（显式选表 + 主键自增接续 + 缺列报错） | 二次追加累计行数翻倍、主键无重复 |
| D4 | 生成历史面板（`mock_generation_tasks` / `_columns`） | ✅ 已完成（Mock 面板底部「生成历史」段：最近 20 条 + 重放 / 删除 + 刷新；读写全在 `crates/mock/src/history.rs` 的后台线程） | 列表按时间倒序、可删除、可重放配置 |
| D5 | 任务自动落库（生成成功即写 `save_task`） | ✅ 已完成（面板在任务收尾时组装 `RunRecord`，后台写入） | 生成一次 → 历史多一条；失败时记录 `error_message` |
| D6 | ~~分析资源注册（`persist_as_asset` → M6）~~ | ✅ 口径已澄清（**不需要单独实现**：「持久化到项目分析库」落地的是**本项目上的一张持久表**——它就是分析资源的**项目级形态**，没有「再登记一次」这回事，也没有触发时机可谈；早先这句「生成后可在资源管理器中看到该表」是错的落点） | 资源管理器（M6）列的是 `resources/` 下的**存档副本**（`list_file_archives`，文件语义），本就不应包含项目库里的表；要进全局由用户在资源管理器发起 M6 存档（对表而言是 M6 自己的事），mock 不开直写口子 |

### Phase E — 项目作用域与并发

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| E1 | ~~导航右键携带目标表上下文~~ | ✅ 已完成（`SchemaRequest` 含 conn/catalog/schema/table） | 打开面板即已导入该表结构 |
| E2 | ~~源库表结构获取~~ | ✅ 已完成（cache-aside：L2 缓存 → 实时内省） | 与属性面板显示一致 |
| E3 | ~~按源结构在分析库建表~~ | ✅ 已完成（「持久化为分析库表」按草稿列生成 DDL） | 建表 + 灌注行数正确 |
| E4 | 项目作用域分析库（`{项目}/.RSmeta/analytics.duckdb`） | ✅ 已完成（**已拍板：Mock 输出恒为项目级**）——`analysis_db_path(project_root)` 派生自当前项目；未打开项目时落库 / 追加**明确拒绝**（可读原因 + 替代路径），纯生成仍可用 | 打开项目时写项目库；要进全局走资产库存档（M6）/ 草稿箱（M5）升级 |
| E5 | 临时表随项目切换清理（架构 §9-I1/I2） | engine 提供 `drop_by_source(Mock)`；project 会话切换时调用 | 切项目后无 `temp_mock_*` 残留 |

### Phase F — 排版分权（管理表 vs 设计这张表，D38）

> ✅ 已实现（与原型 `mock-prototype.html` 对齐）。两条不变式落到可测的纯逻辑上：`job_row_scope`（进度只在一处）
> 与 `table_status`（清单状态点），窗口测试 `the_table_list_keeps_status_and_progress_in_one_place` 锁定。

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| F1 | ✅ 右 Dock 收成「管理表」：移走表名 / 行数 / 种子 / 语言 / 列来源 / 生成行 / 进度行 / 「查看详情」按钮 / 「当前表」选择器，只留清单 + 集合动作 + 出口组 + 折叠的历史 / 模板 | `crates/mock/src/mock_view.rs`（`MockPanel::render` / `render_actions`） | 右 Dock 只四块；导出也在生成前禁用（与原型一致） |
| F2 | ✅ 表清单：单表 / 本次生成 / 结果表三态共用一个渲染器（行 = 状态点 + 表名 + 行数；选中行就是当前表）；点行 → 切 / 开该表的 tab | 同上（`render_table_list`）+ `TableStatus` / `table_status` | 点结果行开那张表的 tab；状态点随 `results` / `landed_tables` / `job` 变；悬空引用优先于「已落库」 |
| F3 | ✅ 关系并进清单行（挂在**子表**行下的 `↳` 子行，行尾 ✕ 删关系）；集合动作与它的进度留在右 Dock | 同上（`outgoing_relations` + `render_job_row(Collection)`） | `outgoing_relations("orders")` = 1、父表无出边；场景进度只在这一处渲染 |
| F4 | ✅ 中央 tab 拿回「设计」：表头（表名 / 行数 / 种子 / 语言输入 + 生成三态 + 场景模板菜单）+ 进度 / 阶段 / 失败重试 | `render_design_head` / `render_generate_row` / `ensure_inputs` | 四项在中央可改且写回同一 `MockDraft`；`job_row_scope` = `Single` 时才有取消 |
| F5 | ✅ 结果表 tab 只读表头（事实 + 跨表后果）与只读列；空态给起手卡片（选模板 / 导入结构 / 手工加列） | `MockDetailView::render` + `relation_note_for` | 结果表 tab 不出输入框与生成按钮；跨表后果与 `relation_note_for` 同源 |
| F6 | ✅ 历史 / 模板折叠（默认收起，标题写条数） | `render_folds`（`Collapsible`）+ `fold_open` | 默认收起；展开后重放 / 应用 / 删除仍可用 |

### Phase G — 生成器覆盖（数值分布族与时序）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| G1 | ✅ 分布族：泊松（到达数）/ 指数（等待时间）/ 帕累托（长尾）/ Beta（比例）/ 二项（成功次数） | `models.rs`（5 个变体）+ `generators.rs`（Knuth 法 + 逆变换 + Marsaglia-Tsang）+ `tools/gen_mock_generator_catalog.py`（标签 / 参数 / 默认值） | 泊松 λ=5 样本均值落 3~8、λ=200 落 180~220（走正态近似）；指数均值 ≈ 1/λ；帕累托恒 ≥ 最小取值；Beta 恒在 0~1 且均值 ≈ α/(α+β)；二项均值 ≈ np |
| G2 | ✅ 时序数值 `TimeSeries`（起始值 + 趋势 + 周期 + 噪声，按行序展开） | 同上 | 相隔一个周期的两行只差 `trend × period`；同 seed 整列一致；`period = 0` 合法（不叠周期，只留趋势） |
| G3 | ✅ 生成前参数护栏扩到分布族 | `engine.rs::generator_param_problem` | λ / α 非正或非有限、`p` 越界、`n = 0` 都在生成前拦住；边界值（`p = 0` / `1`、`λ = 0.5`、`period = 0`）不误拦 |
| G4 | ✅ **列级工作日历**（用户拍板：不做表级共享日历）：仅工作日 + 工作周掩码 + 跳过日期（节假日）+ 上班日期（调休，优先）；`date_time_between` 另有「仅工作时段（09:00~18:00）」（架构 D42） | `models.rs`（3 个日期变体各加字段）+ `generators.rs`（`WorkCalendar` / `advance_work_days` / `datetime_between_with_calendar`）+ `engine.rs`（日历参数护栏）+ `mock_view.rs`（掩码翻人话 + 提示 + 摘要过滤） | 工作日推进跨年 / 跳周末 / 跳节假日 / 调休周六 / 多工作日步长 / 起始落在休息日都正确；随机时刻全落工作日与工作时段；同 seed 可重现 |
| G5 | ✅ 工作时段窗口自定义 + 日历扩到其余日期生成器（本批） | `models.rs`（四个日期时间变体各加 7 个日历字段、`Date` 加 4 个；新增构造器 `date_time` / `date` / `date_time_between`）+ `generators.rs`（`WorkHours`（含跨零点采样）/ `CalendarArgs` / `datetime_with_calendar`）+ `engine.rs`（`work_hours_problem`）+ `schema_map.rs`·`templates.rs`（改走构造器） | 窗口 `HH:MM` 可自定义（起 > 止 = 跨零点，两段都能采到）；`date` / `date_time` / `date_time_before` / `date_time_after` 都能只出工作日；时刻类可同时只出工作日 + 工作时段 |
| G6 | 待做（刻意不做） | —— | ① 内置节假日表（每年发通知，必静默过期）；② 「跟随另一列取值」（跨列求值，独立特性） |

## 4. 测试场景（验收清单）

| 编号 | 场景 | 断言 | 状态 |
| --- | --- | --- | --- |
| T1 | 改单列生成器为「正则」并给 pattern | 生成值全部匹配正则；预览即时反映 | 参数可编辑，正则值由引擎测试兜底 |
| T2 | 列名 `email` / `created_at` / `amount` 的自动映射 | 分别得 SafeEmail / DateTime / RandomDecimal，置信度 `high` | ✅ 引擎 + 视图测试 |
| T3 | 未知列名 + 未知类型（`zzz` + `GEOMETRY`） | 兜底 Sentence + `low` 置信度，不报错 | ✅ 引擎测试 |
| T4 | 行数输入 `0` / `-5` / `abc` | 面板报「行数需为正整数」；不触宿主 | ✅ 视图测试 |
| T5 | 只读项目点四个出口 | 视图层拒绝 + 面板 info/danger 提示；分析库零写入 | ✅ 视图测试 |
| T6 | 追加到同一表（自增主键） | 累计行数翻倍、主键无重复（`MAX(id)=100`） | ✅ 装配测试 |
| T7 | 大行数（≥ 100k）生成与取消 | 可取消；取消后提示「生成已取消」 | ✅ 任务测试（200k 行，批次边界中断） |
| T8 | 应用电商模板 | 4 张临时表生成成功；总行数 = 模板行数之和 | ✅ 引擎测试 |
| T9 | 从源库导入结构（含 `DECIMAL(12,2)` / `TINYINT`） | 类型与生成器映射正确；无回退为文本 | ✅ 类型映射测试 + 导入路径 |
| T10 | 导出 CSV / SQL INSERT | CSV 首行列头、行数 = 生成行数；INSERT 条数 = 行数 | ✅ 集成 + 装配测试 |
| T11 | 保存到草稿箱 | `{项目}/mock/mock_{表}_{时间戳}.csv` 出现 | ✅ 装配测试 |
| T12 | 持久化为分析库表 | 表出现且行数正确；同名再点报「已存在」且不覆盖 | ✅ 装配测试 |
| T13 | 生成后查历史并重放配置 | 历史条目字段完整；重放后参数一致 | ✅ `tests/history_roundtrip.rs`（记录 → 列表 → 详情 → 重放草稿；含失败原因与 `limit` 截尾） |
| T13b | 存模板 → 读模板 → 应用模板 | 行数 / 种子 / 语言 / 列与生成器参数原样回来；**目标表名不被换掉** | ✅ 同上（`a_template_is_saved_listed_replayed_and_deleted`）+ 视图测试（应用不动表名 / 空配置与空名被拦） |
| T14 | 源表不存在 / 分析库无表 / 追加缺列 | 给出可读中文错误，不 panic，不半途写库 | ✅ 装配测试 |
| T15 | **生成不写库** | 生成后分析库仍未出现目标表 | ✅ 装配测试 `generate_does_not_write_analysis_db` |
| T16 | 改列后旧结果作废 | 改生成器 / 增删列 → `gen_info` 置空，出口不可再落旧数据 | ✅ 视图测试 |
| T17 | 后台任务进度可观测 | 进行中报批次进度（3/10 → 30%）；无结果时不显旧预览 | ✅ 视图测试 |
| T18 | 并发提交与异常退出 | 进行中重复提交被拒；工作线程异常退出给可读错误 | ✅ 视图测试 + 任务测试 |
| T19 | 出口（落库 / 导出 / 草稿箱）后台化 | 提交即返回、阶段报写入 / 导出；落库回表内行数并刷新追加候选 | ✅ 视图测试（`persist_job_runs_in_background_and_keeps_preview` 等 4 项）+ 任务测试 4 项 |
| T20 | 出口完成后预览仍可用 | 落库 / 导出只读临时表 → `MockGenInfo` 不作废（可接着导另一个格式） | ✅ 视图测试 + 任务测试 |
| T21 | 出口不可取消 | 出口任务进行中点取消：不转发给宿主、按钮不渲染 | ✅ 视图测试（`rec.cancels == 0`） |
| T22 | 生成器搜索 | 空查＝全量 143；标签前缀优先于标签包含；多词是 AND；大小写不敏感；确认写回该列且置信度转 `manual`；无命中为空 | ✅ 视图测试（5 纯逻辑 + 2 窗口） |
| T23 | 落库跨库直写 | 建表 + 写行一次 `ATTACH` 完成；中文列名、20k 行、目标表多列均正确；**同名建表不删既有数据**；插入失败回滚刚建的表且已解挂 | ✅ 引擎测试 4 项 + 装配测试 2 项 |
| T24 | 临时表清理 | 切项目清掉全部 mock 临时表（两套命名都认、幂等）；`ATTACH` 进来的文件库表不被误删；面板作废旧预览、草稿保留 | ✅ 引擎测试 3 项 + mock 集成 2 项 + 视图测试 1 项 + 任务集成 1 项 |
| T25 | 集合类参数可编辑 | 取值集合往返 / 分隔符变体（半角、全角、制表符，值可带逗号）；非法输入保留上一个有效值 + 行号提示；留空 / 全零权重生成前拦住（不 panic） | ✅ 视图测试 6 项 + 引擎测试 2 项 |
| T26 | 状态单点 | `job_row_scope`：单表任务是 `Single`、场景任务是 `Collection`（同时刻只有一处画进度与取消） | ✅ 窗口测试（`the_table_list_keeps_status_and_progress_in_one_place`） |
| T27 | 清单是唯一入口 | 点单表行开草稿 tab、点结果行开该表 tab；代码里不存在「查看详情」按钮与「当前表」选择器 | ✅ `mock-open-detail` 已删；状态点口径由 `table_status` 锁定 |
| T28 | 关系挂在子表行下 | 子表行下渲染 `↳` 子行；`outgoing_relations` 按子表归组（父表无出边），落父表后 `⚠` 消失 | ✅ 窗口测试（同上） |
| T29 | 分布族与时序的统计性质 | 泊松 λ=5 均值落 3~8、λ=200 落 180~220；指数 λ=2 均值 ≈ 0.5；帕累托恒 ≥ 最小取值；Beta 恒在 0~1 且均值 ≈ α/(α+β)；二项 n=10 p=0.5 均值 ≈ 5；时序隔一个周期只差 `trend × period`、同 seed 整列一致、`period = 0` 只留趋势 | ✅ 单元测试 9 项（宽容差，只校量级） |
| T30 | 分布族非法参数不进入抽样 | λ / α 非正或非有限、`p` 越界、`n = 0` 被 `generator_param_problem` 拦住并给出可读原因；合法边界值不误拦 | ✅ 单元测试（旧拒绝 / 误拦两组用例扩项） |
| T31 | 列级工作日历（顺序日期） | 默认工作周下跳过周末与节假日（跨年正确）；调休周六出现在序列里；步长可以是多个工作日；起始日落在休息日时从下一个工作日起步；日内时刻保持起始值 | ✅ 单元测试 4 组 + 引擎集成 1 项 |
| T32 | 列级工作日历（随机时刻） | 「仅工作日」下取值全落在工作日（跳过日期不出现、调休周六可出现）且不越出给定区间；「仅工作时段」下时刻落在 09:00~18:00 | ✅ 单元测试 2 项（含 1000 次采样）+ 引擎集成 1 项 |
| T33 | 日历参数写错不静默 | 掩码非 7 位 0/1 / 全 0、`2026-1-1` 这类非规范日期串、步长不是整天、列表超 366 条都在生成前拦住并说清格式；未勾「仅工作日」时留着旧值不拦 | ✅ 单元测试（拒绍 / 误拦两组）+ 引擎集成 1 项（错误里点出列名） |
| T34 | 自定义工作时段窗口 | 窗口 `HH:MM` 可自定义（`10:30~12:00` 只出这段）；起 > 止 按跨零点理解且两段都能采到（`22:00~06:00`）；起 = 止 / 解析不了的时刻 / `24:00` 当起点都被拦；未勾开关时不校验 | ✅ 单元测试 2 项 + 护栏单元 4 项 |
| T35 | 日历覆盖所有日期生成器 | `date` / `date_time` / `date_time_before` / `date_time_after` 勾「仅工作日」后只出工作日、跳过列表生效、不越出区间；`Date` 列与序列均能走完整条生成链路 | ✅ 单元测试 2 项 + 引擎集成 2 项 |

## 5. 风险

| 编号 | 风险 | 影响 | 缓解 |
| --- | --- | --- | --- |
| R1 | 面板继续长胖 | `panels/` 已近 9k 行，`mock_view.rs` 近 2k 行 | 视图随 crate 已定；新能力优先放 mock crate，不在 `panels/` 长 |
| R2 | 生成器参数表单与 143 变体手工对齐 | 新增变体漏配表单，用户看到空参数区 | 参数表单由 `GeneratorConfig` 派生（编译期穷尽匹配），禁止手写清单 |
| R3 | 分析库并发写入（面板写 + SQL 执行区写） | Windows 同文件多连接受限，可能出现「文件被占用」 | 统一经 engine 的单连接纪律；必要时串行化写入入口（架构 §9-I3） |
| R4 | ~~大行数同步生成阻塞 UI~~ | 已解决：生成 / 追加 / **三个出口**全部走后台工作线程（进度 + 取消；出口为不定量进度、不提供取消，见架构 D23） | 出口进行中只报阶段（写入 / 导出不可中断） | 若将来要可中断，需引擎侧提供 DuckDB 写入的取消点（目前无） |
| R5 | ~~临时表前缀与 engine 管理器约定不一致~~（架构 §9-I1） | 已解决：清理按**两套前缀**枚举（以库为准），切项目时宿主调 `clear_temp_tables` 并作废面板预览（D27） | 清理后旧预览会作废（面板给一句可读提示）；草稿保留 | 若将来改成项目作用域分析库，切项目天然不带过去，这层清理可退化成冗余 |
| R6 | 「追加」语义被误用为「替换」 | 用户期望覆盖却得到翻倍数据 | 出口命名与确认文案已区分「持久化（新建）」/「追加」；`landed` 行显示当前落库目标 |
| R7 | v1 文档与实现继续漂移（本文档以代码为准） | 新人按 v1 文档实现已不存在的命令 | 本目录文档为权威；v1 素材删除前先提炼 |
| R8 | 临时表名只由目标表名派生（架构 §9-I0e） | 同名目标表并发生成会互相覆盖临时表内容（当前顺序操作为下无影响） | 引入后台并发生成时给临时表名加会话后缀 |

## 6. 验证命令

```bash
cargo check -p rds-mock        --all-targets -j 2
cargo test  -p rds-mock                       -j 2
cargo check -p rds-workbench   --all-targets -j 2
cargo test  -p rds-workbench --test mock_generator -j 2
# 全量：cargo check-all / cargo test-all（.cargo/config.toml 别名，自带 -j 2 与 RUST_MIN_STACK）
```

## 7. 进度记录

| 日期 | 阶段 | 内容 | 结果 |
| --- | --- | --- | --- |
| 2026-09-15 | 迁移核查 | 逐文件比对 v1/v2 mock 模块；发现 v1 集成测试不可编译、文档与实现漂移；定位命令层/视图层缺口 | 见 `README.md` 与 `mock-architecture.md` §9 |
| 2026-09-15 | Phase A（第一段） | A1–A8（集成测试 / SQL 缺陷修复 / 类型串收敛 / 追加语义 / 面板闭环 / 按钮接线 / 清理 / 文档） | 62 单测 + 26 集成 + 4 装配测试全过；两个 crate `check` 通过 |
| 2026-09-15 | Phase A（第二段） | A9–A11（视图随 crate + 语义对话框 + 生成器目录穷尽派生 + 参数 JSON 补丁） | 74 单元 + 8 窗口 + 26 集成全过 |
| 2026-09-15 | Phase A（第三段 · 本轮） | A12–A17：**语义回归**（造新表 / 生成不写库）+ 四个显式出口 + **方案①排版**（中央详情 tab）+ 列模型解放 + 导入源库结构 + 装配层去中转（并修掉自身引入的锁重入死锁） | 93 单元（含 16 窗口）+ 26 集成 + 10 装配全过；两个 crate `check` 零告警 |
| 2026-09-15 | 文档 | 五件套 + crate README 按新语义重写（交互稿重画为 7 场景：空态 / 导入 / 已生成 / 落库反馈 / 只读 / 生成器子菜单 / 列编辑） | 本目录 |
| 2026-09-15 | Phase B5（本轮） | **生成 / 追加转后台任务**：`services::mock_jobs`（工作线程 + 进度槽 + 取消）+ 面板进度条 / 取消按钮 + 120ms 定时泵；`generate_at_with_progress` 接引擎批次回调 | 99 单元（含 34 视图）+ 26 引擎集成 + 10 装配 + 4 任务集成全过 |
| 2026-09-16 | Phase B5（第二段 · 本轮） | **三个出口也转后台任务**：`MockJobKind` 加 `Persist` / `Export` / `Scratchpad`（把 `MockGenInfo` 带进任务）+ `MockJobPhase`（阶段：生成 / 写入 / 导出）+ `JobPaths`（分析库与项目根在 UI 线程解析）；视图侧出口改走 `start_job`、出口任务不渲染取消、完成后**不作废旧预览**；装配层删掉三个已无人调用的同步包装 | 103 单元（含 38 视图）+ 26 引擎集成 + 10 装配 + 8 任务集成全过 |
| 2026-09-16 | Phase B8（本轮） | **生成器搜索**：`search_generators`（标签 / 名称 / 分类，多词 AND，前缀优先排序）+ `GeneratorSearchDelegate`（`ListDelegate`）+ `MockPanel::open_generator_search`（`List` 自带搜索框 / 虚拟化 / 空态）；字段行菜单首项作入口，选择后写回该列；固定「不在 update 里 read 自己」的重入问题（`current` 由 `&mut self` 算出传入） | 110 单元（含 45 视图）+ 26 引擎集成 + 10 装配 + 8 任务集成全过 |
| 2026-09-16 | Phase E（本轮 · 落库直写） | **去文本中转**：engine 新增 `build_attach_database` / `build_detach_database` / `build_create_table_in` / `build_drop_table_in` / `build_insert_select` + `QualifiedTable`；mock 新增 `write_temp_table_to_database`（`ATTACH` → 建表 → `INSERT SELECT` → `DETACH`，失败只回滚本次刚建的表）；装配层 `persist_table_at` / `append_table_at` 改走直写；**并修掉一个潜在的误删风险**（回滚分支原本会把同名既有表 DROP 掉，现已加测试锁住） | 110 单元 + **30 引擎集成** + **12 装配** + 8 任务集成全过；engine 库测试 305 项全过 |
| 2026-09-16 | Phase E5（本轮 · 临时表清理） | **按来源清理临时表**：`TempTableSource::prefixes()`（两套命名都认）+ `TempTableManager::list_by_source` / `drop_by_source`（以库为准，限定 `catalog = memory`）+ `DuckDBManager::{in_memory_temp_tables, drop_in_memory_temp_tables}`；mock 暴露 `clear_temp_tables` / `temp_tables`；宿主在**项目切换**时清理并让面板 `forget_generated`（草稿保留） | 111 单元（含 46 视图）+ 30 + **2 清理集成**（独立进程）+ 12 装配 + **8 任务集成**全过 |
| 2026-09-16 | Phase C5（本轮 · 复杂参数） | **集合类参数可编辑**：`parse_complex_param` / `complex_param_text`（一行一项 / 一行「值, 权重」，分隔符取最后一个）+ `ParamWidget::{Scalar, Complex}`（标量单行、集合多行 `Textarea`）+ `commit_complex_param`（非法输入保留上一个有效值 + 就地行号提示）；`summarize_params` 显示集合项数；**并补上生成前护栏** `constraint_set_problem`（空集合 / 全零权重原本会在生成期 panic 掉工作线程）；目录注释随脚本更新（改 `tools/gen_mock_generator_catalog.py` 后重跑 + rustfmt） | 117 单元（含 52 视图）+ **32 引擎集成** + 2 清理集成 + 12 装配 + 8 任务集成全过 |
| 2026-09-16 | Phase E4（本轮 · 输出改项目级） | **Mock 的输出恒为项目级**（拍板）：`analysis_db_path(project_root)` 取代原 `analytics_db_path()`（全局），落库 / 追加 / 追加候选表全走 `{项目}/.RSmeta/analytics.duckdb`；`JobPaths.db_path` 变为 `Option<PathBuf>`，未打开项目时落库与追加**明确拒绝**（原因里写清替代路径），而**纯生成仍可用**（内存临时表是进程级的，与库无关）；engine 新增 `ProjectDatabaseManager::analysis_db_path`（项目分析库路径的唯一定义处，装配层不再自己拼路径） | 130 单元 + 4 历史/模板集成 + 32 引擎 + 5 持久化 + 2 清理 + workbench 12 装配 + 8 任务 + 1 取消全过 |
| 2026-09-16 | Phase C4（本轮 · 用户模板） | **用户模板接线**：「保存为模板…」对话框（只问名字；行数 / 种子 / 语言 / 列 / 生成器参数都取当前配置）+ 模板段（应用 / 删除）；`template_of_draft` / `draft_of_template` 与历史共用 `ColumnFields` / `StoredColumn` 两份公共映射（两张「列」表字段完全一致，差异只剩父 id 的列名）；**模板不存目标表名**——「怎么造数据」可复用，「造到哪张表」留当时的输入；空配置与空名两道门都落在 `save_template`（对话框只是入口之一）；`HistorySnapshot` 一次读回历史 + 模板（不会出现一半新一半旧） | 130 单元（含 58 视图）+ 32 引擎集成 + **4 历史/模板集成** + 5 持久化往返 + 2 清理集成全过 |
| 2026-09-16 | Phase D4/D5（本轮 · 生成历史） | **生成历史接线**（v1 的「历史」在 v1 源码里并不存在，这里按 v2 语义重做）：`history.rs` 作为**领域门面 + 后台入口**——`HistoryAction::{Record,DeleteTask}` / `list` / `detail` / `run`（跑完动作顺带重读列表，面板不会出现「删了但列表还是旧的」）+ 纯映射 `task_of_run` / `draft_of_detail` / `generator_parts`⇄`config_from_parts`（生成器存**目录名 + 参数 JSON**，重放不需要第二套映射）；`MockHost` 只新增 `project_root()`（存储细节不摊到宿主）；面板新增历史段（时间倒序 + 重放 / 删除 / 刷新）并在生成收尾自动落库（出口类与取消不记，见 `RunRecord::of`）；项目切换时重读（历史随项目走）。**并处理一个环境约束**：项目库走 `tokio::fs`，而 GPUI 后台执行器不是 tokio 运行时 → `history::drive` 在后台线程内自备运行时（与 `resource_jobs` 工作线程同口径） | 128 单元（含 56 视图）+ 32 引擎集成 + **3 历史集成** + 5 持久化往返 + 2 清理集成 + workbench 12 装配 + 8 任务 + 1 取消全过 |
| 2026-09-16 | Phase C4 前置（本轮 · 持久化往返） | **给 `MockGenerationStore` 补真库往返**（原先只有序列化单测，SQL 那一半没人验；store 目前全项目零调用，接线前先钉住）：新增 `tests/persistence_roundtrip.rs` 5 项，走 `ProjectDatabaseManager` 的真迁移链（顺带验证 009 已挂上）——任务 + 列（乱序插入按 `sort_order` 读回）/ 全可空列保持 `None` / 历史最近在前且 `limit` 截尾 / 列按 `task_id` 归属 / **删任务带走子行**（建表语句的 `ON DELETE CASCADE` 靠连接池的 `foreign_keys=ON`）/ 模板四方法；**并修一处读写不守恒**：`created_at` / `updated_at` 为 `None` 时原本写成空串（读回 `Some("")`，与「确实空」分不开），改为写 `NULL` | 117 单元（含 52 视图）+ 32 引擎集成 + **5 持久化往返** + 2 清理集成全过；`check --all-targets` 零告警 |
| 2026-09-17 | Phase C1（本轮 · 场景模板） | **场景模板选择与一键生成**：「场景模板 ▾」菜单（6 套内置，清单在 `MockPanel::new` 里算一次，渲染期零重活）+ `MockJobKind::Scenario(模板 id)` 后台任务 + 装配层 `generate_scenario_at`（跑 `MockEngine::generate_scenario` 后**逐表**补预览）；面板持**多张结果**（`results` / `current` / `scenario_source`），结果区与详情都有「当前表」下拉，四个出口作用于选中那张；场景任务的量纲是「张表」（`batches_done` / `batches_total` 复用为表计数，`rows_total` = 0）。**并修掉一个被场景揭出来的结构性缺陷**：出口原本拿**草稿**当写入规格（`draft.table_name` + `draft.columns`），而场景模板产出的表与草稿毫无关系——现改为出口只看**结果自己**（`MockGenInfo` 新增 `columns`），`persist_table_at` / `append_table_at` / `export_file` 不再收 `draft`；顺带把草稿校验（表名 / 行数 / 至少一列）收拢到 `MockJobKind::uses_draft`（只对生成 / 追加生效） | 135 单元（含 62 视图，新增 5 项场景用例）+ 32 引擎集成 + 4 历史/模板 + 5 持久化往返 + 2 清理 + workbench 12 装配 + **9 任务（含新增「场景一键多表不写库」）** + 1 取消全过 |
| 2026-09-17 | 原型对齐 + 行数文案（本轮） | **原型补上 C1**：`mock-prototype.html` 新增两个可切场景（「场景模板」= 下拉选 6 套内置模板；「场景结果」= 多表 + 当前表展开），并把过期的「场景模板仍待办…本稿暂不画」改成对应关系说明；`mock-prototype-design.md` §4.4 重写为场景模板规格（含菜单文案 / 当前表 / 量纲 / 来源说明四条），原 §4.4 的「尚未落地」四项（场景模板 / 保存模板 / 生成历史 / 复杂参数）**全部已落地**，改成真正的待办（依赖编辑 / 跨表引用 / v1 时序关联）。**行数文案统一千分位**：新增 `with_thousands`（与 `analytics_resource::present` / `insight::model` 各自持一份同口径小助手一致），面板与详情的行数、结果摘要、进度行、历史 / 模板行全部走它——场景模板第一次把五、六位数（161,000 行）摆到菜单上，不加分隔读不出量；**并核实「外键关联」的真实状态**：用范围对齐父表行数**手写的**（逐套核对：ecommerce / hr / finance / social_media / company 恰好落域内，**blog 有 2 处悬空**：`author_id` / `user_id` = 1..200 而无对应父表，v1 同款）；而 `ColumnDependency` / `DependencyType::ForeignKey` 这套**模型**两代都无人写值（`dependency` 全仓库 `None`、生成路径不调用 `resolve_dependencies`），`GeneratorConfig::ForeignKey { values }` 是值集合、v1 原型的模拟实现是 `'FK_' + 随机数`、v1 引擎同名分支是 `#[allow(dead_code)]` 且只取同行源列——结论写进架构 §9-I11（升级为模型化引用：由父表实际行数派生取值域，自增父键 O(1)） | 137 单元（含 65 视图，新增 2 项纯逻辑：千分位 + 场景菜单文案）全过 |
| 2026-09-17 | 表间引用（本轮 · 落实拍板） | **多表生成时可设置表间关系**（用户拍板「一步到位」）：关系挂在**列**上（`ColumnDependency::foreign_key` 是唯一构造点），取值域由父表参数**算出**（父列 `AutoIncrement` + 行数 → `[start, start+step×(n-1)]`），因此**不读任何已落地数据**、O(1) 内存，也**不要求父表先于子表**（自关联与前向引用合法；hr / finance 的表序因此不用调整）。面板：选模板改为**载入工作副本**（不立即生成）→ 列出本次要生成的表 + 表间关系段（可删）+「＋ 加关系」对话框（四下拉；父列只列自增列，加关系时自动把子列生成器对齐到父域）→「生成 N 张表」提交**工作副本**（按 id 重取会丢掉编辑）。校验在生成前（`resolve_reference_domains`，面板提交前也调）。**内置 6 套模板的 24 处引用从「范围手写」改为声明引用**，blog 顺带补上缺失的父表 `users`（200 行，原本 `author_id` / `comments.user_id` = 1..200 悬空，v1 遗留）；新增 4 项模板自检（声明可解析 / 生成器域 ⊆ 父域 / `*_id` 列必须声明或进白名单 / 白名单无幽灵条目） | 145 单元（含 4 项模板关系自检 + 4 项面板关系用例）+ 35 引擎集成 + 4 历史/模板 + 5 持久化往返 + 2 清理 + workbench 12 装配 + 10 任务 + 1 取消全过 |
| 2026-09-17 | 跨表后果 + 历史口径（本轮） | **发现并处理一个关系引入后的真问题**：出口以「当前表」为单位，关系却是跨表的——只落子表不落父表，项目库里就有悬空值（而 mock 不改已落地数据，也不会替用户补）。处置：结果就绪且当前表参与关系时，出口区给一行 **warning**（「引用了 `user_id → users.id`：只落这张表，被引用的表不会跟着落库」/「被 `order_items.order_id` 引用：引用它的一方会落空」），关系快照随结果留存（`last_relations`：工作副本可能已被改或退出场景态，出口提示不能靠它）。**并写清历史口径**：`RunRecord::of` 对 `Scenario` 返回 `None`（历史是**单表配置的重放来源**，场景一次产 N 张表，塞进去反而误导）——面板在场景态明说这一句，免得用户在历史段里找痕迹。架构新增 §9-I14 记录这个洞口与两个未拍板的可选方向（批量落库 / `FOREIGN KEY` DDL） | 147 单元（新增 2 项：跨表后果提示 + 场景不进历史）+ 35 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过 |
| 2026-09-17 | 批量落库 + 原型契合度核对（本轮） | **把 §9-I14 的洞口补上一个可用手段**：「关系里还有 N 张没落库：…」+「落库这 N 张」一键**依次**落库（`MockJobKind::PersistAll`，进度按「张表」，与场景生成同一量纲；`by_table()` 收拢量纲判断）；语义按 **D30**：逐张走「新建」，同名的跳过并报出，**成功的不回滚**（回滚会碰到别人已落好的表）。面板新增 `landed_tables`（本会话已落库；切项目清空——那是另一个库，不随结果清空）、`pending_relation_tables`（关系闭包 − 已落库，按结果表顺序）。**并逐项核对了实现与原型**：28 项用户可见文案/元素全比对，补齐原型里缺的两处（待落库清单 + 批量落库按钮、量纲说明扩到批量落库），其余差异归为三类并已记录（① 已由 note 覆盖：场景/批量任务的「张表」量纲；② 原型有意不画：瞬时态与错误文案；③ 原型画的是填充态：关系空态只在实现里出现） | 149 单元（含 2 项新用例：闭包落库顺序与拒绝空跑、部分失败保留成功）+ 35 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；workbench 新增 1 项批量落库集成 |
| 2026-09-17 | 自定义多表（本轮） | **“多张表”不再只能来自 6 套内置模板**（D31）：工作副本支持「＋ 加表（当前草稿）」与每张表的「删除」。加表取草稿的目标表名 / 行数 / 列——导入源库结构、智能映射、列编辑、行数 / 种子这一整套都在单表态里现成可用，**不必另做一个多表编辑器**（做成多表草稿要把 `MockDraft` 拆成 N 份，波及面板输入 / 列编辑 / 历史模板 / 出口）；重名与空列都拒（加一张生不出东西的表只会拖累整次生成）。删表**连带清掉指向它的入边关系**并报出清了几条（不清的话剩余引用变成「指向模板里没有的表」，用户到生成前才看到错误，那时已不知道是谁指向它）；出边随表一起消失，无需处理。原型与两处设计文档同步（工作副本态补「每表删除 + ＋加表」） | 151 单元（新增 2 项：草稿加表全链路含重名 / 空列 / 生成、删表连带清入边含出边消失与生成仍可跑）+ 35 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过 |
| 2026-09-17 | 编辑表（本轮） | **工作副本里的每张表都能改名字 / 改行数**（D32）：入口是表清单每行的「编辑」，对话框只这两项——行数是最常改的一项（10 万行的财务模板要先缩下来），表名则往往要改成项目里的真实名字。两个跨表后果都在这里一次做完：① 改行数后关系行里的**取值域自动跟着走**（域由父列 `AutoIncrement` + 父表行数算出，新增 `relation_range` 把它显示在关系行尾，改前改后一眼看得出）；② 改名字时**同步所有指向它的 `ref_table`** 并报出同步了几条——不重定向的话，指向它的关系立刻变成「指向模板里没有的表」。校验没过**不收起对话框**（错误就地显示在对话框里，改完能直接再点「应用」；打开时先清掉上一次的错误，免得被当成这次的输入有问题）；表名走与单表路径**同一个** `validate_table_name`（空名 / 非法标识符 / 撞名都在对话框里拦住）。仍不碰已生成的预览与已落地的表（“mock 不可读数据”的边界） | 154 单元（新增 3 项：改行数域跟着走含父列非自增不给域、改名重定向入边且照旧能生成、空名 · 非法标识符 · 重名 · 非法行数都拒且保留对话框）+ 35 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过 |
| 2026-09-18 | 一表一 tab（本轮） | **详情 tab 从「单一 tab」改为「一个目标一个 tab」**（按 `DetailTarget::key()` 去重：草稿 `draft`，每张结果表 `table:{表名}`）：`view.rs` 的宿主命令改为查 `Shared.mock_details`（`HashMap<key, WeakEntity<MockDetailView>>`）——已存在则 `TabGroup::select_tab` 聚焦，不存在才建；`MockDetailView::new` 开始收 `target`，tab 标题写「`Mock · items（1,000 行）`」（行数实时取自结果，重生成或「编辑表」改行数后标题自己更新），**预览取自己那张表**（以前只有一个 tab，预览跟着面板「当前表」跑，两张表并排看就没有差别）；**切 tab 就是切表**——`BasePanel::set_active` 调 `panel.focus_table(表名)` 把那张表设为当前表（四个出口跟着走），右 Dock 的结果表清单是这些 tab 的管理入口（点一行开 / 切，打开即选中）。**身份用 key 不用下标**：`results` 每次生成都会重建，下标做身份会让 tab 指向别的表；表不在本轮结果里时不改状态（tab 可能是上一轮留下的） | 视图测试新增 4 项（一表一 tab · 切 tab 即切表 · 标题带表名与行数 · 预览各看各表）+ `open_detail_calls_host` 补断言；`Shared` 契约白名单与 `panels-coupling-plan` §2 / `panels-modules` §3 耦合表同步 |
| 2026-09-17 | 审计与硬缺陷修复（本轮） | **先把「会毁掉整个 mock 会话」的漏洞补掉**（用户要求排查后继续）。实测确认并修复五类问题，全部在生成/渲染**入口处**设护栏而非靠 `catch_unwind` 兜：① **生成期参数护栏**（D33）——`constraint_set_problem` 扩成 `generator_param_problem`，把区间类（`min >= max` / 反向）、时间区间（fake 走 `(0..分钟差)`，不足一分钟即空区间）、权重（非有限 / 负数 / 全零）全拦在生成前，错误带具体数字；② **映射兜底的空区间**——`schema_map` 的文本兜底原为 `Sentence { min: 1, max: 1 }`（实测必 panic），改 `{1,3}` 与 `default_generator_for` 对齐；③ **panic 不再废掉会话**——`get_conn` 遇锁毒化复用连接并 `warn`（实测毒化后该进程之后每次生成都报 `poisoned lock`），`mock_jobs::worker` 用 `run_job_catching` 兜住 panic → 降级为「这一次任务失败」（此前进度槽永远 `Running`，面板卡在「已有任务在进行中」且取消按钮消失，只能重启）；④ **值覆盖**（D34）——新增 `engine::duckdb::value_text` 作唯一渲染处，实测 `created_at` / `amount` 在预览里是 `Null`、导出的 `.sql` 里是 `NULL`（`row_to_arrow` 只认 5 类变体、`value_to_sql_literal` 只认 9 类），两侧补齐并加端到端用例；⑤ **列名净化一致性**——`build_create_table_ddl` 改用 `sanitize_identifier`（实测 `Order Date` 会产出 `CREATE TABLE … (Order Date VARCHAR …)` → DuckDB `Parser Error`），净化后为空 / 重名报可读错误。另有 ⑥ **「追加到既有表」候选清单**：`refresh_sources` 原本只有测试调用（新会话菜单永远写「暂无表」、切项目后留着旧项目表名），改为打开面板与切项目时刷新、`forget_generated` 清掉项目级清单。**审计还列出未修项**：`parse_data_type` 对 `NUMERIC(10,3)` / `TIMESTAMP WITH TIME ZONE` 等落 `Text`、三份重复的类型解析（含全项目零调用的 `import_schema`）、列依赖表达式仍未接线、持久化 `save_task` 无事务等，见架构 §9-I5/I6/I19 与下一批计划 | 160 单元（23 纯逻辑 + 56 窗口 + 81 其他）+ **37 引擎集成** + 5 持久化 + 4 历史/模板 + 2 清理全过；engine 库测试 336 项全过；workbench 66 lib + 12 装配 + 11 任务 + 1 取消全过；顺带修掉 HEAD 上 `workbench/src/panels/shared.rs` 测试模块缺 `super::` 导致 `--lib` 测试目标编不过的问题 |
| 2026-09-18 | P2 行为 + P3 清理（本轮 · 收尾） | **P2（行为）**：① **取消由面板持续压**（D36）——引擎在生成开始会清一次取消标志，提交后头几百毫秒点的取消会被吞掉，而面板侧「正在取消」不可逆；改为每个轮询周期重发（`cancel` 幂等）；② **跨项目收尾归属**——出口任务的写入路径与临时表名都是提交那一刻快照的，旧行为却在切项目后按「当前项目」叙事、还把结果记进新项目的历史；现在面板记 `job_project`，不一致就追加「写入的是上一个项目」并**不记历史**；③ **切项目不等锁**（D37）——出口任务不可取消且整段持有内存库连接锁，UI 线程同步等锁会假死；改为 `try_lock` + 拿不到就置 `pending_temp_cleanup`、在任务收尾那一拍重试；④ `PersistedAll` 补上导航失效；⑤ **启动恢复的项目也取写锁**（被占用 → 只读 + 提示）——原先 `read_only` 只在交互式打开时写，六个模块的只读护栏在启动实例上全不生效。**P3（清理）**：⑥ `parse_data_type` 改成「剥参数 + 取首词」（`NUMERIC(10,3)` / `INT(11) UNSIGNED` / `DOUBLE PRECISION` / `TIMESTAMP WITH TIME ZONE` 都认，不再静默落 `Text`）；⑦ 删掉三份重复的类型解析（`import_schema` 零调用 / `map_sql_type_to_column_data_type` / `infer_datatype_for_column`）与依赖表达式死代码（`resolve_dependencies` + 两个 `#[allow(dead_code)]` + `DependencyConfig`）——**本模块不解释依赖表达式**；⑧ 删掉三个未在 `lib.rs` 声明的占位文件；⑨ 持久化三条写路径（`save_task` / `save_template` / `delete_template`）改走 rusqlite 事务 | 165 单元（23 纯逻辑 + 63 窗口 + 79 其他）+ **34 引擎集成**（删掉 3 项依赖解析用例）+ 5 持久化 + 4 历史/模板 + 2 清理全过；workbench 侧 check 与四个 mock 测试目标全过；新增 2 项窗口用例（取消重发 / 跨项目归属） |
| 2026-09-18 | 原型改版：管理表 vs 设计这张表（本轮 · 先改原型） | **排版分权（D38，本轮只落原型，实现见 Phase F）**：右 Dock 收回「管理表」——表清单（唯一入口：表名 · 行数 · 状态点，点一行切 / 开它的 tab）+ 出口组 + 折叠的历史 / 模板；中央 tab 拿回「这张表的设计与生成状态」——表名 / 行数 / 种子 / 语言、生成、进度与取消、失败原因与重试、列、预览、跨表后果。**状态单点**：单表任务的进度 / 取消在中央表头，集合任务（场景 N 张表）的在右 Dock 场景清单下（两者互斥），清单行只给状态点与百分比（`◐ orders 40%`）。**订正两处旧结论**：① 右 Dock **可拖拽调宽**——`DockSkin::render_resize_handle` 给每个 placement（含 Right）都画了抓手（`resize-handle-right` + `ResizePanel`），旧文写的「不可拖拽」是错的；② 关系不再单开一段，改为挂在**子表**行下的 `↳` 子行。原型：`mock-prototype.html` 重排右 Dock 与中央区，+2 场景（生成失败（参数护栏）/ 场景生成中（张表进度），共 18 场景），右 Dock 左边缘加了真能拖的调宽抓手 | 交互稿 + `mock-prototype-design.md` §1–§3 / §6–§10、`mock-architecture.md` D18 订正 + D38、本目录 README 同步；实现未动 |
| 2026-09-18 | Phase F（本轮 · 排版分权落地） | **把 D38 原型落到实现**：右 Dock 收成「管理表」——表清单（`render_table_list`：单表 / 本次生成 / 结果表三态共用一个渲染器，行 = 状态点 + 表名 + 行数，点一行切 / 开它的 tab）+ 集合动作与它的进度 + 出口组（生成前禁用，与原型一致）+ 出口反馈 + **折叠**的历史 / 模板（`Collapsible`，默认收起）；中央 tab 拿回「这张表的设计与生成状态」——表头（表名 / 行数 / 种子 / 语言；输入状态仍归面板：`ensure_inputs` / `render_target_rows`）+ 生成三态 + `render_job_row(Single)` + 失败原因与重试，列来源（导入结构 / ＋ 加列）也搬过去；结果表 tab 只读表头（事实 + 跨表后果 `relation_note_for`）与只读列，空态给起手卡片。**删掉**「查看详情」按钮（`mock-open-detail`）与独立的「当前表」选择器（清单点一行就是入口）。**两条不变式落到可测纯逻辑**：`job_row_scope`（进度只在一处：单表在中央、集合在右 Dock）与 `table_status`（已落库 / 未落库 / 引用的表未落库 / 生成中 / 失败；悬空引用优先于「已落库」），关系按子表归组（`outgoing_relations`） | 166 单元（+1 窗口用例）+ 34 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；workbench 12 装配 + 11 任务 + 1 取消全过；`check --all-targets` 零告警；rustfmt 只格式化本轮 hunk（HEAD 原有漂移未动） |
| 2026-09-18 | B7 生成器推荐 / 最近使用（本轮） | **把「常用的一眼可选」补上**：生成器菜单顶部加**「最近使用」**分组（`recent_generators`：`set_generator` 里 `remember_generator` 去重入队，最近在前、最多 5 条；菜单与搜索对话框两条路径都经 `set_generator`，不必各自记一笔）；**「推荐」标记**取 `recommended_generator`（与导入结构同一条 `ColumnMapper::infer` 推理，仅用于标记菜单项，不写回配置——写回仍然只有用户点击或导入）；当前生成器打勾（`PopupMenuItem::checked`），菜单项文案用 `generator_menu_label` 加后缀（组件没有「右侧说明」的位置）。**应用侧验证**：`check -p rds-app --all-targets` 通过（整个二进制装配含新 mock 布局）、workbench lib 74 项全过 | 167 单元（+1 窗口用例 `generator_menu_remembers_recent_picks_and_marks_the_recommendation`）；原型 `menu` 场景同步（最近使用 + 推荐后缀） |
| 2026-09-18 | 口径澄清：落库即项目级持久表（本轮） | **取消 D6「分析资源注册」这条待办**（用户口径）：落库落地的是本项目上的一张**持久表**——它就是分析资源的项目级形态，不存在「再登记一次」也没有触发时机；资源管理器（M6）列的是 `resources/` 下的存档副本（`list_file_archives`，文件语义），本就不应包含项目库里的表。同时订正两处会误导实现的说法：`persist_as_asset` 的「资源注册用」改为「建表在内存库内；写项目库的是装配层 `write_temp_table_to_database`」，概念模型里的「持久表」定义改指项目分析库里的真表。新决策 **D39** 记下被否的三个方案（自动登记 / 登记按钮 / 给 mock 开直写资源库的口子）与理由 | 纯文档：不动代码与测试；`mock-dev-plan` D6、`mock-architecture` §2 · §4.5 · 决策 D39、两处 README 同步 |
| 2026-09-18 | A 步：删空壳 + 列依赖订正（前一批提交 `373cc6d`） | **把「看着能用、实际没人读」的模型字段删干净**：`DependencyType` 四个变体与 `source_columns` / `expression` / `weights` 三个空字段从 `models.rs` 删除，`ColumnDependency { ref_table, ref_column }` **存在即跨表引用**（新决策 D40；文档里关于「依赖表达式」的说明一并订正为「本模块不解释表达式」） | 167 单元（23 纯逻辑 + 64 窗口 + 80 其他）+ 34 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；`check --all-targets` 零告警 |
| 2026-09-18 | 业务日历：核对 `fake` 能力后落地（本轮） | **先核实再实现**：`fake` 5.1.0 的日期时间面只有 Time / Date / DateTime / Duration / DateTimeBefore / After / Between（全区间均匀，裸 `Date` 甚至在公元 1~3000 年里挑），**零日历概念**；仓库也没引任何 holiday 类 crate。用户拍板**日历只做列级**（不要表级共享），于是把它做成生成器参数（新决策 **D42**）：`sequential_date` / `sequential_date_with_gaps` / `date_time_between` 三个变体各加「仅工作日 + 工作周掩码（`1111100` = 周一~周五）+ 跳过日期（节假日）+ 上班日期（调休，优先）」，`date_time_between` 另有「仅工作时段（09:00~18:00）」。顺序日期按**工作日数**推进（日内时刻保持），随机时刻**重抽 + 顺延**（最后夹回区间，端点优先）。实现要点：列表项按 ISO 串**字节序**比较（不建 `NaiveDate`、不排序），「第 k 个工作日」用「估跨度 + 实际工作日数纠正」收敛（不做 O(行数) 逐日扫描），列表各限 366 条（逐行判定的生成期预算）；护栏新增 `work_calendar_problem` / `work_days_step_problem`，并要求日期串是**规范 10 位**（`2026-1-1` 能被 chrono 解析但与字节比较对不上，会静默失效）；面板把掩码翻成人话（`1111100` → 「周一~周五」）并让日历参数**只在启用时占摘要位置**。**不内置节假日表**（每年发通知，必静默过期——过期的表看着像真的） | **185 单元**（+8：工作日推进 4 组 + 随机时刻 2 项 + 掩码展示与摘要 2 项）+ **36 引擎集成**（+2：日历列端到端、错误说清列名）+ 4 历史/模板 + 5 持久化 + 2 清理全过；workbench 12 装配 + 11 任务 + 1 取消全过；`check --all-targets` 零告警；rustfmt 只格式化本轮 hunk（HEAD 原有漂移未动） |
| 2026-09-18 | 工作日历：窗口自定义 + 覆盖全部日期生成器（本轮） | **把上一批留下的两件做完**：① **工作时段窗口可自定义**——`work_hour_start` / `work_hour_end` 两个 `HH:MM` 字段取代写死的 09:00~18:00，起 > 止 按**跨零点（夜班）**理解（两段按长度加权采样），起 = 止 被护栏拦住（`work_hours_problem`）；② **日历扩到其余日期生成器**——`date_time` / `date_time_before` / `date_time_after` / `date` 都接上同一套（`DateTime` / `DateTimeBefore` / `DateTimeAfter` / `DateTimeBetween` 各带 7 个日历字段、`Date` 带 4 个，共**七个**日期生成器），四个带时刻的用 `CalendarArgs` 打包复用同一条实现路径（`datetime_with_calendar`）。**顺带收拾构造点**：日历默认值收进模型构造器 `GeneratorConfig::date_time` / `date` / `date_time_between`，`schema_map`（智能映射 5 处）与 `templates`（两宏）改走它们——以后再加字段不必满仓找构造点。面板：窗口只在勾了开关且非默认值时才进字段摘要 | **190 单元**（+5：自定义窗口 / 夜班窗口 / 日期列工作日 / 其余三个时刻生成器 / 摘要窗口展示；另护栏用例扩了 4 项）+ **37 引擎集成**（+1：日期列 + 夜班窗口端到端）+ 4 历史/模板 + 5 持久化 + 2 清理全过；workbench 12 装配 + 11 任务 + 1 取消全过；`check --all-targets` 零告警；rustfmt 只格式化本轮 hunk（HEAD 原有漂移未动） |
| 2026-09-18 | Phase G（分布族与时序，前一批提交 `b28e02e`） | **把数值与时序补到数据科学家常用的形态**：数值类新增 5 个分布变体（`Poisson` λ / `Exponential` λ / `Pareto` 最小取值·α / `Beta` α·β / `Binomial` n·p）+ `TimeSeries`（起始值 · 趋势 · 周期 · 噪声，按行序展开——与 `sequential_date` 同一行序并排即一条带季节性的时间序列），共 143 变体。分布实现**自建**（Box-Muller / 逆变换 / Marsaglia-Tsang；泊松 `λ ≥ 30` 与二项 `n > 64` 转正态近似，避开单值 O(λ) / O(n) 次循环拖慢十万行生成，新决策 D41）。**参数护栏扩到分布族**：λ / α 非正或非有限、`p` 越界、`n = 0` 都在生成前拦住并给可读原因；`period = 0` 表示「不叠周期」不算错。目录脚本的 `LABELS` / `PARAM_LABEL` / 默认值字典同步（143 断言），并在写完文件后提示「请跑 rustfmt」——脚本产出的是紧凑写法，不格式化就会让每次重跑混进几百行与逻辑无关的换行 | **176 单元**（+9：分布族与时序统计冒烟）+ 34 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；workbench 12 装配 + 11 任务全过；`check --all-targets` 零告警；rustfmt 只格式化本轮 hunk（HEAD 原有漂移未动） |
| 2026-09-20 | UI 审查收口：入口刷新 + 清单行交互 + 尺寸契约（本轮） | 审图后发现的三类问题一次收口。① **四个入口的候选刷新不一致**（功能缺陷）：候选清单与生成历史原先只有 `Shared::open_mock_panel`（导航右键 / 编辑器按钮）与切项目会刷，**活动栏与命令面板这两条入口只改状态**——从那儿进面板会看到上一次的连接清册与既有表清单（又是 §9-I18 那个形态）。现把「展开 Mock 即重读」放进 `Shared::open_right_panel`（四个入口都走它），活动栏 / 命令面板改调它；另拆出 `refresh_connections` 只在连接保存 / 导航重载 / 切项目时刷**连接候选**（内存派生，不开库）而与要开分析库的既有表清单分开；顺带修掉 `refresh_after_open` 的次序——`clear_mock_temp_tables` 先于连接清册换新，导致面板从**旧项目的** `P_`/`GP_` 派生连接候选。② **清单行交互与规范对齐**：选中底从 `accent` 改 `list_active`，未选中行补 `list_hover`（悬停不覆盖激活态），左缘 2px 标识条走共用原语 `workbench_shell::tree::active_bar`，行高钜到 `ui::ROW_HEIGHT`（与导航 / 草稿箱同值），「生成中」那一行补百分比（D38 的 `◐ orders 40%` 口径，结果表清单原先只写「生成中」）。③ **尺寸常量与 UI 契约**：补 `crates/mock/src/ui.rs`（结构尺寸单一来源，与外壳同源的走重导出）、`mock` 加 `workbench_shell` 依赖，`mock/mock_view.rs` 纳入 `ui_contract` 的两份清单（尺寸 + 颜色）与 2c 下沉视图点名表；折叠段的**段序改成与标题一致**（先「生成历史」后「用户模板」，与原型 §2 / 使用手册 §3 同口径）。④ **项目级出口预先禁用**：新增 `MockPanel::sinks_ready`（有结果 + 非只读 + 已打开项目）作为三个项目级出口的唯一判据，未打开项目 / 只读时按钮直接不可点并给一行理由（早先是点了由任务层拒绝，同一句话天天弹）；右 Dock 底部长说明收成一句边界（「要进全局走资产库存档 / 草稿箱」由被拒的出口在需要时给）。⑤ **键盘路径**（订正使用手册早先的「没有专属快捷键」）：`mock::commands::GenerateMock` + `key_context("mock-detail")` + `track_focus`，`Ctrl+Enter` = 生成当前草稿（只在草稿 tab 生效，进行中静默忽略）；键位在 `crates/app` 注册，`workbench::commands` 重导 | **193 单元**（+2：连接候选只刷连接 / 快捷键真按键与结果表 tab 无效）+ 37 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；`cargo check -p rds-mock --all-targets` 与 `-p rds-app --all-targets` 零告警；**workbench 侧已补跑**：`check -p rds-workbench --all-targets` 零告警、`--test ui_contract` 7/7（含新增的 mock 扫描）、`cargo test -p rds-workbench` 全绿（前提：先把 `crates/scratchpad/src/scratchpad_view.rs` 三处被拼坏的文本复原，见本次会话结论；备份在 `.rds/tmp/scratchpad_view.rs.corrupt-backup`） |
| 2026-09-20 | 预览表改走组件库 `DataTable`（本轮续） | **把「只读宽表」这块 UI 资产交还给组件库**：审图时发现预览表是**手搓的固定 9rem 列宽 div 表**（80 行 + 三个局部常量），而结果集（`editor/view/results/grid.rs`）用的就是组件库的 `DataTable` + `TableDelegate`——预览与结果集本是同一类东西，手搓版的代价是具体的：列宽钉死（值一截断就永远看不到全，即上一轮记的 C3）、无斑马纹 / 列菜单、与结果集两套观感、尺寸与交互各要同步一次（当初的组件选型表漏了 `table`，见 `mock-prototype-design` §8 的订正行）。改法：`PreviewTableDelegate`（`columns_count` / `rows_count` / `column` / `render_td` / `cell_text`，首列是 `#` 行号槽，其余走组件 `Column` 默认档 100px + 可拖宽）+ `MockDetailView::ensure_preview_table`（懒创建，`TableState::new` 要 window）+ `sync_preview_table`（在面板通知那一拍把取样推进去、只在真变了时 `refresh`；渲染期只读）。只读取样不开的开关：排序 / 行选 / 列选 / 拖列；行高走组件 `Size::XSmall`（26px）。顺带删掉 `PREVIEW_CELL_WIDTH`（列宽不再由本 crate 定），新增 `PREVIEW_ROW_NUMBER_WIDTH`（48px，与结果集同值，两侧各持一份的原因写在常量注释里：`editor` 目前不依赖 `workbench_shell`，等它也依赖时上提到外壳） | **194 单元**（+1：`preview_table_dumps_the_sample_through_the_component_table`，用 `TableState::dump` 读回表头与行断言 delegate 接线）+ 37 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；`check -p rds-mock / rds-workbench / rds-app --all-targets` 零告警；`ui_contract` 7/7（`mock/mock_view.rs` 仍在扫描内，仍无裸 `px(` / 裸色值） |
| 2026-09-20 | 遗留项收口（本轮续） | 把上两轮列出的剩余项一次做掉。① **旧表 tab 的「为什么没结果」**（D35 定了「旧 tab 不自动关」，但切项目后它说的「这一轮没有它的结果」是假话）：面板新增 `project_switched`（`forget_generated` 置位、下一次生成成功清掉）+ 访问器 `results_dropped_by_project_switch`，结果表 tab 与预览标题据此改说「所属项目已切换：结果已作废」。② **预览右键「拿得走」两件事**：`PreviewTableDelegate::context_menu` 给「复制此值 / 复制整行（TSV）」（右键位置在 `render_td` 里记一笔，与结果集网格同一做法；`context_text` 带行号槽 / 过期位置的回退）；**筛选与排序刻意不做**——那是「前 N 行取样」，筛成子集或排成有序都会让这个唯一的信息失真。③ **任务在跑时的可见性**：面板被切走 / 中央 tab 被关掉就看进度不到、也取消不了——状态栏右侧新增一行 `◐ Mock 生成中 N% · 取消`（`mock_view::status_chip_text` 出文案与量纲，`view.rs::mock_status_chip` 读 `Shared::mock_panel` 的 `job_progress` / `job_kind` / `cancel_requested`，**不新增 `Shared` 字段**，故 `ui_contract` 白名单与耦合表不用动）；出口类任务不给取消（D23），读数仍只属于它自己那一处（D38）。④ 复核后**撤销**一条旧的遗留项：字段区「嵌套滚动」不成立——中央 tab 的 body 自身不滚动，字段区滚动与预览 `flex_1` 是并列关系，「列多时滚字段区、预览始终可见」是设计意图 | **197 单元**（+3：右键取值的回退规则 / 切项目标记 / 状态栏文案量纲）+ 37 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；`check -p rds-mock / rds-workbench / rds-app --all-targets` 零告警；`ui_contract` 7/7；文档同步：使用手册 §3.1–§3.3 + 排查表、原型设计 §3/§6、架构 §11、`panels-modules` §3 的宿主读面板一例 |
| 2026-09-20 | 预览的按列重排 + 单元格悬停全文（本轮续） | **把「排序」这件事做对**：早先刻意不做排序，理由写的是「会把『前 N 行取样』弄失真」——那个理由只对**就地重排**成立。预览只有前 10 行，在 delegate 里重排给的是「前 10 行里最大的几个」，而用户点列头想看的是「这一列最大的几个」；后者只能由库算，所以排序走**重查**（新条 §9-I26）。链路：`SqlEngine::build_select_ordered`（列名当**标识符**加引号，不走表达式解析——与 `select(columns)` 那条路不同，中文 / 带空格列名都安全）→ `MockEngine::read_ordered_preview` / `try_preview_ordered`（**非阻塞**：`try_lock` 拿不到内存库连接锁就回落 `Ok(None)`，理由同 I22——排序是随手动作，不值得为它等一个不可取消的出口任务；`read_preview` / `read_select` 顺带拆开，两条路共用同一段 Arrow 转换）→ 新宿主能力 `MockHost::preview_ordered`（workbench 侧 `mock_generator::preview_ordered` + 复用 `flatten_preview`）→ `MockPanel::sort_preview`。**两个方向循环与箭头都是组件的**：`Column::sortable()` / `Column::sort(ColumnSort)` + `TableDelegate::perform_sort`（默认 → 降序 → 升序 → 默认）；delegate 只把「这一列 + 方向」转给面板，`PreviewSnapshot` 多带一个**生效中的排序**——面板算完（或被拒）后回推，`set_preview` 把排序也算进「变了没」（只变排序也要 `refresh` 重建 `col_groups`），于是表头箭头永远跟实际一致（内存库忙 / 列不存在时它会被纠回去，同时面板给一句可读原因）。**排序自带失效依据**：`PreviewSort` 存一份 `base`（派生它的那份取样），与当前取样不等就不再生效——重新生成 / 切项目不必在十几个 `results` 清空点各手写一行。界面：预览标题改写成「预览（按 id 降序 · 前 10 行）」+ 一行口径说明；右键菜单与表头同一套（升 / 降 / 取消排序，只给「这一列」，取消项只在真有排序时出现）；**筛选仍不做**（取样会变成长度未知的子集）。**单元格悬停全文**：取值先 `truncate`，长 JSON / 长文本只能靠悬停看全——组件库的 `.tooltip()` 只挂在 Button / Checkbox / Switch / Radio / Clipboard（`ComponentTooltip` 是 `pub(crate)`），改用 gpui 自己那层 `StatefulInteractiveElement::tooltip` + 组件库 `Tooltip::element`（宽度走新的 `ui::PREVIEW_TOOLTIP_MAX_WIDTH` = 24rem，超长折行；代价是每个数据单元格要一个 id，行号槽不挂） | **200 单元**（+3：快照含排序（只变排序也要报变更 / 列名对不上 / 取消也是变更）、重查与失效（含内存库忙与重查失败两条拒绝路径）、**从组件库 `TableState` 真调 `perform_sort`**（行号槽不发请求 · 数据列转成一次重查 · 回推后 `delegate().sort` 与 `dump` 都跟上））+ **39 引擎集成**（+2：按列重排取的是**全局**前 N 行（自增列降序首行 = 总行数）+ 锁被占住时回落 `None` 且放锁后能查）+ 5 持久化 + 4 历史/模板 + 2 清理全过；`engine` 侧新增 1 项 SQL 固化用例（引号 / 方向 / LIMIT）；`check -p rds-mock / rds-workbench --all-targets` 零告警；`ui_contract` 7/7（预览新加的悬停 / 菜单部分仍无裸尺寸与裸色值）；文档同步：架构 §2 / §4.1b（新子流）/ §8 / §9-I26 / §10（数目按 `--list` 重数：98 单元 + 102 视图 + 39 引擎集成）/ §11、原型设计 §3 要点行 + §7 尺寸表 + §8 选型表（新增两行）、使用手册 §3.2 + 排查表 + USIT |
| 2026-09-20 | 字段区列搜索（本轮续） | **把「列多了不好找」补上**：预览排序与悬停全文落地后，同一批审图里提的最后一项。字段区原来只有一串卡片（导入一张 80 列的真实表，要改其中一列得一直滚），现在「列（N）」那一行右侧多一个常驻搜索框：按**列名或生成器名**筛（`column_matches`：子串、大小写不敏感——「邮箱」能搜到 `SafeEmail` 的列，因为改列时人记住的往往是「那列是邮箱」而不是列名）；计数变成「列（3 / 50）· 已筛选」，筛没了给一句空态而不是空白。**搜索词只存一份**（输入框实体自己），面板用 `cx.subscribe` 盯 `InputEvent::Change` 重画字段区（订阅必须持有：丢了就自动退订）；筛选逻辑收在 `MockDetailView::visible_columns`——**渲染与用例读的是同一个助手**，不会出现「画一套、测一套」。只作用于草稿 tab（结果表 tab 的列是只读产物）；新增尺寸常量 `ui::COLUMN_FILTER_WIDTH`（9rem，与对话框内输入框同档） | **202 单元**（+2：匹配规则表驱动 + 筛选从渲染那份助手读回来（空词 / 子串 / 大小写 / 筛没 / 清空））+ 39 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；`check -p rds-mock / rds-workbench --all-targets` 零告警；`ui_contract` 7/7；rustfmt 干净；文档同步：原型设计 §3 要点行 + §7 尺寸表、使用手册 §3.2 + 排查表、架构 §11 |
| 2026-09-20 | 预览右键「复制此列」+ 筛选扩到四个靶子（本轮续） | ① **复制此列**：预览右键多一项「复制此列（N 个取值）」（每行一个值，粘进表格就是一列）——与「复制此值 / 复制整行（TSV）」成为一组。**只给取样窗口内的值**（不重查整列）：与「预览是取样」同一口径，而十万行的整列粘到哪里都不好用（真要全列，落库 / 导出后去查那张表）。② **字段区筛选从两个靶子扩到四个**：`column_matches` 现在同时看**列名 / 生成器名 / 类型名（`column_type_label` 给的 DuckDB 类型名）/ 置信度（`high`/`low`/`manual`，卡片上就写着）**，并把多词改成 **AND**（与「搜索生成器」对话框同一约定：`id INTEGER` 只留两条都中的列）——不新增控件，一个搜索框就能干「找出所有时间列」「找出所有按类型猜的列」两件事；顺手把表头的筛选计数改成调 `visible_columns`（原先另写了一份同样的过滤，两处口径会漂） | **202 单元**（本轮不增用例数：把筛选用例改名扩到四个靶子 + 多词 AND，并在右键助手那条用例里补 `column_text` 的取值 / 越界；另新增预览用例仍计 200 那份） + 39 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；`check -p rds-mock --all-targets` 零告警；`ui_contract` 7/7；rustfmt 干净；文档同步：原型设计 §3 要点行（筛选四靶子 + 右键复制三项）、使用手册 §3.2 + 排查表、架构 §11、`crates/mock/README.md` |
| 2026-09-20 | 表头悬停全文 + 订正排序入口（本轮续） | ① **列名也补悬停全文**：`TableDelegate::render_th` 自画表头（截断 + 悬停全文，行号槽不挂），空白列名不挂 id；给用例留 `debug_selector`（`mock-preview-th-{ix}`，`.id()` 不登记坐标）。② **订正一个自己写错的口径**：写排序那轮说的是「**点列头**就是排序」，但组件 0.6.1 的 `on_col_head_click` **只做列选择**（我们关掉了 `col_selectable`），排序只挂在 `render_sort_icon` 上——真实入口是**表头右端那个箭头**（与结果集网格同一处）。代码注释 / 标题提示 / 使用手册（§3.2、排查表、USIT）/ 原型设计（§3、§8）/ 架构 §4.1b 一并改成「箭头」。③ **用真点击看住这条**：新用例在表头坐标上真发点击（`simulate_click`），从右端往左探几次命中箭头即算通过；行号槽那一侧探多少次都不该有反应——它同时看住「表头自己挂了 id/悬停之后，点击还在」 | **203 单元**（+1：表头排序箭头的真点击）+ 39 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；`check -p rds-mock --all-targets` 零告警；`ui_contract` 7/7；rustfmt 干净；文档同步：使用手册 §3.2 + 排查表 + USIT、原型设计 §3 + §8（选型行改名「单元格 / 表头悬停全文」）、架构 §4.1b + §10 + §11 |
| 2026-09-20 | 预览行数档（10/25/50/100/200）+ 重查取样合并（本轮续） | **把「预览只能看 10 行」这道墙拆掉**：`PREVIEW_ROWS` 是**生成时**读了几行的常量（引擎侧不变），而面板现在多一个「行数 ▾」档（`PREVIEW_LIMIT_CHOICES = [10, 25, 50, 100, 200]`，`Button::ghost().xsmall()` + `dropdown_menu`，画在预览标题行右侧）——选多于 10 就是**重查临时表**（生成时只读了 10 行，库不动的话手上就这么多）。顺带把上一轮的「按列重排缓存」推广成**一处** `PreviewCache`（`table` + `base` 取样 + `limit` 档 + 可选排序 + 结果）：排序与扩行数共用一条管道、一套失效规则（取样变了 / 行数档变了就自动失效），两者可叠加（「按 id 降序的前 50 行」）；接口从 `preview_ordered(temp, column, desc, limit)` 收成 `preview_sample(temp, order: Option<(&str, bool)>, limit)`（引擎 `try_preview_sample` / 阅读器 `read_sample_preview`；不排序走 `build_select_all`），宿主端口同名。菜单档位而不是输入框：行数是**重查代价**的旋钮（`LIMIT N`），档位够用就不必解析输入与拦非法值；回到 10 行且无排序时**丢缓存即可**，不查库 | **204 单元**（+1：行数档重查与排序共用缓存（含「忙时档位留着、缓存失效回落生成取样」与「回到 10 行不重查」））+ 39 引擎集成（同一用例内补「不排序 = 多要几行」断言）+ 5 持久化 + 4 历史/模板 + 2 清理全过；`check -p rds-mock / rds-workbench --all-targets` 零告警；`ui_contract` 7/7；rustfmt 干净；文档同步：架构 §2 / §4.1b / §8 / §9-I26 / §10（按 `--list` 重数：98 单元 + 106 视图）/ §11、原型设计 §3 要点行 + §8（新增行数档一行）、使用手册 §3.2 + USIT、两处 README |
| 2026-09-20 | 预览列宽活过重建（本轮续） | 把前两轮两次记下的那条代价真正解决掉：重建表头（重查 / 改行数 / 重新生成）会把**用户拖好的列宽**打回组件默认档（100px），而后端提示是「拖开看全」——两者相互抵消。根因：列宽归组件（`col_groups` 是 `TableState` 的私有状态，`resize_cols` 只改它自己），而 `prepare_col_groups` 重建时只从 `column()` 重取，delegate 当时并不知道宽度。做法：订阅组件的 `TableEvent::ColumnWidthsChanged`（拖完 mouse-up 时发，带全部列宽；`emit` 是 pending effect，回调里再 update 那个实体是安全的），在 delegate 里**按列名**记宽度（不用下标：列集合换了名字才对得上；行号槽的宽是钉死的，不记），`set_preview` 里把不再存在的列名剔掉；`column()` 交回记下的宽度 | **205 单元**（+1：拿真实 `TableState` 发那个事件，断言「记下来了 / 行号槽不跟事件跑 / 重建后还是拖过的宽度 / 列换过就剔掉」）+ 39 引擎集成 + 5 持久化 + 4 历史/模板 + 2 清理全过；`check -p rds-mock / rds-workbench --all-targets` 零告警；`ui_contract` 7/7；rustfmt 干净；文档同步：使用手册 §3.2 + 排查表（订正旧的那条「列宽回默认」）、原型设计 §3 + §7（旧注改成「宽度会活下来」）、架构 §8 |
