# 网络配置 UI 设计文档

> 版本：v3.4
> 更新：2026-05-22
> 状态：✅ 全部实施完成；v3.4 终局：侧边栏→工作台全链路 + 驱动管理 UI + 35命令 91.4% 接线
> 后端进度：✅ SSH隧道 + SSL证书 + service/cmd层 + ChainHop + process_chain + TunnelGuard 已完成；环境CRUD命令已实现
> 原型参考：[add-datasource-v5.html](file:///e:/myapps/tauirapps/RdataStation/rdata-station/prototype/add-datasource-v5.html)

---

## 一、概述

本文档定义 RdataStation 网络连接配置的前端 UI 设计方案。

**v2.0 重大变更**：从 v1.0 的"三个独立折叠面板"升级为 **动态协议链（Protocol Chain）** 模式，支持 SSH/Proxy 任意交替穿插（最多 4 跳），SSL/TLS 固定末尾。同时引入**环境策略引擎**，环境不再是颜色标签，而是安全/Schema/性能/审计/UI 五层策略集合。

### 前端技术栈

| 技术            | 版本    | 用途                                                  |
| --------------- | ------- | ----------------------------------------------------- |
| Vue 3           | 3.5.x   | 组件框架 (Composition API + `<script setup>`)         |
| TypeScript      | 6.0.x   | 类型安全                                              |
| dockview-vue    | 6.1.x   | IDE 布局基座                                          |
| naive-ui        | 2.44.x  | 组件库 (NForm/NInput/NSelect/NTabs/NButton/NModal 等) |
| lucide-vue-next | 0.460.x | 图标                                                  |
| Pinia           | 3.0.x   | 状态管理                                              |

---

## 二、设计原则

1. **naive-ui 优先**：所有表单控件使用 naive-ui 组件（NInput, NSelect, NSwitch 等）
2. **协议链为核心**：网络 Tab 采用动态协议链（拖拽排序 + 开关控制），不再用独立折叠面板
3. **环境 = 策略引擎**：环境下拉选择驱动安全/Schema/性能/审计/UI 五层策略预设填充，用户可逐字段覆盖
4. **类型安全**：TS 类型与 Rust 结构体一一对应
5. **配置独立于数据源**：网络配置管理器和环境管理器脱离具体数据源的 CRUD

### 目录结构

````
src/
├── extensions/builtin/connection/
│   ├── domain/
│   │   └── types.ts                          ← Driver / ConnectionConfig / SshConfig / ProxyConfig
│   ├── ui/
│   │   ├── components/
│   │   │   ├── AddDataSourceDialog.vue       ← 入口对话框
│   │   │   ├── AuthConfigManager.vue         ← 🆕 认证配置管理器覆盖层（数据库+SSH双Tab）
│   │   │   ├── tabs/
│   │   │   │   ├── NetworkTab.vue            ← ✅ 已实施：动态协议链 + 内联表单 + 配置管理器覆盖层
│   │   │   │   ├── GeneralTab.vue            ← ✅ 已改造：两栏认证（方法+配置）+ 文件数据库新建按钮
│   │   │   │   ├── AdvancedTab.vue           ← ✅ 已实施：环境+策略+DuckDB
│   │   │   │   └── DriverPropsTab.vue        ← 已有
│   │   ├── composables/
│   │   │   └── useNetworkProfiles.ts         ← 网络配置列表 Composable（SSH/SSL/Proxy）

---

## 三、核心数据结构（v2.0 协议链 + 环境策略）

详细的 TS 类型定义参见 [CONNECTION-METHOD-DESIGN.md 12.2.2 节](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/backend/CONNECTION-METHOD-DESIGN.md#1222-typescript-类型定义扩展)。

### 3.1 协议链数据模型

```typescript
// 前端 UI 内部状态
interface ChainItem {
  id: string                    // 唯一 ID (如 'hop-1')，非持久化
  protocol: 'ssh' | 'ssl' | 'http_proxy' | 'socks_proxy'
  enabled: boolean              // 开关状态
  mode: 'select' | 'new'       // select=选已保存配置, new=内联新建
  profileId?: string            // 选中的配置 ID
}

// 持久化到 connection_method 字段
interface ChainHopConfig {
  protocol: 'ssh' | 'ssl' | 'http_proxy' | 'socks_proxy'
  enabled: boolean
  profileId?: string
}
````

### 3.2 后端 Rust 结构体对应

| 前端 ChainItem.protocol | 后端 ChainHop 变体                  | 产生的网络节点      |
| ----------------------- | ----------------------------------- | ------------------- |
| `'ssh'`                 | `ChainHop::Ssh(SshConfig)`          | ✅ 是               |
| `'http_proxy'`          | `ChainHop::HttpProxy(ProxyConfig)`  | ✅ 是               |
| `'socks_proxy'`         | `ChainHop::SocksProxy(ProxyConfig)` | ✅ 是               |
| `'ssl'`                 | `ChainHop::Ssl(SslConfig)`          | ❌ 否（末尾流加密） |

### 3.3 协议链约束

| 约束               | 值           | 说明                              |
| ------------------ | ------------ | --------------------------------- |
| SSH/Proxy 最大跳数 | 4            | `MAX_NETWORK_HOPS` 硬上限         |
| SSL 最多           | 1            | 链末尾，添加时替换已有            |
| 每种协议最少实例   | 1            | 唯一实例时删除按钮禁用            |
| 拖拽约束           | SSL 固定末尾 | 不可拖到中间，其他不可拖到 SSL 后 |
| 3 跳警告阈值       | ≥ 3          | 黄色横幅显示延迟风险              |

### 3.4 环境策略数据（从 SQLite 加载）

```
environments 表                  environment_policies 表
┌──────────────────────┐        ┌─────────────────────────────┐
│ id: env-prod         │        │ environment_id: env-prod     │
│ name: 生产环境        │──1:N──│ policy_type: security        │
│ color: #f38ba8       │        │ policy_config: JSON string   │
│ sort_order: 4        │        │   { readonly, writeConfirm,  │
└──────────────────────┘        │     ddlConfirm, rowLimit,    │
                                │     sizeLimit, autocommit }  │
                                ├─────────────────────────────┤
                                │ policy_type: schema          │
                                │ policy_type: performance     │
                                │ policy_type: audit           │
                                │ policy_type: ui              │
                                └─────────────────────────────┘
```

## 四、NetworkTab.vue — 动态协议链（重写）

### 4.1 整体布局

```
┌─ 网络 Tab ─────────────────────────────────────────────────┐
│                                                            │
│ ℹ 提示横幅：动态协议链 — SSH/Proxy 任意交替（最多 4 跳）   │
│   SSL/TLS 固定末尾，拖拽排序。新建跟随存储范围自动保存。    │
│                                                            │
│ ┌─ 列头 ─────────────────────────────────────────────────┐ │
│ │ ≡      #     协议             配置                  启用 操作 │
│ ├─────────────────────────────────────────────────────────┤ │
│ │                                                         │ │
│ │ ┌── ChainItem (SSH) ──────────────────────────────────┐ │ │
│ │ │ ≡ │ 1 │ [🔒 SSH 隧道] │ [▼选配置] [+新建] │ [on] │ 📋✕ │ │
│ │ └──────────────────────────────────────────────────────┘ │ │
│ │ ┌── ChainItem (Proxy) ────────────────────────────────┐ │ │
│ │ │ ≡ │ 2 │ [🌐 代理]     │ [▼选配置] [+新建] │ [on] │ 📋✕ │ │
│ │ └──────────────────────────────────────────────────────┘ │ │
│ │ ┌── ChainItem (SSL) — 蓝色左边框，标注"末尾层" ────────┐ │ │
│ │ │ 🔒│ 🔐│ [🛡 SSL/TLS]  │ [▼选配置] [+新建] │ [off]│ 📋  │ │
│ │ └──────────────────────────────────────────────────────┘ │ │
│ └─────────────────────────────────────────────────────────┘ │
│                                                            │
│ ⚠ 3 跳警告：延迟 ~75ms                                    │
│ [+ 添加协议节点]  [已达上限]                                │
│                                                            │
│ ┌─ 📡 数据路径预览 ──────────────────────────────────────┐ │
│ │ [🏠本机] →SSH→ [生产跳板机] →Proxy→ [公司代理] → [🗄DB] │ │
│ └─────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────┘
```

### 4.2 核心交互

| 操作              | 实现                                                                                   |
| ----------------- | -------------------------------------------------------------------------------------- |
| **拖拽排序**      | HTML5 Drag & Drop API，`dragstart/dragover/drop/dragend` 事件                          |
| **开关切换**      | 点击 toggle div，更新 `enabled`，刷新拓扑预览                                          |
| **配置下拉选择**  | `v-for` 渲染对应协议类型的 profiles，选中后存 `profileId`                              |
| **"+ 新建"按钮**  | 切换 `mode: 'new'`，展开内联表单（SSH含端口转发字段）                                  |
| **新建保存**      | 检查当前 scope 选择 → 生成 profile ID → push 到对应 profiles 数组 → 切换回 select 模式 |
| **"📋 管理"按钮** | 打开 `NetworkProfileManager` 覆盖层，修改返回后 `renderChain()`                        |
| **"✕ 删除"**      | `countInstancesOfType(protocol) > 1` 时才可点击，否则灰色禁用                          |
| **"+ 添加"按钮**  | 弹出下拉菜单：SSH 隧道 / 代理 / SSL 加密（标末尾层）                                   |
| **拓扑预览**      | 过滤 `enabled` hops，过滤文件数据库，构建路径链                                        |

### 4.3 关键逻辑伪代码

```typescript
// 协议链核心状态
const protocolChain = ref<ChainItem[]>([
  { id: 'hop-1', protocol: 'ssh', enabled: true, mode: 'select', profileId: 'ssh-prod' },
  { id: 'hop-2', protocol: 'ssl', enabled: false, mode: 'select', profileId: undefined },
])

const MAX_HOPS = 4

function countNetworkHops(): number {
  return protocolChain.value.filter(h => h.protocol !== 'ssl').length
}

function addHop(protocol: string) {
  if (protocol === 'ssl') {
    // 替换已存在的 SSL（最多 1 个）
    const idx = protocolChain.value.findIndex(h => h.protocol === 'ssl')
    if (idx >= 0) protocolChain.value.splice(idx, 1)
    protocolChain.value.push({
      id: 'hop-' + nextId(),
      protocol: 'ssl',
      enabled: true,
      mode: 'select',
    })
  } else {
    if (countNetworkHops() >= MAX_HOPS) return
    // 插入到 SSL 之前（SSL 固定末尾）
    const sslIdx = protocolChain.value.findIndex(h => h.protocol === 'ssl')
    const item = { id: 'hop-' + nextId(), protocol, enabled: true, mode: 'select' }
    sslIdx >= 0 ? protocolChain.value.splice(sslIdx, 0, item) : protocolChain.value.push(item)
  }
  renderChain()
  updateTopology()
}

function handleDrop(srcId, tgtId) {
  // HTML5 DnD → splice 重排 → ensureSslAtEnd() → renderChain()
}

function ensureSslAtEnd() {
  const sslIdx = protocolChain.value.findIndex(h => h.protocol === 'ssl')
  if (sslIdx >= 0 && sslIdx < protocolChain.value.length - 1) {
    const [ssl] = protocolChain.value.splice(sslIdx, 1)
    protocolChain.value.push(ssl)
  }
}

function canDelete(hopId: string): boolean {
  const hop = protocolChain.value.find(h => h.id === hopId)
  return hop ? protocolChain.value.filter(h => h.protocol === hop.protocol).length > 1 : false
}
```

### 4.4 v2.1/v2.2 实施详情（已实现）

以下功能已在实际代码中实施，v2.2 完成原型 v5 对齐：

#### 4.4.1 内联表单模式（v5 对齐）

Select 模式采用 **下拉 + 新建按钮** 并行 layout：

```
[NSelect 选择已保存配置...] [+ 新建]
```

新建模式使用 `inline-form-v5` 包裹（accent 蓝色边框），三段式结构：

```html
<div class="inline-form-v5">  <!-- 蓝色边框 + bg-surface -->
  <!-- Row 1: 名称 + 范围归属 -->
  <div class="form-row">
    <div class="form-group f1">名称 <NInput></div>
    <div class="form-group f1">范围 <span class="profile-scope-badge">📝 项目</span></div>
  </div>

  <!-- Section 1: 跳板机连接 -->
  <div class="form-section-label">🔗 跳板机连接</div>
  <div class="form-row">
    <div class="form-group f2">主机 <NInput></div>
    <div class="form-group f1">端口 <NInputNumber></div>
  </div>

  <!-- Section 2: SSH 认证 -->
  <div class="form-section-label">🔐 SSH 认证</div>
  <div class="form-row">  <!-- two-col -->
    <div class="form-group f1"><NSelect 认证方法></div>
    <div class="form-group" style="flex:1.6"><NSelect 已保存认证></div>
  </div>
  <!-- 用户名 / 密码(密钥+passphrase) / 保活 -->

  <!-- Section 3: 端口转发 -->
  <div class="form-section-label">📡 端口转发</div>
  <div class="form-row">本地端口 + 远程目标 + 端口</div>
  <div class="form-hint">将远程目标通过 SSH 隧道映射到本地端口...</div>

  <!-- Actions -->
  [保存并应用] [🧪 测试连接] [取消]
</div>
```

#### 4.4.2 各协议新建表单差异

| 协议      | Section 结构                                                                                                                                     | 特殊字段               |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------- |
| **SSH**   | 名称+范围 → 🔗跳板机(Host/Port) → 🔐认证(two-col+User+Password/Key+Passphrase+Keepalive) → 📡端口转发(Local/RemoteHost/RemotePort) → Hint → 操作 | 密码/密钥切换、保活    |
| **SSL**   | 名称+范围 → Mode(NSelect) → CA+Cert → Key → 操作                                                                                                 | 证书文件路径           |
| **Proxy** | 名称+范围 → 类型(NSelect) → Host/Port → 🔐代理认证(two-col+User/Pass) → 操作                                                                     | 代理类型切换、可选认证 |

#### 4.4.3 Custom 模式（v5 对齐）

简化为一句话提示 + 关闭按钮，使用 `inline-form-v5.custom`（warning 黄色边框）：

```
⚡ 一次性自定义 — 不保存为配置文件
[关闭自定义]
```

#### 4.4.3 测试连接按钮

- **链内联表单保存行**：`🧪 测试连接` 按钮（`testChainHop(hop)`），模拟测试并弹窗显示延迟
- **配置管理器新建表单**：`🧪 测试连接` 按钮（`testPmProfile(type)`），与链内联一致

#### 4.4.4 配置管理器覆盖层

`NModal` 内嵌三 Tab（SSH / SSL / Proxy），包含：

- 已保存配置卡片列表（名称 + 详情 + 作用域徽章）
- 每张卡片：使用 / ✎编辑 / 🗑删除 三个操作按钮
- 编辑按钮将配置数据回填到新建表单（`editPmProfile(profile)`）
- 新建表单含作用域选择、协议特定字段、保存/取消/测试按钮
- 使用 `useNetworkProfiles` composable 管理数据，通过 `invoke('create_network_config')` / `invoke('delete_network_config')` 持久化

#### 4.4.5 数据流

```
useNetworkProfiles (composable)
  ├── sshProfiles / sslProfiles / proxyProfiles  ← computed refs
  ├── loadAll() → invoke('list_network_configs', { networkType })
  └── NetworkProfile { id, name, type, config, detail, origin }

NetworkTab.vue
  ├── 链列表 (chain: Hop[]) → 内联表单 (newFormData / customData)
  ├── saveNewProfile() → invoke('create_network_config')
  ├── deleteProfile() → invoke('delete_network_config')
  └── 配置管理器覆盖层 → useProfile() / editPmProfile() / deleteProfile()
```

## 五、AdvancedTab.vue — 环境策略 + 安全策略（改造）

### 5.1 新增内容

原有 AdvancedTab 包含：连接参数 + DuckDB 加速 + Schema 加载 + 编码。

**v2.0 新增**：

| 新增区域                | 组件                        | 说明                                        |
| ----------------------- | --------------------------- | ------------------------------------------- |
| **环境选择**            | `EnvironmentSelector.vue`   | 紧凑下拉，含策略行内标签                    |
| **环境策略摘要**        | 内联 Tag 行                 | 只读/写确认/DDL确认/行限/大小限/审计 等 tag |
| **安全策略面板**        | `SecurityPolicySection.vue` | 可折叠，7 个策略字段，环境覆盖指示器        |
| **DuckDB 加速（焕新）** | 内联 Card                   | 提高视觉权重，增加 benefits tag             |

### 5.2 布局

```
┌─ 高级 Tab ───────────────────────────────────────────────┐
│                                                           │
│ 🏷 环境   [● 开发环境 ▾]   [管理]                          │
│  🔒读写 · ✍写确认 · DDL确认 · 行限10000 · 限100M           │
│                                                           │
│ ⚡ 本地加速引擎 (DuckDB)                                   │
│ ┌───────────────────────────────────────────────────────┐ │
│ │ 🦆 启用 DuckDB 查询加速                      [🔘]    │ │
│ │ ── 展开 ──                                            │ │
│ │ 🚀大表分析 🔗联邦查询 📊重复报表 💾列式压缩            │ │
│ │ 同步[▼] 间隔[15]min 内存[512]MB 线程[4]              │ │
│ │ 📁 .rdata/duckdb/accel.duckdb                         │ │
│ └───────────────────────────────────────────────────────┘ │
│                                                           │
│ 🔐 安全策略 [▶] ← dev 预设 · 读写·无限制                  │
│ ┌─ 展开 ───────────────────────────────────────────────┐ │
│ │ [开关]默认只读  [开关]写确认  [开关]DDL确认            │ │
│ │ DROP [▼确认]  [开关]自动提交                          │ │
│ │ 结果行数上限[1000]  结果集上限[20]M                    │ │
│ └──────────────────────────────────────────────────────┘ │
│                                                           │
│ 连接参数                                                 │
│ 超时[30]s  查询超时[0]  保活[60]s  重连[3]               │
│                                                           │
│ Schema [▼自动加载]     编码 [▼UTF-8]                     │
└──────────────────────────────────────────────────────────┘
```

### 5.3 环境联动逻辑

```typescript
function selectEnv(envId: string) {
  currentEnvId.value = envId
  const env = environments.value.find(e => e.id === envId)
  if (!env) return

  // 1. 填充策略字段
  const p = env.policies
  polReadonly.value = p.security.readonly
  polWriteConfirm.value = p.security.writeConfirm
  polDdlConfirm.value = p.security.ddlConfirm
  polDropConfirm.value = p.security.dropConfirm
  polAutocommit.value = p.security.autocommit
  polRowLimit.value = p.security.rowLimit
  polSizeLimit.value = p.security.sizeLimit

  // 2. 填充连接参数
  connectTimeout.value = p.performance.connectTimeout
  queryTimeout.value = p.performance.queryTimeout
  heartbeat.value = p.performance.heartbeat
  maxReconnect.value = p.performance.maxReconnect

  // 3. Schema 加载
  schemaStrategy.value = p.schema.autoLoad ? 'auto' : 'manual'

  // 4. 更新指示器
  envIndicator.value = `← ${env.name} 预设`
  envIndicatorColor.value = env.color
  isOverridden.value = false
}

function onPolicyOverride() {
  isOverridden.value = true
  envIndicator.value = `⚠ 已覆盖预设`
  envIndicatorColor.value = 'var(--warning)'
}
```

## 六、覆盖层管理器

### 6.1 网络配置文件管理器 (NetworkProfileManager.vue)

```
┌─ 网络配置文件管理器 ──────────────────────────────── ✕ ─┐
│ [SSH 隧道] [SSL/TLS] [代理]                                │
├───────────────────────────────────────────────────────────┤
│ ┌─ 🌐全局 ──────────────────────────────────────────────┐ │
│ │ 生产跳板机    192.168.1.100:22 · root · 密钥 · 转发→  │ │
│ │ 测试跳板机    192.168.2.100:22 · admin · 密码         │ │
│ └───────────────────────────────────────────────────────┘ │
│ ┌─ 📝项目 ──────────────────────────────────────────────┐ │
│ │ 项目专用隧道  172.16.0.1:2222 · project-user · 密钥   │ │
│ └───────────────────────────────────────────────────────┘ │
│ [+ 新建 SSH 隧道配置]                                      │
└───────────────────────────────────────────────────────────┘
```

### 6.2 环境类型管理器 (EnvironmentManager.vue)

```
┌─ 环境类型管理 ────────────────────────────────────── ✕ ─┐
│                                                           │
│ ┌─ 内置环境 ────────────────────────────────────────────┐ │
│ │ 🟢 开发环境  内置                                      │ │
│ │   🔐读写·无限制  📊自动Schema  ⚡池10·超时0             │ │
│ ├───────────────────────────────────────────────────────┤ │
│ │ 🔴 生产环境  内置                                      │ │
│ │   🔐只读·写确认·DDL确认·DROP禁用  📋SQL审计            │ │
│ └───────────────────────────────────────────────────────┘ │
│                                                           │
│ ┌─ 自定义环境 ──────────────────────────────────────────┐ │
│ │ ⚪ 演示环境                      [✕ 删除]              │ │
│ │   🔐读写·行限500  📊按需Schema                         │ │
│ └───────────────────────────────────────────────────────┘ │
│                                                           │
│ ✦ 新建自定义环境                                         │
│ 名称[____] 图标[⚪] 颜色[■] 描述[____]                    │
│ 继承模板: [▼ 开发环境]                                   │
│ [+ 新建]                                                  │
└───────────────────────────────────────────────────────────┘
```

## 七、Stores 设计

### 7.1 environmentStore.ts

```typescript
// 环境 + 策略 Pinia Store
export const useEnvironmentStore = defineStore('environment', () => {
  const environments = ref<Environment[]>([])
  const currentEnvId = ref<string>('env-dev')
  const loading = ref(false)

  async function fetchAll() {
    environments.value = await invoke<Environment[]>('list_environments')
  }

  async function create(env: Omit<Environment, 'id'>) {
    const result = await invoke<Environment>('save_environment', { env })
    await fetchAll()
    return result
  }

  async function remove(envId: string) {
    await invoke('delete_environment', { id: envId })
    await fetchAll()
  }

  function getById(id: string): Environment | undefined {
    return environments.value.find(e => e.id === id)
  }

  return { environments, currentEnvId, loading, fetchAll, create, remove, getById }
})
```

### 7.2 networkConfigStore.ts（增强）

```typescript
export const useNetworkConfigStore = defineStore('networkConfig', () => {
  const sshProfiles = ref<NetworkProfile[]>([])
  const sslProfiles = ref<NetworkProfile[]>([])
  const proxyProfiles = ref<NetworkProfile[]>([])
  const loading = ref(false)

  async function fetchAll() {
    // 按类型分组拉取
    sshProfiles.value = await invoke<NetworkProfile[]>('list_network_configs_by_type', {
      networkType: 'ssh',
    })
    sslProfiles.value = await invoke<NetworkProfile[]>('list_network_configs_by_type', {
      networkType: 'ssl',
    })
    proxyProfiles.value = await invoke<NetworkProfile[]>('list_network_configs_by_type', {
      networkType: 'proxy',
    })
  }

  function getProfiles(protocol: string): NetworkProfile[] {
    if (protocol === 'ssh') return sshProfiles.value
    if (protocol === 'ssl') return sslProfiles.value
    return proxyProfiles.value
  }

  async function save(profile: Omit<NetworkProfile, 'id'>) {
    await invoke('save_network_config', { config: profile })
    await fetchAll()
  }

  async function remove(id: string) {
    await invoke('delete_network_config', { id })
    await fetchAll()
  }

  return { sshProfiles, sslProfiles, proxyProfiles, loading, fetchAll, getProfiles, save, remove }
})
```

## 八、Tauri Commands 前端需要调用的接口

### 8.1 已实现

| Command            | 说明                                                  |
| ------------------ | ----------------------------------------------------- |
| `connect_database` | 创建连接（已支持 network_config_id / environment_id） |

### 8.2 待实现（v0.6.0 后端新增）

| Command                        | 说明                         | Rust 调用                         |
| ------------------------------ | ---------------------------- | --------------------------------- |
| `list_environments`            | 列出所有环境（含策略）       | `env_store.list_all()`            |
| `save_environment`             | 保存/更新环境                | `env_store.create()` / `update()` |
| `delete_environment`           | 删除环境（内置不可删）       | `env_store.delete()`              |
| `list_network_configs_by_type` | 按 network_type + scope 过滤 | `network_store.list_by_type()`    |
| `save_network_config`          | 保存网络配置                 | `network_store.create()`          |
| `delete_network_config`        | 删除网络配置                 | `network_store.delete()`          |
| `test_network_config`          | 测试网络配置连通性           | 已有，需支持 chain                |

## 九、实施步骤

| 阶段                    | 内容                                                                                              | 状态      |
| ----------------------- | ------------------------------------------------------------------------------------------------- | --------- |
| **NetworkTab 协议链**   | 动态协议链 + 内联表单 + 拖拽 + 拓扑预览                                                           | ✅ 已完成 |
| **两栏认证布局**        | SSH 认证方法 + 已保存配置选择                                                                     | ✅ 已完成 |
| **测试连接按钮**        | 链内联 + 配置管理器新建表单                                                                       | ✅ 已完成 |
| **配置管理器**          | NModal + 三Tab + CRUD + 编辑回填                                                                  | ✅ 已完成 |
| **GeneralTab 改造**     | 数据库认证两栏 + 文件DB新建按钮                                                                   | ✅ 已完成 |
| **AuthConfigManager**   | 认证配置管理器覆盖层                                                                              | ✅ 已完成 |
| **后端增量**            | 环境 CRUD + Seed SQL + IPC Commands                                                               | ✅ 已完成 |
| **AdvancedTab 改造**    | 环境选择 + 策略 + DuckDB 焕新（已拆分为独立子组件）                                               | ✅ 已完成 |
| **管理面板**            | EnvironmentManager / SecurityPolicySection / EnvironmentSelector / DuckDBAccelSection（独立组件） | ✅ 已完成 |
| **集成联调**            | AddDataSourceDialog 改造 + auth_method/environment_id 透传                                        | ✅ 已完成 |
| **Stores + Composable** | environmentStore.ts + networkConfigStore.ts + useAddDataSource.ts                                 | ✅ 已完成 |
| **快照 IPC**            | snapshot_global_env/network/auth                                                                  | ✅ 已完成 |
| **链校验**              | validate_connection_config (后端 7 步校验)                                                        | ✅ 已完成 |

### 遗留问题

| #   | 问题                                                                   | 严重度 |
| --- | ---------------------------------------------------------------------- | ------ | --------- |
| L1  | Composable/Store 已创建但 AddDataSourceDialog / NetworkTab 未消费      | 🔴     |
| L2  | EnvironmentManager 类型不匹配（summary 字段不存在于 Environment 接口） | 🔴     |
| L3  | `isFileDb` 死代码 — `useAddDataSource.ts` 永远返回 false               | 🟡     |
| L4  | NetworkTab 硬编码 demo 认证配置 (`chainSshAuthCfgOpts`)                | 🟡     |
| L5  | Custom 模式空壳 — 仅提示横幅无实际表单                                 | 🟡     |
| L6  | DataSourceHeader 未独立 — 内联在 AddDataSourceDialog                   | 🟢     | ✅ 已修复 |

#### v2.5 修复记录 (2026-05-22)

| #      | 问题                                                                                                        | 严重度 | 状态                                                                                  |
| ------ | ----------------------------------------------------------------------------------------------------------- | ------ | ------------------------------------------------------------------------------------- |
| **G1** | **网络配置编辑创建重复** — `handleCreate*Profile` 编辑时忽略 `profile.id`，始终调用 `create_network_config` | 🔴     | ✅ 已修复 — 提取 `buildNetworkCfg()` 统一函数，编辑时调用 `update_network_config`     |
| **G2** | **认证配置编辑用 create 代替 update** — `saveNewCfg()` 始终调用 `create_auth_config`                        | 🟡     | ✅ 已修复 — 根据 `editingId` 切换 `update_auth_config` / `create_auth_config`         |
| **G3** | **环境管理器缺少编辑功能** — 只能创建/删除，无法修改已有自定义环境名称/图标/颜色                            | 🟡     | ✅ 已修复 — `EnvironmentManager` + `AdvancedTab` 支持编辑 → 调用 `update_environment` |

#### v2.6 修复记录 (2026-05-22)

| #      | 问题                                                                       | 严重度 | 状态                                                                       |
| ------ | -------------------------------------------------------------------------- | ------ | -------------------------------------------------------------------------- |
| **D1** | **环境列表不区分来源** — EnvironmentManager 混显 G*/P*/GP\_，无 scope 标识 | 🟡     | ✅ 已修复 — 新增 sourceLabel/sourceKind helper + 🌐全局/📁项目/📸快照 标签 |
| **D2** | **loadEnvironments 无 scope 过滤** — 不区分 global/project                 | 🟡     | ✅ 已修复 — AdvancedTab 接收 scope prop，按 ID 前缀过滤                    |
| **D3** | **项目引用全局环境无快照** — 选择 G\_ 环境时未自动 snapshot_global_env     | 🔴     | ✅ 已修复 — onEnvChange 检测 project+G* → snapshot → GP* + 快照提示        |

#### v2.7 修复记录 (2026-05-22)

| #       | 问题                                                                                                             | 严重度 | 状态                                                                                                 |
| ------- | ---------------------------------------------------------------------------------------------------------------- | ------ | ---------------------------------------------------------------------------------------------------- |
| **E1**  | **networkConfigStore.save() 参数名不匹配** — `{ config: profile }` ≠ 后端期望 `{ nc }`                           | 🔴     | ✅ 已修复 → `{ nc: profile }`                                                                        |
| **E2**  | **snapshot*global*\* 三命令缺 project_path 参数** — AdvancedTab / useAddDataSource 三处调用只传 globalEnvId      | 🔴     | ✅ 已修复 — 补全 projectPath 参数                                                                    |
| **E3**  | **snapshot*global*\* 返回类型不匹配** — 前端 `invoke<string>()` 但后端返回 `SnapshotResult { snapshot_id, ... }` | 🔴     | ✅ 已修复 — `invoke<{ snapshot_id: string }>()` + `.snapshot_id`                                     |
| **E4**  | **doSave 缺认证/网络快照** — 仅环境有快照，认证和网络引用 G* 时无 GP* 隔离                                       | 🔴     | ✅ 已修复 — doSave 前检测 authConfigId/networkConfigId 前缀触发快照                                  |
| **E5**  | **AuthConfigManager.deleteCfg() 未二分** — scope=project 时仍调全局 delete_auth_config                           | 🟡     | ✅ v2.8 — project_delete_auth_config({ id, projectPath })                                            |
| **E6**  | **AdvancedTab.handleCreateEnv() 未二分** — scope=project 时仍调全局 create/update_environment                    | 🟡     | ✅ v2.8 — project_create/update_environment 扁平参数适配                                             |
| **E7**  | **AdvancedTab.handleDeleteEnv() 未二分** — scope=project 时仍调全局 delete_environment                           | 🟡     | ✅ v2.8 — project_delete_environment({ id, projectPath })                                            |
| **E8**  | **AdvancedTab.loadEnvironments() 未二分** — scope=project 时仍调全局 list_environments                           | 🟡     | ✅ v2.8 — project_list_environments({ projectPath })                                                 |
| **E9**  | **环境策略未持久化** — AdvancedTab 五类策略仅本地状态，切换环境即丢失                                            | 🟡     | ✅ v2.9 — loadPoliciesForEnv() + debounceSavePolicy(800ms) → `list/create/update_environment_policy` |
| **E10** | **侧边栏缺重测按钮** — 已保存连接无法从侧边栏快速验证连通性                                                      | 🟡     | ✅ v3.0 — DataSourceSidebar 每行添加刷新按钮 → `test_connection({ dbType, url })`                    |
| **E11** | **侧边栏连接点击无响应** — 已保存连接无法通过点击打开数据库                                                      | 🔴     | ✅ v3.2 — `openSavedConnection()` → `connect_database` → `switch_connection` → dispatch `NewQuery`   |
| **E12** | **ProjectConnection 类型重复** — domain/types.ts 与 types/connection.ts 两套定义                                 | 🟡     | ✅ v3.2 — 删除 domain 重复，统一到 types/connection.ts                                               |
| **E13** | **project_update_auth_config 缺失** — 后端无项目级认证更新命令                                                   | 🟡     | ✅ v3.2 — project_db.rs + command + lib.rs 注册                                                      |
| **E14** | **AuthConfigManager delete+create workaround** — 编辑用删除重建代替更新                                          | 🟡     | ✅ v3.2 — saveNewCfg() 直接调用 project_update_auth_config                                           |
| **E15** | **delete_environment_policy 未接线** — 删环境时策略成为孤儿数据                                                  | 🟢     | ✅ v3.2 — handleDeleteEnv() 级联删除关联策略                                                         |
| **E16** | **project\_\* 环境策略族不通** — scope=project 时策略加载/保存未切换                                             | 🟢     | ✅ v3.3 — loadPoliciesForEnv + savePolicyForEnv 二分 scope                                           |
| **E17** | **_*store*_ 双轨冗余** — save/get/delete*project_store_connection 与 project*\* 并存                             | 📋     | ✅ v3.3 — project-connection.ts + connection.ts + store 全量单轨化                                   |
| **E18** | **连接操作命令未接线** — convert_connection_type / detect_global_connections_in_project                          | 🟢     | ✅ v3.3 — connection.ts 新增 connectionService 服务层封装                                            |
| **E19** | **侧边栏→工作台链路断裂** — NewQuery detail 载荷被 handleWorkbenchNewQuery 忽略                                  | 🔴     | ✅ v3.4 — WorkbenchView 读 detail.connectionId → EditorManager.openNewQuery                          |
| **E20** | **驱动管理 API 未接线** — get_driver_detail / install_driver / list_driver_files                                 | 🟢     | ✅ v3.4 — useDriverRegistry 新增三大 API + DataSourceSidebar 驱动管理区域                            |
| **E21** | **驱动管理无 UI** — 无法查看驱动状态或安装外部驱动                                                               | 🟢     | ✅ v3.4 — 侧边栏底部 driversWithStatus + 安装按钮                                                    |

## 十、参考

- [后端实施文档 CONNECTION-METHOD-DESIGN.md 第十二章](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/backend/CONNECTION-METHOD-DESIGN.md#十二v060-完整实施方案基于-add-datasource-v5-原型)
- [v5 原型 add-datasource-v5.html](file:///e:/myapps/tauirapps/RdataStation/rdata-station/prototype/add-datasource-v5.html)
