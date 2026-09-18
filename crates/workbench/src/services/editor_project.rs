//! 项目态端口的工作台实现：把“项目只读”（标题栏的锁）递给编辑器
//!
//! ## 职责边界
//!
//! 编辑器只问一件事：“项目现在锁着吗？”（`editor::project::ProjectPort`）。**真值在项目模块**
//! （`project::ui::ProjectUiState::read_only`，与资产库 / 草稿箱 / 连接对话框的写保护同一判据），
//! 这里只做一件事：把那份真值**按内存快照**递过去——渲染路径每帧都会调它，不能做 I/O。
//!
//! ## 为什么是端口而不是字段
//!
//! `editor` 不依赖 `workbench`（分层硬约束），而项目锁是窗口级状态；把锁写进 `Document`
//! 会留下“解锁了但文档还写着只读”的陈旧值——所以每次现读（`borrow` 一次，代价可忽略）。

use editor::project::{ProjectPort, ProjectState};
use editor::shared::EditorShared;

use crate::panels::Shared;

/// 工作台实现：项目只读（读内存快照，不做 I/O）
struct WorkbenchProject {
    shared: Shared,
}

impl ProjectPort for WorkbenchProject {
    fn state(&self) -> ProjectState {
        ProjectState {
            read_only: self.shared.project_ui.borrow().read_only,
        }
    }
}

/// 把项目态端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &EditorShared, workbench: &Shared) {
    shared.attach_project(std::rc::Rc::new(WorkbenchProject {
        shared: workbench.clone(),
    }));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::WorkbenchProject;
    use editor::project::{ProjectPort, ProjectState};

    use crate::panels::Shared;

    /// 项目锁的真值跟着 `project_ui` 走（不是缓存值）
    #[test]
    fn the_project_lock_is_read_through() {
        let shared = Shared::with_connections(Vec::new(), None);
        let port = WorkbenchProject {
            shared: shared.clone(),
        };
        assert_eq!(port.state(), ProjectState { read_only: false });

        shared.project_ui.borrow_mut().read_only = true;
        assert_eq!(
            port.state(),
            ProjectState { read_only: true },
            "解锁/上锁都该立刻反映（不留缓存）"
        );
    }
}
