# commands 层退役与映射（v1 → v2）

> 结论：v1 `backend/src/commands/`（24 文件，约 11204 行，300+ 个 `#[tauri::command]`）是 **Tauri IPC 壳层**。v2 采用 GPUI-kit 桌面架构，Tauri 已退役（v1 `adapters/tauri` 同期退役），commands 层不迁入编译，其引用的**全部业务服务**均已迁入 12 个 Feature crate，形成完整闭环。

## 架构边界（v1 原约束，v2 继承）

```
❌ commands 禁止 import core::dbi（只能访问 services）
✅ commands 允许 import core::services（业务逻辑入口）
✅ commands 允许 import core::error / core::models（基础类型）
```

v2 对应关系：`services` → 各 Feature crate 的公开服务；`error/models` → `shared`。

## 命令文件 → 服务 → v2 归属映射

| v1 命令文件 | 命令数 | 引用的 v1 服务 | v2 所在 crate |
| --- | --- | --- | --- |
| analytics_resource_commands.rs | 26 | analytics_resource_store | `analytics_resource`（M6） |
| cache_warming_commands.rs | 9 | ConnId / connection_manager | `engine`（M2）+ `workbench`（M5 服务） |
| connection_commands.rs | 17 | connection_manager / connection_service | `engine`（M2）+ `workbench`（M5） |
| data_source_commands.rs | 45 | driver_service | `workbench`（M5） |
| driver_commands.rs | 5 | ConnectionService | `workbench`（M5） |
| logging_commands.rs | 7 | logging | `engine`（M2） |
| memory_commands.rs | 5 | persistence/cache | `engine`（M2） |
| metadata_cache_commands.rs | 12 | persistence/metadata_cache | `engine`（M2） |
| metadata_commands.rs | 19 | MetadataService | `database`（M4） |
| mock_commands.rs | 13 | mock::MockEngine | `mock`（M7，Round 13） |
| mock_persistence_commands.rs | 8 | mock::MockGenerationStore | `mock`（M7，Round 13） |
| navigator_commands.rs | 2 | metadata_service | `database`（M4） |
| performance_commands.rs | 3 | performance | `engine`（M2） |
| plugin_commands.rs | 23 | plugin_bridge / plugin_service | `plugin`（M9，Round 12） |
| port_commands.rs | 8 | port_negotiation | `shared`（M0） |
| project_commands.rs | 26 | driver_service / plugin_service | `workbench`（M5）+ `plugin`（M9）+ `project`（M1） |
| project_store_commands.rs | 7 | project::ProjectManager | `project`（M1，Round 9） |
| result_commands.rs | 21 | result_service | `workbench`（M5，Round 8） |
| scratchpad_commands.rs | 26 | scratchpad::ScratchpadStore | `scratchpad`（M5，Round 10） |
| sql_commands.rs | 16 | duckdb_service | `engine`（M2，Round 6） |
| sql_parser_commands.rs | 5 | sql_parser_service | `engine`（M2，Round 6） |
| sql_template_commands.rs | 6 | sql/template | `engine`（M2） |
| system_commands.rs | 1 | app 信息 | `app`（Shell，Round 1） |

## v2 替代路径（GPUI 视图 → 服务）

v2 中由 GPUI 视图（各 Feature crate 的 `*_view.rs`，逐步实现）直接调用上述服务，不再经过 JSON-RPC 序列化壳层：

```text
v1: 前端(JS) --tauri IPC--> commands --tauri::State--> services
v2: GPUI 视图 --直接调用--> Feature crate 服务 --> engine/shared
```

- 状态注入：v1 `tauri::State` → v2 `app` crate 的 Shell 持有服务实例（Rc<RefCell> / Arc<Mutex>）。
- 返回类型：v1 `Result<Value>`（JSON 序列化）→ v2 直接返回强类型（`MockResult<T>` / `Result<T, CoreError>`）。
- 事件推送：v1 `tauri::Emitter` → v2 `gpui` 事件订阅（或 channel）。

## 退役确认

| v1 文件 | 体量 | 处置 |
| --- | --- | --- |
| `backend/src/commands/`（24 文件） | 11204 行 | 退役（壳层），服务映射见上表 |
| `backend/src/lib.rs` | 851 行 | 退役（Tauri 装配/插件注册），由 `app` Shell 替代 |
| `backend/src/main.rs` | 5 行 | 退役（Tauri 入口），由 `app`（gpui-kit Root）替代 |

以上文件保留于 `v1/` 暂存区，不做删除；后续如需恢复命令层（如发布 Web 版），可依据本映射表重建。
