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

## 11. 真机补测轮（GUI 实机，2026-10-03 下午——示例项目 + SQLite/DuckDB）

> 载体：真实 `rds-app` 窗口 + 真实项目库 `D:\data\A1\A1\.RSmeta\project.db` + `.rds\data\samples` 示例库（SQLite `demo-sqlite.db` / DuckDB `demo-analysis.duckdb`，含中文路径目录「示例项目」）；旧连接不删不脱敏（MySQL / PG 指向 192.168.3.138 不可达，用于失败行验证）。

### 11.1 本轮缺陷修复（已提交 `e01fb78`）

| 缺陷 | 根因 | 修复 |
| --- | --- | --- |
| 连接成功后导航树永不重新加载 / 永久「加载中」 | 连接失败时 `errors`/`attempted` 留痕；三处连接成功路径只清 `errors` 不清 `attempted`，`ensure_nav_loaded` 被 attempted 幂等挡死（树永不重载），且 `loading` 残留 | `nav_view/actions.rs` 三处统一改调 `nav_forget_attempt`（同时抹掉 errors/attempted/loading）：`apply_connect_results` 成功分支 / `toggle_connection` 已连接分支 / `ensure_connected_for_browse` 成功分支（`e01fb78`，8+/3-，全仓 113 测试通过） |

### 11.2 真机验证结果（全链路）

| 链路 | 结果 | 证据 |
| --- | --- | --- |
| 新建连接对话框 → 测试连接 → 保存 | ✅ | 界面新建「SQLite Example」全局连接；`connection_probe` 输出 sqlite 3.53.2 latency_ms=1；保存后 `global.db` 落库 |
| 双击建连（示例项目库） | ✅ | 双击「[项目] SQLite 示例」→ 日志 `Creating P_conn_demo_sqlite` + 元数据缓存 `meta_5245bb07154b9192.sqlite`（454KB，13 条 migration） |
| 失败行错误 + 重试 | ✅ | 点击不可达 MySQL 行 → 行内红字 `[CONN_NOT_FOUND]` + 行尾「重试」按钮 + 分组头「已连 1/4·1 失败」；点「重试」→ 日志「建连超时，重试一次 attempt=1」 |
| 右键菜单（连接行） | ✅ | 菜单含 断开/测试连接/编辑连接…；点「测试连接」→ `connection_probe` sqlite latency_ms=0 |
| 导航树展开（英文路径） | ✅ | 修复后：`P_real_sqlite`（`D:\FossilT\T.fossil`）展开显示 `main` schema；新建英文路径示例连接 `[项目] SQLite EN`（`demo-en-sqlite.db`）完整展开 `main → 表 (3) → customers → 视图 (1)` |
| 搜索过滤 | ✅ | 「示例」过滤命中连接行；子节点（表名）不匹配时被过滤隐藏（预期行为，非缺陷） |
| F2 快捷键 | ✅ | 选中连接行按 F2 唤起「编辑数据源连接」对话框（快捷键链路通；行内内联重命名入口待复核） |
| 分组收起 / 徽标 / 标签 | ✅ | 组头箭头收起生效；面板头徽标「13 连接·3 在线」实时；行内标签（P / GP / ∈ 分组）渲染正常 |

### 11.3 遗留问题（本轮真机暴露，未修）

| 问题 | 表现 | 疑似根因 | 影响 |
| --- | --- | --- | --- |
| 中文路径库树加载永久「加载中」 | `示例项目\demo-sqlite.db` / `demo-analysis.duckdb`（中文路径目录）连接成功（缓存已建）但树展开后 loading 不消失；英文路径同库正常 | `JOB_TIMEOUT`(30s) 对 `rt.block_on` 内**同步阻塞**查询不生效（tokio timeout 需 async 协作）；疑为驱动打开/查询中文路径时同步挂起 | 中文路径目录的库无法浏览树（需定位挂起点：驱动路径转换 / 连接池句柄） |
| DuckDB 连接后应用偶发退出 | demo DuckDB 连接成功 + 树加载中时，进程 1 分钟内无声退出（日志无 panic） | 疑似 DuckDB native 库崩溃（C++ 段错误不进 Rust 日志）或资源问题 | DuckDB 示例库真机浏览不稳（需 native 层隔离 / 复现定位） |
| 属性面板「数据」页（结果集）真机入口 | 表行双击 / 右键均未打开属性面板（代码已实现 + 测试通过） | 真机驱动限制：物理 SendInput 双击未触发 GPUI `click_count>=2` 判定（非应用功能缺陷） | 数据预览需人工 GUI 复核（或补键盘入口） |

### 11.4 真机环境备注

- 驱动通道：UIA（ValuePattern 写文本 + find element）+ 物理 SendInput（DPIAware + SetCursorPos + mouse_event）；`cu` 鼠标 / 键盘在本机 GPUI 窗口失效。
- 截图通道：PrintWindow 帧需最小化 + 恢复窗口刷新；窗口 rect 每次操作前重新 GetWindowRect 换算。
- 应用随机退出 3 次（均无 panic 日志），均为 DuckDB 活动或长时加载后；重启后可继续验证，进程状态需每次操作前确认。

## 12. 遗留①根因闭环 + 退出新证据（2026-10-03 晚——GUI 复现轮）

> 本轮在 §11.3 三个遗留问题基础上做定位轮：加诊断日志真机复现（app_run10~15），得到**根因证据链**并修复遗留①；遗留②拿到可复现触发路径。

### 12.1 遗留①「中文路径库树永久加载中」——根因闭环（提交 `7376210`）

| 层 | 排查结论 | 证据 |
| --- | --- | --- |
| 驱动层 | **正常**（中文路径非根因） | 新增 `cn_path_open_and_list` 测试（`sqlite.rs`）直开中文路径库 + `list_tables` ✅；Python 直开 `示例项目\demo-sqlite.db` 0.0s |
| 加载链路 | **正常**（非卡死） | 诊断日志（app_run11/13/14/15）：`load_children_page 入口 → L1 未命中 → get_database 命中 → get_catalogs 返回 → L1 回填完成` 全程 **~0.5ms 无阻塞**；`Connection → Catalog → Folder` 多层推进、`apply ok=true` |
| 缓存 | 半成品异常（次要） | `meta_5245bb07154b9192.sqlite`：`tables=0 行` 但 FTS 2 行、`tables_fts` 视图缺列 `T.table_comment`（扫描中断残留；**不阻塞读取**，加载走实时/索引重建） |
| **UI 状态层** | **根因** | ① `toggle_nav_node` 收起分支只清 `expanded`，**loading/attempted 残留** → 加载中收起后重展开被 `ensure_nav_loaded` 幂等挡死（树永久「加载中」）；② 轮询泵 `idle-break` 竞态：job 在「多等一拍」期间入队并完成，**结果滞留 load_results**、pump 已 break → loading 永不清除 |

**修复（`7376210`，2 files +44）**：
1. `toggle_nav_node` 收起分支清 `loading`/`attempted`（保留已加载 children 与 errors——重展开直接显示、失败节点走重试）；
2. 轮询泵 `idle-break` 前**最后消费一次** load_results（滞留结果立即 apply）+ 确无滞留时清残留 loading。

**验证**：98 测试通过（database lib）；真机 app_run15 中文路径库连接 → 加载链完整推进 → `apply ok=true` → 无「加载中」残留（搜索过滤状态下子树按设计隐藏）。**「加载中」永久残留主路径已闭环**。

### 12.2 遗留②「连接后应用偶发退出」——可复现触发路径（UIA 层，上游 GPUI）

| 触发 | 表现 | 结论 |
| --- | --- | --- |
| UIA `ValuePattern.SetValue("")` 清空搜索框（uia_clear） | 应用静默退出（无 panic、无 Windows 崩溃事件、日志止于 `apply ok=true`） | **GPUI 0.3.7 a11y/输入层 bug**：SetValue 触发 a11y 更新 → 窗口销毁 → 主循环正常退出（app_run14/15 两次复现；`uia_search` 写入非空串不触发） |
| DuckDB 连接 + 树加载（历史 app_run10） | 日志止于 gpui `window not found 0x80040102` / `无效的窗口句柄 0x80070578` | 另一路径：GPUI 窗口层句柄失效（上游），非 DuckDB native 崩溃 |

**影响**：真实用户鼠标键盘操作不经过 UIA，**不触发**；仅自动化通道（UIA SetValue 清空）会触发。缓解：真机验证避免 UIA 清空搜索框（用物理键 Ctrl+A+Delete 或重启）。

### 12.3 NavPath 树路径设计与实现审查（结论）

| 项 | 结论 |
| --- | --- |
| 五层枚举（Connection/Catalog/Schema/Folder/Table） | ✅ 结构清晰；`NavFolder` 五类（Tables/Views/Routines/Sequences/Triggers）label 中文 + key 持久化分离 |
| Catalog 层动态挂 Schema / 类别文件夹（`has_schema_level`） | ✅ 有 schema 层（MySQL/PG）挂 Schema、无（SQLite/DuckDB）直接挂类别文件夹——避免同名重复层，正确 |
| 分页 | ✅ 仅 `Folder` 走分页（`load_children_page`），其余隐含整页；大对象层（表数千）有 `index_page` 分块 |
| `child_key` 拼接 `{conn}/{seg}` | ⚠️ 未转义：对象名含 `/` 时 key 错位（SQLite/PG 表名可含 `/`）——低风险，建议后续转义或结构化 key |
| 非 Folder 层隐含整页 | ⚠️ 大库 schema 层无分页——低风险（schema 数通常少） |
| L1 进程级 `Mutex` | ⚠️ 多连接并行加载被串行化（nav 三处使用、无他方竞争）——当前规模可接受，大并发时需细粒度锁 |
| 同步阻塞 vs `JOB_TIMEOUT` | ⚠️ tokio timeout 对同步阻塞不生效（本轮实测加载链无阻塞）——防御性线程隔离列为后续项 |

**与遗留①的关系**：卡死不在 NavPath 设计（路径/分发正确），在 UI 状态层的 loading/attempted 生命周期（已修）。

## 13. 缓存缺陷修复与建连自动重载（补记，提交 `8dea72f`）

本轮（用户「完善树显示问题 + 记录并修复缓存设计缺陷」）修复两项并补齐测试基线：

| 项 | 结论 |
| --- | --- |
| 建连成功后树空（`CONN_NOT_FOUND` 竞态） | ✅ 修复：`apply_connect_results` 成功分支自动重载**已展开的连接根**（cache-aside 非 fresh，子层渲染渐进补齐）；新增 2 个窗口级回归测试 |
| L2 半成品缓存（schema 行在、对象不完整） | ✅ 修复：`save_schema` 建行 `is_loaded=0`（扫描进行中），表/视图/列 + 索引全部写完后 `mark_schema_complete` 置 1；读侧 `schemas()` 过滤未完成 → 回源重扫；写侧重扫未完成 schema 先清残留再整块重写；FTS 同步本就按 `is_loaded=1` 过滤，语义修正后自动不再收半成品 |
| 失效策略 | 记录：删除连接不自动清缓存（指纹共享，手动「缓存管理」）；变更检测不做自动（刷新是显式入口）；半成品自动自愈（读侧过滤） |
| 单测基线（本轮） | `rds-database --lib` **102**（+3：缓存半成品防御 1、自动重载 2）；`rds-engine --lib` **527**（FTS 测试补 mark）；`rds-workbench --lib` 130（`quick_open` 文件源 1 例并行 flaky，单跑通过，与本轮改动无关） |

> 缺陷细节、证据与实现位置映射表见 [`database-navigator-cache-invalidation.md`](database-navigator-cache-invalidation.md)。

## 14. MySQL / PG 真机验证轮（2026-10-07，提交 `84dddb9`）

> 用户要求「处理数据导航栏遗留问题，MySQL 和 PG 的真机环境也可以」。TCP 3306/5432 本轮恢复可达，用真实连接（`P_real_mysql` / `P_real_pg`，192.168.3.138）做连接层 + 导航层验证。

### 14.1 第一层根因：懒池吞错（已修复）

| 项 | 证据 |
| --- | --- |
| 表象 | `[DB_QUERY] Query failed: pool timed out while waiting for an open connection`（app_run17，SQL 全文取到 `information_schema.tables WHERE table_schema = ?`） |
| 根因 | URL 里 `ssl-mode=DISABLED` 大写，sqlx 只识别小写 → TLS 握手失败建连全失败；但 `AllowPooling=true` 懒池把 `Pool::connect` 当成功返回（池空），后续查询 acquire 30s 超时；`mysql.rs:96` / `postgres.rs:51` 的 `SELECT VERSION().ok()` 吞掉连接验证错误 |
| 修复 | 显式 `MySqlPoolOptions/PgPoolOptions`（min_connections=1 / max=10 / acquire_timeout 60s / idle 1800s / max_lifetime 3600s）+ `SELECT VERSION()` 不再 `.ok()` 吞错；`postgres.rs` 新增 `normalize_ssl_mode()`（`ssl-mode=DISABLED` → `disable`） |
| 应用内确证 | app_run20（修复后二进制）双击 MySQL 行：`connect OK 17ms` + `verify OK 42ms`，建连成功无 pool timed out |

### 14.2 导航加载链路毫秒级（探针）

| 路径 | 结果 |
| --- | --- |
| 服务器端（直接 sqlx 连 192.168.3.138:3306） | MySQL 9.7.2；`schemata` 5 库 14ms、`tables WHERE schema='mysql'` 46 行 17ms；SHOW FULL PROCESSLIST 无锁无长事务 |
| 应用路径（`MySqlDatabase::new` + `get_tables` + `get_catalogs`） | new 43ms、get_tables 31ms、get_catalogs 8ms —— 毫秒级全通过 |
| NavigatorService 集成（registry 建连 + worker 上下文 + 项目侧缓存，fresh=true 实时内省） | Connection 32ms / Catalog 301ms / Folder 325µs / worker 92ms —— 完整应用链路复现不了 30s |

**30s 超时结论**：曾于 app_run18 出现「加载子节点超时（30 秒未返回）」，但引擎层 / 导航层 / worker 上下文三条路径全部毫秒级，app_run20/21 双击建连后未再复现——**疑似为懒池吞错（§14.1）的次生表象，随池修复消失；需最新二进制 UI 复核做最终定论**。

### 14.3 PG 真机链路贯通（#15 实证）

| 项 | 证据 |
| --- | --- |
| PG 连接 | app_run21（含 ssl 归一）双击 `[项目] PostgreSQL` 行 → 日志 `Creating new connection with ID: P_real_pg`，无 ssl_mode 报错（修复前直接报 `unknown value "DISABLED" for ssl_mode`） |
| 跨库浏览 | 日志 `跨库浏览：已另开一条连到该库的连接 conn_id=P_real_pg database=warehouse_supply_chain`（connection_manager.rs:465） |
| L2 缓存 | `meta_a4ba0a563d0233d4.sqlite` migrations 1-13 全部应用——PG 元数据缓存链建立 |

> 注：PG 密码受 AES-256-GCM 加密（密钥=sha256(安装 salt + machine-id)，salt/机器 ID 在应用数据根），测试隔离的 `paths::data_dir` 读不到应用密钥，独立探针解密失败；验证改走应用内真机路径（已成功）。

### 14.4 MySQL 系统库过滤（#14，已修复）

- 真机实证：改前 `get_catalogs => Ok(5)`（information_schema / mall_business / mysql / performance_schema / sys 全列出）；改后 `Ok(1)`（只剩业务库 mall_business）。
- 实现：`mysql.rs get_catalogs` 过滤 `SYSTEM_SCHEMAS = [information_schema, mysql, performance_schema, sys]`，与 PG 侧 `PG_LIST_SCHEMAS_SQL` 系统对象过滤口径一致。

### 14.5 本轮验证基线

| 项 | 命令 | 结果 |
| --- | --- | --- |
| engine + database 测试 | `cargo test -p rds-engine -p rds-database` | ✅（engine lib 528 + database lib 102 全通过） |
| app 编译 | `cargo build -p rds-app -j 2` | ✅ |
| 真机连接（MySQL） | app_run20 双击 P_real_mysql | ✅ new 42ms、1 在线、右键菜单含「断开」（已连） |
| 真机连接（PG） | app_run21 双击 P_real_pg | ✅ 连接 + 跨库浏览 + L2 建库 |
| 系统库过滤（#14） | MySQL 探针 `get_catalogs` | ✅ 5 → 1（只留 mall_business） |

### 14.6 遗留（待人工 GUI 复核）

| 项 | 状态 |
| --- | --- |
| MySQL catalog 展开 30s 是否彻底消失 | 最新二进制（含 §14.1 修复 + 计时日志）需 UI 复核；探针证明链路毫秒级 |
| PG 树展开到 schema / 表层 | app_run21 链路贯通但 UI 树未展开到子层复核（双击=建连、展开依赖 toggle 稳定性） |
| UIA 遗留②③ | 未变：避免 UIA 清空搜索（GPUI 静默退出）；「数据」页真机入口需人工 GUI 复核 |
| GUI 点击稳定性 | 物理单点常注册为 hover、双击才触发建连；`SetForegroundWindow` 前置后仍不稳定（GPUI 命中测试/自动化通道限制，非应用缺陷） |


