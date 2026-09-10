<template>
  <div class="general-tab">
    <!-- Driver info banner -->
    <NAlert v-if="driver" type="info" :bordered="false" class="drv-banner">
      <template #header>
        <span class="drv-tag" :class="`drv-${driver.driver_kind}`">{{ driver.driver_kind }}</span>
        {{ driver.name }}
      </template>
      <template #default>
        {{ driver.version ? `v${driver.version} · ` : ''
        }}{{ driver.is_file ? $t('navigator.fileDbHint') : $t('navigator.networkDbHint') }}
      </template>
    </NAlert>

    <div v-if="!driver" class="empty-hint">{{ $t('navigator.noDriver') }}</div>

    <template v-else>
      <!-- Network DB form -->
      <NSpace v-if="!driver.is_file" vertical :size="12">
        <div class="sec-title">{{ $t('navigator.connectionParams') }}</div>

        <!-- Dynamic fields from config_schema -->
        <div v-for="field in configSchemaFields" :key="field.key" class="form-row">
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ field.label }}</span>
            <!-- Select (enum) -->
            <NSelect
              v-if="field.options"
              :value="getSelectFieldValue(field.key)"
              size="small"
              :placeholder="field.placeholder"
              :options="field.options as Array<{ label: string; value: string | number }>"
              :disabled="isFieldDisabled(field.key)"
              @update:value="(v: string | number) => onFieldChange(field.key, v)"
            />
            <!-- Integer / Number -->
            <NInputNumber
              v-else-if="field.type === 'integer' || field.type === 'number'"
              :value="getFieldValue(field.key) as number"
              size="small"
              :min="field.type === 'integer' ? 1 : undefined"
              :max="field.key === 'port' ? 65535 : undefined"
              :placeholder="field.placeholder"
              :disabled="isFieldDisabled(field.key)"
              @update:value="(v: number | null) => onFieldChange(field.key, v ?? 0)"
            />
            <!-- Boolean / Switch -->
            <NSwitch
              v-else-if="field.type === 'boolean'"
              :value="getFieldValue(field.key) as boolean"
              :disabled="isFieldDisabled(field.key)"
              @update:value="(v: boolean) => onFieldChange(field.key, v)"
            />
            <!-- Password -->
            <NInput
              v-else-if="field.format === 'password'"
              :value="getFieldValue(field.key) as string"
              type="password"
              size="small"
              show-password-on="click"
              :placeholder="field.placeholder"
              :maxlength="256"
              :disabled="isFieldDisabled(field.key)"
              @update:value="(v: string) => onFieldChange(field.key, v)"
            />
            <!-- Default Input -->
            <NInput
              v-else
              :value="getFieldValue(field.key) as string"
              size="small"
              :placeholder="field.placeholder"
              :maxlength="255"
              :disabled="isFieldDisabled(field.key)"
              @update:value="(v: string) => onFieldChange(field.key, v)"
            />
          </div>
        </div>

        <!-- Database Auth: Two-column layout (independent, above dynamic fields) -->
        <div class="sec-title" style="margin-top: 4px">{{
          $t('navigator.databaseAuth') || '数据库认证'
        }}</div>
        <div class="form-row auth-two-col">
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.authMethod') || '认证方法' }}</span>
            <NSelect
              v-model:value="authMethod"
              size="small"
              :options="authMethodOpts"
              :placeholder="$t('navigator.selectAuthMethod') || '选择认证方法'"
              @update:value="onAuthMethodChange"
            />
          </div>
          <div class="form-grp" style="flex: 1.5">
            <span class="form-label">{{
              $t('navigator.savedAuthConfig') || '已保存的认证配置'
            }}</span>
            <div class="auth-cfg-row">
              <NSelect
                v-model:value="selectedAuthConfigId"
                size="small"
                :options="filteredAuthConfigOpts"
                :placeholder="$t('navigator.selectAuthConfig') || '使用已保存配置'"
                class="auth-cfg-select"
                filterable
                clearable
                @update:value="onAuthConfigSelect"
              />
              <NButton
                size="tiny"
                quaternary
                :title="$t('navigator.manageAuth') || '管理认证配置'"
                @click="showAuthManager = true"
              >
                📋
              </NButton>
            </div>
          </div>
        </div>

        <!-- Dynamic auth fields (not in v-for) -->
        <template v-if="!selectedAuthConfigId">
          <div v-if="authMethod === 'password' || authMethod === 'ldap'" class="form-row">
            <div class="form-grp" style="flex: 1">
              <span class="form-label">{{ $t('navigator.username') }}</span>
              <NInput
                v-model:value="local.username"
                size="small"
                placeholder="root"
                :maxlength="128"
                @update:value="emitUpdate"
              />
            </div>
            <div class="form-grp" style="flex: 1">
              <span class="form-label">{{ $t('navigator.password') }}</span>
              <NInput
                v-model:value="local.password"
                type="password"
                size="small"
                show-password-on="click"
                placeholder="****"
                :maxlength="256"
                @update:value="emitUpdate"
              />
            </div>
          </div>
          <div v-else-if="authMethod === 'pg_class'" class="form-row">
            <div class="form-grp" style="flex: 1">
              <span class="form-label">{{
                $t('navigator.clientCert') || '客户端证书 (.crt)'
              }}</span>
              <div class="file-input-row">
                <NInput
                  v-model:value="local.certPath"
                  size="small"
                  placeholder="~/client.crt"
                  :maxlength="1024"
                  @update:value="emitUpdate"
                />
                <NButton size="tiny" secondary @click="browseCert"> 📂 </NButton>
              </div>
            </div>
            <div class="form-grp" style="flex: 1">
              <span class="form-label">{{ $t('navigator.clientKey') || '私钥 (.key)' }}</span>
              <div class="file-input-row">
                <NInput
                  v-model:value="local.certKeyPath"
                  size="small"
                  placeholder="~/client.key"
                  :maxlength="1024"
                  @update:value="emitUpdate"
                />
                <NButton size="tiny" secondary @click="browseCertKey"> 📂 </NButton>
              </div>
            </div>
          </div>
          <div v-else-if="authMethod === 'kerberos'" class="form-row">
            <div class="form-grp" style="flex: 1">
              <span class="form-label">{{ $t('navigator.principal') || 'Principal' }}</span>
              <NInput
                v-model:value="local.principal"
                size="small"
                placeholder="user@REALM.COM"
                :maxlength="255"
                @update:value="emitUpdate"
              />
            </div>
            <div class="form-grp" style="flex: 1">
              <span class="form-label">{{ $t('navigator.keytabPath') || 'Keytab 文件' }}</span>
              <div class="file-input-row">
                <NInput
                  v-model:value="local.keytabPath"
                  size="small"
                  placeholder="/etc/krb5.keytab"
                  :maxlength="1024"
                  @update:value="emitUpdate"
                />
                <NButton size="tiny" secondary @click="browseKeytab"> 📂 </NButton>
              </div>
            </div>
          </div>
          <div v-else-if="authMethod === 'oauth2'" class="form-row">
            <div class="form-grp" style="flex: 1">
              <span class="form-label">{{ $t('navigator.tokenEndpoint') || 'Token 端点' }}</span>
              <NInput
                v-model:value="local.tokenEndpoint"
                size="small"
                placeholder="https://auth.example.com/token"
                :maxlength="2048"
                @update:value="emitUpdate"
              />
            </div>
            <div class="form-grp" style="flex: 1">
              <span class="form-label">{{ $t('navigator.clientId') || 'Client ID' }}</span>
              <NInput
                v-model:value="local.clientId"
                size="small"
                placeholder="your-client-id"
                :maxlength="255"
                @update:value="emitUpdate"
              />
            </div>
          </div>
          <div v-if="authMethod === 'oauth2'" class="form-row">
            <div class="form-grp" style="flex: 1">
              <span class="form-label">{{ $t('navigator.clientSecret') || 'Client Secret' }}</span>
              <NInput
                v-model:value="local.clientSecret"
                type="password"
                size="small"
                show-password-on="click"
                placeholder="****"
                :maxlength="512"
                @update:value="emitUpdate"
              />
            </div>
          </div>
          <div v-if="authMethod === 'os_auth' || authMethod === 'trust'" class="form-row">
            <NAlert type="info" :bordered="false">
              {{ authMethod === 'os_auth' ? $t('navigator.osAuthTip') : $t('navigator.trustTip') }}
            </NAlert>
          </div>
        </template>

        <!-- Pre-filled from auth config -->
        <div v-else class="form-row">
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.username') }}</span>
            <NInput v-model:value="local.username" size="small" disabled />
          </div>
          <div v-if="authMethod === 'password'" class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.password') }}</span>
            <NInput
              v-model:value="local.password"
              type="password"
              size="small"
              disabled
              show-password-on="click"
            />
          </div>
          <div class="form-config-badge">
            🔐 {{ $t('navigator.credentialsFromAuth') || '凭据来自认证配置' }}
          </div>
        </div>
        <div v-if="selectedAuthConfigId && authMethod === 'pg_class'" class="form-row">
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.clientCert') || '客户端证书' }}</span>
            <NInput v-model:value="local.certPath" size="small" disabled />
          </div>
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.clientKey') || '私钥' }}</span>
            <NInput v-model:value="local.certKeyPath" size="small" disabled />
          </div>
        </div>
        <div v-if="selectedAuthConfigId && authMethod === 'kerberos'" class="form-row">
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.principal') || 'Principal' }}</span>
            <NInput v-model:value="local.principal" size="small" disabled />
          </div>
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.keytabPath') || 'Keytab' }}</span>
            <NInput v-model:value="local.keytabPath" size="small" disabled />
          </div>
        </div>
        <div v-if="selectedAuthConfigId && authMethod === 'oauth2'" class="form-row">
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.tokenEndpoint') || 'Token 端点' }}</span>
            <NInput v-model:value="local.tokenEndpoint" size="small" disabled />
          </div>
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.clientId') || 'Client ID' }}</span>
            <NInput v-model:value="local.clientId" size="small" disabled />
          </div>
        </div>
      </NSpace>

      <!-- File DB form (keeps existing logic) -->
      <NSpace v-else vertical :size="12">
        <div class="sec-title">{{ $t('navigator.databaseFile') }}</div>
        <div class="form-row">
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ $t('navigator.filePath') }}</span>
            <NSpace :size="8">
              <NInput
                v-model:value="local.file_path"
                size="small"
                :placeholder="filePathPlaceholder"
                style="flex: 1"
                :maxlength="1024"
                @update:value="emitUpdate"
              />
              <NButton size="small" secondary @click="browseFile">{{
                $t('navigator.browse')
              }}</NButton>
              <NButton size="small" secondary class="btn-new-file" @click="createNewDbFile">{{
                $t('navigator.newFile') || '新建'
              }}</NButton>
            </NSpace>
          </div>
        </div>
      </NSpace>

      <!-- Advanced config schema fields (fields not rendered above) -->
      <NSpace v-if="advancedSchemaFields.length > 0" vertical :size="12">
        <div class="sec-title">{{ $t('navigator.advancedParams') || '高级连接参数' }}</div>
        <div v-for="field in advancedSchemaFields" :key="field.key" class="form-row">
          <div class="form-grp" style="flex: 1">
            <span class="form-label">{{ field.label }}</span>
            <!-- Select 类型 -->
            <NSelect
              v-if="field.options"
              v-model:value="schemaFormData[field.key] as string | number | null"
              size="small"
              :placeholder="field.placeholder"
              :options="field.options as Array<{ label: string; value: string | number }>"
              @update:value="emitUpdate"
            />
            <!-- Switch 类型 -->
            <NSwitch
              v-else-if="field.type === 'boolean'"
              v-model:value="schemaFormData[field.key] as boolean"
              @update:value="emitUpdate"
            />
            <!-- Number 类型 -->
            <NInputNumber
              v-else-if="field.type === 'integer' || field.type === 'number'"
              v-model:value="schemaFormData[field.key] as number | null"
              size="small"
              :placeholder="field.placeholder"
              :min="field.min"
              :max="field.max"
              @update:value="emitUpdate"
            />
            <!-- 默认 Input 类型 -->
            <NInput
              v-else
              v-model:value="schemaFormData[field.key] as string"
              size="small"
              :placeholder="field.placeholder"
              :maxlength="255"
              @update:value="emitUpdate"
            />
            <div v-if="field.helpText" class="field-help">{{ field.helpText }}</div>
          </div>
        </div>
      </NSpace>
    </template>

    <!-- Auth Config Manager overlay -->
    <AuthConfigManager
      v-if="showAuthManager"
      :scope="props.scope"
      @close="onAuthManagerClose"
      @select="onAuthConfigExternalSelect"
    />
    <!-- New DB file dialog -->
    <NModal
      v-model:show="showNewDbDialog"
      :title="($t('navigator.newDbFilePrompt') as string) || '新建数据库文件'"
    >
      <div style="padding: 16px">
        <p style="margin-bottom: 12px; font-size: 13px; color: var(--n-text-color-2)">
          {{ $t('navigator.newDbFileHint') || '请输入文件路径（已存在则复用，不存在则自动创建）' }}
        </p>
        <NInput v-model:value="newDbFilePath" placeholder="例如: data/mydb.db" />
      </div>
      <template #footer>
        <NButton @click="showNewDbDialog = false">{{ $t('common.cancel') }}</NButton>
        <NButton type="primary" @click="confirmNewDb">{{ $t('common.create') }}</NButton>
      </template>
    </NModal>
  </div>
</template>

<script setup lang="ts">
import { NAlert, NButton, NInput, NInputNumber, NModal, NSelect, NSpace, NSwitch } from 'naive-ui'
import { reactive, computed, watch, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { useAuthConfig, parseSupportedAuthTypes } from '../../composables/useAuthConfig'
import AuthConfigManager from '../AuthConfigManager.vue'

import type { Driver } from '../../../domain/types'

// ==================== Props / Emits ====================

interface Props {
  driver: Driver | null
  formData: Record<string, unknown>
  scope?: { global: boolean; project: boolean }
  projectPath?: string | null
}

const props = withDefaults(defineProps<Props>(), {
  driver: null,
  formData: () => ({}),
})

const emit = defineEmits<{
  'update:form-data': [data: Record<string, unknown>]
  'auth-config-change': [authConfigId: string | null, authMethod: string]
}>()

useI18n() // initialize i18n for template $t()

// ==================== Config Schema Field Types ====================

/** config_schema 解析出的表单字段 */
interface ConfigSchemaField {
  key: string
  label: string
  type: 'string' | 'integer' | 'number' | 'boolean'
  required: boolean
  placeholder: string
  defaultValue: unknown
  order: number
  format?: string
  options?: Array<{ label: string; value: unknown }>
  helpText?: string
  min?: number
  max?: number
  rows?: number
}

/** config_schema 为空时根据 driver_kind 返回不同的降级字段 */
function getDefaultFields(driverKind?: string): ConfigSchemaField[] {
  switch (driverKind) {
    case 'http':
      return [
        {
          key: 'url',
          label: 'URL',
          type: 'string',
          required: true,
          order: 1,
          placeholder: 'https://api.example.com/v1',
          defaultValue: '',
        },
        {
          key: 'headers',
          label: 'Headers',
          type: 'string',
          required: false,
          order: 2,
          placeholder: '{"Authorization": "Bearer ..."}',
          defaultValue: '',
        },
      ]
    case 'wasm':
      return [
        {
          key: 'wasmPath',
          label: 'WASM 路径',
          type: 'string',
          required: true,
          order: 1,
          placeholder: '',
          defaultValue: '',
        },
      ]
    default:
      return [
        {
          key: 'host',
          label: '主机',
          type: 'string',
          required: true,
          placeholder: '请输入主机地址',
          defaultValue: 'localhost',
          order: 1,
        },
        {
          key: 'port',
          label: '端口',
          type: 'integer',
          required: true,
          placeholder: '请输入端口号',
          defaultValue: 3306,
          order: 2,
        },
        {
          key: 'database',
          label: '数据库',
          type: 'string',
          required: false,
          placeholder: '请输入数据库名',
          defaultValue: '',
          order: 3,
        },
        {
          key: 'username',
          label: '用户名',
          type: 'string',
          required: false,
          placeholder: 'root',
          defaultValue: 'root',
          order: 4,
        },
        {
          key: 'password',
          label: '密码',
          type: 'string',
          required: false,
          placeholder: '****',
          defaultValue: '',
          order: 5,
          format: 'password',
        },
      ]
  }
}

/**
 * 解析 driver.config_schema (JSON Schema 字符串) → 表单字段列表
 * 按 order 排序；解析失败时根据 driverKind 返回降级基础字段
 */
function parseConfigSchema(schema: string, driverKind?: string): ConfigSchemaField[] {
  const fallback = getDefaultFields(driverKind)
  if (!schema) return fallback

  let parsed: Record<string, unknown>
  try {
    parsed = JSON.parse(schema) as Record<string, unknown>
  } catch {
    console.warn('[GeneralTab] config_schema JSON 解析失败，使用降级字段')
    return fallback
  }

  if (parsed.type !== 'object' || !parsed.properties) {
    return fallback
  }

  const propsMap = parsed.properties as Record<string, Record<string, unknown>>
  const requiredSet = new Set<string>(
    Array.isArray(parsed.required) ? (parsed.required as string[]) : []
  )
  const fields: ConfigSchemaField[] = []

  for (const [key, prop] of Object.entries(propsMap)) {
    const jsonType = String(prop.type ?? 'string')
    let fieldType: ConfigSchemaField['type'] = 'string'
    if (jsonType === 'integer') fieldType = 'integer'
    else if (jsonType === 'number') fieldType = 'number'
    else if (jsonType === 'boolean') fieldType = 'boolean'

    const field: ConfigSchemaField = {
      key,
      label: (prop.title as string) || key,
      type: fieldType,
      required: requiredSet.has(key),
      placeholder: (prop.description as string) || '',
      defaultValue: prop.default,
      order: typeof prop.order === 'number' ? (prop.order as number) : 999,
      format: prop.format as string | undefined,
    }

    // 处理 enum → select options
    if (Array.isArray(prop.enum)) {
      field.options = (prop.enum as unknown[]).map((v: unknown) => ({
        label: String(v),
        value: v,
      }))
    }

    if (typeof prop.minimum === 'number') field.min = prop.minimum as number
    if (typeof prop.maximum === 'number') field.max = prop.maximum as number
    if (prop.description) field.helpText = prop.description as string

    fields.push(field)
  }

  // 按 order 排序
  fields.sort((a, b) => a.order - b.order)

  return fields.length > 0 ? fields : fallback
}

// ==================== Local Form State ====================

interface LocalForm {
  host: string
  port: number
  database: string
  username: string
  password: string
  file_path: string
  certPath: string
  certKeyPath: string
  principal: string
  keytabPath: string
  tokenEndpoint: string
  clientId: string
  clientSecret: string
}

const local = reactive<LocalForm>({
  host: '',
  port: 0,
  database: '',
  username: '',
  password: '',
  file_path: '',
  certPath: '',
  certKeyPath: '',
  principal: '',
  keytabPath: '',
  tokenEndpoint: '',
  clientId: '',
  clientSecret: '',
})

/** 动态表单数据（config_schema 定义但不在 LocalForm 中的字段） */
const schemaFormData = reactive<Record<string, unknown>>({})

// ==================== Auth State (via composable) ====================

const {
  authMethod,
  selectedAuthConfigId,
  showAuthManager,
  authMethodOpts,
  filteredAuthConfigOpts,
  onAuthMethodChange,
  onAuthConfigSelect,
  onAuthConfigExternalSelect,
  onAuthManagerClose,
  loadAuthConfigs,
  updateSupportedAuthTypes,
} = useAuthConfig({
  local: {
    get username() {
      return local.username
    },
    set username(v) {
      local.username = v
    },
    get password() {
      return local.password
    },
    set password(v) {
      local.password = v
    },
    get certPath() {
      return local.certPath
    },
    set certPath(v) {
      local.certPath = v
    },
    get certKeyPath() {
      return local.certKeyPath
    },
    set certKeyPath(v) {
      local.certKeyPath = v
    },
    get principal() {
      return local.principal
    },
    set principal(v) {
      local.principal = v
    },
    get keytabPath() {
      return local.keytabPath
    },
    set keytabPath(v) {
      local.keytabPath = v
    },
    get tokenEndpoint() {
      return local.tokenEndpoint
    },
    set tokenEndpoint(v) {
      local.tokenEndpoint = v
    },
    get clientId() {
      return local.clientId
    },
    set clientId(v) {
      local.clientId = v
    },
    get clientSecret() {
      return local.clientSecret
    },
    set clientSecret(v) {
      local.clientSecret = v
    },
  },
  onFormUpdate: emitUpdate,
  onAuthConfigChange: (configId, authType) => emit('auth-config-change', configId, authType),
})

// ==================== Computed ====================

const filePathPlaceholder = computed(() => {
  if (props.driver?.name?.toLowerCase().includes('duckdb')) return '~/data.duckdb'
  return '~/data.db'
})

/** 由认证区独立处理的字段 key 集合 */
const AUTH_MANAGED_KEYS = new Set([
  'username',
  'password',
  'certPath',
  'certKeyPath',
  'principal',
  'keytabPath',
  'tokenEndpoint',
  'clientId',
  'clientSecret',
])

/** 基础连接字段 key 集合（主 v-for 渲染，其余非认证字段进入高级参数区） */
const BASIC_SCHEMA_KEYS = new Set([
  'host',
  'port',
  'database',
  'url',
  'wasmPath',
  'headers',
  'file_path',
])

/** 从 config_schema 解析的基础连接字段（主 v-for 渲染） */
const configSchemaFields = computed<ConfigSchemaField[]>(() => {
  const schema = props.driver?.config_schema
  const allFields = parseConfigSchema(schema || '', props.driver?.driver_kind)
  // 仅渲染基础连接字段，过滤认证管理区和高级参数字段
  return allFields.filter(f => BASIC_SCHEMA_KEYS.has(f.key) && !AUTH_MANAGED_KEYS.has(f.key))
})

/** 高级参数字段（config_schema 定义的，但不属于主 v-for 基本字段） */
const advancedSchemaFields = ref<ConfigSchemaField[]>([])

/** 新建数据库文件弹窗 */
const showNewDbDialog = ref(false)
const newDbFilePath = ref('')

// ==================== Field Helpers ====================

/** 获取字段当前值（优先 local，其次 schemaFormData） */
function getFieldValue(key: string): unknown {
  const localRecord = local as unknown as Record<string, unknown>
  if (localRecord[key] !== undefined) return localRecord[key]
  return schemaFormData[key] ?? ''
}

/** 获取 Select 字段值（类型安全，避免模板中使用 | 联合类型触发 vue/no-deprecated-filter） */
function getSelectFieldValue(key: string): string | number | null {
  const val = getFieldValue(key)
  if (typeof val === 'string' || typeof val === 'number') return val
  return null
}

/** 更新字段值并通知父组件 */
function onFieldChange(key: string, value: unknown): void {
  const localRecord = local as unknown as Record<string, unknown>
  if (localRecord[key] !== undefined) {
    localRecord[key] = value
  } else {
    schemaFormData[key] = value
  }
  emitUpdate()
}

/**
 * 判断字段是否应禁用：当选择了已保存认证配置时，
 * 用户名 / 密码字段应显示为禁用（凭据来自认证配置）
 */
function isFieldDisabled(key: string): boolean {
  if (!selectedAuthConfigId.value) return false
  return key === 'username' || key === 'password'
}

// ==================== Emit ====================

function emitUpdate() {
  emit('update:form-data', {
    ...local,
    ...schemaFormData,
    authMethod: authMethod.value,
    selectedAuthConfigId: selectedAuthConfigId.value,
  })
}

// ==================== File Dialogs ====================

async function browseFile() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const file = await open({
      filters: [{ name: 'Database', extensions: ['db', 'sqlite', 'sqlite3', 'duckdb'] }],
    })
    if (file) {
      local.file_path = file as string
      emitUpdate()
    }
  } catch {
    console.warn('[GeneralTab] 文件对话框不可用（浏览器环境）')
  }
}

async function browseCert() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const file = await open({
      filters: [{ name: 'Certificate', extensions: ['crt', 'pem', 'cert'] }],
    })
    if (file) {
      local.certPath = file as string
      emitUpdate()
    }
  } catch (err) {
    console.warn('[GeneralTab] 证书文件选择失败:', err)
  }
}

async function browseCertKey() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const file = await open({
      filters: [{ name: 'Private Key', extensions: ['key', 'pem'] }],
    })
    if (file) {
      local.certKeyPath = file as string
      emitUpdate()
    }
  } catch (err) {
    console.warn('[GeneralTab] 私钥文件选择失败:', err)
  }
}

async function browseKeytab() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const file = await open({
      filters: [{ name: 'Keytab', extensions: ['keytab'] }],
    })
    if (file) {
      local.keytabPath = file as string
      emitUpdate()
    }
  } catch (err) {
    console.warn('[GeneralTab] Keytab 文件选择失败:', err)
  }
}

function createNewDbFile() {
  const ext = props.driver?.name?.toLowerCase().includes('duckdb') ? 'duckdb' : 'db'
  const defaultName = `new_database.${ext}`
  newDbFilePath.value = (local.file_path || defaultName) as string
  showNewDbDialog.value = true
}

function confirmNewDb() {
  if (newDbFilePath.value.trim()) {
    local.file_path = newDbFilePath.value.trim()
    emitUpdate()
  }
  showNewDbDialog.value = false
}

// ==================== Lifecycle ====================

// Sync from props.formData on creation
onMounted(async () => {
  syncFromFormData()
  if (!props.formData || Object.keys(props.formData).length === 0) {
    if (props.driver?.default_port) {
      local.port = props.driver.default_port
    }
  }

  // 从后端加载已保存的认证配置列表
  loadAuthConfigs(props.projectPath ?? undefined)
})

/** 从 props.formData 同步到 local 响应式对象 */
function syncFromFormData() {
  if (!props.formData || Object.keys(props.formData).length === 0) return
  local.host = String(props.formData.host ?? local.host)
  local.port = Number(props.formData.port ?? props.driver?.default_port ?? local.port)
  local.database = String(props.formData.database ?? local.database)
  local.username = String(props.formData.username ?? local.username)
  local.password = String(props.formData.password ?? local.password)
  local.file_path = String(props.formData.file_path ?? local.file_path)
  if (props.formData.authMethod) authMethod.value = String(props.formData.authMethod)
  if (props.formData.selectedAuthConfigId)
    selectedAuthConfigId.value = String(props.formData.selectedAuthConfigId)
}

// P1: 监听外部 formData 变更（如 URL 解析填充），同步到 local 响应式对象
watch(
  () => props.formData,
  () => syncFromFormData(),
  { deep: true }
)

/** 更新 config_schema 衍生的高级参数字段 */
function updateAdvancedSchemaFields() {
  const schema = props.driver?.config_schema
  const allFields = parseConfigSchema(schema || '', props.driver?.driver_kind)
  const mainKeys = new Set(configSchemaFields.value.map(f => f.key))
  advancedSchemaFields.value = allFields.filter(
    f => !mainKeys.has(f.key) && !AUTH_MANAGED_KEYS.has(f.key)
  )

  for (const field of advancedSchemaFields.value) {
    if (schemaFormData[field.key] === undefined) {
      schemaFormData[field.key] = field.defaultValue ?? ''
    }
  }

  for (const key of Object.keys(schemaFormData)) {
    if (props.formData[key] !== undefined) {
      schemaFormData[key] = props.formData[key]
    }
  }
}

// Watch driver changes: reset port, auth types, schema fields
watch(
  () => props.driver?.id,
  () => {
    local.port = props.driver?.default_port ?? 0
    // 解析驱动支持的认证方式列表
    const types = parseSupportedAuthTypes(props.driver?.supported_auth_types)
    updateSupportedAuthTypes(types)
    // 更新 config_schema 衍生字段
    updateAdvancedSchemaFields()
    // 文件数据库：自动填充默认文件路径
    if (props.driver?.is_file && !local.file_path) {
      const defaultName = props.driver.id === 'duckdb' ? 'data.duckdb' : 'data.db'
      const homeDir =
        typeof window !== 'undefined'
          ? (window as any).__TAURI_INTERNALS__?.metadata?.homeDir || ''
          : ''
      local.file_path = homeDir ? `${homeDir}/${defaultName}` : `./${defaultName}`
    }
    emitUpdate()
  },
  { immediate: true }
)
</script>

<style scoped>
.general-tab {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 2px 0;
}
.drv-banner {
  border-radius: 6px;
}
.drv-tag {
  font-size: 10px;
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 600;
  margin-right: 6px;
}
.drv-tag.drv-native {
  background: rgba(255, 255, 255, 0.06);
  color: var(--color-text-secondary);
}
.drv-tag.drv-jdbc {
  background: rgba(244, 102, 35, 0.15);
  color: var(--driver-jdbc);
}
.drv-tag.drv-python {
  background: rgba(55, 118, 171, 0.15);
  color: var(--driver-python);
}
.drv-tag.drv-js {
  background: rgba(247, 223, 30, 0.15);
  color: var(--driver-js);
}
.empty-hint {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 120px;
  font-size: 13px;
  color: var(--color-text-muted);
}
.sec-title {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  color: var(--color-text-muted);
  letter-spacing: 0.5px;
}
.form-row {
  display: flex;
  gap: 12px;
}
.form-grp {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.form-label {
  font-size: 12px;
  color: var(--color-text-secondary);
  font-weight: 500;
}

/* Auth two-column */
.auth-two-col {
  align-items: flex-start;
}
.auth-cfg-row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.auth-cfg-select {
  flex: 1;
}

/* File input row */
.file-input-row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.file-input-row :first-child {
  flex: 1;
}

/* Auth config badge */
.form-config-badge {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--brand-accent);
  padding: 4px 8px;
  background: var(--brand-accent-soft);
  border-radius: var(--border-radius-sm);
}

/* New file button */
.btn-new-file {
  white-space: nowrap;
}

/* Schema dynamic fields */
.field-help {
  font-size: 11px;
  color: var(--color-text-muted);
  margin-top: 2px;
}
</style>
