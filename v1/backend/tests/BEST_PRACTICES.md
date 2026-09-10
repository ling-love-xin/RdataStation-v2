# RdataStation 数据库连接配置最佳实践

> 版本：v2.1
> 日期：2026-06-20
> 基于 v0.5.3 安全审计与测试结果

---

## 一、总则

### 1.1 连接配置架构

```
用户输入 → DriverConnectionConfig → to_url() → Database::new(url)
                                          ↓
                              driver_properties 追加到查询参数
```

**URL 生成优先级**：

```
url_override > url_template > 硬编码匹配（legacy fallback）
```

所有路径最终都会调用 `append_query_params()` 追加 `driver_properties`、`options`、`encoding`。

### 1.2 安全性原则（v2.0 强化）

| 原则         | 说明                                                       | 验证方式                                             |
| ------------ | ---------------------------------------------------------- | ---------------------------------------------------- |
| 密码加密     | 所有密码/Auth 凭据使用 AES-256-GCM 加密存储（`AES:` 前缀） | `auth_store_tests.rs`（15 用例）                     |
| 日志脱敏     | 日志中使用 `mask_password_in_url()` 脱敏 URL               | `connection_commands_tests.rs`（7 用例）             |
| 参数化查询   | 始终使用 `query_with_params()` 防止 SQL 注入               | `mysql_integration_tests.rs` / `postgresql_tests.rs` |
| 最小权限     | 数据库用户仅授予必要权限                                   | 运维规范                                             |
| 错误消息安全 | 前端返回的错误消息不包含明文密码                           | `error_propagation_snapshot_tests.rs`                |
| 网络认证     | 网络认证配置支持全局 + 项目级双层查询                      | `data_source_commands_integration_tests.rs`          |
| 驱动属性     | `url_override` 路径也正确追加 `driver_properties`          | `connection_commands_tests.rs`（22 用例）            |

### 1.3 安全审计已知问题（v2.1 更新）

| 编号    | 问题                                                          | 风险    | 状态        |
| ------- | ------------------------------------------------------------- | ------- | ----------- |
| SEC-001 | `test_connection` 超时日志含明文密码 URL                      | 🔴 高危 | ✅ 已修复   |
| SEC-002 | `test_connection` 超时错误消息返回明文密码给前端              | 🔴 高危 | ✅ 已修复   |
| SEC-003 | `ConnectionInfoResponse::from_info` 返回含明文密码 URL        | 🔴 高危 | ✅ 已修复   |
| SEC-004 | 网络认证配置仅查全局 DB，忽略项目级                           | 🟡 中危 | ✅ 已修复   |
| SEC-005 | `url_override` 路径跳过 `driver_properties`                   | 🟡 中危 | ✅ 已修复   |
| SEC-006 | `build_postgres_url` 不编码特殊字符密码                       | 🟡 中危 | ⚠️ 已知限制 |
| SEC-007 | `scale_down` 在空池下 u32 下溢                                | 🟢 低危 | ✅ 已修复   |
| SEC-008 | `SslConfig::default()` 的 `verify_server_cert` 默认为 `false` | 🟢 低危 | ⚠️ 已知限制 |
| SEC-009 | `test_connection` 创建临时连接的日志含明文密码 URL            | 🔴 高危 | ✅ 已修复   |
| SEC-010 | `test_connection_config` 日志含明文密码 URL                   | 🔴 高危 | ✅ 已修复   |
| SEC-011 | `get_recent_connections` 返回含明文密码 URL                   | 🔴 高危 | ✅ 已修复   |

---

## 二、SQLite 配置

### 2.1 连接 URL 格式

```
# 文件路径（绝对路径）
sqlite:///absolute/path/to/database.db

# 文件路径（相对路径）
sqlite://relative/path/to/database.db

# 内存数据库
:memory:
```

### 2.2 推荐 driver_properties

```json
{
  "journal_mode": "WAL",
  "synchronous": "NORMAL",
  "cache_size": "-2000",
  "foreign_keys": "ON",
  "busy_timeout": "5000"
}
```

### 2.3 最佳实践

| 场景     | 建议                                | 说明                                 |
| -------- | ----------------------------------- | ------------------------------------ |
| 并发读   | 使用 WAL 模式                       | 支持多读单写，提升并发读取性能       |
| 性能优化 | `synchronous=NORMAL`                | 平衡性能与数据安全，每秒 fsync 一次  |
| 大事务   | 设置 `cache_size` 为负值            | 负值表示 KB 单位，`-2000` = 2MB 缓存 |
| 外键约束 | 显式启用 `PRAGMA foreign_keys = ON` | SQLite 默认不开启外键约束            |
| 锁超时   | 设置 `busy_timeout`                 | 避免并发写入时立即报错，等待 5 秒    |
| 磁盘 I/O | 使用 `temp_store=MEMORY`            | 临时表存储在内存中，减少磁盘 I/O     |

### 2.4 安全配置

```json
{
  "journal_mode": "WAL",
  "synchronous": "FULL",
  "foreign_keys": "ON",
  "secure_delete": "ON",
  "trusted_schema": "OFF"
}
```

| 参数                 | 安全说明                           |
| -------------------- | ---------------------------------- |
| `synchronous=FULL`   | 生产环境推荐，每次写入后立即 fsync |
| `secure_delete=ON`   | 覆盖删除数据，防止恢复             |
| `trusted_schema=OFF` | 禁止不受信任的 SQL 函数和虚拟表    |

### 2.5 已知限制

- 仅支持单写者，高并发写入需排队
- 默认不启用外键约束，需显式 PRAGMA
- 文件路径不存在时创建失败，需先确保目录存在
- `DataSourceMeta::sqlite()` 固定返回 `is_in_memory: false`，不检测 `:memory:`

---

## 三、DuckDB 配置

### 3.1 连接 URL 格式

```
# 文件路径
duckdb:///absolute/path/to/database.duckdb

# 内存数据库
:memory:
```

### 3.2 推荐 driver_properties

```json
{
  "threads": "4",
  "memory_limit": "1GB",
  "access_mode": "read_write"
}
```

### 3.3 最佳实践

| 场景     | 建议                               | 说明                      |
| -------- | ---------------------------------- | ------------------------- |
| 分析查询 | 设置 `threads` 为 CPU 核心数       | 充分利用多核并行计算      |
| 内存控制 | 设置 `memory_limit` 避免 OOM       | 限制 DuckDB 最大内存使用  |
| 只读模式 | 设置 `access_mode=read_only`       | 保护数据不被意外修改      |
| 异步操作 | 使用 `spawn_blocking` 隔离阻塞操作 | 防止阻塞 tokio 运行时线程 |
| 大结果集 | 使用 `SET threads TO N` 动态调整   | 根据查询复杂度动态调整    |
| 临时目录 | 设置 `temp_directory` 到 SSD       | 加速大查询的溢出到磁盘    |

### 3.4 安全配置

```json
{
  "access_mode": "read_only",
  "memory_limit": "2GB",
  "enable_external_access": "false"
}
```

| 参数                           | 安全说明                     |
| ------------------------------ | ---------------------------- |
| `access_mode=read_only`        | 只读模式，防止数据被篡改     |
| `enable_external_access=false` | 禁止 DuckDB 访问外部文件系统 |

### 3.5 已知限制

- 错误查询可能阻塞 tokio 运行时线程，**必须**使用 `spawn_blocking` 隔离
- 单文件写入为串行，高并发写入场景有限制
- 不支持远程连接，仅本地文件访问
- DuckDB 驱动不支持通过 URL 查询参数传递 `access_mode`，需通过初始化 SQL 设置

---

## 四、MySQL 配置

### 4.1 连接 URL 格式

```
# 基本格式
mysql://username:password@host:port/database

# 带参数
mysql://root:root@localhost:3306/testdb?allowPublicKeyRetrieval=TRUE&useSSL=false

# 不带数据库名（连接后手动选择）
mysql://root:root@localhost:3306/
```

### 4.2 推荐 driver_properties

```json
{
  "allowPublicKeyRetrieval": "TRUE",
  "useSSL": "false",
  "serverTimezone": "Asia/Shanghai",
  "characterEncoding": "utf8",
  "connect_timeout": "30",
  "socketTimeout": "60"
}
```

### 4.3 编码映射（encoding → charset）

| encoding 值 | 生成的 charset | 说明                      |
| ----------- | -------------- | ------------------------- |
| `UTF-8`     | `utf8mb4`      | 完整 Unicode 支持（推荐） |
| `UTF8`      | `utf8mb4`      | 同 UTF-8                  |
| `GBK`       | `gbk`          | 简体中文                  |
| `GB2312`    | `gb2312`       | 简体中文（旧）            |
| `LATIN1`    | `latin1`       | 西欧字符                  |
| 其他        | 原值           | 直接透传                  |

### 4.4 最佳实践

| 场景       | 建议                                    | 说明                               |
| ---------- | --------------------------------------- | ---------------------------------- |
| MySQL 8.0+ | 必须设置 `allowPublicKeyRetrieval=TRUE` | 支持 caching_sha2_password 认证    |
| 本地开发   | `useSSL=false`                          | 简化配置，快速连接                 |
| 生产环境   | 启用 SSL，设置 `useSSL=true` 和证书路径 | 加密传输                           |
| 时区处理   | 设置 `serverTimezone`                   | 避免时区偏移，推荐 `Asia/Shanghai` |
| 字符编码   | 强烈推荐 `utf8mb4`                      | 支持完整 Unicode 包括 emoji        |
| 连接超时   | 设置 `connect_timeout`                  | 避免长时间挂起，推荐 30 秒         |
| 读取超时   | 设置 `socketTimeout`                    | 避免长时间查询卡住连接，推荐 60 秒 |
| 连接池     | 通过 Tauri 连接池管理                   | 避免频繁创建/销毁连接              |

### 4.5 安全配置（生产环境）

```json
{
  "allowPublicKeyRetrieval": "TRUE",
  "useSSL": "true",
  "requireSSL": "true",
  "verifyServerCertificate": "true",
  "serverTimezone": "Asia/Shanghai",
  "characterEncoding": "utf8",
  "connect_timeout": "10",
  "socketTimeout": "30"
}
```

| 参数                           | 安全说明                   |
| ------------------------------ | -------------------------- |
| `useSSL=true`                  | 启用 SSL 加密传输          |
| `requireSSL=true`              | 强制要求 SSL 连接          |
| `verifyServerCertificate=true` | 验证服务器证书             |
| `connect_timeout=10`           | 缩短连接超时，减少攻击窗口 |

### 4.6 已知限制

- MySQL 8.0+ 可能限制 `information_schema` 访问，元数据浏览可能返回空
- `USE database` 语句在 prepared statement 协议中不支持，需直连目标库
- 密码特殊字符需 URL 编码
- `allowPublicKeyRetrieval=TRUE` 必须放在 `driver_properties` 中，而非 URL 中

---

## 五、PostgreSQL 配置

### 5.1 连接 URL 格式

```
# 基本格式
postgres://username:password@host:port/database

# 带参数
postgres://postgres:postgresql@localhost:5432/business_db?sslmode=disable

# 注意：驱动内部使用 "postgres" 前缀，非 "postgresql"
```

### 5.2 推荐 driver_properties

```json
{
  "sslmode": "disable",
  "application_name": "RdataStation",
  "connect_timeout": "30",
  "statement_timeout": "30000",
  "idle_in_transaction_session_timeout": "60000"
}
```

### 5.3 SSL 模式说明

| sslmode       | 说明                             | 适用场景     |
| ------------- | -------------------------------- | ------------ |
| `disable`     | 不加密                           | 仅本地开发   |
| `allow`       | 先尝试加密，失败则不加密         | 不推荐       |
| `prefer`      | 先尝试加密，失败则不加密（默认） | 不推荐       |
| `require`     | 加密但不验证证书                 | 需要加密传输 |
| `verify-ca`   | 验证 CA 证书                     | 生产环境推荐 |
| `verify-full` | 验证证书和主机名                 | 最高安全等级 |

### 5.4 两种驱动选择

| 驱动                           | 适用场景           | TLS 支持 | 连接 URL 前缀 |
| ------------------------------ | ------------------ | -------- | ------------- |
| `postgres` (sqlx)              | 通用场景，无需 TLS | ❌       | `postgres://` |
| `postgres_native` (native-tls) | 需要 SSL/TLS 加密  | ✅       | `postgres://` |

### 5.5 最佳实践

| 场景             | 建议                                            | 说明                           |
| ---------------- | ----------------------------------------------- | ------------------------------ |
| 本地开发         | `sslmode=disable`                               | 无需加密，快速连接             |
| 生产环境         | 使用 `postgres_native` 驱动 + `sslmode=require` | 加密传输                       |
| 长时间查询       | 设置 `statement_timeout`                        | 防止挂起，单位毫秒，推荐 30s   |
| 空闲事务         | 设置 `idle_in_transaction_session_timeout`      | 防止未提交事务占用连接         |
| 连接池           | 使用 Tauri 连接池管理                           | 避免频繁创建/销毁              |
| 参数化查询       | 使用 `$1, $2, ...` 占位符                       | 防止 SQL 注入                  |
| Application Name | 设置 `application_name`                         | 便于在 pg_stat_activity 中识别 |

### 5.6 安全配置（生产环境）

```json
{
  "sslmode": "verify-full",
  "sslcert": "/path/to/client-cert.pem",
  "sslkey": "/path/to/client-key.pem",
  "sslrootcert": "/path/to/ca-cert.pem",
  "application_name": "RdataStation",
  "connect_timeout": "10",
  "statement_timeout": "30000",
  "idle_in_transaction_session_timeout": "30000"
}
```

### 5.7 已知限制

- `build_postgres_url` 不编码特殊字符密码，可能破坏 URL 格式
- 生产环境建议使用 `url_template` + URL 编码方案
- `postgres` 驱动（sqlx）不支持 TLS，需要 TLS 时使用 `postgres_native`

---

## 六、DriverConnectionConfig 通用规范

### 6.1 构建方式

```rust
// 方式一：url_override（推荐，前端直接传 URL）
let config = DriverConnectionConfig::new("mysql")
    .with_url_override("mysql://root:root@localhost:3306/testdb")
    .with_connect_timeout(30)
    .with_encoding("UTF-8");

// 方式二：逐字段构建（后端拼接）
let config = DriverConnectionConfig::new("postgres")
    .with_host("localhost")
    .with_port(5432)
    .with_database("business_db")
    .with_username("postgres")
    .with_password("postgresql");

// 方式三：url_template（模板替换，推荐生产环境）
let config = DriverConnectionConfig::new("postgres")
    .with_url_template("postgres://{username}:{password}@{host}:{port}/{database}")
    .with_host("localhost")
    .with_port(5432)
    .with_database("business_db")
    .with_username("postgres")
    .with_password("postgresql");
```

### 6.2 URL 生成优先级

```
url_override > url_template > 硬编码匹配（legacy fallback）
```

**v2.0 重要修复**：所有路径（包括 `url_override`）都会调用 `append_query_params()` 追加 `driver_properties`、`options`、`encoding`。

### 6.3 网络配置集成

当使用 SSH 隧道或 HTTP 代理时：

```
SSH 隧道: host → 127.0.0.1, port → 转发端口
HTTP 代理: 通过代理 URL 发起连接
SSL 证书: 通过 driver_properties 传入证书路径
```

### 6.4 密码脱敏（v2.0 新增）

```rust
use crate::core::services::connection_service::ConnectionService;

// 日志中脱敏
let safe_url = ConnectionService::mask_password_in_url(&url);
log::info!("测试连接: {}", safe_url);
// 输出: 测试连接: postgres://postgres:******@localhost:5432/business_db

// 错误消息中脱敏
let error_msg = format!("连接超时: {}", ConnectionService::mask_password_in_url(&url));
// 输出: 连接超时: postgres://postgres:******@localhost:5432/business_db
```

---

## 七、认证配置

### 7.1 认证类型分类

| 大类       | auth_type        | 说明                | 是否需要凭据 |
| ---------- | ---------------- | ------------------- | ------------ |
| 数据库认证 | `password`       | 用户名+密码         | ✅ 是        |
|            | `ldap`           | LDAP 认证           | ✅ 是        |
|            | `kerberos`       | Kerberos 认证       | ✅ 是        |
|            | `pg_class`       | PostgreSQL 证书认证 | ✅ 是        |
|            | `oauth2`         | OAuth2 认证         | ✅ 是        |
|            | `os_auth`        | 操作系统认证        | ❌ 否        |
|            | `trust`          | 信任认证            | ❌ 否        |
| 网络认证   | `ssh_password`   | SSH 隧道密码        | ✅ 是        |
|            | `proxy_password` | 代理密码            | ✅ 是        |

### 7.2 安全存储流程

```rust
// ===== 存储时 =====
// 1. 接收明文密码
let plaintext = "my_secret_password";

// 2. AES-256-GCM 加密
let encrypted = encrypt_auth_data(plaintext)?;
// 结果: "AES:<base64_encoded_ciphertext>"

// 3. 写入数据库
store_auth_config(auth_type, &encrypted)?;

// ===== 读取时 =====
// 1. 从数据库读取
let encrypted = load_auth_config(auth_type)?;
// 值: "AES:<base64_encoded_ciphertext>"

// 2. AES-256-GCM 解密
let plaintext = decrypt_auth_data(&encrypted)?;
// 结果: "my_secret_password"

// ===== 日志中 =====
// 始终脱敏
let safe_url = ConnectionService::mask_password_in_url(&url);
log::info!("连接 URL: {}", safe_url);
```

### 7.3 注意事项

- `os_auth` / `trust` 等无凭据认证方式**不触发**认证配置保存
- 密码/敏感字段写入前**必须**加密，前缀 `AES:`
- 网络认证配置支持**全局**和**项目级**两层查询（v2.0 修复）
- 认证配置 ID 由**后端统一生成**，禁止前端自生成（如 `G_ssh_auth_${Date.now()}`）
- `auth_data` 只存认证凭据，禁止混入 host/port/database 等连接属性

---

## 八、网络配置

### 8.1 配置类型

| 类型       | 说明                      | 配置内容                                  |
| ---------- | ------------------------- | ----------------------------------------- |
| SSH 隧道   | 通过 SSH 跳板机连接数据库 | host, port, username, auth_type, 转发规则 |
| SSL/TLS    | 数据库连接加密            | 证书路径, sslmode, 验证选项               |
| HTTP 代理  | 通过 HTTP 代理连接        | 代理 URL, 认证信息                        |
| SOCKS 代理 | 通过 SOCKS 代理连接       | 代理 URL, 认证信息                        |

### 8.2 协议链引擎

支持 SSH/Proxy 任意交替穿插（最多 4 跳网络节点），SSL/TLS 固定末尾（流加密包装器，不产生新网络节点）。

```
示例 1: 数据库 ← SSL ← 直接连接
示例 2: 数据库 ← SSL ← SSH 隧道 ← 直接连接
示例 3: 数据库 ← SSL ← HTTP 代理 ← SSH 隧道 ← 直接连接
示例 4: 数据库 ← SSL ← SSH 隧道 ← SOCKS 代理 ← 直接连接
```

### 8.3 配置存储

```rust
// 全局 network_configs 表：6 列基础字段
// (id, name, type, config, auth_config_id, created_at)

// 项目 network_configs 表：9 列（含 origin + 快照溯源）
// (id, name, type, config, auth_config_id, origin, source_id, snapshot_at, created_at)
```

---

## 九、测试运行指南

### 9.1 文件数据库测试（无需外部服务）

```bash
# 进入 src-tauri 目录
cd src-tauri

# 运行 SQLite + DuckDB 测试
cargo test --test file_database_tests -- --test-threads=1

# 运行数据源模块测试
cargo test --test data_source_tests
cargo test --test data_source_commands_integration_tests

# 运行连接命令测试
cargo test --test connection_commands_tests

# 运行全部无需外部服务的测试
cargo test --test file_database_tests \
           --test data_source_tests \
           --test driver_registry_tests \
           --test connection_commands_tests \
           --test error_propagation_snapshot_tests \
           --test connector_tests \
           --test pool_management_tests \
           --test metadata_driver_tests \
           --test auth_store_tests \
           --test e2e_add_datasource_tests
```

### 9.2 MySQL 测试（需要 MySQL 服务）

```bash
# 准备 MySQL 服务（Docker）
docker run -d --name mysql-test \
  -p 3306:3306 \
  -e MYSQL_ROOT_PASSWORD=root \
  mysql:8.0

# 等待 MySQL 启动
sleep 10

# 运行 MySQL 测试（包含被忽略的集成测试）
cargo test --test mysql_integration_tests -- --ignored --test-threads=1

# 运行真实数据库集成测试
cargo test --test real_db_integration_tests -- --ignored --test-threads=1

# 清理
docker stop mysql-test && docker rm mysql-test
```

### 9.3 PostgreSQL 测试（需要 PostgreSQL 服务）

```bash
# 准备 PostgreSQL 服务（Docker）
docker run -d --name pg-test \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=postgresql \
  postgres:16

# 创建测试数据库
docker exec pg-test psql -U postgres -c "CREATE DATABASE business_db"

# 运行 PostgreSQL 测试（包含被忽略的集成测试）
cargo test --test postgresql_tests -- --ignored --test-threads=1

# 清理
docker stop pg-test && docker rm pg-test
```

### 9.4 前端测试

```bash
# 进入项目根目录
cd rdata-station

# 运行所有前端测试
pnpm run test

# 运行特定测试文件
pnpm vitest run src/extensions/builtin/connection/ui/stores/__tests__/

# 运行 Vue 组件测试
pnpm vitest run src/extensions/builtin/connection/ui/components/__tests__/
```

### 9.5 运行全部测试（一键脚本）

```bash
# 后端全部测试
cd src-tauri
cargo test --workspace -- --test-threads=1

# 前端全部测试
cd ..
pnpm run test
```

---

## 十、常见问题排查

### 10.1 MySQL 连接失败

| 症状                                  | 原因                                       | 解决方案                                                                          |
| ------------------------------------- | ------------------------------------------ | --------------------------------------------------------------------------------- |
| `Public Key Retrieval is not allowed` | MySQL 8.0+ 使用 caching_sha2_password 认证 | 在 `driver_properties` 中添加 `allowPublicKeyRetrieval=TRUE`                      |
| `Access denied for user`              | 密码错误或用户权限不足                     | 检查用户名密码，确认认证插件为 `mysql_native_password` 或 `caching_sha2_password` |
| `Communications link failure`         | 网络不可达                                 | 检查 host/port/防火墙，确认 MySQL 监听 `0.0.0.0` 而非 `127.0.0.1`                 |
| `Unknown database`                    | 数据库不存在                               | 先创建数据库，或使用不带数据库名的 URL `mysql://root:root@localhost:3306/`        |
| `The server time zone value`          | 时区未设置                                 | 添加 `serverTimezone=Asia/Shanghai`                                               |

### 10.2 PostgreSQL 连接失败

| 症状                             | 原因           | 解决方案                                                          |
| -------------------------------- | -------------- | ----------------------------------------------------------------- |
| `no pg_hba.conf entry`           | 未配置远程访问 | 编辑 `pg_hba.conf` 添加 `host all all 0.0.0.0/0 md5`              |
| `password authentication failed` | 密码错误       | 检查密码，使用 `ALTER USER postgres PASSWORD 'new_password'` 重置 |
| `SSL is required`                | 服务器要求 SSL | 使用 `postgres_native` 驱动 + `sslmode=require`                   |
| `database does not exist`        | 数据库不存在   | 使用 `CREATE DATABASE db_name` 创建                               |
| `too many connections`           | 连接数超出限制 | 增加 `max_connections` 或关闭空闲连接                             |

### 10.3 SQLite 连接失败

| 症状                           | 原因         | 解决方案                                |
| ------------------------------ | ------------ | --------------------------------------- |
| `unable to open database file` | 路径不存在   | 先创建父目录 `mkdir -p /path/to/dir`    |
| `database is locked`           | 并发写入冲突 | 使用 WAL 模式 `PRAGMA journal_mode=WAL` |
| `permission denied`            | 无写入权限   | 检查文件权限 `chmod 644 database.db`    |
| `no such table`                | 表不存在     | 检查数据库文件是否正确                  |

### 10.4 DuckDB 连接失败

| 症状                  | 原因               | 解决方案                                                      |
| --------------------- | ------------------ | ------------------------------------------------------------- | --- | ----------------------- |
| `IO Error` 打开文件   | 路径不存在         | 先创建父目录                                                  |
| 查询阻塞              | 错误查询卡住线程   | 使用 `spawn_blocking` 隔离：`tokio::task::spawn_blocking(move |     | db.query(sql)).await??` |
| `Serialization Error` | 并发写入冲突       | DuckDB 单文件写入为串行，需排队                               |
| 内存不足              | 大查询超出内存限制 | 设置 `memory_limit` 或 `temp_directory`                       |

### 10.5 密码脱敏验证

确认密码脱敏正常工作：

```bash
# 运行密码脱敏相关测试
cargo test --test connection_commands_tests mask_password -- --nocapture

# 预期输出：所有测试中 URL 密码部分被替换为 ******
```

---

## 十一、前端最佳实践（v2.1 新增）

### 11.1 新增数据源对话框状态管理

基于 v0.5.3 深度审计中发现的 8 个功能缺陷和 3 个 UI/UX 问题，总结以下最佳实践：

#### 表单校验

```typescript
// ✅ 正确：保存前必须校验关键字段，与测试连接保持一致
function saveToStaging() {
  // 1. 驱动选择校验
  if (!selectedDriver.value) return message.warning('请选择数据库类型')

  // 2. 表单校验（validate 函数）
  const validation = validate()
  if (!validation.valid) return message.warning(firstError)

  // 3. 作用域校验
  if (!scope.global && !scope.project) return message.warning('请选择保存位置')

  // 4. 连接字段前置校验（host/port/database，与 handleTest 保持一致）
  if (!selectedDriver.value.is_file) {
    if (!fd.host) return message.warning('请输入主机地址')
    if (port < 1 || port > 65535) return message.warning('端口号必须在 1-65535 之间')
    if (!fd.database) return message.warning('请输入数据库名')
  }
  // ... 后续保存逻辑
}
```

#### 驱动切换状态清理

```typescript
// ✅ 正确：切换驱动时必须清空所有驱动相关状态
function doDriverChange(driverId: string) {
  formData.value = {} // 表单数据
  testResult.value = null // 测试结果
  authConfigId.value = null // 认证配置
  authMethod.value = 'password' // 认证方式（重置默认值）
  selectedEnvId.value = null // 环境
  networkConfigId.value = null // 网络配置
  driverPropertiesExtra.value = null // 驱动属性
  advancedOptions.value = null // 高级选项
  schemaName.value = null // Schema 名称
  options.value = null // 选项
  metadataPath.value = null // 元数据路径
  tags.value = null // 标签
  useDuckdbFed.value = null // DuckDB 联邦
  manualUri.value = '' // 手动 URI
  uriEditing.value = false // URI 编辑模式
}
```

#### 初始化数据保护

```typescript
// ✅ 正确：使用 isRestoring 标志防止初始化时触发 dirty 状态
const isRestoring = ref(false)

watch(
  () => props.modelValue,
  async open => {
    if (open) {
      isRestoring.value = true
      stagingDirty.value = false
      // ... 初始化逻辑
      await nextTick()
      isRestoring.value = false // 初始化完成后重置
    }
  }
)

// ✅ 正确：使用 try/finally 保证标志重置
async function handleSelectStaging(i: number) {
  isRestoring.value = true
  try {
    // ... 恢复暂存项逻辑
  } finally {
    isRestoring.value = false
  }
}
```

#### 编辑模式同步

```typescript
// ✅ 正确：编辑模式下先同步当前表单到暂存，再执行更新
async function handleEditApply() {
  if (!editingConnId.value) return

  syncCurrentToStaging() // 必须先同步

  // ... 后续校验和更新逻辑
}
```

#### 删除确认

```typescript
// ✅ 正确：删除操作必须有确认对话框
function handleRemoveStaging(i: number) {
  const item = stagingItems.value[i]
  if (!item?.name) {
    removeStaging(i)
    return
  }

  dialog.warning({
    title: '删除暂存项',
    content: `确定要删除暂存项 "${item.name}" 吗？此操作不可撤销。`,
    positiveText: '确认删除',
    negativeText: '取消',
    onPositiveClick: () => removeStaging(i),
  })
}
```

#### 网络配置作用域清理

```typescript
// ✅ 正确：作用域从"全局+项目"切到"仅全局"时，清除项目级配置缓存
watch(
  () => props.scope?.project,
  async isProject => {
    if (isProject) {
      const pp = await getProjectPath()
      if (pp) await loadAllProject(pp)
    } else {
      // 项目作用域取消时，重新加载全局配置，清除项目级配置缓存
      await loadAll()
    }
  }
)
```

#### 文件数据库默认路径

```typescript
// ✅ 正确：切换到文件数据库时自动填充默认路径
watch(
  () => props.driver?.id,
  () => {
    if (props.driver?.is_file && !local.file_path) {
      const ext = props.driver.id === 'duckdb' ? 'duckdb' : 'db'
      const defaultName = props.driver.id === 'duckdb' ? 'data.duckdb' : 'data.db'
      local.file_path = homeDir ? `${homeDir}/${defaultName}` : `./${defaultName}`
    }
  }
)
```

### 11.2 UI/UX 最佳实践

#### 按钮状态分离

```typescript
// ✅ 正确：三个操作按钮各自独立 loading 状态
const testing = ref(false) // 测试连接
const saving = ref(false) // 保存
const applying = ref(false) // 应用

// 模板中：
// <NButton :loading="testing" @click="handleTest">测试连接</NButton>
// <NButton type="primary" :loading="saving" @click="handleSave">保存</NButton>
// <NButton type="primary" secondary :loading="applying" @click="handleApply">应用</NButton>
```

#### 测试成功自动同步

```typescript
// ✅ 正确：测试连接成功后自动同步当前表单到暂存
async function onTestModalClose() {
  showTestModal.value = false
  if (!lastTestResult.value?.success) return

  // 测试成功后自动同步当前表单到暂存
  syncCurrentToStaging()

  // 询问是否保存认证配置
  // ...
}
```

#### 实时警告提示

```vue
<!-- ✅ 正确：当勾选"项目连接"但未打开项目时，显示实时警告 -->
<NAlert
  v-if="scope.project && !projectStore.hasProject"
  type="info"
  class="scope-warning"
  :bordered="false"
>
  请先打开或创建一个项目，才能使用项目连接功能
</NAlert>
```

### 11.3 安全覆盖地图（v2.1 完整版）

| 函数/方法                           | 文件                     | 脱敏状态  |
| ----------------------------------- | ------------------------ | --------- |
| `test_connection` 超时日志          | `connection_service.rs`  | ✅ 已脱敏 |
| `test_connection` 超时错误消息      | `connection_service.rs`  | ✅ 已脱敏 |
| `test_connection` 临时连接日志      | `connection_commands.rs` | ✅ 已脱敏 |
| `test_connection_config` 连接日志   | `connection_commands.rs` | ✅ 已脱敏 |
| `ConnectionInfoResponse::from_info` | `connection_commands.rs` | ✅ 已脱敏 |
| `get_recent_connections`            | `connection_commands.rs` | ✅ 已脱敏 |

---

## 十二、版本历史

| 版本 | 日期       | 说明                                                                                                                                                       |
| ---- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| v2.1 | 2026-06-20 | 深度审计补充：新增 SEC-009~SEC-011（3 个高危 URL 脱敏修复）、前端最佳实践（8 个功能缺陷 + 3 个 UI/UX 优化代码示例）、安全覆盖地图（6 处脱敏点完整清单）    |
| v2.0 | 2026-06-20 | 新增安全审计章节（SEC-001~SEC-008）、安全配置（SQLite/DuckDB/MySQL/PostgreSQL 生产环境）、密码脱敏代码示例、网络配置协议链引擎、前端测试指南、一键测试脚本 |
| v1.0 | 2026-06-19 | 初始版本：SQLite/DuckDB/MySQL/PostgreSQL 配置、DriverConnectionConfig 通用规范、认证配置、常见问题排查                                                     |
