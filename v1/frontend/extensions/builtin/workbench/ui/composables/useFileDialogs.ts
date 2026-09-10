/**
 * 文件操作确认对话框（naive-ui）
 * 提供统一的脏状态关闭 / 外部变更 / 窗口冲突对话框
 */
import { useDialog, useMessage } from 'naive-ui'
import { h } from 'vue'

type UnsavedActionResult = 'save' | 'discard' | 'cancel'
type ExternalChangeResult = 'reload' | 'keep'
type FileConflictResult = 'keep-local' | 'keep-remote' | 'merge'

let _dialog: ReturnType<typeof useDialog> | null = null
let _message: ReturnType<typeof useMessage> | null = null

function ensureDialog() {
  if (!_dialog) _dialog = useDialog()
  if (!_message) _message = useMessage()
  return { dialog: _dialog, message: _message }
}

/**
 * 关闭未保存文件确认
 * @returns 'save' | 'discard' | 'cancel'
 */
export async function confirmUnsavedClose(fileName: string): Promise<UnsavedActionResult> {
  const { dialog } = ensureDialog()

  return new Promise<UnsavedActionResult>(resolve => {
    const d = dialog.warning({
      title: '未保存的更改',
      content: `"${fileName}" 有未保存的更改，是否在关闭前保存？`,
      positiveText: '保存',
      negativeText: '不保存',
      closable: true,
      maskClosable: false,
      showIcon: true,
      onPositiveClick: () => {
        d.destroy()
        resolve('save')
      },
      onNegativeClick: () => {
        d.destroy()
        resolve('discard')
      },
      onClose: () => {
        d.destroy()
        resolve('cancel')
      },
      onMaskClick: () => {
        resolve('cancel')
      },
    })
  })
}

/**
 * 外部文件变更确认
 * @returns 'reload' | 'keep'
 */
export async function confirmExternalChange(fileName: string): Promise<ExternalChangeResult> {
  const { dialog } = ensureDialog()

  return new Promise<ExternalChangeResult>(resolve => {
    const d = dialog.warning({
      title: '文件已被外部修改',
      content: `"${fileName}" 在磁盘上已被其他程序修改。是否重新加载？选择"保持当前"将保留编辑器中的内容。`,
      positiveText: '重新加载',
      negativeText: '保持当前',
      closable: true,
      maskClosable: false,
      onPositiveClick: () => {
        d.destroy()
        resolve('reload')
      },
      onNegativeClick: () => {
        d.destroy()
        resolve('keep')
      },
      onClose: () => resolve('keep'),
      onMaskClick: () => resolve('keep'),
    })
  })
}

/**
 * 多窗口文件冲突确认
 * @returns 'keep-local' | 'keep-remote' | 'merge'
 */
export async function confirmFileConflict(fileName: string): Promise<FileConflictResult> {
  const { dialog } = ensureDialog()

  return new Promise<FileConflictResult>(resolve => {
    const d = dialog.warning({
      title: '文件冲突',
      content: `"${fileName}" 在另一个窗口也被修改了。请选择：`,
      positiveText: '保留本地',
      negativeText: '保留远程',
      action: () =>
        h(
          'button',
          {
            class: 'n-button n-button--default-type n-button--medium-type',
            onClick: () => {
              d.destroy()
              resolve('merge')
            },
          },
          '合并'
        ),
      closable: true,
      maskClosable: false,
      onPositiveClick: () => {
        d.destroy()
        resolve('keep-local')
      },
      onNegativeClick: () => {
        d.destroy()
        resolve('keep-remote')
      },
      onClose: () => resolve('keep-local'),
      onMaskClick: () => resolve('keep-local'),
    })
  })
}
