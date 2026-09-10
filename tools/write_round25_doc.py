# -*- coding: utf-8 -*-
"""追加 Round 25 节到迁移文档（锚点替换）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round25 = '''### ✅ Round 25（已完成，M4 数据库导航第一步：DuckDB 分析库元数据树真实接入工作台）

**目标**：启动 M4 数据库导航——选中连接（DuckDB 联邦开启）时，读取分析引擎库真实元数据，渲染 表 → 列 导航树。这是 DBeaver 式导航的最小闭环：**不依赖网络**（本地 DuckDB 文件），元数据来自 engine 原生驱动（`DuckDbDatabase`，Database trait 的 `list_tables` / `list_columns`）。

**探查结论**：
- engine `DuckDbDatabase::new(url)`（`duckdb://` 前缀或裸路径）同步打开文件，实现完整 `Database` trait（list_catalogs="main" / list_tables / list_columns / list_indexes…），可独立于 ConnectionManager 使用——**导航数据源首选**。
- engine 已 pub 导出 `Database` trait；`driver::native::duckdb::DuckDbDatabase` 经 `pub mod native → pub mod duckdb` 可达（无需新增导出）。
- gpui-component 0.6 `DockArea` **无 dump_state/load_state**（布局持久化 API 缺失）——本轮放弃布局持久化，改做导航。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/db_navigator.rs` | 新增 `load_navigator_tree(duckdb_path) -> Result<Vec<NavTable>, String>`：DuckDbDatabase::new → list_tables("main") → 每表 list_columns → NavTable{name, columns[NavColumn{name, data_type, is_primary_key, is_nullable}]} |
| `crates/workbench/src/services/mod.rs` | 注册 `db_navigator` 模块 |
| `crates/workbench/src/panels.rs` | `Shared` 加导航缓存（`nav_for` 记录已加载连接 id + `nav_tables` 树）；详情卡片下「数据库导航（DuckDB 分析库）」区：选中联邦连接按需加载（nav_for 变更时重新加载，避免每帧重查）；渲染表名 + 列数 → 每列（列名/类型/PK 标记）；空库显示"运行 seed_demo 或导入数据" |
| `crates/workbench/examples/seed_demo.rs` | 扩展：global.duckdb 幂等建 3 张演示表（orders/order_items/customers）+ 视图 v_order_summary（`CREATE TABLE IF NOT EXISTS`），重跑不报错 |
| `crates/workbench/tests/db_navigator.rs` | 新增 2 集成测试：建表后读回树（表名排序断言 + 列断言）、空库返回空 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **DuckDB 文件锁（Windows）**：同一 .duckdb 文件仅 1 个连接句柄（R18 已知）——测试里建表的 `Connection` **必须显式 drop** 后再 `load_navigator_tree`，否则 "另一个程序正在使用此文件"（os error 32）。
- **DuckDB 无 PK 元数据语义**：`CREATE TABLE ... PRIMARY KEY` 后 `list_columns` 的 `is_primary_key` 可能为 false（DuckDB 约束元数据有限）——测试只断言列存在。
- **E0382 seed move**：`duckdb`（PathBuf 非 Copy）被 async 块 move 后又借用 → 在 block_on 前先 `to_string_lossy().to_string()` 存 String，async 块内用 `duckdb.clone()`。
- **导航加载位置**：render 内同步 block_on（小库 <100ms），用 `nav_for`（连接 id）做缓存 key——选中切换重新加载，同连接重绘不重查。

**验证**：workbench 15/15 通过（8 lib + 2 navigator + 5 real_connections）；**全仓 398 测试 0 失败**（396 → 398）；seed 重跑成功（`C:\\Users\\sling\\AppData\\Roaming\\rdata-station\\global\\global.duckdb` 已建 3 表 + 1 视图）；`cargo build -p rds-app` + 运行验证进程稳定（选中 demo 连接自动加载导航树无 panic）。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round25 + marker)
p.write_text(t, encoding='utf-8')
print('Round 25 appended, total lines:', len(t.splitlines()))
