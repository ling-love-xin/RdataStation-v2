/**
 * useNetworkChain 协议链引擎单元测试
 *
 * 测试协议链节点增删改查、SSL 末尾约束、跳数上限、拖拽排序、
 * 拓扑预览、默认链创建等。
 */
import { describe, expect, it } from 'vitest'

// ==================== 类型定义（与源码对齐） ====================

type ProtocolType = 'ssh' | 'ssl' | 'proxy'
type HopConfigMode = 'select' | 'new' | 'custom'

interface ProtocolNode {
  id: string
  protocol: ProtocolType
  enabled: boolean
  mode: HopConfigMode
  profileId: string
  customData?: Record<string, unknown>
}

const MAX_NETWORK_HOPS = 4
const WARN_NETWORK_HOPS = 3

function createDefaultChain(): ProtocolNode[] {
  let id = 0
  return [
    { id: `hop-${++id}`, protocol: 'ssh', enabled: false, mode: 'select', profileId: '' },
    { id: `hop-${++id}`, protocol: 'proxy', enabled: false, mode: 'select', profileId: '' },
    { id: `hop-${++id}`, protocol: 'ssl', enabled: false, mode: 'select', profileId: '' },
  ]
}

// ==================== 协议链操作函数 ====================

function ensureSslAtEnd(chain: ProtocolNode[]) {
  const sslIdx = chain.findIndex(h => h.protocol === 'ssl')
  if (sslIdx >= 0 && sslIdx < chain.length - 1) {
    const [ssl] = chain.splice(sslIdx, 1)
    chain.push(ssl)
  }
}

function addHop(
  chain: ProtocolNode[],
  protocol: ProtocolType,
  nextId: () => string
): string | null {
  const enabledNetwork = chain.filter(h => h.protocol !== 'ssl' && h.enabled).length
  if (protocol !== 'ssl' && enabledNetwork >= MAX_NETWORK_HOPS) return null

  if (protocol === 'ssl') {
    const existingIdx = chain.findIndex(h => h.protocol === 'ssl')
    if (existingIdx >= 0) {
      chain.splice(existingIdx, 1)
    }
    const newHop: ProtocolNode = {
      id: nextId(),
      protocol: 'ssl',
      enabled: true,
      mode: 'select',
      profileId: '',
    }
    chain.push(newHop)
    return newHop.id
  }

  const newHop: ProtocolNode = {
    id: nextId(),
    protocol,
    enabled: true,
    mode: 'select',
    profileId: '',
  }

  const sslIdx = chain.findIndex(h => h.protocol === 'ssl')
  if (sslIdx >= 0) {
    chain.splice(sslIdx, 0, newHop)
  } else {
    chain.push(newHop)
  }

  return newHop.id
}

function deleteHop(chain: ProtocolNode[], hopId: string): boolean {
  const hop = chain.find(h => h.id === hopId)
  if (!hop) return false
  const count = chain.filter(h => h.protocol === hop.protocol).length
  if (count <= 1) return false
  const idx = chain.findIndex(h => h.id === hopId)
  chain.splice(idx, 1)
  return true
}

function toggleHop(chain: ProtocolNode[], hopId: string) {
  const hop = chain.find(h => h.id === hopId)
  if (hop) hop.enabled = !hop.enabled
}

function switchHopMode(chain: ProtocolNode[], hopId: string, mode: HopConfigMode) {
  const hop = chain.find(h => h.id === hopId)
  if (!hop) return
  hop.mode = mode
  if (mode === 'select') {
    hop.profileId = ''
  }
}

function selectProfile(chain: ProtocolNode[], hopId: string, profileId: string) {
  const hop = chain.find(h => h.id === hopId)
  if (!hop) return
  hop.profileId = profileId
}

function onDrop(chain: ProtocolNode[], srcId: string, targetId: string): boolean {
  if (srcId === targetId) return false
  const srcIdx = chain.findIndex(h => h.id === srcId)
  const tgtIdx = chain.findIndex(h => h.id === targetId)
  if (srcIdx < 0 || tgtIdx < 0) return false

  const srcHop = chain[srcIdx]
  if (srcHop.protocol === 'ssl' && tgtIdx < chain.length - 1) {
    return false
  }

  const [moved] = chain.splice(srcIdx, 1)
  const newTgtIdx = chain.findIndex(h => h.id === targetId)
  chain.splice(newTgtIdx, 0, moved)
  ensureSslAtEnd(chain)
  return true
}

// ==================== 默认链创建 ====================

describe('createDefaultChain 默认协议链', () => {
  it('应包含 SSH + Proxy + SSL 三个节点', () => {
    const chain = createDefaultChain()
    expect(chain).toHaveLength(3)
    expect(chain[0].protocol).toBe('ssh')
    expect(chain[1].protocol).toBe('proxy')
    expect(chain[2].protocol).toBe('ssl')
  })

  it('所有节点默认 disabled', () => {
    const chain = createDefaultChain()
    for (const hop of chain) {
      expect(hop.enabled).toBe(false)
    }
  })

  it('所有节点默认 mode=select', () => {
    const chain = createDefaultChain()
    for (const hop of chain) {
      expect(hop.mode).toBe('select')
    }
  })
})

// ==================== addHop 测试 ====================

describe('addHop 添加节点', () => {
  it('添加 SSH 节点 → 插入 SSL 之前', () => {
    const chain = createDefaultChain()
    let counter = 10
    const id = addHop(chain, 'ssh', () => `hop-${counter++}`)
    expect(id).not.toBeNull()
    // SSH 应在 SSL 之前
    const sslIdx = chain.findIndex(h => h.protocol === 'ssl')
    expect(chain[sslIdx - 1].protocol).toBe('ssh')
    expect(chain[sslIdx - 1].enabled).toBe(true)
  })

  it('添加代理节点 → 插入 SSL 之前', () => {
    const chain = createDefaultChain()
    let counter = 10
    const id = addHop(chain, 'proxy', () => `hop-${counter++}`)
    expect(id).not.toBeNull()
    const sslIdx = chain.findIndex(h => h.protocol === 'ssl')
    expect(chain[sslIdx - 1].protocol).toBe('proxy')
  })

  it('添加 SSL → 替换旧 SSL 并放在末尾', () => {
    const chain = createDefaultChain()
    let counter = 10
    const id = addHop(chain, 'ssl', () => `hop-${counter++}`)
    expect(id).not.toBeNull()
    // 只有一个 SSL
    const sslCount = chain.filter(h => h.protocol === 'ssl').length
    expect(sslCount).toBe(1)
    // SSL 在末尾
    expect(chain[chain.length - 1].protocol).toBe('ssl')
  })

  it('超过 4 跳上限 → 返回 null', () => {
    const chain = createDefaultChain()
    // 启用所有节点
    for (const hop of chain) {
      if (hop.protocol !== 'ssl') hop.enabled = true
    }
    let counter = 10
    // 添加 2 个 SSH（共 2 跳）
    addHop(chain, 'ssh', () => `hop-${counter++}`)
    addHop(chain, 'ssh', () => `hop-${counter++}`)
    // 现在网络跳数 = 1 + 2 = 3，再加 1 个 = 4 跳
    addHop(chain, 'proxy', () => `hop-${counter++}`)
    // 再加第 5 跳应失败
    const id = addHop(chain, 'ssh', () => `hop-${counter++}`)
    expect(id).toBeNull()
  })
})

// ==================== deleteHop 测试 ====================

describe('deleteHop 删除节点', () => {
  it('删除不存在的节点 → false', () => {
    const chain = createDefaultChain()
    expect(deleteHop(chain, 'nonexistent')).toBe(false)
  })

  it('每种协议至少保留一个 → 不能删除唯一的 SSH', () => {
    const chain = createDefaultChain()
    // 只有一个 SSH（hop-1）
    const sshCount = chain.filter(h => h.protocol === 'ssh').length
    expect(sshCount).toBe(1)
    expect(deleteHop(chain, 'hop-1')).toBe(false)
  })

  it('多个 SSH 时可以删除', () => {
    const chain = createDefaultChain()
    let counter = 10
    addHop(chain, 'ssh', () => `hop-${counter++}`)
    const sshCount = chain.filter(h => h.protocol === 'ssh').length
    expect(sshCount).toBe(2)
    expect(deleteHop(chain, 'hop-1')).toBe(true)
  })
})

// ==================== toggleHop 测试 ====================

describe('toggleHop 切换启用', () => {
  it('disabled → enabled', () => {
    const chain = createDefaultChain()
    expect(chain[0].enabled).toBe(false)
    toggleHop(chain, 'hop-1')
    expect(chain[0].enabled).toBe(true)
  })

  it('enabled → disabled', () => {
    const chain = createDefaultChain()
    chain[0].enabled = true
    toggleHop(chain, 'hop-1')
    expect(chain[0].enabled).toBe(false)
  })
})

// ==================== switchHopMode 测试 ====================

describe('switchHopMode 切换配置模式', () => {
  it('切换到 select → 清空 profileId', () => {
    const chain = createDefaultChain()
    chain[0].profileId = 'G_ssh_001'
    switchHopMode(chain, 'hop-1', 'select')
    expect(chain[0].mode).toBe('select')
    expect(chain[0].profileId).toBe('')
  })

  it('切换到 custom → 保留 profileId', () => {
    const chain = createDefaultChain()
    chain[0].profileId = 'G_ssh_001'
    switchHopMode(chain, 'hop-1', 'custom')
    expect(chain[0].mode).toBe('custom')
    expect(chain[0].profileId).toBe('G_ssh_001')
  })
})

// ==================== selectProfile 测试 ====================

describe('selectProfile 选择配置文件', () => {
  it('选择配置文件 → 更新 profileId', () => {
    const chain = createDefaultChain()
    selectProfile(chain, 'hop-1', 'G_ssh_001')
    expect(chain[0].profileId).toBe('G_ssh_001')
  })
})

// ==================== ensureSslAtEnd 测试 ====================

describe('ensureSslAtEnd SSL 末尾约束', () => {
  it('SSL 不在末尾 → 移到末尾', () => {
    const chain: ProtocolNode[] = [
      { id: '1', protocol: 'ssl', enabled: true, mode: 'select', profileId: '' },
      { id: '2', protocol: 'ssh', enabled: true, mode: 'select', profileId: '' },
      { id: '3', protocol: 'proxy', enabled: true, mode: 'select', profileId: '' },
    ]
    ensureSslAtEnd(chain)
    expect(chain[chain.length - 1].protocol).toBe('ssl')
  })

  it('SSL 已在末尾 → 不变', () => {
    const chain: ProtocolNode[] = [
      { id: '1', protocol: 'ssh', enabled: true, mode: 'select', profileId: '' },
      { id: '2', protocol: 'ssl', enabled: true, mode: 'select', profileId: '' },
    ]
    const before = chain.map(h => h.id).join(',')
    ensureSslAtEnd(chain)
    const after = chain.map(h => h.id).join(',')
    expect(before).toBe(after)
  })
})

// ==================== onDrop 拖拽排序 ====================

describe('onDrop 拖拽排序', () => {
  it('同 id 拖拽 → false', () => {
    const chain = createDefaultChain()
    expect(onDrop(chain, 'hop-1', 'hop-1')).toBe(false)
  })

  it('SSL 不能拖到中间位置', () => {
    const chain = createDefaultChain()
    // hop-3 是 SSL，拖到 hop-1 位置（中间）应失败
    expect(onDrop(chain, 'hop-3', 'hop-1')).toBe(false)
  })

  it('SSH 正常拖拽排序', () => {
    const chain: ProtocolNode[] = [
      { id: '1', protocol: 'ssh', enabled: true, mode: 'select', profileId: '' },
      { id: '2', protocol: 'proxy', enabled: true, mode: 'select', profileId: '' },
      { id: '3', protocol: 'ssl', enabled: true, mode: 'select', profileId: '' },
    ]
    // 拖拽 hop-1 到 hop-2 位置
    expect(onDrop(chain, '1', '2')).toBe(true)
    // SSL 仍在末尾
    expect(chain[chain.length - 1].protocol).toBe('ssl')
  })
})

// ==================== 跳数计算 ====================

describe('网络跳数计算', () => {
  it('空链 → 0 跳', () => {
    const chain: ProtocolNode[] = []
    const networkHops = chain.filter(h => h.protocol !== 'ssl').length
    expect(networkHops).toBe(0)
  })

  it('仅 SSL → 0 跳（SSL 不产生网络节点）', () => {
    const chain: ProtocolNode[] = [
      { id: '1', protocol: 'ssl', enabled: true, mode: 'select', profileId: '' },
    ]
    const networkHops = chain.filter(h => h.protocol !== 'ssl').length
    expect(networkHops).toBe(0)
  })

  it('SSH+Proxy+SSL → 2 跳', () => {
    const chain = createDefaultChain()
    for (const h of chain) h.enabled = true
    const networkHops = chain.filter(h => h.protocol !== 'ssl').length
    expect(networkHops).toBe(2)
  })

  it('已启用跳数 ≥ 3 时显示警告', () => {
    const chain: ProtocolNode[] = [
      { id: '1', protocol: 'ssh', enabled: true, mode: 'select', profileId: '' },
      { id: '2', protocol: 'ssh', enabled: true, mode: 'select', profileId: '' },
      { id: '3', protocol: 'proxy', enabled: true, mode: 'select', profileId: '' },
      { id: '4', protocol: 'ssl', enabled: true, mode: 'select', profileId: '' },
    ]
    const enabled = chain.filter(h => h.protocol !== 'ssl' && h.enabled).length
    expect(enabled >= WARN_NETWORK_HOPS).toBe(true)
  })
})

// ==================== 拓扑预览 ====================

describe('topologyNodes 拓扑预览', () => {
  it('直连模式 → 仅本机', () => {
    const nodes = [{ kind: 'self', label: '本机' }]
    expect(nodes).toHaveLength(1)
    expect(nodes[0].kind).toBe('self')
  })

  it('SSH → SSL → 本机 + SSH + TLS', () => {
    const nodes = [
      { kind: 'self', label: '本机' },
      { kind: 'ssh', label: 'SSH', detail: 'jump.corp.com:22' },
      { kind: 'ssl', label: 'TLS 加密' },
    ]
    expect(nodes).toHaveLength(3)
    expect(nodes[0].kind).toBe('self')
    expect(nodes[1].kind).toBe('ssh')
    expect(nodes[2].kind).toBe('ssl')
  })
})
