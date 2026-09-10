# -*- coding: utf-8 -*-
"""追加 Round 29 节到迁移文档（锚点替换）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round29 = '''### ✅ Round 29（已完成，M5 查询工作台第四步：SQL 历史记录，跨会话持久化）

**目标**：查询工作台记忆最近执行的 SQL（上限 20 条、去重、最新在前），跨会话持久化到全局目录；点击历史条目回填到多行编辑器，一键重放。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/query_history.rs` | 新增 `load_history_from(dir)` / `append_history_at(dir, sql)`（**目录可注入**，便于测试与未来多项目隔离）+ 默认目录变体 `load_history()` / `append_history()`：JSON 数组持久化到 `<全局目录>/query_history.json`，去重（相同 SQL 移到最前）→ 插入头部 → 截断 20 条；文件缺失/损坏 → 空历史 |
| `crates/workbench/src/services/mod.rs` | 注册 `query_history` 模块 |
| `crates/workbench/src/panels.rs` | `sql_history: Rc<RefCell<Vec<String>>>` 字段（new() 时 `load_history()` 初始化）；执行成功后在 `entity.update` 内 `append_history(&sql)` 刷新；SQL 区「历史」小节——条目（42 字符截断预览）可点击回填 `set_value` 到 Textarea |
| `crates/workbench/tests/query_history.rs` | 新增 3 集成测试（临时目录注入）：去重 + 持久化读回、上限 20 最新在前、缺失文件空历史 + 空白 SQL 忽略 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **set_value 泛型推断**：`s.set_value(sql.into(), ...)` 的 `.into()` 触发 E0283——`impl Into<SharedString>` 直接传 `String` 即可（`String: Into<SharedString>`），无需 `.into()`。
- **E0382 链式 move**：执行按钮闭包 move `sql_state` 后，历史按钮再 clone 报 borrow of moved value——**沿用 R27 模式**：在闭包捕获前提前克隆 `sql_state_hist`。
- **测试注入目录**：历史读写若硬编码全局目录会污染真实种子数据——用 `*_at(dir)` 参数化变体 + 临时目录测试。

**验证**：workbench 24/24 通过（8 lib + 2 navigator + 3 query_runner + 3 query_export + **3 query_history** + 5 real_connections）；**全仓 407 测试 0 失败**（404 → 407）；`cargo build -p rds-app` + 运行验证进程稳定（历史列表渲染/回填无 panic）。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round29 + marker)
p.write_text(t, encoding='utf-8')
print('Round 29 appended, total lines:', len(t.splitlines()))
