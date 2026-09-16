# rds-mock — M7 测试数据生成

> 本文件是 crate 的 **README 级入口**：只提炼模块特点与代码结构，完整设计以 `docs/architecture/mock/` 为准
> （架构约定：crate 内不复制设计文档）。

## 一句话定位

按列定义生成**可信的测试数据**：元数据驱动（列名 + 类型 + 可空/主键）、确定性可复现（`seed`）、
**只落 DuckDB 分析引擎**（内存临时表 / 分析库），不回传任何源数据库。

## 模块特点

### 1. 元数据驱动，不需要真实数据样本

输入是 `MockConfig`：`table_name` / `row_count` / `seed` / `locale` / `columns`。
每列只需要四件事（列名、类型、可空率、是否唯一），生成器可以从列名与类型**自动推断**：

```
列名 + 类型串 ─► mock::parse_data_type（唯一类型入口）─► ColumnMapper::infer
    精确名 → 前后缀 → 模糊子串 → 类型兜底（≈91 条规则，137 个生成器变体）
  ─► ColumnMappingResponse{ generator, confidence: "high"|"low", sample_value }
```

未知类型回退 `Text`（DDL 同为 `VARCHAR`），配 Sentence 生成器——永远「有话可说」，不阻断用户。

### 2. 生成在后台线程（进度 + 取消）

生成与「追加」都提交给 `services::mock_jobs` 的**工作线程**执行（`MockHost::start_job`），
面板只做三件事：读进度（`job_state`）、取一次性结果（`take_job_done`）、请求取消（`cancel_job`）；
进度由 120ms 定时泵拉取（任务进行中没有其他事件会触发重绘）。取消置引擎的进程级标志，
引擎在**批次边界**（10k 行/批）响应，回传可读的「生成已取消」。

为何不在视图里直接 `spawn`：`MockHost` 是 `Rc<dyn>`（持 `Shared`，非 Send），无法搬进 GPUI
的后台执行器；因此沿用 `nav_jobs` / `scratchpad_jobs` 的成熟模式——宿主内部起工作线程。

### 3. 生成≠写入，只进分析引擎（M7 硬约束）

- **生成**（`MockEngine::generate`）只落 `temp_mock_{表名}` **内存临时表**（engine 的进程级内存 DuckDB），
  并注册到 engine 的临时表管理器；预览随结果返回；
- **写入**只能由显式出口触发：落盘文件（CSV·Parquet·Xlsx·SQL INSERT）/ 分析库新建表 / 追加到既有表；
- crate 内**没有**任何写源数据库的代码路径；落库只写「分析引擎 / 项目文件」，且跨库直写（`write_temp_table_to_database`：`ATTACH` 目标库 → `INSERT SELECT`）；`insert_statements` 只产 SQL 文本，供导出脚本用。

### 4. SQL 全量走构造器

DDL / DML / DQL 一律由 `engine::sql::SqlEngine` 构造（`build_create_table` / `build_insert` / `build_select*`），
消除字符串拼接与转义缺陷面；唯一例外是 DuckDB 专有的 `COPY`（导出用），已在代码中注明理由。

### 5. 确定性可复现

`seed: Some(s)` 时同配置逐值可复现（`StdRng`）；`seed: None` 走随机。
集成测试 `generate_is_reproducible_with_same_seed` 锁定该契约。

### 6. 场景模板与列依赖

- 内置 6 套多表模板（电商 / HR / 博客 / 金融 / 社交 / 企业通讯录），`generate_scenario` 逐表生成并按**表**回调进度；
- 列依赖 `resolve_dependencies` 用 Kahn 拓扑排序给出**生成顺序**与依赖映射；
  注意：生成器本身按列顺序取值，**不解释依赖表达式**（属待办，见架构文档 §9-I6）。

### 7. 视图随 crate（Feature 自持视图，方案①两处排版）

Mock 的**两处**视图都在本 crate（`mock_view.rs`）：

- `MockPanel`：右 Dock 280px——目标表名 / 行数·种子·语言 / 列来源 / 生成 / 出口按钮组 / 结果；
- `MockDetailView`：中央「Mock 数据」tab——字段卡片（生成器分类子菜单 + 编辑 / 智能 / 删除）+ 预览表格；
- 详情 tab 只持 `Entity<MockPanel>`，字段与预览都从它读、编辑动作写回它（**状态单一权威**）；
- crate 依赖 `gpui-kit`（UI 基础设施），**不依赖 workbench**；
- 宿主能力（生成 / 四个出口 / 列来源 / 既有表 / 只读 / 打开详情 / 重绘）由 `MockHost` 注入，
  workbench 侧实现见 `crates/workbench/src/components/mock_host.rs`；
- 137 个生成器的分类 / 中文标签 / 参数规格由 `generator_catalog.rs` **穷尽派生**（脚本生成，见下）。

### 8. 生成任务与用户模板的持久化

`MockGenerationStore` 把「生成任务 / 列配置 / 用户模板 / 模板列」写入 `{项目}/.RSmeta/project.db`
（迁移 `engine/migrations/project_meta/009_mock_generation.sql`，4 表 + 3 索引），方法 8 个：
`save_task` / `get_history` / `get_detail` / `delete_task` / `save_template` / `get_templates` / `get_template_detail` / `delete_template`。

## 代码结构

| 文件 | 职责 |
| --- | --- |
| `src/lib.rs` | crate 入口与 re-export（含依赖方向声明） |
| `src/models.rs` | 域模型：`MockConfig` / `ColumnDef` / `ColumnDataType`(13) / `GeneratorConfig`(137) / `Locale`(13) / 导出与持久化模型 / 依赖模型 |
| `src/engine.rs` | `MockEngine`：生成（分批 10k 行 + 进度回调 + 取消 + **集合类参数生成前校验**）/ 预览 / 5 种导出 / **`write_temp_table_to_database`（跨库直写落库：ATTACH → INSERT SELECT）** / **`clear_temp_tables`·`temp_tables`（临时表生命周期）** / `insert_statements`（INSERT 文本，仅导出脚本用）/ 草稿目录 / 持久化为资产 / 列映射 / 模板 / 场景生成 / `sanitize_identifier` |
| `src/generators.rs` | `generate_cell`：137 变体 → 值（`fake` crate，接入 `StdRng`） |
| `src/generator_catalog.rs` | 生成器目录（分类 / 中文标签 / 参数规格 / 默认构造）；由 `tools/gen_mock_generator_catalog.py` 生成，**不手改** |
| `src/schema_map.rs` | `ColumnMapper`（列名规则表 + 置信度 + 示例值）+ `parse_data_type`（类型串唯一入口） |
| `src/mock_view.rs` | **视图**：`MockPanel`（右 Dock）/ `MockDetailView`（中央 tab）/ `MockHost` 契约 / 导入结构 + 列编辑 + 生成器搜索对话框 |
| `src/mock_view/tests.rs` | 视图测试（12 纯逻辑 + 16 项 GPUI headless 窗口测试；含测试宿主桥） |
| `src/templates.rs` | 内置 6 套场景模板 |
| `src/persistence.rs` | `MockGenerationStore`（SQLite 读写） |
| `src/error.rs` | `MockError` / `MockResult`（含 DuckDB / 锁错误桥接） |
| `src/{commands,model,generator}.rs` | 占位（全项目统一脚手架；命令层按 Round 14 退役） |
| `tests/mock_engine_tests.rs` | 公开 API 端到端集成测试（26 项） |

宿主侧（workbench）：

| 文件 | 职责 |
| --- | --- |
| `src/services/mock_jobs.rs` | **后台任务**：工作线程 + 进度槽（含阶段）+ 一次性结果 + 取消（生成 / 追加 / 三个出口）；出口路径在提交前由 UI 线程解析 |

依赖方向：`mock → engine → shared`；视图另依赖 `gpui-kit`（不得依赖 workbench / database）。

## 能力状态

| 已实现 | 待补 |
| --- | --- |
| 生成（分批 + 唯一列 + 空值率 + 进度回调 + 取消）、预览（Arrow 前 10 行） | —— |
| **后台任务**：五种任务（生成 / 追加 / 落库 / 导出 / 草稿箱）同走工作线程；生成类有批次进度 + 取消，出口类报阶段 + 不定量进度 | 出口不可取消（DuckDB / 文件系统内无中断点，见架构 D23） |
| 4 种导出（CSV / Parquet / Xlsx / SQL INSERT）+ `insert_statements` 文本 | 导出大行数时的流式写出（现在是全量文本） |
| **落库跨库直写**（`ATTACH` + `INSERT SELECT`，数据不经 Rust 字符串） | 写入期间内存库连接持有目标文件锁（导出类任务不可取消，见架构 D23） |
| **临时表清理**（切项目时按前缀清掉本进程的 mock 临时表，两套命名都认） | 同目标表名重复生成会重建，**换名字**才会多占一份内存 |
| 列映射（≈91 条规则 + 类型兜底 + 置信度三态） | —— |
| 生成器目录（137 变体分类 / 标签 / 参数规格，穷尽派生） | 生成器的「推荐」标记与最近使用 |
| **生成器搜索**：分类子菜单 + 搜索对话框（中文标签 / 名称 / 分类，多词 AND，`List` 自带搜索框与空态） | 生成器的「推荐」标记与最近使用 |
| **集合类参数可编辑**：外键取值 / 序列取值 / 加权选项用多行文本填（一行一项 / 一行「值, 权重」） | 集合的导入 / 粘贴（从 CSV 列拷值） |
| **面板：表名 / 行数·种子·语言 / 列来源 / 生成 / 出口按钮组 / 结果** | 生成器的「推荐」标记与最近使用 |
| **详情 tab：字段卡片（生成器分类子菜单 / 搜索对话框 + 编辑 / 智能 / 删除）+ 预览表格** | 预览列宽自适应与列头排序（目前固定 9rem） |
| **四个显式出口：新建表 / 追加（自增接续）/ 草稿箱 `{项目}/mock/` / 另存为** | 项目作用域分析库（装配层已留 `*_at` 路径入口） |
| **列编辑对话框（列名 / 类型 / 参数 / 空值率 / 唯一 / 恢复智能默认）** | 列依赖编辑（依赖表达式待拍板） |
| **导入源库结构（连接 / 库 / schema / 表，cache-aside 取列）** | 表结构浏览选择器（现在是手填表名 + 连接默认库预填） |
| 6 套场景模板 + 场景生成（逐表进度） | 模板选择弹窗、用户模板保存/复用（Phase C） |
| 草稿目录落盘（调用方给目录） | 草稿箱面板对 `mock/` 分组的展示（Phase D） |
| 持久化为正式表（`persist_as_asset` / 装配层新建与追加） | 分析资源注册（M6）、生成历史面板（Phase D） |
| 生成任务 / 用户模板的 SQLite 读写 | 真实 SQLite 往返测试（当前只有序列化测试） |
| 公开 API 集成 26 项 + 视图测试 34 项 + 装配测试 10 项 + 后台任务 4 项 | 并发生成（临时表名会与同名目标表冲突，见架构 §9-I0e） |

## 设计与验证

- 设计（权威）：`docs/architecture/mock/README.md`（入口）、`mock-architecture.md`（理念与数据流）、
  `mock-prototype-design.md`（两处排版与对话框）、`mock-dev-plan.md`（阶段任务）、`mock-prototype.html`（交互稿）。
- 装配与宿主桥：`crates/workbench/src/services/mock_generator.rs`（生成 / 落库 / 追加 / 导出 / 结构导入）、
  `crates/workbench/src/services/mock_jobs.rs`（后台任务：进度 + 取消）、
  `crates/workbench/src/components/mock_host.rs`（`MockHost` 的宿主实现）、
  `crates/workbench/src/panels.rs`（面板构造期创建 + 句柄登记）、`crates/workbench/src/view.rs`（详情 tab 加入中央 tab 组）。
- 验证：`cargo check -p rds-mock --all-targets -j 2`；`cargo test -p rds-mock -j 2`（99 单元（12 纯逻辑 + 22 窗口 + 65 其他）+ 26 集成）；
  `cargo test -p rds-workbench --test mock_generator --test mock_jobs --test mock_job_cancel -j 2`（装配 10 + 后台任务 3 + 取消 1）。
- **命令约定**：全量编译/测试必须限制并发（`cargo check-all` / `cargo test-all` 别名，含 `-j 2` 与 `RUST_MIN_STACK`）。
- 生成器目录改动流程：改 `models.rs` 的 `GeneratorConfig` → 跑 `python tools/gen_mock_generator_catalog.py`
  → `rustfmt` 生成文件 → 补 `LABELS` / 默认值字典。
