import type { ScratchpadEntry } from '../../types'
import type { Ref, ComputedRef } from 'vue'

export interface KeyboardDeps {
  contextMenuVisible: Ref<boolean>
  showExplorerFilter: Ref<boolean>
  selectedKey: Ref<string | null>
  selectedKeys: Ref<Set<string>>
  multiSelected: ComputedRef<number>
  flattenedEntries: ComputedRef<{ entry: ScratchpadEntry; depth: number }[]>
}

export interface KeyboardCallbacks {
  onCloseContextMenu: () => void
  onToggleSearch: () => void
  onCreateFile: () => void
  onSelectAll: () => void
  onNavigateToEntry: (entry: ScratchpadEntry) => void
  onOpenEntry: (entry: ScratchpadEntry) => void
  onRenameEntry: (entry: ScratchpadEntry) => void
  onDeleteEntry: (entry: ScratchpadEntry) => void
  onBatchDelete: (paths: string[]) => void
  onEscCloseSearch: () => void
}

export function useKeyboard(deps: KeyboardDeps, callbacks: KeyboardCallbacks) {
  const {
    contextMenuVisible,
    showExplorerFilter,
    selectedKey,
    selectedKeys,
    multiSelected,
    flattenedEntries,
  } = deps

  async function handleKeydown(event: KeyboardEvent): Promise<void> {
    const el = document.activeElement
    const tag = (el as HTMLElement)?.tagName || ''
    if (
      tag === 'INPUT' ||
      tag === 'TEXTAREA' ||
      tag === 'SELECT' ||
      (el as HTMLElement)?.getAttribute('contenteditable') === 'true'
    ) {
      return
    }
    if ((el as HTMLElement)?.closest('.cm-editor')) {
      return
    }

    if (event.key === 'Escape') {
      if (contextMenuVisible.value) {
        callbacks.onCloseContextMenu()
        return
      }
      if (showExplorerFilter.value) {
        callbacks.onEscCloseSearch()
        return
      }
    }

    if ((event.ctrlKey || event.metaKey) && event.key === 'f') {
      event.preventDefault()
      if (!showExplorerFilter.value) {
        callbacks.onToggleSearch()
      }
      return
    }

    if ((event.ctrlKey || event.metaKey) && event.key === 'n') {
      event.preventDefault()
      callbacks.onCreateFile()
      return
    }

    if ((event.ctrlKey || event.metaKey) && event.key === 'a') {
      event.preventDefault()
      callbacks.onSelectAll()
      return
    }

    if (!selectedKey.value) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        const entries = flattenedEntries.value.map(item => item.entry)
        if (entries.length > 0) {
          callbacks.onNavigateToEntry(
            event.key === 'ArrowDown' ? entries[0] : entries[entries.length - 1]
          )
        }
      }
      return
    }

    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const entries = flattenedEntries.value.map(item => item.entry)
      const idx = entries.findIndex(e => e.path === selectedKey.value)
      if (idx === -1) return
      const nextIdx =
        event.key === 'ArrowDown' ? Math.min(idx + 1, entries.length - 1) : Math.max(idx - 1, 0)
      callbacks.onNavigateToEntry(entries[nextIdx])
      return
    }

    if (event.key === 'Enter') {
      event.preventDefault()
      const entries = flattenedEntries.value.map(item => item.entry)
      const entry = entries.find(e => e.path === selectedKey.value)
      if (entry) callbacks.onOpenEntry(entry)
      return
    }

    if (event.key === 'F2') {
      event.preventDefault()
      const entries = flattenedEntries.value.map(item => item.entry)
      const entry = entries.find(e => e.path === selectedKey.value)
      if (entry) callbacks.onRenameEntry(entry)
    } else if (event.key === 'Delete') {
      event.preventDefault()
      if (multiSelected.value > 1) {
        const paths = [...selectedKeys.value]
        callbacks.onBatchDelete(paths)
        selectedKeys.value = new Set<string>()
      } else {
        const entries = flattenedEntries.value.map(item => item.entry)
        const entry = entries.find(e => e.path === selectedKey.value)
        if (entry) callbacks.onDeleteEntry(entry)
      }
    }
  }

  return {
    handleKeydown,
  }
}
