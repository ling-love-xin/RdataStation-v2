//! 项目态端口的工作台实现：把“项目只读”（标题栏的锁）递给编辑器
//!
//! ## 职责边界
//!
//! 编辑器只问一件事：“项目现在锁着吗？”（`crate::project::ProjectPort`）。**真值在项目模块**
//! （`project::ui::ProjectUiState::read_only`，与资产库 / 草稿箱 / 连接对话框的写保护同一判据），
//! 这里只做一件事：把那份真值**按内存快照**递过去——渲染路径每帧都会调它，不能做 I/O。
//!
//! ## 为什么是端口而不是字段
//!
//! `editor` 不依赖 `workbench`（分层硬约束），而项目锁是窗口级状态；把锁写进 `Document`
//! 会留下“解锁了但文档还写着只读”的陈旧值——所以每次现读（`borrow` 一次，代价可忽略）。
//!
//! P2-a：经 [`crate::host::EditorHost::project_read_only`] 转发（宿主在 workbench
//! 侧读 `Shared.project_ui`），不再依赖 `Shared` 具体类型。

use std::rc::Rc;

use crate::host::EditorHost;
use crate::project::{ProjectPort, ProjectState};
use crate::shared::EditorShared;

/// 工作台实现：项目只读（读内存快照，不做 I/O）
struct WorkbenchProject {
    host: Rc<dyn EditorHost>,
}

impl ProjectPort for WorkbenchProject {
    fn state(&self) -> ProjectState {
        ProjectState {
            read_only: self.host.project_read_only(),
        }
    }
}

/// 把项目态端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &EditorShared, host: Rc<dyn EditorHost>) {
    shared.attach_project(Rc::new(WorkbenchProject { host }));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use std::path::PathBuf;

    use super::WorkbenchProject;
    use crate::host::EditorHost;
    use crate::project::{ProjectPort, ProjectState};
    use gpui_kit::App;
    use workbench_shell::model::ConnectionItem;

    /// 测试宿主：只记录 `project_read_only` 真值（不接真实项目模块）
    struct StubHost {
        read_only: bool,
    }

    impl EditorHost for StubHost {
        fn connections(&self) -> Vec<ConnectionItem> {
            Vec::new()
        }
        fn project_root(&self) -> Option<PathBuf> {
            None
        }
        fn project_read_only(&self) -> bool {
            self.read_only
        }
        fn set_connection_connected(&self, _conn_id: &str, _connected: bool) {}
        fn is_connected(&self, _conn_id: &str) -> bool {
            false
        }
        fn connect_entry(&self, _conn_id: &str) -> Result<(), String> {
            Err("测试宿主未接入".to_string())
        }
        fn open_history(&self, _cx: &mut App) {}
        fn open_insight_source_column(&self, _request: crate::shared::InsightColumnRequest, _cx: &mut App) {}
        fn request_reveal(&self, _object: engine::ObjectRef) {}
    }

    /// 项目锁的真值跟着宿主走（不是缓存值）
    #[test]
    fn the_project_lock_is_read_through() {
        let port = WorkbenchProject {
            host: std::rc::Rc::new(StubHost { read_only: false }),
        };
        assert_eq!(port.state(), ProjectState { read_only: false });

        let port = WorkbenchProject {
            host: std::rc::Rc::new(StubHost { read_only: true }),
        };
        assert_eq!(
            port.state(),
            ProjectState { read_only: true },
            "解锁/上锁都该立刻反映（不留缓存）"
        );
    }
}
