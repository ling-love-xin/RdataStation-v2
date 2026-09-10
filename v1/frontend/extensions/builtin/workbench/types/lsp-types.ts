/**
 * LSP（Language Server Protocol）扩展点类型定义
 *
 * 为代码编辑器预留 LSP 集成接口，不做实际 LSP 客户端实现。
 * 后续可通过插件系统注入 LSP 实现。
 */

export interface LspPosition {
  line: number
  character: number
}

export interface LspRange {
  start: LspPosition
  end: LspPosition
}

export interface LspDiagnostic {
  severity: 'error' | 'warning' | 'info' | 'hint'
  message: string
  range: LspRange
  source?: string
  code?: string
}

export interface LspCompletion {
  label: string
  kind: string
  detail?: string
  insertText?: string
  documentation?: string
}

export interface LspHover {
  contents: string
  range?: LspRange
}

export interface LspLocation {
  uri: string
  range: LspRange
}

/**
 * LSP 扩展点接口
 * 插件系统可注入此接口的实现来提供语言服务功能
 */
export interface LspExtensionPoint {
  /** 获取诊断信息（错误/警告波浪线） */
  getDiagnostics?: (filePath: string, content: string) => Promise<LspDiagnostic[]>

  /** 获取代码补全 */
  getCompletions?: (filePath: string, position: LspPosition) => Promise<LspCompletion[]>

  /** 获取悬停提示 */
  getHover?: (filePath: string, position: LspPosition) => Promise<LspHover | null>

  /** 跳转到定义 */
  getDefinition?: (filePath: string, position: LspPosition) => Promise<LspLocation | null>

  /** 格式化文档 */
  formatDocument?: (filePath: string, content: string) => Promise<string>
}
