# 项目切换与管理面板 — 设计文档

> 版本：v1.2
> 创建日期：2026-05-10
> 最后更新：2026-05-11
> 状态：✅ 已完成

---

## 1. 概述

### 1.1 功能定位

一个从标题栏项目名称触发的非模态下拉面板，负责项目的**快速切换**和**基础管理**（重命名、编辑信息、移除、删除）。

### 1.2 职责边界

| 负责              | 不负责（留给欢迎页） |
| ----------------- | -------------------- |
| 最近项目快速切换  | 浏览所有项目         |
| 重命名项目        | 搜索项目             |
| 编辑项目名称/描述 | 项目分组             |
| 从最近列表移除    | 项目排序             |
| 物理删除项目      | 联邦项目管理         |

### 1.3 用户故事

- 作为用户，我点击标题栏项目名，可以看到最近使用的 10 个项目
- 作为用户，我可以一键切换到另一个项目
- 作为用户，我可以重命名项目（内联编辑）
- 作为用户，我可以编辑项目的名称和描述
- 作为用户，我可以在不删除文件的情况下从列表中移除项目
- 作为用户，我可以永久删除一个项目（需二次确认）

---

## 2. 架构设计

### 2.1 组件关系图

```
WorkbenchTitleBar.vue
├── ProjectSwitcherPanel.vue          ← 新增（替换 ProjectSelector.vue）
│   ├── 顶部操作栏（新建/打开）
│   ├── 最近项目列表（卡片式）
│   │   └── 每个卡片：[···] 触发右键菜单
│   └── 底部入口（管理所有项目...）
├── EditProjectModal.vue              ← 新增
│   └── 编辑项目名称/描述/路径(只读)
├── DeleteProjectConfirmModal.vue     ← 新增
│   └── 输入项目名确认删除
├── NewProjectModal.vue               ← 保留，由顶部 [+ 新建项目] 触发
└── 其他已有组件...
```

### 2.2 数据流

```
┌─────────────────────────────────────────────────────┐
│  Frontend (Vue 3 + Pinia + TypeScript)              │
│                                                     │
│  WorkbenchTitleBar.vue                              │
│    ↓ props: showPanel (ref<boolean>)                 │
│  ProjectSwitcherPanel.vue                           │
│    ↓ $t() i18n                                       │
│  useProjectStore (Pinia)                            │
│    ↓ invoke()                                        │
│  ProjectService (API Layer)                         │
│    ↓ Tauri IPC                                       │
├─────────────────────────────────────────────────────┤
│  Backend (Rust + Tauri)                             │
│                                                     │
│  project_commands.rs                                │
│    ├── get_recent_projects()    → 全局 SQLite       │
│    ├── open_project_by_id()     → 全局 SQLite       │
│    ├── rename_project()         → 全局 SQLite       │
│    ├── update_project()         → 全局 SQLite       │
│    ├── remove_from_recent()     → 全局 SQLite (NEW) │
│    └── delete_project_disk()    → 全局SQLite+磁盘(NEW)│
│                                                     │
│  全局 SQLite (global-settings.db)                    │
│    └── projects 表                                  │
└─────────────────────────────────────────────────────┘
```

### 2.3 状态管理

| 状态         | 管理方式                                    | 说明           |
| ------------ | ------------------------------------------- | -------------- |
| 面板开/关    | `ref<boolean>` 在 ProjectSwitcherPanel 内部 | 不写入全局状态 |
| 最近项目列表 | `useProjectStore.recentProjects`            | Pinia 全局     |
| 当前项目     | `useProjectStore.currentProject`            | Pinia 全局     |
| 编辑模态框   | `ref<boolean>` 在 ProjectSwitcherPanel 内部 | 按需显示       |
| 删除确认框   | `ref<boolean>` 在 ProjectSwitcherPanel 内部 | 按需显示       |

---

## 3. 组件规范

### 3.1 ProjectSwitcherPanel.vue

**路径**: `src/extensions/builtin/workbench/ui/components/ProjectSwitcherPanel.vue`

**Props**:

```typescript
interface Props {
  currentProject?: Project | null
  currentProjectId?: string | null
  recentProjects: Project[]
}
```

**Emits**:

```typescript
defineEmits<{
  'switch-project': [project: Project]
  'new-project': []
  'open-project': []
  close: []
}>()
```

**内部状态**:

| 变量                  | 类型                       | 说明                |
| --------------------- | -------------------------- | ------------------- |
| `showPanel`           | `ref<boolean>`             | 面板开/关           |
| `showEditModal`       | `ref<boolean>`             | 编辑模态框          |
| `showDeleteConfirm`   | `ref<boolean>`             | 删除确认框          |
| `contextMenuProject`  | `ref<Project \| null>`     | 右键菜单目标项目    |
| `contextMenuPosition` | `ref<{x:number,y:number}>` | 右键菜单位置        |
| `renamingProjectId`   | `ref<string \| null>`      | 正在重命名的项目 ID |

**面板尺寸**: 宽度 420px，最大高度 520px（超出内部滚动）

### 3.2 EditProjectModal.vue

**路径**: `src/extensions/builtin/workbench/ui/components/EditProjectModal.vue`

**Props**:

```typescript
interface Props {
  visible: boolean
  project: Project | null
}
```

**Emits**:

```typescript
defineEmits<{
  confirm: [id: string, name: string, description?: string]
  cancel: []
}>()
```

### 3.3 DeleteProjectConfirmModal.vue

**路径**: `src/extensions/builtin/workbench/ui/components/DeleteProjectConfirmModal.vue`

**Props**:

```typescript
interface Props {
  visible: boolean
  project: Project | null
}
```

**Emits**:

```typescript
defineEmits<{
  confirm: [projectId: string]
  cancel: []
}>()
```

---

## 4. 后端接口规范

### 4.1 已有命令（复用）

| 命令                  | 入参                        | 返回值                     | 说明          |
| --------------------- | --------------------------- | -------------------------- | ------------- |
| `get_recent_projects` | `limit?: usize`             | `Vec<ProjectInfoResponse>` | 获取最近项目  |
| `open_project_by_id`  | `id: String`                | `ProjectInfoResponse`      | 切换项目      |
| `rename_project`      | `input: RenameProjectInput` | `()`                       | 重命名        |
| `update_project`      | `input: UpdateProjectInput` | `()`                       | 更新名称+描述 |
| `delete_project`      | `project_id: String`        | `()`                       | 从数据库删除  |

### 4.2 新增命令

#### remove_from_recent

```rust
#[tauri::command]
pub async fn remove_from_recent(project_id: String) -> Result<ProjectInfoResponse, String>
```

- 功能：从全局数据库的 `projects` 表中移除记录
- 不删除物理文件和目录
- 使缓存失效
- **v1.2 变更**: 返回被移除的项目信息（`ProjectInfoResponse`），供前端做 UI 回滚
- 先查询项目信息 → 删除 DB 记录 → 返回项目信息

#### delete_project_disk

```rust
#[tauri::command]
pub async fn delete_project_disk(project_id: String) -> Result<(), String>
```

- 功能：从全局数据库删除 + 物理删除项目目录
- 需验证项目存在和路径安全
- 使缓存失效

### 4.3 数据模型

**ProjectInfoResponse**（已有）:

```rust
pub struct ProjectInfoResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub path: ProjectPathResponse,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_opened_at: Option<String>,
    pub version: String,
}
```

**前端 Project 类型**（已有，`src/core/project/stores/project.ts`）:

```typescript
export interface Project {
  id: string
  name: string
  description?: string
  path: string
  createdAt: string
  updatedAt: string
}
```

---

## 5. 交互流程

### 5.1 打开面板

```
用户点击标题栏项目名称
 → ProjectSwitcherPanel.showPanel = true
 → 面板在按钮下方弹出（position: absolute）
 → 自动聚焦，支持 Esc 关闭
```

### 5.2 切换项目

```
用户点击非当前项目卡片
 → emit('switch-project', project)
 → WorkbenchTitleBar.handleSwitchProject()
 → useTitleBar.switchProject()
 → useProjectStore.switchProject(projectId)
 → invoke('add_recent_project') + invoke('open_project_by_id')
 → 面板关闭
 → 标题栏项目名更新
 → 发射 'project-switched' 事件
```

### 5.3 重命名项目

```
用户点击 [···] → "重命名"
 → renamingProjectId = project.id
 → 项目名称变为 NInput 输入框
 → 回车确认
 → invoke('rename_project', { projectId, newName })
 → 刷新最近项目列表
 → renamingProjectId = null
```

### 5.4 编辑项目信息

```
用户点击 [···] → "编辑项目信息"
 → showEditModal = true
 → EditProjectModal 显示
 → 用户修改名称/描述 → 保存
 → invoke('update_project', { id, name, description })
 → 刷新最近项目列表
 → 标题栏更新
```

### 5.5 删除项目

```
用户点击 [···] → "删除项目"
 → showDeleteConfirm = true
 → DeleteProjectConfirmModal 显示
 → 用户输入项目名称确认
 → "确认删除" 按钮启用
 → invoke('delete_project_disk', { projectId })
 → 刷新最近项目列表
 → 如果删除的是当前项目 → currentProject = null
```

### 5.6 移除项目

```
用户点击 [×] 或 [···] → "从列表中移除"
 → invoke('remove_from_recent', { projectId })
 → 刷新最近项目列表（项目从列表消失）
```

---

## 6. 样式约束

| 规则                              | 说明                                                                                 |
| --------------------------------- | ------------------------------------------------------------------------------------ |
| 所有颜色使用 CSS 变量             | `var(--color-bg-primary)`, `var(--color-text-primary)`, `var(--brand-accent)` 等     |
| 间距使用 `--spacing-*` 系列       | `--spacing-xs(4px)`, `--spacing-sm(8px)`, `--spacing-md(12px)`, `--spacing-lg(16px)` |
| 圆角使用 `--border-radius-*` 系列 | `--border-radius-sm(4px)`, `--border-radius-md(6px)`                                 |
| 字号使用 `--font-size-*` 系列     | `--font-size-sm(12px)`, `--font-size-md(13px)`, `--font-size-lg(14px)`               |
| 暗色/亮色双主题适配               | 所有变量在 `.theme-dark` / `.theme-light` 下均有定义                                 |
| 组件高度                          | 32px（按钮/输入框），36px（面板标题）                                                |
| 过渡动画                          | 0.15s-0.2s ease                                                                      |
| 当前项目高亮                      | `border: 1px solid var(--brand-accent)`                                              |
| 悬停效果                          | `background: var(--color-hover)`                                                     |
| 移除按钮悬停                      | `color: var(--brand-danger)`                                                         |

---

## 7. i18n 翻译键

### 新增翻译键

| 键                                 | zh-CN                                                                  | en                                                                                          |
| ---------------------------------- | ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `workbench.switchProject`          | 切换项目                                                               | Switch Project                                                                              |
| `workbench.renameProject`          | 重命名                                                                 | Rename                                                                                      |
| `workbench.editProjectInfo`        | 编辑项目信息                                                           | Edit Project Info                                                                           |
| `workbench.removeFromRecent`       | 从列表中移除                                                           | Remove from List                                                                            |
| `workbench.deleteProjectTitle`     | 删除项目                                                               | Delete Project                                                                              |
| `workbench.confirmDeleteProject`   | 确认删除项目                                                           | Confirm Delete Project                                                                      |
| `workbench.deleteProjectWarning`   | 此操作不可恢复！将永久删除项目目录及所有数据：                         | This action cannot be undone! Will permanently delete the project directory and all data:   |
| `workbench.deleteProjectInputHint` | 请输入项目名称以确认删除                                               | Please enter the project name to confirm deletion                                           |
| `workbench.confirmDelete`          | 确认删除                                                               | Confirm Delete                                                                              |
| `workbench.projectLocation`        | 项目位置                                                               | Project Location                                                                            |
| `workbench.projectCreatedAt`       | 创建时间                                                               | Created At                                                                                  |
| `workbench.projectLastOpened`      | 最后打开                                                               | Last Opened                                                                                 |
| `workbench.manageAllProjects`      | 管理所有项目...                                                        | Manage All Projects...                                                                      |
| `workbench.comingSoon`             | 即将推出                                                               | Coming Soon                                                                                 |
| `workbench.noRecentProjects`       | 暂无最近项目                                                           | No Recent Projects                                                                          |
| `workbench.noRecentProjectsHint`   | 点击上方 [+ 新建项目] 创建你的第一个项目或 [打开已有项目] 导入现有项目 | Click [+ New Project] to create your first project or [Open Existing Project] to import one |
| `workbench.openExistingProject`    | 打开已有项目                                                           | Open Existing Project                                                                       |
| `workbench.lastOpened`             | 最后打开                                                               | Last Opened                                                                                 |
| `workbench.projectRemoved`         | 已从列表中移除                                                         | Removed from List                                                                           |
| `workbench.renameSuccess`          | 重命名成功                                                             | Renamed Successfully                                                                        |
| `workbench.updateSuccess`          | 更新成功                                                               | Updated Successfully                                                                        |

---

## 8. 文件变更清单

### 新增文件

| 文件                                                                           | 说明               |
| ------------------------------------------------------------------------------ | ------------------ |
| `src/extensions/builtin/workbench/ui/components/ProjectSwitcherPanel.vue`      | 项目切换下拉面板   |
| `src/extensions/builtin/workbench/ui/components/EditProjectModal.vue`          | 编辑项目信息模态框 |
| `src/extensions/builtin/workbench/ui/components/DeleteProjectConfirmModal.vue` | 删除确认对话框     |
| `src/extensions/builtin/workbench/ui/utils/format.ts`                          | 共享日期格式化工具 |
| `docs/frontend/title-bar/project-switcher-panel.md`                            | 本文档             |

### 修改文件

| 文件                                                                   | 变更说明                                                                                              |
| ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `src/extensions/builtin/workbench/ui/components/WorkbenchTitleBar.vue` | 替换 ProjectSelector → ProjectSwitcherPanel                                                           |
| `src/extensions/builtin/workbench/ui/services/project.ts`              | 添加 removeFromRecent / deleteProjectDisk API；v1.2: 移除 console.log、添加重试/超时机制              |
| `src/core/project/stores/project.ts`                                   | 添加 removeFromRecent / deleteProjectDisk 方法；v1.2: console.log→debugLog                            |
| `src/extensions/builtin/workbench/ui/composables/useTitleBar.ts`       | 添加 rename / remove / update 方法                                                                    |
| `src-tauri/src/commands/project_commands.rs`                           | 添加 remove_from_recent / delete_project_disk 命令；v1.2: remove_from_recent 返回 ProjectInfoResponse |
| `src-tauri/src/lib.rs`                                                 | 注册新命令                                                                                            |
| `src/shared/locales/zh-CN.json`                                        | 添加翻译键                                                                                            |
| `src/shared/locales/en.json`                                           | 添加翻译键                                                                                            |

### 删除文件

| 文件                                                                                   | 说明                             |
| -------------------------------------------------------------------------------------- | -------------------------------- |
| ~~`src/extensions/builtin/workbench/ui/components/title-bar/ProjectSelector.vue`~~     | 被 ProjectSwitcherPanel.vue 替代 |
| ~~`src/extensions/builtin/workbench/ui/components/title-bar/ProjectSelector.test.ts`~~ | 对应测试文件                     |

---

## 9. 测试要点

- [ ] 点击标题栏项目名弹出面板
- [ ] 点击面板外部关闭
- [ ] 按 Esc 关闭
- [ ] 项目列表最多显示 10 个
- [ ] 当前项目高亮（主题色边框）
- [ ] 单击非当前项目卡片切换项目
- [ ] 单击当前项目卡片无操作
- [ ] 悬停显示 [···] 和 [×]
- [ ] [···] 弹出操作菜单
- [ ] 重命名内联编辑 + 回车确认
- [ ] 编辑项目信息弹出模态框
- [ ] 删除项目二次确认（输入名称）
- [ ] [×] 直接移除
- [ ] 空列表显示空状态
- [ ] 管理所有项目显示禁用态
- [ ] 所有修改立即反映在标题栏

---

## 10. 开发规范检查

- [x] 所有颜色使用 CSS 变量
- [x] 使用 naive-ui 组件库（NButton, NInput, NModal, NDropdown 等）
- [x] 图标使用 lucide-vue-next
- [x] 禁止 any 类型
- [x] 文本使用 i18n（$t()）
- [x] 暗色/亮色双主题适配
- [x] 组件 Props/Emits 强类型
- [x] 单文件 < 300 行
- [x] Rust 禁止 unwrap/expect
- [x] Rust 使用 CoreError 统一错误处理

---

## 11. 开发者指南：如何新增项目管理 Tauri Command

### 11.1 新增命令步骤

**Step 1: Rust — 在 `project_commands.rs` 中添加函数**

```rust
/// 命令描述
#[tauri::command]
pub async fn your_new_command(param: String) -> Result<ReturnType, CoreError> {
    tracing::info!(param = %param, "Executing your_new_command");

    let global_db = crate::core::migration::get_global_db_manager()
        .ok_or_else(|| CoreError::from(
            ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string()
        ))?;

    // 业务逻辑 ...

    Ok(response)
}
```

**Step 2: Rust — 在 `lib.rs` 中注册**

```rust
.invoke_handler(tauri::generate_handler![
    // ... 现有命令 ...
    your_new_command,
])
```

**Step 3: TypeScript — 在 `ProjectService` 中添加方法**

```typescript
static async yourNewMethod(param: string): Promise<ReturnType> {
    return invoke<ReturnType>('your_new_command', { param })
}
```

**Step 4: TypeScript — 在 `project.ts` Store 中添加 action**

```typescript
async function yourNewAction(param: string): Promise<void> {
  try {
    await ProjectService.yourNewMethod(param)
    await loadRecentProjects(true)
  } catch (e) {
    error.value = e instanceof Error ? e.message : '操作失败'
    throw e
  }
}
```

### 11.2 注意事项

- 使用 `CoreError` 而非 `String` 作为错误类型
- 写操作后需调用 `get_recent_projects_cache().invalidate().await`
- 前端调用统一通过 `ProjectService`，不要直接 `invoke()`
- 每个公开方法添加 JSDoc / Rust doc comment
- 在 `zh-CN.json` / `en.json` 中添加对应的 i18n 翻译键
