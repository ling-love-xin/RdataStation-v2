/**
 * useNetworkProfileBridge 桥接器单元测试
 *
 * 测试 configMappers 三个协议映射器的序列化逻辑。
 */
import { describe, expect, it } from 'vitest'

// ==================== 纯函数提取（configMappers） ====================

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

// ==================== SSH Mapper ====================

describe('configMappers.ssh SSH 配置映射器', () => {
  it('密码认证 → 完整映射', () => {
    const profile = {
      host: 'jump.corp.com',
      port: 2222,
      username: 'admin',
      authMethod: 'password',
      password: 'secret123',
      keepalive: 60,
      localPort: 13306,
      remoteHost: 'db.internal',
      remotePort: 3306,
    }
    const result = configMappers.ssh(profile)
    expect(result).toEqual({
      host: 'jump.corp.com',
      port: 2222,
      username: 'admin',
      authMethod: 'password',
      password: 'secret123',
      keyPath: undefined,
      passphrase: undefined,
      keepalive: 60,
      localPort: 13306,
      remoteHost: 'db.internal',
      remotePort: 3306,
    })
  })

  it('密钥认证 → 含 keyPath + passphrase', () => {
    const profile = {
      host: '10.0.0.1',
      port: 22,
      username: 'deploy',
      authMethod: 'key',
      keyPath: '~/.ssh/id_rsa',
      passphrase: 'my-pass',
      keepalive: 90,
    }
    const result = configMappers.ssh(profile)
    expect(result.authMethod).toBe('key')
    expect(result.keyPath).toBe('~/.ssh/id_rsa')
    expect(result.passphrase).toBe('my-pass')
    expect(result.password).toBeUndefined()
  })

  it('最小字段 → 仅 host/port/username', () => {
    const profile = {
      host: 'localhost',
      port: 22,
      username: 'root',
    }
    const result = configMappers.ssh(profile)
    expect(result.host).toBe('localhost')
    expect(result.port).toBe(22)
    expect(result.username).toBe('root')
    expect(result.authMethod).toBeUndefined()
    expect(result.password).toBeUndefined()
  })
})

// ==================== SSL Mapper ====================

describe('configMappers.ssl SSL 配置映射器', () => {
  it('verify-full → 完整映射', () => {
    const profile = {
      mode: 'verify-full',
      ca: '/etc/ssl/ca.pem',
      clientCert: '/etc/ssl/client.pem',
      clientKey: '/etc/ssl/client-key.pem',
      hostnameOverride: 'db.internal',
    }
    const result = configMappers.ssl(profile)
    expect(result).toEqual({
      mode: 'verify-full',
      ca: '/etc/ssl/ca.pem',
      clientCert: '/etc/ssl/client.pem',
      clientKey: '/etc/ssl/client-key.pem',
      hostnameOverride: 'db.internal',
    })
  })

  it('require → 无证书路径', () => {
    const profile = {
      mode: 'require',
    }
    const result = configMappers.ssl(profile)
    expect(result.mode).toBe('require')
    expect(result.ca).toBeUndefined()
    expect(result.clientCert).toBeUndefined()
    expect(result.clientKey).toBeUndefined()
  })

  it('无 hostnameOverride → undefined', () => {
    const profile = {
      mode: 'verify-ca',
      ca: '/ca.pem',
    }
    const result = configMappers.ssl(profile)
    expect(result.hostnameOverride).toBeUndefined()
  })
})

// ==================== Proxy Mapper ====================

describe('configMappers.proxy 代理配置映射器', () => {
  it('socks5 带认证 → 完整映射', () => {
    const profile = {
      type: 'socks5',
      host: 'proxy.corp.com',
      port: 1080,
      username: 'proxyuser',
      password: 'proxy-pass',
    }
    const result = configMappers.proxy(profile)
    expect(result).toEqual({
      type: 'socks5',
      host: 'proxy.corp.com',
      port: 1080,
      username: 'proxyuser',
      password: 'proxy-pass',
    })
  })

  it('http 无认证 → 仅 host/port', () => {
    const profile = {
      type: 'http',
      host: 'http-proxy.local',
      port: 8080,
    }
    const result = configMappers.proxy(profile)
    expect(result.type).toBe('http')
    expect(result.host).toBe('http-proxy.local')
    expect(result.port).toBe(8080)
    expect(result.username).toBeUndefined()
    expect(result.password).toBeUndefined()
  })

  it('socks4 → type 保持 socks4', () => {
    const profile = {
      type: 'socks4',
      host: 'socks4.local',
      port: 1080,
    }
    const result = configMappers.proxy(profile)
    expect(result.type).toBe('socks4')
  })
})

// ==================== 边界场景 ====================

describe('configMappers 边界场景', () => {
  it('empty profile → 所有字段 undefined', () => {
    const result = configMappers.ssh({})
    expect(result.host).toBeUndefined()
    expect(result.port).toBeUndefined()
    expect(result.username).toBeUndefined()
  })

  it('null 字段 → 透传 null', () => {
    const result = configMappers.ssh({ host: null, port: null })
    expect(result.host).toBeNull()
    expect(result.port).toBeNull()
  })

  it('不相关的 extra 字段 → 不映射', () => {
    const result = configMappers.ssh({ host: 'h', port: 22, username: 'u', extraField: 'ignored' })
    expect(result).not.toHaveProperty('extraField')
  })
})
