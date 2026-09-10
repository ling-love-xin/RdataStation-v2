<template>
  <div class="preview-toolbar">
    <div class="toolbar-left">
      <span class="table-name">{{ schemaName }}.{{ tableName }}</span>
      <span class="row-count">{{ rowCount }} 行</span>
    </div>

    <div class="toolbar-center">
      <button class="toolbar-btn" title="刷新" @click="$emit('refresh')">
        <RefreshCw :size="16" />
      </button>
      <button class="toolbar-btn" title="导出数据" @click="$emit('export')">
        <Download :size="16" />
      </button>
      <button class="toolbar-btn" title="复制数据" @click="$emit('copy')">
        <Copy :size="16" />
      </button>
    </div>

    <div class="toolbar-right">
      <button class="toolbar-btn" title="关闭" @click="$emit('close')">
        <X :size="16" />
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { RefreshCw, Download, Copy, X } from 'lucide-vue-next'

interface Props {
  connectionId: string
  schemaName: string
  tableName: string
  isLoading: boolean
  rowCount: number
  currentPage: number
  totalPages: number
}

defineProps<Props>()

defineEmits<{
  refresh: []
  export: []
  copy: []
  pageChange: [page: number]
  close: []
}>()
</script>

<style scoped>
.preview-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
}

.toolbar-left,
.toolbar-center,
.toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.table-name {
  font-weight: 500;
  font-size: 13px;
  color: var(--text-primary);
}

.row-count {
  font-size: 12px;
  color: var(--text-secondary);
}

.toolbar-btn {
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s;
}

.toolbar-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.toolbar-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
