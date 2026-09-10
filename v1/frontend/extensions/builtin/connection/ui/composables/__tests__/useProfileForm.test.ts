/**
 * useProfileForm 单元测试
 *
 * 测试 isGlobalProfile 纯函数 + useProfileForm 表单 CRUD 逻辑。
 */
import { describe, expect, it } from 'vitest'

import { isGlobalProfile } from '../useProfileForm'

import type { NetworkProfile } from '../useNetworkProfiles'

// ==================== isGlobalProfile ====================

describe('isGlobalProfile 全局 Profile 判断', () => {
  it('origin=global → true', () => {
    const p: NetworkProfile = {
      id: 'G_ssh_001',
      name: 'Global SSH',
      type: 'ssh',
      config: {},
      detail: 'localhost:22',
      origin: 'global',
    }
    expect(isGlobalProfile(p)).toBe(true)
  })

  it('origin=project → false', () => {
    const p: NetworkProfile = {
      id: 'P_ssh_001',
      name: 'Project SSH',
      type: 'ssh',
      config: {},
      detail: 'localhost:22',
      origin: 'project',
    }
    expect(isGlobalProfile(p)).toBe(false)
  })

  it('origin=undefined → false', () => {
    const p: NetworkProfile = {
      id: 'G_ssh_002',
      name: 'No Origin',
      type: 'ssh',
      config: {},
      detail: 'localhost:22',
    }
    expect(isGlobalProfile(p)).toBe(false)
  })

  it('SSL profile → 同样规则', () => {
    const p: NetworkProfile = {
      id: 'G_ssl_001',
      name: 'Global SSL',
      type: 'ssl',
      config: {},
      detail: 'verify-full',
      origin: 'global',
    }
    expect(isGlobalProfile(p)).toBe(true)
  })

  it('Proxy profile → 同样规则', () => {
    const p: NetworkProfile = {
      id: 'P_proxy_001',
      name: 'Project Proxy',
      type: 'proxy',
      config: {},
      detail: 'proxy:1080',
      origin: 'project',
    }
    expect(isGlobalProfile(p)).toBe(false)
  })
})
