# 日志系统（tracing → stderr / 文件 / 全局库）

状态：**已接线**（2026-09-16）。装配点 `crates/app/src/main.rs::init_global_system`。
文件位置由 `paths::log_dir()` 解析（见 `data-paths.md`）。

## 1. 形态：一条事件，三个出口

| 出口 | 内容 | 位置 |
| --- | --- | --- |
| stderr | 实时诊断（终端 / 开发时看） | — |
| 文件 | 按天滚动 `app.YYYY-MM-DD`，**逐行脱敏** | `<RDS_HOME>/logs` |
| 全局库 | `app_logs` 表（异步批量写，脱敏） | `<RDS_HOME>/data/system/global.db` |

- **过滤**：`RUST_LOG` 优先（`EnvFilter` 语法），否则取**设置页的「日志级别」**
  （`logging.min_level`，默认 `INFO`）。改设置会即时 reload，不用重启。
  ⚠ 调到 `DEBUG`/`TRACE` 会把**第三方**的内部日志一起放出来（gpui / globset 之类），
  量很大、排障时反而难找：想看具体模块用 `RUST_LOG=rds_engine=debug,rds_app=debug`
  （环境变量优先级高于设置项）。
- **保留与上限（三道闸）**：

  | 闸 | 默认 | 何时生效 |
  | --- | --- | --- |
  | 过期天 | 7 天 | 启动清理：删过期的 `app.YYYY-MM-DD` |
  | 目录配额 | 256 MiB | 启动清理：过期之后仍超配额 → 从最旧的开始删 |
  | 单文件上限 | 16 MiB | **写入时**：到顶后本进程不再写文件（只补一行说明），库与 stderr 照常 |

  只有"过期删"挡不住一天内写几个 G（日志风暴），所以写入侧单独卡一道。
  库侧另有 10 万条上限（`max_db_records`）。
- **落库是异步的**：`DatabaseLogLayer` 只把记录塞进无界 channel，消费者任务按
  **每 100 条或每 1 秒** 批量写一次事务。因此进程直接退出会丢最后一批——
  `App::on_app_quit` 里调 `engine::flush_logs()` 把它补上（见 §5）。

## 2. 启动顺序（这条是有约束的）

```text
main()  : install_process_temp_dir → ensure_dirs → migrate_legacy_layout
run_app : 注册日志级别 sink（设置 → reload） + on_app_quit（退出前 flush）
          → gpui_kit → init_global_system()
                        ├─ initialize_global_system()   # 建 global.db、跑迁移（006 建 app_logs）
                        └─ init_app_logging(配置从设置读) # 挂订阅者 + 起消费者任务
          → SettingsService::init → 主题 → 键位
```

**为什么必须排在全局库之后**：库层要往 `app_logs` 写，而该表由迁移创建；消费者是
tokio 任务，需要运行时上下文（`runtime.enter()` 挂上后再 `spawn`）。

**级别从哪来**：`SettingsService::init` 还没跑（设置页尚未创建），所以直接读盘
（`settings::load_settings()`）构 `LogConfig`；之后用户在设置页改级别时，走装配层注册的
sink 直接 `reload_log_level`，不重走启动流程。

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
v2 去掉的是 Tauri 侧：v1 通过 command 把日志查给前端，v2 改用**应用内对话框**（见 §5）。

## 5. 怎么看到日志（读路径）

三处入口，对应三种问法：

| 问法 | 入口 | 看的是 |
| --- | --- | --- |
| “刚才那一下怎么了” | 设置页 → 日志 → **查看日志…** | 全局库 `app_logs`（最近 500 条，级别门槛 + 关键字） |
| “完整原文 / 崩溃前的” | 设置页 → 日志 → **打开日志目录** | 文件 `app.YYYY-MM-DD`（含崩前已写入的部分） |
| “现在采到多细” | 设置页 → 日志 → **日志级别** | 改完即时生效（`reload_log_level`） |

对话框（`components/log_dialog.rs`）的两个有意选择：

- **只读快照 + 手动刷新**，不自动轮询：轮询会让“我看到的和上一条不一致”变成日常。
- 级别是**门槛**（≥）而不是精确匹配：查一次取最新 500 条，级别在本地筛，
  所以点级别是瞬时的（`LogQuery::level` 是精确匹配，拿它做门槛得查 5 次）。

入口、宿主与重绘：对话框层挂在**宿主视图**的 render 里（`Root::render_dialog_layer`），
而 `cx.notify()` 只重渲染被标脏的子树——入口是设置页里的一个按钮，没有东西会顺着标脏到
工作台，所以 `open_log_dialog` 末尾显式 `cx.refresh_windows()`（否则表现是“点了没反应”）。

## 6. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 三个出口的装配 + reload handle | `crates/engine/src/logging/subscriber.rs::init_tracing_with_db` |
| 应用口径入口（目录 / 级别 / 保留 / 上限 / 库连接） | `crates/engine/src/logging/mod.rs::init_app_logging` |
| 目录 / 保留天数 / 单文件上限 / 目录配额 / 库上限 | `crates/engine/src/logging/config.rs`（默认值） |
| 记录结构（与 `app_logs` 列对应） | `crates/engine/src/logging/record.rs` |
| 脱敏规则（URL 权限段 + `key=value`） | `crates/engine/src/logging/redact.rs` |
| 文件层脱敏 + 单文件上限 + 停写说明 | `crates/engine/src/logging/subscriber.rs`（`RedactingMakeWriter` / `FileSink`） |
| 启动清理（过期 + 目录配额） | `crates/engine/src/logging/subscriber.rs::cleanup_log_files` |
| 落库（批量 + 上限裁剪 + 过期清理） | `crates/engine/src/persistence/log_store.rs` |
| `app_logs` 表 | `crates/engine/migrations/global/006_add_app_logs.sql` |
| 退出前 flush（`on_app_quit` → `flush_logs` → 消费者 oneshot 确认） | `crates/app/src/main.rs` + `logging/subscriber.rs::request_flush` |
| 启动接线 + 补记启动结果 | `crates/app/src/main.rs::init_global_system` |
| 级别设置项 + 运行时 reload 的 sink | `crates/settings/src/{model,registry,lib}.rs` + `crates/app/src/main.rs`（装配点） |
| 查看对话框 + 两个动作行 | `crates/workbench/src/components/log_dialog.rs` + `settings/src/settings_page.rs` |
| 对话框尺寸 | `crates/workbench_shell/src/ui.rs`（`DIALOG_LOG_*`） |
| 生命周期 | `crates/engine/src/logging/mod.rs`（`get_log_store` / `set_log_store` / `session_id` / `flush_logs`） |

## 7. 未做

| 项 | 说明 |
| --- | --- |
| 历史会话切换 | 对话框只看“最近 500 条”，不能按 `session_id` 分组或只看上次启动（表里已有该列，要加得先定“用户真的会按会话看吗”） |
| 字段明细 | 记录行只显示 `message`，`fields`（JSON）存了但没展示；要看详情得再做一个展开区 |
| 自动刷新 | 有意不做（§5）；真需要时优先考虑“打开期间每 2 秒拉一次”的开关，而不是默认轮询 |
| 级别配置的二级项 | 只有全局级别；没有按模块（`target`）分别设级别（v1 的 `module_levels` 仍是空字段） |
