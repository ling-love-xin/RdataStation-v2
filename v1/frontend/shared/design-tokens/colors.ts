/**
 * 设计令牌 —— 色板定义
 *
 * 所有颜色值统一从此处引用，禁止在组件中硬编码颜色
 */

/** 品牌色板 */
const brandColors = {
  coral: '#E17055',
  coralHover: '#D35400',
  coralSoft: 'rgba(225, 112, 85, 0.15)',
  success: '#00B894',
  danger: '#D63031',
  warning: '#FDCB6E',
} as const

/** 暗色主题调色板 */
const darkPalette = {
  bgPrimary: '#1e1f22',
  bgSecondary: '#2b2d30',
  bgTertiary: '#2d3436',
  bgElevated: '#3d4446',
  textPrimary: '#e5e7eb',
  textSecondary: '#9ca3af',
  textMuted: '#6b7280',
  border: '#4a5458',
  borderSubtle: '#3c3f41',
  hover: '#454545',
  selection: 'rgba(225, 112, 85, 0.25)',
  selectionText: '#1e1f22',
  warningText: '#fbbf24',
  warningBg: 'rgba(251, 191, 36, 0.12)',
} as const

/** 亮色主题调色板 */
const lightPalette = {
  bgPrimary: '#ffffff',
  bgSecondary: '#f5f5f5',
  bgTertiary: '#e5e7eb',
  bgElevated: '#ffffff',
  textPrimary: '#1f2937',
  textSecondary: '#4b5563',
  textMuted: '#9ca3af',
  border: '#b2bec3',
  borderSubtle: '#e5e7eb',
  hover: '#e5e7e9',
  selection: 'rgba(225, 112, 85, 0.15)',
  selectionText: '#ffffff',
  warningText: '#92400e',
  warningBg: 'rgba(251, 191, 36, 0.15)',
} as const

/** 资源类型色板 */
const resourceColors = {
  connection: '#165DFF',
  connectionSoft: 'rgba(22, 93, 255, 0.1)',
  table: '#00b42a',
  tableSoft: 'rgba(0, 180, 42, 0.1)',
  file: '#ff7d00',
  fileSoft: 'rgba(255, 125, 0, 0.1)',
  global: '#722ed1',
  globalSoft: 'rgba(114, 46, 209, 0.1)',
  project: '#165DFF',
  projectSoft: 'rgba(22, 93, 255, 0.1)',
  session: '#14c9c9',
  sessionSoft: 'rgba(20, 201, 201, 0.1)',
  tag: '#165DFF',
  folder: '#6366f1',
} as const

/** 健康度色板 */
const healthColors = {
  excellent: '#1a7a1a',
  good: '#1a6db5',
  fair: '#b57a1a',
  poor: '#b54a1a',
  critical: '#b51a1a',
} as const

/** 校验 hex 颜色合法性 */
function isValidHex(color: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(color)
}

export { brandColors, darkPalette, lightPalette, resourceColors, healthColors, isValidHex }
