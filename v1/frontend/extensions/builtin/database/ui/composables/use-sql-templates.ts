import { ref } from 'vue'

export interface SqlTemplateVariable {
  name: string
  type: string
  description: string
  defaultValue?: string
}

export interface SqlTemplate {
  id: string
  name: string
  category: 'crud' | 'join' | 'aggregate' | 'ddl' | 'custom'
  sql: string
  description: string
  variables: SqlTemplateVariable[]
  tags: string[]
  isBuiltIn: boolean
}

const BUILTIN_TEMPLATES: SqlTemplate[] = [
  {
    id: 'select-all',
    name: '查询所有数据',
    category: 'crud',
    sql: 'SELECT * FROM {{table}} LIMIT {{limit}};',
    description: '查询表的所有数据，限制返回行数',
    variables: [
      { name: 'table', type: 'string', description: '表名' },
      { name: 'limit', type: 'number', description: '限制行数', defaultValue: '100' },
    ],
    tags: ['查询', '基础'],
    isBuiltIn: true,
  },
  {
    id: 'select-where',
    name: '条件查询',
    category: 'crud',
    sql: 'SELECT {{columns}} FROM {{table}} WHERE {{condition}};',
    description: '根据条件查询数据',
    variables: [
      { name: 'columns', type: 'string', description: '列名，逗号分隔', defaultValue: '*' },
      { name: 'table', type: 'string', description: '表名' },
      { name: 'condition', type: 'string', description: 'WHERE 条件' },
    ],
    tags: ['查询', '条件'],
    isBuiltIn: true,
  },
  {
    id: 'insert',
    name: '插入数据',
    category: 'crud',
    sql: 'INSERT INTO {{table}} ({{columns}}) VALUES ({{values}});',
    description: '向表中插入新记录',
    variables: [
      { name: 'table', type: 'string', description: '表名' },
      { name: 'columns', type: 'string', description: '列名，逗号分隔' },
      { name: 'values', type: 'string', description: '值，逗号分隔' },
    ],
    tags: ['插入', '基础'],
    isBuiltIn: true,
  },
  {
    id: 'update',
    name: '更新数据',
    category: 'crud',
    sql: 'UPDATE {{table}} SET {{assignments}} WHERE {{condition}};',
    description: '更新表中符合条件的记录',
    variables: [
      { name: 'table', type: 'string', description: '表名' },
      { name: 'assignments', type: 'string', description: '赋值语句，如 col1 = val1, col2 = val2' },
      { name: 'condition', type: 'string', description: 'WHERE 条件' },
    ],
    tags: ['更新', '基础'],
    isBuiltIn: true,
  },
  {
    id: 'delete',
    name: '删除数据',
    category: 'crud',
    sql: 'DELETE FROM {{table}} WHERE {{condition}};',
    description: '删除表中符合条件的记录',
    variables: [
      { name: 'table', type: 'string', description: '表名' },
      { name: 'condition', type: 'string', description: 'WHERE 条件' },
    ],
    tags: ['删除', '基础'],
    isBuiltIn: true,
  },
  {
    id: 'inner-join',
    name: '内连接查询',
    category: 'join',
    sql: 'SELECT {{columns}} FROM {{table1}} t1 INNER JOIN {{table2}} t2 ON t1.{{joinColumn1}} = t2.{{joinColumn2}};',
    description: '两个表的内连接查询',
    variables: [
      { name: 'columns', type: 'string', description: '列名', defaultValue: '*' },
      { name: 'table1', type: 'string', description: '左表名' },
      { name: 'table2', type: 'string', description: '右表名' },
      { name: 'joinColumn1', type: 'string', description: '左表连接列' },
      { name: 'joinColumn2', type: 'string', description: '右表连接列' },
    ],
    tags: ['连接', 'JOIN'],
    isBuiltIn: true,
  },
  {
    id: 'left-join',
    name: '左连接查询',
    category: 'join',
    sql: 'SELECT {{columns}} FROM {{table1}} t1 LEFT JOIN {{table2}} t2 ON t1.{{joinColumn1}} = t2.{{joinColumn2}};',
    description: '两个表的左连接查询',
    variables: [
      { name: 'columns', type: 'string', description: '列名', defaultValue: '*' },
      { name: 'table1', type: 'string', description: '左表名' },
      { name: 'table2', type: 'string', description: '右表名' },
      { name: 'joinColumn1', type: 'string', description: '左表连接列' },
      { name: 'joinColumn2', type: 'string', description: '右表连接列' },
    ],
    tags: ['连接', 'LEFT JOIN'],
    isBuiltIn: true,
  },
  {
    id: 'count',
    name: '统计行数',
    category: 'aggregate',
    sql: 'SELECT COUNT(*) FROM {{table}} WHERE {{condition}};',
    description: '统计符合条件的行数',
    variables: [
      { name: 'table', type: 'string', description: '表名' },
      { name: 'condition', type: 'string', description: 'WHERE 条件', defaultValue: '1=1' },
    ],
    tags: ['聚合', 'COUNT'],
    isBuiltIn: true,
  },
  {
    id: 'group-by',
    name: '分组统计',
    category: 'aggregate',
    sql: 'SELECT {{groupByColumns}}, {{aggregateFunctions}} FROM {{table}} GROUP BY {{groupByColumns}};',
    description: '按列分组并计算聚合值',
    variables: [
      { name: 'groupByColumns', type: 'string', description: '分组列' },
      {
        name: 'aggregateFunctions',
        type: 'string',
        description: '聚合函数，如 COUNT(*), SUM(col)',
      },
      { name: 'table', type: 'string', description: '表名' },
    ],
    tags: ['聚合', 'GROUP BY'],
    isBuiltIn: true,
  },
  {
    id: 'create-table',
    name: '创建表',
    category: 'ddl',
    sql: `CREATE TABLE {{table}} (
  id SERIAL PRIMARY KEY,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);`,
    description: '创建新表',
    variables: [{ name: 'table', type: 'string', description: '表名' }],
    tags: ['DDL', 'CREATE'],
    isBuiltIn: true,
  },
]

export function useSqlTemplates() {
  const templates = ref<SqlTemplate[]>([...BUILTIN_TEMPLATES])

  function getTemplatesByCategory(category: SqlTemplate['category']): SqlTemplate[] {
    return templates.value.filter(t => t.category === category)
  }

  function getTemplateById(id: string): SqlTemplate | undefined {
    return templates.value.find(t => t.id === id)
  }

  function searchTemplates(query: string): SqlTemplate[] {
    const lowerQuery = query.toLowerCase()
    return templates.value.filter(
      t =>
        t.name.toLowerCase().includes(lowerQuery) ||
        t.description.toLowerCase().includes(lowerQuery) ||
        t.tags.some(tag => tag.toLowerCase().includes(lowerQuery))
    )
  }

  function fillTemplate(template: SqlTemplate, values: Record<string, string>): string {
    let sql = template.sql
    for (const [key, value] of Object.entries(values)) {
      sql = sql.replace(new RegExp(`{{${key}}}`, 'g'), value)
    }
    return sql
  }

  function addCustomTemplate(template: Omit<SqlTemplate, 'isBuiltIn'>) {
    templates.value.push({ ...template, isBuiltIn: false })
  }

  function removeCustomTemplate(id: string) {
    templates.value = templates.value.filter(t => !(t.id === id && !t.isBuiltIn))
  }

  return {
    templates,
    getTemplatesByCategory,
    getTemplateById,
    searchTemplates,
    fillTemplate,
    addCustomTemplate,
    removeCustomTemplate,
  }
}
