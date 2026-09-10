# 插件 API 参考文档

## 目录

1. [Tauri Commands](#tauri-commands)
2. [核心插件管理](#plugin-manager)
3. [事件系统](#events)
4. [存储 API](#storage)

---

## Tauri Commands

所有插件相关的 Tauri 命令。

### plugin_list

获取所有已加载的插件列表。

```typescript
import { invoke } from '@tauri-apps/api/core'

const plugins = await invoke('plugin_list')
```

**返回值**: `Array&lt;PluginInfo&gt;`

```typescript
interface PluginInfo {
  id: string
  name: string
  version: string
  plugin_type: string // "Wasm" | "Sidecar"
  metadata: any
  state: string // "Loaded" | "Active" | "Inactive" | "Error"
}
```

### plugin_load

加载插件。

```typescript
await invoke('plugin_load', {
  pluginId: 'com.example.plugin',
  pluginPath: '/path/to/plugin',
})
```

**参数**:

- `pluginId`: 插件唯一标识符
- `pluginPath`: 插件目录路径

**返回值**: `PluginInfo`

### plugin_activate

激活已加载的插件。

```typescript
await invoke('plugin_activate', {
  pluginId: 'com.example.plugin',
})
```

**参数**:

- `pluginId`: 插件 ID

### plugin_deactivate

停用插件。

```typescript
await invoke('plugin_deactivate', {
  pluginId: 'com.example.plugin',
})
```

### plugin_unload

卸载插件。

```typescript
await invoke('plugin_unload', {
  pluginId: 'com.example.plugin',
})
```

### plugin_get_status

获取插件状态。

```typescript
const status = await invoke('plugin_get_status', {
  pluginId: 'com.example.plugin',
})
```

**返回值**: `PluginStatus`

```typescript
interface PluginStatus {
  plugin_id: string
  name: string
  version: string
  plugin_type: string
  status: string
  config: any | null
}
```

### plugin_add_directory

添加插件目录并扫描。

```typescript
const plugins = await invoke('plugin_add_directory', {
  directory: '~/.rdata-station/plugins',
})
```

**返回值**: `Array&lt;PluginInfo&gt;` - 发现的插件列表

### plugin_db_query

通过插件执行数据库查询。

```typescript
const result = await invoke('plugin_db_query', {
  pluginId: 'com.example.plugin',
  connId: 'connection-id',
  sql: 'SELECT * FROM users',
  timeout: 30000,
})
```

### plugin_db_metadata

获取数据库元数据。

```typescript
const metadata = await invoke('plugin_db_metadata', {
  pluginId: 'com.example.plugin',
  connId: 'connection-id',
  catalog: 'db',
  schema: 'public',
  kind: 'tables',
})
```

---

## 核心插件管理 (Rust)

### PluginManager

核心插件管理器。

```rust
use crate::core::plugin::manager::PluginManager;

let manager = PluginManager::new();

// 添加插件目录
manager.add_plugin_dir("/path/to/plugins".into());

// 扫描发现插件
let plugins = manager.scan_plugins()?;

// 加载插件
manager.load_plugin("plugin-id", &amp;"/path/to/plugin".into())?;

// 激活插件
manager.activate_plugin("plugin-id")?;

// 列出插件
let list = manager.list_plugins();
```

### ExtismPluginManager

WASM 插件专用管理器。

```rust
use crate::adapters::wasm::extism::ExtismPluginManager;

let manager = ExtismPluginManager::new();

// 加载插件
manager.load_plugin("plugin-id", &amp;wasm_bytes, Some(config), None)?;

// 激活
manager.activate_plugin("plugin-id")?;

// 调用插件函数
let result = manager.call_plugin("plugin-id", "function", &amp;input)?;
```

### 类型定义

```rust
pub enum PluginState {
    Loaded,
    Active,
    Inactive,
    Error,
}

pub enum PluginKind {
    Wasm,
    Sidecar,
}

pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
    pub metadata: PluginMetadata,
    pub state: PluginState,
}
```

---

## 事件系统

### PluginEventType

可订阅的事件类型。

```rust
pub enum PluginEventType {
    PluginLoaded,
    PluginActivated,
    PluginDeactivated,
    PluginUnloaded,
    BeforeSqlQuery,
    AfterSqlQuery,
    MetadataChanged,
    Custom(String),
}
```

### 事件订阅

```rust
use crate::core::plugin::events::EventManager;
use crate::core::plugin::events::PluginEvent;

let event_manager = EventManager::new();

event_manager.subscribe(
    PluginEventType::AfterSqlQuery,
    Arc::new(|event| {
        println!("SQL executed: {:?}", event.data);
    })
);
```

### 事件发布

```rust
let event = PluginEvent::new(
    PluginEventType::AfterSqlQuery,
    Some("plugin-id".to_string()),
    Some(serde_json::json!({
        "sql": "SELECT * FROM users",
        "rows": 100
    })),
);

event_manager.publish(&amp;event);
```

---

## 存储 API

### PluginStorage

插件存储管理。

```rust
use crate::core::plugin::storage::PluginStorage;

let storage = PluginStorage::new("/path/to/storage".into());

// 设置值
storage.set("plugin-id", "key", "value".to_string());

// 获取值
let value = storage.get("plugin-id", "key");

// 序列化存储
storage.set_serialized("plugin-id", "config", &amp;my_config)?;

// 读取并反序列化
let config: MyConfig = storage.get_deserialized("plugin-id", "config")?
    .ok_or("Not found")?;

// 删除
storage.delete("plugin-id", "key");

// 清除所有数据
storage.clear_plugin_data("plugin-id");
```

### 示例

```rust
// 在插件中使用存储
#[tauri::command]
async fn save_plugin_config(
    plugin_id: String,
    config: serde_json::Value
) -> Result&lt;(), CoreError&gt; {
    let storage = PluginStorage::new(/* ... */);
    storage.set_serialized(&amp;plugin_id, "config", &amp;config)?;
    Ok(())
}
```

---

## 开发示例

### 完整工作流

```typescript
// 1. 添加插件目录
await invoke('plugin_add_directory', {
  directory: '~/.rdata-station/plugins',
})

// 2. 加载插件
await invoke('plugin_load', {
  pluginId: 'com.example.plugin',
  pluginPath: '~/.rdata-station/plugins/example',
})

// 3. 激活插件
await invoke('plugin_activate', {
  pluginId: 'com.example.plugin',
})

// 4. 检查状态
const status = await invoke('plugin_get_status', {
  pluginId: 'com.example.plugin',
})
console.log('Plugin status:', status)

// 5. 使用插件功能
const result = await invoke('plugin_db_query', {
  pluginId: 'com.example.plugin',
  connId: 'my-connection',
  sql: 'SELECT * FROM data',
})
```

### 错误处理

```typescript
try {
  await invoke('plugin_load', {
    /* ... */
  })
} catch (e) {
  console.error('Plugin load failed:', e)
  // 处理错误
}
```

---

## 扩展阅读

- [PLUGIN_SYSTEM.md](./PLUGIN_SYSTEM.md) - 架构文档
- [PLUGIN_DEVELOPMENT.md](./PLUGIN_DEVELOPMENT.md) - 开发指南
