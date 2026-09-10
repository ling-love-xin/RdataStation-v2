# 连接 ID 与元数据缓存懒加载设计

> 版本：v0.6.0  
> 日期：2026-05-22  
> 状态：✅ 已实施

---

## 一、背景

### 原始问题

1. **conn_id 过长且包含文件系统非法字符**：`global-mysql-root:root@127.0.0.1:3306`，在 Windows 上 `:` 导致 SQLite 文件打开失败
2. **测试连接也创建元数据库**：每次 `test_connection` 都创建 `conn_{id}.sqlite` 元数据缓存文件，不必要
3. **元数据缓存过早创建**：连接建立时立即创建，即使用户从不浏览 schema

### 设计目标

- conn_id 短且安全（文件系统兼容）
- 测试连接零副作用（不创建文件）
- 元数据缓存按需创建（首次浏览 schema 时才创建）
- 全局和项目双链路行为一致

---

## 二、conn_id 生成规则

### 旧方案

```
网络数据库：{type}-{db_type}-{url_without_protocol}
  例：global-mysql-root:root@127.0.0.1:3306/db1  ← 太长，含 : @ 非法字符

文件数据库：{type}-{db_type}-{hash(path)}
  例：global-sqlite-a1b2c3d4  ← 短且安全
```

### 新方案（统一）

**所有数据库类型统一使用 URL 哈希**：

```rust
let conn_id = format!("{type_prefix}-{db_type}-{:x}", hash(url));
```

| 连接类型             | 示例 conn_id                                |
| -------------------- | ------------------------------------------- |
| 全局 MySQL           | `global-mysql-a1b2c3d4`                     |
| 项目 MySQL（同 URL） | `project-mysql-a1b2c3d4` ← type_prefix 区分 |
| 全局 SQLite          | `global-sqlite-e5f6a7b8`                    |
| 全局 DuckDB          | `global-duckdb-9c0d1e2f`                    |

### 显示名称

用户指定名称优先，无名称时自动生成简洁格式：

```
用户指定：name = "生产库" → 显示 "生产库"
自动生成：MYSQL@127.0.0.1、POSTGRES@localhost
测试连接：test_connection（临时，用完释放）
```

### 唯一性保证

- **同一服务不同数据库**：`mysql://root@host:3306/db1` 和 `mysql://root@host:3306/db2` URL 不同 → hash 不同 → conn_id 不同
- **同一 URL 全局/项目**：type_prefix 不同 → `global-mysql-XXXX` ≠ `project-mysql-XXXX`
- **Hash 碰撞概率**：64-bit hash，冲突概率 < 10^-18，可接受

---

## 三、元数据缓存懒加载

### 旧流程（Eager）

```
connect_database(test_connection)
  └── connect_with_type()
        ├── create_database()
        ├── add_connection()
        └── initialize_connection_metadata()  ← 立即创建 conn_{id}.sqlite
              └── MetadataCacheManager::open()
```

问题：测试连接也触发文件创建，连接成功但文件路径非法时崩溃。

### 新流程（Lazy）

```
connect_database(test_connection)
  └── connect_with_type()
        ├── create_database()
        └── add_connection()
        // 不再调用 initialize_connection_metadata

---

[用户首次展开数据库树]
load_databases(conn_id, connection_type, project_path)
  ├── L1 内存缓存 → hit? 返回
  ├── L2 磁盘缓存 → 调用 open_l2_cache()
  │     └── mgr.exists()? → 否 → 返回 err
  └── DB 直接查询 → list_catalogs()
        └── 写回 L1 + L2（open_l2_cache_for_write 自动创建文件）
```

### 关键代码路径

| 操作         | 路径                                     | 文件是否存在 | 行为                          |
| ------------ | ---------------------------------------- | ------------ | ----------------------------- |
| **连接**     | `connect_with_type`                      | 不关心       | 不创建                        |
| **读缓存**   | `open_l2_cache` → `mgr.exists()`         | 否           | 返回 err，调用者 fall through |
| **读缓存**   | `open_l2_cache` → `mgr.exists()`         | 是           | 打开并读取                    |
| **写缓存**   | `open_l2_cache_for_write` → `mgr.open()` | 否           | rusqlite 自动创建             |
| **写缓存**   | `open_l2_cache_for_write` → `mgr.open()` | 是           | 打开并写入                    |
| **手动预热** | `build_cache_index` → `mgr.open()`       | 任意         | 创建或打开                    |
| **显式确保** | `ensure_metadata_cache()`                | 否           | 创建 + 迁移                   |
| **显式确保** | `ensure_metadata_cache()`                | 是           | 跳过（幂等）                  |

### 双链路一致性

全局连接和项目连接的元数据缓存路径不同，但懒加载逻辑完全一致：

| 连接类型 | 缓存路径                                                   |
| -------- | ---------------------------------------------------------- |
| **全局** | `{data_dir}/system/global_metadata/conn_{id}.sqlite`       |
| **项目** | `{project_path}/meta/connection_metadata/conn_{id}.sqlite` |

两者都通过 `open_l2_cache` / `open_l2_cache_for_write` 走相同的懒加载路径。

---

## 四、修改文件清单

| 文件                    | 变更                                                                                                       |
| ----------------------- | ---------------------------------------------------------------------------------------------------------- |
| `connection_service.rs` | conn_id 统一 hash 生成；移除 eager `initialize_connection_metadata`；新增 `ensure_metadata_cache` 公开方法 |
| `metadata_cache.rs`     | `sanitize_conn_id_for_filename` 安全网（hash 已天然安全，保留以防自定义 conn_id）                          |
| `04-data-flow.md`       | 更新连接流程文档                                                                                           |

### 未修改的文件（已有懒加载逻辑）

- `metadata_commands.rs`：`open_l2_cache` 读路径已有 `exists()` 检查 → 返回 err 则 fall through
- `cache_warming_commands.rs`：`build_cache_index` 直接 `mgr.open()` → 自动创建
- `metadata_cache_commands.rs`：CRUD 操作同走 `open_l2_cache_for_write`

---

## 五、迁移注意事项

旧版 `conn_id` 格式为 `global-mysql-root:root@127.0.0.1:3306/db1`，新版为 `global-mysql-a1b2c3d4`。

### 影响

1. **已保存的全局连接**（`global_db` 中的 `connections` 表）：conn_id 字段值会改变，如果已有旧版数据，连接信息将不再匹配
2. **元数据缓存文件**：旧版文件名包含特殊字符（如 `conn_global-mysql-root_root_at_127.0.0.1_3306.sqlite`），新版为 `conn_global-mysql-a1b2c3d4.sqlite`
3. **连接管理器内存状态**：重建连接时 conn_id 会变化，旧连接不会自动复用

### 建议

- 如果已有旧版数据，删除 `system/global_metadata/` 下旧格式的 `.sqlite` 文件
- 重新创建连接，让系统自动生成新版 hash-based conn_id
- 这是 v0.x 阶段的设计变更，后续稳定版前完成即可
