# DuckDB 分析引擎架构设计文档

> 版本: v2.0
> 创建日期: 2026-05-12
> 最后更新: 2026-05-12
> 状态: 已实现，合规性已验证

---

## 一、系统概述

DuckDB 分析引擎是 RdataStation 项目中的本地分析核心，提供连接池管理、SQL执行、临时表管理、联邦查询、数据导入导出、全文搜索、查询计划分析、插件系统接口、DuckDB 扩展管理、性能监控和快照备份等完整功能。

### 1.1 定位

- **模块路径**: `core/duckdb/`
- **职责**: DuckDB 本地分析引擎（全局/项目双层连接池）
- **驱动实现**: `core/driver/native/duckdb.rs`（实现 Database trait）
- **区别于**: `core/driver/native/duckdb.rs`（网络数据库驱动，实现 Database trait）

### 1.2 设计原则

1. **读写分离**: 1 写入连接 + N 读取连接 + 1 维护连接
2. **统一执行**: 所有 SQL 通过 `DuckDBExecutor` 执行，禁止直连
3. **模块化**: 11 个子模块职责清晰，易于维护扩展
4. **可插拔**: 支持多实例，路径可配置
5. **零拷贝**: 使用 Arrow RecordBatch 进行数据传输

---

## 二、系统架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                        业务层调用                                  │
│   分析工作台 │ 洞察面板 │ Mock生成器 │ 结果集面板 │ 联邦查询入口     │
└──────────────────────────────┬──────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                  core/duckdb/ (模块入口)                          │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │ manager.rs   │  │ executor.rs  │  │   temp_table.rs      │   │
│  │ 连接池管理    │  │ SQL统一执行   │  │   临时表管理          │   │
│  │              │  │              │  │                      │   │
│  │ - write_conn │  │ - execute_   │  │ - generate_name()    │   │
│  │ - read_conn  │  │   read()     │  │ - register()         │   │
│  │ - maint_conn │  │ - execute_   │  │ - lazy_cleanup()     │   │
│  └──────┬───────┘  │   write()    │  └──────────────────────┘   │
│         │          └──────┬───────┘                              │
│         │                 │                                      │
│         ▼                 ▼                                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │federation.rs │  │import_export.│  │      fts.rs          │   │
│  │ 联邦查询      │  │  导入导出     │  │   全文搜索            │   │
│  │              │  │              │  │                      │   │
│  │ - attach()   │  │ - generate_  │  │ - generate_create_   │   │
│  │ - detach()   │  │   import_*   │  │   index_sql()        │   │
│  │ - list()     │  │ - generate_  │  │ - generate_search_   │   │
│  └──────────────┘  │   export_*   │  │   sql()              │   │
│                    └──────────────┘  └──────────────────────┘   │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │ explain.rs   │  │  plugin.rs   │  │   extensions.rs      │   │
│  │ 查询计划      │  │ 插件接口      │  │   扩展管理            │   │
│  │              │  │              │  │                      │   │
│  │ - generate_  │  │ - create_    │  │ - discover()         │   │
│  │   explain()  │  │   plugin_    │  │ - install()          │   │
│  │ - parse()    │  │   conn()     │  │ - load()             │   │
│  │ - suggest()  │  │ - validate() │  │ - unload()           │   │
│  └──────────────┘  └──────────────┘  └──────────────────────┘   │
│                                                                 │
│  ┌──────────────┐  ┌──────────────────┐                         │
│  │ metrics.rs   │  │   snapshot.rs    │                         │
│  │ 性能监控      │  │   快照与备份      │                         │
│  │              │  │                  │                         │
│  │ - 查询统计   │  │ - create_        │                         │
│  │ - 连接追踪   │  │   snapshot()     │                         │
│  │ - 错误计数   │  │ - restore()      │                         │
│  └──────────────┘  └──────────────────┘                         │
└─────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                    DuckDB 文件实例                                │
│                    (~/.rdatastation/*.duckdb)                     │
└─────────────────────────────────────────────────────────────────┘
```

---

## 三、模块详细设计

### 3.1 DuckDBManager（连接池管理）

**职责**: 管理 DuckDB 文件实例的连接池

**结构**:

```rust
pub struct DuckDBManager {
    db_path: PathBuf,              // 数据库文件路径
    write_conn: Connection,        // 写入连接（1个，独占）
    read_pool: Vec<Connection>,    // 读取连接池（默认4个）
    maintenance_conn: Connection,  // 维护连接（1个，独立）
    read_index: AtomicUsize,       // 轮询索引
}
```

**连接分配策略**:

- **写入连接**: 固定 1 个，DuckDB 单写入者模型
- **读取连接**: 默认 4 个，Round-Robin 轮询
- **维护连接**: 独立 1 个，用于 TTL 清理等后台任务

### 3.2 DuckDBExecutor（SQL执行器）

**职责**: 统一 SQL 执行接口，屏蔽底层连接细节

**接口**:

- `execute_read(sql)` - 只读查询
- `execute_write(sql)` - 写入操作
- `execute_read_with_params(sql, params)` - 参数化只读查询
- `execute_write_with_params(sql, params)` - 参数化写入
- `execute_batch(sql)` - 批量执行
- `execute_transaction(sql)` - 事务执行

### 3.3 TempTableManager（临时表管理）

**职责**: 临时表命名、TTL 清理、数量上限管理

**命名规则**: `tmp_{来源缩写}_{描述}_{时间戳}`

| 来源     | 缩写 | TTL    | 数量上限 |
| -------- | ---- | ------ | -------- |
| 查询结果 | q    | 无     | 无       |
| 洞察中间 | i    | 30分钟 | 100      |
| Mock数据 | m    | 无     | 无       |
| 插件临时 | p    | 无     | 无       |

### 3.4 FederationManager（联邦查询）

**职责**: ATTACH 外部数据源，实现跨库查询

**支持数据源**:

- DuckDB 文件
- MySQL 数据库
- PostgreSQL 数据库
- SQLite 数据库

**单向规则**: 仅项目 ATTACH 全局，全局永不挂载项目库

### 3.5 ImportExportManager（导入导出）

**职责**: 数据导入导出 SQL 生成

**导入格式**: CSV、Parquet、JSON
**导出格式**: CSV、Parquet、JSON

### 3.6 FTSManager（全文搜索）

**职责**: FTS 索引维护与查询 SQL 生成

**功能**:

- 创建 FTS 索引
- 全文搜索查询
- 索引重建
- 索引存在性检查

### 3.7 ExplainAnalyzer（查询计划分析）

**职责**: EXPLAIN 查询计划解析与性能建议

**功能**:

- 生成 EXPLAIN SQL
- 解析执行计划
- 检测全表扫描
- 检测嵌套循环连接
- 性能优化建议

### 3.8 PluginManager（插件系统接口）

**职责**: 为 Extism WASM + Go Sidecar 插件提供安全沙箱式 DuckDB 访问

**权限级别**:
| 级别 | 适用插件 | 权限 |
|------|---------|------|
| 只读 | SQL格式化、数据脱敏、Schema Diff | 仅 SELECT |
| 读写 | 数据导入导出、Mock增强 | 可创建临时表 |
| 管理 | 官方内置插件（JDBC Bridge） | 支持联邦 ATTACH |

### 3.9 ExtensionManager（扩展管理）

**职责**: DuckDB 扩展的发现、安装、加载、卸载、状态查询

**扩展分类**:

- **内置扩展**: parquet、json（启动自动加载）
- **按需扩展**: spatial、excel、httpfs、fts、mysql/postgres（首次使用自动安装）

---

## 四、依赖关系

```
core/duckdb/ 可调用 core/sql/ (SQL解析检测)
反向不依赖: core/sql/ 不依赖 core/duckdb/
core/duckdb/ 依赖 core/driver/ (Database trait)
core/services/ 依赖 core/duckdb/ (分析引擎接口)
```

**外部依赖**:

- duckdb-rs: 1.10502.0
- arrow: 58.1.0 (零拷贝数据传输)
- tracing: 0.1 (日志)
- chrono: 0.4 (时间戳)
- serde: 1.0 (序列化)

---

## 五、错误处理

所有模块统一使用 `CoreError` 错误类型，结合 `CommonError::General` 传递错误信息。

**禁止**:

- ❌ 生产代码中使用 `unwrap()/expect()`
- ❌ 直接暴露内部实现细节

**特殊情况说明**:

- `DuckDBManager::global()` 方法使用 `unwrap_or_else(panic!)` 处理初始化失败
- 原因：全局分析引擎是核心组件，不可降级运行，初始化失败意味着系统不可用
- 文档已明确说明此语义

**示例**:

```rust
let conn = Connection::open(path).map_err(|e| {
    CoreError::common(CommonError::General(format!(
        "创建 DuckDB 连接失败: {}",
        e
    )))
})?;
```

---

## 六、性能指标

| 指标                   | 目标           | 当前状态      |
| ---------------------- | -------------- | ------------- |
| 连接池分配延迟         | < 1ms          | ✅ 轮询 O(1)  |
| 临时表创建延迟         | < 5ms          | ✅ 惰性清理   |
| TTL清理耗时            | < 10ms (100表) | ✅ 哈希表扫描 |
| Arrow RecordBatch 转换 | 零拷贝         | ✅ 直接传递   |

---

## 七、安全要求

| 检查项      | 要求               |
| ----------- | ------------------ |
| SQL注入防护 | 参数化查询支持     |
| 权限控制    | 插件权限级别校验   |
| 扩展签名    | 仅加载签名扩展     |
| 错误信息    | 不暴露内部实现细节 |
| unsafe 代码 | 0 处               |

---

## 八、合规性报告

### 8.1 架构红线检查

| 检查项        | 状态    | 说明                                                              |
| ------------- | ------- | ----------------------------------------------------------------- |
| 循环依赖      | ✅ 通过 | duckdb 模块只依赖 core/driver 和 core/sql，无反向依赖             |
| 层级越界      | ✅ 通过 | services 层通过 core/duckdb 组件访问，未直接调用 datasource       |
| Trait 修改    | ✅ 通过 | driver/traits.rs 未被修改，DuckDbDatabase 完整实现 Database trait |
| DuckDB 可插拔 | ✅ 通过 | 通过 driver::traits 抽象，DuckDB 不是唯一引擎                     |

### 8.2 数据契约检查

| 检查项     | 状态    | 说明                                              |
| ---------- | ------- | ------------------------------------------------- |
| Arrow 传输 | ✅ 通过 | DuckDBResult 内部使用 `batches: Vec<RecordBatch>` |
| 零拷贝     | ✅ 通过 | Arrow RecordBatch 直接传递，无 Row→JSON 转换      |
| IPC 完整性 | ✅ 通过 | Tauri Commands 只调用 service，不做数据转换       |

### 8.3 测试规范检查

| 检查项           | 状态      | 说明                                                 |
| ---------------- | --------- | ---------------------------------------------------- |
| mod.rs 不含测试  | ✅ 通过   | duckdb/mod.rs 只有声明和重新导出                     |
| 私有方法测试内嵌 | ✅ 通过   | 测试代码在各源文件底部 `#[cfg(test)] mod tests` 块中 |
| 测试命名规范     | ✅ 通过   | 使用 `test_<功能描述>` 格式                          |
| 集成测试隔离     | ✅ 已改进 | MySQL/PostgreSQL 测试已标记 `#[ignore]`              |

---

## 九、版本历史

| 版本 | 日期       | 说明                                                    |
| ---- | ---------- | ------------------------------------------------------- |
| v1.0 | 2026-05-12 | 初始架构设计文档                                        |
| v2.0 | 2026-05-12 | 架构更新：模块重命名、新增 metrics/snapshot、合规性验证 |
