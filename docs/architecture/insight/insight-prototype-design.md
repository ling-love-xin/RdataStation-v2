# 洞察模块（M8）· 原型设计

> 状态：**设计定稿（原型 v1，2026-09-15）**，代码尚未开始 · 关联文件：`insight-prototype.html`（可交互原型）、`insight-dev-plan.md`（开发方案）
> 技术栈：gpui-kit 0.6（五段布局见 `layout/layout-design.md`；右 Dock 起步宽 17.5rem = 280px）
> 前置：v1 前端蓝本 `v1/frontend/extensions/builtin/workbench/ui/components/panels/`（`InsightStatsSection` / `QualityScoreCard` / `TableProfileView` / `SchemaInsightPanel` / `InsightHistoryTab` / `ColumnInsightsPanel` / `MultiColumnView`）+ `insight-store.ts`
> 关联文档：`overview.md`（M8 定位）、`ui/ui-design-spec.md`（尺寸与色值规范）、`../connection/connection-prototype-design.md`（作用域三态参照）
>
> **范围界定**：本模块只负责**画像 / 评分 / 规则 / 报告**。SQL 执行与结果集属 M5 编辑器；对象树与内省属 M4；Mock 属 M7；资源目录属 M6。洞察**不自己取数**——数据来自 M5 建立的 DuckDB 临时表或 M3 的连接。

## 1. 设计基准与语义

### 1.1 核心语义：一次分析 = 一个目标 + 一份结论

洞察回答两个问题：**这份数据长什么样**（画像）与**它能不能用**（评分）。由此确定四条硬约束：

- **目标是显式的**：不存在「全局洞察」——每次分析都必须绑定一个目标（列 / 表 / Schema / 多列集合）。面板头部**永远显示当前目标**，避免「这数是哪来的」。
- **结论必须可追溯到算法**：每个数字要么来自统计量，要么来自评分维度，要么来自规则。UI **不发明结论**（v1 `filterByValue` 把「值」当「列名」传即为反例，v2 删除该行为）。
- **采样必须明示**：表探查与批量评估基于采样（后端 `LIMIT 500`），UI 必须写明「基于 N 行采样」，不做静默全表扫描。
- **分析粒度随目标分派**：同一面板按目标类型渲染不同内容（列 / 表 / 多列 / Schema），**不做四套面板**。

### 1.2 规则的归属：三层作用域（本轮新增设计）

规则是本模块**唯一对用户开放的扩展面**，其作用域必须可见、可管理：

| 层 | 位置 | 作用域 | 可写 | 优先级 | UI 呈现 |
| --- | --- | --- | --- | --- | --- |
| 内置 `Builtin` | 应用内嵌（18 条） | 所有项目 | ❌ | 最低 | 灰显「内置」徽标，可**禁用**不可删除 |
| 用户全局 `Global` | `{系统目录}/insight-rules/` | 所有项目 | ✅ | 中 | 「全局」徽标 |
| 项目 `Project` | `{项目}/.RSmeta/insight-rules/` | 当前项目 | ✅ | 最高 | 「项目」徽标；进 git、可 diff |

- 同名 `meta.id` 后者**整体覆盖**前者（沿用 v1 语义）。
- **可禁用**：包括内置规则——禁用不改文件，写一条抑制记录（机制见 `insight-dev-plan.md` §4.2）。
- 解析失败**不连坐**：该条红色标注错误原文，其余规则照常可用。

### 1.3 面板归属：右 Dock，而非底部

v1 把洞察拆成「右栏轻量统计 + 底部四 Tab 容器」两处（`ColumnInsightsPanel` + `BottomInsightPanel`），原因是 v1 用 dockview 动态面板、缺一个稳定的右 Dock。v2 已有右 Dock（`RightPanel::Insight`，17.5rem），因此**收敛为单面板**，与 `RightPanel::Mock` / `RightPanel::History` 并列。

`workbench` 侧只保留 `RightSidebarPanel` 的**装配与 Dock 协议**；面板内容的**归属尚未拍板**（详见 `insight-dev-plan.md` §3.1）：方案 A 入 `crates/insight/src/insight_view.rs`（对齐 `overview.md` §「Feature 可直接依赖 gpui-kit」与 `project` 先例），方案 B 留 `workbench/panels.rs`（对齐 `scratchpad` 新确立的口径）。**本文按方案 A 书写**，两种方案下布局与交互不变。

## 2. 面板布局（17.5rem = 280px 右 Dock）

```
┌──────────────────────────────────────────────────┐
│ 洞察                                       ⚙  ⟳ │  36px 面板头（PANEL_HEADER_HEIGHT）
├──────────────────────────────────────────────────┤
│ 列 │ 表 │ 多列 │ 结构 │ 历史                      │  28px Tab 条
├──────────────────────────────────────────────────┤
│                                                  │
│  ┌ 目标头 ─────────────────────────────────────┐ │
│  │ amount  [DOUBLE]                    5.0% 空 │ │  行高 ROW_HEIGHT
│  └─────────────────────────────────────────────┘ │
│                                                  │
│  ▼ 基础统计                                      │  折叠区（TREE_INDENT 缩进）
│    总行数        10,000                          │
│    非空值        9,500                           │
│    空值          500  (5.0%)                     │
│    唯一值        8,214                           │
│    ─────────────                                 │
│    平均值        127.35                          │
│    中位数        98.00                           │
│    最小值        0.00                            │
│    最大值        9,999.00                        │
│    P25 / P75     42.00 / 188.00                  │
│    标准差        231.7                           │
│    偏度          3.42  右偏                      │
│                                                  │
│  ▼ 数据分布                                      │
│    0–999     ████████████████████  58.2%         │
│    1k–1.9k   ██████████            31.0%         │
│    2k–2.9k   ███                    8.4%         │
│    3k+       █                      2.4%         │
│                                                  │
│  ▼ 数据质量                                      │
│    ⚠ 高空值率：5.0% 接近阈值                     │
│    ⚠ 检测到 1 个极端高值                         │
│    ℹ 分布右偏（偏度 3.42），中位数比平均值更具代表性│
│                                                  │
│  ▼ 样本数据                                      │
│    1   127.35                                    │
│    2   NULL                                      │
│    3   88.00                                     │
│    …                                             │
├──────────────────────────────────────────────────┤
│ 质量评分  72 分  良好                            │  评分卡钉在底部（可选）
│ 完整性 ████████████████░░░░  82                  │
│ 唯一性 ██████████████████░░  88                  │
│ 类型一致 ████████████████████ 100                │
│ 分布均匀 ████████░░░░░░░░░░░░  41                │
└──────────────────────────────────────────────────┘
```

**Tab 命名与宽度核算**：5 项 × 2 字（「多列」2 字、「结构」2 字）在 280px − 左右内距 16px = 264px 内，每项约 52px，12px 字号下最宽标签「多列」约 24px + 内距，**不溢出**。不用 4 字标签（v1 的「多列分析 / 历史版本 / Schema 洞察 / 表探查」在 280px 下会挤压）。

**折叠区默认展开**：`基础统计` + `数据分布`（v1 `InsightStatsSection` 同口径，默认展开 basic / dist / quality / sample 四项；280px 下改为前两项默认展开，其余折叠，避免首屏过长）。

### 2.1 面板头动作

| 动作 | 图标 | 行为 |
| --- | --- | --- |
| 规则管理 | ⚙ | 打开**规则管理对话框**（见 §5；不进 Tab——三层分组 + 错误原文需要宽度，280px 放不下） |
| 刷新 | ⟳ | 重算当前目标画像（保持 Tab 与折叠态） |

## 3. 四种目标的视图分派

### 3.1 列画像（Tab「列」）

见 §2 布局图。四区内容按 `ColumnStatsDetail` 变体分派：

| 类型变体 | 基础统计独有 | 数据分布 | 数据质量文案 |
| --- | --- | --- | --- |
| `Numeric` | 平均 / 中位 / 最小 / 最大 / P25 / P75 / 标准差 / 偏度（+ 右偏·左偏·近似对称） | 直方图条（label + 比例条 + %），`HISTOGRAM_MIN_ROWS = 10` 以下不生成 | 极值告警（`is_extreme`）、\|偏度\| > 1 分布提示 |
| `Text` | 长度范围 `min ~ max` | Top 值频次列表 | Top 值 ≥ 5 类 → 类别数提示 |
| `DateTime` | 最早 / 最晚 / 跨度天数 | 月度分布列表 | — |
| `Boolean` | True 数（含占比）/ False 数 | True 占比条 | `true_ratio > 0.95` → 高度不平衡提示 |
| `Unknown`（全 NULL / BLOB / ARRAY） | 仅总行数 + 空值 | 无（不渲染分布区） | 「类型未识别」提示 |

> 空值率 > 5% 时空值数字转 `warning` 语义色，> 10% 追加质量提示。

### 3.2 表探查（Tab「表」）

```
┌──────────────────────────────────────────────────┐
│ ▤ orders        [MySQL]        12.4 万行         │
│                              [ 评估全表 ]         │
├──────────────────────────────────────────────────┤
│ 表质量  68 分  一般                              │
│ 12 列中已评分 12 列                              │
├──────────────────────────────────────────────────┤
│ #  列名               类型        可空   质量    │
│ 1  id            PK   BIGINT      NO     95 优   │
│ 2  user_id            BIGINT      NO     88 良   │
│ 3  amount             DECIMAL     YES    72 良   │
│ 4  remark             TEXT        YES    41 差   │
│ …                                                │
├──────────────────────────────────────────────────┤
│ 基于 500 行采样                                  │
└──────────────────────────────────────────────────┘
```

- 列名可点 → 下钻到列画像（Tab 切「列」）。
- 质量列未评估显示 `—`；「评估全表」走串行 + 进度（避免撞后端并发上限 4）。
- 底部**必须**显示采样口径。

### 3.3 多列分析（Tab「多列」）

**重新设计**（v1 该功能从未跑通：列清单 `availableColumns` 从未赋值、规则清单恒空、执行按钮恒禁用）：

```
┌──────────────────────────────────────────────────┐
│ 列（来自当前结果集 / 临时表）        已选 3/8    │
│ ☑ amount    ☑ qty    ☑ price                    │
│ ☐ user_id   ☐ status ☐ created_at  …            │
├──────────────────────────────────────────────────┤
│ 规则  [ 相关性分析 (multi)          ▾ ]          │
│                              [ 执行分析 ]         │
├──────────────────────────────────────────────────┤
│ 相关性分析                          列表          │
│ ┌──────────────────────────────────────────────┐ │
│ │ 列 A    列 B     相关系数                     │ │
│ │ amount  qty      0.812                        │ │
│ │ amount  price    -0.134                       │ │
│ └──────────────────────────────────────────────┘ │
│                                    [ 清除结果 ]   │
└──────────────────────────────────────────────────┘
```

- 列清单来源改为**真实列元数据**（当前结果集或 DuckDB 临时表），而非 v1 的空 ref。
- 结果渲染按 `RuleQuery.result_type` 分派：`single` → KV 行；`list` → 表格。
- 字段名中文化映射沿用 v1（corr / covar / regression_slope / sample_size / …）。

### 3.4 结构（Schema 报告，Tab「结构」）

```
┌──────────────────────────────────────────────────┐
│ public                       6 表 · 48 列        │
│ ⤓JSON  ⤓Markdown  ⟳                              │
├──────────────────────────────────────────────────┤
│                 78                                │
│               健康分   良好                       │
│ 外键推断 4 · 类型不一致 2 · 孤立表 1 · 冗余列 3    │
├──────────────────────────────────────────────────┤
│ ▼ 外键候选 (4)                                    │
│   orders.user_id → users.id        高   *_id     │
│   items.order_id → orders.id       中   *_id     │
│ ▼ 类型不一致 (2)                                  │
│   user_id   [critical]                            │
│     orders  BIGINT  ·  users  INT                 │
│ ▼ 孤立表 (1)                                      │
│   tmp_import      3 列   无外键引用               │
│ ▼ 冗余列 (3)                                      │
│   created_at   出现 6 表   建议抽公共字段          │
└──────────────────────────────────────────────────┘
```

- 健康分按 85 / 70 / 50 / 30 四档取色（与质量评分同一色阶）。
- 类型不一致的受影响表可点击 → 下钻表探查。
- 三态：骨架屏 / 错误 + 重试 / 报告。

### 3.5 历史与版本对比（Tab「历史」）

```
┌──────────────────────────────────────────────────┐
│ 快照历史                            ⟳   [ 保存 ] │
├──────────────────────────────────────────────────┤
│ 2026-09-15 14:22   DOUBLE   当前                   │
│ 2026-09-14 09:10   DOUBLE                          │
│ 2026-09-12 18:47   DOUBLE                          │
├──────────────────────────────────────────────────┤
│ 与 2026-09-12 对比                          [ ✕ ] │
│ 总行数     10,000 → 12,400   +2,400  ▲            │
│ 空值率      8.2% → 5.0%      −3.2%   ▼            │
│ 唯一值      7,800 → 8,214    +414   ▲            │
│ 类型        DOUBLE → DOUBLE  不变     ●           │
└──────────────────────────────────────────────────┘
```

- **保存入口必须有**（v1 缺：`saveCurrentInsight` 无调用方 → `save_column_insight_snapshot` 前端不可达）。
- 对比颜色**三态**：增 ▲ / 减 ▼ / 不变 ●（v1 定义了 `.val-same` 却只用一色）。
- 存储用量取**后端真实统计**（`get_insight_storage_stats`），不用 v1 的 `history.length * 2` KB 估算。

## 4. 状态与空态矩阵

| 态 | 触发 | 呈现 |
| --- | --- | --- |
| 无项目 | 未打开项目 | 面板可打开但显示引导「打开项目后可保存快照」；画像可算（临时表在内存），历史与规则管理禁用 |
| 空态（无目标） | 未选中列 / 表 | 居中图标 + 「右键结果表或导航树中的列，查看洞察」 |
| 加载中 | 计算中 | 骨架屏（基础统计 5 行 + 分布 4 条），**不阻塞面板切换 Tab** |
| 并发受限 | 并发超上限 | 行内提示**原样展示引擎的那一份文案**（`insight_engine::ERR_TOO_MANY_CONCURRENT` = 「洞察分析任务过多，请稍候重试」）；**不出现** v1 的英文 `Too many concurrent insight operations, please retry` |
| 错误 | 临时表失效 / 连接断开 / 超时清理 | 文案 + 「重试」**由 `InsightService::describe_error` 给出**：结果集失效或过期 → 「结果集已失效或已过期，请重新执行查询」（**不给重试**，要重新执行查询）；连接抖动 → 「连接不可用，请检查数据源后重试」（给重试）；其余原文照给。不展示 `CoreError` 的 `[code]` 内部错误码 |
| 类型未识别 | 全 NULL / BLOB / ARRAY | 仅基础计数 + 「类型未识别」提示，**不渲染**分布与类型专属统计 |
| 采样提示 | 表探查 / 批量评估 | 底部常驻「基于 N 行采样」 |
| 规则校验失败 | 某条 TOML 非法 | 规则对话框内红色行 + 错误原文；洞察功能**不受影响** |

**关键帧**：

```
结果表列头右键「洞察此列」
  └─► 目标 = 该列 ──► 面板切「列」Tab ──► 骨架屏 ──► 列画像（+ 质量评分卡）
导航树表右键「查看统计」
  └─► 目标 = 该表 ──► 面板切「表」Tab ──► 走采样查询 ──► 表探查
表探查列名点击 ──► 目标 = 该列 ──► 切「列」Tab
表探查「评估全表」──► 串行评分（进度）──► 表质量摘要 + 各列质量分
结构 Tab 类型不一致点击表名 ──► 切「表」Tab
「保存」──► 写快照（DuckDB 正文 + SQLite 元数据，含 checksum / 版本链）──► 历史列表首行
⚙ ──► 规则管理对话框 ──► 启用/禁用 ──► 写索引 ──► 面板规则列表即时刷新
```

## 5. 规则管理对话框

**为什么是对话框而非面板 Tab**：三层分组 + 每行状态 + 错误原文需要宽度；且「配置分析器」与「看分析结果」是两类活动（与 `项目设置` / `连接对话框` 同口径）。

```
┌────────────────────────────────────────────────────────────┐
│ 洞察规则                                                  ✕ │
├────────────────────────────────────────────────────────────┤
│ 搜索规则…            [ ＋ 新建项目规则 ]  [ ⟳ 重新加载 ]   │
├────────────────────────────────────────────────────────────┤
│ ▼ 项目规则 · {项目}/.RSmeta/insight-rules/          (2)     │
│   ● 我的订单空值检查      column    v1.0   [开]  ⧉         │
│   ● 渠道分布对比          multi     v1.2   [开]  ⧉         │
│                                                            │
│ ▼ 全局规则 · {系统目录}/insight-rules/               (0)     │
│   （空）跨项目复用的规则放在这里，所有项目可见              │
│                                                            │
│ ▼ 内置规则 · 应用内嵌                              (18)     │
│   ● 空值检查              column    v1.0   [开]  ⧉         │
│   ● 数值统计              column    v1.0   [开]  ⧉         │
│   ● 相关性分析            multi     v1.0   [关]  ⧉         │
│   ● 表行数                table     v1.0   [开]  ⧉         │
│   …                                                        │
│   ⚠ 用户的规则              project  v1.0   校验失败        │
│      第 3 行未知字段 `outputs`（期望 output）              │
├────────────────────────────────────────────────────────────┤
│ 共 20 条 · 2 条禁用 · 1 条校验失败                          │
└────────────────────────────────────────────────────────────┘
```

| 元素 | 语义 |
| --- | --- |
| 作用域分组 | 项目 → 全局 → 内置（覆盖优先级从高到低，与加载顺序相反，便于用户理解「谁赢」） |
| `[开] / [关]` | 开关。**内置规则也可关**（写抑制记录，不删文件） |
| `⧉` | 在系统编辑器中打开规则文件（内置规则无此项） |
| 校验失败行 | 红色 + 错误原文（TOML 解析错误的**唯一可见出口**；v1 只写 `tracing::warn!`） |
| 底部统计 | 总数 / 禁用数 / 失败数 |
| `＋ 新建项目规则` | 在当前项目的 `.RSmeta/insight-rules/` 建目录 + 空模板文件并打开 |
| `⟳ 重新加载` | 目录监听之外的**兜底与排障**入口（不再作为唯一热加载方式） |

规则的 TOML 字段语义（`meta` / `query` / `output` / `quality` / `render`）与 18 条内置规则示范，属使用手册内容，见 `insight-user-guide.md`（按约定实现后补）。

## 6. 主题映射（token → 视觉，零裸 hex）

### 6.1 面板与容器

| 元素 | 主题字段 | 说明 |
| --- | --- | --- |
| 面板底 | `colors.background` | 右 Dock 内容区 |
| 面板头 / Tab 条底 | `colors.tab_bar` | 与左 Dock 一致 |
| 目标头底 | `colors.list_active` | 当前目标的强调底 |
| 折叠区头 | `colors.foreground`（次级权重） | 不使用独立底色 |
| 折叠区内容的键 | `colors.muted_foreground` | 统计项标签 |
| 折叠区内容的值 | `colors.foreground` | 统计值，等宽字体 |
| 分隔线 | `colors.border` + `ui::HAIRLINE` | 1px |
| 滚动区 | gpui `ScrollableElement` | 面板内容整体可滚 |

### 6.2 语义色（数据状态）

| 元素 | 主题字段 |
| --- | --- |
| 空值率超阈值（> 5%） | `colors.warning` |
| 质量告警（极值 / 高空值率） | `colors.warning` |
| 质量提示（偏度 / 类别数 / 不平衡） | `colors.info` |
| 校验失败 / 危险操作 | `colors.danger` |
| 质量等级 优（≥ 85） | `colors.success` |
| 质量等级 良（≥ 70） | `colors.primary` |
| 质量等级 一般（≥ 50） | `colors.warning` |
| 质量等级 差（< 50） | `colors.danger` |
| 对比 增 / 减 / 不变 | `colors.success` / `colors.danger` / `colors.muted_foreground` |
| 主按钮（执行分析 / 保存） | `colors.primary` + `colors.primary_foreground` |
| 分布条底槽 | `colors.border` |
| 分布条填充 | `colors.primary` |
| 维度进度条填充 | 按分数取四档（同上表） |
| 表头行底 | `colors.list_hover` |
| 表行悬停 | `colors.list_hover` |
| 表行选中 / 下钻热点 | `colors.list_active` + `colors.list_active_border`（2px 侧条 = `ui::TREE_ACTIVE_BAR`） |

> 与 v1 的差异：v1 Phase 21 已把颜色 token 化，但**每处都带 CSS fallback**（约 45 处）。v2 无 fallback 概念，缺失角色应**补产品语义 token**（`assets/themes/product-tokens.json`），不在代码里回退裸色值。

### 6.3 类型徽标（形状 + 颜色双通道）

| 类型 | 呈现 |
| --- | --- |
| Numeric | `colors.info` 文字徽标（`DOUBLE` / `BIGINT` / `DECIMAL`） |
| Text | `colors.success` |
| DateTime | `colors.warning` |
| Boolean | `colors.primary` |
| Unknown | `colors.muted_foreground` |

沿用导航树（M4）「颜色 + 形状双通道」原则：颜色区分类型族，**文字始终显示真实类型名**，不靠颜色单独承载信息。

## 7. 尺寸常量（待登记进 `crates/workbench/src/ui.rs`）

按 M3 连接对话框 / M5 草稿箱的先例，新增一节 `===== 洞察（M8）专用尺寸 =====`，视图只引用不写字面量：

| 常量 | 值（rem） | 说明 |
| --- | --- | --- |
| `INSIGHT_TAB_HEIGHT` | 1.75（≈ 28px） | Tab 条高（比面板头矮一档） |
| `INSIGHT_HISTOGRAM_BAR_HEIGHT` | 0.5（8px） | 直方图条高 |
| `INSIGHT_DIM_BAR_HEIGHT` | 0.375（6px） | 四维进度条 / True 占比条高 |
| `INSIGHT_SCORE_FONT` | 1.75（≈ 28px） | 质量总分字号（主题字号的倍数，随缩放） |
| `INSIGHT_HEALTH_SCORE_FONT` | 2.25（36px） | Schema 健康分字号 |
| `INSIGHT_HISTORY_LIST_MAX_HEIGHT` | 15.0（240px） | 历史列表最大高（超出内部滚动，v1 同值） |
| `INSIGHT_SAMPLE_ROW_MAX_CHARS` | 200（计数） | 样本单元格截断长度（非尺寸，登记以便统一） |
| `INSIGHT_TABLE_PREVIEW_ROWS` | 5（计数） | 面板内表预览行数 |

复用既有常量：`PANEL_HEADER_HEIGHT`（面板头）、`ROW_HEIGHT`（统计行 / 列表行）、`TREE_INDENT`（折叠区缩进）、`CONTROL_HEIGHT_SM`（搜索框 / 工具栏控件）、`ICON_SIZE_SM`（折叠箭头 / 状态图标）、`HAIRLINE`、`TREE_ACTIVE_BAR`、`GAP_SM/MD/LG`、`PANEL_PADDING`、`SCRATCHPAD_EMPTY_ICON_SIZE`（空态大图标，2.25rem）。

规则对话框复用 `DIALOG_*` 系列：`DIALOG_ROW_HEIGHT`（规则行）、`DIALOG_TAB_BODY_HEIGHT`（列表区固定高，超出内部滚动）、`DIALOG_FORM_LABEL_WIDTH`。

## 8. GPUI 落点映射

| 设计决策 | 代码落点 |
| --- | --- |
| 右 Dock 装配（仅协议：`BasePanel` + `Panel`） | `crates/workbench/src/panels.rs`（`RightSidebarPanel`） |
| 面板内容（五 Tab + 目标分派） | `crates/insight/src/insight_view.rs` |
| 规则管理对话框 | `crates/insight/src/rule_view.rs` |
| Schema 报告与导出 | `crates/insight/src/schema_view.rs` |
| 视图模型（`InsightPanelState` / `SelectedTarget` / `PanelTab`） | `crates/insight/src/model.rs` |
| 服务门面（画像 / 评分 / 规则 / 快照） | `crates/insight/src/service/mod.rs` |
| 规则作用域与索引 | `crates/insight/src/{rule.rs, rule_registry.rs, service/indexer.rs}` |
| Action 与快捷键 | `crates/insight/src/commands.rs` + `crates/app/src/main.rs` |
| 尺寸常量 | `crates/workbench/src/ui.rs` |
| 协议契约（零裸色 / 零裸 px） | `crates/workbench/tests/ui_contract.rs` |

组件选型（**禁止手搓**）：折叠区用 `gpui_kit::component::accordion`（v1 用 `NCollapse`）；表格用 `table::DataTable`；Tab 条用组件 `TabBar` 或 Dock 自带能力；对话框用 `Dialog`；开关用 `Switch`；搜索框用 `input::Input`；右键菜单用 `ContextMenuExt`。图标走资产路径（`icons/lightbulb.svg` 等，`RightPanel::Insight` 已如此）。

## 9. 已确认决策

| # | 决策 | 出处 |
| --- | --- | --- |
| 1 | 洞察收敛为**单个右 Dock 面板**（v1 的右栏 + 底栏两处合并） | §1.3 |
| 2 | 规则三层作用域（内置 / 全局 / 项目），同名整体覆盖 | §1.2、`insight-dev-plan.md` §4.1 |
| 3 | 规则管理是**对话框**，不进面板 Tab | §5 |
| 4 | 内置规则**可禁用**（抑制记录，不改文件） | §5、`insight-dev-plan.md` §4.2 |
| 5 | 多列分析**重新设计**：列来源 = 真实列元数据，不照搬 v1 | §3.3 |
| 6 | 删除 `filterByValue` 的「把值当列名」行为；「按值筛选结果集」归 M5 | §8、`insight-dev-plan.md` §1.2 |
| 7 | 不迁 `autoOpenVisualization` / `pendingVisualizationRequest`（v1 死链路）；图表可视化归 M5/M6，洞察只出 `RenderHint` | `insight-dev-plan.md` §0 |
| 8 | 采样口径必须在 UI 明示 | §3.2、§4 |
| 9 | 历史对比用**增 / 减 / 不变三色**（修正 v1 只用一色） | §3.5 |
| 10 | 存储用量取后端真实统计，不由前端估算 | §3.5 |
| 11 | 洞察视图归属**待拍板**（方案 A：入 `crates/insight`；方案 B：留 `workbench`）——本文按 A 书写，布局与交互两案通用 | §1.3、§8、开发方案 §3.1 |

## 10. 与 V1 的逐项对照

| v1 功能点 | v1 位置 | 可用性 | v2 处置 |
| --- | --- | --- | --- |
| 轻量列统计（count / null / type / unique + 类型区块） | `ColumnInsightsPanel.vue` | ✅ | 合并进「列」Tab 基础统计区 |
| 全量列画像（基础统计 / 分布 / 质量 / 样本） | `InsightStatsSection.vue`（477 行） | ✅ | 照搬语义，按 280px 收窄 |
| 质量评分卡（总分 + 四维） | `QualityScoreCard.vue` | ⚠️ 组件在、数据恒空 | 重接（算法后端就绪） |
| 表质量聚合 + 各列质量列 | `TableProfileView.vue` + store | ✅ | 照搬语义 |
| 表探查列元数据表（PK 角标 / 可空 / 质量列） | `TableProfileView.vue` | ✅ | 照搬 |
| Schema 健康报告（4 折叠区 + 下钻 + Markdown 导出） | `SchemaInsightPanel.vue`（575 行） | ✅ | 照搬语义（导出属 Phase 4） |
| 历史版本列表 + 点击高亮 | `InsightHistoryTab.vue` | ⚠️ 可读、**无保存入口** | 先补保存入口 |
| 版本对比面板（字段 + `old → new (+Δ)`） | `InsightHistoryTab.vue` | ⚠️ 颜色只用一色 | 照搬 + 三色修正 |
| 多列分析（列多选 + 规则 + 执行 + 结果） | `MultiColumnView.vue` | ❌ **从未跑通** | 重新设计（§3.3） |
| 导出列洞察 JSON | `ColumnInsightPanel.vue` | ❌ 面板未挂载 | 不进首期（结论以快照持久化承载） |
| 规则热加载按钮 | `ColumnInsightPanel.vue` | ❌ 面板未挂载 | 改为目录监听 + 兜底按钮 |
| 适用规则标签区 | `ColumnInsightPanel.vue` | ❌ 面板未挂载 | 纳入「规则」对话框（按类型过滤） |
| 存储用量 + 清理 | `ColumnInsightPanel.vue` | ⚠️ 前端估算 | 后端真实统计（§3.5） |
| 底部四 Tab 容器 | `BottomInsightPanel.vue` | ✅ | 收敛为右 Dock 五 Tab（§1.3） |
| `filterByValue`（按值筛选） | `ColumnInsightPanel.vue` | ❌ 语义错误 | 删除（§8 决策 6） |
| `autoOpenVisualization` / `pendingVisualizationRequest` | store | ❌ 死标志 / 半链路 | 不迁（§8 决策 7） |
| i18n（6 个 key） | v1 `i18n` | — | 不适用（v2 单语言简体中文） |
