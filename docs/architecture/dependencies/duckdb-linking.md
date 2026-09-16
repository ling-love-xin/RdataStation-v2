# DuckDB 内核：动态链接（设计 / 操作手册）

状态：**已落地（2026-09-16）**。取代此前的 `features = ["bundled"]`（构建期编 DuckDB 的 C++ 内核）。
记录出处：`dependency-strategy.md` §3「编译时间」的待评估项 + §5「DuckDB 内核与扩展」。

## 1. 为什么改

| bundled 的代价 | 实测 |
| --- | --- |
| 每次 `target` 重建都要编 C++ 内核 | 单次数分钟到十几分钟，且与 Rust 侧改动无关 |
| 并发链接静态库吃满内存 | 全仓固定 `-j 2`（`LNK1102` / rustc `STATUS_STACK_BUFFER_OVERRUN`），宿主卡顿 |
| 每个可执行文件都带一份内核 | debug 单文件 ~137 MiB |
| `cargo clean` 后重来一遍 | 开发循环里最贵的一步 |

改后：Rust 侧照常编，DuckDB 只**链接**现成的导入库 + 运行时加载 `duckdb.dll`。

## 2. 形态（谁在哪）

| 项 | 位置 |
| --- | --- |
| 库（`duckdb.dll` / `duckdb.lib` / `duckdb.h` …） | `third_party/duckdb/<版本>/`（**gitignore**，不进仓库、不随 `cargo clean` 丢） |
| 取库脚本 | `tools/fetch-duckdb.sh` |
| 构建期指向 | `.cargo/config.toml` 的 `[env] DUCKDB_LIB_DIR`（`relative = true`，开发机零环境变量） |
| 运行时 dll 拷贝 | `crates/engine/build.rs`（拷到 `target/<profile>/` 与 `.../deps/`） |
| crate 版本 | 根 `Cargo.toml` 的 `duckdb = "1.10505.0"`（对应 DuckDB **v1.5.5**） |
| 体积守门 | `tools/target-guard.sh`（§9） |

**版本映射**：crate 版本第二段的十进制数编码 DuckDB 版本——`10505` → `1.5.5`
（`libduckdb-sys` build script 的 `duckdb_version_from_pkg_version`）。**crate 与库必须成对升级**，
不一致会在运行期以符号缺失 / 崩溃体现，而不是编译期。

## 3. 首次与换机

```bash
tools/fetch-duckdb.sh            # 默认 1.5.5 → third_party/duckdb/1.5.5/
cargo check -p rds-engine        # 冒烟
```

- 本机 **GitHub 直连不通**，脚本默认走镜像（`GH_PROXY=https://ghproxy.net/`）；
  能直连时 `GH_PROXY= tools/fetch-duckdb.sh`。
- 脚本按平台选资产：Windows amd64 / Linux amd64 / macOS universal。
- 已存在则跳过（要重取 `FORCE=1`）。

## 4. 运行时（dll 从哪来）

- **Windows**：`crates/engine/build.rs` 把 `duckdb.dll` 拷到 `target/<profile>/`（`cargo run`）
  与 `target/<profile>/deps/`（测试二进制）。这是 `libduckdb-sys` 的 `DUCKDB_DOWNLOAD_LIB`
  路径同款手法（那边拷的是它下载到 `target/duckdb-download/` 的那份）。
- **Linux / macOS**：动态库靠 rpath / `LD_LIBRARY_PATH` 解析（拷到 exe 旁无效），
  开发机导出 `export LD_LIBRARY_PATH="$PWD/third_party/duckdb/1.5.5"`。

## 5. 升级内核

1. 根 `Cargo.toml`：改 `duckdb` 版本（按 §2 的映射写第二段）；
2. `tools/fetch-duckdb.sh <新版本>`；
3. `.cargo/config.toml`：改 `DUCKDB_LIB_DIR` 的路径；
4. 冒烟：`cargo test -p rds-engine --lib` 与 `cargo test -p rds-mock`（引擎与 mock 是最重的两个 DuckDB 使用方）；
5. 回填本文 + `dependency-strategy.md` 的版本记录。

## 6. 与扩展的关系（不变）

**扩展的获取与内核的编译解耦**（`dependency-strategy.md` §5）：扩展仍走 `INSTALL` / `LOAD` 到指定目录，
只在扩展与内核 ABI 不匹配时才谈内核升级——内核升级现在也只是一次脚本 + 一行路径。
预编译库里带了哪些内置扩展以官方 release 为准（parquet / json / icu 等），缺什么用 `INSTALL` 补。

## 7. 排错

| 现象 | 处置 |
| --- | --- |
| `cannot open input file 'duckdb.lib'` | 库没到位：`ls third_party/duckdb/*/`，重跑取库脚本；或路径与版本不一致 |
| 运行时报「找不到 duckdb.dll」 | `cargo build -p rds-app` 后看 `target/debug/duckdb.dll` 是否在；不在就手动复制一份（build.rs 会打印拷贝失败的原因） |
| `cargo` 打出 `build.rs` 的 warning | 按提示跑取库脚本（warning 只在库缺失时出现，正常环境零告警） |
| 又在编 C++ 内核 | 检查有没有人把 `features = ["bundled"]` 加回 `Cargo.toml` |

## 8. 已知偏差（有意，不是疏忽）

1. 仓库多了 `third_party/`：此前没有任何二进制先例——但它**位置固定、脚本可取、不进 git**，
   比「构建期偷偷下载到 `target/`」更可审计（后者被 `cargo clean` 一清就得重新联网取）。
2. 构建期不再联网：代价是换版本要跑一次脚本（收益是离线可构建、可复现）。
3. `-j 2` 保留：理由从「链接 DuckDB 静态库会 OOM」变成「重型 crate 的 codegen / 链接仍会吃内存」。

## 9. `target/` 体积治理

```bash
tools/target-guard.sh              # 报告；≥60 GB 退出码 1（可挂到日常命令后）
tools/target-guard.sh --clean      # 删：增量缓存 / *.pdb / 旧 duckdb-download
```

改动态链接后 `target/` 的大头变成**每个可执行文件的 PDB** 与**增量编译缓存**（此前还有 C++ 内核的对象文件）。
两者都能再生：`--clean` 删掉后下一次构建只是慢一点，不会重新编 DuckDB。
仍不够时用 `cargo clean -p <crate>` 定向清理，最后手段才是 `cargo clean`。
