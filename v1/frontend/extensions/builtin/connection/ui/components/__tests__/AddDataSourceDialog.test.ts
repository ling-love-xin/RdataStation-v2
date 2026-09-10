/**
 * @vitest-environment jsdom
 */
import { describe, expect, it } from 'vitest'

// ===== Pure logic extracted from AddDataSourceDialog =====

const AUTH_TYPE_FIELDS: Record<string, string[]> = {
  password: ['username', 'password'],
  ldap: ['username', 'password'],
  pg_class: ['certPath', 'certKeyPath'],
  kerberos: ['principal', 'keytabPath'],
  oauth2: ['tokenEndpoint', 'clientId', 'clientSecret'],
  ssh_password: ['username', 'password'],
  proxy_password: ['username', 'password'],
}

function isAuthRequired(authMethod: string): boolean {
  return ['password', 'ldap', 'pg_class', 'kerberos', 'oauth2'].includes(authMethod)
}

function buildAuthData(authType: string, fd: Record<string, unknown>): Record<string, unknown> {
  const fields = AUTH_TYPE_FIELDS[authType] ?? []
  const authData: Record<string, unknown> = {}
  for (const f of fields) {
    if (fd[f]) authData[f] = String(fd[f])
  }
  return authData
}

interface StagingItem {
  id: string
  name: string
  driver: string
  driverId?: string
  url: string
  formData: Record<string, unknown>
  authConfigId?: string | null
  authMethod?: string
  networkConfigId?: string | null
  driverProperties?: string | null
  advancedOptions?: string | null
  environmentId?: string | null
  description?: string
  schemaName?: string | null
  options?: string | null
  metadataPath?: string | null
  tags?: string | null
  useDuckdbFed?: boolean | null
  applied?: boolean
}

function buildStagingItem(
  name: string,
  driver: string,
  driverId: string | undefined,
  url: string,
  formData: Record<string, unknown>,
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
  return {
    id: crypto.randomUUID?.() || `staging-${Date.now()}`,
    name,
    driver,
    driverId,
    url,
    formData,
    authConfigId,
    authMethod: authMethod || 'password',
    networkConfigId,
    driverProperties,
    advancedOptions,
    environmentId,
    description,
    schemaName: schemaName ?? null,
    options: options ?? null,
    metadataPath: metadataPath ?? null,
    tags: tags ?? null,
    useDuckdbFed: useDuckdbFed ?? null,
  }
}

// ===== Tests =====

describe('AddDataSourceDialog — AUTH_TYPE_FIELDS', () => {
  it('password maps to username and password', () => {
    expect(AUTH_TYPE_FIELDS.password).toEqual(['username', 'password'])
  })

  it('pg_class maps to certPath and certKeyPath', () => {
    expect(AUTH_TYPE_FIELDS.pg_class).toEqual(['certPath', 'certKeyPath'])
  })

  it('kerberos maps to principal and keytabPath', () => {
    expect(AUTH_TYPE_FIELDS.kerberos).toEqual(['principal', 'keytabPath'])
  })

  it('oauth2 maps to 3 fields', () => {
    expect(AUTH_TYPE_FIELDS.oauth2).toHaveLength(3)
  })

  it('ssh_password maps to username and password', () => {
    expect(AUTH_TYPE_FIELDS.ssh_password).toEqual(['username', 'password'])
  })

  it('proxy_password maps to username and password', () => {
    expect(AUTH_TYPE_FIELDS.proxy_password).toEqual(['username', 'password'])
  })
})

describe('AddDataSourceDialog — isAuthRequired', () => {
  it('password requires auth', () => expect(isAuthRequired('password')).toBe(true))
  it('ldap requires auth', () => expect(isAuthRequired('ldap')).toBe(true))
  it('pg_class requires auth', () => expect(isAuthRequired('pg_class')).toBe(true))
  it('kerberos requires auth', () => expect(isAuthRequired('kerberos')).toBe(true))
  it('oauth2 requires auth', () => expect(isAuthRequired('oauth2')).toBe(true))
  it('os_auth does NOT require auth', () => expect(isAuthRequired('os_auth')).toBe(false))
  it('trust does NOT require auth', () => expect(isAuthRequired('trust')).toBe(false))
  it('ssh_password does NOT trigger auth save', () =>
    expect(isAuthRequired('ssh_password')).toBe(false))
  it('unknown type does NOT require auth', () => expect(isAuthRequired('unknown')).toBe(false))
})

describe('AddDataSourceDialog — buildAuthData', () => {
  it('builds password auth data', () => {
    const data = buildAuthData('password', { username: 'admin', password: 'secret' })
    expect(data).toEqual({ username: 'admin', password: 'secret' })
  })

  it('builds pg_class auth data', () => {
    const data = buildAuthData('pg_class', { certPath: '/a.crt', certKeyPath: '/a.key' })
    expect(data).toEqual({ certPath: '/a.crt', certKeyPath: '/a.key' })
  })

  it('builds kerberos auth data', () => {
    const data = buildAuthData('kerberos', {
      principal: 'user@REALM',
      keytabPath: '/etc/krb5.keytab',
    })
    expect(data).toEqual({ principal: 'user@REALM', keytabPath: '/etc/krb5.keytab' })
  })

  it('builds oauth2 auth data', () => {
    const data = buildAuthData('oauth2', {
      tokenEndpoint: 'https://auth.example.com',
      clientId: 'c1',
      clientSecret: 's1',
    })
    expect(data).toEqual({
      tokenEndpoint: 'https://auth.example.com',
      clientId: 'c1',
      clientSecret: 's1',
    })
  })

  it('skips empty fields', () => {
    const data = buildAuthData('password', { username: 'admin', password: '' })
    expect(data).toEqual({ username: 'admin' })
  })

  it('unknown type returns empty', () => {
    expect(buildAuthData('unknown', { username: 'test' })).toEqual({})
  })
})

describe('AddDataSourceDialog — buildStagingItem', () => {
  it('builds full staging item', () => {
    const item = buildStagingItem(
      'My DB',
      'mysql',
      'drv-001',
      'mysql://localhost:3306/db',
      { host: 'localhost', port: 3306 },
      'auth-001',
      'password',
      'net-001',
      '{"poolSize":10}',
      '{"timeout":30}',
      'env-001',
      'Test desc',
      'public',
      '{"charset":"utf8"}',
      '/meta/db',
      'prod,db',
      true
    )
    expect(item.name).toBe('My DB')
    expect(item.driver).toBe('mysql')
    expect(item.driverId).toBe('drv-001')
    expect(item.url).toBe('mysql://localhost:3306/db')
    expect(item.formData).toEqual({ host: 'localhost', port: 3306 })
    expect(item.authConfigId).toBe('auth-001')
    expect(item.authMethod).toBe('password')
    expect(item.networkConfigId).toBe('net-001')
    expect(item.driverProperties).toBe('{"poolSize":10}')
    expect(item.advancedOptions).toBe('{"timeout":30}')
    expect(item.environmentId).toBe('env-001')
    expect(item.description).toBe('Test desc')
    expect(item.schemaName).toBe('public')
    expect(item.options).toBe('{"charset":"utf8"}')
    expect(item.metadataPath).toBe('/meta/db')
    expect(item.tags).toBe('prod,db')
    expect(item.useDuckdbFed).toBe(true)
  })

  it('null values preserved', () => {
    const item = buildStagingItem(
      'Min DB',
      'sqlite',
      undefined,
      'sqlite:///data.db',
      {},
      null,
      '',
      null,
      null,
      null,
      null,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      false
    )
    expect(item.authConfigId).toBeNull()
    expect(item.networkConfigId).toBeNull()
    expect(item.schemaName).toBeNull()
    expect(item.options).toBeNull()
    expect(item.useDuckdbFed).toBe(false)
  })
})
