# 一、架构红线（Architecture Gates）

❌ 禁止：循环依赖

```markdown
❌ core 依赖 adapters
❌ adapters 依赖具体 datasource 实现
✅ adapters 只能依赖 core
```

❌ 禁止：层级越界

```markdown
❌ services 直接调用 datasource/\* (绕过 driver)
❌ connection/factory 写死具体数据库类型
✅ factory 只返回 Box
```

<br />

# 二、Trait 与 接口约束（基于 traits.rs）

❌ 禁止：修改宪法

```markdown
❌ 禁止修改 driver/traits.rs 中的 trait 定义
❌ 禁止给 Database trait 增加与数据访问无关的方法
```

✅ 必须：实现完整性

```markdown
✅ datasource/mysql.rs 必须 impl Database + DbPool
✅ 所有 datasource 必须正确处理 meta() 方法
```

# 三、数据契约检查（IPC 生死线）

❌ 致命违规：非 Arrow 传输

```rust
// ❌ 禁止在以下位置出现
async fn query(\&self, sql: \&str) -> Result\<Vec<Row>, CoreError>; // 违规
async fn query(\&self, sql: \&str) -> Result\<QueryResult, CoreError>; // 需检查 QueryResult 内部
```

✅ 合规标准

```rust
// ✅ 必须确保路径畅通
Database::query()
↓
QueryResult
└─ batches: Vec<RecordBatch>  ✅
↓
Tauri Command
↓
JSON
```

❌ 禁止：在 IPC 层做转换

```markdown
❌ Tauri Command 中手动将 Row 转为 JSON
✅ Tauri Command 只负责调用 service 并返回结果
```

四、连接池与资源管理
✅ 必须：Pool 下沉

```markdown
✅ datasource/pool.rs 只定义 DbPool trait
✅ 具体 Pool (SqlxMySqlPool) 必须在 datasource/{db}/pool.rs
❌ 禁止存在顶层 connection/pool.rs 文件
```

✅ 必须：Acquire 返回 Database 实例

```rust
// ✅ 正确
async fn acquire(\&self) -> Result\<Box<dyn Database>, CoreError>;

// ❌ 错误
async fn acquire(\&self) -> Result\<Pool<MySql>, CoreError>;
```

# 五、错误处理与 Rust 规范

❌ 禁止：unwrap / expect

```rust
// ❌ 禁止在生产代码中
let x = foo().unwrap();

// ✅ 必须
let x = foo()?;
```

✅ 必须：使用 CoreError

```rust
// ✅ 必须
use crate::core::error::CoreError;

// ❌ 禁止
use std::io::Error;
```

# 六、前端交互约束

✅ 必须：SchemaObject 懒加载

```rust
// ✅ 正确
SchemaObject {
children: None, // 表示未加载
}

// ❌ 错误
SchemaObject {
children: Some(vec!\[]), // 误判为空
}
```

# 七、SQL 处理封装规则（core/sql/）

✅ 必须：集中引用

```rust
// ✅ 正确：业务模块通过 SqlEngine 调用
use crate::core::sql::SqlEngine;
let (stmt_type, normalized) = SqlEngine::parse_and_route(sql, SqlDialect::Ansi);

// ❌ 错误：业务模块直接依赖 sqlglot-rust
use sqlglot_rust::{parse, Dialect};
```

❌ 禁止：绕过封装

```rust
// ❌ 禁止在业务模块中直接使用
sqlglot_rust::parse(sql, dialect)
sqlglot_rust::transpile(sql, src, tgt)
sqlglot_rust::builder::select_all()
```

✅ 必须：新增能力先定义再实现

```rust
// 1. 在 engine.rs 中定义方法签名
impl SqlEngine {
    pub fn new_capability(input: &str) -> String {
        builder::new_capability(input)
    }
}

// 2. 在对应子模块中实现具体逻辑
// builder.rs / parser.rs / formatter.rs / transpiler.rs
```

# 八、测试代码组织铁律

❌ 绝对禁止：mod.rs 中包含测试代码

```markdown
❌ mod.rs 中禁止 #[cfg(test)] 块
❌ mod.rs 中禁止 fn test\_ 测试函数
❌ mod.rs 中禁止 mod tests { }
✅ mod.rs 只做：声明子模块、重新导出、定义常量
```

✅ 必须：私有函数测试内嵌

```rust
// ✅ 在源文件底部
// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internal_helper() { ... }
}
```

✅ 必须：公共 API 复杂测试外移

```markdown
✅ 源码 > 500 行 → 测试外移到 src-tauri/tests/
✅ 测试总量 > 100 行 → 外移
✅ 需要复杂 Mock → 外移
✅ 不满足以上条件 → 可内嵌在源文件中
```

# 九、检查清单（Checklist）

在提交任何代码前，请自检以下问题：

- [ ] DuckDB 是否被写死为唯一执行引擎？（必须是可插拔的）
- [ ] QueryResult 内部是否包含 RecordBatch？
- [ ] services 层是否只调用 connection / driver，不直接碰 datasource？
- [ ] Rust 代码中是否存在 unwrap()？
- [ ] Pool 是否只负责连接，不负责 SQL 执行？
- [ ] 业务模块是否直接 use sqlglot_rust？（必须通过 SqlEngine）
- [ ] mod.rs 是否包含测试代码？（绝对禁止）
- [ ] 测试函数是否出现在业务代码中间？（必须放在独立的 mod tests 块）
- [ ] 全局/项目 DB 字段映射是否一致？（global_connections 与 connections 表必须对齐）
- [ ] auth_data 只存认证凭据，不混入连接属性（host/port/database）？
- [ ] 密码/Auth 凭据是否已 AES-256-GCM 加密？（前缀 AES: 标记）
- [ ] network_configs 全局/项目表结构是否区别对待？（全局无 origin，项目有 origin）
- [ ] auth_configs 的 auth_type 是否正确分类？（数据库认证 vs 网络认证）
- [ ] addDataSource StagingItem 是否包含全量 19 字段（schemaName/options/metadataPath/tags/useDuckdbFed）？
- [ ] addDataSource selectStaging 是否恢复了 authConfigId 和 authMethod？
- [ ] create_auth_config 是否返回创建的 AuthConfig（含生成的 id），调用方是否捕获并更新状态？
- [ ] NetworkTab.saveNewProfile 是否由后端生成 auth/network ID（禁止前端自生成）？
- [ ] useAuthConfig.loadAuthConfigs 是否按 scope 同时加载全局和项目级认证配置？
- [ ] addDataSource handleApply project_id 参数是否使用 projectPath 而非 projectId？

# 十、数据源模块专项约束（v0.5.1+）

## 10.1 认证配置（auth_configs）

```markdown
❌ auth_data 禁止混入 host / port / database 等连接属性
✅ auth_data 只存认证凭据（username / password / certPath / principal / clientSecret）
✅ 密码/敏感字段写入前必须 encrypt_auth_data()（AES-256-GCM，AES: 前缀）
✅ 读取时自动 decrypt_auth_data()
```

### auth_type 分类

| 大类       | auth_type                                                  |
| ---------- | ---------------------------------------------------------- |
| 数据库认证 | password, ldap, pg_class, kerberos, oauth2, os_auth, trust |
| 网络认证   | ssh_password, proxy_password                               |

## 10.2 网络配置（network_configs）

```markdown
✅ 全局 network_configs 表：6 列基础字段（无 origin/source_id/snapshot_at）
✅ 项目 network_configs 表：9 列（含 origin + 快照溯源）
✅ config 列保留完整冗余（host/port/forwarding + auth）
✅ auth_config_id 列引用 auth_configs.id（可选，SSL 证书配置不需要）
```

## 10.3 连接配置（global_connections / connections）

```markdown
✅ 两表字段完全对齐（25 列），前后端类型一一映射
✅ 全局 DB INSERT：字段值来源自 ConnectDatabaseInput，禁止硬编码
✅ 项目 DB INSERT：由 create_project_connection 统一管理
✅ 前端 StagingItem → CreateProjectConnectionInput → Rust CreateProjectConnectionInput 全程透传
```

## 10.4 StagingItem 状态管理（v0.5.2+）

```markdown
✅ saveToStaging 必须写入 StagingItem 全量 19 字段（含 schemaName/options/metadataPath/tags/useDuckdbFed）
✅ syncCurrentToStaging 必须与 saveToStaging 字段集合一致
✅ selectStaging 必须恢复 authConfigId 和 authMethod（与 syncCurrentToStaging 对称）
✅ addStaging（"+"按钮）必须重置 authConfigId/authMethod 为默认值
❌ 禁止 saveToStaging 静默丢弃字段（如只写 14/19 字段）
✅ create*auth_config（全局）必须返回 AuthConfig（含后端生成的 id，对齐 project_create_auth_config）
✅ doSaveAuth 保存后必须捕获返回值并更新 authConfigId.value
❌ 禁止前端自生成 auth/network ID（如 G_ssh_auth*${Date.now()}），ID 必须由后端统一分配
✅ NetworkTab.onMounted 必须始终 loadAll()（全局），再按需 loadAllProject()（项目），禁止 if/else 短路
✅ useAuthConfig.loadAuthConfigs 必须接受 projectPath 参数，按 scope 合并全局+项目配置
✅ handleApply project 连接时必须使用 projectStore.currentProject?.path（而非 .id）作为 project_id
✅ os_auth/trust 等无凭据认证方式不触发认证配置保存（hasAuth 判定不含 authType 后缀条件）
```

# 十一、项目约束

- 每次提交代码前，必须先自检以上问题，确保代码符合项目规范，并且汇报修改内容。
- 本Skill适用于Trae CN AI编辑器，启用后AI将全程遵循上述规范，生成可直接编译、贴合项目需求的代码，无需重复说明项目规则。
- 技术栈升级需遵循既定策略，升级前需测试兼容性，确保不破坏核心功能与接口兼容性，升级后更新本Skill对应版本信息。
- 所有新增功能、修复均需符合项目架构与长期约束，确保项目轻量、高效、可扩展，支撑10年生命周期。
