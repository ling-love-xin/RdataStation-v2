# -*- coding: utf-8 -*-
"""追加 Round 28 节到迁移文档（锚点替换）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round28 = '''### ✅ Round 28（已完成，M5 查询工作台第三步：多行 SQL 编辑器）

**目标**：SQL 查询区从单行 `Input` 升级为多行 `Textarea`（真实查询往往跨多行/多语句），并调整布局——编辑器占宽、执行按钮右对齐。

**探查结论（gpui-component 0.6 输入组件家族）**：
- `Textarea`（`gpui_kit::component::input`）存在：`Textarea::new(&Entity<TextareaState>)` + `.h(px())` 定高 + 继承 Input 样式链（bordered/appearance/disabled/readonly）。
- `TextareaState = InputBaseState<TextareaMode>`（gpui-base type alias），构造 `TextareaState::new(window, cx)`，读值 `value() -> SharedString`——与 `InputState` 同构，受控懒创建模式（R23 约定）直接复用。

**改动（仅 `crates/workbench/src/panels.rs`）**：
- import 增加 `Textarea, TextareaState`；
- `sql_input: Option<Entity<InputState>>` → `sql_textarea: Option<Entity<TextareaState>>`（struct 字段 / new 初始化 / 懒创建 / SQL 区取状态四处同步）；
- SQL 区布局：`h_flex(Input + 按钮)` → `v_flex(Textarea 高 96px + 右对齐执行按钮)`；
- 执行逻辑不变（`sql_state.read(app).value()` → `execute_sql`）。

**验证**：workbench 21/21 通过（服务层未动，测试数不变 404）；`cargo check` / `cargo build -p rds-app` 通过 + 运行验证进程稳定（多行编辑器渲染无 panic）。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round28 + marker)
p.write_text(t, encoding='utf-8')
print('Round 28 appended, total lines:', len(t.splitlines()))
