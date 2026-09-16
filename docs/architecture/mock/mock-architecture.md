# Mock 数据生成（M7）— 设计理念与架构

> 定位：解释**为什么这样设计、怎么运转**。着手改代码前先读 `README.md`（入口与硬约束），
> 视觉与交互口径见 `mock-prototype-design.md`，任务与进度见 `mock-dev-plan.md`。

## 1. 一句话与不变式

一句话：**输入一份列定义，输出一份可信的测试数据**——生成过程不读真实数据，落点只在分析引擎。

四条不变式（改代码不得破坏）：

| 不变式 | 具体含义 | 现状保障 |
| --- | --- | --- |
| I1 只进分析引擎 | 全 crate 无任何写源库路径；目标只有内存临时表与 `analytics.duckdb` | 代码扫描 + `README` 硬约束 #2 |
| I2 可复现 | 同 `seed` + 同配置 → 逐值相同的序列 | 集成测试 `generate_is_reproducible_with_same_seed` |
| I3 SQL 经构造器 | DDL/DML/DQL 由 `SqlEngine` 生成，消除字符串拼接注入面 | `engine.rs` 模块头注释 + 代码评审点 |
| I4 临时表可识别 | 生成产物一律 `temp_mock_*` 前缀并注册到 engine 的临时表管理器 | `TEMP_MOCK_PREFIX` 常量 + `register_temp_table` |
| I5 生成不写库 | 「生成」只产内存临时表 + 预览；落库只能由显式出口触发 | 视图测试 `generate_produces_preview_without_touching_sinks` + 装配测试 `generate_does_not_write_analysis_db` |
| I6 出口不覆盖 | 「持久化」遇同名表报错；「追加」只追加；两者都不 DROP / 不重写既有数据 | 装配测试 `persist_creates_table_and_rejects_second_run`（断言既有 50 行未变） |
| I7 任务可中断 | 生成 / 追加在后台线程执行，可在批次边界取消，且进度可观测 | 任务测试 `cancel_interrupts_running_job_with_readable_error` + 视图测试 `job_progress_is_mirrored_while_running` |

## 2. 概念模型

```
MockConfig ──┬─ table_name   用户命名的表（用于临时表名与导出目标名）
             ├─ row_count    目标行数（0 直接拒绝）
             ├─ seed         可选种子：Some → 可复现，None → 随机
             ├─ locale       语言/地区（13 种；决定姓名/地址/电话等文本语料）
             └─ columns: Vec<ColumnDef>
                                 └─ name          列名（智能映射的第一依据）
                                    data_type     列类型（13 类；决定 DDL 与兜底生成器）
                                    generator     生成器（137 变体；可自动映射或手工指定）
                                    nullable_ratio 0.0~1.0，>0 时按概率写 NULL
                                    unique        是否要求列值唯一（重试上限 100 次）
                                    dependency    可选列依赖（拓扑排序用）
```

派生概念：

| 概念 | 形态 | 生命周期 |
| --- | --- | --- |
| 内存临时表 | `temp_mock_{sanitize(table_name)}`，engine 进程级内存 DuckDB | 进程内；重名即覆盖（生成前先 DROP）；列名走 `sanitize_identifier`（建表时须同一算法） |
| 预览 | `QueryResult`，只填 Arrow `batches`（**`rows` 为空**，取值须经 `QueryResult::from_batches` 物化） | 每次生成随结果返回 + `preview()` 可按需刷新 |
| 持久表 | `persist_as_asset` / `export(Table)` 产生的正式表 | 随分析库文件 |
| 落盘文件 | CSV / Parquet / Xlsx / SQL INSERT | 由调用方给路径（草稿箱目录 / 用户选择） |
| 生成任务与用户模板 | `{项目}/.RSmeta/project.db` 四张表 | 项目级，跨会话（迁移 `009_mock_generation.sql`） |

## 3. 分层与 crate 归属

```
表现层   workbench（panels.rs 的右 Dock 面板 + view.rs 的中央详情 tab + services/mock_generator.rs 装配）
            │  MockPanel 实体为唯一 UI 状态（两处视图同源）；生成 / 导出在事件路径
服务层   crates/mock（MockEngine / ColumnMapper / templates / MockGenerationStore / mock_view）
            │  无 UI 全局状态（仅一个进程级取消标志）
数据层   engine（内存 DuckDB · SqlEngine · metadata_cache · project_db） + shared（error/models）
```

归属判定（对照 `rds-architecture` 技能）：

- **为什么单独一个 crate**：有独立生命周期（生成任务/模板持久化独立于窗口）、有稳定边界（`MockEngine` 为唯一入口）、使用方 ≥2（workbench 面板 + 未来的 CLI/导出工具）。
- **为什么视图在本 crate**：按「Feature 自持视图」（与 `project::ui` / `settings_view` 同例），mock 配置面板（`MockPanel`）、详情 tab（`MockDetailView`）与三个对话框（导入结构 / 列编辑 / 生成器搜索）随能力同 crate；crate 依赖 `gpui-kit`（UI 基础设施，架构允许），**不依赖 workbench**——生成 / 出口 / 列来源 / 既有表 / 只读标志 / 打开详情 / 重绘均由 [`MockHost`](../../../crates/mock/src/mock_view.rs) 注入。
- **宿主桥**：workbench 侧 `crates/workbench/src/components/mock_host.rs` 实现全部 `MockHost` 能力（转发到 `services::mock_generator` 与 `Shared`）；中央 tab 的加入与聚焦由 `view.rs` 注册在 `Shared::open_mock_detail` 上。

## 4. 五条数据流

### 4.1 直接生成（核心链路）

```
MockConfig ─► generate_with_progress
   ├ 校验：row_count>0、columns 非空、nullable_ratio∈[0,1]（违反即 MockError）
   ├ reset_cancel()（每次生成重置取消标志）
   ├ DROP TABLE IF EXISTS temp_mock_x ; CREATE TABLE temp_mock_x(...)   ← SqlEngine::build_create_table
   ├ 分批（BATCH_SIZE=10_000）逐行 generate_cell → INSERT             ← SqlEngine::build_insert
   │    ├ 唯一列：列内去重集，冲突重试（>100 次报错）
   │    └ nullable_ratio：按概率替换为 NULL
   ├ register_temp_table(temp_mock_x)
   └ read_preview(limit=10) → MockGenerateResult{ preview, row_count, elapsed_ms, … }
```

取消语义：`cancel()` 置位进程级 `AtomicBool`，**在每个批次边界**检查并返回 `MockError::Generation("生成已取消")`；不中断批次内循环。

### 4.2 智能列映射（生成器的自动选择）

```
列名 + 类型串 ─► parse_data_type（唯一类型入口）─► ColumnMapper::infer
     1. 精确名匹配（id / email / created_at …）
     2. 前后缀匹配（_id / is_ / _at …）
     3. 模糊子串匹配
     4. 类型兜底（Varchar→Sentence、Decimal→RandomDecimal、Timestamp→DateTime …）
   ─► ColumnMappingResponse{ generator, confidence: "high"|"low", sample_value }
```

共 ≈91 条列名规则 + 类型兜底，覆盖 137 个生成器变体中的常用项。类型兜底产生的 `confidence` 恒为 `low`（UI 据此提示「按类型猜测」）。

### 4.3 场景模板（多表生成）

```
list_templates() ─► 6 套内置（电商/HR/博客/金融/社交/企业通讯录）
apply_template(id) ─► ScenarioTemplate{ tables: Vec<TemplateTable> }
generate_scenario(&template, on_progress)
   └ 逐表调用 generate_with_progress（seed=None，模板不带种子）
      每张表完成回调 on_progress(已完成表数, 总表数)
```

列依赖（`resolve_dependencies`）只产出**拓扑排序后的列顺序 + 依赖映射**，用于 UI 编辑与顺序提示；当前生成器本身按 `columns` 顺序取值，不解释依赖表达式——这是与 v1 文档描述不一致的地方（见 §9）。

### 4.4 从元数据导入结构

```
ImportSchemaInput{ conn_id, database, schema, tables, connection_type }
   └ metadata_cache（global / project 两种连接类型）取列信息
        └ 列类型 → ColumnDataType（map_sql_type_to_column_data_type）
             └ ColumnMapper 推断 generator → Vec<ColumnDef>
```

### 4.5 出口（三种生命周期）

| 出口 | 目标 | 语义 | 生命周期 |
| --- | --- | --- | --- |
| `export(Csv/Parquet/Xlsx)` | 调用方指定路径 | DuckDB `COPY`（Xlsx 经 JSON 中转） | 文件 |
| `export(SqlInsert)` | 调用方指定路径 | 逐行 `INSERT INTO "<目标表>" ...` 文本（`insert_statements`） | 文件（供外部执行） |
| `export(Table)` | 内存库 | `CREATE TABLE AS SELECT`，并 DROP 临时表（**仅限内存库**，不用于分析库） | 表 |
| `save_to_scratchpad` | `{目录}/mock_{base}_{时间戳}.{ext}` | 目录由调用方给（v2 = `{项目}/mock/`）；返回落盘路径 | 项目文件 |
| `persist_as_asset` | 内存库 | CTAS + DROP 临时表，返回 `(表名, 行数, 列数)`（资源注册用，不经分析库） | 表（资源注册用） |
| `insert_statements`（仅导出脚本用） | 返回 INSERT 文本 | **落库已不再用它**（见 `write_temp_table_to_database`）；保留给「导出 SQL 脚本」那个出口 | 内存字符串 |
| `write_temp_table_to_database`（落库路径） | 目标 DuckDB 文件库 | `ATTACH` 目标库 → （建表）→ `INSERT ... SELECT` → `DETACH`；数据全程在 DuckDB 内部流动 | 表 |

> `insert_statements` 要求**调用方不持有内存库连接锁**：内存库是 `Mutex<Connection>`，同线程重入加锁会死锁
> （本轮真实撞到过一次：`export(SqlInsert)` 先取 `conn` 再调它）。因此 `export` 的 SqlInsert 分支提前返回、不取锁。

### 4.6 装配流（workbench）

**生成（不写库）**：

```
面板草稿（表名 + 列 + 行数/种子/语言）
   └ 追加时：读分析库目标表行数 → 主键自增起点 = 行数 + 1 （append_to 分支）
        └ MockEngine::generate（内存临时表 temp_mock_* + 前 10 行预览）
             └ MockGenInfo（表名 / 行数 / 耗时 / 预览字符串网格）
```

**出口（与生成同一条后台任务链路）**：

| 出口 | 步骤（均在工作线程上执行） |
| --- | --- |
| 持久化为分析库表 | 任务 `Persist`：查既有表（同名 → `Err` 含「已存在」，引导追加）→ 直写建表 + 写行（`ATTACH` → `CREATE TABLE`（列名走 `sanitize_identifier`，与临时表列名同一算法）→ `INSERT ... SELECT` → `DETACH`；插入失败只回滚**本次刚建的表**）→ 返回表内行数 |
| 追加到既有表 | 任务 `AppendTo(表)`：生成 + 校验列 + 直写追加在同一次任务里完成（阶段先 `Generating` 后 `Writing`） |
| 保存到草稿箱 | 任务 `Scratchpad`：项目根由宿主在**提交前**解析（工作线程碰不了 `Shared`）→ `save_to_scratchpad`（时间戳命名）→ 返回文件路径 |
| 另存为 | 系统保存对话框（`prompt_for_new_path`，异步回传）→ 任务 `Export`：`MockEngine::export` |

**后台任务（生成 / 追加 / 三个出口）**：

```
面板（UI 线程）                 services::mock_jobs（工作线程）
  start_job(draft, kind)  ──►  槽：progress = Running{phase, 0, 0, rows}
  ┌ 120ms 定时泵（弱句柄）       │  生成类：MockEngine::generate_with_progress
  ├ job_state()  ◄─────────   │           每批回写 progress{batches_done,total}
  └ take_job_done() ◄──────   收尾：progress = None; done = Some(结果)
       │                        （**同一把锁下一次性收尾**，UI 看不到空洞）
       └─► finish_job：写文案 / 置 landed / 作废旧结果（**仅生成类**）；失败取错误行
  cancel_job()  ──► MockEngine::cancel()（批次边界响应；只有生成类可取消）
```

**阶段（`MockJobPhase`）**：生成类有批次粒度回调 → 定量进度条 + 取消按钮；出口类（`Writing` /
`Exporting`）跑在 DuckDB 与文件系统内部，拿不到中间进度 → 不定量动画（`Progress::loading`），
且**不渲染取消**（强中断会留下半张表 / 半个文件）。视图侧 `cancel_job` 对出口类直接忽略。

**预览与临时表的一致性**：出口类只**读**内存临时表，完成后 `MockGenInfo` 仍有效（落库后可以接着导出）；
只有生成类在收尾时作废旧结果（它重建了临时表）。

为何不能直接在视图里 `spawn`：`MockHost` 是 `Rc<dyn>`（持 `Shared`，非 Send），无法把宿主搬进
GPUI 的后台执行器；因此沿用 `nav_jobs` / `scratchpad_jobs` 的成熟模式——**宿主内部起工作线程**，
只把 owned 数据（草稿 / 路径）跨线程移动。进度用定时泵而非 render 轮询：任务进行中没有任何
其他事件会让面板重绘，不主动唤醒就看不到进度。

**列结构导入（cache-aside）**：

```
SchemaRequest{conn_id, catalog, schema, table}
   └ NavCache（连接级 L2，SQLite）命中 → 列详情
   └ 未命中 → MetadataService::list_columns（进程级桥接运行时）→ 尽力回写 L2
        └ 列详情 → parse_data_type + MockEngine::map_column → MockColumnSpec（含置信度与示例值）
```

为何落库不直连：`MockEngine` 的临时表在 **engine 进程级内存库**，分析库是**文件库**，跨库无法直接 `CTAS`；
目前用 INSERT 文本在分析库连接上执行（已去掉临时文件中转）。后续若 mock crate 提供「生成 → 直接写指定连接」
的接口，应删掉这层文本中转（见 §9-I3）。

## 5. 状态所有权与副作用边界

| 状态 | 所有者 | 读写时机 |
| --- | --- | --- |
| 草稿（表名 / 列 / 行数·种子·语言 / 最近结果 / 出口状态） | `mock::mock_view::MockPanel`（实体，crate 内） | 面板自己的事件路径写；渲染只读 |
| 后台任务槽（进度 + 已完成结果） | `workbench::services::mock_jobs`（进程级单例 + 工作线程） | worker 写、UI 轮询读；`start` / `take_done` 都过同一把锁 |
| 任务进度镜像（含取消请求标志） | `MockPanel.job: Option<MockJobWatch>` | 由定时泵更新；`cx.notify()` 驱动重绘 |
| 字段编辑工作副本 | `mock::mock_view::MockDetailView::draft: Option<ColumnDraft>` | 列编辑对话框打开时建、应用 / 取消时丢弃 |
| 生成器搜索列表 | `ListState<GeneratorSearchDelegate>`（对话框打开时建；由 dialog builder 闭包持有） | `List` 组件的搜索框改 query → `perform_search` 同步过滤；确认写回面板后关对话框 |
| 面板实体句柄 | `Shared.mock_panel`（`WeakEntity<MockPanel>`） | 面板**构造期**登记；导航右键用它定向导入结构（懒创建会让「先右键、后面板未渲染」丢目标） |
| 详情 tab 句柄 | `Shared.mock_detail`（`WeakEntity<MockDetailView>`） | 首次「查看详情」时建并入中央 tab 组；tab 被关闭后实体释放 → 下次重新创建 |
| 打开详情的宿主命令 | `Shared.open_mock_detail`（`Rc<dyn Fn(&mut Window, &mut App)>`） | 由宿主构造期装配（需要 DockArea），与 `editor_clear` 同一口径 |
| 列来源 / 既有表 | 面板状态（由宿主 `MockHost::schema_sources/existing_tables` 提供） | 打开面板 / 导入对话框 / 落库报「已存在」时加载；渲染只读 |
| 取消标志 | mock crate 进程级 `AtomicBool` | 生成前重置、批次边界读 |
| 生成任务 / 用户模板 | 项目 SQLite（`MockGenerationStore`） | 命令式调用（事务内） |

规则：

- **渲染期零副作用**：面板与详情 tab 渲染只读状态，不打开数据库、不解析文件；输入框是唯一的「懒创建」
  （`InputState::new` 需要 window）。
- **副作用只在事件路径**：生成、导入结构、四个出口、刷新候选、打开对话框都由按钮 / 菜单回调触发；
  落库成功后经 `MockHost::notify` 让宿主刷新依赖视图（分析库导航缓存失效）。
- **只读护栏在视图层**：`MockHost::read_only()` 为真时四个写出口直接拒绝且不触宿主（已有窗口测试断言）。
- **两处视图同源**：中央 tab 只持 `Entity<MockPanel>`，字段与预览都从它读、编辑动作写回它（不存在第二份草稿）。

## 6. 决策表

| # | 决策 | 理由 | 被否方案 |
| --- | --- | --- | --- |
| D1 | 生成落**内存 DuckDB 临时表**，不直接写文件库 | 生成是重 CPU 操作，内存表避免反复落盘；预览直接读 Arrow | 直接写分析库文件（每次生成都改文件，失败难回滚） |
| D2 | 临时表名 `temp_mock_*`（保持 v1） | 与 engine 的 `TempTableSource::Mock` 语义对应、便于人工识别 | 改名为 `tmp_m_*`（会破坏 v1 产物与既有脚本，见 §9-I1） |
| D3 | 所有 SQL 走 `SqlEngine`，`COPY` 例外 | 消除拼接与转义错误面；`COPY` 是 DuckDB 专有非标准 SQL | 全量 `format!` 拼接（旧缺陷面） |
| D4 | 导出 `SqlInsert` 用**文本**而非直接执行 | 让调用方决定用哪个连接执行（跨库 / 文件都适用）；文本生成与落文件共用 `insert_statements` | 在 mock crate 内直连目标库（耦合目标连接） |
| D5 | 落库与追加语义归**装配层**（workbench） | 「写哪张表、新建还是追加」属产品决策，不该塞进生成引擎 | 在 `MockEngine::generate` 内判断目标表行数 |
| D6 | 主键自增起点 = 现有行数 + 1（仅追加路径） | 重复追加不应撞主键；新建路径从 1 开始 | 固定从 1 开始（追加必失败） |
| D7 | `parse_data_type` 作为**唯一类型串入口**（宽松匹配） | 源库类型串（`DECIMAL(12,2)`）与内部规范名（`decimal`）共用判定，避免各写一份 | 每个调用方各留一份映射（曾出现 3 份：mock 内部 / workbench / `map_sql_type_to_column_data_type`） |
| D8 | 未知类型回退 `Text`（DDL 同为 `VARCHAR`） | 生成永远有话可说（Sentence 生成器），不阻断用户 | 报错（对未知类型用户无从下手） |
| D9 | `generate_scenario` 的进度回调按**表**粒度 | 与 UI 的「第 k/N 张表」提示粒度一致 | 按行回调（跨表语义混乱） |
| D10 | 取消检查在批次边界 | 实现简单、可预测；10k 行批次的延迟可接受 | 行级检查（热路径原子读 + 分支） |
| D11 | **视图随 mock crate**（`mock_view`），宿主能力 traits 注入 | 与 M1/M3 的「Feature 自持视图」一致；model/service/view 同 crate 降低跨 crate 调用链 | 视图留 workbench（旧方案：状态拆两处、参数表单无法复用 crate 内目录） |
| D12 | 重交互走**语义 Dialog**（生成器选择 / 列配置） | 焦点陷阱 / Escape / 遮罩关闭由组件负责；窄栏（280px）不挤入表单 | 面板内联展开（无焦点语义，窄栏更窄） |
| D13 | 生成器目录**由 models.rs 穷尽派生**（脚本生成 + `spec_of` 穷尽 match） | 137 变体不可能手写对齐（v1 就是手写 1354 行且已漂移）；新增变体会编译失败强制补齐 | 手写生成器清单（漏项 / 漂移） |
| D14 | 列配置用**工作副本**（应用 / 取消） | 与其余模块的对话框语义一致；取消不污染已配置的列 | 即时写回（无法取消，误改难回退） |
| D15 | **生成不写库**（只产内存临时表 + 预览） | 生成是「试算」不是「提交」；写入必须显式点出口，避免误写与不可预览的副作用 | 生成即写分析库（v1 无此语义，v2 早期曾这么实现，已回归） |
| D16 | 目标表是**用户命名的新表**（表名输入） | v1 主路径就是「命名新表 + 组织列」；「往既有表灌数」只是其中一个出口 | 从分析库既有表里选（把 mock 降级为灌数器） |
| D17 | 追加必须**显式选表**，同名不自动追加 | 「持久化」与「追加」是两个语义不同的出口；静默追加会让用户以为新建了一张 | 表已存在时隐式走追加（v1 的 `CREATE TABLE AS SELECT` 会直接报错） |
| D18 | 字段表与预览落**中央 tab**（方案①） | 右 Dock 280px 且不可拖拽调宽，宽表与字段卡片放不下；中央 tab 与编辑器同构（单一权威状态） | 全塞右 Dock（v1 是 380px 可拖拽宽栏，v2 不具备） |
| D19 | 生成器切换走**分类子菜单**，参数在列编辑对话框 | 生成器身份与参数行必须一致；同一对话框内改生成器会让参数行失效（要么重建、要么错位） | 对话框内提供生成器下拉（参数行与生成器不同步） |
| D24 | 分类子菜单**保留**，另加「搜索生成器」对话框（`List` + `ListState`） | 两条路径对应两种心智：知道「属于哪类」→ 翻菜单；只记得名字 → 搜索。搜索同步过滤（137 项全在内存）无 loading 闪烁；搜索框 / 虚拟化 / 上下键 / 空态全是组件的 | 只留子菜单（137 项翻找慢）；把搜索框塞进弹出菜单（菜单只有 item，无输入控件）；自搓输入框 + 滚动列表 |
| D25 | 落库改**跨库直写**（`ATTACH` + `INSERT SELECT`），不再用 INSERT 文本 | 数据全程在 DuckDB 内部流动；大行数下没有「读全量 → 拼文本 → 再解析」两跳与对应的内存峰值 | 继续文本中转；把临时表改成文件表（与「生成是试算」语义冲突） |
| D26 | 回滚**只删本次 `Create` 刚建的表**；追加失败不回滚 | 同名表已存在时建表会失败，此时删表就是删别人的数据（本项目实际存在这个误删风险，已在实现里绕开并加测试锁住） | 「失败就 DROP 目标表」（会把既有数据删掉）；不回滚（留下半成品空表） |
| D27 | 临时表清理**以库为准 + 多前缀**（`list_by_source` / `drop_by_source`），切项目时由宿主调 | 注册表只在新建时写，会漏（旧版本建的 / 上次清理漏掉的）；而 mock 沿用 v1 的 `temp_mock_` 前缀（D2），只认 `tmp_m_` 一套就等于没清。限定 `catalog = memory` 避免误删 `ATTACH` 进来的文件库表 | 只靠注册表枚举（会漏）；只认一套前缀（mock 永远清不到）；按“所有临时表”一刀切（会碰别的来源） |
| D28 | 集合类参数用**多行文本**编辑（一行一项 / 一行「值, 权重」），解析失败保留上一个有效值 | 多行文本比键值对表格轻，且 137 变体零手工对齐（还是同一套目录派生）；值里可带逗号（分隔符取**最后一个**）。同时把「空集合 / 权重全零」提到**生成前**拦住——生成期对它们会 panic，工作线程一挂该进程后续任务全失败 | 集合编辑器表格 / 子对话框（重且要为三个参数各写一套）；只改编辑器不做生成前校验（手改配置 / 旧模板仍会 panic） |
| D20 | 生成 / 追加 / **三个出口**走**后台工作线程 + 进度 + 取消**（`services::mock_jobs`） | 生成是重活，UI 线程 `block_on` 会冻结界面且无法中断；`MockHost` 非 Send，不能在视图里直接 `spawn` | 视图内 `cx.background_spawn`（要求宿主 Send）；不做进度（大行数只能干等） |
| D21 | 进度用**定时泵（120ms）+ 宿主槽**，而非 render 轮询 | 任务进行中没有其他事件触发重绘，不主动唤醒就看不到进度；轮询频率低不抢主线程 | render 内轮询（永远不会被调到）；GPUI 后台执行器直接跑生成（阻塞后台池线程） |
| D22 | 任务收尾「清进度 + 写结果」在**同一把锁下一次性**完成 | UI 侧不可能观察到「既无进度也无结果」的空洞，避免误报「工作线程已退出」 | 两个独立原子量（存在观测窗口） |
| D23 | 出口类任务**不提供取消**，进度改**不定量**形态 | 写入分析库 / 写文件都在 DuckDB 与文件系统内部，探不到中断点；强中断会留下半张表或半个文件 | 假装可取消（点了没用，比没有更差）；干脆不给进度（大行数落库像卡死） |

## 7. 降级与容错矩阵

| 场景 | 行为 | 用户可见提示 |
| --- | --- | --- |
| `row_count = 0` | `MockError::InvalidRowCount(0)` | 「无效的行数: 0」 |
| `columns` 为空 | 面板先拦：「请先添加列：导入源库结构，或手工加列」 | 直接、可操作 |
| `nullable_ratio` 越界 | `MockError::InvalidColumn(...)` | 「列 'x' 的 nullable_ratio 必须介于 0.0~1.0」 |
| 唯一列重试 >100 次 | `Generation("无法为唯一列'x'生成不重复的值")` | 建议减少行数或换生成器 |
| 追加目标表不存在 | `分析库没有表 {t}` | 面板 danger 行；引导先「持久化为分析库表」 |
| 落库同名表 | `分析库已存在表 {t}：请改用「追加到既有表」` | 面板 danger 行 + 刷新追加候选（不覆盖不静默） |
| 追加目标缺列 | `目标表 {t} 缺少列：a、b（列结构需一致）` | 面板 danger 行（不把 DuckDB 原始错误冒到界面） |
| 落库写失败 | `写入分析库失败（已回滚建表）: …` | 面板 danger 行；新表已被 DROP，不留半成品 |
| 未打开项目 | `未打开项目：草稿箱不可用（请先打开或新建项目）` | 面板 danger 行 |
| 任务已在跑 | `已有任务在进行中（请等它结束或先取消）` | 面板 danger 行（按钮同时被禁用，双重护栏） |
| 工作线程不可用 | `后台工作线程不可用` | 面板 danger 行；槽已回滚，不影响后续重试 |
| 任务异常结束 | `后台生成任务异常结束（工作线程已退出）` | 面板 danger 行（既无进度也无结果时才判为异常） |
| 用户取消 | 引擎在批次边界回 `生成已取消` | 面板 danger 行 + 「临时表可能残留部分行，下次生成会重建」；旧结果作废 |
| 出口进行中 | 不渲染取消按钮，`cancel_job` 直接忽略 | 只有不定量进度（写入 / 导出会跑完） |
| 出口失败（同名表 / 路径不可写） | 任务以可读错误收尾 | 面板 danger 行；**预览保留**（可改用「追加」或换路径重试） |
| 集合类参数留空 / 权重全为 0 | 生成前拦住（`MockError::InvalidColumn`） | 「列 'x' 的「外键取值」集合为空：请在列编辑对话框里填取值（一行一个）」；**不会 panic** |
| 集合类参数输入非法 | 保留上一个有效值，不写回生成器 | 对话框内就地 danger 提示，带行号（如「第 2 行「x」：权重需为数字」） |
| 源表列读不到 | `读不到/没有列信息（连接可能已断开，或表不存在）` | 面板 danger 行；不做假导入 |
| 分析库打不开 | 服务层中文错误 | 面板 `error` 行（danger 色）；既有表候选为空清单 |
| 预览不存在的临时表 | `MockError`（DuckDB 报错桥接） | 面板提示 |
| 只读项目 | 视图层直接拒绝，不触服务 | 「只读模式：不允许落库与写文件（仍可生成预览）」 |
| 生成中途取消 | 批次边界返回「生成已取消」 | 面板 `error` 行 |
| Xlsx 导出 | 经 JSON 中转（DuckDB 不原生支持） | 成功文案带格式名 |

## 8. 性能与可观测

- **批量**：10k 行 / 批，批内先拼值再一次性 `INSERT`（避免逐行往返）。
- **行数上限**：面板侧 `MAX_ROWS = 1_000_000`（误输入护栏）；引擎侧不设上限，由 row_count 决定。
- **进度粒度**：按生成批次（`BATCH_SIZE = 10_000` 行/批）回调；面板每 120ms 拉一次，越接近尾声越密。
  提交到首批完成之间显示「准备中…」（总量随首批回调返回）。
- **可观测**：`MockGenerateResult.elapsed_ms`（生成耗时，不含写库）；`tracing::warn!` 用于生成器内的可恢复异常（非法日期回退、语料缺失）。
- **预览成本**：固定前 10 行（`PREVIEW_ROWS`）；`preview()` 支持自定义 limit。

## 9. 已知问题与后续项

**本轮实证（与迁移/文档相关的三点）**

| 编号 | 问题 | 证据 | 处置 |
| --- | --- | --- | --- |
| I0a | v1 的集成测试文件 `v1/backend/tests/mock_engine_tests.rs` **无法编译**（`response.generator.type_name()`、`confidence > 0.0`、`preview.rows` 非空），从未通过 | 该文件引用的 API 在 v1/v2 的 `models.rs` 中都不存在（`confidence` 是 `String`、无 `type_name`）；`read_preview` 只填 `batches` | 本轮按 v2 真实契约重写为 26 项集成测试（`crates/mock/tests/`），并在文件头记录差异 |
| I0b | `persist_as_asset` 与 `export(Table)` 生成非法 SQL：`CREATE TABLE t AS SELECT * FROM SELECT * FROM …` | 集成测试 `persist_as_asset_creates_table_and_consumes_temp` 实测报 Parser Error；根因是 `build_create_table_as_select(table, source_table)` 第二参数是**源表名** | 已修（engine 参数正名 + 两处调用改为传表名 + 新增 `export(Table)` 回归测试） |
| I0c | v1 设计文档与实际实现有出入 | 文档称 `history.rs` / `mock_get_history` / `mock_clear_history` / `mock_re_generate` 已完成，v1 源码中并不存在；文档称生成器 106 变体，实际 137 | 本目录文档以**代码为准**重写；v1 文档仅作素材 |
| I0d | `export(SqlInsert)` 曾**死锁**：重构后它先取内存库连接锁、再调 `insert_statements`（后者又取同一把锁） | 集成测试 `export_sql_insert_writes_insert_statements` 挂住不返回（进程被外部终止时才退） | 已修：`export` 的 SqlInsert 分支提前返回、不持锁；`insert_statements` 文档写明「调用方不得持锁」 |
| I0e | 临时表名只由目标表名派生（`temp_mock_{表名}`） | 同名目标表并发生成会互相覆盖临时表内容（单用户顺序操作为下无影响，集成测试并行必须用不同表名） | 若将来引入后台并发生成，给临时表名加会话后缀 |

**架构级待办（与边界相关）**

| 编号 | 问题 | 影响 | 建议 |
| --- | --- | --- | --- |
| I1 | ~~临时表命名前缀与 `TempTableManager` 约定不一致~~ | 已解决：管理器新增 `TempTableSource::prefixes()`（**两套命名都认**：v2 `tmp_m_` + v1 `temp_mock_`）与 `list_by_source` / `drop_by_source`（**以库里的实际表为准**，不只靠注册表），表名取自库、删完同步注册表 | —— |
| I2 | ~~临时表建在进程级内存单例，切项目不会清~~ | 已解决：`DuckDBManager::drop_in_memory_temp_tables(Mock)`（限定 `catalog = memory` + `schema = main`，`ATTACH` 进来的文件库表不会被误删）由宿主在**项目切换**时调用，并让 mock 面板作废旧预览（表名已失效） | 已足够；将来若改成项目作用域分析库，切项目天然不带过去 |
| I3 | ~~分析库写入是「INSERT 文本 → `execute_batch`」~~ | 已解决：改 **`ATTACH` 跨库直写**（`MockEngine::write_temp_table_to_database`），数据不经 Rust 字符串；回滚只删本次刚建的表（同名既有表不受影响） | —— |
| I4 | 模板/用户模板与生成任务已落 `project.db`，但**无 UI 入口**且无真实 SQLite 往返测试（仅序列化测试） | 能力在代码里，用户在界面上看不到 | Phase C/D 接面板；补 `MockGenerationStore` 的 SQLite 往返测试 |
| I5 | ~~`mock_view.rs` 为占位文件~~ | 已落地：面板与详情 tab 都在 `crates/mock/src/mock_view.rs`；`{commands,model,generator}.rs` 仍是脚手架占位 | 这三个占位文件按全项目统一政策处理（命令层已退役） |
| I6 | 文档称「生成器按依赖表达式计算取值」，实际只做拓扑排序 | 用户可能误以为支持 `price * quantity` 计算 | Phase C 明确：要么实现表达式解释，要么把 `dependency` 降级为「顺序提示」 |
| I7 | ~~生成与落库都是**同步阻塞**调用~~ | 已解决：生成 / 追加 / 三个出口全部走后台工作线程（进度 + 取消，D20/D21/D23） | —— |
| I10 | ~~出口（持久化为分析库表 / 另存为 / 草稿箱）仍是**同步阻塞**调用~~ | 已解决：出口并入 `mock_jobs` 的任务种类（`Persist` / `Export` / `Scratchpad`），阶段上报 + 不定量进度条 | —— |
| I8 | 复杂参数（`ForeignKey.values` / `Sequence.values` / `Weighted.choices`）无编辑入口 | 约束类生成器实际不可配置（面板只显示一句说明） | 提供「集合编辑」弹层（多行文本 / 键值对）或改由模板与导入场景给出 |

## 10. 测试策略

| 层 | 位置 | 数量 | 锁什么 |
| --- | --- | --- | --- |
| 单元 | `crates/mock/src/*.rs`（`#[cfg(test)]`） | 65 | 表名净化、DDL 生成、`generate_cell` 各变体、列名规则表、类型串解析、序列化往返、模板自检、生成器目录自检（3：137 覆盖 / 标签与默认 / 分类往返） |
| 视图 | `crates/mock/src/mock_view/tests.rs`（GPUI headless，窗口根 `Root`） | 52（21 纯逻辑 + 31 窗口） | 解析 / 校验 / JSON 参数补丁 / 摘要文案；**生成器搜索**（空查=全量 / 标签前缀优先 / 多词 AND / 大小写不敏感 / 分类名可搜 / 无命中为空）；**复杂参数**（取值集合往返 / 分隔符变体 / 行号可读错误 / 摘要项数）；面板空态与候选加载；**生成不写库**（三出口调用计数为零）；行数与列校验失败不触宿主；落库新建 → 同名报错；追加按目标表重算自增；只读拦截四个出口；列增删与「改列作废旧结果」；智能默认恢复；定向导入结构；三个对话框可开（导入 / 列编辑 / 生成器搜索）；生成器搜索过滤→确认写回；约束类列的对话框可开 + 集合类参数写回 / 非法输入保留上一个有效值；详情 tab 渲染与 `focus_tab`（含进 Dock 后真正切 tab）；**后台任务**：进度镜像 / 取消 / 提交失败 / 异常结束 / 重复提交被拒；**出口后台化**：落库 / 导出 / 草稿箱的阶段与结果、完成后预览保留、出口不可取消、无生成结果时拒绝提交；**切项目作废旧结果**（草稿保留、无临时表可清时不报提示） |
| 集成（引擎） | `crates/mock/tests/mock_engine_tests.rs` | 32 | 公开 API 端到端：生成 / 预览 / 映射 / 依赖 / 取消标志 / 类型 / 五种导出 / 持久化 / 草稿目录 / 模板 / 场景；**跨库直写**：建表（含中文列名）/ 追加 / 同名建表不删既有数据 / 失败回滚 + 解挂；**集合类参数**：空集合与全零权重生成前拦住 / 填了就能生成且取值来自集合 |
| 集成（临时表清理） | `crates/mock/tests/temp_table_cleanup.rs` | 2 | 清掉本进程全部 mock 临时表（幂等）；同名重复生成只留一张。**独立进程**：清理是进程级动作，与并行用例互相踩 |
| 集成（装配） | `crates/workbench/tests/mock_generator.rs` | 12 | 生成不写分析库；新建 → 同名拒绝（不覆盖）；追加接续主键（`MAX(id)=100` 且无重复）；追加目标不存在 / 缺列的中文错误；**大行数一次落库（20k）**；**目标表多出的列走默认值**；CSV 导出表头；草稿箱无项目拒绝 + 有项目落 `{项目}/mock/`；连接默认库 / schema 预填；类型串映射 |
| 集成（后台任务） | `crates/workbench/tests/mock_jobs.rs` + `mock_job_cancel.rs` | 8 + 1 | 提交即返回 + 进度可读 + 结果一次性取回 + 生成不写库；并发提交被拒且结束后可恢复；追加任务回表内总行数；**出口**：`Persist` 建表回行数 / 同名表回可读错误不覆盖 / `Export` 写出 CSV（表头 + 行数）/ `Scratchpad` 无项目报错 + 有项目落 `{项目}/mock/`；**切项目清理**（宿主入口）；**取消**在批次边界中断并回可读错误（独立进程：`cancel` 是进程级标志） |

回归价值示例：`export_table_creates_named_table_and_drops_temp` 锁 I0b 那个 v1 遗留缺陷；
`export_sql_insert_writes_insert_statements` 锁 I0d 死锁；`generate_is_reproducible_with_same_seed` 锁可复现性；
`parse_data_type_is_public_and_loose` + `map_column_resolves_parameterized_source_types` 锁 D7；
`generate_produces_preview_without_touching_sinks` 锁 D15（生成不写库）；`persist_creates_table_then_reports_existing` 锁 D17；
`read_only_blocks_sinks_but_allows_generate` 锁「只读止于视图」；`persist_job_runs_in_background_and_keeps_preview`
+ `persist_job_creates_table_and_reports_rows` 锁 D23（出口后台化 + 预览不被作废）。

## 11. 实现位置映射

| 设计决策 / 概念 | 代码位置 |
| --- | --- |
| I3 SQL 构造器纪律 | `crates/mock/src/engine.rs`（模块头注释 + 全部 DDL/DML/DQL 调用点） |
| D25/D26 跨库直写与回滚 | `crates/mock/src/engine.rs`（`write_temp_table_to_database` / `TempTableWriteMode`）+ `crates/engine/src/sql/builder.rs`（`build_attach_database` / `build_detach_database` / `build_create_table_in` / `build_drop_table_in` / `build_insert_select`） |
| D27 临时表清理 | `crates/engine/src/duckdb/temp_table.rs`（`TempTableSource::prefixes` / `list_by_source` / `drop_by_source`）+ `manager.rs`（`in_memory_temp_tables` / `drop_in_memory_temp_tables`）+ `crates/mock/src/engine.rs`（`clear_temp_tables` / `temp_tables`）+ `crates/workbench/src/components/project_host.rs`（切项目时清理 + 面板作废）+ `mock_view.rs`（`forget_generated`） |
| D1/D2 内存临时表与命名 | `crates/mock/src/engine.rs`（`TEMP_MOCK_PREFIX` / `get_db` / `sanitize_table_name`） |
| 生成批次与取消 | `crates/mock/src/engine.rs`（`BATCH_SIZE` / `CANCEL_FLAG` / `generate_with_progress`） |
| 生成器实现（137 变体） | `crates/mock/src/generators.rs`（`generate_cell`） |
| D7 类型串唯一入口 | `crates/mock/src/schema_map.rs`（`parse_data_type`）+ `lib.rs` re-export |
| 列名规则与置信度 | `crates/mock/src/schema_map.rs`（`ColumnMapper::exact_rules/suffix/fuzzy/fallback_by_type`） |
| 模板与场景生成 | `crates/mock/src/templates.rs` + `engine.rs::generate_scenario` |
| 任务/模板持久化 | `crates/mock/src/persistence.rs` + `crates/engine/migrations/project_meta/009_mock_generation.sql` |
| D4/D5/D6/D17 装配与追加语义 | `crates/workbench/src/services/mock_generator.rs`（`generate_at_with_progress` / `persist_table_at` / `append_table_at` / `export_file` / `save_scratchpad` / `import_columns`） |
| D20/D21/D22/D23 后台任务 | `crates/workbench/src/services/mock_jobs.rs`（工作线程 + 槽 + `JobPaths` + `start`/`state`/`take_done`/`cancel`）+ `mock_view.rs`（`MockJobKind` / `MockJobPhase` / `MockJobWatch` + 定时泵 + `poll_job`） |
| D11/D18 视图归属与两处排版 | `crates/mock/src/mock_view.rs`（`MockPanel` 右 Dock / `MockDetailView` 中央 tab / `MockDraft` / `MockHost`） |
| D12/D14/D19/D24 对话框与工作副本 | `mock_view.rs`（`open_import_dialog` / `open_column_dialog` / `open_generator_search` / `ColumnDraft` / `generator_menu` / `search_generators` / `GeneratorSearchDelegate`） |
| D28 集合类参数编辑与前置校验 | `mock_view.rs`（`parse_complex_param` / `complex_param_text` / `split_choice` / `ParamWidget` / `commit_complex_param`）+ `crates/mock/src/engine.rs`（`constraint_set_problem`，生成前校验） |
| D13 生成器目录 | `tools/gen_mock_generator_catalog.py` → `crates/mock/src/generator_catalog.rs` |
| 宿主桥 | `crates/workbench/src/components/mock_host.rs`（`MockHost` 实现）+ `crates/workbench/src/panels.rs`（面板构造期创建与句柄登记）+ `crates/workbench/src/view.rs`（详情 tab 宿主命令） |
