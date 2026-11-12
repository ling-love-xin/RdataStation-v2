# 驱动插件包（tools/plugin-drivers）

「一个通用壳 + 每库一个插件包」的落地目录，另有 **Rust 原生 sidecar** 轨：每个库编一个
独立二进制 + `plugin.toml`，走同一套 stdio 帧协议，**不编进主程序**、不需要 Java / Python 运行时。

插件包 = manifest（`plugin.toml`）+ 驱动资产（jar / python 壳 / Rust 二进制），**不内嵌任何运行时**：
Java / Python 解释器由宿主 `crates/plugin/src/runtime.rs` 三级查找；Rust 驱动是自身的可执行文件。

## 结构

```
tools/plugin-drivers/
├── oracle-jdbc/            # rds.driver.oracle：通用 JDBC 壳 + ojdbc11（+ h2 无证书验证；默认不安装）
├── postgres-py/            # rds.driver.postgres-py：Python DB-API 壳 + psycopg（用户环境）
├── rust-core/              # crate rds-driver-sidecar：Rust 驱动共享协议内核（帧 / 错误码 / Page / meta）
├── oracle-rust/            # crate rds-driver-oracle：Oracle 原生驱动源码（oracle-rs）
├── mssql-rust/             # crate rds-driver-mssql：SQL Server 原生驱动源码（tiberius）
├── oracle-rust-plugin/     # rds.driver.oracle-rust：Rust 驱动插件包（plugin.toml + bin/）
├── mssql-rust-plugin/      # rds.driver.mssql-rust：Rust 驱动插件包（plugin.toml + bin/）
└── install.ps1             # 构建壳 / 下载驱动 jar / 构建 Rust 驱动 / 安装到 RDS_HOME/plugins/<manifest-id>
```

## 使用

```powershell
# 默认装 postgres-py + oracle-rust + mssql-rust
powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\install.ps1

# 只预检、不动任何文件
powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\install.ps1 -DryRun -SkipRust

# 回退到 JDBC 版 Oracle（会与 oracle-rust 的驱动 id `oracle` 撞车，不能同时启用）
powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\install.ps1 -WithJdbcOracle
```

> **Oracle 只装一版**：`rds.driver.oracle`（JDBC）与 `rds.driver.oracle-rust` 都向宿主贡献
> 驱动 id `oracle`。两个都装会被加载期跨插件冲突检测拒绝，所以 install 默认只装 Rust 版；
> `-WithJdbcOracle` 时只装 JDBC 版、不装 Rust 版。两版互斥：开关与目标目录里已有版本冲突时
> 脚本直接拒绝，需先把另一版目录移出 `.rds/plugins` 再重试。

产物原则：驱动 jar / 壳 jar / Rust 驱动二进制不入库（各包 `.gitignore` 忽略 `lib/`、`bin/`），
全部由脚本运行时组装（Maven Central 下载 + 壳构建 + `cargo build --release`）。
离线环境用 `-SkipOjdbc`；只想复用已编好的 Rust 二进制用 `-SkipRust`。

## Rust 原生驱动轨

- 协议内核 `rust-core` 提供帧编解码、错误码、Page / Arrow、`Driver` trait 与 `serve` 主循环；
  驱动 crate 只实现 `Driver`，不重复协议代码。
- 单元测试（无需真实数据库）：

```powershell
$env:CARGO_TARGET_DIR = "D:\RdataStation\RDS\RdataStation-v2\.rds\target-gf"
cargo test --manifest-path tools/plugin-drivers/rust-core/Cargo.toml
cargo test --manifest-path tools/plugin-drivers/oracle-rust/Cargo.toml
cargo test --manifest-path tools/plugin-drivers/mssql-rust/Cargo.toml
```

- 插件包只放清单和二进制；每个包自己的 `build.ps1`（如
  `mssql-rust-plugin/build.ps1`）编 release 并复制为 `bin/win-x64/agent.exe`，
  宿主 `layout::resolve_executable` 按平台子目录解析。改完驱动源码后
  **三处副本要保持一致**（`<包>/bin/...`、`.rds/plugins/<id>/bin/...`、
  `.rds/target-gf/debug/plugins/<id>/bin/...`），否则跑的还是旧二进制。
- 能力边界：两个驱动都声明 `cancel = false`（无法中断在飞查询），
  `tx.*` 走真实语句。
- 真机口径（2026-11-12）：
  - **Oracle 已验收**：`sidecar_conformance` 对 `192.168.3.138:1521/XEPDB1` 6/6（含 3000 行分页）；
    `crates/plugin/tests/rust_driver_wiring.rs` 另有「真实安装目录 → `PluginManager` 激活 →
    supervisor 开会话 → 真库查询」用例（设置 `RDS_TEST_ORACLE_URL` 时执行），并在同一用例里
    用新的 `PluginRuntimeLoop::tick_once` 复现宿主启动首拍，验证「重启后自动加载 + 激活、
    `drivers.enabled` 回到 1」——即下拉能看到驱动的完整启动链路。2026-11-12 复跑
    `oracle_plugin_serves_a_real_query` 通过。
  - **SQL Server 已验收（2026-11-12）**：`192.168.3.138:1433` 是 SQL Server 2025 RTM
    （`17.0.1000.7`，Standard Developer Edition）的**默认实例**（`ling` 是 `@@SERVERNAME`，
    不是命名实例），SQL Server 验证 + 用户 `sa`；`mssql_plugin_serves_a_real_query` 走真实
    安装目录 → 激活 → supervisor 开会话 → `select 1 as n` 回一列一行，通过。此前九次
    `18456 state 1` 是缺口令所致，不是网络或加密问题；Windows 集成认证在此环境不可用
    （Rust 侧 `SEC_E_NO_CREDENTIALS`，.NET 侧 `Cannot generate SSPI context`）。
  - **SQL Server 导航面已验收（2026-11-12）**：`sidecar_conformance` 对真机 **6/6 全过**
    （catalog `master/model/msdb/tempdb`、schema `dbo/guest/INFORMATION_SCHEMA/sys`、
    对象 8 个、`MSreplication_options` 6 列 / 索引 0 / 行数估计 3）。
    首次跑时第 6 条把整个 sidecar 打死（宿主侧只见 `帧 I/O 失败：early eof`），
    手动发帧抓 stderr 才看到真因：`sys.columns.precision` / `scale` 是 tinyint，
    `meta.rs` 按 `i16` 取，tiberius 的 `Row::get` 内部 unwrap 时 panic
    （`cannot interpret U8(Some(0)) as an i16 value`，exit 101），改按 `u8` 取后恢复。
    **教训**：单个元数据查询的 panic 会连坐同一实例上的所有会话，元数据提取一律
    只认列的真实类型；宿主侧不会告诉你哪个对象炸的，得自己收 sidecar stderr。
  - **真机凭据与端点档案在本机 `.rds/local/`（不入库）**：`test-env.ps1` 是纯 ASCII 的
    dot-source 脚本，注入 `RDS_TEST_ORACLE_URL` / `RDS_TEST_MSSQL_URL`（含口令）；
    `real-hosts.md` 记录端点 / 版本 / 库清单 / 验证记录 / 历史失败，不含口令明文。
    两者都在 `.rds/` 下，被仓库根 `.gitignore` 的 `/.rds/` 整目录忽略，`git status` 看不到。
  - **验收命令**：先 `. .rds/local/test-env.ps1`，再跑
    `cargo test -j 1 -p rds-plugin --test rust_driver_wiring -- --nocapture --test-threads=1`；
    未设变量的用例会自己跳过，不会把「缺凭据」伪装成通过。`-j 1` 必须写在 `--` 之前
    （写到后面会变成测试二进制的未知参数）。登录失败时驱动会补中文下一步提示
    （填 SQL 认证用户名/密码，或检查实例是否启用 SQL 登录）。

## 指定自己的驱动 jar（driver_jars）

不想用插件包自带的驱动 jar、或想用自己下载的 JDBC 包时，在插件的 `[backend]` 段加：

```toml
[backend]
kind = "script"
runtime = "java"
executable = "lib/rds-jdbc-shell.jar"
interpreter_args = ["-jar"]
# 用户自定驱动 jar：绝对路径，文件或目录（目录扫 *.jar）；多个用数组
driver_jars = ["D:/drivers/oracle/ojdbc11.jar", "D:/drivers/mysql/"]
```

- 仅 `runtime = "java"` 允许（JDBC jar 由 Java 壳加载）；路径必须绝对，相对路径拒绝。
- 宿主部署期校验路径存在（不存在报可读错误），经 `RDS_DRIVER_JARS` 环境变量交给壳；
  壳启动时把它与插件包 `lib/` 的驱动合并加载，连接仍按 `acceptsURL` 匹配。
- 改完清单后重装插件或触发热插拔即可生效。

## 新增一个 JDBC / DB-API 数据库驱动

1. 复制一个现有插件包目录（如 `oracle-jdbc`）；
2. `plugin.toml`：改 `plugin.id` / `name` / `license`；`contributes.drivers` 改
   `id`（连接面板下拉显示的驱动 id）/ `display_name` / `default_port` /
   `url_template`（占位符 `{host}` `{port}` `{database}` `{user}` `{password}`，
   壳内替换）；
3. 驱动 jar 放入 `lib/`（或 Python 侧改 `rds_dbapi_shell.py` 的适配器）；
4. `install.ps1` 的 `$targets` 表加一行（src 目录名 + manifest id）。

壳零改动：JDBC / DB-API 2.0 是标准接口，协议 / 执行 / meta 映射已在壳内一次实现。

## 许可提醒

- ojdbc11：OTN（Oracle Technology Network）许可，见 `oracle-jdbc/README.md`；
- H2：MPL 2.0 / EPL 1.0；psycopg：LGPL / MIT（随用户环境）。
