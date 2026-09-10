/**
 * 前端src目录重构脚本
 * 按照REFACTOR_PLAN.md执行文件移动
 */

const fs = require('fs')
const path = require('path')

const SRC_DIR = path.join(__dirname, '..', 'src')

// 文件移动映射
const fileMappings = [
  // 1. 入口文件 → app/
  { from: 'App.vue', to: 'app/App.vue' },
  { from: 'main.ts', to: 'app/main.ts' },
  { from: 'router/index.ts', to: 'app/router.ts' },

  // 2. connection模块
  { from: 'components/connection', to: 'modules/connection/components' },
  { from: 'services/connection.ts', to: 'modules/connection/services/connection.ts' },
  { from: 'services/projectConnection.ts', to: 'modules/connection/services/projectConnection.ts' },
  { from: 'stores/connection.ts', to: 'modules/connection/stores/connection.ts' },
  { from: 'stores/runtimeConnection.ts', to: 'modules/connection/stores/runtimeConnection.ts' },
  { from: 'stores/projectConnection.ts', to: 'modules/connection/stores/projectConnection.ts' },
  { from: 'types/connection.ts', to: 'modules/connection/types/connection.ts' },
  { from: 'types/driver.ts', to: 'modules/connection/types/driver.ts' },

  // 3. database模块
  { from: 'components/database-nav', to: 'modules/database/components/navigator' },
  { from: 'services/databaseNavigator.ts', to: 'modules/database/services/databaseNavigator.ts' },
  {
    from: 'services/databaseNavigatorLoader.ts',
    to: 'modules/database/services/databaseNavigatorLoader.ts',
  },
  {
    from: 'services/mockDatabaseNavigator.ts',
    to: 'modules/database/services/mockDatabaseNavigator.ts',
  },
  { from: 'stores/databaseNavigator.ts', to: 'modules/database/stores/databaseNavigator.ts' },
  { from: 'stores/databaseNavigatorNew.ts', to: 'modules/database/stores/databaseNavigatorNew.ts' },
  { from: 'types/databaseNavigator.ts', to: 'modules/database/types/databaseNavigator.ts' },

  // 4. project模块
  { from: 'services/projectConnection.ts', to: 'modules/project/services/projectConnection.ts' },
  { from: 'stores/project.ts', to: 'modules/project/stores/project.ts' },
  { from: 'types/project.ts', to: 'modules/project/types/project.ts' },

  // 5. query模块
  { from: 'components/sql-editor', to: 'modules/query/components/editor' },
  { from: 'components/result', to: 'modules/query/components/result' },
  { from: 'services/query.ts', to: 'modules/query/services/query.ts' },
  { from: 'stores/query.ts', to: 'modules/query/stores/query.ts' },
  { from: 'types/query.ts', to: 'modules/query/types/query.ts' },

  // 6. workbench模块
  { from: 'components/workbench', to: 'modules/workbench/components' },
  { from: 'components/layout', to: 'modules/workbench/components/layout' },
  { from: 'stores/workbench.ts', to: 'modules/workbench/stores/workbench.ts' },
  { from: 'views/workbench', to: 'modules/workbench/views' },

  // 7. 共享层
  { from: 'composables', to: 'shared/composables' },
  { from: 'constants', to: 'shared/constants' },
  { from: 'styles', to: 'shared/styles' },
  { from: 'utils', to: 'shared/utils' },
  { from: 'config', to: 'shared/config' },
  { from: 'types/databaseMeta.ts', to: 'shared/types/databaseMeta.ts' },
  { from: 'types/index.ts', to: 'shared/types/index.ts' },
  { from: 'stores/ui.ts', to: 'shared/stores/ui.ts' },

  // 8. 通用组件
  { from: 'components/common', to: 'shared/components/common' },
]

// 创建目录
function ensureDir(dir) {
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true })
    console.log(`Created directory: ${dir}`)
  }
}

// 移动文件或目录
function moveItem(from, to) {
  const fromPath = path.join(SRC_DIR, from)
  const toPath = path.join(SRC_DIR, to)

  if (!fs.existsSync(fromPath)) {
    console.log(`SKIP: ${from} does not exist`)
    return
  }

  ensureDir(path.dirname(toPath))

  try {
    fs.renameSync(fromPath, toPath)
    console.log(`MOVED: ${from} -> ${to}`)
  } catch (err) {
    console.error(`ERROR moving ${from}: ${err.message}`)
  }
}

// 执行移动
console.log('Starting src refactor...\n')

fileMappings.forEach(mapping => {
  moveItem(mapping.from, mapping.to)
})

console.log('\nRefactor complete!')
console.log('\nNext steps:')
console.log('1. Update import paths in moved files')
console.log('2. Run "pnpm run typecheck" to verify')
console.log('3. Run "pnpm run dev" to test')
