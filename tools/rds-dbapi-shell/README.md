# tools/rds-dbapi-shell — Python 通用 DB-API 2.0 驱动壳

与 `tools/rds-jdbc-shell`（Java）同构的 sidecar，面向 Python 生态驱动
（psycopg2/psycopg、mysql-connector、oracledb 等）。v1 用标准库 `sqlite3` 验证全链路。

## 定位

- **进程形态**：rds-driver/1 sidecar（走 `supervisor` 标准轨，协议/生命周期与 Java 壳完全一致）
- **语言/运行时**：Python 3.10+，**不内嵌 Python 环境**。解释器由宿主 `runtime.rs` 三级查找
  （用户指定 → PATH 的 python3/python → `third_party/python`，见 `crates/plugin/src/runtime.rs`）
- **驱动**：DB-API 2.0（PEP 249）——`connect()` → `cursor().execute()` → `description`/`rowcount`

## 文件

```
tools/rds-dbapi-shell/
├── rds_dbapi_shell.py   # 单文件壳（协议 + 帧 + DB-API 适配 + 主循环）
└── smoke.ps1            # 全链路冒烟（sqlite3 内存库，13 项断言）
```

## 验证

```powershell
powershell -ExecutionPolicy Bypass -File smoke.ps1
```

覆盖：initialize 握手 → session.open → describe → DML/查询 → meta.schemas/objects →
参数化查询 → **真取消**（递归 CTE 大聚合阻塞 worker，`Connection.interrupt()` 打断 →
execute 回 -32004）→ session.close → shutdown。

## 与 Java 壳的差异（仅实现层）

| 维度 | rds-jdbc-shell | rds-dbapi-shell |
| --- | --- | --- |
| 连接 | URLClassLoader 驱动实例直连（绕过 DriverManager 陷阱） | DB-API connect（sqlite3 无认证） |
| cancel | Statement.cancel() | Connection.interrupt()（线程安全） |
| meta | DatabaseMetaData 标准接口 | PRAGMA + sqlite_master（SQLite 特化） |
| JSON | 手写极简 Json（无依赖） | 标准库 json |
| 多语句 | 驱动决定（H2 支持） | sqlite3 一次一条（execute） |

## 连接配置 → 数据库路径

`url_override` 优先 → `url_template` 模板替换（`{file}/{host}/{port}/{database}/{user}/{password}`）→
`driver=="sqlite"` 用 `file_path` / `:memory:`。

## 已知限制（v1）

- 内联结果截断（200 行 / 256 KiB）；`query.fetch`/Arrow 第二期
- SQLite 特化的 meta 查询（catalogs 恒空、schema=main/temp）；真实 PG 插件（#39）会扩展
  information_schema 查询
- 真取消依赖驱动的 interrupt 支持（sqlite3 已实证；psycopg 需 `cancel()`，见 #39 插件）
