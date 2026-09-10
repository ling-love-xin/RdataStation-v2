import { describe, it, expect, vi } from 'vitest'

import { createLazyLoader } from '../lazy-loader'

describe('lazy-loader', () => {
  it('should return cached data when cache is valid', async () => {
    const data = { items: [1, 2, 3] }
    const onLoaded = vi.fn()
    const loadingSet = new Set<string>()
    const checkCache = vi.fn().mockResolvedValue({ is_valid: true, stats: null })
    const loadFromCache = vi.fn().mockResolvedValue(data)
    const loadFromApi = vi.fn()

    const loader = createLazyLoader({
      checkCache,
      loadFromCache,
      loadFromApi,
      onLoaded,
      cacheKey: 'test-key',
      loadingSet,
    })

    const result = await loader()

    expect(result).toBe(data)
    expect(checkCache).toHaveBeenCalledOnce()
    expect(loadFromCache).toHaveBeenCalledOnce()
    expect(loadFromApi).not.toHaveBeenCalled()
    expect(onLoaded).toHaveBeenCalledWith(data)
  })

  it('should call API when cache is invalid', async () => {
    const data = { items: [4, 5, 6] }
    const onLoaded = vi.fn()
    const loadingSet = new Set<string>()
    const checkCache = vi.fn().mockResolvedValue({ is_valid: false, stats: null })
    const loadFromCache = vi.fn()
    const loadFromApi = vi.fn().mockResolvedValue(data)

    const loader = createLazyLoader({
      checkCache,
      loadFromCache,
      loadFromApi,
      onLoaded,
      cacheKey: 'test-key-2',
      loadingSet,
    })

    const result = await loader()

    expect(result).toBe(data)
    expect(checkCache).toHaveBeenCalledOnce()
    expect(loadFromCache).not.toHaveBeenCalled()
    expect(loadFromApi).toHaveBeenCalledOnce()
    expect(onLoaded).toHaveBeenCalledWith(data)
  })

  it('should call API when cache check throws', async () => {
    const data = { items: [7, 8, 9] }
    const onLoaded = vi.fn()
    const loadingSet = new Set<string>()
    const checkCache = vi.fn().mockRejectedValue(new Error('cache error'))
    const loadFromCache = vi.fn()
    const loadFromApi = vi.fn().mockResolvedValue(data)

    const loader = createLazyLoader({
      checkCache,
      loadFromCache,
      loadFromApi,
      onLoaded,
      cacheKey: 'test-key-3',
      loadingSet,
    })

    const result = await loader()

    expect(result).toBe(data)
    expect(checkCache).toHaveBeenCalledOnce()
    expect(loadFromCache).not.toHaveBeenCalled()
    expect(loadFromApi).toHaveBeenCalledOnce()
    expect(onLoaded).toHaveBeenCalledWith(data)
  })

  it('should prevent concurrent loads (loading set dedup)', async () => {
    const data = { items: [10, 11, 12] }
    const onLoaded = vi.fn()
    const loadingSet = new Set<string>()
    let resolveApi!: (value: { items: number[] }) => void
    const loadFromApiPromise = new Promise<{ items: number[] }>(resolve => {
      resolveApi = resolve
    })
    const checkCache = vi.fn().mockResolvedValue({ is_valid: false, stats: null })
    const loadFromCache = vi.fn()
    const loadFromApi = vi.fn().mockImplementation(() => loadFromApiPromise)

    const loader = createLazyLoader({
      checkCache,
      loadFromCache,
      loadFromApi,
      onLoaded,
      cacheKey: 'dedup-key',
      loadingSet,
    })

    // Start two concurrent loads
    const p1 = loader()
    const p2 = loader()

    // The second call should see loadingSet already has the key and return null immediately
    const result2 = await p2
    expect(result2).toBeNull()

    // Resolve the first load
    resolveApi(data)
    const result1 = await p1
    expect(result1).toBe(data)

    // API should only be called once
    expect(loadFromApi).toHaveBeenCalledTimes(1)
    expect(onLoaded).toHaveBeenCalledTimes(1)
  })
})
