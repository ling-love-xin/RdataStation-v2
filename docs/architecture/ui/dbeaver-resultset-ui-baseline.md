# DBeaver 结果集 UI 对齐基线

> 目的：为「结果集面板 1:1 复刻 DBeaver」提供**可复核**的组件树、尺寸真值、待确认项与现状差异。
> 全部结论来自本地 `dbeaver-ref` 源码，逐条带文件与行号；未读到的项一律进「待确认」，不做推测。
> 颜色不在此文档取值：只记录 DBeaver 的**颜色角色名**，落地时映射到项目主题 token。

## 0. 结论先行（与任务书前提冲突的部分）

1. **`UIConstants.java` 不存在**。`dbeaver-ref` 全仓无 `org/jkiss/dbeaver/ui/UIConstants.java`；`plugins/org.jkiss.dbeaver.ui/controls/grid/` 目录也不存在。任务书里「尺寸取自 UIConstants」的路径不成立，实际权威是：
   - 网格：`plugins/org.jkiss.dbeaver.ui.editors.data/src/org/jkiss/dbeaver/ui/controls/lightgrid/`
   - 结果集视图：`.../controls/resultset/`
2. **DBeaver 结果集没有「顶部工具栏」**。`ResultSetViewer` 的四个 `TOOLBAR_*_CONTRIBUTION_ID` 全部挂在**底部状态条**（`ResultSetViewer.java:149-152`、`:2043-2195`）。顶部只有一条**筛选栏**（`filterPanel`，`ResultSetViewer.java:311-318`）。
   → 任务书「顶部工具栏 + 表头 + 网格 + 底部状态栏」这一四段式与 DBeaver 真实结构不符；真实结构是「顶部筛选栏 / 左竖条 / 网格（含右栏 sash）/ 右竖条 / 底部状态条」。
3. **行高与表头高不是常量**，是字体度量的算式：
   - 行高 `itemHeight = fontMetrics.getHeight() + 3`（`LightGrid.java:1871`）
   - 表头高 = `topMargin(6) + fontMetrics.getHeight() + bottomMargin(6)`（`GridColumn.java:45-46`、`:201`，`LightGrid.java:1878-1880`）
   - 行与行之间另有 1px 分隔线：`(getItemHeight() + 1) * rows - 1`（`LightGrid.java:1270`、`:1350`）
4. **网格有行头（行号列）**：`Spreadsheet` 构造即 `setRowHeaderVisible(true)`（`Spreadsheet.java:108`），并有网格线（`:109`）与列头（`:110`）。
5. 我方现状（`crates/result_panel`）在**骨架层**是「标签条 / 工具栏 / 主体 / 状态栏」四段 + 两行可选条（展开筛选条、查找条），见 `panel/render.rs:1565-1616`。相对 DBeaver 属于**结构性新增两层**（内部标签条、顶部按钮工具栏），这是「1:1」最大的偏离点，需要先拍板再动代码。

## 1. 取证范围（本轮直接读到的证据）

| 文件 | 行 | 用途 |
| --- | --- | --- |
| `resultset/ResultSetViewer.java` | 286-435 | 面板骨架与子件顺序 |
| 同上 | 149-153 | 四个工具栏 contribution id |
| 同上 | 1971-2012 | 筛选栏挂载 |
| 同上 | 2015-2195 | 底部状态条 10 段 |
| `resultset/ResultSetFilterPanel.java` | 95-214、315-356 | 筛选栏结构与常量 |
| 同上 | 163-180 | 两层 GridLayout 与 margin |
| `resultset/StatusLabel.java` | 50-73 | 状态消息 = Label + 详情 ToolItem |
| `resultset/ResultSetThemeSettings.java` | 39-84 | 颜色角色名（无取值） |
| `resultset/ResultSetPreferences.java` | 48 | `resultset.show.oddRows` |
| `spreadsheet/Spreadsheet.java` | 53、108-110 | 网格 = LightGrid 子类；行头/网格线/列头开启 |
| `lightgrid/LightGrid.java` | 54-79 | 滚动增量、拖拽阈值、行头宽、最小列头宽 |
| 同上 | 1871、1878-1880 | 行高、表头高 |
| 同上 | 1270、1334、1350 | 行区间与 1px 线 |
| 同上 | 2202-2248 | 绘制顺序（表头固定、横向同步） |
| `lightgrid/GridColumn.java` | 43-50、196-218 | 默认宽、列内距、表头高算式 |
| `lightgrid/GridColumnRenderer.java` | 36-49 | 表头绘制内距、排序箭头尺寸来源 |
| `lightgrid/GridCellRenderer.java` | 43-50、96 | 单元格内距 |
| `lightgrid/GridRowRenderer.java` | 37-41 | 行头内距 |
| `plugin.xml` | 1402-1522 | 状态条三组工具条的**项顺序** |
| 本仓 `crates/result_panel/src/panel/render.rs` | 1204-1213、1565-1616 | 我方层序与工具栏组成 |
| 本仓 `crates/result_panel/src/grid/status.rs` | 322-427、840-935 | 我方工具栏、状态行段序 |
| 本仓 `crates/result_panel/src/ui.rs` | 32-197 | 我方尺寸常量 |

## 2. ① 组件树清单（DBeaver 真值）

### 2.1 A｜面板骨架（`ResultSetViewer.java:286-435`）

```
mainPanel                                  GridLayout(3 列 | supportsPanels ? 3 : 2)，verticalSpacing = 0
├─ autoRefreshControl                      (:290)  自动刷新控件（体积很小，常驻）
├─ filterPanel                             (:311-318) FILL_HORIZONTAL + horizontalSpan = 全列  ← 顶部唯一横向条
│   └─ filterComposite                     (见 2.2)
├─ presentationSwitchFolder                (:320-326) VerticalFolder(SWT.LEFT)  ← 左侧竖排「呈现切换」
├─ viewerPanel                             (:328-330) FILL_BOTH  1 列占位
│   └─ viewerSash                          (:346) CustomSashForm(SWT.HORIZONTAL | SWT.SMOOTH)
│       ├─ presentationPanel               (:354) FILL_BOTH ← 网格/文本/记录 呈现的宿主
│       └─ panelFolder                     (:357-418) CTabFolder(SWT.FLAT | SWT.TOP) ← 右侧多页面板
├─ panelSwitchFolder                       (:335-341) VerticalFolder(SWT.RIGHT) ← 右侧竖排「面板开关」
└─ statusBar                               (:422-428) FILL_HORIZONTAL + 全列  ← 底部唯一横向条（见 2.4）
```

要点：
- 左右两条 `VerticalFolder` 是**独立的竖排工具条**，不属于网格容器，也不属于状态条。
- 网格与右栏是 **sash 关系**（可拖分割），网格不是整宽。
- 标签条（多结果集）**不在此树内**：多结果集由外层 `SQLResultsEditor`（`plugins/org.jkiss.dbeaver.ui.editors.sql/.../SQLResultsEditor.java`）承载，属另一 EditorPart。

### 2.2 B｜顶部筛选栏（`ResultSetFilterPanel.java:160-214` + `:315-356`）

```
ResultSetFilterPanel (Composite)           GridLayout(2, false)，marginHeight = 3，marginWidth = 3   (:163-166)
└─ filterComposite (Composite)             GridLayout(compact ? 4 : 5, false)，margin = 2，spacing = 0  (:174-180)
   │                                       + CompositeBorderPainter（自绘 1px 边框）                 (:182)
   ├─ ① activeObjectPanel                  当前对象标识（表名/活动对象）                            (:184)
   ├─ ② filterExpandPanel                  展开/收起开关（仅非 compact）                           (:186)
   ├─ ③ filtersTextViewer (SWT.MULTI)      SQL/过滤器文本区，FILL_BOTH                            (:192-206)
   ├─ ④ executePanel                       执行按钮（仅非 compact）                                (:209)
   └─ ⑤ historyPanel                       右侧按钮组（见下）
        └─ filterToolbar (ToolBar, HORIZONTAL | RIGHT)                                              (:315)
           ├─ filtersClearButton     SWT.DROP_DOWN | NO_FOCUS   「清空筛选」带下拉               (:319)
           ├─ filtersCustomButton    SWT.DROP_DOWN | NO_FOCUS   「自定义筛选」带下拉             (:330)
           ├─ historyBackButton      SWT.DROP_DOWN | NO_FOCUS   历史后退 + 下拉                 (:343)
           ├─ historyForwardButton   SWT.DROP_DOWN | NO_FOCUS   历史前进 + 下拉                 (:348)
           └─ openFindReplaceButton  SWT.CHECK | NO_FOCUS       查找/替换开关（可勾选态）        (:356)
```

常量（`ResultSetFilterPanel.java:102-104`）：`MIN_FILTER_TEXT_WIDTH = 50`、`MIN_FILTER_TEXT_HEIGHT = 20`、`MAX_HISTORY_PANEL_HEIGHT = 200`。
高度推算：`3 + 2 + 20 + 2 + 3 = 30`（外层 margin 3、内层 margin 2、文本高 20）。

### 2.3 C｜网格（`Spreadsheet.java:108-110` + `LightGrid.java:2202-2248`）

```
Spreadsheet extends LightGrid
├─ 列头（columnHeadersVisible = true）     paintHeader(gc)，画在 y = 0，行从 y = headerHeight 起（:2220-2225）
│  每个 GridColumn 表头内：图标（可选） + 标题文本 + 排序箭头（可选）
├─ 行头（rowHeaderVisible = true）         宽 rowHeaderWidth（30 起，MIN 40 / MAX 400，:72-74）
│  内容：行号文本（当前行加粗，见我方对齐注释 GridRowRenderer:96 口径）
└─ 数据格（每行尾 1px 分隔线）
```

绘制与滚动（`LightGrid.java:2202-2248`）：
- `y = 0` → `paintHeader` → `y += headerHeight` → 行从 `headerHeight` 起画 ⇒ **表头固定，垂直滚动只改 `getTopIndex()`**。
- 行绘制时 `x -= getHScrollSelectionInPixels()`，表头绘制同一滚动量 ⇒ **横向滚动与表头天然同步**（同一 Canvas 自绘，不是两套控件联动）。
- `availableRows = (availableHeight + 1) / (getItemHeight() + 1)`（`:1334`）。

### 2.4 D｜底部状态条（`ResultSetViewer.java:2043-2195`）

`statusBar` 用 `RowLayout(SWT.HORIZONTAL)`：`marginLeft = 0`、`center = true`、`wrap = true`、`pack = true`（`:2048-2056`）。段序（严格按创建顺序）：

| # | 段 | 来源 |
| --- | --- | --- |
| 1 | Refresh 工具条（`AutoRefresh.populateRefreshButton`，图标 `UIIcon.REFRESH`，命令 `FILE_REFRESH`） | `:2058-2074` |
| 2 | Edit 工具条（`toolbar:…status.editCmds`） | `:2077-2083` |
| 3 | Navigation 工具条（`toolbar:…status.navCmds`） | `:2084-2090` |
| 4 | Config 工具条（空 populator，为配置项预留） | `:2091-2096` |
| 5 | Export + Additions 工具条（`status.exportCmd` + `GroupMarker(presentations)` + `Separator(additions)` + `toolbar:…status`） | `:2098-2111` |
| 6 | Config 工具条（竖分隔 + `ConfigAction` 齿轮） | `:2113-2122` |
| 7 | `resultSetSize` Text（段大小输入，宽 `5 * fontHeight`，tooltip 段大小） | `:2124-2153` |
| 8 | `rowCountLabel`（`ActiveStatusMessage`，图标 `UIIcon.COMPILE`，点击算 `count(*)`） | `:2155-2180` |
| 9 | `selectionStatLabel`（只读 Text，选中统计） | `:2182-2184` |
| 10 | `statusLabel`（`StatusLabel`：`Label` + 详情 `ToolItem`，宽 `50 * fontHeight`；`FEATURE_COMPACT_STATUS` 时不建） | `:2186-2191`、`StatusLabel.java:65-73` |

三组工具条的**项顺序**（`plugin.xml`）：

- editCmds（`:1403-1462`）：竖分隔 → `resultset.applyChanges`(pulldown, FORCE_TEXT) → `resultset.rejectChanges`(FORCE_TEXT) → `edit.undo` → `edit.redo` → 竖分隔 → `resultset.row.edit` → `row.add` → `row.copy` → `row.delete`
- navCmds（`:1466-1507`）：竖分隔 → `row.first` → `row.previous` → `row.next` → `row.last` → `fetch.page` → `fetch.all`
- exportCmd（`:1511-1521`）：竖分隔 → `resultset.export`(pulldown, FORCE_TEXT, `exportWithParameter=true`)

注：每个 command 都带 `org.jkiss.dbeaver.ui.toolbar.configuration.visible` 的 `visibleWhen` 测试（用户可在工具栏配置里逐项隐藏），面板通过 `TOOLBAR_CONFIGURATION_VISIBLE_PROPERTY` 监听刷新（`:153`、`:1963-1968`）。

### 2.5 E｜网格附属状态（颜色角色，不取值）

`ResultSetThemeSettings.java:39-84` 定义角色：`backgroundAdded / Deleted / Modified / Odd / ReadOnly`、`foregroundSelected / Null`、`backgroundSelected / Matched / Error`、`foregroundError`、`cellHeaderForeground / Background / SelectedBackground / Border`、`lineNormalColor / lineSelectedColor`、`dtBinary / dtBoolean / dtDateTime / dtNumeric / dtString`。

斑马行是**偏好项**：`ResultSetPreferences.RESULT_SET_SHOW_ODD_ROWS = "resultset.show.oddRows"`（`:48`），由 `SpreadsheetPresentation` 读取（`:964`）并在绘制前判断 `showOddRows && !isHighContrastTheme`（`:2870`），底色取 `backgroundOdd`。⇒ 「斑马开关由上层控制」与 DBeaver 完全一致。

## 3. ② 布局常量表（不含颜色）

### 3.1 网格 / 行列

| 项 | 值 | 来源 |
| --- | --- | --- |
| 行高 | `fontMetrics.getHeight() + 3` | `LightGrid.java:1871` |
| 行间分隔线 | 1px（`(itemHeight + 1) * rows - 1`） | `LightGrid.java:1270`、`:1350` |
| 可见行数 | `(availableHeight + 1) / (itemHeight + 1)` | `LightGrid.java:1334` |
| 列默认宽（初值） | 10 | `GridColumn.java:43` |
| 最小列头宽 | 32 | `LightGrid.java:79` |
| 水平滚动增量 | 12px | `LightGrid.java:64` |
| 列边界拖拽命中阈值 | 4px（±4px 内视为 resizer） | `LightGrid.java:71` |
| 行头宽 | 默认 30 / 最小 40 / 最大 400 | `LightGrid.java:72-74` |
| 单元格内距 | 左 6 / 右 6 / 上 0 / 文本上偏移 1 | `GridCellRenderer.java:43-50` |
| 单元格内部边距 | `INSIDE_MARGIN = 3` | `GridCellRenderer.java:45` |
| 单元格提示图标 | `HINT_ICON_MARGIN = 4`、`HINT_ICON_SPACING = 1` | `GridCellRenderer.java:46-47` |
| 层级缩进 | `LEVEL_INDENT = 5` | `GridCellRenderer.java:96` |

### 3.2 表头

| 项 | 值 | 来源 |
| --- | --- | --- |
| 表头高（单行） | `topMargin(6) + fontHeight + bottomMargin(6)` | `GridColumn.java:45-46`、`:201`；`LightGrid.java:1878-1880` |
| 表头高（带描述第二行） | 再加 `6 + fontHeight` | `GridColumn.java:206-208` |
| 表头有图标时 | `height = max(height, 6 + 图标高 + 6)` | `GridColumn.java:202-205` |
| 表头绘制内距 | 左 6 / 右 6 / 上 4 / 下 6 | `GridColumnRenderer.java:36-41` |
| 排序箭头边距 | `ARROW_MARGIN = 6` | `GridColumnRenderer.java:40` |
| 表头图标与文字间距 | `IMAGE_SPACING = 3` | `GridColumnRenderer.java:41` |
| 排序箭头尺寸 | **取自图标本体** `IMAGE_DESC.getBounds()`，非固定数 | `GridColumnRenderer.java:48-49` |
| 表头宽算起算 | `leftMargin(6) + 图标宽 + 3 + hints 图标宽 + 文本宽 …` | `GridColumn.java:220-234` |

> 注意两个 `topMargin` 不同源：`GridColumn.topMargin = 6` 参与**高度算式**；`GridColumnRenderer.TOP_MARGIN = 4` 只决定**图标绘制 y 偏移**。不可混用。

### 3.3 行头

| 项 | 值 | 来源 |
| --- | --- | --- |
| 内距 | 左 4 / 右 4 | `GridRowRenderer.java:37-41` |
| 图标间距 | `IMAGE_SPACING = 5` | 同上 |
| 展开器间距 | `EXPANDER_SPACING = 2` | 同上 |

### 3.4 筛选栏

| 项 | 值 | 来源 |
| --- | --- | --- |
| 外层布局 | `GridLayout(2, false)`，margin 3/3 | `ResultSetFilterPanel.java:163-166` |
| 内层布局 | `GridLayout(compact ? 4 : 5, false)`，margin 2，spacing 0 | `:174-180` |
| 文本区最小宽/高 | 50 / 20 | `:102-103` |
| 历史面板最大高 | 200 | `:104` |
| 整条高（算式） | 3 + 2 + 20 + 2 + 3 = 30 | 由上述常量合成 |

### 3.5 状态条

| 项 | 值 | 来源 |
| --- | --- | --- |
| 布局 | `RowLayout(HORIZONTAL)`：`marginLeft = 0`、`center = true`、`wrap = true`、`pack = true` | `ResultSetViewer.java:2048-2056` |
| 段大小框宽 | `5 * fontHeight` | `:2127` |
| 状态消息标签宽 | `50 * fontHeight` | `:2189` |
| 工具条分组 | 6 段（见 2.4 表 1-6 行） | `:2058-2122` |

### 3.6 面板骨架

| 项 | 值 | 来源 |
| --- | --- | --- |
| mainPanel 列数 | `supportsPanels ? 3 : 2` | `ResultSetViewer.java:287` |
| verticalSpacing | 0 | `:288` |
| 顶部横条 | 筛选栏（`FILL_HORIZONTAL` + 全列 span） | `:311-318` |
| 底部横条 | 状态条（`FILL_HORIZONTAL` + 全列 span） | `:422-428` |

## 4. ③ 待确认参数清单（源码给不出数的项，动代码前必须先定）

| # | 参数 | 缺口 | 影响 | 建议 |
| Q1 | 字体高基准 | DBeaver 用 `fontMetrics.getHeight()`，随系统/主题字体变；GPUI 侧取 theme 字体行高还是宿主传入 | 行高、表头高、段大小框宽（`5×`）、状态标签宽（`50×`）全部依赖它 | 走 `theme` 字体度量，并在参数表里写成算式而非字面数 |
| Q2 | 排序箭头尺寸 | 源码取 `IMAGE_DESC` 图标本体尺寸 | 表头右侧箭头位置/点击热区 | 定 16×16 或取项目真实 asset 尺寸，二选一并记录 |
| Q3 | 状态条按钮图标尺寸与项间距 | SWT `ToolItem` 用平台默认（源码无像素常量） | 状态条 6 段的总宽与点击热区 | 实测 DBeaver 运行态截图量取，或按项目 Button 档位定档并标注偏离 |
| Q4 | 我方表头实际高 | DataTable 组件决定，`ui.rs` 无表头高常量 | 与 3.2 的「fontHeight + 12」无法比对 | 先量当前实现表头高，再决定是否加常量 |
| Q5 | 列宽自动算法 | DBeaver 初始列宽由 `computeHeaderWidth()` + 数据采样算（`GridColumn.java:220+`，未全读） | 我方按类型固定 120-300 档，复刻算法是另一工作量 | 明确取舍：复刻算法 or 保留类型档（推荐后者，标注有意差异） |
| Q6 | 单元格编辑框几何 | `SpreadsheetCellEditor` 的 `setBounds` 未取证 | 编辑态与单元格是否严丝合缝 | 补读该类后再定 |
| Q7 | 行头是否含「当前行加粗」「选中标记」 | `GridRowRenderer` 的标记绘制未逐行取证 | 行号列呈现细节 | 补读 `GridRowRenderer` 绘制方法 |
| Q8 | 冻结列（pinned）默认状态 | `LightGrid` 支持 `isPinned` / `pinnedColumnsWidth`，结果集默认是否已 pin 未取证 | 横向滚动时哪些列钉住 | 补证 `ResultSetTable`/`Spreadsheet` 的 pin 初始化 |
| Q9 | 表头第二行（类型描述）是否启用 | `GridColumn` 支持 description 加高，结果集 `getDescription` 返回什么未取证 | 表头是 1 行还是 2 行（差 `fontHeight + 6`） | 补读 `ResultSetLabelProvider.getDescription` |
| Q10 | 多结果集标签容器 | `SQLResultsEditor` 已确认存在，其内部容器实现未读 | 我方把标签条放进面板内部是否算「结构偏离」 | 补读后决定：保留内部标签条并登记为有意差异 |
| Q11 | 工具栏按钮 hover/按下/禁用三态来源 | DBeaver 由 SWT/CSS 提供，源码无状态表 | 复刻需逐态对齐 | 用项目 Button 组件的三态，逐项截图比对 |
| Q12 | 网格线四面还是单边 | `setLinesVisible(true)` 的实际绘制在 `LightGrid` 绘制段（未逐行读） | 我方现在用 `border` 单边，视觉密度不同 | 补读 `paintCell` 段确认四边 or 仅分隔 |

## 5. ④ 目前实现差异

### 5.1 结构性差异（S）

| # | DBeaver | 我方（证据） | 判定 |
| S1 | 无内部标签条（多结果集在外层 `SQLResultsEditor`） | 面板第一行是结果集标签条（32px，`ui.rs:97`；`render.rs:1581`） | 结构新增 |
| S2 | 无顶部按钮工具栏；动作条全在底部状态条 | 第二行是顶部工具栏（30px，`ui.rs:75`；`render.rs:1583`），右段含「下发源库/注释｜对比/提交撤销/分析/复制」（`grid/status.rs:402-424`） | 结构新增，**最大偏离** |
| S3 | 顶部 = 筛选栏（5 个子件，右端 5 个 `ToolItem`） | 筛选复合体在**工具栏左段**，右段另挂动作按钮（`grid/status.rs:367-425`） | 位置与分组不同 |
| S4 | 状态条 10 段（refresh→edit→nav→config→export→齿轮→段大小→计算行数→选中统计→StatusLabel） | 状态行 10 段：左栏开关→动作组→段大小→行数框→计算行数→选中统计→元信息三项→右栏开关→配置▾→齿轮（`grid/status.rs:840-935`） | 段数接近，**顺序/内容不同**（见 5.2） |

### 5.2 数值差异（N）

| # | 项 | DBeaver | 我方 | 证据 |
| N1 | 行高 | `fontHeight + 3` | 固定 26（`RESULT_TABLE_SIZE = XSmall`，`ui.rs:179`） | 随主题字号变化 vs 固定档 |
| N2 | 表头高 | `fontHeight + 12`（单行） | 组件决定（待量，Q4） | 无法直接比对 |
| N3 | 行号列宽 | 默认 30，生效下限 40，最大 400，可拖宽 | 48，且不可拖宽/不可自适应 | `ui.rs:165-166`；`grid/table_impl.rs:309`；`grid/fit.rs:48-49` |
| N4 | 最小列宽 | 32 | 80 | `ui.rs:189` |
| N5 | 默认列宽 | 10 起步 + 内容自适应 | 128（按类型 120/130/160/200/210/300/260） | `ui.rs:128-163` |
| N6 | 状态条：齿轮位置 | 段大小框**之前** | 最末（配置▾ 之后） | `ResultSetViewer.java:2113-2122` vs `grid/status.rs:929-934` |
| N7 | 状态条：无「行数输入框」 | DBeaver 无此框（行跳转走 `CMD_GO_TO_ROW` 对话框） | 常驻行数框（`RESULT_ROW_COUNT_BOX_WIDTH = 72`） | `grid/status.rs:875-893` |
| N8 | 状态条：无「内存/耗时/条数」三项 | 无 | 右段三项（`ml_auto` 贴右） | `grid/status.rs:908-926` |
| N9 | `StatusLabel`（消息文本 + 详情入口） | 有（宽 `50×fontHeight`） | 无对应件（`notice` 走浮层/其它通道） | `ResultSetViewer.java:2186-2191` |
| N10 | 单元格左右内距 | 6 / 6 | 6 / 6（已对齐） | `ui.rs:197` |
| N11 | 筛选栏整条高 | 30（算式） | 30（`RESULT_TOOLBAR_HEIGHT`，`ui.rs:75`），水平内距 3（`ui.rs:87`） | 已对齐 |

### 5.3 交互差异（I，对应你点名的 8 条）

| # | 要求 | DBeaver 实现 | 我方现状 | 结论 |
| I1 | 表头点击排序 | 列头点击触发 `Event_ChangeSort`（`LightGrid.java:57`），`Spreadsheet` 监听（`:360`） | 列头有 `.sort(...)` 槽（`grid/header.rs:118`）+ `perform_sort`（`grid/table_impl.rs:198`） | 行为在，触发区/箭头位置与 DBeaver 不同 |
| I2 | 拖拽列边界改宽 | `COLUMN_RESIZER_THRESHOLD = 4`（`:71`） | 走 vendor `DataTable` resize handle（阈值未取证，Q 待补） | 需实测阈值与最小宽约束 |
| I3 | Ctrl 多选 / Shift 区间 / 单击单行 | `LightGrid` 选择模型（`selectedRows`、`selectedCells`） | `TableEvent::SelectRow/SelectCell` + delegate 镜像（`grid/delegate.rs:213`、`panel/lifecycle.rs:464`） | 需逐项实测：区间多选是否已接线 |
| I4 | 垂直滚动表头固定 | 自绘：表头画在 y=0，行从 headerHeight 起（`:2220-2225`） | 组件虚拟列表的表头固定 | 语义等价，实现路径不同 |
| I5 | 横向滚动表头同步 | 同一 Canvas 同一 `hScroll` 量（`:2253`） | 组件内同步 | 同上 |
| I6 | 单元格进入编辑 | `SpreadsheetCellEditor` 覆盖单元格 | `Enter` → `EnterCellEdit`（`grid/edit.rs:137`）；`ESC` 结构性不可行（组件先吃掉） | 进入编辑在，**退出/撤销路径不同** |
| I7 | 工具栏按钮 hover/按下/禁用 | SWT + CSS 三态 | Button/Toolbar 组件态；禁用由 `disabled_state` 判据（`panel/tests.rs:1840-1899`） | 需逐项截图比对三态 |
| I8 | 斑马交替行（开关上层控） | 偏好 `resultset.show.oddRows`（`ResultSetPreferences.java:48`）+ `backgroundOdd` | 有斑马开关与用例覆盖（`grid/tests.rs`） | 已对齐（配置项化） |

## 6. ⑤ 需要先拍板的三件事（决定后续代码怎么写）

1. **骨架是否照抄 DBeaver**（去掉顶部按钮工具栏，把动作条并入底部状态条；标签条是否外移）。
   - 照抄：真 1:1，但要动 `panel/render.rs` 层序 + `grid/status.rs` 段序，改动面大（涉及既有键位/测试）。
   - 保留现状：视觉一致度打折，需在文档登记为「有意差异」。
   - 折中：保留内部标签条（多结果集体验更好），把「动作按钮」从顶部工具栏移到状态条，筛选栏独占顶部。
2. **行高/表头高是否改成字体度量算式**（Q1）。改则随主题字号缩放（更接近 DBeaver），不改则维持 26px 固定档。
3. **列宽策略**（Q5）：复刻 DBeaver 内容自适应算法，还是保留按类型固定档并登记差异。推荐后者。

以上三项定下来，Q2-Q12 可在实现期按序补齐，无需再等。

## 7. 拍板结果（本节取代 §6）

| # | 决定 | 落地含义 |
| --- | --- | --- |
| 1 | **保留工具栏** | 我们的顶部工具栏 = DBeaver「筛选栏」+ 右段动作组。整体登记为**有意扩展**（DBeaver 原生无此项），筛选栏部分仍严格按 DBeaver 子件顺序。 |
| 2 | **行高/表头高改字体度量算式** | 行高 = 行高基准 + 3；表头高 = 行高基准 + 12；随主题字号缩放，不再固定 26px。 |
| 3 | **列宽复刻内容自适应** | 复刻 DBeaver「按内容/按值」列宽算法与 `MAX_DEF_COLUMN_WIDTH` 语义，替换现有按类型固定档。 |
| 附加 | 复刻数据编辑器设置项 | 见 §8：6 个设置页的逐项清单。 |

本节同时关闭三个待确认项：
- **Q1（字体高基准）→ 关闭**：按决定 2 取字体行高，不再用固定档。
- **Q5（列宽策略）→ 关闭**：按决定 3 复刻算法。
- **Q9（表头第二行）→ 部分关闭**：存在三档偏好 `RESULT_SET_COLUMN_HEADER_EXTRA`（`NOTHING` / `DESCRIPTION` / `DATA_TYPE`，`ResultSetPreferences.java:56-62`，中文「列标题额外信息」）；**默认档取值尚未取证**（需查 preference initializer），故默认表头是 1 行还是 2 行仍待定。

### 7.1 工具栏最终形态（保留 + 登记为扩展）

```
左段（flex_1，DBeaver 筛选栏本体）                    右段（动作组，本仓扩展）
├ ① 「显示 SQL」chip（本仓 sql_chip）                  ├ pushdown   下发源库
├ ② 筛选输入框（DBeaver filtersText）                  ├ comments   列注释 switch
├ ③ 展开/收起（DBeaver filterExpandPanel）             ├ ───── 竖分隔（1px × 24px）─────
└ ④ 运行 ▶（DBeaver executePanel）                     ├ compare    表对比
                                                       ├ commit     提交/撤销
（DBeaver 此处另有 historyPanel 的 5 个 ToolItem：      ├ analysis   分析 ▾
 清空筛选▾ / 自定义筛选▾ / 历史后退▾ /                └ copy       复制
 历史前进▾ / 查找替换 toggle —— 本仓没有）              （顺序来源：grid/status.rs:402-424）
```

尺寸不变：条高 30px（`ui.rs:75`）、水平内距 3px（`ui.rs:87`）、右段按钮 XSmall 20px、竖分隔 1px×24px（`grid/status.rs:411-421`）。

**Q13**：右段共 6 项，你点名的是「表对比 / 列注释 / 分析」三项，另有 `下发源库`、`提交/撤销`、`复制` 三项已在位 —— 是否 6 项全保留？
**Q14**：DBeaver 筛选栏右端那 5 个 `ToolItem`（清空筛选▾ / 自定义筛选▾ / 历史后退▾ / 历史前进▾ / 查找替换）本仓一个都没有。补则工具栏变三段（左筛选 / 中历史 / 右动作），不补则登记为差异。

### 7.2 行高改法：可行，且不需要改 vendor 结构

已逐条核实（`vendor/gpui-component/src`，0.10.0 口径）：

| 能力 | 证据 | 结论 |
| --- | --- | --- |
| 表头高可单独指定 | `DataTable::header_row_height(impl Into<Pixels>)`（`table/data_table.rs:152-156`） | 直接给 `行高基准 + 12` |
| 表头高缺省回退 | `header_row_height().unwrap_or(Size::table_row_height())`（`table/state.rs:840-843`） | 不指定时才吃档位 |
| 行高支持任意像素 | `table_row_height()`：`Size::Size(h) => h`（`sizing.rs:56-65`） | 用 `Size::Size(px(h))` 即可 |
| 行高读取点 | `state.rs:853 / 2347 / 2797`、`loading.rs:54` | 全部走同一函数，改一处即全生效 |
| 不牵连字号 | `table/` 内 `font_size` / `text_size` **零命中** | 「改 size 会连带字号」不成立 |
| 会牵连单元格内距 | `Size` 在表格内只有三类用途：行高、`table_cell_padding()`、骨架高（`self.size.` 共 8 处） | `Size::Size` 会落到 padding 的 `_ =>` 分支（上4/下4/左8/右8，`sizing.rs:89-94`） |
| 内距可逐列写回 | `Column::paddings(impl Into<Edges<Pixels>>)`（`table/column.rs:26,147-149`）；`column.paddings` 优先于档位（`state.rs:1331-1335`） | 显式写「左右 6 / 上下 0」即可保住 DBeaver 口径 |

落地三步（实现期）：
1. `ui.rs` 把 `RESULT_TABLE_SIZE: Size` 改为 `fn table_size(font_height: Pixels) -> Size` = `Size::Size(px(font_height + 3.))`。
2. 表头：`.header_row_height(px(font_height + 12.))`。
3. 每个列定义追加 `.paddings(Edges { top: 0, bottom: 0, left: 6, right: 6 })`（数值取 `GridCellRenderer.LEFT/RIGHT_MARGIN = 6`、`TOP_MARGIN = 0`）。

**Q15**：`font_height` 取值 —— DBeaver 用 `fontMetrics.getHeight()`（ascent+descent+leading）。GPUI 侧建议取 `TextStyle::line_height`（推荐），备选 `font_size × 系数`。需你确认一处口径，全仓只此一个来源。

### 7.3 列宽复刻仍缺的取证（Q5 细化）

- 总开关：`RESULT_SET_CALC_COLUMN_WIDTH_BY_VALUES` = `resultset.calc.columnWidthByValues`（`ResultSetPreferences.java:73`）。
- 上限口径：`RESULT_SET_MAX_COLUMN_DEF_WIDTH` = `resultset.max.column.def.width`（`:83`）+ `MAX_DEF_COLUMN_WIDTH = 30`（`:26`），中文「最大自动大小列(%)」。
- **算法实现未取证**：列宽计算在 `Spreadsheet` / `ResultSetTable` 一侧，本轮未读。复刻前需补读，否则只能照搬那两个开关而拿不到算式。

## 8. 数据编辑器设置项复刻清单

设置页层级（`plugin.xml:1526-1555`，preferencePages 的 category 关系）：

| 顺序 | 页面类 | 页面 id 后缀 | 状态 |
| --- | --- | --- | --- |
| 1 | `PrefPageResultSetMain` | `…main.resultset` | 已取证（§8.3） |
| 2 | `PrefPageResultSetEditors` | `…main.resultset.editors` | 已取证（§8.2） |
| 3 | `PrefPageResultSetPresentation` | `…main.resultset.presentation` | 未逐项取证 |
| 4 | `PrefPageResultSetPresentationGrid` | `…presentation.grid` | 已取证（§8.1，P0） |
| 5 | `PrefPageResultSetPresentationPlainText` | `…presentation.plain.text` | 未逐项取证（键在 `ResultSetPreferences.java:97-106`） |
| 6 | `PrefPageDataFormat` / `PrefPageDataViewer` | `…main.dataformat` / `…main.dataviewer` | 未逐项取证 |

### 8.1 网格呈现页：17 项（P0，`PrefPageResultSetPresentationGrid.java:98-222`）

两栏 + 两个分组，**逐项顺序**如下（中文文案取 `DataEditorsResources_zh.properties:40-129`）：

| 组 | 顺序 | 控件 | 偏好键 | 中文 |
| --- | --- | --- | --- | --- |
| 外观 | 1 | Checkbox | `resultset.show.oddRows` | 标记奇/偶行 |
| 外观 | 2 | Checkbox | `resultset.show.colorizeDataTypes` | 启用数据类型色彩高亮 |
| 外观 | 3 | Checkbox | `resultset.show.attIcons` | 显示列类型图标 |
| 外观 | 4 | Checkbox | `resultset.show.attFilters` | 显示属性过滤器 |
| 外观 | 5 | Checkbox | `resultset.show.attrOrder` | 显示排序图标 |
| 外观 | 6 | Checkbox | `resultset.show.boolean.checkbox` | 将布尔值显示为复选框 |
| 外观 | 7 | Checkbox | `resultset.show.whitespace.characters` | 显示空白字符 |
| 外观 | 8 | Checkbox | `resultset.show.column.position` | 显示列的顺序设置 |
| 外观 | 9 | ValueFormatSelector | `resultset.grid.value.format` | （值显示格式） |
| 外观 | 10 | Text | `resultset.max.column.def.width` | 最大自动大小列(%) |
| 外观 | 11 | 链接 | — | Colors and Fonts（跳转页） |
| 行为 | 12 | Checkbox | `resultset.highlight.selectedRows` | 突出显示有选定单元格的行 |
| 行为 | 13 | Checkbox | `resultset.hScroll.smooth` | 平滑滚动 |
| 行为 | 14 | Checkbox | `resultset.show.boolean.toggleOnClick` | 点击时切换布尔值 |
| 行为 | 15 | Checkbox | `resultset.behavior.inlineEnter` | 内联编辑后选择下一个单元格 |
| 行为 | 16 | Combo(READ_ONLY, 5 档) | `resultset.behavior.doubleClick` | 双击行为 |
| 行为 | 17 | Text | `resultset.show.row.batch.size` | 行色彩批量大小 |

双击行为 5 档（枚举序，`:192-211`）：None / Editor / Inline editor / Copy cell / Copy-paste cell。

### 8.2 值编辑器页：3 组 13 项（`PrefPageResultSetEditors.java:98-206`）

| 组 | 控件 | 偏好键 | 说明 |
| --- | --- | --- | --- |
| String | Checkbox（默认 true） | `resultset.string.use.content.editor` | 字符串用内容编辑器打开 |
| Binary | Combo | — | 二进制呈现格式（`DBConstants.BINARY_FORMATS` 动态） |
| Binary | Combo（Editor / Dialog） | `resultset.binary.editor` | 二进制编辑器类型 |
| Binary | Spinner（0..10000） | — | 二进制字符串最大长度 |
| Binary | Spinner（0..1GB） | — | 最大 LOB 长度 |
| Binary | Combo | — | Hex 编码（`UIUtils.createEncodingCombo`） |
| Binary | Checkbox（默认 true） | — | CLOB 内容缓存 |
| Binary | Checkbox（默认 true） | — | BLOB 内容缓存 |
| Binary | Spinner（0..MAX_INT） | — | 内容缓存上限 |
| Binary | Checkbox | `resultset.edit.longaslob` | 长文本按 LOB 编辑 |
| Content | Spinner（默认 1000） | `resultset.edit.maxtextsize` | 内容最大文本长度 |
| Content | Checkbox | `resultset.commit.oneditapply` | 应用值时提交 |
| Content | Checkbox | `resultset.commit.oncontentapply` | 应用内容时提交 |

### 8.3 主设置页：左 4 组 / 右 2 组（`PrefPageResultSetMain.java:135-267`）

| 栏 | 组 | 控件 | 偏好键 / 来源 |
| --- | --- | --- | --- |
| 左 | Queries | Text「最大结果集大小」 | `ModelPreferences.RESULT_SET_MAX_ROWS`（`:288`） |
| 左 | Queries | Checkbox（默认 true） | `resultset.autofetch.next.segment` |
| 左 | Queries | Checkbox（默认 true） | `ModelPreferences.RESULT_SET_REREAD_ON_SCROLLING`（`:286`） |
| 左 | Queries | Checkbox | `ModelPreferences.RESULT_SET_MAX_ROWS_USE_SQL`（`:293`） |
| 左 | Queries | Combo（默认 SMART） | `ModelPreferences.RESULT_SET_ORDERING_STRATEGY`（`:295-299`） |
| 左 | Queries | Combo | 排序行为策略项（`OrderingPolicy`，键名未在 `ResultSetPreferences`） |
| 左 | Editing | Checkbox | `resultset.edit.key.use_all_columns` |
| 左 | Editing | Checkbox（前项选中时禁用） | `resultset.edit.disableIfKeyMissing`（联动 `:272-279`） |
| 左 | History | Text（默认 200，0..10000，宽 `5×fontHeight`） | `resultset.edit.undoLevel`（`:202-214`） |
| 左 | Advanced | Text + ms | `resultset.cancel.timeout` |
| 左 | Advanced | Checkbox | 过滤器强制子查询（键名待查） |
| 左 | Advanced | Checkbox | 使用列名（键名待查） |
| 左 | Advanced | Checkbox | 使用 fetch size（键名待查） |
| 右 | Misc | Checkbox | `keep.statement.open` |
| 右 | Misc | Checkbox | `resultset.edit.new.row.after` |
| 右 | Misc | Checkbox | `resultset.edit.refreshAfterUpdate` |
| 右 | Misc | Checkbox | `resultset.filter.use.navigator` |
| 右 | Misc | Checkbox | `ModelPreferences.RESULT_SET_USE_DATETIME_EDITOR`（`:287`） |
| 右 | Misc | Checkbox | `resultset.automatic.row.count` |
| 右 | UI | Checkbox（来自命令名/描述） | `resultset.confirm.beforeSave` |
| 右 | UI | Checkbox | `resultset.show.errorDialog` |
| 右 | UI | Checkbox | `resultset.mark.cell.value.occurrences` |
| 右 | UI | Checkbox | `resultset.image.browser` |

### 8.4 未取证的三页（下一批）

- `PrefPageResultSetPresentation`：呈现方式选择（网格/文本/记录…）与默认呈现，键 `resultset.presentation.active`（`ResultSetPreferences.java:87`）。
- `PrefPageResultSetPresentationPlainText`：Tab 大小、最大列长度、值格式、NULL 显示、四条分隔线（leading/trailing/top/bottom）、额外空格、行号（键见 `:97-106`）。
- `PrefPageDataViewer` / `PrefPageDataFormat`：值查看器与数据格式（日期/数字格式），键在 `model` 侧。

## 9. 待确认表增补（Q13-Q16，接在 §4 之后）

| # | 问题 | 影响 | 建议 |
| --- | --- | --- | --- |
| Q13 | 工具栏右段 6 项是否全保留（你只点名 3 项） | 工具栏最终宽度与顺序 | 默认全保留，除非你要裁掉 `下发源库` / `提交撤销` / `复制` |
| Q14 | 是否补 DBeaver 筛选栏右端 5 个 ToolItem（清空▾/自定义▾/后退▾/前进▾/查找替换） | 工具栏从两段变三段 | 建议补「查找替换」与「清空筛选▾」，历史前后可选 |
| Q15 | `font_height` 取 `line_height` 还是 `font_size × 系数` | 行高/表头高/段大小框宽/状态标签宽全部依赖它 | 取 `line_height` |
| Q16 | 设置页在项目内的承载形态 | 现有「结果集设置」浮层（齿轮入口，`grid/status.rs:931-934`）要扩成 6 页分栏，还是先做 P0 网格页 | 建议：先做 §8.1（17 项）+ §8.3（主设置），其余按批 |

## 10. 拍板补记（Q13-Q16 已关闭）

### 10.1 Q13：工具栏右段裁剪

保留 3 项：`comments`（列注释）、`compare`（表对比）、`analysis`（分析 ▾）。
裁掉 3 项：`pushdown`（下发源库）、`commit`（提交/撤销）、`copy`（复制）。

连带影响（逐项核实）：

| 项 | 是否有替代入口 | 证据 |
| --- | --- | --- |
| `pushdown` | **没有**。唯一 UI 入口就是工具栏那颗 toggle | `ResultRequest::SetPushdown` 全仓只被 `panel/render.rs:646` 触发；无常驻键位、无右键项 |
| `commit` | 有。状态栏「写操作」组已含 Commit / Reject | `grid/status.rs:708-712`（`StatusCommand::Commit` / `Reject`，可配置显隐）；键位 `ctrl-s` |
| `copy` | 有。键位与右键 | `ctrl-c → CopyGridSelection`（`crates/app/src/main.rs`） |

因此裁 `commit` / `copy` 无功能损失；裁 `pushdown` 会让「下发源库」在 UI 上彻底不可达。
**Q17（新）**：`pushdown` 是直接删掉，还是挪到状态栏系统段（`StatusCommand` 新增变体、默认不显示、用户可在 ▾ 里勾出）？建议后者。

### 10.2 Q14：补 5 个 ToolItem

位置：**筛选复合体内部末尾**（DBeaver 里是 `filterComposite` 的第 5 格 `historyPanel`，`ResultSetFilterPanel.java:214`），不是工具栏中段。

补齐顺序（严格按 `:319-356`）：

| 顺序 | 件 | SWT 风格 | 语义 |
| --- | --- | --- | --- |
| 1 | `filtersClearButton` | `NO_FOCUS \| DROP_DOWN` | 清空筛选（带下拉） |
| 2 | `filtersCustomButton` | `NO_FOCUS \| DROP_DOWN` | 自定义筛选（带下拉） |
| 3 | `historyBackButton` | `DROP_DOWN \| NO_FOCUS` | 历史后退（带下拉，菜单最大高 200） |
| 4 | `historyForwardButton` | `DROP_DOWN \| NO_FOCUS` | 历史前进（同上） |
| 5 | `openFindReplaceButton` | `CHECK \| NO_FOCUS` | 查找/替换开关（可勾选态） |

结构影响：筛选复合体从 4 格变 5 格；`MAX_HISTORY_PANEL_HEIGHT = 200`（`ResultSetFilterPanel.java:104`）。

### 10.3 Q15：字体基准

取 `TextStyle::line_height`，在 `ui.rs` 建全仓唯一来源 `fn font_height(cx: &App) -> Pixels`。派生：
- 行高 = `line_height + 3`
- 表头高 = `line_height + 12`
- 段大小框宽 = `5 × line_height`、状态消息标签宽 = `50 × line_height`（待状态条改造时用）

### 10.4 Q16：设置浮层分栏

先更正一处事实：DBeaver 是 **7 个 page**（`plugin.xml:1526-1555`），上一轮把 `PrefPageDataFormat` 与 `PrefPageDataViewer` 合并成 1 页、说成 6 页。
按「6 页」落地即合并后两项为「值格式」一页：

| 页 | 来源类 | 项数 | 优先级 |
| --- | --- | --- | --- |
| 结果集设置 | `PrefPageResultSetMain` | 23 | P1 |
| 值编辑器 | `PrefPageResultSetEditors` | 13 | P1 |
| 呈现 | `PrefPageResultSetPresentation` | 未取证 | P2 |
| 网格 | `PrefPageResultSetPresentationGrid` | 17 | **P0** |
| 纯文本 | `PrefPageResultSetPresentationPlainText` | 未取证 | P2 |
| 值格式 | `PrefPageDataFormat` + `PrefPageDataViewer` | 未取证 | P2 |

承载：现有「结果集设置」浮层（齿轮入口 `grid/status.rs:931-934`；视图在 `panel/render.rs` 的 `render_filter_settings`）改为「左页列 + 右内容」两栏，默认落在「网格」页。

## 11. 实施任务分解

| # | 任务 | 涉及文件 | 前置 |
| --- | --- | --- | --- |
| T1 | 裁工具栏 3 项（pushdown / commit / copy） | `panel/render.rs:634-660, 1204-1213`；`grid/status.rs:322-427` | 无 |
| T2 | `pushdown` 迁状态栏（若选挪） | `grid/status.rs:434-441, 700-830`；`ports.rs` | Q17 |
| T3 | 补 5 个 ToolItem（筛选复合体 5 格） | `panel/render.rs:634-660`；`grid/status.rs:367-380` | 无 |
| T4 | 行高/表头高改算式 + 逐列 `paddings` | `ui.rs:168-179`；`grid/table_impl.rs:280-345` | Q15 已定 |
| T5 | 列宽复刻（先取证算式再改） | `grid/fit.rs`；DBeaver `Spreadsheet` / `ResultSetTable` | 取证 |
| T6 | 设置浮层扩 6 页 + 网格页 17 项 | `panel/render.rs`（settings 浮层）；`ui.rs` | 无 |
| T7 | 补取证：列宽算法、`COLUMN_HEADER_EXTRA` 默认档、设置 3 页 | `dbeaver-ref` | 无 |

建议顺序：**T7 先做** → T1+T3 一批 → T4 一批 → T6 一批 → T5。T7 不产出代码，先把三个缺口钉死，避免 T5/T6 中途返工。

## 12. 拍板补记二（Q17 关闭；compare / analysis 迁标签页右键）

### 12.1 Q17：删除 `pushdown`

理由（按你的口径成立）：下发源库 = 在源库执行 SQL；而筛选框（`filtersText` 对应本仓筛选输入）本身已经是「带 WHERE 的重查」，两套入口语义重叠。

删除范围建议分两步，避免一次牵动过多链路：
1. **UI 层先删**：去掉 `panel/render.rs:634-649` 的按钮与 `ResultControls.pushdown` 传参，`render_toolbar` 少一格；`self.pushdown` 恒置 `false`（这样 `panel/events.rs:357` 的 `requeue` 分支自然不再触发）。
2. **代码清理后置**（单独批次）：`ResultRequest::SetPushdown` / `PushdownFilter`（`ports.rs:115,277`）、`store/entry.rs:31-56` 的 `Pushdown` 枚举、`panel/data.rs:466` 字段——它们牵动 `editor` 侧 `set_pushdown`（`dispatch.rs:332`）与用例，清理要连测试一起改，不宜与 UI 改动同批。

### 12.2 `compare` / `analysis` 迁到结果集标签页右键菜单

**现状核实（一处关键落差）**：结果集标签页**目前没有右键菜单**，只有 tab 上的「⋯」按钮菜单（`sets.rs:38` 的 `TabActions` 只含 `on_rename` / `on_toggle_pin` / `on_close` / `on_reorder`；`sets.rs:117` 注释明确写「tab 上的『⋯』菜单与钉住按钮回调」）。因此这是一项**新增能力**，不是搬位置。

承载能力已具备的部分：
- 对比对象选取：`ResultRequest::SelectCompare(index)`（`editor/.../dispatch.rs:61`）→ `select_compare_set(index)`（`editor/.../grid.rs:493`）= `store.toggle_compare(doc, index)`，**点当前对比项即取消**。
- 分析项清单：`self.analysis_items`（`panel/render.rs:1064-1099` 已在用，每项含 `label` + `sql`），另有 `analysis::CUSTOM_LABEL`「自定义」。

建议的菜单结构（挂在 tab 元素上，`sets.rs:119` 的 `render` 内）：

```
右键某份结果集标签
├ 并排对比 ▸
│   ├ <其它结果集 A>
│   ├ <其它结果集 B>
│   └ 退出对比            （= 当前已是该对比项时执行取消）
├ 二次分析 ▸
│   ├ <聚合项 1>（= analysis_items[0].label）
│   ├ <聚合项 2>
│   ├ ──────
│   └ 自定义…             （= analysis::CUSTOM_LABEL）
├ ──────
└ （现有 ⋯ 菜单项可一并迁入或保留，待定）
```

落点：
| 改动 | 文件 |
| --- | --- |
| tab 元素挂 `.context_menu(...)` | `crates/result_panel/src/sets.rs:119`（`render` 内 tab 构造） |
| `TabActions` 扩字段（带上下文菜单所需数据/回调：`compare_targets`、`analysis_items`、`on_select_compare`、`on_analysis`） | `sets.rs:38` |
| 调用方传参 | `panel/render.rs:113-124`（`result_sets::render` 的 `actions`） |
| 请求分派（已有的复用） | `ResultRequest::SelectCompare` / `Analysis { sql }` / `AnalysisCustom` |

**Q18**：对比子菜单的两种形态，选哪个？
- A.「设为对比对象」单项（点其它结果集 = 设定；点当前对比项 = 取消）—— 与现有 `toggle_compare` 语义完全一致，改动最小。推荐。
- B.「与 X 对比」显式列出，另加独立「退出对比」项 —— 语义更直白，但要新增 `ExitCompare` 请求。

### 12.3 工具栏最终形态（右段只剩 1 项）

```
左段（flex_1）                                          右段
├ ①「显示 SQL」chip                                    └ comments（列注释 switch）
├ ② 筛选输入框
├ ③ 展开/收起
├ ④ 运行 ▶
└ ⑤ 5 个 ToolItem：清空筛选▾ / 自定义筛选▾ / 历史后退▾ / 历史前进▾ / 查找替换 toggle
```

注意：右段只剩 1 项后，`grid/status.rs:411-421` 的判据是「contents 与 actions 都非空才插竖分隔线」——`pushdown` 删除后 `contents` 只剩 `comments`、`actions` 为空，分隔线不会插；但若把 `comments` 归入 `actions`，则会插出一条孤线。实现时要把 `comments` 明确归到哪一组写死，并配一条用例钉住「只剩 1 项时不出现孤线」。

### 12.4 任务分解更新（取代 §11 相应行）

| # | 任务 | 变更 |
| --- | --- | --- |
| T1 | ~~裁 3 项~~ → **删 `pushdown`；迁 `compare` / `analysis` 出工具栏** | `panel/render.rs:634-649, 677-691, 1064-1099, 1204-1213`；`grid/status.rs:322-427` |
| T1b | `pushdown` 代码清理（与 T1 分开提交） | `ports.rs:115,277`；`store/entry.rs:31-56`；`panel/data.rs:466`；`editor/.../dispatch.rs:332` |
| T3 | 补 5 个 ToolItem（筛选复合体 5 格） | 不变 |
| T8 | **新增** tab 右键菜单（对比子菜单 + 分析子菜单） | `sets.rs:38,119`；`panel/render.rs:113-124` |
| T2 | ~~pushdown 迁状态栏~~ → 取消（改为删除） | — |

顺序：T7 取证 → T1+T1b（分两次提交）→ T3 → T8 → T4 → T6 → T5。

## 13. 分析入口真值（08 原型）与 pushdown 论证加强

### 13.1 08 原型已经落地，不是待做项

`crates/editor/src/view/dialogs/result_analyze.rs`（131 行，首行注释即「08 原型『分析这份结果集』落地」，2026-11-16 拍板）。结构：

| 原型元素 | 实现位置 | 是否一致 |
| --- | --- | --- |
| 标题「分析这份结果集」 | `:127` `dialog.title(...)` | 一致 |
| 对象名输入（默认值 = 结果集标签） | `:49-53`、`:71-76`（`Input` + `InputState`，placeholder「给这份结果集起个名字」） | 一致 |
| 临时 / 持久 去向 | `:101-124` 两个按钮「存为临时表」(Primary) /「存入项目库」(Ghost) | **形态不同**（见下） |
| 事实格（列 / 行 / 来源 / 版本） | `:62-65` 一行 summary「label · N 行 · M 列」 | **少 2 格** |
| 表名样例提示 | `:77-94` 三条静态规则文字 | **无实例名** |
| footer 取消 / 存入项目库 / 存为临时表 | `:129` | 一致（顺序相同） |

事实来源 `ResultSetFacts { label, rows, columns }`（`:28-35`），在事件路径算好，render 不读结果集——符合「只做 UI 层，数据外部传入」。

**三处偏差，全部来自实现的主动简化，需拍板是否补齐（Q20）**：
1. **去向形态**：原型是两个 radio 卡片（`存为临时表` / `存入项目库`，`.mode` / `.mode.on` 高亮）；实现是「两个按钮即提交」，代码注释明写「不引入框内单选状态」（`:101`）。功能等价，但少了「先选再看」的一步确认。
2. **facts 格数**：原型 4 格（列 / 行 / 来源 / 版本）；实现只有「N 行 · M 列」，**没有「来源」与「版本」**。原型里「来源」= 结果 3、「版本」= 当前一版，二者对判断「这份结果的出处」有意义。
3. **表名样例**：原型 hint 直接给实例名（临时 `tmp_q_result_3_8f2a`、持久 `res_结果_3_订单明细_20261115104200`），用户能看到「我起的名字会变成什么」；实现只有三条规则文字（只有 ASCII / 中文折下划线 / 持久保留中文），**不显示实例名**。这条信息量损失最明显。

### 13.2 现状是双入口，且都不在标签页右键

| 入口 | 位置 | 点击后 |
| --- | --- | --- |
| 工具栏「分析」下拉 | `panel/render.rs:1064-1099`（`analysis_items` 非空才摆） | 每项 → `Analysis { sql }`；`自定义分析 SQL…` → `AnalysisCustom`。**不进 08 对话框** |
| 网格单元格右键「分析此结果集」 | `grid/menu.rs:1052`、`:1188`；hook 挂载 `editor/.../construct.rs:573-574` | → `panel.analyze_result_set`（`sort.rs:67`）→ **打开 08 对话框** |
| 可用性判据 | `grid/delegate.rs:689-690`：有 hook 且 `view_len() > 0` | 已有用例 `grid/tests.rs:2024` |

结论：你说的「点分析应进 08 对话框」**目前只在网格右键成立**；工具栏那颗仍走旧下拉。上一轮定的「迁入标签页右键」正好把这个不一致收掉。

### 13.3 迁移方案（修订 §12.2 的菜单结构）

标签页右键菜单（新增，落点 `sets.rs:119`）：

```
分析这份结果集…   → panel.analyze_result_set（复用现链，零新增后端）
自定义分析 SQL…   → ResultRequest::AnalysisCustom
并排对比 ▸ <其它结果集…> / 退出对比
```

预置聚合项（`analysis_items`）**不直接铺进右键菜单**——否则菜单会被 8+ 条聚合项撑长，且这些条目本质是「预设 SQL 快捷方式」，与「命名 + 去向」是不同的动作层级。→ **Q19**：预置聚合项放哪？
- A（推荐）：右键菜单只留上面 3 条；聚合项收进 08 对话框之后的「数据对象 / EDA」侧按需使用，右键不再暴露。
- B：右键加二级「更多聚合 ▸」承载 `analysis_items`。
- C：聚合项整体废弃，统一走 08 对话框 + 数据对象 EDA。

**原型前提已变**：08 原型第 157 行写「结果集工具栏的『分析』下拉仍有自定义 SQL 与 `{table}` 替换」——这是以「分析仍在工具栏」为前提的。分析移出工具栏后，这句描述的载体消失，须登记为「原型前提已被后续拍板覆盖」，后续改原型时同步。

### 13.4 pushdown 删除的论证加强（你的补充成立）

`pushdown` 的语义是「应用筛选时拼 WHERE **重查源库**」（`panel/render.rs:643` tooltip 原文）。当执行通道不是源库（分析结果 / 联邦结果）时：
- 数据来自本进程物化的结果集，没有可「重查」的源库连接；
- `panel/events.rs:357` 的 `if requeue && self.pushdown` 分支在此时指向一个不存在的目标。

所以这不是「可选删除」，而是**在两种通道下都缺少正确语义**。实现时补一条硬约束：删 UI 入口后，该分支必须连 `exec_channel`（源库 / 分析 / 联邦）一起判，或整体移除；只把开关写死 `false` 而留下分支，等于留一个永假的判断。

## 14. 工具栏归零（完全对齐 DBeaver 筛选栏）与三处新取证

### 14.1 工具栏 5 项去向收口

| 现工具栏项 | 最终去向 | 依据 |
| --- | --- | --- |
| `pushdown`（下发源库） | **删除** | §12.1 + §13.4（通道非源库时无语义） |
| `comments`（列注释 switch） | **进设置**：呈现页「列头附加内容」三档 | §14.2（`PrefPageResultSetPresentation.java:96-110`） |
| `compare`（并排对比） | **标签页右键**「并排对比 ▸」 | §12.2 |
| `analysis`（分析下拉） | **标签页右键**「分析这份结果集…」→ 08 对话框 | §13.2 / §13.3 |
| `commit` / `copy` | 已裁（Q13）：状态栏 Commit/Reject + `ctrl-s`；`ctrl-c` | §10.1 |

结果：整条只剩筛选复合体，右段为空 → **结构上等于 DBeaver 的 `ResultSetFilterPanel`**（DBeaver 结果集本来就没有顶部工具栏，见 §0）。

**Q21（待定）**：「显示 SQL」chip 怎么办。DBeaver 筛选栏是 5 格（`activeObjectPanel` → `filterExpandPanel` → `filtersTextViewer` → `executePanel` → `historyPanel`），**没有「显示 SQL」这一格**。严格 1:1 就必须去掉（SQL 全文已能通过筛选展开条 / 右栏「执行 SQL」页签看到）。

### 14.2 「列注释进设置」的真值：它是三档下拉，不是开关

新取证 `PrefPageResultSetPresentation.java`（呈现页，11 项，单分组 `Common`）：

| # | 控件 | 标签（资源键尾） | 形态 | 默认 |
| --- | --- | --- | --- | --- |
| 1 | `autoSwitchMode` | `switch_mode_on_rows` | checkbox | false |
| 2 | `showFiltersInSingleTabMode` | `filters_panel_in_singletab_mode` | checkbox | true |
| 3 | `showFilterPanel` | `show_filter_panel` | checkbox | true |
| 4 | **`columnHeaderExtra`** | `column_header_extra` | **Combo（只读）3 档** | **NOTHING** |
| 5 | `columnWidthByValue` | `calc_column_width_by_values` | checkbox + tip | **false** |
| 6 | `showConnectionName` | `show_connection_name` | checkbox | false |
| 7 | `transformComplexTypes` | `structurize_complex_types` | checkbox + tip | false |
| 8 | `rightJustifyNumbers` | `right_justify_numbers_and_date` | checkbox | false |
| 9 | `rightJustifyDateTime` | `right_justify_datetime` | checkbox | false |
| 10 | `autoCompleteProposal` | `auto_completion` | checkbox + tip | true |

（`UIUtils.createCheckbox` 第 4 个参数是 performDefaults 的值，实际初值由 `loadPreferences` 覆盖。）

**两个直接影响**：

a) **`show_column_comments: bool` 要改成三档枚举**（`NOTHING` / `DESCRIPTION` / `DATA_TYPE`，对应 `ResultSetPreferences.ColumnHeaderExtraContent`）。我们的「列注释」= `DESCRIPTION`。
b) **长期待确认项「COLUMN_HEADER_EXTRA 默认档」已解**：默认 `NOTHING`（`:143-149`、`:203-208` 两处兜底值都是 NOTHING）→ **表头默认只有 1 行**，第二行 `+ font_height + 6` 只在选 DESCRIPTION / DATA_TYPE 时才出现。这改写了 §3.2 的「表头高一律 2 行」推断。

**Q22（要你定，重要）**：`columnWidthByValue` 在 DBeaver 里**默认 false**（不按值算列宽），而你此前拍板「列宽复刻内容自适应」。二者冲突，必须选一个：
- A（DBeaver 原样）：默认 false，用户去设置里开 —— 1:1 复刻口径，但开箱后列宽是类型的默认宽度。
- B（我们的选择）：默认 true —— 视觉更像 DBeaver 日常用起来的样子（多数人装完会开），但默认值偏离源码。
- 我建议 A，理由：你的硬约束是「尺寸必须取自 DBeaver 源码原始值」，默认值也属于源码事实。

### 14.3 右键菜单收敛为 2 项；「自定义分析 SQL」的可达性已核实

按你的口径，标签页右键最终只有两项：

```
分析这份结果集…   （→ 08 对话框）
并排对比 ▸
```

**核实结果（一处必须记下的差异）**：筛选框 ▶ 运行不是「把筛选词当完整 SQL 跑」，而是 **`select * from (原SQL) where 词` 包装**（`dispatch.rs:170-171` 注释原文），且生成后的 SQL 会写入右栏「执行 SQL」页签。所以：

- 筛选框**只能约束行，不能聚合**（做不了 `GROUP BY` / `SUM`）；
- 但它生成的 SQL 进「执行 SQL」页签后**可改写** → 这是「自定义 SQL 仍然可达」的实际路径；
- 原 `AnalysisCustom` 能力（对**已物化结果集**跑自定义 SQL）失去入口后，等价路径变成：右键「分析这份结果集…」→ 物化成 `tmp_q_` → 在数据对象 / 执行 SQL 侧写聚合。**多一步，但能力没丢**。

这条要写进验收清单，避免日后被当成「功能回退」。

**Q19 随之收敛**：预置聚合项 `analysis_items` 失去唯一入口（工具栏下拉消失、右键不放）。建议按 **C（废弃 UI 入口）**，其数据来源（洞察侧的预置项）保留给数据对象 / EDA 侧使用。

### 14.4 08 原型补「来源 SQL」（你的新要求）

现状 `ResultSetFacts { label, rows, columns }`（`result_analyze.rs:28-35`），数据从 `active_result_set_facts` 算（`sort.rs:69`）。补法：

| 改动 | 位置 |
| --- | --- |
| `ResultSetFacts` 增 `sql: Option<String>` | `result_analyze.rs:28-35` |
| 取数（面板字段已有 `result_sql: Option<String>`） | `panel/data.rs:385`；`sort.rs:69` 的 facts 组装处带上 |
| 展示 | 对话框 body 内独立一段：标签「来源 SQL」+ 等宽 1~2 行截断 + 悬停/点击看全文 + 复制 |

为什么不塞进 facts 小格：原型 facts 是 4 个窄方块（列 / 行 / 来源 / 版本），SQL 是长文本，塞进去必然截成无意义的片段。建议把原型「来源」格改为「来源：结果 3」保留，SQL 另起一段。

无 SQL 的情况（手输数据 / 空结果 / 分析结果自身无源 SQL）→ 整段隐藏，不留空框。

**Q23（要你定）**：SQL 段默认显示几行？建议 2 行 + 「查看全文」（3 行会挤掉 09 的原型比例）。

### 14.5 任务分解更新（第三次修订）

| # | 任务 | 范围 |
| --- | --- | --- |
| T1 | 工具栏收口：删 `pushdown`、迁 `compare`/`analysis` 出工具栏、`comments` 出工具栏 | `render.rs:634-691,1064-1099,1204-1213`；`status.rs:322-427` |
| T1b | `pushdown` 代码清理（单独提交；连 `exec_channel` 一起判） | `ports.rs:115,277`；`store/entry.rs:31-56`；`panel/data.rs:466`；`events.rs:357`；`editor/.../dispatch.rs:332` |
| T2 | ~~pushdown 迁状态栏~~ 取消 | — |
| T3 | 筛选栏补 5 个 ToolItem | `status.rs` 左段 |
| T4 | 行高 / 表头高按字体度量（表头默认 1 行，三档才 2 行） | `ui.rs` + `table_impl.rs` |
| T5 | 列宽（按 Q22 结论） | 待定 |
| T6 | 设置浮层 6 页；呈现页 10 项已取证（§14.2） | `dialog.rs` |
| T8 | 标签页右键菜单（2 项） | `sets.rs:38,119`；`render.rs:113-124` |
| T9 | 08 对话框补「来源 SQL」+ 三处原型偏差（待 Q20） | `result_analyze.rs` |

## 15. 左右侧边栏与状态栏对齐（新取证）

> 口径：此处的「左右侧边栏」指结果集面板自身的两条竖条（左缘 `VerticalFolder(LEFT)`、右缘 `VerticalFolder(RIGHT)`）与右栏页签组，不含工作台级左右栏。

### 15.1 左缘竖条：结构已对齐，只剩 1 项本仓扩展

DBeaver（`ResultSetViewer.java:320-326`、`:923-970`）：
- `presentationSwitchFolder = new VerticalFolder(mainPanel, SWT.LEFT)`，`FILL_VERTICAL`，条件是 `FEATURE_PRESENTATIONS`；
- 内容按 `availablePresentations` 逐个建 **`VerticalButton(SWT.LEFT | SWT.RADIO)`**（`:949`），点选即换呈现方式；
- 中间塞 1×1 `FILL_VERTICAL` 空标签当弹簧（`:963`）；
- 底部 `recordModeButton = VerticalButton(SWT.LEFT | SWT.CHECK)`（`:964`）→ 记录模式。

我方 `sidebars.rs:107-206`：Grid / Text 两颗（`selected` + `toggled` 表达 radio）、Record（checkbox）、`pt_2`，并且注释已按 2026-11-18 结构对齐（`sidebars.rs:198-202` 逐条引 DBeaver 行号）。

差异只有 1 条：记录视图激活后，我方在 Record 下方追加「上一行 / 下一行」导航（`sidebars.rs:153-181`）——DBeaver 没有这对按钮（它靠导航命令）。**登记为本仓扩展，不需要改**。

### 15.2 右缘竖条：结构已对齐

DBeaver（`:1013-1083`）：顶部 `panelsButton`（CHECK，图标 `UIIcon.PANEL_CUSTOMIZE`，`:1038`）→ 每个 panel 一颗 CHECK（`:1055-1080`，勾选态 = 该页打开）→ 弹簧（`:1082`）→ 底部 `CMD_TOGGLE_LAYOUT`（`:1083`）。

我方 `sidebars.rs:692-787`：总开关 → `right_tabs()` 全集逐页 CHECK → `flex_1` 弹簧 → 竖排切换；每处都带 DBeaver 行号注释。**一致，不改。**

### 15.3 右栏页签：清单对得上，但两页有语义/功能落差

DBeaver 右侧页签来自扩展点 `ResultSetPanelDescriptor`（`ResultSetPresentationRegistry.java:65-68`），`panel/` 目录实证只有 5 个实现：

| DBeaver | 实现类 | 我方 `RightTab` | 判定 |
| --- | --- | --- | --- |
| metadata | `MetaDataPanel.java` | `Browser` | 对齐（列清单 + 底部元数据明细） |
| valueviewer | `ValueViewerPanel.java` | `Value` | 对齐 |
| aggregate | `AggregateColumnsPanel.java` | `Stats` | **待重做（§16.1 已更正）**：DBeaver 是「函数 / 值」两列 Tree，函数集可配置启用与顺序、可开 `groupByColumns` 按列分组；我方是固定 5 项指标。 |
| grouping | `GroupingPanel.java`（含 `GroupingConfigDialog`、多列 + 函数列） | `Group` | **部分对齐**：我方只有单列本地 groupby 计数（`ports.rs:853` 注释自陈「DBeaver 大表口径是下发 GROUP BY」） |
| references | `ReferencesPanel.java` / `ReferencesResultsContainer.java` | `Reference` | **功能缺口**：我方是空态 backlog（`ports.rs:855-856` 自陈） |
| （无） | — | `ExecSql` | 本仓扩展（DBeaver 无「执行 SQL」页），登记 |

**Q24**：`Stats` 怎么收？
- A（推荐）：改名为「聚合」并把结果叠加到网格底部（对齐 DBeaver `AggregateColumnsPanel`），侧栏那份 count/null/min/max 并入其中。
- B：保留侧栏统计，同时新增「叠加到网格底部」的那一层 —— 等于两套。
- C：只改名不动行为（等于承认语义不同，但名字改成「统计」避免误会）。

**Q25**：`Reference` 页是要补实现，还是明确标注「未实现」保留空态？（DBeaver 那边有完整外键引用查询，我们只有空态。）

### 15.4 状态栏：段数都是 10，但顺序与宽度口径不同

DBeaver（`createStatusBar0`，`ResultSetViewer.java:2043-2195`，`RowLayout: marginLeft=0, center/wrap/pack=true`）：

| 段 | DBeaver | 我方（`grid/status.rs:840-935`） | 差异 |
| 1 | Refresh（AutoRefresh 开关） | 动作组内的 `StatusCommand::Refresh` | **位置不同**：DBeaver 在最左独立一项 |
| 2 | editCmds（applyChanges / rejectChanges / undo / redo / row.edit / row.add / row.copy / row.delete） | 左·动作组（写操作） | 位置不同，内容齐 |
| 3 | navCmds（first / prev / next / last / fetch.page / fetch.all） | 左·动作组（翻页） | 位置不同，内容齐 |
| 4 | 空 config（`ToolBar` 占位） | 无 | 我方无此占位 |
| 5 | export + `GroupMarker(presentations)` + `Separator(additions)` + `toolbar:…status` | Export 在动作组；`presentations` 组缺 | **缺 presentation 贡献点**：DBeaver 把「显示控制」贡献在状态栏这条工具条上 |
| 6 | config（竖分隔 + `ConfigAction` 齿轮） | 配置 ▾ + 设置齿轮 | 我方多一个 ▾ |
| 7 | `resultSetSize` Text（宽 `5*fontHeight`） | 段大小框（固定 `RESULT_SEGMENT_BOX_WIDTH=64`） | **宽度口径不同**：DBeaver 按字高算（13px 字号≈65px） |
| 8 | `rowCountLabel`（`ActiveStatusMessage`，`UIIcon.COMPILE`） | 行数框（固定 72） | 同上 |
| 9 | `selectionStatLabel` | 选中统计 | 对齐 |
| 10 | `StatusLabel`（Label + 详情 ToolItem，宽 `50*fontHeight`≈650px） | 元信息三项（内存 / 耗时 / 查询条数，`ml_auto`） | **形态差最大**：DBeaver 是最右一个很宽的复合 StatusLabel，我方是三项分散小文本 |

我方多出（DBeaver 无）：左栏开关、「计算行数」按钮、状态栏右端的右栏开关（与右缘竖条双入口，`render.rs:671-673` 注释已承认是双入口）。

**Q26**：状态栏是否按 DBeaver 顺序重排？建议重排为：`Refresh → edit → nav → export(+presentations 占位) → config → 段大小 → 行数 → 选中统计 → StatusLabel`，并裁掉「左栏开关」「计算行数」「状态栏右栏开关」（右栏开关只留竖条那颗）。

**Q27**：第 10 段要不要做成 DBeaver 那种「一个宽 StatusLabel + 详情弹窗」？还是保留三项分开（信息密度更高，但与源码不同形）？

### 15.5 尺寸口径统一（并回 T4）

两处状态栏宽度都按字高算，不是固定像素：
- `resultSetSize` = `5 * fontHeight`
- `StatusLabel` = `50 * fontHeight`

按 §7.2 已定的 `font_height(cx)` 单一来源，`RESULT_SEGMENT_BOX_WIDTH` / `RESULT_ROW_COUNT_BOX_WIDTH` 应由固定 64/72 改为 `5*fh` / 待定（DBeaver 的 rowCountLabel 无显式宽度，`ActiveStatusMessage` 自适）——实现时以 `5*fh` 起算，`StatusLabel` 若走 Q27-A 则取 `50*fh`。

### 15.6 任务分解（第四次修订，仅列新增）

| # | 任务 | 范围 |
| --- | --- | --- |
| T10 | 左竖条：仅登记「记录上下行导航」为本仓扩展，无代码改动 | — |
| T11 | 右栏页签：`Stats` 语义（Q24）、`Reference` 缺口（Q25）、`ExecSql` 扩展登记 | `ports.rs:841-891`；`sidebars.rs:456-640` |
| T12 | 状态栏重排 + 裁多余入口 + 宽度按字高（Q26/Q27） | `grid/status.rs:840-935`；`ui.rs:38-46` |
| T13 | （并入 T4）`font_height` 作为状态栏/表格共用单一来源 | `ui.rs` |

## 16. 右栏 5 页严格对齐：逐页清单（含 §15.3 更正）

### 16.1 更正：aggregate 不在网格底部，是右栏一棵两列 Tree

上一轮 §15.3 我按印象写成「选列做聚合、结果叠加在网格底部」，源码证伪，现更正。真值（`AggregateColumnsPanel.java`）：
- 内容 = `Tree(SINGLE | FULL_SELECTION)`，两列：函数名 / 值（`aggregate_columns_function_text`、`aggregate_columns_value_text`，`:90-94`）；
- 行 = **启用的聚合函数**，来源 `FunctionsRegistry.getAggregateFunctions()`，启用状态与顺序存 panel settings（`:129-164`），默认集来自 `func.isDefault()`（`:157-160`）；
- 两个会话开关：`groupByColumns`（`:231-249`：每个选中列建一个父节点，子节点是各函数值）、`aggregateAsStrings`（按字符串聚合）；
- 计算全在客户端（`model.getCellValue` → `aggregateValues`），**不拼 SQL**；
- 右键 Copy / CopyAll（`:106-116`）；工具条由 `contributeActions()` 贡献（`:216-219`）。

→ 我方 `Stats` 的改造方向因此确定：**保持侧栏**，把固定 5 项换成「可配置函数集的 Tree」，并补两个开关。不存在「搬到网格底部」这件事。

### 16.2 grouping 真值是「右栏里嵌第二个结果集」

`GroupingPanel`：内容容器 `GroupingResultsContainer`，内部 `getResultSetController()` —— 右栏嵌了一个**独立结果集查看器**显示分组结果（`:78-102`）；`isDirty()` = 分组属性非空（`:105-107`）；配置对话框 `GroupingConfigDialog`，函数列有 4 类实现（`BasicGroupingFunctionColumn` / `PercentGroupingFunctionColumn` / `TransformerGroupingFunctionColumn` / `SQLGroupingAttributeGroupingColumn`）。

与你的判断一致：分组 = 按配置拼 GROUP BY 查出结果，当新结果集显示。我方 `Group` 差三层：列多选、函数可选、结果要以表格呈现。

### 16.3 五页逐页清单

| 页 | DBeaver 结构 | 我方现状 | 要做 | 量级 |
| --- | --- | --- | --- | --- |
| metadata | `MetaDataPanel`：列清单 + 属性明细 | `Browser` 已有 | 逐控件比对（搜索框 / 列清单 / 属性表），补齐差异 | 小 |
| valueviewer | `ValueViewerPanel` + `ValueViewCommandHandler` 命令集 | `Value` 已有 | 比对页内工具条命令 | 小 |
| aggregate | 函数/值两列 Tree + 函数配置 + `groupByColumns` + `aggregateAsStrings` | `Stats` 固定 5 指标 | 重做为可配置函数 Tree | 大 |
| grouping | 内嵌结果集 + `GroupingConfigDialog` + 4 类函数列 | `Group` 单列本地计数 | 重做（下发 GROUP BY + 表格呈现） | 大 |
| references | `ReferencesPanel` 外键引用表 | `Reference` 空态 | 按你要求**置灰** | 极小 |

**references 置灰的一个事实差异（必须登记）**：DBeaver 的页签是**按可用性过滤**的——`ResultSetPresentationRegistry.getSupportedPanels(context, dataSource, presentationId, presentationType)`（`:123-124`）筛过之后才出现在页签条与右竖条上，不支持就根本不出现。我们「保留页签但置灰」是更可发现的取舍（用户看得见这页存在、只是不可用），但**不是 1:1**。置灰实现要求：竖条与页签都保留、按钮 `disabled`、点开显示空态说明，不隐藏。

### 16.4 5 页之外的源码结构点（结论已出，见 §17.3 / §17.4）

- 每个面板可自贡献页内工具条：`contributeActions(manager)`（`AggregateColumnsPanel.java:216`）、`createActionsToolbar(parent)`（`ResultSetPanelBase.java:30-32`）。我方六页**已统一走 `panel_toolbar`**（§17.3）。
- 页签容器能力：`setMinimizeVisible(true)`、`setMRUVisible(true)`、双击页签最大化 `togglePanelsMaximize()`（`ResultSetViewer.java:363-374`）、`TabFolderReorder`（`:360`）。我方四项核对结论见 §17.4。

（本节原有的「待核」已在 §17 全部核实，结论以 §17 为准。）

### 16.5 列画像 / 多列画像：作为扩展页，不塞进 5 页

DBeaver 的 5 页里**没有任何画像类页面**，所以画像只能算本仓扩展，天然不破坏 5 项对齐。

建议形态：新增 `RightTab::Profile`（第 6 页，排在 `Reference` 后、`ExecSql` 前）：
- 单列画像：空值率 / 唯一值数 / top-k / 分布直方 / 分位；
- 多列画像：相关矩阵 / 重复行 / 候选键；
- 算法复用 EDA 侧已有能力，不在结果集里新写。

为什么不并进 aggregate：aggregate 的对象是「选中的单元格值」，画像是「整列/多列分布」，两个心智混一页会互相干扰。为什么也不并进 metadata：metadata 是「列的定义与属性」，画像是「列的数据分布」，信息来源不同。

**Q28**：画像页 = 新开第 6 页（推荐），还是挂在 metadata 页下半段？
**Q29**：多列画像要不要等单列画像稳定后再开（分两批交付）？

### 16.6 任务分解（第五次修订）

| # | 任务 | 说明 |
| --- | --- | --- |
| T11a | metadata 页逐控件比对 | 小 |
| T11b | valueviewer 页逐控件比对 | 小 |
| T11c | aggregate 页重做（函数 Tree + 2 开关 + 函数配置） | 大 |
| T11d | grouping 页重做（内嵌结果集 + 配置对话框） | 大 |
| T11e | references 页置灰（保留页签 + disabled） | 极小 |
| T14 | 画像页（单列 → 多列，按 Q28/Q29） | 扩展 |

## 17. ExecSql 删除、洞察页、页内工具条与页签容器核实

### 17.1 删除 ExecSql 页（含一处必须登记的连带）

- `RightTab::ExecSql` 移出 `ALL`（`ports.rs:857-871`），落盘 key `execsql` 由 `parse()` 的未知值分支兜底丢弃（`ports.rs:887-890`），删 `render_exec_sql_tab` 与该页的请求分支。
- **连带**：筛选框 ▶ 会把生成的 wrapped SQL 写进面板 `exec_sql` 并**自动切到该页**（`dispatch.rs:170-171` 注释原文）。删页后自动切页失效 → 改为「只更新筛选展开条里的 SQL 预览，不切页」（展开条已有该预览，`sidebars.rs:791`）。此条记为有意行为变更，验收清单要写。

### 17.2 洞察页（新增）

内容三块（按你的口径）：
1. **常驻提示条**（页内，不弹窗）：「这份结果已物化到分析库临时表 `tmp_q_…`，随会话结束清理；要长期保留请走『分析这份结果集』存入项目库。」——满足「点击洞察就有心智提醒」，同时不打断操作。
2. **列洞察**：不实现。按钮 `disabled` + 文案「列画像暂不稳定，暂未开放」。
3. **进入表画像**按钮 → 见 Q30。

空态：结果尚未物化时，整页提示「先执行『分析这份结果集』」。

**Q30（关键歧义，需你定）**：「进入表画像」是去哪？
- A：切到右栏新建的 `Profile` 页（表画像在结果集面板内，需新建第 7 页）。
- B：打开数据对象面板里已有的表画像视图（右栏保持 6 页，不新增 Profile）。
成本差：A 要新写一页与算法接线；B 只做一个跳转端口，但用户会离开结果集面板。

### 17.3 【更正】页内工具条：我方已统一实现，右端视图菜单也已对齐

上一轮 §16.4 说「待核」，核实结果与我担心的相反——**已经做过了**。证据 `tabs.rs:738-787`（`panel_toolbar`）：
- 布局 `h_flex + justify_between + px_2 + h(28px)`；左列 = 标题（`max_w 14rem` 截断）+ `Toolbar::new(<id>-actions).contents(actions)`（`flex_1`，动作贴左）；右列 = `panel_view_menu(debug_id)`（贴右）。注释 `:726-730` 明确是照 DBeaver「左 `contributeActions` 吃满、右 `addDefaultPanelActions` 贴右」的列位。
- 分隔线由调用方按 `needsSeparator()` 传入，对齐 `ResultSetViewer.java:1345-1347`——全仓只有 `ValueViewerPanel:171-173` 返回 true，所以只有值页有横线。
- 引用页 `actions` 传空，与 DBeaver `ReferencesPanel:156-158` 的空实现一致。
- 视图菜单已对齐 `fillPanelsMenu:1559-1588`：页勾选 / 面板最大化 / 总开关 / 布局方向（`tabs.rs:793-797`）。

**差异只剩一处**：我方工具条左侧多一个标题（DBeaver 页名只在页签上），代码注释已登记为「保留的有意差异」。
**真正的缺口不在结构、在内容**：`actions` 由各页传入，aggregate 的「函数配置」、grouping 的「分组配置」这些本页专属动作还没传——因为这两页本身待重做，归入 T11c / T11d，不单列任务。

### 17.4 页签容器四项核对

| 能力 | DBeaver | 我方 | 结论 |
| --- | --- | --- | --- |
| 整栏最小化 | `panelFolder.setMinimizeVisible(true)`（`:363`） | 已核 `tab_bar.rs` 全部 `pub fn`（18 个），**没有 minimize 类方法**；等价入口 = 竖条总开关 `SetPanelsVisible(false)` | 功能等价、位置不同，不补 |
| MRU 按钮 | `setMRUVisible(true)`（`:364`） | 无 MRU 按钮，但有 `TabBar::menu(true)`（`sidebars.rs:526-528`）在溢出时给菜单钮，能访问被裁掉的页 | 核心价值已覆盖；差异仅「菜单是固定全集序 vs 最近使用序」（§18.3 / Q32） |
| 双击页签最大化 | 双击**已选中**页签 → `togglePanelsMaximize()`（`:366-374`） | 有 `panel_maximized` + `toggle_panel_maximize`（`events.rs:1675-1676`），入口是视图菜单勾选项（`tabs.rs:893`） | **触发方式不同**：补「双击已选中页签」即对齐 |
| 拖拽重排 | `new TabFolderReorder(panelFolder)`（`:360`） | **组件已支持**：`TabBar::on_reorder`（`tab_bar.rs:92-103`，项目私有 patch，签名 `Fn(usize, usize, &mut Window, &mut App)`）；右栏页签条未接（`sidebars.rs:521-531` 只接了 `on_click`） | **应用层未接**（§18.2 更正）；接上即可，不改 vendor |

**Q31**：拖拽重排做不做？建议暂缓（页签 ≤7 且已有 × 关闭 / 重开，改 vendor `TabBar` 的回归面大）。
**Q32**：MRU 按钮做不做？建议不做。

### 17.5 右栏最终形态（取决于 Q30）

若 Q30 选 A（表画像留在右栏）：

| # | 页 | 来源 | 状态 |
| --- | --- | --- | --- |
| 1 | Browser | metadata | 逐控件比对 |
| 2 | Value | valueviewer | 已对齐 |
| 3 | Aggregate | aggregate | 重做：函数 Tree + `groupByColumns` + `aggregateAsStrings` |
| 4 | Group | grouping | 重做：内嵌结果集 + 配置对话框 |
| 5 | Reference | references | 置灰 |
| 6 | Insight | 本仓扩展 | 新建（提示 + 表画像入口 + 列洞察置灰） |
| 7 | Profile | 本仓扩展 | 新建（表画像） |

若 Q30 选 B（表画像在数据对象侧）：上表去掉第 7 页，Insight 的按钮改为跳转端口。

（ExecSql 已删除；DBeaver 5 页全覆盖。）

### 17.6 任务分解（第六次修订，仅列新增）

| # | 任务 | 说明 |
| --- | --- | --- |
| T11f | 删 ExecSql 页 + 自动切页行为改 | `ports.rs`、`dispatch.rs`、`sidebars.rs` |
| T11g | 洞察页（提示条 + 列洞察置灰 + 表画像入口） | 依赖 Q30 |
| T11h | 双击已选中页签 → 面板最大化 | `sidebars.rs` 页签元素加双击处理 |
| T11i | 页内工具条动作内容（aggregate 配置 / grouping 配置） | 并入 T11c / T11d |
| T14 | 表画像页（仅 Q30=A 时） | 依赖 Q30 |

## 18. Q30 定稿 / Q31 更正 / Q32 MRU 释义

### 18.1 Q30 = B：表画像在数据对象侧，右栏 6 页

右栏最终形态：

| # | 页 | 来源 | 状态 |
| --- | --- | --- | --- |
| 1 | Browser | metadata | 逐控件比对 |
| 2 | Value | valueviewer | 已对齐 |
| 3 | Aggregate | aggregate | 重做：函数 Tree + `groupByColumns` + `aggregateAsStrings` |
| 4 | Group | grouping | 重做：内嵌结果集 + 配置对话框 |
| 5 | Reference | references | 置灰 |
| 6 | Insight | 本仓扩展 | 新建：提示条 + 列洞察置灰 + 「进入表画像」跳转端口 |

- `ExecSql` 删除（§17.1）。
- `Profile` 页**不建**（`RightTab::ALL` 保持 6 项）。
- Insight 的「进入表画像」= 通过端口打开**数据对象面板**的表画像视图，不在结果集面板内渲染画像（Q30-B）。

### 18.2 【更正】拖拽重排：组件支持，我方应用层没接

上一轮 §17.4 我写「`TabBar` 无 reorder，需改 vendor」——**错了，是我 grep 漏了**（只匹配 `reorder`，方法名实际是 `on_reorder`）。真值：

`vendor/gpui-component/src/tab/tab_bar.rs:92-103`
```
/// 【项目私有 patch】让整条标签条可拖拽重排。
/// 只登记一次，渲染时下发给每个标签（每个标签自己知道自己第几个）。
/// 对应 DBeaver 在结果集标签条上装 `new TabFolderReorder(resultTabs)`
/// （`SQLEditor.java:1518`）——能力属于整条标签条，不属于单个标签。
pub fn on_reorder(
    mut self,
    handler: impl Fn(usize, usize, &mut Window, &mut App) + 'static,
) -> Self
```

即：**能力早就为对齐 DBeaver `TabFolderReorder` 加过了**（私有 patch），签名是 `(from_index, to_index)`。

我方右栏页签条的现状：`sidebars.rs:521-531` 只接了 `on_click`，**没接 `on_reorder`** → 所以右栏页签不能拖。
要做的只有应用层接线：`on_reorder` → 新增 `ResultRequest::ReorderRightTab { from, to }` → 重排 `open_tabs` 并落盘顺序（现有 `open_tabs` 偏好键即可承载）。不涉及 vendor 改动。

### 18.3 Q32：MRU 是什么（SWT 官方定义 + 我方已有等价）

**SWT 定义**（`CTabFolder.setMRUVisible(boolean show)`，Eclipse Platform API Spec）：
- 默认行为：页签条水平空间不够时，按 **index 从左到右**依次显示，靠后的被隐藏；
- 开启 MRU 后：改为优先显示**最近使用过的**页签，并在页签条上显示一颗 **MRU 按钮**，点开列出最近使用过的页签（用于访问被隐藏的那些）。
- 它**不影响页签顺序**，只影响「空间不足时谁被藏起来」以及那颗按钮。

**DBeaver 的用法**：只在两处开启——结果集面板 `panelFolder.setMRUVisible(true)`（`ResultSetViewer.java:364`）、多页编辑器 `tabFolder.setMRUVisible(true)`（`MultiPageAbstractEditor.java:147`）。仓库内无其他说明性注释，以上语义来自 SWT 官方 API 文档（非本仓源码实证，标注来源）。

**我方等价物**：`TabBar.menu(true)`（`sidebars.rs:526-528`）——溢出时给一颗菜单钮，能访问被裁掉的页。核心价值（「被隐藏的页仍可达」）已经满足。

**差异只有一处**：我们的菜单是**固定全集顺序**，SWT MRU 是**最近使用序**。

→ **Q32 选项**：
- A：不做（现有 `menu(true)` 够用）。
- B：把溢出菜单改成按「最近激活序」排序（等价 MRU）。成本：维护一个 `recent_tab_order`（6 项小数组），菜单渲染时按它排。推荐 B，因为它顺带让「最常用的一页」在没有可见页签时更容易点到。

### 18.4 任务分解（第七次修订）

| # | 任务 | 变更 |
| --- | --- | --- |
| T11f | 删 ExecSql + 自动切页行为改 | 不变 |
| T11g | 洞察页（提示条 + 列洞察置灰 + 表画像跳转端口） | 按 Q30-B：只做端口 |
| T11h | 双击已选中页签 → 面板最大化 | 不变 |
| T11j | 右栏页签条接 `on_reorder` + 顺序落盘 | **新增**（应用层接线，不改 vendor） |
| T11k | 溢出菜单按最近使用排序（MRU 等价） | 待 Q32 决定 |
| T14 | ~~表画像页~~ | **取消**（Q30-B） |

## 19. 结果标签页现状（含右键缺口）

### 19.1 现有能力（逐项带位置）

| 能力 | 实现 | 位置 |
| --- | --- | --- |
| 切换结果集 | `TabBar.on_click` → 选中下标 | `sets.rs:119-126` |
| 重命名 | 标签 suffix 的「⋯」小按钮（tooltip「重命名结果集」）→ `BeginRename(usize)` → 输入 → `CommitRename` / `CancelRename` → `RenameSet(usize, Option<String>)` | `sets.rs:153-166`；`ports.rs:396,425,427,429` |
| 钉住 | 标签 prefix 小图钉 → `TogglePin(usize)`；语义 = 挡自动淘汰（钉住仍可手动关） | `sets.rs:41-43,184-186`；`ports.rs:408` |
| 关闭 | 标签 suffix 最右 ×（对齐 `resultsTab.setShowClose(true)`） | `sets.rs:168-183` |
| 拖拽重排 | `TabActions.on_reorder` → 槽位整份搬、选中项跟着走（对齐 `SQLEditor.java:1518` 的 `TabFolderReorder`） | `sets.rs:47-49`；`ports.rs:414` |
| 通道徽标 | `source/加速/联邦`；旧结果缀 `·旧` 且标灰 | `sets.rs:59-65,79-85` |
| 悬停看原始 SQL | `ResultSetTab.sql` 进 tooltip（对齐 `MultiTabsQueryResultsContainer.java:96-103`） | `sets.rs:68-71` |
| 失败结果 | 标签带 `danger` 点 | `sets.rs:57-58` |
| 溢出 | `TabBar.menu(true)` 更多菜单 + `track_scroll` 点击滚入视野 | `sets.rs:112-118` |

### 19.2 没有的能力

1. **没有右键菜单**。标签上只有三颗东西：「⋯」（其实是**重命名**钮）、图钉、×。vendor 的 `Tab::on_context_menu` 能力存在（`tab.rs:733-745`），`sets.rs` 未接。
2. 标签不显示行数 / 耗时（`ResultSetTab` 只有 label / failed / channel / stale / pinned / sql 六个字段）。
3. 没有标签级「关闭其它结果」这类批量动作。

### 19.3 右键菜单挂什么（与 §12.2、§17 一致）

```
分析这份结果集…      → panel.analyze_result_set（打开 08 对话框，复用现链）
并排对比 ▸           → 仅 tabs ≥ 2 时出现；点其它结果 = 设为对比对象，点当前对比项 = 取消
```

**Q33**：右键要不要顺带收编「重命名 / 钉住 / 关闭」？
- A（推荐）：不收。这三样在标签上是可见小按钮，右键再放一份 = 两个入口同一状态，且要新增 `close others` 之类能力。
- B：收编 + 加「关闭其它结果」。

## 20. 最终变更清单与施工顺序

### 20.1 变更清单（全部已拍板项汇总）

| # | 变更 | 依据 |
| --- | --- | --- |
| 1 | 工具栏右段清零：删 `pushdown`、`compare`/`analysis` 迁出、`comments` 移入设置 | §14.1 |
| 2 | 筛选栏去掉「显示 SQL」chip，形成纯筛选栏（对齐 DBeaver 5 格） | §14.1 / Q21 |
| 3 | 筛选栏补 5 个 ToolItem（清空 / 自定义 / 后退 / 前进 / 查找替换） | §10.2 |
| 4 | 标签页新增右键菜单（分析这份结果集 / 并排对比） | §19.3 |
| 5 | 行高、表头高按 `font_height` 计算；表头默认 1 行（`COLUMN_HEADER_EXTRA` 默认 NOTHING） | §7.2 / §14.2 |
| 6 | 列宽默认不按值算（`columnWidthByValue=false`） | Q22-A |
| 7 | `show_column_comments` → 三档 `HeaderExtra { Nothing, Description, DataType }` | §14.2 |
| 8 | 设置浮层扩 6 页（左页列 + 右内容） | §10.4 |
| 9 | 08 对话框补「来源 SQL」（2 行截断）+ 三处偏差待定 | §14.4 / Q23 |
| 10 | 状态栏按 DBeaver 顺序重排、裁多余入口、宽度按字高 | §15.4 / Q26 |
| 11 | 右栏 6 页：aggregate 重做、grouping 重做、references 置灰、Insight 新增、ExecSql 删除 | §16 / §17 / §18.1 |
| 12 | 双击已选中页签 → 面板最大化 | §17.4 |
| 13 | 右栏页签条接 `on_reorder` | §18.2 |
| 14 | MRU 不做（`menu(true)` 已覆盖核心价值） | Q32-A |

### 20.2 施工顺序（一次一件事、一刀一提交）

1. **T1 工具栏收口**（删 pushdown、迁 compare/analysis、comments 出工具栏）+ 纯筛选栏 → `cargo check`
2. **T1b pushdown 代码清理**（`SetPushdown` / `PushdownFilter` / `Pushdown` 枚举，连 `exec_channel` 一起判）
3. **T3 筛选栏 5 ToolItem**
4. **T8 标签页右键菜单**
5. **T4/T13 字体度量**（行高 / 表头 / 状态栏宽度单一来源）
6. **T6 设置浮层 6 页 + 三档列头**
7. **T9 08 对话框补 SQL**
8. **T12 状态栏重排**
9. **T11a/b metadata / valueviewer 比对**
10. **T11f 删 ExecSql + T11g 洞察页**
11. **T11c aggregate 重做**
12. **T11d grouping 重做**
13. **T11e references 置灰**
14. **T11h 双击最大化 + T11j 页签重排接线**

（每步独立提交；编译与用例随步验证。）

## 21. Q33 定稿与「标签组」调研结论

### 21.1 Q33：标签只留关闭，其余进右键

按你的口径（对齐 DBeaver）调整结果标签：

| 元素 | 现状 | 改后 |
| --- | --- | --- |
| prefix 通道图标 | 有 | 保留 |
| prefix 图钉 | **可点**（`sets.rs:184-186`） | 见 Q34 |
| suffix「⋯」 | 可点，实为**重命名**钮（`sets.rs:153-166`） | **移除** |
| suffix ×（关闭） | 有 | **保留**（DBeaver 只有这个） |

右键菜单（新增，挂在 `Tab::on_context_menu`，`tab.rs:733-745` 已有该能力）：

```
分析这份结果集…        → panel.analyze_result_set（08 对话框，复用现链）
并排对比 ▸             → tabs ≥ 2；点其它结果=设为对比对象，点当前=取消
────────────────
重命名…                → 复用 BeginRename / CommitRename / CancelRename
钉住 / 取消钉住         → 复用 TogglePin（文案随 pinned 切换）
关闭                   → 复用 on_close
```

`TabActions` 不需要新造回调——`on_rename` / `on_toggle_pin` / `on_close` 已存在，右键与旧按钮共用同一批；只需新增 `on_context` 需要的两项新能力（`analyze` / `compare`，均走 `ResultRequest`）。

**Q34**：钉住状态是否保留标签上的**静态**图钉（不可点、只表示已钉住）？
- A（推荐）：保留静态图钉。否则标签上看不出哪份结果被钉住（钉住影响自动淘汰，属于用户要能一眼看到的状态）。
- B：完全不显示，钉住状态只能靠右键菜单文案（「取消钉住」）反推。

### 21.2 「浏览器标签组」调研结论：组件不支持，需自建

逐项核实（vendor 源码）：

1. **`TabBar` 没有分组能力**。字段只有 `id / base / style / scroll_handle / prefix / suffix / children / last_empty_space / selected_index / variant / size / menu / max_width / on_click / on_reorder`（`tab_bar.rs:48-67`），没有任何 group / 分组容器概念。
2. **`TabGroup` 存在，但属于 DockArea**（`vendor/gpui-base/src/dock/tab_group.rs:146`）。它的能力是**面板分组的合并/拆分**：拖到中心区 = 合并进组（`dock/drag.rs:129,189,268` 注释「centre zone, meaning merge into the group rather than split」），拖到边缘 = 拆出新组。这是 DockArea 的工作台级能力，不是「给结果标签分组命名/折叠/上色」。
3. **`TabVariant::Folder` 只是外观**（`tab.rs:60-63`）：文件注释写「Tabs shaped like a folder's index tabs, as in a web browser: 选中标签从条上升起、圆角、与下方内容衔接」——是**浏览器式文件夹标签的视觉样式**，不含分组语义。你记忆里的「组件支持」，很可能就是这一项。
4. **`div().tab_group()` 是 a11y role**（`checkbox.rs:454`、`carousel.rs:1047`、`button/toggle.rs:456` 用它），accesskit 的「tab group」无障碍语义，没有任何视觉分组效果。

→ 结论：浏览器那种标签组（命名 / 折叠 / 彩色）**组件不支持**。要做得自己在 `TabBar` 上面加一层分组容器（分组数据 + 折叠态 + 组头渲染 + 拖入拖出），成本不小；而结果集标签数量本来有限、且已有钉住 + 自动淘汰机制。**建议不做**，除非你有明确的「同一条 SQL 的多份结果要归成一组」使用场景——如果有，可以作为独立需求单独评估。

### 21.3 施工起点

T1（工具栏收口）为第一刀，改动范围：
- `panel/render.rs:634-649`（删 pushdown 构造）、`:677-691`（删 compare 构造）、`:1064-1099`（删 analysis 构造）、`:1204-1213`（工具栏组装）
- `grid/status.rs:322-427`（右段清空、`comments` 移除、分隔线判据收敛）
- `panel/render.rs` 里「显示 SQL」chip 一并移除

验收：`cargo check` 通过 + 工具栏只剩筛选复合体；`compare` / `analysis` 暂以「先摘除、再在 T8 接到右键」两步走（中间态允许这两项短暂无入口，但 `ctrl-s` / 状态栏提交入口仍可用）。

## 22. T1/T8 施工记录（2026-11-24）

### 22.1 Q34 定稿：钉住保留静态指示

标签上**保留**图钉图标，但**不可点**（只表示「这份被钉住」）。钉住 / 取消钉住的动作收进右键菜单。

### 22.2 结果标签最终形态

| 位置 | 内容 |
| --- | --- |
| prefix | 类型图标（`icons/table.svg`）→ 钉住时追加**静态**图钉 |
| 文字 | 标签名（`结果 N` 或用户重命名后的别名） |
| suffix | 通道徽标（`源库/加速/联邦`，旧结果缀 `·旧` 并灰）→ 失败 `danger` 点 → **关闭 ×** |

「⋯」重命名按钮已删除——标签上只剩关闭一个按钮。

### 22.3 标签右键菜单（实测形态）

```
分析这份结果集…            ← 只有「当前激活的那份」才出现（分析对象=网格里这份）
────────────
并排对比 · 结果 2           ← 每个其它结果一项（有几个列几项）
并排对比 · 结果 3
────────────
重命名…
钉住 / 取消钉住            ← 文案随 pinned 切换
────────────
关闭
```

### 22.4 为什么「并排对比」是扁平项而不是子菜单（实证）

1. `Tab::on_context_menu` 的 builder 签名是 `Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu`（`tab.rs:733-739`）——第三个参数是 `&mut App`。
2. 子菜单 API 是 `PopupMenu::submenu(label, window, cx: &mut Context<Self>, f)`（`popup_menu.rs:714-722`），需要 **`Context<PopupMenu>`**；`PopupMenuItem::submenu` 则要求一个已建好的 `Entity<PopupMenu>`（`popup_menu.rs:108`）。
3. 在 `&mut App` 里构造该 entity 需要 `PopupMenu::new(cx: &mut App)`，而它是 **`pub(crate)`**（`popup_menu.rs:346`）——跨 crate 不可达。
4. 结论：在现有组件能力下，右键菜单只能用扁平项。（若要真子菜单，得给 `Tab::on_context_menu` 或 `PopupMenu` 加公开构造入口，属于 vendor 改动，另议。）

另：`select_compare_set` 的语义已核（`editor/view/host/results/grid.rs:493-501`）——`store.toggle_compare(document, index)` + `sync_compare_grid`，即**点一下就加入/移出对比集合并立刻同步对比视图**，不需要再补一次「进入对比模式」。

### 22.5 落地清单

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/sets.rs` | 删「⋯」块；pin 改静态 `div + Icon`（selector `editor-result-tab-pin-N`）；`Tab::on_context_menu` 装配 5 类菜单项 |
| 同上 `TabActions` | 新增 `on_analyze` / `on_compare` 两个回调槽 |
| `crates/result_panel/src/panel/render.rs` | 注入两回调：`on_analyze` 取 `grid.delegate().on_analyze_result_set` 执行（与网格右键同一入口 → 08 对话框）；`on_compare` 发 `ResultRequest::SelectCompare(index)` |
| `crates/result_panel/src/grid/status.rs` | `render_toolbar` 收为 `(filter, cx)`，右段整体删除（T1） |
| 同上 | 删 `Toolbar` / `Separator` 导入 |
| `crates/result_panel/src/panel/render.rs` | 删 pushdown / comments / compare / commit / copy / analysis 六处构造（T1） |
| `crates/result_panel/src/panel/tests.rs` | `toolbar_check_buttons_flip_their_panel_state` 收为两颗探针 |
| `crates/result_panel/src/grid/tests.rs` | `ToolbarHarness` 调用签名同步为 2 参 |

### 22.6 验证

| 检查 | 结果 |
| --- | --- |
| `cargo check -p rds-result-panel --all-targets` | **0 error / 0 warning**（两次，T1 与 T8 各一次） |
| `cargo test -p rds-result-panel --lib toolbar`（T1 阶段） | 15 passed / 0 failed |
| `cargo test -p rds-result-panel --lib`（T8 阶段全量） | **421 passed / 0 failed / 0 ignored**（3.12s） |

### 22.7 遗留（本节登记，尚未做）

- 「分析」只对激活标签显示：点其它标签的「分析」需要先切过去（DBeaver 的分析对象同样是当前结果，此处一致）。
- 双击页签最大化：状态与动作都有（`panel_maximized` + `toggle_panel_maximize`），但入口只在视图菜单勾选；双击手势未接。
- 页签条拖拽重排：`TabBar::on_reorder` 已核存在（`tab_bar.rs:92-103`），右栏页签条（`sidebars.rs:521-531`）仍只接 `on_click`，未接重排。
- 标签组（浏览器式）：组件不支持，见 §21.2。
- 钉住状态在标签上不再可点 → 相关旧测试 selector（`editor-result-tab-unpin-N`）已随实现删除，无残留引用。

## 23. 双击最大化落地（含一次 vendor 误改的回滚）

### 23.1 先调研：0.10.0 官方到底有没有这个能力

| 查证点 | 结论 |
| --- | --- |
| `on_double_click` 是否存在 | **存在**：`vendor/gpui-base/src/event.rs:44`，内部就是 `interactivity().on_click` + `event.click_count() == 2` |
| 对谁可用 | `event.rs:60`：`impl<E: InteractiveElement> InteractiveElementExt for Stateful<E>` —— **只对 `Stateful<E>`** |
| `gpui_base::tabs::Tab` 有没有 | **没有**（`Tab` 不是 `Stateful`；渲染时还会被 `.test_support()` 包成 `Observed<E>`） |
| `gpui_component::tab::Tab` 有没有 | **没有**（只暴露 `on_click` / `on_context_menu` / `tooltip` / `on_reorder`） |
| `ClickEvent` 有没有点击计数 | **有**：`ClickEvent::click_count()`（官方 `event.rs:52` 判双击用的就是它） |

### 23.2 结论与我的失误

第一版实现直接给 vendor 两层 `Tab` 加了 `on_double_click`（字段 + builder + 渲染挂载）。**这是多余的**——因为 `Tab::on_click` 回调拿到的 `ClickEvent` 本来就带 `click_count()`，应用层判一下即可，完全不需要动组件。

已**全部回滚**，并验证：`git status --porcelain -- vendor/` 空、`git diff --stat -- vendor/` 空（vendor 与 HEAD 零差异）。

### 23.3 最终实现（零 vendor 改动）

`crates/result_panel/src/panel/sidebars.rs` 右栏页签：

```rust
Tab::new()
    .icon(..).label(..).tooltip(..).suffix(close)
    .on_click({
        let double_queue = self.queue.clone();
        // 渲染时快照 = 当时的「已选中页」
        let is_current = tab == self.right_tab;
        move |event, _window, app| {
            if event.click_count() < 2 || !is_current { return; }
            double_queue.request(ResultRequest::TogglePanelMaximize, app);
        }
    })
```

- 复用既有端口 `ResultRequest::TogglePanelMaximize`（`ports.rs:296`，注释里本就写着「对齐 DBeaver `togglePanelsMaximize`」）。
- 语义对齐 `ResultSetViewer.java:366-374`：DBeaver 是 `SWT.MouseDoubleClick` + 只在 `selectedItem == panelFolder.getSelection()` 时 toggle。

### 23.4 与 DBeaver 的已知差异（须实机确认）

| 点 | DBeaver | 本仓 |
| --- | --- | --- |
| 「是不是当前页」判定时机 | 事件触发时实时查 `panelFolder.getSelection()` | 渲染时快照 `tab == right_tab` |
| 双击非当前页 | 只切页，不最大化 | 第一次点击先切页（TabBar 单击）；若第二击落在重渲染后的同一页签，可能顺带最大化 |

若实机发现「双击非当前页也最大化」，改法很小：把判定挪进面板侧（`TogglePanelMaximize` 带 `tab` 参数，panel 里再比 `tab == self.right_tab`），不涉及组件。

### 23.5 本轮验证

| 检查 | 结果 |
| --- | --- |
| `cargo test -p rds-result-panel --lib` | **421 passed / 0 failed**（3.05s） |
| `cargo check -p rds-result-panel --all-targets` | 0 error / 0 warning |
| `vendor/` 工作区状态 | 干净（与 HEAD 零差异） |

### 23.6 流程修正（写进约定）

改 vendor 之前必须先查「当前版本官方是否已支持」：

1. 先查能力本身是否存在（如 `on_double_click` 在 `gpui-base/src/event.rs`）。
2. 再查它的 impl 约束覆盖到不到我们要用的类型（如只对 `Stateful<E>`）。
3. 最后查目标组件是否已经暴露（`Tab` 的 builder 列表）。
4. 只有「能力不存在」或「约束够不着且无法绕过」时才改 vendor，并在改动处标【项目私有 patch】。

## 24. 右栏页签重排落地（纯应用层，零 vendor 改动）

### 24.1 先调研：官方有没有

| 查证点 | 结论 |
| --- | --- |
| `TabBar::on_reorder` 是否存在 | **存在**：`vendor/gpui-component/src/tab/tab_bar.rs:92-103`，签名 `Fn(usize, usize, &mut Window, &mut App)`，注释标明【项目私有 patch】= 组件侧早已支持 |
| 结果标签条有没有接 | **已接**（`sets.rs:238-240`，结果集标签拖拽重排走 `MoveSet`） |
| 右栏页签条有没有接 | **没接**（`sidebars.rs` 此前只挂 `on_click`） |

→ 该项**不需要改 vendor**，只补一处应用层接线。

### 24.2 顺序真值只有一个：`open_tabs`

右栏页签条逐项渲染的就是 `self.open_tabs`（`sidebars.rs:469`），
`selected_index` 用 `strip_index(&open_tabs, right_tab)`、点击用 `tab_at_strip_index`。
所以「重排」= 搬 `open_tabs` 的元素，不需要第二份顺序状态。

### 24.3 落地（4 处）

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/ports.rs` | `ResultRequest` 新增 `MoveRightTab { from, to }`（下标是页签条位置） |
| `crates/result_panel/src/panel/events.rs` | 新增 `move_right_tab(from, to, cx)`：越界/同位直接返回，否则 `remove` → `insert` → `notify` |
| `crates/editor/src/view/host/results/dispatch.rs` | 新分支转发到面板，并 `persist_result_open_tabs(cx)` 落盘 |
| `crates/result_panel/src/panel/sidebars.rs` | 右栏 `TabBar` 挂 `.on_reorder(...)` → 发 `MoveRightTab` |

### 24.4 验证

| 检查 | 结果 |
| --- | --- |
| `cargo check -p rds-editor -p rds-result-panel --all-targets` | 0 error / 0 warning（4m16s，含 editor 重编） |
| `cargo test -p rds-result-panel --lib` | **422 passed / 0 failed**（2.39s，较上轮 +1） |
| 新增用例 | `moving_a_right_tab_reorders_only_the_strip`：搬下标 2 → 0 后 `open_tabs` 顺序符合预期；当前页不变；越界请求不改变集合 |

### 24.5 与 DBeaver 的差异 / 遗留

- DBeaver `TabFolderReorder` 只改 `CTabFolder` 的显示顺序、**不持久化**；本仓把顺序跟着 `result_panel.open_tabs` 偏好一起落盘。这属于**更严格**的行为（重启后仍保留用户排布），已在差异表登记为「有意增强」。
- 走「视图菜单重新勾选一页」时，`toggle_right_tab` 的插入位置仍按规范序（`events.rs:730-740` 的 `insert_at`），会把该页插回规范位置、可能打乱用户拖拽过的顺序。若要严格保持用户顺序，需把这个 `insert_at` 改成「追加到末尾」。**尚未改**（登记为待定项，改动只需一行）。
- 页签拖拽的 ghost 视觉沿用组件默认（`TabDragGhost`），未做额外美化（符合「不加动效/装饰」的口径）。

## 25. 右栏五页对齐：DBeaver 真值 vs 本仓现状（取证，未动代码）

本轮目标形态：右栏由 6 页收成 **5 页**（对外名称与 DBeaver 的五个 panel 一一对应），
references 先置灰；另加「洞察」入口的心智提醒。本节只记录取证与差异，改码待拍板。

### 25.1 五页真值（逐条带行号）

**① 元数据页 `MetaDataPanel.java`**
- 结构只有两块（`:81-102`）：顶部 filterPanel（`GridLayout(2 列)`，`FILL_HORIZONTAL`），
  下面是 `MetaDataTable`（继承 `DatabaseObjectListControl`，`SWT.SHEET | SWT.BORDER`，
  `FILL_HORIZONTAL | FILL_VERTICAL`，`setFitWidth(true)`）。
- filterPanel 子件顺序（`:84-89`）：`createControlLabel("筛选：")` → `Text(SWT.BORDER | SWT.SEARCH | SWT.ICON_CANCEL)`
  （`FILL_HORIZONTAL`，message 为提示语，`addModifyListener → refresh(true)`）。
- filterPanel 只对**列名**做包含匹配（`:178-183`，`a.getName().toUpperCase().contains(filterText)`）。
- 表格是 **Tree**，支持嵌套属性（`:219-230` 的 `getNestedBindings`），载入后 `expandToLevel(2)`（`:315`）。
- 额外列 `"Description"`（`:291-302`）；默认列集由 `getListConfigId` 按驱动分流（`:254-262`，`"MetaData/<driverId>"`）。
- 不可见列用**斜体**（`:283-289`，`treeAndTableFontItalic`）。
- 选中行 ↔ 网格当前列**双向联动**（`:104-134`，`setCurrentAttribute` / `setSelection`）。
- 右键菜单 = 默认树菜单 + 「复制列名」（`:234-251`）。
- 页内工具条 `contributeActions` **只有 1 项**（`:204-215`）：`sql.generate.ddl.by.resultSet`（「按结果集生成 DDL」，icon `DBIcon.SQL_TEXT`）。
- 整页**没有**「列详情键值区」。

**② 值页 `ValueViewerPanel.java`**
- 单一占位容器 `viewPlaceholder`，`FillLayout` + `margins(2, 2)`（`:85-86`）。
- 空态由 paint 画**两行**居中文字（`:89-106`）：`value_viewer_select_view_message`（第 0 行）、
  `value_viewer_hide_panel_message`（带 hidePanel 快捷键，第 30 行偏移）。
- 正文 = 当前单元格的 value editor（面板型）；单行编辑器或字典视图时改 `GridLayout(1)` 并插入
  编辑选择器（`:294-302`）；无编辑器时画 `No editor for [type]`（`:305-313`）。
- 页内工具条 `fillToolBar`（`:409-428`）顺序：valueManager 动作 → `GROUP_ACTIONS_ADDITIONAL` 标记 →
  引用相关项 → 非只读才加 `SaveValueAction`（写回结果集）。
- `needsSeparator() = true`（`:171-173`）——全仓唯一一页在工具条下画分隔线。
- 跨页联动：选区变化即刷值（`:123-131`）。

**③ 计算页 `AggregateColumnsPanel.java`**
- 控件只有一棵 `Tree`（`SWT.SINGLE | SWT.FULL_SELECTION`，header 可见、网格线可见），
  **两列**：`aggregate_columns_function_text` / `aggregate_columns_value_text`（`:90-94`）。
- 输入是**多单元格选区**（`:203-208`）；`groupByColumns=true` 时按列分组，每列一个顶层节点（列名 + 图标，展开后是各函数值）（`:231-249`）；否则所有选中值合成一组（`:250-261`）。
- 计算在**客户端**（`model.getCellValue` + `IAggregateFunction`），不下发 SQL；数值格式化 `###,###,###,###,###,##0.###` / `###,###,###,###,###,##0`（`:65-66`、`:300-312`）。
- 右键菜单（`:106-116`）：复制 → 复制全部 → 分隔 → 工具条全项。
- 页内工具条 6 项（`:322-331`）：**添加函数**（`MODE_FORCE_TEXT`，带未启用函数的下拉）→ **移除函数** → **重置函数** → 竖分隔 → **按列分组**（check，`GROUP_BY_ATTR`）→ **值类型切换**（check，图标随开关在 `TYPE_STRING`/`TYPE_NUMBER` 间切换）。
- 函数集与两个开关持久化在 `panel-column-aggregate` 视图设置（`:129-181`）。

**④ 分组页 `GroupingPanel.java`**
- 单一占位 + `GroupingResultsContainer`（`:64-102`），容器内是**第二个完整 `ResultSetViewer`**（`GroupingResultsContainer.java:64-70`）。
- 分组 = 生成 SQL 后**重查**：`GroupingDataContainer` 负责把分组列/函数拼成 `SELECT … GROUP BY …`，
  默认函数 `COUNT(*)`（`GroupingResultsContainer.java:75-89`）。
- 分组列与函数列由 `GroupingColumnsContainer` 维护；配置界面是对话框 `GroupingConfigDialog.java`（266 行）。
- 结果区是标准结果集（自带网格 + 状态条），不是自定义列表。

**⑤ 引用页 `ReferencesPanel.java`**
- 单一占位 + `ReferencesResultsContainer`（`:62-72`），同样是**内嵌第二个结果集**显示被引用行。
- 选区变化时刷新引用（`:73-99`），带 `UIExecutionQueue` 节流与 `prevSelection` 去重。
- `contributeActions` **空实现**（`:156-158`）→ 页内工具条没有任何动作。

### 25.2 本仓现状（文件:行）

| 页 | 本仓实现 | 结构 |
| --- | --- | --- |
| Browser（列浏览器） | `panel/sidebars.rs:244` | 表头（列名/类型/眼睛）+ kit `List`（内置搜索）+ 行尾眼睛 + 右侧明细键值区（可见/冻结/来源表/来源列/注释/驱动类型/可空/默认值）+ ESC 清空 |
| Value | `panel/tabs.rs:237` | `panel_toolbar`（标题「列名 · 类型」+ 位置标记 + 复制 + 保存至文件 + 外部打开 + 分隔线）+ 正文按值形态分派（文本 / JSON 树 / hex 转储 / 图片） |
| Stats（计算） | `panel/tabs.rs:617` | `stats_toolbar`（＋ 添加 / Reset / 口径切换）+ 表头（函数 / 值）+ 行 + 行尾 ×（停用单项）+ 千分位；数据来自列级访问器 `column_stats_for` |
| Group（分组） | `panel/tabs.rs:1113` | 工具条（分组列勾选下拉 + 排序口径 + 仅重复）+ 本地 `group_by_columns_for` 计数列表；大表口径下发 `GROUP BY` 重查（§8.8） |
| Reference（引用） | `panel/tabs.rs:1423` | 工具条（空动作，与 DBeaver 一致）+ 正文四态（未读/读中/失败/空/有键）；有键时 = 键下拉 + 打开目标对象 + 列映射；**无引用行结果集** |
| ExecSql（执行 SQL） | `panel/tabs.rs:1599` | 只读 wrapped SQL 原文 + 复制；`RunFilterAsQuery` 执行后自动切到本页 |

### 25.3 差异表（按页）

| # | 页 | DBeaver | 本仓 | 判定 |
| --- | --- | --- | --- | --- |
| D1 | 元数据 | 属性 **Tree**（嵌套 + `expandToLevel(2)`） | 平铺 `List` | 需拍板：要不要为嵌套属性换 Tree（换则失去行尾眼睛/搜索框内置） |
| D2 | 元数据 | 顶部「筛选：」标签 + 搜索框两列 | 搜索框在 List 内置、无标签 | 可对齐（加一个前缀标签成本低） |
| D3 | 元数据 | 额外列 `Description` | 注释放在右侧明细区 | 保留差异（明细区信息更全） |
| D4 | 元数据 | 不可见列**斜体** | 隐藏列置灰斜体（已做） | 已对齐 |
| D5 | 元数据 | 页内工具条 1 项：按结果集生成 DDL | 本页工具条为空 | 需拍板：本仓有无「由结果集生成 DDL」能力（有 `ddl.rs`，需查是否为同一件事） |
| D6 | 元数据 | 选中 ↔ 网格列双向联动 | 单击联动、网格选中回写清单 | 已对齐（需实机确认反向联动） |
| D7 | 值 | 编辑器可**写回**（`SaveValueAction`） | 只读预览（复制 / 保存至文件 / 外部打开） | 保留差异（写回属编辑能力，单独立项） |
| D8 | 值 | 空态两行提示（含 hidePanel 快捷键） | 两行空态（`value_empty_lines`） | 已对齐 |
| D9 | 值 | 工具条下分隔线（全仓唯一） | 已传 `separator=true` | 已对齐 |
| D10 | 计算 | 输入 = **多单元格选区** | 输入 = 单格 / 清单选中列 | **结构性差异**：要真正对齐需给网格加单元格选区（独立工程） |
| D11 | 计算 | 按列分组（树顶层节点） | 无（依赖多选） | 随 D10 |
| D12 | 计算 | 移除函数 = 选 Tree 行按「−」 | 每行行尾 × | 保留差异（本仓统计行无行选中模型，已登记） |
| D13 | 计算 | 工具条 6 项（添加/移除/重置/分隔/分组/口径） | ＋ / Reset / 口径（3 项） | 缺「移除」与「分组」，随 D10/D12 |
| D14 | 分组 | 配置 + **重查**，结果区是完整结果集 | 本地 groupby 列表（另有下发重查路径） | 需拍板：分组页是否改成「内嵌结果集」形态 |
| D15 | 引用 | 内嵌结果集显示引用行 | 只有键 + 列映射 | **按拍板置灰**（先不做） |
| D16 | — | 无「执行 SQL」页 | 有 ExecSql 页 | 按拍板删除 |

### 25.4 删 ExecSql 的影响面（先算清再动）

引用分布（`ExecSql` / `exec_sql` 命中 48 处 / 10 文件）：

| 文件 | 命中 | 性质 |
| --- | --- | --- |
| `result_panel/src/ports.rs` | 5 | `RightTab::ExecSql` 变体、`ALL`、`storage_id()`、`CopyExecSql` 请求、`RunFilterAsQuery` 注释 |
| `result_panel/src/panel/events.rs` | 11 | 自动切页、复制、页状态维护 |
| `result_panel/src/panel/tests.rs` | 12 | 用例断言 |
| `result_panel/src/panel/tabs.rs` | 6 | `render_exec_sql_tab` 本体与调用 |
| `editor/src/view/tests/results_a.rs` | 5 | 宿主侧用例 |
| `editor/src/view/host/results/{filter,dispatch}.rs` | 6 | 生成 SQL 后写面板 + 转发 |
| 其余（`data.rs` / `lifecycle.rs` / `sidebars.rs`） | 3 | 字段与分派 |

**必须同时回答的问题**：`exec_sql` 字段与 `RunFilterAsQuery` 生成的那段 wrapped SQL，
删页之后还看不看得到？三种口径的输出完全不同（只删页签 / 连 SQL 一起删 / 暂缓），见待确认项 Q35。

### 25.5 洞察入口：本仓已有链路（不是从零做）

- 入口已有两处：**列头右键「洞察此列」**（`store/entry.rs:315-325` 的 `can_insight_column` 四条门槛）
  与结果集级 `open_insight_result_set`（`editor/src/ports/editor_completion.rs:1531`）。
- 下游是 `crates/insight` 的 `InsightService`（`editor/src/ports/result_service.rs:13-18`：
  结果集只提供 **DuckDB 临时表** `create_duckdb_temp_table` 与连接句柄）。
- 右栏 `RightPanel::Insight` 已存在（`workbench_shell/src/model.rs:36-37`、命令 `panel.insight`）。

→ 本轮要补的只有两件事：①点洞察时先给「数据已进入分析库临时表」的心智提醒；
②「列洞察不稳定 → 只提醒，直接进表画像」。两者都是入口层改动，不动分析内核。

## 26 第 5 项施工记录：右栏五页严格对齐 + 洞察确认（2026-11-24）

三项拍板（用户在 §25 取证后的答复）：Q35-A 只删「执行SQL」页签、SQL 并入筛选栏展开区；
Q36-A 引用页保留页签位置、正文写占位；Q37-A 点洞察先弹确认框再进。本节记录落地结果。

### 26.1 改动清单

| 文件 | 改动 |
| --- | --- |
| `result_panel/src/ports.rs` | 删 `RightTab::ExecSql` 变体，`ALL` 6→5（Browser/Value/Stats/Group/Reference 顺序不变）；`parse()` 走 `ALL` 匹配，旧偏好值 `execsql` 自动丢弃，无需额外容错；新增 `ResultRequest::RequestInsightColumn(String)` |
| `result_panel/src/panel/events.rs` | `right_tabs()` → `[RightTab;5]`；`tab_close_selector` / `right_tab_label` / `right_tab_icon` / `right_tab_description` 四条 match 分支删；`set_exec_sql` 改为**只存文本 + notify**（不再切页、不再强开右栏） |
| `result_panel/src/panel/tabs.rs` | 删 `render_exec_sql_tab` 整函数；新增 `render_reference_tab`（`empty_page(RightTab::Reference, "editor-result-reference-toolbar", "引用", …)`）；原实现改名 `render_reference_tab_live` 并 `#[allow(dead_code)]` 原样保留（状态机、取数、列映射、单测都留着） |
| `result_panel/src/panel/sidebars.rs` | 内容分派删 ExecSql 分支（右栏只剩五页）；`render_filter_expand_bar` 新增 `executed` 一行：截断 80 字符的已执行 SQL + 「打开全文」（`OpenSqlDialog`）+ 「复制」（`CopyExecSql`），selector `editor-result-filter-executed-sql` / `-open` / `-copy`；`executed` 存在时不画 `preview`（否则同一栏两条互相矛盾的 SQL）；最终 `.children(executed).children(preview)` |
| `result_panel/src/panel/data.rs` 之外 | `open_tabs: Vec<RightTab>`（顺序唯一权威）、`exec_sql: Option<String>`、`filter_expanded` 三个字段未动，语义不变 |
| `editor/src/view/host/results/dispatch.rs` | `use …::{ResultRequest, RightTab}` 去掉 `RightTab`（唯一引用是三处置灰触发）；三处 `if tab == RightTab::Reference { begin_references_load }`（`SetRightTab` / `ToggleRightTab` / `OpenRightTab`）注释停用并留恢复指引——置灰后触发只会白跑一次元数据查询；新增 `RequestInsightColumn` 分支：`dialogs::open_insight_confirm(window, cx, column, move \|_w, cx\| entity.update(cx, \|panel, cx\| panel.insight_column(&column, cx)))` |
| `editor/src/view/host/construct.rs` | 洞察钩子改为**入队**：`host.result_action_queue.push(ResultRequest::RequestInsightColumn(column))` + `result_panel` notify（原来直接调 `panel.insight_column`）。原因：钩子签名只有 `&mut App`，开不了对话框 |
| `editor/src/view/dialogs/confirm.rs` | 新增 `open_insight_confirm(window, cx, column, on_confirm)`：标题「进入洞察」，正文两行（「要洞察「X」这一列。」+ 灰字「洞察会在分析库里建一张临时表：这份结果的行会被取样写进去再分析，源库不受影响。确认后打开右侧洞察面板。」），footer 两钮「取消」/「进入洞察」（`action_button` ⇒ selector `editor-dialog-insight-cancel` / `editor-dialog-insight-confirm`） |
| `result_panel/src/sets.rs` | 修 T8 留下的编译错：`TabBar::children` 只吃 `Into<Tab>`，而 T8 用 `ContextMenuExt::context_menu` 把 `Tab` 变成了别的元素类型（当时写成 `Vec<AnyElement>`）。改为 `Tab::on_context_menu`（vendor 私有 patch，`vendor/gpui-component/src/tab/tab.rs:733`：传入已建好的 `PopupMenu`、返回它），`children` 回到 `Vec<Tab>`，顺带删掉不再用的 `ContextMenuExt` import |
| `settings/src/model/result_panel.rs` | `default_result_open_tabs()` → `"browser,value,stats,group,reference"` |
| `settings/src/registry/entries/result_panel.rs` | `result_panel.open_tabs` 条目的 hint 与 `default_json` 同步五页 |
| `settings/src/{registry/mod.rs, service/result_panel.rs}` | 相关注释改五页 |

另外把全仓 `crates/**/*.rs` 的「六页」统一改成「五页」（10 文件 20 处），并手动改回两处被误伤语义
（`ports.rs` 与 `settings/model/result_panel.rs` 里的「原第六页」指的是历史事实，不能变成「原第五页」）。

**为什么用 Button 承载展开区那行 SQL 的点击**：`gpui_kit::Div` 没有 `on_click` / `tooltip`
（第一版照 Div 写编译错误 `no method named on_click found for struct gpui_kit::Div`）；
ghost `Button` 同样紧凑且自带 hover / tooltip。

### 26.2 验证

门禁环境：`CARGO_BUILD_JOBS=1`、`CARGO_TARGET_DIR=<repo>/target`、`DUCKDB_LIB_DIR=third_party/duckdb/1.5.6`，
测试另加 `CARGO_PROFILE_TEST_DEBUG=line-tables-only`。

| 命令 | 结果 |
| --- | --- |
| `cargo check -p rds-editor -p rds-result-panel --all-targets` | exit 0，0 error 0 warning（2m45s，含增量重编） |
| `cargo test -p rds-result-panel --lib` | **422 passed / 0 failed**（2.98s） |
| `cargo test -p rds-editor --lib` | **865 passed / 0 failed / 4 ignored**（15.78s） |
| `cargo test -p rds-settings --lib` | **49 passed / 0 failed** |
| `cargo test -p rds-workbench --test insight_editor_entry` | **2 passed / 0 failed**（端口直调那条链未受钩子改造影响） |

日志：`.rds/tmp/t16-check.txt`（首个失败：`Tab` ≠ `AnyElement`）、`t17-check.txt`（0/0）、
`t18/t19-test-resultpanel.txt`、`t20/t21/t25-test-editor.txt`、`t22-test-settings.txt`、
`t23/t24-test-insight-confirm.txt`、`t26-test-workbench-insight.txt`。

### 26.3 测试同步（含 T1 漏跑全量留下的 4 个失效断言）

**(a) 五页 / 删 ExecSql**（`result_panel/src/panel/tests.rs`、`editor/src/view/tests/results_a.rs`）：
`right_tab_index_maps_to_the_five_panel_tabs`、`neighbor_after_close(..., RightTab::Reference)`、
出厂五页、`open.len()==4`、重开页期望去 ExecSql、`set_open_tabs_initial(&[Value, Reference])`、
关闭 selector 列表、`before.len()==5`、视图菜单 `items.len()==10` 且索引 5–9、状态栏工具条 id 列表、
`right_tab_descriptions...seen.len()==5`、删掉「执行SQL 页渲染」整块（注明去向）、页签条点击用例改用
Reference；`results_a.rs` 两处页签期望改五页、关 Reference 后 `vec![RightTab::Value]`。

**(b) 引用页置灰**：`reference_tab_requests_metadata_and_renders_the_key_list` 后半改为
「有页内工具条 / 无键下拉 / 无「打开目标对象」」；`reference_rows_use_the_selected_row_values` 的渲染段
改成「`editor-result-reference-toolbar` 在、`editor-result-reference-rows` 不在」，**数据侧断言全部保留**
（`reference_rows_sql` 生成 `SELECT * FROM customers WHERE id = 1`、`open_reference_rows` 入队一条
`OpenReferenceRows`）。

**(c) 计数口径**：`right_tab_ids_round_trip` 的 `ALL.len()` 6→5；
`toggling_the_sidebar_keeps_a_custom_tab_set` 关掉列浏览器后 5→4。

**(d) T1 漏跑全量留下的 4 个失效断言（本轮补齐）**——这是 T1 当时只跑 `cargo test --lib toolbar` 的代价：

| 用例 | 失效原因 | 处置 |
| --- | --- | --- |
| `results_a::copying_the_active_result_puts_tsv_on_the_clipboard` | `editor-result-copy` 按钮已按 Q13 裁掉 | 断言反转（守住「别长回工具栏」），剪贴板内容断言不变 |
| `analysis::commit_controls_appear_with_pending_changes` | `editor-result-commit-group` 组件已删 | 改名 `pending_cell_changes_are_counted_without_a_commit_group`，UI 两处断言反转，计数仍从权威变更集取（2 条） |
| `analysis::reset_cell_request_reverts_a_cell_change` | 预置段靠 commit 组出现作证据 | 改用变更集 `pending_count()==1` 作预置证据，其余断言不变 |
| `exec::execution::tests::queue::cancel_right_after_submit_still_targets_the_job_connection` | 首轮失败（「中断在它开跑前就到了」，left 1 right 0） | 同环境重跑通过；判为**时序 flaky**，与本次改动无交集，未深挖（记录在案，不复现即不再追） |

**(e) 本轮新增**：`editor/src/view/tests/dialogs.rs::insight_column_asks_before_touching_the_analysis_library`
——真窗口 + `Root` 对话框层，投一条 `RequestInsightColumn` 后断言：弹层在（`has_active_dialog`）、
两钮都渲染、**端口未被调用**（未确认前数据不进分析库临时表）。

### 26.4 T1 的副作用（现状必须记住）

T1 按 Q13 裁掉的六个工具栏构造里，有两处的**入口至今没有真实替代**：

- **复制**：`copy_active_result` 能力在（剪贴板断言仍绿），UI 入口没有——状态栏只有 disabled 占位
  （`editor-result-action-*` 全 `disabled(true)`），`ResultControls` 的 copy 字段为 `None`。
- **提交 / 回滚**：`ResultControls.commit` / `reject` 为 `None` → 状态栏那两格落 disabled 占位；
  变更集与提交内核（`changes.rs`、`commit_updates_rows`）仍在，只是没有可点的钮。

要补回来的正确位置是 DBeaver 状态栏那两段（`status.editCmds`，`plugin.xml:1407-1466`），
**不是**顶部工具栏——顶部本来就不该有它。

### 26.5 仍开放的缺口

1. **引用页 live 未接线**：`render_reference_tab_live`、`begin_references_load`、三处 `Reference` 触发
   全部保留（两处注释、一处原样），接线时把 `render_reference_tab` 的占位换回即可。
2. **洞察确认的点击级回归没做**：只验到「弹框 + 未确认不动作」。确认之后的整链
   （弹框 → `insight_column` → 宿主端口 → 展开洞察面板）未做点击级测试——headless 下
   `debug_bounds` 坐标与命中测试对不上（见 `dialog_button_rendered` 注释：试过 `simulate_click`，
   单跑能中、全套跑中不了）。
3. **筛选展开区 `preview` 的字节切片**：`&wrapped[..80]` 按字节截断，遇多字节列名会 panic。
   既有问题（非本轮引入），未修，记在这里。
4. **§25 差异表 D1–D16 仍未拍板的部分**：元数据页 Tree vs List（D1）、筛选标签（D2）、
   元数据页 DDL 工具条项（D5）、计算页多单元格选区（D10/D11/D13）、分组页是否改「内嵌结果集」（D14）。

## 27 差异表结算与 D2 落地（2026-11-24 续）

用户口径（本轮明确）：**凡 DBeaver 有明确实现的差异项，不再回头拍板，直接按源码做**；
只有「DBeaver 那侧也没实现」或「本仓缺前端/数据前提」才回来说明。本节按这条口径
逐条结算 §25.3，并记录已落地的第一刀（D2）。

### 27.1 D1–D16 结算

| # | 结算 | 依据 |
| --- | --- | --- |
| D1 | **数据前提缺口，不是形态选择** | DBeaver 用 Tree 的价值在**嵌套属性**（`MetaDataPanel.java:219-230` 的 `getNestedBindings` 递归取子绑定，`:315` 载入后 `expandToLevel(2)`）。本仓 `ColumnAnnotation`（`crates/workbench_shell/src/model.rs:331`）只有 7 个平字段，没有任何子属性/嵌套来源；换成 Tree 只能把平表画成一层节点——收益 0，还要丢行尾眼睛与内置搜索。真正的对齐前提是「注解能表达 struct/list 的子字段」，属驱动/元数据能力，不是 UI 形态。留在待办，等数据侧能给出嵌套再换。 |
| D2 | **已做**（见 27.2） | `MetaDataPanel.java:81-89`（`GridLayout(2 列)` = Label「筛选：」+ `Text`）、`:87-98`（Esc 清空）、`:178-183`（只按列名/类型/注释包含匹配）。 |
| D3 | 保留差异 | 本仓把注释放在右栏明细键值区，比 DBeaver 的额外列信息更全（`MetaDataPanel.java:291-302`）。 |
| D4 | 已对齐 | 不可见列置灰斜体（`MetaDataPanel.java:283-289`）。 |
| D5 | **已完成（§25.3 该行已过期）** | 页内工具条的唯一一项就是「生成 DDL」（`editor-result-generate-ddl`），走 `crates/result_panel/src/ddl.rs::create_table_ddl` + SQL 预览浮层，与 `MetaDataPanel.java:203-215` 的 `generate.ddl.by.resultSet` 同口径（生成**新表** DDL，不是来源表）。该行是施工前取证留下的旧状态。 |
| D6 | 已对齐（待实机确认反向联动） | `MetaDataPanel.java:104-134`。 |
| D7 | 保留差异，单独立项 | 写回结果集要动编辑/提交内核（`SaveValueAction`，`ValueViewerPanel.java:409-428` 的只读判断），不属 UI 对齐。 |
| D8/D9 | 已对齐 | 空态两行 + 工具条下分隔线（`ValueViewerPanel.java:89-106`、`:171-173`）。 |
| D10 | **受前端能力约束，独立工程** | DBeaver 的输入是**多单元格选区**（`AggregateColumnsPanel.java:203-208` 的 `model.getCellValue`）。本仓网格只有整行选中与单格当前值，没有矩形选区/选区集合——要真对齐必须先有选区模型（选区状态 + 拖选/Shift 选 + 渲染高亮 + 事件）。这是前置工程，不是二选一的形态题。 |
| D11 | 随 D10 | 「按列分组」要求 Tree 有顶层列节点（`:231-249`），而节点的来源就是多选集合。 |
| D12 | 随 D10 | 「移除函数 = 选中 Tree 行按 −」要求行选中模型（`:322-331`）；本仓现用行尾 ×，等 D10 落地后统一。 |
| D13 | 随 D10/D11 | 工具条 6 项里缺的「移除」「分组」两项都以选区/行选中为前提，剩下的项已就位（＋ / Reset / 口径）。 |
| D14 | **按 DBeaver 做，独立一刀** | 分组页结果区是第二个完整 `ResultSetViewer`（`GroupingResultsContainer.java:64-70`）；本仓已有「下发 `GROUP BY` 重查」路径（§8.8），差的是把重查结果装进内嵌结果集面板，而不是本地计数列表。不需要拍板。 |
| D15 | 已按既有口径置灰 | 引用页正文占位（§26）；live 实现（`render_reference_tab_live`）保留待接线。 |
| D16 | 已做 | 删除「执行SQL」页（§26）。 |

**结论**：§25.3 里名义上标着「需拍板」的四项，按 DBeaver 口径只有两种归宿——
D2 已直接做掉、D5 本来就已经做了、D14 排进独立一刀；
D1 属于数据前提缺口、D10/D11/D12/D13 属于网格选区这一前置工程。
没有一项是真的「两种做法都对、要人拍」的。

### 27.2 D2 落地：列浏览器页顶过滤条

DBeaver 的过滤框**不在表内**，是页顶 `filterPanel` 的一格（`GridLayout(2 列)`，
Label「筛选：」在左、`Text(SWT.BORDER|SWT.SEARCH|SWT.ICON_CANCEL)` 在右）。
此前本仓把搜索放在 kit `List` 的内置搜索里，形态上少了「筛选：」这一列。

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/panel/data.rs` | 新增 `browser_filter_input: Entity<InputState>` 与 `_browser_filter_sub: Option<Subscription>`；`browser_list` 的注释改为「搜索框外置」口径 |
| `crates/result_panel/src/panel/lifecycle.rs` | `ListState` 改 `.searchable(false)`（内置搜索关闭）；构造期建 `InputState`（placeholder「筛选列名 / 类型 / 注释…」）；`subscribe_in` 的 `InputEvent::Change` → `browser_list.set_query`（`set_query` 内部直接起一次搜索，不依赖 `searchable`，见 `vendor/gpui-component/src/list/list.rs:240`） |
| `crates/result_panel/src/panel/sidebars.rs` | 页顶新增 `editor-result-col-browser-filter` 行（「筛选：」+ `Input::new(...).xsmall().cleanable(true)`，`cleanable` 承担 ICON_CANCEL）；Esc 清空从 List 容器上移到整页根容器，同时清输入框与查询词（`set_value` 程序化写入**不发** `Change`，所以必须显式 `set_query("")`）；List 去掉 `search_placeholder` |
| `crates/result_panel/src/panel/tests.rs` | 新增 `the_top_filter_bar_drives_the_column_list`（真窗口：断言过滤条渲染 + 真敲键穿透到 delegate）；`column_browser_filter_clears_on_escape` 的注释改为外置口径（行为不变，仍绿） |

验证：

| 命令 | 结果 |
| --- | --- |
| `cargo check -p rds-result-panel --all-targets` | exit 0，0 error 0 warning（3m44s） |
| `cargo test -p rds-result-panel --lib browser` | **15 passed**（含新用例） |
| `cargo test -p rds-result-panel --lib` | **423 passed / 0 failed**（原 422 + 1 新） |

日志：`.rds/tmp/d2-check.txt`、`d2-test-browser.txt`、`d2-test-all.txt`。

**仍未做（明账）**：过滤条的**视觉**（行高/内距）未做像素级比对——本仓用 `Input.xsmall`
（20px）配 `py_1` + 一条 HAIRLINE；DBeaver 的 `filterPanel` 是 `GridLayout(2 列)` 默认
间距、行高随 Text（约 20px）与 Label 的行高。目视验收按既有约定由用户手动做，这里不做截图。

### 27.3 后续队列（不再拍板，按此推进）

1. **D14 分组页内嵌结果集**（独立一刀）：把 `GroupingResultsContainer` 的形态落成
   「重查结果装进内嵌结果集面板」，复用现有 `RunFilterAsQuery` 那套下发路径。
2. **D10 网格单元格选区**（前置工程）：选区状态模型 → 拖选/Shift 选 → 渲染高亮 →
   计算页接入多选输入。落地后 D11（按列分组）、D12（− 移除）、D13（工具条补两项）一起收。
3. **D1 元数据页 Tree**：等 `ColumnAnnotation` 能表达嵌套子属性（驱动 struct/list）再做，
   在此之前换 Tree 只是形式主义。
4. 引用页 live 接线（§26.5-1）、洞察确认后的点击级回归（§26.5-2）、筛选 `preview` 的
   字节切片 panic（§26.5-3）仍按 §26.5 挂着。
## 28. D14 落地：分组页改成 DBeaver 的内嵌结果集形态

### 28.1 取证（源码行号）

| 事实 | 出处 |
| --- | --- |
| 分组页（`GroupingPanel`）只有一个占位件，容器懒建（`FillLayout`） | `GroupingPanel.java:64-65`、`:78-102` |
| 页被激活即 `refresh(false)`，元数据变了才 `clearGrouping()`，否则 `rebuildGrouping()` | `GroupingPanel.java:122-127`、`:139-157` |
| 工具条顺序：`DefaultSorting`（文字按钮）→ `DuplicatesOnly` → SEP → `EditColumns` → `DeleteColumn` → SEP → `ClearGrouping` → SEP → 扩展动作 | `GroupingPanel.java:164-176`、`:178-184` |
| `isDirty` = 分组属性非空；`loadSettings` / `saveSettings` 是空实现（分组键不进面板设置） | `GroupingPanel.java:105-107`、`:114-120` |
| 真骨架：容器里**内嵌一个完整 `ResultSetViewer`**（覆写 `refreshWithFilter` 记 currentFilter）+ `GroupingColumnsContainer` + 默认函数 `COUNT(*)` | `GroupingResultsContainer.java:61-73`、`:64-70`、`:75-89` |
| `clearGrouping` 只清内嵌那份的 data / filter / history | `GroupingResultsContainer.java:198-209` |
| `rebuildGrouping`：取原查询文本 → 生成 groupingQuery → 写内嵌 viewer → 按 `RS_GROUPING_DEFAULT_SORTING` 用函数别名排序 → 列集空则 `showEmptyPresentation` | `GroupingResultsContainer.java:211-272`、`:231-232`、`:237-246`、`:249-265`、`:223-226` |
| 改写形状：非实体容器 + 支持子查询 → 包一层 `SELECT <attr…>, <func> AS <alias> FROM (\n<原查询>\n) src`，再追加 `GROUP BY <attr…>`；`showDuplicatesOnly` 追加 `HAVING … > 1`；别名规则 `COUNT(*)` → `count` | `SQLGroupingQueryGenerator.java:113-131`、`:166-172`、`:174-184`、`:189-222` |

### 28.2 本仓落地（改动表）

| 文件 | 改动 |
| --- | --- |
| `crates/sql/src/filter.rs` | 新增 `pub fn rewrite_with_grouping(original, columns, duplicates_only) -> Result<Rewrite, String>`：生成 `SELECT <keys>, COUNT(*) AS count\nFROM (\n<body 去顶层 LIMIT/ORDER BY>\n) AS src\nGROUP BY <keys>`，`duplicates_only` 追加 `\nHAVING COUNT(*) > 1`；新增 4 个单测 |
| `crates/sql/src/lib.rs` | re-export `rewrite_with_grouping` |
| `crates/result_panel/src/store/entry.rs` | `ResultPlacement` 新增 `Grouping` 变体（语义：只进分组页内嵌结果集，不进标签条） |
| `crates/result_panel/src/store/store.rs` | `push` 函数开头拦截 `Grouping`（不建槽、不动选中）；新增单测 `grouping_results_never_take_a_slot` |
| `crates/result_panel/src/panel/data.rs` | `ResultPanel` 新增 `group_grid`（页内嵌结果集）、`group_query`（最近分组 SQL）、`group_snapshot`（原始投影，供排序口径就地重排） |
| `crates/result_panel/src/panel/lifecycle.rs` | `set_grids` 增加第三个网格参数；新增 `set_group_projection` / `clear_group_projection` / `write_group_grid` / `group_key_names` / `group_query`；构造期初始化新字段 |
| `crates/result_panel/src/panel/events.rs` | `set_group_sort` 改为「已灌过数据就就地重排」（对齐 DBeaver viewer 侧排序，不改 SQL、不重查） |
| `crates/result_panel/src/panel/tabs.rs` | `render_group_tab` 正文从「本地计数列表」换成页内嵌网格（`result_grid::render`，选择器 `editor-result-group-pane` / `editor-result-group-grid`）；工具条最左新增「运行」（`editor-result-group-run`）作为执行侧 rebuild 触发点；`layout` 顺序仍照 `fillToolBar`；新增纯函数 `arrange_group_rows`；`arrange_groups` / `group_table_header` 保留并标 `#[allow(dead_code)]` |
| `crates/result_panel/src/grid/status.rs` | `ResultPane` 新增 `pane_selector` / `grid_selector`（默认主栏名），手写 `Default`；`render` 用它们——同一面板里第二、第三个结果区不再撞选择器 |
| `crates/result_panel/src/panel/render.rs` | 对比栏显式用 `editor-result-compare-grid-pane` / `editor-result-compare-grid` |
| `crates/result_panel/src/ports.rs` | 新增 `ResultRequest::RunGroupingQuery { columns, duplicates_only }` |
| `crates/editor/src/view/host/results/filter.rs` | 新增 `run_grouping_query`（取当前结果 SQL → 改写 → 以 `ResultPlacement::Grouping` 递交；空列集只回提示） |
| `crates/editor/src/view/host/results/dispatch.rs` | 分派 `RunGroupingQuery`；`SetGroupDuplicatesOnly` 之后若已查过且有分组键则自动重查（对齐 `DuplicatesOnlyAction` 即改即查） |
| `crates/editor/src/view/host/exec/pump.rs` | 回执循环新增 `Grouping` 落点：成功写 `set_group_projection`、失败写 `clear_group_projection`，随后 `continue`（不做主键探测 / 注释回填 / 在途标题） |
| `crates/editor/src/view/host/construct.rs` | 构造期建第三个 `TableState`（`group_grid`）并随 `set_grids` 注入 |
| `crates/result_panel/src/panel/tests.rs` | 新增 `the_group_tab_runs_and_shows_a_embedded_result_set`（真窗口：点「运行」→ 断言请求形状 → 注入投影 → 断言页内网格出现、主网格不变）；新增 `group_rows_arrange_by_value_or_count_on_the_client`；`group_tab_shows_a_header_row_with_count_star` 的渲染断言改为「运行按钮在」 |
| `crates/editor/src/view/tests/results_b.rs` | 新增 `run_grouping_query_wraps_group_by_and_never_takes_a_tab`（空列集不递交 / 改写形状 / 标签条不多一份 / 面板记住分组 SQL） |

### 28.3 与 DBeaver 的差异（明账）

1. **触发器**：DBeaver 没有独立「运行」按钮（rebuild 由工具条各动作与 `activatePanel` 内部触发）。本仓执行在宿主侧、面板拿不到执行器，所以需要一个显式触发点；位置同理放在工具条最左（最右会被右栏窄宽度裁出可视区，点击落不到）。
2. **切页不自动重查**：DBeaver 的 `activatePanel` 会 `refresh(false)` 重建。本仓不在切页时自动发查询（避免只是切页就打出 SQL），首查由「运行」发起；`onlyDuplicates` 切换在已查过时自动重查（这一点对齐）。
3. **「编辑列」形态**：DBeaver 是 `GroupingConfigDialog` 的多列编辑表格，本仓是勾选式下拉 + 上移/下移/移除（能力等价，键盘可达）。
4. **`HAVING` 写法**：DBeaver 在方言支持别名时用函数别名（`:177-183`），本仓不知道方言，统一 `HAVING COUNT(*) > 1`（语义等价、所有方言合法）。
5. **函数集**：DBeaver 可编辑（COUNT / SUM / AVG / MIN / MAX…），本仓固定 `COUNT(*)`（与既有本地分组口径一致），函数集扩展属后续。
6. **`aggregateAsStrings` 之类**：分组页没有这些开关，未涉及。
7. **未做像素级目视比对**：页内网格复用主网格的 `DataTable`（同档密度 `RESULT_TABLE_SIZE`），未单独量分组页内嵌 viewer 的行高/表头高。目视验收按既有约定由用户手动做。

### 28.4 验证

| 命令 | 结果 |
| --- | --- |
| `cargo check -p rds-result-panel -p rds-editor --all-targets` | exit 0，0 error 0 warning |
| `cargo test -p rds-sql --lib` | **236 passed / 0 failed**（含 4 个新的分组改写用例） |
| `cargo test -p rds-result-panel --lib` | **426 passed / 0 failed**（原 423 + `the_group_tab_runs_and_shows_a_embedded_result_set` + `grouping_results_never_take_a_slot` + `group_rows_arrange_by_value_or_count_on_the_client`；`group_tab_shows_a_header_row_with_count_star` 的渲染断言改写后仍绿） |
| `cargo test -p rds-editor --lib` | **866 passed / 0 failed / 4 ignored** |
| `cargo test -p rds-editor --lib run_grouping_query` | 1 passed（新用例） |

日志：`.rds/tmp/d14a-test.txt`、`d14-check.txt`、`d14-test-rp.txt`、`d14-test-sql.txt`、`d14-test-ed.txt`、`d14-test-ed2.txt`、`d14-test-group.txt`、`d14-test-rp2.txt`。
## 29. D10 前置取证：矩形单元格选区（网格）

### 29.1 DBeaver 的选区模型（源码行号）

| 事实 | 出处 |
| --- | --- |
| 选区真值 = 一组 `GridPos{col,row}` + 锚点 `focusColumn/focusItem` | `LightGrid.java:1590-1662`（`setSelection` 三个重载都收敛到 `selectCells`）、`:4251-4259`（`selectCells` = 逐格 `addToCellSelection` + `updateSelectionCache`） |
| 普通点击只移动锚点（`focusColumn/focusItem = 点到的格`）；后文再补选区 | `LightGrid.java:3152-3170` |
| **Shift 点击 / 拖拽 = 以锚点与终点为对角的矩形**：`updateCellSelection(newCells, stateMask, dragging, …)` 的 `shift` 分支先 `getSelectionRange` 归一化行列边界，再用「外层行、内层列」双循环逐格加入 | `LightGrid.java:2674-2790`、`:4448-4486`（`getSelectionRange`：列序与行序各自交换成 from≤to） |
| 拖拽行（行头起拖）：整行区间 | `LightGrid.java:3567-3589`、`:4424-4440`（`getCells(startRow, endRow, …)`） |
| 拖拽列（列头起拖）：整列 | `LightGrid.java:3591-3626`、`:4378-4395`（`getCells(col, …)` = 该列所有行） |
| 行头单击 = 整行；列头单击 = 整列；左上角 = 全选 | `LightGrid.java:3127-3137`、`:3174-3184`、`:4261-4263` |
| `shiftSelectionAnchorColumn/Item` 只在 Shift 期间有效，非 Shift 时清空 | `LightGrid.java:2699-2702`、`:2727-2728` |
| Shift+Ctrl = 在当前选区基础上**并集**（`selectedCellsBeforeRangeSelect` 恢复后叠加） | `LightGrid.java:2718-2735` |

一句话：DBeaver 的矩形选区是**加法模型**（Set + 锚点 + 修饰键语义），不是「一个 Range 对象」。

### 29.2 本仓现状与卡点（已核实）

| 事实 | 出处 |
| --- | --- |
| 组件层的选区只有三值：`None` / `Row(usize)` / `Column(usize)` / `Cell(usize,usize)`——**没有矩形 / 多格** | `vendor/gpui-component/src/table/state.rs:64-74` |
| 单格选中的 API 是 `set_selected_cell(row, col)` / `selected_cell()`，只承载一格 | `vendor/gpui-component/src/table/state.rs:618-649`（另 `set_selection(TableSelection)` `:655`） |
| 组件向外发的选区事件 `TableEvent::SelectCell(usize, usize)` **不带修饰键**，也没有 range 变体 | `vendor/gpui-component/src/table/state.rs:76-125`（`:91`、`:98`、`:120`） |
| 网格当前的「选中」有两份：delegate 的整行 `selected_row`（`selected_view_row()` 读）与面板的 `selection_pos: Option<(usize, usize)>` | `crates/result_panel/src/grid/delegate.rs:326-333`、`crates/result_panel/src/panel/*`（`selection_pos`） |
| delegate 可以**自绘每个单元格**（`render_td(row_ix, col_ix, …) -> impl IntoElement`），所以高亮不必依赖组件 | `vendor/gpui-component/src/table/delegate.rs:134-141` |

结论（这就是上一轮把它列为「前置工程」的原因）：**矩形选区的状态模型可以在 delegate 里自建，但「Shift + 点击 / 正文拖拽」这两个入口在组件层没有出口**——组件只发不带修饰键的单格事件，delegate 也拿不到鼠标。

### 29.3 下一刀范围（按此推进，不需要再拍板）

1. **vendor 增量增强（只加不改）**：给组件补一个带修饰键的单元格事件（形如
   `TableEvent::CellClicked { row, col, modifiers }`），在现有 cell 点击处按 `ClickEvent.modifiers`
   发；`TableEvent::SelectCell` 的既有语义与既有消费者全部不动（本仓 `ResultPanel::handle_grid_event`
   等多处 `match` 只新增分支）。
2. **delegate 侧模型**：`ResultGridDelegate` 增 `cell_anchor` + `cell_range`（与 DBeaver 的
   `focusColumn/focusItem` + `selectedCells` 同构；先做「单个矩形」，不做 Shift+Ctrl 并集），
   提供 `in_cell_range(row, col)` 给 `render_td` 画高亮（用主题 `list.active` / `accent` 档色，
   不写裸值）。
3. **交互**：普通点击 = 单格 + 锚点；`Shift` 点击 = 锚点→终点矩形；正文按住拖 = 持续更新矩形
   （拖期间只用已落定的行/列，不做自动滚动，先与 DBeaver 的「滚动是 `showItem/showColumn`
   附带」解耦，自动滚动留作后续）。
   **（2026-11-24 修订）** 本刀只落了点击两态；正文拖拽需要容器级几何换算
   （`DragMoveEvent.bounds` + 行高 + 列宽累积 + 横向滚动偏移 + 固定列），风险面独立，
   拆成第二批单独一刀，理由与现状见 §30.4 第 1 条。
4. **计算页接入**（D11/D12/D13）：选区非空时「按列分组」取选区覆盖的列、「−」移除选中函数、
   工具条补两项——这一步落地后 §25.3 的 D10–D13 一起归零。

风险与边界（写明，不在实现时静默扩张）：

- vendor 改动必须**只增事件、不改既有事件与既有布局**，`git diff -- vendor/` 只允许出现在
  `table/state.rs`（事件枚举 + 发射点）与必要的 `table/table.rs` 点击处理。
- 「自动滚动 / 键盘 Shift+方向键扩选 / Shift+Ctrl 并集」本轮不做（DBeaver 有，但属第二批）。
- 高亮颜色走 `cx.theme().colors`，不写裸值。
## 30. D10 落地：网格矩形单元格选区（点击档）

### 30.1 vendor 的出口（为什么必须动 vendor）

`TableEvent` 原先只有 `SelectCell(row, col)`：既不带修饰键、也没有 range 变体（§29.2 已核）。
但点击处理本身是拿得到修饰键的——`TableState::on_cell_click` 的参数就是 `&ClickEvent`
（`vendor/gpui-component/src/table/state.rs:932-939`），修饰键经 `ClickEvent::modifiers()` 取
（实测是**方法**不是字段，`e.modifiers` 编译报 E0615）。所以出口从来不是「拿不到」，而是
「没往外发」。

### 30.2 vendor 改动（只增不改，2 处）

| 位置 | 改动 |
| --- | --- |
| `vendor/gpui-component/src/table/state.rs` import 块 | 增 `Modifiers`（`ClickEvent::modifiers()` 的返回类型） |
| 同文件 `TableEvent` 枚举 | 在 `SelectCell` 之后**新增** `CellClicked { row_ix, col_ix, modifiers }`，附文档注释说明「在 `SelectCell` 之前发、组件自己的选区仍是单格」 |
| 同文件 `on_cell_click` | `cx.stop_propagation()` 之后、任何选区变更之前 `cx.emit(TableEvent::CellClicked { .. })` |

`SelectCell` 的语义、时序（`set_selected_cell` 之后发）、既有消费者一行未动；本仓
`ResultPanel::handle_grid_event` 与 editor 侧 `construct.rs` 的 `match` 都是新增分支
（后者本就有 `_ =>` 兜底）。

**升级重放提示**：这是 vendor 上的私有 patch 第 4 处（前 3 处见
`docs/architecture/dependencies/core-packages-2026-11-22-upgrade-assessment.md` §11.3），
下次升级 `gpui-component` 时必须一起重打；`git diff -- vendor/` 只应出现上面三行 + 注释。

### 30.3 本仓改动

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/grid/selection.rs`（新） | `CellRange{anchor, focus}`：`single` / `rows` / `cols` / `contains` / `rows_count` / `data_columns`（后者去掉行号槽，长度 = DBeaver 的 `getSelectedColumnCount`），带 3 个单测 |
| `crates/result_panel/src/grid/mod.rs` | `mod selection` + `pub use selection::CellRange` |
| `crates/result_panel/src/grid/delegate.rs` | 字段 `cell_range: Option<CellRange>`；方法 `click_cell(row, col, extend)`（普通=重立锚点、`extend`=只换焦点、无锚点时按普通处理）、`cell_range` / `in_cell_range` / `range_focus` / `selected_data_columns` / `clear_cell_range`；`set_data` 与 `clear` 里复位选区与整行镜像 |
| `crates/result_panel/src/grid/cell.rs` | `render_td`：矩形内**非焦点格**叠绝对铺满的 `tokens.table_active` 覆盖层（与组件画焦点格那层同构），哨兵 `editor-result-cell-in-range-{row}-{col}` |
| `crates/result_panel/src/panel/lifecycle.rs` | `handle_grid_event` 新增 `CellClicked` 分支：`modifiers.shift` → `click_cell(..., true)`；同时清 delegate 的 `selected_row`（DBeaver 点格会清行区间，`LightGrid.java:2705-2717`，本仓此前旧整行色会一直亮着）；`ClearSelection` 分支连矩形一起撤 |
| `crates/result_panel/src/panel/tests.rs` | 新用例 `shift_click_extends_a_rectangular_cell_range_and_paints_it`：emit 生产事件 → 断言锚点/焦点、2×2、矩形内三格铺覆盖层、焦点格不叠第二层（`window.refresh()` 后再 draw，0.10.0 帧语义） |

### 30.4 与 DBeaver 的差异明账（本刀未做，不静默）

1. **正文拖拽选矩形**：DBeaver 按下后拖动即扩矩形（`:3565`）。本仓点击两态已覆盖「任意矩形」，
   拖拽需要容器级几何（`DragMoveEvent.bounds` + 行高 + 列宽累积 + 横向滚动偏移 + 固定列），
   单独立一刀。→ **已由 §33（D10-1）落地**。
2. **Ctrl 并集 / 反选**：DBeaver 是往集合里加/减（`:2718-2735`）。单矩形模型表达不了，Ctrl 现按
   「普通点击」处理（清选区重立锚点）。
3. **选区边框**：DBeaver 的选中矩形有一圈边框（`cellActiveBorder`）。本刀只有底色 + 组件给焦点格
   的边框。
4. **状态行选中计数**：仍是单格口径（1/1/1）。编辑器 `construct.rs` 的 `set_selected_counts` 与
   面板一处写入都要改成读选区规模，属独立小刀。
5. **焦点格居中滚动**：组件的 `set_selected_cell` 每次都 `scroll_to_item(.., Center)`
   （`state.rs:642-645`），DBeaver 只在不可见时滚动。既有行为，未动。

### 30.5 验证

| 项 | 结果 |
| --- | --- |
| `cargo check -p rds-result-panel --tests` | 通过，**0 warning**（曾因 `CellRange` 可见性报 `private_interfaces`，已把模型提升为 `pub` 并 `pub use`） |
| `cargo test -p rds-result-panel --lib` | 430 passed / 0 failed（原 426 + selection 3 + panel 1） |
| `cargo test -p rds-editor` | 867 passed / 4 ignored；`tests/offline_baseline.rs` 5 passed |
| `cargo check -p rds-mock --tests -p rds-workbench` | 通过（另两处使用 `DataTable` 的 crate 也吃到新事件变体） |
| 新增用例点 | 事件链（`CellClicked` → 面板 → delegate）+ 渲染哨兵（三格有、焦点格无） |

## 31. D11 / D12 / D13 落地：计算页按列分组 + 工具条「−」+ 六项顺序

取证对象：`AggregateColumnsPanel.java`（498 行，行号见 §27 与本节）。

### 31.1 DBeaver 的事实

**工具条六项**（`fillToolBar:322-331`）：

| # | 动作 | 形态 | 判据 / 行为 | 源码 |
| --- | --- | --- | --- | --- |
| 1 | `AddFunctionAction` | `MODE_FORCE_TEXT` 文字钮 | 菜单只列 `enabledFunctions` 里**没有**的函数；`isEnabled` 无判据 | `:365-401`、`:403-416` |
| 2 | `RemoveFunctionAction` | 图标钮 | `isEnabled = aggregateTable != null && !isDisposed() && getSelectionCount() > 0`；`run()` 遍历 `getSelection()` 逐个 `enabledFunctions.remove(func)` | `:418-436` |
| 3 | `ResetFunctionsAction` | 图标钮 | `enabledFunctions.clear()` + `loadDefaultFunctions()`（只收 `isDefault()`，按 label 排序） | `:438-449`、`:157-164` |
| 4 | `ToolbarSeparator` | 分隔 | — | `:322-331` |
| 5 | `GroupByColumnsAction` | CHECK | `run()` 只 `groupByColumns = !groupByColumns; setChecked; refresh(false)` | `:333-346` |
| 6 | `ValueTypeToggleAction` | CHECK | `aggregateAsStrings` 翻转 + `updatePanelActions()` + `refresh(false)` | `:348-363` |

**分组聚合口径**（`:199-315`）：
- `refresh(force)`：清表 → 有 `IResultSetSelection` 就 `aggregateSelection` → `packColumns` → `saveSettings`（`:199-214`）
- `aggregateSelection`：`groupByColumns == true` 时按 `selection.getElementAttribute(element)` 归组成
  `LinkedHashMap<attr, List<Object>>`，每 attr 一个 `TreeItem`（列名 + 类型图标）+ `setExpanded(true)`（`:221-262`）；
  否则把所有值汇成一个列表平铺（`:250-261`）
- `aggregateValues`：每个启用函数一行，`accumulate(element, aggregateAsStrings)` 累计；
  `funcCount <= 0` 的行不出值（`:294-297`）；Double/Float/BigDecimal 走 `DOUBLE_FORMAT`，
  Int/Long/Short 走 `INTEGER_FORMAT`（`:303-309`）

### 31.2 本仓卡点与解法

| 卡点 | 解法 |
| --- | --- |
| 「−」删的是 Tree 选中项，本仓统计页是平铺行、无选中模型 | 新增 `stats_selected: Option<StatsItem>` 单选态，**行点击**就是选中（`:424` 的 `isEnabled` 只要求「有选中」，不要求多选）；行尾 × 保留 |
| 「按列分组」要「选区覆盖了哪些列、每列哪些值」 | D10 的 `CellRange` 提供列区间；`delegate.data_column_checked(view_col)` 把**视图列**换成**数据列**（列可重排 / 可隐藏，不能 `col - 1` 硬算）；`column_values_in_rows(col, rows)` 只取**行区间内**的值 |
| 单列页与分组页要同一套取值 / 极值 / 去重 / 中位数口径 | 抽 `stats_entries_for_column(delegate, data_col, Option<rows>)`：`None` = 整列（数值口径可走事实档列式聚合）、`Some` = 区间（必须走显示取值，事实档无法按区间切） |
| 统计页不是树，分组的「每列一组」怎么落地 | `render_stats_grouped`：每列一个组标题行（列名 + 底 hairline，哨兵 `editor-result-stats-group-{data_col}`）+ 该列函数行；不引 `Tree` |

### 31.3 本仓改动表

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/ports.rs` | 新请求 `SetStatsGroupBy(bool)`、`SelectStatsItem(Option<StatsItem>)`（紧跟 `SetStatsMode`） |
| `crates/result_panel/src/panel/data.rs` | 新字段 `stats_group_by: bool`、`stats_selected: Option<StatsItem>` |
| `crates/result_panel/src/panel/lifecycle.rs` | 默认值：`stats_group_by: false`、`stats_selected: None` |
| `crates/result_panel/src/panel/events.rs` | `set_stats_group_by(on, cx)`、`select_stats_item(item, cx)`；`remove_stats_item` 顺带清「删掉的就是选中行」的选中态 |
| `crates/result_panel/src/panel/tabs.rs` | `render_stats_tab` 加分组分支；新增 `stats_entries_for_column`（共用取值口径）与 `render_stats_grouped`（组标题 + 组内行）；`stats_toolbar` 补「−」「分组」两钮、顺序改为 ＋ / − / Reset / 分隔 / 分组 / 口径；`stat_row` 加行点击选中 + `tokens.table_active` 底色 |
| `crates/result_panel/src/grid/text.rs` | 新访问器 `column_values_in_rows(column, (top, bottom))`（口径同 `column_values`，只收窄行范围，本地行 / 被删行仍跳过） |
| `crates/editor/src/view/host/results/dispatch.rs` | `SetStatsGroupBy` / `SelectStatsItem` 两个分支（纯展示态、不落盘） |
| `crates/result_panel/src/panel/tests.rs` | 新用例 `stats_group_by_splits_selected_columns_and_rows`：工具条两钮在 + 行选中态 + 分组两组标题 + 区间口径「2 行」vs 整列「4 行」+ 删除后选中态作废 |

### 31.4 与 DBeaver 的差异明账（未做，不静默）

1. **组标题无列类型图标**：DBeaver 的 `TreeItem` 带 `getColumnImage`（`:236`）。本仓组标题只有列名，
   类型图标在列浏览器已有，统计页不重复。
2. **选区为空时退回当前作用列**：DBeaver `aggregateSelection` 需要 `IResultSetSelection`，
   无选区时整表为空。本仓保留「作用列（选中格 → 列浏览器选中列）」整列视图，不弹空表。
3. **分组开关不落盘**：DBeaver 存 `PARAM_GROUP_BY_COLUMNS`（`:131`/`:167`）。本仓沿用
   「口径切换（`SetStatsMode`）不落盘」的既有取舍。
4. **组内多列不同行区间**：DBeaver 的分组按「每个选中格所属列 + 该格的值」归集，行集合是**逐格**的；
   本仓按矩形行区间取整列值。单矩形模型下二者等价（矩形内每列的格集合就是同一行区间）。
5. **「−」删除后函数集为空**：DBeaver 允许删空（`packColumns` 后表空）；本仓同样允许，
   并给「没有启用任何统计项：点 ＋ 添加」的空态提示。

### 31.5 验证

| 项 | 结果 |
| --- | --- |
| `cargo check -p rds-result-panel -p rds-editor` | 通过，0 warning |
| `cargo test -p rds-result-panel` | **431 passed / 0 failed**（原 430 + 本刀 1） |
| `cargo test -p rds-editor` | 867 passed + 4 ignored；离线 `tests/offline_baseline.rs` 5 passed |
| `cargo check -p rds-workbench` | 通过 |
| 新增用例点 | 工具条两钮真渲染（`editor-result-stat-remove-tool` / `editor-result-stat-group`）、行点击选中、分组两组标题（`editor-result-stats-group-1` / `-2`）、区间口径 vs 整列口径的计数差、删除后选中态清空 |

**门禁前置修补（未随本刀提交）**：`cargo test` 曾被 i18n 合流遗留缺陷挡住 ——
`crates/result_panel/src/analysis.rs:281` 的 `tr("计数行数")` 在 `#[cfg(test)]` 模块内没有 `tr`
在作用域（该模块是显式列举 import 的「安全模式」）。临时补 `use super::tr;` 一行后测试全绿；
该修复是否单独成刀待定。

## §32 D10-4 状态行「已选 N 格」按矩形选区规模

### 32.1 DBeaver 事实（源码实证）

- `ResultSetStatListener.java:80-92`：三项文案的数据全部来自 `IResultSetSelectionExt`
  （`getSelectedRowCount` / `getSelectedColumnCount` / `getSelectedCellCount`）。
- `SpreadsheetPresentation.java:2038-2051`（内部类 `GridSelection implements IResultSetSelectionExt`）
  三个方法**全部转调** `spreadsheet.get*SelectionSize()`，自己不另算。
- `Spreadsheet extends LightGrid`（`Spreadsheet.java:53`），三个 size 都继承自 `LightGrid`：
  `getCellSelectionSize` = `selectedCells.size()`（`:4349-4351`）、
  `getColumnSelectionSize` = `selectedColumns.size()`（`:4361-4363`）、
  `getRowSelectionSize` = `selectedRows.size()`（`:4374-4376`）。
- 关键在于 `updateSelectionCache()`（`:2863-2882`）：`selectedColumns` / `selectedRows`
  每次从 `selectedCells` **反推** —— 只要有一格落在该行（该列）就计入。因此
  **矩形 rows × cols** 的计数就是 `(rows, cols, rows × cols)`，而不是恒定的 `(1,1,1)`。
- 整行选中：`setSelection(int)`（`:1590-1597`）= `selectedCells.clear()` +
  `selectCells(getCells(index))`，而 `getCells(row)`（`:4397-4401`）把该行
  **所有 columns** 填进集合 → `(1, 列数, 列数)`。

### 32.2 本仓改动

改前：`crates/editor/src/view/host/construct.rs` 的 `SelectCell` 分支写死
`set_selected_counts(1, 1, 1, cx)` —— 状态行在矩形选区下永远报 1 格。

改后：

- `CellRange::data_cells_count()` = `rows_count() × data_columns().len()`（口径同
  `getCellSelectionSize`：行号槽那列不算数据格）。
- 新增 `grid::selection_counts(range: Option<&CellRange>) -> (rows, cols, cells)`：
  **口径只有一份**，由 `grid/mod.rs` re-export；`None`（还没建立矩形 / 键盘刚落到第一格）
  按单格 `(1, 1, 1)`。
- `construct.rs::SelectCell` 改为读 `delegate.cell_range()`，交给 `selection_counts` 算，
  再写 `set_selected_counts`。

**为什么计数权威放在编辑器侧**：`on_cell_click`
（`vendor/gpui-component/src/table/state.rs:943-989`）先 `emit(CellClicked)`，随后
`set_selected_cell`；而 `set_selected_cell`（`:648-660`）**无条件** `emit(SelectCell)`
（`:658`）——所以每次点击必有一条 `SelectCell` 紧随 `CellClicked`。面板侧
`handle_grid_event` 的 `CellClicked` 分支因此**不写** counts：两处订阅（编辑器 / 面板）
写同一字段时的执行顺序不确定，双写会出现「矩形值被单格值盖掉」的抖动。

**唯一不发 `SelectCell` 的路径**：行头隐藏时「点击已选格 → 升级整行」
（`on_cell_click:973-982` 直接 `return`），此时计数由编辑器 `SelectRow` 分支给出
`(1, visible_columns, visible_columns)` —— 与 DBeaver `getCells(row)` 的「整行所有列」同义。

### 32.3 改动表

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/grid/selection.rs` | 新 `data_cells_count()`、新 `selection_counts(Option<&CellRange>)`；单测 `data_cells_count_multiplies_rows_by_data_columns`、`selection_counts_mirror_the_rectangles_rows_and_data_columns` |
| `crates/result_panel/src/grid/mod.rs` | re-export `selection_counts` |
| `crates/editor/src/view/host/construct.rs` | `SelectCell` 分支由写死 `1/1/1` 改为读选区规模（算式走 `selection_counts`） |
| `crates/editor/src/view/tests/results_a.rs` | 新用例 `the_status_row_counts_the_rectangular_cell_selection`：按生产序列 emit `CellClicked`（带 Shift）+ `SelectCell`，断言单格 `(1,1,1)`、1 行 2 列 → `(1,2,2)` |

### 32.4 与 DBeaver 的差异明账（未做，不静默）

1. **整行选中「列数」**：DBeaver 的行头不在 `columns` 里，`getCells(row)` 只遍历数据列，
   所以「列数」= 数据列数，与本仓 `visible_column_count()` 同口径。§34.1 已取证
   `Spreadsheet.java:108`（结果集构造即 `setRowHeaderVisible(true)`），原先「可能差 1」作废。
2. **行号槽列**：若矩形只覆盖视图列 0，本仓算出 `(1, 0, 0)`。生产路径下点行头走
   `SelectRow`（不发 `CellClicked`），不会产生这种 range；防御性登记。
3. **Ctrl 并集 / 反选仍缺**（§30.4 #2）：DBeaver 的「并集计数」要多矩形模型才表达，
   当前一律按单矩形算。
4. **正文拖拽选矩形**：已由 §33（D10-1）落地——拖拽沿用同一口径，业务侧零改动。

### 32.5 验证

| 项 | 结果 |
| --- | --- |
| `cargo test -p rds-result-panel --lib` | **433 passed / 0 failed**（原 431 + 本刀 2） |
| `cargo check -p rds-editor --tests` | Finished，0 warning |
| `cargo test -p rds-editor` | **868 passed + 4 ignored**（原 867 + 本刀 1）；离线 `tests/offline_baseline.rs` 5 passed |
| 新增用例点 | 单格 `(1,1,1)`；Shift 点同行下一列 → `(1,2,2)`（写死 `1/1/1` 的旧实现必失败） |

**环境说明**：本轮与并行线共用同一 `target` 目录，期间并行线在跑
`cargo check -p rds-workbench --tests`；并发起过两次 `E0462`（staticlib vs rlib）与一次
`memory allocation failed`。等其进程结束后顺序执行，全绿。结论：本仓门禁必须**串行**跑
（`CARGO_BUILD_JOBS=1` 之外还要保证同一 target 只有一个 cargo）。

## 33. D10-1 落地：正文拖拽选矩形

取证对象：`LightGrid.java` 的 `mouseMove` 全链（`:3480-3610`）。

### 33.1 DBeaver 的事实

**起拖条件**（`:3507-3516`）：`!cellDragSelectionOccurring && cellSelectedOnLastMouseDown`
成立时置 `cellDragSelectionOccurring = true`、光标换 `SWT.CURSOR_CROSS`，并记
`cellDragCTRL = (stateMask & SWT.MOD1) != 0`（Ctrl 按住时把当前选集快照进
`selectedCellsBeforeRangeSelect`，供并集用）。`cellSelectedOnLastMouseDown` 由鼠标按下设值
（`:3166`）：按下时先 `updateCellSelection(newCell, stateMask, false, true, MOUSE)` 选中该格，
再取 `getCellSelectionCount() > 0` —— 所以**按下任意格都能起拖**。

**每次 move**（`:3537-3566`）：

| # | 行为 | 源码 |
| --- | --- | --- |
| 1 | `ctrlFlag = cellDragCTRL ? SWT.MOD1 : SWT.NONE` | `:3537` |
| 2 | `handleCellHover(e.x, e.y)` 得 hover 列/行；hover 行 < 0 时按方向取 `min(itemCount-1, bottomIndex+1)` / `max(0, topIndex-1)`；hover 列为空时取 `columns.getLast()` / `columns.getFirst()` | `:3539-3560` |
| 3 | `showColumn(intentColumn)` + `showItem(intentItem)`：把目标格滚进可视区 | `:3562-3563` |
| 4 | `updateCellSelection(newCell, ctrlFlag \| SWT.MOD2, true, false, MOUSE)` —— **`SWT.MOD2` 就是 Shift**，即「终点换成 hover 格、锚点不动」 | `:3564-3565` |

同一段还有两条同族分支：整行拖（`:3567-3589`，`getCells(intentItem, focusItem, cells)` 取整行区间）
与整列拖（`:3591+`，从 `focusColumn` 逐列 `getCells(iterCol, newSelected)` 累加）。

**与 D10 的关系**：拖拽在 DBeaver 里走的就是「带 Shift 的同一个 `updateCellSelection`」，
等价于本仓 D10 已经接好的 `click_cell(row, col, extend = true)`。所以拖拽只需把「鼠标位置」
换成「带 Shift 的格点击」，delegate / 面板 / 状态行计数全部零改动。

### 33.2 本仓实现（vendor 私有 patch 第 2 处）

| 层 | 落点 | 做法 |
| --- | --- | --- |
| 纯函数 | `vendor/gpui-component/src/table/hit.rs`（**新文件**） | `CellHitGeometry`（`body_top` / `row_height` / `scroll_x` / `scroll_y` / `width` / `left_fixed_width` / `right_fixed_width` / `left_fixed_count` / `right_fixed_count` / `col_widths` / `rows_count`）+ `cell_at_point(x, y, &geometry)`：表头区 → `None`（对应 DBeaver `getRow` 返回 -1）；行按 `(y - body_top + scroll_y) / row_height` 再 clamp 到 `rows_count - 1`；列分「左固定区 / 可滚动区（扣左固定宽再叠 `scroll_x`）/ 右固定区」三支累积，越界收拢到首末行列 |
| 布局快照 | `state.rs::cell_hit_geometry(width, cx)` | 取 `col_groups[i].width`、`header_band_height()`、`options.size.table_row_height()`、两个滚动 handle 的 offset、`fixed_cols_count(Left/Right)`、`delegate.rows_count(cx)` |
| 拖拽负载 | `state.rs::SelectCellsDrag { owner: EntityId }` | 只带发起者 id：多表实例共存时 `on_drag_move` 会派发到每个挂了回调的元素，用 owner 过滤 |
| 挂载点 | `state.rs` 的 `render_table_row`（可滚动列）与 `render_fixed_cells`（固定列） | 任意格都挂 `on_drag`（`cell_selectable` 下无条件），负载带起点格 + 一次性 `Rc<Cell<bool>>`，首个 `DragMove` 由 `begin_drag_at` 补立锚点（见 §33.6）；容器 `inner_table` 挂 `on_drag_move` |
| 判定 | `state.rs::handle_select_cells_drag` | 相对坐标 = `e.event.position - e.bounds.origin`；owner 校验后转调 `drag_select_to` |
| 出口 | `state.rs::drag_select_to(x, y, width, cx)` | `cell_at_point` → 与当前选中格相同则不动 → 否则 `emit(CellClicked{row, col, modifiers: shift})` + `set_selected_cell` + `scroll_to_item(row, Nearest)` + `scroll_to_col(col)` |

**为什么复用 `CellClicked{shift}` 而不新增事件**：D10 已把「带修饰键的格点击」接到 delegate
（`crates/result_panel/src/panel/lifecycle.rs:473-488`），拖拽自动继承选中覆盖层、状态行计数、
值详情跟随、JSON 树重建等全部既有行为。

**为什么滚动用 `Nearest`**：`ScrollStrategy` 只有 `Top/Center/Bottom/Nearest`（**没有 `Minimal`**），
`Nearest` 是最接近「不可见才滚」的一档。

### 33.3 改动表

| 文件 | 变化 |
| --- | --- |
| `vendor/gpui-component/src/table/hit.rs` | **新增 162 行**：`CellHitGeometry` + `cell_at_point` + 5 条内置单测 |
| `vendor/gpui-component/src/table/mod.rs` | +2：`mod hit;` + `pub use hit::*;` |
| `vendor/gpui-component/src/table/state.rs` | +161/−5（D10-1）+68/−28（D10-1b）：`SelectCellsDrag`、`cell_hit_geometry`、`handle_select_cells_drag`、`drag_select_to`、`debug_hit_geometry`、两处 `on_drag`、容器 `on_drag_move`、`EntityId` 入 imports |
| `crates/result_panel/src/grid/selection.rs` | +51：命中换算用例（表头 / 行号槽 / 滚动量 / 两侧固定列 / 越界收拢，6 类断言） |
| `crates/result_panel/src/panel/tests.rs` | +89/−1（D10-1）+81（D10-1b）：`dragging_inside_the_grid_extends_the_cell_range`（锚点 → 拖到 (1,2) → 2×2 矩形） |

`vendor` 不在 workspace 成员里，`hit.rs` 内置的 5 条单测不随 `cargo test -p rds-result-panel` 跑；
同一批口径已在 `grid/selection.rs` 的门禁用例里复刻（见 33.5）。

### 33.4 与 DBeaver 的差异明账（未做，不静默）

1. **任意格起拖**：已由 §33.6（D10-1b）落地——`on_drag` 放宽到全部格，起点格未选中时由首个
   `DragMove` 补一次无修饰 `CellClicked` 立锚点（对应 DBeaver `:3152-3170` 的「按下即选」）。
2. **边缘滚动差一格**：DBeaver 在 hover 越界时把终点设为 `bottomIndex+1` / `topIndex-1` 再
   `showItem`（`:3543-3550`）；本仓先把坐标收拢到首末行列，再 `scroll_to_item(.., Nearest)`，
   等价于「把边界格滚进可视区」，但不会跨到可视区之外的下一格。
3. **Ctrl 并集 / 反选**：DBeaver 在 Ctrl 拖时先快照 `selectedCellsBeforeRangeSelect`（`:3512-3515`）
   再做并集；本仓单矩形模型表达不了，Ctrl 仍按单矩形处理（同 §30.4 #2）。
4. **整行拖 / 整列拖**：DBeaver 另有两条分支（`:3567-3589`、`:3591+`，从行头 / 列头起拖）；
   本仓行头是自绘 `#` 列、列头拖拽已用于列序重排，未接。
5. **拖拽光标**：DBeaver 换成 `SWT.CURSOR_CROSS`（`:3510`）；本仓未改光标。
6. **headless 覆盖边界**：gpui 的真实拖拽状态机不在测试窗口里跑，`on_drag` 挂载与 `on_drag_move`
   派发只有实机可见；门禁覆盖的是「几何换算 → 事件 → 矩形」这条链。

### 33.5 验证

| 项 | 结果 |
| --- | --- |
| `cargo test -p rds-result-panel --lib` | **436 passed / 0 failed**（435 + D10-1b 用例 1；命中用例口径扩成 6 类断言） |
| `cargo test -p rds-editor` | **868 passed + 4 ignored**；离线 `tests/offline_baseline.rs` 5 passed |
| `cargo check -p rds-result-panel --lib` | Finished，0 warning |
| `cargo check -p rds-workbench --lib` | Finished，0 warning（vendor patch 不破坏主程序编译） |
| 新增用例点 | 锚点 (0,1) 不动、焦点到 (1,2)、`rows_count() == 2`、`data_columns() == [1,2]`；并分两层自证：① `cell_at_point(目标点) == (1,2)`；② 组件选中格已搬到 (1,2) |
| D10-1b 用例点 | 旧锚点 (0,1) → `begin_drag_at(1, 2)` 把锚点与组件选中格一起搬到起点格 → 拖到 (0,1) → 矩形 (0,1)..(1,2) |

### 33.6 D10-1b：任意格起拖（补 §33.4 #1）

**DBeaver 事实**：鼠标按下走 `:3152-3170`（`e.button == 1` 分支）——先
`updateCellSelection(new GridPos(col.getIndex(), row), e.stateMask, false, true, MOUSE)` 把该格选进
选区，再 `cellSelectedOnLastMouseDown = (getCellSelectionCount() > 0)`（`:3166`）。所以 `:3507`
的起拖条件「按下时选区非空」对**任意格**都成立。

**本仓约束**：`on_drag` 是渲染期决定挂载的，按下那一帧起点格还没有变成 `selected_cell`，
所以「只给选中格挂 `on_drag`」天然只能从已选中格起拖。

**做法**：

| 改动 | 内容 |
| --- | --- |
| 挂载条件 | 从 `is_cell_selected && cell_selectable` 放宽为 `cell_selectable`（可滚动列与固定列两处） |
| 拖拽负载 | `SelectCellsDrag { owner, start: (row, col), anchored: Rc<Cell<bool>> }`。`anchored` 由 `SelectCellsDrag::new` 每次渲染新建：一次拖拽内的多次 `DragMove` 共享同一个 `Rc`，下一手势从新渲染出来的元素启动会拿到新的 `Rc` → **不需要在拖拽结束时清理** |
| 首个 DragMove | `!anchored.replace(true)` 时调 `begin_drag_at(start)`：起点格不是当前选中格就 emit 一次**无修饰** `CellClicked` + `set_selected_cell`，等价 DBeaver 的「按下即选中」 |
| 之后 | 照旧 `drag_select_to`（把 hover 格当带 Shift 的终点） |

**顺带核实的一条上游行为**：gpui-fast `div.rs:53` 的 `DRAG_THRESHOLD = 2.`（px）——移动超过 2px
就启动拖拽，并把 `clicked_state` 清空（`:3127`，这一次 click 不再触发）。「拖拽启动吞掉点击」是
组件既有语义；`begin_drag_at` 同时补上「点一下就被选中」的观感，不会出现「按了没反应」。

**验证**：`cargo test -p rds-result-panel --lib` **436 passed / 0 failed**（新增用例
`dragging_from_an_unselected_cell_anchors_there_first`）；`cargo test -p rds-editor` 868 passed +
4 ignored（离线 5 passed）；`cargo check -p rds-workbench --lib` Finished、0 warning。

**调试记录（值得留的经验）**：拖拽用例最初卡在「焦点不动」，`cell_selectable` 断言把方向钉住 ——
`panel_harness`（`panel/tests.rs:2052`）给别的用例建的是裸 `TableState::new`，
`cell_selectable` 默认 **false**，而生产走 `grid::new_table_state`（`grid/status.rs:488` 开了它）。
面板级用例只要碰「单元格选择」相关能力，都要按生产口径补齐开关，否则测的是另一张表。
另一坑：`cx.debug_bounds` 只认 `debug_selector` 的 div，`.id()` + `test_support()` 的元素取不到 bounds，
所以坐标改由 `debug_hit_geometry` 自算（`drag_select_to` 内部用的同一份）。

**并发**：本仓门禁串行跑（同 §32.5）。

## 34. D10-2 落地：行号槽按下 = 整行

取证对象：`LightGrid.java` 的行头链（`:226` 字段、`:1574-1579` 开关、`:1866-1898` 宽度、
`:3107-3151` 按下、`:3567-3589` 拖动、`:3174-3184` 交角），`GridRowRenderer.java`（137 行，
渲染与宽度算式），`SpreadsheetPresentation.java:3365-3409`（行头文本与配色槽）。

### 34.1 DBeaver 的事实

**行号列恒开**：`LightGrid.rowHeaderVisible` 默认 `false`（`:226`），但结果集视图的
`Spreadsheet` 构造就 `super.setRowHeaderVisible(true)`（`Spreadsheet.java:108`）→ DBeaver 结果集
**始终**有行号列。（§32.4 里「`showRowNumber` 默认值未取证」由本条结清。）

**宽度算式**（`computeHeaderSizes`，`:1866-1898`）：起点 `DEFAULT_ROW_HEADER_WIDTH = 30`
（`:72`），逐行取 `rowHeaderRenderer.computeHeaderWidth` 的最大值，再夹到
`MIN_ROW_HEADER_WIDTH = 40`（`:73`）与 `MAX_ROW_HEADER_WIDTH = 400`（`:74`）。
→ **实际下限是 40**（只记 30 是漏了这层 clamp）。内容宽算式见
`GridRowRenderer.computeHeaderWidth`（`:114-129`）：`LEFT_MARGIN + RIGHT_MARGIN`（4 + 4）
+ 展开箭头位（`EXPANDED_BOUNDS.width + EXPANDER_SPACING`，可展开行才有）+ 行图标宽 + `IMAGE_SPACING`
+ 文本宽 + `level * LEVEL_SPACING`。

**行号文本**（`SpreadsheetPresentation.GridLabelProvider#getText`，`:3390-3408`）：
列（记录模式）返回 `"行 #" + (visualNumber + 1)`；**行**且非记录模式返回 `row.toString()`
（`:3396-3398`）；其余行对象走 `String.valueOf(row.getRelativeIndex() + 1)`（`:3404-3405`）
——即**相对序号 + 1**，从 1 起。

**行头绘制**（`GridRowRenderer.paint`，`:47-112`）：背景 `getHeaderBackground(element, selected)`；
右边线与底边线 `getHeaderBorder`（`:59-71`）；文本从 `x = LEFT_MARGIN(4)` 起、垂直居中
（`:104`）；当前行用 `boldFont`（`:96`）。配色槽在结果集侧：`cellHeaderBackground` /
选中 `cellHeaderSelectionBackground` / `cellHeaderForeground` / `cellHeaderBorder`
（`SpreadsheetPresentation.java:3365-3381`）。

**行头按下**（`:3107-3151`，条件 `col == null && rowHeaderVisible && e.x <= rowHeaderWidth`）：
无修饰 → `getCells(row, cells)`（**整行 = 全部数据列**，行头格本身不在 `columns` 里），
`cellRowSelectedOnLastMouseDown = true`（`:3138`），并把 `focusColumn` 挪到行头右侧第一列
（`:3142`）、`focusItem = row`；Shift → `getCells(row, focusItem, cells)`（行区间，列仍全宽）。

**行头拖动**（`:3567-3589`）：`getCells(intentItem, focusItem, cells)` —— 焦点行与锚点行之间的
**整行区间**，非 Ctrl 时替换选区。边界：hover 落在表头之上且 `topIndex > 0` → `topIndex - 1`，
否则 0；底部越界 → `topIndex + 1`（与格拖的收拢算式不同）。

**左上角交角**（`:3174-3184`）：点击 = 全选（`selectAllCellsInternal`）。

**列头**（`:3185-3204` 按下 / `:3591-3626` 拖动）：按下 = `getCells(col, cells)` 整列；
拖动 = 从 hover 列逐步走到 `focusColumn`，每列 `getCells(iterCol, …)`，即**整列区间**
（`dragging = true`）。

### 34.2 本仓现状（先取证，避免走弯路）

- 本仓**已经有行号列**，形态不是组件 `row_header`，而是 delegate 的**首个组件列**：
  `table_impl.rs::columns_count` = `columns.len() + 1 - hidden.len()`（转置固定 3 列 `#`/字段/值），
  渲染走 `grid/cell.rs:267-313` 的行号槽分支（灰字、每格表头底色、当前行加粗、选中行留空让行容器色透上、
  右键记 `context_cell=(row,0)` 供「复制行号」），注释里已按 `GridRowRenderer:55-70 / :96` 对齐。
- 组件自带的 `row_header` 是**关的**（`grid/status.rs:492` `.row_header(false)`）：它在 vendor
  `render_row_header_cell` 里是 `w_3()` 宽、无文本、只给 `row_selectable` 挂点击，与既有行号槽
  **重复**。→ 本刀**不动 vendor**，只补口径。

### 34.3 落点

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/grid/delegate.rs` | `click_cell` 命中行号槽时把矩形两侧归一：锚点列钉 0、焦点列钉末列（Shift 只换行的一端、列仍全宽）；新增私有 `last_view_column()`（末列下标 = 可见数据列数，转置恒 2，无结果 0） |
| `crates/result_panel/src/grid/tests.rs` | 用例 `clicking_the_row_number_slot_covers_the_whole_row`：单行全宽 → Shift 行区间（列仍全宽）→ 数据格点击不受影响 → 隐藏一列后整行收窄 |
| `crates/result_panel/src/panel/tests.rs` | 用例 `clicking_the_row_number_slot_selects_the_whole_row`：按生产路径 `begin_drag_at(1, 0)`（先 `cell_selectable = true`，裸 `TableState::new` 默认关）→ 选区 (1,1)..(1,末列)、`selection_counts == (1,2,2)`、`selection_pos` 仍是 `(1,0)` |

### 34.4 差异明账（未做，不静默）

1. **值详情焦点不回落第一列**：DBeaver 行头按下把 `focusColumn` 挪到右侧第一列（`:3142`），
   本仓 `selection_pos` 同时是「当前数据格」判据（「修改」按钮置灰、列归属、宿主提示三处既有
   用例锁定），归一化会让 `edit_row_button_is_disabled_in_record_view` /
   `select_column_goes_to_the_column_under_the_current_cell` 失效 → **保持现口径**，登记为差异。
2. **列头拖动（整列区间）未做**：vendor 列头的 `on_drag` 已被列重排 `DragColumn` 占用
   （`state.rs::render_th:2156-2169`），要加选区拖必须先决定「列头拖拽优先做重排还是做选区」。
3. **交角点击全选未做**（`:3174-3184`）：需要 `selectAll` 的 delegate 径路与整表选区表达。
4. **行头 Ctrl 追加 / Shift 区间只做了一半**：Shift 已对齐（列全宽 + 行区间），Ctrl 并集仍缺
   （同 §30.4 #2 的单矩形模型限制）。
5. **行号列宽度未按 40 下限夹**：本仓行号槽宽度由 `fit.rs:48` 钉死、不参与自适应，
   DBeaver 是 `max(40, 内容宽)` 且随行号位数增长——属「尺寸」类，留到统一尺寸回补批次。

### 34.5 验证

| 项 | 结果 |
| --- | --- |
| `cargo test -p rds-result-panel --lib` | **438 passed / 0 failed**（436 + 本刀 2） |
| `cargo test -p rds-editor` | **868 passed + 4 ignored**；离线 `tests/offline_baseline.rs` 5 passed |
| `cargo check -p rds-workbench --lib` | Finished，0 warning |

**本刀没有 vendor 改动**：D10-1 / D10-1b 的私有 patch 保持原样（`table/{state,hit}.rs`），
`vendor` 仍只此五处。

## §35 D10-3：左上交角点击 = 全选

（结 §34.4 #3）

### 35.1 DBeaver 取证

**命中条件**（`LightGrid.java:3174`）：`e.button == 1 && rowHeaderVisible && e.x <= rowHeaderWidth && e.y < headerHeight`
—— 即「行头带 ∩ 列头带」那一格（左上交角）。分支体（`:3175-3184`）：

- `getItemCount() == 0` → **直接 return**（不选、不发 Selection 事件）；
- `selectAllCellsInternal(e.stateMask)`（`:4272-4296`）：再挡两档空表（`columns.isEmpty()` 与
  `getItemCount() == 0` 都 `return null`），随后 `focusColumn = columns.getFirst()`、`focusItem = 0`，
  把 `getAllCells()` 整批塞进 `updateCellSelection(…, EventSource.KEYBOARD)`，最后把
  `focusColumn` / `focusItem` **恢复原值**；
- 交角分支自己再把 `focusColumn = getColumn(new Point(rowHeaderWidth + 1, 1))`（= 数据区第一列）、
  `focusItem = getTopIndex()`（`:3183-3184`）。

**全选集合**（`getAllCells`，`:4404-4415`）= `itemCount × columns.size()` 的笛卡尔积（列序从 0 起）。
行头不在 `columns` 里，所以「全选」= 全部行 × 全部数据列；状态行三层数字（行 / 列 / 格）按
`ResultSetStatListener.java:80-92` 的 `IResultSetSelectionExt` 口径算。

**结论**：交角全选的语义是「整表矩形」，不是「只选数据区的高亮格」——行头列只是不在计数里。

### 35.2 本仓的关键落差：空态占位行

本仓 `set_data`（`grid/delegate.rs:494-509`）在「0 行 + 有列」时**塞一行全空行**撑起表头
（组件 `rows = 0` 会切到 `render_empty` 不画表头），于是空结果集在网格里看起来有 1 行、
`view_len() == 1`。DBeaver 那边 `getItemCount() == 0`，交角全选什么都不选（`:4276-4277`）；
本仓若照 `view_len()` 全选，状态行会把空结果报成「1 行 × N 列」——数字是错的。

落法：`set_data` 塞行的那一处同时置 `placeholder_row = true`（字段 `grid/delegate.rs:160`、
置位 `:502`，`clear()` 复位 `:757`），`select_all_cells` 认这个标记，按 DBeaver 口径返回 `false`。

### 35.3 落点

| 文件 | 改动 |
| --- | --- |
| `crates/result_panel/src/grid/delegate.rs` | 新增 `select_all_cells()`（`:415`）：空表（无列 / 无行 / 只有占位行）返回 `false`，否则矩形 = `(0,0)`..`(末行, 末列)`（末列沿用 `last_view_column()`，隐藏列后随之收窄）；新增 `placeholder_row` 字段（`:160`）及 `set_data` / `clear` 两处维护 |
| `crates/result_panel/src/grid/header.rs` | `render_th` 把「行号槽」这一档从原合并分支拆出来（`:234` 起）：元素加 `.id(("editor-result-th-corner", col_ix))`（`on_click` 需要 Stateful），点击即 `cx.stop_propagation()` → `table.update(… select_all_cells())` 成功才 `cx.notify()` |
| `crates/result_panel/src/grid/tests.rs` | `selecting_all_cells_covers_every_row_and_column`（`:5188`，纯函数：整表矩形 / 隐藏一列后收窄 / 无列与无行两档不选）；`clicking_the_top_left_corner_selects_every_cell`（`:5227`，`HeaderHarness` 用真鼠标事件打 `editor-result-th-0`） |

**为什么必须截冒泡**：组件把 `on_col_head_click` 挂在列头格**外层**（`vendor/.../table/state.rs:2139-2141`），
那条路报 `SelectColumn(0)`，面板收到就是「按此列排序」——行号槽没有可排的键。与列头菜单 guard
同一手法（`grid/header.rs:311-322`）。

**零 vendor 改动**：交角落在本仓自己的行号槽表头格上（组件 `row_header` 仍然关着，见 §34.2）。

### 35.4 差异明账（仍未做，不静默）

1. **交角点击后焦点不挪到数据区第一列**：DBeaver `:3183-3184` 把 `focusColumn` 摆到右侧第一列、
   `focusItem = topIndex`；本仓「当前格」语义（`selection_pos`）被三处既有用例锁定
   （`edit_row_button_is_disabled_in_record_view` 等，见 §34.4 #1），全选后保持原值，登记为差异。
2. **列头点击 = 整列选中未做**（`:3185-3204`）：本仓列头点击仍是「按此列排序」，且列头 `on_drag`
   被列重排 `DragColumn` 占用（§34.4 #2）。改成 DBeaver 口径要先定「排序入口搬到排序箭头、
   列头点击让位给整列选中」的整体方案，不在本刀硬塞。
3. 行号列宽 40 下限（§34.4 #5）、Ctrl 并集（§34.4 #4）、空态占位行本身的渲染（DBeaver 空结果
   不画数据行、本仓塞一行撑表头）三处仍开放。

### 35.5 验证

| 项 | 结果 |
| --- | --- |
| `cargo test -p rds-result-panel --lib` | **440 passed / 0 failed**（438 + 本刀 2） |
| `cargo test -p rds-editor` | **868 passed + 4 ignored**；离线 `tests/offline_baseline.rs` 5 passed |
| `cargo check -p rds-workbench --lib` | Finished，0 warning |

工作区改动仅 `grid/{delegate,header,tests}.rs` 三个文件（+44/−1、+36/−2、+85/0），无 vendor、
无并行线文件；文档本节随另一 commit 入库（LF blob）。
