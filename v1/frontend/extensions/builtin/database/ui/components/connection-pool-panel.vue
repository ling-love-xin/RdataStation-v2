<template>
  <div class="connection-pool-panel">
    <div class="panel-header">
      <h3>连接池状态</h3>
      <button class="btn-refresh" @click="refreshStatus">
        <RefreshCw :size="14" />
      </button>
    </div>

    <div v-if="loading" class="loading">
      <Loader2 :size="20" class="loader" />
      <span>加载中...</span>
    </div>

    <div v-else-if="error" class="error">
      <AlertCircle :size="16" />
      <span>{{ error }}</span>
    </div>

    <div v-else class="pool-content">
      <div class="stats-grid">
        <div class="stat-item">
          <div class="stat-icon active">
            <Users :size="20" />
          </div>
          <div class="stat-info">
            <span class="stat-value">{{ status.activeConnections }}</span>
            <span class="stat-label">活跃连接</span>
          </div>
        </div>

        <div class="stat-item">
          <div class="stat-icon idle">
            <Clock :size="20" />
          </div>
          <div class="stat-info">
            <span class="stat-value">{{ status.idleConnections }}</span>
            <span class="stat-label">空闲连接</span>
          </div>
        </div>

        <div class="stat-item">
          <div class="stat-icon total">
            <Layers :size="20" />
          </div>
          <div class="stat-info">
            <span class="stat-value"
              >{{ status.totalConnections }} / {{ status.maxConnections }}</span
            >
            <span class="stat-label">总连接数</span>
          </div>
        </div>

        <div class="stat-item">
          <div class="stat-icon queue">
            <ListOrdered :size="20" />
          </div>
          <div class="stat-info">
            <span class="stat-value">{{ status.waitQueueSize }}</span>
            <span class="stat-label">等待队列</span>
          </div>
        </div>
      </div>

      <div class="progress-section">
        <div class="progress-header">
          <span>连接使用率</span>
          <span class="progress-value">{{ usagePercent }}%</span>
        </div>
        <div class="progress-bar">
          <div class="progress-fill" :style="{ width: usagePercent + '%' }"></div>
        </div>
        <div class="progress-labels">
          <span>0</span>
          <span>{{ status.maxConnections }}</span>
        </div>
      </div>

      <div class="config-section">
        <h4>连接池配置</h4>
        <div class="config-grid">
          <div class="config-item">
            <span class="config-label">最小连接数</span>
            <span class="config-value">{{ status.minConnections }}</span>
          </div>
          <div class="config-item">
            <span class="config-label">最大连接数</span>
            <span class="config-value">{{ status.maxConnections }}</span>
          </div>
          <div class="config-item">
            <span class="config-label">连接超时</span>
            <span class="config-value">{{ formatTime(status.connectionTimeoutMs) }}</span>
          </div>
          <div class="config-item">
            <span class="config-label">空闲超时</span>
            <span class="config-value">{{ formatTime(status.idleTimeoutMs) }}</span>
          </div>
        </div>
      </div>

      <div class="action-section">
        <button class="btn-action" @click="increasePoolSize">
          <Plus :size="14" />
          增加连接数
        </button>
        <button class="btn-action secondary" @click="decreasePoolSize">
          <Minus :size="14" />
          减少连接数
        </button>
        <button class="btn-action danger" @click="clearIdleConnections">
          <Trash2 :size="14" />
          清理空闲连接
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import {
  RefreshCw,
  Loader2,
  AlertCircle,
  Users,
  Clock,
  Layers,
  ListOrdered,
  Plus,
  Minus,
  Trash2,
} from 'lucide-vue-next'
import { ref, computed, onMounted, onUnmounted } from 'vue'

interface ConnectionPoolStatus {
  connId: string
  activeConnections: number
  idleConnections: number
  maxConnections: number
  minConnections: number
  connectionTimeoutMs: number
  idleTimeoutMs: number
  totalConnections: number
  waitQueueSize: number
}

const props = defineProps<{
  connId: string
}>()

const loading = ref(false)
const error = ref<string | null>(null)
const status = ref<ConnectionPoolStatus>({
  connId: '',
  activeConnections: 0,
  idleConnections: 0,
  maxConnections: 10,
  minConnections: 2,
  connectionTimeoutMs: 30000,
  idleTimeoutMs: 300000,
  totalConnections: 0,
  waitQueueSize: 0,
})

const usagePercent = computed(() => {
  if (status.value.maxConnections === 0) return 0
  return Math.round((status.value.totalConnections / status.value.maxConnections) * 100)
})

function formatTime(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
  return `${Math.round(ms / 60000)}m`
}

async function refreshStatus() {
  loading.value = true
  error.value = null

  try {
    const result = await invoke<ConnectionPoolStatus>('get_connection_pool_status', {
      connId: props.connId,
    })
    status.value = result
  } catch (e) {
    error.value = e instanceof Error ? e.message : '获取连接池状态失败'
  } finally {
    loading.value = false
  }
}

function increasePoolSize() {}

function decreasePoolSize() {}

function clearIdleConnections() {}

let refreshInterval: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  refreshStatus()
  refreshInterval = setInterval(refreshStatus, 5000)
})

onUnmounted(() => {
  if (refreshInterval) {
    clearInterval(refreshInterval)
  }
})
</script>

<style scoped>
.connection-pool-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
}

.panel-header h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.btn-refresh {
  padding: 4px;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.btn-refresh:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.loading,
.error {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px;
  gap: 8px;
}

.loader {
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

.error {
  color: var(--error-color);
}

.pool-content {
  flex: 1;
  padding: 16px;
  overflow-y: auto;
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 12px;
  margin-bottom: 20px;
}

.stat-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px;
  background: var(--bg-secondary);
  border-radius: 8px;
}

.stat-icon {
  width: 40px;
  height: 40px;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.stat-icon.active {
  background: rgba(0, 180, 100, 0.15);
  color: #00b464;
}

.stat-icon.idle {
  background: rgba(100, 100, 255, 0.15);
  color: #6464ff;
}

.stat-icon.total {
  background: rgba(255, 150, 50, 0.15);
  color: #ff9632;
}

.stat-icon.queue {
  background: rgba(150, 100, 255, 0.15);
  color: #9664ff;
}

.stat-info {
  display: flex;
  flex-direction: column;
}

.stat-value {
  font-size: 18px;
  font-weight: 600;
  color: var(--text-primary);
}

.stat-label {
  font-size: 12px;
  color: var(--text-tertiary);
}

.progress-section {
  margin-bottom: 20px;
}

.progress-header {
  display: flex;
  justify-content: space-between;
  margin-bottom: 8px;
  font-size: 13px;
  color: var(--text-secondary);
}

.progress-value {
  font-weight: 600;
  color: var(--text-primary);
}

.progress-bar {
  height: 8px;
  background: var(--bg-tertiary);
  border-radius: 4px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: linear-gradient(90deg, #00b464, #00c853);
  border-radius: 4px;
  transition: width 0.3s ease;
}

.progress-labels {
  display: flex;
  justify-content: space-between;
  margin-top: 4px;
  font-size: 11px;
  color: var(--text-tertiary);
}

.config-section {
  margin-bottom: 20px;
}

.config-section h4 {
  margin: 0 0 12px 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.config-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 8px;
}

.config-item {
  display: flex;
  justify-content: space-between;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border-radius: 6px;
}

.config-label {
  font-size: 12px;
  color: var(--text-tertiary);
}

.config-value {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
}

.action-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.btn-action {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 10px;
  background: var(--primary-color);
  border: none;
  border-radius: 6px;
  font-size: 13px;
  color: white;
  cursor: pointer;
  transition: background-color 0.15s;
}

.btn-action:hover {
  background: var(--primary-dark);
}

.btn-action.secondary {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.btn-action.secondary:hover {
  background: var(--border-color);
}

.btn-action.danger {
  background: rgba(255, 100, 100, 0.15);
  color: var(--error-color);
}

.btn-action.danger:hover {
  background: rgba(255, 100, 100, 0.25);
}
</style>
