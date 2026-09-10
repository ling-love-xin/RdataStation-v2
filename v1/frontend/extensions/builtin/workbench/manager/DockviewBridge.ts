const DEFAULT_POPOUT_GEOMETRY = { x: 200, y: 200, width: 800, height: 400 }

interface DockviewApi {
  addPanel(opts: Record<string, unknown>): void
  getPanel(id: string):
    | {
        api: { close(): void; setTitle(t: string): void; setVisible(v: boolean): void }
        focus(): void
        id: string
        group?: { id: string }
      }
    | undefined
  getGroup(id: string):
    | {
        api: { close(): void; setVisible(v: boolean): void; moveTo(p: { group: string }): void }
        id: string
        panels: Array<{ id: string }>
      }
    | undefined
  movePanelOrGroup(panelId: string, opts: Record<string, unknown>): void
}

export class DockviewBridge {
  private api: DockviewApi | null = null

  init(api: DockviewApi): void {
    this.api = api
  }

  addPanel(opts: Record<string, unknown>): void {
    this.api?.addPanel(opts)
  }

  getPanel(
    id: string
  ):
    | { api: { close(): void; setTitle(t: string): void; setVisible(v: boolean): void } }
    | undefined {
    try {
      return this.api?.getPanel(id)
    } catch {
      console.warn('[DockviewBridge] getPanel failed for', id)
      return undefined
    }
  }

  getGroup(id: string): { id: string; panels: Array<{ id: string }> } | undefined {
    try {
      return this.api?.getGroup(id)
    } catch {
      console.warn('[DockviewBridge] getGroup failed for', id)
      return undefined
    }
  }

  movePanelOrGroup(panelId: string, opts: Record<string, unknown>): void {
    try {
      this.api?.movePanelOrGroup(panelId, opts)
    } catch {
      console.warn('[DockviewBridge] movePanelOrGroup failed for', panelId)
    }
  }

  detachResultPanel(panelId: string): void {
    try {
      this.api?.movePanelOrGroup(panelId, {
        group: `detached_${panelId}`,
        position: { direction: 'right' },
        floating: DEFAULT_POPOUT_GEOMETRY,
      })
    } catch {
      console.warn('[DockviewBridge] detachResultPanel failed for', panelId)
    }
  }

  closePanel(panelId: string): void {
    try {
      this.api?.getPanel(panelId)?.api.close()
    } catch {
      console.warn('[DockviewBridge] closePanel failed for', panelId)
    }
  }

  setPanelTitle(panelId: string, title: string): void {
    try {
      this.api?.getPanel(panelId)?.api.setTitle(title)
    } catch {
      console.warn('[DockviewBridge] setPanelTitle failed for', panelId)
    }
  }

  setPanelVisible(panelId: string, visible: boolean): void {
    try {
      this.api?.getPanel(panelId)?.api.setVisible(visible)
    } catch {
      console.warn('[DockviewBridge] setPanelVisible failed for', panelId)
    }
  }
}
