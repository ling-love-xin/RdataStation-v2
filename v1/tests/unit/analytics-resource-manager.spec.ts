/**
 * @vitest-environment happy-dom
 */
import { mount } from '@vue/test-utils'
import { NMessageProvider } from 'naive-ui'
import { defineStore, setActivePinia, createPinia } from 'pinia'
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { ref, nextTick, h } from 'vue'

const mockT = vi.fn((key: string) => {
  const translations: Record<string, string> = {
    'analyticsResource.title': 'Analytics Resources',
    'analyticsResource.create': 'Create',
    'analyticsResource.empty': 'No resources found',
    'analyticsResource.save': 'Save',
  }
  return translations[key] ?? key
})

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: mockT }),
}))

vi.mock('lucide-vue-next', () => ({
  Database: { name: 'Database', template: '<span>DB</span>' },
  Edit: { name: 'Edit', template: '<span>Edit</span>' },
  Plus: { name: 'Plus', template: '<span>+</span>' },
  Trash2: { name: 'Trash2', template: '<span>Del</span>' },
}))

function makeResource(overrides = {}) {
  return {
    id: 'res-1',
    resource_type: 'table',
    name: 'Test Resource',
    alias: undefined,
    config: {},
    scope: 'project',
    version: 1,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-15T00:00:00Z',
    ...overrides,
  }
}

function createTestStore(initialResources = []) {
  const store = defineStore('analytics-resource', () => {
    const resources = ref([...initialResources])
    const loading = ref(false)
    const error = ref(null)

    return {
      resources,
      loading,
      error,
      deleteResource: () => {},
      createResource: () => {},
      updateResource: () => {},
    }
  })()

  vi.spyOn(store, 'deleteResource')
  vi.spyOn(store, 'createResource')
  vi.spyOn(store, 'updateResource')

  return store
}

import AnalyticsResourceManager from '../../src/extensions/builtin/analytics-resource/ui/components/AnalyticsResourceManager.vue'

function mountWithProvider(component: typeof AnalyticsResourceManager) {
  return mount(NMessageProvider, {
    slots: { default: () => h(component) },
  })
}

describe('AnalyticsResourceManager', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    setActivePinia(createPinia())
  })

  describe('Rendering', () => {
    it('should render the component title', () => {
      createTestStore()
      const wrapper = mountWithProvider(AnalyticsResourceManager)

      expect(wrapper.find('h2').text()).toBe('Analytics Resources')
    })

    it('should render create button', () => {
      createTestStore()
      const wrapper = mountWithProvider(AnalyticsResourceManager)

      expect(wrapper.find('.n-button').exists() || wrapper.find('button').exists()).toBe(true)
    })

    it('should render empty state when store has no resources and not loading', () => {
      createTestStore([])
      const wrapper = mountWithProvider(AnalyticsResourceManager)

      expect(wrapper.find('.empty-state').exists()).toBe(true)
    })

    it('should render resource cards when store has resources', () => {
      createTestStore([
        makeResource({ id: '1', name: 'Users' }),
        makeResource({ id: '2', name: 'Orders', resource_type: 'view' }),
      ])
      const wrapper = mountWithProvider(AnalyticsResourceManager)

      const cards = wrapper.findAll('.resource-card')
      expect(cards).toHaveLength(2)
    })
  })

  describe('Interactions', () => {
    it('should open create modal when create button is clicked', async () => {
      createTestStore()
      const wrapper = mountWithProvider(AnalyticsResourceManager)

      await wrapper.find('.n-button').trigger('click')
      await nextTick()

      expect(wrapper.findComponent({ name: 'CreateResourceModal' }).exists()).toBe(true)
    })

    it('should call store.deleteResource when delete button is clicked', async () => {
      const store = createTestStore([makeResource({ id: 'del-1', name: 'To Delete' })])
      const wrapper = mountWithProvider(AnalyticsResourceManager)

      await wrapper.findAll('.n-button')[2].trigger('click')

      expect(store.deleteResource).toHaveBeenCalledWith('del-1')
    })

    it('should close modal on close event', async () => {
      createTestStore()
      const wrapper = mountWithProvider(AnalyticsResourceManager)

      await wrapper.find('.n-button').trigger('click')
      await nextTick()

      const modal = wrapper.findComponent({ name: 'CreateResourceModal' })
      expect(modal.exists()).toBe(true)

      await modal.vm.$emit('close')
      await nextTick()

      const modalAfterClose = wrapper.findComponent({ name: 'CreateResourceModal' })
      expect(modalAfterClose.props('show')).toBe(false)
    })
  })

  describe('Edge Cases', () => {
    it('should render resource alias when present', () => {
      createTestStore([makeResource({ id: '1', name: 'Users', alias: 'usr' })])
      const wrapper = mountWithProvider(AnalyticsResourceManager)

      expect(wrapper.find('.resource-alias').exists()).toBe(true)
    })

    it('should handle multiple open/close modal cycles', async () => {
      createTestStore()
      const wrapper = mountWithProvider(AnalyticsResourceManager)

      for (let i = 0; i < 3; i++) {
        await wrapper.find('.n-button').trigger('click')
        await nextTick()
        expect(wrapper.findComponent({ name: 'CreateResourceModal' }).exists()).toBe(true)

        await wrapper.findComponent({ name: 'CreateResourceModal' }).vm.$emit('close')
        await nextTick()
        expect(wrapper.findComponent({ name: 'CreateResourceModal' }).props('show')).toBe(false)
      }
    })
  })
})
