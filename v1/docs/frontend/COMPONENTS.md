# 前端组件规范

> 本文档定义 RdataStation 前端组件开发规范。

---

## 组件分类

### 1. 基础组件（shared/components/common/）

通用、可复用的基础 UI 组件，不包含业务逻辑。

**示例**：

- `BaseButton.vue`
- `BaseModal.vue`
- `DbIcon.vue`
- `LoadingOverlay.vue`

**规范**：

```vue
<template>
  <button :class="['base-button', variant, sizeClass]" :disabled="disabled">
    <slot />
  </button>
</template>

<script setup lang="ts">
interface Props {
  variant?: 'primary' | 'secondary' | 'danger'
  size?: 'small' | 'medium' | 'large'
  disabled?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  variant: 'primary',
  size: 'medium',
  disabled: false,
})

const sizeClass = computed(() => `base-button--${props.size}`)
</script>
```

### 2. 业务组件（extensions/builtin/\*/ui/components/）

包含特定业务逻辑的组件，属于具体插件。

**示例**：

- `DatabaseNavigatorTreeReal.vue`
- `TableStructurePanel.vue`
- `QueryResultGrid.vue`

**规范**：

```vue
<template>
  <div class="database-navigator">
    <NavigatorNode v-for="node in nodes" :key="node.id" :node="node" @select="handleSelect" />
  </div>
</template>

<script setup lang="ts">
import { useNavigator } from '../composables/useNavigator'
import NavigatorNode from './NavigatorNode.vue'

import type { NavigatorNode } from '../../types'

interface Props {
  connectionId: string
}

const props = defineProps<Props>()

const { nodes, handleSelect } = useNavigator(props.connectionId)
</script>
```

---

## 组件结构

### 标准模板

```vue
<template>
  <!-- 模板内容 -->
</template>

<script setup lang="ts">
// 1. 导入依赖（按顺序：Vue → 第三方 → 共享 → 插件内部 → 类型）
import { ref, computed, onMounted } from 'vue'
import { NButton } from 'naive-ui'
import { connectionApi } from '@/shared/api'
import { useLocalState } from '../composables/useLocalState'
import type { Connection } from '@/shared/types'

// 2. Props 定义
interface Props {
  title: string
  data?: Connection[]
}

const props = withDefaults(defineProps<Props>(), {
  data: () => [],
})

// 3. Emits 定义
interface Emits {
  (e: 'update', value: string): void
  (e: 'select', item: Connection): void
}

const emit = defineEmits<Emits>()

// 4. 响应式状态
const loading = ref(false)
const error = ref<string | null>(null)

// 5. 计算属性
const hasData = computed(() => props.data.length > 0)

// 6. 方法
async function loadData() {
  loading.value = true
  try {
    // 加载逻辑
  } finally {
    loading.value = false
  }
}

// 7. 生命周期
onMounted(() => {
  loadData()
})
</script>

<style scoped>
/* 样式 */
.component-name {
  padding: 16px;
}
</style>
```

---

## Props 规范

### 使用 TypeScript 接口

```typescript
// ✅ 推荐
interface Props {
  title: string
  count?: number
  items: Connection[]
  onClick?: (item: Connection) => void
}

const props = withDefaults(defineProps<Props>(), {
  count: 0,
  onClick: undefined,
})
```

### Props 命名

- 使用 camelCase
- 布尔值使用 `is`、`has`、`show` 前缀
- 回调函数使用 `on` 前缀

```typescript
interface Props {
  isVisible: boolean
  hasError: boolean
  showLoading: boolean
  onSelect: (id: string) => void
  onError: (error: Error) => void
}
```

---

## Emits 规范

### 使用 TypeScript 接口

```typescript
// ✅ 推荐
interface Emits {
  (e: 'update', value: string): void
  (e: 'select', item: Connection): void
  (e: 'error', error: Error): void
}

const emit = defineEmits<Emits>()

// 使用
emit('select', item)
```

### 事件命名

- 使用 kebab-case（模板中）
- 使用 camelCase（脚本中）
- 使用现在时动词

```vue
<template>
  <button @click="emit('select', item)">选择</button>
</template>

<script setup>
emit('select', item)
emit('update:modelValue', newValue)
</script>
```

---

## 样式规范

### 使用 scoped 样式

```vue
<style scoped>
.component-name {
  /* 组件样式 */
}
</style>
```

### 使用 CSS 变量

```vue
<style scoped>
.component-name {
  --primary-color: var(--n-primary-color);
  --padding: 16px;

  padding: var(--padding);
  color: var(--primary-color);
}
</style>
```

### 使用 naive-ui 主题变量

```vue
<style scoped>
.button {
  background: var(--n-color);
  color: var(--n-text-color);
  border-radius: var(--n-border-radius);
}
</style>
```

---

## 状态管理

### 组件内部状态

```vue
<script setup lang="ts">
// 使用 ref/reactive
const loading = ref(false)
const data = ref<Connection[]>([])
const form = reactive({
  name: '',
  host: '',
  port: 3306,
})
</script>
```

### 共享状态

```vue
<script setup lang="ts">
// 使用 Pinia store
import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'

const connectionStore = useConnectionStore()

// 访问状态
const connections = computed(() => connectionStore.connections)

// 调用 action
function connect() {
  connectionStore.connect(url)
}
</script>
```

---

## 组合式函数（Composables）

### 命名规范

- 文件名：`use-*.ts`（kebab-case）
- 函数名：`use*`（camelCase，PascalCase 开头）

### 标准模板

```typescript
// use-connection.ts
import { ref, computed } from 'vue'
import { connectionApi } from '@/shared/api'
import { safeAsync } from '@/shared/utils/error'

import type { Connection } from '@/shared/types'

export function useConnection(connId: string) {
  const connection = ref<Connection | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  const isConnected = computed(() => connection.value?.status === 'connected')

  async function loadConnection() {
    loading.value = true
    error.value = null

    const result = await safeAsync(() => connectionApi.getConnectionInfo(connId))

    if (result.ok) {
      connection.value = result.value
    } else {
      error.value = result.error.getUserMessage()
    }

    loading.value = false
  }

  return {
    connection,
    loading,
    error,
    isConnected,
    loadConnection,
  }
}
```

---

## 图标使用

### 使用 lucide-vue-next

```vue
<script setup lang="ts">
import { Database, Plus, Loader2, AlertCircle } from 'lucide-vue-next'
</script>

<template>
  <Database :size="16" />
  <Plus :size="14" />
  <Loader2 :size="24" class="spin" />
  <AlertCircle :size="32" />
</template>
```

### 数据库图标

```vue
<script setup lang="ts">
import DbIcon from '@/shared/components/common/DbIcon.vue'
</script>

<template>
  <DbIcon type="mysql" :size="16" />
  <DbIcon type="postgresql" :size="16" />
  <DbIcon type="sqlite" :size="16" />
</template>
```

---

## 性能优化

### 使用 v-memo

```vue
<template>
  <div v-memo="[item.id, item.name]" v-for="item in items" :key="item.id">
    {{ item.name }}
  </div>
</template>
```

### 使用 computed 缓存

```vue
<script setup lang="ts">
const filteredItems = computed(() => {
  return items.value.filter(item => item.isActive)
})
</script>
```

### 虚拟滚动（AG Grid）

```vue
<template>
  <AgGridVue
    :rowData="rowData"
    :columnDefs="columnDefs"
    :domLayout="'normal'"
    :enableRangeSelection="true"
    row-model-type="clientSide"
  />
</template>
```

---

## 测试

### 组件测试

```typescript
// MyComponent.test.ts
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import MyComponent from './MyComponent.vue'

describe('MyComponent', () => {
  it('renders correctly', () => {
    const wrapper = mount(MyComponent, {
      props: {
        title: 'Test',
      },
    })

    expect(wrapper.text()).toContain('Test')
  })
})
```

---

## 代码审查清单

- [ ] 使用 `<script setup lang="ts">`
- [ ] Props 使用 TypeScript 接口
- [ ] Emits 使用 TypeScript 接口
- [ ] 使用 scoped 样式
- [ ] 业务逻辑在 composables 中
- [ ] 错误处理使用 Result 类型
- [ ] 图标使用 lucide-vue-next
- [ ] 组件使用 PascalCase 命名
- [ ] 文件使用 kebab-case 命名
- [ ] 无 `any` 类型
- [ ] 无直接引用其他插件 store
