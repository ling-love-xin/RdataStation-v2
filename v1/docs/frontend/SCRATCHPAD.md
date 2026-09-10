# 草稿箱前端架构文档

> 版本：v1.1
> 创建日期：2026-05-08
> 状态：✅ 组件层完成 | ✅ SQL 编辑器集成完成

---

## 一、模块概览

草稿箱前端模块位于 `src/extensions/builtin/scratchpad/`，遵循 RdataStation 插件扩展架构：`extension.ts` 注册 → dockview 面板渲染 → composable 管理状态 → API 层调用后端。

```
src/extensions/builtin/scratchpad/
├── package.json                        # 扩展元数据（name: "scratchpad", version: "1.0.0"）
├── extension.ts                        # 扩展激活入口
├── types/index.ts                      # 类型定义
├── infrastructure/api/scratchpad-api.ts # 后端调用层
└── ui/
    ├── composables/use-scratchpad.ts    # 状态 + 业务逻辑
    └── components/
        ├── ScratchpadPanel.vue          # 主面板
        └── ScratchpadTreeNode.vue       # 递归树节点
```

---

## 二、组件树

```
dockview-vue Panel (left sidebar)
└── ScratchpadPanel.vue
    ├── .toolbar
    │   ├── NButton [新建]        → handleCreateFile → showCreateModal
    │   ├── NButton [导入]        → handleImportFile → tauri dialog
    │   ├── NButton [引用]        → handleAddReference → showRefModal
    │   ├── NButton [刷新]        → loadFiles
    │   └── NInput  [搜索...]     ← v-model: searchQuery
    │
    ├── .tree-container
    │   ├── .tree-group (外部引用)
    │   │   ├── .group-header     → toggleGroup('external')
    │   │   └── .group-content
    │   │       └── .ref-row × N  → handleRefClick / showRefMenu
    │   │
    │   └── .tree-group (本地草稿)
    │       ├── .group-header     → toggleGroup('local')
    │       └── .group-content
    │           └── ScratchpadTreeNode.vue × N
    │
    ├── NModal [新建草稿]          ← v-model:showCreateModal
    │   ├── NInput (ref:createInputRef) → Enter: confirmCreate
    │   └── .modal-actions [取消] [确定]
    │
    ├── NModal [添加外部引用]       ← v-model:showRefModal
    │   ├── NInput [别名] + NInput [路径] + NButton [浏览]
    │   └── .modal-actions [取消] [确定]
    │
    └── .scratchpad-context-menu   ← position: fixed, visible on right-click
        └── .menu-item × N          → handleMenuAction(key)
```

---

## 三、扩展注册（extension.ts）

```typescript
const activate = (context: ExtensionContext): ScratchpadExtensionAPI => {
  // 在 dockview 左侧注册一个面板
  const panelDisposable = context.window.registerViewProvider('scratchpad', {
    component: ScratchpadPanel, // Vue SFC 组件
    title: '草稿箱',
    location: 'left', // 左侧边栏
    icon: 'FileText', // lucide-vue-next 图标名
    order: 4, // 活动栏排序位置
  })
  // ...
}
```

面板通过活动栏（activity bar）入口打开：

- `layout-store.ts` 中 `leftActivityItems` 包含 `{ id: 'scratchpad', icon: 'FileText', label: '草稿箱' }`
- `ACTIVEBAR_TO_PANEL_ID` 映射: `'scratchpad' → 'scratchpad'`

---

## 四、类型定义（types/index.ts）

```typescript
export type ScratchpadEntryKind = 'file' | 'folder'

export interface ScratchpadEntry {
  name: string // 文件名（如 "临时查询.sql"）
  path: string // 绝对路径
  kind: ScratchpadEntryKind
  size: number // 字节数
  modified_at: string // ISO 8601
  extension: string // 含点，如 ".sql"
  is_external_ref: boolean
}

export interface ExternalReference {
  alias: string // 用户别名（如 "下载数据"）
  path: string // 外部目录/文件绝对路径
  created_at: string // ISO 8601
}

export interface ScratchpadResponse {
  local_entries: ScratchpadEntry[]
  external_references: ExternalReference[]
  scratchpad_path: string // {project}/.scratchpad/ 绝对路径
}
```

> **与后端映射**：Rust `#[serde(rename_all = "snake_case")]` → JSON key 与 TS 字段名一致。`PathBuf` → `String`。`DateTime<Utc>` → ISO 8601 `String`。

---

## 五、API 层（infrastructure/api/scratchpad-api.ts）

11 个纯函数，全部通过 `invoke()` 调用 Tauri 后端命令：

```typescript
// 文件列表
listScratchpadFiles(): Promise<ScratchpadResponse>

// CRUD
createScratchpadEntry(name: string, isFolder: boolean): Promise<ScratchpadEntry>
deleteScratchpadEntry(relativePath: string): Promise<void>
renameScratchpadEntry(relativePath: string, newName: string): Promise<ScratchpadEntry>

// 读写
readScratchpadFile(relativePath: string): Promise<string>
saveScratchpadFile(relativePath: string, content: string): Promise<void>

// 导入 & 引用
importExternalFile(sourcePath: string): Promise<ScratchpadEntry>
addExternalReference(alias: string, path: string): Promise<ExternalReference>
removeExternalReference(alias: string): Promise<void>

// 工具
openInExplorer(path: string): Promise<void>
checkFileSize(relativePath: string): Promise<number>
```

> **未封装的 4 个命令**：`get_analyzable_files`、`list_scratchpad_trash`、`restore_scratchpad_from_trash`、`empty_scratchpad_trash` — 后端已就绪，前端预留对接。

---

## 六、Composable（use-scratchpad.ts）

`useScratchpad()` 是整个草稿箱模块的业务逻辑核心。所有 Vue 组件通过它获取数据和方法，不直接调用 API 层。

### 状态树

```
useScratchpad()
├── response: Ref<ScratchpadResponse | null>    ← listScratchpadFiles() 结果
├── isLoading: Ref<boolean>
├── error: Ref<string | null>
├── searchQuery: Ref<string>                     ← 搜索输入，双向绑定
│
├── localEntries: ComputedRef                    ← response.local_entries.filter(name.includes(q))
├── externalReferences: ComputedRef              ← response.external_references.filter(alias|path.includes(q))
├── scratchpadPath: ComputedRef                  ← response?.scratchpad_path
├── invalidReferences: ComputedRef               ← 路径格式不合法或含 .. 的引用
├── validReferences: ComputedRef                 ← 路径合法的引用
```

### 写操作模式

所有写操作遵循统一模式：

1. 调用 API 函数
2. 成功则 `await loadFiles()` 刷新列表
3. 失败则设置 `error.value`

```typescript
async function createEntry(name, isFolder) {
  try {
    const entry = await createScratchpadEntry(name, isFolder)
    await loadFiles()
    return entry
  } catch (e) {
    error.value = e.message
    return null
  }
}
```

### 引用有效性判断

```typescript
function isRefValid(ref: ExternalReference): boolean {
  if (!ref.path) return false
  const pathPattern = /^([A-Za-z]:[\\/]|[/\\])/
  return pathPattern.test(ref.path) && !ref.path.includes('..')
}
```

检查两点：路径格式是否合法（绝对路径模式）+ 不含路径遍历符号。

---

## 七、ScratchpadPanel.vue 架构

### 内部状态

| 状态              | 类型                                       | 用途                 |
| ----------------- | ------------------------------------------ | -------------------- |
| `expandedKeys`    | `Ref<Set<string>>`                         | 展开的文件夹路径集合 |
| `selectedKey`     | `Ref<string \| null>`                      | 当前选中条目路径     |
| `renamingKey`     | `Ref<string \| null>`                      | 正在重命名的条目路径 |
| `groupExpanded`   | `Reactive<{external, local}>`              | 分组折叠状态         |
| `showCreateModal` | `Ref<boolean>`                             | 新建对话框可见性     |
| `newFileName`     | `Ref<string>`                              | 新建文件名输入       |
| `showRefModal`    | `Ref<boolean>`                             | 外部引用对话框可见性 |
| `contextMenu`     | `Reactive<{visible, x, y, target, items}>` | 右键菜单状态         |

### 关键交互流程

#### 双击打开文件

```
handleOpen(entry)
  ├── entry.kind === 'folder'
  │     └── handleToggleExpand(entry)
  └── entry.kind === 'file'
        └── openFileInEditor(entry)
              ├── getFileSize(path) → > 50MB? → 拒绝
              └── editorMap[extension] → 目标编辑器类型 → ⏳ 待创建面板
```

#### 右键菜单

```
showEntryMenu(event, entry)
  → selectedKey = entry.path
  → clampToViewport(clientX, clientY, menuW, menuH)
  → contextMenu = { visible: true, items: [...] }
  → document.addEventListener('click', closeContextMenu)
```

#### 内联重命名

```
startRename(entry) → renamingKey = entry.path
  → ScratchpadTreeNode watch renamingKey
      → nextTick → input.focus() → input.select()
      → Enter: finishRename(entry, newName) → renameEntry
      → Escape: cancelRename()
      → Blur: finishRename(entry, newName)  ← 空值不提交
```

---

## 八、ScratchpadTreeNode.vue

递归组件，支持嵌套文件夹渲染。

### Props

```typescript
interface Props {
  entry: ScratchpadEntry
  depth: number
  expandedKeys: Set<string>
  selectedKey: string | null
  renamingKey: string | null
}
```

### 渲染逻辑

```
entry.kind === 'folder'
  → 显示 ChevronDown/ChevronRight 图标
  → expandedKeys.has(entry.path) 时递归渲染子节点

entry.kind === 'file'
  → 显示 FileText 或其他后缀对应图标
  → 显示文件大小（可读格式：B/KB/MB）
  → renamingKey === entry.path 时渲染 NInput 输入框
```

---

## 九、与 SQL 编辑器的对接（v2.1）

### 入参扩展

```typescript
// src/shared/types/sql.ts
export interface SqlEditorParams {
  // ... 现有字段 ...
  scratchpadFilePath?: string // 非空 = 草稿箱文件模式
  scratchpadFileName?: string // 显示用文件名
}
```

### 对接流程

```
ScratchpadPanel: openFileInEditor(entry)
  → readScratchpadFile(entry.path)                    // 读取文件内容
  → 创建 SqlEditorPanel 实例                            // dockview API
       params: {
         scratchpadFilePath: entry.path,
         scratchpadFileName: entry.name,
         initialSql: content
       }

SqlEditorPanel: onMounted()
  → scratchpadFilePath 非空
    → 标题显示 scratchpadFileName
    → 关闭方言高亮/补全/转换/DuckDB加速/执行计划
    → Ctrl+S → saveScratchpadFile
    → 保留完整执行引擎（单语句/多语句/选中执行）
    → 保留结果展示（AG Grid）
    → 保留连接管理（手动选择 + ensureConnection）
```

### 不需要变动的文件

- Rust 后端：`read_scratchpad_file` / `save_scratchpad_file` / `execute_sql` — 全部就绪
- `QueryResultPanel.vue`：结果展示组件完全复用
- `sql-execution-store.ts`：执行状态管理完全复用

---

## 十、国际化 & 主题

当前状态：

- 所有用户可见文本硬编码为中文
- 图标使用 lucide-vue-next（`FileText`, `Folder`, `ChevronDown` 等）
- 颜色使用 CSS 变量（`var(--border-color)`, `var(--bg-primary)` 等），跟随系统主题

后续待办：

- [ ] 多语言支持（i18n key 替换硬编码中文）
- [ ] 文件类型图标主题适配（Dark/Light 双色图标集）

---

## 十一、相关文档

| 文档               | 路径                                                        |
| ------------------ | ----------------------------------------------------------- |
| 全栈数据模型与接口 | [SCRATCHPAD_SCHEMA.md](../backend/SCRATCHPAD_SCHEMA.md)     |
| 设计方案           | [SCRATCHPAD_DESIGN.md](../backend/SCRATCHPAD_DESIGN.md)     |
| 开发进度           | [SCRATCHPAD_PROGRESS.md](../backend/SCRATCHPAD_PROGRESS.md) |
| SQL 编辑器文档     | [SQL-EDITOR.md](./SQL-EDITOR.md)                            |
| 前端文档索引       | [INDEX.md](./INDEX.md)                                      |

</parameter>
<｜DSML｜parameter name
