# -*- coding: utf-8 -*-
"""追加 Round 27 节到迁移文档（锚点替换）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round27 = '''### ✅ Round 27（已完成，M5 查询工作台第二步：SQL 结果导出 CSV）

**目标**：查询工作台闭环再进一步——**查询 → 查看 → 导出**。结果集可一键落盘为 CSV（全局目录 `results/`，按秒时间戳命名），复用 R26 的 `QueryOutput`（列 + 已字符串化行）。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/query_export.rs` | 新增 `csv_field`（逗号/引号/换行加引号包裹、内部引号双写）+ `export_csv(output, path)`（列头 + 数据行写文件）+ `default_export_dir()`（`global/results/`）+ `export_to_default(output)`（秒时间戳命名 `rds_query_<secs>.csv`，返回落盘路径）——**手写转义，无第三方 csv 依赖** |
| `crates/workbench/src/services/mod.rs` | 注册 `query_export` 模块 |
| `crates/workbench/src/panels.rs` | SQL 结果表格后加「导出 CSV」ghost 按钮（行：`共 N 行` + 按钮）→ `export_to_default(&out)` → notice 显示导出路径；导出失败也提示 |
| `crates/workbench/tests/query_export.rs` | 新增 3 集成测试：逗号/引号/换行转义断言、导出默认目录落盘读回（列头 + 数据）、空结果仅列头 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **E0382 部分 move**：「执行」按钮闭包已 move `shared`/`entity`，后续导出按钮再 `clone()` 报 borrow of partially moved value——**在 R26 闭包捕获之前就克隆双副本**（`shared_export`/`entity_export`），各按钮各用一份，避免借用在闭包 move 之后发生。

**验证**：workbench 21/21 通过（8 lib + 2 navigator + 3 query_runner + **3 query_export** + 5 real_connections）；**全仓 404 测试 0 失败**（401 → 404）；`cargo build -p rds-app` + 运行验证进程稳定。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round27 + marker)
p.write_text(t, encoding='utf-8')
print('Round 27 appended, total lines:', len(t.splitlines()))
