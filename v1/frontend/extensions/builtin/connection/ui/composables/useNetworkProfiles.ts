/**
 * useNetworkProfiles — 网络配置文件列表 Composable
 *
 * 核心职责：从后端获取 SSH/SSL/Proxy 三类网络配置，解析 config JSON 并生成可读摘要。
 * 支持 scope=project 时自动切换到 project_* 命令族操作 project.db。
 */
import { invoke } from '@tauri-apps/api/core'
import { ref, computed, readonly } from 'vue'

// ==================== 类型 ====================

export interface NetworkProfile {
  id: string
  name: string
  type: 'ssh' | 'ssl' | 'proxy'
  config: unknown
  detail: string
  origin?: string
}

interface ConfigRaw {
  id: string
  name?: string
  network_type: string
  config: string
  auth_config_id?: string
  origin?: string
  created_at: string
  updated_at: string
}

// ==================== 类型守卫 ====================

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null
}

function isSshConfig(v: unknown): v is { host?: string; port?: number } {
  return isRecord(v) && !('mode' in v) && !('type' in v)
}

function isSslConfig(v: unknown): v is { mode?: string } {
  return isRecord(v) && 'mode' in v && !('type' in v)
}

function isProxyConfig(v: unknown): v is { host?: string; port?: number } {
  return isRecord(v) && 'type' in v && !('mode' in v)
}

// ==================== 状态（全局/项目双数组分离） ====================

const globalSshProfiles = ref<NetworkProfile[]>([])
const globalSslProfiles = ref<NetworkProfile[]>([])
const globalProxyProfiles = ref<NetworkProfile[]>([])
const projectSshProfiles = ref<NetworkProfile[]>([])
const projectSslProfiles = ref<NetworkProfile[]>([])
const projectProxyProfiles = ref<NetworkProfile[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// 对外暴露全局+项目合并后的计算列表（项目配置优先，按 ID 去重）
const sshProfiles = computed(() => {
  const seen = new Set(projectSshProfiles.value.map(p => p.id))
  return [...projectSshProfiles.value, ...globalSshProfiles.value.filter(p => !seen.has(p.id))]
})
const sslProfiles = computed(() => {
  const seen = new Set(projectSslProfiles.value.map(p => p.id))
  return [...projectSslProfiles.value, ...globalSslProfiles.value.filter(p => !seen.has(p.id))]
})
const proxyProfiles = computed(() => {
  const seen = new Set(projectProxyProfiles.value.map(p => p.id))
  return [...projectProxyProfiles.value, ...globalProxyProfiles.value.filter(p => !seen.has(p.id))]
})

// ==================== 工具函数 ====================

export function parseConfig<T>(configJson: string): T | null {
  try {
    return JSON.parse(configJson) as T
  } catch (err) {
    console.warn('[parseConfig] 解析失败:', err)
    return null
  }
}

function buildDetail(type: string, cfg: unknown): string {
  if (isSslConfig(cfg)) return cfg.mode ?? 'verify-full'
  if (isSshConfig(cfg)) return `${cfg.host ?? 'localhost'}:${cfg.port ?? 22}`
  if (isProxyConfig(cfg)) return `${cfg.host ?? 'proxy.corp.com'}:${cfg.port ?? 1080}`
  return type === 'ssh' ? 'localhost:22' : type === 'ssl' ? 'verify-full' : 'proxy.corp.com:1080'
}

function toProfile(raw: ConfigRaw): NetworkProfile | null {
  const config = parseConfig<unknown>(raw.config)
  if (config === null) return null
  const type = raw.network_type as NetworkProfile['type']
  return {
    id: raw.id,
    name: raw.name ?? raw.id,
    type,
    config,
    detail: buildDetail(type, config),
    origin: raw.origin,
  }
}

// ==================== API ====================

/** 命令选择：scope=project 时使用 project_* 命令族 */
async function getProjectPath(): Promise<string | null> {
  const { useProjectStore } = await import('@/core/project/stores/project')
  return useProjectStore().currentProject?.path ?? null
}

async function loadByType(type: 'ssh' | 'ssl' | 'proxy'): Promise<void> {
  try {
    const raws = await invoke<ConfigRaw[]>('list_network_configs', { networkType: type })
    const profiles = raws.map(toProfile).filter((p): p is NetworkProfile => p !== null)
    if (type === 'ssh') globalSshProfiles.value = profiles
    else if (type === 'ssl') globalSslProfiles.value = profiles
    else globalProxyProfiles.value = profiles
  } catch (e) {
    console.error(`[useNetworkProfiles] Failed to load ${type}:`, e)
    error.value = e instanceof Error ? e.message : String(e)
  }
}

async function loadAll(): Promise<void> {
  loading.value = true
  error.value = null
  try {
    await Promise.all([loadByType('ssh'), loadByType('ssl'), loadByType('proxy')])
  } finally {
    loading.value = false
  }
}

// ==================== 项目级命令 ====================

async function loadByTypeProject(
  type: 'ssh' | 'ssl' | 'proxy',
  projectPath: string
): Promise<void> {
  try {
    const raws = await invoke<ConfigRaw[]>('project_list_network_configs', {
      networkType: type,
      projectPath,
    })
    const profiles = raws.map(toProfile).filter((p): p is NetworkProfile => p !== null)
    // 替换项目级配置（不再追加，避免重复；全局与项目由 computed 合并）
    if (type === 'ssh') projectSshProfiles.value = profiles
    else if (type === 'ssl') projectSslProfiles.value = profiles
    else projectProxyProfiles.value = profiles
  } catch (e) {
    console.error(`[useNetworkProfiles] Failed to load project ${type}:`, e)
    error.value = e instanceof Error ? e.message : String(e)
  }
}

async function loadAllProject(projectPath: string): Promise<void> {
  loading.value = true
  error.value = null
  try {
    await Promise.all([
      loadByTypeProject('ssh', projectPath),
      loadByTypeProject('ssl', projectPath),
      loadByTypeProject('proxy', projectPath),
    ])
  } finally {
    loading.value = false
  }
}

/** 创建/更新网络配置 */
async function saveProjectProfile(
  profile: Record<string, unknown>,
  networkType: string,
  configObj: Record<string, unknown>,
  projectPath: string
): Promise<void> {
  const name = (profile.name as string) || `未命名-${networkType.toUpperCase()}`
  const config = JSON.stringify(configObj)
  if (profile.id) {
    await invoke('project_update_network_config', { id: profile.id, name, config, projectPath })
  } else {
    await invoke('project_create_network_config', { name, networkType, config, projectPath })
  }
}

/** 删除网络配置 */
async function removeProjectProfile(id: string, projectPath: string): Promise<void> {
  await invoke('project_delete_network_config', { id, projectPath })
}

// ==================== Composable ====================

export function useNetworkProfiles() {
  return {
    sshProfiles,
    sslProfiles,
    proxyProfiles,
    loading: readonly(loading),
    error: readonly(error),
    loadAll,
    loadByType,
    loadAllProject,
    saveProjectProfile,
    removeProjectProfile,
    parseConfig,
    getProjectPath,
  }
}
