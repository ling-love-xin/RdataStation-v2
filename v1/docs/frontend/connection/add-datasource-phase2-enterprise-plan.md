# 新增数据源 — Phase 2 企业级便利功能规划

> 版本：v1.0
> 更新日期：2026-05-28
> 状态：📋 规划中（Phase 1 审计完成后制定）
> 前置文档：[add-datasource-frontend-plan.md](./add-datasource-frontend-plan.md)（Phase 1 已交付）
> 审计来源：2026-05-28 新增数据源全线审计报告（评级 B+ → 目标 A）

---

## 一、概述

Phase 1 已交付核心数据源创建能力：三级保存策略（全局/项目/全局+项目）、四表数据流矩阵（connections / auth_configs / network_configs / environments）、config_schema 驱动动态表单、SSH/SSL/Proxy 协议链编排。经全线审计，架构设计 A 级，安全机制 A 级，动态性 B+，完成度 B。

Phase 2 聚焦 **企业级便利功能（Enterprise Convenience Features）**，补齐对标 DBeaver / DataGrip 的 6 项关键能力，将综合评级从 B+ 提升至 A。

---

## 二、功能清单

| 编号 | 功能                            | 优先级 | 预估复杂度 | 对标竞品                    |
| ---- | ------------------------------- | ------ | ---------- | --------------------------- |
| E1   | 连接配置导入/导出 (JSON)        | P0     | 中         | DBeaver Import/Export       |
| E2   | 连接配置模板（从模板创建）      | P0     | 中         | DataGrip Templates          |
| E3   | 批量编辑（scope / 环境 / 标签） | P1     | 中         | DBeaver Bulk Edit           |
| E4   | 连接健康监控（心跳检测）        | P1     | 高         | DataGrip Connection Monitor |
| E5   | Staging 撤销/重做               | P1     | 低         | VS Code Undo/Redo           |
| E6   | AddDataSourceDialog 代码收敛    | P2     | 低         | — 工程优化                  |

---

## 三、详细设计

### E1 — 连接配置导入/导出 (JSON)

#### 3.1.1 需求描述

用户可以：

- **导出**：选择一个或多个连接，导出为 `.rdc`（RdataStation Connection）JSON 文件
- **导入**：选择 `.rdc` 文件，解析并批量创建连接
- 导出时自动脱敏密码（替换为 `******`），导入时提示用户补填
- 支持 scope 感知：导出时记录原始 scope，导入时可选择目标 scope

#### 3.1.2 导出载荷格式

```json
{
  "format": "rdc/v1",
  "exported_at": "2026-05-28T10:00:00Z",
  "version": "1.0.0",
  "app_version": "0.5.3",
  "connections": [
    {
      "name": "生产 MySQL",
      "driver": "mysql",
      "driver_id": "mysql",
      "host": "192.168.1.100",
      "port": 3306,
      "database": "production",
      "schema_name": null,
      "username": "admin",
      "password": "******",
      "options": "{\"ssl_mode\": \"REQUIRED\"}",
      "tags": ["production", "mysql"],
      "use_duckdb_fed": false,
      "metadata_path": null,
      "description": "生产环境 MySQL 数据库",
      "auth_config_id": "G_auth_abc123",
      "auth_method": "password",
      "network_config_id": "G_net_xyz789",
      "driver_properties": null,
      "advanced_options": "{\"protocol_chain\":[...]}",
      "environment_id": "G_env_prod",
      "scope": "global"
    }
  ],
  "auth_configs": [
    {
      "id": "G_auth_abc123",
      "name": "生产 MySQL — 认证",
      "auth_type": "password",
      "auth_data": "******"
    }
  ],
  "network_configs": [
    {
      "id": "G_net_xyz789",
      "name": "生产 SSH 隧道",
      "network_type": "ssh",
      "config": "{...}"
    }
  ]
}
```

#### 3.1.3 前端组件树

```
AddDataSourceDialog
└── ImportExportToolbar (新增)
    ├── NButton "导出" → useConnectionExport
    │   ├── 弹出选择列表（多选当前 staging 或已保存连接）
    │   └── 调用 Tauri save dialog 写入 .rdc 文件
    └── NButton "导入" → useConnectionImport
        ├── 调用 Tauri open dialog 选择 .rdc 文件
        ├── 解析 JSON → 预览面板（可反选）
        └── 确认导入 → 逐条 create_connection
```

#### 3.1.4 后端新增 Tauri Command

```rust
// 导出连接为 JSON
#[tauri::command]
async fn export_connections(conn_ids: Vec<String>) -> Result<String, CoreError>;

// 导入连接（校验 + 创建）
#[tauri::command]
async fn import_connections(json: String, target_scope: String) -> Result<ImportResult, CoreError>;

// 导入结果
struct ImportResult {
    success_count: u32,
    fail_count: u32,
    details: Vec<ImportDetail>,
}
```

#### 3.1.5 文件清单

| 层   | 文件                                                    | 说明               |
| ---- | ------------------------------------------------------- | ------------------ |
| 前端 | `src/.../composables/useConnectionExport.ts`            | 导出逻辑           |
| 前端 | `src/.../composables/useConnectionImport.ts`            | 导入逻辑           |
| 前端 | `src/.../components/ImportExportToolbar.vue`            | 导入/导出工具栏    |
| 前端 | `src/.../components/ImportPreviewPanel.vue`             | 导入预览面板       |
| 后端 | `src-tauri/src/commands/connection_commands.rs`（追加） | export/import 命令 |
| 测试 | `src-tauri/tests/connection_import_export_tests.rs`     | 集成测试           |

---

### E2 — 连接配置模板

#### 3.2.1 需求描述

用户可以：

- **保存为模板**：将当前表单配置保存为可复用的模板
- **从模板创建**：在新建数据源时，选择一个模板快速填充表单
- 模板存储于 `global.db` 新表 `connection_templates`

#### 3.2.2 数据库表设计

```sql
-- 迁移版本：014（global）/ 015（project_meta）
CREATE TABLE IF NOT EXISTS connection_templates (
    id              TEXT PRIMARY KEY,           -- T_xxx
    name            TEXT NOT NULL,              -- 模板名称
    description     TEXT,                       -- 模板描述
    category        TEXT DEFAULT 'custom',      -- 分类: custom / production / development / testing
    driver_id       TEXT NOT NULL,              -- 驱动 ID
    template_data   TEXT NOT NULL,              -- JSON 模板数据（不含密码）
    icon            TEXT,                       -- 图标（emoji）
    sort_order      INTEGER DEFAULT 0,
    scope           TEXT DEFAULT 'global',       -- global / project
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 3.2.3 模板数据格式

```json
{
  "host": "localhost",
  "port": 3306,
  "database": "",
  "username": "root",
  "options": { "ssl_mode": "PREFERRED" },
  "driver_properties": {},
  "advanced_options": {},
  "tags": []
}
```

#### 3.2.4 前端 UI 改造

在 `DataSourceHeader` 中新增模板选择器：

```
DataSourceHeader
├── Driver 选择器（现有）
└── 模板选择器（新增）
    ├── NSelect "从模板创建"
    │   └── 分组: 开发 / 测试 / 生产
    └── NButton "保存为模板"（表单非空时可用）
```

#### 3.2.5 后端新增 Tauri Command

```rust
#[tauri::command]
async fn list_connection_templates(driver_id: Option<String>) -> Result<Vec<Template>, CoreError>;

#[tauri::command]
async fn create_connection_template(tmpl: CreateTemplateInput) -> Result<Template, CoreError>;

#[tauri::command]
async fn delete_connection_template(id: String) -> Result<(), CoreError>;
```

#### 3.2.6 预设模板

系统预置 4 个常用模板（随 migration 写入）：

| 模板名称        | 驱动     | 分类        | 关键配置                                 |
| --------------- | -------- | ----------- | ---------------------------------------- |
| MySQL 本地开发  | mysql    | development | host=localhost, port=3306, user=root     |
| PostgreSQL 本地 | postgres | development | host=localhost, port=5432, user=postgres |
| SQLite 文件     | sqlite   | development | file_path=./data.db                      |
| DuckDB 分析     | duckdb   | analytics   | file_path=:memory:                       |

#### 3.2.7 文件清单

| 层   | 文件                                                                 | 说明           |
| ---- | -------------------------------------------------------------------- | -------------- |
| 前端 | `src/.../composables/useConnectionTemplate.ts`                       | 模板管理逻辑   |
| 前端 | `src/.../components/DataSourceHeader.vue`（修改）                    | 新增模板选择器 |
| 后端 | `src-tauri/src/commands/connection_commands.rs`（追加）              | 模板 CRUD 命令 |
| 后端 | `src-tauri/src/core/persistence/template_store.rs`（新增）           | 模板持久化     |
| 迁移 | `src-tauri/migrations/global/014_add_connection_templates.sql`       | 全局模板表     |
| 迁移 | `src-tauri/migrations/project_meta/015_add_connection_templates.sql` | 项目模板表     |

---

### E3 — 批量编辑

#### 3.3.1 需求描述

在 staging 列表（`AddDataSourceSidebar`）中支持：

- 多选暂存项（Shift+Click / Ctrl+Click）
- 批量修改 scope（全部切换为 global / project / both）
- 批量修改 environment_id
- 批量添加/移除 tags
- 批量修改 driver_id（同类型数据库）

#### 3.3.2 前端实现

```
AddDataSourceSidebar（修改）
├── 多选模式
│   ├── NCheckbox 每项左侧
│   ├── 全选/取消全选 头部
│   └── 已选 N 项 → 显示批量操作工具栏
└── 批量操作工具栏
    ├── NSelect "Scope" → global / project / both
    ├── NSelect "Environment" → 环境列表
    ├── NTagInput "Tags" → 添加/移除
    └── NButton "应用"
```

#### 3.3.3 composable 扩展

在 `useAddDataSource.ts` 中新增：

```typescript
// 批量选中
const selectedStagingIds = ref<Set<string>>(new Set())

// 批量操作
function batchSetScope(scope: 'global' | 'project'): void
function batchSetEnvironment(envId: string | null): void
function batchSetTags(tags: string[]): void
function batchSetDriver(driverId: string): void
function toggleSelectAll(): void
function isAllSelected(): boolean
```

#### 3.3.4 文件清单

| 层   | 文件                                                  | 说明                  |
| ---- | ----------------------------------------------------- | --------------------- |
| 前端 | `src/.../composables/useAddDataSource.ts`（修改）     | 新增批量操作方法      |
| 前端 | `src/.../components/AddDataSourceSidebar.vue`（修改） | 新增多选 + 批量工具栏 |

---

### E4 — 连接健康监控

#### 3.4.1 需求描述

- 对已建立的连接，定期执行轻量心跳查询（`SELECT 1`）
- 在连接树中直观显示连接状态（绿色=正常、黄色=延迟高、红色=断开）
- 断开连接自动触发重连（可配置次数+间隔）
- 连接状态变更触发 EventBus 通知

#### 3.4.2 架构设计

```
ConnectionHealthMonitor (新增 Rust 模块)
├── 心跳调度器（tokio::time::interval）
│   ├── 默认间隔: 60s
│   └── 可配置间隔: 10s ~ 600s
├── 状态判定
│   ├── response_time < 1s  → Healthy
│   ├── response_time < 5s  → Degraded
│   ├── response_time >= 5s → Unhealthy
│   └── 连接错误           → Disconnected
└── 自动重连
    ├── 最大重试: 3 次
    ├── 退避策略: 5s / 15s / 30s
    └── 重连成功 → 恢复 Healthy
```

#### 3.4.3 连接状态枚举

```rust
#[derive(Debug, Clone, Serialize, Type)]
pub enum ConnectionHealth {
    Healthy { latency_ms: u64 },
    Degraded { latency_ms: u64, reason: String },
    Unhealthy { latency_ms: u64, reason: String },
    Disconnected { since: chrono::DateTime<chrono::Utc> },
    Reconnecting { attempt: u32, max_attempts: u32 },
    Unknown,
}
```

#### 3.4.4 前端展示

在 `DataSourceSidebar` 连接树中，每个连接节点前显示状态指示灯：

```vue
<!-- 连接状态指示器 -->
<span :class="['health-dot', health]"></span>

<style>
.health-dot.healthy {
  background: var(--brand-success);
}
.health-dot.degraded {
  background: var(--brand-warning);
  animation: pulse 2s infinite;
}
.health-dot.unhealthy {
  background: var(--brand-warning);
}
.health-dot.disconnected {
  background: var(--brand-danger);
}
.health-dot.reconnecting {
  background: var(--brand-warning);
  animation: spin 1s linear infinite;
}
</style>
```

#### 3.4.5 后端新增 Tauri Command

```rust
#[tauri::command]
async fn start_health_monitor(conn_id: String, interval_secs: u64) -> Result<(), CoreError>;

#[tauri::command]
async fn stop_health_monitor(conn_id: String) -> Result<(), CoreError>;

#[tauri::command]
async fn get_health_status(conn_id: String) -> Result<ConnectionHealth, CoreError>;

#[tauri::command]
async fn get_all_health_statuses() -> Result<HashMap<String, ConnectionHealth>, CoreError>;

// Event: health-status-changed { conn_id, status }
```

#### 3.4.6 文件清单

| 层   | 文件                                                    | 说明                   |
| ---- | ------------------------------------------------------- | ---------------------- |
| 后端 | `src-tauri/src/core/services/health_monitor.rs`（新增） | 健康监控服务           |
| 后端 | `src-tauri/src/commands/connection_commands.rs`（追加） | 健康监控 Tauri Command |
| 前端 | `src/.../composables/useHealthMonitor.ts`（新增）       | 前端健康监控订阅       |
| 前端 | `src/.../components/DataSourceSidebar.vue`（修改）      | 连接状态指示器         |

---

### E5 — Staging 撤销/重做

#### 3.5.1 需求描述

当前 staging 已支持 localStorage 持久化和基本 CRUD，但缺少撤销/重做能力。引入轻量操作历史栈：

- 每次 staging 变更（添加/删除/修改字段/scope变更）推入历史栈
- Ctrl+Z 撤销，Ctrl+Shift+Z 重做
- 最大历史深度：50 步

#### 3.5.2 实现方案

在 `useAddDataSource.ts` 中新增：

```typescript
interface StagingSnapshot {
  items: StagingItem[]
  index: number
  timestamp: number
}

// 历史栈
const undoStack = ref<StagingSnapshot[]>([])
const redoStack = ref<StagingSnapshot[]>([])
const MAX_HISTORY = 50

// 操作
function pushSnapshot(): void // 变更前保存快照
function undo(): void // Ctrl+Z
function redo(): void // Ctrl+Shift+Z
function clearHistory(): void // 对话框关闭时清理
```

#### 3.5.3 键盘快捷键绑定

在 `AddDataSourceDialog.vue` 中：

```typescript
onMounted(() => {
  document.addEventListener('keydown', e => {
    if (e.ctrlKey && e.key === 'z' && !e.shiftKey) {
      e.preventDefault()
      undo()
    }
    if (e.ctrlKey && e.key === 'z' && e.shiftKey) {
      e.preventDefault()
      redo()
    }
  })
})
```

#### 3.5.4 文件清单

| 层   | 文件                                                 | 说明              |
| ---- | ---------------------------------------------------- | ----------------- |
| 前端 | `src/.../composables/useAddDataSource.ts`（修改）    | 新增撤销/重做逻辑 |
| 前端 | `src/.../components/AddDataSourceDialog.vue`（修改） | 绑定快捷键        |

---

### E6 — AddDataSourceDialog 代码收敛

#### 3.6.1 现状

`AddDataSourceDialog.vue` 当前 **1060 行**，超过 800 行阈值。以下逻辑应当抽取到 composable：

| 行范围  | 功能                              | 行数 | 目标 composable                            |
| ------- | --------------------------------- | ---- | ------------------------------------------ |
| 319-358 | `handleTest` / `onTestModalClose` | ~40  | `useTestConnection.ts`                     |
| 361-469 | `doSaveAuth` / `buildAuthData`    | ~110 | `useSaveAuth.ts`（或合并到 useAuthConfig） |
| 567-680 | `handleApply` 三路分流            | ~115 | `useApplyConnections.ts`                   |
| 682-726 | `saveToProjectOnly`               | ~45  | 同上                                       |
| 728-774 | `saveToProject`                   | ~50  | 同上                                       |
| 776-783 | `resetAndClose`                   | ~8   | `useDialogLifecycle.ts`                    |

#### 3.6.2 目标

- `AddDataSourceDialog.vue` 收敛至 **~500 行**
- 每个新 composable 控制在 **150 行以内**
- 保持 Props/Emits 接口向后兼容

#### 3.6.3 文件清单

| 层   | 文件                                                 | 说明                     |
| ---- | ---------------------------------------------------- | ------------------------ |
| 前端 | `src/.../composables/useTestConnection.ts`（新增）   | 测试连接逻辑             |
| 前端 | `src/.../composables/useApplyConnections.ts`（新增） | 三路保存分流逻辑         |
| 前端 | `src/.../composables/useDialogLifecycle.ts`（新增）  | resetAndClose 等生命周期 |
| 前端 | `src/.../components/AddDataSourceDialog.vue`（修改） | 精简至 ~500 行           |

---

## 四、开发阶段

### Phase 2A — 快速提效（预期 3-5 天）

| 任务 | 功能      | 工作量 | 依赖 |
| ---- | --------- | ------ | ---- |
| E5   | 撤销/重做 | 0.5d   | 无   |
| E6   | 代码收敛  | 1d     | 无   |
| E1   | 导入/导出 | 2d     | E6   |
| E2   | 连接模板  | 2d     | E6   |

### Phase 2B — 深度增强（预期 3-4 天）

| 任务 | 功能         | 工作量 | 依赖 |
| ---- | ------------ | ------ | ---- |
| E3   | 批量编辑     | 1.5d   | E5   |
| E4   | 连接健康监控 | 2.5d   | 无   |

---

## 五、测试策略

| 功能 | 测试类型  | 测试要点                                        |
| ---- | --------- | ----------------------------------------------- |
| E1   | 集成测试  | 导出 → 导入全链路，密码脱敏/恢复，scope 转换    |
| E2   | 单元+集成 | 模板 CRUD，从模板填充表单字段完整性             |
| E3   | 单元测试  | 批量操作不破坏未选中项，scope/env/driver 联动   |
| E4   | 集成测试  | 心跳超时检测、自动重连、状态变更事件            |
| E5   | 单元测试  | 撤销/重做边界：空栈、满栈、连续操作后的栈一致性 |
| E6   | 回归测试  | AddDataSourceDialog 全部现有功能不受影响        |

---

## 六、风险与注意事项

| 风险                         | 缓解措施                                     |
| ---------------------------- | -------------------------------------------- |
| 导入/导出格式版本不兼容      | 使用 `format: "rdc/v1"` 版本标记，后续可迭代 |
| 健康监控增加连接负载         | 默认 60s 间隔，`SELECT 1` 极轻量             |
| 批量操作可能部分失败         | 逐条错误收集，失败不阻塞成功项               |
| 模板与实际表单差异           | 模板仅填充基础字段，高级配置保持默认         |
| AddDataSourceDialog 重构回归 | E6 在 Phase 2A 最先执行，稳定后再做 E1~E4    |

---

## 七、验收标准

- [ ] E1: 导出 `.rdc` 文件可被同版本应用导入，连接正常建立
- [ ] E2: 4 个预设模板可用，用户模板可创建/删除/选择
- [ ] E3: 批量修改 scope 后，选中的 staging items 全部生效
- [ ] E4: 连接断开后 60s 内检测到状态变更，状态指示灯实时更新
- [ ] E5: Ctrl+Z 撤销后表单恢复到上一步状态，Ctrl+Shift+Z 重做恢复
- [ ] E6: `AddDataSourceDialog.vue` ≤ 500 行，`pnpm run lint` 零错误
- [ ] 所有新增代码通过 `cargo clippy -- -D warnings` 和 `cargo fmt`
- [ ] 不破坏现有 tests 目录下全部已有测试用例

---

## 八、相关文档

| 文档             | 路径                                                                                   |
| ---------------- | -------------------------------------------------------------------------------------- |
| Phase 1 开发计划 | [add-datasource-frontend-plan.md](./add-datasource-frontend-plan.md)                   |
| 后端数据源模块   | [../../backend/DATA-SOURCE-MODULE.md](../../backend/DATA-SOURCE-MODULE.md)             |
| 网络配置 UI 设计 | [../NETWORK-CONFIG-UI-DESIGN.md](../NETWORK-CONFIG-UI-DESIGN.md)                       |
| 连接方式设计     | [../../backend/CONNECTION-METHOD-DESIGN.md](../../backend/CONNECTION-METHOD-DESIGN.md) |
| 前端索引         | [../INDEX.md](../INDEX.md)                                                             |
