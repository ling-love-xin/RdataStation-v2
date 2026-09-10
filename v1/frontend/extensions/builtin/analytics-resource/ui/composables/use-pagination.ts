import type { SortField, SortOrder } from '../../types'
import type { Ref, ComputedRef } from 'vue'

export function usePagination(
  page: Ref<number>,
  pageSize: Ref<number>,
  total: Ref<number>,
  totalPages: ComputedRef<number>,
  sortBy: Ref<SortField | null>,
  sortOrder: Ref<SortOrder>
) {
  function setSort(field: SortField | null, order?: SortOrder) {
    sortBy.value = field
    if (order) {
      sortOrder.value = order
    } else if (sortBy.value === field) {
      sortOrder.value = sortOrder.value === 'asc' ? 'desc' : 'asc'
    }
  }

  function toggleSortOrder() {
    sortOrder.value = sortOrder.value === 'asc' ? 'desc' : 'asc'
  }

  function setPage(newPage: number) {
    if (newPage >= 1 && newPage <= totalPages.value) {
      page.value = newPage
    }
  }

  function setPageSize(size: number) {
    pageSize.value = size
    page.value = 1
  }

  function nextPage() {
    if (page.value < totalPages.value) {
      page.value += 1
    }
  }

  function prevPage() {
    if (page.value > 1) {
      page.value -= 1
    }
  }

  return {
    page,
    pageSize,
    total,
    totalPages,
    sortBy,
    sortOrder,
    setSort,
    toggleSortOrder,
    setPage,
    setPageSize,
    nextPage,
    prevPage,
  }
}
