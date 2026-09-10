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
| `sqlglot-rust` | `=0.9.25` | 0.9.x 内 SQL 转译行为有差异，升级需跑 SQL 回归 |
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

以上验证：`cargo check-all`（= `check --workspace --all-targets`）全量通过；`cargo test -p rds-shared` 19 项全过（含 3 项密码加解密）。

待办：

| 依赖 | 当前 | 最新可用 | 代码使用点 | 备注 |
| --- | --- | --- | --- | --- |
| `rusqlite` | 0.32.1 | 0.39.0（0.40.2 解析时被跳过，需确认） | 314 | 最重，`Params`/`Row`/错误类型均有变更 |
| `mysql_async` | 0.34.2 | 0.37.1 | 67 | 驱动 API 变更 |
| `sqlx` | 0.8.6 | 0.9.0 | 43 | 驱动 API 变更 |
| `sqlglot-rust` | `=0.9.25` | 0.10.29 | 25 | 需 SQL 转译回归 |
| `rand` | 0.8.8 | 0.10.2 | 19 | `thread_rng`/`gen_range` 等改名；注意 `fake` 内部仍用 rand 0.8，升完也不去重 |
| `russh` | 0.49.2 | 0.63.3 | 13 | `russh-keys` 已并入 `russh`，结构需调整；升完 `aes-gcm` 可与本仓 0.11 合流 |
| `arrow` | 58.4.0 | 59.3.0 | — | **不可单独升**，须与 `duckdb` 同步（见 R4） |

## 5. 其他约定

- `v1/` 为历史参考实现，自带 workspace 与 `Cargo.lock`，不参与 v2 构建；根 `Cargo.toml` 用 `exclude = ["v1"]` 显式排除（否则在 `v1/backend` 下执行 cargo 会报 “believes it's in a workspace when it's not”），该目录后续删除。
- 新增 crate 时：先在根表确认依赖是否已有条目，没有再加到根表；crate 内只写 `dep.workspace = true`。
- 项目级 `.cargo/config.toml` 只放「怎么构建 / 命令别名」，不放依赖版本与镜像：
  - 版本归根 `Cargo.toml` + `Cargo.lock`；镜像归机器级配置（本机在 `CARGO_HOME` 的 config.toml 指向 tuna），写进仓库会影响其他网络环境与 CI
  - 不要写 `[profile.*]`：Cargo 1.57 起只认根 `Cargo.toml`（`v1/backend/.cargo/config.toml` 里的 profile 就是这样失效的）
  - 当前内容：`check-all` / `test-all` / `clippy-all` 三个别名，用来弥补 `default-members` 导致裸 `cargo test` 只覆盖 app 图的行为

## 6. 实现位置映射表

| 设计决策 | 代码位置 |
| --- | --- |
| R1 第三方依赖版本唯一入口 | `Cargo.toml` → `[workspace.dependencies]` |
| R1 内部 crate 路径唯一入口 | 同上（`shared` / `engine` / `connection` / `insight` / `mock` / `settings` / `workbench` 条目） |
| R2 crate 内写法 | `crates/*/Cargo.toml`（13 个成员） |
| R3 版本取值来自解析结果 | `Cargo.lock` ↔ 根表版本号 |
| R4 例外：gpui-kit | `Cargo.toml` → `gpui-kit = "0.6"` |
| R4 例外：specta / sqlglot-rust | 同上（精确锁定条目） |
| R4 例外：arrow 跟随 duckdb | 同上（`arrow` / `duckdb` 条目） |
| v1 目录不参与构建 | `Cargo.toml` → `workspace.exclude` |
| 版本继承（version/edition） | `Cargo.toml` → `[workspace.package]` |
| 编译时间：默认只构建 app 图 | `Cargo.toml` → `workspace.default-members` |
| 编译时间：依赖不产 debuginfo | `Cargo.toml` → `[profile.dev.package."*"]` |
| 项目级构建别名 | `.cargo/config.toml` → `[alias]`（`check-all` / `test-all` / `clippy-all`） |
