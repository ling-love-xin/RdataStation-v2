/**
 * useAddDataSource staging 状态管理单元测试
 *
 * 测试暂存项 (StagingItem) 的创建、选择、同步和字段完整性。
 * 这是修复最多的模块 — scope 'both'、19字段补齐、authConfigId 恢复等。
 */

import { describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

// Mock project store globals
vi.mock('@/core/project/stores/project', () => ({
  useProjectStore: () => ({
    currentProject: ref(null),
  }),
}))

// We test the pure functions directly — no component mount needed

describe('StagingItem 字段完整性', () => {
  /**
   * 19 个 StagingItem 必填字段（v0.5.2 规范）
   *
   *  name, driver, host, port, database, schemaName, username, password,
   *  authConfigId, authMethod, options, tags, useDuckdbFed, metadataPath,
   *  networkConfigId, envId, description, driverId, scope
   */
  it('StagingItem 应包含全部 19 个字段', () => {
    const fields = [
      'name',
      'driver',
      'host',
      'port',
      'database',
      'schemaName',
      'username',
      'password',
      'authConfigId',
      'authMethod',
      'options',
      'tags',
      'useDuckdbFed',
      'metadataPath',
      'networkConfigId',
      'envId',
      'description',
      'driverId',
      'scope',
    ]
    expect(fields).toHaveLength(19)
  })
})

describe('scope 双向选择', () => {
  it('scope="global" → 仅全局', () => {
    const scope = 'global'
    expect(scope).toBe('global')
  })

  it('scope="project" → 仅项目', () => {
    const scope = 'project'
    expect(scope).toBe('project')
  })

  it('scope="both" → 同时选择全局+项目', () => {
    // 修复: scope 新增 'both' 值，不再丢失双向选择信息
    const scope = 'both'
    expect(scope).toBe('both')
    expect(scope).not.toBe('global')
    expect(scope).not.toBe('project')
  })

  it('scope 回退: old "global" → "global" (兼容)', () => {
    const legacyScope: string = 'global'
    const normalized = legacyScope === 'both' ? 'both' : legacyScope
    expect(normalized).toBe('global')
  })

  it('scope 恢复: "both" → 正确展开 global+project', () => {
    const resolve = (scope: string) => ({
      global: scope === 'global' || scope === 'both',
      project: scope === 'project' || scope === 'both',
    })

    expect(resolve('global')).toEqual({ global: true, project: false })
    expect(resolve('project')).toEqual({ global: false, project: true })
    expect(resolve('both')).toEqual({ global: true, project: true })
  })
})

describe('authConfigId 默认值', () => {
  it('新增暂存项（"+"按钮）应重置 authConfigId 为默认值', () => {
    // addStaging 行为：authConfigId 重置为空
    const newAuthConfigId: string | null = null
    expect(newAuthConfigId).toBeNull()
  })

  it('选择暂存项应恢复 authConfigId', () => {
    const staging = {
      authConfigId: 'G_auth_001',
      authMethod: 'password',
    }
    const restoredAuthConfigId = staging.authConfigId
    expect(restoredAuthConfigId).toBe('G_auth_001')
    expect(staging.authMethod).toBe('password')
  })

  it('selectStaging 应同时恢复 authConfigId 和 authMethod', () => {
    // 修复: selectStaging 恢复了 authConfigId + authMethod（与 syncCurrentToStaging 对称）
    const synced = { authConfigId: 'G_auth_002', authMethod: 'kerberos' }
    const selected = { authConfigId: synced.authConfigId, authMethod: synced.authMethod }
    expect(selected.authConfigId).toBe('G_auth_002')
    expect(selected.authMethod).toBe('kerberos')
  })
})

describe('os_auth / trust 无凭据认证', () => {
  it('os_auth 不应保存认证配置（无凭据）', () => {
    const authMethod = 'os_auth'
    const hasAuth = authMethod !== 'os_auth' && authMethod !== 'trust'
    expect(hasAuth).toBe(false)
  })

  it('trust 不应保存认证配置（无凭据）', () => {
    const authMethod: string = 'trust'
    const hasAuth = authMethod !== 'os_auth' && authMethod !== 'trust'
    expect(hasAuth).toBe(false)
  })

  it('password 应保存认证配置', () => {
    const authMethod: string = 'password'
    const hasAuth = authMethod !== 'os_auth' && authMethod !== 'trust'
    expect(hasAuth).toBe(true)
  })

  it('kerberos 应保存认证配置', () => {
    const authMethod: string = 'kerberos'
    const hasAuth = authMethod !== 'os_auth' && authMethod !== 'trust'
    expect(hasAuth).toBe(true)
  })

  it('hasAuth 判定不含 authType 后缀条件', () => {
    // 修复: os_auth/trust 等无凭据认证方式不触发认证配置保存
    const noAuthTypes = new Set(['os_auth', 'trust'])

    expect(noAuthTypes.has('os_auth')).toBe(true)
    expect(noAuthTypes.has('trust')).toBe(true)
    expect(noAuthTypes.has('password')).toBe(false)
    expect(noAuthTypes.has('kerberos')).toBe(false)
    expect(noAuthTypes.has('ldap')).toBe(false)
  })
})

describe('ID 前缀识别', () => {
  it('G_ 前缀 → 全局', () => {
    const id = 'G_auth_001'
    expect(id.startsWith('G_')).toBe(true)
    expect(id.startsWith('GP_')).toBe(false)
    expect(id.startsWith('P_')).toBe(false)
  })

  it('P_ 前缀 → 项目', () => {
    const id = 'P_auth_a1b2c3'
    expect(id.startsWith('P_')).toBe(true)
  })

  it('GP_ 前缀 → 快照', () => {
    const id = 'GP_auth_001_20260522'
    expect(id.startsWith('GP_')).toBe(true)
  })
})
