/**
 * useNetworkChain — 协议链引擎 Composable
 *
 * 核心职责：
 * - 协议链的增删改查与排序
 * - SSL 末尾强制约束
 * - 跳数上限检查与警告
 * - 配置文件管理（选择/新建/自定义三种模式）
 * - 拓扑预览数据生成
 *
 * 配置文件状态从 useNetworkProfiles 共享状态读取，不再维护独立副本。
 */
import { invoke } from '@tauri-apps/api/core'
import { ref, computed } from 'vue'

import { useNetworkProfiles, type NetworkProfile } from './useNetworkProfiles'
import {
  sshProfileToNetworkConfig,
  sslProfileToNetworkConfig,
  proxyProfileToNetworkConfig,
  protocolChainToChainHops,
} from '../adapters/network-adapter'
import { MAX_NETWORK_HOPS, WARN_NETWORK_HOPS, createDefaultChain } from '../types/network-chain'

import type {
  ProtocolNode,
  ProtocolType,
  HopConfigMode,
  SshProfile,
  SslProfile,
  ProxyProfile,
  ProfileScope,
  TopologyNode,
  AddHopOption,
} from '../types/network-chain'

// ==================== NetworkProfile → 类型化 Profile 转换 ====================

function networkProfileToSshProfile(np: NetworkProfile): SshProfile | null {
  const cfg = np.config as Record<string, unknown> | null
  if (!cfg || typeof cfg !== 'object') return null
  let auth: Record<string, unknown> | null = null
  if (typeof cfg.auth === 'object' && cfg.auth !== null) {
    auth = cfg.auth as Record<string, unknown>
  }
  return {
    id: np.id,
    name: np.name,
    scope: (np.origin as ProfileScope) || 'project',
    host: (cfg.host as string) || '',
    port: (cfg.port as number) || 22,
    username: (cfg.username as string) || '',
    authType: auth?.type === 'password' ? 'password' : 'key',
    password: auth?.password as string | undefined,
    keyPath: auth?.key_path as string | undefined,
    localPort: cfg.local_port as number | undefined,
    remoteHost: cfg.remote_host as string | undefined,
    remotePort: cfg.remote_port as number | undefined,
    keepAlive: (cfg.timeout_secs as number) || 60,
  }
}

function networkProfileToSslProfile(np: NetworkProfile): SslProfile | null {
  const cfg = np.config as Record<string, unknown> | null
  if (!cfg || typeof cfg !== 'object') return null
  const verifyCert = cfg.verify_server_cert as boolean
  return {
    id: np.id,
    name: np.name,
    scope: (np.origin as ProfileScope) || 'project',
    mode: verifyCert === false ? 'require' : 'verify-full',
    ca: cfg.ca_cert_path as string | undefined,
    cert: cfg.client_cert_path as string | undefined,
    key: cfg.client_key_path as string | undefined,
  }
}

function networkProfileToProxyProfile(np: NetworkProfile): ProxyProfile | null {
  const cfg = np.config as Record<string, unknown> | null
  if (!cfg || typeof cfg !== 'object') return null
  let proxyType: 'socks5' | 'http' | 'socks4'
  const rawType = np.type as string
  if (rawType === 'http' || rawType === 'http_proxy') proxyType = 'http'
  else if (rawType === 'socks4') proxyType = 'socks4'
  else proxyType = 'socks5'
  let auth: Record<string, unknown> | null = null
  if (typeof cfg.auth === 'object' && cfg.auth !== null) {
    auth = cfg.auth as Record<string, unknown>
  }
  return {
    id: np.id,
    name: np.name,
    scope: (np.origin as ProfileScope) || 'project',
    type: proxyType,
    host: (cfg.host as string) || '',
    port: (cfg.port as number) || 1080,
    username: auth?.username as string | undefined,
    password: auth?.password as string | undefined,
  }
}

// ==================== Composable ====================

export function useNetworkChain(initialChain?: ProtocolNode[]) {
  const np = useNetworkProfiles()

  // ===== 配置文件状态（从 useNetworkProfiles 共享状态派生） =====

  const sshProfiles = computed(() =>
    np.sshProfiles.value.map(networkProfileToSshProfile).filter((p): p is SshProfile => p !== null)
  )
  const sslProfiles = computed(() =>
    np.sslProfiles.value.map(networkProfileToSslProfile).filter((p): p is SslProfile => p !== null)
  )
  const proxyProfiles = computed(() =>
    np.proxyProfiles.value
      .map(networkProfileToProxyProfile)
      .filter((p): p is ProxyProfile => p !== null)
  )

  // ===== 状态 =====
  const chain = ref<ProtocolNode[]>(initialChain ?? createDefaultChain())
  let hopIdCounter = chain.value.length + 1

  const menuOpen = ref(false)
  const dragSrcId = ref<string | null>(null)

  // ===== 计算属性 =====

  /** 网络跳总数（不含 SSL） */
  const networkHopCount = computed(() => chain.value.filter(h => h.protocol !== 'ssl').length)

  /** 已启用的网络跳数 */
  const enabledNetworkHopCount = computed(
    () => chain.value.filter(h => h.protocol !== 'ssl' && h.enabled).length
  )

  /** 是否达到网络跳上限 */
  const isMaxNetworkHops = computed(() => enabledNetworkHopCount.value >= MAX_NETWORK_HOPS)

  /** 是否存在 SSL 节点 */
  const hasSsl = computed(() => chain.value.some(h => h.protocol === 'ssl'))

  /** 是否存在已启用的 SSL 节点 */
  const hasEnabledSsl = computed(() => chain.value.some(h => h.protocol === 'ssl' && h.enabled))

  /** 是否应显示跳数警告 */
  const showHopWarning = computed(() => enabledNetworkHopCount.value >= WARN_NETWORK_HOPS)

  /** 预估延迟 (ms) */
  const estimatedLatency = computed(() => enabledNetworkHopCount.value * 25)

  /** 剩余可用跳数 */
  const remainingHops = computed(() => Math.max(0, MAX_NETWORK_HOPS - enabledNetworkHopCount.value))

  /** 添加菜单选项 */
  const addHopOptions = computed<AddHopOption[]>(() => [
    {
      protocol: 'ssh',
      icon: '🔒',
      label: 'SSH 隧道',
      hint: remainingHops.value > 0 ? `剩${remainingHops.value}跳` : '',
      disabled: remainingHops.value <= 0,
    },
    {
      protocol: 'proxy',
      icon: '🌐',
      label: '代理',
      hint: remainingHops.value > 0 ? `剩${remainingHops.value}跳` : '',
      disabled: remainingHops.value <= 0,
    },
    {
      protocol: 'ssl',
      icon: '🛡',
      label: 'SSL/TLS 加密',
      hint: '末尾层',
      disabled: false,
    },
  ])

  /** 拓扑节点列表（用于预览渲染） */
  const topologyNodes = computed<TopologyNode[]>(() => {
    const nodes: TopologyNode[] = [{ kind: 'self', label: '本机' }]

    const networkHops = chain.value.filter(h => h.protocol !== 'ssl' && h.enabled)
    const tlsEnabled = chain.value.some(h => h.protocol === 'ssl' && h.enabled)

    for (const hop of networkHops) {
      const label = hop.protocol === 'ssh' ? 'SSH' : 'Proxy'
      let detail: string | undefined

      if (hop.mode === 'select' && hop.profileId) {
        const profiles = getProfiles(hop.protocol)
        const p = profiles.find(x => x.id === hop.profileId)
        if (p) {
          if (hop.protocol === 'ssh') {
            const sshP = p as SshProfile
            detail = `${sshP.host}:${sshP.port}`
          } else {
            const proxyP = p as ProxyProfile
            detail = `${proxyP.host}:${proxyP.port}`
          }
        }
      }

      nodes.push({
        kind: hop.protocol === 'ssh' ? 'ssh' : 'proxy',
        label,
        detail,
      })
    }

    if (tlsEnabled) {
      nodes.push({ kind: 'ssl', label: 'TLS 加密' })
    }

    return nodes
  })

  /** 协议链是否为空（直连模式） */
  const isEmpty = computed(() => chain.value.length === 0)

  // ===== 辅助函数 =====

  function getProfiles(protocol: ProtocolType): (SshProfile | SslProfile | ProxyProfile)[] {
    if (protocol === 'ssh') return sshProfiles.value
    if (protocol === 'ssl') return sslProfiles.value
    return proxyProfiles.value
  }

  function countInstancesOfType(protocol: ProtocolType): number {
    return chain.value.filter(h => h.protocol === protocol).length
  }

  function findHop(id: string): ProtocolNode | undefined {
    return chain.value.find(h => h.id === id)
  }

  function generateHopId(): string {
    return `hop-${hopIdCounter++}`
  }

  // ===== SSL 约束 =====

  /** 确保 SSL 始终在链末尾 */
  function ensureSslAtEnd() {
    const sslIdx = chain.value.findIndex(h => h.protocol === 'ssl')
    if (sslIdx >= 0 && sslIdx < chain.value.length - 1) {
      const [ssl] = chain.value.splice(sslIdx, 1)
      chain.value.push(ssl)
    }
  }

  // ===== 节点操作 =====

  /** 切换启用/禁用 */
  function toggleHop(hopId: string) {
    const hop = findHop(hopId)
    if (!hop) return
    hop.enabled = !hop.enabled
  }

  /** 删除节点（每种协议至少保留一个） */
  function deleteHop(hopId: string): boolean {
    const hop = findHop(hopId)
    if (!hop) return false
    if (countInstancesOfType(hop.protocol) <= 1) return false

    const idx = chain.value.findIndex(h => h.id === hopId)
    chain.value.splice(idx, 1)
    return true
  }

  /** 切换配置模式 */
  function switchHopMode(hopId: string, mode: HopConfigMode) {
    const hop = findHop(hopId)
    if (!hop) return
    hop.mode = mode
    if (mode === 'select') {
      hop.profileId = ''
    }
  }

  /** 选择已有配置 */
  function selectProfile(hopId: string, profileId: string) {
    const hop = findHop(hopId)
    if (!hop) return
    hop.profileId = profileId
  }

  /** 添加协议节点 */
  function addHop(protocol: ProtocolType): string | null {
    if (protocol === 'ssl') {
      // SSL 始终在末尾：先移除旧 SSL，再追加新 SSL
      const existingIdx = chain.value.findIndex(h => h.protocol === 'ssl')
      if (existingIdx >= 0) {
        chain.value.splice(existingIdx, 1)
      }
      const newHop: ProtocolNode = {
        id: generateHopId(),
        protocol: 'ssl',
        enabled: true,
        mode: 'select',
        profileId: '',
      }
      chain.value.push(newHop)
      return newHop.id
    }

    // SSH/Proxy：检查上限
    if (isMaxNetworkHops.value) return null

    const newHop: ProtocolNode = {
      id: generateHopId(),
      protocol,
      enabled: true,
      mode: 'select',
      profileId: '',
    }

    // 在 SSL 之前插入（如果 SSL 存在）
    const sslIdx = chain.value.findIndex(h => h.protocol === 'ssl')
    if (sslIdx >= 0) {
      chain.value.splice(sslIdx, 0, newHop)
    } else {
      chain.value.push(newHop)
    }

    return newHop.id
  }

  /** 保存新建配置到后端 DB 并应用 */
  async function saveNewHop(
    hopId: string,
    data: Record<string, unknown>
  ): Promise<{ profileId: string; name: string; scope: ProfileScope } | null> {
    const hop = findHop(hopId)
    if (!hop) return null

    const name = (data.name as string) || '未命名'
    const scope: ProfileScope = (data.scope as ProfileScope) || 'project'

    try {
      if (hop.protocol === 'ssh') {
        const nc = sshProfileToNetworkConfig({
          id: '',
          name,
          scope,
          host: (data.host as string) || 'localhost',
          port: (data.port as number) || 22,
          username: (data.username as string) || 'root',
          authType: (data.authType as 'password' | 'key') || 'key',
          keyPath: data.keyPath as string | undefined,
          localPort: data.localPort as number | undefined,
          remoteHost: data.remoteHost as string | undefined,
          remotePort: data.remotePort as number | undefined,
          keepAlive: (data.keepAlive as number) || 60,
          password: data.password as string | undefined,
        })

        const result = await invoke<{ id: string }>('create_network_config', {
          nc: {
            id: '',
            name: nc.name,
            network_type: nc.network_type,
            config: nc.config,
            origin: scope,
          },
        }).catch(() => null)

        if (result?.id) {
          hop.mode = 'select'
          hop.profileId = result.id
          await np.loadAll()
          return { profileId: result.id, name, scope }
        }
      } else if (hop.protocol === 'ssl') {
        const nc = sslProfileToNetworkConfig({
          id: '',
          name,
          scope,
          mode: (data.mode as 'verify-full' | 'verify-ca' | 'require') || 'verify-full',
          ca: data.ca as string | undefined,
          cert: data.cert as string | undefined,
          key: data.key as string | undefined,
        })

        const result = await invoke<{ id: string }>('create_network_config', {
          nc: {
            id: '',
            name: nc.name,
            network_type: nc.network_type,
            config: nc.config,
            origin: scope,
          },
        }).catch(() => null)

        if (result?.id) {
          hop.mode = 'select'
          hop.profileId = result.id
          await np.loadAll()
          return { profileId: result.id, name, scope }
        }
      } else {
        const nc = proxyProfileToNetworkConfig({
          id: '',
          name,
          scope,
          type: (data.type as 'socks5' | 'http' | 'socks4') || 'socks5',
          host: (data.host as string) || '',
          port: (data.port as number) || 1080,
          username: data.username as string | undefined,
          password: data.password as string | undefined,
        })

        const result = await invoke<{ id: string }>('create_network_config', {
          nc: {
            id: '',
            name: nc.name,
            network_type: nc.network_type,
            config: nc.config,
            origin: scope,
          },
        }).catch(() => null)

        if (result?.id) {
          hop.mode = 'select'
          hop.profileId = result.id
          await np.loadAll()
          return { profileId: result.id, name, scope }
        }
      }
    } catch (err) {
      console.error('保存网络配置到 DB 失败:', err)
    }

    return null
  }

  /** 从后端 DB 加载已有配置文件（委托给 useNetworkProfiles 共享状态） */
  async function loadProfilesFromDb() {
    try {
      await np.loadAll()
    } catch (err) {
      console.error('加载网络配置文件失败:', err)
    }
  }

  /** 从后端 DB 删除配置文件 */
  async function deleteProfileInDb(protocol: ProtocolType, profileId: string) {
    try {
      await invoke('delete_network_config', { id: profileId })
      // 清除引用此配置的节点
      for (const hop of chain.value) {
        if (hop.protocol === protocol && hop.profileId === profileId) {
          hop.profileId = ''
        }
      }
      // 刷新共享状态
      await np.loadAll()
    } catch (err) {
      console.error('删除网络配置失败:', err)
    }
  }

  /**
   * 保存整个协议链为 network_config（用于连接保存前）
   * 返回创建的 network_config ID
   */
  async function saveChainToDb(scope: ProfileScope): Promise<string | null> {
    try {
      const hopsJson = protocolChainToChainHops(
        chain.value,
        sshProfiles.value,
        sslProfiles.value,
        proxyProfiles.value
      )
      const configStr = JSON.stringify(hopsJson)

      const result = await invoke<{ id: string }>('create_network_config', {
        nc: {
          id: '',
          name: '协议链',
          network_type: 'chain',
          config: configStr,
          origin: scope,
        },
      }).catch(e => {
        console.error('[useNetworkChain] 保存网络配置失败:', e)
        return null
      })

      return result?.id || null
    } catch (err) {
      console.error('保存协议链到 DB 失败:', err)
      return null
    }
  }

  // ===== 拖拽排序 =====

  function onDragStart(hopId: string) {
    dragSrcId.value = hopId
  }

  function onDragEnd() {
    dragSrcId.value = null
  }

  function onDrop(targetId: string): boolean {
    const srcId = dragSrcId.value
    if (!srcId || srcId === targetId) return false

    const srcIdx = chain.value.findIndex(h => h.id === srcId)
    const tgtIdx = chain.value.findIndex(h => h.id === targetId)
    if (srcIdx < 0 || tgtIdx < 0) return false

    const srcHop = chain.value[srcIdx]

    // SSL 不能拖到中间位置
    if (srcHop.protocol === 'ssl' && tgtIdx < chain.value.length - 1) {
      return false // 由调用方处理提示
    }

    const [moved] = chain.value.splice(srcIdx, 1)
    const newTgtIdx = chain.value.findIndex(h => h.id === targetId)
    chain.value.splice(newTgtIdx, 0, moved)

    ensureSslAtEnd()
    dragSrcId.value = null
    return true
  }

  // ===== 配置文件管理 =====

  function deleteProfile(protocol: ProtocolType, profileId: string) {
    // 清除引用此配置的节点（配置文件列表由 useNetworkProfiles 共享状态管理）
    for (const hop of chain.value) {
      if (hop.protocol === protocol && hop.profileId === profileId) {
        hop.profileId = ''
      }
    }
  }

  /** 应用配置文件到节点 */
  function applyProfile(hopId: string, profileId: string) {
    const hop = findHop(hopId)
    if (!hop) return
    hop.mode = 'select'
    hop.profileId = profileId
  }

  // ===== 网络配置输出（对接 NetworkTab emit） =====

  /** 生成 NetworkTab 输出的 networkConfig */
  const networkConfig = computed(() => {
    const result: Record<string, unknown> = { protocolChain: chain.value }

    // 同时输出扁平化字段以兼容旧 API
    for (const hop of chain.value) {
      if (!hop.enabled) continue

      if (hop.protocol === 'ssh') {
        result.ssh = { enabled: true }
        if (hop.mode === 'select' && hop.profileId) {
          const profile = sshProfiles.value.find(p => p.id === hop.profileId)
          if (profile) {
            result.ssh = { enabled: true, ...profile }
          }
        }
      } else if (hop.protocol === 'ssl') {
        result.ssl = { enabled: true }
        if (hop.mode === 'select' && hop.profileId) {
          const profile = sslProfiles.value.find(p => p.id === hop.profileId)
          if (profile) {
            result.ssl = { enabled: true, ...profile }
          }
        }
      } else if (hop.protocol === 'proxy') {
        result.proxy = { enabled: true }
        if (hop.mode === 'select' && hop.profileId) {
          const profile = proxyProfiles.value.find(p => p.id === hop.profileId)
          if (profile) {
            result.proxy = { enabled: true, ...profile }
          }
        }
      }
    }

    return result
  })

  function resetChain(newChain?: ProtocolNode[]) {
    chain.value = newChain ?? createDefaultChain()
    hopIdCounter = chain.value.length + 1
  }

  return {
    // 状态
    chain,
    menuOpen,
    dragSrcId,

    // 计算属性
    networkHopCount,
    enabledNetworkHopCount,
    isMaxNetworkHops,
    hasSsl,
    hasEnabledSsl,
    showHopWarning,
    estimatedLatency,
    remainingHops,
    addHopOptions,
    topologyNodes,
    isEmpty,
    sshProfiles,
    sslProfiles,
    proxyProfiles,
    networkConfig,

    // 操作
    toggleHop,
    deleteHop,
    switchHopMode,
    selectProfile,
    addHop,
    saveNewHop,
    onDragStart,
    onDragEnd,
    onDrop,
    deleteProfile,
    applyProfile,
    getProfiles,
    findHop,
    countInstancesOfType,
    ensureSslAtEnd,
    resetChain,

    // 后端 API 操作
    loadProfilesFromDb,
    deleteProfileInDb,
    saveChainToDb,
  }
}
