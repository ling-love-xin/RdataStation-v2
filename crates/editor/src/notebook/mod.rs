//! 分析模式数据层：单元 + 会话 + 内核端口 + mime 分发
//!
//! 视图在 `view/analysis.rs`；状态持有在 `EditorShared` 的笔记本持有器（每文档一份）。

/// 【2d】单元模型（Cell / Output / 状态机）与 `.rdsnote` 落盘形状（v2）
pub mod cell;
/// 【#20】内核注册表端口（宿主注入）与 `resolve_kernel` 解析纯函数
pub mod kernel;
/// 【#14】Jupyter mime bundle → 原生输出的纯函数分发器
pub mod media;
/// 【2d】分析会话（绑定连接 / 环境 / 上次用过的内核；切换 → 输出全 stale）
pub mod session;
