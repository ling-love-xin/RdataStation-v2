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
| 列浏览器（右栏页签） | ✅ 本轮交付（§8.12）：列浏览器并入右侧辅助栏，与值详情 / 计算 / 分组同栏四页签切换；清单 ↔ 网格双向选中联动；隐藏走眼睛、冻结/排序/筛选/列统计走右键；左栏已整体移除、宽度全部还给网格。**清单多选批量动作已于 2026-11-08 交付（§8.21：Ctrl 点击选集 + 隐藏/显示所选 + 清空选择）**，列序拖拽也已通链路（§2.4 核查更正）；剩余遗留：搜索词跨结果持久化口径、清单行类型图标、引用页签 |
| 渲染改进批次（clip / 分段语义 / JSON 美化 / 列宽记忆 / 千分位 / 列级统计） | ✅ 本轮交付（§8.13）：单元格 300 字符裁剪、状态行分段提示、JSON pretty、会话内列宽记忆、整数千分位、计算 / 分组列级访问器；延后：COUNT 精确总数、跨帧统计缓存、小数分组（**列宽落盘已于 2026-11-08 交付**，见 §8.7） |
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
| `SetColumnsHidden { columns, hidden }` | 【P2-3】清单批量显隐（隐藏所选 / 显示所选） | `set_columns_hidden`（`delegate.set_columns_hidden` + 刷新；隐藏仍保「至少 1 列可见」） |
| `ColumnSelectionChanged(Vec<usize>)` | 【P2-3】清单多选集合变化（delegate 是权威，回传面板镜像） | `set_multi_selected`（面板 `multi_selected` 驱动标题行「已选 N 列」+ 批量按钮） |
| `ClearColumnSelection` | 【P2-3】清单「清空选择」 | `clear_multi_selected`（面板镜像与 delegate 集合一起清） |
| `GoPage(PageTarget)` / `GoRow(usize)` | 【P1】状态栏翻页三钮 / 行号跳转 | `go_page` / `go_row` |
| `HexPage(usize)` | 【P2-1】值查看器 hex 转储翻页 | 宿主只把页号还回面板：`panel.set_hex_page(page, cx)` |
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
| 列布局持久化（顺序/宽度/隐藏） | blanco `ColumnLayout` 落盘 + 重放 | 同上 | **列序与列宽已交付（2026-11-08）**：列序走 `editor_session.rs` 的 `result_column_order` 编解码 + 宿主 `save_session_now`（`dispatch.rs:200-203`）；列宽走 `result_column_widths`（`列名=宽` 逗号串 + 老库 `ensure_column` 补列），面板订阅 `TableEvent::ColumnWidthsChanged` 存快照 → `ResultRequest::ColumnWidthsChanged` → `save_session_now`，`set_data` 之后按列名回填；开关 `result_panel.persist_column_widths` 读写两侧都判。**列隐藏仍只内存**（`grid/delegate.rs` 的 `hidden`），未落盘 |
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
- ~~列宽跨会话持久化~~（**已交付 2026-11-08**，见 §8.7 列布局持久化行）

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

> 上表与本节是 #26 时期的口径，**已被 §8.66 取代**：翻页段补上了真正的「下页」
> （`PageTarget::Next`，纯视图滚动），四颗行导航都在，位置也按 DBeaver `navCmds` 归位到
> `fetch.page` / `fetch.all` 之前；写操作段的内部顺序同样按 `editCmds` 更正。

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
4. 本轮清单只**单选**；多选批量动作（批量隐藏 / 冻结 / 复制列名）下一轮。**（2026-11-08 更新：批量隐藏 / 显示 / 清空选择已交付，见 §8.21；冻结 / 复制列名仍走单列右键。）**
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

**下一轮 backlog**：~~多选批量动作~~（已交付，见 §8.21）；~~列顺序拖拽~~（全链路已通，见 §2.4 核查更正）；搜索词是否跨结果持久化的口径复核；清单行类型图标（替代类型名文本后缀）。

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

**遗留 backlog**（沿用 §8.11）：~~多选批量动作~~（已交付，见 §8.21）；~~列顺序拖拽~~（全链路已通）；搜索词跨结果持久化口径复核；清单行类型图标；「引用」页签（外键面包屑 + 引用表只读预览）。

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
- ~~列宽落盘持久化~~（**已交付 2026-11-08**）：跨连接 / 库 / 表按**列名**恢复 `列名=宽` 快照；列集合变了（换表 / 列名重命名）对不上的列回默认档，不做 `ColumnLayout` 整体重放。
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
7. **多选批量动作**：列浏览器多选批量动作（需多选模型 + 批量动作入口）——**已于 2026-11-08 交付**（Ctrl 点击选集 + 隐藏/显示所选 + 清空选择，见 §8.21）。
8. **列序拖拽**：列顺序拖拽（`move_column` 钩子 + 列序模型与隐藏/冻结/转置映射改造）。
9. **列宽 backlog**：双击列头边缘自动适应内容宽度、右键列头「设置列宽」输入数值（跨会话持久化已于 2026-11-08 交付）。

> **核查更正（2026-11-08）**：本节是当年盘点快照，以下条目已不成立——④ `ResultRequest::FetchAll`
> 变体**已加**（`ports.rs`，宿主侧仍是单段取数、循环版未实现）；⑧ 列序拖拽**全链路已通**
> （见 §2.4 核查更正：组件 `col_movable` → 面板订阅 → `save_session_now` → 编解码）；
> ⑨ 列宽跨会话持久化**已交付**（见 §8.7 列布局持久化行）；⑦ 列浏览器多选批量动作**已交付**
> （见 §8.21）。①②③⑤⑥ 仍待做。

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

### 8.18 列浏览器清单 List 组件迁移（gpui-kit list）

**背景**：排查「gpui-kit 支持但我们手搓」后，列浏览器清单行（`div().cursor_pointer().on_click(...)` 手搓行）迁移到
`list::{List, ListState, ListDelegate}`，获得 hover / 键盘上下 + Enter 确认 / 焦点管理 / 搜索框（`searchable` 自带
cleanable）/ 滚动条 / a11y。新文件 `crates/result_panel/src/column_browser.rs`（`ColumnBrowserDelegate`），
panel.rs 删掉手搓过滤 / 行渲染 / 搜索框约 200 行。

**迁移关键事实（vendor 0.8.0 实测，供后续参考）**：

- **右键菜单不能挂 ListItem 本体**：`context_menu` 会把元素包装成 `ContextMenu<Self>`，而 List 要求
  `render_item` 返回 `ListItem`（List 内部要覆盖 `.selected()` / `.secondary_selected()`，`Selectable` 是硬约束）。
  菜单挂在**行内容 div** 上（`.id()` 带业务键 → `Stateful` 满足 `InteractiveElement + ParentElement + Styled`）；
  内容 div `w_full()`，右键命中区覆盖整行，与手搓版一致。内容 div 的 id 必须逐行唯一——
  同一调用点渲染多行时菜单状态按 id 区分（ContextMenu 的 RowsRoot 用例：#3134，无 id 会共享弹层状态）。
- **`ClickEvent::click_count` 是方法不是字段**（gpui 0.3.8）：双击判定写 `e.click_count() >= 2`。
- **`ListState` 0.8.0 的 `items_count` 签名带 `section`**：`items_count(&self, section: usize, &App) -> usize`。
- **result_panel 不直接依赖 gpui crate**：`gpui::*` 全部走 `gpui_kit::*` 顶层重导出（`div` / `SharedString` /
  `ClickEvent` / `IndexPath` 从 `gpui_kit::component::IndexPath` 等）。
- **双击无原生通道**：`ListItem` 无 `on_double_click`，单击留给 List 内部 confirm（SelectColumn），双击在
  `on_click` 里按 `click_count >= 2` 判定入队 LocateColumn；键盘 Enter 走同一 confirm 路径，不会产生 click_count。
- **搜索**：`searchable(true)` 内置搜索框，`perform_search` 收到每次输入；`set_query("")` 程序化清空不重发
  Change 事件（`clear_column_search` 借此实现）。搜索范围 = 列名 + 类型名 + 注释（`filter_browser_columns`
  纯函数迁至 column_browser.rs）；隐藏列保留在清单（置灰斜体）。

**正确性修正（发现并处理）**：`ColumnBrowserDelegate` 构造时持有的是**占位网格**（面板建好表状态前
`ListState` 要先建），`sync` 若只 `set_data` 不换网格，右键动作（冻结 / 隐藏 / 显示全部 / 清排序）会落在
空数据上。新增 `set_grid`，`sync` 浏览器段先 `set_grid(grid.clone())` 再 `set_data(...)`。

**验证**：`cargo check -p rds-result-panel --lib` 全绿（0 error / 0 warning）；
`cargo test -p rds-result-panel --lib` = **196 passed / 0 failed**（与历史基线一致，无新增测试——交互口径
未变，纯函数 `filter_browser_columns` / 菜单素材测试沿用既有）；
`cargo check -p rds-editor` 通过（连带验证 host.rs 的 `set_selection_pos` 3 处 `window` 适配；
编译过程中并行线 Toolbar 迁移曾以中间态报 `Toolbar::child` 的 `Sizable` 约束错，该线改完（`child`→`content`）
后编译通过，非本线因果）。

**提交状态**：`crates/result_panel/src/{column_browser.rs, panel.rs, lib.rs}` 已提交（`2d295be6`）；
`crates/editor/src/view/host.rs` 与并行线 Toolbar 迁移混合，按纪律不提交（待该线或用户裁定）。

### 8.19 四行布局重构（2026-10-28 拍板）

**骨架**（用户拍板口径：「第一行 tab 名，第二行工具栏，第三行（左|内容|右），第四行状态栏」）：

```
┌─ 第一行 结果集标签条（跨全宽；含 C1 重命名条） ──────────────┐
├─ 第二行 工具栏（跨全宽） ─────────────────────────────┤
├─ 第三行 主体：左栏 | 内容 | 右栏（对比模式双栏） ───────────┤
├─ 第四行 状态栏（跨全宽；左端左栏开关 · 右端右栏开关） ───────┤
```

- **左右栏 DBeaver 式**：默认展开（对齐 DBeaver 常驻），状态栏两端开关可**完全收起**
  （收起 = 不占宽度，不是折叠成窄条）。左栏 = 网格 / 文本 / 记录竖条
  （`show_left_sidebar`，默认 true）；右栏 = 列浏览器 / 值详情 / 计算 / 分组同栏页签
  （`show_value_sidebar`，默认 true）。
- **开关位置**：状态栏最左端 `PanelLeft` ↔ `PanelLeftOpen`（新按钮，
  `ResultRequest::ToggleLeftSidebar` → host 分派 `toggle_left_sidebar`）；
  最右端 `PanelRight` ↔ `PanelRightOpen`（原工具栏「列浏览器」文字按钮迁来，tooltip 联动开合文案）。
- **文本视图不提前 return**：文本只是第三行主体的一种内容，四行骨架统一组装；
  文本视图有状态栏，右栏让位（无选中格语义），只留左栏开关。
- **对比模式**：主体 = 主栏（竖条可收起 + 主 pane）‖ 对比 pane，左右辅助栏让位
  （三栏会把网格挤没）；状态栏照常渲染第四行（`status_controls` 与主栏同一批动作）。

**实现要点（AnyElement 不实现 Clone 的约束）**：

- `grid.rs::render_status_row` 从 render 内状态行⑦抽出（面板级第四行调用），
  签名**按值收 `ResultControls`**——状态动作按钮（more / fetch_all / 行级 / 撤销重做）
  只渲染一处：面板解构 `controls` 后重组 `status_controls`（移动，不 clone），
  `ResultPane.controls = ResultControls::default()`（grid.rs render 解构 `..` 忽略，
  不再读按钮字段）。
- `ResultPane.status / tabs / row_count_input` 全部 `None`（标签条提面板级第一行、
  状态行提第四行、行数框随状态栏）；`elapsed_ms` 在 `toolbar` 移动进 pane **之前**取出。
- 面板级根 `debug_selector("editor-result-root")`；主体行保留
  `editor-result-sidebars`（普通 / 文本）与 `editor-result-compare-layout`（对比）语义。

**验证**：`cargo test -p rds-result-panel --lib` = **196 passed / 0 failed**；
`cargo check -p rds-editor` 通过（host.rs 分派新增 `ToggleLeftSidebar` 分支）。

**提交状态**：`crates/result_panel/src/{grid.rs, panel.rs, ports.rs}` +
`crates/editor/src/view/host.rs`（仅分派分支；与并行线 Toolbar 迁移混合，待裁定）待提交。

### 8.20 类型样式设置（2026-10-28 拍板：按类型族 / 全局统一 / 预设色板）

**口径**（用户逐项拍板）：① 按类型族配置（五族：数字 / 文本 / 时间 / 布尔 / BLOB）；
②「位置」= 值在格内的对齐位置（与「对齐」合并为一项）；③ 全局统一生效
（先不做 per-结果集覆盖）；④ 预设色板（主题语义 token 派生，零裸色值约束不变）。

**23 项设置**（`settings.json` 的 `result_panel` 节；设置页「结果集面板」分组，全部即时生效）：

| 设置 | 枚举值 | 默认 | 渲染消费点 |
| --- | --- | --- | --- |
| `number_format` | raw / grouped / fixed2 / scientific | raw | `grid.rs::format_cell_text`（数字族） |
| `null_placeholder` | null / emptyset / dash | null | `grid.rs::format_cell_text`（`text == "NULL"` 判定，只换显示） |
| `bool_display` | true_false / cn / digits | true_false | `grid.rs::format_cell_text`（布尔族） |
| `align_numeric/text/temporal/bool/blob` | inherit / left / center / right | inherit | `grid.rs::cell_style`（对齐覆盖） |
| `color_numeric/text/temporal/bool/blob` | inherit / info / success / warning / danger / primary / muted | inherit | `grid.rs::render_td`（角色 → `theme.colors.*`） |
| `bg_numeric/text/temporal/bool/blob` | inherit / info / success / warning / danger / primary / muted | inherit | `grid.rs::render_td`（语义角色 → `theme.colors.*.opacity(0.16)`；`Custom` → `Colorize::parse_hex`） |
| `font_numeric/text/temporal/bool/blob` | inherit / bold / italic / underline / line_through | inherit | `grid.rs::render_td`（→ `font_weight` / `italic` / `underline` / `line_through`） |

**背景色与字体样式补充口径**（2026-10-30 落地）：

- **背景色**：语义角色一律渲染为主题语义色 `opacity(0.16)` 浅底（与脏格 / 冲突格
  底色同透明度口径，不引裸色值）；自定义 hex 走 `settings.json` 手写
  `{"bg_numeric":{"custom":"#ff0000"}}`（serde 反序列化路径，渲染用 gpui-component
  `Colorize::parse_hex`，非法值回退无背景）。设置页当前只出语义角色下拉，
  交互式选色盘（`color_picker::ColorSelect`）属后续批次。
- **字体样式**：`inherit` = 跟随类型默认（NULL 灰斜体、被删行删除线等既有语义保持），
  显式五档与之叠加时按用户样式优先呈现；与 `monospace`（布尔 / 时间族等宽）互不冲突。
- **背景色优先级**：类型背景 < 本地新增行底色 < 脏格 / 冲突格底色（后两者是更高语义）。

**模型归属**（打破循环依赖的关键决策）：枚举与聚合 `TypeStylePrefs` 放
**`crates/workbench_shell/src/result_style.rs`**（与 `product_tokens` 同例）——
`settings`（持久化 + 设置页）与 `result_panel`（渲染消费）双向依赖环由此解开；
枚举 parse 对未知落盘值回退默认（不拒绝写入、不炸设置文件）。

**生效链路**（三层）：

1. **启动**：editor `host.rs` 构造面板时读 `settings.result_panel.type_style()` 同步
   （`panel.set_type_style` → delegate；不随 `set_data` 清掉——视图偏好口径同
   `show_column_comments`）。
2. **设置页变更**：`settings` apply 分支写 global + persist；
   workbench `view.rs::render_settings_panel` 的 `on_settings_changed` 钩子
   遍历 `editor_hosts` 调 `host.apply_result_style(cx)` → 面板 → delegate 重绘。
3. **渲染**：`grid.rs::render_td` 用 `cell_style(kind, &type_style)`（对齐覆盖 +
   前景色角色）+ `format_cell_text(kind, text, prefs)`（NULL 占位 / 布尔映射 / 数字格式）。

**显示 / 取值分离**（用户已确认）：所有格式只影响**渲染显示**；复制 / 导出 /
编辑 / 悬停 tooltip 仍用驱动原始值。NULL 判定仍按原文 `== "NULL"`，灰斜体渲染不变；
被删行灰字优先级高于类型染色。JSON 美化与 BLOB 摘要先于 `format_cell_text` 处理。

**验证**：`cargo test -p rds-workbench-shell` = 13 passed（含 3 个新增 parse/往返测试，
新增 `BgColorPref` / `FontStylePref` 回退、23 项默认全继承、自定义背景 serde 往返）；
`cargo test -p rds-result-panel --lib` = **213 passed / 0 failed**（`CellStyle` 含 `bg` /
`font_style` 后保持）；`cargo test -p rds-settings --lib` = **34 passed / 0 failed**
（`every_model_leaf_is_registered` 契约测试已过：23 项全登记，并行线阻塞已消除）；
`cargo check -p rds-workbench-shell -p rds-settings -p rds-result-panel` 通过。

### 8.21 P2 批：值查看器 hex 翻页 / 图片预览 + 列浏览器多选（2026-11-08）

（遗留盘点顺序表里的 P2 三项；P0 / P1 见 `result-panel-handoff.md` §2.12 与 §2.4 核查更正块，本仓提交 `7a9afb5c` / `bbf81746`。）

| 项 | 落地要点 |
| --- | --- |
| P2-1 hex 转储翻页 | 原实现固定 `hex_lines(&bytes, 0, 16)`（只显示首页 256 字节）；现加 `HEX_LINES_PER_PAGE = 16` + 纯函数 `hex_page_clamp(total_bytes, page) -> (page, total_pages)`（空值也算 1 页、页号夹末页）+ `hex_page_button`（到边界禁用）；底部页栏「第 n/N 页 · 共 X 字节」+ 上一页 / 下一页。**页号是纯视图态**（`ResultPanel.hex_page`），按钮经 `ResultRequest::HexPage(usize)` 回面板——tab 的 render 只有 `&App`、拿不到 `&mut Context`，故走 `queue.request(.., app)` 而非 `cx.listener`；`set_selection_pos` 里复位 `hex_page = 0` |
| P2-2 图片解码预览 | 原 `ViewerMode::Image` 只渲一行「格式 · 字节数」占位。现 `grid/cell.rs::image_format_of(name)`（PNG / JPEG / GIF / WebP → `ImageFormat`）→ 事件路径 `rebuild_value_tree` 把 hex 解成 `Arc<Image>` 存 `ResultPanel.value_image` → render 摆 `img(ImageSource::Image(..)).w_full().max_h_full()`，解码失败 `with_fallback` 回落文案、格式不在支持集回文本占位。**为什么解码不在 render**：`Image::from_bytes` 只内存构造（`id = hash(bytes)` 命中资产缓存），真解码在 `img` 的 layout/paint 期走 `use_asset::<AssetLogger<ImageDecoder>>`，render 仍是纯读路径。**布局陷阱**：`Img` 在 `size` 为 `Auto` 时按真实像素写 size（大图会撑爆面板），必须显式给宽 + 高度上限 |
| P2-3 列浏览器多选 | `List` 每次点击强制覆盖 `selected_index`、只透传 `confirm(secondary)`（vendor 0.8.1 `list/list.rs:516-526`），没有集合语义 → 集合由 `ColumnBrowserDelegate.selected_set` 自持：`next_multi_selection(current, clicked, additive)`（Ctrl 增删 / 普通单选替换 / 升序去重）+ `render_item` 按集合自绘底色；集合经 `ColumnSelectionChanged` 回传面板镜像 `multi_selected`，标题行出「已选 N 列」+ 隐藏所选 / 显示所选 / 清空选择三钮；批量显隐走 `SetColumnsHidden { columns, hidden }` → `delegate.set_columns_hidden`（逐列复用 `hide_column`，隐藏仍保「至少 1 列可见」底线）。`sync` 换投影时按列数剔除越界列号后回灌 delegate（`set_data` 清集合，不回灌则刷新一次丢选集）。列浏览器标题行随之由固定 `h(1.5rem)` 改为 `min_h`——`Button::small()` 是 24px（vendor `button.rs` `Size::Small => h_6`），加上下 `py_1` 会超出 1.5rem 被压 4px。 |

**验证**：`cargo test -p rds-result-panel --lib` = **218 passed / 0 failed**；
`cargo check -p rds-result-panel -p rds-editor` 通过（0 warning）。
**未验证项（如实登记）**：`.w_full()`（Relative）+ `.max_h_full()` 在**真实窗口**下能否夹住超大图、`ObjectFit::Contain` 是否按预期生效——只通过编译与纯函数单测，未跑真窗口帧测。

### 8.22 列隐藏 / 冻结落盘（盘点首项，2026-11-08）

**为什么要做**：竞品对照盘点的第一优先项——隐藏 / 冻结的**能力**早在 P2 批就有
（`grid/view_rows.rs::hide_column` / `toggle_freeze`），会话通道也已有（列序 / 列宽），
缺的只是"把这两张清单写进会话、重启后再读回来"这一层。

**数据口径（列名，不是下标）**：`ResultPanel` 新增 `column_hidden` / `column_frozen`
（`Vec<String>`），与 `column_widths` 同层、同生命周期：跨结果集保留、按列名匹配。
不用下标的原因：`set_data` 前会按持久化列序**物理重排** `columns` / `rows`
（`panel::events::apply_column_order` → `reorder_by`），下标跨会话不稳定。

**delegate 能力**（`grid/delegate.rs`）：

| 方法 | 语义 |
| --- | --- |
| `hidden_column_snapshot` / `frozen_column_snapshot` | 把 `hidden` / `frozen`（数据列下标）映射成列名（`columns.get(..)`，越界静默丢弃） |
| `apply_hidden_columns` / `apply_frozen_columns` | 按列名找数据列回填；**找不到的列名静默跳过**（换了表 / 列改名就回默认）；隐藏逐列走 `hide_column`，因此「至少 1 列可见」的底线与交互路径同一份实现；冻结去重后 `sort_unstable` |

**回灌时序（关键）**：`panel::lifecycle::sync` 的 `grid.update` 闭包内，必须在
`set_data` **之后**（它会 `clear()` 列宽 / 隐藏 / 冻结三张表）依次回填
列宽 → 隐藏 → 冻结。且必须在闭包**内部**完成——在同一 `grid.update` 里再去
`self.grid.read(cx)` 会重入 panic。

**触发与落库（只推一条无载荷请求）**：

- 新枚举 `ResultRequest::ColumnLayoutChanged`（`ports.rs`）。
- 面板 `snapshot_column_layout(cx)`：读 delegate 两个快照 → 与面板字段比 → **真变了才**
  `queue.push(ColumnLayoutChanged)`（避免一次点击两次写库）。
- 宿主 `dispatch_result_request` 的 `ColumnLayoutChanged` 分支：先
  `panel.snapshot_column_layout(cx)`（列头菜单 / 列头右键 / 列浏览器右键是**就地改 delegate**
  的，面板快照这时还没跟上）再 `save_session_now(cx)`。
- 列头菜单（`grid/header.rs` 的 `ToggleFreeze` / `HideColumn` / `ShowAllColumns`）、
  单元格右键菜单（`grid/menu.rs` 同名三项）、列浏览器清单右键（`column_browser.rs` 同名三项）
  改完 delegate 后 `queue.request(ColumnLayoutChanged, app)`。它们拿不到面板方法，所以经队列。
- `set_grids(grid, compare_grid, cx)` 把 `action_queue` 注入主网格与对比网格的 delegate
  （delegate 字段 `action_queue: Option<ResultActionQueue>`）。`None` = 独立网格（单测），
  菜单退回就地切换、不推请求。
- 单列眼睛（`ToggleColumnVisibility`）与批量显隐（`set_columns_hidden`）在**面板方法内**
  改 delegate，随后直接调 `snapshot_column_layout`；不再显式 `save_session_now`，
  靠 drain 循环处理冒出来的 `ColumnLayoutChanged` 落库（一次操作一次写库）。

**存储六层**（与列宽同一条通道）：

1. `editor::notebook::session::SavedSession` 加 `result_column_hidden` / `result_column_frozen`
   （`Option<Vec<String>>`，`None` = 没改过）。
2. `editor::view::host::session` 保存块取面板快照（空则 `None`）、恢复块注入面板
   （`set_column_hidden` / `set_column_frozen`，落到 delegate 由下一次 `sync` 回填）。
3. `editor::ports::editor_session` 编码为逗号串 / 解码（空串 → `None`）。
4. `store::persistence::workbench_context_store::EditorContext` 加两个 `String` 字段；
   `CREATE TABLE` 加两列 + `ensure_column` 幂等补老库。
5. INSERT 扩到 `?1..?18`；三处 SELECT（单份 / 最近一份 / 全部）同步加列，
   `row.get(15)` / `get(16)`，`updated_at_ms` 顺移到 17。
6. `editor::view::tests::channels` 四处 `SavedSession` fixture 补两个 `None`。

**开关口径**：不复用 `settings.result_panel.persist_column_widths`——那个开关只针对列宽记忆；
隐藏 / 冻结是"用户怎么摆这张表"，与列序同语义，总随会话走。

**已知限制（如实登记）**：列名本身含 `,` 时逗号串会被拆错（与列宽串的既有取舍一致；
只影响恢复，不会写坏数据）。要彻底解决得把会话串升级成带转义的编码，与"列宽键粒度
升级为 `connection/db/table`"一并处理（盘点表里已标为待用户拍板项）。

**验证**：`cargo test -p rds-result-panel --lib` = **220 passed / 0 failed**
（218 + 2 新增：`column_layout_snapshots_round_trip_by_name`（换列序后按名字钉对列、
找不到的列名跳过）、`apply_hidden_columns_still_keeps_one_visible_column`）；
`cargo test -p rds-store --lib` = **163 passed / 0 failed**（+1 新增
`editor_context_round_trips_column_layout_strings`，三条 SELECT 路径同值）；
`cargo test -p rds-editor --lib` = **751 passed / 0 failed / 1 ignored**
（并行线书签用例已由其 `72823e9e` 修好）；
`cargo check -p rds-result-panel -p rds-editor -p rds-store` 通过（0 warning）。
**未验证项**：真窗口里"隐藏 → 重启 → 列仍隐藏"的端到端，只到 headless 单测 + SQL 往返，
未跑一次真实的关窗重开。

### 8.23 列宽自适应（双击列头分隔线，2026-11-08）

**为什么要做**：竞品对照盘点的第二优先项。能力入口早就在组件库里（双击 resize handle →
`TableState::fit_col` → `TableDelegate::fit_column`），但本仓从未实现 `fit_column`（trait 默认
返回 `None`），所以双击一直是"什么也不发生"——这不是新造能力，是把已有钩子填上。

**组件链路（vendor 权威，勿凭记忆）**：

| 位置 | 事实 |
| --- | --- |
| `table/state.rs:1730-1737` | resize handle 的 `on_mouse_down`：`click_count == 2` → `fit_col(ix)`；`1` → 清 `fitted_col` |
| `table/state.rs:1292-1321` | `fit_col`：问 delegate 要内容宽 → 加列 padding（`column.paddings`，否则 `size.table_cell_padding()`；本仓 `RESULT_TABLE_SIZE` = `Size::XSmall`，左右各 4px）→ 按 `scale_factor` 逐段 `ceil` 到整设备像素（文本宽哪怕差一个分数像素也会被截断）→ `resize_cols` → 变了才 emit `TableEvent::ColumnWidthsChanged`（0.9.0 复核：原记 1284 起，升级后为 1292；`sizing.rs:69-76` 的 XSmall 仍上下 2 / 左右 4，未变） |
| `table/state.rs:280-285` | `fitted_col` 只用于屏蔽"双击后紧跟着的那一下拖拽"，**不会**每帧重算（一次双击 = 一次采样） |
| `table/column.rs:219-221` | 列宽上界默认 1200px，组件侧已兜住，delegate 不必自己夹 |

**实现分工（新模块 `grid/fit.rs`）**：只回答**内容宽度**，padding / min-max / 事件全由组件做；
**不写 delegate 状态**——列宽随后的 `ColumnWidthsChanged` 回灌（`panel/lifecycle.rs` 已订阅）
走的就是拖拽同一条通道，因此落盘 / 跨会话恢复零新增接线。

**宽度构成（与渲染同口径，不是估算）**：

- 表头：类型 badge（`px_1` 左右各 4 + 符号宽）+ 列名 + 排序箭头（`↑`/`↓`）+ 筛选 `▽` +
  列头菜单，两两之间 `gap_1` 4px。**列头菜单不算**：2026-11-08 §8.29 把它改成**绝对定位**
  （贴主行右端、悬停表头才显形），既不占列宽也不参与测宽，`RESULT_FIT_COLUMN_MENU` 已删除。
  （§8.26 时期它曾并进主行流内恒吃 24px、测宽因此把它算进来；那条口径随 §8.29 一并作废。）
- 另注：组件 `render_th` 自带一个排序图标槽（`justify_between` 的右端），实测占约 16px，
  这一块**不在** delegate 的测宽里也不该在——它由组件自己预留。
- 单元格：前 **200 个视图行**（`FIT_SAMPLE_ROWS`，对齐 dbdelve `FITTED_ROWS=200` / cellar 前
  200 行）里最宽的那串 + 左右各 8px（`render_td` 的 `px_2`）。测的是**画出来的那串**：类型样式
  （NULL 占位 / 千分位 / 布尔中文）、JSON 美化、BLOB 摘要、`CELL_DISPLAY_LIMIT=300` 截断全都
  算在内；布尔 / 时间族用主题等宽字体测（与 `cell_style` 的 `monospace` 一致），否则数字列会
  量偏窄。
- **双层内边距**：delegate 自己加 16px（`px_2` 左右各 8），组件再按 `RESULT_TABLE_SIZE`
  （= `Size::XSmall`，`table_cell_padding()` 左右各 4）加 8px，合计 24px。只算一层的话
  文字正好压在余量里被省略号吃掉，双击就白点了。
- 行号槽返回 `None`（组件侧 `resizable(false)` 已先拦，这里再守一道）。转置视图照常自适应
  （列名取「字段 / 值」，列级类型 `Unknown` → 默认字体）。

**测量 API**（gpui-pre 0.3.10 实查）：`window.text_style().font()` 取当前字体 →
`window.text_system().shape_line(text, font_size, &[TextRun{..}], None).width()`（字形推进宽）。
字号 `text_xs()` = `rems(0.75)` = `0.75 × window.rem_size()`，表头与单元格都这一档。`shape_line`
遇换行符会 panic：单行单元格里换行被当空白，测宽时替换成空格。

**验证**：`cargo test -p rds-result-panel --lib` = **221 passed / 0 failed**（+1
`fit_column_follows_content_and_caps_the_sample`：行号槽 `None` / 长内容列更宽 / 第
`FIT_SAMPLE_ROWS` 行之后的超长值不进采样；断言取相对比较，不写死像素）；
`cargo check -p rds-result-panel -p rds-editor -p rds-store` 通过（0 warning）。
**未验证项**：真窗口里双击列头分隔线的端到端（headless 只到 `fit_column` 的返回值，没走真实
鼠标双击 + `ColumnWidthsChanged` 落库）；真实字体下"每列多 16px 观感是否偏宽"也没目视。
（§8.24 落地后同一条命令 = 222 passed。）

### 8.24 类型默认列宽（2026-11-08）

**为什么要做**：盘点第三项。列宽此前是**一刀切 128px**（`RESULT_COLUMN_WIDTH`）：一份
`id / created_at / payload` 的结果新建出来三列一样宽——时间戳被截、id 白占地方。竞品（tusk）
按类型给初值，这里把同一思路落到我们自己的**语义类型**上。

**优先级**：`column_widths`（用户拖过 / 双击自适应过，按列名落盘）> 类型默认宽 > 128。
用户一旦动过这一列，类型档就永久让位——初值只负责"第一次看到时像样"。

**口径（按 `ColumnKind`，不按驱动原始类型名）**：`int` / `int4` / `bigint` 都是数字列，宽度该
一样；`ColumnKind` 已是归一化语义类型（`ColumnKind::parse` 一张词表，见 `store/entry.rs`）。
未知类型回到 128，守住「类型缺失时一个像素都不变」的既有原则（驱动没填类型时，所有列仍是
今天的样子）。

| `ColumnKind` | 宽 | 依据 |
| --- | --- | --- |
| Integer / Float / Decimal | 130 | tusk numeric 130；容得下 `-1234567890.12345` |
| Boolean | 96 | 值很短（`true` / `是`），宽的是表头 badge + 列名 |
| Date / Time | 120 | `2026-11-08` / `12:34:56` 十来字符，留余量 |
| Timestamp | 210 | tusk timestamptz 210；`2026-11-08 12:34:56.789 +08` 二十多字符 |
| Uuid | 300 | tusk uuid 300；36 字符不截断 |
| Json | 260 | tusk json 260 |
| Binary | 160 | **不照抄** tusk bytea 220——我们画的是 `BLOB · N B` 摘要，不是整串 hex |
| Text | 200 | 竞品没给；初值能读一小段话，长文本靠悬停全文 + 双击自适应 |
| Unknown | 128 | 与现状一致 |

推导口径（竞品没给的几档）：字号 `text_xs` = 12px，西文数字平均宽 ≈ 0.55 × 字号 ≈ 6.6px/字符；
单元格左右内边距合计 24px（自身 `px_2` 16 + 组件 `RESULT_TABLE_SIZE` XSmall 档 8）。

**落点**：

- `ui.rs`：8 个 `RESULT_COLUMN_WIDTH_*` 常量（结构尺寸进常量表，视图里不出现裸 `px`）。
- `grid/table_impl.rs::default_column_width(kind)`：**穷尽 match**——新增 `ColumnKind` 变体会
  编译失败，逼着补档。
- `column()`：`column_widths.get(name)` 命中就用它，否则 `default_column_width(column_kind(col_ix))`。
  时序安全：`sync` 里 `set_column_types` 排在 `set_data` 之后、`refresh(cx)` 之前
  （`panel/lifecycle.rs:313` / `:338`），所以 `column()` 第一次被问时类型已就位，不会"先 128 再跳"。
- 转置视图的「字段 / 值」两列仍走 128：混合类型没有可用的类型档（转置时 `column_kind` 为
  `Unknown`）。

**已知边界**：类型默认宽只影响"没有落盘记录"的列；`column_widths` 按**列名全局记忆**（同名不同
表会串，已登记为待拍板项），所以同一列名在另一张表里会沿用旧宽度、不回到类型档。

**验证**：`cargo test -p rds-result-panel --lib` = **222 passed / 0 failed**（+1
`default_column_width_follows_type_until_the_user_drags`：11 档全覆盖表 + uuid 300 / boolean 96
走真实 `column()` + 拖过后按用户值）；`cargo check -p rds-result-panel -p rds-editor -p rds-store`
0 warning。
**未验证项**：真实窗口里"新建一份结果 → 各列初始宽度看着合不合适"没目视；
`RESULT_COLUMN_WIDTH_TEXT` = 200 对中文长文本列是否仍偏窄没实测。

### 8.25 类型感知单元格编辑（2026-11-08）

**为什么要做**：盘点第四项。此前**已经有了**类型感知的**提交校验**（`validate_edit_value`：
整数列只收整数、布尔列只收 true/false/1/0…、日期列只收 `YYYY-MM-DD`、UUID 查形状），
缺的是**编辑器本身**——只有一种 `Input`，日期列要用户手敲 `2026-11-08`、布尔列要手敲
`true`。竞品（dbdelve §3.3）按列类型给控件，这里把同一思路落到自己的语义类型上。

**范围（用户已确认的口径）**：

| 列类型 | 编辑器 | 值怎么提交 |
| --- | --- | --- |
| Boolean | `true` / `false` 双态按钮（`Button` + `.toggled()` 显示当前值） | 点一次即定值提交 |
| Date | `DatePicker`（自带日历弹层 + 也能手敲，格式 `%Y-%m-%d`） | 选一次即提交，`DatePickerEvent::Change` → `YYYY-MM-DD` 字符串 |
| Integer / Float / Decimal / Text / Uuid / Json / Binary / Time / Timestamp / Unknown | 仍用 `Input` | 手敲 + Enter / 失焦 |

**不做**：`Time` / `Timestamp` 不换时间字段控件——时刻与时区的书写口径各驱动不同
（`12:34:56` / `12:34:56.789` / `+08` / 无时区），控件产出的是"日期 + 时间"复合值，
再转回字符串会把驱动原有格式抹平；JSON 要校验 + 美化，是另一个量级，仍用 `Input`；
enum 类 `Select` 需要"值 → 显示"映射来源，当前拿不到枚举定义，不做。

**硬约束（不破既有链路）**：两种新控件都**不新开提交路径**——布尔走
`commit_edit_as(value)`，日历走同一条 `commit_edit_as(YYYY-MM-DD)`；两者都是
「写进编辑态 → `commit_edit`」，因此类型校验、`CommitEditHook`（变更集 upsert）、脏格 /
冲突投影、撤销栈、提交被拒时的红框提示全部原样生效。`commit_edit_as` 返回 `false`
（校验没过）时编辑态与红框保留，与手敲 Enter 被拒的表现一致。

**落点**：

- `grid/delegate.rs`：新增两个复用实体字段 `edit_input`（原有）/ `edit_calendar`（新），
  由宿主注入——与编辑框一样是**一份复用**，不给每格建实体。
- `grid/edit.rs::render_edit_cell`：按 `column_kind(col_ix)` 分叉（`column_kind` 吃的是
  **含行号槽的组件列序**，内部自己 `data_column(col_ix - 1)`）；返回类型从 `impl IntoElement`
  改成 `AnyElement`（两条分支元素类型不同）。布尔分支**不碰 `InputState`**，所以放在
  `expect("宿主必注入编辑框实体")` 之前——没注入编辑框的独立网格也能用。
- `crates/editor/src/view/host/mod.rs`：与 `edit_input` 并排创建
  `DatePickerState::new(window, cx).date_format("%Y-%m-%d")`，`subscribe_in` 订阅
  `DatePickerEvent::Change` → `format("%Y-%m-%d")` → `commit_edit_as` → 已提交则
  `sync_result_view`（与 Enter 分支同款），未提交则 `window.refresh()` 让红框上屏。
- 日历状态由 `vendor/gpui-component/src/lib.rs:139` 的 `date_picker::init(cx)` 注册
  （`gpui_kit::init` 已包含），无额外初始化接线。

**验证**：`cargo test -p rds-result-panel --lib` = **223 passed / 0 failed**（+2：布尔列
`boolean_edit_commits_through_the_same_channel`（钩子收到 原值 `yes` → 新值 `true`、
提交后编辑态清空、非法值被拒并保留编辑态）；`cargo check -p rds-result-panel -p rds-editor`
0 warning；`cargo test -p rds-editor --lib` = 761 passed / **2 failed**，两条是
`view::tests::legacy::opening_an_analysis_document_builds_a_cell_editor_per_cell`（`legacy.rs:58`）
与 `view::tests::notebook_win::analysis_cell_renders_multiple_output_blocks`——升级报告 §7
第 232 行已登记为「rebase 前基线就是这 2 条」，与本次改动无关（都是 `try_find` 读
元素注册表，属 gpui-fast 观察层口径）。

**未验证 / 已知边界**：

- 真窗口里点日历选日、点布尔按钮的端到端没目视（headless 下日历弹层与按钮点击都在
  元素观测层，gpui-fast 开启后点击类断言不可靠，只到纯逻辑与编译）。
- 日历**不预置当前值**：打开落在"今天所在月"，不是这一格原来那一天。要预置得把
  字符串解析成 `NaiveDate`（引 chrono 或走 `calendar` 模块的解析），本轮没做——对
  "改成另一个日期"够用，对"在原日期上挪一天"要多点一步。
- 布尔按钮文案用 `true` / `false`（数据库值本体），不跟随单元格的显示格式（如 `是`/`否`
  或 `✔`/`✘`）：编辑态写的是要提交的值，不是展示样式；若将来单元格显示改成中文，
  这里仍保持 ASCII 值。
- 转置视图是只读，不进入编辑（`begin_edit` 已先拦），所以转置里看不到这些控件。

### 8.26 表头列名被裁修复（2026-11-08）

**现象（用户截图实证）**：结果集表头带里没有列名，只剩贴着上沿的一条彩色残线（类型 badge
的色块上半截）和右端一个向下箭头（列头菜单）；行号槽 `#`、数据行、筛选框、工具行都正常。

**根因链（vendor 0.9.0 权威源码实查）**：

1. `Sizing::XSmall.table_row_height()` = **26px**（`vendor/gpui-component/src/sizing.rs:57-65`）；
   `table_cell_padding()` = 上下 2 / 左右 4（同上 `:69-96`）→ `render_cell` 的 `table_cell_size`
   吃掉上下 4px，**表头内容高只剩 22px**。
2. `TableState::header_band_height()` = `header_row_height() * header_layout.len().max(1)`
   （`table/state.rs:807-818`，未设 `header_row_height` 时取 `table_row_height()`）。
3. `TableState::render_th` 是一层 `h_flex().h_full()` 包 `render_cell(...)`（`:1878-1911`），
   `render_cell` 自身 `h_full().overflow_hidden().whitespace_nowrap()`（`:1514-1550`）。
4. 本仓 `grid/header.rs::render_th` 把表头造成 `v_flex` **两行**：主行（badge + 列名 +
   排序箭头 + 筛选 ▽）用 `size_full()` 占满 22px，第二行是列头菜单（`d7af6f5d` 引入）。
   两行挂上去后 flex 先满足 `flex_none` 的 28px 菜单，主行被分到 **0 高** → 列名整行不可见，
   只剩 badge 的彩色上沿和菜单箭头。

**修复**：

- 主行 `size_full()` → **`w_full()`**（纵向交给容器，不再抢全部高度）；
- 容器 `head` = `v_flex().size_full().min_w_0().justify_center()`，主行在其中垂直居中；
- 列头菜单从 `head` 的**第二行**并进 `main_row` **尾部**（`flex_none`，与列名同一行右端），
  外层还包一层 `on_click(stop_propagation)` 的守卫，避免点菜单触发列头排序；
- 提前 return 分支（转置 / 行号槽 / 空列名）同步改成 `justify_center` 并前置到接菜单之前
  （避免白构造菜单项）。

**同一改动的另一半（测宽口径）**：菜单进了流内就**真实占列宽**（XSmall 档实测 24×24），
而 `grid/fit.rs` 原先按「不含菜单」测宽。已在 `crate::ui` 新增 `RESULT_FIT_COLUMN_MENU = 24px`
并计入 `header_width`——否则双击自适应算出的宽度差这一块，列名照样被省略号吃掉。

**已知取舍（同轮已解决，见 §8.27）**：表头带只有 22px 内容高，`comment_line`（列注释
第二行）在这一带里必然被裁。当时判断「要两行都可见得抬 `header_row_height`」，随即
按这条做了：注释开关开启时表头带由 delegate 抬到 40px。

**验证**：`cargo test -p rds-result-panel --lib` = **224 passed / 0 failed**（+1
`header_main_row_keeps_a_readable_height`：真实 `TableState` + `DataTable` 渲染进 headless 窗口，
读 `debug_bounds` 断言主行高 14–22px、列头菜单与主行 y 差 < 4px——旧实现下主行高 0、菜单在
第二行，两断言都会挂；同时实测菜单占宽 24px，与 `RESULT_FIT_COLUMN_MENU` 一致）。
`cargo test -p rds-ui-guard` = 4 passed / 0 failed（`ui.rs` 裸 px 基线 15 → 16）。

**未验证项**：真实窗口下的目视（headless 只证到几何，未目视字体渲染与 badge/箭头位置）；
滚动时表头带是否稳定；窄列（`RESULT_COLUMN_WIDTH_BOOLEAN` = 96px）在菜单占 24px 后
列名剩余宽度是否够读，未目视。

> **2026-11-08 后续（§8.28 / §8.29 已改写本节两处结论）**：①「窄列 96px 列名够不够读」
> 实测不够——扣掉 badge/菜单/gap 只剩 37px，布尔档已抬到 120；②「菜单进流内所以测宽要算
> 24px」已作废——菜单改回**绝对定位**（悬停才显形），不占列宽也不参与测宽，
> `RESULT_FIT_COLUMN_MENU` 常量已删除。

### 8.27 注释表头两行高度 + 表头尺寸口径收敛（2026-11-08）

**问题（实证）**：`show_column_comments` 是面板工具栏的「注释」开关，`delegate` 注释写的是
「开启后列头变高（两行：列名 + 注释小字）」——但**表头高度从来没跟着变**。实际路径：
开关只是让 `render_th` 多画一个 `comment_line`，表头带仍是组件 `table_row_height()`
（XSmall = 26px，扣 padding 只剩 22px 内容高），两行挤 22px 各被 `overflow_hidden` 裁掉
一半，主行还会被压到读不出字。等于开关有一半是空转的（开了只多出一个被裁的残条）。

同时清掉两处**零引用死常量**（`grep` 全 crate 只命中定义行）：

| 常量 | 原值 | 实情 |
| --- | --- | --- |
| `RESULT_HEADER_HEIGHT` | `2.0`（2rem = 32px，「列名行高」） | 表头高一直由组件按密度档给 26px；32 是设计稿口径遗留，与实现不符（也已**删除**） |
| `RESULT_HEADER_TYPE_FONT_SIZE` | `0.6875`（11px，「类型小标签字号」） | badge 实际用 `text_xs`（12px）；规格没落地 |

**修复**：

- `ResultGridDelegate::header_row_height() -> Option<Pixels>`：**只有**注释开关开启时返回
  `ui::RESULT_HEADER_COMMENT_ROW_HEIGHT`（40px），单行表头返回 `None`（走组件默认 26px，
  默认视觉一个像素都不动）。40px 的依据：`DataTable::header_row_height` 给的是整条表头带
  （`header_band_height` = 该值 × 列组行数），`render_cell` 再吃上下 padding 4px → 内容高
  36 = 两行各 18px（`text_xs` 行高），刚好装下。
- `grid/status.rs` 渲染处：`DataTable::new(state)…when_some(state.read(cx).delegate()
  .header_row_height(), |t, h| t.header_row_height(h))`——值来自 delegate，不散在视图里。
- badge 字号落地：`render_th` 的 badge 从 `text_xs` 改为
  `text_size(rems(RESULT_HEADER_TYPE_FONT_SIZE))`（11px，比列名小一档）；
  `grid/fit.rs` 量 badge 符号宽同步改用 11px（**两边字号必须一致**，否则双击自适应算出的
  宽度会与实际渲染差一两个字符——这是「测宽口径」第二次踩的同一个坑）。
- `comment_line` 加 `debug_selector("editor-result-th-comment-{col_ix}")` 供用例读几何。

**验证**：`cargo test -p rds-result-panel --lib` = **225 passed / 0 failed**（+1
`comment_header_row_gets_room_for_two_lines`：① 纯逻辑两态——单行 `None`、注释开启
`Some(40px)`；② 真 `TableState` + `DataTable` 渲染两帧后 `debug_bounds` 读到主行高
≥ 14px、注释行高 ≥ 12px、注释行在主行**下方**且不重叠。未抬表头时注释行取不到 bounds，
断言必挂）；`cargo test -p rds-ui-guard` = 4 passed / 0 failed（`ui.rs` 裸 px 基线
16 → 17）；`cargo check -p rds-result-panel` 0 warning。

**未验证**：真窗口下 40px 表头的观感（比 26px 的行高突兀与否，只有眼睛能判）；11px badge
在真实字体下的可读性；开启注释后表头变高会不会挤压可视行数到"少一行"的体感（组件按
`header_band_height` 重算 `page_item_count`，逻辑上正确，观感未目视）。

### 8.28 窄档列宽补齐 + 工具栏高 40px + 筛选框同档（2026-11-08）

**问题一：布尔列 96px 读不出列名**。96 是按「布尔值本身很短」给的，但列宽真正要装的是
**表头**。算术（`vendor/gpui-component/src/sizing.rs` XSmall 档 + `grid/header.rs` 构成）：

```
96 − 8（组件左右 padding）           = 88
    − 19（badge：11px 符号 + px_1 8）
    −  8（badge/列名/菜单之间两个 gap_1）
    − 24（列头菜单）
    = 37px 给列名   →  5–6 个西文字符（`is_active` 渲染成 `is_a…`）
```

改 `RESULT_COLUMN_WIDTH_BOOLEAN` **96 → 120**（列名留 61px ≈ 9 字符）。其余档实测都够
（numeric 130 留 79、date/time 120 留 69、timestamp / uuid / json / binary / text 更宽）。

**问题二：最小列宽 64px 时列名只剩 5px**。64 时 badge(19) + 菜单(24) + 两个 gap(8) = 51
已经吃掉可用宽（56）的一大半。最小宽的职责是**兜住不可用态**、不是保证可读，所以取
**64 → 80**（列名留 21px ≈ 3 字符 + 省略号，看全名靠悬停 tooltip）。

这条有一个可观察的行为变化：`Column::min_width` 会把用户拖到的更小值兜底上去。既有用例
`default_column_width_follows_type_until_the_user_drags` 里「拖到 70」那一步，原先等于
用户值、现在等于 80——用例改成显式断言这条兜底语义（故意保留 70 来钉住它）。

**问题三：工具栏 36px 上下零余量**。36 = `Toolbar` XSmall 自身最小高（`min_h_7` 28 +
`p_1` 8），容器高正好等于内容高；同时筛选框 `Input` 还停在组件默认 **Medium 档**
（`h_8` = 32px，见 `vendor/gpui-component/src/sizing.rs:281` 的 `input_h`），在 36px 里
只剩上下各 2px，视觉上直接顶满上下边框——这才是「工具栏显得挤/矮」的实测原因，不是容器
本身给得不够。

- `RESULT_TOOLBAR_HEIGHT` **2.25 → 2.5**（36 → 40px）；
- 常态筛选框与展开态筛选条的 `Input` 都加 `.small()`（32 → 24px，与同排 `Toolbar`
  XSmall 的 24px 按钮同档；字号随之 12 → 14px，输入更好读）；
- 40px 相比 36px 多占 4px：结果区默认高 `RESULT_PANE_HEIGHT` = 20rem（320px），不影响
  网格可容纳行数（8rem/10rem 时代的「编辑行被虚拟列表裁剪丢 Input 焦点链」教训仍然有效，
  只是当前余量足够；`grid/status.rs` 那条过时注释已按实际值改写）。

**验证**：`cargo test -p rds-result-panel --lib` = **226 passed / 0 failed**（+1
`narrow_column_tiers_keep_the_header_readable`：把「表头固定占宽 59」与「列名可读下限 56」
钉成算术护栏，布尔档或最小宽改回去立刻挂）；`cargo test -p rds-ui-guard` = 4 passed
（常量值变化不增删 `px(N.)`，基线仍 17）；`cargo check -p rds-result-panel` 与
`cargo check -p rds-editor` 均 0 warning。

**未验证**：真窗口下 40px 工具栏的观感（相对 26px 表头 / 24px 状态栏的节奏）、24px
筛选框的可点性、120px 布尔列在多列并排时的总宽影响（20 个布尔列从 1920 → 2400）。

### 8.29 列头菜单改为绝对定位 + 悬停显形，不再占列宽（2026-11-08）

**问题（几何实证）**：菜单从 §8.26 起并进主行流内当 `flex_none` 子元素，实测占 24px，
于是它反过来成了窄列列名被挤没的主因：布尔列 96px 时列名只剩
`96 − 8（组件 padding） − 19（badge） − 8（两个 gap） − 24（菜单） = 37px`。
而它其实是「想起来了才点」的控件，平时一直占着位置。

**改法**：

- 菜单 guard 从流内改成 `absolute().top_0().right_0()`，贴在主行**右端之上**（不参与流，
  也不改列宽）；
- 默认 `invisible()`，整格 hover 才显形：主行挂 `.group("editor-result-th-hover-{col_ix}")`，
  guard 挂 `.group_hover(同名组, |this| this.visible())`——先例是 vendor
  `gpui-component/src/attachment.rs` 的悬停删除按钮（`.group(...)` + `.absolute().top_0()
  .right_0().invisible().group_hover(...)`）；
- 主行加 `.relative()` 当定位父；列名加 `debug_selector("editor-result-th-name-{col_ix}")`
  供用例量「列名拿到多少宽度」；
- `grid/fit.rs` 撤销菜单占宽（`RESULT_FIT_COLUMN_MENU` 删除），`crate::ui` 少一个裸 px
  （`ui_guard` 基线 17 → 16）。

**几何实测（headless，两帧后读 `debug_bounds`）**：`grid()` 未知类型列、默认列宽 128，

```
列宽 128 − 组件左右 padding 8 − 组件自带排序图标槽 16（`render_th` 的 justify_between 右端）
        = 列名 104px（菜单在流内的旧实现只剩 80）
```

**取舍**：hover 时菜单压住列名最右 24px（列名全名本来就有悬停 tooltip 兜底）；
菜单高 24px > 表头内容高 22px，底部 2px 会被组件 `overflow_hidden` 裁掉（图标垂直居中，
不影响可读）。**不采用**「hover 时才占宽（`w(0)` → `w(24)`）」：那会让列名在 hover 瞬间
被横向挤走，视觉跳动比遮挡更显眼。

**验证**：`cargo test -p rds-result-panel --lib` = **227 passed / 0 failed**（+1
`header_column_menu_floats_and_leaves_the_name_its_full_width`：列名宽 ≥ 96px 且与主行宽差
≤ 8px——旧实现在流内占 24px，两条都挂）；`cargo test -p rds-ui-guard` = 4 passed；
`cargo check -p rds-result-panel` 0 warning。

**未验证**：真窗口下 hover 出菜单的手感（mouse-out 是否立刻消失、tooltip 与菜单是否打架）、
菜单在 22px 内容高里被裁 2px 后的图标观感；触屏（无 hover）下入口靠**列头右键菜单**
（列级项与这里同源 `column_menu_items`），未目视确认。

### 8.30 结果集标签条抬到编辑器同档（Small 24px → Medium 32px，2026-11-08）

**问题（用户原话）**：「结果集的标题不是要跟编辑器的文件表一致吗，为什么感觉目前结果集的标题还感觉高度低」。

**取证（全在 vendor 权威源码里，不是推断）**：

| | 编辑器文件标签条 | 结果集标签条（改前） |
| --- | --- | --- |
| 组件 | dock `TabBar::new("tab-bar")`（`dock/tab_panel.rs:486-489`） | `TabBar::new("editor-result-sets")`（`sets.rs:177`） |
| size 档 | **不写** → `Size::default()` = `Medium`（`sizing.rs:6-13` 的 `#[default]`） | `Size::Small` |
| variant | `shared.tab_bar_variant()`，默认 `Tab` | 默认 `Tab`（一致） |
| Tab 档高 | Tab+Medium = **32px**（`tab/tab.rs:79-82`） | Tab+Small = **24px**（`tab.rs:71-74`） |
| 容器外层高 | 无（TabBar 自己撑） | `.h(rems(1.5))` = **24px**，与组件高**相等、零余量** |
| tab 间 gap | Medium 12px（`tab_bar.rs:380-384`） | Small 8px |
| 左右内边距 | Medium 12px（`tab.rs:113-119`） | Small 10px |
| tab 内高 | Medium 30px（`tab.rs:103-108`） | Small 22px |
| 字号 | Small/Medium 都是 `text_sm`（14px，`tab.rs:975-979`） | 同（**唯一没差别的一项**） |

所以不是"结果集标签条容器矮了一点"，而是**整整差一个档位**：24 vs 32 = 8px，且容器把 24 卡死、
连半像素余量都没有。用户肉眼看出"低一截"成立。

**改法**：

- `sets.rs` 的 `TabBar` 档位 `Size::Small` → **`Size::Medium`**，与 dock 的 `tab-bar` 同档，
  高度 / gap / 内边距 / tab 内高四样一起对齐（字号本来就一样）；
- `ui::RESULT_TABS_HEIGHT` `1.5`（24px）→ **`2.0`**（32px），容器高 == 组件高，保持"照档位
  写常量"的既有口径。

**顺带查清的一件事**：dock 的**单面板标题条**（`render_title`，只有一个文件时走这条）
是 `.h(px(30.))`（`dock/tab_panel.rs:386-388`）。也就是编辑器在单文件时 30px、多文件 tab 时
32px，两者不完全一致。结果集标签条恒为多标签形态，因此对齐 **32px** 的那条。

**验证**：`cargo test -p rds-result-panel --lib` = **228 passed / 0 failed**（+1
`sets::tests::tab_strip_matches_the_editor_dock_tab_height`：真实渲染标签条后读
`debug_bounds("editor-result-tabs")`，断言高 == 32px——`TabVariant::height` 在 vendor 里是
私有函数，只能靠真渲染量；旧实现读到 24 必挂）；`cargo test -p rds-ui-guard` = 4 passed；
`cargo check -p rds-result-panel` / `-p rds-editor` 均 0 warning。

**未验证**：真窗口观感（32px 后标签条占掉网格 8px 高度是否显紧）、tab 内右侧「⋯」与钉住
小按钮在 Medium 档 12px 内边距下的排布、Medium 的 hover/选中底色在结果集这张标签条上的观感。

### 8.31 结果集标签条按 DBeaver 补齐四项（图标 / 关闭 / tooltip / 拖拽重排，2026-11-08）

**指令（用户原话）**：「请你完整对齐 dbeaver 的设计」。此前我先只做了图标，用户明确要求补全
其余三项，故本轮四项一起落地。逐项取证 DBeaver 源码（本地竞品仓 `D:\RdataStation\RDS\dbeaver-ref`），
再对照实现：

| # | DBeaver 的做法（出处） | 本仓实现 | 差异 / 取舍 |
| --- | --- | --- | --- |
| ① tab 图标 | `MultiTabsQueryResultsContainer.java:68-77` 的 `tabItem.setImage(SQLEditor.IMG_DATA_GRID)`——每个结果集标签都带 DataGrid 图标 | `sets.rs` 的 `RESULT_SET_ICON = "icons/table.svg"`，经 `Tab::prefix()` 挂 `Icon::default().path(..)` | 不用 `Tab::icon()`：vendor `tab.rs:871` 有 icon 时 `max_width` 失效、`:888-896` 走 `w(inner_height*1.25)` 定宽且**不挂 children**，会把标题徽标与按钮全挤掉。图标走**资产路径**而非 `IconName`：`default-icons.txt` 106 个里没有任何 table/grid，而应用注册的 `gpui_kit::assets::AllAssets` 有全量 1818 个 Lucide |
| ② 关闭按钮 | 同 `:68-77` 的 `tabItem.setShowClose(true)` | `TabActions::on_close` → suffix 末尾 `Button::ghost().small().icon(IconName::Close).tooltip("关闭这个结果集")`；`ResultRequest::CloseSet` → `store.remove_set` | **语义有意偏离**：DBeaver 关掉标签会留一个空白页，我们是从 store 移除槽位（条目 + 变更集/历史/本地行/行级历史/事实档一并丢弃），且**只剩一份时拒删**——下游全部按「至少一份」装配。判据写在 `remove_set` 注释里。钉住不阻止关闭（钉住挡的是自动淘汰） |
| ③ tooltip 显示原始 SQL | `MultiTabsQueryResultsContainer.java:95-103` 的 `tabItem.setToolTipText(toolTip)`（`updateResultsName` 里挂原始 SQL） | `ResultSetTab.sql` 投影（`tabs()` 里 `entry.sql.clone()`）→ `Tab::tooltip(SharedString)`。空 SQL 不挂（避免一个空白气泡） | vendor 两层原本**都没有 tooltip 入口**，本轮在 `gpui-component/src/tab/tab.rs` 补字段 `tooltip: ComponentTooltip` + `pub fn tooltip(text)`，链尾 `self.tooltip.apply(tab_element)`——复用上游 `Button`/`Switch` 同一套（自带入场动效、延迟、减弱动效适配），未设置时原样返回、零开销 |
| ④ 拖拽重排 | `SQLEditor.java:1518` `new TabFolderReorder(resultTabs)`；`:1541-1549` 监听 `ITEM_MOVE_EVENT` 更新引用；`TabFolderReorder.java:35-45` 事件带 `oldIndex`/`newIndex` | 三层：`store.move_set(from, to)`（整份槽位 `Vec::remove`+`insert`，`active`/`compare` 都经 `reorder_index` 重映射）→ `ResultRequest::MoveSet{from,to}` → `TabBar::on_reorder`。vendor 侧 `gpui-base` 的 `TabReorder`（载荷 = 自己的下标）+ `TabDragGhost`（跟着指针的小胶囊）+ `Tab::on_reorder`；`gpui-component` 的 `TabBar::on_reorder` 每帧把回调下发到每个标签 | gpui 的 `on_drag` **强制**要一个 `Entity<W: Render>` 作拖拽预览（没有「不画预览」选项），所以 base 层新增了 `TabDragGhost` 视图。它在 `gpui-base` 里**拿不到主题色**（gpui-component 在它之上），只能用中性半透明白 + 细边框——这是仓库「零裸色值」红线的一处**显式例外**，理由写在结构体注释里。拖拽能力归属「整条标签条」（与 DBeaver 把 `TabFolderReorder` 装在 CTabFolder 上一致），不是单个标签。**未做的差异项**：DBeaver 拖动时在落点画插入标记（`TabFolderReorder` 的 DnD 反馈），我们只有「指针跟手的小胶囊」，落点目标靠 swap 语义本身表达，没有额外的插入位置指示——这是本轮唯一明确没做的 DBeaver 特征，原因是落点标记要重写 TabBar 的绝对定位层，代价远高于收益 |

**store 侧不变量**：`move_set` 越界 / 回原位 / 文档不存在一律返回 `false` 且不动；
`active` 与 `compare` 同经 `reorder_index(index, from, to)`，因此 `compare ≠ active` 保持。

**验证**：

- `cargo test -p rds-result-panel --lib` = **240 passed / 0 failed**（改前 235；+4 条拖拽
  `dragging_a_set_forward_moves_it_and_keeps_selection_on_it` /
  `dragging_a_set_backward_shifts_the_neighbours` /
  `reordering_remaps_the_compare_index` /
  `reordering_ignores_noops_and_out_of_range_targets`，
  sets 侧 `tabs_keep_the_source_sql_for_the_tooltip` 等 5 条已在上一步落地）。
- `cargo check -p rds-result-panel` / `-p rds-editor` 均 Finished、**0 warning**；
  `cargo test -p rds-ui-guard` = 4 passed。
- 两条 `debug_bounds` 断言走静态字符串字面量：`gpui-fast-0.1.13/src/app/test_context.rs:1032`
  的签名是 `debug_bounds(&mut self, selector: &'static str)`，`format!` 出来的 `String`
  传不进（E0308 实测），故关闭按钮/图标前缀的选择器用字面量数组遍历。

**未验证（需真窗口）**：图标 + 通道徽标 + 「⋯」+ 关闭（钉住时再加图钉）全塞进 32px tab 是否挤；
`ComponentTooltip` 在标签上的实际延迟与浮层位置；拖拽手势的完整观感（拖起阈值、落点高亮、
`TabDragGhost` 在深色主题下的反差）——headless 测不了鼠标拖拽，`on_drag`/`on_drop` 只有编译级
保证与 store 层单测。

**副作用（承接 §8.30，仍未处理）**：`RESULT_PANE_HEIGHT` 仍是 `20.0`（320px），标签条从 24 抬到
32 后网格少了 8px（26px 行高下约少 1/3 行），是否抬到 340 待用户反馈。

### 8.32 结果集工具栏按 DBeaver 收形（纯图标 + 图标 toggle + 竖分隔，2026-11-08）

**指令（用户原话）**：「工具栏也可以对齐，我已经目测过了，标题的高度基本没有问题了」——
即标签条 32px（§8.30）已验收，本轮只收工具栏形态，**不动高度**。两个形态细节由用户确认
「按照你的建议继续即可」：右段按钮走**纯图标 + tooltip**，两个开关走**图标 toggle**。

**DBeaver 取证**（本地竞品仓 `D:\RdataStation\RDS\dbeaver-ref`）：结果集那条工具栏的真身是
`plugins\org.jkiss.dbeaver.ui.editors.data\src\org\jkiss\dbeaver\ui\controls\resultset\ResultSetFilterPanel.java`：

- `:171` `filterComposite`（占宽）+ `:192` 多行 `TextViewer`：左段就是**一个筛选文本框**，占满宽。
- `:315` `filterToolbar = new ToolBar(this, SWT.HORIZONTAL|SWT.RIGHT)`，`:316`
  `GridData.HORIZONTAL_ALIGN_BEGINNING | VERTICAL_ALIGN_CENTER`：右段是紧贴文本框的**纯图标**
  ToolBar，只有 4 个 ToolItem——`:319` 清除筛选（`SWT.NO_FOCUS|SWT.DROP_DOWN`）、`:330` 自定义筛选
  （同）、`:343` 筛选历史后退▾、`:348` 前进▾，`:356` 查找替换（`SWT.CHECK|SWT.NO_FOCUS`，
  **按下态 toggle**）。分组靠 `:354` 的 `UIUtils.createToolBarSeparator`（`:340` 还有一处被注释掉）。
- 整条**没有一个带文字标签的按钮，也没有 switch**；提交/导出/刷新这类业务动作不在这一行
  （走右键菜单 / 状态栏）。

**对照与落地**：

| 维度 | DBeaver | 本仓改前 | 本轮改后 |
| --- | --- | --- | --- |
| 右段按钮形态 | 纯图标 + tooltip（`ToolItem` 自带） | `Button`：对比/分析/复制部分带图标，`提交变更 (N)` / `撤销变更` / `复制` / `⚗ 分析` 带文字 | 全部**纯图标 + tooltip**：对比 `Columns2`、提交 `Save`（计数进 tooltip「提交 N 项变更到源库」）、撤销 `RotateCcw`、分析 `Sigma`、复制 `Copy` |
| 二元开关 | CHECK `ToolItem`（图标 + 按下态） | `Switch`「下发源库」「注释」各占一整块（轨道 + 文字） | **图标 toggle** `Button`：`.ghost().small().toggled(..).icon(..).tooltip(..)`（下发源库 `DatabaseZap`、注释 `FileText`）。点击回调里自己翻转（`Button::on_click` 不给新值，闭包捕获当前 bool 取反）；`SetPushdown` / `SetShowColumnComments` 的请求载荷不变 |
| 分组 | `UIUtils.createToolBarSeparator` 竖线 | 只有 `gap` | 开关组与动作组之间插一条 1px 竖线：`w(ui::HAIRLINE).h(rems(1.5)).bg(theme.colors.border)`，**两组都非空才插**（避免空段留孤线）。不用 `separator::Separator::vertical()`：它渲染成 `absolute().h_full()`（`vendor/gpui-component/src/separator.rs:80-84`），塞进 `Toolbar::contents` 的流式槽位会定位到外层祖先，故按同一语义自画 1px 线 |
| 高度 | SWT ToolBar 天然紧凑（约 24px） | 40px（`ui::RESULT_TOOLBAR_HEIGHT`，§8.28 定的） | **当时不动**——用户已验收当前高度；收形靠「去文字 + 去开关轨道 + 加竖线」，压缩高度会碰 §8.28 记录的「编辑行被虚拟列表裁剪丢 Input 焦点链」风险。**该判断已被 §8.34 推翻**：40px 里有 8px 是 `Input` 降档后没回收的空白，已收回 32px（控件未动） |

**范围不变（只改形态，不改任何功能与摆放条件）**：SQL 预览仍在最左（保留 `SQL: <前 60 字>` 文字，
它是「一眼看查询」的信息位，不是按钮标签）；筛选框 / 展开态不变；对比仍只在 `tabs.len() >= 2` 摆；
提交+撤销对仍只在 `pending_changes > 0` 摆、提交中仍置灰；分析仍只在 `analysis_items` 非空摆。
`debug_selector` 全部原样保留（`editor-result-pushdown` / `editor-result-show-comments` 是**新加**的，
原先 `Switch` 侧没有）。

**验证**：`cargo test -p rds-result-panel --lib` = **240 passed / 0 failed**（与改前一致，测试集合没动）；
`cargo check -p rds-result-panel` Finished、0 warning；`cargo test -p rds-ui-guard` = 4 passed
（未新增 `ui.rs` 常量，BASELINE 不动）。

`cargo test -p rds-editor --lib` = 764 passed / **3 failed** / 1 ignored。其中
`view::tests::results_a::the_result_header_sorts_from_its_arrow`（`editor-result-th-1 应登记坐标`）
经 A/B 归因**不是本轮引入**：把本轮两个文件 `git stash` 后单跑同样 FAILED。另两条
（`legacy::opening_an_analysis_document_builds_a_cell_editor_per_cell`、
`notebook_win::analysis_cell_renders_multiple_output_blocks`）是升级报告 §7 登记的基线失败。
工作树当时另有并行线未完成改动（`crates/editor/src/ports/editor_exec/` 迁移中），故不在本轮修。

**未验证（需真窗口）**：纯图标状态下右段整体观感（六个图标 + 一条竖线挤在 40px 行里是否过密）；
`IconName` 语义是否一眼可读（`Save` 表「提交」、`Sigma` 表「分析」、`DatabaseZap` 表「下发源库」
都需要靠 tooltip 兜底）；`ComponentTooltip` 在这排小图标上的延迟/位置。

### 8.33 结果集配色槽（对齐 DBeaver 结果集专属主题色，2026-11-08）

**指令**：第四阶段由用户在交互选项里选定「结果集配色槽」。该选项的口径写的是「**全局生效**——
所有表格（Mock 预览、设置页等）的选中/斑马纹都会跟着变」，用户接受该副作用。**实际落地的路径与
选项描述有一个差异**：四个槽走的是**产品语义 token**（`workbench_shell::product_tokens`），只在
结果集网格内消费，Mock 预览/设置页等其它表格**不受影响**。差异原因见下「为什么不动 vendor」一节。

**DBeaver 取证**（`D:\RdataStation\RDS\dbeaver-ref`，逐行实读）：权威源是
`plugins\org.jkiss.dbeaver.ui.editors.data\plugin.xml:1144-1263` 的 `<colorDefinition>` 段
（`categoryId="org.jkiss.dbeaver.ui.presentation.results"`）：

| id（省略前缀 `org.jkiss.dbeaver.sql.resultset.color.`） | 默认值 | 行号 |
| --- | --- | --- |
| `selection.background` | 175, 215, 255 | :1156 |
| `cell.odd.background` | 235, 245, 255 | :1170 |
| `cell.modified.background` | 255, 228, 181 | :1184 |
| `cell.error.background` | 255, 128, 255 | :1191 |
| `cell.new.background` | 228, 255, 181 | :1198 |
| `cell.deleted.background` / `cell.readonly.background` | 255, 99, 71 | :1205 / :1212 |
| `cell.matched.background` | 255, 215, 0 | :1219 |
| `lines.normal` / `lines.selected` | 240,240,240 / 105,105,105 | :1226 / :1233 |

消费侧对应 `plugins\org.jkiss.dbeaver.ui.editors.data\src\org\jkiss\dbeaver\ui\controls\resultset\
ResultSetThemeSettings.java`（`:38-84` 22 个 `@ThemeColor` 字段，斑马纹消费点
`SpreadsheetPresentation.java:2881`、`PlainTextPresentation.java:154`）。
**深色主题这一批没有覆盖**：`css\e4-dark-data-editor.css` 全文 grep `resultset|result.` 命中 0，
`e4-data-editor.css`（浅色）只有 `header.background = 251,251,251` 一条。

**为什么不动 vendor**：改 `ThemeColor`/`ThemeTokens` 加结果集字段要动 `vendor/gpui-component`
的主题 crate（全局 141 个字段、双 vendor patch），收益只有「所有表格一起变」——而这恰好不是我们要
的结果（DBeaver 的这批色就是**结果集专属**）。本仓早有 `workbench_shell::product_tokens` 这个
产品角色设施（`result.local_row_background` 同例），沿用即可，vendor 一行不动。

**消费通道（本 crate 唯一一处 delegate 原生绘制行的正当理由）**：`DataTable`/`TableState` 没有任何
按行/按格着色的公开 API（`vendor/gpui-component/src/table/delegate.rs` 全文 269 行只有
`render_th` / `render_td` / `render_tr` 等渲染钩子，无背景注入）。但 `state.rs` 的渲染顺序给了入口：

- `:2257-2258` `let mut tr = self.delegate.render_tr(...); let style = tr.style().clone();`
  —— 先把 delegate 返回的**样式快照**存下来；
- `:2274-2278` 组件自己再挂行底线与 `.when(is_stripe_row, |this| this.bg(tokens.table_even))`
  （条纹取的是**全局** `table_even`）；
- `:2278` `.refine_style(&style)` 把快照重放一遍。

`refine` 的语义经源码核实：`vendor/gpui-base/src/styled.rs:79-82` 是
`self.style().refine(style)`，落到 `gpui-pre` 的 `derive_refineable` 生成代码
（`derive_refineable.rs:158-176`）—— **`Option` 字段为 `Some` 就覆盖当前值**。所以 delegate 在
`render_tr` 里铺的 `bg` 会盖过组件的 `table_even`，而 `.hover(table_hover)` 在 refine 之后，
悬停仍归组件（符合预期）。

**落地四件**：

| # | 位置 | 改动 |
| --- | --- | --- |
| 1 | `crates/workbench_shell/src/product_tokens.rs` | 新增 3 个角色 `result.odd_row.background` / `result.cell.modified.background` / `result.cell.error.background`（`TokenMap` + `ProductTokens` + `from_map`），三个访问器全部带 `unwrap_or_else` 回退（缺失 → 组件同款 `tokens.table_even` / 旧口径 `warning`·`danger` 16%），守「资产不全也不 panic」 |
| 2 | `assets/themes/product-tokens.json` | 明暗各加三槽；`result.local_row_background` 由 `#F7E9E5`/`#382620` 换成 DBeaver `cell.new.background` 的 `#E4FFB5`/`#2A3A1E` |
| 3 | `crates/result_panel/src/grid/cell.rs` | 脏格 `warning.opacity(0.16)` → `result_cell_modified_background`、冲突格 `danger.opacity(0.16)` → `result_cell_error_background`（两者在闭包外先求值，避免 `cx` 借用打架） |
| 4 | `crates/result_panel/src/grid/table_impl.rs` | 新增 `TableDelegate::render_tr`：奇数行 `bg(result_odd_row_background(theme))`，偶数行保持裸 `div()`（观感不动） |

**行为变化（需用户知情）**：`result.local_row_background` 从品牌珊瑚浅底（`#F7E9E5`/`#382620`）
换成 DBeaver 新增行的浅绿（`#E4FFB5`/`#2A3A1E`）。这是本地新增/复制行的底色，用户在之前的
交互选项里**没有被告知**这个变化，已作为待确认项登记。

**深色一套是推导值，不是照抄**：DBeaver 该批 colorDefinition 只有一套浅色值、深色 css 零覆盖，
故 `#1C2129`（斑马）/ `#3A3120`（脏格）/ `#4A2131`（错误格）/ `#2A3A1E`（本地行）是按深色背景
等亮度比推导，`product-tokens.json` 的 `$comment` 已写明「非照抄」。待用户确认。

**验证**：`cargo check -p rds-result-panel` Finished、**0 warning**（首轮踩 E0308：
`theme.tokens.table_even` 是 `ThemeToken` 不是 `Hsla`，改 `.unwrap_or_else(|| ....into())`）；
`cargo test -p rds-workbench-shell --lib` = **14 passed**（含新增
`product_tokens::tests::parses_result_color_slots_and_falls_back`）；
`cargo test -p rds-result-panel --lib` = **241 passed / 0 failed**（含新增
`grid::tests::odd_rows_paint_the_stripe_from_the_delegate_row_container`）；
`cargo test -p rds-ui-guard` = **4 passed**（`PRODUCT_ROLES` 白名单补 3 个键，否则
`product_tokens_use_declared_role_names` 会挂——键名写错时 token 会被静默丢弃，这道门禁正是防这个）。

新增渲染用例只锁「奇数行分支真的跑了」（行容器挂 `debug_selector("editor-result-row-odd")`，
取不到就说明 `render_tr` 没被调用、配色槽空转）；**颜色本身 headless 读不到**，颜色正确性由
`refine` 覆盖语义的源码证据 + 真窗口目视共同担保。

**本轮未接的 DBeaver 槽（如实登记）**：`selection.background`/`foreground`（选中格仍走组件
`table_active`）；`cell.matched.background`（本仓无筛选命中高亮功能）；`lines.normal`/`lines.selected`
（每格网格线——`DataTable.bordered(true)` 只加最外圈整框，不提供内部网格线，要做需自绘列分隔）；
`header.*` 四槽（表头自画但取 vendor token）；`preview.background`；
`cell.deleted.background`/`cell.readonly.background`（删除行现用灰字 + 删除线，不铺红底）；
`null.foreground` 与 `dtNumeric`/`dtString` 等数据类型色。

**未验证（需真窗口）**：奇数行 `#EBF5FF` 在深色行底线（`table_row_border`）间的观感是否过浅；
脏格 `#FFE4B5` 与本地行 `#E4FFB5` 同时出现时是否会糊成一片（两色都淡、相邻列可能并排）；
冲突格 `#FF80FF` 是全饱和洋红，实色铺底是否刺眼（DBeaver 原值就是这个，未做降饱和）。

### 8.34 结果集工具栏背景与高度对齐 DBeaver（2026-11-08）

用户实拍反馈：「结果集的工具栏似乎没有对齐 dbeaver，如背景、高度等等」。**这是一条推翻
§8.32 前提的反馈**——§8.32 当时把「高度 40px 不动」写进文档，理由是「用户已验收」，实际
验收的只是按钮形态。

**取证（我们侧，`grid/status.rs::render_toolbar`）**：整条工具栏**没有任何 `.bg()`**，背景靠
父容器透传；高度写死 `.h(rems(ui::RESULT_TOOLBAR_HEIGHT))` = **40px**；只有一条 1px 下边框。

**取证（DBeaver 侧，均已实读源码）**：

| 事实 | 位置 |
| --- | --- |
| 面板 `GridLayout(2, false)`，`marginHeight = 3`、`marginWidth = 3` | `ResultSetFilterPanel.java:163-166` |
| 左列 `filterComposite` 挂 `new CompositeBorderPainter(...)` | `ResultSetFilterPanel.java:182` |
| `CompositeBorderPainter` = 沿控件外框 `drawRectangle(0, 0, w-1, h-1)`**一整圈 1px**，色 = `UIUtils.getShadowColor()` | `CompositeBorderPainter.java:34-39` |
| 框色随明暗换档：浅色 `COLOR_WIDGET_LIGHT_SHADOW`；深色 Windows `COLOR_WIDGET_DARK_SHADOW` / macOS `COLOR_WIDGET_NORMAL_SHADOW` | `UIUtils.java:212-216` |
| 右列 `new ToolBar(this, SWT.HORIZONTAL \| SWT.RIGHT)` + `VERTICAL_ALIGN_CENTER`，**没有固定像素高** | `ResultSetFilterPanel.java:315-316` |
| 分隔符是**图片资产**（`UIIcon.SEPARATOR_V` 包 `Label` 再包 `SWT.SEPARATOR`），不是像素线 | `UIUtils.java:218-222` |
| 面板背景 = 主题 `INACTIVE_TAB_BG_START`，缺失回退系统 `COLOR_WIDGET_BACKGROUND` | `UIStyles.java:122-129` |
| 文本/网格背景 = `COLOR_LIST_BACKGROUND`（白） | `UIStyles.java:131-133` |

也就是说 DBeaver 的层次是：**标签条与筛选条同属「非活动标签 / widget」背景档（灰），网格是
列表底（白）**。插件级 css 是空壳（`e4-dbeaver-widgets.css` 49 B、`e4-dark-dbeaver-widgets.css`
49 B、`e4-dbeaver.css` 55 B），所以这不是某条 css 能照抄的值，而是平台色语义。

**根因（高度）**：40px 的来历是「筛选框 `Input` 还是默认 Medium 档 `h_8`(32px) + `Toolbar`
XSmall 的 `p_1` 上下 8px」。`Input` 在 §8.32 前后已降到 `.small()`（24px），**那 8px 没回收**，
工具栏于是比标签条虚胖一圈。

同时纠正两处历史算术错误（原文写「Toolbar XSmall = `min_h_7`(28px) + `p_1`(8px) = 36px」）：
- `min_h_7` 是**最小总高**约束，不是内容高（vendor `toolbar.rs:253`）；
- 右段所有控件（`Button` / `DropdownButton` 的 `.small()`）真实高 **24px**（vendor
  `button.rs:688-700`：纯图标 `size_6`、带文字 `h_6`），加 `p_1` 上下 8px → **自然高 32px**。

**落地两行**：

| # | 位置 | 改动 |
| --- | --- | --- |
| 1 | `crates/result_panel/src/ui.rs` | `RESULT_TOOLBAR_HEIGHT` `2.5`(40px) → **`2.0`(32px)**，与 `RESULT_TABS_HEIGHT` 同值；注释重写并纠正上述算术 |
| 2 | `crates/result_panel/src/grid/status.rs` | 容器加 `.bg(theme.colors.tab_bar)` —— 本仓对应「非活动标签 / widget 背景」的角色就是 `tab_bar.background`（`assets/themes/rds-theme.json`：浅 `#F8F8F8` / 深 `#181818`），与标签条（`TabVariant::Tab` 取 `tokens.tab_bar`，vendor `tab_bar.rs:403-406`）连成一条 chrome 带 |

**为什么不是 28px 也不是其它值**：`Toolbar` XSmall 的 `min_h_7` 是 28px 下限，但在 24px 控件
+ `p_1` 的前提下自然高就是 32px；32 又是标签条档位，两块等高最整齐。再往下压必须脱离
`Toolbar` 组件（违反「组件选型禁手搓」红线），不做。

**行为变化**：网格可用高度 **+8px**（工具栏矮了，结果区 `RESULT_PANE_HEIGHT` 不动）。控件一个
像素没改，`Input` 仍是 `.small()`，所以不存在 §8.28 记的「编辑行被虚拟列表裁剪丢 Input 焦点链」
风险。

**验证**：`cargo check -p rds-result-panel` Finished、**0 warning**；
`cargo test -p rds-workbench-shell --lib` = **14 passed**；
`cargo test -p rds-result-panel --lib` = **242 passed / 0 failed**（新增
`grid::tests::the_result_toolbar_keeps_the_tabs_height`，真实布局量出 32px，锁住「有人把它改回 40」
和「两个常量各自改档」两种情况）；
`cargo test -p rds-ui-guard` = **4 passed**。

**本轮未做（如实登记）**：

- **筛选区整圈 1px 边框**（`CompositeBorderPainter`）：我们筛选框是 `Input` 组件自带边框，语义
  等价但不铺满整条；要等价需自绘一个包住「预览 + 筛选」的容器，与「组件选型禁手搓」冲突，未做。
- **面板四周 3px 外边距**：我们 `px_2`(8px) 水平内距、无垂直内距；DBeaver 是 3px 四周。差 5px，
  未调（改了要连带量标签条对齐）。
- **图片分隔符**：仍用自画 1px `div`（1.5rem = 24px 高）。DBeaver 那条是图片资产
  （`UIIcon.SEPARATOR_V`），我们资产包里没有同名图，未替换。
- **筛选条高度 28-32px 的精确对齐**：DBeaver 由内容自然决定（无常量），我们受组件最小档约束取
  32px，理论上有 0-4px 残差；**未在真 DBeaver 里量过实际像素**（本仓没有可运行的 DBeaver 实例）。

### 8.35 结果集工具栏第二轮对齐（筛选区外框 / 3px 内距 / 吃满宽 / 右段去内距，2026-11-08）

§8.34 交付后用户确认「先按 A」（底色保持 `tab_bar.background` = `#F8F8F8`，不做整体压暗），
同时要求「工具栏的调整对齐要赶紧对齐」——颜色选择先冻结，结构对齐继续推。

**本轮五项（前四项合成一个「筛选区控件组」，第五项是右段边缘）**：

| # | 项 | DBeaver 依据 | 改前 | 改后 |
| --- | --- | --- | --- | --- |
| 1 | 工具栏水平内距 | `GridLayout.marginWidth = 3`（`ResultSetFilterPanel.java:165`） | `px_2`(8px) | `RESULT_TOOLBAR_PADDING` = `px(3.)`（`ui.rs` 新增常量） |
| 2 | 筛选区**一整圈** 1px 框 | `filterComposite` 挂 `CompositeBorderPainter`（`:182`）→ `drawRectangle(0, 0, w-1, h-1)`（`CompositeBorderPainter.java:34-39`） | 无外框，只有 `Input` 自带边框 | 容器 `border_1 + border_color(colors.input) + rounded(theme.radius)`；`Input` 改 `.appearance(false)` 让出边框 |
| 3 | 筛选区底 | `getDefaultTextBackground()` → `COLOR_LIST_BACKGROUND`（`UIStyles.java:131-133`） | 透明（直接吃工具栏 widget 灰） | `colors.background`（面板白）压在 `tab_bar` 灰底上 |
| 4 | 宽度分配 | 左列 `GridData(FILL_HORIZONTAL)`（`:180`）、右列 ToolBar 内容宽 | 固定 `11.25rem`(180px) | 左段与筛选容器都 `flex_1 + min_w_0` |
| 5 | 右段 ToolBar 内距 | 右列 ToolBar 自身没有内边距，图标距面板边 3px | 组件 XSmall 自带 `p_1`（8px）→ 右端 11px 空白 | `.p_0()` 覆盖（`refine_style` 在 `p_1` 之后，vendor `toolbar.rs:253-257`） |

**筛选区高度为什么是 26px（而不是硬给一个常量）**：`Input.small()` 24px + 上下各 1px 边框
= 26px，落在 32px 工具栏里 `items_center` 居中 → 上下各 3px，**正好等于 DBeaver 的
`marginHeight = 3`**。所以没有新增高度常量：3px 是算术结果，不是抄来的数字。同理工具栏只加
水平 `.px(3px)` 不加 `.py()`——右段 `Toolbar` 受 `min_h_7`(28) 托底，一旦把可用高度压到
26px 就会裁掉它自己的内边距。

**为什么让 `Input` 交出边框**：DBeaver 那圈框画在**父 composite** 上，内部的 `StyledText` 本身
无边框。我们若两层都画就是双框叠加。`appearance(false)` 同时关掉 bg / rounded / 边框 /
焦点环（vendor `input.rs:798-804`、`:809`）。**这是本轮的取舍**：输入框不再有自身焦点环，焦点
靠文字 caret 指示——与 DBeaver 一致（它那圈框也不随 focus 变色），但比 gpui-kit 默认的
`bordered + focus_ring` 弱一档。要回退只需两处：去掉 `.appearance(false)`、撤掉容器边框。
文本内距不受影响（`BaseInput` 的 `input_px/input_py` 在 `input.rs:788-789`，与 appearance 无关）。

**3px 为什么不吃 `px_1`(4px) 档位**：DBeaver 原值就是 3；`ui.rs` 是尺寸常量单一来源，
`px(N.)` 在那是推荐写法（`ui_guard` 的 `BASELINE` 记为额度，本轮 `16 → 17` 并加注理由）。

**验证**：`cargo check -p rds-result-panel` Finished **0 warning**；
`cargo test -p rds-workbench-shell --lib` = **14 passed**；
`cargo test -p rds-result-panel --lib` = **242 passed / 0 failed**（用例数不变，断言加在既有
`panel::tests::rendered_ui_elements_survive`：**真实面板**渲染两帧后 `debug_bounds`
量出 `editor-result-filter` 外框高 **26px**，Input 档位 / 边框 / 工具栏高任一改动都会先红）；
`cargo test -p rds-ui-guard` = **4 passed**（`BASELINE` 更新后；首轮曾因新增 `px(3.)` 命中
`no_new_hardcoded_color_or_pixel` 而 FAILED，是门禁按设计生效）。

**「图片分隔符」一项改判为已对齐（不再算缺口）**：DBeaver 用 `UIIcon.SEPARATOR_V` **图片资产**
（`UIUtils.java:218-222`）是因为 SWT `ToolItem` 里画不了线；本仓用 `div` 画 1px 竖线是**同一个
视觉结果**，不是降级。仍保留 1×24px，不引入资产。

**仍未做**：DBeaver 筛选条的真实像素高（本仓没有可运行的 DBeaver 实例，只能按「组件最小档 +
与标签条等高」推），这条继续作为未验证项登记。

### 8.36 表头底色收回 widget 档（去掉多出来的第四层，2026-11-08）

§8.34 / §8.35 之后工具栏与标签条已同为 `tab_bar.background`（`#F8F8F8` / `#181818`），但表头仍是
`#F5F5F5` / `#252526CC` —— 比 chrome 深一档，于是结果集面板的灰阶是**三层**
（标签条 + 工具栏 `#F8F8F8` → 表头 `#F5F5F5` → 网格 `#FFFFFF`），而 DBeaver 只有**两层**。

**DBeaver 依据（三类取值同出一源）**：

| 区域 | DBeaver 取值 | 出处 |
| --- | --- | --- |
| 表头底 | `header.background` = `COLOR_WIDGET_BACKGROUND` | `plugin.xml:1240`（`org.jkiss.dbeaver.sql.resultset.color.header.background`） |
| 筛选条 / TabFolder 底 | `getDefaultWidgetBackground()` = 主题 `INACTIVE_TAB_BG_START` → 回退 `COLOR_WIDGET_BACKGROUND` | `UIStyles.java:122-129` |
| 网格底 | `getDefaultTextBackground()` = `COLOR_LIST_BACKGROUND` | `UIStyles.java:131-133` |

即表头与筛选条是**同一档 widget 底**，只有数据区才跳一档列表白。本仓 A 方案（用户拍板：不全局压暗，
widget 档就取 `tab_bar.background`）下，这一档就是 `#F8F8F8` / `#181818`，故：

| 槽 | 改前 | 改后 | 依据 |
| --- | --- | --- | --- |
| `table.head.background` light | `#F5F5F5` | `#F8F8F8` | = `tab_bar.background`，取 widget 档 |
| `table.head.background` dark | `#252526CC` | `#181818` | 同上 |

**为什么不动 `table.head.foreground`**：新底比旧底更亮，`#616161` 在 `#F8F8F8` 上的对比度高于在
`#F5F5F5` 上（旧值 `#8E8E8E` 在 `#F5F5F5` 上只有 3.01:1 才被改成 `#616161` 的），所以底色变亮
只提高对比度、不触发 WCAG 回归。文字值保持 —— 注意 DBeaver 的表头字是 `COLOR_LIST_FOREGROUND`
（近黑，`plugin.xml:1247`），与本仓 `#616161` 仍有差异，**本轮不动**（改深会让整条表头变重，属观感
取舍，等真窗口看过再说）。

**影响面（为什么改主题资产而不是结果集单点）**：`table.head.background` 是 `DataTable` 的**全局**
token（`theme/schema.rs:601-603`，回退链 `table_head ← tokens.list_head`，`:1021`），改它同时作用于
结果集网格与 Mock 预览表。这既符合「全局统一配置、不做 per 结果集覆盖」的既定口径，也与 DBeaver
一致（它任何 `Table`/`Tree` 的表头都是 widget 底）。`table.foot.*` 未动（结果集没有 footer）。

**验证**：`assets/themes/rds-theme.json` 用 python `json.load` 严格解析通过（顶层是
`themes` **数组**，按 `modes` 取 `light` / `dark`），并回读两值为 `#F8F8F8` / `#181818`；
`cargo test -p rds-ui-guard` = **4 passed**（主题键名契约照旧）、
`cargo test -p rds-workbench-shell --lib` = **14 passed**、
`cargo test -p rds-result-panel --lib` = **242 passed / 0 failed**（用例数与基线一致，本次无新增断言
——这条是纯配置值，几何不受影响）。

**仍未做（如实登记）**：表头与网格之间的分隔线仍由组件按 `table.row.border`（`#E1E1E1` /
`#3C3C3C33`）画，而 DBeaver 的 `header.border` 是 `COLOR_WIDGET_DARK_SHADOW`（明显更深，`plugin.xml:1258`）。
**该判断里的「无外部覆盖入口」当场被推翻**——入口是 `TableDelegate::render_header`，见 §8.37（同轮接上）。

#### 8.36.1 补记：DBeaver 结果集「选中色 / 表头色 / 网格线」的消费点已取证（2026-11-08）

此前只查到 `plugin.xml` 里的 colorDefinition 默认值、扫不到消费点（`7b78e0f0` 那次整树扫描输出为空）。
本轮限定到 `org.jkiss.dbeaver.ui.editors.data` 插件，链条闭合：

| 环节 | 证据 |
| --- | --- |
| 常量 | `ThemeConstants.java:26-30`：`COLOR_SQL_RESULT_SET_SELECTION_FORE/BACK` = `resultset.color.selection.foreground/background`，另有 `COLOR_SQL_RESULT_HEADER_*`、`COLOR_SQL_RESULT_LINES_*` |
| 注入 | `ResultSetThemeSettings.java:48-73`：`@ThemeColor(...)` 把上述 id 注入 `ThemeListener` 单例 `instance` 的字段（`backgroundSelected` / `foregroundSelected` / `cellHeaderSelectedBackground` / `cellHeaderBorder` / `lineNormalColor` / `lineSelectedColor`） |
| 渲染 | `SpreadsheetPresentation.java:2784/2788/2809/2817/2821` 用 `instance.backgroundSelected` 作**选中格/行**背景（`selectedCellColor`）；`:1535` 表头选中背景；`:1542-1543` 把 `lineNormalColor` / `lineSelectedColor` 交给 `spreadsheet.setLineColor/setLineSelectedColor`；`:3380` 表头边框 |

**结论一（选中色）**：DBeaver 结果集选中是**实心** `#AFD7FF`（175,215,255，`plugin.xml:1156`）+ 前景
`0,0,0`；本仓是 `tokens.table_active` = **珊瑚 20% 透明**（`#BC534033` / `#E8846F33`）。色相与强度都不对齐。

**结论二（为什么本轮仍不改）**：`theme/schema.rs:1036-1059` 的 `clamp_alpha` 把 `table_active` 的 alpha
上限压到 **0.2**。即便把 `table.active.background` 写成不透明的 `#AFD7FF`，运行时也只得到 20% 透明淡蓝
（白底上 ≈ `#E6F1FF`），**复刻不出 DBeaver 的实心强度**；要实心必须改 vendor 的 clamp 上限（属主题契约
层改动，按「不给 vendor 加字段/不改契约」的既定原则否决）。20% 淡蓝的可辨识度弱于现状的珊瑚 20%，
改过去反而更像「没选中」，故**保持现状**，把「是否放宽 clamp 上限以换实心选中蓝」作为**待用户拍板**的
取舍登记（受影响面：结果集 + Mock 预览表两处 `DataTable`，`list_active` 不受影响）。

**结论三（其余三槽）**：`header.selected.background`（`COLOR_LIST_SELECTION`）、`header.border`
（`COLOR_WIDGET_DARK_SHADOW`）、`lines.normal` / `lines.selected`（240,240,240 / 105,105,105，**每格**网格线）
都已确认真的参与结果集渲染，但本仓它们画在 vendor `DataTable` 内部（表头行底色 / 行边框 / 单元格边框），
`TableDelegate` 只暴露 `render_th` / `render_td` 的**内容槽**，没有覆盖这些颜色的入口 —— 要接必须改 vendor，
本轮不动，继续作为未对齐项登记。

### 8.37 表头分隔线对齐 DBeaver `header.border`（2026-11-08）

§8.36 把表头底收回 chrome 档（`#F8F8F8`）之后，表头与网格白（`#FFFFFF`）只差 7/255 —— **底色本身已经不
足以分层**。DBeaver 的分层是两条叠加：表头 widget 底 **＋** 一条明显更深的分隔线（`header.border` =
`COLOR_WIDGET_DARK_SHADOW`，`plugin.xml:1258`）。本轮补的正是这条线。

**入口（此前误判为「无」，本轮修正）**：

| 事实 | 出处 |
| --- | --- |
| 组件只给**数据行**画下边（`border_b_1` + `table_row_border`） | vendor `table/state.rs:2274-2276` |
| 表头行只有铺底 + 文字色，自身没有边框 | `table/state.rs:2059-2069` |
| 组件先铺默认样式，再 `.refine_style(&style)` 重放 delegate 返回元素的样式快照 | `table/state.rs:2056` / `:2069` |
| `TableDelegate::render_header` 返回 `Stateful<Div>`，默认实现是空壳 `div().id("header")` | `table/delegate.rs:64-70` |

即：**delegate 在 `render_header` 里给的边框会被重放到最上层**。之前只看 `render_th`（内容槽）就下了
「无入口」的结论，是取证面不够。

**落地**（`crates/result_panel/src/grid/table_impl.rs`，`TableDelegate for ResultGridDelegate`）：

```rust
fn render_header(&mut self, _window: &mut Window, cx: &mut Context<TableState<Self>>) -> Stateful<Div> {
    div()
        .id("editor-result-header-band")
        .debug_selector(|| "editor-result-header-band".to_string())   // 哨兵，见下
        .border_b(ui::HAIRLINE)
        .border_color(cx.theme().colors.border)
}
```

- **色取 `colors.border`**（`#D4D4D4` / `#3C3C3C`），不是 `table.row.border`（`#E1E1E1`）：表头线必须比行底线
  深一档才有分隔作用。本仓没有 `COLOR_WIDGET_DARK_SHADOW` 这个语义面，`border` 是唯一正确的描边槽；
  要更接近 DBeaver 的强度需要新增语义槽（**未做**，属主题契约层改动）。
- **不会叠线**：数据行只画下边、表头自身无边框，表头下方原本就是断开的。
- **表头带高不变**（单行 26px / 双行 40px）；1px 边框从内容区里吃，单行表头主行 18 → **17px**，仍落在既有
  断言的 `>= 14` / `<= 22` 区间内。

**验证**：`cargo test -p rds-result-panel --lib` = **242 passed / 0 failed**；
`cargo test -p rds-ui-guard` = **4 passed**；`cargo test -p rds-workbench-shell --lib` = **14 passed**。
新增哨兵断言在既有 `grid::tests::header_main_row_keeps_a_readable_height` 里：必须能取到
`editor-result-header-band` 的几何 —— 组件不调用这个 hook 时边框会**静默缺失**，与「组件没这个入口」
无法区分，所以先把 hook 真在跑钉住。

**仍未接（入口确实不存在）**：`header.selected.background`（表头选中列底色）画在 `render_col_wrap`
（`state.rs:1606`），`lines.normal` / `lines.selected`（**每格**网格线）画在单元格包装层 —— 两处都在组件内部，
`TableDelegate` 没有对应槽，要接必须改 vendor。

### 8.38 深色 resultset 配色：DBeaver 其实有整套覆盖（推翻旧结论）

**旧结论作废**：`product-tokens.json` 的 `$comment` 与本文档多处写着「DBeaver 这批 resultset colorDefinition
只有一套浅色值（dark 主题 css 未覆盖），故 dark 一套按亮度比推导」。2026-11-08 实读发现**该文件存在且整份覆盖**：

- 路径：`dbeaver-ref\plugins\org.jkiss.dbeaver.ui.editors.data\css\e4-dark-data-editor.css`（2023B）
- 该插件 `css\` 目录下**只有这一个** css 文件 —— 所以**浅色确实没有覆盖**（浅色走 `plugin.xml` 默认），
  §8.36 的「浅色表头 = `COLOR_WIDGET_BACKGROUND`」结论成立；错的只是「深色也没有」这一半。

**深色全覆盖值（原文照抄，`ColorDefinition#org-jkiss-dbeaver-sql-resultset-color-*`）**：

| 槽 | 深色原值 | 本仓现状 |
| --- | --- | --- |
| `cell.odd.background` | `#252525` | `#1C2129`（推导） |
| `cell.modified.background` | `#963000` | `#3A3120`（推导） |
| `cell.error.background` | `#800000` | `#4A2131`（推导） |
| `cell.new.background` | `#004000` | — |
| `cell.deleted.background` | `#400000` | — |
| `cell.matched.background` | `#8b4513` | — |
| `selection.background` / `.foreground` | `#505050` / `#F4F4F4` | `#E8846F33` / — |
| `lines.normal` | `#1e1e1e` | 组件内部（无入口） |
| `lines.selected` = **`header.border`** | `#696969`（两者同值） | `colors.border` `#3C3C3C` |
| **`header.background`** | **`#394248`**（蓝灰） | `#181818`（上轮改错） |
| `header.foreground` | `#E0E0E0` | `#969696` |
| `header.selected.background` | `#5A5A7C` | 组件内部（无入口） |
| `null.foreground` | `#646464` | `muted_foreground` |
| 数据类型前景 | binary `#808000` / boolean `#808080` / datetime `#5CA259` / numeric `#C0C0FF` / string `#D0D0D0` | 本仓按 `role_color` 走，未接这套值 |

#### 8.38.1 修正两处（照抄原值）

1. **深色表头底 `#181818` → `#394248`**（`assets/themes/rds-theme.json` 的 `table.head.background`）。
   §8.36 的依据「表头 = widget 档」只在**浅色**成立（浅色无 css 覆盖）；深色被 css 明确覆盖成蓝灰，
   与 chrome 底 `#181818` 不是一个值。上轮把两套都写成 widget 档是**过度概括**，本轮按原值改回。
2. **表头分隔线按主题分档**：DBeaver 浅色 `header.border` 取系统 `COLOR_WIDGET_DARK_SHADOW`（无常量可抄），
   深色明确 `#696969`。同一个语义面两套强度不同，只靠 `colors.border` 一个标准字段顶不住 ——
   新增产品 token `result.header.border`（light `#D4D4D4` = 现值 / dark `#696969` = 原值），
   访问器 `ProductTokens::result_header_border()` 缺失时回退 `colors.border`。

**为什么深色三槽（odd / modified / error）仍不照抄**：DBeaver 那几个是**纯饱和深色**（`#963000`、`#800000`），
铺在 `#1F1F1F` 网格上与未改动的行几乎无法区分（它的设计前提是深色网格底更暗、并有自己的前景观）。本仓保持
按深底可读性取的较浅值，**这是有意偏离、不是照抄漏了**，已写进 `product-tokens.json` 的 `$comment`。
要改成照抄得整套（含前景色与网格线）一起换，属另一次决定。

#### 8.38.2 筛选区外框去掉圆角

`CompositeBorderPainter.paintControl` 画的是 `gc.drawRectangle(0, 0, w - 1, h - 1)`
（`CompositeBorderPainter.java:34-39`）—— **直角矩形**，SWT composite 也无圆角。本仓此前挂
`rounded(theme.radius)` 把四角磨圆了，与 DBeaver 的框形不一致，本轮去掉（`panel/render.rs`）。
同理，这也解释了为什么 `Input` 要 `.appearance(false)`：那圈圆角是输入框组件自带的，与外框叠成两层形状。

**验证**：两个主题 JSON 严格解析（`json.load`）通过并回读两值；`rds-workbench-shell --lib` **14 passed**、
`rds-result-panel --lib` **242 passed**、`rds-ui-guard` **4 passed**。

### 8.39 工具栏组件逐档对齐 DBeaver（P0 尺寸档 / P1 列序 / P3 多行框）

用户第 5 次指出工具栏未对齐并授权「逐步对齐」。本轮把「工具栏组件构成」拆成三档落地，
每档一次提交、每档单独跑门禁。

#### 8.39.1 总高：不是「无法量测」，是可算的 30px

此前曾下结论「DBeaver 源码里没有工具栏总高常量、32px 只是推导」——**错**。它没有**单个**常量，
但由四圈 margin 加一个显式行高撑出来（均在 `ResultSetFilterPanel.java`）：

| 来源 | 值 | 行号 |
| --- | --- | --- |
| 外层 `marginHeight` | 3 | `:164` |
| `filterComposite` `marginHeight` | 2 | `:175` |
| `MIN_FILTER_TEXT_HEIGHT`（行高，同时是展开 / 执行 / 历史三面板的 `heightHint`） | 20 | `:103`、`:1067`、`:1304`、`:1351` |
| `filterComposite` `marginHeight` | 2 | `:175` |
| 外层 `marginHeight` | 3 | `:164` |

合计 **3 + 2 + 20 + 2 + 3 = 30px**。本仓 `RESULT_TOOLBAR_HEIGHT` 由 `2.0`（32px）改为
**`1.875`（30px）**。工具栏从此**不再与标签条（32px）等高** —— 这是 DBeaver 的原生关系：
筛选条本来就比它上方的 TabFolder 矮。

#### 8.39.2 尺寸档：控件 24px → 20px（本仓真有这个档）

`Size::input_h` 的映射（`vendor/gpui-component/src/sizing.rs:281-289`）：
`Large h_11` / `Medium h_8`(32) / `Small h_6`(24) / **`XSmall h_5`(20)**。
DBeaver 的 20px 正是 `XSmall`，本仓此前用 `Small`，正好高一档。

共 15 处 `.small()` → `.xsmall()`：工具栏的筛选框 / 执行 / 展开 / 清除，展开条的展开 / 运行 /
清除 / 收起，右段 pushdown / comments / compare / commit / discard / copy / analysis
（`DropdownButton` 与其内 `Button` 各一次）。

筛选容器补 `.p(ui::HAIRLINE)`：DBeaver 那圈框线由 `paintControl` 画、**不占布局**，
「框到内容」只有 2px；本仓边框占 1px 布局，再补 1px 内距才等价 → 容器 **24px**，
与 DBeaver 的 `2 + 20 + 2` 逐段相等。

#### 8.39.3 列序：`GridLayout(5)` + `horizontalSpacing = 0`

DBeaver 的 `filterComposite` 是 `GridLayout(compactMode ? 4 : 5, false)`，`horizontalSpacing = 0`
（`:174-179`），五列横排：**活动对象 / 展开 / 文本 / 执行 / 历史**（`:184-214`）。

本仓改为 **`[展开][文本][执行][清除]`** 紧排（去掉 `gap_1` 的四条 4px 缝）：

- 展开钮从行尾移到文本列**左侧**（DBeaver 第 2 列 `FilterExpandPanel`，`:186/:1303-1306`），
  且不随 `filter_expanded` 分支：展开态下工具栏只剩它（点它收起）
- 执行钮从 `Input.suffix` 拆成**独立列**（DBeaver 第 4 列 `ExecutePanel`，`:209/:1351`）
- 清除钮留在末位：DBeaver 的 ERASE 在右列 ToolBar 首位，但本仓右段是「结果集动作」而非
  「筛选动作」，两组语义不同，不机械搬移

#### 8.39.4 多行控件：`TextViewer(SWT.MULTI)` → `Textarea`

DBeaver 的筛选输入是 `TextViewer(SWT.MULTI)`（`:192`），常态 1 行、展开 5 行
（`gd.heightHint = getLineHeight() * (filterExpanded ? 5 : 1)`，`:1320-1327`）。

本仓换成 `Textarea`（`TextareaState`），高度由 `.h()` 决定（多行分支 `input.rs:794-797`
走 `h_auto()` + `height`，故 `.h()` 精确生效）：

- 行高常量 `LINE_HEIGHT = Rems(1.25)` = **20px**（`input.rs:543`）—— 天然等于
  `MIN_FILTER_TEXT_HEIGHT`
- 工具栏常态 `.h(rems(1.25))` = 1 行；展开条 `.h(rems(6.25))` = **100px** = 5 行
- `submit_on_enter(true)`：多行框回车默认插换行（`state.rs:1618` 的
  `is_multi_line && !submit_on_enter` 分支），但筛选词是单行语义，换行会让拼出的 WHERE
  带 `\n` → 关掉后回车只发 Submit（本仓未订阅），交互与改造前一致

**`Textarea` 没有 prefix / suffix 槽**（`textarea.rs` 只暴露 token / h / appearance /
bordered / context_menu / on_paste 等，全文无插入槽），所以展开条的展开 / 运行两个钮改为
同一行的兄弟元素（`[展开][文本][运行]`），与 DBeaver 的列布局一致。

#### 8.39.5 三条哨兵断言

| 断言 | 位置 | 值 |
| --- | --- | --- |
| 筛选区外框高 | `panel/tests.rs` | 26 → **24px** |
| 工具栏高 | `grid/tests.rs` | 32 → **30px** |
| 展开条高（新增） | `panel/tests.rs` | **100~115px**（≈109，钉「真的 5 行」；退回 1 行即红） |

量绝对像素而不是比值：档位、边框、内距、行高任一改动都会先红。

#### 8.39.6 仍未对齐（P2，未开工）

DBeaver 右列 ToolBar 有五个 `ToolItem`（`:315-379`）：ERASE / FILTER（筛选设置下拉）/
分隔 / RS_BACK / RS_FORWARD（结果集前后跳，带历史菜单）/ FIND_REPLACE。
本仓缺其中三个**功能**：筛选历史下拉、筛选设置对话框、查找替换浮层（P2 范围）；
`RS_BACK/FORWARD` 本仓用标签页替代，机制不同，不算缺口。

此外本仓的「展开」仍是右栏另起一条，而非「输入框就地变 5 行」—— 保留该形态是用户早前
确认的功能（「展开顶宽 + wrapped 重查 + 右栏执行 SQL 页签」），属**有意保留的产品差异**。

**验证**：`rds-result-panel --lib` **242 passed**、`rds-ui-guard` **4 passed**、
`rds-workbench-shell --lib` **14 passed**；三档提交 `a4574c83`（P0）、`4f8acd47`（P1）、
P3 与本文档同一提交。

### 8.40 右栏：面板内工具条（R1）+「计算」页对齐 DBeaver Calc（R2）

右栏对比分析交付后用户拍板：**R1 只加每面板工具条**（不撤销 2026-10-28「横 tab 只显当前
页签」的既有决定），**并直接做 R2**。本节落地 R1 的第一处（计算页工具条）与 R2 的全部内容。

#### 8.40.1 为什么 R1 不搬「页签并列」

DBeaver 右栏是 `new CTabFolder(viewerSash, SWT.FLAT|SWT.TOP)`（`ResultSetViewer.java:358`），
每个面板一个 `CTabItem(SWT.CLOSE)`、并列可关（`:1330-1345`）；本仓 2026-10-28 已拍板横 tab
只显当前页签（320px 窄右栏并列多页签会把标题挤没）。**该决定未推翻**——R1 只补每个面板内部
固定一行靠右工具条，即 DBeaver 的
`ToolBarManager(SWT.HORIZONTAL | SWT.RIGHT | SWT.FLAT)`（`:1337-1374`，`contributeActions`
挂面板自己的动作，`needsSeparator()` 为真时插一条分隔线）。

#### 8.40.2 计算页差异实证（`AggregateColumnsPanel.java` 全 498 行）

| 维度 | DBeaver Calc | 本仓改造前 | 本轮 |
| --- | --- | --- | --- |
| 输入 | 多单元格 / 多列选区（`aggregateSelection`，`:199-214`） | 单列（网格选中格优先、列浏览器选中列兜底） | 保持单列，见 8.40.4 |
| 函数集 | 注册表 + ＋ / − / Reset（`:365` / `:418` / `:438`） | 六项写死 | **可增删 + Reset** |
| 聚合口径 | `aggregateAsStrings` CHECK（`:348`） | 无 | **数值 / 字符串 toggle** |
| 分组聚合 | `GroupByColumnsAction` CHECK（`:333`） | 无（另有独立「分组」页签） | 不实现，见 8.40.4 |
| 表头 | `Tree` 两列头 Function / Value（`:90-94`） | 无 | **两列表头** |
| 千分位 | `###,###,###,###,###,##0.###`（`:65-66`） | `format!("{:.2}")` | **`fmt_grouped`** |
| 面板工具条 | 六动作（＋/−/Reset/分隔/分组/口径，`:322-331`） | 无 | **＋ / Reset / 口径**（R1） |
| 右键菜单 | Copy / CopyAll（`:106-112`） | 无 | 未做，登记在册 |

取值链路保持原口径：`column_stats_for(column)`（内存档数值列走 `FactFrame` 列式原生聚合，
有被删行时回退显示矩阵口径）；**只有字符串口径**才取整列 `column_values(column)` 算字典序
极值（`text_minmax`），数值口径不物化整行矩阵。

#### 8.40.3 四处代码落点

**请求与状态**（`ports.rs` / `panel/data.rs` / `panel/lifecycle.rs`）：

- `ResultRequest{AddStatsItem, RemoveStatsItem, ResetStatsItems, SetStatsMode}`
- `StatsItem{Count, Nulls, Min, Max, Avg, Sum}` + `ALL` / `label()` / `supports_text_mode()`
- `StatsMode{Numeric(默认), Text}` + `toggled()`
- 面板字段 `stats_items: Vec<StatsItem>` / `stats_mode: StatsMode`，构造默认
  全集 + `Numeric`

**事件路径**（`panel/events.rs`）：`add_stats_item` 按 `ALL` 的固定顺序插入而非追加（删了
再加回去要回原位，否则行序随操作乱跳）；`remove_stats_item` / `reset_stats_items` /
`set_stats_mode` 都只在真变化时 `cx.notify()`。`editor/src/view/host/results/dispatch.rs`
四条分支只转发（纯面板视图状态，不落盘、不影响取数）。

**取值纯函数**（`panel/tabs.rs::stat_value`）：计数类不受口径影响；数值口径
Min/Max/Avg/Sum 走千分位，字符串口径 Min/Max 走字典序、Avg/Sum 出 `—`（DBeaver 的
SUM/AVG 对字符串累计不成立）。

**工具条与行**（`panel/tabs.rs::stats_toolbar` / `stat_row`）：
`Toolbar::new("editor-result-stats-actions").xsmall().p_0()` 收「＋ / Reset / 口径」三件，
左列是「「列名」统计」；行 = 函数名（`RESULT_SIDEBAR_KV_LABEL_WIDTH`）+ 值 + 行尾 ×。
容器高走新常量 `ui::RESULT_PANEL_TOOLBAR_HEIGHT`（**1.75 = 28px**）——它是 vendor
`toolbar.rs:242-259` 里 XSmall 档自带的 `min_h_7`，容器写 24px（状态行档）会让 Toolbar
溢出 4px 压住下方表头，所以容器高必须与组件托底高一致。

#### 8.40.4 三处**有意偏离**（不是遗漏）

1. **不做「行选中后点 −」**：DBeaver 的 − 删的是 `Tree` 里选中的函数行；本仓统计行是普通
   元素、没有行选中模型，改为每行行尾一个 × —— 避免为对齐一个按钮引入整棵 `Tree`。
2. **不做「按列分组聚合」**：`GroupByColumnsAction` 的输入是**多单元格选区**，本仓 delegate
   只有 `selection_pos: Option<(usize, usize)>` 单格 + `selected_col` 单列（`multi_selected`
   是列浏览器清单的多选，不是网格选区），派生不出「选中的若干单元格」。要做需先给
   `DataTable` 引入单元格选区，属独立工程；且本仓已有独立「分组」页签，重复实现会两处打架。
3. **不做右键 Copy / CopyAll**：本仓统计页尚无右键菜单基建，且工具栏已有全局复制；
   登记为后续增强（`AggregateColumnsPanel.java:471-496` 的 `func=value` 文案可直接照搬）。

#### 8.40.5 新增测试（5 条）

| 用例 | 文件 | 钉住什么 |
| --- | --- | --- |
| `fmt_grouped_inserts_thousand_separators` | `grid/tests.rs` | 千分位分组 + 负数 + 非有限值 |
| `text_minmax_ignores_null_and_orders_lexically` | `grid/tests.rs` | 字符串口径排除 NULL、字典序、空列=无极值 |
| `stat_value_respects_mode_and_missing_samples` | `panel/tests.rs` | 口径决定 Min/Max 比较方式、Avg/Sum 无值、计数不受口径影响 |
| `stats_defaults_are_full_set_in_display_order` | `panel/tests.rs` | 默认全集与展示顺序、`supports_text_mode`、口径可来回切 |
| `stats_tab_renders_toolbar_header_and_rows` | `panel/tests.rs` | **真渲染**：工具条三件 + 两列表头 + 六行 + 行尾 × 都可见；量像素（工具条 28px、表头与行各 20px）；停用 Sum 后该行消失、Reset 后回来 |

最后一条是渲染层闭环，防的正是「代码里写着但对齐没落到画面上」这类静默回退。

**验证**：`rds-result-panel --lib` **247 passed**（原 242 + 5）；`rds-ui-guard` **4 passed**；
`rds-workbench-shell --lib` **14 passed**；`cargo check -p rds-editor` 通过。

#### 8.40.6 仍未做

- 值详情 / 分组 / 引用 / 执行SQL 四页的同构面板工具条（R1 剩余部分）
- 统计项增删的**跨会话持久化**（DBeaver 写 `IDialogSettings`；本仓暂无对应偏好项）
- `AggregateColumnsPanel` 的 `packColumns` 等列宽自适应（本仓右栏是固定列宽）
- R3（Grouping 配置化）/ R4（Metadata、References 实做）/ R5（布局方向切换 + 可拖分栏）

### 8.41 右栏 R1 收口：四页页内工具条统一（值详情 / 分组 / 引用 / 执行SQL）

§8.40 只落了计算页。本节把「每面板一条页内工具条」补到其余各页，并用 DBeaver 的
面板动作清单决定**每页放什么**——不按「每页都得摆几个按钮」造齐。

#### 8.41.1 各面板动作实证（`contributeActions`）

| 面板 | DBeaver 工具条动作 | 依据 |
| --- | --- | --- |
| Calc | ＋ / − / Reset / 分隔 / 分组 / 口径 | `AggregateColumnsPanel.java:322-331`（§8.40 已落地） |
| Value | `valueManager.contributeActions(…)` + 可编辑时的 `SaveValueAction` | `ValueViewerPanel.java:409-428` |
| Metadata | **只一个**「按结果集生成 DDL」 | `MetaDataPanel.java:204-215` |
| References | **空实现**（方法体里没有任何 `manager.add`） | `ReferencesPanel.java:156-158` |
| Grouping | 无 `contributeActions`（动作经 `GroupingAction` 扩展点驱动，属 R3） | `resultset/panel/` 全量 grep 无命中 |

结论：**References 页在 DBeaver 本来就是空工具条**，本仓维持空态即等价；Metadata 页
本仓尚未建（R4）。

#### 8.41.2 统一容器

`panel_toolbar(debug_id, title, actions, cx)`（`panel/tabs.rs`）：

- 高 `ui::RESULT_PANEL_TOOLBAR_HEIGHT`（**28px** = vendor `Toolbar` XSmall 自带的 `min_h_7`）
- 左标题 `truncate`（窄栏必须截断，否则推挤右侧动作）/ 右 `Toolbar(…).xsmall().p_0().contents(actions)`
- 底边一条 `HAIRLINE` 分隔线

计算页原先自绘的那份改为复用它，五页共用同一形态（避免两套工具条各走各的）。

#### 8.41.3 各页落点

| 页 | 标题 | 右侧动作 | 相对原状的变化 |
| --- | --- | --- | --- |
| 值详情 | 「列名 · 类型」 | 位置标记（灰字）+ 复制值 | 原 24px（`RESULT_STATUS_BAR_HEIGHT`）标题行 → 28px 工具条；**新增** `ResultRequest::CopyValue`，宿主 `cx.write_to_clipboard`（与 `CopySql` / `CopyExecSql` 同机制），面板提供 `selected_value_text(cx)` |
| 分组 | 「按「X」分组 · N 组」 | 无 | 原 24px 行 → 28px 工具条 |
| 引用 | 空态 | 无 | 不动（依据 §8.41.1） |
| 执行SQL | 「生成的 SQL」 | 复制 | 原 `p_2` 里自绘标题行 → 28px 工具条；复制钮 `.small()` → `.xsmall()`；SQL 内容块改 `m_2` + 圆角 + 滚动 |
| 计算 | 「「X」统计」 | ＋ / Reset / 口径 | 改走统一容器（R2 已落地，行为不变） |

背景口径：DBeaver 的 `ToolBarManager` 是 `SWT.RIGHT | SWT.FLAT`，没有左边框也没有
背景色——本仓容器只加 `border_b` 一条（与它的 `needsSeparator()` 等价），背景沿用右栏
`theme.colors.sidebar`。

#### 8.41.4 新增测试

`right_panel_pages_share_the_toolbar_shape`（`panel/tests.rs`）：值详情页断言工具条与复制钮
可见、**量高 28px**（与计算页同高），分组页断言工具条在，执行SQL 页断言工具条与复制钮在。
测试壳抽成 `panel_harness`（空面板 + 两列网格 + 预选第 2 列），与 §8.40.5 那个渲染用例同源。

**验证**：`rds-result-panel --lib` **248 passed**（原 247 + 1）；`cargo check -p rds-editor` 通过。

#### 8.41.5 仍未做

- 值详情「保存值」（DBeaver 的 `SaveValueAction`）：本仓单元格编辑走网格内联编辑，值页只读
- Metadata 页 / 引用页的真实内容（R4）
- 页内工具条无更多动作可放：320px 窄栏里标题 + 一个动作已接近饱和，再加会截断标题

### 8.42 R3 第一步：分组页配置化（多列 / 排序 / 仅重复）

DBeaver 的分组面板动作由 `GroupingAction` 扩展点驱动，配置来自 `GroupingConfigDialog`
（分组列树 + 每列一个聚合函数 + 排序 + 仅重复）。本仓先落**分组键与结果组织**三件事；
每列聚合函数仍走计算页（R2 已覆盖，重复实现会两处打架）。

#### 8.42.1 落地内容

| 能力 | DBeaver 对应 | 本仓落点 |
| --- | --- | --- |
| 多列分组 | `GroupingConfigDialog` 的分组列集合 | `group_columns: Vec<usize>`（工具条下拉勾选，顺序 = 键拼接顺序） |
| 排序 | 分组结果的排序配置 | `GroupSort{Value, CountDesc}` + `arrange_groups` |
| 仅重复 | 同名选项（只留出现 > 1 次的组） | `group_duplicates_only` |
| 每列聚合函数 | `GroupingConfigDialog` 的聚合列 | **不做**：计算页已有（§8.40） |

#### 8.42.2 数据层

- `grid/stats.rs::group_by_tuples(rows: &[Vec<String>])`：元组键分组内核，保留首现顺序
- `grid/text.rs::group_by_columns_for(&[usize])`：0 列 = 空；1 列直接走 `group_by_for`
  （保留内存档原生数值键归一化，`"1"` 与 `"1.0"` 不会被拆成两组）；多列**逐列各自取键**
  ——能走事实档原生数值的列用归一化键，其余列用显示文本。**订正**：本段原写「DBeaver 的
  多列分组键也是按各列显示文本拼的，代价是多列下 `2.0` 与 `2` 会分成两组」属未实证推断，
  DBeaver 实际把分组下推成 SQL `GROUP BY`（按列值比较），见 §8.48.1；本仓的多列口径已于
  §8.48 改为逐列归一化
- `panel/tabs.rs::arrange_groups`：先过滤再排序；`CountDesc` 同数按值序兜底，保证同一份
  数据每次渲染顺序一致（否则 UI 会随统计重算跳动）

#### 8.42.3 UI

分组页工具条（§8.41 的统一容器）右侧三个动作：

| 控件 id | 文案 | 行为 |
| --- | --- | --- |
| `editor-result-group-columns` | 「列」（`DropdownButton` 勾选式多选） | `ToggleGroupColumn`（隐藏列不进分组键） |
| `editor-result-group-sort` | 「按值」/「按计数」 | `SetGroupSort` |
| `editor-result-group-dup` | 「仅重复」（toggle） | `SetGroupDuplicatesOnly` |

标题「按 A + B 分组 · N 组」；多列时行内容按「 | 」连接。三条请求都是**纯面板视图状态**
（不落盘、不影响取数），与计算页那四条同机制。

#### 8.42.4 新增测试

- `arrange_groups_sorts_and_filters`：按值序 / 按计数降序（同数按值序兜底）/ 仅重复 /
  多列元组比较
- `right_panel_pages_share_the_toolbar_shape` 的分组段扩成断言三个动作都可见

**验证**：`rds-result-panel --lib` **249 passed**（原 248 + 1）。

#### 8.42.5 仍未做

- 分组列集合的跨会话持久化。**订正**：原写「DBeaver 写 `IDialogSettings`」未核实到实证——
  `GroupingPanel.loadSettings()` / `saveSettings()`（`GroupingPanel.java:114-120`）都只取出
  一个 `"groups"` section 就结束，函数体为空；分组面板真正走 preference 的只有「仅重复」
  与「默认排序」（`GroupingResultsContainer.java:231-232` 读
  `RS_GROUPING_SHOW_DUPLICATES_ONLY`、`:250` 读 `RS_GROUPING_DEFAULT_SORTING`）。分组列
  集合本处未见持久化，是否另有存取路径本轮未核实，故本仓不做不算偏离
- 每组展开看行（DBeaver 的分组结果是**另一条查询的结果集**，不是原结果集的行树）；
  本仓是平列表 + 计数
- 分组列顺序的可视化调整（本仓按勾选顺序固定，不开拖拽；DBeaver 走
  `GroupingResultsDecorator.java:161-215 dropColumns` 接收拖放，落点列索引见
  `getOverColumnIndex:219-224`，最终 `GroupingColumnsContainer.moveColumns:126`——实测见 §8.49.1）

### 8.43 订正：「Metadata 面板」本仓已合并进列浏览器页

此前多份交接把「本仓缺 Metadata 页」记成 R4 缺口。逐行核过后订正：
`panel/sidebars.rs::render_browser_tab`（`:241-295`）已在列浏览器页底部输出
**来源表 / 来源列 / 注释 / 驱动类型 / 可空 / 默认值**六项——正是 DBeaver
`MetaDataPanel` 的内容（`MetaDataPanel.java:217-302`：属性树 + 额外 `Description` 列）。

差异只在**呈现形态**：DBeaver 是一座属性树（每列一行 + Description 列），本仓是
「列清单表 + 选中列明细」。本仓右栏固定 320px，把六项摊成列必然挤，故保留合并形态。
于是 R4 实际只剩两件：**References 页**（需要外键元数据，引擎侧尚无该端口）+ Metadata 页的
「生成 DDL」动作（本仓无 DDL 生成端口）。

### 8.44 R5 第一步：右栏宽度可拖（对齐 DBeaver `setWeights` 的手感）

#### 8.44.1 DBeaver 侧事实与两条实现路线

`ResultSetViewer.java` 右栏由 `VerticalFolder(SWT.RIGHT)` + `CTabFolder` 装配（`:336-358`），
尺寸走两处：

- `:1032 setOrientation(...)` —— 面板可停靠在网格**下方或右侧**，可切换
- `:1033 setWeights(panelWeights)` —— 分栏**可拖**且比例持久化

本仓右栏是「行内固定宽 320px + 宽度动画开合」的 `<h_flex>` 子元素，不在任何
`ResizablePanelGroup` 里。vendor 0.9.0 给了两条路：

| 路线 | 入口 | 代价 |
| --- | --- | --- |
| 组式 | `h_resizable(id)` + `resizable_panel()` | 要把整行换成 `ResizablePanelGroup`，`flex_1` 语义改为像素分配 + 每组 panel 都要 `size_range`；会牵动现有 249 条渲染断言 |
| 独立手柄（**本轮采用**） | `resize_handle(id, Axis::Horizontal)` | 只加一条贴边手柄，行布局与动画全部不动 |

选后者有 vendor 自己的依据：`gpui-component/src/resizable.rs` 模块注释写明
「for an edge no `ResizablePanelGroup` owns -- **an application's sidebar, say**.
What the drag resizes is the caller's」，正是本仓右栏这种「不在 panel group 里的边」。

#### 8.44.2 落点（5 个文件）

| 层 | 改动 |
| --- | --- |
| `ports.rs` | `ResizeRightSidebar(i32)`：`dx` = 本次水平位移（向右为正 → 右栏变窄）。取整到 i32 是因为 `ResultRequest` derive 了 `Eq`，`f32` 不满足 |
| `panel/data.rs` | `right_sidebar_width: Pixels`（默认 320px）+ `right_sidebar_dragging: bool` |
| `panel/events.rs` | `resize_right_sidebar(dx, cx)`：`clamp_right_sidebar_width` 夹取后写回 + 置 `dragging`；`toggle_value_sidebar` 里清 `dragging`（开合补间恢复） |
| `panel/sidebars.rs` | 纯函数 `clamp_right_sidebar_width`；`SidebarResizeDrag`（载荷 + 空渲染实体）；`render_right_sidebar_resize()` 挂手柄；`full` 改读 `self.right_sidebar_width`；`anim_ms` 拖拽期 1ms、静止期 160ms |
| `editor …/results/dispatch.rs` | 转发到 `p.resize_right_sidebar(dx as f32, cx)` |

`ui.rs` 新增两档（f32 形式，不触发 `ui_guard` 的裸 `px(` 计数）：
`RESULT_SIDEBAR_RIGHT_MIN_WIDTH = 12.0`（192px）、`RESULT_SIDEBAR_RIGHT_MAX_WIDTH = 40.0`（640px）。

手柄形态：`resize_handle::<…>("editor-result-right-sidebar-resize", Axis::Horizontal)`
+ `.inside(HandleEdge::Leading)`（贴右栏**左**边缘）+ `.on_drag(载荷)` + `.on_drag_move`。
band 由 base 负责（`HANDLE_SIZE = px(1.)` + `HANDLE_PADDING = px(2.)`，edge-hug 3px /
straddle 5px 命中带宽）、光标 `cursor_col_resize`、`ResizeHandleState{Idle,Hovered,Pressed,Dragging}`
与指示条外观（`resizable::resize_handle_appearance()`，空闲态只画 hairline）。

#### 8.44.3 四个踩坑（均本轮自修）

1. **`MouseMoveEvent` 没有 `previous_position`**（0.3.11 实测只有 `position` /
   `pressed_button` / `modifiers`）。位移只能自己攒：`on_drag` 把按下时的 `position.x`
   写进 `window.use_keyed_state("editor-result-right-sidebar-drag-x", …)` 的
   `Cell<Pixels>`，`on_drag_move` 读上一帧值、`as_f32()` 算增量、再写回。
   增量口径不依赖绝对坐标（窗口缩放 / 滚动都不会算错）。
2. **`f32` 进不了 `ResultRequest`**（枚举 derive `Eq`）。载荷取 `i32`（`dx.round() as i32`）。
3. **`EffectTransition` 的 duration 不能是 0**：vendor 的 progress = elapsed/duration，
   0 会算出 NaN 并命中 `animated value should be finite` 断言（真机上是 panic，不只是
   测试问题）。拖拽期改用 **1ms**，且 `from == to`，视觉即直写。
4. **`Pixels / Pixels` 可行但会撞 `ui_guard`**：`(pos - prev) / px(1.)` 会新增一处裸
   `px(1)`（该文件 baseline 2）。改用 `Pixels::as_f32()`（`geometry.rs:2802`）即合规。

#### 8.44.4 新增测试（249 → 252）

- `right_sidebar_width_clamps_between_ui_limits`：小幅位移精确换算（±40px）/ 拖过头夹到
  192 与 640 / 已在边界再同向拖不反向越界
- `resize_request_clamps_width_and_suspends_the_open_animation`：拖拽改宽度 → 夹取 +
  `dragging` 置真；点开关 → `dragging` 清回
- `resize_handle_mounts_only_when_the_sidebar_is_expanded`：默认宽展开有手柄 → 收起无 →
  窄展开无 → 再宽展开有（宽展开前把宽度切到「拖拽直写」档，避开 160ms 补间首帧的 0 宽）

#### 8.44.5 本轮未做（登记）

- **布局方向切换**（DBeaver `:1032 setOrientation`：网格下方 vs 右侧）：本仓右栏是
  `h_flex` 行内子元素，改上下结构要重排根 `v_flex` 与状态栏归属，另作一轮。
- **比例持久化**：已做完，见 §8.45（全局偏好项 `result_panel.sidebar_width`，px 值）。
- **双击复位默认宽度**：DBeaver 无此交互，暂不引入。
- 手柄 3px / 5px 命中带宽、拖拽时右栏内容的换行与截断观感，仍需真窗口目视（当前不具备条件）。

### 8.45 R5 第二步：右栏宽度持久化（对齐 DBeaver `panelRatio`）

#### 8.45.1 DBeaver 侧事实（实读 `ResultSetViewer.java`）

- `:1021` `int[] panelWeights = new int[]{700, 300};` —— 默认 7:3。
- `:1023-1031` 有 `activePresentationDescriptor` 时读 `getPresentationSettings()`：
  `:1027-1029` `if (settings.panelRatio > 0) panelWeights = new int[]{1000 - settings.panelRatio, settings.panelRatio};`；
  `:1032` `setOrientation(...)`，`:1033` `viewerSash.setWeights(panelWeights)`。
- `:382-389` `panelFolder.addListener(SWT.Resize, ...)` → `getPresentationSettings().panelRatio = weights[1];`
  —— **只写内存 settings 对象，不落盘**。
- `:1238-1248` `getPresentationSettings()`：按 `activePresentationDescriptor` 分组存 `presentationSettings` map。
- `:1250-1268` `savePresentationSettings()`：把 map 写进 `IDialogSettings` 的
  `SETTINGS_SECTION_PRESENTATIONS` 段（每 presentation 一子段：`enabledPanelIds` / `activePanelId` /
  `panelRatio` / `panelsVisible` / `verticalLayout`）。
- 调用点只有 5 处（`:1205` 在 `setActivePresentation` 内、`:1437`、`:1515`、`:1548` =
  `toggleVerticalLayout()`、`:2335`）：**拖拽后不立即落盘**，靠展示类型切换 / 面板开关 / 方向切换时才写。

结论：DBeaver 的 `panelRatio` = **千分比、表示右栏占比、按展示类型分组、跨结果集共用**；
拖拽只改内存，落盘是「惰性」的（其它交互顺手保存）。

#### 8.45.2 本仓方案与偏离

采用 **全局偏好项 `result_panel.sidebar_width`（px，默认 320，范围 192~640）**：

- 粒度：全局一份，不按展示类型分多份 —— 本仓 `Presentation` 与 DBeaver 的
  presentation descriptor 不是一一对应（本仓一个面板内同时有网格/文本/值详情等多页），
  按类型分份需要先定义映射，属未决项（登记在 8.45.5）。
- 单位：px 而非千分比 —— 本仓右栏是绝对宽度的 `h_flex` 子元素（`right_sidebar_width: Pixels`），
  存 px 可省去「÷ 父宽」的换算依赖（父宽在拖拽期其实就是窗口宽，但恢复时不可靠）。
- 落盘时机：比 DBeaver 更早 —— **松手即写**（DBeaver 是惰性写）。本仓没有「展示类型切换」这个用户动作
  的稳定对应点，惰性写会让拖完的宽度在重启后丢失。

#### 8.45.3 落点（10 个文件）

| 层 | 文件 | 改动 |
| --- | --- | --- |
| settings | `model/result_panel.rs` | `sidebar_width: i64` + `Default` 初值 + `default_result_sidebar_width()` |
| settings | `registry/entries/result_panel.rs` | `result_panel.sidebar_width` 条目（Number / 布局组 / 预设 192·320·480·640） |
| settings | `registry/mod.rs` | `Slot::ResultPanelSidebarWidth` + key 映射 + `KindTag::Number` |
| settings | `service/mod.rs` | 读值分支 `SettingValue::Number(...)` |
| settings | `service/result_panel.rs` | `apply_slot` 分支（越界 return false，不静默夹取）+ 公开 setter |
| 面板 | `ports.rs` | `ResultRequest::EndResizeRightSidebar`（无载荷，`Eq` 安全） |
| 面板 | `panel/events.rs` | `end_right_sidebar_resize() -> Option<Pixels>` / `set_right_sidebar_width_px()` |
| 面板 | `panel/render.rs` | 根元素 `.when(dragging, on_mouse_up(→ EndResizeRightSidebar))` |
| 宿主 | `results/dispatch.rs` | 分支：取回宽度 → `SettingsService::set_result_panel_sidebar_width` |
| 宿主 | `view/host/construct.rs` | 构造面板后把偏好项注入 `set_right_sidebar_width_px` |

关键做法：

- **松手信号**走「面板根 `on_mouse_up`」而非 `window.on_mouse_event`：后者要求
  `debug_assert_paint()`（只在 paint 期成立），在 `Render::render` 内注册窗口级监听
  在 headless 测试里不稳；`on_mouse_up` 走正常冒泡，且只在 `right_sidebar_dragging` 为真时挂
  （`.when` 门控），拖拽结束后自动摘除，不产生常驻窗口级监听。
- **初值注入**复用 `clamp_right_sidebar_width(px(w), 0.)`（`dx = 0` 时它等价于「夹取当前值」），
  不在面板里再写一份上下限比较。
- **setter 幂等**：与现值相同直接 return，避免拖拽结束的无谓写盘。

#### 8.45.4 本轮踩到的环境事故（勿重复踩）

1. **D 盘被 target 占满（0 字节空闲）导致 `Edit` 把文件写空**：`crates/settings/src/service/result_panel.rs`
   与并行线的 `docs/architecture/editor/editor-backlog.md` 都变成 0 字节（`git diff --numstat`
   显示 `0 338` / `0 1048`）。处置：`git checkout -- <路径>` 复原并重做本线改动；随后全仓扫
   0 字节文件（命中只剩 `.gitkeep`、`docs/tmp/*.log`、`tools/_archive/r31_*.log`、`.delta` 嵌套仓库旧文件）。
   → 写盘前先看 `Get-PSDrive -Name D` 剩余空间。
2. **`RUST_MIN_STACK=268435456` 会让冷编译崩**：该变量同样作用于 rustc 的编译线程栈，
   冷编 `windows` crate 时多线程各申请 256MB → `STATUS_STACK_BUFFER_OVERRUN (0xc0000409)`。
   **编译命令不要带这个大栈设置**（它本是给 gpui 测试进程用的）；实测去掉后
   `cargo check -p rds-settings` 一次通过（4m51s，0 error / 0 warning）。
3. **并行线 `cargo clean` / `cargo test` 会与本线抢同一个 `target` 锁**：表现为
   `Blocking waiting for file lock`、`could not parse/generate dep info ... os error 3`。
   处置：先等 `cargo.exe` 进程清零再起本线的门禁，不要并发跑。
4. 另一条：`Edit` 的 `old_string` 末尾带换行而 `new_string` 不带时，会把两行拼成一行
   （`toggle_left_sidebar` 的签名与首句曾被拼在一起）；改完立刻回读该处确认。

#### 8.45.5 验证

- `cargo check -p rds-settings`（4m51s，0 error / 0 warning）/ `-p rds-result-panel` / `-p rds-editor` 均通过。
- 门禁：panel **254**（252 + 2）、settings **35**、workbench_shell **14**、ui_guard **4** 全绿。
  两条新测是纯逻辑断言：`sidebar_width_pref_is_injected_and_clamped`（注入 320 原样落位、
  100→192、9999→640）、`end_resize_reports_width_only_while_dragging`（非拖拽 None、
  拖 40px 后松手 `Some(280)`、再松一次仍 None）。
- settings 侧不另加单测：新条目的形态/预设/默认值与字段映射已由 `registry/mod.rs`
  既有的四条契约测试（`every_registered_key_has_a_matching_slot` /
  `page_rows_are_scalar_and_writable` / `number_presets_are_consistent` /
  `default_value_follows_the_declared_kind`）自动覆盖（本次 35 条全过）。
- **尚未做的验收**：真窗口重启后宽度复现、拖拽落盘时机的手感。

#### 8.45.6 本轮未做 / 待定

- **按展示类型分组**（DBeaver 语义原样）：需先定义本仓 `Presentation` ↔ descriptor 映射，用户未表态。
- **双击手柄复位默认宽度**：DBeaver 无此交互，暂不引入。
- 重启后宽度复现、拖拽落盘时机的手感，仍需真窗口目视。

### 8.46 R5 第三步：布局方向切换（对齐 DBeaver `toggleLayout`）

#### 8.46.1 DBeaver 侧事实（逐条可核）

- `ResultSetViewer.java:1020` `boolean verticalLayout = false;`（默认横排 = 网格左 / 面板右）。
- `:1026` 有 active presentation 时取 `settings.verticalLayout`；`:1032`
  `viewerSash.setOrientation(verticalLayout ? SWT.VERTICAL : SWT.HORIZONTAL);`
- `:1544-1547` `toggleVerticalLayout()`：翻 `settings.verticalLayout` 后**立即** `setOrientation`。
- 入口一（竖条按钮）`:1082-1083`：先 `UIUtils.createEmptyLabel(panelSwitchFolder, 1, 1)`
  配 `GridData.FILL_VERTICAL` 把后续控件推到底部，再
  `VerticalButton.create(panelSwitchFolder, SWT.RIGHT | SWT.CHECK, getSite(), IResultSetCommands.CMD_TOGGLE_LAYOUT, false)`
  —— **CHECK 型**（带选中态）。
- 入口二（上下文菜单）`:1584` `ActionUtils.makeCommandContribution(site, CMD_TOGGLE_LAYOUT)`。
- 命令注册：`plugin.xml:312` id `org.jkiss.dbeaver.core.resultset.grid.toggleLayout`；
  `:367` 图标 `platform:/plugin/org.jkiss.dbeaver.ui/icons/misc/rotate_left.svg`；
  `:436` handler `ResultSetHandlerMain`。
- 分派：`handler/ResultSetHandlerMain.java:176-178` `case CMD_TOGGLE_LAYOUT: rsv.toggleVerticalLayout();`
- 持久化：`:1233` 读 `pSection.getBoolean("verticalLayout")`、`:1265` 写回
  `SETTINGS_SECTION_PRESENTATIONS`（与 `panelRatio` 同一段，按展示类型分组）。

#### 8.46.2 本仓实现口径与偏离

- **方向语义一致**：`vertical_layout = true` = 网格在上、面板满宽在下；
  `false` = 网格左、面板右。默认 `false`（同 `:1020`）。
- **容器换向**：普通视图里「主 pane + 右栏」的外层容器按方向 `h_flex()` / `v_flex()`
  （`items_stretch`）；左缘竖条与右缘竖条留在这一层、贯穿整高，不随方向移动
  （DBeaver 的 `panelSwitchFolder` 也在 SashForm 之外、纵贯整高，同构）。
- **拖拽手柄随方向换轴**：横排 `Axis::Horizontal` + `Leading`（右栏左边缘），
  竖排 `Axis::Vertical` + `Leading`（右栏顶边）；`on_drag_move` 取 x 或 y 位移，
  增量口径不变（`size - delta`，两方向都是「朝手柄方向即变小」）。
  keyed state 键按方向分开（`-drag-x` / `-drag-y`）：切换方向后上一帧另一轴的
  坐标若被当成基准会跳一格。
- **入口一致**：切换按钮挂在右缘竖条**底部**（页签图标之后用一个 `flex_1()` 间隔块
  推到底，等价 DBeaver 的 1×1 `FILL_VERTICAL` 空标签）；`toggled(vertical_layout)`
  表达当前方向（对应 `SWT.CHECK`）。图标用 `IconName::RotateCcw` —— 与 DBeaver
  `rotate_left.svg` 同为逆时针箭头。
- **偏离 1（高度不进偏好项）**：DBeaver 横竖共用同一个 `panelRatio`（千分比），
  所以换方向后比例仍成立；本仓两个方向各用 px 尺寸（宽度偏好 `result_panel.sidebar_width`
  是横向口径），若让竖排高度共用同一个值会串（320px 宽 ≈ 240px 高不是同一语义）。
  因此竖排高度 `right_sidebar_height` 只在面板内存里（切方向来回仍保留各自的值），
  尺寸与边界走 `crate::ui` 的高度三元组（默认 15rem / 下限 8rem / 上限 32rem）。
- **偏离 2（竖排不做开合补间）**：`EffectTransition` 只提供 `width(from, to)`，没有
  height 补间；方向切换本身也不是展开 / 收起动作（DBeaver 换向同样无动画）。
  横排保留原 160ms 宽度补间与拖拽期 1ms 直写。
- **偏离 3（方向状态不持久化）**：DBeaver 存 `presentations` 段（按展示类型）；
  本仓 `vertical_layout` 目前只在面板内存里（重启回默认横排）。要走持久化需先解决
  「按展示类型分组」的映射问题（同 §8.45.6 第一项）。

#### 8.46.3 落点（8 文件）

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/ui.rs` | `RESULT_SIDEBAR_RIGHT_HEIGHT` 15.0 / `_MIN_HEIGHT` 8.0 / `_MAX_HEIGHT` 32.0（f32，不新增裸 `px(`） |
| `panel/data.rs` | `vertical_layout: bool`（默认 false）+ `right_sidebar_height: Pixels` |
| `panel/lifecycle.rs` | 两字段初始化 |
| `panel/events.rs` | `resize_right_sidebar` 按方向改宽度/高度；`end_right_sidebar_resize` 竖排返回 `None`；新增 `toggle_vertical_layout`（翻状态 + 清拖拽标记） |
| `panel/sidebars.rs` | `clamp_right_sidebar_height` 纯函数；手柄按方向选 `Axis` 与位移轴、keyed state 键分开；`render_right_sidebar` 竖排改 `w_full + h(height) + border_t`（无补间）；`render_right_edge_bar` 底部加切换按钮 |
| `panel/render.rs` | 主 pane + 右栏外层容器按方向 `h_flex`/`v_flex` |
| `panel/mod.rs` / `panel/tests.rs` | 测试导出 `clamp_right_sidebar_height`；+2 测（高度夹取 / 切方向与竖排不落盘） |
| `editor/src/view/host/results/dispatch.rs` | 新增 `ToggleVerticalLayout` 分支 |

#### 8.46.4 验证

- `cargo check -p rds-result-panel` / `-p rds-editor` 通过（0 error / 0 warning）。
- 门禁：panel **256**（254 + 2）、workbench_shell **14**、ui_guard **4** 全绿。
- 两条新测：`right_sidebar_height_clamps_between_ui_limits`（240→200/280、越界夹到
  128/512、边界同向不反向）、`toggle_vertical_layout_flips_and_stops_persisting_widths`
  （默认横排 → 切竖排 → 拖 40px 只改高度不改宽度 → 松手 `None` → 切回横排宽度原封不动）。

#### 8.46.5 未做 / 待定

- `vertical_layout` 跨会话持久化（需先定 `Presentation` ↔ DBeaver descriptor 映射）。
- 竖排高度持久化（与宽度共用一个偏好项会串，需要独立偏好项或改用千分比口径）。
- 竖排下右缘竖条仍纵贯全高是否与 DBeaver 观感一致（真窗口目视待做）。
- 竖排时状态栏归属、`compare_mode` 下的方向切换（对比模式双栏结构，本批未接）。

### 8.47 保存值：值详情页「保存至文件…」（对齐 DBeaver `ContentValueManager.saveToFile`）

#### 8.47.1 DBeaver 侧事实（逐条可核）

值页工具条装配在 `ValueViewerPanel.fillToolBar`（`ValueViewerPanel.java:409-428`）：
`valueManager.contributeActions(...)` → `GROUP_ACTIONS_ADDITIONAL` 后追加引用值动作 →
**仅非只读时**追加 `SaveValueAction`（`:445`）。两条动作是**两件不同的事**，别混：

| 动作 | 位置 | 实际行为 |
| --- | --- | --- |
| 「保存至文件…」`model_jdbc_save_to_file_` | `ui/data/managers/ContentValueManager.java:86-99`（`contributeContentActions`） | `saveToFile(controller)`（`:291-334`）：**选路径 + 写盘** |
| `SaveValueAction` | `ValueViewerPanel.java:445`，command `…core.resultset.cell.save`（`ValueViewCommandHandler.java:34`，`:49-51` 与 `SMART_ENTER` 同走 `saveValue()`） | `saveValue()`（`:370-385`）= `extractEditorValue` → `previewController.updateValue(newValue, true)`：**把编辑器里的值写回结果集**，不是存文件 |

- 「保存至文件…」的触发条件：`controller.getValue() instanceof DBDContent` **且非 NULL**
  （`ContentValueManager.java:92`）——只对 LOB / LONG / BINARY 类值出现在值页工具条上；
  同一处还摆 "Open in external editor"（`:100-134`）。
- 文案：`ResultSetMessages_zh.properties:455 model_jdbc_save_to_file_ = 保存至文件...`
  （英文 `ResultSetMessages.properties:151 = Save to file ...`），图标 `UIIcon.SAVE_AS`。
- `saveToFile` 细节：`DialogUtils.selectFileForSave(shell, controller.getValueName())`
  （`:299`，**默认文件名取「值名」即列属性名**）→ 取消 `null` 即 `return`，不算错误（`:300-302`）
  → 文本内容（`ContentUtils.isTextContent`）按 **UTF-8** reader 落盘（`:307-314`），
  其余按二进制流直写（`:315-319`）→ 失败弹 `model_jdbc_could_not_save_content` /
  `model_jdbc_could_not_save_content_to_file_ + path`（`:326-329`）。
- `SaveValueAction` 另有「自动应用值」勾选项（`AS_DROP_DOWN_MENU`）——写回通道的配套。

#### 8.47.2 本仓口径与偏离

- **只做「保存至文件」这一支**：本仓结果集是只读预览，没有 `previewController.updateValue`
  那种写回通道（事务类改造不在范围内），故 `SaveValueAction` / 自动应用值不做。
- **一致点**：入口在值页工具条右段（`位置标记 + 复制 + 保存`）；文本原样按 UTF-8 写出
  （Rust `String` → `as_bytes()`，等价 `GeneralUtils.UTF8_ENCODING`）；二进制写真字节；
  用户取消不弹提示；失败走宿主消息行。
- **偏离 1（触发条件放宽）**：DBeaver 只对 `DBDContent` 摆「保存至文件」，文本值另有
  `TextViewDialog` 自己的保存入口；本仓没有那层对话框，故**任意格都摆保存按钮**，
  文本也能直接落盘。
- **偏离 2（默认文件名）**：DBeaver 用 `getValueName()`（列属性名）；本仓用
  `value_r{行}_c{列}.{扩展名}`（1 基行列号）。理由：列名可能含 `/ : *` 等文件系统非法字符
  与超长中文，清洗规则会引出一整套平台差异，不如不拼。
- **偏离 3（未做 external editor）**：`ContentValueManager.java:100-134` 的
  "Open in external editor" 不做（需外部进程拉起 + 临时目录管理）。
- **端口选型**：复用宿主既有 `SavePathPicker`（另存为 picker，只收 `(当前路径, 默认名)`），
  **不用** `ExportPathPicker` —— 后者签名带 `ExportFormat`，而值可以是 txt / json / png / bin，
  没有固定格式。两者都由 `crates/editor/src/ports/editor_files.rs:37 attach()` 用 `rfd` 注入。

**值形态 → 落盘内容**（`value_payload` 纯函数，与值查看器同源 `classify_value`）：

| 形态 | 字节 | 扩展名 |
| --- | --- | --- |
| `Image { format }` | `hex_to_bytes(值)` 还原真字节 | 格式名小写（`png` / `jpeg` / `gif` / `webp`） |
| `Hex` | `hex_to_bytes(值)` | `bin` |
| `Json` | 展示文本原文 | `json` |
| `Text` | 展示文本原文 | `txt` |

图片存下来是**可直接打开的图片**，不是一份 hex 文本。

#### 8.47.3 落点（5 文件）

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/ports.rs` | `ResultRequest::SaveValue`（紧接 `CopyValue`；注释写清两条 DBeaver 动作的区别）+ 文件末 `pub struct ValuePayload { pub bytes: Vec<u8>, pub default_name: String }` |
| `panel/tabs.rs` | 纯函数 `value_payload(kind, value, row, col) → ValuePayload`（放在 `hex_page_clamp` 之前）；`ResultPanel::selected_value_payload(&self, cx)`（`selection_pos` → `cell_text` → `column_kind`，`col == 0` 按文本）；`render_value_tab` 工具条右段 `vec![marker, copy, save]`，按钮 id `editor-result-value-save`（`ghost().xsmall()` + `IconName::Download`） |
| `panel/tests.rs` | 纯函数测试 `value_payload_extracts_bytes_and_extension_by_shape`（文本 / JSON / 二进制 / PNG 魔数四条）；工具条形态测试的 id 列表加入 `editor-result-value-save` |
| `editor/src/view/host/results/export.rs` | `save_selected_value(&mut self, cx)`：无 picker → 提示未接入；无选中格 → 提示先选格；`pick_save_path(None, default_name)` 返回 `None` = 用户取消直接 return；`std::fs::write` 成功 / 失败各一条状态消息 |
| `editor/src/view/host/results/dispatch.rs` | `ResultRequest::SaveValue => self.save_selected_value(cx)`（`CopyValue` 之后） |

#### 8.47.4 验证

- `cargo check -p rds-result-panel` / `-p rds-editor` 通过（0 error / 0 warning）。
- 门禁：panel **257**（256 + 1 条新测）、ui_guard **4**、settings **35**、workbench_shell **14**。
- 新测钉住四条形态映射与命名（`1234.5 → value_r1_c1.txt`、`{"a":1}`（row 4 / col 2 基 0）
  `→ value_r5_c2.json`、`0102FF → [01,02,FF] + .bin`、PNG 魔数 `89504E470D0A1A0A` → 真字节 + `.png`）。

#### 8.47.5 未做 / 待定

- "Open in external editor"（`ContentValueManager.java:100-134`）未做。
- 默认名不取列名（偏离 2），如需对齐 DBeaver 需先定列名清洗规则。
- 「写回结果集」（`SaveValueAction` + 自动应用值）本仓不支持，属结构性缺口。
- 320px 窄栏下三按钮（标记 / 复制 / 保存）是否挤，真窗口目视待做。

### 8.48 R3 第二步：多列分组下数值列键归一化（订正 §8.42.2 的错误推断）

#### 8.48.1 DBeaver 侧事实（逐行核对，同时订正 §8.42.2）

- 分组面板只管动作与配置：`GroupingPanel.java:164-176 fillToolBar` 依次挂排序、仅重复、
  编辑列、删除列、清空分组，再追加扩展点动作（`:178-184`）。
- **真正执行分组在 `GroupingResultsContainer.rebuildGrouping()`（`:211-272`）**：取分组列属性
  （`:235 getGroupAttributes`，元素是 `SQLGroupingAttribute`）、构造
  `SQLGroupingQueryGenerator`（`:237-245`，参数含方言 / 语法管理器 / 分组属性 / 聚合函数 /
  是否仅重复）、`dataContainer.setGroupingQuery(…)`（`:246`）交给内部第二个
  `ResultSetViewer` 执行（`:271 setDataFilter`）。
- `SQLGroupingQueryGenerator.generateGroupingQuery`（`org.jkiss.dbeaver.model.sql`，`:80`）
  拼 `"\nGROUP BY "`（`:166`），逐列 `groupAttributes.get(i).prepareSqlString(subqueryAlias)`
  （`:167-172`）；仅重复走 `HAVING COUNT(...) > 1`（`:174-179`）；默认排序按聚合别名下推
  （`GroupingResultsContainer.java:250-265`）。
- **结论**：DBeaver 的分组键就是**数据库侧的列值**，前端不参与键构造。§8.42.2 原写
  「DBeaver 的多列分组键也是按各列显示文本拼的」属未实证推断，本轮订正——数据库里
  `DOUBLE 2.0` 与 `2` 是同一个值，根本不存在拆组。

#### 8.48.2 本仓落法（对齐列值口径）

- **缺陷**：多列分组此前一律走 `column_values`（显示矩阵文本），数值列 `2.0` 与 `2` 拆成两组；
  单列早已通过 `number_group_key` 归一化，两条路径口径不一致。
- **改动**：把单列的原生键循环抽成私有 `column_group_keys(column) -> Option<Vec<String>>`；
  `group_by_for` 变为 `match self.column_group_keys(column)`（`Some` 走 `group_by_values(&keys)`，
  `None` 走 `column_values`），`group_by_columns_for` 多列分支逐列
  `self.column_group_keys(*column).unwrap_or_else(|| self.column_values(*column))`。
  其余（各列取最小长度、按行号拼元组、交给 `stats::group_by_tuples`）不变。

| 回退条件（返回 `None` → 该列用显示文本） | 理由 |
| --- | --- |
| 非 `FactKind::Int \| Float` 列 | 无原生数值可比；文本列本就按字符串等值 |
| 无事实档 / 表档 batches 已释放 | 取不到原生值（与 `column_stats_for` 同源） |
| 存在被删行 | 矩阵行与事实帧行号不再一一对应 |
| 该列出现 NULL 或取不到原生值 | 避免原生键与文本键混排，整列统一口径 |

- **行覆盖口径（本轮自查修掉的 bug）**：原生键必须与 `column_values` 一样**跳过本地行**
  （`let Some(data_ix) = self.data_row(view_ix) else { continue; }`）。初版用 `?` 整列返回
  `None`，会让原生键列与文本列行数不同 → 多列元组按行号错位。doc 注释已写明该约束：
  后续若改 `column_values` 的跳过规则，必须同步改 `column_group_keys`。

#### 8.48.3 落点

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/grid/text.rs` | `group_by_for` 改走 `column_group_keys`；新增私有 `column_group_keys`（含回退条件与行覆盖口径注释）；`group_by_columns_for` 多列分支逐列取原生键、两处 doc 注释重写 |
| `crates/result_panel/src/grid/tests.rs` | 新增 `group_by_columns_normalizes_numeric_keys_beside_text_columns` |

#### 8.48.4 验证

- `cargo check -p rds-result-panel` 通过（0 error / 0 warning）。
- `cargo test -p rds-result-panel --lib` = **258 passed / 0 failed**（257 + 1）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= **4 passed**。
- 新测钉住两条：Float64 `[2.0, 2.0, 3.0]` + 文本列，矩阵首列写 `"2"` / `"2.0"` / `"3"`
  → 多列分组得 `[("2","a")×2, ("3","b")×1]`；同一份矩阵**无事实档**仍得 3 组（老口径对照）。

#### 8.48.5 仍未做 / 口径边界

- 表档（batches 已释放）、含被删行、含 NULL 的数值列，在多列分组下仍按显示文本 → 仍可能
  把 `2` 与 `2.0` 拆组。这是本仓「拿不到原生值」的边界（DBeaver 无此问题：分组下推到数据库）。
- 分组页其余三件的处置见 §8.49.5（R3 结项）。

### 8.49 R3 第三步：分组页「清空分组列」+ R3 剩余项定性结项

#### 8.49.1 DBeaver 侧事实（逐行核对）

分组面板工具条动作全集见 `GroupingPanel.fillToolBar`（`:164-176`，按顺序）：

| DBeaver 动作 | 出处 | 本仓对应 |
| --- | --- | --- |
| `DefaultSortingAction` | `:165-167`（`MODE_FORCE_TEXT` 强制文字显示） | 排序按钮（按值 / 按计数） |
| `DuplicatesOnlyAction` | `:168` | 「仅重复」 |
| `EditColumnsAction` | `:170` | 「列」下拉勾选（配置对话框的降级形态，见 §8.42） |
| `DeleteColumnAction` | `:171` | 勾选取消（本仓无「当前选中分组列」概念，不单列动作） |
| `ClearGroupingAction` | `:173` | **本轮新增**「清空分组列」 |
| 扩展动作 | `:178-184`（`GroupingRegistry.getGroupingDescriptors()`） | 本仓无分组注册表，不做 |

`ClearGroupingAction`（`panel/grouping/action/ClearGroupingAction.java`）：文案
`ResultSetMessages.controls_resultset_grouping_clear`、图标 `UIIcon.CLEAN`（`:27-32`）；
`isEnabled()` = `!getGroupAttributes().isEmpty()`（`:34-37`）；`run()` =
`clearGrouping()` + `getController().updatePanelActions()`（`:40-43`）。
`GroupingResultsContainer.clearGrouping()`（`:198-209`）：`clearData(true)` →
`clearDataFilter(false)` → `resetHistory()` → `setGroupingQuery(null)` →
`setGroupingAttributes(null)` → 切 `EmptyPresentation` —— **回到空态**。

**「分组列顺序可调」的真实入口（同时结掉 §8.42.5 第三条遗留）**：
`GroupingResultsDecorator.registerDragAndDrop`（`:96-217`）把**分组结果网格的列头**注册成拖放目标，
`dropColumns`（`:161-215`）分两种语义：
- 拖入的是已在分组里的列（`attrBindingIndex >= 0`，`:170-176`）→ 记为 `movedBindingsIndexes`，
  落点列索引由 `getOverColumnIndex`（`:219-224`，`spreadsheet.getColumnIndex(event.x, event.y)`）
  给出，再 `container.getColumnsContainer().moveColumns(overColumnIndex, movedBindingsIndexes)`
  （`:191`）→ `GroupingColumnsContainer.moveColumns(int, List<Integer>)`（`:126`）——**这就是排序**；
- 拖入新列 → `addGroupingAttributes`（`:195`）；`DROP_COPY` 时先弹 `GroupingConfigDialog`
  （`:198-208`）确认聚合函数，再 `rebuildGrouping()`（`:210`）。

即：DBeaver 没有上移 / 下移按钮，顺序完全由「把列头拖到另一个位置」决定。

#### 8.49.2 本仓落法

| 层 | 落点 |
| --- | --- |
| 端口 | `crates/result_panel/src/ports.rs` `ResultRequest::ClearGroupColumns`（接在 `SetGroupDuplicatesOnly` 之后） |
| 面板状态 | `panel/events.rs::clear_group_columns`：空集合直接 return（对齐 `isEnabled()` 口径），否则清空 + `cx.notify()` |
| 视图 | `panel/tabs.rs::render_group_tab` 第 4 个动作：`Button::new("editor-result-group-clear")` + `IconName::CircleX` + `.disabled(空集合)` + tooltip「清空分组列」 |
| 宿主 | `crates/editor/src/view/host/results/dispatch.rs` 转发到面板 |

图标取自 `use gpui_kit::assets::IconName`（asset 全集），`icons/circle-x.svg` 在
`gpui-kit-assets-0.9.0/default-icons.txt`（内嵌的 106 个默认图标）里，能正常渲染；
`eraser.svg` / `broom.svg` 不在该清单，故未选。

#### 8.49.3 偏离

DBeaver 清空后回空态；本仓没有「无分组键」态（`group_columns` 为空 = 网格选中格 / 清单选中列
兜底），所以清空后仍显示单列分组结果。

#### 8.49.4 验证

- `cargo check -p rds-result-panel -p rds-editor` 通过（0 error / 0 warning）。
- `cargo test -p rds-result-panel --lib` = **259 passed / 0 failed**（258 + 1 新测
  `clear_group_columns_drops_explicit_selection`，钉「两次勾选按序累计 → 清空归零 → 空集合再清空幂等」）。
- 渲染用例 `right_panel_pages_share_the_toolbar_shape` 的分组页断言补 `editor-result-group-clear`。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= **4 passed**。

#### 8.49.5 R3 结项

原登记的三条「分组页遗留」全部有了实测结论，R3 到此收口：

| 原遗留项 | 实测结论 | 处置 |
| --- | --- | --- |
| 分组列集合跨会话持久化 | `GroupingPanel.loadSettings/saveSettings`（`:114-120`）空实现；只有「仅重复」「默认排序」走 preference（§8.42.5） | **不是 DBeaver 行为**，不做不算偏离 |
| 每组展开看行 | DBeaver 的分组结果是另一条 `GROUP BY` 查询的结果集（§8.48.1），不是原结果集的行树 | **不是对齐项**，DBeaver 本身也没有「组内明细」 |
| 分组列顺序可调 | 唯一入口是分组结果网格的列头拖放（§8.49.1） | 本仓 `DataTable` 无列头拖放入口，需自建 DnD 层；成本 / 收益不划算，**登记不做** |

### 8.50 工具栏 SQL 入口改「显示 SQL」+ 右栏六页竖向节奏对齐

用户口径（2026-11-17 原话）：「工具栏的 sql 区域减少或者设置成 显示SQL 点击都弹窗显示完整sql，
正常情况下显示 '显示SQL'」+「右侧栏是不是没有对齐呀」+「工具栏等是否还需要优化」。

#### 8.50.1 DBeaver 侧事实（逐行核对）

**SQL 入口的真实形态**：DBeaver 的「显示 SQL」不是一个独立控件，而是筛选条第 1 格里的
一段固定文案。

- 资源：`ResultSetMessages_zh.properties:746`
  `sql_editor_resultset_filter_panel_show_sql_label = 显示 SQL`。
- 取值：`ResultSetFilterPanel.getActiveSourceQuery(boolean forUI)`（`:573-585`）——
  当数据容器是表实体且当前没有筛选条件时，`forUI = true` 直接返回这段文案（`:577-578`），
  否则返回查询原文或容器名。
- 落点：`filterComposite` 是 `GridLayout(compactMode ? 4 : 5, false)`（`:171-182`，
  `marginHeight/marginWidth = 2`、`horizontalSpacing = verticalSpacing = 0`、
  `GridData(FILL_HORIZONTAL)`），第 1 列就是 `ActiveObjectPanel`（`:184`），
  第 2 列 `FilterExpandPanel`（`:186`），第 3 列筛选文本框，第 4 列 `ExecutePanel`（`:209`），
  第 5 列 `HistoryPanel`（`:214`）。
- 绘制：`ActiveObjectPanel`（内部类 `:911-1052`）先画对象图标再画文字——
  `getActiveObjectImage()`（`:551-571`：表实体→`DBIcon.TREE_TABLE`，其它→`DBIcon.SQL_TEXT`），
  绘制在 `paint`（`:1043-1049`）里「图标 x = 2 + 文字」；文字宽度受
  `maxTextLength = compactMode ? 16 : 24`（`:996`）封顶，
  `updateActiveQueryDisplayText`（`:496-501`）把 displayName 截到 200 字符。
  **标签宽度恒定，不随 SQL 变长**。
- 交互：`ActiveObjectPanel` 注册 `DoubleClickMouseAdapter`（`:924`）→ 弹 SQL 浮窗（`popup` Shell）。

**参考**：完整原文只在弹层里出现；工具栏本身不为 SQL 预留长度。

**右栏页内工具条**：DBeaver 每个结果面板自带一条
`ToolBarManager(SWT.HORIZONTAL | SWT.RIGHT | SWT.FLAT)`（`ResultSetViewer.java:1337-1374`），
动作由各面板 `contributeActions` 贡献；引用页的 `ReferencesPanel.contributeActions`
是空实现（`:156-158`），但**工具条本身仍存在**。分组页的工具条见
`GroupingPanel.fillToolBar:164-176`（§8.49.1 已核对）。

**右栏外壳**：`ResultSetViewer.java:321-341` 左右两条 `VerticalFolder`
（`VerticalFolder.java:42-53`：`GridLayout(1,true)`、`marginWidth = 2`、spacing 0）；
面板内容在下方 `viewerSash`（`:346-351`）的 `panelFolder`（`CTabFolder(FLAT|TOP)`，`:358-418`，
`marginW/H = 0`）+ 每个面板自己的工具条。

#### 8.50.2 本仓落法

| 改动 | 落点 |
| --- | --- |
| SQL 入口 = 筛选框第 1 格的「显示 SQL」 | `panel/render.rs` 新增 `sql_chip`（`Button::new("editor-result-sql-preview")` + `IconName::Table2` + `.label("显示 SQL")` + `.xsmall()`），插在筛选复合体 `.debug_selector("editor-result-filter")` 之后、展开钮之前；点击发 `ResultRequest::OpenSqlDialog`（弹窗沿用既有 `editor-result-sql-dialog`） |
| 工具栏左段只剩筛选复合体 | `grid/status.rs::render_toolbar` 删掉 `sql_preview` 形参与其 `.children(...)`；`grid/tests.rs` 的探针调用同步减一个 `None` |
| 右栏外壳由块级改 flex 列 | `panel/sidebars.rs::render_right_sidebar`：`div()` → `.v_flex()`；页签条行 `.flex_1()`（误用）→ `.flex_none()`；页内容包一层 `div().flex_1().min_h_0().v_flex()` |
| 列浏览器页头统一 | `render_browser_tab` 撤掉手写的 24px 无分隔线标题行（`min_h(1.5rem) + px_2 py_1`），改用 `panel_toolbar("editor-result-browser-toolbar", "列浏览器 · N 列", 批量动作)`；无选中列时**不生成**批量动作元素（空 `Toolbar` 仍占 gap）；外层 `.h_full()` → `.flex_1()` |
| 引用页补工具条 | `render_reference_tab`：`panel_toolbar("editor-result-reference-toolbar", "引用", 空)` + `sidebar_empty`；新增 `editor-result-reference-tab` 选择器 |
| 执行SQL页空态保留工具条 | `render_exec_sql_tab` 空态不再整页早退：工具条常驻、复制钮按 `exec_sql` 有无生成；外层 `.h_full()` → `.flex_1().min_h_0()` |
| 分组页空态保留工具条 | `render_group_tab`：无分组键不再整页早退（否则这一页连「分组列」下拉入口都没有了）；外壳补 `.v_flex()`（此前是块级 `div()`，`flex_1` 失效）；正文按 `cols.is_empty()` 二选一 |
| 值 / 计算空态保留工具条 | 新增 `panel_tabs::empty_page(toolbar_id, title, body, cx)`（「28px 工具条 + 空态正文」整页骨架），值页 / 计算页空态各用一次 |
| 公用件可见性 | `panel_toolbar` 由私有 `fn` 改 `pub(crate)`（`sidebars.rs` 要跨模块调） |

#### 8.50.3 关键机制：`div()` 默认不是 flex

右栏此前出现「长内容不滚动、空态不居中、切页正文起点在 24/28/0 三种头高之间跳」的共同根因是
外壳为块级 `div()`（gpui 里裸 `div()` 是 block 流，`flex_1` / `justify_center` 均无效）：
页内容高度由内容撑开，超出侧栏后被外层 `overflow_hidden` 裁掉。改成 `.v_flex()` 后
`flex_1` 才成立，六页统一为「24px 页签条 + 28px 页内工具条 + 剩余正文」。
（`vendor/gpui-base/src/styled.rs:105-107`：`h_flex()` = `flex().flex_row().items_center()`，
`v_flex()` 才是列；裸 `div()` 两者都不是。）

#### 8.50.4 偏离与未决

- **点击口径**：DBeaver 是**双击**标签开浮窗（`:924`），本仓按用户口径用单击。
- **图标**：DBeaver 按容器类型换表图标 / SQL 图标（`:551-571`），本仓固定 `Table2`（结果集恒为查询结果，暂不分类）。
- **页内工具条高度**：本仓 28px（对齐 vendor `Toolbar` XSmall `min_h_7`），DBeaver 的
  `ToolBarManager` 实测约 26px，两者差 2px，取舍见 §8.46（按 kit 组件档位走）。
- **右缘竖条宽**：本仓 `RESULT_VERTICAL_BAR_WIDTH = 2.25rem = 36px`；DBeaver
  `VerticalFolder(marginWidth = 2)` + `VerticalButton(BASE_MARGIN 2,2,2,2)`（`VerticalButton.java:44-45`）
  ≈ 24px。是否收窄**待拍板**（影响右栏三态观感与页签图标命中带宽）。
- 「执行SQL」页与「显示 SQL」弹窗信息重叠（都展示原文），是否保留该页**未决**。
- P2 三项（筛选历史下拉 `HistoryPanel`、筛选设置对话框、查找替换浮层）仍未开工。

#### 8.50.5 验证

- `cargo check -p rds-result-panel -p rds-editor` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **260 passed / 0 failed**；
  `right_panel_pages_share_the_toolbar_shape` 扩为七步（列浏览器工具条存在且高 28px →
  值详情三按钮 → 分组四动作 → 执行SQL 两件 → 引用页工具条高 28px →
  值/计算空态仍带工具条 → 分组空态仍有 `editor-result-group-columns` →
  设 `result_sql` 后 `editor-result-sql-preview` 可见）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= **4 passed**（`sidebars.rs` 裸 `px(` 计数未增）。
- 未做：真窗口目视（右栏 320px 窄栏下 28px 工具条是否挤、批量动作三按钮是否溢出）。

### 8.51 表格打磨第一批：列竖分隔线 + 单元格 6px 内距

**指令**：用户「我们需要继续打磨表格了」，在我给出的清单里选了先做「①选中色 + ②网格线」
（同一条绘制路径）。查证后 ① 被 vendor 挡住（见 8.51.4），本轮只落 ② 以及与之强耦合的
内距口径。

#### 8.51.1 DBeaver 侧事实（逐行核对 `dbeaver-ref`）

| 事实 | 出处 |
| --- | --- |
| 网格线**默认开**：`private boolean linesVisible = true;`，`setLinesVisible(...)` 可关 | `...\lightgrid\LightGrid.java:295`、`:1568-1570` |
| 画线在**所有单元格之后**统一画一遍（注释原文「Do not paint lines to grid cell to optimize performance」）：水平线每行一条 `y += itemHeight + 1; drawLine(startX + 1, y, startX + width, y)`；垂直线沿每个列边界 `x += column.getWidth(); drawLine(x, startY + 1, x, startY + height)` | `LightGrid.java:2400-2440`（水平 `:2418-2423`、垂直 `:2425-2439`） |
| 起点：表头可见时竖线从 `headerHeight - 1` 开始（**表头内部不画竖线**）、且 `x < 左固定区右边界` 的列跳过；冻结列另外用 `getLineSelectedColor()` 画 2px 分隔 | `:2413-2416`、`:2426-2437`、`:2442-2447` |
| 行距 = `itemHeight + 1`（那 1px 就是线），`itemHeight = fontMetrics.getHeight() + 3` | `:2397`、`:1871`、`:1065` |
| 线色 `lineColor = JFaceColors.getErrorBackground(display)`、选中线 `getErrorBorder(display)` | `:446-447`、`:1083-1089` |
| 单元格文字内距：`LEFT_MARGIN = 6`、`RIGHT_MARGIN = 6`、`TOP_MARGIN = 0`、`TEXT_TOP_MARGIN = 1`；左对齐起点 `x = LEFT_MARGIN`，右对齐起点 `bounds.x + bounds.width - (textSize.x + RIGHT_MARGIN)` | `...\lightgrid\GridCellRenderer.java:43-50`、`:158`、`:213`、`:250` |
| 表头文字内距同为 6px（`GridColumnRenderer.LEFT_MARGIN / RIGHT_MARGIN = 6`，另有 `TOP_MARGIN = 4`、`BOTTOM_MARGIN = 6`） | `...\lightgrid\GridColumnRenderer.java:36-41`、`:117-120` |

结论：DBeaver 的表格既有**行线**也有**列线**，且单元格/表头文字左右内距都是 **6px**。

#### 8.51.2 本仓落法

| 项 | 落点 |
| --- | --- |
| 列竖分隔线 | `grid/cell.rs::render_td` 两支（行号槽 + 数据格）都加 `.border_r(ui::HAIRLINE).border_color(theme.table_row_border)`；颜色与行线同一个主题槽（`TableState::render_table_row` 的行线就是 `table_row_border`），零裸色值 |
| 行线 | 组件本来就有：`vendor/gpui-component/src/table/state.rs:2274-2276` 给每行 `border_b_1().border_color(table_row_border)`，本轮不动 |
| 单元格内距 12px → 6px | `render_td` 的 `.px_2()`（8px）改成 `.px(rems(0.375))`（6px）；表头 `render_th` 两支补 `.px(rems(0.375))`（此前表头**没有**内距，列名与格内文字差一截） |
| 组件层内距置零 | `grid/table_impl.rs::cell_paddings()` 返回 `Edges { top/bottom: ui::RESULT_CELL_VERTICAL_PADDING(2px), left/right: Pixels::ZERO }`，四个列构造点（转置 字段 / 转置 值 / 行号槽 / 数据列）统一 `.paddings(cell_paddings())` |
| 列宽自适应同口径 | `ui::RESULT_FIT_CELL_PADDING` 16 → **12**（左右各 6）。组件 `fit_col` 加的是**列自己的 padding**（`table/state.rs:1299-1303`），置零后它加 0，delegate 的 12 就是屏幕上那 12 |

**为什么要同时改列 padding（关键机制）**：单元格元素是「组件列容器（带 padding）」的子元素。
若左右内距留在组件层，单元格盒子两边各被缩进 4px（叠上自身 8px 共 12px），那条
`border_r` 就画在**离列边界 6px** 的位置——看起来像格子里的竖线，不是列分隔线。
把左右置零、由单元格自己画内距后，单元格盒子撑满整列宽，`border_r` 正好落在列边界，
竖线才成立。这条几何由新增用例
`cells_stretch_to_the_column_boundary_for_grid_lines` 钉住（改前行号槽 40px / 数据列 120px，
改后 48 / 128）。

#### 8.51.3 守卫基线

`crates/ui_guard/tests/workspace_ui_contract.rs` 的 `result_panel/src/ui.rs` 裸 `px(` 计数
16→17→**18**：新增 `RESULT_CELL_VERTICAL_PADDING = px(2.)`（沿用组件 XSmall 档
`table_cell_padding()` 的上下原值，垂直位置不变）。已在 BASELINE 处写明理由，符合
「新增豁免必须同步上调并写理由」的口径。

#### 8.51.4 偏离与未决

- **线色偏离**：DBeaver 用 `JFaceColors.getErrorBackground(display)`（随系统主题的浅底），
  本仓取 `theme.table_row_border`——与行线同源，明暗主题都成立；若按名照抄会得到一个
  本仓主题里不存在的颜色。
- **① 选中色未落（被 vendor 锁死）**：`vendor/gpui-component/src/theme/schema.rs:1036-1067`
  的 `clamp_alpha(raw, color, background, max)` 把 `table_active` 的 alpha 上限压到 **0.2**
  （`list_active` 0.2 / `selection` 0.3），选中格的底色是 vendor 自己在内容**之上**叠
  `div().absolute().inset_0().bg(cx.theme().tokens.table_active)`（`table/state.rs:2327-2334`、
  `:2436-2446`）——**传不透明色也会被压到 20%**。DBeaver 的选中是实心浅蓝，本仓只能做到
  「近似 tint」；要真对齐必须改 vendor 一行上限（会让全应用 `table_active` 变实心），
  待用户拍板。
- **未目视**：竖线在 26px 行高 + 12px 字的密度下是否"过密"、线色在深色主题下的对比度、
  6px 内距对长中文列可读性的影响——真窗口目视已挂账（用户要求不得擅自截图/启动验证）。
- **行高仍 26px**：DBeaver `itemHeight = fontMetrics.getHeight() + 3`（≈19px）、行距 +1；
  本仓是组件 XSmall 档 26px。改行高会牵动可视行数、分页取数、几何用例，单列一项待拍板。

#### 8.51.5 验证

- `cargo check -p rds-result-panel -p rds-editor` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = 见 §2.41 真值（`grid/tests.rs` 新增 1 例）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= 4 passed（基线已同步上调并写明理由）。

### 8.52 表格打磨第二批：表头列边界线（并修正 §8.51 的竖线基准）

#### 8.52.1 DBeaver 事实（`GridColumnRenderer.paint`）

| 事实 | 出处 |
| --- | --- |
| 每个列头**逐列画两条线**：右边界竖线 + 底边横线（`drawLine(x+w-1, y, x+w-1, y+h-1)`、`drawLine(x, y+h-1, x+w-1, y+h-1)`） | `GridColumnRenderer.java:105-108` |
| 获焦列（`element == grid.getFocusColumn()`）改画**四条边**的框，色 `COLOR_WIDGET_NORMAL_SHADOW` | `:96-104` |
| 只读列额外在底部之上再画一条线（`getHeaderReadOnlyColor`） | `:110-115` |
| 线色 = `labelProvider.getHeaderBorder(element)`，与表头底边同色（不是数据区 `lines` 色） | `:106` |

底边本仓早就有：组件给表头行 `border_b_1().border_color(cx.theme().border)`
（`table/state.rs:2114-2115`）。缺的只有**竖直的那条**。

#### 8.52.2 本仓落法与 §8.51 的两处修正

| 项 | 落点 |
| --- | --- |
| 表头列边界线 | `grid/header.rs::render_th` 两支（早退支 + 带列头菜单的主支）都加 `.border_l(ui::HAIRLINE)`，色 `theme.colors.border`（先取到局部 `header_sep`，避免与后面的 `cx` 可变借用打架） |
| 跳过首列 | 两处都包 `.when(col_ix > 0, …)`：首列的左边界就是表格左边缘，不画（DBeaver `LightGrid.java:2426-2437` 同样跳过首个边界） |
| **修正 1：竖线改画左边** | §8.51 用的是 `.border_r`（画在格盒子右边界）。但表头格内容区右端被组件 `render_sort_icon` 占掉 20px（`state.rs:1910` 把图标当 `justify_between` 的第二个 child；图标 + 2px 内距），实测 128px 列的表头格只有 **108px** 宽 ⇒ 右边界线会落在列内侧 20px。改成左边后与列左边界重合，且表头 / 数据区用的是同一个基准 |
| **修正 2：行号槽不画线** | §8.51 里行号槽画右边界线。改左边界后，行号槽自身不再画线——它与首个数据列之间那条由数据格的 `border_l` 画（每处边界仍只有一条 1px 线） |

**为什么不是给表头右端「补 20px」**：那 20px 是组件排序图标槽（`SortAscending` /
`ChevronsUpDown`），不是我们画的；要在它上面补一个绝对定位的线，就得把一个真实的
布局尺寸（图标宽 + 内距）写死进我们的代码，属于对 vendor 内部几何的猜测。画左边不需要
任何偏移量。

> 2026-11-18 修正：这 20px 的**构成**已经查清了（`render_sort_icon` 的 `size_3()` 12px +
> `p(2px)` 内距 = 16px，加 `pr(offset_pr)` = 4px，共 20px，实测 128 列的表头格恰好
> 108px）。所以它不再是「猜内部几何」，而是一个可以从源码逐项加出来的常量
> `ui::RESULT_FIT_HEADER_CHROME`，**只用在列宽自适应**（§8.53）；画线仍走左边界。

#### 8.52.3 测试口径调整

- `cells_stretch_to_the_column_boundary_for_grid_lines` 追加表头断言：
  行号列表头宽 ≥ `RESULT_ROW_NUMBER_WIDTH - 4px`；数据列表头的 `left()` 比行号列表头
  右移 ≥ `RESULT_ROW_NUMBER_WIDTH - 1px`（= 表头竖线与数据区竖线同一条 x）。
  **不能用表头格的宽度当基准**（它是 108 而不是 128，见修正 1）。
- `header_column_menu_floats_and_leaves_the_name_its_full_width`：列名实测 104 → **95**
  ——新增的 1px 左边界线画在格内部，占掉 1px 内容宽。阈值 96 → **92**（「菜单回到流内」
  时只有 80，区分力不变），并在注释里写明这一档 1px 的来由。

#### 8.52.4 未做与未决

- **获焦列四边框**（DBeaver `:96-104`）未做：要画得知道「当前焦点列」，而
  `TableState::selected_cell()` 在 `TableDelegate::render_th` 里读不到——该回调执行时
  `TableState` 实体正被 `&mut` 借用（`cx.entity().read(cx)` 会撞「正在更新」）。
  可行路径是先在 delegate 侧缓存焦点列（由面板 `observe` 表格选择变化写入），属于接线活。
- **只读列标记**（`:110-115`）未做：本仓只有表级 `editable`，没有列级只读信息。
- **表头底边线色**：组件固定 `theme.border`，与本次竖线同源，未另行覆盖。
- 真窗口目视（表头线与数据线是否严丝合缝、深色主题下 1px 线是否可辨）仍挂账。

#### 8.52.5 验证

- `cargo check -p rds-result-panel -p rds-editor` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **261 passed / 0 failed**（本批不新增用例，
  复用 §8.51 的几何用例追加两条表头断言）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= **4 passed**（首轮因注释里写了 `p(px(2.))`
  被 `no_new_hardcoded_color_or_pixel` 误计，改成「图标 + 2px 内距」后归零；未动基线）。

### 8.53 表格打磨第三批：排序指示去重（交给组件图标槽）+ 自适应宽度补 20px

#### 8.53.1 取证：表头右端 20px 到底是什么

| 事实 | 出处 |
| --- | --- |
| `render_sort_icon` 返回一个 **20px 宽的可点控件**：图标 `size_3()`(12) + `p(px(2.))` 内距，`role = Button`、`aria_label = Table.SortBy`，带 hover / active 底色 | vendor `table/state.rs:1828-1872` |
| 图标按 `col_group.column.sort` 取 `SortAscending` / `SortDescending` / `ChevronsUpDown`；`Default` 档套 `opacity(0.5)` | `:1843-1846`、`:1867-1869` |
| 点击图标 → `perform_sort`；它**要求** `column.sort` 是 `Some`，否则直接 return；成功后把当前列置新值、其它列回 `Default`，再回调 delegate | `:1863-1865`、`:1358-1388` |
| 本仓早已接线：`table_impl.rs` 的 `column()` 对数据列 `.sort(self.sort_mark(index))`（`sort_mark` 恒返回 `ColumnSort`，从不给 `None`），delegate 的 `perform_sort` 把值转成 `apply_sort` + `cx.notify()` | `grid/table_impl.rs:208-230`、`:322`；`grid/view_rows.rs:505-511` |
| `render_th` 把我们的元素当 `h_flex().size_full().justify_between()` 的第一个 child，第二 child 就是该图标，内层再 `.pr(offset_pr)`（= 4 - 0 = 4px） | `:1878-1911`、`:1904-1909` |

DBeaver 侧：排序**只用图像**（`IMAGE_DESC` / `IMAGE_ASC` 三角，`ARROW_MARGIN = 6` 贴列头
右端），筛选用 `IMAGE_FILTER = DBeaverIcons.getImage(UIIcon.DROP_DOWN)`
（`GridColumnRenderer.java:40-53`），从不画文字箭头。

⇒ 我们在同一位置另画了一套 `↑` / `↓` 文字：**重复指示器**，而且是**不可点的假控件**
（眼睛看到能点，鼠标点不到）。本批删掉。

#### 8.53.2 落法

| 项 | 落点 |
| --- | --- |
| 删自绘排序箭头 | `grid/header.rs`：删 `sort_indicator` 计算与主行的 `.when_some(sort_indicator, …)` 子元素。排序指示只剩组件那一个（唯一、可点、状态由 `.sort()` 回灌） |
| 筛选指示换图标 | 同文件：`▽` 文字 → `Icon::new(IconName::Funnel).size_3().flex_none().text_color(muted)`。`size_3()` = 12px，与组件排序图标同尺寸，视觉同一档 |
| 自适应宽度 | `grid/fit.rs`：删掉 `↑`/`↓` 的测量（字形已不存在）；筛选改按 `ui::RESULT_FIT_HEADER_ICON`（12px 常量，图标量不出字形推进宽）；表头合计再加 `ui::RESULT_FIT_HEADER_CHROME`（20px，**仅数据列**——转置列与行号槽没有 `.sort(...)`，组件不留那个槽） |
| 常量入表 | `ui.rs` 新增 `RESULT_FIT_HEADER_ICON = px(12.)` 与 `RESULT_FIT_HEADER_CHROME = px(20.)`，注释写清 20px 的逐项构成 |

**这条同时是一个真 bug 修复**：`fit_col` 加给我们的是 `col.paddings`（`cell_paddings` =
左右 0），最终的列宽就是 `fit_column` 的回值。漏算这 20px ⇒ 双击自适应算出的列宽比
「列名完整显示」少 20px，列名照样被省略号吃掉（§8.52 修正 1 实测 128 列表头格只有 108，
差的正是这 20px）。

#### 8.53.3 测试

- 新增 `grid/tests.rs::autofit_reserves_the_component_sort_icon_slot`：未知类型列（无
  badge、无列筛选）、列名远长于单元格内容 ⇒ 断言
  `fit_column == measure(列名) + RESULT_FIT_HEADER_CHROME`，并先断言结果 > 80px
  （证明表头块宽确实压过单元格块宽，等式前提成立）。为此把 `fit::measure` 提升为
  `pub(crate)`，用例按**同一口径**（同字体 / 同字号 / 同换行处理）量列名。
  漏算 20px 时该断言直接失败（差值恰好是常量本身），不是「大概变宽了」的弱断言。
- 既有用例不受影响：`fit_column_follows_content_and_caps_the_sample` 的
  `narrow_w >= px(16.)`、`capped < px(100.)` 全是相对口径，加 20px 后仍成立。

#### 8.53.4 偏离与未决

- **未排序列会显示 50% 透明度的双箭头**（组件的 `ChevronsUpDown`），而 DBeaver 未排序列
  什么都不画。想「什么都不画」只有让 `.sort(None)`，但 `perform_sort` 在
  `column.sort == None` 时直接 return ⇒ 表头点击排序整条失效。二者只能取一，本批选
  「可点」，即保留组件的淡双箭头作为可点提示。要彻底贴合 DBeaver 必须改 vendor
  （让 `perform_sort` 接受 `None` 并自行补 `Default`），登记未做。
- 筛选图标形状与 DBeaver 的 `DROP_DOWN` 不同：DBeaver 那个是**下拉入口**的指示（它的
  列筛选是下拉面板），我们的筛选入口在列头菜单，指示的是「此列已筛选」这个状态，漏斗
  比空心三角更准确。这是有意偏离，不是漏做。
- 真窗口目视仍挂账（未排序列的淡双箭头是否过显、12px 漏斗在 22px 表头里是否看得清）。

#### 8.53.5 验证

- `cargo check -p rds-result-panel -p rds-editor` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **262 passed / 0 failed**（261 + 本批新增 1）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= **4 passed**（基线 18 → **20**，本批
  `ui.rs` 新增两个 `px(…)` 常量；新增常量是「尺寸单一来源」的推荐写法，不是违规）。

### 8.54 表格打磨第五批：整行选中铺色 + 网格事件订阅重挂（并修「订阅从未生效」）

#### 8.54.1 取证：DBeaver 的选中行是整行色，组件只画框

| 事实 | 出处 |
| --- | --- |
| `LightGrid` 选中行是**整行底色**：`getRowRenderer` 逐格取 `getCellBackground(element, selected)`，行头格也用同一口径 | `GridRowRenderer.java:55-70`、`:44` |
| vendor 的 `is_selected` 只喂两处：`need_render_border`（画 1px 行框，`:2255`）与 hover 抑制（`:2279-2285`），**不铺底色** | vendor `table/state.rs:2246-2285` |
| 本仓关了组件的窄行头（`.row_header(false)`，`grid/status.rs:453`）⇒「点 `#` 槽第二下升级整行选中」是选整行的**唯一入口** | 同文件 + vendor 文档 `:247-263` |

⇒ 选中行只有一圈 1px 边框、没有色块，26px 行高下几乎看不出来。本批补整行底色。

#### 8.54.2 结构性发现：面板侧的表格事件订阅**从未生效**

`lifecycle.rs::new` 里原本是：

```rust
let _grid_sub = cx.subscribe_in(&grid, window, |this, _table, event, window, cx| { …整套 match… });
```

`_grid_sub` 是**构造函数的局部变量**，`new` 返回即析构 ⇒ 订阅立刻解订；而且 `set_grids`
（`self.grid = grid`，`:295`）换了网格也不重挂。后果是面板那一整套处理在新网格上
**一次都没跑过**：

- `SelectCell` / `DoubleClickedCell` → `set_selection_pos` + `rebuild_value_tree`；
- `MoveColumn` → `push(ResultRequest::ColumnOrderChanged)`，**全仓只有这一处**，即
  「拖拽列序跨会话落库」是死功能；
- `ColumnWidthsChanged` → 面板侧取快照 + 推「该存了」（注释里写的「宿主那份订阅同样会记，
  同值幂等」正是以为两条都在跑）；
- `ClearSelection` / `SelectRow`。

生产上之所以看起来还能用，是因为编辑器宿主对**自己注入的网格**另有一条订阅
（`host/construct.rs`）覆盖了选格 / 双击 / 列宽 / 清除 / 整行选中的镜像；`MoveColumn`
只在面板侧处理，所以那条确实一直没落库。

实测证据（本批用例，先红后绿）：

```
左红：panel::tests::select_row_event_mirrors_into_the_delegate
  assertion `left == right` failed: 面板订阅没把 `SelectRow` 镜像进 delegate
    left: None          ← 面板处理没跑
   right: Some(1)
右绿：同一用例重挂订阅后通过
```

#### 8.54.3 落法

| 项 | 落点 |
| --- | --- |
| delegate 记选中行 | `grid/delegate.rs` 新增 `selected_row: Option<usize>` + `set_selected_row` / `selected_view_row`；`set_data` 里清空（换结果不该留着旧行号） |
| 整行铺色 | `grid/table_impl.rs::render_tr`：`selected_row == Some(row_ix)` 优先铺 `theme.tokens.table_active`（哨兵 `editor-result-row-selected`），否则奇数行斑马（`editor-result-row-odd`）——同一行只一档色 |
| 事件体抽成方法 | `panel/lifecycle.rs` 新增 `handle_grid_event(&mut self, event, window, cx)`，`new` 与 `set_grids` 共用同一份处理 |
| 句柄入库 | `panel/data.rs` 新增 `_grid_sub: Option<Subscription>`；`new` 里 `Some(cx.subscribe_in(…))` 进 `Self` |
| 换网格重挂 | `set_grids` 增 `window: &mut Window` 形参，末尾重建订阅（旧句柄随赋值解订，不留两条）；3 处调用点补 `window`（`host/construct.rs:171` + `panel/tests.rs` 两处） |
| 宿主镜像 | `host/construct.rs` 的 `SelectRow` 臂补 `delegate_mut().set_selected_row(Some(*row))`、`ClearSelection` 臂补 `None`（宿主这份才是生产渲染路径；面板那份修好后写同值，幂等） |

选中色为什么是淡色：`theme/schema.rs:1036-1067` 的 `clamp_alpha` 把 `table_active` 的
alpha 封顶 **0.2**，DBeaver 的整行选中是实心浅蓝。要实心必须改 vendor（全局影响所有表格），
本批不动，见 §8.54.6。

#### 8.54.4 测试

- `grid/tests.rs::selected_row_paints_a_whole_row_band`：delegate 预置 `Some(1)` →
  有 `row-selected` 哨兵、无 `row-odd` 哨兵；`set_data` 后 `selected_view_row()` 回 `None`。
- `panel/tests.rs::select_row_event_mirrors_into_the_delegate`：走生产入口
  `TableState::set_selected_row(1, cx)`（组件自己 emit `SelectRow`）→ 面板订阅 → delegate
  镜像 `Some(1)` + `row-selected` 哨兵在册。**这条就是 §8.54.2 的回归用例**：
  订阅不挂/挂错实体时 `left: None`。
  两个坑写进了注释：① `panel.read(app).grid().update(app, …)` 会双重借 `app`（E0502），
  先把 `Entity` clone 成本地句柄；② `debug_bounds` 的登记**跨帧累积**，本条先画过一帧
  未选中画面，所以「斑马纹缺席」只能由 `grid` 那条用例断言。

#### 8.54.5 验证

- `cargo check -p rds-result-panel -p rds-editor` = 0 error / 0 warning（39.12s）。
- `cargo test -p rds-result-panel --lib` = **264 passed / 0 failed**（263 + 本批新增 1）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= **4 passed**（基线仍 20）。
- `cargo test -p rds-editor --lib --no-run` = 通过（`set_grids` 形参变更的调用点全部编译）。

#### 8.54.6 偏离与未决

- 选中色只到 20% tint（`clamp_alpha` 封顶），未对齐 DBeaver 的实心浅蓝；走改 vendor 一行
  是全局影响，等拍板。
- `#` 槽仍不铺底色：DBeaver 行头每行铺 `HeaderBackground` + 右线 + 底线
  （`GridRowRenderer:55-70`）。铺了会和整行选中色冲突（`table_active` 半透明，单层 bg
  叠不出两层），需先定口径。
- 获焦列四边框（`GridColumnRenderer:96-104`）仍未做（需 delegate 镜像焦点列 + 焦点色
  token；`.border_1()` 会缩内容盒、vendor/base 无 outline 能力）。
- 真窗口目视挂账：20% tint 在浅色 / 深色下是否看得出「整行被选中」。

### 8.55 表格打磨第六批：筛选框订阅同样被丢弃（「边打边筛」从未生效）

#### 8.55.1 取证：`let _ = filter_sub;` 就是解订

`lifecycle.rs::new` 里原本：

```rust
let filter_sub = cx.subscribe_in(&filter_input, window, |this, state, event, window, cx| {
    if matches!(event, InputEvent::Change) { this.on_filter_input(state.read(cx).value().to_string(), cx); }
});
// 订阅句柄仅持有（防重复建立）
let _ = filter_sub;
```

`Subscription` 是「句柄掉了就解订」的守卫类型，`let _ = expr;` 把值移进通配模式后
**当场丢弃**（不是「持有」）。⇒ `InputEvent::Change` 一次也到不了 `on_filter_input`，
输入框里打字只改文本框自身，不触发 300ms 防抖、不应用筛选。与 §8.54.2 的网格订阅是
同一类错误（注释都写着「仅持有」，实现却是丢弃）。

为什么一直没被发现：编辑器侧用例 `filtering_waits_for_the_debounce`
（`crates/editor/src/view/tests/results_b.rs:88`）是**直接调 `on_filter_input`**，
绕过了订阅这一层——断言全绿，接线却是死的。

#### 8.55.2 落法

| 项 | 落点 |
| --- | --- |
| 句柄入库 | `panel/data.rs` 新增 `_filter_sub: Option<Subscription>`（注释写明为什么要持有） |
| 构造期持有 | `lifecycle.rs::new`：`let _filter_sub = Some(cx.subscribe_in(…))`，进 `Self { … _filter_sub }` |
| 注释纠偏 | 删掉「仅持有 / `let _ =`」的错误说法，写明 `Subscription` 掉即解订 |

#### 8.55.3 测试：走真输入框 + 真敲键，并做 A/B

新增 `panel/tests.rs::typing_into_the_filter_box_reaches_the_panel`：

1. `TextareaState::focus(window, cx)` 把焦点交给真筛选框（`base/state.rs:1804-1809`）；
2. `cx.simulate_keystrokes("1")` 真敲键；
3. 断言 `filter_text == "1"`（**这一条就证明订阅挂着**，不需要等防抖）；
4. `advance_clock(400ms)` + `run_until_parked()`，断言 `view_total() == 1`
   （harness 两行 `1 / 1234.5`、`2 / NULL` 里只有前者匹配）。

**A/B 实测**（把句柄临时退回丢弃写法再跑同一条用例）：

```
panicked at panel/tests.rs:1091:
敲进筛选框的字没到面板：`InputEvent::Change` 订阅没挂着（边打边筛失效）
  left: ""        ← 真敲键后 filter_text 仍为空
 right: "1"
```

⇒ 这条用例不是空转断言：修好绿、退回红。

#### 8.55.4 验证

- `cargo check -p rds-result-panel` = 0 error / 0 warning（18.04s）。
- `cargo test -p rds-result-panel --lib` = **265 passed / 0 failed**（264 + 本批新增 1）。

#### 8.55.5 遗留

- 编辑器侧 `filtering_waits_for_the_debounce` 仍走 `on_filter_input` 直调（它测的是
  「防抖语义」，不是接线）。接线归本条面板用例；两者分工，不重复。
- 同类隐患的检查口径：查 `Subscription` / `Task` 的「仅持有」写法是否真进了字段。
  本批在 `crates/result_panel/src` 逐条核过剩余 `let _ =`（`changes.rs` 两处忽略 `undo()`
  返回值、`lifecycle.rs` 的 `let _ = (from, to)` 消解未用参数），**没有第三处句柄丢弃**；
  两个真订阅（网格 / 筛选）现在都入库。

### 8.56 表格打磨第七批：`#` 行号槽铺表头底色（选中行留空）

#### 8.56.1 取证：DBeaver 的行头格是有底色的

| 事实 | 出处 |
| --- | --- |
| 每个行头格逐行取底：`getHeaderBackground(element, selected)` —— 选中行取选中色，其余取表头色；并画右线 + 底线 | `GridRowRenderer.java:55-70` |
| 行头文本取 `getHeaderForeground`；**获焦行**才换 `boldFont` | `:92`、`:96` |
| 行头前后内距 L/R = 4 | `:38` |

本仓的行号槽是**自有单元格**（`cell.rs::render_td` 的 `is_row_number` 分支，不是组件的
`render_row_header`——`grid/status.rs` 已 `.row_header(false)`），所以底色要画在这里。

#### 8.56.2 为什么不连选中行一起铺

整行选中色（§8.54）铺在**行容器**上，而 `table_active` 是**半透明** token
（theme json `#BC534033` / dark `#E8846F33`，且被 `clamp_alpha` 封顶 0.2）。
行号格若再铺一层同色，两层 alpha 相乘 ⇒ 那一小格比数据格明显深一档，看着像一块
「补丁」。所以口径定为：

| 行状态 | 行号槽 |
| --- | --- |
| 未选中 | 铺 `theme.tokens.table_head`（对齐 DBeaver 的行头条） |
| 整行选中 | **不铺**，让行容器的整行色直接透上来（与左侧数据格同一档） |
| 本地新增 / 复制行 | 保留 `result_local_row_background` 身份色（不叠表头色） |

#### 8.56.3 落法

| 项 | 落点 |
| --- | --- |
| 纯判据 | `grid/cell.rs` 新增 `row_number_paints_head(is_local, selected) -> bool` |
| delegate 包装 | 同文件 `impl ResultGridDelegate::row_number_paints_head(row_ix)`（把行身份与整行选中读成 bool），渲染只照着做 |
| 渲染接线 | `render_td` 行号槽分支：`.when(paints_head, |cell| cell.bg(theme.tokens.table_head))`；本地行底色分支保持在前（身份色优先） |

#### 8.56.4 测试与一处踩坑

新增 `grid/tests.rs::row_number_gutter_paints_the_header_background`：
三种组合的纯判据断言，**外加 delegate 真状态**（`set_selected_row(Some(0))` 后该行翻成
「不铺」、另一行照旧铺），覆盖判据 + 读取两段。

踩坑：最初想给行号格挂第二个 `debug_selector("editor-result-rowno-head")` 来断言接线，
实测这条用例红——`debug_selector` 在 gpui-fast 的 `div` 上是**单一槽位**
（`div.rs:910` `fn debug_selector(mut self, f)` 覆盖前值），而行号格原本已挂
`editor-result-rowno-{row_ix}`（表格密度口径的回归哨兵，不能删）。所以「颜色有没有铺上」
这一层 headless 不锁，只锁判据与状态；色值本身来自主题单一来源
`theme.tokens.table_head`（零裸色值红线内）。

#### 8.56.5 验证

- `cargo check -p rds-result-panel` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **266 passed / 0 failed**（265 + 本批新增 1）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= **4 passed**（基线仍 20）。

#### 8.56.6 未做 / 待目视

- 获焦行加粗（`GridRowRenderer.java:96` 的 `boldFont`）未做：行号槽是自绘单元格，
  加粗要选字重（`FontWeight`），且与「整行选中」是两套语义，先不动。
- 深色主题下 `table.head`（`#394248`）当整列底色是否偏重，需真窗口目视；若嫌重，
  改的是主题 token 而不是这里的分支。

### 8.57 事件流接线复核（§8.54 / §8.55 的副作用排查）

订阅修复把面板侧的事件处理从「从未执行」变成「真执行」，所以必须回头核一遍
「同一条路径会不会因此写两遍」。结论与依据：

| 路径 | 谁处理 | 会不会双写 |
| --- | --- | --- |
| `ColumnWidthsChanged` | 面板 `handle_grid_event` 推 `ResultRequest::ColumnWidthsChanged`（宿主 `dispatch.rs` 消费 → `save_session_now`）；宿主 `construct.rs:257-264` 的臂**只**写 delegate 宽度、**不**推请求 | **否**：落库只有面板这一路 ⇒ 每根手指拖一次宽只写一次 |
| `ColumnOrderChanged` | 只有面板推（`MoveColumn` 臂） | **否**：这条在本轮之前是死功能，现在首次真正生效 |
| `SelectCell` / `DoubleClickedCell` | 面板与宿主都收，各自更新各自持有的状态（面板选中镜像 / 宿主进入编辑门禁） | **否**：写的是不同字段，且同值幂等 |

另一条口径：本仓的竖分隔线（数据格 `cell.rs:335-336`、表头 `header.rs:243/478`）与
组件画的横分隔线（`vendor table/state.rs:2274-2276`）**取的是同一个 token**
`theme.tables.table_row_border`（表头侧走 `colors.border`，对应 DBeaver 的
`getHeaderBorder` 与网格线本就是两个函数，`GridColumnRenderer:105-108` /
`LightGrid:2400-2440`），所以网格线在纵横向是一套颜色，没有深浅接缝。

#### 验证（2026-11-18 复测）

- `cargo test -p rds-editor --lib` = **789 passed / 0 failed / 1 ignored**。
- `cargo test -p rds-editor --lib results` = 45 passed（结果集相关子集）。
- 单跑 `results_a::the_result_header_sorts_from_its_arrow` = ok（早先记录的
  `results_a.rs:290` panic 已不再复现，handoff §2 那三条基线失败口径作废）。

### 8.58 表格打磨第八批：获焦行的行号加粗

#### 8.58.1 取证

| 事实 | 出处 |
| --- | --- |
| 行头文本：`selected ? boldFont : normalFont` —— **只有当前行**的行号加粗 | `GridRowRenderer.java:96` |
| 列头反过来恒用 `grid.normalFont`，**不对获焦列加粗**（获焦列只画 1px 边框） | `GridColumnRenderer.java:84-87`、`:96-104` |

#### 8.58.2 口径

本仓「当前行」= 整行选中行（行模式下 `selected_row` 唯一非空，§8.54 已落地），
所以判据与行号槽底色**共用同一个来源**，不引入新状态：

- 整行选中行 → `font_weight(FontWeight::BOLD)`
- 其余行 → 继承单元格默认字重（不写死 normal，避免压掉上游字号档位）
- 与底色正交：未选中行铺 `table_head`、选中行留空且加粗（两者可同时成立，互不覆盖）

#### 8.58.3 落点与测试

| 项 | 落点 |
| --- | --- |
| 判据 | `grid/cell.rs` 新增 `impl ResultGridDelegate::row_number_is_bold(row_ix)`（渲染只调它） |
| 渲染 | 行号槽分支 `.when(is_focused_row, |cell| cell.font_weight(FontWeight::BOLD))`，紧跟底色分支 |
| 断言 | `row_number_gutter_paints_the_header_background` 追加两条：选中行 `true`、另一行 `false`（走 delegate 真状态，不碰颜色） |

未做：表头获焦列的字重——DBeaver 明确不加粗，本仓也不加（与 `:84-87` 一致）。

#### 8.58.4 验证

- `cargo check -p rds-result-panel` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **266 passed / 0 failed**（断言扩写，未新增用例）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= 4 passed。

### 8.59 内距收成单一来源（渲染 6px ↔ 自适应 12px）

#### 8.59.1 问题

同一份 6px 内距，代码里写着两遍：

| 侧 | 原来写的 | 性质 |
| --- | --- | --- |
| 渲染（`grid/cell.rs` 两处、`grid/header.rs` 两处） | `.px(rems(0.375))` | **随主题基准字号浮动**（16px 基准下才等于 6px） |
| 列宽自适应（`grid/fit.rs` → `ui::RESULT_FIT_CELL_PADDING`） | `px(12.)` | **固定物理像素** |

两套编码一旦分叉（改主题基准字号、或有人只动一边），双击列头分隔线算出的宽度就会
和实际渲染差一整个边距，列名照样被省略号吃掉——而这正是 §8.x 自适应那批要解决的问题，
会在无人察觉时复发。

#### 8.59.2 落法

- `ui.rs` 新增 **`RESULT_CELL_PADDING_X: Pixels = px(6.)`**：单元格 / 表头内容**单侧**
  左右内距，取值对齐 DBeaver `GridCellRenderer.LEFT_MARGIN / RIGHT_MARGIN = 6`
  （`:43-50`），用物理 px（数据网格行高本来就固定，内距不该随字号浮动）。
- 四处渲染字面量全部改成 `.px(ui::RESULT_CELL_PADDING_X)`；`RESULT_FIT_CELL_PADDING`
  保持 `px(12.)`，注释写明「必须 = 单侧 × 2」。
- `Pixels` 的字段是 `pub(crate)`，`const` 上下文里写不了乘法 ⇒ 用一条用例代那个写不出来的
  等式：`grid/tests.rs::cell_padding_single_source_matches_fit_padding` 断言
  `RESULT_FIT_CELL_PADDING == RESULT_CELL_PADDING_X * 2.`。
- 同步更新 `grid/fit.rs` 与 `grid/table_impl.rs` 里引用旧写法的注释。

#### 8.59.3 观感影响

**默认主题下零变化**（16px 基准 × 0.375 = 6px）。只在用户把基准字号调离 16 时不同——
此时内距保持 6px 不再缩放，与 DBeaver 的固定 6px 一致。

#### 8.59.4 门禁

`ui.rs` 多了一个 `px(6.)` 常量，`rds-ui-guard` 的 BASELINE 里
`crates/result_panel/src/ui.rs` 从 **20 → 21**（守门只拦新增，常量定义是推荐写法，
登记时写明理由）。

#### 8.59.5 验证

- `cargo check -p rds-result-panel` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **267 passed / 0 failed**（266 + 本批新增 1）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= 4 passed（基线 21 生效）。

### 8.60 筛选历史下拉（`HistoryPanel` 对齐）

#### 8.60.1 取证

| 事实 | 出处 |
| --- | --- |
| 筛选条 5 列 GridLayout，第 5 列是 `HistoryPanel` | `ResultSetFilterPanel.java:214`、`:174/:192/:209` 为前四列 |
| `HistoryPanel` 画一个向下的箭头，点击弹历史表 | 定义 `:1054`、`paintPanel` 箭头 `:1080-1091`、`showFilterHistoryPopup()` `:1093` |
| 历史表逐条列出已用过的筛选词，选中即回填 | `createFilterHistoryPanel` `:1207`；弹窗宽 = `filtersText + historyPanel + executePanel` `:1146-1174`，高 `Math.min(MAX_HISTORY_PANEL_HEIGHT, …)` `:1159-1161` |
| Enter 触发提交 `setCustomDataFilter()`（置 `e.doit = false`） | `:283-296` |
| 展开态（`filterExpanded`）里 Enter 是换行，**不提交** | `:288-290` |
| IME 组字中不提交 | `:291-293` |
| **↓** 打开历史（`ARROW_DOWN → showFilterHistoryPopup()`） | `:278-282` |
| 历史按 SQL 文本持久化（`loadFiltersHistory(query)`），`Manage Filters` 走 `ResultSetFilterDialog` | `:1121-1142` |

#### 8.60.2 口径（含三处有意偏离）

| 维度 | DBeaver | 本仓 | 理由 |
| --- | --- | --- | --- |
| 入口 | 箭头按钮 + **↓ 快捷键** | 箭头按钮（`Button` + `DropdownMenu`） | 快捷键要抢 Textarea 的方向键；本仓筛选框是多行 Textarea，↓ 在展开态是移动光标，抢键代价大于收益 |
| 持久化 | 按 SQL 文本落 filter manager | **会话级内存**（`filter_history: Vec<String>`） | 本仓没有那条持久化通道；历史是「本次会话内回到上一个口径」的便利，不是档案 |
| 上限 | 一张可滚动表，只封高度 | 最近 **8** 条（`FILTER_HISTORY_LIMIT`） | 下拉菜单不是滚动列表，长列表反而不好点 |

其余照抄：

- **只有显式提交入历史**——Enter 与 ▶ 执行（`RunFilterAsQuery`）。打字过程中的防抖状态
  不入（否则历史被 `a` / `ab` / `abc` 这类前缀灌满）。
- 同一个词重复提交 → **提到最前**，不追加一条。
- 展开态下历史按钮不渲染、Enter 不提交（与 `:288-290` 一致）。
- 从历史选一条 = 回填输入框 + 立即应用（**不等防抖**），并把它提到最前。

#### 8.60.3 落点

| 项 | 落点 |
| --- | --- |
| 状态 | `panel/data.rs` `filter_history: Vec<String>` + 模块常量 `FILTER_HISTORY_LIMIT = 8`（长注释写明 DBeaver 出处与「会话级内存」口径） |
| 初始化 | `panel/lifecycle.rs` `Self{ … }` 里 `filter_history: Vec::new()` |
| 记录 | `panel/events.rs` `note_filter_commit()`：trim → 空即返回 → 已在首位即返回 → `retain` 去重 → `insert(0)` → `truncate(limit)` |
| Enter 提交 | `panel/events.rs` `commit_filter(window, cx)`：读输入框值；词变了就定稿 + bump `filter_version`（作废在途防抖）+ `apply_filter`；词没变也算一次确认（只记历史）；展开态直接返回 |
| 重放 | `panel/events.rs` `apply_filter_from_history(value, window, cx)`：写 `filter_text` + bump 版本 + `InputState::set_value` 回填 + 记历史 + `apply_filter` |
| 订阅 | `panel/lifecycle.rs` 筛选框订阅由「只判 `Change`」改成 `match`：`Change → on_filter_input`、`PressEnter { .. } → commit_filter` |
| 渲染 | `panel/render.rs` 筛选行尾（`✕` 之后）`.when(!expanded && !history.is_empty())` 挂 `Button::new("editor-result-filter-history").ghost().xsmall().icon(IconName::ChevronDown).dropdown_menu_with_anchor(Anchor::BottomLeft, …)`；菜单项 `PopupMenuItem::new(label).on_click(→ queue.request(ResultRequest::ApplyFilter(text)))` |
| 端口 | `ports.rs` `ResultRequest::ApplyFilter(String)`（回填要 `&mut Window`，走 `ClearFilter` 同一条窗口转手路径） |
| 宿主 | `crates/editor/…/results/dispatch.rs`：`ApplyFilter` 分派臂 + `apply_filter_via_window`（`self.window.update` → 面板）；`RunFilterAsQuery` 臂先 `note_filter_commit()` 再重查 |

#### 8.60.4 测试

| 用例 | 钉住什么 |
| --- | --- |
| `panel::tests::commit_on_enter_records_filter_history` | 真敲键 `"1"` → 防抖未到点时 `view_total == 2` 且历史为空（打字不算提交）→ `simulate_keystrokes("enter")` → `view_total == 1`（立即生效，不等 300ms）且历史 `== ["1"]` |
| `panel::tests::filter_history_dedupes_caps_and_replays` | 灌 10 条 → 长度 == `FILTER_HISTORY_LIMIT`、首位是重复提交的那条、被挤掉的那条不在 → `apply_filter_from_history("k2")` → `filter_text`/输入框值都变成 `k2`、`view_total == 0`（证明确实应用到网格） |
| `panel::tests::rendered_ui_elements_survive`（步骤 1.6，追加） | **渲染接线**：空历史不摆入口 → 提交一条后 `editor-result-filter-history` 真的出现在元素树里 |

**测试坑（本批实测）**：在用例里直接改面板字段（`p.filter_text = …`）后，必须自己
`cx.notify()` 再 `window.draw(cx).clear(cx)`——宿主壳（`PanelHost`）只在实体变脏时重渲染，
不 notify 的话 `draw` 复用上一帧的树，`try_find` / `debug_bounds` 读到的是**旧帧**（曾据此
误判「按钮没渲染」；同一帧里 `editor-result-filter-run` 有、`clear`/`history` 没有就是这个症状）。

#### 8.60.5 验证

- `cargo check -p rds-result-panel` = 0 error / 0 warning；`cargo check -p rds-editor` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **269 passed / 0 failed**（267 + 本批新增 2；渲染接线断言并入既有用例）。
- `cargo test -p rds-editor --lib results` = **45 passed / 0 failed**（分派臂改动后的回归）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= 4 passed。

未做（P2 仍在册）：`Manage Filters` 设置对话框（`ResultSetFilterDialog`）、历史持久化（无通道）。

### 8.61 结果内查找栏（`FindReplaceOverlay` 的「查找」半边）

#### 8.61.1 来龙去脉

`ResultGridDelegate::find_next`（`grid/text.rs`，连同 `find_in_cell` / `cell_matches`）早就实现
并有单测（`finding_in_the_result_walks_the_visible_rows_and_wraps`），但它**没有任何 UI 接线**：
注释里写着「选中哪一格、要不要滚过去是宿主的决定」，宿主一直没做。本批把这条能力接上。

#### 8.61.2 取证

| 事实 | 出处 |
| --- | --- |
| 查找入口是筛选条工具条上的 **CHECK** 型 `openFindReplaceButton`（图标 `UIIcon.FIND_REPLACE`，命令 `EDIT_FIND_AND_REPLACE`）：选中打开查找、取消关闭 | `ResultSetFilterPanel.java:356-385` |
| 结果集的查找是**浮层** `ResultsetFindReplaceOverlay extends FindReplaceOverlay`，构造带 `targetControl` / `target` / `presentation` | `ResultsetFindReplaceOverlay.java:47`、`:58-66` |
| 查找目标实现 `canPerformFind()` / `getSelection()` / `setSelection(offset, len)` / `findAll` 等 | `SpreadsheetFindReplaceTarget.java:107`、`:113`、`:190` |
| 替换会**改单元格值**（`controller.updateCellValue(…)`，replaceAll 逐格处理并记 `processedCells`） | `SpreadsheetFindReplaceTarget.java:212-299` |

#### 8.61.3 落法（含三处有意收窄）

- **入口**：筛选行尾一个 `Button`（`toggled(self.find_open)` + `IconName::Search`），
  经 `ResultRequest::ToggleFind` 入队——渲染是纯读路径，不在 render 里改状态。
- **形态**：浮层 → **工具栏下方一行**（`render_find_bar`）：词框（XSmall / 无边框，与筛选框
  同档）+「N 个匹配」+ 上一处 / 下一处 / ✕。没有做绝对定位浮层：浮层要覆盖网格，会把网格
  行/列几何与格内高亮一起绑进来；而「找出 + 选中该格」已经完成读取那一半。
- **定位**：命中 = `grid.update(set_selected_cell(row, col) + scroll_to_row + scroll_to_col
  + refresh)`。`ResultMatch.col` 是**视图列**（含 `#` 槽），直接喂 `set_selected_cell` 不必折算。
- **交互**：输入框 `Change` → 重算命中数并定位第一处；`PressEnter` → 下一处（DBeaver 查找框
  回车即 findNext）；`✕` → 关闭（**保留词**，与 DBeaver 浮层 close 一致）。
- **纯函数新增**：`count_find_matches(needle)`（命中总数）与 `find_prev(needle, before)`
  （反向走法 + 回卷），口径与 `find_next` 完全对称（大小写不敏感子串、只看看得见的行、
  `#` 槽不算、被删行不算）。

三处**有意收窄**（都在册）：① 替换不做（DBeaver 走 `replaceSelection` 改单元格值；本仓
值编辑走「待提交」通道，批量替换另开一批）；② 正则 / 大小写 / 全词三个选项没做（本仓
匹配口径与本地筛选共用 `find_in_cell`，一份口径优先）；③ 格内区间高亮没做（只选中整格，
`ResultMatch.range` 已备好，待高亮通道）。

#### 8.61.4 落点

| 项 | 落点 |
| --- | --- |
| 纯逻辑 | `grid/text.rs`：`count_find_matches` + `find_prev`（紧跟 `find_next`） |
| 状态 | `panel/data.rs`：`find_open` / `find_input` / `_find_sub` / `find_query` / `find_hits` / `find_at`（存视图列，长注释写明为什么不是数据列） |
| 构造 | `panel/lifecycle.rs`：`InputState`（占位「在结果里查找…」）+ 订阅（`Change` → `find_query_changed`、`PressEnter` → `find_step(true)`）；句柄存 `_find_sub` |
| 事件 | `panel/events.rs`：`toggle_find(window, cx)`（打开时焦点交给查找框）、`find_query_changed`、`find_step(forward, cx)`、`close_find`、`find_state()` 只读快照 |
| 渲染 | `panel/render.rs`：筛选行尾查找钮；`render_find_bar(cx)` 挂在工具栏与展开条之后的 `find_open.then(...)` |
| 端口 | `ports.rs`：`ResultRequest::{ToggleFind, FindNext, FindPrev, CloseFind}` |
| 宿主 | `crates/editor/…/results/dispatch.rs`：四条分派臂 + `toggle_find_via_window`（打开要窗口句柄给焦点） |

#### 8.61.5 测试

| 用例 | 钉住什么 |
| --- | --- |
| `grid::tests::finding_backwards_and_counting_matches_mirror_forward` | 计数（`orders` 命中 2、空词 0、筛后看不见的行不计）+ 反向走法（末尾起点 / 往前往回卷）+ 无命中为 `None` |
| `panel::tests::find_bar_locates_matches_and_steps_through_them` | 走**真订阅 + 真敲键**：打开 → 敲 `1` → 命中 2 且 `find_at` 与 `grid.selected_cell()` 都是 `(0,1)`（证明网格真被选中）→ 下一处 `(0,2)` → 再下一处回卷 `(0,1)` → 上一处 `(0,2)` → 敲 `zzz` 计数 0 → 关闭后状态回落但**词保留** |

#### 8.61.6 验证

- `cargo check -p rds-result-panel` / `-p rds-editor` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **271 passed / 0 failed**（269 + 本批新增 2）。
- `cargo test -p rds-editor --lib results` = **45 passed / 0 failed**（新分派臂后的回归）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= 4 passed。

### 8.62 工具栏 / 侧栏结构对齐（2026-11-18）：记录模式沉底 + 清除钮常驻

重心转到「工具栏 + 结果集左右侧栏」后的第一批：两处**结构 / 行为**对齐（不碰配色与密度，
目视部分由用户统一处理）。

#### 8.62.1 左缘竖条 = 两段式（展示组在上、记录模式沉底）

取证（`ResultSetViewer.java`，`buildResultSetViewer`）：

| 事实 | 出处 |
| --- | --- |
| 左缘是 `new VerticalFolder(mainPanel, SWT.LEFT)` + `FILL_VERTICAL` | `:321-322` |
| 每个 presentation 一个 `VerticalButton(SWT.LEFT \| SWT.RADIO)`，`setData(pd)`、`setImage(icon)`、`setText(label)`、tooltip = description，选中者 `setSelection` | `:948-962` |
| 弹簧 = `createEmptyLabel(presentationSwitchFolder, 1, 1)` + `FILL_VERTICAL` | `:963` |
| **记录模式按钮在弹簧之后**：`recordModeButton = new VerticalButton(..., SWT.LEFT \| SWT.CHECK)` + `ToggleModeAction` | `:964-965` |

本仓落法：`render_vertical_bar` 里把 `grid_btn` / `text_btn` 留在顶部（RADIO 语义由 `toggled`
表达），中间插 `div().flex_1().min_h_0()` 当弹簧，`record_btn`（转置）沉到底部，记录视图的
上一行 / 下一行导航紧跟其下（本仓独有的增强，DBeaver 左栏没有行导航）。同时给竖条补
`pb_2()`，与顶部 `pt_2()` 对称，沉底按钮不贴边。

#### 8.62.2 筛选清除钮 = 常驻 + 空词禁用

取证（`ResultSetFilterPanel.java`）：`filtersClearButton` 建出来就 `setEnabled(false)`
（`:319-327`），筛选生效后才由 `enablePanelControls` 打开（`:449-454`）；点击体 =
`viewer.clearDataFilter(true)`（`:1550`），箭头菜单 = 清筛选设置 / 显示隐藏列 / 重置颜色 /
重置 transformers / 重置 pins / 全部重置（`:1525-1545`）。

本仓此前是「有词才出现」，按钮随打字在行尾左右跳一格。改为**常在**、`disabled(空词)`。
箭头菜单（DBeaver 侧那 6 项）按「能力没有就不摆」只合并了本仓真有落点的两条，
见 §8.64。

#### 8.62.3 测试

| 用例 | 钉住什么 |
| --- | --- |
| `panel::tests::left_bar_puts_presentation_group_on_top_and_record_mode_at_the_bottom` | 量**真实像素**：网格在文本之上；记录按钮与文本按钮相距 > 60px（无弹簧时只差 20~24px）；未进记录视图时不摆行导航 |
| `panel::tests::filter_clear_button_is_always_there_and_gated_by_the_filter_text` | 空词：按钮**在位**且 a11y `disabled = Some(true)`；有词：仍在原位、不再是禁用态（实测可用态不暴露 disabled 位，断言用 `assert_ne!(…, Some(true))`） |

#### 8.62.4 验证

- `cargo check -p rds-result-panel` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **273 passed / 0 failed**（271 + 本批 2）。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= 4 passed。

### 8.63 筛选条「状态历史」后退 / 前进（2026-11-18 已实现主体）

DBeaver 筛选条工具条上有一对浏览器式 **后退 / 前进** 按钮（`historyBackButton` /
`historyForwardButton`，`SWT.DROP_DOWN`，`ResultSetFilterPanel.java:343-351`），
本仓此前完全没有这个能力。

#### 8.63.1 DBeaver 取证

| 事实 | 出处 |
| --- | --- |
| 历史项 = `HistoryStateItem(dataContainer, filter, rowNumber)` | `ResultSetViewer.java:5444-5455` |
| 入栈口径 `setNewState(container, filter)`：**同一 (container, filter) 已存在 → 直接跳到那一条**（去重、不追加）；否则**截断当前位置之后的所有项**再追加，位置 = 末位 | `:2419-2442` |
| 导航 = `navigateHistory(position)`：按该状态的容器 / 筛选 / 行号**重跑数据泵**（`runDataPump(...)`） | `:4050-4063` |
| 启用态：后退仅当 `position > 0`；前进仅当 `position < size - 1` | `:456-475` |
| tooltip = 目标状态 `describeState()` + 快捷键描述 | `:459-471` |
| 箭头菜单 = 该方向所有可达状态，标签取 `describeState()`；点选 = `navigateHistory(该位置)` | `:1456-1481` |
| `describeState()` = 容器名 + `[条件 SQL]`，超过 50 字符截断加 `...` | `:5457-5473` |

#### 8.63.2 本仓口径（①：一份状态 = 一份结果 SQL）

本仓重查通道只有一条：`execute(ExecTarget::Statement(sql), ResultPlacement::Replace, cx)`
（「筛选框运行」`run_filter_as_query` 就走它）。所以「需要重跑数据泵的状态」直接等价于
**该结果集 SQL**，「原 SQL / 下发筛选 / 排序」三者的差异都体现为 SQL 文本差异，
不需要额外的快照通道。落点：

| 位置 | 内容 |
| --- | --- |
| `panel/data.rs` | `state_history: Vec<String>`（状态 SQL 栈）+ `history_pos: usize`（当前位置） |
| `panel/lifecycle.rs::sync` | `self.result_sql = input.sql;` 之后调 `note_state_sql()`——每份同步进来的结果都记一条 |
| `panel/events.rs::note_state_sql` | 去重（同一 SQL 只挪位置）、从中间走新路时 `truncate(pos + 1)` 作废前向、空 SQL / 无 SQL 不入栈 |
| `panel/events.rs` | `history_can_back` / `history_can_forward`（启用态）、`history_target(delta) -> Option<(下标, SQL)>`、`set_history_pos`、`history_hint`（tooltip） |
| `panel/events.rs::state_label` | `describeState` 等价：空白折叠成单空格 + 50 字符截断补 `…`（纯函数，可单测） |
| `ports.rs` | `ResultRequest::NavigateHistory(i32)`（`-1` 后退 / `+1` 前进） |
| `host/results/dispatch.rs` | 读 `history_target(delta)` → `set_history_pos(pos)` → `execute(Statement(sql), Replace)`（与「筛选框运行」同一条通道） |
| `panel/render.rs` | 筛选行在「清除钮 / 历史筛选下拉」之后、「查找开关」之前插一对 ghost xsmall 图标钮（`IconName::ChevronLeft` / `ChevronRight`），`disabled(!can_back/forward)`，tooltip = `history_hint` |

为什么位置不放在宿主：镜像是纯面板视图态（与右栏宽度、转置同类），面板 render 才能判断
启用态；宿主只负责「执行」这件事。

`sync` 会把重跑回来的同一 SQL 再记一次 —— `note_state_sql` 的去重正好把位置落回那一条，
所以「后退 → 宿主重查 → sync」不会把历史搞乱（这是口径 ① 能成立的关键）。

#### 8.63.3 分裂按钮的箭头菜单（2026-11-18 补齐）

DBeaver 的这对按钮是 `SWT.DROP_DOWN`：**主体点击走一格**，右侧 caret 弹菜单列出
**该方向全部可达状态**（`ResultSetViewer.java:1456-1481`），点哪条就
`navigateHistory(那条)`。本仓用组件的 `menu::DropdownButton`（分裂按钮：左半是带图标的
主体按钮，右半是 caret，ghost 变体下两半连成一个控件）：

- 左半 = `Button::new("editor-result-history-back-step"/"…-forward-step")`（ghost xsmall、
  `ChevronLeft`/`ChevronRight`、tooltip = 目标状态、点击发 `NavigateHistory(±1)`）；
  整个分裂按钮 `.disabled(!can_back / !can_forward)`（DBeaver 无该方向状态时整项禁用）。
- 右半菜单条目由 `history_menu_items(delta)` 给出：后退方向 = 当前位置之前的全部项、
  前进方向 = 之后的全部项，两个方向都**近的在前**；条目文案 = `state_label(sql)`；
  点击发新增的 `NavigateHistoryTo(usize)`（不限于走一格）。
- `menu_accessibility_label("后退历史" / "前进历史")`：caret 半块本身没有可见文案，
  不留 a11y 空白名。

#### 8.63.4 收窄（在册）

- **状态粒度**只到 SQL：本地视图层的东西（筛选词、转置、列布局、行位置）不入历史。
  DBeaver 的 `HistoryStateItem` 里含 `rowNumber`，本仓暂不还原行位置（回退后停在首行）。
- `pushdown_filter`（下发源库）产生的是**新结果集**（`ResultPlacement::NewSet`），
  它的状态记在新面板自己的历史里；原结果集那一份历史不受影响。
- 菜单条目只显示状态描述，不显示 DBeaver 那样的「navigate backward to …」前缀文案
  与快捷键提示。

#### 8.63.5 测试与验证

| 用例 | 钉住什么 |
| --- | --- |
| `panel::tests::state_history_dedupes_steps_and_drops_the_forward_branch` | 纯状态机：空 SQL / 空白 SQL 不入栈；A→B→C 后位置在末位（能退不能进）；`history_target(-1)` 指向 B、末位无前向；落位 B 后两向都可走；从中间走新路 D → C 被截断；重复状态只挪位置不追加 |
| `panel::tests::history_buttons_sit_in_the_filter_row_and_gate_on_position` | 渲染与菜单：空历史时两主体钮**在位且禁用**、两方向菜单都空；灌两条后后退钮可用、前进钮仍禁用（末位）；后退菜单 = `[(0, "select 1")]`（近的在前）、前进菜单空；`history_hint` = 目标 SQL；`state_label` 折叠空白 + 50 字符截断补 `…` |

- `cargo check -p rds-result-panel -p rds-editor` = 0 error / 0 warning。
- `cargo test -p rds-result-panel --lib` = **276 passed / 0 failed**（273 + §8.63 两条 + §8.64 一条）。
- `cargo test -p rds-editor --lib results` = **45 passed / 0 failed**。
- `cargo test -p rds-ui-guard`（不带 `--lib`）= 4 passed。

### 8.64 筛选条 ✕ 的箭头菜单（EraseItemListener 的本仓子集，2026-11-18）

DBeaver 的 `filtersClearButton` 是 `SWT.DROP_DOWN`：本体 `clearDataFilter(true)`
（`ResultSetFilterPanel.java:1550`），右侧箭头弹 `EraseItemListener` 菜单
（`ResultSetViewer.java:1525-1545`），条目为：清筛选设置 / 显示隐藏列 / 重置颜色 /
重置 transformers / 重置 pins / 全部重置。

本仓按既有口径「**能力没有就不摆**」只合并真有落点的两条，其余一律不摆（不摆假项、
不摆点了没反应的项）：

| 条目 | 本仓落点 | 出现条件 |
| --- | --- | --- |
| 清除筛选词 | `ResultRequest::ClearFilter`（与 ✕ 本体同一条通道） | 有筛选词 |
| 显示全部列 | 新增 `ResultRequest::ShowAllColumns` → 宿主 `delegate.show_all_columns()` + 网格 refresh + `snapshot_column_layout`（与列头 / 右键「显示全部列」同一能力、同一落盘路径） | 有隐藏列（`column_hidden` 非空） |

- 条目清单抽成纯函数 `erase_menu_labels(filter_text, hidden_columns)`
  （`panel/events.rs`）：`PopupMenu` 内部不可读，菜单长什么样只能靠纯函数钉。
- 形态：`DropdownButton` 分裂按钮，左半是原来那颗 `editor-result-filter-clear`
  （ghost xsmall、`✕`、`disabled(空词)`），右半是 caret；`menu_accessibility_label("清除选项")`。
- 两条都不可用时箭头**仍常在**（宽度不跳），菜单里摆一条禁用说明「暂无可清除项」——
  DBeaver 那边同类情况是条目置灰，而不是整项消失。
- 测试：`panel::tests::erase_menu_lists_only_the_entries_this_repo_can_honour`（空词+无隐藏
  → 空；有词 → 一条；有隐藏 → 一条；两者 → 两条且清除在前）。

### 8.65 右缘竖条顶部的面板总开关（2026-11-18）

DBeaver 右缘竖条（`panelSwitchFolder`，`VerticalFolder(SWT.RIGHT)`）的**第一颗**按钮就是
面板总开关，下面是逐面板按钮，再下面弹簧、最底部布局开关
（`ResultSetViewer.java:1038-1090`）：

| 事实 | 出处 |
| --- | --- |
| 总开关 `panelsButton`：`SWT.RIGHT \| SWT.CHECK`、`UIIcon.PANEL_CUSTOMIZE`、tooltip `CMD_TOGGLE_PANELS`、checked = `panelsVisible` | `:1038-1056` |
| 点击体：`showPanels(!isPanelsVisible(), true, true)` + 回写 checked + `updatePanelsButtons()` | `:1041-1055` |
| 每个面板一颗 `VerticalButton(SWT.RIGHT \| SWT.CHECK)`，`verticalIndent = 2`，checked = `panelsVisible && isPanelVisible(id)` | `:1058-1078` |
| 弹簧 `createEmptyLabel(1,1)` + 底部 `CMD_TOGGLE_LAYOUT` | `:1080-1083` |

本仓此前只有状态栏右端的三态开关（收起 / 窄展开 / 宽展开），竖条顶部**缺这颗总开关**，
是结构差异。本批补齐：

- 面板 `events.rs` 新增 `set_panels_visible(bool, cx)` 与访问器 `panels_visible()`。
  定义：**面板可见 = `edge_bar_visible && show_value_sidebar`**（竖条 + 右栏内容都在）；
  关 = 整条收起（与状态栏三态开关的收起态同一定义）。
- `ports.rs` 新增 `ResultRequest::SetPanelsVisible(bool)`；宿主臂直接转发面板方法。
- `render_right_edge_bar` 顶端插 `editor-result-edge-panels`（ghost small、图标随状态取
  `PanelRight` / `PanelRightClose`、`toggled(panels_visible)`、tooltip 报「收起 / 展开结果面板」）。
- **两颗开关并存**：状态栏那颗三态开关保留（DBeaver 的 `CMD_TOGGLE_PANELS` 同时也是菜单
  命令，两边入口本就并存）；竖条关掉后整条消失，重新打开走状态栏那颗
  ——DBeaver 关掉后竖条也消失，重新打开同样走菜单命令，处境一致。
- `set_panels_visible(true)` 幂等：已可见则不重播补间（不动 `right_sidebar_epoch`）。

**用例**：`panel::tests::right_edge_bar_carries_the_master_panels_toggle`
（按钮在竖条里 → `set_panels_visible(false)` 后不可见 → `true` 恢复 → 再 true 幂等）。
测试坑：竖条挂载条件是 `edge_bar_visible && result_can_copy`，`panel_harness` 默认
`result_can_copy = false`，要先置真再出帧，否则 `try_find` 必然取不到。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0/0；
`cargo test -p rds-result-panel --lib` = **277 passed**；
`cargo test -p rds-editor --lib results` = 45 passed；`cargo test -p rds-ui-guard` = 4 passed。

### 8.66 底部动作条按 `editCmds` / `navCmds` 归位（补 `row.next`，2026-11-18）

DBeaver 结果集底部动作条（`ResultSetViewer` 的 `statusBar`）由**三个**状态子贡献点装配，
`plugin.xml:1407-1522` 原文见 `.rds/tmp/db-status-toolbar.txt`：

| 贡献点 | 顺序（含分隔） |
| --- | --- |
| `toolbar:…resultset.status.editCmds` | `applyChanges`（`style="pulldown"` + `FORCE_TEXT`）→ `rejectChanges`（`FORCE_TEXT`）→ `undo` → `redo` ‖ **`rowCmds.separator`（竖直分隔）** ‖ `row.edit` → `row.add` → `row.copy` → `row.delete` |
| `toolbar:…resultset.status.navCmds` | `row.first` → `row.previous` → `row.next` → `row.last` → `fetch.page` → `fetch.all`（组前有分隔，组内无） |
| `toolbar:…resultset.status.exportCmd` | 分隔 → `export`（`pulldown`，带 `exportWithParameter=true`） |

本仓此前的三处结构差（①②③ 是顺序 / 缺失，都不是配色或密度）：

| # | 旧状 | 现口径 |
| --- | --- | --- |
| ① | 写操作段是「保存 · 取消 · 修改 · 新增行 · 复制行 · 删除行 · 撤销 · 重做」，即行级动作排在撤销 / 重做**之前**，且变更集动作与行级动作之间没有分隔 | 改为 `[提交 · 回滚 · 撤销 · 重做] ‖分隔‖ [修改 · 新增行 · 复制行 · 删除行]`，与 `editCmds` 一一对应 |
| ② | 翻页段是「首页 · 上页 · 取下一段 · 取全部 · 末页」——`last` 被摆到取数钮**之后** | 改为 `[首页 · 上页 · 下页 · 末页]` 紧接 `取下一段（≈fetch.page）` → `取全部（≈fetch.all）`，与 `navCmds` 同序 |
| ③ | **没有「下页」这颗钮**：`PageTarget::Next` 端口早就存在（`ports.rs`，语义「以当前可见首行为基准下移一屏；到底且还有下一段时顺带取数」），宿主 `go_page` 也已实现，但面板从未注入按钮 | 面板在 `has_rows` 时注入 `editor-result-page-next`（`ChevronRight`，tooltip 写清「到底且还有下一段时顺带取数」）；`ResultControls` 增 `next_page` 槽，未注入时回退 `status_action_placeholders` 里的禁用占位 |

- 占位清单随之改为 13 项（写 8 + 翻页 4 + 系统 1），分组分界 `FIRST_PAGER_ACTION = 8` /
  `FIRST_SYSTEM_ACTION = 12`；tooltip 也按 DBeaver 语义改成「提交 / 回滚待提交变更」。
- **两处行导航语义仍是本仓口径**（在册，非本轮改动）：DBeaver 的 `row.previous` / `row.next`
  是「上下移一行」，本仓是「上下移一屏」（`PageTarget::Prev/Next` 的既有定义，纯视图滚动，
  0 重查）；DBeaver 的 `fetch.page` 是显式取下一页数据，本仓以「取下一段」真钮（`controls.more`）
  承担，只在 `has_more` 时出现。

**用例**
- `grid::tests::status_action_placeholders_follow_dbeaver_group_order`（改写）：13 项、写操作
  两截顺序、翻页四颗含 `ChevronRight`、系统段只有导出。
- `panel::tests::status_row_pager_follows_dbeaver_nav_order`（新增）：注入 `total_rows = 2`
  后按**真实像素横坐标**断言 `page-first < page-prev < page-next < page-last`
  ——顺序是布局事实，只断言「在不在」守不住摆错位。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0 error / 0 warning；
`cargo test -p rds-result-panel --lib` = **278 passed**；
`cargo test -p rds-editor --lib results` = 45 passed；`cargo test -p rds-ui-guard` = 4 passed。


### 8.67 右栏每页工具条右端的「视图菜单」（2026-11-18）

DBeaver 的右栏面板不是「只有页签」：`ResultSetViewer.addPanel()`（`:1330-1386`）给每个面板
装了两层：`trControl`（横排）= 左 `panelInfo.actionToolBar`（各面板 `contributeActions`）+
右 `panelToolBar`，后者只由 `addDefaultPanelActions()`（`:1590-1614`）填——**就一颗**按钮：
「视图菜单」（`ResultSetMessages.result_set_view_menu_text` + `IMG_LCL_VIEW_MENU`），
点击用 `fillPanelsMenu()`（`:1559-1588`）组菜单逐面板的勾选项。

`fillPanelsMenu()` 的条目顺序（原文见 `.rds/tmp/db-panelsmenu.txt`）：

| # | 条目 | 命令 / 形态 |
| --- | --- | --- |
| ① | 每个可用面板一条 | `ResultSetHandlerTogglePanel.CMD_TOGGLE_PANEL` 的 `CommandContributionItem`，`style = STYLE_CHECK`，参数 `PARAM_PANEL_ID = panel.getId()` |
| ② | 分隔 | `Separator` |
| ③ | （`CMD_TOGGLE_LAYOUT` 在此被注释掉，不显示） | — |
| ④ | 面板最大化 | `IResultSetCommands.CMD_TOGGLE_MAXIMIZE` |
| ⑤ | 激活面板 | `CMD_ACTIVATE_PANELS` |
| ⑥ | 分隔 + 面板总开关 + 布局方向 | 仅当 `FEATURE_PANELS`：`CMD_TOGGLE_PANELS`、`CMD_TOGGLE_LAYOUT` |

本仓落法与口径：

- 插入点是**页内共享工具条** `panel_toolbar`（`panel/tabs.rs`）——六页（含空态页，
  `empty_page`）都走它，与 DBeaver「每个面板都有一颗」同构；按钮 id
  `<页工具条 id>-view-menu`，分裂壳 `<…>-view-menu-anchor`，图标 `IconName::Menu`，
  tooltip「视图菜单（切换面板 / 显示与布局）」。
- 条目清单抽纯函数 `panel_view_menu_items()`（`events.rs`）：**六页 → 分隔 → 面板总开关 →
  布局方向**，共 9 项。页项带 `checked(tab == 当前页)`（对应 DBeaver 的 `STYLE_CHECK`），
  点击发 `SetRightTab`；面板总开关发 `SetPanelsVisible(!当前)`；布局方向发
  `ToggleVerticalLayout`。
- **不摆的两项**：`CMD_TOGGLE_MAXIMIZE`（面板最大化）与 `CMD_ACTIVATE_PANELS`（把焦点交给
  面板）本仓都还没有落地端口，按「能力没有就不摆」不列，不造点了没反应的条目（在册 backlog）。
- 条目文案走 `panel_view_item_label(item, panels_visible, vertical_layout)`：两个开关项报
  **「点了会做什么」**（可见时报「收起结果面板」，否则「展开结果面板」；横排时报「切换为竖排
  布局（上下分栏）」，反之亦然），与 DBeaver 的命令标签口径一致。
- 顺带把页签的**图标 / 文案映射收成单一来源** `right_tab_icon` / `right_tab_label`
  （此前 `render_right_sidebar` 的 `active_tab` match 与 `render_right_edge_bar` 的六元组
  各写一份，视图菜单又要第三份）：现在页签条、右缘竖条、视图菜单三处共用一份。

**用例** `panel::tests::right_panel_view_menu_lists_pages_then_the_two_toggles`：
① 条目顺序（六页按竖条同序、共 9 项、后三项是分隔 / 面板开关 / 布局方向，且没有最大化 /
激活面板）；② 文案随状态切换；③ 六页逐页 `set_right_tab` + 出帧后
`<页工具条 id>-view-menu` 都能 `try_find` 到。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0 error / 0 warning；
`cargo test -p rds-result-panel --lib` = **279 passed**；
`cargo test -p rds-editor --lib results` = 45 passed；`cargo test -p rds-ui-guard` = 4 passed。


### 8.68 右栏页签条列出全部六页、点即切页（2026-11-18）

DBeaver 的右栏不是「一页换一页」，而是**一个**页签夹 `CTabFolder panelFolder`
（`ResultSetViewer.java:1430-1478`）：`addPanel()` 给每一页 `new CTabItem(panelFolder, SWT.CLOSE)`
（`:1330-1386`），六页都是常驻页签；点页签走 `setActivePanel()`，页签顺序 = `availablePanels`
的注册顺序，右缘竖条的 CHECK 按钮与「视图菜单」的勾选项改的是同一份「哪一页是活动页」。

本仓此前的做法是 **2026-10-28 的一次刻意收窄**：横向页签条只显示当前页（当标题用），
切页全靠右缘竖条，`on_click` 写成空实现——于是页签条成了一颗**点了没反应**的控件，
与 DBeaver「页签可点」差着一条真实交互。

| 项 | 旧状 | 现口径 |
| --- | --- | --- |
| 页签数量 | 只渲染当前页 1 个 | 六页全渲染（`events::right_tabs()` 的规范顺序） |
| 选中态 | `selected_index(0)` 常量 | `selected_index(right_tab_index(当前页))`（0..5 真实跟随） |
| 点击 | `.on_click(\|_\| {})` 空实现 | `.on_click` → `ResultRequest::SetRightTab(right_tab_from_index(index))`（宿主 `dispatch` 调 `set_right_tab`） |
| 溢出 | 不适用 | `.menu(true)`：页签放不下时给一颗溢出菜单钮（否则被裁掉的页签只能靠横向滚轮发现） |

- 顺带把**顺序与索引映射收成单一来源**：新增 `events::right_tabs()`（六页规范顺序）、
  `right_tab_index()`（枚举 → 下标）、`right_tab_from_index()`（下标 → 枚举，越界兜底默认页）；
  原来 `tabs.rs` 里那份 `#[cfg(test)]` 的 match 删除，`panel_view_menu_items()` 也改为由
  `right_tabs()` 生成。页签条 / 视图菜单 / 右缘竖条现在读同一份顺序。
- 页签行（`h_flex` 24px）挂调试选择器 `editor-result-right-tabs`：`TabBar` 组件自身不暴露
  可查 id（vendor 里 `tabs.group("tab-bar")` 无 id），测试要按这一行的真实像素点算点击坐标。
- **仍在册的差异**：DBeaver 的页签是 `SWT.CLOSE`（每页可关闭 = 从面板夹里移除）；
  kit `TabBar` 没有逐页签的关闭通道，本仓保留「页签行尾一颗 × → 退回默认页（列浏览器）」，
  × 只退页、不收起右栏（右栏开合仍由竖条总开关 / 状态栏三态开关管）。

**用例**
- `panel::tests::right_tab_index_maps_to_the_six_panel_tabs`（改写原四页版）：六页映射 +
  越界兜底 + `right_tabs()` 与两个映射函数**互逆**。
- `panel::tests::right_panel_tab_strip_lists_all_panels_and_click_switches`（新增）：先把当前页
  切到最后一个（执行SQL），再按页签行的真实像素 `simulate_click` 点第一格 → 断言
  **队列里出现 `SetRightTab(Browser)`**（切页是宿主动作，面板只入队；点空白 / 死回调
  都会让队列为空，这正是要守住的）。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0 error / 0 warning；
`cargo test -p rds-result-panel --lib` = **280 passed**；
`cargo test -p rds-editor --lib results` = 45 passed；`cargo test -p rds-ui-guard` = 4 passed。


### 8.69 筛选「运行」与「下发」收成同一套 wrapped SQL 形状（2026-11-18）

同一份筛选文本在本仓有**两条重查链路**，此前各自拼 SQL，形状不一致：

| 链路 | 入口 | 旧拼法 | 语义 |
| --- | --- | --- | --- |
| 运行（替换当前结果集） | 筛选框 ▶ / 展开条 ▶ | `select * from (原SQL) as _q where 词`（宿主内联 format） | WHERE **条件**原样 |
| 下发源库（产生新结果集） | 状态栏「下发源库」开 | `engine::sql::rewrite_with_filter`（每列 `CAST(x AS TEXT) LIKE '%词%'`） | 关键词 → 全列 LIKE |

旧拼法有三个具体问题：① 别名 `_q` 与下发路径的 `rds_filtered` 不同，日志/历史里同功能两种形状；
② **不摘顶层 `LIMIT`**——原查询带 `LIMIT 20` 时，「筛选」只在被截断的 20 行里找；
③ **`ORDER BY` 留在子查询里**，外层结果顺序不保证。

对齐依据：DBeaver 筛选条里的文本就是**一条 WHERE 条件**——`setCustomDataFilter()`
把 `filtersText.getText()` 直接塞进 `DBDDataFilter.setWhere(condition)`
（`ResultSetFilterPanel.java:618-648`），没有「关键词 → LIKE」这层翻译。

落法：`crates/sql/src/filter.rs` 新增 `rewrite_with_where(original, condition) -> Result<Rewrite, String>`，
与 `rewrite_with_filter` **共用同一套拼装**（`SELECT * FROM (<去 LIMIT 的正文>) AS rds_filtered
WHERE <条件>` + `ORDER BY` 外提 + `dropped_limit` 回报），只差条件生成：

- 「运行」→ `engine::sql::rewrite_with_where(&entry.sql, &value)`，失败给原因、`dropped_limit`
  有值时提示「已去掉原查询的 LIMIT…」（与下发同一口径）；
- 右栏「执行SQL」页的 wrapped SQL 预览（`render_filter_expand_bar`）同步改用同一函数——
  此前预览说的不是真正要跑的那句；
- 「下发」保持 `rewrite_with_filter`（关键词口径不变，见 §8.56 的两档设计）。

**仍在册的口径差异（不是漏做）**：筛选框的输入在两条链路上含义不同——「运行」按 WHERE 条件、
「下发」按关键词（后者是本地「任意列包含」语义的服务端对应）。DBeaver 只有一个口径（条件）。
要统一成条件口径需同时改下发链路与本地过滤语义，属口径拍板项，未动。

**验证**：`cargo test -p rds-sql --lib filter` = 15 passed（新增 4 条：条件原样进 WHERE、
去 LIMIT + ORDER BY 外提、空条件拒绝、两条路拼装形状一致）；
`cargo check -p rds-sql -p rds-result-panel -p rds-editor` 0 error / 0 warning；
`cargo test -p rds-result-panel --lib` = 280 passed；`cargo test -p rds-editor --lib results` = 45 passed；
`cargo test -p rds-ui-guard` = 4 passed。`run_filter_as_query` 本身没有端到端用例
（需要真执行器），其可验证内核就是上面这层改写函数。


### 8.70 工具栏补上「筛选设置」（DBeaver `filtersCustomButton`）+ 自定义 ORDER BY（2026-11-18）

**缺口**：DBeaver 筛选条工具条（`ResultSetFilterPanel.java:315-385`）的完整顺序是
`filtersClearButton`（`!compactMode`）→ **`filtersCustomButton`**（FILTER 图标、`SWT.DROP_DOWN`，
`:330-338`，**恒建**）→ 历史后退 / 前进（`!compactMode`）→ 分隔线 → 查找。本仓前四项都已落地，
唯独 `filtersCustomButton` 从未有过——那是这一行唯一真缺口。

**它打开的是什么**：`CustomFilterListener` 开 `FilterSettingsDialog`，两个页签——
「Columns」（列约束树：名称 / 原位置 / criteria / order 位 / 方向，带列可见勾选与上移下移置顶置底）
与「Custom」（`createCustomFilters`，`:425-451`）里的两个自由文本 `whereText` / `orderText`，
应用时 `setWhere(...)` / `setOrder(...)`（`:493-500`），并有一条警告文案
「自定义排序会禁用列排序」（`controls_resultset_filter_warning_custom_order_disabled`，`:350`）。

**本批落点（Custom 页签那一支）**：

1. 引擎：`crates/sql/src/filter.rs` 新增 `rewrite_with_where_order(original, condition, order_by)`，
   `rewrite_with_where` 改为转调它并传 `None`。语义：`order_by` 空 → 沿用原查询顶层 `ORDER BY`
   （搬到外层）；给了文本 → **替换**它（不再叠一层，避免外层两句 ORDER BY）。`dropped_limit`
   回报照旧。用例 +2（自定义排序替换原句 / 空白回落原句），`filter` 模块 15 → **17**。
2. 面板状态：`filter_settings_open` / `filter_order_input`（`InputState`，唯一可编辑项）/
   `filter_order`（已生效的自定义排序，打开浮层时回填）。
3. 入口：筛选行 ✕ 之后那颗 FILTER 图标键（`editor-result-filter-settings`），点击入队
   `OpenFilterSettings`（要 `&mut Window` 交焦点，走宿主窗口转手）。
4. 浮层：`dialog.rs::render_filter_settings`——标题「筛选设置」+ ✕、WHERE **只读回显**、
   自定义排序输入、页脚提示 + 取消 / 应用，Esc 关闭；`absolute` 遮罩，与 SQL 预览弹窗同范式
   （render 期只有 `&App`，建不了窗口级 dialog）。
5. 应用链路：应用钮入队 `CommitFilterSettings`（读输入框值要 `&mut Context`）→ 宿主调
   `panel.apply_filter_settings()` → 入队 `ApplyFilterSettings { where_text, order_text }`
   → 宿主 `run_custom_filter`：`rewrite_with_where_order` 拼
   `select * from (原SQL) as rds_filtered where 条件 [order by 自定义]`，
   写 `exec_sql` 后以 `ExecTarget::Statement` **替换**当前结果集重查。
6. 尺寸：卡片宽走新常量 `ui::RESULT_FILTER_SETTINGS_WIDTH`（35rem）——裸 `px` 会被
   `rds-ui-guard` 拦下（本轮实测确实拦了）。

**与 DBeaver 的差异（在册，非漏做）**：

- WHERE 一栏只读（筛选框本体才是唯一输入源；DBeaver 在对话框里可直接编 whereText）。
  理由是「同一个 `InputState` 只渲染一处」的硬约束，避免两处同时可编辑。
- 「Columns」页签（列约束树 + 列可见勾选 + 列顺序移动）未做。本仓对等能力已分散在列头菜单
  （列级值筛选 / 清除）、列浏览器（隐藏列）、排序下发（单列），缺的是「集中管理界面」。
  **（已由 §8.71 补上页签与列清单的可见 / 冻结切换；criteria / order / 列序移动仍在册。）**
- 自定义排序目前只作用于**本次重查**（不入状态历史、不落盘），与 DBeaver 的
  `dataFilter.setOrder` 每次应用即生效同口径；是否入历史属待拍板项。

**验证**：`cargo test -p rds-sql --lib filter` = 17 passed；
`cargo test -p rds-result-panel --lib` = **281 passed**（新增
`filter_settings_open_close_and_apply_carry_the_draft`：开关翻转 + 浮层真渲染 / 真消失 +
应用请求带「筛选框文本 + 排序草稿」两段）；
`cargo test -p rds-editor --lib results` = 45 passed；`cargo test -p rds-ui-guard` = 4 passed；
`cargo check -p rds-sql -p rds-result-panel -p rds-editor` = 0 error / 0 warning。

**过程中踩的环境坑（记下以免重犯）**：并行线编译时抢 `build` 目录锁 + 内存只剩 4GB 左右，
`cargo test -p rds-editor --lib --no-run` 出现过 `E0463 can't find crate`、
`invalid metadata files for crate similar`、`memory allocation of … failed` 三种假错——
**都不是代码问题**：等并行线结束、`CARGO_BUILD_JOBS=2` 重跑即过。
另：临时设 `CARGO_INCREMENTAL=0` 会让整条依赖链重编译，在低内存机器上必然 OOM，不要用。


### 8.71 筛选设置浮层扩成两页签，「列约束」页的可见 / 冻结是真的能切（2026-11-18）

**起点**：§8.70 把 `filtersCustomButton` 落地时只做了 DBeaver `FilterSettingsDialog`
的**第二个**页签（Custom）。DBeaver 那份对话框是 `CTabFolder` 两页、**先建列页并
`setSelection(0)`**（`FilterSettingsDialog.java:329-451`）——即默认停在「Columns」页。
本批把浮层补成两页签，并把列页做成真控件。

**取证（DBeaver Columns 页签的完整列集与工具条）**

- 列：名称 `addColumn(...)` `:129`；**可见** `addBooleanColumn(...)` `:148`；**固定(pinned)**
  `:178`；`order`（文本 `" " + constraint.getOrderPosition()`，`:214/223`）；`criteria`
  （`SQLUtils.getConstraintCondition(...)`，`:230/239` 清空）；原位置 = `getOriginalVisualPosition()+1`
  `:144`。
- 工具条（`:278-318`）：移顶 `ARROW_TOP` → 上移 `ARROW_UP` → 下移 `ARROW_DOWN` → 移底
  `ARROW_BOTTOM` → 排序 `SORT` → 全部显示 → 全部隐藏 → 重置 `ARROW_RESET`。
  底层动作：`moveSelectedItems(reverse, singleStep)` `:529`、`moveColumns(curIndex,newIndex)`
  `:399`、`swapVisualPositions` `:388`、`toggleColumnOrder` `:703`、`CheckStateProvider`（列可见）
  `:612`、`FilterSettingsTreeEditor`（criteria 单元格编辑）`:649`。
- 默认页：列页（与「先建列页 + `setSelection(0)`」一致）。

**本批落点**

1. `ports.rs`：新枚举 `FilterSettingsTab { Columns（默认）, Custom }` + 请求
   `SetFilterSettingsTab(FilterSettingsTab)`。
2. 面板状态：`filter_settings_tab`（默认 `Columns`）；`set_filter_settings_tab`（纯 UI 状态，
   不入任何请求）；只读投影 `column_constraint_rows(cx) -> Vec<(列名, 可见, 冻结)>`，
   按**数据列序**从网格 delegate 现算（不做缓存、不在 render 里写任何状态）。
3. 浮层：`render_filter_settings` 改成「标题行 → 页签条（`TabBar::segmented` + 两个 `Tab`）→
   页内容 → 页脚」。页签条挂 `editor-result-filter-settings-tabs` / `-tabbar`；
   页内容分两支：`render_filter_settings_columns`（列清单表头 + 行 + 位置号）与
   `render_filter_settings_custom`（§8.70 的 WHERE 只读回显 + 自定义 ORDER BY）。
4. 列页的真控件：每行 `(列名 | 可见 Checkbox | 冻结 Checkbox | #位置号)`，
   行 id `editor-result-filter-settings-row-{i}`、勾选 id `-vis-{i}` / `-pin-{i}`。
   勾选回调**就地改 delegate**（与列头菜单同一条先例路径）：可见 →
   `delegate_mut().set_columns_hidden(&[i], !show)`（内部仍有「至少留 1 列」兜底）；
   冻结 → `delegate_mut().toggle_freeze(i)`；随后 `state.refresh(cx)` 并
   `queue.request(ColumnLayoutChanged)` → 宿主 `save_session_now`，即落盘通道与列头菜单完全相同。
5. 页脚：列页的「应用」置灰（列改动是即时的，没有待提交草稿），提示文案改为
   「列约束改完即时生效（与列头菜单同一通道落盘）」；自定义页保持「应用 = 重查替换」。
6. 尺寸：列清单区 `max_h(rems(18.))` + `overflow_y_scrollbar()`，宽度仍走
   `ui::RESULT_FILTER_SETTINGS_WIDTH`；本批未引入任何裸 `px`（`rds-ui-guard` 4 条仍绿）。

**与 DBeaver 的差异（在册，非漏做）**

- **criteria 列**（列级条件）未做：本仓列级条件走列头菜单（`FilterColumnValue` /
  `ClearColumnFilter`），**没有可投影的持久列条件状态**；要做先得把列条件提升为面板状态。
- **order 列**（点击切 ASC/DESC）未做：本仓排序走服务端下发（`set_column_order`），
  与 DBeaver 的「约束 sort 只影响本地视图」不同口径。
- **工具条 8 项**（移顶/上移/下移/移底/排序/全显/全隐/重置）未做：本仓已有
  `set_columns_hidden`（批量显隐）、`toggle_freeze`、`show_all_columns`，
  列序移动实现存在（`grid/table_impl.rs::move_column`）但未接任何 UI。
  **（其中全显 / 全隐 / 重置已由 §8.72 接上；四颗移动钮与「排序」仍受列序接口限制。）**
- 页签切换不做键盘左右键漫游（`TabBar` 自身管焦点，未额外绑键）。

**验证**：`cargo test -p rds-result-panel --lib` = **282 passed**（新增
`filter_settings_columns_tab_projects_the_grid_column_state`：默认停在列页 + 切页不入队 +
列清单初始两列可见未冻结 + 隐藏第 1 列后投影变 `false` + 冻结第 2 列后投影变 `true`）；
`cargo test -p rds-editor --lib results` = 45 passed；`cargo test -p rds-ui-guard` = 4 passed；
`cargo check -p rds-result-panel -p rds-editor` = 0 error / 0 warning。


### 8.72 列约束页补齐「条件 / 排序」两列（只读投影）+ 页内工具条三颗（2026-11-18）

**起点**：§8.71 落了列约束页，但只做「列名 / 可见 / 冻结 / #位置」，页内也还没有工具条。
本批按 `FilterSettingsDialog.java` 补齐剩下**有落点**的部分。

**取证**

- 列集（`:129-243`）：名称 / 可见 / 固定 / order / criteria 五列 + 原位置（`getOriginalVisualPosition()+1`）。
- 页内工具条（`:276-325`），八颗：
  `moveTopButton` → `moveSelectedItems(false, false)`；`moveUpButton` → `(false, true)`；
  `moveDownButton` → `(true, true)`；`moveBottomButton` → `(true, false)`（四颗默认 `setEnabled(false)`，
  **要有选中行才亮**）；分隔 → `SORT`（`attributes.sort(ALPHA_SORTER)` 后逐条
  `constraint.setVisualPosition(i)` 重排）→ 分隔 → `showAllButton`（全部 `setVisible(true)`）→
  `showNoneButton`（全部 `setVisible(false)`）→ `reset`（`dataFilter.reset()` 后把 `orderText`
  与 `whereText` 清空）。

**本批落点**

1. `ports.rs` 新结构 `ColumnConstraint { name, visible, frozen, criteria, sort }`；
   `column_constraint_rows()` 的返回类型从元组换成它。
2. 两列只读投影的来源（都是现有 delegate 真状态，不是常量）：
   - **条件** ← `delegate.column_filter()`（列级「按值筛选 / 清除」从列头菜单写入，
     `delegate.rs:656/663`）；
   - **排序** ← `delegate.sort_state()`（列头点击的本地排序，`view_rows.rs:377`），
     `Some(true)` 画 `↓`、`Some(false)` 画 `↑`、无则 `—`。
   空条件显示 `—` 并用 `muted_foreground`，有值用正文色（避免满屏符号抢注意力）。
3. 页内工具条（`Toolbar::xsmall`，三颗 `Button::ghost().xsmall().icon(...)`）：
   - 全部显示 `IconName::Eye` → 请求 `ColumnsShowAll` → 面板 `show_all_columns`
     （delegate `show_all_columns` 清隐藏清单）；
   - 全部不显示 `IconName::EyeOff` → `ColumnsHideAll` → 面板 `hide_all_columns`
     （逐列 `hide_column`；**本仓委托有「至少留 1 列可见」底线，故只留最后一列**——
     与 DBeaver 允许全隐不同，属有意差异）；
   - 重置 `IconName::RotateCcw` → `ColumnsResetLayout` → 面板 `reset_column_layout`
     （新 delegate 方法：清隐藏 + 清冻结；列序与列宽不动，它们各有独立持久化通道）。
   三颗都走 `snapshot_column_layout` → `ColumnLayoutChanged`，与列头菜单同一落盘通道。

**未做与理由**

- **四颗移动钮（移顶 / 上移 / 下移 / 移底）与「排序」**：DBeaver 直接改约束的
  `visualPosition`；本仓列序权威在组件侧（`TableState` 的 `col_groups`），delegate 只有在拖拽
  回调里同步 `display_order` 的一条通道（`table_impl.rs:196`）。要接四颗钮就得先拿到
  「程序化重排列序」的组件接口，否则按钮是假的——本批不摆。
- 「排序」在 DBeaver 里是**列名算法序重排**，同样受上述限制；本仓表头点击排序已有（本地排序）。
- 「重置」在 DBeaver 还清 where/order 两页文本；本仓 where 归筛选框、order 归自定义页，
  页内重置不跨页清它们的草稿（口径差异，记在此）。

**验证**：`cargo test -p rds-result-panel --lib` = 282 passed，用例
`filter_settings_columns_tab_projects_the_grid_column_state` 追加三段：
① 初始列清单四要素全空；② `reset_column_layout` 后全可见全未冻结；
③ 写入 `set_column_filter(Some((1, "> 100")))` + `apply_sort(Some((1, true)))` 后
`rows[1].criteria == "> 100"`、`rows[1].sort == Some(true)`、`rows[0]` 两列仍为空
（钉住「条件 / 排序是真投影、不是常量」）；④ 全部不显示后可见列数 = 1（钉住底线）；
⑤ 全部显示后每列都可见。
`cargo test -p rds-editor --lib results` = 45 passed；`cargo test -p rds-ui-guard` = 4 passed；
`cargo check -p rds-result-panel -p rds-editor` = 0 error / 0 warning。
### 8.73 列序成为视图列序的权威：程序化移动 / 按列名排序 / 重置回数据序（2026-11-18）

**取证**（DBeaver `FilterSettingsDialog.java:276-325`，列集工具条）：
- `:278-293` `moveSelectedItems(boolean moveToEnd, boolean down)` —— 四颗移动钮
  （移顶 / 上移 / 下移 / 移底）作用于**当前选中项**，语义是「把选中项搬到目标席位」，
  不是交换；四颗按钮初始 `setEnabled(false)`，没选中就不可点（`:276` 一带）。
- `:295-302` `SORT` = `ALPHA_SORTER`：按列名算法序，逐条 `setVisualPosition(i)` 写席位。
- `:318-325` 重置 = `dataFilter.reset()`：列可见 / 冻结 / 列序一起回默认（并清
  where/order 草稿，那是 DBeaver 的跨页口径，本仓仍按 §8.72 记的差异走）。

**本批最关键的既有隐患**（改之前 `display_order` 只是个「持久化快照」）：
- 组件的 `data_column` 换算原本**完全不读** `display_order`，只按 `hidden` 顺序数。
- 组件拖拽路径 `TableState::move_column`（`vendor/gpui-component/src/table/state.rs:1390-1407`）
  先调 delegate 的 `move_column` 改 `display_order`，再自己 `col_groups.remove/insert`
  并 `notify`——**不 refresh**。所以拖完当帧看着对，但只要之后发生一次 `refresh`
  （`state.rs:442` → `:731 prepare_col_groups` 全量按 `delegate.column(i)` 重建），
  视觉列序就退回「隐藏列序」，而表头 / 单元格仍按用户拖出来的位置取数 → 错位。

**实现**：
- `delegate.rs` 新增 `order_cache: Vec<usize>`（`display_order` 折算成数据列下标）。
  `view_rows.rs` 新增 `rebuild_column_order()`：按 `display_order` 的列名匹配数据列，
  匹配不到的（上一份结果残留名）跳过，本份结果没进 `display_order` 的列按数据序补末尾
  （与 `reorder_by` 的「不猜」口径一致）。
- `data_column` / `view_column` 改读缓存并跳过隐藏列；另加 `data_column_checked`
  （越界 `None`），`column_kind` / `type_label` 走它，保住既有「越界不 panic」断言。
- 缓存重建点：`set_data`、拖拽 `move_column`、`move_display_column`、
  `sort_display_order_by_name`、`reset_display_order`。
- 新增 `move_display_column(data, target_view)`（目标席位夹取到 `[0, 可见列-1]`，
  同席位直接返回，隐藏列不参与移动但列名仍留在 `display_order` 里）、
  `sort_display_order_by_name()`、`reset_display_order()`。
  `reset_column_layout()` 现在 = 显示全部 + 取消全部冻结 + **列序回数据序**。
- 面板：`move_selected_column(ColumnMove, cx)`（作用在 `selected_col`，无选中 / 隐藏 /
  已在目标席位 → 直接返回；成功 `state.refresh(cx)` 让 `col_groups` 按新序重建）、
  `sort_columns_by_name(cx)`；两者成功后走新的私有 `snapshot_column_order(cx)`，
  与拖拽同一落盘通道（变了才推 `ColumnOrderChanged { order }`）。
- 请求：`MoveSelectedColumn(ColumnMove)`（`ColumnMove { Top, Up, Down, Bottom }`）、
  `SortColumnsByName`；宿主 `dispatch.rs` 两臂直接转发。
- 浮层工具条改 9 元素（DBeaver 顺序）：移顶 `ArrowUpToLine` → 上移 `ArrowUp` →
  下移 `ArrowDown` → 移底 `ArrowDownToLine` → 竖分隔 → 排序 `ArrowUpAZ` → 竖分隔 →
  全部显示 `Eye` → 全部不显示 `EyeOff` → 重置 `RotateCcw`。四颗移动钮的 enabled 由
  `selected_col` 的可见席位决定：`view > 0` 才能上移 / 移顶，`view < last` 才能下移 / 移底。

**验证**：`cargo test -p rds-result-panel --lib` = **283 passed**；新用例
`grid::tests::display_order_drives_the_view_column_mapping` 手算覆盖
「默认序 → 移顶 → 重复移顶幂等 → 下移 → 移底 → 隐藏中间列后移动 → 按列名排序 →
重置（含冻结清空）」，每一步都同时断言 `display_order()` 与 `data_column/view_column`
映射；`panel::tests` 的列约束用例尾部追加五段（移顶 → 列序快照同步 → 幂等 → 移底 →
排序 → 重置）。`cargo test -p rds-editor --lib results` = 45 passed；
`cargo test -p rds-ui-guard` = 4 passed；`cargo check -p rds-result-panel -p rds-editor`
= 0 error / 0 warning。提交 `c70a88be`。

**仍缺**（记在 §2.62 一并）：DBeaver 的列序是「约束的 visualPosition」，本仓列序仍在
`display_order`（列名序）+ `SavedSession` 落盘；多选批量移动（DBeaver 支持多选）本仓
只做当前选中列；重置不跨页清 where/order 草稿。
### 8.74 状态栏命令项按 DBeaver「工具条配置」显隐 + 配置菜单（2026-11-18）

**取证**：
- `plugin.xml` 的 `org.jkiss.dbeaver.toolBarConfiguration` / `resultsetViewer.bottom`
  （`:1381-1397`）逐命令给 `defaultVisibility`：`org.eclipse.ui.edit.undo`、
  `edit.redo`、`core.resultset.fetch.page` 三项 **false**，其余（applyChanges /
  rejectChanges / row.edit / row.add / row.copy / row.delete / row.first / previous /
  next / last / fetch.all / export）**true**。
- 同一份 `plugin.xml` 的 `org.eclipse.ui.menus`（`.rds/tmp/db-status-toolbar.txt` 全文）
  里，每条 `command` 都包在
  `<test property="org.jkiss.dbeaver.ui.toolbar.configuration.visible" …>` 里——可见性
  是**工具条配置**的结果，`false` 只是默认值，用户右键工具条 → Configure 可改。
- `ResultSetViewer.ConfigAction`（`:5402-5418`，CONFIGURATION 图标）打开结果集首选项页，
  是「配置」的另一半（本仓这半还没做，记在 open items）。

**本仓落点**（对齐「能被开关的可见性」，而不是直接把钮删掉）：
- `ports::StatusCommand` 16 项（顺序 = DBeaver 底部工具条顺序）+ `default_visible()`
  + `menu_label()`；`grid::status::status_config_items(hidden)` 是纯函数（顺序 / 文案 /
  勾选态），渲染只按它挂事件（菜单内容不可读，用纯函数钉）。
- `render_status_row` 增两个入参：`hidden_commands: &BTreeSet<StatusCommand>`、
  `config_menu: Option<AnyElement>`。左动作组三段的每个槽位都改成「按命令查显隐再决定
  是否 push」；段间发丝分隔线只在**两侧都还有内容**时画（DBeaver 用 `visibleWhen` 做
  同一件事，全隐一段时不该留孤线）。
- 状态行最末端（右栏开关之后）挂 `Settings2` 图标 ▾（`DropdownButton` + 逐项
  `PopupMenuItem::new(label).checked(..)`），点击推
  `ResultRequest::SetStatusCommandVisible { command, visible }`；面板持有
  `hidden_status_commands: BTreeSet<StatusCommand>`（纯 UI 态、不落盘），
  默认集合 = `default_visible()` 为 false 的两项。

**有意偏离（必须记住）**：DBeaver 把 `fetch.page` 也默认隐藏，但它有自动取数、且能随时
把命令勾回来；本仓「取下一段」是窗口化取数的**唯一可见入口**（无第二处按钮、无快捷键），
默认隐藏等于读者拿不到后面的行 → `FetchPage::default_visible()` 返回 `true`，即常显
（仍可在配置菜单里手动隐藏）。这条是本批唯一的口径差异，别再当 bug 改回去。

**验证**：`grid::tests::status_config_items_follow_dbeaver_toolbar_order_and_defaults`
（默认两项、顺序 = `StatusCommand::ALL`、勾选态 = `defaultVisibility`、勾回撤销只改该项）；
`panel::tests::status_commands_follow_dbeaver_default_visibility_and_can_be_restored`
（默认隐藏集合两项；状态行里 `editor-result-undo` / `-redo` 不渲染、`editor-result-page-first`
在；`editor-result-status-config` 在；勾回撤销后 `editor-result-undo` 出现且集合剩一项；
反向隐藏 First 后 `editor-result-page-first` 消失）。门禁：panel **285 passed**、
editor results 45、ui-guard 4、check 0-0。提交 `e1b797db`。
### 8.75 筛选历史 tooltip 补上命令描述（2026-11-18）

**取证**：DBeaver `ResultSetFilterPanel.updateControls`（`ResultSetFilterPanel.java:456-475`）：
- 后退钮可用时 `setToolTipText(stateHistory.get(pos-1).describeState() + " (" +
  ActionUtils.findCommandDescription(IWorkbenchCommandConstants.NAVIGATE_BACKWARD_HISTORY,
  site, true) + ")")`；前进钮对称（`NAVIGATE_FORWARD_HISTORY`）。
- 禁用分支只 `setEnabled(false)`，**不刷新 tooltip**（保留上一次的文案）。
- 也就是说 tooltip 是「要去的那条状态的描述 + 括号里的命令名」，光有描述读者分不清
  这颗钮是「退到哪」还是「前进到哪」。

**本仓落点**：`panel::events::history_hint(delta)` 在有目标时返回
`format!("{} ({command})", state_label(&sql))`，`command` = 「后退历史」/「前进历史」
（就是 DBeaver 那两条命令的中文描述）。空历史 / 已到头这两支仍给说明句（DBeaver 禁用态
不刷新 tooltip，本仓多给一句没有副作用，且用例可断言）。

**验证**：`panel::tests` 的历史用例把旧的 `hint == "select 1"` 改成
`"select 1 (后退历史)"`，并补一条正向断言（在末位时 `history_hint(1) == "已经在最新的状态"`）。
门禁：panel **285 passed**、editor results 45、ui-guard 4、check 0-0。提交 `a5bede1e`。
### 8.76 状态行齿轮 → 打开设置并定位到「结果集」节（2026-11-18）

§8.74 里登记过「DBeaver `ConfigAction` 的另一半还没做」，本批把它补上。

**取证**：
- `ResultSetViewer.createStatusBar`（`:2113-2122`）：状态栏最后一条 `ToolBarManager`
  只放一个 `ToolbarSeparatorContribution(true)` + `ConfigAction`，也就是说齿轮是
  状态栏**最右端**那颗独立按钮（不在工具条配置里）。
- `ConfigAction`（`:5402-5418`）：文案 `ResultSetMessages.controls_resultset_viewer_action_options`、
  图标 `UIIcon.CONFIGURATION`；点击执行
  `UIUtils.showPreferencesFor(control.getShell(), viewer, PrefPageResultSetMain.PAGE_ID)`
  —— 打开首选项对话框并**直接定位到结果集页**（不是首页）。

**本仓落点**（关键约束：`result_panel` 不认识 `settings` crate，依赖只向下）：
- `ports::ResultRequest::OpenResultPanelSettings`：面板只推请求。
- `grid::status::render_status_row` 新增 `settings_button: Option<AnyElement>` 槽位，
  挂在状态行最右端（在工具条配置 ▾ 之后），与 DBeaver 的「最后一个 config 工具条」同位置。
- `panel::render` 构造 `Button::new("editor-result-status-preferences")`
  （`IconName::Settings` / tooltip「结果集设置」/ ghost + small），点击推请求。
- editor `dispatch_result_request`：经面板持有的窗口句柄
  `window.dispatch_action(Box::new(settings::commands::OpenResultPanelSettings), app)`
  —— 与菜单栏 / 快捷键同一机制，动作沿元素树冒泡到 workbench 根
  （headless / 窗口已关时静默跳过）。
- `settings::commands` 新增动作 `OpenResultPanelSettings`（文档注释写明它对应 DBeaver 的
  `ConfigAction`）；`SettingsPage::new_at(window, host, Option<&'static str>, cx)`
  （`new` 保留为 `new_at(None)`）；`skeleton::build(host, notify, initial_section)`
  用 `page_index_for(section)` 折算 `SelectIndex { page_ix }`，未知 id / `None` 落回首页。
- workbench：`open_settings_at(section, window, cx)`（`open_settings` = `at(None)`，行为不变：
  窗口已存在时不抢焦点），新增 `OpenResultPanelSettings` 的 `on_action` →
  `open_settings_at(Some("result_panel"), …)`。

**验证**：
- `settings::skeleton::tests::page_index_lands_on_the_requested_section`：页序号必须等于
  `registry::sections()` 里该节的真实位置（防硬编码 0 导致落首页），`None` / 未知 id → 0。
- `panel::tests::status_settings_gear_opens_the_result_panel_preferences`：有状态行时齿轮
  必摆（`debug_bounds` 量到），点它入队 `OpenResultPanelSettings`。
- 门禁：check（settings / result_panel / editor / workbench）0 error / 0 warning；
  settings **37 passed**、panel **286 passed**、editor results 45、ui-guard 4。提交 `c398032f`。

**已知限制**：设置窗口已开着时，齿轮**不会**把它切到结果集页（`open_settings_at` 的
「已开则直接返回」保持原语义）；DBeaver 的 `showPreferencesFor` 遇到已开的对话框会
重新选页。要改需要 workbench 侧对已开窗口再发一次「选页」信号（未做）。

### 8.77 状态栏命令显隐落盘：`result_panel.hidden_status_commands`（2026-11-18）

§8.74 的显隐只是面板内存态（关掉面板就丢），而 DBeaver 的这层状态是 workbench 级
工具条配置：`plugin.xml:1381-1397` 的 `defaultVisibility` 只是**默认值**，每条命令都包在
`org.jkiss.dbeaver.ui.toolbar.configuration.visible` 的 `visibleWhen` 里，用户勾掉/勾回后
**重启仍保持**。本批把同一份状态落到全局偏好。

**形态取舍**：不做 16 条布尔登记项，而是**一条 `KindTag::Text`**（逗号分隔的稳定 id）。
理由：登记表的三条不变式要求「模型叶子必须登记」且「默认值一致」，16 条布尔会把
一张显隐表拆成 16 行设置页噪声，而用户真正的写入口是状态行末端的 ▾ 菜单；
设置页只需要「能看见、能手改」这一处。副作用是设置页显示的是 id 串而非中文名，
已在登记项 `hint` 里把 16 个 id 全列出来。

**实现**：
- `ports::StatusCommand` 增 `id()`（kebab：`commit` / `reject` / `undo` / `redo` /
  `edit-row` / `insert-row` / `duplicate-row` / `delete-row` / `first` / `prev` / `next` /
  `last` / `fetch-page` / `fetch-all` / `export` / `refresh`，注释写明**不得改**——settings
  文件里存的就是它）、`from_id()`、`parse_hidden_list()`（忽略空白项与未知 id）、
  `hidden_list_text()`（按 `ALL` 顺序输出 ⇒ 同一集合序列化稳定，不随勾选顺序漂）。
- 模型 `result_panel.hidden_status_commands: String`，`#[serde(default)]` =
  `"undo,redo"`（与 `defaultVisibility=false` 的两项同集合；`fetch-page` 仍是有意常显）。
- 登记表新增 `SettingSpec`（组「选择与操作」/ `SettingKind::Text` /
  `default_json: "\"undo,redo\""` / `effect: NextUse` / `hint_inline`），
  `Slot::ResultPanelHiddenStatusCommands` + `slot_for` + `slot_kind → Text` +
  `value_by_key` / `apply_by_key` 分组臂齐备。
- `service::result_panel`：`apply_slot` 新臂写 `normalize_command_list`（去空白、丢空项、
  统一小写）后落盘；`SettingsService::set_result_panel_hidden_status_commands(list, cx)`
  （同值不写盘、不 `refresh_windows`——面板自己已重绘）。**不校验 id**：id ↔ 命令的
  对应表在面板 crate，settings 层不依赖它（依赖只向下），未知 id 由面板读取时忽略。
- 面板：`set_hidden_status_commands(BTreeSet<StatusCommand>, cx)` = **整批替换**语义
  （从偏好恢复），与 `set_status_command_visible`（增量 + 重绘）分工；值相同则不 notify。
- editor：`construct.rs` 建面板时读偏好 → `parse_hidden_list` → 注入；`dispatch.rs` 的
  `SetStatusCommandVisible` 改为「先改面板，再把面板的权威集合序列化回偏好」——
  点一次 ▾ 菜单落一次盘。

**验证**：
- `grid::tests::status_command_ids_round_trip_and_parse_the_persisted_list`：id 唯一且
  `from_id`/`id` 互逆、未知 id 与空项被忽略、`undo,redo` 解析成撤销 + 重做、
  序列化按 `ALL` 顺序、往返回到同一集合。
- `panel::tests::injected_hidden_status_commands_replace_the_default_set`：注入
  `undo,first` 后面板只剩这两项（「重做」回到显示），状态行上首页钮消失、重做钮出现
  ——钉住「整批替换而非与默认合并」，否则用户勾回的项下次启动会被默认集合吃掉。
- 门禁：check（settings / result_panel / editor / workbench）0 error / 0 warning；
  settings **37 passed**、panel **288 passed**、editor results 45、ui-guard 4。提交 `7836006e`。

**已知取舍**：设置页改本项是 `NextUse`（下次创建结果集面板才生效），面板 ▾ 菜单改则
即时生效；DBeaver 的工具条配置两处都即时。要补齐需要 settings → 活动面板的推送通道
（现有 settings 层只在 NextUse / 各域自己的同步点用工），未做。

### 8.78 列浏览器页补「生成 DDL」（对齐 DBeaver Metadata 面板唯一动作，2026-11-18）

**取证**：
- `panel/metadata/MetaDataPanel.java:203-215` 的 `contributeActions` **只贡献一条**命令：
  `org.jkiss.dbeaver.ui.editors.sql.generate.ddl.by.resultSet`
  （文案 `ResultSetMessages.generate_ddl_by_result_set_name`、图标 `DBIcon.SQL_TEXT`、
  第四参 `true` = 恒可用）。该面板工具条其余内容 = 顶部筛选框（标签 + SEARCH 文本框 + ESC 清空，
  `:84-98`）+ 无其它动作。
- 处理器 `SQLGeneratorHandlerResultSetDDL.execute`：拿当前结果集 →
  `new SQLGeneratorDDLFromResultSet()` → `generator.initGenerator(List.of(controller))`
  → 弹 `ViewSQLDialog`（标题 `sql_generator_title_text` + 连接容器名），
  `createDialogArea` 里跑 `generator` 并把结果 `setSQLText`。
- 生成内容 `SQLGeneratorDDLFromResultSet.generateSQL`（`:52-99`）：
  1. `ResultSetDataContainer` + `DatabaseMappingContainer(mapping.refreshMappingType(create))`
     —— 走**数据迁移**的映射机制，所以生成的是「按结果集列建**新表**」的 DDL；
  2. 动态元数据 → 直接写 `sql_generator_nonsql_text`（不出 SQL）；
  3. 找不到目标对象容器 → `sql_generator_no_obj_container_text`；
  4. `DatabaseTransferUtils.generateTargetTableDDL` + `SQLUtils.generateScript` 出方言脚本；
     结果为空 → `sql_generator_no_ddl_text`。

**本仓落点**：
- 新模块 `crates/result_panel/src/ddl.rs`（纯函数，无 UI 依赖）：
  - `create_table_ddl(table, &[DdlColumn]) -> DdlOutcome::{Sql, Unavailable}`：
    逐列 `  name TYPE [NOT NULL] [DEFAULT v]`，列间逗号、末行 `);`；
    `DdlColumn.nullable == Some(false)` 才写 `NOT NULL`（元数据没说就不写约束）；
    `default_value` 照驱动原话直出。
  - 类型：`resolve_type(driver_type, kind)` —— 驱动口径优先（`inferred=false`），
    否则按 `ColumnKind` 值族给通用类型（**不带精度**）并 `inferred=true`（行尾注明
    「类型按值族推断（驱动未报）」）；`ColumnKind::Unknown` 且无驱动类型 → `None`。
  - 认不出类型的列 → **整份不出 SQL**，返回点名列出哪几列的说明（对齐 DBeaver 的
    `nonsql_text` 分支：只给说明，不编一个假类型）。
  - 标识符按原名直出、不套引号（引号字符随方言变，本层不知道目标方言；DBeaver 走
    `SQLUtils` 的方言引用规则）。
  - 表名由面板给：所有「有来源表」的列指向同一张表时用其**裸名**（去掉 `schema.`），
    否则占位 `new_table`（DBeaver 的表名来自迁移映射容器，本仓无此机制）。
- 列浏览器页页内工具条（`panel_toolbar`，28px + HAIRLINE）**常驻**一颗
  「生成 DDL」钮（`IconName::FileCode`、id `editor-result-generate-ddl`、带 tooltip），
  与多选批量动作同一 `Toolbar`（按钮在前、批量动作在后）→
  `ResultRequest::GenerateResultSetDdl` → editor 走窗口句柄转手（`TextareaState::new`
  需要 `&mut Window`）→ `ResultPanel::open_ddl_dialog`。
- 预览复用既有 SQL 预览浮层：面板新增 `sql_dialog_title: Option<SharedString>`
  （`None` = 「原始 SQL」；生成 DDL = 「生成 DDL（按结果集列建表）」）；
  `open_sql_dialog` / `close_sql_dialog` 都把它复位成 `None`，避免两个场景互相串标题。
- 素材来自网格 delegate：列名 `columns()`、驱动类型
  `column_annotations()[i].data_type`（NavCache 列元数据）→ 退 `column_types()[i].name`、
  可空 `annotations[i].nullable`、默认值 `annotations[i].default_value`、
  值族 `column_types()[i].kind`。

**验证**：
- `ddl::tests`（6 例）：逐列形状与逗号位置；`NOT NULL` 只在明确不可空时写、默认值直出；
  推断类型必须带行尾注明；缺类型列 → 拒绝出 SQL 且说明里点名该列；空列 / 空表名不崩；
  `resolve_type` 的三档（驱动优先 / 值族推断 / `Unknown` → `None`）。
- `panel::tests::column_browser_toolbar_queues_the_generate_ddl_request`：工具条常驻该钮，
  点击入队 `GenerateResultSetDdl`。
- `panel::tests::generate_ddl_dialog_shows_sql_or_an_explanation`：有驱动类型时浮层打开、
  标题正确；清空类型后走说明分支；关闭后标题复位。
- 门禁：check（settings / result_panel / editor）0 error / 0 warning；settings 37、
  result_panel **296**、editor results 45、ui-guard 4。提交 `140079c1`。
  `rds-workbench` 本批未编译：并行线正在改 `crates/connection_dialog/.../managers.rs`
  （6 处 E0308，与本批无关）。

**已知差异 / 缺口**：
- 没有「值类型 → 方言类型」的映射转换（DBeaver 走迁移框架按 `DatabaseMappingType.create`
  逐列选目标方言类型）；本仓只能照驱动类型出，表达式列给不带精度的通用类型并注明。
- 表名不做用户输入：DBeaver 的 `ViewSQLDialog` 里 SQL 可编辑（可改表名后直接执行），
  本仓预览浮层是**只读** Textarea（对齐既有 SQL 预览的做法）。
- DBeaver 的 Metadata 面板还有：顶部筛选框（按列名过滤）、树形嵌套列（`getNestedBindings`）、
  选中列与网格当前列**双向**同步、隐藏列用斜体、"Copy column text" 右键项
  （`MetaDataPanel.java:219-301`）。本仓列浏览器页的搜索框（kit `List` 内置）、
  选中列联动、隐藏列置灰斜体已具备；**树形嵌套列**与**右键「复制列名」**仍未做。


### 8.79 「引用」页接上外键清单（对齐 DBeaver References 面板的键列表，2026-11-18）

**取证**（`plugins\org.jkiss.dbeaver.ui.editors.data\src\org\jkiss\dbeaver\ui\controls\resultset\panel\references\`）：
- `ReferencesPanel.java`：`contributeActions` **空实现**（`:156-158`）⇒ 该页没有页内动作；
  正文只有一个占位容器，内容在 `ReferencesResultsContainer`（`:62-113`）；
  `activatePanel` → `refresh(false)`（`:133-136`）⇒ **每次切到该页都重读**。
- `ReferencesResultsContainer.java`：顶部一行 = 只读下拉 `CSmartCombo<ReferenceKey>`
  （首项 `null`，文案 `ResultSetMessages.refs_no_refs_text`，`:97-121`）+「打开目标对象」
  （`refs_open_target`，`:124-140`）+ 下方**嵌套一份 `ResultSetViewer`** 显示引用行（`:143-149`）。
- 键清单 `refreshReferenceKeyList`（`:219-357`）：可见属性所属实体 → 出向 FK
  （`DBVUtils.getAllAssociations`）与入向引用（`getAllReferences`）；
  **出向的收键条件是该 FK 的全部列都在可见属性里**（`:298`）；读失败**静默**
  （`:347-350`，注释「References or FKs may be unsupported by current database」）。
- 键文案（`:512-528`）= `目标表 (约束名)`，跨数据源再追加 `[连接名]`；
  活动键持久化在虚拟实体属性 `ref-panel-associations`（`:106-120`）。

**本仓落点（本轮 = 方案 C：清单 + 打开目标对象，不做引用行）**：
- 新模块 `crates/result_panel/src/references.rs`（纯函数 + 状态，无 UI 依赖）：
  - `ReferenceKey{table, constraint, columns, target_columns}`；
  - `keys_from_constraints(&[ConstraintDetail], visible)`：三条过滤（是外键 /
    驱动报了目标表 / **全部本表列都在可见列里**），列名按忽略大小写比对
    （约束来自驱动、可见列来自结果集，大小写不保证一致）；结果按
    目标表 → 约束名 → 本表列 排序（DBeaver 按遍历序不排序，这里排序是为了渲染稳定）；
  - `key_label` = `目标表 (约束名)`（没约束名时只留目标表，同 `RefKeyLabelProvider`）；
  - `mapping_text` = `本表列 → 目标表.目标列`（驱动没报目标列时只到表名，**不编列名**）；
  - `ReferencesState::{NotLoaded, Loading, Loaded, Failed}` + `NO_REFERENCES = "无引用"`
    （照 DBeaver 的空项文案）。
- 取数走宿主（`result_panel` 不认识 engine / database，连接身份只在宿主手里）：
  `ResultRequest::LoadReferences { schema, table, visible_columns }` →
  `crates/editor/src/view/host/results/references.rs::load_references` →
  `database::metadata_service::MetadataService::list_constraints`
  （与 ER 视图同一套服务，**不直连库、不碰凭据**）在 `cx.background_spawn` +
  `background_executor().spawn` 的后台任务里跑，**UI 线程不 await**。
  失败分两类：宿主自身原因（未绑定连接）→ `Failed` 显示原因；驱动不支持 / 查询报错 →
  按 DBeaver 口径静默成 `Loaded([])`（只记 `debug` 日志）。
- 来源表身份 `ResultPanel::reference_source(cx)`：所有已注解列指向**同一张**来源表才认
  （多表 join / 计算列 → `None`，直接落「无引用」）；`schema` 取限定名前半段；
  可见列 = delegate 列 **减去** 面板隐藏列，并且只留来自该来源表的列。
- 触发点：editor 的 `ResultRequest::SetRightTab(Reference)` 分派里追加
  `begin_references_load`（面板置 `Loading` + 入队取数）——对应 DBeaver
  「激活即刷新」，不做「读过了就不再读」的缓存。
- 页面（`panel/tabs.rs::render_reference_tab`）四态：未读 / 读取中 / 失败原因 / 「无引用」/
  有键时 = 只读键下拉（`DropdownButton` + 勾选式菜单，对齐 `SWT.READ_ONLY` 的 combo）
  +「打开目标对象」（`IconName::Table`，无键时禁用）+ 选中键的列映射文本。
- 「打开目标对象」：宿主注入端口 `EditorShared::attach_reveal_object`
  （`RevealObjectPort = Rc<dyn Fn(engine::ObjectRef)>`，workbench 在装配期接到
  `EditorHost::request_reveal`，与 Quick Open `⌥↵` / 跳转定义同一条通道；
  未注入时按钮只发消息「宿主未接入导航定位」）。

**验证**：
- `references::tests`（7 例）：过滤三条件（非外键 / 无目标表 / 列不在可见列）；
  大小写不一致仍收；渲染顺序稳定；空输入；`key_label` 两种形状；
  `mapping_text` 两分支（有目标列 / 没目标列不编）；状态默认值 + 空文案。
- `panel::tests`（3 例）：`references_state_selects_first_key_and_drops_a_stale_selection`
  （回填默认第一条、长度不变保住用户选中、清单变空清掉）；
  `reference_source_needs_one_source_table_and_drops_hidden_columns`；
  `reference_tab_requests_metadata_and_renders_the_key_list`（切页签只入队一条
  `LoadReferences` 且载荷正确；有键时下拉与「打开目标对象」都在元素树里）。
- 门禁：check（result_panel / editor）0 error / 0 warning；result_panel **306**、
  editor results 45、ui-guard 4。`rds-workbench` 仍被并行线阻断（`config_center.rs`
  的 `Icon` / `IconName` 未导入、`workspace.rs` 的 `Shared::current_project` 缺失，
  均与本批无关）；本批在 workbench 的装配期改动已复检到「不再报 lifecycle 处错误」。

**已知缺口 / 有意偏离**：
- **引用行数据**未做：DBeaver 在面板内嵌一整份 `ResultSetViewer` 跑
  `navigateReference` / `navigateAssociation`；本仓按 `result-panel-references-plan.md`
  的 `B′` 方案（生成外键条件 SQL → 编辑器新开一份结果集）留待下一步，
  故本页目前只到「键 + 列映射」。
- **入向引用**（其它表 → 本表）未做：要扫 schema 内其它表的约束（N 次往返），
  与 DBeaver 的 `ARROW_LEFT_ALL` 那一类不对齐。
- **catalog 传空串**：`list_constraints(conn_id, "", schema, table)` 依赖
  `MetadataService::database_for` 的默认展开；多 catalog 驱动（PG 跨库）没有精确目录时
  可能读不到——本仓结果集的列注解只带 `schema.表`，没有目录层。
- **活动键不持久化**：DBeaver 把选中键写进虚拟实体属性 `ref-panel-associations`
  跨会话恢复；本仓面板状态只在本次面板生命周期内有效（换结果集即回落第一条）。


### 8.80 「查看引用行」：按当前行值另开一份结果集（方案 B′，2026-11-18）

**取证**（`ReferencesResultsContainer.java`）：
- `refreshKeyValues`（`:382-432`）：键选好后按父结果集的**选中行集合**取数，
  `dataViewer.navigateReference` / `navigateAssociation` 生成查询后执行；选中行为空则清空该区域。
  也就是说「查什么」由**当前行的外键值**决定，不是整表扫。
- 本仓不嵌第二份 `ResultSetViewer`（会把「一份结果集 = 一份 store / exec 上下文」的假设打破），
  按 `result-panel-references-plan.md` §5 的 `B′`：生成查询 → 编辑器**另开一份结果集**。

**本仓落点**：
- `references.rs` 新增纯函数：
  - `ReferenceValue { column, value, data_type }`（值是驱动原始文本，不是渲染后的 `—`）；
  - `sql_literal(value, data_type)`：三档 —— NULL（空串 / `NULL`）→ `NULL`；
    数值列 + 值确实是数字 → 裸拼（数值列被包成字符串时 PostgreSQL / SQL Server 会直接报
    `operator does not exist: integer = text`）；数值列上不是数字的值 → 加引号（不静默产出坏 SQL）；
    其余（布尔 / 日期 / 文本）一律加引号，单引号按标准**双写**（`O'Brien` → `O''Brien`）。
  - `reference_rows_sql(key, values)`：`SELECT * FROM 目标表 WHERE 目标列 = 值 [AND …]`，
    空值改 `IS NULL`（`= NULL` 永远不成立）；目标表为空、目标列没报 / 数量对不上、
    值数量与参与列不符 → `None`（不拼半条条件）。
- 面板 `ResultPanel::reference_rows_sql(cx)`：选中键 + 当前行 → 逐列取值。
  **参与列被隐藏也算拿不到**（DBeaver 只按可见属性收集），列号走
  `display_order()` 的显示位次 + 1（首列是行号槽），隐藏列自然不在显示序里。
- 页面：键下拉右侧加「查看引用行」钮（`IconName::Rows3`、id `editor-result-reference-rows`），
  四缺一（无键 / 无选中行 / 列被隐藏 / 该格无值）就置灰。
- 端口 `ResultRequest::OpenReferenceRows { sql }` → editor 分派
  `execute(ExecTarget::Statement(sql), ResultPlacement::NewSet)`：
  **原结果保持选中、引用行落在旁边新标签**（`ResultPlacement::NewSet` 的既有语义，
  `ResultEntry` 注释里的「别动我正在看的东西，新的那份放旁边」）。

**验证**：
- `references::tests`（+2 例）：`sql_literals_are_typed_and_escaped`（NULL 三态 / 数值裸拼 /
  数值列上非数字退回引号 / 布尔与日期加引号 / 单引号双写 / 裸拼去空白）；
  `reference_rows_sql_covers_the_three_shapes`（单列 / 复合两列 / `IS NULL` /
  缺目标列、值数量不符、空表名三种拒绝）。
- `panel::tests::reference_rows_use_the_selected_row_values`：无选中行 → `None`；
  选中第一行 → `SELECT * FROM customers WHERE id = 1`（数值列裸拼）；入队一条
  `OpenReferenceRows` 且载荷同一条 SQL；切到引用页后按钮真的在元素树里。
- 门禁：check（editor / workbench）**Finished 0 warning**；result_panel **309**、
  editor results 45、ui-guard 4。

**已知缺口 / 有意偏离**：
- 只按**单个选中行**取（DBeaver 支持多行选中后批量 `IN`）；本仓网格的多行选择目前只用于
  列多选 / 复制，未接到这一支。
- 目标表不带 schema 前缀（`ConstraintDetail.referenced_table` 是驱动原话，通常同 schema）：
  跨 schema 的外键在目标表另属一个 schema 时会查错库，需按驱动元数据补全，未做。
- 值取自结果集文本（不是按列类型反序列化）：二进制 / 时间带时区的值会按字面量传下去，
  与 DBeaver 的 `ColumnType` 绑定不同。
- 不做「执行前预览」：DBeaver 也是选中即查；本仓的差别是新结果集**换了标签**，用户能看见 SQL
  （标签悬停即原始 SQL）。

### 8.81 列浏览器清单「复制列名」+ 嵌套列判定（对齐 DBeaver MetaDataPanel，2026-11-18）

**取证**（`plugins/org.jkiss.dbeaver.ui.editors.data/.../controls/resultset/panel/metadata/MetaDataPanel.java`，
文案在同插件 `internal/ResultSetMessages.properties`）：

- 清单右键 `fillCustomActions`（`:234-252`）：先 `UIWidgets.fillDefaultTreeContextMenu`，
  再**只追加一项** `meta_data_panel_action_copy_column_text`（`:236`）；文案
  `:457 = Copy Column Names`。
- 该动作 `run()`：遍历 `getItemsViewer().getStructuredSelection().toArray()`，逐项若是
  `DBDAttributeBinding` 就取 `getName()` 拼接，**项之间 `"\n"`**；`text.isEmpty()` 时
  直接 `return`（不写剪贴板）；否则 `UIUtils.setClipboardContents(display, TextTransfer, text)`。
- 即：拷的是**清单当前的结构化选中项**（多选才有多行），不是整表全部列。

**本仓落点**：

- `grid/menu.rs`：`ContextAction` 新增 `CopyColumnNames`（**无载荷**——选哪些列是清单自己的
  多选集合，菜单纯函数不该知道界面状态）；`column_browser_menu_items` 首项摆「复制列名」
  （`:396` 起），网格右键 / 列头菜单的消费 match 末尾补忽略分支（该动作只属于清单入口，
  网格右键没有多选集合概念）。
- `column_browser.rs`：纯函数 `column_names_to_copy(columns, selected, clicked)` —— 右键列
  在选中集合里 → 按数据列序整集合逐行拼（`"\n"` 分隔）；否则只拷它自己。集合内越界下标
  跳过（搜索词把行过滤掉后旧下标可能失效）；结果空串则点击直接 return，不写剪贴板
  （对齐 DBeaver `text.isEmpty()` 早退）。`render_item` 渲染期算好文本（闭包活到比本帧长，
  只有此刻拿得到多选集合与列名）。
- 验证：`grid::tests::column_browser_menu_is_local_column_subset`（首项文案 + `ContextAction`、
  列头菜单不含该项）、`panel::tests::copy_column_names_joins_the_selection_with_newlines`
  （单选 / 多选 / 部分越界 / 全越界 / 空列清单五路）。门禁：result_panel **310**、
  check（editor / workbench）Finished 0 warning、ui-guard 4。

**嵌套列：本仓 N/A（不做，不造假）**

DBeaver 的清单是**树**：`TreeContentProvider.getChildren` 返回
`DBDAttributeBinding.getNestedBindings()`（`:221-230`），加载完成后
`itemsViewer.expandToLevel(2)`（`:315`）；`getObjectFont`（`:284-289`）只对
「无父且不可见」的顶层列用斜体，子节点永不斜体。

本仓不可表达，理由是**数据结构本身没有层级**：

- `crates/workbench_shell/src/model.rs:327` `ColumnAnnotation` 只有
  `source_table / source_column / comment / data_type / nullable / default_value`，
  没有 children 字段；
- 驱动侧 `crates/shared/src/meta.rs:46` `ColumnDetail` 同样扁平（`name / data_type /
  nullable / is_primary_key / is_foreign_key / references`）。

要造树得先让驱动把 `STRUCT` / 嵌套对象的子字段报上来（JDBC `DatabaseMetaData` 不提供，
DBeaver 自己靠 `DBDAttributeBinding` 从结果集元数据推断），属于**元数据模型改造**，
不是面板层能补的活。因此本仓清单恒为平表：一条列注解 = 一行，无展开箭头、无二级缩进。

**同批取证到的其余差异（事实陈述，未在本批实现）**：

- **额外列 `Description`**（`addExtraColumns` `:292-302`，值 = `entityAttribute.getDescription()`
  即列注释）：本仓清单行只显示「列名 + 类型名 + 眼睛」，注释仅参与搜索匹配
  （`filter_browser_columns` 的 `comments` 参数），不显示出来。
- **选中双向同步**（`:104-134`）：清单选中 → `presentation.setCurrentAttribute`（且要求
  `isAttributeVisible`）；presentation 选中 → 清单 `setSelection`，用 `updateSelection` 布尔防回环。
  本仓已有 `SelectColumn` / `ColumnSelectionChanged` 双向通道 + 面板镜像，口径一致。
- **过滤**（`refresh(force)` `:173-201`，忽略大小写包含匹配；ESC 清空 `:91-98`；
  `curAttributes` 逐元素引用比较无变化直接 return）：本仓 `filter_browser_columns` 同口径
  （额外把类型名与注释也纳入匹配），ESC 清空由 `List` 内置搜索框提供。
- **「生成 DDL」**（`contributeActions` `:203-215` 贡献 `generate.ddl.by.resultSet`）：本仓
  第 18 批已落地（§8.77 / §2.67），口径一致。

**已知缺口**：树形嵌套列 N/A（见上）；清单右键不提供「折叠 / 展开全部」（无树即无此动作）。
列注释已在本批补上（原缺口，见 §8.82）。


### 8.82 列浏览器清单的列注释第二行（对齐 DBeaver `addExtraColumns`，2026-11-18）

**取证**：`MetaDataPanel.addExtraColumns`（`:292-302`）给清单加了第二列
`Description`，值取 `entityAttribute.getDescription()`（即列的注释，来自表元数据），
没有注释时该格空白。

**本仓形态差异（有意）**：本仓右栏是**窄侧栏**（默认 320px，可拖到 192–640px），
三列摆不下（列名 + 类型名已占满）。因此注释走**列名下的第二行**，而不是第三列：
同一份数据、换形态，不是丢数据。

**落点**：
- `column_browser.rs` 纯函数 `browser_comment_text(comment: &Option<String>) -> Option<String>`：
  `None` / 空串 / 纯空白 → `None`（不占第二行）；有内容则去首尾空白后原样显示。
- `render_item`：行内容从「列名 / 类型名 / 眼睛」的水平三段改为
  「（列名 + 注释第二行）竖排 / 类型名 / 眼睛」，注释行 `debug_selector` =
  `editor-result-col-browser-comment`（缺省不渲染）。
- 注释来源不变：`ColumnBrowserDelegate::comments` ← 网格 delegate 的
  `column_annotations()`（`sync` 每帧重灌），即结果集的列注解，不是另开一次元数据查询。

**验证**：
- `panel::tests::browser_comment_skips_empty_and_blank`（`None` / 空串 / 空白 / 去首尾空白四路）。
- `panel::tests::column_browser_draws_the_comment_line_below_the_name`：有注释 → 该元素出现在
  渲染观测里；把两列注释都清空再画一帧 → 元素消失（反证「不画空行」）。

**已知缺口**：第二行是**单行截断**（`truncate()`，长注释看不全，DBeaver 的列可横向滚动）；
没有「按注释搜索后高亮匹配片段」。两者都属目视/交互细化，与本线「结构与行为」口径不冲突，
留待目视统一处理阶段。


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
