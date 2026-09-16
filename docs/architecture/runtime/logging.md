# 日志系统（tracing → stderr / 文件 / 全局库）

状态：**已接线**（2026-09-16）。装配点 `crates/app/src/main.rs::init_global_system`。
文件位置由 `paths::log_dir()` 解析（见 `data-paths.md`）。

## 1. 形态：一条事件，三个出口

| 出口 | 内容 | 位置 |
| --- | --- | --- |
| stderr | 实时诊断（终端 / 开发时看） | — |
| 文件 | 按天滚动 `app.YYYY-MM-DD`，**逐行脱敏** | `<RDS_HOME>/logs` |
| 全局库 | `app_logs` 表（异步批量写，脱敏） | `<RDS_HOME>/data/system/global.db` |

- **过滤**：`RUST_LOG` 优先（`EnvFilter` 语法），否则 `Info`。运行时可用
  `reload_log_level("debug")` 动态改级别（底层是 `reload` layer 的 handle）。
- **保留**：文件 7 天（启动时清理过期的 `app.*` 文件）；库 10 万条
  （`LogConfig::default()` 的 `retention_days` / `max_db_records`）。
- **落库是异步的**：`DatabaseLogLayer` 只把记录塞进无界 channel，消费者任务按
  **每 100 条或每 1 秒** 批量写一次事务。进程被杀时最多丢最后 1 秒的记录。

## 2. 启动顺序（这条是有约束的）

```text
main()  : install_process_temp_dir → ensure_dirs → migrate_legacy_layout
run_app : gpui_kit → init_global_system()
                        ├─ initialize_global_system()   # 建 global.db、跑迁移（006 建 app_logs）
                        └─ init_app_logging()           # 挂订阅者 + 起消费者任务
          → SettingsService::init → 主题 → 键位
```

**为什么必须排在全局库之后**：库层要往 `app_logs` 写，而该表由迁移创建；消费者是
tokio 任务，需要运行时上下文（`runtime.enter()` 挂上后再 `spawn`）。

**代价**：全局库建立之前的日志不落文件也不落库（那时还没有订阅者）。所以启动代码在
接线之后**补记**一条结果：

```text
INFO  rds_app: 全局系统库初始化完成
INFO  rds_app: data_root="D:\\...\\.rds" origin="环境变量 RDS_HOME" 日志系统已启用
```

排障时先看这两行：有它 = 日志链路是通的；只有 stderr = 订阅者没挂上（会打印原因）。

## 3. 脱敏口径（两个出口都要，且要一致）

日志里最容易带密码的是**连接串与错误串**，因此：

| 出口 | 脱敏位置 |
| --- | --- |
| 文件 | `RedactingMakeWriter` / `RedactingWriter`：**按行**攒够再脱敏写盘（模式不能跨行匹配） |
| 全局库 | `DatabaseLogLayer`：`message` 与**每个字段值**都过一遍 `redact_sensitive` |

`redact_sensitive`（`logging/redact.rs`）覆盖两类模式：

1. URL 权限段：`scheme://user:password@host` → `scheme://user:***@host`
   （一行里的**每个** URL 都处理；权限段以 `/`、`?`、空白、引号为界，
   所以 `mysql://host/db?u=a@b` 里的 `@` 不会被误判成凭据）
2. 键值对：`password=` / `pwd=` / `pass=` / `passwd=` / `secret=` → `***`

> 为什么要包一层而不是"调用点注意点"：调用点只会越来越多，漏一处就是一条明文密码落盘。
> 文件层曾经是明文直写（DB 层早有脱敏），接线时一并补上。

## 4. 与 v1 的关系

`v1/docs/backend/LOGGING_MODULE.md` 是参考实现（分层、保留策略、`app_logs` 表结构均沿用）。
v2 去掉的是 Tauri 侧：v1 通过 command 把日志查给前端，v2 目前没有消费方
（查询类型 `LogQuery` / `LogPage` / `LogStats` / `TargetStat` 已在 `logging/record.rs` 备好）。

## 5. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 三个出口的装配 + reload handle | `crates/engine/src/logging/subscriber.rs::init_tracing_with_db` |
| 应用口径入口（目录 / 级别 / 保留 / 库连接） | `crates/engine/src/logging/mod.rs::init_app_logging` |
| 日志目录 / 保留天数 / 库上限 | `crates/engine/src/logging/config.rs`（`LogConfig::default()` 的 `log_dir` = `paths::log_dir()`） |
| 记录结构（与 `app_logs` 列对应） | `crates/engine/src/logging/record.rs` |
| 脱敏规则 | `crates/engine/src/logging/redact.rs` |
| 落库（批量 + 上限裁剪 + 过期清理） | `crates/engine/src/persistence/log_store.rs` |
| `app_logs` 表 | `crates/engine/migrations/global/006_add_app_logs.sql` |
| 启动接线 + 补记启动结果 | `crates/app/src/main.rs::init_global_system` |
| 生命周期 | `crates/engine/src/logging/mod.rs`（`get_log_store` / `set_log_store` / `session_id` / `flush_logs`） |

## 6. 未做

| 项 | 说明 |
| --- | --- |
| 日志查询 UI | 无消费方。要做得先定"给谁看"（设置页的日志页？还是"打开日志目录"就够） |
| 优雅退出 flush | 进程退出即结束；最多丢最后 1 秒的记录 |
| 级别界面的入口 | `LogConfig::min_level` 目前只能靠 `RUST_LOG` 或 `reload_log_level()` 改 |
| 单文件大小上限 | 现在只按天滚动；一天内写得极多就是一个大文件（暂不处理） |
