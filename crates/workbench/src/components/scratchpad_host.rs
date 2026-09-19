//! 草稿箱的宿主端口实现（`scratchpad::ScratchpadHost`）。
//!
//! 与 `nav_host.rs` / `mock_host.rs` 同形：特性 crate 定义 trait，宿主持 `Shared`
//! 并实现它，装配期注入面板（`ScratchpadView::new`）。
//!
//! 实现原则——**只做转接，不加戏**：项目根 / 只读判定 / 提示 / 重绘 / 搜索结果落地 /
//! 在编辑器中打开文件 / 脏文档集合 / 洞察「查看统计」（路径 → 取样来源 → 右 Dock 面板），
//! 全部直接落到 `Shared` 的对应字段与端口。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::App;

use editor::shared::EditorShared;
use scratchpad::{ScratchpadDiffView, ScratchpadHost, ScratchpadSearchView};

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

    /// 「查看统计」的可行性判定：与取样共**同一处口径**（DuckDB 的读取器映射）。
    ///
    /// 判定与执行各写一份，迟早会出现“菜单亮着但点了报不支持”。
    fn can_view_stats(&self, path: &Path) -> bool {
        engine::file_reader_function(&path.display().to_string()).is_some()
    }

    /// 洞察落地面板（M8）：路径 → 取样来源 → 右 Dock 的洞察面板（D58）。
    fn view_stats(&self, path: PathBuf, label: String, cx: &mut App) {
        match insight::SampleSource::duckdb_file(&path, label.clone()) {
            Ok(source) => self.shared.open_insight_source_table(source, label, cx),
            // 判定已经挡过（`can_view_stats`）：走到这里是可分析格式但读不到
            // （路径刚被挪走 / 删了），把真实原因说清。
            Err(error) => self.notice(format!("草稿箱：{error}"), cx),
        }
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

    fn draft_content(&self, path: &Path) -> Option<String> {
        let service = self.editor.service();
        let id = service.find_by_path(path)?.clone();
        service.find(&id).map(|doc| doc.content().to_string())
    }

    /// 「照磁盘重载」：读盘 → 替掉缓冲区 → 清脏。
    ///
    /// 先读盘再改文档（顺序与 `persist::save_document` 的“先写盘后清脏”同一口径：
    /// 失败的一步不能留下“已经同步”的假状态）。
    fn reload_draft(&self, path: &Path) -> Result<(), String> {
        let content = editor::persist::load(path).map_err(|e| e.to_string())?;
        let service = self.editor.service();
        let Some(id) = service.find_by_path(path).cloned() else {
            return Err("该文件未在编辑器中打开".to_string());
        };
        drop(service);
        self.editor.update(|service| {
            service.set_content(&id, content);
            service.mark_saved(&id);
        });
        Ok(())
    }

    fn show_diff(&self, view: Option<ScratchpadDiffView>, cx: &mut App) {
        self.shared.show_scratchpad_diff(view, cx);
    }
}

/// 构造宿主端口（装配期调用；生产入口 `SidebarPanel::new`）。
pub fn build_host(shared: &Shared, editor: &EditorShared) -> Rc<dyn ScratchpadHost> {
    Rc::new(WorkbenchScratchpadHost {
        shared: shared.clone(),
        editor: editor.clone(),
    })
}
