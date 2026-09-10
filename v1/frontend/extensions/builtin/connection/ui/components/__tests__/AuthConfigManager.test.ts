/**
 * @vitest-environment jsdom
 */
import { describe, expect, it } from 'vitest'

// ===== Pure logic extracted from AuthConfigManager =====

interface AuthTypeDef {
  category: 'database' | 'ssh'
  icon: string
  label: string
  fields: string[]
}

const AUTH_TYPE_DEFS: Record<string, AuthTypeDef> = {
  password: {
    category: 'database',
    icon: '🔑',
    label: 'SCRAM-SHA-256 / mysql_native_password',
    fields: ['username', 'password'],
  },
  ldap: {
    category: 'database',
    icon: '📋',
    label: 'LDAP / Active Directory',
    fields: ['username', 'password'],
  },
  pg_class: {
    category: 'database',
    icon: '📜',
    label: 'SSL 客户端证书 (mTLS)',
    fields: ['certPath', 'certKeyPath'],
  },
  kerberos: {
    category: 'database',
    icon: '🎫',
    label: 'GSSAPI Kerberos',
    fields: ['principal', 'keytabPath'],
  },
  oauth2: {
    category: 'database',
    icon: '🔗',
    label: 'OAuth 2.0 Bearer Token',
    fields: ['tokenEndpoint', 'clientId', 'clientSecret'],
  },
  os_auth: { category: 'database', icon: '🖥', label: '操作系统认证 (OS Auth)', fields: [] },
  trust: { category: 'database', icon: '✅', label: '无认证 (Trust)', fields: [] },
  ssh_password: {
    category: 'ssh',
    icon: '🔑',
    label: 'SSH 密码认证',
    fields: ['username', 'password'],
  },
  ssh_private_key: {
    category: 'ssh',
    icon: '🔐',
    label: 'SSH 公钥认证',
    fields: ['username', 'keyPath', 'passphrase'],
  },
}

function needsUsername(type: string): boolean {
  const def = AUTH_TYPE_DEFS[type]
  return !!def?.fields?.includes('username')
}

function authTypeDef(type: string): AuthTypeDef | undefined {
  return AUTH_TYPE_DEFS[type]
}

const FIELD_KEY_MAP: Record<string, string> = {
  certPath: 'certPath',
  certKeyPath: 'certKeyPath',
  principal: 'principal',
  keytabPath: 'keytabPath',
  tokenEndpoint: 'tokenEndpoint',
  clientId: 'clientId',
  clientSecret: 'clientSecret',
  passphrase: 'passphrase',
}

function buildAuthData(newCfg: Record<string, string>): string {
  const data: Record<string, string> = {}
  if (newCfg.username) data.username = newCfg.username
  if (newCfg.password) data.password = newCfg.password
  for (const [formKey, dataKey] of Object.entries(FIELD_KEY_MAP)) {
    const val = newCfg[formKey]
    if (val) data[dataKey] = val
  }
  return JSON.stringify(data)
}

// ===== Tests =====

describe('AuthConfigManager — AUTH_TYPE_DEFS', () => {
  it('password type has username and password fields', () => {
    const def = AUTH_TYPE_DEFS.password
    expect(def.category).toBe('database')
    expect(def.fields).toContain('username')
    expect(def.fields).toContain('password')
  })

  it('pg_class has certPath and certKeyPath', () => {
    const def = AUTH_TYPE_DEFS.pg_class
    expect(def.fields).toContain('certPath')
    expect(def.fields).toContain('certKeyPath')
  })

  it('kerberos has principal and keytabPath', () => {
    const def = AUTH_TYPE_DEFS.kerberos
    expect(def.fields).toContain('principal')
    expect(def.fields).toContain('keytabPath')
  })

  it('oauth2 has 3 fields', () => {
    const def = AUTH_TYPE_DEFS.oauth2
    expect(def.fields).toHaveLength(3)
    expect(def.fields).toContain('tokenEndpoint')
    expect(def.fields).toContain('clientId')
    expect(def.fields).toContain('clientSecret')
  })

  it('os_auth has no fields', () => {
    expect(AUTH_TYPE_DEFS.os_auth.fields).toEqual([])
  })

  it('trust has no fields', () => {
    expect(AUTH_TYPE_DEFS.trust.fields).toEqual([])
  })

  it('ssh_password is ssh category', () => {
    expect(AUTH_TYPE_DEFS.ssh_password.category).toBe('ssh')
  })

  it('ssh_private_key has passphrase field', () => {
    expect(AUTH_TYPE_DEFS.ssh_private_key.fields).toContain('passphrase')
  })
})

describe('AuthConfigManager — needsUsername', () => {
  it('password needs username', () => {
    expect(needsUsername('password')).toBe(true)
  })

  it('ldap needs username', () => {
    expect(needsUsername('ldap')).toBe(true)
  })

  it('pg_class does NOT need username', () => {
    expect(needsUsername('pg_class')).toBe(false)
  })

  it('kerberos does NOT need username', () => {
    expect(needsUsername('kerberos')).toBe(false)
  })

  it('os_auth does NOT need username', () => {
    expect(needsUsername('os_auth')).toBe(false)
  })

  it('trust does NOT need username', () => {
    expect(needsUsername('trust')).toBe(false)
  })

  it('ssh_password needs username', () => {
    expect(needsUsername('ssh_password')).toBe(true)
  })

  it('ssh_private_key needs username', () => {
    expect(needsUsername('ssh_private_key')).toBe(true)
  })
})

describe('AuthConfigManager — buildAuthData', () => {
  it('builds password auth data', () => {
    const data = buildAuthData({ username: 'admin', password: 'secret' })
    const parsed = JSON.parse(data)
    expect(parsed.username).toBe('admin')
    expect(parsed.password).toBe('secret')
  })

  it('builds pg_class auth data', () => {
    const data = buildAuthData({ certPath: '/path/to/cert.crt', certKeyPath: '/path/to/key.key' })
    const parsed = JSON.parse(data)
    expect(parsed.certPath).toBe('/path/to/cert.crt')
    expect(parsed.certKeyPath).toBe('/path/to/key.key')
  })

  it('builds kerberos auth data', () => {
    const data = buildAuthData({ principal: 'user@REALM.COM', keytabPath: '/etc/krb5.keytab' })
    const parsed = JSON.parse(data)
    expect(parsed.principal).toBe('user@REALM.COM')
    expect(parsed.keytabPath).toBe('/etc/krb5.keytab')
  })

  it('builds oauth2 auth data', () => {
    const data = buildAuthData({
      tokenEndpoint: 'https://auth.example.com/token',
      clientId: 'client123',
      clientSecret: 'secret123',
    })
    const parsed = JSON.parse(data)
    expect(parsed.tokenEndpoint).toBe('https://auth.example.com/token')
    expect(parsed.clientId).toBe('client123')
    expect(parsed.clientSecret).toBe('secret123')
  })

  it('skips empty fields', () => {
    const data = buildAuthData({ username: 'admin', password: '' })
    const parsed = JSON.parse(data)
    expect(parsed.username).toBe('admin')
    expect(parsed.password).toBeUndefined()
  })

  it('skips passphrase when empty', () => {
    const data = buildAuthData({ username: 'admin', passphrase: '' })
    const parsed = JSON.parse(data)
    expect(parsed.passphrase).toBeUndefined()
  })
})

describe('AuthConfigManager — authTypeDef', () => {
  it('returns definition for known type', () => {
    expect(authTypeDef('password')?.icon).toBe('🔑')
  })

  it('returns undefined for unknown type', () => {
    expect(authTypeDef('unknown')).toBeUndefined()
  })
})
