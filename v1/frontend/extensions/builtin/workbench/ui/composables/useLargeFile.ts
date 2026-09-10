export type FileSizeTier = 'normal' | 'reduced' | 'large' | 'chunked' | 'rejected'

export interface LargeFileStrategy {
  tier: FileSizeTier
  sizeMB: number
  limitHistoryDepth: boolean
  historyDepth: number
  enableCompletion: boolean
  enableFoldGutter: boolean
  enableLintGutter: boolean
  enableBracketMatching: boolean
  enableHighlightSelection: boolean
  enableSyntaxHighlighting: boolean
  chunkSize: number | null
}

const STRATEGIES: Record<FileSizeTier, LargeFileStrategy> = {
  normal: {
    tier: 'normal',
    sizeMB: 0,
    limitHistoryDepth: false,
    historyDepth: 0,
    enableCompletion: true,
    enableFoldGutter: true,
    enableLintGutter: true,
    enableBracketMatching: true,
    enableHighlightSelection: true,
    enableSyntaxHighlighting: true,
    chunkSize: null,
  },
  reduced: {
    tier: 'reduced',
    sizeMB: 1,
    limitHistoryDepth: true,
    historyDepth: 200,
    enableCompletion: true,
    enableFoldGutter: false,
    enableLintGutter: true,
    enableBracketMatching: true,
    enableHighlightSelection: true,
    enableSyntaxHighlighting: true,
    chunkSize: null,
  },
  large: {
    tier: 'large',
    sizeMB: 10,
    limitHistoryDepth: true,
    historyDepth: 100,
    enableCompletion: false,
    enableFoldGutter: false,
    enableLintGutter: false,
    enableBracketMatching: false,
    enableHighlightSelection: true,
    enableSyntaxHighlighting: true,
    chunkSize: null,
  },
  chunked: {
    tier: 'chunked',
    sizeMB: 50,
    limitHistoryDepth: false,
    historyDepth: 0,
    enableCompletion: false,
    enableFoldGutter: false,
    enableLintGutter: false,
    enableBracketMatching: false,
    enableHighlightSelection: false,
    enableSyntaxHighlighting: false,
    chunkSize: 5000,
  },
  rejected: {
    tier: 'rejected',
    sizeMB: 200,
    limitHistoryDepth: false,
    historyDepth: 0,
    enableCompletion: false,
    enableFoldGutter: false,
    enableLintGutter: false,
    enableBracketMatching: false,
    enableHighlightSelection: false,
    enableSyntaxHighlighting: false,
    chunkSize: null,
  },
}

export function classifyFileSize(content: string): LargeFileStrategy {
  const sizeBytes = new Blob([content]).size
  const sizeMB = sizeBytes / (1024 * 1024)

  let tier: FileSizeTier
  if (sizeMB > 200) {
    tier = 'rejected'
  } else if (sizeMB > 50) {
    tier = 'chunked'
  } else if (sizeMB > 10) {
    tier = 'large'
  } else if (sizeMB > 1) {
    tier = 'reduced'
  } else {
    tier = 'normal'
  }

  return { ...STRATEGIES[tier], sizeMB }
}

export function getChunkedContent(
  content: string,
  visibleLineStart: number,
  visibleLineCount: number,
  bufferLines: number
): string {
  const lines = content.split('\n')
  const totalLines = lines.length
  const start = Math.max(0, visibleLineStart - bufferLines)
  const end = Math.min(totalLines, visibleLineStart + visibleLineCount + bufferLines)
  return lines.slice(start, end).join('\n')
}
