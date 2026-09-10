/**
 * 协议链类型定义
 *
 * 协议链引擎：支持 SSH/Proxy 任意交替穿插（最多 4 跳网络节点），
 * SSL/TLS 固定末尾（流加密包装器，不产生新网络节点）。
 */

// ==================== 协议节点 ====================

/** 协议类型 */
export type ProtocolType = 'ssh' | 'ssl' | 'proxy'

/** 节点配置模式 */
export type HopConfigMode = 'select' | 'new' | 'custom'

/** 协议链节点 */
export interface ProtocolNode {
  /** 唯一标识 */
  id: string
  /** 协议类型 */
  protocol: ProtocolType
  /** 是否启用 */
  enabled: boolean
  /** 配置模式：select=选择已有配置, new=新建配置, custom=一次性自定义 */
  mode: HopConfigMode
  /** 选中的配置 ID（mode=select 时使用） */
  profileId: string
  /** 一次性自定义数据（mode=custom 时使用） */
  customData?: Record<string, unknown>
}

// ==================== 网络配置文件 ====================

/** 配置文件范围 */
export type ProfileScope = 'global' | 'project'

/** SSH 配置文件 */
export interface SshProfile {
  id: string
  name: string
  scope: ProfileScope
  host: string
  port: number
  username: string
  authType: 'password' | 'key'
  password?: string
  keyPath?: string
  localPort?: number
  remoteHost?: string
  remotePort?: number
  keepAlive: number
}

/** SSL/TLS 配置文件 */
export interface SslProfile {
  id: string
  name: string
  scope: ProfileScope
  mode: 'verify-full' | 'verify-ca' | 'require'
  ca?: string
  cert?: string
  key?: string
}

/** 代理配置文件 */
export interface ProxyProfile {
  id: string
  name: string
  scope: ProfileScope
  type: 'socks5' | 'http' | 'socks4'
  host: string
  port: number
  username?: string
  password?: string
}

// ==================== 协议链 ====================

/** 最大网络跳数（SSH/Proxy） */
export const MAX_NETWORK_HOPS = 4

/** 警告网络跳数阈值（≥此数显示延迟警告） */
export const WARN_NETWORK_HOPS = 3

/** 空协议链（直连模式） */
export const EMPTY_CHAIN: ProtocolNode[] = []

/** 默认协议链模板（SSH → Proxy → SSL） */
export function createDefaultChain(): ProtocolNode[] {
  let id = 0
  return [
    { id: `hop-${++id}`, protocol: 'ssh', enabled: false, mode: 'select', profileId: '' },
    { id: `hop-${++id}`, protocol: 'proxy', enabled: false, mode: 'select', profileId: '' },
    { id: `hop-${++id}`, protocol: 'ssl', enabled: false, mode: 'select', profileId: '' },
  ]
}

// ==================== 拓扑预览 ====================

/** 拓扑节点类型 */
export type TopologyNodeKind = 'self' | 'ssh' | 'proxy' | 'ssl' | 'target'

/** 拓扑节点 */
export interface TopologyNode {
  kind: TopologyNodeKind
  label: string
  detail?: string
}

// ==================== 添加菜单选项 ====================

export interface AddHopOption {
  protocol: ProtocolType
  icon: string
  label: string
  hint: string
  disabled: boolean
}
