/**
 * useUrlBuilder — URI 构建 Composable
 *
 * 从 AddDataSourceDialog.vue 提取，处理：
 * - URI 预览（uriPreview computed）
 * - 连接 URL 构建（buildUrl，用于连接测试和保存）
 */
import { computed, type ComputedRef, type Ref } from 'vue'

export interface DriverInfo {
  id: string
  name: string
  type_id: string
  is_file?: boolean
  default_port?: number | string
  url_template?: string
}

export interface UseUrlBuilderOptions {
  /** 当前选中的驱动（可为 null） */
  selectedDriver: ComputedRef<DriverInfo | null>
  /** 表单数据 */
  formData: Ref<Record<string, unknown>>
  /** 是否处于手动 URI 编辑模式 */
  uriEditing: Ref<boolean>
  /** 手动编辑的 URI 值 */
  manualUri: Ref<string>
}

/// P1: URL 解析结果
export interface ParsedUrl {
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

export function useUrlBuilder(opts: UseUrlBuilderOptions) {
  const { selectedDriver, formData, uriEditing, manualUri } = opts

  /** 使用 url_template 构建 URL（对用户名/密码进行编码） */
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

  /** 从驱动信息中获得协议前缀（id 即为协议名，如 mysql / postgresql） */
  function getProto(d: DriverInfo): string {
    return d.id.toLowerCase()
  }

  /** URI 预览（展示用，密码用 **** 遮蔽） */
  const uriPreview = computed(() => {
    const d = selectedDriver.value
    if (!d) return ''
    const fd = formData.value
    if (d.url_template) {
      const masked = { ...fd, password: fd.password ? '****' : '' }
      return applyTemplate(d.url_template, masked)
    }
    if (d.is_file) return `${getProto(d)}://${fd.file_path || fd.database || './data.db'}`
    const usr = fd.username || 'user'
    const pw = fd.password ? '****' : ''
    const h = fd.host || 'localhost'
    const p = fd.port || d.default_port || ''
    const db = fd.database || ''
    if (pw) return `${getProto(d)}://${usr}:${pw}@${h}${p ? ':' + p : ''}/${db}`
    return `${getProto(d)}://${usr}@${h}${p ? ':' + p : ''}/${db}`
  })

  /** 构建实际连接 URL（用于测试/保存），对用户名/密码特殊字符编码 */
  function buildUrl(): string {
    if (uriEditing.value && manualUri.value) return manualUri.value
    const d = selectedDriver.value
    if (!d) return ''
    const fd = formData.value
    if (d.url_template) {
      return applyTemplate(d.url_template, fd)
    }
    const proto = getProto(d)
    if (d.is_file) return `${proto}://${fd.file_path || fd.database || './data.db'}`
    const h = String(fd.host || 'localhost')
    const po = String(fd.port || d.default_port || '')
    const db = String(fd.database || '')
    const u = encodeURIComponent(String(fd.username || ''))
    const pw = encodeURIComponent(String(fd.password || ''))
    if (u && pw) return `${proto}://${u}:${pw}@${h}${po ? ':' + po : ''}/${db}`
    return `${proto}://${u}@${h}${po ? ':' + po : ''}/${db}`
  }

  /** P1: 解析 JDBC/标准 URL → 提取数据库类型、主机、端口等（支持 IPv6） */
  function parseUrl(raw: string): ParsedUrl | null {
    if (!raw || !raw.trim()) return null
    const url = raw.trim()

    // 1. 文件型数据库（file:// 或 sqlite://path/to/file 等路径模式）
    const fileMatch = url.match(/^(\w+):\/\/\/?(.+)$/)
    if (fileMatch) {
      const [_full, proto, path] = fileMatch
      const knownFileDb = ['sqlite', 'duckdb', 'h2']
      if (
        knownFileDb.includes(proto.toLowerCase()) ||
        path.match(/\.(db|sqlite|duckdb|sqlite3)$/i)
      ) {
        return {
          driver: proto.toLowerCase(),
          isFile: true,
          filePath: path,
          database: path.split('/').pop() || path.split('\\').pop(),
        }
      }
    }

    // 2. 标准 URL: proto://[user[:pass]@]host[:port][/database][?params]
    //    支持 IPv6 地址（[::1]、[2001:db8::1] 等）
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

  return { uriPreview, buildUrl, parseUrl }
}
