# 结果集表格模块（rds-result-panel）架构设计

> 状态：已落地（2026-10-23）。本会话范围：独立 crate + 并排对比；状态栏合并是另一会话任务。
> 技术栈：Rust + GPUI-kit 0.8.0（vendor 权威源）；API 一律查 `vendor/gpui-component/src/`。

## 1. 目标

把「结果集表格」从编辑器宿主中拆成独立 crate（DBeaver 式 Data Editor 方向）：

1. **结果集面板独立 crate**：结果域视图层整体搬出 `rds-editor`；依赖只向下
   （`rds-result-panel` 只依赖 `engine` + `gpui-kit` + `workbench_shell::ui` + 主题 token；
   `editor` 依赖它，它绝不反向依赖 `editor`）。
2. **ResultStore 归属**：随结果面板走（用户确认）：结果域模型整体搬进
   `rds-result-panel`，`engine` 只保留 `QueryResult` 不动。
3. **多结果集并排对比**：Tab 为主 + 「并排对比模式」（用户确认形态）。
4. **补充文档与原型**：本文件 + `crates/result_panel/README.md`。

明确不做（本会话）：编辑器状态栏合并进全局状态栏（另一会话）；编辑器文档内核、
insight、plugin。

## 2. Crate 边界

```
                ┌─────────────────────────────┐
                │  rds-editor（编辑器宿主）       │
                │  host.rs 结果区胶水（待实体化）  │
                └──────────────┬──────────────┘
                               │ 依赖
                ┌──────────────▼──────────────┐
                │  rds-result-panel（结果面板）   │
                │  store/changes/export/grid/  │
                │  sets/error_card/script_card/│
                │  analysis/status/ui          │
                └──────────────┬──────────────┘
          ┌────────────────────┼────────────────────┐
          │ 只依赖（不反向）        │                    │
   ┌──────▼──────┐   ┌─────────▼────────┐   ┌─────────▼────────┐
   │ rds-engine   │   │ gpui-kit 0.8.0   │   │ rds-workbench-shell│
   │ QueryResult  │   │ （vendor）         │   │ ui.rs / model.rs   │
   └─────────────┘   └──────────────────┘   └──────────────────┘
```

## 3. 搬迁与重导出（editor 路径零破坏）

| 结果域文件 | 迁入位置 | editor 侧 |
| --- | --- | --- |
| `store.rs` | `result_panel/src/store.rs`（+`ResultPlacement`/`Pushdown` 一并迁入） | `crates/editor/src/store.rs` 改 `pub use result_panel::store::*` |
| `changes.rs` / `export.rs` | 同上 | 同上 |
| `grid.rs` | `result_panel/src/grid.rs`（`RESULT_GRID_CONTEXT` 迁本体；3 个测试访问器去 `#[cfg(test)]` 成为公开 API） | `view/results/grid.rs` 重导出；`commands.rs` `pub use result_panel::grid::RESULT_GRID_CONTEXT` |
| `sets.rs` / `error_card.rs` / `script_card.rs` | 同上 | `view/results/` 对应文件重导出 |
| `analysis.rs` | 纯模型留下（菜单项/请求/常量/引用函数）；`result_entry`（依赖 editor 的 `QueryData`）留守 editor | `editor/src/analysis.rs` = `pub use result_panel::analysis::*` + `result_entry` |
| `status_bar.rs` 的 `elapsed_text` | 新写 `result_panel/src/status.rs`（+3 测试） | `view/widgets/status_bar.rs` 重导出 |
| `ui.rs` 结果面板尺寸常量 | 新写 `result_panel/src/ui.rs`（`pub use workbench_shell::ui::HAIRLINE` 单一来源） | — |

共享模型下沉到外壳 crate（`workbench_shell/src/model.rs`，2026-10-23）：
`DocumentId` / `ExecChannel` / `ColumnAnnotation`；editor 侧重导路径不变
（`crate::model::DocumentId`、`crate::channel::ExecChannel`、`crate::completion::ColumnAnnotation`）。

**测试基线**（`cargo test -p rds-result-panel -p rds-editor --lib`）：
rds-editor **581 passed** / 0 failed（结果域测试随 crate 迁出）+ rds-result-panel **107 passed**
/ 0 failed = 合计 688，无回归。

## 4. ResultStore：按文档分槽 + 对比选中

```rust
struct DocumentResults {
    document: DocumentId,
    sets: Vec<ResultEntry>,          // 按产生顺序
    changes: Vec<ChangeSet>,         // 与 sets 同序；切换选中不清空
    active: usize,                   // 主选中（标签条）
    compare: Option<usize>,          // 【并排对比】对比选中（≠ active 恒成立）
}
```

落位：`Replace`（替换主选中）/ `NewSet`（追加，主选中不动）/ `Append`（取下一段接行）。
淘汰：`evict_oldest` 超上限（5 份）淘汰最旧未选中项；**对比选中同步修正**
（对比项被淘汰 → 清空；下标前移 → 跟着减）。

对比 API：
- `compare_index(document)` / `compare_entry(document)`：读对比项
- `select_compare(document, index)`：设为对比（拒绝 `active` 本身 / 越界）
- `toggle_compare(document, index)`：点同一份 = 取消；点别的份 = 换
- `select()` 切主选中到对比项本身 → 对比自动清空（自己对比自己没有意义）

## 5. 并排对比（本会话落地）

### 5.1 交互（用户确认：Tab 为主 + 对比模式）

- 结果工具栏新增「⧉ 并排对比」开关（`IconName::Columns2`；两份以上结果才摆）。
- 开启：结果区变双栏 —— 左栏 = 主选中结果集（原结果面板全量能力），
  右栏 = 对比选中结果集。自动选「除主选中外最近一份」作对比对象（开开关就要看到两个东西）。
- **双栏各带自己的结果集标签条**：左栏标签条切主选中（原行为），
  右栏标签条切/取消对比对象（点当前对比项 = 取消）。
- 关闭：右栏消失、对比选中清掉（下次开对比重新选）。
- 主选中切换不带动对比对象（对比是显式选择）；切到对比项本身时对比自动失效。

### 5.2 渲染（host.rs 结果区）

```
editor-result-compare-layout（h_flex，对比模式开启时）
├── 左栏 flex_1 ── result_grid::render(主 ResultPane)     ← 原结果面板
│      （标签条 ⑤ / 提示 / 工具栏 ⑥ / 网格 / 状态行 ⑦）
└── 右栏 flex_1 ── result_grid::render(对比 ResultPane)   ← 只读对照
       （右栏标签条 ⑤ / 工具栏（空动作）/ 只读网格 / 状态行 ⑦）
```

对比栏数据经 `sync_compare_grid` 在事件路径拷入 `compare_grid`
（构造期与主网格同处建好，面板生命周期内复用；render 只读网格状态）。
**对比栏只读**：不接编辑 / 取段 / 筛选 / 洞察——那些动作的语义都归主栏。

### 5.3 测试

- `result_panel/src/store.rs`：4 例（对比≠active / 切主选中清对比 / toggle / 淘汰修正）
- `editor/src/view/tests.rs`：3 例（开对比双栏 + 自动选对比对象 / 右栏切换与取消 /
  关闭复位清选中），全部走宿主驱动入口（headless 下点击不稳，按钮接线由选择器 + 真机验证）

## 6. 结果面板原型布局（DBeaver Data Editor 方向）

```
┌──────────────────────────────────────────────────────────┐
│ ⑤ 结果集标签条  结果 1 │ 结果 2 │ 结果 3        ← sets.rs │
├──────────────────────────────────────────────────────────┤
│ ⑥ 结果工具栏  …行数/耗时/血缘…   ⌕筛选 ⤢转置 ⧉对比 ⚗分析 ▾ ⤓导出 ▾ ⟳ 复制 │
├──────────────────────────────────────────────────────────┤
│ ┌──────────────────────┬───────────────────────────────┐ │
│ │ 主选中网格（左栏）        │ 对比网格（右栏，只读）           │ │
│ │ # 列1 列2 …           │ # 列1 列2 …                  │ │
│ │ 1 …                   │ 1 …                          │ │
│ └──────────────────────┴───────────────────────────────┘ │
├──────────────────────────────────────────────────────────┤
│ ⑦ 结果状态行  共 N 行 · X ms · 已截断…        已选第 R 行    │
└──────────────────────────────────────────────────────────┘
```

对比模式 = 双栏各一份；主栏保留全部动作，对比栏只读。

### 6.1 DBeaver 风格网格细节（2026-10-24 补充，对齐 8.7 ①）

网格视觉逐项（`#` 行号列 · 焦点格粗边框 · 选中行/列高亮 · 排序指示 · NULL 灰斜体 · 状态行当前格）：

```
┌───────┬────────────────┬────────────────┬───────────────┐
│  #    │  id ▾          │  name          │  payload      │  ← 列头：名称+类型小标签+排序▾
├───────┼────────────────┼────────────────┼───────────────┤
│  1    │  101           │  Alice         │  BLOB · 1.2K  │  ← 普通格
│  2    │  102           │  NULL          │  NULL         │  ← NULL 灰斜体（与字符串区分）
│  3    │  103           │  Bob           │  _binary…     │
├┼──────┼────────────────┼────────────────┼───────────────┤
│  4    │ ░104░          │ ░Carol░        │ ░…░           │  ← 焦点格：粗边框(2px)+高亮
└───────┴────────────────┴────────────────┴───────────────┘
状态行：共 4 行 · 当前格「104」          已选第 4 行  取下一段
```

要点：

- **行号列**：面板自有 `#` 列（灰字），组件行头关闭；行号随行高，无数据语义
- **焦点格**：选中格 = `table_active` 背景 + 2px `table_active_border` 粗边框（组件补丁，见 §8.7 ①）；
  行选中 = 整行 `table_hover` 底；列选中 = 整列 `table_active` 底（组件原生）
- **排序指示**：组件内置（`Column.sort` → `render_sort_icon`），列头下拉「按此列排序」走右键菜单
- **NULL**：灰 + 斜体；BLOB：`BLOB · 大小` 摘要（悬停给完整 hex）
- **状态行**：左段 `共 N 行` + **当前格值预览**（`· 值`，长值截断 48 字符）+ 截断警告；
  右段 `已选第 R 行` / `未选中行` + 取下一段（has_more 时）

## 7. 未完成 / 遗留

| 项 | 状态 |
| --- | --- |
| 状态栏合并进全局状态栏 | 另一会话（用户明确）；布局债根因（host.rs 状态栏在 `v_resizable` 之下不在 min 保护链）记录在案 |
| ResultPanel Dock 化 | 见 §8 方案（用户已拍板：底部 dock + 全局联动 + 有结果自动开 + 移除嵌式结果区） |
| 结果面板侧边栏 | ✅ 本轮交付（`ee3077fa`）：左右分栏——左栏列结构（列名/类型/单列隐藏切换），右栏选中格值详情（完整值+列名+类型+位置，原样看全可滚动）；工具栏「列结构」「值详情」开关，默认关；对比模式让位双栏 |
| 并排对比增强 | 双栏等宽 → 可拖拽分隔；右栏列宽/滚动独立记忆 |

## 8. ResultPanel Dock 化方案（2026-10-23 拍板）

### 8.1 决策记录（用户确认）

| 决策点 | 拍板结果 |
| --- | --- |
| 面板形态 | 结果面板 = **独立 Dock**（DBeaver Data Editor 那种可停靠/关闭的面板，不是编辑器内嵌分栏） |
| Dock 位置 | **底部 dock（Bottom）**，SQL IDE 惯例（DataGrip/DBeaver 结果在编辑器下方独立面板） |
| 归属联动 | **全局单 dock + 活动文档联动**：结果面板展示当前活动编辑器文档的结果，切文档自动切换（对齐 `ResultStore` 按文档分槽） |
| 打开时机 | **有结果自动开**（执行回执到达即自动打开并聚焦底部结果面板；无结果自动收起） |
| 旧区处置 | **移除嵌式结果区**（host.rs 的 v_resizable 分栏结果区整体搬出，editor 只留访问口） |
| 动作接线 | **editor 宿主订阅面板 notify 自 drain（2026-10-24 定案）**：result_panel 定义请求枚举 + 队列，面板按钮 push 后 `cx.notify()`；editor 宿主构造期 `cx.observe_in(&result_panel, …)` 订阅，回调里 `drain_result_actions` 分派回动作方法。gpui 通知延迟到事务边界派发（`app.rs` notify → pending_effects），不会「cannot update while already being updated」。不用注入闭包、不经 workbench 中转 |

### 8.2 目标架构

```
Workbench DockArea（"workspace"）
├── Left  dock：导航 / 草稿 / 资源 / 插件
├── Right dock：洞察 / Mock / 历史 / 存档
├── Center：编辑器文档 tab（EditorHostPanel，仅剩代码/脚本编辑区）
└── Bottom dock【新增】结果面板（ResultDockTab 薄壳 → ResultPanel 实体）
      └── ResultPanel（result_panel::panel）
            ├── 主网格 Entity<TableState<ResultGridDelegate>>（自 host 迁入）
            ├── 对比网格（compare_mode 时双栏）
            ├── 投影状态（toolbar / status / tabs / error_card / script_card / transpose / compare）
            └── ResultActionQueue（面板级动作入队 → editor 事件路径 drain 分派）
```

依赖方向保持硬约束：`editor → result_panel → engine / gpui-kit / workbench_shell`；
workbench 层薄壳面板 `workbench → editor → result_panel`（结果动作仍在 editor 端口执行）。

### 8.3 迁移清单（host.rs → result_panel）

| host.rs 现状 | 去向 |
| --- | --- |
| 结果区字段 ~25 个（`result_toolbar` / `result_status` / `result_tabs` / `result_active` / `compare_mode` / `compare_grid` / `transposed` / `transpose_row` / `result_sql` / `result_is_analysis` / `result_can_copy` / `result_error_card` / `script_card` / `result_notice` / `fetching_more` / `error_site` / `error_site_key` 等） | `panel::ResultPanel` 字段 |
| 主网格 `grid` + 对比网格 `compare_grid` 实体 | `ResultPanel` 持有（构造期建，防 window 借用冲突） |
| `sync_result_view`（L5696，投影 + grid.update + 错误定位 + 消息） | `ResultPanel::sync(cx, input: ResultSyncInput)`；判据（可编辑性 / 连接文本 / 洞察可用 / 错误定位 site）由 editor 在事件路径算好传入 `ResultSyncInput` |
| 渲染组装（L7680-8146：filter 框 / transpose 组 / compare 开关 / commit 组 / copy / refresh / more / export / analysis / 错误卡 / 脚本卡 / `result_grid::render` + 对比双栏 + v_resizable） | `ResultPanel::render(&self, cx)`（按钮 `on_click` 改为 `queue.push(ResultRequest::…)`） |
| 结果动作方法（apply_filter_value / toggle_transpose / transpose_step / fetch_more / export_active_result(_all) / run_analysis / insight_column / select_result_set / select_compare_set / toggle_compare_mode / submit_pending_changes / discard_pending_changes / refresh_active_result / copy_active_result / copy_error_message / jump_to_error_site_with） | **留在 editor**（依赖执行链 / engine / 端口）；经 `drain_result_actions` 分派 |
| 网格 delegate 钩子（filter / sort / insight / preview / stats / load_more / commit_edit） | **保持注入钩子**（`Rc<dyn Fn(...,&mut App)>` 已跨 crate 安全，不重复发明）；editor 创建面板时 set |

### 8.4 ResultRequest 全集（面板级动作队列）

`result_panel::ports::ResultRequest` 枚举，`ResultActionQueue`（`Rc<RefCell<Vec<ResultRequest>>>`，`push` / `drain`）：
渲染按钮只 `push`（纯读），editor 事件路径 `drain` 后按变体分派到现有动作方法。

| 变体 | 触发 | editor 侧处理 |
| --- | --- | --- |
| `SelectSet(usize)` | 结果集标签条 | `select_result_set` |
| `SelectCompare(usize)` | 对比栏标签条 | `select_compare_set` |
| `ToggleCompareMode` | 工具栏对比开关 | `toggle_compare_mode` |
| `ToggleTranspose` / `TransposePrev` / `TransposeNext` | 工具栏转置组 | `toggle_transpose` / `transpose_step` |
| `Filter{ value }` | 工具栏筛选框提交 | `apply_filter_value` |
| `CommitPending` / `RollbackPending` | 提交 / 撤销变更对 | `submit_pending_changes` / `discard_pending_changes` |
| `Analysis{ sql }` / `AnalysisCustom` | 分析菜单项 / 自定义 | `run_analysis` / `request_custom_analysis` |
| `CopyTsv` | 复制按钮 | `copy_active_result` |
| `Refresh` | 刷新按钮（重跑 SQL） | `refresh_active_result` |
| `FetchMore` | 取下一段按钮 | `fetch_more` |
| `Export{ format, all }` | 导出菜单项 | `export_active_result(_all)` |
| `CopyError` / `LocateError` | 错误卡两按钮 | `copy_error_message` / `jump_to_error_site_with` |
| `InsightColumn{ column }` | 网格右键「洞察此列」 | `insight_column`（经既有 delegate 钩子，不入队） |

> 网格内部交互（右键菜单 / 行编辑 / 列统计 / 值预览 / 加载更多）**不走队列**——它们经
> `ResultGridDelegate` 既有注入钩子直接调 editor（跨 crate 安全，见 §8.3 末行）。

### 8.5 分阶段实施

1. **result_panel 内部**（不碰 editor/workbench，独立可编译）：`ports.rs`（ResultRequest +
   ResultActionQueue + 单测）→ `panel.rs`（ResultPanel 实体：状态 / sync / render，渲染从
   host.rs 照搬、按钮改入队）→ `lib.rs` 注册。✅ 已提交 `88a88999`
2. **editor 接线**：host.rs 删结果字段与渲染、`drain_result_actions` 分派、投影判据计算
   `ResultSyncInput`、网格钩子 set、测试访问器转发（保持 584 基线口径）。✅ 已提交 `375d6840`
3. **workbench dock**：`ResultPanelDock` 薄壳挂 Bottom dock（`panels/result_dock.rs`）、
   活动文档联动（render 权威点 `sync_result_dock`：活动 host → 面板 `has_content` →
   自动开/自动关；薄壳只渲染活动面板）、起步高度 12rem（`ui::RESULT_DOCK_HEIGHT`）。
   ✅ 已提交（阶段 3 收尾提交）
4. 全量回归 + 文档收尾 + 提交（只 add 本线文件）。

### 8.6 交互对齐 DBeaver（2026-10-23 用户拍板「尽量对齐」）

参考实证：DBeaver 官方文档 + `org.jkiss.dbeaver.ui.editors.data` 源码（ResultSetViewer /
IResultSetPresentation / LightGrid）。逐项对照结论：

| 项 | DBeaver 行为 | 本项目处理 |
| --- | --- | --- |
| 执行替换当前结果 tab | `resultSet.replaceCurrentTab=true` 默认**替换** | **已对齐**：`ResultPlacement::Replace` 是普通执行的既有语义（store.rs），无需改 |
| 结果面板最大化 | `Ctrl+Shift+6`（results panel 最大化） | 记 backlog：dock 面板 `zoom_control` 可挂「最大化结果面板」动作（薄壳当前返回 None，未实现） |
| 结果面板收起/展开 | `Ctrl+6` | 对齐：底部 dock 关闭/打开按钮天然等价（DockArea `toggle_dock`） |
| 呈现多视图（Grid/Text/Record） | `IResultSetPresentation` 可插拔 | 预留：`ResultControls` / `ResultPane` 已是呈现接入点雏形；Text/Record 视图后续挂新呈现，不进 dock 化范围 |
| 取全部行 / 计算总行数 | 底部工具栏按钮 | 记 backlog（现有「取下一段」已覆盖分页语义） |
| 列头下拉菜单（筛选/排序/隐藏） | 每列箭头菜单 | 记 backlog（现走右键菜单，交互细节可对齐） |
| 结果面板横竖分栏可配 | `resultSet.orientation` | 记 backlog（dock 布局支持，暂缓） |

> 归属模型差异（重要）：DBeaver 结果是**每个 SQL 编辑器自带 results panel**（多面板并存），
> 本项目拍板为**全局单 dock 联动活动文档**（单面板切换）。维持拍板；dock 内结果集标签条
> 覆盖多结果并存的大部分场景；未来要多文档并存时 dock 面板改「每文档 tab」数据层零成本
> （`ResultStore` 已按文档分槽）。

### 8.7 DBeaver 对齐 backlog（竞品分析合流，2026-10-24）

背景：用户提供竞品源码目录 `D:\RdataStation\RDS`（dbeaver-ref / rustgrid-ref / cellar-ref /
blanco-ref / tusk-ref 等），拍板「可以并实现，要文档记录，补充原型」。源码级竞品探查结论
见仓库根两份文档：`竞品分析-同架构竞品-编辑器与表格模块.md`（cellar/blanco/tusk/rustgrid 四家）
与 `竞品分析-多面板并发执行阻塞.md`（DBeaver 执行架构）。合并路线 = **视觉层对齐 DBeaver +
工程层移植四竞品可复用方案**，分三层：

**① 视觉层（DBeaver 当前格 / 行号 / 状态行）——已交付核对**

| 项 | DBeaver 行为 | 本项目现状 |
| --- | --- | --- |
| 行号列 | 左侧行号（随行高） | ✅ 已交付：面板自有 `#` 列（`grid.rs::is_row_number` + `Column::new("row-no","#")`），组件行头关闭（`row_header(false)`） |
| NULL 灰斜体 | NULL 灰斜体与字符串区分 | ✅ 已交付：`render_td` 判 `text == "NULL"` 灰 + 斜体（原型 §2.4） |
| 焦点格粗边框 | 当前格 2px 边框 + 高亮 | ✅ 本轮补丁：`vendor/gpui-component/src/table/state.rs` 选中格覆盖层加 `border_2` + `table_active_border`（组件仅背景高亮，补丁补边框） |
| 状态行当前格值 | 状态行左下显示当前格值 | ✅ 本轮实现：`ResultStatus.selected_cell` 字段 + `ResultPanel::set_selected_cell`（`SelectCell` 事件增量更新，值由编辑器读网格 delegate 算好传入，长值截断 48 字符） |
| 列头排序指示 | 排序箭头 | ✅ 组件内置（`render_sort_icon` + `Column.sort`） |
| 列头类型标签 / 注释 tooltip | 列头类型与元数据 | ✅ 已交付（P0 类型标签 + P1 分块 tooltip） |
| 列头筛选箭头菜单 | 每列箭头菜单 | ✅ 本轮实现：列头菜单加「按值筛选此列」段（该列去重取值前 8 个平铺 + 筛选生效时「清除此列筛选」项），点击即**列级本地筛选**（与全局筛选词叠加取交集，不动源库）；筛选生效的列名后加 muted `▽` 指示；工具栏「已筛选 N/M 行」统计跟随 |
| 左右分栏（列结构 / 值详情） | Data Editor 的左右辅助区 | ✅ 本轮实现（`ee3077fa`）：左栏列结构列表（列名 + 类型 + 单列隐藏切换，整行点击切换、隐藏列标灰）；右栏选中格值详情（完整值原样看全 + 列名·类型·位置，可滚动；选整行/未选中给空态）；工具栏「列结构」「值详情」开关（默认关不挤网格）；对比模式已双栏、侧栏让位 |

**② 工程层（四竞品可移植方案）——已交付 / backlog**

| 方案 | 竞品实证 | 来源 | 状态 |
| --- | --- | --- | --- |
| 大表窗口化取数 + generation 防抖动 | tusk：窗口取数 + 请求代际（旧响应作废） | `竞品分析-同架构竞品-编辑器与表格模块.md` | backlog |
| pending 变更合并 UPDATE 单事务 | blanco：批量变更集 → 一条 UPDATE 事务 | 同上 | backlog |
| 撤销 / 重做（快照） | tusk `History<T>` 快照栈 | 同上 | ✅ 已交付：`EditHistory` 双栈（`changes.rs`，每格编辑一步、新编辑清空重做栈、随结果集作废）+ 接线（提交入栈、`Ctrl+Z`/`Ctrl+Shift+Z` 恢复上一步值写回变更集并重投影，store `histories` 槽与 `changes` 同序） |
| 类型感知单元格编辑 | tusk：按列类型出编辑器/校验 | 同上 | ✅ 本轮交付**提交校验档**（`ee3077fa`）：数字列必须数字、布尔列限 `true/false/1/0/t/f/yes/no`、UUID 查 `8-4-4-4-12` 形状、日期查 `YYYY-MM-DD`+月日范围；非法拒绝提交（编辑态保留 + 红框 + 悬停错误文案），空 / `NULL` 放行（删值语义）；输入一变化即清错误。Select / Calendar 类型化编辑器（tusk 进一步）仍 backlog |
| 列布局持久化（顺序/宽度/隐藏） | blanco `ColumnLayout` 落盘 + 重放 | 同上 | 阻塞：跨会话落盘依赖会话文件（`editor_session.rs` 正被状态栏合并线占用），同会话列宽保留已达成 |
| IME / 输入法支持 | rustgrid：编辑器输入法路径 | 同上 | backlog |
| 列宽用户设定 | cellar `widths_user_set` | 同上 | backlog |
| 每执行一 Job + 连接级锁 | DBeaver：独立执行 Job、连接级互斥 | `竞品分析-多面板并发执行阻塞.md`（执行架构，与 execution-context-model 衔接） | 下一版本规划（execution-context-model） |

**③ 呈现扩展（DBeaver `IResultSetPresentation` 方向）——backlog**

Grid / Text / Record 多视图呈现：`ResultControls` / `ResultPane` 已是呈现接入点雏形
（§8.6 已记）；Text / Record 视图后续挂新呈现。

> 实施顺序建议：视觉层剩余项（列头筛选菜单）→ 工程层「列布局持久化」与「撤销/重做」
> （两者独立、收益最直接）→ 大表窗口化取数与 pending 单事务（动 engine 侧，与
> execution-context-model 下一版本规划衔接）。
>
> 进度（2026-10-24）：① 视觉层全部交付——焦点格粗边框 / 状态行当前格值 / 列头箭头菜单
> **+ 列头筛选（值列表筛选 + 清除项 + 列头▽指示，与全局筛选叠加）+ 左右分栏（列结构 / 值详情）**；
> ② 工程层「撤销/重做」「类型感知单元格编辑（提交校验档）」已交付；
> 「列布局持久化」被会话文件并行线占用阻塞。转置视图 = 单行 Record 呈现（只读，见 §6.1 网格细节图）。

### 8.8 选型记录：结果网格不引入 polars（2026-10-24 用户确认）

结论：**结果网格的数据层不引入 polars**。触发条件与替代路线见下。

**背景**：DBeaver 结果集面板右侧栏含数值查看器 / 计算 / 分组 / 引用 / 元数据，
评估是否引入 polars（Rust DataFrame 库）承载「本地列式聚合 / 分组」。

**判断依据**（按 DBeaver 功能逐项过）：

| 区域 | 功能 | 需要 polars？ | 现状 / 更优解 |
| --- | --- | --- | --- |
| 左栏·显示方式 | 网格 / 文本切换 | 不需要（呈现层） | 网格已有；文本视图 = 新呈现（§8.9） |
| 左栏·记录模式 | 单行记录视图 | 不需要 | 转置视图已实现（只读） |
| 右栏·数值查看器 | 单值详情 | 不需要 | 值详情栏已实现 |
| 右栏·计算 | count/null/min/max/avg/sum | 唯一真需要 | 列统计纯函数已实现（P2-3）；数据量小，纯函数足够 |
| 右栏·分组 | groupby 聚合 | 部分 | DBeaver 口径是**下发 GROUP BY 重查**，本地只做小结果集即时聚合 |
| 右栏·引用 | 外键引用 | 不需要 | 下发 SQL 查源库 |
| 右栏·元数据 | 列信息 / 注释 | 不需要 | P0/P1 已交付 |
| 状态栏 / 工具栏 | 刷新 / 保存 / 导出 / 分页 | 不需要（动作不是数据处理） | 变更集 + 分页已有 |

**三条否决理由**：
1. **结果网格是「交互表格」不是「计算引擎」**——gpui-kit DataTable 吃 `Vec<Vec<String>>`；
   polars 不解决渲染 / 编辑 / 选中 / 复制这些核心交互；当前编辑模型（变更集 + EditHistory
   快照 + pending 合并）是逐格变更语义，硬套 DataFrame 的不可变 / 表达式模型会两头打架。
2. **polars 与已有 DuckDB 双实例功能重叠**——分析模式 / EDA 走 DuckDB（内存列式 SQL 分析），
   polars 同为内存列式表达式引擎，两者是竞品不是互补；引入 polars = 分析栈多一套引擎。
3. **类型化收益 vs 改造成本不成比例**——polars 的增量价值是类型化排序 / 聚合（当前字符串网格
   `"10" < "9"` 的坑），受益面只有「计算 / 分组」一个功能点；代价是 ResultStore 从字符串网格换
   DataFrame 的全链路重构 + 大表全量载入违背「窗口化取数」方向 + polars 重型依赖拖编译。

**触发条件**（满足才重新评估）：结果集本地列式聚合 / 分组成为主场景（几十万行、用户反复切
分组维度、且不想下发 SQL 的只读结果）。届时先做 polars vs DuckDB（已有）实测对比——大概率
DuckDB 已够用；polars 只在需要 Rust 原生 pandas 式 API 时加分。

**既定路线**：分组 / 统计用纯函数补齐（可测、无新依赖）；大表分析继续走 DuckDB。

### 8.9 DBeaver 四区落地（2026-10-24，提交 817c4621）

DBeaver Data Editor 四区形态已落地（左栏 / 右栏 / 工具栏 / 状态栏），本小节记录本轮实现：

- **左栏·显示方式**：`Presentation { Grid, Text }`（面板级持续，切结果集不重置）；左栏按钮组 + 工具栏「文本」按钮 + 文本视图头部「网格」按钮，入口全用显式 `SetPresentation(Presentation)`（翻转语义在双按钮下会点反，故不用 toggle）。
- **左栏·记录模式**：`Switch` 与工具栏「转置」同状态（`ToggleTranspose`）；单行记录视图 = 转置视图（只读，Row view 可编辑是已知差距）。
- **文本视图**：可见列头 + `visible_rows()` 拼 TSV（隐藏列跳过、NULL 原样、`\t` 分隔），`mono_font_family` + `whitespace_normal` + 滚动；标签条保留（可切结果集）、侧栏让位。
- **右栏四页签**：`RightTab { Value, Stats, Group, Metadata }` + `TabBar::segmented().with_size(Small)`（kit 组件，不手搓）——值详情（原样看全）/ 计算（`column_stats`）/ 分组（`group_by_count` 首现顺序、NULL 独立组）/ 元数据（`column_annotations`：列名/类型/可见/来源表/来源列/注释/驱动类型/可空/默认值）。
- **结构常量**：`ui::RESULT_SIDEBAR_KV_LABEL_WIDTH`（4.5rem=72px，计算/元数据键值行标签列）。
- **性能注意**：计算 / 分组页每次渲染按当前可见行现算（render 纯读，无副作用），中小结果集可接受；几十万行本地聚合需缓存或下发 GROUP BY（§8.8 触发条件）。

**测试**：rds-result-panel 127 passed / 0 failed（+2：`text_presentation_skips_hidden_columns_and_follows_view_rows`、`group_by_count_keeps_order_and_null_group`）；rds-editor 586 passed / 1 failed（`named_parameters_hold_execution_until_filled` 归因并行线 engine 提交，与本线无交集，如实上报不修）。

### 8.10 左缘竖条 + 右栏页签新序与关闭（2026-10-24，#25）

对齐 DBeaver Data Editor 截图基准（左缘独立窄竖条三图标、右栏页签重排与可关闭、底部三段式动作组）。本轮落地竖条与页签段；底部三段式与本地行级动作口径见本节「后续项」。

**① 左缘竖条（替代原左栏视图区 + 工具栏两个入口按钮）**

- 布局：主内容行最左挂 `render_vertical_bar`（`panel.rs`），宽 `ui::RESULT_VERTICAL_BAR_WIDTH = 2.25rem`（36px），`flex_none`，`sidebar` 底色，右发丝线；有 / 无侧栏、对比模式、文本视图分支均挂主栏左侧。
- 三枚幽灵小钮竖排互斥：
  - 网格 `IconName::Table2` → `SetPresentation(Presentation::Grid)`；
  - 文本 `IconName::FileText` → `SetPresentation(Presentation::Text)`；
  - 记录 `IconName::UserRound` → `ToggleTranspose`，**文本态置灰**（记录视图只在网格呈现下可用）。
- **三态模型保留双轨**（拍板口径，未改语义）：`Presentation { Grid, Text }` 与 `ToggleTranspose` 仍是两个独立状态，竖条只做入口位置迁移，不合并成单枚举；记录钮高亮跟随 `transposed`。
- 记录激活时，竖条记录钮下方追加 `ChevronUp` / `ChevronDown` 上下行导航（`TransposePrev` / `TransposeNext`，动作队列原样复用）。
- 顶部工具栏的「文本」「转置」按钮段已删除；`ResultControls` 同步移除 `presentation` / `transpose` 字段；左栏 `render_column_sidebar` 去掉视图区，只留「列结构」标题 + 列名列表。

**② 右栏页签新序与关闭**

- `RightTab` 顺序重排为 `Value(0, 默认) / Stats(1) / Metadata(2) / Group(3)`，映射 `right_tab_from_index` 同步；`render_right_sidebar` labels = `["值详情", "计算", "元数据", "分组"]`，match 顺序 Value / Stats / Metadata / Group。
- 非默认「值详情」页时，页签行尾挂 `IconName::X` 关闭钮（`editor-result-right-tab-close`），点击入队 `SetRightTab(RightTab::Value)`。**拍板口径：× 只退回默认页，不收起整个右栏**；右栏开合仍由工具栏「值详情」开关控制。
- 实现备注：`TabBar` 的 suffix 槽内子按钮点击会冒泡触发页签切换（vendor `tab.rs` 已核实），故关闭钮挂在 TabBar 行右侧独立元素，不用 suffix 内嵌。
- 「引用」页（外键面包屑 + 引用表只读预览）本轮不做，留 backlog。

**③ 底部三段式动作组（目标形态，本轮仅原型先行；Rust 侧未改布局）**

对齐 DBeaver 截图基准的三段结构，原型 `docs/analysis/结果面板-DBeaver风格原型.html` 已呈现目标形态：

| 段 | DBeaver 内容 | 本轮口径 |
| --- | --- | --- |
| 左·动作组 | 刷新▾ / 保存 / 取消 / 新增行 / 复制行 / 删除行 / 撤销 / 重做 / 首页 / 上页 / 下页 / 末页 / 导出▾ / 设置 | 灰态占位，待动作模型落地后逐个接线（见下） |
| 中段 | 行数输入框（只读）+ `N+` 标记 | 待落地 |
| 右段 | `N 行已获取 - 总耗时 (获取耗时), 时间戳` + 当前格预览 + 已选行 | 现有状态行已含当前格预览 / 已选行；耗时元信息在工具栏段，待归位 |

**翻页导航口径（已拍板）**：取数是窗口化模型（`FetchMore` 取下一段，无页号概念），首页 / 上页 / 末页置灰占位，下页 = 现有 `FetchMore`；不实现段导航（需要 engine 支持 offset 重查，超出本线范围）。

**④ 本地行级动作（新增 / 复制 / 删除 + 行级撤销重做）——模型岔路，待确认后落地**

本轮**未写**代码。现状模型与阻塞点：

- 变更集只持格级 `CellChange(row, column, original, edited, status)`；`EditHistory` 是格级双栈；`commit_updates_rows` 只生成 UPDATE、依赖键列 WHERE 定位；无主键结果集整份只读。
- delegate 的 `view_rows: Option<Vec<usize>>` 是「底层数据行号 → 视图行」映射（`None` = 恒等）；本地新增行**没有底层行号可映射**，删除行需要在筛选 / 排序 / 导出 / 列统计 / 分组 / 值预览全链路分叉。
- INSERT 编码在导出层有单行先例（`export.rs`），DELETE 编码不存在；现有 `FetchMore` 窗口模型无页号。

**拍板的行为口径（确认模型方案后照此实现）**：

- 新增 / 复制 / 删除**只改本地网格与撤销栈，不进提交批次、不下发 INSERT / DELETE**；UPDATE 路径完全不动。
- 提交时本地行级变更被忽略并提示「行级变更暂未支持下发」。
- 无主键结果集（`editable = false`）禁用三个行级按钮。
- 撤销 / 重做优先格级 `EditHistory`，栈空再走本地行栈。

> 待确认方案点：本地叠加层（新增行集合 / 待删除行集合 / 行操作历史栈）挂在 delegate 还是 store 槽；视图行号映射如何扩展「底层行 / 本地新行」双态；删除行灰显 + 删除线样式与本地新增行的行号槽口径。确认前不硬写。

**测试基线（本轮回归）**：`cargo test -p rds-result-panel --lib` = **127 passed / 0 failed**；`cargo test -p rds-editor --lib` = **590 passed / 0 failed / 1 ignored**（1 ignored = 需本机 ipykernel 的 python notebook 用例；此前并行线 engine 提交导致的 1 个失败项本轮已恢复通过）。

- 提交只 add 表格模块相关文件；并行线未提交改动（`insight/*`、`plugin/*`、
  `crates/app/src/main.rs`、`crates/settings/*`、`workbench/src/components/insight_actions.rs`、
  `README`、`docs/architecture/connection/execution-context-model.md`、
  `Cargo.lock` 等）不随本模块提交。
- 回归以 `cargo test -p rds-result-panel -p rds-editor --lib` 为准；
  整包（`rds-workbench`）若被 insight 线未提交编译错误阻塞，如实上报而非绕过。
- 注释 / commit 信息简体中文（`feat:` / `fix:` / `refactor:` 前缀）。
