# connection crate 系统测试报告



* 日期：2026-10-01（UTC+8）

* 被测对象：`crates/connection`（`rds-connection`），11 个模块 + `tests/tunnel_roundtrip.rs`

* 范围：单元测试、回归测试、集成测试、文档 - 代码一致性核对、原型对齐验证

* 执行者：Doubao 办公模式（MainAgent）

## 0. 结论摘要



| 项          | 结果                                                                   |
| ---------- | -------------------------------------------------------------------- |
| 单元测试       | **96 项全部通过**（扩充前 48 → 扩充后 96，+48）                                    |
| 集成测试       | **6 项全部通过**（扩充前 4 → 扩充后 6，+2）                                        |
| 编译         | `cargo check -p rds-connection --all-targets` 无警告                    |
| 回归         | 依赖方 `rds-workbench` 编译检查通过（`cargo check` exit 0；本次未改任何生产代码，仅加测试与修文档） |
| 文档 - 代码一致性 | 修正 **6 处**不一致（见 §4）                                                  |
| 原型对齐       | 核心语义全部对齐，发现 1 处代码注释过期（见 §5/§6）                                       |
| 发现的缺陷      | 2 个低 / 中危（见 §6），**已于当日修复并回归通过**（见 §9）                                    |

## 1. 复现命令



```
# 全量（单元 + 集成）
cargo test -p rds-connection --all-targets
# 单独单元
cargo test -p rds-connection --lib
# 单独集成
cargo test -p rds-connection --test tunnel_roundtrip
# 警告检查
cargo check -p rds-connection --all-targets
```

实测输出：`96 passed; 0 failed`（lib）+ `6 passed; 0 failed`（tunnel\_roundtrip）。

## 2. 单元测试覆盖（按模块）



| 模块            | 扩充前 | 扩充后 | 新增内容                                                                                                                                                         |
| ------------- | --- | --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `config`      | 3   | 11  | 方法名矩阵、加密 / 代理判定矩阵、ChainHop.hop\_name、TlsRequest 三档语义、serde 默认值（verify\_server\_cert=true /tls1\_2 / 代理超时 30）、SSH/Chain 配置 serde 往返、ConnectionMethod 默认值      |
| `model`       | 3   | 7   | 作用域 hint 文本（对照原型 §2.1）、作用域 serde 往返、DataSource 全字段 serde 往返、DeleteResult/TestResult serde                                                                    |
| `factory`     | 3   | 5   | 同名连接器注册覆盖、全局工厂单例                                                                                                                                             |
| `stream`      | 1   | 2   | TCP/SSH 隧道变体的 method\_name / 地址 / 加密标记（真实回环 socket）                                                                                                          |
| `chain`       | 4   | 8   | 注册表 clone 共享与缺失连接取空、mysql 方言 SSL 参数注入、no\_proxy 命中跳过（apply 与 process\_chain 两层）                                                                              |
| `known_hosts` | 8   | 12  | 带类型前缀条目、逗号分隔多主机、缺失文件加载为空表、**大小写查询归一化回归**（见 §9-1）                                                                                                              |
| `url`         | 7   | 12  | 默认主机 127.0.0.1、无用户名时密码不落 URL、userinfo 百分号编码、编码密码脱敏 / 明文识别                                                                                                    |
| `url_params`  | 14  | 21  | 查询串保留（rewrite/inject/merge）、SSLMODE as\_str 往返与 from\_ssl\_config 边界、mysql\_native Prefer 等价 Require、带查询串的 host/port 解析                                      |
| `secret`      | 5   | 8   | SQL / 路径转义、全类型 USER 关键字矩阵、内存管理器隔离                                                                                                                            |
| `connector`   | 0   | 10  | **新测试模块**：TLS 版本映射、五连接器 supports/name 矩阵、SSH 占位 connect 报 NotSupported、错误方法报错、HTTP CONNECT 成功 + Basic 认证头、非 200 拒绝、**403+头部含 200 拒绝**（见 §9-2）、证书缺失、TunnelGuard drop 发关闭信号、Connection 句柄 |
| 小计            | 48  | 96  | +48                                                                                                                                                          |

## 3. 集成测试（`tests/tunnel_roundtrip.rs`）

新增 2 项，与既有 4 项一起通过：



| 测试                                                     | 覆盖点                                                    |
| ------------------------------------------------------ | ------------------------------------------------------ |
| `http_proxy_auth_roundtrip_relays_payload`（新）          | 带 `Proxy-Authorization: Basic` 的 HTTP CONNECT 隧道完整数据往返 |
| `http_proxy_missing_auth_is_rejected`（新）               | 代理要求认证但未提供 → 数据面必须失败（**不允许静默降级直连**，对应原型「不静默降级」条款）      |
| `socks_proxy_tunnel_relays_payload_and_closes_on_drop` | SOCKS5 隧道往返 + 守卫释放后本地端口关闭                              |
| `http_connect_proxy_tunnel_relays_payload`             | HTTP CONNECT 隧道往返                                      |
| `two_hop_proxy_chain_relays_payload`                   | 两跳代理链往返                                                |
| `no_proxy_rule_skips_proxy_tunnel`                     | no\_proxy 命中不建隧道、URL 原样                                |

隔离原则：全部 127.0.0.1 临时端口，不读写用户文件、不触外网（与 `data-paths.md` §5 一致）。

## 4. 文档 - 代码一致性：修正清单



| # | 位置                                                                      | 原文问题                                                                                                                   | 修正                                                                                        |
| - | ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| 1 | `docs/architecture/connection/README.md` §5                             | 「连接 crate 48（单元 44 + 集成 tunnel\_roundtrip 4）」：单元数漂移（实际 48），合计口径错（44+4≠48）                                              | 改为「**102 = 单元 96 + 集成 tunnel\_roundtrip 6」**（2026-10-01 复跑核对，权威口径见 `module-status.md` §2） |
| 2 | `crates/connection/README.md` § 职责                                      | 「`supports(Ssh)` 直接返回 NotSupported」—— 实际 `connect(Ssh)` 返回 NotSupported，`supports(Ssh)` 为 true（工厂先命中占位器再如实报错）          | 已修正描述                                                                                     |
| 3 | `docs/architecture/connection/connection-crate-interfaces.md` §2.4/§4.1 | 同上「supports (Ssh) 返回 NotSupported」                                                                                     | 已修正                                                                                       |
| 4 | `connection-crate-interfaces.md` §1                                     | 「lib.rs 的 **9** 个 pub mod」—— 实际 **10** 个                                                                               | 已修正                                                                                       |
| 5 | `connection-crate-interfaces.md` §2.4                                   | Connector「返回 Connection」—— 实际 `connect()` 返回 `ConnectionStream`（`create_connection` 再包 `Connection`）；Connection 字段描述不准 | 已修正                                                                                       |
| 6 | `crates/connection/README.md` + `connection-crate-interfaces.md` §5     | 「换 `RDS_*` 环境变量可指向真实 SSH 目标」—— 测试文件**无任何 RDS\_\* 支持**（纯回环代理）                                                           | 改为真实描述（本地回环 echo + 最小 HTTP CONNECT / SOCKS5 代理端）                                          |
| 7 | `crates/connection/README.md` § 依赖                                      | 「依赖：仅 rds-shared」—— 字面上不准确（外部库 20+）                                                                                    | 改为「内部依赖仅 rds-shared；其余为外部库（见 Cargo.toml）」                                                 |
| 8 | 两处测试数量声明（README 与接口文档）                                                  | 48 项单测                                                                                                                 | 同步为 **96 项**（扩充并修复后）                                                                         |

核对**一致**的项目（未改动）：



* 版本注释：russh 0.63.3（`PrivateKeyWithHashAlg` 路径）、duckdb 1.10505.0 = 1.5.5（Secret 真机实测注释）、mysql\_async 0.37（`opts/mod.rs` 只认 `require_ssl`/`verify_ca`/`verify_identity`）—— 与 `Cargo.toml` 逐项吻合。

* 公开接口清单：`lib.rs` 的 10 个 `pub mod` + 5 个顶层 `pub use`，与接口文档 §2 表一致。

* 接线关系：workbench 消费 `connection::config / chain / url_params / secret / connector / model`，与 README「接线」段一致（`engine/src/driver/declaration.rs` 存在，Secret 被 `data_source_lifecycle` 等使用）。

* 数字权威口径：`module-status.md` §2 与实测一致。

## 5. 原型对齐验证（`connection-prototype-design.md`）



| 原型条款                                                                                       | 实现                                                    | 验证                                                            |
| ------------------------------------------------------------------------------------------ | ----------------------------------------------------- | ------------------------------------------------------------- |
| §2.1 作用域三态：仅全局→`global_connections`（G\_）；仅项目→`project.db`（P\_）；全局 + 项目→全局（G\_）+ 共享快照（GP\_） | `model.rs ConnectionScope` + `hint()`                 | 单测 `test_scope_hint_texts` 逐字断言                               |
| §4 测试连接返回 `{success, message, latency_ms, version}`                                        | `model.rs TestResult` 四字段一致                           | 单测 `test_delete_and_test_result_serde`                        |
| 测试连接「与真实连接同源、真实建立网络档案隧道（测完即关）、**不静默降级**」                                                   | `chain::apply_network_method` + `TunnelGuard` drop 清理 | 集成测试 6 项（含新增缺认证被拒用例）+ `test_tunnel_guard_drop_sends_shutdown` |
| §3.1 字段⇄URI 双向同步（保留凭据与查询串；主机空不重建；数据库空去路径段）                                                 | `url_params::rewrite_url_authority`                   | 既有 + 新增查询串用例                                                  |
| SSL 模式五档 + 驱动分派词汇                                                                          | `SslMode` + `append_ssl_params`                       | `test_ssl_mode_as_str_roundtrip_and_from_config_edges` 等      |
| §3.5 DuckDB 本地加速（PERSISTENT Secret）                                                        | `secret.rs`                                           | 既有持久化复开用例通过                                                   |
| §2.1 作用域「**不再有行内落点提示行**」                                                                   | `model.rs hint()` 文档注释仍写「与原型 Header 作用域提示行一致」         | **注释过期**（见 §6-3）；hint () 未在 workbench 使用                      |

## 6. 发现的问题与建议



| 级别    | 位置                                        | 问题                                                                                                                                               | 建议                                                       |
| ----- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------- |
| 中（P2） | `known_hosts.rs`                          | **大小写不一致**：写入侧 `normalize_host` 归一化为小写，查询侧 `find_candidates` 不做归一化 → 条目 `host-a` 用 `HOST-B` 查询会 `verify` 失败。已用 `test_comma_separated_hosts` 固化现状 | 在 `find_candidates`（或 `verify` 入口）对查询主机做同样归一化；修复后更新该测试断言 |
| 低（P3） | `connector.rs::establish_http_proxy`      | 成功判据为 `response.contains("200")`（子串匹配），未解析状态行：`403` 且正文 / 头部含 "200"（如 `Content-Length: 200`）的响应会被误判为成功                                           | 改为解析首行状态码（`HTTP/1.1 200` 前缀匹配或 `split` 取状态码）             |
| 低（P3） | `model.rs` `ConnectionScope::hint()` 文档注释 | 注释引用「原型 Header 作用域提示行」，原型 v2（§2.1/§7 #6）已删除该行                                                                                                    | 改注释或删除 hint ()（当前无消费方）                                   |

本次发现的两项代码缺陷**已于当日修复并通过全量回归**（见 §9）；P3 注释过期项（`hint()`）无消费方，维持现状不冒险改代码。

## 7. 变更文件清单



| 文件                                                                  | 变更                                                 |
| ------------------------------------------------------------------- | -------------------------------------------------- |
| `crates/connection/src/config.rs`                                   | +8 单测                                              |
| `crates/connection/src/model.rs`                                    | +4 单测                                              |
| `crates/connection/src/factory.rs`                                  | +2 单测                                              |
| `crates/connection/src/stream.rs`                                   | +1 单测                                              |
| `crates/connection/src/chain.rs`                                    | +4 单测                                              |
| `crates/connection/src/known_hosts.rs`                              | +4 单测（含大小写归一化回归）                               |
| `crates/connection/src/url.rs`                                      | +5 单测                                              |
| `crates/connection/src/url_params.rs`                               | +7 单测                                              |
| `crates/connection/src/secret.rs`                                   | +3 单测                                              |
| `crates/connection/src/connector.rs`                                | 新增测试模块，+10 单测；修复 HTTP 代理状态行判据                  |
| `crates/connection/tests/tunnel_roundtrip.rs`                       | +2 集成测试（认证 HTTP 代理正 / 负向）                          |
| `crates/connection/README.md`                                       | 修正 SSH 占位描述、RDS\_\* 描述、依赖口径、测试数量                   |
| `docs/architecture/connection/connection-crate-interfaces.md`       | 修正 pub mod 数、Connector 返回类型、SSH 占位、RDS\_\* 描述、测试数量 |
| `docs/architecture/connection/README.md`                            | 修正 §5 基准数字（48→102 口径）                              |
| `docs/architecture/connection/connection-test-report-2026-10-01.md` | 本报告                                                |

## 8. 遗留项



* `ConnectionStream::Tls` 变体无法脱离真实 TLS 握手构造，单测未覆盖其地址 / 加密访问器（`wrap_tls_stream` 的握手路径由 workbench 真实拨号套件覆盖，不在本 crate）。

## 9. 缺陷修复记录（2026-10-01）

| # | 修复 | 变更 | 回归验证 |
| - | --- | --- | --- |
| 9-1 | **known_hosts 查询侧大小写归一化**（P2）：`find_candidates` 改用 `normalize_host` 后查表，与写入侧一致；同时清掉重复查表的冗余分支 | `known_hosts.rs` `find_candidates` | `test_comma_separated_hosts_and_case_insensitive_lookup`（HOST-A/HOST-B 大写查询命中）+ 新增 `test_mixed_case_entry_matches_lower_and_upper_queries` |
| 9-2 | **HTTP 代理成功判据改为状态行解析**（P3）：`establish_http_proxy` 解析首行 `HTTP/1.x 200`，不再对整段响应做 `contains("200")` 子串匹配 | `connector.rs` `establish_http_proxy` | 新增 `test_http_proxy_403_with_200_in_headers_still_errors`（403 + `Content-Length: 200` 必须报错） |

修复后全量复跑：**单元 96 + 集成 6 = 102 项全部通过，`cargo check --all-targets` 无警告**；`rds-workbench` 编译检查通过（生产 API 未变）。