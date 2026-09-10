import { mount } from '@vue/test-utils'
import { describe, it, expect } from 'vitest'

import DistributionChart from '../DistributionChart.vue'

function makeRows(cols: string[], colTypes: string[], data: unknown[][]) {
  return { columns: cols, columnTypes: colTypes, rows: data }
}

describe('DistributionChart', () => {
  // ==================== 空状态 ====================

  it('should show empty state when no data', () => {
    const wrapper = mount(DistributionChart, {
      props: { columns: [], columnTypes: [], rows: [] },
    })
    expect(wrapper.find('.empty-text').text()).toBe('暂无数据')
  })

  // ==================== 数值型直方图 ====================

  it('should render histogram for numeric columns', () => {
    const data = makeRows(
      ['age', 'score'],
      ['integer', 'integer'],
      [
        [25, 88],
        [30, 92],
        [35, 75],
        [28, 85],
        [32, 95],
        [27, 78],
        [33, 90],
        [29, 82],
        [31, 88],
        [26, 91],
      ]
    )
    const wrapper = mount(DistributionChart, { props: data })

    // 应该有直方图
    expect(wrapper.find('.histogram').exists()).toBe(true)
    // 应该有统计信息
    expect(wrapper.find('.dist-stats').exists()).toBe(true)
    // 应该有两个列区块
    const sections = wrapper.findAll('.dist-section')
    expect(sections.length).toBe(2)
  })

  it('should show Min/Max/Avg for numeric columns', () => {
    const data = makeRows(
      ['value'],
      ['integer'],
      [
        [10],
        [20],
        [30],
      ]
    )
    const wrapper = mount(DistributionChart, { props: data })
    const statsText = wrapper.find('.dist-stats').text()
    expect(statsText).toContain('Min')
    expect(statsText).toContain('Max')
    expect(statsText).toContain('Avg')
  })

  it('should handle single numeric value', () => {
    const data = makeRows(
      ['const'],
      ['integer'],
      [
        [5],
        [5],
        [5],
      ]
    )
    const wrapper = mount(DistributionChart, { props: data })
    expect(wrapper.find('.histo-row').exists()).toBe(true)
  })

  // ==================== 文本型频率 ====================

  it('should render frequency list for text columns', () => {
    const data = makeRows(
      ['name'],
      ['varchar'],
      [
        ['Alice'],
        ['Bob'],
        ['Alice'],
        ['Charlie'],
        ['Bob'],
        ['Alice'],
      ]
    )
    const wrapper = mount(DistributionChart, { props: data })

    const section = wrapper.find('.dist-section')
    expect(section.exists()).toBe(true)
    // 应该有频率列表
    expect(wrapper.find('.freq-row').exists()).toBe(true)
  })

  it('should show unique count and null count for text', () => {
    const data = makeRows(
      ['status'],
      ['varchar'],
      [['active'], ['inactive'], [null], ['active'], ['active']]
    )
    const wrapper = mount(DistributionChart, {
      props: data,
    })

    const statsText = wrapper.find('.dist-stats').text()
    expect(statsText).toContain('唯一值')
    expect(statsText).toContain('空值')
  })

  // ==================== 布尔型 ====================

  it('should render boolean distribution', () => {
    const data = makeRows(
      ['active'],
      ['boolean'],
      [
        [true],
        [false],
        [true],
        [true],
        [false],
      ]
    )
    const wrapper = mount(DistributionChart, {
      props: data,
    })

    const statsText = wrapper.find('.dist-stats').text()
    expect(statsText).toContain('True')
    expect(statsText).toContain('False')
  })

  // ==================== 自定义分箱数 ====================

  it('should respect binCount prop', () => {
    const rows = Array.from({ length: 100 }, (_, i) => [i])
    const wrapper = mount(DistributionChart, {
      props: {
        columns: ['id'],
        columnTypes: ['integer'],
        rows,
        binCount: 5,
      },
    })

    const histoRows = wrapper.findAll('.histo-row')
    expect(histoRows.length).toBe(5)
  })

  // ==================== 自定义 topN ====================

  it('should respect topN prop for text columns', () => {
    const rows = [
      ['a'], ['b'], ['c'], ['d'], ['e'],
      ['a'], ['b'], ['c'], ['d'], ['f'],
    ]
    const wrapper = mount(DistributionChart, {
      props: {
        columns: ['letter'],
        columnTypes: ['varchar'],
        rows,
        topN: 3,
      },
    })

    const freqRows = wrapper.findAll('.freq-row')
    expect(freqRows.length).toBeLessThanOrEqual(3)
  })

  // ==================== 列头显示 ====================

  it('should show column name and type in header', () => {
    const data = makeRows(['score'], ['integer'], [[90], [85], [95]])
    const wrapper = mount(DistributionChart, {
      props: data,
    })

    const header = wrapper.find('.dist-header')
    expect(header.text()).toContain('score')
    expect(header.text()).toContain('integer')
  })
})