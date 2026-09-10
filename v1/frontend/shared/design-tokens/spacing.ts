/**
 * 设计令牌 —— 间距/密度/圆角定义
 */

/** 间距阶梯 */
const spacings = {
  xs: '4px',
  sm: '8px',
  md: '12px',
  lg: '16px',
  xl: '24px',
} as const

/** 密度配置 */
type DensityConfig = {
  scale: number
  spacingOverrides: Record<string, string>
  fontSizeBase: number
}

const densityConfigs: Record<string, DensityConfig> = {
  compact: {
    scale: 0.75,
    spacingOverrides: {
      '--spacing-xs': '2px',
      '--spacing-sm': '4px',
      '--spacing-md': '8px',
      '--spacing-lg': '12px',
      '--spacing-xl': '16px',
    },
    fontSizeBase: 12,
  },
  comfortable: {
    scale: 1,
    spacingOverrides: {
      '--spacing-xs': '4px',
      '--spacing-sm': '8px',
      '--spacing-md': '12px',
      '--spacing-lg': '16px',
      '--spacing-xl': '24px',
    },
    fontSizeBase: 13,
  },
  spacious: {
    scale: 1.25,
    spacingOverrides: {
      '--spacing-xs': '6px',
      '--spacing-sm': '12px',
      '--spacing-md': '16px',
      '--spacing-lg': '20px',
      '--spacing-xl': '32px',
    },
    fontSizeBase: 14,
  },
}

/** 圆角阶梯 */
const borderRadius = {
  sm: '4px',
  md: '6px',
  lg: '8px',
  xl: '12px',
  pill: '999px',
} as const

/** 布局高度常量 */
const layoutHeights = {
  titleBar: '36px',
  statusBar: '22px',
  input: '32px',
} as const

export { spacings, densityConfigs, borderRadius, layoutHeights }

export type { DensityConfig }
