/**
 * 数据库对象加载器（Procedure / Function / Sequence / Trigger）
 *
 * 从 database-navigator-store 抽取，负责：
 * - 存储过程 / 函数 / 序列 / 触发器 列表加载
 * - 统一使用 mutateTreeNode 写入 Schema 节点对应字段
 *
 * 四个函数遵循相同模式：API 调用 → mutateTreeNode 写入
 */

import { ref, type Ref } from 'vue'

import * as databaseApi from '../../api/database-api'
import { mutateTreeNode } from '../../utils/tree-mutation'

import type {
  CatalogNode,
  SchemaNode,
  ProcedureNode,
  FunctionNode,
  SequenceNode,
  TriggerNode,
} from '../../types/nav-types'

// ========== Composable ==========

export function useObjectLoader(
  connectionCatalogs: Ref<Map<string, CatalogNode[]>>,
  connectionTypes: Ref<Map<string, 'global' | 'project'>>,
  connectionProjectPaths: Ref<Map<string, string | undefined>>,
  nodeErrors: Ref<Map<string, string>>
) {
  // 每种对象类型独立的 loading 状态，不再复用 loadingTables
  const loadingProcedures = ref<Set<string>>(new Set())
  const loadingFunctions = ref<Set<string>>(new Set())
  const loadingSequences = ref<Set<string>>(new Set())
  const loadingTriggers = ref<Set<string>>(new Set())

  // ========== Procedures ==========

  async function loadProcedures(connectionId: string, catalogName: string, schemaName: string) {
    const key = `${connectionId}:${catalogName}:${schemaName}:procedures`
    if (loadingProcedures.value.has(key)) return

    loadingProcedures.value.add(key)

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)

      const procedureMetas = await databaseApi.loadProcedures(
        connectionId,
        catalogName,
        schemaName,
        connType,
        projectPath
      )
      const procedures: ProcedureNode[] = procedureMetas.map(p => ({ name: p.name }))

      mutateTreeNode(
        connectionCatalogs.value,
        connectionId,
        { catalogName, schemaName },
        schema => {
          ;(schema as SchemaNode).procedures = procedures
        }
      )
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载存储过程列表失败'
      console.error('[object-loader] 加载存储过程列表失败:', key, e)
      nodeErrors.value.set(key, msg)
      throw e
    } finally {
      loadingProcedures.value.delete(key)
    }
  }

  // ========== Functions ==========

  async function loadFunctions(connectionId: string, catalogName: string, schemaName: string) {
    const key = `${connectionId}:${catalogName}:${schemaName}:functions`
    if (loadingFunctions.value.has(key)) return

    loadingFunctions.value.add(key)

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)

      const functionMetas = await databaseApi.loadFunctions(
        connectionId,
        catalogName,
        schemaName,
        connType,
        projectPath
      )
      const functions: FunctionNode[] = functionMetas.map(f => ({ name: f.name }))

      mutateTreeNode(
        connectionCatalogs.value,
        connectionId,
        { catalogName, schemaName },
        schema => {
          ;(schema as SchemaNode).functions = functions
        }
      )
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载函数列表失败'
      console.error('[object-loader] 加载函数列表失败:', key, e)
      nodeErrors.value.set(key, msg)
      throw e
    } finally {
      loadingFunctions.value.delete(key)
    }
  }

  // ========== Sequences ==========

  async function loadSequences(connectionId: string, catalogName: string, schemaName: string) {
    const key = `${connectionId}:${catalogName}:${schemaName}:sequences`
    if (loadingSequences.value.has(key)) return

    loadingSequences.value.add(key)

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)

      const sequenceMetas = await databaseApi.loadSequences(
        connectionId,
        catalogName,
        schemaName,
        connType,
        projectPath
      )
      const sequences: SequenceNode[] = sequenceMetas.map(s => ({ name: s.name }))

      mutateTreeNode(
        connectionCatalogs.value,
        connectionId,
        { catalogName, schemaName },
        schema => {
          ;(schema as SchemaNode).sequences = sequences
        }
      )
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载序列列表失败'
      console.error('[object-loader] 加载序列列表失败:', key, e)
      nodeErrors.value.set(key, msg)
      throw e
    } finally {
      loadingSequences.value.delete(key)
    }
  }

  // ========== Triggers ==========

  async function loadTriggers(connectionId: string, catalogName: string, schemaName: string) {
    const key = `${connectionId}:${catalogName}:${schemaName}:triggers`
    if (loadingTriggers.value.has(key)) return

    loadingTriggers.value.add(key)

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)

      const triggerMetas = await databaseApi.loadTriggers(
        connectionId,
        catalogName,
        schemaName,
        connType,
        projectPath
      )
      const triggers: TriggerNode[] = triggerMetas.map(t => ({
        name: t.name,
        tableName: t.tableName ?? undefined,
        event: t.event ?? undefined,
      }))

      mutateTreeNode(
        connectionCatalogs.value,
        connectionId,
        { catalogName, schemaName },
        schema => {
          ;(schema as SchemaNode).triggers = triggers
        }
      )
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载触发器列表失败'
      console.error('[object-loader] 加载触发器列表失败:', key, e)
      nodeErrors.value.set(key, msg)
      throw e
    } finally {
      loadingTriggers.value.delete(key)
    }
  }

  return {
    loadProcedures,
    loadFunctions,
    loadSequences,
    loadTriggers,
    loadingProcedures,
    loadingFunctions,
    loadingSequences,
    loadingTriggers,
  }
}
