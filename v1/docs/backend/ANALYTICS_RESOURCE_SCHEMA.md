# 分析资源模块 — 后端 Schema 设计文档

> 版本：v1.5
> 最后更新：2026-05-10
> 状态：✅ P0/P1/P2 优化完成

---

## 一、完整 ER 图

```
┌───────────────────────────┐      ┌─────────────────────────────┐      ┌───────────────────────────┐
│   analytics_resources     │ 1──N │ analytics_resource_folder   │ N──1 │    analytics_folders      │
│───────────────────────────│      │─────────────────────────────│      │───────────────────────────│
│ id (TEXT PK)              │      │ resource_id + folder_id (PK) │      │ id (TEXT PK)              │
│ resource_type (TEXT)      │      │ sort_order (INTEGER)         │      │ name (TEXT NOT NULL)      │
│ name (TEXT NOT NULL)      │      └─────────────────────────────┘      │ scope (CHECK IN ...)      │
│ alias (TEXT)              │                                           │ parent_folder_id (FK自引用)│
│ config (JSON CHECK)       │                                           │ sort_order / color / icon │
│ scope (CHECK IN ...)      │                                           │ created_at / updated_at   │
│ row_count / column_count  │                                           │ deleted_at                │
│ file_size (INTEGER)       │                                           └───────────────────────────┘
│ version (INTEGER)         │                                                    │
│ parent_version_id (TEXT)  │      ┌─────────────────────────────┐      ┌───────────────────────────┐
│ parent_resource_id (FK)─┐ │ 1──N │  analytics_resource_tags   │ N──1 │     analytics_tags        │
│ source_query (TEXT)     │ │      │─────────────────────────────│      │───────────────────────────│
│ created_at / updated_at │ │      │ resource_id + tag_id (PK)   │      │ id (TEXT PK)              │
│ created_by / deleted_at │ │      └─────────────────────────────┘      │ name (TEXT NOT NULL)      │
└──────────┬──────────────┘ │                                           │ color / icon              │
           │                │                                           │ scope (CHECK IN ...)      │
           │ 1──N           │                                           │ created_at / deleted_at   │
           ▼                │                                           └───────────────────────────┘
┌───────────────────────────┐│
│ analytics_resource_versions│
│───────────────────────────││
│ id (TEXT PK)              ││
│ resource_id (FK)──────────┘│
│ version (INTEGER)          │
│ snapshot (JSON)            │
│ created_at (TIMESTAMP)     │
│ UNIQUE(resource_id, ver)   │
└───────────────────────────┘

软删除时快照复制
┌───────────────────────────┐
│  analytics_recycle_bin    │
│───────────────────────────│
│ id (TEXT PK)              │
│ resource_id / type / name │
│ resource_data (JSON快照)  │
│ deleted_by / deleted_at   │
└───────────────────────────┘
```

---

## 二、表结构详细定义

### 2.1 analytics_resources（资源主表）

| 列名               | 类型      | 约束                                                       | 说明                      |
| ------------------ | --------- | ---------------------------------------------------------- | ------------------------- |
| id                 | TEXT      | PRIMARY KEY                                                | 格式: `ar_{uuid}`         |
| resource_type      | TEXT      | NOT NULL                                                   | connection / table / file |
| name               | TEXT      | NOT NULL                                                   | 显示名称                  |
| alias              | TEXT      |                                                            | 用户自定义别名            |
| config             | TEXT      | NOT NULL, `CHECK(json_valid(config))`                      | JSON 配置                 |
| scope              | TEXT      | NOT NULL, `CHECK(scope IN ('global','project','session'))` | 作用域                    |
| row_count          | INTEGER   |                                                            | 表/视图行数               |
| column_count       | INTEGER   |                                                            | 表/视图列数               |
| file_size          | INTEGER   |                                                            | 文件大小(字节)            |
| version            | INTEGER   | DEFAULT 1                                                  | 版本号                    |
| parent_version_id  | TEXT      |                                                            | 上一版本资源 ID           |
| parent_resource_id | TEXT      | FK → analytics_resources(id)                               | 克隆来源                  |
| source_query       | TEXT      |                                                            | 数据来源 SQL              |
| created_at         | TIMESTAMP | DEFAULT CURRENT_TIMESTAMP                                  | 创建时间                  |
| updated_at         | TIMESTAMP | DEFAULT CURRENT_TIMESTAMP, 触发器自动更新                  | 更新时间                  |
| created_by         | TEXT      |                                                            | 创建者                    |
| deleted_at         | TIMESTAMP |                                                            | 软删除时间                |

**索引**:

- `idx_ar_type` ON (resource_type)
- `idx_ar_scope` ON (scope)
- `idx_ar_deleted` ON (deleted_at)
- `idx_ar_name` ON (name)

**触发器**:

- `trg_ar_updated_at`: 业务列变更时自动更新 `updated_at`

### 2.2 analytics_resource_versions（版本历史表） 🆕

| 列名        | 类型      | 约束                                   | 说明                       |
| ----------- | --------- | -------------------------------------- | -------------------------- |
| id          | TEXT      | PRIMARY KEY                            | 格式: `arv_{uuid}`         |
| resource_id | TEXT      | NOT NULL, FK → analytics_resources(id) | 资源 ID                    |
| version     | INTEGER   | NOT NULL                               | 版本号                     |
| snapshot    | TEXT      | NOT NULL                               | 该版本的完整资源 JSON 快照 |
| created_at  | TIMESTAMP | DEFAULT CURRENT_TIMESTAMP              | 快照创建时间               |

**索引**:

- `idx_arv_resource` ON (resource_id)
- `idx_arv_version` ON (resource_id, version DESC)

**唯一约束**: `UNIQUE(resource_id, version)`

### 2.3 analytics_folders（文件夹表）

| 列名             | 类型      | 约束                                                       | 说明                 |
| ---------------- | --------- | ---------------------------------------------------------- | -------------------- |
| id               | TEXT      | PRIMARY KEY                                                | 格式: `af_{uuid}`    |
| name             | TEXT      | NOT NULL                                                   | 文件夹名称           |
| scope            | TEXT      | NOT NULL, `CHECK(scope IN ('global','project','session'))` | 作用域               |
| parent_folder_id | TEXT      | FK → analytics_folders(id)                                 | 父文件夹（树形结构） |
| sort_order       | INTEGER   | DEFAULT 0                                                  | 排序                 |
| color            | TEXT      |                                                            | 颜色标识             |
| icon             | TEXT      |                                                            | 图标                 |
| created_at       | TIMESTAMP | DEFAULT CURRENT_TIMESTAMP                                  |                      |
| updated_at       | TIMESTAMP | 触发器自动更新                                             |                      |
| deleted_at       | TIMESTAMP |                                                            | 软删除               |

### 2.4 analytics_resource_folder（资源-文件夹关联）

| 列名        | 类型    | 约束                             |
| ----------- | ------- | -------------------------------- |
| resource_id | TEXT    | PK, FK → analytics_resources(id) |
| folder_id   | TEXT    | PK, FK → analytics_folders(id)   |
| sort_order  | INTEGER | DEFAULT 0                        |

### 2.5 analytics_tags（标签表）

| 列名       | 类型      | 约束                                             | 说明              |
| ---------- | --------- | ------------------------------------------------ | ----------------- |
| id         | TEXT      | PRIMARY KEY                                      | 格式: `at_{uuid}` |
| name       | TEXT      | NOT NULL                                         | 标签名            |
| color      | TEXT      |                                                  |                   |
| icon       | TEXT      |                                                  |                   |
| scope      | TEXT      | NOT NULL, `CHECK(scope IN ('global','project'))` |                   |
| created_at | TIMESTAMP |                                                  |                   |
| deleted_at | TIMESTAMP |                                                  |                   |

**唯一索引**: `idx_at_name_scope` ON (name, scope) WHERE deleted_at IS NULL

### 2.6 analytics_resource_tags（资源-标签关联）

| 列名        | 类型 | 约束                             |
| ----------- | ---- | -------------------------------- |
| resource_id | TEXT | PK, FK → analytics_resources(id) |
| tag_id      | TEXT | PK, FK → analytics_tags(id)      |

### 2.7 analytics_recycle_bin（回收站）

| 列名          | 类型      | 约束                      | 说明                       |
| ------------- | --------- | ------------------------- | -------------------------- |
| id            | TEXT      | PRIMARY KEY               | 格式: `rb_{uuid}`          |
| resource_id   | TEXT      | NOT NULL                  | 原始资源 ID                |
| resource_type | TEXT      | NOT NULL                  |                            |
| resource_name | TEXT      | NOT NULL                  |                            |
| resource_data | TEXT      | NOT NULL                  | 删除时的完整资源 JSON 快照 |
| deleted_by    | TEXT      |                           |                            |
| deleted_at    | TIMESTAMP | DEFAULT CURRENT_TIMESTAMP |                            |

---

## 三、新增 CHECK 约束

```sql
-- 资源表
config TEXT NOT NULL CHECK (json_valid(config))
scope  TEXT NOT NULL CHECK (scope IN ('global', 'project', 'session'))

-- 文件夹表
scope  TEXT NOT NULL CHECK (scope IN ('global', 'project', 'session'))

-- 标签表
scope  TEXT NOT NULL CHECK (scope IN ('global', 'project'))
```

---

## 四、新增触发器

```sql
-- 资源表：业务列变更时自动更新 updated_at
CREATE TRIGGER IF NOT EXISTS trg_ar_updated_at
    AFTER UPDATE OF name, alias, config, scope, row_count, column_count,
                   file_size, version, parent_version_id,
                   parent_resource_id, source_query, deleted_at
    ON analytics_resources
BEGIN
    UPDATE analytics_resources SET updated_at = CURRENT_TIMESTAMP
    WHERE id = NEW.id;
END;

-- 文件夹表：同理
CREATE TRIGGER IF NOT EXISTS trg_af_updated_at
    AFTER UPDATE OF name, scope, parent_folder_id, sort_order,
                   color, icon, deleted_at
    ON analytics_folders
BEGIN
    UPDATE analytics_folders SET updated_at = CURRENT_TIMESTAMP
    WHERE id = NEW.id;
END;
```

---

## 五、新增 API（v1.3）

### 5.1 版本历史

| Command                        | 说明             | 输入                  | 输出                   |
| ------------------------------ | ---------------- | --------------------- | ---------------------- |
| `get_resource_versions`        | 获取资源版本历史 | `resource_id: String` | `Vec<ResourceVersion>` |
| `save_resource_version` (内部) | 保存版本快照     | 更新资源时自动调用    | -                      |

**触发时机**：`update_resource` 方法在更新前自动保存当前版本快照。

### 5.2 标签双向查询

| Command                 | 说明                   | 输入                  | 输出                     |
| ----------------------- | ---------------------- | --------------------- | ------------------------ |
| `get_tags_for_resource` | 获取资源的标签列表     | `resource_id: String` | `Vec<AnalyticsTag>`      |
| `get_resources_by_tag`  | 获取标签关联的资源列表 | `tag_id: String`      | `Vec<AnalyticsResource>` |

---

## 六、删除时的关联表清理（v1.2）

删除资源时（`delete_resource` 和 `batch_delete_resources`），现在会同步清理：

```sql
DELETE FROM analytics_resource_folder WHERE resource_id = ?;
DELETE FROM analytics_resource_tags WHERE resource_id = ?;
```

保证数据一致性，避免软删除后的关联残留。

---

## 七、v1.1 ~ v1.3 累积变更记录

### 迁移文件变更

| 变更          | 说明                                     |
| ------------- | ---------------------------------------- |
| ✅ CHECK 约束 | `config` 列增加 `json_valid()` 校验      |
| ✅ CHECK 约束 | `scope` 列增加 `IN (...)` 枚举约束       |
| ✅ 新表       | `analytics_resource_versions` 版本历史表 |
| ✅ 触发器     | `trg_ar_updated_at` 资源自动更新时间     |
| ✅ 触发器     | `trg_af_updated_at` 文件夹自动更新时间   |

### Rust Store 变更

| 变更          | 说明                                                                           |
| ------------- | ------------------------------------------------------------------------------ |
| ✅ 统一参数化 | `list_resources/list_folders/list_tags` 统一使用 `Vec<rusqlite::types::Value>` |
| ✅ 修复编译   | `parse_datetime_sqlite` 绕过 CoreError 转换，直接 parse                        |
| ✅ 事务恢复   | `restore_from_recycle` 使用 BEGIN/COMMIT/ROLLBACK                              |
| ✅ 关联清理   | 删除资源时清理 `resource_folder` 和 `resource_tags`                            |
| ✅ 版本快照   | `update_resource` 前自动保存版本快照                                           |
| ✅ 标签查询   | `get_tags_for_resource` + `get_resources_by_tag`                               |

### Command 层变更

| 变更      | 说明                    |
| --------- | ----------------------- |
| ✅ 新命令 | `get_resource_versions` |
| ✅ 新命令 | `get_tags_for_resource` |
| ✅ 新命令 | `get_resources_by_tag`  |

---

## 九、v1.4 变更记录

### Command 层变更

| 变更      | 说明                |
| --------- | ------------------- |
| ✅ 新命令 | `get_analytics_tag` |

### Rust Store 变更

| 变更        | 说明                                                   |
| ----------- | ------------------------------------------------------ |
| ✅ 错误处理 | `parse_datetime_sqlite` 返回 CoreError 替代 raw Error  |
| ✅ 日志增强 | `unwrap_or(1/20)` 替换为 `unwrap_or_else` + trace 日志 |

### 前端变更

| 变更            | 说明                                                   |
| --------------- | ------------------------------------------------------ |
| ✅ version 修复 | extension.ts 版本号从 1.0.0 → 1.4.0，修复 API 接口定义 |
| ✅ 新命令支持   | `get_analytics_tag` 前端 API + Store 方法              |
| ✅ CSS 语义变量 | tokens.css 新增 15 个资源类型/范围语义色变量           |
| ✅ 代码质量     | 清理未使用变量（4 个组件）、修复非空断言（2 个组件）   |

---

## 十、文件清单

```
src-tauri/
├── migrations/project_meta/
│   └── 007_analytics_resources.sql          # Schema 定义（v1.4 更新）
├── src/
│   ├── core/persistence/
│   │   └── analytics_resource_store.rs       # Store 实现（v1.4 更新）
│   ├── commands/
│   │   └── analytics_resource_commands.rs    # Tauri 命令（v1.4 更新）
│   └── lib.rs                                 # 命令注册（v1.4 更新）
```
