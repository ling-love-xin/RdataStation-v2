# -*- coding: utf-8 -*-
"""追加 Round 30 节到迁移文档（锚点替换）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round30 = '''### ✅ Round 30（已完成，连接切换联动：导航/查询不串数据）

**目标**：修复多连接场景的真实 UX 缺口——**切换连接后，导航树与 SQL 结果不得残留上一个连接的数据**（呼应 M1 双层数据架构"项目/连接彼此看不见"的产品主线）。

**现状探查**：
- 导航树（R25）已有 `nav_for` 校验：`nav_for != 当前连接 id` 时重新加载——切换连接会自动重载树 ✓；
- **缺口**：`query_result`（SQL 结果）无连接归属——切换连接后旧结果仍显示。

**改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/panels.rs` | Shared 加 `sql_for: Rc<RefCell<Option<String>>>`（SQL 结果归属连接 id）；SQL 区渲染前校验 `sql_for == 当前连接 id` 才显示结果，否则隐藏（切走即失效）；执行成功记录归属、失败清空 |
| `crates/workbench/src/view.rs` | `SidebarEvent::SelectConnection` 处理（唯一入口）加清理：清空 `nav_for` + `nav_tables` + `sql_for`——切换连接即时清场，导航树下一轮渲染重载 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **shared 闭包 move 链**：执行按钮闭包 move `shared` 后，渲染校验再 `shared.sql_for.borrow()` 报 E0382——沿用 R27/R29 模式，提前克隆 `shared_view`（渲染用）与 `shared`（闭包用）双副本；`conn_id` 同理提前克隆给闭包捕获。

**验证**：workbench 24/24 通过；**全仓 407 测试 0 失败**（服务层未动）；`cargo build -p rds-app` + 运行验证进程稳定。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round30 + marker)
p.write_text(t, encoding='utf-8')
print('Round 30 appended, total lines:', len(t.splitlines()))
