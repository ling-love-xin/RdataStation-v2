/**
 * 缓存操作错误处理工具
 *
 * 统一缓存操作的错误处理策略：
 * - 缓存读取失败：静默失败，返回默认值
 * - 缓存写入失败：记录错误日志，不影响主流程
 * - 缓存刷新失败：记录错误日志，返回失败状态
 */

/**
 * 安全执行缓存读取操作
 * 失败时返回默认值，不抛出异常
 */
export async function safeCacheRead<T>(
  operation: () => Promise<T>,
  defaultValue: T,
  context: string = '缓存读取'
): Promise<T> {
  try {
    return await operation()
  } catch (error) {
    /* eslint-disable-next-line no-console */
    console.debug(`${context} 失败（预期行为）:`, error)
    return defaultValue
  }
}

/**
 * 安全执行缓存写入操作
 * 失败时记录错误日志，不抛出异常
 */
export async function safeCacheWrite(
  operation: () => Promise<void>,
  context: string = '缓存写入'
): Promise<boolean> {
  try {
    await operation()
    return true
  } catch (error) {
    console.warn(`${context} 失败:`, error)
    return false
  }
}

/**
 * 安全执行缓存刷新操作
 * 失败时记录错误日志，返回失败状态
 */
export async function safeCacheRefresh(
  operation: () => Promise<void>,
  context: string = '缓存刷新'
): Promise<{ success: boolean; error?: string }> {
  try {
    await operation()
    return { success: true }
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error)
    console.error(`${context} 失败:`, errorMessage)
    return { success: false, error: errorMessage }
  }
}
