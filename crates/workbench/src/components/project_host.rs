//! 项目视图的宿主桥（M1 / A3）
//!
//! `project` crate 自带项目视图（`project::ui`），但不依赖 workbench 与 settings；
//! 工作台在这里把 `Shared` 的状态句柄、视图重绘、编辑区、排序偏好与打开后刷新
//! 注入为 `ProjectUiHost`。

use std::rc::Rc;

use gpui_kit::*;

use crate::panels::Shared;
use crate::view::WorkbenchView;

/// 重绘桥：把 `WeakEntity<WorkbenchView>` 包成项目视图可调用的 notifier。
///
/// 用弱引用以避免宿主 ↔ 视图的引用环（host 存在 `WorkbenchView` 字段里）。
struct ViewNotifier(WeakEntity<WorkbenchView>);

impl project::ui::ProjectUiNotifier for ViewNotifier {
    fn notify(&self, cx: &mut App) {
        let _ = self.0.update(cx, |_, cx| cx.notify());
    }
}

/// 编辑区桥：读 `Shared` 的编辑区脏标记 / 草稿内容，转发清空命令。
struct EditorBridge(Shared);

impl project::ui::ProjectEditorBridge for EditorBridge {
    fn is_dirty(&self) -> bool {
        self.0.editor_dirty.get()
    }

    fn sql(&self) -> String {
        self.0.editor_sql.borrow().clone()
    }

    fn clear(&self, window: &mut Window, cx: &mut App) {
        let clear = self.0.editor_clear.borrow().clone();
        if let Some(clear) = clear {
            clear(window, cx);
        }
    }

    fn mark_clean(&self) {
        self.0.editor_dirty.set(false);
    }
}

/// 组装项目视图宿主（在 `WorkbenchView::new` 中调用一次）。
pub fn build_host(
    shared: &Shared,
    entity: WeakEntity<WorkbenchView>,
) -> project::ui::ProjectUiHost {
    let save_sort: Rc<dyn Fn(project::ui::ProjectSort, &mut App)> =
        Rc::new(|sort, cx| settings::SettingsService::set_project_sort_mode(sort.key(), cx));

    let on_opened = {
        let shared = shared.clone();
        Rc::new(move |cx: &mut App| refresh_after_open(&shared, cx))
    };

    project::ui::ProjectUiHost::new(
        shared.project_ui.clone(),
        shared.project.clone(),
        Rc::new(ViewNotifier(entity)),
    )
    .with_editor(Rc::new(EditorBridge(shared.clone())))
    .with_sort_saver(save_sort)
    .with_on_opened(on_opened)
}

/// 打开项目后的宿主刷新：连接列表、选中项、导航缓存与结果归属都归零，
/// 避免残留上一项目的数据。
fn refresh_after_open(shared: &Shared, _cx: &mut App) {
    // 项目打开/切换后：列表按作用域合一（全局 + 该项目 P_/GP_）。
    let root = shared.project.borrow().as_ref().map(|p| p.root.clone());

    // M8：告知洞察规则监听器当前项目——监听器每轮读这个值现算目录，
    // 从而自动跟随项目切换（否则会一直看着启动时那个项目）。
    insight::set_watched_project_root(root.clone());

    let (conns, notice) =
        crate::services::workspace_loader::load_connections_for_scope(root.as_deref());
    *shared.connections.borrow_mut() = conns;
    *shared.notice.borrow_mut() = notice;
    let has = !shared.connections.borrow().is_empty();
    shared.selected.set(if has { Some(0) } else { None });
    *shared.nav_for.borrow_mut() = None;
    shared.nav_tables.borrow_mut().clear();
    *shared.sql_for.borrow_mut() = None;
}
