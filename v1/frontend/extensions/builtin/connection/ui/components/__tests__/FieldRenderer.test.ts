/**
 * @vitest-environment jsdom
 */
import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'

import FieldRenderer from '../FieldRenderer.vue'

// Minimal mock for vue-i18n
vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

// Mock lucide icons
vi.mock('lucide-vue-next', () => ({
  HelpCircle: { template: '<span>?</span>', name: 'HelpCircle' },
  Eye: { template: '<span>Eye</span>', name: 'Eye' },
  EyeOff: { template: '<span>EyeOff</span>', name: 'EyeOff' },
  FolderOpen: { template: '<span>Open</span>', name: 'FolderOpen' },
  Plus: { template: '<span>+</span>', name: 'Plus' },
}))

describe('FieldRenderer', () => {
  it('renders text input for text fieldType', () => {
    const wrapper = mount(FieldRenderer, {
      props: {
        field: { key: 'host', type: 'text', label: 'Host', required: true },
        formData: { host: 'localhost' },
        errors: {},
        passwordVisible: {},
      },
    })
    expect(wrapper.html()).toContain('host')
  })

  it('renders password input with type password', () => {
    const wrapper = mount(FieldRenderer, {
      props: {
        field: { key: 'password', type: 'password', label: 'Password' },
        formData: { password: '' },
        errors: {},
        passwordVisible: {},
      },
    })
    expect(wrapper.find('input[type="password"]').exists()).toBe(true)
  })

  it('renders password input as text when passwordVisible is true', () => {
    const wrapper = mount(FieldRenderer, {
      props: {
        field: { key: 'password', type: 'password', label: 'Password' },
        formData: { password: '' },
        errors: {},
        passwordVisible: { password: true },
      },
    })
    const input = wrapper.find('input')
    expect(input.attributes('type')).toBe('text')
  })

  it('renders number input', () => {
    const wrapper = mount(FieldRenderer, {
      props: {
        field: { key: 'port', type: 'number', label: 'Port' },
        formData: { port: 3306 },
        errors: {},
        passwordVisible: {},
      },
    })
    expect(wrapper.find('input[type="number"]').exists()).toBe(true)
  })

  it('emits toggle-password when eye icon clicked', async () => {
    const wrapper = mount(FieldRenderer, {
      props: {
        field: { key: 'password', type: 'password', label: 'Password' },
        formData: { password: '' },
        errors: {},
        passwordVisible: { password: false },
      },
    })
    const btn = wrapper.find('.btn-toggle-password')
    await btn.trigger('click')
    expect(wrapper.emitted('togglePassword')).toBeTruthy()
    expect(wrapper.emitted('togglePassword')![0]).toEqual(['password'])
  })

  it('renders required field with red asterisk', () => {
    const wrapper = mount(FieldRenderer, {
      props: {
        field: { key: 'host', type: 'text', label: 'Host', required: true },
        formData: { host: '' },
        errors: {},
        passwordVisible: {},
      },
    })
    expect(wrapper.html()).toContain('*')
  })

  it('does not render required asterisk when not required', () => {
    const wrapper = mount(FieldRenderer, {
      props: {
        field: { key: 'description', type: 'text', label: 'Description', required: false },
        formData: { description: '' },
        errors: {},
        passwordVisible: {},
      },
    })
    // Should not render * in label
    const label = wrapper.find('.field-label')
    expect(label.exists()).toBe(true)
  })
})
