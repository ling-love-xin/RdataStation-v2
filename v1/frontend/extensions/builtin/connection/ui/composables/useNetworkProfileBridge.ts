/**
 * useNetworkProfileBridge — 网络配置文件 CRUD 桥接
 *
 * 从 NetworkTab.vue 提取，统一处理 SSH/SSL/Proxy 配置文件的
 * 创建（global 调用 invoke → backend API，project 调用 saveProjectProfile）
 * 和删除（global 调用 invoke → delete_network_config，project 调用 removeProjectProfile）
 *
 * 消除 NetworkTab 中 3 对 create/delete 的重复模式。
 */
import { invoke } from '@tauri-apps/api/core'

// ==================== Types ====================

export interface NetworkBridgeDeps {
  /** 是否项目级作用域 */
  isProject: boolean
  /** 获取项目路径 */
  getProjectPath: () => Promise<string | null>
  /** 项目级保存 */
  saveProjectProfile: (
    profile: Record<string, unknown>,
    type: string,
    config: Record<string, unknown>,
    path: string
  ) => Promise<void>
  /** 项目级重新加载 */
  loadAllProject: (path: string) => Promise<void>
  /** 全局级重新加载 */
  loadAll: () => Promise<void>
  /** 项目级删除 */
  removeProjectProfile: (id: string, path: string) => Promise<void>
}

export interface NetworkProfileCreator {
  (profile: Record<string, unknown>): Promise<void>
}

export interface NetworkProfileDeleter {
  (id: string): Promise<void>
}

// ==================== Protocol config mappers ====================

type ConfigMapper = (profile: Record<string, unknown>) => Record<string, unknown>

const configMappers: Record<string, ConfigMapper> = {
  ssh: p => ({
    host: p.host,
    port: p.port,
    username: p.username,
    authMethod: p.authMethod,
    password: p.password,
    keyPath: p.keyPath,
    passphrase: p.passphrase,
    keepalive: p.keepalive,
    localPort: p.localPort,
    remoteHost: p.remoteHost,
    remotePort: p.remotePort,
  }),
  ssl: p => ({
    mode: p.mode,
    ca: p.ca,
    clientCert: p.clientCert,
    clientKey: p.clientKey,
    hostnameOverride: p.hostnameOverride,
  }),
  proxy: p => ({
    type: p.type,
    host: p.host,
    port: p.port,
    username: p.username,
    password: p.password,
  }),
}

// ==================== Composable ====================

export function useNetworkProfileBridge(deps: NetworkBridgeDeps) {
  const {
    isProject,
    getProjectPath,
    saveProjectProfile,
    loadAllProject,
    loadAll,
    removeProjectProfile,
  } = deps

  // ===== Generic create/update =====

  function buildNetworkCfg(
    profile: Record<string, unknown>,
    networkType: string,
    configObj: Record<string, unknown>
  ): Promise<void> {
    const base = {
      id: (profile.id as string) || '',
      name: (profile.name as string) || `未命名-${networkType.toUpperCase()}`,
      network_type: networkType,
      origin: (profile.scope as string) || 'project',
      config: JSON.stringify(configObj),
    }
    if (profile.id) {
      return invoke('update_network_config', { nc: base })
        .then(() => loadAll())
        .catch((err: unknown) => {
          const msg = err instanceof Error ? err.message : String(err)
          console.error(`[NetworkBridge] update ${networkType} failed:`, msg)
        })
    }
    return invoke('create_network_config', { nc: { ...base, id: '' } })
      .then(() => loadAll())
      .catch((err: unknown) => {
        const msg = err instanceof Error ? err.message : String(err)
        console.error(`[NetworkBridge] create ${networkType} failed:`, msg)
      })
  }

  // ===== Create handlers =====

  async function createProfile(profile: Record<string, unknown>, protocol: string) {
    if (isProject) {
      const pp = await getProjectPath()
      if (pp) {
        await saveProjectProfile(profile, protocol, configMappers[protocol](profile), pp)
        await loadAllProject(pp)
      }
      return
    }
    await buildNetworkCfg(profile, protocol, configMappers[protocol](profile))
  }

  function createSshProfile(profile: Record<string, unknown>): Promise<void> {
    return createProfile(profile, 'ssh')
  }
  function createSslProfile(profile: Record<string, unknown>): Promise<void> {
    return createProfile(profile, 'ssl')
  }
  function createProxyProfile(profile: Record<string, unknown>): Promise<void> {
    return createProfile(profile, 'proxy')
  }

  // ===== Delete handlers =====

  async function deleteProfile(id: string, protocol: string) {
    if (isProject) {
      const pp = await getProjectPath()
      if (pp) {
        await removeProjectProfile(id, pp)
        await loadAllProject(pp)
      }
      return
    }
    await invoke('delete_network_config', { id }).catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err)
      console.error(`[NetworkBridge] delete ${protocol} failed:`, msg)
    })
    await loadAll()
  }

  function deleteSshProfile(id: string): Promise<void> {
    return deleteProfile(id, 'ssh')
  }
  function deleteSslProfile(id: string): Promise<void> {
    return deleteProfile(id, 'ssl')
  }
  function deleteProxyProfile(id: string): Promise<void> {
    return deleteProfile(id, 'proxy')
  }

  return {
    buildNetworkCfg,
    createSshProfile,
    createSslProfile,
    createProxyProfile,
    deleteSshProfile,
    deleteSslProfile,
    deleteProxyProfile,
  }
}
