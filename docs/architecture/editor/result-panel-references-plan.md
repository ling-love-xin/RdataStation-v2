# 结果集右栏「引用」页（References）对齐方案

> 状态：**方案待拍板**（2026-11-18）。本文件只记录取证、可行性与取舍，不含实现。
> 实施完成后，结论并入 `docs/architecture/editor/result-panel.md` 的 §8.x 小节。

## 1. DBeaver 取证（源码 `D:\RdataStation\RDS\dbeaver-ref`）

文件：`plugins\org.jkiss.dbeaver.ui.editors.data\src\org\jkiss\dbeaver\ui\controls\resultset\panel\references\`

- `ReferencesPanel.java`
  - `contributeActions`（`:156-158`）**空实现** ⇒ 该面板没有工具条动作（本仓右栏「引用」页目前也维持空态，这一点已对齐）。
  - 面板正文只有一个占位容器（`:68-71`），真正内容在 `ReferencesResultsContainer`。
  - 选中变化 → `getResultsContainer().refreshReferences(false)`（`:73-99`），而且只在**本面板可见**时刷新（`:78-80`、`:85-87`）。
- `ReferencesResultsContainer.java`
  - 顶部（**面板正文内**，不是面板工具条）一行三件（`:91-140`）：
    1. 只读下拉 `CSmartCombo<ReferenceKey>`，首项 `null`，其文案 = `ResultSetMessages.refs_no_refs_text`（"无引用"）；
    2. 「打开目标对象」按钮（`refs_open_target` / `refs_open_target_tip`，图标 `TREE_TABLE`）→ 导航到 `targetEntity`；
    3. 下方**嵌套一个 `ResultSetViewer`**（`:143-149`）显示引用行。
  - 键清单来源（`refreshReferenceKeyList`，`:219-357`）：
    - 参与判断的列 = 结果集**可见属性**（`getVisibleAttributes`）；
    - 出向（`isReference=false`）：`DBVUtils.getAllAssociations(entity)` 的 FK，**且**该 FK 的全部列都在可见属性里（`:298`）才收；
    - 入向（`isReference=true`）：`DBVUtils.getAllReferences(entity)`（`:310-321`）；
    - 实体集合 = 可见属性所属的实体（`:241-247`），结果集本身是实体时兜底（`:248-250`）；
    - 读元数据是**后台 Job**（`AbstractJob`，`:268-356`），失败**静默**（`:347-350`：注释「引用或外键可能不被当前数据库支持」）。
  - 键文案（`RefKeyLabelProvider.getText`，`:512-528`）：`目标实体名 (关联名)`，目标在不同数据源时再追加 `[连接名]`；空项给「无引用」。
  - 选中键后取行（`refreshKeyValues`，`:382-432`）：按父结果集的**选中行集合**，`dataViewer.navigateReference / navigateAssociation` 生成并执行查询；选中行为空则该区域清空。
  - 活动键持久化（`:106-120` / `:252-265`）：写进虚拟实体属性 `ref-panel-associations`（`ReferenceKey.createMemo()`：`ref` / `entity` / `name`），下次同表打开自动选回。

## 2. 本仓现状（实测）

- 右栏六页里的「引用」页目前 = 页内工具条（标题「引用」、动作空，与 DBeaver 的 `contributeActions` 空实现同形）
  + 空态正文「选中一个列查看外键引用」（`crates/result_panel/src/panel/tabs.rs:951-972`）。
  即：**形已对齐，数据与交互未接**。
- 结果集的列元数据 `workbench_shell::model::ColumnAnnotation`（`crates/workbench_shell/src/model.rs:327-340`）只有
  `source_table` / `source_column` / `comment` / `data_type` / `nullable` / `default_value`，**没有外键信息**。
- 外键元数据**已经在仓库里**，口径是 `engine::driver::traits::ColumnDetail::references` +
  `database::metadata_service::MetadataService::list_constraints(conn_id, catalog, schema, table)`
  （`crates/database/src/er/loader.rs:29-76` 已是现成用法：一次 `list_tables` + 一次
  `list_columns_batch` + **逐表** `list_constraints`）。
  ⇒ **不需要动 `rds-driver` 契约、不需要新增驱动端口。**
- 宿主侧能拿到连接：`EditorHostPanel::bound_connection()`（`crates/editor/src/view/host/editor_body.rs:372-385` 一带）给出 `conn_id`；
  catalog / schema 由 `list_tables` 的参数决定（结果集列注解里的 `source_table` 是 `schema.表` 或裸名）。
- 结果面板**不认识 engine / database**（依赖只向下），所以取元数据必须由宿主发起，
  与既有 `ResultRequest::GenerateResultSetDdl` 同一条「面板推请求 → 宿主分派」通道。

## 3. 成本评估

| 环节 | 内容 | 成本 | 风险 |
| --- | --- | --- | --- |
| A 键清单 | 宿主后台任务调 `list_constraints`，按 DBeaver 规则（列 ⊆ 可见列）过滤出向/入向键，回填面板；面板用 `Select` + 「打开目标对象」按钮渲染 | 中 | 入向要扫 schema 内其它表 → N 次往返（ER 已有 `MAX_TABLES=120` 上限可复用） |
| B 引用行（嵌套视图） | 面板内再挂一份 grid/store + 第二次查询通道 | 高 | 现有多结果集（C1）与 store 的生命周期耦合，容易把「一份结果集」的假设打破 |
| B′ 引用行（交给编辑器） | 面板生成 `SELECT … WHERE fk IN (选中行值)`，宿主以「新结果集」执行并切过去 | 中 | 需要选中行值 + 新结果集入口；语义是「另开一份结果集」，不是 DBeaver 的嵌套视图 |
| C 仅清单 + 导航 | 只做 A，不做行数据；选中键后给「打开目标对象」（导航到该表） | 低 | 与 DBeaver 的差就是「看不到引用行」 |

## 4. 决策点（需要拍板）

1. **引用行的载体**：`B`（面板内嵌套结果视图，最像 DBeaver、成本最高）／`B′`（生成外键条件 SQL → 编辑器新开一份结果集）／`C`（首版只做清单 + 打开目标对象）。
2. **键清单范围**：只做出向（本表 → 父表，1 次 `list_constraints`），还是同时做入向（子表 → 本表，需扫 schema 内其它表，N 次往返）。
3. **无外键时的文案**：DBeaver 是「无引用」（`refs_no_refs_text`）；本仓驱动不支持外键时是否也照这句，而不是报错（DBeaver 的读失败是静默的）。

## 5. 建议

**按 `C → B′` 两步走**：

- 第一步（可立即落地）：宿主后台任务取当前表约束 → 面板渲染「引用键清单」（只做出向，1 次往返）+ 「打开目标对象」；
  无约束时显示「无引用（当前连接未提供外键元数据）」。这一步没有任何执行通道风险，也不碰 store。
- 第二步：给选中的键生成「外键条件 SQL」并在编辑器里新开一份结果集（复用 C1 的多结果集），
  这一步落地后才具备「看引用行」的能力；与 DBeaver 的差异（嵌套视图 vs 新结果集）写进文档。

不建议走 `B`（面板内嵌套结果视图）：会把「一份结果集 = 一份 store/exec 上下文」的既有假设打破，
而收益（视觉上更像 DBeaver）远小于回归风险。
