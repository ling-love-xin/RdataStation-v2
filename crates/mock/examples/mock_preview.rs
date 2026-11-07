//! Mock 模块预览工具：独立窗口 + 真数据 + 真主题 + 真图标。
//!
//! ```sh
//! cargo run -p rds-mock --example mock_preview
//! ```
//!
//! ## 它解决什么
//!
//! headless 窗口测试（`mock_view/tests.rs`）只能断言「不 panic、结构不回归」——
//! **它看不到像素**。而 mock 模块的 UI 问题恰恰集中在像素层：对齐、间距、高度塌陷、
//! 对话框比例。本工具起一个真窗口，让这些能用眼睛验收，改一行看一行。
//!
//! ## 与测试的分工（不可替代）
//!
//! | | 窗口测试 | 本工具 |
//! | --- | --- | --- |
//! | 驱动 | headless `draw(cx)` | 真窗口 + 事件循环 |
//! | 能验 | 不 panic / id 唯一 / 状态流转 | 布局 / 间距 / 对齐 / 对话框 / 真数据预览 |
//! | 断言 | 有 | 无（靠人眼） |
//!
//! ## 宿主
//!
//! [`MockHost`] 由本工具自己实现（[`PreviewHost`]）：**真生成**走 `MockEngine`
//! （独立进程内的 DuckDB 内存库），不是假数据，所以「点生成 → 看预览 → 点导出」
//! 整条链路都能在预览里跑通。
//!
//! 三个出口里只有**文件导出**可用：预览没有打开项目，也就没有项目分析库与草稿箱，
//! 落库 / 追加给出可读拒绝（与产品里的口径一致，顺带能验收错误态的 UI）。
//!
//! 过程产物（导出文件）落 `.rds/tmp/mock-preview/`，不进仓库、不写产品数据根。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use gpui_kit::base::StyledExt;
use gpui_kit::component::{ActiveTheme, Root, Theme, ThemeRegistry};
use gpui_kit::*;

use rds_mock::mock_view::{
    AppendTargetStatus, DetailTarget, MockColumnSpec, MockDetailView, MockGenInfo, MockHost,
    MockJobDone, MockJobKind, MockJobPhase, MockJobProgress, MockJobState, MockPanel, MockPreview,
    MockTableConfig, SchemaRequest, SchemaSource, flatten_preview,
};
use rds_mock::{ColumnDef, MockConfig, MockEngine, MockExportFormat, sanitize_identifier};

/// 右侧面板宽度：对齐工作台右 Dock 的常见宽度（面板视图按它排版，太宽会掩盖窄栏下的挤压问题）。
const PANEL_WIDTH: f32 = 420.0;

/// 预览用的数据根 / 导出目录（仓库 `.rds/tmp/` 下，与项目约定一致：过程产物不入库）。
fn preview_home() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.rds/tmp/mock-preview")
}

fn main() {
    // 数据根：必须在任何线程 / 运行时启动前设好（引擎的落点与临时目录都读它）。
    // 不设会让引擎按「可执行文件所在目录」取，产物就散进 target/ 了。
    //
    // SAFETY: edition 2024 下 `set_var` 为 unsafe——此处尚在 main 第一条语句，
    // 进程内没有任何其它线程（连 tokio 运行时都还没建），不存在数据竞争。
    unsafe {
        std::env::set_var("RDS_HOME", preview_home().join("home"));
    }
    std::fs::create_dir_all(preview_home()).ok();

    // 大栈线程：与主程序同口径。GPUI 的视图树构建 / 布局 / 事件派发在 debug 构建下递归较深，
    // Windows 主线程默认 1 MiB 栈会直接溢出崩溃（表现为窗口一闪就没）。
    // `RDS_UI_STACK_MB` 可覆盖（真机「点一下就崩」时用它先判定是不是栈不够）。
    let stack_mb = std::env::var("RDS_UI_STACK_MB")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|mb| *mb >= 16)
        .unwrap_or(64);
    let handle = std::thread::Builder::new()
        .name("mock-preview-main".to_string())
        .stack_size(stack_mb * 1024 * 1024)
        .spawn(run_preview)
        .expect("failed to spawn preview main thread");
    if handle.join().is_err() {
        eprintln!("[mock-preview] 主线程异常退出");
        std::process::exit(1);
    }
}

fn run_preview() {
    gpui_kit::application()
        // 资产源不注册 = 所有图标静默渲染为空（元素在但看不见）：预览里会把「布局错」
        // 和「图标没加载」混在一起看。这里直接用组件库自带的全量图标集。
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            // 引擎的生成接口是 async：先备好运行时，别等点「生成」时才建（首帧卡一下）。
            let _ = tokio_handle();
            // 产品主题：与工作台同一份 `assets/themes/*.json`（读不到就回落 gpui-kit 默认）。
            // 不看真主题，颜色类问题会全部失真。
            load_product_themes(cx);

            let export_dir = preview_home();
            std::fs::create_dir_all(&export_dir).ok();

            let host = Rc::new(PreviewHost::new(export_dir));
            let host_for_window = Rc::clone(&host);
            let _ = cx.open_window(WindowOptions::default(), |window, cx| {
                window.set_rem_size(px(16.));
                let panel = cx.new(|cx| MockPanel::new(host_for_window.clone(), cx));
                let detail =
                    cx.new(|cx| MockDetailView::new(panel.clone(), DetailTarget::Config(0), cx));
                let harness = cx.new(|_cx| PreviewHarness {
                    panel,
                    detail,
                    tab_key: Some(DetailTarget::Config(0).key()),
                });
                // 窗口创建后才拿得到 harness 句柄：回填空位，供「打开详情 tab」切换用
                *host_for_window.harness.borrow_mut() = Some(harness.downgrade());
                // 窗口第一层必须是 component 的 Root（不是 base 的）：
                // 对话框 / 弹层 / 主题都挂在它上面，`gpui_kit::open_window` 给的是 base::Root。
                cx.new(|cx| Root::new(harness, window, cx))
            });
        });
}

/// 预览窗口：右侧 Mock 面板 + 中央详情 tab，与工作台里的形态一致。
struct PreviewHarness {
    panel: Entity<MockPanel>,
    detail: Entity<MockDetailView>,
    /// 当前中央 tab 的键（切 tab 时用它判重，避免重复重建同一个 tab）
    tab_key: Option<String>,
}

impl Render for PreviewHarness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // h_flex 的交叉轴默认居中：两栏都必须 h_full 显式撑满，否则列高于行时上下同时被裁
        div()
            .size_full()
            .h_flex()
            .items_stretch()
            .bg(cx.theme().colors.background)
            .child(
                div()
                    .h_full()
                    .w(px(PANEL_WIDTH))
                    .flex_shrink_0()
                    .child(self.panel.clone()),
            )
            .child(div().h_full().flex_1().min_w_0().child(self.detail.clone()))
    }
}

/// 任务状态槽：工作线程与 UI 线程的**唯一**交接面。
///
/// 为什么是「槽 + 轮询」而不是回调：面板自带 120ms 轮询泵（`MockPanel::spawn_job_pump`），
/// 它每拍问一次 `take_job_done` / `job_state`；宿主只要如实回答这两个问题即可，
/// 不必把 gpui 的 `cx` 或 `Entity` 带进工作线程（跨线程碰它们会 panic）。
struct JobSlot {
    /// 有任务在进行（`start_job` 置位，工作线程收尾清除）
    running: AtomicBool,
    /// 本次任务的取消令牌（每个任务换一个新的；引擎在批次边界响应）
    cancel: Mutex<Arc<AtomicBool>>,
    /// 进度（工作线程写、UI 线程读）
    progress: Mutex<MockJobProgress>,
    /// 结果（**一次性**：`take_job_done` 取走后归位）
    done: Mutex<Option<Result<MockJobDone, String>>>,
}

impl Default for JobSlot {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            progress: Mutex::new(MockJobProgress {
                phase: MockJobPhase::Generating,
                batches_done: 0,
                batches_total: 0,
                rows_total: 0,
            }),
            done: Mutex::new(None),
        }
    }
}

/// tokio 运行时句柄：引擎的生成接口是 async，工作线程用它 `block_on`。
///
/// 静态持有 = 运行时活到进程结束（工作线程只拿 Handle，不必持有本体）。
fn tokio_handle() -> tokio::runtime::Handle {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("构建 tokio 运行时失败")
    })
    .handle()
    .clone()
}

/// 预览宿主：真生成（`MockEngine`）+ 文件出口，不做落库 / 追加（预览没有项目分析库）。
struct PreviewHost {
    slot: Arc<JobSlot>,
    /// 预览窗口（弱引用：窗口创建后回填，避免构造顺序上的循环）
    harness: Rc<RefCell<Option<WeakEntity<PreviewHarness>>>>,
    /// 导出目录
    export_dir: PathBuf,
}

impl PreviewHost {
    fn new(export_dir: PathBuf) -> Self {
        Self {
            slot: Arc::new(JobSlot::default()),
            harness: Rc::new(RefCell::new(None)),
            export_dir,
        }
    }

    /// 起一个工作线程跑 `work`，收尾把结果写进槽。
    ///
    /// 用独立线程而不是 `spawn_blocking`：`block_on` 一个 CPU 密集的生成任务会占住
    /// 一个 runtime worker；预览工具里生成与预览取样抢的是同一把内存库锁，
    /// 线程归属清晰一点更好定位。
    fn spawn(&self, work: impl FnOnce() -> Result<MockJobDone, String> + Send + 'static) {
        let slot = self.slot.clone();
        self.slot.running.store(true, Ordering::SeqCst);
        *self.slot.done.lock().unwrap() = None;
        std::thread::Builder::new()
            .name("mock-preview-worker".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                *slot.done.lock().unwrap() = Some(work());
                slot.running.store(false, Ordering::SeqCst);
            })
            .expect("启动预览工作线程");
    }

    /// 表配置 → 引擎配置（预览里就是「所见即所生成」，不做额外加工）。
    fn config_of(draft: &MockTableConfig) -> (MockConfig, Vec<ColumnDef>) {
        let columns: Vec<ColumnDef> = draft.columns.iter().map(|c| c.def.clone()).collect();
        (
            MockConfig {
                table_name: draft.table_name.clone(),
                row_count: draft.options.rows,
                seed: draft.options.seed,
                locale: draft.options.locale.clone(),
                columns: columns.clone(),
            },
            columns,
        )
    }

    /// 导出目标路径（用户没选就落在预览目录里）。
    fn export_path(&self, info: &MockGenInfo, format: &MockExportFormat) -> PathBuf {
        let ext = match format {
            MockExportFormat::Csv => "csv",
            MockExportFormat::Parquet => "parquet",
            MockExportFormat::Xlsx => "xlsx",
            MockExportFormat::Table => "csv",
            MockExportFormat::SqlInsert => "sql",
        };
        self.export_dir
            .join(format!("{}.{ext}", sanitize_identifier(&info.table_name)))
    }
}

impl MockHost for PreviewHost {
    fn start_job(&self, draft: &MockTableConfig, kind: MockJobKind) -> Result<(), String> {
        if self.slot.running.load(Ordering::SeqCst) {
            return Err("已有任务在进行中（请等它结束或先取消）".to_string());
        }
        match kind {
            // —— 生成：唯一需要 async 的一条（引擎内部按批次推进）——
            MockJobKind::Generate => {
                let (config, columns) = Self::config_of(draft);
                let rows_total = draft.options.rows;
                let cancel = Arc::new(AtomicBool::new(false));
                *self.slot.cancel.lock().unwrap() = cancel.clone();
                *self.slot.progress.lock().unwrap() = MockJobProgress {
                    phase: MockJobPhase::Generating,
                    batches_done: 0,
                    batches_total: 0,
                    rows_total,
                };
                let handle = tokio_handle();
                let progress_slot = self.slot.clone();
                self.spawn(move || {
                    let on_progress = move |done: usize, total: usize| {
                        if let Ok(mut p) = progress_slot.progress.lock() {
                            p.batches_done = done;
                            p.batches_total = total;
                        }
                    };
                    handle
                        .block_on(MockEngine::generate_with_progress(
                            config,
                            cancel,
                            on_progress,
                        ))
                        .map(|r| {
                            MockJobDone::Generated(MockGenInfo {
                                table_name: r.table_name,
                                temp_table_name: r.temp_table_name,
                                columns,
                                row_count: r.row_count,
                                elapsed_ms: r.elapsed_ms,
                                preview: flatten_preview(&r.preview),
                            })
                        })
                        .map_err(|e| e.to_string())
                });
            }
            // —— 文件出口：预览支持（产物落 `.rds/tmp/mock-preview/`，可当场打开验内容）——
            MockJobKind::Export { info, format, path } => {
                let target = if path.trim().is_empty() {
                    self.export_path(&info, &format)
                } else {
                    PathBuf::from(path)
                };
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                *self.slot.progress.lock().unwrap() = MockJobProgress {
                    phase: MockJobPhase::Exporting,
                    batches_done: 0,
                    batches_total: 1,
                    rows_total: info.row_count,
                };
                let path_str = target.to_string_lossy().to_string();
                let table = info.table_name.clone();
                let temp = info.temp_table_name.clone();
                self.spawn(move || {
                    MockEngine::export(&temp, &format, Some(&path_str), Some(&table))
                        .map(|message| MockJobDone::Exported { message })
                        .map_err(|e| e.to_string())
                });
            }
            // —— 草稿箱：预览里没有项目草稿箱目录，落到预览目录（等价于导出）——
            MockJobKind::Scratchpad { info, format } => {
                let target = self.export_path(&info, &format);
                std::fs::create_dir_all(&self.export_dir).ok();
                *self.slot.progress.lock().unwrap() = MockJobProgress {
                    phase: MockJobPhase::Exporting,
                    batches_done: 0,
                    batches_total: 1,
                    rows_total: info.row_count,
                };
                let path_str = target.to_string_lossy().to_string();
                let table = info.table_name.clone();
                let temp = info.temp_table_name.clone();
                self.spawn(move || {
                    MockEngine::export(&temp, &format, Some(&path_str), Some(&table))
                        .map(|message| MockJobDone::Exported { message })
                        .map_err(|e| e.to_string())
                });
            }
            // —— 落库 / 追加 / 场景 / 批量：预览环境不具备，给可读拒绝（不是静默失败）——
            // 顺便这也是「错误态 UI」的预览入口：面板会把这句话原样显示出来。
            MockJobKind::AppendTo(table) => {
                return Err(format!(
                    "预览环境没有项目分析库：无法追加到「{table}」（预览工具只支持生成与文件导出）"
                ));
            }
            MockJobKind::Persist(info) => {
                return Err(format!(
                    "预览环境没有项目分析库：无法落库「{}」（预览工具只支持生成与文件导出）",
                    info.table_name
                ));
            }
            MockJobKind::PersistAll(infos) => {
                return Err(format!(
                    "预览环境没有项目分析库：无法落库 {} 张表（预览工具只支持生成与文件导出）",
                    infos.len()
                ));
            }
            MockJobKind::Scenario(template) => {
                return Err(format!(
                    "场景模板「{}」暂不在预览工具支持范围（请在工作台里试）",
                    template.name
                ));
            }
            MockJobKind::ScratchpadAll { infos, .. } => {
                return Err(format!(
                    "草稿箱批量（{} 张表）暂不在预览工具支持范围（请用单表存草稿箱）",
                    infos.len()
                ));
            }
        }
        Ok(())
    }

    fn job_state(&self) -> MockJobState {
        if self.slot.running.load(Ordering::SeqCst) {
            // MockJobProgress 是 Copy：先取出值再解锁，别把 MutexGuard 带进构造（Guard 不是 Copy）
            let progress = *self.slot.progress.lock().unwrap();
            MockJobState::Running(progress)
        } else {
            MockJobState::Idle
        }
    }

    fn take_job_done(&self) -> Option<Result<MockJobDone, String>> {
        self.slot.done.lock().unwrap().take()
    }

    fn cancel_job(&self) {
        self.slot
            .cancel
            .lock()
            .unwrap()
            .store(true, Ordering::SeqCst);
    }

    fn existing_tables(&self) -> Vec<String> {
        // 预览没有分析库：候选为空（面板的「追加到既有表」下拉会显示空态，也是要看的 UI）
        Vec::new()
    }

    fn append_target_status(
        &self,
        _table: &str,
        _pk_hint: Option<&str>,
    ) -> Option<AppendTargetStatus> {
        None
    }

    fn schema_sources(&self) -> Vec<SchemaSource> {
        // 预览没有连接：「从数据源导入结构」为空态
        Vec::new()
    }

    fn import_columns(&self, _request: &SchemaRequest) -> Result<Vec<MockColumnSpec>, String> {
        Err("预览环境没有数据源连接：导入结构要在工作台里选一个连接".to_string())
    }

    fn preview_sample(
        &self,
        temp_table: &str,
        order: Option<(&str, bool)>,
        limit: usize,
    ) -> Result<Option<MockPreview>, String> {
        // 非阻塞口径与装配层一致：拿不到内存库锁时返回 `Ok(None)`，由视图保持现状
        let preview =
            MockEngine::try_preview_sample(temp_table, order, limit).map_err(|e| match order {
                Some((column, _)) => format!("按「{column}」排序取样失败：{e}"),
                None => format!("取样失败：{e}"),
            })?;
        Ok(preview.as_ref().map(flatten_preview))
    }

    fn temp_table_row_count(&self, temp_table: &str) -> Result<Option<i64>, String> {
        MockEngine::try_temp_table_row_count(temp_table)
            .map_err(|e| format!("重读临时表行数失败：{e}"))
    }

    fn export_dir(&self) -> String {
        self.export_dir.to_string_lossy().to_string()
    }

    fn read_only(&self) -> bool {
        false
    }

    fn project_root(&self) -> Option<std::path::PathBuf> {
        // 未打开项目：历史与用户模板都不可用（与工作台里没开项目时同一口径）
        None
    }

    fn open_detail(&self, target: DetailTarget, _window: &mut Window, cx: &mut App) {
        let Some(harness) = self.harness.borrow().as_ref().and_then(|w| w.upgrade()) else {
            return;
        };
        let key = target.key();
        // 已经是这个 tab 就不重建（重建会丢滚动位置与列区宽度）
        if harness.read(cx).tab_key.as_deref() == Some(key.as_str()) {
            return;
        }
        let panel = harness.read(cx).panel.clone();
        let next = cx.new(|cx| MockDetailView::new(panel, target, cx));
        harness.update(cx, |view, cx| {
            view.detail = next;
            view.tab_key = Some(key);
            cx.notify();
        });
    }

    fn notify(&self, cx: &mut App) {
        cx.refresh_windows();
    }
}

/// 加载产品主题（`assets/themes/*.json`），与工作台启动时同一份文件。
///
/// 读不到时静默回落 gpui-kit 默认主题：预览工具不该因为主题文件缺失就起不来，
/// 但会在 stderr 说清楚，免得把「主题没加载」误当成布局问题。
fn load_product_themes(cx: &mut App) {
    let themes_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes");
    let Ok(entries) = std::fs::read_dir(&themes_dir) else {
        eprintln!(
            "[mock-preview] 主题目录不可用 {}：回落 gpui-kit 默认主题",
            themes_dir.display()
        );
        return;
    };
    let registry = ThemeRegistry::global_mut(cx);
    for entry in entries.flatten() {
        let path = entry.path();
        // 产品语义 token 与键名白名单都不是主题文件（结构不同），预览里不加载（与 app 层同口径）
        if path.extension().and_then(|s| s.to_str()) != Some("json")
            || matches!(
                path.file_name().and_then(|s| s.to_str()),
                Some("product-tokens.json" | "theme-keys.json")
            )
        {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if let Err(e) = registry.load_themes_from_str(&content) {
                    eprintln!("[mock-preview] 主题解析失败 {}: {e}", path.display());
                }
            }
            Err(e) => eprintln!("[mock-preview] 主题读取失败 {}: {e}", path.display()),
        }
    }
    // 接上产品主题：与 `app` 的 `attach_rds_theme` 同一做法（明/暗各取一份）
    let (light, dark) = {
        let registry = ThemeRegistry::global(cx);
        (
            registry.themes().get("RDS Light").cloned(),
            registry.themes().get("RDS Dark").cloned(),
        )
    };
    if let (Some(light), Some(dark)) = (light, dark) {
        let theme = cx.global_mut::<Theme>();
        theme.light_theme = light;
        theme.dark_theme = dark;
    }
}
