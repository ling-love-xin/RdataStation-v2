/**
 * LazyLoader - "先查缓存，再回退 API" 通用工厂
 *
 * 封装 database-navigator-store 中 loadCatalogs / loadSchemas / loadTables / loadColumns
 * 共用的 cache-first-then-fallback 模式：
 *   1. Loading dedup guard（loadingSet 防重入）
 *   2. 检查 L2 缓存是否有效
 *   3. 缓存命中 → 写入 Store → 返回
 *   4. 缓存未命中 → 从 API 加载 → 写入 Store
 *   5. finally 清理 loadingSet
 */

export interface LazyLoaderConfig<T> {
  /** 检查 L2 缓存是否有效 */
  checkCache: () => Promise<{ is_valid: boolean; stats?: { table_count?: number } | null }>
  /** 从 L2 缓存加载 */
  loadFromCache: () => Promise<T | null>
  /** 从 API 加载 */
  loadFromApi: () => Promise<T>
  /** 写入 Store */
  onLoaded: (data: T) => void
  /** 缓存键（用于 loading guard） */
  cacheKey: string
  /** Loading 状态集合（shared ref） */
  loadingSet: Set<string>
}

export function createLazyLoader<T>(config: LazyLoaderConfig<T>): () => Promise<T> {
  return async () => {
    // 1. Loading dedup guard
    if (config.loadingSet.has(config.cacheKey)) {
      const empty = (await Promise.resolve(null)) as unknown as T
      return empty
    }

    config.loadingSet.add(config.cacheKey)

    try {
      // 2. 检查 L2 缓存状态
      const cacheStatus = await config.checkCache().catch(() => ({
        is_valid: false,
        stats: null,
      }))

      // 3. 缓存有效 → 尝试从缓存加载
      if (cacheStatus.is_valid) {
        const cached = await config.loadFromCache().catch(() => null)
        if (cached !== null) {
          config.onLoaded(cached)
          return cached
        }
      }

      // 4. 回退到 API
      const data = await config.loadFromApi()
      config.onLoaded(data)
      return data
    } finally {
      config.loadingSet.delete(config.cacheKey)
    }
  }
}
