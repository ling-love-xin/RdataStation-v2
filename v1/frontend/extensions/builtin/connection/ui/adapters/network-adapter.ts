/**
 * 网络适配器 — 前端协议链 ↔ 后端 NetworkConfig / ChainHop JSON 的双向转换
 *
 * 核心职责：
 * 1. ProtocolNode[] → ChainHop[] JSON（保存到 DB network_configs.config 列）
 * 2. Backend NetworkConfig → 前端 SshProfile / SslProfile / ProxyProfile
 * 3. 前端 profile → Backend NetworkConfig（创建/更新时的序列化）
 */

import type {
  ProtocolNode,
  SshProfile,
  SslProfile,
  ProxyProfile,
  ProfileScope,
} from '../types/network-chain'

// ==================== 后端响应类型 ====================

/** 后端 NetworkConfig 表行 */
export interface BackendNetworkConfig {
  id: string
  name: string | null
  network_type: string // "ssh" | "ssl" | "proxy" | "http_proxy" | "socks" | "socks5" | "chain"
  config: string // JSON string — ChainHop[] 或 SshConfig/SslConfig/ProxyConfig
  auth_config_id: string | null
  origin: string | null // "global" | "project"
  source_id: string | null
  snapshot_at: string | null
  created_at: string
  updated_at: string
}

// ==================== ChainHop JSON 结构（与后端 ChainHop enum 序列化一致） ====================

interface ChainHopJson {
  type: 'ssh' | 'ssl' | 'http_proxy' | 'socks_proxy'
  host?: string
  port?: number
  username?: string
  auth?: Record<string, unknown>
  remote_host?: string
  remote_port?: number
  local_port?: number
  timeout_secs?: number
  // ssl
  verify_server_cert?: boolean
  ca_cert_path?: string
  client_cert_path?: string
  client_key_path?: string
  min_tls_version?: string
}

// ==================== ProtocolNode → ChainHop JSON ====================

/**
 * 将协议链节点列表序列化为 ChainHop JSON 数组
 * 后端 parse_config_json("chain") 可以直接反序列化为 Vec<ChainHop>
 */
export function protocolChainToChainHops(
  chain: ProtocolNode[],
  sshProfiles: SshProfile[],
  sslProfiles: SslProfile[],
  proxyProfiles: ProxyProfile[]
): ChainHopJson[] {
  const hops: ChainHopJson[] = []

  for (const node of chain) {
    if (!node.enabled) continue

    const hop = protocolNodeToChainHop(node, sshProfiles, sslProfiles, proxyProfiles)
    if (hop) {
      hops.push(hop)
    }
  }

  return hops
}

function protocolNodeToChainHop(
  node: ProtocolNode,
  sshProfiles: SshProfile[],
  sslProfiles: SslProfile[],
  proxyProfiles: ProxyProfile[]
): ChainHopJson | null {
  if (node.protocol === 'ssh') {
    const profile = findProfile(node, sshProfiles)
    if (!profile && node.mode !== 'custom') return null

    const data = node.mode === 'custom' ? node.customData || {} : profile || {}
    return {
      type: 'ssh',
      host: (data.host as string) || 'localhost',
      port: (data.port as number) || 22,
      username: (data.username as string) || 'root',
      auth: {
        type: (data.authType as string) === 'password' ? 'password' : 'private_key',
        ...(data.authType === 'password'
          ? { password: (data.password as string) || '' }
          : {
              key_path: (data.keyPath as string) || '~/.ssh/id_rsa',
              ...((data as { passphrase?: string }).passphrase
                ? { passphrase: (data as { passphrase?: string }).passphrase }
                : {}),
            }),
      },
      remote_host: (data.remoteHost as string) || '',
      remote_port: (data.remotePort as number) || 0,
      local_port: (data.localPort as number) || 0,
      timeout_secs: (data.keepAlive as number) || 60,
    }
  }

  if (node.protocol === 'ssl') {
    const profile = findProfile(node, sslProfiles)
    if (!profile && node.mode !== 'custom') return null

    const data = node.mode === 'custom' ? node.customData || {} : profile || {}
    return {
      type: 'ssl',
      verify_server_cert: (data.mode as string) !== 'require',
      ca_cert_path: (data.ca as string) || undefined,
      client_cert_path: (data.cert as string) || undefined,
      client_key_path: (data.key as string) || undefined,
    }
  }

  if (node.protocol === 'proxy') {
    const profile = findProfile(node, proxyProfiles)
    if (!profile && node.mode !== 'custom') return null

    const data = node.mode === 'custom' ? node.customData || {} : profile || {}
    const proxyType = (data.type as string) || 'socks5'
    const host = (data.host as string) || ''
    const port = (data.port as number) || 1080

    return {
      type: proxyType === 'http' ? 'http_proxy' : 'socks_proxy',
      host,
      port,
      ...(data.username
        ? {
            auth: {
              type: 'password',
              username: data.username as string,
              password: (data.password as string) || '',
            },
          }
        : {}),
    }
  }

  return null
}

// ==================== Backend NetworkConfig → 前端 Profile ====================

/**
 * 将后端 NetworkConfig 转为前端 SSH 配置
 */
export function backendConfigToSshProfile(nc: BackendNetworkConfig): SshProfile | null {
  try {
    const config = JSON.parse(nc.config) as Record<string, unknown>
    let auth: Record<string, unknown> | null = null
    if (typeof config.auth === 'object' && config.auth !== null) {
      auth = config.auth as Record<string, unknown>
    }

    return {
      id: nc.id,
      name: nc.name || '未命名SSH',
      scope: (nc.origin as ProfileScope) || 'project',
      host: (config.host as string) || '',
      port: (config.port as number) || 22,
      username: (config.username as string) || '',
      authType: auth?.type === 'password' ? 'password' : 'key',
      password: auth?.password as string | undefined,
      keyPath: auth?.key_path as string | undefined,
      localPort: config.local_port as number | undefined,
      remoteHost: config.remote_host as string | undefined,
      remotePort: config.remote_port as number | undefined,
      keepAlive: (config.timeout_secs as number) || 60,
    }
  } catch (err) {
    console.warn(
      '[network-adapter] SSH 配置 JSON 解析失败:',
      err instanceof Error ? err.message : String(err)
    )
    return null
  }
}
export function backendConfigToSslProfile(nc: BackendNetworkConfig): SslProfile | null {
  try {
    const config = JSON.parse(nc.config) as Record<string, unknown>
    const verifyCert = config.verify_server_cert as boolean
    const mode = verifyCert === false ? 'require' : 'verify-full'

    return {
      id: nc.id,
      name: nc.name || '未命名SSL',
      scope: (nc.origin as ProfileScope) || 'project',
      mode,
      ca: config.ca_cert_path as string | undefined,
      cert: config.client_cert_path as string | undefined,
      key: config.client_key_path as string | undefined,
    }
  } catch (err) {
    console.warn(
      '[network-adapter] SSL 配置 JSON 解析失败:',
      err instanceof Error ? err.message : String(err)
    )
    return null
  }
}
export function backendConfigToProxyProfile(nc: BackendNetworkConfig): ProxyProfile | null {
  try {
    const config = JSON.parse(nc.config) as Record<string, unknown>
    const ncType = nc.network_type
    let proxyType: 'socks5' | 'http' | 'socks4'
    if (ncType === 'http' || ncType === 'http_proxy') proxyType = 'http'
    else if (ncType === 'socks4') proxyType = 'socks4'
    else proxyType = 'socks5'

    let auth: Record<string, unknown> | null = null
    if (typeof config.auth === 'object' && config.auth !== null) {
      auth = config.auth as Record<string, unknown>
    }

    return {
      id: nc.id,
      name: nc.name || '未命名代理',
      scope: (nc.origin as ProfileScope) || 'project',
      type: proxyType,
      host: (config.host as string) || '',
      port: (config.port as number) || 1080,
      username: auth?.username as string | undefined,
      password: auth?.password as string | undefined,
    }
  } catch (err) {
    console.warn(
      '[network-adapter] 代理配置 JSON 解析失败:',
      err instanceof Error ? err.message : String(err)
    )
    return null
  }
}

/**
 * 将前端 SSH 配置文件转为网络配置 JSON（用于 create/update_network_config）
 */
export function sshProfileToNetworkConfig(profile: SshProfile): {
  config: string
  network_type: string
  name: string
} {
  const config = {
    host: profile.host,
    port: profile.port,
    username: profile.username,
    auth: {
      type: profile.authType === 'password' ? 'password' : 'private_key',
      ...(profile.authType === 'password'
        ? { password: profile.password || '' }
        : {
            key_path: profile.keyPath || '',
            ...(profile.password ? { passphrase: profile.password } : {}),
          }),
    },
    remote_host: profile.remoteHost || '',
    remote_port: profile.remotePort || 0,
    local_port: profile.localPort || 0,
    timeout_secs: profile.keepAlive,
  }

  return {
    config: JSON.stringify(config),
    network_type: 'ssh',
    name: profile.name,
  }
}

/**
 * 将前端 SSL 配置文件转为网络配置 JSON
 */
export function sslProfileToNetworkConfig(profile: SslProfile): {
  config: string
  network_type: string
  name: string
} {
  const config = {
    verify_server_cert: profile.mode !== 'require',
    ca_cert_path: profile.ca || null,
    client_cert_path: profile.cert || null,
    client_key_path: profile.key || null,
  }

  return {
    config: JSON.stringify(config),
    network_type: 'ssl',
    name: profile.name,
  }
}

/**
 * 将前端代理配置文件转为网络配置 JSON
 */
export function proxyProfileToNetworkConfig(profile: ProxyProfile): {
  config: string
  network_type: string
  name: string
} {
  const networkType = profile.type === 'http' ? 'http_proxy' : 'socks5'

  const config: Record<string, unknown> = {
    host: profile.host,
    port: profile.port,
  }

  if (profile.username) {
    config.auth = {
      type: 'password',
      username: profile.username,
      password: profile.password || '',
    }
  }

  return {
    config: JSON.stringify(config),
    network_type: networkType,
    name: profile.name,
  }
}

// ==================== 辅助 ====================

function findProfile<T extends { id: string }>(node: ProtocolNode, profiles: T[]): T | undefined {
  if (!node.profileId) return undefined
  return profiles.find(p => p.id === node.profileId)
}
