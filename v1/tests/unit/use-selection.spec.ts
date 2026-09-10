import { describe, it, expect } from 'vitest'
import { ref } from 'vue'

import { useSelection } from '../../src/extensions/builtin/analytics-resource/ui/composables/use-selection'

describe('useSelection', () => {
  function setup() {
    const selectedResources = ref<string[]>([])
    const selectedScope = ref<string | null>(null)
    const selectedType = ref<string | null>(null)
    const selectedFolderId = ref<string | null>(null)

    const selection = useSelection(selectedResources, selectedScope, selectedType, selectedFolderId)
    return { selectedResources, selectedScope, selectedType, selectedFolderId, selection }
  }

  describe('selectResource', () => {
    it('should select single resource (replace mode)', () => {
      const { selection, selectedResources } = setup()
      selection.selectResource('res-1')
      expect(selectedResources.value).toEqual(['res-1'])
    })

    it('should replace previous selection in non-multi mode', () => {
      const { selection, selectedResources } = setup()
      selectedResources.value = ['res-1', 'res-2']
      selection.selectResource('res-3')
      expect(selectedResources.value).toEqual(['res-3'])
    })

    it('should add to selection in multi mode', () => {
      const { selection, selectedResources } = setup()
      selection.selectResource('res-1', true)
      selection.selectResource('res-2', true)
      expect(selectedResources.value).toContain('res-1')
      expect(selectedResources.value).toContain('res-2')
      expect(selectedResources.value).toHaveLength(2)
    })

    it('should toggle off in multi mode if already selected', () => {
      const { selection, selectedResources } = setup()
      selection.selectResource('res-1', true)
      selection.selectResource('res-2', true)
      selection.selectResource('res-1', true)
      expect(selectedResources.value).not.toContain('res-1')
      expect(selectedResources.value).toContain('res-2')
    })
  })

  describe('clearSelection', () => {
    it('should empty selected resources', () => {
      const { selection, selectedResources } = setup()
      selectedResources.value = ['res-1', 'res-2', 'res-3']
      selection.clearSelection()
      expect(selectedResources.value).toHaveLength(0)
    })

    it('should handle empty selection', () => {
      const { selection, selectedResources } = setup()
      selection.clearSelection()
      expect(selectedResources.value).toHaveLength(0)
    })
  })

  describe('selectScope', () => {
    it('should set selected scope', () => {
      const { selection, selectedScope } = setup()
      selection.selectScope('project')
      expect(selectedScope.value).toBe('project')
    })

    it('should allow null to clear scope filter', () => {
      const { selection, selectedScope } = setup()
      selection.selectScope('project')
      selection.selectScope(null)
      expect(selectedScope.value).toBeNull()
    })
  })

  describe('selectType', () => {
    it('should set selected type', () => {
      const { selection, selectedType } = setup()
      selection.selectType('table')
      expect(selectedType.value).toBe('table')
    })
  })

  describe('selectFolder', () => {
    it('should set selected folder', () => {
      const { selection, selectedFolderId } = setup()
      selection.selectFolder('folder-1')
      expect(selectedFolderId.value).toBe('folder-1')
    })
  })
})
