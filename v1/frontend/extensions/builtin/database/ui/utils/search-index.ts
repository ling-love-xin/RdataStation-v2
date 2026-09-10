/**
 * 搜索索引实现
 *
 * 使用倒排索引加速搜索，支持高亮和匹配度排序
 */

export interface SearchIndexEntry {
  nodeId: string
  nodeType: string
  connectionId: string
  labels: string[]
}

export interface SearchResult {
  nodeId: string
  score: number
  highlights: HighlightInfo[]
}

export interface HighlightInfo {
  label: string
  matchPositions: MatchPosition[]
}

export interface MatchPosition {
  start: number
  length: number
}

export class SearchIndex {
  private index = new Map<string, Set<string>>()
  private entries = new Map<string, SearchIndexEntry>()

  add(entry: SearchIndexEntry): void {
    this.entries.set(entry.nodeId, entry)

    for (const label of entry.labels) {
      const terms = this.tokenize(label.toLowerCase())
      for (const term of terms) {
        if (!this.index.has(term)) {
          this.index.set(term, new Set())
        }
        const nodeSet = this.index.get(term)
        if (nodeSet) {
          nodeSet.add(entry.nodeId)
        }
      }
    }
  }

  remove(nodeId: string): void {
    const entry = this.entries.get(nodeId)
    if (entry) {
      for (const label of entry.labels) {
        const terms = this.tokenize(label.toLowerCase())
        for (const term of terms) {
          const nodeIds = this.index.get(term)
          if (nodeIds) {
            nodeIds.delete(nodeId)
            if (nodeIds.size === 0) {
              this.index.delete(term)
            }
          }
        }
      }
      this.entries.delete(nodeId)
    }
  }

  search(query: string): SearchResult[] {
    if (!query.trim()) return []

    const queryTerms = this.tokenize(query.toLowerCase())
    if (queryTerms.length === 0) return []

    let candidateIds: Set<string> | null = null

    for (const term of queryTerms) {
      const nodeIds = this.matchTerm(term)
      if (!nodeIds || nodeIds.size === 0) return []

      if (candidateIds === null) {
        candidateIds = new Set(nodeIds)
      } else {
        const intersection = new Set<string>()
        for (const id of nodeIds) {
          if (candidateIds.has(id)) {
            intersection.add(id)
          }
        }
        candidateIds = intersection
      }

      if (candidateIds.size === 0) break
    }

    if (!candidateIds || candidateIds.size === 0) return []

    const results: SearchResult[] = []
    const lowerQuery = query.toLowerCase()

    for (const nodeId of candidateIds) {
      const entry = this.entries.get(nodeId)
      if (!entry) continue

      const highlights: HighlightInfo[] = []
      let score = 0

      for (const label of entry.labels) {
        const matchPositions = this.findMatchPositions(label, lowerQuery, queryTerms)
        if (matchPositions.length > 0) {
          highlights.push({
            label,
            matchPositions,
          })

          score += this.calculateScore(label, query, queryTerms)
        }
      }

      if (highlights.length > 0) {
        results.push({
          nodeId,
          score,
          highlights,
        })
      }
    }

    results.sort((a, b) => b.score - a.score)

    return results
  }

  searchSimple(query: string): string[] {
    const results = this.search(query)
    return results.map(r => r.nodeId)
  }

  getEntry(nodeId: string): SearchIndexEntry | undefined {
    return this.entries.get(nodeId)
  }

  clear(): void {
    this.index.clear()
    this.entries.clear()
  }

  size(): number {
    return this.entries.size
  }

  private matchTerm(term: string): Set<string> | null {
    const matches = new Set<string>()

    const exact = this.index.get(term)
    if (exact) {
      for (const id of exact) {
        matches.add(id)
      }
    }

    for (const [key, ids] of this.index) {
      if (key.startsWith(term)) {
        for (const id of ids) {
          matches.add(id)
        }
      }
    }

    return matches.size > 0 ? matches : null
  }

  private tokenize(text: string): string[] {
    const tokens = text.split(/[^a-zA-Z0-9_\u4e00-\u9fa5]+/).filter(token => token.length > 0)

    const result: string[] = []
    for (const token of tokens) {
      result.push(token)
      for (let i = 1; i < token.length; i++) {
        result.push(token.slice(i))
      }
    }
    return result
  }

  private findMatchPositions(
    label: string,
    lowerQuery: string,
    queryTerms: string[]
  ): MatchPosition[] {
    const lowerLabel = label.toLowerCase()
    const positions: MatchPosition[] = []
    const queryLength = lowerQuery.length

    if (lowerLabel.includes(lowerQuery)) {
      let start = 0
      while (start < lowerLabel.length) {
        const index = lowerLabel.indexOf(lowerQuery, start)
        if (index === -1) break
        positions.push({
          start: index,
          length: queryLength,
        })
        start = index + queryLength
      }
    } else {
      for (const term of queryTerms) {
        let start = 0
        while (start < lowerLabel.length) {
          const index = lowerLabel.indexOf(term, start)
          if (index === -1) break
          positions.push({
            start: index,
            length: term.length,
          })
          start = index + term.length
        }
      }
    }

    return positions.sort((a, b) => a.start - b.start)
  }

  private calculateScore(label: string, query: string, queryTerms: string[]): number {
    let score = 0
    const lowerLabel = label.toLowerCase()
    const lowerQuery = query.toLowerCase()

    if (lowerLabel === lowerQuery) {
      score += 100
    } else if (lowerLabel.startsWith(lowerQuery)) {
      score += 50
    } else if (lowerLabel.includes(lowerQuery)) {
      score += 30
    }

    const termMatches = queryTerms.filter(term => lowerLabel.includes(term)).length
    score += termMatches * 10

    return score
  }
}
