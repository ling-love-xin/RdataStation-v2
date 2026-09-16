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
| A10 | **生成器目录穷尽派生**（137 变体分类 / 中文标签 / 参数规格 / 默认构造） | `tools/gen_mock_generator_catalog.py` → `crates/mock/src/generator_catalog.rs` | 目录自检 3 项：137 覆盖、标签与默认构造齐备、变体↔分类往返 |
| A11 | 参数编辑用 **JSON 补丁**（`patch_param`），避开 137 份「表单 → 变体」构造 | `crates/mock/src/mock_view.rs` | 单元测试：整数 / 浮点 / 文本 / 字符串 / `Option` 字段与非法输入保留原值 |
| A12 | **语义回归**：目标表是用户命名的**新表**（表名输入），不再「从分析库既有表里挑着灌数」；**生成不写库**（只产内存临时表 + 预览） | `mock_view.rs`（`MockDraft.table_name` + `run_generate`）、`services/mock_generator.rs`（`generate` vs `persist_table` 拆分） | 视图测试 `generate_produces_preview_without_touching_sinks`（三出口调用计数为零）；装配测试 `generate_does_not_write_analysis_db` |
| A13 | **四个显式出口**：新建分析库表（同名报错 + 回滚）/ 追加到既有表（显式选表、主键自增接续、缺列报错）/ 草稿箱 `{项目}/mock/` / 另存为（系统保存对话框） | `services/mock_generator.rs`（`persist_table` / `append_table` / `export_file` / `save_scratchpad`）、`mock_view.rs`（出口按钮组） | 装配测试 10 项（含 `append_continues_primary_key_sequence`：`MAX(id)=100` 且无重复） |
| A14 | **方案①排版**：右 Dock = 配置 + 出口；**中央「Mock 数据」tab** = 字段卡片 + 预览表（详情持面板实体，状态单一权威） | `mock_view.rs`（`MockDetailView` + `Panel` 协议实现）、`view.rs`（`Shared::open_mock_detail` 宿主命令 + `DockArea::add_panel(Center)`）、`panels/`（构造期创建面板实体 + 句柄） | 视图测试：详情渲染字段与预览、`focus_tab` 幂等（未加入 Dock 时静默返回） |
| A15 | **列模型解放**：增删列 / 改列名与类型 / 13 类型下拉 / 唯一值改 `Switch` / 生成器改**分类子菜单**（137 项仍在，形态从对话框改为子菜单） | `mock_view.rs`（`add_column` / `remove_column` / `ColumnDraft` / `generator_menu` / `rebuild_params`） | 视图测试：列增删、改列后旧结果作废、智能默认恢复、列编辑对话框可开 |
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
| B2 | 生成器选择（137 变体，按 15 分类分组） | ✅ 已完成（字段行的**分类子菜单**） | 选到的变体与 `GeneratorConfig` 一一对应（目录自检测试） |
| B3 | 参数编辑（标量：数值 / 文本 / 布尔；列名 / 类型 / 空值率 / 唯一） | ✅ 已完成（列编辑对话框）；复杂参数待外置入口 | 非法值保留原值；取消不污染目标列 |
| B4 | 行数预设与种子开关（可复现） | ✅ 已完成（行数 / 种子输入 + 校验） | 同 seed 两次生成结果一致（引擎测试兜底） |
| B5 | 生成中态与取消（`generate_with_progress` + `cancel()`） | ✅ 已完成（`services::mock_jobs` 工作线程 + 面板进度条 / 取消 + 定时泵；追加与**三个出口**同走后台） | 大行数生成时进度可见、可中断；出口报不定量进度且不可取消 |
| B6 | 预览（前 10 行） | ✅ 已完成（中央 tab 预览表，`#` 行号 + 横向滚动） | 列名与值来自真实生成结果 |
| B7 | 生成器「推荐」标记与最近使用 | ⬜ 待做（目录已有分类与默认构造） | 常用生成器一眼可选 |
| B8 | 生成器**搜索**（137 项按名称 / 标签） | ✅ 已完成（字段行菜单首项开「搜索生成器」对话框：`List` + `ListState`，同步过滤 + 多词 AND，无命中显空态） | 输入关键词即过滤；确认写回该列 |

### Phase C — 入口扩展

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | 场景模板选择与一键生成（6 套内置） | ✅ 已完成（面板「场景模板 ▾」菜单 + `MockJobKind::Scenario` 后台任务 + `generate_scenario_at` 装配；模板清单构造时缓存，不每帧重算） | 一键产多张表（电商模板 4 张）；逐表进度按「张表」报；结果区/详情支持**当前表**切换，出口作用于选中那张 |
| C2 | 从数据库导入结构 → 字段表 | ✅ 已完成（导入结构对话框 + 导航右键定向；`NavCache` → `MetadataService` cache-aside） | 选连接/库/schema/表 → 字段表自动填充 + 智能映射（含置信度） |
| C3 | 列依赖编辑器 | Mock 面板（依赖 `resolve_dependencies`） | 拓扑顺序正确；**先拍板**是否实现表达式计算（§10-（6）） |
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
| D6 | 分析资源注册（`persist_as_asset` → M6） | Mock 面板 + `analytics_resource` | 生成后可在资源管理器中看到该表 |

### Phase E — 项目作用域与并发

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| E1 | ~~导航右键携带目标表上下文~~ | ✅ 已完成（`SchemaRequest` 含 conn/catalog/schema/table） | 打开面板即已导入该表结构 |
| E2 | ~~源库表结构获取~~ | ✅ 已完成（cache-aside：L2 缓存 → 实时内省） | 与属性面板显示一致 |
| E3 | ~~按源结构在分析库建表~~ | ✅ 已完成（「持久化为分析库表」按草稿列生成 DDL） | 建表 + 灌注行数正确 |
| E4 | 项目作用域分析库（`{项目}/.RSmeta/analytics.duckdb`） | ✅ 已完成（**已拍板：Mock 输出恒为项目级**）——`analysis_db_path(project_root)` 派生自当前项目；未打开项目时落库 / 追加**明确拒绝**（可读原因 + 替代路径），纯生成仍可用 | 打开项目时写项目库；要进全局走资产库存档（M6）/ 草稿箱（M5）升级 |
| E5 | 临时表随项目切换清理（架构 §9-I1/I2） | engine 提供 `drop_by_source(Mock)`；project 会话切换时调用 | 切项目后无 `temp_mock_*` 残留 |

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
| T22 | 生成器搜索 | 空查＝全量 137；标签前缀优先于标签包含；多词是 AND；大小写不敏感；确认写回该列且置信度转 `manual`；无命中为空 | ✅ 视图测试（5 纯逻辑 + 2 窗口） |
| T23 | 落库跨库直写 | 建表 + 写行一次 `ATTACH` 完成；中文列名、20k 行、目标表多列均正确；**同名建表不删既有数据**；插入失败回滚刚建的表且已解挂 | ✅ 引擎测试 4 项 + 装配测试 2 项 |
| T24 | 临时表清理 | 切项目清掉全部 mock 临时表（两套命名都认、幂等）；`ATTACH` 进来的文件库表不被误删；面板作废旧预览、草稿保留 | ✅ 引擎测试 3 项 + mock 集成 2 项 + 视图测试 1 项 + 任务集成 1 项 |
| T25 | 集合类参数可编辑 | 取值集合往返 / 分隔符变体（半角、全角、制表符，值可带逗号）；非法输入保留上一个有效值 + 行号提示；留空 / 全零权重生成前拦住（不 panic） | ✅ 视图测试 6 项 + 引擎测试 2 项 |

## 5. 风险

| 编号 | 风险 | 影响 | 缓解 |
| --- | --- | --- | --- |
| R1 | 面板继续长胖 | `panels/` 已近 9k 行，`mock_view.rs` 近 2k 行 | 视图随 crate 已定；新能力优先放 mock crate，不在 `panels/` 长 |
| R2 | 生成器参数表单与 137 变体手工对齐 | 新增变体漏配表单，用户看到空参数区 | 参数表单由 `GeneratorConfig` 派生（编译期穷尽匹配），禁止手写清单 |
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
