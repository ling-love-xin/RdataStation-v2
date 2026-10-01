# 项目管理模块 · 数据字典

> 状态：**2026-10-01 首版**。字段来源：`crates/engine/migrations/` 迁移 SQL + `crates/project/src/models.rs`。
> 本文是 `.RSmeta` 目录树、`project.db` / `global.db` 表结构、`project.json` schema 的**唯一集中说明**；
> 改表结构时请同步本文，并保持与 `project-wiring-matrix.md` 的接线状态一致。

## 1. `.RSmeta` 目录树（项目本体）

创建逻辑见 `store.rs::ProjectStore::create`；锁文件见 `lock.rs`。

```text
{project_path}/
└── .RSmeta/                        # 项目元数据目录（物理隔离，删除项目数据只动这里）
    ├── project.db                  # 项目级 SQLite（SQLite 迁移器 MigrationManager::ProjectMeta 建库）
    ├── project.json                # ★ 权威源（ProjectInfo 序列化；详见 §3）
    ├── analytics.duckdb            # 项目分析库（DuckDB 迁移器 migrate_at_path 建库；魔数断言 DUCK）
    ├── project.lock                # 实例锁（OS 字节锁，进程崩溃自动释放）
    ├── project.lock.owner          # 锁占用者信息（pid / 时间；与锁文件分离，见 lock.rs 注释）
    ├── config/
    │   └── settings.json           # 项目配置（ProjectConfig：默认连接等；就地读写 update_config）
    ├── queries/                    # SQL 文件目录（预留）
    └── project_metadata/           # 连接元数据缓存目录（每连接一个 .db；⚠️ 归属待定，见 wiring-matrix）
```

## 2. `project.db`（SQLite，项目级）

由 `project_meta/` 迁移（001→022）创建。核心表如下。

### 2.1 project（001）—— project.json 的镜像

| 列 | 类型 | 说明 |
| --- | --- | --- |
| id | TEXT PK | 项目 ID |
| name | TEXT NOT NULL | 项目名 |
| description | TEXT | 描述 |
| created_at | TIMESTAMP | 默认 CURRENT_TIMESTAMP |
| updated_at | TIMESTAMP | 默认 CURRENT_TIMESTAMP |

> **权威约定**：`project.json` 是权威，本表是镜像（字段不全于 project.json——无 path / status / last_opened_at）。
> 读优先 JSON，镜像只在 JSON 缺失时兜底；写收敛到 `store::save_info`（JSON 与镜像一并写）。

### 2.2 connections（001）—— 连接配置

| 列 | 类型 | 说明 |
| --- | --- | --- |
| id | TEXT PK | 连接 ID |
| name | TEXT NOT NULL | 连接名 |
| db_type | TEXT NOT NULL | 驱动类型（mysql/postgres/sqlite/duckdb） |
| host / port / database / username | — | 连接参数 |
| password_encrypted | TEXT | 加密密码 |
| options / tags | TEXT | JSON 格式 |
| is_active | BOOLEAN | 默认 1 |
| created_at / updated_at | TIMESTAMP | — |

> ⚠️ **接线现状**：schema 就绪（002 重建过），但项目级连接持久化读取未实现（原 `load_connections` 为 stub，已删）。连接真实落点在**全局库** `connections` 表 + engine 连接管理；本项目 connections 表为 DuckLake 三期预留。

### 2.3 query_history（001）+ sql_history 视图

| 列 | 类型 | 说明 |
| --- | --- | --- |
| id | TEXT PK | 历史 ID |
| connection_id | TEXT FK→connections | 关联连接 |
| database_name / schema_name | TEXT | 执行上下文 |
| sql | TEXT NOT NULL | 执行 SQL |
| sql_hash | TEXT NOT NULL | 去重哈希 |
| exec_mode | TEXT | native / duckdb_fed |
| category | TEXT | query / ddl / dml |
| success | BOOLEAN | 默认 1 |
| error_message / duration_ms / rows_returned / rows_affected | — | 执行统计 |
| is_pinned | BOOLEAN | 固定置顶 |
| created_at | TIMESTAMP | 执行时间 |

索引：`idx_qh_conn` / `idx_qh_time` / `idx_qh_hash` / `idx_qh_category` / `idx_qh_pinned`。
`sql_history` 为兼容视图（字段别名映射旧代码）。

### 2.4 project_settings / workbench_state / 插件表

| 表 | 用途 |
| --- | --- |
| project_settings(key PK, value, updated_at) | 项目级键值设置 |
| workbench_state(id='default', layout, open_panels, active_panel_id, updated_at) | 工作台布局状态 |
| project_used_plugins(plugin_code, plugin_version, enabled, required) | 项目插件引用（预留） |
| project_plugin_config(plugin_code, plugin_version, key, value) | 插件配置（预留） |

### 2.5 project_versions（018）—— 手动快照台账

| 列 | 类型 | 说明 |
| --- | --- | --- |
| id | TEXT PK | 版本 ID |
| parent_id | TEXT | 版本链父级（DuckLake 预留） |
| message | TEXT NOT NULL | 快照说明 |
| created_by | TEXT | 创建者（DuckLake 预留） |
| created_at | TIMESTAMP | 默认 CURRENT_TIMESTAMP |

索引：`idx_project_versions_created(created_at DESC)`。
> **接线现状**：当前仅**手动快照台账**（`service::{list_versions, create_version}`，只记一行 message，不承载数据内容）；
> `Versioned<T>` 内容级版本化 / checksum 未接入（DuckLake 三期，见 wiring-matrix）。

## 3. `project.json`（权威源 schema）

对应 `models::ProjectInfo`（serde 序列化），字段：

```json
{
  "id": "<uuid>",
  "name": "项目名",
  "description": null,
  "path": { "kind": "local", "path": "D:/..." },
  "status": "active",
  "created_at": "<RFC3339>",
  "updated_at": "<RFC3339>",
  "last_opened_at": null,
  "created_by": null,
  "version_count": 0
}
```

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| id | String | 唯一 ID（uuid） |
| name | String | 项目名 |
| description | Option<String> | 描述（空 = 未写） |
| path | ProjectPath | 路径；`Local{path}`（当前），`Remote` 为 DuckLake 预留 |
| status | ProjectStatus | active / archived / syncing / offline（后两者 DuckLake 预留） |
| created_at / updated_at | DateTime<Utc> | 时间戳 |
| last_opened_at | Option<DateTime<Utc>> | 最后打开 |
| created_by | Option<String> | DuckLake 预留 |
| version_count | u32 | 版本快照数 |

> 镜像表 `project.db.project` 只同步 id / name / description / created_at / updated_at（见 §2.1）。

## 4. `global.db` 名册表 `project_info`

全局库 `global/001_init.sql` 建表 + `019_add_project_ui_state.sql` 加列。

| 列 | 类型 | 说明 |
| --- | --- | --- |
| id | TEXT PK | 项目 ID |
| name | TEXT NOT NULL | 项目名 |
| description | TEXT | 描述 |
| path | TEXT NOT NULL | 项目路径 |
| status | TEXT | 默认 'active' |
| created_at / updated_at | TIMESTAMP | 默认 CURRENT_TIMESTAMP |
| last_opened_at | TIMESTAMP | 最后打开（001 基础） |
| is_pinned | INTEGER NOT NULL DEFAULT 0 | 固定置顶（019 加列） |
| pinned_at | TIMESTAMP | 固定时间（019） |
| removed_at | TIMESTAMP | 软删标记（019；磁盘保留） |

索引：`idx_project_info_pinned(is_pinned DESC, last_opened_at DESC)`、`idx_project_info_removed(removed_at)`、`idx_project_info_path(path)`（007 加）。

> 名册是**跨项目可见**的唯一事实源（固定/软删/最后打开）；本体 `project.json` 是项目内权威。两者分离，见 `project-prototype-design.md` §2.1。

## 5. 变更纪律

- 加列用 `ALTER TABLE ... ADD COLUMN`（带默认值，旧行安全），迁移按版本号幂等（`global/019`、`project_meta/*` 均如此）。
- 改表结构后：① 更新本文；② 更新 `wiring-matrix.md` 的接线状态；③ 补迁移 SQL；④ 跑 `cargo test -p rds-engine`（迁移套件）。
