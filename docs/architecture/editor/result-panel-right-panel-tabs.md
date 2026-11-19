# 结果集右栏页签模型：对齐 DBeaver `CTabFolder`

> 状态：**方案 B 两步均已实施**（2026-11-19：第 1 步 `open_tabs` 集合 + 逐页关闭 + 视图菜单
> 勾选态；第 2 步删掉「点当前页签收起整栏」）。实施记录见 `result-panel.md` §9.08 / §9.09、
> `result-panel-handoff.md` §2.97 / §2.98。本文保留为模型与竞品取证的依据。
> 范围：`crates/result_panel/src/panel/{data.rs,events.rs,tabs.rs,sidebars.rs,tests.rs}`，
> 以及 `crates/editor/src/view/host/results/{dispatch.rs,mod.rs}` 的分派。
> 竞品取证目录：`D:\RdataStation\RDS\dbeaver-ref`（`org.jkiss.dbeaver.ui.editors.data`）。

## 1. 竞品事实（逐行取证，勿凭印象）

1. **每个面板页签带关闭按钮**：`ResultSetViewer.addPanel:1330-1394` 里
   `CTabItem panelTab = new CTabItem(panelFolder, SWT.CLOSE);`（`:1357`）。
   同一段还给每个页装两段工具条：面板自己贡献的（`panel.contributeActions(...)`，`:1373-1376`，
   `SWT.RIGHT`）与默认面板动作（`addDefaultPanelActions(...)`，`:1379-1383`）。
   `panel.needsSeparator()` 决定页内容上沿有没有分隔线（`:1345-1347`）。
2. **已启用面板是一个集合，并且会持久化**：`:1388` `presentationSettings.enabledPanelIds.add(id)`；
   `activateDefaultPanels:1396-1400` 会先 `removeIf(CommonUtils::isEmpty)` 再补默认面板。
   ⇒ 「关掉某个页」在这边是**从集合里移除该 id**，不是「隐藏整个右栏」。
3. **视图菜单逐页是勾选项**：`fillPanelsMenu:1559-1588` 对 `availablePanels` 逐个产出
   `CommandContributionItem`（`CMD_TOGGLE_PANEL` + `PARAM_PANEL_ID`，`STYLE_CHECK`），
   之后才是 `Separator`、`CMD_TOGGLE_MAXIMIZE`、`CMD_ACTIVATE_PANELS`，
   再按 `FEATURE_PANELS` 追加 `CMD_TOGGLE_PANELS` / `CMD_TOGGLE_LAYOUT`。
   ⇒ 勾选状态 = 该面板在不在 `enabledPanelIds` 里，与页签的关闭按钮**同一个状态**。
4. **同一时刻只显示一个页**：`CTabFolder` 语义（`panelFolder.setSelection(panelTab)`，`:1366`），
   其余页只是页签条上的条目，内容控件不显示。
5. **整栏的显隐是另一件事**：`isPanelsVisible:1473-1476` / `showPanels:1478-1518` /
   `togglePanelsMaximize:1551-1557`（`viewerSash.hideUp()/showUp()`）。
   已单独对齐过（§8.94 面板最大化 ↔ 总开关互斥、§8.95 竖条/视图菜单的面板项语义）。

## 2. 本仓现状

> 下表记的是**方案定稿时**的现状；第 1 步已落地（差异见 §5），实际状态以
> `result-panel.md` §9.08 为准。

| 项 | 现状 |
| --- | --- |
| 页集合 | 固定六页（`events::right_tabs()` 顺序：列浏览器 / 值详情 / 计算 / 分组 / 引用 / 执行SQL），无「关掉某页」的概念 |
| 当前页 | `panel/data.rs` 的 `right_tab: RightTab`（单值），`set_right_tab` / `toggle_right_tab` |
| 页签条 | `TabBar::segmented()`，六个页签恒定出现 |
| 关闭语义 | **点当前页签 = 收起整条右栏**（`events::toggle_right_tab` + 用例 `toggling_the_active_page_collapses_the_panel`） |
| 视图菜单 | 六页各一项 + 「面板最大化 / 收起全部面板」（用例 `right_panel_view_menu_lists_pages_then_the_two_toggles`），第 10 批已改成开/关语义 |
| 持久化 | `right_tab` 不落盘（重开回到列浏览器） |

## 3. 差异清单（用户可见）

1. DBeaver 有**每页 × 关闭**；本仓只能靠「点当前页签」收起整个右栏，无法只关掉一页。
2. DBeaver 关掉一页后**其余页仍在页签条里**，可以继续切；本仓没有这个概念。
3. DBeaver 已启用面板集合**跨会话持久化**（`presentationSettings.enabledPanelIds`）；
   本仓连当前页都不落盘。
4. DBeaver 点「已选中页签」什么都不做；本仓会收起整栏。

## 4. 三个方案

### 方案 A：只加 ×（最小改动）

给每个页签加 `Tab::suffix(× 按钮)`，点击 = 关掉**这一页**：
`open_tabs: Vec<RightTab>` 移除该项；被关的是当前页则选邻近页；集合空 → 收起右栏
（`show_value_sidebar = false`）。点当前页签的旧行为（收起整栏）**保留**。

- 改动面：`data.rs`（+`open_tabs`）、`events.rs`（`close_right_tab`、`toggle_right_tab` 分支、
  `set_right_tab` 自动补登记）、`tabs.rs`（页签条按 `open_tabs` 渲染 + suffix 按钮）、
  `sidebars.rs`（当前页不在集合时的兜底）、视图菜单勾选态。
- 风险：两套关闭语义并存（× 关单页 / 点当前页签关整栏），用户可能困惑；
  DBeaver 没有第二套。但不动现有用例的既有预期，回归面最小。

### 方案 B：A + 去掉「点当前页签收起整栏」（推荐）

在 A 之上把 `toggle_right_tab(tab)` 改成「只有点当前页签才关该页」→ 与 DBeaver 一致：
点已选中页签 = 无操作（选中不变），关闭只能走 × 或视图菜单勾选。
同时视图菜单的每页勾选项与 `open_tabs` 读写同一份状态（勾 = 在集合里）。

- 需要改的既有用例：`toggling_the_active_page_collapses_the_panel`
  （语义从「收起整栏」改为「关掉该页并选中邻近页」）、
  `right_panel_tab_strip_lists_all_panels_and_click_switches`
  （改成「按 open_tabs 渲染」的版本）、`right_tab_index_maps_to_the_six_panel_tabs`
  （索引映射要按「固定六页的规范序 × 当前集合」两段换算）。
- 风险：`right_tab_index` / `right_tab_from_index` 是页签点击与视图菜单共用换算，
  改集合后必须同步六处（顺序、索引、图标、描述、文案、菜单项），漏一处就错位。

### 方案 C：完整 CTabFolder（多页内容同时挂载，按页关闭 + 重开）

把六个页内容全部挂载、只显示一个（`hidden()` 而非不渲染），并持久化 `open_tabs`。
结构上最接近，但：① 六页同时各跑一遍数据准备（统计 / 分组 / 引用），
在没有缓存的当下等于把 §9.07 之前的「每帧重算」放大六倍；
② 持久化要新增 settings 键与宿主注入，跨 crate 三处同步；
③ 目视差异（页签条密度、关闭按钮位置）用户已声明统一处理，先做会返工。

## 5. 结论与实施步骤

**推荐方案 B**，理由：它把「关掉一页」这件事变成真实状态（对齐 `enabledPanelIds`），
同时消掉本仓自造的「点当前页签收起整栏」这一条与 DBeaver 不一致的语义，
改动仍限制在右栏内部，不碰 settings、不碰宿主分派以外的代码。

分两步落地（每步一批、各自带用例与门禁）：

**第 1 步：状态模型 + 页签条 ×**
1. `data.rs`：新增 `open_tabs: Vec<RightTab>`（初值 = `events::right_tabs()` 全集），
   `right_tab` 保持单值作「当前页」。
2. `events.rs`：`close_right_tab(tab, cx)` —— 从 `open_tabs` 移除；若关的是当前页，
   选同序下一个（没有则前一个）；`open_tabs` 空 → `show_value_sidebar = false`；
   `open_right_tab(tab, cx)` —— 按规范序插回并置为当前（视图菜单勾选用）。
3. `tabs.rs`：页签条按 `open_tabs` 渲染，每页 `Tab::suffix(Button ×)`；
   × 的点击走 `ResultRequest::CloseRightTab(tab)`（新请求变体，宿主转手给
   `close_right_tab`）。
4. 用例：关中间页后页签条少一项且当前页落到邻近页；关当前页后视图菜单该页未勾选；
   关到空集时 `show_value_sidebar == false` 且右栏不渲染；再勾回来恢复。

**第 1 步的实际实施差异（2026-11-19 落地时登记）**：
- 页签 ×：改为**行尾单颗**（`editor-result-right-tab-close`），不是每页 `Tab::suffix`。
  原因：kit `Tab` 无 `closable`，`.suffix` 内嵌按钮在 headless 下无法验证「不会与页签点击
  双触发」；行尾按钮独立成 hitbox，行为可断言。DBeaver 的 `SWT.CLOSE` 仍属未对齐项。
- 空集兜底：走 `set_panels_visible(false)`（面板总开关同一入口），不用裸
  `show_value_sidebar = false`——保证「关空 = 右栏收起」与工具栏开关的互斥关系一致。
- 视图菜单第 1 步就一并改成「勾选 = 在集合里」，未等第 2 步：勾选态与页签 × 共用同一个集合，
  分开做会临时出现「× 已关但菜单仍显示当前页勾选」的错态。
- 右缘竖条：`toggle_right_tab` 补「目标页不在集合 → 先启用再切」，避免被关掉的页只能从菜单找回。

**第 2 步：消掉「点当前页签收起整栏」**
1. `toggle_right_tab` 改为「点已选中页签 → 无操作」（不再收起整栏）；
   保留 `set_panels_visible` / 视图菜单「收起全部面板」作为唯一整栏收起入口。
2. 视图菜单六项勾选态改读 `open_tabs`，点击走 `open_right_tab` / `close_right_tab`。
3. 改上述三条既有用例，补一条「视图菜单勾选 → 页签条同步增减」的闭环用例。

**第 2 步的实际实施差异（2026-11-19 落地时登记）**：① `toggle_right_tab` 已改成纯切页
（点已选中页返回 `false`、面板不动），`right_tab_click_collapses` 删除；② 视图菜单的勾选态
与 `close_right_tab` 已经在第 1 步落地，本步只补竖条侧；③ 既有用例改为
`clicking_the_active_edge_tab_is_a_noop`（点当前页无操作 / 点别页切过去 / 收起后点当前页重展）
与 `edge_bar_reopens_a_closed_page`（× 关掉的页可由竖条重新启用），竖条提示改三态、
勾选态改判「面板可见且是这一页」。详见 `result-panel.md` §9.09。

## 6. 边界与不做的事

- **不做**方案 C 的「六页内容同时挂载」（会把统计 / 分组 / 引用的每帧重算放大六倍）。
  `open_tabs` 持久化已在第 26 批补上（`result_panel.open_tabs`，见 `result-panel.md` §9.10），
  状态行上面的「不做」一条仅剩「同时挂载」与 `exec_sql` 页存废待拍板。
- 页签条密度、× 的大小与位置、关闭按钮 hover 表现属「目视统一处理」，本方案不定义。
- `exec_sql` 页（本仓多出的一页）是否保留仍待拍板；本方案对它在集合里一视同仁。
