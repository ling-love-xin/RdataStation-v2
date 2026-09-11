# 数据源连接模块 · 开发方案（Phase A/B/C）

> 状态：**Phase A/B + 运行时主链路修复 + 测试体系（单测 / 窗口测试 / 全局联动）已实现（2026-09-11）** · Phase C：C1 ✅（由 database-nav 承接）/ C2 ✅ / C3 ✅（传输层全部下沉）/ C4 暂缓 · 关联文件：`connection-prototype-design.md`（原型）、`connection-dialog-prototype.html`（可交互原型）
> 测试补强与修复记录（2026-09-11）：① 服务层单测 + 全局联动（`crates/workbench/tests/data_source_lifecycle.rs`：保存/回读/更新/删除/同名拦截/作用域预检/测试连接 + 服务写入→加载器读回）；② 窗口级测试（`crates/workbench/tests/connection_dialog_ui.rs`：打开/渲染/关闭、五 Tab 逐个渲染、编辑入口、状态保留、重入不叠加）；③ Secret 持久化修正——`CREATE PERSISTENT SECRET`（默认 `CREATE SECRET` 为会话级会随连接丢失）+ `SET secret_directory` 指向应用可控目录 `{system}/secrets`（不再写用户主目录 `~/.duckdb`），名称净化统一小写（DuckDB 标识符折叠）；④ Secret 注册按 `use_duckdb_fed` 门控（未开启加速不注册，关闭时清理）；⑤ 对话框 `open` 幂等重入；⑥ 可测性注入点：`DataSourceService::with_analysis_db`、`secret_integration::*_at`；⑦ **项目库初始化修复**——DuckDB 迁移从 `MigrationManager`（rusqlite，无法打开 DuckDB 文件且二次 open 触发文件锁冲突）拆到 `migration/duckdb.rs`，复用已打开连接执行（global / project 两处共用），双作用域端到端用例已启用。
> Phase C 推进记录（2026-09-11）：⑧ C2 当前项目会话接入——对话框打开时自动预填项目根（已填值不覆盖）、删除路由携带项目路径（`workspace_loader::delete_connection(conn_id, project_path)`），P_/GP_ 连接在项目打开时可正常删除；⑨ 全局库单例可注入（`engine::migration::install_global_db_manager`）+ 单例生产路径集成测试（`crates/workbench/tests/global_service_singleton.rs`：`DataSourceService::global()` 与 `load_persisted_connections()` 单例分支、重复注入拒绝）；⑩ 遗留路径收尾——编辑区「DuckDB 分析库」表树与 SQL 执行（非数据源导航栏）改走 `workspace_loader::global_analysis_db_path()`（不再拼旧文件名 `global.duckdb`）；⑪ **连接运行态**——`ConnectionItem.connected` 不再冒充记录有效性，由连接管理器运行态填充（`workspace_loader::fill_connected`），列表/详情状态点反映真实连接与断开；⑫ **项目连接可见性**——`load_connections_for_scope(project_root)` 合并全局 + 项目侧 P_/GP_（未打开项目仅全局可见），面板删除刷新 / 对话框保存刷新 / 项目切换刷新三处调用点统一；⑬ **C1 状态更正**——introspection → 导航树已由 database-nav Phase A 承接（`database::MetadataService` 实时内省 + `navigator_service` 懒加载），无需重复建设；⑭ **C3 第一批收敛**——URL 处理族下沉 `crates/connection/src/url.rs`（`build_connection_url` 含 userinfo 百分号编码、`mask_password_in_url`、`extract_credentials_from_url`、`url_has_plaintext_password`），workbench / engine 三处重复实现统一（顺带修复 engine 版脱敏丢失用户名）。
> C3 第二批 + 组织元数据上提（2026-09-11）：⑮ **C3 第二批收敛**——URL 参数注入 / 改写 10 函数下沉 `crates/connection/src/url_params.rs`（`inject_auth_into_url` / `inject_username_password` / `inject_ssl_cert` / `inject_kerberos` / `inject_chain_ssl_params` / `rewrite_url_host_port` / `parse_host_port_from_url` / `append_ssl_params` / `append_url_params` / `matches_no_proxy`），workbench 侧删除本地实现并补 8 项单测；⑯ **分组/标签服务上提**（连接组织元数据归连接域）——`engine::persistence::ConnectionOrgStore` 统一分组（项目级多对多 + 排序）与标签（多值 + 检索 + 统计）读写，取代 `nav_store` 原标签实现（`nav_store` 仅保留导航视图状态）；`DataSourceService` 保存/更新时同步 tags 到权威检索表、删除时一致性清理标签与分组成员（避免孤儿），`nav_runtime` 标签读写改走新存储。
> C3 第三批（2026-09-11）：⑰ **协议链执行 + 隧道生命周期下沉**——`crates/connection/src/chain.rs` 承接 `TunnelRegistry`（按连接 ID 持有隧道守卫，`insert` 覆盖释放旧隧道 / `take` / `count` / `clear`，Clone 共享同一张表）与协议链执行（`apply_network_method` 单跳分发 + `process_chain` 多跳迭代 + SSH/代理隧道端口工厂），URL 改写复用 `url_params` 族；workbench `connection_service.rs` 删除本地实现（`tunnels` 字段改为 `connection::chain::TunnelRegistry`，断开走 `take` 并在连接清理后释放，全关走 `clear`），新增 4 项单测（注册表生命周期 / Direct 原样 / SSL 仅注入参数 / 纯 SSL 链无隧道）。**传输/协议层至此全部下沉，C3 收敛完成**；剩余为依赖 engine 的会话生命周期编排（连接信息 / 持久化 / 元数据缓存 / 连接池），按依赖方向保留在 workbench。
> 集成测试与缺陷修复（2026-09-11）：⑱ **隧道数据面集成测试**——新增 `crates/connection/tests/tunnel_roundtrip.rs`（4 项端到端：SOCKS5 隧道真实数据往返且守卫释放后本地端口关闭、HTTP CONNECT 隧道往返、两跳代理链嵌套转发、no_proxy 命中不建隧道）与 `crates/workbench/tests/connection_tunnel_cleanup.rs`（协议链隧道建立后数据库握手失败 → 断言不残留，`tunnel_count == 0`）；⑲ **集成测试暴露并修复真实缺陷**——`ConnectionService::connect_with_type` 在 `create_database` / `add_connection` 失败时未回收已登记的隧道守卫（本地端口 + 后台 accept 循环泄漏），现统一走 `release_tunnels`，并新增 `tunnel_count` 诊断方法（测试可断言）。
> 运行时修复记录（2026-09-10）：① 应用启动补齐 `initialize_global_system()`（`crates/app/src/main.rs`，全局库单例 + 常驻运行时）；② 路径统一——`workspace_loader` 复用单例与 `RdataStation/system` 目录，消除“保存写 A 库、列表读 B 库”的分裂；③ DuckDB Secret 落地到持久分析库（`analytics.duckdb`）并补齐删除联动与 URL 百分号解码；④ 服务层新增同名连接拦截（`INSERT OR REPLACE` 静默覆盖防护）与项目路径预检（消除半成品落库）；⑤ 对话框测试连接回显服务器版本；⑥ connection crate 清理占位死文件（commands / connection_view / connection_dialog / mod.rs）。
> 模块边界：**本模块只负责“新增/管理数据源连接”本身**（连接对话框、CRUD、作用域路由、测试连接、DuckDB Secret、运行态连接与列表），不包含数据源导航树；导航树、来源短码 P/G/GP 展示、分组/标签/缓存管理均属 database-nav 模块（见 `docs/architecture/database/database-nav-dev-plan.md`）。
> 遗留（2026-09-11 更新）：C3 传输/协议层已全部下沉（`url.rs` / `url_params.rs` / `chain.rs`）且隧道数据面已有集成测试（含失败路径回滚），剩余编排层（会话生命周期、连接池、连接信息登记、持久化与元数据缓存）因依赖 engine 按依赖方向保留在 workbench `connection_service.rs`；C4（模板导入导出 + 暂存）按决策暂缓；数据库导航侧 Phase B/C（分组/标签视图、缓存管理、预热增量）见 `docs/architecture/database/database-nav-dev-plan.md`。
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
| C1 | introspection 触发 → 数据库导航树衔接 | `database` crate + `global_metadata/<id>` 缓存 | ✅ 2026-09-11 由 database-nav Phase A 承接（实时内省 + 懒加载 + 展开态持久化）；connection 侧无需重复建设 |
| C2 | 双作用域连接：仅全局（G_）/仅项目（P_）/全局+项目（G_ 定义 + GP_ 共享快照）｜✅ 2026-09-11 已接入当前项目会话（对话框项目根自动预填、删除路由携带项目路径、列表按作用域合并显示 P_/GP_）；来源短码展示归 database-nav（B8） | 对话框 + `project_connection_store` + `snapshot_service` | 三种组合落库与回读正确；可见性测试已覆盖（`connection_scope_and_state.rs`） |
| C3 | 遗留 workbench `connection_service.rs` 收敛：调用点逐个迁入 connection crate｜✅ 2026-09-11 传输/协议层全部下沉——第一批（URL 处理族 → `url.rs`）、第二批（URL 参数注入/改写 → `url_params.rs`）、第三批（协议链执行 + 隧道生命周期 → `chain.rs`）；workbench 仅保留依赖 engine 的会话生命周期编排（连接信息 / 持久化 / 元数据缓存 / 连接池），符合依赖方向 | `workbench` + `connection` | 遗留服务仅剩编排层；传输层单测全绿（`cargo test -p rds-connection --lib`） |
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

- 每阶段：`cargo check -p rds-workbench -p rds-connection --all-targets` 零告警
- 连接模块测试清单（不影响布局调试）：
  - `cargo test -p rds-connection --lib`（协议链执行 + 隧道注册表 / URL 处理族 / url_params / Secret / known_hosts / 模型）
  - `cargo test -p rds-engine --lib`（持久化与迁移层回归）
  - `cargo test -p rds-workbench --test data_source_lifecycle`（服务层单测 + 全局联动）
  - `cargo test -p rds-workbench --test connection_dialog_ui`（窗口级 headless 测试）
  - `cargo test -p rds-workbench --test real_connections`（加载器契约）
  - `cargo test -p rds-workbench --test global_service_singleton`（单例生产路径：`global()` / 列表单例分支 / 重复注入拒绝）
  - `cargo test -p rds-workbench --test connection_scope_and_state`（作用域可见性 P_/GP_ 合并 + 运行态 connected 填充）
  - `cargo test -p rds-connection --test tunnel_roundtrip`（隧道数据面：SOCKS5 / HTTP CONNECT / 两跳链真实转发 + 守卫释放关闭）
  - `cargo test -p rds-workbench --test connection_tunnel_cleanup`（连接失败路径隧道回收）
- **编译/测试统一加 `-j 2`**（或用 alias `cargo test-all`）：并发链接 DuckDB 静态库会耗尽内存，触发 rustc `STATUS_STACK_BUFFER_OVERRUN` 崩溃并拖慢宿主（Zed 卡顿）；`.cargo/config.toml` 已含 `RUST_MIN_STACK` 补偿与 alias 说明
- UI：`cargo run -p rds-app` 手动走通 §3 场景清单
- 主题：明暗切换核对 token（theme-preview.html 色卡为基准）

## 7. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| 启动初始化全局系统库（单例 + 常驻运行时） | `crates/app/src/main.rs` |
| 全局库单例注入（测试 / 嵌入） | `crates/engine/src/migration/global_init.rs`（`install_global_db_manager`） |
| 全局库 / 分析库路径定义 | `crates/engine/src/migration/global_init.rs` |
| 连接加载（单例优先，路径注入降级） | `crates/workbench/src/services/workspace_loader.rs` |
| DuckDB 迁移执行（复用已打开连接，global / project 共用） | `crates/engine/src/migration/duckdb.rs` |
| 连接 CRUD / 测试 / 同名检查 / 项目预检 | `crates/workbench/src/services/data_source_service.rs` |
| DuckDB Secret 注册 / 注销 / URL 解码 | `crates/workbench/src/services/secret_integration.rs`、`crates/connection/src/secret.rs` |
| URL 组装 / 脱敏 / 凭据提取（C3 收敛） | `crates/connection/src/url.rs` |
| URL 参数注入 / 改写（认证 / SSL / Kerberos / host:port / no_proxy） | `crates/connection/src/url_params.rs` |
| 协议链执行 / 隧道生命周期（`TunnelRegistry` / `apply_network_method` / `process_chain`） | `crates/connection/src/chain.rs` |
| 隧道数据面集成测试（SOCKS5 / HTTP CONNECT / 两跳链） | `crates/connection/tests/tunnel_roundtrip.rs` |
| 失败路径隧道回收测试（`tunnel_count` 断言） | `crates/workbench/tests/connection_tunnel_cleanup.rs` |
| 连接组织元数据（标签 / 分组，M3 与 M4 共用） | `crates/engine/src/persistence/connection_org_store.rs` |
| 作用域可见性 / 运行态状态填充 | `crates/workbench/src/services/workspace_loader.rs` |
| 连接对话框（五 Tab / 作用域 / SSL / 编辑回读） | `crates/workbench/src/components/connection_dialog.rs` |
| 领域模型（DataSource / Scope / TestResult） | `crates/connection/src/model.rs` |
| 传输层（协议链配置 / 连接器 / 流 / known_hosts） | `crates/connection/src/{config,connector,factory,stream,known_hosts}.rs` |
