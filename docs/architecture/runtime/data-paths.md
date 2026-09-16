# 运行时数据路径（配置 / 数据 / 临时 / 日志）

状态：**设计待实施**（2026-09-16，口径已确认）。动机：默认路径全部落在 C 盘（`%APPDATA%` / `%TEMP%` / `~`），
开发机 C 盘空间紧张；且现状没有单一解析入口，路径散落在 7 个 crate 里**各自拼字符串**。

**已确认的口径**：① 这个软件生成的**任何信息**（配置 / 数据 / 日志 / 临时 / 缓存 / 扩展）都待
在软件自己的目录下，**默认 = 软件的安装目录**（可执行文件所在目录）；② 这些生成物**一律不进 git**。

## 1. 现状侦察（实测）

| 类别 | 当前位置（Windows） | 代码点 |
| --- | --- | --- |
| 全局设置 | `%APPDATA%/RdataStation/settings.json`，失败回退 `%TEMP%/RdataStation` | `crates/settings/src/lib.rs:72,75` |
| 全局数据根 | `%APPDATA%/RdataStation` | `engine/src/migration/global_init.rs:18`（`GLOBAL_DATA_DIR_NAME`）、`engine/src/persistence/{connection_store,history_store}.rs`、`shared/src/crypto.rs:21,92` |
| 系统库 / 分析库 | `%TEMP%/RdataStation/system`（**回退到临时目录**） | `project/src/ui.rs:881,885`、`workbench/src/services/workspace_loader.rs:27` |
| 日志 | `<log_dir>/app.log` + 保留期清理 | `engine/src/logging/{config.rs:56,subscriber.rs:131}` |
| 临时文件（DuckDB spill / 联邦 / 测试） | `std::env::temp_dir()` → `%TEMP%` | `engine/src/duckdb/{executor,federation,manager,snapshot}.rs` + 各 crate 测试 |
| DuckDB 扩展 | `{data_dir}/duckdb/extensions`；另 `dirs::home_dir()` 一处 | `engine/src/duckdb/{engine/duckdb_engine.rs:508,manager.rs:300}` |
| SSH known_hosts | `dirs::home_dir()` → `~/.ssh` | `connection/src/known_hosts.rs:57` |

**顺带发现两个隐患**（与 C 盘无关也值得修）：

1. 项目/系统数据**回退到 `%TEMP%`**：系统清理（存储感知 / 磁盘清理）会直接删掉全局配置、分析库与密钥库。
2. 9 处硬编码 `"RdataStation"` 字面量（`shared/crypto.rs`、`engine/persistence/*`、`settings`、`project/ui.rs`、`workbench/services`）——改路径要动 9 个文件，且容易漏。

## 2. 目标形态：单一根 `RDS_HOME`

```
<RDS_HOME>/                     # 默认：仓库根/.rds（见下放规则）
├── config/settings.json        # 全局设置
├── data/                       # global.sqlite / shared.duckdb / 密钥库
├── logs/app.log                # 日志（保留期清理不变）
├── tmp/                        # DuckDB spill / 联邦临时库 / 进程 scratch
└── extensions/                 # duckdb 扩展
```

派生规则（`crates/paths`，唯一解析点）：

```rust
paths::home()            // RDS_HOME 环境变量 → 否则按下方规则推导
paths::config_dir()      // home/config
paths::data_dir()        // home/data
paths::log_dir()         // home/logs
paths::temp_dir()        // home/tmp
paths::extensions_dir()  // home/extensions
```

**默认值规则**：`RDS_HOME` 未设时 = **可执行文件所在目录**（即软件的安装目录；开发运行时即 `target/debug/`）：

- 一切生成物都在这个目录下，随软件装到哪就跟到哪，**不碰 C 盘**（除非软件本身装在 C 盘，那时用 `RDS_HOME` 覆盖）
- 启动先探测可写性：不可写（如装在 `Program Files`）→ 回退 `%LOCALAPPDATA%/RdataStation` 并在日志里明确提示
- **开发注意**：从 `target/debug` 运行时数据落在 `target/` 内，`cargo clean` 会连数据一起清掉；
  开发期间要保留就设 `RDS_HOME=<repo>/.rds`（该目录已在忽略规则中）

> 为什么不默认放"用户项目目录"：`<用户项目>/{project.sqlite, analysis.duckdb}` 是**用户资产**，
> 归 M1 项目会话管理（已在项目根下）；而 settings / global.sqlite / 密钥库是**应用资产**，
> 放进用户项目里会污染用户的工程目录。两者必须分开（见 `overview.md` 双层数据）。

## 3. 覆盖与例外

| 变量 | 作用 | 默认 |
| --- | --- | --- |
| `RDS_HOME` | 覆盖全部派生路径的根 | 见 §2 |
| `RDS_TEMP_DIR` | 单独覆盖临时目录（放到机械盘/网络盘会拖慢 DuckDB spill） | `<RDS_HOME>/tmp` |
| `RDS_KNOWN_HOSTS` | SSH known_hosts（**默认仍用 `~/.ssh/known_hosts`**：属用户资产，跨应用共用） | 用户主目录 |
| `RUST_LOG` / 既有日志开关 | 不变 | — |

## 4. 实施要点（低成本的关键做法）

1. **新建 `crates/paths`**（无重依赖：仅 `std` + 可选 `dirs`），导出 §2 的 6 个函数；所有 crate 依赖它。
2. **启动最早处设置进程 `TEMP`/`TMPDIR`**（`crates/app/src/main.rs` 第一行）：一次覆盖所有
   `std::env::temp_dir()` 调用点（DuckDB spill、联邦临时库、各处 scratch）——**改动面从几十处降到 1 处**。
   注意：`set_var` 必须在任何线程/运行时启动前调用（Rust 2024 中 `set_var` 已是 `unsafe`）。
3. 替换 9 处硬编码 `"RdataStation"` 字面量为 `paths::*` 调用。
4. 兼容与迁移：启动时若新路径为空且旧路径有数据 → 提示并**一次性迁移**（或只提示路径变更 + 提供开关）。
5. **一律不提交**：`.gitignore` 加 `/.rds/`、`/rds-*.log`（数据/日志/临时不入库）。
   注意：`*.fossil` 是**测试用的 SQLite 库**（非生成物），不要加入忽略规则。
   已完成：`docs/tmp/*.log`、`tools/r2*.log` 等**已被跟踪**的日志需 `git rm --cached`（保留工作区文件）——
   属索引操作，等仓库安静时做（见 §6）。
6. 文档同步：本文档 + `settings/*`（settings.json 位置）、`database/*`（sqlite/duckdb 位置）、
   `overview.md`（双层数据落盘位置）、`connection/*`（known_hosts 例外）。

## 5. 测试与风险

| 风险 | 处理 |
| --- | --- |
| 测试大量用 `env::temp_dir()`（各 crate 的 `rds_*` 前缀临时目录） | **测试不改**（仍用系统临时目录，避免 `set_var` 并发与污染）；只要求生产代码走 `paths::temp_dir()`。Rust 2024 下测试内 `set_var` 是 `unsafe` 且与并行测试冲突，不值得 |
| DuckDB spill 放到项目盘影响性能 | `RDS_TEMP_DIR` 单独覆盖；文档写明取舍 |
| 安装到 `Program Files`（目录不可写） | 启动探测可写性 → 回退 `%LOCALAPPDATA%/RdataStation` + 日志提示 |
| 开发时 `cargo clean` 清掉数据 | 文档说明；开发期用 `RDS_HOME=<repo>/.rds` |
| 旧数据"看起来丢失" | 启动时检测旧路径并提示/迁移（§4.4） |
| 安装到只读目录 | `paths::home()` 失败时回退：`%LOCALAPPDATA%/RdataStation`（并在日志中提示），保持"能用" |

## 6. git 卫生现状（待处理）

`git ls-files` 实测：**已有生成物被跟踪**（均应 `git rm --cached`，保留工作区文件）：

- `docs/tmp/app.out.log`、`docs/tmp/app.err.log`
- `tools/r2*.log`（`r21_test` / `r22_*` / `r23_*` 等一批测试日志）

未跟踪的生成物（应加入忽略规则或删除）：

- 仓库根：`rds-a.log` / `rds-c.log` / `rds-e.log` / `rds-i.log` / `rds-si.log` / `rds-sw.log` / `rds-t.log` / `rds-w*.log`
- 其他：`commit_msg_b3.txt`（提交信息临时文件）、`crates/workbench/data123`（先 `file`/`head` 看一眼是测试数据还是废物）
- **不是生成物、不要删也不要忽略**：`crates/workbench/FossilTT.fossil`（测试用 SQLite 库）等 `*.fossil`

建议动作顺序（等当前并行会话停下来再做，避免动索引）：

1. `.gitignore` 补规则（本文件 §4.5）；
2. `git rm --cached docs/tmp/*.log tools/r2*.log`（一次提交，说明"生成物不入库"）；
3. 删除工作区里的临时件（`commit_msg_*.txt`、`rds-*.log`；`data123` 先 `file` 看一眼再定）；
   `*.fossil` 属测试资产，**不动**。

## 7. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 唯一路径解析点 | `crates/paths/src/lib.rs`（新增 crate：`home/config_dir/data_dir/log_dir/temp_dir/extensions_dir`） |
| 进程临时目录重定向 | `crates/app/src/main.rs`（启动最早处设 `TEMP`/`TMPDIR`） |
| 设置路径 | `crates/settings/src/lib.rs` |
| 全局数据 / 密钥库 | `crates/engine/src/migration/global_init.rs`、`crates/engine/src/persistence/*`、`crates/shared/src/crypto.rs` |
| 日志目录 | `crates/engine/src/logging/config.rs` |
| 系统库 / 分析库 | `crates/project/src/ui.rs`、`crates/workbench/src/services/workspace_loader.rs` |
| 忽略规则与"不提交" | `/.gitignore`（`/.rds/`、`/rds-*.log`）+ 已跟踪日志的 `git rm --cached` 清单（§6）；`*.fossil` 是测试库，不忽略 |
| 插件目录 / 插件数据 | `crates/engine/src/driver/loader.rs`（`WasmDriverDiscovery::plugin_dirs`）、`crates/plugin/src/{manager,manifest,permission}.rs`、`crates/plugin/src/wasm/plugin_manager.rs`、`crates/plugin/src/sidecar/{manager,driver}.rs`（见 §8） |

## 8. 插件系统对路径设计的影响（全面分析）

插件不是单一形态：`crates/plugin` 共 7488 行，含 **wasm（extism 1.30 → wasmtime）**、**sidecar（独立进程）**、
manifest、permission 四个子系统；驱动侧还有 `engine/src/driver/wasm/`。路径约束必须覆盖它们，
否则“生成物都在安装目录下”会被插件绕过。

### 8.1 现状（实测）

| 项 | 现状 | 问题 |
| --- | --- | --- |
| 插件发现目录 | `./plugins`（**相对当前工作目录**）+ `~/.rdatastation/plugins` | `engine/src/driver/loader.rs:135`；前者随启动目录漂移，后者写 C 盘；且 `~/.rdatastation` 小写风格与现有 `RdataStation` 不一致 |
| 插件注册表 | `global.sqlite` 的 `plugin_store`（`manifest_json` 等） | ✅ 随 `RDS_HOME` 自动迁移 |
| WASM 运行时 | extism 1.30（wasmtime）；`wasm/plugin_manager.rs` **未见** cache/data 目录配置 | wasmtime 编译缓存可能落 C 盘（如 `~/.cache`）；需显式指向插件缓存目录 |
| Sidecar 插件 | `sidecar/manager.rs`：`Command::new` 起独立进程，**从 stdout 读端口号** | ① **真实占用本地端口**（需保留段 + 冲突重试 + 退出回收）；② 未设子进程 `current_dir`、日志与临时目录；③ 子进程崩溃/残留需清理 |
| 权限模型 | `permission.rs`（373 行） | 插件可申请的**路径权限**必须与“只能写自己目录”的约束一致，否则插件能绕开本设计写 C 盘 |
| 文档 | `docs/` 下**无插件架构文档** | 7488 行、4 个子系统，无设计文档（见 §8.3） |

### 8.2 目标形态（并入 §2 的同一根）

```
<RDS_HOME>/
├── plugins/                   ← 已安装插件（wasm 或 sidecar 二进制 + manifest）
│   └── <plugin-id>/{plugin.wasm | sidecar.exe, manifest.json}
├── plugin-data/<plugin-id>/   ← 插件私有数据（沙箱内唯一可写区，卸载后可保留）
├── plugin-cache/<plugin-id>/  ← wasmtime/extism 编译缓存、sidecar 日志与临时文件
└── tmp/sidecar/<plugin-id>/   ← sidecar 进程的 current_dir 与工作文件
```

规则：

1. 插件**只允许**写 `plugin-data/<id>/` 与 `plugin-cache/<id>/`（由 `permission.rs` 的路径白名单强制）；
   一律不得写 CWD、用户主目录、C 盘（插件若声明更多路径，安装时明确提示用户）。
2. `plugin_dirs` 改为 `paths::plugins_dir()`：去掉 `./plugins` 的 CWD 依赖，去掉 `~/.rdatastation`；
   如需支持“随身插件目录”（绿色版），另给 `RDS_PLUGIN_DIRS` 覆盖。
3. extism/wasmtime 显式配置 cache 目录 → `plugin-cache/<id>/`（避免写 `~/.cache`）。
4. sidecar：`Command::current_dir(paths::sidecar_work_dir(id))`、stdout/stderr 落
   `plugin-cache/<id>/sidecar.log`、端口从**保留段**（建议 41000–41999）分配并在超时/子进程退出时回收。
5. 卸载插件 = 删除 `plugins/<id>`；`plugin-data/<id>` 是否保留由 manifest 声明（默认保留）。

### 8.3 待补文档

插件系统（manifest / permission / wasm / sidecar）**缺自己的架构文档**。建议新增
`docs/architecture/plugin/plugin-architecture.md`，至少覆盖：清单与版本/依赖解析、权限模型、
wasm 与 sidecar 两种运行形态的生命周期、路径与进程约束（引用本文档 §8）。
