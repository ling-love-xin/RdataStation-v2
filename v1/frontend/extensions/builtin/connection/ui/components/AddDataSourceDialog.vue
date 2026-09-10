<template>
  <NModal :show="modelValue" :mask-closable="false" @update:show="handleClose">
    <div class="datasource-card">
      <!-- Card header -->
      <div class="card-header">
        <Database :size="16" class="card-icon" />
        <span class="card-title">{{
          isEditing ? $t('navigator.editDataSource') : $t('navigator.addDataSource')
        }}</span>
      </div>

      <div class="card-body">
        <!-- Sidebar -->
        <AddDataSourceSidebar
          v-model:selected-type-id="selectedTypeId"
          :staging-items="stagingItems"
          :staging-index="stagingIndex"
          @add-staging="handleAddStaging"
          @remove-staging="handleRemoveStaging"
          @select-staging="handleSelectStaging"
        />

        <!-- Right panel -->
        <div class="dlg-right">
          <DataSourceHeader
            v-model:name="headerData.name"
            v-model:description="headerData.description"
            v-model:scope-global="scope.global"
            v-model:scope-project="scope.project"
            v-model:selected-driver-id="headerData.selectedDriverId"
            v-model:uri-editing="uriEditing"
            v-model:manual-uri="manualUri"
            :driver-options="driverOptions"
            :uri-preview="uriPreview"
            :name-label="'* ' + $t('navigator.name')"
            name-required
            :name-placeholder="$t('navigator.dataSourceNamePlaceholder')"
            :desc-label="$t('navigator.description')"
            :desc-placeholder="$t('navigator.dataSourceDescPlaceholder')"
            :global-label="$t('navigator.globalConnection')"
            :project-label="$t('navigator.projectConnection')"
            :driver-label="'* ' + $t('navigator.driver')"
            :driver-placeholder="$t('navigator.selectDbType')"
            uri-label="URI"
            uri-placeholder="jdbc:mysql://..."
            :url-template="selectedDriver?.url_template ?? null"
            @driver-change="onDriverChange"
            @parse-url="onParseUrl"
          />

          <NAlert
            v-if="scopeChangedWarning"
            type="warning"
            class="scope-warning"
            closable
            @close="scopeChangedWarning = false"
          >
            {{ $t('navigator.scopeChangeWarning') }}
          </NAlert>

          <NAlert
            v-if="scope.project && !projectStore.hasProject"
            type="info"
            class="scope-warning"
            :bordered="false"
          >
            {{ $t('navigator.projectScopeNoProject') }}
          </NAlert>

          <!-- Tabs -->
          <NTabs v-model:value="activeTab" type="line" size="small" class="dlg-tabs">
            <NTabPane name="general" :tab="$t('navigator.tabGeneral')">
              <GeneralTab
                :driver="selectedDriver"
                :form-data="formData"
                :scope="scope"
                :project-path="projectStore.currentProject?.path"
                @update:form-data="onFormData"
                @auth-config-change="onAuthConfigChange"
              />
            </NTabPane>
            <NTabPane name="network" :tab="$t('navigator.tabNetwork')">
              <NetworkTab :driver="selectedDriver" :scope="scope" @extra-config="onExtraConfig" />
            </NTabPane>
            <NTabPane name="capabilities" :tab="$t('navigator.tabCapabilities')">
              <CapabilitiesTab :driver="selectedDriver" @extra-config="onCapabilityChange" />
            </NTabPane>
            <NTabPane name="properties" :tab="$t('navigator.tabDriverProps')">
              <DriverPropsTab
                :driver="selectedDriver"
                :driver-properties="driverPropertiesExtra"
                @extra-config="onExtraConfig"
              />
            </NTabPane>
            <NTabPane name="advanced" :tab="$t('navigator.tabAdvanced')">
              <AdvancedTab :driver="selectedDriver" :scope="scope" @extra-config="onExtraConfig" />
            </NTabPane>
          </NTabs>

          <!-- Footer -->
          <div class="card-footer">
            <div v-if="testResult" class="test-result">
              <span :class="['test-icon', testResult.success ? 'ok' : 'fail']">
                {{ testResult.success ? '✓' : '✗' }}
              </span>
              <span class="test-msg">{{ testResult.message }}</span>
              <span v-if="testResult.latencyMs != null" class="test-latency"
                >· {{ testResult.latencyMs }}ms</span
              >
            </div>
            <div class="footer-spacer" />
            <span v-if="applyProgress" class="apply-progress">
              {{ $t('navigator.applying') }} {{ applyProgress.current }}/{{ applyProgress.total }}
            </span>
            <NButton @click="handleClose">{{ $t('navigator.cancel') }}</NButton>
            <NButton :loading="testing" @click="handleTest">{{
              $t('navigator.testConnection')
            }}</NButton>
            <NButton type="primary" :loading="saving" @click="handleSave">{{
              $t('navigator.save')
            }}</NButton>
            <NButton type="primary" secondary :loading="applying" @click="handleApply">
              {{ $t('navigator.apply') }}
            </NButton>
          </div>
        </div>
      </div>
    </div>
  </NModal>
  <!-- Test connection feedback modal -->
  <TestResultModal
    :show="showTestModal"
    :result="lastTestResult"
    :host="String(formData.host || '')"
    :port="String(formData.port || '')"
    :database="String(formData.database || '')"
    :user="String(formData.username || '')"
    :url="uriPreview"
    :driver-name="selectedDriver?.name ?? ''"
    :network-info="testNetworkInfo"
    @close="onTestModalClose"
  />
  <!-- 关闭确认弹窗 -->
  <NModal
    v-model:show="showCloseConfirm"
    preset="dialog"
    :title="$t('navigator.unsavedChanges')"
    positive-text="确定关闭"
    :negative-text="$t('navigator.continueEditing')"
    @positive-click="confirmClose"
    @negative-click="cancelClose"
  >
    {{ $t('navigator.unsavedChangesHint') }}
  </NModal>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { Database } from 'lucide-vue-next'
import { NButton, NModal, NTabs, NTabPane, NAlert, useMessage, useDialog } from 'naive-ui'
import { ref, computed, watch, onMounted, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'

import { useProjectStore } from '@/core/project/stores/project'

import AddDataSourceSidebar from './AddDataSourceSidebar.vue'
import DataSourceHeader from './DataSourceHeader.vue'
import AdvancedTab from './tabs/AdvancedTab.vue'
import CapabilitiesTab from './tabs/CapabilitiesTab.vue'
import DriverPropsTab from './tabs/DriverPropsTab.vue'
import GeneralTab from './tabs/GeneralTab.vue'
import NetworkTab from './tabs/NetworkTab.vue'
import TestResultModal from './TestResultModal.vue'
import { useAddDataSource } from '../composables/useAddDataSource'
import { useDriverRegistry } from '../composables/useDriverRegistry'
import { useNetworkProfiles } from '../composables/useNetworkProfiles'
import { useUrlBuilder } from '../composables/useUrlBuilder'
import {
  connectDatabase as connectDatabaseService,
  closeConnection,
  updateGlobalConnection,
} from '../services/connection'
import { useProjectConnectionStore } from '../stores/project-connection-store'

import type { Driver } from '../../domain/types'
import type { ProjectConnection } from '../../types/connection'
import type { StagingItem } from '../composables/useAddDataSource'

interface Props {
  modelValue: boolean
  initialDriver?: Driver | null
  initialName?: string
  initialConnection?: ProjectConnection | null
}

const props = withDefaults(defineProps<Props>(), {
  modelValue: false,
  initialDriver: null,
  initialName: '',
  initialConnection: null,
})

const emit = defineEmits<{
  (e: 'update:modelValue', v: boolean): void
  (e: 'save'): void
  (e: 'connectionsChanged'): void
}>()

const { t } = useI18n()
const message = useMessage()
const dialog = useDialog()
const projectStore = useProjectStore()
const projectConnectionStore = useProjectConnectionStore()
const { drivers, loadAll } = useDriverRegistry()
const {
  headerData,
  scope,
  selectedEnvId,
  setFileDb,
  validate,
  stagingItems,
  stagingIndex,
  buildStagingItem,
  addStaging,
  removeStaging,
  selectStaging,
  clearStagingItems,
  loadStagingItems,
  markStagingApplied,
  formData,
  authConfigId,
  authMethod,
  networkConfigId,
  schemaName,
  options,
  metadataPath,
  tags,
  useDuckdbFed,
} = useAddDataSource()

// Dialog state
const activeTab = ref('general')
const selectedTypeId = ref<string | null>(null)
const uriEditing = ref(false)
const manualUri = ref('')
const testResult = ref<{ success: boolean; message: string; latencyMs?: number } | null>(null)
const testing = ref(false)
const saving = ref(false)
const applying = ref(false)
const applyProgress = ref<{ current: number; total: number } | null>(null)
const savingAuth = ref(false) // 防重复调用 create_auth_config
const isEditing = ref(false)
const editingConnId = ref<string | null>(null)
const scopeChangedWarning = ref(false)
const stagingDirty = ref(false) // P0: 暂存项表单脏标记（切换前确认）

// Close confirmation
const showCloseConfirm = ref(false)

// Extra config from child tabs (UI composition state — not in composable)
const driverPropertiesExtra = ref<string | null>(null)
const advancedOptions = ref<string | null>(null)

const { sshProfiles, proxyProfiles } = useNetworkProfiles()

// Test connection modal state
const showTestModal = ref(false)
const lastTestResult = ref<{
  success: boolean
  message: string
  serverVersion?: string
  responseTimeMs?: number
}>({ success: false, message: '' })

const testNetworkInfo = computed(() => {
  if (!networkConfigId.value) return t('navigator.directConnect')
  // 从已加载的网络配置中查找
  const allProfiles = [...sshProfiles.value, ...proxyProfiles.value]
  const profile = allProfiles.find(p => p.id === networkConfigId.value)
  if (!profile) return t('navigator.none')

  const detail = profile.detail || ''
  const typeLabel =
    profile.type === 'ssh'
      ? 'SSH'
      : profile.type === 'proxy'
        ? t('navigator.proxy')
        : profile.type.toUpperCase()

  // 构建路径: RdataStation → SSH(user@host:22) → target
  const dbTarget = String(formData.value.host || formData.value.file_path || '—')
  return `RdataStation → ${typeLabel}(${detail}) → ${dbTarget}`
})

// Computed
const selectedDriver = computed(
  () => drivers.value.find(d => d.id === headerData.selectedDriverId) ?? null
)

const { uriPreview, buildUrl, parseUrl } = useUrlBuilder({
  selectedDriver,
  formData,
  uriEditing,
  manualUri,
})

const driverOptions = computed(() => {
  if (!selectedTypeId.value) return []
  return drivers.value
    .filter(d => d.type_id === selectedTypeId.value && d.enabled)
    .map(d => ({ label: d.name, value: d.id }))
})

// Actions
function onDriverChange(driverId: string) {
  // 同一驱动不重复 reset（防止子组件重复 parse schema）
  if (driverId === headerData.selectedDriverId && Object.keys(formData.value).length > 0) return

  const hasData = Object.keys(formData.value).length > 0
  if (hasData) {
    const d = dialog.warning({
      title: t('navigator.switchDriver') || '切换驱动',
      content: t('navigator.switchDriverConfirm') || '切换驱动将清空已填写的表单数据，是否继续？',
      positiveText: t('navigator.confirm') || '确认',
      negativeText: t('navigator.cancel') || '取消',
      onPositiveClick: () => {
        d.destroy()
        doDriverChange(driverId)
      },
      onNegativeClick: () => {
        d.destroy()
      },
    })
    return
  }
  doDriverChange(driverId)
}

function doDriverChange(driverId: string) {
  formData.value = {}
  testResult.value = null
  authConfigId.value = null
  authMethod.value = 'password'
  selectedEnvId.value = null
  networkConfigId.value = null
  driverPropertiesExtra.value = null
  advancedOptions.value = null
  schemaName.value = null
  options.value = null
  metadataPath.value = null
  tags.value = null
  useDuckdbFed.value = null
  manualUri.value = ''
  uriEditing.value = false
  // Set file DB flag for URI preview
  const d = drivers.value.find(x => x.id === driverId)
  setFileDb(d?.is_file ?? false)
}

/** P1: URL 解析 → 自动填充连接字段 */
function onParseUrl(rawUrl: string) {
  const parsed = parseUrl(rawUrl)
  if (!parsed) {
    message.warning(t('navigator.parseUrlFailed'))
    return
  }

  // 匹配驱动
  if (parsed.driver) {
    const match = drivers.value.find(
      d => d.id.toLowerCase() === parsed.driver || d.type_id.toLowerCase() === parsed.driver
    )
    if (match) {
      headerData.selectedDriverId = match.id
      selectedTypeId.value = match.type_id
      if (match.is_file) setFileDb(true)
    }
  }

  // 填充表单
  const fd: Record<string, unknown> = {}
  if (parsed.isFile) {
    if (parsed.filePath) fd.file_path = parsed.filePath
    if (parsed.database) fd.database = parsed.database
  } else {
    if (parsed.host) fd.host = parsed.host
    if (parsed.port) fd.port = parsed.port
    if (parsed.database) fd.database = parsed.database
    if (parsed.username) fd.username = parsed.username
    if (parsed.password) fd.password = parsed.password
    if (parsed.params) {
      for (const [k, v] of Object.entries(parsed.params)) {
        fd[k] = v
      }
    }
  }
  formData.value = fd
  testResult.value = null
  message.success(t('navigator.parseUrlSuccess'))

  // 关闭 URI 编辑模式
  uriEditing.value = false
}

function onFormData(d: Record<string, unknown>) {
  formData.value = { ...formData.value, ...d }
  if (!headerData.name && d.name) headerData.name = String(d.name)
}

function onExtraConfig(config: Record<string, unknown>) {
  if (config.networkConfigId !== undefined)
    networkConfigId.value = config.networkConfigId as string | null
  if (config.driverProperties !== undefined)
    driverPropertiesExtra.value = config.driverProperties as string | null
  if (config.advancedOptions !== undefined)
    advancedOptions.value = config.advancedOptions as string | null
  if (config.environmentId !== undefined)
    selectedEnvId.value = config.environmentId as string | null
  if (config.schemaName !== undefined) schemaName.value = config.schemaName as string | null
  if (config.options !== undefined) options.value = config.options as string | null
  if (config.metadataPath !== undefined) metadataPath.value = config.metadataPath as string | null
  if (config.tags !== undefined) tags.value = config.tags as string | null
  if (config.useDuckdbFed !== undefined) useDuckdbFed.value = config.useDuckdbFed as boolean
}

function onCapabilityChange(extra: Record<string, unknown>) {
  const current = advancedOptions.value ? JSON.parse(advancedOptions.value) : {}
  advancedOptions.value = JSON.stringify({ ...current, ...extra })
}

function onAuthConfigChange(authCfgId: string | null, method: string) {
  authConfigId.value = authCfgId
  authMethod.value = method
}

function handleAddStaging() {
  addStaging()
  // 新增时清空右侧表单
  headerData.name = ''
  headerData.description = ''
  formData.value = {}
  testResult.value = null
  authConfigId.value = null
  authMethod.value = 'password'
  networkConfigId.value = null
  driverPropertiesExtra.value = null
  advancedOptions.value = null
  selectedEnvId.value = null
  activeTab.value = 'general'
}

function handleRemoveStaging(i: number) {
  const item = stagingItems.value[i]
  if (!item || !item.name) {
    removeStaging(i)
    return
  }
  dialog.warning({
    title: t('navigator.deleteStagingTitle'),
    content: t('navigator.deleteStagingConfirm').replace('{name}', item.name || ''),
    positiveText: t('navigator.confirm'),
    negativeText: t('navigator.cancel'),
    onPositiveClick: () => {
      removeStaging(i)
    },
  })
}

async function handleSelectStaging(i: number) {
  // P0: 当前表单有未保存更改时确认切换
  if (
    stagingDirty.value &&
    i !== stagingIndex.value &&
    stagingItems.value[stagingIndex.value]?.name
  ) {
    const confirmed = await new Promise<boolean>(resolve => {
      dialog.warning({
        title: t('navigator.stagingSwitchTitle') || '切换暂存项',
        content:
          t('navigator.stagingSwitchHint') ||
          '当前表单有未保存的更改，切换后将丢失。确定要切换吗？',
        positiveText: t('common.confirm') || '确定切换',
        negativeText: t('common.cancel') || '取消',
        onPositiveClick: () => {
          resolve(true)
        },
        onNegativeClick: () => {
          resolve(false)
        },
        onClose: () => {
          resolve(false)
        },
      })
    })
    if (!confirmed) return
  }

  selectStaging(i)
  stagingDirty.value = false
  isRestoring.value = true
  try {
    const s = stagingItems.value[i]
    if (!s) return
    headerData.name = s.name || ''
    headerData.description = s.description || ''
    if (s.driver) {
      const d = drivers.value.find(x => x.name.toLowerCase() === s.driver?.toLowerCase())
      if (d) {
        selectedTypeId.value = d.type_id
        headerData.selectedDriverId = d.id
      }
    } else if (s.driverId) {
      headerData.selectedDriverId = s.driverId
      const d = drivers.value.find(x => x.id === s.driverId)
      if (d) selectedTypeId.value = d.type_id
    }
    formData.value = s.formData ? { ...s.formData } : {}
    if (s.scope) {
      if (s.scope === 'both') {
        scope.global = true
        scope.project = true
      } else {
        scope.global = s.scope === 'global'
        scope.project = s.scope === 'project'
      }
    }
    networkConfigId.value = s.networkConfigId ?? null
    driverPropertiesExtra.value = s.driverProperties ?? null
    advancedOptions.value = s.advancedOptions ?? null
    selectedEnvId.value = s.environmentId ?? null
    authConfigId.value = s.authConfigId ?? null
    authMethod.value = s.authMethod ?? 'password'
    schemaName.value = s.schemaName ?? null
    options.value = s.options ?? null
    metadataPath.value = s.metadataPath ?? null
    tags.value = s.tags ?? null
    useDuckdbFed.value = s.useDuckdbFed ?? null
    testResult.value = null
    await nextTick()
  } finally {
    isRestoring.value = false
  }
}

async function handleTest() {
  const validation = validate()
  if (!validation.valid) {
    const firstError = Object.values(validation.errors)[0]
    message.warning(firstError)
    return
  }

  if (!selectedDriver.value) {
    message.warning(t('navigator.selectDbType'))
    return
  }

  // 连接字段前置校验（防止空 host/port 导致超时）
  if (!selectedDriver.value.is_file) {
    const fd = formData.value
    if (!fd.host || String(fd.host).trim() === '') {
      message.warning(t('navigator.validation.hostRequired'))
      return
    }
    const port = Number(fd.port || selectedDriver.value.default_port || 0)
    if (port < 1 || port > 65535) {
      message.warning(t('navigator.validation.portInvalid'))
      return
    }
  }

  const url = buildUrl()
  if (!url) {
    message.warning(t('navigator.urlEmpty'))
    return
  }

  testing.value = true
  try {
    const driverName = selectedDriver.value.name
    const dbType = selectedDriver.value.id
    const params: Record<string, unknown> = {
      dbType: dbType,
      url,
    }
    if (networkConfigId.value) params.networkConfigId = networkConfigId.value
    if (authConfigId.value) params.authConfigId = authConfigId.value
    if (authMethod.value) params.authMethod = authMethod.value
    // 传递密码字段，验证加密存储链路
    const pw = formData.value.password
    if (pw) params.password = String(pw)
    // 传递项目路径，支持项目级网络/认证配置解析
    if (projectStore.currentProject?.path) {
      params.projectPath = projectStore.currentProject.path
    }
    const r = await invoke<{
      success: boolean
      message?: string
      server_version?: string
      response_time_ms?: number
    }>('test_connection', params)
    testResult.value = {
      success: r.success,
      message: r.success
        ? `\u2713 ${t('navigator.connectionSuccess', { name: driverName })} \u2014 ${driverName} \u2014 [本机] \u2192 ${r.message || 'DB'}`
        : r.message || t('navigator.connectionFailedGeneric'),
      latencyMs: r.success ? (r.response_time_ms ?? undefined) : undefined,
    }

    // 存储结构化结果并弹出详细反馈弹窗
    lastTestResult.value = {
      success: r.success,
      message: r.message || '',
      serverVersion: r.server_version,
      responseTimeMs: r.response_time_ms,
    }
    showTestModal.value = true
  } catch (err) {
    const msg = err instanceof Error ? err.message : typeof err === 'string' ? err : String(err)
    console.error('[test_connection] 测试连接失败')
    testResult.value = { success: false, message: msg }
    lastTestResult.value = { success: false, message: msg }
    showTestModal.value = true
  } finally {
    testing.value = false
  }
}

/** 测试连接弹窗关闭回调 — 成功时弹出确认对话框再决定是否保存认证 */
async function onTestModalClose() {
  showTestModal.value = false

  if (savingAuth.value) return
  if (!lastTestResult.value?.success) return

  // 测试成功后自动同步当前表单到暂存
  syncCurrentToStaging()

  const fd = formData.value
  const authType = authMethod.value
  const hasAuth =
    isAuthRequired(authType) &&
    (fd.username || fd.password || fd.certPath || fd.principal || fd.tokenEndpoint)
  if (!hasAuth) return

  // 如果已经选择了认证配置，不需要再次询问保存
  if (authConfigId.value) return

  const d = dialog.info({
    title: t('navigator.testSuccess'),
    content: t('navigator.saveAuthConfirm'),
    positiveText: t('navigator.confirm'),
    negativeText: t('navigator.cancel'),
    onPositiveClick: async () => {
      d.loading = true
      await doSaveAuth(authType, fd)
      d.loading = false
    },
    onNegativeClick: () => {
      d.destroy()
    },
  })
}

/** 判断认证方式是否需要凭据 */
function isAuthRequired(authMethod: string): boolean {
  return ['password', 'ldap', 'pg_class', 'kerberos', 'oauth2'].includes(authMethod)
}

/** 实际执行认证保存 */
async function doSaveAuth(authType: string, fd: Record<string, unknown>) {
  if (savingAuth.value) return
  savingAuth.value = true
  try {
    const authData = buildAuthData(authType, fd)

    const driverName = selectedDriver.value?.name || ''
    const authName = headerData.name ? `${headerData.name} — 认证` : `${driverName} — 认证`

    const authDataStr = JSON.stringify(authData)

    const shouldSaveGlobal = scope.global
    const shouldSaveProject = scope.project && projectStore.hasProject

    if (shouldSaveGlobal && shouldSaveProject) {
      const _globalId = await invoke<{ id: string }>('create_auth_config', {
        ac: {
          id: '',
          name: `${authName} (全局)`,
          auth_type: authType,
          auth_data: authDataStr,
          created_at: '',
          updated_at: '',
        },
      })

      const pp = projectStore.currentProject?.path
      if (!pp) {
        message.warning(t('navigator.noProjectPath') || '无法获取项目路径，跳过项目认证保存')
        return
      }
      const projectId = await invoke<{ id: string }>('project_create_auth_config', {
        name: `${authName} (项目)`,
        authType,
        authData: authDataStr,
        projectPath: pp,
      })

      authConfigId.value = projectId.id
      authMethod.value = authType
      message.info(t('navigator.authSavedHint', { global: true, project: true }))
    } else if (shouldSaveGlobal) {
      const created = await invoke<{ id: string }>('create_auth_config', {
        ac: {
          id: '',
          name: authName,
          auth_type: authType,
          auth_data: authDataStr,
          created_at: '',
          updated_at: '',
        },
      })
      authConfigId.value = created.id
      authMethod.value = authType
      message.info(t('navigator.authSavedHint', { global: true, project: false }))
    } else if (shouldSaveProject) {
      const pp = projectStore.currentProject?.path
      if (!pp) {
        message.warning(t('navigator.noProjectPath') || '无法获取项目路径，跳过项目认证保存')
        return
      }
      const created = await invoke<{ id: string }>('project_create_auth_config', {
        name: authName,
        authType,
        authData: authDataStr,
        projectPath: pp,
      })
      authConfigId.value = created.id
      authMethod.value = authType
      message.info(t('navigator.authSavedHint', { global: false, project: true }))
    }
  } catch (authErr) {
    const msg = authErr instanceof Error ? authErr.message : String(authErr)
    console.error('[auth] 保存认证配置失败:', msg)
    message.error(`${t('navigator.authSaveFailed')}: ${msg}`)
  } finally {
    savingAuth.value = false
  }
}

/** 认证类型字段映射 */
const AUTH_TYPE_FIELDS: Record<string, string[]> = {
  password: ['username', 'password'],
  ldap: ['username', 'password'],
  pg_class: ['certPath', 'certKeyPath'],
  kerberos: ['principal', 'keytabPath'],
  oauth2: ['tokenEndpoint', 'clientId', 'clientSecret'],
  ssh_password: ['username', 'password'],
  proxy_password: ['username', 'password'],
}

/** 构建认证数据对象 */
function buildAuthData(authType: string, fd: Record<string, unknown>): Record<string, unknown> {
  const fields = AUTH_TYPE_FIELDS[authType] ?? []
  const authData: Record<string, unknown> = {}
  for (const f of fields) {
    if (fd[f]) authData[f] = String(fd[f])
  }
  return authData
}

/** 保存到暂存列表（不连库，不关对话框） */
function saveToStaging() {
  if (!selectedDriver.value) {
    message.warning(t('navigator.selectDbType'))
    return
  }

  const validation = validate()
  if (!validation.valid) {
    const firstError = Object.values(validation.errors)[0]
    message.warning(firstError)
    return
  }

  if (!scope.global && !scope.project) {
    message.warning(t('navigator.selectSaveLocation'))
    return
  }

  // 连接字段前置校验（与 handleTest 保持一致，防止空 host/port 导致保存无效连接）
  if (!selectedDriver.value.is_file) {
    const fd = formData.value
    if (!fd.host || String(fd.host).trim() === '') {
      message.warning(t('navigator.validation.hostRequired'))
      return
    }
    const port = Number(fd.port || selectedDriver.value.default_port || 0)
    if (port < 1 || port > 65535) {
      message.warning(t('navigator.validation.portInvalid'))
      return
    }
  }

  const url = buildUrl()
  if (!url) {
    message.warning(t('navigator.urlEmpty'))
    return
  }
  const name = headerData.name || selectedDriver.value.name
  const d = selectedDriver.value

  const item = buildStagingItem(
    name,
    d.id,
    headerData.selectedDriverId ?? undefined,
    url,
    { ...formData.value },
    authConfigId.value,
    authMethod.value,
    networkConfigId.value,
    driverPropertiesExtra.value,
    advancedOptions.value,
    selectedEnvId.value ?? null,
    headerData.description || undefined,
    schemaName.value || undefined,
    options.value || undefined,
    metadataPath.value || undefined,
    tags.value || undefined,
    useDuckdbFed.value ?? false
  )

  const current = stagingItems.value[stagingIndex.value]
  if (current && current.name) {
    // 当前项已有内容 → 追加新项
    stagingItems.value.push(item)
    stagingIndex.value = stagingItems.value.length - 1
  } else {
    // 当前项为空或不存在 → 原地覆盖
    stagingItems.value[stagingIndex.value] = item
  }

  message.success(t('navigator.savedToStaging', { name }))
  stagingDirty.value = false
}

async function handleSave() {
  saveToStaging()
}

function syncCurrentToStaging() {
  const idx = stagingIndex.value
  const name = headerData.name || stagingItems.value[idx]?.name || selectedDriver.value?.name || ''

  stagingItems.value[idx] = buildStagingItem(
    name,
    selectedDriver.value?.id,
    headerData.selectedDriverId ?? undefined,
    buildUrl(),
    { ...formData.value },
    authConfigId.value,
    authMethod.value,
    networkConfigId.value,
    driverPropertiesExtra.value,
    advancedOptions.value,
    selectedEnvId.value ?? null,
    headerData.description || undefined,
    schemaName.value || undefined,
    options.value || undefined,
    metadataPath.value || undefined,
    tags.value || undefined,
    useDuckdbFed.value ?? false
  )
}

/** 应用操作：新连接创建 或 已有连接更新 */
async function handleApply() {
  if (isEditing.value && editingConnId.value) {
    await handleEditApply()
    return
  }

  await handleCreateApply()
}

/** 编辑模式下：更新已有连接 */
async function handleEditApply() {
  if (!editingConnId.value) return

  syncCurrentToStaging()

  const validation = validate()
  if (!validation.valid) {
    const firstError = Object.values(validation.errors)[0]
    message.warning(firstError)
    return
  }

  if (!scope.global && !scope.project) {
    message.warning(t('navigator.selectSaveLocation'))
    return
  }

  applying.value = true
  try {
    const _url = buildUrl()
    const name = headerData.name || selectedDriver.value?.name || ''
    const fd = formData.value

    let projectOk = !scope.project
    let globalOk = !scope.global
    let projectErr = ''
    let globalErr = ''

    if (scope.project) {
      try {
        const conn: ProjectConnection = {
          id: editingConnId.value,
          name,
          driver: selectedDriver.value?.id || '',
          host: String(fd.host || ''),
          port: Number(fd.port || 0),
          database: String(fd.database || ''),
          schema_name: schemaName.value || (fd.schema_name as string) || undefined,
          username: String(fd.username || ''),
          password: fd.password ? String(fd.password) : undefined,
          options: options.value || (fd.options as string) || undefined,
          tags: tags.value || (fd.tags as string) || undefined,
          use_duckdb_fed: useDuckdbFed.value ?? (fd.use_duckdb_fed as boolean) ?? false,
          metadata_path: metadataPath.value || (fd.metadata_path as string) || undefined,
          description: headerData.description || undefined,
          driver_id: headerData.selectedDriverId || undefined,
          environment_id: selectedEnvId.value ?? undefined,
          auth_config_id: authConfigId.value ?? undefined,
          auth_method: authMethod.value ?? undefined,
          network_config_id: networkConfigId.value ?? undefined,
          driver_properties: driverPropertiesExtra.value ?? undefined,
          advanced_options: advancedOptions.value ?? undefined,
          server_version: (fd.server_version as string) || undefined,
          created_at: '',
          updated_at: new Date().toISOString(),
        }
        await projectConnectionStore.updateConnection(conn)
        projectOk = true
      } catch (e: unknown) {
        projectErr = e instanceof Error ? e.message : String(e)
      }
    }

    if (scope.global) {
      try {
        await updateGlobalConnection({
          conn_id: editingConnId.value,
          name,
          driver: selectedDriver.value?.id,
          host: String(fd.host || ''),
          port: Number(fd.port || 0),
          database: String(fd.database || ''),
          schema_name: schemaName.value || (fd.schema_name as string) || undefined,
          username: String(fd.username || ''),
          password: fd.password ? String(fd.password) : undefined,
          options: options.value || (fd.options as string) || undefined,
          tags:
            tags.value || (fd.tags as string)
              ? (tags.value || (fd.tags as string))
                  .split(',')
                  .map(s => s.trim())
                  .filter(Boolean)
              : undefined,
          use_duckdb_fed: useDuckdbFed.value ?? (fd.use_duckdb_fed as boolean) ?? undefined,
          metadata_path: metadataPath.value || (fd.metadata_path as string) || undefined,
          driver_id: headerData.selectedDriverId,
          environment_id: selectedEnvId.value ?? undefined,
          auth_config_id: authConfigId.value ?? undefined,
          auth_method: authMethod.value ?? undefined,
          network_config_id: networkConfigId.value ?? undefined,
          driver_properties: driverPropertiesExtra.value ?? undefined,
          advanced_options: advancedOptions.value ?? undefined,
          description: headerData.description || undefined,
          server_version: (fd.server_version as string) || undefined,
        })
        globalOk = true
      } catch (e: unknown) {
        globalErr = e instanceof Error ? e.message : String(e)
      }
    }

    if (projectOk && globalOk) {
      message.success(t('navigator.applySuccess', { count: 1 }))
      emit('save')
      emit('connectionsChanged')
      resetAndClose()
    } else if (!projectOk && !globalOk) {
      message.error(t('navigator.applyFailed', { error: projectErr || globalErr }))
    } else {
      const failed = !projectOk ? t('navigator.projectConnection') : t('navigator.globalConnection')
      const success = projectOk ? t('navigator.projectConnection') : t('navigator.globalConnection')
      const errDetail = projectErr || globalErr
      message.warning(t('navigator.applyPartialSuccess', { success, failed, error: errDetail }))
      emit('save')
      emit('connectionsChanged')
      resetAndClose()
    }
  } catch (e) {
    message.error(`${t('common.operationFailed')}: ${(e as Error).message}`)
  } finally {
    applying.value = false
  }
}

/** 新增模式：批量应用所有暂存连接 → 写入数据库 */
async function handleCreateApply() {
  // P0: 在 syncCurrentToStaging 前捕获当前表单密码（暂存项 formData 会剥离密码）
  const currentPassword = formData.value.password ? String(formData.value.password) : undefined

  syncCurrentToStaging()

  const validItems = stagingItems.value.filter(item => item.name)
  if (validItems.length === 0) {
    message.warning(t('navigator.noStagingToApply'))
    return
  }

  // P0: 批内连接名称重复校验
  const nameSet = new Set<string>()
  for (const item of validItems) {
    if (!item.name) continue
    const lower = item.name.toLowerCase().trim()
    if (nameSet.has(lower)) {
      message.warning(t('navigator.duplicateName', { name: item.name }))
      return
    }
    nameSet.add(lower)
  }

  applying.value = true
  let successCount = 0
  const errors: string[] = []
  applyProgress.value = { current: 0, total: validItems.length }

  try {
    for (let idx = 0; idx < validItems.length; idx++) {
      const item = validItems[idx]
      const originalIndex = stagingItems.value.findIndex(i => i.id === item.id)
      applyProgress.value = { current: idx + 1, total: validItems.length }

      try {
        const driverName = item.driver || 'mysql'
        const url = item.url || ''
        const name = item.name

        // 当前对话框 scope 勾选为权威来源，StagingItem.scope 仅用于初始展示
        const shouldSaveGlobal = scope.global
        const shouldSaveProject = scope.project

        let itemSuccess = false

        if (shouldSaveProject && !shouldSaveGlobal) {
          await saveToProjectOnly(item, driverName, url, name, errors, currentPassword)
          itemSuccess = true
        } else {
          let snapshotNetId = item.networkConfigId ?? null
          let snapshotAuthId = item.authConfigId ?? null

          if (shouldSaveProject && projectStore.hasProject) {
            const pp = projectStore.currentProject?.path
            snapshotAuthId = await snapshotIfNeeded(snapshotAuthId, 'auth', pp, name, errors)
            snapshotNetId = await snapshotIfNeeded(snapshotNetId, 'network', pp, name, errors)

            if (snapshotAuthId === 'failed' || snapshotNetId === 'failed') {
              continue
            }
          }

          const connectOpts = buildConnectOpts(item, snapshotNetId, snapshotAuthId, currentPassword)

          let globalConnId: string | null = null
          if (shouldSaveGlobal) {
            try {
              const result = await connectDatabaseService(
                driverName,
                url,
                name,
                'global',
                undefined,
                connectOpts
              )
              globalConnId = result.conn_id
            } catch (e) {
              errors.push(`${name} (全局): ${String(e)}`)
              if (!shouldSaveProject) continue
            }
          }

          if (shouldSaveProject && projectStore.hasProject) {
            try {
              await saveToProject(
                item,
                driverName,
                url,
                name,
                snapshotNetId,
                snapshotAuthId,
                currentPassword
              )
            } catch (e) {
              errors.push(`${name} (项目): ${String(e)}`)
              if (globalConnId) {
                try {
                  await closeConnection(globalConnId)
                } catch {
                  /* cleanup error ignored */
                }
              }
            }
          }

          if (!errors.some(err => err.startsWith(name))) {
            itemSuccess = true
          }
        }

        if (itemSuccess) {
          successCount++
          if (originalIndex !== -1) {
            markStagingApplied(originalIndex)
          }
        }
      } catch (e) {
        errors.push(`${name}: ${String(e)}`)
      }
    }

    if (errors.length === 0) {
      message.success(t('navigator.applySuccess', { count: successCount }))
    } else if (successCount > 0) {
      message.warning(t('navigator.applyPartial', { success: successCount, fail: errors.length }))
    } else {
      message.error(`${t('common.operationFailed')}: ${errors.join('; ')}`)
    }

    if (successCount > 0) {
      emit('save')
      emit('connectionsChanged')
      resetAndClose()
    }
  } catch (e) {
    message.error(`${t('common.operationFailed')}: ${(e as Error).message}`)
  } finally {
    applying.value = false
    applyProgress.value = null
  }
}

async function snapshotIfNeeded(
  configId: string | null,
  type: 'auth' | 'network',
  projectPath: string | undefined,
  name: string,
  errors: string[]
): Promise<string | null> {
  if (!configId?.startsWith('G_') || configId.startsWith('GP_')) {
    return configId
  }

  const invokeFn = type === 'auth' ? 'snapshot_global_auth' : 'snapshot_global_network'
  const paramName = type === 'auth' ? 'globalAuthId' : 'globalNetId'

  try {
    const r = await invoke<{ snapshot_id: string }>(invokeFn, {
      [paramName]: configId,
      projectPath,
    })
    return r.snapshot_id
  } catch (e) {
    const detail = e instanceof Error ? e.message : String(e)
    console.error(`[snapshotIfNeeded] ${type} 快照失败:`, detail, e)
    errors.push(`${name}: ${type === 'auth' ? '认证' : '网络'}配置快照失败 (${detail})`)
    return 'failed'
  }
}

function buildConnectOpts(
  item: StagingItem,
  networkConfigId: string | null,
  authConfigId: string | null,
  /** 当前表单密码（暂存项 formData 已剥离密码，需从当前表单独立传入） */
  currentPassword?: string
) {
  const fd = item.formData || {}
  return {
    driverId: item.driverId,
    networkConfigId,
    environmentId: item.environmentId ?? undefined,
    authConfigId: authConfigId ?? undefined,
    authMethod: item.authMethod ?? undefined,
    driverProperties: item.driverProperties ?? undefined,
    advancedOptions: item.advancedOptions ?? undefined,
    description: item.description || undefined,
    options: item.options || undefined,
    tags: item.tags || undefined,
    metadataPath: item.metadataPath || undefined,
    schemaName: item.schemaName || undefined,
    useDuckdbFed: item.useDuckdbFed ?? false,
    password: currentPassword || (fd.password ? String(fd.password) : undefined),
  }
}

async function saveToProjectOnly(
  item: StagingItem,
  driverName: string,
  url: string,
  name: string,
  errors: string[],
  currentPassword?: string
) {
  if (!projectStore.hasProject) {
    errors.push(`${name}: 没有打开的项目`)
    return
  }

  const pp = projectStore.currentProject?.path
  const snapshotNetId = await snapshotIfNeeded(
    item.networkConfigId ?? null,
    'network',
    pp,
    name,
    errors
  )
  const snapshotAuthId = await snapshotIfNeeded(item.authConfigId ?? null, 'auth', pp, name, errors)

  if (snapshotAuthId === 'failed' || snapshotNetId === 'failed') {
    return
  }

  const fd = item.formData || {}

  // 如果没有使用已保存的认证配置，且用户填写了认证信息，则保存新的认证配置
  let finalAuthConfigId = snapshotAuthId
  const finalAuthMethod = item.authMethod ?? authMethod.value
  const hasAuthData = (fd.username && fd.password) || fd.certPath || fd.principal
  if (!finalAuthConfigId && hasAuthData) {
    try {
      const authName = `${name} (认证)`
      const authData = buildAuthData(finalAuthMethod, fd as Record<string, unknown>)

      const r = await invoke<{ id: string }>('project_create_auth_config', {
        name: authName,
        authType: finalAuthMethod,
        authData: JSON.stringify(authData),
        projectPath: pp,
      })
      finalAuthConfigId = r.id
    } catch (e) {
      console.warn('[AddDataSource] 保存认证配置失败:', e)
    }
  }

  const conn = await projectConnectionStore.createConnection({
    name,
    driver: driverName,
    host: fd.host && String(fd.host).trim() ? String(fd.host) : undefined,
    port: fd.port && Number(fd.port) > 0 ? Number(fd.port) : undefined,
    database: fd.database && String(fd.database).trim() ? String(fd.database) : undefined,
    schema_name: item.schemaName || undefined,
    username: fd.username && String(fd.username).trim() ? String(fd.username) : undefined,
    password:
      currentPassword || (fd.password && String(fd.password))
        ? currentPassword || String(fd.password)
        : undefined,
    options: item.options || undefined,
    tags: item.tags || undefined,
    use_duckdb_fed: item.useDuckdbFed ?? false,
    metadata_path: item.metadataPath || undefined,
    description: item.description || undefined,
    driver_id: item.driverId,
    environment_id: item.environmentId ?? undefined,
    auth_config_id: finalAuthConfigId ?? undefined,
    auth_method: finalAuthMethod ?? undefined,
    network_config_id: snapshotNetId ?? undefined,
    driver_properties: item.driverProperties ?? undefined,
    advanced_options: item.advancedOptions ?? undefined,
  })

  await projectConnectionStore.loadConnections()

  // 自动建立项目连接
  if (pp) {
    try {
      await connectDatabaseService(
        driverName,
        url,
        name,
        'project',
        pp,
        buildConnectOpts(item, snapshotNetId, finalAuthConfigId, currentPassword)
      )
    } catch (e) {
      console.warn('[AddDataSource] 自动建立项目连接失败:', e)
    }
  }

  return conn
}

async function saveToProject(
  item: StagingItem,
  driverName: string,
  url: string,
  name: string,
  networkConfigId: string | null,
  authConfigId: string | null,
  currentPassword?: string
) {
  const pp = projectStore.currentProject?.path

  // 先对全局配置做快照
  const snapshotNetId = await snapshotIfNeeded(networkConfigId, 'network', pp, name, [])
  const snapshotAuthId = await snapshotIfNeeded(authConfigId, 'auth', pp, name, [])

  const fd = item.formData || {}
  const conn = await projectConnectionStore.createConnection({
    name,
    driver: driverName,
    host: fd.host && String(fd.host).trim() ? String(fd.host) : undefined,
    port: fd.port && Number(fd.port) > 0 ? Number(fd.port) : undefined,
    database: fd.database && String(fd.database).trim() ? String(fd.database) : undefined,
    schema_name: item.schemaName || undefined,
    username: fd.username && String(fd.username).trim() ? String(fd.username) : undefined,
    password:
      currentPassword || (fd.password && String(fd.password))
        ? currentPassword || String(fd.password)
        : undefined,
    options: item.options || undefined,
    tags: item.tags || undefined,
    use_duckdb_fed: item.useDuckdbFed ?? false,
    metadata_path: item.metadataPath || undefined,
    description: item.description || undefined,
    driver_id: item.driverId,
    environment_id: item.environmentId ?? undefined,
    auth_config_id: (snapshotAuthId !== 'failed' ? snapshotAuthId : authConfigId) ?? undefined,
    auth_method: item.authMethod ?? undefined,
    network_config_id: (snapshotNetId !== 'failed' ? snapshotNetId : networkConfigId) ?? undefined,
    driver_properties: item.driverProperties ?? undefined,
    advanced_options: item.advancedOptions ?? undefined,
  })

  await projectConnectionStore.loadConnections()

  // 自动建立项目连接
  if (pp) {
    try {
      await connectDatabaseService(
        driverName,
        url,
        name,
        'project',
        pp,
        buildConnectOpts(
          item,
          snapshotNetId !== 'failed' ? snapshotNetId : networkConfigId,
          snapshotAuthId !== 'failed' ? snapshotAuthId : authConfigId,
          currentPassword
        )
      )
    } catch (e) {
      console.warn('[AddDataSource] 自动建立项目连接失败:', e)
    }
  }

  return conn
}

function resetAndClose() {
  testResult.value = null
  headerData.name = ''
  headerData.description = ''
  formData.value = {}
  isEditing.value = false
  editingConnId.value = null
  clearStagingItems()
  emit('update:modelValue', false)
}

function handleClose() {
  const hasChanges =
    stagingDirty.value || stagingItems.value.some(item => !item.applied && item.name)
  if (hasChanges) {
    showCloseConfirm.value = true
    return
  }
  resetAndClose()
}

function confirmClose() {
  showCloseConfirm.value = false
  resetAndClose()
}

function cancelClose() {
  showCloseConfirm.value = false
}

/** 从已有 ProjectConnection 初始化编辑表单 */
async function initFromConnection(conn: ProjectConnection) {
  isEditing.value = true
  editingConnId.value = conn.id

  headerData.name = conn.name || ''
  headerData.description = conn.description || ''
  headerData.selectedDriverId = conn.driver_id || ''

  scope.global = conn.connection_type === 'global'
  scope.project = conn.connection_type !== 'global'

  const d = drivers.value.find(x => x.id === conn.driver_id)
  if (d) selectedTypeId.value = d.type_id

  const defaultPort = d?.default_port ?? 0

  formData.value = {
    host: conn.host ?? '',
    port: conn.port ?? defaultPort,
    database: conn.database ?? '',
    username: conn.username ?? '',
    password: conn.password ?? '',
    schema_name: conn.schema_name ?? null,
    options: conn.options ?? null,
    metadata_path: conn.metadata_path ?? null,
    tags: conn.tags ?? null,
    use_duckdb_fed: conn.use_duckdb_fed ?? false,
    server_version: conn.server_version ?? null,
  }

  authConfigId.value = conn.auth_config_id ?? null
  authMethod.value = conn.auth_method ?? 'password'
  networkConfigId.value = conn.network_config_id ?? null
  driverPropertiesExtra.value = conn.driver_properties ?? null
  advancedOptions.value = conn.advanced_options ?? null
  selectedEnvId.value = conn.environment_id ?? null
  schemaName.value = conn.schema_name ?? null
  options.value = conn.options ?? null
  metadataPath.value = conn.metadata_path ?? null
  tags.value = conn.tags ?? null
  useDuckdbFed.value = conn.use_duckdb_fed ?? false

  testResult.value = null
  await nextTick()
}

// Init
onMounted(async () => {
  await loadAll(projectStore.currentProject?.path)
})

watch(
  () => props.modelValue,
  async open => {
    if (open) {
      isRestoring.value = true
      stagingDirty.value = false
      loadStagingItems()
      await loadAll(projectStore.currentProject?.path)
      activeTab.value = 'general'
      testResult.value = null
      networkConfigId.value = null
      driverPropertiesExtra.value = null
      advancedOptions.value = null
      authConfigId.value = null
      authMethod.value = 'password'
      selectedEnvId.value = null
      manualUri.value = ''
      uriEditing.value = false
      editingConnId.value = null
      if (props.initialConnection) {
        await initFromConnection(props.initialConnection)
      } else if (props.initialDriver) {
        isEditing.value = false
        selectedTypeId.value = props.initialDriver.type_id
        headerData.selectedDriverId = props.initialDriver.id
      } else {
        isEditing.value = false
      }
      await nextTick()
      isRestoring.value = false
    }
  },
  { immediate: true }
)

watch(uriEditing, editing => {
  if (editing) {
    manualUri.value = uriPreview.value
  }
})

// T1: 暂存列表名字实时跟随右侧名称（flush:sync 确保表单重置时同步拦截）
watch(
  () => headerData.name,
  name => {
    if (stagingItems.value[stagingIndex.value]) {
      stagingItems.value[stagingIndex.value].name = name
    }
  },
  { flush: 'sync' }
)

// T6: 选择数据源类型时自动选中第一个驱动
watch(selectedTypeId, typeId => {
  if (!typeId) return
  // 仅在未选择驱动或当前驱动不属于该类型时自动选中
  const currentDriver = drivers.value.find(d => d.id === headerData.selectedDriverId)
  if (currentDriver && currentDriver.type_id === typeId) return
  const firstDriver = drivers.value.find(d => d.type_id === typeId && d.enabled)
  if (firstDriver) {
    headerData.selectedDriverId = firstDriver.id
    formData.value = {}
    testResult.value = null
  }
})

watch(
  () => ({ global: scope.global, project: scope.project }),
  (_newVal, oldVal) => {
    // 仅在 scope 实际发生变化且用户已填写了网络/环境/存储配置时显示警告
    if (!oldVal || (_newVal.global === oldVal.global && _newVal.project === oldVal.project)) return
    const hasExtra =
      networkConfigId.value ||
      selectedEnvId.value ||
      driverPropertiesExtra.value ||
      advancedOptions.value
    if (hasExtra) {
      scopeChangedWarning.value = true
    } else {
      scopeChangedWarning.value = false
    }
  }
)
// P0: 表单脏标记 — 暂存项切换前确认（监控所有可编辑字段）
const isRestoring = ref(false) // 数据恢复期间暂停脏标记
watch(
  [
    formData,
    headerData,
    selectedEnvId,
    networkConfigId,
    authConfigId,
    authMethod,
    driverPropertiesExtra,
    advancedOptions,
    schemaName,
    options,
    metadataPath,
    tags,
    useDuckdbFed,
  ],
  () => {
    if (isRestoring.value) return
    stagingDirty.value = true
  },
  { deep: true }
)
</script>

<style scoped>
/* ===== Card shell (matches settings-card pattern) ===== */
.datasource-card {
  display: flex;
  flex-direction: column;
  width: min(980px, calc(100vw - 48px));
  max-height: 88vh;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-md);
  overflow: hidden;
}

.card-header {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: 4px var(--spacing-md);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.card-icon {
  color: var(--brand-accent);
  flex-shrink: 0;
}

.card-title {
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-primary);
  white-space: nowrap;
}

.card-body {
  display: flex;
  height: 460px;
  min-height: 400px;
  overflow: hidden;
}

/* ===== Right panel ===== */
.dlg-right {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-width: 0;
}

.scope-warning {
  margin: var(--spacing-xs) var(--spacing-xs) 0;
  flex-shrink: 0;
}

/* ===== Tabs ===== */
.dlg-tabs {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-height: 0;
}

.dlg-tabs :deep(.n-tabs-nav) {
  flex-shrink: 0;
  padding-left: var(--spacing-xs);
}
.dlg-tabs :deep(.n-tabs-content) {
  flex: 1;
  min-height: 0;
}
.dlg-tabs :deep(.n-tab-pane) {
  height: 100%;
  overflow-y: auto;
  padding: 10px;
  box-sizing: border-box;
}

/* ===== Footer ===== */
.card-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  padding: var(--spacing-sm) var(--spacing-md);
  border-top: 1px solid var(--color-border);
  flex-shrink: 0;
  gap: var(--spacing-sm);
}

.test-result {
  font-size: var(--font-size-sm);
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  margin-right: auto;
}

.test-icon.ok {
  color: var(--brand-success);
}
.test-icon.fail {
  color: var(--brand-danger);
}
.test-msg {
  color: var(--color-text-secondary);
}
.test-latency {
  color: var(--brand-accent);
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
}

.footer-spacer {
  flex: 1;
}

.apply-progress {
  font-size: var(--font-size-xs);
  color: var(--brand-accent);
  white-space: nowrap;
}
</style>
