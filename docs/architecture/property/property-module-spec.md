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
| 一句话 | 把「一个数据库对象长什么样、能看哪些属性」做成**描述驱动**的可复用面板：对象类型 → 属性页集合（注册表）→ 动态渲染；暂不支持的能力（如权限）**先绘制、置灰标注**，不做硬编码分支 |
| 交付状态 | 抽取 + 重设计（2026-10-03 起）；抽取后 `crates/property` 独立 crate，`database` 仅保留取数实现 |
| 权威文档 | 本文件（§13 = 已知问题唯一权威） |
| 主要文件 | 见 §4 代码地图 |
| 测试基线 | rds-property 单测（模型 / 描述模型 / 注册表契约）；rds-database / rds-workbench 回归全绿 |

### 1.1 模块边界（做什么 / 不做什么）

**做**：属性面板领域模型（`PropertyKind`）、数据容器（`ObjectProperties` / `PropertyRow` /
`PropertyTable` / `PropertySection`）、面板状态（`PropertyState`）、描述模型（`PropertyPage` /
`PropertyEntry` / `PageId` / 支持状态）、注册表契约（`PropertyRegistry` trait）。
**不做**：数据库对象树导航（M4 `nav_view`，仍属 `database`）；属性面板的停靠容器与宽度记忆
（`workbench` `EditorPanel` 右 Dock，属布局）；元数据实时内省与驱动（`engine`）；对象定位参数
（`PropertyRef` / `PropertyRequest`，留在 `database::model` —— 它们是**导航域 → 属性面板**的输入契约，
依赖 `NavSource`）。

## 2. 技术栈与依赖

| 项 | 内容 |
| --- | --- |
| 序列化 | serde（`PropertyKind` 参与 `PropertyRef` 序列化） |
| 数据容器 | 纯 Rust 结构体（`String` / `Vec`），无运行时依赖 |
| 注册表契约 | `trait PropertyRegistry`（无默认实现，由数据生产方实现） |
| 异步 | 不涉及（取数在 `database`，入队 / 轮询在 `nav_jobs` / `EditorPanel`） |
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
| `src/descriptor.rs` | 描述模型：`PageId` / `PropertyPage` / `PropertyEntry` / `SupportState` / `PropertyRegistry` trait |
| `src/tests.rs` | 单测（kind 覆盖、描述模型、状态默认值） |

### 4.2 `crates/database/`（保留取数实现）

| 文件 | 职责（改动后） |
| --- | --- |
| `src/model.rs` | 删除 `PropertyKind` 定义（改 `use property::PropertyKind`）；`PropertyRef` / `PropertyRequest` / `property_ref_of` 保留（字段类型改用 `property::PropertyKind`） |
| `src/property_panel.rs` | 删除数据容器定义（改 `use property::…`）；保留 `load_properties`（取数，返回 `property::ObjectProperties`）；新增 `PropertyRegistryImpl`（实现 `property::PropertyRegistry`，注册各类型页集合 + 支持状态） |
| `src/nav_jobs.rs` | `PropsResult` 改用 `property::ObjectProperties`；`enqueue_properties` 不变 |

### 4.3 `crates/workbench/`（消费与渲染）

| 文件 | 职责（改动后） |
| --- | --- |
| `src/panels/editor.rs` | `render_property_panel` 改为**描述驱动 tab 渲染**（见 §10）；`PropertyState` 增加 `active_page` |
| `src/quick_open/model.rs` | `PropertyRequest` 类型不变；tests 中 `PropertyKind` 引用改 `property::PropertyKind` |

## 5. 公开接口

`lib.rs` 导出：

```text
pub mod descriptor;   // 描述模型 + PropertyRegistry trait
pub mod model;        // PropertyKind / ObjectProperties / PropertyRow / PropertyTable / PropertySection / PropertyState

pub use descriptor::{PageId, PropertyEntry, PropertyPage, PropertyRegistry, SupportState};
pub use model::{ObjectProperties, PropertyKind, PropertyRow, PropertySection, PropertyState, PropertyTable};
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
}
```

`PropertySection` 增加 `id: PageId` 字段（供描述模型映射「页 → 内容」）。

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

## 7. 描述模型与能力掩码

```text
/// 属性页 id（页 → 内容映射键，也是 tab 标识）。
pub enum PageId {
    General,       // 属性网格
    Columns,       // 列
    Indexes,       // 索引
    Constraints,   // 约束
    Ddl,           // DDL（源版 / 合成）
    Source,        // 例程序源码
    Privileges,    // 权限（当前不支持 → 置灰）
    Dependencies,  // 依赖（当前不支持 → 置灰）
}

/// 支持状态：驱动 / 类型能力位决定。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SupportState {
    Supported,      // 可渲染实际内容
    Unsupported,    // 置灰渲染（内容区显示「暂不可用」占位）
}

/// 一个属性页的描述。
pub struct PropertyPage {
    pub id: PageId,
    pub title: String,
    pub state: SupportState,
}

/// 属性网格中的一个条目（可扩展：editable 预留编辑能力）。
pub struct PropertyEntry {
    pub label: String,
    pub value: String,
    pub state: SupportState,
    pub editable: bool,
}

/// 对象类型 → 属性页集合的注册表契约（数据生产方实现）。
pub trait PropertyRegistry {
    /// 返回某类型对象的页集合（顺序即 tab 顺序）。
    fn pages_for(&self, kind: PropertyKind) -> Vec<PropertyPage>;
}
```

**动态渲染原则**：渲染层只遍历 `registry.pages_for(kind)` + `ObjectProperties`，
不按 `kind` 写分支；新增对象类型 / 新增属性 = 注册表加条目，渲染零改动。

## 8. 权限页灰置规格（示例）

- `PropertyRegistry::pages_for` 对 `Table` / `View` / `Connection` 注册 `Privileges` 页，`state = Unsupported`
  （`engine::MetadataBrowser` 暂无 `get_privileges` 内省实现）。
- 渲染：tab 条上该页以 muted 色呈现（可点，点了内容区显示「权限查看与授权暂不支持（驱动能力未实现）」）。
- 将来驱动补上 `get_privileges` 能力位后，注册表把该页 `state` 翻为 `Supported`，渲染零改动自动激活。
- `Dependencies`（依赖 / 被引用）页同理先灰置。

## 9. 数据流

```text
导航双击 / Quick Open ShowProperties
  → PropertyRequest（database::model，含 PropertyRef + conn_label + driver）
  → EditorBridge::show_properties → EditorPanel::request_properties
  → nav_jobs::enqueue_properties（后台单工作线程）
  → database::load_properties（MetadataService 实时内省）
  → property::ObjectProperties（回填 PropertyState，key 不匹配的过期结果丢弃）
  → EditorPanel::render_property_panel：registry.pages_for(kind) 逐页渲染
```

## 10. 渲染协议（tab 化）

1. 顶部 tab 条：`registry.pages_for(kind)` 的页集合（gpui-kit TabBar），当前页 = `PropertyState.active_page`。
2. 当前页内容按 `PageId` 从 `ObjectProperties` 取：
   - `General` → `properties` 网格；
   - `Columns` / `Indexes` / `Constraints` → `sections` 中 `id` 匹配的分区表格；
   - `Ddl` / `Source` → `sections` 中对应分区（多行文本单列表）；
   - `Privileges` / `Dependencies` 且 `state = Unsupported` → 灰置占位内容；
   - 找不到对应 section 且 `state = Supported` → 空态文案（不报错）。
3. 宽高沿用右 Dock（280px 基准，宽度可拖拽记忆），行内无裸 px / 裸色值（走 `workbench_shell::ui` 与主题 token）。

## 11. 使用方

| 使用方 | 接入方式 |
| --- | --- |
| M4 导航 | `database` 内部构造 `PropertyRequest`，宿主桥打开面板 |
| Quick Open | `ShowProperties` 命令（`quick_open/model.rs`），已接，类型不变 |
| Mock / 洞察 | 未来消费 `property::ObjectProperties` / `property::PropertyKind`（公开边界已就绪） |

## 12. 测试

- `rds-property`：kind 覆盖 / 描述模型构造 / `PropertyState` 默认值 / 注册表契约（mock registry）。
- `rds-database`：`property_ref_of` 映射（kind 全量）、`load_properties` 各类型分区、列表 / 索引 / 约束表渲染。
- `rds-workbench`：Quick Open `ShowProperties` 构造、`apply_props_results` 过期 key 丢弃。
- 窗口级（`#[gpui_kit::test]`）：tab 切换渲染、灰置页占位。

## 13. 已知问题与限制

1. `PropertyRef` / `PropertyRequest` 仍留在 `database::model`（依赖 `NavSource`）；若未来出现
   非导航域的属性请求方，再评估把定位参数下沉（引入 `SourceScope` 与转换点）。
2. 权限 / 依赖页为灰置占位，**不发起任何内省查询**（能力未实现，不做假数据）。
3. `ObjectProperties.sections` 的 id 与描述模型 `PageId` 是一一映射；新增分区类型需同时
   扩展 `PageId` 与注册表（渲染不动）。
4. 列表格「引用」列语义（`表.列` / `FK` / 空）与列序号回退逻辑是既有行为，抽取不改语义。

## 14. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| `PropertyKind` / 数据容器 / `PropertyState` | `crates/property/src/model.rs` |
| 描述模型 + `PropertyRegistry` trait | `crates/property/src/descriptor.rs` |
| crate 公共 API | `crates/property/src/lib.rs` |
| `load_properties` 取数（保留） | `crates/database/src/property_panel.rs` |
| 注册表实现（各类型页集合 + 支持状态） | `crates/database/src/property_panel.rs`（`PropertyRegistryImpl`） |
| `PropertyRef` / `PropertyRequest` / `property_ref_of`（保留） | `crates/database/src/model.rs` |
| 属性加载入队 / 结果类型 | `crates/database/src/nav_jobs.rs` |
| tab 化描述驱动渲染 | `crates/workbench/src/panels/editor.rs::render_property_panel` |
| Quick Open 打开属性 | `crates/workbench/src/quick_open/model.rs`（`ShowProperties`，类型不变） |
