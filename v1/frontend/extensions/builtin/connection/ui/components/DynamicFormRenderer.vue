<template>
  <div class="dynamic-form-renderer">
    <div v-for="section in visibleSections" :key="section.key" class="form-section-wrapper">
      <!-- 分区头部 -->
      <div
        v-if="section.title"
        class="section-header"
        :class="{ collapsible: section.collapsible }"
        @click="section.collapsible && toggleSection(section.key)"
      >
        <div class="header-left">
          <component
            :is="getIconComponent(section.icon)"
            v-if="section.icon"
            :size="16"
            class="section-icon"
          />
          <h4 class="section-title">{{ section.title }}</h4>
        </div>
        <div class="header-right">
          <!-- 启用开关（如果有 enableField 配置） -->
          <label v-if="section.enableField" class="section-toggle" @click.stop>
            <input
              :checked="formData[section.enableField] === true"
              type="checkbox"
              @change="toggleSectionEnable(section.enableField, $event)"
            />
            <span class="toggle-slider"></span>
          </label>
          <ChevronDown
            v-if="section.collapsible"
            :size="16"
            class="collapse-icon"
            :class="{ collapsed: collapsedSections[section.key] }"
          />
        </div>
      </div>

      <!-- 分区描述 -->
      <p v-if="section.description" class="section-description">
        {{ section.description }}
      </p>

      <!-- 字段列表 -->
      <div
        v-show="!section.collapsible || !collapsedSections[section.key]"
        v-if="!section.enableField || formData[section.enableField] === true"
        class="section-fields"
      >
        <!-- 处理行内布局：将连续的 inline 字段组合成一行 -->
        <template v-for="(row, rowIndex) in getGroupedFields(section.fields)" :key="rowIndex">
          <!-- 单字段行 -->
          <div v-if="row.fields.length === 1" class="field-wrapper">
            <FieldRenderer
              :field="row.fields[0]"
              :form-data="formData"
              :errors="errors"
              :password-visible="passwordVisibility"
              @update:form-data="updateFormData"
              @select-file="selectFile"
              @create-file="createFile"
              @toggle-password="togglePasswordVisibility"
            />
          </div>

          <!-- 多字段行（inline 布局） -->
          <div v-else class="inline-row">
            <div
              v-for="field in row.fields"
              :key="field.key"
              class="field-wrapper inline-field"
              :style="{ flex: field.flex || 1 }"
            >
              <FieldRenderer
                :field="field"
                :form-data="formData"
                :errors="errors"
                :password-visible="passwordVisibility"
                @update:form-data="updateFormData"
                @select-file="selectFile"
                @create-file="createFile"
                @toggle-password="togglePasswordVisibility"
              />
            </div>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ChevronDown } from 'lucide-vue-next'
import { ref, computed } from 'vue'

import FieldRenderer from './FieldRenderer.vue'

import type { FormSectionConfig, FormFieldConfig } from '../types/form-schema'

interface Props {
  sections: FormSectionConfig[]
  formData: Record<string, unknown>
  errors: Record<string, string>
}

const props = defineProps<Props>()

const emit = defineEmits<{
  'update:formData': [data: Record<string, unknown>]
  selectFile: [fieldKey: string]
  createFile: [fieldKey: string]
}>()

const passwordVisibility = ref<Record<string, boolean>>({})
const collapsedSections = ref<Record<string, boolean>>({})

const visibleSections = computed(() => {
  return props.sections.filter(section => {
    return !section.fields.every(field => isFieldHidden(field))
  })
})

function isFieldHidden(field: FormFieldConfig): boolean {
  if (field.hidden) return true

  if (field.dependsOn) {
    const depValue = props.formData[field.dependsOn.field]
    return depValue !== field.dependsOn.value
  }

  return false
}

function getGroupedFields(fields: FormFieldConfig[]): Array<{ fields: FormFieldConfig[] }> {
  const groups: Array<{ fields: FormFieldConfig[] }> = []
  let currentGroup: FormFieldConfig[] = []

  for (const field of fields) {
    if (field.inline) {
      currentGroup.push(field)
    } else {
      if (currentGroup.length > 0) {
        groups.push({ fields: [...currentGroup] })
        currentGroup = []
      }
      groups.push({ fields: [field] })
    }
  }

  if (currentGroup.length > 0) {
    groups.push({ fields: currentGroup })
  }

  return groups
}

function toggleSection(sectionKey: string) {
  collapsedSections.value[sectionKey] = !collapsedSections.value[sectionKey]
}

function toggleSectionEnable(fieldKey: string, event: Event) {
  const target = event.target as HTMLInputElement
  emit('update:formData', {
    ...props.formData,
    [fieldKey]: target.checked,
  })
}

function togglePasswordVisibility(fieldKey: string) {
  passwordVisibility.value[fieldKey] = !passwordVisibility.value[fieldKey]
}

function selectFile(fieldKey: string) {
  emit('selectFile', fieldKey)
}

function createFile(fieldKey: string) {
  emit('createFile', fieldKey)
}

function updateFormData(data: Record<string, unknown>) {
  emit('update:formData', {
    ...props.formData,
    ...data,
  })
}

function getIconComponent(iconName: string) {
  const iconMap: Record<string, string> = {
    shield: 'Shield',
    lock: 'Lock',
    database: 'Database',
    settings: 'Settings',
    user: 'User',
    zap: 'Zap',
  }
  return iconMap[iconName] || 'Settings'
}
</script>

<style scoped>
/* ---- 容器 ---- */
.dynamic-form-renderer {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-lg);
}

/* ---- 分区包装器 ---- */
.form-section-wrapper {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  padding: var(--spacing-md);
  background: var(--color-bg-secondary);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-lg);
}

/* ---- 分区头部 ---- */
.section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-bottom: var(--spacing-xs);
  margin-bottom: var(--spacing-xs);
  border-bottom: 1px solid var(--color-border-subtle);
}

.section-header.collapsible {
  cursor: pointer;
  user-select: none;
}

.section-header.collapsible:hover .section-title {
  color: var(--brand-accent);
}

.header-left {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.header-right {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
}

.section-icon {
  color: var(--brand-accent);
}

.section-title {
  font-size: var(--font-size-xs);
  font-weight: 700;
  text-transform: uppercase;
  color: var(--color-text-muted);
  letter-spacing: 0.5px;
  margin: 0;
  transition: color 0.2s ease;
}

.collapse-icon {
  color: var(--color-text-muted);
  transition: transform 0.2s ease;
}

.collapse-icon.collapsed {
  transform: rotate(-90deg);
}

/* ---- 分区描述 ---- */
.section-description {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  margin: 0 0 var(--spacing-xs);
}

/* ---- 分区开关 ---- */
.section-toggle {
  display: flex;
  align-items: center;
  cursor: pointer;
}

.section-toggle input {
  display: none;
}

.section-toggle .toggle-slider {
  width: 34px;
  height: 18px;
  background: var(--color-bg-tertiary);
  border-radius: var(--border-radius-pill);
  position: relative;
  transition: background 0.2s ease;
}

.section-toggle .toggle-slider::after {
  content: '';
  position: absolute;
  width: 14px;
  height: 14px;
  background: var(--color-text-primary);
  border-radius: 50%;
  top: 2px;
  left: 2px;
  transition: left 0.2s ease;
}

.section-toggle input:checked + .toggle-slider {
  background: var(--brand-accent);
}

.section-toggle input:checked + .toggle-slider::after {
  left: 18px;
}

/* ---- 字段列表 ---- */
.section-fields {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
}

/* ---- 字段包装器 ---- */
.field-wrapper {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

/* ---- 行内多字段行 ---- */
.inline-row {
  display: flex;
  gap: var(--spacing-md);
  align-items: flex-end;
}

.inline-field {
  min-width: 0;
}
</style>
