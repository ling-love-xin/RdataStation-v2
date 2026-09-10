<template>
  <div class="sql-history-panel">
    <!-- 头部工具栏 -->
    <div class="history-header">
      <h3 class="history-title">{{ t('sqlEditor.history') }}</h3>
      <div class="header-actions">
        <NButton size="small" quaternary :title="t('common.filter')" @click="toggleFilter">
          <template #icon>
            <Filter :size="14" />
          </template>
        </NButton>
        <NButton size="small" quaternary :title="t('common.refresh')" @click="refreshHistory">
          <template #icon>
            <RefreshCw :size="14" />
          </template>
        </NButton>
        <NButton
          size="small"
          quaternary
          :title="t('sqlHistory.clearHistory')"
          @click="clearAllHistory"
        >
          <template #icon>
            <Trash2 :size="14" />
          </template>
        </NButton>
      </div>
    </div>

    <!-- 搜索框 -->
    <div class="search-section">
      <NInput
        v-model:value="searchText"
        size="small"
        :placeholder="t('sqlHistory.searchHistory')"
        clearable
      >
        <template #prefix>
          <Search :size="14" />
        </template>
      </NInput>
    </div>

    <!-- 筛选面板 -->
    <div v-if="showFilter" class="filter-section">
      <NSelect
        v-model:value="filterConnection"
        size="small"
        :options="connectionOptions"
        :placeholder="t('sqlHistory.allConnections')"
        clearable
        style="width: 100%; margin-bottom: 8px"
      />
      <NSelect
        v-model:value="filterType"
        size="small"
        :options="typeOptions"
        :placeholder="t('sqlHistory.allTypes')"
        clearable
        style="width: 100%; margin-bottom: 8px"
      />
      <NSelect
        v-model:value="filterStatus"
        size="small"
        :options="statusOptions"
        :placeholder="t('sqlHistory.allStatuses')"
        clearable
        style="width: 100%"
      />
    </div>

    <!-- Tab 切换 -->
    <div class="tab-section">
      <NTabs v-model:value="activeTab" type="line" size="small">
        <NTab name="all" :tab="t('navigator.all')" />
        <NTab name="favorites" :tab="t('sqlHistory.favorites')" />
        <NTab name="recent" :tab="t('sqlHistory.recent')" />
      </NTabs>
    </div>

    <!-- 历史列表 -->
    <div class="history-list">
      <div v-if="filteredHistory.length === 0" class="empty-state">
        <NEmpty :description="t('sqlHistory.noHistory')" />
      </div>

      <div
        v-for="item in filteredHistory"
        :key="item.id"
        class="history-item"
        :class="{ 'is-favorite': item.isFavorite }"
        @click="selectHistory(item as SqlHistoryItem)"
        @dblclick="reExecuteHistory(item as SqlHistoryItem)"
      >
        <div class="item-header">
          <div class="item-info">
            <span class="item-connection">{{ getConnectionName(item.connId) }}</span>
            <span class="item-type">{{ item.dbType }}</span>
          </div>
          <div class="item-actions">
            <NButton size="small" quaternary circle @click.stop="handleToggleFavorite(item.id)">
              <template #icon>
                <Star
                  :size="14"
                  :fill="item.isFavorite ? 'var(--favorite-color)' : 'none'"
                  :color="item.isFavorite ? 'var(--favorite-color)' : 'currentColor'"
                />
              </template>
            </NButton>
            <NButton size="small" quaternary circle @click.stop="deleteHistoryItem(item.id)">
              <template #icon>
                <X :size="14" />
              </template>
            </NButton>
          </div>
        </div>

        <div class="item-sql">
          <pre>{{ truncateSql(item.sql, 100) }}</pre>
        </div>

        <div class="item-footer">
          <span class="item-time">{{ formatTime(item.executedAt) }}</span>
          <span class="item-status" :class="item.success ? 'success' : 'error'">
            {{ item.success ? '✓' : '✗' }}
          </span>
          <span v-if="item.rowsAffected && item.rowsAffected > 0" class="item-rows">
            {{ item.rowsAffected }} {{ t('sqlHistory.rows') }}
          </span>
          <span class="item-duration"> {{ item.durationMs }}ms </span>
        </div>
      </div>
    </div>

    <!-- 底部统计 -->
    <div class="history-footer">
      <span class="stat-item">
        {{ t('sqlHistory.totalCount', { count: filteredHistory.length }) }}
      </span>
      <span v-if="favoriteCount > 0" class="stat-item">
        {{ t('sqlHistory.favoriteCount', { count: favoriteCount }) }}
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Filter, RefreshCw, Trash2, Search, Star, X } from 'lucide-vue-next'
import { NButton, NInput, NSelect, NTabs, NTab, NEmpty, createDiscreteApi } from 'naive-ui'
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'

import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'
import {
  getHistory,
  deleteHistory,
  clearHistory,
  toggleFavorite,
  migrateFromLocalStorage,
  type SqlHistoryItem,
} from '@/extensions/builtin/workbench/services/sql-history-service'

const { t } = useI18n()
const { message } = createDiscreteApi(['message'])
const connectionStore = useConnectionStore()

// 状态
const searchText = ref('')
const activeTab = ref<'all' | 'favorites' | 'recent'>('all')
const showFilter = ref(false)
const filterConnection = ref<string | null>(null)
const filterType = ref<string | null>(null)
const filterStatus = ref<'success' | 'error' | null>(null)
const historyList = ref<SqlHistoryItem[]>([])
const loading = ref(false)

// 选项
const connectionOptions = computed(() => {
  return connectionStore.connections.map(conn => ({
    label: conn.name || conn.connId,
    value: conn.connId,
  }))
})

const typeOptions = computed(() => {
  const types = [...new Set(historyList.value.map(item => item.dbType).filter((t): t is string => t !== null))]
  return types.map(type => ({
    label: type,
    value: type,
  }))
})

const statusOptions = [
  { label: t('sqlHistory.success'), value: 'success' },
  { label: t('sqlHistory.failed'), value: 'error' },
]

/** 根据 connId 查找连接名称 */
function getConnectionName(connId: string | null): string {
  if (!connId) return '—'
  const conn = connectionStore.connections.find(c => c.connId === connId)
  return conn?.name || connId
}

// 计算属性
const favoriteCount = computed(() => historyList.value.filter(item => item.isFavorite).length)

const filteredHistory = computed(() => {
  let result = [...historyList.value]

  // 按 Tab 过滤
  if (activeTab.value === 'favorites') {
    result = result.filter(item => item.isFavorite)
  } else if (activeTab.value === 'recent') {
    result = result.slice(0, 50)
  }

  // 按搜索文本过滤
  if (searchText.value) {
    const search = searchText.value.toLowerCase()
    result = result.filter(
      item =>
        item.sql.toLowerCase().includes(search) ||
        getConnectionName(item.connId).toLowerCase().includes(search)
    )
  }

  // 按连接过滤
  if (filterConnection.value) {
    result = result.filter(item => item.connId === filterConnection.value)
  }

  // 按类型过滤
  if (filterType.value) {
    result = result.filter(item => item.dbType === filterType.value)
  }

  // 按状态过滤
  if (filterStatus.value) {
    result = result.filter(item => item.success === (filterStatus.value === 'success'))
  }

  return result
})

// 方法
const toggleFilter = () => {
  showFilter.value = !showFilter.value
}

const refreshHistory = async () => {
  loading.value = true
  try {
    historyList.value = await getHistory(200)
    message.success(t('sqlHistory.refreshHistory'))
  } finally {
    loading.value = false
  }
}

const clearAllHistory = async () => {
  await clearHistory()
  historyList.value = []
  message.success(t('sqlHistory.clearHistorySuccess'))
}

const selectHistory = (item: SqlHistoryItem) => {
  window.dispatchEvent(
    new CustomEvent('sql-history-select', {
      detail: {
        sql: item.sql,
        connectionId: item.connId,
        historyItem: item,
      },
    })
  )
}

const reExecuteHistory = (item: SqlHistoryItem) => {
  window.dispatchEvent(
    new CustomEvent('sql-history-re-execute', {
      detail: {
        sql: item.sql,
        connectionId: item.connId,
        historyItem: item,
      },
    })
  )
}

const handleToggleFavorite = async (id: string) => {
  toggleFavorite(id)
  // 刷新列表以更新收藏状态
  historyList.value = await getHistory(200)
}

const deleteHistoryItem = async (id: string) => {
  await deleteHistory(id)
  historyList.value = await getHistory(200)
  message.success(t('sqlHistory.deleteHistorySuccess'))
}

const truncateSql = (sql: string, maxLength: number): string => {
  const trimmed = sql.trim()
  if (trimmed.length <= maxLength) return trimmed
  return trimmed.substring(0, maxLength) + '...'
}

const formatTime = (timestamp: string | number): string => {
  const date = typeof timestamp === 'string' ? new Date(timestamp) : new Date(timestamp)
  const now = Date.now()
  const diff = now - date.getTime()

  if (diff < 60000) return t('sqlHistory.justNow')
  if (diff < 3600000) return t('sqlHistory.minutesAgo', { count: Math.floor(diff / 60000) })
  if (diff < 86400000) return t('sqlHistory.hoursAgo', { count: Math.floor(diff / 3600000) })

  return `${date.getMonth() + 1}/${date.getDate()} ${date.getHours()}:${date.getMinutes().toString().padStart(2, '0')}`
}

// 初始化
onMounted(async () => {
  // 迁移旧 localStorage 数据
  await migrateFromLocalStorage()
  // 加载历史记录
  loading.value = true
  try {
    historyList.value = await getHistory(200)
  } finally {
    loading.value = false
  }
})
</script>

<style scoped>
.sql-history-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.history-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
}

.history-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.header-actions {
  display: flex;
  gap: 4px;
}

.search-section {
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
}

.filter-section {
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.tab-section {
  padding: 0 16px;
  border-bottom: 1px solid var(--border-color);
}

.history-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px 0;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  padding: 40px 20px;
}

.history-item {
  padding: 12px 16px;
  cursor: pointer;
  transition: background 0.2s;
  border-bottom: 1px solid var(--border-color);
}

.history-item:hover {
  background: var(--bg-hover);
}

.history-item.is-favorite {
  border-left: 3px solid var(--favorite-color);
}

.item-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}

.item-info {
  display: flex;
  gap: 8px;
  align-items: center;
}

.item-connection {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
}

.item-type {
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  padding: 2px 6px;
  border-radius: 3px;
}

.item-actions {
  display: flex;
  gap: 4px;
  opacity: 0;
  transition: opacity 0.2s;
}

.history-item:hover .item-actions {
  opacity: 1;
}

.item-sql {
  margin-bottom: 8px;
}

.item-sql pre {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.4;
  white-space: pre-wrap;
  word-break: break-all;
}

.item-footer {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 11px;
  color: var(--text-tertiary);
}

.item-time {
  flex: 1;
}

.item-status {
  font-weight: 600;
}

.item-status.success {
  color: var(--success-color);
}

.item-status.error {
  color: var(--danger-color);
}

.item-rows {
  color: var(--text-secondary);
}

.item-duration {
  color: var(--text-secondary);
}

.history-footer {
  display: flex;
  justify-content: space-between;
  padding: 8px 16px;
  border-top: 1px solid var(--border-color);
  font-size: 11px;
  color: var(--text-tertiary);
}

.stat-item {
  display: flex;
  align-items: center;
  gap: 4px;
}
</style>
