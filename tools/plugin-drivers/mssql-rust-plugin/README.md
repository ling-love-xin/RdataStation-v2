# SQL Server 原生 Rust 驱动插件包

独立 sidecar 进程（`rds-mssql-sidecar`），走宿主既有的驱动插件安装 / stdio 帧协议，
**不编进主程序**、不依赖 JVM / ODBC 驱动管理器。

- 驱动实现：`../mssql-rust/`（crate `rds-driver-mssql`）
- 共享协议内核：`../rust-core/`（crate `rds-driver-sidecar`）
- 依赖：`tiberius 0.12`（TDS 协议直连，MIT；`default-features = false` + `tds73` / `rustls` / `sql-browser-tokio`）

## 构建与安装

```powershell
# 只构建本包（release → bin/win-x64/agent.exe）
powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\mssql-rust-plugin\build.ps1

# 或随全部驱动包一起构建并安装到 .rds/plugins
powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\install.ps1
```

产物 `bin/` 不入库，由构建脚本生成。

## 连接参数

`host` / `port`（默认 1433）/ `username` / `password` / `database`。
unix 平台缺 `username` 会直接报错；Windows 缺 `username` 时走集成认证。

`options` 里的额外项：

| 键 | 作用 |
| --- | --- |
| `encrypt` | `true` / `"required"` 时强制加密；默认 `false`（仅登录过程加密） |
| `trust_cert` | 默认 `true`，接受自签证书；显式 `false` 才严格校验 |
| `instance_name` | 命名实例（仅 Windows，走 SQL Browser 的 1434） |

## 已声明能力与边界

- `schemas` / `transactions` 为 true；`cancel` / `cursor` / `streaming` 为 false。
- 事务走 `BEGIN / COMMIT / ROLLBACK TRANSACTION` 语句（tiberius 的 `Client` 没有事务入口）。
- `SMALLDATETIME` 的第二段按「自午夜起的分钟数」解释——这是 TDS `smalldatetime` 的语义，
  与 tiberius 内部 `DateTime` 的 1/300 秒口径不同，驱动侧按前者显式换算。
- 未在真实实例上验收；当前验收口径为编译 + 协议单测。
