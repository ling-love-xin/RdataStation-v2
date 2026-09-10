# -*- coding: utf-8 -*-
"""追加 Round 24 节到迁移文档（锚点替换）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round24 = '''### ✅ Round 24（已完成，M3 增删查写全闭环：删除连接，engine+loader+UI+测试四层打通）

**目标**：补上连接管理的**删除**能力——M3 数据源连接至此具备完整生命周期：读（R21）→ 详情（R22）→ 新建（R23）→ **删除（本轮）**。删除为物理删除（`DELETE FROM global_connections`），配合 `get_global_connections` 的 `WHERE is_active = 1` 过滤，删除后连接即从工作台列表消失。

**探查结论**：engine `GlobalDatabaseManager` 此前只有 `delete_environment/delete_plugin/delete_project` 等对象删除，**无连接删除方法**；`get_global_connections` SQL 带 `WHERE is_active = 1`（激活=可见）——删除与"隐藏"（is_active 置 0）是两种语义，本轮实现物理删除，doc 注释明确标注两种方案。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/engine/src/persistence/global_db.rs` | 新增 `delete_global_connection(conn_id)`：`sqlite_pool.acquire` → `conn.inner()?.execute("DELETE FROM global_connections WHERE id = ?1", [conn_id])` → `CoreError::storage` 包装 + `tracing::info`；对齐 `delete_environment` 模式 |
| `crates/workbench/src/services/workspace_loader.rs` | 新增 `delete_connection_at(dir, conn_id)`（目录注入，初始化失败/删除失败均转中文错误）与 `delete_connection(conn_id)`（默认全局目录） |
| `crates/workbench/src/panels.rs` | 详情卡片底部新增「删除连接」`danger` 按钮：取选中连接 id → `delete_connection` → 成功刷新列表 + 清空选中（`shared.selected.set(None)`）+ notice；失败 notice |
| `crates/workbench/tests/real_connections.rs` | 新增 `delete_connection_at_removes_from_list`：save → load 1 条 → delete → load 空 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **E0382 use of moved value: `entity`**：render 顶部 `let entity = cx.entity()`（非 Copy）被**删除按钮闭包 move 后**，**新建按钮闭包再 move** 冲突 → 详情块开头 `let entity = entity.clone();`（每个需要捕获的闭包用克隆）。
- **E0277 `?` 错误转换**：`runtime.block_on(async { ... .await? ... })` 的 async 块错误类型是 `CoreError`，与外层 `Result<(), String>` 不匹配 → 改 `match` 显式 `map_err` 转中文错误（load 路径本来就 match，写路径别用 `?` 穿透）。

**验证**：workbench 13/13 通过（8 lib + 5 集成，含新删除 roundtrip）；**全仓 396 测试 0 失败**（394 → 396）；`cargo build -p rds-app` + 运行验证进程稳定无 panic。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round24 + marker)
p.write_text(t, encoding='utf-8')
print('Round 24 appended, total lines:', len(t.splitlines()))
