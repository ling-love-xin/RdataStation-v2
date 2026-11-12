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
├── rust-core/              # crate rds-driver-sidecar：Rust 驱动共享协议内核（帧 / 错误码 / Page / meta / 多驱动路由）
├── oracle-rust/            # crate rds-driver-oracle：Oracle 原生驱动源码（oracle-rs）
├── mssql-rust/             # crate rds-driver-mssql：SQL Server 原生驱动源码（tiberius）
├── sqlx-rust/              # crate rds-driver-sqlx：MySQL + PostgreSQL 的 sqlx 实现（一个进程两个 driver id）
├── oracle-rust-plugin/     # rds.driver.oracle-rust：Rust 驱动插件包（plugin.toml + bin/）
├── mssql-rust-plugin/      # rds.driver.mssql-rust：Rust 驱动插件包（plugin.toml + bin/）
├── sqlx-rust-plugin/       # rds.driver.sqlx：Rust 驱动插件包（contributions: mysql / postgres）
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

- 协议内核 `rust-core` 提供帧编解码、错误码、Page / Arrow、`Driver` trait 与主循环；
  驱动 crate 只实现 `Driver`，不重复协议代码。一个进程要供多个 driver id 时用
  `serve_many(vec![as_sidecar(A), as_sidecar(B)], …)`（`mysql` / `postgres` 这么用）。
- 单元测试（无需真实数据库）：

```powershell
$env:CARGO_TARGET_DIR = "D:\RdataStation\RDS\RdataStation-v2\.rds\target-gf"
cargo test --manifest-path tools/plugin-drivers/rust-core/Cargo.toml
cargo test --manifest-path tools/plugin-drivers/oracle-rust/Cargo.toml
cargo test --manifest-path tools/plugin-drivers/mssql-rust/Cargo.toml
cargo test --manifest-path tools/plugin-drivers/sqlx-rust/Cargo.toml
```

- 插件包只放清单和二进制；每个包自己的 `build.ps1`（如
  `mssql-rust-plugin/build.ps1`）编 release 并复制为 `bin/win-x64/agent.exe`，
  宿主 `layout::resolve_executable` 按平台子目录解析。改完驱动源码后
  **三处副本要保持一致**（`<包>/bin/...`、`.rds/plugins/<id>/bin/...`、
  `.rds/target-gf/debug/plugins/<id>/bin/...`），否则跑的还是旧二进制。
- **换了新二进制，宿主会自己重启那个插件**（2026-11-12）：热插拔扫描不只看
  `plugin.toml` 的内容 hash，也看**载荷指纹**——插件目录里每个文件的相对路径 + 长度 +
  纳秒修改时间，加上清单 `[backend] driver_jars` 声明的**外部 jar / 目录**（那些在插件
  目录之外，路径是绝对路径）。只换 `agent.exe`、清单一个字节不动时，扫描在 5 秒内认出
  「壳没变、载荷变了」，自动 `deactivate → activate` 该插件（别的插件不受影响）：
  旧进程被停、旧会话如实作废，下一个连接用的就是新二进制。**不用重启宿主，也不用
  手动点重载**。
  但**那个插件上还有活动会话时不重启**（2026-11-13）：先退回「待重载」提示，等会话都
  关掉后由下一轮扫描自己补上——免得你正在跑查询时连接被静默断掉。代价是会话一直不断
  就一直不换，想立刻生效就手动点「重载」。
  两个现实约束：① Windows 上运行中的 exe 不能直接覆盖
  （`ERROR_SHARING_VIOLATION`），要先把旧的改名（`agent.exe` → `agent.exe.old`）
  再放新的——扫描认这种替换；② 用保留时间戳的工具覆盖（`robocopy /COPY:T` 之类）
  指纹不会变，那种情况仍需手动重载。
- 能力边界：两个驱动都声明 `cancel = false`（无法中断在飞查询），
  `tx.*` 走真实语句。这条不是偷懒：`oracle-rs 0.1.7` 与 `tiberius 0.12.3` 都只定义了
  BREAK / ATTENTION 协议常量，**没有公开的 cancel API**（`oracle-rs` 只有服务端发
  break 时的解析分支，`tiberius` 只有 `AttentionSignal` 常量）。要真中断得等上游开口子。
- **`meta.*` 不许在内核里留硬编码桩**（2026-11-12，硬约束）：`rust-core/src/server.rs`
  曾经把 `meta.routine_source` 直接写成 `Ok(Dispatch::Result(Value::Null))`，于是
  **所有 Rust 驱动的例程源码面板永远是空的**，而自检只看「null 是合法答案」照样全绿
  —— 假绿比报错更难发现。现在请求路由到 `Driver::routine_source`（默认 `Ok(None)`，
  「查不到」与「不支持」都表达成 null），各驱动真去查：Oracle 走 `all_source`
  （`type IN ('PROCEDURE','FUNCTION')`，按 `line` 拼回正文），SQL Server 走
  `sys.sql_modules.definition`（`type IN ('P','FN','IF','TF')`，视图不进例程通道）。
  实测（2026-11-12）：Oracle `XEPDB1 / MDSYS` 下 `ELOCATION_EDGE_LINK_LEVEL`、SQL Server
  `master / dbo` 下 `sp_MScleanupmergepublisher` 都从「对端说没有」变成「拿得到」。
  **新增任何 `meta.*` 时先看内核那条 match 是不是还在返回常量**——那是桩，不是实现。
- **Oracle 元数据 SQL 一律用绑定变量**（2026-11-12，硬约束）：`owner` / 对象名
  **不许拼进 SQL 文本**，一律 `owner = :1 AND table_name = :2` 传 `Value::String`
  （见 `oracle-rust/src/meta.rs` 六处）。理由是实测的：Oracle 按 SQL **文本**做硬解析，
  名字一变就是一条新语句，字典视图一次硬解析 3–4 秒——属性面板每打开一个新对象都卡
  3–4 秒，连续几十次之后服务端直接把会话踢掉。改绑定变量后同一个 100 对象全量探测：
  只有第一条（首次 prepare）2.4 秒，其余全部毫秒级。**改回拼接就等于把这 3–4 秒/对象
  重新加回来**。标识符（不能用绑定变量的场合）才走 `types::quote_literal`。
- **元数据路径要能自愈**（2026-11-12）：`oracle-rs` 那条连接在连着跑几十条字典查询
  之后会被服务端关掉（实测第 ~82 个对象报 `Oracle closed the connection without
  providing error details`），此后每个 `meta.*` 都是 `connection closed unexpectedly`。
  `query.execute` 一直有「失败 → `reconnect()` → 返回原错误」；`meta.catalogs/schemas/
  objects/object_detail` 现在也补了同一条策略（失败 → 重连 → 原样重试一次）。实测：
  断连那个对象退回 `indexes=None`，**下一个对象起恢复正常**。辅助查询（主键 / 索引 /
  行数）失败不再静默——写 stderr 并注明对象名，否则会被误读成「这表没索引」。
- 协议内核的 panic 隔离（2026-11-12）：`rust-core` 的 `serve` 用 `catch_unwind`
  把**单个请求**的 panic 转成 `-32603 internal`（`ErrorCode::Internal`），进程
  继续服务下一条请求。不隔离的代价是实测过的：MSSQL 导航面一次 `unwrap` 踩炸 →
  sidecar exit 101 → 宿主只看到 `帧 I/O 失败：early eof` → 同一实例上所有会话
  一起断，且无从知道是哪个对象炸的。宿主侧 `RpcErrorCode` **不收** `-32603`
  （它是 JSON-RPC 标准码，不属于驱动号段 `-32001…-32009`）：`conn.rs` 看到
  `proto::JSONRPC_INTERNAL_CODE` 就走独立的 `CallError::RpcInternal` →
  `DriverError::Internal`，界面文案是「驱动内部出错（该请求已隔离，可重试）」，
  既不混进「未知错误码」，也不冒充「连接失败」。回归证据：`rust-core` 的
  `a_panicking_request_is_isolated_and_the_process_keeps_serving` +
  `rds-plugin` 的 `call_errors_map_to_typed_driver_errors`。
- 真机口径（2026-11-12）：
  - **Oracle 已验收**：`sidecar_conformance` 对 `192.168.3.138:1521/XEPDB1` 6/6（含 3000 行分页）；
    `crates/plugin/tests/rust_driver_wiring.rs` 另有「真实安装目录 → `PluginManager` 激活 →
    supervisor 开会话 → 真库查询」用例（设置 `RDS_TEST_ORACLE_URL` 时执行），并在同一用例里
    用新的 `PluginRuntimeLoop::tick_once` 复现宿主启动首拍，验证「重启后自动加载 + 激活、
    `drivers.enabled` 回到 1」——即下拉能看到驱动的完整启动链路。2026-11-12 复跑
    `oracle_plugin_serves_a_real_query` 通过。**注意它的导航面是「空库合法通过」**：
    conformance 取 `schema[0] = DEVUSER`，而 `DEVUSER` 下 0 个对象，所以
    `meta.object_detail` / 例程源码两项被跳过——这只证明方法面答得出来，**不等于**
    对象详情路径被真机覆盖。补验办法：`RDS_SIDECAR_SCHEMA=SYS RDS_SIDECAR_CATALOG=XEPDB1`
    再跑一次自检（`SYS` 下 100 个对象），2026-11-12 实测 6/6 且详情真验到
    （首对象 10 列、索引 0、行数 0）。自检已加硬约束：**指定了 schema 却拿不出表 / 视图
    就判失败**，不再静默跳过（反向验证过：`RDS_SIDECAR_SCHEMA=DEVUSER` → 5 passed / 1 failed）。
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
