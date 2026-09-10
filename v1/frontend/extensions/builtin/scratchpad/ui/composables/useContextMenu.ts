import {
  FilePlus,
  FolderPlus,
  FileText,
  BarChart3,
  ChevronRight,
  Pencil,
  Copy,
  Trash2,
  GitBranch,
  Scissors,
  ClipboardPaste,
  ExternalLink,
  X,
} from 'lucide-vue-next'
import { reactive, type Ref, type ComputedRef } from 'vue'

import type { ScratchpadEntry, ExternalReference } from '../../types'

export interface ContextMenuItem {
  key: string
  label: string
  icon?: typeof FileText
  danger?: boolean
  disabled?: boolean
  shortcut?: string
  type?: 'divider'
}

export interface ContextMenuState {
  visible: boolean
  x: number
  y: number
  target: ScratchpadEntry | ExternalReference | null
  isRefTarget: boolean
  isBlankTarget: boolean
  items: ContextMenuItem[]
}

export interface ContextMenuDeps {
  t: (key: string, params?: Record<string, unknown>) => string
  selectedKeys: Ref<Set<string>>
  expandedKeys: Ref<Set<string>>
  clipboardEntry: Ref<ScratchpadEntry | null>
  multiSelected: ComputedRef<number>
}

export interface ContextMenuCallbacks {
  onNewFile: (parentPath: string | null) => void
  onNewFolder: (parentPath: string | null) => void
  onOpen: (entry: ScratchpadEntry) => void
  onAnalyzeDuckDB: (entry: ScratchpadEntry) => void
  onToggleFolder: (entry: ScratchpadEntry) => void
  onRename: (entry: ScratchpadEntry) => void
  onDelete: (entry: ScratchpadEntry) => void
  onBatchDelete: (paths: string[]) => void
  onCopyFile: (entry: ScratchpadEntry) => void
  onCutFile: (entry: ScratchpadEntry) => void
  onPasteFile: () => Promise<void>
  onPromote: (entry: ScratchpadEntry) => void
  onCopyPath: (path: string) => void
  onCopyAbsPath: (path: string) => void
  onRemoveRef: (alias: string) => void
  onOpenRefLocation: (path: string) => void
}

const ANALYZABLE_EXTENSIONS = ['.csv', '.parquet', '.json', '.xlsx', '.duckdb']

function isAnalyzableFile(entry: ScratchpadEntry): boolean {
  const ext = entry.name.includes('.') ? '.' + entry.name.split('.').pop()?.toLowerCase() : ''
  return ANALYZABLE_EXTENSIONS.includes(ext)
}

function clampToViewport(
  x: number,
  y: number,
  menuWidth: number,
  menuHeight: number
): { x: number; y: number } {
  const w = window.innerWidth
  const h = window.innerHeight
  return {
    x: Math.min(x, w - menuWidth),
    y: Math.min(y, h - menuHeight),
  }
}

export function useContextMenu(deps: ContextMenuDeps, callbacks: ContextMenuCallbacks) {
  const { t, selectedKeys, expandedKeys, clipboardEntry, multiSelected } = deps

  const contextMenu = reactive<ContextMenuState>({
    visible: false,
    x: 0,
    y: 0,
    target: null,
    isRefTarget: false,
    isBlankTarget: false,
    items: [],
  })

  function closeContextMenu(): void {
    contextMenu.visible = false
    contextMenu.target = null
  }

  function showBlankMenu(event: MouseEvent): void {
    event.preventDefault()
    event.stopPropagation()
    const pos = clampToViewport(event.clientX, event.clientY, 180, 100)
    contextMenu.x = pos.x
    contextMenu.y = pos.y
    contextMenu.isRefTarget = false
    contextMenu.isBlankTarget = true
    contextMenu.target = null
    contextMenu.items = [
      { key: 'new-file', label: t('scratchpad.newFile'), icon: FilePlus },
      { key: 'new-folder', label: t('scratchpad.newFolder'), icon: FolderPlus },
    ]
    contextMenu.visible = true
  }

  function showEntryMenu(event: MouseEvent, entry: ScratchpadEntry): void {
    event.preventDefault()
    event.stopPropagation()

    if (!selectedKeys.value.has(entry.path)) {
      selectedKeys.value = new Set<string>([entry.path])
    }

    const multi = multiSelected.value > 1
    const pos = clampToViewport(event.clientX, event.clientY, 180, 240)
    contextMenu.x = pos.x
    contextMenu.y = pos.y
    contextMenu.isRefTarget = false
    contextMenu.isBlankTarget = false
    contextMenu.target = entry
    contextMenu.items = multi
      ? [
          {
            key: 'batch-delete',
            label: t('scratchpad.batchDelete', { n: multiSelected.value }),
            icon: Trash2,
            danger: true,
          },
        ]
      : [
          { key: 'new-file', label: t('scratchpad.newFile'), icon: FilePlus },
          { key: 'new-folder', label: t('scratchpad.newFolder'), icon: FolderPlus },
          { type: 'divider' as const, key: 'menu-d1', label: '' },
          { key: 'open', label: t('scratchpad.open'), icon: FileText },
          ...(isAnalyzableFile(entry)
            ? [
                {
                  key: 'analyze-duckdb',
                  label: t('scratchpad.analyzeWithDuckDB'),
                  icon: BarChart3,
                },
              ]
            : []),
          ...(entry.kind === 'folder'
            ? [
                {
                  key: 'toggle-folder',
                  label: expandedKeys.value.has(entry.path)
                    ? t('scratchpad.collapse')
                    : t('scratchpad.expand'),
                  icon: ChevronRight,
                },
              ]
            : []),
          { type: 'divider' as const, key: 'menu-d2', label: '' },
          { key: 'copy-file', label: t('scratchpad.copyFile'), icon: Copy },
          { key: 'cut-file', label: t('scratchpad.cutFile'), icon: Scissors },
          {
            key: 'paste-file',
            label: t('scratchpad.pasteFile'),
            icon: ClipboardPaste,
            disabled: !clipboardEntry.value,
          },
          { type: 'divider' as const, key: 'menu-d3', label: '' },
          { key: 'rename', label: t('scratchpad.rename'), icon: Pencil, shortcut: 'F2' },
          { key: 'copy-path', label: t('scratchpad.copyPath'), icon: Copy },
          { key: 'copy-abs-path', label: t('scratchpad.copyAbsolutePath'), icon: Copy },
          { type: 'divider' as const, key: 'menu-d4', label: '' },
          { key: 'promote', label: t('scratchpad.promoteToResource'), icon: GitBranch },
          { type: 'divider' as const, key: 'menu-d5', label: '' },
          {
            key: 'delete',
            label: t('scratchpad.delete'),
            icon: Trash2,
            danger: true,
            shortcut: 'Del',
          },
        ]
    contextMenu.visible = true
  }

  function showRefMenu(event: MouseEvent, ref: ExternalReference): void {
    event.preventDefault()
    event.stopPropagation()
    const pos = clampToViewport(event.clientX, event.clientY, 180, 100)
    contextMenu.x = pos.x
    contextMenu.y = pos.y
    contextMenu.isRefTarget = true
    contextMenu.isBlankTarget = false
    contextMenu.target = ref
    contextMenu.items = [
      { key: 'open-ref-location', label: t('scratchpad.openLocation'), icon: ExternalLink },
      { key: 'remove-ref', label: t('scratchpad.removeReference'), icon: X, danger: true },
    ]
    contextMenu.visible = true
  }

  function getParentPathOfEntry(path: string): string | null {
    const normalized = path.replace(/\\/g, '/')
    const lastSlash = normalized.lastIndexOf('/')
    return lastSlash > 0 ? normalized.substring(0, lastSlash) : null
  }

  async function handleMenuAction(key: string): Promise<void> {
    const isRefTarget = contextMenu.isRefTarget
    const isBlankTarget = contextMenu.isBlankTarget
    const target = contextMenu.target
    closeContextMenu()

    if (key === 'new-file') {
      let parentPath: string | null = null
      if (!isBlankTarget && !isRefTarget && target) {
        const entry = target as ScratchpadEntry
        parentPath = entry.kind === 'folder' ? entry.path : getParentPathOfEntry(entry.path)
      }
      callbacks.onNewFile(parentPath)
      return
    }

    if (key === 'new-folder') {
      let parentPath: string | null = null
      if (!isBlankTarget && !isRefTarget && target) {
        const entry = target as ScratchpadEntry
        parentPath = entry.kind === 'folder' ? entry.path : getParentPathOfEntry(entry.path)
      }
      callbacks.onNewFolder(parentPath)
      return
    }

    if (isRefTarget) {
      const ref = target as ExternalReference
      if (ref) {
        if (key === 'remove-ref') {
          callbacks.onRemoveRef(ref.alias)
        } else if (key === 'open-ref-location') {
          callbacks.onOpenRefLocation(ref.path)
        }
      }
      return
    }

    const entry = target as ScratchpadEntry
    if (!entry) return
    switch (key) {
      case 'open':
        callbacks.onOpen(entry)
        break
      case 'analyze-duckdb':
        callbacks.onAnalyzeDuckDB(entry)
        break
      case 'toggle-folder':
        callbacks.onToggleFolder(entry)
        break
      case 'rename':
        callbacks.onRename(entry)
        break
      case 'copy-path':
        callbacks.onCopyPath(entry.path)
        break
      case 'copy-abs-path':
        callbacks.onCopyAbsPath(entry.path)
        break
      case 'delete':
        callbacks.onDelete(entry)
        break
      case 'promote':
        callbacks.onPromote(entry)
        break
      case 'copy-file':
        callbacks.onCopyFile(entry)
        break
      case 'cut-file':
        callbacks.onCutFile(entry)
        break
      case 'paste-file':
        await callbacks.onPasteFile()
        break
      case 'batch-delete': {
        const paths = [...selectedKeys.value]
        callbacks.onBatchDelete(paths)
        break
      }
    }
  }

  return {
    contextMenu,
    showBlankMenu,
    showEntryMenu,
    showRefMenu,
    closeContextMenu,
    handleMenuAction,
    getParentPathOfEntry,
    isAnalyzableFile,
  }
}
