# 分析资源管理器 — 架构设计文档

> 版本：v1.7
> 最后更新：2026-05-12
> 作者：RdataStation 团队

---

## 一、概述

### 1.1 模块定位

分析资源管理器（Analytics Resource Manager）是 RdataStation 的**数据资源元数据管理中心**，负责统一管理用户在数据分析工作流中涉及的各类资源（数据库连接、数据表、分析文件）的元信息、组织关系和生命周期。

### 1.2 核心理念

```
资源 = 元数据指针（非实际数据）
├── 连接类型：指向某个数据库连接
├── 表类型：  指向某个数据库中的表/视图
└── 文件类型：指向某个分析文件（CSV/Parquet/...）

资源 ≠ 数据本身
查询数据 → DuckDB/MySQL 引擎承担
临时文件 → 草稿板（Scratchpad）承担
```

---

## 二、架构分层

```
┌──────────────────────────────────────────────────────────────────┐
│                     Frontend (Vue 3 + TypeScript)                │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │         AnalyticsResourceManager.vue (主页面)                │ │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────┐  │ │
│  │  │SearchBar │ │FilterBar │ │TagManager│ │FolderList│ │ ResourceList  │  │ │
│  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘ └───────────────┘  │ │
│  │  ┌──────────────┐ ┌───────────────┐ ┌──────────────────┐ ┌────────────┐│ │
│  │  │Pagination    │ │SettingsModal  │ │RecycleBinModal   │ │CreateTag   ││ │
│  │  └──────────────┘ └───────────────┘ └──────────────────┘ │Modal       ││ │
│  │  ┌──────────────────────┐ ┌─────────────────────────────┐└────────────┘│ │
│  │  │VersionHistoryModal   │ │CreateResourceModal(新建/编辑)│              │ │
│  │  └──────────────────────┘ └─────────────────────────────┘              │ │
│  └──────────────────────────┬──────────────────────────────────┘ │
│                             │ Pinia Store                        │
│  ┌──────────────────────────▼──────────────────────────────────┐ │
│  │   analytics-resource-store.ts (状态管理 + 缓存)              │ │
│  └──────────────────────────┬──────────────────────────────────┘ │
│                             │ analytics-resource-api.ts          │
│                             │ tauri.invoke()                     │
├─────────────────────────────┼────────────────────────────────────┤
│                     IPC Layer (Tauri Bridge)                     │
│                                                                  │
│  IPC 传输策略 (Arrow Compliance):                                 │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │ 配置 CRUD (本模块)                                           │ │
│  │   Tauri Command → Result<AnalyticsResource, CoreError>       │ │
│  │   结构体序列化 JSON ←────→ 前端强类型接口                     │ │
│  │   特征：数据量小 (< 100条)、元数据属性、需精确类型安全        │ │
│  │                                                              │ │
│  │ 数据查询 (sql_service / dbi)                                 │ │
│  │   Tauri Command → Result<QueryResult, CoreError>             │ │
│  │   QueryResult.batches: Vec<RecordBatch> → JSON               │ │
│  │   特征：数据量大 (> 万行)、需零拷贝、Arrow 列式传输           │ │
│  └──────────────────────────────────────────────────────────────┘ │
├─────────────────────────────┼────────────────────────────────────┤
│                     Backend (Rust)                               │
│  ┌──────────────────────────▼──────────────────────────────────┐ │
│  │   analytics_resource_commands.rs (26 个 Tauri Command)      │ │
│  │   + AnalyticsResourceState (Arc<OnceLock<Store>>)            │ │
│  └──────────────────────────┬──────────────────────────────────┘ │
│  ┌──────────────────────────▼──────────────────────────────────┐ │
│  │   analytics_resource_store/ (9 文件目录模块)                  │ │
│  │   ├── mod.rs        (40L)  — 模块入口 + Store 结构体         │ │
│  │   ├── models.rs     (100L) — 9 个数据模型                    │ │
│  │   ├── helpers.rs    (30L)  — parse_datetime 工具函数         │ │
│  │   ├── resource.rs   (408L) — CRUD + 分页 + 克隆              │ │
│  │   ├── recycle.rs    (383L) — 软删除 + 回收站 + 恢复 + 永久删除│ │
│  │   ├── folder.rs     (185L) — 文件夹 CRUD + 资源关联          │ │
│  │   ├── tag.rs        (283L) — 标签 CRUD + 双向关联查询        │ │
│  │   ├── version.rs    (75L)  — 版本历史                        │ │
│  │   └── tests.rs      (505L) — 15 个单元测试                   │ │
│  │   7 张表 + 索引 + 触发器 + 事务 + 参数化查询                 │ │
│  └──────────────────────────┬──────────────────────────────────┘ │
│                             │ rusqlite                           │
│  ┌──────────────────────────▼──────────────────────────────────┐ │
│  │   SQLite (project.db) — 项目元数据库                        │ │
│  │   存储路径：project/meta/project.db                          │ │
│  └─────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
```

---

## 三、数据模型

### 3.1 ER 图

```
analytics_resources (资源)          analytics_folders (文件夹)
┌──────────────────────┐            ┌─────────────────────┐
│ id (PK)              │            │ id (PK)             │
│ resource_type        │            │ name                │
│ name                 │            │ scope (CHECK)       │
│ alias                │            │ parent_folder_id(FK)│──┐ 自引用树
│ config (JSON, CHECK) │            │ sort_order          │  │
│ scope (CHECK)        │            │ color               │  │
│ row_count            │            │ icon                │  │
│ column_count         │            │ created/updated_at  │  │
│ file_size            │            │ deleted_at          │  │
│ version              │            └─────────┬───────────┘  │
│ parent_version_id    │                      │              │
│ parent_resource_id(FK)──┐                    │ N:M          │
│ source_query         │  │  analytics_resource_folder       │
│ created/updated_at   │  │  ┌──────────────────────┐        │
│ created_by           │  │  │ resource_id (PK,FK)  │        │
│ deleted_at           │  │  │ folder_id   (PK,FK)──┼────────┘
└───────┬──────────────┘  │  │ sort_order           │
        │                 │  └──────────────────────┘
        │ 1:N             │
        │                 │  analytics_resource_tags
┌───────▼──────────────┐  │  ┌──────────────────────┐  analytics_tags
│ resource_versions    │  │  │ resource_id (PK,FK)  │  ┌─────────────────┐
│──────────────────────│  │  │ tag_id      (PK,FK)──┼──│ id (PK)         │
│ id (PK)              │  │  └──────────────────────┘  │ name            │
│ resource_id (FK)─────┘  │                             │ color           │
│ version                 │                             │ icon            │
│ snapshot (JSON)         │                             │ scope (CHECK)   │
│ created_at              │                             │ created/deleted │
└─────────────────────────┘                             └─────────────────┘

        analytics_recycle_bin (回收站)
        ┌──────────────────────┐
        │ id (PK)              │
        │ resource_id          │
        │ resource_type        │
        │ resource_name        │
        │ resource_data (JSON) │ ← 删除瞬间的完整快照
        │ deleted_at           │
        └──────────────────────┘
```

### 3.2 Rust 结构体

| 结构体                  | 文件               | 说明          |
| ----------------------- | ------------------ | ------------- |
| `AnalyticsResource`     | `store.rs:L9-28`   | 资源主实体    |
| `AnalyticsFolder`       | `store.rs:L30-42`  | 文件夹        |
| `AnalyticsTag`          | `store.rs:L44-53`  | 标签          |
| `AnalyticsRecycleItem`  | `store.rs:L55-64`  | 回收站条目    |
| `ResourceVersion`       | `store.rs:L66-72`  | 版本快照      |
| `CreateResourceRequest` | `store.rs:L74-83`  | 创建/更新请求 |
| `CreateFolderRequest`   | `store.rs:L85-90`  | 文件夹请求    |
| `CreateTagRequest`      | `store.rs:L92-97`  | 标签请求      |
| `ListResourcesOutput`   | `store.rs:L99-106` | 分页响应      |

### 3.3 TypeScript 类型

| 接口                        | 文件                      | 说明         |
| --------------------------- | ------------------------- | ------------ |
| `AnalyticsResource`         | `types/index.ts:L7-25`    | 前端资源实体 |
| `AnalyticsResourceSettings` | `types/index.ts:L108-125` | 设置配置     |
| `DEFAULT_SETTINGS`          | `types/index.ts:L127-144` | 默认设置值   |

---

## 四、前端组件树

```
AnalyticsResourceManager.vue (根组件)
├── SearchBar.vue              — 搜索框 (debounce 300ms + 搜索历史) 🆕
├── FilterBar.vue              — 作用域/类型筛选 + 排序 + 批量删除
├── TagManager.vue             — 标签栏 (标签筛选 + 新建入口)
├── FolderList.vue             — 文件夹列表 (支持拖放目标)
├── ResourceList.vue           — 资源列表 (虚拟滚动 + 标签徽章 + 拖拽源) 🆕
│   └── ContextMenu.vue        — 右键菜单 (含版本历史入口)
├── Pagination.vue             — 分页控件
├── CreateResourceModal.vue    — 创建/编辑资源 (共享模态框)
├── CreateFolderModal.vue      — 创建文件夹
├── CreateTagModal.vue         — 创建标签
├── ResourceDetailModal.vue    — 资源详情面板 🆕
├── RecycleBinModal.vue        — 回收站管理
├── SettingsModal.vue          — 设置面板 (4 标签页)
├── VersionHistoryModal.vue    — 版本历史查看
└── ToastContainer.vue         — 消息提示 (支持详情展开)
```

### 组件通信方式

```
AnalyticsResourceManager
  │ props ↓                    events ↑
  ├─→ SearchBar    ←── v-model:searchQuery
  ├─→ FilterBar    ←── v-model:scope/type, @batch-delete
  ├─→ TagManager   ←── :tags, @select-tag, @create-tag 🆕
  ├─→ FolderList   ←── @select-folder, @drop-resource
  ├─→ ResourceList ←── :display-settings, @edit/@delete/@copy/@view-versions 🆕
  ├─→ Pagination   ←── @update:page, @update:page-size
  ├─→ *Modal 系列  ←── v-if 控制显示 / @close/@create/@save
  └─→ ToastContainer ← 共享 useToast() composable
```

---

## 五、数据流

### 5.1 初始化

```
App 启动 / 打开项目
  → AnalyticsResourceManager.onMounted()
    → store.initStore()
      → store.loadSettings()        // localStorage → settings
      → store.applySettingsToState() // settings → runtime
      → api.initAnalyticsResourceStore()
        → tauri.invoke → Rust init_analytics_resource_store
          → AnalyticsResourceStore::init(project_path)
      → store.loadResourcesPaginated()
      → store.loadFolders()
      → store.loadTags()
    → initialized = true
```

### 5.2 设置联动

```
SettingsModal @save
  → handleSaveSettings(settings)
    → store.saveSettings(settings)
      → localStorage.setItem(...)        // 持久化
      → applySettingsToState()
        ├─→ pageSize / sortBy / sortOrder   // 通用设置
        ├─→ resourceCache.updateConfig()    // 缓存设置
        └─→ [props] → ResourceList          // 显示设置
```

### 5.3 资源生命周期

```
create ──→ view ──→ edit ──→ delete (软删除)
                            ├─→ recycle_bin INSERT
                            ├─→ resource deleted_at = now
                            ├─→ resource_folder DELETE
                            └─→ resource_tags DELETE
                                    │
                            ┌───────▼────────┐
                            │ restore (恢复)  │
                            │ 事务保护         │
                            └───────┬────────┘
                                    │
                            permanent_delete (永久)
```

---

## 六、技术决策记录

| 决策       | 选择                          | 原因                                       |
| ---------- | ----------------------------- | ------------------------------------------ |
| 存储引擎   | SQLite (rusqlite)             | 项目级本地数据库，同项目架构一致           |
| 参数化方式 | `Vec<rusqlite::types::Value>` | 零字符串拼接，防 SQL 注入                  |
| 缓存策略   | 前端 LRU (TTL 30s)            | 资源查询频率高但数据量小，前端缓存减少 IPC |
| 虚拟滚动   | ResourceList 自实现           | 依赖最小化，契合 Tauri 桌面环境            |
| 状态管理   | Pinia + localStorage          | Vue 3 标准，简单可靠，无需后端同步         |
| 版本快照   | 更新时自动保存旧版 JSON       | 完整回滚能力，JSON 格式灵活                |
| 回收站     | 后端独立表                    | 支持快照语义，删除时保留完整数据           |

---

## 七、文件索引

### 后端（Rust）

| 文件                                                  | 行数  | 职责                          |
| ----------------------------------------------------- | ----- | ----------------------------- |
| `core/persistence/analytics_resource_store.rs`        | ~1100 | 7 表 CRUD + 事务 + 参数化查询 |
| `commands/analytics_resource_commands.rs`             | ~400  | 26 个 Tauri Command + State   |
| `migrations/project_meta/007_analytics_resources.sql` | ~150  | DDL + 索引 + 触发器 + CHECK   |
| `lib.rs`                                              | ~250  | 命令注册 + State 管理         |

### 前端（Vue/TS）

| 文件                                           | 职责                        |
| ---------------------------------------------- | --------------------------- |
| `ui/components/AnalyticsResourceManager.vue`   | 主组件，事件协调中心        |
| `ui/components/ResourceList.vue`               | 虚拟滚动列表 + 拖拽源       |
| `ui/components/FolderList.vue`                 | 文件夹侧边栏 + 拖放目标     |
| `ui/components/SettingsModal.vue`              | 4 标签页设置面板            |
| `ui/components/CreateResourceModal.vue`        | 创建/编辑双模式模态框       |
| `ui/components/ResourceDetailModal.vue`        | 资源详情面板 🆕             |
| `ui/components/FilterBar.vue`                  | 筛选 + 排序 + 批量操作栏    |
| `ui/components/TagManager.vue`                 | 标签栏（筛选 + 新建入口）🆕 |
| `ui/components/SearchBar.vue`                  | 防抖搜索框                  |
| `ui/components/Pagination.vue`                 | 分页控件                    |
| `ui/components/ContextMenu.vue`                | 右键菜单                    |
| `ui/components/RecycleBinModal.vue`            | 回收站模态框                |
| `ui/components/CreateFolderModal.vue`          | 创建文件夹模态框            |
| `ui/components/CreateTagModal.vue`             | 创建标签模态框 🆕           |
| `ui/components/VersionHistoryModal.vue`        | 版本历史模态框 🆕           |
| `ui/components/ToastContainer.vue`             | 消息提示（支持详情展开）    |
| `ui/stores/analytics-resource-store.ts`        | Pinia Store + 缓存 + 设置   |
| `ui/composables/use-cache.ts`                  | LRU 缓存                    |
| `ui/composables/use-toast.ts`                  | 消息提示                    |
| `ui/composables/use-search.ts`                 | 搜索逻辑                    |
| `ui/composables/use-debounce.ts`               | 防抖                        |
| `ui/composables/use-search-history.ts`         | 搜索历史 (localStorage) 🆕  |
| `infrastructure/api/analytics-resource-api.ts` | Tauri invoke 封装           |
| `types/index.ts`                               | 全部类型定义                |
