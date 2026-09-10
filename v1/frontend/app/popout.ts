import { sql } from '@codemirror/lang-sql'
import { EditorView, keymap as cmKeymap, type ViewUpdate } from '@codemirror/view'
import { basicSetup } from 'codemirror'
import { DockviewVue } from 'dockview-vue'
import { createPinia } from 'pinia'
import { createApp, ref } from 'vue'

import 'dockview-vue/dist/styles/dockview.css'

import {
  listenPopoutTransfer,
  sendMergeTransfer,
  sendStateSync,
  sendWindowReady,
  type PopoutPayload,
} from '@/extensions/builtin/workbench/ui/composables/useCrossWindow'

const popoutFile = ref<PopoutPayload | null>(null)
let editorView: EditorView | null = null
let unlistenPopout: (() => void) | null = null

async function initPopoutListener(): Promise<void> {
  unlistenPopout = await listenPopoutTransfer(payload => {
    popoutFile.value = payload
    if (editorView) {
      editorView.dispatch({
        changes: {
          from: 0,
          to: editorView.state.doc.length,
          insert: payload.content,
        },
      })
    }
  })
}

function setupEditorView(container: HTMLElement): void {
  const lang = sql()

  editorView = new EditorView({
    doc: '',
    extensions: [
      basicSetup,
      lang,
      cmKeymap.of([
        {
          key: 'Mod-s',
          run: () => {
            if (popoutFile.value) {
              // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
              const content = editorView!.state.doc.toString()
              sendMergeTransfer({
                filePath: popoutFile.value.filePath,
                content,
                // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
                stateJSON: editorView!.state.toJSON() as Record<string, unknown>,
                isDirty: content !== popoutFile.value.content,
              })
            }
            return true
          },
          preventDefault: true,
        },
      ]),
      EditorView.updateListener.of((update: ViewUpdate) => {
        if (!popoutFile.value) return
        if (update.docChanged || update.selectionSet) {
          const cursor = update.state.selection.main.head
          const line = update.state.doc.lineAt(cursor)
          sendStateSync({
            filePath: popoutFile.value.filePath,
            content: update.state.doc.toString(),
            isDirty: update.state.doc.toString() !== popoutFile.value.content,
            cursorLine: line.number,
            cursorCol: cursor - line.from + 1,
          })
        }
      }),
    ],
    parent: container,
  })

  if (popoutFile.value) {
    editorView.dispatch({
      changes: {
        from: 0,
        insert: popoutFile.value.content,
      },
    })
  }
}

window.addEventListener('beforeunload', () => {
  if (popoutFile.value && editorView) {
    const currentContent = editorView.state.doc.toString()
    sendMergeTransfer({
      filePath: popoutFile.value.filePath,
      content: currentContent,
      stateJSON: editorView.state.toJSON() as Record<string, unknown>,
      isDirty: currentContent !== popoutFile.value.content,
    })
  }
  if (unlistenPopout) unlistenPopout()
})

const app = createApp({
  components: { DockviewVue },
  setup() {
    const editorContainer = ref<HTMLElement | null>(null)

    return {
      editorContainer,
      popoutFile,
      onReady() {
        sendWindowReady()
        initPopoutListener()

        if (editorContainer.value) {
          setupEditorView(editorContainer.value)
        }
      },
      setEditorRef(el: HTMLElement | null) {
        editorContainer.value = el
      },
    }
  },
  template: `
    <DockviewVue
      class="dockview"
      @ready="onReady"
    >
      <template #default>
        <div style="flex:1; display:flex; flex-direction:column; overflow:hidden;">
          <div
            v-if="!popoutFile"
            style="display:flex; align-items:center; justify-content:center; height:100%; color:#888;"
          >
            等待编辑器传输...
          </div>
          <div
            ref="setEditorRef"
            style="flex:1; overflow:hidden;"
          />
        </div>
      </template>
    </DockviewVue>
  `,
})

app.use(createPinia())
app.mount('#popout-root')
