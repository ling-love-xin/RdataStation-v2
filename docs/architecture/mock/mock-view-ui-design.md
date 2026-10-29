# Mock 视图 UI 统一设计与组件化改造方案

> 状态：**设计稿（待验收）**——本方案只审查与设计，未改任何代码。
> 审查基线：本地 `main`（`521b3069`，mock_view 拆分合入后）`crates/mock/src/mock_view/` 9 文件。
> 规范依据：`.agents/skills/gpui-kit-dev/SKILL.md`（组件选型禁手搓 / 主题取色零裸色 / 键盘可达 / 容器层组件标准件 / 0.8.1 组件清单）。

## 一、审查范围与基线

| 文件 | 行数 | 职责 | 涉及组件 |
| --- | ---: | --- | --- |
| `panel/render.rs` | 1923 | MockPanel 渲染（工具栏 / 表清单 / 折叠段 / 搜索） | Button / Progress / Collapsible / IconName / PopupMenu |
| `panel/mod.rs` | 2526 | MockPanel 状态与事件（对话框 / 任务状态机驱动） | Dialog / DialogFooter / Switch |
| `detail.rs` | 1743 | MockDetailView（列配置 / 预览表 / 右键复制） | DataTable / TableState / TableDelegate / Switch / PopupMenu |
| `generator_search.rs` | 195 | 生成器搜索列表 | List / ListState / ListDelegate |
| `mod.rs` / `contract.rs` / `job.rs` / `pure.rs` / `tests.rs` | — | 入口 / 契约 / 任务状态 / 纯逻辑 / 测试 | — |

## 二、现状体检表

### ✅ 已合规（做得好的部分，不动的基线）

| 规范条目 | 实证 |
| --- | --- |
| 零裸色值 | 全模块 **0 处** `rgb()/hsla()/hex`，一律 `cx.theme().colors.<字段>`；状态文案语义取色（成功 `success` / 失败 `danger` / 提示 `warning` / 次要 `muted_foreground`） |
| 组件选型 | 62 处 `on_click` **全部落在** `Button / Switch / PopupMenuItem / DataTable 单元格 / 自绘可点行(合规例外)` 上，无 `div().on_click()` 手搓按钮 |
| 预览表 | `DataTable::new(&table)` + `TableState::new(PreviewTableDelegate)` + `impl TableDelegate`（detail.rs:1262/:226/:1479） |
| 生成器搜索 | `impl ListDelegate`（generator_search.rs:85），标准 `List` 委托 |
| 折叠段 | `Collapsible::new()` + `motion_id("mock-fold-reveal")`（panel_render.rs:1336/:1338），不手搓 `request_animation_frame` |
| 生成进度 | `Progress::new("mock-job-progress")`（panel_render.rs:259） |
| 对话框 | `window.open_dialog` + `DialogFooter::new()` ×4（panel_mod.rs:1380/:1531/:1880/:2430） |
| 键盘可达 | 自绘可点行（结果清单名 / 草稿名）齐备 `role(Button) + track_focus + tab_stop + Enter/Space 激活`，并注释说明为何不用 Button（内容容器 `justify_center` 破坏左对齐，panel_render.rs:445-448） |
| ElementId | 业务键不用下标：`mock-result-open-{表名}` / `mock-template-apply-{id}` / `mock-fold-reveal` |
| 尺寸常量 | 26 处 `ui::*`（`PANEL_HEADER_HEIGHT / PANEL_PADDING / ROW_HEIGHT / NUM_INPUT_WIDTH / CONTROL_HEIGHT_SM`），无散落魔法数 |
| 滚动所有权 | 主 render：`v_flex().size_full().min_h_0()` + `#mock-panel-scroll`（`flex_1().min_h_0().overflow_y_scrollbar()`），滚动挂内容主体（panel_render.rs:1963-1984） |
| render 纯读 | render 只读 Shared，I/O 走事件路径（`open_import_dialog` 在事件路径填充 `sources`） |
| 右键菜单 | DataTable 单元格 `on_mouse_down(Right)` 设 `context_cell` + `PopupMenu` 复制（detail.rs:1554/:1693+），表格标准模式 |

### ⚠️ 待统一（本方案的处理对象）

| # | 位置 | 现状 | 违规点 |
| --- | --- | --- | --- |
| **A** | `panel_render.rs:1256-1278` `render_toolbar` | 4 个图标 `Button`（Plus/HardDrive/LayoutDashboard/RefreshCw）装在手搓 `div().h_flex().gap_1().h(PANEL_HEADER_HEIGHT)` 里 | 面板头命令按钮行应迁 `toolbar::Toolbar`（M1 反例同款：`crates/database/src/nav_view/chrome.rs`）；丢 roving 方向键焦点与 toolbar a11y 语义 |
| **B** | `panel_render.rs:1387-1392` 模板空态 | 手搓 `div().text_xs().text_color(muted).child("还没有保存的模板…")` | 空态应迁 `empty::Empty`（自带居中布局 + 虚线框 + 图标变体） |
| **B** | `panel_render.rs:1521-1528` 历史空态 | 手搓 `div().text_xs().text_color(muted).child("本项目还没有生成记录")` | 同上 |
| **B** | `panel_render.rs:1519` 加载态 | 手搓 `div().text_xs().text_color(muted).child("读取中…")` | 不确定加载应迁 `spinner::Spinner` / `skeleton::Skeleton`（小段内可保留文字弱化版，见方案） |
| **C** | `panel_render.rs:430-441/:515` 结果清单状态徽标 | Unicode 字形（`✓ ● ⚠ ◐ ○`）+ 语义色文字经 `render_status_cell` | 状态点 / 图标徽标应迁 `badge::Badge`（`.dot() / .icon(IconName) / .color()`）；字形方案无 a11y 语义、样式与全仓徽标不统一 |
| **D** | `panel_render.rs:1287-1303` 折叠切换按钮 | `Button::ghost().xsmall().label("▾ 生成历史…")` 用 `▾/▸` 字符做箭头 | 折叠揭示箭头应走图标（`IconName::ChevronDown / ChevronRight`）而非字形字符 |

### ✋ 有意不动的项（设计判断）

| 位置 | 现状 | 不迁理由 |
| --- | --- | --- |
| `render_target_rows` / `render_generate_row`（panel_render.rs:6-116） | 表名/行数/种子/语言 **Input + Button 混合表单行** | 是表单工具行非纯命令按钮行，`Toolbar` 的 roving 焦点语义针对命令按钮；强迁会引入表单内 Tab 顺序混乱。保持 `div().h_flex().gap_2()` |
| 菜单内空态（panel_render.rs:299/:1191） | `PopupMenuItem::new("（暂无数据源）").disabled(true)` | 菜单场景无 `Empty` 组件，禁用项是标准做法 |
| `pending_relation_tables` 空态（panel_render.rs:900-902） | 条件空 `div()` | 无内容即不渲染，非空态场景 |
| 结果清单行 / 草稿行自绘可点（panel_render.rs:461-496/:764-790） | `div + track_focus + tab_stop + KeyDown 激活` | 规范允许的合规例外（左对齐 + 选中加粗 vs Button 内容容器居中），且已键盘可达；改 Button 是行为回归 |
| 工具栏底部边框（panel_render.rs:1263 `border_b_1`） | 手绘下边框 | `Toolbar` 规范原文「背景/边框/高度外层自己画」，此写法迁移后保留 |

## 三、统一设计原则

1. **语义优先**：同一 UI 角色全模块（进而全仓）只用 gpui-kit 同一组件——工具栏→`Toolbar`、空态→`Empty`、状态徽标→`Badge`、折叠箭头→图标。
2. **零新增手搓**：改造只替换容器/组件，不引入新的裸色、字形徽标或 `div().on_click()`。
3. **行为保真**：每个改造点保持现有交互与测试可观测性（`id` / `test_support` / `debug_selector` 不变或等价），262 项 rds-mock 测试是验收基线。
4. **键盘可达不回退**：`Toolbar` 自带 roving 焦点，迁移后键盘行为只增不减。

## 四、逐项改造方案

### A. 工具栏 → `toolbar::Toolbar`（panel_render.rs `render_toolbar`）

```rust
use gpui_kit::component::toolbar::{Toolbar, ToolbarGroup};

let mut tb = Toolbar::new("mock-toolbar").children([
    ToolbarGroup::new("mock-toolbar-actions").children([
        new_table.into_any_element(),
        import.into_any_element(),
        new_scene.into_any_element(),
        refresh.into_any_element(),
    ]),
]);
// 右侧清单状态提示走 `.content()` 槽；外层高度/边框照旧：
tb.content(div().flex_1().min_w_0().text_xs().text_color(muted).text_ellipsis().child(hint))
```

- 外层 `div().h(rems(ui::PANEL_HEADER_HEIGHT)).px(rems(ui::PANEL_PADDING)).border_b_1().border_color(cx.theme().colors.border)` 保留（规范：背景/边框/高度外层自己画）。
- 按钮本体（icon/ghost/tooltip/disabled/dropdown_menu）不动。
- 注意点：`Toolbar::new(id)` 的 id 用 `ElementId::Name` 或字符串字面量均可（0.8.1 `toolbar.rs` 实测）；`children` 接受 `AnyElement`；按钮行按钮已带各自 `ElementId`（`mock-toolbar-*`），无冲突。
- API 查证路径：vendor `gpui-component/src/toolbar.rs`（`Toolbar::new` / `ToolbarGroup::new` / `.content()` 槽位）；全仓先例见 connection_dialog 等已迁移点。

### B. 空态 / 加载态 → `empty::Empty` + `spinner::Spinner`（panel_render.rs 折叠段）

```rust
// 历史空态
section = section.child(
    Empty::new()
        .header(EmptyHeader::new()
            .media(EmptyMedia::new())
            .title("还没有生成记录")
            .description("跑一次生成后，记录会出现在这里"))
);
// 模板空态（保留引导动作 → `.content()` 槽放「保存当前配置」按钮）
section = section.child(
    Empty::new()
        .header(EmptyHeader::new().media(EmptyMedia::new())
            .title("还没有保存的模板")
            .description("把当前列配置存下来，下次一键套用"))
);
```

- 折叠段内空态：`Empty` 自带居中布局 + 虚线框，段落高度由内容自然撑起，不额外固定高度。
- 加载态：`Collapsible` 段内小行 `div().h_flex().items_center().gap_1().child(Spinner::new().small()).child(div().text_xs().text_color(muted).child("读取中…"))`——保留文字便于测试断言与弱网下的可读性，Spinner 提供进度感；若嫌重可仅换 `Skeleton` 行。
- API 查证路径：vendor `gpui-component/src/empty.rs` / `spinner.rs`；`Empty` 0.7.0 起存在（0.8.1 必然有，组件清单 §数据展示/反馈已登记）。

### C. 结果清单状态徽标 → `badge::Badge`（panel_render.rs `render_status_cell`）

```rust
// 以 `Badge` 替换字形 + 文字的 `render_status_cell`：
fn render_status_cell(status: &TableStatus, …) -> AnyElement {
    let (icon, color) = match status {
        TableStatus::Persisted    => (IconName::CheckCircle, success),
        TableStatus::NotPersisted => (IconName::Check, success),
        TableStatus::DanglingParent => (IconName::AlertCircle, warning),
        TableStatus::Generating   => (IconName::RefreshCw, primary),  // 百分比并入 label
        TableStatus::Failed       => (IconName::AlertCircle, danger),
        TableStatus::Idle         => (IconName::Circle, muted),
    };
    Badge::new().icon(icon).color(color).label(…)
}
```

- 生成中行的百分比（`◐ orders 40%`）在 `Badge::label` 中保留（`"生成中 40%"`），测试断言口径改为新 id/文本（tests.rs 有对应用例需同步）。
- 备选：若 `IconName` 无上述变体（以 vendor `icon.rs` 实测为准），退化为 `Badge::new().dot().color(color).label(status)`——仍统一为 `Badge` 形态。
- 注意：`Badge` 是胶囊形，行高 `ROW_HEIGHT` 内放小号 `Badge` 需确认尺寸档（`Sizable`）；若挤，可仅对**失败/生成中**两类用 Badge，常态保留文字（最小干预）。

### D. 折叠切换按钮箭头 → 图标（panel_render.rs:1287-1303）

```rust
Button::new("mock-fold-toggle")
    .ghost().xsmall()
    .icon(if open { IconName::ChevronDown } else { IconName::ChevronRight })
    .label(format!(" 生成历史（{}）· 用户模板（{}）", …))
```

- 去掉字形 `▾/▸`；图标随 `open` 切换（`Collapsible` 内容与 `open` 状态仍由实体字段驱动，逻辑不动）。
- API 查证：vendor `gpui-component/src/icon.rs` `IconName` 枚举确认 `ChevronDown/ChevronRight` 变体名。

## 五、分批实施计划（每批独立验证、可单独合并）

| 批次 | 内容 | 验证 | 风险 |
| --- | --- | --- | --- |
| **M1** | A 工具栏 → `Toolbar` | rds-mock 262 测试全过；`mock-toolbar-*` 元素 id 保留，测试不破；窗口内 Tab 顺序冒烟 | 低：纯容器替换 |
| **M2** | B 空态/加载态 → `Empty`/`Spinner` | 折叠段用例（历史/模板空态断言改为新 id + 文案） | 中：tests.rs 空态断言需同步更新 |
| **M3** | C 状态徽标 → `Badge` | 清单行用例（状态文本/百分比断言同步） | 中：断言口径变更 |
| **M4** | D 折叠箭头图标化 | 折叠用例（label 断言去字形） | 低 |

- 全批完成后：`cargo test -p rds-mock --all-targets -j 2`（main 检出下）；workbench 编译验证仍受 `rds-plugin 0xc0000409` 环境问题阻塞，以静态核验 + ui_contract 断言兜底。
- 若 M3 的 `Badge` 尺寸与行高冲突，降级为「仅失败/生成中用 Badge」并记录取舍。

## 六、遗留与边界

- 本方案只动 `crates/mock/src/mock_view/`；全仓其余模块（nav chrome 等 M1-M3 反例）不在本次范围。
- `render_target_rows` / `render_generate_row` 表单行有意不迁 `Toolbar`（理由见 §三）。
- 实施需在 **main 的拆分目录结构**下进行（主工作区为 analytics 分支原版文件，勿直接改；可用独立 worktree）。
