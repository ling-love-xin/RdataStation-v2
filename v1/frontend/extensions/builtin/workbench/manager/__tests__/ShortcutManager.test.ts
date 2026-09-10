import { describe, it, expect, beforeEach, vi } from 'vitest'

// 直接测试 ShortcutManagerImpl 类，避免单例状态污染
import { ShortcutManager } from '../ShortcutManager'

describe('ShortcutManager', () => {
  beforeEach(() => {
    // 清理所有注册
    ShortcutManager.setActiveScope('none')
    const all = ShortcutManager.getAllRegistrations()
    for (const r of all) {
      ShortcutManager.unregister(r.key)
    }
  })

  it('register and retrieve by key', () => {
    const handler = () => {}
    ShortcutManager.register('Ctrl+S', 'editor', handler, 'Save')
    const all = ShortcutManager.getAllRegistrations()
    expect(all).toHaveLength(1)
    expect(all[0]).toMatchObject({ key: 'Ctrl+S', scope: 'editor', description: 'Save' })
  })

  it('register with same key+scope replaces existing', () => {
    ShortcutManager.register('Ctrl+S', 'editor', () => {}, 'old')
    ShortcutManager.register('Ctrl+S', 'editor', () => {}, 'new')
    const all = ShortcutManager.getAllRegistrations()
    expect(all).toHaveLength(1)
    expect(all[0].description).toBe('new')
  })

  it('register with same key but different scope adds new', () => {
    ShortcutManager.register('Ctrl+S', 'editor', () => {}, 'editor')
    ShortcutManager.register('Ctrl+S', 'global', () => {}, 'global')
    const all = ShortcutManager.getAllRegistrations()
    expect(all).toHaveLength(2)
  })

  it('unregister by key removes all scopes', () => {
    ShortcutManager.register('Ctrl+S', 'editor', () => {}, 'editor')
    ShortcutManager.register('Ctrl+S', 'global', () => {}, 'global')
    ShortcutManager.unregister('Ctrl+S')
    const all = ShortcutManager.getAllRegistrations()
    expect(all).toHaveLength(0)
  })

  it('unregisterByScope removes only matching scope', () => {
    ShortcutManager.register('Ctrl+S', 'editor', () => {}, 'save')
    ShortcutManager.register('Ctrl+Enter', 'editor', () => {}, 'execute')
    ShortcutManager.register('Ctrl+O', 'global', () => {}, 'open')
    ShortcutManager.unregisterByScope('editor')
    const all = ShortcutManager.getAllRegistrations()
    expect(all).toHaveLength(1)
    expect(all[0].scope).toBe('global')
  })

  it('setActiveScope changes the active scope', () => {
    expect(ShortcutManager.activeScope).toBe('none')
    ShortcutManager.setActiveScope('editor')
    expect(ShortcutManager.activeScope).toBe('editor')
  })

  it('handleKeydown triggers matching scope handler', () => {
    let called = false
    ShortcutManager.register(
      'Ctrl+Enter',
      'editor',
      () => {
        called = true
      },
      'execute'
    )
    ShortcutManager.setActiveScope('editor')

    const event = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true })
    ShortcutManager.handleKeydown(event)
    expect(called).toBe(true)
  })

  it('handleKeydown triggers global scope handler regardless of activeScope', () => {
    let called = false
    ShortcutManager.register(
      'Ctrl+Enter',
      'global',
      () => {
        called = true
      },
      'global execute'
    )
    ShortcutManager.setActiveScope('none')

    const event = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true })
    ShortcutManager.handleKeydown(event)
    expect(called).toBe(true)
  })

  it('handleKeydown does not trigger handler from non-matching scope', () => {
    let called = false
    ShortcutManager.register(
      'Ctrl+Enter',
      'editor',
      () => {
        called = true
      },
      'execute'
    )
    ShortcutManager.setActiveScope('result')

    const event = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true })
    ShortcutManager.handleKeydown(event)
    expect(called).toBe(false)
  })

  it('getAllRegistrations returns a copy (not reference)', () => {
    ShortcutManager.register('Ctrl+S', 'editor', () => {}, 'save')
    const copy = ShortcutManager.getAllRegistrations()
    copy.push({ key: 'Ctrl+X', scope: 'editor', handler: () => {}, description: 'fake' })
    const original = ShortcutManager.getAllRegistrations()
    expect(original).toHaveLength(1)
  })

  it('handleKeydown prevents default and stops propagation on match', () => {
    ShortcutManager.register('Ctrl+S', 'editor', () => {}, 'save')
    ShortcutManager.setActiveScope('editor')

    const event = new KeyboardEvent('keydown', { key: 's', ctrlKey: true, bubbles: true })
    const preventDefaultSpy = vi.spyOn(event, 'preventDefault')
    const stopPropagationSpy = vi.spyOn(event, 'stopPropagation')

    ShortcutManager.handleKeydown(event)
    expect(preventDefaultSpy).toHaveBeenCalled()
    expect(stopPropagationSpy).toHaveBeenCalled()
  })
})
