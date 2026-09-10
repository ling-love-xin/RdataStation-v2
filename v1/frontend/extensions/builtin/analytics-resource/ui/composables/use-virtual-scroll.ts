import { ref, computed, onMounted, onUnmounted, watch, type Ref, type ComputedRef } from 'vue'

export interface VirtualScrollOptions<T> {
  itemHeight: number
  items: Ref<readonly T[]>
  overscan?: number
}

export interface VirtualScrollReturn<T> {
  totalHeight: ComputedRef<number>
  offsetY: ComputedRef<number>
  visibleItems: ComputedRef<T[]>
  containerRef: Ref<HTMLDivElement | null>
  handleScroll: () => void
}

export function useVirtualScroll<T>(options: VirtualScrollOptions<T>): VirtualScrollReturn<T> {
  const { itemHeight, items, overscan = 2 } = options

  const containerRef = ref<HTMLDivElement | null>(null)
  const containerHeight = ref(0)
  const scrollTop = ref(0)

  const totalHeight = computed(() => items.value.length * itemHeight)

  const visibleRange = computed(() => {
    const start = Math.floor(scrollTop.value / itemHeight)
    const visibleCount = Math.ceil(containerHeight.value / itemHeight)
    const startIndex = Math.max(0, start - overscan)
    const endIndex = Math.min(items.value.length, startIndex + visibleCount + overscan * 2)
    return { startIndex, endIndex }
  })

  const offsetY = computed(() => visibleRange.value.startIndex * itemHeight)

  const visibleItems = computed(() => {
    const { startIndex, endIndex } = visibleRange.value
    return items.value.slice(startIndex, endIndex)
  })

  function handleScroll() {
    if (containerRef.value) {
      scrollTop.value = containerRef.value.scrollTop
    }
  }

  function updateContainerHeight() {
    if (containerRef.value) {
      containerHeight.value = containerRef.value.clientHeight
    }
  }

  onMounted(() => {
    updateContainerHeight()
    window.addEventListener('resize', updateContainerHeight)
  })

  onUnmounted(() => {
    window.removeEventListener('resize', updateContainerHeight)
  })

  watch(items, () => {
    updateContainerHeight()
  })

  return {
    totalHeight,
    offsetY,
    visibleItems,
    containerRef,
    handleScroll,
  }
}
