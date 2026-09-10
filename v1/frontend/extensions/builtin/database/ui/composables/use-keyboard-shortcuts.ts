/**
 * 键盘快捷键处理器
 *
 * 实现数据库导航栏的键盘快捷键支持
 */

import { onMounted, onUnmounted } from 'vue'

export interface KeyboardShortcutHandlers {
  onNewConnection?: () => void
  onDisconnect?: () => void
  onRefresh?: () => void
  onSearch?: () => void
  onBeginTransaction?: () => void
  onCommitTransaction?: () => void
  onRollbackTransaction?: () => void
  onToggleExpand?: () => void
  onDelete?: () => void
}

export function useKeyboardShortcuts(handlers: KeyboardShortcutHandlers) {
  function handleKeyDown(event: KeyboardEvent) {
    const ctrl = event.ctrlKey || event.metaKey
    const shift = event.shiftKey

    if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) {
      return
    }

    if ((event.target as HTMLElement)?.closest?.('.cm-editor')) {
      return
    }

    if (ctrl && !shift && event.key === 'n') {
      event.preventDefault()
      handlers.onNewConnection?.()
    }

    if (ctrl && !shift && event.key === 'd') {
      event.preventDefault()
      handlers.onDisconnect?.()
    }

    if (ctrl && !shift && event.key === 'r') {
      event.preventDefault()
      handlers.onRefresh?.()
    }

    if (ctrl && !shift && event.key === 'f') {
      event.preventDefault()
      handlers.onSearch?.()
    }

    if (ctrl && !shift && event.key === 'b') {
      event.preventDefault()
      handlers.onBeginTransaction?.()
    }

    if (ctrl && shift && event.key === 'b') {
      event.preventDefault()
      handlers.onCommitTransaction?.()
    }

    if (ctrl && shift && event.key === 'r') {
      event.preventDefault()
      handlers.onRollbackTransaction?.()
    }

    if (event.key === 'Enter') {
      handlers.onToggleExpand?.()
    }

    if (event.key === 'Delete') {
      event.preventDefault()
      handlers.onDelete?.()
    }
  }

  onMounted(() => {
    window.addEventListener('keydown', handleKeyDown)
  })

  onUnmounted(() => {
    window.removeEventListener('keydown', handleKeyDown)
  })
}
