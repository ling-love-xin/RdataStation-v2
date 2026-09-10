/**
 * useSecurityPolicies — 安全策略状态管理 Composable
 *
 * 管理 Class 安全策略：
 * - 读写/只读模式
 * - 写确认 / DDL确认
 * - DROP 策略（允许/确认/禁用）
 * - 自动提交
 * - 行限制 / 大小限制
 * - 策略覆盖检测
 */
import { ref, computed } from 'vue'

import { envDefs, type EnvDefPolicy } from '../constants/envDefaults'

/** DROP 选项（硬编码中文，与 i18n fallback 一致） */
const dropOpts: { label: string; value: string }[] = [
  { label: '允许', value: 'false' },
  { label: '确认', value: 'true' },
  { label: '禁用', value: 'disable' },
]

export function useSecurityPolicies(
  envId: ReturnType<typeof ref<string | null>> | { value: string | null }
) {
  // ===== 策略状态 =====
  const polReadonly = ref(false)
  const polWriteConfirm = ref(false)
  const polDdlConfirm = ref(false)
  const polAutocommit = ref(true)
  const polDrop = ref('false')
  const polRowLimit = ref(0)
  const polSizeLimit = ref(0)
  /** 标记为环境默认值正在写入（防止 watcher 误判为覆盖） */
  const tempDefaultLocked = ref(false)

  // ===== 计算属性 =====

  const securitySummary = computed(() => {
    const parts: string[] = []
    if (polReadonly.value) parts.push('只读')
    else parts.push('读写')
    if (polWriteConfirm.value) parts.push('写确认')
    if (polDdlConfirm.value) parts.push('DDL确认')
    if (polDrop.value === 'disable') parts.push('DROP禁用')
    else if (polDrop.value === 'true') parts.push('DROP确认')
    if (polRowLimit.value > 0) parts.push(`行限${polRowLimit.value}`)
    if (polSizeLimit.value > 0) parts.push(`限${polSizeLimit.value}M`)
    return parts.join('·') || '默认'
  })

  const isPolicyOverridden = computed(() => {
    const p: EnvDefPolicy | null = envDefs.find(e => e.id === envId.value)?.policy ?? null
    if (!p) return false
    if (polReadonly.value !== p.ro) return true
    if (polWriteConfirm.value !== p.wc) return true
    if (polDdlConfirm.value !== p.ddl) return true
    if (polDrop.value !== p.drop) return true
    if (polAutocommit.value !== p.ac) return true
    if (polRowLimit.value !== p.rl) return true
    if (polSizeLimit.value !== p.sl) return true
    return false
  })

  // ===== 方法 =====

  /** 应用环境默认策略值 */
  function applyEnvDefaults(envIdVal: string) {
    const defaults: Record<string, Record<string, unknown>> = {
      'env-dev': { ro: false, wc: false, ddl: false, drop: 'false', ac: true, rl: 0, sl: 0 },
      'env-test': { ro: false, wc: false, ddl: true, drop: 'true', ac: true, rl: 10000, sl: 100 },
      'env-staging': { ro: false, wc: true, ddl: true, drop: 'true', ac: false, rl: 5000, sl: 50 },
      'env-prod': { ro: true, wc: true, ddl: true, drop: 'disable', ac: false, rl: 1000, sl: 20 },
      'env-sandbox': {
        ro: false,
        wc: false,
        ddl: false,
        drop: 'false',
        ac: true,
        rl: 1000,
        sl: 50,
      },
    }
    const d = defaults[envIdVal] ?? defaults['env-dev']
    tempDefaultLocked.value = true
    polReadonly.value = d.ro as boolean
    polWriteConfirm.value = d.wc as boolean
    polDdlConfirm.value = d.ddl as boolean
    polAutocommit.value = d.ac as boolean
    polDrop.value = d.drop as string
    polRowLimit.value = d.rl as number
    polSizeLimit.value = d.sl as number
    setTimeout(() => {
      tempDefaultLocked.value = false
    }, 0)
  }

  /** 导出策略快照 */
  function collectPolicyConfig(): Record<string, unknown> {
    return {
      ro: polReadonly.value,
      wc: polWriteConfirm.value,
      ddl: polDdlConfirm.value,
      drop: polDrop.value,
      ac: polAutocommit.value,
      rl: polRowLimit.value,
      sl: polSizeLimit.value,
    }
  }

  /** 从持久化配置恢复 */
  function applyPolicyConfig(config: Record<string, unknown>) {
    if ('ro' in config) polReadonly.value = config.ro as boolean
    if ('wc' in config) polWriteConfirm.value = config.wc as boolean
    if ('ddl' in config) polDdlConfirm.value = config.ddl as boolean
    if ('drop' in config) polDrop.value = config.drop as string
    if ('ac' in config) polAutocommit.value = config.ac as boolean
    if ('rl' in config) polRowLimit.value = config.rl as number
    if ('sl' in config) polSizeLimit.value = config.sl as number
  }

  /** 空操作——computed 自动更新覆盖状态 */
  function checkPolicyOverride() {
    /* no-op */
  }

  return {
    polReadonly,
    polWriteConfirm,
    polDdlConfirm,
    polAutocommit,
    polDrop,
    polRowLimit,
    polSizeLimit,
    tempDefaultLocked,
    dropOpts,
    securitySummary,
    isPolicyOverridden,
    applyEnvDefaults,
    collectPolicyConfig,
    applyPolicyConfig,
    checkPolicyOverride,
  }
}
