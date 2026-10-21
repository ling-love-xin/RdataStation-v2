# tools/rds-jdbc-shell — Java 通用 JDBC 驱动壳

一次开发、全库复用的 JDBC 驱动 sidecar。换数据库 = 换驱动 jar + URL 模板，本壳零改动。

## 定位

- **进程形态**：rds-driver/1 sidecar（走 `supervisor` 标准轨：spawn → initialize → session → meta/query → shutdown，协议与生命周期与宿主 Rust 实现完全一致）
- **语言/运行时**：Java 21+，**不内嵌 JRE**。解释器由宿主 `runtime.rs` 三级查找（用户指定 → `JAVA_HOME`/PATH → `third_party/jre`，见 `crates/plugin/src/runtime.rs`）
- **驱动加载**：壳 jar 同目录（及同目录 `lib/`）下的驱动 jar 用 URLClassLoader 自举，无需 `-cp`

## 目录

```
tools/rds-jdbc-shell/
├── build.ps1              # 构建（javac → jar）；自动探测 java.home 找 bin\jar.exe
├── smoke.ps1              # 全链路冒烟测试（真实进程 + 分帧协议）
├── lib/                   # 产物 rds-jdbc-shell.jar + 驱动 jar（如 h2-2.5.252.jar）
├── src/rds/jdbc/
│   ├── Main.java          # 入口：自举驱动注册 → 协议主循环
│   ├── Server.java        # JSON-RPC 分发（方法面见下）
│   ├── Session.java       # 会话：连接/执行/meta/取消；URL 构造
│   ├── Meta.java          # meta.catalogs/schemas/objects/object_detail
│   ├── DriverRegistry.java# 驱动 jar 扫描与注册（绕过 DriverManager 的 isDriverAllowed 陷阱）
│   ├── FrameIO.java       # 二进制分帧（对齐 sidecar/proto.rs）
│   ├── Json.java          # 极简 JSON（无外部依赖）
│   └── RpcException.java  # 协议错误码（对齐 RpcErrorCode -32001…-32009）
```

## 构建与验证

```powershell
# 构建（需 JDK 21；javac 在 PATH 即可，jar 从 java.home 探测）
powershell -ExecutionPolicy Bypass -File build.ps1

# 全链路验证（H2 内存库；需已下载 h2 jar 到 lib/）
powershell -ExecutionPolicy Bypass -File smoke.ps1
```

smoke 覆盖：initialize 握手 → session.open → driver.describe → DML/查询 →
meta.schemas/objects → 参数化查询 → query.cancel（契约级）→ session.close → shutdown。
当前 16 项断言。

## 方法面（v1）

| 方法 | 说明 |
| --- | --- |
| `initialize` | 协议版本握手，返回 driver_ids=["jdbc"] |
| `session.open/close/ping` | 会话生命周期（params.params = 连接配置原样透传） |
| `driver.describe` | 运行时自述（能力/版本/默认 schema） |
| `meta.catalogs/schemas/objects/object_detail` | DatabaseMetaData → wire 形状 |
| `meta.routine_source` | 返回 null（JDBC 无标准例程源码接口） |
| `query.execute` | 查询/DDL/DML；参数绑定；max_rows；timeout；内联截断 |
| `query.fetch` | v1 显式 -32001（游标/Arrow 第二期） |
| `query.cancel` | Statement.cancel() 真取消 + 幂等 |

## 连接配置 → URL

`url_override` 优先 → `url_template` 模板替换（`{host}/{port}/{database}/{file}/{user}/{password}`）→
按 driver 兜底（h2/sqlite 特殊处理，其余 `jdbc:<driver>://host:port/db`）。

## 关键实现取舍

- **不走 `DriverManager.getConnection`**：子 URLClassLoader 加载的驱动会被
  isDriverAllowed（按调用者类加载器）拒绝——经典 JDBC 陷阱。连接改为
  acceptsURL 匹配驱动实例直连。
- **内联结果 v1**：上限 200 行 / 256 KiB，超出截断 + truncated + notice；
  Arrow 附件第二期。
- **取消**：running 表（request_id → Statement）+ cancel 标志，SQLException 时
  区分「被取消」(-32004) 与「SQL 错」(-32003)。

## 已知限制（v1）

- 大结果集截断（Arrow 未接）；`query.fetch` 不支持
- H2 冒烟里 cancel 无法可靠复现"执行中"阻塞（H2 对 SLEEP/RANGE 不支持中断），
  深度 cancel 验证留给真实 Oracle/PG 插件（任务 #39）
- JDK 21+ 要求（代码用了 switch 表达式 / records 无关特性，-source 21）
