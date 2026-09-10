<template>
  <div class="favorites-panel">
    <div class="favorites-header">
      <span class="title">收藏</span>
      <div class="actions">
        <button class="icon-btn" title="刷新" @click="handleRefresh">
          <RefreshCw :size="14" />
        </button>
        <button class="icon-btn" title="导入" @click="handleImport">
          <Upload :size="14" />
        </button>
        <button class="icon-btn" title="导出" @click="handleExport">
          <Download :size="14" />
        </button>
        <button class="icon-btn" title="清空" @click="handleClear">
          <Trash2 :size="14" />
        </button>
      </div>
    </div>

    <div class="favorites-search">
      <input v-model="searchQuery" type="text" placeholder="搜索收藏..." class="search-input" />
    </div>

    <div v-if="stats.total > 0" class="favorites-stats">
      <span class="stat-item">总计: {{ stats.total }}</span>
      <span v-for="(count, type) in stats.byType" :key="type" class="stat-item">
        {{ type }}: {{ count }}
      </span>
    </div>

    <div class="favorites-list">
      <div
        v-for="item in filteredFavorites"
        :key="item.key"
        class="favorite-item"
        draggable="true"
        @dragstart="handleDragStart(item, $event)"
        @dblclick="handleOpen(item)"
        @contextmenu.prevent="handleContextMenu(item, $event)"
      >
        <component :is="getNodeIcon(item.type)" :size="16" class="item-icon" />
        <div class="item-info">
          <span class="item-label">{{ item.label }}</span>
          <span class="item-meta"> {{ item.dbName }}.{{ item.schemaName }} </span>
        </div>
        <div class="item-actions">
          <span class="access-count" title="访问次数">
            {{ item.accessCount }}
          </span>
          <button class="icon-btn remove-btn" title="移除收藏" @click.stop="handleRemove(item.key)">
            <X :size="12" />
          </button>
        </div>
      </div>

      <div v-if="filteredFavorites.length === 0" class="empty-state">
        <Star :size="48" class="empty-icon" />
        <p class="empty-text">暂无收藏</p>
        <p class="empty-hint">右键点击数据库对象可添加收藏</p>
      </div>
    </div>

    <div v-if="contextMenu.visible" class="favorites-context-menu">
      <div
        v-for="menuItem in contextMenuItems"
        :key="menuItem.id"
        class="menu-item"
        @click="handleMenuItemClick(menuItem)"
      >
        <component :is="menuItem.icon" v-if="menuItem.icon" :size="14" class="menu-icon" />
        <span class="menu-label">{{ menuItem.label }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  RefreshCw,
  Upload,
  Download,
  Trash2,
  Star,
  X,
  Table,
  FileText,
  Database,
  Layers,
  Server,
} from 'lucide-vue-next'
import { ref, computed, onMounted } from 'vue'

import { useFavorites } from '../composables/use-favorites'

import type { IFavoriteItem } from '../composables/use-favorites'
import type { Component } from 'vue'

const favorites = useFavorites()

const searchQuery = ref('')
const contextMenu = ref({
  visible: false,
  x: 0,
  y: 0,
  item: null as IFavoriteItem | null,
})

const filteredFavorites = computed(() => {
  if (!searchQuery.value) {
    return favorites.favoriteList.value
  }
  return favorites.searchFavorites(searchQuery.value)
})

const stats = computed(() => favorites.getStats())

function getNodeIcon(type: string) {
  const iconMap: Record<string, Component> = {
    table: Table,
    view: FileText,
    database: Database,
    schema: Layers,
    connection: Server,
  }
  return iconMap[type] || Database
}

function handleDragStart(item: IFavoriteItem, event: DragEvent) {
  const dragData = {
    type: item.type,
    connectionId: item.connectionId,
    dbName: item.dbName,
    schemaName: item.schemaName,
    objectName: item.objectName,
  }
  if (event.dataTransfer) {
    event.dataTransfer.setData('application/x-rdatastation-favorite', JSON.stringify(dragData))
    event.dataTransfer.effectAllowed = 'copy'
  }
}

function handleOpen(item: IFavoriteItem) {
  favorites.updateAccessTime(item.key)

  if (item.type === 'table' || item.type === 'view') {
    window.dispatchEvent(
      new CustomEvent('open-table-data', {
        detail: {
          connectionId: item.connectionId,
          dbName: item.dbName,
          schemaName: item.schemaName,
          tableName: item.objectName,
        },
      })
    )
  }
}

function handleRemove(key: string) {
  favorites.removeFavorite(key)
}

function handleRefresh() {
  favorites.saveToStorage()
}

function handleImport() {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = '.json'
  input.onchange = e => {
    const file = (e.target as HTMLInputElement).files?.[0]
    if (!file) return

    const reader = new FileReader()
    reader.onload = event => {
      try {
        const items = JSON.parse(event.target?.result as string) as IFavoriteItem[]
        favorites.importFavorites(items)
      } catch (error) {
        console.error('导入收藏失败:', error)
      }
    }
    reader.readAsText(file)
  }
  input.click()
}

function handleExport() {
  const items = favorites.exportFavorites()
  const blob = new Blob([JSON.stringify(items, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `rdatastation-favorites-${Date.now()}.json`
  a.click()
  URL.revokeObjectURL(url)
}

function handleClear() {
  if (confirm('确定要清空所有收藏吗？')) {
    favorites.clearAll()
  }
}

function handleContextMenu(item: IFavoriteItem, event: MouseEvent) {
  contextMenu.value = {
    visible: true,
    x: event.clientX,
    y: event.clientY,
    item,
  }
}

const contextMenuItems = computed(() => {
  if (!contextMenu.value.item) return []

  return [
    {
      id: 'open',
      label: '打开',
      icon: Table,
      action: () => {
        if (contextMenu.value.item) {
          handleOpen(contextMenu.value.item)
        }
      },
    },
    {
      id: 'remove',
      label: '移除收藏',
      icon: X,
      action: () => {
        if (contextMenu.value.item) {
          handleRemove(contextMenu.value.item.key)
        }
      },
    },
  ]
})

interface MenuItem {
  id: string
  label: string
  icon: Component
  action: () => void
}

function handleMenuItemClick(menuItem: MenuItem) {
  menuItem.action?.()
  contextMenu.value.visible = false
}

onMounted(() => {
  document.addEventListener('click', () => {
    contextMenu.value.visible = false
  })
})
</script>

<style scoped>
.favorites-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.favorites-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
}

.title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.actions {
  display: flex;
  gap: 4px;
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
  transition: all 0.2s;
}

.icon-btn:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.favorites-search {
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
}

.search-input {
  width: 100%;
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 12px;
  outline: none;
  transition: border-color 0.2s;
}

.search-input:focus {
  border-color: var(--primary-color);
}

.favorites-stats {
  display: flex;
  gap: 8px;
  padding: 6px 12px;
  border-bottom: 1px solid var(--border-color);
  font-size: 11px;
  color: var(--text-tertiary);
}

.stat-item {
  padding: 2px 6px;
  background: var(--bg-secondary);
  border-radius: 3px;
}

.favorites-list {
  flex: 1;
  overflow-y: auto;
  padding: 4px;
}

.favorite-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 4px;
  cursor: pointer;
  transition: background 0.2s;
}

.favorite-item:hover {
  background: var(--bg-secondary);
}

.item-icon {
  color: var(--text-secondary);
  flex-shrink: 0;
}

.item-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.item-label {
  font-size: 12px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.item-meta {
  font-size: 10px;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.item-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.access-count {
  font-size: 10px;
  color: var(--text-tertiary);
  padding: 2px 4px;
  background: var(--bg-tertiary);
  border-radius: 3px;
}

.remove-btn {
  opacity: 0;
  transition: opacity 0.2s;
}

.favorite-item:hover .remove-btn {
  opacity: 1;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px 20px;
  text-align: center;
}

.empty-icon {
  color: var(--text-tertiary);
  margin-bottom: 12px;
}

.empty-text {
  font-size: 13px;
  color: var(--text-secondary);
  margin: 0 0 4px 0;
}

.empty-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  margin: 0;
}

.favorites-context-menu {
  position: fixed;
  z-index: 1000;
  min-width: 160px;
  padding: 4px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
}

.menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-radius: 4px;
  cursor: pointer;
  transition: background 0.2s;
  font-size: 12px;
  color: var(--text-primary);
}

.menu-item:hover {
  background: var(--bg-secondary);
}

.menu-icon {
  color: var(--text-secondary);
}

.menu-label {
  flex: 1;
}
</style>
