/**
 * 修复剩余的导入路径问题
 */

const fs = require('fs')
const path = require('path')

const SRC_DIR = path.join(__dirname, '..', 'src')

// 额外的导入路径修复
const additionalMappings = [
  // @/types 需要指向 shared/types/index.ts
  { from: '@/types', to: '@/shared/types' },

  // 相对路径导入修复
  { from: '../../stores/ui', to: '@/shared/stores/ui' },
  { from: '../../stores/connection', to: '@/modules/connection/stores/connection' },
  { from: '../../stores/project', to: '@/modules/project/stores/project' },
  { from: '../stores/ui', to: '@/shared/stores/ui' },
  { from: '../stores/connection', to: '@/modules/connection/stores/connection' },
  { from: './project', to: '@/modules/project/stores/project' },
  { from: './connection', to: '@/modules/connection/services/connection' },
  { from: './query', to: '@/modules/query/services/query' },

  // shared/types 内部的相对导入
  { from: "'./connection'", to: "'@/modules/connection/types/connection'" },
  { from: "'./query'", to: "'@/modules/query/types/query'" },
  { from: "'./project'", to: "'@/modules/project/types/project'" },
]

// 递归获取所有 .ts 和 .vue 文件
function getFiles(dir, files = []) {
  const items = fs.readdirSync(dir)

  for (const item of items) {
    const fullPath = path.join(dir, item)
    const stat = fs.statSync(fullPath)

    if (stat.isDirectory()) {
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

  additionalMappings.forEach(mapping => {
    const regex = new RegExp(mapping.from.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g')
    if (regex.test(content)) {
      content = content.replace(regex, mapping.to)
      modified = true
    }
  })

  if (modified) {
    fs.writeFileSync(filePath, content, 'utf-8')
    console.log(`Updated: ${path.relative(SRC_DIR, filePath)}`)
  }
}

// 主函数
console.log('Fixing remaining import paths...\n')

const files = getFiles(SRC_DIR)

files.forEach(file => {
  try {
    updateImports(file)
  } catch (err) {
    console.error(`Error processing ${file}: ${err.message}`)
  }
})

console.log('\nImport path fix complete!')
