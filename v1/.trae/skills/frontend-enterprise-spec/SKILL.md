---
name: 'frontend-enterprise-spec'
description: 'RdataStation 企业级前端一体化规范，包含目录结构、布局设计、UI主题、组件规范、TS规范、Tauri桌面客户端规则、数据库工作台交互规范。Invoke when developing frontend components, layouts, or UI features for the RdataStation desktop database tool.'
---

# RdataStation 企业级前端一体化规范

## 定位

适用于 **Tauri + Vue3 + TS + dockview-vue + Monaco** 桌面数据库工具
目标：专业、统一、无割裂、可长期维护、可团队协作

---

# 1. 目录结构（强制企业级）

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

---

# 2. 布局设计规范（frontend-design 强化）

## 2.1 全局布局结构（VSCode 风格）

```
┌──────────────────────────────────────────────────┐
│ Menu Bar（36px，非 dockview，自定义标题栏）         │
├────┬──────────┬───────────────────┬──────────┬────┤
│Left│Left      │                   │Right     │Rght│
│Act │Primary   │   Center Area     │Primary   │Act │
│Bar │Side Bar  │  (SQL Editor,     │Side Bar  │Bar │
│48px│(280px)   │   Welcome Page)   │(280px)   │48px│
│    │          │                   │          │    │
│    │• 数据库   │                   │• 列洞察   │    │
│    │  导航     │                   │• SQL历史  │    │
│    │• 分析资源 │                   │          │    │
│    │  管理器   │                   │          │    │
│    │• 插件     │                   │          │    │
│    │• 设置     │                   │          │    │
│    │• 自定义   │                   │          │    │
│    │  布局     │                   │          │    │
├────┴──────────┴───────────────────┴──────────┴────┤
│ Panel / Bottom Area（输出、查询结果，可拖拽高度）    │
├──────────────────────────────────────────────────┤
│ Status Bar（22px，非 dockview）                    │
└──────────────────────────────────────────────────┘
```

**核心原则：除 Menu Bar 和 Status Bar 外，所有区域均使用 dockview-vue 6.0 管理。**

## 2.2 布局区域职责

| 区域                   | 技术       | 宽度/高度       | 说明                                |
| ---------------------- | ---------- | --------------- | ----------------------------------- |
| Menu Bar               | 自定义组件 | 36px            | 标题栏 + 菜单，非 dockview          |
| Left Activity Bar      | dockview   | 48px（固定）    | 左侧图标导航，切换 Primary Side Bar |
| Left Primary Side Bar  | dockview   | 280px（可拖拽） | 数据库导航、分析资源、插件、设置    |
| Center Area            | dockview   | 自适应          | SQL 编辑器、欢迎页                  |
| Right Primary Side Bar | dockview   | 280px（可拖拽） | 列洞察、SQL 历史                    |
| Right Activity Bar     | dockview   | 48px（固定）    | 右侧图标导航，切换右侧面板          |
| Panel/Bottom           | dockview   | 200px（可拖拽） | 输出、查询结果                      |
| Status Bar             | 自定义组件 | 22px            | 连接状态、执行信息，非 dockview     |

## 2.3 Workbench 工作台布局

- **左侧面板组**：数据库导航、分析资源管理器、插件、设置、自定义布局（dockview tab 组）
- **中心区域**：SQL 编辑器（按需创建）、欢迎页
- **右侧面板组**：列洞察、SQL 历史（dockview tab 组）
- **底部面板**：输出面板、查询结果（按需创建）
- **所有面板支持拖拽、重组、关闭、最大化**

## 2.4 dockview-vue 6.0 规则

- 版本：**dockview-vue 6.0+**
- 组件注册：通过 `getCurrentInstance().appContext.components` 全局注册
- 面板创建：使用 `api.addPanel({ id, component, title, position, ... })`
- 固定宽度面板：设置 `minimumWidth` = `maximumWidth` = 目标宽度
- 面板内边距：**12px**
- 间距：**8px**
- 分割线：**2px**
- 面板标题高度：**36px**
- 支持：拖拽、折叠、关闭、最大化、分栏、标签组
- **禁止过度嵌套、禁止面板混乱**
- **布局数据持久化到 localStorage，启动时恢复**

## 2.5 交互统一

- 双击标题栏 = 最大化/还原
- 面板拖拽实时响应
- 标签页支持关闭、固定、右键菜单
- 最小点击区域 **32px**
- 滚动条全局统一
- 全局无突兀跳转、无视觉割裂

---

# 3. UI 设计规范（ui-design 强化）

## 3.1 主题

- 双主题：dark / light
- 风格：克制、专业、长时间办公友好

## 3.2 颜色体系（新版设计系统）

品牌基色（`src/shared/styles/tokens.css`）：

```css
:root {
  --brand-accent: #e17055;
  --brand-accent-hover: #d35400;
  --brand-accent-soft: rgba(225, 112, 85, 0.15);
  --brand-success: #00b894;
  --brand-danger: #d63031;
  --brand-warning: #fdcb6e;
}
```

暗色主题（`body.theme-dark`）：

```css
--color-bg-primary: #1e1f22;
--color-bg-secondary: #2b2d30;
--color-bg-tertiary: #2d3436;
--color-bg-elevated: #3d4446;
--color-text-primary: #e5e7eb;
--color-text-secondary: #9ca3af;
--color-text-muted: #6b7280;
--color-border: #4a5458;
--color-border-subtle: #3c3f41;
--color-hover: #454545;
--color-selection: rgba(225, 112, 85, 0.25);
```

亮色主题（`body.theme-light`）：

```css
--color-bg-primary: #ffffff;
--color-bg-secondary: #f5f5f5;
--color-bg-tertiary: #e5e7eb;
--color-bg-elevated: #ffffff;
--color-text-primary: #1f2937;
--color-text-secondary: #4b5563;
--color-text-muted: #9ca3af;
--color-border: #b2bec3;
--color-border-subtle: #e5e7eb;
--color-hover: #e5e7e9;
--color-selection: rgba(225, 112, 85, 0.15);
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

## 4.4 Tauri WebView2 HTML5 DnD 配置（强制规则）

### 关键发现

Tauri v2 的 `dragDropEnabled` 默认值为 `true`，其含义为：**启用 Tauri 内部拖放系统 → 拦截所有 DOM `dragover`/`drop` 事件 → 转为 `tauri://drag-drop` Rust 事件 → HTML5 原生 DnD 完全失效。**

dockview 依赖 HTML5 Drag and Drop API 实现 tab 拖拽、分组、浮动。因此 **`dragDropEnabled` 必须设置为 `false`**。

### 强制配置

`src-tauri/tauri.conf.json`:

```json
{
  "app": {
    "windows": [
      {
        "dragDropEnabled": false
      }
    ]
  }
}
```

### 补充权限

`src-tauri/capabilities/default.json`:

```json
{
  "permissions": ["core:window:allow-start-dragging", "core:webview:allow-create-webview-window"]
}
```

| 权限                                       | 作用                                                       |
| ------------------------------------------ | ---------------------------------------------------------- |
| `core:window:allow-start-dragging`         | 允许 Tauri 原生窗口拖拽（标题栏 `data-tauri-drag-region`） |
| `core:webview:allow-create-webview-window` | 允许 dockview `addPopoutGroup()` 创建独立弹出窗口          |

## 4.5 dockview Popout 独立窗口配置

### 前提

1. `dragDropEnabled: false`（见 4.4）
2. `core:webview:allow-create-webview-window` 权限已添加（见 4.4）

### 创建承载页面

`public/popout.html`:

```html
<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <style>
      html,
      body {
        margin: 0;
        padding: 0;
        width: 100%;
        height: 100%;
        overflow: hidden;
        background: #1e1e1e;
      }
    </style>
  </head>
  <body>
    <div id="popout-root" style="width: 100%; height: 100%;"></div>
    <script type="module" src="/src/app/popout.ts"></script>
  </body>
</html>
```

### DockviewVue 配置

```vue
<DockviewVue :popout-url="'/popout.html'" :floating-group-bounds="'boundedWithinViewport'" />
```

## 4.6 dockview Context Menu / Header Actions 规则

### 组件注册

Header actions 组件必须全局注册（`app.component`）才能在 dockview 内部被 Vue 解析：

```typescript
// main.ts
app.component('panelHeaderActions', PanelHeaderActions)
```

### params 结构（由 dockview-vue VueHeaderActionsRenderer 传入）

```
params.api              = DockviewGroupPanelApi（group 级别 API）
params.containerApi     = DockviewGroupPanelApi（同 params.api）
params.group            = DockviewGroupPanel（→ group.model.accessor.api = DockviewApi 根 API）
params.panels           = DockviewPanel[]
params.activePanel      = DockviewPanel | undefined
params.location         = { type: 'grid' | 'edge' | 'floating', position?: string }
```

### 获取根 DockviewApi

HeaderActions 中 `params.api` 和 `params.containerApi` 都是 `DockviewGroupPanelApi`，**不包含** `addFloatingGroup()`。

获取根 `DockviewApi`（用于浮动/弹出）：

```typescript
const dApi = params.group.model.accessor.api
dApi.addFloatingGroup(group) // 浮动整组
dApi.addFloatingGroup(panel) // 浮动单个 tab
dApi.addPopoutGroup(group) // 弹出整组窗口
dApi.addPopoutGroup(panel) // 弹出单个 tab 窗口
```

### Context Menu API 区别

右键菜单 callback 中的 `params.api` **已经是 `DockviewApi`（根 API）**，可直接使用：

```typescript
getTabContextMenuItems(params) {
  params.api.addFloatingGroup(params.group)   // ✅ 直接可用
  params.api.addPopoutGroup(params.panel)     // ✅ 直接可用
}
```

### 钉住 panel 实现

钉住是前端行为，通过 `Set<string>` 记录 panel ID：

```typescript
const pinnedPanelIds = new Set<string>()
pinnedPanelIds.add(panel.id) // 钉住
pinnedPanelIds.delete(panel.id) // 取消
```

### 最大化

`api.maximize()` 仅在 `location.type === 'grid'` 时生效。Edge Group（侧边栏）不支持最大化。

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
- 接口 `IXxx` 格式
- 枚举 PascalCase
- 全局类型放入 `types/`
- 所有请求/状态必须标注类型

---

# 7. 业务分层规范（核心架构）

| 层级 | 职责         | 位置           |
| ---- | ------------ | -------------- |
| 页面 | 只做展示     | `views/`       |
| 逻辑 | 业务 hooks   | `composables/` |
| 状态 | Pinia stores | `stores/`      |
| 请求 | API 层       | `api/`         |
| 工具 | 工具函数     | `utils/`       |
| UI   | 组件         | `components/`  |

**禁止在页面写请求，禁止在组件写复杂业务。**

---

# 8. 数据库工作台专属规则

## 8.1 SQL 编辑器

- Monaco 主题统一
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

# 11. 设计系统关键原则（AI 强制执行）

以下原则适用于所有前端代码生成，必须严格遵守：

1. **所有颜色必须使用 CSS 变量**，禁止硬编码十六进制色值或 RGB 值；禁止使用 white、black、gray 等颜色关键字
2. **所有间距只从 `--spacing-xs/sm/md/lg` 中选择**；所有圆角只从 `--border-radius-sm/md` 中选择；所有字号只从 `--font-size-sm/md/lg` 中选择
3. **新颜色需求必须先在 `tokens.css` 中定义**，并同时为暗色和亮色模式提供对应值
4. **所有用户可见文本必须使用 `$t('key.subkey')` 或 `useI18n().t()`** 引用翻译键，禁止硬编码中英文字符串
5. **新页面需要同时在 `zh-CN.json` 和 `en.json` 中提供对应的翻译键对**
6. **暗色（`.theme-dark`）和亮色（`.theme-light`）必须都正常显示**，禁止只为一种主题写样式
7. **主题和语言的切换必须通过 Pinia `useAppStore` 进行**，禁止直接从组件中调用底层 Store API
8. **禁止在组件 `<style>` 中使用 `:root {}` 覆盖全局 CSS 变量**
9. **禁止对 `--color-*` 或 `--brand-*` 开头的全局 CSS 变量做本地覆盖**
10. **组件销毁时必须清理事件监听和定时器**

---

# 12. 常用代码模板

## 12.1 组件模板

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

## 12.2 Composable 模板

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

## 12.3 Store 模板

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
