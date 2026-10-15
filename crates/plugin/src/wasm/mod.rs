//! WASM Adapter 模块
//!
//! 负责 WASM 插件系统的适配，支持：
//! - 插件加载与管理
//! - 插件与 Core 的通信
//! - 数据分析插件支持
//!
//! # 架构设计
//!
//! ```text
//! WASM Plugin (Rust/C/Go compiled to WASM)
//!     │
//!     ▼
//! WASM Runtime (Extism)
//!     │
//!     ▼
//! WASM Adapter (adapters/wasm/)
//!     │
//!     ▼
//! Core Services (core/services/)
//! ```
//!
//! # 插件类型
//!
//! 1. **分析插件**: 数据可视化、统计分析
//! 2. **驱动插件**: 自定义数据库驱动
//! 3. **工具插件**: SQL 格式化、代码生成

use serde::Serialize;

// 导出子模块
pub mod api;
pub mod extism;
// 注：`host_functions` 已于 P0 删除。原有 4 个 host function 全是“无条件返回错误”的占位
// （签名也不对：Extism 读宿主内存要走 `host_fn!` + `Memory`，不是 Val 数组），且全仓零调用。
// 回收口径：**现在不提供任何 host function**（wasm 插件拿不到宿主能力，与“默认拒绝”一致）；
// 真正的 host function 面在 P3 按 Q4 收敛后重建。
//
// `plugin_manager.rs`（AdvancedPluginManager / PluginSandbox / ResourceLimits）已于 2026-10-15
// 删除：与 `ExtismPluginManager` 职责重复且**全仓零调用**，沙箱资源计量是 `cpu_time_used_ms += 1`
// 的假数据。wasm 沙箱/资源限制的真相归 P3 与 `CapabilitiesWasm`（max_memory_mb / max_cpu_time_ms /
// allowed_host_functions）一起重建 —— 那是「运行期强制」，不是一张自维护的表。

// 重新导出核心类型
pub use self::extism::ExtismPluginManager;

/// 插件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PluginType {
    /// 分析插件
    Analytics,
    /// 驱动插件
    Driver,
    /// 工具插件
    Tool,
}

/// 插件元数据
#[derive(Debug, Clone, Serialize)]
pub struct PluginMetadata {
    /// 插件 ID
    pub id: String,
    /// 插件名称
    pub name: String,
    /// 插件版本
    pub version: String,
    /// 插件类型
    pub plugin_type: PluginType,
    /// 插件描述
    pub description: String,
    /// 作者
    pub author: String,
    /// 入口函数
    pub entry_point: String,
}
