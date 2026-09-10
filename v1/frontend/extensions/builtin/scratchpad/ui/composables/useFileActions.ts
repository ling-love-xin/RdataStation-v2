import { open as openDialog } from '@tauri-apps/plugin-dialog'

import type { useScratchpadEditorStore } from '@/stores/useScratchpadEditorStore'

import type { ExternalReference, PromoteResult, ScratchpadEntry } from '../../types'
import type { Ref } from 'vue'

export interface ScratchpadApi {
  createEntry: (
    name: string,
    isFolder: boolean,
    parentPath?: string
  ) => Promise<ScratchpadEntry | null>
  deleteEntry: (path: string) => Promise<boolean>
  renameEntry: (path: string, newName: string) => Promise<ScratchpadEntry | null>
  importFile: (path: string) => Promise<ScratchpadEntry | null>
  promoteToResource: (relativePath: string, removeAfter: boolean) => Promise<PromoteResult | null>
  emptyTrashBin: () => Promise<boolean>
  restoreTrashEntry: (name: string) => Promise<boolean>
  addReference: (alias: string, path: string) => Promise<ExternalReference | null>
  removeReference: (alias: string) => Promise<boolean>
  loadFileContent: (relativePath: string) => Promise<string | null>
  setContentSnapshot: (relativePath: string, content: string) => void
  error: Ref<string | null>
}

export interface FileActionsDeps {
  api: ScratchpadApi
  store: ReturnType<typeof useScratchpadEditorStore>
  message: {
    success: (msg: string) => void
    error: (msg: string) => void
    info: (msg: string) => void
    warning: (msg: string) => void
  }
  t: (key: string, params?: Record<string, unknown>) => string
  expandedKeys: Ref<Set<string>>
  scratchpadPath: Ref<string>
  fileMeta: Ref<Record<string, { last_connection_id?: string }>>
  newRefAlias: Ref<string>
  newRefPath: Ref<string>
  showRefModal: Ref<boolean>
  showPromoteConfirm: Ref<boolean>
  promoteTarget: Ref<ScratchpadEntry | null>
  showConflictDialog: Ref<boolean>
  conflictFilePath: Ref<string | null>
}

export function useFileActions(deps: FileActionsDeps) {
  const { api, store, message, t, expandedKeys, scratchpadPath, fileMeta } = deps

  async function confirmCreate(
    name: string,
    isFolder: boolean,
    parentPath: string | null
  ): Promise<ScratchpadEntry | null> {
    if (!name) return null
    const entry = await api.createEntry(name, isFolder, parentPath || undefined)
    if (entry) {
      if (parentPath) {
        expandedKeys.value = new Set([...expandedKeys.value, parentPath])
      }
      message.success(t('scratchpad.createdSuccess', { name }))
    }
    return entry
  }

  async function doImportFile(): Promise<ScratchpadEntry | null> {
    try {
      const selected = await openDialog({
        multiple: false,
        title: t('scratchpad.selectFileToImport'),
        filters: [{ name: t('scratchpad.allFiles'), extensions: ['*'] }],
      })
      if (selected && typeof selected === 'string') {
        const result = await api.importFile(selected)
        if (result) {
          message.success(t('scratchpad.importedSuccess', { name: result.name }))
        }
        return result
      }
    } catch {
      message.warning(t('scratchpad.dialogNotAvailable'))
    }
    return null
  }

  async function doDelete(path: string): Promise<boolean> {
    try {
      await api.deleteEntry(path)
      store.removeOpen(path)
      return true
    } catch {
      return false
    }
  }

  async function doBatchDelete(paths: string[]): Promise<boolean> {
    try {
      for (const p of paths) {
        await api.deleteEntry(p)
        store.removeOpen(p)
      }
      return true
    } catch {
      return false
    }
  }

  async function doRename(path: string, newName: string): Promise<ScratchpadEntry | null> {
    const result = await api.renameEntry(path, newName)
    if (result) {
      message.success(t('scratchpad.renamedSuccess', { name: newName }))
      store.syncRename(path, result.path, newName)
    }
    return result
  }

  async function doPromote(entry: ScratchpadEntry, removeAfter: boolean): Promise<boolean> {
    const base = scratchpadPath.value || ''
    const relativePath = base
      ? entry.path.replace(base.replace(/\\/g, '/'), '').replace(/^\//, '')
      : entry.path
    const result = await api.promoteToResource(relativePath, removeAfter)
    if (result) {
      message.success(t('scratchpad.promotedSuccess', { name: entry.name }))
      return true
    }
    return false
  }

  function doEmptyTrash(): Promise<boolean> {
    return api
      .emptyTrashBin()
      .then(ok => {
        if (ok) message.success(t('scratchpad.trashEmptied'))
        return ok
      })
      .catch(e => {
        message.error(e instanceof Error ? e.message : String(e))
        return false
      })
  }

  function doUndoDelete(name: string): Promise<boolean> {
    return api.restoreTrashEntry(name).then(ok => {
      if (ok) message.success(t('scratchpad.restoredSuccess', { name }))
      return ok
    })
  }

  async function doAddReference(alias: string, path: string): Promise<ExternalReference | null> {
    deps.showRefModal.value = false
    const ref = await api.addReference(alias, path)
    if (ref) {
      message.success(t('scratchpad.refAdded', { alias }))
    } else {
      message.error(api.error.value || t('scratchpad.refAddFailed'))
    }
    return ref
  }

  function doRemoveReference(alias: string): Promise<boolean> {
    return api.removeReference(alias).then(ok => {
      if (ok) message.success(t('scratchpad.refRemoved', { alias }))
      return ok
    })
  }

  async function doBrowseRefPath(): Promise<void> {
    try {
      const selected = await openDialog({
        directory: true,
        multiple: false,
        title: t('scratchpad.selectRefDirectory'),
      })
      if (selected && typeof selected === 'string') {
        deps.newRefPath.value = selected
      }
    } catch {
      message.warning(t('scratchpad.dialogNotAvailable'))
    }
  }

  function openInEditor(entry: ScratchpadEntry): string {
    const ext = entry.name.includes('.') ? '.' + entry.name.split('.').pop()?.toLowerCase() : ''
    const langMap: Record<string, string> = {
      '.sql': 'sql',
      '.py': 'python',
      '.json': 'json',
      '.txt': 'plaintext',
      '.md': 'markdown',
    }
    const language = langMap[ext] || 'plaintext'

    const base = scratchpadPath.value || ''
    const relativePath = base
      ? entry.path.replace(base.replace(/\\/g, '/'), '').replace(/^\//, '')
      : entry.path

    api.loadFileContent(relativePath).then(content => {
      const resolvedContent = content === null ? '' : content

      api.setContentSnapshot(relativePath, resolvedContent)

      const metaForFile = fileMeta.value[relativePath]
      const lastConnectionId = metaForFile?.last_connection_id || ''

      window.dispatchEvent(
        new CustomEvent('open-sql-editor', {
          detail: {
            connectionId: lastConnectionId || '',
            databaseName: '',
            sql: resolvedContent,
            scratchpadRelativePath: relativePath,
            scratchpadFileName: entry.name,
            language,
          },
        })
      )
    })
    return relativePath
  }

  return {
    confirmCreate,
    doImportFile,
    doDelete,
    doBatchDelete,
    doRename,
    doPromote,
    doEmptyTrash,
    doUndoDelete,
    doAddReference,
    doRemoveReference,
    doBrowseRefPath,
    openInEditor,
  }
}
