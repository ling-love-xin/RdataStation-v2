# connection crate 接口与接线

> 定位：`crates/connection` 是数据源连接的**传输层 + 领域模型** crate。本文聚焦它**对外暴露的接口**（`lib.rs` 的 10 个 `pub mod` + 顶层 `pub use`）以及 **workbench / engine 如何接线消费**。模块级产品行为见同目录 `README.md`。
>
> 只读建议：新增数据库类型**不需要动本 crate**（连接与数据库协议无关）；要动的是 engine 的驱动声明 / 族目录（见 `docs/architecture/driver-capability-matrix.md`）。

---

## 1. 公开接口总览

`lib.rs` 导出 10 个 `pub mod`，另有 5 个顶层 `pub use`：

```text
pub mod chain;        // 协议链执行 + 隧道生命周期守卫
pub mod config;       // 连接配置与连接方式枚举
pub mod connector;    // 连接器 trait 与实现
pub mod factory;      // 连接工厂
pub mod known_hosts;  // SSH known_hosts 解析校验
pub mod model;        // 领域模型
pub mod secret;       // DuckDB Secret 本地加速
pub mod stream;       // 统一异步传输流
pub mod url;          // URL 组装/脱敏/凭据提取
pub mod url_params;   // URL 参数注入与改写

pub use config::ConnectionConfig;
pub use connector::Connection;
pub use factory::ConnectionFactory;
pub use model::{ConnectionScope, DataSource, DataSourceSaveInput, DeleteResult, TestResult};
pub use stream::ConnectionStream;
```

---

## 2. 各模块接口与职责

### 2.1 `config` — 连接配置（单向数据，被其余模块读）

| 项 | 作用 |
|---|---|
| `ConnectionConfig` | 一份完整连接配置（地址、认证、网络方式、TLS、代理、协议链） |
| `ConnectionMethod` | 连接方式：`Direct / Ssl / Ssh / HttpProxy / SocksProxy / Chain` |
| `ChainHop` | 协议链一跳（如"先 SSH 再走代理"） |
| `TlsRequest` / `SslConfig` / `TlsVersion` | TLS/SSL 请求与配置 |
| `SshConfig` / `SshAuth` | SSH 隧道配置与认证方式 |
| `ProxyConfig` / `ProxyAuth` | HTTP/SOCKS 代理配置与认证 |

### 2.2 `url` — URL 组装 / 脱敏 / 凭据提取

| 函数 | 作用 |
|---|---|
| `build_connection_url(ds: &DataSource)` | 由领域模型拼出连接 URL |
| `encode_userinfo(s)` | 对 URL userinfo 做百分号编码 |
| `mask_password_in_url(url)` | 脱敏（日志/界面不回显密码） |
| `extract_credentials_from_url(url)` | 从 URL 提取 user/password |
| `url_has_plaintext_password(url)` | 探测 URL 是否含明文密码 |

### 2.3 `url_params` — URL 参数注入与改写

| 函数 | 作用 |
|---|---|
| `inject_auth_into_url` / `inject_username_password` / `merge_credentials` | 认证参数注入 |
| `inject_ssl_cert` / `append_ssl_params` / `inject_chain_ssl_params` | SSL/TLS 参数注入（链式场景） |
| `inject_kerberos` | Kerberos 参数注入 |
| `rewrite_url_authority` / `rewrite_url_host_port` | 改写 host:port（代理/隧道后） |
| `parse_host_port_from_url` | 解析 host 与端口 |
| `normalize_url_scheme` | 归一 URL scheme |
| `append_url_params` | 追加查询参数 |
| `matches_no_proxy` | no_proxy 规则匹配 |
| `SslMode` | SSL 模式枚举 |

### 2.4 `connector` — 连接器 trait 与实现

| 项 | 作用 |
|---|---|
| `trait Connector` | 连接器抽象（SPI）：`connect()` 返回 `ConnectionStream`，工厂 `create_connection` 再包成 `Connection` |
| `Connection` | 一条已建立的连接（含流 / 配置 / 建立时刻） |
| `DirectConnector` / `SslConnector` | 直连 / SSL 连接器 |
| `HttpProxyConnector` / `SocksProxyConnector` | 两种代理连接器 |
| `establish_ssh_tunnel` / `establish_http_proxy` / `establish_socks_proxy` | 各通道建立逻辑 |
| `SshTunnelConnector` | **已废弃占位**：`connect(Ssh)` 返回 `NotSupported`（`supports(Ssh)` 仍为 true），SSH 走 `chain` 层 |
| `TunnelGuard` | 隧道守卫（生命周期） |
| `check_cert_expiry(cert_path)` | 证书过期检查 |
| `wrap_tls_stream` | 把 stream 包装成 TLS |

### 2.5 `factory` — 连接工厂

| 项 | 作用 |
|---|---|
| `ConnectionFactory` | 按 `ConnectionConfig` 自动选连接器并建连 |
| `get_connection_factory()` | 返回全局单例工厂 |
| `create_connection(config)` | 便捷入口 |

### 2.6 `chain` — 协议链执行

| 项 | 作用 |
|---|---|
| `TunnelRegistry` | 隧道生命周期守卫（幂等 / 防泄漏） |
| `apply_network_method` | 把连接方式展开成实际拨号路径（SSH 隧道走这里） |
| `process_chain` | 执行多跳协议链 |

### 2.7 `stream` — 统一异步传输流

| 项 | 作用 |
|---|---|
| `enum ConnectionStream` | 统一的异步流（TCP / TLS / 代理 / 隧道后的流） |
| `trait StreamWrapper` | 流包装抽象（`AsyncRead + AsyncWrite`） |
| `StreamInfo` | 流信息（对端、是否 TLS 等） |

### 2.8 `known_hosts` — SSH host key 校验

| 项 | 作用 |
|---|---|
| `KnownHosts` | known_hosts 解析器 |
| `create_known_hosts_checker(allow_unknown)` | 生成校验器（是否允许未知主机） |

### 2.9 `secret` — DuckDB Secret 本地加速（M3 差异化能力）

| 项 | 作用 |
|---|---|
| `SECRET_TYPE_POSTGRES / MYSQL / SQLITE / DUCKDB / S3` | 支持的 Secret 类型常量 |
| `DatabaseCredential` | 凭据载体 |
| `SecretInfo` | Secret 元信息 |
| `SecretManager` | 注册 / 列出 / 删除 Secret（PERSISTENT 落盘） |
| `SecretError` / `SecretResult` | 错误与结果别名 |

> 踩坑记录：MySQL 认 `USER`、Postgres 认 `USERNAME` —— 新增 Secret 类型务必去 DuckDB 官方确认参数名（`secret.rs` 有真机实测注释与回归测试）。

### 2.10 `model` — 领域模型

`DataSource`（ID 前缀双轨制 G_/P_/GP_）、`ConnectionScope`、`DataSourceSaveInput`、`TestResult`、`DeleteResult`，字段对齐 `global_connections` 表。

---

## 3. 接线关系（谁消费本 crate）

### 3.1 workbench（主消费方）

- `services::connection_service::ConnectionService` —— 消费 `config` / `chain` / `url_params` / `secret`，把 `ConnectRequest` 转成可拨号配置；**连接生命周期必须走 service 层**，本 crate 不持有全局连接池。
- `services::data_source_service::DataSourceService` —— 负责「存储 + 生命周期」编排，`build_probe_config` 与 `connect` 都经 `ConnectionService`。
- `services::nav_runtime` —— 复用同一 `ConnectionManager`，只留需宿主服务的部分。

### 3.2 engine

- connection 只被 engine 消费，**不回依赖 engine**。
- 联邦直连源库时，经 workbench 的 `ConnectionService` 走 `secret.rs` 注册 DuckDB Secret。
- 驱动声明 / 族目录在 engine 侧（`driver/declaration.rs`），不属本 crate。

### 3.3 依赖

本 crate 依赖仅 `rds-shared`（错误 `CoreError`、AES 加密）。无 HTTP / IPC / UI。

---

## 4. 关键设计判断（改动前必读）

1. **SSH 隧道不在连接器里**：`SshTunnelConnector` 是刻意的废弃占位，防静默直连；真实隧道走 `chain::apply_network_method`。
2. **协议链（Chain）**：多跳（SSH→代理…）由 `chain.rs` 串，`TunnelRegistry` 防隧道泄漏。
3. **凭据安全**：URL 百分号编码、SQL 注入转义、密码 AES 加密、日志/界面脱敏。
4. **本 crate 与"新增数据库"解耦**：加数据库类型改 engine 驱动，不碰传输层。

---

## 5. 测试锚点

- crate 内 96 项单测（2026-10-01 扩充并修复后）：URL 分派、参数注入、TLS 请求、Secret 踩坑回归、连接器错误分支、隧道守卫生命周期等。
- `tests/tunnel_roundtrip.rs`：`apply_network_method` 隧道往返——本地回环 echo + 最小 HTTP CONNECT / SOCKS5 代理端（全 127.0.0.1 临时端口，不触外网）。
- 真实拨号回归在 workbench（`official_driver_real.rs` / `connection_tunnel_cleanup.rs`）——本 crate 保持无 I/O 副作用。
