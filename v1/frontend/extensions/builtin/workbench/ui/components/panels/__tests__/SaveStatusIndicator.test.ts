import { mount } from '@vue/test-utils'
import { describe, it, expect } from 'vitest'

import SaveStatusIndicator from '../SaveStatusIndicator.vue'

import type { SaveStatus } from '../../../composables/useFileSave'

describe('SaveStatusIndicator', () => {
  const mountComponent = (
    status: SaveStatus,
    lastSaveTime?: number | null,
    showLabel?: boolean
  ) => {
    return mount(SaveStatusIndicator, {
      props: {
        status,
        lastSaveTime: lastSaveTime ?? null,
        showLabel: showLabel ?? false,
      },
    })
  }

  describe('status icons', () => {
    it('should show checkmark for saved status', () => {
      const wrapper = mountComponent('saved')
      expect(wrapper.find('.save-status-icon').text()).toBe('✓')
    })

    it('should show dot for unsaved status', () => {
      const wrapper = mountComponent('unsaved')
      expect(wrapper.find('.save-status-icon').text()).toBe('●')
    })

    it('should show spinner for saving status', () => {
      const wrapper = mountComponent('saving')
      expect(wrapper.find('.save-status-icon').text()).toBe('⟳')
    })

    it('should show cross for error status', () => {
      const wrapper = mountComponent('error')
      expect(wrapper.find('.save-status-icon').text()).toBe('✗')
    })

    it('should show checkmark for idle status', () => {
      const wrapper = mountComponent('idle')
      expect(wrapper.find('.save-status-icon').text()).toBe('✓')
    })
  })

  describe('CSS classes', () => {
    it('should have save-status-saved class for saved', () => {
      const wrapper = mountComponent('saved')
      expect(wrapper.find('.save-status-saved').exists()).toBe(true)
    })

    it('should have save-status-unsaved class for unsaved', () => {
      const wrapper = mountComponent('unsaved')
      expect(wrapper.find('.save-status-unsaved').exists()).toBe(true)
    })

    it('should have save-status-saving class for saving', () => {
      const wrapper = mountComponent('saving')
      expect(wrapper.find('.save-status-saving').exists()).toBe(true)
    })

    it('should have save-status-error class for error', () => {
      const wrapper = mountComponent('error')
      expect(wrapper.find('.save-status-error').exists()).toBe(true)
    })

    it('should have save-status-idle class for idle', () => {
      const wrapper = mountComponent('idle')
      expect(wrapper.find('.save-status-idle').exists()).toBe(true)
    })
  })

  describe('labels', () => {
    it('should show label when showLabel is true', () => {
      const wrapper = mountComponent('saving', null, true)
      expect(wrapper.find('.save-status-label').exists()).toBe(true)
      expect(wrapper.find('.save-status-label').text()).toBe('Saving...')
    })

    it('should hide label when showLabel is false', () => {
      const wrapper = mountComponent('saved', null, false)
      expect(wrapper.find('.save-status-label').exists()).toBe(false)
    })

    it('should show correct label for each status', () => {
      const cases: [SaveStatus, string][] = [
        ['saving', 'Saving...'],
        ['saved', 'Saved'],
        ['unsaved', 'Unsaved'],
        ['error', 'Save Error'],
        ['idle', ''],
      ]

      for (const [status, expected] of cases) {
        const wrapper = mountComponent(status, null, true)
        expect(wrapper.find('.save-status-label').text()).toBe(expected)
      }
    })
  })

  describe('tooltips', () => {
    it('should show last save time tooltip for saved status', () => {
      const timestamp = new Date('2026-05-15T10:30:00').getTime()
      const wrapper = mountComponent('saved', timestamp)
      expect(wrapper.attributes('title')).toContain('Last saved:')
    })

    it('should show saving tooltip for saving status', () => {
      const wrapper = mountComponent('saving')
      expect(wrapper.attributes('title')).toBe('Saving file...')
    })

    it('should show unsaved tooltip for unsaved status', () => {
      const wrapper = mountComponent('unsaved')
      expect(wrapper.attributes('title')).toBe('File has unsaved changes')
    })

    it('should show error tooltip for error status', () => {
      const wrapper = mountComponent('error')
      expect(wrapper.attributes('title')).toBe('Save failed - click to retry')
    })

    it('should show empty tooltip for idle status', () => {
      const wrapper = mountComponent('idle')
      expect(wrapper.attributes('title')).toBe('')
    })
  })
})
