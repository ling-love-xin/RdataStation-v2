import { createI18n } from 'vue-i18n'

import enMessages from '@/shared/locales/en.json'
import zhCNMessages from '@/shared/locales/zh-CN.json'

const i18n = createI18n({
  locale: 'zh-CN',
  fallbackLocale: 'en',
  legacy: false,
  globalInjection: true,
  messages: {
    'zh-CN': zhCNMessages,
    en: enMessages,
  },
})

export default i18n
