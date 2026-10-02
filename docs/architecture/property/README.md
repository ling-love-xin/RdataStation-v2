# 属性面板模块（M4 子模块）· 模块入口

> **宣传页（一页看懂）**：[`property-showcase.html`](property-showcase.html)（视觉版，明暗双主题、离线可开、可切换对象看页签联动）· [`property-showcase.md`](property-showcase.md)（可贴版，适合贴进 PR / wiki）
> **可交互原型**：[`property-prototype.html`](property-prototype.html)（模拟导航选中对象 + 页签切换 + 权限灰置）
>
> **一句话**：选中导航里的对象，右侧面板用页签把元信息摊开——列 / 索引 / 约束 / DDL 各归其位；驱动尚未实现的能力（权限 / 依赖）先绘制、置灰标注，不静默消失。
>
> 本文只提炼**特点 / 边界 / 地图**；细节一律指向下方文档，**不复制设计**。
> 状态：属性面板抽取为独立 crate（`rds-property`）+ tab 化重设计完成（2026-10-03）；SQLite / DuckDB 真机验证通过（详见测试报告）。

## 1. 模块特点

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **DBeaver 式 tab** | 面板头部 = 对象名 + 类型；tab 条 = 该类型注册的页集合；内容区 = 当前页。无数据分区显示「（无）」，不摆空表头 | 原型 §2、§3 |
| **描述驱动** | 页签集合是注册表数据（`PropertyRegistry::pages_for(kind)`），渲染只遍历描述，**不写 match 分支**；加能力不加代码 | 规格 §6、原型 §4 |
| **权限 / 依赖灰置** | 驱动能力未实现 → 页签**先绘制、置灰**，内容区明确说明；能力位补齐后注册表翻 `Supported`，渲染零改动 | 规格 §8、原型 §4 |
| **零 UI 造数据** | 网格与分区全部来自 `MetadataService` 实时内省；面板侧只有「键 → 显示名」映射 | 规格 §9 |
| **DDL 能取源就取源** | MySQL `SHOW CREATE` / SQLite `sqlite_master.sql` / DuckDB `duckdb_tables().sql`；取不到才由列/约束/索引合成并标注来源 | 规格 §9.3 |
| **过期结果丢弃** | 快速连续打开多个对象，先返回的旧结果按请求 key 丢弃 | 规格 §10、原型 §4 |
| **独立 crate** | `rds-property`（描述模型 + 数据容器，仅依赖 serde）；database 作数据生产者、workbench 作渲染方，三方职责分离 | 规格 §2、§5 |

## 2. 边界

- **只读**：属性面板不做元数据编辑（改名 / 建索引 / 授权不在本模块）。
- **不进引擎**：`rds-property` 只描述「属性长什么样」，不碰连接 / 内省；取数在 `database::property_panel`。
- **不硬凑能力**：驱动没实现的能力一律灰置，绝不用目录信息冒充（如权限清单）。

## 3. 文档地图

| 文档 | 内容 |
| --- | --- |
| [`property-module-spec.md`](property-module-spec.md) | 模块规格（总览 / 边界 / 依赖 / 代码地图 / 公开接口 / 领域模型 / 描述模型 / 权限灰置 / 数据流 / 渲染协议 / 使用方 / 测试 / 已知问题 / 实现位置映射） |
| [`property-prototype-design.md`](property-prototype-design.md) | 原型设计（布局结构 / 页签集合 / 灰置规则 / 交互 / 实现映射） |
| [`property-prototype.html`](property-prototype.html) | 可交互原型（对象切换 + 页签联动 + 灰置） |
| [`property-showcase.md`](property-showcase.md) · [`property-showcase.html`](property-showcase.html) | 一页看懂（宣传页，可贴版 / 视觉版） |
| [`property-user-guide.md`](property-user-guide.md) | 用户指南（打开方式 / 页签说明 / 灰置 / 常见问题） |
| [`property-test-report-2026-10-03.md`](property-test-report-2026-10-03.md) | 真机验证报告（SQLite / DuckDB 全链路 + 回归基线） |

## 4. 代码地图（入口）

| 层 | 位置 |
| --- | --- |
| 描述模型 / 数据容器 / 状态 | `crates/property/src/{descriptor.rs, model.rs}` |
| 取数（实时内省） / 注册表实现 | `crates/database/src/property_panel.rs` |
| 面板渲染（tab 化） | `crates/workbench/src/panels/editor.rs` |
| 请求入队 / 结果回填 | `crates/database/src/nav_jobs.rs` + `editor.rs::request_properties` |
