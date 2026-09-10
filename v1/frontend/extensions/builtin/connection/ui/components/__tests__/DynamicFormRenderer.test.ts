/**
 * @vitest-environment jsdom
 */
import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'

import DynamicFormRenderer from '../DynamicFormRenderer.vue'

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

// Mock lucide icons used by DynamicFormRenderer (ChevronDown)
vi.mock('lucide-vue-next', () => ({
  ChevronDown: { template: '<span>▼</span>', name: 'ChevronDown' },
}))

describe('DynamicFormRenderer', () => {
  it('renders empty when sections array is empty', () => {
    const wrapper = mount(DynamicFormRenderer, {
      props: {
        sections: [],
        formData: {},
        errors: {},
      },
    })
    expect(wrapper.findAll('.form-section-wrapper').length).toBe(0)
  })

  it('renders single section', () => {
    const wrapper = mount(DynamicFormRenderer, {
      props: {
        sections: [
          {
            key: 'connection',
            title: 'Connection',
            fields: [
              { key: 'host', type: 'text', label: 'Host', required: true },
              { key: 'port', type: 'number', label: 'Port' },
            ],
          },
        ],
        formData: { host: 'localhost', port: 3306 },
        errors: {},
      },
    })
    expect(wrapper.find('.form-section-wrapper').exists()).toBe(true)
  })

  it('renders multiple sections', () => {
    const wrapper = mount(DynamicFormRenderer, {
      props: {
        sections: [
          {
            key: 'connection',
            title: 'Connection',
            fields: [{ key: 'host', type: 'text', label: 'Host' }],
          },
          {
            key: 'options',
            title: 'Options',
            fields: [{ key: 'charset', type: 'text', label: 'Charset' }],
          },
        ],
        formData: { host: 'localhost', charset: 'utf8' },
        errors: {},
      },
    })
    expect(wrapper.findAll('.form-section-wrapper').length).toBe(2)
  })

  it('renders collapsible section with toggle', () => {
    const wrapper = mount(DynamicFormRenderer, {
      props: {
        sections: [
          {
            key: 'advanced',
            title: 'Advanced',
            collapsible: true,
            fields: [{ key: 'timeout', type: 'number', label: 'Timeout' }],
          },
        ],
        formData: { timeout: 30 },
        errors: {},
      },
    })
    expect(wrapper.find('.collapse-icon').exists()).toBe(true)
  })

  it('toggles collapsible section on header click', async () => {
    const wrapper = mount(DynamicFormRenderer, {
      props: {
        sections: [
          {
            key: 'advanced',
            title: 'Advanced',
            collapsible: true,
            fields: [{ key: 'timeout', type: 'number', label: 'Timeout' }],
          },
        ],
        formData: { timeout: 30 },
        errors: {},
      },
    })
    const header = wrapper.find('.section-header.collapsible')
    await header.trigger('click')
    // After toggle, collapsible section should toggle visibility
    // The icon should have collapsed class
    const icon = wrapper.find('.collapse-icon')
    expect(icon.classes()).toContain('collapsed')
  })
})
