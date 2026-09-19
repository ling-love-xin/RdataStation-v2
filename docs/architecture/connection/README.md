# 数据源连接模块（M3）· 模块入口

> **宣传页（一页看懂）**：[`connection-showcase.html`](connection-showcase.html)（视觉版，明暗双主题、离线可开，首屏可点着切四种数据库类型看表单 / Tab 联动）· [`connection-showcase.md`](connection-showcase.md)（可贴版，适合贴进 PR / wiki）
>
> **一句话**：把「一个数据源该怎么连」这件事做完整、做诚实——从选类型/驱动、填连接信息、引用凭据与网络档案，到测试连接、落库、运行时连接，全程一个对话框 + 一个服务层。
>
> 本文只提炼**特点 / 边界 / 硬约束 / 地图**；细节一律指向下方五份文档（本目录内），**不复制设计**。
> 状态：M3 主线完成（2026-09-13）。已知缺口与排期**唯一权威**是架构文档 §14。

## 1. 模块特点

### 产品行为

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **一个对话框管完** | 类型/驱动两层选择 + 五 Tab（常规 / 网络 / 能力 / 驱动属性 / 高级）+ 暂存列表 + 三个管理器覆盖层（认证 / 网络 / 环境），保存后不关闭、连续配置多条 | 架构 §2、原型 §2–§4 |
| **测试连接 = 真实连接的预演** | 唯一组装点 `build_probe_config`：同一套凭据注入 + **真实建隧道**（测完即关）+ 驱动属性/高级选项；差异只剩“不注册连接池 / 不落库” | 架构 §6 #74 |
| **严格模式，不静默降级** | 引用的认证 / 网络档案缺失、解析不出连接方式 → 测试与真实连接**双双报错**（绝不回退“直连 + 无凭据”） | 架构 §6 #75、§11 |
| **反馈不静默** | 同名拦截、项目根无效、文件新建失败、分组未落库（warning 级）、测试进度…全部落到结果行；只有标签同步等次要路径保留日志告警 | 架构 §11 |
| **结果行分级 + 详情/复制** | 成功/警告/错误/提示四级配色；长消息（或带详情）出现「详情 / 复制」，展开可滚动；复制内容 = 服务层原文 | 架构 §6 #82 |
| **暂存区只放未保存草稿** | 已保存连接统一从导航栏进入编辑；草稿跨会话持久化（**无密码列**） | 架构 §6 #80 |
| **首次引导是空态、不是常驻** | 仅“未选类型 + 名称/地址都空”时出现五步引导条，选完类型自动消失 | 架构 §6 #90 |

### 数据与安全

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **零 UI 造数据** | 每个 UI 数据项都有表/服务来源；UI 只做「键 → 显示名」字典映射，不产生业务取值（能力矩阵读 `drivers.capabilities`、策略读 `environment_policies`、字段读 `drivers.config_schema`） | 架构 §15（含逐项来源表） |
| **凭据全程加密** | `auth_configs.auth_data` 与 `network_configs.config` 内密码 / 口令一律 AES-256-GCM（`AES:` 前缀幂等），读路径解密、列表脱敏、存量明文启动迁移（全局库 + 项目库）、绕过的直查 SQL 也补解密 | 架构 §6 #79、§12 |
| **作用域双轨 + 单一判定** | `G_` 全局 / `P_` 项目 / `GP_` 快照（独立副本，显式同步）；前缀判定收归 `id_prefix`（服务层与导航侧**同一来源**） | 架构 §3.2、#68 |
| **连接 ID 内部化（B 案）** | 界面与提示词只出现**名称**（列表带 `P`/`G`/`GP` 来源短码）；完整 ID 在结果行「详情」里按需可查；**编辑改名不换 ID**，新建重名被拦 | 架构 §3.2、#93 |
| **读路径零副作用** | 项目根预检读写分离：写路径报错、读路径降级为空，**绝不**因一次回读在磁盘上造出 `.RSmeta` 骨架 | 架构 §15.4 |
| **缓存身份指纹（规则已冻结）** | 同一物理库的多条连接共享 L2：身份 = 数据库族 + 规范化地址 + principal + 格式版本；驱动实现 / 密码 / 连接参数**不进身份**；指纹只作缓存键、**不作主键** | 架构 §3.6、#63–#66 |

### 架构与约束

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **依赖只向下** | 视图只依赖 `gpui-kit`；`connection` crate 不吃 engine；传输/协议层（URL 处理、隧道、协议链）已全部下沉到 `crates/connection`，workbench 只留编排 | 架构 §2.1 |
| **状态所有权清晰** | 对话框状态挂 `EditorPanel`（`Rc<ConnectionDialogState>`）；副作用（订阅 / 宿主重绘）一律在面板入口，`render` 是模式同步的权威点 | 架构 §4、#30 |
| **对话框层挂载机制** | `window.open_dialog` + 宿主 `Root::render_dialog_layer`；`cx.notify` 只重渲染子树 → 打开/关闭后显式 `notify_host` | 架构 §4.2 |
| **渲染热路径收敛** | 驱动派生数据按“驱动 id + 声明原文”缓存、地址占位“变化才写”（`set_placeholder` 是无条件 notify）、暂存列表用 `LiveEntryView` 逐行短借用 | 架构 #67、#73 |
| **组件化 + 尺寸契约** | Tab 条 / 分段控件 / 开关一律 gpui-kit 组件（`TabBar::underline` / `segmented` / `Switch`）；模块内**无裸 `px(...)`**，尺寸登记在 `crates/workbench_shell/src/ui.rs`，由 `ui_contract` 契约测试扫描 | 架构 #84–#88 |
| **单一写入口** | 结果行 `set_result{,_ok,_line}`、标签 `overlay_authoritative_tags`（权威表优先）、分组/标签同步收在服务层 | 架构 #82、#89 |

### 工程与文档

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **六文件拆分** | `connection_dialog/{mod,state,staging,render,managers,helpers}.rs` + `project_picker.rs`，每个文件职责单一（状态 / 快照 / 渲染 / 覆盖层 / 纯函数） | 架构 §2.2 |
| **纯函数优先** | 可测的判定都拍成纯函数：`driver_form_fields` / `build_{auth,network}_config_json` / `strip_file_db_noise` / `dialog_tab_defs` / `visible_tab_index` / `saved_result` / `create_new_db_file` / `metadata_identity::*` | 架构 §7 |
| **测试分层** | 传输单测（`crates/connection`）→ 服务集成（`data_source_lifecycle` 等）→ 窗口测试（`#[gpui_kit::test]` headless）→ 契约（`ui_contract`）；测试落临时目录、不触真实库 | 架构 §7 |
| **真机反馈成文** | 每轮 USIT 的问题 → 决策编号 → 文档 + 测试同步；`§14` 是唯一待办清单，`dev-plan` 顶部是逐轮记录 | 架构 §6 / §14、dev-plan |
| **原型非权威** | 交互稿顶部标注“示意稿（非权威）+ 同步戳（决策号）”，权威是 `connection-prototype-design.md` | 架构 #10 |

## 2. 边界

- **做**：数据源连接的**新增 / 编辑 / 删除 / 测试**、作用域路由、凭据与档案引用、DuckDB Secret 加速、运行时连接与隧道、暂存草稿、连接的组织元数据（标签 / 分组勾选）。
- **不做**：数据源**导航树 / 分组管理 / 标签检索视图**（属 database-nav，见 `../database/database-nav-dev-plan.md`）；驱动**安装 / 插件**（平台）；模板**导入导出 UI**（能力与测试已就绪，入口按决策暂缓）。

## 3. 代码地图（要改什么去哪）

| 想改 | 去哪 |
| --- | --- |
| 对话框 UI / 布局 / 交互 | `crates/workbench/src/components/connection_dialog/render.rs`（+ `helpers.rs` 纯函数、`ui.rs` 尺寸常量） |
| 暂存列表 / 草稿快照 | `connection_dialog/staging.rs`（`ConnectionDraft` + `staging_*`） |
| 三个管理器覆盖层 | `connection_dialog/managers.rs` |
| 项目下拉 | `connection_dialog/project_picker.rs`（+ `Shared::{project_new_request, project_open_request}` → `take_project_action_request`） |
| 落库 / 回读 / 测试连接 / 作用域路由 | `crates/workbench/src/services/data_source_service.rs` |
| 连接组装与隧道 | `crates/workbench/src/services/connection_service.rs` + `crates/connection/src/{url,url_params,chain}.rs` |
| **网络配置（SSH / 代理 / SOCKS / SSL）**：模型 | `crates/connection/src/config.rs`（`SshConfig` / `SshAuth` / `ProxyConfig` / `SslConfig` / `ChainHop` / `ConnectionMethod`） |
| **网络配置**：真实拨号与隧道 | `crates/connection/src/connector.rs`（`SshTunnelConnector` 基于 **russh** 做端口转发、`SslConnector`、`TunnelGuard`）、`stream.rs`（隧道流）、`known_hosts.rs`（主机指纹校验）、`chain.rs`（`apply_network_method` 单跳 + `process_chain` 多跳 + `TunnelRegistry` 守卫表） |
| **网络配置**：落库 | `crates/engine/src/persistence/network_store.rs`（结构化字段组装 / 回填 + 凭据 AES 加密 / 脱敏 / 存量迁移） |
| **网络配置**：UI 入口 | 「网络」Tab（引用下拉 + 「管理网络配置」）→ `connection_dialog/managers.rs`（类型 + 字段表单）+ `helpers.rs::{network_field_specs, network_config_values}`（按类型展开字段）；**内联协议链编辑器已撤下**（决策 #72，多跳走 `chain` 档案） |
| 表结构与迁移 | `crates/engine/src/{migrations/global,persistence}/*`（`global_connections` / `connection_drafts` / `auth_store` / `network_store` / `connection_org_store` / `metadata_identity`） |
| ID 规则与作用域判定 | `crates/engine/src/persistence/id_prefix.rs` |

数据链路：`UI（对话框 / 导航）→ DataSourceService（唯一的服务层入口）→ engine 持久化层 → 本地 SQLite / DuckDB`。**无 HTTP / IPC 层**——“前后端联通”在本模块即“UI 只经服务层读写本地库”。

## 4. 改这个模块前必须遵守

1. 视图只依赖 `gpui-kit`；颜色一律 `theme.colors.*`，尺寸走 `ui.rs` / Tailwind 尺度（`px(` 会被契约测试拦下，跨模块规则见 `../theme/ui-constraints.md`）。
2. 新 UI 数据项先回答“来自哪张表 / 哪个服务方法”；只能来自字典的（如能力键 → 中文）不得携带取值。
3. 副作用（订阅、宿主重绘、模式切换）不放点击回调里直接做：回调只更新状态 + `notify()`，`render` 是权威同步点。
4. 服务层写入前做项目根预检；**读路径不得建目录 / 建表**。
5. 凭据一律走加密写路径；列表接口默认脱敏；错误消息里不出现 ID（只给名称 + 可操作信息）。
6. 注释与文档用简体中文，说明意图与取舍（不复述代码）。
7. `cargo` 命令固定 `-j 2`（并发链接重型 crate 会 OOM（DuckDB 已改动态链接））。
8. 窗口测试：`#[gpui_kit::test]`、**禁用通配导入**（`use gpui_kit::*` / `use super::*` 会与 `#[test]` 宏自相残杀）；断言“节点真的渲染”必须 `.debug_selector(...)` + `cx.debug_bounds(...)`（`.id(...)` 不登记）。

## 5. 测试与验证

```sh
# 模块回归（连接组织存储 + 工作台 lib + 连接相关套件）
cargo test -p rds-engine --lib connection_org_store -j 2
cargo test -p rds-workbench --lib \
  --test data_source_lifecycle --test connection_dialog_ui --test connection_staging \
  --test connection_multi_save --test connection_drafts_persist --test connection_type_driver \
  --test connection_project_picker --test dialog_host_layer --test connection_tunnel_cleanup \
  --test connection_scope_and_state --test real_connections --test global_service_singleton \
  --test connection_template --test ui_contract \
  --test connection_render_matrix --test connection_edit_backfill -j 2

# 全工作区编译守卫（含全部 target）
cargo check --workspace --all-targets -j 2
```

- 基准（2026-09-17）：**18 个目标 / 155 用例全绿**——lib **44**（2026-09-19 实测；该「18 目标 / 155 用例」是跨 crate 组合口径，逐 crate 明细见 `../module-status.md` §2）、`data_source_lifecycle` 28、`connection_type_driver` 7、`connection_project_picker` 7、`ui_contract` 7、`connection_staging` 6、`real_connections` 5、`connection_dialog_ui` 4、`dialog_host_layer` 4、`connection_multi_save` 3、`connection_render_matrix` 2、`connection_scope_and_state` 2、`connection_template` 2、`global_service_singleton` 2、`db_navigator` 2、`connection_edit_backfill` / `connection_drafts_persist` / `connection_tunnel_cleanup` 各 1。
- **引擎侧存量迁移**（同一轮）：`cargo test -p rds-engine --lib` = **369 项全绿**——含新增的启动迁移接线测试（`migration::global_init::startup_migration_backfills_tags_for_global_and_project`）与项目打开回填（`persistence::project_db::opening_project_backfills_legacy_connection_tags`）。
- **两条补强套件**（2026-09-17）：`connection_render_matrix`（状态 × 渲染矩阵：引导条三态、五 Tab 降级渲染、作用域三态、结果行四级、**暂存区固定高度 + 两列等高**）与 `connection_edit_backfill`（编辑入口 → 读库 → 表单逐项回填 + 五 Tab 渲染；**本轮由此拖出“类型 / 驱动不回填”缺陷**）。
- **布局高度的写法约定**（本轮踩到，必守）：固定高度必须 `h + min_h + max_h` **三向显式**约束——只给 `h()`（哪怕再加 `min_h_0()`）夹不住 flex 子项的自动最小尺寸，内容多时会按内容撑高（实测暂存区 120px → 412px、侧栏 522px 撑高对话框）。
- 测试模块的硬规则：**禁** `use gpui_kit::*` / `use super::*`（`#[test]` 宏遮蔽）；断言“节点真的渲染”必须 `.debug_selector(...)` + `cx.debug_bounds(...)`（`.id(...)` **不**登记坐标）；宿主设置 `host_redraw` 桥时，面板入口要**从宿主外部**触发（在 `Harness::update` 内调会重入 panic）。
- **标签单一权威**（2026-09-17 收尾）：读取**只认** `connection_tags`；行内 `tags` JSON 降为写入侧投影；存量数据由**两处**一次性回填迁入（都幂等）——① 启动迁移 `initialize_global_system` → `migrate_legacy_data`（全局库 + 名册里已存在权威表的项目库）；② 打开项目、项目迁移建好表之后 `ProjectDatabaseManager::open` → `backfill_project_connection_tags`（否则升级后第一次打开项目看不到旧标签）；两处都不建表不建目录（决策 #95）。
- 契约测试 `ui_contract`：尺寸（禁裸 `px(`）+ 颜色（禁 `rgb(` / `hsla(`）扫描范围含本模块全部文件。
- 真机验收走 `connection-user-guide.md` §9 的 A–V 清单（USIT）。

## 6. 文档地图

| 文档 | 什么时候读它 |
| --- | --- |
| `connection-showcase.html` / `connection-showcase.md` | **宣传页（一页看懂）**：特点 / 流程 / 架构 / 质量证据一页扫完；改了实现请顺手核对页里的数字与「边界」段（两版章节一一对应） |
| `connection-user-guide.md` | **怎么用**：入口 / 导览 / 典型流程 / FAQ 排查 / USIT 清单 |
| `connection-prototype-design.md` | **长什么样**：布局 / 五 Tab / 交互 / 主题映射（视觉权威） |
| `connection-dialog-architecture.md` | **为什么这样设计 / 怎么运转**：概念模型 / 数据流 / 90+ 条决策 / 测试策略 / 数据字典 / 降级矩阵 / **§14 待办（权威）** / **§15 零造数据审计** |
| `connection-dev-plan.md` | **做到哪了**：Phase A/B/C 任务、逐轮实现记录（含踩过的坑） |
| `connection-dialog-prototype.html` | 可交互示意稿（非权威，页头带同步戳） |
| `../database/database-navigator-*.md` | 消费方：导航树 / 分组 / 标签视图（另一模块） |

## 7. 下一步（摘要，权威见架构 §14）

| 类别 | 项 |
| --- | --- |
| 模块内可做 | **（已清空）**——标签权威表回填迁移已于 2026-09-17 完成（决策 #89 收尾，兼容回退已删）；本模块内暂无可自主推进项 |
| 待拍板 | #36 保存结果行「详情 / 复制」是否算噪音（待 USIT）；#11 类型树折叠三选项（①维持现状〔推荐〕/②上折叠/③分类头吸顶） |
| 需协调 | 元数据缓存指纹接线（与导航接入 L2 同轮）、宿主开窗分支测试（需 `WorkbenchView` 可测试化） |
| M4 / 宿主侧 | 分组管理在导航侧、导航行删除入口、标签视图接线、打开·关闭项目刷新 |
| 待拍板 / 平台 | 驱动插件安装、其他模块尺寸迁移、图像回归与性能基准 |
| **beta2** | **连接主键改 ULID + 存量迁移（原 C 案）**：前缀只留作用域标记，`uid` 作真主键 / 外键锚点 |
