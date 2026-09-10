/**
 * MySQL Driver Extension
 *
 * 提供 MySQL 数据库驱动支持
 */

import type {
  ExtensionContext,
  ExtensionAPI,
  ExtensionModule,
  Disposable,
  DatabaseDriverContribution,
  ConnectionProvider,
  Connection,
  QueryResult,
} from '../../core/types'

// MySQL Driver 扩展特定的 API 接口
interface MySQLDriverExtensionAPI extends ExtensionAPI {
  driver: {
    readonly driverId: string
    getContribution(): DatabaseDriverContribution
  }
}

interface MySQLConfig {
  host: string
  port: number
  database?: string
  username: string
  password: string
  ssl?: boolean
}

/**
 * MySQL 连接实现
 */
class MySQLConnection implements Connection {
  readonly id: string
  readonly state: 'connecting' | 'connected' | 'error' | 'closed' = 'disconnected' as 'connected'
  private config: MySQLConfig

  constructor(id: string, config: MySQLConfig) {
    this.id = id
    this.config = config
  }

  async disconnect(): Promise<void> {
    // eslint-disable-next-line no-console
    console.log(`[MySQL] Disconnecting: ${this.id}`)
  }

  async execute(sql: string): Promise<QueryResult> {
    // eslint-disable-next-line no-console
    console.log(`[MySQL] Executing: ${sql}`)
    return {
      columns: [],
      rows: [],
      rowCount: 0,
      executionTime: 0,
    }
  }
}

/**
 * MySQL 连接提供者
 */
const mysqlConnectionProvider: ConnectionProvider = {
  driverId: 'mysql',

  async connect(config: unknown): Promise<Connection> {
    const mysqlConfig = config as MySQLConfig
    const connectionId = `mysql_${Date.now()}`

    // TODO: 实际建立连接
    // eslint-disable-next-line no-console
    console.log(`[MySQL] Connecting to ${mysqlConfig.host}:${mysqlConfig.port}`)

    return new MySQLConnection(connectionId, mysqlConfig)
  },
}

/**
 * 扩展激活函数
 */
const activate = (context: ExtensionContext): MySQLDriverExtensionAPI => {
  // eslint-disable-next-line no-console
  console.log('[MySQL Driver] Activating for project:', context.project.name)

  const driverContribution: DatabaseDriverContribution = {
    id: 'mysql',
    name: 'MySQL',
    icon: 'database',
    features: [
      'schemas',
      'tables',
      'views',
      'procedures',
      'functions',
      'triggers',
      'indexes',
      'foreignKeys',
      'ssl',
      'sshTunnel',
    ],
    defaultPort: 3306,
    connectionSchema: {
      fields: [
        { name: 'host', label: 'Host', type: 'text', required: true, default: 'localhost' },
        { name: 'port', label: 'Port', type: 'number', required: true, default: 3306 },
        { name: 'database', label: 'Database', type: 'text', required: false },
        { name: 'username', label: 'Username', type: 'text', required: true },
        { name: 'password', label: 'Password', type: 'password', required: true },
        { name: 'ssl', label: 'Use SSL', type: 'checkbox', required: false, default: false },
      ],
    },
  }

  // 注册连接提供者
  const disposables: Disposable[] = [
    context.database.registerConnectionProvider(mysqlConnectionProvider),
  ]

  return {
    version: '1.0.0',
    project: context.project,
    commands: context.commands,
    window: context.window,
    workspace: context.workspace,
    database: context.database,
    sqlEditor: context.sqlEditor,
    events: context.events,
    configuration: context.configuration,
    utils: context.utils,

    driver: {
      driverId: 'mysql',
      getContribution: () => driverContribution,
    },

    dispose: () => {
      disposables.forEach(d => d.dispose())
    },
  }
}

const deactivate = (): void => {
  // eslint-disable-next-line no-console
  console.log('[MySQL Driver] Deactivated')
}

const extension: ExtensionModule = {
  activate: activate as (context: ExtensionContext) => ExtensionAPI,
  deactivate,
}

export default extension
