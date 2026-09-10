# ⛔ 已废弃 — SQL 编辑器文档

> **此文档已废弃，不再维护。**
> 原因：编辑器引擎已从 Monaco Editor 迁移至 CodeMirror 6（2026-05-17 完成）。
> 替代文档：[编辑器架构设计文档 v3.0](../editor-architecture-v3.md)
>
> 以下为历史存档内容，仅供参考。

---

# SQL 编辑器完整文档（已废弃）

> 版本：v1.9
> 最后更新：2026-05-09
> 状态：✅ P0-P6 全覆盖（全局配置持久化集成） | [优化计划](./SQL-EDITOR-OPTIMIZATION-PLAN.md)

---

> 🔗 **架构优化计划**：详见 [SQL-EDITOR-OPTIMIZATION-PLAN.md](./SQL-EDITOR-OPTIMIZATION-PLAN.md)
> 优化目标：组件拆分（1600行→80行）、类型安全强化、通信规范化、持久化统一

---

## 📖 目录

- [概述](#概述)
- [架构设计](#架构设计)
- [核心功能](#核心功能)
- [快捷键](#快捷键)
- [SQL 模板库](#sql-模板库)
- [执行历史](#执行历史)
- [服务层](#服务层)
- [状态管理](#状态管理)
- [优化记录](#优化记录)
- [优化计划](#优化计划)
- [待优化项](#待优化项)

---

## 概述

SQL 编辑器是 RdataStation 的核心功能模块，基于 Monaco Editor 构建，提供完整的 SQL 编写、执行、结果展示能力。

### 技术栈

| 技术          | 版本   | 用途            |
| ------------- | ------ | --------------- |
| Monaco Editor | 0.52.2 | SQL 代码编辑器  |
| Vue 3         | 3.5.13 | UI 框架         |
| TypeScript    | 5.8.3  | 类型安全        |
| Pinia         | 2.3.1  | 状态管理        |
| AG Grid       | 33.0.0 | 结果表格        |
| Naive UI      | 最新   | 组件库          |
| sqlglot-rust  | 最新   | SQL 解析/格式化 |

### 文件结构

```
src/extensions/builtin/workbench/
├── ui/
│   ├── components/panels/
│   │   ├── SqlEditorPanel.vue      # SQL 编辑器主面板
│   │   ├── QueryResultPanel.vue    # 查询结果面板
│   │   ├── MultiTabResults.vue     # 多标签结果面板
│   │   └── SqlHistoryPanel.vue     # 执行历史侧边栏
│   ├── views/
│   │   └── WorkbenchView.vue       # 工作台视图
│   └── stores/
│       └── sql-execution-store.ts  # SQL 执行状态管理
├── services/
│   ├── sql-editor-service.ts       # SQL 编辑器服务
│   ├── sql-dialect-highlight.ts    # 方言语法高亮服务
│   ├── sql-snippets.ts             # SQL 模板库
│   └── sql-history-service.ts      # 执行历史服务
```

---

## 1. 架构

### 1.1 1:n 编辑器↔结果关系 (DBeaver 风格)

每个 SqlEditorPanel 是一个自包含的编辑器单元：

```
┌─ SqlEditorPanel ───────────────────────────────────────────────────┐
│ ┌─ 工具栏 ──────────────────────────────────────────────────┐      │
│ │ ▶ Execute ▶▶ Execute+ │ ⚡ DuckDB │ 🔍 解释 │ 💾 其他... │      │
│ └─────────────────────────────────────────────────────────────┘      │
│ ┌─ 编辑器与结果分割区域 ─────────────────────────────────────┐      │
│ │ ┌─ SQL 编辑器 (Monaco) ────────────────────────────┐     │      │
│ │ │  SELECT * FROM users WHERE id > 100              │     │      │
│ │ └───────────────────────────────────────────────────┘     │      │
│ │ ├─ 分割线 (可拖拽 ────────────────────────────────┤      │      │
│ │ ┌─ 内嵌结果面板 ─────────────────────────────────┐      │      │
│ │ │ [结果 #1 x] [结果 #2 x]                         │      │      │
│ │ │ ┌─ AG Grid ─────────────────────────────────┐  │      │      │
│ │ │ │ # │ id │ name │ email               ...   │  │      │      │
│ │ │ └────────────────────────────────────────────┘  │      │      │
│ │ │ 347 行  0.003s                                  │      │      │
│ │ └─────────────────────────────────────────────────┘      │      │
│ └──────────────────────────────────────────────────────────┘      │
│ ┌─ 状态栏 ─────────────────────────────────────────────────┐      │
│ │ Ln 1, Col 1  │ 连接: localhost:3306                      │      │
│ └──────────────────────────────────────────────────────────┘      │
└──────────────────────────────────────────────────────────────────┘
```

**核心原则**：

- 每个 SQL 编辑器面板拥有**自己独立的结果区域**
- 结果区域通过可拖拽分割线与编辑器垂直分割
- 默认不显示（节省空间），有结果时自动展开
- `Execute+` 在同一编辑器内创建新结果标签
- 多个编辑器互不影响，各自持有独立的结果标签

### 1.2 对比旧架构 (n:n)

| 方面     | 旧架构                                 | 新架构                  |
| -------- | -------------------------------------- | ----------------------- |
| 结果展示 | 独立 Dockview 面板 `panel_queryResult` | 嵌入编辑器内部          |
| 多编辑器 | 共享同一个结果面板，混乱               | 每个编辑器独立结果区域  |
| 数据流   | 全局事件广播 → 结果面板监听            | 直接写入本地 ref        |
| 多结果   | QueryResultPanel 内部标签              | SqlEditorPanel 内部标签 |
| 分割方式 | Dockview 组分割                        | 编辑器内部 CSS 分割     |

---

## 核心功能

### 1. SQL 编辑

- **语法高亮**: 基于 Monaco Editor SQL 语言支持，根据数据库类型动态调整高亮规则
- **代码补全**: SQL 关键字 + 数据库 schema 智能提示
- **实时验证**: 500ms 防抖自动语法检查
- **自动保存**: 1000ms 防抖保存草稿到 localStorage

### 2. SQL 执行

#### 单语句执行

```typescript
const result = await sqlExecutionStore.executeSql(panelId, sql, connectionId)
```

#### 多语句执行

- **自动检测**: 使用 sqlglot-rust 解析语句数量（DDL 语句如 CREATE TABLE 解析失败时，按单语句执行）
- **执行模式**:
  - 批量执行: 全部执行完再显示结果
  - 逐条执行: 每条语句执行后立即显示结果

> **注意**: DDL 语句（CREATE TABLE / DROP / ALTER 等）可能被 sqlglot-rust 判定为解析失败。此时不会阻断执行，而是自动降级为单语句执行，交由后端 SQL 引擎处理语法。`parseSql` 失败不再阻断 DDL 执行。

### 3. SQL 格式化

```typescript
const formatted = await formatSql(sql, dialect)
```

支持方言: MySQL, PostgreSQL, SQLite, DuckDB

### 4. 执行计划

```typescript
const explainSql = `EXPLAIN ${sql}`
const result = await sqlExecutionStore.executeSql(panelId, explainSql, connectionId)
```

### 5. 结果展示

#### DBeaver 风格布局

```
┌─ SQL 编辑器组 ─────────────────────────────────────────────┐
│ ┌─ SqlEditorPanel ───────────────────────────────────────┐ │
│ │  SELECT * FROM users                                    │ │
│ └─────────────────────────────────────────────────────────┘ │
│ ┌─ QueryResultPanel ─────────────────────────────────────┐ │
│ │ ┌─ Tab 栏 ──────────────────────────────────────────┐  │ │
│ │ │  [结果 100]  [输出]                                 │  │ │
│ │ └────────────────────────────────────────────────────┘  │ │
│ │ ┌─ 工具栏 ───────────────────────────────────────────┐  │ │
│ │ │  [复制 CSV] [复制制表符] [复制 INSERT] | [CSV] [JSON] | [自动调整列宽]  |  [搜索...] │  │
│ │ └────────────────────────────────────────────────────┘  │ │
│ │ ┌─ AG Grid ──────────────────────────────────────────┐  │ │
│ │ │  # │ id │ name │ email │ age │ ...                 │  │ │
│ │ │  1 │ 1  │ John │ j@... │ 30  │ ...                 │  │ │
│ │ │  2 │ 2  │ Jane │ jn@.. │ 25  │ ...                 │  │ │
│ │ └────────────────────────────────────────────────────┘  │ │
│ │ ┌─ 状态栏 ───────────────────────────────────────────┐  │ │
│ │ │  100 行 | 列数: 5 | 已选 2 行 | 排序: age desc     │  │ │
│ │ └────────────────────────────────────────────────────┘  │ │
│ └─────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
```

#### 二次分析功能

工具栏提供对 SQL 结果的二次封装能力（基于当前选择生成新的 SQL）：

| 功能           | 按钮                                                       | 说明                                        |
| -------------- | ---------------------------------------------------------- | ------------------------------------------- |
| **增强复制**   | 选中行复制为 TSV；支持复制为 INSERT/JSON；右键菜单多种格式 |
| **导出 CSV**   | 📊                                                         | 下载为 CSV 文件                             |
| **导出 JSON**  | 📊                                                         | 下载为 JSON 文件                            |
| **自适应列宽** | ↔                                                          | 自动调整列宽适配内容                        |
| **快速搜索**   | 🔍                                                         | AG Grid 的 quick filter，高亮匹配内容       |
| **排序**       | 点击列头                                                   | 点击列头 `↑/↓` 排序，状态栏实时显示排序信息 |
| **筛选**       | 列头输入框                                                 | 浮动过滤器，支持文本和数值智能比较          |
| **分页**       | 分页控件                                                   | 自动分页（200条/页），支持切换全部显示      |

#### 二次分析场景

1. **筛选 + 生成 WHERE**：在 AG Grid 中设置筛选条件，复制为 SELECT → 生成带 WHERE 的查询
2. **选中行 + 生成 WHERE IN**：选中多行 → 复制为 INSERT 或使用行 ID 生成 IN 子句
3. **排序状态复用**：在 AG Grid 中排序后，排序信息出现在状态栏，可手动复制 ORDER BY 子句
4. **数据导出**：CSV → Excel 进一步分析，JSON → API 分装

#### Tab 导航

| Tab      | 内容                                      |
| -------- | ----------------------------------------- |
| **结果** | AG Grid 数据表格，支持排序/筛选/分页/选择 |
| **输出** | 执行信息（行数/耗时/影响行数）            |

#### 显示优化

- NULL 值以灰色斜体 `NULL` 标记显示，不占位空单元格
- 行号列固定左侧，显示行序号
- 搜索高亮匹配内容
- 悬浮筛选输入框
- 切换分页/不分页模式（大数据量时关闭分页提升性能）

### 6. 工具栏

#### 分组设计

工具栏按功能分为三个可折叠组（状态持久化到 localStorage）：

```typescript
interface ToolbarGroups {
  execute: boolean // 执行组：执行 SQL、DuckDB 加速、解释查询
  edit: boolean // 编辑组：格式化、验证、方言转换
  features: boolean // 功能组：执行历史、设置
}
```

每组有标签头和折叠箭头，点击标签头切换折叠状态：

- 三角形箭头 ▼ 朝下 = 展开
- 箭头 ▶ 朝右 = 折叠

#### 分组布局

| 组       | 按钮            | 说明                                                                |
| -------- | --------------- | ------------------------------------------------------------------- |
| **执行** | ▶ 执行          | 执行当前 SQL (Ctrl+Enter)                                           |
|          | ➕ **Execute+** | **执行并打开新结果标签**（DBeaver 风格，快捷键 `Ctrl+Shift+Enter`） |
|          | ⚡ 加速         | DuckDB 连接时才显示                                                 |
|          | 🔍 解释         | 显示执行计划                                                        |
| **编辑** | ↹ 格式化        | 格式化 SQL (Ctrl+Shift+F)                                           |
|          | ✨ 验证         | 语法验证                                                            |
|          | ↔ 转换          | SQL 方言转换弹窗                                                    |
| **功能** | 🕐 历史         | 执行历史记录                                                        |
|          | ⚙ 设置          | 编辑器设置                                                          |

**Execute+ 行为**：

- 执行当前 SQL / 选中 SQL
- 通过 `query-result-new` 事件通知结果面板
- 结果面板始终创建 **新标签**（不会覆盖已有结果）
- 每个标签可独立进行三模式过滤分析

#### 工具栏位置

支持三种布局位置，点击右侧位置切换按钮循环切换：

- **顶部** (默认): 水平工具栏，分组横向排列
- **左侧**: 垂直工具栏 (48px 宽)，分组纵向排列
- **右侧**: 垂直工具栏 (48px 宽)，分组纵向排列

位置切换状态持久化到 localStorage。

### 7. 状态栏

状态栏显示以下信息：

- **光标位置**: `Ln 1, Col 1`
- **选中文本**: `已选择 X 行, Y 字符`
- **编辑器模式**: `SQL`
- **执行状态**: 执行中动画 / 上次执行耗时
- **连接信息**: `连接名 → 数据库 → schema` (挖空内嵌标签，点击可切换连接)

连接选择从工具栏移除，集成到状态栏中：

- 使用 `NPopselect` 组件，点击连接信息标签弹出连接列表
- 选项来源：`connectionStore.connections(已连接) + runtimeConnectionIds(补充)`
- 连接状态变化时（建立/断开），状态栏实时同步
- 初始化时，通过 `waitForConnection()` 轮询连接列表（最多 10 秒），解决异步加载时序问题
- 连接选择**完全独立**，导航栏切换连接不影响 SQL 编辑器

#### 连接信息文本的三层回退策略

状态栏显示的连接信息来自 `connectionInfoText` computed，有三层回退：

```
1. connectionStore.connections.find(connId) → 有值 → 显示 "连接名 → 数据库 → schema"
2. runtimeConnectionIds.has(connId)         → 有值 → 显示 "connId (已连接)"
3. selectedConnection.value 非空            → 有值 → 显示 "connId"
4. 全部失败                                → 显示 "未连接"
```

这确保了即使 `connectionStore` 尚未加载完成，只要运行时连接已建立（`runtimeConnectionIds` 中有记录），状态栏也能正确显示连接状态。

#### 连接选择器行为

| 场景                              | 行为                                    |
| --------------------------------- | --------------------------------------- |
| 从数据库导航右键"打开 SQL 编辑器" | 初始自动选中该连接（waitForConnection） |
| 导航栏切换连接                    | **不影响** SQL 编辑器                   |
| 手动点击状态栏连接标签            | 弹出 NPopselect 下拉列表，点击切换      |
| 无可用连接时                      | 显示 "未连接"                           |

### 8. 欢迎页

编辑器为空时显示欢迎页，包含：

- 常用快捷键提示
- 快速操作按钮 (插入示例 SQL、查看历史记录)

---

## 快捷键

| 快捷键             | 功能                           | 说明                                      |
| ------------------ | ------------------------------ | ----------------------------------------- |
| `Ctrl+Enter`       | 执行 SQL（当前语句或选中语句） | 执行当前编辑器内容                        |
| `Ctrl+Shift+Enter` | 执行并打开新结果标签           | 执行当前 SQL/选中 SQL，在新标签中显示结果 |
| `Ctrl+Shift+F`     | 格式化 SQL                     | 自动格式化代码                            |
| `Ctrl+R`           | 执行选中 SQL                   | 执行选中的文本                            |
| `Ctrl+/`           | 注释/取消注释                  | 切换行注释                                |
| `Ctrl+Shift+R`     | 刷新结果                       | 重新执行并刷新                            |
| `Ctrl+L`           | 清空编辑器                     | 清空所有内容                              |
| `Ctrl+S`           | 保存 SQL 文件                  | 触发保存对话框                            |
| `F5`               | 执行全部                       | 执行所有语句                              |

---

## SQL 模板库

### 内置模板（30+）

#### 查询类

| 模板名          | 说明                 |
| --------------- | -------------------- |
| select-all      | SELECT \* FROM       |
| select-where    | SELECT with WHERE    |
| select-distinct | SELECT DISTINCT      |
| select-order-by | SELECT with ORDER BY |
| select-group-by | SELECT with GROUP BY |
| select-limit    | SELECT with LIMIT    |

#### 插入类

| 模板名          | 说明                 |
| --------------- | -------------------- |
| insert-into     | INSERT INTO          |
| insert-multiple | INSERT multiple rows |
| insert-select   | INSERT from SELECT   |

#### 更新类

| 模板名          | 说明                    |
| --------------- | ----------------------- |
| update-set      | UPDATE SET              |
| update-multiple | UPDATE multiple columns |

#### 删除类

| 模板名      | 说明           |
| ----------- | -------------- |
| delete-from | DELETE FROM    |
| delete-all  | TRUNCATE TABLE |

#### 创建类

| 模板名       | 说明         |
| ------------ | ------------ |
| create-table | CREATE TABLE |
| create-view  | CREATE VIEW  |
| create-index | CREATE INDEX |

#### 连接类

| 模板名     | 说明            |
| ---------- | --------------- |
| inner-join | INNER JOIN      |
| left-join  | LEFT JOIN       |
| right-join | RIGHT JOIN      |
| full-join  | FULL OUTER JOIN |
| cross-join | CROSS JOIN      |

#### 聚合函数

| 模板名 | 说明        |
| ------ | ----------- |
| count  | COUNT(\*)   |
| sum    | SUM(column) |
| avg    | AVG(column) |
| max    | MAX(column) |
| min    | MIN(column) |

#### 事务

| 模板名      | 说明              |
| ----------- | ----------------- |
| transaction | BEGIN TRANSACTION |
| rollback    | ROLLBACK          |

#### 窗口函数

| 模板名     | 说明              |
| ---------- | ----------------- |
| row-number | ROW_NUMBER() OVER |
| rank       | RANK() OVER       |
| dense-rank | DENSE_RANK() OVER |
| lag        | LAG() OVER        |
| lead       | LEAD() OVER       |

#### CTE

| 模板名        | 说明           |
| ------------- | -------------- |
| with-cte      | WITH (CTE)     |
| recursive-cte | WITH RECURSIVE |

#### 子查询

| 模板名          | 说明              |
| --------------- | ----------------- |
| subquery-in     | IN (subquery)     |
| subquery-exists | EXISTS (subquery) |

#### 条件表达式

| 模板名    | 说明      |
| --------- | --------- |
| case-when | CASE WHEN |
| coalesce  | COALESCE  |
| nullif    | NULLIF    |

### 自定义模板

用户可通过 localStorage 添加自定义模板:

```typescript
import { addCustomSnippet, getCustomSnippets } from './sql-snippets'

// 添加自定义模板
addCustomSnippet({
  label: 'my-template',
  detail: 'My Custom Template',
  insertText: 'SELECT * FROM my_table',
  category: '自定义',
})

// 获取所有自定义模板
const custom = getCustomSnippets()
```

### 导入/导出

```typescript
import { exportCustomSnippets, importCustomSnippets } from './sql-snippets'

// 导出为 JSON
const json = exportCustomSnippets()

// 导入 JSON
importCustomSnippets(json)
```

---

## 执行历史

### 功能

- ✅ 自动记录每次执行
- ✅ 收藏常用 SQL
- ✅ 搜索历史记录
- ✅ 添加标签和备注
- ✅ 执行统计
- ✅ 导出/导入历史记录

### 数据结构

```typescript
interface SqlHistoryItem {
  id: string
  sql: string
  connectionId: string
  connectionName: string
  databaseType: string
  executedAt: number
  executionTime: number
  rowCount: number
  success: boolean
  error?: string
  isFavorite: boolean
  tags?: string[]
  note?: string
}
```

### API

```typescript
import {
  getHistory,
  addHistory,
  deleteHistory,
  clearHistory,
  toggleFavorite,
  getFavorites,
  searchHistory,
  filterByConnection,
  filterByDatabaseType,
  filterByDateRange,
  getStatistics,
  addTag,
  removeTag,
  addNote,
  exportHistory,
  importHistory,
  getFrequentSql,
  getRecentSql,
} from './sql-history-service'

// 添加历史记录
addHistory({
  sql: 'SELECT * FROM users',
  connectionId: 'conn-1',
  connectionName: 'MySQL Local',
  databaseType: 'mysql',
  executionTime: 150,
  rowCount: 100,
  success: true,
  isFavorite: false,
})

// 获取统计信息
const stats = getStatistics()
// {
//   totalExecutions: 150,
//   successRate: 95.5,
//   averageExecutionTime: 120,
//   totalFavorites: 20,
//   topConnections: [...]
// }
```

---

## 服务层

### sql-editor-service.ts

提供 SQL 编辑器核心服务:

| 函数                                 | 说明               |
| ------------------------------------ | ------------------ |
| `registerDatabaseCompletionProvider` | 注册数据库代码补全 |
| `unregisterCompletionProvider`       | 注销代码补全       |
| `validateSql`                        | 验证 SQL 语法      |
| `formatSql`                          | 格式化 SQL         |
| `parseSql`                           | 解析 SQL           |
| `splitSql`                           | 分割多语句 SQL     |
| `transpileSql`                       | SQL 方言转换       |
| `getSchema`                          | 获取数据库 schema  |
| `clearSchemaCache`                   | 清除 schema 缓存   |

### query.ts（SQL 执行 + 事务 + 历史）

提供 SQL 执行、事务控制、取消和历史管理:

| 函数                   | 说明               |
| ---------------------- | ------------------ |
| `executeSql`           | 执行 SQL（主入口） |
| `executeTransaction`   | 事务内执行多 SQL   |
| `cancelQuery`          | 取消正在执行的查询 |
| `beginTransaction`     | 开始事务           |
| `commitTransaction`    | 提交事务           |
| `rollbackTransaction`  | 回滚事务           |
| `getTransactionStatus` | 获取事务状态       |
| `getSqlHistory`        | 获取 SQL 历史      |
| `searchSqlHistory`     | 搜索 SQL 历史      |
| `clearSqlHistory`      | 清空 SQL 历史      |
| `removeSqlHistory`     | 删除单条历史       |

### sql-snippets.ts

提供 SQL 模板库服务:

| 函数                    | 说明           |
| ----------------------- | -------------- |
| `getAllSnippets`        | 获取所有模板   |
| `getBuiltInSnippets`    | 获取内置模板   |
| `getCustomSnippets`     | 获取自定义模板 |
| `addCustomSnippet`      | 添加自定义模板 |
| `deleteCustomSnippet`   | 删除自定义模板 |
| `updateCustomSnippet`   | 更新自定义模板 |
| `getSnippetsByCategory` | 按分类获取     |
| `getCategories`         | 获取所有分类   |
| `searchSnippets`        | 搜索模板       |
| `exportCustomSnippets`  | 导出模板       |
| `importCustomSnippets`  | 导入模板       |
| `resetCustomSnippets`   | 重置自定义模板 |

### sql-history-service.ts

提供执行历史服务:

| 函数                   | 说明             |
| ---------------------- | ---------------- |
| `getHistory`           | 获取历史记录     |
| `addHistory`           | 添加历史记录     |
| `deleteHistory`        | 删除历史记录     |
| `clearHistory`         | 清空历史记录     |
| `toggleFavorite`       | 切换收藏         |
| `getFavorites`         | 获取收藏         |
| `searchHistory`        | 搜索历史         |
| `filterByConnection`   | 按连接过滤       |
| `filterByDatabaseType` | 按数据库类型过滤 |
| `filterByDateRange`    | 按时间范围过滤   |
| `getStatistics`        | 获取统计信息     |
| `addTag`               | 添加标签         |
| `removeTag`            | 移除标签         |
| `addNote`              | 添加备注         |
| `exportHistory`        | 导出历史         |
| `importHistory`        | 导入历史         |
| `getFrequentSql`       | 获取常用 SQL     |
| `getRecentSql`         | 获取最近 SQL     |

---

## 状态管理

### sql-execution-store.ts

Pinia Store 管理 SQL 执行状态:

```typescript
interface ExecutionResult {
  panelId: string
  result: QueryResult | null
  error: string | null
  timestamp: number
}

interface SqlExecutionState {
  executionResults: Map<string, ExecutionResult>
  newTabRequests: Map<string, ExecutionResult>
  refreshRequests: Map<string, number>
  activeEditorPanelId: string | null
}
```

### API

```typescript
import { useSqlExecutionStore } from './sql-execution-store'

const sqlExecutionStore = useSqlExecutionStore()

// 获取结果
const result = sqlExecutionStore.getResult(panelId)

// 清除结果
sqlExecutionStore.clearResult(panelId)

// 清除所有结果
sqlExecutionStore.clearAllResults()

// 请求在新标签打开
sqlExecutionStore.requestNewTab(panelId, executionResult)
```

---

## 优化记录

### 第一轮优化（2026-04-29）

#### P0 级别（核心修复）

| #   | 优化项           | 状态 | 说明                           |
| --- | ---------------- | ---- | ------------------------------ |
| 1   | Pinia Store 通信 | ✅   | 替代全局事件，使用面板 ID 绑定 |
| 2   | 连接状态同步     | ✅   | 从 connectionStore 实时获取    |
| 3   | 结果面板自动创建 | ✅   | 执行 SQL 时自动创建结果面板    |

#### P1 级别（核心功能）

| #   | 优化项       | 状态 | 说明                              |
| --- | ------------ | ---- | --------------------------------- |
| 4   | 多语句执行   | ✅   | 支持批量/逐条执行模式             |
| 5   | 代码补全优化 | ✅   | Disposable 管理 + 5 分钟 TTL 缓存 |
| 6   | 实时语法验证 | ✅   | 500ms 防抖自动验证                |
| 7   | SQL 自动保存 | ✅   | 1000ms 防抖保存到 localStorage    |

#### P2 级别（体验增强）

| #   | 优化项       | 状态 | 说明                     |
| --- | ------------ | ---- | ------------------------ |
| 8   | 执行计划     | ✅   | 自动生成 EXPLAIN 结果    |
| 9   | 结果面板增强 | ✅   | 启用过滤/排序/编辑功能   |
| 10  | 快捷键完善   | ✅   | 新增 6 个快捷键          |
| 11  | SQL 模板库   | ✅   | 30+ 内置模板，支持自定义 |
| 12  | 执行历史增强 | ✅   | 收藏/搜索/标签/统计      |

### 第二轮优化（2026-04-29 下午）

#### P1 级别（核心功能）

| #   | 优化项          | 状态 | 说明                             |
| --- | --------------- | ---- | -------------------------------- |
| 1   | 执行结果 Tab 化 | ✅   | 多语句执行时，结果面板支持多 Tab |
| 2   | 执行状态指示器  | ✅   | 工具栏添加执行进度条/旋转图标    |

#### P2 级别（体验增强）

| #   | 优化项           | 状态 | 说明                           |
| --- | ---------------- | ---- | ------------------------------ |
| 3   | SQL 语法高亮增强 | ✅   | 根据数据库类型动态调整高亮规则 |
| 4   | 结果面板主题跟随 | ✅   | AG Grid 的主题随系统主题切换   |
| 5   | 执行历史侧边栏   | ✅   | 可折叠的历史记录面板           |

### 第三轮优化（2026-04-29 晚上）

#### 工具栏与状态栏优化

| #   | 优化项           | 状态 | 说明                                                    |
| --- | ---------------- | ---- | ------------------------------------------------------- |
| 1   | 工具栏图标化     | ✅   | 所有按钮使用 lucide-vue-next 图标                       |
| 2   | 工具栏位置切换   | ✅   | 支持顶部/左侧/右侧三种布局                              |
| 3   | 状态栏连接信息   | ✅   | 内嵌显示 `连接名 → 数据库 → schema`                     |
| 4   | DuckDB 加速按钮  | ✅   | DuckDB 连接时显示 ⚡ 按钮，点击调用 DuckDB 引擎加速查询 |
| 5   | SQL 方言转换按钮 | ✅   | 支持 9 种方言互转                                       |
| 6   | 欢迎页优化       | ✅   | 空编辑器时显示快捷键和快速操作                          |

#### 前端错误修复

| #   | 修复项                 | 状态 | 说明                                    |
| --- | ---------------------- | ---- | --------------------------------------- |
| 1   | X 图标未导入           | ✅   | 从 lucide-vue-next 导入 X 图标          |
| 2   | MessageProvider 警告   | ✅   | 移除 containerProps 配置                |
| 3   | NTooltip 插槽错误      | ✅   | 修正 trigger 和 default 插槽顺序        |
| 4   | 连接字段不匹配         | ✅   | 兼容 connectionStore 的 dbType 字段     |
| 5   | Monaco Web Worker 错误 | ✅   | 添加 removeEventListener 到 mock worker |

### 第四轮优化（2026-04-30）

#### 连接打通与状态栏重构

| #   | 优化项               | 状态 | 说明                                   |
| --- | -------------------- | ---- | -------------------------------------- |
| 1   | 顶部连接选择器移除   | ✅   | 从工具栏移除以节省空间                 |
| 2   | 状态栏连接集成       | ✅   | 使用 NPopselect 挖空内嵌，点击切换连接 |
| 3   | 导航栏连接同步       | ✅   | 导航栏建立连接后自动同步到 SQL 编辑器  |
| 4   | 默认选中连接         | ✅   | 从导航打开时自动选中对应连接           |
| 5   | DDL 语句执行支持     | ✅   | parseSql 失败不再阻断 DDL 执行         |
| 6   | connectionStore 重构 | ✅   | 统一加载全局+项目连接，合并运行时状态  |

### 第五轮优化（2026-04-30）

#### 工具栏可折叠分组 + 连接选择修复

| #   | 优化项               | 状态 | 说明                                      |
| --- | -------------------- | ---- | ----------------------------------------- |
| 1   | 工具栏可折叠分组     | ✅   | 执行/编辑/功能三组，localStorage 持久化   |
| 2   | 连接选择器迁至状态栏 | ✅   | 从工具栏移除，使用 NPopselect 挖空内嵌    |
| 3   | 连接自动匹配修复     | ✅   | 通过 waitForConnection() 轮询解决时序问题 |
| 4   | SQL 执行失败修复     | ✅   | selectedConnection 不再被无效 ID 占用     |
| 5   | DDL 兼容性修复       | ✅   | parseSql 失败降级为单语句执行             |

#### 连接修复记录

| #   | 修复项                                    | 说明                                    |
| --- | ----------------------------------------- | --------------------------------------- |
| 1   | 移除 initEditor 中预设 selectedConnection | 避免设置无效 ID                         |
| 2   | 新增 waitForConnection 轮询               | 10 秒超时，200ms 间隔，解决异步加载时序 |
| 3   | 简化 connectionStore watcher              | 只在选中连接丢失时自动恢复              |

### 第六轮优化（2026-04-30 下午）

#### 自动建连 + DuckDB 偏好 + 导航栏优化

| #   | 优化项                                                | 状态 | 说明                                    |
| --- | ----------------------------------------------------- | ---- | --------------------------------------- |
| 1   | SQL 编辑器自动建立运行时连接                          | ✅   | 执行/解释/加速前自动 `ensureConnection` |
| 2   | runtimeConnectionStore 新增 `establishFromConnection` | ✅   | 接受 `Connection` 类型，SQL 编辑器可用  |
| 3   | 导航栏右键菜单 DuckDB 开关                            | ✅   | 每个连接可独立开启/关闭本地加速         |
| 4   | DuckDB 偏好持久化                                     | ✅   | localStorage 存储，启动自动加载         |
| 5   | 导航栏 childCount 角标移除                            | ✅   | 不再在连接行右侧显示数字                |
| 6   | toggleConnection 支持全局连接                         | ✅   | 右键菜单「连接」对全局连接生效          |

#### SQL 编辑器自动建连流程

```
用户点击执行 → 检查 runtimeConnectionIds.has(connectionId)?
        │
        ├── 是 → 直接执行
        │
        └── 否 → establishFromConnection(conn)
                  ├── connectionService.connectDatabase()
                  ├── 更新 runtimeConnectionIds
                  └── 执行 SQL
```

#### DuckDB 启用方式

| 方式                       | 说明                                       |
| -------------------------- | ------------------------------------------ |
| 右键连接 → DuckDB 本地加速 | 任意连接均可开启，持久化到 localStorage    |
| 自动 DuckDB 连接           | dbType === 'duckdb' 的连接默认显示加速按钮 |

### 第七轮优化（2026-04-30 傍晚）

#### 连接解耦 + 运行时ID修复

| #   | 优化项                                  | 状态 | 说明                                                                                                      |
| --- | --------------------------------------- | ---- | --------------------------------------------------------------------------------------------------------- |
| 1   | SQL 编辑器连接独立                      | ✅   | 移除 `connectionStore.currentConnection` watcher，导航栏切换不再影响                                      |
| 2   | 移除自动恢复 watcher                    | ✅   | 不再自动选中第一个连接，用户手动选择                                                                      |
| 3   | 配置ID→运行时ID翻译                     | ✅   | 新增 `runtimeConnId` computed，所有 `executeSql` 调用改用运行时 ID                                        |
| 4   | SQL 执行成功                            | ✅   | `executeSingleStatement` / `executeMultipleStatements` / `handleExplain` / `handleDuckDbExecute` 全部修复 |
| 5   | 全局状态栏连接信息移除                  | ✅   | `WorkbenchStatusBar` 不再显示连接名称                                                                     |
| 6   | waitForConnection 主动重载              | ✅   | 1 秒后未匹配则调用 `loadConnections()` 重试                                                               |
| 7   | runtimeMap ID 修复                      | ✅   | 用 `runtimeConnectionStore` 代替错误的 `runtimeId` 比对                                                   |
| 8   | 导航栏 select 改用 syncConnectionStatus | ✅   | 点击连接后实时更新 connectionStore 状态                                                                   |
| 9   | connectionInfoText 三层回退             | ✅   | connectionStore → runtimeIds → selectedConnection ID                                                      |
| 10  | popselectOptions 合并 runtimeIds        | ✅   | 从 runtimeConnectionIds 补充下拉选项                                                                      |
| 11  | ensureConnection 三层保障               | ✅   | runtimeIds → establishFromConnection → 返回 false                                                         |

#### 连接信息独立策略

```
导航栏切换连接 → SQL 编辑器的连接不受影响 ×
从导航右键"打开 SQL 编辑器" → 初始获取当前连接 ✓（waitForConnection）
用户在状态栏 NPopselect 手动切换 → 仅影响该编辑器实例 ✓
```

#### 运行时 ID 翻译流程

```
selectedConnection.value = "conn-new-mysql"  // 配置连接 ID（用户可见）
         ↓
runtimeConnId = runtimeConnectionStore.runtimeConnectionIds.get("conn-new-mysql")
         ↓ = "runtime-uuid-xxxx"             // 运行时连接 ID（后端使用）
         ↓
sqlExecutionStore.executeSql(panelId, sql, runtimeConnId)
         ↓
invoke('execute_sql', { sql, conn_id: "runtime-uuid-xxxx" })
         ↓
后端 connection_manager.get_connection("runtime-uuid-xxxx") → Found! ✅ → 执行 SQL
```

#### 连接信息归属确认

| 级别       | 组件                                 | 显示内容                       | 现状态                                |
| ---------- | ------------------------------------ | ------------------------------ | ------------------------------------- |
| **全局**   | `WorkbenchStatusBar`                 | 应用级信息（缓存状态、耗时等） | ✅ 仅显示系统信息，**不显示**连接信息 |
| **页面级** | `SqlEditorPanel > .editor-statusbar` | 光标位置、执行状态、连接信息   | ✅ 每个 SQL 编辑器实例独立维护        |
| **页面级** | `NavigatorStatus`                    | 连接数、数据库数、表数         | ✅ 导航栏底部                         |

全局状态栏不再显示任何数据库连接信息，连接信息仅出现在 SQL 编辑器的页面级状态栏中。

---

## 已知 Bug 修复记录

### runtimeMap ID 错配（2026-04-30）

**问题**：`connectionStore.loadConnections()` 中通过以下代码判断连接状态：

```typescript
const runtimeResult = await getConnections()
const runtimeMap = new Map(runtimeResult.map(r => [r.conn_id, r]))
//                              ↑ runtime 连接 ID（如 "runtime-uuid-xxx"）
status: runtimeMap.has(r.id) ? 'connected' : 'disconnected'
//                  ↑ 配置 ID（如 "conn-new-mysql"）— 永远找不到！
```

`getConnections()` 返回的 `conn_id` 是运行时连接ID，但配置的全局连接ID是另一个值。`runtimeMap.has(r.id)` 永远返回 `false`，所有连接状态被错误标注为 `disconnected`。

**修复**：改用 `runtimeConnectionStore.runtimeConnectionIds`（Map<配置ID, 运行时ID>）：

```typescript
const runtimeIds = useRuntimeConnectionStore().runtimeConnectionIds
const hasRuntime = (connId: string) => runtimeIds.has(connId)
status: hasRuntime(r.id) ? 'connected' : 'disconnected'
```

---

## 优化计划

> ✅ **全部完成**（2026-05-09）：六个 Phase 的架构优化已全部完成，详见 [SQL-EDITOR-OPTIMIZATION-PLAN.md](./SQL-EDITOR-OPTIMIZATION-PLAN.md)

| Phase  | 内容                                     | 状态    |
| ------ | ---------------------------------------- | ------- |
| **P1** | 类型统一 + 持久化统一                    | ✅ 完成 |
| **P2** | 组件拆分 + Composable 抽取               | ✅ 完成 |
| **P3** | CustomEvent → Pinia Store 通信重构       | ✅ 完成 |
| **P4** | Abort 取消机制 + 方言高亮增量            | ✅ 完成 |
| **P5** | 死代码清理 + 质量完善                    | ✅ 完成 |
| **P6** | 全局配置集成（设置持久化到 useAppStore） | ✅ 完成 |

### 优化成果

| 指标                    | 优化前        | 优化后                                   |
| ----------------------- | ------------- | ---------------------------------------- |
| SqlEditorPanel.vue 行数 | ~1600         | ~365 (**减少 77%**)                      |
| 文件数                  | 1 个大组件    | 4 组件 + 4 composables + 1 编排层        |
| 全局 CustomEvent        | 7 个          | **0 个**                                 |
| 类型松散 (`as any`)     | 多处          | **归类到统一类型定义**                   |
| 方言高亮重建            | 每次完整重建  | **按方言缓存 + 基础规则复用**            |
| Abort 取消支持          | ❌            | ✅                                       |
| 执行路径                | 双路径并行    | **统一 queryService**                    |
| 死代码                  | 多处残留      | **全部清理**                             |
| 代码折叠                | 仅缩进        | **SQL 语义折叠**                         |
| SQL 收藏管理            | ❌            | ✅ **Star 保存 + 片段面板管理**          |
| 参数化查询 :param       | ❌            | ✅ **检测 → 弹窗 → 自动执行**            |
| 表数据 Inline Edit      | ❌            | ✅ **双击编辑 + UPDATE 生成**            |
| SQL 错误定位            | ❌ 仅消息提示 | ✅ **行列解析 → Monaco marker 跳转**     |
| Minimap / Settings      | ❌ 无         | ✅ **Map toggle + 实时设置面板**         |
| Monaco 右键菜单         | ❌ 仅原生     | ✅ **Execute Selected / Copy as VALUES** |
| Scratchpad 关闭保存     | ❌ 丢失修改   | ✅ **自动保存脏内容**                    |
| SQL 历史重执行          | ❌ 仅插入     | ✅ **双击直接执行**                      |
| Welcome 快速连接        | ❌ 仅快捷键   | ✅ **点击最近连接一键连接**              |
| 全局配置持久化          | ❌ 会话丢失   | ✅ **6 项设置跨会话保持**                |

---

## 待优化项

### 已纳入优化计划

以下项目已纳入 [架构优化计划](./SQL-EDITOR-OPTIMIZATION-PLAN.md)：

| #   | 优化项                         | 归属 Phase |
| --- | ------------------------------ | ---------- |
| 1   | 组件拆分（5组件+4composables） | P2         |
| 2   | 类型安全强化（消除 as any）    | P1         |
| 3   | CustomEvent → Pinia Store 通信 | P3         |
| 4   | localStorage 持久化统一        | P1         |
| 5   | 方言高亮增量更新               | P4         |
| 6   | Abort 查询取消                 | P4         |

### P3 级别（未来规划）

| #   | 优化项             | 说明                       | 预期效果         |
| --- | ------------------ | -------------------------- | ---------------- |
| 1   | **SQL 对比视图**   | 修改前后 SQL 对比显示      | 便于审查变更     |
| 2   | **执行计划可视化** | 树形图展示执行计划         | 直观理解查询性能 |
| 3   | **SQL 性能分析**   | 自动分析慢查询并给出建议   | 提升查询性能     |
| 4   | **批量导入 SQL**   | 支持导入 .sql 文件批量执行 | 提高工作效率     |
| 5   | **SQL 分享**       | 生成 SQL 分享链接          | 团队协作         |

---

## 附录

### 错误处理

所有错误统一使用 `CoreError` 处理:

```typescript
try {
  const result = await sqlExecutionStore.executeSql(panelId, sql, connectionId)
  if (result.error) {
    message.error(result.error)
  }
} catch (error) {
  const errorMsg = error instanceof Error ? error.message : '执行失败'
  message.error(errorMsg)
}
```

### 性能优化

| 优化项          | 实现方式                   |
| --------------- | -------------------------- |
| 代码补全缓存    | 5 分钟 TTL，避免重复请求   |
| Disposable 管理 | 及时清理不需要的补全提供器 |
| 防抖处理        | 验证 500ms，保存 1000ms    |
| 虚拟滚动        | AG Grid 默认启用           |

### 兼容性

| 数据库     | 支持状态    |
| ---------- | ----------- |
| MySQL      | ✅ 完整支持 |
| PostgreSQL | ✅ 完整支持 |
| SQLite     | ✅ 完整支持 |
| DuckDB     | ✅ 完整支持 |

### 已知问题

| 问题                   | 状态    | 说明                                                                     |
| ---------------------- | ------- | ------------------------------------------------------------------------ |
| Monaco Web Worker 警告 | ⚠️ 已知 | `w.removeEventListener is not a function` 是 Monaco 已知问题，不影响功能 |

---

## Tauri 后端命令对齐

### SQL 执行与事务（18 命令，全注册）

| 命令                         | src-tauri                                                                                                             | 前端 query.ts                | 状态     |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------- | ---------------------------- | -------- |
| `execute_sql`                | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L39)  | `executeSql()`               | ✅       |
| `execute_transaction`        | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L74)  | `executeTransaction()`       | ✅       |
| `begin_transaction`          | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L107) | `beginTransaction()`         | ✅       |
| `commit_transaction`         | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L123) | `commitTransaction()`        | ✅       |
| `rollback_transaction`       | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L139) | `rollbackTransaction()`      | ✅       |
| `get_transaction_status`     | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L164) | `getTransactionStatus()`     | ✅       |
| `cancel_sql_query`           | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L154) | `cancelQuery()`              | ✅ 新增  |
| `execute_duckdb_accelerated` | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L306) | `executeDuckDBAccelerated()` | ✅ 新增  |
| `get_sql_history`            | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L189) | `getSqlHistory()`            | ✅       |
| `search_sql_history`         | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L210) | `searchSqlHistory()`         | ✅       |
| `clear_sql_history`          | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L234) | `clearSqlHistory()`          | ✅       |
| `remove_sql_history`         | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L243) | `removeSqlHistory()`         | ✅       |
| `register_external_database` | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L263) | (无前端入口)                 | ⚠️ 缺 UI |
| `create_external_table`      | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L291) | (无前端入口)                 | ⚠️ 缺 UI |

### 新增功能入口

| 功能               | 入口                                                                                                                                              | 快捷键                                                                                                                                  |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| SQL 编辑器全局打开 | `Ctrl+Shift+E`                                                                                                                                    | [WorkbenchView.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/views/WorkbenchView.vue) |
| 生成 SELECT        | 导航树右键表/视图 → "生成 SELECT 语句"                                                                                                            | 复制 + 打开 SQL 编辑器                                                                                                                  |
| 生成 INSERT        | 导航树右键表 → "生成 INSERT 语句"                                                                                                                 | 复制 + 打开 SQL 编辑器                                                                                                                  |
| 生成 UPDATE        | 导航树右键表 → "生成 UPDATE 语句"                                                                                                                 | 复制 + 打开 SQL 编辑器                                                                                                                  |
| 生成 DELETE        | 导航树右键表 → "生成 DELETE 语句"                                                                                                                 | 复制 + 打开 SQL 编辑器                                                                                                                  |
| 代码片段面板       | [SnippetPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SnippetPanel.vue) | 搜索 + 点击插入编辑器                                                                                                                   |

### SQL 解析与格式（6 命令，全注册）

| 命令            | src-tauri                                                                                                                          | 前端 service     | 状态 |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------- | ---- |
| `parse_sql`     | [sql_parser_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_parser_commands.rs#L11) | `parseSql()`     | ✅   |
| `format_sql`    | [sql_parser_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_parser_commands.rs#L17) | `formatSql()`    | ✅   |
| `transpile_sql` | [sql_parser_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_parser_commands.rs#L23) | `transpileSql()` | ✅   |
| `validate_sql`  | [sql_parser_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_parser_commands.rs#L33) | `validateSql()`  | ✅   |
| `split_sql`     | [sql_parser_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_parser_commands.rs#L39) | `splitSql()`     | ✅   |

### 取消链路架构

```
用户点击 X (Statusbar)
  → useSqlExecution.cancelExecution()
    ├─ AbortController.abort()        [JS 终止 Promise]
    └─ queryService.cancelQuery(connId)
      → invoke('cancel_sql_query')
        → ConnectionManager.cancel_query()
          → CancellationToken.cancel()
            → db.query_with_cancel() 立即返回
```

---

## DuckDB 加速执行

### 概述

DuckDB 加速功能利用 DuckDB 内存引擎 + scanner 扩展，对 MySQL / PostgreSQL / SQLite 数据库进行联邦查询加速。

### 扩展离线包

启动时在 APP 数据目录 `{app_data}/duckdb/extensions/` 自动创建扩展目录，并加载 P0 级别扩展：

| 扩展             | 优先级 | 用途                  |
| ---------------- | ------ | --------------------- |
| mysql_scanner    | P0     | MySQL 外部表扫描      |
| postgres_scanner | P0     | PostgreSQL 外部表扫描 |
| sqlite_scanner   | P0     | SQLite 外部表扫描     |
| json             | P1     | JSON 文件读写         |
| parquet          | P1     | Parquet 文件读写      |
| excel            | P1     | Excel 文件读写        |
| httpfs           | P1     | HTTP 文件系统         |
| fts              | P1     | 全文搜索              |

P1 扩展通过 `load_extension_by_name()` 按需加载。

### 数据流

```
用户点击 DuckDB 按钮 (⚡)
  → handleDuckDbExecute() [SqlEditorPanel.vue]
    → ensureConnection(connId)
    → executeDuckDBAccelerated() [useSqlExecution.ts]
      → appDataDir() 获取扩展目录
      → queryService.executeDuckDBAccelerated({ sql, connId, dbType, dataDir })
        → invoke('execute_duckdb_accelerated', ...)
          → [Rust] state.duckdb_engine.lock().await
          → [Rust] DuckDBEngine::init_extensions(conn, dataDir)
            → LOAD P0 scanner 扩展
          → [Rust] engine.execute(sql, ctx)
            → Arrow RecordBatch
          → [Rust] format_arrow_value() → JSON
          → DuckDBAcceleratedResponse
      → storeResult({ columns, rows, ... })
      → QueryResultPanel 渲染
```

### 涉及文件

| 层   | 文件                                                                                                                                                       | 职责                            |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| Rust | [duckdb_engine.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/dbi/engine/duckdb_engine.rs)                                  | DuckDBEngine + 扩展管理         |
| Rust | [state.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/adapters/tauri/state.rs)                                                   | AppState.duckdb_engine          |
| Rust | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L306)                                      | execute_duckdb_accelerated 命令 |
| Rust | [lib.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/lib.rs)                                                                      | 命令注册                        |
| TS   | [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts#L156)                                  | executeDuckDBAccelerated()      |
| TS   | [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts)            | executeDuckDBAccelerated()      |
| Vue  | [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L298) | handleDuckDbExecute() 入口      |

### ATTACH 自动桥接（P0）

`execute_duckdb_accelerated` 命令内置了完整的 ATTACH / DETACH 生命周期，前端仅需传入 `connId` + `sql`：

1. `get_connection_info(connId)` → 获取 `url` + `db_type`
2. 构建 `ATTACH '{url}' AS ext_{name} (TYPE {type})`
3. 初始化 P0 扩展 → 执行 ATTACH
4. 执行用户 SQL
5. 自动 DETACH（best-effort）

---

## 结果集图表可视化

### 概述

查询结果支持 4 种 ECharts 图表，通过结果面板左侧视图侧边栏切换。

### 图表类型

| 类型   | 图标           | 说明         |
| ------ | -------------- | ------------ |
| 柱状图 | `BarChart3`    | 分类数据对比 |
| 折线图 | `LineChart`    | 趋势变化     |
| 饼图   | `PieChart`     | 占比分析     |
| 散点图 | `ScatterChart` | 相关性分析   |

### 交互流程

1. 执行 SQL → 查看结果
2. 点击左侧 `BarChart3` 按钮 → 切换到图表视图
3. 选择图表类型（bar/line/pie/scatter）
4. 选择 X 轴列 + Y 轴列
5. 图表自动渲染

### 涉及文件

| 文件                                                                                                                                                                  | 职责             |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| [DataVisualizationPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/DataVisualizationPanel.vue) | ECharts 渲染组件 |
| [QueryResultPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/QueryResultPanel.vue)             | 视图切换入口     |
| [result.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/types/result.ts#L3)                                            | `ViewMode` 类型  |

---

## Schema 自动补全

### 概述

Monaco Editor 的 `CompletionItemProvider` 基于当前连接的 schema 信息提供表和列名补全。

### 工作流程

```
用户输入 '.'
  → registerDatabaseCompletionProvider() 触发
    → getDatabaseSchema(connId, database, schema, dbType)
      → 查询 information_schema.tables (或 sqlite_master)
      → 遍历每张表查询 columns
      → 缓存结果 (TTL 可配置)
    → 生成 CompletionItem[] (表名 + 表.列名)
  → Monaco 渲染补全列表
```

### 涉及文件

| 文件                                                                                                                                                       | 职责                   |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- |
| [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts#L140)       | 补全服务注册           |
| [useConnectionBinding.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useConnectionBinding.ts)  | `currentDatabase` 提供 |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L458) | 补全注册调用点         |

---

## 多语句批量执行

工具栏的 `ListChecks` "批量执行所有语句" 按钮将编辑器中的 SQL 按 `;` 拆分后逐条执行，每条语句的结果自动创建独立标签页。

| 场景     | 按钮               | 行为                               |
| -------- | ------------------ | ---------------------------------- |
| 单条语句 | 批量执行 / Execute | 等同于 Execute，显示一个结果标签页 |
| 多条语句 | 批量执行           | 拆分 → 逐条执行 → N 个标签页       |

**错误处理：** 某条语句失败不影响后续语句执行，最终汇总显示成功/失败计数。

---

## DuckDB SQL 自动重写

当使用 ⚡ DuckDB 加速时，系统自动将无前缀表名加上 ATTACH 名称：

| 用户输入                              | 自动重写为                                                  |
| ------------------------------------- | ----------------------------------------------------------- |
| `SELECT * FROM users`                 | `SELECT * FROM ext_MyConn.users`                            |
| `SELECT * FROM orders o JOIN users u` | `SELECT * FROM ext_MyConn.orders o JOIN ext_MyConn.users u` |
| `INSERT INTO users VALUES (1)`        | `INSERT INTO ext_MyConn.users VALUES (1)`                   |

**安全规则：**

- 已有 `.` 前缀的表名（如 `db.users`）不重写
- SQL 关键字（WHERE/ON/SET/...）不误重写

---

## EXPLAIN 执行计划

工具栏的 `FileSearch` "执行计划" 按钮包装当前 SQL 为 `EXPLAIN {sql}` 并执行，结果显示为独立标签页，标题 "执行计划"。

---

## Monaco 代码折叠

### 概述

SQL 编辑器支持基于语义的代码折叠，超越 Monaco 默认的缩进/括号折叠。

### 折叠场景

| 场景              | 示例                     | 折叠范围     |
| ----------------- | ------------------------ | ------------ |
| BEGIN...END 块    | `BEGIN ... END`          | 从头到尾     |
| 嵌套子查询        | `(SELECT ... FROM ...)`  | 括号内整段   |
| CTE 定义          | `WITH cte AS ( ... )`    | AS 后括号内  |
| 多行注释          | `/* ... */`              | 注释范围     |
| CREATE TABLE 定义 | `CREATE TABLE t ( ... )` | 括号内列定义 |

### 实现

- `useMonacoEditor.ts` 编辑器选项中启用 `foldingStrategy: 'auto'` + `showFoldingControls: 'always'`
- `sql-editor-service.ts` 中 `registerSqlFoldingProvider()` 自定义 `FoldingRangeProvider`
- `SqlEditorPanel.vue` 在 onMounted 注册，onBeforeUnmount 注销（Disposable 管理）
- 识别算法：行扫描 + Bracket/BEGIN/Comment Stack 追踪 → 生成 FoldingRange[]

---

## SQL 代码收藏管理

### 概述

用户可将当前编辑器中的 SQL 保存为自定义代码片段，在片段面板中统一管理。

### 操作流程

1. **保存收藏**：点击工具栏 Star ⭐ 按钮 → 当前 SQL / 选中 SQL 保存至 localStorage
2. **查看收藏**：打开片段面板（SnippetPanel）→ 分类"收藏"下可见
3. **插入收藏**：点击片段项 → 插入到编辑器光标位置
4. **删除收藏**：hover 片段项 → 右侧出现 × 按钮 → 点击删除

### 实现

- `EditorToolbar.vue` — Star 按钮 + `saveSnippet` emit
- `SqlEditorPanel.vue` — `handleSaveSnippet()` 将编辑内容通过 `addCustomSnippet()` 保存
- `SnippetPanel.vue` — 自定义片段项 × 删除按钮（hover opacity 过渡动画）
- i18n 键：`saveSnippet`（保存为收藏）、`snippetSaved`（已保存到收藏）

---

## 查询参数化 :param 绑定

### 概述

SQL 编辑器支持命名参数占位符语法 `:param_name`，执行时自动检测并弹出参数绑定窗口。

### 操作流程

1. 编写 SQL，使用 `:param_name` 作为占位符，如 `SELECT * FROM users WHERE id = :user_id`
2. 点击 Execute → 系统自动检测到 `:param` 占位符
3. 弹出 ParamBindingModal → 为每个参数填写值
4. 确认 → 占位符替换为实际值 → 执行 SQL

### 安全

- 值中的单引号 `'` 自动转义为 `''`，防止 SQL 注入

### 涉及文件

| 文件                                                                                                                                                        | 职责                               |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------- |
| [ParamBindingModal.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/ParamBindingModal.vue) | 参数绑定弹窗（NInput 动态表单）    |
| [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts#L561)        | detectParams() + bindParams()      |
| [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts)             | checkForParams() + buildBoundSql() |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L310)  | handleExecute() 拦截参数           |

---

## 表数据 Inline Edit

### 概述

结果集表格支持双击单元格直接编辑值，修改后底部状态栏出现 Save / Cancel 按钮。

### 编辑流程

1. 执行 SQL → 查看结果表格
2. 双击单元格 → 进入编辑模式 → 修改值 → Enter 确认
3. 底部状态栏出现 Save / Cancel 按钮
4. 点击 Save → 生成 UPDATE SQL → 通过 `save_cell_update` 后端命令执行
5. 点击 Cancel → 恢复原始值

### 实现

- `QueryResultPanel.vue` — `onCellValueChanged` 追踪 dirty cells
- `QueryResultPanel.vue` — `handleSave` 通过 `saveCellUpdate` 生成 UPDATE
- `QueryResultPanel.vue` — `handleCancel` 恢复所有修改
- `result_commands.rs` — `save_cell_update` Rust 命令（`row_identity` + `table_name` → UPDATE）
- `editable: true` — AG Grid 的 defaultColDef 已启用编辑

### 涉及文件

| 文件                                                                                                                                                           | 职责                      |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| [QueryResultPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/QueryResultPanel.vue#L754) | 编辑事件 + Save/Cancel    |
| [result-analysis.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/services/result-analysis.ts#L454)              | saveCellUpdate 前端封装   |
| [result_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/result_commands.rs#L58)                                     | save_cell_update 后端命令 |

---

## SQL 执行错误位置高亮

### 概述

SQL 执行出错时，系统自动解析错误消息中的行列号，在 Monaco 编辑器中标记错误行并跳转到错误位置。

### 支持的错误格式

| 数据库           | 格式示例                     | 解析                    |
| ---------------- | ---------------------------- | ----------------------- |
| MySQL/PostgreSQL | `ERROR at line 5, column 12` | ✅                      |
| 通用 SQL         | `line 3 ... column 8`        | ✅                      |
| SQLite           | `near "xxx" at line 2`       | ✅                      |
| DuckDB           | `at position: 156`           | ✅ 字符偏移自动换算行列 |

### 行为

1. 执行 SQL 出错 → `currentResultData.error` 变化
2. `SqlEditorPanel.vue` watch 检测到 error → 调用 `setErrorMarker()`
3. Monaco 编辑器标记整行错误（红色波浪线）
4. 自动 `revealLineInCenter` 滚动到错误行
5. 光标定位到错误位置
6. 下次成功执行 → 自动 `clearErrorMarkers()`

### 涉及文件

| 文件                                                                                                                                                       | 职责                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts#L560)       | parseErrorPosition/setErrorMarker/clearErrorMarkers |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L740) | watch error → set/clear markers                     |

---

## Monaco Minimap 切换

工具栏 Map 按钮可一键切换 Monaco 小地图（Minimap）的开/关。

- 初始状态：**开启**
- 点击切换：`showMinimap.value = !showMinimap.value` → `setMinimap(enabled)`
- `useMonacoEditor.ts` → `setMinimap()` 封装 `updateOptions({ minimap: { enabled } })`

---

## 编辑器设置面板

工具栏 Settings 齿轮按钮弹出设置面板，实时调整编辑器参数。

| 设置项               | UI 控件      | 范围   | 初始值 |
| -------------------- | ------------ | ------ | ------ |
| 字号 (Font Size)     | NSlider      | 10-28  | 14     |
| 缩进 (Tab Size)      | NInputNumber | 1-8    | 2      |
| 自动换行 (Word Wrap) | NSwitch      | on/off | ✅ on  |
| 行号 (Line Numbers)  | NSwitch      | on/off | ✅ on  |

所有改动通过 `useMonacoEditor.updateOptions()` 实时生效。

### 涉及文件

| 文件                                                                                                                                                  | 职责                                                                       |
| ----------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| [useMonacoEditor.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useMonacoEditor.ts)       | setFontSize/setWordWrap/setLineNumbers/setTabSize/setMinimap/setFontFamily |
| [EditorToolbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorToolbar.vue)   | Map + Settings 按钮                                                        |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue) | NPopover 面板 + toggle/minimap 函数                                        |

---

## 编辑器设置全局持久化

### 概述

编辑器设置面板的全部 6 项参数（字号、缩进、自动换行、行号、Minimap、字体）通过全局配置系统（`useAppStore`）持久化到 `tauri-plugin-store` 的 JSON 文件中，跨应用会话保持。

### 配置读链路

编辑器启动时从 `appStore.effectiveEditorSettings` 读取配置，合并全局默认值 + 项目级覆盖：

```typescript
// SqlEditorPanel.vue — 初始化时读取全局配置
const editorFontSize = ref(appStore.effectiveEditorSettings.fontSize) // 默认 14
const editorTabSize = ref(appStore.effectiveEditorSettings.tabSize) // 默认 2
const editorWordWrap = ref(appStore.effectiveEditorSettings.wordWrap) // 默认 true
const showMinimap = ref(appStore.effectiveEditorSettings.minimap) // 默认 true
const editorLineNumbers = ref(appStore.effectiveEditorSettings.lineNumbers) // 默认 true
const editorFontFamily = ref(appStore.effectiveEditorSettings.fontFamily) // 默认 'Cascadia Code', ...

// 编辑器创建后立即应用配置
createEditor()
setFontSize(editorFontSize.value)
setTabSize(editorTabSize.value)
setWordWrap(editorWordWrap.value)
setLineNumbers(editorLineNumbers.value)
setMinimap(showMinimap.value)
setFontFamily(editorFontFamily.value)
```

### 配置写链路

每次用户修改任意设置项，立即通过 `persistEditorSettings()` 持久化：

```typescript
function buildEditorSettingsPayload(): EditorSettings {
  return {
    fontSize: editorFontSize.value,
    tabSize: editorTabSize.value,
    wordWrap: editorWordWrap.value,
    minimap: showMinimap.value,
    lineNumbers: editorLineNumbers.value,
    fontFamily: editorFontFamily.value,
  }
}

function persistEditorSettings(): void {
  appStore.saveConfig('editorSettings', buildEditorSettingsPayload(), 'global').catch(() => {})
}
```

| 操作                  | 持久化时机      | 对应函数             |
| --------------------- | --------------- | -------------------- |
| 拖动字号 NSlider      | `@update:value` | `applyFontSize()`    |
| 修改缩进 NInputNumber | `@update:value` | `applyTabSize()`     |
| 切换自动换行 NSwitch  | `@update:value` | `applyWordWrap()`    |
| 切换行号 NSwitch      | `@update:value` | `applyLineNumbers()` |
| 切换 Minimap Map 按钮 | `@click`        | `toggleMinimap()`    |
| 修改字体 NInput       | `@update:value` | `applyFontFamily()`  |

### 存储结构

设置保存为全局配置（scope: `'global'`），存在于 `tauri-plugin-store` 的 JSON 文件中：

```json
{
  "editorSettings": {
    "fontSize": 16,
    "tabSize": 2,
    "wordWrap": true,
    "minimap": false,
    "lineNumbers": true,
    "fontFamily": "'JetBrains Mono', 'Fira Code', monospace"
  }
}
```

### 跨面板同步

`watch(effectiveEditorSettings)` 监听外部配置变更（如从全局设置页修改），自动同步到当前编辑器实例，保证多编辑器面板之间设置一致。

### 默认值来源

编辑器设置的默认值定义在 [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts) 的 `CONFIG_REGISTRY.editorSettings.default`：

| 字段        | 默认值                                                |
| ----------- | ----------------------------------------------------- |
| fontSize    | 14                                                    |
| tabSize     | 2                                                     |
| wordWrap    | true                                                  |
| minimap     | true                                                  |
| lineNumbers | true                                                  |
| fontFamily  | `'Cascadia Code', 'Fira Code', 'Consolas', monospace` |

### 涉及文件

| 文件                                                                                                                                                  | 职责                                              |
| ----------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)                                                              | EditorSettings 接口 + CONFIG_REGISTRY 默认值      |
| [useAppStore.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/useAppStore.ts)                                                    | effectiveEditorSettings 合并计算 + saveConfig API |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue) | 读写链路 + persistEditorSettings                  |
| [useMonacoEditor.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useMonacoEditor.ts)       | setFontFamily 等 6 个设置函数                     |

---

## Monaco 右键菜单定制

### 概述

Monaco 编辑器内置右键菜单已扩展两个数据库工作台专属操作。

| 菜单项            | 位置                 | 功能                                         |
| ----------------- | -------------------- | -------------------------------------------- |
| **执行选中 SQL**  | 顶部 navigation 分组 | 触发 `handleExecute()`，等效 Ctrl+Enter      |
| **复制为 VALUES** | 9_cutcopypaste 分组  | 选中内容包装为 `VALUES (...)` 并复制到剪贴板 |

### 实现

- `useMonacoEditor.ts` → `registerContextActions()` 批量注册 Monaco `IActionDescriptor`，返回 `IDisposable[]`
- `SqlEditorPanel.vue` → onMounted 注册，onBeforeUnmount dispose
- 与 Monaco 原生上下文菜单无缝融合

---

## Scratchpad 退出自动保存

### 概述

关闭草稿（Scratchpad）模式编辑器标签页时，自动将未保存内容写入磁盘。

### 行为

```
关闭标签页 → onBeforeUnmount
  → isScratchpadMode && isDirty && scratchpadRelativePath
    → invoke('save_scratchpad_file', { content })
    → invoke('update_scratchpad_file_meta', { ... })
```

静默执行，不阻塞关闭流程，保存失败不影响 UI。

---

## SQL 历史双击重执行

### 概述

SQL 历史面板（SqlHistoryPanel）中双击任意历史条目，即刻在上方编辑器重新执行该 SQL。

### 行为

```
双击历史条目
  → SqlHistoryPanel 发送 CustomEvent('sql-history-re-execute', { sql, connectionId })
    → SqlEditorPanel 监听到事件
      → setValue(sql)  // 写入编辑器
      → 自动切换连接（如果来源连接不同）
      → handleExecute()  // 立即执行
```

### 涉及文件

| 文件                                                                                                                                                    | 职责                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| [SqlHistoryPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlHistoryPanel.vue) | @dblclick → reExecuteHistory() |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue)   | 监听 sql-history-re-execute    |

---

## Welcome 欢迎页快速连接

### 概述

编辑器空白欢迎页展示最近使用的数据库连接列表（最多 5 条），点击直接连接并清空编辑器。

### 涉及文件

| 文件                                                                                                                                                  | 职责                         |
| ----------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------- |
| [EditorWelcome.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorWelcome.vue)   | 连接列表渲染 + @connect emit |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue) | handleWelcomeConnect()       |
