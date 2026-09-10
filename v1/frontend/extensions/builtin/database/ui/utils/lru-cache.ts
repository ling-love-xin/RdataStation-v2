/**
 * LRU 缓存实现
 *
 * 基于最近最少使用原则的缓存策略
 */

export interface LRUCacheOptions<T> {
  maxSize: number
  onEvict?: (key: string, value: T) => void
}

export class LRUCache<T> {
  private cache = new Map<string, { value: T; timestamp: number }>()
  private maxSize: number
  private onEvict?: (key: string, value: T) => void

  constructor(options: LRUCacheOptions<T>) {
    this.maxSize = options.maxSize
    this.onEvict = options.onEvict
  }

  get(key: string): T | undefined {
    const entry = this.cache.get(key)
    if (entry) {
      entry.timestamp = Date.now()
      return entry.value
    }
    return undefined
  }

  set(key: string, value: T): void {
    if (this.cache.size >= this.maxSize) {
      this.evict()
    }
    this.cache.set(key, { value, timestamp: Date.now() })
  }

  has(key: string): boolean {
    return this.cache.has(key)
  }

  delete(key: string): boolean {
    const entry = this.cache.get(key)
    if (entry && this.onEvict) {
      this.onEvict(key, entry.value)
    }
    return this.cache.delete(key)
  }

  clear(): void {
    const onEvictFn = this.onEvict
    if (onEvictFn) {
      this.cache.forEach((entry, key) => {
        onEvictFn(key, entry.value)
      })
    }
    this.cache.clear()
  }

  size(): number {
    return this.cache.size
  }

  keys(): IterableIterator<string> {
    return this.cache.keys()
  }

  private evict(): void {
    let oldestKey: string | null = null
    let oldestTimestamp = Infinity

    this.cache.forEach((entry, key) => {
      if (entry.timestamp < oldestTimestamp) {
        oldestTimestamp = entry.timestamp
        oldestKey = key
      }
    })

    if (oldestKey !== null) {
      const entry = this.cache.get(oldestKey)
      if (entry && this.onEvict) {
        this.onEvict(oldestKey, entry.value)
      }
      this.cache.delete(oldestKey)
    }
  }
}
