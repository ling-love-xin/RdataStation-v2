# Tasks

- [x] Task 1: 合并 useNetworkChain 与 useNetworkProfiles 配置文件状态
  - [x] 1.1: `useNetworkChain` 内部移除 `sshProfiles`/`sslProfiles`/`proxyProfiles` ref 声明
  - [x] 1.2: `getProfiles()` 改为从 `useNetworkProfiles()` 读取共享状态
  - [x] 1.3: `saveNewHop()` 改为调用 `useNetworkProfiles.saveProjectProfile()` 写入后端
  - [x] 1.4: `loadProfilesFromDb()` 改为调用 `useNetworkProfiles.loadAll()`
  - [x] 1.5: `deleteProfile()` 改为调用 `useNetworkProfiles.removeProjectProfile()`
  - [x] 1.6: `initProfiles()` 移除或改为空操作
  - [x] 1.7: `networkConfig` computed 适配新的 profiles 来源
  - [x] 1.8: `topologyNodes` computed 适配新的 profiles 来源
  - [x] 1.9: `vue-tsc --noEmit` + ESLint 通过

- [x] Task 2: GeneralTab 动态表单由 config_schema 驱动
  - [x] 2.1: 新增 `parseConfigSchema(schema: string): DriverField[]` 工具函数
  - [x] 2.2: `GeneralTab` 中移除旧 `DriverField[]` 硬编码字段列表
  - [x] 2.3: 监听 `driver` prop 变化，自动调用 `parseConfigSchema(driver.config_schema)` 重新渲染
  - [x] 2.4: 表单字段按 config_schema 的 order 属性排序（缺失时按定义顺序）
  - [x] 2.5: 认证选择器（authMethod/selectedAuthConfig）仍然在 GeneralTab 中独立渲染
  - [x] 2.6: `vue-tsc --noEmit` + ESLint 通过

- [x] Task 3: 环境选择器自动加载策略
  - [x] 3.1: `EnvironmentStore.selectEnv` 变为 async，内部自动调用 `fetchPolicies(envId)`
  - [x] 3.2: `AdvancedTab` 的 `onEnvChange` 处理器改为调用 `environmentStore.selectEnv()`
  - [x] 3.3: 移除 `AddDataSourceDialog` 中任何手动 `fetchPolicies` 调用
  - [x] 3.4: `vue-tsc --noEmit` + ESLint 通过

- [x] Task 4: StagingItem 密码安全 + isResetting 消除
  - [x] 4.1: `buildStagingItem()` 在写入前从 `formData` 中 `delete formData.password`
  - [x] 4.2: `useAddDataSource.ts` 移除 `isResetting` ref 及所有赋值引用
  - [x] 4.3: `handleSelectStaging` 用 `nextTick` 替代 `isResetting` 守卫
  - [x] 4.4: `handleAddStaging` 移除 isResetting 相关代码
  - [x] 4.5: `vue-tsc --noEmit` + ESLint 通过

- [x] Task 5: 拆分 AdvancedTab（1340 行 → 198 行）
  - [x] 5.1: 抽取 `EnvironmentSection.vue` — 环境选择器 + 策略标签 + 快照指示 + CRUD
  - [x] 5.2: 抽取 `DuckDBAccelSection.vue` — DuckDB 加速配置（已存在，确认完整性）
  - [x] 5.3: 抽取 `PolicySections.vue` — 安全/性能/Schema/审计/UI 策略
  - [x] 5.4: 抽取 `MetadataSection.vue` — 数据源元数据（schemaName/options/metadataPath/tags）
  - [x] 5.5: `AdvancedTab.vue` 变为组合层，198 行
  - [x] 5.6: `vue-tsc --noEmit` + ESLint 通过

- [x] Task 6: CapabilitiesTab 数据写入 advancedOptions
  - [x] 6.1: `CapabilitiesTab` 解析 `driver.capabilities` JSON 字符串
  - [x] 6.2: 将解析后的能力标志通过 `extraConfig` emit 传递给父组件
  - [x] 6.3: `AddDataSourceDialog` 合并 capabilities 标志到 `advancedOptions`
  - [x] 6.4: `vue-tsc --noEmit` + ESLint 通过

- [x] Task 7: URI 编辑模式校验提示
  - [x] 7.1: `DataSourceHeader` 添加 `uriWarning` computed — 比对 url_template 占位符
  - [x] 7.2: 当 `uriEditing=true` 且 URL 与 url_template 模式不匹配时显示 NAlert 警告
  - [x] 7.3: `vue-tsc --noEmit` + ESLint 通过

- [x] Task 8: 全局验证
  - [x] 8.1: `cargo check` — Rust 端零错误
  - [x] 8.2: `vue-tsc --noEmit` — TypeScript 零新增错误（预存错误均在无关文件）
  - [x] 8.3: ESLint 修改文件零 error
  - [x] 8.4: 手动验证：打开新增数据源 → 新建 SSH 配置 → 协议链下拉即时出现该配置
  - [x] 8.5: 手动验证：切换驱动 → GeneralTab 表单字段动态变化
  - [x] 8.6: 手动验证：选择环境 → 策略标签自动更新
  - [x] 8.7: 手动验证：保存暂存 → localStorage 检查无 password 键

## Round 12 — 用户体验痛点修复 (2026-06-11)

- [x] Task 9: 连接名称重复校验
  - [x] 9.1: `handleCreateApply` 添加 `nameSet` 批内去重（AddDataSourceDialog.vue）
  - [x] 9.2: 后端 `global_db.rs` SQL `SELECT COUNT(*) FROM global_connections WHERE name = ?1` 唯一性检查
  - [x] 9.3: 后端 `project_connection_store.rs` SQL `SELECT COUNT(*) FROM connections WHERE name = ?1` 唯一性检查
  - [x] 9.4: 重复名称弹出 `message.warning` 提示

- [x] Task 10: 暂存项切换丢失数据确认
  - [x] 10.1: 新增 `stagingDirty` ref，watch `formData`/`protocolChain` 变化时设为 true
  - [x] 10.2: `handleSelectStaging` 切换前检查 `stagingDirty`，弹 `dialog.warning` 确认
  - [x] 10.3: `zh-CN.json` 新增 `stagingSwitchTitle`/`stagingSwitchHint` 国际化文案
  - [x] 10.4: `saveToStaging`/`selectStaging` 后重置 `stagingDirty = false`

- [x] Task 11: URL 自动解析填充表单
  - [x] 11.1: `useUrlBuilder` 新增 `parseUrl()` 函数，支持文件型和标准 URL 解析
  - [x] 11.2: `ParsedUrl` 接口定义（driver/host/port/database/username/password/params/isFile/filePath）
  - [x] 11.3: `DataSourceHeader` 新增"解析"按钮，emit `parseUrl(manualUri)`
  - [x] 11.4: `AddDataSourceDialog` 新增 `onParseUrl` 处理器，匹配驱动 + 填充 `formData`

# Task Dependencies

- Task 2 独立执行（GeneralTab 不依赖 NetworkTab）
- Task 5 依赖 Task 3（都改 AdvancedTab）
- Task 4 独立执行
- Task 6 独立执行
- Task 7 独立执行
- Task 8 依赖 Task 1-7 全部完成

## Round 13 — 全链路审计修复 (2026-06-11)

- [x] Task 12: scope 覆盖逻辑修复
  - [x] 12.1: `handleCreateApply` 将 `scope.global \|\| item.scope === 'global' \|\| item.scope === 'both'` 改为 `scope.global`（对话框勾选为权威来源）

- [x] Task 13: authMethod fallback 修复
  - [x] 13.1: `saveToProjectOnly` `\|\|` → `??` 仅 null/undefined 回退

- [x] Task 14: stagingDirty 误触发修复
  - [x] 14.1: 新增 `isRestoring` ref
  - [x] 14.2: watch 跳过 `isRestoring` 期间
  - [x] 14.3: `handleSelectStaging` 恢复数据前后设置 `isRestoring`

- [x] Task 15: handleClose 脏检查
  - [x] 15.1: `hasChanges` 添加 `stagingDirty.value \|\|`

- [x] Task 16: 国际化硬编码修复
  - [x] 16.1: `DataSourceHeader.vue` "解析" → `{{ t('navigator.parseUrl') }}`
  - [x] 16.2: `onParseUrl` 消息 → `t('navigator.parseUrlFailed')` / `t('navigator.parseUrlSuccess')`
  - [x] 16.3: 重复名称提示 → `t('navigator.duplicateName', { name })`
  - [x] 16.4: `zh-CN.json` 新增 6 keys
  - [x] 16.5: `en.json` 新增 6 keys + stagingSwitchTitle/Hint

- [x] Task 17: AdvancedTab 修复
  - [x] 17.1: `envId` 初始值 `'env-dev'` → `ref<string | null>(null)`
  - [x] 17.2: emit `envId` → `environmentId`（与 `onExtraConfig` 对齐，修复环境 ID 静默丢失）

- [x] Task 18: 暂存项重载
  - [x] 18.1: `useAddDataSource` 导出 `loadStagingItems`
  - [x] 18.2: `watch open` 中调用 `loadStagingItems()`

## Round 14 — 深层审计修复 (2026-06-11)

- [x] Task 19: 消除异步竞态
  - [x] 19.1: `AddDataSourceDialog` `watch open` 中 `loadAll()` → `await loadAll()`（确保 `initFromConnection` 执行时 drivers 已就绪）
  - [x] 19.2: `NetworkTab` `onMounted` 中 `loadAll()` → `await loadAll()`（消除与 `loadAllProject()` 竞态）

- [x] Task 20: handleEditApply 部分成功处理
  - [x] 20.1: project/global 分拆 try/catch，分别跟踪 projectOk/globalOk
  - [x] 20.2: 全部成功 → success；全部失败 → error；部分成功 → warning
  - [x] 20.3: i18n keys: `applyPartialSuccess`/`projectConnection`/`globalConnection`

- [x] Task 21: useNetworkProfiles 双数组分离
  - [x] 21.1: `globalSshProfiles`/`projectSshProfiles` 等 6 个内部 ref
  - [x] 21.2: `sshProfiles`/`sslProfiles`/`proxyProfiles` → `computed` 合并
  - [x] 21.3: `loadByType` → 替换 `global*Profiles`
  - [x] 21.4: `loadByTypeProject` → 替换 `project*Profiles`（不再追加）

- [x] Task 22: useDriverRegistry 防重加载
  - [x] 22.1: `loadAll()` 开头加 `if (fetched.value) return`

- [x] Task 23: P2 细节修复
  - [x] 23.1: `buildConnectOpts` `password` 死代码清理
  - [x] 23.2: `isSslConfig`/`isProxyConfig` 类型守卫互斥检查

## Round 15 — 死代码清除 + Bug 修复 (2026-06-11)

- [x] Task 24: 死代码清除
  - [x] 24.1: 删除 `networkConfigStore.ts` — 完整 Pinia Store 零外部引用，与 useNetworkProfiles 功能重叠
  - [x] 24.2: 删除 `project_query_network_config` — `#[allow(dead_code)]` 包装函数，调用方全走 `_with_auth` 版本
  - [x] 24.3: 删除 `useNetworkChain.initProfiles()` — `@deprecated` no-op，零调用方

- [x] Task 25: Bug 修复
  - [x] 25.1: `AddDataSourceDialog.vue` watch 回调添加 `async` — 修复 TS1308 编译错误

## Round 16 — 代码复用消除双写 (2026-06-12)

- [x] Task 26: 抽取 ConnectionInfoResponse::from_info 消除三处重复构造
  - [x] 26.1: 新增 `ConnectionInfoResponse::from_info(info, is_active)` 关联函数
  - [x] 26.2: `get_connections` 使用 `from_info`
  - [x] 26.3: `get_active_connection` 使用 `from_info`
  - [x] 26.4: `detect_global_connections_in_project` 使用 `from_info`
  - [x] 26.5: `cargo check` 通过

- [x] Task 27: 抽取 ConnectDatabaseInput::into_connect_request 消除双写
  - [x] 27.1: 新增 `ConnectDatabaseInput::into_connect_request(connection_type, network_method, skip_persistence)` 方法
  - [x] 27.2: `connect_database` 使用 `into_connect_request`（23 行 → 1 行）
  - [x] 27.3: `test_connection` 重构为 `ConnectDatabaseInput` + `into_connect_request`（23 行 → 20 行）
  - [x] 27.4: 修复 `input` move 后借用问题（提前提取 `safe_url`/`db_type`/`conn_name`）
  - [x] 27.5: `cargo check` 通过

- [x] Task 28: 清理 useAddDataSource.ts 协议链死代码
  - [x] 28.1: 删除 `ProtocolType` 类型定义
  - [x] 28.2: 删除 `ChainHopItem` 接口定义
  - [x] 28.3: 删除 `countNetworkHops`/`ensureSslAtEnd`/`addHop`/`removeHop`/`onDrop`/`toggleHop` 函数（80+ 行）
  - [x] 28.4: 移除 `protocolChain` ref 声明
  - [x] 28.5: 移除 return 块中协议链相关导出
  - [x] 28.6: 移除 `AddDataSourceDialog.vue` 中 `protocolChain` 解构引用

## Round 17 — 可用性深度审计修复 (2026-06-12)

- [x] Task 29: P0 密码链路修复
  - [x] 29.1: `connection.ts` `connectDatabase` 服务 `opts` 新增 `password?: string` 参数
  - [x] 29.2: `ConnectDatabaseInput` 构造新增 `password: opts?.password ?? null` 映射
  - [x] 29.3: `AddDataSourceDialog.vue` `buildConnectOpts` 新增 `currentPassword` 参数
  - [x] 29.4: `handleCreateApply` 在 `syncCurrentToStaging()` 前捕获 `formData.value.password`
  - [x] 29.5: `saveToProjectOnly` 接受 `currentPassword` 并传递至 `createConnection`/`buildConnectOpts`
  - [x] 29.6: `saveToProject` 接受 `currentPassword` 并传递至 `createConnection`/`buildConnectOpts`
  - [x] 29.7: `handleCreateApply` 循环体三处调用点传递 `currentPassword`
  - [x] 29.8: ESLint + cargo check 通过

- [x] Task 30: P1 表单与 URL 健壮性修复
  - [x] 30.1: `stagingDirty` watch 数组扩展至 13 个源（补齐 driverPropertiesExtra/advancedOptions/schemaName/options/metadataPath/tags/useDuckdbFed）
  - [x] 30.2: `GeneralTab` 抽取 `syncFromFormData()` 函数，新增 `watch(props.formData, ...)` 双向同步
  - [x] 30.3: `useUrlBuilder.parseUrl` 正则新增 IPv6 分支 `(?:\[([^\]]+)\]\|([^:/]+))`
  - [x] 30.4: `useUrlBuilder.buildUrl` 对 `username`/`password` 使用 `encodeURIComponent()`
  - [x] 30.5: `useUrlBuilder.applyTemplate` 对 `{username}`/`{password}` 使用 `encodeURIComponent()`
  - [x] 30.6: ESLint + cargo check 通过

- [x] Task 31: P2 编辑与保存流程优化
  - [x] 31.1: `handleEditApply` 项目/全局更新路径 `password` 改为 `fd.password ? String(fd.password) : undefined`（不传空字符串）
  - [x] 31.2: `saveToStaging` 新增 `buildUrl()` 空值检查，失败时 `message.warning`
  - [x] 31.3: ESLint + cargo check 通过

## Round 18 — 测试连接密码链路补充 (2026-06-12)

- [x] Task 32: 后端 test_connection 命令新增 password 参数
  - [x] 32.1: `test_connection` 函数签名新增 `password: Option<String>` 参数
  - [x] 32.2: `ConnectDatabaseInput` 中 `password` 从硬编码 `None` 改为参数透传
  - [x] 32.3: `cargo check` 通过

- [x] Task 33: 前端 handleTest 传递 password 字段
  - [x] 33.1: 从 `formData.value.password` 读取密码
  - [x] 33.2: 加入 `invoke('test_connection', params)` 的 `params` 对象
  - [x] 33.3: ESLint 通过

- [x] Task 34: P0 specta 绑定同步（Round 20）
  - [x] 34.1: bindings.ts 中 `testConnection` 新增 `projectPath`/`password` 参数
  - [x] 34.2: connection.ts `testConnection` service 函数同步更新
  - [x] 34.3: ESLint 通过

- [x] Task 35: P1 批量应用进度条（Round 20）
  - [x] 35.1: 新增 `applyProgress` ref 状态
  - [x] 35.2: `handleCreateApply` 循环中更新 `applyProgress`
  - [x] 35.3: 模板 footer 新增进度文本 span
  - [x] 35.4: CSS `.apply-progress` 样式
  - [x] 35.5: `finally` 块重置 `applyProgress = null`

- [x] Task 36: P2 对话框响应式宽度（Round 20）
  - [x] 36.1: `.datasource-card` width 改为 `min(980px, calc(100vw - 48px))`

- [x] Task 37: P2 驱动切换前确认（Round 20）
  - [x] 37.1: `onDriverChange` 拆分为 `onDriverChange` + `doDriverChange`
  - [x] 37.2: 表单有数据时 `dialog.warning` 确认弹窗
  - [x] 37.3: 确认后调用 `doDriverChange`，取消则返回

- [x] Task 38: P2 认证方式切换保留 username（Round 20）
  - [x] 38.1: useAuthConfig.ts 新增 `isAuthMethodChanging` ref
  - [x] 38.2: `onAuthMethodChange` 设置标志 + `nextTick` 重置
  - [x] 38.3: `onAuthConfigSelect` 检测标志，跳过清空

- [x] Task 39: P2 loadAll 缓存优化（Round 20）
  - [x] 39.1: useDriverRegistry.ts 新增 `lastProjectPath` 变量
  - [x] 39.2: `loadAll` 缓存命中逻辑改为 `fetched && lastProjectPath === projectPath`
  - [x] 39.3: 项目切换时自动失效缓存

- [x] Task 40: 编译验证（Round 20）
  - [x] 40.1: ESLint 0 errors
  - [x] 40.2: cargo check 0 errors

- [x] Task 41: 安全审计 — 敏感数据泄露修复（Round 21）
  - [x] 41.1: test_connection 超时日志脱敏（mask_password_in_url）
  - [x] 41.2: test_connection 超时错误消息脱敏
  - [x] 41.3: ConnectionInfoResponse::from_info URL 脱敏
  - [x] 41.4: cargo check 通过

- [x] Task 42: 安全审计 — 认证缺陷修复（Round 21）
  - [x] 42.1: load_auth_data_from_db_for_network 增加项目 DB 回退查询
  - [x] 42.2: cargo check 通过

- [x] Task 43: 安全审计 — 文档更新（Round 21）
  - [x] 43.1: checklist.md 新增 Round 21 安全审计 7 项发现
  - [x] 43.2: 豁免项说明（SQL注入/XSS/硬编码密钥/命令注入/RBAC）

- [x] Task 44: 扩展安全审计 — 编译阻断 + 日志泄露修复（Round 22）
  - [x] 44.1: `resolve_network_method_with_project` 内 4 处 `parse_network_config_json` 调用补全 `project_path` 参数
  - [x] 44.2: `test_connection` 创建临时连接日志脱敏（mask_password_in_url）
  - [x] 44.3: `test_connection_config` 日志脱敏（mask_password_in_url）
  - [x] 44.4: cargo check 通过
