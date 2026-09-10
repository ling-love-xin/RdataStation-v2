/**
 * 数据库对象收藏功能
 *
 * 支持收藏常用表/视图/数据库对象
 * 使用 localStorage 持久化存储
 */

import { ref, computed } from 'vue'

export interface IFavoriteItem {
  /** 节点 key */
  key: string
  /** 节点类型 */
  type: string
  /** 节点标签 */
  label: string
  /** 连接 ID */
  connectionId: string
  /** 数据库名称 */
  dbName?: string
  /** Schema 名称 */
  schemaName?: string
  /** 对象名称 */
  objectName?: string
  /** 收藏时间 */
  createdAt: number
  /** 最后访问时间 */
  lastAccessedAt?: number
  /** 访问次数 */
  accessCount: number
}

export interface IFavoriteGroup {
  /** 分组 ID */
  id: string
  /** 分组名称 */
  name: string
  /** 收藏项列表 */
  items: IFavoriteItem[]
  /** 是否展开 */
  isExpanded: boolean
}

const STORAGE_KEY = 'rdatastation-favorites'

export function useFavorites() {
  const favorites = ref<Map<string, IFavoriteItem>>(new Map())
  const groups = ref<IFavoriteGroup[]>([
    {
      id: 'default',
      name: '默认收藏',
      items: [],
      isExpanded: true,
    },
  ])

  /**
   * 初始化：从 localStorage 加载收藏
   */
  function initialize(): void {
    try {
      const stored = localStorage.getItem(STORAGE_KEY)
      if (stored) {
        const data = JSON.parse(stored) as IFavoriteItem[]
        for (const item of data) {
          favorites.value.set(item.key, item)
        }
      }
    } catch (error) {
      console.error('加载收藏失败:', error)
    }
  }

  /**
   * 保存到 localStorage
   */
  function saveToStorage(): void {
    try {
      const items = Array.from(favorites.value.values())
      localStorage.setItem(STORAGE_KEY, JSON.stringify(items))
    } catch (error) {
      console.error('保存收藏失败:', error)
    }
  }

  /**
   * 添加收藏
   */
  function addFavorite(item: Omit<IFavoriteItem, 'createdAt' | 'accessCount'>): void {
    const favorite: IFavoriteItem = {
      ...item,
      createdAt: Date.now(),
      accessCount: 0,
    }

    favorites.value.set(item.key, favorite)
    saveToStorage()
  }

  /**
   * 移除收藏
   */
  function removeFavorite(key: string): void {
    favorites.value.delete(key)
    saveToStorage()
  }

  /**
   * 切换收藏状态
   */
  function toggleFavorite(item: Omit<IFavoriteItem, 'createdAt' | 'accessCount'>): boolean {
    if (favorites.value.has(item.key)) {
      removeFavorite(item.key)
      return false
    } else {
      addFavorite(item)
      return true
    }
  }

  /**
   * 检查是否已收藏
   */
  function isFavorite(key: string): boolean {
    return favorites.value.has(key)
  }

  /**
   * 获取所有收藏的 key 集合
   */
  const favoriteKeys = computed(() => {
    return new Set(favorites.value.keys())
  })

  /**
   * 获取收藏列表
   */
  const favoriteList = computed(() => {
    return Array.from(favorites.value.values()).sort(
      (a, b) => (b.lastAccessedAt || b.createdAt) - (a.lastAccessedAt || a.createdAt)
    )
  })

  /**
   * 更新访问时间
   */
  function updateAccessTime(key: string): void {
    const item = favorites.value.get(key)
    if (item) {
      item.lastAccessedAt = Date.now()
      item.accessCount++
      favorites.value.set(key, item)
      saveToStorage()
    }
  }

  /**
   * 搜索收藏
   */
  function searchFavorites(query: string): IFavoriteItem[] {
    const queryLower = query.toLowerCase()
    return Array.from(favorites.value.values()).filter(
      item =>
        item.label.toLowerCase().includes(queryLower) ||
        item.type.toLowerCase().includes(queryLower) ||
        (item.objectName && item.objectName.toLowerCase().includes(queryLower))
    )
  }

  /**
   * 批量导入收藏
   */
  function importFavorites(items: IFavoriteItem[]): void {
    for (const item of items) {
      favorites.value.set(item.key, item)
    }
    saveToStorage()
  }

  /**
   * 导出收藏
   */
  function exportFavorites(): IFavoriteItem[] {
    return Array.from(favorites.value.values())
  }

  /**
   * 清空所有收藏
   */
  function clearAll(): void {
    favorites.value.clear()
    saveToStorage()
  }

  /**
   * 获取收藏统计
   */
  function getStats(): { total: number; byType: Record<string, number> } {
    const items = Array.from(favorites.value.values())
    const byType: Record<string, number> = {}

    for (const item of items) {
      byType[item.type] = (byType[item.type] || 0) + 1
    }

    return {
      total: items.length,
      byType,
    }
  }

  // 初始化
  initialize()

  return {
    favorites,
    groups,
    favoriteKeys,
    favoriteList,
    addFavorite,
    removeFavorite,
    toggleFavorite,
    isFavorite,
    updateAccessTime,
    searchFavorites,
    importFavorites,
    exportFavorites,
    clearAll,
    getStats,
    saveToStorage,
  }
}
