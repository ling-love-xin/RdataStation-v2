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
| 结果面板侧边栏 | 选中列信息（类型 / 字段注释 / 列统计）常驻显示，DBeaver Data Editor 右侧栏（用户明确延后，纯搬迁优先） |
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

**② 工程层（四竞品可移植方案）——已交付 / backlog**

| 方案 | 竞品实证 | 来源 | 状态 |
| --- | --- | --- | --- |
| 大表窗口化取数 + generation 防抖动 | tusk：窗口取数 + 请求代际（旧响应作废） | `竞品分析-同架构竞品-编辑器与表格模块.md` | backlog |
| pending 变更合并 UPDATE 单事务 | blanco：批量变更集 → 一条 UPDATE 事务 | 同上 | backlog |
| 撤销 / 重做（快照） | tusk `History<T>` 快照栈 | 同上 | ✅ 已交付：`EditHistory` 双栈（`changes.rs`，每格编辑一步、新编辑清空重做栈、随结果集作废）+ 接线（提交入栈、`Ctrl+Z`/`Ctrl+Shift+Z` 恢复上一步值写回变更集并重投影，store `histories` 槽与 `changes` 同序） |
| 类型感知单元格编辑 | tusk：按列类型出编辑器/校验 | 同上 | backlog |
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
> **+ 列头筛选（值列表筛选 + 清除项 + 列头▽指示，与全局筛选叠加）**；② 工程层「撤销/重做」
> 已交付；「列布局持久化」被会话文件并行线占用阻塞。

## 8. 协作纪律

- 提交只 add 表格模块相关文件；并行线未提交改动（`insight/*`、`plugin/*`、
  `crates/app/src/main.rs`、`crates/settings/*`、`workbench/src/components/insight_actions.rs`、
  `README`、`docs/architecture/connection/execution-context-model.md`、
  `Cargo.lock` 等）不随本模块提交。
- 回归以 `cargo test -p rds-result-panel -p rds-editor --lib` 为准；
  整包（`rds-workbench`）若被 insight 线未提交编译错误阻塞，如实上报而非绕过。
- 注释 / commit 信息简体中文（`feat:` / `fix:` / `refactor:` 前缀）。
