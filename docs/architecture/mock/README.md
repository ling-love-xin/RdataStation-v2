# Mock 数据生成（M7）— 模块入口

> 本文件是 mock 模块的**入口**：先读这个，再按需进入同目录的原型 / 架构 / 开发方案。
> 约定遵循 `docs/architecture/README.md` 的「模块文档集约定」。

## 一句话定位

**把「表结构」变成「可用的测试数据」**——用户命名一张**新表** + 组织列定义（导入源库结构或手工加列），
在 DuckDB 分析引擎生成模拟数据，再经**显式出口**落地（落库 / 追加 / 草稿箱 / 另存为）；
**数据只进分析引擎，绝不回传源库**（M7 硬约束）。

## 特点速览

| 特性 | 含义 |
| --- | --- |
| 生成与写入分离 | 「生成」只产内存临时表 `temp_mock_*` + 预览；落库 / 落盘只能由出口按钮触发 |
| 元数据驱动 | 输入只有「列名 + 类型 + 可空/主键」三件事，不需要真实数据样本；未知类型一律退到可读默认值 |
| 只进不出 | 目标只有分析引擎（内存临时表 / `analytics.duckdb`）与项目文件；没有任何写入源库的代码路径 |
| 落库一次直写 | 落库/追加走 `ATTACH` 跨库直写（`INSERT ... SELECT`），数据不经 Rust 字符串；建表失败只回滚本次刚建的表 |
| 临时表随项目收敛 | 切项目时宿主清掉本进程的 mock 临时表（按前缀、以库为准），并作废面板里的旧预览 |
| 确定性可复现 | `seed` 固定即同序列（`StdRng`），同配置两次生成结果逐值相同（已测） |
| 列名智能映射 | 四级优先：精确名 → 前后缀 → 模糊子串 → 类型兜底；置信度写回 `high` / `low` / `manual` |
| 后台任务（生成 / 出口） | 五种任务都在工作线程上跑：生成 / 追加 / 落库 / 导出 / 草稿箱；面板显进度条与阶段文案（生成可取消，出口为不定量进度） |
| 四个显式出口 | 持久化为分析库表（新建，同名报错）/ 追加到既有表（显式选表，主键自增接续）/ 保存到草稿箱 `{项目}/mock/` / 另存为（CSV·Parquet·Xlsx·SQL INSERT） |
| 两处排版（方案①） | 右 Dock 280px = 配置 + 出口；中央「Mock 数据」tab = 字段表 + 预览表（同源：详情持面板实体） |
| 视图随 crate | 面板 + 详情 tab + 三个语义对话框（导入结构 / 列编辑 / 生成器搜索）同 crate；宿主能力经 `MockHost` 注入 |
| 生成器目录穷尽派生 | 137 变体的分类 / 标签 / 参数规格由脚本从 `models.rs` 派生，新增变体编译失败强制补齐 |
| 生成器两条找法 | 分类子菜单（知道属于哪类）+ **搜索对话框**（只记得名字：按中文标签 / 名称 / 分类过滤，多词 AND） |
| 集合类参数可编辑 | `ForeignKey.values` / `Sequence.values` / `Weighted.choices` 在列编辑对话框里用多行文本填（一行一项 / 一行「值, 权重」）；留空或全零权重在生成前拦住，不会 panic |
| 场景模板与列依赖 | 内置 6 套多表场景模板；列间依赖用 Kahn 拓扑排序定序（生成顺序，不是取值计算） |
| SQL 全量走构造器 | DDL/DML/DQL 一律 `engine::sql::SqlEngine` 生成，`format!` 仅保留给 DuckDB 专有 `COPY` |

## 边界

### 依赖方向（硬约束）

```
mock ──► engine ──► shared          （crate 依赖，见 crates/mock/Cargo.toml + lib.rs）
mock ──► gpui-kit                   （视图基础设施：面板 / 详情 tab / 对话框随 crate）
workbench ──► mock                  （宿主：实现 MockHost + 持面板与详情句柄）
```

- **允许**：用 engine 的内存 DuckDB（`DuckDBManager`）、SQL 构造器（`SqlEngine`）、元数据缓存
  （`metadata_cache`）、项目 SQLite 池（`project_db`）；用 shared 的 `error` / `models`；用 gpui-kit 写视图。
- **禁止**：mock 依赖 workbench / database（宿主能力必须经 traits 注入）；mock 自己拼 DDL/DML 字符串。

### 与相邻模块的关系

| 模块 | 关系 | 边界要点 |
| --- | --- | --- |
| M4 数据库导航 | 上游（输入结构） | 只消费「列名 + 类型」；结构由宿主编排读取（L2 缓存 `NavCache` → 实时内省 `MetadataService`） |
| M2 engine | 下游（执行与生命周期） | 临时表建在 engine 的进程级内存库；只 `register_temp_table` 注册，TTL/清理归 engine（见架构 §9-I1） |
| M1 project | 持久化载体 | 生成任务与用户模板落 `{项目}/.RSmeta/project.db`（迁移 `009_mock_generation.sql`）；产物落 `{项目}/mock/` |
| M5 草稿箱 / M6 资源 | 出口 | 草稿箱目录按项目根拼 `{项目}/mock/`（草稿箱文档已预留该模块目录）；`persist_as_asset` 供资源注册 |
| M8 洞察 | 无依赖 | 各自独立使用分析引擎临时表（`temp_mock_` / `temp_insight_`） |
| workbench（M5 视图层） | **唯一装配方** | `services/mock_generator.rs` 决定「从哪读结构、往哪写」；`components/mock_host.rs` 实现 `MockHost` |

## 代码地图

| 文件 | 职责 |
| --- | --- |
| `crates/mock/src/lib.rs` | crate 入口与 re-export；写明依赖方向 |
| `crates/mock/src/models.rs` | 域模型：`MockConfig` / `ColumnDef` / `ColumnDataType`（13 类）/ `GeneratorConfig`（137 变体）/ `Locale` / 导出与持久化模型 / 依赖模型 |
| `crates/mock/src/engine.rs` | `MockEngine`：生成（分批 10k 行）/ 预览 / 导出 / **`insert_statements`**（INSERT 文本，导出与落库共用）/ 持久化为资产 / 列映射 / 模板 / 场景生成 / 取消 / `sanitize_identifier`（列名规范化唯一入口） |
| `crates/mock/src/generators.rs` | `generate_cell`：137 变体 → 值（fake crate，确定性接入 `StdRng`） |
| `crates/mock/src/generator_catalog.rs` | 生成器目录（分类 / 中文标签 / 参数规格 / 默认构造）；由 `tools/gen_mock_generator_catalog.py` 生成，**不手改** |
| `crates/mock/src/schema_map.rs` | `ColumnMapper`（列名规则表）+ **`parse_data_type`（类型串唯一入口）** |
| `crates/mock/src/mock_view.rs` | **视图**：`MockPanel`（右 Dock 配置 + 出口 + 进度与取消）/ `MockDetailView`（中央字段 + 预览 + 列编辑对话框）/ `MockHost` 契约 / `MockJobKind`·`MockJobPhase` / `search_generators` + 搜索对话框 / **集合类参数的多行编辑** |
| `crates/mock/src/mock_view/tests.rs` | 视图测试（21 纯逻辑 + 31 项 headless 窗口测试 + 测试宿主桥） |
| `crates/mock/src/templates.rs` | 内置 6 套场景模板（电商 / HR / 博客 / 金融 / 社交 / 企业通讯录） |
| `crates/mock/src/persistence.rs` | `MockGenerationStore`：任务历史与用户模板的 SQLite 读写（8 个方法） |
| `crates/mock/src/error.rs` | `MockError` / `MockResult`（含 DuckDB 错误桥接） |
| `crates/mock/src/{commands,model,generator}.rs` | **占位**（全项目统一脚手架；命令层按 Round 14 政策退役） |
| `crates/mock/tests/mock_engine_tests.rs` | 公开 API 端到端集成测试（32 项） |
| `crates/mock/tests/temp_table_cleanup.rs` | 临时表清理集成测试（2 项；独立进程：清理是进程级动作） |
| `crates/workbench/src/components/mock_host.rs` | **宿主桥**：`MockHost` 实现（后台任务转发 + 路径解析 + 连接清单 + 只读 + 打开详情 + 重绘 + 写入成功后导航缓存失效） |
| `crates/workbench/src/services/mock_jobs.rs` | **后台任务**：单一工作线程 + 进度槽（含阶段）+ 结果一次性取回 + 取消；任务种类＝生成 / 追加 / 三个出口 |
| `crates/workbench/src/services/mock_generator.rs` | **装配层**：生成（不写库）/ 落库新建 / 追加 / 导出 / 草稿箱 / 结构导入（cache-aside 取列） |
| `crates/workbench/src/panels/` | 右 Dock 面板构造期创建 + 句柄登记；`Shared::open_mock_panel`（入口统一） |
| `crates/workbench/src/view.rs` | 中央「Mock 数据」tab 的宿主命令（`Shared::open_mock_detail`） |

## 改前必守约束

1. **不拼 SQL**：DDL/DML/DQL 必须经 `engine::sql::SqlEngine`（`COPY` 是唯一例外，且注明理由）。
2. **不做「往既有表灌数」默认路径**：目标表由用户命名（新表）；「追加」是**显式**出口，同名不自动追加。
3. **生成不写库**：`generate` 只写 `temp_mock_{safe_name}` 内存临时表；分析与项目数据的写入只能由出口触发。
4. **列名规范化只走 `sanitize_identifier`**：临时表列名与本层建表列名必须同一算法，否则 INSERT 列清单对不上。
5. **类型串只走 `mock::parse_data_type`**：源库类型串与内部规范名共用同一入口，禁止各处再写一份。
6. **`seed` 语义**：`Some(s)` 用于可复现，`None` 走随机；同 seed 同配置必须逐值可复现（回归测试已锁）。
7. **只读项目禁写**：`MockHost::read_only()` 为真时四个写出口都要拒绝（视图层拦截 + 不触宿主）。
8. **不持锁调用 `insert_statements`**：内存库是 `Mutex<Connection>`，重入加锁会死锁（架构 §9-I0d）。
9. **临时表注册**：新建 `temp_mock_*` 后调用 `DuckDBManager::register_temp_table`；前缀命名不得随意变更（架构 §9-I1）。
10. **渲染期零 I/O**：面板与详情 tab 渲染只读状态；列来源 / 既有表 / 导入结构 / 生成 / 出口全在事件路径。

## 测试与验证命令

```bash
# 全量编译/测试必须限并发（DuckDB 静态库链接耗内存），见 .cargo/config.toml 别名
cargo check -p rds-mock --all-targets -j 2
cargo test  -p rds-mock -j 2                                   # 117 单元（含 52 视图）+ 32 + 2 集成
cargo test  -p rds-workbench --test mock_generator -j 2         # 装配层 10 项
cargo test  -p rds-workbench --test mock_jobs -j 2              # 后台任务 7 项
cargo test  -p rds-workbench --test mock_job_cancel -j 2        # 取消 1 项（独立进程）
```

实测基线（本轮）：

| 目标 | 结果 |
| --- | --- |
| `cargo check -p rds-mock --all-targets` | 通过（零告警） |
| `cargo test -p rds-mock` | 117 单元（21 纯逻辑 + 31 窗口 + 65 其他）+ 32 引擎集成 + 2 清理集成全过 |
| `cargo check -p rds-workbench --all-targets` | 通过（零告警） |
| `cargo test -p rds-workbench` | 全绿（含 12 装配 + 9 任务测试） |

> 存量欠债（非本模块）：`crates/engine/tests/transaction_affinity.rs` 调用了不存在的 `Value::as_i64()`
> （实际是 `as_int()`），使 `cargo check --workspace --all-targets` 在该 target 报错。

## 文档地图

| 文档 | 内容 |
| --- | --- |
| `mock-prototype-design.md` | 长什么样：落位与尺寸 / **方案①两处排版** / 对话框 / 状态矩阵 / 与 v1 逐项对照 |
| `mock-prototype.html` | 交互稿（v2 原生，RDS Light/Dark + 11 场景可切） |
| `mock-architecture.md` | 为什么这样设计：不变式 / 概念模型 / 分层与状态所有权 / 数据流 / D1–D19 决策表 / 降级矩阵 / 已知问题 |
| `mock-dev-plan.md` | 做什么、做到哪：现状盘点 / Phase A–E 任务与落点 / 验收与风险 / 进度记录 |
| `crates/mock/README.md` | crate 级入口（特点与代码结构，不复述本目录设计） |

v1 素材（暂存区，删除前请先提炼）：`v1/docs/frontend/mock/mock-data-generator-design.md`（2957 行）、
`v1/docs/frontend/mock/mock-persistence-layer.md`（1005 行）、`v1/prototype/mock-data-generator.html`（2305 行）。

## 下一步（优化建议清单）

| # | 建议 | 收益 | 现状 |
| --- | --- | --- | --- |
| 1 | ~~生成走后台任务 + 进度 + 取消~~ | 十万行不再卡界面 | ✅ 已完成（`services::mock_jobs`；取消在批次边界） |
| 2 | ~~出口也纳入后台任务（Persist / Export）~~ | 落库与导出不再阻塞界面；完成后预览仍可用 | ✅ 本轮完成（架构 D23；出口不可取消：DuckDB / 文件系统无中断点） |
| 3 | ~~落库去文本中转（`ATTACH` 直写）~~ | 省一次全量序列化与解析；错误定位收在一处 | ✅ 本轮完成（`MockEngine::write_temp_table_to_database` + engine 的 `build_attach_database` / `build_insert_select`；架构 D25/D26） |
| 4 | ~~生成器搜索~~ | 137 项下按名称 / 标签定位 | ✅ 本轮完成（`search_generators` + `List`/`ListState` 搜索对话框，架构 D24） |
| 5 | ~~复杂参数编辑入口（集合 / 加权）~~ | 约束类生成器从「不可用」变可用 | ✅ 本轮完成（列编辑里的多行文本 + 生成前拦截空集合，架构 D28） |
| 6 | ~~临时表清理~~ | 切项目时释放进程级内存库里的临时表 | ✅ 本轮完成（两套前缀、以库为准；切项目时清理 + 面板作废旧预览，架构 D27） |
| 7 | 生成任务 / 模板**落库接线** | `MockGenerationStore` 8 方法 + 迁移 009 已就位但无 UI | §9-I4 |
| 8 | **项目作用域分析库** | 与「窗口 = 项目」隔离原则一致 | 装配层已留 `*_at(path, ..)` 入口 |
