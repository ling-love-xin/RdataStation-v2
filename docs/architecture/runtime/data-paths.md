# 运行时数据路径（配置 / 数据 / 临时 / 日志）

状态：**已实施**（2026-09-16 设计并落地，代码见 `crates/paths`；实施记录与偏差见 §10）。
动机：默认路径全部落在 C 盘（`%APPDATA%` / `%TEMP%` / `~`），开发机 C 盘空间紧张；
且当时的现状没有单一解析入口，路径散落在 7 个 crate 里**各自拼字符串**。

**已确认的口径**：① 这个软件生成的**任何信息**（配置 / 数据 / 日志 / 临时 / 缓存 / 扩展）都待
在软件自己的目录下，**默认 = 软件的安装目录**（可执行文件所在目录）；② 这些生成物**一律不进 git**。

## 1. 改造前的现状（实测，已全部替换）

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
└── extensions/                 # DuckDB 引擎扩展（内部再按内核版本分子目录，如 v1.5.5/；
                              #   与应用插件的 plugins/ 不是一回事，见 §8.2 第 6 条）
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
| ~~`RDS_KNOWN_HOSTS`~~ | SSH known_hosts（**默认仍用 `~/.ssh/known_hosts`**：属用户资产，跨应用共用）。⚠ **本次未实现**（`known_hosts.rs` 未改，见 §9.3）；要用请先落地读取 | 用户主目录 |
| `RUST_LOG` / 既有日志开关 | 不变 | — |

## 4. 实施要点（低成本的关键做法）

> 下列 6 条均已落地；具体文件与偏差见 §10。

1. **新建 `crates/paths`**（无重依赖：仅 `std` + `dirs`），导出 §2 的 6 个函数；所有 crate 依赖它。
2. **启动最早处设置进程 `TEMP`/`TMPDIR`**（`crates/app/src/main.rs` 第一条语句）：一次覆盖所有
   `std::env::temp_dir()` 调用点（DuckDB spill、联邦临时库、各处 scratch）——**改动面从几十处降到 1 处**。
   注意：`set_var` 必须在任何线程/运行时启动前调用（Rust 2024 中 `set_var` 已是 `unsafe`）。
3. 替换 9 处硬编码 `"RdataStation"` 字面量为 `paths::*` 调用。
4. 兼容与迁移：启动时若新路径为空且旧路径有数据 → 提示并**一次性迁移**（或只提示路径变更 + 提供开关）。
   **已定为：自动迁移、只补不盖、复制不移动、标记文件一次性**（理由见 §10.1）。
5. **一律不提交**：`.gitignore` 加 `/.rds/`、`/rds-*.log`、`/docs/tmp/*.log`、`/tools/*.log`（数据/日志/临时不入库）。
   注意：`*.fossil` 是**测试用的 SQLite 库**（非生成物），不要加入忽略规则。
   已跟踪的日志已 `git rm --cached`（保留工作区文件），结果与坑见 §6。
6. 文档同步：本文档 + `settings/*`（settings.json 位置）、`database/*`（sqlite/duckdb 位置）、
   `overview.md`（双层数据落盘位置）、`connection/*`（known_hosts 例外）。

## 5. 测试与风险

| 风险 | 处理 |
| --- | --- |
| 测试大量用 `env::temp_dir()`（各 crate 的 `rds_*` 前缀临时目录） | **测试代码不改**（不引入 `set_var`），而是由 `.cargo/config.toml` 把 `TEMP`/`TMP`/`TMPDIR` 钉到 `<repo>/.rds/tmp`（**必须 `force = true`**：`[env]` 默认不覆盖环境里已存在的变量，而 `TEMP` 天生就存在）——一行代码不改，测试垃圾就从系统盘挪到仓库内。历史积压用 `tools/clean-temp.sh` 清（默认 dry-run） |
| 测试与风险 | 实际做法 |
| --- | --- |
| **测试会往产品数据根写**（密钥库 / 草稿库 / SQL 历史 / 元数据缓存都要落 `paths::*`） | **已由构建期隔离解决**（2026-09-16 续作）：见下方「测试数据根隔离」 |
| DuckDB spill 放到项目盘影响性能 | `RDS_TEMP_DIR` 单独覆盖；文档写明取舍 |
| 安装到 `Program Files`（目录不可写） | 启动探测可写性 → 回退 `%LOCALAPPDATA%/RdataStation` + 日志提示 |
| 开发时 `cargo clean` 清掉数据 | 已解决：`.cargo/config.toml` 把开发期的 `RDS_HOME` 钉到 `<repo>/.rds`（§10.1） |
| 旧数据"看起来丢失" | 启动时检测旧路径并提示/迁移（§4.4）：**已实现自动迁移**，标记文件 `<<RDS_HOME>>/.migrated-from-legacy` 记录已迁项 |
| 安装到只读目录 | `paths::home()` 探测可写性失败时回退：`%LOCALAPPDATA%/RdataStation`（并在 stderr 提示），保持"能用" |

### 测试数据根隔离（2026-09-16）

**问题**：测试会调用产品代码里的**全局路径**（密钥库、草稿库、SQL 历史、元数据缓存），
默认落到产品数据根；因为旧布局迁移是"只补不盖"，这些垃圾还会把真数据挡在门外
（实测发现一个只有 `connection_drafts` 表的 `system/global.db` 就是这么来的）。

**为什么不逐个测试注入**：全仓几十个测试文件、以后还会加；靠人记得必漏。

**做法（构建期隔离）**：`paths` 新增 `test-support` feature；一旦开启，**测试构建的数据根
自动换成进程专属临时目录**（`RDS_TEST_HOME` 可定向），无需逐测试改动。

| 环节 | 位置 |
| --- | --- |
| 隔离逻辑（`test_root` / `pin_root` / `HomeOrigin::TestRoot`） | `crates/paths/src/lib.rs` |
| feature 声明 | `crates/paths/Cargo.toml`（`test-support`） |
| 各成员打开 feature | 各自的 `[dev-dependencies]`：`paths = { workspace = true, features = ["test-support"] }`（仅测试构建生效，`cargo build/run/release` 拿不到） |
| 防漏开的静态契约 | `crates/paths/tests/test_support_is_wired.rs`（每个依赖 `paths`/`engine`/`shared` 的成员都必须开；漏开在运行时看不出来，所以用扫描兜住） |
| 自检断言 | `crates/paths/src/tests.rs::test_build_is_isolated_from_the_product_root`（隔离根必须在临时目录下，不能是安装目录 / 开发根） |

**验证口径**：清空 `<repo>/.rds/data`（与 `config`/`logs`）后跑一遍全量测试，它**应该仍然是空的**——
垃圾会全部落在 `<repo>/.rds/tmp/rds-test-root-<pid>/`（仓库内，已忽略，`tools/clean-temp.sh` 清）。

**两个例外要知道**：

1. `cargo run --example ...` 用的是带 dev-dependencies 的构建，因此也拿到隔离根；example 若要写
   **开发数据根**（如 `seed_demo`），需自己 `paths::pin_root(RDS_HOME)`（已接）。
2. 早期版本跑出来的测试产物仍可能躺在 `<repo>/.rds/data`（密钥 / 空 `global.db` / `sql_history.json`）——
   真机启动前删掉即可，别让它们被"只补不盖"的迁移当成已存在。

## 6. git 卫生（已处理，2026-09-16）

### 6.1 已经做完的

| 动作 | 结果 |
| --- | --- |
| `.gitignore` 补规则 | `/.rds/`、`/rds-*.log`、`/docs/tmp/*.log`、`/tools/*.log` |
| 已跟踪的日志退出跟踪 | `git rm --cached`：`docs/tmp/app.{out,err}.log` + `tools/*.log`（共 95 个），**工作区文件保留** |
| 工作区临时件 | `commit_msg_b3.txt` / `crates/workbench/data123` 已不存在（早前已清）；根目录的 `rds-*.log`（上一会话的编译错误堆）保持在盘上且已被忽略 |

### 6.2 两条不要踩的线

1. **`git rm --cached` 后不能用 `git commit -- <路径>`**：带路径的提交取的是**工作区**内容，
   而 `--cached` 就是"索引删、工作区留"——两者相遇的结果是**删除被默默吞掉**（提交成功、文件仍在库里）。
   实测确认。要保留工作区文件时，得把文件先移出工作区再带路径提交（本次采用），或改用临时索引
   （`GIT_INDEX_FILE=<临时文件> git read-tree HEAD` 后操作）。
2. **`*.fossil` 是测试资产，不是生成物**：`crates/workbench/FossilTT.fossil` 是测试用 SQLite 库，
   **不要删也不要加入忽略规则**。

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
| 插件目录 / 插件数据 | `crates/engine/src/driver/loader.rs`（`WasmDriverDiscovery::plugin_dirs`）、`crates/plugin/src/{manager,manifest,permission}.rs`、`crates/plugin/src/wasm/plugin_manager.rs`、`crates/plugin/src/sidecar/manager.rs`、`crates/paths/src/lib.rs`（已落地，见 §8） |

## 8. 插件系统对路径设计的影响（全面分析）

插件不是单一形态：`crates/plugin` 现为 4395 行（P0 后；删了 516 行死代码），含 **wasm（extism 1.30 → wasmtime）**、**sidecar（独立进程）**、
manifest、permission 四个子系统；驱动侧还有 `engine/src/driver/wasm/`。路径约束必须覆盖它们，
否则“生成物都在安装目录下”会被插件绕过。

> **更新（2026-09-20，M9 P0）**：本节 §8.2 列的目标目录**已全部落地**。
> `paths` 新增 `plugins_dir()` / `plugin_dir(id)` / `plugin_data_dir(id)` / `plugin_cache_dir(id)` /
> `sidecar_work_dir(id)` / `plugin_registry_dir()`（+ `validate_plugin_id` 白名单），并已登记进
> `NEW_LAYOUT_DIRS`（否则旧布局迁移会把 `plugins/` 当旧数据搬走）。插件文档见 `../plugin/README.md`。

### 8.1 现状（实测）

| 项 | 现状 | 问题 |
| --- | --- | --- |
| 插件发现目录 | `./plugins`（**相对当前工作目录**）+ `~/.rdatastation/plugins` | `engine/src/driver/loader.rs:135`；前者随启动目录漂移，后者写 C 盘；且 `~/.rdatastation` 小写风格与现有 `RdataStation` 不一致 |
| 插件注册表 | `global.sqlite` 的 `plugins` 表（`manifest_json` 等；`plugin_store` 只是 Rust 模块名） | ✅ 随 `RDS_HOME` 自动迁移 |
| WASM 运行时 | extism 1.30（wasmtime）；`wasm/plugin_manager.rs` **未见** cache/data 目录配置 | wasmtime 编译缓存可能落 C 盘（如 `~/.cache`）；需显式指向插件缓存目录 |
| Sidecar 插件 | `sidecar/manager.rs`：`Command::new` 起独立进程，**从 stdout 读端口号** | ① **真实占用本地端口**（需保留段 + 冲突重试 + 退出回收）；② 未设子进程 `current_dir`、日志与临时目录；③ 子进程崩溃/残留需清理 —— ①②已由 P1 的 `sidecar/process.rs` 解决（**stdio 分帧，不再占端口**），旧 `manager.rs` 待删 |
| 权限模型 | `permission.rs`（P0 后四轨：`Frontend`/`Wasm` 门控 + `Sidecar`/`Driver` 展示轨） | 插件可申请的**路径权限**必须与“只能写自己目录”的约束一致，否则插件能绕开本设计写 C 盘 |
| 文档 | ✅ **已有**：`../plugin/` 五件（入口 / 架构 / 开发方案 / 原型设计 / 使用手册）+ 5 张可交互原型 | —（原“docs/ 下无插件架构文档”已失效） |

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
4. sidecar：`Command::current_dir(paths::sidecar_work_dir(id))`、stderr 落
   `plugin-cache/<id>/sidecar.log`（**stdout 是协议通道，不进日志**）。
   **不再有端口**：传输改成 stdio 二进制分帧（`../plugin/plugin-dev-plan.md` §4.2.1），
   子进程靠「宿主持有 stdin 管道、见 EOF 即退」自收场，无需杀进程组。
   已落地：`crates/plugin/src/sidecar/process.rs`。
5. 卸载插件 = 删除 `plugins/<id>`；`plugin-data/<id>` 是否保留由 manifest 声明（默认保留）。
6. **引擎扩展（DuckDB 扩展）与应用插件分开**：`extensions/` 是 **DuckDB 自己的**扩展目录
   （`extension_directory`，内部再按内核版本分 `v<版本>/`，可离线预置），与应用插件的
   `plugins/` 不同层；**心智统一（都是“全局装、项目引用”）、物理不混**，
   详见 `../plugin/plugin-architecture.md` §7 与 §7.1。已接线：所有长期存活的 DuckDB 连接
   统一过 `duckdb/manager.rs::configure_connection`（扩展目录 + 内存闸 + 溢写口 + 关掉静默联网）。

### 8.3 插件文档

✅ **已建**（2026-09-20）：`docs/architecture/plugin/` 五件 —— `README.md`（模块入口）/ `plugin-architecture.md`（设计意图）/ `plugin-dev-plan.md`（开发方案：决策、阶段、验收、实现位置映射）/ `plugin-prototype-design.md`（进程边界、清单、`rds` API 面、贡献点）/ `plugin-user-guide.md`（使用手册），
外加 5 张可交互原型（`plugin/prototype/`，含插件入口与驱动 sidecar）。本节 §8 与它们冲突时**以 §8 的路径约束为准**，其余以插件目录为准。

## 9. W2 逐文件改动清单（执行参照：已全部执行完，结果与偏差见 §10.2）

前置：W1 已产出 `crates/paths`（`home/config_dir/data_dir/log_dir/temp_dir/extensions_dir` +
可写性探测 + 回退）与 `paths::install_process_temp_dir()`。

### 9.1 替换清单（12 处 / 8 个文件）

| # | 文件:行 | 现状 | 改成 | 备注 |
| --- | --- | --- | --- | --- |
| 1 | `crates/settings/src/lib.rs:68-80` | `config_dir()`：`%APPDATA%/RdataStation`，否则 `temp_dir()/RdataStation` | `paths::config_dir()` | **保留** `#[cfg(test)] CONFIG_DIR_OVERRIDE` 分支（测试路径注入） |
| 2 | `engine/src/migration/global_init.rs:36-52` | `get_global_data_dir()`：`dirs::data_dir()?/RdataStation` + `create_dir_all` | `let app_dir = paths::data_dir(); create_dir_all(&app_dir)?; Ok(app_dir)` | 删/改写 L18 `GLOBAL_DATA_DIR_NAME` |
| 3 | 同上 `L58+` | `get_system_dir()` = `get_global_data_dir()/system` | **不改**（自动跟随） | `system/`、`global.db`、`analytics.duckdb` 名字不变 |
| 4 | `engine/src/persistence/connection_store.rs:645-652` | `dirs::data_dir()/RdataStation` + `create_dir_all` | `paths::data_dir()` + `create_dir_all` | 存储文件名不变 |
| 5 | `engine/src/persistence/history_store.rs:768-776` | 同上 | 同上 | `sql_history.json` 不变 |
| 6 | `shared/src/crypto.rs:15-25` | `salt_path()`：`dirs::data_local_dir()/RdataStation/encryption-salt` | `paths::data_dir().join("encryption-salt")` | ⚠ **迁移敏感**：不迁移 = 旧密文全部解不开（连接密码失效） |
| 7 | `shared/src/crypto.rs:86-95` | `machine_id_path()`：`…/RdataStation/machine-id` | `paths::data_dir().join("machine-id")` | 同上，需迁移 |
| 8 | `project/src/ui.rs:879-887` | `sample_project_dir()`：`get_system_dir()` + **两处** `%TEMP%` 回退 | 回退改 `paths::data_dir().join("samples")` | 顺带修“回退到临时目录”隐患 |
| 9 | `workbench/src/services/workspace_loader.rs:23-28` | `default_global_dir()`：`get_system_dir()` + `%TEMP%` 回退 | 回退改 `paths::data_dir().join("system")` | 同 #8；`global_analysis_db_path()` 靠它跟随 |
| 10 | `engine/src/duckdb/manager.rs:297-304` | `extensions_dir()`：`dirs::home_dir()/<DUCKDB_EXTENSIONS_DIR>` | `paths::extensions_dir()` | 旧位置兼容读取或迁移（W3） |
| 11 | `engine/src/dbi/engine/duckdb_engine.rs:505-509` | `init_extensions(conn, data_dir)`：`{data_dir}/duckdb/extensions` | 传参改走 `paths::extensions_dir()` | ✅ 已执行，但**改了签名**（参数删掉，理由见 §10.2）。**后续（2026-09-19）**：`dbi` 层整体删除，此处现为 `duckdb/manager.rs::configure_connection` 设 `extension_directory` |
| 12 | 日志目录：`LogConfig::with_log_dir(...)` 的**调用方**（在 `crates/app`） | `Default` 里 `log_dir: PathBuf::from("")`，目录由调用方传入 | 调用方改传 `paths::log_dir()` | ✅ 已执行（改为 `Default` 直接取 `paths::log_dir()`；**无调用方**这一事实见 §10.2） |

### 9.2 由“启动重定向 TEMP/TMPDIR”自动覆盖（不改代码）

| 位置 | 说明 |
| --- | --- |
| `engine/src/duckdb/executor.rs:321`、`federation.rs:361,410,454`、`manager.rs:403,522`、`snapshot.rs:353` | 直接用 `env::temp_dir()`；重定向后自动落 `<RDS_HOME>/tmp`（DuckDB spill / 联邦临时库） |
| 各 crate 测试里的 `rds_*` 临时目录（`analytics_resource` / `connection` / `database` / `editor` / `engine`） | **保持系统临时目录**（测试不改；Rust 2024 下 `set_var` 是 `unsafe` 且与并行测试冲突） |

### 9.3 明确不改

| 位置 | 理由 |
| --- | --- |
| `connection/src/known_hosts.rs:57`（`~/.ssh/known_hosts`） | 跨应用共用的用户资产；需要时用 `RDS_KNOWN_HOSTS` 覆盖 |
| `crates/plugin/*`、`engine/src/driver/loader.rs:135`（插件目录 / wasm 缓存 / sidecar） | **三期 P3-a**，见 `../plugin/plugin-architecture.md` §6。`loader.rs:177` 的 `~/.rdatastation/jdbc-drivers` 同族（空实现） |
| `<用户项目>/{project.db, analytics.duckdb}`（M1 项目会话） | 用户资产，留在用户工程目录 |

### 9.4 启动顺序插入点（W1 的关键一步）

`crates/app/src/main.rs`：**在所有路径解析之前**插入：

```rust
paths::install_process_temp_dir();   // 内部：create_dir_all(<RDS_HOME>/tmp) + set_var("TEMP"/"TMPDIR")
```

- 位置：必须早于 `init_global_system()`（`main.rs:56`）与 `SettingsService::init(cx)`（`:65`）
- 约束：`set_var` 在 Rust 2024 是 `unsafe`，必须在**单线程阶段**调用——若 `fn main()` 开头已建 tokio/gpui 线程，需前移到 `main()` 第一条语句
- 验收：`RDS_HOME=<临时目录> cargo run -p rds-app`，启动后该目录下应出现 `data/`、`logs/`、`tmp/`；且 `%TEMP%` 下不再新增 `RdataStation` 相关目录

## 10. 实施记录（2026-09-16）

### 10.1 实施时定的三个口径

| 决策 | 结论 | 理由 |
| --- | --- | --- |
| 旧数据怎么办 | **自动迁移**：启动时只补不盖、复制不移动、迁完写标记文件。**例外：密钥类文件（`encryption-salt` / `machine-id`）允许覆盖** | ① `encryption-salt` / `machine-id` 不跟着搬 = 存量连接密码全部解不开（静默故障，用户只会看到"密码不对"）；② 提示式迁移要先做 UI 与阻塞流程，收益不抵成本；③ 复制不移动 → 出问题可回滚；④ 密钥文件必须覆盖：首次迁移时**旧位置那份才是有数据在用的钥匙**，新位置即便有文件也只可能是垃圾（开发机上跑过 `cargo test` 就会在那里落一份随机盐） |
| DuckDB spill 跟不跟走 | **跟走**（`<RDS_HOME>/tmp`），`RDS_TEMP_DIR` 可单独覆盖 | 与"生成物都在软件目录下"同一口径；放机械盘/网络盘会拖慢 spill，留覆盖口 |
| 开发期数据放哪 | `.cargo/config.toml` 的 `[env] RDS_HOME = { value = ".rds", relative = true }` | 发布版默认 = 安装目录；开发时那是 `target/debug`，一次 `cargo clean` 就把 global.db / 密钥库 / 设置全清掉。钉到仓库根 `.rds/`（已忽略）。**不开 `force`**，命令行 `RDS_HOME=<X> cargo run` 仍然优先 |

### 10.2 落地清单

| 内容 | 位置 |
| --- | --- |
| 唯一解析点 + 可写性探测 + 回退 | `crates/paths/src/lib.rs`（`home` / `config_dir` / `data_dir` / `log_dir` / `temp_dir` / `extensions_dir`、`ensure_dirs`、`install_process_temp_dir`、`home_origin`、`summary`） |
| 旧位置定义（**只给迁移用**） | `crates/paths/src/legacy.rs` |
| 一次性迁移（只补不盖 + 密钥覆盖例外 + 标记文件） | `crates/paths/src/migrate.rs`（`SECRET_FILES` 是唯一例外） |
| 启动最早处：TEMP 重定向 → 建目录 → 迁移 → 打印数据根 | `crates/app/src/main.rs::main` 的前三步 |
| 单元测试（8 条：派生同根 / 迁移路由 / 只补不盖 / **密钥覆盖** / 递归复制） | `crates/paths/src/tests.rs` |
| 依赖登记 | 根 `Cargo.toml`（`paths = { path = "crates/paths", package = "rds-paths" }`）；`app` / `engine` / `shared` / `settings` / `project` / `workbench` 六处 `paths.workspace = true` |
| 开发期数据根 / 临时目录的钉住 | `.cargo/config.toml` 的 `[env]`：`RDS_HOME = .rds`（不开 `force`，命令行可用）、`TEMP`/`TMP`/`TMPDIR = .rds/tmp`（**必须 `force`**） |
| 临时目录积压清理 | `tools/clean-temp.sh`（默认 dry-run；只碰 `$TEMP` 顶层 `rds_*` 条目，`RdataStation*` 明确不碰） |
| 测试数据根隔离 | `crates/paths` 的 `test-support` feature + 各成员 `[dev-dependencies]` 打开 + 契约测试 `crates/paths/tests/test_support_is_wired.rs`（见 §5） |

替换的 12 处见 §9.1。**实施中的两处偏差 + 一处实测发现的坑**：

1. **#11 `init_extensions` 改了签名**：原计划"传参改走 `paths::extensions_dir()`"，实际把 `data_dir: &str`
   参数**删掉**（函数内直接取 `paths::extensions_dir()`）。原因：唯一调用方 `DuckDbService::accelerate_query`
   的 `data_dir: Option<&str>` 会让"参数为 `None` 时扩展目录根本不设"；该 API 全仓无调用方，留一个已经
   失去意义的参数比删掉更坏（`accelerate_query` 的 `data_dir` 参数一并删除）。
   顺带修好一处真 bug：**扩展目录原本有两份**（`DuckDBManager::extensions_dir()` 用
   `~/.rdatastation/duckdb/extensions`，而 `init_extensions` 用 `{data_dir}/duckdb/extensions`），
   现在两处都走 `paths::extensions_dir()`。
2. **#12 日志目录没有"调用方"**（2026-09-16 当日**已接线**）：初次改造时发现
   `LogConfig::with_log_dir` / `init_logging` **全仓无调用方**（只有定义与文档注释），
   所以不存在"调用方改传 `paths::log_dir()`"这一步。当时做法：
   - `LogConfig::default()` 的 `log_dir` 从 `PathBuf::from("")` 改为 `paths::log_dir()`（谁构造谁就对）；
   - 启动时由 `paths::ensure_dirs()` 建好 `<RDS_HOME>/logs/`。

   **当日续作已把子系统接上**（详见 `logging.md`）：新增 `engine::init_app_logging()`，
   由 `crates/app/src/main.rs::init_global_system` 在全局库建立之后调用
   （库层要写 `app_logs` 表 + 起异步消费者，所以不能更早），并在接线后补记一条启动结果。
   接线时一并补齐两个脱敏缺口：文件层改为**逐行脱敏**写盘（原先明文直写），
   库层的**字段值**也过 `redact_sensitive`（原先只脱敏 message，而连接串大多作为字段进来）。
3. **实测发现的坑：`cargo test` 会往产品数据根写密钥**。验收时发现 `<repo>/.rds/data` 里
   已经多了 `encryption-salt` / `machine-id`——测试跑的是产品代码里的全局便捷路径（`encrypt_password`
   等），于是它们在数据根生成了一份**跟任何密文都没关系**的随机盐。此时"只补不盖"会让真正的旧密钥
   永远迁不进来，表现就是**所有已保存的连接密码突然解不开**（且完全静默）。
   修法：把密钥类文件定为"只补不盖"的唯一例外（[`SECRET_FILES`]，覆盖式迁移）。
   为什么覆盖是对的：首次迁移时**旧位置那份才是有数据在用的钥匙**，新位置即便有文件也只可能是
   垃圾（测试生成的盐 / 半途而废的尝试）。标记文件保证迁移只跑一次，所以这个"覆盖"不会反复发生。

### 10.3 验证

| 命令 | 结果 |
| --- | --- |
| `cargo check --all-targets` | 0 error / 0 warning（app 依赖图） |
| `cargo check -p rds-engine -p rds-settings -p rds-shared -p rds-project -p rds-workbench -p rds-paths --all-targets` | 通过（含各 crate 的测试目标） |
| `cargo test -p rds-paths` | 8 / 8 |
| `cargo test -p rds-engine --lib` | 310 / 310（23 ignored）；接通日志后 316 / 316（+6：脱敏 4 + 文件层 2） |
| `cargo test -p rds-shared -p rds-settings --lib` | 22 / 22、21 / 21 |
| `cargo test -p rds-project --lib` | 29 / 29 |
| `cargo test -p rds-workbench --lib panels::` / `--test ui_contract` / `--test dialog_host_layer` | 10 / 10、7 / 7、4 / 4 |

测试临时目录收进仓库后另测：`cargo test -p rds-engine --lib persistence::connection_draft_store`
建的两个 `rds_draft_store_*` 出现在 `<repo>/.rds/tmp`，系统 TEMP 的 `rds_*` 计数不增（以前每跑一次多几个）；
历史积压 2583 个条目 / 1.17 GB 已由 `tools/clean-temp.sh` 清掉（系统 Temp 1.7 GB → 528 MB）。

**§9.4 的真机验收已于当日完成**（`cargo build -p rds-app` 后独立 `RDS_HOME` 跑 30 秒），实测：

| 验收项 | 实测 |
| --- | --- |
| 五个派生目录 | `<RDS_HOME>/{config,data,logs,tmp,extensions}` 均出现 |
| 旧数据迁移 | `复制 26 项（跳过 0 项）`，含 `settings.json` / `system/` / `samples/` / 密钥库 |
| 密钥迁移 | `encryption-salt` / `machine-id` 与 `%LOCALAPPDATA%` 那份 **md5 完全一致**（存量连接密码不会失效） |
| 文件日志 | `logs/app.2026-09-16` 有内容，与 stderr 一致 |
| 库日志 | `app_logs` 7 行，字段 JSON 正常（`[["data_root","…"],["origin","…"]]`） |
| `%TEMP%` | 无新增 `RdataStation*` 目录 |
| 应用本体 | 主题加载、项目库（`<项目>/.RSmeta`）正常打开 |

（`RDS_HOME` 由 `.cargo/config.toml` 钉到仓库根，且 **cargo 会把它转成绝对路径**——这一点很关键：
若它保持相对字符串，二进制会把它解析成"相对当前工作目录"，换目录启动就换数据位置）。

### 10.4 仍未做（明确不在本次范围）

| 项 | 说明 |
| --- | --- |
| 日志查询 UI / 优雅退出 flush / 单文件上限 | 见 `logging.md` §6（日志**已接线**，这三项是后续） |
| 插件目录 / wasm 缓存 / sidecar 工作目录 | 三期 P3-a，见 `../plugin/plugin-architecture.md` §6 |
| `~/.rdatastation/jdbc-drivers`（`driver/loader.rs:177`） | 与 `WasmDriverDiscovery` 同族，且 `JdbcDriverDiscovery::load_drivers` 是空实现（返回空 Vec）——随 P3-a 一起定 |
| `~/.ssh/known_hosts` | 有意不改（跨应用用户资产）；§3 的 `RDS_KNOWN_HOSTS` 覆盖**未实现** |
| 测试代码里的 `env::temp_dir()` | **不改代码**（不引入 `set_var`）：由 cargo 把 `TEMP` 钉到 `.rds/tmp`（§5）。不经 cargo 跑的场景（直接跑测试二进制）仍会落系统临时目录，积压用 `tools/clean-temp.sh` 清 |
