import { describe, it, expect, beforeEach } from 'vitest'

import {
  SearchIndex,
  type SearchIndexEntry,
} from '../../src/extensions/builtin/database/ui/utils/search-index'

describe('SearchIndex', () => {
  let index: SearchIndex

  beforeEach(() => {
    index = new SearchIndex()
  })

  describe('add', () => {
    it('should add an entry to the index', () => {
      const entry: SearchIndexEntry = {
        nodeId: 'test-node',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['users', 'user_table'],
      }

      index.add(entry)
      expect(index.size()).toBe(1)
      expect(index.getEntry('test-node')).toEqual(entry)
    })

    it('should index all tokens from labels', () => {
      const entry: SearchIndexEntry = {
        nodeId: 'test-node',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['users'],
      }

      index.add(entry)

      const results = index.searchSimple('user')
      expect(results).toContain('test-node')

      const partialResults = index.searchSimple('sers')
      expect(partialResults).toContain('test-node')
    })
  })

  describe('remove', () => {
    it('should remove an entry from the index', () => {
      const entry: SearchIndexEntry = {
        nodeId: 'test-node',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['users'],
      }

      index.add(entry)
      expect(index.size()).toBe(1)

      index.remove('test-node')
      expect(index.size()).toBe(0)
      expect(index.getEntry('test-node')).toBeUndefined()
    })

    it('should not throw when removing non-existent entry', () => {
      expect(() => index.remove('non-existent')).not.toThrow()
    })
  })

  describe('search', () => {
    it('should return empty array for empty query', () => {
      expect(index.searchSimple('')).toEqual([])
      expect(index.searchSimple('   ')).toEqual([])
    })

    it('should find exact matches', () => {
      index.add({
        nodeId: 'users',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['users'],
      })
      index.add({
        nodeId: 'orders',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['orders'],
      })

      const results = index.searchSimple('users')
      expect(results).toEqual(['users'])
    })

    it('should support partial matches', () => {
      index.add({
        nodeId: 'users',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['users'],
      })
      index.add({
        nodeId: 'user_profiles',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['user_profiles'],
      })

      const results = index.searchSimple('user')
      expect(results).toContain('users')
      expect(results).toContain('user_profiles')
    })

    it('should support Chinese characters', () => {
      index.add({
        nodeId: '用户表',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['用户表', 'user_table'],
      })

      const results = index.searchSimple('用户')
      expect(results).toContain('用户表')
    })

    it('should return results sorted by score', () => {
      index.add({
        nodeId: 'user',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['user'],
      })
      index.add({
        nodeId: 'users',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['users'],
      })
      index.add({
        nodeId: 'user_profiles',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['user_profiles'],
      })

      const results = index.search('user')
      expect(results.length).toBe(3)
      expect(results[0].nodeId).toBe('user')
      expect(results[1].nodeId).toBe('users')
      expect(results[2].nodeId).toBe('user_profiles')
    })

    it('should return highlights info', () => {
      index.add({
        nodeId: 'users',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['users'],
      })

      const results = index.search('user')
      expect(results.length).toBe(1)
      expect(results[0].highlights.length).toBe(1)
      expect(results[0].highlights[0].label).toBe('users')
      expect(results[0].highlights[0].matchPositions).toEqual([{ start: 0, length: 4 }])
    })
  })

  describe('clear', () => {
    it('should clear all entries', () => {
      index.add({
        nodeId: 'users',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['users'],
      })
      index.add({
        nodeId: 'orders',
        nodeType: 'table',
        connectionId: 'conn-1',
        labels: ['orders'],
      })

      expect(index.size()).toBe(2)
      index.clear()
      expect(index.size()).toBe(0)
    })
  })
})
