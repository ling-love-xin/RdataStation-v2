//! 资产库面板的宿主桥（M6）。
//!
//! `rds-analytics-resource` 自带面板视图（`resource_view::ResourcesPanel`）但不依赖 workbench：
//! 工作台在这里把「重绘 / 只读判定 / 动作去向」注入为 [`ResourcesHost`]——与 `mock_host`
//! 同一形态（crate 持有视图与状态，宿主只装配）。
//!
//! **动作类请求本批只给明确回执**（状态栏提示），不接半条链路：
//! - 归档对话框（P1.4）、取回对话框（P1.5）：属 Phase 1 对话框批；
//! - 打开（只读）：需编辑器侧"分析资源锁定"只读来源（P1.6）；
//! - 移入回收站：一律走项目级 `ProjectTrash`，而上提尚未落地（P0.8）——**不做**先软删
//!   再等回收站那条（会变成两套回收站，违反模块硬约束 5）；
//! - 索引修复对话框：Phase 3（异常计数本批已在状态行可见）。
//!
//! 回执而不是空操作：面板上的按钮是既有入口，点了没反应比"明确说还没接入"更难排查。

use std::rc::Rc;

use gpui_kit::{App, Window};

use analytics_resource::resource_view::ResourcesHost;

use crate::panels::Shared;

/// 宿主桥：动作转发到 `Shared` 的状态栏提示（无自有状态）。
struct WorkbenchResourceHost {
    shared: Shared,
}

impl WorkbenchResourceHost {
    /// 给一次明确回执（写入 `Shared::notice` 并请求宿主重绘）。
    fn pending(&self, action: &str, detail: &str, cx: &mut App) {
        *self.shared.notice.borrow_mut() = Some(format!("资产库：{action}{detail}"));
        self.shared.notify_host(cx);
    }
}

impl ResourcesHost for WorkbenchResourceHost {
    fn request_archive(&self, _window: &mut Window, cx: &mut App) {
        self.pending(
            "归档尚未接入",
            "（对话框属 Phase 1 对话框批；归档由草稿箱发起，落点已就绪）",
            cx,
        );
    }

    fn request_open(&self, _resource_id: &str, _window: &mut Window, cx: &mut App) {
        self.pending("只读打开尚未接入", "（需编辑器只读来源，P1.6）", cx);
    }

    fn request_checkout(&self, _resource_id: &str, _window: &mut Window, cx: &mut App) {
        self.pending("取回尚未接入", "（对话框属 Phase 1 对话框批，P1.5）", cx);
    }

    fn request_delete(&self, _resource_id: &str, _window: &mut Window, cx: &mut App) {
        self.pending(
            "移入回收站尚未接入",
            "（等项目级回收站上提，P0.8；不做两套回收站）",
            cx,
        );
    }

    fn request_index_repair(&self, _window: &mut Window, cx: &mut App) {
        self.pending(
            "索引修复对话框尚未接入",
            "（Phase 3；异常计数已在状态行显示）",
            cx,
        );
    }
}

/// 组装资产库面板宿主（面板实体创建时调用一次）。
pub fn build_host(shared: &Shared) -> Rc<dyn ResourcesHost> {
    Rc::new(WorkbenchResourceHost {
        shared: shared.clone(),
    })
}
