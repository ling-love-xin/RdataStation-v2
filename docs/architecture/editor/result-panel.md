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
| 结果面板侧边栏 | ✅ 早期交付（`ee3077fa`）：左右分栏——左栏列结构、右栏选中格值详情；该形态后被 §8.11 / §8.12 取代 |
| 列浏览器（右栏页签） | ✅ 本轮交付（§8.12）：列浏览器并入右侧辅助栏，与值详情 / 计算 / 分组同栏四页签切换；清单 ↔ 网格双向选中联动；隐藏走眼睛、冻结/排序/筛选/列统计走右键；左栏已整体移除、宽度全部还给网格；遗留：多选批量动作、列序拖拽 |
| 渲染改进批次（clip / 分段语义 / JSON 美化 / 列宽记忆 / 千分位 / 列级统计） | ✅ 本轮交付（§8.13）：单元格 300 字符裁剪、状态行分段提示、JSON pretty、会话内列宽记忆、整数千分位、计算 / 分组列级访问器；延后：COUNT 精确总数、列宽落盘、跨帧统计缓存、小数分组 |
| 事实层 / 显示层分离 · A 批底座 | ✅ 本轮交付（§8.14）：A1 `FactFrame` 类型与访问器落 shared；A2 `facts` 平行槽位 + `FactState` 三态；A3 `QueryData.fact` 双产物入槽；A4 `fact_storage` 三档偏好 + Auto 分流纯函数与高低水位；B（DuckDB 分析保真链 / 表档接通）、D（导出分级）待开工；C（IronCalc）待拍板；E（engine 第一跳失真）另立项 |
| 事实层 / 显示层分离 · B 批保真链 | ✅ 已交付（§8.15，提交 `84a81572`）：常驻表句柄回挂、分析三分支保真、内存档列式统计 / 排序 / 分组、表档占用可见、漂移注释勘误 |
| 事实层 / 显示层分离 · D1 导出分级 | ✅ 本轮交付（§8.16，提交 `83655b26`）：Parquet / XLSX 事实档三分支保真直灌，无事实档猜型降级档回执标注；文本档不动；typed JSON 新档后置 |
| 行级叠加层 · #28-B 接线 | ✅ 本轮交付（§8.10 ④）：视图序列 `RowRef` 分叉、新增 / 复制 / 删除本地行、行级撤销重做组合、删除线灰显、本地行格编辑入 `LocalRowSet`；全链路排除矩阵收口（本地行 / 被删行在统计 / 分组 / 导出 / 提交 / 分析各链路的纳入与排除口径）；遗留：锚点插入落点、本地行进统计分组、fact 原生整表聚合逐行排除 |
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
| `ToggleValueSidebar` | 工具栏右侧辅助栏开合钮（列浏览器 / 值详情 / 计算 / 分组同栏页签） | `toggle_value_sidebar` |
| `ToggleColumnVisibility(usize)` | 列浏览器清单眼睛 | host 改网格显隐 + 面板 `notify` |
| `SelectColumn(usize)` | 清单单击选中列 | `select_column`（写 `selected_col` + 网格列高亮） |
| `LocateColumn(usize)` | 清单双击定位 | `locate_column`（滚到该列首行并选中） |
| `ClearColumnSearch` | 清单搜索框 ✕ | `clear_column_search_via_window`（转手 `clear_column_search(window, cx)`） |
| `SetPresentation(Presentation)` / `SetRightTab(RightTab)` | 竖条显示方式 / 右栏页签 | `set_presentation` / `set_right_tab`（纯面板状态） |

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
| 右侧辅助栏（列浏览器 / 值详情 / 计算 / 分组） | Data Editor 的右侧辅助区（同栏页签切换） | ✅ 本轮实现（§8.12）：右栏四页签——列浏览器（搜索 + 清单 + 元数据明细 + 双向联动 + 右键列级动作）/ 值详情（完整值原样看全 + 列名·类型·位置，可滚动；选整行/未选中给空态）/ 计算（列统计）/ 分组（本地 groupby 计数）；工具栏单一开关控制整栏开合；对比模式让位双栏；左栏已整体移除、宽度全部还给网格 |

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
> **+ 列头筛选（值列表筛选 + 清除项 + 列头▽指示，与全局筛选叠加）+ 右侧辅助栏四页签（列浏览器 / 值详情 / 计算 / 分组）**；
> ② 工程层「撤销/重做」「类型感知单元格编辑（提交校验档）」已交付；
> 「列布局持久化」被会话文件并行线占用阻塞。转置视图 = 单行 Record 呈现（只读，见 §6.1 网格细节图）。

### 8.8 选型记录：结果网格不引入 polars（2026-10-24 用户确认）

结论：**结果网格的数据层不引入 polars**。触发条件与替代路线见下。

**背景**：DBeaver 结果集面板右侧栏含数值查看器 / 计算 / 分组 / 引用 / 元数据，
评估是否引入 polars（Rust DataFrame 库）承载「本地列式聚合 / 分组」。

**判断依据**（按 DBeaver 功能逐项过）：

| 区域 | 功能 | 需要 polars？ | 现状 / 更优解 |
| --- | --- | --- | --- |
| 竖条·显示方式 | 网格 / 文本切换 | 不需要（呈现层） | 网格已有；文本视图 = 新呈现（§8.9） |
| 竖条·记录模式 | 单行记录视图 | 不需要 | 转置视图已实现（只读） |
| 右栏·数值查看器 | 单值详情 | 不需要 | 值详情页签已实现 |
| 右栏·计算 | count/null/min/max/avg/sum | 唯一真需要 | 列统计纯函数已实现（P2-3）；数据量小，纯函数足够 |
| 右栏·分组 | groupby 聚合 | 部分 | DBeaver 口径是**下发 GROUP BY 重查**，本地只做小结果集即时聚合 |
| 右栏·引用 | 外键引用 | 不需要 | 下发 SQL 查源库（backlog） |
| 右栏·元数据 | 列信息 / 注释 | 不需要 | 并入列浏览器页签底部明细（§8.12）；独立 Metadata 页签已下线 |
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

### 8.17 列头类型徽标 + 注释直接显示（A1/B1/B2）

用户拍板：列头类型从「列名右侧 muted 纯文字」改成「色块短码 Tag 前置」；字段注释从「只在
hover tooltip」改成「列名下方第二行小字可开关」。

**类型族分类（`grid.rs::type_kind_short` / `type_kind_tag`）**：

| 类型族 | 覆盖 ColumnKind | 短码 | Tag 变体 |
| --- | --- | --- | --- |
| 数字 | Integer / Float / Decimal | INT / FLT / DEC | Primary（蓝） |
| 文本 | Text / Uuid / Json | TXT / UUID / JSON | Success（绿） |
| 时间 | Timestamp / Date / Time | TS / DATE / TIME | Info（紫） |
| 布尔 | Boolean | BOOL | Warning（橙） |
| 二进制 | Binary | BIN | Secondary（灰） |
| 未知 | Unknown | — | 不画 badge |

**A1 色块短码 Tag**（本轮已落地）：列头顺序 = `[badge] [列名(flex_1, truncate)] [筛选▽]`。
用 gpui-kit `Tag::new().with_variant(..).small().child(short)`——主题自动处理明暗底色/文字色，
零裸色值。

**A2 纯图标版**（待用户在原型里选）：不画底色文字，只画一个 8px 方块按族着色。原型
工具栏有 `A1 / A2` 切换按钮可并排看效果。

**B1 面板级临时开关**：工具栏「注释」Switch（跟「下发源库」并排），`ResultRequest::
SetShowColumnComments { open }` 经队列回 host 调 `panel.set_show_column_comments(open)`。
开关开启时列头变两行：第一行 badge+列名+筛选▽，第二行 `muted text_xs` 注释小字（截短
ellipsis，hover tooltip 看全——tooltip 已含注释分块，不重复造）。注释为空时不占位（列头
回到单行高）。

**B2 全局偏好**：`settings` 注册 `result_panel.show_column_comments`（Bool，默认 false）。
面板启动时 host 从 `cx.try_global::<Settings>()` 读偏好同步给面板；用户在面板工具栏切换
后是面板级临时态，不回写全局——全局只控「默认开不开」。

**边界**：转置视图 / 行号槽不画类型 badge 也不画注释；列宽拖拽 / 冻结 / 隐藏不受影响。

**A3 定稿（用户拍板）**：放弃 A1 实色 Tag，改成浅底 + 彩色符号：

| 族 | 符号 | 浅底色 |
| --- | --- | --- |
| 数字（Int/Float/Decimal） | `123` | primary 18% 透明度底 + primary 字 |
| 文本（Text/Uuid/Json） | `Aa` | success 18% 透明度底 + success 字 |
| 时间（Timestamp/Date/Time） | `◷` | info 18% 透明度底 + info 字 |
| 布尔（Boolean） | `✓` | warning 18% 透明度底 + warning 字 |
| 二进制（Binary） | `01` | secondary 25% 透明度底 + secondary 字 |

实现：`grid.rs::type_kind_sym()` 返回符号字符串；render 时 match kind 取 theme.colors 族色，
`div().bg(color.opacity(0.18)).text_color(color).px_1().text_xs().rounded(3.)` 手搓 badge。
不依赖 Tag 组件（Tag 实色太重），零裸色值（族色全走 theme.colors）。

**空结果集保留列头**：DataTable 组件在 rows=0 时切到 `render_empty` 不画表头；用户要求
0 行也要看到字段名（DBeaver 口径）。方案：`set_data` 时若 rows 为空且 columns 非空，
塞一行全空字符串（`vec![vec![String::new(); columns.len()]]`），让组件画表头 + 一行空白数据区。
空态文案在状态栏/工具栏显示，不在网格里画。

**列宽 backlog（未实现，DBeaver 有）**：
- 双击列头边缘自动适应内容宽度（auto-fit）
- 右键列头「设置列宽」输入数值
- 列宽跨会话持久化（当前只会话内按列名记）

**既定路线**：分组 / 统计用纯函数补齐（可测、无新依赖）；大表分析继续走 DuckDB。

### 8.9 DBeaver 四区落地（2026-10-24，提交 817c4621）

> 历史形态记录：本节「左栏显示方式 / 记录模式」后由 §8.10 ① 迁入左缘竖条；「右栏四页签 Value/Stats/Group/Metadata」经 §8.11 删 Metadata、§8.12 重排为 Browser/Value/Stats/Group。当前栏位与页签口径以 §8.12 为准。

DBeaver Data Editor 四区形态已落地（左栏 / 右栏 / 工具栏 / 状态栏），本小节记录本轮实现：

- **左栏·显示方式**：`Presentation { Grid, Text }`（面板级持续，切结果集不重置）；左栏按钮组 + 工具栏「文本」按钮 + 文本视图头部「网格」按钮，入口全用显式 `SetPresentation(Presentation)`（翻转语义在双按钮下会点反，故不用 toggle）。
- **左栏·记录模式**：`Switch` 与工具栏「转置」同状态（`ToggleTranspose`）；单行记录视图 = 转置视图（只读，Row view 可编辑是已知差距）。
- **文本视图**：可见列头 + `visible_rows()` 拼 TSV（隐藏列跳过、NULL 原样、`\t` 分隔），`mono_font_family` + `whitespace_normal` + 滚动；标签条保留（可切结果集）、侧栏让位。
- **右栏四页签**：`RightTab { Value, Stats, Group, Metadata }` + `TabBar::segmented().with_size(Small)`（kit 组件，不手搓）——值详情（原样看全）/ 计算（`column_stats`）/ 分组（`group_by_count` 首现顺序、NULL 独立组）/ 元数据（`column_annotations`：列名/类型/可见/来源表/来源列/注释/驱动类型/可空/默认值）。
- **结构常量**：`ui::RESULT_SIDEBAR_KV_LABEL_WIDTH`（4.5rem=72px，计算/元数据键值行标签列）。
- **性能注意**：计算 / 分组页每次渲染按当前可见行现算（render 纯读，无副作用），中小结果集可接受；几十万行本地聚合需缓存或下发 GROUP BY（§8.8 触发条件）。

**测试**：rds-result-panel 127 passed / 0 failed（+2：`text_presentation_skips_hidden_columns_and_follows_view_rows`、`group_by_count_keeps_order_and_null_group`）；rds-editor 586 passed / 1 failed（`named_parameters_hold_execution_until_filled` 归因并行线 engine 提交，与本线无交集，如实上报不修）。

### 8.10 左缘竖条 + 右栏页签新序与关闭（2026-10-24，#25）

对齐 DBeaver Data Editor 截图基准（左缘独立窄竖条三图标、右栏页签重排与可关闭、底部三段式动作组）。#25 落地竖条与页签段；#26 落地底部三段式视图骨架（③），本地行级动作模型归 #28（④）。

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

> 历史记录：当时序为 Value/Stats/Metadata/Group；Metadata 页签后于 §8.11 删除，§8.12 重排为 Browser/Value/Stats/Group、默认页改为 Browser。当前口径以 §8.12 为准。

- `RightTab` 顺序重排为 `Value(0, 默认) / Stats(1) / Metadata(2) / Group(3)`，映射 `right_tab_from_index` 同步；`render_right_sidebar` labels = `["值详情", "计算", "元数据", "分组"]`，match 顺序 Value / Stats / Metadata / Group。
- 非默认「值详情」页时，页签行尾挂 `IconName::X` 关闭钮（`editor-result-right-tab-close`），点击入队 `SetRightTab(RightTab::Value)`。**拍板口径：× 只退回默认页，不收起整个右栏**；右栏开合仍由工具栏「值详情」开关控制。
- 实现备注：`TabBar` 的 suffix 槽内子按钮点击会冒泡触发页签切换（vendor `tab.rs` 已核实），故关闭钮挂在 TabBar 行右侧独立元素，不用 suffix 内嵌。
- 「引用」页（外键面包屑 + 引用表只读预览）本轮不做，留 backlog。

**③ 底部三段式动作组（#26 Rust 视图骨架已落地；动作模型归 #28）**

对齐 DBeaver 截图基准的三段结构，`grid.rs` 状态行（`editor-result-status-row`）已重排为左动作组 / 中段行数框 / 右段元信息；原型 `docs/analysis/结果面板-DBeaver风格原型.html` 同步目标形态：

| 段 | DBeaver 内容 | #26 落地口径 |
| --- | --- | --- |
| 左·动作组 | 刷新▾ / 保存 / 取消 / 新增行 / 复制行 / 删除行 / 撤销 / 重做 / 首页 / 上页 / 下页 / 末页 / 导出▾ / 设置 | 12 枚占位幽灵钮全部 `disabled(true)`（纯函数 `status_action_placeholders` 给清单与顺序，分组分界 `FIRST_PAGER_ACTION=7` / `FIRST_SYSTEM_ACTION=10`）；写操作段 保存·取消·新增行·复制行·删除行·撤销·重做 ‖ 翻页段 首页·上页·**下页=现有 `FetchMore` 真钮**·末页 ‖ 系统段 导出·设置；段间发丝分隔线。刷新 / 导出 / 提交 / 撤销重做的底部入口待 #28 行级模型落地后逐个接线，本轮不重复挂载顶部已有真钮 |
| 中段 | 行数输入框（只读）+ `N+` 标记 | 已落地只读槽位（`ui::RESULT_ROW_COUNT_BOX_WIDTH = 4.5rem`），显示 `status_segments().0`（共 N[+] 行）；行号跳转输入是 backlog |
| 右段 | `N 行已获取 - 总耗时 (获取耗时), 时间戳` + 当前格预览 + 已选行 | 保留现有当前格预览 / 截断警告 / 已选行；耗时元信息仍在工具栏段，归位右段留 backlog |

**翻页导航口径（已拍板）**：取数是窗口化模型（`FetchMore` 取下一段，无页号概念），首页 / 上页 / 末页置灰占位，下页 = 现有 `FetchMore`；不实现段导航（需要 engine 支持 offset 重查，超出本线范围）。

**④ 本地行级动作（新增 / 复制 / 删除 + 行级撤销重做）——方案 A 已拍板，叠加层模型归 #28**

2026-10-24 用户拍板：#26 先落地底部三段式**视图骨架**（③ 已完成）；叠加层模型与动作接线归下一轮任务 #28：

| 决策点 | 拍板口径 |
| --- | --- |
| 模型方案 | **A 叠加层**：底层 `rows` 与格级 `ChangeSet` 完全不动；行身份扩成 `RowRef { Data(底层号) \| Local(稳定 id) }`，`view_rows` 映射同步扩展；本地行格编辑单独存、不进提交批次；行级动作走独立 `RowHistory` 双栈；删除只打标记不真删，底层行号不滑移。否决方案 B（新增直接进 `rows` + 行号偏移账本平移）——全链路平移错位风险高、本地行易漏排除混入下发 / 统计 |
| 删除行呈现 | 灰显 + 删除线保留在网格内，撤销可恢复（对齐 DBeaver 默认；不直接移出网格） |
| 新增行落点 | 插到当前选中行紧下方；无选中行时追加末尾（对齐 DBeaver 默认） |
| 撤销 / 重做 | 单组按钮：格级 `EditHistory` 优先，栈空再走本地行栈 |
| 权威层归属（2026-10-24 二次确认，**修正**草图「delegate 内」口径） | 本地行集合 + 删除标记 + 行级历史挂 **store 结果集槽位**（与 `ChangeSet` / `EditHistory` 并列）；delegate 只在 `sync` 时投影、不持本地行真值。原因：`set_data` 每次投影重建 delegate 行状态，本地行挂视图层会在切标签 / 提交 / 撤销 / 重投影后丢失；与现有「delegate 只投影、不缓存第二份真值」约定一致 |
| 纳入范围（2026-10-24 确认；**2026-10-26 修正**：排序 / 分组 / 统计本期排除本地行，见下方 B5 排除矩阵） | 本地行进入网格视图（筛选命中才显示 / 预览 / 转置 / 复制 TSV / 文本视图）；**被删行全链路排除**（仅网格内灰显删除线可见，不进分组 / 统计 / 导出 / 复制）；提交批次与 SQL 导出一律排除本地行与被删行，不下发 INSERT / DELETE。排序只作用底层未删行（本地行锚定末尾区段），统计 / 分组本期排除本地行，后置批次再补 |
| 交付边界（2026-10-24 确认） | 分两轮：本轮（#28-A）只做模型层——`RowRef` + `LocalRowSet` + `RowHistory` + store 槽位 + 纯函数单测；动作按钮接线、删除线渲染、本地行格编辑、全链路 `RowRef` 分叉归下一轮（#28-B） |

**行为口径（实现时照此）**：

- 新增 / 复制 / 删除**只改本地网格与撤销栈，不进提交批次、不下发 INSERT / DELETE**；UPDATE 路径完全不动。
- 提交时本地行级变更被忽略并提示「行级变更暂未支持下发」。
- 无主键结果集（`editable = false`）禁用三个行级按钮。

**现状模型与阻塞点（#28 实现背景）**：

- 变更集只持格级 `CellChange(row, column, original, edited, status)`；`EditHistory` 是格级双栈；`commit_updates_rows` 只生成 UPDATE、依赖键列 WHERE 定位；无主键结果集整份只读。
- delegate 的 `view_rows: Option<Vec<usize>>` 是「底层数据行号 → 视图行」映射（`None` = 恒等）；本地新增行**没有底层行号可映射**，删除行需要在筛选 / 排序 / 导出 / 列统计 / 分组 / 值预览全链路分叉到 `RowRef`。
- INSERT 编码在导出层有单行先例（`export.rs`），DELETE 编码不存在；现有 `FetchMore` 窗口模型无页号。

**#28-A 模型层已落地（2026-10-24，本轮只交付模型 + 单测，无可见 UI 变化）**：

- 新模块 `crates/result_panel/src/rowset.rs`：
  - `RowRef { Data(usize), Local(u64) }`：行身份判别 / 取值访问器（`data_index` / `local_id` / `is_data` / `is_local`）；本地行 id 与底层行号不做偏移账本（方案 B 已否决）。
  - `LocalRow { id, cells }`：本地行快照（新增 = 全空；复制 = 来源行展示值快照，独立拷贝、不引用底层行）。
  - `LocalRowSet`：本地行登记 / 取号 / 移除快照 / 本地行就地改值；底层行删除标记 `BTreeSet<usize>`（`mark_deleted` / `unmark_deleted` / `is_deleted`）；`clear` 清空集合但 id 计数器不归零（新 id 继续递增，不与已作废快照撞号）。
  - `RowOp`：行级动作快照（`InsertLocal { local_id, anchor }` / `DuplicateLocal { local_id, source, snapshot, anchor }` / `DeleteData(usize)` / `RemoveLocal { local_id, cells, anchor }`）；`anchor` = 锚定底层行号（插其下方，`None` = 末尾），撤销重做恢复位置用，落点排序归下一轮视图接线。
  - `RowHistory`：行级撤销 / 重做双栈（`push` 清空重做栈、`undo` / `redo` / `can_undo` / `can_redo` / `clear`）；与格级 `EditHistory` 并列、互不混栈。
  - `visible_refs(data_row_count, local)`：基准叠加序列 = 底层未删行按原序 + 本地行按登记序；被删行不出现。筛选 / 排序 / 锚点落点由视图层在序列上再加工。
- `store.rs` 槽位：`DocumentResults` 新增 `locals: Vec<LocalRowSet>`、`row_histories: Vec<RowHistory>`，与 `sets` / `changes` / `histories` 同序同增减；`Replace` / `NewSet` / `Append`（形状不符替换）/ `evict_oldest` 生命周期同步清空 / 淘汰；访问器 `local_rows` / `mutate_local_rows` / `row_history` / `mutate_row_history`（不存在返回 `false`，口径同 `mutate_change_set`）。`Append` 可追加路径不清空本地叠加层（与格级变更集现状一致：底层行追加，本地行保留）。

**#28-B 接线已落地（2026-10-26，B2 → B4 → B3 → B5 顺序；模型层 #28-A 见上节）**：

| 批次 | 内容 | 源码锚点 |
| --- | --- | --- |
| B2 视图序列分叉 | delegate `view_rows: Option<Vec<RowRef>>` 由 `Vec<usize>` 分叉；`row_ref` / `data_row` / `cell` / `row_cell_text` / 脏格 / 选中态 / 右键目标 / 转置三方法 / 行文本 / JSON / INSERT / 选区文本全部按 `RowRef` 身份分叉；`GridPayload.local` 快照随 `sync` 注入，对比栏不传本地快照（D8 排除）；转置步长夹取基准改 `view_len` | `crates/result_panel/src/grid.rs`（`rebuild_view_rows` / `row_ref` / 各取值分叉）；`crates/result_panel/src/panel.rs`（`GridPayload.local` + `sync` 注入）；`crates/editor/src/view/host.rs`（投影元组带本地快照） |
| B4 删除线渲染 + 本地格编辑 | 被删行灰显 + 元素层 `line_through()` 删除线保留在网格内；本地新增 / 复制行走专用底色 token `result.local_row_background`（明暗两套，缺失回退 `theme.colors.accent`）；本地行格编辑提交走独立 `CommitLocalEditHook`，只写 `LocalRowSet.cells`，**不进**格级 `ChangeSet`、**不进**格级 / 行级撤销栈；`begin_edit` 闸口拒绝本地行以外的被删底层行；右栏值详情标题按身份标注「本地行 / 已删除」 | `crates/result_panel/src/grid.rs`（`on_commit_local_edit` / `begin_edit` / `commit_edit` 分叉 / `render_td` 底色与删除线叠加）；`crates/result_panel/src/panel.rs`（身份标注）；`crates/editor/src/view/host.rs`（本地格编辑钩子注入）；`crates/workbench_shell/src/product_tokens.rs` + `assets/themes/product-tokens.json`（本地行底色角色） |
| B3 行级动作接线 | 端口五变体 `InsertRowBelow(Option<usize>)` / `DuplicateRow(usize)` / `DeleteRow(usize)` / `UndoAction` / `RedoAction`；右键菜单在复制段之后、排序段之前插入行级独立分组（`separator_before: true`），`editable && !is_deleted` 闸口；`RowActionHook` + delegate 访问器 `reference_of`；底部五槽真钮；宿主五方法 `insert_local_row` / `duplicate_local_row` / `delete_local_row` / `undo_row_action` / `redo_row_action`；撤销 / 重做组合语义 = 格级 `EditHistory` 非空先走格级，栈空再走 `RowHistory`，两栈互不混栈；`ResultStatus.total_rows = row_count - deleted_count + local_count`；无主键结果集（`editable = false`）三个行级入口同步禁用 | `crates/result_panel/src/ports.rs`（五变体）；`crates/result_panel/src/grid.rs`（`ContextTarget.row_ref/editable/is_deleted` / `ContextAction` 三变体 / `context_menu_items` 行级分组 / `on_row_action` / `reference_of`）；`crates/result_panel/src/panel.rs`（`ResultSyncInput.can_undo/can_redo` + 五槽真钮）；`crates/editor/src/view/host.rs`（钩子注入 / 五宿主方法 / dispatch 五分支 / 组合态投影） |
| B5 全链路排除收口 | 按 §8.10 ④ 拍板矩阵逐链路口径收口：网格渲染保留被删行灰显删除线可见；全局词 / 列级筛选命中本地行才显示、被删行排除；列排序只作用底层未删行；统计 / 分组本期排除本地行、排除被删行（fact 原生整列聚合在被删集合非空时回退矩阵分支）；复制单格 / 整行 TSV / 复制列 / 行 JSON 纳入本地行、排除被删行；整行 INSERT 排除本地与被删；工具栏整份复制 `active_result_tsv()` 纳入本地行、排除被删行；文本视图 / 转置纳入本地行、排除被删行；CSV / JSON / INSERT / Updates / Where 与 Parquet / XLSX 导出序列 = 底层未删行（本地 / 被删都排除）；提交 UPDATE 批次过滤被删底层行的格级变更，本地行天然不在 `ChangeSet`；提交成功回执追加「本地新增 / 复制 / 删除的行未随本次提交落库」提示 | `crates/result_panel/src/grid.rs`（`visible_rows` / `visible_rows_for_export` / `column_text` / `column_values` / `column_stats_for` / `group_by_for` / `selection_text` / `row_text` / `row_json_text` / `row_insert_text` / `transpose_cell` / `find_next` / `active_result_tsv`）；`crates/editor/src/view/host.rs`（`copy_active_result` 走 `active_result_tsv`、`visible_rows_for_export` 恒返回底层未删序列、`submit_pending_changes` 过滤被删 pending + `local_pending` 探测、`CommitInFlight.local_pending` + 回执文案） |

**排除矩阵（B5 收口口径，实现照此）**：

| 链路 | 本地行 | 被删行 |
| --- | --- | --- |
| 网格渲染 | 纳入（专用底色） | 灰显 + 删除线保留可见 |
| 全局词 / 列级筛选 | 命中才显示 | 排除 |
| 列排序 | 排除（始终锚定末尾区段） | 排除 |
| 列统计 / 分组 | 本期排除（后置批次补） | 排除 |
| 复制单格 / 整行 TSV / 复制列 / 行 JSON | 纳入 | 排除 |
| 整行 INSERT | 排除 | 排除 |
| 工具栏整份复制（CopyTsv） | 纳入 | 排除 |
| 文本视图 / 转置视图 | 纳入 | 排除 |
| CSV / JSON / INSERT / Updates / Where 导出 | 排除 | 排除 |
| Parquet / XLSX 导出 | 排除 | 排除（fact 原生 batches 整表对齐，降级档矩阵序列排除） |
| 提交 UPDATE 批次 | 排除（不进 `ChangeSet`） | 排除（pending 过滤） |
| 本地 DuckDB 分析 / 并排对比栏 | 排除 | 排除（fact 原生整表聚合无法逐行排除，属已知边界，后置批次补行集合过滤） |

**口径边界（#28-B 守住的不做项）**：

- 新增 / 复制 / 删除只改本地网格与撤销栈，不进提交批次、不下发 INSERT / DELETE；UPDATE 路径完全不动；提交时本地行级变更被忽略并在回执提示「未随本次提交落库」。
- 本地行格编辑不进任何撤销栈（格级 `EditHistory` 与行级 `RowHistory` 都不进），提交即落快照；格级撤销 / 重做与行级撤销 / 重做经单组按钮组合分派，两栈互不混栈。
- 本地行 `cells` 列数 = `entry.columns.len()`；新增行全空串（不套 NULL 灰斜体，NULL 判定 = 文本 `NULL`）；复制行带源行展示值快照；本地行同列套同一套 `validate_edit_value` 类型校验，失败保留编辑态。
- 已知边界（后置批次）：① 锚点插入落点本期只落末尾区段，`RowOp.anchor` 作为历史元信息保留，未实现锚定行下方插入；② 本地行纳入统计 / 分组后置；③ fact 原生整列 / 整表聚合（统计原生分支、分析临时表、Parquet/XLSX batches 直灌）无法逐行排除被删行，本期被删集合非空时统计原生分支回退矩阵，分析与带类型导出原生路径保留全量行集合；④ 转置导航步长基准含被删行，锚到被删行时取值落空（后置批次在导航步长上跳过被删行）。
- 编辑器状态栏并入全局状态栏属另一会话任务，本线未动；insight / plugin / 分析资源等并行线改动不随本线提交。

**测试基线（#28-B 回归，2026-10-26）**：

- `cargo test -p rds-shared --lib` = **41 passed / 0 failed**；`cargo test -p rds-settings --lib` = **35 passed / 0 failed**（locale 用例组合并行偶发，单跑复现通过，非本线回归）。
- `cargo test -p rds-result-panel --lib` = **195 passed / 0 failed**（#28-A 147 + B2/B4/B3/B5 新增 48 例，含 B5 排除矩阵 6 例：文本视图纳入本地排除被删、导出序列双排除、统计取值双排除、整份复制纳入本地排除被删、复制列口径、被删行复制落空）。
- `cargo test -p rds-editor --lib` = **603 passed / 0 failed / 1 ignored**（1 ignored = 需本机 ipykernel 的 python notebook 用例）。
- `cargo test -p rds-engine --lib` = **660 passed / 0 failed / 24 ignored**；`cargo test -p rds-workbench --lib analysis_tests` = **4 passed / 0 failed**；`cargo test -p rds-workbench --lib export_tests` = **4 passed / 0 failed**。
- 整包 `cargo test -p rds-workbench` 未随本线执行（insight 线未提交 test-cfg 可能阻塞整包，被阻塞时如实上报不绕过）。

### 8.11 列浏览器（方案 A：左栏搜索 + 清单 + 底部明细，2026-10-24，已被 §8.12 取代）

> 本节为方案 A 初版（列浏览器挂左栏）的历史记录。**布局方向后于 2026-10-24 变更**：左栏挤占网格宽度、多列时对不齐；复查本地 DBeaver 源码确认 `resultset/panel/` 五个面板（valueviewer / aggregate / grouping / metadata / references）全部位于右侧、页签切换。列浏览器已并入右栏四页签，左栏整体移除——当前口径见 §8.12。调研过程与 DBeaver 源码对照见 `docs/analysis/dbeaver-列交互调研与列浏览器方案.md`。

**方案选型（初版，已被 §8.12 推翻布局归属）**：三方案 A（改造现有左栏为列浏览器，明细归左栏）/ B（独立浮动面板）/ C（清单与明细上下独立两区），当时拍板 **A**；本轮布局方向变更后，A 的「清单 + 明细上下分区」内部结构保留，归属由左栏改为右栏页签。

**拍板口径（5 点决策，不可擅自回退）**：

1. 清单行只留眼睛（隐藏开关，单击眼睛**不改变**选中列）；冻结 / 排序 / 隐藏 / 列统计 / 复制列名 / 显示全部走右键菜单；行上不摆图钉、排序箭头、拖拽手柄。
2. 列顺序拖拽本轮不做（kit 手势 + `move_column` 钩子现成；列序模型与隐藏 / 冻结 / 转置映射改造紧随下一轮）。
3. 搜索匹配范围 = **列名 + 类型名 + 注释**（忽略大小写包含匹配；DBeaver 原版只匹配列名，本轮加宽）；空词全量原序；隐藏列保留在清单（置灰斜体 + 眼睛关闭态）；无匹配摆空态「无匹配列」。
4. 本轮清单只**单选**；多选批量动作（批量隐藏 / 冻结 / 复制列名）下一轮。
5. （初版决策）右栏 `RightTab::Metadata` 页签与对应 `SetRightTab` 分支直接删除，不留兼容分支；右栏只留 值详情 / 计算 / 分组。**§8.12 变更**：列浏览器以 `RightTab::Browser` 作为新默认页签加回右栏，重排为 Browser / Value / Stats / Group。

**状态模型**：

- 面板级 `selected_col: Option<usize>`（**数据列序**，与 `hidden` / `frozen` 同一套下标空间）；清单选中 ≠ 网格焦点（清单可保持选中而网格焦点丢失）；明细只读 `selected_col`，不再读 `selection_pos`；新结果集 / 切标签 / 对比模式重置为 `None`（渲染兜底第一可见列）。
- 两个写入方向各走各的字段，不存在环，无需 `syncing` 标志位（比 DBeaver `updateSelection` 防回环简化）：
  - 清单单击 → 队列 `SelectColumn` → `select_column`：写 `selected_col` + `set_selected_col` 网格列高亮；**不回写** `selection_pos`（值详情仍只跟网格格焦点）。
  - 网格选中格 → `set_selection_pos`：经 `data_column(col - 1)` 反推数据列序同步 `selected_col`，驱动清单高亮与底部明细。
  - 清单双击 → 队列 `LocateColumn` → `locate_column`：`set_selected_cell(0, view_col, cx)` 滚到首行该列。
- 搜索词 `column_search` 跨结果集保留（与全局筛选框同款：`sync` 无 `window`，`InputState::set_value` 需要 `window`，故 `sync` 不清空；清空走搜索框 ✕ 入队 `ClearColumnSearch`，host 经窗口句柄转手 `clear_column_search(window, cx)`）。

**布局**：

- 列浏览器页签三区块（归属见 §8.12）：标题行「列浏览器 · N 列」→ 搜索框（`Size::Small` + Search 前缀 + 有词才摆 ✕）→ 清单滚动区（行 = 列名 `flex_1` 省略 + 类型名次级色 + 眼睛；选中底 `list_active`，隐藏列 `muted` + 斜体）→ 底部明细定高 `ui::RESULT_BROWSER_DETAIL_HEIGHT = 7.75rem`（标题行 + 九行键值，超出内部滚动）。
- 明细字段：列名 · 类型 / 可见 / 冻结 / 来源表 / 来源列 / 注释 / 驱动类型 / 可空 / 默认值（来自 `column_annotations()`）；无可见列给空态「暂无列」。
- 右栏计算 / 分组页作用列：网格选中格优先（`selection_pos`，经 `data_column` 换算）、清单选中列兜底；转置视图不给语义列；辅助方法 `inspect_column(delegate) -> Option<usize>`。

**菜单分层**（`column_browser_menu_items` 纯函数，与列头菜单共用 `ContextTarget`）：

- 本地动作（面板内直调网格实体）：复制此列（`ClipboardItem`，列文本闭包外算好）/ 冻结 · 解冻 / 隐藏此列 / 列统计 / 显示全部列（有隐藏列才摆）/ 取消排序（排序生效才摆）。
- **不摆进清单**：排序下发（依赖宿主 `on_sort_down` 钩子）、列级筛选、洞察此列、值与行动作（属列头 / 格交互，清单入口不生成）。
- 列统计复用既有 `on_column_stats` 钩子：构造菜单期 `column_stats_payload(column)` 取好 `(hook, name, text)`，点击直调 `hook(name, text, app)`，**不包** `grid_entity.update` 闭包（避免闭包内二次可变借用 `app`，与列头菜单既有范式一致）。

**端口与结构常量**：

- 新增 `SelectColumn(usize)` / `LocateColumn(usize)` / `ClearColumnSearch`；当时 `RightTab` 为 `Value(0, 默认) / Stats(1) / Group(2)`。**§8.12 重排**：`Browser(0, 默认) / Value(1) / Stats(2) / Group(3)`，映射 `right_tab_from_index` 同步。
- `ui::RESULT_BROWSER_DETAIL_HEIGHT = 7.75`（明细区定高倍率，§8.12 沿用）。

**测试基线（§8.11 本轮回归，历史记录）**：`cargo test -p rds-result-panel --lib` = **155 passed / 0 failed**（基线 147 + 新增 8 例：`column_browser_menu_items` 3 例——本地动作子集 / 条件项出现条件 / 动作归属与列号映射；`filter_browser_columns` 5 例——空词全量原序 / 列名忽略大小写 / 类型名 / 注释 / 无匹配空）。§8.12 迁移后基线为 **156 passed / 0 failed**（见下节）。`cargo test -p rds-editor --lib` 本轮被并行线「函数手册 #54-C5」未提交改动阻塞（`host.rs` 重复 `IconName` 导入 E0252 + 未跟踪 `function_manual.rs` 测试模块通配导入 recursion limit），非本线因果；本线 host.rs 仅 drain 段分派小段 + 窗口转手方法，写法与既有 `ToggleColumnVisibility` / `clear_filter_via_window` 同范式，待并行线解阻后复跑。

**下一轮 backlog**：多选批量动作；列顺序拖拽（`move_column` 钩子 + 列序模型与隐藏 / 冻结 / 转置映射改造）；搜索词是否跨结果持久化的口径复核；清单行类型图标（替代类型名文本后缀）。

### 8.12 列浏览器并入右栏四页签（2026-10-24，本轮拍板）

**背景与方向变更**：§8.11 初版把列浏览器挂在左栏，落地后发现左栏固定宽（210px 档）挤占网格宽度，多列时列头 / 单元格对不齐；复查本地 DBeaver 源码（`dbeaver-ref/.../resultset/panel/`）确认五个面板 `valueviewer` / `aggregate` / `grouping` / `metadata` / `references` **全部位于右侧、同栏页签切换**，此前把 metadata 归左栏本就偏离 DBeaver 原始形态。经决策确认：列浏览器迁入右栏，与值详情 / 计算 / 分组同栏四页签；本轮同步迁移 Rust 代码与 HTML 原型（不采用「先原型 + 文档、Rust 下一轮迁」的备选）。

**拍板口径（不可擅自回退）**：

1. 右栏四页签顺序：**列浏览器 Browser(0, `#[default]`) / 值详情 Value(1) / 计算 Stats(2) / 分组 Group(3)**；构造初始即 `Browser`。
2. **移除左栏**：旧 `show_column_sidebar` 字段、`ToggleColumnSidebar` 端口、`render_column_sidebar` 左栏外壳（固定宽 + `border_r` + `sidebar` 底色）整体取消；网格左侧只留竖条 `render_vertical_bar`，宽度全部还给网格。
3. 开合开关合并为单一右栏开关 `show_value_sidebar` / `ToggleValueSidebar`（语义已是「右侧辅助栏」）；不留两套侧栏状态。
4. 列浏览器页签内部结构沿用 §8.11：标题「列浏览器 · N 列」→ 搜索框（`Size::Small` + Search 前缀 + 有词才摆 ✕）→ 清单滚动区 → 底部明细定高 `RESULT_BROWSER_DETAIL_HEIGHT = 7.75rem`。
5. 清单 ↔ 网格双向选中联动口径不变：清单单击只选中列 + 刷新明细 + 右栏联动，不移动网格焦点；双击定位首行；眼睛切显隐不改变选中列；冻结 / 排序 / 统计 / 复制列名走右键菜单；行上不放图钉 / 排序箭头 / 拖拽手柄。
6. 搜索范围 = 列名 + 类型名 + 注释（忽略大小写包含匹配）；空词全量原序；隐藏列保留清单（置灰斜体 + 眼睛关闭态）；无匹配空态「无匹配列」；本轮清单只单选。
7. 明细只读 `selected_col`，越界 / 隐藏兜底第一可见列；值详情只跟 `selection_pos`；计算 / 分组列源 = `inspect_column`（焦点格优先、清单兜底、排除隐藏列、转置返回 `None`）。
8. 页签 × 关闭：非 Browser 页才摆 ×，点击退回 `RightTab::Browser`（**不收起右栏**；右栏开合仍由工具栏开关控制）。

**端口与结构常量变更**：

- 删除 `ToggleColumnSidebar`；`ToggleValueSidebar` / `SetRightTab` 注释改四页签口径；`RightTab = Browser(#[default]) / Value / Stats / Group`，「引用」页留 backlog。
- `ui::RESULT_SIDEBAR_LEFT_WIDTH` 删除；`RESULT_SIDEBAR_RIGHT_WIDTH = 20.0rem` 注释改「右侧辅助栏」；`RESULT_BROWSER_DETAIL_HEIGHT = 7.75`、`RESULT_SIDEBAR_KV_LABEL_WIDTH = 4.5` 保留不动。
- 模块级纯函数 `right_tab_from_index`：0→Browser、1→Value、2→Stats、3→Group、越界→Browser；`filter_browser_columns` / `kv_row` / `fmt_stat` 不变。
- `ResultControls` 删除 `column_sidebar` 字段与工具栏消费行；菜单纯函数 `column_browser_menu_items` 与访问器未动（动作语义不变，仅入口搬到右栏页签清单）。
- editor 宿主：删除 `ToggleColumnSidebar` 分派段；`ToggleValueSidebar` 分派保留；`SetRightTab` 注释改四页签；`SelectColumn` / `LocateColumn` / `ClearColumnSearch` 分派不变。

**原型同步**：`docs/analysis/结果面板-DBeaver风格原型.html` 移除 `#side-left` 容器与 `showLeftSidebar` 状态；右栏四页签「列浏览器 / 值详情 / 计算 / 分组」；工具栏双开关合并为单一右栏开关；`renderRightTab` 下标重映射；窄屏隐藏右栏与状态栏右侧元信息防横向溢出；`shot.py` 自检 `error=0`（warning 仅沿用旧原型的字符图标与字体提示，不阻塞）。

**测试基线（本轮回归）**：`cargo test -p rds-result-panel --lib` = **156 passed / 0 failed**（迁移前 155；新增 `right_tab_index_maps_to_four_tabs` 断言 0/1/2/3 与 99 越界映射；5 例 `filter_browser_columns` 纯函数测试与 3 例菜单测试保留；文档回写轮复跑复核仍为 156 passed / 0 failed）。`cargo check -p rds-editor --lib` 仍被并行线「函数手册 #54-C5」未提交改动阻塞：10 个编译错误逐条核对路径——`host.rs:24` 重复 `IconName` 导入（E0252）、`host.rs:1513` 函数手册按钮（`IconName::BookMarked`），其余全部在未跟踪 `crates/editor/src/view/function_manual.rs`（`Rc` 缺失、`v_flex`/`h_flex`/`open_dialog`/`close_dialog` 缺 trait 导入、值移动借用、unused `ManualCategory` 等）；本线分派段（删除 `ToggleColumnSidebar`、`SetRightTab`/`SelectColumn`/`LocateColumn`/`ClearColumnSearch`）未引入新错误。按纪律不替并行线修、不绕过、不提交其 hunk，待解阻后复跑。

**遗留 backlog**（沿用 §8.11）：多选批量动作；列顺序拖拽；搜索词跨结果持久化口径复核；清单行类型图标；「引用」页签（外键面包屑 + 引用表只读预览）。

### 8.13 渲染改进批次（P0-1 / P0-2a / P1-4 / P1-5 / P2-1 / P2-2）

> 对照 `docs/architecture/editor/result-panel-table-render-plan.md` 与 `docs/analysis/竞品分析-dbdelve表格渲染与全竞品表格对照.md` 落地；依据源码 `crates/result_panel/src/grid.rs`、`panel.rs`、`crates/editor/src/view/host.rs`、`crates/engine/src/services/sql_service.rs`。下列各项均为**展示层**改动：复制 / 悬停 / 导出 / 编辑 / 右键筛选 / 统计始终走单元格完整值，展示派生不污染存储值。

| 编号 | 能力 | 落地口径（源码锚点） |
| --- | --- | --- |
| P0-1 | 单元格字符级裁剪 | `CELL_DISPLAY_LIMIT = 300` + 纯函数 `clip_cell_display`（按 `char_indices` 计边界，不裁断中文 / emoji，省略号不占额度，不展平换行）；`render_td` 非 BLOB 分支只把裁剪后串交 GPUI 布局，消除大 JSONB / 几何 / 大文本导致的每帧长串测量退化。完整值路径不动：`cell()` / `row_cell_text` / `cell_text`（导出）、悬停 `full = text.clone()`、右键 `value`、编辑初值、统计口径 |
| P0-2a | 状态行分段语义与窗口代价提示 | `MAX_SINGLE_SEGMENT_ROWS = 10_000`（对齐 engine `sql_service.rs` `MAX_QUERY_ROWS`，本 crate 不跨模块搬常量）；`WINDOW_FETCH_NOTICE`「分段加载：每段重跑查询；无稳定 ORDER BY 时行可能重复或跳过」；纯函数 `fetch_more_hint` / `truncation_notice` / `window_fetch_active`；状态行右段顺序 = 当前格预览 → 取段提示（muted，挂代价 tooltip）→ 截断警告段（挂代价 tooltip）→ 旧文案兜底 → 选中行。区分「取完了」与「撞单段上限 10000」 |
| P1-4 | 格内 JSON pretty-print | 纯函数 `pretty_json_display`：trim 后首字符 `{` / `[` 且 `serde_json` 解析成功才美化（两空格缩进多行）；非法 JSON / 裸标量 / 普通文本 / NULL / 空串返回 `None` 交回普通文本路径；美化后仍过 `clip_cell_display` |
| P1-5 | 列宽会话内记忆 | delegate 字段 `column_widths: HashMap<String, Pixels>`——**按列名**存（不用下标 `Vec`，隐藏列 / 将来列序拖拽不错位；行号槽不记）；`set_data` 清空（换结果集列集合变了，旧宽度不成立，与 frozen / hidden 同口径）；`set_column_widths(all)` 遍历事件宽度，经 `data_column(view_col)` 映射到数据列名回填；`column()` 命中按列宽、未命中回 `RESULT_COLUMN_WIDTH`。宿主 `host.rs` grid_sub 接 `TableEvent::ColumnWidthsChanged(widths)` → `delegate_mut().set_column_widths`，不 notify（组件发完事件已自刷新）、不动选中态。**仅会话内**，落盘持久化延后 |
| P2-1 | 整数千分位展示 | 分组内核 `group_digits(digits)`（`thousands` 重构复用）；纯函数 `format_integer_cell(text)`：trim 后 parse `i64` 成功 → 负号 + 绝对值三位一组插逗号；非数值列不尝试，数值列解析不成整数（小数 / 科学计数 / 异常样本）回退原文。`render_td` 派生顺序：JSON 美化优先 → 数值列整数分组 → 原文，最后统一裁剪；浮点 / Decimal 小数本轮不分组（避免破坏精度与科学计数法口径） |
| P2-2 | 计算 / 分组页列级访问器 | 原 `render_stats_tab` / `render_group_tab` 先 `visible_rows()` 物化整行矩阵（每行全列字符串 clone），再交给 `column_stats` / `group_by_count` 扫一列——万行 × 多列时是每帧 render 的冗余 O(行×列) 拷贝。落地：纯函数拆列级内核 `column_stats_from_values` / `group_by_values`（矩阵版 `column_stats` / `group_by_count` 抽列后委托，口径不变）；delegate 新增 `column_values` / `column_stats_for` / `group_by_for`，只 clone 目标列取值、跟随筛选 / 排序后的视图行序；右栏两页改走列级访问器。列头 / 清单菜单构造期的 `column_stats(&self.rows, column)` 事件期一次性现算保留不动 |

**明确延后 / 不做（本轮不立项）**：

- COUNT 精确总数（`count_wrapped` + 默认关开关 + 2s 超时放弃）——延后；engine `QueryResult.total_rows` 恒 0，`has_more` 时状态行只能报 `N+`。
- 列宽落盘持久化（跨连接 / 库 / 表列集合的 `ColumnLayout` 重放）——延后，仅会话内记忆。
- 统计 / 分组结果跨帧缓存——未做：render 须保持纯读（不在 render 内写 Shared / 复杂计算），事件期算好存状态的失效口径（筛选 / 排序 / 取段追加 / 选中列 / 转置）易脏；且右栏非活动页签根本不渲染，收益不抵风险。现扫成本已随 P2-2 消除整行物化而下降，记 backlog。
- 小数 / Decimal 整数部分分组——延后，先只对可解析 `i64` 的整数值分组。
- 结果落盘恢复（dbdelve `StoredGrid`）→ P1 另立项；全量流式拉入（dbdelve 4GB 持有）→ 不采纳，保留窗口模式内存界住；polars 不引入（§8.8 选型记录）。

**测试基线（本轮回归）**：`cargo test -p rds-result-panel --lib` = **164 passed / 0 failed**（161 + 千分位 1 例 + 列级口径一致性 2 例）；`cargo test -p rds-editor --lib` = **603 passed / 0 failed / 1 ignored**，编译无 error（宿主侧仅 `ColumnWidthsChanged` 分派小段，与既有事件分支同范式；唯一 `unused import: signature_view` warning 属并行线未提交改动，非本线符号；603 与历史 683 基线的差额来自并行线工作区状态，本线未删改 editor 测试模块）。

### 8.14 事实层 / 显示层分离 · A 批底座（A1–A4，2026-10-26，提交 `39c15033`）

> 方案定稿与逐跳口径：`docs/analysis/结果集数据流-事实层显示层分离方案.md`（分层模型 / 三态状态机 / 水位实测 / A·B·C·D·E 分期）；本节约束 A 批已落地口径，B / D 开工前回读该方案对应章节。

**分层口径（单向派生 L0 → L1，L1 永不回写 L0）**

- **L0 事实层** `FactFrame`（`crates/shared/src/fact.rs`）：复用 Arrow batches 引用 + schema + 列名，是类型保真的唯一来源；不 derive `Clone`（batches 深拷贝代价高），槽位侧 `Arc<FactFrame>` 共享；事实值起步档复用 `shared::Value` 六变体（Null / Bool / Int(i64) / Float(f64) / Text(String) / Bytes(Vec<u8>)），日期 / Decimal / Timestamp 类型族扩展属 E 阶段。
- **L1 显示层** `ResultEntry`（现状不动）：columns + `rows: Vec<Vec<String>>` + column_types + annotations + 血缘 / 原料 / 连接 / 通道；网格渲染、文本档导出、现状本地交互继续只读显示矩阵。
- **叠加层归属**：#28 行级新增 / 复制 / 删除、格级变更集继续挂显示槽位（`changes` / `histories` / `locals` / `row_histories` 同口径），提交落库时按行键回源，不污染事实档。

**A 批落点（源码锚点，行号以当前基线为准）**

| 批次 | 内容 | 锚点 |
| --- | --- | --- |
| A1 | `FactFrame` / `FactKind` / `FactValue` 类型定义 + 访问器 `column_kind` / `value_at` / `rows_as_text` / `append_batches` / `row_count` / `estimated_bytes`；`from_query_result`（batches 空 → `None` 降级档）；8 例单测 + 水位探针测试 | `crates/shared/src/fact.rs:77`（`FactFrame`）、`:85`（`from_query_result`）、`:190`（`estimated_bytes`）；`crates/shared/src/models.rs` `arrow_value_at` 放开 `pub(crate)` 复用 |
| A2 | `FactState { None, InMemory(Arc<FactFrame>), OnDuckDb }` 三态枚举；`facts: Vec<FactState>` 与 `sets` 同序同增减平行槽位；`merge_fact_state` 处理新段 None 保留旧档 / 旧 None+新 Some 挂新帧 / 旧内存档+新 Some 两段合并重建（形状校验失败整档作废 None）；只读访问器 `fact_at` / `active_fact`；淘汰 / 清空平行同步；4 例槽位生命周期单测 | `crates/result_panel/src/store.rs:657`（`FactState`）、`:705`（`merge_fact_state`）、`:742`（`facts` 槽位）、`:768`（`push` 第三参）、`:932` / `:940`（访问器）、`:1191`（`evict_oldest`） |
| A3 | `QueryData` 内嵌 `fact: Option<Arc<FactFrame>>` 字段；derive 由 `Debug, Clone, PartialEq, Eq, Default` 改为 `Debug, Clone, Default`（事实帧不参与整帧深拷贝与逐值比较）；`to_data` 内 `FactFrame::from_query_result` 装入 fact；`analyze` 本地 DuckDB 分析产物 `fact: None`（无原始 batches 降级档）；`entry_from` 返回 `(ResultEntry, Option<Arc<FactFrame>>)` 元组入槽；五处入槽映射同步；失败 Append 分支随失败回执丢弃 fact；3 例入槽 / 合并 / 形状不匹配单测；`QueryRunner` trait 签名不变 | `crates/editor/src/execution.rs:488`（derive）、`:489`（struct）、`:510`（`fact` 字段）；`crates/editor/src/cell.rs` `CellOutcome` 连带去 `PartialEq, Eq`；`crates/workbench/src/services/editor_exec.rs:1308`（`to_data`）、`:1309`（构造 fact）、`:828`（`analyze`）、`:860`（`fact: None`）；`crates/editor/src/view/host.rs:5867`（解构）、`:5896`（`push(entry, placement, fact)`）、`:7466`（`entry_from`） |
| A4 | 偏好 `fact_storage: Auto(默认) / Memory / Table` 注册；分流纯函数 `storage_choice(dual_bytes, pref, current) -> FactTier` + `dual_estimate`；高低水位滞回；强制档优先于 Auto；5 例分流单测 + 3 例偏好持久化 / 映射 / 未知值拒绝单测；设置页「结果集面板 · 事实档载体」，`NextUse` 生效方式（下次触发物化才读，无需即时通知重绘） | `crates/shared/src/fact.rs:242`（`StoragePref`）、`:256`（`CurrentTier`）、`:264`（`FactTier`）、`:274`（8 MiB 上界）、`:278`（3 MiB 滞回线）、`:283`（×5 系数）、`:286`（`dual_estimate`）、`:303`（`storage_choice`）；`crates/settings/src/model.rs:53`（顶层分节）、`:136`（`FactStoragePref`）；`crates/settings/src/registry.rs:144` / `:183` / `:224` / `:858`；`crates/settings/src/lib.rs:280`（读）、`:543`（写） |

**分流规则（`storage_choice`，方案 §4.1 / §8.3）**

- 强制 `Memory` → `InMemory`（不看水位）；强制 `Table` → `OnDuckDb`（不看水位）。
- `Auto` + 当前 `InMemory`：估算 < 低水位 → `None` 回落；低–高区间保持 `InMemory`（滞回防抖动）；超 高水位 → `OnDuckDb`。
- `Auto` + 当前 `None`（新结果集 / 已降级）：估算 ≤ 高水位 → `InMemory`；超 高水位 → `OnDuckDb`。
- `Auto` + 当前 `OnDuckDb`：保持 `OnDuckDb`（表档不回读 Rust 堆；回落内存档属 B 批细化）。
- 端点含端点：等于低水位保持内存档、等于高水位保持内存档。

**A 批边界（明确不做，B / C / D / E 开工前不越界）**

- `OnDuckDb` 变体 A 批留空壳不构造、不接线；**A 批未接触发物化调用点**：`storage_choice` 的消费（进分析 / 带类型导出 / IronCalc 快照时读偏好、估双份、按规则构造 `InMemory` 候选或 `OnDuckDb` 意向）属 B 批跳 5 路径；A 批 Auto 超水位时不构造事实档，按 `None` 降级处理。
- 网格渲染、CSV / JSON / INSERT / SQL Updates / WHERE 文本导出不触发物化；渲染路径一行不改。
- 不扩第一跳 Arrow 类型族、不修驱动硬错（E 阶段 engine 跨线 backlog，本线不擅动 `crates/engine/src/driver/native/*`）；不引入 polars；不做 mmap 文件档 / 后台异步物化 / 后台预取；表档不随机读回 Rust 堆。
- 不改编辑器状态栏归属（另一会话任务）；不碰 insight / plugin / 编辑器文档内核。

**测试基线（A5 回归，2026-10-26）**

- `cargo test -p rds-shared --lib` = **33 passed / 0 failed**（A1 八例 + 探针 + A4 五例分流）。
- `cargo test -p rds-settings --lib` = **35 passed / 0 failed**（契约护栏 `every_model_leaf_is_registered` / `every_registered_key_exists_in_the_model` / `registered_defaults_match_the_model` / `sections_are_contiguous` / `every_registered_key_has_a_matching_slot` / `page_rows_are_scalar_and_writable` 全绿 + A4 三例偏好）。
- `cargo test -p rds-result-panel --lib` = **171 passed / 0 failed**（持平 A3 基线；A4 未动面板生产代码）。
- `cargo test -p rds-editor --lib` = **603 passed / 0 failed / 1 ignored**（1 ignored = 需本机 ipykernel 的 python notebook 用例；3 个 warning——`signature_view` unused import、`apply_filter` / `clear_filter` never used、`substr_sig` never used——均属并行线存量，非本线符号）。
- `cargo check -p rds-workbench --lib --jobs 2` = **Finished，0 error / 0 warning**。
- `cargo check -p rds-workbench --tests --jobs 2` = **Finished，0 error**；仅 1 个存量 warning `unused import: AppContext`（`crates/workbench/tests/history_editor_entry.rs:11:16`，并行线存量，本线只补 `push` 第三参 `None`，未动 import）；整包测试未随 A5 执行（insight 线工作区状态以 lib / tests 编译通过为准，不绕过阻塞项）。

### 8.15 事实层 / 显示层分离 · B 批保真链（B2–B6，2026-10-26，提交 `84a81572`）

> 方案定稿与逐跳口径：`docs/analysis/结果集数据流-事实层显示层分离方案.md` §7-B / 跳 4 / 跳 5；本节记录 B 批已落地架构口径。A 批底座见 §8.14；D1 导出分级见 §8.16。

**B 批落点（源码锚点，行号以当前基线为准）**

| 批次 | 内容 | 锚点 |
| --- | --- | --- |
| B2 | 常驻表句柄回挂 `FactFrame`（`temp_table` / `row_count` 缓存）；`OnDuckDb(Arc<FactFrame>)` 变体接通；store `set_fact_table` / `promote_fact_to_table`（表档释放 batches、回挂表名） | `crates/shared/src/fact.rs`（`temp_table` / `row_count`）；`crates/result_panel/src/store.rs`（`FactState::OnDuckDb`、`promote_fact_to_table`、`set_fact_table`） |
| B3 | `AnalysisRequest` 增 `fact` / `pref`，`DuckDbExportRequest` 增 `fact`；host 事件路径读偏好与事实档快照随请求带入工作线程；`ExecTarget` 去 derive 比较改手工 `PartialEq` | `crates/editor/src/execution.rs`（`AnalysisRequest` / `DuckDbExportRequest` / `ExecTarget`）；`crates/editor/src/view/host.rs`（分析 / 导出入口取 `fact_at` 快照） |
| B4 | 分析保真三分支：表档复用常驻表不重建；内存档 batches 经 `create_temp_table_from_batches` Arrow Appender 直灌新建表、按 `storage_choice` 分流决定是否回带释放信号；无事实档 / 已释放 / 空批回退 internal 猜型降级档，notice 显式标注「文本猜型降级档：整列按文本存储，数字类型可能失真」 | `crates/workbench/src/services/editor_exec.rs`（`analyze` 三分支 + 降级标注）；`crates/engine/src/services/execution_service.rs`（`execute_duckdb_analysis_from_batches`）；`crates/engine/src/services/duckdb_service.rs`（`create_temp_table_from_batches`） |
| B5 | 内存档列式保真：shared `FactFrame::column_stats` / `column_number`（Int / Float 原生 min / max / sum / avg，Bool 只计 count / nulls，文本 / 二进制 / 其他族与表档返回 `None`）；delegate `column_stats_for` / `compare_rows` / `group_by_for` 三路分流，任一取不到原生数值整对回退显示矩阵口径，不出现同一列两套口径混排；分组整数归一化键（`1.0` 与 `1` 同组）；工具栏载体段：内存档 `内存档 X MiB`、表档只摆 `表档`、无事实档不摆段 | `crates/shared/src/fact.rs`（`FactColumnStats` / `column_stats` / `column_number`）；`crates/result_panel/src/grid.rs`（`fact` 句柄 / `set_fact` / 三路分流 / `stats_from_fact` / `number_group_key` / `bytes_text` / `ResultToolbar.fact_tier` / `fact_bytes`）；`crates/result_panel/src/panel.rs`（`GridPayload.fact` + `sync` 注入）；`crates/editor/src/view/host.rs`（事实档快照投影到网格与工具栏） |
| B6 | 漂移注释勘误：旧注释承诺「数字列在 Parquet / XLSX 里就是数、前导零文本被推成数字」与现状不符——`cell_json` 只产 Null / String，`infer_type` 只认 JSON 值类型、不 parse 文本，整列落 `VARCHAR`，失真方向是**数字列被收窄成文本**；注释与降级档 notice 文案按实证口径改写 | `crates/workbench/src/services/editor_exec.rs`（分析 / 导出两段 + 降级标注文案）；`crates/engine/src/services/execution_service.rs`（导出口径段注释）；`crates/editor/src/analysis.rs` / `crates/result_panel/src/analysis.rs`（模块头失真边界） |

**口径边界（B 批守住的不做项）**

- 列式聚合纯函数下沉 shared；`arrow` 在 `result_panel` 仅 `[dev-dependencies]`（测试 fixture 用），产品代码不 `use arrow`；delegate 只调 shared 纯函数分流，不新增 result_panel 产品 arrow 依赖。
- 表档本地交互（排序 / 统计 / 分组）第一期回退显示矩阵口径；SQL 下推不读回 Rust 属后置项。筛选 / 列隐藏 / 冻结 / 列宽不涉及类型，未改动。
- 分析产物当前只回列名 + JSON 行、不带 batches，新结果集 `fact: None`；链式产物新 FactFrame 依赖引擎 `query_duckdb` 回 Arrow，属后置增强项。
- insight 切回复用宿主表属并行线，本批未碰；编辑器状态栏并入全局状态栏属另一会话任务，本批未动。

**测试基线（B 批回归，2026-10-26）**

- `cargo test -p rds-shared --lib` = **40 passed / 0 failed**（含 B5 列式统计 / 数值取数 5 例）。
- `cargo test -p rds-result-panel --lib` = **181 passed / 0 failed**（含 B5 分流 / 归一化 / 载体段 6 例）。
- `cargo test -p rds-editor --lib` = **603 passed / 0 failed / 1 ignored**；全量偶发 1 例 `named_parameters_hold_execution_until_filled` 单跑复现通过，定性工作线程 mpsc 排队时序，非本线回归。
- `cargo test -p rds-engine --lib` = **658 passed / 0 failed / 24 ignored**。
- `cargo test -p rds-workbench --lib analysis_tests` = **4 passed / 0 failed**；`cargo check -p rds-workbench --lib --tests --jobs 2` = 0 error（1 个 unused import warning 属并行线 `history_editor_entry.rs`，非本线符号）。

### 8.16 事实层 / 显示层分离 · D1 导出分级（Parquet / XLSX 三分支保真，2026-10-26，提交 `83655b26`）

> 方案定稿与逐跳口径：`docs/analysis/结果集数据流-事实层显示层分离方案.md` §7-D；本节记录 D1 已落地架构口径。B 批保真链见 §8.15，A 批底座见 §8.14。

**D1 落点（源码锚点，行号以当前基线为准）**

| 批次 | 内容 | 锚点 |
| --- | --- | --- |
| D1-a | shared 新增 `FactExportMode` 三态（`Batches` / `ExistingTable` / `Matrix`）、`FactFrame::export_mode()` 与纯函数 `fact_export_mode(Option<&FactFrame>)`；`is_degraded()` 仅 `Matrix` 返回 true；判定顺序与 B4 分析分支对齐：先判已挂常驻表名 → 复用，其次未释放且非空批 → 直灌，其余归降级档；1 例三态判定单测（内存档 Batches / 释放挂表名 ExistingTable / 无表名孤儿 Matrix / `None` → Matrix） | `crates/shared/src/fact.rs`（`FactExportMode` / `export_mode` / `fact_export_mode` / `is_degraded`）；`crates/shared/src/lib.rs`（重导出） |
| D1-b | engine 新增 `export_batches_via_duckdb`（batches 经 Arrow Appender 直灌新建临时表 → `COPY` → 成败都 `DROP`、即用即收，不回挂事实档句柄）与 `export_existing_table_via_duckdb`（复用常驻表 `COPY`，不新建、不回收；行数由调用方按事实帧 `row_count` 缓存带入）；2 例保真导出单测：batches 读回 `typeof(id)=BIGINT`（对照 internal 档整列 VARCHAR）、常驻表导出后仍可查询未被 DROP | `crates/engine/src/services/execution_service.rs`（两导出函数 + `export_tests` 两例） |
| D1-c | workbench `ResultService` 两透传；`export_via_duckdb` 改三分支分流：`Batches` 走 batches 直灌、`ExistingTable` 走常驻表复用、`Matrix` 走既有 `cell_json` 桥接 `export_rows_via_duckdb` 降级档；info 日志带 `mode`；`export_tests` 补 2 例（内存档读回 BIGINT、表档复用不 DROP 并清场） | `crates/workbench/src/services/result_service.rs`（两透传）；`crates/workbench/src/services/editor_exec.rs`（`export_via_duckdb` 三分支 + `export_tests`） |
| D1-d | editor `DuckDbExportRequest` / `ExportNote` 增 `degraded: bool`；host 提交侧按 `fact_export_mode(fact).is_degraded()` 纯判定置位（与 workbench 执行分流共用同一纯函数，不依赖工作线程回执回传口径）；成功回执追加「文本猜型降级档：数字列按文本存储」说明；替身导出用例补 `degraded` 标记断言与文案断言 | `crates/editor/src/execution.rs`（两结构字段 + 线程内透传）；`crates/editor/src/view/host.rs`（提交侧判定 + 回执文案）；`crates/editor/src/view/tests.rs`（两例替身断言） |

**口径边界（D1 守住的不做项）**

- 文本档（CSV / JSON / INSERT / SQL Updates / WHERE）维持显示矩阵纯函数编码，不触发物化；`crates/result_panel/src/export.rs` 一行未改。
- 本期只覆盖 Parquet / XLSX 两档事实档直灌；方案 §7-D 第 2 条提到的 `typed JSON` 属新增格式档，**后置不扩面**；现状 `Json` 档仍走文本档纯函数。
- 导出范围仍只导已抓取段；全量导出另立取数路径，本批不做。
- 降级档失真（整列落 `VARCHAR`、数字列被收窄成文本）只在两个保真分支消除；前导零文本在降级档保持原样（不被推成数字）。
- 表档常驻表回收归事实档生命周期（槽位淘汰 / 文档关闭 / 项目切换清场），导出路径不 `DROP`；导出专用临时表（Batches 分支）即用即收，与分析常驻表生命周期不同。
- insight 切回复用宿主表属并行线，本批未碰；编辑器状态栏并入全局状态栏属另一会话任务，本批未动。

**测试基线（D1 回归，2026-10-26）**

- `cargo test -p rds-shared --lib` = **41 passed / 0 failed**（基线 40 + D1 三态判定 1 例）。
- `cargo test -p rds-settings --lib` = **35 passed / 0 failed**（D1 未动 settings，基线持平）。
- `cargo test -p rds-result-panel --lib` = **181 passed / 0 failed**（D1 未动面板产品代码，基线持平）。
- `cargo test -p rds-engine --lib` = **660 passed / 0 failed / 24 ignored**（基线 658 + D1 保真导出 2 例）。
- `cargo test -p rds-workbench --lib analysis_tests` = **4 passed / 0 failed**；`cargo test -p rds-workbench --lib export_tests` = **4 passed / 0 failed**（2 例存量降级档 + 2 例 D1 保真档）。
- `cargo test -p rds-editor --lib` = **602 passed / 1 failed / 1 ignored**；唯一失败 `named_parameters_hold_execution_until_filled` 为工作线程 mpsc 排队时序偶发，单跑复现 1 passed，非本线回归；两条 D1 替身导出用例（`parquet_export_goes_through_the_runner` / `duckdb_export_follows_the_local_filter`）通过。
- `cargo check -p rds-editor --lib --jobs 2` = 0 error / 0 warning；`cargo check -p rds-workbench --lib --tests --jobs 2` = 0 error（1 个 unused import warning 属并行线 `crates/workbench/tests/history_editor_entry.rs`，非本线符号）。整包测试未执行（insight 线未提交 test-cfg 可能阻塞整包，被阻塞时如实上报不绕过）。

### 8.14 C1：结果集 tab 标签重命名 + 钉住（会话级，2026-10-27）

**拍板口径（用户确认）**：
- 默认标签保持 `结果 N`，不自动拼 SQL 片段（SQL 片段对齐 DBeaver hover tooltip，不在默认标签上）；
- 重命名走 tab suffix 末尾「⋯」ghost 按钮 → tab 条下方内联输入框（✓ 提交 / ✕ 取消）；
- 钉住独立于分组（C2 才做），pinned tab 不被 FIFO evict 淘汰；prefix 加图钉小图标；
- **会话级持久化，不落盘**（对齐 DBeaver / DataGrip：重命名与钉住只在当前会话有效，重启恢复 `结果 N` + 全未钉住）；
- 手动 tab 分组（C2）后置：数据模型 `groups: Vec<ResultGroup{id,name,members,collapsed}>`、折叠组合成 tab + Popover 列组内、右键移动到新组/已有组/移出组；C3 拖放分组/组颜色更后置。

**代码落点**：
- `store.rs`：`DocumentResults` 加 `pinned: Vec<bool>` 平行槽位（与 sets 同序）；构造 / Replace / NewSet / Append 三分支联动 push `false`；Replace 覆盖 active 时重置 `pinned=false`（新结果不继承钉住）；`evict_oldest` 找 victim 时排除 `slot.pinned[victim]`，全钉住则 break（不硬淘汰用户钉住的）；新增方法 `set_title(doc, idx, Option<String>)`（空白串归一为 `None` 恢复序号）、`toggle_pin(doc, idx)`、`is_pinned(doc, idx)`、`pinned(doc) -> &[bool]`。
- `ports.rs`：`ResultRequest` 加 `RenameSet(usize, Option<String>)` / `TogglePin(usize)` / `BeginRename(usize)` / `CommitRename` / `CancelRename` 五变体。
- `sets.rs`：`ResultSetTab` 加 `pinned: bool`；`tabs()` 签名加 `pinned: &[bool]` 参数（越界按未钉住）；新增 `TabActions { on_rename: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>, on_toggle_pin: Option<Rc<dyn Fn(usize)>> }`；`render()` 加 `actions: TabActions` 参数；tab suffix 末尾加 ⋯ 按钮（Ellipsis 图标，ghost small）；tab pinned 时 prefix 加图钉图标（`Icon::default().data(PIN_ICON_SVG)` 内联 SVG，因 `gpui-component::IconName` 兼容宏只暴露 default-icons.txt 子集，无 Pin 变体）；新增测试 `pinned_slice_projects_into_tabs`。
- `panel.rs`：`ResultPanel` 加字段 `renaming: Option<usize>` / `renaming_input: Option<Entity<InputState>>`（懒创建复用）；新增 `begin_rename(index, window, cx)`（预填当前 label）/ `commit_rename(cx)`（读输入框值，空串归一 `None`，push RenameSet）/ `cancel_rename(cx)`；主 tab 条传 `TabActions`（on_rename → push BeginRename，on_toggle_pin → push TogglePin）；对比 tab 条传 `TabActions::default()`（None，不画 ⋯ 按钮）；renaming 态时 tabs 外包 v_flex，下方插输入框条（Input + ✓ Check + ✕ Close）。
- `host.rs`：`result_sets::tabs()` 调用补 `store.pinned(&self.document)` 第三参数；`dispatch_result_request` match 加 5 个 C1 分支——`RenameSet` / `TogglePin` 直接 update_results；`BeginRename` 走 `window.update` 转手调 `panel.begin_rename`（需要 window 创建/回填 InputState）；`CommitRename` / `CancelRename` 调 `panel` 方法。

**关键技术决策**：
- **不用右键菜单**：gpui Tab 是封装组件，render 内部挂 on_click，外部无法注入 context_menu；改用 suffix 末尾 ⋯ ghost 按钮（符合「组件选型禁手搓」，用 Button 不是自绘 div）。
- **render 闭包是 `&self`，不能调 &mut Self 方法**：进入 renaming 态走 BeginRename 端口事件路径（host dispatch → window.update 转手 → panel.begin_rename）；提交/取消走 CommitRename/CancelRename 端口。
- **Pin 图标不在 gpui-component::IconName 默认集**：用 `Icon::default().data(PIN_ICON_SVG)` 内联 SVG 字节（lucide pin 路径），零额外资产文件。
- **evict 全钉住时不硬淘汰**：break 出循环（用户钉住的结果集不被 FIFO 干掉；新结果进不来是显式语义——用户可以取消钉住旧结果再跑新 SQL）。

**已知遗留（C2 之前不做）**：
- 重命名输入框 Enter 提交 / Esc 取消键盘支持尚未实现（当前只有 ✓/✕ 按钮）；
- renaming 时切换其他 tab 是否自动取消 renaming 未处理（切换 tab 后 renaming 条仍在，用户可点 ✕ 取消）；
- renaming 输入框预填值是当前 label（含自定义标题或自动序号），用户清空后提交 = 恢复序号；
- 手动 tab 分组（C2）与拖放分组（C3）后置。

### 8.18 DBeaver 布局对齐（2026-10-27）

本轮全方面对齐 DBeaver Data Editor 布局：

**已落地**：
- **左缘竖条**（`render_vertical_bar`）：三枚幽灵小钮竖排互斥——网格 `Table2` / 文本 `FileText` / 记录 `UserRound`（记录钮下方追加上下行导航）；宽 `RESULT_VERTICAL_BAR_WIDTH = 2.25rem`（36px）。
- **右缘竖条**（`render_right_edge_bar`）：最右侧边缘面板图标条，顶部 `PanelRight` 图标控制右侧边栏开合；预留面板切换图标位置（图表/统计等，backlog）。
- **右侧边栏 5 页签**：`RightTab = Browser(0, 默认) / Value(1) / Stats(2) / Group(3) / Reference(4)`；labels = `["列浏览器", "值详情", "计算", "分组", "引用"]`；引用页签空态占位（外键引用查看，backlog）。
- **状态栏左段 8 按钮**：保存 `Save` / 取消 `X` / 编辑 `Pencil` / 新增 `Plus` / 复制 `CopyPlus` / 删除 `Trash` / 撤销 `Undo2` / 重做 `Redo2`（写操作段 `0..8`；翻页段 `8..11`；系统段 `11..`）。
- **状态栏右段格式**：`... N 行已获取 · 0.025s`（对齐 DBeaver）。
- **工具栏最左侧 SQL 预览**：SQL 前 60 字符 + …，hover tooltip 看全文；点击复制到剪贴板（`ResultRequest::CopySql`）。
- **中段行数输入框**：`Input` 组件（`row_count_input: Entity<InputState>`，`panel.rs` 构造时创建，`ResultPane.row_count_input` 传入 `render`）。
- **列头排序箭头指示**：当前列是排序列时显示 `↑`（升序）或 `↓`（降序），在筛选▽左边。

**剩余 backlog**（需要架构改动或跨 crate 接线）：
1. **SQL 按钮点击弹窗**：当前是 tooltip hover + 点击复制，需改成 Popover 点击弹窗（需要 `PopoverState` 状态管理）。
2. **SQL 按钮双击新编辑器**：双击 SQL 预览另打开新编辑器，SQL 也在上面（需要 editor 宿主提供"打开新编辑器"入口，跨 crate 接线）。
3. **列头下拉菜单**：当前走右键菜单 + DropdownButton，DBeaver 是每列箭头菜单（交互细节未对齐）。
4. **取全部行 / 计算总行数**：底部工具栏按钮（`ResultRequest::FetchAll` 变体未加）。
5. **结果面板最大化**：`Ctrl+Shift+6`（dock 面板 `zoom_control` 未实现）。
6. **引用页签**：外键引用查看（面包屑 + 引用表只读预览，需下发 SQL 查源库）。
7. **多选批量动作**：列浏览器多选批量动作（需多选模型 + 批量动作入口）。
8. **列序拖拽**：列顺序拖拽（`move_column` 钩子 + 列序模型与隐藏/冻结/转置映射改造）。
9. **列宽 backlog**：双击列头边缘自动适应内容宽度、右键列头「设置列宽」输入数值、列宽跨会话持久化（当前只会话内按列名记）。

**测试基线**：`cargo test -p rds-result-panel --lib` = **196 passed / 0 failed**。

### 8.19 gpui-kit table 组件能力释放（2026-10-27）

在 `TableState::new()` 链式调用里开启之前未用的 5 项能力：

| 能力 | 方法 | DBeaver 对标 | 说明 |
| --- | --- | --- | --- |
| 列序拖拽 | `.col_movable(true)` | ✅ | 拖拽列头改变列顺序 |
| 列头排序 | `.sortable(true)` | ✅ | 点击列头排序 |
| 选中整列 | `.col_selectable(true)` | ✅ | 选中整列 |
| 键盘循环选择 | `.loop_selection(true)` | ⚠️ | 键盘导航到末尾后循环回开头 |
| Ctrl+点击多选 | `.select_on_modifier_click(true)` | ✅ | Ctrl+点击多选 |

**已用能力（之前已开启）**：
- `.col_resizable(true)`：列宽可调
- `.row_selectable(true)`：行可选
- `.cell_selectable(true)`：单元格可选
- `.row_header(false)`：隐藏组件自带行头（用自己的 `#` 列）

**释放后能力清单**（gpui-kit table 组件全部 11 项能力已开启）：
1. col_resizable ✅
2. row_selectable ✅
3. cell_selectable ✅
4. row_header(false) ✅
5. col_movable ✅（本轮新增）
6. sortable ✅（本轮新增）
7. col_selectable ✅（本轮新增）
8. loop_selection ✅（本轮新增）
9. select_on_modifier_click ✅（本轮新增）
10. ColumnWidthsChanged 事件 ✅
11. RightClickedRow / RightClickedCell 事件 ✅

## 8. 协作纪律

- 提交只 add 表格模块相关文件；并行线未提交改动（`insight/*`、`plugin/*`、
  `crates/app/src/main.rs`、`workbench/src/components/insight_actions.rs`、
  `README`、`docs/architecture/connection/execution-context-model.md`、
  `Cargo.lock` 等）不随本模块提交。`crates/settings/*` 自 A4 起本线合法持有
  `result_panel.fact_storage` 偏好注册 hunk（model / registry / lib 三文件），提交时只摘
  本线 hunk，settings 内其余并行线改动不随本线提交。
- 回归以 `cargo test -p rds-shared -p rds-settings -p rds-result-panel -p rds-editor --lib`
  为准；整包（`rds-workbench`）若被 insight 线未提交编译错误阻塞，如实上报而非绕过。
- 注释 / commit 信息简体中文（`feat:` / `fix:` / `refactor:` 前缀）。
