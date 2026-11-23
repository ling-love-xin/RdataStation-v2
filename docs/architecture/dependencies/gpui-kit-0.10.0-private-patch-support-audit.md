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
3. **补丁仍全部在手，且绝大部分在生效**：vendor 内 12 个源文件、70 个 hunk、`+1165 −48` 都在；
   项目侧有实际调用点（下表 §3），只有 3 处是死代码/仅测试调用。
4. **唯一有"官方等价入口"的是自定义语法上下文**——但它是 **0.9.0 就有**的 `LanguageProvider` +
   `set_language_provider`，不是这次 0.10.0 升级带来的；而且完全替代有代价（见 §2 B-1）。
5. 另有 **2 处上游本来就有等价公开 API**，我们的对应小补丁属"非必需"（`Tab` 右键菜单、多光标 public 包装），
   可以撤，撤法见 §2。

> 换句话说：**没有"升级后上游已经支持了、补丁可以整体退休"这回事**。能瘦身的只有少数几处，
> 主体（书签列 / 折叠说明 / 悬停说明 / 折叠候选守卫 / 标签拖拽重排 / dock 标签栏配置 / 通知时长）必须继续靠 vendor patch；
> 语法上下文虽可换官方入口，但要自己承接组件的别名与默认配置，工程上不划算。

## 1. 量化：补丁面实测

对比基准＝纯净 0.10.0 发布包 `.rds/tmp/crates1011/{gpui-base,gpui-component}-0.10.0`（与 crates.io 发布内容一致），
对比对象＝`vendor/`。行尾差异用 `--ignore-cr-at-eol` 排除。

| 文件 | +/− | hunk | 补丁内容要点 |
|---|---:|---:|---|
| `gpui-base/src/input/base/element.rs` | +527/−12 | 19 | gutter 书签列布局/绘制、折叠说明下划线、悬停说明气泡（300ms 延迟）、pin、点击上报、行号右移 |
| `gpui-base/src/input/base/state.rs` | +289/−8 | 17 | `GutterMarker{Kind}` / `set_gutter_markers` / `GutterMarkerClick` / pin / hover 计时 / 列开关；多光标与列选择 public API；`LineDelta` 编辑行增量；折叠候选空守卫（2 处） |
| `gpui-base/src/input/editor/display_map/display_map.rs` | +9/−0 | 1 | `has_fold_candidates()`（供空守卫判断） |
| `gpui-base/src/tabs.rs` | +86/−5 | 6 | `TabReorder` / `TabDragGhost` / `Tab::on_reorder`（拖拽重排 + 拖拽浮层） |
| `gpui-component/src/dock/mod.rs` | +26/−0 | 4 | `SkinShared` 新增 `tab_bar_max_width` / `tab_bar_variant` + `DockSkin::set_*` |
| `gpui-component/src/dock/tab_panel.rs` | +72/−2 | 4 | 把上面两个值透传给 `TabBar`；dock 标签右键菜单（缩放/关闭/关闭其他/关闭全部） |
| `gpui-component/src/input/input.rs` | +16/−17 | 1 | 代码编辑器左内边距 `min(6px)` → `0`（行号顶到最左） |
| `gpui-component/src/input/mod.rs` | +3/−0 | 1 | re-export `register_syntax_context_provider` / `SyntaxContextFactory` |
| `gpui-component/src/input/syntax_context.rs` | +41/−1 | 2 | 自定义语法上下文工厂注册表，优先于 tree-sitter |
| `gpui-component/src/notification.rs` | +12/−1 | 5 | `Notification::timeout(Duration)`（默认 5s） |
| `gpui-component/src/tab/tab.rs` | +65/−2 | 7 | `Tab::on_context_menu` / `Tab::tooltip` / `on_reorder` 字段与接线 |
| `gpui-component/src/tab/tab_bar.rs` | +19/−0 | 3 | `TabBar::on_reorder`（整条标签条下发到每个标签） |

合计 **12 个源文件 / 70 hunk / +1165 −48**。另有 2 个 `Cargo.toml.orig` 是发布包打包产物差异（vendor 不带），
不计入补丁。

## 2. 逐点判定

### A. 上游没有、必须保留（11 项）

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

### B. 上游有等价入口/API，可评撤（3 项，附代价）

| 补丁点 | 上游等价入口 | 撤的代价 |
|---|---|---|
| **自定义语法上下文注册表**（`input/mod.rs` re-export + `syntax_context.rs` 注册表） | `gpui_base::input::{LanguageProvider, set_language_provider}` —— **0.9.0 起就有**，`0.10.0` 只是继续沿用（两版都是 `1/1` 命中）。provider 的 `syntax_context_provider(name)` 正是我们要的钩子 | ① 全局**单** provider：组件 `init` 会装自己的 `ComponentLanguages`，应用再装会顶掉它，顺序敏感；② 组件的 `language_name()` 走 `LanguageRegistry::editing_language_name`，该方法 **`pub(crate)`**，别名归一（`pyi`→`python`、`JSON` 之类）拿不到，得自己维护；③ `text/json/python` 的默认 bracket / auto-closing / indent 配置也要自己重写一遍。→ **技术上可撤，工程上不划算**；若不撤，建议把这条补丁标成"上游有粗粒度入口、我们提供按语言细粒度注册" |
| `Tab::on_context_menu`（应用自建标签条部分，`result_panel/sets.rs:219`） | `Tab` 实现了 `ParentElement + InteractiveElement + Styled`，`ContextMenuExt` 是 **public**（`menu/mod.rs:10` 导出），可直接 `tab.context_menu(|menu, window, cx| ..)` | 闭包签名从 `(PopupMenu, &mut Window, &mut App)` 改成 `(PopupMenu, &mut Window, &mut Context<PopupMenu>)`，返回值多一层 `ContextMenu<Tab>` 包装；**dock 内的标签仍要补丁**（那部分由 vendor 渲染，改不到） |
| 多光标 public 包装 `add_cursor_above` / `add_cursor_below`（`pub(super)` → `pub`） | 上游动作 `AddCursorAbove` / `AddCursorBelow` 存在（快捷键已绑 `ctrl-alt-up/down`），只是没有公开方法 | 项目生产代码 **0 调用**（只有 `*_line` 包装被测试用到）；改动作派发即可撤 |

### C. 死代码 / 仅测试调用（现在就能删）

| 符号 | 项目内调用 |
|---|---|
| `add_cursor_above_line` | **0 处**（生产与测试都没有） |
| `add_cursor_below_line` | 3 处，全在测试（`view/host/results/tests.rs`、`view/tests/toolbar_custom.rs`） |
| `select_columnar_block` | 5 处，全在测试（同上两个文件） |

> 这三条可以先删，或把测试改成动作派发后再删；删了能少 2 个 hunk 的公共 API 面。

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
| `Tab::on_context_menu` | `result_panel/src/sets.rs` |
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
- 未重新跑 `cargo check`（引用 11-22 升级评估与 11-23 门禁记录，不重复消耗 8 核/15.8GB 的构建窗口）。
- 未做 UI 点验：书签列的实际像素外观、悬停 300ms 体感、拖拽浮层观感仍属视觉验收范围。
- `add_cursor_below_line` / `select_columnar_block` 的"仅测试调用"是按文本搜索统计的；若存在宏或
  间接调用（未发现），计数会偏低。
