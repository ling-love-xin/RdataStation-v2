import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round19 = '''### ✅ Round 19（已完成，GPUI 视图层：首个可交互工作台骨架，`cargo check --workspace` 零告警）

**目标**：按 GPUI-kit 编码规范（https://gpui-kit.com/zh-CN/docs/coding-guides/）搭建 workbench 首个可交互视图——活动栏 + 侧边栏 + 内容区 + 状态栏，并接入 app shell（`cargo run -p rds-app` 即显示真实工作台）。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/view.rs`（17KB） | `WorkbenchView`：标题栏、活动栏四工具（连接/导航/资源/设置）、侧边栏按工具渲染（连接列表可选中高亮、导航树缩进占位、资源/设置占位）、中央内容区（连接详情 + "新建连接"按钮）、`StatusBar` 状态栏（工具 + 选中连接 + 通知文案）；`Tool` 枚举、`ConnectionItem`（首版 4 条占位连接） |
| `crates/workbench/src/lib.rs` | `pub mod view;` + re-export `ConnectionItem/Tool/WorkbenchView`；迁移进度注释补 Round 19 |
| `crates/workbench/Cargo.toml` | 新增 `gpui-kit.workspace = true` |
| `crates/app/src/main.rs` | 删除占位 `Workspace`，接入 `workbench::WorkbenchView`（App Shell 只组合窗口，业务在 Feature crate） |
| `crates/app/Cargo.toml` | 新增 `workbench = { path = "../workbench", package = "rds-workbench" }` |
| `tools/fix_round19_view.py` / `fix_round19_sidebar.py` | 本轮修复脚本归档 |

**编码规范落地要点**：窗口第一层 `Root::new(workspace, window, cx)`（dialog/sheet/notification/tooltip/menu/focus trap 统一协调）；`h_flex` 交叉轴居中而 `v_flex` 拉伸，行内放列必须 `items_stretch()` + `min_h_0()`；颜色全部取 `cx.theme().colors`（禁止 raw hex/hsla）；依赖只向下（视图只用 gpui-kit，不承载业务逻辑）。

**gpui-kit 0.6 真实 API 修正记录（初版 25 编译错误 → 0）**：

| 初版用法 | 0.6 真实 API | 说明 |
| --- | --- | --- |
| `Button::new(cx)` | `Button::new(id)`（`impl Into<ElementId>`） | 无 `new(cx)` 的 Button；id 内置元素（ElementId），不再需要 `.id()` |
| `.primary()`/`.ghost()` 直接调 | `ButtonVariants` trait 方法（需 `use button::ButtonVariants` 或 `button::*`） | variant 是私有字段，公开入口是 trait 的 `with_variant` 派生方法 |
| `cx.listener(\|this, _, cx\| ...)` 3 参 | `on_click(\|_, _, app\| ...)` 内 `entity.clone().update(app, \|this, cx\| ...)` | `on_click` 签名 `Fn(&ClickEvent, &mut Window, &mut App)`；`App::update` 私有，用 `Entity::update` |
| `.id(("a","b"))` 元组 | 字符串 `format!("activity-{}", ...)` | `ElementId: From<(&str,&str)>` 未实现 |
| `gap_0_5`/`py_0_5`/`w_60` | 显式 `gap_2`/`pt/pb/pl/pr` + `w(px(240.))` | gpui-pre 0.3 utility 数字步长有限（`0_5` 半单位不存在，`w_60` 无），统一用 px() 显式值 |
| `.when(...)` 不可用 | `use gpui_kit::prelude::FluentBuilder as _;` | FluentBuilder trait 需显式引入 |
| `div().on_click(...)` | `div().id("conn-N").on_click(...)` | `on_click` 在 Stateful（InteractiveElement 需先 `.id()`） |
| `Toast`（notification） | 首版用 `notice: Option<String>` 状态栏文案 | notification::Toast 的导出路径与构造链未实证，不冒险引入 |
| `app.update(entity, ...)` | `entity.update(app, ...)` | `App::update` 私有 |
| `let theme = cx.theme();` 后 `cx.entity()` | 颜色字段先拷贝为 `Hsla` 局部值（E0502） | `Hsla: Copy`，拷贝后借用立即结束 |

**验证**：`cargo check -p rds-workbench` 25 错 → 0；`cargo check --workspace -j 2` 零告警；`cargo build -p rds-app` + 运行验证工作台弹窗（详见交付说明）。DockArea 可拖拽布局系统与命令面板（Action/Keybinding）留待下一轮。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, marker + round19)
p.write_text(t, encoding='utf-8')
print('Round 19 appended, new total lines:', len(t.splitlines()))
