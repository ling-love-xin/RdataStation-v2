# Oracle 原生 Rust 驱动插件包

独立 sidecar 进程（`rds-oracle-sidecar`），走宿主既有的驱动插件安装 / stdio 帧协议，
**不编进主程序**、不依赖 JVM / OCI 客户端。

- 驱动实现：`../oracle-rust/`（crate `rds-driver-oracle`）
- 共享协议内核：`../rust-core/`（crate `rds-driver-sidecar`）
- 依赖：`oracle-rs 0.1.7`（纯 Rust TNS 实现，MIT OR Apache-2.0，最低 Oracle 12.1）

## 构建与安装

```powershell
# 只构建本包（release → bin/win-x64/agent.exe）
powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\oracle-rust-plugin\build.ps1

# 或随全部驱动包一起构建并安装到 .rds/plugins
powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\install.ps1
```

产物 `bin/` 不入库，由构建脚本生成。

## 连接参数

标准五件套：`host` / `port`（默认 1521）/ `username` / `password` / `database`（service name）。

`options` 里的额外项：

| 键 | 作用 |
| --- | --- |
| `sid` | 走 SID 连接（优先于 service name） |
| `service_name` / `service` | 显式 service name，覆盖 `database` |
| `connect_timeout` | 连接超时秒数（默认 10） |
| `stmtcachesize` | 语句缓存条数 |
| `ssl` / `tls` | `"true"` 时启用 TLS |

## 已声明能力与边界

- `schemas` / `transactions` 为 true；`cancel` / `cursor` / `streaming` 为 false——
  oracle-rs 无法中断在飞查询，协议层只能做结果返回后的取消判定。
- oracle-rs 默认关闭 auto-commit：非事务状态下执行 DML / DDL 由驱动侧补 `COMMIT`；
  `tx.begin` 只置标志（Oracle 隐式事务），`tx.commit` / `tx.rollback` 发真实语句。
- 未在真实实例上验收；当前验收口径为编译 + 协议单测。
