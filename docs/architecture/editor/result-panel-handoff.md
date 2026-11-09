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
cargo test -p rds-result-panel --lib    # 基线：259 passed / 0 failed（含 rendered_ui_elements_survive 防回退）
cargo test -p rds-settings --lib        # 基线：35 passed / 0 failed
cargo test -p rds-ui-guard              # 基线：4 passed / 0 failed（含 no_new_hardcoded_color_or_pixel）
cargo test -p rds-workbench-shell --lib # 基线：14 passed / 0 failed
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

**仍未做**：值详情 / 分组 / 引用 / 执行SQL 四页工具条；统计项增删跨会话持久化；
R3 / R4 / R5。

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

1. 先重跑 §1.3 四条验证命令，确认基线（尤其 **259 passed / 0 failed**）落地。
2. 处理 §2.1 提交待办：先 `git status` / `git diff` 摘本线 hunk，与并行线协调后统一提交类型样式渲染消费（settings 三文件 + grid.rs + host.rs + workbench view.rs）。
3. 知悉 §2.2 外线阻塞（settings 契约测试）与 §1.3 契约测试既有失败，不替并行线补。
4. 按 §2.4 优先级推进功能遗留（P0（注释与文档漂移）/ P1（分页·行号跳转·列宽落盘）/ P2（hex 翻页·图片预览·列浏览器多选）已交付；剩余见 §2.4 表，开工前先按模块惯例说理解、列方案）。
5. 每完成一批：跑 §1.3 基线 + 记录到 `result-panel.md`（新增 §8.2x 迭代小节），提交只 add 本线文件。

## 5. 常用锚点（行号随近期提交微移，以现场 grep 为准）

- `panel/`：`mod.rs`（门面 + pub 面 re-export：`GridPayload` / `ExportCtx` / `ResultSyncInput` / `ResultPanel` / `AnalysisItem`）；`render.rs` 主 `render` 组装；`sidebars.rs` `render_vertical_bar`（左缘竖条：网格/文本/记录/上一行/下一行）/ `render_right_edge_bar`（右缘竖条，页签图标区）/ `render_right_sidebar`（横 tab 只显激活页签）/ `render_browser_tab` / `render_filter_expand_bar`（展开条：左=展开右=运行无边框）+ `sidebar_width_anim` / `next_right_edge_state` / `clamp_right_sidebar_width` / `clamp_right_sidebar_height` 纯函数 + **R5** `SidebarResizeDrag` / `render_right_sidebar_resize`（`resize_handle` 独立手柄，id `editor-result-right-sidebar-resize`，横排 Horizontal / 竖排 Vertical）；`tabs.rs` `render_value_tab`（值页工具条右段 = 位置标记 + 复制 + **保存值** `editor-result-value-save`）/ `render_stats_tab` / `render_group_tab` / `render_reference_tab`（backlog 空态）/ `render_exec_sql_tab` + **保存值** `value_payload(kind, value, row, col)`（纯函数，形态→字节+默认名）/ `ResultPanel::selected_value_payload` + `kv_row` / `fmt_stat` / `right_tab_from_index` + `panel_toolbar`（28px 页内工具条公用件）；`dialog.rs` `render_sql_dialog`（SQL 预览弹窗浮层）；`events.rs` `toggle_value_sidebar`（三态）/ `resize_right_sidebar`（拖拽改宽度 / 竖排改高度）/ **R5** `toggle_vertical_layout`（方向切换，清拖拽标记）/ `end_right_sidebar_resize` / `set_right_sidebar_width_px`/ `toggle_transpose` / `open_sql_dialog` / `close_sql_dialog`（事件路径，需 window）+ 分组页 `toggle_group_column` / `set_group_sort` / `set_group_duplicates_only` / **§2.39** `clear_group_columns` + `reorder_by` 纯函数；`lifecycle.rs` `new` / `sync`（投影同步）；`tests.rs` 尾部 `rendered_ui_elements_survive`（渲染断言防回退，见 §2.10）。
- `ui.rs`：`RESULT_VERTICAL_BAR_WIDTH`（2.25）/ `RESULT_SIDEBAR_RIGHT_WIDTH`（20.0）+ **R5** `RESULT_SIDEBAR_RIGHT_MIN_WIDTH`（12.0 = 192px）/ `RESULT_SIDEBAR_RIGHT_MAX_WIDTH`（40.0 = 640px，均 f32，避裸 `px(`）/ **R5 方向切换** `RESULT_SIDEBAR_RIGHT_HEIGHT`（15.0 = 240px）/ `RESULT_SIDEBAR_RIGHT_MIN_HEIGHT`（8.0 = 128px）/ `RESULT_SIDEBAR_RIGHT_MAX_HEIGHT`（32.0 = 512px）/ `RESULT_STATUS_BAR_HEIGHT`（1.5）/ `RESULT_TOOLBAR_HEIGHT`（**1.875 = 30px，比标签条 2.0 矮一档**，对齐 DBeaver `3+2+20+2+3`）/ `RESULT_PANEL_TOOLBAR_HEIGHT`（**1.75 = 28px，右栏页内工具条，必须等于 vendor `toolbar.rs:242-259` XSmall 档的 `min_h_7`**）/ `RESULT_TOOLBAR_PADDING`（`px(3.)`，水平内距，对齐 DBeaver `marginWidth = 3`）。
- `ports.rs`：`ResultRequest` 全集（含 #7 / §2.9 新增变体 `RunFilterAsQuery` / `ToggleFilterExpand` / `OpenSqlDialog` / `CloseSqlDialog`；`ToggleSqlPreview` 已删）+ **§2.37** `SaveValue` 变体 / `ValuePayload { bytes, default_name }` + **§2.38/§2.39** 分组变体 `ToggleGroupColumn` / `SetGroupSort` / `SetGroupDuplicatesOnly` / `ClearGroupColumns` + `ResultActionQueue`（`request` / `set_panel` / `push` / `drain`）。
- `grid/`：`delegate.rs` `ResultGridDelegate`（TableDelegate 主体实现 + 钩子类型 + RESULT_GRID_CONTEXT 等常量）；`cell.rs` `render_td` / `header.rs` `render_th` / `menu.rs` `context_menu`（trait 方法不可分块 → 顶层自由函数 + `table_impl.rs` 薄转发）；`status.rs` `render_status_row`（三项新签名）/ `ResultToolbar` / `ResultStatus` / `ResultControls` / `ResultPane`；`text.rs` `format_cell_text` / `column_values`（按视图行序取列，跳过本地行与被删行）/ **§2.38** `column_group_keys`（原生数值分组键，回退口径必须与 `column_values` 一致）/ `group_by_for` / `group_by_columns_for`（多列逐列取原生键）/ `view_rows.rs` `text_presentation` 指纹缓存 / `stats.rs` 列统计 + `group_by_values` / `group_by_tuples` / `number_group_key` / `edit.rs` 行编辑 / `tests.rs`（全量测试迁入）。
- `store/`：`store.rs` `ResultStore`（按文档分槽，多结果集 sets(document) + TabBar 标签；事实档 A2 未接线）+ `entry.rs` / `fact.rs`。
- `export/`：`mod.rs` 门面 + `formats.rs` / `encode.rs` / `table_name.rs`（editor 侧 `pub use result_panel::export::*` 通配，全 pub 面保持）。

## 6. 文件地图（crates/result_panel/src，2026-10-29 目录拆分后）

顶层单文件：`lib.rs` / `ports.rs`（请求 + 队列）/ `rowset.rs` / `sets.rs`（结果集标签条）/ `status.rs` / `ui.rs`（尺寸常量）/ `analysis.rs` / `changes.rs` / `column_browser.rs`（列浏览器 List 委托）/ `error_card.rs` / `script_card.rs`。

拆分目录（2026-10-29 一批完成，原巨型单文件按域切分；**trait impl 不可分块**处统一「顶层自由函数 + impl 薄转发」、跨文件私有方法 / struct 字段以 `pub(crate)` 暴露；`panel.rs` 无标准 `Render` trait impl（自定义 `render(&self, cx)`，host 手动调）故普通 impl 可切多块）：
- `store/`（原 `store.rs`）：`mod.rs` + `entry.rs` + `fact.rs` + `store.rs`
- `export/`（原 `export.rs`）：`mod.rs` + `formats.rs` + `encode.rs` + `table_name.rs`
- `grid/`（原 `grid.rs`，6556 行）：`mod.rs` + `delegate.rs` + `view_rows.rs` + `edit.rs` + `text.rs` + `menu.rs` + `stats.rs` + `cell.rs` + `header.rs` + `table_impl.rs` + `status.rs` + `tests.rs`
- `panel/`（原 `panel.rs`，3338 行）：`mod.rs` + `data.rs` + `lifecycle.rs` + `render.rs` + `events.rs` + `sidebars.rs` + `dialog.rs` + `tabs.rs` + `tests.rs`
