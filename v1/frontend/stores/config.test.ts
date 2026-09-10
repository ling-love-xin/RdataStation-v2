import { describe, it, expect } from 'vitest'

import {
  TitleBarSettingsSchema,
  StatusBarSettingsSchema,
  CommandPaletteSettingsSchema,
  DEFAULT_GLOBAL_CONFIG,
  CONFIG_KEYS,
} from './config'

describe('TitleBarSettingsSchema', () => {
  it('should validate valid settings', () => {
    const valid = {
      menuStyle: 'full',
      toolbarTools: ['settings', 'history'],
      showProjectSelector: true,
      showCommandCenter: true,
      recentProjectCount: 5,
    }
    expect(TitleBarSettingsSchema.safeParse(valid).success).toBe(true)
  })

  it('should validate compact menu style', () => {
    const valid = {
      menuStyle: 'compact',
      toolbarTools: [],
      showProjectSelector: false,
      showCommandCenter: false,
      recentProjectCount: 1,
    }
    expect(TitleBarSettingsSchema.safeParse(valid).success).toBe(true)
  })

  it('should reject invalid menu style', () => {
    const invalid = {
      menuStyle: 'invalid',
      toolbarTools: [],
      showProjectSelector: true,
      showCommandCenter: true,
      recentProjectCount: 5,
    }
    expect(TitleBarSettingsSchema.safeParse(invalid).success).toBe(false)
  })

  it('should reject recentProjectCount < 1', () => {
    const invalid = {
      ...DEFAULT_GLOBAL_CONFIG.titleBarSettings,
      recentProjectCount: 0,
    }
    expect(TitleBarSettingsSchema.safeParse(invalid).success).toBe(false)
  })

  it('should reject recentProjectCount > 10', () => {
    const invalid = {
      ...DEFAULT_GLOBAL_CONFIG.titleBarSettings,
      recentProjectCount: 11,
    }
    expect(TitleBarSettingsSchema.safeParse(invalid).success).toBe(false)
  })
})

describe('StatusBarSettingsSchema', () => {
  it('should validate valid settings', () => {
    const valid = {
      visible: true,
      showConnectionStatus: true,
      showExecutionTime: true,
      showRowCount: true,
      showDuckDBIndicator: true,
      showEncoding: true,
      showVersion: true,
    }
    expect(StatusBarSettingsSchema.safeParse(valid).success).toBe(true)
  })

  it('should validate hidden status bar', () => {
    const valid = {
      visible: false,
      showConnectionStatus: false,
      showExecutionTime: false,
      showRowCount: false,
      showDuckDBIndicator: false,
      showEncoding: false,
      showVersion: false,
    }
    expect(StatusBarSettingsSchema.safeParse(valid).success).toBe(true)
  })
})

describe('CommandPaletteSettingsSchema', () => {
  it('should validate valid settings', () => {
    const valid = {
      maxRecentCommands: 5,
      includeDisabledCommands: false,
    }
    expect(CommandPaletteSettingsSchema.safeParse(valid).success).toBe(true)
  })

  it('should reject maxRecentCommands < 1', () => {
    const invalid = {
      maxRecentCommands: 0,
      includeDisabledCommands: false,
    }
    expect(CommandPaletteSettingsSchema.safeParse(invalid).success).toBe(false)
  })

  it('should reject maxRecentCommands > 20', () => {
    const invalid = {
      maxRecentCommands: 21,
      includeDisabledCommands: false,
    }
    expect(CommandPaletteSettingsSchema.safeParse(invalid).success).toBe(false)
  })
})

describe('DEFAULT_GLOBAL_CONFIG', () => {
  it('should have titleBarSettings with correct defaults', () => {
    expect(DEFAULT_GLOBAL_CONFIG.titleBarSettings.menuStyle).toBe('full')
    expect(DEFAULT_GLOBAL_CONFIG.titleBarSettings.toolbarTools).toEqual(['settings'])
    expect(DEFAULT_GLOBAL_CONFIG.titleBarSettings.showProjectSelector).toBe(true)
    expect(DEFAULT_GLOBAL_CONFIG.titleBarSettings.showCommandCenter).toBe(true)
    expect(DEFAULT_GLOBAL_CONFIG.titleBarSettings.recentProjectCount).toBe(5)
  })

  it('should have statusBarSettings with correct defaults', () => {
    expect(DEFAULT_GLOBAL_CONFIG.statusBarSettings.visible).toBe(true)
    expect(DEFAULT_GLOBAL_CONFIG.statusBarSettings.showConnectionStatus).toBe(true)
    expect(DEFAULT_GLOBAL_CONFIG.statusBarSettings.showExecutionTime).toBe(true)
    expect(DEFAULT_GLOBAL_CONFIG.statusBarSettings.showRowCount).toBe(true)
    expect(DEFAULT_GLOBAL_CONFIG.statusBarSettings.showDuckDBIndicator).toBe(true)
    expect(DEFAULT_GLOBAL_CONFIG.statusBarSettings.showEncoding).toBe(true)
    expect(DEFAULT_GLOBAL_CONFIG.statusBarSettings.showVersion).toBe(true)
  })

  it('should have commandPaletteSettings with correct defaults', () => {
    expect(DEFAULT_GLOBAL_CONFIG.commandPaletteSettings.maxRecentCommands).toBe(5)
    expect(DEFAULT_GLOBAL_CONFIG.commandPaletteSettings.includeDisabledCommands).toBe(false)
  })
})

describe('CONFIG_KEYS', () => {
  it('should have new config keys', () => {
    expect(CONFIG_KEYS.TITLE_BAR_SETTINGS).toBe('titleBarSettings')
    expect(CONFIG_KEYS.STATUS_BAR_SETTINGS).toBe('statusBarSettings')
    expect(CONFIG_KEYS.COMMAND_PALETTE_SETTINGS).toBe('commandPaletteSettings')
  })
})
