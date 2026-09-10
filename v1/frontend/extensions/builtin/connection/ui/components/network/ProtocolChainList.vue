<template>
  <div class="protocol-chain-list">
    <!-- 列头 -->
    <div class="chain-header">
      <span class="drag-col"></span>
      <span class="order-col">#</span>
      <span class="type-col">{{ t('connection.networkTab.protocol') }}</span>
      <span class="config-col">{{ t('connection.networkTab.configuration') }}</span>
      <span class="toggle-col">{{ t('connection.networkTab.enable') }}</span>
      <span class="action-col">{{ t('connection.networkTab.actions') }}</span>
    </div>

    <!-- 节点列表 -->
    <div class="chain-list">
      <div v-if="chain.length === 0" class="empty-chain">
        <span class="empty-icon">🔗</span>
        <span class="empty-text">尚未添加任何协议节点 — 将采用直连模式</span>
      </div>

      <ProtocolChainItem
        v-for="(hop, index) in chain"
        :key="hop.id"
        :hop="hop"
        :order-label="getOrderLabel(hop, index)"
        :can-delete="canDeleteHop(hop.id)"
        :profiles="getProfilesForHop(hop)"
        @toggle="(id: string) => emit('toggle', id)"
        @delete="(id: string) => emit('delete', id)"
        @switch-mode="(id: string, mode: HopConfigMode) => emit('switch-mode', id, mode)"
        @save-new="(id: string) => emit('save-new', id)"
        @manage="(protocol: ProtocolType) => emit('manage', protocol)"
        @select-profile="(id: string, pid: string) => emit('select-profile', id, pid)"
        @drag-start="(id: string) => emit('drag-start', id)"
        @drag-end="emit('drag-end')"
        @drop="(id: string) => emit('drop', id)"
      >
        <!-- 新建表单插槽：由父组件提供具体表单 -->
        <template #new-form="{ hop: slotHop }">
          <slot name="new-form" :hop="slotHop" />
        </template>
        <template #custom-form="{ hop: slotHop }">
          <slot name="custom-form" :hop="slotHop" />
        </template>
      </ProtocolChainItem>
    </div>

    <!-- 添加按钮区域 -->
    <div class="add-hop-section">
      <div class="add-btn-wrapper">
        <button
          class="btn-add-hop"
          :class="{ full: isMaxNetworkHops && hasSsl, disabled: isMaxNetworkHops && hasSsl }"
          :disabled="isMaxNetworkHops && hasSsl"
          @click="emit('toggle-menu')"
        >
          {{ addButtonText }}
        </button>
        <span v-if="isMaxNetworkHops && hasSsl" class="hop-limit-hint">
          已达上限（最多 {{ maxNetworkHops }} 个 SSH/Proxy 网络跳）
        </span>
      </div>

      <!-- 弹出菜单 -->
      <div v-if="menuOpen" class="hop-type-menu" @click.stop>
        <div
          v-for="option in addHopOptions"
          :key="option.protocol"
          class="hop-type-option"
          :class="{ disabled: option.disabled }"
          @click="!option.disabled && emit('add-hop', option.protocol)"
        >
          <span class="opt-icon">{{ option.icon }}</span>
          {{ option.label }}
          <span class="opt-hint">{{ option.hint }}</span>
          <span v-if="option.protocol === 'ssl' && hasSsl" class="opt-replace"> (替换) </span>
        </div>
        <div class="menu-footer"> SSL 始终在链末尾（流加密包装器），不产生网络节点 </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import ProtocolChainItem from './ProtocolChainItem.vue'
import { MAX_NETWORK_HOPS } from '../../types/network-chain'

import type {
  ProtocolNode,
  ProtocolType,
  SshProfile,
  SslProfile,
  ProxyProfile,
  AddHopOption,
  HopConfigMode,
} from '../../types/network-chain'

const { t } = useI18n()

const props = defineProps<{
  chain: ProtocolNode[]
  menuOpen: boolean
  sshProfiles: SshProfile[]
  sslProfiles: SslProfile[]
  proxyProfiles: ProxyProfile[]
  addHopOptions: AddHopOption[]
  networkHopCount: number
  enabledNetworkHopCount: number
  isMaxNetworkHops: boolean
  hasSsl: boolean
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
  drop: [targetId: string]
  'toggle-menu': []
  'add-hop': [protocol: ProtocolType]
}>()

const maxNetworkHops = MAX_NETWORK_HOPS

const addButtonText = computed(() => {
  if (props.isMaxNetworkHops) {
    if (props.hasSsl) return '协议链已满'
    return '+ 添加 TLS 加密'
  }
  return '+ 添加协议节点'
})

function getProfilesForHop(hop: ProtocolNode) {
  if (hop.protocol === 'ssh') return props.sshProfiles
  if (hop.protocol === 'ssl') return props.sslProfiles
  return props.proxyProfiles
}

function canDeleteHop(hopId: string): boolean {
  const hop = props.chain.find(h => h.id === hopId)
  if (!hop) return false
  return props.chain.filter(h => h.protocol === hop.protocol).length > 1
}

function getOrderLabel(hop: ProtocolNode, _index: number): string | number {
  if (!hop.enabled) return '-'
  if (hop.protocol === 'ssl') return '🔐'

  const enabledNetworkHops = props.chain.filter(h => h.protocol !== 'ssl' && h.enabled)
  const idx = enabledNetworkHops.indexOf(hop)
  return idx >= 0 ? idx + 1 : '-'
}
</script>

<style scoped>
.protocol-chain-list {
  position: relative;
}

/* 列头 */
.chain-header {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: 0 12px 8px;
  font-size: var(--font-size-xxs);
  font-weight: 600;
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  border-bottom: 1px solid var(--color-border);
  margin-bottom: var(--spacing-sm);
}

.drag-col {
  width: 24px;
  flex-shrink: 0;
}
.order-col {
  width: 28px;
  text-align: center;
  flex-shrink: 0;
}
.type-col {
  width: 110px;
  flex-shrink: 0;
}
.config-col {
  flex: 1;
  min-width: 0;
}
.toggle-col {
  width: 48px;
  text-align: center;
  flex-shrink: 0;
}
.action-col {
  width: 60px;
  text-align: center;
  flex-shrink: 0;
}

/* 节点列表 */
.chain-list {
  min-height: 0;
}

.empty-chain {
  text-align: center;
  padding: 20px;
  color: var(--color-text-muted);
  font-size: var(--font-size-sm);
}

.empty-icon {
  display: block;
  font-size: 28px;
  margin-bottom: var(--spacing-sm);
  opacity: 0.3;
}

/* 添加按钮 */
.add-hop-section {
  margin-top: 10px;
  position: relative;
}

.add-btn-wrapper {
  display: flex;
  gap: var(--spacing-sm);
  align-items: center;
}

.btn-add-hop {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  background: transparent;
  border: 1px dashed rgba(255, 255, 255, 0.08);
  border-radius: var(--border-radius-lg);
  color: var(--color-text-muted);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition: all 0.15s;
}

.btn-add-hop:hover:not(.disabled) {
  border-color: #89b4fa;
  color: #89b4fa;
  background: rgba(137, 180, 250, 0.06);
}

.btn-add-hop.disabled,
.btn-add-hop.full {
  opacity: 0.4;
  pointer-events: none;
}

.hop-limit-hint {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
}

/* 弹出菜单 */
.hop-type-menu {
  position: absolute;
  top: 100%;
  left: 0;
  z-index: 100;
  margin-top: 4px;
  background: var(--color-bg-surface, #1a1b26);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-lg);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  overflow: hidden;
  min-width: 200px;
}

.hop-type-option {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: 10px 14px;
  cursor: pointer;
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  transition: all 0.1s;
}

.hop-type-option:hover:not(.disabled) {
  background: rgba(255, 255, 255, 0.05);
  color: var(--color-text-primary);
}

.hop-type-option.disabled {
  opacity: 0.35;
  pointer-events: none;
  cursor: not-allowed;
}

.opt-icon {
  width: 20px;
  text-align: center;
  flex-shrink: 0;
}

.opt-hint {
  font-size: var(--font-size-xxs);
  color: var(--color-text-muted);
  margin-left: auto;
}

.opt-replace {
  font-size: var(--font-size-xxs);
  color: var(--brand-warning);
}

.menu-footer {
  border-top: 1px solid var(--color-border);
  padding: 5px 14px;
  font-size: var(--font-size-xxs);
  color: var(--color-text-muted);
}
</style>
