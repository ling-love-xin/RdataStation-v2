import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useCommandStore } from './command-store'

describe('CommandStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('should register a command', () => {
    const store = useCommandStore()
    const action = vi.fn()

    store.register({
      id: 'test-cmd',
      label: 'Test Command',
      category: 'test',
      action,
    })

    expect(store.allCommands).toHaveLength(1)
    expect(store.allCommands[0].id).toBe('test-cmd')
  })

  it('should unregister a command', () => {
    const store = useCommandStore()
    store.register({ id: 'cmd1', label: 'Cmd 1', category: 'test', action: vi.fn() })
    store.register({ id: 'cmd2', label: 'Cmd 2', category: 'test', action: vi.fn() })

    store.unregister('cmd1')

    expect(store.allCommands).toHaveLength(1)
    expect(store.allCommands[0].id).toBe('cmd2')
  })

  it('should execute a command and add to recent', () => {
    const store = useCommandStore()
    const action = vi.fn()

    store.register({ id: 'exec-cmd', label: 'Exec', category: 'test', action })
    store.execute('exec-cmd')

    expect(action).toHaveBeenCalledTimes(1)
    expect(store.recentCommandList).toHaveLength(1)
    expect(store.recentCommandList[0].id).toBe('exec-cmd')
  })

  it('should limit recent commands to 5', () => {
    const store = useCommandStore()

    for (let i = 1; i <= 7; i++) {
      store.register({ id: `cmd${i}`, label: `Cmd ${i}`, category: 'test', action: vi.fn() })
      store.execute(`cmd${i}`)
    }

    expect(store.recentCommandList).toHaveLength(5)
    expect(store.recentCommandList[0].id).toBe('cmd7')
  })

  it('should search commands by label', () => {
    const store = useCommandStore()
    store.register({ id: 'new-query', label: 'New Query', category: 'file', action: vi.fn() })
    store.register({ id: 'open-project', label: 'Open Project', category: 'file', action: vi.fn() })
    store.register({ id: 'settings', label: 'Settings', category: 'tools', action: vi.fn() })

    const results = store.search('new')

    expect(results).toHaveLength(1)
    expect(results[0].id).toBe('new-query')
  })

  it('should search commands by category', () => {
    const store = useCommandStore()
    store.register({ id: 'cmd1', label: 'Cmd 1', category: 'tools', action: vi.fn() })
    store.register({ id: 'cmd2', label: 'Cmd 2', category: 'file', action: vi.fn() })

    const results = store.search('tools')

    expect(results).toHaveLength(1)
    expect(results[0].id).toBe('cmd1')
  })

  it('should return recent commands when search query is empty', () => {
    const store = useCommandStore()
    store.register({ id: 'cmd1', label: 'Cmd 1', category: 'test', action: vi.fn() })
    store.register({ id: 'cmd2', label: 'Cmd 2', category: 'test', action: vi.fn() })
    store.execute('cmd1')

    const results = store.search('')

    expect(results).toHaveLength(1)
    expect(results[0].id).toBe('cmd1')
  })

  it('should return all commands when search query is empty and no recent', () => {
    const store = useCommandStore()
    store.register({ id: 'cmd1', label: 'Cmd 1', category: 'test', action: vi.fn() })
    store.register({ id: 'cmd2', label: 'Cmd 2', category: 'test', action: vi.fn() })

    const results = store.search('')

    expect(results).toHaveLength(2)
  })

  it('should sort search results with prefix match first', () => {
    const store = useCommandStore()
    store.register({ id: 'open', label: 'Open', category: 'file', action: vi.fn() })
    store.register({ id: 'new-open', label: 'New Open', category: 'file', action: vi.fn() })

    const results = store.search('open')

    expect(results[0].id).toBe('open')
    expect(results[1].id).toBe('new-open')
  })

  it('should group commands by category', () => {
    const store = useCommandStore()
    store.register({ id: 'cmd1', label: 'Cmd 1', category: 'file', action: vi.fn() })
    store.register({ id: 'cmd2', label: 'Cmd 2', category: 'file', action: vi.fn() })
    store.register({ id: 'cmd3', label: 'Cmd 3', category: 'tools', action: vi.fn() })

    const grouped = store.commandsByCategory

    expect(grouped.get('file')).toHaveLength(2)
    expect(grouped.get('tools')).toHaveLength(1)
  })

  it('should not execute non-existent command', () => {
    const store = useCommandStore()

    expect(() => store.execute('non-existent')).not.toThrow()
    expect(store.recentCommandList).toHaveLength(0)
  })

  it('should update existing command on duplicate register', () => {
    const store = useCommandStore()
    const action1 = vi.fn()
    const action2 = vi.fn()

    store.register({ id: 'dup', label: 'Original', category: 'test', action: action1 })
    store.register({ id: 'dup', label: 'Updated', category: 'test', action: action2 })

    expect(store.allCommands).toHaveLength(1)
    expect(store.allCommands[0].label).toBe('Updated')

    store.execute('dup')
    expect(action2).toHaveBeenCalledTimes(1)
    expect(action1).not.toHaveBeenCalled()
  })
})
