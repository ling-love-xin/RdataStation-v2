/**
 * useSecurityPolicies 安全策略单元测试
 *
 * 测试 securitySummary、isPolicyOverridden、applyEnvDefaults、collectPolicyConfig、
 * applyPolicyConfig、dropOpts 等，通过直接操作 ref 来驱动 computed 重新计算。
 */
import { describe, expect, it } from 'vitest'

import { useSecurityPolicies } from '../useSecurityPolicies'

// ==================== dropOpts ====================

describe('dropOpts DROP 选项', () => {
  it('三个选项：允许/确认/禁用', () => {
    const { dropOpts } = useSecurityPolicies({ value: 'env-dev' })
    expect(dropOpts).toHaveLength(3)
    expect(dropOpts[0]).toEqual({ label: '允许', value: 'false' })
    expect(dropOpts[1]).toEqual({ label: '确认', value: 'true' })
    expect(dropOpts[2]).toEqual({ label: '禁用', value: 'disable' })
  })
})

// ==================== securitySummary ====================

describe('securitySummary 安全摘要 computed', () => {
  it('dev 默认 → 读写', () => {
    const { applyEnvDefaults, securitySummary } = useSecurityPolicies({ value: 'env-dev' })
    applyEnvDefaults('env-dev')
    expect(securitySummary.value).toBe('读写')
  })

  it('只读模式 → 只读', () => {
    const { polReadonly, securitySummary } = useSecurityPolicies({ value: 'env-dev' })
    polReadonly.value = true
    expect(securitySummary.value).toContain('只读')
  })

  it('写确认 + DDL确认 + DROP禁用 + 行限制 → 完整摘要', () => {
    const {
      polReadonly,
      polWriteConfirm,
      polDdlConfirm,
      polDrop,
      polRowLimit,
      polSizeLimit,
      securitySummary,
    } = useSecurityPolicies({ value: 'env-prod' })
    polReadonly.value = true
    polWriteConfirm.value = true
    polDdlConfirm.value = true
    polDrop.value = 'disable'
    polRowLimit.value = 1000
    polSizeLimit.value = 20
    const summary = securitySummary.value
    expect(summary).toContain('只读')
    expect(summary).toContain('写确认')
    expect(summary).toContain('DDL确认')
    expect(summary).toContain('DROP禁用')
    expect(summary).toContain('行限1000')
    expect(summary).toContain('限20M')
  })

  it('DROP确认 → 摘要含 DROP确认', () => {
    const { polDrop, securitySummary } = useSecurityPolicies({ value: 'env-dev' })
    polDrop.value = 'true'
    expect(securitySummary.value).toContain('DROP确认')
  })

  it('无任何策略 → 读写（polReadonly=false 时始终推送"读写"）', () => {
    const { securitySummary } = useSecurityPolicies({ value: 'env-dev' })
    // polReadonly 为 false 时，else 分支推送 "读写"
    expect(securitySummary.value).toBe('读写')
  })
})

// ==================== isPolicyOverridden ====================

describe('isPolicyOverridden 策略覆盖检测', () => {
  it('env-dev 默认策略 → 无覆盖', () => {
    const { applyEnvDefaults, isPolicyOverridden } = useSecurityPolicies({ value: 'env-dev' })
    applyEnvDefaults('env-dev')
    expect(isPolicyOverridden.value).toBe(false)
  })

  it('env-dev 修改只读 → 有覆盖', () => {
    const { applyEnvDefaults, polReadonly, isPolicyOverridden } = useSecurityPolicies({
      value: 'env-dev',
    })
    applyEnvDefaults('env-dev')
    polReadonly.value = true
    expect(isPolicyOverridden.value).toBe(true)
  })

  it('env-prod 默认策略 → 无覆盖', () => {
    const { applyEnvDefaults, isPolicyOverridden } = useSecurityPolicies({ value: 'env-prod' })
    applyEnvDefaults('env-prod')
    expect(isPolicyOverridden.value).toBe(false)
  })

  it('env-prod 修改 rowLimit → 有覆盖', () => {
    const { applyEnvDefaults, polRowLimit, isPolicyOverridden } = useSecurityPolicies({
      value: 'env-prod',
    })
    applyEnvDefaults('env-prod')
    polRowLimit.value = 500
    expect(isPolicyOverridden.value).toBe(true)
  })

  it('不匹配任何预定义环境 → 无覆盖', () => {
    const { isPolicyOverridden } = useSecurityPolicies({ value: 'env-custom' })
    expect(isPolicyOverridden.value).toBe(false)
  })
})

// ==================== applyEnvDefaults ====================

describe('applyEnvDefaults 环境默认值应用', () => {
  it('env-dev → 读写、自动提交、无限制', () => {
    const { applyEnvDefaults, polReadonly, polAutocommit, polRowLimit, polSizeLimit } =
      useSecurityPolicies({ value: 'env-dev' })
    applyEnvDefaults('env-dev')
    expect(polReadonly.value).toBe(false)
    expect(polAutocommit.value).toBe(true)
    expect(polRowLimit.value).toBe(0)
    expect(polSizeLimit.value).toBe(0)
  })

  it('env-test → DDL确认、行限10000', () => {
    const { applyEnvDefaults, polDdlConfirm, polDrop, polRowLimit, polSizeLimit } =
      useSecurityPolicies({ value: 'env-test' })
    applyEnvDefaults('env-test')
    expect(polDdlConfirm.value).toBe(true)
    expect(polDrop.value).toBe('true')
    expect(polRowLimit.value).toBe(10000)
    expect(polSizeLimit.value).toBe(100)
  })

  it('env-staging → 写确认、自动提交关闭', () => {
    const { applyEnvDefaults, polWriteConfirm, polAutocommit, polDrop, polRowLimit } =
      useSecurityPolicies({ value: 'env-staging' })
    applyEnvDefaults('env-staging')
    expect(polWriteConfirm.value).toBe(true)
    expect(polAutocommit.value).toBe(false)
    expect(polDrop.value).toBe('true')
    expect(polRowLimit.value).toBe(5000)
  })

  it('env-prod → 只读、写确认、DROP禁用', () => {
    const {
      applyEnvDefaults,
      polReadonly,
      polWriteConfirm,
      polDdlConfirm,
      polDrop,
      polAutocommit,
      polRowLimit,
      polSizeLimit,
    } = useSecurityPolicies({ value: 'env-prod' })
    applyEnvDefaults('env-prod')
    expect(polReadonly.value).toBe(true)
    expect(polWriteConfirm.value).toBe(true)
    expect(polDdlConfirm.value).toBe(true)
    expect(polDrop.value).toBe('disable')
    expect(polAutocommit.value).toBe(false)
    expect(polRowLimit.value).toBe(1000)
    expect(polSizeLimit.value).toBe(20)
  })

  it('env-sandbox → 读写、行限1000', () => {
    const {
      applyEnvDefaults,
      polReadonly,
      polWriteConfirm,
      polDdlConfirm,
      polRowLimit,
      polSizeLimit,
    } = useSecurityPolicies({ value: 'env-sandbox' })
    applyEnvDefaults('env-sandbox')
    expect(polReadonly.value).toBe(false)
    expect(polWriteConfirm.value).toBe(false)
    expect(polDdlConfirm.value).toBe(false)
    expect(polRowLimit.value).toBe(1000)
    expect(polSizeLimit.value).toBe(50)
  })

  it('未知环境 → 回退 env-dev', () => {
    const { applyEnvDefaults, polReadonly, polAutocommit } = useSecurityPolicies({
      value: 'unknown',
    })
    applyEnvDefaults('unknown')
    expect(polReadonly.value).toBe(false)
    expect(polAutocommit.value).toBe(true)
  })
})

// ==================== collectPolicyConfig / applyPolicyConfig ====================

describe('collectPolicyConfig / applyPolicyConfig 策略快照与恢复', () => {
  it('collect → 快照包含所有策略字段', () => {
    const { applyEnvDefaults, collectPolicyConfig } = useSecurityPolicies({ value: 'env-prod' })
    applyEnvDefaults('env-prod')
    const config = collectPolicyConfig()
    expect(config).toEqual({
      ro: true,
      wc: true,
      ddl: true,
      drop: 'disable',
      ac: false,
      rl: 1000,
      sl: 20,
    })
  })

  it('apply → 从快照恢复', () => {
    const { polReadonly, polDrop, polRowLimit, applyPolicyConfig } = useSecurityPolicies({
      value: 'env-dev',
    })
    applyPolicyConfig({
      ro: true,
      drop: 'disable',
      rl: 500,
    })
    expect(polReadonly.value).toBe(true)
    expect(polDrop.value).toBe('disable')
    expect(polRowLimit.value).toBe(500)
  })

  it('apply → 部分快照，只更新指定字段', () => {
    const { polDdlConfirm, polAutocommit, applyPolicyConfig } = useSecurityPolicies({
      value: 'env-dev',
    })
    // 先设置初始值
    polDdlConfirm.value = false
    polAutocommit.value = true
    // 只更新 ddl
    applyPolicyConfig({ ddl: true })
    expect(polDdlConfirm.value).toBe(true)
    expect(polAutocommit.value).toBe(true) // 未变
  })

  it('apply → 空对象不改变任何值', () => {
    const { applyEnvDefaults, polReadonly, polDrop, applyPolicyConfig } = useSecurityPolicies({
      value: 'env-dev',
    })
    applyEnvDefaults('env-dev')
    applyPolicyConfig({})
    expect(polReadonly.value).toBe(false)
    expect(polDrop.value).toBe('false')
  })
})

// ==================== tempDefaultLocked ====================

describe('tempDefaultLocked 默认值写入锁', () => {
  it('applyEnvDefaults → 短暂锁定', () => {
    const { applyEnvDefaults, tempDefaultLocked } = useSecurityPolicies({ value: 'env-dev' })
    applyEnvDefaults('env-dev')
    // 同步调用后立即为 true
    expect(tempDefaultLocked.value).toBe(true)
  })
})

// ==================== checkPolicyOverride ====================

describe('checkPolicyOverride 空操作', () => {
  it('调用不抛出异常', () => {
    const { checkPolicyOverride } = useSecurityPolicies({ value: 'env-dev' })
    expect(() => checkPolicyOverride()).not.toThrow()
  })
})
