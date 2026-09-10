# 前端开发指南

> 版本：v1.1
> 最后更新：2026-05-11
> 本文档面向 RdataStation 前端开发者，提供快速上手指南。

---

## 环境准备

### 1. 安装依赖

```bash
pnpm install
```

### 2. 启动开发服务器

```bash
pnpm tauri dev
```

### 3. 类型检查

```bash
pnpm run typecheck
```

### 4. 代码规范检查

```bash
pnpm run lint
pnpm run format
```

---

## 项目结构速览

```
src/
├── app/              # 应用入口
├── extensions/       # 插件系统（核心）
│   ├── core/         # 扩展系统核心
│   └── builtin/      # 内置插件
├── shared/           # 共享资源
└── core/             # 核心业务
```

---

## 创建新插件

### 1. 创建插件目录

```bash
mkdir -p src/extensions/builtin/my-plugin/{domain,infrastructure,ui,types}
```

### 2. 创建插件入口文件

```typescript
// src/extensions/builtin/my-plugin/extension.ts
import type { ExtensionContext, ExtensionAPI, ExtensionModule } from '../../core/types'

const activate = (context: ExtensionContext): ExtensionAPI => {
  console.log('MyPlugin activated')

  // 注册命令
  context.commands.register('myPlugin.hello', () => {
    console.log('Hello from MyPlugin!')
  })

  // 订阅事件
  context.events.on('connection:changed', data => {
    console.log('Connection changed:', data)
  })

  return {
    // 暴露 API 给其他插件
    sayHello: () => console.log('Hello!'),
  }
}

const deactivate = (): void => {
  console.log('MyPlugin deactivated')
}

const extension: ExtensionModule = {
  activate,
  deactivate,
}

export default extension
```

### 3. 注册插件

```typescript
// src/extensions/index.ts
import myPlugin from './builtin/my-plugin/extension'

export const builtinExtensions = [
  // ... 其他插件
  myPlugin,
]
```

---

## 创建新组件

### 1. 在插件 UI 目录创建组件

```vue
<!-- src/extensions/builtin/my-plugin/ui/components/MyComponent.vue -->
<template>
  <div class="my-component">
    <h3>{{ title }}</h3>
    <slot />
  </div>
</template>

<script setup lang="ts">
interface Props {
  title: string
}

const props = defineProps<Props>()
</script>

<style scoped>
.my-component {
  padding: 16px;
}
</style>
```

### 2. 使用组件

```vue
<template>
  <MyComponent title="Hello World">
    <p>Content here</p>
  </MyComponent>
</template>

<script setup lang="ts">
import MyComponent from './MyComponent.vue'
</script>
```

---

## 调用后端 API

### 方式 1：使用统一 API 层（推荐）

```typescript
import { connectionApi } from '@/shared/api'

const result = await connectionApi.connectDatabase('mysql', url)
```

### 方式 2：直接调用（不推荐）

```typescript
import { invoke } from '@tauri-apps/api/core'

const result = await invoke('connect_database', { db_type: 'mysql', url })
```

---

## 错误处理

### 使用 Result 类型

```typescript
import { safeAsync } from '@/shared/utils/error'

const result = await safeAsync(() => api.call())

if (result.ok) {
  // 成功处理
  console.log(result.value)
} else {
  // 错误处理
  console.error(result.error.getUserMessage())
}
```

### 抛出 AppError

```typescript
import { AppError, ErrorCode } from '@/shared/utils/error'

if (!connection) {
  throw new AppError(ErrorCode.CONNECTION_FAILED, '连接不存在')
}
```

---

## 插件间通信

### 发布事件

```typescript
context.events.emit('myPlugin:dataLoaded', { data: '...' })
```

### 订阅事件

```typescript
context.events.on('myPlugin:dataLoaded', data => {
  console.log('Data loaded:', data)
})
```

---

## 命名规范

| 类型      | 规范             | 示例              |
| --------- | ---------------- | ----------------- |
| 文件      | kebab-case       | `my-component.ts` |
| 组件      | PascalCase       | `MyComponent.vue` |
| 变量/函数 | camelCase        | `getData()`       |
| 常量      | UPPER_SNAKE_CASE | `MAX_COUNT`       |

---

## 常见问题

### Q: 如何访问其他插件的状态？

**A**: 通过事件总线通信，禁止直接引用其他插件的 store。

```typescript
// ❌ 错误
import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'

// ✅ 正确
context.events.on('connection:changed', handler)
```

### Q: 如何添加全局类型？

**A**: 在 `shared/types/index.ts` 中添加。

### Q: 如何添加插件专用类型？

**A**: 在插件的 `types/index.ts` 中添加。

---

## 参考资料

- [架构文档](./ARCHITECTURE.md)
- [组件规范](./COMPONENTS.md)
- [插件架构文档](./PLUGIN-ARCHITECTURE-REFACTOR.md)
- [配置系统](./CONFIG-SYSTEM.md)
- [联调测试方案](../project-integration-test-plan.md)
