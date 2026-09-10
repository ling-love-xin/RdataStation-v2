/**
 * 连接服务类型定义
 */

import type { DriverDescriptor } from '../../domain/types'
import type { Connection, RecentConnection } from '../../types/connection'

export type { Connection, RecentConnection, DriverDescriptor }

// 连接服务接口
export interface ConnectionService {
  getConnections(): Promise<Connection[]>
  connect(dbType: string, url: string, name?: string): Promise<Connection>
  disconnect(connId: string): Promise<void>
  switchConnection(connId: string): Promise<void>
  getRecentConnections(): Promise<RecentConnection[]>
  removeRecentConnection(name: string): Promise<void>
  testConnection(dbType: string, url: string): Promise<boolean>
}

// 连接数据库输入
export interface ConnectDatabaseInput {
  db_type: string
  url: string
  name?: string
}

// 连接数据库响应
export interface ConnectDatabaseResponse {
  connId: string
  name: string
  dbType: string
  url: string
  status: 'connected' | 'disconnected' | 'error'
  isActive: boolean
}

// 连接信息响应
export interface ConnectionInfoResponse {
  connId: string
  name: string
  dbType: string
  url: string
  status: 'connected' | 'disconnected' | 'error'
  isActive: boolean
  meta?: {
    supportsTransaction?: boolean
    supportsStreaming?: boolean
    supportsArrow?: boolean
    supportsFederated?: boolean
    supportsConcurrentWrite?: boolean
    isInMemory?: boolean
  }
}

// 最近连接记录
export interface RecentConnectionRecord {
  id: string
  name: string
  dbType: string
  url: string
  connectedAt: string
}

// 连接配置
export interface ConnectionConfig {
  id?: string
  name: string
  driver: string
  host: string
  port?: number
  database?: string
  username?: string
  password?: string
  properties?: Record<string, unknown>
}
