# rds-result-panel（结果集表格模块）

结果集表格模块的独立 crate：DBeaver 式 Data Editor 方向的「结果集面板」。

## 定位

从 `rds-editor` 拆出的结果域视图/模型层。**依赖只向下**：

```
rds-editor  ──依赖──>  rds-result-panel  ──依赖──>  rds-engine / gpui-kit / rds-workbench-shell / rds-shared
```

本 crate **绝不反向依赖 rds-editor**。editor 侧经 `pub use` 重导出保旧路径（
`crate::store::ResultStore`、`crate::view::results::grid::*` 等写法不变）。

## 模块

| 模块 | 职责 |
| --- | --- |
| `store` | 结果集权威存储：按文档分槽（`DocumentResults`），多结果集 + 主选中 `active` + 对比选中 `compare`（并排对比）；淘汰上限 5 份 |
| `changes` | 单元格未提交变更集（脏格 / 冲突格 / 批量提交状态） |
| `export` | 结果导出（菜单项纯函数 / 默认表名 / RowKey / 导出执行） |
| `grid` | 结果网格视图：`ResultGridDelegate`（TableDelegate）、`ResultToolbar` / `ResultStatus` / `ResultControls` / `ResultPane`、`result_grid::render`、右键/筛选/排序/取段/转置/洞察钩子、`RESULT_GRID_CONTEXT` |
| `sets` | 结果集标签条（`ResultSetTab` 纯函数 + TabBar 渲染） |
| `error_card` | 执行失败卡片 |
| `script_card` | 脚本输出卡片 |
| `analysis` | 本地分析菜单（纯模型：菜单项 / 请求构造 / SQL 标识符引用；`result_entry` 留在 editor 侧） |
| `status` | 耗时文案（`elapsed_text`） |
| `ui` | 结果面板私有尺寸常量（单一来源 `pub use workbench_shell::ui::HAIRLINE`） |

## 测试

- `cargo test -p rds-result-panel --lib`：107 passed（grid 46 / store 17 / export / sets / changes / status / analysis）
- editor 侧回归：`cargo test -p rds-editor --lib`（581 passed；结果域测试随本 crate 迁出）
- 编译/测试前设 `$env:RUST_MIN_STACK='268435456'`

## 设计要点

- **ResultStore 归属随面板走**（用户确认）：`rds-engine` 只保留 `QueryResult` 不动；结果集多槽、变更集、导出、标签全在本 crate。
- **render 纯读**：投影（`sync_result_view`）在事件路径算好，渲染只组装；网格数据经 `set_data` 拷入 delegate，网格不持有第二份真值。
- **并排对比**：Tab 为主 + 工具栏「⧉ 并排对比」开关；双栏各带标签条（左切主选中 / 右切对比对象）；对比栏只读（编辑/取段/筛选/洞察都归主栏）。详见 `docs/architecture/editor/result-panel.md`。
- 测试访问器（`has_filter_value_hook` 等）是**无条件公开 API**：作为依赖编译时 `#[cfg(test)]` 不生成，而 editor 窗口测试也断言它们。

## 待办（不属于本 crate 现状）

- ResultPanel 实体化：`rds-editor` 的 `host.rs` 结果区胶水（sync_result_view / 工具栏组装 / 双栏）迁入本 crate，editor 只保留访问口。
- 结果面板侧边栏（选中列信息：类型 / 注释 / 统计常驻显示）。
