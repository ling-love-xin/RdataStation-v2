<template>
  <div v-if="visible" class="chain-warning" role="alert">
    <div class="warning-icon">&#9888;</div>
    <div class="warning-text">
      当前协议链包含 <strong>{{ hopCount }}</strong> 个网络跳（SSH/Proxy）， 建连延迟预期
      <strong>~{{ estimatedLatency }}ms</strong>。每增加一跳会增加 10-50ms 延迟和一个故障点。
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  visible: boolean
  hopCount: number
  estimatedLatency: number
}>()
</script>

<style scoped>
.chain-warning {
  display: flex;
  align-items: flex-start;
  gap: var(--spacing-sm);
  margin-top: var(--spacing-sm);
  padding: 10px 14px;
  background: rgba(249, 226, 175, 0.06);
  border: 1px solid rgba(249, 226, 175, 0.15);
  border-radius: var(--border-radius-md);
  font-size: var(--font-size-sm);
  color: var(--brand-warning);
  line-height: 1.5;
}

.warning-icon {
  font-size: var(--font-size-lg);
  flex-shrink: 0;
  line-height: 1.5;
}

.warning-text strong {
  color: #fab387;
  font-weight: 600;
}
</style>
