<template>
  <div class="preview-pagination">
    <button class="page-btn" :disabled="currentPage <= 1" @click="$emit('change', currentPage - 1)">
      <ChevronLeft :size="16" />
    </button>

    <span class="page-info"> 第 {{ currentPage }} / {{ totalPages }} 页 </span>

    <button
      class="page-btn"
      :disabled="currentPage >= totalPages"
      @click="$emit('change', currentPage + 1)"
    >
      <ChevronRight :size="16" />
    </button>

    <select class="page-size-select" :value="pageSize" @change="$emit('change', 1)">
      <option :value="50">50 行/页</option>
      <option :value="100">100 行/页</option>
      <option :value="200">200 行/页</option>
      <option :value="500">500 行/页</option>
    </select>
  </div>
</template>

<script setup lang="ts">
import { ChevronLeft, ChevronRight } from 'lucide-vue-next'

interface Props {
  currentPage: number
  totalPages: number
  pageSize: number
}

defineProps<Props>()

defineEmits<{
  change: [page: number]
}>()
</script>

<style scoped>
.preview-pagination {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border-top: 1px solid var(--border-color);
}

.page-btn {
  width: 28px;
  height: 28px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--bg-primary);
  color: var(--text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}

.page-btn:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.page-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.page-info {
  font-size: 12px;
  color: var(--text-secondary);
}

.page-size-select {
  padding: 4px 8px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 12px;
  cursor: pointer;
}
</style>
