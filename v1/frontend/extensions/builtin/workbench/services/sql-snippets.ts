/**
 * SQL 模板/片段库
 * 提供可配置的 SQL 模板系统，支持用户自定义模板
 */

export interface SqlSnippet {
  id: string
  label: string
  detail: string
  insertText: string
  category: string
  isCustom?: boolean
}

// 内置 SQL 模板
const builtInSnippets: SqlSnippet[] = [
  // 查询类
  {
    id: 'select-all',
    label: 'select-all',
    detail: 'SELECT * FROM',
    insertText: 'SELECT * FROM ${1:table_name}',
    category: '查询',
  },
  {
    id: 'select-where',
    label: 'select-where',
    detail: 'SELECT with WHERE',
    insertText: 'SELECT ${1:*} FROM ${2:table_name} WHERE ${3:condition}',
    category: '查询',
  },
  {
    id: 'select-distinct',
    label: 'select-distinct',
    detail: 'SELECT DISTINCT',
    insertText: 'SELECT DISTINCT ${1:column} FROM ${2:table_name}',
    category: '查询',
  },
  {
    id: 'select-order-by',
    label: 'select-order-by',
    detail: 'SELECT with ORDER BY',
    insertText: 'SELECT ${1:*} FROM ${2:table_name} ORDER BY ${3:column} ${4:ASC|DESC}',
    category: '查询',
  },
  {
    id: 'select-group-by',
    label: 'select-group-by',
    detail: 'SELECT with GROUP BY',
    insertText: 'SELECT ${1:column}, COUNT(*) FROM ${2:table_name} GROUP BY ${1:column}',
    category: '查询',
  },
  {
    id: 'select-limit',
    label: 'select-limit',
    detail: 'SELECT with LIMIT',
    insertText: 'SELECT * FROM ${1:table_name} LIMIT ${2:10}',
    category: '查询',
  },

  // 插入类
  {
    id: 'insert-into',
    label: 'insert-into',
    detail: 'INSERT INTO',
    insertText: 'INSERT INTO ${1:table_name} (${2:columns}) VALUES (${3:values})',
    category: '插入',
  },
  {
    id: 'insert-multiple',
    label: 'insert-multiple',
    detail: 'INSERT multiple rows',
    insertText:
      'INSERT INTO ${1:table_name} (${2:columns})\nVALUES\n  (${3:values1}),\n  (${4:values2})',
    category: '插入',
  },
  {
    id: 'insert-select',
    label: 'insert-select',
    detail: 'INSERT from SELECT',
    insertText:
      'INSERT INTO ${1:table_name} (${2:columns})\nSELECT ${3:columns} FROM ${4:source_table}',
    category: '插入',
  },

  // 更新类
  {
    id: 'update-set',
    label: 'update-set',
    detail: 'UPDATE SET',
    insertText: 'UPDATE ${1:table_name} SET ${2:column} = ${3:value} WHERE ${4:condition}',
    category: '更新',
  },
  {
    id: 'update-multiple',
    label: 'update-multiple',
    detail: 'UPDATE multiple columns',
    insertText:
      'UPDATE ${1:table_name}\nSET ${2:column1} = ${3:value1},\n    ${4:column2} = ${5:value2}\nWHERE ${6:condition}',
    category: '更新',
  },

  // 删除类
  {
    id: 'delete-from',
    label: 'delete-from',
    detail: 'DELETE FROM',
    insertText: 'DELETE FROM ${1:table_name} WHERE ${2:condition}',
    category: '删除',
  },
  {
    id: 'delete-all',
    label: 'delete-all',
    detail: 'DELETE all rows (TRUNCATE)',
    insertText: 'TRUNCATE TABLE ${1:table_name}',
    category: '删除',
  },

  // 创建类
  {
    id: 'create-table',
    label: 'create-table',
    detail: 'CREATE TABLE',
    insertText:
      'CREATE TABLE ${1:table_name} (\n  ${2:id} ${3:INT} ${4:PRIMARY KEY},\n  ${5:name} ${6:VARCHAR(255)} NOT NULL,\n  ${7:created_at} ${8:TIMESTAMP} DEFAULT CURRENT_TIMESTAMP\n)',
    category: '创建',
  },
  {
    id: 'create-view',
    label: 'create-view',
    detail: 'CREATE VIEW',
    insertText: 'CREATE VIEW ${1:view_name} AS\nSELECT ${2:columns} FROM ${3:table_name}',
    category: '创建',
  },
  {
    id: 'create-index',
    label: 'create-index',
    detail: 'CREATE INDEX',
    insertText: 'CREATE INDEX ${1:index_name} ON ${2:table_name} (${3:column})',
    category: '创建',
  },

  // 连接类
  {
    id: 'inner-join',
    label: 'inner-join',
    detail: 'INNER JOIN',
    insertText: 'INNER JOIN ${1:table_name} ON ${2:condition}',
    category: '连接',
  },
  {
    id: 'left-join',
    label: 'left-join',
    detail: 'LEFT JOIN',
    insertText: 'LEFT JOIN ${1:table_name} ON ${2:condition}',
    category: '连接',
  },
  {
    id: 'right-join',
    label: 'right-join',
    detail: 'RIGHT JOIN',
    insertText: 'RIGHT JOIN ${1:table_name} ON ${2:condition}',
    category: '连接',
  },
  {
    id: 'full-join',
    label: 'full-join',
    detail: 'FULL OUTER JOIN',
    insertText: 'FULL OUTER JOIN ${1:table_name} ON ${2:condition}',
    category: '连接',
  },
  {
    id: 'cross-join',
    label: 'cross-join',
    detail: 'CROSS JOIN',
    insertText: 'CROSS JOIN ${1:table_name}',
    category: '连接',
  },

  // 聚合函数
  {
    id: 'count',
    label: 'count',
    detail: 'COUNT(*)',
    insertText: 'COUNT(*)',
    category: '聚合',
  },
  {
    id: 'sum',
    label: 'sum',
    detail: 'SUM(column)',
    insertText: 'SUM(${1:column})',
    category: '聚合',
  },
  {
    id: 'avg',
    label: 'avg',
    detail: 'AVG(column)',
    insertText: 'AVG(${1:column})',
    category: '聚合',
  },
  {
    id: 'max',
    label: 'max',
    detail: 'MAX(column)',
    insertText: 'MAX(${1:column})',
    category: '聚合',
  },
  {
    id: 'min',
    label: 'min',
    detail: 'MIN(column)',
    insertText: 'MIN(${1:column})',
    category: '聚合',
  },

  // 事务
  {
    id: 'transaction',
    label: 'transaction',
    detail: 'BEGIN TRANSACTION',
    insertText: 'BEGIN TRANSACTION;\n\n${1:-- SQL statements}\n\nCOMMIT;',
    category: '事务',
  },
  {
    id: 'rollback',
    label: 'rollback',
    detail: 'ROLLBACK',
    insertText: 'ROLLBACK;',
    category: '事务',
  },

  // 窗口函数
  {
    id: 'row-number',
    label: 'row-number',
    detail: 'ROW_NUMBER() OVER',
    insertText: 'ROW_NUMBER() OVER (PARTITION BY ${1:column} ORDER BY ${2:column})',
    category: '窗口函数',
  },
  {
    id: 'rank',
    label: 'rank',
    detail: 'RANK() OVER',
    insertText: 'RANK() OVER (PARTITION BY ${1:column} ORDER BY ${2:column})',
    category: '窗口函数',
  },
  {
    id: 'dense-rank',
    label: 'dense-rank',
    detail: 'DENSE_RANK() OVER',
    insertText: 'DENSE_RANK() OVER (PARTITION BY ${1:column} ORDER BY ${2:column})',
    category: '窗口函数',
  },
  {
    id: 'lag',
    label: 'lag',
    detail: 'LAG() OVER',
    insertText: 'LAG(${1:column}, ${2:1}) OVER (PARTITION BY ${3:column} ORDER BY ${4:column})',
    category: '窗口函数',
  },
  {
    id: 'lead',
    label: 'lead',
    detail: 'LEAD() OVER',
    insertText: 'LEAD(${1:column}, ${2:1}) OVER (PARTITION BY ${3:column} ORDER BY ${4:column})',
    category: '窗口函数',
  },

  // CTE (Common Table Expression)
  {
    id: 'with-cte',
    label: 'with-cte',
    detail: 'WITH (CTE)',
    insertText:
      'WITH ${1:cte_name} AS (\n  SELECT ${2:columns} FROM ${3:table_name}\n)\nSELECT * FROM ${1:cte_name}',
    category: 'CTE',
  },
  {
    id: 'recursive-cte',
    label: 'recursive-cte',
    detail: 'WITH RECURSIVE',
    insertText:
      'WITH RECURSIVE ${1:cte_name} AS (\n  -- Anchor query\n  SELECT ${2:columns} FROM ${3:table_name} WHERE ${4:condition}\n  \n  UNION ALL\n  \n  -- Recursive query\n  SELECT ${5:columns} FROM ${3:table_name}\n  JOIN ${1:cte_name} ON ${6:join_condition}\n)\nSELECT * FROM ${1:cte_name}',
    category: 'CTE',
  },

  // 子查询
  {
    id: 'subquery-in',
    label: 'subquery-in',
    detail: 'IN (subquery)',
    insertText: 'IN (SELECT ${1:column} FROM ${2:table_name} WHERE ${3:condition})',
    category: '子查询',
  },
  {
    id: 'subquery-exists',
    label: 'subquery-exists',
    detail: 'EXISTS (subquery)',
    insertText: 'EXISTS (SELECT 1 FROM ${1:table_name} WHERE ${2:condition})',
    category: '子查询',
  },

  // 条件表达式
  {
    id: 'case-when',
    label: 'case-when',
    detail: 'CASE WHEN',
    insertText: 'CASE\n  WHEN ${1:condition} THEN ${2:result}\n  ELSE ${3:default_result}\nEND',
    category: '条件',
  },
  {
    id: 'coalesce',
    label: 'coalesce',
    detail: 'COALESCE',
    insertText: 'COALESCE(${1:column}, ${2:default_value})',
    category: '条件',
  },
  {
    id: 'nullif',
    label: 'nullif',
    detail: 'NULLIF',
    insertText: 'NULLIF(${1:column}, ${2:value})',
    category: '条件',
  },
]

// 用户自定义模板存储
const STORAGE_KEY = 'sql-snippets-custom'

/**
 * 获取所有 SQL 模板（内置 + 自定义）
 */
export function getAllSnippets(): SqlSnippet[] {
  const customSnippets = getCustomSnippets()
  return [...builtInSnippets, ...customSnippets]
}

/**
 * 获取内置模板
 */
export function getBuiltInSnippets(): SqlSnippet[] {
  return [...builtInSnippets]
}

/**
 * 获取用户自定义模板
 */
export function getCustomSnippets(): SqlSnippet[] {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored) {
      return JSON.parse(stored)
    }
  } catch (error) {
    console.error('Failed to load custom snippets:', error)
  }
  return []
}

/**
 * 添加自定义模板
 */
export function addCustomSnippet(snippet: Omit<SqlSnippet, 'id' | 'isCustom'>): SqlSnippet {
  const customSnippets = getCustomSnippets()

  const newSnippet: SqlSnippet = {
    ...snippet,
    id: `custom-${Date.now()}`,
    isCustom: true,
  }

  customSnippets.push(newSnippet)
  saveCustomSnippets(customSnippets)

  return newSnippet
}

/**
 * 删除自定义模板
 */
export function deleteCustomSnippet(id: string): boolean {
  const customSnippets = getCustomSnippets()
  const filtered = customSnippets.filter(s => s.id !== id)

  if (filtered.length === customSnippets.length) {
    return false
  }

  saveCustomSnippets(filtered)
  return true
}

/**
 * 更新自定义模板
 */
export function updateCustomSnippet(id: string, updates: Partial<SqlSnippet>): boolean {
  const customSnippets = getCustomSnippets()
  const index = customSnippets.findIndex(s => s.id === id)

  if (index === -1) {
    return false
  }

  customSnippets[index] = { ...customSnippets[index], ...updates }
  saveCustomSnippets(customSnippets)
  return true
}

/**
 * 保存自定义模板到 localStorage
 */
function saveCustomSnippets(snippets: SqlSnippet[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(snippets))
  } catch (error) {
    console.error('Failed to save custom snippets:', error)
  }
}

/**
 * 按分类获取模板
 */
export function getSnippetsByCategory(category: string): SqlSnippet[] {
  return getAllSnippets().filter(s => s.category === category)
}

/**
 * 获取所有分类
 */
export function getCategories(): string[] {
  const categories = new Set(getAllSnippets().map(s => s.category))
  return Array.from(categories)
}

/**
 * 搜索模板
 */
export function searchSnippets(query: string): SqlSnippet[] {
  const lowerQuery = query.toLowerCase()
  return getAllSnippets().filter(
    s =>
      s.label.toLowerCase().includes(lowerQuery) ||
      s.detail.toLowerCase().includes(lowerQuery) ||
      s.insertText.toLowerCase().includes(lowerQuery)
  )
}

/**
 * 导出自定义模板
 */
export function exportCustomSnippets(): string {
  const customSnippets = getCustomSnippets()
  return JSON.stringify(customSnippets, null, 2)
}

/**
 * 导入自定义模板
 */
export function importCustomSnippets(json: string): boolean {
  try {
    const snippets = JSON.parse(json)
    if (!Array.isArray(snippets)) {
      return false
    }

    const existingSnippets = getCustomSnippets()
    const mergedSnippets = [
      ...existingSnippets,
      ...snippets.map(s => ({
        ...s,
        isCustom: true,
      })),
    ]

    saveCustomSnippets(mergedSnippets)
    return true
  } catch (error) {
    console.error('Failed to import snippets:', error)
    return false
  }
}

/**
 * 重置自定义模板
 */
export function resetCustomSnippets(): void {
  localStorage.removeItem(STORAGE_KEY)
}
