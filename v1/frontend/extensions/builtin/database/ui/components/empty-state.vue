<template>
  <div class="empty-state">
    <div class="empty-icon">
      <Database :size="48" />
    </div>
    
    <div class="empty-content">
      <h3>{{ title }}</h3>
      <p>{{ description }}</p>
      
      <div class="empty-actions">
        <button 
          v-for="action in actions" 
          :key="action.id"
          class="btn-action"
          :class="{ primary: action.primary }"
          @click="$emit('action', action.id)"
        >
          <component :is="action.icon" :size="14" />
          {{ action.label }}
        </button>
      </div>
    </div>

    <div class="empty-decoration">
      <div class="decoration-circle"></div>
      <div class="decoration-circle small"></div>
      <div class="decoration-circle tiny"></div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Database } from 'lucide-vue-next'

import type { Component } from 'vue'

interface Action {
  id: string
  label: string
  icon: Component
  primary?: boolean
}

defineProps<{
  title?: string
  description?: string
  actions?: Action[]
}>()

defineEmits<{
  action: [id: string]
}>()
</script>

<style scoped>
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 200px;
  padding: 40px;
  position: relative;
  overflow: hidden;
}

.empty-icon {
  width: 96px;
  height: 96px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, rgba(0, 180, 100, 0.1), rgba(100, 100, 255, 0.1));
  border-radius: 24px;
  margin-bottom: 20px;
  color: var(--primary-color);
}

.empty-content {
  text-align: center;
  z-index: 1;
}

.empty-content h3 {
  margin: 0 0 8px 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.empty-content p {
  margin: 0 0 24px 0;
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.5;
}

.empty-actions {
  display: flex;
  gap: 8px;
  justify-content: center;
  flex-wrap: wrap;
}

.btn-action {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.2s;
}

.btn-action:hover {
  background: var(--bg-hover);
}

.btn-action.primary {
  background: var(--primary-color);
  color: #fff;
  border-color: var(--primary-color);
}

.btn-action.primary:hover {
  opacity: 0.9;
}

.empty-decoration {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  pointer-events: none;
}

.decoration-circle {
  position: absolute;
  border-radius: 50%;
  background: linear-gradient(135deg, rgba(0, 180, 100, 0.04), rgba(100, 100, 255, 0.04));
  width: 120px;
  height: 120px;
  top: -20px;
  right: -20px;
}

.decoration-circle.small {
  width: 80px;
  height: 80px;
  bottom: 10px;
  left: 10px;
  top: auto;
  right: auto;
}

.decoration-circle.tiny {
  width: 40px;
  height: 40px;
  top: 30px;
  left: 30px;
  bottom: auto;
  right: auto;
}
</style>