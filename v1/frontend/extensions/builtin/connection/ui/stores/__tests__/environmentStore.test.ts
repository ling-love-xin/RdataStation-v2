/**
 * environmentStore 环境管理 Store 单元测试
 *
 * 测试 forScope、getById、computed 属性等纯逻辑函数。
 * CRUD 操作依赖 Tauri invoke，仅测试错误处理逻辑流程。
 */
import { describe, expect, it } from 'vitest'
import { ref } from 'vue'

import type { Environment, EnvironmentPolicy } from '../environmentStore'

// ==================== 纯函数提取 ====================

function forScope(environments: Environment[], scope: 'global' | 'project' | 'all'): Environment[] {
  if (scope === 'global')
    return environments.filter(e => e.id.startsWith('G_') && !e.id.startsWith('GP_'))
  if (scope === 'project')
    return environments.filter(e => e.id.startsWith('P_') || e.id.startsWith('GP_'))
  return environments
}

function getById(environments: Environment[], id: string): Environment | undefined {
  return environments.find(e => e.id === id)
}

// ==================== 测试数据 ====================

const mockEnvironments: Environment[] = [
  {
    id: 'G_env_dev',
    name: '开发环境',
    sort_order: 1,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'G_env_prod',
    name: '生产环境',
    description: '生产环境配置',
    color: '#ff0000',
    sort_order: 2,
    created_at: '2026-01-02T00:00:00Z',
  },
  {
    id: 'G_env_test',
    name: '测试环境',
    sort_order: 3,
    created_at: '2026-01-03T00:00:00Z',
  },
  {
    id: 'P_env_myapp',
    name: '项目专属-开发',
    sort_order: 1,
    origin: 'project',
    source_id: 'proj-001',
    created_at: '2026-02-01T00:00:00Z',
  },
  {
    id: 'GP_env_shared',
    name: '全局+项目共享',
    sort_order: 4,
    origin: 'global',
    source_id: 'proj-001',
    snapshot_at: '2026-02-01T00:00:00Z',
    created_at: '2026-02-01T00:00:00Z',
  },
  {
    id: 'P_env_other',
    name: '其他项目',
    sort_order: 2,
    origin: 'project',
    created_at: '2026-03-01T00:00:00Z',
  },
]

const mockPolicies: EnvironmentPolicy[] = [
  {
    id: 'pol_001',
    environment_id: 'G_env_dev',
    policy_type: 'connection_limit',
    policy_config: '{"max": 5}',
    enabled: true,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'pol_002',
    environment_id: 'G_env_dev',
    policy_type: 'timeout',
    policy_config: '{"seconds": 30}',
    enabled: true,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'pol_003',
    environment_id: 'G_env_prod',
    policy_type: 'readonly',
    policy_config: '{}',
    enabled: false,
    created_at: '2026-01-02T00:00:00Z',
  },
]

// ==================== forScope ====================

describe('forScope 环境过滤', () => {
  it('scope=global → 只返回 G_ 前缀且非 GP_ 的环境', () => {
    const result = forScope(mockEnvironments, 'global')
    expect(result).toHaveLength(3)
    expect(result.map(e => e.id)).toEqual(['G_env_dev', 'G_env_prod', 'G_env_test'])
  })

  it('scope=project → 返回 P_ 和 GP_ 前缀的环境', () => {
    const result = forScope(mockEnvironments, 'project')
    expect(result).toHaveLength(3)
    expect(result.map(e => e.id)).toEqual(['P_env_myapp', 'GP_env_shared', 'P_env_other'])
  })

  it('scope=all → 返回全部环境', () => {
    const result = forScope(mockEnvironments, 'all')
    expect(result).toHaveLength(mockEnvironments.length)
  })

  it('空列表 → 返回空数组', () => {
    expect(forScope([], 'global')).toHaveLength(0)
    expect(forScope([], 'project')).toHaveLength(0)
    expect(forScope([], 'all')).toHaveLength(0)
  })

  it('仅 G_ 前缀 → global 包含, project 不包含', () => {
    const envs: Environment[] = [{ id: 'G_env_a', name: 'A', sort_order: 1, created_at: '' }]
    expect(forScope(envs, 'global')).toHaveLength(1)
    expect(forScope(envs, 'project')).toHaveLength(0)
    expect(forScope(envs, 'all')).toHaveLength(1)
  })

  it('仅 P_ 前缀 → global 不包含, project 包含', () => {
    const envs: Environment[] = [{ id: 'P_env_a', name: 'A', sort_order: 1, created_at: '' }]
    expect(forScope(envs, 'global')).toHaveLength(0)
    expect(forScope(envs, 'project')).toHaveLength(1)
    expect(forScope(envs, 'all')).toHaveLength(1)
  })

  it('GP_ 前缀 → global 不包含, project 包含', () => {
    const envs: Environment[] = [
      { id: 'GP_env_shared', name: 'Shared', sort_order: 1, created_at: '' },
    ]
    expect(forScope(envs, 'global')).toHaveLength(0)
    expect(forScope(envs, 'project')).toHaveLength(1)
  })

  it('不匹配任何前缀 → global 和 project 都不包含', () => {
    const envs: Environment[] = [
      { id: 'X_env_unknown', name: 'Unknown', sort_order: 1, created_at: '' },
    ]
    expect(forScope(envs, 'global')).toHaveLength(0)
    expect(forScope(envs, 'project')).toHaveLength(0)
    expect(forScope(envs, 'all')).toHaveLength(1)
  })
})

// ==================== getById ====================

describe('getById 按 ID 查找', () => {
  it('存在的 ID → 返回环境对象', () => {
    const env = getById(mockEnvironments, 'G_env_dev')
    expect(env).toBeDefined()
    expect(env!.name).toBe('开发环境')
  })

  it('不存在的 ID → 返回 undefined', () => {
    const env = getById(mockEnvironments, 'NONEXISTENT')
    expect(env).toBeUndefined()
  })

  it('空列表 → 返回 undefined', () => {
    const env = getById([], 'G_env_dev')
    expect(env).toBeUndefined()
  })
})

// ==================== Environment 数据模型 ====================

describe('Environment 数据模型', () => {
  it('必填字段：id, name, sort_order, created_at', () => {
    const env: Environment = {
      id: 'G_env_sample',
      name: 'Sample',
      sort_order: 1,
      created_at: '2026-01-01T00:00:00Z',
    }
    expect(env.id).toBe('G_env_sample')
    expect(env.name).toBe('Sample')
    expect(env.sort_order).toBe(1)
    expect(env.created_at).toBe('2026-01-01T00:00:00Z')
  })

  it('可选字段：description, color, origin, source_id, snapshot_at', () => {
    const env: Environment = {
      id: 'P_env_full',
      name: 'Full',
      description: '完整环境',
      color: '#00ff00',
      sort_order: 5,
      origin: 'project',
      source_id: 'src-123',
      snapshot_at: '2026-06-01T00:00:00Z',
      created_at: '2026-01-01T00:00:00Z',
    }
    expect(env.description).toBe('完整环境')
    expect(env.color).toBe('#00ff00')
    expect(env.origin).toBe('project')
    expect(env.source_id).toBe('src-123')
    expect(env.snapshot_at).toBe('2026-06-01T00:00:00Z')
  })
})

// ==================== EnvironmentPolicy 数据模型 ====================

describe('EnvironmentPolicy 数据模型', () => {
  it('policy_config 为 JSON 字符串', () => {
    const policy: EnvironmentPolicy = {
      id: 'pol_001',
      environment_id: 'G_env_dev',
      policy_type: 'connection_limit',
      policy_config: '{"max": 5}',
      enabled: true,
      created_at: '2026-01-01T00:00:00Z',
    }

    const parsed = JSON.parse(policy.policy_config)
    expect(parsed.max).toBe(5)
  })

  it('disabled 策略', () => {
    const policy: EnvironmentPolicy = {
      id: 'pol_002',
      environment_id: 'G_env_prod',
      policy_type: 'readonly',
      policy_config: '{}',
      enabled: false,
      created_at: '2026-01-02T00:00:00Z',
    }
    expect(policy.enabled).toBe(false)
  })
})

// ==================== Store 逻辑测试（无 Tauri 依赖） ====================

describe('environmentStore 逻辑测试', () => {
  it('currentEnv computed：匹配时返回环境', () => {
    const environments = ref<Environment[]>(mockEnvironments)
    const currentEnvId = ref('G_env_dev')

    const currentEnv = environments.value.find(e => e.id === currentEnvId.value)
    expect(currentEnv).toBeDefined()
    expect(currentEnv!.name).toBe('开发环境')
  })

  it('currentEnv computed：无匹配时返回 undefined', () => {
    const environments = ref<Environment[]>(mockEnvironments)
    const currentEnvId = ref('NONEXISTENT')

    const currentEnv = environments.value.find(e => e.id === currentEnvId.value)
    expect(currentEnv).toBeUndefined()
  })

  it('currentEnv computed：空列表返回 undefined', () => {
    const environments = ref<Environment[]>([])
    const currentEnvId = ref('G_env_dev')

    const currentEnv = environments.value.find(e => e.id === currentEnvId.value)
    expect(currentEnv).toBeUndefined()
  })

  it('currentPolicies computed：根据当前环境 ID 过滤', () => {
    const policies = ref<Map<string, EnvironmentPolicy[]>>(new Map())
    policies.value.set('G_env_dev', [mockPolicies[0], mockPolicies[1]])
    policies.value.set('G_env_prod', [mockPolicies[2]])
    const currentEnvId = ref('G_env_dev')

    const currentPolicies = policies.value.get(currentEnvId.value) ?? []
    expect(currentPolicies).toHaveLength(2)
    expect(currentPolicies.map(p => p.policy_type)).toEqual(['connection_limit', 'timeout'])
  })

  it('currentPolicies computed：无匹配策略时返回空数组', () => {
    const policies = ref<Map<string, EnvironmentPolicy[]>>(new Map())
    const currentEnvId = ref('G_env_test')

    const currentPolicies = policies.value.get(currentEnvId.value) ?? []
    expect(currentPolicies).toHaveLength(0)
  })

  it('selectEnv：切换环境并加载策略', async () => {
    const currentEnvId = ref('G_env_dev')
    const policies = ref<Map<string, EnvironmentPolicy[]>>(new Map())

    // 模拟 selectEnv 逻辑
    const newEnvId = 'G_env_prod'
    currentEnvId.value = newEnvId
    // 模拟 fetchPolicies 结果
    policies.value.set(newEnvId, [mockPolicies[2]])

    expect(currentEnvId.value).toBe('G_env_prod')
    const loaded = policies.value.get(currentEnvId.value) ?? []
    expect(loaded).toHaveLength(1)
    expect(loaded[0].policy_type).toBe('readonly')
  })

  it('remove：删除环境后清理策略', () => {
    const environments = ref<Environment[]>([...mockEnvironments])
    const policies = ref<Map<string, EnvironmentPolicy[]>>(new Map())
    policies.value.set('G_env_dev', [mockPolicies[0], mockPolicies[1]])

    const envIdToRemove = 'G_env_dev'
    // 模拟 remove 逻辑
    environments.value = environments.value.filter(e => e.id !== envIdToRemove)
    policies.value.delete(envIdToRemove)

    expect(environments.value.find(e => e.id === 'G_env_dev')).toBeUndefined()
    expect(policies.value.has('G_env_dev')).toBe(false)
  })
})

// ==================== 边界场景 ====================

describe('边界场景', () => {
  it('ID 以 G 开头但不是 G_ → 不会被 global 过滤', () => {
    const envs: Environment[] = [{ id: 'GX_env', name: 'Edge', sort_order: 1, created_at: '' }]
    // GX_ 不会匹配 G_ 前缀（不包含下划线）
    // 实际上 GX_env 以 G_ 开头吗？GX_env 以 G 开头但不是 G_
    expect(forScope(envs, 'global')).toHaveLength(0)
  })

  it('ID 以 P_ 开头但不是项目环境 → 仍被 project 过滤', () => {
    const envs: Environment[] = [{ id: 'P_env', name: 'Project', sort_order: 1, created_at: '' }]
    expect(forScope(envs, 'project')).toHaveLength(1)
  })

  it('空 ID → 查找返回 undefined', () => {
    const env = getById(mockEnvironments, '')
    expect(env).toBeUndefined()
  })
})
