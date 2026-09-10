/**
 * useAddDataSource — 新增数据源提交逻辑封装
 *
 * 统一管理新增/编辑数据源的状态、校验、快照、协议链与提交载荷构建。
 * 对应文档: docs/frontend/connection/add-datasource-frontend-plan.md (Phase 4-5)
 */

import { invoke } from '@tauri-apps/api/core'
import { useMessage } from 'naive-ui'
import { v4 as uuidv4 } from 'uuid'
import { reactive, ref, watch, onMounted } from 'vue'

import { useProjectStore } from '@/core/project/stores/project'

import { useEnvironmentStore } from '../stores/environmentStore'

// ==================== 类型定义 ====================

/** 连接作用域 */
export interface ConnectionScope {
  global: boolean
  project: boolean
}

/** 连接表单数据 */
export interface ConnectionFormData {
  host?: string
  port?: number
  database?: string
  username?: string
  password?: string
  filePath?: string
  url?: string
  [key: string]: unknown
}

/** 暂存项 */
export interface StagingItem {
  id: string
  name: string
  /** 数据库类型名称 (e.g. "mysql", "postgres")，注意与 driverId（具体驱动实例ID）区分 */
  driver?: string
  /** 具体驱动实例 ID (e.g. "mysql_local_01") */
  driverId?: string
  url?: string
  formData?: ConnectionFormData
  authConfigId?: string | null
  authMethod?: string
  networkConfigId?: string | null
  driverProperties?: string | null
  advancedOptions?: string | null
  environmentId?: string | null
  scope?: 'global' | 'project' | 'both'
  description?: string
  schemaName?: string
  options?: string
  metadataPath?: string
  tags?: string
  useDuckdbFed?: boolean
  applied?: boolean
}

/** localStorage 存储键 */
const STAGING_STORAGE_KEY = 'rdata-station-staging-items'

/** 安全策略 */
export interface SecurityPolicy {
  readonly: boolean
  writeConfirm: boolean
  ddlConfirm: boolean
  dropConfirm: 'disable' | 'confirm' | 'allow'
  autocommit: boolean
  rowLimit: number
  sizeLimit: number
}

/** Schema 策略 */
export interface SchemaPolicy {
  autoLoad: boolean
  loadDepth: number
  showSystem: boolean
  refreshInterval: number
}

/** 性能策略 */
export interface PerformancePolicy {
  poolSize: number
  queryTimeout: number
  connectTimeout: number
  heartbeat: number
  maxReconnect: number
}

/** 审计策略 */
export interface AuditPolicy {
  sqlLog: boolean
  operationRecord: boolean
  sensitiveTableAlert: boolean
}

/** UI 策略 */
export interface UiPolicy {
  topBarColor: string
  tabIndicator: boolean
  sqlWarningBanner: boolean
  writeBtnStyle: 'normal' | 'confirm' | 'danger'
}

/** 环境策略集合 */
export interface EnvironmentPolicies {
  security: SecurityPolicy
  schema: SchemaPolicy
  performance: PerformancePolicy
  audit: AuditPolicy
  ui: UiPolicy
}

/** DuckDB 加速配置 */
export interface DuckdbAccelConfig {
  enabled: boolean
  syncStrategy: 'schedule' | 'manual' | 'on_change'
  syncIntervalMin: number
  memoryLimitMB: number
  threads: number
  localPath: string
}

/** 校验结果 */
export interface ValidationResult {
  valid: boolean
  errors: Record<string, string>
}

// ==================== 辅助函数 ====================

function defaultDuckdbAccel(): DuckdbAccelConfig {
  return {
    enabled: false,
    syncStrategy: 'schedule',
    syncIntervalMin: 60,
    memoryLimitMB: 512,
    threads: 2,
    localPath: '',
  }
}

// ==================== Composable ====================

export function useAddDataSource() {
  const message = useMessage()

  // ========== 状态 ==========
  const headerData = reactive({
    name: '',
    description: '',
    selectedDriverId: '',
  })

  const scope = reactive<ConnectionScope>({ global: true, project: false })

  const selectedEnvId = ref<string | null>(null)
  const overriddenPolicies = ref<Partial<EnvironmentPolicies>>({})
  const duckdbAccel = reactive<DuckdbAccelConfig>(defaultDuckdbAccel())
  const driverProps = ref<Record<string, string>>({})
  const formData = ref<ConnectionFormData>({})

  // Auth / Network / Extra — owned by composable, synced by dialog
  const authConfigId = ref<string | null>(null)
  const authMethod = ref<string>('password')
  const networkConfigId = ref<string | null>(null)
  const schemaName = ref<string | null>(null)
  const options = ref<string | null>(null)
  const metadataPath = ref<string | null>(null)
  const tags = ref<string | null>(null)
  const useDuckdbFed = ref<boolean | null>(null)

  const saving = ref(false)
  const error = ref<string | null>(null)

  // ========== 计算属性 ==========
  /** 是否为文件型数据库（SQLite/DuckDB），由调用方在 onDriverChange 时设置 */
  const isFileDb = ref(false)

  function setFileDb(val: boolean) {
    isFileDb.value = val
  }

  // ========== 环境联动 ==========
  async function selectEnv(envId: string) {
    const envStore = useEnvironmentStore()
    const env = envStore.getById(envId)
    if (!env) return

    selectedEnvId.value = envId
    overriddenPolicies.value = {}

    // 引用全局环境时触发快照
    if (envId.startsWith('G_')) {
      try {
        const pp = useProjectStore().currentProject?.path
        const r = await invoke<{ snapshot_id: string }>('snapshot_global_env', {
          globalEnvId: envId,
          projectPath: pp,
        })
        selectedEnvId.value = r.snapshot_id
      } catch (e) {
        error.value = `快照环境失败: ${e instanceof Error ? e.message : String(e)}`
      }
    }
  }

  function onPolicyOverride(path: string, value: unknown) {
    const keys = path.split('.')
    let obj: Record<string, unknown> = overriddenPolicies.value as unknown as Record<
      string,
      unknown
    >
    for (let i = 0; i < keys.length - 1; i++) {
      if (!obj[keys[i]] || typeof obj[keys[i]] !== 'object') {
        obj[keys[i]] = {} as Record<string, unknown>
      }
      obj = obj[keys[i]] as Record<string, unknown>
    }
    obj[keys[keys.length - 1]] = value
  }

  // ========== 校验 ==========
  /** 验证结果类型 */
  interface ExtendedValidationResult extends ValidationResult {
    warnings?: string[]
  }

  /** 扩展验证（包含警告） */
  function validateExtended(): ExtendedValidationResult {
    const errs: Record<string, string> = {}
    const warnings: string[] = []

    if (!headerData.name.trim()) errs.name = '请输入数据源名称'
    if (!headerData.selectedDriverId) errs.driver = '请选择数据驱动'
    if (!scope.global && !scope.project) errs.scope = '请至少选择一个作用域'

    if (scope.project && !useProjectStore().currentProject) {
      errs.project = '请先打开一个项目'
    }

    return { valid: Object.keys(errs).length === 0, errors: errs, warnings }
  }

  /** 基础验证 */
  function validate(): ValidationResult {
    const result = validateExtended()
    return { valid: result.valid, errors: result.errors }
  }

  // ========== StagingItem 管理 ==========
  const stagingItems = ref<StagingItem[]>([{ id: '', name: '' }])
  const stagingIndex = ref(0)

  /**
   * 构建 StagingItem（统一字段处理）
   */
  function buildStagingItem(
    name: string,
    driver: string | undefined,
    driverId: string | undefined,
    url: string,
    formData: ConnectionFormData,
    authConfigId: string | null,
    authMethod: string,
    networkConfigId: string | null,
    driverProperties: string | null,
    advancedOptions: string | null,
    environmentId: string | null,
    description: string | undefined,
    schemaName: string | undefined,
    options: string | undefined,
    metadataPath: string | undefined,
    tags: string | undefined,
    useDuckdbFed: boolean
  ): StagingItem {
    const safeFormData = { ...formData }
    // 清除所有敏感字段，确保暂存项不包含凭据
    delete safeFormData.password
    delete safeFormData.token
    delete safeFormData.apiKey
    delete safeFormData.secretKey
    delete safeFormData.privateKey
    delete safeFormData.certPath
    delete safeFormData.keyPath
    return {
      id: uuidv4(),
      name,
      driver,
      driverId,
      url,
      formData: safeFormData,
      authConfigId,
      authMethod,
      networkConfigId,
      driverProperties,
      advancedOptions,
      environmentId,
      scope: scope.global && scope.project ? 'both' : scope.global ? 'global' : 'project',
      description,
      schemaName,
      options,
      metadataPath,
      tags,
      useDuckdbFed,
      applied: false,
    }
  }

  /**
   * 加载持久化的暂存项
   */
  function loadStagingItems() {
    try {
      const stored = localStorage.getItem(STAGING_STORAGE_KEY)
      if (stored) {
        const parsed = JSON.parse(stored)
        if (Array.isArray(parsed) && parsed.length > 0) {
          stagingItems.value = parsed
        }
      }
    } catch (e) {
      console.error('[Staging] 加载暂存项失败:', e)
    }
  }

  /**
   * 保存暂存项到 localStorage
   */
  function saveStagingItems() {
    try {
      localStorage.setItem(STAGING_STORAGE_KEY, JSON.stringify(stagingItems.value))
    } catch (e) {
      console.error('[Staging] 保存暂存项失败:', e)
      message?.warning?.('暂存保存失败，请检查磁盘空间')
    }
  }

  /**
   * 清空持久化的暂存项
   */
  function clearStagingItems() {
    try {
      localStorage.removeItem(STAGING_STORAGE_KEY)
      stagingItems.value = [{ id: '', name: '' }]
      stagingIndex.value = 0
    } catch (e) {
      console.error('[Staging] 清空暂存项失败:', e)
    }
  }

  /**
   * 添加暂存项（同时重置 auth/network/extra 状态）
   */
  function addStaging() {
    const current = stagingItems.value[stagingIndex.value]
    if (current && !current.name && !current.applied) {
      // 当前项是空项 → 清空状态，不追加
      stagingItems.value[stagingIndex.value] = { id: current.id, name: '', applied: false }
    } else {
      stagingItems.value.push({ id: uuidv4(), name: '', applied: false })
      stagingIndex.value = stagingItems.value.length - 1
    }
    authConfigId.value = null
    authMethod.value = 'password'
    networkConfigId.value = null
    schemaName.value = null
    options.value = null
    metadataPath.value = null
    tags.value = null
    useDuckdbFed.value = null
    formData.value = {}
    selectedEnvId.value = null
    driverProps.value = {}
  }

  /**
   * 标记暂存项为已应用
   */
  function markStagingApplied(index: number) {
    if (stagingItems.value[index]) {
      stagingItems.value[index].applied = true
    }
  }

  /**
   * 移除暂存项
   */
  function removeStaging(index: number) {
    if (stagingItems.value.length <= 1) return
    stagingItems.value.splice(index, 1)
    if (stagingIndex.value >= stagingItems.value.length) {
      stagingIndex.value = stagingItems.value.length - 1
    }
  }

  /**
   * 选择暂存项
   */
  function selectStaging(index: number) {
    stagingIndex.value = index
  }

  // 初始化时加载暂存项
  onMounted(() => {
    loadStagingItems()
    if (stagingItems.value.length > 0 && stagingItems.value[stagingItems.value.length - 1].name) {
      stagingIndex.value = stagingItems.value.length - 1
    }
  })

  // 监听暂存项变化，自动持久化
  watch(stagingItems, saveStagingItems, { deep: true })

  return {
    // 状态
    headerData,
    scope,
    selectedEnvId,
    overriddenPolicies,
    duckdbAccel,
    driverProps,
    formData,
    authConfigId,
    authMethod,
    networkConfigId,
    schemaName,
    options,
    metadataPath,
    tags,
    useDuckdbFed,
    saving,
    error,
    // 暂存项管理
    stagingItems,
    stagingIndex,
    loadStagingItems,
    buildStagingItem,
    addStaging,
    removeStaging,
    selectStaging,
    clearStagingItems,
    markStagingApplied,
    // 计算
    isFileDb,
    setFileDb,
    // 环境
    selectEnv,
    onPolicyOverride,
    // 校验
    validate,
    validateExtended,
  }
}
