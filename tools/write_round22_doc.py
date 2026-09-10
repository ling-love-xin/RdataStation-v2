# -*- coding: utf-8 -*-
"""追加 Round 22 节到迁移文档（锚点替换）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round22 = '''### ✅ Round 22（已完成，连接详情面板：选中连接展示真实元数据，全仓 394 测试 0 失败）

**目标**：把「选中连接 → 详情查看」闭环做实——EditorPanel 从「名称/驱动摘要」升级为**真实元数据详情卡片**（主机/端口/数据库/Schema/DuckDB 联邦/描述/时间戳等 10 项键值行），数据来自 Round 21 打通的全局系统库。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/view.rs` | `ConnectionItem` 扩展 8 个真实元数据字段：`host / port / database / schema / description / use_duckdb_fed / created_at / updated_at` |
| `crates/workbench/src/services/workspace_loader.rs` | 映射函数填充全部新字段（GlobalConnectionInfo → ConnectionItem 全字段对齐） |
| `crates/workbench/src/panels.rs` | `EditorPanel::render` 重写：连接详情卡片（状态徽标 + 10 行键值对 + 新建连接按钮），布局从居中改为顶部对齐流式 |
| `crates/workbench/tests/real_connections.rs` | 集成测试断言扩展：host/port/database/use_duckdb_fed/description 映射验证 |
| `tools/patch_r22_*.py`（4 个） | 本轮字段扩展 / 断言 / EditorPanel / 测试映射脚本归档 |

**验证**：
- 全仓 `cargo test --workspace -j 2`：**394 通过 0 失败**（25 个测试套件全 ok，含 workbench 8 单测 + 3 集成测试）。
- `cargo build -p rds-app` + 运行验证：进程稳定存活、无 panic、无 stderr（带 Round 21 种子连接 + 详情卡片渲染正常）。

**⚠️ 磁盘治理事件（本轮关键运维记录）**：
- **现象**：`cargo test` 连环报 `LNK1318 PDB LIMIT`、`LNK1180`、`E0462 staticlib std`、`E0463 can't find crate`、`os error 112 磁盘空间不足`——根因是 **D 盘 target/debug 膨胀至 125GB**（92646 文件，D 盘仅剩 6.67GB），链接产物损坏污染 deps。
- **处置**：`cargo clean` 释放 125GB（D 盘回 105GB）→ 全量重编译 28m36s → 测试 0 失败；当前 D 盘余 72GB。
- **预防**：后续轮次如再遇链接失败（LNK13xx/11xx 或 E0462/E0463），优先查磁盘余量；建议定期 `cargo clean` 或监控 target 大小（正常全量后约 24GB，异常增长至 125GB 前应干预）。
- 教训：`-j 2` 链接失败多为资源（磁盘/内存/PDB 竞争）而非代码错误；`cargo check --tests` 通过但 `cargo test` 失败时，先看链接器真实错误码与磁盘。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round22 + marker)
p.write_text(t, encoding='utf-8')
print('Round 22 appended, total lines:', len(t.splitlines()))
