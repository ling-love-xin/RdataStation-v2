<template>
  <div v-if="isWarming || showMetrics" class="cache-warming-status">
    <div v-if="isWarming" class="warming-progress">
      <Loader2 :size="12" class="spinner" />
      <span class="status-text">缓存预热中 {{ progress.toFixed(0) }}%</span>
      <NButton text size="tiny" class="cancel-btn" @click="handleCancel"> 取消 </NButton>
    </div>
    <div v-if="showMetrics" class="cache-metrics">
      <span class="metric">
        <Database :size="12" />
        命中率: {{ hitRate }}%
      </span>
      <span class="metric">
        <Clock :size="12" />
        延迟: {{ avgLatency }}ms
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Database, Loader2, Clock } from 'lucide-vue-next'
import { NButton } from 'naive-ui'
import { computed } from 'vue'

import { cacheMetricsManager } from '../composables/use-cache-metrics'
import { useCacheWarming } from '../composables/use-cache-warming'

const { state: warmingState, cancelWarming } = useCacheWarming()

const isWarming = computed(() => warmingState.value.isWarming)
const progress = computed(() => warmingState.value.progress)

const showMetrics = computed(() => {
  const metrics = cacheMetricsManager.getMetrics()
  return metrics.totalOperations > 0
})

const hitRate = computed(() => {
  const metrics = cacheMetricsManager.getMetrics()
  return (metrics.hitRate * 100).toFixed(1)
})

const avgLatency = computed(() => {
  const metrics = cacheMetricsManager.getMetrics()
  return metrics.avgLatency.toFixed(1)
})

function handleCancel() {
  cancelWarming()
}
</script>

<style scoped>
.cache-warming-status {
  display: flex;
  align-items: center;
  gap: 12px;
}

.warming-progress {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 8px;
  background: rgba(255, 255, 255, 0.15);
  border-radius: 3px;
}

.spinner {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.status-text {
  font-size: 12px;
  color: white;
  white-space: nowrap;
}

.cancel-btn {
  color: white;
  font-size: 11px;
  padding: 0 4px;
  height: auto;
}

.cancel-btn:hover {
  background: rgba(255, 255, 255, 0.2);
}

.cache-metrics {
  display: flex;
  align-items: center;
  gap: 12px;
}

.metric {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: white;
  white-space: nowrap;
}
</style>
