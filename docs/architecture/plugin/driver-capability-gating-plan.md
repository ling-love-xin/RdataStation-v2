# 驱动能力位 × 属性分区置灰（方案 · 待评审）

> **一句话**：同一数据库有两个驱动实现时，能力差异要**如实表达成 UI**——驱动不支持的属性分区**置灰并写明原因**，既不显示「索引 (0)」，也不让它悄悄消失。
>
> **状态**：方案稿，待评审；**不改代码**。评审通过后并入 `docs/architecture/property/` 的决策编号体系。
>
> **相关**：`sqlx-driver-extraction-plan.md`（本方案是它的前置）、`driver-operation-audit-plan.md`（操作留痕）、`../driver-capability-matrix.md`（能力矩阵现状）。

---

## 1. 验收口径（用户已定）

| # | 口径 | 说明 |
| --- | --- | --- |
| 1 | 功能面**对齐 DBeaver** | 只做 DBeaver 有的功能，不额外发明 |
| 2 | 单驱动不支持 → **对应属性模块置灰** | 保留页签、整块灰化 + 一行原因；不做事后静默降级 |
| 3 | 不依赖「官方驱动一定比 sqlx 强」这个前提 | 谁缺哪块就灰谁，两个方向都成立 |

这与插件模块既有硬约束一致：`plugin/README.md` §5 #2「**能力必须门控**，未声明的调用返回可读原因，不允许『静默退化』」。

**边界（与 2026-10-24 决策的关系）**：`82c9cd46` 拍板「**隐藏式**」——未实现类别不产出页签，灰置渲染分支与 `enabled` 字段被清理。本方案**不推翻它**，只切一刀：

- **产品未实现**（能力字典 `Stage::NotBuilt`，如 `index_analysis` / `table_editor`）→ 继续**隐藏**（避免假承诺）；
- **产品已实现、但当前驱动给不出**（如 sidecar 无索引明细、SQLite 无序列）→ **置灰 + 原因**（本方案新增的语义）。

---

## 2. 现状（代码实证）

| # | 位置 | 事实 | 后果 |
| --- | --- | --- | --- |
| 1 | `database/src/property_panel.rs:131-144` | `list_indexes(...).unwrap_or_default()` / `list_constraints(...).unwrap_or_default()` / `get_table_ddl(...).ok().flatten()` | `Err(NotSupported)` 被吞成 `Ok(vec![])` → 显示「索引 (0)」「约束 (0)」，DDL 静默改走合成 |
| 2 | `database/src/property_providers/mod.rs:261-300` | 表级核心页（列 / 索引 / 约束 / DDL / 数据）**无条件产出**，三条元数据页没有任何能力位 | 索引/约束没有"该驱动不支持"的表达位置 |
| 3 | `database/src/property_providers/mod.rs:334-348`、`:259-260` | `Some(false)` = **不产出**（隐藏式）；渲染层已无灰置分支 | 要置灰就得改回模型（见 §4.3） |
| 4 | `property/src/model.rs:85` | `PropertySection { id, label, table }` 只有三个字段 | 没有"不可用 / 原因"位 |
| 5 | `driver/src/native/mod.rs` 头注 | trait 默认实现就是空，「未支持」与「真的没有」分不清 | 三态信息在 trait 层就丢了 |
| 6 | `plugin/src/sidecar/driver.rs`（`no_index_detail` / `list_indexes` / `list_constraints`） | sidecar **报 `Err`**；`get_table_ddl` 协议无方法；列 `references` / `ordinal` / `is_identity` / `collation` 缺失 | 唯一"老实"的驱动反而被 UI 吞掉，其余静默 |
| 7 | `driver/src/capability.rs:155-285` | 15 个键里 `index_analysis` / `table_editor` = `Stage::NotBuilt`；`export` / `mock` / `resource` = `Scope::App` | 现有字典是**产品功能键**，没有"索引明细 / 约束明细 / 源版 DDL / 外键目标"这一层 |

**结论**：`drivers.capabilities` 这条通道是通的（内核 descriptors 与插件 `[[contributes.drivers]] features` 都落到同一列），缺的是 **① 元数据面能力位 ② 三态取数 ③ 模型里的不可用态 ④ 渲染分支**。

---

## 3. DBeaver 对照（本地源码实证）

`D:\RdataStation\RDS\dbeaver-ref`：

1. **能力位统一挂在数据源信息上**：`plugins/org.jkiss.dbeaver.model/src/org/jkiss/dbeaver/model/DBPDataSourceInfo.java` 共 18 个 boolean，与本方案相关的是：

| DBeaver 位 | 语义 | 对应我们哪个分区 |
| --- | --- | --- |
| `supportsIndexes()` | 能否读索引 | 索引 |
| `supportsReferentialIntegrity()` | 能否读约束 / 引用完整性 | 约束 |
| `supportsStoredCode()` | 能否读例程 / 存储代码 | 例程源码（`PageId::Source`） |
| `supportsTransactions()` / `supportsTransactionsForDDL()` / `supportsSavepoints()` | 事务层次 | 编辑器事务开关（已由能力键 `transactions` 承担） |
| `supportsBatchUpdates()` / `supportsResultSetLimit` / `Scroll` / `Ordering` | 结果集能力 | 结果集网格（后续批次） |
| `isReadOnlyData()` / `isReadOnlyMetaData()` | 数据/元数据只读 | 编辑门控（后续批次） |

2. **三态样板**：`plugins/org.jkiss.dbeaver.ext.generic/.../GenericTableBase.java:233-256` ——

```java
if (getDataSource().getInfo().supportsIndexes()) { ... return cache.getObjects(...); }
return null;   // 不支持 = null，而不是空集合
```

**取法**：能力位由**驱动/数据源**回答，模型层用 `null` 表达"不支持"，UI 消费位。我们照抄这个分工，不把能力判断散进 provider。

---

## 4. 设计

### 4.1 能力分两层（不改现有字典的语义）

| 层 | 载体 | 用途 |
| --- | --- | --- |
| **功能键**（现有 15 键） | `driver/src/capability.rs` + `drivers.capabilities` | 连接对话框「能力」Tab、能力矩阵对比。**本次不动** |
| **元数据面能力位**（新增） | 同一列 `drivers.capabilities`，新键 | 属性分区置灰的输入 |

新增键（初稿 9 条，按分区可再收）：

| 键 | 中文 | DBeaver 对应 | 内核 6 驱动现状 | sidecar 现状 |
| --- | --- | --- | --- | --- |
| `meta_index_detail` | 索引明细 | `supportsIndexes` | 4 族全支持 | **不支持**（报 Err） |
| `meta_constraint_detail` | 约束明细 | `supportsReferentialIntegrity` | 4 族全支持 | **不支持**（报 Err） |
| `meta_ddl_source` | 源版 DDL | （DBeaver 无独立位，按驱动实现） | MySQL `SHOW CREATE`；PG/SQLite/DuckDB 无 → 合成 | **不支持**（协议无方法） |
| `meta_ddl_compose` | 合成 DDL | — | 全部（`sql_gen::create_table_ddl`） | 需要宿主侧合成（`meta.object_detail` 够用） |
| `meta_routine_source` | 例程源码 | `supportsStoredCode` | PG/MySQL 支持；SQLite/DuckDB 不适用 | 支持（`meta.routine_source`） |
| `meta_sequences` | 序列 | — | PG/DuckDB/MySQL 支持；SQLite 无 | 取决于插件 |
| `meta_triggers` | 触发器 | — | 4 族支持 | 取决于 `meta.objects` 覆盖 |
| `meta_fk_target` | 列级外键目标 | `supportsReferentialIntegrity` | 4 族支持 | **不支持**（线格式无字段） |
| `meta_column_ordinal` | 列序号 | — | 4 族支持 | **不支持**（填 0） |

> 说明：`meta_ddl_compose` 不是"驱动能力"而是"产品能力"——只要拿到列/约束/索引，宿主就能合成。它单独成键是为了让 UI 能区分「源版不可用但合成可用」与「两个都不可用」。

### 4.2 取数三态（把"不支持"传上来）

新增一个显式三态，替代 trait 默认的空 Vec（对应 §2 #5）：

```
Support<T> = Supported(T) | Empty | NotSupported(&'static str)
```

落地方式二选一（**待评审选一个**）：

- **A（最小改动）**：trait 方法签名不动，新增 `MetadataBrowser::supports(key) -> bool` + `MetadataService::capability(conn_id, key) -> CapState`；分区是否可用只看能力位，取数仍 `unwrap_or_default()`。
- **B（更彻底）**：trait 改为 `Result<Option<Vec<T>>>`（`None` = 不支持），`MetadataService` 原样透传；能力位只做**前置**判断，兜底仍以实际返回为准。

> 建议 **B**：能力位会漂移（插件清单写了 `meta_index_detail` 但实现没跟上），三态里"实际返回优先"能自我纠偏；代价是改 trait 签名 + 6 个驱动 + sidecar 桥 + 调用点（约 10 处）。

### 4.3 模型与渲染

```
PropertySection {
    id: PageId,
    label: String,
    table: PropertyTable,
    availability: Availability,   // 新增
}
Availability = Available | Unsupported(reason: String)
```

- **页签保留**，内容区整块灰化 + 一行原因文案（例：「该驱动不支持读取索引明细（sqlx 插件未实现 meta.indexes）」）；
- 计数标签在不可用时**不显示 `(0)`**，改为「索引」+ 灰化（0 是"真的没有"，不支持是另一回事）；
- provider `categories()` 仍决定"页签集合与顺序"，`availability` 由**骨架**（`property_panel::load_properties`）按能力位填写——provider 不感知驱动。

### 4.4 门控输入从"族"改为"族默认 × 驱动声明"

`property_providers::DriverCapabilities`（`:373`）现在是**族级硬编码**（`DriverCapabilities::mysql()` / `::pg()` …）。改动：

1. 保留族级默认（新增数据库仍是"填一份 caps"）；
2. `ProviderInput` 增 `driver_caps: &DriverCaps`（来自 `drivers.capabilities` 实际行）；
3. provider 只填默认，骨架用 `driver_caps` 覆盖 → 同族两个驱动可以显示不同页签状态，**provider 零改动**。

### 4.5 谁来写这些位

| 来源 | 写入路径（已存在） |
| --- | --- |
| 内核驱动 | `driver/src/registry/descriptors.rs` 的 `with_capabilities` → `declaration.rs::sync_driver_declarations` upsert |
| 插件驱动 | `plugin.toml` 的 `[[contributes.drivers]] features` → `plugin/src/driver_decl.rs` 落 `drivers.capabilities`（`:182`） |

两者都进 `drivers` 表同一列，读模型唯一——不需要第二份契约。

---

## 5. 留痕

见 `driver-operation-audit-plan.md`。本方案只需两件事：

1. **能力判定可查**：属性分区灰化时，hover 显示「能力位名称 + 声明来源（代码 / 插件 `<id>`）+ 最后同步时间」；
2. **判定结果不入日志**（读操作，量大且无审计价值）；**执行侧拒绝要留痕**（如因驱动不支持而拦下的 DDL 操作，写入查询历史，见该文档 §2 缺口 1）。

---

## 6. 与 sqlx 抽出的衔接

顺序：**先落本方案，再抽 sqlx**。理由：抽完之后 `mysql` / `postgres` 由插件提供，它们的 `meta_index_detail` / `meta_constraint_detail` / `meta_ddl_source` 一律为"不支持"；没有置灰底座，这些缺口会静默成「索引 (0)」，与验收口径 2 直接冲突。

置灰底座落地后，sqlx 插件只需在清单里**如实声明**自己有哪些位，UI 自动灰掉其余——抽取变成纯机械工作。

---

## 7. 验收清单（评审后细化成用例名）

| # | 场景 | 断言 | 落点 |
| --- | --- | --- | --- |
| G-1 | 内核 MySQL（`mysql_native`）表属性 | 索引/约束/DDL 三页 `Available`，内容非空 | `crates/workbench/tests/official_driver_real.rs` 扩 |
| G-2 | sidecar 驱动（mssql 或 oracle）表属性 | 索引/约束页 `Unsupported(原因)` 且**页签仍在**；不出现「索引 (0)」 | `crates/plugin/tests/meta_bridge_real_process.rs` 扩 |
| G-3 | SQLite 表属性 | 序列/例程页按能力位置灰（不是消失） | `crates/database` 单测 |
| G-4 | 同族两驱动切换 | 同一张表在两个驱动下页签集合可不同，provider 零改动 | `property_providers` 单测 + 契约测试 |
| G-5 | `Stage::NotBuilt` 键 | 仍不产出页签（隐藏式未被推翻） | `capability::tests::not_built_keys_are_claimed_by_nobody` 扩 |
| G-6 | 能力位与实现不一致 | 位声明支持但取数返回不支持 → 以实际返回为准并置灰 | 单测（三态 B） |
| G-7 | 能力位来源可查 | hover 文案含键名 + 来源 + 同步时间 | UI 契约测试 |

**D10 口径**：以上每条都必须有可复现用例名登记进能力字典的 `acceptance`，否则标"未验收"。

---

## 8. 不做 / 待定

- **不做**：权限页、依赖页、分区页的内容接入（`CapSet` 里仍是 `false`，继续隐藏）；敏感列脱敏（用户已明确不做）。
- **待定**：
  - Q1 三态用 A 还是 B（§4.2）；
  - Q2 `meta_ddl_source` 是否要独立成键，还是复用一个 `meta_ddl` + `source_kind` 字段；
  - Q3 结果集面（`supportsResultSetLimit/Scroll/Ordering`）是否本批一起做，还是留给结果集网格批次。
