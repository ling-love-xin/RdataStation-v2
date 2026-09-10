/**
 * 数据源 Tab 统一样式配置
 *
 * 供模板 :class 绑定使用，确保五个 Tab 页面视觉风格统一。
 * 所有 CSS 值均引用 tokens.css 中定义的变量，遵循 frontend-enterprise-spec：
 *   - 颜色：--color-*, --brand-*
 *   - 间距：--spacing-xs/sm/md/lg/xl
 *   - 圆角：--border-radius-sm/md/lg
 *   - 字号：--font-size-sm/md/lg/xl
 *   - 输入高度：--height-input (32px)
 */

// ============================================================
// CSS Class Name 常量（用于 :class 绑定）
// ============================================================

/** Tab 根容器 */
export const TAB_ROOT = 'tab-root' as const

/** 信息横幅 */
export const TAB_INFO_BANNER = 'tab-info-banner' as const

/** 信息横幅图标 */
export const TAB_INFO_BANNER_ICON = 'tab-info-banner-icon' as const

/** 分区容器 */
export const TAB_SECTION = 'tab-section' as const

/** 分区标题 */
export const TAB_SECTION_TITLE = 'tab-section-title' as const

/** 分区标题（带图标） */
export const TAB_SECTION_TITLE_ICON = 'tab-section-title has-icon' as const

/** 表单组 */
export const TAB_FORM_GROUP = 'tab-form-group' as const

/** 表单组 flex=1 */
export const TAB_FORM_GROUP_F1 = 'tab-form-group flex-1' as const

/** 表单组 flex=2 */
export const TAB_FORM_GROUP_F2 = 'tab-form-group flex-2' as const

/** 表单标签 */
export const TAB_FORM_LABEL = 'tab-form-label' as const

/** 表单输入 */
export const TAB_FORM_INPUT = 'tab-form-input' as const

/** 表单下拉 */
export const TAB_FORM_SELECT = 'tab-form-select' as const

/** 表单行 */
export const TAB_FORM_ROW = 'tab-form-row' as const

/** 表单分区标签 */
export const TAB_FORM_SECTION_LABEL = 'tab-form-section-label' as const

/** 空状态容器 */
export const TAB_EMPTY_STATE = 'tab-empty-state' as const

/** 空状态图标 */
export const TAB_EMPTY_ICON = 'tab-empty-icon' as const

/** 空状态标题 */
export const TAB_EMPTY_TITLE = 'tab-empty-title' as const

/** 空状态描述 */
export const TAB_EMPTY_DESC = 'tab-empty-desc' as const

/** 文件数据库提示 */
export const TAB_FILE_HINT = 'tab-file-hint' as const

/** 文件数据库提示图标 */
export const TAB_FILE_HINT_ICON = 'tab-file-hint-icon' as const

/** 文件数据库提示标题 */
export const TAB_FILE_HINT_TITLE = 'tab-file-hint-title' as const

/** 文件数据库提示描述 */
export const TAB_FILE_HINT_DESC = 'tab-file-hint-desc' as const

/** 网络内容容器 */
export const TAB_NETWORK_CONTENT = 'tab-network-content' as const

/** 网络提示横幅 */
export const TAB_NETWORK_HINT = 'tab-network-hint' as const

/** 网络提示描述 */
export const TAB_NETWORK_HINT_DESC = 'tab-network-hint-desc' as const

/** 统一表格 */
export const TAB_TABLE = 'tab-table' as const

/** 能力徽章 */
export const TAB_CAP_BADGE = 'tab-cap-badge' as const

/** 能力徽章 - 支持 */
export const TAB_CAP_BADGE_YES = 'tab-cap-badge supported' as const

/** 能力徽章 - 不支持 */
export const TAB_CAP_BADGE_NO = 'tab-cap-badge unsupported' as const

/** 驱动属性输入 */
export const TAB_PROP_INPUT = 'tab-prop-input' as const

/** 双列网格 */
export const TAB_ADV_GRID = 'tab-adv-grid' as const

/** 加速卡片 */
export const TAB_ACCEL_CARD = 'tab-accel-card' as const

/** 加速卡片头部 */
export const TAB_ACCEL_HEADER = 'tab-accel-header' as const

/** 加速卡片图标 */
export const TAB_ACCEL_ICON = 'tab-accel-icon' as const

/** 加速卡片名称 */
export const TAB_ACCEL_NAME = 'tab-accel-name' as const

/** 加速卡片主体 */
export const TAB_ACCEL_BODY = 'tab-accel-body' as const

/** 加速卡片描述 */
export const TAB_ACCEL_DESC = 'tab-accel-desc' as const

/** 加速开关容器 */
export const TAB_ACCEL_SWITCH = 'tab-accel-switch' as const

/** 开关 */
export const TAB_SWITCH_TOGGLE = 'tab-switch-toggle' as const

/** 开关 - 开启 */
export const TAB_SWITCH_TOGGLE_ON = 'tab-switch-toggle on' as const

/** 范围徽章 */
export const TAB_SCOPE_BADGE = 'tab-scope-badge' as const

/** 范围徽章 - 全局 */
export const TAB_SCOPE_BADGE_GLOBAL = 'tab-scope-badge global' as const

/** 范围徽章 - 项目 */
export const TAB_SCOPE_BADGE_PROJECT = 'tab-scope-badge project' as const

/** 环境策略提示 */
export const TAB_ENV_HINT = 'tab-env-hint' as const

/** 窄选择器 */
export const TAB_SELECT_NARROW = 'tab-select-narrow' as const

/** 更窄选择器 */
export const TAB_SELECT_NARROWER = 'tab-select-narrower' as const

// ============================================================
// 动态 Style 对象（用于 :style 绑定，覆盖默认不适用场景）
// ============================================================

/** 内联设置固定宽度 */
export function styleWidth(px: number): Record<string, string> {
  return { width: `${px}px` }
}

/** 内联设置列百分比宽度（表格） */
export function styleColWidth(pct: string): Record<string, string> {
  return { width: pct }
}
