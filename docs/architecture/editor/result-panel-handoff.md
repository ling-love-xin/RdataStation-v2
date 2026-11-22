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
cargo test -p rds-result-panel --lib    # 基线：267 passed / 0 failed（含 rendered_ui_elements_survive 防回退）
cargo test -p rds-settings --lib        # 基线：35 passed / 0 failed
cargo test -p rds-ui-guard              # 基线：4 passed / 0 failed（含 no_new_hardcoded_color_or_pixel）
cargo test -p rds-workbench-shell --lib # 基线：14 passed / 0 failed
cargo test -p rds-editor --lib          # 基线：789 passed / 0 failed / 1 ignored（2026-11-18 复测）
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
>
> **2026-10-30 更新：类型样式扩展（背景色 5 项 + 字体样式 5 项，13 → 23 项）已落地并提交**——
> `result_style.rs` 新增 `BgColorPref`（6 语义角色浅底 + `Custom(String)` hex）与
> `FontStylePref`（Bold/Italic/Underline/LineThrough）；settings model / registry
> （Slot 10 变体 + key 映射 + kind_tags）/ entries（bg/font 两组 spec）/ service
> （apply/get 各 10 分支 / 分派 match）全链路接线；`grid/cell.rs` render_td 铺底
> （语义角色 `opacity(0.16)` / Custom 走 `Colorize::parse_hex`，优先级：类型背景 <
> 本地行 < 脏格 / 冲突格）+ 字体五档挂载。验证：`cargo test -p rds-result-panel --lib`
> = 213 passed、`-p rds-workbench-shell` = 13 passed、`-p rds-settings --lib` = 34 passed。

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

- **settings `--lib` 契约测试编译失败（E0004 非穷尽匹配）**：并行线在 `registry.rs` 登记了 `navigator.show_chips` / `navigator.show_search` / `scratchpad.group_max_h` 三个 Slot，但 `lib.rs` 的 get/apply 分支未补齐。本线 13 项登记齐全，**不应替并行线补分支**；等并行线补完，settings 回归即全绿。**2026-10-30：已随并行线合入解除，`cargo test -p rds-settings --lib` = 34 passed（含 `every_model_leaf_is_registered` 契约测试），类型样式 23 项登记齐全。**

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
| 高 | ~~列浏览器多选批量动作~~ | **已交付（2026-11-08，P2-3）**：`List` 只给单个 `selected_index` + `confirm(secondary)`，没有集合语义 → 集合由 `ColumnBrowserDelegate.selected_set` 自持（Ctrl 点击增删 / 普通点击单选替换，纯函数 `next_multi_selection`），行内容按集合自绘底色（`list_active`）；集合经 `ResultRequest::ColumnSelectionChanged` 回传面板镜像，标题行出现「已选 N 列」+「隐藏所选 / 显示所选 / 清空选择」三钮，批量显隐走 `SetColumnsHidden { columns, hidden }` → `delegate.set_columns_hidden`（隐藏仍保「至少 1 列可见」底线）。**冻结 / 复制列名批量未做**（多选只接了显隐，其余仍走单列右键） |
| 中 | ~~列宽持久化~~ | **已交付（2026-11-08，P1-3）**：列宽按列名随会话落库（`result_column_widths` 列 + `列名=宽` 逗号串编解码 + `TableEvent::ColumnWidthsChanged` → `save_session_now`），并受全局开关 `result_panel.persist_column_widths` 管 |
| 中 | COUNT 精确总数 / 跨帧统计缓存 / 小数分组 | §8.13 渲染改进批次的延后项（当前列统计按可见行现算、整数千分位） |
| 中 | 并排对比增强 | 双栏等宽 → 可拖拽分隔；右栏列宽/滚动独立记忆 |
| 中 | 行级叠加层收尾 | 锚点插入落点、本地行进统计/分组、fact 原生整表聚合逐行排除（§8.10 ④ 遗留）——**依赖 §2.4-A2 内存事实档接线** |
| 低 | P3 钻取关联数据 | 右键值→查关联表；`ColumnDetail.references` 已就位，取数链路现成（用户明确下期）；右栏「引用」页签当前为空态 |
| 低 | 执行上下文模型 | 物理连接/事务/取消 per-context 独立（下一版本规划，0.2.4 不实现）；**结果归属按文档分槽不变** |
| 低 | DBeaver 对标未实现项（backlog） | ① 撤销 / 重做（网格编辑历史栈，`RowHistory` 快照已就位）；② 二进制 / 图片**网格内单元格**渲染（网格内仍文本降级显示；**值查看器的图片解码已于 2026-11-08 交付**，P2-2）；③ 多级分组表头（DataTable 单级表头）。均为对标 DBeaver 完整表格功能的独立 backlog，不在当前排期（2026-10-29 补录，10-30 更新②，11-08 更新②） |
| 低 | **DBeaver 右栏对标剩余（R 系列，2026-11-09 现状）** | 已交付：R1（四页页内工具条，`531aca1c`）/ R2（计算页对齐 Calc，`592624f4`+`5857090d`）/ R3 第一步（分组页多列·排序·仅重复，`d748bc5e`）/ R5 第一步（可拖分栏，`13e057e9`），证据见 `result-panel.md` §8.40~§8.44。**剩余**：R4 = References 页（需外键元数据端口，引擎侧无）+ Metadata「生成 DDL」动作（无 DDL 端口）；R5 第二步 = `setOrientation` 布局方向切换（网格下方 vs 右侧）、`setWeights` 比例持久化、双击复位；R3 剩余 = 分组列集合跨会话持久化、每组展开看行、分组列顺序拖拽；R2 有意未做三项（§8.40.4）；P2 三项（筛选历史下拉 / 筛选设置对话框 / 查找替换浮层）用户登记在册、未开工 |

> **核查更正（2026-11-08，调用链五段核对）**：原表「高 · 列序拖拽 UI 入口未接线」**已不成立，故删除**。
> 实测全链路已闭合：`grid/status.rs:400 col_movable(true)`（组件开启）→ `panel/lifecycle.rs:93-99`
> 拖拽后推 `ResultRequest::ColumnOrderChanged` → `editor/.../dispatch.rs:200-203` 宿主 `save_session_now` 落库
> → `panel/events.rs:301 apply_column_order` + `lifecycle.rs:242` 换新结果时按列名重排
> → `editor/src/ports/editor_session.rs:82-116` 编解码为逗号串（含测试 `:333-359`）。
> 同时原「列宽持久化」条目的阻塞当时已解除（`editor_session.rs` 已稳定承载 `result_column_order`
> 编解码，列宽只需复用同通道加字段）——**该项已于同日 P1-3 落地**：`EditorContext.result_column_widths`
> + 老库幂等补列（`ensure_column`）+ `editor_session.rs` 的 `列名=宽` 逗号串编解码（测试
> `the_result_column_widths_travel_as_name_width_pairs`）+ 面板快照（`panel/lifecycle.rs` 订阅
> `TableEvent::ColumnWidthsChanged` → `ResultRequest::ColumnWidthsChanged` → 宿主 `save_session_now`，
> `set_data` 之后按列名回填）+ 全局开关 `result_panel.persist_column_widths` 读写两侧均判。

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

### 2.11 2026-10-30（值查看器类型分派 + 选格跟随修复）

DBeaver 值查看器类型感知吸收（类型 → 内容形态分派），组件全走 gpui-kit，提交 `9d5e0f8d`：

| 项 | 落地 |
| --- | --- |
| 分派纯函数层（cell.rs） | `ViewerMode`（Text / Json / Hex / Image）+ `classify_value`（Binary → 图片魔数命中 `Image{format,size}` / 否则 `Hex`；`{` / `[` 前缀 → `Json`）+ `detect_image_format`（PNG / JPEG / GIF / WebP 魔数）+ `hex_to_bytes` + `hex_lines`（offset·hex·ASCII 三栏，16 字节/行）+ `json_tree_items`（嵌套 JSON/数组展开树） |
| JSON 展开树 | `tree::Tree` 组件 + `TreeState`：`ResultPanel.json_tree` 字段；事件路径 `rebuild_value_tree` 解析 + `set_items` 重建，**render 只摆不解析**（纯读红线）；每节点子项上限 `JSON_TREE_CHILD_LIMIT = 500`，超出追加「… 还有 N 项」占位；顶层预展开 2 层；缩进 / 展开箭头行内自画（`ui::JSON_TREE_INDENT_STEP` / `JSON_TREE_ARROW_WIDTH`） |
| Hex 转储 | 等宽字体 + **按页翻页**（2026-11-08 P2-1）：每页 16 行 × 16 字节（256 字节），底部页栏「第 n/N 页 · 共 X 字节」+ 上一页 / 下一页按钮（`ResultPanel.hex_page` 纯视图态，`ResultRequest::HexPage` 回面板；到边界按钮置灰，换选中格复位到第 1 页） |
| 图片预览 | **已真解码显示**（2026-11-08 P2-2）：事件路径 `rebuild_value_tree` 把 hex 解成 `Arc<Image>`（`Image::from_bytes` 只做内存构造，解码在 `img` 的 layout 期按内容 id 走资产缓存）→ 面板 `value_image` → render 摆 `img(ImageSource::Image(..)).w_full().max_h_full()`；解码失败 `with_fallback` 回落文案，格式不在支持集回文本占位 |
| **Bug 修复** | `lifecycle.rs` 订阅补 `TableEvent::SelectCell`：选格即 `set_selection_pos` + `rebuild_value_tree`（此前只接双击——「点击文字半天才出来」根因之一）；`ClearSelection` 同步清空树 |

验证：`cargo test -p rds-result-panel --lib` = **213 passed / 0 failed**（新增 5 测试：classify 分派 / 魔数识别 / hex 分页 / JSON 嵌套树 / 大数组截断 + 非法 JSON 拒绝）。

### 2.12 2026-11-08（P2 批：值查看器 hex 翻页 / 图片预览 + 列浏览器多选）

| 项 | 落地 |
| --- | --- |
| P2-1 hex 转储翻页 | `panel/tabs.rs`：`HEX_LINES_PER_PAGE = 16` + `hex_page_clamp(total, page) -> (page, total_pages)`（空值也算 1 页、页号夹到末页）+ `hex_page_button`（ghost/small/icon/disabled 到边界即禁用）；`render_hex_dump` 改「本页 16 行滚动区 + 底部页栏（`第 n/N 页 · 共 X 字节` + 上一页 / 下一页）」。页号是面板自持纯视图态 `ResultPanel.hex_page`，按钮经 `ResultRequest::HexPage(usize)` 走队列——**tab 的 render 签名只有 `&App`、拿不到 `&mut Context`**，所以统一用 `queue.request(.., app)` 而不是 `cx.listener`；`set_selection_pos` 复位 `hex_page = 0`（换格不留在旧页）；`grid/cell.rs` 的 `HEX_DUMP_BYTES_PER_LINE` 提升为 `pub(crate)` |
| P2-2 图片解码预览 | `grid/cell.rs` 加 `image_format_of(name) -> Option<ImageFormat>`（PNG / JPEG / GIF / WebP → `ImageFormat::{Png, Jpeg, Gif, Webp}`）；`rebuild_value_tree` 在**事件路径**把 hex 解成 `Arc<Image>` 存 `ResultPanel.value_image`（`Image::from_bytes` 只做内存构造，真解码在 `img` 的 layout/paint 期按内容 id 走资产缓存 → render 纯读不破）；render 的 `Image` 分支只摆 `img(ImageSource::Image(..)).w_full().max_h_full()`（`ObjectFit::Contain` 是默认），解码失败走 `with_fallback` 回落文案、格式不在支持集回文本占位。**布局陷阱（gpui-pre 0.3.10 `elements/img.rs` request_layout）**：`style.size` 为 `Auto` 时会按真实像素尺寸写 size（4000×3000 的图会撑爆面板），必须显式给宽（`w_full()`）+ 高度上限（`max_h_full()`）才让 contain 生效 |
| P2-3 列浏览器多选 | 见 §2.4 表「列浏览器多选批量动作」行的落地说明；另加 `ResultPanel.multi_selected` 镜像、`panel/events.rs::{set_multi_selected, set_columns_hidden, clear_multi_selected}`、`ResultRequest::{SetColumnsHidden, ColumnSelectionChanged, ClearColumnSelection}` + `dispatch.rs` 三条分派；`sync` 换投影时按当前列数剔除越界列号后**回灌** delegate（`set_data` 会清 delegate 侧集合，不回灌则刷新一次就丢用户选集） |

验证：`cargo test -p rds-result-panel --lib` = **218 passed / 0 failed**（本批新增 4 测试：hex 分页夹取 / 图片格式映射链路 / 多选集合推进 / 批量显隐保底 1 列）；`cargo check -p rds-result-panel -p rds-editor` 通过（0 warning）。

### 2.13 2026-11-08（列隐藏 / 冻结落盘：盘点首选项）

| 层 | 落地 |
| --- | --- |
| 面板 | `ResultPanel.column_hidden` / `column_frozen`（`Vec<String>`，与 `column_widths` 同层同生命周期、按列名匹配）；`set_column_hidden` / `set_column_frozen` / `column_hidden()` / `column_frozen()`；`snapshot_column_layout(cx)` 读 delegate 快照 → 与面板字段比 → 真变了才推请求 |
| delegate | `hidden_column_snapshot` / `apply_hidden_columns` / `frozen_column_snapshot` / `apply_frozen_columns`（下标 ↔ 列名，找不到静默跳过；隐藏逐列 `hide_column` 守住「至少 1 列可见」；冻结去重排序）；新增字段 `action_queue: Option<ResultActionQueue>` + `set_action_queue` |
| 时序 | `panel/lifecycle.rs::sync` 的 `grid.update` 闭包内、`set_data` 之后依次回填 列宽 → 隐藏 → 冻结（必须闭包内做：同一 `grid.update` 里再 `grid.read(cx)` 会重入 panic） |
| 触发 | 新枚举 `ResultRequest::ColumnLayoutChanged`（无载荷）。列头菜单（`header.rs` 三项）/ 单元格右键（`menu.rs` 三项）/ 列浏览器右键（`column_browser.rs` 三项）就地改 delegate 后 `queue.request(..)`；`set_grids` 现在收 `cx` 并把队列注入主网格 + 对比网格；单列眼睛与批量显隐在面板方法内改完直接 `snapshot_column_layout` |
| 落库 | `dispatch.rs` 的 `ColumnLayoutChanged` 分支 = 先取面板快照 + `save_session_now`（列头 / 右键路径面板快照滞后，必须补取）；眼睛路径不再显式存，靠 drain 循环处理冒出来的请求，一次操作只写一遍 |
| 会话 / 存储 | `SavedSession` 两字段 → `host/session.rs` 保存 + 恢复 → `ports/editor_session.rs` 逗号串编解码 → `EditorContext` 两字段 → 建表 + `ensure_column` → INSERT `?1..?18` → 三处 SELECT + `row.get(15)/(16)`（`updated_at_ms` 顺移 17）→ `tests/channels.rs` 四处 fixture 补 `None` |

口径：不复用 `persist_column_widths` 开关（那个只管列宽）；隐藏 / 冻结与列序同语义，总随会话走，空则存 `None`。已知限制：列名含逗号会被拆错（与列宽串同款，只影响恢复）。

验证：`cargo test -p rds-result-panel --lib` = **220 passed / 0 failed**（+2：按列名往返换列序仍钉对列 / 回填全隐藏仍保 1 列）；`cargo test -p rds-store --lib` = **163 passed / 0 failed**（+1 三路 SELECT 同值）；`cargo test -p rds-editor --lib` = **751 passed / 0 failed / 1 ignored**；`cargo check -p rds-result-panel -p rds-editor -p rds-store` 通过（0 warning）。未验证：真窗口「隐藏 → 关窗重开 → 仍隐藏」的端到端。

### 2.14 2026-11-08（列宽自适应：盘点第二优先项）

| 层 | 落地 |
| --- | --- |
| 入口 | 组件早已提供：双击 resize handle → `TableState::fit_col` → `TableDelegate::fit_column`（trait 默认 `None`，本仓此前没实现，所以双击没反应） |
| 实现 | 新模块 `crates/result_panel/src/grid/fit.rs`；`TableDelegate::fit_column` 在 `grid/table_impl.rs` 薄转发。只回答**内容宽**，padding / min-max / `ColumnWidthsChanged` 全由组件做 |
| 宽度 | 表头 = 类型 badge(8px) + 列名 + 排序箭头 + `▽` + `gap_1`(4px)，不含列头菜单按钮；单元格 = 前 200 视图行最宽的展示串 + `px_2`(16px)；两层 padding 都要算（组件按 `RESULT_TABLE_SIZE` = XSmall 档再 +8px） |
| 口径 | 测"画出来的那串"（NULL 占位 / 千分位 / 布尔中文 / JSON 美化 / BLOB 摘要 / 300 截断）；等宽族用 `theme.mono_font_family`；行号槽返回 `None`；转置视图可自适应 |
| 落盘 | 零新增接线：`ColumnWidthsChanged` 已在 `panel/lifecycle.rs` 订阅 → 与拖拽同一条通道 |

采样上限 `FIT_SAMPLE_ROWS = 200`。`fitted_col` 只屏蔽双击后那一下拖拽，不会每帧重算。验证：`cargo test -p rds-result-panel --lib` = **221 passed / 0 failed**（+1）；`cargo check -p rds-result-panel -p rds-editor -p rds-store` 0 warning。未验证：真窗口双击端到端、真实字体下观感是否偏宽。

### 2.15 2026-11-08（类型默认列宽：盘点第三项）

| 层 | 落地 |
| --- | --- |
| 优先级 | `column_widths`（用户拖过 / 双击自适应过，按列名落盘）> 类型默认宽 > 128；用户动过就永久让位 |
| 口径 | 按 `ColumnKind` 归一化语义（`int` / `int4` / `bigint` 同档），不按驱动原名；`Unknown` 仍 128（「类型缺失一个像素都不变」） |
| 取值 | numeric 130 / boolean 96 / date·time 120 / timestamp 210 / uuid 300 / json 260 / binary 160（我们画 `BLOB · N B` 摘要，不照抄 tusk bytea 220）/ text 200 / unknown 128 |
| 落点 | `ui.rs` 8 个 `RESULT_COLUMN_WIDTH_*` 常量；`grid/table_impl.rs::default_column_width(kind)` 穷尽 match；`column()` 改 `column_widths.get(name).copied().unwrap_or_else(|| default_column_width(column_kind(col_ix)))` |
| 时序 | `set_column_types` 在 `set_data` 之后、`refresh(cx)` 之前（`panel/lifecycle.rs:313` / `:338`），`column()` 首次被问时类型已就位，不会「先 128 再跳」 |
| 例外 | 转置视图「字段 / 值」两列仍 128（混合类型无可用档）；行号槽不变 |

推导：`text_xs` = 12px，西文数字宽 ≈ 0.55 × 字号 ≈ 6.6px/字符，内边距合计 24px（自身 `px_2` 16 + 组件 XSmall 8）。验证：`cargo test -p rds-result-panel --lib` = **222 passed / 0 failed**（+1 `default_column_width_follows_type_until_the_user_drags`）；三 crate check 0 warning。未验证：真窗口初始观感、text 200 对中文长文本是否仍偏窄。

### 2.16 2026-11-08（类型感知单元格编辑：盘点第四项）

| 层 | 落地 |
| --- | --- |
| 已有 | 提交**校验**早就是类型感知的（`validate_edit_value`：整数 / 数字 / 布尔 / 日期 / UUID 各有闸口）；缺的是**编辑器** |
| Boolean | `true` / `false` 双态 `Button`（`.toggled()` 标当前值），点一次即定值提交 |
| Date | `DatePicker`（自带日历弹层 + 可手敲，格式 `%Y-%m-%d`）；`DatePickerEvent::Change` → `YYYY-MM-DD` 字符串 → 提交 |
| 其余 | `Input` 原样（Integer / Float / Decimal / Text / Uuid / Json / Binary / Time / Timestamp / Unknown） |
| 通道 | 两种新控件都走 `delegate.commit_edit_as(value)`：写值 → `commit_edit`，校验 / `CommitEditHook` / 脏格 / 冲突 / 撤销栈全不另开 |
| 落点 | `delegate` 新增 `edit_calendar`（与 `edit_input` 同款「一份复用」）+ `set_edit_calendar`；`render_edit_cell` 按 `column_kind(col_ix)`（含行号槽的组件列序）分叉，返回 `AnyElement`；宿主 `host/mod.rs` 建 `DatePickerState` + `subscribe_in(DatePickerEvent::Change)` + 注入 |
| 不做 | Time / Timestamp 不换时间字段（各驱动时刻格式不同，复合值转字符串会抹平原格式）；JSON 校验美化另一个量级；enum `Select` 缺「值 → 显示」映射来源 |

已知边界：日历**不预置当前值**（打开落在今天所在月，要预置得解析 `NaiveDate`）；布尔按钮文案用 ASCII `true`/`false`，不跟随单元格显示样式；转置视图只读、不进编辑。验证：`cargo test -p rds-result-panel --lib` = **223 passed / 0 failed**（+1 布尔通道）；`cargo check -p rds-result-panel -p rds-editor` 0 warning；`cargo test -p rds-editor --lib` = 761 passed / **2 failed**（升级报告 §7 第 232 行已登记的基线既有，与本次无关）。未验证：真窗口点日历 / 点布尔按钮的端到端。

### 2.17 2026-11-08（表头列名被裁修复：用户截图「结果集字段被遮挡」）

| 层 | 落地 |
| --- | --- |
| 现象 | 表头带里没有列名，只剩贴在上沿的一条彩色残线（badge 上半截）+ 右端下拉箭头；行号槽 / 数据行 / 筛选框正常 |
| 根因 | 表头带 26px、减 `table_cell_padding` 上下各 2px → **内容高 22px**；`render_th` 却把表头造 `v_flex` 两行，主行用 `size_full()` 抢满 22px，第二行（列头菜单）一挂，flex 先满足 `flex_none` 的 28px 菜单 → 主行被分到 **0 高** |
| 修法 | 主行 `size_full()` → `w_full()`；`head` 加 `justify_center`；列头菜单从第二行并入主行尾端（`flex_none` + `on_click` 守卫挡排序）；提前 return 分支同步 |
| 同批口径 | 菜单进流内就真实吃列宽（XSmall 实测 24×24），`grid/fit.rs` 新增 `RESULT_FIT_COLUMN_MENU` 计入测宽，否则双击自适应后列名照样被省略号吃掉（原文「不含列头菜单」作废） |
| 取舍 | 表头带只有 22px，`comment_line` 第二行必然被裁——**同轮已解决**（见 §2.18），此处留作当时判断记录 |

验证：`cargo test -p rds-result-panel --lib` = **224 passed / 0 failed**（+1 `header_main_row_keeps_a_readable_height`：真 `TableState` + `DataTable` 渲染后读 `debug_bounds`，断言主行高 14–22px、菜单与主行同行；旧实现下两条都挂）；`cargo test -p rds-ui-guard` = 4 passed / 0 failed（`ui.rs` 裸 px 基线 15 → 16）。未验证：真窗口目视、滚动时表头带稳定性、96px 布尔列扣掉菜单 24px 后列名剩余宽度。

### 2.18 2026-11-08（注释表头两行高度 + 表头尺寸口径收敛）

| 层 | 落地 |
| --- | --- |
| 问题 | 「注释」开关承诺"开启后列头变高（两行）"，但表头高度从来没跟着变：两行挤 22px 内容高，各被 `overflow_hidden` 裁一半，主行还被压到读不出字 |
| 抬高的口子 | `DataTable::header_row_height(impl Into<Pixels>)`（`vendor table/data_table.rs:153`）；组件侧 `header_band_height` = 该值 × 列组行数（`state.rs:807-818`），`page_item_count` 一并跟着算 |
| 实现 | `ResultGridDelegate::header_row_height()`：注释开启 → `Some(ui::RESULT_HEADER_COMMENT_ROW_HEIGHT)`（40px，内容高 36 = 两行各 18px）；单行 → `None`（组件默认 26px，默认视觉零变化）。`grid/status.rs` 渲染处把值交给 `DataTable`，视图不写死尺寸 |
| badge 字号 | `RESULT_HEADER_TYPE_FONT_SIZE`（11px）从死常量变生效：`render_th` badge 用它，`grid/fit.rs` 量符号宽也用它（两边同字号，否则自适应差一两个字符） |
| 死常量 | `RESULT_HEADER_HEIGHT`（2rem = 32px）零引用且与实现的 26px 不符，**删除** |
| guard | `ui.rs` 裸 px 基线 16 → 17（新增那个 `px(40.)`） |

验证：`cargo test -p rds-result-panel --lib` = **225 passed / 0 failed**（+1 `comment_header_row_gets_room_for_two_lines`：两态纯逻辑 + 真渲染几何 —— 主行 ≥14px、注释行 ≥12px 且在主行下方；不抬表头时注释行取不到 bounds）；`cargo test -p rds-ui-guard` = 4 passed；`cargo check -p rds-result-panel` 0 warning。未验证：真窗口 40px 表头观感、11px badge 可读性、开注释后可视行数少一行的体感。

### 2.19 2026-11-08（窄档列宽补齐 + 工具栏高 40px + 筛选框同档）

| 层 | 落地 |
| --- | --- |
| 布尔列 | `RESULT_COLUMN_WIDTH_BOOLEAN` 96 → **120**：96 扣掉组件 padding 8 + badge 19 + 两个 gap 8 + 列头菜单 24 = 只剩 37px 给列名（5–6 个西文字符，`is_active` → `is_a…`）。列宽要装的是表头，不是值 |
| 最小列宽 | `RESULT_COLUMN_MIN_WIDTH` 64 → **80**：64 时列名只剩 5px，连省略号都摆不下。最小宽只管兜住不可用态，可读性靠悬停 tooltip |
| 兜底语义 | `Column::min_width` 会把拖到的更小值抬上去；`default_column_width_follows_type_until_the_user_drags` 里「拖到 70」的断言改成显式表达这条兜底（故意留 70 钉住） |
| 工具栏高 | `RESULT_TOOLBAR_HEIGHT` 2.25 → **2.5**（36 → 40px）：36 正好等于 `Toolbar` XSmall 自身最小高（28 + p_1 8），容器上下零余量 |
| 筛选框档位 | 常态工具栏与展开态筛选条的 `Input` 都加 `.small()`（32 → 24px，与同排 24px 按钮同档，字号 12 → 14）；这也是「工具栏挤」的真实原因——32px 输入框塞在 36px 里只剩上下各 2px |
| 注释漂移 | `grid/status.rs` 那条「结果区已提到 10rem」过时注释改写为当前 20rem，并保留「压缩前先量网格剩余高度」的警告 |

验证：`cargo test -p rds-result-panel --lib` = **226 passed / 0 failed**（+1 `narrow_column_tiers_keep_the_header_readable` 算术护栏）；`cargo test -p rds-ui-guard` = 4 passed；`cargo check -p rds-result-panel` / `-p rds-editor` 均 0 warning。未验证：真窗口 40px 工具栏观感、24px 筛选框可点性、120px 布尔列多列并排的总宽影响。

### 2.20 2026-11-08（列头菜单改绝对定位 + 悬停显形，不再占列宽）

| 层 | 落地 |
| --- | --- |
| 问题 | 菜单自 §2.17 起并进主行流内当 `flex_none` 子元素，恒占 24px——成了窄列列名被挤没的主因（布尔列 96px 时列名只剩 37px），而它平时根本用不上 |
| 改法 | guard 改 `absolute().top_0().right_0()` 贴在主行右端之上（不参与流），默认 `invisible()`；主行挂 `.group("editor-result-th-hover-{col_ix}")`，guard 挂 `.group_hover(同名组, visible)`。先例：vendor `gpui-component/src/attachment.rs` 的悬停删除按钮 |
| 测宽 | `grid/fit.rs` 撤销菜单占宽；`crate::ui::RESULT_FIT_COLUMN_MENU` 删除（裸 px 基线 17 → 16） |
| 实测 | 未知类型列：128 − 组件 padding 8 − 组件自带排序图标槽 16 = 列名 **104px**（旧实现 80） |
| 取舍 | hover 时菜单压住列名最右 24px（全名有 tooltip 兜底）；菜单 24px 高于 22px 内容高，底部裁 2px。**不采用**「hover 才占宽 `w(0)`→`w(24)`」——列名会被横向挤走，跳动比遮挡显眼 |

验证：`cargo test -p rds-result-panel --lib` = **227 passed / 0 failed**（+1 `header_column_menu_floats_and_leaves_the_name_its_full_width`：列名宽 ≥ 96 且与主行差 ≤ 8，旧实现两条都挂）；`cargo test -p rds-ui-guard` = 4 passed；`cargo check -p rds-result-panel` 0 warning。未验证：真窗口 hover 手感、tooltip 与菜单是否打架、2px 裁切的图标观感；触屏入口靠列头右键菜单（同源 `column_menu_items`）未目视。

### 2.21 2026-11-08（结果集标签条抬到编辑器同档 32px）

| 层 | 落地 |
| --- | --- |
| 问题 | 用户"结果集标题比编辑器文件表矮"。实证是**差一个 size 档**，不是差几像素 |
| 取证 | 编辑器 = dock `TabBar::new("tab-bar")` 不写 size → `Size::default()`=Medium → Tab+Medium **32px**；结果集原为 `Size::Small` → Tab+Small **24px**，外层 `.h(1.5rem)` 还卡死 24（零余量）。gap/内边距/内高均为 12/12/30 vs 8/10/22，字号两档相同（14px） |
| 改法 | `sets.rs` 档位 Small → **Medium**；`ui::RESULT_TABS_HEIGHT` 1.5 → **2.0**（32px） |
| 附带事实 | dock 单面板标题条（`render_title`）是固定 30px——编辑器单文件 30 / 多文件 32。结果集恒为多标签形态，故对齐 32 |

验证：`cargo test -p rds-result-panel --lib` = **228 passed / 0 failed**（+1 `sets::tests::tab_strip_matches_the_editor_dock_tab_height`，真渲染读 `debug_bounds` 断言 32px，旧实现读 24 必挂）；`cargo test -p rds-ui-guard` = 4 passed；`cargo check` 两个 crate 0 warning。未验证：真窗口观感、tab 内「⋯」/钉住按钮在 Medium 12px 内边距下的排布。

### 2.22 2026-11-08（标签条按 DBeaver 补齐：图标 / 关闭 / tooltip / 拖拽重排）

用户指令「请你完整对齐 dbeaver 的设计」，四项一起落地（前一轮我只做了图标，被这句覆盖）。

| # | DBeaver 出处 | 本仓落地 |
| --- | --- | --- |
| ① 图标 | `MultiTabsQueryResultsContainer.java:68-77` `setImage(IMG_DATA_GRID)` | `RESULT_SET_ICON = "icons/table.svg"` 挂 `Tab::prefix()`（**不用** `Tab::icon()`：`tab.rs:871/:888-896` 会让 `max_width` 失效且不挂 children）。走资产路径而非 `IconName`（`default-icons.txt` 106 个无 table/grid，`AllAssets` 1818 个有） |
| ② 关闭 | 同 `setShowClose(true)` | `TabActions::on_close` → suffix 末尾 ghost 小按钮 → `ResultRequest::CloseSet` → `store.remove_set`。**有意偏离**：移除槽位而非留空白页；**只剩一份拒删**（下游按「至少一份」装配）；钉住不阻止关闭 |
| ③ tooltip | `:95-103` `setToolTipText(toolTip)` | `ResultSetTab.sql` 投影 → `Tab::tooltip()`。vendor 两层原本无 tooltip 入口，本轮在 `gpui-component/tab/tab.rs` 补 `tooltip: ComponentTooltip` 字段 + `pub fn tooltip`，复用上游同一套（动效/延迟都自带） |
| ④ 拖拽重排 | `SQLEditor.java:1518` `new TabFolderReorder(resultTabs)`；`:1541-1549`；`TabFolderReorder.java:35-45`（`ITEM_MOVE_EVENT` 带 oldIndex/newIndex） | `store.move_set(from,to)`（整份槽位搬，`active`/`compare` 同经 `reorder_index`）→ `ResultRequest::MoveSet` → `TabBar::on_reorder`。vendor：`gpui-base` 的 `TabReorder`/`TabDragGhost`/`Tab::on_reorder`，`gpui-component` 的 `TabBar::on_reorder` 每帧下发到每个标签 |

**vendor 私有 patch 记账（0.9.0，三文件）**：

- `gpui-base/src/tabs.rs`：`TabReorder`（`index`/`label`/`on_move`）、`TabDragGhost`（`Render` 预览视图）、`Tab::reorder` 字段 + `pub fn on_reorder`、`RenderOnce` 里 `on_drag`/`on_drop`。**`TabDragGhost` 用裸 `hsla`**：base 层拿不到主题色，是「零裸色值」的唯一显式例外。
- `gpui-component/src/tab/tab.rs`：`tooltip: ComponentTooltip` 字段 + `pub fn tooltip`；`reorder` 字段（`on_reorder` 由 Bar 下发，因为 `ix` 是每帧 `.ix()` 注入、构造期未知）。
- `gpui-component/src/tab/tab_bar.rs`：`on_reorder` 字段 + `pub fn on_reorder` + 渲染循环里 `tab.reorder = self.on_reorder.clone()`。

**验证**：`cargo test -p rds-result-panel --lib` = **240 passed / 0 failed**（+4 拖拽用例：`dragging_a_set_forward_moves_it_and_keeps_selection_on_it` / `dragging_a_set_backward_shifts_the_neighbours` / `reordering_remaps_the_compare_index` / `reordering_ignores_noops_and_out_of_range_targets`）；`cargo check -p rds-result-panel` / `-p rds-editor` 均 Finished 0 warning；`cargo test -p rds-ui-guard` = 4 passed。`debug_bounds` 只吃 `&'static str`（`test_context.rs:1032`），断言侧一律静态字面量。

**未验证（需真窗口）**：32px tab 内塞图标+徽标+「⋯」+关闭（+图钉）是否挤；tooltip 实际延迟与位置；拖拽手势观感（阈值、落点反馈、ghost 深色主题反差）——headless 无鼠标拖拽，只有编译级 + store 单测保证。

**未做的一项**：DBeaver 拖动时在落点画插入标记（`TabFolderReorder` 的 DnD 反馈），我们只有指针跟手的小胶囊；要做需要重写 TabBar 的绝对定位层，本轮判定代价高于收益。这是四项里唯一明确留下的差异。

### 2.23 2026-11-08（工具栏按 DBeaver 收形：纯图标 + 图标 toggle + 竖分隔）

用户「工具栏也可以对齐，我已经目测过了，标题的高度基本没有问题了」——标签条 32px 已验收，
本轮只收工具栏形态、**高度 40px 不动**。两个形态细节由用户「按照你的建议继续即可」确认：
右段纯图标 + tooltip、两个开关改图标 toggle。

**DBeaver 取证**：`ResultSetFilterPanel.java`（结果集那条工具栏的真身，不是另起一行）——
`:171`/`:192` 左段是多行筛选文本框占满宽；`:315` 右段 `ToolBar(SWT.HORIZONTAL|SWT.RIGHT)` 里
只有 4 个纯图标 ToolItem（`:319` 清除▾ / `:330` 自定义▾ / `:343` 历史后退▾ / `:348` 前进▾ /
`:356` 查找替换 = `SWT.CHECK` 按下态 switch）；分组靠 `:354` 的 `createToolBarSeparator`。
**整条没有带文字标签的按钮，也没有 switch**；提交/导出/刷新不在这一行。

**落地**（`panel/render.rs` + `grid/status.rs`）：

| 项 | 改前 | 改后 |
| --- | --- | --- |
| 右段按钮 | `提交变更 (N)` / `撤销变更` / `复制` / `⚗ 分析` 带文字 | 纯图标 + tooltip：`Save`（计数进 tooltip）/ `RotateCcw` / `Copy` / `Sigma`；对比保持 `Columns2` |
| 两个开关 | `Switch`「下发源库」「注释」 | `.ghost().small().toggled(..).icon(..).tooltip(..)`（`DatabaseZap` / `FileText`）。`Button::on_click` 不给新值 → 闭包捕获当前 bool 取反；请求载荷（`SetPushdown` / `SetShowColumnComments`）不变 |
| 分组 | 只有 `gap` | 两组之间 1px 竖线 `w(ui::HAIRLINE).h(rems(1.5)).bg(theme.colors.border)`，**两组都非空才插**。不用 `separator::Separator::vertical()`：它渲染成 `absolute().h_full()`（`separator.rs:80-84`），进 `Toolbar::contents` 流式槽会定位到外层祖先 |
| 高度 | 40px（§8.28） | **不动**（用户已验收；压缩会碰「虚拟列表裁剪丢 Input 焦点链」那条风险）——**该前提已被 §2.25 推翻**（用户实拍指出高度也没对齐，40 → 32px 且补 `tab_bar` 底，控件本身未动） |

功能与摆放条件一律不变：SQL 预览仍最左（保留 `SQL: <前 60 字>` 文字——它是信息位不是按钮标签）；
对比仍 `tabs.len() >= 2`、提交/撤销仍 `pending_changes > 0`（提交中置灰）、分析仍 `analysis_items` 非空。
`debug_selector` 全保留，`editor-result-pushdown` / `editor-result-show-comments` 是本轮**新加**（原先
`Switch` 侧没有）。

**验证**：`cargo test -p rds-result-panel --lib` = **240 passed / 0 failed**（测试集合未变）；
`cargo check -p rds-result-panel` Finished 0 warning；`cargo test -p rds-ui-guard` = 4 passed
（未动 `ui.rs` 常量，BASELINE 不变）。

`cargo test -p rds-editor --lib` = 764 passed / 3 failed / 1 ignored。新增的那条
`results_a::the_result_header_sorts_from_its_arrow`（`editor-result-th-1 应登记坐标`）经 A/B 归因
**非本轮引入**（把本轮两文件 `git stash` 后单跑同样 FAILED）；另两条是升级报告 §7 的基线失败。
当时工作树另有并行线未完成改动（`crates/editor/src/ports/editor_exec/` 迁移中），本轮不动它。

> **基线更新（2026-11-18，`6b945b8e` 之后复测）**：`cargo test -p rds-editor --lib` =
> **789 passed / 0 failed / 1 ignored**。上面那三条失败（含 `the_result_header_sorts_from_its_arrow`
> 在 `results_a.rs:290` 的 `window.draw` panic）**已全部不再复现**——单跑该用例 ok、
> 全量 789 条 ok。归因是当时并行线的工作树状态；现在这条口径作废，读数以 789 为准。
> 另：结果集相关的 45 条（`--lib results` 过滤）单独跑也是 45 passed。

**未验证（需真窗口）**：六个图标 + 一条竖线挤在 40px 行里是否过密；`Save`/`Sigma`/`DatabaseZap`
的语义是否一眼可读（靠 tooltip 兜底）；小图标上 `ComponentTooltip` 的延迟与位置。

### 2.24 2026-11-08（结果集配色槽：接 DBeaver 结果集专属主题色）

第四阶段由用户在交互选项里选定「结果集配色槽」，选项里写的副作用是「**全局生效**——所有表格都会
跟着变」，用户接受。**实际只落在结果集内**（走产品 token 而非 vendor 全局主题），因为 DBeaver 这批
色本身就是结果集专属的（`colorDefinition` 的 `categoryId` 是 `...presentation.results`）。这个差异
已写进 `result-panel.md` §8.33，交付时也向用户说明。

**DBeaver 取证**：`plugins\org.jkiss.dbeaver.ui.editors.data\plugin.xml:1144-1263` 的
`<colorDefinition>` 段逐行实读——`cell.odd.background` = 235,245,255（:1170）、
`cell.modified.background` = 255,228,181（:1184）、`cell.error.background` = 255,128,255（:1191）、
`cell.new.background` = 228,255,181（:1198）、`selection.background` = 175,215,255（:1156）。
**深色没覆盖**：`css\e4-dark-data-editor.css` grep `resultset|result.` 命中 0。

**落地四处**：

| 文件 | 改动 |
| --- | --- |
| `crates/workbench_shell/src/product_tokens.rs` | 3 个新角色（`result.odd_row.background` / `result.cell.modified.background` / `result.cell.error.background`）+ 3 个带 `unwrap_or_else` 回退的访问器 + 单测 `parses_result_color_slots_and_falls_back` |
| `assets/themes/product-tokens.json` | 明暗各三槽；`result.local_row_background` 由 `#F7E9E5`/`#382620` 换成 `#E4FFB5`/`#2A3A1E` |
| `crates/result_panel/src/grid/cell.rs` | 脏格 / 冲突格底色改走 token（闭包外先求值，避免 `cx` 借用打架） |
| `crates/result_panel/src/grid/table_impl.rs` | 新增 `TableDelegate::render_tr`：奇数行铺斑马底色（本 crate 唯一一处 delegate 原生绘行） |

**关键机制（源码级，非推测）**：`table/state.rs:2257-2258` 存 delegate 的样式快照、`:2277` 组件自己
铺全局 `tokens.table_even`、`:2278` `.refine_style(&style)` 重放快照；`refine` 的语义是
**`Option` 字段 `Some` 即覆盖**（`gpui-base/src/styled.rs:79-82` → `gpui-refineable` 生成代码
`derive_refineable.rs:158-176`）。所以 delegate 的 `bg` 胜出，hover 仍归组件。

**行为变化（用户此前未被告知）**：本地新增/复制行底色从品牌珊瑚浅底变浅绿（对齐 DBeaver
`cell.new.background`）。待确认。

**深色一套是推导值**：`#1C2129` / `#3A3120` / `#4A2131` / `#2A3A1E`，按深色背景等亮度比推导，
`product-tokens.json` 的 `$comment` 写明「非照抄」。待确认。

**验证**：`cargo check -p rds-result-panel` Finished 0 warning；
`cargo test -p rds-workbench-shell --lib` = 14 passed；
`cargo test -p rds-result-panel --lib` = **241 passed / 0 failed**（新增
`odd_rows_paint_the_stripe_from_the_delegate_row_container` 锁「`render_tr` 真的被调用」，
行容器挂 `debug_selector("editor-result-row-odd")`——取不到就说明配色槽空转）；
`cargo test -p rds-ui-guard` = 4 passed（`PRODUCT_ROLES` 补 3 键，否则键名写错会被静默丢弃，
正是这道门禁拦的对象）。

**未接的槽**（DBeaver 有、本轮没做）：`selection.background/foreground`（仍走组件 `table_active`）；
`cell.matched.background`（本仓无筛选命中高亮）；`lines.normal`/`lines.selected`（每格网格线，
`bordered(true)` 只有最外圈整框）；`header.*` 四槽；`preview.background`；
`cell.deleted`/`cell.readonly`（删除行仍灰字 + 删除线，不铺红底）；`null.foreground` 与数据类型色。

**未验证（需真窗口）**：`#EBF5FF` 斑马底在深色行底线间的观感；脏格 `#FFE4B5` 与本地行 `#E4FFB5`
相邻时会不会糊；冲突格 `#FF80FF` 全饱和洋红实色铺底是否刺眼（DBeaver 原值如此，未降饱和）。

### 2.25 2026-11-08（工具栏背景 + 高度对齐 DBeaver：40px → 32px，补 `tab_bar` 底）

用户实拍：「结果集的工具栏似乎没有对齐 dbeaver，如背景、高度等等」——**推翻 §2.23 里「高度 40px
不动」的前提**。

**两个根因**：

1. **背景**：`render_toolbar` 整条没有 `.bg()`，底色靠父容器透传（面板背景 = 白）。而 DBeaver 的
   筛选条与上方 TabFolder 同属「非活动标签 / widget」背景档（`UIStyles.getDefaultWidgetBackground()`
   → 主题 `INACTIVE_TAB_BG_START`，回退系统 `COLOR_WIDGET_BACKGROUND`；`UIStyles.java:122-129`），
   网格才是列表底白（`getDefaultTextBackground()` → `COLOR_LIST_BACKGROUND`；`:131-133`）。
   结果：我们标签条（`#F8F8F8`）与工具栏（白）断层，DBeaver 是连续一条。
2. **高度**：40px 的来历是「筛选框 `Input` 还是 Medium 档 `h_8`(32) + `Toolbar` 的 `p_1` 上下 8」。
   `Input` 已降到 `.small()`(24)，那 8px 没回收。

顺带纠正文档里流传的错算术「Toolbar XSmall = `min_h_7`(28) + `p_1`(8) = 36px」：`min_h_7` 是
**最小总高**约束（vendor `toolbar.rs:253`），右段控件真实高 24px（vendor `button.rs:688-700`），
加 `p_1` 8px → **自然高 32px**。

**改动两行**：

- `ui.rs`：`RESULT_TOOLBAR_HEIGHT` `2.5` → **`2.0`**（32px，与 `RESULT_TABS_HEIGHT` 同值）
- `grid/status.rs`：容器加 `.bg(theme.colors.tab_bar)`

**验证**：`check` 0 warning；`rds-workbench-shell --lib` 14 passed；`rds-result-panel --lib`
**242 passed**（新增 `grid::tests::the_result_toolbar_keeps_the_tabs_height`，真实布局量出 32px）；
`rds-ui-guard` 4 passed。网格可用高度 +8px，控件一个像素没动（不触发 §2.10 记的 Input 焦点链风险）。

**当时未做**（其中前两项已由 §2.26 补上，第三项改判为「视觉等价、不算缺口」）：筛选区整圈 1px 边框
（`CompositeBorderPainter`，需自绘容器包住筛选组）；面板四周 3px 外边距（当时 `px_2` = 8px、无垂直
内距）；图片分隔符（`UIIcon.SEPARATOR_V`）；DBeaver 筛选条真实像素高未在真实例上量过（本仓无可运行
DBeaver）。

### 2.26 2026-11-08（工具栏第二轮：筛选区外框 / 3px 内距 / 吃满宽 / 右段去内距）

用户「先按 A」（底色 `#F8F8F8` 不动）＋「工具栏的调整对齐要赶紧对齐」。

| # | 项 | DBeaver 依据 | 改后 |
| --- | --- | --- | --- |
| 1 | 工具栏水平内距 | `marginWidth = 3`（`ResultSetFilterPanel.java:165`） | `ui::RESULT_TOOLBAR_PADDING` = `px(3.)`，`.px()` 用它 |
| 2 | 筛选区一整圈 1px 框 | `CompositeBorderPainter`（`:182` / `.java:34-39`） | 容器 `border_1 + border_color(colors.input) + rounded`；`Input.appearance(false)` 让出边框 |
| 3 | 筛选区底 | `COLOR_LIST_BACKGROUND`（`UIStyles.java:131-133`） | `colors.background`（白） |
| 4 | 宽度分配 | 左列 `FILL_HORIZONTAL`（`:180`） | 左段 + 筛选容器 `flex_1 + min_w_0`；`RESULT_FILTER_WIDTH` 只剩重命名框消费 |
| 5 | 右段 ToolBar 内距 | 右列 ToolBar 无内边距 | `.p_0()`（`refine_style` 在 `p_1` 之后，vendor `toolbar.rs:253-257`） |

**高度没新增常量**：`Input.small()` 24 + 上下边框各 1 = **26px**，32px 工具栏里居中 → 上下各
3px = DBeaver `marginHeight = 3`。工具栏**只加水平 px、不加 py**：右段 `Toolbar` 有 `min_h_7`(28)
托底，压到 26px 会裁它的内边距。

**取舍（需知情）**：`Input.appearance(false)` 会一并去掉输入框自身的**焦点环**，焦点改由 caret
指示（DBeaver 的框同样不随 focus 变色）。觉得太弱就回退两处：去掉 `.appearance(false)` + 撤掉
容器边框。

**验证**：check 0 warning；result_panel **242 passed**（断言加在既有
`panel::tests::rendered_ui_elements_survive`，真实面板量出外框 26px）；workbench-shell 14 passed；
ui-guard 4 passed（`BASELINE` 从 `(0, 16)` 调到 `(0, 17)`，因新增 `px(3.)`——首轮是门禁先红的，
按设计生效）。

**「图片分隔符」改判为已对齐**：DBeaver 用图片是因为 SWT `ToolItem` 画不了线，本仓 `div` 1px 竖线
是同一视觉结果。

### 2.27 2026-11-08（表头底色收回 widget 档：去掉第四层）

§2.25 / §2.26 之后工具栏与标签条同为 `#F8F8F8` / `#181818`，但表头还是 `#F5F5F5` / `#252526CC`，
面板灰阶于是有**三层**（chrome → 表头 → 网格白）；DBeaver 只有**两层**。

**依据**：`header.background` = `COLOR_WIDGET_BACKGROUND`（`plugin.xml:1240`），与筛选条 /
TabFolder 同一档（`UIStyles.getDefaultWidgetBackground()`，`UIStyles.java:122-129`）；只有数据区
才是 `COLOR_LIST_BACKGROUND`（`:131-133`）。A 方案下本仓 widget 档 = `tab_bar.background`。

**改动两个值**（`assets/themes/rds-theme.json`，`table.head.background`）：

- light `#F5F5F5` → **`#F8F8F8`**（= `tab_bar.background`）
- dark `#252526CC` → **`#181818`**（= `tab_bar.background`）

`table.head.foreground` 不动：底变亮后 `#616161` 对比度更高，不触发 WCAG 回归。值改在主题资产、
`DataTable` 全局生效（结果集 + Mock 预览表），符合「全局统一、不做 per 结果集覆盖」口径，也与
DBeaver（任何表头都是 widget 底）一致。

**验证**：JSON 严格解析通过并回读两值；result_panel **242 passed**、workbench-shell **14 passed**、
ui-guard **4 passed**（用例数与基线一致，纯配置值不动几何）。

**仍未做**：表头下边线仍是 `table.row.border`（`#E1E1E1`），DBeaver `header.border` 是
`COLOR_WIDGET_DARK_SHADOW`（更深，`plugin.xml:1261`）——表头行边框在 `table/state.rs` 内部，
`TableDelegate` 只有 `render_th` 内容槽，**未取证到外部覆盖入口**，登记未做。

### 2.28 2026-11-08（表头分隔线：`render_header` 这个入口找到了）

§2.27 里「表头线没有外部入口」那条**当场被推翻**：`TableDelegate::render_header`
（`vendor table/delegate.rs:64-70`，默认空壳 `div().id("header")`）返回的 `Stateful<Div>`
会被组件 `.refine_style(&style)` 重放到默认样式**之后**（`table/state.rs:2056` 取快照 / `:2069` 重放），
所以 delegate 给的 `border_b` 能落地。之前只看 `render_th`（内容槽）就判「无入口」，取证面不够。

**改动**（`crates/result_panel/src/grid/table_impl.rs`）：

```rust
fn render_header(...) -> Stateful<Div> {
    div().id("editor-result-header-band")
        .debug_selector(|| "editor-result-header-band".to_string())
        .border_b(ui::HAIRLINE)
        .border_color(cx.theme().colors.border)
}
```

- 色取 `colors.border`（`#D4D4D4` / `#3C3C3C`）而非 `table.row.border`（`#E1E1E1`）：表头线必须比行底线深一档；
  本仓无 `COLOR_WIDGET_DARK_SHADOW` 语义面，要更强须新增槽（未做）
- 不叠线：数据行只画下边（`state.rs:2274-2276`）、表头自身无边框
- 表头带高不变；1px 从内容区吃，单行主行 18 → 17px，仍在既有 `>=14 / <=22` 断言区间

**验证**：result_panel **242 passed**（哨兵断言钉住 hook 真在跑——不跑时边框静默缺失，与「没入口」分不清）、
ui-guard **4 passed**、workbench-shell **14 passed**。

**仍未接**：`header.selected.background`（`state.rs:1606` `render_col_wrap`）与 `lines.normal`/`lines.selected`
（每格网格线，单元格包装层）——组件内部绘制，`TableDelegate` 无槽，要接必须改 vendor。

> **2026-11-17 修正**：`lines.normal`（列竖分隔线）**不需要改 vendor**——把列的左右内距置零
> （`grid/table_impl.rs::cell_paddings`）、由单元格自己画 6px 内距后，单元格盒子撑满整列宽，
> 在它身上加 `.border_r(ui::HAIRLINE)` 就正好落在列边界。已落地，见 §2.41 / `result-panel.md §8.51`。

### 2.29 2026-11-08（深色 resultset 配色其实有整套 DBeaver 覆盖 + 两处修正）

**推翻旧结论**：此前一直按「DBeaver 深色 css 未覆盖 resultset 色，dark 一套只能推导」处理。实读发现
`dbeaver-ref\plugins\org.jkiss.dbeaver.ui.editors.data\css\e4-dark-data-editor.css`（2023B）**整份覆盖**，
该插件 `css\` 下只有这一个文件 —— 也就是说**浅色确实没有覆盖**（§2.27 的浅色结论成立），错的只是「深色也没有」。
覆盖里含 `header.background #394248`、`header.border #696969`（与 `lines.selected` 同值）、`selection #505050/#F4F4F4`、
`cell.odd #252525`、`cell.modified #963000`、`cell.error #800000`、`lines.normal #1e1e1e`、`header.foreground #E0E0E0`、
`header.selected.background #5A5A7C`、`null #646464`、数据类型前景（binary #808000 / boolean #808080 /
datetime #5CA259 / numeric #C0C0FF / string #D0D0D0）。全表见 `result-panel.md` §8.38。

**本轮两处修正**：

1. 深色 `table.head.background` `#181818` → **`#394248`**（原值）。§2.27 的「表头 = widget 档」只在浅色成立。
2. 新增产品 token `result.header.border`（light `#D4D4D4` 保持现值 / dark **`#696969`** 原值），
   `table_impl.rs::render_header` 改用它（缺失回退 `colors.border`）。同步把该键写进
   `ui_guard` 的 `PRODUCT_ROLES` 声明表（漏了会被 `product_tokens_use_declared_role_names` 拦下——本轮真踩到）。

**为什么三个色槽（odd/modified/error）仍不照抄**：DBeaver 深色那几个是纯饱和深色（`#963000`、`#800000`），
在 `#1F1F1F` 网格上与未改动行几乎分不开；本仓保持按深底可读性取的较浅值，**有意偏离**，已写进 `$comment`。
要照抄得连前景色和网格线整套换。

**顺带**：筛选区外框去掉 `rounded(theme.radius)` —— `CompositeBorderPainter` 画的是 `drawRectangle` 直角矩形
（`CompositeBorderPainter.java:34-39`），圆角与 DBeaver 框形不一致。

**验证**：两个主题 JSON 严格解析回读通过；shell **14** / panel **242** / guard **4** passed。

### 2.30 2026-11-09（工具栏组件逐档对齐 DBeaver：P0 尺寸档 / P1 列序 / P3 多行框）

`result-panel.md` §8.39 是完整证据，这里只留接手要点。

**关键推翻**：此前写「DBeaver 工具栏总高无常量、32px 只是推导」是错的。它没有**单个**常量，
但四圈 margin + 一个显式行高就撑出确定值：外层 `marginHeight 3`（`:164`）+ `filterComposite`
`marginHeight 2`（`:175`）+ `MIN_FILTER_TEXT_HEIGHT 20`（`:103`）+ `2` + `3` = **30px**。
本仓 `RESULT_TOOLBAR_HEIGHT` `2.0` → **`1.875`**，工具栏与标签条（32px）**不再等高**。

**三档改动**：

1. **P0 尺寸档**（`a4574c83`）：15 处 `.small()` → `.xsmall()`（`Size::input_h` 的 `XSmall = h_5`
   = 20px，`sizing.rs:281-289`）；筛选容器补 `.p(ui::HAIRLINE)`（DBeaver 框线不占布局，本仓边框占 1px，
   再垫 1px 才等价）→ 容器 24px；两条量像素断言同步 26→24（panel）/ 32→30（grid）。
2. **P1 列序**（`4f8acd47`）：`[展开][文本][执行][清除]` 紧排、去 `gap_1`（DBeaver
   `GridLayout(5)` + `horizontalSpacing = 0`，`:174-179`）；展开钮从行尾移文本列左侧且不随展开分支；
   执行钮从 `Input.suffix` 拆成独立列（也顺便为 P3 铺路——`Textarea` 没有 suffix 槽）。
   清除钮留末位是有意偏离：DBeaver 的 ERASE 在右列 ToolBar，本仓右段是结果集动作，语义不同。
3. **P3 多行框**（与本节同一提交）：`Entity<InputState>` → `Entity<TextareaState>`（`data.rs`）、
   `TextareaState::new` + `set_submit_on_enter(true, cx)`（`lifecycle.rs`）、
   渲染处 `Textarea::new(..).xsmall().appearance(false).h(rems(1.25))`（工具栏 1 行）与
   `.h(rems(6.25))`（展开条 5 行，100px）。行高常量 `LINE_HEIGHT = Rems(1.25)` = 20px
   （`input.rs:543`）正好等于 `MIN_FILTER_TEXT_HEIGHT`，故 5 行 = 5×20 逐像素相等。
   展开条原来靠 `Input` 的 prefix/suffix 内嵌两个钮，`Textarea` 两个槽都没有 → 改成同行兄弟元素。

**新增哨兵**：`panel/tests.rs` 展开条高断言 **100~115px**（≈109；退回 1 行就红）。

**仍未做**：筛选历史下拉 / 筛选设置对话框 / 查找替换浮层（P2）；「展开」仍是右栏另起一条
（用户早前确认的功能，非缺口）。

**验证**：panel **242** / guard **4** / shell **14** passed。

### 2.31 2026-11-09（右栏 R1 面板工具条 + R2 计算页对齐 DBeaver Calc）

`result-panel.md` §8.40 是完整证据，这里只留接手要点。

**用户拍板**：R1 **不搬**页签并列（2026-10-28「横 tab 只显当前页签」的决定不撤销），只加
每面板工具条；并直接做 R2。R1 本轮只落了计算页（另四页未加工具条）。

**R2 做了什么**（`AggregateColumnsPanel.java` 全 498 行实证）：

1. 函数集可变：`StatsItem{Count,Nulls,Min,Max,Avg,Sum}` + `ALL`，
   `ResultRequest{AddStatsItem,RemoveStatsItem,ResetStatsItems}`；`add_stats_item` 按 `ALL`
   固定顺序插入（不是追加），删了再加回原位。
2. 聚合口径：`StatsMode{Numeric,Text}` + `SetStatsMode`，对齐 `aggregateAsStrings`（`:348`）：
   字符串口径 Min/Max 走字典序（`grid/stats.rs::text_minmax`），Avg/Sum 出 `—`。
3. 千分位：`grid/stats.rs::fmt_grouped`（对齐 `###,###,###,###,###,##0.###`，`:65-66`）；
   `panel/tabs.rs::fmt_stat` 改走它。
4. 表头两列（函数 / 值）+ 行尾 ×（替代 DBeaver 的「行选中后点 −」）。
5. 面板工具条（R1 首处）：`Toolbar` 收「＋ / Reset / 口径」；新常量
   `ui::RESULT_PANEL_TOOLBAR_HEIGHT = 1.75`（28px）——**必须**等于 vendor `toolbar.rs:242-259`
   XSmall 档自带的 `min_h_7`，写 24px 会溢出压住表头。

**三处有意偏离**：不做行选中删除（改行尾 ×）；不做「按列分组聚合」（本仓网格只有
`selection_pos` 单格选区，且已有独立分组页签）；不做右键 Copy/CopyAll。

**踩坑**：`StyledExt` 没带 `.when(..)`（`gpui_kit::Div` / `Scrollable` 都没有这个方法，
编译 `E0599`）——条件元素改用 `bool::then(|| …into_any_element())` + `children(Option<..>)`。

**验证**：panel **247**（原 242 + 5：`fmt_grouped` / `text_minmax` / `stat_value` /
`stats_defaults_are_full_set_in_display_order` / `stats_tab_renders_toolbar_header_and_rows`
（最后一条是真渲染断言——工具条 28px、表头与行各 20px，且停用 Sum 后行消失、Reset 后回来））
/ guard **4** / shell **14** passed；`cargo check -p rds-editor` 通过。

**仍未做**：值详情 / 分组 / 引用 / 执行SQL 四页工具条（已由 §2.33 起逐页补齐）；
统计项增删跨会话持久化（**已由 §2.77 补齐**）；R3 / R4 / R5。

### 2.32 2026-11-09（右栏 R1 收口：四页页内工具条统一）

`result-panel.md` §8.41 是完整证据，这里只留接手要点。

**做法**：抽 `panel_toolbar(debug_id, title, actions, cx)`（`panel/tabs.rs`）——28px、
左标题 `truncate` + 右 `Toolbar.xsmall().p_0().contents(actions)` + 底边 `HAIRLINE`；
计算页改成复用它，值详情 / 分组 / 执行SQL 三页接入。**引用页保持空态**：DBeaver
`ReferencesPanel.contributeActions`（`:156-158`）本身是空实现，摆按钮反而是偏离。

**新增能力**：`ResultRequest::CopyValue` + 面板 `pub fn selected_value_text(cx)`（注意
**必须 `pub`**，`pub(crate)` 时 editor crate 调不到、编译警告 `never used`）+ dispatch
分支走 `cx.write_to_clipboard`（与 `CopySql` / `CopyExecSql` 同机制）。

**测试**：`panel_harness` 抽成公用测试壳（空面板 + 两列网格 + 预选第 2 列），
`right_panel_pages_share_the_toolbar_shape` 断言三页工具条可见 + 值详情工具条 28px。

**验证**：panel **248**（原 247 + 1）/ guard **4** / shell **14** passed；
`cargo check -p rds-editor` 通过。

### 2.33 2026-11-09（R3 第一步：分组页多列 / 排序 / 仅重复 + Metadata 订正）

`result-panel.md` §8.42 / §8.43 是完整证据。

**分组页配置化**（纯面板视图状态，不落盘）：
- 新增 `group_columns: Vec<usize>`（空 = 退回当前选中列）/ `group_sort: GroupSort` /
  `group_duplicates_only: bool`
- 数据层：`grid/stats.rs::group_by_tuples` + `grid/text.rs::group_by_columns_for`
  （1 列保留原生数值键归一化，多列统一字符串口径）
- 视图层：`panel/tabs.rs::arrange_groups`（先过滤再排序，`CountDesc` 同数按值序兜底）
- 三条请求：`ToggleGroupColumn` / `SetGroupSort` / `SetGroupDuplicatesOnly`
- 工具条三个动作 id：`editor-result-group-columns` / `-sort` / `-dup`

**踩坑**：`arrange_groups` 一开始插在 `impl ResultPanel` 块内（缩进顶格也合法），成了关联
函数，`render_group_tab` 里的裸调用与 `panel::tests` 的调用都报 E0425；移到文件级并在测试
模块显式 `use super::tabs::arrange_groups` 才通。

**Metadata 订正**：本仓并不缺 Metadata 面板——`render_browser_tab`（`sidebars.rs:241-295`）
已输出来源表 / 来源列 / 注释 / 驱动类型 / 可空 / 默认值，等于把 DBeaver 的
`MetaDataPanel` 合并进了列浏览器页。R4 实际只剩 References（缺外键元数据端口）与
「生成 DDL」动作（无 DDL 端口）。

**验证**：panel **249**（原 248 + 1）/ guard **4** / shell **14** passed；
`cargo check -p rds-editor` 通过。

### 2.34 2026-11-09（R5 第一步：右栏宽度可拖）

`result-panel.md` §8.44 是完整证据，这里只留接手要点。

**路线选择**：不把网格+右栏改成 `h_resizable` 组（会重排 `flex_1` 语义、牵动既有渲染断言），
改用 vendor 明说「供 application's sidebar 这种不在 panel group 里的边」的独立手柄
`resize_handle::<T, E>(id, Axis::Horizontal).inside(HandleEdge::Leading)`，只贴一条手柄。

**状态**：`panel/data.rs` 加 `right_sidebar_width: Pixels`（默认 320px）+
`right_sidebar_dragging: bool`；`ui.rs` 加 `RESULT_SIDEBAR_RIGHT_MIN_WIDTH = 12.0` /
`RESULT_SIDEBAR_RIGHT_MAX_WIDTH = 40.0`（f32，避免裸 `px(`）；新请求
`ResultRequest::ResizeRightSidebar(i32)`（**必须 i32**：枚举 derive `Eq`，f32 不过）+ dispatch
转发；`panel/events.rs::resize_right_sidebar`；`sidebars.rs::clamp_right_sidebar_width` 纯函数。

**增量口径**：`MouseMoveEvent` 没有 `previous_position`，用
`window.use_keyed_state("editor-result-right-sidebar-drag-x", …)` 的 `Cell<Pixels>` 攒上一帧 x，
每次 `on_drag_move` 取差值 `(pos - prev).as_f32()` 发请求再写回。

**两个硬坑**：① `EffectTransition` 时长给 **0** 会 NaN → 命中
`animated value should be finite`（拖拽期用 1ms，`from == to` 即直写）；② 用
`(pos - prev) / px(1.)` 算 f32 会新增裸 `px(1)` 让 `ui_guard` 红，改 `Pixels::as_f32()`
（`gpui-pre-0.3.11/src/geometry.rs:2802`）。另：`panel_harness` 不自带 `init`，新 gpui 用例
首行要 `cx.update(gpui_kit::init)`；默认 `show_value_sidebar = true`（宽展开），不是收起。

**验证**：panel **252**（原 249 + 3：`right_sidebar_width_clamps_between_ui_limits` /
`resize_request_clamps_width_and_suspends_the_open_animation` /
`resize_handle_mounts_only_when_the_sidebar_is_expanded`）/ guard **4** / shell **14** passed；
`cargo check -p rds-editor` 通过。

**仍未做**：双击复位；手柄命中带宽与拖拽手感的真窗口目视。
（比例持久化已在 §2.35 落地；布局方向切换已在 §2.36 落地）

**关于 `theme-tokens-complete.md`**：本批（P0/P1/P3/R1/R2/R3/R5）**不需要**写进那份文档——
它的职责是色板与「尺寸常量单一来源」守门，而本批未改任何色值；R5 新增的两个尺寸常量只在
`result_panel/src/ui.rs` 单处定义（`shared_constants_do_not_diverge` 仍 4 passed，无分叉）。

### 2.35 R5 第二步「比例持久化」（已提交 `065cbb7d`）

口径：全局偏好项 `result_panel.sidebar_width`（px，默认 320，范围 192~640），
与 DBeaver 的 `panelRatio`（千分比 / 按展示类型分组 / 惰性落盘）有意偏离，理由写在
`result-panel.md §8.45.2`。**落盘时机取「松手即写」**，不是 DBeaver 的惰性写 —— 本仓没有
「展示类型切换」的稳定对应动作，惰性写会让重启丢宽度。

代码面共 10 文件：settings 5（model / entries / registry mod / service mod / service result_panel）、
面板 3（`ports.rs` 加 `EndResizeRightSidebar`、`panel/events.rs` 加
`end_right_sidebar_resize() -> Option<Pixels>` 与 `set_right_sidebar_width_px()`、
`panel/render.rs` 根元素 `.when(dragging, on_mouse_up → EndResizeRightSidebar)`）、
宿主 2（`results/dispatch.rs` 分支落盘、`view/host/construct.rs` 构造后注入初值）。

**验证**：`cargo check -p rds-settings` / `-p rds-result-panel` / `-p rds-editor` 三条均通过；
门禁 panel **254**（原 252 + 2：`sidebar_width_pref_is_injected_and_clamped` /
`end_resize_reports_width_only_while_dragging`）/ settings **35** / shell **14** / guard **4** 全绿。
已提交 `065cbb7d`（13 files +288/−4）。

**环境坑（两次编译失败的真因）**：`RUST_MIN_STACK=268435456` 同样作用于 rustc 编译线程，
冷编 `windows` crate 会 `STATUS_STACK_BUFFER_OVERRUN`；**编译命令不要带它**（只在跑 gpui 测试时设）。
另：D 盘曾被 target 占满（0 空闲）导致 `Edit` 把两个文件写成 0 字节（`settings/src/service/result_panel.rs`
与并行线 `editor-backlog.md`），已 `git checkout` 复原；写盘前先查 D 盘剩余。

### 2.36 R5 第三步「布局方向切换」（已提交）

对齐 DBeaver `CMD_TOGGLE_LAYOUT` → `toggleVerticalLayout()`：右栏在**右侧**与**网格下方**之间切换。
证据与偏离全文见 `result-panel.md §8.46`，接手要点：

- **状态**：`vertical_layout: bool`（默认 `false`，同 `ResultSetViewer.java:1020`）+
  `right_sidebar_height: Pixels`（默认 240px）。两个方向各记自己的尺寸：横排改宽度、
  竖排改高度，互不影响。
- **换向**：普通视图「主 pane + 右栏」外层容器按方向 `h_flex()` / `v_flex()`（`items_stretch`）；
  左右两条竖条留在这一层纵贯整高（与 DBeaver `panelSwitchFolder` 在 SashForm 之外同构）。
- **手柄换轴**：`Axis::Horizontal`+`Leading`（右栏左边缘）↔ `Axis::Vertical`+`Leading`（右栏顶边）；
  `on_drag_move` 取 x / y 位移，口径仍是 `size - delta`。keyed state 键按方向分开
  （`editor-result-right-sidebar-drag-x` / `-drag-y`），否则换向后把上一帧另一轴坐标当基准会跳一格。
- **入口**：右缘竖条**底部**加 `IconName::RotateCcw` 按钮（页签图标后用 `flex_1()` 间隔块推到底，
  等价 DBeaver 的 1×1 `FILL_VERTICAL` 空标签），`toggled(vertical_layout)` 表达当前方向。
- **两条偏离**：① 竖排高度**不进偏好项**（宽度偏好是横向 px 口径，共用会串；DBeaver 用千分比才通用）；
  ② 竖排**不做开合补间**（`EffectTransition` 只有 `width`，且换向不是展开/收起动作）。
  另 `vertical_layout` 自身也**未持久化**（DBeaver 存 presentations 段，需先解决按展示类型分组）。
- **竖排松手**：`end_right_sidebar_resize` 返回 `None`（只清拖拽标记，不落盘）。

**验证**：`cargo check -p rds-result-panel` / `-p rds-editor` 通过（0 error / 0 warning）；
门禁 panel **256**（254 + 2：`right_sidebar_height_clamps_between_ui_limits` /
`toggle_vertical_layout_flips_and_stops_persisting_widths`）/ shell **14** / guard **4** 全绿。

**仍未做**：方向状态与竖排高度的跨会话持久化、对比模式下的方向切换、
竖排观感的真窗口目视。

### 2.37 保存值「保存至文件…」（对齐 DBeaver `ContentValueManager.saveToFile`）

值详情页工具条右段新增「保存值」按钮（`editor-result-value-save`，`IconName::Download`）。
证据与偏离全文见 `result-panel.md §8.47`，接手要点：

- **对齐的是哪一支（容易搞错）**：DBeaver 值页有两个动作，`SaveValueAction`
  （`ValueViewerPanel.java:445`，command `…core.resultset.cell.save`）的 `saveValue()`（`:370-385`）
  是把编辑器里的值 **写回结果集**；真正「保存至文件…」在
  `ContentValueManager.contributeContentActions`（`:86-99`）→ `saveToFile`（`:291-334`）。
  **本仓只做后者**（只读预览、无写回通道）。
- **职责切分**：面板算字节 + 默认文件名（`value_payload(kind, value, row, col)` 纯函数、
  `ResultPanel::selected_value_payload`），宿主只选路径 + 写盘
  （`editor/.../results/export.rs::save_selected_value`）。取消 = 直接 return 不提示。
- **端口**：复用 `SavePathPicker`（`(当前路径, 默认名)`），**不用** `ExportPathPicker`
  （后者签名带 `ExportFormat`，而值可以是 txt / json / png / bin，无固定格式）。
- **形态映射**：图片 → `hex_to_bytes` 真字节 + 格式名小写扩展名；Hex → 真字节 + `bin`；
  Json → 原文 + `json`；Text → 原文 + `txt`。默认名 `value_r{行}_c{列}.{ext}`（1 基）。
- **三条偏离**：① 触发条件放宽（DBeaver 只对 `DBDContent` 摆该动作，本仓任意格都摆，
  文本也能落盘）；② 默认名不取列名（列名可能含 `/ : *` 与超长中文，清洗会引平台差异）；
  ③ 未做 "Open in external editor"（`ContentValueManager.java:100-134`）。

**验证**：`cargo check -p rds-result-panel` / `-p rds-editor` 通过（0 error / 0 warning）；
门禁 panel **257**（256 + 1：`value_payload_extracts_bytes_and_extension_by_shape`）/ guard **4**
全绿（本轮未动 settings / workbench_shell）。

**仍未做**：external editor、列名当默认名、「写回结果集」通道、320px 窄栏三按钮拥挤度目视。

### 2.38 R3 第二步：多列分组下数值列键归一化

多列分组里数值列 `2.0` 与 `2` 被拆成两组，本轮改为逐列取原生归一化键。全文见
`result-panel.md §8.48`，接手要点：

- **订正**：`§8.42.2` 原写「DBeaver 多列分组键按各列显示文本拼」是未实证推断。DBeaver 的
  分组是**下推 SQL**：`GroupingResultsContainer.rebuildGrouping()`（`:211-272`）→
  `SQLGroupingQueryGenerator.generateGroupingQuery`（`SQLGroupingQueryGenerator.java:80`）拼
  `GROUP BY`（`:166`）逐列 `prepareSqlString`（`:167-172`），仅重复走 `HAVING COUNT(...) > 1`
  （`:174-179`），再交内部第二个 `ResultSetViewer` 执行。键 = 数据库列值，前端不参与。
- **落点**：`grid/text.rs` 抽私有 `column_group_keys(column) -> Option<Vec<String>>`，
  `group_by_for` 与 `group_by_columns_for` 多列分支共用；`None` 时回退 `column_values`。
- **回退条件**（四个）：非 Int/Float 列 / 无事实档（表档已 release）/ 存在被删行 /
  该列出现 NULL 或取不到原生值。
- **行覆盖口径（易错）**：原生键必须与 `column_values` 一样跳过本地行，否则原生键列与文本列
  长度不同 → 多列元组按行号错位。初版用 `?` 整列返回 `None` 已经踩到，已改为 `continue`。
  **后续若改 `column_values` 的跳过规则，必须同步改 `column_group_keys`**。
- **边界**：表档 / 有删行 / 含 NULL 的数值列在多列分组下仍按显示文本，仍可能拆组（本仓拿不到
  原生值的边界，DBeaver 无此问题）。

**验证**：`cargo check -p rds-result-panel` 0 error / 0 warning；panel **258 passed**
（257 + 1：`group_by_columns_normalizes_numeric_keys_beside_text_columns`）/ guard **4**。

### 2.39 R3 第三步：分组页「清空分组列」+ R3 结项

分组页工具条第 4 个动作「清空分组列」（`editor-result-group-clear`，`IconName::CircleX`）。
全文见 `result-panel.md §8.49`，接手要点：

- **DBeaver 工具条九宫格**（`GroupingPanel.fillToolBar:164-176`）：排序（`MODE_FORCE_TEXT`）、
  仅重复、分隔、编辑列、删除列、分隔、**清空分组**（`ClearGroupingAction`，`:173`）、分隔、
  扩展动作（`addExtensionActions:178-184` 走 `GroupingRegistry`，本仓无注册表故不做）。
- **清空的启用条件**：`ClearGroupingAction.isEnabled()` = 分组列非空（`:34-37`）；本仓
  `clear_group_columns` 在空集合直接 return、按钮 `.disabled(空集合)`。
- **清空后的落点差异**：DBeaver `clearGrouping()`（`:198-209`）清查询 + 清属性 + 切
  `EmptyPresentation`（**空态**）；本仓没有「无分组键」态，清空后回落「网格选中格 /
  清单选中列」兜底列（偏离已入档）。
- **落点四处**：`ports.rs` `ResultRequest::ClearGroupColumns` / `panel/events.rs::clear_group_columns`
  / `panel/tabs.rs::render_group_tab` 第 4 个 `Button` / `results/dispatch.rs` 转发。
- **图标选择依据**：`IconName::CircleX` 来自 `use gpui_kit::assets::IconName` 全集，且
  `icons/circle-x.svg` 在 `gpui-kit-assets-0.9.0/default-icons.txt`（内嵌 106 个）里；
  `eraser.svg` / `broom.svg` 不在该清单（会静默渲染空白），故未选。

**R3 结项（三条遗留的实测结论）**：① 分组列持久化 = 不是 DBeaver 行为（`GroupingPanel`
的 `loadSettings/saveSettings` 空实现）；② 每组展开看行 = 不是对齐项（DBeaver 分组结果是
另一条 `GROUP BY` 查询的结果集，没有组内明细）；③ 分组列顺序可调 = DBeaver 唯一入口是分组
结果网格的列头拖放（`dropColumns:161-215` → `getOverColumnIndex:219-224` →
`GroupingColumnsContainer.moveColumns:126`），本仓 `DataTable` 无该入口，登记不做。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0 error / 0 warning；panel **259 passed**
（258 + 1：`clear_group_columns_drops_explicit_selection`）/ guard **4**。

### 2.40 工具栏 SQL 入口改「显示 SQL」+ 右栏六页竖向节奏对齐

全文见 `result-panel.md §8.50`，接手要点：

- **「显示 SQL」不是独立控件**：DBeaver 把它放在筛选条第 1 格（`ActiveObjectPanel`，
  `ResultSetFilterPanel.java:184`），内容是固定文案
  `sql_editor_resultset_filter_panel_show_sql_label`（zh `ResultSetMessages_zh.properties:746`
  =「显示 SQL」），取值见 `getActiveSourceQuery:573-585`（容器为表且无筛选时返回该文案），
  宽度按 `maxTextLength = compactMode ? 16 : 24` 封顶（`:996`），原文只在弹层里。
- **落点**：`panel/render.rs` 的 `sql_chip`（`editor-result-sql-preview`，`IconName::Table2` +
  `label("显示 SQL")` + `.xsmall()`）插进筛选复合体第一格；`grid/status.rs::render_toolbar`
  的 `sql_preview` 形参已删（`grid/tests.rs` 探针调用同步减一个 `None`）。
- **右栏「没对齐」的四个实体原因**（都已修）：① 外壳是块级 `div()` → 各页 `flex_1` 全失效
  （长列表不滚动、空态不居中）；② 列浏览器页手写 24px 无分隔线标题行，与其它页 28px 不同形；
  ③ 引用页整页只有空态、没有工具条；④ 值/计算/分组/执行SQL 的空态整页早退（少 28px）。
  现在六页统一「24px 页签条 + 28px `panel_toolbar` + 剩余正文」，空态也保留工具条
  （新增公用件 `tabs.rs::empty_page`；`panel_toolbar` 改 `pub(crate)` 供 `sidebars.rs` 调）。
- **陷阱**：gpui 裸 `div()` 是 block（`vendor/gpui-base/src/styled.rs:105-107`，`h_flex`/`v_flex`
  才是 flex）。右栏页签条行里那个 `.flex_1()` 在块布局下"看着没事"，改 flex 后会立刻吃掉剩余高度。
- **仍待拍板**：右缘竖条 36px vs DBeaver ≈24px；「执行SQL」页与 SQL 弹窗是否二选一；
  P2 三项（筛选历史 / 筛选设置 / 查找替换）。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0 error / 0 warning；panel **260 passed**
（`right_panel_pages_share_the_toolbar_shape` 扩成七步）/ guard **4**。

### 2.41 表格打磨第一批：列竖分隔线 + 单元格 6px 内距

全文见 `result-panel.md §8.51`，接手要点：

- **DBeaver 有竖线且默认开**：`LightGrid.java:295` `linesVisible = true`，画线在所有单元格之后
  统一画（`:2400-2440`：水平每行一条、垂直沿每个列边界一条；表头内部不画，`startY = headerHeight - 1`）。
  单元格/表头文字左右内距都是 **6px**（`GridCellRenderer.java:43-48`、`GridColumnRenderer.java:36-41`）。
- **落点**：`grid/cell.rs::render_td` 两支加 `.border_r(ui::HAIRLINE).border_color(theme.table_row_border)`；
  内距 `.px_2()`(8) → `.px(rems(0.375))`(6)；`grid/header.rs::render_th` 两支补 `.px(rems(0.375))`
  （此前表头没有内距，列名与格内文字差一截）；行线组件自带（`table/state.rs:2274-2276`），不动。
- **关键机制**：左右内距必须**从组件层挪到单元格自己身上**（`grid/table_impl.rs::cell_paddings` 置零），
  否则单元格盒子两边各缩进 4px，`border_r` 会画在离列边界 6px 的位置（像格内竖线而不是列分隔）。
  改后单元格撑满整列宽，竖线正好落在边界；`fit_col` 读同一份列 padding，自适应宽度自动同口径。
- **守卫**：`ui.rs` 新增 `RESULT_CELL_VERTICAL_PADDING = px(2.)`（沿用组件 XSmall 档上下原值），
  BASELINE 17 → 18 并写了理由。
- **仍待拍板**：① 选中色被 vendor `clamp_alpha` 封顶 0.2（`theme/schema.rs:1036-1067`），实心浅蓝只能
  改 vendor 或接受近似；行高仍 26px（DBeaver ≈19px + 1px 线）；真窗口目视未做。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0 error / 0 warning；panel **261 passed**
（+1 `cells_stretch_to_the_column_boundary_for_grid_lines`：行号槽 ≥48-1、数据列 ≥128-4，改前
40 / 120 必挂）/ guard **4**（基线已同步上调）。

> **2026-11-18 修正**：上面写的 `.border_r` 当天就改成了 **`.border_l`**（行号槽不再画线），
> 表头也补了同一条线。原因与测法见 §2.42。

### 2.42 表格打磨第二批：表头列边界线（并修正 §2.41 的竖线基准）

全文见 `result-panel.md §8.52`，接手要点：

- **DBeaver 列头逐列画两条线**：右边界竖线 + 底边横线（`GridColumnRenderer.java:105-108`）；
  获焦列改画四边框（`:96-104`）；只读列底部多一条线（`:110-115`）。底边本仓早就有
  （组件表头行 `border_b_1`，`table/state.rs:2114-2115`），缺的只有竖直那条。
- **落点**：`grid/header.rs::render_th` 两支加 `.border_l(ui::HAIRLINE)`，色
  `theme.colors.border`（与表头底边同源，对应 DBeaver 的 `getHeaderBorder`）；`.when(col_ix > 0, …)`
  跳过首列（首列左边界就是表格左边缘）。
- **为什么必须画左边**：表头格内容区右端被组件 `render_sort_icon` 占掉 20px
  （`table/state.rs:1910`，图标 + 2px 内距）——实测 128px 列的表头格只有 **108px**，
  右边界线会落在列内侧 20px。左边与列左边界重合，数据格也随之统一成 `.border_l`，
  表头线与数据线落在同一条 x 上；行号槽不再画线（首条边界由首个数据格画）。
- **测试口径**：表头几何断言不能用「表头格宽 = 列宽」（它是 108），改成
  「行号列表头宽 ≥48-4」+「数据列表头左缘比行号槽右移 ≥48-1」；
  `header_column_menu_floats…` 的列名宽度 104 → 95（1px 线占内容宽），阈值 96 → 92。
- **未做**：获焦列四边框（`TableState::selected_cell()` 在 `render_th` 里读不到，实体正被
  `&mut` 借用，需先在 delegate 侧缓存焦点列）；只读列标记（无列级只读信息）。
- **陷阱**：`no_new_hardcoded_color_or_pixel` 会连**注释里的** `px(` 一起计——注释写
  「图标 + 2px 内距」而不是 `p(px(2.))`。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0 error / 0 warning；
panel **261 passed**（不新增用例，复用 §2.41 的几何用例加两条表头断言）/ guard **4**。

### 2.43 表格打磨第三批：排序指示去重（交给组件图标槽）+ 自适应宽度补 20px

全文见 `result-panel.md §8.53`，接手要点：

- **表头右端 20px 是组件的排序图标槽，不是空白**：`render_sort_icon`
  （vendor `table/state.rs:1828-1872`）给出 `size_3()`(12) + `p(2px)` = 16px，加
  `render_th` 内层的 `.pr(offset_pr)`（4-0=4px），共 20px；实测 128px 列的表头格只有 108。
  图标本身是**可点排序控件**（`on_click → perform_sort`），状态随 `.sort(...)` 回灌。
- **本批删掉自绘 `↑`/`↓`**（`grid/header.rs`）：它与组件图标是重复指示器，而且不可点
  （看着能点、点不到）。排序指示从此只有一个来源——组件图标（`SortAscending` /
  `SortDescending` / 未排 `ChevronsUpDown`）。DBeaver 同样只用图像三角
  （`GridColumnRenderer.java:40-53`），从不画文字箭头。
- **筛选 `▽` → `Icon::new(IconName::Funnel).size_3()`**（12px，与排序图标同尺寸）。
  有意偏离：DBeaver 那个 `IMAGE_FILTER` 是 `DROP_DOWN`（下拉入口指示），我们的筛选入口
  在列头菜单，图标表达的是「此列已筛选」这个状态。
- **自适应宽度补 20px（真 bug 修复）**：`grid/fit.rs::header_width` 删掉箭头测量、筛选改按
  `ui::RESULT_FIT_HEADER_ICON`(12px)、合计再加 `ui::RESULT_FIT_HEADER_CHROME`(20px，仅数据列)。
  修前双击算出的列宽比「列名完整显示」少 20px，列名照样省略号。
- **新用例**：`grid/tests.rs::autofit_reserves_the_component_sort_icon_slot` 断言
  `fit_column == measure(列名) + RESULT_FIT_HEADER_CHROME`（`fit::measure` 已提升
  `pub(crate)` 供用例按同一口径量列名），不是弱断言。
- **未做**：未排序列仍会显示组件那个 50% 透明度的双箭头（DBeaver 什么都不画）。要「什么都不画」
  得让 `.sort(None)`，而 `perform_sort` 在 `column.sort == None` 时直接 return ⇒ 表头点击
  排序失效；要彻底贴合必须改 vendor。登记未做。
- **守卫**：`ui.rs` 基线 18 → **20**（新增两个 `px(…)` 常量）；注释里仍不能出现 `px(`。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0 error / 0 warning；
panel **262 passed** / guard **4**。

### 2.44 表格打磨第五批：整行选中铺色 + 网格事件订阅重挂（修「订阅从未生效」）

全文见 `result-panel.md §8.54`，接手要点：

- **先读这条再动网格事件**：`lifecycle.rs::new` 原来把订阅句柄写成
  `let _grid_sub = cx.subscribe_in(…)` —— **局部变量，`new` 返回即解订**，且 `set_grids`
  换网格不重挂。所以面板那套处理（`SelectCell` / `DoubleClickedCell` / `MoveColumn` /
  `ColumnWidthsChanged` / `ClearSelection` / `SelectRow`）**一次都没跑过**；
  生产看着正常只是靠宿主 `host/construct.rs` 另一条订阅，而 `MoveColumn`
  → `ColumnOrderChanged`（列序跨会话落库）**全仓只有面板这一处**，即那条一直是死功能。
  现在：处理抽成 `handle_grid_event` 方法，句柄存 `Self::_grid_sub`，
  `set_grids` 增 `window` 形参并在末尾重挂（旧句柄随赋值解订，不会留两条）。
- **整行选中铺色**：组件只画 1px 行框（vendor `table/state.rs:2255`），不铺底色，
  而 DBeaver 是整行色（`GridRowRenderer.java:55-70`）。`delegate.selected_row` +
  `render_tr` 里 `selected → bg(table_active)`、否则奇数行斑马（哨兵
  `editor-result-row-selected` / `editor-result-row-odd`），同一行只一档色；`set_data` 清空。
- **回归用例**：`panel/tests.rs::select_row_event_mirrors_into_the_delegate` 就是订阅
  那条的探针（订阅没挂时 `left: None`）。两个坑：`panel.read(app).grid().update(app,..)`
  双重借 `app`（E0502，先 clone `Entity`）；`debug_bounds` **跨帧累积**，所以「斑马纹
  缺席」只能在 `grid/tests.rs::selected_row_paints_a_whole_row_band` 里断言。
- **未做**：选中色只到 20% tint（vendor `theme/schema.rs:1036-1067` 的 `clamp_alpha`
  把 `table_active` 封顶 0.2），DBeaver 是实心浅蓝；要实心须改 vendor（全局影响所有表格），
  等拍板。`#` 槽底色、获焦列四边框仍未做（理由见 §8.54.6）。

**验证**：`cargo check -p rds-result-panel -p rds-editor` 0 error / 0 warning；
panel **264 passed** / guard **4**（基线 20）/ editor 测试目标 `--no-run` 通过。

### 2.45 表格打磨第六批：筛选框订阅同样被丢弃（「边打边筛」从未生效）

全文见 `result-panel.md §8.55`，接手要点：

- **同一个坑的第二例**：`let filter_sub = cx.subscribe_in(…)` 后面跟 `let _ = filter_sub;`
  —— `Subscription` 掉即解订，`let _ =` 就是当场丢弃。于是 `InputEvent::Change`
  从不到达 `on_filter_input`，输入框打字不触发 300ms 防抖、不筛行。现已入库为
  `Self::_filter_sub`。
- **为什么之前没发现**：编辑器侧 `results_b.rs::filtering_waits_for_the_debounce` 直接调
  `on_filter_input`，测的是防抖语义、绕过了订阅。接线这事现在归面板用例
  `typing_into_the_filter_box_reaches_the_panel`（真 `TextareaState::focus` + 真敲键 +
  断言 `filter_text` 立刻变化 + 推进 400ms 后 `view_total()==1`）。
- **A/B 已验**：把句柄退回丢弃写法，同一条用例在 `left: "" / right: "1"` 处红；
  恢复即绿。别把它改回直调 `on_filter_input`，那样会重新变成空转断言。
- **顺带记一笔**：§2.44 那批修好的网格订阅现在也把 `MoveColumn` 通了，
  宿主 `host/results/dispatch.rs:263` 的 `ColumnOrderChanged => save_session_now`
  确实在消费 ⇒ 列序跨会话落库从此真正生效（此前是死推送）。

**验证**：`cargo check -p rds-result-panel` 0 error / 0 warning；
panel **265 passed** / guard **4**。

### 2.46 表格打磨第七批：`#` 行号槽铺表头底色（选中行留空）

全文见 `result-panel.md §8.56`，接手要点：

- **DBeaver 行头格有底色**：`GridRowRenderer.java:55-70` 每行 `getHeaderBackground(element, selected)`
  （选中→选中色，否则表头色）⇒ 行号列是一条独立竖条。本仓行号槽是自绘单元格
  （`cell.rs::render_td` 的 `is_row_number` 分支，组件行头已 `.row_header(false)`）。
- **选中行必须留空**：整行选中色铺在行容器上且是**半透明** `table_active`
  （`clamp_alpha` 封顶 0.2），行号格再铺一层会双层叠加、比数据格深一档。
  口径：未选中 → `theme.tokens.table_head`；整行选中 → 不铺（让整行色透上来）；
  本地行 → 保留身份色。
- **判据抽纯函数**：`row_number_paints_head(is_local, selected)` + delegate 包装
  `ResultGridDelegate::row_number_paints_head(row_ix)`，渲染只照着做；
  用例 `row_number_gutter_paints_the_header_background` 锁组合 + delegate 真状态。
- **踩过的坑**：`debug_selector` 在 gpui-fast 的 `div` 上是**单一槽位**（`div.rs:910` 覆盖前值），
  行号格已挂着 `editor-result-rowno-{row_ix}`（表格密度口径），所以**不能再挂第二个哨兵**
  ——色值这层 headless 不锁，靠主题单一来源。
- **未做**：获焦行加粗（`GridRowRenderer:96` 的 `boldFont`）；深色 `table.head`(#394248)
  当整列底色是否偏重视目视结果，要调就调主题 token。

**验证**：`cargo check -p rds-result-panel` 0 error / 0 warning；
panel **266 passed** / guard **4**。

### 2.47 表格打磨第八批：获焦行的行号加粗

全文见 `result-panel.md §8.58`，接手要点：

- DBeaver 的行头文本 `selected ? boldFont : normalFont`（`GridRowRenderer.java:96`）——
  **只有当前行**的行号加粗；列头反过来恒 normalFont（`GridColumnRenderer.java:84-87`）。
- 本仓「当前行」= 整行选中行（`selected_row`），所以判据 `row_number_is_bold(row_ix)`
  与行号槽底色共用同一来源，不引新状态；与底色正交（选中行既不铺表头色又加粗）。
- 渲染只调 delegate 判据；断言并入 `row_number_gutter_paints_the_header_background`
  （选中行 true / 另一行 false），不碰颜色层。
- **未做**：表头获焦列字重（DBeaver 本来就不加粗）。

**验证**：`cargo check -p rds-result-panel` 0 error / 0 warning；
panel **266 passed** / guard **4**。

### 2.48 内距收成单一来源（渲染 6px ↔ 自适应 12px）

全文见 `result-panel.md §8.59`，接手要点：

- 同一份 6px 内距原来写两遍：渲染 `.px(rems(0.375))`（**随主题基准字号浮动**）、
  自适应 `ui::RESULT_FIT_CELL_PADDING = px(12.)`（固定）。分叉 ⇒ 双击自适应算出的列宽
  差一整个边距，列名被省略号吃掉。
- 现在 `ui::RESULT_CELL_PADDING_X = px(6.)` 是单侧唯一来源，四处渲染字面量全改它；
  合计 12px 仍是 `RESULT_FIT_CELL_PADDING`。
- `Pixels` 字段 `pub(crate)`，`const` 里写不了乘法 ⇒ 等式由用例兜底：
  `grid/tests.rs::cell_padding_single_source_matches_fit_padding`。
- 默认主题下零观感变化；只有基准字号不是 16px 时内距保持 6px 不再缩放（对齐 DBeaver 的固定 6px）。
- `rds-ui-guard` BASELINE：`crates/result_panel/src/ui.rs` **20 → 21**（新增那个 `px(6.)` 常量）。

**验证**：`cargo check -p rds-result-panel` 0 error / 0 warning；
panel **267 passed** / guard **4**（基线 21 生效）。

### 2.49 筛选历史下拉（`HistoryPanel` 对齐）

全文见 `result-panel.md §8.60`，接手要点：

- DBeaver 筛选条第 5 列是 `HistoryPanel`（`ResultSetFilterPanel.java:214`）：箭头按钮，
  点开是历史筛选表（`createFilterHistoryPanel` `:1207`），选中一条即回到那个口径。
- **只有显式提交入历史**：Enter（`:283-296` `setCustomDataFilter()`）与 ▶ 执行
  （`RunFilterAsQuery` 臂先 `note_filter_commit()` 再重查）。打字过程中的防抖状态不入，
  否则历史被 `a`/`ab`/`abc` 灌满。重复提交同一个词 → 提到最前，不追加。
- 三处**有意偏离**（文档已写明理由）：① ↓ 快捷键未接（Textarea 方向键冲突，改由按钮打开）；
  ② 历史是**会话级内存**（本仓无 filter manager 持久化通道）；③ 上限 8 条（下拉菜单不是滚动表）。
- 展开态（`filter_expanded`）下历史按钮不渲染、Enter 不提交——与 DBeaver `:288-290` 一致。
- 接线：`filter_history` 字段（`panel/data.rs`）+ `note_filter_commit` / `commit_filter` /
  `apply_filter_from_history`（`panel/events.rs`）+ 订阅 `match` 加 `PressEnter` 臂
  （`panel/lifecycle.rs`）+ 渲染下拉（`panel/render.rs`）+ `ResultRequest::ApplyFilter(String)`
  与宿主 `apply_filter_via_window`（`ports.rs` / `editor/…/results/dispatch.rs`）。
- 测试：`commit_on_enter_records_filter_history`（真敲键 + Enter：立即生效且入历史）、
  `filter_history_dedupes_caps_and_replays`（去重/封顶/重放回填）。

**验证**：panel **269 passed**（267 → 269）/ editor `--lib results` **45 passed** / guard **4**。

未做（P2 在册）：`Manage Filters` 设置对话框（`ResultSetFilterDialog`）、历史持久化（无通道）。

### 2.50 结果内查找栏（`find_next` 接上 UI）

全文见 `result-panel.md §8.61`，接手要点：

- 背景：`ResultGridDelegate::find_next` 早有实现 + 单测，但**从未接线**（注释说「选中/滚动是
  宿主的决定」，宿主一直没做）。本批把这条能力接上。
- 入口对齐 DBeaver 筛选条上的 CHECK 型 `openFindReplaceButton`（`ResultSetFilterPanel.java:356`）。
- 形态：浮层（DBeaver `ResultsetFindReplaceOverlay`）→ 工具栏下方一行 `render_find_bar`：
  词框 +「N 个匹配」+ 上一处 / 下一处 / ✕；回车 = 下一处；关闭保留词。
- 定位 = `set_selected_cell(row, col)` + `scroll_to_row` + `scroll_to_col`；`ResultMatch.col`
  是**视图列**（含 `#` 槽），不用折算。
- 新增纯函数：`count_find_matches` / `find_prev`（口径与 `find_next` 对称）。
- 收窄三项（在册）：替换（要改单元格值，另开一批）、正则/大小写/全词选项、格内区间高亮
  （`ResultMatch.range` 已备好）。

**验证**：panel **271 passed**（269 → 271）/ editor `--lib results` 45 / guard 4。

**2026-11-20 更新（Show matching，全文见 `result-panel.md §9.21`）**：查找栏补上 DBeaver 的
`selectAllButton` 语义——**勾上「仅显示匹配」把查找词当临时行过滤**（`model.setQuickFilter`
那一半），不再只能逐处定位。接手要点：

- `ResultGridDelegate::find_filter: Option<String>` 与 `filter` **分字段**（撤查找过滤绝不能
  动筛选框的词）；`rebuild_view_rows` 里三层叠加取交集（全局筛词 / 列级筛选 / 查找词），
  口径同全局筛选（所有列、大小写不敏感子串）；`TextFingerprint` 必须带上它。
- 面板态 `find_filter_only`；**关查找栏即撤销**（`toggle_find(false)` 与 `close_find` 都走），
  词保留；端口 `ResultRequest::SetFindFilterOnly(bool)`；勾选框探针 = `editor-result-find-only-wrap`。
- 差异：DBeaver 是边敲边过滤（250ms 去抖），本仓是勾选框 + 词变即同步；大小写 / 正则 /
  全词 / 替换栏 / info 行仍在册。
- 验证：`find_filter_narrows_rows_and_stacks_with_the_other_filters` /
  `closing_the_find_bar_drops_the_matching_filter_but_keeps_the_query` /
  `find_bar_only_matching_checkbox_queues_the_toggle`；panel **364**。

**2026-11-20 更新（三口径开关，全文见 `result-panel.md §9.22`）**：补齐 DBeaver
`FindReplaceOverlay` 行内那三颗 `SWT.CHECK`（`createRegexSearchButton:726` /
`createCaseSensitiveButton:740` / `createWholeWordsButton:751`）。接手要点：

- 口径本体 = `ports::FindCriteria{case_sensitive, regex, whole_word}`（**放 ports**，
  因为 `grid::text` / `view_rows` / `delegate` 都是 `pub(crate) mod`，公开签名里用会触发
  private-interface 警告）；按钮枚举 = `ports::FindOption`（`ALL` 顺序 = 正则 → 大小写 → 全词）。
- 匹配归一 `grid::text::FindMatcher`：词只 `trim`（**不小写化**，否则 `[A-Z]` / `\D` 失真）；
  非正则 + 非全词走既有零分配子串快路径，其余编译一次复用；编译失败（非法正则）= 零命中。
- `delegate.find_filter` 从 `Option<String>` 升为 `Option<FindFilter{needle, criteria}>`
  ——改口径也算「变了」，要重排视图行序；`rebuild_view_rows` 建一次 matcher 全表复用。
- 面板 `find_criteria` + `set_find_option`（改口径 → 清光标 → 过滤开着重推 / 否则重算命中
  → 有命中跳到第一处）；`count_find_hits` 是命中数的唯一算法；`find_step` 补传口径。
- 渲染三颗 `Button`（`.*` / `Aa` / `W`，本仓没有 `FIND_REPLACE_MATCH_*` 图标名），
  选中态必须用 `Selectable::selected`（0.9.0 的 `toggled` 只写 a11y）；勾正则时全词 disabled
  且被清 false。端口 `ResultRequest::SetFindOption(FindOption, bool)`。
- 差异：非法正则路径不同（本仓零命中 / DBeaver 抛 `PatternSyntaxException` 后被 job 吞）；
  三颗开关**不落盘**；图标目视阶段再补；替换栏 / 250ms 去抖 / info 行仍在册。
- 验证：`find_criteria_case_regex_whole_word_narrow_the_filter` /
  `find_bar_criteria_buttons_queue_their_option`；panel **366**（+2）、editor **798**。

### 2.51 工具栏 / 侧栏结构对齐第一批（2026-11-18）

全文见 `result-panel.md §8.62`（已做）+ `§8.63`（待拍板）。接手要点：

- **左缘竖条两段式**：展示组（网格 / 文本）在上、弹簧、**记录模式沉底**
  （DBeaver `ResultSetViewer.java:948-965`：RADIO 展示按钮 → `createEmptyLabel(FILL_VERTICAL)`
  → `recordModeButton`）。记录视图的行导航跟记录按钮走（本仓独有）。
- **筛选清除钮常驻**（~~空词禁用~~ → 已被 2026-11-20 更新推翻：DBeaver
  `enablePanelControls` 是**无条件** `setEnabled(true)`（`ResultSetFilterPanel.java:449-454`，
  旧条件已注释掉），见本节末「删除所有过滤 / 排序」段）。箭头菜单（清设置 / 显示隐藏列 /
  重置颜色 / transformers / pins / 全部重置）本轮未合并。
- 用例：`left_bar_puts_presentation_group_on_top_and_record_mode_at_the_bottom`（量真实像素）、
  `filter_clear_button_is_always_there_and_gated_by_the_filter_text`（a11y disabled 位口径：
  **可用态不暴露** disabled，只有禁用时才是 `Some(true)`；2026-11-20 改名
  `filter_clear_button_is_always_present_and_never_gated`）。
- 后续已补齐：状态历史（§2.52）、✕ 箭头菜单（§2.53）、右缘竖条面板总开关（§2.54）。
  仍在册：DBeaver 面板按钮是「独立 CHECK + 可增减页签」而本仓是固定页签。

**2026-11-20 更新（CHECK 按下态，全文见 `result-panel.md §9.23`）**：0.9.0 的
`Button::toggled(bool)` **只写 a11y**（`vendor/gpui-component/src/button/button.rs:513-522`），
按下态必须另调 `Selectable::selected`。工具栏五颗 CHECK 已补齐：
`editor-result-filter-expand` / `-find-toggle` / `-pushdown` / `-show-comments` / `-compare`
（`selected` 与 `toggled` 用同一个布尔，样式走主题 selected token）。
- 用例 `toolbar_check_buttons_flip_their_panel_state`：四颗探针在 + 点击入队对应「开」请求。
- **在册未做 → 已在 §9.24 铺完**：侧栏同类（`editor-result-bar-grid/text/record`、
  `editor-result-edge-*`、`editor-result-edge-vertical-layout`）、`editor-result-stat-mode`、
  `editor-result-group-dup`、`grid/edit.rs` 布尔选择器共 12 处，同一份改法补齐
  （`crates/result_panel/src` 内 19 处 `.toggled(` 已全部配 `.selected(`，脚本核对 0 缺）。
- 选中色 / 对比度属目视验收（用户口径：目视后面统一处理）。

**2026-11-20 更新（结构对账复核，全文见 `result-panel.md §9.25`）**：把注册表 / 五列筛选区 /
五颗工具条钮 / 右缘竖条 / 面板菜单三挂载点回到 DBeaver 源码重核一遍，结论是**结构对齐已收尾**，
只余一处可落地的小差异，本批已改：

- 筛选框空提示（`ResultSetFilterPanel.java:219-223`）→
  「输入一个 SQL 表达式来过滤结果」，**截掉 DBeaver 原文尾随的 `(使用 Ctrl+Space)`**
  （那半句指内容辅助 `:304-308`，本仓无补全）。用例 `the_filter_box_hints_the_sql_expression_it_takes`
  经 `presentation().placeholder()` 钉住。panel **368**（+1）/ editor **798**。
- **`org.jkiss.dbeaver.resultset.panel` 只注册 5 个 panel，没有 SQL 面板**（`plugin.xml:127-173`）：
  本仓第六页 `RightTab::ExecSql` 属本仓扩展页（在册待拍板：保留则登记、砍掉要另找地方放 wrapped SQL）。
- **不做（附理由，勿当缺口重做）**：
  ① 面板区右键菜单——DBeaver 把 `fillPanelsMenu` 挂三处（`:407-417` / `:1590-1614` / `:3231-3233`），
  本仓视图菜单按钮常驻，右键是同一份条目的第二个入口；
  ② `CMD_ACTIVATE_PANELS`（无焦点模型）；
  ③ `filtersCustomButton` 的 DROP_DOWN 箭头（能落地的只有「编辑筛选设置」，与主点击重复；
  另两条「按表保存 / 重置筛选」本仓无能力，`:1563-1570`）；
  ④ `panelsButton` 不加下拉（DBeaver 那颗本身就是纯 CHECK，`:1038-1052`）。
- **待拍板**：默认页集合（`activateDefaultPanels:1396-1426` + 只有 `value-view` 是
  `default="true"` ⇒ DBeaver 首开只开值查看器；本仓六页全开、active = 列浏览器）。
  若照抄，改法只有一处：默认集合收窄到 `show_by_default` 那几页。

**2026-11-20 更新（筛选设置·存档，全文见 `result-panel.md §9.26`）**：上面 ③ 那条
「`filtersCustomButton` 的 DROP_DOWN 箭头不做」**本批销账**——后两项的能力补上了，
箭头随之补齐：

- 面板新增 `SavedFilterSetting`（结果 SQL + 筛选词 + 自定义排序 + 唯一来源表）+ 字段
  `saved_filter`；`save_filter_setting` 读当前生效值，`reset_filter_setting` 只清存档、
  不动正在生效的筛选（DBeaver `ResultSetViewer.java:648-664` / `:671-687`）。
- 箭头菜单三项 = `CustomFilterListener.fillDropDownMenu`（`ResultSetFilterPanel.java:1561-1571`）：
  编辑筛选设置 / 保存过滤器设置 / 重置过滤器设置。筛选设置钮 `Button` → `DropdownButton`。
- 自动套用：`sync` 里过 `saved_filter_applies`（**按结果 SQL 认领、空筛选词不算、已有筛选不覆盖**）
  就写回 `filter_text` 并 `apply_filter_inner(cx, false)`（不 requeue，不替用户重查）。
  **只恢复筛选词，不套自定义排序**（排序要重查才算数，属显式动作）。
- 输入框回填走 `filter_backfill` → 宿主 `self.window.update` 转手（`sync` 没有窗口句柄）。
- **仍未做（登记）**：① `CMD_FILTER_CLEAR_SETTING`「删除所有过滤/排序」（DBeaver 一次清
  filter + order + 隐藏 + 冻结 + 颜色覆盖 + 转换器 + pin，`EraseItemListener:1525-1551`），
  本仓 erase 箭头只有「清除筛选词」+「显示隐藏列」；② 列头右键 `filtersMenu` 尾部四项
  （`ResultSetViewer.java:3926-3932`）；③ ~~存档尚未落盘（`SavedSession` 无字段，跨会话会丢）~~
  ——**第 45 批已销账**，见本文 §2.51 末段与 `result-panel.md §9.29`。
- 门禁：panel **371**（+3）/ editor check `Finished` / editor **798**（1 ignored）；
  ui-guard 红 2 项均来自并行线（`workbench/unlock.rs` 裸 px、`settings` 的 `BUILTIN` 分叉）。

**2026-11-20 更新（筛选条 ✕ = 删除所有过滤 / 排序，全文见 `result-panel.md §9.27`）**：
上面「仍未做 ①」**本批销账**，同时纠正 §9.26 一处记载错误：

- `clearDataFilter(true)`（`ResultSetViewer.java:605-626`）**不清隐藏列**——造新 filter 后
  把每列 `visible` / `options` 搬回去（`:609-623`，注释「keeping all visual state」）；
  被清的是 where / order / 每列 criteria / 每列 orderPosition。颜色覆盖 / 转换器 / pins
  是 `fillDropDownMenu` 里**另外的菜单项**，不在这条命令里。
- 本仓：`ResultRequest::ClearFilterAndOrdering` → `ResultPanel::clear_filter_and_ordering`，
  清 `filter_text` / `filter_order`（连浮层草稿）/ 列级筛选 / 本地排序，**不动**
  `column_hidden` / `column_frozen`；`clear_filter` 降为窄口径（只清词）。
- 清除钮改**恒可用**（DBeaver `enablePanelControls` 无条件 `setEnabled(true)`，`:449-454`），
  tooltip 与主点击都改为「删除所有过滤 / 排序」；箭头菜单第一项恒为该命令（原「清除筛选词」
  窄项并入，`erase_menu_labels` 的 `filter_text` 参数暂不参与判定）。
- 用例：`the_erase_menu_always_offers_the_full_clear`、
  `clearing_all_filters_drops_ordering_but_keeps_hidden_columns`（窗口用例，含隐藏列保留
  断言，对应 `restoreConstraintVisualState` 语义）。
- 门禁：panel **372**（+1 净增）/ editor check `Finished` / editor **798**（1 ignored）；
  ui-guard 3 passed / 1 failed（`workbench/unlock.rs` 裸 px，并行线面）。
- 仍在册：~~列头右键 `filtersMenu` 尾部四项（`ResultSetViewer.java:3926-3932`）~~（第 44 批
  销账）、~~存档落盘~~（第 45 批销账，见本文 §2.51 末段）、筛选条之外的菜单挂载点。

**2026-11-20 更新（列头菜单 `filtersMenu` 尾部四项，全文见 `result-panel.md §9.28`）**：
上面「仍在册」第一条**本批销账**：

- `column_menu_items` 尾部补四项（一条分隔线之后）：保存过滤器设置 / 重置过滤器设置 /
  删除所有过滤 / 排序 / 编辑筛选设置…（`ResultSetViewer.java:3926-3932`）。
- 四条命令全部复用既有端口（`SaveFilterSetting` / `ResetFilterSetting` /
  `ClearFilterAndOrdering` / `OpenFilterSettings`），本批只补入口，editor 侧不改；
  `header.rs` 新增 `filter_queue` 把请求转给面板。
- **门控差异**：DBeaver 只在 `getDataContainer() instanceof DBSEntity` 时摆保存 / 重置
  （`:3927-3930`）；本仓存档键是结果 SQL（§9.26.4 第 1 条），没有「非实体容器」层，
  四项恒摆（用例用 `table: None` 钉死这条）。
- 单元格右键（`context_menu`）对这四项走忽略分支：它们按「当前结果的筛选状态」出手，
  右键落点是单格，语义不对。
- 门禁：panel **373**（+1）/ editor check `Finished` / editor **798**（1 ignored）；
  ui-guard 3 passed / 1 failed（`workbench/unlock.rs` 裸 px，并行线面）。
- 仍在册：~~存档落盘（`SavedSession` 无 `result_filter_*`）~~（第 45 批销账，见本文 §2.51
  末段）、面板区右键入口（按「一处入口」原则不摆）。

**2026-11-20 更新（筛选存档落盘，全文见 `result-panel.md §9.29`）**：上面「③ 存档尚未
落盘」**本批销账**（§9.26 更新段那一条与 §9.28 更新段的「仍在册」同时失效）：

- 存档串用 **JSON 单列**（`SavedFilterSetting::encode` / `decode`）。不用列布局那套逗号串：
  SQL 原文与筛选词里本来就有逗号，逗号串必拆错。`#[serde(default)]` 让缺字段的旧串按空
  默认补齐，空串 / 坏 JSON 一律 `None`（宁可不恢复，也不恢复半份错存档）。
- 面板推 `ResultRequest::SavedFilterChanged`（无载荷，与 `ColumnLayoutChanged` 同分工：面板
  推、宿主存）。`save_filter_setting` 与 `reset_filter_setting` 只在**真的变了 / 真的有一份
  存档**时才推，避免空操作把整份会话写一遍。
- editor：`SavedSession.result_saved_filter: Option<String>`，`worth_saving` 加
  `|| result_saved_filter.is_some()`（空文档只带一份存档也要落库）；`restore_session` 走
  新方法 `set_saved_filter`——**只注入、不套用**（套用仍归下一次 `sync` 的
  `saved_filter_applies`，等价 DBeaver「打开结果才 `restoreDataFilter`」），且注入不推请求
  （否则恢复会把读出来的存档回写一遍，成环）。
- store：`EditorContext.result_saved_filter: String` 放**最末**（`updated_at_ms` 之后）；
  `updated_at_ms` 仍 `row.get(17)`、新列 `row.get(18)`，既有下标不平移；`ensure_column`
  幂等给老库补列。
- 门禁：store **193**（2 ignored，+1）/ panel **375**（+2）/ editor check `Finished` /
  editor **800**（1 ignored，+2）/ ui-guard **4 passed**（`unlock.rs` 裸 px 已由 `07162b9e`
  修掉，本线首次全绿）。
- ~~仍在册：自定义 ORDER BY 的**历史**~~（**订正**：`fillOrderingsMenu`
  `ResultSetViewer.java:3935-3948` 只是「按列升序 / 降序 /（正在按此列排时）清除」三项命令，
  DBeaver **没有**排序历史这一层，本仓筛选历史只记 `filter` 词与它一致）；存档通知用平台
  气泡（本仓只有面板内 `set_notice`，属跨模块）。

**2026-11-20 更新（列头菜单补「复制列名」，全文见 `result-panel.md §9.30`）**：列浏览器
清单右键早有「复制列名」，列头菜单没有；DBeaver 的列头右键是有的（`:3151-3161` 挂
`CMD_COPY_COLUMN_NAMES` 并带 `columns = attr.getName()`，即只复制当前列）：

- `ContextAction::CopyColumnName`（无载荷）+ `column_menu_items` 复制段首位
  「复制列名「列名」」；`header.rs` 分支直接写剪贴板（列名闭包外已算好）。
- 与列浏览器的 `CopyColumnNames` 分开：那个拷多选集合、换行分隔；列头这个是单列名。
- 两条有意差异：复制**裸名**（本仓网格不持有方言，DBeaver 走 `getQuotedIdentifier`）；
  不做多列配置对话框（列头菜单天然单列）。~~「复制行号」（`CMD_COPY_ROW_NAMES`）本仓没有
  行头菜单，无落点，不在册。~~ **2026-11-21 订正**：已补（见下段），行号槽现在挂右键。
- 门禁：panel **376**（+1）/ editor check `Finished` / editor **800**（1 ignored）/
  ui-guard **4 passed**。

**2026-11-21 更新（斑马线 A 方案 + 行号槽「复制行号」，全文见 `result-panel.md §9.31`）**：

- **斑马线定案 A（用户拍板：跟随主题）**。改前是「两个来源说同一件事」：组件
  `.stripe(true)` 已按奇数行铺主题 `tokens.table_even`（`vendor/gpui-component/src/table/state.rs:2246`、
  `:2277`），delegate 的 `render_tr` 又叠一层产品槽 `result.odd_row.background`；组件在
  `:2258` 取 `tr.style()`、`:2278` `.refine_style` 重放 → delegate 的 bg 赢，所以看到的是
  DBeaver 那颗 `#EBF5FF` 而不是主题色。**A 方案 = 删掉产品槽**：9 份资产
  （主资产 + 8 个主题包，各 light/dark 两处）删 `result.odd_row.background`；
  `workbench_shell::product_tokens` 删 `TokenMap` / `ProductTokens` 字段、`from_map` 行、
  访问器；`grid/table_impl.rs::render_tr` 奇数行改取 `theme.tokens.table_even`
  （与组件同值同源，显式再铺一遍是为了留 `editor-result-row-odd` 哨兵）。
- 连带：`ui_guard` 的 `PRODUCT_ROLES` 删该键（该校验单向：只拦「资产里有未声明键」，
  删键不会失败；删声明是为保持与结构体字段一一对应）；`crates/app/src/theme_pack.rs`
  的 `every_shipped_pack_loads` 产品角色计数 **13 → 12**；工具脚本同步
  （`tools/gen_theme_packs.py` 删该键生成行、`tools/audit_theme_packs.py` 从 `PRODUCT_KEYS` 删），
  否则下次重跑生成器会把键加回来。
- **不改**：`docs/architecture/theme/palette-candidates/*-product-tokens.json`（历史候选快照，
  是当时方案的取证材料）；`tools/gen_palette_options.py` 的 `odd` 候选调色板维度。
- **行号槽「复制行号」**（DBeaver `ROW_HEADER` → `CMD_COPY_ROW_NAMES`，
  `ResultSetViewer.java:3162-3164`；handler `ResultSetHandlerMain.java:377-396`）：
  `ContextAction::CopyRowNumber(String)`（载荷 = 显示行号）+ 纯函数
  `row_number_menu_items` 产出唯一一项「复制行号「3」」；`context_menu` 把原来的
  「行号列 ⇒ 空菜单」早退拆开，行号列单独产菜单（不进 `context_menu_items`）。
  `grid/cell.rs` 行号格补 `.id(...)` + `on_mouse_down(Right)` 记
  `set_context_cell(Some((row_ix, 0)))`——不补这一笔，点行号格会沿用上一个数据格的落点。
- ~~**仍在册**：行号槽右键的交互链路（真右键 → 记落点 → 弹菜单）**没有窗口用例**：当前
  测试设施没有模拟右键的入口，只能实机验证；菜单形态（纯函数）已锁死。~~ —— **2026-11-21
  已补窗口用例**：`right_clicking_the_row_number_cell_records_it_as_the_menu_target`（渲染两帧 →
  取 `editor-result-rowno-0` 的 `debug_bounds` 中心 → `simulate_mouse_move` + `simulate_mouse_down`
  均传 `MouseButton::Right` → 断言 `context_cell == Some((0, 0))`）。入口来自 `VisualTestContext`
  的 `simulate_mouse_move/simulate_mouse_down/simulate_mouse_up`（`gpui-pre-0.3.11/src/app/test_context.rs:935-977`），
  不是 `MouseDownEvent` 手搓。上一批判成「没入口」是只查了 `vendor/` 使用现场、没查 `gpui-pre` 测试支持。
  DBeaver `ROW_HEADER` 落点后续的过滤 / 排序 / 导航等通用子菜单未做（行号落点没有数据列语义）。
  行号是**视图行号**（`row + 1`，按当前视图行序）；若将来引入「换页替换」模式，需重新核对偏移。
- 门禁：panel **378**（第 47 批 +1、本批 +1）/ editor check `Finished` / editor **800**
  （1 ignored）/ ui-guard **4 passed**。（第 47 批 editor 首跑撞上编译期 `memory allocation failed`，
  用 `CARGO_PROFILE_TEST_DEBUG=line-tables-only` 重跑通过——只降符号表，不改用例行为；本批只动
  `#[cfg(test)]`，同 profile 增量跑未复现。）

### 2.52 筛选条「状态历史」后退 / 前进（2026-11-18 已实现主体）

全文见 `result-panel.md §8.63`。接手要点：

- **口径 ①（已拍板落地）**：一份状态 = **一份结果 SQL**。本仓重查只有
  `execute(ExecTarget::Statement(sql), ResultPlacement::Replace)` 一条通道（「筛选框运行」
  同款），所以不需要额外快照通道；「原 SQL / 下发筛选 / 排序」的差异都落在 SQL 文本上。
- 面板持历史：`state_history: Vec<String>` + `history_pos: usize`；`sync` 里 `note_state_sql()`
  入栈（去重 = 同一 SQL 只挪位置；从中间走新路的 `truncate(pos+1)` 作废前向；空 SQL 不入）。
- 宿主只执行：`ResultRequest::NavigateHistory(±1)` → `history_target` → `set_history_pos`
  → `execute(Statement(sql), Replace)`。执行完 `sync` 再记一次同款 SQL，去重把位置落回该条
  —— 这是「后退 / 前进」不乱套的关键。
- 按钮在筛选行「清除钮 / 历史筛选下拉」之后、「查找开关」之前，**分裂按钮**形态
  （`menu::DropdownButton`：左半 ghost xsmall `ChevronLeft` / `ChevronRight` 走一格，
  右半 caret 弹「该方向全部可达状态」菜单，点哪条发 `NavigateHistoryTo(pos)`）；
  `.disabled(!can_back/forward)`，tooltip = `history_hint`（`state_label` = 折叠空白 +
  50 字符截断补 `…`，对齐 `describeState`）；菜单条目由 `history_menu_items(delta)` 给（近的在前）。
- **收窄在册**：不还原行位置（DBeaver 的 `HistoryStateItem` 含 `rowNumber`，本仓回退后停首行）；
  视图态（筛选词 / 转置 / 列布局）不入历史；菜单条目不带「navigate backward to …」前缀文案。
- 用例：`state_history_dedupes_steps_and_drops_the_forward_branch`（纯状态机）、
  `history_buttons_sit_in_the_filter_row_and_gate_on_position`（在位数 + 启用态 + tooltip 口径）。
- **验证**：panel **273 → 275 passed** / editor `--lib results` 45 / guard 4 / check 0-0。

### 2.53 筛选条 ✕ 的箭头菜单（2026-11-18）

全文见 `result-panel.md §8.64`。接手要点：

- DBeaver `filtersClearButton` 是 `SWT.DROP_DOWN`：本体 `clearDataFilter(true)`
  （`ResultSetFilterPanel.java:1550`），箭头弹 `EraseItemListener`（`ResultSetViewer.java:1525-1545`：
  清筛选设置 / 显示隐藏列 / 重置颜色 / transformers / pins / 全部重置）。
- 本仓按「能力没有就不摆」只合并两条：**清除筛选词**（有词才出现）与**显示全部列**
  （有隐藏列才出现，走新增的 `ResultRequest::ShowAllColumns` → `delegate.show_all_columns()`
  + 网格 refresh + `snapshot_column_layout`）。
- 条目清单是纯函数 `erase_menu_labels(filter_text, hidden_columns)`（`panel/events.rs`）——
  `PopupMenu` 内部不可读，菜单形态只能靠纯函数钉。
- 形态：`DropdownButton` 分裂按钮（左半仍是 `editor-result-filter-clear`、`✕`、空词禁用；
  右半 caret，`menu_accessibility_label("清除选项")`）；两条都不可用时箭头仍在、菜单摆一条
  禁用说明「暂无可清除项」，宽度不跳。
- 用例：`erase_menu_lists_only_the_entries_this_repo_can_honour`。
- **验证**：panel **276 passed** / editor `--lib results` 45 / guard 4 / check 0-0。
- **2026-11-20 已变（见 `result-panel.md §9.27` 与本文件 §2.51 末段）**：第一条改为
  「删除所有过滤 / 排序」且**恒在**，左半主点击 = 全清，清除钮**恒可用**；原
  「清除筛选词（有词才出现）」窄项并入，用例改为
  `the_erase_menu_always_offers_the_full_clear`。「两条都不可用 → 摆禁用说明」这一分支
  已不可达（清单恒非空）。

### 2.54 右缘竖条顶部的面板总开关（2026-11-18）

全文见 `result-panel.md §8.65`。接手要点：

- DBeaver 右缘竖条第一颗就是总开关 `panelsButton`（`SWT.RIGHT | SWT.CHECK`、图标
  `UIIcon.PANEL_CUSTOMIZE`、tooltip `CMD_TOGGLE_PANELS`；点击 `showPanels(!isPanelsVisible(), true, true)`，
  checked = 面板可见，`ResultSetViewer.java:1038-1056`）；再往下才是每个面板一颗 CHECK
  （`verticalIndent = 2`），最后弹簧 + 底部布局开关。
- 本仓补的就是那颗缺的总开关：`editor-result-edge-panels`（ghost small、图标随状态
  `PanelRight` / `PanelRightClose`、`toggled(panels_visible)`、→ `ResultRequest::SetPanelsVisible(!visible)`）。
- 定义：面板可见 = `edge_bar_visible && show_value_sidebar`（竖条 + 右栏内容都在）；关 =
  整条收起（与状态栏右端三态开关的收起态同一定义）。关掉后竖条消失 → 重新打开走状态栏
  那颗开关（DBeaver 那边关掉后也是走菜单命令 `CMD_TOGGLE_PANELS`）。
- 状态栏三态开关**保留**（两颗改同一份状态，不是二选一）。
- `set_panels_visible(true)` 幂等（已可见则不动状态、不动 epoch）。
- 用例：`right_edge_bar_carries_the_master_panels_toggle`（在竖条里 + 收/放两向 + 幂等）。
  测试坑：竖条挂载条件是 `edge_bar_visible && result_can_copy`，测试壳默认
  `result_can_copy = false`，要先置真再出帧，否则 `try_find` 一定取不到。
- **验证**：panel **277 passed** / editor `--lib results` 45 / guard 4 / check 0-0。

### 2.55 底部动作条按 editCmds / navCmds 归位（2026-11-18）

全文见 `result-panel.md §8.66`，原文取证在 `.rds/tmp/db-status-toolbar.txt`
（DBeaver `plugins/org.jkiss.dbeaver.ui.editors.data/plugin.xml` 1407-1522）。接手要点：

- DBeaver 底部动作条是**三个**状态子贡献点装出来的，不能凭截图排：
  `editCmds` = applyChanges(pulldown) → rejectChanges → undo → redo ‖分隔‖ row.edit →
  row.add → row.copy → row.delete；`navCmds` = row.first → previous → **next** → last →
  fetch.page → fetch.all；`exportCmd` = 分隔 → export(pulldown)。
- 本仓改了三处：① 写操作段改成 `[提交·回滚·撤销·重做] ‖分隔‖ [修改·新增·复制·删除]`；
  ② 翻页段改成 `[首页·上页·下页·末页]` 后再接取下一段 / 取全部（`last` 不再被挤到取数钮之后）；
  ③ **补上「下页」钮**——`PageTarget::Next` 端口与宿主 `go_page` 早就实现了，只是面板从没注入
  按钮（这是本轮最实质的缺口）。
- 占位清单变 13 项，分界 `FIRST_PAGER_ACTION = 8` / `FIRST_SYSTEM_ACTION = 12`；改顺序时
  必须同步改 `grid::tests::status_action_placeholders_follow_dbeaver_group_order`。
- 仍在册（非本轮改动）：行导航语义本仓是「移一屏」（DBeaver 是「移一行」），取数是窗口化
  `FetchMore`（DBeaver 是 `fetch.page` 显式取页）。
- 新增用例 `status_row_pager_follows_dbeaver_nav_order`：按真实像素横坐标钉四颗钮的左右顺序
  （顺带示范：`ElementSnapshot::bounds()` 能取绝对位置，`total_rows > 0` 才有四颗钮）。
- **验证**：panel **278 passed** / editor `--lib results` 45 / guard 4 / check 0-0。

---

### 2.56 右栏每页工具条右端的「视图菜单」（2026-11-18）

全文见 `result-panel.md §8.67`，原文取证在 `.rds/tmp/db-panelsmenu.txt`
（`fillPanelsMenu` + `addDefaultPanelActions` + 三处调用点）。接手要点：

- DBeaver 每个面板的工具条是**两层**：左 `actionToolBar`（面板 `contributeActions`）+
  右 `panelToolBar`（`addDefaultPanelActions` 只填**一颗**「视图菜单」按钮，
  `ResultSetViewer.java:1590-1614`），点开是 `fillPanelsMenu()`（`:1559-1588`）。
- 菜单顺序：各面板 CHECK 项 → 分隔 → 最大化 → 激活面板 →（有 `FEATURE_PANELS` 时）
  分隔 → 面板总开关 → 布局方向。本仓前两项**没有端口**，按「能力没有就不摆」不列。
- 落地插入点是六页共用的 `panel_toolbar`（`panel/tabs.rs`，含 `empty_page` 空态页），
  改一处六页全生效；按钮 id `<页工具条 id>-view-menu`、分裂壳 `<…>-view-menu-anchor`。
- 清单 / 文案都是纯函数：`panel_view_menu_items()`（9 项）+
  `panel_view_item_label(item, panels_visible, vertical_layout)`（开关项报「点了会做什么」）。
  `PopupMenu` 内部不可读，形态断言一律钉在这两个纯函数上。
- 顺带收口：页签的图标 / 文案从两处（`render_right_sidebar` 的 `active_tab`、
  `render_right_edge_bar` 的六元组）收成 `events::right_tab_icon` / `right_tab_label`
  单一来源，视图菜单复用同一份。
- 新增用例 `right_panel_view_menu_lists_pages_then_the_two_toggles`（顺序 / 文案 / 六页挂载）。
- **验证**：panel **279 passed** / editor `--lib results` 45 / guard 4 / check 0-0。

---

### 2.57 右栏页签条列出全部六页、点即切页（2026-11-18）

全文见 `result-panel.md §8.68`。接手要点：

- DBeaver 右栏是**一个** `CTabFolder panelFolder`（`ResultSetViewer.java:1430-1478`），
  六页都是常驻页签（`addPanel` 里 `new CTabItem(panelFolder, SWT.CLOSE)`），点页签 =
  `setActivePanel`。本仓 2026-10-28 曾刻意收窄成「只显示当前页、`on_click` 空实现」，
  那等于摆了一颗死控件；本批改回六页常驻 + 点即入队 `SetRightTab`。
- 选中态用 `right_tab_index(当前页)`（0..5 真跟随），溢出用 `TabBar::menu(true)`
  （放不下时给一颗溢出菜单钮，避免被裁的页签只能靠滚轮找）。
- **顺序单一来源**：`events::right_tabs()` + `right_tab_index` + `right_tab_from_index`
  （越界兜底默认页）；`tabs.rs` 原 `#[cfg(test)]` 的 match 已删，视图菜单 / 右缘竖条 /
  页签条读同一份。新增页或调序只改 `right_tabs()` 一处。
- 页签行（24px `h_flex`）带 `debug_selector("editor-result-right-tabs")`：`TabBar`
  组件自身没有可查 id，按这一行的真实像素做 `simulate_click` 才点得到页签。
- **切页是宿主动作**：面板只 `queue.request`，断言要看**队列**（`panel.read(app).queue.drain()`）
  而不是面板的 `right_tab`（宿主 `dispatch` 才会调 `set_right_tab`）。
- **已落地**（原「仍在册」）：逐页关闭先由第 24 批的行尾 ×（`CloseRightTab`）承接，
  第 32 批再收到**每页签内侧**（`Tab::suffix` + `app.stop_propagation()`，见 §3.05）；
  与 DBeaver `CTabItem(SWT.CLOSE)` 同构，剩下的只是视觉形态。
- **验证**：panel **280 passed** / editor `--lib results` 45 / guard 4 / check 0-0。

---

### 2.58 筛选「运行」与「下发」共用一套 wrapped SQL 形状（2026-11-18）

全文见 `result-panel.md §8.69`。接手要点：

- 两条重查链路（运行 = 替换、下发 = 新结果集）此前各拼各的 SQL：运行是宿主内联
  `select * from (…) as _q where 词`，下发是 `engine::sql::rewrite_with_filter`。
  别名不同、运行那条**不摘 LIMIT**、`ORDER BY` 留在子查询里。
- 现统一在 `crates/sql/src/filter.rs`：新增 `rewrite_with_where(original, condition)`
  与 `rewrite_with_filter` 共用拼装（`rds_filtered` 别名 / 摘顶层 LIMIT / `ORDER BY` 外提 /
  `dropped_limit` 回报），只差条件生成。改这两条链路的 SQL 形状只改这一个文件。
- 对齐依据：DBeaver 的筛选条文本就是一条 WHERE 条件（`ResultSetFilterPanel.setCustomDataFilter()`
  `:618-648` 直接 `setWhere(text)`）。
- 右栏「执行SQL」页的预览串也改走同一函数——预览必须等于真要跑的那句。
- **未统一的口径**：运行按「条件」、下发按「关键词（全列 LIKE）」。统一需同时改下发链路与
  本地过滤语义，属拍板项（已登记）。
- 新导出符号 `rds_sql::rewrite_with_where`，`engine::sql`（`pub use rds_sql::*`）与
  `engine` 根都随之可用；`result_panel` 已依赖 `engine`，直接调即可，不必自己拼串。
- **验证**：sql 15 passed（filter 模块）/ panel 280 / editor 45 / guard 4 / check 0-0。

---

### 2.59 工具栏补上「筛选设置」+ 自定义 ORDER BY（2026-11-18）

全文见 `result-panel.md §8.70`。接手要点：

- 筛选条工具条的最后一块缺口 `filtersCustomButton`（FILTER 图标、恒建，DBeaver
  `ResultSetFilterPanel.java:330-338`）已补：`editor-result-filter-settings`，位置在 ✕ 之后、
  筛选历史之前，点击入队 `OpenFilterSettings`。
- 打开的是本面板内的浮层（`dialog.rs::render_filter_settings`，absolute 遮罩，与 SQL 预览
  弹窗同范式）：**WHERE 只读回显** + **自定义 ORDER BY 输入** + 取消 / 应用。
  WHERE 不在浮层里编辑——筛选框本体是唯一输入源（单输入源硬约束）。
- 应用链路三段：`CommitFilterSettings`（读输入框要 `&mut Context`）→ 宿主调
  `panel.apply_filter_settings()` → `ApplyFilterSettings { where_text, order_text }` →
  宿主 `run_custom_filter`（`host/results/filter.rs`）。
- 引擎侧新增 `rds_sql::rewrite_with_where_order(original, condition, order_by)`；
  `rewrite_with_where` 是它传 `None` 的转调。**自定义排序是替换而非追加**（外层只允许一句 ORDER BY）。
- 卡片宽度是 `ui::RESULT_FILTER_SETTINGS_WIDTH`——`result_panel/src/panel/dialog.rs` 的裸 `px`
  计数已被 `rds-ui-guard` 钉住（baseline 3），再加一个就会红。
- **仍在册**：DBeaver 的「Columns」页签（列约束树 + 列可见勾选 + 列顺序移动）未做；
  WHERE 在对话框内可编辑 vs 本仓只读；自定义排序不入状态历史 / 不落盘。
- **验证**：sql 17 / panel **281** / editor 45 / guard 4 / check 0-0。

### 2.60 筛选设置浮层扩成两页签，「列约束」页可见 / 冻结真能切（2026-11-18）

全文见 `result-panel.md §8.71`。接手要点：

- DBeaver 那份 `FilterSettingsDialog` 是 `CTabFolder` **两页**、**先建列页并 `setSelection(0)`**；
  §8.70 只落了 Custom 页，本批补成「列约束 / 自定义条件」两页签，默认停在列约束。
- 页签是纯 UI 状态：`FilterSettingsTab { Columns（默认）, Custom }` + 请求
  `SetFilterSettingsTab`，面板 `set_filter_settings_tab`（不入队任何重查）。
- 列页的**列清单不是假控件**：`column_constraint_rows(cx)` 从网格 delegate 现算
  `(列名, 可见, 冻结)`；勾选回调就地改 delegate（`set_columns_hidden` / `toggle_freeze`）
  + `ColumnLayoutChanged`，落盘通道与列头菜单完全一致。列页的「应用」因此置灰。
- **仍在册**：criteria 列（本仓列级条件走列头菜单，无持久状态可投影）、order 列
  （排序走服务端下发）、工具条 8 项（移顶/上移/下移/移底/排序/全显/全隐/重置；
  列序移动 `grid/table_impl.rs::move_column` 已实现但没接 UI）。
- 三条已踩过的 vendor 事实（别再撞）：`gpui_component::checkbox::Checkbox::on_click/on_change`
  是 **3 参** `Fn(&bool, &mut Window, &mut App)`（不是 gpui-base 的 4 参）；
  `Tab::new()` **无参**（没有逐页 id）；`TabBar::with_size(Size::Small)` 在
  `panel/dialog.rs` 会报 `gpui_kit::Size<T>` 无 `Small`（`sidebars.rs` 同写法却能过，原因未查明），
  本批直接不设 size。
- **验证**：panel **282**（新增 `filter_settings_columns_tab_projects_the_grid_column_state`）/
  editor 45 / guard 4 / check 0-0。

### 2.61 列约束页补「条件 / 排序」只读两列 + 页内工具条三颗（2026-11-18）

全文见 `result-panel.md §8.72`。接手要点：

- `column_constraint_rows()` 现在返回 `ports::ColumnConstraint { name, visible, frozen,
  criteria, sort }`（不再是三元组）。`criteria` ← `delegate.column_filter()`（列头菜单写入）、
  `sort` ← `delegate.sort_state()`（列头点击的本地排序），两者**只读**。
- 页内工具条三颗：`ColumnsShowAll` / `ColumnsHideAll` / `ColumnsResetLayout`
  → 面板 `show_all_columns` / `hide_all_columns` / `reset_column_layout`，
  三者都走 `snapshot_column_layout` → `ColumnLayoutChanged`（与列头菜单同一条落盘通道）。
- **全部不显示有底线**：委托逐列 `hide_column` 保「至少留 1 列可见」，故结果是只剩一列
  （DBeaver 允许全隐，这里是有意差异，别再当 bug 修）。
- **四颗移动钮 + 「排序（列名 A→Z）」未做**：本仓列序权威在组件 `TableState` 的 `col_groups`，
  delegate 只有拖拽回调里同步 `display_order` 一条通道（`grid/table_impl.rs::move_column`）。
  要接先拿到「程序化重排列序」的组件接口，否则按钮是假的。
- **验证**：panel 282 / editor 45 / guard 4 / check 0-0。

### 2.62 列序成为视图列序权威 + 列约束页移动四钮与排序（2026-11-18）

全文见 `result-panel.md §8.73`。接手要点：

- **列序链路**：`display_order`（列名序，持久化同源）→ `order_cache`（数据列下标，新增字段）
  → `data_column` / `view_column`（跳过隐藏列，读缓存）。任何改 `display_order` 的地方
  **必须**跟一次 `rebuild_column_order()`，漏了就退化成「视觉序与取数序不一致」。
- **已修的既有隐患**：组件拖拽 `TableState::move_column` 改 `display_order` 后不 refresh；
  一旦之后发生 `refresh`（`state.rs:442` → `:731` 按 `delegate.column(i)` 全量重建
  `col_groups`），视觉列序会退回隐藏列序且表头 / 单元格错位。所以 `table_impl.rs::move_column`
  末尾现在也重建缓存。
- **程序化排列列序不需要碰 vendor**：改 delegate 的列序 + `TableState::refresh(cx)` 即可
  （`prepare_col_groups` 每次按 `columns_count` / `column(i)` 重建）。这是 2.61 里写的
  「未做」那条的解法。
- **面板侧入口**：`move_selected_column(ColumnMove, cx)`（作用 `selected_col`，成功才
  `snapshot_column_order` 推 `ColumnOrderChanged`）、`sort_columns_by_name(cx)`；请求
  `MoveSelectedColumn(ColumnMove)` / `SortColumnsByName`。
- **仍是已知差异**：移动只作用于当前选中列（DBeaver 支持多选批量）；重置会把列序
  回**本份结果的列数据序**，不是用户上次持久化的列序（要那个得先取回 `SavedSession`
  的 `column_order` 再 `apply_column_order`）；重置不跨页清 where/order 草稿。
- **验证**：panel 283 / editor results 45 / guard 4 / check 0-0。提交 `c70a88be`。

### 2.63 状态栏命令项按「工具条配置」显隐 + 配置菜单（2026-11-18）

全文见 `result-panel.md §8.74`。接手要点：

- 可见性口径在 `ports::StatusCommand`：`ALL`（16 项，顺序 = DBeaver 底部工具条）+
  `default_visible()` + `menu_label()`；菜单内容用 `grid::status_config_items(hidden)`
  纯函数钉，不要把文案散回渲染里。
- `render_status_row` 现在要两个新入参（`hidden_commands` / `config_menu`），
  新增调用方漏传会编译失败而不是静默丢功能。左动作组的槽位是「按命令判断再 push」，
  加新命令时记得同时改 `StatusCommand::ALL` 与三个分段的组装点。
- 默认隐藏只有 Undo / Redo；**`FetchPage` 有意常显**（本仓窗口化取数的唯一入口），
  别再照 DBeaver 的 `fetch.page defaultVisibility=false` 改。
- 状态是纯 UI 态（`hidden_status_commands`），不落盘；要持久化就接 `SavedSession`
  （与列宽/列序同一条通道）。
- 未做的一半：DBeaver `ConfigAction` 的「结果集首选项页」（齿轮 → 偏好页）本仓还没有。
- **验证**：panel 285 / editor results 45 / guard 4 / check 0-0。提交 `e1b797db`。

### 2.64 筛选历史 tooltip 补命令描述（2026-11-18）

全文见 `result-panel.md §8.75`。接手要点：

- tooltip 口径 = `state_label(目标状态 SQL) + " (" + 命令描述 + ")"`，命令描述是
  「后退历史」/「前进历史」；改文案的地方只有 `panel::events::history_hint`。
- 空历史 / 已到头仍走说明句；不要在渲染里再拼一次括号（会双份）。
- 已知差异仍在：DBeaver 的 `describeState` 还带**容器名**（`ResultSetViewer.java:5457-5473`
  = 容器名 + `" [" + 条件 + "]"`），本仓历史只存 SQL，没有容器名可拼；状态历史仍不还原
  行位置、视图态（筛选词 / 转置 / 列布局）也还不入历史。
- **验证**：panel 285 / editor results 45 / guard 4 / check 0-0。提交 `a5bede1e`。

### 2.65 状态行齿轮 → 设置页结果集节（2026-11-18）

全文见 `result-panel.md §8.76`。接手要点：

- 链路：状态行最右端 `editor-result-status-preferences`（`IconName::Settings`）→
  `ResultRequest::OpenResultPanelSettings` → editor `dispatch` 用窗口句柄
  `window.dispatch_action(settings::commands::OpenResultPanelSettings)` → workbench
  `on_action` → `open_settings_at(Some("result_panel"))` → `SettingsPage::new_at`
  → `skeleton::build(..., initial_section)` → `page_index_for` 折算 `page_ix`。
- 契约红线：`result_panel` **不得**直接依赖 `settings` crate；要加「从面板打开某页」的能力，
  一律走「面板推请求 + 宿主派发动作」这条通道。
- `render_status_row` 现在多了 `settings_button` 一个入参（新调用方漏传会编译失败）。
- 设置窗口已开着时齿轮不切页（保持 `open_settings` 的「已开则返回」语义）；要对齐 DBeaver
  的 `showPreferencesFor` 得给已开窗口发「选页」信号（未做）。
- **并行线注意**：本批提交时工作树曾被切到 `refactor/header-extract`（并行线分支），
  提交落在该分支后已用 `git branch -f main <sha>` 快进归到 `main`；两分支当时 tip 一致。
- **验证**：settings 37 / panel 286 / editor results 45 / guard 4 / check 0-0。提交 `c398032f`。

### 2.66 状态栏命令显隐落盘（2026-11-18）

全文见 `result-panel.md §8.77`。接手要点：

- 状态有两处：面板内存态 `hidden_status_commands: BTreeSet<StatusCommand>`（渲染用）与
  全局偏好 `result_panel.hidden_status_commands`（落盘用，逗号分隔 kebab id）。
  面板是内存权威，偏好是跨会话权威；**写回永远以面板集合为准**
  （`StatusCommand::hidden_list_text`，按 `ALL` 顺序 ⇒ 稳定文本）。
- id 红线：`StatusCommand::id()` 是落盘契约，**不得改名**；新增命令必须同时补
  `id()` / `from_id()` / `ALL` / `menu_label()` / `default_visible()`，否则往返用例红。
- 显隐写入口有两个：状态行末端 ▾ 菜单（`SetStatusCommandVisible` → 即时生效 + 落盘）、
  设置页「状态栏隐藏的命令」（Text 文本，`NextUse`：下次创建面板生效）。两者写的是
  同一条偏好，但生效时机不同（取舍见 §8.77 末段）。
- `parse_hidden_list` 忽略未知 id / 空白项：settings 层**不做** id 校验（依赖只向下），
  手改设置文件写错的 id 只是静默无效。
- 默认值 `"undo,redo"` 必须与 `StatusCommand::default_visible()` 的口径一致
  （撤销 / 重做默认隐藏，`fetch-page` 有意常显）；改默认要两边一起改，登记表默认值
  一致性用例会拦。
- **验证**：settings 37 / panel 288（+2 用例）/ editor results 45 / guard 4 / check 0-0。
  提交 `7836006e`。

### 2.67 列浏览器页「生成 DDL」（2026-11-18）

全文见 `result-panel.md §8.78`。接手要点：

- DBeaver 这条动作生成的是**新表** DDL（走数据迁移映射 `DatabaseMappingType.create`），
  不是来源表 DDL。本仓没有迁移机制，类型**照驱动口径**出；驱动没报的列按值族给不带
  精度的通用类型、行尾注明「按值族推断」；连值族都认不出（`ColumnKind::Unknown`）
  则整份不出 SQL、只给说明（对齐 `sql_generator_nonsql_text`）——**不要**为了让输出
  「看起来完整」而编类型。
- 纯函数入口 `result_panel::ddl::create_table_ddl(table, &[DdlColumn])`；
  类型解析 `ddl::resolve_type(driver_type, kind) -> Option<(String, bool)>`
  （第二个返回值就是「是不是推断的」，决定要不要加行尾注明）。改生成规则改这里，
  面板只负责取素材。
- 面板侧：`open_ddl_dialog(window, cx)`（`TextareaState::new` 需要 `&mut Window`，
  所以由宿主经 `ResultRequest::GenerateResultSetDdl` 分派）。素材 = 网格 delegate 的
  `columns()` / `column_types()` / `column_annotations()`（后者的 `data_type` 是
  NavCache 的驱动类型，比渲染层 `ColumnType.name` 更权威）。
- 预览复用 SQL 预览浮层：`sql_dialog_title`（`None` = 原始 SQL）。**两个入口都要复位**
  （`open_sql_dialog` / `close_sql_dialog` 已复位），新增第三个复用场景时别忘了。
- 未做（DBeaver 有、本仓无）：树形嵌套列（`getNestedBindings`）、右键
  「复制列名」项、可编辑 SQL（本仓浮层只读）、表名用户输入。
- **并行线阻断**：本批 `cargo check -p rds-workbench` 失败于
  `crates/connection_dialog/.../managers.rs` 6 处 E0308（并行线在改，与本批无关）；
  本线三 crate（settings / result_panel / editor）check 0-0。
- **验证**：settings 37 / panel 296（+8 用例）/ editor results 45 / guard 4。提交 `140079c1`。

---

### 2.68 「引用」页的接手要点（外键清单已接，引用行未接，2026-11-18）

- 页面形状与 DBeaver 一致：**页内工具条没有动作**（`ReferencesPanel.contributeActions` 是空实现），
  正文才是内容。别往该页工具条里加按钮——DBeaver 那边加不了。
- 清单取数在宿主，不在面板：面板只交 `schema / table / visible_columns`
  （`ResultRequest::LoadReferences`），宿主在后台任务里走
  `MetadataService::list_constraints`。**不要在 render 里发起任何元数据查询**
  （面板 render 是 `&App`，且 UI 线程不 await 元数据是硬约束）。
- 收键规则是 DBeaver `:298` 那条「FK 的全部列 ⊆ 可见列」，实现放在纯函数
  `references::keys_from_constraints`（列名忽略大小写）。改规则只改它 + 它的用例。
- 「无引用」不是错误：驱动不支持外键 / 查询失败 → 静默成 `Loaded([])`（DBeaver 只 `log.debug`）；
  只有**宿主自身原因**（未绑定连接）才落 `Failed` 并显示原因。
- 下一步（引用行）按 `result-panel-references-plan.md` 的 `B′`：
  用选中行值生成「外键条件 SQL」，交给编辑器**新开一份结果集**，
  不要尝试在面板内嵌第二份 grid/store（那是 `B`，会打破「一份结果集 = 一份 store/exec 上下文」）。
- 入向引用（子表 → 本表）没做：要扫 schema 内其它表的约束（N 次往返，ER 那边有 120 表上限可参考）。
- `catalog` 目前传空串（结果集列注解只有 `schema.表`，没有目录层）；多 catalog 驱动读不到时先查这里，
  别先怀疑约束过滤。


### 2.73 段大小输入框的接手要点（2026-11-18）

- 「段大小」的唯一权威是**面板状态** `ResultPanel.segment_rows`（默认 1000 =
  `ui::RESULT_SEGMENT_ROWS_DEFAULT`，与编辑器 `execution::SEGMENT_ROWS` 同值）。
  消费点只有一处：`view/host/results/copy.rs::fetch_more` 的
  `ExecTarget::Segment { limit }`。别再回到写死常量。
- 输入解析在 `panel::events::parse_segment_rows`：`>0` 才算有效（DBeaver 只在 `>0` 时写
  偏好）；`0 / 负数 / 非数字 / 小数` 一律 `None` = 什么都不做。下限
  `ui::RESULT_SEGMENT_ROWS_MIN = 1` 对齐 `ResultSetPreferences.MIN_SEGMENT_SIZE`。
- 状态行注入方式与行数框一致（面板建 `InputState` + 订阅，`render_status_row` 取
  `Option<&Entity<InputState>>`）；**调试选择器挂外层行**——`Input` 是 RenderOnce 组件，
  内部元素的 id 进不了渲染观测。
- 测试壳默认不注入 `status`，状态行整块不渲染（`show_status` 口径）：断言状态行元素前
  先 `p.status = Some(ResultStatus { .. })`。
- 未落地：~~跨会话持久化~~（已由 §2.74 补齐）；~~失焦回填~~（已由 §2.80 补齐，
  `InputEvent::Blur` 是有的，此前"没有失焦钩子"的结论已作废）。

### 3.09 筛选设置列约束表的列序（2026-11-20）

- 竞品列序（`FilterSettingsDialog.addColumn` 依次挂）：Name `:129-137` → `#` `:139-146`
  → Visible `:148-176`（可编辑）→ Pinned `:178-211`（可编辑，顶层属性才允许）→
  Order `:213+`（仅 `supportsResultSetOrdering()`）。
- 本仓改法：`#` 从行尾挪到列名正后方（`rems(2.)` 左对齐，表头同步）；
  可见/冻结本来就是可编辑勾选框（对 DBeaver 的 `EditingSupport`），不动；
  「条件 / 排序」是本仓追加的只读投影列，仍在末尾。
- 用例：`filter_settings_columns_put_the_position_right_after_the_name`（几何断言
  `pos < vis < pin < criteria`；两个 `Checkbox` 各包一层带 id 的 div 才能被 `debug_bounds` 量到）。
- **登记差异**：`#` 取值 = 当前数据列序（DBeaver = `OriginalVisualPosition + 1`，移动列后不变）。
  本仓列序即持久化显示序（`display_order`），没有「模型原始序」；要复刻得给 delegate 加不可变
  初始序 + 定义「重置列布局」语义，属独立设计题。
- 门禁基线：panel **361**、editor check 通过、editor **798**（1 ignored）。

### 3.08 分组列下拉标位次（2026-11-20）

- 承 §3.07：键顺序可改了，但下拉里看不见顺序。DBeaver 的顺序在 `GroupingConfigDialog`
  的表格行序里天然可见；本仓换成勾选下拉后丢了这层信息。
- 落点：`tabs::group_column_menu_label`（已勾选 → `"{位次}. {列名}"`，否则原名），
  菜单项文字改用它；勾选态 / 点击 / 菜单项顺序（按数据列序）都不动。
- 用例：`group_column_menu_label_marks_only_selected_columns`（纯函数，因 `PopupMenu`
  内部不可读）。
- 门禁基线：panel **360**、editor check 通过、editor **798**（1 ignored）。
- flaky 备查：editor 4 线程偶发 `exec/.../queue.rs:143`「中断在它开跑前就到了」，
  该目录无改动、单跑通过、重跑全绿——并发时序抖动，不在本线修。

### 3.07 分组键可上移 / 下移（2026-11-20）

- 竞品：`GroupingColumnsContainer.moveColumns:126-152` —— 分组键顺序 = 嵌套顺序；列分
  「属性列 / 函数列」两段，函数列只能在属性列之后；入口是 `GroupingResultsDecorator.dropColumns`
  （拖放）。`GroupingPanel.loadSettings`/`saveSettings` 是空桩，顺序不落盘。
- 本仓：`group_columns` 顺序即层级；新增 `MoveGroupColumn { column, delta }` +
  `tabs::can_move_group_column` / `tabs::move_group_column_in_place`（纯函数，相邻交换）+
  `events::move_group_column`；分组页工具条在勾选下拉后加一对 Chevron 按钮。
- 形态差异（有意）：DBeaver 拖放，本仓上移/下移一位——键盘可达、headless 可验，不引入
  拖拽排序模型。要做拖拽时把纯函数的 `delta` 换成 `to_index` 就能复刻 `moveColumns` 的落点语义。
- 用例：`moving_group_column_reorders_the_group_keys`（纯函数 + 面板态）、
  `group_move_buttons_queue_the_request_for_the_selected_column`（窗口级点击 → 队列）。
- 门禁基线：panel **359**、editor check 通过、editor **798**（1 ignored）。ui-guard 仍红，
  归因并行线 `crates/workbench/src/unlock.rs`。
- 未做：拖拽排序；函数列段的重排（本仓分组页没有聚合函数列，见 §9.15 的「不做」清单）。

### 3.06 右栏关空后重新展开恢复默认页（2026-11-20）

- 竞品口径：`showPanels:1478-1518` 的展开分支带 `showDefaults=true`，先
  `activateDefaultPanels`（`:1493-1496`）再放开 sash；`removePanel:1441-1451` 关空只
  触发 `showPanels(false, true, true)`。所以 DBeaver 里**没有「展开后页签夹空」这种态**。
- 本仓修复前的洞：`close_right_tab` 关空把 `open_tabs` 留空，`set_panels_visible(true)`
  只翻可见位 → 重新展开是「右栏空白、一个页签都没有」。
- 修法：`events::restore_default_tabs_when_empty()`（**仅空集时**填回 `right_tabs()` 并
  归一当前页），在 `set_panels_visible(true)` 与 `toggle_value_sidebar` 的显示方向各调一次。
  非空的自定义集合必须原样保留——否则每次开合都会冲掉用户关掉的页。
- 用例：`reopening_after_closing_all_pages_restores_default_tabs`（先红后绿）、
  `toggling_the_sidebar_keeps_a_custom_tab_set`（反向）。
- 门禁基线：panel **357**、editor check 通过、editor **798**（1 ignored）。
  **ui-guard 本批红，归因外部**：`no_new_hardcoded_color_or_pixel` 指向并行线未跟踪
  文件 `crates/workbench/src/unlock.rs`（裸 px 未登记），本批未碰 workbench。
- 未做：`showPanels` 还会在展开后把焦点还给「之前那个页签的内容」
  （`:1504-1506`）与 `showAttribute(currentAttribute)`（`:1508-1511`）；本仓焦点由
  点击来源决定，不做程序化回焦（属交互细节）。

### 3.05 逐页关闭按钮收到页签内侧（2026-11-19）

- DBeaver 取证：`addPanel:1357` 的长相是 `new CTabItem(panelFolder, SWT.CLOSE)`
  ——关闭样式位属于**那个页签**，所以能关任意一页；此前本仓只有行尾一颗「关当前页」。
- 本仓：页签构造加 `Tab::suffix(close)`，`close` 是 12px × 的自绘点击区
  （`Icon::new(IconName::X).size_3()` + `.hover(list_hover)`），点击推
  `CloseRightTab(tab)`——端口与视图菜单 / 竖条共用，别另开一个「关页」请求。
- **两个坑（都踩过）**：① 不能用 `Button`，XSmall 档 `min_h_7`(28px) 撑破 24px 页签条；
  ② 必须 `app.stop_propagation()`，否则 `TabBar` 外层的切页回调按**渲染时**的
  `open_tabs` 快照映射下标，会把刚摘掉的页按旧下标重开（实测队列
  `[CloseRightTab(Browser), SetRightTab(Browser)]`）。
- 选择器 `events::tab_close_selector(tab)` 六页各一个（`&'static str`，供
  `debug_bounds` 定位）；共用 id 会让定位只命中第一个，也是 a11y 重复 id 风险。
- 用例：`right_tab_close_button_closes_only_that_page`（窗口级，点非当前页的 × →
  队列只有 Close、没有 SetRightTab）、`right_tab_close_selectors_are_distinct`。
- 门禁基线：panel **355**、editor check 通过、editor **798**（1 ignored）、ui-guard **4**。
- 未做（目视）：× 的内距 / 悬停色 / 未激活页签上的表现；DBeaver 由 `CTabFolder`
  内建绘制。

### 3.04 分组页补「不排序」档（2026-11-19）

- DBeaver 取证：`ChangeSortingAction` 的常量是 `ASC = "ASC" / DESC = "DESC" / NONE = ""`，
  `descending == null` 即「不排序」，三个动作在 `DefaultSortingAction:44-46` 里按顺序装进
  一个 `AS_DROP_DOWN_MENU`；`GroupingResultsContainer.rebuildGrouping:249-265` 只在值
  **非空**且 `isDefaultGrouping`（函数列只有 COUNT）时 `dataFilter.setOrder(别名 + " " + 值)`，
  所以 NONE 档 = **不设 ORDER BY**，结果就是数据源/扫描顺序。
- 本仓映射：`GroupSort::None ↔ NONE("")`、`GroupSort::CountDesc ↔ DESC`；
  `GroupSort::Value`（按分组键字典序）是**超集档**，DBeaver 的 `ASC` 是「COUNT 列升序」，
  不是一回事（差异继续登记在 §3.05 之前的旧节 / `result-panel.md` §9.05）。
- 实现要点：`GroupSort` 加 `None` 变体（`#[default]` 仍是 `Value`，出厂档不动）；
  `toggled` 三档循环 `None → Value → CountDesc → None`；`id` = `none`/`value`/`countdesc`
  直接作为 `result_panel.group_sort` 的落盘值（`dispatch.rs` 写的就是 `sort.id()`，无需改）。
- **「不排序」不需要额外状态**：`arrange_groups` 的 `None` 分支什么都不做，
  因为 `group_by_columns_for` 已经是**首现序**——单列走 `group_by_for`、多列走
  `grid/stats.rs::group_by_tuples:89-99` 的 `Vec` 线性插入，两条路径都没有 `HashMap`。
  别为了「原序」引入 `BTreeMap` / 排序后再还原，那反而会打乱首现序。
- 设置侧口径：`Slot::ResultPanelGroupSort` 白名单与 `set_result_panel_group_sort` 归一
  都扩到三档；**未知值仍回落 `value`**（手改坏配置不该让面板乱排序），
  `registry/entries/result_panel.rs` 的 hint 已删掉旧「本仓暂不做」句。
- 用例：`group_sort_ids_round_trip_and_reject_unknown`（三档往返、`"none"` 不再是未知）、
  `group_sort_toggle_cycles_through_three_modes`（循环回「不排序」+ 三标签互不相同）、
  `arrange_groups_sorts_and_filters`（先以 `None` 跑一遍断言仍是 `["b","a","c"]` 首现序）、
  `group_prefs_initial_round_trip_and_unknown_sort_falls_back`（`"none"` 注入后 `group_sort_id() == "none"`）。
- 门禁基线：settings **41**、panel **353**、editor check 通过、editor **798**（1 ignored）、
  ui-guard **4**。
- 形态差异（未改，属目视/形态范围）：DBeaver 是下拉里的三个单选，本仓是一个循环按钮
  （点一次换一档，三档可达）；另 DBeaver 的三档只在「函数列只有 COUNT」时生效，本仓不受限。

### 3.03 列浏览器过滤框 ESC 清空（2026-11-19）

- DBeaver 取证（`MetaDataPanel.java:84-98`）：过滤行 = 标签 + `Text(SWT.BORDER|SWT.SEARCH|SWT.ICON_CANCEL)`，
  每改一次 `refresh(true)`；`KeyListener` 里 `SWT.ESC → setText("")`。`refresh:178-183`
  只按列名大小写不敏感匹配，集合不变则早退。
- 本仓：过滤框仍是 kit `List` 内置 searchable（不手搓）；`List` 外套一层
  `editor-result-col-browser-list` 容器挂 `on_key_down`，`escape → ListState::set_query("")`
  （该 API 内部会直接起一次搜索，等价「清空 + refresh」）。kit `Input` 不消费 Esc，
  焦点在搜索框也能冒泡到容器。
- 用例 `column_browser_filter_clears_on_escape`（窗口级：set_query("amount") → 1 行；
  focus + simulate_keystrokes("escape") → query 空、matched 回 2）。
- 门禁基线：panel **352**、editor check 通过（唯一 warning 来自并行线 host/mod.rs）、
  editor **795**（1 ignored）、ui-guard **4**。
- 登记差异（未改）：本仓过滤还匹配类型名 / 注释（此前拍板加宽），DBeaver 只匹配列名。

### 3.02 竖条 / 视图菜单页项 = 「开/关该页」toggle（2026-11-19）

- DBeaver 取证：`ResultSetHandlerTogglePanel.toggleResultsPanel:64-69` +
  `showResultsPanel:71-81`——`isPanelsVisible() && isPanelVisible(id)` 为真 →
  `closePanel(id)`（关**那一页**）；否则 → `activatePanel(id, true, true)`
  （打开并激活，整栏收起时顺带展开）。竖条每页 `SWT.CHECK`（`:1056-1080`），勾选态 =
  `panelsVisible && isPanelVisible(id)`（多页可同时勾亮）；视图菜单页项
  （`fillPanelsMenu:1559-1588`）绑同一命令，勾选态 = `isPanelVisible(id)`（`updateElement:99`）。
- 本仓：`toggle_right_tab` 改为「可见 → `close_right_tab`（返回 false）；否则
  `set_right_tab`（返回 true）」；新增纯函数 `edge_tab_active` / `page_item_closes`；
  `edge_tab_tooltip(tab, is_open, panels_visible)` 三态报动作；`dispatch.rs` 的
  `ToggleRightTab` 落盘判据改为「switched 或 `open_tabs` 有变化」。
- 用例：`clicking_an_edge_tab_toggles_that_page`（原 `..._is_a_noop` 作废）、
  `closing_the_last_edge_tab_collapses_the_bar`、
  `view_menu_page_item_closes_only_when_the_bar_is_open`、
  `edge_bar_tooltip_reports_the_toggle_action`（原 `..._carries_label_and_description` 三态断言作废）。
- 门禁基线：panel **351**、editor check 通过、editor **795**（1 ignored）、ui-guard **4**
  ——第 28 批遗留的 editor E0063 已由并行线补齐，editor 两道门禁恢复全绿。
- 未做：逐页关闭已由 §3.05 收到页签内侧（自绘 12px × + `Tab::suffix`；DBeaver 是
  `SWT.CLOSE` 由 `CTabFolder` 内建绘制，视觉差异属目视范围）。

### 3.01 列浏览器工具条只留「生成 DDL」（2026-11-19）

- 对齐 `MetaDataPanel.contributeActions:204-215`：页内工具条只贡献一条「生成 DDL」。
  多选动作在 DBeaver 那边挂**清单右键菜单**（`fillCustomActions:234-252`，操作对象 =
  `getStructuredSelection()`）。
- 本仓：列浏览器工具条只剩 `editor-result-generate-ddl`；「已选 N 列」chip 与
  「隐藏所选 / 显示所选 / 清空选择」三颗按钮改挂行右键菜单尾部，由纯函数
  `column_browser::batch_menu_items(selected, clicked)` 决定是否出现（右键列须在集合内），
  标签带集合规模。
- 用例 `column_browser_toolbar_keeps_only_the_ddl_action`（纯函数 + 渲染层：DDL 在、
  三颗批量按钮不在）。
- 门禁基线：panel **349**、ui-guard **4**。**editor 门禁本批未跑通**——并行线未提交改动
  （`crates/sql/src/formatter.rs` 新增 `FormatOptions` 字段，`crates/editor/src/view/host/format.rs:36`
  未跟进，E0063）；本批未改 editor，等并行线修好重跑。

### 3.00 页内工具条列位 + 分隔线只值页（2026-11-19）

- 对齐 `ResultSetViewer.addPanel:1337-1383` 的 `GridLayout(2, false)`：左列
  （`panel.contributeActions` + `FILL_HORIZONTAL`）= 页内动作**贴左**；右列
  （`addDefaultPanelActions` + `HORIZONTAL_ALIGN_END`）= 视图菜单**贴右**。
  本仓 `panel_toolbar` 已按此列位重排：左组 = 标题 + `Toolbar`，右 = `panel_view_menu`。
- 新增纯函数 `events::page_needs_separator(tab)`（只有 `RightTab::Value` 为 true，
  对齐 `ValueViewerPanel.java:171-173`；其余继承 `ResultSetPanelBase.java:26-28`）；
  `panel_toolbar` 多一个 `separator: bool`，`empty_page` / `empty_page_lines` 多一个 `tab`，
  七处调用点按页传值。
- 有意差异（保留）：工具条左边的页标题（DBeaver 面板头无标题，页名只在页签上）；
  统计页标题带列名，仍有信息价值。
- 用例 `panel_toolbar_puts_actions_left_and_view_menu_right`（纯函数 + 窗口列位四条断言）。
- 门禁基线（本批之后）：panel **348**、editor **794**（1 ignored）且 check 0 warning、ui-guard **4**。

### 2.99 `open_tabs` 已落盘（2026-11-19）

- 偏好键 `result_panel.open_tabs`（Text）：出厂六页全开，落盘值 = 逗号分隔的
  `RightTab::id()`（`browser,value,stats,group,reference,execsql`）。**空串归一为出厂全集**
  （对齐 `activateDefaultPanels` 的「空集合补默认」），所以「全关」不会被记成空白条。
- 新增 `Slot::ResultPanelOpenTabs`；`ports::RightTab` 有 `ALL` / `id()` / `parse()`（未知值
  返回 `None`）。面板入口 `set_open_tabs_initial(tabs, cx)`：按规范序去重、丢未知、
  空集合保持现状、当前页被关掉则落到集合第一页。
- 宿主：`construct.rs` 启动注入；`dispatch.rs::persist_result_open_tabs` 在
  `CloseRightTab` / `OpenRightTab` / `ToggleRightTab`（`switched`）后写回。
- 仍不落盘：当前页 `right_tab`（DBeaver 也不把选中页写进 `enabledPanelIds`，不算差异）。
- 门禁基线（本批之后）：settings **37**、panel **347**、editor **794**（1 ignored）、ui-guard **4**。

### 2.98 方案 B 第 2 步：竖条不再「点当前页收起整栏」（2026-11-19）

- `toggle_right_tab` 现在是**纯切页**：未启用 → 先启用再切（`true`）；已启用且正在显示
  → 无操作（返回 `false`，派发侧据此不触发引用页刷新）；其余 → `set_right_tab`（`true`）。
  **整栏收起只剩 `SetPanelsVisible`**（竖条顶部总开关 / 状态栏三态 / 视图菜单）。
  `right_tab_click_collapses` 已删除——不要再按旧语义写「点当前页 = 收起」的用例或提示。
- 竖条提示 `edge_tab_tooltip(tab, is_open, is_current)` 三态：当前页 / 可切到 / 已关闭
  可重新打开；勾选态 `toggled = panels_visible && active == tab`（整栏收起时不再谎报勾选）。
- 竖条页序改为取 `events::right_tabs()`（原先在 `sidebars.rs` 硬编码了一份六页数组）。
- 仍缺（登记，非缺陷）：DBeaver 竖条对已勾选页是 `closePanel`（逐页关），本仓把逐页关
  收敛到页签条的 ×（第 32 批起在每页签内侧，见 §3.05），竖条点已勾选页为无操作。
- 门禁基线（本批之后）：panel **345**、editor **794**（1 ignored）、ui-guard **4**。

### 2.97 右栏页签模型已落地第 1 步（2026-11-19）

- **已实现（方案 B 第 1 步）**：`panel::data::open_tabs: Vec<RightTab>`（规范序、恒非空，
  出厂 = `right_tabs().to_vec()`）；`ports.rs` 新增 `CloseRightTab(RightTab)` /
  `OpenRightTab(RightTab)`；`events.rs` 有 `open_tabs()` / `open_right_tab()` /
  `close_right_tab()` + 私有 `open_right_tab_only()`；`set_right_tab` 与 `toggle_right_tab`
  都会先确保目标页在集合里；纯函数换成集合化的 `strip_index` / `tab_at_strip_index` +
  `neighbor_after_close`。**`right_tab_index` / `right_tab_from_index` 已删除**。
- **界面落点**：`sidebars.rs` 页签条按 `open_tabs` 渲染、行尾 × 无条件渲染并发
  `CloseRightTab(self.right_tab)`（提示「关闭此页（其余页仍保留）」）；
  `tabs.rs::panel_view_menu` 的六个页项 = 「勾选态看集合成员」+ 未勾走 `OpenRightTab`；
  `host/results/dispatch.rs` 两个新分支（`OpenRightTab` 切到引用页时同样触发
  `begin_references_load`）。
- **关闭规则**：关当前页 → 落后继（无后继取最后一个剩页）；关空 → `set_panels_visible(false)`
  整栏收起，再开任一一页重新展开。
- **仍缺（下一步）**：① ~~右缘竖条的 `ToggleRightTab` 还是「点当前页收起整栏」的旧语义~~
  （已由 §3.02 统一为「开/关该页」）；② ~~关闭按钮是行尾单颗，不是每页签 `SWT.CLOSE`~~
  （已由 §3.05 收到页签内侧，`stop_propagation()` 已验证不双触发）；
  ③ `open_tabs` 不持久化（已由第 26 批的 `result_panel.open_tabs` 补齐）。
- 门禁基线（本批之后）：panel **345**、editor **794**（1 ignored）、ui-guard **4**。

### 2.96 右栏页签模型：方案已定，代码未动（2026-11-19）

> **状态更新**：本节已被 §2.97 取代（方案 B 第 1 步已落地，含 `right_tab_index` /
> `right_tab_from_index` 的删除）。本节保留为「当时的方案口径与竞品行号」，改代码前读 §2.97。

- **先读方案再改**：`docs/architecture/editor/result-panel-right-panel-tabs.md`（方案 B：
  `open_tabs` 集合 + 每页 × 关闭 + 去掉「点当前页签收起整栏」）。代码一行未改，
  别把它当成「已经实现」。
- 竞品事实（都在 `ResultSetViewer.java`）：`addPanel:1330-1394` 的
  `CTabItem(panelFolder, SWT.CLOSE)`（每页带关闭）、`:1388` 的
  `presentationSettings.enabledPanelIds.add(id)`（已启用集合会持久化）、
  `fillPanelsMenu:1559-1588`（视图菜单逐页是 `STYLE_CHECK` 的 `CMD_TOGGLE_PANEL`）。
  ⇒ 「关掉一页」= 从集合移除，「收起整栏」是另一条命令（`CMD_TOGGLE_PANELS`），
  两者在 DBeaver 里**从不混用**；本仓当前把两者合成「点当前页签」是自造语义。
- 改的时候必须同步的六处：`events::right_tabs()` 顺序、`right_tab_index` /
  `right_tab_from_index`（改成「规范序 × 当前集合」两段换算）、`right_tab_icon`、
  `right_tab_description`、`right_tab_label`、`panel_view_menu_items()` + `tabs.rs`
  / `sidebars.rs` 的 `match self.right_tab`。
- 不做（已登记）：六页同时挂载（会把统计/分组/引用的每帧重算放大六倍）、
  `open_tabs` 持久化（要新增 settings 键，与 §9.05 那类偏好同批做）。

### 2.95 分组页正文的表结构（键列 + COUNT(*)）与三条差异（2026-11-19）

- DBeaver 的分组结果是**表**：属性列 = 每个分组键一列，函数列 = `getFunctionColumns()`
  逐个建（`GroupingResultsContainer` 的 `addAttribute` / `addFunction`），默认函数由
  `getDefaultFunction:75-80` 拼成 `COUNT(<方言默认分组属性>)`，无数据源时 `COUNT(*)`。
- 本仓：`group_table_header(names)` = 键列名 + `COUNT(*)`；正文表头
  `editor-result-group-header`（`h_5` + `HAIRLINE` 下边线，与计算页列头同形），
  行内每键一格 `flex_1` + 省略号，计数格 `text_right` + `muted`。
- 差异（改了会返工，先看这里）：列宽是等分 flex（那边用结果集列宽体系）；
  只有一个函数列；`COUNT(*)` 是本仓常量（本地分组不发 SQL，无别名可反推）。
- 门禁：panel 344 / editor 794（1 ignored）/ ui-guard 4。

### 2.94 值页空态两行与「文案取不到」（2026-11-19）

- DBeaver 值页空态是**两行**（`ValueViewerPanel.createContents:89-106` 的 `PaintListener`，
  y=0 与 y=30 各一条 `UIUtils.drawMessageOverControl`），第二行把 `CMD_TOGGLE_PANELS`
  的**命令描述**绑进文案。抄这个形态时别做成两个 Label——它是居中绘制的两段文字。
- 正文**取不到**：`OSGI-INF/l10n/bundle.properties` 只有 12 行含 `viewer`、无
  `value_viewer_*`，`bundle_zh.properties` 同；`ResultSetMessages.java:432-433` 只有字段声明。
  任何人再说「按 DBeaver 原文逐字对齐」，先给出这两个 key 的正文出处。
- 本仓实现：`tabs.rs` 的 `sidebar_empty` 委托 `sidebar_empty_lines(&[text])`（其余页仍单行）；
  `empty_page_lines(...)` 多行版骨架 + `debug_selector("editor-result-empty-page")`；
  值页空态走纯函数 `value_empty_lines()`。第二行**不写快捷键**（本仓面板总开关无按键绑定），
  改为指向右缘竖条 / 视图菜单两个真实入口。
- 门禁：panel 343（告警清零）/ editor 794（1 ignored）/ ui-guard 4。

### 2.93 分组页偏好的持久化与「DBeaver 不存分组键」（2026-11-19）

- **别把「分组键重开就丢」当缺口**：DBeaver `GroupingPanel.saveSettings:118-120` 只有一行
  取 `"groups"` section，全仓无调用点；`loadSettings:114-116` 同样只取 section 不读。
  分组键在那边就是会话态。要改这个结论，先给出该文件里的读写调用点。
- 真持久化的是两个偏好键（`ResultSetPreferences:41-42`）：
  `resultset.grouping.defaultSorting`（`""`/`ASC`/`DESC`，`ChangeSortingAction` 写后
  `persistConfiguration()`）与 `resultset.grouping.showDuplicatesOnly`（`DuplicatesOnlyAction`
  的 check box）。**两键在 plugin.xml 里都没有 initializer**，故默认 = 空串（不排序）/
  false（显示全部组）。
- 本仓对应键：`result_panel.group_sort`（`none`|`value`|`countdesc`）与
  `result_panel.group_duplicates_only`（bool）。写入口 = 分组页的两个钮，落盘走
  `dispatch.rs::persist_result_group_sort` / `persist_result_group_duplicates_only`；
  读入口 = `construct.rs` 注入 `panel.set_group_prefs_initial(...)`。
- 加同类偏好照这个五步：`ports::GroupSort::id/parse` → settings 模型字段 + 默认 →
  `Slot` + key 映射 + `slot_kind` → `service::mod.rs` 读值 arm + 写分发 or-pattern →
  `service/result_panel.rs` 的 `apply_slot` 分支 + `set_*` 快捷写 → 面板 `set_*_initial`
  + 只读访问器 → 宿主注入与回写 + 两条用例。
- 已知差异（别改）：本仓 `none`/`value`/`countdesc` 三档，`countdesc` 对应那边的 `DESC`、
  `none` 对应 `""`（本批 §3.04 已补），`value` 是超集档（那边的 `ASC` 排的是 count）。
  作用域是全局一份（那边按连接）。
- 门禁：settings 41 / panel 353 / editor 798（1 ignored）/ ui-guard 4（§3.04 后基线）。

### 2.92 计算页统计项扩到九项（2026-11-19）

- 取证位置：`dbeaver-ref` 的 `org.jkiss.dbeaver.ui.editors.data/plugin.xml:89-99` 列九个
  `aggregateFunction`（`count` / `countDistinct` 标了 `default="true"`，另有 `countNulls`、
  `sum`、`avg`、`min`、`max`、`median`、`mode`）；行为在
  `panel/aggregate/AggregateColumnsPanel.java`（`loadSettings:129-155` 空集回落
  `loadDefaultFunctions:157-164`、`saveSettings:166-181`、`refresh:199-214`）。
- **`AggregateFunctionDescriptor` / `AggregateColumnsConfig` / `FunctionXxx` 这几个类在本仓
  dbeaver-ref 快照里不存在**（`panel/aggregate/` 只有 `AggregateColumnsPanel.java`），所以
  median / mode 的具体算法无法逐行对照。本仓自定确定性口径，改这两项时先看
  `grid/stats.rs` 的 `median_of` / `mode_of` 文档注释与 `grid/tests.rs` 的三条纯函数用例。
- 九项的稳定 id 与顺序（`ports::StatsItem::ALL`，id 全小写）：
  `count,countdistinct,nulls,min,max,median,mode,avg,sum`。落盘串由
  `stats_items_to_ids` 生成，`stats_items_from_ids` 解析（未知项丢弃、重复项丢弃）。
- 默认集 = `DEFAULT_SET = [Count, CountDistinct]`（对齐 `isDefault()` 两个打标），
  settings 里 `result_panel.stats_items` 的模型默认与 registry 的 `default_json` 是
  `"count,countdistinct"`——**两处必须同时改**，registry 不变式校验二者相等。
- 整列取值的门槛：`StatsItem::needs_column_values()`（仅 `CountDistinct` / `Median` /
  `Mode`）+ `StatsMode::Text`。`render_stats_tab` 把「要扫整列」压成一次 `need_values`
  判断，算好的派生量通过 `tabs::StatExtras` 传给纯函数 `stat_value(item, stats, &extras, mode)`。
  再加聚合函数时按这个位置扩：`ALL` / `label` / `supports_text_mode` /
  `needs_column_values` / `id` 五处同改，然后 `StatExtras` 加字段、`stat_value` 加分支。
- 已登记口径（**有意偏离**，不是缺陷）：`distinct_count` 按**值文本**判等，所以 `1` 与
  `1.0` 算两个值；`median_of` 只收可解析为 `f64` 的样本（与 `column_stats_from_values`
  同一份解析）；`mode_of` 并列时取**首现更早**的值（DBeaver 实现不可考）。
- **「组内聚合函数列」不适用**：DBeaver 的 Tree 嵌套（每列父节点挂函数子项）只在
  `aggregateSelection:221-262` 的 `groupByColumns == true` 分支出现，需要多单元格选区；
  本仓只有单格选择（`selection_pos: Option<(usize, usize)>`），见第 14 批登记。
- 门禁：`cargo test -p rds-settings --lib`（registry 不变式会校验 `default_json`）→
  `cargo test -p rds-result-panel --lib` → `cargo check -p rds-editor` →
  `cargo test -p rds-editor --lib -- --test-threads=4` → `cargo test -p rds-ui-guard`。

### 2.91 「筛选运行」链路与状态栏说明的接手要点（2026-11-19）

- 链路：面板 `ResultRequest::RunFilterAsQuery` → `dispatch` → 宿主
  `view/host/results/filter.rs::run_filter_as_query` → `engine::sql::rewrite_with_where`
  → `panel.set_exec_sql(...)` → `execute(Statement, Replace)`。端到端用例
  `view/tests/results_b.rs::run_filter_as_query_rewraps_and_replaces_the_result`
  （五条断言：空词不递交 / 含 `rds_filtered` / 条件原样进 WHERE / 无 `limit` /
  提示含「已去掉原查询的」且仍是一份结果）。改链路先看它。
- **状态栏说明不要在递交前 `set_message`**：`execute` 起步清一次消息，回执时
  `sync_result_view` 之后泵又重写一次，递交前摆的说明两处都会被擦掉。
  「已去掉原查询的 LIMIT」走 `EditorHostPanel::pending_result_notice`，由泵在
  同步结果区之后补上。新增重查入口（如自定义 ORDER BY 的别的形态）照这个走。
- 优先级：`segment_failure`（取段失败）> 执行器 `fresh_notice` > `pending_result_notice`；
  只要 `self.message.is_some()` 就不覆盖（错误比说明重要）。`execute` 同步被拒时
  要把 `pending_result_notice` 清空，否则它会搭下一次执行的便车。
- 测试口径：`cargo test -p rds-editor --lib` 用**默认线程数**跑全量时，
  `ports::editor_completion` 三条「后台线程 3s 内」用例会偶发超时（满负载起不线程），
  加 `-- --test-threads=4` 全绿，单跑也全绿——别当回归修。

### 2.90 分析结果与「计算行数」的边界（2026-11-19）

- 判据只有一处：`panel::events::count_rows_available(has_data, is_analysis)`。要加别的
  置灰条件（例如「无绑定连接」）也加在这里，别在 `render` 里另写一串 `&&`。
- **面板置灰 ≠ 拦住**：队列里的在途 `CountRows` 照样会到宿主，所以
  `count_result_rows` 里必须再判一次 `entry.analysis`（两处都改才算改完）。
- 文案走 `count_rows_tooltip(is_analysis, computed)`：分析态讲**原因 + 行数在哪**
  （「总行数就是这里的行数」），其余两态讲「算什么 / 重算」。不要在 render 里写字符串。

### 2.89 「计算行数」的接手要点（2026-11-19）

- 链路三段：`ResultControls.count_rows`（槽位）→ `ResultRequest::CountRows` →
  宿主 `view/host/results/count.rs::count_result_rows`。改文案 / 图标改
  `panel/render.rs` 里那颗 `Button`；改取数口径改 `count_result_rows`。
- **拼 SQL 只在一处**：`count::rows_count_sql(sql, pushdown)`。下发筛选时必须把
  `WHERE` 带进内层（否则报的是全表行数），排序下发不拼 `ORDER BY`。要支持别的形态
  （例如 `Segment` 取段后的部分行）也在这里加分支，别在宿主里拼字符串。
- **不开新结果集**：`ExecTarget` 没有 Count 变体、`ResultPlacement::{Replace, NewSet}`
  都会污染标签，所以走 `SqlService::execute` 直调（`record_history: false` /
  `use_cache: false` / `use_transaction: false`），回主线程只
  `panel.set_row_count_notice(text)`。不要在 `dispatch.rs` 里改成 `execute(...)`。
- `count_from_rows` 认 `shared::Value::{Int, Float, Text}`（`Null` / `Bool` / `Bytes` 一律
  `None`）——驱动给什么形态都不该 panic，认不出就报「计数查询没有返回数字」。
- **两个未决口径**：① `ResultEntry` 没 schema 字段，计数按连接默认 schema 跑；
  ② 本地分析结果（`analysis = true`）~~当前未拦~~**已在 §2.90 拦掉**（面板置灰 +
  宿主兜底，缺一条都会漏）。

### 2.88 状态栏 / 左缘竖条的接手要点（2026-11-19）

- 状态栏十段对照见 `result-panel.md` §9.00。三处**已登记的差异**（别当遗漏）：
  「计算行数」动作~~本仓没有（行数框是**行号跳转**）~~**已由 §2.89 补齐**（行数框仍是
  **行号跳转**，不是算总行数——两者现在是并排的两颗控件）；选区统计标签**有意不摆**
  （用户 2026-10-28 口径只留内存 / 耗时 / 条数）；自动刷新无机制。
- 段大小框的行为契约集中在三个纯函数：`panel::events::parse_segment_rows`
  （只认 `>0`）、`segment_rows_blur_display`（失焦回填）、设置侧
  `result_panel.segment_rows`；改口径改这三处。
- 左缘竖条 = `panel/sidebars.rs::render_vertical_bar`：**展示组 RADIO 在上 → 弹簧 →
  记录模式 CHECK 沉底**（`ResultSetViewer:948-965`），宽度用 `EffectTransition` 补间，
  容器始终挂载、内容按 `show_left_sidebar` 挂载（headless 里 `debug_bounds` 收起态仍命中，
  别用它断言「未渲染」）。
- 竖条提示只有两份纯函数：`presentation_tooltip(presentation)`（网格 / 文本 =
  「名字（描述）」）与 `record_view_tooltip(active)`（记录模式随开关报「会看到什么 /
  怎么回去」）。要改文案改这两处，别在渲染里写字符串。

### 2.87 计算页的接手要点（2026-11-19）

- 工具条三颗：＋（缺项下拉）/ Reset / 口径 toggle，都在 `panel/tabs.rs::stats_toolbar`；
  口径只有 `StatsMode::{Numeric, Text}` 两档，取值逻辑在纯函数
  `panel::tabs::stat_value`（计数类不受口径影响、字符串档 Avg/Sum 不出值）。
- **右键菜单不要再写死**：条目由 `stat_menu_entries(&stats_items, stats_mode)` 派生
  （顺序 = 复制值 → 复制全部 → SEP → 缺项逐个「添加」→ SEP → 恢复默认 → SEP → 口径切换），
  文案由 `stat_menu_label(entry)` 给（口径项报「切到哪一档」）。要加动作就进那两个函数，
  渲染侧只按条目挂事件——`PopupMenu` 内部不可读，测试只能钉纯函数。
- 持久化口径：`result_panel.stats_items` 存 id 列表（顺序即显示顺序），
  `stats_items_from_ids` 丢掉未知 / 重复项；**空串 = 全停用**，是合法状态，不要在
  载入时静默回落到默认集（DBeaver `loadSettings:152-154` 会回落，本仓有意不照抄）。
- 两处**已登记的形态差异**：移除是行尾 ×（不是「选中行 + 工具条 Remove」，
  本仓统计页没有树行选中模型）；口径按钮用文字标签（DBeaver 是勾选 + 类型图标）。
- 两处**能力缺口**：组内聚合函数列、`countDistinct`（默认集因此只剩「非空」）。

### 2.86 值详情页的接手要点（2026-11-18）

- 工具条四颗 = `[位置标记, 复制, 保存, 外部打开]`（顺序照 `ValueViewerPanel.fillToolBar`
  的「内容类动作在前、写回类在后」）。两颗落盘动作的**可用性同源**：
  `panel::tabs::value_can_save_to_file(col, raw)`——NULL / 缺列 / 行号槽都给不出口。
- 「保存至文件」走 `ResultRequest::SaveValue` → 宿主 `save_selected_value`（**要路径**，
  取消不算错误）；「在外部程序中打开」走 `ResultRequest::OpenValueExternally` →
  `open_selected_value_externally`（**不要路径**，落 `%TEMP%/rds-value-preview/` 再唤起）。
  两者共用面板算好的字节与建议名 `selected_value_payload`。
- 文件名安全化只有一份：`export::external_file_name`（`/` `\`、Windows 非法字符、控制字符
  → `_`，去前导点，只剩 `_` 回退 `value.bin`）。想改落盘目录只改
  `open_selected_value_externally` 里的 `temp_dir().join(...)`。
- 唤起平台分支在 `export::open_with_default_app`（Windows `cmd /C start ""` +
  `CREATE_NO_WINDOW`；macOS `open`；Linux `xdg-open`）。**没有**用例覆盖真实唤起（headless
  不能开外部程序），返回值只用来写 status message。
- 值页仍是**只读预览**：`SaveValueAction`（写回结果集 + 「编辑后自动应用」勾选
  `RS_EDIT_AUTO_UPDATE_VALUE`）不做；空态文案 DBeaver 是两行、本仓一行（属目视阶段）。

### 2.85 列浏览器页的接手要点（2026-11-18）

- 投影灌入点是 `ColumnBrowserDelegate::set_data(columns, type_names, hidden, comments)`，
  **四份全等即直接返回**（对齐 `MetaDataPanel.refresh` 的 "No changes"）。它有副作用
  （清 `last_selected` 与多选集合），所以别为了「刷新一下」反复重灌同一份投影——
  会把用户多选和滚动位置打掉。真换结果集时才重灌（列数/列名变化自然不相等）。
- 页内工具条固定两段语义：左侧只有「生成 DDL」（DBeaver `contributeActions` 唯一贡献），
  右侧是共享视图菜单；多选批量动作（隐藏所选 / 显示所选 / 清空选择）是本仓扩展，
  挂在左侧动作段里，选中为空时不生成元素（空 `Toolbar` 仍吃 gap，会把标题挤窄）。
- 过滤口径**有意宽于** DBeaver（列名 / 类型名 / 注释 vs 只列名），不是漏做；
  要收窄就改 `filter_browser_columns` 一处（用例 `browser_filter_*` 会跟着红）。
- 两项**未做**的差异，别当遗漏：嵌套子列树（本仓列模型没有复合类型子属性）、
  列布局按驱动记忆（`MetaData/<driverId>`）。

### 2.84 竖条 / 页签提示文案的接手要点（2026-11-18）

- 描述只有一份：`panel::events::right_tab_description(tab)`（页签条 tooltip 与竖条 tooltip 共用）。
  改描述改这里，两处一起变。
- 竖条提示 = `edge_tab_tooltip(tab, collapses)`：正常态 `页名（描述）`，
  正在显示的页 → 「收起结果面板（本页正在显示）」。新增页记得在 `right_tabs()` 里排位，
  提示 / 图标 / 菜单顺序三处会自动跟上（都从 `right_tabs()` 派生）。

### 2.83 右缘竖条 / 菜单面板项的开关键语义（2026-11-18）

- 三条入口，语义不同，别混：
  - `ResultRequest::SetRightTab`（**页签条**点选）＝ 只切页，永不收起；
  - `ResultRequest::ToggleRightTab`（**右缘竖条**图标 / **视图菜单**面板项）＝
    「该页已显示就收起整块面板，否则切页并展开」，对齐 `CMD_TOGGLE_PANEL`；
  - `ResultRequest::SetPanelsVisible`（**竖条顶部总开关** / 状态栏三态开关 / 菜单总开关）＝ 整组收放。
- 判定只有一份：`panel::events::right_tab_click_collapses(tab, active, panels_visible)`；
  改口径改这一处。`toggle_right_tab` 的返回值 = 「是否真的切页」，宿主靠它决定
  引用页要不要取外键（收起分支**不要**再触发一次元数据往返）。
- 竖条提示随状态变：本页显示中 → 「收起结果面板（本页正在显示）」，否则是页名。

### 2.82 面板最大化 / 总开关的接手要点（2026-11-18）

- **硬不变量：`panel_maximized ⇒ show_value_sidebar`**。守它的只有三处入口：
  `set_panels_visible(false)` 复位最大化、`toggle_value_sidebar` 走到收起态复位最大化、
  `toggle_panel_maximize` 进入最大化前先展开面板。新增别的「收起面板」路径时，
  必须同样复位，否则 `render_right_sidebar` 会按 `show_value_sidebar` 判可见，
  网格已不渲染、右栏也不渲染 = 整块区域空白。
- 别再给 `render_right_sidebar` 加 `|| maximized` 兜底：那会把「状态矛盾」掩盖成
  「看起来还行」，而真正的错（竖条已消失、无从再展开）还在。
- DBeaver 依据：`togglePanelsMaximize`（`ResultSetViewer.java:1551-1557`）只 hide/show
  presentation；`showPanels(false)`（`:1487`）就是 `setMaximizedControl(children[0])`，
  两者共用同一份 sash 可见性。最大化**不落盘**（不走 `savePresentationSettings`），
  本仓 `panel_maximized` 同样不在 settings 里。
- **测试口径坑**：右栏外壳容器始终挂载（收起时宽度补间到 0），
  `debug_bounds("editor-result-value-sidebar")` 在收起态**仍为 `Some`**，
  不能用它断言「未渲染」；面板可见性用 `panels_visible()` / `show_value_sidebar`，
  渲染断言留给网格（`editor-result-main-grid` 才是真按条件挂载的）。

### 2.81 分组页的接手要点（2026-11-18）

- **工具条顺序别打乱**：排序 → 仅重复 → 分隔 → 列（勾选下拉）→ 移除 → 分隔 → 清除，
  照 `GroupingPanel.fillToolBar:164-176`。三簇的语义是「结果视角开关 / 分组键编辑 / 整体清除」，
  把「列」挪到最前会把语义打散（本仓原布局就是这样，已改）。
- **分组键刻意不持久化**：DBeaver 的 `loadSettings/saveSettings`（`:114-120`）是空壳且
  `saveSettings` 无调用点；那边分组靠「生成 GROUP BY 查询交给数据源」
  （`GroupingResultsContainer.rebuildGrouping:211-247`）。本仓是客户端分组，
  因此更不该把键做成偏好项——真要"记住上次分组"，那是新增能力，得单独拍板。
- 本仓分组结果没有「组内聚合函数列」（DBeaver 有 `getTransformerBindFunctions`），
  要对齐得先有「生成 GROUP BY SQL → 执行 → 嵌套结果集」链路。

### 2.80 段大小输入框失焦回填的接手要点（2026-11-18）

- 回填判定是纯函数 `panel::events::segment_rows_blur_display(text, current)`：
  文本（去首尾空白）≠ 当前生效值就返回该生效值，否则 `None`（不改写）；
  改口径只改这一处，订阅里只负责 `set_value`。
- **`InputEvent::Blur` 是 vendor 0.9.0 就有的**（`gpui_base/.../input/base/state.rs:137-147`，
  `Change` / `PressEnter` / `Focus` / `Blur` / `GutterMarkerClick`）——
  以后要"失焦提交"这类交互别再写"没有失焦钩子"。
- 但 headless 测试**派发不了它**（`window.focus()` 只改窗口焦点跟踪，不派发元素级事件；
  同款注记见 `crates/editor/src/view/tests/analysis.rs:476-483`），
  所以这类逻辑一律抽纯函数钉测，真窗口行为靠目视。

### 2.79 计算页默认集 / Reset 的接手要点（2026-11-18）

- 默认集只有**一个**定义：`ports::StatsItem::DEFAULT_SET`（现 = `[Count]`/「非空」）。
  出厂初始化（`panel::lifecycle`）、Reset（`panel::events::reset_stats_items`）、
  设置模型默认值（`default_result_stats_items()`）与登记项 `default_json` 都指向它；
  **改默认集就改这一处 + 设置侧那条字符串**，别只改其中一边（不变式 3 会红）。
- 为什么只有「非空」：DBeaver 用 `FunctionDescriptor.isDefault()` 标记默认集，
  九个函数里只 `count`/`countDistinct` 标了 `default="true"`；本仓没有「去重计数」这一项，
  所以默认集只剩 `count`。要补 `countDistinct` 得先有 distinct 计数能力，再把它加进
  `DEFAULT_SET`（`is_default()` 会自动跟着变）。
- Reset = `DEFAULT_SET`，**不是**六项全开；`stats_tab_renders_*` 用例里的「六行」是测试
  自己 `p.stats_items = StatsItem::ALL.to_vec()` 摆出来的，别误以为是出厂状态。

### 2.78 值页动作可用性的接手要点（2026-11-18）

- 「保存值到文件」的判据只有一份：`panel::tabs::value_can_save_to_file(col, raw)`。
  UI（置灰 + 提示文案）与 `selected_value_payload`（返回 `None`）都读它——
  以后要加强制条件（比如只对某个类型给保存），改这一处、两处行为同时变，
  别在渲染分支里再写一遍判断。
- `raw` 必须传 `delegate.row_cell_text(row, col)`（**原始格值**），不要传 `cell_text`
  （它是展示文本，NULL 占位会被类型样式偏好改成 `∅` / `—`，`is_null_cell` 就认不出来了）。
- 行号槽（`col == 0`）按「不是数据格」处理：上面显示的数字是视图行号，不是单元格内容。

### 2.77 计算页启用统计项持久化的接手要点（2026-11-18）

- 落盘形态 = `result_panel.stats_items`（逗号分隔的稳定 id：`count,nulls,min,max,avg,sum`，
  顺序即行序）。**英文 id 是契约**，中文 `StatsItem::label()` 只做展示——改文案不动 id。
- 解析口径在 `ports::stats_items_from_ids`：顺序照抄、未知项与重复项丢弃；
  **空串 = 全停用**（合法状态，页内给「点 ＋ 添加」提示），模型默认值 = `DEFAULT_SET`
  （现在只有 `count`，见 §2.79）。
- 写入口三条（计算页 ＋ / 行尾 × / Reset）都汇总到
  `EditorView::persist_result_stats_items`；**口径切换（StatsMode）不落盘**
  ——DBeaver 的 `aggregateAsStrings` 也不进面板设置。
- 新增统计项时同步：`StatsItem::ALL`、`id()` / `parse()`、`label()`、
  `supports_text_mode()`、`default_result_stats_items()` 的默认串、登记项 `presets`/`hint` 的
  id 清单——漏一处就会出现「页内能勾、重启后丢」。

### 2.76 右栏页签提示的接手要点（2026-11-18）

- 文案单一来源 = `panel::events::right_tab_description`（前五条译 DBeaver
  `bundle.properties:15-24` 的 `panel.*.description`）；新增页要在这里登记一条，
  用例 `right_tab_descriptions_are_distinct_and_non_empty` 会拦重复 / 空串。
- 页签条用 `.tooltip(...)`（`Tab::tooltip`）；右缘竖条的按钮仍用 `right_tab_label`
  ——DBeaver 竖条按钮提示就是面板 label，别把两处统一成同一个字符串。
- 新增页时一并核对：`right_tabs()` 顺序、`right_tab_index` / `right_tab_from_index`
  映射、`right_tab_icon`、`panel_view_menu_items()`（页数 + 分隔段位）、
  `render_right_sidebar` 的 `match self.right_tab`——五处漏一处就会出现「页签在但点不开」。

### 2.75 计算页右键菜单的接手要点（2026-11-18）

- 菜单内容写死在一处：`panel::tabs::stat_row` 的 `.context_menu(...)`；文本由
  `stats_copy_all_text(entries)`（纯函数）与行自己的值在**渲染期**算好再进闭包。
  改复制口径先改纯函数 + 它的用例，不要改菜单回调。
- 菜单文案对齐 DBeaver `CopyAllAction` 的非分组口径（`函数=值`、`\n`）；分组口径在
  本仓不适用（没有分组聚合，`GroupByColumnsAction` 见 §8.40.4 第 2 条）。
- 行容器必须带**业务键 id**（`("editor-result-stat-menu", item)`）：`context_menu` 的状态
  按元素 id 记账，六行共用一个 id 会串菜单。同理列浏览器行用的是
  `("editor-result-col-browser-menu", data_col)`。
- 空文本不写剪贴板（DBeaver 靠 SWT 早退；本仓显式 `if text.is_empty() { return; }`），
  新增复制入口时照抄这个判空。
- 门禁基线：result_panel **320 passed** / editor check Finished / guard 4 passed。

### 2.74 段大小持久化的接手要点（2026-11-18）

- 值链路一条线：状态行段框（`InputState`）→ `parse_segment_rows`（`>0` 才算有效）→
  `ResultRequest::SetSegmentRows` → 面板 `set_segment_rows`（夹 `MIN=1`）→
  宿主读回 `panel.segment_rows()` → `SettingsService::set_result_panel_segment_rows`
  （夹 `1..=100_000`、同值不写盘、persist）。**写盘写的是夹取后的生效值**，
  别把用户原始输入直传持久层。
- 构造期注入走 `panel.set_segment_rows_initial(rows, window, cx)`（需要 `&mut Window`），
  它同时把值写进输入框：只设状态不写框，用户会看到空框 + 占位 `1000`，误以为没生效。
  `set_value` 内部 `emit_events = false`，不会把注入当成用户输入再入队一次。
- `construct.rs` 里 `segment_rows` 必须与 `sidebar_width` 一样**在 `&mut cx` 之前拷贝**
  （E0502：`try_global` 的借用会跨到后面的 `panel.set_*(…, cx)`）。
- settings 侧三处必登记：`ResultPanel.segment_rows`（struct + `Default` + 末尾 default fn）、
  `Slot` 变体 + key 映射、**`registry::slot_kind` 的 `KindTag::Number`**
  （漏这一处 → `E0004 non-exhaustive`；契约测试还要求它与登记项 `kind.tag()` 一致）。
- 登记项顺序：`result_panel.segment_rows` 属 `group: "data"`，**必须插在 `bool_display`
  之后**（插到 `sidebar_width`（`layout`）后面会打断 `data` 分组连续，
  `groups_are_contiguous_and_consistently_named` 直接失败）。
- 作用域偏离：DBeaver 存连接级 `RESULT_SET_MAX_ROWS`，本仓存**全局一份**（与
  `result_panel.sidebar_width` 同口径）。要按连接分档得把键迁进连接配置域，属另一批。
- 门禁基线：settings **37 passed** / result_panel **319 passed** / editor check Finished /
  guard 4 passed。

### 2.72 面板最大化的接手要点（2026-11-18）

- 语义是「收起结果网格、面板独占整块区域」，**不是**「把右栏拖宽」：最大化是一个
  `panel_maximized: bool` 纯视图态，`render` 里网格那块**条件不渲染**
  （`editor-result-main-grid` 会从渲染观测里消失，测试就靠这个 id 断言）。
- 与开合态正交：`show_value_sidebar` 保持原值，右栏渲染用
  `show = show_value_sidebar || panel_maximized` 强制挂载；退出最大化后回到用户原本的开合。
  别把「最大化」写成「顺便把面板打开」——那会改掉用户的开合偏好。
- 最大化期间**不挂右栏拖拽手柄**、不走 `EffectTransition` 宽度补间（没有可拖的分栏，
  补间也没有意义）；竖排时满高、横排时满宽。
- 视图菜单段位照 `fillPanelsMenu:1559-1588`：六页 → 分隔 → 最大化 → 分隔 → 面板开关 →
  布局方向（`CMD_ACTIVATE_PANELS` 仍无端口，不摆）。`panel_view_item_label` 现在收
  `(item, panels_visible, vertical_layout, panel_maximized)` 四参——新增动态条目时按同一
  形态加参数，不要另开一套函数。
- 若要加快捷键：DBeaver 该命令带默认键位，本仓 `KeyBinding` 表还没登记，属未落地项。

### 2.71 分组页「移除分组列」的接手要点（2026-11-18）

- 可用性判定集中在纯函数 `panel::tabs::can_remove_group_column(group_columns, selected)`：
  **选中列在显式分组键里 + 移除后仍 ≥1 列**才可用（对齐 DBeaver
  `GroupingColumnsContainer.canColumnBeRemoved` `:92-94` 的「不许删到零」）。
  删到零走「清空分组」（`ClearGroupColumns`），两条路径别合并。
- 移除动作**复用** `ResultRequest::ToggleGroupColumn`（与下拉取消勾选同一条路径）。
  要改移除语义（比如只允许从末尾删）就改这个端口，别在按钮回调里各写一份。
- 分组页动作顺序对齐 `GroupingPanel.fillToolBar:164-176` 的段位：
  列下拉（≈`EditColumns` 轻量形态）→ 排序口径 → 仅重复 → 移除分组列 → 清空分组。
  排序口径与 DBeaver 的 `Unsorted/Asc/Desc` **不是同一件事**（本仓是本地聚合、
  原序 / 按值 / 按计数三档）；三档里的「原序」不是摆设——它保留首现序
  （§3.04），加第四档之前先问「它对应哪条排序键」。
- 已知偏离：本仓下拉可以把分组键全取消（落回「选中列兜底」），DBeaver 的逐列删除
  禁止删到零；终态不同但都不会出现「零分组键却有结果」。
- 门禁基线：result_panel **314 passed**；`cargo check -p rds-editor` Finished。
  **整包 check 的注意点**：并行线在 `crates/connection_dialog/src/components/connection_dialog/`
  有未提交改动时（曾出现 `render.rs` 里 `result` 字段重复），`cargo check -p rds-workbench`
  会因依赖它而失败——先看清报错归属，别当成自己改坏，也别替并行线修。

### 2.70 列浏览器「复制列名」与嵌套列 N/A 的接手要点（2026-11-18）

- 「复制列名」= `ContextAction::CopyColumnNames`（**无载荷**）+ 清单侧纯函数
  `column_browser::column_names_to_copy(columns, selected, clicked)`。选哪些列属于
  清单的多选集合（`ColumnBrowserDelegate::selected_set`），菜单纯函数不该知道它——
  别把列名塞进 `ContextAction`（那会让菜单项依赖界面状态，且 `ContextAction` 被
  `grid/tests.rs` 按 eq 断言）。
- 拷贝口径：右键列**在集合里**→ 按数据列序整集合逐行拼（`\n`）；否则只拷它自己。
  越界下标静默跳过；结果空串**不写剪贴板**（对齐 DBeaver `MetaDataPanel` 的
  `text.isEmpty()` 早退）。集合里的下标是**数据列下标**，不是显示位次。
- 入口只有列浏览器清单。网格右键 / 列头菜单的 match 里它是忽略分支——网格右键没有
  多选集合概念，硬塞会变成「拷哪几列说不清」。
- **嵌套列不做**：DBeaver 清单是树（`getNestedBindings()` + `expandToLevel(2)`），
  本仓 `ColumnAnnotation`（`workbench_shell/src/model.rs:327`）与驱动 `ColumnDetail`
  （`shared/src/meta.rs:46`）都**没有 children 字段**。要做树得先改元数据模型（让驱动报
  `STRUCT` 子字段），面板层补不出来——不要用「按名字前缀猜层级」之类的假实现。
- 清单行**第二行是列注释**（DBeaver 的额外列 `Description`，`addExtraColumns` `:292-302`）；
  值来自 `ColumnBrowserDelegate::comments`（网格 delegate 的 `column_annotations()`），
  缺省（`None` / 空串 / 纯空白）不占行。别把它挪回第三列——窄侧栏（默认 320px）摆不下
  三列；要加宽显示就改 `RESULT_SIDEBAR_RIGHT_WIDTH` 的默认值，不是改行结构。
  第二行是单行截断，长注释看不全（记录在 §8.82 已知缺口）。
- 门禁基线：result_panel **312 passed**（本批 +2 例：复制列名纯函数 + 注释行渲染/反证）。

### 2.69 「查看引用行」的接手要点（B′ 已接，多行与跨 schema 未接，2026-11-18）

- 查询由**面板**算（`ResultPanel::reference_rows_sql` → `references::reference_rows_sql`），
  宿主只管执行：`ResultRequest::OpenReferenceRows { sql }` →
  `execute(ExecTarget::Statement(sql), ResultPlacement::NewSet)`。要在面板里加新的取值来源，
  改的是面板那一步；宿主那一步不需要理解外键。
- 字面量规则集中在 `references::sql_literal`：**数值列裸拼、其它加引号、NULL 走 `IS NULL`**。
  改这条规则会让「引用的值查不出来 / 查出错」直接可见，务必同步它的用例。
- 「参与列被隐藏」= 拿不到值（DBeaver 只按可见属性收集）。列号必须用
  `display_order()` 的显示位次 + 1，别用数据列下标——隐藏 / 拖列之后两者不一致。
- 落在**新标签**（`NewSet`）而不是替换：这是 `B′` 与 DBeaver「面板内嵌视图」的核心差异，
  别改成 `Replace`（那会把用户正在看的结果冲掉）。
- 未做：多行选中（DBeaver 会拼 `IN`）、跨 schema 目标表（`referenced_table` 只有驱动原话的表名）、
  按列类型反序列化值（现在是结果集文本直出）。
### 2.100 状态行选中统计三项的接手要点（2026-11-21）

- 链路方向：**偏好 → 构造期注入 → 事件写计数 → 状态行纯函数拼文案 → 渲染段**；开关改动反向走
  **右键菜单 → `ResultRequest::ToggleSelectionStat` → 面板 `toggle_selection_stat` → 宿主
  `persist_result_selection_stats` 落盘**。面板是唯一权威态，网格 delegate 只持镜像（右键菜单的
  「✓」读镜像）。
- 改这一片必须先看 `crates/result_panel/src/grid/status.rs`：`SelectionStatPrefs`、
  `selection_statistics`（纯函数，顺序 行→列→单元格、`", "`、`thousands`）、`render_status_row`
  的 `selection_stat: Option<String>` 实参（位置在 `segment_input` 之后、`elapsed_ms` 之前——
  **加参数时别插错位**，上一批就插进过 doc 注释里）。
- 计数口径：格选中 `1/1/1`、整行选中 `1/可见列数/可见列数`、清选 `0/0/0`。可见列数用
  `ResultGridDelegate::visible_column_count()`（**已存在于 `grid/view_rows.rs:36`**，别再在
  `delegate.rs` 里新造一个同名方法——本批踩过 `E0034` 二义性）。
- 给 `ResultStatus` 加字段会打破三处**全字段字面量**（`grid/tests.rs` 的 idle / partial / done，
  以及 `editor/src/view/host/results/grid.rs:158` 的 sync 投影）；新增字段时一起补，否则
  `E0063` 会从三处测试和一处宿主同时冒出来。
- `ContextTarget` 加字段同理：`grid/tests.rs` 的 `target()` helper 与五处字面量要补；带
  `..target.clone()` 的结构体更新语法**不能**再补字段（会把它插到 spread 后面直接语法错误）。
- 偏好只在**构造期**读（`construct.rs` 的 settings 借用块）：设置页改三项要下次构造面板才生效，
  与 `result_panel.hidden_status_commands` 同一管线；右键改则立即生效并落盘。这段差异写在
  `result-panel.md §9.32.3` 第 4 条，不要误当成 bug 去加跨面板广播。
- 门禁基线：result_panel **382 passed**（+4 例）、settings **45 passed**、editor check `Finished`
  （0 warning）、editor lib **800 passed（1 ignored）**、ui-guard **4 passed**。


### 2.101 右键 layout 段「呈现方式」组的接手要点（2026-11-21）

- 这一组是**面板状态**，网格不持有权威值：菜单只把「当前是什么」写进 `ContextTarget`
  （`presentation` / `record_mode`），点击后一律发 `ResultRequest`（`ToggleTranspose` /
  `CyclePresentation` / `SetPresentation(..)`），由面板改自己的 `presentation` 与 `transposed`。
- 改菜单文本前先看 `grid/menu.rs`：`layout_presentation_menu_items(target)`（纯函数，别在
  `context_menu` 里手写项）+ `context_menu_items` 里的 `extend` 顺序（**先呈现方式组、后三颗
  选中统计**，照 `ResultSetViewer.fillLayoutMenu:3549-3597`）。
- 勾选态读的是 delegate 镜像 `presentation_view()`：面板 `set_presentation` 是唯一同步点
  （`panel/events.rs`），竖条三颗按钮与右键走同一条队列，所以镜像不会漂。镜像与 `selection_stat_prefs`
  同口径，**不随 `set_data` 清**（新结果沿用当前呈现方式）。
- 文本态没有「记录」项（DBeaver 按 `supportsRecordMode()` 门控，`:3550`）；不要为了「菜单永不缺项」
  去补一个禁用项。
- 加 `ContextTarget` 字段的老坑复现：`grid/tests.rs` 的 `target()` helper + 四处全字段字面量要补，
  带 `..target.clone()` 的位置**不能**补；`column_browser.rs` / `grid/header.rs` 两处列级菜单落点
  也要补（它们不摆这一段，只为结构完整）。
- `Presentation::next()`（`ports.rs`）就是「切换表示」的循环语义，两档网格 ⇄ 文本；**记录（转置）
  不在这个循环里**（与 DBeaver 一致：记录模式属于 spreadsheet presentation 内部）。
- zoom（ALT+0 / ALT+9）已取证判为纯目视（只改字体高度 ±1pt、不持久），归入「目视统一处理」，
  别再当功能项捡回来。
- 门禁基线：result_panel **385 passed**（+3 例）、editor check `Finished`（0 warning）、
  editor lib **800 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.102 右键 layout 段「显示过滤器面板」的接手要点（2026-11-21）

- 这一项 = DBeaver `ToggleFilterPanelAction`（`ResultSetViewer.java:3589-3592` / `:5420-5442`）：
  勾选控制**整条筛选区**显隐，偏好落 `resultset.show.filterPanel`（出厂 true）。本仓偏好键
  `result_panel.show_filter_panel`（`Slot::ResultPanelShowFilterPanel`）。
- 「整条筛选区」在本仓就是 `panel/render.rs` 里那个 `filter` 容器（`filter: self.show_filter_panel
  .then_some(filter)`）；**不是** `filter_expanded`（那是「输入框爬到工具栏下方顶宽」的展开态）。
  改渲染前先把这两件事分清，别把展开条当整条。
- 关掉时同时把 `filter_expanded` 收为 false（`toggle_filter_panel(open == false)` 里做），避免回开时
  停在展开态；**不**连带关 `pushdown`（`:607`）/ `comments`（`:624`）两个独立开关。
- 网格侧镜像存的是**反向布尔** `filter_panel_hidden`（`grid/delegate.rs`）：`ResultGridDelegate` 走
  `derive(Default)`，而出厂语义是「显示」，反向存才能让默认 `false` 落在正确一侧。别把它改成
  `show_filter_panel: bool` 正向字段——那样默认会变成「隐藏」。
- 菜单只读镜像（`ContextTarget::show_filter_panel`）并发 `ResultRequest::ToggleFilterPanel { open }`；
  面板改自身态后由宿主 `persist_result_show_filter_panel` 落盘（读面板当前值，不读网格）。
- 两处列级菜单落点（`column_browser.rs` / `grid/header.rs`）要继续补这个字段但不摆该项；`grid/tests.rs`
  的 `target()` helper + 全字段字面量同样要补（老坑）。
- 有意差异（写进 `result-panel.md` §9.34.3）：设置页改偏好只在**下次构造面板**生效（无跨面板广播，
  与 §9.32.3 第 4 条同一条缺口）；菜单项恒摆（本仓无 `FEATURE_FILTERS` 能力位）；隐藏后无额外常驻入口
  （回开入口只在右键该项，与 DBeaver 同）。
- 门禁基线（第 51 批后）：result_panel **387 passed**、settings **45 passed**、editor check
  `Finished`（0 warning）、editor lib **800 passed（1 ignored）**、ui-guard **4 passed**。
- 工作区提醒：2026-11-21 打开时 `crates/notification/src/{core,service}.rs` 有**并行线**大改
  （通知中心历史/图标/合并窗口），**不随本线提交**；提交前一律按文件列表 add，不用 `git add -A`。

### 2.103 数据格右键 filters 段的接手要点（2026-11-21）

- 四项（保存 / 重置 / 删除所有过滤·排序 / 编辑筛选设置）的口径**只有一处定义**：
  `grid/menu.rs::filter_settings_menu_items()`（纯函数）。列头菜单与数据格右键都 `extend` 它；
  别在任何一处再手写一遍 push（文案 / 分隔线会漂）。
- 落点依据：`ResultSetViewer.fillContextMenu:3169-3177` 的 `filtersMenu` **没有落点门控**，
  DATA / COLUMN_HEADER / ROW_HEADER 都摆。所以「这四项只属于列头菜单」是**错判**，已在 §9.35.2
  推翻并把 `grid/menu.rs` 的注释改成取证结论；不要按旧注释把数据格这一组删掉。
- 位置：数据格右键里在**复制段之后、行级动作段之前**（对应 `fillContextMenu:3166` 的
  Separator + filtersMenu 位置）。排序段仍在行级动作之后——DBeaver 是 filters → order →
  navigate → edit，本仓 filters → edit → order；把 order 提到 edit 之前的「段落顺序归一」
  是独立候选（会动 §2.28-B3 那批的断言）。
- 点击路径：`grid/menu.rs` 的 `context_menu()` 四个分支发 `ResultRequest`（保存 / 重置 /
  `ClearFilterAndOrdering` / `OpenFilterSettings`），与 `grid/header.rs:476-514` 同一组请求；
  没有 `action_queue` 的独立网格不摆（沿用既有写法）。
- 暂不做的三项（§9.35.3 已登记）：`EDIT_FIND_AND_REPLACE` 中的一条**已在第 54 批落地**
  为菜单首项「在结果中查找…」（命令语义 `ResultRequest::OpenFind`，见 §2.105）；按操作符过滤组与
  `FilterResetAttributeAction`（本仓没有列级约束模型，筛选是一条 SQL 过滤串）；`CMD_FILTER_MENU_DISTINCT`
  同属该组。
- 门禁基线（第 52 批后）：result_panel **388 passed**、editor check `Finished`（0 warning）、
  editor lib **800 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.104 数据格右键段序归一的接手要点（2026-11-21）

- 段序口径照 `ResultSetViewer.fillContextMenu:3166-3219`：复制段 → filters → **order** →
  navigate（本仓无）→ **edit 行级动作** → [Separator `:3219`] → 列级视图段 → layout。
  `grid/menu.rs::context_menu_items` 的 push 顺序就是这个顺序，改菜单项位置前先对着这段源码。
- 三条分隔线的理由各不相同，别合并：
  1. filters 首项 `separator_before: true` —— `:3166` 的 Separator；
  2. 行级动作首项 `separator_before: true` —— `:3217` edit 子菜单前的 Separator；
  3. 冻结首项 `separator_before: true` —— `:3219` edit 之后的 Separator（后面接 view 子菜单）。
  **排序段首项是 `false`**：`fillContextMenu:3177-3179` 里 filters 与 order 两个子菜单相邻无 Separator；
  「取消排序」也与升降序同组（`fillOrderingsMenu:3939-3946` 内部无分隔线）。
- 不要为了「补齐段数」造假的 navigate / panels / hints / virtual 段：本仓没有对应命令集或模型，
  如实缺省（§9.36.3 已登记）。edit 段也只有 新增 / 复制此行 / 删除此行 三行
  （DBeaver 另有 `CMD_ROW_COPY_FROM_ABOVE` / `_BELOW`，`:3213-3214`，本批不扩）。
- 用例：`context_menu_sections_follow_fill_context_menu_order` 钉段序与三条分隔线；
  改顺序时同步更新 `context_menu_offers_filter_copy_and_freeze`（下标 + 分隔线断言）与
  `context_menu_row_actions_gate_on_editable_and_deleted`（行级动作现在在排序**之后**）。
- **环境坑（这次真踩到）**：D 盘写满时 `cargo check -p rds-editor` 会报
  `error: failed to write query cache to …\target\debug\incremental\rds_engine-…`
  并 exit 101——看起来像编译错误，其实是磁盘满。先 `Get-PSDrive D` 看 `Free`，
  `target\debug\incremental` 曾占 75 GB（纯缓存，可删）。清理前需用户确认，不要自行删除。
- 门禁基线（第 53 批后）：result_panel **389 passed**、editor check `Finished`（0 warning）、
  editor lib **800 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.105 filters 段首「在结果中查找…」的接手要点（2026-11-21）

- **整组口径只有一处**：`grid/menu.rs::filters_menu_items()`（纯函数）= 首项查找命令 +
  `filter_settings_menu_items()` 四项。数据格右键（`context_menu_items`）与列头菜单
  （`column_menu_items`）都只 `extend` 它；再手写 push 会让文案 / 分隔线漂。
- **两个请求不要合并**：`ResultRequest::ToggleFind` 是工具栏那颗 CHECK 开关（再按关闭）；
  `ResultRequest::OpenFind` 是菜单命令（只开不关，已开只聚焦）。用 `ToggleFind` 顶替菜单项会
  出现「点菜单把已开的查找栏关掉」这种明显不对的行为——`open_find_is_idempotent_and_never_closes`
  就是钉这个的。
- 面板侧入口是 `ResultPanel::open_find(window, cx)`（幂等）；宿主侧是
  `EditorHostPanel::open_find_via_window`（与 `toggle_find_via_window` 同一窗口转手写法）。
- **文案是自定的**：DBeaver 用平台命令 `EDIT_FIND_AND_REPLACE`（「查找/替换」，文案在平台
  bundle 里，`dbeaver-ref` 未随包提供）；本仓结果集只读、无替换，故取「在结果中查找…」。
  若日后要改成「查找/替换」，只改 `filters_menu_items` 一处。
- 分隔线：查找项 `separator_before: true`（`fillContextMenu:3166`）；四项首项也是 `true`
  （`fillFiltersMenu:3926`）→ 扁平平铺后是「段前一条 + 组前一条」，别把其中一条删掉。
- 未做（§9.37.3 已登记）：`CMD_FILTER_MENU_DISTINCT` / 按操作符生成的条件项 /
  `FilterResetAttributeAction`（无列级约束模型）；`EDIT_FIND_AND_REPLACE` 的快捷键（本仓
  结果面板暂无键位层）。
- 用例：`filters_group_starts_with_the_find_command`（纯函数，5 项 + 两条线 + 两落点）、
  `open_find_is_idempotent_and_never_closes`（窗口）；改首项时同步
  `context_menu_offers_filter_copy_and_freeze` 的标签数组与下标（当前 `items[7]` 是查找项，
  `items[14]` 冻结、`items[17]` 显示全部列）。
- 门禁基线（第 54 批后）：result_panel **391 passed**、editor check `Finished`（0 warning）、
  editor lib **800 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.106 「从上一行 / 下一行复制」的接手要点（2026-11-21）

- **两条通道别混**：`DuplicateRow` 是「**新增**一条本地行」（插行）；`CopyRowValuesFrom`
  是「逐格**改写现有行**」（DBeaver `copyRowValues:5062-5156`）。用户报「点复制后多了一行」
  大概率是把这两条接错了。
- 写哪几格**只有一处口径**：`crates/result_panel/src/changes.rs::row_copy_changes`
  ——已改过的格展示值取 `ChangeSet::peek().edited`、原值取库里那一份；值相同的格不写；
  列数按短的。编辑器侧的循环不要自己再推导一遍（会与撤销栈对不上）。
- 宿主侧落点（`editor/src/view/host/exec/row_ops.rs::copy_row_values_from_adjacent`）：
  底层行 → `mutate_change_set` + `mutate_history`；本地行 → `mutate_local_rows`（**不进**
  变更集 / 撤销栈）；有写入才 `sync_result_view`。改这里时注意 `store.change_set(...)` 的
  不可变借用必须先结束再 `mutate_change_set`（本批按「取值 → 计算 writes → 再 mutate」的顺序写）。
- 门控：源行越界 / 目标行被标记删除 / 列数为 0 静默返回 0。菜单两项**恒摆**（纯函数拿不到
  视图行数），首行 / 末行点不到邻行是空操作——这是有意与 DBeaver `break` 对齐，不要改成「灰掉」。
- 未做（§9.38.3 已登记）：多选区连续分区复制（本仓右键只有一个视图行）、伪列 / 自动生成列
  跳过（无该模型）、`CTRL+D` / `CTRL+ALT+D` 键位（无面板级键位层）、工具栏按钮（DBeaver 也没有）。
- 用例：`row_copy_changes_*`（纯函数）、`change_set_peek_*`、
  `context_menu_row_actions_gate_on_editable_and_deleted`（现五项 + 段内分隔线 + 顺序）。
  **editor 侧 store 编排无行为用例**（无 host 面板测试设施，与 `insert_local_row` 同状况）。
- 门禁基线（第 55 批后）：result_panel **394 passed**、editor check `Finished`（0 warning）、
  editor lib **800 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.107 筛选条列序与组间分隔线的接手要点（2026-11-21）

- **列序只有一处口径**：`crates/result_panel/src/panel/render.rs` 里 `let filter = div()` 的 `.child` 链。
  当前顺序 `显示 SQL → 展开 → 文本 → 运行 → 历史 → 擦除 → 筛选设置 → 后退 → 前进 → ｜ → 查找`。
  历史（`editor-result-filter-history`）**必须紧跟运行**：DBeaver 的 `HistoryPanel` 是 `filterComposite`
  第 5 列（`ResultSetFilterPanel.java:174/214`），在 `filterToolbar` 之前；曾经摆在「筛选设置之后」是错的。
  改顺序只挪 `.when(...)` 块，别动 `.when` 条件（`!filter_expanded && !filter_history.is_empty()`）。
- **分隔线的壳高是硬约束**：`editor-result-filter-history-sep` 是观测壳（`Separator` 没有
  `debug_selector`），壳 `.h(rems(1.25))`（20px）。改成 24px 会把筛选区外框从 24px 撑到 28px，
  `rendered_ui_elements_survive` 的「外框 24px」断言立刻红（本批已踩一次）。
- **分隔线一律用 `Separator::vertical()`**：筛选条这条（`render.rs`）与右段 Toolbar 那条
  （`grid/status.rs::render_toolbar`）都是标准组件，不要再写 `div().w(ui::HAIRLINE).bg(..)`。
- **展开态不摆这条线**：展开后 clear / settings / history / back / forward 全收起，线两侧没有可分的两组。
- **对账表在 §9.39.3**：十一条「已对上」+ 六条「有意差异」，其中最后一条（状态条默认可见集：
  缺 `row.edit`、`fetch.page`/`undo`/`redo` 本仓常摆而 DBeaver 默认隐藏）是**下一批候选**，不要当 bug 直接改。
- 用例：`rendered_ui_elements_survive` 的 1.7 段（x 坐标列序 + 分隔线可见）。
  读几何一律用 `debug_bounds(id)`；`Button` 的 `debug_selector` 会注册 debug bounds。
- 门禁基线（第 56 批后）：result_panel **394 passed**、editor check `Finished`（0 warning）、
  editor lib **800 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.108 状态栏「修改单元格」钮的接手要点（2026-11-21）

- **一颗钮、一条事件**：`editor-result-edit-cell`（`panel/render.rs`）只发
  `ResultRequest::BeginEditCell`；面板 `begin_edit_cell` 读 `grid.selected_cell()` 后
  **emit `TableEvent::DoubleClickedCell(row, col)`**。也就是说它与双击是**同一条链路**，
  不是另一套编辑入口。改行为请改宿主的 `DoubleClickedCell` 订阅（`host/construct.rs:325-339`），
  不要在这里另写门控——两套判定一定会分叉。
- **组件列 0 是行号槽**：`begin_edit_cell` 判 `col == 0` 直接 return；宿主侧同样做
  `col.saturating_sub(1)`（`construct.rs:337`）。把手动改列偏移的话两处必须一起改。
- **`ResultControls` 有三处书写，加字段必同步**：`panel/render.rs` 的
  （1）全字段字面量、（2）**全字段解构（无 `..`，漏加即 E0027）**、（3）`..Default::default()`。
  本批新增 `edit_row: Option<AnyElement>` 时三处都动了。
- **占位号是 `placeholders[4]`**：`grid/status.rs:220` 的 Pencil 占位描述本就写着
  「修改选中格（双击单元格进入编辑）」——本批把占位换成真钮，文案与行为终于对上。
  只读结果集不注入真钮 → 回退该占位，布局不跳。
- **为什么不是外部值编辑器**：DBeaver 状态条 `row.edit` 是 `openValueEditor(false)`
  （`ResultSetHandlerMain.java:207-211`），双击/回车才是 `inline`（`:212-216`、键位
  `plugin.xml:1071/1072`）。本仓无「值编辑器 + 写回」通道（`ports.rs:214-215`），只能落内嵌这一族，
  这是 §9.40.3 记的第 1 条有意差异。
- **未做**：`SHIFT+ENTER` 外部编辑键位；右键 edit 段的「编辑单元格」项（`BeginEditCell` 读的是
  选中格，菜单 target 是右键格，语义不同）；状态条默认可见集里 `fetch.page`（本仓有意常显）与
  `undo`/`redo`（本仓默认隐藏，已对上）之外的其余差异。
  记录（整行）模式下这颗钮的可用态**已由第 58 批补齐**（置灰，见 §2.109）；不要再用
  `selected_row` 兜底猜列，那会改错格。
- 用例：`begin_edit_cell_emits_the_double_click_only_for_data_cells`（窗口，哨兵 `selection_pos`
  抹掉 `SelectCell` 副作用）、`analysis.rs::begin_edit_cell_request_enters_inline_editing`
  （端到端：真跑 SQL → 选中 → 面板入口 → `delegate().editing()`）。窗口用例必须先
  `cx.update(gpui_kit::init)`；`VisualTestContext` 里取 `Entity::read` 要
  `cx.update(|_window, app| panel.read(app).…)`。
- 门禁基线（第 57 批后）：result_panel **395 passed**、editor check `Finished`（0 warning）、
  editor lib **801 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.109 「修改」钮可用态门控的接手要点（2026-11-21）

- **唯一口径**：`panel/render.rs` 的 `edit_row` —— `disabled(transposed || !picked)`，
  `picked = self.selection_pos.is_some_and(|(_, col)| col > 0)`。别在 `begin_edit_cell` 里
  再加一套判定：那是运行时防线（无选中格即 return），这里是 UI 灰态，两回事。
- **灰态靠三段事件重绘**：`TableEvent::SelectCell` / `ClearSelection`（`lifecycle.rs:365-410`）
  与 `toggle_transpose`（`events.rs:432`）都会 `cx.notify()`。以后若新增「设置选中格」的入口，
  忘了 notify 就会让灰态滞后一帧甚至更久。
- **测试前提是 `panel_harness` 的两个默认值**：只读（`editable == false`）+ 无状态行
  （`show_status` 为假）。测这颗钮必须先 `p.editable = true` 并注入
  `p.status = Some(ResultStatus { total_rows: 2, ..Default::default() })`，否则钮根本不在树里
  （本批首跑 `try_find` 返回 `None` 就是这个原因）。
- **断言口径**：可用态不写 a11y disabled 位（`disabled()` = `None`），只有禁用才 `Some(true)`
  ——钉 `assert_ne!(.., Some(true))` 而不是 `assert_eq!(.., Some(false))`。
- **为什么比 DBeaver 严**：那边状态条那颗是 `"edit"`（只要当前行），本仓落 inline 链路
  （§9.40.2），所以还要求「有选中数据格」。要放松就先改落点，别只改这一个 bool。
- 未做：属性级只读（本仓无列级只读模型）、其它行级钮是否也按格判（新增 / 复制 / 删除目前
  只按选中行）。面板级键位层（`ENTER` 内嵌编辑 / `Ctrl+反引号`）已由 §2.110 交付。
- 门禁基线（第 58 批后）：result_panel **396 passed**、editor check `Finished`（0 warning）、
  editor lib **801 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.110 结果网格键位层的接手要点（2026-11-21）

- **键位基础设施早就在**，别再新建 context：`grid/delegate.rs:30` 的
  `RESULT_GRID_CONTEXT = "editor-result-grid"`，挂在 `grid/status.rs:593-616` 的网格容器上。
  新键位只要 action 定义在 `grid/mod.rs`、handler 挂 editor 宿主的 `.on_action`、
  再到 `crates/app/src/main.rs` + `tests/mod.rs::bind_editor_keys` 各注册一次即可。
- **action 定义要看宏可见性**：`actions!(result_panel, [Name]);` 需要先
  `use gpui_kit::actions;`；宏前若写 `///` 说明会报 `unused_doc_comment`（写 `//`）。
- **handler 只转调面板方法**，别在 editor 里再写一套状态机：
  `ENTER` → `panel.begin_edit_cell(cx)`（同双击 / 状态栏「修改」），
  `Ctrl+反引号` → `panel.cycle_presentation(cx)`（同左缘竖条切换）。
- **端到端用例必须把焦点交给网格**：
  `panel.read(app).grid_for_test().read(app).focus_handle(app)`（`TableState` 实现 `Focusable`），
  再 `window.focus(&handle, cx)`。焦点留在面板/编辑器上时按键不落 `RESULT_GRID_CONTEXT`，
  action 不触发，用例会「按键没反应」。
- **为什么 `ENTER` 安全**：vendor `DataTable::init`（`table/data_table.rs:16-31`）没绑 enter；
  编辑态的 `ENTER` 由 `vendor/gpui-base/src/input/base/state.rs:229-277` 的 `"Input"` context 先吃（提交）。
  以后若要改表格的 enter 绑定，先回来确认这条不会重复进编辑。
- **文本态盲区已解决（第 60 批，见 §2.111）**：面板根 `editor-result-root` 挂了
  `RESULT_PANEL_CONTEXT` + `track_focus`，切呈现后宿主把焦点移交到
  `ResultPanel::presentation_focus_target(cx)` 给出的句柄。
- **未做**：`SHIFT+ENTER`（外部值编辑器，本仓无写回通道，§9.40.3 第 1 条）。
  编辑菜单/右键里的编辑项**已由第 61 批补上（见 §2.112）**。
- 门禁基线（第 59 批后）：result_panel **396 passed**、editor check `Finished`（0 warning）、
  editor lib **803 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.111 面板级键位上下文与焦点移交的接手要点（2026-11-21）

- **两层 context 是父子关系**：`RESULT_PANEL_CONTEXT = "editor-result-panel"`（面板根）⊃
  `RESULT_GRID_CONTEXT = "editor-result-grid"`（网格容器）。同一键绑两层是安全的：
  gpui 按 context 深度取更深的网格那条，只执行一条（不会切两次）。
- **别在面板层做焦点移交**：面板 `render` 是 `&App`（挂不了 `on_action`），事件方法用
  `Context<Self>`（不是 `VisualContext`，`Entity::update_in` 会 E0277）。移交必须在宿主侧，
  用 `.on_action` handler 的 `window` 或 `observe_in` 回调的 window。
- **唯一移交入口**：`ResultPanel::presentation_focus_target(&self, cx) -> FocusHandle`
  —— 文本态 → 面板根 `focus_handle`，网格态 → 网格 `TableState` 的 `focus_handle`。
  宿主切完 presentation 后调 `window.focus(&target, cx)`（见 `focus_after_presentation_switch`）。
- **`drain_result_actions` / `dispatch_result_request` 带 window**：点击路径（左缘竖条 /
  右键 layout 段）也走这两条，所以 window 一路透传；新增调用点记得补参数
  （测试里从外层 `cx.update(|window, cx| …)` 捕获）。
- **面板根的 `track_focus` 不带 `tab_stop`**：它只是焦点落点，不参与 Tab 顺序。别顺手加，
  否则 Tab 遍历会多停一站。
- **面板侧用例的取句柄写法**：面板**没有** `grid_for_test()`（那是 editor 宿主的）；测试里
  直接读 `pub(crate)` 字段：`panel.read(app).grid.clone()` / `panel.read(app).focus_handle.clone()`，
  再 `grid.read(app).focus_handle(app)`（`Focusable` trait，`FocusHandle: PartialEq` 可断言）。
  该 `tests.rs` 的 `#[gpui_kit::test]` 签名写全路径 `cx: &mut gpui_kit::TestAppContext`。
- **已知行为**：从右栏页签 / 筛选框里按 `Ctrl+反引号`，焦点会被移走（切回网格时落网格、
  切到文本时落面板根）。这是显式移交的结果，不是 bug。
- 门禁基线（第 60 批后）：result_panel **397 passed**、editor check `Finished`（0 warning）、
  editor lib **804 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.112 右键「修改单元格」的接手要点（2026-11-21）

- **两条入口别合并**：`begin_edit_cell(cx)` 读**网格选中格**（状态栏「修改」钮、`ENTER`），
  `begin_edit_cell_at(row, col, cx)` 用**给定格**（右键）。右键不改选区，右键的格与选中格
  可以不同——合并回一条会变成「右键 A 格、实际编辑 B 格」。
- **坐标折算在哪**：`ContextTarget::column` 是**数据列下标**（已折算掉行号槽，`menu.rs`
  的 `let column = delegate.data_column(col - 1)`），`DoubleClickedCell` 要的是**组件列**
  （数据格从 1 起）。折算点只有一处——`ContextAction::EditCell` 分派里 `let col = column + 1;`
  别在面板侧再折一次（会变成数据格从 2 起）。
- **请求仍走行级队列**：`delegate.on_row_action` 钩子（与 `InsertRowBelow` / `DuplicateRow` /
  `CopyRowValuesFrom` 同一份），`editor` 侧在 `dispatch_result_request` 分支里转调面板方法。
  不要为它单开一条 action 通道。
- **菜单文案统一口径**：沿用状态栏那颗的「修改单元格」（§9.24），不照 `bundle_zh` 的
  「内嵌编辑」。要改就两处一起改（状态条 label + 菜单项 label）。
- **段位与分隔线**：它是 edit 段首项且 `separator_before: true`（开段）；「在下方插入新行」
  也保留 `separator_before: true`，因此那条线现在承担 DBeaver `:3208` 的**段内** Separator。
  以后在 edit 段里插新项，注意别把这两条线的语义搞混。
- **用例三层**：纯函数 `context_menu_edit_cell_leads_the_edit_group`（门控 / 坐标 / 段位）、
  面板 `begin_edit_cell_at_uses_the_given_cell_instead_of_the_selection`（落点优先于选区）、
  editor 端到端 `begin_edit_cell_at_request_enters_inline_editing_without_a_selection`
  （**不设选区**也能进编辑，这是本批与第 57 批用例的关键差别）。
- **别用选中格去断言**：端到端用例刻意不设选区；若先 `set_selected_cell` 再发请求，用例就
  退化成第 57 批那条、验不出落点独立性。
- **全量并发下的既有 flake**：`ports::editor_completion::tests::*`（3 条，warm-introspection
  竞态）偶发失败；单跑 `ports::editor_completion::tests::` 13 条全过、全量重跑也全过。
  遇到先重跑，别当成本批回归。
- **未做**：`CMD_CELL_SET_NULL` / `CMD_CELL_SET_DEFAULT` / `CMD_CELL_RESET`（本仓无 NULL /
  默认值 / 单格撤销语义，§9.44.3 第 3 条）。
- 门禁基线（第 61 批后）：result_panel **399 passed**、editor check `Finished`（0 warning）、
  editor lib **805 passed（1 ignored）**、ui-guard **4 passed**；本批未动 settings。

### 2.113 编辑门控单一判据的接手要点（2026-11-21）

- **改编辑入口的可用态只改一处**：`crates/result_panel/src/grid/edit.rs` 的
  `can_edit_cell(editable, record_view, has_current_cell)`（`grid/mod.rs` 已 re-export）。
  状态栏「修改」钮（`panel/render.rs`）与右键编辑段（`grid/menu.rs`）都读它；
  别再在任一处写 `transposed || !picked` 之类的局部式子。
- **`editable` 的来源别搞错**：面板的 `self.editable` 是宿主用
  `editor::exec::editability::result_editable(...)` 算好同步的（`host/results/grid.rs:211-223`），
  已含 通道 / 项目 / 文档只读 / 连接只读 / 行标识。`can_edit_cell` 只补「呈现」与
  「当前属性」两条；在面板层再判前五项就是重复（也拿不到 channel）。
- **`is_deleted` 故意不在函数里**：它是本仓叠加层的第 4 个维度，只作用于右键整段
  （DBeaver `STATE_REMOVED` 只管 `CMD_CELL_RESET`）。要给状态栏钮也加，是另一批的事。
- **键位不要跟着判 `editable`**：`begin_edit_cell_at` 只拦 `col == 0` 与 `self.transposed`。
  判 `editable` 会让只读结果集**丢掉**宿主 `cell_editable` 给出的原因提示
  （`host/construct.rs:340-354` 的顶部说明）。
- **权威判据仍在下游**：delegate `begin_edit`（`grid/edit.rs`，转置 / 可编辑 / 编辑中 /
  行号槽 / 已删行）与 `cell_editable`。`can_edit_cell` 只是它们的 UI 投影，
  别把它当第二套规则去加白名单。
- **新增入口时**：先问「它有没有当前属性 / 能不能在记录视图里用」，再决定传参；
  `has_current_cell` 的说法是「组件列 > 0」，不是「选中了行」。
- **既有 flake 已修（第 63 批，见 §2.114）**：`ports::editor_completion` 的 4 条在默认并发下
  会踩 `wait_until` 的 3s deadline（`editor_completion.rs:1574-1592`），已放宽到 10s；
  现在门禁直接跑默认并发，不用再 `--test-threads=1`。
- 门禁基线（第 62 批后）：result_panel **401 passed**、editor check `Finished`（0 warning）、
  editor lib **805 passed（1 ignored，单线程）**、ui-guard **4 passed**；本批未动 settings。

### 2.114 editor_completion 等待上限的接手要点（2026-11-21）

- **跑门禁用默认并发**：`cargo test -p rds-editor --lib`（805 / 1 ignored）。此前那 4 条
  completion 用例在并发下超时的问题已修（`WAIT_DEADLINE` 10s），不需要再加
  `--test-threads=1`。
- **超时上限的位置**：`crates/editor/src/ports/editor_completion.rs` 的
  `mod tests` 辅助区，`const WAIT_DEADLINE: Duration = Duration::from_secs(10);`，
  `wait_until` 紧接其后。断言文案带 `{WAIT_DEADLINE:?}`，别再写死「3s」。
- **为什么是 10s**：正常路径条件几十毫秒成立（整组 13 条 0.9~1.3s），10s 只用来把
  「并发饿死」与「真卡死」分开。**不要**用「重跑到过」当结论；连续跑 3 次仍绿才算验证。
- **若 10s 也不够**：先确认该条件是否真有界（现为「后台落笔某字段」，有界），再提高阈值或
  改为信号量；别加全局串行依赖。
- 门禁基线（第 63 批后）：result_panel **401 passed**、editor lib **805 passed（1 ignored，
  默认并发）**、ui-guard **4 passed**、check Finished（并行线 `rds-notification` 有 2 条
  never-used warning，与本线无关）。

### 2.115 行级三钮可用态的接手要点（2026-11-21）

- **判据只有一处**：`crates/result_panel/src/grid/menu.rs` 的
  `pub fn row_action_state(editable, has_current_row, row_deleted) -> RowActionState`。
  渲染层不要再自己写 `row.is_none()` 之类的条件——那是第 64 批之前的写法，已替换。
- **两套门不要混**：`can_edit_cell(editable, record_view, has_current_cell)` 管「编辑某一格」；
  `row_action_state` 管「行级新增/复制/删除」。前者不调后者，后者也不调前者。
- **`add` 不要求当前行**：`InsertRowBelow(None)` 的语义就是追加到末尾，所以无选中时
  「新增」仍应可点。若后续有人改成 `has_current_row && ...`，那是回退。
- **`delete` 为什么只看 `!row_deleted`**：本仓没有 `isDeletable` 能力位，`editable` 已含
  通道/项目/文档/连接只读/行标识/列缺失六道门（§9.45）。DBeaver 那边多出的那层是
  `isManipulationSupported(FEATURE_DATA_DELETE)`，本仓以 `editable` 代偿。
- **`copy` 有意不看行状态**：DBeaver 的 `"copy"` 分支只判 `currentRow != null && isInsertable()`，
  已删行仍可复制。别「顺手」给它加 `!row_deleted`。
- **`row_deleted` 的来源**：`panel/render.rs` 用 `status.selected_row`（1 基）减 1 得视图行，
  再 `self.grid.read(cx).delegate().is_deleted_row(row)`。`render` 是 `pub fn render(&self, cx: &App)`，
  拿不到 `&mut Context`，所以只能 `read`——这是既有约束，不是遗漏。
- **造已删行的测试素材**：`set_data(columns, rows)` + `set_local_snapshot(Some(LocalRowSet::new()))`
  再 `local.mark_deleted(底层行号)` + `state.refresh(cx)`；无筛选无排序时底层行号即视图行号。
  `LocalRowSet` 从 `crate::rowset` 取，`RowRef::Local` 的行 `is_deleted_row` 恒 false。
- **断言禁用态**：可用时 a11y `disabled()` 是 `None`，只有真禁用才是 `Some(true)` ⇒
  用 `assert_ne!(…, Some(true))` 断「可用」。钮的 `ElementId` 是
  `editor-result-insert-row` / `-duplicate-row` / `-delete-row` / `-edit-cell`。
- 门禁基线（第 64 批后）：result_panel **403 passed**、editor lib **805 passed（1 ignored，
  默认并发）**、ui-guard **4 passed**、check Finished（并行线 `rds-notification` 2 条
  never-used warning，与本线无关）。
- **仍未接**：状态栏「修改」钮不感知已删行（**第 66 批已补，见 §2.117**）；
  `SET_NULL` / `SET_DEFAULT` / `CELL_RESET` 无对应语义；列级只读缺失。

### 2.116 右键菜单「灰项」的接手要点（2026-11-21）

- **`ContextMenuItem` 多了必填字段 `disabled: bool`**（`grid/menu.rs`，56 处字面量已按默认 `false`
  补齐）。新加菜单项时编译器会逼你写，**默认必须给 `false`**：守卫用例
  `context_menu_items_are_enabled_unless_explicitly_shaded` 会在「可编辑未删行出现灰项」时炸。
- **渲染只能走 `shade(menu, item.disabled, …)`**：`context_menu` 里 **34 处** add 点已统一
  （主循环 33 + 行号槽早退路径 1）。
  若有人写回 `menu = menu.item(...)`，**编译不报错**，但灰态静默丢失——这是本批最容易踩的坑。
  `shade` 就是 `menu.item(el.disabled(disabled))` 的薄封装（vendor `menu/popup_menu.rs:159-171`）。
- **§9.47 的 delete 归因已更正**：DBeaver `"delete"` 分支在 `isDeletable()` 为真时**直接返回
  true**，不看 `STATE_REMOVED`（`ResultSetPropertyTester.java:142-157`）；
  `deleteSelectedRows:5198-5200` 也不因此拒删。本仓的「已删行删除钮置灰」是**有意收紧**
  （`delete_local_row` 对已删底层行 `return false`，点了没动静），不要再写成「与 DBeaver 同向」。
- **右键 edit 段的现行口径**：段前提只有 `can_edit_cell(target.editable, target.record_mode, true)`；
  段内五项逐项 `disabled`（修改单元格 ← `target.is_deleted`；插入 ← `!add`；复制 ← `!copy`；
  删除 ← `!delete`；邻行复制恒亮）。`row_actions` 在段外用
  `row_action_state(target.editable, true, target.is_deleted)` 先算（右键必有一行）。
- **`row_action_state` 三参数不能删 `row_deleted`**：工具栏 delete 钮与右键 delete 项都靠它收紧。
- **菜单灰项的视觉归组件主题**：headless 只锁「`disabled` 布尔 + 编译级接线」，看不到实际灰底/
  灰字；目视统一时一并看。
- 门禁基线（第 65 批后）：result_panel **404 passed**、editor lib **805 passed（1 ignored，
  默认并发）**、ui-guard **4 passed**、check Finished（并行线 `rds-notification` 2 条
  never-used warning，与本线无关）。
- **未做**：`filters` / 布局段仍用「不摆」表达不可用，是否统一成灰项要逐段对照 DBeaver 的
  `setEnabled` 调用点，别一次性铺开。

### 2.117 状态栏「修改」钮的行状态判据（2026-11-21）

- **判据取当前格，不取当前行**：`panel/render.rs` 的 `edit_row` 闭包用
  `selected = self.selection_pos` 算 `picked`（`col > 0`）与 `cell_deleted`
  （`grid.delegate().is_deleted_row(row)`），**不要**改用 `status.selected_row`。
  两者是两个来源：`selection_pos` 由 `TableEvent::SelectCell` 写（`lifecycle.rs:370-374`），
  `status.selected_row` 由宿主 `set_selected_row` 写（`events.rs:2057`）。混用会在
  「换了格但 currentRow 还是旧的」时让钮不亮。
- **对应 DBeaver 的两个不同方法**：「修改单元格」= `getCurrentAttribute()`
  （`ResultSetPropertyTester.java:127-136`）；行级三钮 = `getCurrentRow()`
  （`:142-157`）。照抄对应关系，别互相借用。
- **`disabled = !can_edit_cell(...) || cell_deleted`**：`can_edit_cell` 仍是唯一「呈现 + 当前属性」
  判据（第 62 批），行状态由**调用方叠加**，函数签名不动。
- **tooltip 四态序**：记录视图 → 未选数据格 → 行已删 → 正常。新增态插在第三位，别打乱前两态的
  文案（有用例守着 `status_commands_follow_dbeaver_default_visibility_and_can_be_restored` 邻居）。
- **用例口径**：三段（已删行灰 → 换未删行可用 → `set_local_snapshot(None)` 后恢复）。可用态断言
  用 `assert_ne!(snap.disabled(), Some(true))`，别写 `== None`。
- **门禁注意**：`cargo test -p rds-editor --lib` 的 `view::tests::exec` 两条大文件用例会
  `set_len(200MiB/50MiB)` 造稀疏文件，磁盘吃紧时直接 panic `StorageFull`（`exec.rs:943`）——
  **是环境态，不是回归**。D 盘现在只剩 ~40MB（target 在 D 盘），跑之前先看 `Get-PSDrive`。
- 门禁基线（第 66 批后）：result_panel **405 passed**、editor lib **805 passed（1 ignored）**、
  ui-guard **4 passed**、check Finished（并行线 `rds-notification` 2 条 never-used warning）。

### 2.118 右键「重置单元格」（`CMD_CELL_RESET`）（2026-11-21）

- **条件 add ≠ 灰项**：DBeaver `fillEditMenu:3536` 是 `if (row.getState() == STATE_REMOVED ||
  row.isChanged(attr)) add(CMD_CELL_RESET);` —— **不成立时菜单里没有这一项**，与同段
  `add`/`copy`/`delete` 的「无条件 add + `setEnabled(false)`」是两种表达。本仓两条都照抄：
  行级三钮 + 「修改单元格」是灰项（§9.48），「重置单元格」是条件出现（§9.50）。
  写这一项时**不要**顺手改成灰项——那会与 DBeaver 相反。
- **判据全在 `ContextTarget` 里**：新字段 `cell_changed`，构造点取
  `delegate.is_dirty(row, col)`。`is_dirty(视图行, 组件列)`、`ContextTarget::column` 是
  **数据列** —— 构造时 `col` 就是组件列（别先折），出请求时才 `column + 1`。
  这是本批最容易写错的一处（首版漏了 `+1`，会静默操作到左一格）。
- **两支顺序固定**：`reset_result_cell` 先查 `is_deleted(ix)` → `unmark_deleted` 并 return；
  否则 `changes.remove(ix, column)`。反了就会出现「行还在删除态、格却已经清了」。
  DBeaver `ResultSetModel.resetCellValue:571-590` 同序。
- **显示回落天然**：网格显示值 = `entry.rows` 叠加变更集，移掉那一条即回原值，
  不需要显式回写（DBeaver 那边要 `setAttributeValue`，因为它的 cell value 是持久的）。
- **不碰撤销栈**：`RowHistory` 里的 `DeleteData` 保留，之后按撤销会重跑 `unmark_deleted`
  （未删行幂等）。这与 DBeaver 不走 undo context 同向；若将来要清栈，先想清楚
  `undo_row_action` / `redo_row_action` 的组合语义。
- **本地行不参与**：本地行格值在快照里、不进变更集（#28-B4），`RowRef::Local` 直接 false。
- **用例断言改用可见物**：`is_dirty` / `dirty` 是 `pub(crate)`，editor 测试跨 crate 读不到
  （编译报 E0624）。改用**「待提交按钮组」`editor-result-commit-group` 的可见性**：
  预置脏格后 `is_some()`、重置后 `is_none()`（比读内部字段更贴用户所见）。
  行删除态可用 `delegate().is_deleted_row(0)`（那是 `pub`）。
- **门禁基线（第 67 批后）**：result_panel **407 passed**（+2）、editor lib
  **807 passed（1 ignored）**（+2）、ui-guard **4 passed**、check Finished。
- **磁盘判断更正**：§2.117 的「D 盘只剩 ~40MB」是 `Get-Volume` 行渲染错位的误读；
  `Get-Volume` 实测 **86.72 GB 空闲**。真正的原因见 §2.117 的 `StorageFull` 段（机制未查透），
  处置仍是：跑 editor 全量前清掉 `.rds/tmp` 下 `rds-test-root-*` / `rds_editor_tier_*` /
  `rds_gtest_*` / `rds_svc_it_*`（本批清掉一个 `rds-test-root-*`，864 MB，之后一次通过）。
- **本批未做**：`ESC`（`plugin.xml:1081`）未接——**第 68 批已查清原因，见 §2.119**
  （组件 `DataTable` 的 `Cancel` 抢键，不改 vendor 接不了）；`SET_NULL` / `SET_DEFAULT` 仍未建模；
  「重置」是否连带清 `RowHistory` 的 `DeleteData` 待拍板；文案「重置单元格」是自定
  （`bundle_zh` 是「重置单元格更改」）。

### 2.119 键位层 `Ctrl+S` = 提交待提交变更（2026-11-21）

- **覆写键位是既有手法，不是新发明**：`ctrl-s` 在 `editor` context = `SaveDocument`
  （`app/src/main.rs:301`），在 `database-nav` = `NavCommitStaged`（`:410`）。第 68 批加第三条：
  `RESULT_GRID_CONTEXT` = `CommitResultChanges`。gpui 按 dispatch 路径深度取更深者，
  焦点在网格时不会同时触发存文档。
- **动作与命令同源**：`ResultRequest::CommitPending` 与工具栏「提交 N 处」都走
  `commit_pending_from_ui` → `submit_pending_changes`（在途 / 通道 / 项目只读 / 连接只读 /
  无行标识 / 无变更 五道复核）。**不要再给键位单独写一条提交路径**——两套复核迟早会漂。
- **`ESC` 在本仓结果网格里是死路（结构性）**：组件 `DataTable` 自己绑了 `escape → Cancel`
  （`vendor/gpui-component/src/table/data_table.rs:19`），`table/state.rs:943-949` 是
  「有选区先清选区并 return，无选区才 `cx.propagate()`」。本仓 `RESULT_GRID_CONTEXT` 挂在
  **包裹 `DataTable` 的外层 div**（`grid/status.rs:594-602`），gpui 按键按 dispatch 路径
  深层优先（`gpui-pre-0.3.11 window.rs:6321-6363` 一次匹配后不再换 binding）⇒ 焦点在网格时
  `ESC` 必先被组件吃掉，我们挂不上 `cell.reset`。要接只能改 vendor 或另加拦截层，
  两者都超出「不改 vendor」边界；DBeaver 也没有「表格 ESC 清选区」。
  **记档为有意差异：`ESC` 保留组件语义（清选区）。**
- **`Ctrl+S` 在网格里不再保存 SQL 文档**：这是 DBeaver 同行为（`plugin.xml:1082`），
  但用户感知上是行为变更，需要用户确认接受。`editor` context 那层不受影响。
- **用例造法**：与 §2.118 的提交用例一致——`commit_ready_shared` + 两条 `seed_cell_change`
  （行 0 列 1、行 1 列 1），回执 `per_statement: vec![1, 1]`。**列 0 不能拿来 seed**：
  行号槽不参与提交映射，`pending_count()` 不归零，用例会假失败（本批踩过一次）。
- **焦点移交**：`panel.read(app).grid_for_test().read(app).focus_handle(app)` +
  `window.focus(&grid_focus, cx)`，之后 `simulate_keystrokes("ctrl-s")` 才落在网格 context。
- **空态的断言**：`commit_in_flight_for_test().is_none()` + 状态栏 `message` 含「没有待提交」
  （`EditorHostPanel::message` 是 `pub` 字段，测试可直读；**没有** `notice_for_test`）。
- **门禁基线（第 68 批后）**：result_panel **407 passed**（不变）、editor lib
  **808 passed（1 ignored）**（+1）、ui-guard **4 passed**、`cargo check --workspace
  --all-targets` Finished（并行线 `rds-notification` 2 条 never-used warning）。
- **提交摘 hunk**：`crates/app/src/main.rs` 与通知并行线共享（app identity ~行 44、
  `flush_now` ~211、`set_app_identity` ~223），本批键位 hunk 在 ~360。做法见
  `.rds/tmp/stage-main-hunk-r128.py`（取 HEAD 内容 + 从工作区逐字摘块 + `update-index`）。
- **本批未做**：`CTRL+R`（`rejectChanges`，`plugin.xml:1083`）未接，`discard_pending_changes`
  无确认弹窗，绑键前先决定是否补确认；`CTRL+BACKSPACE`（`setDefault`）随 `SET_NULL`/
  `SET_DEFAULT` 一起仍未建模。

### 2.120 行级键位组（2026-11-22）

- **行级动作的两个入口现在都读同一段**：`ResultPanel::row_target_state(cx)` →
  `grid::row_target_state(editable, selected_row, row_deleted)`。状态栏三钮与
  `ALT+INSERT` / `CTRL+ALT+INSERT` / `ALT+DELETE` / `CTRL+D` / `CTRL+ALT+D`
  五个 handler 全走它；**别再把 render 里那套式子复制一份到宿主**（第 64 批的原式
  已删，复制回去就是两套算法）。
- **`status.selected_row` 才是行级口径**，`selection_pos`（当前格）是格级口径
  （第 66 批 §2.117）。行级键位不能用 `selection_pos` 折行——那会把「停在行号槽」
  与「整行选中」混起来。
- **行级动作后 `selected_row` 会丢**：`sync_result_view` 构造 status 时写
  `selected_row: None`（`host/results/grid.rs:169`）。表现是：插一行后状态栏不再记
  「已选第 N 行」、三钮同时变灰，再按 `CTRL+ALT+INSERT` 会提示「先选中一行」。
  这是既有口径，**用例里每次按键前要重新 `set_selected_row(Some(n))`**，
  否则第二条键必然假失败（本批踩过一次）。
- **测试里怎么选中行**：`panel.read(cx).result_panel` 是 `pub(crate)` 字段
  （`host/mod.rs:341`），拿它 `.update(cx, |p, cx| p.set_selected_row(Some(1), cx))`
  ——这是事件路径同一个函数，不要直接改 `status` 字段。
  `EditorHostPanel` 本身**没有** `set_selected_row` / `row_target_state`（那是面板的）。
- **邻行复制的门控顺序**：只读 → 无当前行 → 目标行已删 → 源行越界 → 写 0 格。
  `ResultPanel::adjacent_row_view(row, above, cx)` 回答「源行在不在」，
  不自己碰 `grid.read()`。
- **键位串**：`alt-insert` / `ctrl-alt-insert` / `alt-delete` / `ctrl-d` / `ctrl-alt-d`
  在 gpui 里都能解析（`simulate_keystrokes` 实跑通过）。`ctrl-d` 在本仓
  `editor` context **没有**既有绑定，无覆写冲突。
- **没接的两条**：`SHIFT+ALT+INSERT`（add.before）、`CTRL+SHIFT+ALT+INSERT`
  （copy.before）——本仓本地行固定落末尾（#28-B3），这两条会退化成与不带 SHIFT
  相同，属于「有意差异」，不要当缺口反复补。
- **门禁基线（第 69 批后）**：result_panel **408 passed**（+1）、editor lib
  **810 passed（1 ignored）**（+2）、ui-guard **4 passed**、
  `cargo check --workspace --all-targets` Finished。
- **只摘 main.rs 本线 hunk**：`.rds/tmp/stage-main-hunk-r129.py`（锚在第 68 批
  `ctrl-s` 那条 KeyBinding 之后；同样从工作区逐字摘块 + `update-index`）。

### 2.121 取数与跳转键位（2026-11-22）

- **`CTRL+ALT+N` 不要在面板层重写判定**：`ResultPanel::request_next_segment` 只
  入队 `ResultRequest::FetchMore`，一切前置与提示都在宿主 `fetch_more`
  （`host/results/copy.rs:105-143`）。DBeaver 的四道前置（quickFilter /
  verifyQuerySafety / hasMoreData / blocked）在本仓的对应物就是这一段——
  **在键位 handler 里再写一遍「有没有下一段」，就是第二套真值表**。
- **`CTRL+G` 只准备 + 聚焦，不弹窗**：`ResultPanel::row_jump_focus(window, cx)`
  返回 `Option<FocusHandle>`，`status` 为空时返回 `None`（宿主据此提示
  「还没有行可跳」）。行号框的 `InputEvent::PressEnter` 必须把焦点交回网格
  （`grid.read(cx).focus_handle(cx)`；要 `use gpui_kit::Focusable as _`）——
  **漏了这步，跳完行网格键位全哑**，是本批最容易回归的一处。
- **不预填 value，改放 placeholder**：`InputState` 对外没有 `select_all`
  （`vendor/gpui-component/src/input/state.rs:214` 是 `pub(crate)`）。预填后用户
  敲数字会追加（`1` + `2` = `12`）直接误跳，所以写 `set_placeholder(当前行号)`
  + `set_value("")`。这是与 DBeaver（预填 + 全选）**有意**的差异，别当 bug 改回来。
- **`goto_row` 之后 `status.selected_row` 会被事件路径补回**：
  `TableState::set_selected_row` 会 `emit(TableEvent::SelectRow)`（`table/state.rs:555`），
  所以「敲 `2` → 当前行变 2」是一个干净可断言的判据，也是「焦点确实进了输入框」
  的证明（数字落在网格上不会产生 `GoRow`）。
- **取段用例必须先钉死「按键前 `asked` 为空」**：`delegate.load_more` 在渲染路径里
  也会被触发（`rendering_a_short_result_asks_for_more_by_itself`），不先断言空，
  就分不清请求是按键发的还是重绘发的。
- **`ctrl-g` 在本仓 `RESULT_GRID_CONTEXT` 没有既有绑定**，不覆写任何键；
  `ctrl-alt-n` 同理。两条都只挂网格 context（面板层不挂）。
- **没接的三条**：`CTRL+SHIFT+=`（fetch.all，缺确认弹窗 + 宿主 `FetchAll` 只取一段）、
  `CTRL+SHIFT+G`（gotoColumn，本仓无「选中列并滚动」入口）、`CTRL+R`
  （rejectChanges，`discard_pending_changes` 无确认）。前两条见主文档 §9.53.5。
- **门禁基线（第 70 批后）**：result_panel **408 passed**、editor lib
  **812 passed（1 ignored）**（+2）、ui-guard **4 passed**、
  `cargo check --workspace --all-targets` Finished。
- **editor 全量首跑 rustc OOM 的规避**：本轮首跑在**编译阶段**挂掉
  （`memory allocation of 2097152 bytes failed` + `STATUS_STACK_BUFFER_OVERRUN`，
  `error: could not compile rds-editor (lib test)`），当时物理内存仅余 ~3.8 GB。
  加 `$env:CARGO_INCREMENTAL='0'`（本轮不用增量，**不删**
  `target\debug\incremental`）后一次通过。这是继第 66 批 `StorageFull` 之后
  第二个环境性失败点，与代码无关；判据看日志里的 `test result:` 行，不看 exit code
  （`2>&1 | Tee-Object` 会把 cargo 的 stderr 包成 `NativeCommandError`）。
- **只摘 main.rs 本线 hunk**：`.rds/tmp/stage-main-hunk-r130.py`（锚在第 69 批
  `ctrl-alt-d` → `CopyRowValuesFromBelow` 那条 KeyBinding 之后）。

### 2.122 `FetchAll` 真循环（2026-11-22）

- **循环只能落在回填点**：`fetch_all` 只负责「发起 + 置标志」，每段的推进在
  `pump.rs` 的 `pending == 0` 块里（`advance_fetch_all`）。写成 `while` 是错的——
  `execute` 返回时这一段的行还没回来，`offset` 会一直停在旧值。
- **调用顺序**：`advance_pending_export`（导出）→ `advance_fetch_all`（取全部），
  两者都在 `sync_result_view` 之后。**不要在 `sync` 之前调**：下一段的 offset 取
  「已抓到的行数」，只有同步过才是新值。
- **提交只有一个入口**：`submit_segment`（`results/copy.rs`，私有）。`fetch_more` /
  `fetch_all` / `advance_fetch_all` 都走它；判定（无 SQL / `can_fetch_more` 为假）与提示
  都在里面。**不要在别处再写一遍 `entry.can_fetch_more()` 分支**——第 70 批之前
  `dispatch` 的注释与实现不符就是这种双写的后果。
- **两道互斥门控**：`fetch_more` 与 `fetch_all` 入口都判「正在取全部」和「导出在跑」。
  导出侧不用改：`advance_pending_export` 自己 `execute(Segment)`，不经过 `fetch_more`。
  导出**启动**时反查 `fetching_all` 是第 73 批补的（§9.56.2），别以为已经在了。
- **`fetching_all` 与 `fetching_more` 不是一回事**：前者跨多轮（回填时才推进），
  后者只在「提交到回填之间」为真。别用 `fetching_more` 当取全部的防重入判据。
- **切结果集必须停**：`FetchAllProgress.set` 记发起时的 `active()`，每轮比对；
  `Append` 落位永远接「当前选中」那份，不停就会把行接到另一份结果上。
- **测试想验「真循环」必须三段 runner**（`shared_with_three_segment_runner`）：
  两段的 runner 下取全部与取下一段都是「发一次」，断言分不开。
- **安全阀**：`FETCH_ALL_MAX_SEGMENTS = 100`（本仓差异，DBeaver 无上限）。
  改这个值时同步改 `result-panel.md` §9.54.5。
- **门禁基线（第 71 批后）**：editor lib **814 passed（1 ignored）**（812 → +2）。

### 2.123 「取全部」先确认 + `CTRL+SHIFT+=`（2026-11-22）

- **入口三分，别退回去**：钮与键位只发 `AskFetchAll`，`FetchAll` 只由浮层「继续」发。
  谁会想把钮直接接回 `FetchAll`，先想清楚「点按钮不确认、按键要确认」这个矛盾。
- **判定在宿主**：`ask` 的四道前置（取全部在跑 / 取段在途 / 有执行错误 / 没有下一段）
  读的是 `ResultEntry` 的真值；面板只有 `status.has_more`，别在面板里再造一份。
- **`FetchAll` 分支先收浮层（幂等）再 `fetch_all`**：顺序反了会让浮层在取数期间盖着结果。
- **`fetching_more` 是短结果自动补齐的并发源**：第 71 批只在「取全部 → 取段」方向做了
  互斥，本批补上反向。`fetch_all` 与 `ask_fetch_all` 都必须先看它，否则同一个
  `offset` 会被提交两次（用例里表现为 `asked` 重复 `(2, …)`）。
- **写「取全部」的用例要掐渲染旁路**：2 行 + `has_more` 的结果一重绘就自动取下一段
  （`rendering_a_short_result_asks_for_more_by_itself` 是有意行为）。用例里
  `state.delegate_mut().set_has_more(false)` 只关 delegate，store 仍真，
  于是「该不该弹」与「有没有自动取」分得开。
- **浮层不开输入框就不需要 `&mut Window`**：`open_fetch_all_confirm` / `close_...`
  都是 `&mut Context` 签名，与 SQL 预览一致；别为了对称硬塞窗口参数。
- **门禁基线（第 72 批后）**：editor lib **817 passed（1 ignored）**、
  panel **409 passed**、ui-guard **4 passed**。改到这几处时同步改 `result-panel.md` §9.55.6。

### 2.124 互斥矩阵闭合（2026-11-22，第 73 批）

- **加取数类入口前先查矩阵**：`result-panel.md` §9.56.3 那张表是权威。入口 × 在途状态
  每一格都要么「拒」要么「让出」，别留空。
- **`start_export` 的 `fetching_all` 判在 `is_executing()` 之前**。段与段之间
  `is_executing()` 会短暂为 false（回填那一下），那个窗口放过去就是落半截文件；
  而且提示要说清是「本仓在续段」而不是「通道被占用」。
- **`ask_fetch_all` 的 `pending_export` 判在 `fetching_more` 之前**：导出抓全量也会置
  `fetching_more`，顺序反了提示就变成「正在取下一段」，看不出是谁在取。
- **别用 `export_pending_for_test()` 验 `pending_export`**：它读的是 `export_pending`
  （DuckDB 导出回执计数，`host/format.rs:316-324`）。抓全量的在途证据用取段请求序列
  （测试里的 `asked`）。
- **门禁基线（第 73 批后）**：editor lib **819 passed（1 ignored）**、
  panel **409 passed**、ui-guard **4 passed**。改到这几处时同步改 `result-panel.md` §9.56.4。

### 2.125 `CTRL+R` 撤销未提交变更（2026-11-22，第 74 批）

- **`mutate_change_set` 的返回值不是「有变化」**：它在找到槽位、执行完闭包后一律返回
  `true`（`store/store.rs:522-536`），`ChangeSet::discard_pending` 返回 `()`。
  判断「撤销掉没有」要自己读 `pending_count()`。
- **有条件的钮会掩盖死分支**：`discard_pending_changes` 的 `else` 分支因为钮只在
  `pending_changes > 0` 时摆而永远跑不到，接键位才暴露。以后给「条件渲染的钮」加键位，
  先问一句「无数据时这条路径能不能走到」。
- **回滚不要求 `editable`**：它不写源库，只丢本地改动。`CTRL+S`（提交）才要求。
- **键位只转请求，不带判定**：`request_rollback_pending` 只入队 `RollbackPending`，
  清什么、怎么提示都在 `discard_pending_changes`（钮与键位共用）。
- **DBeaver 的 `undoRedoManager.clear()` 无对应物**：本仓没有结果编辑的 undo 栈。
  别为了「对齐」去造一个。
- **门禁基线（第 74 批后）**：editor lib **820 passed（1 ignored）**、
  panel **409 passed**、ui-guard **4 passed**。改到这几处时同步改 `result-panel.md` §9.57.6。

### 2.126 跳转到列（2026-11-22，第 75 批）

- 键位：`CTRL+SHIFT+G` → `result_panel::grid::GotoColumn`（`plugin.xml:1090`），挂
  `RESULT_GRID_CONTEXT`，生产注册在 `crates/app/src/main.rs`、测试注册在 `bind_editor_keys`。
- 形态：面板内浮层（`goto_column_open` + 常驻 `goto_column_input`），不另开窗口。输入框在
  **构造期**建（`InputState::new` 要 `&mut Window`），打开时回填 placeholder + 交焦点。
- 解析（`ResultPanel::goto_column_target`）：名字（`eq_ignore_ascii_case`，重名取第一个）
  → 1 基序号 `1..=列数` → 否则「未知列名」。命中但被隐藏 → 「已隐藏」。
- 落点：`set_selected_cell(当前行, view_col + 1)` —— 组件列 0 是行号槽；**行保持不动**
  （DBeaver `scrollToRow(CURRENT)`）。`selected_col` 一并写。
- 顺手修：`select_column` / `locate_column` 原来直接喂 `view_col`（少 1），网格列高亮左移一格；
  已改 `view_col + 1`。
- 用例：panel 3 条（解析 / 开-跳-保持行 / 失败不关浮层）；editor 2 条（键位全链 / 无结果提示）。
- 门禁：editor 822、panel 412、ui-guard 4、workspace `Finished`（2 warning 来自通知并行线）。
- 差异：无内容补全、不预填不全选（`InputState` 无 `select_all`）、隐藏列不自动取消隐藏、
  未知列名文案中文化。
- Esc：`InputState::clean_on_escape` 默认 false（`gpui-base/input/base/state.rs:1099`），
  单光标时走 `cx.propagate()`（`:2607`）→ 遮罩的 `on_key_down` 收得到；**无窗口用例**
  （四个浮层同口径都缺）。

### 2.127 选列 / 选行（2026-11-22，第 76 批）

- 键位：`CTRL+ALT+C` / `CTRL+ALT+R` → `result_panel::grid::SelectCurrentColumn` /
  `SelectCurrentRow`（`plugin.xml:1088/1089`；`SpreadsheetCommandHandler:106-131`），挂
  `RESULT_GRID_CONTEXT`，注册在 `crates/app/src/main.rs` + `bind_editor_keys`。
- 落点：单选区的等价表达——选列 = `select_column(data_column(col - 1))`（组件列 0 是行号槽，
  按下去不选、留提示）；选行 = 组件 `set_selected_row(row)` + 状态行 `Some(row + 1)`。
- 为什么选行要写状态行：`row_target_state` 只读 `status.selected_row`，而 `SelectRow`
  事件路径不写 → 只铺色不带状态时，行级三钮 / 邻行复制 / 「修改」钮仍全灰。
- 用例：panel 2 条（列映射 / 行 + 状态行）；editor 3 条（两键全链 / 无结果提示）。
- 门禁（最终实测）：pair `Finished`；panel 414；ui-guard 4；editor lib **825 passed**
  （1 ignored）；workspace `Finished`（2 warning 来自通知线）。检查期曾有 6 条格式化用例红，
  起因是并行线只改了 `FormatOptions` / 引擎、映射未跟上；那条线收尾前已补齐并转绿，
  **本批未提交任何格式化线文件**。
- 记档不实现：`CTRL+BACKSPACE`（`cell.setDefault`）要 `DBDValueDefaultGenerator` + 会话，
  本仓无驱动侧默认值通道。
- 未决：`SelectRow` 事件路径要不要统一写 `status.selected_row`（本批只补键位路径）。

### 2.128 切筛选焦点（2026-11-22，第 77 批）

- 键位：`CTRL+ALT+SHIFT+T` → `result_panel::grid::SwitchFilterFocus`
  （`plugin.xml:1058`；`ResultSetHandlerMain:179-181` → `ResultSetViewer.switchFilterFocus():689-701`），
  **挂 `RESULT_PANEL_CONTEXT`**（上游 context 就是 `...ui.context.resultset`，不是 `.focused`）
  ——若绑网格层，焦点在筛选框时「切回来」那一半按不到。
- 落点：`ResultPanel::switch_filter_focus(window, cx) -> bool` + 公共 `filter_focus_handle(cx)`；
  判据 `window.focused(cx) == Some(filter)`（贴 `isFocusControl()` 的精确语义）；交回走
  `presentation_focus_target`（网格态回网格、文本态回面板根）。
- 收起 / 没有筛选面板 → 返回 `false`，宿主留提示（上游静默 return，本仓按第 69 批口径）。
- 无结果时面板整块不渲染（`host/render.rs:221/241`），键位不命中、焦点不动——与上游
  `enabledWhen` 要求 `resultset.part` 一致。
- 用例：panel 2 条（双向往返 / 收起时不动作）；editor 2 条（键位全链往返 / 无结果 inert）。
- 门禁：pair `Finished` 无 warning；panel **416**（+2）；ui-guard 4；editor lib **831 passed**
  （1 ignored；825 → +6，其中 +2 本批、+4 来自并行线同期补的用例）；workspace `Finished`。
- 记档：`TableState` 没有 `focus_handle` 方法，窗口用例必须 `use gpui_kit::Focusable as _;`。

### 2.129 隐藏 / 显示列（2026-11-22，第 78 批）

- 键位：`ALT+SHIFT+H` → `HideCurrentColumn`、`ALT+SHIFT+T` → `ShowHiddenColumns`
  （`plugin.xml:1096/1097`；`SpreadsheetCommandHandler:174-194` / `:195-212`），
  挂 `RESULT_PANEL_CONTEXT`（上游 contextId 是整块 `...ui.context.resultset`）。
- 落点：`hide_current_column(window, cx) -> Result<(), String>`（当前列 = `selection_pos` 的组件列
  换算数据列；拒绝：无当前格 / 行号槽 / 已隐藏 / 只剩一列）、`show_hidden_columns(window, cx) -> bool`
  （`hidden_count() == 0` → false）。
- 关键点：藏 / 显之后把当前格**按同一视觉列位重投影**（`set_selection_pos`），列位越界则清
  `selection_pos`（右栏 `selected_col` 按「清选保持清单当前选中」保留，第 79 批实测见 §2.130）。
- 有意差异：无「默认可见列」概念（`show_all_columns` 全恢复等价）、不允许全隐（留 1 列）、
  ERROR 框 → 状态栏一句话、一次只藏一列。
- 用例：panel 2 条（三列场景藏中间列 + 重投影 / 只剩一列被拒）；editor 1 条（提示 → 藏 → 放回）。
- 门禁：pair `Finished` 无 warning；panel **418**（+2）；ui-guard 4；editor lib **832 passed**
  （1 ignored）；workspace `Finished`（2 warning 来自通知线）。
- 记档：用例初版用两列 harness 断言「停在原列位」，实际走的是越界清选分支——改成三列场景，
  代码未改。

### 2.130 复制列名（2026-11-22，第 79 批）

- 键位：`ALT+SHIFT+C` → `CopyColumnNames`（`plugin.xml:1095`；命令定义 `:313`、handler `:439`、
  实现 `ResultSetHandlerMain:333-375`），挂 `RESULT_PANEL_CONTEXT`（上游 contextId 整块
  `...ui.context.resultset`）。
- 落点：`ResultPanel::copy_column_names(cx) -> Result<usize, String>`（`panel/events.rs`）——
  判据两级：当前格所在列（`data_column_checked` 校验仍可见）→ 全部可见列（数据序、跳过隐藏）；
  `\t` 拼接，空则 `Err`。宿主 `on_copy_column_names` 给回执（「已复制列名」/「已复制 N 个列名（TSV）」）。
- 关键取舍：**右栏 `selected_col` 不参与判据**。上游只认表格选区；而本仓「清选保持清单当前选中」
  会让 `selected_col` 停在隐藏列上（实测 `(None, Some(1))`），读它会把隐藏列名拷出来。
- 有意差异：不弹「复制配置」对话框（固定 `\t`）、不加标识符引号（无方言通道）、
  空列表 → 状态栏一句话（上游空串照拷）、复制完留回执（上游静默）。
- 用例：panel 2 条（两级判据 + 隐藏列跳过 / 空列被拒）、editor 1 条（选中列 → 藏列 → 全量）；
  另补第 78 批越界分支 1 条（藏最右列 → 清 `selection_pos`）。
- 门禁：pair `Finished` 无 warning；panel **421**（+3）；ui-guard 4；editor lib **833 passed**
  （1 ignored）；workspace `Finished`（2 warning 来自通知线）。
- 记档：`ALT+SHIFT+C` 原本在 §9.61.9 遗留清单里；本批接上，§9.61.9 与 §9.61.4 的措辞一并按实测收紧。

### 2.131 筛选设置键位（2026-11-22，第 80 批）

- 键位：`ALT+SHIFT+F` → `OpenFilterSettings`（`plugin.xml:1094`；命令 `:333`、handler `:570`、
  实现 `ResultSetHandlerMain:546-549` → `ResultSetViewer.showFilterSettingsDialog()` `:644-646`
  = 直接开对话框、不判条件），挂 `RESULT_PANEL_CONTEXT`。
- 落点：`actions!` 增 `OpenFilterSettings`；宿主 `on_open_filter_settings` **直接调**
  `panel.open_filter_settings(window, cx)`（不入队列；队列那条留给工具栏 / 右键那三个鼠标入口）。
  不留 message（打开本身是可见反馈）。
- 实测缺口（结构性，非本批引入）：焦点在浮层的排序输入框里按 `ESC` 关不掉浮层 ——
  `escape` 先命中输入框 binding（`gpui-base/src/input/base/state.rs:277`），`clean_on_escape == false`
  时走 `cx.propagate()`（`:2607`）只往上找**上层 binding**，而浮层根接的是 `on_key_down`
  （`panel/dialog.rs:465-469`），接不到。修法 = 浮层遮罩挂 `key_context` + app 注册 escape binding，
  留待单独一批；现状已钉进用例。
- 用例：editor 1 条（打开 → ESC 仍在 → 点 ✕ 收起）。
- 门禁：pair `Finished` 无 warning；panel **421**（不变）；ui-guard 4；editor lib **834 passed**
  （1 ignored）；`check --workspace --all-targets` **被并行线阻塞**（未跟踪的
  `crates/workbench/src/components/version_dialog.rs` 有 3 处编译错：缺 `ActiveTheme` 导入、
  `Div::v_flex` ×2）——本批改动 crate 由前三条覆盖，待并行线修好后复查整包。

### 2.132 浮层 `ESC` 收起（2026-11-22，第 81 批）

- 入口**不是键位**：`ResultPanel::new` 里 `cx.intercept_keystrokes`（app 级，binding 之前跑，
  与焦点无关）。实测依据是 `Window::context_stack()`：焦点在网格 =
  `[Root, editor, editor-result-panel, editor-result-grid, DataTable]`，焦点在浮层 `InputState` =
  **`[]`**（空栈 ⇒ 带 context 的 binding 全不匹配，连 `CTRL+SHIFT+G` 都按不出来）。
  因此「给遮罩挂 context」「挂面板层 binding」「全局 binding」三条都被否掉（全局那条会截走
  `grid/edit.rs` 的内联编辑取消）。
- `dismiss_overlay(window, cx)`：按叠放次序复用 `close_*`，**关完 `window.focus(&target)` 交回
  `presentation_focus_target`**（不交回则焦点停在已消失的输入框，后续面板键位全废）；
  interceptor 只在返回 `true` 时 `cx.stop_propagation()`，无浮层时零影响。
- 删掉的东西：四个遮罩的 `on_key_down`（从未生效的旧实现）、`actions!::DismissOverlay`、
  `ResultRequest::DismissOverlay`、面板层 `escape` 键位（`crates/app` 与测试各一条）、
  面板根 `.on_action`、`RESULT_OVERLAY_CONTEXT`（中途方案，已撤）。
- 用例：`escape_dismisses_every_result_overlay`（四段：筛选设置 / 跳列 / SQL 预览 / 取全部确认）；
  第 80 批 `alt_shift_f_opens_the_filter_settings_overlay` 的 ESC 断言改成「应收起」。
- 门禁：pair `Finished` 无 warning；panel **421**；editor lib **835 passed**（1 ignored）；
  ui-guard 3/4（失败项报的是并行线未跟踪的 `version_dialog.rs` 裸 px）；整包被并行线阻塞
  （`version_dialog.rs` 3 处编译错 + `crates/settings/src/registry/entries/projects.rs:49`
  unclosed delimiter）。
- 遗留：三条鼠标 ✕ 出口（`CloseFilterSettings` / `CloseSqlDialog` / `CloseFetchAllConfirm`）
  没有 window、不交回焦点；「输入框焦点下所有 context binding 失效」属框架层现象，
  本批只接了 `ESC` 一条。

### 2.133 面板总开关 `F7` / `CTRL+7`（2026-11-22，第 82 批）

- 上游：`plugin.xml:1084/1085`（`CTRL+7` / `F7` → `...grid.togglePreview`，contextId 为整块
  `...ui.context.resultset`）→ `ResultSetHandlerMain:167-169`
  `showPanels(!isPanelsVisible(), true, true)` → `ResultSetViewer:1478-1518`。
- 落点：`actions!` 增 `TogglePanels`；宿主 `on_toggle_panels` 只做
  `set_panels_visible(!panels_visible(), cx)`（判据、补默认页、复位最大化都在
  `events.rs::set_panels_visible` 里）；键位 `f7` / `ctrl-7` 挂 `RESULT_PANEL_CONTEXT`。
- 与既有两条鼠标入口同状态：状态栏右端三态循环、竖条顶部 `panelsButton`
  （`SetPanelsVisible(!panels_visible)`）。`F7` 是二态，不复制三态。
- 有意差异：**不落盘**（上游 `saveSettings=true`；本仓无 `panelsVisible` 偏好项，
  且 `crates/settings` 在并行线手里）；**`CTRL+SHIFT+7`（`activatePreview`）不接**
  （右栏六页无 focus handle，焦点交接没有回退位）。
- 用例：`f7_and_ctrl_7_toggle_the_result_panels`（真态 + `editor-result-right-tabs` 存在性）。
- 门禁：4 条 `check`（panel lib / editor lib / panel all-targets / editor tests）全 `Finished`
  且 0 warning；editor **836 passed**（1 ignored，+1）；panel **421**；ui-guard 3/4
  （并行线 `version_dialog.rs` 裸 px，老账）；`check --workspace --all-targets` **Finished**
  （6m46s，0 error；6 条 warning 均在并行线 crate）。
- 环境事故记录：并行线同时编译共享 `target` 时 rustc 曾 `STATUS_STACK_BUFFER_OVERRUN`，
  截断 `libserde-*.rmeta` 导致后续全部 `E0786`，`cargo clean -p serde` 后恢复。
- 遗留：`CTRL+SHIFT+7`、面板可见性不跨会话。

### 2.134 右栏单页开关 `CTRL+ALT+F2..F6`（2026-11-22，第 83 批）

- 上游：`plugin.xml:1099-1113` 五条 `...grid.togglePanel` + `panelId`（`value-view` /
  `results-references` / `results-metadata` / `results-grouping` / `column-aggregate`；
  contextId 整块 `...ui.context.resultset`、scheme 键盘专用表）→ 命令定义 `:280-287`、
  handler 绑定 `:416-418`、实现 `handler/ResultSetHandlerTogglePanel.java:46-81`
  （`isPanelsVisible() && isPanelVisible(id)` 为真 → `closePanel(id)`，否则
  `activatePanel(id, true, true)`）。
- 落点：`actions!` 增五条零参 action（`ToggleValuePanel` / `ToggleReferencePanel` /
  `ToggleBrowserPanel` / `ToggleGroupPanel` / `ToggleStatsPanel`）；宿主共用段
  `dispatch_toggle_panel` 一律转 `ResultRequest::ToggleRightTab(tab)`（不新建面板方法：
  `ResultPanel::toggle_right_tab` 与 `dispatch.rs:445-461` 早已完整接线，含页集合落盘与
  引用页读外键）；键位 `ctrl-alt-f2..f6` 挂 `RESULT_PANEL_CONTEXT`。
- 顺带：`events.rs:750` 新增 `pub fn right_tab()` 只读访问器（`right_tab` 字段是 `pub(crate)`，
  用例断言「切到该页」需要它）。
- 与上游的差异：**不把焦点交给面板控件**（`activatePanel` 的 `panel.setFocus()`）——
  右栏六页无 `FocusHandle`，与第 82 批 `CTRL+SHIFT+7` 同一卡点；`CTRL+SHIFT+F2..F6`
  （`activatePanel` 那组）同理不接；`RightTab::ExecSql` 是自有页，不接键位；
  `results-metadata` 映射到 `Browser`（`ports.rs:846` / `events.rs:3002` 都这样记）。
- 顺带修两处既有问题：① `persist_result_open_tabs`（`dispatch.rs:762`）在缺 `Settings`
  全局时 panic（headless/用例直接崩在 `gpui-fast app.rs:2657`），加 `has_global` 守卫
  （`dispatch.rs:769`），同时避免用例真写 `settings.json`；② `notebook/orchestrate.rs:243`
  测试里的 `let applied =` 未使用（`check --all-targets` 唯一一条本 crate warning），
  改 `let _ =`。
- 用例：`ctrl_alt_f2_to_f6_toggle_the_right_panels`（关该页 / 再开并切过去 / 关当前页落到
  规范序下一页 / 五个页逐页核对 / 引用页开关 / `f7` 收起后按单页键 → 展开+启用+切换）。
- 门禁：三 crate `--all-targets` **0 error**；editor **837 passed**（1 ignored，+1）；
  panel **421**；ui-guard 3/4（并行线 `version_dialog.rs` 裸 px，老账）；
  `check --workspace --all-targets` **Finished** 0 error（23m24s，前半程等并行线释放
  `target` 锁）。
- 遗留：焦点交给面板、`CTRL+SHIFT+F2..F6` —— 都等右栏焦点体系。

## 3. 并行线协作纪律（工作区现状）

- **共享文件的提交姿势**（第 77 批踩过，务必照做）：`git commit --only <paths>` /
  `git commit -F msg -- <paths>` 提交的是**工作区内容**，会绕过刚用 `hash-object -w` +
  `update-index --cacheinfo` 摘好的那个 blob。所以顺序必须是：摘 blob →
  `update-index --cacheinfo` → `git add` 本线其余文件 → **不带 paths 的** `git commit -F msg`。
  第 77 批第一次提交用了 `--only`，把 `crates/app/src/main.rs` 里的通知线 3 个 hunk 一起带进了
  本线提交（25 行而不是 11 行）；发现后用 `git reset --soft HEAD~2` + `git reset` 重做，
  工作区未受影响、通知线改动仍在未提交状态。
- 只 add 本线文件；`Cargo.lock` / vendor patch / `crates/insight/*` / `plugin/*` / `crates/app/src/main.rs` / `workbench/src/components/insight_actions.rs` / README 等并行线文件一律不随本线提交。
- 当前未提交（并行线在动，改前先 `git status` 确认归属）：
  - 本线混合：`result_panel/src/{store/, export/, grid/, panel/}`（2026-10-29 拆分目录）/ `ports.rs` / `host.rs` / `settings/*` / `workbench/src/view.rs` / `panels/mod.rs` / `tests/ui_contract.rs` / `workbench_shell/src/ui.rs`
  - 并行线：`database/src/nav_view/rows.rs`、`editor/src/{session.rs, view/dialogs.rs, encoding_dialog.rs, function_manual.rs, indent_dialog.rs, invisible_dialog.rs, tests.rs}`、`engine/src/persistence/workbench_context_store.rs`、`scratchpad/chrome.rs`、`workbench/src/{notification.rs, panels/editor.rs, quick_open/delegate.rs, services/editor_session.rs}`、`workbench_shell/src/ui.rs`、`.agents/skills/rds-design/`、`docs/architecture/{settings/settings-architecture.md, ui/design-spec.md, ui/theme-spec.md}` 等
- 注释 / commit 信息一律简体中文（`feat:` / `fix:` / `refactor:` 前缀）。
- **commit 消息文件必须无 BOM**（第 82 批踩到；历史批次标题前的 `\ufeff` 同源）：
  `Set-Content -Encoding utf8` 在这台机器上写出的仍是**带 BOM** 的 UTF-8，
  `git commit -F <file>` 会把 `\ufeff` 直接吃进标题的第一个字符。一律用 python
  `io.open(path, "w", encoding="utf-8", newline="\n")` 写消息文件；提交后
  `git log -1 --format=%B` 的首字节断言 `!= b"\xef\xbb\xbf"`。
- 过程产物（日志 / 临时脚本）落 `.rds/tmp/` 或 `tools/_archive/`，不入库、不留工作目录。
- 未确定的 API / 设计决策先查 vendor（`vendor/gpui-component/src/`、`vendor/gpui-base/src/`，0.8.x 权威）或本地 MCP，禁止联网 / 凭记忆猜；不确定的交互口径先列出来问用户。

---

## 4. 接手指引（新会话建议顺序）

1. 先重跑 §1.3 四条验证命令，确认基线（尤其 **267 passed / 0 failed**）落地。
2. 处理 §2.1 提交待办：先 `git status` / `git diff` 摘本线 hunk，与并行线协调后统一提交类型样式渲染消费（settings 三文件 + grid.rs + host.rs + workbench view.rs）。
3. 知悉 §2.2 外线阻塞（settings 契约测试）与 §1.3 契约测试既有失败，不替并行线补。
4. 按 §2.4 优先级推进功能遗留（P0（注释与文档漂移）/ P1（分页·行号跳转·列宽落盘）/ P2（hex 翻页·图片预览·列浏览器多选）已交付；剩余见 §2.4 表，开工前先按模块惯例说理解、列方案）。
5. 每完成一批：跑 §1.3 基线 + 记录到 `result-panel.md`（新增 §8.2x 迭代小节），提交只 add 本线文件。

## 5. 常用锚点（行号随近期提交微移，以现场 grep 为准）

- `panel/`：`mod.rs`（门面 + pub 面 re-export：`GridPayload` / `ExportCtx` / `ResultSyncInput` / `ResultPanel` / `AnalysisItem`）；`render.rs` 主 `render` 组装；`sidebars.rs` `render_vertical_bar`（左缘竖条：网格/文本/记录/上一行/下一行）/ `render_right_edge_bar`（右缘竖条，页签图标区）/ `render_right_sidebar`（横 tab 只显激活页签）/ `render_browser_tab` / `render_filter_expand_bar`（展开条：左=展开右=运行无边框）+ `sidebar_width_anim` / `next_right_edge_state` / `clamp_right_sidebar_width` / `clamp_right_sidebar_height` 纯函数 + **R5** `SidebarResizeDrag` / `render_right_sidebar_resize`（`resize_handle` 独立手柄，id `editor-result-right-sidebar-resize`，横排 Horizontal / 竖排 Vertical）；`tabs.rs` `render_value_tab`（值页工具条右段 = 位置标记 + 复制 + **保存值** `editor-result-value-save`）/ `render_stats_tab` / `render_group_tab` / `render_reference_tab`（backlog 空态）/ `render_exec_sql_tab` + **保存值** `value_payload(kind, value, row, col)`（纯函数，形态→字节+默认名）/ `ResultPanel::selected_value_payload` + `kv_row` / `fmt_stat` / `right_tab_from_index` + `panel_toolbar`（28px 页内工具条公用件，**`pub(crate)`**）/ **§2.40** `empty_page`（「工具条 + 空态正文」整页骨架）；`dialog.rs` `render_sql_dialog`（SQL 预览弹窗浮层）；`events.rs` `toggle_value_sidebar`（三态）/ `resize_right_sidebar`（拖拽改宽度 / 竖排改高度）/ **R5** `toggle_vertical_layout`（方向切换，清拖拽标记）/ `end_right_sidebar_resize` / `set_right_sidebar_width_px`/ `toggle_transpose` / `open_sql_dialog` / `close_sql_dialog`（事件路径，需 window）+ 分组页 `toggle_group_column` / `set_group_sort` / `set_group_duplicates_only` / **§2.39** `clear_group_columns` + `reorder_by` 纯函数；`lifecycle.rs` `new` / `sync`（投影同步）；`tests.rs` 尾部 `rendered_ui_elements_survive`（渲染断言防回退，见 §2.10）。
- `ui.rs`：`RESULT_VERTICAL_BAR_WIDTH`（2.25）/ `RESULT_SIDEBAR_RIGHT_WIDTH`（20.0）+ **R5** `RESULT_SIDEBAR_RIGHT_MIN_WIDTH`（12.0 = 192px）/ `RESULT_SIDEBAR_RIGHT_MAX_WIDTH`（40.0 = 640px，均 f32，避裸 `px(`）/ **R5 方向切换** `RESULT_SIDEBAR_RIGHT_HEIGHT`（15.0 = 240px）/ `RESULT_SIDEBAR_RIGHT_MIN_HEIGHT`（8.0 = 128px）/ `RESULT_SIDEBAR_RIGHT_MAX_HEIGHT`（32.0 = 512px）/ `RESULT_STATUS_BAR_HEIGHT`（1.5）/ `RESULT_TOOLBAR_HEIGHT`（**1.875 = 30px，比标签条 2.0 矮一档**，对齐 DBeaver `3+2+20+2+3`）/ `RESULT_PANEL_TOOLBAR_HEIGHT`（**1.75 = 28px，右栏页内工具条，必须等于 vendor `toolbar.rs:242-259` XSmall 档的 `min_h_7`**）/ `RESULT_TOOLBAR_PADDING`（`px(3.)`，水平内距，对齐 DBeaver `marginWidth = 3`）/ **§2.41** `RESULT_FIT_CELL_PADDING`（12px，单元格左右内距合计 6+6）/ `RESULT_CELL_VERTICAL_PADDING`（`px(2.)`，列级 padding 的上下两份，沿用组件 XSmall 档原值）/ **§2.43** `RESULT_FIT_HEADER_ICON`（`px(12.)`，表头筛选漏斗图标宽）+ `RESULT_FIT_HEADER_CHROME`（`px(20.)`，组件排序图标槽 16 + 右侧偏移 4，自适应宽度必须计入）。
- `ports.rs`：`ResultRequest` 全集（含 #7 / §2.9 新增变体 `RunFilterAsQuery` / `ToggleFilterExpand` / `OpenSqlDialog` / `CloseSqlDialog`；`ToggleSqlPreview` 已删）+ **§2.37** `SaveValue` 变体 / `ValuePayload { bytes, default_name }` + **§2.38/§2.39** 分组变体 `ToggleGroupColumn` / `SetGroupSort` / `SetGroupDuplicatesOnly` / `ClearGroupColumns` + `ResultActionQueue`（`request` / `set_panel` / `push` / `drain`）。
- `grid/`：`delegate.rs` `ResultGridDelegate`（TableDelegate 主体实现 + 钩子类型 + RESULT_GRID_CONTEXT 等常量）；`cell.rs` `render_td`（**§2.41/§2.42** 单元格自带 6px 内距 + `.border_l(HAIRLINE)` 画列分隔线；行号槽不画线）/ `header.rs` `render_th`（**§2.41/§2.42** 同 6px 内距、同一左边界线基准，首列跳过；**§2.43** 不再自绘排序箭头，筛选指示为 `IconName::Funnel`）/ `fit.rs` `fit_column` + `header_width`（**§2.43** 表头宽计入组件排序槽 `RESULT_FIT_HEADER_CHROME` 20px；`measure` 为 `pub(crate)` 供用例按同口径量列名）/ `table_impl.rs` `cell_paddings`（列级 padding：上下 2px、左右 0，是竖线能落在列边界的前提）/ `menu.rs` `context_menu`（trait 方法不可分块 → 顶层自由函数 + `table_impl.rs` 薄转发）；`status.rs` `render_status_row`（三项新签名）/ `ResultToolbar` / `ResultStatus` / `ResultControls` / `ResultPane`；`text.rs` `format_cell_text` / `column_values`（按视图行序取列，跳过本地行与被删行）/ **§2.38** `column_group_keys`（原生数值分组键，回退口径必须与 `column_values` 一致）/ `group_by_for` / `group_by_columns_for`（多列逐列取原生键）/ `view_rows.rs` `text_presentation` 指纹缓存 / `stats.rs` 列统计 + `group_by_values` / `group_by_tuples` / `number_group_key` / `edit.rs` 行编辑 / `tests.rs`（全量测试迁入）。
- `store/`：`store.rs` `ResultStore`（按文档分槽，多结果集 sets(document) + TabBar 标签；事实档 A2 未接线）+ `entry.rs` / `fact.rs`。
- `export/`：`mod.rs` 门面 + `formats.rs` / `encode.rs` / `table_name.rs`（editor 侧 `pub use result_panel::export::*` 通配，全 pub 面保持）。

## 6. 文件地图（crates/result_panel/src，2026-10-29 目录拆分后）

顶层单文件：`lib.rs` / `ports.rs`（请求 + 队列）/ `rowset.rs` / `sets.rs`（结果集标签条）/ `status.rs` / `ui.rs`（尺寸常量）/ `analysis.rs` / `changes.rs` / `column_browser.rs`（列浏览器 List 委托）/ `error_card.rs` / `script_card.rs`。

拆分目录（2026-10-29 一批完成，原巨型单文件按域切分；**trait impl 不可分块**处统一「顶层自由函数 + impl 薄转发」、跨文件私有方法 / struct 字段以 `pub(crate)` 暴露；`panel.rs` 无标准 `Render` trait impl（自定义 `render(&self, cx)`，host 手动调）故普通 impl 可切多块）：
- `store/`（原 `store.rs`）：`mod.rs` + `entry.rs` + `fact.rs` + `store.rs`
- `export/`（原 `export.rs`）：`mod.rs` + `formats.rs` + `encode.rs` + `table_name.rs`
- `grid/`（原 `grid.rs`，6556 行）：`mod.rs` + `delegate.rs` + `view_rows.rs` + `edit.rs` + `text.rs` + `menu.rs` + `stats.rs` + `cell.rs` + `header.rs` + `table_impl.rs` + `status.rs` + `tests.rs`
- `panel/`（原 `panel.rs`，3338 行）：`mod.rs` + `data.rs` + `lifecycle.rs` + `render.rs` + `events.rs` + `sidebars.rs` + `dialog.rs` + `tabs.rs` + `tests.rs`
