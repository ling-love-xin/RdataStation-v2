//! 项目态端口（今天只有一项事实：**项目只读**）
//!
//! ## 为什么要有它
//!
//! “项目只读”是**宿主的事实**（项目锁在标题栏上，`Shared::project_ui.read_only`），
//! 而 `editor` 不得依赖 `workbench` → 按老规矩走端口注入（与连接 / 通道 / 源清单同一套）。
//! 这条闸门在 B12 删旧 SQL 面板时遗失过（旧路径有、新入口没有），这里补回。
//!
//! ## 语义（原型 §1.4 的“连接只读”维度）
//!
//! 项目只读 = **能编辑、能跑读语句，写源库对象的语句被拒**：
//!
//! - 编辑区照常可输入（那是「编辑器只读」维度的事，两者互不蕴含）；
//! - `SELECT` / `WITH … SELECT` 照跑；
//! - `INSERT` / `UPDATE` / `DELETE` / DDL **在提交前就被拒**，原因可读，不打扰执行器。
//!
//! ## 未注入时不拦
//!
//! 端口没接（单元测试 / 宿主不关心项目态）时按“不读只”处理——与 1b 之前的行为一致；
//! 注入之后必须**如实**（宿主读的是内存快照，渲染路径可调，不做 I/O）。

use std::rc::Rc;

/// 项目态快照（**纯数据**：渲染路径可调）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProjectState {
    /// 项目为只读模式（标题栏的锁）：写源库对象的语句一律拒
    pub read_only: bool,
}

/// 项目态端口（宿主注入一次）
pub trait ProjectPort: 'static {
    fn state(&self) -> ProjectState;
}

/// 共享句柄（面板 clone 的是它）
pub type ProjectHandle = Rc<dyn ProjectPort>;
