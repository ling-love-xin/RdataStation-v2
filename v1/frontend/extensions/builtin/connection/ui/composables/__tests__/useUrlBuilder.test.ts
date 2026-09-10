/**
 * useUrlBuilder URI 构建与解析单元测试
 *
 * 测试 applyTemplate、buildUrl、uriPreview、parseUrl 等核心方法。
 */
import { describe, expect, it } from 'vitest'

// ==================== 纯函数提取（从 useUrlBuilder.ts 提取） ====================

function applyTemplate(template: string, fd: Record<string, unknown>): string {
  return template
    .replace('{host}', String(fd.host || 'localhost'))
    .replace('{port}', String(fd.port || ''))
    .replace('{database}', String(fd.database || ''))
    .replace('{username}', encodeURIComponent(String(fd.username || '')))
    .replace('{password}', encodeURIComponent(String(fd.password || '')))
    .replace('{file_path}', String(fd.file_path || fd.database || ''))
    .replace('{driver}', String(fd.driver || ''))
}

function getProto(id: string): string {
  return id.toLowerCase()
}

function buildUrlPreview(
  driverId: string,
  formData: Record<string, unknown>,
  urlTemplate?: string,
  isFile?: boolean
): string {
  if (urlTemplate) {
    const masked = { ...formData, password: formData.password ? '****' : '' }
    return applyTemplate(urlTemplate, masked)
  }
  if (isFile)
    return `${getProto(driverId)}://${formData.file_path || formData.database || './data.db'}`
  const usr = formData.username || 'user'
  const pw = formData.password ? '****' : ''
  const h = formData.host || 'localhost'
  const p = formData.port || ''
  const db = formData.database || ''
  if (pw) return `${getProto(driverId)}://${usr}:${pw}@${h}${p ? ':' + p : ''}/${db}`
  return `${getProto(driverId)}://${usr}@${h}${p ? ':' + p : ''}/${db}`
}

function buildUrl(
  driverId: string,
  formData: Record<string, unknown>,
  urlTemplate?: string,
  isFile?: boolean
): string {
  if (urlTemplate) {
    return applyTemplate(urlTemplate, formData)
  }
  const proto = getProto(driverId)
  if (isFile) return `${proto}://${formData.file_path || formData.database || './data.db'}`
  const h = String(formData.host || 'localhost')
  const po = String(formData.port || '')
  const db = String(formData.database || '')
  const u = encodeURIComponent(String(formData.username || ''))
  const pw = encodeURIComponent(String(formData.password || ''))
  if (u && pw) return `${proto}://${u}:${pw}@${h}${po ? ':' + po : ''}/${db}`
  return `${proto}://${u}@${h}${po ? ':' + po : ''}/${db}`
}

interface ParsedUrl {
  driver?: string
  host?: string
  port?: string
  database?: string
  username?: string
  password?: string
  params?: Record<string, string>
  isFile?: boolean
  filePath?: string
}

function parseUrl(raw: string): ParsedUrl | null {
  if (!raw || !raw.trim()) return null
  const url = raw.trim()

  // 文件型数据库
  const fileMatch = url.match(/^(\w+):\/\/\/?(.+)$/)
  if (fileMatch) {
    const [, proto, path] = fileMatch
    const knownFileDb = ['sqlite', 'duckdb', 'h2']
    if (knownFileDb.includes(proto.toLowerCase()) || path.match(/\.(db|sqlite|duckdb|sqlite3)$/i)) {
      return {
        driver: proto.toLowerCase(),
        isFile: true,
        filePath: path,
        database: path.split('/').pop() || path.split('\\').pop(),
      }
    }
  }

  // 标准 URL
  try {
    const match = url.match(
      /^(\w+):\/\/(?:([^:@]+)(?::([^@]*))?@)?(?:\[([^\]]+)\]|([^:/]+))(?::(\d+))?(?:\/([^?\n]*))?(?:\?(.*))?$/
    )
    if (!match) return null

    const [, proto, user, pass, ipv6Host, host, port, db, queryStr] = match

    const result: ParsedUrl = {
      driver: proto.toLowerCase(),
      host: ipv6Host || host,
      port,
      database: db || undefined,
      username: user || undefined,
      password: pass || undefined,
    }

    if (queryStr) {
      const params: Record<string, string> = {}
      for (const pair of queryStr.split('&')) {
        const [k, v] = pair.split('=')
        if (k) params[decodeURIComponent(k)] = v ? decodeURIComponent(v) : ''
      }
      if (Object.keys(params).length > 0) result.params = params
    }

    return result
  } catch {
    return null
  }
}

// ==================== applyTemplate 测试 ====================

describe('applyTemplate URL 模板', () => {
  it('mysql://{host}:{port}/{database} 模板替换', () => {
    const template = 'mysql://{host}:{port}/{database}'
    const fd = { host: '192.168.1.1', port: 3306, database: 'mydb' }
    const result = applyTemplate(template, fd)
    expect(result).toBe('mysql://192.168.1.1:3306/mydb')
  })

  it('template 中对用户名密码编码', () => {
    const template = 'postgres://{username}:{password}@{host}:{port}/{database}'
    const fd = {
      host: 'localhost',
      port: 5432,
      database: 'test',
      username: 'user@domain',
      password: 'p@ss:w0rd!',
    }
    const result = applyTemplate(template, fd)
    expect(result).toContain('user%40domain')
    expect(result).toContain('p%40ss%3Aw0rd!')
  })

  it('file_path 模板替换', () => {
    const template = 'sqlite://{file_path}'
    const fd = { file_path: '/data/mydb.db' }
    const result = applyTemplate(template, fd)
    expect(result).toBe('sqlite:///data/mydb.db')
  })
})

// ==================== uriPreview 测试 ====================

describe('uriPreview 预览', () => {
  it('MySQL 密码遮蔽为 ****', () => {
    const result = buildUrlPreview('mysql', {
      host: 'localhost',
      port: 3306,
      database: 'mydb',
      username: 'root',
      password: 'secret123',
    })
    expect(result).toBe('mysql://root:****@localhost:3306/mydb')
  })

  it('文件型数据库预览', () => {
    const result = buildUrlPreview('sqlite', { file_path: '/data/test.db' }, undefined, true)
    expect(result).toBe('sqlite:///data/test.db')
  })

  it('模板预览时密码遮蔽', () => {
    const template = 'mysql://{username}:{password}@{host}:{port}/{database}'
    const result = buildUrlPreview(
      'mysql',
      {
        host: 'localhost',
        port: 3306,
        database: 'test',
        username: 'root',
        password: 'secret',
      },
      template
    )
    expect(result).toContain('****')
    expect(result).not.toContain('secret')
  })
})

// ==================== buildUrl 测试 ====================

describe('buildUrl 实际 URL 构建', () => {
  it('MySQL 标准 URL 构建', () => {
    const result = buildUrl('mysql', {
      host: 'localhost',
      port: 3306,
      database: 'mydb',
      username: 'root',
      password: 'root',
    })
    expect(result).toBe('mysql://root:root@localhost:3306/mydb')
  })

  it('特殊字符编码', () => {
    const result = buildUrl('postgres', {
      host: 'localhost',
      port: 5432,
      database: 'mydb',
      username: 'user@domain',
      password: 'p@ss!',
    })
    expect(result).toContain('user%40domain')
    expect(result).toContain('p%40ss!')
  })

  it('无密码场景', () => {
    const result = buildUrl('mysql', {
      host: 'localhost',
      port: 3306,
      username: 'root',
    })
    expect(result).toBe('mysql://root@localhost:3306/')
  })

  it('文件型数据库', () => {
    const result = buildUrl('duckdb', { file_path: '/data/test.duckdb' }, undefined, true)
    expect(result).toBe('duckdb:///data/test.duckdb')
  })
})

// ==================== parseUrl 测试 ====================

describe('parseUrl URL 解析', () => {
  it('标准 MySQL URL 解析', () => {
    const result = parseUrl('mysql://root:root@localhost:3306/mydb')
    expect(result).not.toBeNull()
    expect(result!.driver).toBe('mysql')
    expect(result!.host).toBe('localhost')
    expect(result!.port).toBe('3306')
    expect(result!.database).toBe('mydb')
    expect(result!.username).toBe('root')
    expect(result!.password).toBe('root')
  })

  it('无密码 URL 解析', () => {
    const result = parseUrl('postgres://postgres@localhost:5432/test')
    expect(result).not.toBeNull()
    expect(result!.username).toBe('postgres')
    expect(result!.password).toBeUndefined()
  })

  it('带查询参数 URL 解析', () => {
    const result = parseUrl('mysql://root:root@localhost:3306/test?sslmode=disable&charset=utf8')
    expect(result).not.toBeNull()
    expect(result!.params).toEqual({ sslmode: 'disable', charset: 'utf8' })
  })

  it('SQLite 文件 URL 解析', () => {
    const result = parseUrl('sqlite:///data/mydb.db')
    expect(result).not.toBeNull()
    expect(result!.driver).toBe('sqlite')
    expect(result!.isFile).toBe(true)
    expect(result!.filePath).toBe('data/mydb.db')
  })

  it('DuckDB 文件 URL 解析', () => {
    const result = parseUrl('duckdb:///data/mydb.duckdb')
    expect(result).not.toBeNull()
    expect(result!.driver).toBe('duckdb')
    expect(result!.isFile).toBe(true)
  })

  it('IPv6 地址解析', () => {
    const result = parseUrl('mysql://root:root@[::1]:3306/mydb')
    expect(result).not.toBeNull()
    expect(result!.host).toBe('::1')
  })

  it('空字符串返回 null', () => {
    expect(parseUrl('')).toBeNull()
    expect(parseUrl('   ')).toBeNull()
  })

  it('无效 URL 返回 null', () => {
    expect(parseUrl('not-a-url')).toBeNull()
    expect(parseUrl('://')).toBeNull()
  })
})

// ==================== getProto 测试 ====================

describe('getProto 协议名获取', () => {
  it('mysql → mysql', () => {
    expect(getProto('mysql')).toBe('mysql')
  })

  it('SQLite → sqlite', () => {
    expect(getProto('SQLite')).toBe('sqlite')
  })

  it('PostgreSQL → postgresql', () => {
    expect(getProto('PostgreSQL')).toBe('postgresql')
  })
})
