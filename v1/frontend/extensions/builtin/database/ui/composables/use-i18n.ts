/**
 * 国际化支持逻辑
 */

import { ref, computed } from 'vue'

export type Locale = 'zh-CN' | 'en-US'

export interface I18nMessages {
  [key: string]: string | I18nMessages
}

export interface LocaleMessages {
  [locale: string]: I18nMessages
}

const defaultMessages: LocaleMessages = {
  'zh-CN': {
    navigator: {
      title: '数据库导航',
      search: '搜索...',
      filter: '过滤',
      refresh: '刷新',
      newConnection: '新建连接',
      disconnect: '断开连接',
      newGroup: '新建分组',
      beginTransaction: '开始事务',
      commitTransaction: '提交事务',
      rollbackTransaction: '回滚事务',
      loading: '加载中...',
      noConnections: '暂无连接',
      noSearchResults: '未找到匹配结果',
      connectionSuccess: '连接成功',
      connectionFailed: '连接失败',
      disconnectSuccess: '已断开连接',
      transactionStarted: '事务已开始',
      transactionCommitted: '事务已提交',
      transactionRolledBack: '事务已回滚',
      groupCreated: '分组创建成功',
      groupUpdated: '分组已更新',
      groupDeleted: '分组已删除',
    },
    filter: {
      databaseType: '数据库类型',
      connectionStatus: '连接状态',
      nodeType: '节点类型',
      showSystemObjects: '显示系统对象',
      all: '全部',
      connected: '已连接',
      connecting: '连接中',
      disconnected: '未连接',
      table: '表',
      view: '视图',
      procedure: '存储过程',
      function: '函数',
      column: '列',
      reset: '重置',
      apply: '应用',
    },
    toast: {
      success: '成功',
      error: '错误',
      info: '提示',
      warning: '警告',
    },
  },
  'en-US': {
    navigator: {
      title: 'Database Navigator',
      search: 'Search...',
      filter: 'Filter',
      refresh: 'Refresh',
      newConnection: 'New Connection',
      disconnect: 'Disconnect',
      newGroup: 'New Group',
      beginTransaction: 'Begin Transaction',
      commitTransaction: 'Commit Transaction',
      rollbackTransaction: 'Rollback Transaction',
      loading: 'Loading...',
      noConnections: 'No connections',
      noSearchResults: 'No results found',
      connectionSuccess: 'Connected successfully',
      connectionFailed: 'Connection failed',
      disconnectSuccess: 'Disconnected',
      transactionStarted: 'Transaction started',
      transactionCommitted: 'Transaction committed',
      transactionRolledBack: 'Transaction rolled back',
      groupCreated: 'Group created',
      groupUpdated: 'Group updated',
      groupDeleted: 'Group deleted',
    },
    filter: {
      databaseType: 'Database Type',
      connectionStatus: 'Connection Status',
      nodeType: 'Node Type',
      showSystemObjects: 'Show System Objects',
      all: 'All',
      connected: 'Connected',
      connecting: 'Connecting',
      disconnected: 'Disconnected',
      table: 'Table',
      view: 'View',
      procedure: 'Procedure',
      function: 'Function',
      column: 'Column',
      reset: 'Reset',
      apply: 'Apply',
    },
    toast: {
      success: 'Success',
      error: 'Error',
      info: 'Info',
      warning: 'Warning',
    },
  },
}

const currentLocale = ref<Locale>('zh-CN')
const messages = ref<LocaleMessages>(defaultMessages)

export function useI18n() {
  const locale = computed(() => currentLocale.value)

  function setLocale(locale: Locale) {
    currentLocale.value = locale
    localStorage.setItem('rdata-station-locale', locale)
  }

  function loadLocale(locale: Locale, customMessages: I18nMessages) {
    if (!messages.value[locale]) {
      messages.value[locale] = {}
    }
    messages.value[locale] = { ...messages.value[locale], ...customMessages }
  }

  function t(key: string, params?: Record<string, string | number>): string {
    const keys = key.split('.')
    let result: string | I18nMessages | undefined = messages.value[currentLocale.value]

    for (const k of keys) {
      if (result && typeof result === 'object' && k in result) {
        result = result[k]
      } else {
        return key
      }
    }

    if (typeof result === 'string') {
      if (params) {
        return result.replace(/\{\{(\w+)\}\}/g, (_, paramKey) => {
          return String(params[paramKey] ?? '')
        })
      }
      return result
    }

    return key
  }

  const availableLocales: { code: Locale; label: string }[] = [
    { code: 'zh-CN', label: '中文' },
    { code: 'en-US', label: 'English' },
  ]

  return {
    locale,
    setLocale,
    loadLocale,
    t,
    availableLocales,
  }
}
