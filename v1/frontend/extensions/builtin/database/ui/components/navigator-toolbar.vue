<template>
  <div class="dbeaver-toolbar">
    <button
      class="toolbar-btn"
      title="新建连接"
      aria-label="新建连接"
      @click="$emit('new-connection')"
    >
      <Plug :size="16" aria-hidden="true" />
    </button>
    <button class="toolbar-btn" title="新建分组" aria-label="新建分组" @click="$emit('new-group')">
      <FolderPlus :size="16" aria-hidden="true" />
    </button>
    <button
      class="toolbar-btn"
      title="断开连接"
      aria-label="断开连接"
      :disabled="!hasConnection"
      @click="$emit('disconnect')"
    >
      <Unplug :size="16" aria-hidden="true" />
    </button>
    <div class="toolbar-separator"></div>
    <button
      class="toolbar-btn"
      :class="{
        disabled: !hasConnection || isInTransaction,
        'transaction-active': isInTransaction,
      }"
      title="开始事务"
      aria-label="开始事务"
      :disabled="!hasConnection || isInTransaction"
      @click="$emit('begin-transaction')"
    >
      <PlayCircle :size="16" aria-hidden="true" />
    </button>
    <button
      class="toolbar-btn"
      :class="{
        disabled: !hasConnection || !isInTransaction,
        'transaction-active': isInTransaction,
      }"
      title="提交事务"
      aria-label="提交事务"
      :disabled="!hasConnection || !isInTransaction"
      @click="$emit('commit-transaction')"
    >
      <CheckCircle :size="16" aria-hidden="true" />
    </button>
    <button
      class="toolbar-btn"
      :class="{
        disabled: !hasConnection || !isInTransaction,
        'transaction-active': isInTransaction,
        'transaction-warning': isInTransaction,
      }"
      title="回滚事务"
      aria-label="回滚事务"
      :disabled="!hasConnection || !isInTransaction"
      @click="$emit('rollback-transaction')"
    >
      <XCircle :size="16" aria-hidden="true" />
    </button>
    <div class="toolbar-separator"></div>
    <button
      class="toolbar-btn"
      title="刷新"
      aria-label="刷新"
      :disabled="isRefreshing"
      @click="$emit('refresh')"
    >
      <RefreshCw :size="16" :class="{ spinning: isRefreshing }" aria-hidden="true" />
    </button>
    <div class="toolbar-separator"></div>
    <button class="toolbar-btn" title="搜索" aria-label="搜索" @click="$emit('focus-search')">
      <Search :size="16" aria-hidden="true" />
    </button>
    <div class="toolbar-separator"></div>
    <button class="toolbar-btn" title="过滤器" aria-label="过滤器" @click="$emit('toggle-filter')">
      <Filter :size="16" :class="{ active: showFilter }" aria-hidden="true" />
    </button>
    <button class="toolbar-btn" title="视图" aria-label="切换视图" @click="$emit('toggle-view')">
      <LayoutTemplate :size="16" aria-hidden="true" />
    </button>
  </div>
</template>

<script setup lang="ts">
import {
  Plug,
  Unplug,
  RefreshCw,
  Search,
  Filter,
  LayoutTemplate,
  PlayCircle,
  CheckCircle,
  XCircle,
  FolderPlus,
} from 'lucide-vue-next'

defineProps<{
  hasConnection: boolean
  isRefreshing: boolean
  showFilter: boolean
  isInTransaction: boolean
}>()

defineEmits<{
  'new-connection': []
  'new-group': []
  disconnect: []
  'begin-transaction': []
  'commit-transaction': []
  'rollback-transaction': []
  refresh: []
  'focus-search': []
  'toggle-filter': []
  'toggle-view': []
}>()
</script>

<style scoped>
.dbeaver-toolbar {
  display: flex;
  align-items: center;
  padding: 4px 6px;
  background: var(--bg-tertiary);
  border-bottom: 1px solid var(--border-color);
  gap: 2px;
}

.toolbar-btn {
  width: 24px;
  height: 24px;
  border: none;
  border-radius: 2px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.1s;
  padding: 0;
}

.toolbar-btn:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.toolbar-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.toolbar-btn.active {
  background: var(--primary-light);
  color: var(--primary-color);
}

.toolbar-btn.transaction-active {
  background: var(--bg-hover);
  color: var(--success-color);
}

.toolbar-btn.transaction-warning {
  color: var(--warning-color);
}

.toolbar-btn.transaction-active:hover:not(:disabled) {
  background: var(--success-light);
}

.toolbar-btn.transaction-warning:hover:not(:disabled) {
  background: var(--warning-light);
}

.toolbar-separator {
  width: 1px;
  height: 18px;
  background: var(--border-color);
  margin: 0 2px;
}

.spinning {
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
</style>
