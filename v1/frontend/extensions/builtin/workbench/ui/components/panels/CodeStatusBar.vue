<template>
  <div class="code-statusbar">
    <span class="statusbar-item">{{ language }}</span>
    <span class="statusbar-separator">|</span>
    <span class="statusbar-item">{{ encoding }}</span>
    <span class="statusbar-separator">|</span>
    <span class="statusbar-item">缩进: {{ indent }}</span>
    <span class="statusbar-separator">|</span>
    <span class="statusbar-item">{{ cursorPosition }}</span>
    <template v-if="diagnosticCount.errors > 0 || diagnosticCount.warnings > 0">
      <span class="statusbar-separator">|</span>
    </template>
    <span v-if="diagnosticCount.errors > 0" class="statusbar-item statusbar-error">
      &#10060; {{ diagnosticCount.errors }}E
    </span>
    <span v-if="diagnosticCount.warnings > 0" class="statusbar-item statusbar-warning">
      &#9888; {{ diagnosticCount.warnings }}W
    </span>
    <span v-if="diagnosticCount.infos > 0" class="statusbar-item statusbar-info">
      &#8505; {{ diagnosticCount.infos }}I
    </span>
    <span class="statusbar-spacer" />
    <span class="statusbar-item">LSP</span>
  </div>
</template>

<script setup lang="ts">
interface Props {
  language: string
  encoding: string
  indent: string
  cursorPosition: string
  diagnosticCount: {
    errors: number
    warnings: number
    infos: number
  }
}

defineProps<Props>()
</script>

<style scoped>
.code-statusbar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 10px;
  background: var(--statusbar-bg, #68217a);
  color: var(--statusbar-fg, #ffffff);
  font-size: 12px;
  flex-shrink: 0;
  min-height: 22px;
  user-select: none;
}

.statusbar-item {
  white-space: nowrap;
  opacity: 0.9;
}

.statusbar-separator {
  opacity: 0.4;
}

.statusbar-spacer {
  flex: 1;
}

.statusbar-error {
  color: #f48771;
  font-weight: 500;
}

.statusbar-warning {
  color: #f0c040;
  font-weight: 500;
}

.statusbar-info {
  color: #75beff;
}
</style>
