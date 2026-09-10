import type { PluginManifest } from '@/extensions/core/types'

export type PluginSource = 'builtin' | 'user' | 'project'

export interface DiscoveredPlugin {
  id: string
  path: string
  source: PluginSource
  manifest: PluginManifest
  status: 'discovered' | 'loaded' | 'active' | 'error' | 'inactive'
  errorMessage?: string
}

export interface ValidationResult {
  valid: boolean
  errors: string[]
}

export class PluginLoader {
  private discovered = new Map<string, DiscoveredPlugin>()

  async discoverPlugins(
    builtinExtensions: Array<{ id: string; manifest: PluginManifest }>,
    projectPath?: string
  ): Promise<DiscoveredPlugin[]> {
    const plugins: DiscoveredPlugin[] = []

    for (const ext of builtinExtensions) {
      plugins.push({
        id: ext.id,
        path: `builtin://${ext.id}`,
        source: 'builtin',
        manifest: ext.manifest,
        status: 'discovered',
      })
    }

    if (projectPath) {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const projectPlugins: string[] = await invoke('list_project_plugins', { projectPath })
        void projectPlugins
      } catch {
        // 项目没有插件目录，跳过
      }
    }

    const deduped = new Map<string, DiscoveredPlugin>()
    for (const p of plugins) {
      const existing = deduped.get(p.id)
      if (!existing || this.priority(p.source) > this.priority(existing.source)) {
        deduped.set(p.id, p)
      }
    }

    this.discovered = deduped
    return Array.from(deduped.values())
  }

  validateManifest(manifest: PluginManifest): ValidationResult {
    const errors: string[] = []

    if (!manifest.plugin.id) errors.push('Missing plugin.id')
    if (!manifest.plugin.name) errors.push('Missing plugin.name')
    if (!manifest.plugin.version) errors.push('Missing plugin.version')

    const hasFrontend = manifest.capabilities?.frontend !== undefined
    const hasWasm = manifest.capabilities?.wasm !== undefined
    if (!hasFrontend && !hasWasm) {
      errors.push('Must have at least one capability (frontend or wasm)')
    }

    return { valid: errors.length === 0, errors }
  }

  resolveDependencies(plugins: DiscoveredPlugin[]): DiscoveredPlugin[] {
    const pluginMap = new Map(plugins.map(p => [p.id, p]))

    for (const plugin of plugins) {
      const deps = plugin.manifest.dependencies ?? []
      for (const dep of deps) {
        if (!pluginMap.has(dep.id)) {
          plugin.status = 'error'
          plugin.errorMessage = `Dependency missing: ${dep.id}`
        }
      }
    }

    const visited = new Set<string>()
    const sorted: DiscoveredPlugin[] = []

    const visit = (id: string, path: Set<string> = new Set()) => {
      if (visited.has(id)) return
      if (path.has(id)) {
        const plugin = pluginMap.get(id)
        if (plugin) {
          plugin.status = 'error'
          plugin.errorMessage = 'Circular dependency detected'
        }
        return
      }

      path.add(id)
      const plugin = pluginMap.get(id)
      if (plugin) {
        const deps = plugin.manifest.dependencies ?? []
        for (const dep of deps) {
          if (pluginMap.has(dep.id)) {
            visit(dep.id, new Set(path))
          }
        }
        visited.add(id)
        sorted.push(plugin)
      }
    }

    for (const plugin of plugins) {
      visit(plugin.id)
    }

    return sorted
  }

  private priority(source: PluginSource): number {
    switch (source) {
      case 'project':
        return 3
      case 'user':
        return 2
      case 'builtin':
        return 1
    }
  }
}

export const pluginLoader = new PluginLoader()
