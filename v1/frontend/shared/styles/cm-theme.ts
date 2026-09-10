import { HighlightStyle } from '@codemirror/language'
import { EditorView } from '@codemirror/view'
import { tags as t } from '@lezer/highlight'

const commonEditorStyles = {
  '&': {
    fontSize: '14px',
    fontFamily: '"JetBrains Mono", "Fira Code", "Consolas", monospace',
  },
  '.cm-content': {
    caretColor: 'var(--brand-accent, #e17055)',
    fontFamily: '"JetBrains Mono", "Fira Code", "Consolas", monospace',
  },
  '.cm-cursor, .cm-dropCursor': {
    borderLeftColor: 'var(--brand-accent, #e17055)',
  },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
    backgroundColor: 'rgba(225, 112, 85, 0.25)',
  },
  '.cm-activeLine': {
    backgroundColor: 'rgba(255, 255, 255, 0.04)',
  },
  '.cm-gutters': {
    borderRight: '1px solid rgba(255, 255, 255, 0.06)',
  },
  '.cm-foldPlaceholder': {
    backgroundColor: 'rgba(255, 255, 255, 0.06)',
    border: '1px solid rgba(255, 255, 255, 0.1)',
    color: '#8b949e',
  },
  '.cm-tooltip': {
    backgroundColor: '#2b2d30',
    border: '1px solid #4a5458',
    color: '#e5e7eb',
  },
  '.cm-tooltip-autocomplete li[aria-selected]': {
    backgroundColor: 'rgba(225, 112, 85, 0.2)',
    color: '#ffffff',
  },
}

export const rdataDarkTheme = EditorView.theme(
  {
    ...commonEditorStyles,
    '&': {
      ...commonEditorStyles['&'],
      backgroundColor: '#1e1f22',
      color: '#e5e7eb',
    },
    '.cm-activeLine': {
      backgroundColor: 'rgba(255, 255, 255, 0.04)',
    },
    '.cm-activeLineGutter': {
      backgroundColor: 'rgba(255, 255, 255, 0.02)',
    },
  },
  { dark: true }
)

export const rdataDarkHighlight = HighlightStyle.define([
  { tag: t.keyword, color: '#c678dd' },
  { tag: t.operator, color: '#abb2bf' },
  { tag: t.special(t.brace), color: '#abb2bf' },
  { tag: t.number, color: '#d19a66' },
  { tag: t.string, color: '#98c379' },
  { tag: t.typeName, color: '#e5c07b' },
  { tag: t.function(t.variableName), color: '#61afef' },
  { tag: t.function(t.definition(t.variableName)), color: '#61afef' },
  { tag: t.propertyName, color: '#e06c75' },
  { tag: t.comment, color: '#5c6370', fontStyle: 'italic' },
  { tag: t.bool, color: '#d19a66' },
  { tag: t.null, color: '#d19a66' },
  { tag: t.bracket, color: '#abb2bf' },
  { tag: t.tagName, color: '#e06c75' },
  { tag: t.attributeName, color: '#d19a66' },
  { tag: t.emphasis, color: '#abb2bf', fontStyle: 'italic' },
  { tag: t.strong, color: '#abb2bf', fontWeight: 'bold' },
])

export const rdataLightTheme = EditorView.theme(
  {
    ...commonEditorStyles,
    '&': {
      ...commonEditorStyles['&'],
      backgroundColor: '#ffffff',
      color: '#1e1f22',
    },
    '.cm-activeLine': {
      backgroundColor: 'rgba(0, 0, 0, 0.04)',
    },
    '.cm-activeLineGutter': {
      backgroundColor: 'rgba(0, 0, 0, 0.02)',
    },
    '.cm-gutters': {
      borderRight: '1px solid rgba(0, 0, 0, 0.08)',
    },
    '.cm-foldPlaceholder': {
      backgroundColor: 'rgba(0, 0, 0, 0.04)',
      border: '1px solid rgba(0, 0, 0, 0.08)',
      color: '#6b7280',
    },
    '.cm-tooltip': {
      backgroundColor: '#ffffff',
      border: '1px solid #d1d5db',
      color: '#1e1f22',
    },
  },
  { dark: false }
)

export const rdataLightHighlight = HighlightStyle.define([
  { tag: t.keyword, color: '#a626a4' },
  { tag: t.operator, color: '#383a42' },
  { tag: t.special(t.brace), color: '#383a42' },
  { tag: t.number, color: '#986801' },
  { tag: t.string, color: '#50a14f' },
  { tag: t.typeName, color: '#c18401' },
  { tag: t.function(t.variableName), color: '#4078f2' },
  { tag: t.function(t.definition(t.variableName)), color: '#4078f2' },
  { tag: t.propertyName, color: '#e45649' },
  { tag: t.comment, color: '#a0a1a7', fontStyle: 'italic' },
  { tag: t.bool, color: '#986801' },
  { tag: t.null, color: '#986801' },
  { tag: t.bracket, color: '#383a42' },
  { tag: t.tagName, color: '#e45649' },
  { tag: t.attributeName, color: '#986801' },
  { tag: t.emphasis, color: '#383a42', fontStyle: 'italic' },
  { tag: t.strong, color: '#383a42', fontWeight: 'bold' },
])
