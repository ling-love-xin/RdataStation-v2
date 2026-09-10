import { invoke } from '@tauri-apps/api/core'

import type {
  ScratchpadResponse,
  ScratchpadEntry,
  ExternalReference,
  AnalyzableFile,
  PromoteResult,
  SearchResult,
  ReplaceResult,
  DiffResult,
} from '../../types'

export async function initScratchpadStore(projectPath: string): Promise<void> {
  return invoke('init_scratchpad_store', { projectPath })
}

export async function listScratchpadFiles(): Promise<ScratchpadResponse> {
  return invoke<ScratchpadResponse>('list_scratchpad_files')
}

export async function createScratchpadEntry(
  name: string,
  isFolder: boolean,
  parentPath?: string
): Promise<ScratchpadEntry> {
  return invoke<ScratchpadEntry>('create_scratchpad_entry', {
    name,
    isFolder,
    parentPath: parentPath ?? null,
  })
}

export async function deleteScratchpadEntry(relativePath: string): Promise<void> {
  return invoke<void>('delete_scratchpad_entry', {
    relativePath,
  })
}

export async function renameScratchpadEntry(
  relativePath: string,
  newName: string
): Promise<ScratchpadEntry> {
  return invoke<ScratchpadEntry>('rename_scratchpad_entry', {
    relativePath,
    newName,
  })
}

export async function readScratchpadFile(relativePath: string): Promise<string> {
  return invoke<string>('read_scratchpad_file', {
    relativePath,
  })
}

export async function saveScratchpadFile(relativePath: string, content: string): Promise<void> {
  return invoke<void>('save_scratchpad_file', {
    relativePath,
    content,
  })
}

export async function importExternalFile(sourcePath: string): Promise<ScratchpadEntry> {
  return invoke<ScratchpadEntry>('import_external_file', {
    sourcePath,
  })
}

export async function addExternalReference(
  alias: string,
  path: string
): Promise<ExternalReference> {
  return invoke<ExternalReference>('add_external_reference', {
    alias,
    path,
  })
}

export async function removeExternalReference(alias: string): Promise<void> {
  return invoke<void>('remove_external_reference', {
    alias,
  })
}

export async function openInExplorer(path: string): Promise<void> {
  return invoke<void>('open_scratchpad_in_explorer', {
    path,
  })
}

export async function checkFileSize(relativePath: string): Promise<number> {
  return invoke<number>('check_scratchpad_file_size', {
    relativePath,
  })
}

export async function updateFileMeta(relativePath: string, connectionId?: string): Promise<void> {
  return invoke<void>('update_scratchpad_file_meta', {
    relativePath,
    connectionId: connectionId ?? null,
  })
}

export async function searchFileContent(
  query: string,
  caseSensitive = false,
  contextLines = 2
): Promise<SearchResult> {
  return invoke<SearchResult>('search_scratchpad_content', {
    query,
    caseSensitive,
    contextLines,
  })
}

export async function listTrash(): Promise<ScratchpadEntry[]> {
  return invoke<ScratchpadEntry[]>('list_scratchpad_trash')
}

export async function restoreFromTrash(trashName: string): Promise<ScratchpadEntry> {
  return invoke<ScratchpadEntry>('restore_scratchpad_from_trash', { trashName })
}

export async function emptyTrash(): Promise<void> {
  return invoke<void>('empty_scratchpad_trash')
}

export async function getAnalyzableFiles(): Promise<AnalyzableFile[]> {
  return invoke<AnalyzableFile[]>('get_analyzable_files')
}

export async function watchScratchpad(): Promise<void> {
  return invoke<void>('watch_scratchpad')
}

export async function unwatchScratchpad(): Promise<void> {
  return invoke<void>('unwatch_scratchpad')
}

export async function promoteScratchpadToResource(
  relativePath: string,
  removeAfter: boolean
): Promise<PromoteResult> {
  return invoke<PromoteResult>('promote_scratchpad_to_resource', {
    relativePath,
    removeAfter,
  })
}

export async function getScratchpadEntry(relativePath: string): Promise<ScratchpadEntry | null> {
  return invoke<ScratchpadEntry | null>('get_scratchpad_entry', {
    relativePath,
  })
}

export async function listScratchpadDirectory(parentPath: string): Promise<ScratchpadEntry[]> {
  return invoke<ScratchpadEntry[]>('list_scratchpad_directory', {
    parentPath,
  })
}

export async function moveScratchpadEntry(
  fromPath: string,
  toParentPath: string
): Promise<ScratchpadEntry> {
  return invoke<ScratchpadEntry>('move_scratchpad_entry', {
    fromPath,
    toParentPath,
  })
}

export async function replaceScratchpadContent(
  path: string,
  pattern: string,
  replacement: string,
  isRegex: boolean
): Promise<ReplaceResult> {
  return invoke<ReplaceResult>('replace_scratchpad_content', {
    path,
    pattern,
    replacement,
    isRegex,
  })
}

export async function diffScratchpadWithContent(
  relativePath: string,
  otherContent: string,
  leftLabel: string,
  rightLabel: string
): Promise<DiffResult> {
  return invoke<DiffResult>('diff_scratchpad_with_content', {
    relativePath,
    otherContent,
    leftLabel,
    rightLabel,
  })
}
