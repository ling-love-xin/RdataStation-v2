# -*- coding: utf-8 -*-
"""追加 Round 21 节到迁移文档（锚点替换，避免章节倒序）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round21 = '''### ✅ Round 21（已完成，M3 真实数据接入：连接列表由全局系统库填充，`cargo check --workspace` 零告警、全仓 0 失败）

**目标**：替换工作台连接列表的占位数据（`ConnectionItem::sample()`），改为从 M3 数据源连接模块的持久化层（`GlobalDatabaseManager` 全局系统库）读取真实连接元数据。这是「模块 → UI」打通的第一条真实数据链路。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/workspace_loader.rs`（新增） | 工作台真实数据加载器：`default_global_dir()`（`%APPDATA%\\rdata-station\\global`）+ `load_persisted_connections()` / `load_persisted_connections_from(dir)`（目录可注入，便于测试与后续数据目录切换）；tokio runtime + `GlobalDatabaseManager::new` + `get_global_connections` → 映射 `ConnectionItem`；失败降级为空列表 + 错误提示，不阻塞启动 |
| `crates/workbench/src/view.rs` | `WorkbenchView::new()` 启动时经 loader 加载真实连接 → `Shared::with_connections`；`ConnectionItem::sample()` 占位数据退役 |
| `crates/workbench/src/panels.rs` | `Shared::with_connections(connections, notice)`（连接为空时 `selected=None`）；连接列表空态 UI（暂无连接 / 加载失败提示） |
| `crates/workbench/tests/real_connections.rs`（新增，3 测试） | 真实接入链路集成测试：全局库 roundtrip（迁移 + 保存 + 读回 + 映射）、空库返回空列表、**重开库后连接持久化仍在**（模拟下次启动） |
| `crates/workbench/examples/seed_demo.rs`（新增） | 演示种子：`cargo run -p rds-workbench --example seed_demo` 向默认全局库写入一条演示连接 |
| `tools/patch_r21_view.py` / `tools/fix_r21_literal.py` / `tools/write_round21_doc.py` | 本轮修复与文档脚本归档 |

**验证**：
- 集成测试 3/3：`GlobalDatabaseManager::new`（含全局迁移链）+ `save_global_connection` + `get_global_connections` 全链路通过；重开库持久化验证通过。
- 全仓 `cargo test --workspace -j 2` 0 失败（新增 3 条后总计 394+ 单测/集成测试；engine 217 过 24 ignore 不变）。
- `cargo build -p rds-app` + 运行验证：进程稳定存活、无 panic、无 stderr。
- 端到端确认：`seed_demo` 写入 `%APPDATA%\\rdata-station\\global\\global.db`（`conn-demo-mysql / 演示 MySQL 分析库 / mysql / is_active=1 / use_duckdb_fed=1`），Python sqlite3 直读确认落库；app 启动加载该连接渲染。

**实测发现**：
- `save_global_connection` 保存后连接默认激活（`is_active=1`），视图 `connected` 状态如实映射。
- Windows 上 `GlobalDatabaseManager::new` 会创建 DuckDB 文件（`global.duckdb`）；同进程单连接句柄约束下测试用唯一临时目录隔离（与 Round 18 结论一致）。
- GPUI 同步渲染模型 + async 数据源：启动期一次性 `block_on`（本地 SQLite 查询毫秒级）接入；后续连接操作（新建/删除/切换）仍可挂命令面板或异步刷新，留待下一轮。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round21 + marker)
p.write_text(t, encoding='utf-8')
print('Round 21 appended, total lines:', len(t.splitlines()))
