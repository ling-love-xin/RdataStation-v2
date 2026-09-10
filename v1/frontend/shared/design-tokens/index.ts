/**
 * 设计令牌 —— 统一入口
 *
 * ─── 职责 ───
 *   buildThemeOverrides() → 生成 naive-ui NConfigProvider 的 themeOverrides
 *   buildCSSOverrides()   → 生成密度驱动的 CSS 变量覆盖
 *   applyDensityClass()   → 在 body 上设置 data-density 属性
 *
 * ─── 数据流 ───
 *   AppearanceSettings → buildThemeOverrides() → App.vue NConfigProvider
 *                     → applyDensityClass()    → body CSS 变量
 *
 * @module design-tokens
 */

import type { AppearanceSettings } from '@/stores/config'

import { brandColors, isValidHex } from './colors'
import { densityConfigs } from './spacing'
import { fontFamilies } from './typography'

import type { DensityConfig } from './spacing'

/** 读取 body 上由 theme-dark/theme-light 设置的 CSS 变量 */
function readThemeCSSVars(): Record<string, string> {
  const bodyStyles = getComputedStyle(document.body)
  return {
    bgPrimary: bodyStyles.getPropertyValue('--color-bg-primary').trim(),
    bgSecondary: bodyStyles.getPropertyValue('--color-bg-secondary').trim(),
    bgElevated: bodyStyles.getPropertyValue('--color-bg-elevated').trim(),
    bgTertiary: bodyStyles.getPropertyValue('--color-bg-tertiary').trim(),
    textPrimary: bodyStyles.getPropertyValue('--color-text-primary').trim(),
    textSecondary: bodyStyles.getPropertyValue('--color-text-secondary').trim(),
    textMuted: bodyStyles.getPropertyValue('--color-text-muted').trim(),
    borderSubtle: bodyStyles.getPropertyValue('--color-border-subtle').trim(),
    hover: bodyStyles.getPropertyValue('--color-hover').trim(),
  }
}

function fallback(
  val: string,
  darkFallback: string,
  lightFallback: string,
  isDark: boolean
): string {
  return val || (isDark ? darkFallback : lightFallback)
}

/**
 * 从外观配置 + CSS 变量构建 naive-ui themeOverrides
 *
 * 此类支持项目级覆盖：当 effectiveAppearanceSettings 包含项目级
 * accentColor 时，naive-ui 的 primaryColor 会自动切换。
 */
function buildThemeOverrides(
  settings: AppearanceSettings,
  isDark: boolean
): Record<string, unknown> {
  const css = readThemeCSSVars()
  const accent = settings.accentColor || brandColors.coral
  const accentHover = brandColors.coralHover

  const bgPrimary = fallback(css.bgPrimary, '#1e1f22', '#ffffff', isDark)
  const bgSecondary = fallback(css.bgSecondary, '#2b2d30', '#f5f5f5', isDark)
  const bgElevated = fallback(css.bgElevated, '#3d4446', '#ffffff', isDark)
  const borderSubtle = fallback(css.borderSubtle, '#3c3f41', '#e5e7eb', isDark)
  const textPrimary = fallback(css.textPrimary, '#e5e7eb', '#1f2937', isDark)
  const textSecondary = fallback(css.textSecondary, '#9ca3af', '#4b5563', isDark)
  const textMuted = fallback(css.textMuted, '#6b7280', '#9ca3af', isDark)
  const hoverBg = fallback(css.hover, '#454545', '#e5e7e9', isDark)

  return {
    common: {
      primaryColor: accent,
      primaryColorHover: accentHover,
      primaryColorPressed: accentHover,
      primaryColorSuppl: accent,
      successColor: brandColors.success,
      warningColor: brandColors.warning,
      errorColor: brandColors.danger,
      infoColor: accent,
      bodyColor: bgPrimary,
      cardColor: bgElevated,
      modalColor: bgElevated,
      popoverColor: bgElevated,
      tableColor: bgSecondary,
      dividerColor: borderSubtle,
      borderColor: borderSubtle,
      hoverColor: hoverBg,
      fontFamily: settings.fontFamily || fontFamilies.sans,
      fontSize: `${settings.uiFontSize}px`,
      borderRadius: `${settings.borderRadius}px`,
    },
    Card: {
      color: bgElevated,
      borderColor: borderSubtle,
      titleTextColor: textPrimary,
      closeColor: textMuted,
      closeColorHover: textPrimary,
      titleFontWeight: '600',
    },
    Modal: {
      color: bgElevated,
      textColor: textPrimary,
    },
    Drawer: {
      color: bgPrimary,
      textColor: textPrimary,
      headerBorderColor: borderSubtle,
      footerBorderColor: borderSubtle,
    },
    Dropdown: {
      color: bgElevated,
      textColor: textPrimary,
      borderColor: borderSubtle,
      dividerColor: borderSubtle,
      optionColorHover: hoverBg,
    },
    Input: {
      color: css.bgTertiary || bgSecondary,
      colorFocus: bgPrimary,
      textColor: textPrimary,
      placeholderColor: textMuted,
      border: borderSubtle,
      borderHover: accent,
      borderFocus: accent,
    },
    Button: {
      colorTertiary: css.bgTertiary || bgSecondary,
      textColorTertiary: textSecondary,
      borderTertiary: borderSubtle,
    },
  }
}

/**
 * 生效密度配置：在 body 上应用 CSS 变量覆盖
 */
function applyDensityClass(density: AppearanceSettings['density']): void {
  const body = document.body
  body.removeAttribute('data-density')

  const config: DensityConfig | undefined = densityConfigs[density]
  if (config && density !== 'comfortable') {
    body.setAttribute('data-density', density)
  }
}

export { buildThemeOverrides, applyDensityClass, readThemeCSSVars, isValidHex }

export { brandColors } from './colors'
export { fontFamilies, fontSizes, fontSizeMap, fontWeights } from './typography'
export { spacings, densityConfigs, borderRadius, layoutHeights } from './spacing'
