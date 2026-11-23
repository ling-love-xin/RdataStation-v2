# gpui-kit 0.11.0 对我们的影响评估

日期：2026-11-24　分支：`main`　评估对象：`gpui-kit` **v0.11.0**（GitHub tag `v0.11.0` = `e97fe438`）
参照基准：本地 `vendor/gpui-base`、`vendor/gpui-component`（0.10.0 发布包 + 项目私有 patch，见
`docs/architecture/dependencies/gpui-kit-0.10.0-private-patch-support-audit.md`）

## 0. 结论摘要

1. **0.11.0 确实存在，但这台机器上装不到**：GitHub tag 已在（`v0.11.0` → `e97fe438`）；
   crates.io 走的清华镜像索引里**只有 0.10.0**（`cargo info --registry crates-io gpui-base` 刷新索引后仍解析到
   `0.10.0`，稀疏索引缓存 `mtime` 仍是 11-22）。要升 0.11，现在只能走 `git` 依赖或本地 `[patch]`。
2. **0.11 没有替我们接住任何一处私有 patch**：把我们补丁里的独有符号在 0.11 源码里逐个数，
   全部 0 命中（`GutterMarker*`、`set_gutter_markers`、`has_fold_candidates`、`collapse_cursors`、`cursor_count`、
   `take_last_edit_delta` / `LineDelta`、`TabReorder`、`TabBar::on_reorder`、`register_syntax_context_provider`、
   `tab_bar_max_width` / `tab_bar_variant`、`Notification::timeout`）。**该保留的补丁一条都少不了。**
3. 上一轮的两条结论在 0.11 依然成立，并且源码位置可以复核：
   `ContextMenuExt` 仍公开（`menu/mod.rs:11`）；但 `TabBar::children` 仍是 `impl Into<Tab>`（`tab_bar.rs:161`），
   `ManagedTooltipExt` 仍是 `pub(crate)`（`tooltip.rs:319`），`Notification` 仍写死 `Duration::from_secs(5)`
   （`notification.rs:885`），代码编辑器左内边距仍硬写 `min(px(6.))`（`input/input.rs:657` 附近）。
4. **升级不是白升级**：base 62 文件 `+7466 −1900`、component 48 文件 `+3026 −546`；
   我们打补丁的 12 个文件里，`element.rs`（`+1350/−529`）、`state.rs`（`+609/−62`）、`component/input/input.rs`
   （`+120/−7`）改动最大——**书签列 / 折叠说明这两块补丁要重做一遍**。
5. **真正的收益集中在"新能力"，不在"补丁退休"**。按对本项目的价值排序：
   ① 编辑器 gutter 可配（`GutterStyle` + `EditorLineNumberRenderer`，0.10 没有）；
   ② 折叠变成公开命令（`set_folded` / `fold_all`，0.10 没有）；
   ③ `Input::list` 下拉检索列表（`list_popup.rs` 921 行，0.10 没有）；
   ④ `diff` 组件大升级（`DiffSpanHighlight` / `DiffGutterMarker` / `DiffState` / 冲突解，0.10 没有）；
   ⑤ 滚动位置/滚动条位置（`ScrollbarPlacement`、`scroll_y_f64`、`content_height_f64`，0.10 没有）；
   ⑥ 文本区定位（`reveal_range` / `on_reveal`，0.10 没有）；⑦ `resizable::is_resizing`；⑧ 文本排版重写。
6. **建议：本轮不升**。先把"能靠公开入口替代的补丁口径"收紧（减少以后再 rebase 的面），
   等镜像同步 0.11 且 0.11.x 稳定后，在一个隔离分支上做一次"补丁试合并 + 类型体检"，再决定。

## 1. 事实：版本与获取路径

| 项 | 实测 |
|---|---|
| GitHub tag | `v0.11.0` → `e97fe438129c5e4ccab504f2e6fafd9a4e396bba`（`git ls-remote --tags`） |
| 各 crate 版本 | `crates/{base,component,kit,assets,shell,webview,fps,component-macros}` 的 `Cargo.toml` 均为 `version = "0.11.0"` |
| 本机镜像 | `D:\Dev\Rust\.cargo\registry\index\mirrors.tuna.tsinghua.edu.cn-...\.cache\gp\ui\gpui-{base,component,kit}`：版本列表止于 `0.10.0`，缓存 `mtime` = 11-22 01:05 |
| `cargo info` | `cargo info --registry crates-io gpui-base` → `Updating tuna index` … `version: 0.10.0` |
| 本地源码 | `.rds/tmp/gpui-kit-0.11`（`git clone --depth 1 --branch v0.11.0`，`describe` = `v0.11.0`） |
| 上游 release notes | 仓库内 `release-notes.md` 在 0.11 tag 上仍只有 `# GPUI Kit v0.8.0` 一节（**过期**），因此本文的"0.11 改了什么"全部来自源码 diff，不引用发布说明 |

## 2. 补丁符合性：0.11 里我们独有符号的命中数（实测）

来源：`.rds/tmp/v011_symbol_probe.py` → `.rds/tmp/v011_symbol_probe.txt`（扫描 `vendor/*/src` 与 `.rds/tmp/gpui-kit-0.11/crates/{base,component}/src`）。

| 我们的补丁符号 | vendor | 0.11 base | 0.11 component | 结论 |
|---|---:|---:|---:|---|
| `GutterMarkerKind` / `GutterMarkerClick` / `set_gutter_markers` / `set_gutter_marker_column` / `shows_gutter_column` | 有 | 0 | 0 | 上游仍无 → 保留 |
| `gutter_note_pin` / `set_gutter_note_pin` / `gutter_hover_note_since` | 有 | 0 | 0 | 上游仍无 → 保留 |
| `has_fold_candidates` | 有 | 0 | 0 | 上游仍无 → 保留 |
| `collapse_cursors` / `cursor_count` / `select_columnar_block` | 有 | 0 | 0 | 上游仍无 → 保留 |
| `take_last_edit_delta` / `LineDelta` | 有 | 0 | 0 | 上游仍无 → 保留 |
| `TabReorder` / `TabDragGhost` / `on_reorder`（base `tabs.rs`） | 有 | 0 | — | 上游仍无 → 保留 |
| `on_reorder`（component `tab/`） | 有 | — | **0** | `TabBar::on_reorder` 仍是补丁独有 |
| `register_syntax_context_provider` / `SyntaxContextFactory` | 有 | 0 | 0 | 上游仍无（`LanguageProvider` 仍只是粗粒度入口） |
| `tab_bar_max_width` / `tab_bar_variant` | 有 | 0 | 0 | 上游仍无 → 保留 |
| `Notification::timeout`（`fn timeout(`） | 有 | 0 | 0 | 上游仍无 → 保留 |
| `GutterDecoration` | 有 | 5 文件 | 1 文件 | 仍是 `range + color + folded_color`，**无图标/点击/说明** |
| `FoldPlaceholderRenderer` | 有 | 4 文件 | 0 | 仍是"折叠行占位内容"，不是折叠说明 |

顺带记一条**与我们补丁无关、但容易被误判成"上游已支持"**的：
`cursor_offsets`、`set_editor_paddings`、`ResizablePanel::fixed`、`Editor::context_menu`、`TabVariant::Folder`、
`DataTable::fit_col` / `fixed_cols_border` / `focus_ring` 这些**在 0.10.0 就已经是公开的**，不是 0.11 新增。
（本轮先按 `release-notes.md` 归因，随后用 0.10/0.11 双侧 grep 纠正——发布说明过期，不能作为归属依据。）

## 3. 升级成本：我们打补丁的 12 个文件，上游改了多少

来源：`.rds/tmp/v011_12files.py` → `.rds/tmp/v011_12files_diffstat.txt`（0.10.0 纯净包 vs 0.11 tag，`--ignore-cr-at-eol`）。

| 文件 | 上游 0.11 改动 | 我们的补丁量 | 冲突风险 |
|---|---|---|---|
| `input/base/element.rs` | `+1350 / −529` | `+527 / −12`（gutter 列、折叠说明、悬停说明） | **高**（同一个渲染函数被重写） |
| `input/base/state.rs` | `+609 / −62` | `+285 / −8` | **高**（折叠/选区/滚动都在动） |
| `input/editor/display_map/display_map.rs` | `+33 / −0` | `+9` | 低 |
| `tabs.rs` | `+14 / −10` | `+86 / −5` | 中 |
| `dock/mod.rs` | `+15 / −0` | `+26` | 低 |
| `dock/tab_panel.rs` | 无改动 | `+72 / −2` | 无 |
| `component/input/input.rs` | `+120 / −7` | `+16 / −17`（左内边距 0） | 中（同一处 `set_editor_paddings` 附近） |
| `component/input/mod.rs` | `+3 / −0` | `+3` | 低 |
| `component/input/syntax_context.rs` | 无改动 | `+41 / −1` | 无 |
| `component/notification.rs` | `+3 / −3` | `+12 / −1` | 低 |
| `component/tab/tab.rs` | `+4 / −6` | `+65 / −2` | 低 |
| `component/tab/tab_bar.rs` | 无改动 | `+19` | 无 |

体量参考（整 crate `src/`）：base **62 文件 `+7466 −1900`**；component **48 文件 `+3026 −546`**。
新增文件：base `input/editor/display_map/text_wrapper/{char_widths.rs,monospace.rs}`（各带 tests）；
component `input/gutter_style.rs`（319 行）、`input/list_popup.rs`（921 行）、`diff/pointer_tests.rs`；
删除：component `time/utils.rs`（200 行）。

## 4. 0.11 对我们的实际价值（逐项：上游位置 → 我们能用在哪儿）

### 4.1 编辑器 gutter 可配：`GutterStyle` + `EditorLineNumberRenderer`

- 上游：`crates/component/src/input/gutter_style.rs:14 pub struct GutterStyle`（`gap` / `line_number_color` /
  `active_line_number_color` / `line_number_font_family` / `line_number_font_size`，
  builder：`with_gap` / `with_line_number_color` / …），`pub use gutter_style::GutterStyle`（`input/mod.rs:9` 附近）；
  组件侧入口 `Editor::gutter_style(style)`（`input/editor.rs:63`）；
  base 侧抽象 `pub trait EditorLineNumberRenderer`（`crates/base/src/input/editor/mod.rs:14`，含
  `measure_line_numbers` / `resolve_line_number_baseline_offset` / `shape_line_number`）。
- 我们的收益：书签列那套补丁里有一批"行号样式/间距"的常量（行号右移、颜色、字号），现在可以**从应用侧配**，
  不必每版都改 `element.rs` 里的常数。注意：它只描述"行号车道"，**不提供自定义 gutter 内容的绘制入口**，
  所以书签/说明图标的绘制仍得靠补丁（§2 已证 0 命中）。
- 附带发现：`Input::renders` 里代码编辑器的左内边距仍是 `min(px(6.))`（`input/input.rs:657` 附近），
  且 `state.set_editor_paddings` 早在 0.10 就是 `pub`（`base/src/input/base/state.rs:1199`）——
  说明我们那条 padding 补丁不是"没有公开入口"，而是"组件每帧重设、应用覆盖会被打回去"，0.11 没变，补丁继续留。

### 4.2 折叠变成公开命令：`set_folded` / `fold_all`

- 上游：`base/src/input/base/state.rs:13953 pub fn set_folded(&mut self, start_line, folded, cx) -> bool`、
  `:13963 pub fn fold_all(&mut self, cx)`（0.10 均无）；判据 `self.display_map.is_fold_candidate(start_line)` 也在上游。
- 我们的收益：分析模式/编辑器要"折叠全部""按行折叠/展开"时，不必再摸内部 `display_map`；
  书签/折叠说明的交互可以只保留"画"的部分补丁，"折叠状态"走公开 API。
- 边界：**折叠说明（下划线 + 常显 chevron）仍需补丁**；"高亮器返回空把外部候选冲掉"的上游行为也没修，
  所以我们的折叠候选空守卫 `has_fold_candidates` 仍要留。

### 4.3 `Input::list`：输入框自带检索下拉

- 上游：`component/src/input/input.rs:319 pub fn list<D: ListDelegate>(mut self, list: &Entity<ListState<D>>) -> Self`
  + 新文件 `input/list_popup.rs`（921 行，`List` 浮层、随输入筛选、`Enter`/`Esc`/上下键、空结果态）。
- 我们能用在哪儿：连接选择器 / 数据库对象选择器 / 设置页里的长列表选择——目前这些是自建 picker
  （`crates/project/src/ui/picker.rs`）。0.11 给了一条"官方 combo list"，可以逐步替换自建实现（符合"禁手搓"）。
- 注意：`Input::list` 接受的是 `Entity<ListState<D>>`，要接我们的 delegate 抽象，替换是"改一处、验一处"，不是全局替换。

### 4.4 `diff` 组件大升级（资产版本对比的现成渲染器）

- 上游：`component/src/diff/mod.rs` 导出 `DiffState` / `DiffFile` / `DiffFileStatus` / `DiffLinePosition` /
  `DiffLineRange` / `DiffSide` / `DiffTextPosition` / `DiffDecoration` / **`DiffGutterMarker`** /
  `DiffSpanHighlight` / `DiffConflict` / `DiffConflictResolution` / `DiffEvent` / `DiffInlineUnit`；
  0.11 新增/大改：`decoration.rs +89`、`document.rs +32`、`mod.rs +187/−19`、`selection.rs +94/−19`、`conflict.rs +60/−7`。
- 我们能用在哪儿：资产存档的版本对比 / 时间线里的"这一版改了什么"——我们现在没有通用的 diff 渲染组件，
  自建成本高；0.11 这个是"只读、虚拟化、带 gutter 标记"的现成件。
- 注意：它是"解析外部 patch 文本"的模型（`DiffParseOptions`），不是"任意两段文本 diff"；要先产出 unified diff。

### 4.5 滚动：位置可配 + 精确滚动读数

- 上游：`base/src/scrollbar.rs` 新增 `enum ScrollbarPlacement`（`is_left` / `is_top` / `placement`）；
  `input/base/state.rs` 新增 `scroll_y_f64` / `set_scroll_y_f64` / `scroll_by_y_f64` / `content_height_f64` /
  `scrollbar_placement` / `set_scrollbar_placement`；`input/scrollbar_presentation.rs` 同步（`+21/−1`）。
- 我们能用在哪儿：结果集网格的横向滚动（滚动条放左侧/顶部）、"滚动位置同步/恢复"（`scroll_y_f64` 是精确值，
  现在只有像素/行级别的近似）；`content_height_f64` 可以直接喂自定义滚动指示器。

### 4.6 文本区定位：`reveal_range` / `on_reveal`

- 上游：`base/src/selectable_text.rs`（`+137/−10`）新增 `reveal_range` / `on_reveal`（0.10 无）。
- 我们能用在哪儿：书签跳转、搜索结果定位、分析模式输出里"跳到某个 block"。现在这类跳转要自己算滚动偏移。

### 4.7 `resizable::is_resizing`

- 上游：`base/src/resizable/mod.rs:64 pub fn is_resizing(&self) -> bool`（0.10 无）。
- 我们能用在哪儿：我们已经在用 `resizable::resize_handle`（`crates/result_panel/src/panel/sidebars.rs:16/677`），
  拖拽期可以据此降载（拖拽中不重排表格 / 不重算 EDA），松手再刷。

### 4.8 文本排版重写（分析模式输出会受益）

- 上游：`base/src/text/{node.rs +400/−167, inline_flow.rs +894/−245, text_view.rs +149/−13, state.rs +63/−1}`；
  编辑器侧 `input/editor/display_map/text_wrapper.rs +634/−160`，并新增 `text_wrapper/char_widths.rs`（404 行）
  与 `text_wrapper/monospace.rs`（119 行）。
- 我们能用在哪儿：分析模式里 Markdown/表格/长段落输出的换行与等宽对齐；`char_widths` 是"等宽字符宽度表"，
  能减少逐帧 shaping 抖动（大结果集/长文本滚动更稳）。
- 需要真机验：这类改动只读源码看不出收益，得跑起来才知道帧率/对齐是否真的更好。

### 4.9 顺带修复（低优先，需真机验）

- `title_bar.rs +107/−20`：新增 `window_button_layout` / `supported_buttons` / `WindowControlArea` 相关，
  主要是 Linux 按钮布局；Windows 侧是否有行为变化需要点验（我们做过 Windows 材质/拖拽标题栏）。
- `a11y_text.rs +122/−83`、`a11y_disabled.rs −9`、`focus_trap.rs −10`：无障碍实现有改动，
  但**不能据此断言**解决过我们遇到过的 a11y 闪退，只能作为"升级后回归验证点"。

## 5. 不建议现在升级的理由

1. **镜像没有 0.11**：本地索引 + 清华镜像刷新后仍是 0.10.0；升级要改 `Cargo.toml` 成 git 依赖或
   把 tag 源码 vendored 进来，属于"供应链形态变更"，应先拍板。
2. **补丁要重做两处大的**：`element.rs`（书签列/折叠说明/悬停说明）与 `state.rs`（`GutterMarker`、
   `LineDelta`、多光标）都撞在上游大改的同一批函数上，rebase 不是"改几行"。
3. **没有 GUI 验收条件**：升级后必须点验书签列、悬停 300ms、拖拽浮层、折叠说明、结果集标签条；
   你目前不便帮测 GUI，所以"升了也不知道有没有坏"。
4. **0.11.0 有破坏性尾巴**：`time/utils.rs` 删除、`Editor` 内部改成泛型 renderer（`Editor<R>`）、
   `menu/context_menu.rs` 有 `−18` 的删改——需要逐条核我们的调用点（本轮只核了补丁点与几个关键组件，**完整 breaking 清单未产出**）。

## 6. 建议的执行顺序（若要升）

1. **先量冲突，不动 main**：在隔离分支/`.rds/tmp` 里把 0.11 源码就位，按 12 个文件把我们的补丁
   `git apply --3way` 试合并，产出"哪几 hunk 冲突、冲突在哪个函数"的清单。
2. **收紧可撤面**（无论升不升都值得）：
   - 书签列里的行号样式/间距常量 → 换成 `GutterStyle`（0.11 才有，所以这条与 1 同时做）；
   - "折叠全部/按行折叠"的入口 → 换 `set_folded` / `fold_all`；
   - 其余（`Tab::on_context_menu`、`Tab::tooltip`、`Notification::timeout`、dock 标签栏 width/variant、
     gutter 图标/点击/说明、语法上下文注册表）**确认必须留**（§2）。
3. **类型体检**：`cargo check -j 1 -p gpui-base -p gpui-component` 与 `-p rds-editor --lib --tests`，
   只看 0.11 引入的 API 差异（`Editor<R>`、`time/utils` 删除、`context_menu` 改动）。
4. **门禁与点验**：editor 864/0/4、kernel 14/0/6 复跑；GUI 侧书签列/折叠说明/悬停/拖拽/标签条逐项点验。
5. **再决定是否落 main**；不满足"冲突可控 + 门禁绿 + 点验过"就保持 0.10 + 私有 patch。

## 7. 本轮能立刻用的（不改版本）

| 想做的事 | 现在能做吗 | 依赖 |
|---|---|---|
| 资产版本对比视图 | 不能（等 0.11 diff 组件） | 0.11 |
| 对象选择器换官方下拉 | 不能（`Input::list` 在 0.11） | 0.11 |
| 书签跳转精确滚动 | 部分（`reveal_range` 在 0.11；当前自算偏移可用） | 0.11 更好 |
| 折叠全部/按行折叠走公开 API | 不能（`set_folded` 在 0.11） | 0.11 |
| 结果集横向滚动条位置 | 不能（`ScrollbarPlacement` 在 0.11） | 0.11 |

## 8. 限制与未覆盖

- **只做了源码比对，没有编译 0.11**：`Cargo.lock` 未切换，未跑 `cargo check`/`cargo test`，未做 GUI。
  因此"升级会不会编译不过"未验证；§3 的冲突风险是按改动量估计，不是试合并结果。
- 0.11 的完整 breaking changes 清单未产出（只核了我们用的组件入口 + 补丁点）。
- 0.11 源码来自 GitHub tag 的 shallow clone（`e97fe438`），**不是** crates.io 发布包；
  发布包可能裁掉 `examples/`、`crates/story*` 等，但不影响 `src/` 结论。
- 仓库内 `release-notes.md` 在 0.11 tag 上仍是 v0.8.0 内容，本文一律不引用发布说明下结论。

## 9. 复现方式

```powershell
# 1) 版本事实
git ls-remote --tags https://github.com/longbridge/gpui-kit | Select-String 'v0.1[01]'
cargo info --registry crates-io gpui-base        # → version: 0.10.0（镜像未同步）

# 2) 取 0.11 源码（已落在 .rds/tmp，gitignore）
git clone --depth 1 --branch v0.11.0 --single-branch https://github.com/longbridge/gpui-kit .rds\tmp\gpui-kit-0.11

# 3) 补丁符合性（vendor vs 0.11，逐符号命中数）
python .rds\tmp\v011_probe.py                     # → .rds\tmp\v011_symbol_probe.txt

# 4) 12 个补丁文件的上游改动量
python .rds\tmp\v011_12files.py                   # → .rds\tmp\v011_12files_diffstat.txt

# 5) 0.10 → 0.11 新增公共 API（源码 diff 提取）
python .rds\tmp\v011_delta.py                     # → .rds\tmp\v011_delta.txt
```
