import type {
  ShortcutScope,
  ShortcutRegistration,
  IShortcutManager,
} from '@/extensions/builtin/workbench/types/editor-types'

function buildKeyCombo(e: KeyboardEvent): string {
  const parts: string[] = []
  if (e.ctrlKey || e.metaKey) parts.push('Ctrl')
  if (e.altKey) parts.push('Alt')
  if (e.shiftKey && e.key.length === 1) parts.push('Shift')
  const key = e.key === ' ' ? 'Space' : e.key.length === 1 ? e.key.toUpperCase() : e.key
  parts.push(key)
  return parts.join('+')
}

class ShortcutManagerImpl implements IShortcutManager {
  private registrations: ShortcutRegistration[] = []
  private _activeScope: ShortcutScope = 'none'

  get activeScope(): ShortcutScope {
    return this._activeScope
  }

  register(key: string, scope: ShortcutScope, handler: () => void, desc: string): void {
    const existing = this.registrations.findIndex(r => r.key === key && r.scope === scope)
    if (existing >= 0) {
      this.registrations[existing] = { key, scope, handler, description: desc }
      return
    }
    this.registrations.push({ key, scope, handler, description: desc })
  }

  unregister(key: string): void {
    this.registrations = this.registrations.filter(r => r.key !== key)
  }

  unregisterByScope(scope: ShortcutScope): void {
    this.registrations = this.registrations.filter(r => r.scope !== scope)
  }

  setActiveScope(scope: ShortcutScope): void {
    this._activeScope = scope
  }

  handleKeydown(e: KeyboardEvent): void {
    const combo = buildKeyCombo(e)

    const match = this.registrations.find(
      r => r.key === combo && (r.scope === this._activeScope || r.scope === 'global')
    )

    if (match) {
      e.preventDefault()
      e.stopPropagation()
      match.handler()
    }
  }

  getAllRegistrations(): ShortcutRegistration[] {
    return [...this.registrations]
  }
}

export const ShortcutManager = new ShortcutManagerImpl()
