# 标题栏重构架构文档

> 版本: v2.0
> 日期: 2026-05-10
> 状态: ✅ 已完成

---

## 一、重构背景

### 1.1 问题描述

`WorkbenchTitleBar.vue` 原为 1100+ 行的"上帝组件"，存在以下问题：

- **违反单文件规范**：超过 300 行限制（原 1100+ 行）
- **职责不单一**：包含 UI 渲染、业务逻辑、状态管理、模态框、样式
- **维护困难**：代码审查成本高，定位问题困难
- **测试困难**：逻辑与 UI 耦合，难以单元测试
- **菜单系统缺失**：7 个顶级菜单只有文本，无下拉功能
- **命令面板缺失**：搜索框只有 UI，无实际功能
- **工具栏未赋能**：6 个工具按钮全部 `notImplemented`

### 1.2 重构目标

- [x] 将 1100+ 行拆分为 6 个独立组件 + 1 个 composable + 1 个 store
- [x] 实现完整的菜单系统（7 个顶级菜单 + 下拉面板）
- [x] 实现命令面板（Command Palette）
- [x] 赋能自定义工具栏（6 个工具）
- [x] 统一样式规范（CSS 变量、无硬编码）
- [x] 完善键盘导航和交互

---

## 二、架构设计

### 2.1 组件拆分架构

```
WorkbenchTitleBar.vue (入口组件, ~120 行)
├── title-bar/
│   ├── MenuBar.vue              # 汉堡菜单 + 7 个顶级菜单 + 下拉面板 (190 行)
│   ├── ProjectSelector.vue      # 项目选择器 + 下拉 (已拆分)
│   ├── CommandCenter.vue        # 命令中心搜索框 (已拆分)
│   ├── CommandPalette.vue       # 命令面板弹窗 (139 行) ⭐新增
│   ├── ToolbarActions.vue       # 自定义工具栏按钮组 (已拆分)
│   ├── WindowControls.vue       # 最小化/最大化/关闭 (已拆分)
│   └── NewProjectModal.vue      # 新建项目对话框 (已拆分)
├── stores/
│   └── command-store.ts         # 命令注册表 store (98 行) ⭐新增
├── composables/
│   └── useTitleBar.ts           # 标题栏业务逻辑聚合 (88 行)
└── styles/
    └── title-bar.css            # 标题栏共享样式
```

### 2.2 组件职责

| 组件              | 职责                                            | Props                              | Emits                                           |
| ----------------- | ----------------------------------------------- | ---------------------------------- | ----------------------------------------------- |
| WorkbenchTitleBar | 入口组装，协调子组件，注册命令，处理全局快捷键  | `isMaximized: boolean`             | `minimize`, `maximize`, `close`                 |
| MenuBar           | 渲染汉堡按钮 + 7 个菜单项 + 下拉面板 + 键盘导航 | `menus: MenuConfig[]`              | `menu-action`                                   |
| ProjectSelector   | 渲染项目按钮 + 下拉菜单                         | `currentProject`, `recentProjects` | `switch-project`, `new-project`, `open-project` |
| CommandCenter     | 渲染搜索按钮，触发命令面板                      | -                                  | `open`                                          |
| CommandPalette    | 命令面板弹窗：搜索、执行命令                    | `visible`                          | `close`                                         |
| ToolbarActions    | 渲染工具栏按钮 + 自定义下拉                     | `tools: ToolbarTool[]`             | `tool-action`, `toggle-tool`, `reset-toolbar`   |
| WindowControls    | 渲染窗口控制按钮                                | `isMaximized`                      | `minimize`, `maximize`, `close`                 |

### 2.3 数据流设计

```
WorkbenchTitleBar
├── useTitleBar() composable
│   ├── useProjectStore()      # 项目状态
│   ├── useUiStore()           # UI 状态（主题）
│   └── useCommandStore()      # 命令注册表 ⭐新增
├── MenuBar ──→ menuConfig (本地状态)
├── ProjectSelector ──→ projectStore
├── CommandCenter ──→ 打开 CommandPalette
├── CommandPalette ──→ useCommandStore
├── ToolbarActions ──→ toolbarConfig (本地状态)
└── WindowControls ──→ Tauri API
```

---

## 三、接口设计

### 3.1 Command Store 接口 ⭐新增

```typescript
// src/extensions/builtin/workbench/ui/stores/command-store.ts

export interface Command {
  id: string
  label: string
  category: string
  icon?: string
  shortcut?: string
  action: () => void
}

export const useCommandStore = defineStore('commands', () => {
  // State
  const commands = ref<Map<string, Command>>(new Map())
  const recentCommands = ref<string[]>([])

  // Getters
  const allCommands = computed(() => Array.from(commands.value.values()))
  const commandsByCategory = computed(() => /* 按分类分组 */)
  const recentCommandList = computed(() => /* 最近使用 */)

  // Actions
  function register(command: Command): void
  function unregister(commandId: string): void
  function execute(commandId: string): void
  function search(query: string): Command[]  // 模糊搜索 + 排序
})
```

### 3.2 CommandPalette 接口 ⭐新增

```typescript
// Props
interface CommandPaletteProps {
  visible: boolean
}

// Emits
interface CommandPaletteEmits {
  (e: 'close'): void
}

// 功能
// - 搜索输入（实时过滤）
// - 键盘导航（↑↓ 选择，Enter 执行，Esc 关闭）
// - 鼠标悬停高亮
// - 分类显示 + 快捷键提示
```

### 3.3 MenuBar 接口

```typescript
// 菜单项数据结构
export interface MenuItem {
  id: string
  label: string
  icon?: unknown
  shortcut?: string
  disabled?: boolean
  separator?: boolean
  action?: () => void
}

export interface MenuConfig {
  id: string
  label: string
  items: MenuItem[]
}

// 键盘导航
// - Alt + F/E/V/C/R/T/H: 打开对应菜单
// - Esc: 关闭菜单
// - 点击外部: 关闭菜单
```

### 3.4 ToolbarActions 接口

```typescript
export interface ToolbarTool {
  id: string
  name: string
  icon: unknown
  enabled: boolean
  action: () => void
}

// 功能
// - 显示已启用的工具按钮
// - 下拉面板：启用/禁用工具
// - 重置为默认
// - 配置持久化（localStorage）
```

---

## 四、全局快捷键系统 ⭐新增

### 4.1 已实现的快捷键

| 快捷键         | 功能          | 触发位置                |
| -------------- | ------------- | ----------------------- |
| `Ctrl+Shift+P` | 打开命令面板  | WorkbenchTitleBar       |
| `Ctrl+N`       | 新建查询      | WorkbenchTitleBar       |
| `Ctrl+Shift+N` | 新建连接      | WorkbenchTitleBar       |
| `Alt+F`        | 打开文件菜单  | MenuBar                 |
| `Alt+E`        | 打开编辑菜单  | MenuBar                 |
| `Alt+V`        | 打开视图菜单  | MenuBar                 |
| `Alt+C`        | 打开连接菜单  | MenuBar                 |
| `Alt+R`        | 打开运行菜单  | MenuBar                 |
| `Alt+T`        | 打开工具菜单  | MenuBar                 |
| `Alt+H`        | 打开帮助菜单  | MenuBar                 |
| `Esc`          | 关闭下拉/面板 | MenuBar, CommandPalette |

### 4.2 命令注册表

已注册的核心命令：

| 命令 ID          | 标签     | 分类       | 快捷键       |
| ---------------- | -------- | ---------- | ------------ |
| `newQuery`       | 新建查询 | file       | Ctrl+N       |
| `newConnection`  | 新建连接 | connection | Ctrl+Shift+N |
| `openProject`    | 打开项目 | file       | Ctrl+O       |
| `save`           | 保存     | file       | Ctrl+S       |
| `executeSql`     | 执行 SQL | run        | Ctrl+Enter   |
| `settings`       | 设置     | tools      | Ctrl+,       |
| `commandPalette` | 命令面板 | view       | Ctrl+Shift+P |

---

## 五、样式规范

### 5.1 共享样式变量

```css
/* title-bar.css */
.title-bar {
  height: 36px;
  background: var(--bg-secondary);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0;
  flex-shrink: 0;
  user-select: none;
  border-bottom: 1px solid var(--border-color);
}

/* 下拉面板统一样式 */
.dropdown-panel {
  position: absolute;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--border-radius-md);
  padding: var(--spacing-xs) 0;
  z-index: 1000;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  min-width: 200px;
}

/* 图标按钮 */
.icon-btn {
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--border-radius-sm);
  transition: all 0.2s;
}
```

### 5.2 命令面板样式

```css
.command-palette-overlay {
  position: fixed;
  background: rgba(0, 0, 0, 0.4);
  backdrop-filter: blur(2px);
  z-index: 2000;
}

.command-palette-container {
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--border-radius-md);
  max-width: 600px;
}

.command-item.active,
.command-item:hover {
  background: var(--primary-light);
  color: var(--text-primary);
}
```

---

## 六、开发进度

### Phase 1: 组件拆分 + 样式规范化 ✅

- [x] 创建目录结构
- [x] 提取共享样式 `title-bar.css`
- [x] 创建 `useTitleBar.ts` composable
- [x] 拆分 `MenuBar.vue`
- [x] 拆分 `ProjectSelector.vue`
- [x] 拆分 `CommandCenter.vue`
- [x] 拆分 `ToolbarActions.vue`
- [x] 拆分 `WindowControls.vue`
- [x] 拆分 `NewProjectModal.vue`
- [x] 重构 `WorkbenchTitleBar.vue` 为入口组件
- [x] 验证 lint + typecheck

### Phase 2: 菜单系统实现 ✅

- [x] 定义菜单数据结构
- [x] 实现菜单下拉面板
- [x] 实现 7 个顶级菜单内容
- [x] 添加菜单项点击处理
- [x] 添加键盘导航（Alt 聚焦）
- [x] 添加点击外部关闭
- [x] 添加 Esc 关闭

### Phase 3: 命令面板实现 ✅

- [x] 创建命令注册表 store (`command-store.ts`)
- [x] 实现命令面板组件 (`CommandPalette.vue`)
- [x] 实现模糊搜索算法（多关键词 + 排序）
- [x] 注册核心命令（7 个）
- [x] 添加快捷键绑定（Ctrl+Shift+P）
- [x] 键盘导航（↑↓ Enter Esc）
- [x] 最近使用命令记录

### Phase 4: 工具栏赋能 ✅

- [x] 设置面板打开（settings）
- [x] 历史记录面板打开（history）
- [x] 文档链接打开（docs）
- [x] 快捷键参考面板（shortcuts）
- [x] 终端面板打开（terminal）
- [x] 快速操作浮层（quick → 命令面板）
- [x] 工具栏配置持久化

### Phase 5: 键盘导航 + 交互优化 ✅

- [x] 实现 Alt 聚焦菜单栏（Alt+F/E/V/C/R/T/H）
- [x] 实现 Esc 关闭下拉
- [x] 实现点击外部关闭
- [x] 全局快捷键系统（Ctrl+Shift+P, Ctrl+N, Ctrl+Shift+N）
- [x] 命令面板键盘导航

---

## 七、翻译键规划

### 7.1 已新增的翻译键

```json
{
  "menu": {
    "newQuery": "新建查询",
    "newConnection": "新建连接",
    "openProject": "打开项目...",
    "save": "保存",
    "undo": "撤销",
    "redo": "重做",
    "cut": "剪切",
    "copy": "复制",
    "paste": "粘贴",
    "find": "查找",
    "replace": "替换",
    "commandPalette": "命令面板",
    "toggleSidebar": "显示/隐藏侧边栏",
    "togglePanel": "显示/隐藏面板",
    "executeSql": "执行 SQL",
    "stopExecution": "停止执行",
    "pluginManagement": "插件管理",
    "settings": "设置",
    "keyboardShortcuts": "键盘快捷键",
    "documentation": "文档",
    "checkUpdates": "检查更新",
    "about": "关于 RdataStation"
  },
  "commandPalette": {
    "placeholder": "输入命令...",
    "noResults": "未找到匹配的命令",
    "recentlyUsed": "最近使用"
  }
}
```

---

## 八、质量报告

### 8.1 Lint 状态

- **当前**: 4 errors, ~418 warnings
- **基线**: 16 errors, 499 warnings
- **改进**: 减少 12 errors, 减少 ~81 warnings

### 8.2 Typecheck 状态

- **当前**: 29 errors（与基线一致，未引入新错误）

### 8.3 代码规范检查

- [x] 无 `unwrap()` / `expect()`（Rust 规范）
- [x] 无 `any` 类型（前端规范）
- [x] 使用 CSS 变量（无硬编码颜色）
- [x] 图标组件化使用（lucide-vue-next）
- [x] 使用 Pinia store 管理状态
- [x] 组件 / 逻辑分离

---

## 九、风险与注意事项

1. **快捷键冲突**：全局快捷键（Ctrl+N, Ctrl+Shift+P）可能与 Monaco Editor 冲突，需要进一步完善焦点管理
2. **命令面板定位**：当前命令面板使用 Teleport 挂载到 body，z-index 为 2000，确保覆盖其他面板
3. **主题一致性**：命令面板和菜单下拉已使用 CSS 变量，支持暗色/亮色主题
4. **i18n 完整性**：所有新增文本已同步到 `zh-CN.json` 和 `en.json`
5. **向后兼容**：重构期间保持现有功能不受影响

---

## 十、参考文档

- [DESIGN-SYSTEM-IMPLEMENTATION.md](./DESIGN-SYSTEM-IMPLEMENTATION.md)
- [frontend-enterprise-spec.md](../../.trae/rules/frontend-enterprise-spec.md)
- [common-rules.md](../../.trae/rules/common-rules.md)
- [API-INTERFACE.md](./API-INTERFACE.md) ⭐新增（见下方）

---

## 附录：新增文件清单

| 文件路径                                                                      | 行数 | 说明                       |
| ----------------------------------------------------------------------------- | ---- | -------------------------- |
| `src/extensions/builtin/workbench/ui/stores/command-store.ts`                 | 98   | 命令注册表 Pinia store     |
| `src/extensions/builtin/workbench/ui/components/title-bar/CommandPalette.vue` | 139  | 命令面板组件               |
| `docs/frontend/API-INTERFACE.md`                                              | -    | 前端接口文档（见下方创建） |

## 附录：修改文件清单

| 文件路径                                                               | 修改内容                                               |
| ---------------------------------------------------------------------- | ------------------------------------------------------ |
| `src/extensions/builtin/workbench/ui/components/WorkbenchTitleBar.vue` | 导入 useCommandStore，注册命令，全局快捷键，工具栏赋能 |
| `src/extensions/builtin/workbench/ui/components/title-bar/MenuBar.vue` | 键盘导航，点击外部关闭                                 |
| `src/shared/locales/zh-CN.json`                                        | 新增菜单和命令面板翻译                                 |
| `src/shared/locales/en.json`                                           | 新增英文翻译                                           |
