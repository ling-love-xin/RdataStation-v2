/**
 * Zod 类型验证工具
 *
 * 提供运行时类型验证功能
 */

import { z } from 'zod'

export const ConnectionSchema = z.object({
  id: z.string(),
  name: z.string(),
  type: z.string(),
  host: z.string().optional(),
  port: z.number().optional(),
  database: z.string().optional(),
  username: z.string().optional(),
  password: z.string().optional(),
  connectionStatus: z.enum(['connected', 'connecting', 'disconnected']).optional(),
  createdAt: z.number().optional(),
  updatedAt: z.number().optional(),
})

export const ConnectionGroupSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string().optional(),
  connectionIds: z.array(z.string()),
  expanded: z.boolean(),
  color: z.string().optional(),
  createdAt: z.number(),
  updatedAt: z.number(),
})

export const VirtualTreeNodeSchema = z.object({
  id: z.string(),
  type: z.string(),
  label: z.string(),
  children: z.array(z.any()).optional(),
  hasChildren: z.boolean().optional(),
  connectionId: z.string().optional(),
  databaseName: z.string().optional(),
  schemaName: z.string().optional(),
  tableName: z.string().optional(),
  icon: z.string().optional(),
  color: z.string().optional(),
  loading: z.boolean().optional(),
  expanded: z.boolean().optional(),
})

export const SearchIndexEntrySchema = z.object({
  nodeId: z.string(),
  nodeType: z.string(),
  connectionId: z.string(),
  labels: z.array(z.string()),
})

export const ToastMessageSchema = z.object({
  id: z.string(),
  type: z.enum(['success', 'error', 'info', 'warning']),
  message: z.string(),
  duration: z.number().optional(),
})

export const FiltersSchema = z.object({
  databaseType: z.string(),
  connectionStatus: z.string(),
  nodeTypes: z.array(z.string()),
  showSystemObjects: z.boolean(),
})

export type Connection = z.infer<typeof ConnectionSchema>
export type ConnectionGroup = z.infer<typeof ConnectionGroupSchema>
export type VirtualTreeNode = z.infer<typeof VirtualTreeNodeSchema>
export type SearchIndexEntry = z.infer<typeof SearchIndexEntrySchema>
export type ToastMessage = z.infer<typeof ToastMessageSchema>
export type Filters = z.infer<typeof FiltersSchema>

export function validateConnection(data: unknown): Connection | null {
  const result = ConnectionSchema.safeParse(data)
  return result.success ? result.data : null
}

export function validateConnectionGroup(data: unknown): ConnectionGroup | null {
  const result = ConnectionGroupSchema.safeParse(data)
  return result.success ? result.data : null
}

export function validateVirtualTreeNode(data: unknown): VirtualTreeNode | null {
  const result = VirtualTreeNodeSchema.safeParse(data)
  return result.success ? result.data : null
}

export function validateSearchIndexEntry(data: unknown): SearchIndexEntry | null {
  const result = SearchIndexEntrySchema.safeParse(data)
  return result.success ? result.data : null
}

export function validateToastMessage(data: unknown): ToastMessage | null {
  const result = ToastMessageSchema.safeParse(data)
  return result.success ? result.data : null
}

export function validateFilters(data: unknown): Filters | null {
  const result = FiltersSchema.safeParse(data)
  return result.success ? result.data : null
}
