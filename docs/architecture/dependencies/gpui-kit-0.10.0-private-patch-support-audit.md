# gpui-kit 0.10.0 私有 patch 上游符合性核查

日期：2026-11-24　分支：`main`　目标：判定 `vendor/gpui-base`、`vendor/gpui-component` 上的项目私有 patch
**「上游升级后的包是否已经支持」**，以及补丁当前是否真的在生效。

## 0. 结论摘要

1. **上游最新可获取版本就是 0.10.0，我们已经在 0.10.0**：本地镜像索引里 `gpui-base` / `gpui-component`
   的版本列表到 `0.10.0` 为止（无 0.10.1 / 0.11.x）。所以"升级的包"没有新版本可期待。
2. **0.10.0 发布包没有原生吸收我方任何一处补丁符号**：把我方独有符号在纯净 0.10.0 与 vendor 中逐个数，
   纯净包全部为 0 命中（`set_gutter_markers` / `GutterMarkerKind` / `GutterMarkerClick` / `collapse_cursors` /
   `select_columnar_block` / `take_last_edit_delta` / `TabReorder` / `on_reorder` / `register_syntax_context_provider` /
   `tab_bar_max_width` / `tab_bar_variant` / `Notification::timeout` 均是）。
3. **补丁仍全部在手，且绝大部分在生效**：vendor 内 16 个源文件、93 个 hunk、`+1373 −84`（含 2026-11-24 追加的
   dock 三层 chrome 4 处 + P3 tab 状态槽 2 处，见 §1 与 §2 A 末 5 行）都在；
   项目侧有实际调用点（下表 §3），只有 2 处属仅测试调用（另 1 处零调用已在本轮删除，见 §7）。
4. **唯一有"官方等价入口"的是自定义语法上下文**——但它是 **0.9.0 就有**的 `LanguageProvider` +
   `set_language_provider`，不是这次 0.10.0 升级带来的；而且完全替代有代价（见 §2 B-1）。
5. 多光标/列选择那组 public 包装里，**零调用的 `add_cursor_above_line` 本轮已删**；`add_cursor_below_line` /
   `select_columnar_block` 只被测试调用，**保留**（两者都没有等价公开入口，撤要以重写列编辑用例为代价，见 §2 C）。
6. **"`Tab` 右键菜单改用上游 `ContextMenuExt`"这条不可行**（§2 A 末行、§7）：`TabBar::children` 只接受
   `impl Into<Tab>`，`ContextMenu<Tab>` 既不是 `Tab` 也不是 `Into<Tab>`，套上就进不了标签条。
7. 本轮已做的一处瘦身：删掉零调用的 `add_cursor_above_line`（commit `cec060ac`，vendor diff 3+/7−）。

> 换句话说：**没有"升级后上游已经支持了、补丁可以整体退休"这回事**。能瘦身的只有少数几处，
> 主体（书签列 / 折叠说明 / 悬停说明 / 折叠候选守卫 / 标签拖拽重排 / dock 标签栏配置 / 标签右键菜单 / 通知时长）必须继续靠 vendor patch；
> 语法上下文虽可换官方入口，但要自己承接组件的别名与默认配置，工程上不划算。

## 1. 量化：补丁面实测

对比基准＝纯净 0.10.0 发布包 `.rds/tmp/crates1011/{gpui-base,gpui-component}-0.10.0`（与 crates.io 发布内容一致），
对比对象＝`vendor/`。行尾差异用 `--ignore-cr-at-eol` 排除。

| 文件 | +/− | hunk | 补丁内容要点 |
|---|---:|---:|---|
| `gpui-base/src/input/base/element.rs` | +527/−12 | 19 | gutter 书签列布局/绘制、折叠说明下划线、悬停说明气泡（300ms 延迟）、pin、点击上报、行号右移 |
| `gpui-base/src/input/base/state.rs` | +285/−8 | 17 | `GutterMarker{Kind}` / `set_gutter_markers` / `GutterMarkerClick` / pin / hover 计时 / 列开关；多光标与列选择 public API；`LineDelta` 编辑行增量；折叠候选空守卫（2 处）；核查时为 +289/−8，删 `add_cursor_above_line` 后 +285/−8 |
| `gpui-base/src/input/editor/display_map/display_map.rs` | +9/−0 | 1 | `has_fold_candidates()`（供空守卫判断） |
| `gpui-base/src/tabs.rs` | +86/−5 | 6 | `TabReorder` / `TabDragGhost` / `Tab::on_reorder`（拖拽重排 + 拖拽浮层） |
| `gpui-base/src/dock/dock_area.rs` | +31/−7 | 4 | 【2026-11-24 追加】`plan_tree` / `plan_node` 带 `DockPlacement`；中央组 `GroupChrome::Tabs`、dock 组 `Auto`（placement 级形态） |
| `gpui-base/src/dock/tab_group.rs` | +58/−0 | 6 | 【2026-11-24 追加】新增 `GroupChrome{Auto,Tabs}`；`TabGroupConstraints` 增 `placement` / `chrome` 字段与 `at()` / `with_chrome()` + getter；`TabGroupContext::placement()` / `chrome()` |
| `gpui-base/src/dock/mod.rs` | +1/−1 | 1 | 【2026-11-24 追加】re-export `GroupChrome` |
| `gpui-component/src/dock/mod.rs` | +26/−0 | 4 | `SkinShared` 新增 `tab_bar_max_width` / `tab_bar_variant` + `DockSkin::set_*` |
| `gpui-component/src/dock/panel.rs` | +17/−0 | 3 | 【2026-11-24 追加】`Panel::tab_prefix()`（tab 状态槽）+ 对象安全 `PanelView::tab_prefix()` + `Entity<T>` 空实现转发 |
| `gpui-component/src/dock/tab_panel.rs` | +177/−30 | 13 | 把上面两个值透传给 `TabBar`；dock 标签右键菜单（缩放/关闭/关闭其他/关闭全部）；【2026-11-24 追加】组头 32px + 1px 底边、标题行 hover 分组 + 单面板 ✕（= 收起该 dock，走 `toggle_dock_by_user`）、标签 ✕ 常驻占位 + hover 浮出（`dock-tab-{ix}` 分组）、`render_tab_bar` 改读 `group.chrome()`、`Tab::prefix` 接面板的 `tab_prefix()` |
| `gpui-component/src/input/input.rs` | +16/−17 | 1 | 代码编辑器左内边距 `min(6px)` → `0`（行号顶到最左） |
| `gpui-component/src/input/mod.rs` | +3/−0 | 1 | re-export `register_syntax_context_provider` / `SyntaxContextFactory` |
| `gpui-component/src/input/syntax_context.rs` | +41/−1 | 2 | 自定义语法上下文工厂注册表，优先于 tree-sitter |
| `gpui-component/src/notification.rs` | +12/−1 | 5 | `Notification::timeout(Duration)`（默认 5s） |
| `gpui-component/src/tab/tab.rs` | +65/−2 | 7 | `Tab::on_context_menu` / `Tab::tooltip` / `on_reorder` 字段与接线 |
| `gpui-component/src/tab/tab_bar.rs` | +19/−0 | 3 | `TabBar::on_reorder`（整条标签条下发到每个标签） |

合计 **16 个源文件 / 93 hunk / +1373 −84**。另有 2 个 `Cargo.toml.orig` 是发布包打包产物差异（vendor 不带），
不计入补丁（它们会在 `--stat` 里各显示一行 deletion，base 侧 95 行、component 侧 271 行，别误当补丁）。

> 2026-11-24 复核（删 `add_cursor_above_line` 之后）：hunk 仍是 70（`-- 2 个 Cargo.toml.orig` 后 base 44 + component 28 = 72），
> 行数变为 **+1161 −48**（`state.rs` 由 +289/−8 → +285/−8，净 −4 行）。
>
> 2026-11-24 追加（dock 三层 chrome）：`dock_area.rs` +31/−7（4 hunk）、`tab_group.rs` +58/−0（6 hunk）、
> `gpui-base/src/dock/mod.rs` +1/−1（1 hunk）、`tab_panel.rs` 由 +72/−2（4 hunk）长到 +168/−30（13 hunk）。
> 合计 **+1347 −84 / 15 文件 / 90 hunk**，口径同上（`git diff --no-index --ignore-cr-at-eol` vs 纯净 0.10.0 发布包）。
>
> 2026-11-24 追加（P3 tab 状态槽）：`gpui-component/src/dock/panel.rs` 由未改变为 +17/−0（3 hunk，`Panel::tab_prefix`
> 三处：trait 默认方法 / 对象安全 trait / `Entity<T>` 转发），`tab_panel.rs` 再 +9 行（`Tab::prefix` 接面板内容）。
> 合计 **+1373 −84 / 16 文件 / 93 hunk**。

## 2. 逐点判定

### A. 上游没有、必须保留（19 项）

| 补丁点 | 上游 0.10.0 实况（实测） | 判定 |
|---|---|---|
| gutter 书签列（字形 / 常显列 / 悬停淡书签） | `GutterDecoration` 只有 `range + color + folded_color` 的色块，**无图标、无点击、无说明** | 保留 |
| `InputEvent::GutterMarkerClick` | 纯净包 `InputEvent` 只有 `Change` / `PressEnter{..}` / `Focus` / `Blur` 四个变体，没有 gutter 点击 | 保留 |
| 折叠说明（下划线 + 非悬停常显 chevron） | 0.10.0 新增 `FoldPlaceholderRenderer` 只负责**折叠行占位内容**（`set_fold_placeholder_renderer`），与"说明"无关 | 保留（0.10.0 与它只是视觉地盘相邻，不能替代） |
| 悬停说明 300ms 延迟 + 跳转 pin | 无任何等价 API | 保留 |
| 折叠候选空守卫 + `has_fold_candidates` | 上游仍是「高亮器返回空就覆盖/merge 掉候选」；`apply_highlighter_fold_candidates` 是公开入口，但刷新路径会清空外供候选 | 保留（属上游行为缺口） |
| `collapse_cursors` / `cursor_count` / `cursor_offsets` | 无公开等价（`selections.remove_all_but_active` 仅内部使用） | 保留（书签、构造、状态栏都在用） |
| `LineDelta` / `take_last_edit_delta` | 无编辑行增量 API；上游只有 `adjust_folds_for_edit` 内部口径 | 保留（书签行号平移依赖） |
| `Tab` 拖拽重排（base `tabs.rs`） | 纯净 `tabs.rs` 无 `on_drag` / `on_drop` / reorder 任何代码 | 保留（结果集标签条在用） |
| dock 标签栏 `max_width` / `variant` 透传 | `TabBar` 本身有 `with_variant` / `max_width`，但 `DockSkin` **没有任何标签栏配置入口**（grep 0 命中） | 保留（属胶水，但无替代路径：dock 内部渲染，应用改不到） |
| dock 标签右键菜单 | dock 的标签由 `TabGroupSkin` 渲染，应用无法插手；`Tab` 组件自身没有 `on_context_menu` | 保留（只能靠 vendor 内补） |
| `Tab::tooltip` | 组件库有 `ComponentTooltip`，但给元素挂载的 `ManagedTooltipExt` 是 **`pub(crate)`**，外部无法给 `Tab` 挂 tooltip | 保留 |
| `Notification::timeout` | 上游写死 `Duration::from_secs(5)`；内层 `ToastOptions` 有 `timeout` 字段但 `Notification` 无 setter | 保留 |
| 代码编辑器左内边距 0 | 上游写死 `min(px(6.))`，`Input` 无 padding builder | 保留（纯视觉口径） |
| `Tab::on_context_menu`（**应用自建标签条**那半边） | 上游 `ContextMenuExt` 确实是 public（`menu/mod.rs:10`），`Tab` 也满足它的 bound；**但套上去就进不了标签条**：`TabBar::children` 签名是 `impl IntoIterator<Item = impl Into<Tab>>`（`vendor/gpui-component/src/tab/tab_bar.rs:178`），而 `ContextMenu<Tab>` 不是 `Tab`、也没有 `Into<Tab>` 通道（`ContextMenu<E>: IntoElement` 只在"能挂到任意元素上"这一层成立） | 保留（dock 内部同理；撤它需要给 `TabBar` 另开一条 `AnyElement` 子项入口＝再加补丁，或手搓标签条＝违规） |
| dock 组头形态按 **placement**（`GroupChrome` + `TabGroupConstraints.placement/chrome`） | `PanelStyle` 只是皮肤上的**全局一档**（`SkinShared.panel_style`，`dock/mod.rs:135`），没有 placement 维度；组约束原只有 `alone/dock_locked/collapsed/closable`（`tab_group.rs:61-67`） | 保留（否则"中央恒标签条 + 侧栏单面板标题栏"无法共存：`workspace.rs` 只能全局选一档，侧栏会被连带画成空标签条） |
| dock 组头 **32px + 1px 底边** | `render_title` 写死 `h(px(30.))`、无 `border_b`（0.10.0 `tab_panel.rs:394`，函数体 `:375-446`） | 保留（与标签条（`tab/tab_bar.rs:684-698` 自带 `border_b_1`、Default 32px）对齐，形态切换时高度不跳） |
| dock 标题行 hover 分组 + 单面板 ✕（= 收起该 dock） | `render_title` 未注册 hover 分组；`is_closable = constraints.is_closable() && draggable && panel.closable`，而 `is_last_panel → draggable = false`（`tab_group.rs:223/229/251`）⇒ 侧栏最后一个面板永远关不掉；`set_close_button_visible` 只有常显/常隐 | 保留（"整行 hover 才显 ✕"与"最后一个面板收起 dock"都无上游入口；`toggle_dock_by_user` 是上游 `pub(crate)`，只有在 vendor 内才能调到，它才会发 `DockToggleEvent`） |
| dock 标签 ✕ **常驻占位 + hover 浮出** | `Tab` 有 `prefix/suffix`（`tab/tab.rs:706/712`）与 `InteractiveElement`（`:788`），但"关闭按钮只在悬停时出现"没有开关（`close_button_visible` 只有常显/常隐） | 保留（Zed 的 `tabs.show_close_button = "hover"` 没有上游对应物；项目侧只能改结果集标签条，dock 内部渲染够不到） |
| `Panel::tab_prefix()`（tab 状态槽） | 组件层 `Panel` 的钩子只有 `tab_name` / `title` / `title_style` / `title_suffix` / `toolbar_buttons` / `dropdown_menu` / `zoom_control` / `inner_padding` / `title_bar`（`dock/panel.rs:71-149`）；`title_suffix` 只挂在**标题栏**右端，而 `Tab::prefix` 是皮肤独占（`tab_panel.rs` 自己 set），面板插不进去 | 保留（中央恒为标签条形态 ⇒ 标题栏不画 ⇒ 非激活的脏/失败标签零标记，属上游能力缺口） |

### B. 上游有等价入口/API，可评撤（1 项，附代价）

> **更正（2026-11-24 二次核查）**：本表原有 3 项，其中「`Tab::on_context_menu` 换 `ContextMenuExt`」一项
> **判定错误**——首轮只验了 `ContextMenu<E>: IntoElement`（`context_menu.rs:114`），没验 `TabBar::children`
> 的收口类型。`TabBar::children` 只吃 `impl Into<Tab>`（`tab_bar.rs:178`），`ContextMenu<Tab>` 进不去，
> 故该项已移到 §2 A（必须保留）。多光标包装一项也从"可撤"降级为"仅测试用，见 §2 C"。

| 补丁点 | 上游等价入口 | 撤的代价 |
|---|---|---|
| **自定义语法上下文注册表**（`input/mod.rs` re-export + `syntax_context.rs` 注册表） | `gpui_base::input::{LanguageProvider, set_language_provider}` —— **0.9.0 起就有**，`0.10.0` 只是继续沿用（两版都是 `1/1` 命中）。provider 的 `syntax_context_provider(name)` 正是我们要的钩子 | ① 全局**单** provider：组件 `init` 会装自己的 `ComponentLanguages`，应用再装会顶掉它，顺序敏感；② 组件的 `language_name()` 走 `LanguageRegistry::editing_language_name`，该方法 **`pub(crate)`**，别名归一（`pyi`→`python`、`JSON` 之类）拿不到，得自己维护；③ `text/json/python` 的默认 bracket / auto-closing / indent 配置也要自己重写一遍。→ **技术上可撤，工程上不划算**；若不撤，建议把这条补丁标成"上游有粗粒度入口、我们提供按语言细粒度注册" |

### C. 死代码 / 仅测试调用（本轮处置）

| 符号 | 项目内调用（2026-11-24 实测） | 处置 |
|---|---|---|
| `add_cursor_above_line` | **0 处**（生产与测试都没有；`crates/` + `vendor/` 全量 grep 只命中它自己的定义） | **已删**（commit `cec060ac`；`git show --stat` = 3+/7−；`cargo check -j 1 -p gpui-base` → `Finished`） |
| `add_cursor_below_line` | 1 处真实调用（`crates/editor/src/view/host/results/tests.rs:201`）+ 1 处注释引用（`crates/editor/src/view/tests/toolbar_custom.rs:243`），都在测试 | **保留**：撤它要把用例改走 `add_cursor_below(&AddCursorBelow, &mut Window, cx)`，为删 5 行去改两个测试文件不划算；已在源码注释里标明"测试用薄封装" |
| `select_columnar_block` | 5 处，全在测试，经 `panel.select_columnar_block_for_test(1, 28, cx)` 调用 | **保留**：它是 `build_columnar_selection`（`state.rs:2858`，`pub(super)`，入参 `pub(super) struct ColumnarPoint`，`state.rs:417`）对外的**唯一**出口；撤了只能在测试里模拟真实鼠标拖拽（`on_mouse_down`/`on_drag_move`）才能复现同一块选区 |

> 结论：C 类实际只清掉 1 项。另外两项不是"上游已支持"，而是**没有等价公开入口**，只服务于测试；
> 保留的价值（可读的用例）大于删掉省下的 5 行。

## 3. 补丁是否仍生效（调用点证据）

| 补丁点 | 项目内调用点 |
|---|---|
| `set_gutter_markers` | `crates/editor/src/view/host/bookmarks.rs` |
| `set_gutter_note_pin` | `bookmarks.rs`（2 处）+ `view/tests/bookmarks.rs` |
| `GutterMarkerClick` / `GutterMarker{Kind}` | `view/host/construct.rs`（事件订阅）+ `bookmarks.rs` |
| `set_gutter_marker_column` | `view/host/mod.rs`、`view/host/render.rs`、`settings/src/model/editor.rs` |
| `take_last_edit_delta` / `LineDelta` | `view/host/bookmarks.rs` + `support/bookmark.rs`（行号平移） |
| `collapse_cursors` | `bookmarks.rs`、`construct.rs` |
| `cursor_count` / `cursor_offsets` | 状态栏、会话、结果面板、构造等多处（27 / 7 处） |
| `TabBar::on_reorder` | `result_panel/src/sets.rs`、`panel/sidebars.rs`、`panel/render.rs` |
| `Tab::on_context_menu` | `result_panel/src/sets.rs`（应用侧标签条）+ `vendor/gpui-component/src/dock/tab_panel.rs`（dock 内部标签；这份补丁不可能整体撤） |
| `DockSkin::set_tab_bar_max_width` / `set_tab_bar_variant` | `crates/workbench/src/view/actions.rs` |
| `register_syntax_context_provider` | `editor/src/lsp/language_config.rs`、`editor/src/lsp/sql_syntax_context.rs` |
| `Notification::timeout` | `crates/notification/src/service.rs` |

配套证据：vendor 两个包版本都是 `0.10.0`，`Cargo.lock` 两个条目也是 `0.10.0`；
0.9.0→0.10.0 升级时三方合并的 `cargo check -p gpui-base`（15.39s）与 `-p gpui-component`（4m20s）结果记录在
`docs/architecture/dependencies/core-packages-2026-11-22-upgrade-assessment.md` §11；
2026-11-23 editor 门禁 864/0/4 即是在这层补丁之上跑出来的，说明补丁参与编译并被测试覆盖（书签、拖拽、
notebook 输入等用例）。

## 4. 0.9.0 → 0.10.0 差异里与我方补丁相关的部分

| 符号 | 0.9.0 | 0.10.0 | 说明 |
|---|---|---|---|
| `LanguageProvider` / `set_language_provider` | 1 文件 / 1、3 文件 / 6 命中 | 同 | **两版都有**；我方语法上下文补丁在 0.9.0 时本可改用该入口 |
| `FoldPlaceholderRenderer` | 0 | 4 文件 / 7 命中 | 0.10.0 新增，折叠行占位内容；与"折叠说明"无关 |
| `GutterDecoration` | 5 文件 / 53 命中 | 同 | 两版都只有色块，没有点击/图标/说明 |
| 其余我方独有符号 | 0 | 0 | `GutterMarker*`、`TabReorder`、`tab_bar_*`、`Notification::timeout` 等两版皆无 |

## 5. 复现方式

```powershell
# 纯净 0.10.0 / 0.9.0 发布包已解包在 .rds/tmp/crates1011/
git -c core.autocrlf=false --no-pager diff --no-index --ignore-cr-at-eol --stat -- `
  '.rds/tmp/crates1011/gpui-base-0.10.0' 'vendor/gpui-base'
git -c core.autocrlf=false --no-pager diff --no-index --ignore-cr-at-eol --stat -- `
  '.rds/tmp/crates1011/gpui-component-0.10.0' 'vendor/gpui-component'

# 符号计数（纯净 vs vendor；0.9.0 vs 0.10.0；项目调用点）
python .rds\tmp\probe_patch_support.py
python .rds\tmp\probe_090_vs_010.py
python .rds\tmp\probe_patch_usage.py

# 完整 diff 落盘
# .rds/tmp/kit_patch_vs_0.10.0.diff
```

本地版本列表来源：`D:\Dev\Rust\.cargo\registry\index\mirrors.tuna.tsinghua.edu.cn-4dc01642fd091eda\.cache\gp\ui\gpui-{base,component}`
（稀疏索引缓存，`"vers"` 字段，最新均为 `0.10.0`）。

## 6. 限制与未覆盖

- **没有联网查上游仓库**（AGENTS.md：gpui-kit 一律走本地索引/本地源码）。因此结论只对本地可见的
  0.10.0 与 0.9.0 负责；若上游已发布更新版本而本地索引未同步，本结论不覆盖。
- 首轮核查时未重跑 `cargo check`（引用 11-22 升级评估与 11-23 门禁记录）；**本轮只补跑了
  `cargo check -j 1 -p gpui-base`（`Finished ... 2m49s`）**，用于验证删 `add_cursor_above_line` 后
  vendor 仍可编译；`gpui-component` 与 editor/kernel 门禁未重跑（另有并行会话占用构建目录）。
- 未做 UI 点验：书签列的实际像素外观、悬停 300ms 体感、拖拽浮层观感仍属视觉验收范围。
- `add_cursor_below_line` / `select_columnar_block` 的"仅测试调用"是按文本搜索统计的；若存在宏或
  间接调用（未发现），计数会偏低。

## 7. 本轮瘦身执行结果（2026-11-24）

### 7.1 已做

| 动作 | 证据 |
|---|---|
| 删除 `vendor/gpui-base/src/input/base/state.rs::add_cursor_above_line`（含其 doc 两行；`add_cursor_below_line` 的 doc 改成不指向已删方法） | commit `cec060ac`；`git show --stat` = `1 file changed, 3 insertions(+), 7 deletions(-)`；`cargo check -j 1 -p gpui-base` → `Finished` |

### 7.2 判定翻转：`Tab::on_context_menu` **不可撤**

首轮把它放进了"可撤"，依据是 `ContextMenuExt` 是 public 且 `Tab: ParentElement + InteractiveElement + Styled`
（`vendor/gpui-component/src/menu/context_menu.rs:17/23/43`，`ContextMenu<E>: IntoElement` 见 `:114`）。
二次核查补上收口一侧后结论正好相反：

- `TabBar::children(mut self, children: impl IntoIterator<Item = impl Into<Tab>>)`
  （`vendor/gpui-component/src/tab/tab_bar.rs:178`）—— 标签条只收 `Into<Tab>`；
- `ContextMenu<Tab>` 不是 `Tab`，也没有任何 `Into<Tab>` 的转换通道（只有 `IntoElement`），
  所以 `tab.context_menu(...)` 之后**无法再交给 `TabBar`**；
- 要在应用侧用它，只能给 `TabBar` 另开一条 `AnyElement` 子项入口（等于**新增** vendor 补丁），
  或者不用 `TabBar` 手搓标签条（违反"禁手搓"）。
- 另外 dock 内部（`dock/tab_panel.rs`）本来就用 `Tab::on_context_menu`，该补丁不可能整体退休。

→ 因此 `result_panel/src/sets.rs` **本轮未改**（该文件当时还有并行会话未提交的 T8 改动，见 7.3）。

### 7.3 未做与原因

- `crates/result_panel/src/sets.rs` 保持原样：核查期间该工作区有另一会话的未提交改动（T8 标签右键菜单等，
  约 40+ 文件 dirty），且该文件在 02:44 被对方重写过一次。按"绝不暂存他人并行改动"的纪律，本轮不碰应用侧文件。
- `add_cursor_below_line` / `select_columnar_block` 保留（理由见 §2 C）。

### 7.4 仓库坑：改 vendor 文件必须绕开 `core.autocrlf`

本机 `core.autocrlf=true`，而本轮改的 `vendor/gpui-base/src/input/base/state.rs` 在仓库里是 **CRLF 的 blob**
（`git cat-file` 数 CR 与 `git ls-files --eol` 实测；vendor 其它文件未逐个核，改之前先按同一手法确认）。
直接用"读文本 → 改 → 写回 → `git add`"会触发两件事：工作区变成 LF、`git add` 的 clean filter 再把整个文件规范成
LF 写进索引，`git show --stat` 就变成 `13885 insertions / 13885 deletions` 的假 diff
（本轮踩了两次，均已 `git reset --mixed HEAD~1` 撤销重做）。正确姿势：

```powershell
# 1) 用字节级替换改文件（保持 CRLF 不变），例如 python 里按 "\r\n" 拼 old/new，count(old) 必须 == 1
# 2) 绕过 filter 直接写 blob 与索引
$sha = (git hash-object -w --no-filters vendor/gpui-base/src/input/base/state.rs)
git update-index --cacheinfo "100644,$sha,vendor/gpui-base/src/input/base/state.rs"
git commit -m "<msg>" -- vendor/gpui-base/src/input/base/state.rs
git reset -q    # 让索引与 HEAD 同步，避免留下 "M " 悬挂态
```

改完后必须用 `git show --stat --oneline HEAD` 复核：行数应与逻辑改动等量，出现"上万行"就是又被 CRLF 坑了。
