/**
 * useProfileForm — 网络配置 Profile 表单 CRUD 通用 Composable
 *
 * 从 NetworkConfigManager.vue 提取，消除 SSH/SSL/Proxy
 * 三套高度重复的表单管理逻辑（showForm / editingId / reset / edit / cancel / test / save）。
 *
 * @param defaults  表单字段默认值
 * @param opts.onSave  保存回调（emit 到父组件）
 * @param opts.testMsg  测试按钮的 alert 消息模板（可选）
 */
import { ref, reactive, type Ref } from 'vue'

import type { NetworkProfile } from './useNetworkProfiles'

export interface ProfileFormOptions<T extends Record<string, unknown>> {
  onSave: (form: T & { id?: string | null }) => void
  testMsg?: (form: T) => string
}

export function useProfileForm<T extends Record<string, unknown>>(
  defaults: T,
  opts: ProfileFormOptions<T>
) {
  const showForm = ref(false)
  const editingId: Ref<string | null> = ref(null)
  const form = reactive<T>({ ...defaults })

  function resetForm() {
    Object.assign(form, defaults)
    editingId.value = null
  }

  /** 从 NetworkProfile 加载字段到表单 */
  function edit(profile: NetworkProfile, fieldMapper: (p: NetworkProfile) => Partial<T>) {
    Object.assign(form, defaults)
    const mapped = fieldMapper(profile)
    Object.assign(form, mapped)
    editingId.value = profile.id
    showForm.value = true
  }

  function cancelForm() {
    showForm.value = false
    resetForm()
  }

  function testForm() {
    const msg = opts.testMsg ? opts.testMsg(form as unknown as T) : '测试功能'
    // eslint-disable-next-line no-alert
    alert(msg)
  }

  function saveForm() {
    const name = (form as unknown as Record<string, unknown>).name
    if (!name || !String(name).trim()) {
      // eslint-disable-next-line no-alert
      alert('请填写配置名称')
      return
    }
    opts.onSave({ ...form, id: editingId.value } as T & { id?: string | null })
    cancelForm()
  }

  return { showForm, editingId, form, resetForm, edit, cancelForm, testForm, saveForm }
}

/** 判断 NetworkProfile 是否为全局作用域 */
export function isGlobalProfile(p: NetworkProfile): boolean {
  return p.origin === 'global'
}
