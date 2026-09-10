<template>
  <div class="db-icon" :class="type">
    <svg v-if="type === 'mysql'" viewBox="0 0 24 24" fill="currentColor">
      <path
        d="M12.5 2c-1.5 0-2.5 1-2.5 2.5v.5c-1.5.5-3 1.5-4 2.5-1 1-1.5 2-1.5 3.5 0 1.5.5 2.5 1.5 3.5 1 1 2.5 2 4 2.5v.5c0 1.5 1 2.5 2.5 2.5s2.5-1 2.5-2.5v-.5c1.5-.5 3-1.5 4-2.5 1-1 1.5-2 1.5-3.5 0-1.5-.5-2.5-1.5-3.5-1-1-2.5-2-4-2.5v-.5C15 3 14 2 12.5 2z"
      />
    </svg>
    <svg v-else-if="type === 'postgres'" viewBox="0 0 24 24" fill="currentColor">
      <path
        d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 15h-2v-2h2v2zm0-4h-2V7h2v6zm4 4h-2v-2h2v2zm0-4h-2V7h2v6z"
      />
    </svg>
    <svg v-else-if="type === 'sqlite'" viewBox="0 0 24 24" fill="currentColor">
      <path d="M4 4h16v16H4V4zm2 2v12h12V6H6zm2 2h8v2H8V8zm0 4h8v2H8v-2z" />
    </svg>
    <svg v-else-if="type === 'duckdb'" viewBox="0 0 24 24" fill="currentColor">
      <path
        d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2V7zm0 8h2v2h-2v-2z"
      />
    </svg>
    <svg v-else viewBox="0 0 24 24" fill="currentColor">
      <path
        d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z"
      />
    </svg>
  </div>
</template>

<script setup lang="ts">
interface Props {
  type: string
}

defineProps<Props>()
</script>

<style scoped>
.db-icon {
  width: 48px;
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
  font-size: 24px;
}

.db-icon svg {
  width: 28px;
  height: 28px;
}

.db-icon.mysql {
  background: rgba(0, 117, 143, 0.15);
  color: #00758f;
}

.db-icon.postgres {
  background: rgba(51, 103, 145, 0.15);
  color: #336791;
}

.db-icon.sqlite {
  background: rgba(0, 59, 87, 0.15);
  color: #003b57;
}

.db-icon.duckdb {
  background: rgba(255, 188, 0, 0.15);
  color: #ffbc00;
}
</style>
