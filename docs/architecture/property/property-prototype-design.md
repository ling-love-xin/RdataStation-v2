# 属性面板模块 · 原型设计（右侧停靠面板，DBeaver 式 tab 页签）

> 状态：**已实现（2026-10-03，抽取 rds-property 后首版）** · 关联文件：`property-module-spec.md`（模块规格）、`property-prototype.html`（可交互原型）、`property-test-report-2026-10-03.md`（真机验证报告）
> 参考基准：DBeaver 对象属性（Entity Editor）多页签形态；配色服从 `docs/architecture/theme/theme-design.md`（rds-theme.json 语义 token）；技术栈 GPUI（gpui-kit 0.7.0 `TabBar::underline`），代码零裸 hex / 裸 px
> 设计要点：**描述驱动** —— 页签集合是注册表数据（`PropertyRegistry`），不是渲染分支；暂不支持的能力（权限 / 依赖）**先绘制、置灰标注**，不静默消失

## 1. 设计基准

| 维度 | 基准 | 说明 |
| --- | --- | --- |
| 形态 | DBeaver Entity Editor | 右侧停靠面板（编辑区右 Dock，宽度可拖拽记忆） |
| 页签 | gpui-kit `TabBar::underline`（Size::Small） | 与洞察面板 `insight_tab_bar` 同组件同档 |
| 配色 | theme-design.md | 面板底 `background`、头部 `tab_bar`、边框 `border`、正文 `foreground`、弱化 `muted_foreground`、错误 `danger` |
| 机制 | 描述驱动 | `PropertyRegistry::pages_for(kind)` → 页集合；渲染只遍历描述 |
| 数据 | 实时内省 | `database::load_properties`（MetadataService），后台任务取数、渲染零 I/O |
| 布局 | 右 Dock（280px 基准） | `h_resizable` 拖拽宽度，关闭时持久化 `settings.json` |

## 2. 布局结构

```
┌─ property panel（右侧停靠，右 Dock 宽度可拖）────────────────────────┐
│ 头部（tab_bar 底，高 2.125rem）：对象名（SEMIBOLD） · 类型文案（弱化） · 弹性 · 关闭 │
│ tab 条（underline，Size::Small）：[通用] [列] [索引] [约束] [DDL] [权限] [依赖]   │
│      （页集合由对象类型决定；权限 / 依赖置灰样式）                           │
├───────────────────────────────────────────────────────────────┤
│ 内容区（flex_1 + min_h_0，内部滚动，px_3 py_2）                    │
│  加载中… ｜ 错误（danger）｜ 当前页内容                            │
│   ├ 通用（General）→ 属性网格（label 定宽 4.5rem + 值弹性）          │
│   ├ 列 / 索引 / 约束 → 分区表格（表头弱化 + 数据行）                 │
│   ├ DDL / 源码 → 多行文本单列                                      │
│   ├ 权限 / 依赖（Unsupported）→ 灰置占位文案                       │
└───────────────────────────────────────────────────────────────┘
```

- **页签集合 = 注册表数据**：`PropertyRegistryImpl::pages_for(kind)`（表 = 通用/列/索引/约束/DDL/权限/依赖；视图 = 通用/列/DDL/权限；连接 = 通用/权限；例程 = 通用/源码；其余 = 通用）。渲染方不写 `match kind`。
- **灰置规则**：页签 `state = Unsupported` 时**可点**，内容区显示「「权限」查看与操作暂不支持（驱动能力未实现）」——用户能看到"这能力存在但暂不可用"；将来驱动补内省能力位，注册表翻 `Supported`，渲染零改动自动激活。
- **内容映射**：页签 id（`PageId`）↔ `ObjectProperties.sections[].id` 一一对应；找不到分区（如该表无索引）显示「（无）」，不报错、不摆空表头。
- **加载与错误**：`loading` 显示「加载中…」；错误整段 danger 色展示（保留重试语义由宿主事件路径负责）。
- **宽度记忆**：拖拽过程不写盘，关闭面板时持久化 `property_panel_width`（默认 24.5rem）。

## 3. 各对象类型页签集合

| 对象类型 | 页签（顺序即 tab 顺序） | 说明 |
| --- | --- | --- |
| 连接 | 通用 · 权限（灰） | 通用 = 名称 / 数据库类型 / 归属域 / 驱动 |
| Catalog / Schema | 通用 | 名称 / 类型 / 归属域 / 限定名 |
| 表 | 通用 · 列 · 索引 · 约束 · DDL · 权限（灰）· 依赖（灰） | 通用含列数；DDL 优先源版（MySQL/SQLite/DuckDB 存原文），取不到才合成 |
| 视图 | 通用 · 列 · DDL · 权限（灰） | 视图无源版时不摆空 DDL 分区 |
| 列 | 通用 | 名称 / 所属表 / 类型 / 可空 / 主键 / 外键 / 引用 / 默认值 / 注释 |
| 例程 | 通用 · 源码 | 源码先按存储过程取、未命中按函数取 |
| 序列 / 触发器 | 通用 | 触发器另显「关联表」 |

## 4. 关键交互

1. **打开**：导航双击对象 / F4 / 搜索结果行单击 / Quick Open `ShowProperties` → 右侧面板打开并加载。
2. **切换页签**：点击即切（`active_page` 在 `PropertyState`，跨请求保留在面板内）；加载完成前页签可点（内容区显示加载中）。
3. **灰置页签**：样式弱化（muted），点击显示占位文案，**不发起任何内省查询**。
4. **关闭**：面板头「关闭」；关闭时持久化拖拽宽度。
5. **过期结果丢弃**：快速连续打开两个对象，先返回的过期结果按请求 key 丢弃（`apply_props_results` 比对 `loaded_for`）。

## 5. 与实现映射

| 设计决策 | 代码文件 |
| --- | --- |
| 描述模型 / 注册表契约 | `crates/property/src/descriptor.rs` |
| 数据容器 / 状态 | `crates/property/src/model.rs` |
| 取数（实时内省） | `crates/database/src/property_panel.rs::load_properties` |
| 注册表实现（含灰置页） | `crates/database/src/property_panel.rs::PropertyRegistryImpl` |
| 面板渲染（tab 化） | `crates/workbench/src/panels/editor.rs::render_property_panel` |
| 请求入队 / 结果回填 | `crates/database/src/nav_jobs.rs` + `editor.rs::request_properties` |
