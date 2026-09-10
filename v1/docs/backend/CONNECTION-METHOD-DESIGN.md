# 网络连接方式 迭代设计文档

> 版本：v1.4
> 更新：2026-05-19
> 状态：🚧 开发中 — v0.6.0 后端增量：ID前缀规范 + 快照机制 + SSH Agent + known_hosts + Proxy→SSL嵌套
> 目标版本：v0.6.0 / v0.7.0

---

## 一、概述

本文档覆盖 RdataStation 数据库连接层的三种网络连接方式：

| 连接方式         | 英文                | 典型场景                             |
| ---------------- | ------------------- | ------------------------------------ |
| **SSH 隧道**     | SSH Tunnel          | 通过跳板机/堡垒机访问内网数据库      |
| **SSL/TLS 加密** | SSL/TLS             | 数据库连接加密传输，证书校验         |
| **代理**         | Proxy（HTTP/SOCKS） | 通过公司代理或 SOCKS5 访问外部数据库 |

三者在架构中统一建模为 `ConnectionMethod` 枚举，通过 `ConnectionConfig` → `ConnectionService` → `Connector` 三层调度。

---

## 二、当前代码架构现状

### 2.1 代码文件索引

| 文件                                                                                                                              | 职责                                                              | 状态    |
| --------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ------- |
| [config.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/config.rs)                | ConnectionMethod / SshConfig / SslConfig / ProxyConfig 结构体定义 | ✅      |
| [connector.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/connector.rs)          | Connector trait + 5 个连接器实现 + SSH Agent + TLS wrapper        | ✅ v1.3 |
| [known_hosts.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/known_hosts.rs)      | SSH known_hosts 文件解析与 Host Key 校验                          | ✅ v1.3 |
| [stream.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/stream.rs)                | ConnectionStream 枚举（Tcp/Tls/SshTunnel/HttpProxy/SocksProxy）   | ✅      |
| [factory.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/factory.rs)              | ConnectionFactory 注册调度                                        | ✅      |
| [connection_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_service.rs) | 连接服务 + URL改写(SSH隧道)                                       | ✅ v1.1 |
| [connection_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/connection_commands.rs)    | Tauri Commands + network_config JSON解析                          | ✅ v1.1 |
| [network_store.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/network_store.rs)        | 网络配置持久化 CRUD                                               | ✅      |
| `migrations/global/008_add_data_source_module.sql`                                                                                | network_configs 表定义                                            | ✅      |
| `migrations/project_meta/010_add_data_source_module.sql`                                                                          | 项目级 network_configs 表                                         | ✅      |

### 2.2 架构分层

```
ConnectionService (服务层) ── URL 改写 + TunnelGuard 生命周期管理
├── tunnels: HashMap<String, Vec<TunnelGuard>>  ← 连接→隧道守卫映射
├── apply_network_method() → Result<(String, Vec<TunnelGuard>), CoreError>
│   ├── Single: SSH/SSL/Proxy → 对应隧道 + 返回 TunnelGuard
│   └── Chain: 迭代 hops → process_chain() → 层层端口转发 + 收集 TunnelGuard
├── connect_with_type() → 注册 TunnelGuard 到 tunnels map
├── close_connection() → 从 map 移除 + drop TunnelGuard（自动清理）
└── close_all_connections() → 清理所有 TunnelGuard

Connector 层（基础设施）
├── TunnelGuard ← 隧道生命周期守卫（Drop 自动发送关闭信号 + abort 后台任务）
├── establish_ssh_tunnel() → accept 循环 + oneshot 关闭 → TunnelGuard
├── create_proxy_tunnel_port() → accept 循环 + oneshot 关闭 → TunnelGuard
├── establish_tls() / establish_http_proxy() / establish_socks_proxy()
└── check_cert_expiry() ← x509-parser 证书过期检测

ConnectionFactory (调度层)
├── DirectConnector  ← TCP 直连 ✓
├── SslConnector     ← SSL/TLS 加密 ✓ CA证书/mTLS
├── SshTunnelConnector ← SSH 隧道（已废弃，由 service 层直接调用 establish_ssh_tunnel）
├── HttpProxyConnector ← HTTP 代理（CONNECT）✓
└── SocksProxyConnector ← SOCKS5 代理 ✓

create_database(url) → DataSourceRouter::route(config) → DynDatabase
```

### 2.3 API 接口说明

#### 2.3.1 Tauri Command 层

```typescript
// connect_database 接受 network_config_id，自动解析为 ConnectionMethod
{
  db_type: "mysql",
  url: "mysql://user@host:3306/db",
  network_config_id: "net_abc123",  // 引用网络配置
  // ... 其他字段
}
```

#### 2.3.2 网络配置 JSON 格式

```json
// SSH 隧道配置
{
  "network_type": "ssh",
  "config": {
    "host": "ssh.example.com",
    "port": 22,
    "username": "jumpuser",
    "auth_type": "password",
    "password": "...",
    "remote_host": "db.internal",
    "remote_port": 3306,
    "local_port": 0
  }
}

// SSL 配置
{
  "network_type": "ssl",
  "config": {
    "verify_server_cert": true,
    "ca_cert_path": "/path/to/ca.pem",
    "client_cert_path": "/path/to/client-cert.pem",
    "client_key_path": "/path/to/client-key.pem",
    "min_tls_version": "tls1_2"
  }
}
```

#### 2.3.3 网络配置测试命令

```typescript
// test_network_config 独立测试网络配置连通性
// Rust 返回类型: TestNetworkConfigResponse
interface TestNetworkConfigResponse {
  success: boolean
  message: string
  response_time_ms: number
  detail: string | null
}

// SSH: 建立完整隧道测试
// SSL: 验证证书文件存在
// Proxy: 测试 HTTP CONNECT / SOCKS5 代理连通性
invoke('test_network_config', { network_config_id: 'net_abc123' })
```

### 2.4 五种 Connector 实现完成度矩阵

| 连接器              | 结构体定义 | 工厂注册 | 运行时实现   | 服务层集成    | 状态    |
| ------------------- | ---------- | -------- | ------------ | ------------- | ------- |
| DirectConnector     | ✅         | ✅       | ✅           | ✅ 透传       | ✅ 完成 |
| SslConnector        | ✅         | ✅       | ✅ CA+mTLS   | ✅ 透传(sqlx) | ✅ 完成 |
| SshTunnelConnector  | ✅         | ✅       | ✅ russh隧道 | ✅ URL改写    | ✅ 完成 |
| HttpProxyConnector  | ✅         | ✅       | ✅ CONNECT   | ✅ URL改写    | ✅ 完成 |
| SocksProxyConnector | ✅         | ✅       | ✅ SOCKS5    | ✅ URL改写    | ✅ 完成 |

---

## 三、SSH 隧道（SSH Tunnel）✅ v0.5.0 已实现

### 3.1 场景地图

```
场景 A：单跳 SSH 隧道 ✅ 已实现
  本机 ──SSH──► 跳板机 ──TCP──► 数据库:3306
  用户先 SSH 到跳板机，再通过端口转发连数据库

场景 B：多跳 SSH 级联（堡垒机链）🚧 待 v0.6.0
  本机 ──SSH──► 堡垒机A ──SSH──► 堡垒机B ──TCP──► 数据库:5432

场景 C：SSH 隧道 + 端口映射 ✅ 已实现
  localhost:13306 ──SSH──► 跳板机 ──► 内网DB:3306
  前端通过 localhost:tunnel_port 访问，底层走 SSH 隧道

场景 D：SSH Agent 转发 ✅ v0.6.0 已实现（Unix only，Windows 返回 NotSupported）

场景 E：本地端口自动分配 ✅ 已实现
  localhost:0 → 系统自动分配空闲端口 → SSH → DB

场景 F：SSH 连接超时与自动重连 🚧 待 v0.7.0
```

### 3.2 当前结构体设计

[config.rs:60-82](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/config.rs#L60-L82)：

```rust
pub struct SshConfig {
    pub host: String,          // SSH 服务器主机
    pub port: u16,             // SSH 服务器端口（默认 22）
    pub username: String,      // SSH 用户名
    pub auth: SshAuth,         // 认证方式
    pub remote_host: String,   // 目标 DB 主机
    pub remote_port: u16,      // 目标 DB 端口
    pub local_port: u16,       // 本地绑定端口（0=自动）
    pub timeout_secs: u64,     // 超时
}
```

### 3.3 运行时实现（v1.1 已完成）

[connector.rs:286-429](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/connector.rs#L286-L429) `establish_ssh_tunnel`：

```
实际执行流（基于 russh 0.49.2）：
  1. russh::client::connect → SSH 协议握手
  2. authenticate_password / authenticate_publickey → 用户认证
  3. TcpListener::bind(local_bind) → 绑定本地端口
  4. tokio::spawn → 后台任务 accept 本地连接
  5. channel_open_direct_tcpip → 通过 SSH 打开到远程 DB 的端口转发
  6. channel.into_stream() → ChannelStream (AsyncRead + AsyncWrite)
  7. tokio::io::split → 双向拷贝（local ↔ channel）
  8. TcpStream::connect(local_addr) → 返回本地 TcpStream
```

**关键技术点**：

1. **认证方式**：
   - `Password` → `session.authenticate_password(username, password).await`
   - `PrivateKey` → `russh::keys::load_secret_key()` + `PrivateKeyWithHashAlg::new(key, None)` + `session.authenticate_publickey(username, key)`
   - `Agent` → ✅ v0.6.0 已实现（仅 Unix）：`connect_ssh_agent()` → `agent.request_identities()` → 遍历尝试 `session.authenticate_publickey_with(username, pubkey, &mut agent)` → 首个成功即通过。Windows 返回 `NotSupported` 错误。

2. **Host Key 校验** ✅ v0.6.0 已实现：
   - `SshClientHandler` 持有 `KnownHosts` 实例，在 `check_server_key` 回调中调用 `known_hosts.verify(host, port, server_key)`
   - 默认使用 `allow_unknown=true` 模式（未知主机允许通过但记录日志）
   - 密钥不匹配时拒绝连接并记录 `ERROR` 级别日志（MITM 攻击告警）

3. **Channel 双向拷贝**：

   ```rust
   let mut channel_stream = channel.into_stream();
   let (mut local_read, mut local_write) = tokio::io::split(local_stream);
   let (mut channel_read, mut channel_write) = tokio::io::split(&mut channel_stream);
   tokio::join!(
       tokio::io::copy(&mut local_read, &mut channel_write),
       tokio::io::copy(&mut channel_read, &mut local_write),
   );
   ```

4. **Session 生命周期管理**：使用 `Arc<tokio::sync::Mutex<Handle>>` 包装，auth 阶段 lock→authenticate→drop lock，端口转发阶段 clone Arc 到 spawn task。

### 3.4 服务层集成：URL 改写 + 隧道生命周期管理

[connection_service.rs:441-506](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_service.rs#L441-L506)

```
apply_network_method() SSH 处理流程（v1.2 TunnelGuard 模式）：
  1. create_ssh_tunnel_port(ssh_config) → TunnelGuard
     - 内部：TcpListener::bind(127.0.0.1:0) → accept 循环（tokio::select!）
     - 每个 accept 的连接 → channel_open_direct_tcpip → ssh channel → 双向桥接
  2. guard.port() → local_port
  3. rewrite_url_host_port(url, "127.0.0.1", local_port)
     → mysql://user:pass@10.0.0.5:3306/db → mysql://user:pass@127.0.0.1:54321/db
  4. 返回 (rewritten_url, vec![guard])  ← TunnelGuard 随返回值生命周期传递
  5. connect_with_type() 将 TunnelGuard 存入 tunnels HashMap
     → 隧道在连接生命周期内保持存活（后台 accept 循环持续运行）
  6. close_connection() 从 tunnels 移除 → drop(TunnelGuard) → 发送 oneshot 关闭信号 → abort 后台任务
```

**TunnelGuard 生命周期时间线**：

```
建立：apply_network_method() → TunnelGuard::new() → accept 循环启动
      ↓
持有：tunnels HashMap 持有 → 数据库连接存活期（sqlx 通过 localhost:port 通信）
      ↓
清理：close_connection() → tunnels.remove() → drop(TunnelGuard)
      → shutdown_tx.send(()) → accept 循环收到信号 → break → listener drop → task 退出
      → task.abort()（兜底，确保任务终止）
```

### 3.5 下一步优化

| 优化项         | 版本      | 说明                                                 |
| -------------- | --------- | ---------------------------------------------------- |
| SSH Agent 转发 | ✅ v0.6.0 | `russh_keys::agent` 模块接入（仅 Unix）              |
| Host Key 校验  | ✅ v0.6.0 | `known_hosts` 模块解析 + `check_server_key` 回调校验 |
| 多跳隧道链     | v0.6.0    | hops 链 + 逐跳 SSH 连接                              |
| 自动重连       | v0.7.0    | 网络闪断后自动重建隧道                               |

---

## 四、SSL/TLS 加密 ✅ v0.5.0 已实现

### 4.1 场景地图

```
场景 A：单向 SSL（服务器证书校验）✅ 已实现
场景 B：双向 SSL/mTLS（客户端证书认证）✅ 已实现
场景 C：SSL with CA Certificate ✅ 已实现
场景 D：SSL 模式选择 🚧 待 v0.6.0
场景 E：自签名证书开发环境 ✅ 已实现
场景 F：SSL 证书过期检测 🚧 待 v0.6.0
```

### 4.2 当前结构体设计

```rust
pub struct SslConfig {
    pub verify_server_cert: bool,            // 是否验证服务器证书
    pub ca_cert_path: Option<String>,        // CA 证书路径
    pub client_cert_path: Option<String>,    // 客户端证书路径（mTLS）
    pub client_key_path: Option<String>,     // 客户端私钥路径（mTLS）
    pub min_tls_version: TlsVersion,         // 最低 TLS 版本（Tls1_0 ~ Tls1_3）
}
```

### 4.3 运行时实现（v1.1 已完成）

[connector.rs:119-230](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/connector.rs#L119-L230) `establish_tls`：

```
实际执行流（基于 native-tls + tokio-native-tls）：
  1. TlsConnector::builder()
       .danger_accept_invalid_certs(!verify_server_cert)  ← 自签名证书开关
       .min_protocol_version(map_tls_version(min))        ← TLS 版本约束
       .max_protocol_version(Tlsv12)                      ← 默认上限

  2. CA 证书（可选）
     std::fs::read(ca_cert_path) → native_tls::Certificate::from_pem() → builder.add_root_certificate()

  3. 客户端证书 mTLS（可选）
     std::fs::read(cert_path + key_path) → Identity::from_pkcs8()
     失败时 fallback PKCS#12: Identity::from_pkcs12(cert, "")

  4. tokio_connector.connect(domain, tcp_stream).await
```

### 4.4 服务层集成

SSL 在 service 层**不做特殊处理**（透传 URL），因为 sqlx 原生支持 SSL 参数：

- MySQL: `mysql://host/db?ssl-mode=VERIFY_CA&ssl-ca=/path/ca.pem`
- PostgreSQL: `postgres://host/db?sslmode=verify-ca&sslrootcert=/path/ca.pem`

当前 SslConfig 用于 connector 层的 TLS 流加密，未来 v0.6.0 可统一两套 SSL 机制。

### 4.5 下一步优化

| 优化项           | 版本   | 说明                                        |
| ---------------- | ------ | ------------------------------------------- |
| SSL 模式自动映射 | v0.6.0 | MySQL 5 级 / PostgreSQL 6 级 → SslMode 枚举 |
| 证书过期检测     | v0.6.0 | peer_certificate() → 过期前告警             |
| 密码套件配置     | v0.7.0 | 高级安全需求                                |

---

## 五、代理（Proxy）

### 5.1 场景地图

```
场景 A：HTTP 代理（公司内网出口）✅ Connector层实现
场景 B：SOCKS5 代理（无认证）✅ Connector层实现
场景 C：SOCKS5 代理（用户名/密码认证）✅ Connector层实现
场景 D：代理旁路规则（no_proxy）🚧 未实现
场景 E：代理链（多个代理串联）🚧 未实现
场景 F：不同数据库走不同代理 🚧 未实现
```

### 5.2 当前状态

HTTP Proxy 和 SOCKS5 Proxy 的 **Connector 层已完整实现**，并通过**本地端口转发 + URL 改写**模式集成到 `ConnectionService` 服务层。

**已实现**：

- `apply_network_method()` 中对 `HttpProxy`/`SocksProxy` 调用 `create_proxy_tunnel_port()` → `rewrite_url_host_port()`
- URL 解析自动识别目标数据库主机端口，默认 MySQL:3306 / PostgreSQL:5432
- HTTP CONNECT 代理 Basic Auth 认证、SOCKS5 代理用户名密码认证
- `test_network_config` Tauri Command 独立测试代理连通性
- `no_proxy` 规则匹配：主机名/IP/域名后缀通配（单跳 + 链式集成）
- Proxy 隧道 accept 循环 + oneshot 关闭（TunnelGuard 管理）

### 5.3 下一步优化

| 优化项                   | 版本      | 说明                              |
| ------------------------ | --------- | --------------------------------- |
| 代理链支持（多代理串联） | v0.6.0    | upstream_proxy 嵌套               |
| NTLM 认证                | v0.7.0    | 企业环境常见需求                  |
| Proxy → SSL 嵌套         | ✅ v0.6.0 | TlsStream 包装代理流（见 5.4 节） |

### 5.4 Proxy → SSL 嵌套（TlsStream 包装）✅ v0.6.0 已实现

代理 CONNECT 建立隧道后，对透传 TCP 流进行 TLS 加密封装。sqlx 通过隧道本地端口连接时，数据在代理隧道内已经过 TLS 加密，最终数据库服务器收到的是 TCP+TLS 流量。

**核心原理**：

```
本机:port ── 代理隧道(HTTP CONNECT/SOCKS5) ──► 代理服务器 ──TCP──► 数据库服务器
                                                                    │
                             本机端: TcpStream → wrap_tls_stream() → TlsStream → TLS 握手
                             服务器端看到的: TLS ClientHello → TLS 加密流量
```

**实现要点**：

1. **`connector::wrap_tls_stream()`** — 在 TcpStream 之上建立 TLS 层：
   - 构建 `native_tls::TlsConnector`（CA 证书、客户端证书 mTLS、verify 模式）
   - `tokio_native_tls::TlsConnector::connect(server_host, tcp_stream)` → TlsStream
   - 返回 `tokio_native_tls::TlsStream<tokio::net::TcpStream>`

2. **`create_proxy_tunnel_port()` TLS-aware accept 循环**：
   - 新增 `wrap_ssl: Option<SslConfig>` 参数
   - accept 接受本地连接 → 建立代理隧道 → 可选调用 `wrap_tls_stream()` 封装 TLS
   - 桥接时使用 `local_stream ↔ tls_stream`（而非 `local_stream ↔ proxy_stream`）

3. **`process_chain()` 跳邻接检测**：
   - 当 Proxy 跳的下一跳是 Ssl 时，提取 `ssl_cfg` 传入 `wrap_ssl` 参数
   - 代码：`let wrap_ssl = match next_hop { Some(ChainHop::Ssl(ref ssl_cfg)) => Some(ssl_cfg.clone()), _ => None };`

4. **`inject_chain_ssl_params()` 跳过逻辑**：
   - 当 SSL 跳的前一跳是 Proxy 时，跳过 URL 参数注入
   - 因为 TLS 加密已由 Proxy→SSL 嵌套层在代理隧道内完成

**使用示例**：

```json
// Chain: [HttpProxy, Ssl] → DB
{
  "network_type": "chain",
  "config": [
    { "type": "http_proxy", "host": "proxy.corp.com", "port": 8080 },
    { "type": "ssl", "verify_server_cert": true, "ca_cert_path": "/etc/ssl/ca.pem" }
  ]
}
```

**数据流**：

```
sqlx → localhost:13301 → HTTP CONNECT → proxy.corp:8080 → db.external.com:3306
                              │
                              └── TcpStream → wrap_tls_stream(ca.pem) → TlsStream
                                   └── TLS ClientHello → TLS 1.2/1.3 握手 → 加密数据传输
```

## 六、统一连接配置流程（ConnectionConfig Pipeline）

### 6.1 前端 → 后端 连接建立全链路（v1.1 已实现）

```
┌─ 前端 ───────────────────────────────────────────────────────────────────┐
│  ConnectDatabaseInput {                                                   │
│    db_type: "mysql",                                                      │
│    url: "mysql://user@host:3306/db",                                      │
│    connection_type: "project",                                            │
│    network_config_id: "net_abc123",   ← 引用预配置的网络配置              │
│  }                                                                        │
└──────────────────────────────────────────────────────────────────────────┘
                                    ↓
┌─ connect_database Command ───────────────────────────────────────────────┐
│  1. 校验 driver / environment / auth 三步链                                │
│  2. parse_network_method(network_config_id):                              │
│     - 查询 global/project DB 的 network_configs 表                        │
│     - parse_config_json(network_type, config):                            │
│       "ssh"  → serde_json → SshConfig → ConnectionMethod::Ssh            │
│       "ssl"  → serde_json → SslConfig → ConnectionMethod::Ssl            │
│       "proxy"→ serde_json → ProxyConfig→ ConnectionMethod::HttpProxy/Socks│
│  3. service.connect_with_type(..., network_method)                       │
└──────────────────────────────────────────────────────────────────────────┘
                                    ↓
┌─ ConnectionService ──────────────────────────────────────────────────────┐
│  connect_with_type():                                                     │
│    1. 参数校验（url 非空）                                                │
│    2. apply_network_method(url, &network_method, &conn_id):               │
│       ├── SSH: create_ssh_tunnel_port() → rewrite URL → localhost:port   │
│       ├── SSL: 透传 URL（sqlx 原生 SSL 参数）                             │
│       └── Proxy: create_proxy_tunnel_port() → rewrite URL → localhost:port                                   │
│    3. create_database(effective_url) → DataSourceRouter::route()         │
│    4. 保存到 ConnectionManager + SQLite（safe_url 使用原始 URL）          │
│    5. 保存到最近连接记录                                                   │
└──────────────────────────────────────────────────────────────────────────┘
```

### 6.2 连接方式组合矩阵

| 组合             | 是否支持  | 说明                                     |
| ---------------- | --------- | ---------------------------------------- |
| Direct           | ✅        | 最常见                                   |
| SSL only         | ✅        | 直接 TLS（CA证书 + mTLS）                |
| SSH only         | ✅        | russh 隧道 + URL 改写                    |
| Proxy only       | ✅        | URL改写 + 端口转发                       |
| Proxy → SSH      | ✅        | Proxy 隧道作为 SSH 握手通道              |
| SSH → SSH        | ✅        | 多跳堡垒机链                             |
| Proxy → SSH → DB | ✅        | 公司代理 → 跳板机 → 内网 DB              |
| SSH → SSL → DB   | ✅        | sqlx 在 SSH 隧道内处理 SSL               |
| Proxy → SSL → DB | ✅ v0.6.0 | Proxy 隧道内 TlsStream 嵌套（见 5.4 节） |

### 6.3 协议链路配置 JSON 格式

```json
// Proxy → SSH → DB 链式配置
{
  "network_type": "chain",
  "config": [
    {
      "type": "http_proxy",
      "host": "corp-proxy.example.com",
      "port": 8080,
      "auth": { "username": "user", "password": "pass" }
    },
    {
      "type": "ssh",
      "host": "bastion.example.com",
      "port": 22,
      "username": "jumpuser",
      "auth_type": "password",
      "password": "...",
      "remote_host": "db.internal",
      "remote_port": 3306
    }
  ]
}
```

### 6.4 协议链路处理流程

```
process_chain(url="mysql://db.internal:3306/db", hops=[Proxy→SSH]):

Hop 0 (HttpProxy): target = SSH hop 的 host:port = "bastion.example.com:22"
  create_proxy_tunnel_port(proxy, "bastion.example.com", 22)
  → localhost:13301 → proxy → bastion:22
  tunnel_port = Some(13301)

Hop 1 (Ssh): connect_override = ("127.0.0.1", 13301)
  create_ssh_tunnel_port(ssh, connect_override)
  → 通过 localhost:13301 (经 proxy) SSH 握手到 bastion
  → channel_open_direct_tcpip("db.internal", 3306)
  → localhost:13302 → SSH → db.internal:3306
  tunnel_port = Some(13302)

Final: rewrite_url(url, "127.0.0.1", 13302)
  → "mysql://127.0.0.1:13302/db"
  sqlx 连接 localhost:13302 → proxy → SSH → DB ✅
```

**核心原理：每跳创建一个本地端口转发，本地端口作为下一跳的 TCP 入口，层层嵌套。**

### 6.5 协议链路深度分析

| 层数 | 场景示例                 | 延迟增量 | 可行性      |
| ---- | ------------------------ | -------- | ----------- |
| 1 层 | Proxy→DB, SSH→DB, SSL→DB | +5ms     | ✅ 基准     |
| 2 层 | Proxy→SSH→DB, SSH→SSH→DB | +15ms    | ✅ 常见     |
| 3 层 | Proxy→SSH→Proxy→DB       | +30ms    | ✅ 企业级   |
| 4 层 | Proxy→SSH1→SSH2→Proxy→DB | +50ms    | 🟡 罕见     |
| 5 层 | Proxy×2→SSH×2→Proxy→DB   | +75ms    | 🟡 极端     |
| 6 层 | Proxy×3→SSH×2→Proxy→DB   | +100ms   | 🔴 工程上限 |

**6 层为工程上限原因：**

- 每层消耗 1 个 localhost TCP 端口（OS 资源） + 1 个 tokio 桥接任务（~8KB 栈）
- 延迟叠加：6 层 = 6× localhost 往返 + 3-6× 网络往返
- TCP 拥塞控制失控：每层独立 TCP 窗口，N 层 = N× bufferbloat
- 实际堆栈：
  ```
  本机 localhost:P1 ── Proxy1 ──► ....(internet)....
  localhost:P2 ── SSH1 ──► bastion1:22
  localhost:P3 ── Proxy2 ──► internal_proxy:8080
  localhost:P4 ── SSH2 ──► bastion2:22
  localhost:P5 ── Proxy3 ──► proxy3.internal:3128
  localhost:P6 ── 最终路由 ──► db.internal:3306
  ```

### 6.6 SSL 模式智能感知

SslConfig 字段根据数据库类型自动映射为 URL 查询参数：

| SslConfig 字段                             | MySQL 参数                 | PostgreSQL 参数             |
| ------------------------------------------ | -------------------------- | --------------------------- |
| `verify_server_cert=true` + `ca_cert_path` | `ssl-mode=VERIFY_CA`       | `sslmode=verify-ca`         |
| `verify_server_cert=true` (无 CA)          | `ssl-mode=REQUIRED`        | `sslmode=require`           |
| `verify_server_cert=false`                 | `ssl-mode=REQUIRED`        | `sslmode=require`           |
| `ca_cert_path`                             | `&ssl-ca=/path/ca.pem`     | `&sslrootcert=/path/ca.pem` |
| `client_cert_path`                         | `&ssl-cert=/path/cert.pem` | `&sslcert=/path/cert.pem`   |
| `client_key_path`                          | `&ssl-key=/path/key.pem`   | `&sslkey=/path/key.pem`     |

**单跳 SSL 示例：**

```
输入 URL:  mysql://user@db.internal:3306/mydb
SslConfig: { verify_server_cert: true, ca_cert_path: "/etc/ssl/ca.pem" }
输出 URL:  mysql://user@db.internal:3306/mydb?ssl-mode=VERIFY_CA&ssl-ca=/etc/ssl/ca.pem
```

**链式 SSL 示例（Proxy → SSH → SSL → DB）：**

```
Chain: [HttpProxy, Ssh, Ssl]
Step 1-2: Proxy + SSH 隧道建立（端口转发）
Step 3:   在最终 localhost URL 上追加 SSL 参数
结果:     mysql://127.0.0.1:13303/mydb?ssl-mode=VERIFY_CA&ssl-ca=/etc/ssl/ca.pem
```

### 6.9 TunnelGuard 隧道生命周期管理（v1.2 重构）

**问题背景**：v1.0/v1.1 的隧道实现存在严重的生命周期泄漏：

- `drop(tunnel_stream)` 在 `apply_network_method()` 中过早释放本地连接，导致 sqlx 无法通过隧道通信
- 后台 accept 任务只接受**一次**连接（无循环），sqlx 连接池的其他连接无法复用隧道
- 链式跳转的中间跳隧道在下一跳建立前被销毁，下一跳无法通过中间隧道

**解决方案**：[connector.rs:UnitGuard](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/connector.rs#L21-L76) 结构体：

```rust
pub struct TunnelGuard {
    port: u16,                                          // 本地监听端口
    shutdown_tx: Option<oneshot::Sender<()>>,           // 优雅关闭信号
    task: Option<tokio::task::JoinHandle<()>>,          // 后台 accept 循环任务
    label: String,                                       // 隧道标识（日志用）
}

impl Drop for TunnelGuard {
    fn drop(&mut self) {
        // 1. 发送 oneshot 关闭信号 → accept 循环优雅退出
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        // 2. abort 后台任务（兜底，确保资源释放）
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
```

**核心设计**：

| 特性            | 实现                                                                               |
| --------------- | ---------------------------------------------------------------------------------- |
| **Accept 循环** | 后台任务使用 `tokio::select! { accept, shutdown_rx }` 持续接受连接                 |
| **多连接复用**  | 每个 accept 接受的连接独立 spawn 桥接任务，支持 sqlx 连接池多连接复用同一隧道      |
| **优雅关闭**    | oneshot channel 发送信号 → accept 循环退出 → drop(listener)                        |
| **强制中止**    | `task.abort()` 兜底，即使 accept 阻塞也能强制终止                                  |
| **自动清理**    | Drop trait 保证 TunnelGuard 离开作用域时自动触发清理                               |
| **连接级管理**  | `ConnectionService.tunnels: HashMap<String, Vec<TunnelGuard>>` 映射连接 → 隧道守卫 |

**生命周期集成点**：

```
connect_with_type():
  ├── apply_network_method() → (url, Vec<TunnelGuard>)  ← 隧道建立
  ├── tunnels.insert(conn_id, guards)                    ← 注册守卫
  └── create_database(url) → sqlx 连接                  ← 隧道存活期间使用

close_connection(conn_id):
  ├── tunnels.remove(conn_id) → Option<Vec<TunnelGuard>> ← 取出守卫
  └── drop(guards) → TunnelGuard::drop() 自动清理        ← 信号 + abort

close_all_connections():
  └── tunnels.clear() → 所有 TunnelGuard 自动 drop
```

**与旧实现的对比**：

| 方面           | 旧实现 (v1.0/v1.1 Bug)         | 新实现 (v1.2 TunnelGuard)                |
| -------------- | ------------------------------ | ---------------------------------------- |
| 隧道流生命周期 | `drop(tunnel_stream)` 过早释放 | TunnelGuard 在连接生命周期内持有         |
| 后台 accept    | **单次** accept（无循环）      | **无限循环** accept + tokio::select!     |
| 关闭机制       | 无（tokio::spawn 僵尸任务）    | oneshot 信号 + task.abort() 双重保证     |
| 多连接复用     | ❌ 不支持（单 accept）         | ✅ 支持（每个 accept 独立 spawn bridge） |
| 链式跳转       | ❌ 中间跳隧道被过早销毁        | ✅ Vec<TunnelGuard> 收集所有跳的守卫     |

### 6.10 no_proxy 规则匹配

ProxyConfig 支持 `no_proxy: Vec<String>` 字段，用于指定**绕过代理的主机列表**。
匹配规则沿袭 UNIX `no_proxy` 环境变量的惯例：

| 规则格式   | 示例          | 匹配行为                                              |
| ---------- | ------------- | ----------------------------------------------------- |
| 精确主机名 | `db.internal` | 精确匹配 `db.internal`                                |
| 域名后缀   | `.internal`   | 匹配 `*.internal`（如 `db.internal`, `svc.internal`） |
| IP 地址    | `192.168.1.1` | 精确匹配 IP                                           |
| localhost  | `localhost`   | 同时匹配 `localhost` / `127.0.0.1` / `::1`            |

**集成位置**：

- 单跳 Proxy：`apply_network_method()` 在建立隧道前检查
- 链中 Proxy 跳：`process_chain()` 在每跳处理前检查
- 匹配成功 → **跳过此代理跳**，日志记录

```json
{
  "host": "corp-proxy.example.com",
  "port": 8080,
  "no_proxy": ["localhost", ".internal", "192.168.0.0/16"]
}
```

### 6.11 SSL 证书过期检测

`connector::check_cert_expiry(path)` 基于 `x509-parser` 纯 Rust 实现，
支持 PEM 和 DER 格式，返回 `SslCertInfo`：

```
SslCertInfo {
  path, subject, issuer,
  not_before, not_after,
  days_until_expiry,  // 负数 = 已过期
  is_expired
}
```

已集成到 `test_network_config` SSL 测试中，同时检查 CA 证书和客户端证书。

```bash
# test_network_config SSL 输出示例
CA 证书文件存在: /etc/ssl/ca.pem
CA 证书: subject=CN=MyOrg CA, 过期日期=2027-12-31, 剩余365天
客户端证书文件存在: /etc/ssl/client.pem
客户端证书: subject=CN=app-user, 过期日期=2025-03-01, 剩余-79天 ⚠️ 已过期
```

---

## 七、v0.5.0 开发计划

### 7.1 已完成项 ✅

| 序号  | 任务                                                                           | 状态    |
| ----- | ------------------------------------------------------------------------------ | ------- |
| P0    | SSH 隧道：russh 握手 + 端口转发 + 双向拷贝                                     | ✅      |
| P0    | SSH 隧道：Password / PrivateKey 认证                                           | ✅      |
| P1    | SSL：CA 证书 + 客户端证书 mTLS (PKCS8/PKCS12)                                  | ✅      |
| P1    | SSL：TLS 版本映射 (TlsVersion → native_tls::Protocol)                          | ✅      |
| P2    | ConnectionService: apply_network_method() URL 改写                             | ✅      |
| P2    | ConnectionCommands: parse_network_method() JSON 解析                           | ✅      |
| P3    | cargo check + clippy -D warnings 全部通过                                      | ✅      |
| P3    | Proxy 代理 service 层集成（本地端口转发 + URL 改写）                           | ✅      |
| P3    | test_network_config Tauri Command（SSH/SSL/Proxy 连通性测试）                  | ✅      |
| P2    | SSH Host Key 指纹日志记录                                                      | ✅      |
| P1    | 协议链路（Chain）：Proxy→SSH / SSH→SSH / Proxy→SSH→DB                          | ✅      |
| P1    | 协议链路 Proxy 跳 connect_override（支持 Proxy→SSH→Proxy→DB）                  | ✅      |
| P1    | SSL 模式智能感知：SslConfig → MySQL ssl-mode / PostgreSQL sslmode              | ✅      |
| P3    | 协议链路深度分析：6 层工程上限                                                 | ✅      |
| P1    | no_proxy 规则匹配：主机名/IP/域名后缀 · 单跳+链式集成                          | ✅      |
| P2    | SSL 证书过期检测：x509-parser not_after · test_network_config 集成             | ✅      |
| 🔴 P0 | **TunnelGuard 生命周期重构**：accept循环 + oneshot关闭 + Drop清理 + 多连接复用 | ✅ v1.2 |
| 🔴 P0 | **ConnectionService 隧道守卫管理**：tunnels HashMap + close/disconnect 清理    | ✅ v1.2 |

### 7.2 待完成项 🚧

| 序号 | 任务                                    | 优先级    | 预估      |
| ---- | --------------------------------------- | --------- | --------- |
| F1   | 前端：网络配置管理 UI（CRUD + 测试）    | 🔴 P0     | 3天       |
| F2   | 前端：新建连接时选择网络配置            | 🔴 P0     | 1天       |
| F3   | ~~Proxy 代理 service 层集成~~           | ~~🟡 P1~~ | ✅ v0.5.0 |
| F4   | SSH 隧道集成测试（端到端）              | 🟡 P1     | 2天       |
| F5   | 结构化持久化表（ssh_tunnel_configs 等） | 🟡 P2     | 2天       |

### 7.3 后续版本规划

```
v0.6.0（后端 ✅ 核心完成，前端待开发）:
  ├── ✅ SSH Agent 转发（仅 Unix，Windows 返回 NotSupported）
  ├── ✅ Host Key 校验（known_hosts 模块 + check_server_key 回调）
  ├── ✅ Proxy → SSL 嵌套（TlsStream 包装代理流 + process_chain 邻接检测）
  ├── 🚧 Chain 硬校验（validate_chain：SSH/Proxy ≤ 4 跳、SSL 末尾约束）
  ├── 🚧 预置环境 Seed（5 个内置环境 + 25 条策略）
  ├── 🚧 IPC Commands（list_environments / list_network_configs_by_type 等）
  ├── 🚧 前端 NetworkTab 重写（动态协议链 + 拖拽 + 拓扑预览）
  └── 🚧 前端 AdvancedTab 改造（环境选择 + 策略联动）

v0.7.0（预计 5-7 天）:
  ├── SSH 隧道自动重连
  ├── NTLM 代理认证
  ├── TLS 1.3 密码套件配置
  └── 连接方式性能对比（Direct/SSL/SSH/Proxy 延迟监控）
```

---

## 八、前端 UI 设计要点

详见前端设计文档：`docs/frontend/NETWORK-CONFIG-UI-DESIGN.md`

### 核心页面

1. **网络配置管理页** — NetworkConfigManager.vue
   - 列表视图：所有网络配置（SSH/SSL/Proxy 分类 Tab）
   - 新建/编辑表单：根据 network_type 动态渲染表单字段
   - 测试连接按钮：验证 SSH/SSL/Proxy 配置

2. **新建连接对话框改造** — NewConnectionDialog.vue
   - 增加"网络配置"下拉选择框（可选）
   - 关联 network_config_id 字段

### 关键技术点

- dockview-vue 布局基座
- naive-ui 组件（NTabs, NSelect, NInput, NForm 等）
- lucide-vue-next 图标

---

## 九、附录

### A. 依赖项

| 依赖               | 用途                       | 版本   | 状态      |
| ------------------ | -------------------------- | ------ | --------- |
| `russh`            | SSH 协议实现               | 0.49.2 | ✅ 已使用 |
| `russh-keys`       | SSH 密钥管理               | 0.49.2 | ✅ 已使用 |
| `native-tls`       | TLS 实现                   | 0.2.13 | ✅ 已使用 |
| `tokio-native-tls` | 异步 TLS 包装              | 0.3.1  | ✅ 已使用 |
| `tokio-socks`      | SOCKS5 代理                | 0.5.2  | ✅ 已使用 |
| `base64`           | HTTP Proxy Basic Auth 编码 | 0.22.1 | ✅ 已使用 |

### B. 版本历史

| 版本 | 日期       | 说明                                                                                                                                                                   |
| ---- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| v1.4 | 2026-05-19 | ID前缀规范化 + 快照机制：G*/P*/GP\_ 三级前缀约定；新增 id_prefix.rs + snapshot_service.rs；project_meta 表增加 origin/source_id/snapshot_at 字段；文档新增第十三章     |
| v1.3 | 2026-05-19 | v0.6.0后端增量：SSH Agent认证(仅Unix) + known_hosts Host Key校验 + Proxy→SSL嵌套TlsStream包装 + process_chain邻接检测 + inject_chain_ssl_params跳过逻辑；文档新增5.4节 |
| v1.2 | 2026-05-19 | 🔴 TunnelGuard生命周期重构：accept循环+oneshot关闭+Drop清理+多连接复用；ConnectionService tunnels HashMap管理；文档新增6.9节                                           |
| v1.1 | 2026-05-19 | P0 SSH隧道 + P1 SSL证书 + service/cmd 集成 + Chain协议链路 + no_proxy + SSL证书过期检测                                                                                |
| v1.0 | 2026-05-19 | 初始规划文档，场景地图 + 结构体设计 + 三期路线图                                                                                                                       |

---

## 十、动态协议链设计（v0.6.0 核心特性）

### 10.1 设计目标

将当前固定的单一 `ConnectionMethod` 扩展为**动态有序协议链**，支持用户在数据源连接时自由组合 SSH 隧道、代理、SSL/TLS 加密，按序执行。

### 10.2 三种协议的本质差异

这是理解协议链设计的基础——三者做的事情完全不同：

```
协议     | 本质              | 产生新网络节点？ | 延迟代价        | 在链中的位置
─────────┼───────────────────┼─────────────────┼────────────────┼─────────────
SSH      | 隧道创建器         | ✅ 是            | 高 (5-30ms/跳)  | 任意位置
Proxy    | 流量中继器         | ✅ 是            | 中 (3-15ms/跳)  | 任意位置
SSL/TLS  | 流加密包装器       | ❌ 否            | 极低 (仅握手)   | 必须最后
```

**关键认知**：

- SSL/TLS 拿一个已建立好的 TCP 流做 TLS 握手，只能在链的最末端。不可能在已加密的 TLS 流上再建 SSH（SSH 需要 TCP 三次握手，TLS 流已是一个加密字节流）。
- SSH 和 Proxy 每增加一个，就在数据路径上插入一个真实的网络节点，增加一次 TCP 往返延迟。
- 每经过一个 SSH/Proxy 跳，网络的"立足点"就变了，新立足点可能有自己的网络约束，因此**交替穿插是完全合理的**。

### 10.3 协议链结构

```
正确的协议链结构：

  [N 个 SSH/Proxy 网络跳] → [可选 1 层 TLS 加密] → [DB]

  N = 网络跳数（SSH + Proxy 个数），TLS 不占跳数但占用末尾位置
```

### 10.4 全场景矩阵（N = SSH/Proxy 网络跳数）

#### 0 跳 — 频次 ~85%

| #   | 链路      | 频次 | 场景                                      |
| --- | --------- | ---- | ----------------------------------------- |
| 1   | 直连 → DB | 80%  | 本地开发、内网直连                        |
| 2   | TLS → DB  | 5%   | 云数据库强制 TLS（RDS/Cloud SQL），无跳板 |

#### 1 跳 — 频次 ~13%

| #   | 链路             | 频次 | 场景                    |
| --- | ---------------- | ---- | ----------------------- |
| 3   | SSH → DB         | 8%   | 单堡垒机访问内网库      |
| 4   | Proxy → DB       | 2%   | 公司代理出口访问外部 DB |
| 5   | SSH → TLS → DB   | 2.5% | 堡垒机 + 云数据库 TLS   |
| 6   | Proxy → TLS → DB | 0.5% | 代理出口 + TLS          |

#### 2 跳 — 频次 ~11%（含交替穿插）

| #   | 链路                   | 频次 | 场景                                      |
| --- | ---------------------- | ---- | ----------------------------------------- |
| 7   | **Proxy → SSH → DB**   | 3%   | ★ 公司代理 → 堡垒机 → 内网 DB，最常用多跳 |
| 8   | **SSH → Proxy → DB**   | 2%   | ★ 跳板机入内网 → 内网代理访问受限 DB      |
| 9   | SSH₁ → SSH₂ → DB       | 1.5% | 双跳板：DMZ + 生产网段                    |
| 10  | Proxy₁ → Proxy₂ → DB   | 0.3% | 代理链：公司代理 → 境外代理               |
| 11  | Proxy → SSH → TLS → DB | 2.5% | 代理 + 堡垒机 + TLS 云数据库              |
| 12  | SSH → Proxy → TLS → DB | 0.8% | VPN跳板 + 内网代理 + TLS                  |
| 13  | SSH₁ → SSH₂ → TLS → DB | 0.5% | 双跳板 + TLS                              |

#### 3 跳 — 频次 ~3%（交替穿插为主）

| #   | 链路                       | 频次 | 场景                                |
| --- | -------------------------- | ---- | ----------------------------------- |
| 14  | **SSH → Proxy → SSH → DB** | 1%   | ★ VPN跳板 → 内网代理审计 → 生产跳板 |
| 15  | Proxy → SSH₁ → SSH₂ → DB   | 1.2% | 代理 + 双跳板                       |
| 16  | Proxy₁ → Proxy₂ → SSH → DB | 0.4% | 双代理 + 跳板                       |
| 17  | SSH₁ → SSH₂ → Proxy → DB   | 0.2% | 双跳板入隔离区 + 代理出站           |
| 18  | +TLS 变体                  | 0.3% | 以上变体末尾加 TLS                  |

**SSH → Proxy → SSH 典型场景**：开发者 → VPN跳板机(入内网) → 内网HTTP代理(审计) → 生产跳板机(隔离) → DB。这是中大型企业的标配架构。

#### 4 跳 — 频次 ~0.2%

| #   | 链路                               | 场景                                            |
| --- | ---------------------------------- | ----------------------------------------------- |
| 19  | Proxy → SSH → Proxy → SSH → DB     | 双交替：多区域 + 多安全域。跨国金融机构合规架构 |
| 20  | SSH → Proxy → SSH → Proxy → DB     | 双交替反向                                      |
| 21  | Proxy₁ → Proxy₂ → SSH₁ → SSH₂ → DB | 双代理 + 双跳板                                 |

#### 5 跳 — 频次 < 0.01%

仅理论组合，实际网络中不存在需要跨越 5 个不同安全域的场景。

### 10.5 频次漏斗图

```
N=0  ████████████████████████████████████████  85%  直连/纯TLS
N=1  ██████                                    13%  单跳
N=2  █████                                     11%  代理+SSH(含交替)
N=3  ██                                         3%  双跳板+代理等(含交替)
N=4  ▏                                        0.2%  双交替
N=5  ·                                       <0.01%
────────────────────────────────────────────────────────
      3跳覆盖 99%    4跳覆盖 99.8%    5跳覆盖全部
```

### 10.6 嵌套上限设计

| 上限            | 覆盖                                                  | 决定                                                              |
| --------------- | ----------------------------------------------------- | ----------------------------------------------------------------- |
| **代码硬上限**  | 4 个 SSH/Proxy 网络跳 + 1 层 TLS + DB = **最多 6 层** | 覆盖 99.8%，超过时拒绝并提示                                      |
| **UI 黄色警告** | 3 跳（含）以上                                        | 显示延迟风险警告："当前协议链包含 N 个网络跳，建连延迟预期 ~XXms" |
| **V5 原型默认** | 1 跳 SSH + 1 层 TLS + 1 跳 Proxy = 2 跳 + TLS         | 每种协议至少保留 1 个实例，开关控制是否生效                       |

**不推荐无限套娃的原因**：每增加一个 SSH/Proxy 跳，建连时间增加 30-80ms，每个查询增加节点 RTT 延迟，故障点 +1，调试复杂度翻倍。

### 10.7 每跳延迟基准（参考值）

| 跳类型               | TCP 握手 | 协议握手 | 总延迟增量 |
| -------------------- | -------- | -------- | ---------- |
| 本地/内网 SSH        | ~2ms     | ~15ms    | ~17ms      |
| 跨区域 SSH           | ~30ms    | ~20ms    | ~50ms      |
| HTTP Proxy (CONNECT) | ~5ms     | ~5ms     | ~10ms      |
| SOCKS5 Proxy         | ~5ms     | ~8ms     | ~13ms      |
| TLS 握手             | N/A      | ~15ms    | ~15ms      |

```
最大延迟预估（4 跳 + TLS）：
内网场景：17ms×4 + 15ms ≈ 83ms  ← 可接受
跨区域场景：50ms×4 + 30ms ≈ 230ms ← 有感知但仍可用
```

### 10.8 协议链 JSON 数据模型（规划）

```json
{
  "chain": [
    { "type": "ssh", "profile_id": "ssh-prod-bastion" },
    { "type": "proxy", "profile_id": "proxy-corp-socks5" },
    { "type": "ssh", "profile_id": "ssh-internal-bastion" },
    { "type": "ssl", "profile_id": "ssl-rds-default" }
  ]
}
```

### 10.9 v0.6.0 后端实现要点

1. `ConnectionMethod` → `Vec<Hop>` 数据结构升级
2. `HopExecutor` 链式引擎：按序执行每个 hop，SSH/Proxy 产出地址改写或流包装，SSL 在最终流上包装
3. 硬上限：`const MAX_HOP_CHAIN: usize = 4`（SSH/Proxy 跳数）
4. 隧道句柄数组管理：`Vec<TunnelHandle>` 在连接生命周期中保持所有 SSH 隧道 alive
5. 每跳超时累计：前 N 跳超时之和 + 最终跳超时 = 总超时

---

## 十一、大厂网页跳板机集成可行性分析

### 11.1 什么是"网页跳板机"

大厂常见的跳板机/堡垒机不再是传统 SSH 22 端口的服务器，而是**基于 Web 的零信任接入网关**：

| 产品                             | 厂商      | 接入方式                                              |
| -------------------------------- | --------- | ----------------------------------------------------- |
| AWS Session Manager              | AWS       | `aws ssm start-session` → WebSocket → EC2             |
| Cloud IAP (Identity-Aware Proxy) | GCP       | `gcloud compute ssh --tunnel-through-iap` → HTTP 代理 |
| Azure Bastion                    | Azure     | HTML5 WebSocket，浏览器内终端                         |
| JumpServer                       | 开源      | Web Terminal + SSH 代理网关                           |
| Teleport                         | 开源/商业 | `tsh` CLI → WebSocket/HTTP2 → 目标节点                |
| 阿里云堡垒机                     | 阿里云    | Web 运维门户 + SSH/RDP 代理                           |

**共同特征**：

- 不暴露标准 SSH 22 端口
- 通过 WebSocket / HTTP2 隧道传输
- 使用 Token/Cookie/OAuth 而非 SSH Key 认证
- 通常有 CLI 工具可以创建**本地代理端口**

### 11.2 桌面客户端的集成方案

#### 方案 A：CLI 钩子（推荐，通用性最好）

```
用户在 RdataStation 中配置"前置命令"：

1. 常规 Tab 填连接信息（如 Aurora host:port）
2. 网络 Tab 选择"外部隧道"类型
3. 填写前置命令：
   $ aws ssm start-session \
       --target i-1234567890abcdef0 \
       --document-name AWS-StartPortForwardingSessionToRemoteHost \
       --parameters '{"host":["mydb.cluster-xxx.rds.amazonaws.com"],"portNumber":["3306"],"localPortNumber":["13306"]}'

4. 点击"连接" → RdataStation 执行前置命令（后台进程）
5. 等待 localhost:13306 就绪 → sqlx 连接 127.0.0.1:13306

同样适用于：
- gcloud compute ssh --tunnel-through-iap --ssh-flag="-L 15432:db:5432"
- tsh proxy db --tunnel --port=15432  (Teleport)
```

**优点**：零侵入，不依赖任何 SDK，只需要 `child_process.spawn` + 端口就绪检测。
**实现**：在 `ConnectionService` 增加 `pre_connect_commands: Vec<String>` 字段。

#### 方案 B：SDK 集成（深度集成，按厂商逐个实现）

| 厂商     | Rust SDK              | 可行性                                         |
| -------- | --------------------- | ---------------------------------------------- |
| AWS SSM  | `aws-sdk-ssm` (官方)  | ✅ 可集成，通过 `StartSession` API + WebSocket |
| GCP IAP  | 无官方 Rust SDK       | 🟡 需通过 REST/WebSocket 自实现                |
| Teleport | `tsh` CLI 或 gRPC API | 🟡 CLI 包装更实用                              |

**缺点**：维护成本高，每个厂商需要单独的 Connector 实现。

#### 方案 C：用户手动启动隧道（当前可行方案）

```
用户手动在终端执行：
$ aws ssm start-session ... --localPortNumber 13306

然后在 RdataStation 中连接 127.0.0.1:13306
（或者在常规 Tab 中填 localhost:13306，网络 Tab 设为"直连"）
```

这是当前就支持的，零开发成本。

### 11.3 推荐路线

| 阶段 | 方案                                         | 版本             |
| ---- | -------------------------------------------- | ---------------- |
| 当前 | 方案 C：用户手动启动隧道                     | ✅ v0.5.0 已可用 |
| 短期 | 方案 A：`pre_connect_commands` 前置命令钩子  | v0.7.0           |
| 中期 | 方案 A 增强：端口就绪检测 + 进程生命周期管理 | v0.8.0           |
| 长期 | 方案 B：AWS SSM SDK 深度集成                 | v1.0+            |

**结论**：网页跳板机完全可以集成。最务实的路径是先支持"前置 shell 命令"，利用各厂商已有的 CLI 工具做端口转发，RdataStation 只需管理进程生命周期 + 检测端口就绪。这个方案通用性强、零依赖、覆盖所有厂商。

---

## 十二、v0.6.0 完整实施方案（基于 add-datasource-v5 原型）

> 本章基于 `prototype/add-datasource-v5.html` 原型与前后端代码现状，给出分阶段实施方案。
> 原型地址：[add-datasource-v5.html](file:///e:/myapps/tauirapps/RdataStation/rdata-station/prototype/add-datasource-v5.html)

### 12.1 现状总结：已有 vs 缺失

#### 12.1.1 后端（Rust）— 已具备能力

| 能力                                                                   | 文件                                                                                                                                                                                                                                                         | 覆盖面               |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------- |
| `ConnectionMethod::Chain(Vec<ChainHop>)`                               | [config.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/config.rs)                                                                                                                                           | ✅ 完整              |
| `ChainHop` 枚举（Ssh/Ssl/HttpProxy/SocksProxy）                        | [config.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/config.rs)                                                                                                                                           | ✅ 完整              |
| `process_chain()` 链式处理（每跳端口转发）                             | [connection_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_service.rs)                                                                                                                            | ✅ 完整              |
| `TunnelGuard` 隧道生命周期管理                                         | [connector.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/connector.rs)                                                                                                                                     | ✅ 完整              |
| `env_store.rs` CRUD（Environment + EnvironmentPolicy）                 | [env_store.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/env_store.rs)                                                                                                                                           | ✅ 完整              |
| `network_store.rs` CRUD（NetworkConfig）                               | [network_store.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/network_store.rs)                                                                                                                                   | ✅ 完整              |
| `project_connection_store.rs` CRUD（environment_id/network_config_id） | [project_connection_store.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/project_connection_store.rs)                                                                                                             | ✅ 完整              |
| `global_connections` 表含 `environment_id`/`network_config_id`         | 迁移文件 [008_add_data_source_module.sql](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/migrations/global/008_add_data_source_module.sql)                                                                                                 | ✅ 已有字段          |
| `environments` / `environment_policies` / `network_configs` 表         | 迁移文件                                                                                                                                                                                                                                                     | ✅ 表已存在          |
| **SSH Agent 认证**                                                     | [connector.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/connector.rs)                                                                                                                                     | ✅ v0.6.0（仅 Unix） |
| **known_hosts Host Key 校验**                                          | [known_hosts.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/known_hosts.rs)                                                                                                                                 | ✅ v0.6.0            |
| **Proxy → SSL 嵌套**                                                   | [connector.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/connector.rs) + [connection_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_service.rs) | ✅ v0.6.0            |

#### 12.1.2 后端（Rust）— 待增加

| 缺项                       | 说明                                                                                       | 优先级 |
| -------------------------- | ------------------------------------------------------------------------------------------ | ------ |
| **Chain 校验**             | `ConnectionMethod::Chain` 缺少硬上限校验（4 SSH/Proxy 跳）、SSL 必须末尾的约束             | 🔴 P0  |
| **预置环境 Seed**          | `environments` 表为空，需要预置 5 个内置环境（开发/测试/预发布/生产/沙箱）及对应 25 条策略 | 🔴 P0  |
| **IPC Commands**           | 缺少前端直接调用的 `list_environments` / `list_network_configs` 等 Tauri Command           | 🔴 P0  |
| **network_configs scope**  | 当前表无 `scope` 字段区分全局/项目，需要补充                                               | 🟡 P1  |
| **chain 拓扑预览后端接口** | 前端展示拓扑不需要，但 `test_network_config` 当前只测单配置，需支持 chain 测试             | 🟡 P1  |

#### 12.1.3 前端（Vue/TS）— 现状 vs 原型

| 组件                | 现状                                              | v5 原型目标                                                                       | 差距        |
| ------------------- | ------------------------------------------------- | --------------------------------------------------------------------------------- | ----------- |
| **NetworkTab.vue**  | 3 个独立折叠面板（SSH/SSL/Proxy）                 | 动态协议链：拖拽排序、开关控制、添加/删除跳、拓扑预览                             | 🔴 需要重写 |
| **AdvancedTab.vue** | 连接参数 + DuckDB 加速 + Schema + 编码            | 环境紧凑下拉 + 策略行内标签 + DuckDB 焕新加速 + 安全策略可折叠 + 连接参数         | 🔴 需要大改 |
| **类型定义**        | `ConnectionMethodConfig` 单配置模式，无 Chain/Hop | `ChainHop`, `ProtocolChain`, `NetworkProfile`, `Environment`, `EnvironmentPolicy` | 🔴 需要新增 |
| **Stores**          | `projectConnectStore` / `runtimeConnectionStore`  | 新增 `environmentStore` + `networkConfigStore`（增强）                            | 🟡 需要新增 |
| **管理覆盖层**      | 无                                                | 网络配置文件管理器 + 环境类型管理器                                               | 🟡 需要新增 |

### 12.2 数据模型变更

#### 12.2.1 SQLite 表变更

```sql
-- network_configs 表增加 scope 字段
ALTER TABLE network_configs ADD COLUMN scope TEXT DEFAULT 'global';
-- scope 值: 'global' | 'project'

-- 预置 5 个环境
INSERT OR IGNORE INTO environments (id, name, description, color, sort_order) VALUES
  ('env-dev',      '开发环境', '本地开发、调试数据库',      '#a6e3a1', 1),
  ('env-test',     '测试环境', '集成测试、QA 验证',          '#f9e2af', 2),
  ('env-staging',  '预发布',   '灰度验证、预发布环境',      '#89b4fa', 3),
  ('env-prod',     '生产环境', '线上生产数据库，谨慎操作',  '#f38ba8', 4),
  ('env-sandbox',  '沙箱环境', '安全隔离的沙箱数据库',      '#cba6f7', 5);

-- 预置 5×5=25 条策略（每个环境 5 类策略：security/schema/performance/audit/ui）
INSERT OR IGNORE INTO environment_policies (id, environment_id, policy_type, policy_config, enabled) VALUES
  -- env-dev
  ('ep-dev-sec',   'env-dev', 'security',    '{"readonly":false,"writeConfirm":false,"ddlConfirm":false,"dropConfirm":"false","autocommit":true,"rowLimit":0,"sizeLimit":0}', 1),
  ('ep-dev-sch',   'env-dev', 'schema',      '{"autoLoad":true,"loadDepth":2,"showSystem":true,"refreshInterval":0}', 1),
  ('ep-dev-perf',  'env-dev', 'performance', '{"poolSize":10,"queryTimeout":0,"connectTimeout":30,"heartbeat":60,"maxReconnect":3}', 1),
  ('ep-dev-audit', 'env-dev', 'audit',       '{"sqlLog":false,"operationRecord":false,"sensitiveTableAlert":false}', 1),
  ('ep-dev-ui',    'env-dev', 'ui',          '{"topBarColor":"#a6e3a1","tabIndicator":true,"sqlWarningBanner":false,"writeBtnStyle":"normal"}', 1),
  -- env-test
  ('ep-test-sec',   'env-test', 'security',    '{"readonly":false,"writeConfirm":false,"ddlConfirm":true,"dropConfirm":"true","autocommit":true,"rowLimit":10000,"sizeLimit":100}', 1),
  ('ep-test-sch',   'env-test', 'schema',      '{"autoLoad":true,"loadDepth":2,"showSystem":true,"refreshInterval":60}', 1),
  ('ep-test-perf',  'env-test', 'performance', '{"poolSize":10,"queryTimeout":120,"connectTimeout":30,"heartbeat":60,"maxReconnect":3}', 1),
  ('ep-test-audit', 'env-test', 'audit',       '{"sqlLog":false,"operationRecord":false,"sensitiveTableAlert":false}', 1),
  ('ep-test-ui',    'env-test', 'ui',          '{"topBarColor":"#f9e2af","tabIndicator":true,"sqlWarningBanner":false,"writeBtnStyle":"normal"}', 1),
  -- env-staging
  ('ep-stg-sec',   'env-staging', 'security',    '{"readonly":false,"writeConfirm":true,"ddlConfirm":true,"dropConfirm":"true","autocommit":false,"rowLimit":5000,"sizeLimit":50}', 1),
  ('ep-stg-sch',   'env-staging', 'schema',      '{"autoLoad":true,"loadDepth":2,"showSystem":false,"refreshInterval":120}', 1),
  ('ep-stg-perf',  'env-staging', 'performance', '{"poolSize":15,"queryTimeout":180,"connectTimeout":30,"heartbeat":60,"maxReconnect":5}', 1),
  ('ep-stg-audit', 'env-staging', 'audit',       '{"sqlLog":true,"operationRecord":true,"sensitiveTableAlert":true}', 1),
  ('ep-stg-ui',    'env-staging', 'ui',          '{"topBarColor":"#89b4fa","tabIndicator":true,"sqlWarningBanner":true,"writeBtnStyle":"confirm"}', 1),
  -- env-prod
  ('ep-prod-sec',   'env-prod', 'security',    '{"readonly":true,"writeConfirm":true,"ddlConfirm":true,"dropConfirm":"disable","autocommit":false,"rowLimit":1000,"sizeLimit":20}', 1),
  ('ep-prod-sch',   'env-prod', 'schema',      '{"autoLoad":false,"loadDepth":1,"showSystem":false,"refreshInterval":300}', 1),
  ('ep-prod-perf',  'env-prod', 'performance', '{"poolSize":20,"queryTimeout":60,"connectTimeout":15,"heartbeat":30,"maxReconnect":3}', 1),
  ('ep-prod-audit', 'env-prod', 'audit',       '{"sqlLog":true,"operationRecord":true,"sensitiveTableAlert":true}', 1),
  ('ep-prod-ui',    'env-prod', 'ui',          '{"topBarColor":"#f38ba8","tabIndicator":true,"sqlWarningBanner":true,"writeBtnStyle":"danger"}', 1),
  -- env-sandbox
  ('ep-sbx-sec',   'env-sandbox', 'security',    '{"readonly":false,"writeConfirm":false,"ddlConfirm":false,"dropConfirm":"false","autocommit":true,"rowLimit":1000,"sizeLimit":50}', 1),
  ('ep-sbx-sch',   'env-sandbox', 'schema',      '{"autoLoad":true,"loadDepth":1,"showSystem":false,"refreshInterval":0}', 1),
  ('ep-sbx-perf',  'env-sandbox', 'performance', '{"poolSize":5,"queryTimeout":60,"connectTimeout":30,"heartbeat":60,"maxReconnect":2}', 1),
  ('ep-sbx-audit', 'env-sandbox', 'audit',       '{"sqlLog":false,"operationRecord":false,"sensitiveTableAlert":false}', 1),
  ('ep-sbx-ui',    'env-sandbox', 'ui',          '{"topBarColor":"#cba6f7","tabIndicator":true,"sqlWarningBanner":false,"writeBtnStyle":"normal"}', 1);
```

#### 12.2.2 TypeScript 类型定义扩展

```typescript
// 新增到 domain/types.ts

/** 协议链路中的单跳 */
export interface ChainHop {
  id: string // 前端 UI 唯一标识（非持久化）
  protocol: 'ssh' | 'ssl' | 'http_proxy' | 'socks_proxy'
  enabled: boolean
  mode: 'select' | 'new' | 'custom'
  profileId?: string // 预配置 profile id (select 模式)
  customData?: Record<string, unknown> // 自定义配置 (custom 模式)
}

/** 协议链（持久化到 connection_method 字段） */
export interface ProtocolChain {
  hops: ChainHopConfig[] // 有序 hop 列表
}

/** 持久化的单跳配置（不含 UI 状态） */
export interface ChainHopConfig {
  protocol: 'ssh' | 'ssl' | 'http_proxy' | 'socks_proxy'
  enabled: boolean
  profileId?: string
}

/** 网络配置文件 */
export interface NetworkProfile {
  id: string
  name: string
  network_type: 'ssh' | 'ssl' | 'http_proxy' | 'socks_proxy'
  scope: 'global' | 'project'
  config: SshConfig | SslConfig | ProxyConfig
  created_at: string
  updated_at: string
}

/** 环境配置 */
export interface Environment {
  id: string
  name: string
  description: string
  color: string
  icon: string
  sort_order: number
  deletable: boolean
  policies: EnvironmentPolicies
}

/** 环境策略集合 */
export interface EnvironmentPolicies {
  security: SecurityPolicy
  schema: SchemaPolicy
  performance: PerformancePolicy
  audit: AuditPolicy
  ui: UiPolicy
}

export interface SecurityPolicy {
  readonly: boolean
  writeConfirm: boolean
  ddlConfirm: boolean
  dropConfirm: 'false' | 'true' | 'disable'
  autocommit: boolean
  rowLimit: number
  sizeLimit: number
}

export interface SchemaPolicy {
  autoLoad: boolean
  loadDepth: number
  showSystem: boolean
  refreshInterval: number
}

export interface PerformancePolicy {
  poolSize: number
  queryTimeout: number
  connectTimeout: number
  heartbeat: number
  maxReconnect: number
}

export interface AuditPolicy {
  sqlLog: boolean
  operationRecord: boolean
  sensitiveTableAlert: boolean
}

export interface UiPolicy {
  topBarColor: string
  tabIndicator: boolean
  sqlWarningBanner: boolean
  writeBtnStyle: 'normal' | 'confirm' | 'danger'
}

// ConnectionMethodType 扩展
export type ConnectionMethodType = 'direct' | 'ssl' | 'ssh' | 'http_proxy' | 'socks_proxy' | 'chain'
```

### 12.3 前端改造清单

#### 阶段一：核心组件（🔴 P0，约 5 天）

| #   | 任务                          | 文件                                         | 说明                                                                                                                                    |
| --- | ----------------------------- | -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | **重写 NetworkTab.vue**       | `tabs/NetworkTab.vue`                        | 动态协议链：拖拽排序 + 开关控制 + 添加/删除跳 + inline 配置选择 + 硬盘上限(4 SSH/Proxy) + SSL 末尾约束 + 拓扑预览 + 协议管理器入口      |
| 2   | **改造 AdvancedTab.vue**      | `tabs/AdvancedTab.vue`                       | 新增紧凑环境下拉（EnvironmentSelector）+ 策略摘要行 + DuckDB 加速卡焕新 + 安全策略可折叠（SecurityPolicySection）+ 连接参数联动环境策略 |
| 3   | **新增 TS 类型**              | `domain/types.ts` + `ui/types/connection.ts` | 添加 ChainHop / ProtocolChain / NetworkProfile / Environment / EnvironmentPolicies / SecurityPolicy 等类型                              |
| 4   | **扩展 ConnectionMethodType** | `domain/types.ts` + `ui/types/connection.ts` | 增加 `'chain'` 值                                                                                                                       |

#### 阶段二：管理面板（🟡 P1，约 3 天）

| #   | 任务                   | 文件                                           | 说明                                                                                                     |
| --- | ---------------------- | ---------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| 5   | **环境类型管理器**     | `components/network/EnvironmentManager.vue`    | 覆盖层弹窗：环境列表（内置/自定义）+ 新建自定义环境（继承策略模板）+ 策略摘要展示 + 删除确认             |
| 6   | **网络配置文件管理器** | `components/network/NetworkProfileManager.vue` | 覆盖层弹窗：SSH/SSL/Proxy 三 Tab + 配置列表 CRUD + 新建表单 + 范围标识(全局/项目) + 一键应用到协议链节点 |
| 7   | **环境选择组件**       | `components/network/EnvironmentSelector.vue`   | 紧凑下拉：颜色点 + 图标 + 策略标签 + 选中态 + 点击外部关闭                                               |
| 8   | **安全策略组件**       | `components/network/SecurityPolicySection.vue` | 可折叠面板：只读切换/写确认/DDL确认/DROP下拉/自动提交/行限制/大小限制 + 环境覆盖指示器                   |

#### 阶段三：Stores & Services（🟡 P1，约 2 天）

| #   | 任务                           | 文件                           | 说明                                                                                            |
| --- | ------------------------------ | ------------------------------ | ----------------------------------------------------------------------------------------------- |
| 9   | **新增 environmentStore.ts**   | `stores/environmentStore.ts`   | Pinia Store：fetchEnvironments / createEnvironment / deleteEnvironment / getEnvironmentPolicies |
| 10  | **增强 networkConfigStore.ts** | `stores/networkConfigStore.ts` | 增加 protocol chain 相关 actions / listByScope / chain CRUD                                     |

#### 阶段四：集成 & 联调（🟡 P1，约 2 天）

| #   | 任务                             | 文件                                 | 说明                                                      |
| --- | -------------------------------- | ------------------------------------ | --------------------------------------------------------- |
| 11  | **改造 AddDataSourceDialog.vue** | `components/AddDataSourceDialog.vue` | 传递 chain/env 数据到保存/连接接口                        |
| 12  | **改造 connection.ts service**   | `services/connection.ts`             | connectDatabase 增加 chain/environment_id 参数            |
| 13  | **端到端联调**                   | 全链路                               | 创建连接 → 选环境 → 配协议链 → 保存 → 测试连接 → 连接成功 |

### 12.4 后端增量清单（Rust）

#### 阶段一：Chain 校验 + Seed（🔴 P0，约 2 天）

| #   | 任务                      | 文件                                               | 说明                                                                                                                       |
| --- | ------------------------- | -------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| B1  | **Chain 硬校验**          | `config.rs` 或 `connection_service.rs`             | 新增 `validate_chain(hops: &[ChainHop]) -> Result<(), CoreError>`：检查 SSH/Proxy 跳数 ≤ 4，SSL 必须在末尾，不允许空 chain |
| B2  | **预置环境 Seed**         | 新增 `migrations/global/009_seed_environments.sql` | INSERT 5 个环境 + 25 条策略                                                                                                |
| B3  | **network_configs scope** | `migrations/global/009_seed_environments.sql`      | `ALTER TABLE network_configs ADD COLUMN scope`                                                                             |

#### 阶段二：IPC Commands（🔴 P0，约 1 天）

| #   | 任务                             | 文件                                                                 | 说明                                                           |
| --- | -------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------- |
| B4  | **list_environments**            | `commands/connection_commands.rs` 或 新建 `commands/env_commands.rs` | 返回全部环境列表（含策略），Tauri Command                      |
| B5  | **save_environment**             | 同上                                                                 | 保存自定义环境                                                 |
| B6  | **delete_environment**           | 同上                                                                 | 删除自定义环境（内置环境不可删）                               |
| B7  | **list_network_configs_by_type** | `commands/connection_commands.rs`                                    | 按 network_type 过滤 + scope 过滤                              |
| B8  | **test_network_chain**           | `commands/connection_commands.rs`                                    | 支持 chain 的连通性测试（现有 test_network_config 只测单配置） |

### 12.5 NetworkTab.vue 重写详细设计

#### 12.5.1 数据模型

```typescript
// 协议链内部状态（驱动 UI 渲染）
interface ChainItem {
  id: string // 前端唯一 ID (如 'hop-1')
  protocol: 'ssh' | 'ssl' | 'http_proxy' | 'socks_proxy'
  enabled: boolean // 开关状态
  mode: 'select' | 'new' // select=选已保存配置, new=内联新建
  profileId?: string // 选中的配置 ID
}

// 持久化时：
// ConnectionMethod::Chain(hops) 由 ChainItem[] 提取 enabled=true 的 hop
// 按 ChainItem 在协议链中的顺序构建 Vec<ChainHop>
```

#### 12.5.2 约束规则

| 规则                      | 实现                                                 |
| ------------------------- | ---------------------------------------------------- |
| **SSL 必须在末尾**        | 渲染时固定 SSL 在链末尾，拖拽时限制 SSL 只能放在最后 |
| **SSL 最多 1 个**         | 添加 SSL 时替换已存在的 SSL（而非追加第二个）        |
| **SSH/Proxy 最多 4 个**   | `countNetworkHops()` < 4，达到上限时添加按钮置灰     |
| **每种协议至少 1 个实例** | 当某协议只有 1 个实例时，删除按钮禁用（灰色）        |
| **拖拽约束**              | SSL 不可拖到非末尾位置，非 SSL 不可拖到 SSL 之后     |
| **3 跳警告**              | 启用数 ≥ 3 时显示黄色延迟警告横幅                    |

#### 12.5.3 渲染结构

```
┌─ 网络 Tab ───────────────────────────────────────────────┐
│ ℹ 动态协议链 — 支持 SSH/Proxy 任意交替（最多 4 跳）     │
│                                                          │
│ [列头]  # | 拖拽 | 协议类型 | 配置选择        | 启用 | 操作 │
│ ┌────────────────────────────────────────────────────────┐│
│ │ ≡ 1 | [🔒 SSH 隧道] | [选择▼]+ 新建 | [🔘on] | 📋 ✕ ││
│ │ ≡ 2 | [🌐 代理]     | [选择▼]+ 新建 | [🔘on] | 📋 ✕ ││
│ │ 🔒 🔐| [🛡 SSL末尾]  | [选择▼]+ 新建 | [○off] | 📋 ✕ ││
│ └────────────────────────────────────────────────────────┘│
│ [⚠ 3跳警告: 延迟 ~75ms]                                  │
│ [+ 添加协议节点]                                          │
│                                                          │
│ 📡 数据路径预览                                          │
│ [🏠本机] →SSH→ [生产跳板机] →Proxy→ [内网代理] →TLS→ [DB] │
└──────────────────────────────────────────────────────────┘
```

#### 12.5.4 关键交互

| 交互              | 行为                                                                             |
| ----------------- | -------------------------------------------------------------------------------- |
| **拖拽排序**      | HTML5 Drag & Drop，排序后自动 `ensureSslAtEnd()`                                 |
| **开关切换**      | 切换 `enabled`，实时更新拓扑预览                                                 |
| **配置下拉选择**  | 从对应协议类型的 profiles 中选择                                                 |
| **"+ 新建"按钮**  | 切换到 `mode: 'new'`，展开内联表单（含范围标识）                                 |
| **新建保存**      | 根据页面 scope 选择决定保存到 `global` 或 `project` 的 network_configs，自动应用 |
| **"📋 管理"按钮** | 打开 NetworkProfileManager 覆盖层                                                |
| **拓扑预览**      | 实时反映 `enabled` hops 的数据路径（本机→跳1→跳2→...→DB）                        |

### 12.6 AdvancedTab.vue 改造详细设计

#### 12.6.1 新增区域布局

```
┌─ 高级 Tab ──────────────────────────────────────────────┐
│                                                          │
│ 🏷 环境  [● 开发环境 ▾]  [管理]                          │
│  🔒读写 · ✍写确认 · DDL确认 · 行限5000 · 限20M           │
│                                                          │
│ ⚡ 本地加速引擎 (DuckDB)                    ──高权重──     │
│ ┌──────────────────────────────────────────────────────┐ │
│ │ 🦆 启用 DuckDB 查询加速                    [开关]    │ │
│ │ ── 展开 ──                                           │ │
│ │ 🚀大表关联  🔗跨库联邦  📊重复报表  💾列式存储        │ │
│ │ 同步策略[▼] 间隔[15]min  内存[512]MB  线程[4]         │ │
│ │ 📁 .rdata/duckdb/accel.duckdb                        │ │
│ └──────────────────────────────────────────────────────┘ │
│                                                          │
│ 🔐 安全策略 [▶] ← dev预置 · 读写·无限制                  │
│ ┌─ 展开 ──────────────────────────────────────────────┐ │
│ │ [开关]只读 [开关]写确认 [开关]DDL确认                │ │
│ │ DROP [▼确认/禁用] [开关]自动提交                     │ │
│ │ 行上限[0]  大小上限[0]M                              │ │
│ └──────────────────────────────────────────────────────┘ │
│                                                          │
│ 连接参数                                                │
│ 超时[30]s  查询超时[0]  保活[60]s  重连[3]              │
│                                                          │
│ Schema加载 [▼自动]    编码 [▼UTF-8]                     │
└──────────────────────────────────────────────────────────┘
```

#### 12.6.2 环境联动策略逻辑

```
选择环境 → applyEnvPolicies() →
  ├── security 字段 → 安全策略开关/下拉 (polReadonly/WriteConfirm/DdlConfirm/DropConfirm/Autocommit/RowLimit/SizeLimit)
  ├── performance 字段 → 连接参数 (connectTimeout/queryTimeout/heartbeat/maxReconnect)
  ├── schema 字段 → Schema 加载策略下拉
  └── 策略摘要行 + 覆盖指示器更新

用户手动修改任一策略字段 →
  ├── 触发 onPolicyOverride()
  ├── 安全策略摘要更新
  └── 指示器从 "← 环境名 预设" 变为 "⚠ 已覆盖 环境名 预设" (黄色)
```

### 12.7 实施路线图

```
v0.6.0-alpha (✅ 已完成) — 后端网络层核心
  ├── ✅ SSH Agent 认证（connector.rs，仅 Unix）
  ├── ✅ known_hosts Host Key 校验（新增 known_hosts.rs 模块）
  ├── ✅ Proxy → SSL 嵌套（connector::wrap_tls_stream + process_chain 邻接检测）
  ├── ✅ 编译 Send trait 修复（平台条件编译隔离非 Send 类型）
  ├── ✅ ID 前缀规范化（G_/P_/GP_ 三级前缀 + id_prefix.rs）
  ├── ✅ 快照机制（snapshot_service.rs + origin/source_id/snapshot_at 字段）
  └── ✅ 全局/项目迁移 SQL（009 + 011）

v0.6.0-beta (待开发) — 后端增量
  ├── B1: Chain 硬校验 (validate_chain)
  ├── B2-B3: 预置环境 Seed SQL + network_configs scope
  └── B4-B8: IPC Commands (list_environments / list_network_configs_by_type / save/delete_env)

v0.6.0-gamma (待开发) — 前端核心
  ├── #1: 重写 NetworkTab.vue (协议链 + 拖拽 + 拓扑)
  ├── #2: 改造 AdvancedTab.vue (环境选择 + 策略 + DuckDB焕新)
  ├── #3-4: TS 类型定义扩展
  └── #9-10: environmentStore + networkConfigStore

v0.6.0-rc (待开发) — 管理面板 + 集成
  ├── #5-8: EnvironmentManager / NetworkProfileManager / EnvironmentSelector / SecurityPolicySection
  ├── #11: AddDataSourceDialog 传参改造
  ├── #12: connection.ts service 改造
  └── #13: 端到端联调

v0.6.0 发布
  └── 测试 + bug fix + 文档更新
```

### 12.8 关键风险与缓解

| 风险                                                | 影响                | 缓解                                                                            |
| --------------------------------------------------- | ------------------- | ------------------------------------------------------------------------------- |
| NetworkTab 拖拽排序在 naive-ui 下实现复杂           | 开发周期延长        | 使用原生 HTML5 Drag & Drop API，参考 v5 原型实现                                |
| 协议链数据与后端 ConnectionMethod::Chain 序列化兼容 | 保存/加载失败       | 前后端约定 chain JSON 格式：`{"chain":[{"type":"ssh","profile_id":"xxx"},...]}` |
| 环境策略与后端 environment_policies 表字段对齐      | 策略不生效          | 前端 environmentStore 解析 `policy_config` JSON 字段，后端返回完整结构          |
| DuckDB 加速功能部分暂未实现                         | UI 显示但功能不可用 | 保留 DuckDB 加速开关和配置 UI，后端实现标记 TODO v0.7.0                         |

---

## 十三、数据库 ID 前缀规范与快照机制（v0.6.0 新增）

### 13.1 设计背景

项目存在大量全局（Application）与项目（Project）级别表结构完全一致的数据表：

| 表名                   | global.db | project.db |
| ---------------------- | :-------: | :--------: |
| `environments`         |    ✅     |     ✅     |
| `environment_policies` |    ✅     |     ✅     |
| `network_configs`      |    ✅     |     ✅     |
| `auth_configs`         |    ✅     |     ✅     |

此前两张表使用无前缀的 ID（`env_dev`、`net_abc123`），无法一眼区分来源，且项目迁移到另一台机器时，若引用的是全局表数据则引用断裂。

### 13.2 ID 前缀约定

```
G_xxx  = 全局（Application）表主键，存储于 global.db
P_xxx  = 项目（Project）表主键（本地创建），存储于 project.db
GP_xxx = 从全局快照到项目的数据，存储于 project.db
```

**解析优先级**（以 `network_config_id` 为例）：

| 前缀   | 查找顺序                                                       |
| ------ | -------------------------------------------------------------- |
| `G_`   | global.db.network_configs                                      |
| `P_`   | project.db.network_configs                                     |
| `GP_`  | project.db.network_configs（origin='global_snapshot'，自包含） |
| 无前缀 | 向后兼容：先查 global，再查 project（历史数据）                |

### 13.3 快照溯源字段

project.db 的三张表（environments、network_configs、auth_configs）新增三个字段：

```sql
ALTER TABLE network_configs ADD COLUMN origin       TEXT DEFAULT 'project';
ALTER TABLE network_configs ADD COLUMN source_id    TEXT;
ALTER TABLE network_configs ADD COLUMN snapshot_at  TIMESTAMP;
```

| 字段          | 类型      | 说明                                                     |
| ------------- | --------- | -------------------------------------------------------- |
| `origin`      | TEXT      | `'project'` = 本地创建；`'global_snapshot'` = 从全局快照 |
| `source_id`   | TEXT      | 快照来源的全局 G_xxx ID，用于同步溯源                    |
| `snapshot_at` | TIMESTAMP | 快照创建时间，用于判断是否过期                           |

### 13.4 快照服务（snapshot_service）

提供三个核心能力：

**创建快照**：将全局配置完整复制到项目本地

```
snapshot_environment("G_env_prod") → project.db 写入 "GP_env_prod"（含策略）
snapshot_network_config("G_net_proxy") → 写入 "GP_net_proxy"
snapshot_auth_config("G_auth_root") → 写入 "GP_auth_root"
```

**检测过期**：比较 project 的 `snapshot_at` 与 global 的 `updated_at`

```
check_sync_status("network_configs", "GP_net_proxy")
→ SnapshotStatus { outdated: true, global_updated_at: "2026-05-19T15:00:00Z" }
```

**执行同步**：用全局最新数据更新项目快照

```
sync_snapshot("network_configs", "GP_net_proxy")
→ INSERT OR REPLACE in project.db with latest global data
```

### 13.5 迁移安全性

```
项目 A                                项目 B（目标机器）
├── .RSmeta/project.db               ├── .RSmeta/project.db
│   ├── GP_env_prod  ← 自包含 ✅     │   ├── GP_env_prod  ← 自包含 ✅
│   ├── GP_net_proxy  ← 迁移不断裂   │   ├── GP_net_proxy  ← 无需 global.db
│   └── P_net_local   ← 本地配置     │   └── P_net_local   ← 本地配置
└── ...

目标机器不需要 global.db 中有对应的 G_ 配置，
GP_ 快照随项目走，拷走即用。
```

### 13.6 相关文件

| 文件                                                                                                                             | 说明                                        |
| -------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| [id_prefix.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/id_prefix.rs)               | ID 前缀常量 + 生成函数 + 前缀检测           |
| [snapshot_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/snapshot_service.rs)    | 快照创建 / 同步检测 / 同步执行              |
| [data_source_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/data_source_commands.rs) | 全局命令 ID 改为 G\_ 前缀                   |
| [project_db.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/project_db.rs)             | 项目命令 ID 改为 P\_ 前缀                   |
| [connection_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/connection_commands.rs)   | parse*network_method 支持 GP* 前缀解析      |
| migrations/global/009_add_id_prefix_convention.sql                                                                               | 全局迁移：Seed 数据 G\_ 前缀                |
| migrations/project_meta/011_add_id_prefix_snapshot.sql                                                                           | 项目迁移：新增 origin/source_id/snapshot_at |
