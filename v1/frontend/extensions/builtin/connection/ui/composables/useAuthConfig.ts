/**
 * useAuthConfig — 认证配置管理 Composable
 *
 * 从 GeneralTab.vue 提取，处理：
 * - 认证方式切换（password / mTLS / Kerberos / OAuth2）
 * - 已保存认证配置的选择与预填
 * - 认证配置管理器（AuthConfigManager Modal）集成
 *
 * 同时提供规范的 AuthConfig / BackendAuthConfig 类型定义，
 * 替代 NetworkTab.vue、AuthConfigManager.vue 中的内联定义。
 */
import { invoke } from '@tauri-apps/api/core'
import { ref, computed, nextTick } from 'vue'

// ==================== Types (canonical) ====================

/** 认证配置数据模型 */
export interface AuthConfig {
  id: string
  name: string
  authType: string
  scope: 'global' | 'project'
  username?: string
  password?: string
  certPath?: string
  certKeyPath?: string
  principal?: string
  keytabPath?: string
  keyPath?: string
  passphrase?: string
  tokenEndpoint?: string
  clientId?: string
  clientSecret?: string
  createdAt?: string
}

/** 后端原始响应格式（snake_case + auth_data JSON） */
export interface BackendAuthConfig {
  id: string
  name: string | null
  auth_type: string
  auth_data: string
  origin: string | null
  source_id?: string | null
  snapshot_at?: string | null
  created_at: string
  updated_at: string
}

/** 将后端原始配置解析为前端 AuthConfig */
export function parseAuthConfig(raw: BackendAuthConfig): AuthConfig {
  let data: Record<string, unknown> = {}
  try {
    data = JSON.parse(raw.auth_data || '{}')
  } catch (err) {
    console.warn('[parseAuthConfig] 解析失败:', err)
  }
  return {
    id: raw.id,
    name: raw.name || '',
    authType: raw.auth_type,
    scope: raw.origin === 'global' ? 'global' : 'project',
    username: data.username as string | undefined,
    password: data.password as string | undefined,
    certPath: data.certPath as string | undefined,
    certKeyPath: data.certKeyPath as string | undefined,
    principal: data.principal as string | undefined,
    keytabPath: data.keytabPath as string | undefined,
    keyPath: data.keyPath as string | undefined,
    passphrase: data.passphrase as string | undefined,
    tokenEndpoint: data.tokenEndpoint as string | undefined,
    clientId: data.clientId as string | undefined,
    clientSecret: data.clientSecret as string | undefined,
    createdAt: raw.created_at,
  }
}

// ==================== Constants ====================

/** 认证方式 → 人性化标签（7 大抽象类别） */
const AUTH_LABELS: Record<string, string> = {
  password: '用户名/密码',
  kerberos: 'Kerberos',
  oauth2: 'OAuth 2.0',
  ldap: 'LDAP / AD',
  pg_class: 'TLS 客户端证书',
  os_auth: '操作系统认证',
  trust: '无认证',
}

/** 所有认证方式（标签 + 值） */
const ALL_AUTH_OPTS = Object.entries(AUTH_LABELS).map(([value, label]) => ({ label, value }))

/** 解析驱动 supported_auth_types JSON 字符串 → 认证方式数组 */
export function parseSupportedAuthTypes(raw?: string): string[] {
  if (!raw) return []
  try {
    const arr = JSON.parse(raw)
    if (Array.isArray(arr)) return arr.filter((v): v is string => typeof v === 'string')
    return []
  } catch {
    return []
  }
}

// ==================== Composable ====================

export interface AuthFormFields {
  username: string
  password: string
  certPath: string
  certKeyPath: string
  principal: string
  keytabPath: string
  tokenEndpoint: string
  clientId: string
  clientSecret: string
}

export interface UseAuthConfigOptions {
  /** 表单数据（会由 composable 修改预填字段） */
  local: AuthFormFields
  /** 表单更新回调 */
  onFormUpdate: () => void
  /** 认证配置变更回调 */
  onAuthConfigChange: (configId: string | null, authMethod: string) => void
  /** 驱动支持的认证方式列表（如 ["password","kerberos"]），不传则显示全部 */
  supportedAuthTypes?: string[]
}

export function useAuthConfig(opts: UseAuthConfigOptions) {
  const { local, onFormUpdate, onAuthConfigChange } = opts

  // 驱动支持的认证方式（可在运行时更新）
  const _supportedAuthTypes = ref<string[]>(opts.supportedAuthTypes ?? [])

  // ===== State =====
  const authMethod = ref('password')
  const selectedAuthConfigId = ref<string | null>(null)
  const showAuthManager = ref(false)
  const authConfigs = ref<AuthConfig[]>([])

  // ===== Computed =====

  const authMethodOpts = computed(() => {
    const supported = _supportedAuthTypes.value
    if (supported.length === 0) return ALL_AUTH_OPTS
    return ALL_AUTH_OPTS.filter(m => supported.includes(m.value))
  })

  /** 按当前认证方式过滤已保存的认证配置 */
  const filteredAuthConfigOpts = computed(() => {
    const configs = authConfigs.value.filter(ac => ac.authType === authMethod.value)
    if (configs.length === 0) return []
    return [
      { label: '— 手动填写 —', value: '' },
      ...configs.map(ac => ({
        label: `${ac.name} · ${ac.scope === 'global' ? '🌐' : '📝'}`,
        value: ac.id,
      })),
    ]
  })

  // 标记认证方式切换中，防止 onAuthConfigSelect 误清空表单字段
  const isAuthMethodChanging = ref(false)

  // ===== Methods =====

  /** 切换认证方式时清空已选配置，并确保当前方式在驱动支持列表内 */
  function onAuthMethodChange() {
    isAuthMethodChanging.value = true
    selectedAuthConfigId.value = null
    // 在 nextTick 后重置标志，确保 v-model 触发的 onAuthConfigSelect 已执行完毕
    nextTick(() => {
      isAuthMethodChanging.value = false
    })
    onFormUpdate()
  }

  /** 选择已保存的认证配置 → 预填表单字段 */
  function onAuthConfigSelect(configId: string | null) {
    // 认证方式切换时跳过清空，保留已填写的 username 等字段
    if (isAuthMethodChanging.value) return
    if (!configId) {
      selectedAuthConfigId.value = null
      local.username = ''
      local.password = ''
      local.certPath = ''
      local.certKeyPath = ''
      local.principal = ''
      local.keytabPath = ''
      local.tokenEndpoint = ''
      local.clientId = ''
      local.clientSecret = ''
      onFormUpdate()
      return
    }

    const config = authConfigs.value.find(ac => ac.id === configId)
    if (!config) return

    selectedAuthConfigId.value = configId
    authMethod.value = config.authType
    if (config.username) local.username = config.username
    if (config.password) local.password = config.password
    if (config.certPath) local.certPath = config.certPath
    if (config.certKeyPath) local.certKeyPath = config.certKeyPath
    if (config.principal) local.principal = config.principal
    if (config.keytabPath) local.keytabPath = config.keytabPath
    if (config.tokenEndpoint) local.tokenEndpoint = config.tokenEndpoint
    if (config.clientId) local.clientId = config.clientId
    if (config.clientSecret) local.clientSecret = config.clientSecret

    onAuthConfigChange(configId, config.authType)
    onFormUpdate()
  }

  /** AuthConfigManager select 事件处理 */
  function onAuthConfigExternalSelect(configId: string) {
    showAuthManager.value = false
    onAuthConfigSelect(configId)
  }

  /** AuthConfigManager 关闭后刷新配置列表 */
  async function onAuthManagerClose() {
    showAuthManager.value = false
    await loadAuthConfigs()
  }

  /** 驱动切换时更新支持的认证方式，当前方式不兼容则重置为首个 */
  function updateSupportedAuthTypes(types: string[]) {
    _supportedAuthTypes.value = types
    if (types.length > 0 && !types.includes(authMethod.value)) {
      authMethod.value = types[0]
      selectedAuthConfigId.value = null
    }
  }

  /** 从后端加载已保存的认证配置列表 */
  async function loadAuthConfigs(projectPath?: string) {
    try {
      const configs = await invoke<BackendAuthConfig[]>('list_auth_configs')
      const globalConfigs = configs.map(parseAuthConfig)

      let projectConfigs: AuthConfig[] = []
      if (projectPath) {
        const projConfigs = await invoke<BackendAuthConfig[]>('project_list_auth_configs', {
          projectPath,
        })
        projectConfigs = projConfigs.map(parseAuthConfig)
      }

      // 按 ID 去重：项目配置优先（覆写同名全局配置），全局配置补充
      const seenIds = new Set<string>()
      const deduped: AuthConfig[] = []

      // 先添加项目配置（优先级更高）
      for (const c of projectConfigs) {
        seenIds.add(c.id)
        deduped.push(c)
      }
      // 再添加全局配置（仅当 ID 不在项目配置中）
      for (const c of globalConfigs) {
        if (!seenIds.has(c.id)) {
          deduped.push(c)
        }
      }

      authConfigs.value = deduped
    } catch (err) {
      console.warn('[useAuthConfig] loadAuthConfigs 失败:', err)
    }
  }

  return {
    // State
    authMethod,
    selectedAuthConfigId,
    showAuthManager,
    authConfigs,
    // Computed
    authMethodOpts,
    filteredAuthConfigOpts,
    // Methods
    onAuthMethodChange,
    onAuthConfigSelect,
    onAuthConfigExternalSelect,
    onAuthManagerClose,
    loadAuthConfigs,
    updateSupportedAuthTypes,
  }
}
