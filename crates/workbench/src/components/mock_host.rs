//! Mock 生成面板的宿主桥（M7）。
//!
//! mock crate 自带面板视图（`mock::mock_view`），但不依赖 workbench 与 engine 的文件布局；
//! 工作台在这里把「从哪读、往哪写、怎么重绘」注入为 [`MockHost`]：
//!
//! | 宿主能力 | 实现 |
//! | --- | --- |
//! | 后台任务（生成 / 追加） | `services::mock_jobs`（工作线程 + 进度槽 + 取消） |
//! | 落库 / 导出 / 草稿箱 | `services::mock_generator`（装配层） |
//! | 连接清单（导入结构来源） | `Shared::connections`（工作台当前连接列表） |
//! | 既有分析库表 / 导入列结构 | `services::mock_generator` → `NavCache` / `MetadataService` |
//! | 只读判定 | `Shared.project_ui.read_only`（与 SQL 执行入口同一护栏） |
//! | 打开详情 tab | `Shared::open_mock_detail`（宿主命令，接中央 Dock） |
//! | 重绘 | `Shared::notify_host`（宿主重绘桥，与连接对话框层同一口径） |
//!
//! 落库成功后让导航树失效（`Shared::nav_for` 置空）：下一次渲染会重新加载分析库对象，
//! 否则新表要等下次切连接才出现。

use std::rc::Rc;

use gpui_kit::{App, Window};
use mock::mock_view::{
    MockColumnSpec, MockDraft, MockGenInfo, MockHost, MockJobDone, MockJobKind, MockJobState,
    SchemaRequest, SchemaSource,
};
use mock::models::MockExportFormat;

use crate::panels::Shared;

/// 宿主桥：全部能力转发到装配层服务与 `Shared`（无自有状态）。
struct WorkbenchMockHost {
    shared: Shared,
}

impl WorkbenchMockHost {
    /// 项目根（未打开项目时为 `None`）。
    fn project_root(&self) -> Option<std::path::PathBuf> {
        self.shared
            .project
            .borrow()
            .as_ref()
            .map(|session| session.root.clone())
    }

    /// 落库 / 追加成功：让分析库导航树失效（下一帧按 `nav_for` 重新加载）。
    fn invalidate_analysis_nav(&self) {
        *self.shared.nav_for.borrow_mut() = None;
    }
}

impl MockHost for WorkbenchMockHost {
    fn start_job(&self, draft: &MockDraft, kind: MockJobKind) -> Result<(), String> {
        crate::services::mock_jobs::start(
            draft,
            kind,
            &crate::services::mock_generator::analytics_db_path(),
        )
    }

    fn job_state(&self) -> MockJobState {
        crate::services::mock_jobs::state()
    }

    fn take_job_done(&self) -> Option<Result<MockJobDone, String>> {
        crate::services::mock_jobs::take_done()
    }

    fn cancel_job(&self) {
        crate::services::mock_jobs::cancel();
    }

    fn persist_table(&self, draft: &MockDraft, info: &MockGenInfo) -> Result<i64, String> {
        let rows = crate::services::mock_generator::persist_table(draft, info)?;
        self.invalidate_analysis_nav();
        Ok(rows)
    }

    fn export_file(
        &self,
        draft: &MockDraft,
        info: &MockGenInfo,
        format: &MockExportFormat,
        path: &str,
    ) -> Result<String, String> {
        crate::services::mock_generator::export_file(draft, info, format, path)
    }

    fn save_scratchpad(
        &self,
        draft: &MockDraft,
        info: &MockGenInfo,
        format: &MockExportFormat,
    ) -> Result<String, String> {
        let root = self.project_root();
        crate::services::mock_generator::save_scratchpad(
            draft,
            info,
            format,
            root.as_deref(),
        )
    }

    fn existing_tables(&self) -> Vec<String> {
        crate::services::mock_generator::existing_tables()
    }

    fn schema_sources(&self) -> Vec<SchemaSource> {
        let connections = self.shared.connections.borrow();
        crate::services::mock_generator::schema_sources(&connections)
    }

    fn import_columns(&self, request: &SchemaRequest) -> Result<Vec<MockColumnSpec>, String> {
        let root = self.project_root();
        let root_text = root.map(|p| p.to_string_lossy().to_string());
        crate::services::mock_generator::import_columns(request, root_text.as_deref())
    }

    fn export_dir(&self) -> String {
        match self.project_root() {
            Some(root) => root.to_string_lossy().to_string(),
            None => crate::services::query_export::default_export_dir()
                .to_string_lossy()
                .to_string(),
        }
    }

    fn read_only(&self) -> bool {
        self.shared.project_ui.borrow().read_only
    }

    fn open_detail(&self, window: &mut Window, cx: &mut App) {
        let open = self.shared.open_mock_detail.borrow().clone();
        if let Some(open) = open {
            open(window, cx);
        } else {
            // 宿主未装配（理论上不会）：退到只重绘，面板结果仍可查看
            self.shared.notify_host(cx);
        }
    }

    fn notify(&self, cx: &mut App) {
        self.shared.notify_host(cx);
    }
}

/// 组装 Mock 面板宿主（面板实体创建时调用一次）。
pub fn build_host(shared: &Shared) -> Rc<dyn MockHost> {
    Rc::new(WorkbenchMockHost {
        shared: shared.clone(),
    })
}
