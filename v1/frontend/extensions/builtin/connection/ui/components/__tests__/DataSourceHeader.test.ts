/**
 * @vitest-environment jsdom
 */
import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'

import DataSourceHeader from '../DataSourceHeader.vue'

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

vi.mock('lucide-vue-next', () => ({
  Edit: { template: '<span>Edit</span>', name: 'Edit' },
}))

// Stub naive-ui components
const naiveStubs = {
  NInput: {
    template:
      '<input :value="value" :placeholder="placeholder" @input="$emit(\'update:value\', ($event.target as HTMLInputElement).value)" />',
    props: ['value', 'placeholder', 'type', 'size', 'rows'],
    name: 'NInput',
  },
  NSelect: {
    template:
      '<select :value="value" @change="$emit(\'update:value\', ($event.target as HTMLSelectElement).value)"><option v-for="opt in options" :key="opt.value" :value="opt.value">{{ opt.label }}</option></select>',
    props: ['value', 'options', 'placeholder', 'size'],
    name: 'NSelect',
  },
  NCheckbox: {
    template:
      '<label><input type="checkbox" :checked="checked" @change="$emit(\'update:checked\', ($event.target as HTMLInputElement).checked)" /><span><slot /></span></label>',
    props: ['checked', 'size'],
    name: 'NCheckbox',
  },
  NButton: {
    template:
      '<button :type="type" :disabled="disabled" @click="$emit(\'click\')"><slot name="icon" /><slot /></button>',
    props: ['type', 'size', 'disabled', 'quaternary'],
    name: 'NButton',
  },
  NAlert: {
    template: '<div v-if="title" class="n-alert" :class="type">{{ title }}</div>',
    props: ['type', 'title', 'closable'],
    name: 'NAlert',
  },
}

describe('DataSourceHeader', () => {
  const baseProps = {
    name: 'My DB',
    description: '',
    scopeGlobal: true,
    scopeProject: false,
    selectedDriverId: null,
    driverOptions: [],
    uriPreview: '',
    uriEditing: false,
    manualUri: '',
    nameLabel: 'Name',
    namePlaceholder: 'Enter name',
    descLabel: 'Desc',
    descPlaceholder: 'Enter desc',
    globalLabel: 'Global',
    projectLabel: 'Project',
    driverLabel: 'Driver',
    driverPlaceholder: 'Select',
    uriLabel: 'URI',
    uriPlaceholder: 'jdbc:...',
  }

  it('renders name input', () => {
    const wrapper = mount(DataSourceHeader, {
      props: baseProps,
      global: { stubs: naiveStubs },
    })
    expect(wrapper.html()).toContain('My DB')
  })

  it('renders description textarea', () => {
    const wrapper = mount(DataSourceHeader, {
      props: { ...baseProps, name: '', description: 'A test DB' },
      global: { stubs: naiveStubs },
    })
    expect(wrapper.html()).toContain('A test DB')
  })

  it('renders driver select with options', () => {
    const wrapper = mount(DataSourceHeader, {
      props: {
        ...baseProps,
        name: '',
        selectedDriverId: 'mysql',
        driverOptions: [
          { label: 'MySQL', value: 'mysql' },
          { label: 'PostgreSQL', value: 'postgresql' },
        ],
      },
      global: { stubs: naiveStubs },
    })
    expect(wrapper.html()).toContain('MySQL')
  })

  it('renders URI preview', () => {
    const wrapper = mount(DataSourceHeader, {
      props: {
        ...baseProps,
        name: '',
        uriPreview: 'mysql://root:***@localhost:3306/mydb',
      },
      global: { stubs: naiveStubs },
    })
    expect(wrapper.html()).toContain('mysql://root:***@localhost:3306/mydb')
  })

  it('renders URI edit input when editing', () => {
    const wrapper = mount(DataSourceHeader, {
      props: {
        ...baseProps,
        name: '',
        uriEditing: true,
        manualUri: 'mysql://localhost:3306/db',
      },
      global: { stubs: naiveStubs },
    })
    expect(wrapper.html()).toContain('mysql://localhost:3306/db')
  })

  it('shows parse button when editing with manual URI', () => {
    const wrapper = mount(DataSourceHeader, {
      props: {
        ...baseProps,
        name: '',
        uriEditing: true,
        manualUri: 'mysql://localhost:3306/db',
      },
      global: { stubs: naiveStubs },
    })
    expect(wrapper.html()).toContain('navigator.parseUrl')
  })

  it('shows warning when URL protocol mismatch', () => {
    const wrapper = mount(DataSourceHeader, {
      props: {
        ...baseProps,
        name: '',
        uriEditing: true,
        manualUri: 'postgres://host:5432/db',
        urlTemplate: 'mysql://{host}:{port}/{database}',
      },
      global: { stubs: naiveStubs },
    })
    expect(wrapper.html()).toContain('navigator.parseUrl')
  })
})
