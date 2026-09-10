import { closeBrackets, autocompletion, completionKeymap } from '@codemirror/autocomplete'
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { sql } from '@codemirror/lang-sql'
import {
  indentOnInput,
  bracketMatching,
  foldGutter,
  syntaxHighlighting,
} from '@codemirror/language'
import { lintGutter } from '@codemirror/lint'
import { highlightSelectionMatches, searchKeymap } from '@codemirror/search'
import { EditorState, type Extension, Compartment } from '@codemirror/state'
import { EditorView, keymap as cmKeymap } from '@codemirror/view'
import { shallowRef, type ShallowRef } from 'vue'

import {
  rdataDarkTheme,
  rdataDarkHighlight,
  rdataLightTheme,
  rdataLightHighlight,
} from '@/shared/styles/cm-theme'

import { type LargeFileStrategy } from './useLargeFile'

export type LanguageType = 'sql' | 'python' | 'json' | 'plaintext' | string

export type EditorUpdateCallback = (
  doc: string,
  cursorLine: number,
  cursorCol: number,
  hasSelection: boolean
) => void

export interface UseCodeMirrorReturn {
  view: ShallowRef<EditorView | null>
  createView(
    parent: HTMLElement,
    doc: string,
    language: LanguageType,
    theme: 'dark' | 'light',
    onUpdate?: EditorUpdateCallback,
    extraExtensions?: Extension[],
    strategy?: LargeFileStrategy
  ): EditorView
  destroyView(): void
  getValue(): string
  setValue(text: string): void
  getSelection(): { from: number; to: number; text: string } | null
  getEditorState(): EditorState | null
  setEditorState(state: EditorState): void
  setLanguage(language: LanguageType): void
  setTheme(theme: 'dark' | 'light'): void
  focus(): void
  revealLine(line: number): void
  reconfigureExtra(extensions: Extension[]): void
}

export function useCodeMirror(): UseCodeMirrorReturn {
  const view: ShallowRef<EditorView | null> = shallowRef(null)

  const languageCompartment = new Compartment()
  const themeCompartment = new Compartment()
  const extraCompartment = new Compartment()

  function getLanguageExtension(language: LanguageType): Extension {
    switch (language) {
      case 'sql':
        return sql({})
      default:
        return []
    }
  }

  function getThemeExtension(theme: 'dark' | 'light'): Extension {
    if (theme === 'dark') {
      return [rdataDarkTheme, syntaxHighlighting(rdataDarkHighlight)]
    }
    return [rdataLightTheme, syntaxHighlighting(rdataLightHighlight)]
  }

  function createView(
    parent: HTMLElement,
    doc: string,
    language: LanguageType = 'sql',
    theme: 'dark' | 'light' = 'dark',
    onUpdate?: EditorUpdateCallback,
    extraExtensions: Extension[] = [],
    strategy?: LargeFileStrategy
  ): EditorView {
    if (view.value) {
      view.value.destroy()
    }

    const s = strategy ?? null
    const isLarge = s?.tier === 'large' || s?.tier === 'chunked' || s?.tier === 'rejected'
    const isChunked = s?.tier === 'chunked' || s?.tier === 'rejected'

    const baseExtensions: Extension[] = [EditorView.lineWrapping]

    if (!isChunked) {
      baseExtensions.push(indentOnInput())
    }

    if (!isLarge) {
      baseExtensions.push(bracketMatching())
      baseExtensions.push(closeBrackets())
    }

    if (!isChunked) {
      baseExtensions.push(highlightSelectionMatches())
    }

    if (!isLarge) {
      baseExtensions.push(lintGutter())
      baseExtensions.push(foldGutter())
    }

    if (s?.limitHistoryDepth && s.historyDepth > 0) {
      baseExtensions.push(history({ minDepth: s.historyDepth }))
    } else if (!isChunked) {
      baseExtensions.push(history())
    }

    if (!isLarge) {
      baseExtensions.push(
        autocompletion({
          closeOnBlur: true,
          activateOnTyping: true,
        })
      )
    }

    const keymaps = [defaultKeymap, historyKeymap, searchKeymap]
    if (!isLarge) {
      keymaps.push(completionKeymap)
    }
    baseExtensions.push(cmKeymap.of(keymaps.flat()))

    if (language !== 'plaintext') {
      baseExtensions.push(languageCompartment.of(getLanguageExtension(language)))
    }

    baseExtensions.push(themeCompartment.of(getThemeExtension(theme)))

    if (extraExtensions.length > 0) {
      baseExtensions.push(extraCompartment.of(extraExtensions))
    }

    if (onUpdate) {
      baseExtensions.push(
        EditorView.updateListener.of(update => {
          if (update.docChanged || update.selectionSet) {
            const state = update.state
            const cursor = state.selection.main.head
            const line = state.doc.lineAt(cursor)
            const hasSelection = !state.selection.main.empty
            onUpdate(state.doc.toString(), line.number, cursor - line.from + 1, hasSelection)
          }
        })
      )
    }

    const state = EditorState.create({
      doc,
      extensions: baseExtensions,
    })

    const editorView = new EditorView({
      parent,
      state,
    })

    view.value = editorView
    return editorView
  }

  function destroyView(): void {
    if (view.value) {
      view.value.destroy()
      view.value = null
    }
  }

  function getValue(): string {
    return view.value?.state.doc.toString() ?? ''
  }

  function setValue(text: string): void {
    const v = view.value
    if (!v) return
    v.dispatch({
      changes: { from: 0, to: v.state.doc.length, insert: text },
    })
  }

  function getSelection(): { from: number; to: number; text: string } | null {
    const v = view.value
    if (!v) return null
    const sel = v.state.selection.main
    if (sel.empty) return null
    return {
      from: sel.from,
      to: sel.to,
      text: v.state.doc.sliceString(sel.from, sel.to),
    }
  }

  function getEditorState(): EditorState | null {
    return view.value?.state ?? null
  }

  function setEditorState(state: EditorState): void {
    const v = view.value
    if (!v) return
    v.setState(state)
  }

  function setLanguage(language: LanguageType): void {
    const v = view.value
    if (!v) return
    v.dispatch({
      effects: languageCompartment.reconfigure(getLanguageExtension(language)),
    })
  }

  function setTheme(theme: 'dark' | 'light'): void {
    const v = view.value
    if (!v) return
    v.dispatch({
      effects: themeCompartment.reconfigure(getThemeExtension(theme)),
    })
  }

  function focus(): void {
    view.value?.focus()
  }

  function revealLine(line: number): void {
    const v = view.value
    if (!v) return
    try {
      const pos = v.state.doc.line(line).from
      v.dispatch({
        effects: EditorView.scrollIntoView(pos, { y: 'center' }),
      })
    } catch {
      console.warn('[useCodeMirror] gotoLine invalid line number')
    }
  }

  function reconfigureExtra(extensions: Extension[]): void {
    const v = view.value
    if (!v) return
    v.dispatch({
      effects: extraCompartment.reconfigure(extensions),
    })
  }

  return {
    view,
    createView,
    destroyView,
    getValue,
    setValue,
    getSelection,
    getEditorState,
    setEditorState,
    setLanguage,
    setTheme,
    focus,
    revealLine,
    reconfigureExtra,
  }
}
