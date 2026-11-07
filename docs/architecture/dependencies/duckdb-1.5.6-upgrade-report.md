# DuckDB 1.5.5 → 1.5.6 升级报告（duckdb crate 1.10505.0 → 1.10506.1）

> 日期：2026-11-08 · 分支：`upgrade/duckdb-1.5.6`（在 `upgrade/gpui-kit-0.9.0` 之上叠放，可独立 rebase 到 main）
> 关联：`duckdb-linking.md`（动态链接设计）· `dependency-strategy.md`（版本记录）· `duckdb-2.0-gpui-kit-0.8.1-upgrade-assessment.md`（2.0 线成本）

## 1. 结论

**1.5 线内的 patch 升级已完成且低风险**：crate `duckdb` `1.10505.0` → **`1.10506.1`**（对应 DuckDB **v1.5.6**），
`arrow` **保持 58.4.0 不动**，**项目代码零改动**，编译（engine / app 两级）与运行时（引擎 155 项 DuckDB 单测 + 12s 启动冒烟）
全部通过。DuckDB 2.0 线（`1.20000.1`）继续暂缓——它强制 `arrow ^60` 且有存储格式单向兼容（2.0 之前版本读不了 2.0 写的库）。

一个新增的硬约束：**`duckdb` 必须用 `=` 精确锁定**，caret 约束会被 cargo 解析到 `1.20000.1`（见 §3）。

## 2. 版本与资产事实（2026-11-08 核实）

| 项 | 值 | 来源 |
| --- | --- | --- |
| crate | `duckdb 1.10506.1` | crates.io `versions[].num` |
| 内核库 | DuckDB **v1.5.6** | `duckdb.dll` 的 `duckdb_library_version()` 实读 = `v1.5.6` |
| crate 依赖 | `libduckdb-sys =1.10506.1`、`arrow ^58`（features `prettyprint`+`ffi`，非 optional）、`comfy-table ~7.1`、`hashlink ^0.10`、`strum ^0.27`（derive）、`cast ^0.3`、`fallible-iterator 0.3`、`fallible-streaming-iterator 0.1`、`num-integer ^0.1.46` | `crates.io/api/v1/crates/duckdb/1.10506.1/dependencies` |
| 库资产 | `libduckdb-windows-amd64.zip`（13,408,787 B） | `github.com/duckdb/duckdb/releases/download/v1.5.6/`（本机走 `ghproxy.net` 镜像） |
| 解包内容 | `duckdb.dll` 36,697,896 B（35.0 MB）· `duckdb.h` 0.3 MB · `duckdb.hpp` 2.1 MB · `duckdb.lib` 1.9 MB · `duckdb_extension.h` 0.1 MB | `third_party/duckdb/1.5.6/` |
| dll 指纹 | SHA256 `7E90BDEF…84ED0`（v1.5.6）≠ `2B7468A4…7EDE46`（v1.5.5，36,724,520 B） | `Get-FileHash` |

版本映射：crate 第二段十进制编码内核版本，`10506` → `1.5.6`（`libduckdb-sys` build script 的
`duckdb_version_from_pkg_version`）。**crate 与 `third_party/duckdb/<版本>/` 必须成对**，不一致只在运行期以符号缺失 / 崩溃体现。

## 3. 关键坑：caret 约束把 1.5.6 升级解析成了 DuckDB 2.0

第一次按惯例写 `duckdb = "1.10506.1"`（caret 语义 `^1.10506.1`）再 `cargo update -p duckdb --precise 1.10506.1`，
`--precise` 未生效，解析落到 **`1.20000.1`**：

```
Adding arrow v60.1.0  …（arrow 全族 60.1.0，共 11 个包）
Updating comfy-table v7.1.4 -> v8.0.1
Updating duckdb v1.10505.0 -> v1.20000.1
Updating libduckdb-sys v1.10505.0 -> v1.20000.1
Removing zip v6.0.0 / crossterm v0.28.1 / crossterm_winapi v0.9.1
```

处置：
1. `git checkout -- Cargo.lock` 回滚（一次性生成物，可安全丢弃）；
2. 根 `Cargo.toml` 改**精确锁定** `duckdb = { version = "=1.10506.1", features = ["appender-arrow"] }`；
3. 重跑 `cargo update -p duckdb --precise 1.10506.1` → `Updating duckdb v1.10505.0 -> v1.10506.1`，
   `arrow` 全族仍只有 `58.4.0`（`Cargo.lock` 里 `name = "arrow"` 条目 1 条）。

**结论**：`1.10505` / `1.10506` / `1.20000` 在 semver 上同属 `1.x`，caret 会把它们当兼容版本；
而它们实际是三条不同内核线，且 2.0 线改变 `arrow` 主版本与存储格式兼容性。今后 bump 必须写 `=`。

## 4. 改动清单（四处成对 + 库落地）

| 文件 | 改动 |
| --- | --- |
| `Cargo.toml`（根） | `duckdb = { version = "=1.10506.1", features = ["appender-arrow"] }`；注释补 1.5 线 patch 沿革与「必须精确锁定」的理由 |
| `Cargo.lock` | `duckdb` / `libduckdb-sys` → `1.10506.1`；`zip` 6.0.0 → 9.4.1（新增一份，与 rds-plugin 的 0.6.6 并存）；新增 `typed-path 0.13.0`；移除 `derive_arbitrary 1.4.2` |
| `.cargo/config.toml` | `DUCKDB_LIB_DIR = { value = "third_party/duckdb/1.5.6", relative = true }` |
| `tools/fetch-duckdb.sh` | 默认版本 `1.5.5` → `1.5.6`（用法注释同步） |
| `third_party/duckdb/1.5.6/` | 新增（**gitignore，不入库**）；`1.5.5/` 暂时保留未删 |
| `Cargo.toml`（根，顺带清理） | 删除 `reqwest` 死声明：`use reqwest` 全仓 0 命中、`Cargo.lock` 无该包（sidecar 早已改 std JSON-RPC 直连）；同步修正 `encoding_rs` 与 `crates/editor/Cargo.toml` 里「版本随 reqwest 已在锁里」的过时注释 |

无 bash / curl 的环境（本机即是）取库方式：`Invoke-WebRequest` 拉
`https://ghproxy.net/https://github.com/duckdb/duckdb/releases/download/v1.5.6/libduckdb-windows-amd64.zip`
→ `Expand-Archive` 到 `third_party/duckdb/1.5.6/`。目录里必须有 `duckdb.dll` + `duckdb.h`（后者供 `libduckdb-sys` 生成 bindings）。

## 5. 验证

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 引擎编译 | `cargo check -p rds-engine -j 2` | exit 0，**0 error 0 warning**，2m55s（`Compiling libduckdb-sys v1.10506.1` → `Checking duckdb v1.10506.1`） |
| 应用编译 | `cargo check -p rds-app -j 2` | exit 0，**0 error 0 warning**，2m21s |
| 链接 + 产物 | `cargo build -p rds-app -j 2` | exit 0，`target/debug/rds-app.exe` 153.7 MB |
| dll 部署 | `crates/engine/build.rs` | `target/debug/duckdb.dll` 与 `target/debug/deps/duckdb.dll` 均为 36,697,896 B，SHA256 与 `third_party/duckdb/1.5.6/duckdb.dll` **一致**（不是残留的 1.5.5） |
| 运行时版本 | P/Invoke `duckdb_library_version()` | **`v1.5.6`** |
| 引擎单测 | `cargo test -p rds-engine -j 2 --lib duckdb` | `155 passed; 0 failed`（64 filtered），2.76s |
| 启动冒烟 | 直跑 exe，12s | 存活未退出，stderr 45 行全 INFO，`panic` / `ERROR` **0 命中**，`demo_duckdb` / `real_duckdb` 连接项正常恢复 |

## 6. 副作用（都来自 `libduckdb-sys 1.10506.1` 的 build-dep，不是内核）

- `zip 6.0.0 → 9.4.1`：现在锁里**两份 zip**（`rds-plugin` 用 `0.6.6`，`libduckdb-sys` 构建期用 `9.4.1`）。
  这也回答了之前全量体检里「`zip 0.6.6 → 9.4.1` 要不要升」的一半问题：构建链已被动升到 9，业务侧仍是 0.6.6，二者互不影响。
- 新增 `typed-path 0.13.0`（zip 9 的依赖）；移除 `derive_arbitrary 1.4.2`（zip 6 的依赖）。
- `arrow` / `comfy-table` / `hashlink` / `strum` **无变化**（1.5 线内不换 arrow 主版本）。

## 7. 未验证与风险

1. **源码里带 `1.10505` 的实测注释没有复测**：`crates/engine/src/duckdb/executor.rs:202/273`、`services/duckdb_service.rs:207`
   （「`column_name` / `schema()` 必须在语句执行后取」）、`crates/connection/src/secret.rs:193` 与
   `connection_dialog` / `editor` / `accel.rs`（「DuckDB 1.5.5 的 mysql / postgres 扫描器不认 Secret」）。
   这些是**行为断言**，本轮只跑了引擎单测，没有跑真机 MySQL/PG 联邦与 `oracle_probe`。升级后再动这些代码前，先按原探针复跑一遍，不要默认仍然成立。
2. **存储格式**：1.5.6 与 1.5.5 同属 1.5 线（官方声明向后兼容写读），但本仓 `analytics.duckdb` 的跨版本读写**没有专门验证**（冒烟未触发分析库打开）。旧的 1.5.5 库文件建议先备份再用新版本打开一次。
3. **`crates/mock` 的 DuckDB 路径**未单独跑测试（`duckdb-linking.md` §5 要求 engine + mock 双冒烟，本轮只做了 engine）。
4. **磁盘**：`third_party/duckdb/1.5.5/` 与 `1.5.6/` 并存（各 ~35 MB dll），确认新版本稳定后可删 1.5.5 并清 `target` 里的旧 dll。
5. DuckDB 2.0（`1.20000.1`）仍未评估落地时机；一旦升，`arrow` 必须同步到 60，且旧版本读不了 2.0 写的库。

## 8. 复现步骤

```powershell
# 1) 取库（Windows，无 bash 时）
$wt = "D:\RdataStation\RDS\RdataStation-v2-upgrade"
Invoke-WebRequest -Uri "https://ghproxy.net/https://github.com/duckdb/duckdb/releases/download/v1.5.6/libduckdb-windows-amd64.zip" -OutFile "$wt\.rds\tmp\d6.zip"
Expand-Archive -LiteralPath "$wt\.rds\tmp\d6.zip" -DestinationPath "$wt\third_party\duckdb\1.5.6" -Force
# 2) 三处成对：根 Cargo.toml（=1.10506.1）/ .cargo/config.toml（1.5.6）/ tools/fetch-duckdb.sh（默认 1.5.6）
cargo update -p duckdb --precise 1.10506.1
# 3) 验证
cargo check -p rds-engine -j 2 && cargo check -p rds-app -j 2
cargo test -p rds-engine -j 2 --lib duckdb
cargo build -p rds-app -j 2
(Get-FileHash "$wt\target\debug\duckdb.dll").Hash   # 应等于 third_party\duckdb\1.5.6\duckdb.dll 的哈希
```

回滚：`git checkout -- Cargo.toml Cargo.lock .cargo/config.toml tools/fetch-duckdb.sh`，库目录可留在原地（gitignore）。
