# UI 样式统一设计方案（2026-10-30）

> 范围：激活 `ui-todo.md` T-3（模块内部组件换 gpui-kit 现成组件），全应用 UI 样式统一。
> 基线规范：`gpui-kit-dev/SKILL.md`（组件选型 / 禁裸色 / 键盘可达 / 树行例外）、`theme-spec.md`（141 token）、`ui-design-spec.md`、`vscode-ui-design-system.md`。
> 状态：**设计已交付，待拍板后分批次实施**（默认只读，不改代码）。

---

## 1. 设计目标与统一基线

### 1.1 目标

| 目标 | 口径 |
| --- | --- |
| 零裸色值 | 视图层不再直接构造 `rgb()` / `hsla()` / `Hsla::` / hex；无 token 场景走登记豁免或补 token |
| 零手搓控件 | 控件 / 容器层一律标准 gpui-kit 组件；唯一例外 = 树行例外（复合行模型自绘） |
| 尺寸进常量 | 布局尺寸引用各 crate `ui.rs` 常量（`UI_*` / `NAV_*` / `PANEL_*` / `WS_*`），不写裸 `rems/px` 魔法数 |
| 键盘可达 | 可点元素优先语义 `Button`（自带 track_focus / tab_stop / KeyUp）；确需自绘必须 `track_focus` + `tab_stop(true)` + 稳定 `ElementId` |
| 主题适配 | 明暗双主题下字号不变、语义色跟随（连接状态 / 徽标 / 选中态同一套 token） |

### 1.2 统一基线（五条，实施时逐项对照）

1. **组件优先**：写控件前查组件清单（vendor `gpui-component/src/lib.rs` 69 模块）；容器层用 `Toolbar` / `Empty` / `Badge` / `Separator` / `Spinner` / `Skeleton` / `StatusBar` / `Notification` / `ResizablePanelGroup` / `CommandPalette`，禁止手搓 div 容器。
2. **颜色零裸值**：取色一律 `cx.theme().colors.<token>`（141 个）；`border` 不当文字色 / 大面积填充；状态文案按语义（success / danger / info）。
3. **尺寸进常量**：同一 crate 的尺寸常量收敛到 `ui.rs`（单一来源）；跨 crate 共享原语（`tree::indent_spacer` / `tree::disclosure_slot` / `ui::TREE_INDENT` 等）复用，不另造一套。
4. **键盘可达**：可点 = 语义 `Button`；自绘可点必须 `track_focus(&handle).tab_stop(true)` 且 `ElementId` 不随状态变化；滚动主体 `flex_1() + min_h_0()` + `overflow_y_scrollbar()`。
5. **树行例外**：导航树 / 草稿箱 / 列表这类复合行模型（staged 删除线 / 行内重命名 / 分段高亮 / 增量 diff-merge）超出 `tree::Tree` 边界，继续 `v_virtual_list` + 自绘行；**但行内控件（按钮 / 输入 / 徽标 / 折叠箭头）仍须标准组件**，折叠指示用 `tree::disclosure_slot()` / `tree::disclosure_icon()` 共享原语。

---

## 2. 现状证据矩阵（2026-10-30 实测）

### 2.1 已完成（合规，作为迁移先例）

| 模块 | 迁移内容 | 证据 |
| --- | --- | --- |
| database 导航视图 | 面板头按钮行 → `Toolbar::new("nav-header")`；空态 → `Empty` 全套；连接徽标 → `Badge` + `nav_badge_pulses` 呼吸 | `nav_view/chrome.rs:295/:1965-2027/:1601`、`rows.rs:16`、`primitives.rs:511` |
| insight | 全干净：裸色 0、`cursor_pointer` 0、`on_click` 全在标准组件链 | `insight/tests/ui_contract.rs` 守卫 |
| connection_dialog | TabBar / Switch / List 迁移（2026-09-20） | skill 已登记 |
| scratchpad 引用分组头 | 树行自绘 + 共享原语 + 行内标准 `Button` | `scratchpad_view/chrome.rs:754-777` |

### 2.2 反例清单（待迁移）

| 批次 | 位置 | 现状 | 迁移目标 |
| --- | --- | --- | --- |
| P0 | `database/nav_view/chrome.rs:1165` `nav-search-case` | 手搓开关（Aa，`rounded_sm + cursor_pointer + bg`） | `switch::Switch` |
| P0 | `database/nav_view/chrome.rs:1189` `nav-search-connected` | 手搓开关（"已连接"） | `switch::Switch` |
| P1 | `workbench/src/view.rs:1895` quick-open | 可点 div（有 `role(Button)` 但无 track_focus / tab_stop） | `Button`（ghost，含图标 + 快捷键 hint）或 `Input` 只读态 |
| P1 | `workbench/src/view.rs:2808-2824` 取消 chip | 可点 div（cursor_pointer + hover + on_click） | `Button::ghost().small()` |
| P1 | `workbench/src/view.rs:2827` `sb_toggle` 状态栏开关 | 手搓开关按钮（注释"对齐设计稿 .sb-btn"） | `Button`（`.toggled(bool)` + 图标 + 文案）或 `Switch` |
| P1 | `analytics_resource/dialogs/tag.rs:491` | 行内 `when(can_edit, \|row\| row.cursor_pointer())` | 行内 `Button::ghost()`（行例外内） |
| P1 | `content_search/src/view.rs:231` 文件行 | 可点 div 行（文件图标 + 名） | 行例外（`v_virtual_list` 自绘行）；行内控件检查 |
| P1 | `workbench/src/panels/metadata_search.rs:548` | `cursor_pointer`（待现场确认上下文） | 行内 `Button` 或登记例外 |
| P2 | `database/nav_view/rows.rs:745/774`、`chrome.rs:1224-1283/1575/1649/1682` | 状态点 / 计数手搓 `rounded_full + bg` | `badge::Badge`（`.dot()` / `.count(n)`）+ `nav_badge_pulses` |
| P2 | `workbench/panels/property_panel.rs:508`、`editor.rs:375`、`result_panel/sets.rs:126`、`editor/view/host/panel_traits.rs:131`、`workbench_shell/tree.rs:94` | 状态点 / 图形 `rounded_full` | `badge::Badge` 或图形自绘（登记） |
| P3 | `insight/src/view/set.rs:280` | 手搓容器候选（`div().h_flex().items_center().gap_1()`） | 现场核对：若面板头按钮行 → `Toolbar`；若普通行 → 保留 |
| P4 | `result_panel/src/panel/dialog.rs:31` | SQL 对话框遮罩裸 `hsla(0,0,0,0.45)`（注释：theme 无 scrim 字段） | **方案 A**：登记豁免（遮罩无对应 token，两主题通用）；**方案 B**：theme-spec 增补 `scrim` token 后替换 |

### 2.3 树行例外（合规，行内控件检查即可，不迁移行本体）

`database/nav_view/rows.rs`（10 处）、`scratchpad/rows.rs` / `chrome.rs`、`analytics_resource/resource_view/list_delegate.rs:178`、`mock/mock_view/panel/render.rs:475/775` —— 均属复合行模型自绘（允许），检查行内按钮 / 输入 / 徽标是否已用标准组件（目前抽查合规）。

---

## 3. 分批次迁移设计

### 3.1 批次与依赖

```
P0 手搓开关→Switch（database，2 处，无依赖）
  ↓
P1 可点 div→Button（workbench / analytics / content_search，5 处）
  ↓
P2 徽标/状态点→Badge（database / workbench / result_panel，7 处）
  ↓
P3 容器核对（insight set.rs 等，1 处）
  ↓
P4 遮罩豁免或补 token（result_panel，1 处）
```

各批独立可合并；P0→P4 由低风险 → 高风险排序，每批编译 + 测试闭环。

### 3.2 每批验收口径（统一）

1. `cargo check -p <crate> --all-targets` EXIT=0 零 error 零 warning（改动 crate）
2. 相关测试全绿：`cargo test -p <crate> -j 2 --target-dir D:\RdataStation\RDS\RdataStation-v2\target`
   - database 触碰后：`nav_view` 相关用例 + ui_contract
   - workbench 触碰后：`--test ui_contract` + 状态栏相关
3. `cargo fmt -p <crate> --check` exit 0
4. ui_contract 守卫扩展（见 §5）通过
5. 真机抽查：明暗主题切换无字号变化、键盘焦点序列（Tab）可达、hover 态不丢

### 3.3 关键迁移细节（防回归）

- **Aa / 已连接开关 → Switch**：T-2 已定"无边框 + 匹配开关入框内"只在侧边栏成立。`Switch::new(...)` 尺寸档 `xsmall`（对齐 `h_5` 行高）；开关状态来自 `nav.borrow()` 现有字段（`search_case_sensitive` / `search_connected_only`），**事件路径不变**（更新 Shared + `mark_panel_state_dirty` + `rerun_nav_search` + notify），只换渲染壳。
- **quick-open → Button**：现有 div 有 `role(Button)` + `test_support()`，迁移后 `Button` 自带 a11y；`Ctrl+P` 提示走 `.tooltip()` 或右侧 hint 槽；`id("quick-open-entry")` 保留（契约测试可能引用，先查 `ui_contract.rs` / tests 是否断言该 id）。
- **`sb_toggle` → Button.toggled**：状态栏开关是瞬态 toggle（按钮形态），语义上属 `Button`（`.toggled(bool)`）而非 `Switch`（表单开关）；保持图标 + 文案 + hover；键盘可达由 Button 自带。
- **徽标 → Badge**：`Badge::new().dot()` 用于连接状态点；`.count(n).max(n)` 用于待删除 / 预热计数；**颜色走 token**（`.color(hsla)` 参数取 `theme.colors.<语义>`，不造裸色）；呼吸动画挂 `with_animation`（`NAV_BADGE_PULSE_MS` 常量）——**只挂暂态**（Connecting），稳态不挂。
- **遮罩**：默认选方案 A（登记豁免，注释写清"theme 无 scrim 字段，深浅通用"），避免为 1 处加 token 牵动 theme 全量回归；若后续多处出现遮罩需求再补 `scrim` token（方案 B）。

---

## 4. 契约守卫扩展（ui_contract 加固）

在 `crates/insight/tests/ui_contract.rs` 与 `crates/workbench/tests/ui_contract.rs` 现有守卫基础上，新增两条源码契约（同"裸 px / 裸色守卫"模式）：

1. **无手搓开关**：视图源扫描——`cursor_pointer()` 与 `on_click` 不得出现在「开关语义」位置（人工清单：`nav-search-case` / `nav-search-connected` / `sb_toggle` 已迁移后从反例清单摘除；新代码触发用 `switch::Switch` / `Button::toggled`）。
2. **无裸遮罩色**：`hsla(` / `rgba(` 在视图层仅允许登记豁免路径（`result_panel/panel/dialog.rs` 遮罩）；其余命中即失败。

守卫实现参照既有 `every_view_module_is_registered_in_the_manifests`（显式路径清单 + `src.contains` 断言）。

---

## 5. 与在途工作的衔接

| 项 | 处理 |
| --- | --- |
| 工作区 12 个 M（editor bookmark/dialogs、result_panel cell、settings×5、workbench notification、workbench_shell result_style、vendor notification） | **不触碰**——属用户并行 UI 统一工作，本设计与其对齐但不覆盖 |
| `docs/architecture/editor/result-panel-handoff.md` | 已提交（b5e4048b），不涉及 |
| vendor `gpui-component/src/notification.rs` | 用户正在改；本设计 P 批次不涉 vendor |

---

## 6. 开放决定（待拍板）

1. **范围确认**：本设计默认覆盖"全应用 UI 样式统一（T-3 激活）"，批次 P0-P4 共约 16 处反例。若只想先统一某一模块（如结果面板 / 导航栏），可裁剪批次。
2. **遮罩方案**：P4 默认方案 A（登记豁免）；要补 `scrim` token 则改方案 B（需 theme-spec + ThemeColor 全量回归）。
3. **实施方式**：默认按 P0→P4 分批次实施（每批独立提交 + 测试）；也可一次合批实施（P0-P2 同批，P3-P4 同批）。
4. **`metadata_search.rs:548` / `set.rs:280`**：两处需实施时现场核对上下文再定（行例外 or 容器迁移）。

## 7. 交付物

- 本设计文档（`docs/architecture/ui/ui-style-unification-design.md`）
- 迁移路线可视化（HTML/SVG，见对话交付）
- 实施时：每批独立 commit + 验证日志（`.rds/tmp/`）
