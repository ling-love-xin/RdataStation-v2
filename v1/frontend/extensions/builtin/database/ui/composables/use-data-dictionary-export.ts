import type { SchemaInfo, TableInfo } from '../types/navigator'

export interface DataDictionaryOptions {
  format: 'markdown' | 'html' | 'json'
  includeTables: boolean
  includeViews: boolean
  includeIndexes: boolean
  includeConstraints: boolean
  schemaName?: string
}

export function useDataDictionaryExport() {
  function generateMarkdown(schema: SchemaInfo, options: DataDictionaryOptions): string {
    let md = `# 数据字典 - ${schema.name}\n\n`
    md += `生成时间：${new Date().toLocaleString()}\n\n`

    if (options.includeTables && schema.tables.length > 0) {
      md += `## 表 (${schema.tables.length})\n\n`

      for (const table of schema.tables) {
        md += generateTableMarkdown(table, options)
      }
    }

    if (options.includeViews && schema.views.length > 0) {
      md += `## 视图 (${schema.views.length})\n\n`

      for (const view of schema.views) {
        md += `### ${view.name}\n\n`
        if (view.description) {
          md += `${view.description}\n\n`
        }
        md += `\`\`\`sql\n${view.definition || 'N/A'}\n\`\`\`\n\n`
      }
    }

    if (options.includeIndexes && schema.indexes.length > 0) {
      md += `## 索引 (${schema.indexes.length})\n\n`
      md += '| 索引名 | 列 | 唯一 | 类型 |\n'
      md += '|--------|-----|------|------|\n'

      for (const index of schema.indexes) {
        md += `| ${index.name} | ${index.columnNames.join(', ')} | ${index.isUnique ? '是' : '否'} | ${index.type || ''} |\n`
      }

      md += '\n'
    }

    return md
  }

  function generateTableMarkdown(table: TableInfo, _options: DataDictionaryOptions): string {
    let md = `### ${table.name}\n\n`

    if (table.description) {
      md += `${table.description}\n\n`
    }

    if (table.columns && table.columns.length > 0) {
      md += '| 列名 | 类型 | 可空 | 默认值 | 主键 |\n'
      md += '|------|------|------|--------|------|\n'

      for (const col of table.columns) {
        md += `| ${col.name} | ${col.dataType} | ${col.isNullable ? '是' : '否'} | ${col.defaultValue || '-'} | ${col.isPrimaryKey ? '是' : '-'} |\n`
      }

      md += '\n'
    }

    return md
  }

  function generateHtml(schema: SchemaInfo, options: DataDictionaryOptions): string {
    let html = `<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <title>数据字典 - ${schema.name}</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 40px; line-height: 1.6; }
    h1 { color: #333; border-bottom: 2px solid #eee; padding-bottom: 10px; }
    h2 { color: #555; margin-top: 40px; }
    h3 { color: #666; }
    table { border-collapse: collapse; width: 100%; margin: 20px 0; }
    th, td { border: 1px solid #ddd; padding: 8px 12px; text-align: left; }
    th { background: #f5f5f5; font-weight: 600; }
    tr:hover { background: #f9f9f9; }
    code { background: #f5f5f5; padding: 2px 6px; border-radius: 3px; }
    .meta { color: #999; font-size: 14px; }
  </style>
</head>
<body>
`

    html += `<h1>数据字典 - ${schema.name}</h1>\n`
    html += `<p class="meta">生成时间：${new Date().toLocaleString()}</p>\n`

    if (options.includeTables && schema.tables.length > 0) {
      html += `<h2>表 (${schema.tables.length})</h2>\n`

      for (const table of schema.tables) {
        html += generateTableHtml(table, options)
      }
    }

    if (options.includeViews && schema.views.length > 0) {
      html += `<h2>视图 (${schema.views.length})</h2>\n`

      for (const view of schema.views) {
        html += `<h3>${view.name}</h3>\n`
        if (view.description) {
          html += `<p>${view.description}</p>\n`
        }
        html += `<pre><code>${view.definition || 'N/A'}</code></pre>\n`
      }
    }

    html += `</body>\n</html>`

    return html
  }

  function generateTableHtml(table: TableInfo, _options: DataDictionaryOptions): string {
    let html = `<h3>${table.name}</h3>\n`

    if (table.description) {
      html += `<p>${table.description}</p>\n`
    }

    if (table.columns && table.columns.length > 0) {
      html += `<table>
<thead>
  <tr><th>列名</th><th>类型</th><th>可空</th><th>默认值</th><th>主键</th></tr>
</thead>
<tbody>
`

      for (const col of table.columns) {
        html += `<tr>
  <td>${col.name}</td>
  <td><code>${col.dataType}</code></td>
  <td>${col.isNullable ? '是' : '否'}</td>
  <td>${col.defaultValue || '-'}</td>
  <td>${col.isPrimaryKey ? '是' : '-'}</td>
</tr>
`
      }

      html += `</tbody>\n</table>\n`
    }

    return html
  }

  function generateJson(schema: SchemaInfo, options: DataDictionaryOptions): string {
    const data = {
      schema: schema.name,
      generatedAt: new Date().toISOString(),
      tables: options.includeTables ? schema.tables : [],
      views: options.includeViews ? schema.views : [],
      indexes: options.includeIndexes ? schema.indexes : [],
    }

    return JSON.stringify(data, null, 2)
  }

  function exportDataDictionary(schema: SchemaInfo, options: DataDictionaryOptions): string {
    switch (options.format) {
      case 'markdown':
        return generateMarkdown(schema, options)
      case 'html':
        return generateHtml(schema, options)
      case 'json':
        return generateJson(schema, options)
      default:
        throw new Error(`Unsupported format: ${options.format}`)
    }
  }

  function downloadFile(content: string, filename: string, mimeType: string) {
    const blob = new Blob([content], { type: mimeType })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = filename
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  }

  function exportAndDownload(
    schema: SchemaInfo,
    options: DataDictionaryOptions,
    filename?: string
  ) {
    const content = exportDataDictionary(schema, options)

    const ext = options.format === 'markdown' ? 'md' : options.format
    const defaultFilename = `data-dictionary-${schema.name}.${ext}`

    downloadFile(
      content,
      filename || defaultFilename,
      options.format === 'html' ? 'text/html' : 'text/plain'
    )
  }

  return {
    exportDataDictionary,
    exportAndDownload,
    generateMarkdown,
    generateHtml,
    generateJson,
  }
}
