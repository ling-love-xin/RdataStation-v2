/**
 * @vitest-environment happy-dom
 */
/* eslint-disable @typescript-eslint/no-non-null-assertion */
import { mount } from '@vue/test-utils'
import { describe, it, expect } from 'vitest'
import { ref, defineComponent, h, nextTick } from 'vue'

import { useVirtualScroll } from '../../src/extensions/builtin/analytics-resource/ui/composables/use-virtual-scroll'

import type { VirtualScrollReturn } from '../../src/extensions/builtin/analytics-resource/ui/composables/use-virtual-scroll'

interface TestItem {
  id: number
  label: string
}

function makeItems(count: number): TestItem[] {
  return Array.from({ length: count }, (_, i) => ({ id: i, label: `Item ${i}` }))
}

function mountVirtualScroll<T>(
  items: T[],
  options: { itemHeight: number; overscan?: number }
): { wrapper: ReturnType<typeof mount>; vr: VirtualScrollReturn<T> } {
  const itemsRef = ref(items) as ReturnType<typeof ref<T[]>>
  let vr: VirtualScrollReturn<T>

  const wrapper = mount(
    defineComponent({
      setup() {
        vr = useVirtualScroll<T>({
          items: itemsRef,
          itemHeight: options.itemHeight,
          overscan: options.overscan,
        })
        return () => h('div', { ref: vr.containerRef })
      },
    })
  )

  return { wrapper, vr: vr! }
}

describe('useVirtualScroll', () => {
  it('should calculate totalHeight from items and itemHeight', () => {
    const items = ref(makeItems(100))
    const { totalHeight } = useVirtualScroll({ items, itemHeight: 40 })
    expect(totalHeight.value).toBe(4000)
  })

  it('should return zero totalHeight for empty items', () => {
    const items = ref<TestItem[]>([])
    const { totalHeight, offsetY, visibleItems } = useVirtualScroll({ items, itemHeight: 40 })
    expect(totalHeight.value).toBe(0)
    expect(offsetY.value).toBe(0)
    expect(visibleItems.value).toHaveLength(0)
  })

  it('should calculate visible range after DOM mount and scroll', async () => {
    const items = makeItems(100)
    const { wrapper: _wrapper, vr } = mountVirtualScroll(items, { itemHeight: 40, overscan: 1 })
    await nextTick()

    const el = vr.containerRef.value!
    Object.defineProperty(el, 'clientHeight', { value: 400, writable: true, configurable: true })
    Object.defineProperty(el, 'scrollTop', { value: 200, writable: true, configurable: true })

    window.dispatchEvent(new Event('resize'))
    await nextTick()

    vr.handleScroll()

    expect(vr.visibleItems.value.length).toBeGreaterThan(0)
    expect(vr.visibleItems.value[0].id).toBeGreaterThanOrEqual(4)
  })

  it('should include overscan items before and after visible range', async () => {
    const items = makeItems(200)
    const { wrapper: _wrapper, vr } = mountVirtualScroll(items, { itemHeight: 40, overscan: 3 })
    await nextTick()

    const el = vr.containerRef.value!
    Object.defineProperty(el, 'clientHeight', { value: 400, writable: true, configurable: true })
    Object.defineProperty(el, 'scrollTop', { value: 800, writable: true, configurable: true })

    window.dispatchEvent(new Event('resize'))
    await nextTick()

    vr.handleScroll()

    expect(vr.visibleItems.value.length).toBeGreaterThan(10)
    expect(vr.visibleItems.value[0].id).toBeLessThanOrEqual(17)
  })

  it('should not crash when scrollTop exceeds totalHeight', async () => {
    const items = makeItems(10)
    const { wrapper: _wrapper, vr } = mountVirtualScroll(items, { itemHeight: 40 })
    await nextTick()

    const el = vr.containerRef.value!
    Object.defineProperty(el, 'clientHeight', { value: 400, writable: true, configurable: true })
    Object.defineProperty(el, 'scrollTop', { value: 99999, writable: true, configurable: true })

    window.dispatchEvent(new Event('resize'))
    await nextTick()

    vr.handleScroll()

    expect(vr.visibleItems.value).toBeDefined()
    expect(vr.visibleItems.value.length).toBeGreaterThanOrEqual(0)
  })

  it('should expose handleScroll without throwing', async () => {
    const items = makeItems(50)
    const { wrapper: _wrapper, vr } = mountVirtualScroll(items, { itemHeight: 40 })
    await nextTick()

    const el = vr.containerRef.value!
    Object.defineProperty(el, 'scrollTop', { value: 100, writable: true, configurable: true })

    expect(() => vr.handleScroll()).not.toThrow()
  })

  it('should compute offsetY based on startIndex', async () => {
    const items = makeItems(100)
    const { wrapper: _wrapper, vr } = mountVirtualScroll(items, { itemHeight: 40, overscan: 1 })
    await nextTick()

    const el = vr.containerRef.value!
    Object.defineProperty(el, 'clientHeight', { value: 400, writable: true, configurable: true })
    Object.defineProperty(el, 'scrollTop', { value: 400, writable: true, configurable: true })

    window.dispatchEvent(new Event('resize'))
    await nextTick()

    vr.handleScroll()

    expect(vr.offsetY.value).toBeGreaterThanOrEqual(0)
  })
})
