//! 草稿箱视图的**宿主端口**（`workbench` 实现并注入）。
//!
//! ## 为什么需要
//!
//! 草稿箱视图从 `workbench` 下沉到本 crate 后，不能再看见 `Shared` 与工作台组件。
//! 沿用本仓已有的三个正例（`mock::MockHost` / `insight` 取数端口 / `database::NavHost`）：
//! 视图只拿一个**本 crate 定义**的 trait。
//!
//! ## 边界怎么划
//!
//! 端口只收「换个宿主还成立吗」这个判据下真正需要宿主的能力：
//!
//! | 能力 | 去向 | 理由 |
//! | --- | --- | --- |
//! | 草稿库读写（`ScratchpadStore`）、回收站、文件监控 | **不进端口** | `scratchpad` 自己的设施，只需项目根 |
//! | 读盘 / 导入 / 搜索 / 替换等重活 | **不进端口** | 走 [`crate::jobs`] 的工作线程 |
//! | 项目根、只读判定 | 端口读取器 | 宿主自持的会话状态（`project_ui`） |
//! | 状态栏提示、宿主重绘 | 端口 | 提示与模态层都在宿主 |
//! | 内容搜索结果展示、在编辑器中打开文件 | 端口 | 结果与文件都在中央编辑区，属宿主 |
//!
//! 注入方式：`ScratchpadView::new(host, cx)`。宿主实现见
//! `crates/workbench/src/components/scratchpad_host.rs`。

use std::path::PathBuf;

use gpui_kit::App;

use crate::scratchpad_view::ScratchpadSearchView;

/// 宿主注入的能力。
pub trait ScratchpadHost: 'static {
    /// 当前项目根（未打开项目为 `None`；草稿箱是项目级能力，此时按空态呈现）。
    fn project_root(&self) -> Option<PathBuf>;

    /// 项目只读？写操作（新建 / 重命名 / 删除 / 粘贴 / 改引用）据此拒绝。
    fn read_only(&self) -> bool;

    /// 状态栏提示（一行文本）。
    fn notice(&self, message: String, cx: &mut App);

    /// 让宿主重绘（模态层挂在宿主 render 上；只 `cx.notify` 子视图不够）。
    fn notify_host(&self, cx: &mut App);

    /// 把内容搜索结果投给中央编辑区（`None` = 清空）。
    fn show_search_results(&self, view: Option<ScratchpadSearchView>, cx: &mut App);

    /// 在中央编辑器里打开一个文件（**绝对路径**；编辑器按路径自己判定模式与只读等级）。
    fn open_in_editor(&self, path: PathBuf);

    /// 当前**有未保存修改**的文件（绝对路径）：草稿树在这些条目上打脏点。
    ///
    /// 由宿主从编辑器的文档集合取（草稿箱不依赖 `editor`）；未接编辑器或没有脏文档时为空集。
    /// 默认实现返回空集——不接编辑器的宿主（测试 / 其他入口）不用实现它。
    fn dirty_files(&self) -> std::collections::HashSet<PathBuf> {
        std::collections::HashSet::new()
    }
}
