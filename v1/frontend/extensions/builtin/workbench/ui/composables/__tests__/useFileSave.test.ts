import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { ref, nextTick } from 'vue'

const mockSaveScratchpadFile = vi.fn()

vi.mock('@/extensions/builtin/scratchpad/infrastructure/api/scratchpad-api', () => ({
  saveScratchpadFile: (...args: unknown[]) => mockSaveScratchpadFile(...args),
}))

import { useFileSave } from '../useFileSave'

import type { SaveStatus } from '../useFileSave'

describe('useFileSave', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mockSaveScratchpadFile.mockReset()
    mockSaveScratchpadFile.mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  function createComposable(
    options: {
      filePath?: string
      content?: string
      autoSaveInterval?: number
      maxRetries?: number
      retryDelay?: number
    } = {}
  ) {
    const filePath = ref(options.filePath ?? '')
    let contentValue = options.content ?? 'test content'
    const getContent = () => contentValue

    const instance = useFileSave({
      filePath,
      getContent,
      autoSaveInterval: options.autoSaveInterval ?? 0,
      maxRetries: options.maxRetries ?? 3,
      retryDelay: options.retryDelay ?? 2000,
    })

    return {
      filePath,
      getContent: () => contentValue,
      setContent: (v: string) => {
        contentValue = v
      },
      ...instance,
    }
  }

  describe('status transitions', () => {
    it('should start with idle status', () => {
      const { saveStatus } = createComposable()
      expect(saveStatus.value).toBe('idle')
    })

    it('should transition to unsaved when marked dirty', () => {
      const { markDirty, saveStatus } = createComposable()
      markDirty()
      expect(saveStatus.value).toBe('unsaved')
    })

    it('should remain unsaved when marked dirty multiple times', () => {
      const { markDirty, saveStatus } = createComposable()
      markDirty()
      markDirty()
      expect(saveStatus.value).toBe('unsaved')
    })

    it('should transition to saving during save operation', async () => {
      const { manualSave, saveStatus, filePath } = createComposable()
      filePath.value = 'test.sql'

      mockSaveScratchpadFile.mockImplementation(
        () => new Promise(resolve => setTimeout(resolve, 100))
      )

      const savePromise = manualSave()
      expect(saveStatus.value).toBe('saving')

      vi.advanceTimersByTime(100)
      await savePromise
    })

    it('should transition to saved after successful save', async () => {
      const { manualSave, saveStatus, isDirty, filePath } = createComposable()
      filePath.value = 'test.sql'

      await manualSave()

      expect(saveStatus.value).toBe('saved')
      expect(isDirty.value).toBe(false)
    })

    it('should transition to error on save failure', async () => {
      mockSaveScratchpadFile.mockRejectedValue(new Error('Disk full'))
      const { manualSave, saveStatus, filePath } = createComposable({ maxRetries: 0 })
      filePath.value = 'test.sql'

      await manualSave()

      expect(saveStatus.value).toBe('error')
    })
  })

  describe('manual save', () => {
    it('should call saveScratchpadFile with correct arguments', async () => {
      const { manualSave, filePath, getContent } = createComposable()
      filePath.value = 'queries/select.sql'

      await manualSave()

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(1)
      expect(mockSaveScratchpadFile).toHaveBeenCalledWith('queries/select.sql', getContent())
    })

    it('should return true on successful save', async () => {
      const { manualSave, filePath } = createComposable()
      filePath.value = 'test.sql'

      const result = await manualSave()
      expect(result).toBe(true)
    })

    it('should return false on save failure', async () => {
      mockSaveScratchpadFile.mockRejectedValue(new Error('Network error'))
      const { manualSave, filePath } = createComposable({ maxRetries: 0 })
      filePath.value = 'test.sql'

      const result = await manualSave()
      expect(result).toBe(false)
    })

    it('should not save when filePath is empty', async () => {
      const { manualSave } = createComposable()

      const result = await manualSave()

      expect(result).toBe(false)
      expect(mockSaveScratchpadFile).not.toHaveBeenCalled()
    })

    it('should update lastSaveTime on successful save', async () => {
      const { manualSave, lastSaveTime, filePath } = createComposable()
      filePath.value = 'test.sql'

      const before = Date.now()
      await manualSave()

      expect(lastSaveTime.value).not.toBeNull()
      expect(lastSaveTime.value as number).toBeGreaterThanOrEqual(before)
    })

    it('should call onSaveSuccess callback', async () => {
      const onSuccess = vi.fn()
      const filePath = ref('test.sql')
      const instance = useFileSave({
        filePath,
        getContent: () => 'content',
        onSaveSuccess: onSuccess,
        autoSaveInterval: 0,
      })

      await instance.manualSave()

      expect(onSuccess).toHaveBeenCalledTimes(1)
    })

    it('should call onSaveError callback', async () => {
      mockSaveScratchpadFile.mockRejectedValue(new Error('Fail'))
      const onError = vi.fn()
      const filePath = ref('test.sql')
      const instance = useFileSave({
        filePath,
        getContent: () => 'content',
        onSaveError: onError,
        maxRetries: 0,
        autoSaveInterval: 0,
      })

      await instance.manualSave()

      expect(onError).toHaveBeenCalledWith('Fail')
    })
  })

  describe('auto save', () => {
    it('should auto-save at configured interval', async () => {
      const { markDirty, filePath } = createComposable({ autoSaveInterval: 30000 })
      filePath.value = 'test.sql'
      await nextTick()
      markDirty()

      await vi.advanceTimersByTimeAsync(31000)

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(1)
    })

    it('should not auto-save when filePath is empty', async () => {
      const { markDirty } = createComposable({ autoSaveInterval: 30000 })
      markDirty()

      await vi.advanceTimersByTimeAsync(31000)

      expect(mockSaveScratchpadFile).not.toHaveBeenCalled()
    })

    it('should not auto-save when not dirty', async () => {
      const { filePath } = createComposable({ autoSaveInterval: 30000 })
      filePath.value = 'test.sql'

      await vi.advanceTimersByTimeAsync(31000)

      expect(mockSaveScratchpadFile).not.toHaveBeenCalled()
    })

    it('should auto-save multiple times for ongoing edits', async () => {
      const { markDirty, filePath } = createComposable({ autoSaveInterval: 30000 })
      filePath.value = 'test.sql'
      await nextTick()

      markDirty()
      await vi.advanceTimersByTimeAsync(31000)

      markDirty()
      await vi.advanceTimersByTimeAsync(31000)

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(2)
    })

    it('should start auto-save when filePath is set', async () => {
      const { markDirty, filePath } = createComposable({ autoSaveInterval: 30000 })

      markDirty()
      filePath.value = 'new.sql'
      await nextTick()

      await vi.advanceTimersByTimeAsync(31000)

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(1)
    })

    it('should stop auto-save when filePath is cleared', async () => {
      const { markDirty, filePath } = createComposable({ autoSaveInterval: 30000 })
      filePath.value = 'test.sql'
      await nextTick()
      markDirty()

      filePath.value = ''

      await vi.advanceTimersByTimeAsync(31000)

      expect(mockSaveScratchpadFile).not.toHaveBeenCalled()
    })

    it('should allow changing auto-save interval', async () => {
      const { markDirty, filePath, setAutoSaveInterval } = createComposable({
        autoSaveInterval: 30000,
      })
      filePath.value = 'test.sql'
      await nextTick()

      setAutoSaveInterval(10000)
      markDirty()

      await vi.advanceTimersByTimeAsync(11000)

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(1)
    })
  })

  describe('retry mechanism', () => {
    it('should retry on failure up to maxRetries', async () => {
      mockSaveScratchpadFile.mockRejectedValue(new Error('Network error'))
      const { manualSave, filePath } = createComposable({ maxRetries: 2, retryDelay: 2000 })
      filePath.value = 'test.sql'

      manualSave()
      await vi.advanceTimersByTimeAsync(2000)
      await vi.advanceTimersByTimeAsync(4000)

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(3) // 1 initial + 2 retries
    })

    it('should increase retry delay with each attempt', async () => {
      mockSaveScratchpadFile.mockRejectedValue(new Error('Network error'))
      const { manualSave, filePath, retryCount } = createComposable({
        maxRetries: 2,
        retryDelay: 2000,
      })
      filePath.value = 'test.sql'

      manualSave()
      await vi.advanceTimersByTimeAsync(2000)

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(2)
      expect(retryCount.value).toBe(2)
    })

    it('should stop retrying after success', async () => {
      mockSaveScratchpadFile
        .mockRejectedValueOnce(new Error('Fail 1'))
        .mockResolvedValueOnce(undefined)

      const { manualSave, filePath, saveStatus } = createComposable({
        maxRetries: 3,
        retryDelay: 2000,
      })
      filePath.value = 'test.sql'

      manualSave()
      await vi.advanceTimersByTimeAsync(2000)

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(2)
      expect(saveStatus.value).toBe('saved')
    })

    it('should clear retry on new manual save', async () => {
      mockSaveScratchpadFile.mockRejectedValue(new Error('Fail'))

      const { manualSave, filePath } = createComposable({ maxRetries: 2, retryDelay: 2000 })
      filePath.value = 'test.sql'

      manualSave()

      mockSaveScratchpadFile.mockResolvedValue(undefined)
      await manualSave()

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(2) // 1 (first save) + 1 (new save, retry cleared)
    })

    it('should set error status after exhausting retries', async () => {
      mockSaveScratchpadFile.mockRejectedValue(new Error('Persistent error'))
      const { manualSave, saveStatus, filePath } = createComposable({
        maxRetries: 1,
        retryDelay: 2000,
      })
      filePath.value = 'test.sql'

      manualSave()
      await vi.advanceTimersByTimeAsync(2000)
      await nextTick()

      expect(saveStatus.value).toBe('error')
    })
  })

  describe('dirty state', () => {
    it('should mark dirty on content change', () => {
      const { markDirty, isDirty } = createComposable()
      expect(isDirty.value).toBe(false)

      markDirty()
      expect(isDirty.value).toBe(true)
    })

    it('should clear dirty after successful save', async () => {
      const { markDirty, manualSave, isDirty, filePath } = createComposable()
      filePath.value = 'test.sql'

      markDirty()
      expect(isDirty.value).toBe(true)

      await manualSave()
      expect(isDirty.value).toBe(false)
    })

    it('should remain dirty after failed save', async () => {
      mockSaveScratchpadFile.mockRejectedValue(new Error('Failed'))
      const { markDirty, manualSave, isDirty, filePath } = createComposable({ maxRetries: 0 })
      filePath.value = 'test.sql'

      markDirty()
      await manualSave()

      expect(isDirty.value).toBe(true)
    })
  })

  describe('beforeunload', () => {
    it('should attempt save on beforeunload when dirty', () => {
      const { markDirty, triggerBeforeUnloadSave, filePath } = createComposable()
      filePath.value = 'test.sql'
      markDirty()

      triggerBeforeUnloadSave()

      expect(mockSaveScratchpadFile).toHaveBeenCalledTimes(1)
    })

    it('should not save on beforeunload when not dirty', () => {
      const { triggerBeforeUnloadSave, filePath } = createComposable()
      filePath.value = 'test.sql'

      triggerBeforeUnloadSave()

      expect(mockSaveScratchpadFile).not.toHaveBeenCalled()
    })

    it('should not save on beforeunload when no filePath', () => {
      const { markDirty, triggerBeforeUnloadSave } = createComposable()
      markDirty()

      triggerBeforeUnloadSave()

      expect(mockSaveScratchpadFile).not.toHaveBeenCalled()
    })

    it('should catch errors silently on beforeunload save', () => {
      mockSaveScratchpadFile.mockRejectedValue(new Error('Failed'))
      const { markDirty, triggerBeforeUnloadSave, filePath } = createComposable()
      filePath.value = 'test.sql'
      markDirty()

      expect(() => triggerBeforeUnloadSave()).not.toThrow()
    })
  })

  describe('save status combinations', () => {
    it('should track all status states', async () => {
      const { manualSave, markDirty, saveStatus, filePath } = createComposable({ maxRetries: 0 })
      filePath.value = 'test.sql'

      const statuses: SaveStatus[] = []

      markDirty()
      statuses.push(saveStatus.value)

      mockSaveScratchpadFile.mockRejectedValue(new Error('Fail'))
      await manualSave()
      statuses.push(saveStatus.value)

      mockSaveScratchpadFile.mockResolvedValue(undefined)
      await manualSave()
      statuses.push(saveStatus.value)

      expect(statuses).toEqual(['unsaved', 'error', 'saved'])
    })
  })
})
