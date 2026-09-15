# Mock 数据生成（M7）— 原型设计

> 本文件定义**长什么样**：落位与尺寸、两处排版、对话框、状态矩阵、主题映射、与 v1 的逐项对照。
> 交互稿：`mock-prototype.html`（v2 原生，RDS Light/Dark + 7 场景可切）。
> 语义与数据流：`mock-architecture.md`；任务与进度：`mock-dev-plan.md`。

## 1. 落位与尺寸

| 项 | 取值 | 来源 |
| --- | --- | --- |
| 落位（配置与出口） | 右侧 Dock 面板（`RightPanel::Mock`），与洞察 / 历史共用 Dock 槽位 | `crates/workbench/src/view.rs` |
| 面板宽度 | `ui::RIGHT_DOCK_WIDTH` = 17.5rem（280px）起步；**未发现拖拽调宽**（`DockLayout::tabs()` + `set_dock_size` 固定起步宽） | `crates/workbench/src/ui.rs` |
| 落位（字段与预览） | **中央编辑区 tab**「Mock · {目标表}」（`MockDetailView` 加入编辑区 tab 组） | `crates/workbench/src/view.rs` |
| 面板头 | Dock 的 `ComponentPanel::title/tab_name`（高度 `ui::PANEL_HEADER_HEIGHT` = 2.25rem） | 同上 |
| 视图归属 | **随 mock crate**（`crates/mock/src/mock_view.rs`，与 `project::ui` / `settings_view` 同例） | 架构 §3 / 决策 D11 |
| 视内尺寸 | 视图局部常量（`NUM_INPUT_WIDTH` 5rem / `PARAM_INPUT_WIDTH` 9rem / `FIELD_LIST_MAX_HEIGHT` 16rem / `PREVIEW_CELL_WIDTH` 9rem / `PREVIEW_MIN_HEIGHT` 10rem） | `crates/mock/src/mock_view.rs` |

**为什么拆两处**：状态语义是「造新表」——表名 + 列定义 + 选项 + 出口是**配置与提交**，字段表与预览表是**内容**。
280px 放不下字段卡片与宽预览表（v1 原型右面板是 `width:380px; min-width:200px; max-width:600px` 的可拖拽宽栏，
v2 右 Dock 没有拖拽调宽），故按「配置与出口在右、字段与预览在中」切分；两处状态同源：
中央 tab 持有配置面板实体，字段与预览都从它读，编辑动作写回它（**单一权威**）。

入口（全部走 `Shared::open_mock_panel`）：

1. 分析库页「生成 Mock」按钮 → 展开右 Dock（不带来源）；
2. 导航树对象右键「生成 Mock 数据」（表 / 视图）→ 展开右 Dock，并按**源库表**定向：读该表结构（列名 / 类型 / 可空 / 主键）
   导入为列定义、目标表名预填为该表名（v1 主路径：源库结构 → 造新数据）；
3. 右 Dock 面板切换（活动栏 / 状态栏开关 / Quick Open 的「打开 Mock 生成」）；
4. 面板内「查看详情（字段与预览）」→ 创建 / 聚焦中央「Mock 数据」tab。

## 2. 排版方案①：右 Dock = 配置 + 出口

```
┌ Mock 生成 ──────────────────────────────┐
│ Mock 数据生成                            │
│ 目标表名（新表；落库与文件名取此名）        │
│ [mock_orders                          ]  │ ← 表名输入（新建表，不是「从既有表里选」）
│ [1000] [随机种子]  [简体中文 ▾]           │ ← 行数 / 种子（留空随机）/ 语言
│ 列（5）            [导入结构] [＋ 加列]   │ ← 列来源：导入源库结构 / 手工加列
│ [                生成                ]   │ ← 主操作（全宽 primary）：只产临时表 + 预览（后台线程）
│ ▓▓▓▓▓▓░░░░░░░░░░░░░░░░░  40%          │ ← 生成中才显示：Progress 组件（不手搓）
│ 4 / 10 批（≈40000 / 100000 行）  [取消] │ ← 批次进度 + 取消（批次边界响应；点后转「正在取消…」）
│ 临时表 temp_mock_mock_orders · 1000 行 · 12 ms │ ← 生成结果摘要（muted）
│ 出口（生成后可用）                        │
│ [查看详情（字段与预览）]                  │ ← 切到中央 tab
│ [持久化为分析库表]                        │ ← 新建表；已存在则报错并引导「追加」
│ [追加到既有表 ▾]                          │ ← 显式选表（候选＝分析库既有表；主键自增接续）
│ [保存到草稿箱 ▾]                          │ ← CSV / Parquet / Xlsx / SQL INSERT → {项目}/mock/
│ [另存为 ▾]                                │ ← 同上四格式 → 系统保存对话框选路径
│ 已在分析库新建表 mock_orders（1000 行）    │ ← 成功（success）/ 失败（danger）/ 只读（info）
│ 数据只写入分析库与文件，不回传源库（M7）    │
└─────────────────────────────────────────┘
```

设计要点：

| 要点 | 做法 |
| --- | --- |
| 出口**显式**且分工不重叠 | 「持久化」= 新建表；「追加」= 写入既有表（显式选表，不猜同名）；两者都**不覆盖**既有数据 |
| 生成 ≠ 写入 | 「生成」只产内存临时表与预览；一切落库 / 落盘由出口按钮触发（v1 语义） |
| 出口按钮常显、生成前给禁用态 | 不给「hover 才可见」的图标；未生成时 `Button::disabled` 并附「（生成后可用）」提示 |
| 重交互用语义对话框 | 导入结构 / 列编辑走 `window.open_dialog`（焦点陷阱 / Escape / 遮罩关闭由组件负责） |
| 空态给出下一步 | 无列 → 「请先添加列：导入源库结构，或手工加列」；分析库无表 → 追加菜单只有「（分析库暂无表）」禁用项 |
| 渲染期零 I/O | 连接清单、既有表、导入结构、生成、出口全部在事件路径执行 |
| 自身可滚动 | Dock 内容区不产生滚动 → 面板内层 `overflow_y_scrollbar()` + `flex_1().min_h_0()` |
| 生成不阻塞 UI | 生成 / 追加提交给工作线程（`MockHost::start_job`）；进度由 120ms 定时泵拉取，取消在批次边界生效 |

## 3. 排版方案①：中央 tab = 字段 + 预览

```
┌ 工作台 │ Mock · mock_orders ✕ ────────────────────────────────────┐
│ 字段（5）· 目标表 mock_orders · 1000 行 · 种子 随机 · 简体中文  [生成] │ ← 摘要 + 快速生成（同一动作）
│ ┌───────────────────────────────────────────────────────────────┐ │
│ │ id            INTEGER    high                                 │ │ ← 列卡片：名称（flex）/ 类型 / 置信度
│ │ [自增序列 ▾] [编辑] [智能] [删除]   空值 0% · 起始值 1 · 步长 1 │ │ ← 生成器菜单（分类子菜单）+ 三个动作 + 摘要
│ ├───────────────────────────────────────────────────────────────┤ │
│ │ customer_email VARCHAR   high                                 │ │
│ │ [安全邮箱 ▾] [编辑] [智能] [删除]   空值 0% · zhangsan@exa…    │ │
│ └───────────────────────────────────────────────────────────────┘ │ ← 字段区最高 16rem，超出内部滚动
│ 预览（前 10 行）· 临时表 temp_mock_mock_orders · 本次 1000 行 · 12 ms │
│ ┌──┬──────┬──────────────────────┬──────────┬────────┬──────────┐ │
│ │# │ id   │ customer_email       │ amount   │ status │created_at│ │
│ │1 │ 1    │ zhangsan@example.com │ 1234.56  │ paid   │2024-01-… │ │ ← 固定列宽 + 横向滚动，纵向占满剩余高度
│ │2 │ 2    │ li.si@example.org    │ 88.00    │ pending│2023-11-… │ │
│ └──┴──────┴──────────────────────┴──────────┴────────┴──────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

| 要点 | 做法 |
| --- | --- |
| 字段卡片两行制 | 第一行「名称 · 类型 · 置信度徽标」，第二行「生成器菜单 + 编辑 / 智能 / 删除 + 参数摘要（或示例值）」 |
| 生成器切换不打断编辑 | 生成器用 `Button::dropdown_menu` 的**分类子菜单**（15 类 × 均值 9 项）；参数编辑在「编辑」对话框里，避免对话框内生成器与参数行不一致 |
| 置信度三态 | `high` success / `low` muted / `manual` info（与 v1 同语义） |
| 预览为只读网格 | `#` 行号列 + 固定列宽（9rem）+ `overflow_x_scrollbar()`；生成前显示「尚无结果——点右上『生成』」 |
| 状态行同源 | 成功 / 失败 / 落库目标都读配置面板的状态（同一 `MockPanel` 实体） |

## 4. 对话框

### 4.1 导入源库结构（`open_import_dialog`）

```
┌ 导入源库结构 ──────────────────────────┐
│ 从源库读取表结构（列名 / 类型 / 可空 /   │
│ 主键）并自动推断生成器；不读取数据本身。   │
│ 连接        [生产库 ▾]                  │ ← 菜单：Shared::connections（选中即预填库 / schema）
│ 数据库 / catalog [shop          ]       │
│ schema      [public             ]       │
│ 表名        [orders             ]       │
│              [取消] [导入]              │
└────────────────────────────────────────┘
```

- 读列走 **cache-aside**：先查连接级 L2 缓存（`database::NavCache`），未命中实时内省
  （`MetadataService::list_columns`，跑在进程级桥接运行时上），命中后尽力回写 L2；
- 导入结果 = 列定义 + 生成器 + 置信度 + 示例值；目标表名同步预填为该表名；
- 不读取数据本身（只读结构），导入后旧生成结果失效（避免出口拿旧结果落库）。

### 4.2 列编辑（`open_column_dialog`）

```
┌ 列编辑 ────────────────────────────────┐
│ 列名        [customer_email      ]      │
│ 类型        [VARCHAR ▾]                 │ ← 13 种（与 ColumnDataType 一一对应）
│ 生成器      安全邮箱（在字段行的菜单里切换）│ ← 只读：生成器身份只从字段行切换
│ 最小值      [2020-01-01          ]      │ ← 参数表单（按 spec 派生，标量可编辑）
│ 最大值      [2025-12-31          ]      │
│ 空值率（%）  [10                  ]      │
│ 唯一值      [开关（Switch）]             │
│ [恢复智能默认]                           │
│                    [取消] [应用]         │ ← DialogFooter（取消 secondary / 应用 primary）
└────────────────────────────────────────┘
```

- **工作副本语义**：「应用」才写回目标列，取消即丢弃（`ColumnDraft`）；
- 参数表单由 `generator_catalog::spec_of()` 派生（137 变体零手工对齐）；标量参数即时补丁回生成器配置
  （JSON 补丁，见 `patch_param`）；复杂参数（`ForeignKey.values` / `Sequence.values` / `Weighted.choices`）
  在对话框内只做说明，不内联编辑；
- 生成器身份变化（如「恢复智能默认」）后参数行**重建**（事件路径 `defer_in`，不在 render 里建实体）；
- 「恢复智能默认」= `ColumnMapper::infer(列名, 类型)` 重跑映射（生成器 / 空值率 / 唯一 / 置信度 / 示例值一起复位）。

### 4.3 尚未落地（Phase C/D，本稿不画）

场景模板选择、保存为模板、生成历史、复杂参数外置编辑——按 v1 原型入口在后续轮次接入；
入口按钮**在实现前不渲染**（不摆空控件）。

## 5. 生成器目录（137 变体）

`GeneratorConfig` 共 **137** 个变体（v1 文档写的「106」已过时），由 `tools/gen_mock_generator_catalog.py`
从 `models.rs` **穷尽派生**为 `crates/mock/src/generator_catalog.rs`（分类 / 中文标签 / 参数规格 / 默认构造）：

| 分类 | 数量 | 代表变体 |
| --- | --- | --- |
| 数值 | 10 | `auto_increment` / `random_int` / `random_decimal` / `normal` / `random_walk` / `boolean` |
| 文本 | 9 | `constant` / `words` / `sentence` / `paragraph` / `regex` / `template` |
| Markdown | 8 | `markdown_bold_word` / `markdown_link` / `markdown_bullet_points` |
| 个人信息 | 15 | `name` / `safe_email` / `username` / `password` / `cell_number` |
| 地址与网络标识 | 25 | `city` / `zip_code` / `latitude` / `geohash` / `ipv4` / `mac_address` |
| 日期时间 | 9 | `date_time` / `date_time_between` / `sequential_date_with_gaps` |
| 商业 | 16 | `company_name` / `job_title` / `industry` / `catch_phrase` |
| 金融 | 6 | `currency_code` / `bic` / `isin` / `credit_card_number` |
| 网络与技术 | 13 | `uuid_v4` / `url` / `user_agent` / `semver` / `file_path` |
| 图片 | 5 | `image_url` / `image_url_blur` / `image_url_custom` |
| 颜色 | 6 | `hex_color` / `rgb_color` / `hsla_color` |
| Ferroid ID | 5 | `ferroid_ulid` / `ferroid_twitter_id` / … |
| 标准编码 | 5 | `isbn13` / `rfc_status_code` |
| 汽车与行政 | 2 | `licence_plate` / `health_insurance_code` |
| 约束 | 3 | `foreign_key` / `sequence` / `weighted`（行为变体，参数为集合） |

**约束类**是行为变体（指定集合 / 循环序列 / 加权选择），参数为列表，面板只读展示——它们的编辑点应是模板与导入场景。

### 5.1 智能映射的置信度呈现

`ColumnMapper` 四级优先：**精确名 → 前后缀 → 模糊子串 → 类型兜底**，产出：

| 命中层 | `confidence` | 面板呈现 |
| --- | --- | --- |
| 名称精确 / 前后缀 / 模糊 | `high` | 绿色徽标（`theme.colors.success`） |
| 类型兜底 | `low` | muted 徽标（提示「按类型猜测」） |
| 用户手工选择 / 改过参数 | `manual` | info 徽标（`theme.colors.info`） |

参数摘要优先显示生成器参数（`起始值 1 · 步长 1`），无参数时回退到映射的示例值（`zhangsan@example.com`）。

## 6. 状态与空态矩阵

| 状态 | 触发 | 呈现 |
| --- | --- | --- |
| 无列 | 首次打开 / 还没导入或加列 | 字段区一行 muted 引导语；点「生成」得到可读错误「请先添加列…」 |
| 已导入结构 | 导航右键定向 / 导入结构对话框 | 目标表名预填 + `已从 {库}.{schema}.{表} 导入 N 列`（success 行）+ 字段区展开 |
| 分析库无表 | 分析库为空 / 读取失败 | 追加菜单只有「（分析库暂无表）」禁用项 |
| 生成成功 | 点「生成」且宿主生成成功 | `已生成 N 行（耗时 T ms）→ 临时表 temp_mock_*`；出口按钮转为可用；中央 tab 出预览 |
| 生成失败 | 行数 / 种子非法、无列、引擎报错 | danger 行，保留目标与列配置（可改再试）；旧结果作废 |
| 落库成功 | 「持久化为分析库表」 | success 行 `已在分析库新建表 X（N 行）` + 「落库目标：X」；导航树缓存失效并刷新 |
| 落库同名 | 「持久化为分析库表」而表已存在 | danger 行 `分析库已存在表 X：请改用「追加到既有表」`，并刷新追加候选 |
| 追加成功 | 「追加到既有表」选表 | success 行 `已追加到 X（表内共 N 行）`；生成阶段已按表内行数接续自增起点 |
| 追加缺列 | 目标表缺草稿里的列 | danger 行列出缺失列名（不让 DuckDB 原始错误冒到界面） |
| 只读项目 | `Shared.project_ui.read_only` | info 行「只读模式：不允许落库与写文件（仍可生成预览）」；四个写出口**在视图层拦截**并给可读错误（不静默失败） |
| 未打开项目 | 「保存到草稿箱」 | danger 行「未打开项目：草稿箱不可用（请先打开或新建项目）」 |
| 生成中 | 点「生成」/「追加」（后台任务） | 生成按钮转「生成中…」并禁用；下方 `Progress` 进度条 + 「k / N 批（≈已生成 / 目标 行）」+ 「取消」；详情 tab 摘要行尾追加百分比；首批回调前显示「准备中…」 |
| 取消中 | 已点「取消」，引擎尚未到批次边界 | 取消按钮转「正在取消…」并禁用（不重复下发）；到边界后转为「已取消」文案 |
| 生成异常 | 后台线程退出（既无进度也无结果） | danger 行「后台生成任务异常结束（工作线程已退出）」；面板归位空闲可重试 |

## 7. 主题映射与尺寸常量

| 元素 | 取色（`cx.theme().colors.*`） | 尺寸 |
| --- | --- | --- |
| 面板底 | `background` | `size_full` |
| 面板标题 / tab 标题 | `foreground` | `text_sm` + MEDIUM |
| 行数 / 种子 / 表名输入 | 组件默认 | 宽 `NUM_INPUT_WIDTH` = 5rem（数字）/ 自适应（表名） |
| 对话框内输入 | 组件默认 | 宽 `PARAM_INPUT_WIDTH` = 9rem |
| 字段区滚动 | —— | `max_h(FIELD_LIST_MAX_HEIGHT)` = 16rem |
| 字段卡片 | `border` + `radius` | `p_2` + `border_1` |
| 预览单元格 | —— | 宽 `PREVIEW_CELL_WIDTH` = 9rem；容器最小高 `PREVIEW_MIN_HEIGHT` = 10rem |
| 成功 / 失败 / 只读 | `success` / `danger` / `info` | `text_xs` |
| 参数摘要 / 引导语 / 类型 | `muted_foreground` | `text_xs` |
| 置信度徽标 | `success` / `info` / `muted_foreground` | `text_xs` |

**零裸色值 / 零裸结构尺寸**：全部来自主题 token 与视图局部 rem 常量（视图不在 workbench 的 `ui.rs` 契约扫描范围内，
但沿用同一口径：结构性尺寸用 `rems()` + 具名常量，局部间距用 Tailwind 尺度方法）。

## 8. GPUI 落点与组件选型

| 需求 | 选用 | 说明 |
| --- | --- | --- |
| 配置面板 | `Entity<MockPanel>`（`crates/mock/src/mock_view.rs`） | 跨 frame 状态在实体上；宿主只持弱句柄 |
| 详情 tab | `Entity<MockDetailView>` + `BasePanel` / `ComponentPanel` | 由宿主 `DockArea::add_panel(.., DockPlacement::Center, ..)` 加入编辑区 tab 组；`on_added_to` 记 tab 组句柄，重复点「查看详情」用 `TabGroup::select_tab` 聚焦自身 |
| 表名 / 行数 / 种子 / 参数 / 空值率 | `Input` + `InputState` | `InputState::new` 需要 window：面板在 render 首次创建，对话框在打开时创建 |
| 语言 / 生成器 / 追加目标 / 草稿箱 / 另存为 | `Button` + `dropdown_menu`（`PopupMenuItem::checked/disabled`、`PopupMenu::submenu`） | 生成器 137 项按 **15 类子菜单**承载；追加目标列既有分析表 |
| 字段行操作 | `Button`（`ghost` / `xsmall`） | ElementId 用列 id（`mock-edit-{id}` / `mock-gen-{id}`），不用下标 |
| 唯一值开关 | `Switch`（`gpui_kit::component::switch`） | 受控：回调收到请求值，由视图写回并 `notify()` |
| 生成 / 应用 / 取消 / 导入 / **任务取消** | `Button`（`primary` / `secondary`；运行中禁用） | 不手搓 `div + on_click`；运行中不给 `on_click`（禁用态） |
| **任务进度** | `Progress`（`gpui_kit::component::progress`，`.value(0..100)`） | 不手搓进度条；文案与取消按钮同排一行 |
| **进度轮询** | `cx.spawn` + `background_executor().timer(120ms)` + 弱句柄 | 任务进行中没有其他事件触发重绘，必须主动唤醒（与 `scratchpad_jobs` 的泵同例） |
| 导入结构 / 列编辑 | `window.open_dialog` + `Dialog` + `DialogFooter` | 焦点陷阱 / Escape / 遮罩关闭由组件负责；窗口根须为 `Root` |
| 另存为（选路径） | `App::prompt_for_new_path` + `Window::spawn` | 异步回传：取消则不动；落盘动作在回传里执行（事件路径） |
| 滚动区 | `overflow_y_scrollbar()` / `overflow_x_scrollbar()` + `max_h` / `flex_1().min_h_0()` | Dock 内容区自身不产生滚动，面板与 tab 各自给滚动主体 |
| 宿主能力 | `MockHost`（生成 / 出口 / 来源 / 只读 / 打开详情 / 重绘） | 与 `project::ui::ProjectUiHost` 同范式；workbench 侧桥接见 `components/mock_host.rs` |

## 9. 与 v1 的逐项对照

| v1 能力 | 处置 | 说明 |
| --- | --- | --- |
| 配置区（表名 / 行数 / 种子 / 语言 / 重置） | **已迁** | 表名输入保留（造新表）；行数 / 种子 / 语言保留；「重置」并入字段行「智能」与列编辑「恢复智能默认」 |
| 字段表（可增删列 / 改名改类型 / 生成器 / 空值率 / 唯一 / ⚙） | **已迁（改落位）** | 右 Dock 280px 放不下 → 中央 tab 的字段卡片 + 列编辑对话框（工作副本 + 应用/取消） |
| 生成器下拉（137 项平铺） | **已迁（改形态）** | 分类子菜单（15 类）；搜索待办 |
| 智能映射 + 置信度 | 照搬 | `ColumnMapper` 原样使用，换呈现（徽标三态） |
| 行数 / 种子 / 语言三参数 | 照搬 | `MockConfig` 已是引擎输入 |
| 生成 → 预览（前 10 行） | **已迁** | 预览表落中央 tab（`#` 行号 + 固定列宽 + 横向滚动），行数上限 `PREVIEW_ROWS` |
| 导出 CSV / XLSX / Parquet / SQL | **已迁** | 右 Dock「另存为 ▾」（系统保存对话框选路径）；「保存到草稿箱 ▾」写 `{项目}/mock/mock_*.{ext}` |
| 持久化为分析库表 | **已迁 + 显式化** | 新建表；同名已存在 → 报错并引导「追加」（v1 是隐式 `CREATE TABLE AS SELECT`） |
| 追加到既有表 | **v2 新增** | v1 无此路径；v2 显式选表 + 主键自增接续表内行数 |
| 从数据库导入结构 | **已迁** | 导入结构对话框（连接 / 库 / schema / 表）+ 导航右键定向；`NavCache` / `MetadataService` cache-aside |
| 场景模板选择与一键生成 | 待办（Phase C） | `list_templates` / `apply_template` / `generate_scenario` 已就绪 |
| 保存为模板 / 模板复用 | 待办（Phase C） | 后端 `MockGenerationStore` 已就绪 |
| 生成历史段 / 弹窗 | 待办（Phase D） | 后端 8 方法 + 迁移 009 已就绪 |
| 列依赖编辑器 | 待定 | 引擎只做拓扑排序、不解释表达式（架构 §9-I6） |
| v1 进度条 / 取消（`mock:generate-progress`） | **已迁（改形态）** | Tauri 事件通道退役 → 工作线程 + 进度槽 + 120ms 定时泵；取消走引擎的进程级标志（批次边界响应） |
| 数据分布图 | 不迁 | 属可视化专项，与结果集图表合并考虑 |
| Vue 组件 / Pinia store / `mock-api.ts` | 不迁 | Tauri IPC 与 Vue 层退役（`docs/migration/commands-retirement.md`） |

## 10. 待拍板项

| # | 议题 | 现状与建议 |
| --- | --- | --- |
| 1 | ~~视图归属~~ | ✅ 已定：**随 mock crate**（与 project / settings 同例），宿主经 `MockHost` 注入 |
| 2 | ~~预览归属~~ | ✅ 已定：预览表格在**中央「Mock 数据」tab**（方案①），面板只留结果摘要一行 |
| 3 | ~~目标表语义~~ | ✅ 已定：**造新表**（表名输入 + 出口显式落库），不再「从分析库既有表里挑着灌数」 |
| 4 | 目标库作用域 | 当前写**全局分析库**（`services::mock_generator::analytics_db_path()`，与 SQL 执行入口一致）；项目分析库 `{项目}/.RSmeta/analytics.duckdb` 待接入（装配层已留 `*_at(path, ..)` 显式路径入口） |
| 5 | 出口（落库 / 导出）的后台化 | 生成 / 追加已走后台（进度 + 取消）；**出口仍同步**，大行数落库仍会卡界面（架构 §9-I10） |
| 6 | 依赖表达式 | `Expression` / `Template` / `Weighted` 的取值计算：要么实现解释器，要么把 `dependency` 降级为「生成顺序提示」（当前实现是后者） |
| 7 | 生成器搜索 | 分类子菜单已可用；137 项下「按名称 / 标签搜索」仍值得补（需要一个带输入框的弹层） |
