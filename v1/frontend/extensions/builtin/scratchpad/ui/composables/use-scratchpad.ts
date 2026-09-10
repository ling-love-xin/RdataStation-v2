import { ref, computed } from 'vue'

import { extractErrorMessage } from '@/shared/utils/error'

import {
  listScratchpadFiles,
  createScratchpadEntry,
  deleteScratchpadEntry,
  renameScratchpadEntry,
  readScratchpadFile,
  saveScratchpadFile,
  importExternalFile,
  addExternalReference,
  removeExternalReference,
  openInExplorer,
  checkFileSize,
  updateFileMeta,
  searchFileContent,
  listTrash,
  restoreFromTrash,
  emptyTrash,
  getAnalyzableFiles,
  watchScratchpad,
  unwatchScratchpad,
  promoteScratchpadToResource,
  getScratchpadEntry,
  listScratchpadDirectory,
  moveScratchpadEntry,
  replaceScratchpadContent,
  diffScratchpadWithContent,
} from '../../infrastructure/api/scratchpad-api'

import type {
  ScratchpadResponse,
  ScratchpadEntry,
  ExternalReference,
  AnalyzableFile,
  PromoteResult,
  SearchResult,
  ScratchpadChangeEvent,
  ScratchpadChangeEntry,
  ReplaceResult,
  DiffResult,
} from '../../types'

export function useScratchpad() {
  const response = ref<ScratchpadResponse | null>(null)
  const isLoading = ref(false)
  const error = ref<string | null>(null)
  const notInitialized = ref(false)
  const searchQuery = ref('')
  const trashEntries = ref<ScratchpadEntry[]>([])
  const analyzableFiles = ref<AnalyzableFile[]>([])

  const localEntries = computed(() => {
    if (!response.value) return []
    const entries = response.value.local_entries
    if (!searchQuery.value) return entries
    const q = searchQuery.value.toLowerCase()
    return filterTree(entries, e => e.name.toLowerCase().includes(q))
  })

  function filterTree(
    entries: ScratchpadEntry[],
    pred: (e: ScratchpadEntry) => boolean
  ): ScratchpadEntry[] {
    const result: ScratchpadEntry[] = []
    for (const entry of entries) {
      const childMatch = entry.children ? filterTree(entry.children, pred) : []
      if (pred(entry) || childMatch.length > 0) {
        result.push({ ...entry, children: childMatch.length > 0 ? childMatch : entry.children })
      }
    }
    return result
  }

  function flattenVisibleEntries(
    entries: ScratchpadEntry[],
    expandedKeys: Set<string>,
    depth = 0
  ): { entry: ScratchpadEntry; depth: number }[] {
    const result: { entry: ScratchpadEntry; depth: number }[] = []
    const sorted = [...entries].sort((a, b) => {
      if (a.kind !== b.kind) {
        return a.kind === 'folder' ? -1 : 1
      }
      return a.name.localeCompare(b.name)
    })
    for (const entry of sorted) {
      result.push({ entry, depth })
      if (
        entry.kind === 'folder' &&
        entry.children &&
        expandedKeys.has(normalizePathForCompare(entry.path))
      ) {
        result.push(...flattenVisibleEntries(entry.children, expandedKeys, depth + 1))
      }
    }
    return result
  }

  const externalReferences = computed(() => {
    if (!response.value) return []
    const refs = response.value.external_references
    if (!searchQuery.value) return refs
    const q = searchQuery.value.toLowerCase()
    return refs.filter(r => r.alias.toLowerCase().includes(q) || r.path.toLowerCase().includes(q))
  })

  const scratchpadPath = computed(() => response.value?.scratchpad_path ?? '')

  const invalidReferences = computed(() =>
    externalReferences.value.filter(ref => {
      if (!ref.path) return true
      const pathPattern = /^([A-Za-z]:[\\/]|[/\\])/
      return !pathPattern.test(ref.path) || ref.path.includes('..')
    })
  )

  const validReferences = computed(() =>
    externalReferences.value.filter(ref => {
      if (!ref.path) return false
      const pathPattern = /^([A-Za-z]:[\\/]|[/\\])/
      return pathPattern.test(ref.path) && !ref.path.includes('..')
    })
  )

  async function loadFiles(): Promise<void> {
    isLoading.value = true
    error.value = null
    notInitialized.value = false
    try {
      response.value = await listScratchpadFiles()
    } catch (e) {
      const msg = extractErrorMessage(e)
      if (msg.includes('未初始化') || msg.includes('not initialized')) {
        await new Promise(resolve => setTimeout(resolve, 600))
        try {
          response.value = await listScratchpadFiles()
          isLoading.value = false
          return
        } catch {
          /* retry failed, fall through to notInitialized */
        }
        notInitialized.value = true
        response.value = null
      } else {
        error.value = msg
        response.value = null
      }
    } finally {
      isLoading.value = false
    }
  }

  async function createEntry(
    name: string,
    isFolder: boolean,
    parentPath?: string
  ): Promise<ScratchpadEntry | null> {
    try {
      const entry = await createScratchpadEntry(name, isFolder, parentPath)
      // eslint-disable-next-line no-console
      console.log('[createEntry] created:', entry.name, 'parentPath:', parentPath)
      if (parentPath) {
        if (!response.value) return null
        const children = await listScratchpadDirectory(parentPath)
        // eslint-disable-next-line no-console
        console.log(
          '[createEntry] loaded children for:',
          parentPath,
          'count:',
          children.length,
          'names:',
          children.map(c => c.name)
        )
        setEntryChildren(response.value.local_entries, parentPath, children)
      } else {
        await loadFiles()
      }
      return entry
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  async function deleteEntry(relativePath: string): Promise<boolean> {
    try {
      await deleteScratchpadEntry(relativePath)
      await loadFiles()
      return true
    } catch (e) {
      error.value = extractErrorMessage(e)
      return false
    }
  }

  async function renameEntry(
    relativePath: string,
    newName: string
  ): Promise<ScratchpadEntry | null> {
    try {
      const entry = await renameScratchpadEntry(relativePath, newName)
      await loadFiles()
      return entry
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  async function loadFileContent(relativePath: string): Promise<string | null> {
    try {
      return await readScratchpadFile(relativePath)
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  async function saveFile(relativePath: string, content: string): Promise<boolean> {
    try {
      await saveScratchpadFile(relativePath, content)
      return true
    } catch (e) {
      error.value = extractErrorMessage(e)
      return false
    }
  }

  async function importFile(sourcePath: string): Promise<ScratchpadEntry | null> {
    try {
      const entry = await importExternalFile(sourcePath)
      await loadFiles()
      return entry
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  async function addReference(alias: string, path: string): Promise<ExternalReference | null> {
    try {
      const ref = await addExternalReference(alias, path)
      await loadFiles()
      return ref
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  async function removeReference(alias: string): Promise<boolean> {
    try {
      await removeExternalReference(alias)
      await loadFiles()
      return true
    } catch (e) {
      error.value = extractErrorMessage(e)
      return false
    }
  }

  function isRefValid(ref: ExternalReference): boolean {
    if (!ref.path) return false
    const pathPattern = /^([A-Za-z]:[\\/]|[/\\])/
    return pathPattern.test(ref.path) && !ref.path.includes('..')
  }

  function findEntry(entryPath: string): ScratchpadEntry | undefined {
    return localEntries.value.find(e => e.path === entryPath)
  }

  async function openInExplorerAction(path: string): Promise<boolean> {
    try {
      await openInExplorer(path)
      return true
    } catch (e) {
      error.value = extractErrorMessage(e)
      return false
    }
  }

  async function getFileSize(relativePath: string): Promise<number | null> {
    try {
      return await checkFileSize(relativePath)
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  function isRefInvalid(ref: ExternalReference): boolean {
    return !isRefValid(ref)
  }

  function clearError(): void {
    error.value = null
  }

  async function loadTrashEntries(): Promise<void> {
    try {
      trashEntries.value = await listTrash()
    } catch (e) {
      error.value = extractErrorMessage(e)
    }
  }

  async function restoreTrashEntry(trashName: string): Promise<boolean> {
    try {
      await restoreFromTrash(trashName)
      await loadTrashEntries()
      await loadFiles()
      return true
    } catch (e) {
      error.value = extractErrorMessage(e)
      return false
    }
  }

  async function emptyTrashBin(): Promise<boolean> {
    try {
      await emptyTrash()
      trashEntries.value = []
      return true
    } catch (e) {
      error.value = extractErrorMessage(e)
      return false
    }
  }

  async function loadAnalyzableFiles(): Promise<void> {
    try {
      analyzableFiles.value = await getAnalyzableFiles()
    } catch (e) {
      error.value = extractErrorMessage(e)
    }
  }

  async function saveFileMeta(relativePath: string, connectionId?: string): Promise<boolean> {
    try {
      await updateFileMeta(relativePath, connectionId)
      return true
    } catch (e) {
      error.value = extractErrorMessage(e)
      return false
    }
  }

  async function searchContent(query: string, caseSensitive = false): Promise<SearchResult | null> {
    try {
      return await searchFileContent(query, caseSensitive)
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  async function startWatching(): Promise<void> {
    if (notInitialized.value) {
      return
    }
    try {
      await watchScratchpad()
    } catch (e) {
      error.value = extractErrorMessage(e)
    }
  }

  async function stopWatching(): Promise<void> {
    try {
      await unwatchScratchpad()
    } catch (e) {
      error.value = extractErrorMessage(e)
    }
  }

  async function promoteToResource(
    relativePath: string,
    removeAfter: boolean
  ): Promise<PromoteResult | null> {
    try {
      const result = await promoteScratchpadToResource(relativePath, removeAfter)
      if (removeAfter) {
        await loadFiles()
      }
      return result
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  async function applyFileChanges(event: ScratchpadChangeEvent): Promise<void> {
    if (!response.value) return

    const changes = event.changes
    const creates: ScratchpadChangeEntry[] = []
    const modifies: ScratchpadChangeEntry[] = []
    const deletes: string[] = []

    for (const change of changes) {
      switch (change.kind) {
        case 'create':
          creates.push(change)
          break
        case 'modify':
          modifies.push(change)
          break
        case 'delete':
          deletes.push(change.path)
          break
      }
    }

    let currentEntries = response.value.local_entries

    if (deletes.length > 0) {
      const deleteSet = new Set(deletes.map(p => normalizePathForCompare(p)))
      currentEntries = removeDeletedFromTree(currentEntries, deleteSet)
    }

    if (creates.length > 0) {
      // eslint-disable-next-line no-console
      console.log(
        '[applyFileChanges] create events:',
        creates.map(c => c.path)
      )
      const fetchResults = await Promise.allSettled(creates.map(c => getScratchpadEntry(c.path)))
      const newEntries: ScratchpadEntry[] = []
      for (const result of fetchResults) {
        if (result.status === 'fulfilled' && result.value) {
          newEntries.push(result.value)
        }
      }
      if (newEntries.length > 0) {
        const existingPaths = collectAllPaths(currentEntries)
        const filtered = newEntries.filter(e => !existingPaths.has(normalizePathForCompare(e.path)))
        // eslint-disable-next-line no-console
        console.log(
          '[applyFileChanges] newEntries:',
          newEntries.map(e => e.name),
          'filtered:',
          filtered.map(e => e.name),
          'existingPaths count:',
          existingPaths.size
        )
        if (filtered.length > 0) {
          // eslint-disable-next-line no-console
          console.log(
            '[applyFileChanges] inserting:',
            filtered.map(e => e.name + '@' + e.path)
          )
        }
        currentEntries = insertEntriesIntoTree(currentEntries, filtered)
      }
    }

    if (modifies.length > 0) {
      const fetchResults = await Promise.allSettled(modifies.map(c => getScratchpadEntry(c.path)))
      const modifyMap = new Map<string, ScratchpadEntry>()
      for (const result of fetchResults) {
        if (result.status === 'fulfilled' && result.value) {
          modifyMap.set(normalizePathForCompare(result.value.path), result.value)
        }
      }
      if (modifyMap.size > 0) {
        currentEntries = patchEntriesInTree(currentEntries, modifyMap)
      }
      for (const m of modifies) {
        const key = normalizePathForCompare(m.path)
        if (dirtyFiles.value.has(key)) {
          const current = [...externalConflicts.value]
          if (!current.some(p => normalizePathForCompare(p) === key)) {
            externalConflicts.value = [...current, m.path]
          }
        }
      }
    }

    response.value = {
      ...response.value,
      local_entries: currentEntries,
    }
  }

  function removeDeletedFromTree(
    entries: ScratchpadEntry[],
    deleteSet: Set<string>
  ): ScratchpadEntry[] {
    return entries
      .filter(e => !deleteSet.has(normalizePathForCompare(e.path)))
      .map(e => {
        if (e.children) {
          return { ...e, children: removeDeletedFromTree(e.children, deleteSet) }
        }
        return e
      })
  }

  function collectAllPaths(entries: ScratchpadEntry[]): Set<string> {
    const paths = new Set<string>()
    for (const e of entries) {
      paths.add(normalizePathForCompare(e.path))
      if (e.children) {
        for (const p of collectAllPaths(e.children)) {
          paths.add(p)
        }
      }
    }
    return paths
  }

  function insertEntriesIntoTree(
    entries: ScratchpadEntry[],
    newEntries: ScratchpadEntry[]
  ): ScratchpadEntry[] {
    const result = [...entries]
    for (const ne of newEntries) {
      const parentPath = getParentPath(ne.path)
      if (!parentPath) {
        result.push(ne)
        continue
      }
      const inserted = insertUnderParent(result, parentPath, ne)
      if (!inserted) {
        result.push(ne)
      }
    }
    return result
  }

  function getParentPath(path: string): string | null {
    const normalized = path.replace(/\\/g, '/')
    const lastSlash = normalized.lastIndexOf('/')
    return lastSlash > 0 ? normalized.substring(0, lastSlash) : null
  }

  function insertUnderParent(
    entries: ScratchpadEntry[],
    parentPath: string,
    child: ScratchpadEntry
  ): boolean {
    for (let i = 0; i < entries.length; i++) {
      const e = entries[i]
      if (normalizePathForCompare(e.path) === parentPath) {
        const existingNames = e.children ? e.children.map(c => c.name) : []
        // eslint-disable-next-line no-console
        console.log(
          '[insertUnderParent] adding child:',
          child.name,
          'to parent:',
          parentPath,
          'existing children:',
          existingNames
        )
        entries[i] = {
          ...e,
          children: e.children ? [...e.children, child] : [child],
        }
        return true
      }
      if (e.children) {
        if (insertUnderParent(e.children, parentPath, child)) return true
      }
    }
    return false
  }

  function patchEntriesInTree(
    entries: ScratchpadEntry[],
    modifyMap: Map<string, ScratchpadEntry>
  ): ScratchpadEntry[] {
    return entries.map(e => {
      const key = normalizePathForCompare(e.path)
      const updated = modifyMap.get(key)
      if (updated) {
        return { ...updated, children: e.children }
      }
      if (e.children) {
        return { ...e, children: patchEntriesInTree(e.children, modifyMap) }
      }
      return e
    })
  }

  function normalizePathForCompare(p: string): string {
    return p.replace(/\\/g, '/').replace(/\/$/, '')
  }

  const dirtyFiles = ref<Set<string>>(new Set())
  const externalConflicts = ref<string[]>([])

  function markDirty(path: string): void {
    const s = new Set(dirtyFiles.value)
    s.add(normalizePathForCompare(path))
    dirtyFiles.value = s
  }

  function markClean(path: string): void {
    const s = new Set(dirtyFiles.value)
    s.delete(normalizePathForCompare(path))
    dirtyFiles.value = s
  }

  function isDirty(path: string): boolean {
    return dirtyFiles.value.has(normalizePathForCompare(path))
  }

  const hasUnsavedChanges = computed(() => dirtyFiles.value.size > 0)

  function dismissConflict(path: string): void {
    externalConflicts.value = externalConflicts.value.filter(
      p => normalizePathForCompare(p) !== normalizePathForCompare(path)
    )
  }

  async function loadChildEntries(parentPath: string): Promise<ScratchpadEntry[] | null> {
    if (!response.value) return null
    try {
      const children = await listScratchpadDirectory(parentPath)
      setEntryChildren(response.value.local_entries, parentPath, children)
      return children
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  function setEntryChildren(
    entries: ScratchpadEntry[],
    parentPath: string,
    children: ScratchpadEntry[]
  ): boolean {
    for (let i = 0; i < entries.length; i++) {
      const e = entries[i]
      if (normalizePathForCompare(e.path) === normalizePathForCompare(parentPath)) {
        // eslint-disable-next-line no-console
        console.log(
          '[setEntryChildren] replacing children for:',
          parentPath,
          'old children count:',
          e.children?.length,
          'new children:',
          children.map(c => c.name)
        )
        entries[i] = { ...e, children }
        return true
      }
      if (e.children) {
        if (setEntryChildren(e.children, parentPath, children)) return true
      }
    }
    return false
  }

  function hasChildrenLoaded(entry: ScratchpadEntry): boolean {
    return entry.kind !== 'folder' || (entry.children !== null && entry.children !== undefined)
  }

  const clipboardMode = ref<'copy' | 'cut'>('copy')

  async function moveEntry(
    fromPath: string,
    toParentPath: string
  ): Promise<ScratchpadEntry | null> {
    try {
      const entry = await moveScratchpadEntry(fromPath, toParentPath)
      await loadFiles()
      return entry
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  async function replaceInFile(
    path: string,
    pattern: string,
    replacement: string,
    isRegex: boolean
  ): Promise<ReplaceResult | null> {
    try {
      const result = await replaceScratchpadContent(path, pattern, replacement, isRegex)
      addReplaceHistory({ path, pattern, replacement, isRegex, timestamp: Date.now() })
      return result
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  async function diffWithContent(
    relativePath: string,
    otherContent: string,
    leftLabel: string,
    rightLabel: string
  ): Promise<DiffResult | null> {
    try {
      return await diffScratchpadWithContent(relativePath, otherContent, leftLabel, rightLabel)
    } catch (e) {
      error.value = extractErrorMessage(e)
      return null
    }
  }

  const contentSnapshots = new Map<string, string>()

  function setContentSnapshot(path: string, content: string): void {
    contentSnapshots.set(normalizePathForCompare(path), content)
  }

  function getContentSnapshot(path: string): string | null {
    return contentSnapshots.get(normalizePathForCompare(path)) ?? null
  }

  function clearContentSnapshot(path: string): void {
    contentSnapshots.delete(normalizePathForCompare(path))
  }

  interface ReplaceHistoryEntry {
    path: string
    pattern: string
    replacement: string
    isRegex: boolean
    timestamp: number
  }

  const replaceHistory = ref<ReplaceHistoryEntry[]>([])
  const maxReplaceHistory = 20

  function addReplaceHistory(entry: ReplaceHistoryEntry): void {
    replaceHistory.value = [entry, ...replaceHistory.value].slice(0, maxReplaceHistory)
  }

  function clearReplaceHistory(): void {
    replaceHistory.value = []
  }

  function validateRegex(pattern: string): { valid: boolean; error: string | null } {
    if (!pattern) return { valid: true, error: null }
    try {
      new RegExp(pattern)
      return { valid: true, error: null }
    } catch (e) {
      return { valid: false, error: extractErrorMessage(e) }
    }
  }

  return {
    response,
    isLoading,
    error,
    notInitialized,
    searchQuery,
    localEntries,
    externalReferences,
    scratchpadPath,
    invalidReferences,
    validReferences,
    loadFiles,
    createEntry,
    deleteEntry,
    renameEntry,
    loadFileContent,
    saveFile,
    importFile,
    addReference,
    removeReference,
    isRefValid,
    isRefInvalid,
    findEntry,
    openInExplorerAction,
    getFileSize,
    clearError,
    saveFileMeta,
    searchContent,
    trashEntries,
    loadTrashEntries,
    restoreTrashEntry,
    emptyTrashBin,
    analyzableFiles,
    loadAnalyzableFiles,
    startWatching,
    stopWatching,
    promoteToResource,
    applyFileChanges,
    normalizePathForCompare,
    flattenVisibleEntries,
    dirtyFiles,
    markDirty,
    markClean,
    isDirty,
    hasUnsavedChanges,
    externalConflicts,
    dismissConflict,
    loadChildEntries,
    hasChildrenLoaded,
    clipboardMode,
    moveEntry,
    replaceInFile,
    diffWithContent,
    replaceHistory,
    clearReplaceHistory,
    validateRegex,
    setContentSnapshot,
    getContentSnapshot,
    clearContentSnapshot,
  }
}
