import { mount } from '@vue/test-utils'
import { Settings, History } from 'lucide-vue-next'
import { describe, it, expect, vi } from 'vitest'
import { nextTick, markRaw } from 'vue'

import ToolbarActions from './ToolbarActions.vue'

// Mock vue-i18n
vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string) => key,
  }),
}))

describe('ToolbarActions', () => {
  const createTools = () => [
    {
      id: 'settings',
      name: 'Settings',
      icon: markRaw(Settings),
      enabled: false,
      action: vi.fn(),
    },
    {
      id: 'history',
      name: 'History',
      icon: markRaw(History),
      enabled: true,
      action: vi.fn(),
    },
  ]

  it('should render enabled tools', () => {
    const tools = createTools()
    const wrapper = mount(ToolbarActions, { props: { tools } })

    expect(wrapper.findAll('.toolbar-btn')).toHaveLength(1)
  })

  it('should emit tool-action when clicking enabled tool', async () => {
    const tools = createTools()
    const wrapper = mount(ToolbarActions, { props: { tools } })

    await wrapper.find('.toolbar-btn').trigger('click')

    expect(wrapper.emitted('tool-action')).toBeTruthy()
    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    expect(wrapper.emitted('tool-action')![0][0]).toBe('history')
  })

  it('should open dropdown when clicking more button', async () => {
    const tools = createTools()
    const wrapper = mount(ToolbarActions, { props: { tools } })

    await wrapper.find('.more-btn').trigger('click')
    await nextTick()

    expect(wrapper.find('.toolbar-dropdown').exists()).toBe(true)
    expect(wrapper.text()).toContain('Settings')
    expect(wrapper.text()).toContain('History')
  })

  it('should emit toggle-tool when toggling tool in dropdown', async () => {
    const tools = createTools()
    const wrapper = mount(ToolbarActions, { props: { tools } })

    await wrapper.find('.more-btn').trigger('click')
    await nextTick()

    const toggle = wrapper.find('.toolbar-option input')
    await toggle.trigger('change')

    expect(wrapper.emitted('toggle-tool')).toBeTruthy()
  })

  it('should emit reset-toolbar when clicking reset', async () => {
    const tools = createTools()
    const wrapper = mount(ToolbarActions, { props: { tools } })

    await wrapper.find('.more-btn').trigger('click')
    await nextTick()

    const dropdownItems = wrapper.findAll('.dropdown-item')
    await dropdownItems[dropdownItems.length - 1].trigger('click')

    expect(wrapper.emitted('reset-toolbar')).toBeTruthy()
  })

  it('should close dropdown when clicking outside', async () => {
    const tools = createTools()
    const wrapper = mount(ToolbarActions, { props: { tools }, attachTo: document.body })

    await wrapper.find('.more-btn').trigger('click')
    await nextTick()
    await new Promise(resolve => setTimeout(resolve, 10))
    await nextTick()

    expect(wrapper.find('.toolbar-dropdown').exists()).toBe(true)

    document.body.click()
    await nextTick()

    expect(wrapper.find('.toolbar-dropdown').exists()).toBe(false)
  })

  it('should render empty state when no tools enabled', () => {
    const tools = createTools().map(t => ({ ...t, enabled: false }))
    const wrapper = mount(ToolbarActions, { props: { tools } })

    expect(wrapper.findAll('.toolbar-btn')).toHaveLength(0)
  })
})
