# 项目管理模块 · 接线矩阵

> 状态：**2026-10-01 首版**（对照 `crates/project` 源码逐方法核实，`cargo check -p rds-project` 零告警）
> **2026-11-22 复核**：行数按当前源码实测更新；补记本次三项新增的接线落点（`BUILD_TAG_KEY` 写盘、`location_hint` 行内校验、`last_create_location` 记忆——后两项的持久化在 `rds-settings`，不在本 crate）；复核“零调用”结论仍成立（`has_recent_available` 仍只被集成测试断言，`*_connection_metadata` 仍只被 store 自身与其单测调用）。同轮 clippy 清理后本 crate 告警 22 → 2（余 `OpenOutcome` 的 `large_enum_variant`、`dialog_footer_three` 的 8 参数，均为有意取舍，见 view-architecture §8）。
> **2026-11-23 复核（遗留点清理第一、二刀）**：
> ① **行数口径更正**：上一版的 3029 / 200 / 389 是用 `Measure-Object -Line` 数的，该口径在 CRLF 文件上少算行；按 `\n` 字节数权威重算为 **`ui.rs` 3300、`lock.rs` 230、`models.rs` 395**（`58cd599f` 提交那一刻 `ui.rs` 就是 3300，不是后来长出来的）。
> ② **`models.rs` 删掉 4 个自带路径推导**：`ProjectPath::{sqlite_path, duckdb_path, config_dir, queries_dir}` 与 `Project::{sqlite_path, duckdb_path}`——它们指向 v1 旧布局（`.RSmeta/meta/project.db`、`.RSmeta/analytics/data.duckdb`、`.RSmeta/queries`），与真实的扁平布局不符，且生产零调用。`ProjectPath::meta_dir` 保留，但改走 `store::RS_META_DIR_NAME` 常量，不再硬编码 `".RSmeta"`。
> ③ 口径：与根仓 `docs/architecture/data-layer-wiring-matrix.md` 同族——把「文档声称」与「代码实测」分开，
> 逐方法标接线状态，防止「预留超前」产生的死代码继续累积。
> 本文是**唯一接线台账**；改 `crates/project` 实现后请同步本表。

## 图例

| 标记 | 含义 |
| --- | --- |
| ✅ 已接线 | 有生产调用方（feature / workbench / app），是活代码 |
| 🟡 预留 | 有类型 / 结构 / 文档声称，但未接任何业务数据（DuckLake 等预留） |
| 🔴 待删 | **无生产调用方**（仅 store 自身 / 测试 / `lib` re-export 引用），v1 遗留或 stub |

## 总览

| 文件 | 代码量（2026-11-23 实测，`\n` 计数） | 接线健康度 |
| --- | --- | --- |
| `service.rs` | 716 行 | ✅ 全部已接线（本项目真实入口） |
| `lock.rs` | 230 行 | ✅ 全部已接线 |
| `store.rs` | 948 行 | ⚠️ 核心已接线；**死代码已清（ProjectManager / to_project / stub），剩 1 组归属待定**（见下）；新增 `BUILD_TAG_KEY` 写盘分支 |
| `models.rs` | 395 行 → 2026-11-23 删掉 4 个错误路径推导后更短 | ⚠️ 域模型已接线，**含 1 组预留**（版本化） |
| `ui.rs` + `ui/tests.rs` | 3300 + 1411 行（测试 38 项：24 窗口 + 14 纯函数） | ✅ 全部已接线（有窗口测试锚定） |

## 逐方法

### `service.rs` — 生命周期编排（✅ 全部已接线）

| 方法 | 状态 | 说明 |
| --- | --- | --- |
| `manager()` / `runtime()` | ✅ | 服务入口；`runtime()` 为进程级 current-thread 复用（P0.3 兑现，见 dev-plan §0） |
| `validate_project_name` / `is_valid_project` / `inspect_target` | ✅ | 校验 / 目标探测 |
| `list_recent` / `list_all` / `list_removed` / `summarize` | ✅ | 名册查询（仅最近项做驱动自检） |
| `missing_driver_names` | ✅ | 缺失驱动探测 |
| `create` / `register_and_summarize` | ✅ | 新建 + 登记名册 |
| `open` / `open_read_only` / `load_and_register` / `close` | ✅ | 打开（含只读逃生口）/ 关闭 |
| `update_meta` / `set_pinned` / `set_archived` | ✅ | 重命名 / 固定 / 归档 |
| `load_default_connection` / `save_default_connection` | ✅ | 默认连接（U3） |
| `soft_remove` / `restore` / `delete_disk` / `forget` | ✅ | 软删 / 恢复 / 硬删 / 移出 |
| `has_recent_available` | 🔴 生产零调用（仅测试） | 会话解析辅助；暂无用例，保留为预留能力 |
| `relocate` | ✅ | 重新定位 |
| `list_versions` / `create_version` / `ensure_project_meta` | ✅ | **手动快照台账**（唯一在用的版本能力） |

### `lock.rs` — 实例锁（✅ 全部已接线）

| 方法 | 状态 | 说明 |
| --- | --- | --- |
| `ProjectLock::acquire` / `probe` / `release` | ✅ | OS 文件锁 + owner 分离；崩溃自动释放 |
| `LockInfo` / `AcquireOutcome` | ✅ | 占用者信息 / 结果枚举 |

### `store.rs` — 磁盘持久化（⚠️ 核心已接线 + 待删组）

**已接线**

| 方法 | 状态 | 说明 |
| --- | --- | --- |
| `create` / `load` | ✅ | `.RSmeta` 建库（双引擎分流）/ 加载（JSON 权威） |
| `migrate_old_structure` / `merge_connections_db` | ✅ | v1 `.rdata-station` → `.RSmeta` 一次性迁移 |
| `info` / `info_mut` | ✅ | 项目信息访问 |
| `load_config` / `update_config` / `save_config` | ✅ | 配置读写（U3 默认连接） |
| `save_info` / `update_info` | ✅ | **双真相源唯一写入入口**（JSON 权威 + SQLite 镜像） |
| `meta_dir` / `project_db_path` | ✅ | 路径访问 |
| `check_project_missing_drivers` | ✅ | 缺失驱动自检（全局库比对） |

**🔴 已删除（2026-10-01）**

| 方法 | 状态 | 说明 |
| --- | --- | --- |
| ~~`ProjectManager`（整个 struct）~~ | ✅ 已删 | 无生产调用方，v1 遗留；连同 `impl Default`、`lib.rs` re-export、`test_project_manager`（store 内）与集成测试 `manager_tracks_current_and_recent` 一并移除 |
| ~~`to_project`~~ | ✅ 已删 | 无生产调用方 |
| ~~`load_connections` / `load_queries`~~ | ✅ 已删 | 显式 stub（`SQL read not yet implemented`）；**`Project.connections` / `queries` 字段与 `ConnectionRef` / `QueryRef` 类型保留在 `models.rs` 作为 DuckLake 三期预留** |
| ~~`ProjectStore::connections` / `queries` 缓存字段~~ | ✅ 已删 | 仅被上述 stub 读取，一并移除 |

**🟡 归属待定（未删，待三期一并决策）**

| 方法 | 状态 | 说明 |
| --- | --- | --- |
| `create_connection_metadata` / `connection_metadata_path` / `delete_connection_metadata` | 🟡 | 仍无生产调用方（原型 §1.2 设计过 `project_metadata/`，但 engine L2 缓存有自己的实现，从未接这条链）；归属空洞——project 提供、engine 消费？还是整体删除？建议与 DuckLake 三期一并定 |

### `models.rs` — 域模型（⚠️ 已接线 + 预留组）

**已接线**

| 类型 | 状态 | 说明 |
| --- | --- | --- |
| `Project` / `ProjectInfo` / `ProjectStatus` | ✅ | 核心项目模型 |
| `ProjectPath::Local` | ✅ | 本地路径（当前唯一真实形态） |
| `ProjectConfig` | ✅ | 配置（U3 默认连接） |

**🔴 已删除（2026-11-23）**

| 方法 | 状态 | 说明 |
| --- | --- | --- |
| ~~`ProjectPath::{sqlite_path, duckdb_path, config_dir, queries_dir}`~~ | ✅ 已删 | 指向 v1 旧布局（`.RSmeta/meta/project.db`、`.RSmeta/analytics/data.duckdb`、`.RSmeta/queries`），与真实扁平布局（`.RSmeta/{project.db, analytics.duckdb, config/, project_metadata/}`）不符，生产零调用 |
| ~~`Project::{sqlite_path, duckdb_path}`~~ | ✅ 已删 | 只是转发上面那套错误推导；库文件路径的权威入口是 `ProjectStore::{meta_dir, project_db_path}` |
| `ProjectPath::meta_dir` | ✅ 保留并改源 | 不再硬编码 `".RSmeta"`，改走 `store::RS_META_DIR_NAME`（单一来源） |

**🟡 预留 / 未接业务**

| 类型 | 状态 | 说明 |
| --- | --- | --- |
| `ProjectPath::Remote` | 🟡 | DuckLake 远程预留（UI 已明示范围） |
| `Versioned<T>` / `Version` / `VersionInfo` / `create_child` / `verify_checksum` | 🟡 | **未接入任何业务数据**；真实在用的是 `project_versions` 手动快照台账（见 service） |
| `calculate_checksum` | 🟡 | 仅被未接线的 `Versioned<T>` 用；`DefaultHasher` 非加密，若真做内容级版本化须换 `sha2` |
| `ConnectionRef` / `QueryRef` / `Project::connections` / `Project::queries` / `add_connection` / `add_query` | 🟡 | 模型存在，但 `load_connections` / `load_queries` 是 stub，**读侧空转** |

### `ui.rs` + `ui/tests.rs`（✅ 全部已接线）

选择器 / 标题栏菜单 / 项目设置 / 语义对话框 / 宿主桥 `ProjectUiHost`，均有窗口测试锚定（`ui/tests.rs` 38 项：24 窗口 + 14 纯函数）。

2026-11-22 新增接线（均已接线、有生产调用方）：

| 组成 | 状态 | 说明 |
| --- | --- | --- |
| `store::BUILD_TAG_KEY` + `apply_opened` 写盘 | ✅ | 非只读打开时把 `version::info().tag()` 写进 `config/settings.json` 的 `extensions.rds.build_tag`；值未变跳过写盘，失败只 `tracing::warn!`（由 workbench 注入 `.with_build_tag`）。⚠️ **无单测**：写盘在 `apply_opened` 事件路径、依赖全局库，目前只由 `cargo test -p rds-project` 冒烟兜底 |
| `location_hint(location, name)` | ✅ | 新建对话框行内提示纯函数，判据与 `inspect_target` 同源；单测 `location_hint_matches_create_preconditions` 覆盖六分支，窗口测试 `create_dialog_inline_hint_updates_while_typing` 证明键入后提示同帧出现 |
| `folder_hint(path)` | ✅ | 「打开现有项目」对话框行内提示纯函数，**文案逐字复用 `submit_open_folder`**；空目录不报错（提交时才问是否在此创建）；单测 `folder_hint_matches_open_preconditions`（五分支）+ 窗口测试 `open_folder_dialog_hint_flags_non_project_dir`（换路径后提示出现 / 消失） |
| 位置记忆 `last_create_location` | ✅ | 读 / 写经 `ProjectUiHost.last_create_location` / `save_create_location` 注入；持久化在 `rds-settings`（`projects.last_create_location`，`SettingEntry::Module`，不上设置页） |

## 处置建议（按成本从低到高）

1. **✅ 已执行（2026-10-01）**：删除 `ProjectManager`（整 struct）、`to_project`、`load_connections` /
   `load_queries`（stub）及 `ProjectStore` 的 `connections` / `queries` 缓存字段；同步移除 `lib.rs`
   re-export、`store.rs::mod tests::test_project_manager`、集成测试 `manager_tracks_current_and_recent`。
   验证：`cargo check -p rds-project` 零告警；测试 42 + 名册 1 + 存储 3 全过。
2. **🟡 待三期一并决策**：`create_connection_metadata` / `connection_metadata_path` /
   `delete_connection_metadata`（`project_metadata/` 归属空洞）——与 DuckLake 一并定：project 提供、
   engine L2 缓存消费，或整体删除。
3. **🟡 二选一**：版本化（`Versioned<T>` / checksum）——删除（维持「手动快照台账」）或真做内容级
   版本化并把 `DefaultHasher` 换成 `sha2`（DuckLake 三期可一并定）。
