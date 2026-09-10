/**
 * 编辑器模式解析器
 *
 * 根据文件扩展名和语言自动确定编辑器类型（query / analysis / code），
 * 支持用户手动覆盖。
 */

import type { EditorType } from '@/extensions/builtin/workbench/types/editor-types'
import { EDITOR_MODE_RULES } from '@/extensions/builtin/workbench/types/editor-types'

/** 用户手动选择的编辑器类型缓存 (filePath → EditorType) */
const userPreferenceCache = new Map<string, EditorType>()

export const EditorModeResolver = {
  /**
   * 根据文件路径和语言解析编辑器类型
   * 优先级：用户手动选择 > 文件扩展名匹配 > 语言匹配 > 默认 'code'
   */
  resolve(filePath: string, language?: string): EditorType {
    const cached = userPreferenceCache.get(filePath)
    if (cached) return cached

    const ext = filePath.split('.').pop()?.toLowerCase() ?? ''
    const lang = language?.toLowerCase() ?? ''

    for (const rule of EDITOR_MODE_RULES) {
      if (rule.extensions.includes(ext)) return rule.editorType
      if (rule.languages.includes(lang)) return rule.editorType
    }

    return 'code'
  },

  /** 设置用户手动选择的编辑器类型 */
  setPreference(filePath: string, editorType: EditorType): void {
    userPreferenceCache.set(filePath, editorType)
  },

  /** 清除用户偏好（恢复自动解析） */
  clearPreference(filePath: string): void {
    userPreferenceCache.delete(filePath)
  },

  /** 清除所有偏好 */
  clearAllPreferences(): void {
    userPreferenceCache.clear()
  },
}
