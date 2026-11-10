# 驱动插件包（tools/plugin-drivers）

「一个通用壳 + 每库一个插件包」的落地目录，另有 **Rust 原生 sidecar** 轨：每个库编一个
独立二进制 + `plugin.toml`，走同一套 stdio 帧协议，**不编进主程序**、不需要 Java / Python 运行时。

插件包 = manifest（`plugin.toml`）+ 驱动资产（jar / python 壳 / Rust 二进制），**不内嵌任何运行时**：
Java / Python 解释器由宿主 `crates/plugin/src/runtime.rs` 三级查找；Rust 驱动是自身的可执行文件。

## 结构

```
tools/plugin-drivers/
├── oracle-jdbc/            # rds.driver.oracle：通用 JDBC 壳 + ojdbc11（+ h2 无证书验证）
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
powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\install.ps1
```

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

- 插件包只放清单和二进制；`build.ps1` 编 release 并复制为 `bin/win-x64/agent.exe`，
  宿主 `layout::resolve_executable` 按平台子目录解析。
- 能力边界：两个驱动都声明 `cancel = false`（无法中断在飞查询），
  `tx.*` 走真实语句；未在真实实例上验收，当前口径 = 编译 + 协议单测。

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
