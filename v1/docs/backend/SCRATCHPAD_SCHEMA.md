# 草稿箱模块 — 全栈数据模型与接口文档

> 版本：v3.12
> 最后更新：2026-05-10
> 状态：✅ v3.12 — 文件监控增强 (事件详情+原子保存检测+状态保持)

---

## 一、模块定位

草稿箱（Scratchpad）是用户随项目携带的临时文件暂存区，存放 SQL 脚本、分析用数据文件（CSV/Parquet/JSON/Excel）、Python 脚本、笔记等。所有文件存储在项目目录下的 `.scratchpad/` 中。

**核心差异**：与分析资源管理器（Analytics Resource）不同，草稿箱没有数据库元数据表，纯文件系统操作——简单、直接、本地化。

---

## 二、目录结构

### 2.1 物理存储

```
project/
├── .scratchpad/
│   ├── my_query.sql           # 用户 SQL 文件
│   ├── sales_data.csv         # 分析用数据文件（DuckDB 可直接导入）
│   ├── notes/                 # 用户文件夹（最多 4 层嵌套）
│   │   └── schema.md
│   ├── .trash/                # 回收站
│   │   └── old_file.sql       # 被删除的文件（软删除）
│   └── .scratchpad.json       # 草稿箱配置（外部引用列表）
```

### 2.2 后端代码（Rust）

```
src-tauri/src/
├── core/scratchpad/
│   ├── mod.rs          # re-export（~10 行）
│   ├── models.rs       # DTO：Entry / AnalyzableFile / Reference / Response（~74 行）
│   ├── state.rs        # Arc<Mutex<Option<Store>>> 全局缓存 + AtomicBool watcher（~72 行）
│   └── store.rs        # 文件系统操作（~910 行）
├── commands/
│   └── scratchpad_commands.rs  # 21 个 Tauri Command（~347 行）
└── lib.rs              # 状态注册 + generate_handler! 注册
```

### 2.3 前端代码（Vue 3 + TypeScript）

```
src/extensions/builtin/scratchpad/
├── package.json                        # 扩展元数据
├── extension.ts                        # 扩展入口（注册 dockview 面板）
├── types/
│   └── index.ts                        # TypeScript 类型定义
├── infrastructure/
│   └── api/
│       └── scratchpad-api.ts           # 22 个 Tauri invoke 封装函数
└── ui/
    ├── composables/
    │   └── use-scratchpad.ts           # 业务逻辑 hook（33 个导出项）
    └── components/
        ├── ScratchpadPanel.vue         # 主面板（工具栏/搜索/回收站/右键菜单/提升确认/拖放导入/事件监听）
        └── ScratchpadTreeNode.vue      # 递归树节点（图标映射 + 内联重命名 + 主题变量对齐）
```

---

## 三、核心数据结构

### 3.1 Rust 侧（后端）

#### ScratchpadEntry

```rust
pub struct ScratchpadEntry {
    pub name: String,              // 文件/文件夹名
    pub path: PathBuf,             // 绝对路径
    pub kind: ScratchpadEntryKind, // File | Folder
    pub size: u64,                 // 文件大小（字节）
    pub modified_at: Option<String>, // 最后修改时间（ISO 8601），None 表示读取失败
}
```

#### SearchMatch (v2.2 新增)

```rust
pub struct SearchMatch {
    pub file: String,              // 相对路径文件名
    pub line_number: usize,        // 匹配行号
    pub line_content: String,      // 匹配行内容
}
```

#### AnalyzableFile

```rust
pub struct AnalyzableFile {
    pub name: String,              // 文件名
    pub relative_path: String,     // 相对路径（相对于 .scratchpad/）
    pub file_type: String,         // csv/parquet/json/xlsx/sqlite/duckdb
    pub size_bytes: u64,           // 文件大小
    pub duckdb_query_hint: String, // DuckDB 推荐查询 SQL
}
```

#### ExternalReference

```rust
pub struct ExternalReference {
    pub alias: String,             // 别名
    pub path: PathBuf,             // 外部文件/目录绝对路径
    pub created_at: DateTime<Utc>,
}
```

#### ScratchpadResponse

```rust
pub struct ScratchpadResponse {
    pub local_entries: Vec<ScratchpadEntry>,
    pub external_references: Vec<ExternalReference>,
    pub scratchpad_path: PathBuf,
    pub file_meta: HashMap<String, FileMeta>,
}
```

#### ScratchpadChangeEvent

```rust
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScratchpadChangeEvent {
    pub paths: Vec<String>,
}
```

#### FileMeta

```rust
pub struct FileMeta {
    pub last_connection_id: Option<String>,
    pub last_executed_at: Option<DateTime<Utc>>,
}
```

### 3.2 TypeScript 侧（前端）

```typescript
// src/extensions/builtin/scratchpad/types/index.ts
export type ScratchpadEntryKind = 'file' | 'folder'

export interface ScratchpadEntry {
  name: string
  path: string
  kind: ScratchpadEntryKind
  size: number
  modified_at: string | null
}

export interface SearchMatch {
  file: string
  line_number: number
  line_content: string
}

export interface ExternalReference {
  alias: string
  path: string
  created_at: string
}

export interface ScratchpadResponse {
  local_entries: ScratchpadEntry[]
  external_references: ExternalReference[]
  scratchpad_path: string
  file_meta: Record<string, FileMeta>
}

export interface FileMeta {
  last_connection_id?: string
  last_executed_at?: string
}

export interface ScratchpadChangeEvent {
  paths: string[]
}

export interface PromoteResult {
  resource: {
    id: string
    resource_type: string
    name: string
    scope: string
  }
  removed: boolean
}
```

> **Rust↔TS 映射**：`serde(rename_all = "snake_case")` 保证 JSON key 一致；`PathBuf` 序列化为 `String`；`DateTime<Utc>` 序列化为 ISO 8601 字符串。

```typescript
export interface SqlEditorParams {
  connectionId?: string
  databaseName?: string
  initialSql?: string
  panelId?: string
  schema?: string
  scratchpadRelativePath?: string
  scratchpadFileName?: string
  language?: string
  initialLine?: number // v2.3: 搜索跳转到指定行
}
```

---

## 四、API 接口全表

### 4.1 Tauri Command → 前端 API 映射

| #   | Tauri Command                    | 前端封装函数                                             | 类别                                             |          状态          |
| --- | -------------------------------- | -------------------------------------------------------- | ------------------------------------------------ | :--------------------: |
| 1   | `init_scratchpad_store`          | _(扩展激活时自动调用)_                                   | 初始化                                           |           ✅           |
| 2   | `list_scratchpad_files`          | `listScratchpadFiles()`                                  | 文件列表                                         |           ✅           |
| 3   | `create_scratchpad_entry`        | `createScratchpadEntry(name, isFolder)`                  | CRUD                                             |           ✅           |
| 4   | `delete_scratchpad_entry`        | `deleteScratchpadEntry(relativePath)`                    | CRUD                                             |           ✅           |
| 5   | `rename_scratchpad_entry`        | `renameScratchpadEntry(relativePath, newName)`           | CRUD                                             |           ✅           |
| 6   | `read_scratchpad_file`           | `readScratchpadFile(relativePath)`                       | 读写                                             |           ✅           |
| 7   | `save_scratchpad_file`           | `saveScratchpadFile(relativePath, content)`              | 读写                                             |           ✅           |
| 8   | `import_external_file`           | `importExternalFile(sourcePath)`                         | 导入                                             |           ✅           |
| 9   | `add_external_reference`         | `addExternalReference(alias, path)`                      | 引用                                             |           ✅           |
| 10  | `remove_external_reference`      | `removeExternalReference(alias)`                         | 引用                                             |           ✅           |
| 11  | `open_scratchpad_in_explorer`    | `openInExplorer(path)`                                   | 工具                                             |           ✅           |
| 12  | `check_scratchpad_file_size`     | `checkFileSize(relativePath)`                            | 工具                                             |           ✅           |
| 13  | `get_analyzable_files`           | `getAnalyzableFiles()`                                   | 分析                                             |           ✅           |
| 14  | `update_scratchpad_file_meta`    | `updateFileMeta(relativePath, connectionId?)`            | 元数据                                           |           ✅           |
| 15  | `search_scratchpad_content`      | `searchFileContent(query, caseSensitive)`                | 搜索                                             | ✅ (→ `SearchMatch[]`) |
| 16  | `list_scratchpad_trash`          | `listTrash()`                                            | 回收站                                           |           ✅           |
| 17  | `restore_scratchpad_from_trash`  | `restoreFromTrash(trashName)`                            | 回收站                                           |           ✅           |
| 18  | `empty_scratchpad_trash`         | `emptyTrash()`                                           | 回收站                                           |           ✅           |
| 19  | `watch_scratchpad`               | `watchScratchpad()`                                      | 监控                                             |           ✅           |
| 20  | `unwatch_scratchpad`             | `unwatchScratchpad()`                                    | 监控                                             |           ✅           |
| 21  | `promote_scratchpad_to_resource` | `promoteScratchpadToResource(relativePath, removeAfter)` | 提升（完成后 emit `analytics-resource-changed`） |           ✅           |

> **状态说明**：21/21 main 命令全部封装。回收站、元数据、内容搜索、文件监控、"提升"机制全栈打通。

### 4.2 前端 Composable（useScratchpad）完整导出

```typescript
export function useScratchpad() {
  return {
    // ── 响应式状态 ──
    response, // Ref<ScratchpadResponse | null>
    isLoading, // Ref<boolean>
    error, // Ref<string | null>
    searchQuery, // Ref<string>
    localEntries, // ComputedRef<ScratchpadEntry[]>   (搜索过滤)
    externalReferences, // ComputedRef<ExternalReference[]> (搜索过滤)
    scratchpadPath, // ComputedRef<string>
    invalidReferences, // ComputedRef<ExternalReference[]>
    validReferences, // ComputedRef<ExternalReference[]>

    // ── CRUD ──
    loadFiles, // () => Promise<void>
    createEntry, // (name, isFolder) => Promise<ScratchpadEntry | null>
    deleteEntry, // (relativePath) => Promise<boolean>
    renameEntry, // (relativePath, newName) => Promise<ScratchpadEntry | null>
    loadFileContent, // (relativePath) => Promise<string | null>
    saveFile, // (relativePath, content) => Promise<boolean>

    // ── 导入 & 引用 ──
    importFile, // (sourcePath) => Promise<ScratchpadEntry | null>
    addReference, // (alias, path) => Promise<ExternalReference | null>
    removeReference, // (alias) => Promise<boolean>

    // ── 工具 ──
    isRefValid, // (ref) => boolean
    isRefInvalid, // (ref) => boolean
    findEntry, // (entryPath) => ScratchpadEntry | undefined
    openInExplorerAction, // (path) => Promise<boolean>
    getFileSize, // (relativePath) => Promise<number | null>
    clearError, // () => void
    saveFileMeta, // (relativePath, connectionId?) => Promise<boolean>
    searchContent, // (query: string) => Promise<SearchMatch[]>
    trashEntries, // Ref<ScratchpadEntry[]>
    loadTrashEntries, // () => Promise<void>
    restoreTrashEntry, // (trashName) => Promise<boolean>
    emptyTrashBin, // () => Promise<boolean>
    analyzableFiles, // Ref<AnalyzableFile[]>
    loadAnalyzableFiles, // () => Promise<void>

    // ── 文件监控 ──
    startWatching, // () => Promise<void>
    stopWatching, // () => Promise<void>

    // ── 提升机制 ──
    promoteToResource, // (relativePath: string, removeAfter: boolean) => Promise<PromoteResult | null>
  }
}
```

---

## 五、全栈数据流

```
┌── Frontend (Vue 3 + TS) ────────────────────────────────────┐
│  ScratchpadPanel.vue ─ ─ use ─ ─ ▶ useScratchpad()          │
│       │                             │ invoke()               │
│       ▼                             ▼                        │
│  ScratchpadTreeNode.vue    scratchpad-api.ts                 │
│  (递归树 + 内联重命名)      (11 个 IPC 封装)                  │
├──────────────────────────────────────┼───────────────────────┤
│                            Tauri IPC Bridge                   │
├──────────────────────────────────────┼───────────────────────┤
│                                      ▼                        │
│  scratchpad_commands.rs (21 cmd) ─ ─ ▶ ScratchpadState        │
│                                      └─ ─ ▶ ScratchpadStore   │
│                                           ├─ resolve_path()  │
│                                           ├─ scan_dir()      │
│                                           ├─ save_file()     │
│                                           └─ delete_entry()  │
│                                                ↓              │
│                                  {project}/.scratchpad/       │
└──────────────────────────────────────────────────────────────┘
```

---

## 六、路径安全 & 原子写入 & 回收站

| 机制             | 实现                                                                                                                          |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| **路径遍历防护** | `resolve_path_impl` 拒绝 `..` + canonicalize 前缀校验 + `validate_name` 拒绝 `/` `\` `..`                                     |
| **原子写入**     | `save_file`: `fs::write(tmp_path, content)` → `fs::rename(tmp_path → file_path)` → 失败清理 tmp                               |
| **回收站**       | `delete_entry`: `rename(target → .trash/unique_name)`；恢复时 `rename` 回 `.scratchpad/`；`empty_trash` 直接 `remove_dir_all` |

---

## 七、编辑器类型映射（openFileInEditor）

| 后缀                     | 目标编辑器                                          | 状态 |
| ------------------------ | --------------------------------------------------- | :--: |
| `.sql`                   | **sql-editor**（核心场景——自动恢复连接 + 执行引擎） |  ✅  |
| `.py`                    | code-editor（Monaco Python 高亮 + Ctrl+S 保存）     |  ✅  |
| `.csv`                   | data-preview / DuckDB 分析入口                      |  ✅  |
| `.json` / `.txt` / `.md` | code-editor                                         |  ✅  |

> **✅ 全部打通**：`openFileInEditor()` 已完整实现，含 50MB 大小预检、file_meta 恢复连接、dockview 面板创建。

---

## 八、文件变更记录（自 v1.1 起）

| 变更                               | 文件                                                                                                                                               | 说明                                                                                                                                                                                                                                                |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ✅ ScratchpadState 缓存            | `state.rs` (新)                                                                                                                                    | `Arc<Mutex<Option<ScratchpadStore>>>` 避免每次命令重建 Store                                                                                                                                                                                        |
| ✅ 回收站 (.trash/)                | `store.rs`                                                                                                                                         | `delete_entry` 改为 rename 到 `.trash/`；新增 list/restore/empty                                                                                                                                                                                    |
| ✅ DuckDB 分析元数据               | `models.rs`                                                                                                                                        | `AnalyzableFile` 结构体 + `duckdb_query_hint` 映射表 + `get_analyzable_files`                                                                                                                                                                       |
| ✅ 命令层重构                      | `commands`                                                                                                                                         | 全部改用 `ScratchpadState`；新增 4 命令                                                                                                                                                                                                             |
| ✅ 安全合规修复                    | `store.rs`                                                                                                                                         | `resolve_path` 中 unwrap 替换为 CoreError                                                                                                                                                                                                           |
| ✅ 原子写入                        | `store.rs`                                                                                                                                         | 先写 `.tmp` 临时文件，再 rename；失败清理 tmp                                                                                                                                                                                                       |
| ✅ 前端对话框                      | `ScratchpadPanel.vue`                                                                                                                              | 新建/导入/引用的模态框                                                                                                                                                                                                                              |
| ✅ 右键菜单                        | `ScratchpadPanel.vue`                                                                                                                              | 打开/重命名/删除/复制路径/展开折叠 + 移除引用/打开位置                                                                                                                                                                                              |
| ✅ F2 内联重命名                   | `ScratchpadTreeNode.vue`                                                                                                                           | Enter/Escape/Blur 确认取消                                                                                                                                                                                                                          |
| ✅ 大文件检查                      | Panel + Store                                                                                                                                      | 前端 50MB 前置校验 + 后端 `check_file_size`                                                                                                                                                                                                         |
| ✅ 系统管理器打开                  | `store.rs` + `opener` crate                                                                                                                        | 外部引用右键"打开位置"                                                                                                                                                                                                                              |
| ✅ 右键菜单溢出检测                | `ScratchpadPanel.vue`                                                                                                                              | `clampToViewport()` 防越界                                                                                                                                                                                                                          |
| ✅ 文件监控                        | `state.rs` + `commands`                                                                                                                            | `notify` crate + `AtomicBool` watcher + `scratchpad-changed` event                                                                                                                                                                                  |
| ✅ 防重复 Tab                      | `WorkbenchView.vue`                                                                                                                                | `handleOpenSqlEditor` 检测同 `scratchpadRelativePath` 面板                                                                                                                                                                                          |
| ✅ 拖放导入                        | `ScratchpadPanel.vue`                                                                                                                              | dragover/dragleave/drop 文件从资源管理器导入                                                                                                                                                                                                        |
| ✅ 主题变量对齐                    | `ScratchpadTreeNode.vue`                                                                                                                           | CSS 变量迁移至 `--color-bg-*` / `--color-text-*` 全局主题                                                                                                                                                                                           |
| ✅ 工具栏人性化优化                | `ScratchpadPanel.vue`                                                                                                                              | 两行布局（操作行+搜索行）、`type="primary"` 主操作、硬编码文本 i18n 化                                                                                                                                                                              |
| ✅ "提升"机制                      | `commands.rs` + 全栈链路                                                                                                                           | `promote_scratchpad_to_resource` → CreateResourceRequest → 分析资源管理器                                                                                                                                                                           |
| ✅ 键盘导航                        | `ScratchpadPanel.vue`                                                                                                                              | ↑↓/Enter/F2/Delete/Ctrl+N 快捷键 + `scrollToSelected` 自动滚屏                                                                                                                                                                                      |
| ✅ 重命名 feedback                 | `ScratchpadTreeNode.vue`                                                                                                                           | `renamingSaving` loading spinner + input `:disabled`                                                                                                                                                                                                |
| ✅ 提升事件联动                    | `commands.rs`                                                                                                                                      | `app.emit("analytics-resource-changed")` 通知分析资源面板刷新                                                                                                                                                                                       |
| ✅ Entry 精简 + SearchMatch (v2.2) | `models.rs`, `store.rs`, `types/index.ts`, `ScratchpadPanel.vue`                                                                                   | 移除 `extension`/`is_external_ref`，`modified_at` 改 ISO 8601；新增 `SearchMatch` 结构体（file+line_number+line_content）；搜索返回行级上下文                                                                                                       |
| ✅ 交互增强 (v2.3)                 | `ScratchpadPanel.vue`, `ScratchpadTreeNode.vue`, `SqlEditorPanel.vue`, `WorkbenchView.vue`, `sql.ts`                                               | 新建文件夹入口/搜索点击跳转（`initialLine` → Monaco `revealLineInCenter`）/修改时间相对显示（分钟/小时/天）/Toast 反馈（`createDiscreteApi`）/extension 修复                                                                                        |
| ✅ 搜索增强 + Bug (v2.4)           | `store.rs`, `commands.rs`, `ScratchpadPanel.vue`, `ScratchpadTreeNode.vue`                                                                         | `case_sensitive` 参数大小写搜索/折叠展开全部/搜索文本高亮（`v-html` + `<mark>`）/`.duckdb` 图标/`isAnalyzableFile` extension 修复                                                                                                                   |
| ✅ 最近打开 + 空状态 (v2.5)        | `ScratchpadPanel.vue`                                                                                                                              | `recentFiles` 内存列表（最大 5）+ `addRecentFile` 去重推到首位 + `recentFileEntries` computed 查找 + 可折叠区域；`empty-state` 图标标题引导按钮                                                                                                     |
| ✅ 多选批量操作 (v2.6)             | `ScratchpadPanel.vue`, `ScratchpadTreeNode.vue`                                                                                                    | `selectedKeys` Set 多选 + `lastSelectPath` 范围选择 + Ctrl/Shift 点击处理 + 批量删除 confirm toast + `clipboardEntry` 复制粘贴 + Ctrl+A 全选 + `openFileInEditor` extension Bug 修复                                                                |
| ✅ 搜索安全加固 (v2.8)             | `models.rs`, `store.rs`, `commands.rs`, `types/index.ts`, `scratchpad-api.ts`, `use-scratchpad.ts`, `ScratchpadPanel.vue`, `zh-CN.json`, `en.json` | `SearchResult` 结构体（matches/total_scanned/total_skipped/skipped_files/truncated）；`MAX_SEARCH_FILE_SIZE = 10MB` 大文件跳过；`MAX_SEARCH_RESULTS = 500` 结果截断；frontend notice bar 黄色警告 + search-no-results 空态；3 新 i18n key + EN 同步 |
| ✅ 流式搜索 (v2.9)                 | `store.rs`                                                                                                                                         | `read_to_string` → `BufReader::lines()` 逐行流式读取；`search_single_file` 独立 async；`tokio::time::timeout` 30s 超时；移除 MAX_SEARCH_FILE_SIZE；1GB 文件搜索内存从 1GB 降至 ~8KB                                                                 |

</parameter>
</｜DSML｜inv
