<template>
  <div v-if="visible" class="config-manager-overlay" @click.self="close">
    <div class="config-manager-dialog">
      <!-- Header -->
      <div class="cm-header">
        <h3 class="cm-title">
          <Boxes :size="16" />
          {{ t('networkTab.profileManager.title') }}
        </h3>
        <button class="cm-close-btn" :title="String(t('common.close'))" @click="close">
          <X :size="16" />
        </button>
      </div>

      <!-- Tabs -->
      <div class="cm-tabs">
        <button class="cm-tab" :class="{ active: activeTab === 'ssh' }" @click="switchTab('ssh')">
          🔐 {{ t('networkTab.profileManager.ssh') }}
          <span class="cm-badge">{{ sshProfiles.length }}</span>
        </button>
        <button class="cm-tab" :class="{ active: activeTab === 'ssl' }" @click="switchTab('ssl')">
          🔒 {{ t('networkTab.profileManager.ssl') }}
          <span class="cm-badge">{{ sslProfiles.length }}</span>
        </button>
        <button
          class="cm-tab"
          :class="{ active: activeTab === 'proxy' }"
          @click="switchTab('proxy')"
        >
          🌐 {{ t('networkTab.profileManager.proxy') }}
          <span class="cm-badge">{{ proxyProfiles.length }}</span>
        </button>
      </div>

      <!-- Content -->
      <div class="cm-content">
        <!-- ===== SSH Tab ===== -->
        <template v-if="activeTab === 'ssh'">
          <div v-if="sshGlobal.length > 0" class="cm-group">
            <div class="cm-group-label">🌐 全局配置</div>
            <div v-for="p in sshGlobal" :key="p.id" class="cm-card">
              <div class="cm-card-info">
                <span class="cm-card-name">{{ p.name }}</span>
                <span class="cm-card-detail">{{ p.detail }}</span>
              </div>
              <div class="cm-card-actions">
                <button class="cm-action-btn edit" title="编辑" @click="editSsh(p)"
                  ><Pencil :size="13"
                /></button>
                <button class="cm-action-btn" title="删除" @click="onDeleteSsh(p.id)"
                  ><Trash2 :size="13"
                /></button>
              </div>
            </div>
          </div>
          <div v-if="sshProject.length > 0" class="cm-group">
            <div class="cm-group-label">📝 项目配置</div>
            <div v-for="p in sshProject" :key="p.id" class="cm-card">
              <div class="cm-card-info">
                <span class="cm-card-name">{{ p.name }}</span>
                <span class="cm-card-detail">{{ p.detail }}</span>
              </div>
              <div class="cm-card-actions">
                <button class="cm-action-btn edit" title="编辑" @click="editSsh(p)"
                  ><Pencil :size="13"
                /></button>
                <button class="cm-action-btn" title="删除" @click="onDeleteSsh(p.id)"
                  ><Trash2 :size="13"
                /></button>
              </div>
            </div>
          </div>

          <!-- New SSH form -->
          <template v-if="showSshForm">
            <div class="cm-create-form">
              <div class="cm-form-title">{{
                editingSshId ? '编辑 SSH 隧道配置' : '+ 新建 SSH 隧道配置'
              }}</div>

              <div class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">配置名称</label>
                  <input v-model="sshForm.name" class="cm-input" placeholder="例如：生产跳板机" />
                </div>
                <div class="cm-form-group f1">
                  <label class="cm-form-label">作用域</label>
                  <select v-model="sshForm.scope" class="cm-select">
                    <option value="global">🌐 全局</option>
                    <option value="project">📝 项目</option>
                  </select>
                </div>
              </div>

              <div class="cm-form-section">🔗 跳板机连接</div>
              <div class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">主机</label>
                  <input v-model="sshForm.host" class="cm-input" placeholder="192.168.1.100" />
                </div>
                <div class="cm-form-group f1">
                  <label class="cm-form-label">端口</label>
                  <input
                    v-model.number="sshForm.port"
                    type="number"
                    class="cm-input"
                    placeholder="22"
                  />
                </div>
              </div>

              <div class="cm-form-section">🔐 SSH 认证</div>
              <div class="cm-form-row">
                <div class="cm-form-group f1">
                  <label class="cm-form-label">认证方式</label>
                  <select v-model="sshForm.authMethod" class="cm-select">
                    <option value="password">密码</option>
                    <option value="key">密钥</option>
                  </select>
                </div>
                <div class="cm-form-group f1">
                  <label class="cm-form-label">用户名</label>
                  <input v-model="sshForm.username" class="cm-input" placeholder="root" />
                </div>
              </div>
              <div v-if="sshForm.authMethod === 'password'" class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">密码</label>
                  <input
                    v-model="sshForm.password"
                    type="password"
                    class="cm-input"
                    placeholder="输入 SSH 密码"
                  />
                </div>
              </div>
              <div v-else class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">私钥路径</label>
                  <input v-model="sshForm.keyPath" class="cm-input" placeholder="~/.ssh/id_rsa" />
                </div>
              </div>
              <div v-if="sshForm.authMethod === 'key'" class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">私钥密码 (可选)</label>
                  <input
                    v-model="sshForm.passphrase"
                    type="password"
                    class="cm-input"
                    placeholder="私钥保护密码"
                  />
                </div>
              </div>
              <div class="cm-form-row">
                <div class="cm-form-group f1">
                  <label class="cm-form-label">保活间隔 (秒)</label>
                  <input
                    v-model.number="sshForm.keepalive"
                    type="number"
                    class="cm-input"
                    placeholder="60"
                  />
                </div>
              </div>

              <div class="cm-form-section">📡 端口转发 (可选)</div>
              <div class="cm-form-row">
                <div class="cm-form-group f1">
                  <label class="cm-form-label">本地端口</label>
                  <input
                    v-model.number="sshForm.localPort"
                    type="number"
                    class="cm-input"
                    placeholder="自动分配"
                  />
                </div>
                <div class="cm-form-group f1">
                  <label class="cm-form-label">远程主机</label>
                  <input v-model="sshForm.remoteHost" class="cm-input" placeholder="db.internal" />
                </div>
                <div class="cm-form-group f1">
                  <label class="cm-form-label">远程端口</label>
                  <input
                    v-model.number="sshForm.remotePort"
                    type="number"
                    class="cm-input"
                    placeholder="3306"
                  />
                </div>
              </div>

              <div class="cm-form-hint"
                >将远程目标通过 SSH 隧道映射到本地端口，实现安全穿透访问</div
              >

              <div class="cm-form-actions">
                <button class="cm-btn secondary" @click="cancelSshForm">取消</button>
                <button class="cm-btn test" @click="testSshForm">🧪 测试连接</button>
                <button class="cm-btn primary" @click="saveSshForm">{{
                  editingSshId ? '保存修改' : '保存配置'
                }}</button>
              </div>
            </div>
          </template>

          <!-- Add button -->
          <button v-if="!showSshForm" class="cm-add-btn" @click="showSshForm = true">
            <Plus :size="14" /> 新建 SSH 隧道配置
          </button>
        </template>

        <!-- ===== SSL Tab ===== -->
        <template v-if="activeTab === 'ssl'">
          <div v-if="sslGlobal.length > 0" class="cm-group">
            <div class="cm-group-label">🌐 全局配置</div>
            <div v-for="p in sslGlobal" :key="p.id" class="cm-card">
              <div class="cm-card-info">
                <span class="cm-card-name">{{ p.name }}</span>
                <span class="cm-card-detail"
                  >模式: {{ cfgField(p, 'mode', 'require')
                  }}<template v-if="cfgField(p, 'ca')">
                    · CA: {{ basename(String(cfgField(p, 'ca', ''))) }}</template
                  ></span
                >
              </div>
              <div class="cm-card-actions">
                <button class="cm-action-btn edit" title="编辑" @click="editSsl(p)"
                  ><Pencil :size="13"
                /></button>
                <button class="cm-action-btn" title="删除" @click="onDeleteSsl(p.id)"
                  ><Trash2 :size="13"
                /></button>
              </div>
            </div>
          </div>
          <div v-if="sslProject.length > 0" class="cm-group">
            <div class="cm-group-label">📝 项目配置</div>
            <div v-for="p in sslProject" :key="p.id" class="cm-card">
              <div class="cm-card-info">
                <span class="cm-card-name">{{ p.name }}</span>
                <span class="cm-card-detail"
                  >模式: {{ cfgField(p, 'mode', 'require')
                  }}<template v-if="cfgField(p, 'ca')">
                    · CA: {{ basename(String(cfgField(p, 'ca', ''))) }}</template
                  ></span
                >
              </div>
              <div class="cm-card-actions">
                <button class="cm-action-btn edit" title="编辑" @click="editSsl(p)"
                  ><Pencil :size="13"
                /></button>
                <button class="cm-action-btn" title="删除" @click="onDeleteSsl(p.id)"
                  ><Trash2 :size="13"
                /></button>
              </div>
            </div>
          </div>

          <template v-if="showSslForm">
            <div class="cm-create-form">
              <div class="cm-form-title">{{
                editingSslId ? '编辑 SSL/TLS 配置' : '+ 新建 SSL/TLS 配置'
              }}</div>
              <div class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">配置名称</label>
                  <input v-model="sslForm.name" class="cm-input" placeholder="例如：生产 SSL" />
                </div>
                <div class="cm-form-group f1">
                  <label class="cm-form-label">作用域</label>
                  <select v-model="sslForm.scope" class="cm-select">
                    <option value="global">🌐 全局</option>
                    <option value="project">📝 项目</option>
                  </select>
                </div>
              </div>
              <div class="cm-form-row">
                <div class="cm-form-group f1">
                  <label class="cm-form-label">SSL 模式</label>
                  <select v-model="sslForm.mode" class="cm-select">
                    <option value="disable">禁用</option>
                    <option value="prefer">优先</option>
                    <option value="require">要求</option>
                    <option value="verify-ca">验证 CA</option>
                    <option value="verify-full">完全验证</option>
                  </select>
                </div>
              </div>
              <div class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">CA 证书路径</label>
                  <input v-model="sslForm.ca" class="cm-input" placeholder="/path/to/ca.pem" />
                </div>
              </div>
              <div class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">客户端证书路径</label>
                  <input
                    v-model="sslForm.clientCert"
                    class="cm-input"
                    placeholder="/path/to/client-cert.pem"
                  />
                </div>
              </div>
              <div class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">客户端密钥路径</label>
                  <input
                    v-model="sslForm.clientKey"
                    class="cm-input"
                    placeholder="/path/to/client-key.pem"
                  />
                </div>
              </div>
              <div class="cm-form-row">
                <div class="cm-form-group f1">
                  <label class="cm-form-label">主机名覆盖 (可选)</label>
                  <input
                    v-model="sslForm.hostnameOverride"
                    class="cm-input"
                    placeholder="db.example.com"
                  />
                </div>
              </div>
              <div class="cm-form-actions">
                <button class="cm-btn secondary" @click="cancelSslForm">取消</button>
                <button class="cm-btn test" @click="testSslForm">🧪 测试连接</button>
                <button class="cm-btn primary" @click="saveSslForm">{{
                  editingSslId ? '保存修改' : '保存配置'
                }}</button>
              </div>
            </div>
          </template>

          <button v-if="!showSslForm" class="cm-add-btn" @click="showSslForm = true">
            <Plus :size="14" /> 新建 SSL/TLS 配置
          </button>
        </template>

        <!-- ===== Proxy Tab ===== -->
        <template v-if="activeTab === 'proxy'">
          <div v-if="proxyGlobal.length > 0" class="cm-group">
            <div class="cm-group-label">🌐 全局配置</div>
            <div v-for="p in proxyGlobal" :key="p.id" class="cm-card">
              <div class="cm-card-info">
                <span class="cm-card-name">{{ p.name }}</span>
                <span class="cm-card-detail"
                  >{{ String(cfgField(p, 'type', 'socks5')).toUpperCase() }}
                  {{ cfgField(p, 'host') }}:{{ cfgField(p, 'port')
                  }}<template v-if="cfgField(p, 'username')">
                    · {{ cfgField(p, 'username') }}</template
                  ></span
                >
              </div>
              <div class="cm-card-actions">
                <button class="cm-action-btn edit" title="编辑" @click="editProxy(p)"
                  ><Pencil :size="13"
                /></button>
                <button class="cm-action-btn" title="删除" @click="onDeleteProxy(p.id)"
                  ><Trash2 :size="13"
                /></button>
              </div>
            </div>
          </div>
          <div v-if="proxyProject.length > 0" class="cm-group">
            <div class="cm-group-label">📝 项目配置</div>
            <div v-for="p in proxyProject" :key="p.id" class="cm-card">
              <div class="cm-card-info">
                <span class="cm-card-name">{{ p.name }}</span>
                <span class="cm-card-detail"
                  >{{ String(cfgField(p, 'type', 'socks5')).toUpperCase() }}
                  {{ cfgField(p, 'host') }}:{{ cfgField(p, 'port')
                  }}<template v-if="cfgField(p, 'username')">
                    · {{ cfgField(p, 'username') }}</template
                  ></span
                >
              </div>
              <div class="cm-card-actions">
                <button class="cm-action-btn edit" title="编辑" @click="editProxy(p)"
                  ><Pencil :size="13"
                /></button>
                <button class="cm-action-btn" title="删除" @click="onDeleteProxy(p.id)"
                  ><Trash2 :size="13"
                /></button>
              </div>
            </div>
          </div>

          <template v-if="showProxyForm">
            <div class="cm-create-form">
              <div class="cm-form-title">{{
                editingProxyId ? '编辑代理配置' : '+ 新建代理配置'
              }}</div>
              <div class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">配置名称</label>
                  <input v-model="proxyForm.name" class="cm-input" placeholder="例如：公司代理" />
                </div>
                <div class="cm-form-group f1">
                  <label class="cm-form-label">作用域</label>
                  <select v-model="proxyForm.scope" class="cm-select">
                    <option value="global">🌐 全局</option>
                    <option value="project">📝 项目</option>
                  </select>
                </div>
              </div>
              <div class="cm-form-row">
                <div class="cm-form-group f1">
                  <label class="cm-form-label">代理类型</label>
                  <select v-model="proxyForm.type" class="cm-select">
                    <option value="http">HTTP</option>
                    <option value="https">HTTPS</option>
                    <option value="socks4">SOCKS4</option>
                    <option value="socks5">SOCKS5</option>
                  </select>
                </div>
              </div>
              <div class="cm-form-row">
                <div class="cm-form-group f2">
                  <label class="cm-form-label">主机</label>
                  <input
                    v-model="proxyForm.host"
                    class="cm-input"
                    placeholder="proxy.company.com"
                  />
                </div>
                <div class="cm-form-group f1">
                  <label class="cm-form-label">端口</label>
                  <input
                    v-model.number="proxyForm.port"
                    type="number"
                    class="cm-input"
                    placeholder="1080"
                  />
                </div>
              </div>
              <div class="cm-form-section">🔐 代理认证 (可选)</div>
              <div class="cm-form-row">
                <div class="cm-form-group f1">
                  <label class="cm-form-label">用户名</label>
                  <input v-model="proxyForm.username" class="cm-input" placeholder="可选" />
                </div>
                <div class="cm-form-group f1">
                  <label class="cm-form-label">密码</label>
                  <input
                    v-model="proxyForm.password"
                    type="password"
                    class="cm-input"
                    placeholder="可选"
                  />
                </div>
              </div>
              <div class="cm-form-actions">
                <button class="cm-btn secondary" @click="cancelProxyForm">取消</button>
                <button class="cm-btn test" @click="testProxyForm">🧪 测试连接</button>
                <button class="cm-btn primary" @click="saveProxyForm">{{
                  editingProxyId ? '保存修改' : '保存配置'
                }}</button>
              </div>
            </div>
          </template>

          <button v-if="!showProxyForm" class="cm-add-btn" @click="showProxyForm = true">
            <Plus :size="14" /> 新建代理配置
          </button>
        </template>

        <!-- Empty states -->
        <div v-if="activeList.length === 0 && !showActiveForm" class="cm-empty">
          {{ emptyLabel }}
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Boxes, X, Trash2, Pencil, Plus } from 'lucide-vue-next'
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useProfileForm, isGlobalProfile } from '../../composables/useProfileForm'

import type { NetworkProfile } from '../../composables/useNetworkProfiles'

const { t } = useI18n()

// ===== Props =====
interface Props {
  visible: boolean
  defaultTab?: 'ssh' | 'ssl' | 'proxy'
  sshProfiles: NetworkProfile[]
  sslProfiles: NetworkProfile[]
  proxyProfiles: NetworkProfile[]
}
const props = withDefaults(defineProps<Props>(), { defaultTab: 'ssh' })

// ===== Emits =====
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'create-ssh', profile: Record<string, unknown>): void
  (e: 'create-ssl', profile: Record<string, unknown>): void
  (e: 'create-proxy', profile: Record<string, unknown>): void
  (e: 'delete-ssh', id: string): void
  (e: 'delete-ssl', id: string): void
  (e: 'delete-proxy', id: string): void
}>()

// ===== Helpers =====
function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null
}

function cfgField(p: NetworkProfile, field: string, fallback?: unknown): unknown {
  const c = isRecord(p.config) ? p.config : {}
  return c[field] ?? fallback
}

// ===== Tab state =====
const activeTab = ref<'ssh' | 'ssl' | 'proxy'>('ssh')
watch(
  () => props.visible,
  v => {
    if (v) activeTab.value = props.defaultTab ?? 'ssh'
  }
)

function switchTab(t: 'ssh' | 'ssl' | 'proxy') {
  activeTab.value = t
  cancelForms()
}

// ===== Grouped lists =====
const sshGlobal = computed(() => props.sshProfiles.filter(p => isGlobalProfile(p)))
const sshProject = computed(() => props.sshProfiles.filter(p => !isGlobalProfile(p)))
const sslGlobal = computed(() => props.sslProfiles.filter(p => isGlobalProfile(p)))
const sslProject = computed(() => props.sslProfiles.filter(p => !isGlobalProfile(p)))
const proxyGlobal = computed(() => props.proxyProfiles.filter(p => isGlobalProfile(p)))
const proxyProject = computed(() => props.proxyProfiles.filter(p => !isGlobalProfile(p)))

const activeList = computed(() => {
  if (activeTab.value === 'ssh') return props.sshProfiles
  if (activeTab.value === 'ssl') return props.sslProfiles
  return props.proxyProfiles
})

const emptyLabel = computed(() => {
  if (activeTab.value === 'ssh') return t('networkTab.profileManager.noSsh')
  if (activeTab.value === 'ssl') return t('networkTab.profileManager.noSsl')
  return t('networkTab.profileManager.noProxy')
})

const showActiveForm = computed(() => {
  if (activeTab.value === 'ssh') return ssh.showForm.value
  if (activeTab.value === 'ssl') return ssl.showForm.value
  return proxy.showForm.value
})

// ===== SSH form (composable) =====
const ssh = useProfileForm(
  {
    name: '',
    scope: 'project' as 'global' | 'project',
    host: '',
    port: 22,
    username: 'root',
    authMethod: 'password' as 'password' | 'key',
    password: '',
    keyPath: '',
    passphrase: '',
    keepalive: 60,
    localPort: undefined as number | undefined,
    remoteHost: '',
    remotePort: undefined as number | undefined,
  },
  {
    onSave: f => emit('create-ssh', f),
    testMsg: f => `🧪 测试 SSH 连接: ${f.host}:${f.port}`,
  }
)
function editSsh(p: NetworkProfile) {
  ssh.edit(p, p => ({
    name: p.name,
    scope: isGlobalProfile(p) ? 'global' : 'project',
    host: String(cfgField(p, 'host', '')),
    port: Number(cfgField(p, 'port', 22)),
    username: String(cfgField(p, 'username', 'root')),
    authMethod: cfgField(p, 'authMethod') === 'key' ? 'key' : 'password',
    password: String(cfgField(p, 'password', '')),
    keyPath: String(cfgField(p, 'keyPath', '')),
    passphrase: String(cfgField(p, 'passphrase', '')),
    keepalive: Number(cfgField(p, 'keepalive', 60)),
    localPort: (cfgField(p, 'localPort') as number) || undefined,
    remoteHost: String(cfgField(p, 'remoteHost', '')),
    remotePort: (cfgField(p, 'remotePort') as number) || undefined,
  }))
}
const {
  showForm: showSshForm,
  editingId: editingSshId,
  form: sshForm,
  cancelForm: cancelSshForm,
  testForm: testSshForm,
  saveForm: saveSshForm,
} = ssh

// ===== SSL form (composable) =====
const ssl = useProfileForm(
  {
    name: '',
    scope: 'project' as 'global' | 'project',
    mode: 'require',
    ca: '',
    clientCert: '',
    clientKey: '',
    hostnameOverride: '',
  },
  {
    onSave: f => emit('create-ssl', f),
    testMsg: f => `🧪 测试 SSL 连接: ${f.mode}`,
  }
)
function editSsl(p: NetworkProfile) {
  ssl.edit(p, p => ({
    name: p.name,
    scope: isGlobalProfile(p) ? 'global' : 'project',
    mode: String(cfgField(p, 'mode', 'require')),
    ca: String(cfgField(p, 'ca', '')),
    clientCert: String(cfgField(p, 'clientCert', '')),
    clientKey: String(cfgField(p, 'clientKey', '')),
    hostnameOverride: String(cfgField(p, 'hostnameOverride', '')),
  }))
}
const {
  showForm: showSslForm,
  editingId: editingSslId,
  form: sslForm,
  cancelForm: cancelSslForm,
  testForm: testSslForm,
  saveForm: saveSslForm,
} = ssl

// ===== Proxy form (composable) =====
const proxy = useProfileForm(
  {
    name: '',
    scope: 'project' as 'global' | 'project',
    type: 'socks5',
    host: '',
    port: 1080,
    username: '',
    password: '',
  },
  {
    onSave: f => emit('create-proxy', f),
    testMsg: f => `🧪 测试代理: ${String(f.type).toUpperCase()} ${f.host}:${f.port}`,
  }
)
function editProxy(p: NetworkProfile) {
  proxy.edit(p, p => ({
    name: p.name,
    scope: isGlobalProfile(p) ? 'global' : 'project',
    type: String(cfgField(p, 'type', 'socks5')),
    host: String(cfgField(p, 'host', '')),
    port: Number(cfgField(p, 'port', 1080)),
    username: String(cfgField(p, 'username', '')),
    password: String(cfgField(p, 'password', '')),
  }))
}
const {
  showForm: showProxyForm,
  editingId: editingProxyId,
  form: proxyForm,
  cancelForm: cancelProxyForm,
  testForm: testProxyForm,
  saveForm: saveProxyForm,
} = proxy

// ===== Cancel all on tab switch =====
function cancelForms() {
  cancelSshForm()
  cancelSslForm()
  cancelProxyForm()
}

// ===== Delete =====
function close() {
  emit('close')
}
function onDeleteSsh(id: string) {
  emit('delete-ssh', id)
}
function onDeleteSsl(id: string) {
  emit('delete-ssl', id)
}
function onDeleteProxy(id: string) {
  emit('delete-proxy', id)
}

function basename(path: string): string {
  return path.replace(/\\/g, '/').split('/').pop() || path
}
</script>

<style scoped>
.config-manager-overlay {
  position: fixed;
  inset: 0;
  z-index: 1001;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.45);
}
.config-manager-dialog {
  width: 620px;
  max-height: 560px;
  background: var(--color-bg-primary, #1e1e2e);
  border: 1px solid var(--color-border, #313244);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
}

.cm-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  border-bottom: 1px solid var(--color-border, #313244);
}
.cm-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--color-text-primary, #cdd6f4);
}
.cm-close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--color-text-muted, #6c7086);
  cursor: pointer;
}
.cm-close-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--color-text-primary, #cdd6f4);
}

.cm-tabs {
  display: flex;
  gap: 0;
  border-bottom: 1px solid var(--color-border, #313244);
  padding: 0 18px;
}
.cm-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 10px 16px;
  border: none;
  border-bottom: 2px solid transparent;
  background: transparent;
  color: var(--color-text-muted, #6c7086);
  font-size: 13px;
  cursor: pointer;
  transition:
    color 0.15s,
    border-color 0.15s;
}
.cm-tab:hover {
  color: var(--color-text-secondary, #a6adc8);
}
.cm-tab.active {
  color: var(--brand-accent, #e17055);
  border-bottom-color: var(--brand-accent, #e17055);
}
.cm-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  padding: 0 4px;
  border-radius: 9px;
  background: rgba(255, 255, 255, 0.08);
  font-size: 11px;
  color: var(--color-text-muted, #6c7086);
}
.cm-tab.active .cm-badge {
  background: rgba(225, 112, 85, 0.15);
  color: var(--brand-accent, #e17055);
}

.cm-content {
  flex: 1;
  overflow-y: auto;
  padding: 12px 18px 16px;
}

.cm-group {
  margin-bottom: 8px;
}
.cm-group-label {
  font-size: 11px;
  font-weight: 600;
  color: var(--color-text-muted, #6c7086);
  padding: 6px 4px 4px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.cm-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  margin-bottom: 4px;
  border-radius: 6px;
  background: var(--color-bg-secondary, rgba(255, 255, 255, 0.04));
  border: 1px solid transparent;
  transition: border-color 0.15s;
}
.cm-card:hover {
  border-color: var(--color-border, #313244);
}
.cm-card-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.cm-card-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--color-text-primary, #cdd6f4);
}
.cm-card-detail {
  font-size: 11px;
  color: var(--color-text-muted, #6c7086);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cm-card-actions {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}
.cm-action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--color-text-muted, #6c7086);
  cursor: pointer;
}
.cm-action-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--color-text-primary, #cdd6f4);
}
.cm-action-btn.edit:hover {
  color: var(--brand-accent, #e17055);
}

.cm-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 80px;
  color: var(--color-text-muted, #6c7086);
  font-size: 13px;
}

/* add button */
.cm-add-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 10px;
  border: 1px dashed var(--color-border, #45475a);
  border-radius: 6px;
  background: transparent;
  color: var(--color-text-muted, #6c7086);
  font-size: 13px;
  cursor: pointer;
  margin-top: 8px;
  transition: all 0.15s;
}
.cm-add-btn:hover {
  border-color: var(--brand-accent, #e17055);
  color: var(--color-text-primary, #cdd6f4);
  background: rgba(255, 255, 255, 0.02);
}

/* create form */
.cm-create-form {
  margin-top: 8px;
  padding: 14px;
  border: 1px solid var(--brand-accent, #e17055);
  border-radius: 8px;
  background: var(--color-bg-secondary, rgba(137, 180, 250, 0.03));
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.cm-form-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--brand-accent, #e17055);
  margin-bottom: 2px;
}
.cm-form-section {
  font-size: 11px;
  font-weight: 600;
  color: var(--color-text-muted, #6c7086);
  padding-top: 4px;
  border-top: 1px solid var(--color-border-subtle, rgba(255, 255, 255, 0.06));
  letter-spacing: 0.3px;
}
.cm-form-row {
  display: flex;
  gap: 10px;
  align-items: flex-end;
}
.cm-form-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.cm-form-group.f2 {
  flex: 2;
}
.cm-form-group.f1 {
  flex: 1;
}
.cm-form-label {
  font-size: 11px;
  font-weight: 500;
  color: var(--color-text-secondary, #a6adc8);
}
.cm-form-hint {
  font-size: 11px;
  color: var(--color-text-muted, #6c7086);
  font-style: italic;
  padding: 2px 0;
}

.cm-input,
.cm-select {
  padding: 6px 10px;
  border: 1px solid var(--color-border, #45475a);
  border-radius: 4px;
  background: var(--color-bg-primary, #1e1e2e);
  color: var(--color-text-primary, #cdd6f4);
  font-size: 12px;
  font-family: inherit;
  transition: border-color 0.15s;
  box-sizing: border-box;
  width: 100%;
}
.cm-input:focus,
.cm-select:focus {
  outline: none;
  border-color: var(--brand-accent, #e17055);
}
.cm-select {
  cursor: pointer;
}
.cm-input[type='number'] {
  -moz-appearance: textfield;
}

.cm-form-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  padding-top: 6px;
  border-top: 1px solid var(--color-border-subtle, rgba(255, 255, 255, 0.06));
}
.cm-btn {
  padding: 6px 16px;
  border: 1px solid transparent;
  border-radius: 4px;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
  font-family: inherit;
}
.cm-btn.primary {
  background: var(--brand-accent, #e17055);
  color: #fff;
  border-color: var(--brand-accent, #e17055);
}
.cm-btn.primary:hover {
  opacity: 0.9;
}
.cm-btn.secondary {
  background: transparent;
  color: var(--color-text-secondary, #a6adc8);
  border-color: var(--color-border, #45475a);
}
.cm-btn.secondary:hover {
  border-color: var(--color-text-muted, #6c7086);
}
.cm-btn.test {
  background: transparent;
  color: var(--status-attached, #a6e3a1);
  border-color: rgba(166, 227, 161, 0.3);
}
.cm-btn.test:hover {
  background: rgba(166, 227, 161, 0.08);
}
</style>
