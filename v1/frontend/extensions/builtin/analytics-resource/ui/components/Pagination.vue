<template>
  <div class="pagination">
    <div class="pagination-info">
      <span>{{ t('analyticsResource.totalItems', { total }) }}</span>
      <select v-model="localPageSize" class="page-size-select" @change="handlePageSizeChange">
        <option :value="10">{{ t('analyticsResource.perPage', { size: 10 }) }}</option>
        <option :value="20">{{ t('analyticsResource.perPage', { size: 20 }) }}</option>
        <option :value="50">{{ t('analyticsResource.perPage', { size: 50 }) }}</option>
        <option :value="100">{{ t('analyticsResource.perPage', { size: 100 }) }}</option>
      </select>
    </div>

    <div class="pagination-controls">
      <button class="page-btn" :disabled="page <= 1" @click="emit('prev')"> ‹ </button>

      <template v-for="p in visiblePages" :key="p">
        <span v-if="p === '...'" class="page-ellipsis">...</span>
        <button v-else :class="['page-btn', { active: p === page }]" @click="goToPage(p as number)">
          {{ p }}
        </button>
      </template>

      <button class="page-btn" :disabled="page >= totalPages" @click="emit('next')"> › </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const props = withDefaults(
  defineProps<{
    page: number
    pageSize: number
    total: number
    totalPages: number
  }>(),
  {
    page: 1,
    pageSize: 20,
    total: 0,
    totalPages: 1,
  }
)

const emit = defineEmits<{
  'update:page': [page: number]
  'update:pageSize': [size: number]
  prev: []
  next: []
}>()

const localPageSize = ref(props.pageSize)

watch(localPageSize, newSize => {
  emit('update:pageSize', newSize)
})

function handlePageSizeChange() {
  emit('update:page', 1)
}

function goToPage(p: number) {
  if (p >= 1 && p <= props.totalPages) {
    emit('update:page', p)
  }
}

const visiblePages = computed<(number | string)[]>(() => {
  const pages: (number | string)[] = []
  const total = props.totalPages
  const current = props.page

  if (total <= 7) {
    for (let i = 1; i <= total; i++) {
      pages.push(i)
    }
  } else {
    pages.push(1)

    if (current > 3) {
      pages.push('...')
    }

    const start = Math.max(2, current - 1)
    const end = Math.min(total - 1, current + 1)

    for (let i = start; i <= end; i++) {
      pages.push(i)
    }

    if (current < total - 2) {
      pages.push('...')
    }

    pages.push(total)
  }

  return pages
})
</script>

<style scoped>
.pagination {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--size-md) var(--size-lg);
  border-top: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.pagination-info {
  display: flex;
  align-items: center;
  gap: var(--size-md);
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
}

.page-size-select {
  padding: 4px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: var(--font-size-sm);
  cursor: pointer;
}

.pagination-controls {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.page-btn {
  min-width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition: all 0.15s;
}

.page-btn:hover:not(:disabled) {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.page-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.page-btn.active {
  background: var(--primary-color);
  border-color: var(--primary-color);
  color: white;
}

.page-ellipsis {
  padding: 0 4px;
  color: var(--text-tertiary);
}
</style>
