# ER 建模视图设计（M4）

> **一句话**：把「某个 schema 里有哪些表、它们怎么被外键连起来」画成一张**可读的图**——实体卡片 + 外键连线，宿主原生 GPUI 渲染，**不引入 webview / 前端栈**。
>
> 本文回答「为什么这么设计 / 结构怎么切 / 做到哪了」；上游决策见
> [`../plugin/plugin-prototype-design.md`](../plugin/plugin-prototype-design.md) §11.3（ER 评估）与 §14.3（四场景之三）。
>
> 状态：**第一期骨架已落地（2026-10-15）**——ER 输入模型 + 「元数据 → 模型」桥 + 分层布局（纯函数，带单测）；渲染与面板装配为后续切片。

## 1. 结论与定位

- **以插件形式进入合适，但视图不走 webview**。用户定调：本项目由 tauri 架构迁来，**不引入 webview / 前端栈**（避免 z-order、输入桥接、主题不一致、性能一类前端问题）。路线 = **GPUI 原生渲染**。
- **视图 = 宿主的原生面板**；插件系统退到「**数据源 + 视图声明**」角色（`contributes.views` + 数据变换），v1 只读渲染甚至可不依赖插件系统（宿主直取元数据），插件是加分项不是前提。
- **桌面工具先例清一色原生渲染**（§11.3 已核实）：DBeaver = Java/SWT + GEF/Draw2D；DataGrip = Swing + IntelliJ Platform Diagram 框架；Navicat/HeidiSQL 各自原生图形框架。web 系（drawdb / erd-editor / Mermaid / dbdiagram）全部需要 webview → **已否决**。
- **代价说明**：等于自研一个**小型图编辑器**（实体卡片 + 关系连线 + 布局 + pan/zoom）。「组件选型不手搓」约束管的是**组件层**（按钮 / 列表仍用 gpui-kit）；图视图是**宿主面板级能力**，二者不冲突。

## 2. 边界与硬约束

- **只读结构**：ER 只看「表 / 列 / 主外键」结构，**不直连库、不碰凭据**（对齐插件硬约束 #1、#2）。数据来源是宿主已落地的元数据服务，不经插件宿主、不额外建连。
- **不引入前端栈**：无 webview、无 JS 图库、无 `evaluate_script` 桥。
- **零裸值**：颜色走 `cx.theme().colors.*`；尺寸进 `crates/workbench_shell/src/ui.rs` 常量（裸 `px(` 会被 `ui_contract` 拦下）。
- **render 零 I/O**：元数据加载走后台任务 + 结果回填（对照 M4 导航：`nav_jobs`）；render 只读 Shared。
- **回程分层**：正向（元数据 → 图）第一期做；导出 DDL 可先做；**写回源库后置**（涉及凭据 / 权限，与 mock「永不回写源库」纪律对齐）。

## 3. 数据流

```text
meta.* 五件套（catalogs / schemas / objects / object_detail / routine_source）
  → 宿主 MetadataService（已存在，M4：list_tables / list_columns / list_constraints）
  → TableInput（schema + name + kind + 列 + 约束）
  → build_diagram（纯函数）→ ErDiagram（实体 + 关系，仅结构）
  → layered_layout（纯函数）→ LaidOutDiagram（实体框 + 内容包围盒）
  → GPUI 原生绘制（卡片 + 折线 + crow's foot + pan/zoom）
```

前两段（`TableInput → ErDiagram → LaidOutDiagram`）**本轮已落地**，是纯函数、可单测：
`crates/database/src/er/{model.rs,layout.rs}`。

## 4. 模块归属

**落 M4 `crates/database/src/er/`，暂不新建 crate。**

对照 `rds-architecture` 的 crate 归属判定（`docs/architecture/settings/settings-crate-design.md` §1）：

| 判据 | 现状 | 结论 |
| --- | --- | --- |
| 独立状态与生命周期 | 有（图模型 + 布局 + 视口），但生命周期随面板 | 尚不足以立 crate |
| 稳定公开边界 | 有（模型 / 桥 / 布局 / 视图） | 具备雏形 |
| 使用方 ≥2 | **否**（目前只有 workbench 装配） | **否决**新建 |

- `crates/database` 本就是「元数据浏览器 + 属性面板」的 M4 家，且已依赖 `gpui-kit` + `workbench_shell`，视图可原地放。
- **依赖方向**：`database（Feature）→ engine / shared / property / gpui-kit / workbench_shell`，不反向依赖 `workbench`。view 由 workbench 装配（对照导航 / 属性面板：`workbench/src/panels/*`）。
- **将来抽 crate 的触发条件**：ER 成为**独立窗口**，或被 Mock（M7）/ 洞察（M8）等 ≥2 处复用视图能力时，再抽 `rds-er`。

## 5. ER 输入模型（`er::model`）

只读投影，字段名与语义对齐 `engine::driver::traits`，避免二次解释：

| 类型 | 形状 | 说明 |
| --- | --- | --- |
| `ErDiagram` | `{ entities: Vec<ErEntity>, relations: Vec<ErRelation> }` | 图的全部内容 |
| `ErEntity` | `{ schema, name, key: EntityKey, is_view, columns: Vec<ErColumn> }` | 表 / 视图（+ 其列） |
| `ErColumn` | `{ name, data_type, nullable, is_primary_key, is_foreign_key, references: Option<ErColumnRef> }` | 列的只读投影 |
| `ErColumnRef` | `{ table, column }` | 列级外键目标端点 |
| `ErRelation` | `{ from, from_columns, to, to_columns, name, on_delete, on_update }` | 外键边（子表列 → 父表列） |
| `EntityKey` | `schema.name`（无 schema 层退化 `name`） | 实体唯一键 |

**要点**：

- `EntityKey` 只由构造保证自洽，identity 一律用全串；`display_name()` 只用于显示。
- `is_foreign_key` 与 `references` 的分工沿用 `ColumnDetail`：前者答「有没有」，后者答「引到哪」——渲染判断外键要读前者。

## 6. 元数据 → ER 桥（`er::model::build_diagram`）

输入 `&[TableInput]`（`{ schema, name, kind, columns, constraints }`），输出 `ErDiagram`。规则：

1. **实体**：键 = `schema.name`；同名输入只保留首个；`kind == View` 置 `is_view`。
2. **列序**：按 `ordinal`（1 基）排序；全为 0（驱动没给）时保持输入顺序（稳定排序）。
3. **关系两来源**：
   - ① 约束里 `FOREIGN KEY`（`constraint_type` 含 `FOREIGN` 或为 `FK`）→ 一条关系，带约束名 / `on_delete` / `on_update`；
   - ② 列上的 `references` → 兜底关系（驱动只给列级引用、未给约束时）。
   - 同表内**已被约束覆盖的列**不再走 ②，避免同一条外键出两条关系；多列外键按整组算一条。
4. **自洽性**：端点不在本次集合内的关系被**丢弃**（跨 schema / 未加载表）——v1 保证图自洽，跨 schema 关系后置。

## 7. 分层布局（`er::layout::layered_layout`）

纯函数、确定性。**被引用者在前（左），引用者在后（右）**：

- **分层**：依赖图（child → parent）按**最长路径**定层：`layer(v) = 1 + max(layer(parent))`，无父者为 0。**容忍环**——值以 `n` 封顶并把最小值归一化到 0，迭代至多 `n` 轮必终止（自引用不入依赖）。
- **层内排序**：重心启发式——按已定序父节点位置的平均排列，减少连线交叉；无已定序父者按原始下标兜底。
- **落位**：层号 → x（`padding + l*(card_width + h_gap)`）；层内自顶向下堆叠、每层在垂直方向居中；卡片高 = `header_height + 列数 × row_height`。
- **输出**：`LaidOutDiagram { boxes, bounds }`；`boxes` 与 `entities` 同序同长（视图可直接 `zip`），`position_of(&EntityKey)` 供画连线定位。
- **度量入参**：几何参数由 `LayoutMetrics` 显式传入；`default()` 只服务测试 / 无主题环境，**视图必须传 `ui.rs` 派生值**。

## 8. 渲染方案（GPUI 原生，后续切片）

- **实体卡片**：矩形 + 表头（表名）+ 列行（名 / 类型 / 主键 / 外键标记）。用 gpui-kit 组件做内部行（如 `badge` / `marker`），卡片本体是面板级绘制。
- **连线**：折线（正交走线）+ 端点标记（crow's foot / 1:N）；按列的纵向位置吸附到行。
- **交互**：pan / zoom（滚轮缩放 + 拖拽平移）、卡片拖拽重排、布局持久化。
- **状态**：图模型 / 布局 / 视口放 `Shared`；`render` 纯读；加载走后台任务回填（render 零 I/O）。
- **颜色 / 尺寸**：`cx.theme().colors.*` + `ui.rs` 常量（新的 `ER_*` 常量按 `ui_size_constants_match_design` 登记）。

## 9. 面板装配与插件角色

- **面板形态**：中央 Dock 独立 tab（对照属性面板 `workbench/src/panels/property_panel.rs::PropertyHostPanel` 的装配方式）；入口从导航树右键「查看 ER 图」/ 命令面板。
- **作用域**：以「连接 + schema」为一个图；大 schema 分批加载 + 过滤（后置）。
- **插件角色**：插件贡献 `contributes.views` 声明 + 数据变换（`rds.metadata` → ER 输入模型）；**v1 宿主直取**，插件接入后置（与 §11.3「插件是加分项不是前提」一致）。
- **与 shell 插件系统分界**：gpui-kit `shell` 的 QuickJS 插件模型是「宿主注册组件 + 脚本组合元素树」，**无 canvas / path 自由绘制** → 不适合画 ER（§11.3 附注）；ER 走原生面板。

## 10. 分期落地（对齐 §11.3 五步）

| 期 | 内容 | 状态 |
| --- | --- | --- |
| ① | schema → 渲染模型桥（`meta.*` → 表 / 列 / 外键 → 布局输入） | ✅ 本轮（`er::model`） |
| ① | 分层布局（外键引用分层） | ✅ 本轮（`er::layout`） |
| ② | 只读渲染（实体卡片 + 连线 + 自动布局）双接入面板 | ⏳ 下一片 |
| ③ | pan/zoom + 拖拽排布 + 布局持久化 | ⏳ |
| ④ | DDL 导出 | ⏳ |
| ⑤ | 写回源库 | 后置（涉及凭据 / 权限） |

## 11. 开放项（未确定）

- **渲染实现细节**：卡片/连线的具体绘制方式（`canvas` / `Path` / 子元素堆叠）与命中测试方案未定；gpui-kit 无 canvas 自由绘制的既有先例，需在下一片查证 0.7.1 API 后定。
- **端点吸附**：连线吸附到具体列行 vs 卡片边缘（后者实现简单、前者更精确）未定。
- **布局算法**：自带分层 vs 引入 `petgraph`（§11.3 列为候选）vs 可选 graphviz。当前为自研分层（无新依赖）；若拓扑/层次需求变复杂再评估。
- **性能**：大 schema（数百表）一次性成图的可读性与绘制成本未验证；可能需要按「表分组 / 邻居子图 / 过滤」限流。
- **作用域与跨 schema**：v1 只画单 schema 且丢弃跨 schema 关系；多 schema / 跨库关系后置。
- **布局持久化**：手工拖拽后的位置是否落库（`navigator_state` 类似物）未定。
- **插件接入点**：`contributes.views` 声明形状与「数据变换」契约未细化。
- **`ordinal` 缺省**：驱动不给 `ordinal` 时按输入顺序（已实现），但不同驱动输入顺序稳定性未逐一核实。

## 12. 实现位置映射表

| 设计决策 | 代码位置 |
| --- | --- |
| ER 输入模型（实体 / 列 / 关系 / 键） | `crates/database/src/er/model.rs` |
| 元数据 → ER 桥（`build_diagram`） | `crates/database/src/er/model.rs` |
| 外键判定（`is_foreign_key_constraint`） | `crates/database/src/er/model.rs` |
| 分层布局（`layered_layout` / `assign_layers`） | `crates/database/src/er/layout.rs` |
| 布局度量（`LayoutMetrics`） | `crates/database/src/er/layout.rs` |
| 模块注册 | `crates/database/src/lib.rs` |
| 上游决策（ER 评估 / 四场景） | `docs/architecture/plugin/plugin-prototype-design.md` §11.3 / §14.3 |
| 渲染 / 面板装配（待做） | `crates/database/src/er/view.rs`（拟）+ `crates/workbench/src/panels/`（拟） |

## 13. 验证

```sh
cargo test -p rds-database --lib -j 2
```

- 桥单测：列序（有 / 无 `ordinal`）、约束成关系、列级兜底、约束优先、复合外键去重、悬空关系丢弃、视图标记、重复输入去重、无 schema 层退化。
- 布局单测：被引用者在前、无依赖同层、最长路径链序、环终止且有界、boxes 与 entities 同序、层间不重叠、确定性、空图、卡片高随行数。
