<template>
  <NModal
    :show="modelValue"
    preset="card"
    :style="{ maxWidth: '560px' }"
    :title="title"
    @update:show="$emit('update:modelValue', $event)"
  >
    <div class="env-mgr-list">
      <div v-if="loading">{{ loadingText }}</div>
      <div
        v-for="env in environments"
        :key="env.id"
        class="env-mgr-card"
        :style="{ borderLeft: `3px solid ${env.color}` }"
      >
        <div class="env-mgr-color-dot" :style="{ background: env.color }" />
        <div class="env-mgr-content">
          <div class="env-mgr-header">
            <span class="env-mgr-icon">{{ env.icon }}</span>
            <span class="env-mgr-name">{{ env.name }}</span>
            <span v-if="env.builtin" class="env-mgr-badge">{{ builtinBadge }}</span>
            <span
              v-else-if="sourceLabel(env.id)"
              :class="['env-mgr-source-tag', sourceKind(env.id)]"
              >{{ sourceLabel(env.id) }}</span
            >
          </div>
          <div class="env-mgr-desc">{{ env.desc }}</div>
          <div v-if="env.ui" class="env-mgr-meta">
            <span class="env-mgr-meta-lbl">UI:</span>
            <span class="env-mgr-meta-dot" :style="{ background: env.ui.summaryUI }"></span>
            <span class="env-mgr-meta-val">{{ env.ui.summaryUI }}</span>
          </div>
          <div class="env-policy-tags">
            <span class="policy-tag security">{{ env.summarySecurity }}</span>
            <span class="policy-tag schema">{{ env.summarySchema }}</span>
            <span class="policy-tag performance">{{ env.summaryPerf }}</span>
            <span class="policy-tag audit">{{ env.summaryAudit }}</span>
          </div>
        </div>
        <div class="env-mgr-actions">
          <NButton v-if="!env.builtin" size="tiny" text @click="$emit('edit', env)">✎</NButton>
          <NButton
            v-if="!env.builtin"
            size="tiny"
            type="error"
            text
            @click="$emit('delete', env.id)"
            >✕</NButton
          >
        </div>
      </div>
    </div>

    <div class="env-mgr-create-section">
      <NButton v-if="!showCreateForm" size="small" dashed @click="$emit('toggle-create')"
        >+ {{ createLabel }}</NButton
      >
      <div v-else class="env-create-form">
        <div class="env-form-title">{{ editing ? '✎ 编辑环境' : '+ 新建环境' }}</div>
        <div class="env-create-row">
          <label class="env-create-lbl">{{ nameLabel }}</label>
          <NInput
            :value="newName"
            size="small"
            :placeholder="namePlaceholder"
            @update:value="$emit('update:newName', $event)"
          />
        </div>
        <div class="env-create-row">
          <label class="env-create-lbl">{{ iconLabel }}</label>
          <NInput
            :value="newIcon"
            size="small"
            placeholder="🟢"
            style="max-width: 80px"
            @update:value="$emit('update:newIcon', $event)"
          />
          <label class="env-create-lbl" style="margin-left: 12px">{{ colorLabel }}</label>
          <input
            :value="newColor"
            type="color"
            class="color-input"
            @input="$emit('update:newColor', ($event.target as HTMLInputElement).value)"
          />
        </div>
        <div class="env-create-row">
          <label class="env-create-lbl">{{ descLabel }}</label>
          <NInput
            :value="newDesc"
            size="small"
            :placeholder="descPlaceholder"
            @update:value="$emit('update:newDesc', $event)"
          />
        </div>
        <div class="env-create-row">
          <label class="env-create-lbl">{{ templateLabel }}</label>
          <NSelect
            :value="newTemplate"
            size="small"
            :options="templateOptions"
            style="flex: 1"
            @update:value="$emit('update:newTemplate', $event)"
          />
        </div>
        <div class="env-create-actions">
          <NButton size="tiny" type="primary" @click="$emit('create')">{{ saveLabel }}</NButton>
          <NButton size="tiny" @click="$emit('toggle-create')">{{ cancelLabel }}</NButton>
        </div>
      </div>
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { NModal, NButton, NInput, NSelect } from 'naive-ui'

export interface EnvInfo {
  id: string
  name: string
  color: string
  icon: string
  desc: string
  builtin: boolean
  summarySecurity: string
  summarySchema: string
  summaryPerf: string
  summaryAudit: string
  ui: { summaryUI: string }
}

defineProps<{
  modelValue: boolean
  title: string
  loading: boolean
  loadingText: string
  environments: EnvInfo[]
  builtinBadge: string
  showCreateForm: boolean
  editing: boolean
  createLabel: string
  nameLabel: string
  namePlaceholder: string
  iconLabel: string
  colorLabel: string
  descLabel: string
  descPlaceholder: string
  templateLabel: string
  saveLabel: string
  cancelLabel: string
  newName: string
  newIcon: string
  newColor: string
  newDesc: string
  newTemplate: string
  templateOptions: { label: string; value: string }[]
}>()

defineEmits<{
  'update:modelValue': [value: boolean]
  'update:newName': [value: string]
  'update:newIcon': [value: string]
  'update:newColor': [value: string]
  'update:newDesc': [value: string]
  'update:newTemplate': [value: string]
  'toggle-create': []
  create: []
  edit: [env: EnvInfo]
  delete: [id: string]
}>()

/**
 * 根据 ID 前缀推导环境来源标签
 *   G_  → "全局" (global.db)
 *   P_  → "项目" (project.db)
 *   GP_ → "快照" (全局快照到项目)
 */
function sourceLabel(id: string): string {
  if (id.startsWith('GP_')) return '📸 快照'
  if (id.startsWith('G_')) return '🌐 全局'
  if (id.startsWith('P_')) return '📁 项目'
  return ''
}

function sourceKind(id: string): string {
  if (id.startsWith('GP_')) return 'snapshot'
  if (id.startsWith('G_')) return 'global'
  if (id.startsWith('P_')) return 'project'
  return ''
}
</script>

<style scoped>
.env-mgr-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.env-mgr-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  border-radius: var(--border-radius-lg);
  background: var(--color-bg-elevated);
}
.env-mgr-color-dot {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  flex-shrink: 0;
}
.env-mgr-content {
  flex: 1;
  min-width: 0;
}
.env-mgr-header {
  display: flex;
  align-items: center;
  gap: 6px;
}
.env-mgr-icon {
  font-size: 14px;
}
.env-mgr-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-text-primary);
}
.env-mgr-badge {
  font-size: var(--font-size-xs);
  padding: 1px 6px;
  border-radius: var(--border-radius-sm);
  background: rgba(137, 180, 250, 0.1);
  color: var(--brand-accent);
  font-weight: 500;
}
.env-mgr-source-tag {
  font-size: var(--font-size-xs);
  padding: 1px 6px;
  border-radius: var(--border-radius-sm);
  font-weight: 500;
}
.env-mgr-source-tag.global {
  background: rgba(137, 180, 250, 0.1);
  color: #89b4fa;
}
.env-mgr-source-tag.project {
  background: rgba(166, 227, 161, 0.1);
  color: var(--brand-success);
}
.env-mgr-source-tag.snapshot {
  background: rgba(245, 194, 231, 0.12);
  color: #cba6f7;
  font-style: italic;
}
.env-mgr-desc {
  font-size: 11px;
  color: var(--color-text-muted);
  margin-top: 2px;
}
.env-mgr-meta {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-top: 2px;
  font-size: var(--font-size-xxs);
  color: var(--color-text-muted);
}
.env-mgr-meta-lbl {
  font-weight: 600;
}
.env-mgr-meta-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}
.env-mgr-meta-val {
  font-family: var(--font-mono);
}
.env-policy-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  margin-top: 6px;
}
.policy-tag {
  font-size: var(--font-size-xs);
  padding: 2px 8px;
  border-radius: var(--border-radius-sm);
  font-weight: 500;
}
.policy-tag.security {
  background: rgba(243, 139, 168, 0.12);
  color: var(--status-locked);
}
.policy-tag.schema {
  background: rgba(137, 180, 250, 0.1);
  color: var(--brand-accent);
}
.policy-tag.performance {
  background: rgba(166, 227, 161, 0.1);
  color: var(--brand-success);
}
.policy-tag.audit {
  background: rgba(249, 226, 175, 0.12);
  color: var(--brand-warning);
}
.env-mgr-actions {
  flex-shrink: 0;
  display: flex;
  align-items: center;
}
.env-mgr-create-section {
  margin-top: 14px;
  padding-top: 14px;
  border-top: 1px solid var(--color-border-subtle);
}
.env-create-form {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  background: var(--color-bg-elevated);
  border-radius: var(--border-radius-lg);
  border: 1px solid var(--color-border-subtle);
}
.env-form-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--brand-accent, #e17055);
  margin-bottom: 2px;
}
.env-create-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.env-create-lbl {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  font-weight: 500;
  min-width: 56px;
  flex-shrink: 0;
}
.env-create-actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}
.color-input {
  width: 32px;
  height: 32px;
  padding: 0;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-sm);
  background: transparent;
  cursor: pointer;
  box-sizing: border-box;
  flex-shrink: 0;
}
.color-input::-webkit-color-swatch-wrapper {
  padding: 2px;
}
.color-input::-webkit-color-swatch {
  border: none;
  border-radius: 2px;
}
.color-input:focus-visible {
  outline: 2px solid var(--brand-accent);
  outline-offset: 2px;
}
</style>
