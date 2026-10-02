# RdataStation v2・数据源连接模块（M3）完整规格书

> **用途**
>
> ：本文件是 connection 模块的自包含技术规格 —— 把代码、文档、测试、决策浓缩为一份
> 可独立复现的说明。目标读者：新开发者 / 另一 AI，期望据此生成相同结构、相同语义的代码与文档。
> 生成基准：仓库当前 HEAD 
>
> `b2e07859`
>
> （2026-10-02，保存语义重构已合入）。
> 一切以「代码为准」：本文件若与源码冲突，以源码为准；若与 
>
> `connection-dialog-architecture.md`
>
>  冲突，以架构文档为准。



***

## 1. 模块总览



| 项    | 内容                                                                                                 |
| ---- | -------------------------------------------------------------------------------------------------- |
| 一句话  | 把「一个数据源该怎么连」做完整：选类型 / 驱动 → 填连接信息（五 Tab）→ 引用凭据与网络档案 → 测试连接 → 暂存 / 批量提交 → 运行时连接与隧道，全程一个对话框 + 一个服务层   |
| 交付状态 | M3 主线完成（2026-09-13）；保存语义重构完成（2026-10-02，提交 `b2e07859`）                                             |
| 权威文档 | `docs/architecture/connection/connection-dialog-architecture.md`（§14 = 已知问题唯一权威）                   |
| 主要文件 | 见 §3 代码地图                                                                                          |
| 测试基线 | rds-connection 96 单测 + 6 集成；rds-workbench connection\_dialog lib 38 项；connection 集成 12 个测试文件全绿（§9） |

### 1.1 模块边界（做什么 / 不做什么）

**做**：连接配置的创建 / 编辑 / 保存（作用域三态）、草稿暂存与批量提交、测试连接（真实预演）、

连接列表、凭据加密存储、DuckDB Secret 注册、身份指纹驱动的元数据缓存共享。

**不做**：数据库查询执行（M4/M5 导航）、连接池管理之外的会话编排细节、SQL 编辑器（Monaco）。

## 2. 技术栈与依赖



| 项     | 内容                                                                                                                                                                          |
| ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 桌面框架  | gpui 2.x（自定义 Window/Dialog 宿主）；组件库 gpui-kit 0.7（TabBar/Switch/Input/Button/List）                                                                                            |
| 服务层   | `DataSourceService`（workbench）+ `connection` crate（传输 / 协议链 / URL）+ `engine` crate（持久化 / 驱动 / 缓存）                                                                           |
| 数据库引擎 | SQLite（`rusqlite`，全局 / 项目元数据）、DuckDB 1.5.5（分析引擎 + 联邦 Secret，`DUCKDB_LIB_DIR=third_party/duckdb/1.5.5`）、mysql/postgres 客户端库                                                  |
| 凭据加密  | AES-256-GCM，`AES:` 前缀幂等（读路径解密 / 列表脱敏 / 存量迁移）                                                                                                                                |
| 异步    | tokio runtime（服务层同步 `block_on` 风格，UI 线程用 `background_executor` + `spawn_in` 续体）                                                                                             |
| 构建约束  | `cargo build/test`**&#x20;必须&#x20;**`-j 2`（`-j 4` 触发 0xc0000409 + rust\_oom，Windows 已知）；`.cargo/config.toml` 已设 `RDS_HOME=.rds`、`DUCKDB_LIB_DIR`、`RUST_MIN_STACK=268435456` |
| 目标平台  | Windows（本机验证环境）；PowerShell 交互（Bash 不可用）                                                                                                                                     |

## 3. 代码地图

### 3.1 `crates/workbench/src/components/connection_dialog/`（对话框主体，8 文件）



| 文件                  | 职责                                                                                                                                                                                                                              |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`            | 对话框入口：`ConnectionDialog` 组件、`open_connection_dialog` 宿主 API、`saved_result`/`conn_display_name`（`#[allow(dead_code)]` 保留给 helpers 单测）                                                                                            |
| `state.rs`          | `ConnectionDialogState`：全部表单字段（gpui `Rc<Entity>` 模型）、五 Tab 状态、`collect()`（表单→`DataSourceSaveInput`）、`draft_to_save_input()`（草稿→`DataSourceSaveInput`）、`cloned_state`/`ClonedDialogState`（批量提交快照）、`#[cfg(test)]` 3 项单测           |
| `staging.rs`        | `ConnectionDraft` 草稿模型、`capture_draft`/`apply_draft`（表单↔草稿）、`staging_persist`/`staging_restore`/`staging_flush`（持久化）、`staging_clear`/`remove_drafts_at`（保存并退出的清空 / 部分失败）、`draft_dirty`（脏标记）、`templates_export/import`（模板，UI 暂缓） |
| `render.rs`         | 全部渲染 + 交互：Header / 五 Tab / 暂存列表 / 类型树 /footer 按钮；`do_stage`（保存 = 暂存）、`do_batch_commit`（保存并退出 = 批量提交）、`do_test`（测试连接）、`set_result`/`set_result_ok`（结果行分级）、`BatchOutcome`（\~2000-2272 行）                                          |
| `helpers.rs`        | `dialog_row_height`、`save_blocker`（缺项拦截）、`section_header`（w\_full 左对齐）、`strip_file_db_noise`、`create_new_db_file`（文件型按驱动分派初始化）                                                                                                  |
| `draft_list.rs`     | 暂存列表 UI：行渲染、脏标记 `●`、**同步指纹**（行数 + 光标 + 每条 saved\_id / 名称 + 光标位实时表单值，解决 "填名称列表不更新"）                                                                                                                                              |
| `type_tree.rs`      | 左侧类型目录 UI + 同步指纹（目录 / 过滤词 / 选中项）                                                                                                                                                                                                |
| `project_picker.rs` | 项目下拉（仅项目 / 全局 + 项目作用域）                                                                                                                                                                                                          |

### 3.2 `crates/connection/`（传输层 crate，11 模块）



| 模块               | 职责                                                                  |
| ---------------- | ------------------------------------------------------------------- |
| `chain.rs`       | 协议链（SSH 隧道 → 直连）执行与生命周期，隧道建完即用即关                                    |
| `connector.rs`   | `establish_http_proxy`（状态行解析判定 200，不做子串 contains）、`wrap_tls_stream` |
| `url.rs`         | URL 解析族（模板 / 占位 / 规范化）                                              |
| `url_params.rs`  | URL 参数注入 / 改写（驱动属性、连接参数）                                            |
| `model.rs`       | 连接结果模型（`{success, message, latency_ms, version}`）                   |
| `config.rs`      | 连接配置（probe / 真实共用 `build_probe_config`）                             |
| `factory.rs`     | 按驱动构造连接对象                                                           |
| `stream.rs`      | 底层流（`ConnectionStream::Tls` 等；Tls 变体无单测覆盖，靠 workbench 真实拨号）         |
| `known_hosts.rs` | SSH known\_hosts：查询侧大小写归一化（`normalize_host` 写读一致）                   |
| `secret.rs`      | 凭据加解密（AES-256-GCM）                                                  |
| `lib.rs`         | crate 导出与公共 API                                                     |

### 3.3 `crates/engine/`（持久化 / 驱动 / 缓存）



| 文件                                      | 职责                                                                                                                           |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `persistence/id_prefix.rs`              | ID 前缀生成与判定（§4.2）                                                                                                             |
| `persistence/connection_draft_store.rs` | `connection_drafts` 表读写（`list`/`replace_all`，无密码列）                                                                           |
| `persistence/metadata_cache_pool.rs`    | 元数据缓存池（mmap 读路径修复：`query_row` 不 panic）                                                                                       |
| `cache/metadata_cache.rs`               | `MetadataCacheKey`（conn\_id 为键：Catalogs/Schemas/Tables/Columns/Views/Procedures/Functions/Sequences/Triggers）+ LRU           |
| `driver/declaration.rs`                 | **驱动声明落库：代码是权威，**`drivers`**&#x20;表是读模型**；启动幂等 upsert（`upsert_all`）；`enabled` 归用户、`download_*` 归外部驱动                         |
| `driver/registry/descriptors.rs`        | `DriverDescriptor`：6 个内置驱动声明 + `capability_keys`/`fields`/`extra_options`                                                    |
| `duckdb/mod.rs` + `file_probe.rs`       | DuckDB 文件初始化（**按驱动分派：**`initialize_duckdb_file`**&#x20;空文件→真库初始化**，修复 "新建报 not a valid DuckDB database file"）、结构指纹（定义 + 列结构） |
| `migrations/global/`                    | 020 `connection_drafts`、021 草稿加 `type_id`+`driver_id`、022 草稿加 tags/groups、023 草稿加 auth\_method                               |

## 4. 核心概念模型

### 4.1 作用域三态与落库语义



| 选择      | 落库                              | ID 前缀 |
| ------- | ------------------------------- | ----- |
| 仅全局     | `global.db::global_connections` | `G_`  |
| 仅项目     | 当前项目 `project.db::connections`  | `P_`  |
| 全局 + 项目 | 全局定义 + 项目快照引用（独立副本，显式同步）        | `GP_` |



* 项目可引用全局（快照 `GP_`），全局不可引用项目私有配置。

* 遗留 v1 全局连接前缀 `conn-`（迁移数据）；`is_global_connection` 统一判定（服务层与导航侧同一来源，修复过错位写库）。

### 4.2 连接 ID 生成规则（`id_prefix.rs`，实证）



| 函数                            | 格式                                        | 示例                      |
| ----------------------------- | ----------------------------------------- | ----------------------- |
| `generate_gid(entity, name)`  | `G_{entity}_{sanitized_name}`             | `G_conn_prod`           |
| `generate_pid(entity)`        | `P_{entity}_{8位随机hex}`                    | `P_conn_a1b2c3d4`       |
| `generate_gpid(entity, name)` | `GP_{entity}_{sanitized_name}_{YYYYMMDD}` | `GP_conn_prod_20261002` |



* `sanitize_name`：非字母数字 /`-`/`_` 全部替换为 `_`。

* `source_global_id(GP_env_dev_20260522)` → `G_env_dev`（`rsplitn(2,'_')` 去日期）；`to_snapshot_id(G_env_dev)` → `GP_env_dev_<today>`。

* 判定函数：`is_global` / `is_project` / `is_snapshot` / `is_global_connection`（含遗留）/ `uses_project_storage` / `origin_from_id`。

* **编辑改名不换 ID，新建重名被拦**（ID 内部化：界面只显示名称 + `P/G/GP` 来源短码，完整 ID 在结果行「详情」）。

### 4.3 缓存身份指纹（规则已冻结）



* 同一物理库的多条连接**共享 L2 元数据缓存**。

* **身份 = 数据库族 + 规范化地址 + principal + 格式版本**；驱动实现 / 密码 / 连接参数**不进身份**。

* 指纹只作缓存键、**不作主键**；DuckDB 文件结构指纹 = 定义 + 列结构（行数只作元信息）。

* 缓存键：`MetadataCacheKey` 以 `conn_id` 为第一维（L1），L2 按身份指纹共享；LRU + 分级内存估算（Catalogs 100 / Tables 500 / Columns 1000 等，见 `metadata_cache.rs`）。

### 4.4 草稿 / 暂存概念



* **暂存区只放未保存草稿**（用户决策 #50）：已保存连接不进暂存列表，统一从导航栏编辑（✎）；打开对话框时清理历史残留的已保存条目。

* 草稿**跨会话持久化**：`connection_drafts` 表（**无密码列**，`pass` 字段只存内存快照），应用重启后首次打开自动恢复。

* 列表恒非空：删至最后一条自动补空草稿；空草稿 `is_empty_draft()` = 无 saved\_id 且名称 / URL / 驱动 id 全空。

## 5. 数据模型

### 5.1 `ConnectionDraft`（草稿快照，`staging.rs:78`）



```
pub struct ConnectionDraft {
    pub name: String,              // 显示名（空 → 「新建数据源」）
    pub saved_id: Option<String>,  // Some = 已保存条目（编辑模式回读）
    // ---- 表单快照 ----
    pub type_id: String,           // 数据库类型 id（类型树/驱动过滤依据）
    pub driver_id: String,         // 驱动 id（drivers.id；落库/回读最稳，如 mysql_native）
    pub driver_name: String,       // 驱动实现短名（Header 下拉显示值，如 sqlx）
    pub url: String, pub user: String, pub pass: String, pub remark: String,
    pub tags: String,              // 逗号分隔；保存时解析为 JSON
    pub groups: Vec<String>,       // 项目分组 id（替换语义同步成员关系）
    pub scope: String, pub project_path: String,
    pub ssl_mode: String, pub ssl_ca: String, pub ssl_cert: String, pub ssl_key: String,
    pub cache_path: String,        // 元数据缓存路径
    pub duckdb_fed: bool,          // 默认 true（ConnectionDraft::empty()）
    pub active_tab: usize,
    pub props: Vec<(String, String)>,     // 驱动属性（BTreeMap 化后 JSON）
    pub sec_overrides: Vec<String>,       // 策略覆盖（environment_policies.policy_type）
    pub auth_method: String,              // drivers.supported_auth_types 中的键
    pub auth_ref: Option<String>,         // 认证档案引用（按名称）
    pub network_ref: Option<String>,      // 网络档案引用（按名称）
    pub env: Option<String>,              // 环境引用（按名称）
}
```



* `display_name()`：空名回退「新建数据源」/「（未命名连接）」（已保存）。

* `empty()`：`duckdb_fed: true`、`sec_overrides: Vec::new()`、其余 Default。

* `draft_to_row`/`row_to_draft`：UI 条目 ↔ 持久化行（**不写密码**；`hops_json` 列保留但固定写空数组 —— 内联协议链已撤下 #25；`pass` 回读恒为空）。

### 5.2 保存输入口径（`draft_to_save_input`，`state.rs:1188`）

与 `collect()`（表单→输入）同一校验与组装规则，但**字段全来自草稿快照**：



1. **缺项拦截**：`driver_id` / `name` / `url` 任一空 → `None`（批量提交整体中止）。

2. 引用档案：**名称 → ID**（在对话框已加载的 `auth_list`/`network_list`/`env_list` 中按名查找）；批量提交后台只带解析后的 ID 过线。

3. SSL 覆盖：**仅当驱动声明&#x20;**`ssl`**&#x20;认证类型**时组装（mode/ca/cert/key）；`disable` 不落。

4. 策略覆盖：落已勾选的 `policy_type`（`policy_overrides` 数组）。

5. 文件型驱动（`is_file_db_driver`）：`strip_file_db_noise` 清掉 username/password/ 凭据引用 / SSL。

6. `scope_from_label`：UI 标签 → `ConnectionScope`；`tags_to_json`：逗号串 → JSON。

7. `use_duckdb_fed: Some(duckdb_fed)`、`metadata_path` = cache\_path。

`DataSourceSaveInput` 关键字段：`name` / `db_type`(=driver\_id) / `url` / `username` / `password` /

`scope` / `description` / `tags` / `driver_id` / `environment_id` / `auth_config_id` / `auth_method` /

`network_config_id` / `driver_properties` / `advanced_options` / `options` / `use_duckdb_fed` / `schema_name` / `metadata_path`。

## 6. 保存语义（2026-10-02 用户决策，代码实证）

> **「保存」= 仅暂存草稿箱（不落库）**
>
> ；
>
> **「保存并退出」= 批量提交暂存栏全部条目到数据库 + 清空草稿箱 + 关闭**
>
> 。
> 旧语义（已废弃）：保存 / 保存并关闭都只落库当前表单 —— 用户以为已实现批量写入，经核对确认未实现，本轮补齐。

### 6.1 按钮与快捷键



| 控件                                                  | 动作                | 行为                 |
| --------------------------------------------------- | ----------------- | ------------------ |
| 「保存」（footer / Ctrl+Enter / `SaveConnection` Action） | `do_stage`        | 仅暂存草稿箱             |
| 「保存并退出」（footer，原名「保存并关闭」）                           | `do_batch_commit` | 批量提交 + 清空 + 关闭     |
| 「测试连接」（Ctrl+T）                                      | `do_test`         | 独立会话真实预演（不落库、不注册池） |

### 6.2 `do_stage`（保存 = 暂存，`render.rs:2010`）



1. `BusyOp != None` 直接 return（防并发）。

2. **编辑模式**（`editing_id` Some）：不落草稿箱，提示「编辑中的修改将在『保存并退出』时提交」—— 已保存连接从导航栏编辑，批量提交时统一 update。

3. 新建模式：`capture_draft(cur)` + `staging_persist()` → 提示「已暂存『{名}』（未写入数据库；保存并退出时统一提交）」。

4. `notify_host` 刷新宿主层。

### 6.3 `do_batch_commit`（保存并退出，`render.rs:2051`）



1. **收集待提交条目**（`pending: Vec<(draft_idx: Option<usize>, saved_id: Option<String>, DataSourceSaveInput, Option<String> 项目路径, Vec<String> 分组)>`）：

* 编辑中的连接：表单实时 `collect()`（缺项 → `blocked_hint` + 拦截提示，中止）；`draft_idx=None, saved_id=Some(editing_id)`。

* `staging_flush(app)` 先把当前表单写回草稿（新建模式光标位草稿持有最新值）。

* 遍历草稿箱：`is_empty_draft()` 跳过；`draft_to_save_input(d)` 失败 → **整体中止不落库**，提示「条目『{名}』{缺什么}：未提交任何条目」；`draft_idx=Some(idx)`、`saved_id=d.saved_id`。

* `pending` 为空 → 提示「暂存列表为空：先填写连接信息并点『保存』暂存」。

1. **后台线程**（`background_executor().spawn`）：`BusyOp::Save`；逐条 `DataSourceService` 按 `saved_id` 路由 `update(cid, input, pp)` / `save(input, pp)`；**失败即停**（`fail = Some((draft_idx, err))`）；成功时 `set_connection_groups`（分组未落库 → `group_degrade += 1`，不静默 #28）；累计 `ok_indices`；最后 `load_connections_for_scope(session_root)` 刷新列表 → 返回 `BatchOutcome { ok_indices, fail, total, group_degrade, items }`。

2. **续体**（`spawn_in` 回前台）：

* 失败：`remove_drafts_at(ok_indices)`（已成功条目移出草稿箱，失败及后续保留）→ 提示「保存失败：{err}（已成功的条目已移出暂存，其余保留，请修正后重试）」**不关闭**。

* 全成功：写回 `shared.connections` + `refresh_mock_connections` + 通知「已保存 N 条连接」→ `staging_clear(app)` 清空草稿箱 → 结果行「已保存 N 条连接，暂存草稿箱已清空」（有降级则「N 条分组未同步」）→ `window.close_dialog(app)`。

* `shared.notify_host` + `editor.update` 重绘。

### 6.4 暂存持久化原语（`staging.rs`）



| 原语                                      | 触发                  | 行为                                                    |
| --------------------------------------- | ------------------- | ----------------------------------------------------- |
| `staging_restore(window, cx)`           | 首次打开                | 仅当仍为初始「单个空草稿」时用库中条目替换（避免覆盖本会话已编辑内容）+ `apply_draft(0)` |
| `staging_persist()`                     | 添加 / 切换 / 删除 / 保存暂存 | 全量替换写入（`replace_all`；只写未保存草稿）                         |
| `staging_flush(cx)`                     | 关闭对话框 / 批量提交前       | `capture_draft(cur)` 写回当前表单再 persist                  |
| `staging_clear(cx)`                     | 批量提交全成功             | 清空内存列表 + 光标归 0 + persist（表内旧行一并清）                     |
| `remove_drafts_at(indices, window, cx)` | 批量提交部分失败            | 升序去重后倒序删除已成功下标；失败条目保留；删空补空草稿；`apply_draft` + persist  |

## 7. 关键流程

### 7.1 打开对话框

导航栏「新建连接」/「编辑（✎）」→ `ConnectionDialog` 挂载 → `staging_restore` 恢复草稿 →

`apply_draft(0)` 载入表单；编辑模式 `load_for_edit` 回读全部字段（项目侧走 `get_with_project`，无项目根得空表单不报错）。

### 7.2 切换条目 / 关窗



* 切换：`capture_draft(旧)` 写回 → `apply_draft(新)` 载入（脏标记 `●` = 草稿 vs 表单有差异）。

* 关窗（取消 / Esc / 遮罩）：`staging_flush` 写回当前表单 → 草稿跨会话保留（**关闭不丢失**）。

### 7.3 测试连接（`do_test`）



* `build_probe_config` 唯一组装点：同规则注入认证档案凭据、**真实建立网络档案隧道（测完即关）**；

  差异只剩「不注册连接池 / 不落库」。

* 档案缺失 / 隧道建不起来 → **直接失败**（严格模式不静默降级，决策 #75）。

* 后台线程探测（决策 #102，UI 不阻塞）；结果 `{success, message, latency_ms, version}` → 结果行

  成功绿 ✓ + 版本 + 延迟（明细化，2026-10-02），失败红 + 摘要 + `详情/复制`。

### 7.4 落库（保存并退出批量路径）

`DataSourceService::save(input, project_path)` / `update(id, input, project_path)`：

同名检查（大小写不敏感）+ 项目路径预检 → ID 生成（按作用域）→ tags / 分组同步 → DuckDB Secret 注册

（`use_duckdb_fed=1` 时 `ensure_secret_registered`）→ 列表刷新。文件型新建走

`create_new_db_file` → **按驱动分派**：SQLite `sqlite3_open` 建真库；DuckDB `initialize_duckdb_file`

初始化真库（不再用空文件占位，修复 `[CONN_INVALID_CONFIG] not a valid DuckDB database file!`）。

## 8. 驱动体系



| 内置驱动              | 实现           | 数据库族        |
| ----------------- | ------------ | ----------- |
| `mysql`           | sqlx         | mysql       |
| `mysql_native`    | Official 客户端 | mysql       |
| `postgres`        | sqlx         | postgres    |
| `postgres_native` | Official 客户端 | postgres    |
| `sqlite`          | 内建           | sqlite（文件型） |
| `duckdb`          | DuckDB 1.5.5 | duckdb（文件型） |



* **声明在代码**（`registry/descriptors.rs`），启动时 `upsert_all` 幂等落 `drivers` 表（迁移种子退居首装兜底）；

  `enabled` 归用户（DO UPDATE 不覆盖）、`download_url/checksum` 归外部驱动、`driver_files` 归本机安装状态。

* `capabilities` = 显式声明 + 网络布尔位派生；`config_schema` = 界面实际读的键（key/label/type/required/default/placeholder/options）。

* 类型目录按 `enabled` 过滤；无可用驱动的类型置灰「暂无驱动」。

* 文件型驱动：地址输入在「常规 → 连接设置」卡片（`打开文件… / 新建文件…`），Header 只显示路径；网络型地址在 Header 输入框（占位取 `url_template`）。

## 9. 测试体系

### 9.1 基线（2026-10-02 实证）



| 套件                  | 命令                                                                      | 结果                                                                                                |
| ------------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| connection crate 单元 | `cargo test -p rds-connection --lib -j 2`                               | **96 passed**（扩充前 48 → +48）                                                                       |
| connection crate 集成 | `cargo test -p rds-connection --test tunnel_roundtrip -j 2`             | **6 passed**                                                                                      |
| workbench 对话框 lib   | `cargo test -p rds-workbench --lib connection_dialog -j 2`              | **38 passed**（含 state.rs +3）                                                                      |
| connection 集成测试     | `cargo test -p rds-workbench --test connection_<各文件> -j 2`              | 12 文件全绿（multi\_save 7 /staging 7 /dialog\_ui 7 /render\_matrix 6 /data\_source\_lifecycle 9 / 其余） |
| 编译守卫                | `cargo check -p rds-workbench -j 2` / `-p rds-connection --all-targets` | 0 warning 0 error                                                                                 |

集成测试文件：`connection_dialog_ui` / `connection_drafts_persist` / `connection_edit_backfill` /

`connection_multi_save` / `connection_project_picker` / `connection_render_matrix` / `connection_scope_and_state` /

`connection_staging` / `connection_template` / `connection_tunnel_cleanup` / `connection_type_driver` / `real_connections`。

### 9.2 关键测试点（保存语义重构新增 / 改写）



* `two_connections_staged_then_batch_committed`：构造 2 条非空草稿 → 逐条 save + `staging_clear` → 草稿箱 0 条 + 库可读回两条。

* `batch_commit_updates_editing_entry`：saved\_id 条目走 update，库中名称更新验证。

* `staging_clear_empties_draft_box`：2 条非空草稿 → clear → 0 条 + 光标 0。

* `remove_drafts_at_keeps_failed_entries`：3 条移除 \[0,2] → 留「草稿 B」；再删空 → 补 1 空草稿。

* `draft_to_save_input_*`（3 项）：快照字段映射 / 文件型清噪 / 缺项拒绝。

### 9.3 已知测试缺口



* `ConnectionStream::Tls` 变体无法脱离真实 TLS 握手构造，单测未覆盖其地址 / 加密访问器（真实拨号由 workbench 套件覆盖）。

* MySQL/PostgreSQL 真机套件需 `RDS_TEST_PG_URL` 等环境变量（未设跳过）；本机实测以 SQLite/DuckDB 为主。

## 10. 文档体系（`docs/architecture/connection/`）



| 文档                                     | 职责                                                                                                            |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `README.md`                            | 模块入口：特点 / 边界 / 代码地图 / 硬约束 / 文档地图 / 下一步                                                                        |
| `connection-dialog-architecture.md`    | **唯一权威设计**：分层 / 概念模型 / 状态所有权 / 数据流 / 决策表 / 测试策略 / 实现映射 / 数据字典 / 错误降级 / 性能安全 / 成熟度 /§14 待办 / 数据来源审计 / M3↔M4 契约 |
| `connection-prototype-design.md`       | 原型设计：设计基准 / 布局 / 五 Tab / 关键交互 / 主题映射 / GPUI 落点 / 开放问题                                                         |
| `connection-user-guide.md`             | 使用指南：能做 / 入口 / 导览 / 流程 / Tab 速览 / 快捷键 / 草稿安全 / FAQ/USIT 清单                                                    |
| `connection-crate-interfaces.md`       | connection crate 接口与接线                                                                                        |
| `connection-dev-plan.md`               | 开发方案（Phase A/B/C + 变更史）                                                                                       |
| `connection-test-report-2026-10-01.md` | 系统测试报告（§10 = 保存语义重构回归）                                                                                        |
| `connection-showcase.md` / `.html`     | 宣传页（一页看懂，明暗双主题）                                                                                               |
| `connection-dialog-prototype.html`     | 交互原型稿（手工维护，可能滞后）                                                                                              |

## 11. 关键设计决策（速查；详情见架构 §6）



| #    | 决策                                                                      |
| ---- | ----------------------------------------------------------------------- |
| #50  | 暂存区只放未保存草稿（用户决策）；保存并退出批量提交后清空（2026-10-02 扩展）                            |
| #68  | ID 前缀判定收归 `id_prefix`（服务层与导航同一来源）                                       |
| #74  | 测试连接 = 真实连接预演（`build_probe_config` 唯一组装点）                               |
| #75  | 严格模式不静默降级（档案缺失直接失败）                                                     |
| #79  | 凭据全程 AES-256-GCM 加密                                                     |
| #80  | 草稿跨会话持久化（无密码列）                                                          |
| #82  | 结果行分级 + 详情 / 复制（级别双通道：图标 + 颜色）                                          |
| #90  | 首次引导是空态、不是常驻                                                            |
| #93  | 连接 ID 内部化（B 案）：界面只出现名称 + 来源短码                                           |
| #101 | 右列子项高度之和不超行高（overflow\_hidden 红线）                                       |
| #102 | 在途操作离开 UI 线程（`BusyOp` + `background_executor` + `spawn_in`；关闭动作在结果回来之后） |
| #105 | 行高先定 + 按窗口可用高度夹住                                                        |
| #107 | 暂存行由 `list::List` + `ListItem` 渲染；行尾 ✕ 是 Button 不冒泡                     |

## 12. 已知问题与待办（权威：架构 §14）



* 页面美化（2026-10-02 用户反馈：常规 Tab 卡片标题已左对齐；整体美化待用户指定区域后继续）。

* 模板导入导出能力就绪、UI 入口暂缓（C4/C12）。

* dev-plan / README 文档瘦身整理方案已出（2026-10-02），待用户确认后执行。

## 13. 复现命令速查（Windows + PowerShell）



```
# 编译（必须 -j 2）
cd D:\RdataStation\RDS\RdataStation-v2
cargo check -p rds-workbench -j 2           # Finished 即成功（stderr exit 101 是 PowerShell 显示噪音）
cargo build -p rds-app -j 2                 # 产物 target\debug\rds-app.exe

# 测试
cargo test -p rds-connection --lib -j 2                       # 96
cargo test -p rds-connection --test tunnel_roundtrip -j 2      # 6
cargo test -p rds-workbench --lib connection_dialog -j 2       # 38
cargo test -p rds-workbench --test connection_multi_save -j 2  # 批量提交语义
cargo test -p rds-workbench --test connection_staging -j 2     # 暂存原语

# 启动
Start-Process -FilePath "D:\RdataStation\RDS\RdataStation-v2\target\debug\rds-app.exe" -WorkingDirectory "D:\RdataStation\RDS\RdataStation-v2"

# 提交规范：type(scope): 中文  （例：feat(connection): 保存语义重构…）
# 注意：提交前 git status 核对，勿卷入他人在途改动（如 driver/declaration.rs、migration/global_init.rs）
```



***

*本规格书由 Doubao 办公模式于 2026-10-02 生成；保存语义、测试基线、决策编号均已按源码与提交&#x20;*`b2e07859`*&#x20;核对。*