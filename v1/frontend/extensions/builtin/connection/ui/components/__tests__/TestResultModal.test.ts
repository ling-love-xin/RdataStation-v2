/**
 * @vitest-environment jsdom
 */
import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

vi.mock('lucide-vue-next', () => ({
  CheckCircle: { template: '<span>✓</span>', name: 'CheckCircle' },
  XCircle: { template: '<span>✗</span>', name: 'XCircle' },
}))

// Mock naive-ui NModal and NButton to render inline (no teleport)
vi.mock('naive-ui', () => ({
  NModal: {
    template: '<div v-if="show" class="n-modal-stub"><slot /></div>',
    props: ['show', 'maskClosable'],
    name: 'NModal',
  },
  NButton: {
    template: '<button @click="$emit(\'click\')"><slot /></button>',
    props: ['type', 'size'],
    name: 'NButton',
  },
}))

import TestResultModal from '../TestResultModal.vue'

describe('TestResultModal', () => {
  it('renders success state with check icon', () => {
    const wrapper = mount(TestResultModal, {
      props: {
        show: true,
        result: {
          success: true,
          message: 'Connection successful',
          responseTimeMs: 42,
          serverVersion: '8.0.36',
        },
        host: 'localhost',
        port: '3306',
        database: 'mydb',
        user: 'root',
        url: 'mysql://root:***@localhost:3306/mydb',
        driverName: 'MySQL',
      },
    })
    expect(wrapper.html()).toContain('navigator.testSuccess')
    expect(wrapper.html()).toContain('navigator.connected')
    expect(wrapper.html()).toContain('42ms')
    expect(wrapper.html()).toContain('8.0.36')
  })

  it('renders failure state with error icon', () => {
    const wrapper = mount(TestResultModal, {
      props: {
        show: true,
        result: {
          success: false,
          message: 'Connection refused',
        },
        host: 'localhost',
        port: '3306',
      },
    })
    expect(wrapper.html()).toContain('navigator.testFailed')
    expect(wrapper.html()).toContain('Connection refused')
  })

  it('renders hostPort correctly', () => {
    const wrapper = mount(TestResultModal, {
      props: {
        show: true,
        result: { success: true, message: 'OK' },
        host: 'db.example.com',
        port: '5432',
      },
    })
    expect(wrapper.html()).toContain('db.example.com:5432')
  })

  it('renders hostPort without port when port is empty', () => {
    const wrapper = mount(TestResultModal, {
      props: {
        show: true,
        result: { success: true, message: 'OK' },
        host: 'localhost',
        port: '',
      },
    })
    expect(wrapper.html()).toContain('localhost')
  })

  it('renders hostPort as dash when host is empty', () => {
    const wrapper = mount(TestResultModal, {
      props: {
        show: true,
        result: { success: true, message: 'OK' },
        host: '',
        port: '',
      },
    })
    expect(wrapper.html()).toContain('-')
  })

  it('shows driver name', () => {
    const wrapper = mount(TestResultModal, {
      props: {
        show: true,
        result: { success: true, message: 'OK' },
        host: 'localhost',
        driverName: 'PostgreSQL',
      },
    })
    expect(wrapper.html()).toContain('PostgreSQL')
  })

  it('emits close when confirm button clicked', async () => {
    const wrapper = mount(TestResultModal, {
      props: {
        show: true,
        result: { success: true, message: 'OK' },
        host: 'localhost',
      },
    })
    const btn = wrapper.find('button')
    await btn.trigger('click')
    expect(wrapper.emitted('close')).toBeTruthy()
  })
})
