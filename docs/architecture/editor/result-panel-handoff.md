# 结果集表格模块（rds-result-panel）现状盘点与遗留清单（交接快照）

> 本文档是**独立会话交接快照**（2026-10-28 整理，2026-10-28 更新清理），给接手「结果集模块」的新会话直接使用。
> 模块长线设计见 [`result-panel.md`](./result-panel.md)（§1-§8 架构 / §8.17-8.20 迭代记录）；
> 模块 backlog 与编辑器线共享见 [`editor-backlog.md`](./editor-backlog.md)。
> 本文档只负责：**当前状态 / 未完成点 / 并行线协作约束 / 接手指引**，内容自包含。

---

## 1. 模块现状（截至 2026-10-28 清理批次）

### 1.1 形态与边界

- **Crate**：`crates/result_panel`（15 个源文件，见 §6 文件地图），版本 0.2.4。
- **形态**：结果面板 = **编辑器宿主内嵌**（`crates/editor/src/view/host.rs` 的 `v_resizable` 分栏，`bfcc94b6`），
  **全局底部 dock 已取消**——`result_dock.rs` 薄壳 / `sync_result_dock` / `RESULT_DOCK_HEIGHT` 已于 2026-10-28 清理移除
  （详见 §2.5 清理记录）。结果区仅当面板 `has_content()` 时渲染下半区，无内容时不占位。
- **依赖方向硬约束**：`result_panel → engine / gpui-kit / workbench_shell`；`editor → result_panel`；**绝不反向依赖 editor**
  （依赖编辑器的动作全部走 `ports.rs` 的 `ResultActionQueue` 入队，editor 事件路径 drain 分派）。
- **四行布局**（DBeaver Data Editor 形态，`d4d69eea`）：
  1. 第一行：结果集标签条（多结果集 Tab，可重命名 / 钉住）
  2. 第二行：工具栏（SQL 弹窗 / 筛选 / 转置 / 对比 / 提交撤销 / 复制 / 刷新 / 取段 / 导出 / 分析）
  3. 第三行：`左栏（网格/文本/记录竖条） | 内容区（网格/文本/对比） | 右栏（列浏览器/值详情/计算/分组/引用/执行SQL）` + **右缘竖条**（页面开合开关 + 页签图标）
  4. 第四行：状态栏（**内存 / 耗时 / 查询条数** 三项，其余信息已精简移除；两端左右栏开关）
- **左右栏丝滑**（`953b28aa`）：展开/收起 = `EffectTransition::width` 宽度过渡（160ms ease_in_out_cubic），容器始终挂载、内容按 show 挂载、epoch 递增换 id 重播、首帧免动画；记录视图图标 = `IconName::Form`（Lucide 字段条）。

### 1.2 已交付能力（近期提交链 + 2026-10-28 批次）

| 提交 / 批次 | 内容 |
| --- | --- |
| `9cb8dd0f` | 工具栏横跨顶部 + 取全部行按钮 |
| `a1d74cb1` | 状态栏对齐 DBeaver（文案调整 + 移除设置按钮） |
| `2d295be6` | 列浏览器清单迁移 gpui-kit `List`（含右键菜单挂行内容 div 修正 + sync 同步真实网格） |
| `d4d69eea` | 四行布局重构（DBeaver 式；左右栏开关在状态栏两端可完全收起） |
| `4373705c` | workbench_shell 类型样式偏好模型（`result_style.rs`，13 项）+ 文档 §8.20 |
| `953b28aa` | 左右边栏展开/收起宽度过渡动画 + 记录视图图标换 Form |
| `bfcc94b6` | 结果区拆回编辑器宿主内嵌（取消全局 dock） |
| 2026-10-28 #1–#8 | 见 §2.6 批次变更明细 |

### 1.3 验证基线（接手必须重跑）

```powershell
$env:RUST_MIN_STACK='268435456'; cd D:\RdataStation\RDS\RdataStation-v2
cargo test -p rds-result-panel --lib    # 基线：207 passed / 0 failed（含 rendered_ui_elements_survive 防回退）
cargo check -p rds-editor -p rds-workbench -p rds-workbench-shell  # 通过（workbench 7 warnings 为并行线遗留）
```

> 注意：整包 `cargo test -p rds-workbench` 被既有契约测试失败阻塞（非本线）：
> `shared_fields_are_whitelisted`（Shared 字段集白名单，并行线新增字段未登记）、
> `ui_size_constants_match_design`（右侧边栏 18rem vs 设计 320px）、
> `view_layer_has_no_raw_size_literals`（editor/view/host.rs 既有裸 `px(...)`）。本线回归以上面三条为准。

---

## 2. 未完成 / 遗留点

### 2.1 提交待办（代码已就位、已验证，未提交——因与并行线同文件混合）

> **2026-10-29 核对：本小节为历史性记录，相关代码已随后续提交全部入库**（settings 拆分 `b4791fa0`、result_panel 拆分 `4621c8fa`、并行线 editor 拆分 `727e79fe`），不再属于待办。保留小节作历史参考。

**类型样式设置渲染消费**（`4373705c` 之后的消费侧，代码完成并通过编译测试）：

| 文件 | 内容 | 混合情况 |
| --- | --- | --- |
| `crates/settings/src/{model.rs, registry.rs, lib.rs}`（+ `fields.rs` / `skeleton.rs` 相关） | 13 字段 + Default；13 个 Enum Slot / kind_tags / REGISTRY specs；get/apply 13 分支 | 与并行线（scratchpad/navigator 设置字段）同文件混合 |
| `crates/result_panel/src/grid.rs` | `cell_style(kind, prefs)` + `format_cell_text` 纯函数 + delegate `type_style` 字段/setter + render_td 消费 + 4 新测试 | 与并行线同文件混合 |
| `crates/editor/src/view/host.rs` | 构造同步 `settings.result_panel.type_style()` + `pub fn apply_result_style(&self, cx)` | 与并行线（Toolbar 组件化迁移）同文件混合 |
| `crates/workbench/src/view.rs` | `on_settings_changed` 钩子新增 `hosts_for_style` 遍历调用 `host.apply_result_style(cx)` | 与并行线（connection matrix / 组件统一化）同文件混合 |

- 生效链路（已实现）：设置页「结果集面板」分组 13 个下拉（即时生效）→ shell 模型 parse（未知值回退默认）→ delegate 类型样式（对齐覆盖 + 前景色角色）→ `format_cell_text`（NULL 占位 / 布尔映射 / 千分位·两位小数·科学计数，只改显示不改取值）→ workbench 设置变更钩子遍历 editor_hosts 即时推送。
- 设计记录：`result-panel.md` §8.20。

### 2.2 外线阻塞（非本线，接手需知悉）

- **settings `--lib` 契约测试编译失败（E0004 非穷尽匹配）**：并行线在 `registry.rs` 登记了 `navigator.show_chips` / `navigator.show_search` / `scratchpad.group_max_h` 三个 Slot，但 `lib.rs` 的 get/apply 分支未补齐。本线 13 项登记齐全，**不应替并行线补分支**；等并行线补完，settings 回归即全绿。

### 2.3 已知限制（有意为之，勿当缺口重做）

| 项 | 现状 | 说明 |
| --- | --- | --- |
| 侧栏动画宽度基准 | 按默认 rem 基准 16px 换算像素 | zoom 改 `rem_size` 会偏差；当前应用无 zoom 入口，风险低（`panel.rs` 注释在案） |
| 收起动画期间内容卸载 | 收起瞬间内容卸载，宽度平滑收缩（背景滑出） | 未做「内容滑到结束再卸载」的延迟卸载（vendor sidebar 的 hide_scheduled 模式）；要更细腻可后续加 |
| 左右栏宽度不可拖拽 | 固定宽（左 2.25rem / 右 20rem） | DBeaver 侧栏可拖宽；加拖拽需引入拖拽状态管理，未排期 |
| `analyze` 路径 | 分析/引擎内结果无列类型也无注释 | `ResultSet` 无类型字段；P1 已按「无注释」如实处理 |
| 右栏「引用」页签 | **backlog 空态**（`sidebar_empty` 占位） | 外键引用查看未实现；`ColumnDetail.references` 字段已就位但取数链路未接（见 §2.4 P3） |

### 2.4 功能遗留（按优先级排；现状已核对）

| 优先级 | 项 | 现状 / 期望 |
| --- | --- | --- |
| 高 | 列浏览器多选批量动作 | 清单已迁移 List（单选联动）；多选（批量隐藏/冻结/复制列名等）未做 |
| 高 | 列序拖拽 | 列浏览器/表头拖拽重排列序；`column_order` 状态与 `ColumnOrderChanged` 队列已就位，**UI 拖拽入口未接线** |
| 中 | 列宽持久化 | DataTable/TableState 列宽拖拽现成，但不落盘；换文档/重启恢复默认宽；待评估落 NavCache/会话 |
| 中 | COUNT 精确总数 / 跨帧统计缓存 / 小数分组 | §8.13 渲染改进批次的延后项（当前列统计按可见行现算、整数千分位） |
| 中 | 并排对比增强 | 双栏等宽 → 可拖拽分隔；右栏列宽/滚动独立记忆 |
| 中 | 行级叠加层收尾 | 锚点插入落点、本地行进统计/分组、fact 原生整表聚合逐行排除（§8.10 ④ 遗留）——**依赖 §2.4-A2 内存事实档接线** |
| 低 | P3 钻取关联数据 | 右键值→查关联表；`ColumnDetail.references` 已就位，取数链路现成（用户明确下期）；右栏「引用」页签当前为空态 |
| 低 | 执行上下文模型 | 物理连接/事务/取消 per-context 独立（下一版本规划，0.2.4 不实现）；**结果归属按文档分槽不变** |
| 低 | DBeaver 对标未实现项（backlog） | ① 撤销 / 重做（网格编辑历史栈，`RowHistory` 快照已就位）；② 二进制 / 图片单元格渲染（当前文本降级显示）；③ 多级分组表头（DataTable 单级表头）。均为对标 DBeaver 完整表格功能的独立 backlog，不在当前排期（2026-10-29 补录） |

### 2.5 清理记录（2026-10-28，脏代码已移除）

| 项 | 位置 | 处置 |
| --- | --- | --- |
| 孤儿结果 dock 薄壳 | `crates/workbench/src/panels/result_dock.rs`（137 行）+ view.rs `result_dock` 字段/创建 + `panels/mod.rs` 导出 + `ui_contract.rs` 快照项 | **整体删除**（实体创建后从不渲染/接线；2026-10-27 已拆回内嵌） |
| 死常量 | `crates/workbench_shell/src/ui.rs` `RESULT_DOCK_HEIGHT` | **删除**（`sync_result_dock` 停用后无消费） |
| 死函数 | `crates/editor/src/view/host.rs` `apply_filter` / `clear_filter` 薄包装 | **删除**（生产走队列 `clear_filter_via_window` / 面板自洽；无任何调用者） |
| 过时注释 | `host.rs` `drain_result_actions` / `result_panel` / `jump_to_error_site_with` 的「阶段 3 dock」措辞 | **更新**（形态已内嵌；drain 现由 `observe_in` 回调驱动） |
| 注释残留 | `grid.rs` bench「临时基准」；`host.rs` 重复注释行 | **改正式 / 去重** |
| 实验探针 | `panel.rs` / `host.rs` / `tests.rs` 的 `eprintln! PROBE`（#8 排查遗留） | **已移除**（上一批次） |

### 2.6 2026-10-28 批次变更明细（#1–#8）

| # | 项 | 内容 |
| --- | --- | --- |
| 1 | 状态栏精简 | 只显示 内存 / 耗时 / 查询条数（`grid.rs render_status_row` 新签名，写语句显示影响行数） |
| 2 | 状态栏防重叠 | 右段三项 `.min_w_0().truncate()` |
| 3 | 工具栏对齐 | 注释 switch / 分析 / 对比 等控件对齐 |
| 4 | 右栏页签图标 + 右缘竖条 | 5 个页签加 `.icon()`；新增 `render_right_edge_bar`（最右缘 sidebar 底色竖条，顶部开合开关 + 页签按钮） |
| 5 | 标题栏改名 | ⋯ 按钮 tooltip「重命名结果集」；改名确认/取消/选择/钉住按钮补 `request`（此前 push 断链——点改名没反应即此因） |
| 6 | SQL 文本框高亮可点击 | 工具栏最左侧 SQL 预览 Button（ghost + tooltip「点击查看完整 SQL（弹出窗口）」）；点击 → `OpenSqlDialog` 弹出窗口（§2.9） |
| 7 | 筛选框升级 | 常态内嵌 ▶ 运行 / 展开顶宽输入框；运行 = `select * from (原SQL) as _q where 词` 替换当前结果集；生成 SQL 在右栏「执行SQL」页签可见可复制 |
| 8 | 左栏稳定性 | **根因**：按钮 `on_click` 只 `queue.push` 不通知面板 → 宿主 `observe_in`（drain）不触发 → 动作滞留队列。**修复**：`ResultActionQueue::request(req, app)`（push + notify 面板）+ 宿主注入面板弱引用 + 全量按钮改 `request`。另：文本视图 TSV 拼接加指纹缓存（50k×8 首拼 120ms → 命中近 0），`set_data` / `set_local_snapshot` 显式清缓存 |

### 2.7 2026-10-28 微调批次（三态右缘 + 单页签 + 重命名键盘）

| # | 项 | 内容 |
| --- | --- | --- |
| a | 右侧开关回状态栏 | 右缘竖条顶部开关移除，改挂状态栏右端（`right_switch`，debug_selector `editor-result-right-sidebar-toggle`）；左侧开关在状态栏左端不变 |
| b | 右侧三态（比左侧多一个展开态） | 收起（竖条+右栏都不显）→ 窄展开（只显右缘竖条图标条）→ 宽展开（竖条+右栏内容）→ 收起。纯函数 `next_right_edge_state(edge, show)` + 新字段 `edge_bar_visible`；窄/收起态点页签图标或 `set_exec_sql` 自动升宽展开。图标：PanelRight（收起）/ PanelRightDashed（窄，`IconName` 有此变体）/ PanelRightOpen（宽） |
| c | 右栏横 tab 只显激活页签 | `render_right_sidebar` 的 TabBar children 从 6 全量改为只放当前页签 1 个（作标题用，点击无操作）；页签切换全靠右缘竖条图标（`render_right_edge_bar`）。`right_tab_from_index` 转测试专用 `#[cfg(test)]` |
| d | 表格标题设计补充 | 结果集标签条（C1 §8.14）遗留的「重命名输入框 Enter 提交 / Esc 取消」补齐：`div.on_key_down`（Bubble 阶段，kit Input 不消费 Enter/Esc 已验证 input.rs 无 key_down 挂载）→ 队列 `CommitRename` / `CancelRename`（变体已存在），与 ✓/✕ 按钮同端口口径 |

测试：新增 `right_edge_state_cycles_through_three_states`；result-panel 基线 206 passed / 0 failed。

### 2.8 2026-10-28 微调批次（kit 组件统一风格：展开输入框 / 列浏览器表格化 / 值查看器位置行）

| # | 项 | 内容 |
| --- | --- | --- |
| e | 展开输入框两内嵌组件 + 无边框（**用户纠正后**） | `render_filter_expand_bar`：`Input::bordered(false)` 无边框；**最左侧 = 展开**（prefix `IconName::Maximize2`，点击收起筛选条，标识本框为展开态）；**最右侧 = 运行**（suffix `IconName::Play`，wrapped 重查）。✕ 清除 / 收起 / wrapped SQL 预览保持。~~旧版曾把 ▶ 放 prefix（误解），已纠正~~ |
| f | 列浏览器表格化 + 内嵌操作行 | 清单上方加**表头行**（列名 / 类型 / 操作，`px_3/py_1/gap_x_1` 与 kit ListItem 行内边距一致，`border_b` 分隔）；行渲染仍在 `ColumnBrowserDelegate`（kit List + Button，眼睛 = 行内操作，右键菜单 / 双击定位 / 搜索 / 键盘导航全保留）——kit 组件统一风格，功能不丢 |
| g | 值查看器位置行补列号 | 第一行 = 值位置信息：`第 N 行 · 第 M 列`（行号槽 col==0 无列号）；本地 / 已删除标注接在位置后；第二行 = 大展示框（滚动值，保持） |

组件风格盘点（回应"左侧组件是否用 kit"）：列浏览器 = kit List/Button/Input；值查看器 = kit div 结构（无现成 kit 数值查看器，保留手排）；计算 / 分组 = kit 纯文本键值行；引用 = backlog 空态；执行SQL = kit 只读块 + Button。除值查看器（kit 无对应组件）外全部 kit 组件。

### 2.9 2026-10-28 微调批次二（SQL 预览弹窗：等效浮层 + kit Textarea 只读）

| # | 项 | 内容 |
| --- | --- | --- |
| h | SQL 预览改**弹出窗口**（用户纠正：不是工具栏下方展开条） | 工具栏 SQL 文本框点击 → `OpenSqlDialog` 端口 → host 走窗口句柄转手（`self.window.update(cx, ...)`，与 `BeginRename` 同模式）→ 面板 `open_sql_dialog(window, cx)`：置 `sql_dialog_open` + 懒建只读 `TextareaState` 实体（`TextareaState::new` 需要 window；SQL 变化 `set_value` 刷新）→ render 根**最后挂**浮层（绘制顺序在最上） |
| i | 浮层结构 | 半透明黑遮罩（`gpui::hsla(0.,0.,0.,0.45)`，theme.colors 无 scrim 字段）+ 居中卡片（680px，border + background）+ 标题「原始 SQL」+ ✕ 按钮 + `Textarea::new(&entity).readonly(true).h(360px)`。两路关闭：✕ 按钮 / Esc（`div.on_key_down` Bubble，`keystroke.key == "escape"`） |
| j | **不用 kit Dialog 的架构决策** | `Dialog::new(cx)` 需 `&mut App`；本面板 render 是**自定义 `render(&self, cx: &App)`**（host 手动调 `result_panel.read(cx).render(cx)`，见 host.rs v_resizable 分栏）——**render 期无 `&mut App`，无法 Dialog::new**；workbench 能直接用 Dialog 是因为其 render 是标准 `Render` trait（`&mut Context<Self>`）。故用**等效浮层 + kit Textarea**（Textarea 元素 render 期只需 `&Entity`），满足"弹出窗口 + 文本组件"口径。若未来面板改标准 Render trait 可换 kit Dialog |

清理：`ToggleSqlPreview` 变体 / `sql_preview_open` 字段 / `render_sql_preview_bar` 函数（工具栏下方展开条）**整体删除**，全仓零残留；`Textarea` / `TextareaState` import 走 `gpui_kit::component::input`。测试：result-panel 基线仍 206 passed / 0 failed。

### 2.10 2026-10-29（UI 回退核查 + 渲染断言测试防回退）

**回退核查结论：代码零回退。** git 工作区干净（HEAD `5fe75961`）；最近含结果集大改的提交 `330e0631`（feat(result_panel): 文本视图缓存、列序拖拽恢复、按钮断链修复与 SQL 预览，16 files +1104/−494）已包含全部本轮改动（`render_sql_dialog` / `sql_dialog_open` / `filter-expand-toggle/-run` 均在该提交 diff 内）；grep 复核全部 UI 标志物在库。gpui-kit 0.8.1 自带 `gpui_kit::test` 测试框架（`pub mod test`，lib.rs:128），已用于新增**渲染断言测试**防回退：

| # | 项 | 内容 |
| --- | --- | --- |
| k | 渲染断言测试 `rendered_ui_elements_survive` | `#[gpui_kit::test]` + `cx.update(gpui_kit::init)` + `add_window_view` + `Root::new(host)` + `draw/clear`；因 ResultPanel **无标准 `Render` trait**（自定义 `render(&self, cx)`），测试内定义 `PanelHost` 壳（仿 host.rs 9075 手动调 `panel.read(cx).render(cx)`）。断言：空面板根 `editor-result-root` 恒渲染；展开筛选条后两内嵌按钮（左 `-toggle` 展开 / 右 `-run` 运行）出现；`open_sql_dialog` 后遮罩 `editor-result-sql-dialog` 出现、`close_sql_dialog` 后消失 |
| l | **测试查询机制（实测，勿踩）** | `TestWindowExt.find/try_find` 只匹配**显式 ElementId** 元素（`Button::new(id)` / `.id()` / EffectTransition 包装）；纯 `div().debug_selector()` 的 div **不注册**（gpui-base `test_support::Observed::new` 断言 `Element::id().is_some()`）——`editor-result-root` 这类 div 必须用 `VisualTestContext::debug_bounds(&'static str)`（gpui 原生路径，paint 阶段注册）。故测试 `visible()` 两路兜底：id 元素走 `try_find().visible()`，div 走 `debug_bounds().is_some()`。**`gpui_kit::test_support` 不存在**（test_support 是 gpui-base 内部模块；gpui_kit 只 re-export `test::{TestWindowExt, ElementSnapshot, ...}`）；`debug_bounds` 参数要求 `&'static str`（闭包参数须标注 'static） |

基线：`cargo test -p rds-result-panel --lib` = **207 passed / 0 failed**；`cargo check -p rds-editor -p rds-workbench` 通过。测试装配模板参照 `crates/workbench/tests/mock_panel_probe.rs`。

---

## 3. 并行线协作纪律（工作区现状）

- 只 add 本线文件；`Cargo.lock` / vendor patch / `crates/insight/*` / `plugin/*` / `crates/app/src/main.rs` / `workbench/src/components/insight_actions.rs` / README 等并行线文件一律不随本线提交。
- 当前未提交（并行线在动，改前先 `git status` 确认归属）：
  - 本线混合：`result_panel/src/{store/, export/, grid/, panel/}`（2026-10-29 拆分目录）/ `ports.rs` / `host.rs` / `settings/*` / `workbench/src/view.rs` / `panels/mod.rs` / `tests/ui_contract.rs` / `workbench_shell/src/ui.rs`
  - 并行线：`database/src/nav_view/rows.rs`、`editor/src/{session.rs, view/dialogs.rs, encoding_dialog.rs, function_manual.rs, indent_dialog.rs, invisible_dialog.rs, tests.rs}`、`engine/src/persistence/workbench_context_store.rs`、`scratchpad/chrome.rs`、`workbench/src/{notification.rs, panels/editor.rs, quick_open/delegate.rs, services/editor_session.rs}`、`workbench_shell/src/ui.rs`、`.agents/skills/rds-design/`、`docs/architecture/{settings/settings-architecture.md, ui/design-spec.md, ui/theme-spec.md}` 等
- 注释 / commit 信息一律简体中文（`feat:` / `fix:` / `refactor:` 前缀）。
- 过程产物（日志 / 临时脚本）落 `.rds/tmp/` 或 `tools/_archive/`，不入库、不留工作目录。
- 未确定的 API / 设计决策先查 vendor（`vendor/gpui-component/src/`、`vendor/gpui-base/src/`，0.8.x 权威）或本地 MCP，禁止联网 / 凭记忆猜；不确定的交互口径先列出来问用户。

---

## 4. 接手指引（新会话建议顺序）

1. 先重跑 §1.3 三条验证命令，确认基线（尤其 **207 passed / 0 failed**）落地。
2. 处理 §2.1 提交待办：先 `git status` / `git diff` 摘本线 hunk，与并行线协调后统一提交类型样式渲染消费（settings 三文件 + grid.rs + host.rs + workbench view.rs）。
3. 知悉 §2.2 外线阻塞（settings 契约测试）与 §1.3 契约测试既有失败，不替并行线补。
4. 按 §2.4 优先级推进功能遗留（建议先「列浏览器多选批量动作」「列序拖拽」两个高优先级项，开工前先按模块惯例说理解、列方案）。
5. 每完成一批：跑 §1.3 基线 + 记录到 `result-panel.md`（新增 §8.2x 迭代小节），提交只 add 本线文件。

## 5. 常用锚点（行号随近期提交微移，以现场 grep 为准）

- `panel/`：`mod.rs`（门面 + pub 面 re-export：`GridPayload` / `ExportCtx` / `ResultSyncInput` / `ResultPanel` / `AnalysisItem`）；`render.rs` 主 `render` 组装；`sidebars.rs` `render_vertical_bar`（左缘竖条：网格/文本/记录/上一行/下一行）/ `render_right_edge_bar`（右缘竖条，页签图标区）/ `render_right_sidebar`（横 tab 只显激活页签）/ `render_browser_tab` / `render_filter_expand_bar`（展开条：左=展开右=运行无边框）+ `sidebar_width_anim` / `next_right_edge_state` 纯函数；`tabs.rs` `render_value_tab` / `render_stats_tab` / `render_group_tab` / `render_reference_tab`（backlog 空态）/ `render_exec_sql_tab` + `kv_row` / `fmt_stat` / `right_tab_from_index`；`dialog.rs` `render_sql_dialog`（SQL 预览弹窗浮层）；`events.rs` `toggle_value_sidebar`（三态）/ `toggle_transpose` / `open_sql_dialog` / `close_sql_dialog`（事件路径，需 window）+ `reorder_by` 纯函数；`lifecycle.rs` `new` / `sync`（投影同步）；`tests.rs` 尾部 `rendered_ui_elements_survive`（渲染断言防回退，见 §2.10）。
- `ui.rs`：`RESULT_VERTICAL_BAR_WIDTH`（2.25）/ `RESULT_SIDEBAR_RIGHT_WIDTH`（20.0）/ `RESULT_STATUS_BAR_HEIGHT`（1.5）/ `RESULT_TOOLBAR_HEIGHT`（2.25）。
- `ports.rs`：`ResultRequest` 全集（含 #7 / §2.9 新增变体 `RunFilterAsQuery` / `ToggleFilterExpand` / `OpenSqlDialog` / `CloseSqlDialog`；`ToggleSqlPreview` 已删）+ `ResultActionQueue`（`request` / `set_panel` / `push` / `drain`）。
- `grid/`：`delegate.rs` `ResultGridDelegate`（TableDelegate 主体实现 + 钩子类型 + RESULT_GRID_CONTEXT 等常量）；`cell.rs` `render_td` / `header.rs` `render_th` / `menu.rs` `context_menu`（trait 方法不可分块 → 顶层自由函数 + `table_impl.rs` 薄转发）；`status.rs` `render_status_row`（三项新签名）/ `ResultToolbar` / `ResultStatus` / `ResultControls` / `ResultPane`；`text.rs` `format_cell_text` / `view_rows.rs` `text_presentation` 指纹缓存 / `stats.rs` 列统计 / `edit.rs` 行编辑 / `tests.rs`（全量测试迁入）。
- `store/`：`store.rs` `ResultStore`（按文档分槽，多结果集 sets(document) + TabBar 标签；事实档 A2 未接线）+ `entry.rs` / `fact.rs`。
- `export/`：`mod.rs` 门面 + `formats.rs` / `encode.rs` / `table_name.rs`（editor 侧 `pub use result_panel::export::*` 通配，全 pub 面保持）。

## 6. 文件地图（crates/result_panel/src，2026-10-29 目录拆分后）

顶层单文件：`lib.rs` / `ports.rs`（请求 + 队列）/ `rowset.rs` / `sets.rs`（结果集标签条）/ `status.rs` / `ui.rs`（尺寸常量）/ `analysis.rs` / `changes.rs` / `column_browser.rs`（列浏览器 List 委托）/ `error_card.rs` / `script_card.rs`。

拆分目录（2026-10-29 一批完成，原巨型单文件按域切分；**trait impl 不可分块**处统一「顶层自由函数 + impl 薄转发」、跨文件私有方法 / struct 字段以 `pub(crate)` 暴露；`panel.rs` 无标准 `Render` trait impl（自定义 `render(&self, cx)`，host 手动调）故普通 impl 可切多块）：
- `store/`（原 `store.rs`）：`mod.rs` + `entry.rs` + `fact.rs` + `store.rs`
- `export/`（原 `export.rs`）：`mod.rs` + `formats.rs` + `encode.rs` + `table_name.rs`
- `grid/`（原 `grid.rs`，6556 行）：`mod.rs` + `delegate.rs` + `view_rows.rs` + `edit.rs` + `text.rs` + `menu.rs` + `stats.rs` + `cell.rs` + `header.rs` + `table_impl.rs` + `status.rs` + `tests.rs`
- `panel/`（原 `panel.rs`，3338 行）：`mod.rs` + `data.rs` + `lifecycle.rs` + `render.rs` + `events.rs` + `sidebars.rs` + `dialog.rs` + `tabs.rs` + `tests.rs`
