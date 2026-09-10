<template>
  <div class="right-header">
    <!-- Row 1: Name + Scope -->
    <div class="rh-row">
      <span class="rh-label">{{ nameLabel }}</span>
      <NInput
        :value="name"
        :placeholder="namePlaceholder"
        size="small"
        class="rh-name-input"
        @update:value="(v: string) => emit('update:name', v)"
      />
      <NCheckbox
        :checked="scopeGlobal"
        size="small"
        @update:checked="(v: boolean) => emit('update:scopeGlobal', v)"
      >
        {{ globalLabel }}
      </NCheckbox>
      <NCheckbox
        :checked="scopeProject"
        size="small"
        @update:checked="(v: boolean) => emit('update:scopeProject', v)"
      >
        {{ projectLabel }}
      </NCheckbox>
    </div>
    <!-- Row 2: Description standalone -->
    <div class="rh-row">
      <span class="rh-label">{{ descLabel }}</span>
      <NInput
        :value="description"
        type="textarea"
        :placeholder="descPlaceholder"
        size="small"
        :rows="2"
        class="rh-desc-input"
        @update:value="(v: string) => emit('update:description', v)"
      />
    </div>
    <!-- Row 3: Driver + URI -->
    <div class="rh-row uri-row">
      <span class="rh-label">{{ driverLabel }}</span>
      <NSelect
        :value="selectedDriverId"
        :options="driverOptions"
        :placeholder="driverPlaceholder"
        size="small"
        class="rh-driver-select"
        @update:value="onDriverSelect"
      />
      <span class="uri-label">{{ uriLabel }}</span>
      <NInput
        v-if="uriEditing"
        :value="manualUri"
        size="small"
        class="uri-edit-input"
        :placeholder="uriPlaceholder"
        @update:value="(v: string) => emit('update:manualUri', v)"
      />
      <div v-else class="uri-display">{{ uriPreview || '—' }}</div>
      <NButton
        size="tiny"
        quaternary
        :type="uriEditing ? 'primary' : 'default'"
        @click="emit('update:uriEditing', !uriEditing)"
      >
        <template #icon><Edit :size="13" /></template>
      </NButton>
      <NButton
        v-if="uriEditing && manualUri"
        size="tiny"
        quaternary
        type="primary"
        @click="emit('parseUrl', manualUri)"
      >
        {{ t('navigator.parseUrl') }}
      </NButton>
    </div>
    <NAlert v-if="uriWarning" type="warning" :title="uriWarning" closable style="margin-top: 8px" />
  </div>
</template>

<script setup lang="ts">
import { Edit } from 'lucide-vue-next'
import { NAlert, NButton, NCheckbox, NInput, NSelect } from 'naive-ui'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { SelectOption } from 'naive-ui'

interface Props {
  name: string
  description: string
  scopeGlobal: boolean
  scopeProject: boolean
  selectedDriverId: string | null
  driverOptions: SelectOption[]
  uriPreview: string
  uriEditing: boolean
  manualUri: string
  nameLabel: string
  namePlaceholder: string
  descLabel: string
  descPlaceholder: string
  globalLabel: string
  projectLabel: string
  driverLabel: string
  driverPlaceholder: string
  uriLabel: string
  uriPlaceholder: string
  urlTemplate?: string | null
}

const props = withDefaults(defineProps<Props>(), {
  name: '',
  description: '',
  scopeGlobal: true,
  scopeProject: false,
  selectedDriverId: null,
  driverOptions: () => [],
  uriPreview: '',
  uriEditing: false,
  manualUri: '',
  nameLabel: '',
  namePlaceholder: '',
  descLabel: '',
  descPlaceholder: '',
  globalLabel: '',
  projectLabel: '',
  driverLabel: '',
  driverPlaceholder: '',
  uriLabel: 'URI',
  uriPlaceholder: 'jdbc:mysql://...',
  urlTemplate: null,
})

const emit = defineEmits<{
  (e: 'update:name', v: string): void
  (e: 'update:description', v: string): void
  (e: 'update:scopeGlobal', v: boolean): void
  (e: 'update:scopeProject', v: boolean): void
  (e: 'update:selectedDriverId', v: string): void
  (e: 'update:uriEditing', v: boolean): void
  (e: 'update:manualUri', v: string): void
  (e: 'parseUrl', url: string): void
  (e: 'driver-change', driverId: string): void
}>()

const { t } = useI18n()

function onDriverSelect(value: string | number | null) {
  const driverId = value as string
  emit('update:selectedDriverId', driverId)
  emit('driver-change', driverId)
}

const uriWarning = computed<string | null>(() => {
  if (!props.uriEditing) return null
  const tmpl = props.urlTemplate
  if (!tmpl) return null
  const uri = props.manualUri.trim()
  if (!uri) return null

  // 提取 url_template 中的协议前缀（如 mysql、postgres、sqlite）
  const schemeMatch = tmpl.match(/^(\w+):\/\//)
  const expectedScheme = schemeMatch ? schemeMatch[1].toLowerCase() : null

  // 检查 URL 是否包含 ://
  if (!uri.includes('://')) {
    return expectedScheme
      ? `URL 格式不匹配：缺少 "://" 分隔符，期望格式以 "${expectedScheme}://" 开头`
      : 'URL 格式不匹配：缺少 "://" 分隔符'
  }

  // 检查协议是否匹配
  if (expectedScheme) {
    const actualScheme = uri.split('://')[0].toLowerCase()
    if (actualScheme !== expectedScheme) {
      return `URL 协议不匹配：当前为 "${actualScheme}://"，驱动期望 "${expectedScheme}://"`
    }
  }

  // 对网络数据库检查 host:port 段
  if (tmpl.includes('{host}') && tmpl.includes('{port}')) {
    const afterScheme = uri.substring(uri.indexOf('://') + 3)
    // 去除认证信息部分 (@ 之前)
    const hostPart = afterScheme.includes('@') ? afterScheme.split('@')[1] : afterScheme
    const hostPortMatch = hostPart.match(/^[^/:]+(:\d+)?/)
    if (!hostPortMatch) {
      return 'URL 格式不匹配：缺少有效的 host:port 段'
    }
  }

  return null
})
</script>

<style scoped>
.right-header {
  padding: 8px var(--spacing-md) var(--spacing-xs);
  border-bottom: 1px solid var(--color-border-subtle);
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex-shrink: 0;
}

.rh-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
}

.rh-label {
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-muted);
  width: 48px;
  flex-shrink: 0;
  text-align: right;
}

.rh-name-input {
  flex: 1;
  max-width: 280px;
}
.rh-desc-input {
  flex: 1;
}

.rh-driver-select {
  flex: 0 0 200px;
}

/* URI row */
.uri-row {
  gap: var(--spacing-xs);
}
.uri-label {
  font-size: 11px;
  color: var(--color-text-muted);
  flex-shrink: 0;
  padding: 0 2px;
}

.uri-display {
  flex: 1;
  height: 28px;
  padding: 0 10px;
  font-size: 11px;
  font-family: 'JetBrains Mono', monospace;
  color: var(--brand-success);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-sm);
  display: flex;
  align-items: center;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  min-width: 0;
}

.uri-edit-input {
  flex: 1;
  min-width: 0;
}
.uri-edit-input :deep(.n-input__input) {
  font-family: 'JetBrains Mono', monospace;
  font-size: 11px;
}
</style>
