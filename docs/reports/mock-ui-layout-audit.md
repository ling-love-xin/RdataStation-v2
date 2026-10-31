# Mock 视图 UI / 布局审查

审查对象：`crates/mock/src/mock_view/**`（panel/render.rs · detail.rs · generator_library.rs · ui.rs）
审查方式：**静态代码审查**（对照 `.agents/skills/gpui-kit-dev/SKILL.md` 与 vendor 0.8.1 组件事实），**未运行 UI**。
结论分级：🔴 结构性问题（有明确代码依据，建议修）／🟡 观感或一致性问题／🟢 已查证合规（不必动）。

> 运行时外观问题（具体错位像素、某个控件被裁之类）无法由静态审查确认，需要截图或实测才能定位。
> 本文列出的是「代码结构决定、必然发生」的那一类。

## 总体评价

先说结论：**这份视图代码的布局素养明显高于仓内平均水平**。
55 处 `h_flex` / 37 处 `min_w_0` + `min_h_0` 收缩保护、结构尺寸全部走 `ui.rs` 常量（且多数从
`workbench_shell::ui` 重导出以保持同源）、颜色全部 `cx.theme().colors.*`（零裸 hex / rgb / hsla）、
滚动容器一律 `size_full + flex_1 + min_h_0 + overflow_y_scrollbar` 四件套、容器层用 `toolbar::Toolbar`
而非手搓按钮行、空态用 `empty::Empty`。这些恰恰是 SKILL.md 里反复强调的陷阱，作者都避开了。

有意为之的两处「自绘可点行」（`panel/render.rs:521`、`panel/render.rs:845`）也**合规**：它们属于
SKILL.md 的「复合行模型例外」，并且齐备 `role(Button)` + `track_focus` + `tab_stop` + `test_support`
+ 显式 `on_key_down`（Enter/Space），hover 态也只对未选中行生效（`:578-582`），不构成回归。🟢

下面的问题因此不多，但每一条都有明确代码依据。

---

## 🔴 1. 详情标签页「列区」存在嵌套双层滚动，且拖到某个高度后拖不动

**位置**：`crates/mock/src/mock_view/detail.rs`

两段_HOST互不知情的高度约束叠加在同一块内容上：

- 外层 — 三段可拖分栏的第 2 段（`detail.rs:1955-1965`）：
  `resizable_panel().size(columns_h)`，`columns_h = detail_columns_rem * font_size`；
  `detail_columns_rem` 由 `on_resize` 回写（`:1977-1986`），**没有上限**，默认 15.0rem（=240px）。
- 内层 — 字段清单（`detail.rs:1525-1533`）：
  `.max_h(rems(ui::FIELD_LIST_MAX_HEIGHT))`（`FIELD_LIST_MAX_HEIGHT = 16.0` → 256px）
  **并且自己也挂了 `.overflow_y_scrollbar()`**。

外层给的是「用户拖出来的高度」，内层硬顶 256px：

| 拖到的高度 | 实际表现 |
| --- | --- |
| ≤ 256px（默认 240px 在此列） | 正常，内外同高，无异常 |
| > 256px | 分栏变高，内容停在 256px → **下方留白**；且内层滚动条与外层滚动条**同时存在** |

用户可见的两个症状：

1. **拖到约 16rem 之后继续拖，列区不再长高**（拖了没反应）。
2. **嵌套滚动**：字段清单内部滚到底后再滚，会接着滚动外层分栏容器——同一块内容两条滚动条，
   手势衔接是断的。SKILL.md 的滚动所有权条款要求「滚动条贴滚动区域边缘」、一个区域一条滚动通道。

**建议**（二选一，不要并存）：
- 若想让列区跟手长大：去掉 `max_h` 与内层的 `overflow_y_scrollbar`，高度完全交给外层分栏，
  滚动由外层那一个容器负责（`detail.rs:1962` 已有）；
- 若确实要限制列区高度：给第 2 段 `resizable_panel` 加 `.size_range(px(120.)..rems(ui::FIELD_LIST_MAX_HEIGHT))`，
  让拖动手感在边界上收住——用户一拖就到底，不会「拽了没用」。

## 🟡 2. 右 Dock 工具栏随内容一起滚走

**位置**：`crates/mock/src/mock_view/panel/render.rs:2158-2180`

根容器是一个纵向滚动区（`.id("mock-panel-scroll") … .overflow_y_scrollbar()`），
而工具栏在**滚动区内部**（`:2176` `.child(self.render_toolbar(window, cx))`）。

内容是「工具栏 + 集合动作 + 折叠区」，一旦折叠区展开（历史 20 条 + 用户模板），
面板内容超过一屏时向上滚，**工具栏和「生成 N 张表」按钮会一起滑出视野**。

常见期待是这一 DPX保持常驻（它就一行 36px，代价很小）。
**建议**：把 `render_toolbar` 提到滚动容器之外，做滚动区的一个兄弟节点：

```rust
div().v_flex().size_full().min_h_0()
    .child(self.render_toolbar(window, cx))   // 常驻，不参与滚动
    .child(div().id("mock-panel-scroll").flex_1().min_h_0().overflow_y_scrollbar()
        .child(div().v_flex().gap_2().w_full().child(actions).child(self.render_folds(cx))))
```

注：这与 SKILL.md 的规范不冲突（那条规范管的是「面板自己要有滚动主体」，本面板确实有）。

## 🟡 3. 工具栏按钮顺序与 README / 原型不一致

- 代码（`render.rs:1428-1442`）：`Toolbar::children([＋新增, ▦场景, ⟳刷新])`，
  然后 `.content(⇄引用结构)` —— `content` 槽渲染在 children **之后**，实际顺序为 **＋ ▦ ⟳ ⇄**。
- README §7 写的是「工具栏（**＋⇄▦⟳**）」。

⇄（引用数据库结构）被排到了最后一位。
另外注释（`:1349-1351`）说 ⇄ 因为被 `DropdownMenuPopover` 包装而无 `Sizable`、进不了 `Toolbar` 的
size 托管槽，所以走 `content` 槽——这是正确的处理，代价就是无法插入到 children 中间。

**建议**：二选一——要么改 README 使之一致（改动小），要么 UI 上调整（把 ⇄ 与 ▦ 在视觉上归到一处）。
两者不一致会让「按文档验收」的人反复确认。

## 🟡 4. 关系子行用 Unicode 字形 `↳`

**位置**：`render.rs:598` — `format!("↳ {} → {}.{}", …)`

SKILL.md 把「树展开字符」登记为技术债反例（sibling 树建议用 `tree::disclosure_icon()` 共享原语）。
这里是「关系指向」符号而非展开箭头，语义不同，所以**不构成同类回归**；
但字形渲染依赖字体，跨平台 / 换字体时基线与宽度会飘，且不能被主题着色。
若仑上有对标的图标，改用 `IconName` 更稳。优先级低。

---

## 🟢 已查证合规（不必动，列出以免重复审查）

| 项 | 结论 |
| --- | --- |
| 裸色值（`rgb` / `hsla` / hex） | 0 处。schema_map.rs 里的 `#ff5733` 等是**生成器示例值数据**，不是 UI 取色 |
| 手搓圆形徽标 | 0 处「rounded_full + bg」；状态点走 `render_status_cell` + `IconName` |
| 容器层组件 | 面板头用 `toolbar::Toolbar`（`:1429`）；折叠段用 `collapsible::Collapsible`；空态用 `empty::Empty` |
| 结构尺寸 | 全部来自 `ui.rs`，且与外壳同源的重导出 `workbench_shell::ui::*` |
| 详情标签页分栏 | 三段各自 `size_full + min_h_0`，第 1 段还给了 `size_range(100..220px)` ✓ |
| 对话框主区 | `h_flex + items_stretch`（`detail.rs:973`）——放两列时不会被上下裁 ✓ |
| 多标签页 id 冲突 | 根容器 / 分栏 / sinks / 预览菜单均拼 `tab_key`；草案 tab 唯一，所以其内固定 id 不会重复 ✓ |
| 单输入源 | 10 处 `Input::new` 逐一追溯，`render_search_box` 与 `render_browse` 分属两个对话框、**不同时挂树** ✓ |
| 自绘可点行的可达性 | 两处均带 `role` + `track_focus` + `tab_stop` + 键盘激活 + `test_support` ✓ |
| 表格列宽 | 预览表走组件库 `DataTable` 默认列宽 + 可拖宽，未把列宽钉死在 rem 上 ✓ |
