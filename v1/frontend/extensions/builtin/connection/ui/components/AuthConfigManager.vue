<template>
  <div class="auth-manager-overlay" @click.self="emit('close')">
    <div class="auth-manager-dialog">
      <!-- Header -->
      <div class="am-header">
        <h3 class="am-title">
          <Shield :size="16" />
          {{ t('navigator.authConfigManager') || '认证配置管理器' }}
        </h3>
        <NButton text size="small" @click="emit('close')">
          <template #icon><X :size="16" /></template>
        </NButton>
      </div>

      <!-- Tabs -->
      <div class="am-tabs">
        <button
          class="am-tab"
          :class="{ active: amTab === 'database' }"
          @click="amTab = 'database'"
        >
          📊 {{ t('navigator.databaseAuth') || '数据库认证' }}
          <span class="am-badge">{{ dbAuthConfigs.length }}</span>
        </button>
        <button class="am-tab" :class="{ active: amTab === 'ssh' }" @click="amTab = 'ssh'">
          🖥 {{ t('navigator.sshAuth') || 'SSH 认证' }}
          <span class="am-badge">{{ sshAuthConfigs.length }}</span>
        </button>
      </div>

      <!-- Content -->
      <div class="am-content">
        <!-- Add new form toggle -->
        <div v-if="!showAddForm" class="am-toolbar">
          <NButton size="small" type="primary" ghost @click="openAddForm">
            + {{ t('navigator.newAuthConfig') || '新建认证配置' }}
          </NButton>
        </div>

        <!-- Add / Edit form -->
        <div v-if="showAddForm" class="am-form-box">
          <div class="am-form-row">
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.name') }}</span>
              <NInput
                v-model:value="newCfg.name"
                size="small"
                :placeholder="t('navigator.dataSourceNamePlaceholder')"
              />
            </div>
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.scope') || '范围' }}</span>
              <NSelect v-model:value="newCfg.scope" size="small" :options="scopeOpts" />
            </div>
          </div>
          <div class="am-form-row">
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.authType') || '认证类型' }}</span>
              <NSelect
                v-model:value="newCfg.authType"
                size="small"
                :options="amTab === 'database' ? dbAuthTypeOpts : sshAuthTypeOpts"
                @update:value="onNewCfgTypeChange"
              />
            </div>
            <div v-if="needsUsername(newCfg.authType)" class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.username') }}</span>
              <NInput v-model:value="newCfg.username" size="small" placeholder="root" />
            </div>
          </div>
          <!-- Dynamic extra fields -->
          <div v-if="newCfg.authType === 'password'" class="am-form-row">
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.password') }}</span>
              <NInput
                v-model:value="newCfg.password"
                type="password"
                size="small"
                show-password-on="click"
                placeholder="****"
              />
            </div>
          </div>
          <div v-if="newCfg.authType === 'pg_class'" class="am-form-row">
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.clientCert') || '客户端证书 (.crt)' }}</span>
              <NInput v-model:value="newCfg.certPath" size="small" placeholder="~/client.crt" />
            </div>
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.clientKey') || '私钥 (.key)' }}</span>
              <NInput v-model:value="newCfg.certKeyPath" size="small" placeholder="~/client.key" />
            </div>
          </div>
          <div v-if="newCfg.authType === 'kerberos'" class="am-form-row">
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.principal') || 'Principal' }}</span>
              <NInput v-model:value="newCfg.principal" size="small" placeholder="user@REALM.COM" />
            </div>
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.keytabPath') || 'Keytab 文件' }}</span>
              <NInput
                v-model:value="newCfg.keytabPath"
                size="small"
                placeholder="/etc/krb5.keytab"
              />
            </div>
          </div>
          <div v-if="newCfg.authType === 'ssh_private_key'" class="am-form-row">
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">{{ t('navigator.privateKeyPath') || '私钥路径' }}</span>
              <NInput v-model:value="newCfg.keytabPath" size="small" placeholder="~/.ssh/id_rsa" />
            </div>
            <div class="am-fg" style="flex: 1">
              <span class="am-fl">Passphrase</span>
              <NInput
                v-model:value="newCfg.passphrase"
                type="password"
                size="small"
                placeholder="可选"
              />
            </div>
          </div>
          <div class="am-form-row" style="margin-top: 4px">
            <NButton size="tiny" type="primary" :loading="saving" @click="saveNewCfg">{{
              editingId ? t('navigator.update') || '更新' : t('navigator.save')
            }}</NButton>
            <NButton size="tiny" @click="cancelEdit">{{ t('navigator.cancel') }}</NButton>
          </div>
        </div>

        <!-- Config list -->
        <div v-if="loading" class="am-empty">
          {{ t('dataPreview.loading') || '加载中...' }}
        </div>
        <div v-else-if="currentConfigs.length === 0 && !showAddForm" class="am-empty">
          {{ t('navigator.noAuthConfigs') || '暂无认证配置，点击上方按钮新建' }}
        </div>
        <div v-for="cfg in currentConfigs" :key="cfg.id" class="am-card">
          <div class="am-card-info">
            <span class="am-card-type"
              >{{ authTypeDef(cfg.authType)?.icon || '🔒' }}
              {{ authTypeDef(cfg.authType)?.label || cfg.authType }}</span
            >
            <span class="am-card-name">{{ cfg.name }}</span>
            <span class="am-card-detail">
              <template v-if="cfg.username">{{ cfg.username }} · </template>
              <template v-if="cfg.authType === 'pg_class'"
                >{{ (cfg.certPath || '').split('/').pop() }}
              </template>
              <template v-if="cfg.authType === 'kerberos'">{{ cfg.principal }} </template>
            </span>
            <span class="am-card-scope">{{ cfg.scope === 'global' ? '🌐' : '📝' }}</span>
          </div>
          <div class="am-card-actions">
            <NButton text size="tiny" title="编辑" @click="editCfg(cfg)">
              <template #icon><Edit2 :size="14" /></template>
            </NButton>
            <NButton text size="tiny" title="删除" type="error" @click="deleteCfg(cfg.id)">
              <template #icon><Trash2 :size="14" /></template>
            </NButton>
            <NButton size="tiny" secondary title="应用到数据源" @click="emit('select', cfg.id)">
              {{ t('navigator.apply') || '应用' }}
            </NButton>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Shield, X, Edit2, Trash2 } from 'lucide-vue-next'
import { NButton, NInput, NSelect, useMessage } from 'naive-ui'
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'

import {
  parseAuthConfig,
  type AuthConfig,
  type BackendAuthConfig,
} from '../composables/useAuthConfig'

const emit = defineEmits<{
  close: []
  select: [configId: string]
}>()

const props = defineProps<{
  scope?: { global: boolean; project: boolean }
}>()

const { t } = useI18n()
const message = useMessage()

// ===== Auth type definitions =====
interface AuthTypeDef {
  category: 'database' | 'ssh'
  icon: string
  label: string
  fields: string[]
}

const AUTH_TYPE_DEFS: Record<string, AuthTypeDef> = {
  password: {
    category: 'database',
    icon: '🔑',
    label: 'SCRAM-SHA-256 / mysql_native_password',
    fields: ['username', 'password'],
  },
  ldap: {
    category: 'database',
    icon: '📋',
    label: 'LDAP / Active Directory',
    fields: ['username', 'password'],
  },
  pg_class: {
    category: 'database',
    icon: '📜',
    label: 'SSL 客户端证书 (mTLS)',
    fields: ['certPath', 'certKeyPath'],
  },
  kerberos: {
    category: 'database',
    icon: '🎫',
    label: 'GSSAPI Kerberos',
    fields: ['principal', 'keytabPath'],
  },
  oauth2: {
    category: 'database',
    icon: '🔗',
    label: 'OAuth 2.0 Bearer Token',
    fields: ['tokenEndpoint', 'clientId', 'clientSecret'],
  },
  os_auth: {
    category: 'database',
    icon: '🖥',
    label: '操作系统认证 (OS Auth)',
    fields: [],
  },
  trust: {
    category: 'database',
    icon: '✅',
    label: '无认证 (Trust)',
    fields: [],
  },
  ssh_password: {
    category: 'ssh',
    icon: '🔑',
    label: 'SSH 密码认证',
    fields: ['username', 'password'],
  },
  ssh_private_key: {
    category: 'ssh',
    icon: '🔐',
    label: 'SSH 公钥认证 (RSA/ED25519/ECDSA)',
    fields: ['username', 'keyPath', 'passphrase'],
  },
}

// ===== State loaded from backend =====
const allConfigs = ref<AuthConfig[]>([])
const loading = ref(false)

async function loadAuthConfigs() {
  loading.value = true
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const isProject = !!props.scope?.project
    const isGlobal = !!props.scope?.global

    if (isGlobal) {
      const raw = await invoke<BackendAuthConfig[]>('list_auth_configs')
      allConfigs.value = raw.map(parseAuthConfig)
    }

    if (isProject) {
      const { useProjectStore } = await import('@/core/project/stores/project')
      const pp = useProjectStore().currentProject?.path
      if (pp) {
        const raw = await invoke<BackendAuthConfig[]>('project_list_auth_configs', {
          projectPath: pp,
        })
        const projectConfigs = raw.map(parseAuthConfig)
        if (isGlobal) {
          // 按 ID 去重：项目配置优先，全局配置补充
          const seenIds = new Set(projectConfigs.map(c => c.id))
          allConfigs.value = [
            ...projectConfigs,
            ...allConfigs.value.filter(c => !seenIds.has(c.id)),
          ]
        } else {
          allConfigs.value = projectConfigs
        }
      }
    }
  } catch (e) {
    console.warn('[AuthConfigManager] loadAuthConfigs failed:', e)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  loadAuthConfigs()
})

// ===== UI State =====
const amTab = ref<'database' | 'ssh'>('database')
const showAddForm = ref(false)
const editingId = ref<string | null>(null)
const saving = ref(false)
const newCfg = ref({
  name: '',
  authType: 'password',
  scope: 'global' as 'global' | 'project',
  username: '',
  password: '',
  certPath: '',
  certKeyPath: '',
  principal: '',
  keytabPath: '',
  tokenEndpoint: '',
  clientId: '',
  clientSecret: '',
  passphrase: '',
})

const scopeOpts = [
  { label: '🌐 全局', value: 'global' },
  { label: '📝 项目', value: 'project' },
]

const dbAuthTypeOpts = [
  { label: '🔑 SCRAM-SHA-256 / mysql_native_password', value: 'password' },
  { label: '📜 SSL 客户端证书 (mTLS)', value: 'pg_class' },
  { label: '🎫 GSSAPI Kerberos', value: 'kerberos' },
  { label: '🔗 OAuth 2.0 Bearer Token', value: 'oauth2' },
]

const sshAuthTypeOpts = [
  { label: '🔑 密码认证', value: 'ssh_password' },
  { label: '🔐 公钥认证 (RSA/ED25519/ECDSA)', value: 'ssh_private_key' },
]

// ===== Computed =====
const dbAuthConfigs = computed(() =>
  allConfigs.value.filter(c => AUTH_TYPE_DEFS[c.authType]?.category === 'database')
)
const sshAuthConfigs = computed(() =>
  allConfigs.value.filter(c => AUTH_TYPE_DEFS[c.authType]?.category === 'ssh')
)
const currentConfigs = computed(() =>
  amTab.value === 'database' ? dbAuthConfigs.value : sshAuthConfigs.value
)

function authTypeDef(type: string): AuthTypeDef | undefined {
  return AUTH_TYPE_DEFS[type]
}

function needsUsername(type: string): boolean {
  const def = AUTH_TYPE_DEFS[type]
  return !!def?.fields?.includes('username')
}

// ===== Form field name → auth_data key mapping =====
const FIELD_KEY_MAP: Record<string, string> = {
  certPath: 'certPath',
  certKeyPath: 'certKeyPath',
  principal: 'principal',
  keytabPath: 'keytabPath',
  tokenEndpoint: 'tokenEndpoint',
  clientId: 'clientId',
  clientSecret: 'clientSecret',
  passphrase: 'passphrase',
}

function buildAuthData(): string {
  const data: Record<string, string> = {}
  if (newCfg.value.username) data.username = newCfg.value.username
  if (newCfg.value.password) data.password = newCfg.value.password
  // Map known field keys
  for (const [formKey, dataKey] of Object.entries(FIELD_KEY_MAP)) {
    const val = (newCfg.value as Record<string, string>)[formKey]
    if (val) data[dataKey] = val
  }
  return JSON.stringify(data)
}

// ===== Actions =====
function onNewCfgTypeChange() {
  newCfg.value.username = ''
  newCfg.value.password = ''
  newCfg.value.certPath = ''
  newCfg.value.certKeyPath = ''
  newCfg.value.principal = ''
  newCfg.value.keytabPath = ''
  newCfg.value.tokenEndpoint = ''
  newCfg.value.clientId = ''
  newCfg.value.clientSecret = ''
  newCfg.value.passphrase = ''
}

function openAddForm() {
  showAddForm.value = true
  editingId.value = null
  newCfg.value = {
    name: '',
    authType: amTab.value === 'database' ? 'password' : 'ssh_password',
    scope: 'global',
    username: '',
    password: '',
    certPath: '',
    certKeyPath: '',
    principal: '',
    keytabPath: '',
    tokenEndpoint: '',
    clientId: '',
    clientSecret: '',
    passphrase: '',
  }
}

function cancelEdit() {
  showAddForm.value = false
  editingId.value = null
}

async function saveNewCfg() {
  if (!newCfg.value.name.trim()) return
  saving.value = true
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const isEdit = !!editingId.value

    // scope=project → 使用 project_* 命令族，参数形状不同
    if (props.scope?.project) {
      const { useProjectStore } = await import('@/core/project/stores/project')
      const pp = useProjectStore().currentProject?.path
      if (!pp) {
        message.warning('⚠️ 未打开项目')
        return
      }
      if (isEdit) {
        await invoke('project_update_auth_config', {
          id: editingId.value,
          name: newCfg.value.name,
          authType: newCfg.value.authType,
          authData: buildAuthData(),
          projectPath: pp,
        })
      } else {
        await invoke('project_create_auth_config', {
          name: newCfg.value.name,
          authType: newCfg.value.authType,
          authData: buildAuthData(),
          projectPath: pp,
        })
      }
    } else {
      const cmd = isEdit ? 'update_auth_config' : 'create_auth_config'
      await invoke(cmd, {
        ac: {
          id: editingId.value || '',
          name: newCfg.value.name,
          auth_type: newCfg.value.authType,
          auth_data: buildAuthData(),
          origin: newCfg.value.scope,
          source_id: null,
          snapshot_at: null,
          created_at: '',
          updated_at: '',
        },
      })
    }
    showAddForm.value = false
    editingId.value = null
    await loadAuthConfigs()
  } catch (e) {
    message.error(
      `❌ ${t('common.operationFailed')}: ${e instanceof Error ? e.message : String(e)}`
    )
  } finally {
    saving.value = false
  }
}

function editCfg(cfg: AuthConfig) {
  editingId.value = cfg.id
  showAddForm.value = true
  amTab.value = AUTH_TYPE_DEFS[cfg.authType]?.category === 'ssh' ? 'ssh' : 'database'
  newCfg.value = {
    name: cfg.name,
    authType: cfg.authType,
    scope: cfg.scope,
    username: cfg.username || '',
    password: cfg.password || '',
    certPath: cfg.certPath || '',
    certKeyPath: cfg.certKeyPath || '',
    principal: cfg.principal || '',
    keytabPath: cfg.keytabPath || '',
    tokenEndpoint: cfg.tokenEndpoint || '',
    clientId: cfg.clientId || '',
    clientSecret: cfg.clientSecret || '',
    passphrase: cfg.passphrase || '',
  }
}

async function deleteCfg(id: string) {
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    if (props.scope?.project) {
      const { useProjectStore } = await import('@/core/project/stores/project')
      const pp = useProjectStore().currentProject?.path
      if (!pp) {
        message.warning('⚠️ 未打开项目')
        return
      }
      await invoke('project_delete_auth_config', { id, projectPath: pp })
    } else {
      await invoke('delete_auth_config', { id })
    }
    await loadAuthConfigs()
  } catch (e) {
    message.error(
      `❌ ${t('common.operationFailed')}: ${e instanceof Error ? e.message : String(e)}`
    )
  }
}
</script>

<style scoped>
.auth-manager-overlay {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background: rgba(0, 0, 0, 0.55);
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
}
.auth-manager-dialog {
  width: 560px;
  max-height: 72vh;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-md);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
}
.am-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px var(--spacing-md);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}
.am-title {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-primary);
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
}
.am-tabs {
  display: flex;
  border-bottom: 1px solid var(--color-border-subtle);
  flex-shrink: 0;
}
.am-tab {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 12px;
  font-size: 12px;
  font-weight: 600;
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  border-bottom: 2px solid transparent;
  transition: all 0.15s;
}
.am-tab.active {
  color: var(--brand-accent);
  border-bottom-color: var(--brand-accent);
}
.am-tab:hover {
  background: var(--color-hover);
}
.am-badge {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 8px;
  background: var(--color-bg-elevated);
  color: var(--color-text-muted);
}
.am-content {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-md);
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.am-toolbar {
  margin-bottom: 4px;
}
.am-form-box {
  padding: 12px;
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-sm);
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.am-form-row {
  display: flex;
  gap: 12px;
}
.am-fg {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.am-fl {
  font-size: 11px;
  font-weight: 600;
  color: var(--color-text-muted);
}
.am-empty {
  text-align: center;
  padding: 32px 0;
  font-size: 12px;
  color: var(--color-text-muted);
}
.am-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-sm);
}
.am-card-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.am-card-type {
  font-size: 11px;
  font-weight: 600;
  color: var(--color-text-secondary);
}
.am-card-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-text-primary);
}
.am-card-detail {
  font-size: 11px;
  color: var(--color-text-muted);
}
.am-card-scope {
  font-size: 10px;
}
.am-card-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
</style>
