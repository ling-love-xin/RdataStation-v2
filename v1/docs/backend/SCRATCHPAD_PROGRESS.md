# 草稿箱 (Scratchpad) 开发进度

> 版本：v3.15
> 最后更新：2026-05-13
> 状态：✅ v3.15 — P0 操作闭环三件套：剪切移动/正则替换/Diff对比 (MVP 达成)

---

## 开发时间线

| 阶段                                                                          | 状态      | 日期           |
| ----------------------------------------------------------------------------- | --------- | -------------- |
| 阶段一：Rust 数据模型 + Store                                                 | ✅        | 2026-05-07     |
| 阶段二：Rust Command 层                                                       | ✅        | 2026-05-07     |
| 阶段三：命令注册                                                              | ✅        | 2026-05-07     |
| 阶段四：前端 TS 类型 + API                                                    | ✅        | 2026-05-07     |
| 阶段五：前端 Composable                                                       | ✅        | 2026-05-07     |
| 阶段六：前端 Vue 组件                                                         | ✅        | 2026-05-07     |
| 阶段七：扩展注册 + 活动栏                                                     | ✅        | 2026-05-07     |
| 阶段八：测试验证                                                              | ✅        | 2026-05-07     |
| 阶段九：文档记录                                                              | ✅        | 2026-05-07     |
| 阶段十：交互优化 (v1.1)                                                       | ✅        | 2026-05-07     |
| 阶段十一：安全合规 + 性能优化 (v2.0)                                          | ✅        | 2026-05-07     |
| **阶段十二：SQL 编辑器集成 (v2.2)**                                           | **✅**    | **2026-05-08** |
| **阶段十三：文件元数据层 (v2.3)**                                             | **✅**    | **2026-05-08** |
| **阶段十四：Bug修复 + 代码编辑器 + 内容搜索 (v2.4)**                          | **✅**    | **2026-05-08** |
| **阶段十五：内容搜索修复 + Config 缓存 (v2.5)**                               | **✅**    | **2026-05-08** |
| **阶段十六：回收站 UI + 自动保存 (v2.6)**                                     | **✅**    | **2026-05-08** |
| **阶段十七：主题适配 + DuckDB 分析 (v2.7)**                                   | **✅**    | **2026-05-08** |
| **阶段十八：文件监控 + 防重复Tab + 拖放导入 + 主题对齐 + 工具栏优化 (v2.8)**  | **✅**    | **2026-05-08** |
| **阶段十九："提升"机制 (v2.9)**                                               | **✅**    | **2026-05-08** |
| **阶段二十：UX 增强 — 键盘导航/重命名反馈/提升事件 (v3.0)**                   | **✅**    | **2026-05-08** |
| **阶段二十一：文件排序 + 搜索行上下文 (v3.1)**                                | **✅**    | **2026-05-09** |
| **阶段二十二：交互增强 — 文件夹/搜索跳转/修改时间/Toast (v3.2)**              | **✅**    | **2026-05-09** |
| **阶段二十三：搜索增强 + 树操作 — 大小写/折叠展开/高亮/Bug (v3.3)**           | **✅**    | **2026-05-09** |
| **阶段二十四：最近打开 + 空状态引导 (v3.4)**                                  | **✅**    | **2026-05-09** |
| **阶段二十五：多选批量操作 + 复制粘贴 (v3.5)**                                | **✅**    | **2026-05-09** |
| **阶段二十六：文件模板 + 拖放插入编辑器 + 删除撤销 (v3.6)**                   | **✅**    | **2026-05-09** |
| **阶段二十七：搜索安全加固 — 大文件跳过/结果截断 (v3.7)**                     | **✅→♻️** | **2026-05-09** |
| **阶段二十八：流式搜索 — BufReader 逐行读取 (v3.8)**                          | **✅**    | **2026-05-09** |
| **阶段二十九：质量加固 — unwrap清除/类型对齐/i18n补全 (v3.9)**                | **✅**    | **2026-05-09** |
| **阶段四十：VSCode 差距消除 — 树结构/内联创建/搜索上下文/子目录创建 (v3.13)** | **✅**    | **2026-05-13** |
| **阶段四十一：P0 三件套 — 虚拟滚动/脏状态/懒加载 (v3.14)**                    | **✅**    | **2026-05-13** |
| **阶段四十二：P0 操作闭环三件套 — 剪切移动/正则替换/Diff对比 (v3.15)**        | **✅**    | **2026-05-13** |

---

## 新增文件清单

### Rust 后端 (5 文件)

| 文件                              | 行数  | 说明                                                                                                 |
| --------------------------------- | ----- | ---------------------------------------------------------------------------------------------------- |
| `core/scratchpad/mod.rs`          | ~10   | 模块入口，re-export                                                                                  |
| `core/scratchpad/models.rs`       | ~82   | DTO 数据模型（Entry / FileMeta / AnalyzableFile / Reference / Response / SearchMatch / ChangeEvent） |
| `core/scratchpad/state.rs`        | ~72   | `Arc<Mutex<Option<Store>>>` 全局状态缓存 + `AtomicBool` watcher 状态                                 |
| `core/scratchpad/store.rs`        | ~1180 | 文件系统操作（扫描/创建/删除/搜索/导入/引用/回收站/树结构/懒加载/移动/替换/diff）                    |
| `commands/scratchpad_commands.rs` | ~420  | 26 个 Tauri Command                                                                                  |

### 前端 (8 文件)

| 文件                                              | 行数  | 说明                                                                                                                                                                                                                                                |
| ------------------------------------------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `scratchpad/package.json`                         | 7     | 扩展元数据                                                                                                                                                                                                                                          |
| `scratchpad/extension.ts`                         | ~94   | 扩展注册（dockview 面板）                                                                                                                                                                                                                           |
| `scratchpad/types/index.ts`                       | ~82   | TypeScript 类型定义（含 ReplaceResult / DiffLine / DiffResult）                                                                                                                                                                                     |
| `scratchpad/infrastructure/api/scratchpad-api.ts` | ~178  | Tauri invoke 封装（27 个 API）                                                                                                                                                                                                                      |
| `scratchpad/ui/composables/use-scratchpad.ts`     | ~660  | 业务逻辑 hook（48 个导出项，含脏状态/懒加载/剪切/替换/diff/正则验证）                                                                                                                                                                               |
| `scratchpad/ui/components/ScratchpadPanel.vue`    | ~1950 | 主面板组件（工具栏排序/搜索双模式/回收站/右键菜单/提升确认/拖放导入/文件夹新建/Toast/搜索跳转/大小写/高亮/折叠展开/最近打开/空状态/多选批量/复制粘贴/模板选择/删除撤销/拖拽到编辑器/内联创建/搜索上下文/虚拟滚动/冲突检测/正则/替换/Diff/移动撤销） |
| `scratchpad/ui/components/ScratchpadTreeNode.vue` | ~430  | 递归树节点组件（+重命名 spinner + disabled + entry.children 真正树结构 + 内联创建行 + 脏状态点）                                                                                                                                                    |

---

## 修改文件清单

| 文件                         | 修改内容                                                          | 影响 |
| ---------------------------- | ----------------------------------------------------------------- | ---- |
| `core/mod.rs`                | 新增 `pub mod scratchpad` + re-exports                            | 低   |
| `commands/mod.rs`            | 新增 `pub mod scratchpad_commands` + re-export                    | 低   |
| `lib.rs`                     | `generate_handler![]` 新增 23 个命令                              | 低   |
| `core/builtin-extensions.ts` | 导入注册 scratchpad 扩展                                          | 低   |
| `layout-store.ts`            | `leftActivityItems` 新增草稿箱入口 + `ACTIVEBAR_TO_PANEL_ID` 映射 | 低   |

---

## 验证结果

| 检查项                        | 历史结果                                    |
| ----------------------------- | ------------------------------------------- |
| `cargo check`                 | ✅ 编译通过，0 错误                         |
| `rustfmt`                     | ✅ 格式化通过                               |
| `cargo clippy`                | ⚠️ 环境问题（Windows 进程管理），非代码问题 |
| `pnpm typecheck` (scratchpad) | ✅ 草稿箱相关 0 错误                        |
| `pnpm lint`                   | ⚠️ 错误均为已有文件问题，与本次变更无关     |

---

## 已验证功能 ✅

- [x] Rust 数据模型（ScratchpadEntry、ExternalReference、ScratchpadConfig、ScratchpadResponse）
- [x] 文件系统操作 Store（增删改查、导入、引用管理）
- [x] 路径安全校验（防止 `..` 遍历攻击、路径前缀检查）
- [x] 唯一路径生成（导入重名文件自动追加后缀 `_1` / timestamp）
- [x] 原子写入（`.tmp` → `rename` → 失败清理）
- [x] 18 个 Tauri Command 完整实现
- [x] 前端 TypeScript 类型定义
- [x] 前端 API 层封装（11 个函数，覆盖核心 12 个命令）
- [x] Composable use-scratchpad（状态管理 + 业务逻辑 + 引用有效性检测 + 搜索过滤）
- [x] ScratchpadPanel 主面板（工具栏 + 分组 + 搜索 + 加载/错误/空状态）
- [x] ScratchpadTreeNode 递归树节点（图标映射 + 文件大小显示 + 内联重命名）
- [x] 扩展注册（dockview left 区域 + 活动栏 FileText 图标）
- [x] 外部引用分组（折叠/展开 + 失效检测 + "已失效"徽章）
- [x] 本地草稿分组（空状态提示）
- [x] 搜索过滤（文件名 + 引用别名/路径）
- [x] 新建文件对话框（自动聚焦 + Enter 确认）
- [x] 导入文件对话框（@tauri-apps/plugin-dialog 集成）
- [x] 添加外部引用对话框（别名 + 路径 + 浏览按钮）
- [x] 右键菜单（打开/重命名/折叠展开/复制路径/删除）
- [x] 外部引用右键菜单（移除引用/打开位置）
- [x] F2 内联重命名（自动聚焦全选 / Enter 确认 / Escape 取消 / Blur 提交）
- [x] Delete 键删除选中条目
- [x] Ctrl+N 快捷键新建草稿
- [x] 右键菜单溢出检测 `clampToViewport()`
- [x] `isRefInvalid` 去重到 composable
- [x] 大文件检查（前端 50MB 前置校验 + 后端 `check_file_size`）
- [x] 系统文件管理器打开（`opener` crate）

---

## v2.2 已完成 ✅

### SQL 编辑器集成（阶段十二）

| #   | 任务                                                                   | 状态 | 涉及文件                  |
| --- | ---------------------------------------------------------------------- | :--: | ------------------------- |
| 1   | `SqlEditorParams` 新增 `scratchpadRelativePath` / `scratchpadFileName` |  ✅  | `src/shared/types/sql.ts` |
| 2   | `openFileInEditor` 打通——读取文件 → 派发事件 → dockview 创建面板       |  ✅  | `ScratchpadPanel.vue`     |
| 3   | SqlEditorPanel 精简模式——`scratchpadRelativePath` 非空时关闭方言功能   |  ✅  | `SqlEditorPanel.vue`      |
| 4   | Ctrl+S 保存走 `save_scratchpad_file`                                   |  ✅  | `SqlEditorPanel.vue`      |
| 5   | 编辑器 tab 标题显示草稿文件名                                          |  ✅  | `WorkbenchView.vue`       |
| 6   | EditorToolbar `showAdvanced` prop 控制高级按钮                         |  ✅  | `EditorToolbar.vue`       |

### v2.2 实际改动文件

| 文件                      | 改动类型                                                     | 改动量 |
| ------------------------- | ------------------------------------------------------------ | :----: |
| `src/shared/types/sql.ts` | 新增 2 字段                                                  |  +2行  |
| `WorkbenchView.vue`       | 透传 params + 标题                                           | ~10行  |
| `SqlEditorPanel.vue`      | 精简模式 + Ctrl+S + invoke import                            | ~30行  |
| `EditorToolbar.vue`       | `showAdvanced` prop + v-if                                   | ~10行  |
| `ScratchpadPanel.vue`     | `openFileInEditor` 实现 + `loadFileContent`/`scratchpadPath` | ~25行  |

> **Rust 后端：零改动。**

### v2.3 已完成 ✅

### 文件元数据层（阶段十三）

| #   | 任务                                                      | 状态 | 涉及文件                      |
| --- | --------------------------------------------------------- | :--: | ----------------------------- |
| 1   | `FileMeta` 数据模型（Rust + TS）                          |  ✅  | `models.rs`, `types/index.ts` |
| 2   | `store.update_file_meta()` 方法                           |  ✅  | `store.rs`                    |
| 3   | `update_scratchpad_file_meta` Tauri Command               |  ✅  | `commands.rs`, `lib.rs`       |
| 4   | 前端 API `updateFileMeta()` + Composable `saveFileMeta()` |  ✅  | `api.ts`, `use-scratchpad.ts` |
| 5   | `openFileInEditor` 自动恢复 `lastConnectionId`            |  ✅  | `ScratchpadPanel.vue`         |
| 6   | `handleScratchpadSave` / `handleExecute` 写入 file_meta   |  ✅  | `SqlEditorPanel.vue`          |

### v2.3 实际改动文件

| 文件                              | 改动类型                                                       | 改动量 |
| --------------------------------- | -------------------------------------------------------------- | :----: |
| `core/scratchpad/models.rs`       | 新增 `FileMeta` + 更新 Config/Response                         | +12行  |
| `core/scratchpad/mod.rs`          | pub use 新增 `FileMeta`                                        |  +1行  |
| `core/scratchpad/store.rs`        | 新增 `update_file_meta()` + import `FileMeta`                  | +20行  |
| `commands/scratchpad_commands.rs` | 新增 `update_scratchpad_file_meta` 命令                        | +13行  |
| `lib.rs`                          | `generate_handler!` 注册新命令                                 |  +1行  |
| `types/index.ts`                  | 新增 `FileMeta` + 更新 `ScratchpadResponse`                    |  +7行  |
| `scratchpad-api.ts`               | 新增 `updateFileMeta()`                                        | +10行  |
| `use-scratchpad.ts`               | 新增 `saveFileMeta()` + import                                 | +14行  |
| `ScratchpadPanel.vue`             | `openFileInEditor` 读 file_meta + `response`/`computed` import |  +5行  |
| `SqlEditorPanel.vue`              | save/execute 时调用 `update_scratchpad_file_meta`              | +12行  |

### v2.4 已完成 ✅

### Bug 修复 + 代码编辑器 + 内容搜索（阶段十四）

| #   | 任务                                                          | 状态 | 涉及文件                      |
| --- | ------------------------------------------------------------- | :--: | ----------------------------- |
| 1   | `rename_entry()` 迁移 file_meta 键                            |  ✅  | `store.rs`                    |
| 2   | 代码编辑器打通 `.py`/`.json`/`.txt`/`.md`                     |  ✅  | 3 files                       |
| 3   | `search_file_content()` Rust 方法                             |  ✅  | `store.rs`                    |
| 4   | `search_scratchpad_content` Tauri Command                     |  ✅  | `commands.rs`, `lib.rs`       |
| 5   | 前端 API `searchFileContent()` + Composable `searchContent()` |  ✅  | `api.ts`, `use-scratchpad.ts` |
| 6   | 面板搜索双模式（文件名/全文）+ Search 按钮                    |  ✅  | `ScratchpadPanel.vue` + i18n  |

### v2.4 实际改动文件

| 文件                              | 改动类型                                   | 改动量 |
| --------------------------------- | ------------------------------------------ | :----: |
| `core/scratchpad/store.rs`        | 新增 `search_file_content()` + rename 迁移 | +30行  |
| `commands/scratchpad_commands.rs` | 新增 `search_scratchpad_content` 命令      | +12行  |
| `lib.rs`                          | 注册新命令                                 |  +1行  |
| `types/sql.ts`                    | 新增 `language` 字段                       |  +1行  |
| `scratchpad-api.ts`               | 新增 `searchFileContent()`                 |  +8行  |
| `use-scratchpad.ts`               | 新增 `searchContent()` + import            | +14行  |
| `zh-CN.json`                      | 新增 `searchContent`/`searchByFilename`    |  +2行  |
| `ScratchpadPanel.vue`             | 搜索双模式 + 非SQL文件事件派发             | +30行  |
| `WorkbenchView.vue`               | 透传 `language` 参数                       |  +3行  |
| `SqlEditorPanel.vue`              | 动态 language + 非SQL隐藏工具栏            | +12行  |

### v2.5 已完成 ✅

### 内容搜索修复 + Config 缓存（阶段十五）

| #   | 任务                                              | 状态 | 涉及文件              |
| --- | ------------------------------------------------- | :--: | --------------------- |
| 1   | 修复 `filteredLocalEntries` 路径匹配（相对↔绝对） |  ✅  | `ScratchpadPanel.vue` |
| 2   | `ScratchpadStore` Config 内存缓存                 |  ✅  | `store.rs`            |

### v2.5 实际改动文件

| 文件                       | 改动类型                                                  | 改动量 |
| -------------------------- | --------------------------------------------------------- | :----: |
| `ScratchpadPanel.vue`      | `filteredLocalEntries` 路径对齐                           |  +7行  |
| `core/scratchpad/store.rs` | struct + `new()` + `load_config()` + `save_config()` 缓存 | +25行  |

### v2.6 已完成 ✅

### 回收站 UI + 自动保存（阶段十六）

| #   | 任务                                                                                         | 状态 | 涉及文件                             |
| --- | -------------------------------------------------------------------------------------------- | :--: | ------------------------------------ |
| 1   | 前端 API `listTrash()` / `restoreFromTrash()` / `emptyTrash()`                               |  ✅  | `scratchpad-api.ts`                  |
| 2   | Composable `trashEntries` + `loadTrashEntries()` / `restoreTrashEntry()` / `emptyTrashBin()` |  ✅  | `use-scratchpad.ts`                  |
| 3   | 回收站折叠面板（恢复 / 清空按钮）+ i18n                                                      |  ✅  | `ScratchpadPanel.vue` + `zh-CN.json` |
| 4   | `markDirty()` 触发 2s 防抖 `scheduleAutoSave()`                                              |  ✅  | `SqlEditorPanel.vue`                 |

### v2.6 实际改动文件

| 文件                  | 改动类型                                               | 改动量 |
| --------------------- | ------------------------------------------------------ | :----: |
| `scratchpad-api.ts`   | 新增 `listTrash()`/`restoreFromTrash()`/`emptyTrash()` | +20行  |
| `use-scratchpad.ts`   | `trashEntries` ref + 3 方法 + 4 exports                | +38行  |
| `zh-CN.json`          | 回收站 i18n 5 个 key                                   |  +5行  |
| `ScratchpadPanel.vue` | 回收站折叠面板 + script 状态/方法                      | +45行  |
| `SqlEditorPanel.vue`  | `markDirty()` 改为触发 auto-save + timer 清理          | +14行  |

### v2.7 已完成 ✅

### 主题适配 + DuckDB 分析（阶段十七）

| #   | 任务                                                  | 状态 | 涉及文件                              |
| --- | ----------------------------------------------------- | :--: | ------------------------------------- |
| 1   | CSS 变量对齐 `global.css` 主题系统（10+ 处）          |  ✅  | `ScratchpadPanel.vue`                 |
| 2   | `AnalyzableFile` TS 接口 + `getAnalyzableFiles()` API |  ✅  | `types/index.ts`, `scratchpad-api.ts` |
| 3   | `loadAnalyzableFiles()` composable 方法               |  ✅  | `use-scratchpad.ts`                   |
| 4   | 右键菜单 "用 DuckDB 分析"（csv/parquet/json/xlsx）    |  ✅  | `ScratchpadPanel.vue` + i18n          |

### v2.7 实际改动文件

| 文件                  | 改动类型                                                     | 改动量 |
| --------------------- | ------------------------------------------------------------ | :----: |
| `ScratchpadPanel.vue` | CSS 变量全面对齐 + DuckDB 分析入口                           | +30行  |
| `types/index.ts`      | 新增 `AnalyzableFile` 接口                                   |  +7行  |
| `scratchpad-api.ts`   | 新增 `getAnalyzableFiles()`（17 个 IPC）                     |  +6行  |
| `use-scratchpad.ts`   | `analyzableFiles` ref + `loadAnalyzableFiles()`（29 个导出） | +15行  |
| `zh-CN.json`          | 新增 `analyzeWithDuckDB`                                     |  +1行  |

### v2.8 已完成 ✅

### 文件监控 + 防重复Tab + 拖放导入 + 主题变量对齐（阶段十八）

| #   | 任务                                                       | 状态 | 涉及文件                                             |
| --- | ---------------------------------------------------------- | :--: | ---------------------------------------------------- |
| 1   | 防重复 Tab — 同一草稿文件不重复打开 editor tab             |  ✅  | `WorkbenchView.vue`                                  |
| 2   | 拖放导入 — 从文件资源管理器拖放文件到草稿箱面板            |  ✅  | `ScratchpadPanel.vue`                                |
| 3   | 文件监控 — `notify` crate + Tauri event 推送               |  ✅  | `Cargo.toml`, `state.rs`, `commands.rs`, `lib.rs`    |
| 4   | 前端事件监听 — `listen('scratchpad-changed')` 自动刷新     |  ✅  | `api.ts`, `use-scratchpad.ts`, `ScratchpadPanel.vue` |
| 5   | CSS 变量对齐 — `ScratchpadTreeNode.vue` 迁移至全局主题变量 |  ✅  | `ScratchpadTreeNode.vue`                             |

### v2.8 实际改动文件

| 文件                                   | 改动类型                                                                      | 改动量 |
| -------------------------------------- | ----------------------------------------------------------------------------- | :----: |
| `Cargo.toml`                           | 新增 `notify = "6"` 依赖                                                      |  +2行  |
| `core/scratchpad/state.rs`             | 新增 `AtomicBool` watcher 状态 + `is_watching`/`set_watching` 方法            | +15行  |
| `commands/scratchpad_commands.rs`      | 新增 `watch_scratchpad` / `unwatch_scratchpad` (22 cmd)                       | +65行  |
| `lib.rs`                               | 注册 `watch_scratchpad` / `unwatch_scratchpad`                                |  +2行  |
| `infrastructure/api/scratchpad-api.ts` | 新增 `watchScratchpad()` / `unwatchScratchpad()` (19 API)                     |  +8行  |
| `ui/composables/use-scratchpad.ts`     | 新增 `startWatching()` / `stopWatching()` (31 exports)                        | +16行  |
| `ui/components/ScratchpadPanel.vue`    | 拖放导入 dragover/dragleave/drop + Tauri event listener + onMounted/Unmounted | +20行  |
| `ui/components/ScratchpadTreeNode.vue` | CSS 变量迁移至 `--color-*` 主题系统                                           |  ~4行  |

### 文件监控技术细节

**Rust 侧 (notify crate)**：

- `watch_scratchpad` 命令：创建 `RecommendedWatcher`，监控 `.scratchpad/` 递归目录
- `unwatch_scratchpad` 命令：设置 `AtomicBool` 停止标志
- 防抖：500ms 内重复变更仅触发一次 `scratchpad-changed` event
- 幂等：重复调用 `watch_scratchpad` 自动跳过（`is_watching()` 检测）

**前端侧 (Tauri event)**：

- `onMounted`: 调用 `startWatching()` → `invoke('watch_scratchpad')`，注册 `listen('scratchpad-changed', loadFiles)`
- `onUnmounted`: 注销 event listener → `stopWatching()` → `invoke('unwatch_scratchpad')`

**拖放导入技术细节**：

- `dragover.prevent` + `dragleave` 控制 `.dragover-active` CSS class（虚线边框覆盖层）
- `drop.prevent` 读取 `event.dataTransfer.files[].path`（Tauri 提供的本地文件路径）
- 直接调用 `importFile(path)` 导入

---

## 技术细节

### 路径安全

`resolve_path_impl` 三重防御：

1. 禁止 `..` 路径遍历
2. canonicalize 后检查前缀是否在 `.scratchpad/` 下
3. 支持 `must_exist=false` 模式（用于创建文件前的路径校验）

### 数据结构

```rust
pub struct ScratchpadStore {
    scratchpad_dir: PathBuf,  // {project}/.scratchpad/
    config_path: PathBuf,     // {project}/.scratchpad/.scratchpad.json
}
```

Store 不接受外部注入，所有路径从 `project_path` 计算，避免注入攻击。

### 错误处理

全部通过 `CoreError::storage(StorageError::Io { path, operation, reason })` 统一处理，禁止 unwrap/expect。

### 前端交互模式

- **右键菜单**：fixed 定位 + reactive contextMenu state + document click 自动关闭
- **内联重命名**：Vue watch 监听 renamingKey → nextTick → focus/select
- **导入对话框**：动态 import `@tauri-apps/plugin-dialog` + Tauri 环境检测
- **搜索过滤**：computed 属性实时响应 searchQuery 变化

### v2.9 已完成 ✅

### "提升"机制 — 草稿 → 分析资源（阶段十九）

| #   | 任务                                               | 状态 | 涉及文件                |
| --- | -------------------------------------------------- | :--: | ----------------------- |
| 1   | Rust `promote_scratchpad_to_resource` 命令         |  ✅  | `commands.rs`           |
| 2   | 后缀→resource_type 映射                            |  ✅  | `commands.rs`           |
| 3   | 跨模块访问 `AnalyticsResourceState`                |  ✅  | `commands.rs`, `lib.rs` |
| 4   | `PromoteResult` / `AnalyticsResourceBrief` TS 类型 |  ✅  | `types/index.ts`        |
| 5   | `promoteScratchpadToResource()` API                |  ✅  | `api.ts`                |
| 6   | `promoteToResource()` composable                   |  ✅  | `use-scratchpad.ts`     |
| 7   | 右键菜单 "提升为分析资源" + `GitBranch` 图标       |  ✅  | `ScratchpadPanel.vue`   |
| 8   | 确认对话框：两个按钮（保留/删除原稿）              |  ✅  | `ScratchpadPanel.vue`   |
| 9   | i18n 键 + lib.rs 注册                              |  ✅  | `zh-CN.json`, `lib.rs`  |

### v2.9 变更文件

| 文件                              | 改动                                                                                   | 行数 |
| --------------------------------- | -------------------------------------------------------------------------------------- | :--: |
| `commands/scratchpad_commands.rs` | +`promote_scratchpad_to_resource` + `extension_to_resource_type` + `PromoteResult`     | +90  |
| `lib.rs`                          | 注册 `promote_scratchpad_to_resource`                                                  |  +1  |
| `scratchpad-api.ts`               | +`promoteScratchpadToResource()`                                                       | +10  |
| `types/index.ts`                  | +`PromoteResult` / `AnalyticsResourceBrief`                                            | +12  |
| `use-scratchpad.ts`               | +`promoteToResource()` + import                                                        | +18  |
| `ScratchpadPanel.vue`             | +`GitBranch` icon + context menu item + NModal confirm dialog + `handlePromoteConfirm` | +25  |
| `zh-CN.json`                      | +`promoteToResource` / `promoteToResourceConfirm`                                      |  +2  |

### "提升"机制数据流

```
草稿文件右键 "提升为分析资源" (GitBranch)
  → showPromoteConfirm = true
  → NModal 确认对话框
  → 用户选择:
      [提升并保留草稿] → promoteToResource(path, removeAfter=false)
      [提升并删除草稿] → promoteToResource(path, removeAfter=true)
  → API: invoke('promote_scratchpad_to_resource', { relativePath, removeAfter })
  → Rust: read_file + check_file_size + extension_to_resource_type
  → Rust: CreateResourceRequest → AnalyticsResourceStore::create_resource
  → Rust: if removeAfter → ScratchpadStore::delete_entry (软删除到 .trash/)
  → 返回 PromoteResult { resource, removed }
  → 前端: removeAfter 时自动 loadFiles() 刷新面板
```

### v3.0 已完成 ✅

### UX 增强 — 键盘导航/重命名反馈/提升事件（阶段二十）

| #   | 任务                                                | 状态 | 涉及文件                 |
| --- | --------------------------------------------------- | :--: | ------------------------ |
| 1   | ↑↓ 键盘导航 + scrollIntoView 自动滚动               |  ✅  | `ScratchpadPanel.vue`    |
| 2   | Enter 打开选中文件                                  |  ✅  | `ScratchpadPanel.vue`    |
| 3   | 重命名 empty 防提交                                 |  ✅  | `ScratchpadTreeNode.vue` |
| 4   | 重命名 loading spinner + disabled                   |  ✅  | `ScratchpadTreeNode.vue` |
| 5   | promote 后 emit `analytics-resource-changed`        |  ✅  | `scratchpad_commands.rs` |
| 6   | TreeNode `children` 修复为 `entry.children \|\| []` |  ✅  | `ScratchpadTreeNode.vue` |

### v3.0 变更文件

| 文件                     | 改动                                                                       | 行数 |
| ------------------------ | -------------------------------------------------------------------------- | :--: |
| `ScratchpadPanel.vue`    | +`handleKeydown`: ArrowUp/Down/Enter 分支 + `scrollToSelected`             | +30  |
| `ScratchpadTreeNode.vue` | +`renamingSaving` + `.rename-wrapper` + `.rename-spinner` + disabled input | +30  |
| `scratchpad_commands.rs` | +`app: AppHandle` param + `app.emit("analytics-resource-changed")`         |  +3  |

### 键盘导航快捷键表

| 按键      | 行为         | 说明                                 |
| --------- | ------------ | ------------------------------------ |
| `↑` / `↓` | 移动选中项   | 在 `filteredLocalEntries` 内循环边界 |
| `Enter`   | 打开选中文件 | = `dblclick` 行为                    |
| `F2`      | 开始重命名   | 保持                                 |
| `Delete`  | 删除选中项   | 软删除到 `.trash/`                   |
| `Ctrl+N`  | 新建文件     | 弹出文件名输入框                     |

### v3.1 已完成 ✅

### 文件排序 + 搜索行上下文（阶段二十一）

| #   | 任务                                                                                                            | 状态 | 涉及文件                                         |
| --- | --------------------------------------------------------------------------------------------------------------- | :--: | ------------------------------------------------ |
| 1   | Rust `ScratchpadEntry` 精简（移除 `extension`/`is_external_ref`，`modified_at` 改为 `Option<String>` ISO 8601） |  ✅  | `models.rs`, `store.rs`                          |
| 2   | `SearchMatch` 结构体 + `search_file_content` 返回 `Vec<SearchMatch>`                                            |  ✅  | `models.rs`, `store.rs`, `mod.rs`, `commands.rs` |
| 3   | 前端 `SearchMatch` 类型 + API/composable 返回类型更新                                                           |  ✅  | `types/index.ts`, `api.ts`, `use-scratchpad.ts`  |
| 4   | 排序下拉菜单（按名称/大小/修改时间，升序/降序切换）                                                             |  ✅  | `ScratchpadPanel.vue`                            |
| 5   | 搜索行上下文展示（文件→匹配计数→行号+行内容，最多 5 行）                                                        |  ✅  | `ScratchpadPanel.vue`                            |
| 6   | i18n 扩展（7 个新 key）                                                                                         |  ✅  | `zh-CN.json`                                     |

### v3.1 变更文件

| 文件                              | 改动                                                                                                                     | 行数 |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------ | :--: |
| `core/scratchpad/models.rs`       | 移除 `extension`/`is_external_ref` + `modified_at` 改类型 + 新增 `SearchMatch`                                           | ~12  |
| `core/scratchpad/store.rs`        | `scan_dir`/`search_file_content`/`create_entry`/`restore_entry`/`rename_entry`/`import_file`/`get_analyzable_files` 适配 | ~30  |
| `core/scratchpad/mod.rs`          | pub use 新增 `SearchMatch`                                                                                               |  +1  |
| `commands/scratchpad_commands.rs` | `search_scratchpad_content` 返回类型改为 `Vec<SearchMatch>`                                                              |  +2  |
| `types/index.ts`                  | 新增 `modified_at` / `SearchMatch` + 移除 `extension`/`is_external_ref`                                                  |  +6  |
| `scratchpad-api.ts`               | `searchFileContent` 返回 `Promise<SearchMatch[]>`                                                                        |  +1  |
| `use-scratchpad.ts`               | `searchContent` 返回 `Promise<SearchMatch[]>`                                                                            |  +1  |
| `ScratchpadPanel.vue`             | 排序工具栏按钮 + `contentResults` Map + 搜索结果面板 + CSS                                                               | +120 |
| `zh-CN.json`                      | 7 个新 i18n 键                                                                                                           |  +7  |

### 排序交互细节

- 默认按**名称升序**排列（首次加载）
- 工具栏右侧三个按钮：按名称 / 按大小 / 按修改时间
- 点击当前排序字段 → 切换升序/降序，图标实时反映：
  - 非当前字段 → `ArrowUpDown`
  - 当前字段升序 → `ArrowUp`
  - 当前字段降序 → `ArrowDown`
- 排序在 `filteredLocalEntries` computed 中通过 `[...entries].sort()` 实现
- 排序与搜索过滤可同时生效（先过滤再排序）

### 内容搜索行上下文细节

- `search_file_content` 返回 `Vec<SearchMatch>`（替代旧的 `Vec<String>` 文件路径）
- 前端 `watch(searchQuery)` 调用 `searchContent()` → 按 `m.file` 分组到 `Map<string, SearchMatch[]>`
- 有结果时，搜索面板替换树视图：
  - 文件图标 + 文件名 + 匹配计数 badge（圆角灰色背景）
  - 每文件最多显示前 5 行匹配：行号（等宽右对齐）+ 行内容（等宽截断）
  - 超过 5 行显示 "...还有 N 处匹配未显示"
- 无结果时，树视图正常显示

### v3.2 已完成 ✅

### 交互增强 — 新建文件夹/搜索跳转/修改时间/Toast（阶段二十二）

| #   | 任务                                     | 状态 | 涉及文件                                                                      |
| --- | ---------------------------------------- | :--: | ----------------------------------------------------------------------------- |
| 1   | 新建文件夹按钮 + 模态框                  |  ✅  | `ScratchpadPanel.vue`                                                         |
| 2   | 搜索结果点击跳转到文件/行                |  ✅  | `ScratchpadPanel.vue` + `SqlEditorPanel.vue` + `WorkbenchView.vue` + `sql.ts` |
| 3   | TreeNode 修改时间相对显示                |  ✅  | `ScratchpadTreeNode.vue`                                                      |
| 4   | Toast 操作反馈（`createDiscreteApi`）    |  ✅  | `ScratchpadPanel.vue`                                                         |
| 5   | TreeNode extension 修复（v3.1 模型适配） |  ✅  | `ScratchpadTreeNode.vue`                                                      |
| 6   | i18n 扩展（10+ 个新 key）                |  ✅  | `zh-CN.json`                                                                  |

### v3.2 变更文件

| 文件                     | 改动                                                                | 行数 |
| ------------------------ | ------------------------------------------------------------------- | :--: |
| `ScratchpadPanel.vue`    | 文件夹按钮+modal+toast+搜索点击跳转                                 | +90  |
| `ScratchpadTreeNode.vue` | 修改时间 `modifiedTime` computed + `node-time` CSS + extension 修复 | +28  |
| `SqlEditorPanel.vue`     | `initialLine` computed + `revealLineInCenter` + `setPosition`       | +12  |
| `WorkbenchView.vue`      | 透传 `initialLine` 参数                                             |  +2  |
| `sql.ts`                 | `SqlEditorParams.initialLine?: number`                              |  +1  |
| `zh-CN.json`             | 10+ 个新 i18n 键                                                    | +12  |

### 交互细节

**新建文件夹：**

- 工具栏 `FolderPlus` 图标按钮，独立 `NModal` 输入框
- Enter 键确认，空值禁止提交
- 调用 `createEntry(name, true)`，成功后 `message.success` toast

**搜索结果点击跳转：**

- 点击文件名 → `openFileAtLine(file, 0)` → 打开文件但不跳转
- 点击行 → `openFileAtLine(file, line_number)` → `initialLine` 通过 `open-sql-editor` CustomEvent → `WorkbenchView` 透传 → `SqlEditorPanel`
- `SqlEditorPanel.onMounted` 中 `await nextTick()` → `editor.revealLineInCenter(initialLine)` + `editor.setPosition({ lineNumber, column: 1 })`

**修改时间相对显示：**

- `<1 分钟`：不显示
- `1–59 分钟`：`{n}m`
- `1–23 小时`：`{n}h`
- `1–6 天`：`{n}d`
- `≥7 天`：不显示（避免过期信息误导）

**Toast 覆盖操作：**

- `createDiscreteApi(['message'])` 在以下操作成功后触发 `message.success()`：
  - 创建文件/文件夹、删除、重命名、导入（文件选择+拖放）、恢复、清空回收站、提升为分析资源

### v3.3 已完成 ✅

### 搜索增强 + 树操作（阶段二十三）

| #   | 任务                                                  | 状态 | 涉及文件                                             |
| --- | ----------------------------------------------------- | :--: | ---------------------------------------------------- |
| 1   | Bug fix: `isAnalyzableFile` 不用 `entry.extension`    |  ✅  | `ScratchpadPanel.vue`                                |
| 2   | Rust `search_file_content` 新增 `case_sensitive` 参数 |  ✅  | `store.rs`, `commands.rs`                            |
| 3   | 前端大小写搜索切换                                    |  ✅  | `api.ts`, `use-scratchpad.ts`, `ScratchpadPanel.vue` |
| 4   | 折叠/展开全部按钮                                     |  ✅  | `ScratchpadPanel.vue`                                |
| 5   | 搜索文本高亮（`v-html` + `<mark>`）                   |  ✅  | `ScratchpadPanel.vue`                                |
| 6   | 复制路径 Toast                                        |  ✅  | `ScratchpadPanel.vue`                                |
| 7   | `.duckdb`/`.parquet` 图标                             |  ✅  | `ScratchpadTreeNode.vue`                             |
| 8   | i18n（4 个新 key）                                    |  ✅  | `zh-CN.json`                                         |

### v3.3 变更文件

| 文件                     | 改动                                                          | 行数 |
| ------------------------ | ------------------------------------------------------------- | :--: |
| `store.rs`               | `search_file_content(case_sensitive)` 条件匹配                |  +6  |
| `commands.rs`            | `search_scratchpad_content(query, case_sensitive)`            |  +2  |
| `scratchpad-api.ts`      | `searchFileContent(query, caseSensitive)`                     |  +1  |
| `use-scratchpad.ts`      | `searchContent(query, caseSensitive)`                         |  +1  |
| `ScratchpadPanel.vue`    | Bug fix + 折叠展开 + 大小写按钮 + 高亮函数 + 复制 toast + CSS | +60  |
| `ScratchpadTreeNode.vue` | `.duckdb`/`.parquet` 图标映射                                 |  +2  |
| `zh-CN.json`             | 4 个新 key                                                    |  +4  |

### 搜索增强交互细节

**大小写搜索：**

- 内容搜索模式激活时显示 `Aa` 切换按钮
- 默认关闭（不区分大小写）
- 点击后变为 `primary` 色，watch 自动重新搜索
- Rust: `case_sensitive=true` → `line.contains(query)`，`false` → `line.to_lowercase().contains(query_lower)`

**搜索文本高亮：**

- `highlightMatch(line, query)` 纯函数：
  - `escapeHtml` 防 XSS 处理
  - `indexOf` 查找匹配位置（不区分大小写）
  - 返回 `"…<mark class='search-hl'>matched</mark>…"` 字符串
- 模板使用 `v-html` 渲染，CSS `:deep(.search-hl)` 黄色半透明背景

**折叠/展开全部：**

- `collectFolderPaths` 递归收集所有 `kind === 'folder'` 的路径
- "展开全部" → `expandedKeys = new Set(allFolders)`
- "折叠全部" → `expandedKeys = new Set()`
- 仅作用于 `filteredLocalEntries`（即当前可见条目）

### v3.4 已完成 ✅

### 最近打开 + 空状态引导（阶段二十四）

| #   | 任务                                      | 状态 | 涉及文件              |
| --- | ----------------------------------------- | :--: | --------------------- |
| 1   | `recentFiles` ref + `addRecentFile` 函数  |  ✅  | `ScratchpadPanel.vue` |
| 2   | `recentFileEntries` computed 查找实际条目 |  ✅  | `ScratchpadPanel.vue` |
| 3   | 可折叠"最近打开"区域模板 + CSS            |  ✅  | `ScratchpadPanel.vue` |
| 4   | 丰富空状态引导（图标+标题+按钮）          |  ✅  | `ScratchpadPanel.vue` |
| 5   | i18n（4 个新 key）                        |  ✅  | `zh-CN.json`          |

### v3.4 变更文件

| 文件                  | 改动                                                                                             | 行数 |
| --------------------- | ------------------------------------------------------------------------------------------------ | :--: |
| `ScratchpadPanel.vue` | recentFiles state + addRecentFile + recentFileEntries computed + 最近打开 section + 空状态 + CSS | +100 |
| `zh-CN.json`          | 4 个新 key                                                                                       |  +4  |

### 交互细节

**最近打开：**

- 纯内存实现（不持久化），会话内有效
- `addRecentFile(relativePath)` 在每次打开文件时调用（`openFileInEditor`/`openFileAtLine`）
- 去重逻辑：先 `filter` 移除已有同路径，再 `unshift` 推到首位，最后 `slice(0, MAX_RECENT=5)`
- `recentFileEntries` computed 将相对路径映射回 `localEntries` 中的实际 `ScratchpadEntry` 对象
- 可折叠区域：`showRecent` ref 控制展开/折叠，ChevronDown/Right 图标
- 仅在非内容搜索模式显示（内容搜索模式由搜索面板接管）
- 点击条目 → `handleOpen(entry)` 打开文件

**空状态引导：**

- 替代旧版纯文本 `empty-hint`
- 居中布局 `flexbox column`，垂直间距 8px
- `FolderOpen` 图标 32px、50% 透明度
- 标题 14px/600 权重
- 引导文本 12px/muted 色
- 两个操作按钮："新建文件"（primary）+ "导入文件"（default）
- 搜索模式下不显示空状态（由搜索结果面板接管）

### v3.5 已完成 ✅

### 多选批量操作 + 复制粘贴（阶段二十五）

| #   | 任务                                                               | 状态 | 涉及文件                 |
| --- | ------------------------------------------------------------------ | :--: | ------------------------ |
| 1   | `selectedKeys` Set 多选 + `lastSelectPath` + `flattenEntries`      |  ✅  | `ScratchpadPanel.vue`    |
| 2   | TreeNode `selectedKeys` prop + `isSelected` 兼容 + MouseEvent emit |  ✅  | `ScratchpadTreeNode.vue` |
| 3   | Ctrl/Shift 点击处理 + 右键菜单自适应                               |  ✅  | `ScratchpadPanel.vue`    |
| 4   | 批量删除（confirm + toast）                                        |  ✅  | `ScratchpadPanel.vue`    |
| 5   | 复制/粘贴文件（clipboard → `_copy`）                               |  ✅  | `ScratchpadPanel.vue`    |
| 6   | Ctrl+A 全选                                                        |  ✅  | `ScratchpadPanel.vue`    |
| 7   | Bug fix: `openFileInEditor` extension                              |  ✅  | `ScratchpadPanel.vue`    |
| 8   | i18n（7 个新 key）                                                 |  ✅  | `zh-CN.json`             |

### v3.5 交互细节

**多选规则：**

- **普通点击**：清除多选，单选当前节点，记录 `lastSelectPath`
- **Ctrl+点击**：切换当前节点入选/出选，不改变其他节点
- **Shift+点击**：从 `lastSelectPath` 到当前节点范围全选（使用 `flattenEntries` 平铺后计算 from/to 区间）
- **右键**：若点击的节点不在选中集合中，清除选中并单选该节点；若已在选中集合中，保持多选

**批量删除：**

- 右键菜单显示 `batch-delete({n})`（仅当 `multiSelected > 1`）
- Delete 键自动判断：多选 → 批量弹窗确认，单选 → 直接删除
- 成功后 toast `batchDeletedSuccess` + 清空 `selectedKeys`

**复制/粘贴：**

- 右键菜单显示 "复制" → `clipboardEntry = entry`
- 当 `clipboardEntry` 非空时，右键菜单末尾显示 "粘贴"
- 粘贴流程：读取源文件内容 → `createEntry(baseName_copy.ext)` → `saveFile` → toast
- 仅支持同一会话内复制（不持久化 clipboard）

### 变更文件

| 文件                     | 改动                                                       | 行数 |
| ------------------------ | ---------------------------------------------------------- | :--: |
| `ScratchpadPanel.vue`    | 多选 + 批量删除 + 复制粘贴 + Ctrl+A + openFileInEditor fix | +120 |
| `ScratchpadTreeNode.vue` | selectedKeys prop + isSelected 兼容 + MouseEvent           |  +8  |
| `zh-CN.json`             | 7 个新 key                                                 |  +7  |

### v3.6 已完成 ✅

### 文件模板 + 拖放插入编辑器 + 删除撤销（阶段二十六）

| #   | 任务                                           | 状态 | 涉及文件                 |
| --- | ---------------------------------------------- | :--: | ------------------------ |
| 1   | 新建文件模板选择器（SQL/JSON/Markdown/Python） |  ✅  | `ScratchpadPanel.vue`    |
| 2   | 模板选择自动填充后缀 + 预设内容写入            |  ✅  | `ScratchpadPanel.vue`    |
| 3   | TreeNode 文件节点 draggable + dragstart emit   |  ✅  | `ScratchpadTreeNode.vue` |
| 4   | Panel `handleTreeNodeDragStart` 设置 MIME 数据 |  ✅  | `ScratchpadPanel.vue`    |
| 5   | SqlEditorPanel drop handler 插入文件内容       |  ✅  | `SqlEditorPanel.vue`     |
| 6   | 删除撤销栏（5秒自动消失 + 撤销按钮）           |  ✅  | `ScratchpadPanel.vue`    |
| 7   | Bug Fix: zh-CN.json JSON 语法修复              |  ✅  | `zh-CN.json`             |
| 8   | i18n（templateType/undo 2 个新 key）           |  ✅  | `zh-CN.json`             |

### v3.6 交互细节

**新建文件模板：**

- 新建对话框新增模板选择行（标签"模板" + SQL/JSON/MD/PY 4个按钮）
- 选择模板后自动设置文件后缀（如选 SQL → `untitled.sql`）
- 确认创建后：`createEntry` → `saveFile` 写入预设模板内容
- 模板内容：SQL=`-- 新建查询\n\n`、JSON=`{\n  \n}\n`、MD=`# \n\n`、PY=`# -*- coding: utf-8 -*-\n\n`

**拖放文件到编辑器：**

- 树节点 `draggable="true"`（仅 file 类型，renaming 时禁用）
- 拖拽开始时设置 `dataTransfer`：`application/x-scratchpad-file`（relative_path）+ `text/plain` + `effectAllowed = 'copy'`
- 编辑器接受 drop 时读取 MIME 数据 → `invoke('read_scratchpad_file')` → `insertText(content)`
- 仅限 Tauri 环境内使用，不依赖外部剪贴板

**删除撤销：**

- `undoState` reactive：visible/name/relativePath/timer
- 单文件删除后调用 `showUndo(name, relativePath)` 显示撤销栏
- 5秒自动消失（`UNDO_TIMEOUT = 5000`）
- 点击"撤销"按钮 → `dismissUndo` → `restoreTrashEntry(name)` → toast 确认
- 面板已有 `position: relative`，undo-bar 使用 `position: absolute; bottom: 0`
- CSS 动画：`undo-bar-in` 0.2s ease-out 从底部滑入

### 变更文件

| 文件                     | 改动                                                                                                                                                                                                                     | 行数 |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | :--: |
| `ScratchpadPanel.vue`    | 模板选择器 modal + selectedTemplate + TEMPLATE_CONTENTS + selectTemplate + confirmCreate 重写 + undoState/showUndo/dismissUndo/handleUndoDelete + undo-bar 模板 + dragEntry + handleTreeNodeDragStart；删除改用 showUndo | +110 |
| `ScratchpadTreeNode.vue` | draggable + dragstart emit/forward + forwardDragStart                                                                                                                                                                    | +12  |
| `SqlEditorPanel.vue`     | editor-container @dragover/@drop + handleEditorDragOver/handleEditorDrop                                                                                                                                                 | +21  |
| `zh-CN.json`             | templateType/undo + 逗号修复                                                                                                                                                                                             |  +2  |

### v3.7 已完成 ✅

### 搜索安全加固 — 大文件跳过/结果截断（阶段二十七）

| #   | 任务                                                                                        | 状态 | 涉及文件                 |
| --- | ------------------------------------------------------------------------------------------- | :--: | ------------------------ |
| 1   | 新增 `SearchResult` 结构体（matches/total_scanned/total_skipped/skipped_files/truncated）   |  ✅  | `models.rs`              |
| 2   | `search_file_content` 重写：文件大小检查（10MB）+ 结果截断（500条）+ 统计数据               |  ✅  | `store.rs`               |
| 3   | `MAX_SEARCH_FILE_SIZE` / `MAX_SEARCH_RESULTS` 常量                                          |  ✅  | `store.rs`               |
| 4   | `search_scratchpad_content` 返回类型更新                                                    |  ✅  | `scratchpad_commands.rs` |
| 5   | `SearchResult` 导出                                                                         |  ✅  | `mod.rs`                 |
| 6   | 前端 `SearchResult` interface 定义                                                          |  ✅  | `types/index.ts`         |
| 7   | API `searchFileContent` 返回类型更新                                                        |  ✅  | `scratchpad-api.ts`      |
| 8   | Composable `searchContent` → `SearchResult \| null`                                         |  ✅  | `use-scratchpad.ts`      |
| 9   | Panel：searchResult ref + contentResults/contentAllMatches computed + searchNotice computed |  ✅  | `ScratchpadPanel.vue`    |
| 10  | Panel watcher 改写（searchResult 替代 contentResults/contentAllMatches）                    |  ✅  | `ScratchpadPanel.vue`    |
| 11  | Notice bar 模板（黄色警告条：截断/跳过信息）                                                |  ✅  | `ScratchpadPanel.vue`    |
| 12  | No-results 展示区域（搜索模式 + 空结果时显示）                                              |  ✅  | `ScratchpadPanel.vue`    |
| 13  | Info 图标导入 + CSS                                                                         |  ✅  | `ScratchpadPanel.vue`    |
| 14  | i18n 3 新 key + EN 同步                                                                     |  ✅  | `zh-CN.json` / `en.json` |
| 15  | Unused import 清理                                                                          |  ✅  | `scratchpad_commands.rs` |

### v3.7 技术细节

**防护常量（store.rs）：**

```rust
const MAX_SEARCH_FILE_SIZE: u64 = 10 * 1024 * 1024;  // 10MB
const MAX_SEARCH_RESULTS: usize = 500;                  // 最多 500 条结果
```

**搜索流程变化：**

```
search_file_content(query, case_sensitive) → SearchResult (不再是 Vec<SearchMatch>)
  for each file entry:
    1. 跳过文件夹
    2. 检查文件大小 > MAX_SEARCH_FILE_SIZE → 跳过 + 记录 skipped_files
    3. 读文件到内存
    4. 逐行匹配
    5. 达到 MAX_SEARCH_RESULTS → truncate + break
  return SearchResult { matches, total_scanned, total_skipped, skipped_files, truncated }
```

**前端 UI 通知：**

- 结果有内容 + 截断/跳过 → 黄色 notice bar 显示 "结果已截断（最多 500 条）；已跳过 2 个大文件（>=10MB）"
- 搜索模式 + 无结果 → 灰色 "未找到匹配内容"
- 搜索结果区域下展示保留原有的文件分组 + 行高亮

### 变更文件

| 文件                     | 改动                              | 行数 |
| ------------------------ | --------------------------------- | :--: |
| `models.rs`              | 新增 `SearchResult` struct        |  +9  |
| `store.rs`               | `search_file_content` 重写 + 常量 | +60  |
| `scratchpad_commands.rs` | 返回类型 + 清理 unused import     |  +1  |
| `mod.rs`                 | 导出 SearchResult                 |  +1  |
| `types/index.ts`         | SearchResult interface            |  +7  |
| `scratchpad-api.ts`      | 返回类型                          |  +2  |
| `use-scratchpad.ts`      | 返回类型 + 清理 unused import     |  +2  |
| `ScratchpadPanel.vue`    | ref/computed/watcher/template/CSS | +50  |
| `zh-CN.json`             | 3 key                             |  +3  |
| `en.json`                | 3 key                             |  +3  |

### v3.8 已完成 ✅

### 流式搜索 — BufReader 逐行读取 (阶段二十八)

v3.7 的 10MB 硬限制被替换为流式方案，大文件不再跳过。

| #   | 任务                                                       | 状态 | 涉及文件   |
| --- | ---------------------------------------------------------- | :--: | ---------- |
| 1   | `read_to_string` → `BufReader::lines()` 流式逐行读取       |  ✅  | `store.rs` |
| 2   | 提取 `search_single_file` 独立 async 函数                  |  ✅  | `store.rs` |
| 3   | `tokio::time::timeout` 30s 超时保护                        |  ✅  | `store.rs` |
| 4   | 移除 `MAX_SEARCH_FILE_SIZE` 常量                           |  ✅  | `store.rs` |
| 5   | 新增 imports（AsyncBufReadExt/BufReader/timeout/Duration） |  ✅  | `store.rs` |

**技术变化：**

```
v3.7:  read_to_string(file) → 100MB 全部进内存 → lines()
v3.8:  BufReader(file).lines() → 一次只读一行 → 恒定 ~8KB
       + timeout(30s) 包裹 → 超时则跳过该文件
```

**收益：**
| 维度 | v3.7 (门槛) | v3.8 (流式) |
|------|:--:|:--:|
| 1GB 文件 | 跳过，不搜 | 搜索，~8KB 内存 |
| 单文件超时 | 无 | 30s 自动终止 |
| UX | "已跳过 N 个大文件" | 所有文件都能搜 |

### 变更文件

| 文件       | 改动                                                               | 行数 |
| ---------- | ------------------------------------------------------------------ | :--: |
| `store.rs` | imports + `search_single_file` + `search_file_content` 重写 + 常量 | +55  |

---

### v3.9 已完成 ✅

### 质量加固 — unwrap 全清除 + 类型对齐 + i18n 补全 (阶段二十九)

综合审计修复，消除所有代码违规和类型/hygiene 差距。

| #   | 任务                                                                                      | 状态 | 涉及文件              |
| --- | ----------------------------------------------------------------------------------------- | :--: | --------------------- |
| 1   | `store.rs` 12 处 `unwrap_or_*` → `ok_or_else` / `match`                                   |  ✅  | `store.rs`            |
| 2   | `commands.rs` 2 处 `unwrap_or_*` → `match` / `unwrap_or_else`                             |  ✅  | `commands.rs`         |
| 3   | `state.rs` `futures::executor::block_on` → `tokio::runtime::Handle::current().block_on()` |  ✅  | `state.rs`            |
| 4   | `state.rs` 新增 `Drop` impl 确保 watcher 线程清理                                         |  ✅  | `state.rs`            |
| 5   | `types/index.ts` 移除 `children: ScratchpadEntry[] \| null`                               |  ✅  | `types/index.ts`      |
| 6   | `panel.vue` `handlePromoteConfirm` 修复路径（绝对→相对）                                  |  ✅  | `ScratchpadPanel.vue` |
| 7   | `panel.vue` `escapeHtml` 补单引号转义 `&#39;`                                             |  ✅  | `ScratchpadPanel.vue` |
| 8   | `panel.vue` `flattenEntries` / `collectFolderPaths` / Ctrl+A 简化                         |  ✅  | `ScratchpadPanel.vue` |
| 9   | `en.json` 补齐 40+ 缺失 scratchpad locale 键                                              |  ✅  | `en.json`             |

**技术变化：**

```
v3.8:  14 个 unwrap_or_* + futures::block_on + children 类型错配 + i18n 缺失
v3.9:  0 个 unwrap_*（全部 ? 错误传播）+ Handle::block_on + 类型对齐 + 完整 locale
```

**变更文件：**
| 文件 | 改动 | 行数 |
|------|------|:--:|
| `store.rs` | 12 处 unwrap*or*_ 修复 | ~30 |
| `commands.rs` | 2 处 unwrap*or*_ 修复 | +6 |
| `state.rs` | Handle::current + Drop | +8 |
| `types/index.ts` | -1 行 children | -1 |
| `ScratchpadPanel.vue` | promote 路径 + escapeHtml + 简化3函数 | ~15 |
| `en.json` | 40+ 新键 | +47 |

---

### v3.10 已完成 ✅

### 一致性治理 — 全局审计 16 项修复 (阶段三十)

跨后端/前端/文档/接口全栈审计，消除所有不一致。

| #   | 任务                                                                       | 严重度 | 状态 | 涉及文件                                                               |
| --- | -------------------------------------------------------------------------- | :----: | :--: | ---------------------------------------------------------------------- |
| 1   | `types/index.ts` 彻底移除 `children?: ScratchpadEntry[]`                   |   🔴   |  ✅  | `types/index.ts`                                                       |
| 2   | `scratchpad-api.ts` `restoreFromTrash` 返回 void → ScratchpadEntry         |   🔴   |  ✅  | `scratchpad-api.ts`                                                    |
| 3   | `panel.vue` `t('navigator.retry')` → `t('scratchpad.retry')` + locale 补齐 |   🔴   |  ✅  | `panel.vue` `en.json` `zh-CN.json`                                     |
| 4   | `en.json` 补 `minutesAgo`/`hoursAgo`/`daysAgo`/`weeksAgo`（{n} 占位）      |   🟡   |  ✅  | `en.json`                                                              |
| 5   | `zh-CN.json` 移除重复 `"undo"` 键                                          |   🟡   |  ✅  | `zh-CN.json`                                                           |
| 6   | `extension.ts` `init` 通过 `initScratchpadStore()` API 封装而非裸 `invoke` |   🔵   |  ✅  | `extension.ts` `scratchpad-api.ts`                                     |
| 7   | `composable` `startWatching` 增加 `notInitialized` 守卫                    |   🔵   |  ✅  | `use-scratchpad.ts`                                                    |
| 8   | `PROGRESS.md` 修正 23→21 命令、11→21 API 函数                              |   🟠   |  ✅  | `SCRATCHPAD_PROGRESS.md`                                               |
| 9   | `DESIGN.md` / `PROGRESS.md` / `SCHEMA.md` 三文档版本对齐 v3.10             |   🟠   |  ✅  | `SCRATCHPAD_DESIGN.md` `SCRATCHPAD_PROGRESS.md` `SCRATCHPAD_SCHEMA.md` |

**技术变化：**

```
v3.9:  幽灵 children 在 TS、restoreFromTrash 返回值错配、locale 缺失 retry + 时间格式化、文档数字错误
v3.10: 全栈类型一致、API 返回值正确、locale 完整、文档精确、init 走封装层、watcher 守卫未初始化
```

**变更文件：**
| 文件 | 改动 | 行数 |
|------|------|:--:|
| `types/index.ts` | -1 行 children | -1 |
| `scratchpad-api.ts` | void → ScratchpadEntry + initStore | +7 |
| `ScratchpadPanel.vue` | retry key 修复 | +1/-1 |
| `en.json` | +5 时间 +1 retry | +6 |
| `zh-CN.json` | +1 retry -1 重复 undo | 0 |
| `extension.ts` | 裸 invoke → API 封装 | +1/-1 |
| `use-scratchpad.ts` | startWatching 守卫 | +3 |
| `SCRATCHPAD_DESIGN.md` | v3.9→v3.10 | +1/-1 |
| `SCRATCHPAD_PROGRESS.md` | 数字修正 + 阶段30 | +55 |
| `SCRATCHPAD_SCHEMA.md` | v2.10→v3.10 版本对齐 | +1/-1 |

### v3.11 已完成 ✅

### i18n 收尾 + 生命周期合并 + 图标工具栏（阶段三十一）

| #   | 任务                                            | 状态 | 涉及文件                                     |
| --- | ----------------------------------------------- | :--: | -------------------------------------------- |
| 1   | promote modal 按钮硬编码中文 → i18n             |  ✅  | `ScratchpadPanel.vue` `zh-CN.json` `en.json` |
| 2   | 5 组生命周期钩子合并为 1 组                     |  ✅  | `ScratchpadPanel.vue`                        |
| 3   | 删除未使用 CSS + 未使用变量                     |  ✅  | `ScratchpadPanel.vue`                        |
| 4   | 工具栏全图标化（NTooltip 提示 + active 态高亮） |  ✅  | `ScratchpadPanel.vue`                        |
| 5   | 文档三件套版本对齐 v3.11                        |  ✅  | `DESIGN.md` `PROGRESS.md` `SCHEMA.md`        |

**变更文件：**
| 文件 | 改动 | 行数 |
|------|------|:--:|
| `ScratchpadPanel.vue` | promote i18n + 生命周期合并 + 图标工具栏 | +60/-40 |
| `zh-CN.json` | +2 promote 键 +1 refresh | +3 |
| `en.json` | +2 promote 键 +1 refresh | +3 |
| `SCRATCHPAD_DESIGN.md` / `SCHEMA.md` | v3.10→v3.11 | +2/-2 |

### v3.12 已完成 ✅

### 文件监控增强 — 事件详情 + 原子保存检测 + 状态保持（阶段三十二）

| #   | 任务                                                  | 状态 | 涉及文件                                   |
| --- | ----------------------------------------------------- | :--: | ------------------------------------------ |
| 1   | 文件变更事件携带 paths 负载（替代空 ()）              |  ✅  | `models.rs` `commands.rs` `types/index.ts` |
| 2   | 原子保存检测（delete+create ≈ modify）                |  ✅  | `commands.rs`                              |
| 3   | 智能防抖（300ms + 路径去重 + 超时自动 flush）         |  ✅  | `commands.rs`                              |
| 4   | watcher 断开自动停标（防止僵尸状态）                  |  ✅  | `commands.rs`                              |
| 5   | 前端状态保持（重载时保留 expandedKeys + selectedKey） |  ✅  | `ScratchpadPanel.vue`                      |

**技术细节：**

```
v3.12 watcher 增强：
  notify 回调 → 提取相对路径 + 事件类型 (create/modify/delete)
  → mpsc channel → spawn_blocking 线程
  → 原子保存检测：HashMap<path, Instant> 记录 delete
    → 若 500ms 内同路径 create → 视为 modify（跳过 delete 事件）
  → 路径去重：Vec<pending_paths> 去重
  → 300ms debounce → emit ScratchpadChangeEvent { paths }
  → 超时自动 flush：200ms recv_timeout 触发清空
  → Disconnected → 自动停标 watcher_active

前端状态保持：
  listen('scratchpad-changed') → 保存 expandedKeys + selectedKey
  → loadFiles() → 恢复 expandedKeys + selectedKey
  → 树不闪烁、不折叠、不跳滚
```

**变更文件：**
| 文件 | 改动 | 行数 |
|------|------|:--:|
| `models.rs` | +ScratchpadChangeEvent | +5 |
| `mod.rs` | +ScratchpadChangeEvent re-export | +1 |
| `commands.rs` | 重写 watch_scratchpad（+原子保存/+去重/+flush） | +50/-35 |
| `types/index.ts` | +ScratchpadChangeEvent type | +4 |
| `ScratchpadPanel.vue` | 事件监听器更新 + 状态保持 | +6/-2 |
| `SCRATCHPAD_DESIGN.md` / `SCHEMA.md` | v3.11→v3.12 | +2/-2 |

### v3.13 已完成 ✅

### VSCode 差距消除 — 树结构/内联创建/搜索上下文/子目录创建 (阶段四十)

消除此前审计发现的 5 项与 VSCode 文件管理的核心差距：

| #   | 改进项                  | 问题                                                   | 解决方案                                                                                                                   | 状态 |
| --- | ----------------------- | ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------- | :--: |
| 1   | **目录树结构**          | 后端返回扁平 Vec，前端 children 永远为空               | `ScanDir` → `scan_dir_tree` 递归构建嵌套树；`ScratchpadEntry.children: Option<Vec<ScratchpadEntry>>`                       |  ✅  |
| 2   | **子目录创建**          | `target_path = scratchpad_dir.join(name)` 硬编码根目录 | `create_entry()` 新增 `parent_path` 参数；前端传递选中文件夹上下文                                                         |  ✅  |
| 3   | **搜索上下文行**        | 仅返回匹配行，无前后文                                 | `search_single_file()` 采集 `before_context`/`after_context`；`SearchMatch` 新增两个 `Vec<String>` 字段                    |  ✅  |
| 4   | **内联创建 (去模态框)** | 创建文件/文件夹走 NModal 弹窗                          | TreeNode 新增 `inlineCreateParentPath`/`inlineCreateIsFolder` props；文件夹节点下渲染 `<input>` 行；Enter 提交/Escape 取消 |  ✅  |
| 5   | **增量树更新**          | onChange 事件仅含 paths，全量 reload                   | `ScratchpadChangeEvent` → `changes: [{ path, kind }]`；前端 `applyFileChanges()` 按 create/delete/modify 分别处理树结构    |  ✅  |

#### 后端改动

| 文件          | 改动                                                                                                                                                                                                                                                                                             | 说明                           |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------ |
| `models.rs`   | `ScratchpadEntry.children` 新增；`SearchMatch.before_context`/`after_context` 新增；`ScratchpadChangeEntry` + `ScratchpadChangeEvent.changes` 重定义                                                                                                                                             | 树结构 + 搜索上下文 + 事件细化 |
| `store.rs`    | `scan_dir` → `scan_dir_tree`（嵌套树）；`flatten_entries_from_ref` 新增；`create_entry(name, parent_path, is_folder)` 签名变更；`search_file_content(query, case_sensitive, context_lines)` 签名变更；`search_single_file` 增加上下文采集逻辑；所有 `ScratchpadEntry` 构造器增加 `children` 字段 | 核心改造                       |
| `commands.rs` | `create_scratchpad_entry` + `parent_path` 参数；`search_scratchpad_content` + `context_lines` 参数；`watch_scratchpad` 事件格式更新为 `ScratchpadChangeEntry`；新增 `get_scratchpad_entry` 命令；`case_sensitive` → `Option<bool>` 向后兼容                                                      | 命令层适配                     |
| `lib.rs`      | 注册 `get_scratchpad_entry`                                                                                                                                                                                                                                                                      | 命令注册                       |

#### 前端改动

| 文件                     | 改动                                                                                                                                                                                                                                                                                                                                                                           | 说明                |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------- |
| `types/index.ts`         | `SearchMatch` + `before_context`/`after_context`；`ScratchpadChangeEntry` + `ScratchpadChangeEvent.changes` 重定义                                                                                                                                                                                                                                                             | 类型对齐            |
| `scratchpad-api.ts`      | `createScratchpadEntry(name, isFolder, parentPath?)` 签名变更；`searchFileContent(query, caseSensitive, contextLines = 2)` 签名变更；新增 `getScratchpadEntry()`                                                                                                                                                                                                               | API 层适配          |
| `use-scratchpad.ts`      | `createEntry(name, isFolder, parentPath?)` 签名变更；`filterTree`/`flattenVisibleEntries` 新增树操作；`applyFileChanges` 重写（`removeDeletedFromTree`/`collectAllPaths`/`insertEntriesIntoTree`/`patchEntriesInTree`）树级增量更新                                                                                                                                            | Composable 核心改造 |
| `ScratchpadPanel.vue`    | 移除 `NModal` create/file 模态框；`inlineCreateParentPath`/`inlineCreateIsFolder` refs；`startInlineCreate`/`cancelInlineCreate`/`confirmInlineCreate`/`findSelectedFolder`/`findEntryInTree` 新增；`flattenEntries`/`collectFolderPaths` 树版本重写；搜索结果显示 before/after context 行；TreeNode props 新增 `inlineCreateParentPath`/`inlineCreateIsFolder`/@create-inline | UI 核心改造         |
| `ScratchpadTreeNode.vue` | Props 新增 `inlineCreateParentPath`/`inlineCreateIsFolder`；emit 新增 `create-inline`；模板新增 `inline-create-row` (带 input)；`isInlineCreateTarget`/`commitInlineCreate`/`cancelInlineCreate`/`forwardCreateInline` 新增；watch `isInlineCreateTarget` → auto focus；CSS `.inline-create-row`                                                                               | 树节点改造          |

#### 树结构数据流

```
scan_dir_tree(dir)
  → 读取目录条目
  → 文件夹: children = scan_dir_tree(subdir) 递归
  → 文件: children = None
  → 返回 Vec<ScratchpadEntry> 嵌套树

前端渲染:
  ScratchpadPanel
  → v-for filteredLocalEntries (顶层)
  → ScratchpadTreeNode
    → childEntries = computed(() => entry.children || [])  ← 现在有真实数据
    → v-for childEntries 递归渲染
    → depth + 1 缩进生效
```

#### 增量更新树级 patch

```
notify watcher → ScratchpadChangeEvent { changes: [{ path, kind }] }
  → applyFileChanges(event)
    → delete: removeDeletedFromTree() 递归过滤
    → create: getScratchpadEntry() 批量 fetch → insertEntriesIntoTree() 插入父节点
    → modify: getScratchpadEntry() 批量 fetch → patchEntriesInTree() 替换
    → 保持 expandedKeys + selectedKey
```

#### 验证结果

| 检查项        |                       结果                        |
| ------------- | :-----------------------------------------------: |
| `cargo check` |                     ✅ 0 错误                     |
| `pnpm lint`   | ✅ 0 新增错误（2 预存错误均为 WorkbenchView.vue） |
| 向后兼容      |    ✅ 所有 API `Option<T>` 化，旧调用不受影响     |

### v3.14 已完成 ✅

### P0 三件套 — 虚拟滚动 + 脏状态 + 懒加载 (阶段四十一)

此前审计发现的 3 项最高优先级性能与体验差距：

| #   | 改进项         | 问题                                                            | 解决方案                                                                                                                                                                                                                                                                                                                                | 状态 |
| --- | -------------- | --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :--: |
| 1   | **虚拟滚动**   | 超大文件列表（>200条）全量渲染 DOM 节点导致性能下降             | 仅渲染可视区 + overscan 8 行；`ROW_HEIGHT=28px`；`VIRTUAL_SCROLL_THRESHOLD=50` 阈值自动切换；`ResizeObserver` 动态容器高度；`flattenedTree` → `visibleTreeEntries` 按 scrollTop 切片                                                                                                                                                    |  ✅  |
| 2   | **脏状态跟踪** | 编辑器修改后无法感知未保存文件，外部修改无冲突提示              | `use-scratchpad.ts` 新增 `dirtyFiles: Set<string>` + `markDirty()`/`markClean()`/`isDirty()`；`hasUnsavedChanges` computed；`ScratchpadTreeNode.vue` 在文件名前显示 `●` 脏状态点；文件监控 `modify` 事件检测到 `dirtyFiles` 中的文件时追加到 `externalConflicts` 列表；`NModal` 冲突对话框提供"重新加载"/"忽略"选项                     |  ✅  |
| 3   | **懒加载**     | 初始加载全量递归扫描整个目录树（最多 4 层），深层嵌套项目启动慢 | 后端 `get_full_response()` 改为 `depth=0` 仅加载顶层条目；新增 `list_directory_entries(parent_path)` 方法 + `list_scratchpad_directory` Tauri 命令；前端 `handleToggleExpand` 检测 `children === null` 时异步 `loadChildEntries(path)` 按需加载；`handleExpandAll` 批量加载所有已展开文件夹的子级；`hasChildrenLoaded()` 判断是否已加载 |  ✅  |

#### 后端改动

| 文件          | 改动                                                                             | 说明         |
| ------------- | -------------------------------------------------------------------------------- | ------------ |
| `store.rs`    | `get_full_response` `depth=0` 仅顶层；新增 `list_directory_entries(parent_path)` | 懒加载核心   |
| `commands.rs` | 新增 `list_scratchpad_directory` 命令                                            | 按需加载 API |
| `lib.rs`      | 注册 `list_scratchpad_directory`                                                 | 命令注册     |

#### 前端改动

| 文件                     | 改动                                                                                                                                                                                                                                                                                                                                                                           | 说明         |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------ |
| `scratchpad-api.ts`      | 新增 `listScratchpadDirectory()`                                                                                                                                                                                                                                                                                                                                               | API 封装     |
| `use-scratchpad.ts`      | 新增 `dirtyFiles`/`markDirty()`/`markClean()`/`isDirty()`/`hasUnsavedChanges`/`externalConflicts`/`dismissConflict()` 脏状态管理；新增 `loadChildEntries()`/`setEntryChildren()`/`hasChildrenLoaded()` 懒加载；`applyFileChanges` 中新增外部冲突检测                                                                                                                           | 核心状态管理 |
| `ScratchpadPanel.vue`    | 虚拟滚动：`ROW_HEIGHT`/`OVERSCAN`/`VIRTUAL_SCROLL_THRESHOLD` 常量 + `visibleTreeEntries`/`virtualScrollPaddingTop`/`virtualScrollTotalHeight` computed + `handleTreeScroll`/`updateTreeContainerSize` + `ResizeObserver`；冲突对话框 NModal + `handleConflictReload`/`handleConflictIgnore`；`handleToggleExpand` 异步懒加载；`handleExpandAll` 批量加载；`showTrash` ref 修复 | UI 核心改造  |
| `ScratchpadTreeNode.vue` | 新增 `dirtyFiles?: Set<string>` prop；`isNodeDirty` computed 显示 `●` 脏状态点；CSS `.dirty-dot` 样式                                                                                                                                                                                                                                                                          | 脏状态指示   |

#### 虚拟滚动数据流

```
flattenedTree (全量平铺树)
  ↓
useVirtualScrollEnabled (flattenedTree.length > VIRTUAL_SCROLL_THRESHOLD)
  ↓ true → visibleTreeEntries = flattenedTree.slice(from, to)
  ↓ false → visibleTreeEntries = flattenedTree (全量渲染)
  ↓
virtual-scroll-viewport (height = totalHeight)
  └─ virtual-scroll-spacer (paddingTop)
       └─ ScratchpadTreeNode v-for visibleTreeEntries
```

#### 脏状态数据流

```
编辑器修改内容 → markDirty(relativePath)
  → dirtyFiles.add(normalizedPath)
  → TreeNode 检测 isNodeDirty → 显示 ● 脏点
Ctrl+S 保存 → markClean(relativePath)
  → dirtyFiles.delete(normalizedPath)
  → ● 消失

文件监控 modify 事件:
  → applyFileChanges 检查 modify paths
  → 若 path ∈ dirtyFiles → externalConflicts.push(path)
  → watch(externalConflicts) → NModal 冲突对话框
  → 用户选择: 重新加载(丢失未保存内容) / 忽略(保留编辑内容)
```

#### 懒加载数据流

```
初始加载: list_scratchpad_files → depth=0 仅顶层条目（children=null）
用户展开文件夹:
  → handleToggleExpand(entry)
  → hasChildrenLoaded(entry)?
    → false: loadChildEntries(entry.path)
      → invoke('list_scratchpad_directory', { parentPath })
      → setEntryChildren(response.local_entries, parentPath, children)
      → expandedKeys.add(entry.path)
    → true: expandedKeys.toggle(entry.path)
  → flattenedTree 更新 → 子条目渲染
```

#### 验证结果

| 检查项        |                           结果                           |
| ------------- | :------------------------------------------------------: |
| `cargo check` |                        ✅ 0 错误                         |
| `pnpm lint`   | ✅ scratchpad 0 错误（2 预存错误均为 WorkbenchView.vue） |
| 向后兼容      |            ✅ 懒加载对前端透明，展开行为不变             |

---

### v3.15 已完成 ✅

### P0 操作闭环三件套 — 剪切移动 + 正则替换 + Diff 对比 (阶段四十二)

此前审计发现的 3 项最高优先级操作闭环差距：

| #   | 改进项        | 问题                                          | 解决方案                                                                                                                                                             | 状态 |
| --- | ------------- | --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :--: |
| 1   | **剪切/移动** | 只能复制粘贴，无法移动文件整理目录结构        | `move_entry(from, to_parent)` 后端 + `move_scratchpad_entry` 命令 + 右键"剪切"设置 `clipboardMode='cut'` + 粘贴时走 move 路径 + 5秒撤销栏 + 同目录/重复检测          |  ✅  |
| 2   | **正则+替换** | 搜索不支持正则，搜索后无法替换                | `.*` 按钮切换正则模式 + `Regex` 前端验证 + `replace_in_file(pattern, replacement, is_regex)` 后端 + 替换栏预览 + 全部替换 + 替换历史追踪 + 外层 `regex` crate        |  ✅  |
| 3   | **Diff 对比** | 外部冲突只能二选一（重载/忽略），无法查看差异 | `diff_with_content(path, content, left, right)` 后端调用 `similar` crate + `DiffResult` 行级对比 + `NModal` 800px 弹窗 + 绿色✅新增/红色❌删除 + 行号 + 接受右侧按钮 |  ✅  |

#### 后端改动

| 文件          | 新增                                                                                                                                                                                                         | 说明                    |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------- |
| `Cargo.toml`  | `regex = "1.11"`, `similar = "2.6"`                                                                                                                                                                          | 正则引擎 + 文本差异算法 |
| `models.rs`   | `ReplaceResult`, `DiffLineKind`, `DiffLine`, `DiffResult`                                                                                                                                                    | 4 个新数据模型          |
| `mod.rs`      | 导出 4 个新模型                                                                                                                                                                                              | 模块重导出              |
| `store.rs`    | `move_entry(from, to_parent)` → `ScratchpadEntry`<br>`replace_in_file(path, pattern, replacement, is_regex)` → `ReplaceResult`<br>`diff_with_content(path, content, left_label, right_label)` → `DiffResult` | 3 个新方法              |
| `commands.rs` | `move_scratchpad_entry`<br>`replace_scratchpad_content`<br>`diff_scratchpad_with_content`                                                                                                                    | 3 个新 Tauri 命令       |
| `lib.rs`      | 注册 3 个新命令                                                                                                                                                                                              | 命令注册                |

#### 前端改动

| 文件                  | 新增                                                                                                                                     | 说明                 |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| `types/index.ts`      | `ReplaceResult`, `DiffLineKind`, `DiffLine`, `DiffResult`                                                                                | 4 个新 TS 类型       |
| `scratchpad-api.ts`   | `moveScratchpadEntry`, `replaceScratchpadContent`, `diffScratchpadWithContent`                                                           | 3 个新 API           |
| `use-scratchpad.ts`   | `clipboardMode`, `moveEntry`, `replaceInFile`, `diffWithContent`, `replaceHistory`, `clearReplaceHistory`, `validateRegex` + 10 个新导出 | 状态管理             |
| `ScratchpadPanel.vue` | 正则 .\* 按钮, 替换栏(NInput+预览+NButton), Diff NModal 800px 弹窗, move-undo-bar, 冲突"查看差异"按钮                                    | ~310 行新增，完整 UI |
| `zh-CN.json`          | 17 个新翻译键                                                                                                                            | 中文 i18n            |
| `en.json`             | 17 个新翻译键                                                                                                                            | 英文 i18n            |

#### 剪切/移动数据流

```
右键选择文件 → "剪切" → clipboardMode='cut', clipboardEntry=entry
↓
选中目标文件夹 → 右键 → "粘贴" → handlePaste()
  ├─ clipboardMode === 'cut' → moveEntry(fromPath, toParent)
  │   → backend: fs::rename + file_meta 迁移 + config 更新
  │   → 成功: showMoveUndo(toParent, name) + toast
  │   → 失败: toast error (同目录/不存在/重名)
  └─ clipboardMode === 'copy' → 原有 copy+create 逻辑

移动撤销: moveUndoBar (5秒) → handleMoveUndo → moveEntry(name, getParentPath(fromPath))
```

#### 正则替换数据流

```
全文搜索 (contentSearchMode) → 搜索结果展示
  ↓
.* 按钮 → isRegex=true → validateRegex(query) → 实时语法验证 (regexError)
  ↓
"替换" 按钮 → showReplaceBar 展开
  ↓
输入替换文本 → computeReplacePreview → 遍历搜索结果统计匹配数
  ↓
"全部替换" → handleReplaceAll → 遍历每个文件 → replaceInFile
  → backend: Regex::new + replace_all / String::replace
  → 原子写回文件 (save_file)
  → 刷新搜索结果 + toast 统计 (X 个文件 Y 处)
  → addReplaceHistory 记录历史
```

#### Diff 对比数据流

```
外部文件修改 → watch(modify event) → externalConflicts.push(path)
  → NModal 冲突对话框:
    [重新加载] [忽略] [查看差异]  ← 新增按钮
         ↓
    handleConflictDiff → loadFileContent(磁盘内容)
    → diffWithContent(磁盘内容, 编辑器内容)
      → backend: similar::TextDiff::from_lines
      → 遍历 changes → DiffLine { kind, line_number, content }
    → NModal 800px 弹窗:
      ┌─ 左侧: 磁盘文件 ─┬─ 右侧: 编辑器内容 ─┐
      ├─  44  hello world  │  44  hello world   │ (unchanged, 白底)
      ├─     - old line    │  98  new line       │ (removed/added, 红/绿底)
      └───────────────────┴────────────────────┘
    [关闭(返回冲突)] [接受右侧(编辑器内容)]
```

#### 新增依赖

| 依赖      | 版本 | 用途                                                |
| --------- | ---- | --------------------------------------------------- |
| `regex`   | 1.11 | Rust 正则表达式引擎（`Regex::new` + `replace_all`） |
| `similar` | 2.6  | 文本差异算法（`TextDiff::from_lines` 行级 diff）    |

#### 验证结果

| 检查项        |              结果               |
| ------------- | :-----------------------------: |
| `cargo check` |            ✅ 0 错误            |
| `pnpm lint`   | ✅ 0 错误（已修复，无预存错误） |

---

## MVP 范围与验收标准

### MVP 定义

草稿箱 MVP (v3.15) 包含以下完整操作闭环：

| 模块          | 闭环                                             | 状态 |
| ------------- | ------------------------------------------------ | :--: |
| **文件 CRUD** | 创建 → 编辑 → 保存 → 删除 → 回收站恢复 → 清空    |  ✅  |
| **文件组织**  | 剪切 → 粘贴移动 → 撤销 → 复制粘贴                |  ✅  |
| **搜索**      | 名称过滤 → 全文搜索 → 大小写 → 正则 → 结果跳转   |  ✅  |
| **替换**      | 搜索 → 替换预览 → 全部替换 → 搜索刷新 → 历史追踪 |  ✅  |
| **冲突**      | 外部检测 → 冲突提示 → 查看差异 → 接受/忽略       |  ✅  |
| **导入**      | 拖放/浏览 → 内容导入 → 外部引用                  |  ✅  |
| **导航**      | 树展开/折叠 → 键盘导航 → 内联创建 → 最近文件     |  ✅  |
| **性能**      | 虚拟滚动 → 懒加载 → 流式搜索 → 文件监控          |  ✅  |

### 验收标准

1. **剪切移动**：右键"剪切"→ 粘贴到目标文件夹 → 文件物理移动 → 5秒撤销栏可见
2. **正则替换**：全文搜索 → 切换 .\* 模式 → 输入正则 → 实时语法验证 → 展开替换 → 预览计数 → 全部替换 → 搜索结果刷新
3. **Diff 对比**：外部修改文件 → 冲突对话框 → "查看差异"按钮 → 800px 弹窗 → 红/绿行标记 → "接受右侧"保存
