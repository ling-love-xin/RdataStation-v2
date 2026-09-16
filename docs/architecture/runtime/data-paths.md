# 运行时数据路径（配置 / 数据 / 临时 / 日志）

状态：**设计待实施**（2026-09-16）。动机：默认路径全部落在 C 盘（`%APPDATA%` / `%TEMP%` / `~`），
开发机 C 盘空间紧张；且现状没有单一解析入口，路径散落在 7 个 crate 里**各自拼字符串**。

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

**默认值规则（关键决策点）**：`RDS_HOME` 未设时取 `可执行文件所在目录的上一级 + /.rds`：

- 开发运行（`target/debug/rds-app.exe`）→ `<repo>/target/debug/../../.rds` = `<repo>/.rds`（不占 C 盘）
- 发行安装 → `<安装目录>/.rds`（随安装位置；若装在 C 盘再用 `RDS_HOME` 覆盖）

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
5. `.gitignore` 加 `/.rds/`（数据与日志不入库）；`.rds/` 内再放 `.gitignore` 兜底。
6. 文档同步：本文档 + `settings/*`（settings.json 位置）、`database/*`（sqlite/duckdb 位置）、
   `overview.md`（双层数据落盘位置）、`connection/*`（known_hosts 例外）。

## 5. 测试与风险

| 风险 | 处理 |
| --- | --- |
| 测试大量用 `env::temp_dir()`（各 crate 的 `rds_*` 前缀临时目录） | **测试不改**（仍用系统临时目录，避免 `set_var` 并发与污染）；只要求生产代码走 `paths::temp_dir()`。Rust 2024 下测试内 `set_var` 是 `unsafe` 且与并行测试冲突，不值得 |
| DuckDB spill 放到项目盘影响性能 | `RDS_TEMP_DIR` 单独覆盖；文档写明取舍 |
| 旧数据"看起来丢失" | 启动时检测旧路径并提示/迁移（§4.4） |
| 安装到只读目录 | `paths::home()` 失败时回退：`%LOCALAPPDATA%/RdataStation`（并在日志中提示），保持"能用" |

## 6. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 唯一路径解析点 | `crates/paths/src/lib.rs`（新增 crate：`home/config_dir/data_dir/log_dir/temp_dir/extensions_dir`） |
| 进程临时目录重定向 | `crates/app/src/main.rs`（启动最早处设 `TEMP`/`TMPDIR`） |
| 设置路径 | `crates/settings/src/lib.rs` |
| 全局数据 / 密钥库 | `crates/engine/src/migration/global_init.rs`、`crates/engine/src/persistence/*`、`crates/shared/src/crypto.rs` |
| 日志目录 | `crates/engine/src/logging/config.rs` |
| 系统库 / 分析库 | `crates/project/src/ui.rs`、`crates/workbench/src/services/workspace_loader.rs` |
| 忽略规则 | `/.gitignore` |
