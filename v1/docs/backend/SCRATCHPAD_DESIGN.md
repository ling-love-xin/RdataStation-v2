# 草稿箱 (Scratchpad) 设计方案

> 版本：v3.15
> 创建日期：2026-05-07
> 最后更新：2026-05-13
> 状态：✅ v3.15 — P0 操作闭环三件套：剪切移动/正则替换/Diff对比 (MVP 达成)

---

## 一、核心定位

草稿箱是 RdataStation 中一个跟随项目存在的临时文件管理区域，用于存放尚未决定归属的探索性文件（SQL 片段、Python 脚本、测试数据等）。

### 核心特征

| 特征                 | 说明                                               |
| -------------------- | -------------------------------------------------- |
| **物理跟随项目**     | 文件存储在 `{项目}/.scratchpad/` 中，跟随项目迁移  |
| **逻辑暂存**         | 不属于项目核心资产，不写入 `project.meta.sqlite`   |
| **临时性与实验性**   | 存放不成熟的验证脚本、随手写的 SQL、测试用数据文件 |
| **用户主动决定归属** | 文件是否从草稿箱"提升"到项目资产区，完全由用户决定 |
| **个人化**           | 草稿文件是个人的、临时的，不与团队成员共享         |

---

## 二、文件存储结构

```
项目目录/
├── .scratchpad/              # 草稿箱（隐藏目录，系统管理）
│   ├── .scratchpad.json      # 草稿箱配置（外部引用列表）
│   ├── 临时订单分析.sql
│   ├── 用户留存探索.py
│   └── test_data_sample.csv
```

### 配置文件 (.scratchpad.json)

```json
{
  "external_references": [
    {
      "alias": "下载数据",
      "path": "/home/user/Downloads/data",
      "created_at": "2026-05-07T10:30:00Z"
    }
  ],
  "file_meta": {
    "临时订单分析.sql": {
      "last_connection_id": "conn-mysql-prod-001",
      "last_executed_at": "2026-05-08T14:20:00Z"
    }
  }
}
```

---

## 三、页面布局

```
┌──────────────────────────────┐
│ 草稿箱 (Scratchpad)          │  ← dockview tab 标题
├──────────────────────────────┤
│ [新建] [新建文件夹] [导入] [引用] [🔍] │  ← 工具栏
├──────────────────────────────┤
│ ▼ 📁 本地草稿                  │
│   ├ 📊 data/                 │  ← 真实子目录
│   │   ├ 📜 report.sql        │
│   │   └ 🐍 transform.py      │
│   ├ 📜 临时订单分析.sql      │
│   └ [输入文件名...]          │  ← 内联创建 (选中文件夹后)
│ ▼ 🔗 外部引用                  │
│   └ ⚡ 数据源 (D:\data\)      │
└──────────────────────────────┘
```

### 设计要点

- 支持文件夹嵌套（最多 4 层），`ScratchpadEntry.children` 递归构建真正的目录树
- 按"本地草稿"和"外部引用"分组显示
- 使用 dockview-vue 的 paneview 实现
- 文件图标根据后缀自动识别
- 搜索结果实时过滤，匹配文件名 + 引用别名/路径
- **内联创建**：选中文件夹 → 点击新建 → 在树中直接输入文件名（无模态弹窗中断）
- **搜索上下文**：匹配行显示前后 2 行上下文，灰色字体区分
- **子目录创建**：选中文件夹后新建，文件创建在选中目录下而非根目录

---

## 四、核心交互

| 操作                        | 说明                                                                                                                   |  状态   |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------- | :-----: |
| **新建文件**                | 选中文件夹后点击 [+新建] 或在根目录新建，内联输入文件名，Enter 提交，Escape 取消                                       |   ✅    |
| **新建文件夹**              | 选中文件夹后点击 [新建文件夹]，内联输入文件夹名创建子目录                                                              |   ✅    |
| **打开文件**                | 双击文件，根据后缀自动选择合适的编辑器（.sql → SQL 编辑器），自动恢复上次使用的连接                                    |   ✅    |
| **编辑保存**                | 编辑后 Ctrl+S 保存回 `.scratchpad/`（原子写入）                                                                        |   ✅    |
| **删除文件**                | 右键删除，移入 `.trash/` 回收站（软删除，可恢复）                                                                      |   ✅    |
| **重命名**                  | 右键重命名或选中后按 F2                                                                                                |   ✅    |
| **导入外部文件**            | 通过系统文件对话框或拖放文件到面板，复制到 `.scratchpad/`                                                              |   ✅    |
| **链接外部目录**            | 将外部目录添加到"外部引用"列表（不复制文件，只记录路径引用）                                                           |   ✅    |
| **回收站管理**              | list/restore/empty，面板折叠区域含恢复和清空按钮                                                                       |   ✅    |
| **Python 编辑**             | 双击 `.py`，在代码编辑器打开，Monaco Python 语法高亮，Ctrl+S 保存回草稿箱                                              |   ✅    |
| **防重复 Tab**              | 同一草稿文件不重复打开 editor tab，自动聚焦已有面板                                                                    |   ✅    |
| **文件监控**                | `notify` crate 监控 `.scratchpad/` 目录，文件变更自动刷新面板树                                                        |   ✅    |
| **提升为分析资源**          | 右键 → 将草稿提升为正式分析资源，可选保留/删除原稿                                                                     |   ✅    |
| **键盘导航**                | ↑↓ 切换选中、Enter 打开、F2 重命名、Delete 删除、Ctrl+N 新建                                                           |   ✅    |
| **重命名反馈**              | 提交时输入框禁用 + CSS 旋转 spinner，防止重复提交                                                                      |   ✅    |
| **提升事件联动**            | promote 完成后 emit `analytics-resource-changed` 事件通知面板刷新                                                      |   ✅    |
| **文件排序**                | 工具栏提供按名称/大小/修改时间排序按钮，点击切换升序/降序，图标实时反馈                                                |   ✅    |
| **内容搜索行上下文**        | 搜索结果面板显示文件名、匹配行号、匹配行内容 + **前后 2 行上下文**，最多预览 5 行                                      |   ✅    |
| **新建文件夹**              | 工具栏"新建文件夹"按钮 + 模态框输入名称                                                                                |   ✅    |
| **搜索结果点击跳转**        | 点击搜索结果文件名→打开文件，点击行→打开文件并跳转到对应行                                                             |   ✅    |
| **修改时间显示**            | TreeNode 显示相对时间（分钟/小时/天数），7天内有效                                                                     |   ✅    |
| **Toast 操作反馈**          | `createDiscreteApi(['message'])` 创建/删除/重命名/导入/提升/清空回收站成功后 toast 提示                                |   ✅    |
| **大小写搜索**              | 内容搜索模式新增 Aa 按钮切换大小写敏感，Rust `case_sensitive` 参数逐行条件匹配                                         |   ✅    |
| **折叠/展开全部**           | 本地文件组头新增"展开全部"/"折叠全部"按钮，操作 `expandedKeys` Set                                                     |   ✅    |
| **搜索文本高亮**            | `highlightMatch()` 将匹配文本包裹 `<mark class="search-hl">`，黄色背景高亮                                             |   ✅    |
| **最近打开**                | 打开文件时记录路径到 `recentFiles`（内存，最大5），顶部可折叠"最近打开"区域                                            |   ✅    |
| **空状态引导**              | 无本地文件时显示 `FolderOpen` 大图标 + 标题 + 引导文本 + 新建/导入双按钮                                               |   ✅    |
| **多选（Ctrl/Shift 点击）** | Ctrl+点击切换选中，Shift+点击范围选中，右键菜单自适应单/多选                                                           |   ✅    |
| **批量删除**                | 多选后右键/Delete 批量删除，confirm 确认，toast 反馈                                                                   |   ✅    |
| **复制/粘贴文件**           | 右键"复制"到 clipboard，右键"粘贴"生成 `_copy` 副本                                                                    |   ✅    |
| **Ctrl+A 全选**             | 键盘 Ctrl+A 选中当前所有可见条目                                                                                       |   ✅    |
| **新建文件模板**            | 新建文件时可选 SQL/JSON/Markdown/Python 模板，自动填充预设内容并设置后缀                                               |   ✅    |
| **拖放文件到编辑器**        | 拖拽草稿箱文件节点到编辑器区，自动插入文件内容到光标位置                                                               |   ✅    |
| **删除撤销**                | 单文件删除后底部弹出撤销栏（5秒自动消失），点击"撤销"从回收站恢复                                                      |   ✅    |
| **搜索安全加固**            | 超过 10MB 的文件跳过搜索；结果最多 500 条截断；前端通知跳过/截断信息                                                   | ✅ → ♻️ |
| **流式搜索**                | 大文件不再跳过，改用 `BufReader::lines()` 逐行流式读取，内存恒定 ~8KB；30s 超时保护                                    |   ✅    |
| **质量加固**                | 14 处 `unwrap_or_*` 全清除 → `?` 错误传播；`futures::block_on` → `Handle::block_on`；`Drop` 确保 watcher 清理          |   ✅    |
| **虚拟滚动**                | 超 50 条目自动启用虚拟滚动，仅渲染可视区 + overscan 8 行（ROW_HEIGHT=28px），ResizeObserver 动态容器高度               |   ✅    |
| **脏状态跟踪**              | 编辑后文件名前显示 ● 脏点；Ctrl+S 后消失；外部修改触发冲突对话框（重新加载/忽略）                                      |   ✅    |
| **懒加载**                  | 初始仅加载顶层条目（depth=0），展开文件夹时按需加载子目录，减少初始 IO 和内存占用                                      |   ✅    |
| **文件移动/剪切**           | 右键"剪切"→ clipboardMode='cut' → 粘贴到目标文件夹 → fs::rename 物理移动 → 5秒撤销栏                                   |   ✅    |
| **正则搜索**                | `.*` 按钮切换正则模式 → `Regex` 前端验证 → `replace_in_file(is_regex=true)` 后端 → 外层 `regex` crate                  |   ✅    |
| **搜索替换**                | 全文搜索 → 替换栏输入 → 预览计数 → 全部替换 → 原子写回 → 刷新搜索结果 → 替换历史追踪                                   |   ✅    |
| **Diff 对比**               | 外部冲突 → "查看差异"按钮 → `similar::TextDiff` 行级 diff → 800px 弹窗 → 红/绿标记 → 接受右侧                          |   ✅    |
| **初始化 fallback**         | `ScratchpadState` 存储 `last_project_path`，`get_store()` 未初始化时自动 fallback 重建                                 |   ✅    |
| **主题打通**                | 全组件 CSS 变量化：`font-size`/`spacing`/`color` 统一使用 `var(--font-size-*)`/`var(--spacing-*)`/`var(--color-*)`     |   ✅    |
| **一致性治理**              | 移除幽灵 `children` 字段、修复 `restoreFromTrash` 返回值、补齐 locale 时间键与 retry、`init` 走 API 封装、文档数字修正 |   ✅    |

---

## 五、与项目的关系

| 维度         | 说明                                                                 |
| ------------ | -------------------------------------------------------------------- |
| **物理存储** | 跟随项目，存储在 `{项目}/.scratchpad/` 中                            |
| **逻辑归属** | 不属于项目资产管理视图，不写入 `project.meta.sqlite`                 |
| **迁移行为** | 跟随项目目录迁移（因为是项目目录下的物理文件）                       |
| **资产化**   | 暂不引入"提升"机制，草稿就是草稿，用户自行决定是否手动复制到正式目录 |

---

## 六、技术架构

```
┌─────────────────────────────────────────────┐
│              Frontend (Vue 3 + TS)           │
│  ┌────────────┐  ┌───────────┐              │
│  │ Panel      │  │ TreeNode  │              │
│  │ (dockview) │  │ (递归组件) │              │
│  └─────┬──────┘  └─────┬─────┘              │
│        └───────────────┘                     │
│  ┌─────────────────────────────┐            │
│  │     use-scratchpad.ts       │            │
│  │   (状态管理 + 业务逻辑)      │            │
│  └─────────────┬───────────────┘            │
│  ┌─────────────┴───────────────┐            │
│  │       scratchpad-api.ts      │            │
│  │     (27 个 Tauri IPC 封装)    │            │
│  └─────────────┬───────────────┘            │
├────────────────┼────────────────────────────┤
│              Tauri IPC                       │
├────────────────┼────────────────────────────┤
│            Backend (Rust)                    │
│  ┌─────────────┴───────────────┐            │
│  |   scratchpad_commands.rs    |            │
│  |   (26 个 #[tauri::command])   |            │
│  └─────────────┬───────────────┘            │
│  ┌─────────────┴───────────────┐            │
│  │     core/scratchpad/         │            │
│  │     ├── models.rs            │            │
│  │     ├── state.rs             │            │
│  │     └── store.rs             │            │
│  └──────────────────────────────┘            │
└──────────────────────────────────────────────┘
```

---

## 七、SQL 编辑器集成路线（v2.1 核心）

### 7.1 为什么集成而非独立？

草稿箱 SQL 文件不是"只能写一句话的便签"，用户完全可能在草稿箱里写几十行 SQL（DDL + DML + SELECT），连上一个连接后完整执行。草稿箱 SQL 需要的执行能力与正式 SQL 编辑器高度重合。

| 需求                                   | 草稿箱 SQL 文件 | 正式 SQL 编辑器 |
| -------------------------------------- | :-------------: | :-------------: |
| Monaco 编辑（语法高亮、快捷键）        |       ✅        |       ✅        |
| 完整执行引擎（单语句/多语句/选中执行） |       ✅        |       ✅        |
| 结果展示（AG Grid + 排序/筛选/分页）   |       ✅        |       ✅        |
| 连接管理（自动建连、ensureConnection） |       ✅        |       ✅        |
| 方言高亮 / 数据库补全                  |       ❌        |       ✅        |
| 方言转换 / 执行计划 / DuckDB 加速      |       ❌        |       ✅        |
| localStorage 草稿缓存                  |       ❌        |       ✅        |
| 执行历史记录                           |       ❌        |       ✅        |

### 7.2 集成方式：精简模式（Scratchpad Mode）

让 `SqlEditorPanel.vue` 支持一个 **"草稿箱文件模式"**。在该模式下：

- **编辑器标题** 显示草稿文件名（如 `📜 临时订单分析.sql`）
- **连接管理** 完全保留——用户需要手动选择连接来执行
- **快捷键 Ctrl+S** 保存回 `.scratchpad/` 文件路径（原子写入）
- **关闭/禁用**：方言高亮、数据库补全、方言转换弹窗、DuckDB 加速、执行计划
- **替换持久化**：localStorage 草稿 → `.scratchpad/` 文件读写

### 7.3 新增参数

```typescript
// src/shared/types/sql.ts 新增字段
export interface SqlEditorParams {
  connectionId?: string
  databaseName?: string
  initialSql?: string
  panelId?: string
  schema?: string

  // v2.2 草稿箱模式字段
  scratchpadRelativePath?: string // 相对于 .scratchpad/ 的文件路径，非空 = 草稿箱模式
  scratchpadFileName?: string // 显示用的文件名（如 "临时订单分析.sql"）
}
```

### 7.4 数据流（草稿箱文件模式）

```
用户双击草稿箱 .sql 文件
       │
       ▼
openFileInEditor(entry)
  ├── readScratchpadFile(entry.path) → 文件内容
  ├── 创建 SqlEditorPanel (dockview floating/center)
  │      params: { scratchpadFilePath, scratchpadFileName, initialSql }
  │
  ▼
SqlEditorPanel (scratchpad mode)
  ├── 初始化编辑器（Monaco），加载文件内容
  ├── 状态栏：显示文件名 + 连接选择器
  ├── Ctrl+S → saveScratchpadFile(path, editor.getValue())
  ├── 用户选择连接 → ensureConnection → 执行 SQL
  └── 结果展示（内嵌 QueryResultPanel）
```

### 7.5 需要改动的文件

| 文件                      | 改动                                                              |
| ------------------------- | ----------------------------------------------------------------- |
| `src/shared/types/sql.ts` | `SqlEditorParams` 新增 `scratchpadFilePath`、`scratchpadFileName` |
| `ScratchpadPanel.vue`     | `openFileInEditor` 逻辑打通——调用 dockview API 创建编辑器面板     |
| `SqlEditorPanel.vue`      | `scratchpadFilePath` 非空时启用精简模式                           |

> **不改动的文件**：Rust 后端完全不需要变——`read/save_scratchpad_file` 已有，`execute_sql` 已有。

---

## 七点五、文件元数据层（v2.3）

草稿箱为每个文件记录元数据，存储在 `.scratchpad.json` 的 `file_meta` 字段中。当前记录的元数据包括：

- `last_connection_id`：文件上次执行 SQL 时使用的数据库连接 ID
- `last_executed_at`：上次执行时间

这使得用户在再次打开 SQL 文件时，编辑器能自动恢复到上次使用的连接，提升使用连贯性。

---

## 八、设计决策

| 事项                           | 决策 | 理由                                                                                                      |
| ------------------------------ | ---- | --------------------------------------------------------------------------------------------------------- |
| 草稿不进 SQLite                | ✅   | 保持临时性，零管理负担                                                                                    |
| 不预设文件后缀                 | ✅   | 用户输入完整文件名，自由灵活                                                                              |
| 暂不引入"提升"机制             | ✅   | 保持简洁；analytics_resource 已预留"出口"                                                                 |
| 删除无确认但软删除             | ✅   | 移入 `.trash/`，避免误删，可恢复                                                                          |
| 外部引用用 alias               | ✅   | 避免存储重复，解决"下载目录临时数据"场景                                                                  |
| 递归最多 4 层                  | ✅   | 防止复杂嵌套超出草稿定位                                                                                  |
| 路径遍历防护                   | ✅   | `resolve_path` 三重校验                                                                                   |
| 编辑器集成而非独立             | ✅   | SQL 编辑+执行能力高度重合，独立实现是重复建设                                                             |
| 精简模式而非完整模式           | ✅   | 草稿箱不需要方言高亮/补全/转换/DuckDB 加速                                                                |
| 文件监控自动刷新               | ✅   | `notify` crate 后台线程监控，Tauri event 推送面板刷新                                                     |
| 提升机制命令在 scratchpad      | ✅   | 触发源于草稿箱，跨模块访问 AnalyticsResourceState                                                         |
| 键盘导航在面板级实现           | ✅   | document keydown listener 统一处理，保持 TreeNode 纯展示                                                  |
| 文件排序在面板 computed 实现   | ✅   | `filteredLocalEntries` 对 `[...entries].sort()` 按 sortBy/sortOrder                                       |
| 搜索行上下文展示               | ✅   | `SearchMatch` 结构体（file+line_number+line_content），Map 分组，面板独立区域展示                         |
| 虚拟滚动（>50条目自动切换）    | ✅   | `ROW_HEIGHT=28px`，`OVERSCAN=8`，`visibleTreeEntries` 切片，`ResizeObserver` 容器高度，`<50` 条目全量渲染 |
| 脏状态跟踪（Set + visual dot） | ✅   | `dirtyFiles: Set<string>` 内存跟踪，与文件监控 modify 事件联动检测外部冲突                                |
| 懒加载（depth=0，展开时加载）  | ✅   | 初始仅顶层条目，`handleToggleExpand` 检测 `children===null` 时调用 `list_scratchpad_directory` 按需加载   |
| 文件移动（fs::rename + undo）  | ✅   | `move_entry(from, to_parent)` → `fs::rename` + `file_meta` 迁移 → 5秒撤销栏 `handleMoveUndo`              |
| 正则搜索（regex crate）        | ✅   | `.*` 按钮 → `Regex::new` 前端验证 → `replace_in_file(is_regex=true)` 后端 regex replace_all               |
| 搜索替换（OCR 闭环）           | ✅   | `replace_scratchpad_content` 原子写回 → 搜索结果刷新 → `replaceHistory` 追踪最近 20 条                    |
| Diff 对比（similar crate）     | ✅   | `similar::TextDiff::from_lines` 行级差异 → `DiffResult { lines: Vec<DiffLine> }` → NModal 800px 弹窗      |

---

## 九、待办事项

### 已完成 ✅

- [x] 右键菜单完整实现（打开/重命名/删除/复制路径/展开折叠）
- [x] 外部引用右键菜单（移除引用/打开位置）
- [x] 导入文件系统对话框集成（@tauri-apps/plugin-dialog）
- [x] 添加外部引用对话框（含浏览按钮）
- [x] 外部引用失效检测（UI 标记"已失效"）
- [x] 文件夹展开/折叠（ChevronDown/ChevronRight 切换）
- [x] F2 内联重命名（输入框自动聚焦+全选）
- [x] Delete 键删除选中条目
- [x] Ctrl+N 新建草稿快捷键
- [x] 新建文件对话框（输入框自动聚焦 + Enter 确认）
- [x] 搜索过滤（文件名 + 引用别名/路径）
- [x] 路径安全合规（resolve_path 中 unwrap 替换为 CoreError 错误返回）
- [x] 大文件检查（50MB 限制，前端 + 后端双重校验）
- [x] 外部引用打开系统管理器（opener crate 集成，右键"打开位置"）
- [x] 右键菜单溢出检测（clampToViewport 防止超出屏幕）
- [x] 去重 isRefInvalid（统一到 composable，panel 不再重复）

### v2.1 已完成 ✅

- [x] **打通 openFileInEditor** — 双击 `.sql` 文件读取内容 → 派发 `open-sql-editor` 事件
- [x] **SqlEditorPanel 精简模式** — 支持 `scratchpadRelativePath` 入参，关闭方言高亮/补全/验证/转换/DuckDB加速
- [x] **Ctrl+S 保存回 .scratchpad/** — 草稿箱模式 Ctrl+S 调用 `save_scratchpad_file`
- [x] **编辑器标题显示草稿文件名** — dockview tab title 显示 `📜 文件名.sql`
- [x] **EditorToolbar 高级功能开关** — `showAdvanced` prop 控制格式/验证/转换按钮可见性

### v2.3 已完成 ✅

- [x] **FileMeta 数据模型** — Rust `FileMeta { last_connection_id, last_executed_at }` + HashMap 存储
- [x] **store.update_file_meta()** — 写入/更新文件的连接与执行时间元数据
- [x] **Tauri Command `update_scratchpad_file_meta`** — 注册到 lib.rs
- [x] **前端 API `updateFileMeta()` + Composable `saveFileMeta()`** — 12 个 Tauri IPC 封装
- [x] **打开文件自动恢复连接** — `openFileInEditor` 读取 `file_meta` 中的 `last_connection_id` 并传入编辑器
- [x] **保存/执行时更新元数据** — `handleScratchpadSave` / `handleExecute` 自动调用 `update_scratchpad_file_meta`

### v2.4 已完成 ✅

- [x] **重命名迁移 file_meta 键** — `rename_entry()` 同步迁移 `file_meta` HashMap 中的旧 path → 新 path
- [x] **代码编辑器打通** — `.py`/`.json`/`.txt`/`.md` 在 SqlEditorPanel 打开，Monaco 自动切换语法高亮，Ctrl+S 保存
- [x] **文件内容搜索** — Rust `search_file_content()` + Command `search_scratchpad_content` + 前端 API `searchFileContent()` + Panel 全文/文件名双模式

### v2.5 已完成 ✅

- [x] **内容搜索路径匹配修复** — `filteredLocalEntries` 对齐 Rust 相对路径与前端绝对路径
- [x] **Config 内存缓存** — `ScratchpadStore` 新增 `config_cache: Arc<Mutex<Option<ScratchpadConfig>>>`，`load_config()` 缓存命中免磁盘 IO，`save_config()` 同步更新缓存

### v2.6 已完成 ✅

- [x] **回收站 UI** — `trashEntries` ref + `loadTrashEntries()` / `restoreTrashEntry()` / `emptyTrashBin()` 前端 API → Composable → 面板折叠区域（恢复 / 清空按钮）
- [x] **草稿箱自动保存** — `markDirty()` 触发 2s 防抖 `scheduleAutoSave()`，失焦后自动写回 `.scratchpad/`，组件卸载清理 timer

### v2.7 已完成 ✅

- [x] **图标主题适配** — CSS 变量全面对齐 `global.css`（`--color-text-primary/secondary/muted` / `--color-border` / `--color-bg-elevated/tertiary` / `--brand-danger`），Dark/Light 模式跟随全局 `body.theme-dark` / `body.theme-light` 切换
- [x] **DuckDB 分析入口** — `AnalyzableFile` TS 接口 + `getAnalyzableFiles()` API + `loadAnalyzableFiles()` composable + 右键菜单 "用 DuckDB 分析"（`.csv`/`.parquet`/`.json`/`.xlsx`），打开 DuckDB Query Hint 到 SQL 编辑器

### v2.8 已完成 ✅

- [x] **防重复 Tab** — `WorkbenchView.vue` `handleOpenSqlEditor` 检测已有同 `scratchpadRelativePath` 面板，自动聚焦而非新建
- [x] **拖放文件导入** — `ScratchpadPanel.vue` 支持 dragover/dragleave/drop，从文件资源管理器拖入文件自动导入
- [x] **文件监控 watch .scratchpad/** — `notify` crate 后台线程监控目录变更，500ms 防抖后通过 `scratchpad-changed` Tauri event 推送前端自动刷新
- [x] **CSS 主题变量对齐** — `ScratchpadTreeNode.vue` CSS 变量迁移至 `--color-bg-*` / `--color-text-*` 全局主题系统
- [x] **工具栏人性化优化** — 两行布局（操作行+搜索行）、按钮图标化、`type="primary"` 主操作、主题变量全面对齐、硬编码文本 i18n 化

### v3.0 已完成 ✅

- [x] **键盘导航** — ↑↓ 在 `filteredLocalEntries` 中移动选中项 + `scrollIntoView({ block: 'nearest' })` 自动滚到可视区；无选中时 ↓ 选第一个 ↑ 选最后一个
- [x] **Enter 打开** — 选中文件按 Enter = `handleOpen(entry)` 打开编辑器
- [x] **重命名 loading 反馈** — 提交时 `renamingSaving=true` → 输入框 `:disabled` + 右侧 CSS `@keyframes spin` 旋转动画，后台完成后面板自动清 `renamingKey`
- [x] **提升事件联动** — `promote_scratchpad_to_resource` 完成后 `app.emit("analytics-resource-changed", ())`，分析资源管理器监听此事件自动刷新

### v3.1 已完成 ✅

- [x] **ScratchpadEntry 精简** — 移除 `extension`/`is_external_ref` 字段，`modified_at` 改为 `Option<String>` (ISO 8601)，前端 `extension` 用 `path.extension()` 方法获取
- [x] **SearchMatch 结构体** — Rust 新增 `SearchMatch { file, line_number, line_content }`，`search_file_content` 返回 `Vec<SearchMatch>` 而非 `Vec<String>`
- [x] **文件排序** — 工具栏三按钮（按名称/大小/修改时间），点击切换升序/降序，图标 ArrowUpDown→ArrowUp→ArrowDown 实时反馈
- [x] **搜索行上下文展示** — 内容搜索结果面板：文件名 + 匹配计数 badge + 行号 + 行内容（等宽字体），最多显示 5 行 + "...还有 N 处匹配"
- [x] **i18n 扩展** — 新增 `sortByName/sortBySize/sortByModified/sortAsc/sortDesc/matchesCount/searchFileResults` 7 个 key

### v3.2 已完成 ✅

- [x] **新建文件夹入口** — 工具栏新增"新建文件夹"按钮（`FolderPlus` 图标），独立 `NModal` 输入框，`createEntry(name, true)`
- [x] **搜索结果点击跳转** — 点击搜索结果文件名 → `openFileAtLine(file, 0)` 打开文件；点击行 → `openFileAtLine(file, line)` 打开并跳转到行
- [x] **搜索跳转到行 (Monaco)** — `SqlEditorParams.initialLine` → `WorkbenchView` 透传 → `SqlEditorPanel.onMounted` 中 `editor.revealLineInCenter(initialLine)` + `setPosition`
- [x] **修改时间显示** — `TreeNode` 新增 `node-time` 副标签，computed `modifiedTime` 相对时间格式化（1m→59m→1h→23h→1d→7d→隐藏），CSS 等宽右对齐
- [x] **Toast 操作反馈** — `createDiscreteApi(['message'])` 在创建/删除/重命名/导入/恢复/清空回收站/提升成功后调用 `message.success()`，13 个操作全覆盖
- [x] **TreeNode extension 修复** — `fileIcon` computed 从 `props.entry.extension` 改为推演 `name.includes('.')` 逻辑（适配 v3.1 模型变更）
- [x] **i18n 扩展** — 新增 `newFolder/newFolderTitle/folderNamePlaceholder/createdSuccess/deletedSuccess/renamedSuccess/importedSuccess/promotedSuccess/restoredSuccess/trashEmptied` 等 10+ 个 key

### 变更文件

| 文件                     | 改动                                          | 行数 |
| ------------------------ | --------------------------------------------- | :--: |
| `ScratchpadPanel.vue`    | 文件夹按钮+modal+toast+搜索点击跳转           | +90  |
| `ScratchpadTreeNode.vue` | 修改时间显示+extension 修复                   | +28  |
| `SqlEditorPanel.vue`     | `initialLine` computed + `revealLineInCenter` | +12  |
| `WorkbenchView.vue`      | 透传 `initialLine` 参数                       |  +2  |
| `sql.ts`                 | `SqlEditorParams.initialLine?: number`        |  +1  |
| `zh-CN.json`             | 10+ 个新 i18n 键                              | +12  |

### v3.3 已完成 ✅

- [x] **Bug Fix: `isAnalyzableFile` extension 字段** — 改用 `entry.name.includes('.')` 推导扩展名，适配 v3.1 模型变更；同时新增 `.duckdb` 到可分析文件列表
- [x] **大小写搜索** — Rust `search_file_content` 新增 `case_sensitive: bool` 参数，`true` 用 `line.contains(query)`，`false` 用 `line.to_lowercase().contains(query_lower)`；前端 `caseSensitive` ref + 搜索栏 Aa 按钮切换 + watch 重搜
- [x] **折叠/展开全部** — `collectFolderPaths()` 递归收集所有文件夹路径，本地组头 `NButton` 两个按钮点击设置 `expandedKeys` 为全部/空集
- [x] **搜索文本高亮** — `highlightMatch(line, query)` 使用 `escapeHtml` 防 XSS + `<mark class="search-hl">` 包裹匹配文本，模板 `v-html` 渲染，CSS `:deep(.search-hl)` 黄色背景
- [x] **复制路径 Toast** — `handleMenuAction` 的 `'copy-path'` 分支新增 `message.success(t('scratchpad.pathCopied'))`
- [x] **文件图标扩展** — TreeNode `extensionIconMap` 新增 `.duckdb` → `Database`、`.parquet` → `Table2`
- [x] **i18n 扩展** — 新增 `expandAll/collapseAll/caseSensitive/pathCopied` 4 个 key

### 变更文件

| 文件                     | 改动                                                                | 行数 |
| ------------------------ | ------------------------------------------------------------------- | :--: |
| `store.rs`               | `search_file_content` 新增 `case_sensitive` 参数 + 条件匹配         |  +6  |
| `commands.rs`            | `search_scratchpad_content` 新增 `case_sensitive` 参数              |  +2  |
| `scratchpad-api.ts`      | `searchFileContent` 新增 `caseSensitive` 参数                       |  +1  |
| `use-scratchpad.ts`      | `searchContent` 新增 `caseSensitive` 参数                           |  +1  |
| `ScratchpadPanel.vue`    | Bug 修复 + 折叠/展开全部 + 大小写按钮 + 高亮函数 + 复制 toast + CSS | +60  |
| `ScratchpadTreeNode.vue` | `.duckdb`/`.parquet` 图标                                           |  +2  |
| `zh-CN.json`             | 4 个新 i18n 键                                                      |  +4  |

### v3.4 已完成 ✅

- [x] **最近打开** — `recentFiles: ref<string[]>` 面板级内存列表，`addRecentFile` 去重+推到首位+截断5条；`openFileInEditor`/`openFileAtLine` 调用；`recentFileEntries` computed 从 `localEntries` 查找实际条目；树顶可折叠"最近打开"区域（ChevronDown/Right 切换）
- [x] **空状态引导** — `filteredLocalEntries.length === 0` 时显示 `empty-state`：`FolderOpen` 32px 图标（50% 透明度）+ 标题"草稿箱为空" + 引导文本 + 新建/导入双按钮；搜索模式下不显示（由搜索结果面板接管）
- [x] **i18n 扩展** — 新增 `recentFiles/noRecentFiles/emptyScratchpad/emptyScratchpadHint` 4 个 key

### 变更文件

| 文件                  | 改动                                  | 行数 |
| --------------------- | ------------------------------------- | :--: |
| `ScratchpadPanel.vue` | 最近打开 section + 空状态 state + CSS | +100 |
| `zh-CN.json`          | 4 个新 i18n 键                        |  +4  |

### v3.5 已完成 ✅

- [x] **多选基础设施** — `selectedKeys: ref<Set<string>>()` + `lastSelectPath` + `multiSelected` computed；`handleSelect(entry, event)` 接收 MouseEvent，分 Ctrl/Shift/普通 三种路径
- [x] **TreeNode 多选适配** — 新增 `selectedKeys?: Set<string>` 可选 prop，`isSelected` 兼容新旧两种模式；`select` emit 新增 `MouseEvent` 参数；子节点 props 透传
- [x] **批量删除** — 右键菜单 `batch-delete`（显示数量）+ `window.confirm` 确认 + 逐个 `deleteEntry` + toast `batchDeletedSuccess`；Delete 键同样支持
- [x] **复制/粘贴** — `clipboardEntry` ref 存储被复制条目；右键"复制"设置 clipboard；右键"粘贴" `loadFileContent` + `createEntry` + `saveFile` 生成 `_copy` 副本 + toast
- [x] **Ctrl+A 全选** — `handleKeydown` 新增分支，`flattenEntries` 递归收集所有可见条目路径到 `selectedKeys`
- [x] **Bug Fix: openFileInEditor extension** — `entry.extension` 改为 `entry.name.includes('.')` 推演，适配 v3.1 模型变更
- [x] **i18n 扩展** — 新增 `batchDelete/batchDeleteConfirm/batchDeletedSuccess/copyFile/pasteFile/pasteCopied/selectAll` 7 个 key

### 变更文件

| 文件                     | 改动                                                                                                                                                          | 行数 |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | :--: |
| `ScratchpadPanel.vue`    | selectedKeys/lastSelectPath/clipboardEntry state + handleSelect 重写 + showEntryMenu 自适应 + handleMenuAction 批量/复制/粘贴 + Ctrl+A + openFileInEditor fix | +120 |
| `ScratchpadTreeNode.vue` | selectedKeys prop + isSelected 兼容 + select emit MouseEvent                                                                                                  |  +8  |
| `zh-CN.json`             | 7 个新 i18n 键                                                                                                                                                |  +7  |

### v3.8 已完成 ✅

- [x] **流式搜索替代全量读取** — v3.7 的 10MB 硬限制方案被替换：`read_to_string` → `BufReader::lines()`，一次只持有一行文本在内存中；1GB 文件搜索内存占用从 1GB 降至恒定 ~8KB
- [x] **超时保护** — `SEARCH_PER_FILE_TIMEOUT_SECS = 30`，单文件搜索超过 30 秒自动终止，防止无限阻塞
- [x] **search_single_file 函数** — 提取为独立 async 函数，`tokio::time::timeout` 包裹，返回 `Result<Vec<SearchMatch>, CoreError>`，超时/IO 错误统一跳过不中断搜索
- [x] **简化 SearchResult** — `total_files_skipped` 始终为 0，`skipped_files` 始终为空 vec（不再需要门槛跳过逻辑）

### 变更文件

| 文件       | 改动                                                                        | 行数 |
| ---------- | --------------------------------------------------------------------------- | :--: |
| `store.rs` | imports + `search_single_file` 函数 + `search_file_content` 重写 + 常量替换 | +55  |

### 后续版本 🔮

- [x] **搜索安全加固** — `MAX_SEARCH_FILE_SIZE = 10MB`，超过跳过大文件并记录到 `skipped_files`；`MAX_SEARCH_RESULTS = 500`，超出截断标记 `truncated: true`；新增 `SearchResult` 结构体包含 `matches/total_scanned/total_skipped/skipped_files/truncated`；前端模板新增 yellow notice bar 告知跳过/截断信息；新增 `search-no-results` 区域展示空结果提示
- [x] **前后端同步** — `SearchResult` Rust struct → TS interface；API/composable 返回类型更新为 `SearchResult | null`
- [x] **i18n** — 新增 3 个 key（searchTruncated/searchSkippedFiles/noResults）+ EN locale 同步

### 变更文件

| 文件                     | 改动                                                                                                | 行数 |
| ------------------------ | --------------------------------------------------------------------------------------------------- | :--: |
| `models.rs`              | 新增 `SearchResult` struct（5 字段）                                                                |  +9  |
| `store.rs`               | `search_file_content` 重写：文件大小检查 + 截断 + 统计 + 常量                                       | +60  |
| `scratchpad_commands.rs` | 返回类型 `Vec<SearchMatch>` → `SearchResult`                                                        |  +1  |
| `mod.rs`                 | 导出 `SearchResult`                                                                                 |  +1  |
| `types/index.ts`         | 新增 `SearchResult` interface                                                                       |  +7  |
| `scratchpad-api.ts`      | `searchFileContent` 返回类型更新                                                                    |  +2  |
| `use-scratchpad.ts`      | `searchContent` 返回 `SearchResult \| null`                                                         |  +2  |
| `ScratchpadPanel.vue`    | searchResult ref + 3 computed + 2 watcher 改写 + notice bar 模板 + no-results div + Info icon + CSS | +50  |
| `zh-CN.json`             | 3 新 key                                                                                            |  +3  |
| `en.json`                | 3 新 key                                                                                            |  +3  |

### 后续版本 🔮

- [x] **新建文件模板** — 新建文件对话框新增模板选择器（SQL/JSON/Markdown/Python），选择后自动填充文件后缀（如 `untitled.sql`）；`confirmCreate` 通过 `saveFile` 写入预设模板内容（SQL 注释头、JSON 空对象、Markdown 标题、Python 编码声明）
- [x] **拖放文件到编辑器** — `ScratchpadTreeNode` 新增 `draggable="true"`（仅文件节点）+ `@dragstart` emit；Panel `handleTreeNodeDragStart` 设置 `application/x-scratchpad-file` MIME 数据 + `text/plain`；`SqlEditorPanel` 新增 `@dragover.prevent` + `@drop.prevent` 处理 `read_scratchpad_file` → `insertText` 在光标位置插入
- [x] **删除撤销提示** — 单文件删除后底部弹出 `undo-bar`（5秒自动消失），显示"已删除「name」"+ "撤销"按钮；`undoState` reactive 管理（path/timer/dismissUndo/handleUndoDelete）；撤销调用 `restoreTrashEntry` 恢复文件 + toast 确认
- [x] **Bug Fix: zh-CN.json 语法** — `selectAll` 后缺少逗号导致 JSON 解析失败，已修复

### 变更文件

| 文件                     | 改动                                                                                                                                                                                                    | 行数 |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :--: |
| `ScratchpadPanel.vue`    | 模板选择器 modal + selectedTemplate state + TEMPLATE_CONTENTS + selectTemplate + confirmCreate 重写 + undoState/dismissUndo/showUndo/handleUndoDelete + undo-bar 模板 + 拖放状态；删除处理改用 showUndo | +110 |
| `ScratchpadTreeNode.vue` | draggable + dragstart emit + forwardDragStart                                                                                                                                                           | +12  |
| `SqlEditorPanel.vue`     | editor-container @dragover/@drop + handleEditorDragOver/handleEditorDrop                                                                                                                                | +21  |
| `zh-CN.json`             | templateType/undo 2 个新 key + 逗号修复                                                                                                                                                                 |  +1  |

### 后续版本 🔮

- [ ] 文件拖拽重排序（HTML5 DnD 同级拖拽排序）
- [ ] 正则表达式搜索支持
- [ ] 草稿箱与 DuckDB 深度集成（直连 .duckdb 文件）
- [ ] 最近文件跨会话持久化
      </parameter>
      </｜DSML｜inv
