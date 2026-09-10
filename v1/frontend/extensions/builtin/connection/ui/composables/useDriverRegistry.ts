/**
 * 驱动注册表面 — 从 SQLite global.db 动态获取数据库驱动列表
 *
 * 数据流：
 *   Tauri invoke('get_data_source_types')  → DataSourceType[]  (global.db.data_source_types)
 *   Tauri invoke('get_available_drivers')  → DriverListResponse (global.db.drivers)
 *
 * 架构要点：
 *   - 新增数据库类型无需发版：SQLite INSERT 即可注册
 *   - config_schema JSON 驱动前端动态表单渲染
 */

import { invoke } from '@tauri-apps/api/core'
import { ref, readonly } from 'vue'

import type { DataSourceType, Driver, DriverListResponse, MissingDriver } from '../../domain/types'

const dataSourceTypes = ref<DataSourceType[]>([])
const drivers = ref<Driver[]>([])
const missingDrivers = ref<MissingDriver[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const fetched = ref(false)

/**
 * 获取数据源类型目录（按 category 分组，供侧边栏渲染）
 */
async function fetchDataSourceTypes(): Promise<DataSourceType[]> {
  const types = await invoke<DataSourceType[]>('get_data_source_types')
  dataSourceTypes.value = types
  return types
}

/**
 * 获取驱动列表
 *
 * @param projectPath 可选，传入时自动检测缺失的外部驱动文件
 */
async function fetchDrivers(projectPath?: string): Promise<Driver[]> {
  const response = await invoke<DriverListResponse>('get_available_drivers', {
    projectPath: projectPath ?? null,
  })
  drivers.value = response.drivers
  missingDrivers.value = response.missing
  return response.drivers
}

/** 缓存：上次加载的项目路径，用于检测项目切换时失效缓存 */
let lastProjectPath: string | undefined = undefined

/**
 * 一次性加载数据源类型和驱动列表
 */
async function loadAll(projectPath?: string): Promise<void> {
  // 项目切换时失效缓存，确保加载新项目的驱动列表
  if (fetched.value && lastProjectPath === projectPath) return
  loading.value = true
  error.value = null
  try {
    await Promise.all([fetchDataSourceTypes(), fetchDrivers(projectPath)])
    fetched.value = true
    lastProjectPath = projectPath
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
    console.error('[useDriverRegistry] Failed to load drivers:', e)
  } finally {
    loading.value = false
  }
}

/**
 * 获取指定类型的所有驱动
 */
function getDriversByType(typeId: string): Driver[] {
  return drivers.value.filter(d => d.type_id === typeId && d.enabled)
}

/**
 * 获取指定 category 的数据源类型
 */
function getTypesByCategory(category: string): DataSourceType[] {
  return dataSourceTypes.value.filter(t => t.category === category && t.enabled)
}

/**
 * 将数据源类型按 category 分组
 */
function getGroupedTypes(): Record<string, DataSourceType[]> {
  const groups: Record<string, DataSourceType[]> = {}
  for (const t of dataSourceTypes.value) {
    if (!t.enabled) continue
    if (!groups[t.category]) {
      groups[t.category] = []
    }
    groups[t.category].push(t)
  }
  return groups
}

// ==================== 驱动管理 ====================

interface DriverDetailResponse {
  driver: Driver
  availability: string
}

interface DriverFile {
  name: string
  size: number
  version: string
  path: string
}

/** 获取驱动详情（含可用性状态） */
async function getDriverDetail(
  driverId: string,
  projectPath?: string
): Promise<DriverDetailResponse> {
  return invoke<DriverDetailResponse>('get_driver_detail', {
    driverId,
    projectPath: projectPath ?? null,
  })
}

/** 安装外部驱动（下载并注册到本机） */
async function installDriver(driverId: string): Promise<void> {
  await invoke('install_driver', { driverId })
}

/** 列出某驱动在本机的所有文件 */
async function listDriverFiles(driverId: string): Promise<DriverFile[]> {
  return invoke<DriverFile[]>('list_driver_files', { driverId })
}

export function useDriverRegistry() {
  return {
    // 响应式数据
    dataSourceTypes: readonly(dataSourceTypes),
    drivers: readonly(drivers),
    missingDrivers: readonly(missingDrivers),
    loading: readonly(loading),
    error: readonly(error),
    fetched: readonly(fetched),

    // 方法
    loadAll,
    fetchDataSourceTypes,
    fetchDrivers,
    getDriversByType,
    getTypesByCategory,
    getGroupedTypes,

    // 驱动管理
    getDriverDetail,
    installDriver,
    listDriverFiles,
  }
}
