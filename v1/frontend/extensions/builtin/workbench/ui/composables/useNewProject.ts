import { useMessage } from 'naive-ui'
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

export interface NewProjectForm {
  name: string
  description: string
  path: string
}

export function useNewProject(getVisible: () => boolean) {
  const { t } = useI18n()
  const message = useMessage()

  const isSubmitting = ref(false)

  const form = reactive<NewProjectForm>({
    name: '',
    description: '',
    path: '',
  })

  const canSubmit = computed(() => {
    return form.name.trim() && form.path.trim()
  })

  watch(
    () => getVisible(),
    isVisible => {
      if (isVisible) {
        form.name = ''
        form.description = ''
        form.path = ''
        isSubmitting.value = false
      }
    }
  )

  async function browsePath() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('workbench.selectProjectPath'),
      })

      if (selected && typeof selected === 'string') {
        form.path = selected
      }
    } catch {
      message.error(t('workbench.selectPathFailed'))
    }
  }

  function submit(onConfirm: (name: string, path: string, description?: string) => void) {
    if (!canSubmit.value || isSubmitting.value) return
    isSubmitting.value = true
    onConfirm(form.name.trim(), form.path.trim(), form.description.trim() || undefined)
  }

  return {
    form,
    isSubmitting,
    canSubmit,
    browsePath,
    submit,
  }
}
