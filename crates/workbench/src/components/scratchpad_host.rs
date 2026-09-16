//! 草稿箱的宿主端口实现（`scratchpad::ScratchpadHost`）。
//!
//! 与 `nav_host.rs` / `mock_host.rs` 同形：特性 crate 定义 trait，宿主持 `Shared`
//! 并实现它，装配期注入面板（`ScratchpadView::new`）。
//!
//! 实现原则——**只做转接，不加戏**：项目根 / 只读判定 / 提示 / 重绘 / 搜索结果落地 /
//! 在编辑器中打开文件 / 脏文档集合，全部直接落到 `Shared` 的对应字段与端口。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::App;

use editor::shared::EditorShared;
use scratchpad::{ScratchpadHost, ScratchpadSearchView};

use crate::panels::Shared;

/// 宿主端口实现（无状态外壳，状态全在 [`Shared`] 与编辑器共享状态）。
pub struct WorkbenchScratchpadHost {
    shared: Shared,
    /// 编辑器共享状态：只为「脏点」读一份文档集合（草稿箱不播编辑器能力）。
    editor: EditorShared,
}

impl ScratchpadHost for WorkbenchScratchpadHost {
    fn project_root(&self) -> Option<PathBuf> {
        self.shared.project_root()
    }

    fn read_only(&self) -> bool {
        self.shared.project_ui.borrow().read_only
    }

    fn notice(&self, message: String, cx: &mut App) {
        *self.shared.notice.borrow_mut() = Some(message);
        // 状态栏由宿主渲染，子视图 notify 不会重绘它，故这里显式请求宿主重绘。
        self.shared.notify_host(cx);
    }

    fn notify_host(&self, cx: &mut App) {
        self.shared.notify_host(cx);
    }

    fn show_search_results(&self, view: Option<ScratchpadSearchView>, cx: &mut App) {
        // 结果的渲染在编辑区（`EditorPanel`）；草稿箱不再回读这份展示数据。
        self.shared.show_scratchpad_search(view, cx);
    }

    fn open_in_editor(&self, path: PathBuf) {
        self.shared.request_open_in_editor(path);
    }

    /// 脏点：把编辑器里带未保存修改的文档路径报给草稿箱。
    ///
    /// 不过滤“是否在草稿模块内”——视图侧用条目自己的绝对路径比对，天然只命中模块内的条目。
    fn dirty_files(&self) -> HashSet<PathBuf> {
        let service = self.editor.service();
        service
            .dirty_ids()
            .into_iter()
            .filter_map(|id| service.find(&id).and_then(|doc| doc.path().map(Path::to_path_buf)))
            .collect()
    }
}

/// 构造宿主端口（装配期调用；生产入口 `SidebarPanel::new`）。
pub fn build_host(shared: &Shared, editor: &EditorShared) -> Rc<dyn ScratchpadHost> {
    Rc::new(WorkbenchScratchpadHost {
        shared: shared.clone(),
        editor: editor.clone(),
    })
}
