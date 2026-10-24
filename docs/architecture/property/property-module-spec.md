# RdataStation v2・属性面板模块（Property）规格书

> **用途**：本文件是 property 模块的自包含技术规格 —— 把代码、文档、测试、决策浓缩为一份
> 可独立复现的说明。目标读者：新开发者 / 另一 AI，期望据此生成相同结构、相同语义的代码与文档。
> 生成基准：仓库当前 HEAD（2026-10-03，属性面板从 M4 抽取为独立 crate 的首轮）。
> 一切以「代码为准」：本文件若与源码冲突，以源码为准。
>
> 文档体例与 `docs/architecture/connection/connection-module-spec.md` 对齐（模块总览 / 边界 /
> 技术栈依赖 / 代码地图 / 公开接口 / 已知问题 / 实现位置映射表）。

---

## 1. 模块总览

| 项 | 内容 |
| --- | --- |
| 一句话 | 把「一个数据库对象长什么样、能看哪些属性」做成**Provider 驱动**的可复用面板：对象类型 → provider（`PropertyProvider` trait，按驱动/类型注册）→ 动态渲染；暂不支持的能力（如权限）**先绘制、置灰标注**，不做硬编码分支 |
| 交付状态 | 抽取 + 重设计（2026-10-03 起）；抽取后 `crates/property` 独立 crate，`database` 仅保留取数实现；2026-10-15 描述模型（`PropertyPage`/`PropertyEntry`/`PropertyRegistry`）删除，能力掩码并入 `PropertySection` |
| 权威文档 | 本文件（§13 = 已知问题唯一权威） |
| 主要文件 | 见 §4 代码地图 |
| 测试基线 | rds-property 单测（模型 / provider 契约）；rds-database / rds-workbench 回归全绿 |

### 1.1 模块边界（做什么 / 不做什么）

**做**：属性面板领域模型（`PropertyKind`）、数据容器（`ObjectProperties` / `PropertyRow` /
`PropertyTable` / `PropertySection`）、面板状态（`PropertyState`）、provider 契约（`PropertyProvider` trait
与 `ProviderRegistry`，见 §16）。
**不做**：数据库对象树导航（M4 `nav_view`，仍属 `database`）；属性面板的停靠容器（中央 Dock
`PropertyHostPanel` 独立 tab，与编辑器同级别，2026-10-09 起；宽度由 Dock 拖拽记忆）；元数据实时内省
与驱动（`engine`）；对象定位参数（`PropertyRef` / `PropertyRequest`，留在 `database::model` —— 它们是
**导航域 → 属性面板**的输入契约，依赖 `NavSource`）。

## 2. 技术栈与依赖

| 项 | 内容 |
| --- | --- |
| 序列化 | serde（`PropertyKind` 参与 `PropertyRef` 序列化） |
| 数据容器 | 纯 Rust 结构体（`String` / `Vec`），无运行时依赖 |
| provider 契约 | `trait PropertyProvider` + `ProviderRegistry`（无默认实现，由 `database::property_providers` 注册各驱动实现） |
| 异步 | 不涉及（取数在 `database`，入队 / 轮询在 `nav_jobs` / `PropertyHostPanel`） |
| 构建约束 | `cargo build/test` 必须 `-j 2`（Windows 已知）；`.cargo/config.toml` 已设 `RDS_HOME=.rds` |
| 目标平台 | Windows（本机验证环境）；PowerShell 交互（Bash 不可用） |

**依赖方向（符合 rds-architecture）**：

```text
property ───────────► (serde / shared，无 database / engine / workbench 依赖)
database ───────────► property + engine + shared + workbench_shell + gpui-kit
workbench ──────────► property + database + engine + ...
mock（未来）────────► property（消费 ObjectProperties / PropertyKind 做结构预览）
```

## 3. 背景：为什么抽取

原属性面板为 M4 内部实现（`crates/database/src/property_panel.rs` + `workbench/panels/editor.rs::render_property_panel`），
存在四个问题：

1. **无 tab**：列 / 索引 / 约束 / DDL 纵向堆叠平铺，大表展开一屏放不下；DBeaver 是
   General / Columns / Indexes / Constraints / Privileges / Dependencies / DDL 多页签。
2. **静态硬编码**：`load_properties` 是 300 行大 `match ref_.kind`，每类对象的行、分区写死；
   渲染层（editor.rs）也跟着逐行拼 —— 新增对象类型要改取数与渲染两处。
3. **不支持的能力直接消失**：权限、依赖等完全没有，用户无法感知「这能力存在但暂不可用」。
   （2026-10-24 决策反转：隐藏式成为拍板策略——未实现类别不产出页签，对齐 DBeaver
   呈现语义；能力存在感由代码侧接线位 `DriverCapabilities` 与 §8 承载，见 §7。）
4. 无可复用公开边界：Quick Open 已通过 `database::model::PropertyRequest` 打开属性面板，
   Mock 结构预览需要复用列结构 —— 模型埋在 database 里，跨 crate 消费不干净。

**使用方清单（抽取依据）**：

| 使用方 | 现状 | 消费内容 |
| --- | --- | --- |
| M4 导航面板 | ✅ 已接 | `PropertyRequest` 构造、双击 / F4 / 搜索结果单击打开 |
| Quick Open | ✅ 已接 | `ShowProperties(Box<PropertyRequest>)` 命令（`quick_open/model.rs`） |
| Mock 结构预览（M7） | 🔜 规划 | `ObjectProperties` / `PropertyKind`（列结构预览） |
| 洞察（M8） | 🔜 规划 | 同上 |

## 4. 代码地图

### 4.1 `crates/property/`（新 crate，package `rds-property`）

| 文件 | 职责 |
| --- | --- |
| `src/lib.rs` | crate 导出与公共 API |
| `src/model.rs` | `PropertyKind`（自 `database::model` 迁入）；`ObjectProperties` / `PropertyRow` / `PropertyTable` / `PropertySection` / `PropertyState`（自 `database::property_panel` 迁入） |
| `src/descriptor.rs` | `PageId` enum（页 id 集合；描述模型 `PropertyPage` / `PropertyEntry` / `SupportState` / `PropertyRegistry` 已于 2026-10-15 删除，能力掩码并入 `PropertySection.enabled`） |
| `src/provider.rs` | provider 契约：`PropertyProvider` trait / `ProviderRegistry` / `ProviderInput` / `ObjectOverview` / `PropertySection`（原 `PropertyCategory`） |
| `src/tests.rs` | 单测（kind 覆盖、描述模型、状态默认值） |

### 4.2 `crates/database/`（保留取数实现）

| 文件 | 职责（改动后） |
| --- | --- |
| `src/model.rs` | 删除 `PropertyKind` 定义（改 `use property::PropertyKind`）；`PropertyRef` / `PropertyRequest` / `property_ref_of` 保留（字段类型改用 `property::PropertyKind`） |
| `src/property_panel.rs` | 删除数据容器定义（改 `use property::…`）；保留 `load_properties`（取数，返回 `property::ObjectProperties`）；provider 分派（`property_providers::global_registry()` 按 type_id 选源） |
| `src/nav_jobs.rs` | `PropsResult` 改用 `property::ObjectProperties`；`enqueue_properties` 不变 |

### 4.3 `crates/workbench/`（消费与渲染）

| 文件 | 职责（改动后） |
| --- | --- |
| `src/panels/property_panel.rs` | `PropertyHostPanel`（中央 Dock 独立 tab，2026-10-09 起）**Provider 驱动渲染**（见 §10 / §16）；`PropertyState` 增加 `active_page` |
| `src/quick_open/model.rs` | `PropertyRequest` 类型不变；tests 中 `PropertyKind` 引用改 `property::PropertyKind` |

## 5. 公开接口

`lib.rs` 导出：

```text
pub mod descriptor;   // PageId（页 id 集合）
pub mod model;        // PropertyKind / ObjectProperties / PropertyRow / PropertyTable / PropertySection / PropertyState / TableDataColumn / TableDataPage
pub mod provider;     // PropertyProvider trait / ProviderRegistry / ProviderInput / ObjectOverview / ExtraQuery / ProviderStats

pub use descriptor::PageId;
pub use model::{ObjectProperties, PropertyKind, PropertyRow, PropertySection, PropertyState, PropertyTable, PropertyValueKind, TableDataColumn, TableDataPage};
pub use provider::{ExtraQuery, ObjectOverview, OverviewCard, OverviewStatus, PropertyProvider, PropertyTargetRef, ProviderInput, ProviderRegistry, ProviderStats};
```

## 6. 领域模型

### 6.1 `PropertyKind`（自 database::model 迁入，语义不变）

连接 / Catalog / Schema / 表 / 视图 / 列 / 例程 / 序列 / 触发器，共 9 种。

### 6.2 `ObjectProperties`（数据容器，自 database::property_panel 迁入）

```text
pub struct ObjectProperties {
    pub title: String,        // 标题（对象名）
    pub object_type: String,  // 对象类型文案（BASE TABLE / VIEW / …）
    pub source_label: String, // 归属域文案（项目 (P) / 全局 (G) / 共享 (GP)）
    pub properties: Vec<PropertyRow>,     // 上部属性网格
    pub sections: Vec<PropertySection>,   // 子实体分区（列 / 索引 / 约束 / DDL / 源码）
    pub data: Option<TableDataPage>,      // 「数据」页内容（分页抓取；仅表/视图请求数据时填充）
}
```

`PropertySection` 带 `id: PageId` 字段（分区标识，渲染按 id 匹配内容；能力掩码见 §7）。

### 6.4 数据页模型（M4 表数据预览首期，2026-10-03 新增）

```text
/// 数据页列描述。
pub struct TableDataColumn {
    pub name: String,
}

/// 一页数据：列 + 行（单元格 `None` = SQL NULL，渲染置灰）。
pub struct TableDataPage {
    pub columns: Vec<TableDataColumn>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total: Option<u64>,   // 真 COUNT(*)；拿不到如实留空
    pub offset: usize,        // 本页起始行号（真实行号 = offset + 行下标 + 1）
    pub has_more: bool,       // limit+1 探测：还有没有下一页
    pub is_ordered: bool,     // 有主键或用户排序 → 分页稳定
}
```

取数请求在 `rds-database` 侧（`crates/database/src/table_data.rs`）：

```text
pub struct TableDataRequest {
    pub offset: usize,                 // 页起点
    pub limit: usize,                  // 0 = 默认 200
    pub order_by: Vec<(String, bool)>, // (列名, 升序)；用户排序 + 主键补齐
}
```

### 6.3 `PropertyState`（面板状态，自 database::property_panel 迁入）

```text
pub struct PropertyState {
    pub loaded_for: Option<String>,
    pub props: Option<ObjectProperties>,
    pub error: Option<String>,
    pub loading: bool,
    pub active_page: PageId,  // 新增：当前激活页（tab 化）
}
```

## 7. 能力掩码（子项分区）

> 2026-10-15：描述模型（`PropertyPage` / `PropertyEntry` / `SupportState` / `PropertyRegistry` trait）已删除，
> 能力掩码由 **provider 的分区声明**承担；**2026-10-24 展示策略拍板**：未实现类别**不产出**
> 分区（隐藏式，对齐 DBeaver 只显示有内容页签的呈现语义）——`PropertySection` 不再有
> `enabled` 字段，`sections` 里全是可用类别。

```text
/// 子实体分区（带页 id，供渲染映射「分区 → 内容」）。
pub struct PropertySection {
    pub id: PageId,
    pub label: String,
    pub table: PropertyTable,
}
```

- provider 的 `categories()` 返回 `Vec<PropertySection>`。类别结构收敛到
  `database::property_providers` 的生成函数（`table_categories` / `view_categories` /
  `schema_categories` / `connection_categories`），由 [`DriverCapabilities`] 能力位驱动：
  每个类别声明 `Some(true)` = 已实现（产出分区）、`Some(false)` = 未实现（**接线位，
  不产出**，注释说明未来接入点）、`None` = 该驱动不声明；pg / sqlite / mysql / duckdb
  各填一份 caps，类别结构与顺序只写一份。新增数据库类型 = 补一份 caps；接入某能力 =
  把对应位翻 `true`，页签自动出现，渲染零改动。
- 列级展示同样按能力位裁剪：`DriverCapabilities::identity_column` / `collation_column`
  控制「列」分区表格的 标识 / 排序规则 两列（PG / MySQL 有真值，SQLite / DuckDB 无）。
- **动态渲染原则**：渲染层只遍历 `ObjectProperties.sections`，不按 `kind` 写分支；
  新增对象类型 / 新增属性 = provider 加条目（或新注册一个 provider），渲染零改动。

## 8. 未实现能力规格（示例：权限页）

- **展示策略（2026-10-24 拍板）**：未实现类别不产出页签——用户只看到有内容的子项，
  面板无「暂不可用」噪音；未实现能力的感知由代码侧接线位（caps `Some(false)` 注释）与
  本 spec 承载，UI 不再画灰置页。
- **接线位示例**：`DriverCapabilities::pg()` 的表级 触发器 / 权限 / 依赖 / 规则 / 分区、
  连接级 存储 / 权限 / 事件触发器均为 `Some(false)`——内容批次接入时把对应位翻 `true`，
  `table_categories` / `connection_categories` 自动产出分区，渲染零改动激活。
- 渲染兜底：`sections` 为空（列 / 序列等无可展示子项的对象）→ 子项卡显示「（无）」；
  未注册 provider → 仅 General 类别（`fallback_categories`）。

## 9. 数据流

```text
导航双击 / 右键「查看属性」 / Quick Open ShowProperties
  → PropertyRequest（database::model，含 PropertyRef + conn_label + driver + data）
  → NavHost::show_properties(request, window, cx) → workbench open_property 回调
     （中央 Dock tab：面板未建 → cx.new + area.add_panel(Center) + 聚焦；已建 → 替换内容 + 聚焦）
  → PropertyHostPanel::request_properties
  → nav_jobs::enqueue_properties（后台单工作线程）
  → database::load_properties（MetadataService 实时内省）
      └ 若 data = Some(TableDataRequest)：调 table_data::load_table_data
         （标识符转义 → ORDER BY 用户排序+主键补齐 → LIMIT limit+1 OFFSET 探测 has_more
          → 真 COUNT(*) 单独一问 → 单元格 JSON 化，null → None）
  → property::ObjectProperties（含 data: Option<TableDataPage>，回填 PropertyState，
    key 不匹配的过期结果丢弃）
  → PropertyHostPanel::render_property_panel：按 `ObjectProperties` 渲染（General 网格 +
    子项分区列表；「属性 | 数据」两页签）
  → 「数据」页：render_data_page（列头排序 / 上一页 / 下一页 → 事件路径
    reload_properties_data 重发请求，渲染零 I/O）
```

「数据」页懒加载：首次切到该页签且 `data` 未请求时才自动带 `TableDataRequest::default()`
重发（首屏 200 行）；排序回到第一页，主键自动跟在用户排序后补全（分页稳定）。

## 10. 渲染协议（tab 化）

1. 顶部 tab 条：`属性 | 数据`（仅表 / 视图两页签，gpui-kit TabBar）；当前页 = `PropertyState.active_page`。
2. 当前页内容：
   - `General` → `ObjectProperties`：① 洞察总览卡（`overview`）+ ② General 网格（`properties`，
     双栏/三栏）+ ③ 子项分区列表（`sections`，多分区时左栏子标签 + 右栏表格；
     **DDL / 源码分区 = 等宽代码块 + 复制按钮**，长行折行不截断，2026-10-24）；
   - `Data` → `data` 网格（行号 + 列名 + 单元格；NULL 灰置；列头排序；分页按钮）；
   - 找不到对应 section → 空态文案（不报错）。
3. 宽高沿用右 Dock（280px 基准，宽度可拖拽记忆），行内无裸 px / 裸色值（走 `workbench_shell::ui` 与主题 token）。

## 11. 使用方

| 使用方 | 接入方式 |
| --- | --- |
| M4 导航 | `database` 内部构造 `PropertyRequest`，宿主桥打开面板 |
| Quick Open | `ShowProperties` 命令（`quick_open/model.rs`），已接，类型不变 |
| Mock / 洞察 | 未来消费 `property::ObjectProperties` / `property::PropertyKind`（公开边界已就绪） |

## 12. 测试

- `rds-property`：kind 覆盖 / provider 契约（`ProviderInput.candidates` 查表、registry 分派）/ `PropertyState` 默认值。
- `rds-database`：`property_ref_of` 映射（kind 全量）、`load_properties` 各类型分区、列表 / 索引 / 约束表渲染、
  `table_data` 分页抓取（quote_ident 转义 / has_more 探测 / COUNT / 排序补齐）。
- `rds-workbench`：Quick Open `ShowProperties` 构造、`apply_props_results` 过期 key 丢弃、
  `nav_property_real` 真机全链路（SQLite / DuckDB：导航 → 属性 → 数据页分页 / 排序 / NULL / 视图数据）。
- 窗口级（`#[gpui_kit::test]`）：tab 切换渲染、空 sections 兜底、数据页懒加载。

## 13. 已知问题与限制

1. `PropertyRef` / `PropertyRequest` 仍留在 `database::model`（依赖 `NavSource`）；若未来出现
   非导航域的属性请求方，再评估把定位参数下沉（引入 `SourceScope` 与转换点）。
2. 未实现能力（权限 / 依赖 / 触发器 / 规则 / 分区 / 存储 / 事件触发器）不产出分区、
   **不发起任何内省查询**（能力未实现，不做假数据）；接线位在 `DriverCapabilities`。
3. `ObjectProperties.sections` 的 id 与描述模型 `PageId` 是一一映射；新增分区类型需同时
   扩展 `PageId` 与注册表（渲染不动）。
4. 列表格「引用」列语义（`表.列` / `FK` / 空）与列序号回退逻辑是既有行为，抽取不改语义。
5. 「数据」页当前为**只读预览**（分页 + 排序）；编辑回写（行身份 / 事务 / 暂存 / 应用）待驱动
   `table_editor` 能力位声明后另立模块（`docs/architecture/editor/editor-table-editing-plan.md`）。
6. 数据行数用真 `COUNT(*)` 单独一问：大表上会多花一次查询；取不到时如实显示「行数未知」，
   不编 0。分页 `has_more` 用 `limit + 1` 探测，不为此多跑 COUNT。
7. 视图 / 无主键表无排序时如实显示「本页无定义顺序」（分页不稳定），用户按列排序后恢复稳定。

## 14. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| `PropertyKind` / 数据容器 / `PropertyState` / 数据页模型 | `crates/property/src/model.rs` |
| 描述模型 + `PropertyRegistry` trait（含 `PageId::Data`） | `crates/property/src/descriptor.rs` |
| 属性源接口 + 注册表（对齐 `DBPPropertySource`，§16） | `crates/property/src/provider.rs` |
| crate 公共 API | `crates/property/src/lib.rs` |
| `load_properties` 取数（通用骨架，§16 重构） | `crates/database/src/property_panel.rs` |
| 属性源注册表实现（各驱动 provider） | `crates/database/src/property_providers/{mod,pg,sqlite,duckdb,mysql}.rs` |
| `PropertyRef` / `PropertyRequest` / `property_ref_of`（保留） | `crates/database/src/model.rs` |
| 属性加载入队 / 结果类型 | `crates/database/src/nav_jobs.rs` |
| tab 化描述驱动渲染（三部分布局，§16） | `crates/workbench/src/panels/property_panel.rs::render_property_panel`（`PropertyHostPanel`，中央 Dock 独立 tab） |
| 属性面板中央 tab 装配 / 聚焦（`open_property` 回调，2026-10-09） | `crates/workbench/src/view.rs::init_workspace` + `crates/workbench/src/panels/shared.rs` |
| Quick Open 打开属性 | `crates/workbench/src/quick_open/model.rs`（`ShowProperties`，类型不变） |
| 属性行描述符化 + 编辑控件渲染（网格单元，§16） | `crates/workbench/src/panels/property_panel.rs::render_property_cell` |
| 属性编辑提交（`setPropertyValue` 对齐；SQL 由 provider 生成） | `crates/database/src/property_panel.rs::apply_property_edit` |
| 列编辑 SQL 生成（PG） | `crates/database/src/sql_gen.rs::alter_column_sql` |
| DBeaver 源码参考（本地检出） | `D:\RdataStation\RDS\dbeaver-src\plugins\org.jkiss.dbeaver.model\src\org\jkiss\dbeaver\model\preferences\{DBPPropertySource,DBPPropertyDescriptor,DBPPropertyManager}.java` |

## 15. DBeaver 属性功能复现（2026-10-09）

> 目标：属性面板的**显示逻辑与操作逻辑向 DBeaver 对齐**（用户拍板「复现实现」）。
> 以 DBeaver 源码（`DBPPropertySource` / `DBPPropertyDescriptor` / `@Property` 注解驱动）为基准，
> 对照落地见下表；**只读展示先行、编辑最小集落地**，MySQL 等驱动扩展留待下一步。

### 15.1 对照（DBeaver → RDS）

| DBeaver | RDS 复现 | 状态 |
| --- | --- | --- |
| `@Property(viewable, editable, order)` 注解 getter → 描述符 | `PropertyRow` 描述符化（`id` / `category` / `editable` / `value_kind`） | 已落地 |
| `getPropertyValue(monitor, id)` 懒加载值 | 提供者一次取数填充 `value`（懒加载按需后续引入） | 部分 |
| `category` 分组显示 | `PropertyRow.category`（渲染层按组渲染；本轮提供者全 None → 默认组） | 结构就绪 |
| `isEditable(object)` → 编辑控件 | `PropertyRow.editable` → Switch / 行内输入 + 保存 | 已落地 |
| `setPropertyValue(monitor, id, value)` → SQL | `apply_property_edit` → `alter_column_sql`（PG 列级） | 已落地 |
| `resetPropertyValue` 重置 | 未实现（后续） | 待办 |
| 值类型 `dataType` | `PropertyValueKind`（Text / LongText / Boolean / Select） | 已落地 |

### 15.2 数据模型升级

`PropertyRow` 由「label / value」升级为描述符形态（`crates/property/src/model.rs`）：

```rust
pub struct PropertyRow {
    pub id: String,            // DBeaver getPropertyValue(monitor, id) 的 id；编辑提交按 id 映射 SQL
    pub label: String,         // displayName
    pub category: Option<String>, // 分组；None = 默认组
    pub value: String,
    pub editable: bool,        // isEditable(object)
    pub value_kind: PropertyValueKind, // 决定编辑控件形态
    pub select_options: Vec<String>,   // Select 候选
}
```

提供者新增 `editable_row(id, label, value, kind, options)` 辅助（`property_panel.rs`）：
只读行用 `PropertyRow::read_only`（editable=false、Text 形态）——旧 `row()` 语义不变。

### 15.3 渲染协议（General 页）

`render_property_row`（`workbench/panels/property_panel.rs`）按形态分派：

- **只读**（editable=false）→ 名称 + 文本（原渲染不变）；
- **Boolean** → `Switch`（点击即切换提交，DBeaver「编辑即生效」）；
- **Text / LongText** → 未编辑时「值 + 编辑按钮」；编辑中 → 行内 `Input` + 保存 / 取消。

编辑态由 `PropertyHostPanel.property_edit`（`Option<PropertyEdit{row_id, input}>`）承载，
创建在事件路径（`begin_property_edit`，`cx.new(InputState)`）——渲染零 I/O、零实体创建。

### 15.4 编辑链路（setPropertyValue 对齐）

```
点击编辑 → begin_property_edit（行内输入）
  → submit_property_edit：读 InputState.value()
  → run_property_edit：background_executor 执行 apply_property_edit（async）
  → 成功：request_properties 重载面板；失败：PropertyState.error 面板内红字
```

`apply_property_edit`（`database/property_panel.rs`）仅接受 `PropertyKind::Column`，
其余对象类型明确报「暂不支持」（不静默吞）。

### 15.5 列编辑 SQL 生成（驱动分派）

`sql_gen::alter_column_sql(driver, schema, table, column, property_id, value)`：

| 属性 id | PG SQL | 状态 |
| --- | --- | --- |
| `Column.name` | `ALTER TABLE … RENAME COLUMN "a" TO "b"` | 支持 |
| `Column.comment` | `COMMENT ON COLUMN … IS '…'`（单引号转义） | 支持 |
| `Column.default_value` | `SET DEFAULT …` / 空串 = `DROP DEFAULT` | 支持 |
| `Column.nullable` | `SET / DROP NOT NULL`（value `true` = 允许 NULL） | 支持 |
| 其余属性 | 明确报「暂不支持」 | — |

**驱动支持矩阵**：PostgreSQL 完整；MySQL 注释需 `MODIFY COLUMN` 完整列定义（待扩展）；
SQLite / DuckDB 无 ALTER 语义等价物，明确报错。

### 15.6 已知限制 / 下一步

1. 编辑成功后**导航树列节点未自动刷新**（重命名 / 可空变更需手动刷新或等树刷新事件接入）；
2. `resetPropertyValue`（重置为默认）未实现；
3. MySQL 列注释 / 默认值编辑未实现（需驱动侧 MODIFY 支持）；
4. ~~表级属性（注释 / 描述）编辑未实现（列级先行）~~ → 已实现：表/Schema/视图 `description`
   接 `COMMENT ON`、表级行级安全接 `ALTER TABLE … ENABLE|DISABLE ROW LEVEL SECURITY`（2026-10-15，见 §16.5.6）；
5. ~~Select 形态控件（`select_options`）渲染未接（当前提供者未产生 Select 行）~~ → 已接入：
   常驻下拉选中即提交（2026-10-15，见 §16.5）。

## 16. 属性源接口化 + 三部分布局（2026-10-09 实现）

> 目标（用户拍板）：**属性面板可重建，不受旧布局约束；导航栏最右侧显示属性可同步修改**；
> 核心原则是**动态渲染、驱动无关**——新增数据库（Oracle 等）**不需要动面板代码**。
> 实现基准 = DBeaver 插件机制（`DBPPropertySource` 接口 + 各 `ext.*` 插件实现），
> 原型 = `docs/architecture/property/database-properties-prototype.html`（三视图 + 四驱动动态演示）。

### 16.1 属性源接口（对齐 DBPPropertySource）

DBeaver 动态渲染核心（源码实证）：属性面板 UI 只依赖 `DBPPropertySource` 接口，
各数据库插件用 `@Property` 注解 getter 实现它——**新增数据库 = 新插件实现接口，UI 零分支**。

RDS 对齐 = `crates/property/src/provider.rs`：

```rust
pub trait PropertyProvider: Send + Sync {
    fn db_type(&self) -> &'static str;                       // 数据库类型 id（drivers.type_id：postgresql/sqlite/…）
    fn kind(&self) -> PropertyKind;                         // 每驱动族 × 对象类型一个 provider
    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow>;   // getProperties()
    fn overview(&self, input: &ProviderInput) -> ObjectOverview;          // RDS 新增：洞察总览
    fn categories(&self, input: &ProviderInput) -> Vec<PropertySection>; // 子项分区（能力掩码）
    fn extra_queries(&self, input: &ProviderInput) -> Vec<ExtraQuery>;    // 懒加载统计（key + SQL 模板）
    fn apply_edit_sql(&self, input, row_id, value) -> Result<String, String>; // setPropertyValue → SQL
}
```

- **分派键 = 数据库类型 id（`drivers.type_id`）**，不是驱动 id / 展示名：同一数据库族下的
  多驱动实现（如 `postgres_native` / JDBC）共享一套 provider。`PropertyRequest.type_id`
  由视图层透传、宿主（`EditorPanel::request_properties`）从驱动目录补齐（`driver_catalog`
  驱动 id → `DriverMeta.type_id`）；缺失时回退驱动 id（旧调用方 / 测试直接传类型名）。
- `ProviderInput` 携带：目标引用（`PropertyTargetRef`）、连接显示名、数据库类型 id（分派键）与
  展示名、通用骨架统计（`ProviderStats`：列/索引/约束计数）、调用方统计（`counts`：版本/路径/oid/行估计…）、
  附加对象（`detail`：列明细等，provider 自行 downcast）。
- `ProviderRegistry`（全局单例 `database::property_providers::global_registry()`）按
  类型 id × 对象类型解析；比较大小写不敏感 + 族别名归一（`postgres|pg → postgresql`）。

### 16.2 实现分层（新增驱动不改面板/骨架）

| 层 | 内容 | 位置 | 新增驱动要改？ |
| --- | --- | --- | --- |
| 模型/契约 | `PropertyRow` / `ObjectOverview` / `PropertyCategory` / `ProviderRegistry` | `crates/property/src/{model,descriptor,provider}.rs` | 否 |
| 通用骨架 | 列/索引/约束/DDL/数据取数 + 类别内容合并 + 编辑执行 | `crates/database/src/property_panel.rs` | 否 |
| 驱动 provider | 属性行 / 洞察 / 类别 / 懒加载统计 / 编辑 SQL | `crates/database/src/property_providers/`（pg/sqlite/duckdb/mysql.rs） | **是（只加一个文件 + register 一行）** |
| 渲染层 | 三部分布局 / 编辑控件 / 数据页 | `crates/workbench/src/panels/property_panel.rs`（`PropertyHostPanel`，中央 Dock 独立 tab） | 否 |

**已注册 provider**（`property_providers/mod.rs::register_all`）：
PostgreSQL 8 类（连接/Schema/表/视图/列/例程/序列/触发器）；SQLite / DuckDB 各 3 类
（连接/表/视图）；MySQL 2 类（连接/表）。**Oracle 暂不接入**（壳 provider 已移除——
后续接入 = 驱动内省 + 注册文件，面板/骨架零改动）。
未注册对象类型回退通用展示（`property_panel::fallback_*`）。

### 16.3 三部分布局（原型验收基线）

| 部分 | 内容 | 数据来源 |
| --- | --- | --- |
| ① 洞察总览 | 状态点 + 标题 + meta 芯片（驱动/作用域/分组/标签）+ 指标卡 + 关键信息 + 口径注脚 | `ObjectOverview`（provider 组装；cards 用 stats/counts） |
| ② General 双栏网格 | 定义属性；行数 > 8 自动三栏（面积使用率） | `properties`（provider） |
| ③ 子项类别列表 | 类别纵向堆叠：可用 → 标题+表头+行；无类别对象 → 「（无）」；**DDL / 源码分区 = 等宽代码块 + 复制按钮**（2026-10-24） | `sections`（categories 顺序 + 骨架内容合并；未实现类别隐藏式不产出，2026-10-24） |

- **表 / 视图两页签【属性 | 数据】**（去掉旧「图」页签；数据页懒加载，首屏 200 行）；
- 其余对象为单属性视图（无页签条）；
- 未实现类别（权限/依赖/触发器/规则/策略/分区/Statistics）隐藏式不产出，不发内省查询；
  接线位在 `DriverCapabilities`（`Some(false)`），接入时翻 `true` 自动出现；
- 编辑控件：Boolean → Switch（即改即提交）；Select → 常驻下拉（`use_keyed_state` 惰性创建，
  选中即提交，2026-10-15 接入；空候选回退文本）；Text/LongText → 值 + 编辑按钮 → 行内输入 + 保存/取消。

### 16.4 数据流（事件路径，渲染零 I/O）

```
双击导航对象 → request_properties（事件路径置 loading + 入队）
   → nav_jobs 后台 load_properties（通用骨架取数 + provider 分派）
   → apply_props_results 回填 PropertyState → render 三部分
切「数据」页签 → 懒加载 TableDataRequest::default()（200 行）
编辑行 → begin_property_edit → submit → run_property_edit（provider.apply_edit_sql + execute_query）
   → 成功重载面板 / 失败 PropertyState.error 面板内红字
```

### 16.5 已知限制 / 下一步（相对 15.6 的新增项）

1. `counts` 已由调用方填充（2026-10-09；**2026-10-24 连接对象扩 P0-3**）：连接级
   （`connected`/`server_version`/`version`/`url`/`description`/`auth_method`/`auth_ref`/
   `env_ref`/`network_ref` 来自 `ConnectionInfo`）+ provider 声明的懒加载查询
   （`extra_queries`：PG 表级 `row_estimate`/`size`，取首行首列入洞察指标卡）；
   其余驱动 / 对象的统计随内省逐步接入；
2. 连接级属性编辑（描述/管理员/连接限制/允许连接）待 settings 接入——**当前渲染为只读占位**，
   值标注「待接入」（避免"可点但提交报错"），编辑链路 `apply_property_edit` 已按类型 id 分派；
   连接对象常规行已接 `ConnectionInfo` 真值（描述 / 认证方法+引用 ID / 环境引用 / 网络引用 /
   连接串，引用字段只读，2026-10-24 P0-3）；
3. `PropertySection`（原 `PropertyCategory`）的「左栏子标签 + 右栏内容」DBeaver 形态已落地
   （多分区时左栏竖排子标签 + 右栏表格；单分区退化为直接堆叠）；
4. **Oracle 暂不接入**（驱动内省未就绪；壳 provider 已移除，后续按「注册 provider」机制接入）；
5. 旧版属性面板（右侧停靠 + 页集合 tab）代码已重建，未删旧物（原型与文档保留供对照）；
6. 对象级编辑补全（2026-10-15）：`Table.description`→`COMMENT ON TABLE`、
   `Schema.description`→`COMMENT ON SCHEMA`、`View.description`→`COMMENT ON VIEW`
   （单引号双写转义）；`Table.row_level_security`→`ALTER TABLE … ENABLE|DISABLE ROW
   LEVEL SECURITY`（`relrowsecurity` 真值经 `extra_queries` 懒加载，Switch 初始态真实，
   避免用户基于假开关误提交）；`Table.partition_by` / `Table.reloptions` 无直接 ALTER
   语法 → **降为只读行**，杜绝"可点但提交报错"；
7. Schema 子项类别修正（2026-10-15）：删除旧实现把「表→Columns」「视图→Constraints」
   「外部表→Indexes」「物化视图→Ddl」「索引→Source」的错位 PageId 灰置声明（类别 id
   是 provider 增删时的稳定业务键，错位会让未来启用时内容进错分支）；表/视图/函数/
   序列等 Schema 子项类别待新 PageId + 内省查询接入后再声明。
8. **「保存凭据」开关（2026-10-24，P0-4，对齐 DBeaver `setSavePassword(true)`）**：
   连接对话框认证卡尾部新增 `Switch`；**关 = 密码不落库**（仅本次会话可用、下次需重输），
   开 = 密码 AES-256-GCM 加密后落库（原行为）。语义边界：URL 内用户显式放置的凭据
   **不剥离**（避免隐式篡改）；引用认证档案时开关**置灰**（密文在档案里，开关无意义）。
   链路：`GlobalConnectionSaveInput.save_password`（engine，必填）→
   `SaveGlobalConnectionInput.save_password`（connection_service 透传）→
   `DataSourceSaveInput.save_password: Option<bool>`（新字段，`None` 兼容旧调用，
   `project_connection_from_input` 以 `unwrap_or(true)` 守卫）→
   `ConnectionDialogState.save_credentials`（默认 true）+ 草稿 `ConnectionDraft.save_credentials`
   往返（草稿行无此列，回读默认 true）。
9. **连接对象属性分节（2026-10-25，P0-3 深化）**：认证 / 网络 / 环境从常规行拆为
   `PageId::{Auth,Network,Environment}` 三独立分节（`connection_meta_sections`），
   引用字段语义对齐 DBeaver 连接属性——显示来源**名称**（经 engine 全局单例
   `get_global_db_manager()` 解析 `AuthConfig.name` / `NetworkConfig.name` /
   `Environment.name`，解析失败回落引用 ID）且**只读**；有对应 counts 才产出分节
   （无档案的密码认证只显示「认证方法」一行）。环境分节额外带**标识色**（color 非空时）
   与**策略覆盖**标记（`list_environment_policies` 过滤 enabled 后 join）。
   常规行 `connection_base_rows` 移除原认证/环境/网络三组行（改分节承载）。

### 16.6 实现位置映射（本轮新增/变更 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| `PropertyProvider` trait / `ProviderRegistry` / `ProviderInput` / `ObjectOverview` / `PropertySection` | `crates/property/src/provider.rs` |
| `ObjectProperties.overview` / `PropertySection` / `PageId::{Routines,Partitions,Trigger,Storage,EventTrigger}` | `crates/property/src/{model,descriptor}.rs` |
| 全局注册表 + 各驱动 provider（pg/sqlite/duckdb/mysql） | `crates/database/src/property_providers/{mod,pg,sqlite,duckdb,mysql}.rs` |
| 通用骨架（取数 + provider 分派 + 类别合并 + fallback + 编辑执行） | `crates/database/src/property_panel.rs` |
| 三部分渲染 / 双栏网格 / 子项类别（隐藏式不产出）/ 两页签 / 编辑控件 | `crates/workbench/src/panels/property_panel.rs::{render_property_panel, render_property_view, render_overview_section, render_general_grid, render_category_list, render_category_section, render_property_cell}`（`PropertyHostPanel`，中央 Dock 独立 tab，2026-10-09） |
| 真机链路测试（SQLite/DuckDB 全链路 + provider 类别断言） | `crates/workbench/tests/nav_property_real.rs` |
| 「保存凭据」开关（P0-4，2026-10-24） | `crates/engine/src/persistence/global_db.rs`（`GlobalConnectionSaveInput.save_password` + `final_password` 分支）、`crates/connection_dialog/src/services/{connection_service,data_source_service}.rs`、`crates/connection/src/model.rs`（`DataSourceSaveInput.save_password`）、`crates/connection_dialog/src/components/connection_dialog/{mod.rs,state.rs,render.rs,staging.rs}` |
| 连接属性分节 认证/网络/环境（P0-3 深化，2026-10-25） | `crates/property/src/descriptor.rs`（`PageId::{Auth,Network,Environment}`）、`crates/database/src/property_panel.rs`（counts 解析 auth_name/auth_type/network_name/network_type/env_name/env_color/env_policies）、`crates/database/src/property_providers/mod.rs`（`connection_categories` 双参 + `connection_meta_sections`） |

## 17. 属性面板卡片规范（2026-10-09，连接编辑器 / 洞察 / Mock 同规范）

> 目标：属性面板不再「一列长卡堆叠」，大区块统一为**卡片语言**，小区块（芯片 / 标签 / 表头）
> 也按同一套 token 走。**该规范是产品语义，不限于属性面板**——连接编辑器（`connection_dialog`）、
> 洞察（M8 `insight`）、Mock（M7）新增区块时按本规范做，勿另起一套视觉。

### 17.1 大区块（卡片）

| 区块 | 容器 | 头部 | 内容 |
| --- | --- | --- | --- |
| 洞察卡 | `border_1 + rounded_md + bg(sidebar)`，内距 `p_2` | 标题 `text_xs` 中字重 `fg` + 状态点 / 副标题 | meta 芯片（`border + rounded_md + px_2 py_px`）、指标卡（`bg(background)` 提亮）、kv 键值行 |
| General 卡 | 同上 | `card_header("通用", 尾注)` | 双栏 / 三栏网格（行数 > 8 三栏），单元格 = muted 标签 + fg 值 / 编辑控件 |
| 子项卡 | 同上 | `card_header("子项")` | 左栏子标签（选中 `list_active` + 2px 激活条 + 尾注计数）+ 右栏内容（`bg(background)` 内层表）；sections 为空 → 「（无）」 |
| 数据卡 | 同上 | `card_header("数据")` | 行号 + 列名表头（排序可点）+ 数据行 + 分页脚 |

统一实现点：`PropertyHostPanel::card_header(title, note, theme)`（标题 + 右对齐尾注）；
容器五件套 = `v_flex gap_2 p_2 border_1 border_color(border) rounded_md bg(sidebar)`。

### 17.2 小区块（内部元素）

- **meta 芯片**：`border_1 + rounded_md + px_2 + py_px + text_xs`，标签 `muted`、值 `fg`。
- **指标卡**：`flex_1 + min_w + p_2 + border_1 + rounded_md + bg(background)`，数字 `text_lg` 粗体。
- **表头行**：`text_xs muted` + `border_b_1`（子项表与数据页一致）。
- **数据行**：`text_xs fg`，`NULL` 一律 `muted`；分页脚 `muted + 语义按钮`（gpui-kit Button::Small）。
- **左栏子标签**：行高 1.75rem（`h(rems(1.75))`），`px_2 + rounded_md`；选中态用 `tree::active_bar`（与导航树同原语）。

### 17.3 分层与用色

- 卡片底 = `sidebar`；卡片内层区块（指标卡 / 子项右栏表）= `background`（在侧栏底上提亮一级）；
- 不另起边框色：全部 `border` token；状态色（成功 / 危险 / 警告）只用于洞察状态点与语义文案；
- 零裸色值 / 零裸 px（`ui_contract` 已扫描本文件）。

