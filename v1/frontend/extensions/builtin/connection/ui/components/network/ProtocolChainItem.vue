<template>
  <div
    class="chain-item"
    :class="{
      'chain-item-ssl': isSsl,
      disabled: !hop.enabled,
    }"
    :draggable="true"
    @dragstart="onDragStart"
    @dragover.prevent=""
    @drop.prevent="onDrop"
    @dragend="onDragEnd"
  >
    <!-- 拖拽手柄 -->
    <div
      class="drag-handle"
      :class="{ locked: isSsl }"
      :title="isSsl ? 'SSL 固定在末尾' : '拖拽排序'"
    >
      {{ isSsl ? '🔒' : '≡' }}
    </div>

    <!-- 序号 -->
    <div class="order-badge" :class="[hop.protocol, { 'ssl-badge': isSsl }]">
      {{ orderLabel }}
    </div>

    <!-- 协议类型 -->
    <div class="type-badge" :class="hop.protocol">
      <span class="type-icon">{{ typeIcon }}</span>
      <span class="type-label">{{ typeLabel }}</span>
    </div>

    <!-- 配置区域 -->
    <div class="config-area">
      <!-- 选择模式 -->
      <template v-if="hop.mode === 'select'">
        <div class="config-select-row">
          <select
            class="form-select"
            :value="hop.profileId"
            @change="onProfileSelect(($event.target as HTMLSelectElement).value)"
          >
            <option value="">— 选择一个配置 —</option>
            <option v-for="p in profiles" :key="p.id" :value="p.id">
              {{ getProfileOptionLabel(p) }}
            </option>
          </select>
          <button class="btn-inline save" @click="emit('switch-mode', hop.id, 'new')">
            + 新建
          </button>
        </div>
        <!-- SSH 转发提示 -->
        <div v-if="hop.protocol === 'ssh' && selectedSshProfile" class="fwd-hint">
          📡 转发: {{ selectedSshProfile.remoteHost || 'DB' }}:{{
            selectedSshProfile.remotePort || 'auto'
          }}
          → 127.0.0.1:{{ selectedSshProfile.localPort || 'auto' }}
        </div>
      </template>

      <!-- 新建模式 -->
      <template v-else-if="hop.mode === 'new'">
        <div class="inline-form">
          <slot name="new-form" :hop="hop" :profiles="profiles" />
          <div class="form-actions">
            <button class="btn-inline save" @click="emit('save-new', hop.id)"> 保存并应用 </button>
            <button class="btn-inline cancel" @click="emit('switch-mode', hop.id, 'select')">
              取消
            </button>
          </div>
        </div>
      </template>

      <!-- 自定义模式 -->
      <template v-else-if="hop.mode === 'custom'">
        <div class="inline-form custom-form">
          <div class="custom-hint">⚡ 一次性自定义 — 不保存为配置文件</div>
          <slot name="custom-form" :hop="hop" />
          <div class="form-actions">
            <button class="btn-inline cancel" @click="emit('switch-mode', hop.id, 'select')">
              关闭自定义
            </button>
          </div>
        </div>
      </template>
    </div>

    <!-- 启用开关 -->
    <div class="toggle-wrap" @click.stop>
      <div class="switch-toggle" :class="{ on: hop.enabled }" @click="emit('toggle', hop.id)" />
    </div>

    <!-- 操作按钮 -->
    <div class="action-btns">
      <button class="btn-mini manage" :title="'管理'" @click="emit('manage', hop.protocol)">
        📋
      </button>
      <button v-if="canDelete" class="btn-mini danger" title="删除" @click="emit('delete', hop.id)">
        ✕
      </button>
      <span v-else class="delete-disabled" title="每种协议至少保留一个实例"> ✕ </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

import type {
  ProtocolNode,
  ProtocolType,
  SshProfile,
  SslProfile,
  ProxyProfile,
  HopConfigMode,
} from '../../types/network-chain'

const props = defineProps<{
  hop: ProtocolNode
  orderLabel: string | number
  canDelete: boolean
  profiles: (SshProfile | SslProfile | ProxyProfile)[]
}>()

const emit = defineEmits<{
  toggle: [hopId: string]
  delete: [hopId: string]
  'switch-mode': [hopId: string, mode: HopConfigMode]
  'save-new': [hopId: string]
  manage: [protocol: ProtocolType]
  'select-profile': [hopId: string, profileId: string]
  'drag-start': [hopId: string]
  'drag-end': []
  drop: [hopId: string]
}>()

const isSsl = computed(() => props.hop.protocol === 'ssl')

const typeIcon = computed(() => {
  const icons: Record<ProtocolType, string> = { ssh: '🔒', ssl: '🛡', proxy: '🌐' }
  return icons[props.hop.protocol] || '●'
})

const typeLabel = computed(() => {
  const labels: Record<ProtocolType, string> = {
    ssh: 'SSH 隧道',
    ssl: 'SSL/TLS (末尾)',
    proxy: '代理',
  }
  return labels[props.hop.protocol] || props.hop.protocol.toUpperCase()
})

const selectedSshProfile = computed<SshProfile | null>(() => {
  if (props.hop.protocol !== 'ssh' || !props.hop.profileId) return null
  return (props.profiles.find(p => p.id === props.hop.profileId) as SshProfile) || null
})

function getProfileOptionLabel(p: SshProfile | SslProfile | ProxyProfile): string {
  const scopeIcon = p.scope === 'global' ? '🌐' : '📝'
  const name = p.name

  if ('host' in p && 'port' in p) {
    return `${scopeIcon} ${name} (${p.host}:${p.port})`
  }
  if ('mode' in p) {
    const ssl = p as SslProfile
    return `${scopeIcon} ${name} (${ssl.mode})`
  }
  return `${scopeIcon} ${name}`
}

function onProfileSelect(value: string) {
  emit('select-profile', props.hop.id, value)
}

function onDragStart(event: DragEvent) {
  const dt = event.dataTransfer
  if (!dt) return
  dt.effectAllowed = 'move'
  dt.setData('text/plain', props.hop.id)
  emit('drag-start', props.hop.id)
}

function onDrop(_event: DragEvent) {
  emit('drop', props.hop.id)
}

function onDragEnd() {
  emit('drag-end')
}
</script>

<style scoped>
.chain-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: 8px 12px;
  background: var(--color-bg-raised, #11111b);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: var(--border-radius-lg);
  margin-bottom: 6px;
  transition:
    border-color 0.15s,
    background 0.15s,
    opacity 0.15s;
}

.chain-item:hover {
  border-color: rgba(255, 255, 255, 0.12);
}

.chain-item.disabled {
  opacity: 0.5;
}

.chain-item-ssl {
  background: rgba(137, 180, 250, 0.04);
  border-color: rgba(137, 180, 250, 0.1);
}

.chain-item-ssl:hover {
  border-color: rgba(137, 180, 250, 0.2);
}

.chain-item-ssl.disabled {
  opacity: 0.4;
}

/* 拖拽手柄 */
.drag-handle {
  width: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--color-text-muted);
  font-size: var(--font-size-lg);
  cursor: grab;
  flex-shrink: 0;
  user-select: none;
}

.drag-handle:active {
  cursor: grabbing;
}

.drag-handle.locked {
  cursor: default;
  opacity: 0.5;
}

/* 序号 */
.order-badge {
  width: 28px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--border-radius-sm);
  font-size: var(--font-size-xs);
  font-weight: 600;
  flex-shrink: 0;
}

.order-badge.ssh {
  background: rgba(166, 227, 161, 0.12);
  color: var(--brand-success);
}

.order-badge.proxy {
  background: rgba(250, 179, 135, 0.12);
  color: #fab387;
}

.order-badge.ssl-badge {
  background: rgba(137, 180, 250, 0.12);
  color: #89b4fa;
}

/* 类型标签 */
.type-badge {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 10px;
  border-radius: var(--border-radius-sm);
  font-size: var(--font-size-sm);
  font-weight: 500;
  flex-shrink: 0;
  min-width: 110px;
}

.type-badge.ssh {
  background: rgba(166, 227, 161, 0.08);
  color: var(--brand-success);
}

.type-badge.ssl {
  background: rgba(137, 180, 250, 0.08);
  color: #89b4fa;
}

.type-badge.proxy {
  background: rgba(250, 179, 135, 0.08);
  color: #fab387;
}

.type-icon {
  font-size: var(--font-size-md);
}

/* 配置区域 */
.config-area {
  flex: 1;
  min-width: 0;
}

.config-select-row {
  display: flex;
  gap: 6px;
  align-items: center;
}

.form-select {
  flex: 1;
  height: var(--height-input);
  padding: 0 10px;
  background: var(--color-bg-surface, #1a1b26);
  border: 1px solid rgba(255, 255, 255, 0.06);
  border-radius: var(--border-radius-sm);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  outline: none;
  cursor: pointer;
  transition: border-color 0.2s;
}

.form-select:focus {
  border-color: #89b4fa;
}

.fwd-hint {
  margin-top: 4px;
  font-size: var(--font-size-xxs);
  color: var(--color-text-muted);
}

/* 新建/自定义表单 */
.inline-form {
  padding: 12px;
  margin-top: 0;
  background: var(--color-bg-surface, #1a1b26);
  border: 1px solid #89b4fa;
  border-radius: var(--border-radius-lg);
}

.inline-form.custom-form {
  border-color: var(--brand-warning);
}

.custom-hint {
  font-size: var(--font-size-xxs);
  color: var(--brand-warning);
  margin-bottom: 4px;
}

.form-actions {
  display: flex;
  gap: 6px;
  margin-top: 8px;
}

/* 按钮 */
.btn-inline {
  height: 28px;
  padding: 0 12px;
  border: none;
  border-radius: var(--border-radius-sm);
  font-size: var(--font-size-xs);
  font-weight: 500;
  cursor: pointer;
  transition: all 0.12s;
  white-space: nowrap;
}

.btn-inline.save {
  background: #89b4fa;
  color: #111;
}

.btn-inline.save:hover {
  filter: brightness(1.1);
}

.btn-inline.cancel {
  background: transparent;
  color: var(--color-text-muted);
}

.btn-inline.cancel:hover {
  background: rgba(255, 255, 255, 0.05);
  color: var(--color-text-primary);
}

/* 开关 */
.toggle-wrap {
  flex-shrink: 0;
  display: flex;
  align-items: center;
}

.switch-toggle {
  width: 34px;
  height: 18px;
  background: rgba(255, 255, 255, 0.08);
  border-radius: 9px;
  position: relative;
  cursor: pointer;
  transition: background 0.2s;
}

.switch-toggle.on {
  background: #89b4fa;
}

.switch-toggle::after {
  content: '';
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: #fff;
  position: absolute;
  top: 2px;
  left: 2px;
  transition: left 0.2s;
}

.switch-toggle.on::after {
  left: 18px;
}

/* 操作按钮 */
.action-btns {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}

.btn-mini {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: var(--border-radius-sm);
  color: var(--color-text-muted);
  font-size: var(--font-size-xs);
  cursor: pointer;
  transition: all 0.12s;
}

.btn-mini:hover {
  border-color: rgba(255, 255, 255, 0.2);
  color: var(--color-text-primary);
}

.btn-mini.danger:hover {
  border-color: var(--brand-danger);
  color: var(--brand-danger);
}

.btn-mini.manage:hover {
  border-color: #89b4fa;
  color: #89b4fa;
}

.delete-disabled {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: var(--font-size-xxs);
  color: var(--color-text-muted);
  opacity: 0.3;
  cursor: not-allowed;
}
</style>
