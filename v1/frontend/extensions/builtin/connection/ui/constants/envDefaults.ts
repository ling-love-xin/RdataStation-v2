/**
 * 环境预设（静态配置）
 */
import type { EnvInfo } from '../components/tabs/EnvironmentManager.vue'

// ========== Types ==========

export interface EnvPolicyTag {
  key: string
  label: string
  kind: string
}

export interface EnvDefItem {
  id: string
  name: string
  color: string
  icon: string
  desc: string
  builtin: boolean
  summarySecurity: string
  summarySchema: string
  summaryPerf: string
  summaryAudit: string
  ui: { summaryUI: string }
  policy: EnvDefPolicy
}

export interface EnvDefPolicy {
  ro: boolean
  wc: boolean
  ddl: boolean
  drop: string
  ac: boolean
  rl: number
  sl: number
}

/** 环境策略短标签映射 */
export const envPolicyTagsMap: Record<string, EnvPolicyTag[]> = {
  'env-dev': [{ key: 'rw', label: '读写', kind: '' }],
  'env-test': [
    { key: 'rw', label: '读写', kind: '' },
    { key: 'ddl', label: 'DDL确认', kind: '' },
    { key: 'row', label: '行限10000', kind: '' },
  ],
  'env-staging': [
    { key: 'wc', label: '写确认', kind: 'locked' },
    { key: 'ddl', label: 'DDL确认', kind: 'locked' },
    { key: 'schema', label: '手动Schema', kind: '' },
    { key: 'row', label: '行限5000', kind: 'locked' },
    { key: 'audit', label: '审计', kind: 'audit' },
  ],
  'env-prod': [
    { key: 'ro', label: '默认只读', kind: 'danger' },
    { key: 'wc', label: '写确认', kind: 'locked' },
    { key: 'drop', label: 'DROP禁用', kind: 'danger' },
    { key: 'row', label: '行限1000', kind: 'locked' },
    { key: 'audit', label: '审计', kind: 'audit' },
  ],
  'env-sandbox': [
    { key: 'rw', label: '读写', kind: '' },
    { key: 'row', label: '行限1000', kind: '' },
  ],
}

/** 内置环境定义 */
export const envDefs: EnvDefItem[] = [
  {
    id: 'env-dev',
    name: '开发环境',
    color: '#a6e3a1',
    icon: '🟢',
    desc: '本地开发、调试数据库',
    builtin: true,
    summarySecurity: '读写·自动提交',
    summarySchema: '自动Schema+系统表',
    summaryPerf: '池10·超时0s·重连3',
    summaryAudit: '无审计',
    ui: { summaryUI: '#a6e3a1' },
    policy: { ro: false, wc: false, ddl: false, drop: 'false', ac: true, rl: 0, sl: 0 },
  },
  {
    id: 'env-test',
    name: '测试环境',
    color: '#f9e2af',
    icon: '🟡',
    desc: '集成测试、QA 验证',
    builtin: true,
    summarySecurity: '读写·DDL确认·行限1w',
    summarySchema: '自动Schema+系统表',
    summaryPerf: '池10·超时120s·重连3',
    summaryAudit: '基础审计',
    ui: { summaryUI: '#f9e2af' },
    policy: { ro: false, wc: false, ddl: true, drop: 'true', ac: true, rl: 10000, sl: 100 },
  },
  {
    id: 'env-staging',
    name: '预发布',
    color: '#89b4fa',
    icon: '🔵',
    desc: '灰度验证、预发布环境',
    builtin: true,
    summarySecurity: '写确认·DDL确认·行限5k',
    summarySchema: '自动Schema',
    summaryPerf: '池15·超时180s·重连5',
    summaryAudit: '完整审计',
    ui: { summaryUI: '#89b4fa' },
    policy: { ro: false, wc: true, ddl: true, drop: 'true', ac: false, rl: 5000, sl: 50 },
  },
  {
    id: 'env-prod',
    name: '生产环境',
    color: '#f38ba8',
    icon: '🔴',
    desc: '线上生产数据库，谨慎操作',
    builtin: true,
    summarySecurity: '默认只读·写确认·DROP禁用',
    summarySchema: '按需Schema',
    summaryPerf: '池20·超时60s·重连3',
    summaryAudit: '全面审计',
    ui: { summaryUI: '#f38ba8' },
    policy: { ro: true, wc: true, ddl: true, drop: 'disable', ac: false, rl: 1000, sl: 20 },
  },
  {
    id: 'env-sandbox',
    name: '沙箱环境',
    color: '#cba6f7',
    icon: '🟣',
    desc: '安全隔离的沙箱数据库',
    builtin: true,
    summarySecurity: '读写·行限1k',
    summarySchema: '自动Schema',
    summaryPerf: '池5·超时60s·重连2',
    summaryAudit: '无审计',
    ui: { summaryUI: '#cba6f7' },
    policy: { ro: false, wc: false, ddl: false, drop: 'false', ac: true, rl: 1000, sl: 50 },
  },
]

/** 环境默认值（用于 applyEnvDefaults） */
export const envDefaultValues: Record<string, Record<string, unknown>> = {
  'env-dev': {
    ro: false,
    wc: false,
    ddl: false,
    drop: 'false',
    ac: true,
    rl: 0,
    sl: 0,
    ct: 30,
    qt: 0,
    hb: 60,
    mr: 3,
  },
  'env-test': {
    ro: false,
    wc: false,
    ddl: true,
    drop: 'true',
    ac: true,
    rl: 10000,
    sl: 100,
    ct: 30,
    qt: 120,
    hb: 60,
    mr: 3,
  },
  'env-staging': {
    ro: false,
    wc: true,
    ddl: true,
    drop: 'true',
    ac: false,
    rl: 5000,
    sl: 50,
    ct: 30,
    qt: 180,
    hb: 60,
    mr: 5,
  },
  'env-prod': {
    ro: true,
    wc: true,
    ddl: true,
    drop: 'disable',
    ac: false,
    rl: 1000,
    sl: 20,
    ct: 15,
    qt: 60,
    hb: 30,
    mr: 3,
  },
  'env-sandbox': {
    ro: false,
    wc: false,
    ddl: false,
    drop: 'false',
    ac: true,
    rl: 1000,
    sl: 50,
    ct: 30,
    qt: 60,
    hb: 60,
    mr: 2,
  },
}

/** 给 loadEnvironments 用的回退列表（EnvInfo 格式） */
export const envDefsAsEnvInfo: EnvInfo[] = envDefs.map(e => ({
  id: e.id,
  name: e.name,
  color: e.color,
  icon: e.icon,
  desc: e.desc,
  builtin: e.builtin,
  summarySecurity: e.summarySecurity,
  summarySchema: e.summarySchema,
  summaryPerf: e.summaryPerf,
  summaryAudit: e.summaryAudit,
  ui: e.ui,
}))
