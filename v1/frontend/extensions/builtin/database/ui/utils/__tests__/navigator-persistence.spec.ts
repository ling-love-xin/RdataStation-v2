import { describe, it, expect, beforeEach } from 'vitest'

import {
  saveConnectionNavigatorState,
  getConnectionNavigatorState,
  saveLastActiveConnection,
  getLastActiveConnection,
  clearAllNavigatorStates,
} from '../navigator-persistence'

describe('navigator-persistence', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('should save and restore navigator state', () => {
    saveConnectionNavigatorState('conn-1', {
      expandedKeys: ['a', 'b'],
      selectedKey: 'c',
      filterText: 'test',
    })

    const state = getConnectionNavigatorState('conn-1')
    expect(state).not.toBeNull()
    expect(state?.expandedKeys).toEqual(['a', 'b'])
    expect(state?.selectedKey).toBe('c')
    expect(state?.filterText).toBe('test')
    expect(state?.version).toBe(1)
  })

  it('should save and restore last active connection', () => {
    saveLastActiveConnection('conn-abc', 'global')

    const entry = getLastActiveConnection()
    expect(entry).not.toBeNull()
    expect(entry?.connId).toBe('conn-abc')
    expect(entry?.scope).toBe('global')
  })

  it('should return null for non-existent state', () => {
    const state = getConnectionNavigatorState('non-existent')
    expect(state).toBeNull()
  })

  it('should return null for corrupted JSON', () => {
    localStorage.setItem('rds:navigator:global:bad', 'not-json{{{')

    const state = getConnectionNavigatorState('bad')
    expect(state).toBeNull()
  })

  it('should clear all navigator states', () => {
    saveConnectionNavigatorState('conn-1', { expandedKeys: ['a'] })
    saveConnectionNavigatorState('conn-2', { expandedKeys: ['b'] })
    saveLastActiveConnection('conn-1', 'global')

    clearAllNavigatorStates()

    expect(getConnectionNavigatorState('conn-1')).toBeNull()
    expect(getConnectionNavigatorState('conn-2')).toBeNull()
    // Last active connection should NOT be cleared (different key prefix)
    // Actually it uses LAST_ACTIVE_KEY which is 'rds:navigator:lastActive',
    // which starts with STORAGE_KEY_PREFIX, so it WILL be cleared too.
    expect(getLastActiveConnection()).toBeNull()
  })
})
