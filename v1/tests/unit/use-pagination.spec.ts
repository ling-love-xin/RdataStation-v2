import { describe, it, expect } from 'vitest'
import { ref, computed } from 'vue'

import { usePagination } from '../../src/extensions/builtin/analytics-resource/ui/composables/use-pagination'

describe('usePagination', () => {
  function setup(totalCount = 100) {
    const page = ref(1)
    const pageSize = ref(20)
    const total = ref(totalCount)
    const totalPages = computed(() => Math.ceil(total.value / pageSize.value))
    const sortBy = ref<string | null>(null)
    const sortOrder = ref<'asc' | 'desc'>('asc')

    const pagination = usePagination(page, pageSize, total, totalPages, sortBy, sortOrder)
    return { page, pageSize, total, totalPages, sortBy, sortOrder, pagination }
  }

  describe('setPage', () => {
    it('should set page within valid range', () => {
      const { pagination, page } = setup(100)
      pagination.setPage(3)
      expect(page.value).toBe(3)
    })

    it('should reject page 0', () => {
      const { pagination, page } = setup(100)
      pagination.setPage(0)
      expect(page.value).toBe(1)
    })

    it('should reject page beyond totalPages', () => {
      const { pagination, page, totalPages } = setup(40)
      pagination.setPage(3)
      expect(totalPages.value).toBe(2)
      expect(page.value).toBe(1)
    })
  })

  describe('setPageSize', () => {
    it('should update pageSize and reset to page 1', () => {
      const { pagination, page } = setup(100)
      pagination.setPage(3)
      pagination.setPageSize(10)
      expect(page.value).toBe(1)
    })
  })

  describe('nextPage', () => {
    it('should increment page', () => {
      const { pagination, page } = setup(100)
      pagination.nextPage()
      expect(page.value).toBe(2)
    })

    it('should not exceed totalPages', () => {
      const { pagination, page } = setup(20)
      pagination.nextPage()
      expect(page.value).toBe(1)
    })
  })

  describe('prevPage', () => {
    it('should decrement page', () => {
      const { pagination, page } = setup(100)
      pagination.setPage(3)
      pagination.prevPage()
      expect(page.value).toBe(2)
    })

    it('should not go below 1', () => {
      const { pagination, page } = setup(100)
      pagination.prevPage()
      expect(page.value).toBe(1)
    })
  })

  describe('setSort', () => {
    it('should set sort field and explicit order', () => {
      const { pagination, sortBy, sortOrder } = setup(100)
      pagination.setSort('name', 'desc')
      expect(sortBy.value).toBe('name')
      expect(sortOrder.value).toBe('desc')
    })

    it('should toggle order when called without explicit order', () => {
      const { pagination, sortBy, sortOrder } = setup(100)
      sortOrder.value = 'asc'
      pagination.setSort('name')
      expect(sortBy.value).toBe('name')
      expect(sortOrder.value).toBe('desc')
    })

    it('should toggle order when same field selected', () => {
      const { pagination, sortBy, sortOrder } = setup(100)
      sortBy.value = 'name'
      sortOrder.value = 'asc'
      pagination.setSort('name')
      expect(sortOrder.value).toBe('desc')
    })

    it('should accept null to clear sort', () => {
      const { pagination, sortBy } = setup(100)
      pagination.setSort('name')
      pagination.setSort(null)
      expect(sortBy.value).toBeNull()
    })
  })

  describe('toggleSortOrder', () => {
    it('should flip asc to desc', () => {
      const { pagination, sortOrder } = setup(100)
      sortOrder.value = 'asc'
      pagination.toggleSortOrder()
      expect(sortOrder.value).toBe('desc')
    })

    it('should flip desc to asc', () => {
      const { pagination, sortOrder } = setup(100)
      sortOrder.value = 'desc'
      pagination.toggleSortOrder()
      expect(sortOrder.value).toBe('asc')
    })
  })
})
