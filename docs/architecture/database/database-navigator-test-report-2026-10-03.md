# M4 数据库导航模块 · 真机验证报告（2026-10-03）

> 对齐体例：`docs/architecture/connection/connection-test-report-2026-10-01.md`（M3 测试报告）。
> 范围：M4 数据库导航栏全链路——连接 → 导航树（Catalog / 类别文件夹 / 表 / 视图 / 列）→ 属性面板 → 数据预览 → 注册表灰置 → 缓存（cache-aside）。
> 环境：本机 Windows / 无外部端点（MySQL / PG 不可达）→ 以 **SQLite 3.53.2 + DuckDB v1.5.5 本地文件库**做全链路真机验证。
> 验证载体：`crates/workbench/tests/nav_property_real.rs`（自建临时库；数据根由 `paths` test-support 隔离，不走任何用户机器路径）。

## 0. 结论摘要

| 项 | 结果 |
| --- | --- |
| SQLite 全链路（连接 → 导航 → 属性 → 数据预览 → 灰置 → 缓存） | ✅ 2/2（SQLite / DuckDB 各跑同一参数化链路） |
| 单元回归（database lib / workbench lib / property） | ✅ 98 / 131 / 4 全绿 |
| 应用编译（`cargo check -p rds-app`） | ✅（engine 2 个既有 warning 非本次引入） |
| 文档 - 代码一致性 | ✅ 本轮补齐 `database-navigator-module-spec.md`（对齐 M3 规格书体例），README 文档地图同步（见 §4） |

> 导航栏与属性面板、数据预览**不因驱动差异而分叉**：两种文件型数据库断言口径完全一致（同一 `run_nav_property_chain` 参数化）。

## 1. 复现命令

```powershell
# 全链路真机验证（SQLite + DuckDB）
cargo test -p rds-workbench --test nav_property_real -j 2 -- --nocapture

# 单元回归
cargo test -p rds-database -j 2 --lib
cargo test -p rds-workbench -j 2 --lib
cargo test -p rds-property -j 2

# 应用编译（必须 -j 2）
cargo check -p rds-app -j 2
```

## 2. 单元测试覆盖（按模块）

| crate | 数量 | 覆盖点 |
| --- | --- | --- |
| `rds-database`（lib） | 98 | `nav_view` 状态机（展开 / 选中 / 过滤 / 分页游标）、失败节点重试、`property_ref_of` 映射（kind 全量）、`table_data`（quote_ident 转义 / 限定名去重 / 单元格 JSON 化 / 分页探测）、`property_panel` 注册表与分区、缓存路径 |
| `rds-workbench`（lib） | 131 | Quick Open `ShowProperties` 构造、`apply_props_results` 过期 key 丢弃、面板装配 / 命令路由 |
| `rds-property` | 4 | kind 覆盖 / 描述模型构造 / `PropertyState` 默认值 / 注册表契约（mock registry） |

## 3. 真机集成（`nav_property_real.rs`）

同一 `run_nav_property_chain` 参数化跑两驱动，链路与断言（两驱动一致）：

| 步骤 | 断言（SQLite / DuckDB 均通过） |
| --- | --- |
| 测试连接 | 成功 + 版本（SQLite 3.53.2 / DuckDB v1.5.5） |
| 真实连接 | 成功（`nav_prop_sqlite` / `nav_prop_duckdb`） |
| Catalog 层 | `main`（无 Schema 层退化，FALLBACK_CONTAINER） |
| 类别文件夹 | 表 (2) / 视图 (1)（**带计数**） |
| 表层 / 视图层 / 列层 | orders / parent · v_orders · id / parent_id / tag / note（4 列） |
| 表属性面板 | 网格 4 行 + 分区 [Columns, Indexes, Constraints, Ddl]（列分区 4 行） |
| 数据页 | 4 列 × 3 行、total=3、has_more=false、is_ordered=true（主键） |
| 分页 | limit=1 首页 has_more=true（limit+1 探测）；offset=2 末页且 parent_id=NULL（单元格 None → 置灰） |
| 排序 | tag ASC 首行 t1 / DESC 首行 t3（列头排序链路） |
| 视图数据 | 3 行且 is_ordered=false（无主键如实声明） |
| 列 / 连接属性 | 所属表 / 类型 / 可空 / 主键 / 外键 / 序号 / 默认值 / 注释 · 驱动等 |
| 注册表 | 表 = [General, Columns, Indexes, Constraints, Ddl, Data, Privileges, Dependencies]（后两者灰置、Data 在权限前） |
| 二次加载 | 结果与首次一致（cache-aside 写穿 L2 后命中） |

> fixture：`parent(id, name)` 与 `orders(id, parent_id, tag, note DEFAULT 'x'，UNIQUE idx_orders_tag，FK parent_id→parent.id)`，视图 `v_orders`；INSERT 3 行显式 id（DuckDB 主键不自增，属测试数据写法非产品缺陷）。

## 4. 文档 - 代码一致性（本轮查缺补漏）

对照 M3 `connection` 文档族逐项核对，**补齐缺口**：

| 文档 | 状态 |
| --- | --- |
| `database-navigator-module-spec.md` | **新增**（对齐 `connection-module-spec.md` 13 节：边界 / 代码地图 / 数据模型 / 关键流程 / 缓存体系 / 测试 / 复现命令 / 模块关系） |
| `database-navigator-test-report-2026-10-03.md` | **新增**（本文件，对齐 M3 测试报告体例） |
| `database/README.md` | 文档地图（§6）补两行新文档 |
| 其余（architecture / prototype-design / prototype.html / showcase.* / user-guide / dev-plan / cache-vs-dbeaver） | 已齐，内容以代码实证为准（早前轮次交付） |

代码侧抽查口径（规格书 §3 / §5 引用均实证）：`navigator_service.rs` 公开接口（`load_children` / `load_children_page(path, offset, limit)` / `warm_schemas` / `prefetch_columns` / `load_properties`）、`nav_jobs.rs` Job 枚举（LoadChildren / LoadProperties / SearchIndex / Warm / PrefetchColumns / GenerateDml / TestConnection）、`metadata_service.rs` 内省清单、`model.rs` `PropertyRequest.data` 透传。

## 5. 原型对齐验证（`database-navigator-prototype-design.md` / `.html`）

| 设计点 | 代码落地 | 一致 |
| --- | --- | --- |
| 连接行 3 元素密度（徽标 + 名称 + 归属域列） | `nav_view/rows.rs` 行渲染 + 显示开关 | ✅ |
| 双通道徽标（颜色 = 状态 / 形状 + 2 字母 = 类型） | 徽标映射 + hover 卡 | ✅ |
| 层级随驱动动态（有无 Schema 层） | `has_schema_level` + `FALLBACK_CONTAINER` | ✅ |
| 类别文件夹带计数 + 首批 200 + 加载更多 | `load_children_page` + 计数内省 | ✅ |
| 拖拽表 / 视图进编辑器 | `NavDragPayload` → `request_query` | ✅ |
| 属性面板数据页（排序 / 分页 / NULL 灰置） | `render_data_page` + `table_data` | ✅ |

## 6. 发现的问题与建议

1. **MySQL / PostgreSQL 无端点**（本环境）：连 `192.168.3.138` 超时 `os error 10060` 属环境问题；由既有真机套件（`ddl_from_real_schema` 等，`RDS_TEST_*` 环境变量注入后）覆盖，非本模块缺陷。
2. **`zz_fixture_probe.rs`（未跟踪，含内网明文凭据）**：不依赖、不删除、不脱敏（用户明确保留，后续验证用）；勿 `git add`。
3. **引擎既有 warning**（`metadata_cache_index.rs` 文档注释 + 未用 import）：非本次引入，未动。
4. **大 schema（>500 对象）真机未做**：分页逻辑由 `load_children_page` 单测覆盖；可补一条 500+ 真机库用例。
5. **GUI 像素级冒烟未做**：本报告为服务层全链路（与 UI 同源）；验收清单（`database-navigator-user-guide.md` §9）可补人工冒烟。

## 7. 变更文件清单（本轮查缺补漏）

| 文件 | 动作 |
| --- | --- |
| `docs/architecture/database/database-navigator-module-spec.md` | 新增 |
| `docs/architecture/database/database-navigator-test-report-2026-10-03.md` | 新增 |
| `docs/architecture/database/README.md` | 文档地图 §6 补两行 |
| `docs/architecture/module-status.md` | M4 行补文档族齐备注记 |

## 8. 遗留项

权威清单见 `database-navigator-architecture.md` §11；摘要：

| 类别 | 项 |
| --- | --- |
| 模块内可做 | 系统库 / 系统 schema 过滤；大 schema 列内联展开阈值；标签命名规范 |
| 需协调 | PostgreSQL 跨库浏览（另开连接）；元数据缓存键切身份指纹（C8） |
| 依赖其他模块 | Mock（M7）/ 洞察（M8）面板装配；SQL 可执行区脱离 `use_duckdb_fed` 限制 |
| 数据预览编辑回写 | 只读预览为已知边界；编辑（行身份 / 事务 / 暂存）待驱动 `table_editor` 能力位（见 `../editor/editor-table-editing-plan.md`） |

## 9. 缺陷修复记录（本模块相关轮次）

| 缺陷 | 根因 | 修复 |
| --- | --- | --- |
| 数据预览 0 行 | 驱动 `query` 只填 Arrow `batches`、`rows` 字段恒空（引擎已知坑） | `table_data` 走 `rows_as_json` 从 batches 提取（2026-10-03） |
| 文本列显示带引号 | `serde_json::Value::String.to_string()` 是 JSON 序列化（自带引号） | `cell_text` 对字符串取内部值（2026-10-03） |

## 10. 回归基线（数据预览轮，2026-10-03）

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 真机全链路 | `cargo test -p rds-workbench --test nav_property_real` | ✅ 2/2 |
| database lib | `cargo test -p rds-database --lib` | ✅ 98 |
| workbench lib | `cargo test -p rds-workbench --lib` | ✅ 131 |
| property | `cargo test -p rds-property` | ✅ 4 |
| app 编译 | `cargo check -p rds-app` | ✅ |
| 原型 JS 语法 | `node --check`（prototype / showcase html 内嵌脚本） | ✅ |
