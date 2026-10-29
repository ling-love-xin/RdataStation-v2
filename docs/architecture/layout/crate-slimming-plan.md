# 其他 crate 瘦身计划（P3+）：切入点与架构

> 基线：2026-10-29（P2-c 提交 `17bd766d` 后）。延续 `shared-ownership-plan.md` 的 P2 系列实证结论：
> **Rust 编译单位是 crate，文件拆分不减少编译时间**；本计划的收益目标是**单文件体积与职责边界**
> （可维护性），不是编译时间。
>
> 与 P2 系列的区别：P2 是「下沉」（跨 crate 移动，workbench 行数实减）；本计划是「目录化」
> （crate 内部把超重文件拆成 mod 树，**crate 总行数不变，单文件行数下降**）。两条路分开算。

## 1. 实证盘点（2026-10-29，`crates/*/src` 口径，全仓实测）

| crate | src 行数 | tests 行数 | 最重文件（≤6） |
| --- | --- | --- | --- |
| engine | **61,489** | 3,270 | persistence/metadata_cache.rs **5166** · global_db.rs 2003 · driver/native/sqlite.rs 1645 · mysql.rs 1593 · postgres.rs 1482 · duckdb.rs 1350 |
| editor | 49,607 | 779 | core/mode.rs 655 · core/model.rs 282（已目录化，健康） |
| database | 27,781 | 987 | cache.rs 890 · model.rs 351（已拆散） |
| insight | 24,696 | 812 | model.rs **2713** · jobs.rs **1449** · insight_engine.rs 1250 · insight_view.rs 1041 |
| mock | 22,728 | 2,139 | generator_catalog.rs 2315（脚本生成）· generators.rs **2186** · engine.rs **1753** · history.rs 1008 |
| analytics_resource | 21,602 | 2,261 | filter.rs 949 · detail_view.rs 846 |
| workbench | **17,049** | 10,973 | view.rs **3262** · cache_dialog.rs 321 |
| result_panel | 15,733 | 0 | changes.rs 427（已拆散） |
| plugin | 15,001 | 3,493 | installer.rs 268（已拆散） |
| connection_dialog | 14,528 | 0 | components/connection_dialog/helpers.rs **2535** |
| scratchpad | 11,288 | 0 | jobs.rs 1124 · scratchpad_view.rs 544 |
| settings | 6,260 | 0 | registry/mod.rs 756 |
| project | 5,675 | 312 | ui.rs **2708** |
| connection | 5,149 | 400 | connector.rs **1140** |
| shared | 3,109 | 0 | error.rs **1175** · fact.rs 816 |
| workbench_shell | 1,596 | 0 | ui.rs 362 |

**全仓超重文件（≥1000 行，按性质分类）**：

| 文件 | 行数 | 性质（已读文件头实证） |
| --- | --- | --- |
| engine/persistence/metadata_cache.rs | 5166 | **纯逻辑**（rusqlite 元数据缓存：指纹路径/池/读写/迁移，无 GPUI） |
| workbench/src/view.rs | 3262 | **渲染装配宿主**（v5 布局：标题栏/活动栏/DockArea/状态栏/Quick Open 弹层） |
| insight/src/model.rs | 2713 | **纯逻辑视图模型**（types 已独立 mod；本文件为面板 Tab/目标/四态/列画像，不持色值可单测） |
| project/src/ui.rs | 2708 | **渲染 + 宿主注入**（选择器/项目菜单/设置/对话框，ProjectUiHost 端口） |
| connection_dialog/.../helpers.rs | 2535 | **UI 函数散集**（68 个 pub(crate) 项：图标/头部/过滤/表单/驱动元数据/策略，六组用途混杂，文件头 `use super::*`） |
| mock/generator_catalog.rs | 2315 | **脚本生成**（`tools/gen_mock_generator_catalog.py` 生成，不要手改）⚠️ 排除 |
| mock/generators.rs | 2186 | **纯逻辑**（faker locale 宏派发 + 生成器） |
| engine/persistence/global_db.rs | 2003 | **纯逻辑**（全局库存储） |
| mock/engine.rs | 1753 | **纯逻辑**（生成引擎） |
| insight/jobs.rs | 1449 | **事件接缝 + 后台取数**（GPUI 线程边界，结构已清晰） |
| engine/driver/native/*.rs | 1265–1645 | **驱动适配**（每驱动一文件 = 天然边界，不拆 crate） |
| shared/error.rs | 1175 | **纯逻辑**（错误类型 + 转换） |
| connection/connector.rs | 1140 | **纯逻辑**（连接器） |
| scratchpad/jobs.rs | 1124 | **后台任务** |
| mock/history.rs | 1008 | 生成历史 |

## 2. 关键发现（决定切入点）

1. **早期盘点（第一轮咨询时）的大文件已被拆散**：mock_view.rs 6776 / insight workspace.rs 4040 /
   database nav_view/rows.rs 3172 / connection_dialog render.rs 2792 均已不存在（现状为各自目录化形态）。
   **现存重文件是另一批**——盘点必须随时重跑，不能沿用旧清单。
2. **engine/persistence/metadata_cache.rs 5166 是全仓最重单一文件**，且是纯逻辑——优先级最高。
3. **脚本生成文件要排除**：generator_catalog.rs 由 `tools/gen_mock_generator_catalog.py` 生成，
   手拆会被脚本覆盖（文件头自述「不要手改」）。
4. **行数大 ≠ 需要拆**：editor 49,607 行但 top 文件仅 655（core/exec/language/notebook/support/ports/
   quick_open 目录化健康）；result_panel 15,733 / plugin 15,001 同样已拆散。**拆分的判据是单文件体积**
   与职责混杂度，不是 crate 总行数。
5. **workbench 17,049 行中 view.rs 占 3262**：P2 系列下沉后唯一巨型文件，是装配宿主（不该搬走，
   只能内部按布局区块拆）。

## 3. 拆分模式（延续 P2 实证，三种模式）

### 模式 A：纯逻辑目录化（无 GPUI 依赖）
适用：metadata_cache.rs 5166 / insight model.rs 2713 / mock generators.rs 2186 + engine.rs 1753 /
connection connector.rs 1140 / shared error.rs 1175 / scratchpad jobs.rs 1124。

方法（与 P2-a 下沉同构，只是不跨 crate）：
1. 按职责拆 mod 树（如 metadata_cache → `{paths, pool, io, invalidation, migrate}`）；
2. 原路径 `pub use` 重导保（`persistence/mod.rs` 重导出 `metadata_cache::*` 同名项，外部引用零改动）；
3. 可见性：`pub(crate)` 按需收紧，跨子模块引用用 `super::`/`crate::persistence::…`；
4. 测试随迁到对应子模块；`#[cfg(test)] mod tests` 不跨文件（避免 `use super::*` 引入外部宏）。

### 模式 B：渲染分区（GPUI 视图）
适用：workbench view.rs 3262 / project ui.rs 2708 / connection_dialog helpers.rs 2535。

方法（约束来自 gpui-kit 规范，`gpui-kit-dev/SKILL.md`）：
1. **只移动代码，不重写控件**：组件选型 / 颜色（`cx.theme().colors.*`）/ 动效（`with_animation`）
   一律保持原样，拆分不引入任何新控件或新样式——零 UI 行为变化；
2. **render 权威同步点不动**：`WorkbenchView::render` / `apply_left_mode` / `apply_right_mode`
   等装配逻辑留在宿主入口文件，子模块只提供**区块渲染函数**（如 `render_title_bar` / `render_sidebar`），
   入参显式传 `&mut Context<Self>` 所需句柄——不拆实体、不拆 Shared；
3. 子模块按**布局区块**（标题栏 / 活动栏 / Dock 装配 / 状态栏 / Quick Open 弹层）或**用途组**
   （helpers.rs 的六组）分文件；
4. 测试禁则沿用：测试模块显式列举依赖，**不用 `use super::*` / `use gpui_kit::*` 与
   `#[gpui_kit::test]` 同现**（recursion limit 自相残杀）。

### 模式 C：排除项（不拆）
- **脚本生成文件**：mock/generator_catalog.rs（生成器负责，拆了被覆盖）；
- **驱动适配文件**：engine/driver/native/*.rs（每驱动一文件 = 天然边界；超过 1600 行才评估
  驱动内部子模块化，如 `sqlite.rs` 1645 → `{conn, ddl, types}`）。

## 4. 优先级清单（切入点顺序，逐项给方法/验证/收益）

> 验证口径统一（每项）：`cargo check -p <crate> -j 2` exit 0 无新 warning → 涉及 crate lib 测试全绿 →
> `cargo check -p rds-app` exit 0 → 行数复核（单文件目标 ≤1200 行）。

| # | 文件面 | 性质 | 方法 | 预估 |
| --- | --- | --- | --- | --- |
| **P3** | engine/persistence/metadata_cache.rs 5166 | 纯逻辑 | 模式 A：拆 5–6 子模块（指纹路径 / 池 / 读写 / 失效 / 迁移），`persistence/mod.rs` 重导 | 单文件 5166→<1200；engine lib 测试（3270 行 tests）全绿 |
| **P4** | workbench/src/view.rs 3262 | 渲染装配 | 模式 B：按 v5 区块拆（title_bar / activity / dock / status / quick_open 弹层），render 与 assemble 留在 view.rs | view.rs 3262→<1500（装配核心）；workbench lib 30 passed 不动 |
| **P5** | insight model.rs 2713 + jobs.rs 1449 | 纯逻辑 | 模式 A：model 再拆（面板视图模型 / 阈值与文案 / 列画像视图模型）；jobs 保持（结构已清晰，仅评估） | model 2713→<1200；insight lib 测试全绿 |
| **P6** | project/src/ui.rs 2708 | 渲染 + 端口 | 模式 B：按对话框/选择器/菜单/设置拆 3–4 文件，`ProjectUiHost` 端口不动 | ui.rs 2708→<1200 |
| **P7** | connection_dialog/.../helpers.rs 2535 | UI 函数散集 | 模式 B：按用途六组拆 4–5 文件（icons_input / headers / filters / forms / driver_meta / policy），`helpers/mod.rs` 重导 | 单文件 2535→各 <600 |
| **P8** | mock generators.rs 2186 + engine.rs 1753 + history.rs 1008 | 纯逻辑 | 模式 A：目录化（排除 generator_catalog 脚本生成文件） | 各 →<1200 |
| **P9** | 低优先档：engine global_db 2003 / connection connector 1140 / shared error 1175 / scratchpad jobs 1124 / driver native 单文件超 1600 | 纯逻辑/适配 | 模式 A 或 C（驱动内部子模块化） | 边际收益，按需 |

**预估总工作量**：P3–P8 约 12–20 小时（每项含编译/测试验证；`-j 2` 下每次 check 2–4 分钟）。
P9 视需要。

## 5. 约束（全部来自既有实证，不新增）

1. **不拆 crate**：engine / editor / database 等 crate 边界不动（依赖汇聚点分析已在 P2 完成，
   workbench 是唯一汇聚点；特性 crate 内部拆分不影响编译图）；
2. **对外路径零改动**：所有 `pub use` 重导保持（`lib.rs` / `mod.rs`），外部引用不因拆分改一行；
3. **gpui-kit 规范**：模式 B 拆分不改变组件选型 / 取色 / 动效 / 事件模式（`gpui-kit-dev/SKILL.md`
   约束全量适用）；测试禁 `use super::*` + `#[gpui_kit::test]` 同现；
4. **一次一模块立即 check**（`panels-coupling-plan.md` §9 注意事项 4）；
5. **脚本生成文件不拆**（generator_catalog.rs）。

## 6. 收益口径（诚实标注）

- **可度量**：单文件行数（目标 ≤1200）；职责边界（文件头 doc 注释能一句话说清）；
  测试定位（测试随迁子模块，失败信息指向职责而非巨型文件行号）。
- **不可度量（P2 实证）**：编译时间——Rust 编译单位是 crate，本计划不改变任何 crate 边界，
  **编译时间不变**。
- **减行数不是本计划目标**：文件内拆分是移动不是删除；若目标是**净减行数**，走清理路线
  （重复代码 / 死代码 / 孤儿文件，参照清理批 `b027470b` 的做法：7 unused import + 2 孤儿文件 +
  DEBUG 残留），那是一条独立路线，可与 P3+ 并行推进。
