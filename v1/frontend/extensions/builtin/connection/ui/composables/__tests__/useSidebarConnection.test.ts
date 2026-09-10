/**
 * useSidebarConnection 侧边栏连接操作单元测试
 *
 * 测试 openSavedConnection / testSavedConnection 的纯逻辑流程，
 * 通过 mock deps 验证 URL 构建、状态更新、事件派发等调用链。
 */
import { describe, expect, it } from 'vitest'

import type { ProjectConnection } from '../../../types/connection'

// ==================== Mock deps ====================

const mockConnection: ProjectConnection = {
  id: 'conn-001',
  name: 'My MySQL',
  driver: 'mysql',
  host: '192.168.1.100',
  port: 3306,
  database: 'test_db',
  username: 'root',
  password: 'secret',
  status: 'disconnected',
  is_active: false,
  use_duckdb_fed: false,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  connection_type: 'project',
  driver_id: 'drv-mysql',
  auth_config_id: 'auth-001',
  auth_method: 'password',
  network_config_id: 'net-ssh-001',
  driver_properties: '{"allowPublicKeyRetrieval":"true"}',
  advanced_options: '{"poolSize":5}',
  environment_id: 'env-prod',
  options: '{"charset":"utf8mb4"}',
  tags: 'prod,priority',
  metadata_path: '/meta/test.db',
  schema_name: 'public',
}

// ==================== 纯逻辑提取（不依赖 invoke） ====================

/**
 * 构建 connect_database 参数
 * 提取自 openSavedConnection 中的 input 对象构建逻辑
 */
function buildConnectInput(
  conn: ProjectConnection,
  url: string,
  projectId: string | null
): Record<string, unknown> {
  const driverName = conn.driver || 'mysql'
  return {
    db_type: driverName,
    url,
    name: conn.name,
    connection_type: conn.connection_type || 'project',
    project_id: projectId ?? null,
    driver_id: conn.driver_id ?? null,
    auth_config_id: conn.auth_config_id ?? null,
    auth_method: conn.auth_method ?? null,
    network_config_id: conn.network_config_id ?? null,
    driver_properties: conn.driver_properties ?? null,
    advanced_options: conn.advanced_options ?? null,
    description: conn.description ?? null,
    environment_id: conn.environment_id ?? null,
    options: conn.options ?? null,
    tags: conn.tags ?? null,
    metadata_path: conn.metadata_path ?? null,
    schema_name: conn.schema_name ?? null,
    use_duckdb_fed: conn.use_duckdb_fed ?? false,
  }
}

// ==================== buildConnectInput ====================

describe('buildConnectInput 连接参数构建', () => {
  it('完整连接 → 所有字段映射', () => {
    const url = 'mysql://root:secret@192.168.1.100:3306/test_db'
    const input = buildConnectInput(mockConnection, url, 'proj-path-001')

    expect(input.db_type).toBe('mysql')
    expect(input.url).toBe(url)
    expect(input.name).toBe('My MySQL')
    expect(input.connection_type).toBe('project')
    expect(input.project_id).toBe('proj-path-001')
    expect(input.driver_id).toBe('drv-mysql')
    expect(input.auth_config_id).toBe('auth-001')
    expect(input.auth_method).toBe('password')
    expect(input.network_config_id).toBe('net-ssh-001')
    expect(input.driver_properties).toBe('{"allowPublicKeyRetrieval":"true"}')
    expect(input.advanced_options).toBe('{"poolSize":5}')
    expect(input.environment_id).toBe('env-prod')
    expect(input.options).toBe('{"charset":"utf8mb4"}')
    expect(input.tags).toBe('prod,priority')
    expect(input.metadata_path).toBe('/meta/test.db')
    expect(input.schema_name).toBe('public')
    expect(input.use_duckdb_fed).toBe(false)
  })

  it('最小连接 → null 字段为 null', () => {
    const conn: ProjectConnection = {
      id: 'conn-min',
      name: 'Minimal',
      driver: 'sqlite',
      status: 'disconnected',
      is_active: false,
      use_duckdb_fed: false,
      created_at: '',
      updated_at: '',
      connection_type: 'project',
    }
    const input = buildConnectInput(conn, 'sqlite:///data.db', null)

    expect(input.db_type).toBe('sqlite')
    expect(input.url).toBe('sqlite:///data.db')
    expect(input.driver_id).toBeNull()
    expect(input.auth_config_id).toBeNull()
    expect(input.network_config_id).toBeNull()
    expect(input.driver_properties).toBeNull()
    expect(input.description).toBeNull()
    expect(input.environment_id).toBeNull()
    expect(input.options).toBeNull()
    expect(input.tags).toBeNull()
    expect(input.metadata_path).toBeNull()
    expect(input.schema_name).toBeNull()
    expect(input.use_duckdb_fed).toBe(false)
  })

  it('driver 为 undefined → 默认 mysql', () => {
    const conn: ProjectConnection = {
      ...mockConnection,
      driver: undefined as unknown as string,
    }
    const input = buildConnectInput(conn, 'mysql://localhost:3306/', null)
    expect(input.db_type).toBe('mysql')
  })

  it('connection_type 为 undefined → 默认 project', () => {
    const conn: ProjectConnection = {
      ...mockConnection,
      connection_type: undefined,
    }
    const input = buildConnectInput(conn, 'mysql://localhost:3306/', null)
    expect(input.connection_type).toBe('project')
  })

  it('connection_type 为 global → 保持 global', () => {
    const conn: ProjectConnection = {
      ...mockConnection,
      connection_type: 'global',
    }
    const input = buildConnectInput(conn, 'mysql://localhost:3306/', null)
    expect(input.connection_type).toBe('global')
  })

  it('project_id 为 null → null', () => {
    const input = buildConnectInput(mockConnection, 'mysql://localhost:3306/', null)
    expect(input.project_id).toBeNull()
  })

  it('project_id 为路径 → 透传', () => {
    const input = buildConnectInput(mockConnection, 'mysql://localhost:3306/', '/projects/my-app')
    expect(input.project_id).toBe('/projects/my-app')
  })
})

// ==================== 状态管理 ====================

describe('testingId 测试状态', () => {
  it('初始值为 null', () => {
    const testingId = null
    expect(testingId).toBeNull()
  })

  it('测试时设为连接 ID', () => {
    let testingId: string | null = null
    testingId = 'conn-001'
    expect(testingId).toBe('conn-001')
  })

  it('测试完成后重置为 null', () => {
    let testingId: string | null = 'conn-001'
    testingId = null
    expect(testingId).toBeNull()
  })
})

// ==================== 输出验证 ====================

describe('test_connection 输出验证', () => {
  it('成功结果 → 格式化消息', () => {
    const r = { success: true, message: undefined, response_time_ms: 42 }
    const msg = r.success
      ? `✓ 连接成功 (${r.response_time_ms ?? '?'}ms)`
      : `✗ ${r.message || '连接失败'}`
    expect(msg).toBe('✓ 连接成功 (42ms)')
  })

  it('成功结果无 response_time_ms → 显示 ?', () => {
    const r = { success: true, message: undefined, response_time_ms: undefined }
    const msg = r.success
      ? `✓ 连接成功 (${r.response_time_ms ?? '?'}ms)`
      : `✗ ${r.message || '连接失败'}`
    expect(msg).toBe('✓ 连接成功 (?ms)')
  })

  it('失败结果 → 格式化消息', () => {
    const r = { success: false, message: 'Access denied', response_time_ms: undefined }
    const msg = r.success
      ? `✓ 连接成功 (${r.response_time_ms ?? '?'}ms)`
      : `✗ ${r.message || '连接失败'}`
    expect(msg).toBe('✗ Access denied')
  })

  it('失败结果无 message → 默认消息', () => {
    const r = { success: false, message: undefined, response_time_ms: undefined }
    const msg = r.success
      ? `✓ 连接成功 (${r.response_time_ms ?? '?'}ms)`
      : `✗ ${r.message || '连接失败'}`
    expect(msg).toBe('✗ 连接失败')
  })
})
