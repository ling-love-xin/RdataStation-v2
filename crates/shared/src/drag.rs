//! 跨 crate 的拖放载荷。
//!
//! 拖放**类型**必须被拖起方与落点方同时看见，而两边分属不同特性 crate（互不依赖），
//! 所以这类载荷住在最底层的 `shared`：只装数据，不含任何 UI。

use std::path::PathBuf;

/// 「把这个文件的内容插到落点」——草稿箱树 → 中央编辑器（原型 §4.5）。
///
/// **只带路径，不带内容**：拖拽开始时读盘会把整个文件塞进拖拽幽灵，而用户可能中途放弃；
/// 内容由落点（编辑器）在真正落下时读。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertFileDrag {
    /// 展示文案（拖拽幽灵用；通常是文件名）。
    pub label: String,
    /// 要插入内容的文件（绝对路径）。
    pub path: PathBuf,
}
