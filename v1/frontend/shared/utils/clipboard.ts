/**
 * 跨平台剪贴板写入工具
 *
 * 优先使用现代 navigator.clipboard API，降级使用 document.execCommand('copy')。
 * 在 Tauri WebView2 环境中 clipboard API 始终可用，降级方案仅用于兼容旧环境。
 */

/**
 * 将文本写入剪贴板
 * @returns 是否成功写入
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    // 优先使用现代 API
    if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
      await navigator.clipboard.writeText(text)
      return true
    }
  } catch {
    // clipboard API 不可用或权限被拒，尝试降级
  }

  // 降级方案：使用 textarea + execCommand
  try {
    const textarea = document.createElement('textarea')
    textarea.value = text
    textarea.style.position = 'fixed'
    textarea.style.left = '-9999px'
    textarea.style.top = '-9999px'
    document.body.appendChild(textarea)
    textarea.focus()
    textarea.select()
    const success = document.execCommand('copy')
    document.body.removeChild(textarea)
    return success
  } catch {
    return false
  }
}
