// 种子数据与页面状态；不含 DOM 访问。
"use strict";

  /* ================================================================ 数据：8 个示例插件 */
  const SEED = [
    {
      id: "rds.driver.oracle",
      name: "Oracle JDBC Driver",
      mark: "OJ",
      kind: "driver",
      version: "0.2.4",
      latest: "0.2.6",
      updateAvailable: true,
      enabled: true,
      status: "active",
      description: "通过 Oracle JDBC 驱动连接 Oracle 11g–19c，支持会话、查询、取消与元数据读取。",
      author: "RdataStation",
      installed: "2026-10-08",
      size: "12.4 MB",
      source: "本地包 · oracle-jdbc-0.2.4.zip",
      checksum: "已校验 · checksums.json",
      contributions: { commands: 0, panels: 0, settings: 0, drivers: 1 },
      caps: [
        { name: "启动 sidecar 子进程", key: "process.spawn", state: "granted" },
        { name: "把凭据注入子进程环境", key: "env.inherit", state: "granted" },
        { name: "读取包内 JDBC jar", key: "fs.read:driver-jars", state: "granted" }
      ],
      runtime: { mode: "sidecar · Java 17", pid: "4821", uptime: "2 小时 14 分", memory: "68 MB", state: "active" },
      refs: []
    },
    {
      id: "rds.driver.postgres",
      name: "PostgreSQL Driver",
      mark: "PG",
      kind: "driver",
      version: "0.1.8",
      latest: "0.1.8",
      updateAvailable: false,
      enabled: true,
      status: "active",
      description: "Python sidecar 驱动的 PostgreSQL 连接，覆盖 SSL、会话与元数据读取。",
      author: "RdataStation",
      installed: "2026-09-21",
      size: "4.8 MB",
      source: "本地包 · postgres-py-0.1.8.zip",
      checksum: "已校验 · checksums.json",
      contributions: { commands: 0, panels: 0, settings: 0, drivers: 1 },
      caps: [
        { name: "启动 sidecar 子进程", key: "process.spawn", state: "granted" },
        { name: "把凭据注入子进程环境", key: "env.inherit", state: "granted" },
        { name: "读取 Python 运行时", key: "runtime.python", state: "granted" }
      ],
      runtime: { mode: "sidecar · Python 3.11", pid: "5116", uptime: "6 小时 02 分", memory: "41 MB", state: "active" },
      refs: []
    },
    {
      id: "com.example.table-board",
      name: "表大小看板",
      mark: "TB",
      kind: "ui",
      version: "1.0.0",
      latest: "1.0.0",
      updateAvailable: false,
      enabled: false,
      status: "disabled",
      description: "按 schema 汇总表与索引占用，把结果画成宿主渲染的面板。",
      author: "示例作者",
      installed: "2026-09-30",
      size: "86 KB",
      source: "本地目录 · table-board/",
      checksum: "未提供 checksums.json · 清单/入口已校验",
      contributions: { commands: 2, panels: 1, settings: 1, drivers: 0 },
      caps: [
        { name: "读取项目文件", key: "fs.read:project", state: "granted" },
        { name: "注册宿主面板", key: "panel.render", state: "granted" },
        { name: "写项目设置", key: "settings.write", state: "granted" }
      ],
      runtime: { mode: "已停用 · 不占进程", pid: "—", uptime: "—", memory: "—", state: "disabled" },
      refs: [{ project: "示例项目 / 销售分析", detail: "引用了它贡献的面板「表大小看板」" }]
    },
    {
      id: "com.example.sql-kit",
      name: "SQL 方言增强",
      mark: "SQ",
      kind: "ui",
      version: "0.4.1",
      latest: "0.4.3",
      updateAvailable: true,
      enabled: true,
      status: "active",
      description: "补充 Oracle / PostgreSQL 方言的补全与诊断，贡献 3 条命令和 2 个设置项。",
      author: "示例作者",
      installed: "2026-10-02",
      size: "212 KB",
      source: "本地包 · sql-kit-0.4.1.zip",
      checksum: "已校验 · checksums.json",
      contributions: { commands: 3, panels: 0, settings: 2, drivers: 0 },
      caps: [
        { name: "读取当前编辑器文本", key: "editor.read", state: "granted" },
        { name: "注册命令", key: "commands.register", state: "granted" }
      ],
      runtime: { mode: "注册表已加载 · 执行接线未落地", pid: "—", uptime: "—", memory: "—", state: "loaded" },
      refs: []
    },
    {
      id: "com.example.explain",
      name: "执行计划视图",
      mark: "EX",
      kind: "ui",
      version: "0.3.0",
      latest: "0.3.0",
      updateAvailable: false,
      enabled: true,
      status: "active",
      description: "把 EXPLAIN 结果渲染为树与耗时条，面板由宿主渲染，重算留在 DuckDB。",
      author: "示例作者",
      installed: "2026-09-12",
      size: "54 KB",
      source: "本地目录 · explain-view/",
      checksum: "未提供 checksums.json · 清单/入口已校验",
      contributions: { commands: 1, panels: 1, settings: 0, drivers: 0 },
      caps: [
        { name: "读取查询结果", key: "result.read", state: "granted" },
        { name: "注册宿主面板", key: "panel.render", state: "granted" }
      ],
      runtime: { mode: "注册表已加载 · 执行接线未落地", pid: "—", uptime: "—", memory: "—", state: "loaded" },
      refs: []
    },
    {
      id: "com.example.export",
      name: "导出助手",
      mark: "CSV",
      kind: "ui",
      version: "0.2.1",
      latest: "0.2.1",
      updateAvailable: false,
      enabled: false,
      status: "disabled",
      description: "结果集导出为 CSV / Parquet，贡献 2 条命令与 1 个设置项。",
      author: "示例作者",
      installed: "2026-08-29",
      size: "38 KB",
      source: "本地包 · export-helper-0.2.1.zip",
      checksum: "已校验 · checksums.json",
      contributions: { commands: 2, panels: 0, settings: 1, drivers: 0 },
      caps: [
        { name: "读取结果集", key: "result.read", state: "granted" },
        { name: "写文件", key: "fs.write:exports", state: "pending" }
      ],
      runtime: { mode: "已停用 · 不占进程", pid: "—", uptime: "—", memory: "—", state: "disabled" },
      refs: []
    },
    {
      id: "com.example.broken",
      name: "坏包示例",
      mark: "!",
      kind: "ui",
      version: "0.1.0",
      latest: "0.1.0",
      updateAvailable: false,
      enabled: false,
      status: "error",
      error: "清单解析失败：plugin.toml 第 12 行 location 只允许 left / right / center",
      description: "这个包在清单期就被拒绝，但保留在列表中，便于看原因、重试或卸载。",
      author: "未知",
      installed: "2026-10-11",
      size: "—",
      source: "本地目录 · broken-package/",
      checksum: "未提供 checksums.json · 清单/入口已校验",
      contributions: { commands: 0, panels: 0, settings: 0, drivers: 0 },
      caps: [],
      runtime: { mode: "加载失败 · 见错误原因", pid: "—", uptime: "—", memory: "—", state: "error" },
      refs: []
    },
    {
      id: "com.example.dev-theme",
      name: "本地开发主题",
      mark: "DV",
      kind: "ui",
      version: "0.1.0-dev",
      latest: "0.1.0-dev",
      updateAvailable: false,
      enabled: true,
      status: "active",
      dev: true,
      description: "开发中的面板主题，清单变化会自动进入待重载集合。",
      author: "Extension Developer",
      installed: "2026-10-14",
      size: "12 KB",
      source: "本地目录 · dev-theme/",
      checksum: "开发目录 · 清单/入口已校验",
      contributions: { commands: 1, panels: 2, settings: 0, drivers: 0 },
      caps: [
        { name: "读取开发目录", key: "fs.read:dev", state: "granted" },
        { name: "注册宿主面板", key: "panel.render", state: "granted" }
      ],
      runtime: { mode: "开发扩展 · 自动重载", pid: "—", uptime: "—", memory: "—", state: "loaded" },
      refs: []
    }
  ];

  const state = {
    plugins: SEED.map(function (p) {
      return Object.assign({}, p, {
        contributions: Object.assign({}, p.contributions),
        caps: (p.caps || []).map(function (c) { return Object.assign({}, c); }),
        refs: (p.refs || []).map(function (r) { return Object.assign({}, r); })
      });
    }),
    filter: "all",
    kind: "all",
    query: "",
    sort: "default",
    selectedId: "rds.driver.oracle",
    dialog: null,
    theme: "dark",
    reloadPending: 2
  };
