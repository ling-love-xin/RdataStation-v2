# 前端接口文档

> 版本: v1.0
> 日期: 2026-05-10
> 范围: 标题栏重构相关接口

---

## 一、Command Store 接口

### 1.1 位置

`src/extensions/builtin/workbench/ui/stores/command-store.ts`

### 1.2 Command 接口

```typescript
export interface Command {
  /** 命令唯一标识 */
  id: string
  /** 命令显示名称 */
  label: string
  /** 命令分类（file/edit/view/connection/run/tools/help） */
  category: string
  /** 图标名称（可选） */
  icon?: string
  /** 快捷键显示文本（可选） */
  shortcut?: string
  /** 命令执行函数 */
  action: () => void
}
```

### 1.3 Store 方法

| 方法         | 签名                           | 说明                         |
| ------------ | ------------------------------ | ---------------------------- |
| `register`   | `(command: Command) => void`   | 注册一个新命令               |
| `unregister` | `(commandId: string) => void`  | 注销指定命令                 |
| `execute`    | `(commandId: string) => void`  | 执行指定命令并记录到最近使用 |
| `search`     | `(query: string) => Command[]` | 模糊搜索命令，支持多关键词   |

### 1.4 Store 计算属性

| 属性                 | 类型                     | 说明                            |
| -------------------- | ------------------------ | ------------------------------- |
| `allCommands`        | `Command[]`              | 所有已注册命令                  |
| `commandsByCategory` | `Map<string, Command[]>` | 按分类分组的命令                |
| `recentCommandList`  | `Command[]`              | 最近使用的命令列表（最多 5 个） |

### 1.5 使用示例

```typescript
import { useCommandStore } from '../stores/command-store'

const commandStore = useCommandStore()

// 注册命令
commandStore.register({
  id: 'myCommand',
  label: '我的命令',
  category: 'tools',
  shortcut: 'Ctrl+M',
  action: () => {
    console.log('执行我的命令')
  },
})

// 执行命令
commandStore.execute('myCommand')

// 搜索命令
const results = commandStore.search('新建')
```

---

## 二、CommandPalette 组件接口

### 2.1 位置

`src/extensions/builtin/workbench/ui/components/title-bar/CommandPalette.vue`

### 2.2 Props

| 属性      | 类型      | 必填 | 说明              |
| --------- | --------- | ---- | ----------------- |
| `visible` | `boolean` | 是   | 控制面板显示/隐藏 |

### 2.3 Emits

| 事件    | 参数 | 说明           |
| ------- | ---- | -------------- |
| `close` | -    | 面板关闭时触发 |

### 2.4 功能特性

- **搜索过滤**：实时根据输入过滤命令列表
- **键盘导航**：
  - `↑` / `↓`：选择上一个/下一个命令
  - `Enter`：执行选中的命令
  - `Esc`：关闭面板
- **鼠标交互**：点击执行，悬停高亮
- **空状态**：无匹配时显示提示

### 2.5 使用示例

```vue
<template>
  <CommandPalette :visible="showCommandPalette" @close="showCommandPalette = false" />
</template>

<script setup>
import { ref } from 'vue'
import CommandPalette from './title-bar/CommandPalette.vue'

const showCommandPalette = ref(false)
</script>
```

---

## 三、MenuBar 组件接口

### 3.1 位置

`src/extensions/builtin/workbench/ui/components/title-bar/MenuBar.vue`

### 3.2 类型定义

```typescript
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
```

### 3.3 Props

| 属性    | 类型           | 必填 | 说明         |
| ------- | -------------- | ---- | ------------ |
| `menus` | `MenuConfig[]` | 是   | 菜单配置数组 |

### 3.4 Emits

| 事件          | 参数               | 说明               |
| ------------- | ------------------ | ------------------ |
| `menu-action` | `(item: MenuItem)` | 菜单项被点击时触发 |

### 3.5 键盘导航

| 快捷键  | 功能         |
| ------- | ------------ |
| `Alt+F` | 打开文件菜单 |
| `Alt+E` | 打开编辑菜单 |
| `Alt+V` | 打开视图菜单 |
| `Alt+C` | 打开连接菜单 |
| `Alt+R` | 打开运行菜单 |
| `Alt+T` | 打开工具菜单 |
| `Alt+H` | 打开帮助菜单 |
| `Esc`   | 关闭菜单     |

---

## 四、ToolbarActions 组件接口

### 4.1 位置

`src/extensions/builtin/workbench/ui/components/title-bar/ToolbarActions.vue`

### 4.2 类型定义

```typescript
export interface ToolbarTool {
  id: string
  name: string
  icon: unknown
  enabled: boolean
  action: () => void
}
```

### 4.3 Props

| 属性    | 类型            | 必填 | 说明         |
| ------- | --------------- | ---- | ------------ |
| `tools` | `ToolbarTool[]` | 是   | 工具配置数组 |

### 4.4 Emits

| 事件            | 参数                                 | 说明                   |
| --------------- | ------------------------------------ | ---------------------- |
| `tool-action`   | `(toolId: string)`                   | 工具按钮被点击时触发   |
| `toggle-tool`   | `(toolId: string, enabled: boolean)` | 工具启用状态改变时触发 |
| `reset-toolbar` | -                                    | 重置工具栏时触发       |

---

## 五、全局自定义事件

### 5.1 事件列表

标题栏通过 `CustomEvent` 向全局派发以下事件：

| 事件名                         | 说明            | 触发来源               |
| ------------------------------ | --------------- | ---------------------- |
| `workbench:new-query`          | 新建查询        | 菜单、命令面板、快捷键 |
| `workbench:new-connection`     | 新建连接        | 菜单、命令面板、快捷键 |
| `workbench:open-project`       | 打开项目        | 菜单、命令面板         |
| `workbench:save`               | 保存            | 菜单、命令面板         |
| `workbench:undo`               | 撤销            | 菜单                   |
| `workbench:redo`               | 重做            | 菜单                   |
| `workbench:cut`                | 剪切            | 菜单                   |
| `workbench:copy`               | 复制            | 菜单                   |
| `workbench:paste`              | 粘贴            | 菜单                   |
| `workbench:find`               | 查找            | 菜单                   |
| `workbench:replace`            | 替换            | 菜单                   |
| `workbench:toggle-sidebar`     | 显示/隐藏侧边栏 | 菜单                   |
| `workbench:toggle-panel`       | 显示/隐藏面板   | 菜单                   |
| `workbench:manage-connections` | 管理连接        | 菜单                   |
| `workbench:disconnect`         | 断开连接        | 菜单                   |
| `workbench:execute-sql`        | 执行 SQL        | 菜单、命令面板         |
| `workbench:execute-script`     | 执行脚本        | 菜单                   |
| `workbench:stop-execution`     | 停止执行        | 菜单                   |
| `workbench:plugin-management`  | 插件管理        | 菜单                   |
| `workbench:open-settings`      | 打开设置        | 菜单、命令面板、工具栏 |
| `workbench:keyboard-shortcuts` | 键盘快捷键      | 菜单、工具栏           |
| `workbench:open-docs`          | 打开文档        | 菜单、工具栏           |
| `workbench:open-history`       | 打开历史记录    | 工具栏                 |
| `workbench:open-terminal`      | 打开终端        | 工具栏                 |
| `workbench:check-updates`      | 检查更新        | 菜单                   |
| `workbench:about`              | 关于            | 菜单                   |
| `open-customize-layout-dialog` | 自定义布局      | 标题栏按钮             |

### 5.2 监听示例

```typescript
// 在需要监听事件的组件中
window.addEventListener('workbench:new-query', () => {
  // 处理新建查询
})

window.addEventListener('workbench:open-settings', () => {
  // 打开设置面板
})
```

---

## 六、useTitleBar Composable 接口

### 6.1 位置

`src/extensions/builtin/workbench/ui/composables/useTitleBar.ts`

### 6.2 返回值

| 属性/方法            | 类型                                             | 说明           |
| -------------------- | ------------------------------------------------ | -------------- |
| `currentProject`     | `ComputedRef<string>`                            | 当前项目名称   |
| `recentProjects`     | `ComputedRef<Project[]>`                         | 最近项目列表   |
| `loadRecentProjects` | `() => Promise<void>`                            | 加载最近项目   |
| `switchProject`      | `(project: Project) => Promise<void>`            | 切换项目       |
| `createProject`      | `(name, path, description?) => Promise<Project>` | 创建项目       |
| `openProject`        | `(path: string) => Promise<void>`                | 打开项目       |
| `toggleTheme`        | `() => void`                                     | 切换主题       |
| `saveToolbarConfig`  | `(tools: ToolbarTool[]) => void`                 | 保存工具栏配置 |
| `loadToolbarConfig`  | `(tools: ToolbarTool[]) => void`                 | 加载工具栏配置 |

---

## 七、工具栏配置持久化

### 7.1 存储键

```typescript
const TOOLBAR_STORAGE_KEY = 'customToolbar'
```

### 7.2 存储格式

```typescript
interface ToolbarConfig {
  id: string
  enabled: boolean
}

// 存储在 localStorage 中
localStorage.setItem(
  'customToolbar',
  JSON.stringify([
    { id: 'settings', enabled: true },
    { id: 'history', enabled: false },
    // ...
  ])
)
```

---

## 八、版本历史

| 版本 | 日期       | 说明                                 |
| ---- | ---------- | ------------------------------------ |
| v1.0 | 2026-05-10 | 初始版本，记录标题栏重构后的所有接口 |
