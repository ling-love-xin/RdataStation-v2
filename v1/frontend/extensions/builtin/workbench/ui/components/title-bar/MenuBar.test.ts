import { mount } from '@vue/test-utils'
import { describe, it, expect, vi } from 'vitest'
import { nextTick } from 'vue'

import MenuBar from './MenuBar.vue'

// Mock vue-i18n
vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string) => key,
  }),
}))

const mockMenus = [
  {
    id: 'file',
    label: 'File',
    items: [
      { id: 'new', label: 'New', shortcut: 'Ctrl+N' },
      { id: 'open', label: 'Open', shortcut: 'Ctrl+O' },
      { id: 'separator1', separator: true },
      { id: 'save', label: 'Save', shortcut: 'Ctrl+S' },
    ],
  },
  {
    id: 'edit',
    label: 'Edit',
    items: [
      { id: 'undo', label: 'Undo', shortcut: 'Ctrl+Z' },
      { id: 'redo', label: 'Redo', shortcut: 'Ctrl+Y' },
    ],
  },
]

async function openMenuBar(wrapper: ReturnType<typeof mount>) {
  const hamburger = wrapper.find('.hamburger-btn')
  if (!hamburger.exists()) return
  await hamburger.trigger('click')
  await nextTick()
}

describe('MenuBar', () => {
  it('should render menu items', async () => {
    const wrapper = mount(MenuBar, {
      props: { menus: mockMenus },
    })

    await openMenuBar(wrapper)

    const menuItems = wrapper.findAll('.menu-item')
    expect(menuItems).toHaveLength(2)
    expect(menuItems[0].text()).toBe('File')
    expect(menuItems[1].text()).toBe('Edit')
  })

  it('should open dropdown on menu click', async () => {
    const wrapper = mount(MenuBar, {
      props: { menus: mockMenus },
    })

    await openMenuBar(wrapper)

    const menuItem = wrapper.find('.menu-item')
    await menuItem.trigger('click')

    expect(wrapper.find('.dropdown-panel').exists()).toBe(true)
    expect(wrapper.text()).toContain('New')
    expect(wrapper.text()).toContain('Open')
  })

  it('should emit menu-action on item click', async () => {
    const wrapper = mount(MenuBar, {
      props: { menus: mockMenus },
    })

    await openMenuBar(wrapper)

    const menuItem = wrapper.find('.menu-item')
    await menuItem.trigger('click')

    const dropdownItem = wrapper.find('.dropdown-item')
    await dropdownItem.trigger('click')

    expect(wrapper.emitted('menu-action')).toBeTruthy()
    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    expect(wrapper.emitted('menu-action')![0][0]).toMatchObject({ id: 'new' })
  })

  it('should close dropdown on click outside', async () => {
    const wrapper = mount(MenuBar, {
      props: { menus: mockMenus },
      attachTo: document.body,
    })

    await openMenuBar(wrapper)

    const menuItem = wrapper.find('.menu-item')
    await menuItem.trigger('click')
    expect(wrapper.find('.dropdown-panel').exists()).toBe(true)

    await document.body.click()
    await nextTick()

    expect(wrapper.find('.dropdown-panel').exists()).toBe(false)

    wrapper.unmount()
  })

  it('should close dropdown on escape key', async () => {
    const wrapper = mount(MenuBar, {
      props: { menus: mockMenus },
    })

    await openMenuBar(wrapper)

    const menuItem = wrapper.find('.menu-item')
    await menuItem.trigger('click')
    expect(wrapper.find('.dropdown-panel').exists()).toBe(true)

    await document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await nextTick()

    expect(wrapper.find('.dropdown-panel').exists()).toBe(false)
  })

  it('should not emit for disabled items', async () => {
    const menusWithDisabled = [
      {
        id: 'file',
        label: 'File',
        items: [
          { id: 'new', label: 'New', disabled: true },
          { id: 'open', label: 'Open' },
        ],
      },
    ]

    const wrapper = mount(MenuBar, {
      props: { menus: menusWithDisabled },
    })

    await openMenuBar(wrapper)

    const menuItem = wrapper.find('.menu-item')
    await menuItem.trigger('click')

    const disabledItem = wrapper.find('.dropdown-item.disabled')
    await disabledItem.trigger('click')

    expect(wrapper.emitted('menu-action')).toBeFalsy()
  })

  it('should render separator items', async () => {
    const wrapper = mount(MenuBar, {
      props: { menus: mockMenus },
    })

    await openMenuBar(wrapper)

    const menuItem = wrapper.find('.menu-item')
    await menuItem.trigger('click')

    const separators = wrapper.findAll('.dropdown-divider')
    expect(separators).toHaveLength(1)
  })

  it('should render shortcuts', async () => {
    const wrapper = mount(MenuBar, {
      props: { menus: mockMenus },
    })

    await openMenuBar(wrapper)

    const menuItem = wrapper.find('.menu-item')
    await menuItem.trigger('click')

    expect(wrapper.text()).toContain('Ctrl+N')
    expect(wrapper.text()).toContain('Ctrl+O')
  })

  it('should toggle hamburger menu', async () => {
    const wrapper = mount(MenuBar, {
      props: { menus: mockMenus },
    })

    const hamburger = wrapper.find('.hamburger-btn')

    expect(wrapper.find('.menu-bar').exists()).toBe(false)

    await hamburger.trigger('click')
    await nextTick()
    expect(wrapper.find('.menu-bar').exists()).toBe(true)

    await hamburger.trigger('click')
    await nextTick()
    expect(wrapper.find('.menu-bar').exists()).toBe(false)
  })

  it('should have correct aria attributes', async () => {
    const wrapper = mount(MenuBar, {
      props: { menus: mockMenus },
    })

    const hamburger = wrapper.find('.hamburger-btn')
    expect(hamburger.attributes('aria-haspopup')).toBe('true')

    await hamburger.trigger('click')
    await nextTick()

    const menuBar = wrapper.find('.menu-bar')
    expect(menuBar.attributes('role')).toBe('menubar')

    const menuItems = wrapper.findAll('.menu-item')
    expect(menuItems[0].attributes('role')).toBe('menuitem')
    expect(menuItems[0].attributes('aria-haspopup')).toBe('true')

    await menuItems[0].trigger('click')

    const dropdownPanel = wrapper.find('.dropdown-panel')
    expect(dropdownPanel.attributes('role')).toBe('menu')

    const dropdownItems = wrapper.findAll('.dropdown-item')
    expect(dropdownItems[0].attributes('role')).toBe('menuitem')
  })
})
