/**
 * 设计令牌 —— 字体阶梯定义
 */

/** 字体族 */
const fontFamilies = {
  sans: "'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
  mono: "'JetBrains Mono', 'Consolas', 'Monaco', monospace",
  editor: "'Cascadia Code', 'Fira Code', 'Consolas', monospace",
} as const

/** 字体大小阶梯 */
const fontSizes = {
  xxs: '9px',
  xs: '10px',
  sm: '12px',
  md: '13px',
  lg: '14px',
  xl: '16px',
  xxl: '18px',
  title: '20px',
  display: '48px',
} as const

/** 字体大小数值（供 slider 等组件使用） */
const fontSizeMap: Record<string, number> = {
  xxs: 9,
  xs: 10,
  sm: 12,
  md: 13,
  lg: 14,
  xl: 16,
  xxl: 18,
  title: 20,
  display: 48,
}

/** 字重阶梯 */
const fontWeights = {
  normal: '400',
  medium: '500',
  semibold: '600',
  bold: '700',
} as const

export { fontFamilies, fontSizes, fontSizeMap, fontWeights }
