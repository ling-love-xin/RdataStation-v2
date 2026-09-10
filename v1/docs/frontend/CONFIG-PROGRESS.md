# RdataStation 配置系统 — 开发进度追踪 v2.2

> 更新日期：2026-05-08
> 负责人：AI Assistant + 用户确认
> 总规划：4 阶段

---

## 总览

```
阶段一 ████████████████████ 100%  全局配置链         2026-05-07 ✅
阶段二 ░░░░░░░░░░░░░░░░░░░░   0%  项目配置链           待开始
阶段三 ░░░░░░░░░░░░░░░░░░░░   0%  设置页面 UI         待开始
阶段四 ████████████░░░░░░░░  60%  全局生效            部分完成
────────────────────────────────────────────
总体   ██████████████████░░  78%
```

---

## 阶段一：全局配置链 ✅ 完成

**工期**：1 天（2026-05-07）
**完成标准**：应用重启后全局配置仍然存在

### 任务清单

| #    | 任务                                        | 状态 | 文件                                |
| ---- | ------------------------------------------- | :--: | ----------------------------------- |
| 1.1  | 安装 tauri-plugin-store (Rust + npm + 注册) |  ✅  | Cargo.toml, lib.rs, package.json    |
| 1.2  | 创建 `config.ts` 类型定义 + 注册表 + 默认值 |  ✅  | `src/stores/config.ts`              |
| 1.3  | 创建 `useAppStore.ts` Pinia Store 核心      |  ✅  | `src/stores/useAppStore.ts`         |
| 1.4  | 更新 `main.ts` 初始化顺序                   |  ✅  | `src/app/main.ts`                   |
| 1.5  | 更新 `App.vue` 主题/locale 联动             |  ✅  | `src/app/App.vue`                   |
| 1.6  | 重构 `useUiStore` 委派给 useAppStore        |  ✅  | `src/shared/stores/ui.ts`           |
| 1.7  | 重构 `SettingsPanel` 集成 useAppStore       |  ✅  | `...settings/.../SettingsPanel.vue` |
| 1.8  | **优化轮 v1** 7 项重构                      |  ✅  | 见下文                              |
| 1.9  | **优化轮 v2** Bug 修复 + 功能补全           |  ✅  | 见下文                              |
| 1.10 | **优化轮 v3** 代码质量 + 架构改进           |  ✅  | 见下文                              |
| 1.11 | **优化轮 v4** Bug 修复 + 安全加固           |  ✅  | 见下文                              |
| 1.12 | **优化轮 v5** 架构完善 + 写入校验           |  ✅  | 见下文                              |
| 1.13 | **优化轮 v6** 集成修复 + 消除魔数           |  ✅  | 见下文                              |
| 1.14 | **优化轮 v7** UI 完善 + 双向绑定            |  ✅  | 见下文                              |
| 1.15 | **优化轮 v8** 安全加固 + 类型收窄           |  ✅  | 见下文                              |
| 1.16 | **优化轮 v9** 代码清理 + 签名适配           |  ✅  | 见下文                              |
| 1.17 | **优化轮 v10** Bug 修复 + 死代码清理        |  ✅  | 见下文                              |
| 1.18 | **优化轮 v11** 死代码清理 + 批写优化        |  ✅  | 见下文                              |
| 1.19 | **v2.6.0 Ph2 打通** 布局持久化全线贯通      |  ✅  | 见阶段二                            |

#### 1.8 优化轮 v1 详情（2026-05-07）

| 优化                 | 文件                      | 说明                                                              |
| -------------------- | ------------------------- | ----------------------------------------------------------------- |
| Language 联合类型    | config.ts                 | `string` → `'zh-CN' \| 'en'`                                      |
| 布局/侧边栏类型化    | config.ts                 | `unknown` → `SerializedDockviewLayout` / `SerializedSidebarState` |
| 统一注册表           | config.ts                 | CONFIG_REGISTRY 单一事实来源                                      |
| 种子驱动表           | useAppStore.ts            | 去重 30 行 initialize seed 代码                                   |
| Store 进 Pinia state | useAppStore.ts            | `let` → `shallowRef`                                              |
| Schema 版本管理      | useAppStore.ts            | `_schemaVersion`                                                  |
| Diff 模式项目覆盖    | useAppStore.ts            | 仅存与全局不同的字段                                              |
| 事务性 saveConfig    | useAppStore.ts            | snapshot 回滚                                                     |
| SaveResult 返回      | useAppStore.ts            | UI 可感知成功/失败                                                |
| 去除假安全           | ui.ts                     | try/catch 移除                                                    |
| 清理重复监听器       | ui.ts                     | 仅 App.vue 监听系统主题                                           |
| JSDoc 补充           | config.ts, useAppStore.ts | 完整 JSDoc                                                        |

#### 1.9 优化轮 v2 详情（2026-05-07）

| Bug / 功能              | 文件                       | 说明                                                 |
| ----------------------- | -------------------------- | ---------------------------------------------------- |
| 🔴 系统主题不触发重渲染 | App.vue                    | `systemThemeEpoch` ref + `naiveTheme` computed 依赖  |
| 🔴 dockview defaults    | config.ts                  | 改用合法默认值                                       |
| 🔴 closeProject 主题    | useAppStore.ts             | closeProject/openProject 均调用 applyTheme()         |
| 🟡 PROJECT_SEED_KEYS    | useAppStore.ts             | openProject 去硬编码，从 CONFIG_REGISTRY 派生        |
| 🟡 zod 校验             | config.ts + useAppStore.ts | GlobalConfigSchema / ProjectConfigSchema + safeParse |
| 🟡 resetToFactory       | useAppStore.ts             | 清除 globalStore + 重新 seed + applyTheme            |
| 🟡 SaveResult 统一      | useAppStore.ts             | addRecentProject/removeRecentProject 返回 SaveResult |
| 🟡 initError 暴露       | useAppStore.ts             | `initError: Ref<string \| null>` 供 UI 消费          |

#### 1.10 优化轮 v3 详情（2026-05-07 ~ 2026-05-08）

| 代码质量 / 架构          | 文件           | 说明                                                |
| ------------------------ | -------------- | --------------------------------------------------- |
| 🟢 structuredClone       | useAppStore.ts | 替换 JSON hack deepClone                            |
| 🟢 saveConfig 泛型       | useAppStore.ts | `<K extends ConfigKey>` 类型映射防止写错            |
| 🟢 saveBatch             | useAppStore.ts | 跨 scope 分组，统一 store.save() 减少 I/O           |
| 🟢 effectiveTheme 语义   | ui.ts          | 直接从 appStore 取，不降级                          |
| 🟢 toggleTheme 三态      | ui.ts          | dark → light → system → dark                        |
| 🟢 main.ts 自动打开项目  | main.ts        | mount 后 openProject(recentProjects[0])             |
| 🟢 逐字段 zod 校验       | useAppStore.ts | 一个字段损坏不触发全局 reseed，仅 reseed 该字段     |
| 🟢 OVERRIDE_RULES 驱动   | useAppStore.ts | getConfigInternal 消除硬编码 if-chain               |
| 🟢 computeDiff 提取      | useAppStore.ts | 泛型深比较工具，setEditorSettings 复用              |
| 🟢 loadStoreWithDefaults | useAppStore.ts | Store 加载 + 校验 + seed 通用辅助                   |
| 🟢 reloadConfig          | useAppStore.ts | 外部修改同步：从 JSON 文件重新读取 + 逐字段校验     |
| 🟢 onConfigChanged       | useAppStore.ts | 审计 hook 占位，迁移 Rust 后端后赋值日志审计函数    |
| 🟢 auto-persist watcher  | useAppStore.ts | projectConfig deep watch + 500ms debounce 自动 save |
| 🟢 resetToFactory 增强   | useAppStore.ts | 清除前先 closeProject()，避免孤儿 projectStore      |
| 🟢 initError UI 横幅     | App.vue        | NAlert 固定顶部显示初始化失败原因                   |
| 🟢 stores/README.md      | src/stores/    | 目录文档：架构位置/数据流/使用方式/迁移路径         |

#### 1.11 优化轮 v4 详情（2026-05-08）

| Bug / 功能 / 质量         | 文件                       | 说明                                                         |
| ------------------------- | -------------------------- | ------------------------------------------------------------ |
| 🔴 openProject 异常状态   | useAppStore.ts             | catch 分支不再设 projectOpen=true，重置所有状态为 null/false |
| 🔴 autoSaveTimer 未取消   | useAppStore.ts             | closeProject() 调用 cancelAutoSaveTimer()                    |
| 🟡 reloadConfig zod       | useAppStore.ts             | 复用 loadStoreWithDefaults，补全逐字段校验                   |
| 🟡 beforeunload 保存      | useAppStore.ts             | initialize() 注册 window.beforeunload，flush pending save    |
| 🟡 initError 可关闭       | App.vue + useAppStore.ts   | NAlert closable + clearInitError()                           |
| 🟢 loadStoreWithDefaults  | useAppStore.ts             | initialize() / reloadConfig() 统一接入，消除重复代码         |
| 🟢 死代码清理             | config.ts                  | 移除 EditorSettingsParsed / RESERVED_KEYS                    |
| 🟢 saveBatch 审计         | useAppStore.ts             | onConfigChanged 调用补全                                     |
| 🟢 safeErrorMessage       | useAppStore.ts             | 提取公共错误消息工具函数                                     |
| 🟢 globalConfig 只读      | useAppStore.ts             | readonly() 包装，防止组件绕过 saveConfig                     |
| 🟢 状态字段只读           | useAppStore.ts             | initialized/initError/projectOpen/projectPath 统一 readonly  |
| 🟢 setConfigChangeHandler | useAppStore.ts             | 公开 setter，允许外部注入审计 handler                        |
| 📝 i18n 补充              | locales/zh-CN.json/en.json | 新增 config.initError.title + .hint                          |

#### 1.12 优化轮 v5 详情（2026-05-08）

| 架构 / 安全 / 质量         | 文件                       | 说明                                                                         |
| -------------------------- | -------------------------- | ---------------------------------------------------------------------------- |
| 🏗️ ConfigValueType 派生    | config.ts                  | writeType 字段 + `(typeof CONFIG_REGISTRY)[K]['writeType']`，消除7分支硬编码 |
| 🏗️ Schema 迁移函数         | config.ts + useAppStore.ts | MIGRATIONS 注册表 + migrateConfig()，initialize() 自动运行                   |
| 🏗️ 原子 schemas 提取       | config.ts                  | ThemeSchema/LanguageSchema/... 7个原子schema，组合复用                       |
| 🛡️ 写入时 zod 校验         | useAppStore.ts             | saveConfig 通过 VALUE_SCHEMAS 校验值，非法值拒绝写入                         |
| 🛡️ VALUE_SCHEMAS 表        | config.ts + useAppStore.ts | CONFIG_REGISTRY → ValueSchemaLookup 自动派生                                 |
| 🛡️ editorSettings 部分校验 | useAppStore.ts             | project scope 使用 .partial() 允许部分字段写入                               |

#### 1.13 优化轮 v6 详情（2026-05-08）

| Bug / 质量 / 集成      | 文件                        | 说明                                                                |
| ---------------------- | --------------------------- | ------------------------------------------------------------------- |
| 🔴 Pinia 反模式        | workbench/SettingsPanel.vue | saveSettings/loadSettings 内 useAppStore() 调回顶层，去 try/catch   |
| 🟡 resetToDefault 常量 | settings/SettingsPanel.vue  | 硬编码 9 个默认值 → DEFAULT_GLOBAL_CONFIG + DEFAULT_EDITOR_SETTINGS |
| 🟡 SaveResult 反馈     | settings/SettingsPanel.vue  | applyAllSettings Promise.all + 收集失败项 console.error             |
| 🟢 sidebar 魔数消除    | ui.ts + config.ts           | setSidebarWidth Math.max/min 用 SIDEBAR_WIDTH_MIN/MAX               |
| 🟢 SIDEBAR 常量导出    | config.ts                   | 新增导出 SIDEBAR_WIDTH_MIN/MAX                                      |

#### 1.14 优化轮 v7 详情（2026-05-08）

| 功能 / 质量 / i18n    | 文件                       | 说明                                                                              |
| --------------------- | -------------------------- | --------------------------------------------------------------------------------- |
| 🟡 store↔UI 双向绑定  | settings/SettingsPanel.vue | 4 个 watch：effectiveTheme/Language/EditorSettings/DefaultEngine → local ref 同步 |
| 🟡 fontFamily 控件    | settings/SettingsPanel.vue | 新增文本输入框 v-model="localEditorSettings.fontFamily"                           |
| 🟢 resetToFactory     | settings/SettingsPanel.vue | 新增按钮 + 函数，调用 appStore.resetToFactory() 后同步本地                        |
| 🟢 出厂/默认 语义区分 | settings/SettingsPanel.vue | resetToDefault=本地重置+写入 / resetToFactory=JSON清除+还原                       |
| 📝 i18n 补全 x3       | zh-CN.json / en.json       | settings.fontFamily/fontFamilyHint/resetFactory                                   |

#### 1.15 优化轮 v8 详情（2026-05-08）

| 安全 / 质量 / 文档     | 文件                       | 说明                                                          |
| ---------------------- | -------------------------- | ------------------------------------------------------------- |
| 🛡️ saveBatch 校验      | useAppStore.ts             | 补全 VALUE_SCHEMAS 校验，与 saveConfig 一致，失败条目单独标记 |
| 🛡️ reloadConfig schema | useAppStore.ts             | 新增 \_schemaVersion 检测 + migrateConfig() 迁移              |
| 🟢 SeedEntry 类型收窄  | config.ts + useAppStore.ts | key: string → ConfigKey，消除 2 处 as ConfigKey               |
| 🟢 openProject 返回    | useAppStore.ts             | void → { success, error }，调用方可感知失败                   |
| 📝 MIGRATIONS 示例     | config.ts                  | @example 注释：展示如何添加 locale→language 迁移函数          |

#### 1.16 优化轮 v9 详情（2026-05-08）

| 清理 / 适配           | 文件      | 说明                                                             |
| --------------------- | --------- | ---------------------------------------------------------------- |
| 🧹 GlobalConfigParsed | config.ts | 移除从未导入的死类型导出                                         |
| 🔧 main.ts 签名适配   | main.ts   | .catch(e) → const result = await + result.success + result.error |

#### 1.17 优化轮 v10 详情（2026-05-08）

| Bug 修复 / 清理       | 文件           | 说明                                       |
| --------------------- | -------------- | ------------------------------------------ |
| 🐛 written 未声明     | useAppStore.ts | const 声明补全，修复运行时 ReferenceError  |
| 🧹 ConfigOverrideRule | config.ts      | 移除死导出（仅内部 satisfies 子句使用）    |
| 🧹 Theme 死重导出     | ui.ts          | export type { Theme } 无外部引用，直接移除 |

#### 1.18 优化轮 v11 详情（2026-05-08）

| 性能 / 清理         | 文件                       | 说明                                       |
| ------------------- | -------------------------- | ------------------------------------------ |
| ⚡️ saveBatch 批写   | settings/SettingsPanel.vue | 4 次独立 store.save() I/O → 1 次 saveBatch |
| 🧹 SeedEntry 死导出 | config.ts                  | 仅内部使用，无外部引用                     |
| 🧹 initTheme 死函数 | ui.ts                      | 0 调用方，仅 applyTheme 包装层             |

#### 1.19 v2.6.0 Phase 2 打通（2026-05-08）

| 集成                           | 文件                  | 说明                                              |
| ------------------------------ | --------------------- | ------------------------------------------------- |
| ✨ openProject 生命周期        | ProjectSelectView.vue | enterWorkbench → appStore.openProject(path)       |
| ✨ closeProject 生命周期       | WorkbenchView.vue     | onUnmounted → appStore.closeProject()             |
| ✨ saveLayoutConfig 双写       | layout-store.ts       | localStorage + appStore.saveSidebarState()        |
| ✨ loadLayoutConfig 优先读取   | layout-store.ts       | effectiveSidebarState → 回退 localStorage         |
| ✨ setLayoutData 自动保存      | layout-store.ts       | appStore.saveDockviewLayout()                     |
| ✨ onDidLayoutChange 监听      | WorkbenchView.vue     | api.toJSON() → setLayoutData → saveDockviewLayout |
| ✨ SerializedSidebarState 扩展 | config.ts             | 3 字段 → 14 字段（完整工作台布局）                |

#### 1.20 v2.7.0 Phase 3/4 实时生效（2026-05-08）

| 功能 / 修复                     | 文件                       | 说明                                                     |
| ------------------------------- | -------------------------- | -------------------------------------------------------- |
| ✨ 编辑器设置实时同步           | SqlEditorPanel.vue         | watch effectiveEditorSettings → 所有编辑器面板立即更新   |
| ✨ 编辑器初始状态从 config 读取 | SqlEditorPanel.vue         | 消除 fontSize=14/wordWrap=true 硬编码                    |
| ✨ SaveResult toast 反馈        | settings/SettingsPanel.vue | message.success/error + zh-CN/en i18n 键                 |
| 🧹 影响矩阵修正                 | CONFIG-PROGRESS.md         | MainLayout/useLayoutStore Ph2 ✅ + SqlEditorPanel Ph4 ✅ |

#### 1.21 v2.8.0 Phase 2.1 面板恢复（2026-05-08）

| 功能 / 修复       | 文件              | 说明                                                         |
| ----------------- | ----------------- | ------------------------------------------------------------ |
| ✨ 面板 ID 追踪   | WorkbenchView.vue | onDidLayoutChange → api.panels.map(p => p.id)                |
| ✨ 面板 ID 持久化 | layout-store.ts   | openPanelIds ref → saveLayoutConfig → saveSidebarState       |
| ✨ 面板恢复       | WorkbenchView.vue | restoreSavedPanels → panelRegistry 匹配 → api.addPanel       |
| ✨ openPanelIds   | config.ts         | SerializedSidebarState + zod schema + default 新增 15th 字段 |

### 阶段一 验证

| 检查                |         结果         |
| ------------------- | :------------------: |
| TypeScript (新文件) | v2.8.0 0 新增错误 ✅ |
| ESLint (新文件)     | v2.8.0 0 新增错误 ✅ |
| Rust cargo check    |  预存错误，无关 ✅   |

---

## 阶段二：项目配置链 ✅ 已完成

**工期**：实际 2026-05-08（v2.6.0）
**完成标准**：打开不同项目可以有不同的布局和配置

### 任务清单

| #   | 任务                                      | 依赖    | 文件                                 | 状态 |
| --- | ----------------------------------------- | ------- | ------------------------------------ | ---- |
| 2.1 | AppLayout 集成 `openProject()`            | 1.3     | `ProjectSelectView.vue`              | ✅   |
| 2.2 | AppLayout 集成 `closeProject()`           | 2.1     | `WorkbenchView.vue`                  | ✅   |
| 2.3 | dockview 布局读 `effectiveDockviewLayout` | 2.1     | `layout-store.ts` (setLayoutData)    | ✅   |
| 2.4 | dockview 布局写 `saveDockviewLayout()`    | 2.3     | `layout-store.ts` / `WorkbenchView`  | ✅   |
| 2.5 | 侧边栏状态读 `effectiveSidebarState`      | 2.1     | `layout-store.ts` (loadLayoutConfig) | ✅   |
| 2.6 | 侧边栏状态写 `saveSidebarState()`         | 2.5     | `layout-store.ts` (saveLayoutConfig) | ✅   |
| 2.7 | 编辑器设置优先级合并验证                  | 2.1     | 集成测试                             | ✅   |
| 2.8 | 项目切换协议测试                          | 2.1-2.6 | 集成测试                             | ✅   |

### 数据流

```
ProjectSelectView.enterWorkbench()
  ├── appStore.openProject(project.path)
  │     └── 加载 project-settings.json（sidebarState + dockviewLayout + project config）
  └── router.push('/workbench')
        └── WorkbenchView.onMounted()
              ├── layoutStore.loadLayoutConfig()
              │     └── 优先: appStore.effectiveSidebarState → 回退: localStorage
              └── WorkbenchView.onReady()
                    ├── 创建默认面板布局
                    └── api.onDidLayoutChange() → saveDockviewLayout()

WorkbenchView.onUnmounted()
  └── appStore.closeProject()
        └── 保存 + 清理 projectStore
```

### 持久化策略

| 数据                 | 主存储                        | 兼容层       |
| -------------------- | ----------------------------- | ------------ |
| 侧边栏状态 (14 字段) | `project-settings.json`       | localStorage |
| Dockview 面板布局    | `project-settings.json`       | —            |
| 主题/语言/编辑器     | `global-settings.json` (已有) | —            |

---

## 阶段三：设置页面 UI ⬜ 待开始

**工期**：预估 1 天
**完成标准**：用户可以通过 UI 修改全部设置并生效

### 任务清单

| #   | 任务                             | 文件                      |
| --- | -------------------------------- | ------------------------- |
| 3.1 | 优化三个标签页布局               | `SettingsPanel.vue`       |
| 3.2 | 标签页 1：外观（完成度 90%）     | 已有主题/语言             |
| 3.3 | 标签页 2：编辑器（完成度 70%）   | 已有基础，加字体选择器    |
| 3.4 | 标签页 3：高级（新建）           | 默认引擎/连接池/历史      |
| 3.5 | 可覆盖项的"恢复全局默认值"按钮   | 已有（theme），扩展到全部 |
| 3.6 | "应用"按钮增强：SaveResult toast | 集成 message API          |
| 3.7 | 设置搜索/筛选                    | 可选项                    |

---

## 阶段四：全局生效 🔄 部分完成（50%）

**工期**：预估 1 天
**完成标准**：修改设置后所有组件即时反映变化

### 任务清单

| #    | 任务                                           | 文件               |   状态    |
| ---- | ---------------------------------------------- | ------------------ | :-------: |
| 4.1  | 主题切换 → naive-ui 即时更新                   | App.vue            | ✅ v2.5.2 |
| 4.2  | 语言切换 → vue-i18n + naive-ui locale 即时更新 | App.vue            |  ✅ v2.5  |
| 4.3  | 编辑器设置 → Monaco Editor 即时更新            | `MonacoEditor.vue` |    ⬜     |
| 4.4  | `wordWrap` → Monaco.updateOptions              | `MonacoEditor.vue` |    ⬜     |
| 4.5  | `minimap` → Monaco.updateOptions               | `MonacoEditor.vue` |    ⬜     |
| 4.6  | `lineNumbers` → Monaco.updateOptions           | `MonacoEditor.vue` |    ⬜     |
| 4.7  | 默认引擎 → 新建查询默认引擎选择                | `QueryEditor.vue`  |    ⬜     |
| 4.8  | 全局生效验证测试                               | 集成测试           |    ⬜     |
| 4.9  | initError UI 横幅反馈                          | App.vue            | ✅ v2.5.3 |
| 4.10 | initError 可关闭                               | App.vue            | ✅ v2.5.4 |
| 4.11 | beforeunload 退出保存                          | useAppStore.ts     | ✅ v2.5.4 |
| 4.12 | globalConfig 只读防护                          | useAppStore.ts     | ✅ v2.5.4 |

### 响应式链路

```
useAppStore.effectiveEditorSettings.fontSize 变化
  → watch( effectiveEditorSettings, handler, { deep: true } )
  → MonacoEditor.onConfigChange()
  → editor.updateOptions({ fontSize: newValue })
  → 实时生效
```

---

## 文件影响矩阵（已完成 + 待完成）

| 文件                                 | Phase 1 | Phase 2 | Phase 3 | Phase 4 |
| ------------------------------------ | :-----: | :-----: | :-----: | :-----: |
| `src/stores/config.ts`               | ✅ 新建 |    —    |    —    |    —    |
| `src/stores/useAppStore.ts`          | ✅ 新建 |    —    |    —    |    —    |
| `src/stores/README.md`               | ✅ 新建 |    —    |    —    |    —    |
| `src/shared/stores/ui.ts`            | ✅ 重构 |    —    |    —    |    —    |
| `src/app/main.ts`                    | ✅ 修改 |    —    |    —    |    —    |
| `src/app/App.vue`                    | ✅ 修改 |    —    |    —    |   ✅    |
| `...settings/.../SettingsPanel.vue`  | ✅ 重构 |    —    |   ✅    |    —    |
| `...workbench/.../SettingsPanel.vue` | ✅ 修改 |    —    |   ✅    |    —    |
| `MainLayout.vue`                     |    —    |   ✅    |    —    |    —    |
| `useLayoutStore`                     |    —    |   ✅    |    —    |    —    |
| `SqlEditorPanel.vue`                 |    —    |    —    |    —    |   ✅    |
| `QueryEditor.vue`                    |    —    |    —    |    —    |   ⬜    |

---

## 技术债务

| #   | 条目                                                                 | 优先级 |    状态    |
| --- | -------------------------------------------------------------------- | :----: | :--------: |
| T1  | `openProject` 中硬编码 5 个 seed keys → 应读 PROJECT_SEED_KEYS       |   中   | ✅ v2.5.2  |
| T2  | 迁移遗留 localStorage 布局设置到 projectStore                        |   中   |     ⚠️     |
| T3  | 进度条 / loading 在 `SaveResult.error` 时的 UI 反馈                  |   低   |     ⚠️     |
| T4  | 配置变更审计 hook（`onConfigChanged`）                               |   低   | ✅ v2.5.3  |
| T5  | 乐观更新支持（网络存储场景）                                         |   低   |     ⚠️     |
| T6  | initialize 失败静默 → 已加 `initError` ref + UI 横幅                 |   中   | ✅ v2.5.3  |
| T7  | `deepClone` JSON hack → `structuredClone()`                          |   低   | ✅ v2.5.2  |
| T8  | `saveConfig` 类型安全弱 → 泛型约束                                   |   中   | ✅ v2.5.2  |
| T9  | 批量保存无 → `saveBatch()`                                           |   中   | ✅ v2.5.2  |
| T10 | `useUiStore.effectiveTheme` 语义不一致                               |   中   | ✅ v2.5.2  |
| T11 | `toggleTheme()` 跳过 system                                          |   中   | ✅ v2.5.2  |
| T12 | dockviewLayout/sidebarState 空对象默认值                             |   高   | ✅ v2.5.2  |
| T13 | 系统主题不触发 naive-ui 重渲染                                       | 🔴 Bug | ✅ v2.5.2  |
| T14 | closeProject 主题不回退                                              | 🔴 Bug | ✅ v2.5.2  |
| T15 | 单字段校验失败触发全局 reseed                                        | 🔴 Bug | ✅ v2.5.3  |
| T16 | `as EditorSettings` 假断言                                           | 🔴 Bug | ✅ v2.5.3  |
| T17 | `getConfigInternal` if-chain 硬编码                                  |   中   | ✅ v2.5.3  |
| T18 | `resetToFactory` 不清理项目状态                                      |   中   | ✅ v2.5.3  |
| T19 | `computeDiff` 无提取 → 泛型工具复用                                  |   低   | ✅ v2.5.3  |
| T20 | 外部修改无法同步 → `reloadConfig()`                                  |   低   | ✅ v2.5.3  |
| T21 | Schema 版本有存储无迁移函数                                          |   中   | ✅ v2.5.5  |
| T22 | `ConfigValueType` 手工维护 vs `OVERRIDE_RULES` 自动派生              |   低   | ✅ v2.5.5  |
| T23 | projectStore 文件名依赖路径字符串                                    |   中   |     ⚠️     |
| T24 | `useUiStore` sidebarWidth/sidebarCollapsed 与 projectConfig 语义重叠 |   中   |     ⚠️     |
| T25 | 无集中式配置变更通知机制                                             |   低   |     ⚠️     |
| T26 | `written` 变量在 saveBatch 中未声明（运行时 ReferenceError）         | 🔴 Bug | ✅ v2.5.10 |
| T27 | SettingsPanel 4 次独立 store.save() I/O → 未使用 saveBatch 批写      |   中   | ✅ v2.5.11 |
| T28 | 布局持久化未贯通（Phase 2 未连线），侧边栏状态/dockview 布局丢失     | 🔴 高  | ✅ v2.6.0  |
| T29 | `SerializedSidebarState` 仅 3 字段，无法承载完整工作台布局状态       |   中   | ✅ v2.6.0  |

---

## 变更历史

| 日期               | 变更                                                                                                                                                                                           |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2026-05-08 v2.8.0  | **Phase 2.1 面板恢复**：onDidLayoutChange 追踪面板 ID + SerializedSidebarState.openPanelIds + restoreSavedPanels 还原面板                                                                      |
| 2026-05-08 v2.7.0  | **Phase 3/4 打通**：Monaco Editor 实时同步编辑器设置 + SettingsPanel SaveResult toast 反馈 + 编辑器初始状态从 config 读取 + 影响矩阵修正                                                       |
| 2026-05-08 v2.6.0  | **Phase 2 打通**：项目生命周期（openProject/closeProject）+ 侧边栏状态持久化（双写 localStorage + config）+ dockview 布局自动保存 + SerializedSidebarState 扩展至 14 字段                      |
| 2026-05-08 v2.5.11 | 优化轮 v11：SettingsPanel saveBatch 批写 + SeedEntry 死导出清理 + ui.ts initTheme 死函数清理                                                                                                   |
| 2026-05-08 v2.5.10 | 优化轮 v10：saveBatch written 未声明修复 + ConfigOverrideRule 死导出清理 + ui.ts Theme 死重导出清理                                                                                            |
| 2026-05-08 v2.5.9  | 优化轮 v9：GlobalConfigParsed 死导出清理 + main.ts openProject 签名适配                                                                                                                        |
| 2026-05-08 v2.5.8  | 优化轮 v8：saveBatch 校验 + reloadConfig schema + SeedEntry 类型收窄 + openProject 返回 + MIGRATIONS 示例                                                                                      |
| 2026-05-08 v2.5.7  | 优化轮 v7：store↔UI 双向绑定 + fontFamily 控件 + resetToFactory 按钮 + 3 个 i18n 键补全                                                                                                        |
| 2026-05-08 v2.5.6  | 优化轮 v6：workbench Pinia 反模式修复 + settings resetToDefault 常量化 + SaveResult 反馈 + sidebar 魔数消除                                                                                    |
| 2026-05-08 v2.5.5  | 优化轮 v5：ConfigValueType 自动派生 + Schema 迁移函数 + 原子 schemas 提取 + saveConfig 写入时 zod 校验 + VALUE_SCHEMAS 查找表                                                                  |
| 2026-05-08 v2.5.4  | 优化轮 v4：openProject 异常修复 + reloadConfig zod + beforeunload + initError 可关闭 + loadStoreWithDefaults 接入 + 死代码清理 + saveBatch 审计 + globalConfig 只读 + i18n 补充 + 故障排查章节 |
| 2026-05-08 v2.5.3  | 优化轮 v3：逐字段 zod 校验 + OVERRIDE_RULES 驱动 + computeDiff 提取 + auto-persist + reloadConfig + onConfigChanged + initError UI 横幅 + stores/README.md                                     |
| 2026-05-07 v2.5.2  | 修复 3 个 Bug + 5 项功能补全 + 11 项技术债务解决                                                                                                                                               |
| 2026-05-07 v2.5    | 阶段一完成（含优化轮 v1）+ 文档初始化                                                                                                                                                          |
