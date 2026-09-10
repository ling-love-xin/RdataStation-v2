import { mount, VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { nextTick } from 'vue'

import CommandPalette from './CommandPalette.vue'
import { useCommandStore } from '../../stores/command-store'

vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string) => key,
  }),
}))

describe('CommandPalette', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  function mountPalette(visible: boolean): VueWrapper<InstanceType<typeof CommandPalette>> {
    return mount(CommandPalette, {
      props: { visible },
      global: {
        stubs: {
          Teleport: true,
        },
      },
    })
  }

  it('should display registered commands', async () => {
    const store = useCommandStore()
    store.register({
      id: 'test-cmd',
      label: 'Test Command',
      category: 'test',
      action: vi.fn(),
    })

    const wrapper = mountPalette(true)
    await nextTick()

    expect(wrapper.text()).toContain('Test Command')
  })

  it('should filter commands by search query', async () => {
    const store = useCommandStore()
    store.register({ id: 'new-query', label: 'New Query', category: 'file', action: vi.fn() })
    store.register({ id: 'open-project', label: 'Open Project', category: 'file', action: vi.fn() })

    const wrapper = mountPalette(true)
    await nextTick()

    const input = wrapper.find('.command-palette-input')
    expect(input.exists()).toBe(true)
    await input.setValue('new query')
    await nextTick()

    const results = wrapper.find('.command-palette-results')
    expect(results.text()).toContain('New Query')
    expect(results.text()).not.toContain('Open Project')
  })

  it('should execute command on click', async () => {
    const store = useCommandStore()
    const action = vi.fn()
    store.register({
      id: 'test-cmd',
      label: 'Test Command',
      category: 'test',
      action,
    })

    const wrapper = mountPalette(true)
    await nextTick()

    const commandItem = wrapper.find('.command-item')
    expect(commandItem.exists()).toBe(true)
    await commandItem.trigger('click')
    await nextTick()

    expect(action).toHaveBeenCalledTimes(1)
  })

  it('should emit close event on escape key', async () => {
    const wrapper = mountPalette(true)
    await nextTick()

    const input = wrapper.find('.command-palette-input')
    expect(input.exists()).toBe(true)
    await input.trigger('keydown.esc')
    await nextTick()

    expect(wrapper.emitted('close')).toBeTruthy()
  })

  it('should emit close event on overlay click', async () => {
    const wrapper = mountPalette(true)
    await nextTick()

    const overlay = wrapper.find('.command-palette-overlay')
    expect(overlay.exists()).toBe(true)
    await overlay.trigger('click')
    await nextTick()

    expect(wrapper.emitted('close')).toBeTruthy()
  })

  it('should navigate with arrow keys', async () => {
    const store = useCommandStore()
    store.register({ id: 'cmd1', label: 'Command 1', category: 'test', action: vi.fn() })
    store.register({ id: 'cmd2', label: 'Command 2', category: 'test', action: vi.fn() })

    const wrapper = mountPalette(true)
    await nextTick()

    let items = wrapper.findAll('.command-item')
    expect(items.length).toBeGreaterThanOrEqual(2)
    expect(items[0].classes()).toContain('active')

    const input = wrapper.find('.command-palette-input')
    await input.trigger('keydown', { key: 'ArrowDown' })
    await nextTick()

    items = wrapper.findAll('.command-item')
    expect(items[1].classes()).toContain('active')

    await input.trigger('keydown', { key: 'ArrowUp' })
    await nextTick()

    items = wrapper.findAll('.command-item')
    expect(items[0].classes()).toContain('active')
  })

  it('should execute command on enter key', async () => {
    const store = useCommandStore()
    const action = vi.fn()
    store.register({
      id: 'test-cmd',
      label: 'Test Command',
      category: 'test',
      action,
    })

    const wrapper = mountPalette(true)
    await nextTick()

    const input = wrapper.find('.command-palette-input')
    expect(input.exists()).toBe(true)
    await input.trigger('keydown', { key: 'Enter' })
    await nextTick()

    expect(action).toHaveBeenCalledTimes(1)
    expect(wrapper.emitted('close')).toBeTruthy()
  })

  it('should display no results message when search has no matches', async () => {
    const wrapper = mountPalette(true)
    await nextTick()

    const input = wrapper.find('.command-palette-input')
    expect(input.exists()).toBe(true)
    await input.setValue('xyz-nonexistent')
    await nextTick()

    expect(wrapper.text()).toContain('commandPalette.noResults')
  })
})
