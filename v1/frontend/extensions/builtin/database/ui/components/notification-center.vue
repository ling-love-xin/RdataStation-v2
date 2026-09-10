<template>
  <div class="notification-center">
    <div class="notification-header">
      <h3>通知中心</h3>
      <div class="header-actions">
        <button class="btn-mark-all-read" @click="markAllAsRead">
          <Check :size="14" />
          全部已读
        </button>
        <button class="btn-clear-all" @click="clearAll">
          <Trash2 :size="14" />
          清空
        </button>
      </div>
    </div>

    <div class="notification-tabs">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        class="tab-btn"
        :class="{ active: activeTab === tab.id }"
        @click="activeTab = tab.id"
      >
        {{ tab.label }}
        <span v-if="getTabCount(tab.id) > 0" class="tab-badge">{{ getTabCount(tab.id) }}</span>
      </button>
    </div>

    <div class="notification-list">
      <div v-if="filteredNotifications.length === 0" class="empty-notifications">
        <Bell :size="32" />
        <p>暂无通知</p>
      </div>

      <div
        v-for="notification in filteredNotifications"
        :key="notification.id"
        class="notification-item"
        :class="{ read: notification.read, [notification.type]: true }"
        @click="markAsRead(notification.id)"
      >
        <div class="notification-icon">
          <Info v-if="notification.type === 'info'" :size="16" />
          <AlertTriangle v-else-if="notification.type === 'warning'" :size="16" />
          <AlertCircle v-else-if="notification.type === 'error'" :size="16" />
          <CheckCircle v-else-if="notification.type === 'success'" :size="16" />
        </div>

        <div class="notification-content">
          <p class="notification-message">{{ notification.message }}</p>
          <span class="notification-time">{{ formatTime(notification.timestamp) }}</span>
        </div>

        <button class="btn-dismiss" @click.stop="dismissNotification(notification.id)">
          <X :size="14" />
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  Bell,
  Check,
  Trash2,
  X,
  Info,
  AlertTriangle,
  AlertCircle,
  CheckCircle,
} from 'lucide-vue-next'
import { ref, computed } from 'vue'

export interface Notification {
  id: string
  type: 'info' | 'warning' | 'error' | 'success'
  message: string
  timestamp: number
  read: boolean
}

const props = defineProps<{
  notifications: Notification[]
}>()

const emit = defineEmits<{
  markAsRead: [id: string]
  markAllAsRead: []
  dismiss: [id: string]
  clearAll: []
}>()

const activeTab = ref('all')

const tabs = [
  { id: 'all', label: '全部' },
  { id: 'unread', label: '未读' },
  { id: 'error', label: '错误' },
  { id: 'warning', label: '警告' },
]

const filteredNotifications = computed(() => {
  let result = [...props.notifications]

  if (activeTab.value === 'unread') {
    result = result.filter(n => !n.read)
  } else if (activeTab.value === 'error') {
    result = result.filter(n => n.type === 'error')
  } else if (activeTab.value === 'warning') {
    result = result.filter(n => n.type === 'warning')
  }

  return result.sort((a, b) => b.timestamp - a.timestamp)
})

function getTabCount(tabId: string): number {
  if (tabId === 'all') return props.notifications.length
  if (tabId === 'unread') return props.notifications.filter(n => !n.read).length
  return props.notifications.filter(n => n.type === tabId).length
}

function formatTime(timestamp: number): string {
  const now = Date.now()
  const diff = now - timestamp

  if (diff < 60000) return '刚刚'
  if (diff < 3600000) return `${Math.floor(diff / 60000)}分钟前`
  if (diff < 86400000) return `${Math.floor(diff / 3600000)}小时前`

  const date = new Date(timestamp)
  return `${date.getMonth() + 1}/${date.getDate()}`
}

function markAsRead(id: string) {
  emit('markAsRead', id)
}

function markAllAsRead() {
  emit('markAllAsRead')
}

function dismissNotification(id: string) {
  emit('dismiss', id)
}

function clearAll() {
  emit('clearAll')
}
</script>

<style scoped>
.notification-center {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.notification-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
}

.notification-header h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.header-actions {
  display: flex;
  gap: 8px;
}

.btn-mark-all-read,
.btn-clear-all {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  background: transparent;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.btn-mark-all-read:hover,
.btn-clear-all:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.notification-tabs {
  display: flex;
  gap: 4px;
  padding: 8px 16px;
  border-bottom: 1px solid var(--border-color);
}

.tab-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 12px;
  background: transparent;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: all 0.15s;
}

.tab-btn:hover {
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}

.tab-btn.active {
  background: var(--primary-color);
  color: white;
}

.tab-badge {
  min-width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--error-color);
  border-radius: 8px;
  font-size: 10px;
  color: white;
  padding: 0 4px;
}

.tab-btn.active .tab-badge {
  background: white;
  color: var(--primary-color);
}

.notification-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.empty-notifications {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px;
  color: var(--text-tertiary);
}

.empty-notifications p {
  margin: 8px 0 0 0;
  font-size: 13px;
}

.notification-item {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px;
  margin-bottom: 4px;
  background: var(--bg-secondary);
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.15s;
}

.notification-item:hover {
  background: var(--bg-tertiary);
}

.notification-item.read {
  opacity: 0.6;
}

.notification-icon {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 6px;
  flex-shrink: 0;
}

.notification-item.info .notification-icon {
  background: rgba(100, 100, 255, 0.15);
  color: #6464ff;
}

.notification-item.warning .notification-icon {
  background: rgba(255, 180, 0, 0.15);
  color: #ffb400;
}

.notification-item.error .notification-icon {
  background: rgba(255, 100, 100, 0.15);
  color: #ff6464;
}

.notification-item.success .notification-icon {
  background: rgba(0, 180, 100, 0.15);
  color: #00b464;
}

.notification-content {
  flex: 1;
  min-width: 0;
}

.notification-message {
  margin: 0 0 4px 0;
  font-size: 13px;
  color: var(--text-primary);
  line-height: 1.4;
}

.notification-time {
  font-size: 11px;
  color: var(--text-tertiary);
}

.btn-dismiss {
  padding: 2px;
  background: transparent;
  border: none;
  border-radius: 3px;
  color: var(--text-tertiary);
  cursor: pointer;
  opacity: 0;
  transition: all 0.15s;
}

.notification-item:hover .btn-dismiss {
  opacity: 1;
}

.btn-dismiss:hover {
  background: var(--border-color);
  color: var(--text-primary);
}
</style>
