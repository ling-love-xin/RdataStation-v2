# 数据源连接模块 · 开发方案（Phase A/B/C）

> 状态：**Phase A/B + 运行时主链路修复已实现（2026-09-10，`cargo check -p rds-workbench -p rds-app -p rds-connection --all-targets` 零告警；连接模块测试全绿）** · Phase C 生态收尾待排期 · 关联文件：`connection-prototype-design.md`（原型）、`connection-dialog-prototype.html`（可交互原型）
> 运行时修复记录（2026-09-10）：① 应用启动补齐 `initialize_global_system()`（`crates/app/src/main.rs`，全局库单例 + 常驻运行时）；② 路径统一——`workspace_loader` 复用单例与 `RdataStation/system` 目录，消除“保存写 A 库、列表读 B 库”的分裂；③ DuckDB Secret 落地到持久分析库（`analytics.duckdb`）并补齐删除联动与 URL 百分号解码；④ 服务层新增同名连接拦截（`INSERT OR REPLACE` 静默覆盖防护）与项目路径预检（消除半成品落库）；⑤ 对话框测试连接回显服务器版本；⑥ connection crate 清理占位死文件（commands / connection_view / connection_dialog / mod.rs）。
> 遗留（待布局调整完成后接续）：`panels.rs` 的导航树 / SQL 执行仍拼 `global.duckdb`（应改用 `workspace_loader::global_analysis_db_path()`）；`ConnectionItem.connected` 仍承载记录有效性（is_active），运行时连接状态待连接服务接入；C1–C4 未启动。
> 缺口补齐记录（2026-09-10）：① 环境策略 CRUD（环境管理器内嵌策略面板）与高级 Tab 策略覆盖落库（`advanced_options.policy_overrides`）；② 连接编辑回读（侧边栏「编辑」→ 全 Tab 预填 → `update` 按 ID 前缀路由 G_/P_/GP_）；③ SSL/TLS 配置字段编辑与落库（`advanced_options.ssl`，常规→连接安全）；④ 双作用域 UI（仅全局/仅项目/全局+项目）与落库（P_ 走 `ProjectConnectionStore`、GP_ 走 `generate_gpid` 快照，项目路径来自对话框输入，未打开项目时提示）；⑤ DuckDB 缓存路径落库（`metadata_path`，模型/服务/对话框全链路打通）。
> 前置：v1 后端/前端实现为行为蓝本（`v1/backend/src/core/{services,persistence}`、`v1/frontend/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue`）；v2 engine 持久化层与 connection crate 传输层已完成迁移（见 §6 对齐表）

## 1. 现状结论（盘点摘要）

| 层 | 状态 |
| --- | --- |
| 持久化层（engine：global_db CRUD+update、auth/network/env/driver store、id_prefix、snapshot_service、migrations 008-014） | ✅ 已实现 |
| 传输层（connection crate：ConnectionConfig 协议链、5 连接器、TunnelGuard、factory、stream、known_hosts、DuckDB SecretManager） | ✅ 已实现 |
| 服务编排层（workbench `services/data_source_service.rs`；应用级 test_connection；模型 `connection/src/model.rs`） | ✅ 已实现（落点自 connection crate 调整至 workbench，见 A2 注） |
| UI 层（GPUI 对话框 `workbench/components/connection_dialog.rs`；列表/导航走 `workspace_loader`） | ✅ 已实现 |
| 启动装配（全局系统库单例初始化 + 常驻运行时） | ✅ 已补齐（`crates/app/src/main.rs`） |
| v1 蓝本（stores/services/AddDataSourceDialog.vue 53KB） | ✅ 已实现，可参照 |

**当前缺口**：Phase C（C1–C4）+ 运行时连接服务接入（ConnectionService → UI 连接动作/状态）。

## 2. 阶段划分

### Phase A — 核心闭环（目标：替换 5 字段手填表单）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| A1 | 模型：`DataSource` / `SaveInput` / `TestResult{success,message,latency_ms,version}`，字段对齐 v1 全局扩展 + `G_` 前缀 | `crates/connection/src/model.rs` | 编译通过；字段与 global_connections 列一一对应 |
| A2 | 服务：`DataSourceService`——类型/驱动查询（driver_store）、认证/网络/环境配置读取（stores）、`test_connection`（独立会话，复用 ConnectionFactory + 探测版本）、`save/update/delete`（global_db + AES 凭据 + Secret 联动） | `crates/connection/src/service.rs`（实际：`crates/workbench/src/services/data_source_service.rs`——connection 不得反向依赖 engine） | 集成测试：真实 PostgreSQL/DuckDB 新增→测试→保存→回读→更新→删除 |
| A3 | 对话框骨架：标题栏/左侧栏（搜索+暂存+类型树）/Header（名称+驱动类型+作用域+备注+URI 编辑）/常规 Tab（动态表单+认证+**连接安全 SSL**）/测试/保存/取消 | `crates/connection/src/connection_dialog.rs`（实际：`crates/workbench/src/components/connection_dialog.rs`） | UI 手动走通全流程；配置按 config_schema 渲染 |
| A4 | panels.rs 5 字段表单退役，改弹对话框；列表刷新走新服务 | `crates/workbench/src/panels.rs`、`workspace_loader.rs` | 空态/列表入口可弹框；保存后列表即时刷新 |
| A5 | `update_global_connection` 已有，补齐 `test_connection` 探测 SQL（按 driver） | `engine`（复用现有管道） | 返回 version/latency 正确 |
| A6 | 文档验收：`cargo check --workspace` 零告警；无 unwrap/expect；架构红线复核 | 全仓 | 全绿 |

### Phase B — 完整 Tab

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| B1 | 网络 Tab：协议链编辑（SSH/Proxy 添加/启用/上移/下移/删除）+ 拓扑预览（DB 带 TLS 徽标）+ 约束校验器（≤4 跳）｜✅ 已实现（上移/下移替代拖拽——0.6 无开箱拖拽组件） | `connection_dialog.rs` + `config.rs` 校验 | 协议链落库回读一致；非法链被拦截 |
| B2 | 高级 Tab：环境选择 + 策略摘要/覆盖 + DuckDB 加速卡片（仅网络库）｜✅ 已实现 | 对话框 + `env_store` | 环境引用正确落库；加速开关仅网络库可见 |
| B3 | 能力/驱动属性 Tab：capabilities 矩阵 + driver_properties key-value｜✅ 已实现 | 对话框 | 只读展示正确 |
| B4 | 认证配置管理器覆盖层（CRUD + 脱敏列表 + 类型分类 + 引用联动）｜✅ 已实现 | 对话框 + `auth_store` | 新建/编辑/删除闭环；列表无明文密码；选中引用后字段只读 |
| B5 | 网络配置管理器覆盖层（NetworkConfigManager：档案 CRUD + 引用联动）｜✅ 已实现 | 对话框 + `network_store` | 引用已保存协议链回读一致 |
| B6 | 环境管理器覆盖层（EnvironmentManager：环境 + 策略 CRUD）｜✅ 已实现（策略面板：环境行「策略」→ 类型/启用开关/删除，落库 `env_store` policy） | 对话框 + `env_store` | 环境/策略引用正确 |

### Phase C — 生态收尾

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | introspection 触发 → 数据库导航树衔接 | `database` crate + `global_metadata/<id>` 缓存 | 导航树可用 |
| C2 | 双作用域连接：仅全局（G_）/仅项目（P_）/全局+项目（G_ 定义 + GP_ 共享快照）｜⚠️ 2026-09-10 已实现 UI + 服务层落库（`ProjectConnectionStore::create/update/delete`、`generate_pid/generate_gpid`、GP_ 引用共享）；待接入「当前项目会话」以消除手动填项目路径 | 对话框 + `project_connection_store` + `snapshot_service` | 三种组合落库与回读正确；项目可见性规则测试 |
| C3 | 遗留 workbench `connection_service.rs`（85KB）收敛：调用点逐个迁入 connection crate | `workbench` | 遗留服务仅剩薄适配或归零 |
| C4 | 连接模板导入导出（无密码）+ 暂存撤销/重做 | 对话框 | 模板不含明文凭据 |

## 3. 测试场景清单（参照 v1 §10.4，Phase A 覆盖前 6 项）

1. 新建连接（网络库）：类型选择 → 表单 → 测试成功（版本+延迟）→ 保存 → 列表出现 → 回读字段一致
2. 新建连接（文件库：SQLite/DuckDB）：文件选择、网络 Tab 隐藏、保存成功
3. 测试失败：错误主机/密码 → 红色错误信息，不落库
4. 更新连接：改端口/密码 → 保存 → 回读更新值；凭据仍加密
5. 删除连接：列表移除 + Secret 清理（若开启联邦）
6. 作用域三态：仅全局→G_；仅项目→P_；全局+项目→G_ 定义 + GP_ 共享快照；Header 提示行实时更新（Phase A 支持选择与提示，Phase C 全量落库）
7. 认证配置：引用已保存配置 → 字段只读；管理覆盖层新建/编辑/删除走 AES 加密（Phase B）
8. 协议链：SSH→Proxy 保存回读；>4 跳/非法 hop 被拦截；SSL 模式在常规→连接安全配置并可回读（Phase A/B）
9. 环境：切换环境 → 策略摘要变化；覆盖标记"已覆盖"；环境管理器 CRUD（Phase B）
10. DuckDB 加速：开关开启 → Secret 注册；删除连接 → Secret 移除
11. URI 预览：字段变更实时更新
12. 暂存：关闭对话框 → 重开恢复未保存配置
13. 明暗主题切换：对话框各区域 token 正确（Light/Dark 手动核对）

## 4. 风险与对策

| 风险 | 对策 |
| --- | --- |
| workbench `connection_service.rs`（85KB）与 engine 连接管道职责重叠 | Phase A 并行、Phase C 收敛，避免一次性大搬迁 |
| 测试连接污染正式连接状态 | 独立测试会话（新建连接对象，不注册进连接池/管理器） |
| 凭据泄露（对话框/导出/日志） | 全程 auth_store AES 加密；列表脱敏；模板导出无密码 |
| 动态表单与 config_schema 格式漂移 | 复用 v1 `parseConfigSchema` 兼容逻辑（自定义 fields 与 JSON Schema 双格式） |

## 5. 与 v1 实现对齐表

| v1 设计/实现 | v2 现状 | 本方案 |
| --- | --- | --- |
| `DATA-SOURCE-MODULE.md` 表设计/ID 前缀/快照 | engine 已迁移（migrations 008-014） | 直接对接，不重建 |
| `auth_configs` AES-256-GCM | `auth_store` 已实现 | 认证 Tab 对接 |
| `network_configs` 协议链 + CONNECTION-METHOD-DESIGN | `network_store` + connection crate 传输层 | 网络 Tab 对接 + 校验器 |
| `environments` + 5 类策略 | `env_store` 已实现 | 高级 Tab 对接 |
| `AddDataSourceDialog.vue`（V3 五 Tab） | GPUI 占位 | 按 v5 布局重写 |
| 40 个 data_source_commands | 已退役/迁移（Tauri→GPUI 命令） | 对话框直接调 store/service |

## 6. 验证方式

- 每阶段：`cargo check -p rds-workbench -p rds-app -p rds-connection --all-targets` 零告警 + 对应集成测试（`crates/connection/tests/`、`crates/workbench/tests/real_connections.rs`）
- UI：`cargo run -p rds-app` 手动走通 §3 场景清单
- 主题：明暗切换核对 token（theme-preview.html 色卡为基准）

## 7. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| 启动初始化全局系统库（单例 + 常驻运行时） | `crates/app/src/main.rs` |
| 全局库 / 分析库路径定义 | `crates/engine/src/migration/global_init.rs` |
| 连接加载（单例优先，路径注入降级） | `crates/workbench/src/services/workspace_loader.rs` |
| 连接 CRUD / 测试 / 同名检查 / 项目预检 | `crates/workbench/src/services/data_source_service.rs` |
| DuckDB Secret 注册 / 注销 / URL 解码 | `crates/workbench/src/services/secret_integration.rs`、`crates/connection/src/secret.rs` |
| 连接对话框（五 Tab / 作用域 / SSL / 编辑回读） | `crates/workbench/src/components/connection_dialog.rs` |
| 领域模型（DataSource / Scope / TestResult） | `crates/connection/src/model.rs` |
| 传输层（协议链 / 连接器 / 流 / known_hosts） | `crates/connection/src/{config,connector,factory,stream,known_hosts}.rs` |
