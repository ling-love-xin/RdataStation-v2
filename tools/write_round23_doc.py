# -*- coding: utf-8 -*-
"""追加 Round 23 节到迁移文档（锚点替换）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round23 = '''### ✅ Round 23（已完成，M3 写路径打通 UI：新建连接表单，保存真实落库并刷新列表）

**目标**：把 M3 数据源连接的**写路径**接入工作台——用户通过「新建连接」表单（名称/驱动/URL/用户名/密码）创建连接，保存后真实写入全局系统库，连接列表即时刷新。至此 M3 的读（Round 21）→ 详情（Round 22）→ **写（本轮）**闭环完成。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/workspace_loader.rs` | 新增 `save_connection` / `save_connection_at(dir, ...)`（目录注入，便于测试）：conn_id 时间戳生成 → `GlobalDatabaseManager::save_global_connection` → 返回 Result；用户名/密码为空时存 None |
| `crates/workbench/src/panels.rs` | `EditorPanel` 新增表单状态（show_form + 5 个 `Entity<InputState>` 受控输入懒创建）；「新建连接」按钮改 toggle 表单；`Form::vertical` + `Field` + `Input` 渲染 5 字段；「保存连接」按钮：读值 → 校验非空 → `save_connection` → 成功刷新列表 + notice + 清空表单，失败 notice |
| `crates/workbench/tests/real_connections.rs` | 新增 `save_connection_at_then_load_roundtrip`：写路径保存 → 读回断言全字段（name/driver/host/port/database/联邦） |

**gpui 0.6 表单组件实证（本轮新 API 修正）**：
- **受控输入模型**：`Input::new(&Entity<InputState>)`，状态在面板字段持有；`InputState::new(window, cx)` 构造（需 window）；`state.read(cx).value()` 读（SharedString）、`state.update(app, \\|s, cx\\| s.set_value(v, window, cx))` 写（需 window——on_click 闭包第二参 `\\|_, window, app\\|` 可拿）。
- **E0502 陷阱**：`cx.theme()` 返回 `&Theme`（借用 cx 存活到借用者最后使用）；在其后调 `cx.new`（&mut cx）冲突 → **受控状态懒创建必须放在 `let theme = cx.theme();` 之前**。
- **嵌套 runtime 陷阱**：`save_connection_at` 内部 `Runtime::new()+block_on`，在 `#[tokio::test]` 内调用会 panic（Cannot start a runtime from within a runtime）→ 集成测试改用普通 `#[test]`（读写入口均同步）。
- `Button` 变体含 secondary（primary/ghost/link/text 等 9 种）；`Field::new()` 无参。

**验证**：workbench 12/12 测试通过（8 lib + 4 集成，含新写路径 roundtrip）；`cargo check --workspace` 零告警；`cargo build -p rds-app` + 运行验证进程稳定无 panic（表单懒建 + 详情渲染正常）。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round23 + marker)
p.write_text(t, encoding='utf-8')
print('Round 23 appended, total lines:', len(t.splitlines()))
