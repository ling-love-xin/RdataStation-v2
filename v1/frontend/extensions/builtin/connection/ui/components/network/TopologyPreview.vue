<template>
  <div class="topo-box">
    <div class="topo-title">📡 {{ $t('connection.networkTab.topology') }}</div>
    <div class="topo-path">
      <span class="topo-node self">🏠 {{ t('navigator.localhost') }}</span>
      <template v-for="hop in enabledHops" :key="hop.id">
        <span v-if="hop.protocol !== 'ssl'" class="topo-arrow"
          >──{{ hop.protocol === 'ssh' ? 'SSH' : 'Proxy' }}──▶</span
        >
        <span v-else class="topo-arrow tls">──TLS🔐──▶</span>
        <span :class="['topo-node', topoNodeClass(hop.protocol)]">
          {{ topoHopLabel(hop) }}
        </span>
      </template>
      <span v-if="enabledHops.length === 0" class="topo-arrow">────▶</span>
      <span class="topo-node db">🗄 {{ dbLabel }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

export interface TopoHop {
  id: string
  protocol: 'ssh' | 'ssl' | 'proxy'
  enabled: boolean
  mode: 'select' | 'new' | 'custom'
  profileId: string
  host?: string
  port?: number
  customData?: Record<string, unknown>
}

const props = defineProps<{
  hops: TopoHop[]
  dbLabel: string
}>()

const { t } = useI18n()

const enabledHops = computed(() => props.hops.filter(h => h.enabled))

function topoNodeClass(p: string) {
  return { ssh: 'ssh', proxy: 'proxy', ssl: '' }[p] || ''
}

function topoHopLabel(hop: TopoHop): string {
  if (hop.mode === 'select' && hop.profileId) {
    return hop.profileId
  }
  if (hop.host) {
    return `${hop.host}${hop.port ? ':' + hop.port : ''}`
  }
  return hop.protocol.toUpperCase()
}
</script>

<style scoped>
.topo-box {
  padding: 14px;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-subtle);
  border-radius: 8px;
}
.topo-title {
  font-size: 10px;
  font-weight: 600;
  color: var(--color-text-muted);
  text-transform: uppercase;
  margin-bottom: 10px;
}
.topo-path {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 0;
  font-size: 11px;
}
.topo-node {
  padding: 4px 10px;
  border-radius: 4px;
  font-weight: 500;
  border: 1px solid var(--color-border-subtle);
}
.topo-node.self {
  background: rgba(137, 180, 250, 0.06);
  color: var(--brand-accent);
}
.topo-node.ssh {
  background: rgba(166, 227, 161, 0.06);
  color: var(--brand-success);
}
.topo-node.proxy {
  background: rgba(203, 166, 247, 0.06);
  color: var(--brand-purple);
}
.topo-node.db {
  background: rgba(250, 179, 135, 0.06);
  color: var(--brand-warning);
}
.topo-arrow {
  color: var(--color-text-muted);
  padding: 0 4px;
  white-space: nowrap;
}
.topo-arrow.tls {
  background: rgba(137, 180, 250, 0.06);
  border: 1px dashed rgba(137, 180, 250, 0.2);
  border-radius: 3px;
  color: var(--brand-accent);
}
</style>
