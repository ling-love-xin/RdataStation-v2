/**
 * 文件保存与外部变更检测
 *
 * 从 file-manager.ts 中提取的保存、另存为、外部文件变更检测逻辑。
 * file-manager.ts 重导出这些函数以保持向后兼容。
 */
import { confirmExternalChange } from '@/extensions/builtin/workbench/ui/composables/useFileDialogs'

import { openFiles, dockviewApi, notifyOpenFilesChanged } from './editor-state'
import { getEditorView } from './instance-service'

function syncDirtyToTab(filePath: string, isDirty: boolean): void {
  const info = openFiles.value.get(filePath)
  if (!info) return
  const title = isDirty ? `\u2022 ${info.fileName}` : info.fileName
  const pid = `panel_editor_${filePath.replace(/[^a-zA-Z0-9_-]/g, '_')}`
  try {
    dockviewApi?.getPanel(pid)?.api.setTitle(title)
  } catch {
    // dockviewApi 可能未初始化
  }
}

async function getFileModifiedAt(filePath: string): Promise<number | null> {
  try {
    const { stat } = await import('@tauri-apps/plugin-fs')
    const metadata = await stat(filePath)
    return (metadata as unknown as { mtime?: { ms: number } }).mtime?.ms ?? Date.now()
  } catch {
    return null
  }
}

/**
 * 保存当前文件到磁盘
 * 如果是未保存文件 (exists=false)，弹出"另存为"对话框
 */
export async function saveCurrentFileToDisk(filePath: string): Promise<void> {
  const info = openFiles.value.get(filePath)
  if (!info) throw new Error('File not open')

  const ed = getEditorView(filePath)
  if (!ed) throw new Error('No editor instance')

  // 检查外部文件变更
  if (info.exists && info.lastModifiedAt) {
    const currentMtime = await getFileModifiedAt(filePath)
    if (currentMtime && currentMtime > info.lastModifiedAt) {
      const result = await confirmExternalChange(info.fileName)
      if (result === 'reload') return
    }
  }

  const content = ed.state.doc.toString()

  // 未保存文件 → 另存为
  if (!info.exists) {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const { writeTextFile } = await import('@tauri-apps/plugin-fs')
    const path = await save({
      title: '另存为',
      filters: [{ name: 'SQL Files', extensions: ['sql'] }],
    })
    if (!path) throw new Error('Save cancelled')
    await writeTextFile(path, content)
    info.filePath = path
    info.fileName = path.split(/[/\\]/).pop() ?? path
    info.exists = true
    info.isDirty = false
    info.lastModifiedAt = Date.now()
    syncDirtyToTab(filePath, false)
    notifyOpenFilesChanged()
    return
  }

  // 已存在文件 → 直接写入
  const { writeTextFile } = await import('@tauri-apps/plugin-fs')
  await writeTextFile(filePath, content)
  info.isDirty = false
  info.lastModifiedAt = Date.now()
  syncDirtyToTab(filePath, false)
  notifyOpenFilesChanged()
}

/**
 * 将文件"另存为"到新路径
 */
export async function saveFileAs(filePath: string): Promise<void> {
  const info = openFiles.value.get(filePath)
  if (!info) throw new Error('File not open')

  const ed = getEditorView(filePath)
  if (!ed) throw new Error('No editor instance')

  const { save } = await import('@tauri-apps/plugin-dialog')
  const { writeTextFile } = await import('@tauri-apps/plugin-fs')
  const newPath = await save({
    title: '另存为',
    filters: [{ name: 'All Files', extensions: ['*'] }],
  })
  if (!newPath) throw new Error('Save cancelled')

  const content = ed.state.doc.toString()
  await writeTextFile(newPath, content)

  info.filePath = newPath
  info.fileName = newPath.split(/[/\\]/).pop() ?? newPath
  info.exists = true
  info.isDirty = false
  info.lastModifiedAt = Date.now()
  syncDirtyToTab(filePath, false)
  notifyOpenFilesChanged()
}

/**
 * 检查磁盘文件是否已被外部修改
 * 应在编辑器获得焦点时调用
 */
export async function checkExternalFileChanges(): Promise<void> {
  for (const [filePath, info] of openFiles.value) {
    if (!info.exists) continue
    const diskMtime = await getFileModifiedAt(filePath)
    if (diskMtime && info.lastModifiedAt && diskMtime > info.lastModifiedAt) {
      const result = await confirmExternalChange(info.fileName)
      if (result === 'reload') {
        const ed = getEditorView(filePath)
        if (ed) {
          const { readTextFile } = await import('@tauri-apps/plugin-fs')
          const content = await readTextFile(filePath)
          ed.dispatch({
            changes: { from: 0, to: ed.state.doc.length, insert: content },
          })
          info.isDirty = false
          info.lastModifiedAt = diskMtime
          syncDirtyToTab(filePath, false)
          notifyOpenFilesChanged()
        }
      } else {
        info.lastModifiedAt = diskMtime
      }
    }
  }
}
