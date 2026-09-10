import { type Diagnostic, setDiagnostics } from '@codemirror/lint'
import { EditorView } from '@codemirror/view'

export function setEditorDiagnostics(view: EditorView, diagnostics: Diagnostic[]): void {
  if (!view.dom.isConnected) return
  view.dispatch(setDiagnostics(view.state, diagnostics))
}

export function clearEditorDiagnostics(view: EditorView): void {
  if (!view.dom.isConnected) return
  view.dispatch(setDiagnostics(view.state, []))
}

export function createErrorDiagnostic(
  view: EditorView,
  message: string,
  line: number,
  column: number
): Diagnostic {
  const doc = view.state.doc
  let from = 0
  try {
    const lineObj = doc.line(line)
    from = lineObj.from + Math.max(0, column - 1)
  } catch {
    from = 0
  }

  return {
    from,
    to: from + 1,
    severity: 'error',
    message,
  }
}
