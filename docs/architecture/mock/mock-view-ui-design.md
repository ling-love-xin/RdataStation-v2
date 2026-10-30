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

## 七、第二轮收尾（P0–P3 一致性 + 中央 tab 设计调整，commit 待定）

在 M1–M4 合入后的审计基础上（入口接线 0 假入口；唯一已知未接线扩展点 = 批量「全部存草稿箱」固定 CSV，
panel/mod.rs `scratchpad_all` 硬编码 `MockExportFormat::Csv`，job.rs:46 注释承认"后续要支持 Parquet/Xlsx 再挂 dropdown"）。

### 7.1 改动清单（panel/render.rs + detail.rs + mod.rs 导入）

| 项 | 位置 | 改动 | 实证依据 |
| --- | --- | --- | --- |
| P0-1 | render.rs `render_table_list` 状态图例行 | 旧字形「○ 未生成 ◐ 生成中 ● 已生成 ✓ 已落库 ⚠」→ `render_status_cell` 图标化图例（Check/CircleCheck/Info/RefreshCw/CircleX + 语义色，`flex_wrap`） | M3 同款；图例行与清单行状态点同源 |
| P1-1 | 9 处下拉按钮 | 去手拼「▾」字形 → `.dropdown_caret(true)`（locale / 场景模板 / 行数 / 类型 / 生成器 / 追加 / 草稿箱 / 另存为 / 导入连接） | vendor `button.rs:804-807`：`dropdown_caret` 渲染组件 `Caret`（has_content 时 `justify_between`，右置 opacity 0.75 箭头）；与 `dropdown_menu` 独立字段 |
| P1-2 | render.rs 出口区 | 「持久化到项目分析库」改 `primary`（主出口）；项目级（落库/追加/草稿箱）与文件级（另存为）之间插 `Separator::horizontal()` | vendor `separator.rs:40`；`ButtonVariants::primary`（button.rs:52） |
| P2-2 | 模板 / 历史空态 | `EmptyMedia::new().with_variant(EmptyMediaVariant::Icon).child(Icon::new(Folder/Inbox).size(px(20.)))` | vendor `empty.rs:184 with_variant`、`EmptyMediaVariant::Icon`（155）；IconName 全集实证含 Folder/Inbox |
| P2-3 | 「＋ 加列」 | `.icon(IconName::Plus).label("加列")`（去「＋」字符，与工具栏统一） | IconName::Plus 已在工具栏使用 |
| P2-4 | 场景表行 / 关系行「✕」 | `.icon(IconName::Close)` + tooltip（去「✕」字符） | IconName 全集含 Close（`icons/close.svg`） |
| P3-1 | render.rs 工具栏 doc 注释 | 「＋ 新增 Mock 表 / ⇄ 引用结构 / ▦ 新增场景 / ⟳ 刷新」→ 图标名标注 | 注释与实现同步 |
| C-1 | detail.rs 字段卡片「智能」按钮 | 加 `.tooltip("按列名与类型重跑智能映射…")` | 按钮语义说明（窄面板无法放长 label） |

### 7.2 撤销项（实证后不做）

- **`status_chip_text` 的「◐」前缀保留**：tests.rs:1119/1125 断言 `"◐ Mock 生成中 25%"`——状态栏 chip 前缀是测试锁定的既有契约，非手拼箭头，改动无增益。
- **字段卡片 high/manual/low 徽标不图标化**：置信度不是「状态」；图标化会与清单行状态点（Check/CircleCheck…）混淆信息类型。保持语义色文字（已统一 text_xs + 语义色）。

### 7.3 有意不动（设计判断，记录在案）

- 字段卡片删除按钮保持 ghost 文字（一行 4 个操作按钮，danger 实色会喧宾夺主；危险确认语义在编辑对话框内）。
- 生成按钮不加图标：IconName 0.8.1 全集（101 个，default-icons.txt）**无** Wand/Sparkles/Play 语义贴切的"生成"图标（Play 是播放语义，不硬凑）。
- 场景空态卡片（detail.rs render 的「还没有列」引导卡）保持卡片形态：它是局部起手引导（按钮行 + 说明），`Empty` 适合整页空态，不迁移。
- 引用徽标「↳ 引用 x.y」、关系子行「↳ …」是文本标注（非按钮），保持字符。
- 列编辑对话框 `form_line` 不迁 `form::{Form, Field}`：form_line 已是 4 个对话框共用的统一函数，视觉一致；Form API 迁移收益有限、风险中等，留作后续项。

### 7.4 测试

`cargo test -p rds-mock --all-targets -j 2`（worktree 内；需手动建 `.rds/tmp`、从主仓复制 `third_party/duckdb/1.5.5/*`）。
tests.rs 无「状态：○」「＋ 加列」「✕」「▾」文案断言（grep 实证），P0–P3 不破测试；仅 `status_chip_text` ◐ 断言原样保留。

## 八、第三轮：字段管理增强（P0–P2，排除批量；commit 待定）

用户拍板范围：P0–P2 建议清单（**排除多列批量编辑**——用户判定无真实场景，撤销）；
用户提出的「数据类型统一设置管理（字段预设）」本轮**未实现、记录在册**（见 §8.3）。

### 8.1 改动清单

| 项 | 位置 | 改动 | 实证依据 |
| --- | --- | --- | --- |
| P0-1 | detail.rs `render_fields` 置信度徽标 | 手搓 div 文字 → `tag::Tag` outline（high=success / manual=info / low=secondary） | vendor `badge.rs` 实证：`Badge` 是**角标**（absolute 浮点 dot/计数/icon），不是行内文字标签 → 改用 `tag::Tag`（行内 chip，TagVariant 六色） |
| P0-2 | detail.rs `render_fields` 参数摘要 | ellipsis 截断 div → `HoverCard` 悬停显示完整参数（生成器 + 摘要全文本）；样式走 `trigger_style`（flex_1） | vendor `hover_card.rs`：`trigger_style` 承载布局（注释 76-77）；content 是 `Fn`（闭包内克隆捕获值） |
| P0-3 | detail.rs 列编辑对话框 | 「类型」行下加说明：改类型不自动换生成器，按新类型重推点「恢复智能默认」 | 消除「改类型 ≠ 换生成器」的语义困惑（生成器只读展示的既有取舍） |
| P1-1 | panel/mod.rs + detail.rs | `move_column(id, up)`（越界 no-op）+ 字段卡片 ↑/↓ icon 按钮（首/末列 disabled，tooltip 说明列序=生成顺序） | IconName::ArrowUp/ArrowDown 在 gpui-kit-assets-0.8.1 default-icons.txt |
| P1-2 | panel/mod.rs + detail.rs | `duplicate_column(id)`（深拷贝 + 新 id + 「副本」后缀，插原列后）+ 复制 icon 按钮 | IconName::Copy 在 default-icons.txt |
| P2 | detail.rs 列编辑对话框 | 参数实时样例预览：`ColumnDraft.sample_preview` + `refresh_sample_preview`（`generate_cell` 同步生成 2 条，固定种子 `SAMPLE_PREVIEW_SEED`）；参数 Change 订阅里刷新（事件路径，render 零 I/O） | `generators::generate_cell(config, rng, row, locale)`（pub(super)=crate 内可见，同步单值）；参数已由 `patch_param` 实时写回 generator，预览直接读它 |

### 8.2 撤销 / 有意不动

- **多列批量编辑**：用户判定无真实场景，撤销（当时设想「导入真实表结构后批量调整」不成立——列配置是逐列精细调整的主场景）。
- `form_line → form::{Form, Field}`：维持可缓（§7.3 同款理由：已 4 对话框共用统一，收益有限）。
- 样例预览固定种子：`SAMPLE_PREVIEW_SEED = 20261030`，样例可复现；与最终生成随机种子无关，纯示意（界面无「与最终一致」承诺）。

### 8.3 字段预设（Field Preset）——已实现内建集 + 入口；用户自定义待后续

用户提出「电话、地址、姓名这种数据类型需要统一设置的管理」。实证结论：现有三种粒度均不覆盖单字段打包预设：

| 现有机制 | 粒度 | 覆盖 |
| --- | --- | --- |
| generator_catalog（143 项，含 phone_number/email/city/address 分类） | 生成器 | 只含生成逻辑，不含类型/空值率/唯一打包 |
| schema_map::ColumnMapper::infer | 规则 | 列名+类型→生成器（代码内建，不可自定义，不推空值率/唯一） |
| 场景模板 / MockUserTemplate | 整表 | 多列打包，无单字段粒度 |

**已实现（本轮）**：`mock_view/presets.rs` —— 字段预设 = 列名 + ColumnDataType + GeneratorConfig + nullable_ratio + unique + confidence 一行打包。
- 内建集 12 项五分组（全部由 generator_catalog 已有生成器打包，不引入新生成器）：
  - 常用：日期 `created_at`（Date/DateTime）、布尔 `is_active`（Boolean/ratio 50）；
  - 人物：姓名 `name`（Varchar 50/Name/空 5%）、电话 `phone`（Varchar 30/PhoneNumber/空 10%）、邮箱 `email`（Varchar 100/Email/空 5%）、用户名 `username`（Varchar 50/Username/**唯一**）；
  - 地址：城市 `city`、街道 `street`（Varchar 100/StreetName）、邮编 `zip`（Varchar 20/ZipCode）；
  - 业务：公司 `company`（Varchar 100/CompanyName/空 5%）、金额 `price`（Decimal 10,2/RandomDecimal 1–100k）；
  - 技术：UUID `id`（Uuid/UuidV4/**唯一**）。
- 入口：列段按钮行「导入结构 | **预设** | 加列」三分——预设 = `Button` + `dropdown_caret` + 分组 `PopupMenu`（组标题 disabled 只读行 + 项 on_click）；点击经 `panel.add_preset_column(&FieldPreset, cx)` 加列。
- `add_preset_column`：列名冲突自动加序号（`name` → `name_2`），应用后是普通列（confidence=high），照常走列编辑对话框微调。
- 测试：`builtin_presets_are_unique_and_renderable`（id 唯一 + 生成器在 catalog 有规格）、`preset_column_adds_with_deduplicated_names`（重名去重 + 配置来自预设）。

**未实现（记录在册）**：用户自定义预设管理（增删改 + 持久化，复用 persistence 的 MockUserTemplate 落盘模式；菜单分组增「我的」）。依赖已就绪（generator_catalog / persistence / ui，无新 crate），待后续轮次。

## 九、第四轮：双视角生成器库 + 自定义生成器（commit 待定）

用户质询「列编辑填充面板只有数字吗，参数也很少」→ 实证：类型 13 种全（`COLUMN_TYPES`），但 143 个生成器 spec 里 **103 个（72%）零参数**（fake 生成器本身零参调用）；参数控件只有 Scalar 文本输入 + Complex 多行文本，**Bool 参数 13 个全退化成文本框**（8 个日期系工作日/时段开关 + image_url_custom grayscale + sequence cycle）。
用户随后给出双视角 + 自定义生成器构想，经原型确认后实施。

### 9.1 用户确认的信息架构（两级，双视角）

- **业务视角**：15 业务分类（`GeneratorCategory`）→ 生成器项（带**数据类徽标**）；
- **技术视角**：13 数据类型（`ColumnDataType`）→ 兼容生成器项；
- **一份生成器定义两个属性**：业务分类 + 产出数据类 → 两个视角各按自己的属性归组，**双向可见**；
- **自定义入口 = 分组上的「＋」**（业务组/技术组都有）：从技术组「＋」进入 → 类型**锁定**、业务组可选；从业务组「＋」进入 → 业务组**锁定**、类型可选；生成器名必填、基础生成器必选、参数区按基础生成器重建；
- **保存双向挂组**：永远挂数据类型组（类型是硬属性）+ 选了/锁定的业务组同时挂。

### 9.2 数据模型决策（关键约束）

**自定义生成器不是新的 `GeneratorConfig` 变体**——`spec_of` / `generate_cell` 是编译期穷尽 match（新增变体会编译失败强制补规格），运行时创造新算法不成立。自定义生成器 = **已有生成器 + 参数 + 名称的命名变体**：

```rust
pub struct UserGenerator {
    pub id: String,              // = 去重后的名称（name→name_2）
    pub name: String,            // 显示名（必填，重名自动加序号）
    pub data_type: ColumnDataType, // 产出数据类（硬属性，技术组必挂）
    pub category: Option<GeneratorCategory>, // 业务组（可选；选了双向挂）
    pub base: GeneratorConfig,   // 基础生成器 + 参数（应用时解包写回列）
}
```

**应用语义**：点自定义生成器 → `panel.apply_user_generator(id, ug_id)` 把 `base` 解包写回该列 `def.generator`（confidence=manual）——列模型不变，`generate_cell` 零改动。

### 9.3 UI 决策

| 决策点 | 结论 | 依据 |
| --- | --- | --- |
| 库对话框形态 | 新增「生成器库」对话框（`generator_library.rs`）：顶部搜索框 + `TabBar` 双视角（业务/技术）+ 分组行（`Button` 展开/收起 + 行尾「＋」`Button`，键盘可达）+ 展开的生成器项（选中写回列）；**精调层内嵌**（点「＋」切到创建表单，保存后回浏览） | 避免嵌套 `open_dialog`（gpui 对话框单实例）；分组行不手搓 div |
| 精调层下拉 | `select::{Select, SelectState}` + `SearchableVec`（3 个下拉：类型/业务组/基础生成器） | **不用 `Button`+`dropdown_menu`**——vendor `dropdown_button.rs:207` 触发器 id 硬编码 `"popup"`，同帧只允许一个 DropdownButton；`Select` 是常驻 inline 下拉，多实例无冲突 |
| Bool 参数 | 精调层参数区 Bool → `Switch::new(id).checked(..).on_click(..)`（13 个 Bool 参数不再退化成文本框） | 上轮质询的「全是数字框」观感来源之一；`switch.rs` API 实证 |
| 参数区重建 | 基础生成器 `Select` 变化 → 按 `spec_of(base).params` 重建输入行（Int/Float→`Input`，Text→`Input`，Bool→`Switch`，Complex→`Textarea`） | 与列编辑 `rebuild_params` 同构 |
| 生成器菜单接入 | `generator_menu` 顶部加「自定义」区（空则不显示）；项点击 `apply_user_generator` | 主路径（字段行菜单）直接可用；「搜索生成器…」对话框暂不含自定义（记录在册，List 是 `&'static GeneratorSpec`，自定义非静态） |
| 数据类映射 | `generators.rs` 新增 `data_type_for_generator(&GeneratorConfig) -> ColumnDataType`（穷尽分组 match，编译强制补） | 与 `generate_cell` 同文件同构；技术视角分组与徽标共用 |
| 持久化 | **会话级**：`MockPanel.user_generators: Vec<UserGenerator>`；SQLite 落盘（`store/migrations/project_meta/010_*.sql` + persistence 读写 + 面板后台加载）**记录在册** | store 迁移影响所有项目库 schema，属基础设施改动，宜单独轮；UI 全链路本轮交付 |

### 9.4 本轮改动清单

- `models.rs`：`UserGenerator` 结构 + `GeneratorCategory` 加 serde derive；
- `generators.rs`：`data_type_for_generator`（生成器→数据类，穷尽 match）；
- `mock_view/generator_library.rs`（新）：库对话框（双视角 + 分组「＋」 + 精调层内嵌 + 保存双向挂组 + 样例预览）；
- `mock_view/panel/mod.rs`：`user_generators` 状态 + `add_user_generator`（去重命名）/`update_user_generator`（编辑：改名去重排除自己 + 全量替换）/`delete_user_generator`（会话级删除，已应用列解耦）/`apply_user_generator`（解包 base）/`user_generators_in_category(category)` / `user_generators_in_type(data_type)` + `open_generator_library`；
- `mock_view/pure.rs`：`generator_menu` 加「自定义」区（签名增 `user_generators` 参数）；
- `mock_view/detail.rs`：`generator_menu` 调用点传自定义列表；
- **自定义生成器管理**（2026-10-30）：库对话框自定义项行尾「编辑」「删除」——编辑复用精调层（类型/业务组/基础生成器锁定，参数以**保存过的配置**为基准预填，名字可改 = 重命名；保存走 `update_user_generator`）；删除为会话级移除（不确认，重建成本低）；
- 测试：模型去重、数据类映射、菜单/库对话框接入、编辑改名去重、删除解耦。

### 9.5 遗留（记录在册，待后续）

- **SQLite 持久化**：自定义生成器跨会话落盘（store 迁移 010 + `persistence.rs` 读写 + 面板后台加载/保存）。
- **「搜索生成器」对话框并入自定义项**（`GeneratorSearchDelegate` 的 hits 是 `&'static GeneratorSpec`，自定义非静态；可改为「固定顶部区」渲染）。
- **字段预设「我的」分组**（§8.3 遗留）可与自定义生成器统一入口，待拍板。

