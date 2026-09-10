/**
 * 设计令牌 Composable —— 组件级统一的样式消费入口
 *
 * ─── 用法 ───
 *   const { tokens } = useDesignTokens()
 *   // tokens.value.fontSize  → '13px'
 *   // tokens.value.accentColor → '#E17055'
 *
 * ─── 数据流 ───
 *   AppearanceSettings (config) → useDesignTokens() → 组件 style 绑定
 *
 * @module useDesignTokens
 */

import { computed } from 'vue'

import { brandColors } from '@/shared/design-tokens/colors'
import { borderRadius, spacings } from '@/shared/design-tokens/spacing'
import { fontFamilies, fontSizes } from '@/shared/design-tokens/typography'
import type { AppearanceSettings } from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'

function useDesignTokens() {
  const appStore = useAppStore()

  const tokens = computed(() => {
    const settings: AppearanceSettings = appStore.effectiveAppearanceSettings
    const accent = settings.accentColor || brandColors.coral

    return {
      accentColor: accent,
      accentColorSoft: `${accent}26`,
      fontSize: `${settings.uiFontSize}px`,
      fontFamily: settings.fontFamily || fontFamilies.sans,
      borderRadius: `${settings.borderRadius}px`,
      borderRadiusLg: `${settings.borderRadius + 2}px`,
      isCompact: settings.compactMode,
      density: settings.density,

      fontSizes,
      fontFamilies,
      spacings,
      borderRadiusSteps: borderRadius,

      /** 组件超大字体（如对话框标题） */
      fontSizeLg: `${settings.uiFontSize + 2}px`,

      /** 间距缩放因子，compact=0.75, comfortable=1, spacious=1.25 */
      densityScale:
        settings.density === 'compact' ? 0.75 : settings.density === 'spacious' ? 1.25 : 1,
    }
  })

  return { tokens }
}

export { useDesignTokens }
