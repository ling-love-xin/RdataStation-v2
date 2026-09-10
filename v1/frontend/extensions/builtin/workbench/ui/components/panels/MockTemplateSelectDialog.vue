<template>
  <NModal v-model:show="visible" preset="card" title="选择场景模板" style="width: 680px">
    <div class="template-dialog">
      <div class="template-grid">
        <div
          v-for="tmpl in templates"
          :key="tmpl.id"
          class="template-card"
          :class="{ selected: selectedId === tmpl.id }"
          @click="selectedId = tmpl.id"
          @dblclick="onApply"
        >
          <div class="template-card-header">
            <span class="template-name">{{ tmpl.name }}</span>
            <span class="template-category">{{ tmpl.category }}</span>
          </div>
          <p class="template-desc">{{ tmpl.description }}</p>
          <div class="template-meta">
            <span class="template-tables">{{ tmpl.tables?.length ?? 0 }} 张表</span>
            <span class="template-locale">{{ tmpl.locale }}</span>
          </div>
        </div>
      </div>

      <div v-if="templates.length === 0" class="empty-state"> 暂无可用模板 </div>

      <div class="dialog-footer">
        <NButton size="small" @click="visible = false">取消</NButton>
        <NButton type="primary" size="small" :disabled="!selectedId" @click="onApply">
          应用模板
        </NButton>
      </div>
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { NModal, NButton, createDiscreteApi } from 'naive-ui'
import { ref, watch } from 'vue'

import { mockApi, type ScenarioTemplate } from '@/shared/api/mock-api'

const props = defineProps<{
  show: boolean
}>()

const emit = defineEmits<{
  'update:show': [value: boolean]
  apply: [templateId: string, template: ScenarioTemplate]
}>()

const { message } = createDiscreteApi(['message'])
const visible = ref(props.show)
const selectedId = ref<string | null>(null)
const templates = ref<ScenarioTemplate[]>([])

watch(
  () => props.show,
  val => {
    visible.value = val
    if (val) {
      selectedId.value = null
      loadTemplates()
    }
  }
)

watch(visible, val => {
  emit('update:show', val)
})

async function loadTemplates() {
  try {
    templates.value = await mockApi.listTemplates()
  } catch {
    message.error('加载模板失败')
  }
}

async function onApply() {
  if (!selectedId.value) return
  try {
    const tmpl = await mockApi.applyTemplate(selectedId.value)
    emit('apply', selectedId.value, tmpl)
    visible.value = false
    message.success('模板已应用')
  } catch (e) {
    message.error(`应用模板失败: ${String(e)}`)
  }
}
</script>

<style scoped>
.template-dialog {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
}

.template-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--spacing-sm);
}

.template-card {
  padding: var(--spacing-md) var(--spacing-md);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-md);
  cursor: pointer;
  transition: all 0.15s;
  background: var(--color-bg-secondary);
}

.template-card:hover {
  border-color: var(--brand-accent);
  background: var(--color-bg-elevated);
}

.template-card.selected {
  border-color: var(--brand-accent);
  box-shadow: 0 0 0 2px var(--brand-accent-soft);
}

.template-card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--spacing-xs);
}

.template-name {
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--color-text-primary);
}

.template-category {
  font-size: var(--font-size-xs);
  color: var(--brand-accent);
  background: var(--brand-accent-soft);
  padding: 1px var(--spacing-sm);
  border-radius: var(--border-radius-sm);
}

.template-desc {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  margin: 0 0 var(--spacing-sm) 0;
  line-height: 1.4;
}

.template-meta {
  display: flex;
  gap: var(--spacing-sm);
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
}

.empty-state {
  text-align: center;
  padding: var(--spacing-xl) 0;
  color: var(--color-text-muted);
  font-size: var(--font-size-md);
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--spacing-sm);
  padding-top: var(--spacing-sm);
  border-top: 1px solid var(--color-border-subtle);
}
</style>
