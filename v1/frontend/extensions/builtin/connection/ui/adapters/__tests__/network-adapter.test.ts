/**
 * network-adapter 网络适配器单元测试
 *
 * 测试 protocolChainToChainHops、backendConfigTo*、*ProfileToNetworkConfig 等双向转换函数。
 */
import { describe, expect, it } from 'vitest'

import type { ProtocolNode, SshProfile, SslProfile, ProxyProfile } from '../../types/network-chain'

// ==================== 纯函数提取（从 network-adapter.ts 提取） ====================

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
  verify_server_cert?: boolean
  ca_cert_path?: string
  client_cert_path?: string
  client_key_path?: string
  min_tls_version?: string
}

interface BackendNetworkConfig {
  id: string
  name: string | null
  network_type: string
  config: string
  auth_config_id: string | null
  origin: string | null
  source_id: string | null
  snapshot_at: string | null
  created_at: string
  updated_at: string
}

function findProfile<T extends { id: string }>(node: ProtocolNode, profiles: T[]): T | undefined {
  if (!node.profileId) return undefined
  return profiles.find(p => p.id === node.profileId)
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

function protocolChainToChainHops(
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

function backendConfigToSshProfile(nc: BackendNetworkConfig): SshProfile | null {
  try {
    const config = JSON.parse(nc.config) as Record<string, unknown>
    let auth: Record<string, unknown> | null = null
    if (typeof config.auth === 'object' && config.auth !== null) {
      auth = config.auth as Record<string, unknown>
    }

    return {
      id: nc.id,
      name: nc.name || '未命名SSH',
      scope: (nc.origin as 'global' | 'project') || 'project',
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
  } catch {
    return null
  }
}

function backendConfigToSslProfile(nc: BackendNetworkConfig): SslProfile | null {
  try {
    const config = JSON.parse(nc.config) as Record<string, unknown>
    const verifyCert = config.verify_server_cert as boolean
    const mode = verifyCert === false ? 'require' : 'verify-full'

    return {
      id: nc.id,
      name: nc.name || '未命名SSL',
      scope: (nc.origin as 'global' | 'project') || 'project',
      mode,
      ca: config.ca_cert_path as string | undefined,
      cert: config.client_cert_path as string | undefined,
      key: config.client_key_path as string | undefined,
    }
  } catch {
    return null
  }
}

function backendConfigToProxyProfile(nc: BackendNetworkConfig): ProxyProfile | null {
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
      scope: (nc.origin as 'global' | 'project') || 'project',
      type: proxyType,
      host: (config.host as string) || '',
      port: (config.port as number) || 1080,
      username: auth?.username as string | undefined,
      password: auth?.password as string | undefined,
    }
  } catch {
    return null
  }
}

function sshProfileToNetworkConfig(profile: SshProfile): {
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

function sslProfileToNetworkConfig(profile: SslProfile): {
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

function proxyProfileToNetworkConfig(profile: ProxyProfile): {
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

// ==================== 测试数据 ====================

const sshProfile: SshProfile = {
  id: 'P_ssh_001',
  name: '跳板机',
  scope: 'project',
  host: 'jump.corp.com',
  port: 2222,
  username: 'admin',
  authType: 'password',
  password: 'ssh-secret',
  keepAlive: 60,
}

const sshProfileKey: SshProfile = {
  id: 'G_ssh_002',
  name: '密钥登录',
  scope: 'global',
  host: '10.0.0.1',
  port: 22,
  username: 'deploy',
  authType: 'key',
  keyPath: '/home/user/.ssh/deploy_rsa',
  keepAlive: 90,
}

const sslProfile: SslProfile = {
  id: 'P_ssl_001',
  name: 'SSL 证书',
  scope: 'project',
  mode: 'verify-full',
  ca: '/etc/ssl/ca.pem',
  cert: '/etc/ssl/client.pem',
  key: '/etc/ssl/client-key.pem',
}

const proxyProfile: ProxyProfile = {
  id: 'G_proxy_001',
  name: '公司代理',
  scope: 'global',
  type: 'socks5',
  host: 'proxy.corp.com',
  port: 1080,
  username: 'proxyuser',
  password: 'proxy-pass',
}

const proxyHttpProfile: ProxyProfile = {
  id: 'P_proxy_002',
  name: 'HTTP 代理',
  scope: 'project',
  type: 'http',
  host: 'http-proxy.corp.com',
  port: 8080,
}

// ==================== protocolChainToChainHops ====================

describe('protocolChainToChainHops 协议链 → ChainHop JSON', () => {
  it('空链 → 空数组', () => {
    const result = protocolChainToChainHops([], [], [], [])
    expect(result).toHaveLength(0)
  })

  it('禁用节点 → 跳过', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'ssh', enabled: false, mode: 'select', profileId: 'P_ssh_001' },
    ]
    const result = protocolChainToChainHops(chain, [sshProfile], [], [])
    expect(result).toHaveLength(0)
  })

  it('SSH select 模式 → 根据 profile 生成 hop', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'ssh', enabled: true, mode: 'select', profileId: 'P_ssh_001' },
    ]
    const result = protocolChainToChainHops(chain, [sshProfile], [], [])
    expect(result).toHaveLength(1)
    expect(result[0].type).toBe('ssh')
    expect(result[0].host).toBe('jump.corp.com')
    expect(result[0].port).toBe(2222)
    expect(result[0].username).toBe('admin')
    expect(result[0].auth).toEqual({ type: 'password', password: 'ssh-secret' })
  })

  it('SSH select 模式 profile 不存在 → 返回 null（跳过）', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'ssh', enabled: true, mode: 'select', profileId: 'NONEXISTENT' },
    ]
    const result = protocolChainToChainHops(chain, [sshProfile], [], [])
    expect(result).toHaveLength(0)
  })

  it('SSH custom 模式 → 根据 customData 生成 hop', () => {
    const chain: ProtocolNode[] = [
      {
        id: 'h1',
        protocol: 'ssh',
        enabled: true,
        mode: 'custom',
        profileId: '',
        customData: {
          host: 'custom.host.com',
          port: 9022,
          username: 'customuser',
          authType: 'password',
          password: 'custom-pass',
          keepAlive: 30,
        },
      },
    ]
    const result = protocolChainToChainHops(chain, [], [], [])
    expect(result).toHaveLength(1)
    expect(result[0].host).toBe('custom.host.com')
    expect(result[0].port).toBe(9022)
    expect(result[0].auth).toEqual({ type: 'password', password: 'custom-pass' })
  })

  it('SSH 密钥认证 → auth.type 为 private_key', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'ssh', enabled: true, mode: 'select', profileId: 'G_ssh_002' },
    ]
    const result = protocolChainToChainHops(chain, [sshProfileKey], [], [])
    expect(result).toHaveLength(1)
    expect(result[0].auth).toEqual({ type: 'private_key', key_path: '/home/user/.ssh/deploy_rsa' })
  })

  it('SSH 密钥认证含 passphrase（custom 模式）', () => {
    const chain: ProtocolNode[] = [
      {
        id: 'h1',
        protocol: 'ssh',
        enabled: true,
        mode: 'custom',
        profileId: '',
        customData: {
          authType: 'key',
          keyPath: '/home/user/.ssh/deploy_rsa',
          passphrase: 'key-passphrase',
        },
      },
    ]
    const result = protocolChainToChainHops(chain, [], [], [])
    expect(result[0].auth).toEqual({
      type: 'private_key',
      key_path: '/home/user/.ssh/deploy_rsa',
      passphrase: 'key-passphrase',
    })
  })

  it('SSL select 模式 → 根据 profile 生成 hop', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'ssl', enabled: true, mode: 'select', profileId: 'P_ssl_001' },
    ]
    const result = protocolChainToChainHops(chain, [], [sslProfile], [])
    expect(result).toHaveLength(1)
    expect(result[0].type).toBe('ssl')
    expect(result[0].verify_server_cert).toBe(true) // verify-full → true
    expect(result[0].ca_cert_path).toBe('/etc/ssl/ca.pem')
    expect(result[0].client_cert_path).toBe('/etc/ssl/client.pem')
    expect(result[0].client_key_path).toBe('/etc/ssl/client-key.pem')
  })

  it('SSL require mode → verify_server_cert = false', () => {
    const profileRequire: SslProfile = {
      ...sslProfile,
      mode: 'require',
    }
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'ssl', enabled: true, mode: 'select', profileId: 'P_ssl_001' },
    ]
    const result = protocolChainToChainHops(chain, [], [profileRequire], [])
    expect(result[0].verify_server_cert).toBe(false)
  })

  it('SSL custom 模式 → 根据 customData 生成 hop', () => {
    const chain: ProtocolNode[] = [
      {
        id: 'h1',
        protocol: 'ssl',
        enabled: true,
        mode: 'custom',
        profileId: '',
        customData: {
          mode: 'verify-ca',
          ca: '/custom/ca.pem',
        },
      },
    ]
    const result = protocolChainToChainHops(chain, [], [], [])
    expect(result).toHaveLength(1)
    expect(result[0].type).toBe('ssl')
    expect(result[0].verify_server_cert).toBe(true) // verify-ca !== 'require'
    expect(result[0].ca_cert_path).toBe('/custom/ca.pem')
  })

  it('Proxy select 模式 → socks5', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'proxy', enabled: true, mode: 'select', profileId: 'G_proxy_001' },
    ]
    const result = protocolChainToChainHops(chain, [], [], [proxyProfile])
    expect(result).toHaveLength(1)
    expect(result[0].type).toBe('socks_proxy')
    expect(result[0].host).toBe('proxy.corp.com')
    expect(result[0].port).toBe(1080)
    expect(result[0].auth).toEqual({
      type: 'password',
      username: 'proxyuser',
      password: 'proxy-pass',
    })
  })

  it('Proxy http 类型 → http_proxy', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'proxy', enabled: true, mode: 'select', profileId: 'P_proxy_002' },
    ]
    const result = protocolChainToChainHops(chain, [], [], [proxyHttpProfile])
    expect(result[0].type).toBe('http_proxy')
  })

  it('Proxy custom 模式 → 根据 customData 生成 hop', () => {
    const chain: ProtocolNode[] = [
      {
        id: 'h1',
        protocol: 'proxy',
        enabled: true,
        mode: 'custom',
        profileId: '',
        customData: {
          type: 'http',
          host: 'my-proxy.local',
          port: 3128,
        },
      },
    ]
    const result = protocolChainToChainHops(chain, [], [], [])
    expect(result).toHaveLength(1)
    expect(result[0].type).toBe('http_proxy')
    expect(result[0].host).toBe('my-proxy.local')
    expect(result[0].port).toBe(3128)
    expect(result[0].auth).toBeUndefined() // 无 username
  })

  it('多跳协议链 → SSH → Proxy → SSL', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'ssh', enabled: true, mode: 'select', profileId: 'P_ssh_001' },
      { id: 'h2', protocol: 'proxy', enabled: true, mode: 'select', profileId: 'G_proxy_001' },
      { id: 'h3', protocol: 'ssl', enabled: true, mode: 'select', profileId: 'P_ssl_001' },
    ]
    const result = protocolChainToChainHops(chain, [sshProfile], [sslProfile], [proxyProfile])
    expect(result).toHaveLength(3)
    expect(result[0].type).toBe('ssh')
    expect(result[1].type).toBe('socks_proxy')
    expect(result[2].type).toBe('ssl')
  })

  it('部分禁用节点 → 只输出启用的', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'ssh', enabled: true, mode: 'select', profileId: 'P_ssh_001' },
      { id: 'h2', protocol: 'proxy', enabled: false, mode: 'select', profileId: 'G_proxy_001' },
      { id: 'h3', protocol: 'ssl', enabled: true, mode: 'select', profileId: 'P_ssl_001' },
    ]
    const result = protocolChainToChainHops(chain, [sshProfile], [sslProfile], [proxyProfile])
    expect(result).toHaveLength(2)
    expect(result[0].type).toBe('ssh')
    expect(result[1].type).toBe('ssl')
  })

  it('select 模式 profileId 为空 → 返回 null', () => {
    const chain: ProtocolNode[] = [
      { id: 'h1', protocol: 'ssh', enabled: true, mode: 'select', profileId: '' },
    ]
    const result = protocolChainToChainHops(chain, [sshProfile], [], [])
    expect(result).toHaveLength(0)
  })
})

// ==================== backendConfigToSshProfile ====================

describe('backendConfigToSshProfile 后端配置 → SSH Profile', () => {
  it('密码认证 SSH 配置', () => {
    const nc: BackendNetworkConfig = {
      id: 'G_ssh_001',
      name: '跳板机',
      network_type: 'ssh',
      config: JSON.stringify({
        host: 'jump.corp.com',
        port: 2222,
        username: 'admin',
        auth: { type: 'password', password: 'secret123' },
        timeout_secs: 60,
      }),
      auth_config_id: null,
      origin: 'global',
      source_id: null,
      snapshot_at: null,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }

    const profile = backendConfigToSshProfile(nc)
    expect(profile).not.toBeNull()
    expect(profile!.id).toBe('G_ssh_001')
    expect(profile!.name).toBe('跳板机')
    expect(profile!.scope).toBe('global')
    expect(profile!.host).toBe('jump.corp.com')
    expect(profile!.port).toBe(2222)
    expect(profile!.username).toBe('admin')
    expect(profile!.authType).toBe('password')
    expect(profile!.password).toBe('secret123')
    expect(profile!.keepAlive).toBe(60)
  })

  it('密钥认证 SSH 配置', () => {
    const nc: BackendNetworkConfig = {
      id: 'P_ssh_002',
      name: '部署密钥',
      network_type: 'ssh',
      config: JSON.stringify({
        host: '10.0.0.1',
        port: 22,
        username: 'deploy',
        auth: { type: 'private_key', key_path: '/home/.ssh/deploy_rsa' },
        timeout_secs: 90,
      }),
      auth_config_id: null,
      origin: 'project',
      source_id: null,
      snapshot_at: null,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }

    const profile = backendConfigToSshProfile(nc)
    expect(profile!.authType).toBe('key')
    expect(profile!.keyPath).toBe('/home/.ssh/deploy_rsa')
    expect(profile!.password).toBeUndefined()
  })

  it('Name 为 null → 使用默认名', () => {
    const nc: BackendNetworkConfig = {
      id: 'G_ssh_003',
      name: null,
      network_type: 'ssh',
      config: JSON.stringify({ host: 'host', port: 22, username: 'user' }),
      auth_config_id: null,
      origin: null,
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToSshProfile(nc)
    expect(profile!.name).toBe('未命名SSH')
  })

  it('无效 JSON → 返回 null', () => {
    const nc: BackendNetworkConfig = {
      id: 'G_ssh_004',
      name: 'Broken',
      network_type: 'ssh',
      config: 'not-json{{{',
      auth_config_id: null,
      origin: null,
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToSshProfile(nc)
    expect(profile).toBeNull()
  })

  it('无 origin → 默认 project', () => {
    const nc: BackendNetworkConfig = {
      id: 'G_ssh_005',
      name: 'Test',
      network_type: 'ssh',
      config: JSON.stringify({ host: 'h', port: 22, username: 'u' }),
      auth_config_id: null,
      origin: null,
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToSshProfile(nc)
    expect(profile!.scope).toBe('project')
  })

  it('含 remote_host/remote_port/local_port', () => {
    const nc: BackendNetworkConfig = {
      id: 'G_ssh_006',
      name: 'Forward',
      network_type: 'ssh',
      config: JSON.stringify({
        host: 'gw',
        port: 22,
        username: 'u',
        remote_host: 'db.internal',
        remote_port: 3306,
        local_port: 13306,
      }),
      auth_config_id: null,
      origin: null,
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToSshProfile(nc)
    expect(profile!.remoteHost).toBe('db.internal')
    expect(profile!.remotePort).toBe(3306)
    expect(profile!.localPort).toBe(13306)
  })
})

// ==================== backendConfigToSslProfile ====================

describe('backendConfigToSslProfile 后端配置 → SSL Profile', () => {
  it('verify-full 模式', () => {
    const nc: BackendNetworkConfig = {
      id: 'P_ssl_001',
      name: 'Full SSL',
      network_type: 'ssl',
      config: JSON.stringify({
        verify_server_cert: true,
        ca_cert_path: '/etc/ssl/ca.pem',
        client_cert_path: '/etc/ssl/cert.pem',
        client_key_path: '/etc/ssl/key.pem',
      }),
      auth_config_id: null,
      origin: 'project',
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToSslProfile(nc)
    expect(profile!.mode).toBe('verify-full')
    expect(profile!.ca).toBe('/etc/ssl/ca.pem')
    expect(profile!.cert).toBe('/etc/ssl/cert.pem')
    expect(profile!.key).toBe('/etc/ssl/key.pem')
  })

  it('require 模式（verify_server_cert=false）', () => {
    const nc: BackendNetworkConfig = {
      id: 'P_ssl_002',
      name: 'No Verify',
      network_type: 'ssl',
      config: JSON.stringify({ verify_server_cert: false }),
      auth_config_id: null,
      origin: 'project',
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToSslProfile(nc)
    expect(profile!.mode).toBe('require')
  })

  it('无效 JSON → 返回 null', () => {
    const nc: BackendNetworkConfig = {
      id: 'P_ssl_003',
      name: 'Bad',
      network_type: 'ssl',
      config: 'bad-json',
      auth_config_id: null,
      origin: null,
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    expect(backendConfigToSslProfile(nc)).toBeNull()
  })
})

// ==================== backendConfigToProxyProfile ====================

describe('backendConfigToProxyProfile 后端配置 → Proxy Profile', () => {
  it('socks5 代理', () => {
    const nc: BackendNetworkConfig = {
      id: 'G_proxy_001',
      name: 'SOCKS5',
      network_type: 'socks5',
      config: JSON.stringify({
        host: 'proxy.corp.com',
        port: 1080,
        auth: { type: 'password', username: 'user', password: 'pass' },
      }),
      auth_config_id: null,
      origin: 'global',
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToProxyProfile(nc)
    expect(profile!.type).toBe('socks5')
    expect(profile!.host).toBe('proxy.corp.com')
    expect(profile!.port).toBe(1080)
    expect(profile!.username).toBe('user')
    expect(profile!.password).toBe('pass')
  })

  it('http_proxy 类型映射为 http', () => {
    const nc: BackendNetworkConfig = {
      id: 'P_proxy_002',
      name: 'HTTP',
      network_type: 'http_proxy',
      config: JSON.stringify({ host: 'http-proxy', port: 8080 }),
      auth_config_id: null,
      origin: 'project',
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToProxyProfile(nc)
    expect(profile!.type).toBe('http')
  })

  it('socks4 代理', () => {
    const nc: BackendNetworkConfig = {
      id: 'P_proxy_003',
      name: 'SOCKS4',
      network_type: 'socks4',
      config: JSON.stringify({ host: 'socks4.local', port: 1080 }),
      auth_config_id: null,
      origin: 'project',
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToProxyProfile(nc)
    expect(profile!.type).toBe('socks4')
  })

  it('无 auth 的代理', () => {
    const nc: BackendNetworkConfig = {
      id: 'P_proxy_004',
      name: 'No Auth',
      network_type: 'socks5',
      config: JSON.stringify({ host: 'proxy', port: 1080 }),
      auth_config_id: null,
      origin: null,
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    const profile = backendConfigToProxyProfile(nc)
    expect(profile!.username).toBeUndefined()
    expect(profile!.password).toBeUndefined()
  })

  it('无效 JSON → 返回 null', () => {
    const nc: BackendNetworkConfig = {
      id: 'P_proxy_005',
      name: 'Bad',
      network_type: 'socks5',
      config: '{{{',
      auth_config_id: null,
      origin: null,
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }
    expect(backendConfigToProxyProfile(nc)).toBeNull()
  })
})

// ==================== sshProfileToNetworkConfig ====================

describe('sshProfileToNetworkConfig SSH Profile → 后端配置', () => {
  it('密码认证 → 完整 JSON', () => {
    const result = sshProfileToNetworkConfig(sshProfile)
    expect(result.network_type).toBe('ssh')
    expect(result.name).toBe('跳板机')

    const parsed = JSON.parse(result.config)
    expect(parsed.host).toBe('jump.corp.com')
    expect(parsed.port).toBe(2222)
    expect(parsed.username).toBe('admin')
    expect(parsed.auth).toEqual({ type: 'password', password: 'ssh-secret' })
    expect(parsed.timeout_secs).toBe(60)
  })

  it('密钥认证', () => {
    const result = sshProfileToNetworkConfig(sshProfileKey)
    const parsed = JSON.parse(result.config)
    expect(parsed.auth).toEqual({ type: 'private_key', key_path: '/home/user/.ssh/deploy_rsa' })
  })

  it('密钥认证含 passphrase', () => {
    const profile: SshProfile = {
      ...sshProfileKey,
      password: 'my-passphrase',
    }
    const result = sshProfileToNetworkConfig(profile)
    const parsed = JSON.parse(result.config)
    expect(parsed.auth).toEqual({
      type: 'private_key',
      key_path: '/home/user/.ssh/deploy_rsa',
      passphrase: 'my-passphrase',
    })
  })
})

// ==================== sslProfileToNetworkConfig ====================

describe('sslProfileToNetworkConfig SSL Profile → 后端配置', () => {
  it('verify-full → verify_server_cert=true', () => {
    const result = sslProfileToNetworkConfig(sslProfile)
    expect(result.network_type).toBe('ssl')
    expect(result.name).toBe('SSL 证书')

    const parsed = JSON.parse(result.config)
    expect(parsed.verify_server_cert).toBe(true)
    expect(parsed.ca_cert_path).toBe('/etc/ssl/ca.pem')
    expect(parsed.client_cert_path).toBe('/etc/ssl/client.pem')
    expect(parsed.client_key_path).toBe('/etc/ssl/client-key.pem')
  })

  it('require → verify_server_cert=false', () => {
    const profile: SslProfile = { ...sslProfile, mode: 'require' }
    const result = sslProfileToNetworkConfig(profile)
    const parsed = JSON.parse(result.config)
    expect(parsed.verify_server_cert).toBe(false)
  })

  it('verify-ca → verify_server_cert=true', () => {
    const profile: SslProfile = { ...sslProfile, mode: 'verify-ca' }
    const result = sslProfileToNetworkConfig(profile)
    const parsed = JSON.parse(result.config)
    expect(parsed.verify_server_cert).toBe(true)
  })

  it('无证书路径 → null', () => {
    const profile: SslProfile = {
      id: 'P_ssl_min',
      name: 'Minimal',
      scope: 'project',
      mode: 'verify-full',
    }
    const result = sslProfileToNetworkConfig(profile)
    const parsed = JSON.parse(result.config)
    expect(parsed.ca_cert_path).toBeNull()
    expect(parsed.client_cert_path).toBeNull()
    expect(parsed.client_key_path).toBeNull()
  })
})

// ==================== proxyProfileToNetworkConfig ====================

describe('proxyProfileToNetworkConfig Proxy Profile → 后端配置', () => {
  it('socks5 代理', () => {
    const result = proxyProfileToNetworkConfig(proxyProfile)
    expect(result.network_type).toBe('socks5')
    const parsed = JSON.parse(result.config)
    expect(parsed.host).toBe('proxy.corp.com')
    expect(parsed.port).toBe(1080)
    expect(parsed.auth).toEqual({
      type: 'password',
      username: 'proxyuser',
      password: 'proxy-pass',
    })
  })

  it('http 代理 → network_type=http_proxy', () => {
    const result = proxyProfileToNetworkConfig(proxyHttpProfile)
    expect(result.network_type).toBe('http_proxy')
  })

  it('无认证的代理 → 不生成 auth 字段', () => {
    const profile: ProxyProfile = {
      id: 'P_proxy_noauth',
      name: 'No Auth',
      scope: 'project',
      type: 'socks5',
      host: 'proxy',
      port: 1080,
    }
    const result = proxyProfileToNetworkConfig(profile)
    const parsed = JSON.parse(result.config)
    expect(parsed.auth).toBeUndefined()
  })
})

// ==================== 双向转换往返测试 ====================

describe('往返测试 round-trip', () => {
  it('SSH 密码认证 → 后端 → 前端 一致性', () => {
    const original: SshProfile = {
      id: 'P_ssh_rt',
      name: '往返测试',
      scope: 'project',
      host: 'db.example.com',
      port: 2222,
      username: 'dba',
      authType: 'password',
      password: 'db-password',
      keepAlive: 120,
    }

    // 前端 → 后端
    const ncRaw = sshProfileToNetworkConfig(original)
    const nc: BackendNetworkConfig = {
      id: 'P_ssh_rt',
      name: ncRaw.name,
      network_type: ncRaw.network_type,
      config: ncRaw.config,
      auth_config_id: null,
      origin: 'project',
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }

    // 后端 → 前端
    const restored = backendConfigToSshProfile(nc)
    expect(restored).not.toBeNull()
    expect(restored!.host).toBe(original.host)
    expect(restored!.port).toBe(original.port)
    expect(restored!.username).toBe(original.username)
    expect(restored!.authType).toBe(original.authType)
    expect(restored!.password).toBe(original.password)
    expect(restored!.keepAlive).toBe(original.keepAlive)
  })

  it('SSL verify-full → 后端 → 前端 一致性', () => {
    const original: SslProfile = {
      id: 'P_ssl_rt',
      name: 'SSL 往返',
      scope: 'project',
      mode: 'verify-full',
      ca: '/ca.pem',
      cert: '/cert.pem',
      key: '/key.pem',
    }

    const ncRaw = sslProfileToNetworkConfig(original)
    const nc: BackendNetworkConfig = {
      id: 'P_ssl_rt',
      name: ncRaw.name,
      network_type: ncRaw.network_type,
      config: ncRaw.config,
      auth_config_id: null,
      origin: 'project',
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }

    const restored = backendConfigToSslProfile(nc)
    expect(restored).not.toBeNull()
    expect(restored!.mode).toBe(original.mode)
    expect(restored!.ca).toBe(original.ca)
    expect(restored!.cert).toBe(original.cert)
    expect(restored!.key).toBe(original.key)
  })

  it('Proxy socks5 → 后端 → 前端 一致性', () => {
    const original: ProxyProfile = {
      id: 'G_proxy_rt',
      name: '代理往返',
      scope: 'global',
      type: 'socks5',
      host: 'socks.corp.com',
      port: 1080,
      username: 'socksuser',
      password: 'socks-pass',
    }

    const ncRaw = proxyProfileToNetworkConfig(original)
    const nc: BackendNetworkConfig = {
      id: 'G_proxy_rt',
      name: ncRaw.name,
      network_type: ncRaw.network_type,
      config: ncRaw.config,
      auth_config_id: null,
      origin: 'global',
      source_id: null,
      snapshot_at: null,
      created_at: '',
      updated_at: '',
    }

    const restored = backendConfigToProxyProfile(nc)
    expect(restored).not.toBeNull()
    expect(restored!.type).toBe(original.type)
    expect(restored!.host).toBe(original.host)
    expect(restored!.port).toBe(original.port)
    expect(restored!.username).toBe(original.username)
    expect(restored!.password).toBe(original.password)
  })
})
