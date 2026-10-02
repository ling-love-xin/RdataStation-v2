# 属性面板 · 真机验证报告（2026-10-03）

> 范围：M4 数据库导航栏 + 属性面板（rds-property 抽取后首版全量验证）
> 环境：本机 Windows / 无外部端点（MySQL / PG 不可达）→ 以 **SQLite + DuckDB 本地文件库**做全链路真机验证
> 验证载体：`crates/workbench/tests/nav_property_real.rs`（自建临时库，不走任何用户机器路径；数据根由 `paths` test-support 隔离）
> 运行：`cargo test -p rds-workbench --test nav_property_real -j 2 -- --nocapture`

## 1. 结论

| 项 | 结果 |
| --- | --- |
| SQLite 全链路（连接 → 导航 → 属性面板 → 数据页 → 灰置 → 缓存） | ✅ 通过 |
| DuckDB 全链路（同上） | ✅ 通过 |
| 回归基线（rds-property 单测 / database + workbench 测试 / app 编译） | ✅ 通过（见 §4） |

> 属性面板抽取与 tab 化改造、**数据预览（数据页签）**未破坏导航加载、内省取数与缓存链路；权限 / 依赖灰置页在两种文件型数据库下均按注册表正确呈现。

## 2. 验证链路（与 UI 同源）

| 步骤 | 服务入口 | 对应 UI 行为 |
| --- | --- | --- |
| 1 测试连接 | `DataSourceService::test` | 对话框「测试连接」 |
| 2 真实连接 | `ConnectionService::connect_with_type` | 导航「连接」 |
| 3 Catalog 内省 | `NavigatorService::load_children(Connection)` | 树第一层 |
| 4 类别文件夹 | `load_children(Catalog)` | 树第二层（无 Schema 层退化） |
| 5 表 / 视图 / 列 | `load_children(Folder/Table)` | 树逐层展开 |
| 6 属性面板 | `property_panel::load_properties` | 面板取数（表 / 视图 / 列 / 连接） |
| 7 数据页 | `table_data::load_table_data`（经 `execute_query`） | 「数据」页签（分页 / 排序 / NULL） |
| 8 灰置注册表 | `PropertyRegistryImpl::pages_for` | 页签集合 + 权限 / 依赖灰置 |
| 9 缓存 | `load_children` 二次调用 | cache-aside 写穿 L2 后命中 |

## 3. 用例与断言口径

### 3.1 SQLite（rusqlite，版本 3.53.2）

| 断言 | 结果 |
| --- | --- |
| 测试连接成功（1 ms） | ✅ |
| 真实连接成功（`nav_prop_sqlite`） | ✅ |
| Catalog 层 = `main`（无 Schema 层退化，FALLBACK_CONTAINER） | ✅ |
| 类别文件夹 = 表 (2) / 视图 (1)（**带计数**） | ✅ |
| 表层 = orders / parent（fixture 建表成功） | ✅ |
| 视图层 = v_orders | ✅ |
| 列层 = id / parent_id / tag / note（4 列） | ✅ |
| 表属性：网格 4 行 + 分区 [Columns, Indexes, Constraints, Ddl]（含列分区 4 行） | ✅ |
| **数据页：4 列 × 3 行、total=3、has_more=false、is_ordered=true（主键）** | ✅ |
| **分页：limit=1 首页 1 行且 has_more=true（limit+1 探测）；offset=2 末页且 parent_id=NULL（单元格 None → 置灰）** | ✅ |
| **排序：tag ASC 首行 t1 / DESC 首行 t3（列头排序链路）** | ✅ |
| **视图数据页：3 行且 is_ordered=false（无主键如实声明）** | ✅ |
| 视图属性：含列分区 | ✅ |
| 列属性：名称 / 所属表 / 类型 / 可空 / 主键 / 外键 / 序号 / 默认值 / 注释 | ✅ |
| 连接属性：含驱动等基础信息 | ✅ |
| 注册表：表 = [General, Columns, Indexes, Constraints, Ddl, **Data**, Privileges, Dependencies]（后两者灰置、Data 在权限前） | ✅ |
| 二次加载结果与首次一致（cache-aside） | ✅ |

### 3.2 DuckDB（v1.5.5）

| 断言 | 结果 |
| --- | --- |
| 测试连接成功（43 ms，v1.5.5） | ✅ |
| 真实连接成功（`nav_prop_duckdb`） | ✅ |
| Catalog 层 = `main` | ✅ |
| 类别文件夹 = 表 (2) / 视图 (1) | ✅ |
| 表层 / 视图层 / 列层 与 SQLite 完全同构 | ✅ |
| 表 / 视图 / 列 / 连接属性面板 | ✅ |
| **数据页 / 分页（has_more 探测、NULL 单元格）/ 排序（ASC/DESC）/ 视图数据（is_ordered=false）** | ✅ 与 SQLite 口径一致 |
| 注册表灰置（权限 / 依赖）+ Data 页位置 | ✅ |
| 二次加载（cache-aside） | ✅ |

> 两驱动断言口径完全一致（同一 `run_nav_property_chain` 参数化），验证「**导航、属性面板与数据预览不因驱动差异而分叉**」。
> DuckDB 主键不自增（`INTEGER PRIMARY KEY` 非 rowid 别名）——fixture 显式插入 `id`，属测试数据写法，非产品缺陷。
> 数据页实现要点：驱动 `query` 只填 Arrow `batches`、`rows` 字段恒空（引擎已知坑）——`table_data` 走 `rows_as_json` 从 batches 提取，并在本报告链路内断言数据真实落库；文本单元格经 `serde_json::Value::String` 取内部值（避免 JSON 序列化自带的引号）。

## 4. 回归基线（本次改动后）

| 项 | 命令 | 结果 |
| --- | --- | --- |
| rds-property 单测 | `cargo test -p rds-property` | ✅ 4/4 |
| 全链路验证 | 本报告 §3 | ✅ 2/2 |
| database + workbench 全量 | `cargo test -p rds-database -p rds-workbench -j 2` | ✅ 全绿（仅历史 `zz_fixture_probe` 连真实 MySQL 端点超时，环境问题与本改动无关） |
| app 编译 | `cargo check -p rds-app` | ✅（engine 2 个既有 warning：`metadata_cache_index.rs` 文档注释 + 未用 import，非本次引入） |

## 5. 覆盖边界与缺口

| 维度 | 状态 |
| --- | --- |
| MySQL / PostgreSQL | ⏭ 本环境无端点；由既有真机套件（`ddl_from_real_schema` 等，环境变量注入后）覆盖 |
| GUI 像素级冒烟 | ⏭ 未做（本报告为服务层全链路，与 UI 同源）；S4 清单可补一条人工冒烟 |
| 大 schema 分页（>500 对象） | ⏭ 逻辑由 `load_children_page` 单测覆盖，未做 500+ 真机库 |
| DDL 源版取回（sqlite_master / duckdb_tables） | ✅ 链路已验证（表属性含 DDL 分区）；原文比对见 `source_ddl_from_real_schema.rs`（环境变量注入后） |
| 权限 / 依赖真实数据 | ⏭ 驱动未实现能力位，灰置为预期行为（非缺陷） |
