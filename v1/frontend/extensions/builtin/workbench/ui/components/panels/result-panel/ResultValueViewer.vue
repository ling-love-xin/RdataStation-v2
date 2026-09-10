<template>
  <div v-if="visible" class="value-viewer-sidebar">
    <div class="viewer-header">
      <span class="header-label">{{ column ? 'Value: ' + column : 'Cell Value' }}</span>
      <span v-if="rowIndex >= 0" class="row-info">Row {{ rowIndex + 1 }}</span>
      <NButton size="tiny" quaternary @click="$emit('close')">
        <X :size="12" />
      </NButton>
    </div>
    <div class="viewer-body">
      <div v-if="isNull" class="null-label">NULL</div>
      <pre v-else class="value-display" :class="cssClass">{{ displayText }}</pre>
    </div>
    <div class="viewer-footer">
      <NButton size="tiny" quaternary @click="copy">
        <Clipboard :size="12" />
        <span>Copy</span>
      </NButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import { X, Clipboard } from 'lucide-vue-next'
import { NButton } from 'naive-ui'
import { computed } from 'vue'

const props = defineProps<{
  visible: boolean
  value: unknown
  column: string
  rowIndex: number
  dataType?: string
}>()

defineEmits<{
  close: []
}>()

const isNull = computed(() => props.value === null || props.value === undefined)
const isObject = computed(() => typeof props.value === 'object' && props.value !== null)

const displayText = computed(() => {
  if (props.value === null || props.value === undefined) return 'NULL'
  if (isObject.value) {
    try {
      return JSON.stringify(props.value, null, 2)
    } catch {
      return String(props.value)
    }
  }
  return String(props.value)
})

const cssClass = computed(() => {
  if (isObject.value) return 'is-json'
  if (typeof props.value === 'string' && props.value.length > 200) return 'is-long-text'
  return ''
})

function copy() {
  navigator.clipboard.writeText(displayText.value)
}
</script>

<style scoped>
.value-viewer-sidebar {
  width: 300px;
  border-left: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  background: var(--panel-bg);
}
.viewer-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
}
.header-label {
  font-weight: 600;
  font-size: 12px;
}
.row-info {
  margin-left: auto;
  font-size: 11px;
  color: var(--text-color-secondary);
}
.viewer-body {
  flex: 1;
  overflow: auto;
  padding: 12px;
}
.null-label {
  font-style: italic;
  color: #999;
}
.value-display {
  margin: 0;
  font-family: 'Cascadia Code', 'Fira Code', monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
}
.is-json {
  color: #d19a66;
}
.is-long-text {
  font-size: 11px;
}
.viewer-footer {
  padding: 8px 12px;
  border-top: 1px solid var(--border-color);
  display: flex;
  justify-content: flex-end;
}
</style>
