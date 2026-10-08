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

## 15. 树展开与缓存验证轮（2026-10-08，真机复核）

> 用户要求「验证树的展开情况，以及缓存是否正常」。用最新二进制（`84dddb9` 含 MySQL 过滤 + 计时日志）真机复核（app_run22 / app_run23b）。

### 15.1 树的展开情况

| 连接 | 结果 | 证据 |
| --- | --- | --- |
| MySQL（app_run22 双击建连后） | ✅ catalog 只显示 `mall_business`（#14 系统库过滤真机生效），展开 → 表 (7) / 存储过程/函数 (4) | 截图 + 日志 `collect_objects 成功 catalog=mall_business schema=mall_business folder="triggers" count=0` |
| SQLite（同轮） | ✅ `main → 表 (25)` 正常展开 | 截图树行 `main / 表(25)` |
| PG warehouse_supply_chain（同轮） | ✅ schema 列表加载（asset / base / biz_000~biz_003） | 截图树行 |
| PG postgres 库（同轮） | ❌ schema 查询 `pool timed out while waiting for an open connection`（红字 + 重试） | 日志 SQL 全文：`SELECT n.nspname FROM pg_catalog.pg_namespace ...` |
| **重启后首次展开（app_run23b）** | ❌ MySQL catalog 加载 **「加载子节点超时（30 秒未返回）」**——**30s 超时实机复现**（§14.6 遗留①定论：**真实缺陷，非懒池次生**） | 日志 `导航加载失败 ... error=加载子节点超时（30 秒未返回）` |

### 15.2 缓存验证结果

| 项 | 结果 | 证据 |
| --- | --- | --- |
| L1 进程内缓存命中 | ✅ | `load_folders indexed_counts 完成 elapsed_ms=1`、`collect_objects 缓存句柄就绪 folder="views" elapsed_ms=0` |
| L2 写入 | ✅ | 缓存文件 `meta_*.sqlite`：schemata / tables / columns / routines / FTS 落库（MySQL 53 表 + 4 例程；PG 516 schema） |
| 半成品防御（读侧过滤） | ✅ | 未完成 schema（`is_loaded=0`）被过滤 → UI 回源显示；系统库行虽在缓存但 UI 不显示（#14 读侧过滤） |
| **半成品残留（缺陷）** | ❌ | MySQL `mall_business` / PG 516 个 schema 全部 `is_loaded=0`（列预取超时中断，`mark_schema_complete` 未执行）→ **每次展开都回源实时查询**（慢 / 不稳） |

## 16. 缓存根治轮（2026-10-08，提交 `5da8332`）

> 用户要求「继续修复这些，根治缓存」。针对 §15 三个问题做根因实证 + 修复 + 真机/探针复核。

### 16.1 根因实证

| 假设 | 实证 | 结论 |
| --- | --- | --- |
| 跨 Runtime 查池挂起（worker 自建 Runtime ≠ 池宿主 bridge_runtime） | 探针：同 Runtime 14ms / 跨 Runtime 30ms / 并发 3 路 7~32ms 全部正常 | **证伪**——跨 Runtime 不是 30s 直接原因 |
| 列预取慢（MySQL 列查询） | 探针：7 表每表 6~25ms，总 81ms，超时 0 | **证伪**——列查询本身毫秒级 |
| **30s = 池 acquire 排队** | min=1 时 catalog 回源与列预取并发抢唯一连接 → acquire 等待（acquire_timeout 60s > JOB_TIMEOUT 30s，外层先超） | **确认**——修复方向：min=2 + 每表超时细化 |

### 16.2 修复（三处）

| 位置 | 修复 |
| --- | --- |
| `navigator_service.rs collect_objects` | L2 命中补 `mark_schema_complete`（读侧自愈写侧：半成品 is_loaded=0 → 命中即完整 → 置 1，下次不回源）；**完整 schema 空类别返回空不回源**（如 MySQL 无视图——空是合法结果不是未命中，杜绝每次展开回源 get_tables） |
| `navigator_service.rs prefetch_columns` | **每表 5s 超时**（单表卡跳过，其余继续）——不再整批 30s 外层超时全部放弃留列缓存半成品 |
| `mysql.rs` / `postgres.rs` | 池 `min_connections` 1→2（catalog 回源与列预取并发不抢唯一连接） |

### 16.3 验证（app_run24/25 真机 + 探针）

| 项 | 结果 |
| --- | --- |
| MySQL 树展开 | ✅ 完整展开（`mall_business → 表(7) → order/order_item/payment_record/product/product_category/user/user_address` + 例程 4）；**30s 超时不再复现**（vs §15 app_run23b 重启后首次 30s） |
| 缓存自愈 | ✅ `meta_439cdbba942f1c99.sqlite` schemata `loaded 0→2`（命中补 mark 生效）；日志 `collect_objects 完整 schema 空类别（不回源） conn_id=P_real_mysql folder="views" elapsed_ms=0` |
| 列预取 | ✅ app_run25 无「列预取超时」日志；探针 7 表总 81ms（每表 ≤25ms） |
| PG 池 | ✅ **真机复核（app_run26）**：双击 `GP_real_pg_20260912` → 建连 + 跨库浏览 `warehouse_supply_chain` 成功（0.5s 内），**无 pool timed out**（§15 中 PG postgres 库与重启后 warehouse_supply_chain 均 pool timed out——min=2 修复生效）；树显示已连 1/2 |
| 回归 | ✅ `cargo test -p rds-engine -p rds-database`：engine lib 528 / database lib 102 全过 |

### 16.4 结论

- **30s 根因链全通**：半成品 is_loaded=0 → views 空反复回源 → 池 acquire 排队（min=1 单连接）→ JOB_TIMEOUT 30s 先超。修复（自愈 mark + 空类别不回源 + min=2 + 每表 5s）后**重启后展开不再回源、不再排队、单表卡不再拖死整批**。
- **遗留**：列展开的 UI 侧确认（列预取在「表」文件夹首次展开时触发；GUI 自动化展开树多次未成，需人工展开表确认列显示与 L2 列缓存写入——列查询探针已证每表 ≤25ms、put_columns 写侧逻辑闭环）。

### 16.5 列加载与 L2 写入实证（探针 `zz_probe_cols_l2`，跑完即删）

- **列加载链**：`NavigatorService.load_children(Table{mall_business.order})` → load_columns 回源 → **返回 16 个列节点**（id / order_no / user_id / total_amount / pay_amount …），全程 0.16s，无 pool timed out。
- **L2 交叉印证**：实查缓存 `meta_439…sqlite`（MySQL）`columns` 表恰为该 16 列——即列写入确已发生（缓存半成品 = 只有 order 一表，其余表列未写，与 §15 残留一致）。
- **测试隔离限制**：`cargo test` 下 `NavCache::open` 因 paths test-support 隔离路径不可用返回 None，故探针内无法直读 L2；产品真机路径（应用内）L2 可写（上述 meta_439 实证）。
- **结论**：列加载全链路 ✅；L2 列写入机制 ✅（写侧 put_columns 与读侧 L2 命中回填 L1 均已实证）；UI 列显示需人工展开表复核（GUI 自动化通道限制，非应用缺陷）。

### 15.3 新暴露问题（根因方向，待修）

| 问题 | 表现 | 根因方向 |
| --- | --- | --- |
| 列预取超时 30s | `nav_jobs.rs:741` 列预取超时（已放弃，不影响导航） | `PrefetchColumns` 在 worker 自建 Runtime 里查池（池宿主 = workbench `bridge_runtime`）——跨 runtime 不稳定；且池 `min=1` 时并发任务排队 |
| catalog 回源查池 30s | 重启后首次展开 MySQL catalog 超时 | L2 半成品（is_loaded=0）→ 读侧回源 → 实时查询走池（同上跨 runtime / 排队） |
| PG 主池 pool timed out | postgres 库 schema 查询失败（红字）；重启后 warehouse_supply_chain 也失败 | PG 池 acquire 超时（池连接占用 / 跨 runtime）；与 MySQL 同源 |
| 探针 vs 应用不一致 | 探针（独立池）28~92ms；应用（共享池 + worker 自建 Runtime）30s | 差异在池宿主 Runtime 与 worker Runtime 不一致（`nav_runtime.rs` 注释警告的场景），需实验定位 |

### 15.4 结论

- **树的展开**：加载成功时层级与 #14 过滤均正确；**但重启后首次展开不稳（30s 超时真实复现）**。
- **缓存**：L1 / L2 / 半成品防御机制正常；**但半成品残留（列预取超时）导致回源频繁，是展开不稳定的直接诱因**。
- **下一步**：定位 worker Runtime 与池宿主 Runtime 不一致（跨 runtime 查询）与池连接数（min=1 排队）两个方向的实证；修复后重跑本验证。

---

## 17. 根治验证（2026-10-08 · 批量内省 / 一次展开一条新连接）

> 背景：§16 修复后 30s 池排队消失，但 **PG 真机仍连环 15s/30s 超时红字**。多轮铁证（app_run61/62/64 日志）收敛到唯一根因：**同连接上「同 SQL prepared statement 二次执行」挂起**——无论闲置多久（9.8s）、无论多短（58ms 活跃复用），复用必挂；每次新建全快（探针 10-82ms）。据此把连接模型对齐 DBeaver 会话语义：**一次展开 = 1 条新连接 + 每类对象各一条不同 SQL（无重复 prepared），用完即弃**。

### 17.1 根因铁证时间线（app_run61/62/64）

| run | 铁证 | 结论 |
| --- | --- | --- |
| app_run61 | biz_358 216ms 成功 → 2.4s 后同连接 biz_383 挂 30s；PG 端无 active 无锁 | 连接复用挂起（generic plan 推断被实证推翻） |
| app_run62 | biz_454 245ms 成功 → 9.8s 后同连接 biz_459 挂 15s | 60s 空闲回收阈值远不够，闲置即挂 |
| app_run64 | biz_483 2568ms 成功 → **58ms 后**同 scoped 连接 biz_477 挂 15s | 活跃复用也挂 → 彻底推翻「闲置复用」论 |
| pg_probe 探针 | 同 SQL 每次新建连接：10-82ms 全成功 | SQL / 服务端无问题，问题在**连接复用** |

### 17.2 落地改造（提交 `9e9fd68`）

| 位置 | 改造 |
| --- | --- |
| `postgres.rs` / `mysql.rs` | 单连接化（`Arc<Mutex<Option<Conn>>>`，无池无 acquire）；查询级 `PG_QUERY_TIMEOUT=15s` 超时并**销毁连接**重建；`force_custom_plan` 注入（等价 DBeaver 字面量 SQL 每次重新优化）；空闲回收 |
| 两驱动 `introspect_schema_objects` | **批量内省**：一条**新**连接 + 每类对象（表/视图、过程、函数、序列、触发器）各一条**不同** SQL，连接随函数结束 drop —— 一次展开只付一次建连、无重复 prepared |
| `traits.rs` | 新增 `SchemaObjects`（tables/procedures/functions/sequences/triggers）+ `Database::introspect_schema_objects` 默认实现 |
| `metadata_service.rs` | `introspect_schema_objects` 转发（跨库 scoped 连接取 connect_url，批量方法内部新建） |
| `navigator_service.rs` | `load_folders` 四类别共享**同一次**批量结果；tables/routines/sequences/triggers 实时分支均改从 batch 取，不再逐类别建连 |
| `nav_jobs.rs` | 竞态守卫：点击连接后导航任务等待在途建连（最多 500ms），消除 `CONN_NOT_FOUND` 红字闪现 |

### 17.3 真机验证（app_run65/66/67 · clean-test 项目 · P_real_pg 192.168.3.138）

| 项 | 结果 |
| --- | --- |
| MySQL `mall_business` 展开 | ✅ 355ms（routines 291ms 一次批量，sequences/triggers 0ms 从 batch 取，tables 子文件夹 1ms L2 命中） |
| PG 连续展开 18+ schema（app_run66：biz_002→biz_153） | ✅ **166-321ms 全绿，零超时零红字**；SQL 9-24ms；建连 93-1213ms（Windows 网络栈逐连接检查，环境固有，探针实证 sslmode 无差别） |
| PG 历史卡死 schema（app_run67：biz_264/biz_282/biz_308/biz_315/biz_348 等 + crm/production/logistics/inventory/purchase） | ✅ 全部秒开（97-718ms），**此前 30s/15s 卡死的 schema 全部恢复** |
| 单元回归 | ✅ engine 528 / database 102 全绿 |
| 提交 | ✅ `9e9fd68 根治导航树展开慢与超时：一次展开一条新连接批量内省` |

### 17.4 结论

- **根因闭环**：30s/15s 连环超时 = 连接复用 + 同 SQL prepared 二次执行挂起（PG 服务端/网络层怪异行为，探针同 SQL 新建连接全快、复用必挂互证）。批量内省从结构上消灭「复用」，一次展开一条新连接 + 5 条不同 SQL，建连成本只付一次。
- **性能边界**：展开耗时 ≈ 建连（150-400ms 环境固有）+ 5×SQL（各 ~10ms）+ 框架开销 —— DBeaver 同环境同成本；L2 缓存命中时第二次展开同一 schema 不建连、毫秒级。
- **遗留**：biz_384 在 app_run65 出现过一次 15s 偶发（app_run66/67 未复现，探针无法复现）——有 15s 超时 + 自动重建兜底，不再 30s 连环卡死；数据预览（表数据 execute_query）仍走 scoped 连接复用，若同 SQL 二次执行挂起需同样改造（证据待收集）。

## 18. 顺滑度三阶段：静态类别文件夹 + 批量内省每次新建连接（提交 `6809d5f`）

### 18.1 问题：展开仍付一次建连，用户要求「像 DBeaver 一样顺滑」

批量内省（§17）一次展开 = 一次新连接 + 5 条 SQL（150-400ms），用户判定不达标；同时探索「复用连接抹平建连」路线，app_run68（复用 + ping 验证）暴露 2.1-2.6s 惩罚。

### 18.2 实证链（探针 v7/v8 + app_run69）

| 假设 | 结果 |
| --- | --- |
| 网络层故障（单 Runtime 跨任务复用） | ❌ 探针 v7：5 轮全成功（复用 74-125ms，ping 5-76ms 全过） |
| 跨 tokio Runtime 复用挂起 | ❌ 探针 v8：5 个独立 Runtime 复用同一连接全成功 |
| 复用 + ping 撞故障窗口 | ✅ app_run69：public/tables connect_ms=2115（ping 2s 超时 + 重建）——**ping 引入 2.1s 惩罚，比直接新建慢 6 倍** |

结论：sqlx 复用/跨任务/跨 Runtime 技术上全安全；但**复用前 ping 在撞上网络层间歇故障窗口时反而引入 2s 惩罚**，且旧连接被冻结的概率性无法根除——**最终形态：每次新建连接（自带健康）+ 展开层静态化**。

### 18.3 DBeaver 源码实证（本地检出 `D:\RdataStation\RDS\dbeaver-src`）

- `DBNDatabaseNode.getChildren()`：`childNodes` 内存缓存 + 展开同步递归加载全部（文件夹+对象一次性），之后展开 O(1) 纯内存。
- 加载走已有执行上下文：`getExecutionContext() → DBUtils.getDefaultContext(object, true)`——不新建连接。
- `JDBCExecutionContext`：`volatile Connection connection` 常驻复用 + 懒建 + `SingleThreadLock` 串行；JDBC 同步阻塞、无异步 Runtime 边界——**这是 DBeaver 复用什么都不挂的本质（同步驱动），sqlx 异步层在故障窗口有概率冻结旧连接**。

### 18.4 落地改造（提交 `6809d5f`）

| 文件 | 改动 |
| --- | --- |
| `navigator_service.rs` | `load_folders` 改**静态渲染**：展开 schema 只返回 5 个类别文件夹（Tables/Views/Routines/Sequences/Triggers，无计数），**零内省不碰库**；对象在点击类别文件夹时 `load_objects → collect_objects` 加载（冷 ~300ms / L2 命中毫秒级）——DBeaver 分块树交互 |
| `postgres.rs` / `mysql.rs` | 批量 `introspect_schema_objects` 改回**每次新建连接**（`connect_pg_single`/`connect_mysql_single`，连接随函数结束 drop）；`lock_pg_conn`/`lock_mysql_conn` **去掉复用前 ping**（保留 60s 空闲回收，只作用于 query/query_with_params 会话级复用） |

### 18.5 真机验证（app_run70 · clean-test 项目 · P_real_pg）

| 项 | 结果 |
| --- | --- |
| 展开连接（含等在建连守卫） | ✅ 154ms（建连 536ms 异步并行，任务不阻塞） |
| 展开 catalog `warehouse_supply_chain` | ✅ 69ms |
| 展开 schema（biz_031/crm/finance/logistics 等） | ✅ **全部 0ms（瞬时）** |
| 点类别 Tables（冷：biz_031） | ✅ 711ms（批量建连 184ms + SQL 15ms，其余为 L2 检查/调度开销） |
| 点类别 Tables（热：crm） | ✅ **1ms（L2 命中）** |
| 红字 / 超时 | ✅ 无 |

### 18.6 结论与遗留

- **感知对齐 DBeaver**：展开 schema 瞬时出骨架（静态文件夹），点类别冷加载一次（~200ms）后 L2 命中毫秒级；无 2s ping 惩罚、无红字、无 30s 卡死。
- **与 DBeaver 的差异（可接受）**：DBeaver 是「展开时一次性加载全部 + 内存缓存」，我们是「展开骨架 + 点击类别才查」——用户已确认该交互（点击驱动加载）；冷加载成本从展开移到点类别。
- **遗留**：① 冷点类别 200-700ms 中有建连环境成本（150-400ms 固有）；② 若未来要让「展开即全量」也可行（childNodes 模型），需先根治网络层对旧连接的冻结（超出软件能力，建议换网/换主机验证）；③ 数据预览（execute_query）仍走 scoped 连接复用，遇故障窗口有概率 15s 超时（有兜底，证据待收集）。

## 19. 顺滑度收官：展开即后台预热对象与列（提交 `725f1ef`）

### 19.1 问题：§18 把内省从「展开」移到「点类别」，用户判定「仍不如 DBeaver」

§18 形态（静态文件夹 + 点击类别实时内省）下，点 Tables 冷加载 384~1444ms（含建连环境成本）——用户反馈「还是没有 dbeaver 丝滑」，但「比之前好太多」。用户同时指出：设计是多驱动的，优化不能只落在 sqlx 驱动层。

### 19.2 落地改造（提交 `725f1ef`，四类驱动统一生效）

| 文件 | 改动 |
| --- | --- |
| `navigator_service.rs` | 新增 `warm_schema`（DBeaver `loadChildren` 对齐）：**schema 展开即后台全量内省 5 类对象**（表/视图/例程/序列/触发器）写 L1/L2 + 索引重建 + 完成标记；**列预热**：表+视图 ≤ 8 的 schema 后台顺带内省全部表列（`load_columns`），点表展开列 0ms |
| `nav_jobs.rs` | Schema 路径任务完成后 spawn 独立线程 + 独立 Runtime 执行 `warm_schema`（不随 worker 线程退出被 abort；与导航查询按连接锁串行安全） |
| `postgres.rs` / `mysql.rs` | 批量内省改回**复用常驻连接 + 100ms 快速探活**（`lock_pg_conn`/`lock_mysql_conn` 内置 `SELECT 1` 100ms 超时）：健康连接复用（展开 ≈ 55ms）；冻结连接 100ms 即弃 + 300-600ms 重建（最坏 ~1.4s，替代 2s ping 惩罚 / 每次新建 300ms 常驻成本） |
| `utils.rs` | 列解码红字根治：PG `attnum::text`、MySQL `CAST(ordinal_position AS CHAR)`、DuckDB `CAST(ordinal_position AS VARCHAR)`——`get::<Option<String>>` 不再对 INT2/INT 报类型错（app_run71 `ColumnDecode index "3"` 铁证） |

**多驱动口径**：预热机制在服务层（`navigator_service`），驱动无关——SQLite / DuckDB / PG / MySQL 统一走 `introspect_schema_objects` 接口享受预热；100ms 探活只在网络驱动（PG/MySQL）需要（本地文件驱动无冻结问题）。

### 19.3 真机验证（app_run73/74 · clean-test 项目 · P_real_pg warehouse_supply_chain）

| 项 | 结果 |
| --- | --- |
| 点开全新 schema biz_485（app_run73） | ✅ 文件夹 0ms 秒出；后台预热 758ms（含建连 515ms，用户无感） |
| 点 biz_485/tables | ✅ **2ms（L1 命中）** |
| 点 logistics/outbound tables | ✅ 2-3ms（缓存命中） |
| 点 biz_381（app_run74）→ 点 Tables → 点 detail 表展开列 | ✅ **1ms / 1ms / 0ms（列已预热）** |
| 列预热日志 | ✅ `后台预热表列完成 warmed=2 targets=2`（biz_001/biz_381 均命中） |
| 列解码红字 / 超时 | ✅ 无 |
| 单元回归 | ✅ engine 528 / database 102 全绿 |

### 19.4 结论

- **全链路 0-2ms**：点 schema（文件夹秒出）→ 点 Tables（L1 命中）→ 点表展开列（已预热）——**对齐甚至优于 DBeaver 首启体验**（DBeaver 重启后需重新加载；我们有 L2 跨会话持久，重启仍命中）。
- **与 DBeaver 的差异（本轮收敛后）**：① 展开 schema 后 ~0.6-0.8s 预热窗口内点 Tables 会实时回源（极短窗口，用户浏览时预热已完成）；② 大 schema（>8 表）不预热列，点表首载实时回源、L2 后续命中（后续可做「一次连接批量查全部表列」驱动接口）；③ 网络链路冻结重建成本（300-600ms 环境固有，100ms 探活已是最优识别手段）。
- **遗留**：数据预览（execute_query）仍走 scoped 连接复用，遇故障窗口有概率 15s 超时（有兜底，证据待收集）。

## 20. 批量列内省 + 旧缓存 schema 补列（提交 `6934b3d`）

### 20.1 问题：§19 收官的列预热受 8 表阈值限制，大 schema 点表仍实时回源

§19 的列预热按「表+视图 ≤ 8」阈值跳过（每表一次 `list_columns` = 每次新建连接，public 24 表 = 24 次建连 7s+ 不可接受）；且**旧缓存 schema**（对象完整标记已打、列从未预热，如 8 表阈值时代展开过的大 schema）被 `is_schema_complete` 跳过，永不补列。

### 20.2 落地改造（提交 `6934b3d`，四类驱动统一生效）

| 文件 | 改动 |
| --- | --- |
| `driver/traits.rs` | 新增 `Database::list_columns_batch`（默认实现循环 `list_columns`——本地驱动无建连成本直接可用） |
| `driver/native/postgres.rs` / `mysql.rs` | 覆盖批量列：**复用常驻连接 + 循环列 SQL**（`lock_pg_conn`/`lock_mysql_conn` 内置 100ms 快速探活）——大 schema 一次建连全部列；`get_table_detail` 与批量列共用 `pg_rows_to_columns`/`mysql_rows_to_columns`（去重） |
| `database/cache.rs` | `NavCache` 新增 `schema_columns_count`（列完整性独立判定）+ `schema_object_names`（L2 读表/视图名） |
| `database/navigator_service.rs` | `warm_schema` 重构：对象完整与列完整**分开记**——对象完整但列未预热时从 L2 读对象名补列（不重跑内省）；列预热全量化（去掉 8 表阈值） |

### 20.3 真机验证（app_run76 · clean-test 项目 · P_real_pg postgres/public 24 表）

| 项 | 结果 |
| --- | --- |
| 批量列预热（postgres/public） | ✅ `后台预热表列完成 warmed=24 targets=24`，预热总耗时 **270ms**（含建连，后台用户无感） |
| 点 Tables | ✅ **2ms（L1 命中）** |
| 点 purchase_order_item 表展开列 | ✅ **0ms（列已预热）** |
| 单元回归 | ✅ engine 528 / database 102 全绿 |

### 20.4 结论

- **大 schema 点表 0ms 全量化**：任何规模 schema 展开后点表展开列都是缓存命中，无实时回源等待——**体验目标达成**（DBeaver 大 schema 展开表节点仍要同步加载列，我们后台预热）。
- **多驱动口径保持**：批量列走 `Database` trait 统一接口；本地驱动默认循环（无建连成本），网络驱动覆盖复用连接——四类驱动统一享受预热。
- **遗留收敛**：仅剩 ① 预热窗口内（<300ms）点表实时回源；② 网络链路冻结重建（100ms 探活已最优）；③ 数据预览 execute_query 同故障窗口 15s 超时（有兜底，证据待收集）。





