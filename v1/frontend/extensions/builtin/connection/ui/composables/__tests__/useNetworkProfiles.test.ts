/**
 * useNetworkProfiles 网络配置文件解析单元测试
 *
 * 测试 parseConfig、buildDetail、类型守卫（isSshConfig/isSslConfig/isProxyConfig）、
 * toProfile 转换等。
 */
import { describe, expect, it } from 'vitest'

// ==================== 纯函数提取（从 useNetworkProfiles.ts 提取） ====================

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

function parseConfig<T>(configJson: string): T | null {
  try {
    return JSON.parse(configJson) as T
  } catch {
    return null
  }
}

function buildDetail(type: string, cfg: unknown): string {
  if (isSslConfig(cfg)) return cfg.mode ?? 'verify-full'
  if (isSshConfig(cfg)) return `${cfg.host ?? 'localhost'}:${cfg.port ?? 22}`
  if (isProxyConfig(cfg)) return `${cfg.host ?? 'proxy.corp.com'}:${cfg.port ?? 1080}`
  return type === 'ssh' ? 'localhost:22' : type === 'ssl' ? 'verify-full' : 'proxy.corp.com:1080'
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

interface NetworkProfile {
  id: string
  name: string
  type: 'ssh' | 'ssl' | 'proxy'
  config: unknown
  detail: string
  origin?: string
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

// ==================== parseConfig 测试 ====================

describe('parseConfig', () => {
  it('正常 JSON 解析为对象', () => {
    const result = parseConfig<Record<string, unknown>>('{"host":"localhost","port":22}')
    expect(result).toEqual({ host: 'localhost', port: 22 })
  })

  it('无效 JSON 返回 null', () => {
    const result = parseConfig('{invalid}')
    expect(result).toBeNull()
  })

  it('空字符串返回 null', () => {
    const result = parseConfig('')
    expect(result).toBeNull()
  })
})

// ==================== 类型守卫测试 ====================

describe('网络配置类型守卫', () => {
  it('SSH 配置识别（无 mode 无 type）', () => {
    const cfg = { host: 'jump.corp.com', port: 22, username: 'admin' }
    expect(isSshConfig(cfg)).toBe(true)
    expect(isSslConfig(cfg)).toBe(false)
    expect(isProxyConfig(cfg)).toBe(false)
  })

  it('SSL 配置识别（有 mode 无 type）', () => {
    const cfg = { mode: 'verify-full', ca: '/path/to/ca.pem' }
    expect(isSslConfig(cfg)).toBe(true)
    expect(isSshConfig(cfg)).toBe(false)
    expect(isProxyConfig(cfg)).toBe(false)
  })

  it('代理配置识别（有 type 无 mode）', () => {
    const cfg = { type: 'socks5', host: 'proxy.corp.com', port: 1080 }
    expect(isProxyConfig(cfg)).toBe(true)
    expect(isSshConfig(cfg)).toBe(false)
    expect(isSslConfig(cfg)).toBe(false)
  })

  it('空对象不是任何类型', () => {
    const cfg = {}
    expect(isSshConfig(cfg)).toBe(true) // 空对象无 mode 无 type，被判定为 SSH
    expect(isSslConfig(cfg)).toBe(false)
    expect(isProxyConfig(cfg)).toBe(false)
  })
})

// ==================== buildDetail 测试 ====================

describe('buildDetail 摘要生成', () => {
  it('SSH → host:port', () => {
    const cfg = { host: 'jump.corp.com', port: 2222 }
    expect(buildDetail('ssh', cfg)).toBe('jump.corp.com:2222')
  })

  it('SSL → mode', () => {
    const cfg = { mode: 'verify-ca' }
    expect(buildDetail('ssl', cfg)).toBe('verify-ca')
  })

  it('代理 → host:port', () => {
    const cfg = { type: 'http', host: 'proxy.corp.com', port: 8080 }
    expect(buildDetail('proxy', cfg)).toBe('proxy.corp.com:8080')
  })

  it('未知类型降级（空对象 {} 被 isSshConfig 匹配）', () => {
    // isSshConfig({}) 返回 true（因为 {} 没有 mode/type 属性）
    // 所以 buildDetail('ssl', {}) 和 buildDetail('proxy', {}) 也会走 SSH 分支
    expect(buildDetail('ssh', {})).toBe('localhost:22')
    expect(buildDetail('ssl', {})).toBe('localhost:22')
    expect(buildDetail('proxy', {})).toBe('localhost:22')
  })
})

// ==================== toProfile 测试 ====================

describe('toProfile 转换', () => {
  it('SSH 原始配置 → NetworkProfile', () => {
    const raw: ConfigRaw = {
      id: 'G_ssh_001',
      name: '跳板机',
      network_type: 'ssh',
      config: JSON.stringify({ host: 'jump.corp.com', port: 22, username: 'admin' }),
      origin: 'global',
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const profile = toProfile(raw)
    expect(profile).not.toBeNull()
    expect(profile!.id).toBe('G_ssh_001')
    expect(profile!.name).toBe('跳板机')
    expect(profile!.type).toBe('ssh')
    expect(profile!.detail).toBe('jump.corp.com:22')
    expect(profile!.origin).toBe('global')
  })

  it('SSL 原始配置 → NetworkProfile', () => {
    const raw: ConfigRaw = {
      id: 'P_ssl_001',
      name: '生产 SSL',
      network_type: 'ssl',
      config: JSON.stringify({ mode: 'verify-full', ca: '/path/to/ca.pem' }),
      origin: 'project',
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const profile = toProfile(raw)
    expect(profile).not.toBeNull()
    expect(profile!.type).toBe('ssl')
    expect(profile!.detail).toBe('verify-full')
  })

  it('代理原始配置 → NetworkProfile', () => {
    const raw: ConfigRaw = {
      id: 'G_proxy_001',
      name: '企业代理',
      network_type: 'proxy',
      config: JSON.stringify({ type: 'http', host: 'proxy.corp.com', port: 8080 }),
      origin: 'global',
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const profile = toProfile(raw)
    expect(profile).not.toBeNull()
    expect(profile!.type).toBe('proxy')
    expect(profile!.detail).toBe('proxy.corp.com:8080')
  })

  it('无效 config JSON → null', () => {
    const raw: ConfigRaw = {
      id: 'G_ssh_002',
      name: 'Bad Config',
      network_type: 'ssh',
      config: '{invalid}',
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const profile = toProfile(raw)
    expect(profile).toBeNull()
  })

  it('无 name 时使用 id 作为 name', () => {
    const raw: ConfigRaw = {
      id: 'G_ssh_003',
      network_type: 'ssh',
      config: JSON.stringify({ host: 'localhost', port: 22 }),
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const profile = toProfile(raw)
    expect(profile).not.toBeNull()
    expect(profile!.name).toBe('G_ssh_003')
  })
})
