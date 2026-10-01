# rds-connection

数据源连接的**传输层 + 领域模型** crate。它只回答「怎么连」，不回答「连上后做什么」——编排、UI、持久化都在上游（workbench / engine）。

- 分类：传输层（M3 数据源连接）
- 上游消费方：`rds-workbench` 的 `services::connection_service` / `data_source_service` / `nav_runtime`，以及 engine 驱动层的联邦直连
- 依赖：内部依赖仅 `rds-shared`（错误、加密）；其余为传输/序列化等外部库（russh / native-tls / tokio-socks / duckdb 等，见 Cargo.toml）
- 测试：crate 内 96 项单测 + `tests/tunnel_roundtrip.rs` 集成（6 项，2026-10-01 扩充并修复后）

---

## 职责与边界

**本 crate 管：**
- 连接配置建模（`config`）
- URL 组装 / 脱敏 / 凭据提取（`url`）
- URL 参数注入与改写（认证 / SSL / Kerberos / 协议链 / no_proxy，`url_params`）
- 连接器的选择与创建（`factory`）
- 传输连接：Direct / SSL / HTTP 代理 / SOCKS 代理 / 协议链（`connector` / `stream` / `chain`）
- SSH known_hosts 解析与校验（`known_hosts`）
- 领域模型（`model`）：`DataSource` / `ConnectionScope` / 测试与删除结果
- DuckDB Secret 本地加速（`secret`，M3 差异化能力）

**本 crate 不管（也不要管）：**
- 不持有编排逻辑——`DataSourceService`（workbench）负责「存储 + 生命周期」编排
- 不碰任何 HTTP / IPC / UI
- 不知道具体数据库协议——它只拨 TCP / TLS / 代理 / SSH 隧道
- 数据库族目录与驱动声明落库在 **engine** 的 `driver/declaration.rs`，不在本 crate

## 公开接口

```text
pub mod chain;        // 协议链执行与隧道生命周期守卫（TunnelRegistry / apply_network_method）
pub mod config;       // ConnectionConfig + 连接方式枚举（Direct/SSL/SSH/HTTP/SOCKS/Chain）+ TlsRequest
pub mod connector;    // 连接器 trait 与实现（Direct/SSL/HTTP/SOCKS），SSH 隧道已改由 chain 层处理
pub mod factory;      // ConnectionFactory：按连接方式自动选连接器
pub mod known_hosts;  // SSH known_hosts 解析与校验
pub mod model;        // 领域模型：DataSource / ConnectionScope(G/P/GP) / SaveInput / Test/DeleteResult
pub mod secret;       // DuckDB Secret 本地加速（PERSISTENT Secret 注册/列出/删除）
pub mod stream;       // ConnectionStream：统一的异步传输流包装
pub mod url;          // URL 组装 / 脱敏 / 凭据提取
pub mod url_params;   // URL 参数注入与改写（认证/SSL/Kerberos/no_proxy/协议链）

pub use config::ConnectionConfig;
pub use connector::Connection;
pub use factory::ConnectionFactory;
pub use model::{ConnectionScope, DataSource, DataSourceSaveInput, DeleteResult, TestResult};
pub use stream::ConnectionStream;
```

## 关键设计判断

- **SSH 隧道不在连接器里**：`SshTunnelConnector` 已废弃，`connector.rs` 里 `connect(Ssh)` 直接返回 `NotSupported`（`supports(Ssh)` 仍返回 true，保证工厂能命中占位器再如实报错）；真实隧道由 `chain::apply_network_method` + service 层统一处理。这是刻意的——避免静默直连。
- **协议链（Chain）是重点**：SSH → 再代理等多跳由 `chain.rs` 串起来，`TunnelRegistry` 做隧道生命周期守卫（防泄漏 / 幂等）。
- **URL 凭据全程脱敏 + 百分号编码 + SQL 注入转义**；密码走 `rds-shared` 的 AES 加密。
- **secret.rs 是差异化护城河**：用 PERSISTENT Secret 落盘、跨会话可用；踩过 MySQL 认 `USER`、Postgres 认 `USERNAME` 的坑（有真机实测注释与回归测试）。

## 接线关系

- **workbench → connection**：`ConnectionService` 消费 `config` / `chain` / `url_params` / `secret`，把 `ConnectRequest` 变成可拨号配置；`nav_runtime` 复用同一 `ConnectionManager`（连接生命周期必须走 service 层，本 crate 不直接持有全局连接池）。
- **workbench → secret**：联邦直连源库时，`ConnectionService` 走 `secret.rs` 注册 DuckDB Secret。
- **engine ↔ connection**：connection 只被消费，不回依赖 engine；驱动声明 / 族目录在 engine 侧。

## 测试

- crate 内单测覆盖 URL 分派、参数注入、TLS 请求、Secret 注册的踩坑回归。
- `tests/tunnel_roundtrip.rs`：`apply_network_method` 隧道往返——本地回环 echo 服务 + 最小 HTTP CONNECT / SOCKS5 代理端（全 127.0.0.1 临时端口，不触外网）。
- 真实拨号回归在 workbench 侧（`official_driver_real.rs` / `connection_tunnel_cleanup.rs`）——本 crate 保持无 I/O 副作用。
