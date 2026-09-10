<template>
  <div class="adv-sec env-sec">
    <EnvironmentSelector
      :model-value="environmentId"
      :options="envSelectOpts"
      :label="$t('navigator.advancedEnv')"
      :manage-label="$t('navigator.manage')"
      @update:model-value="onEnvChange"
      @manage="showEnvMgr = true"
    />
    <div v-if="envPolicyTags.length" class="env-tags">
      <span v-for="tag in envPolicyTags" :key="tag.key" :class="['epi-tag', tag.kind]">{{
        tag.label
      }}</span>
    </div>
    <div v-if="!isPolicyOverridden" class="env-preset-indicator"
      >← 🟢 {{ currentEnvDef.name }} 预设</div
    >
    <div v-if="isPolicyOverridden" class="env-override-hint"
      >⚠ {{ $t('navigator.envOverrideHint', { name: currentEnvDef.name }) }}</div
    >
    <div v-if="envSnapshotting" class="env-snapshot-hint">📸 正在快照全局环境...</div>
    <div v-else-if="envSnapshotId" class="env-snapshot-hint">📸 已快照为 {{ envSnapshotId }}</div>

    <EnvironmentManager
      v-model="showEnvMgr"
      :title="$t('navigator.envManager') || '环境管理器'"
      :loading="envListLoading"
      :loading-text="$t('dataPreview.loading')"
      :environments="loadedEnvs"
      :builtin-badge="$t('navigator.builtinBadge') || '内置'"
      :show-create-form="showEnvCreateForm"
      :editing="!!editingEnvId"
      :create-label="$t('navigator.createEnv')"
      :name-label="$t('navigator.envName')"
      :name-placeholder="$t('navigator.envNamePlaceholder') || '输入环境名称'"
      :icon-label="$t('navigator.envIcon')"
      :color-label="$t('navigator.envColor')"
      :desc-label="$t('navigator.envDesc')"
      :desc-placeholder="$t('navigator.envDescPlaceholder') || '输入环境描述'"
      :template-label="$t('navigator.envTemplate')"
      :save-label="editingEnvId ? $t('common.save') || '保存修改' : $t('common.save') || '保存'"
      :cancel-label="$t('common.cancel')"
      :new-name="newEnvName"
      :new-icon="newEnvIcon"
      :new-color="newEnvColor"
      :new-desc="newEnvDesc"
      :new-template="newEnvTemplate"
      :template-options="envTemplateOpts"
      @update:model-value="v => (showEnvMgr = v)"
      @update:new-name="v => (newEnvName = v)"
      @update:new-icon="v => (newEnvIcon = v)"
      @update:new-color="v => (newEnvColor = v)"
      @update:new-desc="v => (newEnvDesc = v)"
      @update:new-template="v => (newEnvTemplate = v)"
      @toggle-create="toggleEnvForm"
      @create="handleCreateEnv"
      @edit="handleEditEnv"
      @delete="handleDeleteEnv"
    />
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { useMessage } from 'naive-ui'
import { ref, computed, onMounted } from 'vue'

import { envDefs, envPolicyTagsMap, envDefsAsEnvInfo } from '../../../constants/envDefaults'
import { useEnvironmentStore } from '../../../stores/environmentStore'
import EnvironmentManager from '../EnvironmentManager.vue'
import EnvironmentSelector from '../EnvironmentSelector.vue'

import type { Driver } from '../../../../domain/types'
import type { EnvInfo } from '../EnvironmentManager.vue'
import type { SelectOption } from 'naive-ui'

const environmentStore = useEnvironmentStore()
const message = useMessage()

const props = defineProps<{
  environmentId: string | null
  driver?: Driver | null
  scope?: { global: boolean; project: boolean }
}>()

const emit = defineEmits<{
  'update:environmentId': [id: string]
  'env-change': [payload: { envId: string; selectedEnvId: string; envSnapshotId: string | null }]
}>()

// ========== Environment state ==========
const selectedEnvId = ref<string | null>(null)
const envSnapshotting = ref(false)
const envSnapshotId = ref<string | null>(null)
const showEnvMgr = ref(false)

const envSelectOpts = computed<SelectOption[]>(() =>
  envDefs.map(e => ({
    label: `${e.icon} ${e.name}`,
    value: e.id,
  }))
)

const currentEnvDef = computed(() => envDefs.find(e => e.id === props.environmentId) || envDefs[0])

const envPolicyTags = computed(
  () => (props.environmentId ? envPolicyTagsMap[props.environmentId] : []) || []
)

// 策略覆盖检测（内联简化版，避免引入 useSecurityPolicies 的复杂度）
const isPolicyOverridden = ref(false)
function setPolicyOverridden(v: boolean) {
  isPolicyOverridden.value = v
}
defineExpose({ setPolicyOverridden })

async function onEnvChange(id: string) {
  // 项目级连接引用全局环境 → 触发快照
  if (props.scope?.project && id.startsWith('G_') && !id.startsWith('GP_')) {
    envSnapshotting.value = true
    try {
      const { useProjectStore } = await import('@/core/project/stores/project')
      const pp = useProjectStore().currentProject?.path
      const r = await invoke<{ snapshot_id: string }>('snapshot_global_env', {
        globalEnvId: id,
        projectPath: pp,
      })
      const gpId = r.snapshot_id
      envSnapshotId.value = gpId
      selectedEnvId.value = gpId
      environmentStore.selectEnv(gpId)
      loadEnvironments()
      emit('update:environmentId', gpId)
      emit('env-change', {
        envId: id,
        selectedEnvId: gpId,
        envSnapshotId: gpId,
      })
    } finally {
      envSnapshotting.value = false
    }
    return
  }
  selectedEnvId.value = id
  envSnapshotId.value = null
  environmentStore.selectEnv(id)
  emit('update:environmentId', id)
  emit('env-change', {
    envId: id,
    selectedEnvId: id,
    envSnapshotId: null,
  })
}

// ========== Env Manager state ==========
const loadedEnvs = ref<EnvInfo[]>([])
const envListLoading = ref(false)
const showEnvCreateForm = ref(false)
const editingEnvId = ref<string | null>(null)
const newEnvName = ref('')
const newEnvIcon = ref('🟢')
const newEnvColor = ref('#a6e3a1')
const newEnvDesc = ref('')
const newEnvTemplate = ref('dev')
const envTemplateOpts = [
  { label: '🟢 开发环境 (宽松)', value: 'dev' },
  { label: '🟡 测试环境 (适中)', value: 'test' },
  { label: '🔵 预发布 (较严)', value: 'staging' },
  { label: '🔴 生产环境 (最严)', value: 'prod' },
  { label: '🟣 沙箱环境 (隔离)', value: 'sandbox' },
]

function resetEnvForm() {
  newEnvName.value = ''
  newEnvIcon.value = '🟢'
  newEnvColor.value = '#a6e3a1'
  newEnvDesc.value = ''
  newEnvTemplate.value = 'dev'
  editingEnvId.value = null
}

function toggleEnvForm() {
  if (showEnvCreateForm.value) {
    showEnvCreateForm.value = false
    resetEnvForm()
  } else {
    resetEnvForm()
    showEnvCreateForm.value = true
  }
}

function handleEditEnv(env: EnvInfo) {
  editingEnvId.value = env.id
  newEnvName.value = env.name
  newEnvIcon.value = env.icon
  newEnvColor.value = env.color
  newEnvDesc.value = env.desc
  newEnvTemplate.value = 'dev'
  showEnvCreateForm.value = true
}

async function handleCreateEnv() {
  const name = newEnvName.value.trim()
  if (!name) return
  const isEdit = !!editingEnvId.value
  try {
    const { invoke } = await import('@tauri-apps/api/core')

    if (props.scope?.project) {
      const { useProjectStore } = await import('@/core/project/stores/project')
      const pp = useProjectStore().currentProject?.path
      if (!pp) {
        message.warning('⚠️ 未打开项目')
        return
      }
      if (isEdit) {
        await invoke('project_update_environment', {
          id: editingEnvId.value,
          name,
          description: newEnvDesc.value || null,
          color: newEnvColor.value || '#a6e3a1',
          sortOrder: 0,
          projectPath: pp,
        })
      } else {
        await invoke('project_create_environment', {
          name,
          description: newEnvDesc.value || null,
          color: newEnvColor.value || '#a6e3a1',
          sortOrder: 0,
          projectPath: pp,
        })
      }
    } else {
      if (isEdit) {
        await invoke('update_environment', {
          env: {
            id: editingEnvId.value,
            name,
            description: newEnvDesc.value,
            color: newEnvColor.value || '#a6e3a1',
            sort_order: 0,
            origin: 'project',
            source_id: null,
          },
        })
      } else {
        await invoke('create_environment', {
          env: {
            name,
            icon: newEnvIcon.value || '🟢',
            color: newEnvColor.value || '#a6e3a1',
            description: newEnvDesc.value,
            templateId: `env-${newEnvTemplate.value}`,
          },
        })
      }
    }
    resetEnvForm()
    showEnvCreateForm.value = false
    await loadEnvironments()
  } catch (e) {
    console.warn('[EnvironmentSection] 保存环境失败，使用本地回退数据:', e)
    if (isEdit) {
      const idx = loadedEnvs.value.findIndex(e => e.id === editingEnvId.value)
      if (idx >= 0) {
        loadedEnvs.value[idx] = {
          ...loadedEnvs.value[idx],
          name,
          color: newEnvColor.value || '#a6e3a1',
          icon: newEnvIcon.value || '🟢',
          desc: newEnvDesc.value,
          ui: { summaryUI: newEnvColor.value || '#a6e3a1' },
        }
      }
    } else {
      const id = `env-custom-${Date.now()}`
      loadedEnvs.value.push({
        id,
        name,
        color: newEnvColor.value || '#a6e3a1',
        icon: newEnvIcon.value || '🟢',
        desc: newEnvDesc.value,
        builtin: false,
        summarySecurity: '自定义',
        summarySchema: '自动',
        summaryPerf: '默认',
        summaryAudit: '自定义',
        ui: { summaryUI: newEnvColor.value || '#a6e3a1' },
      })
    }
    resetEnvForm()
    showEnvCreateForm.value = false
  }
}

async function handleDeleteEnv(id: string) {
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    try {
      const policies = await invoke<
        Array<{ id: string; environment_id: string; policy_type: string; enabled: boolean }>
      >('list_environment_policies', { environmentId: id })
      for (const p of policies) {
        if (props.scope?.project) {
          const { useProjectStore } = await import('@/core/project/stores/project')
          const pp = useProjectStore().currentProject?.path
          if (pp) await invoke('project_delete_environment_policy', { id: p.id, projectPath: pp })
        } else {
          await invoke('delete_environment_policy', { id: p.id })
        }
      }
    } catch (err) {
      console.warn('[EnvironmentSection] 策略清理:', err)
    }

    if (props.scope?.project) {
      const { useProjectStore } = await import('@/core/project/stores/project')
      const pp = useProjectStore().currentProject?.path
      if (!pp) {
        message.warning('⚠️ 未打开项目')
        return
      }
      await invoke('project_delete_environment', { id, projectPath: pp })
    } else {
      await invoke('delete_environment', { id })
    }
    await loadEnvironments()
  } catch (e) {
    console.warn('[EnvironmentSection] 删除环境失败:', e)
    loadedEnvs.value = loadedEnvs.value.filter(e2 => e2.id !== id)
  }
}

async function loadEnvironments() {
  envListLoading.value = true
  try {
    const { invoke } = await import('@tauri-apps/api/core')

    let remote: Array<{
      id: string
      name: string
      description: string
      properties?: Record<string, unknown>
    }> = []

    if (props.scope?.global) {
      const globalEnvs = await invoke<
        Array<{
          id: string
          name: string
          description: string
          properties?: Record<string, unknown>
        }>
      >('list_environments')
      remote = [...remote, ...(globalEnvs || [])]
    }

    if (props.scope?.project) {
      const { useProjectStore } = await import('@/core/project/stores/project')
      const pp = useProjectStore().currentProject?.path
      if (pp) {
        const projectEnvs = await invoke<
          Array<{
            id: string
            name: string
            description: string
            properties?: Record<string, unknown>
          }>
        >('project_list_environments', { projectPath: pp })
        remote = [...remote, ...(projectEnvs || [])]
      }
    }
    if (remote && remote.length > 0) {
      const colors = ['#a6e3a1', '#f9e2af', '#89b4fa', '#f38ba8', '#cba6f7']
      const icons = ['🟢', '🟡', '🔵', '🔴', '🟣']
      loadedEnvs.value = remote.map((e, i) => ({
        id: e.id,
        name: e.name,
        color: colors[i % colors.length],
        icon: icons[i % icons.length],
        desc: e.description || '',
        builtin: false,
        summarySecurity: (e.properties?.security as string) || '默认',
        summarySchema: (e.properties?.schema as string) || '自动',
        summaryPerf: (e.properties?.performance as string) || '默认',
        summaryAudit: (e.properties?.audit as string) || '默认',
        ui: { summaryUI: (e.properties?.uiColor as string) || colors[i % colors.length] },
      }))
    }
  } catch (e) {
    console.warn('[EnvironmentSection] 加载环境列表失败:', e)
    loadedEnvs.value = envDefsAsEnvInfo
  } finally {
    envListLoading.value = false
  }
}

onMounted(() => {
  loadEnvironments()
})
</script>

<style scoped>
.env-sec {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.env-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 3px;
}
.epi-tag {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 3px;
  line-height: 1.4;
}
.epi-tag.prod {
  background: #fde8e8;
  color: #c53030;
}
.epi-tag.staging {
  background: #e8f0fe;
  color: #2b6cb0;
}
.epi-tag.dev {
  background: #e8fde8;
  color: #276749;
}
.epi-tag.test {
  background: #fefcbf;
  color: #975a16;
}
.env-preset-indicator {
  font-size: 11px;
  color: var(--color-text-muted);
}
.env-override-hint {
  font-size: 11px;
  color: var(--brand-warning);
}
.env-snapshot-hint {
  font-size: 11px;
  color: var(--color-text-muted);
}
</style>
