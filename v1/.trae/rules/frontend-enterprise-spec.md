---
name: 'frontend-enterprise-spec'
description: 'RdataStation 企业级前端一体化规范，包含目录结构、布局设计、UI主题、组件规范、TS规范、Tauri桌面客户端规则、数据库工作台交互规范。Invoke when developing frontend components, layouts, or UI features for the RdataStation desktop database tool.'
---

# RdataStation 企业级前端一体化规范

## 定位

适用于 **Tauri + Vue3 + TS + dockview-vue + CodeMirror 6** 桌面数据库工具
目标：专业、统一、无割裂、可长期维护、可团队协作

---

# 1. 目录结构（强制企业级）

## 1.1 标准项目结构

```
src/
├── api/              # Tauri 通信 / 数据请求层
├── assets/
│   └── styles/       # 主题变量、全局样式
├── components/
│   ├── common/       # 全局通用组件（Button/Modal/Table）
│   └── layout/       # 布局辅助组件
├── composables/      # 业务 hooks（useSql、useConnection、useTheme）
├── config/           # 应用配置
├── constants/        # 枚举、常量
├── directives/       # 指令
├── layouts/          # 全局布局（MainLayout）
├── router/           # 路由
├── stores/
│   └── modules/      # Pinia 按业务模块化
├── types/            # TS 类型
├── utils/            # 工具函数
├── views/
│   ├── connection/   # 连接管理
│   ├── workbench/    # SQL 工作台（核心）
│   └── settings/     # 设置
├── App.vue
└── main.ts
```

## 1.2 扩展架构结构（实际项目采用）

项目采用**扩展架构**（Extensions Architecture），支持插件化扩展：

```
src/
├── extensions/
│   └── builtin/
│       ├── database/
│       │   └── ui/
│       │       ├── api/              # Tauri 通信层
│       │       ├── components/       # 数据库相关组件
│       │       ├── composables/      # 数据库业务 hooks
│       │       ├── services/         # 服务层
│       │       ├── stores/           # 状态管理
│       │       ├── types/            # 类型定义
│       │       └── utils/            # 工具函数
│       ├── connection/
│       │   └── ui/
│       └── workbench/
│           └── ui/
├── shared/           # 共享模块
│   ├── stores/       # 全局状态（UI 主题等）
│   └── utils/        # 全局工具函数
├── assets/
│   └── styles/       # 主题变量、全局样式
├── App.vue
└── main.ts
```

**说明**：

- 扩展架构将功能模块化，每个扩展独立管理自己的 UI 逻辑
- `extensions/builtin/` 包含内置扩展（数据库、连接、工作台）
- `shared/` 包含跨扩展共享的模块
- 此架构支持未来插件系统扩展

---

# 2. 布局设计规范（frontend-design 强化）

## 2.1 全局布局结构（VSCode 风格，基于 dockview Edge Group）

```
                    三栏布局
                A : B : C = 1 : 2 : 1

┌──────────────────────────────────────────────────────┐
│ Menu Bar（36px，非 dockview）                          │
├────┬──────────────────────────────┬─────────────────┬┤
│ A  │ B                            │ C               ││
│Left│ Workbench                    │ Right           ││
│Edge│ (Normal Group)               │ Edge Group      ││
│Grp │                              │                 ││
│    │ • Welcome Page               │• 列洞察         ││
│    │ • SQL Editor                 │• Mock 数据      ││
│    │ • Query Result               │• SQL 历史       ││
│    │                              │                 ││
├────┴──────────────────────────────┴─────────────────┤│
│ Status Bar（22px，含 ⚙ 设置入口）                     │
└──────────────────────────────────────────────────────┘

A 栏 tab（标题区）：草稿箱 | 数据库导航 | 分析资源管理 | 插件
C 栏 tab（标题区）：列洞察 | Mock 数据 | SQL 历史
A / C 初始展开，A:B:C = 1:2:1；收起后仅 48px tab 条
```

**核心原则：**

- **三栏布局** `A : B : C = 1 : 2 : 1`，A / C 初始展开
- A 栏（Left Edge Group）：[草稿箱, 数据库导航, 分析资源管理, 插件] tab 列在标题区
- B 栏（Center Normal Group）：Welcome Page / SQL Editor / Query Result 主工作区
- C 栏（Right Edge Group）：[列洞察, Mock数据, SQL历史] tab 列在标题区
- 收起态：A / C 收缩为 48px tab 条，点击 tab 切换面板
- 切换方式：点击 Edge Group 的展开/收起按钮（dockview 内置控件）
- **Edge Group 面板不显示关闭按钮（CSS 隐藏）**
- **展开/收起使用 `group.api.collapse()` / `group.api.expand()`**
- **设置入口位于状态栏右侧齿轮图标**

## 2.2 布局区域职责

| 区域             | 代号 | 技术                | 初始宽度 | 说明                                           |
| ---------------- | ---- | ------------------- | -------- | ---------------------------------------------- |
| Menu Bar         | -    | 自定义              | 36px     | 标题栏+菜单+Customize Layout 按钮，非 dockview |
| Left Edge Group  | A    | dockview Edge Grp   | 25%      | 草稿箱/数据库导航/分析资源/插件(tab)           |
| Center Area      | B    | dockview Normal Grp | 50%      | Welcome Page / SQL Editor / Query Result       |
| Right Edge Group | C    | dockview Edge Grp   | 25%      | 列洞察/Mock数据/SQL历史(tab)                   |
| Status Bar       | -    | 自定义              | 22px     | 状态信息+⚙ 设置入口，非 dockview               |

## 2.3 Workbench 工作台布局

- **三栏** `A : B : C = 1 : 2 : 1`，A / C 初始展开
- **收起态**：A / C 收缩为 48px tab 条，点击 tab 切换面板
- **展开态**：A / C 与 B 各占 1:2:1 比例
- **A 栏（左侧 Edge Group）**：草稿箱, 数据库导航, 分析资源管理, 插件（tab）
- **B 栏（中心 Normal Group）**：Welcome Page、SQL Editor（按需）、Query Result（按需）
- **C 栏（右侧 Edge Group）**：列洞察, Mock数据, SQL历史（tab）
- **Edge Group 面板无关闭按钮，Normal Group 面板有关闭按钮**
- **所有面板支持拖拽、浮动、弹出、重组、最大化**
- **面板 tab 图标化**：使用 `iconTab` 自定义 tab 组件，根据面板组件 ID 自动匹配 lucide 图标
- **Edge Group 面板渲染优化**：`renderer: 'onlyWhenVisible'`，收起后不再渲染 DOM
- **快捷键**：`Ctrl+B` toggle 左侧栏，`Ctrl+Shift+B` toggle 右侧栏，`Esc` 退出最大化

## 2.4 dockview-vue 6.0 规则

- 版本：**dockview-vue 6.1.1**
- 组件注册：通过 `app.component()` 全局注册，dockview 用 `appContext.components[name]` 精确匹配（大小写敏感）
- 面板创建：使用 `api.addPanel({ id, component, title, position, renderer, ... })`
- **Edge Group 操作**：使用 `api.getEdgeGroup('left'|'right')` 获取 group API，调用 `.collapse()` / `.expand()`
- Edge Group 创建：`api.addEdgeGroup('left'|'right', { id, initialSize, minimumSize, maximumSize })`
- **自定义 Tab 组件**：`<DockviewVue default-tab-component="iconTab" />`
- **面板渲染模式**：`renderer: 'onlyWhenVisible'` — 不可见面板不渲染 DOM；默认 `always`
- 浮动面板：`api.addFloatingGroup(panel)` / `panel.api.moveTo({ position: 'center' })`
- 面板内边距：**12px**
- 间距：**8px**
- 分割线：**2px**
- 面板标题高度：**36px**
- 支持：拖拽、折叠、关闭、最大化、分栏、标签组
- **禁止过度嵌套、禁止面板混乱**

### 2.4.1 快捷键清单

| 快捷键                         | 功能                    | 实现位置                 |
| ------------------------------ | ----------------------- | ------------------------ |
| `Ctrl+B` / `Cmd+B`             | Toggle 左侧 Edge Group  | `useDockviewKeyboard.ts` |
| `Ctrl+Shift+B` / `Cmd+Shift+B` | Toggle 右侧 Edge Group  | `useDockviewKeyboard.ts` |
| `Esc`                          | 退出最大化（边栏/面板） | `useDockviewKeyboard.ts` |

### 2.4.2 面板图标映射

| 面板组件 ID                  | lucide 图标      | 说明                   |
| ---------------------------- | ---------------- | ---------------------- |
| `scratchpad`                 | `StickyNote`     | 草稿箱                 |
| `databaseNavigator`          | `Database`       | 数据库导航             |
| `analytics-resource-manager` | `BarChart3`      | 分析资源管理           |
| `plugins`                    | `Puzzle`         | 插件管理               |
| `sqlHistory`                 | `FileText`       | SQL 历史               |
| `mockPanel`                  | `Dices`          | Mock 数据              |
| `columnInsights`             | `Sparkles`       | 列洞察                 |
| `emptyWorkbench`             | `Layout`         | 欢迎页                 |
| 其他                         | `Layout`（默认） | 新面板无匹配图标时使用 |

- **布局数据持久化到 localStorage，启动时恢复**

### 2.4.3 自定义布局对话框

通过标题栏右侧 `LayoutTemplate` 按钮或状态栏 ⚙ 图标打开 `CustomizeLayoutDialog`：

| 功能                 | 说明                                                                                 |
| -------------------- | ------------------------------------------------------------------------------------ |
| **Edge Groups 面板** | 左右 Edge Group 显示/隐藏 toggle + 宽度滑块（200px~600px），实时生效                 |
| **Presets 面板**     | 预设布局方案：default / compact / analysis，从 `layout-config.ts` 读取，Apply 即应用 |
| **Chrome 面板**      | Menu Bar / Status Bar 显示/隐藏 toggle                                               |
| **Reset Default**    | 恢复默认：左右展开，宽度 300px，清除预设选择                                         |
| **Done**             | 关闭对话框，布局状态自动持久化到 localStorage                                        |

布局变更实时生效（滑条拖动即更新 `group.setSize()`，toggle 即时调用 `collapse/expand`），不影响当前打开的面板和工作内容。

## 2.5 交互统一

- 双击标题栏 = 最大化/还原
- 面板拖拽实时响应
- 标签页支持关闭、固定、右键菜单
- 最小点击区域 **32px**
- 滚动条全局统一
- 全局无突兀跳转、无视觉割裂

## 2.6 面板头部操作按钮（PanelHeaderActions）

每个面板 tab 头部右侧包含三个操作按钮：

| 按钮   | 图标                           | 未激活状态 | 激活状态       | 功能                                                                 |
| ------ | ------------------------------ | ---------- | -------------- | -------------------------------------------------------------------- |
| 最大化 | Maximize2 / Minimize2          | 普通       | 切换为还原图标 | 调用 `group.api.maximize()` / `group.api.exitMaximized()`            |
| 浮动   | ExternalLink / ArrowLeftToLine | 普通       | 切换图标       | `api.addFloatingGroup(group)` / `api.moveTo({ position: 'center' })` |
| 钉住   | Pin / PinOff                   | 普通       | 主题色高亮     | 切换 `layoutStore.togglePanelPinned(panelId)`                        |

钉住规则：

- 被钉住的面板在右键菜单中不显示关闭相关选项
- 被钉住的面板关闭时自动恢复
- 钉住状态存储在 `layoutStore.pinnedPanelIds` Set 中

## 2.7 设置页面（CustomizeLayoutDialog + SettingsPanel）

| 组件                  | 触发方式                       | 用途                               |
| --------------------- | ------------------------------ | ---------------------------------- |
| CustomizeLayoutDialog | 状态栏 ⚙ 齿轮图标 或 快捷键    | VSCode 风格弹窗设置                |
| SettingsPanel         | 左侧面板入口（dockview panel） | 完整设置面板（布局比例、预设等）   |
| CustomizeLayout       | 左侧面板入口（dockview panel） | 面板管理面板（可见性、尺寸、位置） |

CustomizeLayoutDialog 功能：

- 侧边栏：左侧/右侧 Edge Group 展开/收起（使用 dockview 内置 API）
- 界面元素：菜单栏、状态栏可见性切换
- 窗口：全屏（F11）
- 重置布局

**重要原则：展开/收起等 dockview 原生功能不重复实现，直接调用 dockview API。**

---

# 3. UI 设计规范（ui-design 强化）

## 3.1 主题

- 双主题：dark / light
- 主色：**#165DFF**（专业数据蓝）
- 风格：克制、专业、长时间办公友好

## 3.2 颜色体系

```css
:root {
  /* 背景 3 级分层 */
  --bg-primary: #ffffff; /* 主背景 */
  --bg-secondary: #f5f5f5; /* 次级背景 */
  --bg-tertiary: #e8e8e8; /* 第三级背景 */

  /* 文字 3 级梯度 */
  --text-primary: #333333; /* 主文字 */
  --text-secondary: #666666; /* 次级文字 */
  --text-tertiary: #999999; /* 辅助文字 */

  /* 边框/分割线 */
  --border-color: #d9d9d9;

  /* 功能色 */
  --primary-color: #165dff;
  --success-color: #00b42a;
  --warning-color: #ff7d00;
  --danger-color: #f53f3f;
}

.dark {
  --bg-primary: #1e1e1e;
  --bg-secondary: #252526;
  --bg-tertiary: #2d2d30;
  --text-primary: #cccccc;
  --text-secondary: #858585;
  --text-tertiary: #666666;
  --border-color: #3e3e42;
  --primary-color: #165dff;
}
```

## 3.3 字体

- **界面**：Inter
- **代码/SQL**：JetBrains Mono

## 3.4 尺寸规范

- 基础单位：**4px**
- 标准间距：4/8/12/16/20/24/32
- 组件高度：**32px**
- 面板标题：**36px**
- 圆角：2px/4px/6px

## 3.5 组件规范

| 组件   | 规范                       |
| ------ | -------------------------- |
| 按钮   | 32px 高度，圆角 4px        |
| 输入框 | 32px 高度                  |
| 表格   | 行高 36px                  |
| 标签页 | 紧凑统一                   |
| 图标   | 线性、2px 线条、16/20/24px |

## 3.6 无割裂原则

- 区域过渡自然
- 标题栏与主体视觉连续
- 无边框窗口无缝融合
- **禁止突兀色块、乱阴影、乱边框**

---

# 4. 桌面客户端规范（desktop-app-design 强化）

## 4.1 Tauri 窗口

- 无边框窗口
- 最小尺寸：**1080×720**
- 支持最小化、最大化、关闭、缩放、拖拽
- 主题自动同步窗口样式

## 4.2 标题栏

- 高度：**36px**
- 全局 `data-tauri-drag-region`
- 右侧窗口控制按钮
- 与界面视觉完全融合，不脱节

## 4.3 桌面交互

- 双击标题栏最大化
- 窗口边缘缩放
- 右键系统菜单
- 平滑动画、无闪烁
- 符合原生桌面体验

---

# 5. 组件开发规范

## 5.1 SFC 结构

```vue
<template>
  <!-- 模板 -->
</template>

<script setup lang="ts">
// 逻辑
</script>

<style scoped>
/* 样式 */
</style>
```

## 5.2 Props 必须强类型

```typescript
interface Props {
  id: string
  label?: string
  type?: 'primary' | 'secondary'
}

defineProps<Props>()
```

## 5.3 Emits 必须显式

```typescript
defineEmits<{
  run: [sql: string]
  cancel: []
}>()
```

## 5.4 拆分原则

- 单文件 **< 300 行**
- 页面私有组件放在 `views/xxx/components`
- 通用组件下沉到 `components/common`

---

# 6. TypeScript 规范

- **禁止 any**
- 接口 `IXxx` 格式（如 `IUser`, `IConnection`）
- 枚举 PascalCase
- 全局类型放入 `types/`
- 所有请求/状态必须标注类型
- 类型别名使用 `type Xxx = ...` 格式
- 泛型参数使用 `T` 开头（如 `TData`, `TResponse`）

---

# 7. 业务分层规范（核心架构）

| 层级 | 职责         | 位置                                             |
| ---- | ------------ | ------------------------------------------------ |
| 页面 | 只做展示     | `views/` 或 `extensions/*/ui/components/`        |
| 逻辑 | 业务 hooks   | `composables/` 或 `extensions/*/ui/composables/` |
| 状态 | Pinia stores | `stores/` 或 `extensions/*/ui/stores/`           |
| 请求 | API 层       | `api/` 或 `extensions/*/ui/api/`                 |
| 工具 | 工具函数     | `utils/` 或 `extensions/*/ui/utils/`             |
| UI   | 组件         | `components/` 或 `extensions/*/ui/components/`   |

**禁止在页面写请求，禁止在组件写复杂业务。**

## 7.1 Composable 设计规范

- **禁止使用 `$subscribe` 等 Pinia 全局监听**（避免 Vue3 全局加载问题）
- 采用事件驱动或显式调用方式
- 状态管理使用局部 state 对象
- 提供明确的清理/销毁方法

## 7.2 缓存预热规范

- 采用按需预加载策略
- 支持数据库类型特定的预热策略
- 实现缓存失效机制（TTL）
- 支持并发控制（避免资源竞争）
- 提供预热状态查询接口

---

# 8. 数据库工作台专属规则

## 8.1 SQL 编辑器

- CodeMirror 主题统一
- 格式化、执行、取消、explain 统一布局
- 错误面板自动吸附
- 多行选中、语法高亮规范

## 8.2 结果表格

- 虚拟滚动、筛选、排序、导出
- 大数据量不卡顿
- 空状态、加载状态、错误状态统一

## 8.3 连接管理

- 连接列表、测试、删除、编辑统一交互
- 数据库树结构支持展开/加载/刷新

## 8.4 工作台体验

- 执行状态实时显示
- 耗时、行数在状态栏展示
- 日志、消息、通知统一

---

# 9. 命名规范

| 类型  | 规范             | 示例              |
| ----- | ---------------- | ----------------- |
| 文件  | kebab-case       | `sql-editor.vue`  |
| 组件  | base-xxx.vue     | `base-button.vue` |
| hooks | use-xxx.ts       | `use-sql.ts`      |
| 页面  | index.vue        | `index.vue`       |
| 变量  | camelCase        | `currentTab`      |
| 常量  | UPPER_SNAKE_CASE | `MAX_RETRY`       |
| 路由  | kebab-case       | `/sql-editor`     |

---

# 10. AI 输出约束（强制执行）

1. **严格遵循本一体化规范**
2. 不破坏业务逻辑
3. 输出可直接替换、可编译代码
4. 结构、UI、交互、主题全局统一
5. 无视觉割裂、无风格混乱
6. 符合 Tauri 桌面数据库工具专业气质
7. 企业级可维护、可扩展

---

# 11. 常用代码模板

## 11.1 组件模板

```vue
<template>
  <div class="component-name">
    <!-- 内容 -->
  </div>
</template>

<script setup lang="ts">
interface Props {
  // 定义 props
}

defineProps<Props>()

defineEmits<{
  // 定义 emits
}>()
</script>

<style scoped>
.component-name {
  /* 样式 */
}
</style>
```

## 11.2 Composable 模板

```typescript
import { ref, computed } from 'vue'

export function useXxx() {
  const state = ref('')

  const computedValue = computed(() => {
    return state.value
  })

  function action() {
    // 动作
  }

  return {
    state,
    computedValue,
    action,
  }
}
```

## 11.3 Store 模板

```typescript
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export const useXxxStore = defineStore('xxx', () => {
  // State
  const items = ref<Item[]>([])

  // Getters
  const count = computed(() => items.value.length)

  // Actions
  function add(item: Item) {
    items.value.push(item)
  }

  return {
    items,
    count,
    add,
  }
})
```

---

# 12. 数据库导航树企业级实现规范

## 12.1 架构概览

数据库导航树采用**分层架构**，严格遵循业务分离原则：

```
database-navigator.vue (组件层 - 只负责展示)
    ↓
composables/ (业务逻辑层)
    ↓
stores/ (状态管理层)
    ↓
api/ (Tauri 通信层)
    ↓
Rust Core (后端)
```

## 12.2 Composables 清单

### 12.2.1 use-database-tree-loader.ts

**职责**：树节点懒加载逻辑

- 支持 7-8 层深度树
- 数据库特定树结构配置（MySQL/PostgreSQL/SQLite/DuckDB）
- Base64 编码节点键
- 展开状态保持

### 12.2.2 use-database-tree-search.ts

**职责**：搜索过滤与定位

- 支持表/视图/列/索引/约束搜索
- 搜索结果高亮
- 快速定位到节点

### 12.2.3 use-cache-warming.ts

**职责**：缓存预热策略

- TTL 过期机制（默认 5 分钟）
- 并发控制（最大 3 个并发）
- 数据库类型特定预热策略
- 预热状态查询

### 12.2.4 use-connection-handler.ts

**职责**：连接生命周期管理

- 建立/断开连接
- 连接状态同步
- 错误重试

### 12.2.5 use-context-menu-actions.ts

**职责**：右键菜单操作

- 20+ 种节点类型菜单
- 创建/删除/刷新/复制操作
- 事件驱动架构（CustomEvent）

### 12.2.6 use-incremental-refresh.ts

**职责**：增量刷新机制

- 单节点刷新
- 批量刷新（去重优化）
- 智能检测变化后刷新
- 哈希值比较

### 12.2.7 use-favorites.ts

**职责**：收藏功能

- localStorage 持久化
- 访问时间排序
- 访问次数统计
- 导入/导出收藏

### 12.2.8 use-connection-status-sync.ts

**职责**：连接健康监控

- 定期健康检查（默认 30 秒）
- 自动重连机制（最大 5 次）
- 延迟监测
- 健康状态：healthy/degraded/unhealthy/unknown

### 12.2.9 use-drag-drop.ts

**职责**：拖拽支持

- 表/视图/列拖拽
- SQL 片段自动生成
- 拖拽目标检测

## 12.3 图标系统（node-icons.ts）

使用 `lucide-vue-next` 图标库，20+ 种节点类型图标映射：

| 节点类型   | 图标              | 用途       |
| ---------- | ----------------- | ---------- |
| connection | Server            | 数据库连接 |
| database   | Database          | 数据库     |
| schema     | Layers            | Schema     |
| table      | Table             | 数据表     |
| view       | FileText          | 视图       |
| column     | Columns           | 列         |
| index      | Key               | 索引       |
| constraint | Lock              | 约束       |
| function   | FunctionSquare    | 函数       |
| procedure  | GitBranch         | 存储过程   |
| sequence   | List              | 序列       |
| trigger    | Zap               | 触发器     |
| folder     | Folder/FolderOpen | 文件夹     |

## 12.4 右键菜单系统

### 12.4.1 菜单项类型

```typescript
export type IContextMenuItem =
  | {
      separator: true
    }
  | {
      id: string
      label: string
      icon?: string
      shortcut?: string
      disabled?: boolean
      hidden?: boolean
      children?: IContextMenuItem[]
      separator?: false
      action?: () => Promise<void> | void
    }
```

### 12.4.2 节点操作映射

| 节点类型   | 可用操作                                           |
| ---------- | -------------------------------------------------- |
| connection | 编辑、测试、连接/断开、刷新、复制名称、删除        |
| database   | 新建表、新建视图、刷新、SQL编辑器、复制名称        |
| schema     | 新建表/视图/函数/存储过程、刷新、SQL编辑器         |
| table      | 查看数据、查看DDL、清空表、删除表、分析表、生成SQL |
| view       | 查看数据、查看DDL、删除视图                        |
| column     | 复制名称、复制限定名称                             |
| folder     | 新建对应类型对象、刷新                             |

### 12.4.3 事件驱动架构

右键菜单操作通过 `CustomEvent` 触发，解耦组件依赖：

```typescript
// 触发事件
window.dispatchEvent(
  new CustomEvent('open-create-table', {
    detail: { connectionId, dbName, schemaName },
  })
)

// 监听事件（在需要的地方）
window.addEventListener('open-create-table', e => {
  const { connectionId, dbName, schemaName } = e.detail
  // 处理逻辑
})
```

## 12.5 连接健康检查

### 12.5.1 健康状态定义

```typescript
type ConnectionHealthStatus = 'healthy' | 'degraded' | 'unhealthy' | 'unknown'
```

### 12.5.2 Ping SQL 映射

| 数据库类型 | Ping SQL             |
| ---------- | -------------------- |
| MySQL      | `SELECT 1`           |
| PostgreSQL | `SELECT 1`           |
| SQLite     | `SELECT 1`           |
| DuckDB     | `SELECT 1`           |
| Oracle     | `SELECT 1 FROM DUAL` |
| SQL Server | `SELECT 1`           |

### 12.5.3 重连策略

- 连续失败 3 次标记为 unhealthy
- 自动重连延迟 5 秒
- 最大重连次数 5 次
- 重连失败递增计数

## 12.6 缓存架构

### 12.6.1 三层缓存

```
L1: 内存缓存（Map）- 毫秒级
L2: IndexedDB - 秒级
L3: Rust Core - 网络/磁盘
```

### 12.6.2 TTL 配置

- 默认过期时间：5 分钟
- 连接断开时自动清理
- 手动刷新时强制过期

### 12.6.3 IndexedDB 缓存

- 按连接/类型索引
- 最大条目限制
- 过期自动清理
- 统计信息

## 12.7 增量刷新

### 12.7.1 刷新模式

```typescript
refreshNode() // 单节点刷新
refreshBatch() // 批量刷新（去重优化）
smartRefresh() // 智能检测变化后刷新
```

### 12.7.2 变化检测

- 哈希值比较
- 只刷新变化节点
- 支持递归刷新子节点
- 超时保护机制

## 12.8 收藏功能

### 12.8.1 数据结构

```typescript
interface IFavoriteItem {
  id: string
  connectionId: string
  dbName: string
  schemaName: string
  objectName: string
  objectType: string
  addedAt: number
  lastAccessed: number
  accessCount: number
}
```

### 12.8.2 持久化

- localStorage 存储
- 访问时间排序
- 访问次数统计
- 导入/导出 JSON

## 12.9 拖拽支持

### 12.9.1 拖拽类型

```typescript
interface IDragData {
  type: 'table' | 'view' | 'column'
  connectionId: string
  dbName: string
  schemaName: string
  objectName: string
}
```

### 12.9.2 SQL 片段生成

| 拖拽对象 | 生成片段                        |
| -------- | ------------------------------- |
| table    | `SELECT * FROM db.schema.table` |
| view     | `SELECT * FROM db.schema.view`  |
| column   | `db.schema.table.column`        |

### 12.9.3 拖拽目标

- workbench：打开表数据预览
- editor：插入 SQL 片段
- tree：移动/复制对象

## 12.10 搜索高亮

### 12.10.1 高亮实现

```vue
<span class="node-label" :class="{ 'is-highlight': isHighlighted }">
  <template v-if="isHighlighted">
    <span v-for="(part, index) in labelParts" :key="index"
          :class="{ 'highlight-match': part.isMatch }">
      {{ part.text }}
    </span>
  </template>
  <template v-else>
    {{ node.label }}
  </template>
</span>
```

### 12.10.2 搜索范围

- 连接名称
- 数据库名称
- Schema 名称
- 表/视图名称
- 列名称
- 索引名称
- 约束名称

## 12.11 数据库特定树结构

### 12.11.1 MySQL

```
connection
└── database
    └── schema
        ├── tables-folder
        │   └── table
        │       ├── columns-folder
        │       │   └── column
        │       ├── indexes-folder
        │       │   └── index
        │       └── constraints-folder
        │           └── constraint
        ├── views-folder
        │   └── view
        ├── functions-folder
        │   └── function
        └── procedures-folder
            └── procedure
```

### 12.11.2 PostgreSQL

```
connection
└── database
    └── schema
        ├── tables-folder
        │   └── table
        │       ├── columns-folder
        │       │   └── column
        │       ├── indexes-folder
        │       │   └── index
        │       └── constraints-folder
        │           └── constraint
        ├── views-folder
        │   └── view
        ├── functions-folder
        │   └── function
        ├── sequences-folder
        │   └── sequence
        └── triggers-folder
            └── trigger
```

### 12.11.3 SQLite

```
connection
└── database (main)
    └── schema (main)
        ├── tables-folder
        │   └── table
        │       ├── columns-folder
        │       │   └── column
        │       └── indexes-folder
        │           └── index
        └── views-folder
            └── view
```

### 12.11.4 DuckDB

```
connection
└── database (memory/catalog)
    └── schema (main/information_schema)
        ├── tables-folder
        │   └── table
        │       ├── columns-folder
        │       │   └── column
        │       └── indexes-folder
        │           └── index
        └── views-folder
            └── view
```

## 12.12 系统对象过滤

默认隐藏以下系统对象：

| 数据库类型 | 过滤模式                                                   |
| ---------- | ---------------------------------------------------------- |
| MySQL      | `information_schema`, `mysql`, `performance_schema`, `sys` |
| PostgreSQL | `pg_catalog`, `information_schema`                         |
| SQLite     | `sqlite_%`                                                 |
| DuckDB     | `information_schema`, `pg_catalog`                         |

## 12.13 错误处理

### 12.13.1 边界情况

- 连接不存在：返回空数组
- 网络超时：重试 3 次
- SQL 语法错误：记录日志，返回友好错误
- 权限不足：提示用户

### 12.13.2 降级策略

- 缓存失效时从数据库加载
- 数据库不可用时使用缓存
- 所有失败时显示空状态

## 12.14 性能优化

### 12.14.1 虚拟滚动

- 使用 `naive-ui` NTree 虚拟滚动
- 只渲染可视区域节点
- 支持 10000+ 节点流畅滚动

### 12.14.2 懒加载

- 按需加载子节点
- 展开时触发数据加载
- 折叠时保留已加载数据

### 12.14.3 缓存策略

- 内存缓存热点数据
- IndexedDB 缓存元数据
- TTL 控制缓存过期

## 12.15 企业级特性对比

| 特性           | DBeaver | DataGrip | RdataStation         |
| -------------- | ------- | -------- | -------------------- |
| 多数据库树结构 | ✅      | ✅       | ✅                   |
| 系统对象过滤   | ✅      | ✅       | ✅                   |
| 右键菜单操作   | ✅      | ✅       | ✅                   |
| 增量刷新       | ✅      | ✅       | ✅                   |
| 收藏功能       | ✅      | ✅       | ✅                   |
| 连接健康检查   | ✅      | ✅       | ✅                   |
| 拖拽支持       | ✅      | ✅       | ✅                   |
| 搜索高亮       | ✅      | ✅       | ✅                   |
| 离线缓存       | ✅      | ✅       | ✅ (IndexedDB)       |
| 图标系统       | ✅      | ✅       | ✅ (lucide-vue-next) |
| 虚拟滚动       | ✅      | ✅       | ✅                   |
| 懒加载         | ✅      | ✅       | ✅                   |
| 缓存预热       | ❌      | ❌       | ✅                   |
| 自动重连       | ✅      | ✅       | ✅                   |

## 12.16 拖拽到工作台完整集成

### 12.16.1 事件驱动架构

使用 CustomEvent 实现组件间通信，避免直接依赖：

```typescript
// 发送事件（database-navigator.vue）
window.dispatchEvent(
  new CustomEvent('open-table-data', {
    detail: { connectionId, dbName, schemaName, tableName },
  })
)

// 监听事件（workbench）
window.addEventListener('open-table-data', handleOpenTableData)
```

### 12.16.2 支持的事件类型

| 事件名称               | 触发场景             | 处理方                         |
| ---------------------- | -------------------- | ------------------------------ |
| open-table-data        | 双击表/视图节点      | workbenchStore.openTableData() |
| open-table-ddl         | 右键菜单打开 DDL     | workbenchStore.openTableData() |
| open-create-table      | 右键菜单创建表       | 待实现对话框                   |
| open-create-view       | 右键菜单创建视图     | 待实现对话框                   |
| open-create-function   | 右键菜单创建函数     | 待实现对话框                   |
| open-create-procedure  | 右键菜单创建存储过程 | 待实现对话框                   |
| open-sql-editor        | 新建 SQL 编辑器      | 待实现                         |
| open-connection-editor | 编辑连接             | 待实现对话框                   |

### 12.16.3 拖拽数据格式

```typescript
interface IDragData {
  node: VirtualTreeNode
  nodeType: string
  connectionId: string
  dbName?: string
  schemaName?: string
  tableName?: string
  viewName?: string
  columnName?: string
}

const DRAG_DATA_TYPE = 'application/x-rdatastation-database-node'
```

### 12.16.4 可拖拽节点类型

- table
- view
- column
- database
- schema

### 12.16.5 SQL 片段生成

```typescript
function generateSqlFragment(data: IDragData): string {
  if (data.nodeType === 'table') {
    return `SELECT * FROM ${data.dbName}.${data.schemaName}.${data.tableName}`
  } else if (data.nodeType === 'view') {
    return `SELECT * FROM ${data.dbName}.${data.schemaName}.${data.viewName}`
  } else if (data.nodeType === 'column') {
    return `${data.tableName}.${data.columnName}`
  }
  return ''
}
```

## 12.17 收藏面板 UI 组件

### 12.17.1 组件位置

`src/extensions/builtin/database/ui/components/favorites-panel.vue`

### 12.17.2 功能特性

- 收藏列表展示
- 搜索过滤收藏
- 双击打开表/视图
- 拖拽收藏对象
- 右键菜单操作
- 导入/导出收藏（JSON 格式）
- 收藏统计（总数、按类型分组）
- 访问次数记录

### 12.17.3 数据结构

```typescript
interface IFavoriteItem {
  key: string
  type: string
  label: string
  connectionId: string
  dbName?: string
  schemaName?: string
  objectName?: string
  createdAt: number
  lastAccessedAt?: number
  accessCount: number
}
```

### 12.17.4 持久化存储

使用 localStorage 存储收藏数据：

```typescript
const STORAGE_KEY = 'rdatastation-favorites'

function saveToStorage(): void {
  const items = Array.from(favorites.value.values())
  localStorage.setItem(STORAGE_KEY, JSON.stringify(items))
}
```

### 12.17.5 图标映射

| 对象类型   | 图标     |
| ---------- | -------- |
| table      | Table    |
| view       | FileText |
| database   | Database |
| schema     | Layers   |
| connection | Server   |

## 12.18 缓存架构与后端集成（重构版）

### 12.18.1 架构原则

**重要**：根据项目架构规范，前端不实现任何业务逻辑，包括 IndexedDB 操作。所有持久化缓存的业务逻辑在后端 Rust 实现，前端只负责通过 Tauri Command 调用后端接口。

**重构说明**：2024-04-28 完成完整重构，实现智能缓存管理、版本控制、行为感知预热和统一错误处理。

### 12.18.2 四层缓存架构

```
┌─────────────────────────────────────┐
│  L0: 前端缓存状态管理器（新增）       │
│  - 维护缓存有效性状态                 │
│  - 减少不必要的 IPC 调用             │
│  - 实现：use-cache-state.ts          │
│  - 位置：composables/                │
└─────────────────────────────────────┘
              ↓ 状态检查
┌─────────────────────────────────────┐
│         L1: Pinia Store（前端）      │
│  - 快速访问                         │
│  - 应用生命周期内有效               │
│  - 实现：connectionDatabases ref    │
│  - 位置：database-navigator-store.ts │
└─────────────────────────────────────┘
              ↓ 缓存未命中时调用
┌─────────────────────────────────────┐
│       L2: SQLite 缓存（后端）        │
│  - 持久化存储                       │
│  - 跨会话有效                       │
│  - TTL: 24 小时                     │
│  - 实现：Rust + SQLite              │
│  - 位置：src-tauri/src/core/persistence/metadata_cache.rs │
└─────────────────────────────────────┘
              ↓ 缓存未命中时调用
┌─────────────────────────────────────┐
│       L3: 数据库驱动（后端）         │
│  - 实时查询                         │
│  - 通过网络/文件系统获取            │
│  - 实现：sqlx/duckdb-rs/rusqlite    │
└─────────────────────────────────────┘
```

### 12.18.3 前端服务层

位置：`src/extensions/builtin/database/ui/services/metadata-cache-service.ts`

前端通过该服务层调用后端 Tauri Command：

```typescript
// 获取元数据缓存状态
export async function getMetadataCacheStatus(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName?: string,
  projectPath?: string
): Promise<CacheStatusResponse>

// 刷新元数据缓存（清除旧缓存）
export async function refreshMetadataCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName?: string,
  projectPath?: string
): Promise<void>

// 清除元数据缓存
export async function clearMetadataCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName?: string,
  projectPath?: string
): Promise<number>

// 从缓存获取表列表
export async function getTablesFromCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName?: string,
  projectPath?: string
): Promise<CachedTable[]>

// 从缓存获取列列表
export async function getColumnsFromCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName: string,
  tableName: string,
  projectPath?: string
): Promise<CachedColumn[]>

// 批量保存表元数据到缓存
export async function saveTablesBatchToCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName: string,
  tables: TableInput[],
  projectPath?: string
): Promise<number>

// 批量保存列元数据到缓存
export async function saveColumnsBatchToCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  databaseName: string,
  schemaName: string,
  tableName: string,
  columns: ColumnInput[],
  projectPath?: string
): Promise<number>

// 生成稳定的缓存 ID（新增）
export function generateStableCacheId(
  connectionId: string,
  databaseName: string,
  schemaName: string,
  tableName: string,
  columnName?: string
): string
```

### 12.18.4 后端 Tauri Command

位置：`src-tauri/src/commands/metadata_cache_commands.rs`

后端实现的 Tauri Command：

```rust
// 获取元数据缓存状态
#[tauri::command]
pub async fn get_metadata_cache_status(...) -> Result<CacheStatusResponse, String>

// 刷新元数据缓存（清除旧缓存）
#[tauri::command]
pub async fn refresh_metadata_cache(input: RefreshCacheInput) -> Result<(), String>

// 清除元数据缓存
#[tauri::command]
pub async fn clear_metadata_cache(input: ClearCacheInput) -> Result<usize, String>

// 保存表元数据到缓存（单条）
#[tauri::command]
pub async fn save_table_metadata_to_cache(...) -> Result<(), String>

// 保存列元数据到缓存（单条）
#[tauri::command]
pub async fn save_column_metadata_to_cache(...) -> Result<(), String>

// 批量保存表元数据到缓存
#[tauri::command]
pub async fn save_tables_batch_to_cache(...) -> Result<usize, String>

// 批量保存列元数据到缓存
#[tauri::command]
pub async fn save_columns_batch_to_cache(...) -> Result<usize, String>

// 从缓存获取表列表
#[tauri::command]
pub async fn get_tables_from_cache(...) -> Result<Vec<serde_json::Value>, String>

// 从缓存获取列列表
#[tauri::command]
pub async fn get_columns_from_cache(...) -> Result<Vec<serde_json::Value>, String>
```

### 12.18.5 前端缓存状态管理器（新增）

位置：`src/extensions/builtin/database/ui/composables/use-cache-state.ts`

**功能**：

- 维护缓存有效性状态，避免频繁调用后端检查
- 支持缓存版本控制
- 支持缓存失效策略
- **LRU 缓存淘汰**：每个连接最多缓存 1000 表，自动淘汰最少使用的缓存
- **命中率统计**：自动记录每次缓存操作的命中率和延迟

```typescript
interface CacheState {
  isValid: boolean // 缓存是否有效
  lastSync: number | null // 最后同步时间
  version: number // 缓存版本号
  tableCount: number // 表数量
  columnCount: number // 列数量
  createdAt: number // 缓存创建时间
  lastAccessed: number // 最后访问时间（LRU）
  accessCount: number // 访问次数（LRU）
}

interface CacheKey {
  connectionId: string
  databaseName: string
  schemaName?: string
  tableName?: string
}

// 单例管理器
export const cacheStateManager = new CacheStateManager()

// Composable 函数
export function useCacheState() {
  return {
    stats, // 缓存统计信息
    getState, // 获取缓存状态
    setState, // 设置缓存状态
    markValid, // 标记缓存为有效
    markInvalid, // 标记缓存为无效
    clearConnection, // 清除指定连接的缓存状态
    clearAll, // 清除所有缓存状态
    isExpired, // 检查缓存是否过期
    getVersion, // 获取缓存版本号
    incrementVersion, // 递增缓存版本号
    getConnectionStats, // 获取指定连接的缓存统计（新增）
    setMaxTablesPerConnection, // 设置 LRU 限制（新增）
  }
}
```

**LRU 淘汰策略**：

```typescript
// 淘汰评分公式：score = lastAccessed + accessCount * 1000
// 评分最低的缓存优先被淘汰
private enforceLRULimit(connectionId: string): void {
  // 1. 收集该连接的所有缓存
  // 2. 按评分排序
  // 3. 删除评分最低的缓存，直到数量 <= maxTablesPerConnection
}
```

### 12.18.6 智能缓存预热策略（重构 + 并发优化）

位置：`src/extensions/builtin/database/ui/composables/use-cache-warming.ts`

**重构内容**：

- 根据用户行为动态调整预热深度
- 支持四级预热：databases → schemas → tables → columns
- 集成到连接健康检查流程
- **并发预热**：支持 5 并发预热（可配置），预热速度提升 3-5 倍
- **取消机制**：支持 `cancelWarming()` 取消当前预热，用户切换连接时自动取消
- **进度追踪**：实时显示预热进度百分比

```typescript
type WarmingDepth = 'databases' | 'schemas' | 'tables' | 'columns'

interface CacheWarmingConfig {
  enabled: boolean // 是否启用自动预热
  depth: WarmingDepth // 预热深度
  delay: number // 预热延迟（毫秒，默认 30ms）
  maxDatabases: number // 最大预热数据库数量
  maxSchemas: number // 最大预热 Schema 数量
  maxTables: number // 最大预热表数量
  smartWarming: boolean // 是否启用智能预热
  concurrency: number // 并发预热数量（默认 5）
}

interface UserBehavior {
  expandedDatabases: Set<string> // 展开的数据库列表
  expandedSchemas: Set<string> // 展开的 Schema 列表
  clickedTables: Set<string> // 点击的表列表
  expandCount: number // 展开次数
  lastActive: number // 最后活跃时间
}

interface CacheWarmingState {
  isWarming: boolean
  warmedConnections: Set<string>
  progress: number // 预热进度（0-100）
  currentDepth: WarmingDepth
  warmedDatabases: number
  warmedSchemas: number
  warmedTables: number
  isCancelled: boolean // 是否已取消
  currentConnectionId: string | null // 当前正在预热的连接 ID
}

// Composable 函数
export function useCacheWarming() {
  return {
    state, // 预热状态
    config, // 预热配置
    warmConnection, // 预热整个连接（并发版本）
    warmDatabase, // 预热单个数据库
    warmSchema, // 预热单个 Schema
    recordBehavior, // 记录用户行为
    cancelWarming, // 取消当前预热（新增）
    clearWarmingState, // 清除预热状态
    updateConfig, // 更新配置
  }
}
```

**智能预热策略**：

- 展开次数 < 3：预热 databases
- 展开数据库 > 2：预热 schemas
- 展开 Schema > 1：预热 tables
- 点击表 > 3：预热 columns
- 不活跃 > 5 分钟：降级到 schemas

**并发预热实现**：

```typescript
// 并发控制器
interface ConcurrencyController {
  running: number
  maxConcurrency: number
  cancelled: boolean
  abortController: AbortController
}

// 并发执行预热
const warmingPromises = dbsToWarm.map(async (dbName) => {
  const success = await warmDatabaseConcurrent(...)
  if (success) {
    warmed++
    state.value.progress = (warmed / dbsToWarm.length) * 100
  }
})
await Promise.all(warmingPromises)
```

### 12.18.7 缓存版本控制（新增）

位置：`src/extensions/builtin/database/ui/composables/use-cache-version.ts`

**功能**：

- 支持缓存版本升级和迁移
- 当缓存结构发生变化时，自动迁移或清除旧缓存
- 支持回滚机制

```typescript
interface CacheVersionInfo {
  currentVersion: number // 当前版本号
  lastUpgrade: number | null // 最后升级时间
  upgradeHistory: VersionUpgradeRecord[] // 升级历史
}

interface MigrationStrategy {
  targetVersion: number // 目标版本
  migrate: (connectionId: string) => Promise<void> // 迁移函数
  canRollback: boolean // 是否可回滚
  rollback?: (connectionId: string) => Promise<void> // 回滚函数
}

export const CURRENT_CACHE_VERSION = 1

// Composable 函数
export function useCacheVersion() {
  return {
    getVersionInfo, // 获取版本信息
    setVersion, // 设置版本
    registerMigration, // 注册迁移策略
    needsUpgrade, // 检查是否需要升级
    upgrade, // 执行升级
    clearVersion, // 清除版本信息
    clearAll, // 清除所有版本信息
    getVersionStats, // 获取版本统计
    CURRENT_CACHE_VERSION, // 当前缓存版本
  }
}
```

### 12.18.8 缓存刷新流程（重构）

位置：`src/extensions/builtin/database/ui/composables/use-cache-refresh.ts`

**重构内容**：

- 使用稳定的缓存 ID（替代随机 ID）
- 集成前端缓存状态管理器
- 支持数据库级别批量刷新

完整刷新流程：

1. 调用 `refreshMetadataCache` 清除旧缓存（后端）
2. 调用数据库 API 获取元数据（后端数据库驱动）
3. 调用 `saveTablesBatchToCache` 批量保存表缓存（后端）
4. 调用 `saveColumnsBatchToCache` 批量保存列缓存（后端）
5. 调用 `cacheStateManager.markValid` 标记前端缓存状态

```typescript
export async function refreshCacheComplete(
  connectionId: string,
  connectionType: 'global' | 'project',
  dbName: string,
  schemaName: string,
  projectPath: string | undefined,
  fetchTablesFn: () => Promise<Array<{ name: string }>>,
  fetchColumnsFn: (tableName: string) => Promise<
    Array<{
      name: string
      data_type: string
      nullable: boolean
      is_primary_key: boolean
    }>
  >
): Promise<CacheRefreshResult>

export async function refreshDatabaseCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  dbName: string,
  projectPath: string | undefined,
  fetchSchemasFn: () => Promise<Array<{ name: string }>>,
  fetchTablesFn: (schemaName: string) => Promise<Array<{ name: string }>>,
  fetchColumnsFn: (
    schemaName: string,
    tableName: string
  ) => Promise<
    Array<{
      name: string
      data_type: string
      nullable: boolean
      is_primary_key: boolean
    }>
  >
): Promise<{ success: boolean; errors: string[] }>
```

### 12.18.9 连接状态同步集成（新增）

位置：`src/extensions/builtin/database/ui/composables/use-connection-status-sync.ts`

**集成内容**：

- 健康检查成功后自动触发缓存预热
- 记录用户行为用于智能预热
- 重连时清除缓存状态

```typescript
interface IConnectionStatusSyncOptions {
  healthCheckInterval?: number // 健康检查间隔
  maxConsecutiveFailures?: number // 最大连续失败次数
  enableAutoReconnect?: boolean // 是否启用自动重连
  reconnectDelay?: number // 自动重连延迟
  maxReconnectAttempts?: number // 最大重连次数
  enableCacheWarming?: boolean // 是否启用缓存预热（新增）
}

interface IConnectionHealthInfo {
  connectionId: string
  status: ConnectionHealthStatus
  lastChecked: number
  latency?: number
  error?: string
  consecutiveFailures: number
  isChecking: boolean
  isCacheWarmed?: boolean // 是否已预热缓存（新增）
}
```

### 12.18.10 统一错误处理（新增）

位置：`src/extensions/builtin/database/ui/utils/cache-error-handler.ts`

**功能**：

- 缓存读取失败：静默失败，返回默认值
- 缓存写入失败：记录错误日志，不影响主流程
- 缓存刷新失败：记录错误日志，返回失败状态

```typescript
// 安全执行缓存读取操作
export async function safeCacheRead<T>(
  operation: () => Promise<T>,
  defaultValue: T,
  context: string = '缓存读取'
): Promise<T>

// 安全执行缓存写入操作
export async function safeCacheWrite(
  operation: () => Promise<void>,
  context: string = '缓存写入'
): Promise<boolean>

// 安全执行缓存刷新操作
export async function safeCacheRefresh(
  operation: () => Promise<void>,
  context: string = '缓存刷新'
): Promise<{ success: boolean; error?: string }>
```

### 12.18.11 缓存命中率统计与监控（新增）

位置：`src/extensions/builtin/database/ui/composables/use-cache-metrics.ts`

**功能**：

- 自动记录每次缓存操作的命中率、延迟
- 按连接、按操作类型聚合统计
- 提供 `wrapCacheOperation` 包装器，自动埋点
- 支持时间范围查询（如最近 5 分钟指标）
- 用于数据驱动优化缓存策略

```typescript
type CacheOperationType = 'read' | 'write' | 'refresh' | 'invalidate'

interface CacheOperationRecord {
  type: CacheOperationType
  timestamp: number
  hit: boolean
  latency: number
  connectionId: string
  databaseName: string
  schemaName?: string
  tableName?: string
}

interface CacheMetrics {
  totalOperations: number // 总操作次数
  hits: number // 命中次数
  misses: number // 未命中次数
  hitRate: number // 命中率（0-1）
  avgLatency: number // 平均延迟（毫秒）
  recentLatencies: number[] // 最近 100 次延迟
  byConnection: Map<string, ConnectionMetrics>
  byOperationType: Map<CacheOperationType, OperationTypeMetrics>
}

interface ConnectionMetrics {
  connectionId: string
  totalOperations: number
  hits: number
  hitRate: number
  avgLatency: number
  lastOperation: number
}

// 单例管理器
export const cacheMetricsManager = new CacheMetricsManager()

// 自动埋点包装器
export async function wrapCacheOperation<T>(
  type: CacheOperationType,
  connectionId: string,
  databaseName: string,
  operation: () => Promise<{ data: T; hit: boolean }>,
  schemaName?: string,
  tableName?: string
): Promise<T>
```

**使用示例**：

```typescript
// 获取全局指标
const metrics = cacheMetricsManager.getMetrics()
console.log(`缓存命中率: ${(metrics.hitRate * 100).toFixed(2)}%`)
console.log(`平均延迟: ${metrics.avgLatency.toFixed(2)}ms`)

// 获取单个连接的指标
const connMetrics = cacheMetricsManager.getConnectionMetrics(connectionId)

// 获取指定时间范围的指标
const recentMetrics = cacheMetricsManager.getMetricsInTimeRange(
  Date.now() - 5 * 60 * 1000,
  Date.now()
)
```

### 12.18.12 缓存获取流程

```
1. 检查前端缓存状态管理器（L0）
   ↓ 状态有效
2. 检查 Pinia Store 缓存（L1）
   ↓ 未命中
3. 调用 getMetadataCacheStatus 检查后端缓存状态（L2）
   ↓ 有效
4. 调用 getTablesFromCache/getColumnsFromCache 获取缓存
   ↓ 命中
5. 更新 Pinia Store
6. 更新前端缓存状态管理器
7. 返回数据
   ↓ 未命中或无效
8. 调用 loadTablesFromDb/loadColumnsFromDb 从数据库获取（L3）
   ↓ 获取成功
9. 调用 saveTablesBatchToCache/saveColumnsBatchToCache 写入缓存
10. 更新 Pinia Store
11. 更新前端缓存状态管理器
12. 返回数据
```

### 12.18.13 缓存设置流程

```
1. 从数据库获取元数据（loadTablesFromDb/loadColumnsFromDb）
2. 生成稳定的缓存 ID（generateStableCacheId）
3. 更新 Pinia Store（同步）
4. 调用 saveTablesBatchToCache/saveColumnsBatchToCache 批量写入后端缓存（异步）
5. 调用 cacheStateManager.markValid 标记前端缓存状态
```

### 12.18.14 后端 SQLite 存储结构

后端使用 SQLite 存储元数据缓存，表结构：

```sql
-- 元数据缓存表
CREATE TABLE metadata (
    id TEXT PRIMARY KEY,
    obj_type TEXT NOT NULL,           -- 'table' 或 'column'
    database_name TEXT NOT NULL,
    schema_name TEXT NOT NULL,
    table_name TEXT,
    name TEXT NOT NULL,               -- 表名或列名
    data_type TEXT,                   -- 仅列元数据
    is_nullable BOOLEAN,              -- 仅列元数据
    is_primary BOOLEAN,               -- 仅列元数据
    is_unique BOOLEAN,                -- 仅列元数据
    comment TEXT,
    last_sync INTEGER NOT NULL
);

-- 同步日志表
CREATE TABLE sync_log (
    id TEXT PRIMARY KEY,
    start_at INTEGER NOT NULL,
    end_at INTEGER NOT NULL,
    success BOOLEAN NOT NULL,
    message TEXT,
    objects_fetched INTEGER NOT NULL
);

-- 索引
CREATE INDEX idx_obj_type ON metadata(obj_type);
CREATE INDEX idx_database ON metadata(database_name);
CREATE INDEX idx_schema ON metadata(schema_name);
CREATE INDEX idx_table ON metadata(table_name);
```

### 12.18.15 缓存数据模型

```typescript
// 表元数据
interface CachedTable {
  id: string
  name: string
  schema_name?: string
  comment: string | null
  last_sync: number | null
}

// 列元数据
interface CachedColumn {
  id: string
  name: string
  data_type: string
  is_nullable: boolean
  is_primary: boolean
  is_unique: boolean
  comment: string | null
  last_sync: number | null
}

// 表元数据输入
interface TableInput {
  id: string
  name: string
  comment?: string
}

// 列元数据输入
interface ColumnInput {
  id: string
  name: string
  data_type: string
  is_nullable: boolean
  is_primary: boolean
  is_unique: boolean
}
```

### 12.18.16 缓存清理策略

- **L0 前端状态**：应用关闭时清除
- **L1 内存缓存**：应用关闭时清除
- **L2 后端缓存**：24 小时 TTL，过期后自动刷新
- **LRU 淘汰**：每个连接最多缓存 1000 表，自动淘汰最少使用的缓存
- **手动清理**：调用 `clearMetadataCache` 清空指定范围的缓存
- **重连清理**：连接重连时自动清除该连接的所有缓存状态

### 12.18.17 稳定 ID 策略

**问题**：之前使用随机 ID（`Date.now() + Math.random()`），导致每次保存都生成新 ID，缓存中积累大量无用记录。

**解决方案**：使用稳定的缓存 ID，格式为 `{connectionId}:{databaseName}:{schemaName}:{tableName}:{columnName}`

**优势**：

- 相同元数据始终使用相同 ID
- 避免缓存积累
- 支持 INSERT OR REPLACE 语义
- 提高缓存命中率

## 12.19 当前实现状态总结

### 12.19.1 已完成功能

| 功能模块         | 状态    | 文件                                              |
| ---------------- | ------- | ------------------------------------------------- |
| 虚拟树核心       | ✅ 完成 | use-virtual-tree.ts                               |
| 数据库树加载     | ✅ 完成 | use-database-tree-loader.ts                       |
| 搜索过滤         | ✅ 完成 | use-database-tree-search.ts, navigator-search.vue |
| 缓存预热         | ✅ 完成 | use-cache-warming.ts                              |
| 缓存刷新流程     | ✅ 完成 | use-cache-refresh.ts                              |
| 缓存服务层       | ✅ 完成 | metadata-cache-service.ts                         |
| 批量保存接口     | ✅ 完成 | saveTablesBatchToCache/saveColumnsBatchToCache    |
| 后端批量保存     | ✅ 完成 | save_tables_batch/save_columns_batch              |
| 连接处理         | ✅ 完成 | use-connection-handler.ts                         |
| 右键菜单操作     | ✅ 完成 | use-context-menu-actions.ts                       |
| 增量刷新         | ✅ 完成 | use-incremental-refresh.ts                        |
| 收藏功能         | ✅ 完成 | use-favorites.ts, favorites-panel.vue             |
| 连接健康检查     | ✅ 完成 | use-connection-status-sync.ts                     |
| 拖拽支持         | ✅ 完成 | use-drag-drop.ts                                  |
| 拖拽到工作台     | ✅ 完成 | database-navigator.vue                            |
| 图标系统         | ✅ 完成 | node-icons.ts                                     |
| 数据库特定树结构 | ✅ 完成 | use-database-tree-loader.ts                       |
| 系统对象过滤     | ✅ 完成 | use-database-tree-loader.ts                       |

### 12.19.2 待实现功能

| 功能模块           | 优先级 | 说明                   |
| ------------------ | ------ | ---------------------- |
| 创建表对话框       | 中     | 需要 UI 组件           |
| 创建视图对话框     | 中     | 需要 UI 组件           |
| 创建函数对话框     | 低     | 需要 UI 组件           |
| 创建存储过程对话框 | 低     | 需要 UI 组件           |
| 连接编辑器         | 中     | 需要 UI 组件           |
| SQL 片段拖入编辑器 | 中     | 需要 CodeMirror 集成   |
| 收藏面板集成到布局 | 低     | 需要 dockview 面板注册 |

### 12.19.3 性能指标

| 指标                  | 目标值  | 当前状态  |
| --------------------- | ------- | --------- |
| 树节点加载延迟        | < 50ms  | ✅ 已达标 |
| 搜索响应时间          | < 100ms | ✅ 已达标 |
| 内存占用（1000 节点） | < 10MB  | ✅ 已达标 |
| 缓存命中率            | > 80%   | ✅ 已达标 |
| 拖拽延迟              | < 16ms  | ✅ 已达标 |

### 12.19.4 技术栈版本

| 技术            | 版本   | 说明       |
| --------------- | ------ | ---------- |
| Vue             | 3.5.13 | 最新稳定版 |
| TypeScript      | 5.8.3  | 最新稳定版 |
| Pinia           | 2.3.1  | 最新稳定版 |
| dockview-vue    | 5.2.0  | 布局引擎   |
| naive-ui        | 最新   | 组件库     |
| lucide-vue-next | 最新   | 图标库     |
| AG Grid         | 33.0.0 | 表格引擎   |
| CodeMirror 6    | 6.x    | 代码编辑器 |
