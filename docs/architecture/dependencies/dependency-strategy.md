# 依赖治理方案（Cargo 版本唯一入口）

> 目标：把「哪个依赖用哪个版本」收敛到一处，避免每个 crate 各自挑版本、各自升级。
> 本文件是依赖相关决策的唯一说明；实现位置见文末映射表。

## 1. 规则

### R1 版本唯一入口

所有第三方依赖的版本只在根 `Cargo.toml` 的 `[workspace.dependencies]` 声明，crate 内不写版本号。
（`version` / `edition` 同样走 `[workspace.package]` 继承。）

### R2 crate 内写法

```toml
serde.workspace = true                                   # 普通引用
duckdb = { workspace = true, features = ["extra"] }      # 需要额外 feature 时追加
```

- `default-features = false` 与「基线 feature」必须写在根表：workspace 继承的 feature 是**并集**语义，只能加、不能减。
- 内部 crate 同样走本表（如 `shared = { path = "crates/shared", package = "rds-shared" }`），路径只在根出现，依赖键沿用迁移代码里的模块名，保证 `shared::` / `engine::` 等引用不变。

### R3 版本取值

版本号写「当前构建实际使用的最低版本」，取值来自 `Cargo.lock` 的解析结果；同大版本内一律取最新。
先 `cargo update` 让锁文件升到最新兼容版，再把该版本回填到根表，避免两者不一致。

### R4 例外（精确锁定，不随 `cargo update` 漂移）

| 依赖 | 约束 | 原因 |
| --- | --- | --- |
| `gpui-kit` | `0.6.1` | 与 gpui 版本强绑定：`gpui-kit` / `gpui-base` / `gpui-component` / `gpui-kit-assets` **必须同版本**（门面与内部错配会出问题），随 GPUI-kit 发布节奏整套升（当前 0.6.1）。写法是 caret 约束，实际生效版本看 `Cargo.lock` |
| `specta` | `=2.0.0-rc.25` | rc 阶段，11 个 crate 共用；升级需同步核对类型生成链路 |
| `sqlglot-rust` | `=0.10.29` | SQL 转译/生成行为对版本敏感（业务关键，见 §5），升级需跑 SQL 回归 |
| `arrow` | `58.4.0` | 主版本跟随 `duckdb`：两端之间传 `RecordBatch`，必须是同一 arrow，不能单独升 |

## 2. 升级流程

1. 同大版本：`cargo update --dry-run` 查看可升项 → `cargo update` → 把新版本回填根表（若解析结果高于根表声明）。
2. 跨大版本：改根表 + 修代码 → **单独一次提交**，一轮只升一组，便于回滚与定位。
3. 验证：`cargo metadata --no-deps`（manifest 自检，秒级）→ `cargo check --workspace --all-targets`。
4. 提交信息写明升级的依赖与影响面。

## 3. 编译时间

**澄清：把依赖提炼到外层不减少编译时间。** Cargo 按「crate + feature + 目标」去重，同一版本在整张图里只编译一份，声明写在哪不影响构建。提炼省的是维护成本（版本漂移、逐处升级、漏改），不是编译时间。

当前图上确实存在同一 crate 多版本共存（实测 `cargo tree -d`）：`aead` 0.5.2 / 0.6.1、`aes` 0.8.4 / 0.9.3、`aes-gcm` 0.10.3 / 0.11.1、`base64` 0.21.7 / 0.22.1 / 0.23.1、`bitflags` 1.3.2 / 2.13.2、`block-buffer` 0.10.4 / 0.12.1、`rand` 0.8.8 / 0.9.5 / 0.10.2、`toml` 0.8.23 / 0.9.12 / 1.1.5、`thiserror` 1.0.69 / 2.0.20 等。来源基本是第三方（gpui-pre、mysql_common、wasmtime/extism、russh），本仓改不动。

⚠️ 反直觉的实测结论：**升级本仓的直接依赖并不总能减少重复，反而可能多出一份**。实例：`aes-gcm` 原本本仓与 `russh` 共用 0.10.3（图上只有一份），升到 0.11.1 后变成 0.10.3（russh）+ 0.11.1（本仓），连 `aead` / `aes` 也跟着各拆两份，要等 `russh` 升到 0.63 才会合流。反方向的例子同样存在：`dirs` 升到 7.0.0 后，图上已无任何东西需要 5.0.1，等于顺手少了一份。

所以评估升级收益要按 crate 体积算：`dirs`、`aes-gcm`、`thiserror` 这类小 crate 多一份只是秒级开销，按「最新版优先」处理即可；`arrow`、`duckdb`、`wasmtime` 这类大 crate 则必须等上游（`arrow` 只能随 `duckdb` 走）。

真正有效的编译时间手段（按收益排序，前两项已启用）：

| 手段 | 状态 | 说明 / 代价 |
| --- | --- | --- |
| `default-members = ["crates/app"]` | 已启用（根 `Cargo.toml`） | 裸 `cargo build/check/run` 只构建 app 依赖图，跳过 `plugin`（extism → wasmtime，图上最重且不在 app 图上）。代价：`cargo test` 默认不覆盖全部 crate，需显式 `--workspace`；编辑器侧 rust-analyzer 默认仍按整个 workspace 检查，如需一致要在编辑器侧关掉 `check.workspace` |
| `[profile.dev.package."*"] debug = false` | 已启用（根 `Cargo.toml`） | 第三方依赖不产 debuginfo（本仓 crate 保留 `line-tables-only`），降低链接与磁盘开销。代价：依赖内部帧无行号；改动后需一次依赖全量重建 |
| `tokio` feature 收敛（`full` → 按需） | 已评估：不做 | 实测代码用到 `rt` / `rt-multi-thread` / `macros` / `sync` / `time` / `io-util` / `fs` / `net`（未用 `process` / `signal` / `test-util`）。收敛只能省下 tokio 少许可选依赖，却会因 feature 变更让 tokio 及其全部依赖方重新编译一次，收益低于成本 |
| 日常只跑 `cargo check` / `cargo check -p <crate>` | 约定 | 不生成代码、不链接、不产 PDB |
| Windows 启用 `rust-lld` | 待评估 | 链接耗时下降；需 `.cargo/config.toml`，与 MSVC 工具链兼容性需实测 |

## 4. 跨大版本升级（已完成 / 待办）

按「代码使用点」排序（实测 grep 计数），越靠前越重，建议逐个单独提交。

已完成：

- `thiserror` 1.0.69 → 2.0.20、`lru` 0.12.5 → 0.18.4、`similar` 2.7.0 → 3.2.0、`opener` 0.7.2 → 0.8.5（用法简单，未改代码）
- `dirs` 5.0.1 → 7.0.0、`x509-parser` 0.17.0 → 0.18.1（未改代码）
- `aes-gcm` 0.10.3 → 0.11.1（`crates/shared/src/crypto.rs`：`OsRng` 改用 `rand::rngs::OsRng`，`Nonce::from_slice` 改 `Nonce::try_from`，旧的 `aes_gcm::aead::OsRng` 已被移除）
- `toml` 0.8.23 → 1.1.5、`reqwest` 0.12.28 → 0.13.5（未改代码；reqwest 0.13 把 `rustls-tls` feature 改名为 `rustls`）
- `rusqlite` 0.32.1 → **0.40.2**（与 sqlx 联动，见 §5「native 库约束」）。代码改动集中在 `crates/engine/src/persistence/`：
  - `log_store.rs`：3 处 `COUNT(*)` 先取 `i64` 再 `as usize`
  - `sql_template_store.rs`：`created_at_ms` / `updated_at_ms` 写入用 `as i64`，读回用 `row.get::<_, i64>(..)? as u64`
  - `workbench_context_store.rs`：`updated_at_ms` 同上；可空列 `selection_start` / `selection_end`（`Option<usize>`）用 `Option<i64>` + `map`，保持 NULL ↔ None 语义
  - 根因：rusqlite 0.33+ **移除了 `u64` / `usize` 的 `ToSql` / `FromSql`**（SQLite 整数只有 i64，避免越界歧义）
- `mysql_async` 0.34.2 → **0.37.1**（零代码改动；连带 `mysql_common` 升到 0.37.3，并消掉图上的 `base64 0.21.7` 重复）
- `sqlglot-rust` 0.9.25 → **0.10.29**（跨 0.9 → 0.10）。代码改动仅 1 处：`crates/engine/src/sql/builder.rs` 的 `TableRef` 字面量补上新字段 `alias_quote_style`（此处无别名，取 `QuoteStyle::None`）
  - 行为核对：比对两版生成器源码，表名加引号路径完全一致（`write_quoted(&table.name, table.name_quote_style)`），0.10 只新增了**别名**的引号处理；我们的 `TableRef` 无别名，生成结果不变
  - 新增回归用例 `test_generated_sql_is_pinned`：把 CREATE TABLE / DROP TABLE / SELECT / INSERT 的生成结果与引号风格钉死，后续升级越界即失败
- `russh` 0.49.2 → 0.63.3（`russh-keys` 并入 `russh::keys`，依赖项已删除）。代码改动集中在 `crates/connection`：
  - `russh_keys::*` → `russh::keys::*`（含 `#[cfg(unix)]` 的 agent 路径；**Windows 上不参与编译，需在 Linux/macOS 侧补验**）
  - `Handler` 改为原生 async trait（去掉 `#[async_trait]`），`check_server_key` 入参改为 `PublicKeyOrCertificate`（证书形式统一 `.public_key()` 后按公钥校验）
  - `PrivateKeyWithHashAlg::new` 不再返回 `Result`，移除对应的 `map_err` 分支
  - 单测去掉 RNG 依赖：改用两把固定测试公钥（`PublicKey::from_openssh`）—— `rand_core 0.10` 已移除 `OsRng`，且 `PrivateKey::random` 要求的 trait 与 rand 0.8 不兼容；`connection` 的 `dev-dependencies.rand` 随之删除

以上验证：`cargo check-all` 全量通过；`cargo test-all`（全 workspace）**约 430 项全过**（engine 219 / insight 53 / mock 56 / connection 21 / shared 19 / workbench 12 / project 10 / plugin 11，加 workbench 集成测试 19 项）。

待办：

| 依赖 | 当前 | 最新可用 | 代码使用点 | 备注 |
| --- | --- | --- | --- | --- |
| `sqlx` | 0.8.6 | 0.9.0 | 43 | 与 rusqlite 共享 `libsqlite3-sys`（`links`），升级需两者同步评估；见 §5「native 库约束」 |
| `rand` | 0.8.8 | 0.10.2 | 19 | `thread_rng`/`gen_range` 等改名；注意 `fake` 内部仍用 rand 0.8，升完也不去重 |
| `arrow` | 58.4.0 | 59.3.0 | — | **不可单独升**，须与 `duckdb` 同步（见 R4） |

## 5. 关键依赖与约束

### 业务关键依赖

- **`sqlglot-rust`｜SQL 编辑器的解析 / 格式化 / 转译**：`crates/engine/src/sql/`（`mod.rs` / `engine.rs` / `parser.rs` / `formatter.rs` / `transpiler.rs` / `builder.rs`）是它在全仓的**唯一接入点**，业务模块只 `use crate::sql::SqlEngine`，不直接依赖 sqlglot API。它直接支撑编辑页面的 SQL 能力，因此精确锁定 `=0.10.29`。升级前必须跑 SQL 回归：`cargo test -p rds-engine sql::` 覆盖 parser（语句分类）/ formatter / transpiler / builder，其中 `builder::tests::test_generated_sql_is_pinned` 钉住了生成 SQL 的字面结果（含标识符加引号方式）。
- **`gpui-kit` 家族**：`gpui-kit` / `gpui-base` / `gpui-component` / `gpui-kit-assets` 必须同版本，随 GPUI-kit 发布节奏整套升（见 R4）。
- **`arrow` 跟随 `duckdb`**（见 R4）。

### 加密后端约束（不要改回默认）

- `russh` 默认 feature 走 `aws-lc-rs` 后端，其构建脚本在 Windows 上要求本机安装 NASM，缺失会直接构建失败。本仓改用官方支持的 `ring` 后端：
  `russh = { version = "0.63.3", default-features = false, features = ["ring", "rsa", "flate2"] }`。
  `ring` 与 `aws-lc-rs` 同属 BoringSSL 家族，SSH 算法集合基本一致；`rsa`（RSA 密钥）与 `flate2`（压缩）保持启用。
- `aes-gcm` 只用于 `crates/shared/src/crypto.rs` 的密码加解密（12 字节 nonce + AES-256-GCM + base64），升级后密文格式不变，已有密文可继续解开。
- 另：`reqwest` → `rustls` 链路也会带入 `aws-lc-sys`，本机实测未触发 NASM 报错（两处配置不同）；新机器首次构建若报 NASM 缺失，优先查这条链（装 NASM 或调整 rustls 的 crypto provider）。

### native 库约束（links）

- `sqlx` 与 `rusqlite` 都间接依赖 `libsqlite3-sys`，而它声明 `links = "sqlite3"`——**同一依赖图里只能存在一份**，否则 cargo 直接拒绝解析（报 links 冲突）。实测：sqlx 0.8.6 一旦启用 `macros` / `migrate` / `json`（经 `sqlx-macros-core`）就会把 `sqlx-sqlite` 拉进图，从而钉死 `libsqlite3-sys 0.30`，使 rusqlite 无法升到 0.40（需 0.38）。
- 因此本仓 sqlx 用**最小 feature 集**：`default-features = false` + `["mysql", "postgres", "runtime-tokio", "runtime-tokio-native-tls"]`。我们只用运行时的 `sqlx::query` / `query_scalar`（不用 `query!` 宏、不用 sqlx 迁移、不用 JSON 列），所以去掉 `macros` / `migrate` / `json` 是安全的。
- 动这条规则前先确认：一旦 sqlx 重新引入 `sqlx-sqlite`，必须保证它与 rusqlite 需要**同一个** `libsqlite3-sys` 版本（或干脆先升级 sqlx）。
- 本次为拿到干净解析，`Cargo.lock` 整体重新生成过一次（各依赖仍在 caret 范围内取最新）。

### 存量评估

- **`specta`｜v1 遗留，暂留**：全仓 31+ 处 `use specta::Type` / `#[derive(Type)]`（engine 最多），但**没有任何导出器消费**（根表无 `specta-typescript` / `tauri-specta`），当前只产出类型元数据、没有实际产物——它是 v1（Tauri + TS 前端）留下的。结论：**暂时保留**（M9 插件或未来对外类型导出可能复用；删除涉及 31 个文件，恢复成本更高）。若确定 v2 不再对外暴露类型，可作为一次独立清理移除，同时去掉 `=2.0.0-rc.25` 这个 rc pin。

### 已知脆点（本轮发现，未修）

- `crates/connection/src/known_hosts.rs` 的 `verify()` 用 `PublicKey == PublicKey` 比较，而 `ssh_key::PublicKey` 的 `PartialEq` **包含 `comment` 字段**；写入侧用的 `public_key_base64()` 又只输出 base64 本体（不含类型前缀与注释）。所以当 known_hosts 行尾带注释（部分工具生成或手工粘贴）时，解析出的 key 注释非空、与服务端 key（注释恒空）不等，**即使密钥一致也会判定为不匹配并报 MITM 风险**。建议改为比较 `key_data()` 或 SHA-256 指纹（语义等价、与注释无关）。

## 6. 工程约定

- `v1/` 为历史参考实现，自带 workspace 与 `Cargo.lock`，不参与 v2 构建；根 `Cargo.toml` 用 `exclude = ["v1"]` 显式排除（否则在 `v1/backend` 下执行 cargo 会报 “believes it's in a workspace when it's not”），该目录后续删除。
- 新增 crate 时：先在根表确认依赖是否已有条目，没有再加到根表；crate 内只写 `dep.workspace = true`。
- 项目级 `.cargo/config.toml` 只放「怎么构建 / 命令别名」，不放依赖版本与镜像：
  - 版本归根 `Cargo.toml` + `Cargo.lock`；镜像归机器级配置（本机在 `CARGO_HOME` 的 config.toml 指向 tuna），写进仓库会影响其他网络环境与 CI
  - 不要写 `[profile.*]`：Cargo 1.57 起只认根 `Cargo.toml`（`v1/backend/.cargo/config.toml` 里的 profile 就是这样失效的）
  - 当前内容：`check-all` / `test-all` / `clippy-all` 三个别名，用来弥补 `default-members` 导致裸 `cargo test` 只覆盖 app 图的行为

## 7. 实现位置映射表

| 设计决策 | 代码位置 |
| --- | --- |
| R1 第三方依赖版本唯一入口 | `Cargo.toml` → `[workspace.dependencies]` |
| R1 内部 crate 路径唯一入口 | 同上（`shared` / `engine` / `connection` / `insight` / `mock` / `settings` / `workbench` 条目） |
| R2 crate 内写法 | `crates/*/Cargo.toml`（13 个成员） |
| R3 版本取值来自解析结果 | `Cargo.lock` ↔ 根表版本号 |
| R4 例外：gpui-kit | `Cargo.toml` → `gpui-kit = "0.6.1"` |
| R4 例外：specta / sqlglot-rust | 同上（精确锁定条目；sqlglot 的唯一边界见 `crates/engine/src/sql/`） |
| R4 例外：arrow 跟随 duckdb | 同上（`arrow` / `duckdb` 条目） |
| v1 目录不参与构建 | `Cargo.toml` → `workspace.exclude` |
| 版本继承（version/edition） | `Cargo.toml` → `[workspace.package]` |
| 编译时间：默认只构建 app 图 | `Cargo.toml` → `workspace.default-members` |
| 编译时间：依赖不产 debuginfo | `Cargo.toml` → `[profile.dev.package."*"]` |
| 项目级构建别名 | `.cargo/config.toml` → `[alias]`（`check-all` / `test-all` / `clippy-all`） |
| 加密后端约束（ring） | `Cargo.toml` → `russh` 条目（`default-features = false`） |
| native 库约束（links） | `Cargo.toml` → `sqlx` 条目（最小 feature 集） |
| SQL 生成结果回归 | `crates/engine/src/sql/builder.rs` → `tests::test_generated_sql_is_pinned` |
