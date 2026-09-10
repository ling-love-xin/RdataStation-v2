import { describe, it, expect, beforeEach } from 'vitest'

import {
  LRUCache,
  useCache,
  createCacheKey,
} from '../../src/extensions/builtin/analytics-resource/ui/composables/use-cache'

describe('LRUCache', () => {
  let cache: LRUCache<string>

  beforeEach(() => {
    cache = new LRUCache<string>(5, 1000)
  })

  describe('set / get', () => {
    it('should store and retrieve values', () => {
      cache.set('key1', 'value1')
      expect(cache.get('key1')).toBe('value1')
    })

    it('should return null for missing keys', () => {
      expect(cache.get('missing')).toBeNull()
    })
  })

  describe('TTL expiration', () => {
    it('should expire entries after TTL', async () => {
      const shortTtl = new LRUCache<string>(5, 20)
      shortTtl.set('key1', 'value1')
      await new Promise(r => setTimeout(r, 50))
      expect(shortTtl.get('key1')).toBeNull()
    })

    it('should not expire recently accessed entries', () => {
      cache = new LRUCache<string>(5, 5000)
      cache.set('key1', 'value1')
      expect(cache.get('key1')).toBe('value1')
    })
  })

  describe('has', () => {
    it('should return true for existing non-expired keys', () => {
      cache.set('key1', 'value1')
      expect(cache.has('key1')).toBe(true)
    })

    it('should return false for expired keys', async () => {
      const shortTtl = new LRUCache<string>(5, 20)
      shortTtl.set('key1', 'value1')
      await new Promise(r => setTimeout(r, 50))
      expect(shortTtl.has('key1')).toBe(false)
    })
  })

  describe('eviction', () => {
    it('should evict oldest entry when capacity exceeded', () => {
      for (let i = 0; i < 6; i++) {
        cache.set(`key${i}`, `value${i}`)
      }
      expect(cache.has('key0')).toBe(false)
      expect(cache.has('key1')).toBe(true)
    })
  })

  describe('delete', () => {
    it('should remove an entry', () => {
      cache.set('key1', 'value1')
      cache.delete('key1')
      expect(cache.get('key1')).toBeNull()
    })
  })

  describe('clear', () => {
    it('should remove all entries', () => {
      cache.set('key1', 'value1')
      cache.set('key2', 'value2')
      cache.clear()
      expect(cache.size()).toBe(0)
    })
  })

  describe('size', () => {
    it('should return current entry count', () => {
      expect(cache.size()).toBe(0)
      cache.set('k1', 'v1')
      cache.set('k2', 'v2')
      expect(cache.size()).toBe(2)
    })

    it('should not count expired entries', async () => {
      cache.set('k1', 'v1')
      await new Promise(r => setTimeout(r, 50))
      expect(cache.size()).toBe(1)
    })
  })
})

describe('useCache', () => {
  it('should track hit and miss counts', () => {
    const c = useCache<string>({ maxSize: 10, ttl: 60000 })
    c.set('a', 'hello')
    c.get('a')
    c.get('b')

    const s = c.stats()
    expect(s.hits).toBe(1)
    expect(s.misses).toBe(1)
    expect(s.hitRate).toBe('50.00')
  })

  it('should skip cache when disabled', () => {
    const c = useCache<string>({ enabled: false })
    c.set('a', 'hello')
    expect(c.get('a')).toBeNull()
    expect(c.has('a')).toBe(false)
  })

  it('should update config and clear when disabled', () => {
    const c = useCache<string>({ maxSize: 10, ttl: 60000 })
    c.set('a', 'hello')
    c.updateConfig({ enabled: false })
    expect(c.get('a')).toBeNull()
  })

  it('should clear all entries and reset counters', () => {
    const c = useCache<string>({ maxSize: 10, ttl: 60000 })
    c.set('a', 'hello')
    c.get('a')
    c.get('b')
    c.clear()

    const s = c.stats()
    expect(s.hits).toBe(0)
    expect(s.misses).toBe(0)
    expect(c.get('a')).toBeNull()
  })

  it('should track size from underlying LRU', () => {
    const c = useCache<string>({ maxSize: 10, ttl: 60000 })
    c.set('a', '1')
    c.set('b', '2')
    expect(c.stats().size).toBe(2)
  })
})

describe('createCacheKey', () => {
  it('should join parts with colon', () => {
    expect(createCacheKey('resources', 'project', 'table')).toBe('resources:project:table')
  })

  it('should handle null and undefined', () => {
    expect(createCacheKey('resources', null, undefined)).toBe('resources:null:null')
  })

  it('should handle numbers and booleans', () => {
    expect(createCacheKey('page', 3, true)).toBe('page:3:true')
  })
})
