# 连接分类与元数据管理

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

---

## 一、需求概述

实现**全局连接 + 项目连接 + 自由切换**的连接分类体系，这是数据库管理工具的事实标准（DataGrip / DBeaver / Navicat 均采用）。

---

## 二、连接分类定义

### 2.1 全局连接（Global Connection）

| 属性         | 说明                                                          |
| ------------ | ------------------------------------------------------------- |
| **归属**     | 软件全局                                                      |
| **存储路径** | `{系统应用目录}/RdataStation/metadata/conn_global_xxx.sqlite` |
| **生命周期** | 与软件共存，不跟随项目                                        |
| **迁移行为** | 项目迁移时不迁移                                              |
| **适用场景** | 个人常用数据库、本地开发库、公共连接                          |

### 2.2 项目专属连接（Project Connection）

| 属性         | 说明                                                         |
| ------------ | ------------------------------------------------------------ |
| **归属**     | 项目私有                                                     |
| **存储路径** | `{项目目录}/.rdata-station/metadata/conn_project_xxx.sqlite` |
| **生命周期** | 与项目共存                                                   |
| **迁移行为** | 项目迁移时完整带走                                           |
| **适用场景** | 客户环境、交付项目、临时环境、跨设备工作                     |

---

## 三、实现状态

### 3.1 连接创建时分类 ✅ 已完成

- [x] 创建连接时允许用户选择连接类型（全局/项目）
- [x] 根据连接类型决定元数据存储路径
- [x] 全局连接存储到系统目录
- [x] 项目连接存储到项目目录

**实现文件**：

- `src/core/services/connection_manager.rs` - 添加 `ConnectionType` 枚举
- `src/core/services/connection_service.rs` - `connect_with_type()` 方法
- `src/commands/connection_commands.rs` - `connect_database` 命令支持类型参数

### 3.2 连接类型转换 ✅ 已完成

- [x] 实现全局连接 → 项目连接转换
  - [x] 复制元数据到项目目录
  - [x] 更新连接记录中的类型标记
  - [x] 原全局连接保留
- [x] 实现项目连接 → 全局连接转换
  - [x] 移动元数据到系统目录
  - [x] 更新连接记录中的类型标记
  - [x] 原项目连接移除

**实现文件**：

- `src/core/services/connection_service.rs` - `convert_to_project_connection()` / `convert_to_global_connection()`
- `src/core/services/connection_manager.rs` - `update_connection_info()` 方法
- `src/commands/connection_commands.rs` - `convert_connection_type` 命令

### 3.3 项目迁移提醒 ✅ 后端已完成

- [x] 检测项目中是否存在全局连接
- [ ] 迁移时弹出提醒对话框（前端实现）
- [ ] 提供"转为项目连接"快捷操作（前端实现）

**实现文件**：

- `src/core/services/connection_service.rs` - `detect_global_connections_in_project()`
- `src/commands/connection_commands.rs` - `detect_global_connections_in_project` 命令

### 3.4 元数据路径管理 ✅ 已完成

- [x] 修改 `connection_service.rs` 中的元数据初始化逻辑
- [x] 根据连接类型选择正确的存储路径
- [x] 全局连接：`{data_dir}/RdataStation/metadata/conn_global_{conn_id}.sqlite`
- [x] 项目连接：`{project_path}/.rdata-station/metadata/conn_project_{conn_id}.sqlite`

### 3.5 数据模型扩展 ✅ 已完成

- [x] 在连接配置中添加 `connection_type` 字段（global/project）
- [x] 更新连接列表 API，返回连接类型信息
- [x] `ConnectionInfoResponse` 添加 `connection_type` 和 `project_id` 字段

---

## 四、前端待实现任务

### 4.1 连接创建 UI

- [ ] 创建连接对话框中添加连接类型选择（全局/项目）
- [ ] 项目连接时自动关联当前项目

### 4.2 连接类型转换 UI

- [ ] 连接列表中添加"转换为项目连接"操作
- [ ] 连接列表中添加"转换为全局连接"操作
- [ ] 转换确认对话框

### 4.3 项目迁移提醒

- [ ] 项目迁移时检测全局连接
- [ ] 弹出提醒对话框
- [ ] 提供"一键转为项目连接"按钮

### 4.4 连接列表展示

- [ ] 连接列表中显示连接类型标签（全局/项目）
- [ ] 不同类型连接使用不同图标区分

---

## 五、Tauri 命令 API

### 5.1 创建连接

```typescript
// 创建全局连接（默认）
await invoke('connect_database', {
  db_type: 'mysql',
  url: 'mysql://localhost:3306/test',
  name: '本地 MySQL',
  connection_type: 'global',
})

// 创建项目连接
await invoke('connect_database', {
  db_type: 'mysql',
  url: 'mysql://localhost:3306/test',
  name: '客户数据库',
  connection_type: 'project',
  project_id: '/path/to/project',
})
```

### 5.2 转换连接类型

```typescript
// 全局 → 项目
await invoke('convert_connection_type', {
  conn_id: 'mysql-localhost',
  target_type: 'project',
  project_id: '/path/to/project',
})

// 项目 → 全局
await invoke('convert_connection_type', {
  conn_id: 'mysql-localhost',
  target_type: 'global',
})
```

### 5.3 检测全局连接

```typescript
// 检测项目中的全局连接
const globalConnections = await invoke('detect_global_connections_in_project', {
  project_id: '/path/to/project',
})
```

### 5.4 获取连接列表

```typescript
// 返回的连接信息包含类型字段
const connections = await invoke('get_connections')
// [
//   {
//     id: 'mysql-localhost',
//     name: '本地 MySQL',
//     db_type: 'mysql',
//     connection_type: 'global',
//     project_id: null,
//     ...
//   }
// ]
```

---

## 六、设计优势

1. **不破坏项目迁移能力**：项目专属连接 = 100% 可迁移
2. **不破坏目录整洁性**：全局连接统一存放，不污染项目
3. **用户可自由选择**：新手随便用，高级用户自由控制
4. **架构极其清晰**：全局=系统级，项目=可迁移，元数据按归属自动存放

---

## 七、最终目录结构

```
【系统全局目录】
└── metadata/
    └── conn_global_xxx.sqlite    <-- 全局连接

【用户项目目录】
├── project.meta.sqlite
├── project.analysis.duckdb
└── metadata/
    └── conn_project_xxx.sqlite   <-- 项目连接
```

---

## 八、官方描述

```
连接分类与元数据存储规范：

1. 连接分为两类：
   • 全局连接：归属软件，不随项目迁移
   • 项目专属连接：归属项目，随项目完整迁移

2. 元数据存储：
   • 全局连接：系统应用目录/metadata
   • 项目连接：项目目录/metadata

3. 迁移规则：
   • 迁移项目时，仅迁移项目专属连接
   • 全局连接会给出提示，支持一键转为项目连接

4. 设计目标：
   兼顾目录整洁性与项目可移植性，满足个人使用与交付迁移双重场景。
```

---

## 九、相关文件

### 后端（已完成）

- `src/core/services/connection_manager.rs` - 连接管理器，添加 `ConnectionType` 枚举
- `src/core/services/connection_service.rs` - 连接服务，实现类型转换逻辑
- `src/commands/connection_commands.rs` - Tauri 命令，暴露 API 给前端
- `src/core/services/mod.rs` - 模块导出

### 前端（待实现）

- `src/views/connection/` - 连接管理界面
- `src/hooks/useConnection.ts` - 连接管理 Hook
- `src/components/connection/ConnectionDialog.vue` - 连接创建对话框

---

## 十、编译验证

- [x] `cargo check` 通过
- [x] 无编译错误
- [x] 无编译警告（待 clippy 检查）

---

## 十一、备注

- 这是 DataGrip / DBeaver / Navicat 都在用的标准设计
- 能同时解决「目录整洁」和「项目迁移」两个核心需求
- 后端核心功能已完成，前端 UI 待实现
- 实现时确保向后兼容，不影响现有连接
