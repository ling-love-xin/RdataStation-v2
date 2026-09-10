/**
 * @vitest-environment happy-dom
 */
/* eslint-disable @typescript-eslint/no-non-null-assertion */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { nextTick } from 'vue'

import { useSearch } from '../../src/extensions/builtin/analytics-resource/ui/composables/use-search'

describe('useSearch', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('should initialize with empty query', () => {
    const { searchQuery, isSearching, hasSearchQuery } = useSearch()

    expect(searchQuery.value).toBe('')
    expect(isSearching.value).toBe(false)
    expect(hasSearchQuery.value).toBe(false)
  })

  it('should debounce search by default 300ms', async () => {
    const onSearch = vi.fn()
    const { searchQuery } = useSearch({ onSearch })

    searchQuery.value = 'test'
    await nextTick()

    expect(onSearch).not.toHaveBeenCalled()

    vi.advanceTimersByTime(300)
    await nextTick()

    expect(onSearch).toHaveBeenCalledWith('test')
    expect(onSearch).toHaveBeenCalledTimes(1)
  })

  it('should respect custom debounceMs', async () => {
    const onSearch = vi.fn()
    const { searchQuery } = useSearch({ onSearch, debounceMs: 100 })

    searchQuery.value = 'fast'
    await nextTick()

    vi.advanceTimersByTime(100)
    await nextTick()

    expect(onSearch).toHaveBeenCalledWith('fast')
  })

  it('should debounce rapid changes', async () => {
    const onSearch = vi.fn()
    const { searchQuery } = useSearch({ onSearch })

    searchQuery.value = 'a'
    searchQuery.value = 'ab'
    searchQuery.value = 'abc'
    await nextTick()

    vi.advanceTimersByTime(300)
    await nextTick()

    expect(onSearch).toHaveBeenCalledTimes(1)
    expect(onSearch).toHaveBeenCalledWith('abc')
  })

  it('should set isSearching during search', async () => {
    let resolveSearch: (value: unknown) => void
    const onSearch = vi.fn().mockImplementation(() => {
      return new Promise(resolve => {
        resolveSearch = resolve
      })
    })
    const { searchQuery, isSearching } = useSearch({ onSearch })

    searchQuery.value = 'pending'
    await nextTick()

    vi.advanceTimersByTime(300)
    await nextTick()

    expect(isSearching.value).toBe(true)

    resolveSearch!(undefined)
    await nextTick()

    expect(isSearching.value).toBe(false)
  })

  it('should clear search query', () => {
    const { searchQuery, clearSearch, hasSearchQuery } = useSearch()

    searchQuery.value = 'something'
    expect(hasSearchQuery.value).toBe(true)

    clearSearch()

    expect(searchQuery.value).toBe('')
    expect(hasSearchQuery.value).toBe(false)
  })

  it('should update debouncedQuery after debounce', async () => {
    const { searchQuery, debouncedQuery } = useSearch()

    searchQuery.value = 'hello'
    await nextTick()

    vi.advanceTimersByTime(300)
    await nextTick()

    expect(debouncedQuery.value).toBe('hello')
  })

  it('should not call onSearch when no callback provided', async () => {
    const { searchQuery } = useSearch()

    searchQuery.value = 'noop'
    await nextTick()

    vi.advanceTimersByTime(300)
    await nextTick()

    // Should not throw
    expect(searchQuery.value).toBe('noop')
  })
})

describe('useSearch empty query', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('should handle very short debounce time', async () => {
    const onSearch = vi.fn()
    const { searchQuery } = useSearch({ onSearch, debounceMs: 10 })

    searchQuery.value = 'instant'
    await nextTick()

    vi.advanceTimersByTime(10)
    await nextTick()

    expect(onSearch).toHaveBeenCalledWith('instant')
  })

  it('should not call onSearch when watch does not fire', () => {
    const onSearch = vi.fn()
    useSearch({ onSearch })
    // searchQuery stays at initial '' — watch does not fire for unchanged value
    expect(onSearch).not.toHaveBeenCalled()
  })
})
