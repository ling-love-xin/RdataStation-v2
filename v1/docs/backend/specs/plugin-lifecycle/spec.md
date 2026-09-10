# 插件生命周期与 Extism 增强 - Product Requirement Document

## Overview

- **Summary**: 完善 RdataStation 插件系统，利用 Extism 简化 WASM 插件生命周期管理，并实现完整的插件激活/停用机制
- **Purpose**: 解决当前插件系统核心功能缺失的问题，为插件开发者提供完整的 API 和生命周期管理
- **Target Users**: 插件开发者、RdataStation 核心开发团队

## Goals

- ✅ 利用 Extism 简化 WASM 插件生命周期管理
- ✅ 实现完整的插件生命周期（激活/停用）
- ✅ 增强 Extism 宿主函数 API
- ✅ 实现插件依赖管理基础框架
- ✅ 提供插件状态管理和事件系统

## Non-Goals (Out of Scope)

- ❌ 插件市场（后续独立项目）
- ❌ 插件调试器（Phase 2）
- ❌ 插件可视化编辑器（Phase 3）

## Background & Context

### Current State

- Extism 已集成，基础实现存在但功能不完整
- `ExtismPluginManager` 只有基础的 load/unload/call 功能
- 插件清单解析已完成（`manifest.rs`）
- 缺少插件生命周期管理（activate/deactivate）
- 缺少宿主函数 API 的完整实现

### Extism 优势

1. 沙箱隔离完善
2. 跨语言支持
3. 简化的 WASM 生命周期管理
4. 宿主函数注册机制
5. 支持配置和环境变量

## Functional Requirements

### FR-1: 完整的插件生命周期管理

系统必须实现完整的插件生命周期，包括：

- 发现和加载插件
- 激活插件（调用 `activate` 函数）
- 停用插件（调用 `deactivate` 函数）
- 热更新插件
- 卸载插件

### FR-2: Extism 宿主函数增强

系统必须提供完整的宿主函数 API，包括：

- 数据库查询 API
- 元数据访问 API
- 事件系统 API
- 日志记录 API
- 配置访问 API

### FR-3: 插件事件系统

系统必须实现插件事件系统，支持：

- 插件事件订阅
- 事件触发
- 事件优先级处理

### FR-4: 插件状态管理

系统必须提供插件状态持久化，包括：

- 插件配置存储
- 插件数据存储
- 插件密钥存储（安全）

### FR-5: Extism 生命周期简化

利用 Extism 的功能简化 WASM 插件生命周期管理：

- 自动管理 Extism Plugin 实例
- 支持插件配置和环境变量
- 简化宿主函数注册
- 提供插件上下文管理

## Non-Functional Requirements

### NFR-1: 性能

- 插件激活时间 < 500ms
- 插件调用延迟 < 10ms
- 内存占用 < 100MB（单个插件）

### NFR-2: 安全性

- WASM 插件必须在沙箱中运行
- 宿主函数必须有权限检查
- 插件不能访问系统文件系统（除非明确授权）

### NFR-3: 可靠性

- 插件崩溃不能影响主程序
- 必须有完整的错误处理和恢复机制
- 插件资源必须在停用时正确释放

## Constraints

- **Technical**: 必须使用 Extism 作为 WASM 运行时
- **Business**: 保持与现有 Sidecar 插件系统的兼容性
- **Dependencies**: Extism, Tauri, SQLite

## Assumptions

- Extism 将继续保持活跃维护
- 插件开发者熟悉 Rust/TypeScript/Wasm
- 用户愿意安装和使用插件

## Acceptance Criteria

### AC-1: 插件生命周期完整实现

- **Given**: 一个有效插件已安装
- **When**: 用户启动应用
- **Then**: 插件自动加载并激活
- **Verification**: `programmatic`
- **Notes**: 测试插件激活事件触发

### AC-2: 插件激活/停用功能

- **Given**: 插件已加载但未激活
- **When**: 用户激活插件
- **Then**: 插件 `activate` 函数被调用，插件状态变为 ACTIVE
- **Verification**: `programmatic`
- **Notes**: 验证激活钩子函数执行

### AC-3: Extism 宿主函数可用

- **Given**: 插件已激活
- **When**: 插件调用宿主函数 `db_query`
- **Then**: 宿主函数正确执行并返回结果
- **Verification**: `programmatic`
- **Notes**: 测试多个宿主函数调用

### AC-4: 插件事件系统工作

- **Given**: 插件已订阅 `on_sql_executed` 事件
- **When**: SQL 查询执行
- **Then**: 插件事件回调被触发
- **Verification**: `programmatic`
- **Notes**: 验证事件回调参数传递

### AC-5: 插件状态持久化

- **Given**: 插件存储了配置数据
- **When**: 插件停用再激活
- **Then**: 配置数据被正确恢复
- **Verification**: `programmatic`
- **Notes**: 验证数据持久化到 SQLite

### AC-6: Extism 生命周期简化

- **Given**: 插件清单中包含 Extism 配置
- **When**: 插件加载
- **Then**: Extism 自动处理配置、环境变量和宿主函数注册
- **Verification**: `human-judgment`
- **Notes**: 代码审查，验证实现是否简化

## Open Questions

- [ ] 插件激活策略（onStartup vs onCommand vs onEvent）
- [ ] 插件版本兼容性如何处理
- [ ] 插件间依赖关系管理策略
