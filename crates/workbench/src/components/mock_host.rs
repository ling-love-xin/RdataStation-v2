//! Mock 生成面板的宿主桥（M7）。
//!
//! mock crate 自带面板视图（`mock::mock_view`），但不依赖 workbench 与 engine 的文件布局；
//! 工作台在这里把「从哪读、往哪写、怎么重绘」注入为 [`MockHost`]：
//!
//! | 宿主能力 | 实现 |
//! | --- | --- |
//! | 后台任务（生成 / 追加 / 三个出口） | `services::mock_jobs`（工作线程 + 进度槽 + 取消） |
//! | 落库 / 导出 / 草稿箱的**实现体** | `services::mock_generator`（装配层；由任务层在工作线程上调用） |
//! | 连接清单（导入结构来源） | `Shared::connections`（工作台当前连接列表） |
//! | 既有分析库表 / 导入列结构 | `services::mock_generator` → `NavCache` / `MetadataService` |
//! | 只读判定 | `Shared.project_ui.read_only`（与 SQL 执行入口同一护栏） |
//! | 项目根（生成历史 / 用户模板的落点） | `Shared.project`（面板只拿这一个问题：
//!   历史的读写都在 mock crate 内完成后台执行，见 `mock::history`） |
//! | 打开详情 tab | `Shared::open_mock_detail`（宿主命令，接中央 Dock） |
//! | 重绘 | `Shared::notify_host`（宿主重绘桥，与连接对话框层同一口径） |
//!
//! 出口统一走后台任务（没有同步入口）：出口用的两个路径——分析库与项目根——在这一层解析，
//! 因为工作线程不能碰 `Shared`（`Rc<RefCell<…>>` 不跨线程）。
//!
//! 落库 / 追加成功后让导航树失效（`Shared::nav_for` 置空）：下一次渲染会重新加载分析库对象，
//! 否则新表要等下次切连接才出现。写入发生在工作线程上，所以这个判定放在 [`MockHost::take_job_done`]
//! （它由 UI 线程调用）。

use std::rc::Rc;

use gpui_kit::{App, Window};
use mock::mock_view::{
    MockColumnSpec, MockDraft, MockHost, MockJobDone, MockJobKind, MockJobState, SchemaRequest,
    SchemaSource,
};

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

    /// 落库 / 追加成功：让分析库导航树失效（编辑区下一帧按 `Shared::nav_cache_epoch` 重载）。
    fn invalidate_analysis_nav(&self) {
        self.shared.invalidate_nav_cache();
    }
}

impl MockHost for WorkbenchMockHost {
    fn start_job(&self, draft: &MockDraft, kind: MockJobKind) -> Result<(), String> {
        // 出口类任务要用的路径在这里（UI 线程）解析：工作线程碰不了 `Shared`
        let paths = crate::services::mock_jobs::JobPaths {
            db_path: crate::services::mock_generator::analytics_db_path(),
            project_root: self.project_root(),
        };
        crate::services::mock_jobs::start(draft, kind, &paths)
    }

    fn job_state(&self) -> MockJobState {
        crate::services::mock_jobs::state()
    }

    fn take_job_done(&self) -> Option<Result<MockJobDone, String>> {
        let done = crate::services::mock_jobs::take_done();
        // 写入分析库成功（新建 / 追加）→ 导航树失效：本方法在 UI 线程上被调用，
        // 而写入本身在 worker 线程上，碰不了 `Shared`
        if matches!(
            &done,
            Some(Ok(MockJobDone::Persisted { .. } | MockJobDone::Appended { .. }))
        ) {
            self.invalidate_analysis_nav();
        }
        done
    }

    fn cancel_job(&self) {
        crate::services::mock_jobs::cancel();
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

    fn project_root(&self) -> Option<std::path::PathBuf> {
        // 固有方法同名：显式限定，避免读者以为在递归
        WorkbenchMockHost::project_root(self)
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
