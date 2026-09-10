<template>
  <div v-if="qualityScore" class="quality-score-section">
    <div class="quality-badge" :class="scoreLevelClass">
      <span class="quality-score-num">{{ Math.round(qualityScore.overall_score) }}</span>
      <span class="quality-level">{{ tLevel }}</span>
    </div>
    <div class="quality-summary">{{ tSummary }}</div>
    <div class="quality-dimensions">
      <div v-for="dim in qualityScore.dimensions" :key="dim.name" class="quality-dim">
        <div class="dim-header">
          <span class="dim-name">{{ tDimName(dim.name) }}</span>
          <span class="dim-score">{{ Math.round(dim.score) }}</span>
        </div>
        <div class="dim-bar-track">
          <div
            class="dim-bar-fill"
            :style="{ width: dim.score + '%' }"
            :class="dimScoreBarClass(dim.score)"
          />
        </div>
        <span class="dim-detail">{{ dim.detail }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { QualityScore } from '../../../services/result-analysis'

const { t } = useI18n()

const props = defineProps<{
  qualityScore: QualityScore | null
}>()

const dimNameMap: Record<string, string> = {
  completeness: 'schemaInsight.dimCompleteness',
  uniqueness: 'schemaInsight.dimUniqueness',
  type_consistency: 'schemaInsight.dimTypeConsistency',
  distribution: 'schemaInsight.dimDistribution',
}

const levelMap: Record<string, string> = {
  excellent: 'schemaInsight.levelExcellent',
  good: 'schemaInsight.levelGood',
  fair: 'schemaInsight.levelFair',
  poor: 'schemaInsight.levelPoor',
  critical: 'schemaInsight.levelCritical',
}

const summaryMap: Record<string, string> = {
  quality_excellent: 'schemaInsight.qualityExcellent',
  quality_good: 'schemaInsight.qualityGood',
  quality_fair: 'schemaInsight.qualityFair',
  quality_poor: 'schemaInsight.qualityPoor',
}

function tDimName(key: string): string {
  return t(dimNameMap[key] ?? key)
}

const tLevel = computed(() => {
  if (!props.qualityScore) return ''
  return t(levelMap[props.qualityScore.level] ?? props.qualityScore.level)
})

const tSummary = computed(() => {
  if (!props.qualityScore) return ''
  const s = props.qualityScore.summary
  const colonIdx = s.indexOf(':')
  if (colonIdx > 0) {
    const key = s.substring(0, colonIdx)
    const score = s.substring(colonIdx + 1).trim()
    const template = t(summaryMap[key] ?? key)
    return template.replace('{score}', score)
  }
  return s
})

const scoreLevelClass = computed(() => {
  if (!props.qualityScore) return ''
  const s = props.qualityScore.overall_score
  if (s >= 85) return 'score-excellent'
  if (s >= 70) return 'score-good'
  if (s >= 50) return 'score-fair'
  if (s >= 30) return 'score-poor'
  return 'score-bad'
})

function dimScoreBarClass(score: number): string {
  if (score >= 80) return 'bar-good'
  if (score >= 50) return 'bar-fair'
  return 'bar-poor'
}
</script>

<style scoped>
.quality-score-section {
  padding: var(--spacing-sm) var(--spacing-md);
  margin: 6px 0;
  background: var(--bg-elevated);
  border-radius: var(--border-radius-md);
  border: 1px solid var(--border-color);
}
.quality-badge {
  display: inline-flex;
  align-items: baseline;
  gap: 6px;
  padding: var(--spacing-xs) var(--spacing-sm);
  border-radius: var(--border-radius-sm);
  margin-bottom: 6px;
}
.quality-badge.score-excellent {
  background: rgba(0, 184, 148, 0.15);
  color: var(--brand-success);
}
.quality-badge.score-good {
  background: var(--brand-info-soft);
  color: var(--brand-info);
}
.quality-badge.score-fair {
  background: rgba(253, 203, 110, 0.15);
  color: var(--brand-warning);
}
.quality-badge.score-poor {
  background: var(--brand-danger-soft);
  color: var(--brand-danger);
}
.quality-badge.score-bad {
  background: rgba(214, 48, 49, 0.25);
  color: var(--brand-danger);
}

.quality-score-num {
  font-size: var(--font-size-xxl);
  font-weight: 700;
  line-height: 1;
}
.quality-level {
  font-size: var(--font-size-xss);
  opacity: 0.8;
}
.quality-summary {
  font-size: var(--font-size-xss);
  color: var(--text-secondary);
  margin-bottom: var(--spacing-sm);
  line-height: 1.4;
}

.quality-dimensions {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.quality-dim {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.dim-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.dim-name {
  font-size: var(--font-size-xss);
  font-weight: 500;
}
.dim-score {
  font-size: var(--font-size-sm);
  font-weight: 600;
}

.dim-bar-track {
  height: var(--spacing-xs);
  background: var(--bg-secondary);
  border-radius: 2px;
  overflow: hidden;
}
.dim-bar-fill {
  height: 100%;
  border-radius: 2px;
  transition: width 0.4s ease;
}
.dim-bar-fill.bar-good {
  background: var(--brand-success);
}
.dim-bar-fill.bar-fair {
  background: var(--brand-warning);
}
.dim-bar-fill.bar-poor {
  background: var(--brand-danger);
}

.dim-detail {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  line-height: 1.3;
}
</style>
