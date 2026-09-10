/**
 * 虚拟滚动核心逻辑
 *
 * 计算可见区域，只渲染可见范围内的节点
 *
 * 性能优化：
 * - 使用整数计算避免浮点运算
 * - 缓存计算结果避免重复计算
 */

import { computed, type Ref } from 'vue'

export interface UseVirtualScrollOptions {
  /** 容器高度 */
  containerHeight: Ref<number>
  /** 滚动位置 */
  scrollTop: Ref<number>
  /** 每项高度 */
  itemHeight: number
  /** 总项数 */
  totalItems: Ref<number>
  /** 缓冲区大小（上下各缓冲几项） */
  bufferSize?: number
}

export function useVirtualScroll(options: UseVirtualScrollOptions) {
  const { containerHeight, scrollTop, itemHeight, totalItems, bufferSize = 5 } = options

  // 预计算：每项高度的整数版本
  const itemHeightInt = Math.round(itemHeight)

  // 总高度（使用整数运算）
  const totalHeight = computed(() => {
    return totalItems.value * itemHeightInt
  })

  // 可见区域起始索引（包含缓冲）
  const visibleStart = computed(() => {
    const start = Math.floor(scrollTop.value / itemHeightInt)
    const withBuffer = start - bufferSize
    return withBuffer > 0 ? withBuffer : 0
  })

  // 可见区域结束索引（包含缓冲）
  const visibleEnd = computed(() => {
    const containerHeightVal = containerHeight.value
    const end = Math.ceil((scrollTop.value + containerHeightVal) / itemHeightInt)
    const withBuffer = end + bufferSize
    const total = totalItems.value
    return withBuffer < total ? withBuffer : total
  })

  // 可见项数量
  const visibleCount = computed(() => visibleEnd.value - visibleStart.value)

  // 偏移量（使用整数运算）
  const offsetY = computed(() => visibleStart.value * itemHeightInt)

  return {
    totalHeight,
    visibleStart,
    visibleEnd,
    visibleCount,
    offsetY,
  }
}
