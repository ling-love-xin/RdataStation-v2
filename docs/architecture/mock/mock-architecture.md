# Mock 数据生成（M7）— 设计理念与架构

> 定位：解释**为什么这样设计、怎么运转**。着手改代码前先读 `README.md`（入口与硬约束），
> 视觉与交互口径见 `mock-prototype-design.md`，任务与进度见 `mock-dev-plan.md`。

## 1. 一句话与不变式

一句话：**输入一份列定义，输出一份可信的测试数据**——生成过程不读真实数据，落点只在分析引擎。

九条不变式（改代码不得破坏）：

| 不变式 | 具体含义 | 现状保障 |
| --- | --- | --- |
| I1 只进分析引擎 | 全 crate 无任何写源库路径；目标只有内存临时表与项目分析库 | 代码扫描 + `README` 硬约束 #2 |
| I2 可复现 | 同 `seed` + 同配置 → 逐值相同的序列 | 集成测试 `generate_is_reproducible_with_same_seed` |
| I3 SQL 经构造器 | DDL/DML/DQL 由 `SqlEngine` 生成，消除字符串拼接注入面 | `engine.rs` 模块头注释 + 代码评审点 |
| I4 临时表可识别 | 生成产物一律 `temp_mock_*` 前缀并注册到 engine 的临时表管理器 | `TEMP_MOCK_PREFIX` 常量 + `register_temp_table` |
| I5 生成不写库 | 「生成」只产内存临时表 + 预览；落库只能由显式出口触发 | 视图测试 `generate_produces_preview_without_touching_sinks` + 装配测试 `generate_does_not_write_analysis_db` |
| I6 出口不覆盖 | 「持久化」遇同名表报错；「追加」只追加；两者都不 DROP / 不重写既有数据 | 装配测试 `persist_creates_table_and_rejects_second_run`（断言既有 50 行未变） |
| I7 任务可中断 | 生成 / 追加在后台线程执行，可在批次边界取消，且进度可观测 | 任务测试 `cancel_interrupts_running_job_with_readable_error` + 视图测试 `job_progress_is_mirrored_while_running` |
| **I8 输出项目级** | 落库 / 追加**只写当前项目**的分析库（`{项目}/.RSmeta/analytics.duckdb`）；未打开项目时明确拒绝（并给出替代路径），**纯生成仍可用** | 装配层 `analysis_db_path(project_root)` + `JobPaths.db_path: Option<_>`；升级路径经 M6 存档 / M5 草稿箱 |
| **I9 只取结构，不取数据** | 与源库的**唯一沟通**是「表结构」（列名 / 类型 / 可空 / 主键），经元数据路径取：`NavCache` L2 → `MetadataService` 实时内省（cache-aside）。**没有跨库取数、也没有跨库引用**——mock 不读源库一行数据。文档里出现的「跨库直写」只指 DuckDB `ATTACH`（进程内内存临时表 → 项目分析库文件），与源库无关 | 装配层 `import_columns` 只调 `list_columns`；全 crate 无任何源库查询路径 |

## 2. 概念模型

```
MockConfig ──┬─ table_name   用户命名的表（用于临时表名与导出目标名）
             ├─ row_count    目标行数（0 直接拒绝）
             ├─ seed         可选种子：Some → 可复现，None → 随机
             ├─ locale       语言/地区（13 种；决定姓名/地址/电话等文本语料）
             └─ columns: Vec<ColumnDef>
                                 └─ name          列名（智能映射的第一依据）
                                    data_type     列类型（13 类；决定 DDL 与兜底生成器）
                                    generator     生成器（143 变体；可自动映射或手工指定）
                                    nullable_ratio 0.0~1.0，>0 时按概率写 NULL
                                    unique        是否要求列值唯一（重试上限 100 次）
                                    dependency    可选列依赖：跨表引用（ref_table / ref_column，存在即引用）
```

派生概念：

| 概念 | 形态 | 生命周期 |
| --- | --- | --- |
| 内存临时表 | `temp_mock_{sanitize(table_name)}`，engine 进程级内存 DuckDB | 进程内；重名即覆盖（生成前先 DROP）；列名走 `sanitize_identifier`（建表时须同一算法） |
| 预览 | `QueryResult`，只填 Arrow `batches`（**`rows` 为空**，取值须经 `QueryResult::from_batches` 物化） | 每次生成随结果返回 + `preview()` 可按需刷新；**按列重排**则是另一次查询（`try_preview_ordered`，见 §4.6 与 I26），视图侧只持「生成时那份取样 + 正在生效的排序」 |
| 持久表 | 落库 / 追加在**项目分析库**里建的真表（装配层 `write_temp_table_to_database`：`ATTACH` → 建表 → `INSERT SELECT` → `DETACH`）；它就是分析资源的**项目级形态**（无「再登记」一步） | 随分析库文件（`{项目}/.RSmeta/analytics.duckdb`） |
| 落盘文件 | CSV / Parquet / Xlsx / SQL INSERT | 由调用方给路径（草稿箱目录 / 用户选择） |
| 生成任务与用户模板 | `{项目}/.RSmeta/project.db` 四张表 | 项目级，跨会话（迁移 `009_mock_generation.sql`） |

## 3. 分层与 crate 归属

```
表现层   workbench（panels/ 的右 Dock 面板 + view.rs 的中央详情 tab + services/mock_generator.rs 装配）
            │  MockPanel 实体为唯一 UI 状态（两处视图同源）；生成 / 导出在事件路径
服务层   crates/mock（MockEngine / ColumnMapper / templates / MockGenerationStore / mock_view）
            │  无 UI 全局状态（仅一个进程级取消标志）
数据层   engine（内存 DuckDB · SqlEngine · metadata_cache · project_db） + shared（error/models）
```

归属判定（对照 `rds-architecture` 技能）：

- **为什么单独一个 crate**：有独立生命周期（生成任务/模板持久化独立于窗口）、有稳定边界（`MockEngine` 为唯一入口）、使用方 ≥2（workbench 面板 + 未来的 CLI/导出工具）。
- **为什么视图在本 crate**：按「Feature 自持视图」（与 `project::ui` / `settings_view` 同例），mock 配置面板（`MockPanel`）、详情 tab（`MockDetailView`）与三个对话框（导入结构 / 列编辑 / 生成器搜索）随能力同 crate；crate 依赖 `gpui-kit`（UI 基础设施，架构允许），**不依赖 workbench**——生成 / 出口 / 列来源 / 既有表 / 只读标志 / 打开详情 / 重绘均由 [`MockHost`](../../../crates/mock/src/mock_view.rs) 注入。
- **宿主桥**：workbench 侧 `crates/workbench/src/components/mock_host.rs` 实现全部 `MockHost` 能力（转发到 `services::mock_generator` 与 `Shared`）；中央 tab 的加入与聚焦由 `view.rs` 注册在 `Shared::open_mock_detail` 上——**一个目标一个 tab**（按 `DetailTarget::key()` 去重：草稿 `draft`，每张结果表 `table:{表名}`）。

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

### 4.1b 预览的按列重排（重查，不是本地重排）

```
表头点列头 / 右键菜单「按此列升序|降序」
   └ PreviewTableDelegate::perform_sort（组件库算好方向循环）
        └ MockPanel::sort_preview（面板：唯一握有宿主端口与结果清单的地方）
             └ MockHost::preview_ordered → mock_generator::preview_ordered
                  └ MockEngine::try_preview_ordered（**try_lock**：拿不到内存库连接锁就 Ok(None)）
                       └ read_ordered_preview → SqlEngine::build_select_ordered
                            └ SELECT * FROM temp_mock_x ORDER BY "col" [DESC] LIMIT 10
             └ 成功：把这一份连同 base（派生它的取样）存进 panel.preview_sort
                  └ cx.notify() → 详情 tab 的 observer 推快照（列 / 行 / **生效中的排序**）
                       └ TableState::refresh（重建 col_groups ⇒ 表头箭头与实际一致；被拒时箭头被纠回）
```

为什么是重查：预览只有前 10 行，就地重排给的是「前 10 行里最大的几个」，而用户想要的是「这一列最大的几个」——后者只能由库算（I26）。
为什么非阻塞：重查在事件路径上跑，而出口任务整段持有内存库连接锁且不可取消（I22 / D23）。

### 4.2 智能列映射（生成器的自动选择）

```
列名 + 类型串 ─► parse_data_type（唯一类型入口）─► ColumnMapper::infer
     1. 精确名匹配（id / email / created_at …）
     2. 前后缀匹配（_id / is_ / _at …）
     3. 模糊子串匹配
     4. 类型兜底（Varchar→Sentence、Decimal→RandomDecimal、Timestamp→DateTime …）
   ─► ColumnMappingResponse{ generator, confidence: "high"|"low", sample_value }
```

共 ≈91 条列名规则 + 类型兜底，覆盖 143 个生成器变体中的常用项。类型兜底产生的 `confidence` 恒为 `low`（UI 据此提示「按类型猜测」）。

数值类可覆盖常见的统计形态（不只均匀与正态）：**对数正态**（收入 / 价格）、**泊松**（到达数 / 计数）、**指数**（等待时间 / 间隔）、
**帕累托**（长尾 / 幂律）、**Beta**（比例 / 比率）、**二项**（成功次数）、**时序数值**（起始值 + 趋势 + 周期 + 噪声，按行序展开，
与 `sequential_date` 同一行序并排就是一条时间序列）。分布实现自建（D41）。

**工作日历也是列级参数**（D42）：「仅工作日」把列限制在工作日上——顺序日期**按工作日数推进**（跳过休息日，日内时刻保持），
其余日期生成器落在休息日就重抽（重抽不成顺延）；工作周掩码（`1111100` = 周一~周五）表达单双休，
「跳过日期」（节假日）与「上班日期」（调休，优先）两个列表表达年度例外；四个带时刻的生成器另有「仅工作时段」，
**窗口可自定义**（`HH:MM`，起 > 止 按跨零点 / 夜班理解）。**不内置节假日表**：法定节假日与调休每年发通知，
内置表必然静默过期（数据看着像真的，只是落在不该上班的那天）。参数在**每一列自己身上**（列与列之间不共享）。
仍缺：「跟随另一列取值」（跨列求值）。

### 4.3 场景模板（多表生成）

```
list_templates() ─► 6 套内置（电商/HR/博客/金融/社交/企业通讯录）
apply_template(id) ─► ScenarioTemplate{ tables: Vec<TemplateTable> }
generate_scenario(&template, on_progress)
   └ 逐表调用 generate_with_progress（seed=None，模板不带种子）
      每张表完成回调 on_progress(已完成表数, 总表数)
```

v2 的接线（`MockJobKind::Scenario(工作副本)`）：

```
面板「场景模板 ▾」──► open_scenario(id)：按 id 取模板，**载入为工作副本**（不生成）
                          ├ 列出本次要生成的表（名称 + 行数）
                          └ 「＋ 加关系」对话框：子表.列 → 父表.列（父列只列自增列）
       「生成 N 张表」──► mock_jobs（工作线程）──► mock_generator::generate_scenario_at(template)
                                                   ├ resolve_reference_domains（错就不提交）
                                                   ├ generate_scenario（逐表：引用列走父域采样）
                                                   └ 逐表 MockEngine::preview 补预览
        ◄── ScenarioGenerated { template_name, tables: Vec<MockGenInfo> }
```

语义要点：

- **不写库**：与单表生成同一定律（只产 `temp_mock_*`），未打开项目也能跑；
- **先载入再生成**（不是选即生成）：用户要能先看「本次生成哪几张表」、并把关系调好；
  关系是**这次生成的一部分**，生成完再补就只能去改已落地的数据了；
- **草稿不参与**：目标表 / 列 / 行数全来自模板，草稿既不被读也不被改写
  （面板的草稿校验只对 `Generate` / `AppendTo` 生效，见 `MockJobKind::uses_draft`）；
- **任务带的是工作副本**：关系挂在列的 `dependency` 上，按 id 重取会把编辑丢掉；
- **工作副本是可改的（自定义多表）**：「＋ 加表（当前草稿）」把单表态调好的表加成场景的第 N 张
  （导入结构 / 智能映射 / 列编辑都在单表态里现成可用，不必再做一个多表编辑器）；
  「删除」把表从本次生成里去掉，**并连带清掉指向它的入边关系**——不清的话剩余引用会变成
  「指向模板里没有的表」，用户到生成前才看到错误，那时已经不知道是谁指向它了
  （出边随表一起消失，无需处理）；
  「编辑」改这张表的**表名 / 行数**（见 D32）：行数是最常改的一项（10 万行的财务模板要先缩下来），
  而关系行里的取值域**会自动跟着变**（域由父表行数算出），改名字则**自动重定向指向它的入边**；
- **多张结果 + 当前表**：面板持 `results` 与 `current`，结果区与四个出口都作用于选中那张；
  **中央一个目标一个 tab**（每张结果表一个 + 一个草稿 tab，见 D35）：右 Dock 的结果表清单就是这些 tab
  的管理入口（点一行打开 / 切过去，行尾带状态点：`✓ 已落库` / `○ 未落库` / `⚠ 引用的表未落库`），
  tab 被激活时把那张表设为当前表（**切 tab 就是切表**）；生成本身不开 tab（结果表多时不淹用户），按需打开；
- **量纲是「张表」**：`MockJobProgress.batches_done / batches_total` 复用为表计数，`rows_total = 0`；
  集合任务的进度行与取消画在**右 Dock 的场景清单下**（它属于这一批，不属于某张表），单表任务反之在中央表头（D38）；
- **出口只看结果**：`MockGenInfo` 自带 `columns`，场景表因此能各自落到自己的表名下（见 §4.5）。

#### 表间引用（取值域由父表参数算出）

关系挂在**列**上（`ColumnDependency::{foreign_key, is_foreign_key}` 是唯一构造点）：

```
orders.user_id ──(dependency: ForeignKey{ref_table: users, ref_column: id})──► users.id
```

- **域是算出来的**：父列若是 `AutoIncrement { start, step }`，域 = `[start, start + step×(父表行数-1)]`
  ——O(1)、零内存、**不读任何已落地的数据**（不查库、不开文件）；
- **因此不要求父表先于子表**：域与“哪张表先跑”无关，自关联与前向引用都合法
  （`hr` 的 `employees → departments`、`finance` 的 `transactions → accounts` 表序不用改）；
- **单表生成**没有父表上下文，按该列自己的 `generator` 取值——它的域必须**落在父域内**，
  由 `templates` 的自检测试锁定；面板加关系时自动把它对齐到父域；
- **能算才能引用**：父列不是自增（uuid / 随机）直接拒绝并给可读理由，不猜也不去读数据；
- **校验在生成前**：`MockEngine::resolve_reference_domains` 检查父表 / 父列 / 可算域，
  面板提交前也调它（错就地显示，不等到生成中途）。

列依赖字段 `dependency` = **跨表引用**（`ColumnDependency { ref_table, ref_column }`，存在即引用）：
`resolve_reference_domains` 生成前校验并算取值域，`generate_table` 按域采样（见 D29 / §9-I11）。
v1 的 `DependencyType`（Expression / Template / Sequence / Weighted）连同一串只服务于它们的空字段已删除（D40）——**本模块不解释依赖表达式**。
也没有「拓扑排序」这一步：列按 `columns` 顺序取值，跨表引用不依赖列间顺序（域由父表参数算出，父表先跑后跑都合法）。

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
| `persist_as_asset` | 内存库 | CTAS + DROP 临时表，返回 `(表名, 行数, 列数)`（**建表在内存库内**；写项目分析库的是装配层的 `write_temp_table_to_database`） | 表（内存库；供不需要落文件的调用方） |
| `insert_statements`（仅导出脚本用） | 返回 INSERT 文本 | **落库已不再用它**（见 `write_temp_table_to_database`）；保留给「导出 SQL 脚本」那个出口 | 内存字符串 |
| `write_temp_table_to_database`（落库路径） | 目标 DuckDB 文件库 | `ATTACH` 目标库 → （建表）→ `INSERT ... SELECT` → `DETACH`；数据全程在 DuckDB 内部流动 | 表 |

> `insert_statements` 要求**调用方不持有内存库连接锁**：内存库是 `Mutex<Connection>`，同线程重入加锁会死锁
> （本轮真实撞到过一次：`export(SqlInsert)` 先取 `conn` 再调它）。因此 `export` 的 SqlInsert 分支提前返回、不取锁。

### 4.6 装配流（workbench）

**生成（不写库）**：

```
面板草稿（表名 + 列 + 行数/种子/语言）
   └ 追加时：读项目分析库目标表行数 → 主键自增起点 = 行数 + 1 （append_to 分支）
        └ MockEngine::generate（内存临时表 temp_mock_* + 前 10 行预览）
             └ MockGenInfo（表名 + **列定义** + 行数 / 耗时 / 预览字符串网格）

场景模板（`Scenario(模板 id)`，同样不写库、同样不读草稿）
   └ templates::get_template_by_id → MockEngine::generate_scenario
        └ 逐表 MockEngine::preview → Vec<MockGenInfo>（每张表都是完整的出口输入）
```

**出口（与生成同一条后台任务链路）**：

出口的输入是**结果自己**（`MockGenInfo`）——目标表名取 `info.table_name`，建表 DDL 取 `info.columns`，
数据取 `info.temp_table_name`。**不读草稿**的原因：场景模板产出的多张表与草稿毫无关系，
拿草稿当写入规格会建出「数据是场景表的、列名是草稿的」的表。

| 出口 | 步骤（均在工作线程上执行） |
| --- | --- |
| 持久化为项目分析库表 | 任务 `Persist(info)`：查既有表（同名 → `Err` 含「已存在」，引导追加）→ 直写建表 + 写行（`ATTACH` → `CREATE TABLE`（列名走 `sanitize_identifier`，与临时表列名同一算法）→ `INSERT ... SELECT` → `DETACH`；插入失败只回滚**本次刚建的表**）→ 返回表内行数 |
| 追加到既有表 | 任务 `AppendTo(表)`：生成 + 校验列 + 直写追加在同一次任务里完成（阶段先 `Generating` 后 `Writing`） |
| 保存到草稿箱 | 任务 `Scratchpad(info)`：项目根由宿主在**提交前**解析（工作线程碰不了 `Shared`）→ `save_to_scratchpad`（时间戳命名）→ 返回文件路径 |
| 另存为 | 系统保存对话框（`prompt_for_new_path`，异步回传；默认文件名取**当前结果表**）→ 任务 `Export{info}`：`MockEngine::export` |
| 落库关系里的表（批量） | 任务 `PersistAll(Vec<MockGenInfo>)`：**依次**走「新建」语义（同名跳过并报出），成功的保留（不回滚别人的表）；进度按「张表」 |

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

为何落库不直连：`MockEngine` 的临时表在 **engine 进程级内存库**，项目分析库是**文件库**，跨库无法直接 `CTAS`；
目前用 INSERT 文本在项目分析库连接上执行（已去掉临时文件中转）。后续若 mock crate 提供「生成 → 直接写指定连接」
的接口，应删掉这层文本中转（见 §9-I3）。

## 5. 状态所有权与副作用边界

| 状态 | 所有者 | 读写时机 |
| --- | --- | --- |
| 草稿（表名 / 列 / 行数·种子·语言 / 最近结果 / 出口状态） | `mock::mock_view::MockPanel`（实体，crate 内） | 面板自己的事件路径写；渲染只读。结果区是**一组**结果表（`results` + `current` + `scenario_source`）：单表生成 1 条，场景模板 N 条 |
| 后台任务槽（进度 + 已完成结果） | `workbench::services::mock_jobs`（进程级单例 + 工作线程） | worker 写、UI 轮询读；`start` / `take_done` 都过同一把锁 |
| 任务进度镜像（含取消请求标志） | `MockPanel.job: Option<MockJobWatch>` | 由定时泵更新；`cx.notify()` 驱动重绘 |
| 字段编辑工作副本 | `mock::mock_view::MockDetailView::draft: Option<ColumnDraft>` | 列编辑对话框打开时建、应用 / 取消时丢弃 |
| 生成器搜索列表 | `ListState<GeneratorSearchDelegate>`（对话框打开时建；由 dialog builder 闭包持有） | `List` 组件的搜索框改 query → `perform_search` 同步过滤；确认写回面板后关对话框 |
| 面板实体句柄 | `Shared.mock_panel`（`WeakEntity<MockPanel>`） | 面板**构造期**登记；导航右键用它定向导入结构（懒创建会让「先右键、后面板未渲染」丢目标） |
| 详情 tab 句柄 | `Shared.mock_details`（`HashMap<目标 key, WeakEntity<MockDetailView>>`） | 首次打开时按 `DetailTarget::key()` 建 tab 并登记（**一个目标一个 tab**，入口是右 Dock 清单点一行 / 定向打开）；tab 被关闭后实体释放 → 下次重新创建 |
| 打开详情的宿主命令 | `Shared.open_mock_detail`（`Rc<dyn Fn(DetailTarget, &mut Window, &mut App)>`） | 由宿主构造期装配（需要 DockArea），与 `editor_clear` 同一口径；按 key 去重，已存在则 `TabGroup::select_tab` 聚焦 |
| 列来源 / 既有表 | 面板状态（由宿主 `MockHost::schema_sources/existing_tables` 提供） | 打开面板 / 导入对话框 / 落库报「已存在」时加载；渲染只读 |
| 取消标志 | mock crate 进程级 `AtomicBool` | 生成前重置、批次边界读 |
| 生成任务 / 用户模板 | 项目 SQLite（`MockGenerationStore`） | 命令式调用（事务内） |

规则：

- **渲染期零副作用**：面板与详情 tab 渲染只读状态，不打开数据库、不解析文件；输入框是唯一的「懒创建」
  （`InputState::new` 需要 window）。
- **副作用只在事件路径**：生成、导入结构、四个出口、刷新候选、打开对话框都由按钮 / 菜单回调触发；
  落库成功后经 `MockHost::notify` 让宿主刷新依赖视图（分析库导航缓存失效）。
- **只读护栏在视图层**：`MockHost::read_only()` 为真时四个写出口直接拒绝且不触宿主（已有窗口测试断言）。
- **两处视图同源**：中央 tab 只持 `Entity<MockPanel>`，列与预览都从它读、编辑动作写回它（不存在第二份草稿：
  连表头那几个输入状态也在面板上，面板只是把这段**渲染**让给中央）。
- **状态单点**（D38）：进度 / 取消 / 失败只画一处——`job_row_scope` 给出归属（单表任务在中央那张表的表头，
  集合任务在右 Dock 场景清单下）；清单行只给 `table_status` 出的状态点（已落库 / 未落库 / 引用的表未落库 /
  生成中 / 失败，悬空引用优先于「已落库」），关系按子表归组（`outgoing_relations`）。

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
| D13 | 生成器目录**由 models.rs 穷尽派生**（脚本生成 + `spec_of` 穷尽 match） | 143 变体不可能手写对齐（v1 就是手写 1354 行且已漂移）；新增变体会编译失败强制补齐 | 手写生成器清单（漏项 / 漂移） |
| D14 | 列配置用**工作副本**（应用 / 取消） | 与其余模块的对话框语义一致；取消不污染已配置的列 | 即时写回（无法取消，误改难回退） |
| D15 | **生成不写库**（只产内存临时表 + 预览） | 生成是「试算」不是「提交」；写入必须显式点出口，避免误写与不可预览的副作用 | 生成即写分析库（v1 无此语义，v2 早期曾这么实现，已回归） |
| D16 | 目标表是**用户命名的新表**（表名输入） | v1 主路径就是「命名新表 + 组织列」；「往既有表灌数」只是其中一个出口 | 从分析库既有表里选（把 mock 降级为灌数器） |
| D17 | 追加必须**显式选表**，同名不自动追加 | 「持久化」与「追加」是两个语义不同的出口；静默追加会让用户以为新建了一张 | 表已存在时隐式走追加（v1 的 `CREATE TABLE AS SELECT` 会直接报错） |
| D18 | 字段表与预览落**中央 tab**（方案①），表名 / 行数 / 种子 / 语言也一并归它（D38 扩过） | 右 Dock 起步 280px，宽表与字段卡片放不下（**可拖拽调宽**：`DockSkin` 给每个 placement 都画了 resize 抓手，旧文写的「不可拖拽」是错的）；中央 tab 与编辑器同构（单一权威状态） | 全塞右 Dock（v1 是 380px 可拖拽宽栏） |
| D19 | 生成器切换走**分类子菜单**，参数在列编辑对话框 | 生成器身份与参数行必须一致；同一对话框内改生成器会让参数行失效（要么重建、要么错位） | 对话框内提供生成器下拉（参数行与生成器不同步） |
| D24 | 分类子菜单**保留**，另加「搜索生成器」对话框（`List` + `ListState`） | 两条路径对应两种心智：知道「属于哪类」→ 翻菜单；只记得名字 → 搜索。搜索同步过滤（143 项全在内存）无 loading 闪烁；搜索框 / 虚拟化 / 上下键 / 空态全是组件的 | 只留子菜单（143 项翻找慢）；把搜索框塞进弹出菜单（菜单只有 item，无输入控件）；自搓输入框 + 滚动列表 |
| D25 | 落库改**跨库直写**（`ATTACH` + `INSERT SELECT`），不再用 INSERT 文本 | 数据全程在 DuckDB 内部流动；大行数下没有「读全量 → 拼文本 → 再解析」两跳与对应的内存峰值 | 继续文本中转；把临时表改成文件表（与「生成是试算」语义冲突） |
| D26 | 回滚**只删本次 `Create` 刚建的表**；追加失败不回滚 | 同名表已存在时建表会失败，此时删表就是删别人的数据（本项目实际存在这个误删风险，已在实现里绕开并加测试锁住） | 「失败就 DROP 目标表」（会把既有数据删掉）；不回滚（留下半成品空表） |
| D27 | 临时表清理**以库为准 + 多前缀**（`list_by_source` / `drop_by_source`），切项目时由宿主调 | 注册表只在新建时写，会漏（旧版本建的 / 上次清理漏掉的）；而 mock 沿用 v1 的 `temp_mock_` 前缀（D2），只认 `tmp_m_` 一套就等于没清。限定 `catalog = memory` 避免误删 `ATTACH` 进来的文件库表 | 只靠注册表枚举（会漏）；只认一套前缀（mock 永远清不到）；按“所有临时表”一刀切（会碰别的来源） |
| D28 | 集合类参数用**多行文本**编辑（一行一项 / 一行「值, 权重」），解析失败保留上一个有效值 | 多行文本比键值对表格轻，且 143 变体零手工对齐（还是同一套目录派生）；值里可带逗号（分隔符取**最后一个**）。同时把「空集合 / 权重全零」提到**生成前**拦住——生成期对它们会 panic，工作线程一挂该进程后续任务全失败 | 集合编辑器表格 / 子对话框（重且要为三个参数各写一套）；只改编辑器不做生成前校验（手改配置 / 旧模板仍会 panic） |
| D29 | 表间引用**挂在列上**（`dependency`），取值域由**父表参数算出**（自增 + 行数），不读已落地数据 | 关系属于「两列之间」而不是「某张模板的附加清单」——挂列上就只有一处权威（不会出现模板清单与列定义两份）。域算得出来就 O(1)、无内存、不碰库，也才能在**生成前**就把错报出来；因此不要求父表先于子表（自关联 / 前向引用都合法） | 模板/任务里另存一份关系清单（两处状态必漂移）；建值池或读父表已有主键（越界，且十万量级要装内存）；生成后 UPDATE 子表（要改已落地的数据） |
| D31 | 自定义多表**从单表草稿加表**（而不是另做多表草稿 / 多表编辑器） | 导入结构、智能映射、列编辑、行数 / 种子这一整套都在单表态里现成可用；把它加成场景的一张表，等于复用了整条调表链路，成本只是「草稿 → `TemplateTable`」的一次转换。做成多表草稿则要把 `MockDraft` 拆成 N 份，波及面板输入、列编辑、历史 / 模板（用户模板存的是单表配置）、出口——那是大手术 | 多表草稿 / 多表编辑器（重且与现有单表链路并存会分裂状态）；从内置模板复制表（用户要的往往是自己的结构，模板里那几张只是起点） |
| D30 | 批量落库**逐张走「新建」语义**：同名的跳过并报出，**成功的不回滚** | 「全成功或全失败」听着干净，代价是回滚——而回滚会碰到别人（或上次）已经落好的表，比「留下几张已落的」危险得多。逐张落还有个好处：同名冲突能落到具体表名上，而不是一句「批量失败」 | 事务化全量（回滚会删别人的表）；先检查后落（检查与落之间仍有窗口，且多一次全库扫描） |
| D32 | 「编辑表」只改工作副本的**表名 / 行数**，改名时**自动重定向指向它的入边** | 表名与行数是场景里最常被改的两项（10 万行的财务模板要先缩下来），两者都有跨表后果：行数改小了，引用它的取值域跟着变（域由父表行数算出，无需用户同步）；改名字不重定向，指向它的关系立刻变成「指向模板里没有的表」。把这两件事一次做完，比给一个全功能表编辑器（加列 / 删列 / 改类型）划算得多——那些在单表态的列编辑里已经有了 | 只给行数不给改名（用户想换表名只能重选模板，编辑全丢）；改名不重定向（留下悬空引用，到生成前才报错）；做一个多表全功能编辑器（重，且与单表列编辑两套并存） |
| D20 | 生成 / 追加 / **三个出口**走**后台工作线程 + 进度 + 取消**（`services::mock_jobs`） | 生成是重活，UI 线程 `block_on` 会冻结界面且无法中断；`MockHost` 非 Send，不能在视图里直接 `spawn` | 视图内 `cx.background_spawn`（要求宿主 Send）；不做进度（大行数只能干等） |
| D21 | 进度用**定时泵（120ms）+ 宿主槽**，而非 render 轮询 | 任务进行中没有其他事件触发重绘，不主动唤醒就看不到进度；轮询频率低不抢主线程 | render 内轮询（永远不会被调到）；GPUI 后台执行器直接跑生成（阻塞后台池线程） |
| D22 | 任务收尾「清进度 + 写结果」在**同一把锁下一次性**完成 | UI 侧不可能观察到「既无进度也无结果」的空洞，避免误报「工作线程已退出」 | 两个独立原子量（存在观测窗口） |
| D23 | 出口类任务**不提供取消**，进度改**不定量**形态 | 写入分析库 / 写文件都在 DuckDB 与文件系统内部，探不到中断点；强中断会留下半张表或半个文件 | 假装可取消（点了没用，比没有更差）；干脆不给进度（大行数落库像卡死） |
| D33 | 「不能采样」的参数**一律在生成前拦**：`generator_param_problem` 覆盖区间类（`min >= max` / 反向）、时间区间（不足一分钟，fake 走 `(0..分钟差)`）、权重（非有限 / 负数 / 全零）；并给 worker `catch_unwind` + 连接锁毒化恢复 | 这类参数原样进 `rand` 就是 panic（空区间），而 panic 发生在**持有内存库连接锁**期间：锁被毒化 → 该进程之后每次生成都报 `poisoned lock`，再叠加「worker panic 后进度槽永远 `Running`」，用户只能重启应用。护栏放在**生成前**（引擎侧、唯一一处，公开 API 与面板都走它），而不是靠 `catch_unwind` 兜——兜住只是不让会话死，告诉用户「哪个参数不对」才有用 | 只兜 panic（用户拿到的是「内部错误」，不知道改哪个字段）；在每个入口（面板 / 历史重放 / 模板应用）各写一遍校验（必漂移）；让 `rand` 的 panic 冒到工作线程 |
| D34 | 值 → 文本**只有一处**：`engine::duckdb::value_text`（arrow 展示与 SQL 字面量共用）；不认识的类型**不许静默变 NULL** | 时间戳 / 日期 / 时间 / 十进制 / 区间在「预览」与「导出 .sql」两条路上都要渲染，各写一份必然漂移——历史上就是两边都漏（`row_to_arrow` 5 类变体、`value_to_sql_literal` 9 类），于是这些列在预览里恒为 `NULL`、在导出里也是 `NULL`。统一渲染处后，覆盖不全在编译期就看得见（`display_text` 是穷尽 match + `#[non_exhaustive]` 的 `Debug` 兜底） | 两边各写一份（漂移只是时间问题）；漏的类型继续写 `NULL`（用户看到「生成完了但数据是空的」）；把时间 / 十进制就当成字符串列（落库后 `SUM` / 时间运算用不了，且会改变已有列类型语义） |
| D35 | **一个目标一个中央 tab**（草稿 + 每张结果表），身份 = `DetailTarget::key()`；**切 tab 就是切表**；不在生成时一次性开全部 tab | 单个 tab + 「当前表」下拉会把两张表的字段与预览混在一个 tab 里（要切下拉才看得出差别，而 tab 并排对照更自然），且“我在看哪张表”多一层间接。身份用**名字**不用下标：`results` 每次生成都会重建，下标会让旧 tab 指向别的表。生成时一次开 N 个 tab 会在 4~5 张表的场景里淹掉编辑区，所以按需打开（首个进面板、其余从结果表清单点）。**旧 tab 不自动关**：重新生成 / 切项目后，上一轮的 tab 留在 Dock 里显示「这一轮没有它的结果」——关不关 tab 是用户的窗口布局，不由数据作主（结果仍在面板里，要看得重新生成） | 单 tab + 下拉（预览与「当前表」脱钩，容易看错表）；用下标做身份（重建后指向别的表）；自动开全部 tab（多表时淹界面）；为每张表存一份草稿（状态分裂，见 D31） |
| D36 | 取消由面板**持续压**（每个轮询周期重发 `cancel`），而不是只在点的那一刻发一次 | 引擎在每次生成开始时清一次取消标志（`reset_cancel`），而取消按钮从提交起就可点——提交后头几百毫秒（`AppendTo` 要先开库读目标表行数，窗口更宽）点的取消会被清掉，任务照常跑完。重发是幂等的（置一个 `AtomicBool`），直到任务真的结束；这比把取消做成任务级句柄（要改引擎公开 API 与全部调用点）便宜得多，语义也够用 | 任务级取消令牌（改 API 面）；只在点击时发一次（保留被吞的窗口）；面板侧把「正在取消」做成可撤销（用户看不出来到底取没取到） |
| D37 | 切项目清理临时表**不等锁**（`try_lock`），拿不到就留给「任务收尾」重试 | 清理要内存库连接锁，而出口任务（不可取消，D23）可能整段持锁——在 UI 线程上同步等锁就是界面假死到大行数落库 / 导出结束。拿不到锁就置一个 `pending_temp_cleanup`，在任务收尾那一拍（锁已释放）重试；一直没机会就下次切项目再试（临时表是进程内存，晚清只是多占一会儿，不影响正确性） | UI 线程同步等锁（假死）；直接跳过不重试（临时表一直不释放）；把清理丢进后台线程（还得自己找回 UI 线程通知面板） |
| D38 | **右 Dock = 管理表**（清单唯一入口 + 出口组 + 折叠的历史 / 模板），**中央 tab = 这张表的设计与生成状态**（表名 / 行数 / 种子 / 语言 + 生成 + 列 + 预览）；**状态单点**：单表任务的进度 / 取消在中央表头，集合任务（场景 N 张表）的在右 Dock 场景清单下 | 「有哪些表 · 落了没 · 关系连到谁」是集合视角，「这张表怎么造 · 造到哪一步」是单表视角——混在一栏里就会重复（进度条 / 当前表 / 生成按钮各画两遍）。清单行只给状态点与百分比（`◐ orders 40%`）就够：要停任务就点那一行切到那张 tab 再取消，**入口唯一**。散开还有几处顺带收益：出口组挨着清单（作用于当前 tab 那张表，语义清楚）；关系并进清单行（`↳ user_id → users.id · 1..1,000` 挂在**子表**行下），不再单开一段；历史 / 模板默认收起，面板的天天用部分就是那份清单 | 配置与出口都留在右 Dock（280px 里塞表名 / 行数 / 种子 / 语言 / 生成 / 进度 / 出口 / 历史，天天用的一半要滚动才够）；列表与详情各画一份进度（两处都要同步，也容易不一致）；给清单再配一个「当前表」选择器（`sel` 那一行就是当前表，多一个是多一处状态）；把集合任务的进度也塞进中央（那里看的可能是别的表，且它不属于某一张表） |
| D40 | 列依赖模型**只留跨表引用**：`ColumnDependency { ref_table, ref_column }`，删掉 v1 的 `DependencyType`（Expression / Template / Sequence / Weighted）与 `source_columns` / `expression` / `weights` 三个空字段 | 这四个变体与三个字段在 v1 与 v2 **两代都没被求值过**（全仓零构造零读取），而依赖求值的旧代码早按「本模块不解释依赖表达式」删了；模型里留着「看着能用、实际没人读」的字段，只会让下一个读代码的人（包括我自己）把它当半成品再规划一轮。删掉之后语义只剩一句话：**列上有 `dependency` 就是跨表引用**，取值域由父表参数算出 | 留着当「以后要做的占位」（`mock-dev-plan` C3 就因此挂了三个月，还让我误判成「有半成品可收尾」）；只标 `#[deprecated]` / 只删变体留字段（半吊子状态更绕）；顺手实现求值（那是**带求值器**进来的独立特性，不是 A 步） |
| D41 | 分布类生成器**自建实现，不引 `rand_distr`**；泊松 `λ ≥ 30`、二项 `n > 64` 改走**正态近似** | crate 现有依赖只有 `rand` + `fake`，而需要的分布用三个助手就能拼全：Box-Muller（标准正态）、逆变换（指数 / 帕累托）、Marsaglia-Tsang（Gamma，供 Beta 用两个 Gamma 之比）。吞吐上两边都得看：Knuth 法抽一个泊松值是 O(λ) 次循环，二项精确法是 O(n) 次伯努利——十万行 × λ=1000 会直接卡住生成，而近似分支是常数时间且量级正确（均值 λ / np、标准差 √λ / √(np(1-p))，两项都有统计冒烟测试盯着） | 引 `rand_distr`（多一个依赖，且大部分分布用不上）；全用近似（小 λ / 小 n 下取值会失真，而小参数恰恰是默认值）；泊松逐次循环到底（默认 λ=1 无感，但架不住用户把 λ 调到千级后十万行） |
| D42 | **工作日历是列级参数**（不共享、不内置节假日表）：判定顺序「上班日期（调休）→ 跳过日期（节假日）→ 工作周掩码」；顺序日期**按工作日数推进**，随机时刻**重抽 + 顺延**；覆盖**七个日期生成器**（`sequential_date` / `sequential_date_with_gaps` / `date` / `date_time` / `date_time_before` / `date_time_after` / `date_time_between`），四个带时刻的另有**可自定义的工作时段窗口**（`HH:MM`，起 > 止 = 跨零点 / 夜班） | 日历是「这一列怎么造」的一部分：落在列的生成器参数上，就能沿用现成的目录派生 / 目录表单 / JSON 补丁 / 生成前护栏四条链路，一行新机制都不用加（表级共享日历要动 `MockConfig` + 面板 + 历史与模板往返）。**不内置节假日表**是因为它必然静默过期——法定节假日与调休每年发通知，过期的表看着像真的、只是落在不该上班的那天；两个列表把这件事交回用户，且「上班日期」优先于工作周与跳过列表，刚好能表达调休上班的周六。窗口做成两个 `HH:MM` 字段而不是写死 09:00~18:00：早班 / 晚班 / 夜班都是真实形态，而跨零点只是「两段按长度加权采样」一件事。实现上：列表项按 ISO 串**字节序**比较（`YYYY-MM-DD` 的字典序就是时间序），不建 `NaiveDate`、不排序；「第 k 个工作日」用「估跨度 + 按实际工作日数纠正」收敛，不做 O(行数) 逐日扫；两个列表各限 366 条（逐行判定的生成期预算）；日历全关的默认值收在模型构造器（`GeneratorConfig::date_time` / `date` / `date_time_between`）里，智能映射与内置模板都走它们 | 表级 / 项目级共享日历（用户明确不要：多一次配置概念与一处状态，收益只在反复给同项目多表生成时）；内置节假日表（每年腐烂，且调休表达不了）；读项目库里的日历表（碰「不读已落地数据」的边界，还要先定谁维护它）；只做「跳过周末」不带列表（节假日恰好是用户点名要的自定义部分）；工作时段只给一个不弹窗口（早班 / 晚班 / 夜班都会被写死的那段矬空） |
| D39 | 落库**不做「资源注册」这一步**：出口把表落在项目分析库，那张持久表**就是**分析资源的项目级形态 | 「落库 = 本项目上一张持久表」已经是完整语义（D3 / E4 实现）；再叠一层「登记到 M6」需要新的触发时机、重复登记的版本语义、以及一个不搬本体的索引入口（`archive()` 是**文件**语义：先校验 `source_path` 是文件，再 `payload.archive_in` 把本体 **move** 进 `resources/`——照搬会把整个 `analytics.duckdb` 搬走）。而资源管理器列的本来就是 `resources/` 下的存档副本（`list_file_archives`），**不应**包含项目库里的表：项目表在本项目的分析库视图 / 导航里看，进全局是用户在资源管理器发起的 M6 存档（对表怎么存档是 M6 自己的事） | 「持久化后自动登记为分析资源」（塞满资源库，且清单里的表会与项目库的重名难辨）；「出口组加登记按钮」（多一个用户要理解的概念，而它没有增值——表已经在项目上了）；给 mock 开一条直写资源库 / 全局库的口子（破不变式 I9：输出恒为项目级） |

## 7. 降级与容错矩阵

| 场景 | 行为 | 用户可见提示 |
| --- | --- | --- |
| `row_count = 0` | `MockError::InvalidRowCount(0)` | 「无效的行数: 0」 |
| `columns` 为空 | 面板先拦：「请先添加列：导入源库结构，或手工加列」 | 直接、可操作 |
| `nullable_ratio` 越界 | `MockError::InvalidColumn(...)` | 「列 'x' 的 nullable_ratio 必须介于 0.0~1.0」 |
| 唯一列重试 >100 次 | `Generation("无法为唯一列'x'生成不重复的值")` | 建议减少行数或换生成器 |
| 追加目标表不存在 | `项目分析库没有表 {t}` | 面板 danger 行；引导先「持久化到项目分析库」 |
| 落库同名表 | `项目分析库已存在表 {t}：请改用「追加到既有表」` | 面板 danger 行 + 刷新追加候选（不覆盖不静默） |
| 追加目标缺列 | `目标表 {t} 缺少列：a、b（列结构需一致）` | 面板 danger 行（不把 DuckDB 原始错误冒到界面） |
| 落库写失败 | `写入项目分析库失败: …`（建表已回滚） | 面板 danger 行；新表已被 DROP，不留半成品 |
| 未打开项目（落库 / 追加） | `未打开项目：Mock 只写项目分析库（{项目}/.RSmeta/analytics.duckdb）——请先打开项目；要进全局分析库，用资产库存档（M6）或草稿箱（M5）升级` | 面板 info 行 + **三个项目级出口预先禁用**（`sinks_ready`；2026-09-20 起不再等点了才报）；这句仍是**任务层的兵底线**（面板已摆着而项目被关掉时）；**纯生成不受影响** |
| 未打开项目（草稿箱 / 历史） | `未打开项目：草稿箱不可用（请先打开或新建项目）` / `未打开项目：生成历史与模板随项目保存` | 面板 danger 行 |
| 任务已在跑 | `已有任务在进行中（请等它结束或先取消）` | 面板 danger 行（按钮同时被禁用，双重护栏） |
| 工作线程不可用 | `后台工作线程不可用` | 面板 danger 行；槽已回滚，不影响后续重试 |
| 任务异常结束 | `后台生成任务异常结束（工作线程已退出）` | 面板 danger 行（既无进度也无结果时才判为异常） |
| 用户取消 | 引擎在批次边界回 `生成已取消`（面板每个轮询周期重发取消，见 D36） | 面板 danger 行 + 「临时表可能残留部分行，下次生成会重建」；旧结果作废 |
| 出口进行中 | 不渲染取消按钮，`cancel_job` 直接忽略 | 只有不定量进度（写入 / 导出会跑完） |
| 出口失败（同名表 / 路径不可写） | 任务以可读错误收尾 | 面板 danger 行；**预览保留**（可改用「追加」或换路径重试） |
| 参数**不能采样**（集合留空 / 权重非正 / 区间反向或空 / 时间区间不足一分钟） | 生成前拦住（`MockError::InvalidColumn`，见 D33） | 例：「列 'x' 的随机整数区间是空的（min 10 > max 5）：把 min 改小或 max 改大」；**不会 panic**（panic 会毒化内存库锁，把整个会话弄皮） |
| 集合类参数输入非法 | 保留上一个有效值，不写回生成器 | 对话框内就地 danger 提示，带行号（如「第 2 行「x」：权重需为数字」） |
| 源表列读不到 | `读不到/没有列信息（连接可能已断开，或表不存在）` | 面板 danger 行；不做假导入 |
| 分析库打不开 | 服务层中文错误 | 面板 `error` 行（danger 色）；既有表候选为空清单 |
| 只读项目 | 视图层直接拒绝，不触服务 | 「只读模式：不允许落库与写文件（仍可生成预览）」 |
| 生成中途取消 | 批次边界返回「生成已取消」 | 面板 `error` 行 |
| Xlsx 导出 | 经 JSON 中转（DuckDB 不原生支持） | 成功文案带格式名 |

## 8. 性能与可观测

- **批量**：10k 行 / 批，批内先拼值再一次性 `INSERT`（避免逐行往返）。
- **分布抽样的单值成本**：正态 / 指数 / 帕累托 / Beta 都是常数时间；泊松在 `λ ≥ 30`、二项在 `n > 64` 时切到正态近似，
  避免「单值 O(λ) 次循环」在十万行上变成千万次迭代（默认 λ=1 / n=10 走精确分支，形状不失真，见 D41）。
- **工作日历的逐行成本**：「第 k 个工作日」约 2~3 轮收敛，每轮只做常数算术 + 两个列表各一遍线性扫（按字节比较，
  明显不在区间的项先被字节序比较跳过）；列表各限 366 条正是为了把这个预算封住（一年 20 条上下，实际远低于上限）。
- **行数上限**：面板侧 `MAX_ROWS = 1_000_000`（误输入护栏）；引擎侧不设上限，由 row_count 决定。
- **进度粒度**：按生成批次（`BATCH_SIZE = 10_000` 行/批）回调；面板每 120ms 拉一次，越接近尾声越密。
  提交到首批完成之间显示「准备中…」（总量随首批回调返回）。
- **可观测**：`MockGenerateResult.elapsed_ms`（生成耗时，不含写库）；`tracing::warn!` 用于生成器内的可恢复异常（非法日期回退、语料缺失）。
- **预览按列重排的代价**：每次点列头 = 一次 `ORDER BY col LIMIT 10`（带 `try_lock`，不等锁）。DuckDB 排 10 万行在毫秒级、行数上限 1_000_000 时也在百毫秒内；不以「保持列宽」为代价再挤一次重排（重建表头时列宽回默认档，与重新生成同一行为）。
- **预览成本**：固定前 10 行（`PREVIEW_ROWS`）；`preview()` 支持自定义 limit。

## 9. 已知问题与后续项

**本轮实证（与迁移/文档相关的三点）**

| 编号 | 问题 | 证据 | 处置 |
| --- | --- | --- | --- |
| I0a | v1 的集成测试文件 `v1/backend/tests/mock_engine_tests.rs` **无法编译**（`response.generator.type_name()`、`confidence > 0.0`、`preview.rows` 非空），从未通过 | 该文件引用的 API 在 v1/v2 的 `models.rs` 中都不存在（`confidence` 是 `String`、无 `type_name`）；`read_preview` 只填 `batches` | 本轮按 v2 真实契约重写为 26 项集成测试（`crates/mock/tests/`），并在文件头记录差异 |
| I0b | `persist_as_asset` 与 `export(Table)` 生成非法 SQL：`CREATE TABLE t AS SELECT * FROM SELECT * FROM …` | 集成测试 `persist_as_asset_creates_table_and_consumes_temp` 实测报 Parser Error；根因是 `build_create_table_as_select(table, source_table)` 第二参数是**源表名** | 已修（engine 参数正名 + 两处调用改为传表名 + 新增 `export(Table)` 回归测试） |
| I0c | v1 设计文档与实际实现有出入 | 文档称 `history.rs` / `mock_get_history` / `mock_clear_history` / `mock_re_generate` 已完成，v1 源码中并不存在；文档称生成器 106 变体，实际远不止（现 143） | 本目录文档以**代码为准**重写；v1 文档仅作素材 |
| I0d | `export(SqlInsert)` 曾**死锁**：重构后它先取内存库连接锁、再调 `insert_statements`（后者又取同一把锁） | 集成测试 `export_sql_insert_writes_insert_statements` 挂住不返回（进程被外部终止时才退） | 已修：`export` 的 SqlInsert 分支提前返回、不持锁；`insert_statements` 文档写明「调用方不得持锁」 |
| I0e | 临时表名只由目标表名派生（`temp_mock_{表名}`） | 同名目标表并发生成会互相覆盖临时表内容（单用户顺序操作为下无影响，集成测试并行必须用不同表名） | 若将来引入后台并发生成，给临时表名加会话后缀 |

**架构级待办（与边界相关）**

| 编号 | 问题 | 影响 | 建议 |
| --- | --- | --- | --- |
| I1 | ~~临时表命名前缀与 `TempTableManager` 约定不一致~~ | 已解决：管理器新增 `TempTableSource::prefixes()`（**两套命名都认**：v2 `tmp_m_` + v1 `temp_mock_`）与 `list_by_source` / `drop_by_source`（**以库里的实际表为准**，不只靠注册表），表名取自库、删完同步注册表 | —— |
| I2 | ~~临时表建在进程级内存单例，切项目不会清~~ | 已解决：`DuckDBManager::drop_in_memory_temp_tables(Mock)`（限定 `catalog = memory` + `schema = main`，`ATTACH` 进来的文件库表不会被误删）由宿主在**项目切换**时调用，并让 mock 面板作废旧预览（表名已失效） | 已足够；将来若改成项目作用域分析库，切项目天然不带过去 |
| I3 | ~~分析库写入是「INSERT 文本 → `execute_batch`」~~ | 已解决：改 **`ATTACH` 跨库直写**（`MockEngine::write_temp_table_to_database`），数据不经 Rust 字符串；回滚只删本次刚建的表（同名既有表不受影响） | —— |
| I4 | 模板/用户模板与生成任务已落 `project.db` | **已解决**：生成任务——真库往返测试 + 生成历史入口（重放 / 删除 / 自动落库）；用户模板——保存对话框 + 模板段（应用 / 删除）+ 真库往返与端到端测试 | —— |
| I5 | ~~`{model,generator,commands}.rs` 占位~~ | **已解决**（本轮）：三个文件根本没在 `lib.rs` 里声明（不参与编译），已删除；`commands.rs` 对应的 Tauri 命令层早已按 Round 14 退役 | —— |
| I6 | ~~文档称「生成器按依赖表达式计算取值」~~ | **已收口**：依赖表达式的死代码已删（见 I25）——**本模块不解释表达式**；`dependency` 的用途只剩跨表引用（D29），v1 的空壳变体与空字段也已从模型里删掉（D40）。历史背景：`resolve_dependencies`（拓扑排序）只被测试调用，`eval_expression` 是半成品（除零静默返回 0.0） | —— |
| I7 | ~~生成与落库都是**同步阻塞**调用~~ | 已解决：生成 / 追加 / 三个出口全部走后台工作线程（进度 + 取消，D20/D21/D23） | —— |
| I10 | ~~出口（持久化为分析库表 / 另存为 / 草稿箱）仍是**同步阻塞**调用~~ | 已解决：出口并入 `mock_jobs` 的任务种类（`Persist` / `Export` / `Scratchpad`），阶段上报 + 不定量进度条 | —— |
| I8 | ~~复杂参数（`ForeignKey.values` / `Sequence.values` / `Weighted.choices`）无编辑入口~~ | 已解决：列编辑对话框里的**多行文本**（一行一项 / 一行「值, 权重」）+ 生成前拦空集合与全零权重（D28） | —— |
| I11 | ~~「列依赖 / 外键」模型空转~~ | **已解决**：跨表引用现在挂在列上（`ColumnDependency::foreign_key` 是唯一构造点），生成路径真的读它——`MockEngine::resolve_reference_domains` 算域 + 校验，`generate_table` 按域采样；内置 6 套模板的 24 处引用已从「范围手写」改为声明引用（blog 顺带补了缺失的父表 `users`）。历史上它是「用范围对齐父表行数」的手写约定（5/6 套恰好落域内，blog 有 2 处悬空，v1 同款），升级后由自检测试盯住：声明可解析 / 生成器域 ⊆ 父域 / 每个 `*_id` 列要么声明要么在白名单。**仍不考虑**：跨模板 / 跨库引用，以及从已落地数据里取值（那是后续的**影子数据**，不属本轮边界） | —— |
| I12 | v1 原型的「⚙ 高级抽屉 / 时序关联（可选）」未迁 | v1 前端有 UI（`v1/frontend/extensions/builtin/workbench/ui/components/panels/MockAdvancedDrawer.vue`）、v1 后端**零实现**（`v1/backend` 全文 grep 无 timeseries / 趋势）。**已部分落地**：`GeneratorConfig::TimeSeries`（起始值 + 趋势 + 周期 + 噪声，按行序展开）＋ `sequential_date` 同一行序并排即一条时间序列；**仍未做**的是「真的看日期列的实际取值」——那属跨列求值 | 剩下的部分要做就作为「数值列跟随日期列」的**跨列求值**独立特性重新设计，而不是搬抽屉（见 `mock-prototype-design.md` §4.5） |
| I13 | **影子数据**（读已落地数据、按其特征生成相似数据） | 与当前的「生成不读数据」边界相反：本轮的表间引用**不读任何已落地数据**（域由父表参数算出）；把“读真实数据再仿制”做进来会同时碰到 I1（只进分析引擎）与「不改已落地数据」两条 | 要做就单独立项：读什么（表 / 文件）、读多少、采样出来的值怎么回写（只写内存临时表）都要先定；本轮的引用设计**不构成障碍**（引用列只是众多生成器中的一种） |
| I14 | **落地数据之间的引用完整性**：出口以「当前表」为单位，而关系是跨表的 | 只落子表不落父表，项目库里就有悬空值（而 mock 不改已落地数据，也不会替用户补）——未来 JOIN 会落空。**当前处置**（§4.6 / D30）：出口区给 warning 说明跨表后果（「引用了 x → y.z：只落这张表…」/「被 … 引用：…」）+ 列出「关系里还有 N 张没落库」+ **一键依次落库**；场景态里写明“场景生成不记入历史” | **剩下的方向未拍板**：落库时给目标表加 `FOREIGN KEY` DDL（DuckDB 要求父表先存在、插入顺序受限，与现在的自由落库 / 追加相冲）。至于「跨会话续落」（上次落的父表 + 这次生成的子表）——那要读已落地的数据，属影子数据的边界之外 |
| I15 | ~~生成期参数没有护栏 → panic → **锁毒化、会话废掉**~~ | 实测：`RandomInt{min>max}`、映射兜底 `Sentence{1,1}` 等空区间会在工作线程里 panic；panic 发生在持有内存库连接锁期间 → `Mutex` 被毒化，**该进程之后每次生成都报 `poisoned lock`**；再叠加 worker 侧「panic 后进度槽永远 `Some`」，面板卡在「已有任务在进行中」且取消按钮不再渲染——只能重启应用 | **已解决**（D33）：① 兜底改 `Sentence{1,3}`；② `generator_param_problem` 把「不能采样」的参数全拦在生成前（区间类 / 时间区间 / 权重非负且和 > 0），错误带具体数字；③ `get_conn` 遇毒化**复用连接**并 `warn`；④ `mock_jobs::worker` 用 `catch_unwind` 把 panic 降级成「这一次任务失败」。回归：单元 4 项 + 引擎集成 1 项 + workbench 2 项 |
| I16 | ~~值 → 文本覆盖不全：时间 / 十进制**静默变 NULL**~~ | 实测：`created_at` / `amount` 在预览里是 `Null`、导出的 `.sql` 里是 `NULL`（`row_to_arrow` 只认 5 类变体、`value_to_sql_literal` 只认 9 类）。两条路都属「不认识的当 NULL」，而用户看到的是「生成完了但数据是空的」 | **已解决**（D34）：新增 `engine::duckdb::value_text` 作**唯一渲染处**；arrow 转换的非原生类型走 `display_text`（容器也给文本）；mock 的 SQL 字面量补 TIMESTAMP / DATE / TIME / DECIMAL / BLOB / 大整数。回归：engine 单测 3 项 + mock 单元 1 项 + 引擎集成 1 项 |
| I17 | ~~列名净化只在插入侧 → 含空格 / 符号的列名产出非法 SQL~~ | 实测：列名 `Order Date` → `CREATE TABLE … (Order Date VARCHAR …)` → DuckDB `Parser Error`；而 INSERT 的列清单早已是 `sanitize_identifier` 的结果（注释自己写着「建表 / 追加写回必须复用同一算法」，只有生成路径没照做） | **已解决**：`build_create_table_ddl` 用净化名建表，净化后为空 / 重名报可读错误（与落地路径 `column_def_infos` 同口径）。回归：单元 2 项 |
| I18 | ~~「追加到既有表」候选清单在生产路径从不加载~~ | `refresh_sources` 只有测试调用 → 新会话里候选为空（菜单写「（项目分析库暂无表）」），而项目库里其实有表；切项目后清单仍是旧项目的表名（`forget_generated` 清了 `landed_tables` 却漏了它） | **已解决**：打开面板（`Shared::open_mock_panel`）与切项目（`clear_mock_temp_tables`）都刷一遍；`forget_generated` 把 `existing_tables` / `sources` 一并清掉（项目级清单）。回归：视图测试 1 项。**2026-09-20 补宽**：原先只盖住导航右键与编辑器按钮两条入口，**活动栏 / 命令面板**仍只改状态——从那儿进面板会看到上一次的连接清册与既有表清单（同一形态）。现把「展开 Mock 即重读」收进 `Shared::open_right_panel`（四个入口都走它），并另给连接清册变化一条廉价路径 `refresh_mock_connections`（只刷连接候选，不开分析库）；`refresh_after_open` 的次序也修了（清临时表时的连接清册还是旧项目的，清册换掉后再派生一次）。回归：窗口用例「连接清册变了只刷连接候选」 |
| I19 | ~~`NUMERIC(10,3)` / `NUMBER(10,2)` / `TIMESTAMP WITH TIME ZONE` / `DOUBLE PRECISION` 全落到 `Text`~~ | **已解决**（本轮）：`parse_data_type` 先剥参数再取首词（`VARCHAR(64)` / `INT(11) UNSIGNED` / `DOUBLE PRECISION` / `TIMESTAMP WITH TIME ZONE` 都认），金额与时间列不再静默降级为文本；回归：单元用例表驱动 15 例。**并清掉重复实现**：`import_schema`（全项目零调用）、`map_sql_type_to_column_data_type`、`infer_datatype_for_column` 三个函数与它们的类型串各写一份全部删除（D7 的唯一入口名副其实） | —— |
| I20 | ~~取消请求可能被引擎的清标志吞掉~~ | `reset_cancel` 在**生成开始时**清一次标志，而面板的取消按钮从提交起就可点——提交后头几百毫秒（`AppendTo` 要先开库读行数，窗口更宽）点的取消会被清掉，任务照常跑完；而面板侧 `cancel_requested` 已置位且不可逆，用户既没取消成也不能再点 | **已解决**（D36）：面板在每个轮询周期重发取消（`cancel` 幂等，任务一结束就不再发）；回归：窗口用例「轮询到运行中的任务要补发取消」 | —— |
| I21 | ~~跨项目收尾的结果按「当前项目」叙事~~ | 出口任务的写入路径（`JobPaths`）与临时表名都是**提交那一刻**快照的，切项目不会改写入点；旧行为却在收尾时说「已在项目分析库新建表 X」并把 `landed_tables` / 历史记在当前项目头上——用户会在新项目里找一张不存在的表 | **已解决**：面板记下提交时的项目根（`job_project`），收尾时不一致就给成败文案追加「这个任务是在上一个项目下提交的，写入的也是那个项目」并**不记历史**（历史属于项目）；回归：窗口用例「跨项目收尾的归属」 | —— |
| I22 | ~~切项目时在 UI 线程同步等内存库连接锁~~ | 出口任务不可取消（D23）且整段持有内存库连接锁，而切项目在 UI 线程上清理临时表（`clear_mock_temp_tables`）——大行数落库 / 导出期间切项目会把界面冻到那个任务结束 | **已解决**：新增非阻塞清理（`MockEngine::try_clear_temp_tables` → `DuckDBManager::try_drop_in_memory_temp_tables`，`try_lock` 失败即返回 `None`）；拿不到锁就置 `Shared.pending_temp_cleanup`，在任务收尾那一拍（`MockHost::take_job_done`，锁已释放）重试，一直没机会则下次切项目再试 | —— |
| I23 | ~~`PersistedAll`（批量落库）漏了导航失效~~ | 批量落库同样在项目分析库里新建表，却只有单张落库 / 追加会触发导航树失效，与它们口径不一致 | **已解决**：`take_job_done` 的 `matches!` 补上 `PersistedAll` | —— |
| I24 | ~~启动恢复的项目不带 `read_only`~~ | `project_ui.read_only` 只在**交互式**打开时写（`project::ui::apply_opened`），而启动路径直接装会话——另一实例占着同一个项目时两边都以为自己可写，**六个模块的只读护栅**（mock 的四个出口、资源库、草稿箱、编辑器替换…）全部不生效 | **已部分解决**：启动路径也走 `project::service::open` 取写锁，被占用则 `open_read_only` + 提示（`WorkbenchView::new`）。**仍待处理**：失败时只提示、不改会话（旧行为保留），以及 `host.rs` 里各模块是否还有别的只读判断点需要复核 | —— |
| I25 | ~~三项清理欠账~~ | ① `{model,generator,commands}.rs` 三个占位文件**根本没在 `lib.rs` 声明**（不参与编译，还让人找错文件）；② 依赖表达式的死代码（`resolve_dependencies` 仅测试调用，`resolve_dependent_value` / `eval_expression` 带 `#[allow(dead_code)]`，后者除零还静默返回 0.0）；③ `persistence::save_task` / `save_template` / `delete_template` 无事务（父行与 N 个子行分次写，中途失败会留下「有历史没列」） | **已解决**：① 删除三个文件；② 删除三个函数与 `DependencyConfig`（**本模块不解释依赖表达式**，`dependency` 的用途是跨表引用，见 D29）；③ 三个写路径改成 rusqlite 事务（`unchecked_transaction`），失败整批回滚 | —— |
| I26 | 预览只有前 `PREVIEW_ROWS` 行 → **就地排序会撒谎** | 预览表把「前 N 行」当唯一信息量（标题也这么写）。若照结果集网格的做法在 delegate 里就地重排，用户点一下列头得到的是「前 10 行里最大的几个」——看着像「这列最大的几个」，实际不是，而且这张表有 5 万行时差得最远 | **已解决**：排序走**重查**——`SqlEngine::build_select_ordered`（列名当标识符加引号）+ `MockEngine::try_preview_ordered`（**非阻塞**：`try_lock` 拿不到内存库连接锁就回落 `None`，理由同 I22）+ `MockHost::preview_ordered`；面板持有的排序带一份 `base` 取样，与当前取样不等就自动失效（重新生成 / 切项目不必逐处手写清理）；表头箭头画的是**回推的生效排序**，被拒（内存库忙 / 列不存在）时箭头会被纠回去。筛选仍不做（取样会变成未知长度的子集）。回归：引擎集成 2 项（全局前 N 行 + 忙时回落）+ 视图 3 项（重查与失效 / 表头真点击 / 快照含排序） | 就地重排（错得最隐蔽）；按需加「取全量再排」（大表不划算，且「取样」这个语义就没了） |

## 10. 测试策略

| 层 | 位置 | 数量 | 锁什么 |
| --- | --- | --- | --- |
| 单元 | `crates/mock/src/*.rs`（`#[cfg(test)]`） | 98 | 表名净化、DDL 生成（**列名净化 / 净化后为空与重名都要报可读错误**）、`generate_cell` 各变体、列名规则表、类型串解析、序列化往返、**生成前护栏**（反向区间 / 半开空区间 / 负权重 / 时间区间不足一分钟全被拦，合法参数不被误拦）、**SQL 字面量覆盖**（时间戳 / 日期 / 时间 / 十进制 / 大整数 / 二进制 / 区间）、模板自检（**4 项关系自检**：声明可解析 / 生成器域 ⊆ 父域 / `*_id` 列必须声明或进白名单 / 白名单无幽灵条目）、生成器目录自检（3：143 覆盖 / 标签与默认 / 分类往返） |
| 视图 | `crates/mock/src/mock_view/tests.rs`（GPUI headless，窗口根 `Root`） | 104（31 纯逻辑 + 73 窗口） | 解析 / 校验 / JSON 参数补丁 / 摘要文案 / **行数千分位**（`with_thousands`，六位数量级）；**场景菜单文案**（名称 + 张表 + 千分位行数，取真实内置模板）；**生成器搜索**（空查=全量 / 标签前缀优先 / 多词 AND / 大小写不敏感 / 分类名可搜 / 无命中为空）；**复杂参数**（取值集合往返 / 分隔符变体 / 行号可读错误 / 摘要项数）；**预览快照与排序**（右键取值回退 / 整行 TSV / 排序是快照的一部分（只变排序也要报变更）/ 重查+失效+两条拒绝路径 / 从组件库 `TableState` 真调 `perform_sort`（行号槽不发请求 · 数据列转成一次重查 · 回推后排序列与 `dump` 都跟上））；面板空态与候选加载（连接 + 既有分析表）；**切项目清掉项目级清单**（既有表 / 连接）；**生成不写库**（三出口调用计数为零）；行数与列校验失败不触宿主；落库新建 → 同名报错；追加按目标表重算自增；只读拦截四个出口；列增删与「改列作废旧结果」；智能默认恢复；定向导入结构；三个对话框可开（导入 / 列编辑 / 生成器搜索）；生成器搜索过滤→确认写回；约束类列的对话框可开 + 集合类参数写回 / 非法输入保留上一个有效值；详情 tab 渲染与 `focus_tab`（含进 Dock 后真正切 tab）；**一表一 tab**（每张结果表一个 tab · 身份是表名 · 切 tab 即切表 · 标题带表名与行数 · 各 tab 预览认自己那张表 · 生成本身不开 tab · **真实 Dock 里切 tab 也验**：`set_active` 是排程投递的，用例用 `run_until_parked` 把它推到位）；**后台任务**：进度镜像 / 取消 / 提交失败 / 异常结束 / 重复提交被拒；**出口后台化**：落库 / 导出 / 草稿箱的阶段与结果、完成后预览保留、出口不可取消、无生成结果时拒绝提交；**切项目作废旧结果**（草稿保留、无临时表可清时不报提示）；**场景模板**：选模板只载入工作副本（不提交任务）；**自定义多表**：草稿加表（名字 / 行数 / 列取值与草稿一致、重名拒、空列拒、加完能一起生成）、删表连带清入边（出边随表消失）；**跨表后果与批量落库**：出口提示引用了谁 / 被谁引用、待落库清单与闭包计算（已落库的跳过）、一键依次落库、部分失败保留成功的、无待落时拒绝提交/ 未知 id 可读错误 / 退出场景态 / 关系是派生视图（扫列的 `dependency`）/ **加关系写在子列上并把生成器对齐到父域** / 父列非自增时拒并给原因 / 四个位置没选全也拒 / **提交的是编辑后的工作副本**（关系随任务走）/ 一次回填多张结果（顺序即模板表序）/ 来源标注 / 切换当前表看 `gen_info` / 越界下标不改状态 / 出口落选中那张（表名与临时表都取自结果）/ 导出文件名跟当前表 / 单表生成清掉场景态 / 进度按「张表」计（`rows_done` 为 0）；**编辑表**（改行数 → 引用它的关系取值域跟着走、父列非自增时不给域 / 改名 → 入边自动重定向且照旧能生成 / 空表名 · 非法标识符 · 撞已有表名 · 非法行数都拒且**不收起对话框**、工作副本原样不动） |
| 集成（引擎） | `crates/mock/tests/mock_engine_tests.rs` | 39 | 公开 API 端到端：生成 / 预览 / **按列重排取样（全局前 N 行 + 内存库忙时非阻塞回落 `None`）** / 映射 / 取消标志 / 类型串（含带修饰的形状） / 五种导出 / 持久化 / 草稿目录 / 模板 / 场景；**取值覆盖**：时间 / 日期 / 十进制列在预览与 SQL 导出里都有值（不是 NULL）、非法参数在生成前被拦且拦完内存库仍可用；**跨库直写**：建表（含中文列名）/ 追加 / 同名建表不删既有数据 / 失败回滚 + 解挂；**集合类参数**：空集合与全零权重生成前拦住 / 填了就能生成且取值来自集合 |
| 单元（engine 侧共用渲染） | `crates/engine/src/duckdb/value_text.rs` + `row_to_arrow.rs` | 3 + 3 | `value_text`：时间戳 / 日期 / 时间 / 区间 / 二进制 / 容器 / NULL 的文本渲染；`row_to_arrow`：时间与十进制不再变 NULL、容器给文本、真 NULL 仍是 NULL |
| 集成（临时表清理） | `crates/mock/tests/temp_table_cleanup.rs` | 2 | 清掉本进程全部 mock 临时表（幂等）；同名重复生成只留一张。**独立进程**：清理是进程级动作，与并行用例互相踩 |
| 集成（装配） | `crates/workbench/tests/mock_generator.rs` | 12 | 生成不写分析库；新建 → 同名拒绝（不覆盖）；追加接续主键（`MAX(id)=100` 且无重复）；追加目标不存在 / 缺列的中文错误；**大行数一次落库（20k）**；**目标表多出的列走默认值**；CSV 导出表头；草稿箱无项目拒绝 + 有项目落 `{项目}/mock/`；连接默认库 / schema 预填；类型串映射 |
| 集成（后台任务） | `crates/workbench/tests/mock_jobs.rs` + `mock_job_cancel.rs` | 11 + 1 | 提交即返回 + 进度可读 + 结果一次性取回 + 生成不写库；并发提交被拒且结束后可恢复；追加任务回表内总行数；**出口**：`Persist` 建表回行数 / 同名表回可读错误不覆盖 / `Export` 写出 CSV（表头 + 行数）/ `Scratchpad` 无项目报错 + 有项目落 `{项目}/mock/`；**场景模板**：一键生成多张临时表（逐表带预览与列定义）/ 逐表进度 / 不写库（连 `db_path` 都不需要）/ 结果能各自落库（表名取自结果而非草稿）；**切项目清理**（宿主入口）；**取消**在批次边界中断并回可读错误（独立进程：`cancel` 是进程级标志） |
| 集成（持久化往返） | `crates/mock/tests/persistence_roundtrip.rs` | 5 | store 读写守恒：列序 / 可空列（`None` 不写成空串）/ 历史排序与 `limit` 截尾 / 级联删除 / 模板四方法；走 `ProjectDatabaseManager` 的真迁移链 |
| 集成（历史 / 模板） | `crates/mock/tests/history_roundtrip.rs` | 4 | 记录 → 列表 → 详情 → 重放；模板存 ↔ 取 ↔ 应用 ↔ 删；失败原因与 `limit` 截尾；项目根不是目录时的可读错误 |

回归价值示例：`export_table_creates_named_table_and_drops_temp` 锁 I0b 那个 v1 遗留缺陷；
`export_sql_insert_writes_insert_statements` 锁 I0d 死锁；`generate_is_reproducible_with_same_seed` 锁可复现性；
`parse_data_type_is_public_and_loose` + `map_column_resolves_parameterized_source_types` 锁 D7；
`generate_produces_preview_without_touching_sinks` 锁 D15（生成不写库）；`persist_creates_table_then_reports_existing` 锁 D17；
`read_only_blocks_sinks_but_allows_generate` 锁「只读止于视图」；`persist_job_runs_in_background_and_keeps_preview`
+ `persist_job_creates_table_and_reports_rows` 锁 D23（出口后台化 + 预览不被作废）；
`ordered_preview_returns_the_global_top_rows` 锁 I26（排序是重查而不是就地重排）、
`preview_header_click_requeries_through_the_panel` 锁表头那条链路（delegate → 面板 → 宿主 → 回推）。

## 11. 实现位置映射

| 设计决策 / 概念 | 代码位置 |
| --- | --- |
| I3 SQL 构造器纪律 | `crates/mock/src/engine.rs`（模块头注释 + 全部 DDL/DML/DQL 调用点） |
| D25/D26 跨库直写与回滚 | `crates/mock/src/engine.rs`（`write_temp_table_to_database` / `TempTableWriteMode`）+ `crates/engine/src/sql/builder.rs`（`build_attach_database` / `build_detach_database` / `build_create_table_in` / `build_drop_table_in` / `build_insert_select`） |
| D28 集合类参数编辑与前置校验 | `mock_view.rs`（`parse_complex_param` / `complex_param_text` / `split_choice` / `ParamWidget` / `commit_complex_param`）+ `crates/mock/src/engine.rs`（`generator_param_problem`，**生成前的唯一闸门**：集合 / 区间 / 时间区间 / 权重） |
| D35 一表一 tab | `mock_view.rs`（`DetailTarget` / `MockDetailView::target`·`tab_label` / `BasePanel::set_active` → `MockPanel::focus_table` / `open_detail_for`·`open_table_detail` / 结果表清单点行 / 结果表 tab 的只读列 `render_result_columns`）+ `crates/workbench/src/view.rs`（`Shared.mock_details` 按 key 去重开 tab）+ `panels/shared.rs`（句柄表与宿主命令签名） |
| D33 参数护栏与 panic 围栏 | `crates/mock/src/engine.rs`（`generator_param_problem` / `datetime_range_problem` / `date_range_problem`；`get_conn` 遇锁毒化复用连接并 `warn`）+ `crates/mock/src/schema_map.rs`（文本兜底 `Sentence{1,3}`）+ `crates/workbench/src/services/mock_jobs.rs`（`run_job_catching` = `catch_unwind` 兜住 panic → 一次任务失败） |
| D41 分布族与时序实现 | `crates/mock/src/generators.rs`（`standard_normal` / `gamma_sample` + `Poisson` / `Exponential` / `Pareto` / `Beta` / `Binomial` / `TimeSeries` 分支）+ `crates/mock/src/engine.rs`（分布参数的生成前护栏）+ `tools/gen_mock_generator_catalog.py`（标签 / 参数 / 默认值） |
| D42 列级工作日历 | `crates/mock/src/models.rs`（七个日期生成器上的日历字段 + 构造器 `date_time` / `date` / `date_time_between`）+ `crates/mock/src/generators.rs`（`WorkCalendar`：`is_work_day` / `snap_forward` / `work_days_between` / `nth_work_day`、`WorkHours`（含跨零点采样）、`CalendarArgs`、`advance_work_days`、`datetime_with_calendar` / `datetime_between_with_calendar`、`iso_key` / `key_to_date` / `parse_hour_minutes`）+ `crates/mock/src/engine.rs`（`work_calendar_problem` / `work_hours_problem` / `work_days_step_problem` / `MAX_CALENDAR_DATES`）+ `crates/mock/src/mock_view.rs`（`work_week_text`、日历参数提示与摘要过滤）+ `crates/mock/src/schema_map.rs`·`templates.rs`（构造器接默认）+ `tools/gen_mock_generator_catalog.py`（标签 / 默认值） |
| D34 值 → 文本唯一实现 | `crates/engine/src/duckdb/value_text.rs`（`display_text` / `timestamp_text` / `date_text` / `time_text` / `interval_text` / `blob_hex`）+ `row_to_arrow.rs`（非原生类型走 `display_text`）+ `crates/mock/src/engine.rs`（`value_to_sql_literal` 补时间 / 十进制 / 二进制 / 大整数） |
| 列名净化一致性 | `crates/mock/src/engine.rs`（`sanitize_identifier` + `build_create_table_ddl` 与 `safe_col_names` 同算法）+ `crates/workbench/src/services/mock_generator.rs`（`column_def_infos`，落地侧同口径） |
| I26 预览按列重排（重查） | `crates/engine/src/sql/builder.rs`（`build_select_ordered`：列名当标识符加引号）+ `sql/engine.rs`（`SqlEngine::build_select_ordered`）+ `crates/mock/src/engine.rs`（`read_select` 抽取 / `read_ordered_preview` / `try_preview_ordered`）+ `crates/mock/src/mock_view.rs`（`PreviewSort` + `MockPanel::preview_for`·`sort_preview` / `PreviewSnapshot` / `PreviewTableDelegate::{new,map_sort,set_preview,perform_sort,context_column}` / `preview_sort_menu_item`·`preview_cell_tooltip` / `ui::PREVIEW_TOOLTIP_MAX_WIDTH`）+ `crates/workbench/src/services/mock_generator.rs`（`preview_ordered`）+ `crates/workbench/src/components/mock_host.rs`（`MockHost::preview_ordered`） |
| 字段区列搜索 | `crates/mock/src/mock_view.rs`（纯函数 `column_matches` + `MockPanel::column_filter_input`·`column_filter_query`·`render_column_filter`（懒创建 + `cx.subscribe` 搜索词变化重画）+ `MockDetailView::visible_columns`（渲染与断言同一处）+ `render_fields` 的筛选空态 / `render` 表头行的计数与输入框 + `ui::COLUMN_FILTER_WIDTH`） |
| D36/D37 取消重发与不等锁清理 | `mock_view.rs`（`poll_job` 的 Running 分支重发 `cancel_job`；`job_project` 记提交时的项目并在 `finish_job` 里做归属告知与历史抉择）+ `mock_view.rs::MockPanel` / `crates/workbench/src/panels/shared.rs`（`pending_temp_cleanup`）+ `crates/mock/src/engine.rs`（`try_clear_temp_tables`）+ `crates/engine/src/duckdb/manager.rs`（`try_drop_in_memory_temp_tables`）+ `crates/workbench/src/components/{project_host,mock_host}.rs`（切项目不等锁 / 收尾重试）+ `crates/workbench/src/view.rs`（启动取锁 → `read_only`） |
| D27 临时表清理 | `crates/engine/src/duckdb/temp_table.rs`（`TempTableSource::prefixes` / `list_by_source` / `drop_by_source`）+ `manager.rs`（`in_memory_temp_tables` / `drop_in_memory_temp_tables`）+ `crates/mock/src/engine.rs`（`clear_temp_tables` / `temp_tables`）+ `crates/workbench/src/components/project_host.rs`（切项目时清理 + 面板作废 + **重读候选清单**）+ `mock_view.rs`（`forget_generated` 清结果与项目级清单） |
| D1/D2 内存临时表与命名 | `crates/mock/src/engine.rs`（`TEMP_MOCK_PREFIX` / `get_db` / `sanitize_table_name`） |
| 生成批次与取消 | `crates/mock/src/engine.rs`（`BATCH_SIZE` / `CANCEL_FLAG` / `generate_with_progress`） |
| 生成器实现（143 变体） | `crates/mock/src/generators.rs`（`generate_cell`；分布类另有三个助手：`standard_normal`（Box-Muller）/ `gamma_sample`（Marsaglia-Tsang）/ 逆变换内联在各分支） |
| D7 类型串唯一入口 | `crates/mock/src/schema_map.rs`（`parse_data_type`）+ `lib.rs` re-export |
| 列名规则与置信度 | `crates/mock/src/schema_map.rs`（`ColumnMapper::exact_rules/suffix/fuzzy/fallback_by_type`） |
| 模板与场景生成 | `crates/mock/src/templates.rs` + `engine.rs::generate_scenario` + **装配层 `mock_generator::generate_scenario_at`**（逐表补预览；装配层入口） |
| 任务/模板持久化 | `crates/mock/src/persistence.rs` + `crates/engine/migrations/project_meta/009_mock_generation.sql` |
| 生成历史（读 / 重放 / 删除） | `crates/mock/src/history.rs`（后台线程 + 自备 tokio 运行时；宿主经 `MockHost::project_root` 只给项目根） |
| 用户模板（存 / 应用 / 删） | 同上（`template_of_draft` / `draft_of_template`；与历史共用两份「列」表公共映射） |
| D4/D5/D6/D17 装配与追加语义 | `crates/workbench/src/services/mock_generator.rs`（`generate_at_with_progress` / `generate_scenario_at` / `persist_table_at` / `append_table_at` / `export_file` / `save_scratchpad` / `import_columns`；**出口的输入是 `MockGenInfo`，不收草稿**） |
| D20/D21/D22/D23 后台任务 | `crates/workbench/src/services/mock_jobs.rs`（工作线程 + 槽 + `JobPaths` + `start`/`state`/`take_done`/`cancel`）+ `mock_view.rs`（`MockJobKind` / `MockJobPhase` / `MockJobWatch` + 定时泵 + `poll_job`） |
| D11/D18/D38 视图归属与两处排版 | `crates/mock/src/mock_view.rs`（`MockPanel` 右 Dock / `MockDetailView` 中央 tab / `MockDraft` / `MockHost`） |
| 预览表（2026-09-20 改走组件表） | `mock_view.rs`（`PreviewTableDelegate` + `TableDelegate`；`ensure_preview_table` 懒创建 / `sync_preview_table` 在面板通知那一拍推取样）+ `crates/mock/src/ui.rs`（`PREVIEW_ROW_NUMBER_WIDTH`，与结果集同档）+ 组件库 `gpui_kit::component::table`（与 `editor/view/results/grid.rs` 同一套） |
| D12/D14/D19/D24 对话框与工作副本 | `mock_view.rs`（`open_import_dialog` / `open_column_dialog` / `open_generator_search` / `ColumnDraft` / `generator_menu` / `search_generators` / `GeneratorSearchDelegate`） |
| D28 集合类参数编辑与前置校验 | `mock_view.rs`（`parse_complex_param` / `complex_param_text` / `split_choice` / `ParamWidget` / `commit_complex_param`）+ `crates/mock/src/engine.rs`（`generator_param_problem`，生成前校验） |
| D29 表间引用 | `crates/mock/src/models.rs`（`ColumnDependency::{foreign_key, is_foreign_key}` = 唯一构造点；`ReferenceDomain` 域与文案）+ `crates/mock/src/engine.rs`（`resolve_reference_domains` 校验＋算域；`generate_table` 按域采样）+ `crates/mock/src/templates.rs`（`col_ref!` + 24 处声明 + 4 项关系自检）+ `mock_view.rs`（`load_scenario` / `add_relation` / `remove_relation` / `scenario_relations` / `open_relation_dialog`） |
| D30 批量落库 | `mock_view.rs`（`pending_relation_tables` / `persist_related` / `MockJobKind::PersistAll`）+ `crates/workbench/src/services/mock_jobs.rs`（逐张落 + 按「张表」报进度） |
| D31 自定义多表 | `mock_view.rs`（`add_draft_to_scenario` / `remove_scenario_table`；工作副本表清单的「删除」与「＋ 加表（当前草稿）」） |
| D32 编辑表 | `mock_view.rs`（`open_table_dialog` / `apply_table_edit` 改工作副本并重定向入边；`relation_range` 由父列自增参数 + 父表行数算域文案） |
| D13 生成器目录 | `tools/gen_mock_generator_catalog.py` → `crates/mock/src/generator_catalog.rs` |
| 宿主桥 | `crates/workbench/src/components/mock_host.rs`（`MockHost` 实现）+ `crates/workbench/src/panels/`（面板构造期创建与句柄登记）+ `crates/workbench/src/view.rs`（详情 tab 宿主命令：按目标 key 建 tab / 已存在则聚焦） |
| 四个入口的候选刷新（2026-09-20 补齐） | `crates/workbench/src/panels/shared.rs`（`open_right_panel` 展开 Mock 时自带 `refresh_mock_panel`；`refresh_mock_connections` 给连接变化用）+ `crates/workbench/src/view.rs`（活动栏 / 命令面板改走 `open_right_panel`）+ `components/nav_host.rs`（`reload_connections`）+ `components/connection_dialog/render.rs`（保存连接后）+ `components/project_host.rs`（连接清册换掉后）+ `crates/mock/src/mock_view.rs`（`refresh_connection_sources` 只刷连接候选，不碰要开分析库的既有表清单） |
| 视图尺寸与 UI 契约（2026-09-20） | `crates/mock/src/ui.rs`（结构尺寸单一来源；`ROW_HEIGHT` 重导自外壳）+ `workbench_shell::tree::active_bar`（选中标识条）+ `crates/workbench/tests/ui_contract.rs`（两份清单 + 2c 点名表纳入 `mock/mock_view.rs`） |
| 出口可用性与快捷键（2026-09-20） | `mock_view.rs`（`sinks_ready` = 有结果 + 非只读 + 已打开项目，三个项目级出口的唯一判据与禁用理由；`render_actions` 的两行状态提示 + 底部一句边界）+ `crates/mock/src/commands.rs`（`GenerateMock`）+ `mock_view.rs`（`MockDetailView::on_generate_key` + `key_context("mock-detail")` / `track_focus`）+ `crates/app/src/main.rs`（`Ctrl+Enter` 键位）+ `crates/workbench/src/commands.rs`（重导） |
| 遗留项收口（2026-09-20 续） | 预览右键复制：`mock_view.rs`（`PreviewTableDelegate::{context_menu, context_text, row_text}` + `render_td` 里的右键位置记录）+ `gpui_kit::ClipboardItem`；旧 tab 的「为什么没结果」：`mock_view.rs`（`project_switched` / `results_dropped_by_project_switch` + `render` 的两处文案）；状态栏指示：`mock_view.rs`（`status_chip_text`）+ `crates/workbench/src/view.rs`（`mock_status_chip` 读 `Shared::mock_panel` 的 `job_progress` / `job_kind` / `cancel_requested`，不新增 `Shared` 字段） |
