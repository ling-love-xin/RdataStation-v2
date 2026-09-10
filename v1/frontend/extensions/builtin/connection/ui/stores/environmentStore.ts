/**
 * environmentStore — 环境 + 策略 Pinia Store
 *
 * 管理环境列表和策略数据的加载、缓存。
 * 环境 CRUD 通过 Tauri invoke 调用后端 API。
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export interface Environment {
  id: string
  name: string
  description?: string
  color?: string
  sort_order: number
  created_at: string
  origin?: string
  source_id?: string
  snapshot_at?: string
}

export interface EnvironmentPolicy {
  id: string
  environment_id: string
  policy_type: string
  policy_config: string
  enabled: boolean
  created_at: string
}

export const useEnvironmentStore = defineStore('environment', () => {
  const environments = ref<Environment[]>([])
  const policies = ref<Map<string, EnvironmentPolicy[]>>(new Map())
  const currentEnvId = ref<string>('G_env_dev')
  const loading = ref(false)

  const currentEnv = computed(
    () => environments.value.find(e => e.id === currentEnvId.value) ?? null
  )

  const currentPolicies = computed(() => policies.value.get(currentEnvId.value) ?? [])

  async function fetchAll() {
    loading.value = true
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      environments.value = await invoke<Environment[]>('list_environments')
    } catch (e) {
      console.error('[environmentStore] fetchAll failed:', e)
    } finally {
      loading.value = false
    }
  }

  async function fetchPolicies(envId: string) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const result = await invoke<EnvironmentPolicy[]>('list_environment_policies', {
        environmentId: envId,
      })
      policies.value.set(envId, result)
    } catch (e) {
      console.error('[environmentStore] fetchPolicies failed:', e)
    }
  }

  async function create(env: Omit<Environment, 'id' | 'created_at'>) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('create_environment', { env: { ...env, id: '', created_at: '' } })
      await fetchAll()
    } catch (e) {
      console.error('[environmentStore] create failed:', e)
      throw e
    }
  }

  async function update(env: Environment) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('update_environment', { env })
      await fetchAll()
    } catch (e) {
      console.error('[environmentStore] update failed:', e)
      throw e
    }
  }

  async function remove(envId: string) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('delete_environment', { id: envId })
      policies.value.delete(envId)
      await fetchAll()
    } catch (e) {
      console.error('[environmentStore] remove failed:', e)
      throw e
    }
  }

  function getById(id: string): Environment | undefined {
    return environments.value.find(e => e.id === id)
  }

  /** 按作用域过滤环境列表 */
  function forScope(scope: 'global' | 'project' | 'all'): Environment[] {
    if (scope === 'global')
      return environments.value.filter(e => e.id.startsWith('G_') && !e.id.startsWith('GP_'))
    if (scope === 'project')
      return environments.value.filter(e => e.id.startsWith('P_') || e.id.startsWith('GP_'))
    return environments.value
  }

  async function selectEnv(envId: string) {
    currentEnvId.value = envId
    await fetchPolicies(envId)
  }

  return {
    environments,
    policies,
    currentEnvId,
    currentEnv,
    currentPolicies,
    loading,
    fetchAll,
    fetchPolicies,
    create,
    update,
    remove,
    getById,
    forScope,
    selectEnv,
  }
})
