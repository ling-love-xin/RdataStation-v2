/**
 * 更新导入路径脚本
 * 将旧的导入路径更新为新的模块路径
 */

const fs = require('fs')
const path = require('path')

const SRC_DIR = path.join(__dirname, '..', 'src')

// 导入路径映射
const importMappings = [
  // stores
  { from: '@/stores/project', to: '@/modules/project/stores/project' },
  { from: '@/stores/projectConnection', to: '@/modules/connection/stores/projectConnection' },
  { from: '@/stores/connection', to: '@/modules/connection/stores/connection' },
  { from: '@/stores/runtimeConnection', to: '@/modules/connection/stores/runtimeConnection' },
  { from: '@/stores/query', to: '@/modules/query/stores/query' },
  { from: '@/stores/workbench', to: '@/modules/workbench/stores/workbench' },
  { from: '@/stores/databaseNavigator', to: '@/modules/database/stores/databaseNavigator' },
  { from: '@/stores/databaseNavigatorNew', to: '@/modules/database/stores/databaseNavigatorNew' },
  { from: '@/stores/ui', to: '@/shared/stores/ui' },

  // types
  { from: '@/types/connection', to: '@/modules/connection/types/connection' },
  { from: '@/types/driver', to: '@/modules/connection/types/driver' },
  { from: '@/types/project', to: '@/modules/project/types/project' },
  { from: '@/types/query', to: '@/modules/query/types/query' },
  { from: '@/types/databaseNavigator', to: '@/modules/database/types/databaseNavigator' },
  { from: '@/types/databaseMeta', to: '@/shared/types/databaseMeta' },

  // services
  { from: '@/services/connection', to: '@/modules/connection/services/connection' },
  { from: '@/services/projectConnection', to: '@/modules/connection/services/projectConnection' },
  { from: '@/services/query', to: '@/modules/query/services/query' },
  { from: '@/services/databaseNavigator', to: '@/modules/database/services/databaseNavigator' },
  {
    from: '@/services/databaseNavigatorLoader',
    to: '@/modules/database/services/databaseNavigatorLoader',
  },
  {
    from: '@/services/mockDatabaseNavigator',
    to: '@/modules/database/services/mockDatabaseNavigator',
  },

  // components - connection
  { from: '@/components/connection/', to: '@/components/connection/' }, // 保持原样，因为还在原位置

  // config
  { from: '@/config/', to: '@/shared/config/' },

  // composables
  { from: '@/composables/', to: '@/shared/composables/' },

  // constants
  { from: '@/constants/', to: '@/shared/constants/' },

  // utils
  { from: '@/utils/', to: '@/shared/utils/' },

  // styles
  { from: '@/styles/', to: '@/shared/styles/' },
]

// 递归获取所有 .ts 和 .vue 文件
function getFiles(dir, files = []) {
  const items = fs.readdirSync(dir)

  for (const item of items) {
    const fullPath = path.join(dir, item)
    const stat = fs.statSync(fullPath)

    if (stat.isDirectory()) {
      // 跳过 node_modules 和 src-tauri
      if (item !== 'node_modules' && item !== 'src-tauri') {
        getFiles(fullPath, files)
      }
    } else if (item.endsWith('.ts') || item.endsWith('.vue')) {
      files.push(fullPath)
    }
  }

  return files
}

// 更新文件中的导入路径
function updateImports(filePath) {
  let content = fs.readFileSync(filePath, 'utf-8')
  let modified = false

  importMappings.forEach(mapping => {
    // 处理 import { x } from 'path'
    const importRegex = new RegExp(`from ['"]${mapping.from}['"]`, 'g')
    if (importRegex.test(content)) {
      content = content.replace(importRegex, `from '${mapping.to}'`)
      modified = true
    }

    // 处理 import type { x } from 'path'
    const importTypeRegex = new RegExp(`from ['"]${mapping.from}['"]`, 'g')
    if (importTypeRegex.test(content)) {
      content = content.replace(importTypeRegex, `from '${mapping.to}'`)
      modified = true
    }

    // 处理 import('path')
    const dynamicImportRegex = new RegExp(`import\\(['"]${mapping.from}['"]\\)`, 'g')
    if (dynamicImportRegex.test(content)) {
      content = content.replace(dynamicImportRegex, `import('${mapping.to}')`)
      modified = true
    }
  })

  if (modified) {
    fs.writeFileSync(filePath, content, 'utf-8')
    console.log(`Updated: ${path.relative(SRC_DIR, filePath)}`)
  }
}

// 主函数
console.log('Updating import paths...\n')

const files = getFiles(SRC_DIR)
console.log(`Found ${files.length} files to process\n`)

files.forEach(file => {
  try {
    updateImports(file)
  } catch (err) {
    console.error(`Error processing ${file}: ${err.message}`)
  }
})

console.log('\nImport path update complete!')
