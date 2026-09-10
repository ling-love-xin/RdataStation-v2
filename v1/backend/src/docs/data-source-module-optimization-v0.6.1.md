# 数据源模块优化记录 v0.6.1

> 日期：2026-05-23  
> 状态：✅ 已完成  
> 基于：[数据源模块三阶段分析](./data-source-module-analysis.md)

---

## 一、问题清单与修复

| #     | 问题                                | 严重度 | 状态            | 修复内容                                                                                                                        |
| ----- | ----------------------------------- | ------ | --------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| B1/B2 | 持久化 ID vs runtime conn_id 不匹配 | 高     | ✅ 已验证无问题 | `save_global_connection` 用 `INSERT OR REPLACE` 写入 `id=conn_id(hash)`，`get_global_connections` SELECT 同一字段返回，全程一致 |
| B3    | ConnectDatabaseInput 缺少 conn_id   | 高     | ✅ 已修复       | 前后端均增加 `conn_id` 字段；`AddDataSourceDialog.doSave` 传递返回的 conn_id                                                    |
| D1    | Mock 数据回退                       | 高     | ✅ 已修复       | `navigator-loader.ts` 改为真实 `invoke('execute_sql')`；删除 2 个 mock 文件                                                     |
| C1    | 缓存无自动失效                      | 高     | ✅ 已修复       | 后端 `CACHE_TTL_SECS=300` 过期检测；`refreshAllMetadata` 清除缓存后重载                                                         |
| C3    | 错误静默回退 default catalog        | 中     | ✅ 已修复       | `loadCatalogs` catch 改为 `throw e`，错误信息通过 `error` ref 展示                                                              |
| A5    | Scope 切换无警告                    | 中     | ✅ 已修复       | `watch scope` 检测非空配置时显示 `NAlert` 警告                                                                                  |
| B4    | RecentConnection 缺 conn_id         | 中     | ✅ 已修复       | `ConnectionInfo` 结构体新增 `conn_id` 字段                                                                                      |

---

## 二、变更文件清单

### 后端 (Rust)

| 文件                                                 | 变更                                                                                   |
| ---------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `src-tauri/src/commands/connection_commands.rs`      | `ConnectDatabaseInput` 新增 `conn_id` 字段，传递到 `connect_with_type`                 |
| `src-tauri/src/commands/metadata_cache_commands.rs`  | `get_metadata_cache_status` 新增 TTL 过期检测；`clear_metadata_cache` 物理删除缓存文件 |
| `src-tauri/src/core/persistence/connection_store.rs` | `ConnectionInfo` + `RecentConnectionInput` 新增 `conn_id` 字段                         |
| `src-tauri/src/core/services/connection_service.rs`  | `save_recent_connection` 传递 `conn_id`                                                |

### 前端 (Vue/TS)

| 文件                                                                      | 变更                                              |
| ------------------------------------------------------------------------- | ------------------------------------------------- |
| `src/extensions/builtin/connection/ui/services/connection.ts`             | `connectDatabase` opts 新增 `connId` 参数         |
| `src/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue` | `doSave` 传递 conn_id；新增 scope 切换警告        |
| `src/extensions/builtin/database/domain/services/navigator-loader.ts`     | 移除 mock import，`loadNodeChildren` 改用真实 API |
| `src/extensions/builtin/database/domain/index.ts`                         | 移除 `loadMockChildren` re-export                 |
| `src/extensions/builtin/database/ui/stores/database-navigator-store.ts`   | `loadCatalogs` 错误改为 throw                     |
| `src/shared/locales/zh-CN.json`                                           | 新增 `navigator.scopeChangeWarning` key           |

### 删除文件

| 文件                                                                         | 原因           |
| ---------------------------------------------------------------------------- | -------------- |
| `src/extensions/builtin/database/domain/services/mock-database-navigator.ts` | 死代码，无引用 |
| `src/extensions/builtin/database/services/mock-navigator-data.ts`            | 死代码，无引用 |

---

## 三、架构设计决策

### 3.1 conn_id 全链路一致性

```
新建数据源
  └── connect_database(conn_id=hash(url))
        ├── 返回 conn_id=hash → 前端存储
        ├── save_global_connection(id=hash) → global_connections 表
        │     └── INSERT OR REPLACE (id=hash)  ← 幂等
        └── 元数据缓存: conn_{hash}.sqlite

重启后
  └── get_global_connections() → id=hash (来自 global_connections)
        └── loadCatalogs(conn.id=hash) → MetadataCacheManager(hash)
              └── 缓存路径: conn_{hash}.sqlite → 命中 ✅
```

### 3.2 缓存失效策略

```
loadCatalogs:
  ├── getMetadataCacheStatus(conn_id, ...)
  │     ├── cache_exists? → 否 → is_valid=false → 进入 DB 查询
  │     ├── last_sync + TTL(300s) < now? → 是 → is_valid=false → 进入 DB 查询
  │     └── 缓存可用 → 从 L2 读取
  └── DB 查询 → 写回 L2 缓存

手动刷新:
  refreshAllMetadata()
    ├── clearMetadataCache(conn_id) → 物理删除 conn_{id}.sqlite
    └── loadCatalogs(conn_id) → 强制 DB 查询 + 重建缓存
```

### 3.3 Scope 切换安全防护

```
用户切换 scope (全局 ↔ 项目):
  ├── watch(scope)
  │     ├── networkConfigId != null? → 警告
  │     ├── selectedEnvId != null? → 警告
  │     └── extraConfig 非空? → 警告
  └── NAlert type="warning" (可关闭)
```

---

## 四、验证结果

- `cargo check` → 0 errors, 0 warnings
- `pnpm run lint` → 0 errors, 263 warnings（全部为已有）
