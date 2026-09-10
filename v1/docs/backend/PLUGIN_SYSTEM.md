# RdataStation 插件系统架构

## 概述

RdataStation 插件系统提供了一个灵活、安全、高效的插件框架，支持两种插件类型：

- **WASM 插件**：基于 Extism 的沙箱化插件，用于跨语言、安全隔离的功能扩展
- **Go Sidecar 插件**：独立进程的高性能插件，用于复杂功能和原生数据库驱动

插件系统采用双层数据库架构，与驱动管理保持一致的设计风格。

## 目录结构

```
rdata-station/src-tauri/src/
├── core/plugin/
│   ├── mod.rs             # 模块导出
│   ├── manifest.rs        # 插件清单定义
│   ├── manager.rs         # 核心插件管理器
│   ├── events.rs          # 事件系统
│   └── storage.rs         # 插件存储系统
├── core/persistence/
│   ├── plugin_store.rs    # 插件数据持久化
│   ├── global_db.rs       # 全局数据库管理器（已集成插件方法）
│   └── project_connection_store.rs  # 项目数据库管理器（已集成插件方法）
├── adapters/
│   ├── wasm/              # WASM 插件适配器
│   │   ├── extism.rs      # Extism 插件管理器
│   │   ├── host_functions.rs
│   │   └── mod.rs
│   └── sidecar/           # Go Sidecar 插件适配器
│       ├── manager.rs
│       ├── health_checker.rs
│       ├── hot_reload_manager.rs
│       └── mod.rs
└── commands/plugin_commands.rs  # Tauri 命令层
```

## 架构图

```
┌─────────────────────────────────────────────────────────────┐
│                        前端 UI                              │
└────────────────────────┬────────────────────────────────────┘
                         │
                    ┌────▼────┐
                    │ Tauri   │
                    │ Commands│
                    └────┬────┘
                         │
        ┌────────────────┼────────────────┐
        │                │                │
   ┌────▼────┐    ┌─────▼─────┐   ┌────▼────┐
   │  WASM   │    │  Core    │   │ Sidecar │
   │ Manager │    │  Plugin  │   │ Manager │
   └────┬────┘    │  Manager │   └────┬────┘
        │         └─────┬─────┘        │
        └───────────────┼──────────────┘
                        │
            ┌───────────┼───────────┐
            │           │           │
      ┌─────▼───┐  ┌───▼────┐ ┌───▼─────┐
      │ Events  │  │ Storage│ │ Persist │
      │ Manager │  │        │ │  Layer  │
      └─────────┘  └────────┘ └────┬────┘
                                    │
                    ┌───────────────┼───────────────┐
                    │               │               │
            ┌───────▼──────┐ ┌──────▼──────┐       │
            │   Global     │ │   Project   │       │
            │   Database   │ │   Database  │       │
            └──────────────┘ └─────────────┘       │
```

## 双层数据库架构

与驱动管理设计保持一致，插件系统使用双层数据库架构：

### 1. 全局数据库 (Global Database)

- 存储插件注册表信息
- 存储所有已安装的插件元数据
- 存储插件全局配置
- 跨所有项目共享
- 表结构：
  - `plugins`: 插件注册表
  - `plugin_dependencies`: 插件依赖关系
  - `plugin_global_config`: 插件全局配置

### 2. 项目数据库 (Project Database)

- 存储每个项目使用的插件引用
- 存储每个项目的插件配置
- 每个项目独立的配置
- 表结构：
  - `project_used_plugins`: 项目使用的插件
  - `project_plugin_config`: 项目插件配置

## 核心组件

### 1. PluginManager (核心管理器)

统一管理所有插件的生命周期，包括：

- 插件发现和加载
- 插件激活/停用
- 插件类型路由
- 生命周期协调

### 2. PluginStore (持久化层)

负责插件数据的持久化，提供：

- 全局插件注册表操作
- 项目插件引用管理
- 插件配置存储
- 与驱动管理保持一致的 API 风格

### 3. ExtismPluginManager (WASM 插件)

负责 WASM 插件的加载和管理：

- 沙箱隔离执行
- 宿主函数绑定
- 配置和环境变量
- 状态跟踪

### 4. SidecarManager (Sidecar 插件)

管理 Go Sidecar 进程：

- 进程生命周期管理
- 健康检查
- 热重载/热更新
- RPC 通信

### 5. EventManager (事件系统)

提供插件间通信机制：

- 发布-订阅模式
- 类型安全事件
- 插件生命周期事件

### 6. PluginStorage (存储系统)

为插件提供运行时存储：

- 键值存储
- 内存缓存
- 数据隔离

## 插件清单

每个插件必须包含 `plugin.toml` 清单文件：

```toml
# plugin.toml
id = "com.example.plugin"
name = "Example Plugin"
version = "1.0.0"
description = "An example plugin"
author = "Your Name"
plugin_type = "Wasm"  # 或 "Sidecar"

[contributes]
commands = ["example.command"]
panels = ["example.panel"]
drivers = ["mysql-enhanced"]

[config]
api_key = ""
timeout = 30000
```

## 插件生命周期

```
    ┌──────┐
    │ Load │
    └──┬───┘
       │
    ┌──▼────┐
    │ Activate│
    └──┬────┘
       │
    ┌──▼──────┐
    │ Running │ ◄──────────┐
    └──┬──────┘            │
       │                   │
    ┌──▼────────┐     Hot  │
    │ Deactivate│  Update  │
    └──┬────────┘          │
       │                   │
    ┌──▼──────┐            │
    │ Unload  │────────────┘
    └─────────┘
```

## 数据模型

### 全局插件表 (plugins)

```sql
CREATE TABLE plugins (
    id TEXT PRIMARY KEY,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    author TEXT,
    description TEXT,
    repo_url TEXT,
    plugin_type TEXT NOT NULL,
    manifest_json TEXT,
    install_path TEXT NOT NULL,
    is_enabled INTEGER DEFAULT 1,
    is_builtin INTEGER DEFAULT 0,
    installed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(code, version)
);
```

### 项目使用插件表 (project_used_plugins)

```sql
CREATE TABLE project_used_plugins (
    plugin_code TEXT NOT NULL,
    plugin_version TEXT NOT NULL,
    enabled INTEGER DEFAULT 1,
    required INTEGER DEFAULT 1,
    PRIMARY KEY (plugin_code, plugin_version)
);
```

## 安全性

- WASM 插件在 Extism 沙箱中运行
- Sidecar 进程完全隔离
- 宿主函数白名单机制
- 数据按插件隔离存储
- 数据库操作使用参数化查询防止注入

## 扩展性

- 插件可以扩展 SQL 查询能力
- 可以添加自定义数据源驱动
- 可以注入 UI 组件和面板
- 可以订阅和响应系统事件
- 支持项目级配置隔离

## Tauri 命令 API

### 插件管理

- `plugin_install`: 安装新插件
- `plugin_uninstall`: 卸载插件
- `plugin_enable`: 全局启用插件
- `plugin_disable`: 全局禁用插件
- `plugin_get_all_installed`: 获取所有已安装插件

### 项目引用

- `project_plugin_enable`: 在项目中启用插件
- `project_plugin_disable`: 在项目中禁用插件
- `project_plugin_remove`: 从项目移除插件
- `project_plugin_list`: 获取项目使用的插件列表
- `project_plugin_set_config`: 设置项目插件配置
- `project_plugin_get_configs`: 获取项目插件配置

### 运行时

- `plugin_load`: 加载插件（内存）
- `plugin_activate`: 激活插件
- `plugin_deactivate`: 停用插件
- `plugin_unload`: 卸载插件
