# 插件开发指南

本指南将帮助你为 RdataStation 开发插件。

## 快速开始

### 1. 创建插件项目

#### WASM 插件

```
my-plugin/
├── plugin.toml
└── src/
    └── lib.rs
```

#### Go Sidecar 插件

```
my-plugin/
├── plugin.toml
└── src/
    └── main.go
```

### 2. 编写插件清单

**plugin.toml**

```toml
id = "com.example.myplugin"
name = "My Plugin"
version = "0.1.0"
description = "A description of my plugin"
author = "Your Name"
plugin_type = "Wasm"  # 或 "Sidecar"

[contributes]
drivers = []
commands = ["myplugin.doSomething"]
panels = []

[config]
api_key = ""
```

## WASM 插件开发 (Rust)

### Cargo.toml

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
extism-pdk = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### lib.rs

```rust
use extism_pdk::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct MyRequest {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MyResponse {
    pub result: String,
}

#[plugin_fn]
pub fn greet(input: String) -> FnResult<String> {
    Ok(format!("Hello, {}!", input))
}

#[plugin_fn]
pub fn activate() -> FnResult<()> {
    log!("Plugin activated!");
    Ok(())
}

#[plugin_fn]
pub fn deactivate() -> FnResult<()> {
    log!("Plugin deactivated!");
    Ok(())
}

#[plugin_fn]
pub fn process(input: Json<MyRequest>) -> FnResult<Json<MyResponse>> {
    Ok(Json(MyResponse {
        result: format!("Processed: {}", input.message),
    }))
}
```

## Go Sidecar 插件开发

### main.go

```go
package main

import (
    "encoding/json"
    "fmt"
    "net/http"
)

type Request struct {
    Method string          `json:"method"`
    Params json.RawMessage `json:"params"`
}

type Response struct {
    Result json.RawMessage `json:"result,omitempty"`
    Error  *string         `json:"error,omitempty"`
}

func main() {
    http.HandleFunc("/", handler)
    fmt.Println("Sidecar running on :8080")
    http.ListenAndServe(":8080", nil)
}

func handler(w http.ResponseWriter, r *http.Request) {
    var req Request
    if err := json.NewDecoder(r.Body).Decode(&amp;req); err != nil {
        sendError(w, err.Error())
        return
    }

    switch req.Method {
    case "ping":
        sendResult(w, map[string]string{"status": "ok"})
    case "activate":
        sendResult(w, map[string]bool{"success": true})
    case "deactivate":
        sendResult(w, map[string]bool{"success": true})
    default:
        sendError(w, "Unknown method")
    }
}

func sendResult(w http.ResponseWriter, result interface{}) {
    resp := Response{Result: toJSON(result)}
    json.NewEncoder(w).Encode(resp)
}

func sendError(w http.ResponseWriter, err string) {
    resp := Response{Error: &amp;err}
    json.NewEncoder(w).Encode(resp)
}

func toJSON(v interface{}) json.RawMessage {
    b, _ := json.Marshal(v)
    return b
}
```

## 安装插件

1. 将插件文件放入插件目录：

```
~/.rdata-station/plugins/
└── my-plugin/
    ├── plugin.toml
    └── my_plugin.wasm
```

2. 在 RdataStation 中通过 UI 或命令加载插件：

```typescript
import { invoke } from '@tauri-apps/api/core'

// 加载插件
await invoke('plugin_load', {
  pluginId: 'com.example.myplugin',
  pluginPath: '~/.rdata-station/plugins/my-plugin',
})

// 激活插件
await invoke('plugin_activate', {
  pluginId: 'com.example.myplugin',
})
```

## 前端集成

### 注册插件命令

```typescript
// 在前端应用中
import { invoke } from '@tauri-apps/api/core'

export async function callMyPlugin(data: string) {
  return await invoke('plugin_call', {
    pluginId: 'com.example.myplugin',
    method: 'process',
    input: { message: data },
  })
}
```

### 添加插件面板

```tsx
// plugin-panel.tsx
import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export function MyPluginPanel() {
    const [data, setData] = useState('');
    const [result, setResult] = useState('');

    const handleClick = async () => {
        const response = await invoke('plugin_call', {
            pluginId: 'com.example.myplugin',
            method: 'process',
            input: data
        });
        setResult(response.result);
    };

    return (
        <div>
            <h2>My Plugin</h2>
            <input
                value={data}
                onChange={e =&gt; setData(e.target.value)}
                placeholder="Input message"
            /&gt;
            <button onClick={handleClick}>Process</button>
            {result &amp;&amp; <p>Result: {result}</p>}
        </div>
    );
}
```

## 事件订阅

插件可以监听系统事件：

```rust
// 在你的插件中
#[plugin_fn]
pub fn on_sql_executed(event: Json<SqlExecutedEvent>) -> FnResult<()> {
    log!("SQL executed: {:?}", event.sql);
    Ok(())
}
```

前端也可以监听事件：

```typescript
import { listen } from '@tauri-apps/api/event'

await listen('plugin-event', event => {
  console.log('Plugin event:', event.payload)
})
```

## 数据存储

使用插件存储 API 持久化数据：

```rust
#[plugin_fn]
pub fn save_setting(key: String, value: String) -> FnResult<()> {
    // 通过宿主函数存储数据
    let _ = host::call("storage.set", json!({
        "key": key,
        "value": value
    }));
    Ok(())
}
```

## 调试技巧

1. **日志查看**：插件日志会输出到应用日志
2. **错误信息**：调用失败会返回详细的错误信息
3. **状态检查**：使用 `plugin_get_status` 查看插件状态

## 最佳实践

1. **错误处理**：总是返回详细的错误信息
2. **性能优化**：尽量减少与宿主的通信
3. **内存管理**：WASM 插件注意内存限制
4. **版本兼容性**：遵循语义化版本规范

## 插件发布

完成开发后，可以：

1. 在 GitHub 上发布插件
2. 提交到插件市场（未来功能）
3. 分享文档和使用示例
