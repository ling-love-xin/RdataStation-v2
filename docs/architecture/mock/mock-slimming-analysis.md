# Mock（M7）模块瘦身可行性分析

> 结论先行：mock 模块**可以**做小规模瘦身（约 5 处，合计可减 700～1000 行 + 消除 1 处真实行为不一致），
> 但**大改不可行**——模块体量主要来自「143 变体 / 91 规则 / 6 模板 / 265 项测试」这些**定位本身**，
> 而非冗余代码。瘦身必须守住：定位（M7 一句话定位 + 元数据驱动 + 只落分析引擎）、
> README 承诺（变体数 / 规则数 / 模板数 / 测试数）、gpui-kit-dev 等 skill 约束。

- 分析日期：2026-10-29
- 分析对象：`crates/mock`（约 18,900 行）+ `crates/workbench` 侧装配（mock_generator / mock_jobs / mock_host，约 1,220 行）
- 证据口径：全部来自源码逐文件行数统计、`Grep` 交叉引用、README 声明核对；未运行构建（只读分析）

---

## 1. 规模基线（各文件行数 / 测试数）

### 1.1 crates/mock/src

| 文件 | 行数 | 测试数 | 性质 |
| --- | ---: | ---: | --- |
| `mock_view.rs` | 6,776 | 0（测试在 tests.rs） | 视图（面板 + 详情 + 对话框） |
| `mock_view/tests.rs` | 4,441 | 113 | 视图测试 |
| `generator_catalog.rs` | 2,315 | 3 | **脚本生成**（tools/gen_mock_generator_catalog.py 派生） |
| `generators.rs` | 2,186 | 32 | 143 变体 → faker 实现 |
| `engine.rs` | 1,753 | 14 | 生成 / 预览 / 导出 / 落库管线 |
| `templates.rs` | 1,665 | 11 | 6 套内置模板 |
| `schema_map.rs` | 1,140 | 26 | 91 条列映射规则 |
| `history.rs` | 1,008 | 8 | 生成历史 / 用户模板存储 |
| `models.rs` | 662 | 0 | 列定义 / GeneratorConfig 143 变体 / 导出模型 |
| `persistence.rs` | 614 | 6 | 任务 / 模板持久化 |
| `error.rs` | 81 | 2 | 错误类型 |
| `ui.rs` | 73 | 0 | 尺寸常量 |
| `commands.rs` | 16 | 0 | 面板动作 |
| `lib.rs` | 51 | 0 | crate 出口 |
| **src 合计** | **约 22,780** | **215** | |

### 1.2 crates/mock/tests（集成测试）

| 文件 | 行数 | 测试数 |
| --- | ---: | ---: |
| `mock_engine_tests.rs` | 1,525 | 41 |
| `persistence_roundtrip.rs` | 277 | 5 |
| `history_roundtrip.rs` | 252 | 4 |
| `temp_table_cleanup.rs` | 85 | 2 |

### 1.3 workbench 侧装配

| 文件 | 行数 | 职责 |
| --- | ---: | --- |
| `mock_generator.rs` | 656 | 草稿 → MockConfig / 场景生成 / 出口落库落盘 |
| `mock_jobs.rs` | 375 | 工作线程任务队列 |
| `mock_host.rs` | 191 | 宿主桥接（Rc<dyn> 注入） |

---

## 2. 定位与约束红线（瘦身不可触碰）

| 约束 | 出处 | 含义 |
| --- | --- | --- |
| M7 一句话定位 | README | 按列定义生成**可信**测试数据：元数据驱动、确定性可复现（seed）、**只落 DuckDB 分析引擎**（内存临时表/分析库），不回传源库 |
| 143 个生成器变体 | README / models.rs / generator_catalog | 是目录卖点与脚本穷尽派生的对象，**删变体即破定位** |
| 91 条映射规则 | README / schema_map.rs | 元数据驱动核心，规则即功能 |
| 6 套内置模板 / 24 处引用 | README / templates.rs | 场景生成主路径 |
| 265 项测试（215 单元 + 41 集成 + 5 + 4 + 2） | README 测试表 | 质量承诺，**瘦身不等于砍测试** |
| 生成 ≠ 写入 | README §3 | generate 只落内存临时表，落库落盘走显式出口 |
| gpui-kit-dev 约束 | `.agents/skills/gpui-kit-dev/SKILL.md` | 组件选型禁手搓、主题取色、render 权威同步点、事件/Action、动效规范、测试骨架 |

---

## 3. 可瘦身项（不影响定位、不违反 skill）

### A. 必做（低风险、纯瘦身 + 修不一致）

#### A1. `PREVIEW_ROWS` 三处重复且**值不一致**（真实行为差异）

| 位置 | 值 | 用途 |
| --- | ---: | --- |
| `engine.rs:66` | 10 | 生成后引擎内建预览取样 |
| `mock_view.rs:593` | 10 | 面板「预览行数档」首档 |
| `workbench/mock_generator.rs:287` | **20** | 场景生成后逐表重查预览 |

- 影响：单表生成预览 10 行、场景生成预览 20 行，行为不一致且同一概念三处维护。
- 方案：以 `engine.rs` 的 `PREVIEW_ROWS` 为单一来源（`pub(crate)` 或经 `lib.rs` 导出），
  mock_view 与 workbench 引用之；或明确区分「引擎取样行数」与「面板展示行数」两个常量。
- 预期减负：≈30 行重复定义 + 消除行为分叉。

#### A2. `MockError::TemplateNotFound` 是死变体

- 证据：全仓 `Grep` 只在 `error.rs:23`（定义）与 `error.rs:61`（自身测试）出现，
  生产代码无任何构造点（模板查找实际返回 `InvalidColumn` / `Generation` 类错误）。
- 方案：删除该变体及其测试断言（连带删除 `TemplateNotFound("t1")` 测试行）。
- 预期减负：≈2 行 + 消除误导性 API。

#### A3. `generator_catalog.rs` 是脚本产物，勿手改；优化应作用于生成脚本

- 2,315 行由 `tools/gen_mock_generator_catalog.py` 派生，属**数据**（143 变体的分类/标签/参数规格），
  不是手写代码。瘦身空间在脚本（如压缩 spec 常量表达），不在产物。

### B. 可选（中风险，需先出方案再动）

#### B1. `mock_view.rs` 单文件 6,776 行拆分为子模块

- 现状：面板（MockPanel）+ 详情（MockDetailView）+ 三个对话框 + 纯逻辑 + 常量全在一文件。
- 方案：拆 `mock_view/panel.rs`、`mock_view/detail.rs`、`mock_view/dialogs.rs`、`mock_view/pure.rs`，
  行为零变化；**注意**：测试文件 `mock_view/tests.rs`（4,441 行 / 113 测试）引用的路径与可见性需同步迁移。
- 不违反 skill：gpui-kit-dev 约束的是组件选型/渲染/事件模式，不约束文件组织。
- 预期减负：不减少总行数，但单文件可读性显著提升（这是「瘦身」的另一种形态）。

#### B2. `generators.rs` 中若干「别名变体」共享同一 faker 实现

- 证据（行为完全相同的对子）：
  - `Email` / `FreeEmail` → 均 `fake::faker::internet::en::FreeEmail`（generators.rs:226-244）
  - `Country` / `CountryCode` → 均 `fake::faker::address::en::CountryCode`（generators.rs:258-264）
  - `IpAddress` / `IP` → 均 `fake::faker::internet::en::IP`（generators.rs:328-341）
- 约束：**变体本身不能删**（143 是定位、目录按名字枚举、schema_map 规则按变体名引用），
  只能让 match 分支共享实现（如 `Email | FreeEmail =>` 合并 arm）。
- 预期减负：约 20～30 行；收益有限，主要价值是消除「两处同实现」的维护困惑。

### C. 不建议动（看似可瘦、实为定位/承诺）

| 项 | 原因 |
| --- | --- |
| 删变体 / 合并变体 | 143 变体是 README 卖点；generator_catalog 由脚本穷尽派生，删=改定位 |
| 砍 schema_map 规则 | 91 条规则是元数据驱动核心 |
| 砍模板 | 6 套模板是内置场景承诺 |
| 砍测试 | README 明确列 213+41+5+4+2 的测试矩阵，砍测试违反承诺 |
| `MockEngine::generate`（无进度版）仅测试使用 | 它是公开 API 与 README 入口，删除需连带改文档与测试 |
| `MockExportFormat::Table`（引擎内部用法） | 注释已声明「UI 不在此列」，是引擎能力保留，非冗余 |

---

## 4. 收益估算

| 类别 | 行数 | 说明 |
| --- | ---: | --- |
| A1 常量收敛 | ~30 | 消除 3 处重复 + 1 处行为不一致（10 vs 20） |
| A2 死变体 | ~5 | 删变体 + 测试断言 |
| B1 文件拆分 | 0（结构） | 6,776 行单文件 → 4 个子模块，可维护性收益为主 |
| B2 实现共享 | ~25 | 3 组别名变体合并 match arm |
| **合计** | **≈60 行 + 结构优化** | 真正可减行数有限，价值在一致性与可读性 |

**关键判断**：mock 模块「大」不是代码膨胀，而是定位（143 变体 + 91 规则 + 6 模板）与
质量承诺（265 测试）的必然结果。瘦身的正确姿势是**收敛重复常量、清理死代码、拆分巨型文件**，
而不是减功能。若要进一步减体量，只有调整定位本身（如缩减变体数）——那属于产品决策，不在本次范围。

---

## 5. 验证与风险

- 本分析未运行构建/测试；A1/A2 改动后应跑 `cargo test -p rds-mock --all-targets -j 2`（README 验证入口）。
- B1 拆分涉及 113 个视图测试的路径迁移，需独立 MR 并全量跑 `rds-workbench` 测试。
- 任何改动不得引入手搓组件 / 裸色值 / render 期 I/O（gpui-kit-dev 硬约束）。
