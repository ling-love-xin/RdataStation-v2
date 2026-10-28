//! 语言服务：SQL 补全 + 诊断 / 缩进 / 扫描 + LSP 客户端

/// 【B9】SQL 补全（上下文判定 + 候选挑排 + 内建 provider 适配 + 模板片段）
pub mod completion;
/// 【B6/B18】语言服务（诊断 / 缩进 / 扫描合并 / 方言段）
pub mod language_service;
