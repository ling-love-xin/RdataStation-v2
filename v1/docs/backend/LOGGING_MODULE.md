# 日志模块 (Logging Module)

> 版本：v1.2
> 最后更新：2026-05-10
> 作者：RdataStation 开发团队

---

## 一、模块概述

日志模块是 RdataStation 的统一日志基础设施，基于 `tracing` 生态构建。它将所有应用运行时日志（`tracing::info!` / `tracing::error!` 等宏调用）截获并输出到三个目标：

| 输出目标 | 说明                                               | 持久化        |
| -------- | -------------------------------------------------- | ------------- |
| stderr   | 控制台实时输出，供开发者调试                       | ❌ 不持久     |
| 文件     | 按天滚动 `{data}/RdataStation/logs/app.YYYY-MM-DD` | ✅ 7天保留    |
| SQLite   | `global.db` → `app_logs` 表，支持前端查询          | ✅ 上限10万条 |

### 为什么需要日志持久化？

1. **问题回溯**：数据库管理工具的操作具有高风险性（DROP TABLE、DELETE 等），用户需要事后追溯操作历史和自我排查
2. **会话恢复**：应用重启后保留上次运行的完整日志上下文（通过 `session_id` 区分）
3. **前端诊断面板**：为用户提供可视化的日志查询界面，支持按级别/模块/时间/关键字过滤
4. **运维审计**：在团队协作场景中，日志可作为审计追踪依据（为后续 DuckLake 多用户预留）

---

## 二、架构设计

```
┌─────────────────────────────────────────────────────────┐
│               业务代码 (30+ 处 tracing 宏)               │
│     tracing::info!("SQL executed");                     │
│     tracing::error!("Connection failed: {}", e);        │
└──────────────────────┬──────────────────────────────────┘
                       │ tracing::Event
                       ▼
┌──────────────────────────────────────────────────────────┐
│              tracing-subscriber Registry                  │
│  ┌─────────────┐ ┌─────────────┐ ┌──────────────────┐   │
│  │ fmt layer   │ │ file layer  │ │ DatabaseLogLayer │   │
│  │ (stderr)    │ │ (滚动文件)  │ │ (自定义 Layer)   │   │
│  └──────┬──────┘ └──────┬──────┘ └────────┬─────────┘   │
│         │               │                 │              │
│         ▼               ▼                 ▼              │
│      stderr      app.YYYY-MM-DD    mpsc channel          │
│                                        │                 │
└────────────────────────────────────────┼─────────────────┘
                                         │
┌────────────────────────────────────────┼─────────────────┐
│                          spawn_log_consumer              │
│                              │                            │
│                    batch(100) / 定时1s                    │
│                              ▼                            │
│                    LogStore::flush_records()              │
│                              │                            │
│                              ▼                            │
│                   global.db → app_logs                    │
└──────────────────────────────────────────────────────────┘
                                         │
┌────────────────────────────────────────┼─────────────────┐
│                    Tauri Commands                         │
│   get_logs / search_logs / get_log_stats / export_logs   │
└──────────────────────────────────────────────────────────┘
```

### 关键设计决策

| 决策                         | 理由                                                                                          |
| ---------------------------- | --------------------------------------------------------------------------------------------- |
| 使用 `tracing` 而非 `log`    | 项目已有 `tracing = "0.1.41"`，结构化，异步友好                                               |
| 自定义 Layer 而非 MakeWriter | `Layer::on_event` 可直接访问元数据，无需解析文本                                              |
| mpsc channel + 批量写入      | 避免每条日志同步写 DB，批量 100 条或 1s 间隔落盘                                              |
| unbounded channel            | 日志生产速度不可控，bound 可能导致丢日志或死锁                                                |
| 两阶段启动（lib.rs）         | tracing subscriber 必须最先初始化（阶段1），DB 就绪后再创建 LogStore + 启动 consumer（阶段2） |

---

## 三、目录结构

```
src-tauri/
├── Cargo.toml                          # + tracing-subscriber, tracing-appender
├── migrations/global/
│   └── 006_add_app_logs.sql            # app_logs 表 + 索引
└── src/
    ├── lib.rs                          # 两阶段初始化入口
    ├── core/
    │   ├── mod.rs                      # + pub mod logging; re-exports
    │   ├── logging/
    │   │   ├── mod.rs                  # 模块入口 + init_logging API + OnceLock
    │   │   ├── record.rs               # LogRecord / LogLevel / LogQuery / LogStats
    │   │   ├── config.rs               # LogConfig 配置结构
    │   │   ├── redact.rs               # 敏感数据脱敏（密码/连接字符串）
    │   │   └── subscriber.rs           # DatabaseLogLayer + tracing 初始化 + reload handle
    │   └── persistence/
    │       ├── mod.rs                  # + pub mod log_store; re-exports
    │       └── log_store.rs            # LogStore SQLite CRUD
    └── commands/
        └── logging_commands.rs         # 7个 Tauri 命令
```

---

## 四、数据模型

### 4.1 LogRecord (SQLite ↔ 前端)

```rust
pub struct LogRecord {
    pub id: i64,                    // 自增主键
    pub timestamp: String,          // "2026-05-10T14:30:00.123Z"
    pub level: LogLevel,            // TRACE/DEBUG/INFO/WARN/ERROR
    pub target: String,             // "rdata_station::core::sql_service"
    pub message: String,            // 日志消息体
    pub fields: Option<String>,     // 结构化字段 JSON (nullable)
    pub file: Option<String>,       // 源文件路径
    pub line: Option<u32>,          // 行号
    pub session_id: String,         // UUID v4 应用会话
}
```

### 4.2 LogQuery (查询参数)

```typescript
interface LogQuery {
  page?: number // default: 1
  page_size?: number // default: 50, max: 500
  level?: string // "ERROR" | "WARN" | ...
  target?: string // LIKE '%sql_service%'
  keyword?: string // LIKE '%timeout%' in message/target
  start?: string // ISO 8601 start
  end?: string // ISO 8601 end
}
```

### 4.3 LogStats (统计)

```typescript
interface LogStats {
  total: number
  by_level: { trace: number; debug: number; info: number; warn: number; error: number }
  by_target: Array<{ target: string; count: number }>
  first_timestamp?: string
  last_timestamp?: string
}
```

### 4.4 SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS app_logs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT NOT NULL,                         -- ISO 8601
    level       TEXT NOT NULL CHECK(level IN ('TRACE','DEBUG','INFO','WARN','ERROR')),
    target      TEXT NOT NULL,                         -- 模块路径
    message     TEXT NOT NULL,
    fields      TEXT,                                  -- JSON (nullable)
    file        TEXT,
    line        INTEGER,
    session_id  TEXT NOT NULL,                         -- UUID
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON app_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_logs_level ON app_logs(level);
CREATE INDEX IF NOT EXISTS idx_logs_target ON app_logs(target);
CREATE INDEX IF NOT EXISTS idx_logs_session ON app_logs(session_id);
```

---

## 五、API 接口

### 5.1 Rust 内部 API

#### 初始化

```rust
// lib.rs 中两阶段启动
// 阶段1: tracing subscriber 初始化（stderr + 文件 + DB channel）
let rx = init_tracing_with_db(&log_dir, LogLevel::Info, 7)?;

// 阶段2: DB 就绪后创建 LogStore + 启动消费任务
let store = Arc::new(LogStore::new(manager.sqlite_pool()));
let handle = spawn_log_consumer(rx, store);
```

#### LogStore

```rust
impl LogStore {
    // 批量写入（subscriber 调用）
    pub async fn flush_records(&self, records: &[LogRecord]) -> Result<(), CoreError>;

    // 分页查询
    pub async fn query_logs(&self, query: &LogQuery) -> Result<LogPage, CoreError>;

    // 统计
    pub async fn get_stats(&self) -> Result<LogStats, CoreError>;

    // 清理
    pub async fn cleanup(&self, before: Option<&str>) -> Result<usize, CoreError>;
}
```

### 5.2 Tauri Commands (前端调用)

| 命令                 | 参数                                                       | 返回             | 说明                                              |
| -------------------- | ---------------------------------------------------------- | ---------------- | ------------------------------------------------- |
| `get_logs`           | page?, page_size?, level?, target?, keyword?, start?, end? | `LogPage`        | 分页查询                                          |
| `search_logs`        | keyword, level?, target?                                   | `LogPage`        | 关键字搜索                                        |
| `get_log_stats`      | -                                                          | `LogStats`       | 统计信息                                          |
| `clear_logs`         | before? (ISO time)                                         | `usize`          | 清理旧日志                                        |
| `get_log_session_id` | -                                                          | `String`         | 当前会话 ID                                       |
| `export_logs`        | level?, start?, end?, max_results?                         | `Vec<LogRecord>` | 导出（默认1万条，上限5万条）                      |
| `set_log_level`      | level (String)                                             | `()`             | 动态修改全局日志级别，通过 reload handle 实时生效 |

#### 调用示例 (TypeScript)

```typescript
import { invoke } from '@tauri-apps/api/core'

// 查询最近 20 条 ERROR 日志
const result = await invoke<LogPage>('get_logs', {
  page: 1,
  page_size: 20,
  level: 'ERROR',
})

// 搜索包含 "timeout" 的日志
const searchResult = await invoke<LogPage>('search_logs', {
  keyword: 'timeout',
  target: 'connection',
})

// 获取统计
const stats = await invoke<LogStats>('get_log_stats')
```

### 5.3 LogConfig 配置

```rust
pub struct LogConfig {
    pub min_level: LogLevel,                  // default: Info
    pub module_levels: HashMap<String, LogLevel>, // 模块级覆盖（预留）
    pub file_output: bool,                    // 是否输出到文件（预留）
    pub db_output: bool,                      // 是否输出到数据库（预留）
    pub log_dir: PathBuf,                     // 日志文件目录
    pub retention_days: u32,                  // 文件/SQLite 保留天数, default: 7
    pub max_db_records: usize,                // SQLite 最大记录数, default: 100_000
}
```

---

## 六、性能考量

| 维度     | 策略                                                                            | 预期指标                |
| -------- | ------------------------------------------------------------------------------- | ----------------------- |
| DB 写入  | `BEGIN TRANSACTION` / `COMMIT` 包裹批量 INSERT，100 条/批                       | ~1-2ms/批（单次 fsync） |
| 自动清理 | 双重策略：时间维度（7 天过期）+ 数量维度（超 10 万 × 1.2 裁剪），每 10 批懒检查 | < 5ms 偶发              |
| 查询     | SQLite 索引 (timestamp, level, target, session_id)                              | < 3ms 常规查询          |
| 文件 IO  | `tracing-appender` 按天滚动 + 启动时清理过期文件                                | 对应用吞吐无影响        |
| 内存     | unbounded channel，最多积累数百条待写入                                         | < 1MB                   |
| 动态调级 | `reload::Layer` + `Handle::modify`，`set_log_level` 命令                        | < 1μs 即时生效          |

---

## 七、日志生命周期管理

### 7.1 stderr — 进程级

```
创建：应用启动，tracing subscriber 初始化
销毁：进程退出 → 操作系统自动回收
无需主动清理
```

### 7.2 文件滚动 — 创建 + 归档 + 清理

```
创建：tracing_appender::rolling::daily() 每天 00:00 创建新文件
归档：app.log → app.YYYY-MM-DD（每天自动重命名）
清理：应用启动时执行 cleanup_log_files()
      ├─ 扫描 {log_dir}/app.YYYY-MM-DD
      ├─ 解析文件名日期
      └─ 删除 日期 < (今天 - retention_days) 的文件

默认保留 7 天，配置项：LogConfig.retention_days
单文件大小：~200KB/天 (INFO 级别，日均 1000 条)
稳态占用：7 × 200KB ≈ 1.4MB
```

### 7.3 SQLite — 时间 + 数量双重策略

```
策略1（时间维度）：
  每次清理检查时无条件执行
  DELETE FROM app_logs WHERE timestamp < (NOW - retention_days 天)
  → 保证日志不会超过 retention_days 天

策略2（数量维度）：
  仅在 COUNT > max_db_records × 1.2 时触发
  DELETE 最旧 N 条，恢复到 max_db_records
  → 防止突发大量写入撑爆磁盘

触发频率：每 10 次 flush 检查一次（CLEANUP_CHECK_INTERVAL）
配置项：
  - LogConfig.retention_days (默认 7)
  - LogConfig.max_db_records (默认 100_000)

典型场景（日均 1000 条）：
  Day 1-7: 增长到 7,000 条，远低于 10 万上限
  Day 8:   策略1 触发，删除 Day1 的 1000 条
  稳态:    每天 ±1000 条进出，维持 ~7,000 条

突发场景（某天 50,000 条）：
  Day 3:   策略2 触发，150,000 > 120,000 → 裁剪到 100,000
  Day 8+:  策略1 逐步清理，回到稳态
```

---

## 八、扩展点

| 方向         | 说明                                                                     |
| ------------ | ------------------------------------------------------------------------ |
| 前端日志面板 | 使用 AG Grid + Naive UI 构建可视化日志浏览器，支持后端分页、自动刷新     |
| 动态级别调整 | 通过 `set_log_level` 命令运行时修改 EnvFilter                            |
| 敏感数据脱敏 | `redact.rs` 自动掩码连接字符串密码、key=value 密码等，可扩展脱敏规则     |
| 远程日志     | 预留 DuckLake 远程持久化接口，支持多用户审计                             |
| 日志上报告警 | ERROR 级别日志可通过 channel 触发前端 toast 通知                         |
| 结构化查询   | `fields` JSON 字段支持按结构化 key-value 查询（如 `duration_ms > 1000`） |

---

## 九、测试

```bash
# 编译检查
cd src-tauri && cargo check

# 运行所有测试（含现有测试）
cargo test

# 仅测试日志模块
cargo test --lib logging
```

### 手动验证

1. 启动应用 → 检查 `{data}/RdataStation/logs/app.{date}` 是否存在
2. 执行任意 SQL → 日志文件应有 `SQL executed` 记录
3. 前端调用 `get_log_stats` → 应返回各级别计数
4. 前端调用 `get_logs` → 应返回 Recent 日志列表
5. 重启应用 → `get_log_session_id` 应变化，`get_logs` 仍可查旧会话日志

---

## 十、变更记录

| 版本 | 日期       | 说明                                                                                                                                |
| ---- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| v1.2 | 2026-05-10 | 改进：添加敏感数据脱敏(redact.rs)、日志面板后端分页+自动刷新、export_logs 支持 max_results 参数、修复静默降级和序列化错误、完善文档 |
| v1.1 | 2026-05-10 | 优化：事务包裹批量写入(4x)、懒清理(90% COUNT减少)、reload handle 动态调级、文件保留清理、双重生命周期策略                           |
| v1.0 | 2026-05-10 | 初始版本，stderr + 文件滚动 + SQLite 持久化                                                                                         |
