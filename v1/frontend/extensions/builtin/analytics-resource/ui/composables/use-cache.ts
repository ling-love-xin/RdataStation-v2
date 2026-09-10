import { ref } from 'vue'
interface CacheEntry<T> {
  key: string
  data: T
  timestamp: number
  accessCount: number
}
export class LRUCache<T = unknown> {
  private cache: Map<string, CacheEntry<T>>
  private maxSize: number
  private ttl: number
  constructor(maxSize: number = 100, ttl: number = 5 * 60 * 1000) {
    this.cache = new Map()
    this.maxSize = maxSize
    this.ttl = ttl
  }
  get(key: string): T | null {
    const entry = this.cache.get(key)
    if (!entry) return null
    if (Date.now() - entry.timestamp > this.ttl) {
      this.cache.delete(key)
      return null
    }
    entry.accessCount++
    entry.timestamp = Date.now()
    return entry.data
  }
  set(key: string, data: T): void {
    if (this.cache.size >= this.maxSize) {
      this.evict()
    }
    this.cache.set(key, {
      key,
      data,
      timestamp: Date.now(),
      accessCount: 1,
    })
  }
  has(key: string): boolean {
    const entry = this.cache.get(key)
    if (!entry) return false
    if (Date.now() - entry.timestamp > this.ttl) {
      this.cache.delete(key)
      return false
    }
    return true
  }
  delete(key: string): void {
    this.cache.delete(key)
  }
  clear(): void {
    this.cache.clear()
  }
  size(): number {
    return this.cache.size
  }
  private evict(): void {
    let oldestKey: string | null = null
    let oldestTime = Infinity
    for (const [key, entry] of this.cache) {
      if (entry.timestamp < oldestTime) {
        oldestTime = entry.timestamp
        oldestKey = key
      }
    }
    if (oldestKey) {
      this.cache.delete(oldestKey)
    }
  }
}
export interface CacheConfig {
  maxSize?: number
  ttl?: number
  enabled?: boolean
}
export function useCache<T = unknown>(config: CacheConfig = {}) {
  let { maxSize = 50, ttl = 5 * 60 * 1000, enabled = true } = config
  const cache = new LRUCache<T>(maxSize, ttl)
  const hitCount = ref(0)
  const missCount = ref(0)
  const get = (key: string): T | null => {
    if (!enabled) return null
    const result = cache.get(key)
    if (result !== null) {
      hitCount.value++
    } else {
      missCount.value++
    }
    return result
  }
  const set = (key: string, data: T): void => {
    if (!enabled) return
    cache.set(key, data)
  }
  const has = (key: string): boolean => {
    if (!enabled) return false
    return cache.has(key)
  }
  const deleteKey = (key: string): void => {
    cache.delete(key)
  }
  const clear = (): void => {
    cache.clear()
    hitCount.value = 0
    missCount.value = 0
  }
  const updateConfig = (newConfig: Partial<CacheConfig>): void => {
    if (newConfig.enabled !== undefined) enabled = newConfig.enabled
    if (newConfig.ttl !== undefined) ttl = newConfig.ttl
    if (newConfig.maxSize !== undefined) maxSize = newConfig.maxSize
    if (!enabled) {
      cache.clear()
    }
  }
  const stats = () => ({
    hits: hitCount.value,
    misses: missCount.value,
    total: hitCount.value + missCount.value,
    hitRate:
      hitCount.value + missCount.value > 0
        ? ((hitCount.value / (hitCount.value + missCount.value)) * 100).toFixed(2)
        : '0',
    size: cache.size(),
  })
  return {
    get,
    set,
    has,
    delete: deleteKey,
    clear,
    updateConfig,
    stats,
  }
}
export function createCacheKey(...parts: (string | number | boolean | null | undefined)[]): string {
  return parts.map(p => String(p ?? 'null')).join(':')
}
