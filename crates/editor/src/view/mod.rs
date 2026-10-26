//! 编辑器视图层
//!
//! **一个面板 = 一个标签 = 一份文档**：多文档由 Dock 的 tab 组承载（架构 D13 / P0.1 结论），
//! 不自绘标签条。每个面板只负责三件事——标题（文档名）、`title_suffix`（脏点）、正文（编辑内核）。

pub mod analysis;
pub mod completion;
pub mod dialogs;
pub mod encoding_dialog;
pub mod fold;
pub mod function_manual;
pub mod highlight;
pub mod history;
pub mod host;
pub mod manage_env;
pub mod results;
pub mod signature_help;
pub mod widgets;

#[cfg(test)]
mod tests;
