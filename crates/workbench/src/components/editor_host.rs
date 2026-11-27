//! 编辑器宿主端口实现（P2-a）：把 `EditorHost` / `FederatedSourcePort` 接到宿主侧。
//!
//! `services/editor_*`（原 workbench 侧执行 / 连接 / 补全等端口实现）下沉到
//! `editor::ports` 后，它们不再依赖 `Shared` 的具体类型，只依赖
//! [`editor::host::EditorHost`]——本文件就是那个 trait 的 workbench 侧实现：
//! 连接快照 / 项目根 / 项目锁转发给 `Shared`，宿主命令（历史 / 洞察 / 导航定位）
//! 转发给 `Shared` 的既有 pub 方法。
//!
//! [`editor::host::FederatedSourcePort`] 是**工作线程**端口（exec 的 `QueryRunner`
//! 跨线程调用）：实现不持 `Shared`（`Rc` 不能跨线程），只调全局 `DataSourceService`。

use std::sync::Arc;

use editor::host::{EditorHost, FederatedSourcePort, FederatedSourceRecord, PrimaryMemory};
use editor::shared::InsightColumnRequest;
use gpui_kit::App;
use workbench_shell::model::ConnectionItem;

use crate::panels::Shared;

/// 编辑器宿主实现：把所有问题都转发给 `Shared`（连接快照 / 项目根 / 项目锁 / 宿主命令）。
pub struct WorkbenchEditorHost {
    shared: Shared,
}

impl WorkbenchEditorHost {
    pub fn new(shared: Shared) -> Self {
        Self { shared }
    }
}

impl EditorHost for WorkbenchEditorHost {
    fn connections(&self) -> Vec<ConnectionItem> {
        self.shared.connections.borrow().clone()
    }

    fn project_root(&self) -> Option<std::path::PathBuf> {
        self.shared.project_root()
    }

    fn project_read_only(&self) -> bool {
        self.shared.project_ui.borrow().read_only
    }

    fn set_connection_connected(&self, conn_id: &str, connected: bool) {
        let mut connections = self.shared.connections.borrow_mut();
        if let Some(item) = connections.iter_mut().find(|c| c.id == conn_id) {
            item.connected = connected;
        }
    }

    fn is_connected(&self, conn_id: &str) -> bool {
        crate::services::nav_runtime::is_connected(conn_id)
    }

    fn connect_entry(&self, conn_id: &str) -> Result<(), String> {
        let root = self
            .shared
            .project_root()
            .map(|path| path.to_string_lossy().to_string());
        crate::services::nav_runtime::connect_entry(conn_id, root.as_deref())
    }

    fn open_history(&self, cx: &mut App) {
        self.shared
            .open_right_panel(crate::view::RightPanel::History, cx);
    }

    fn open_insight_source_column(&self, request: InsightColumnRequest, cx: &mut App) {
        let source = insight::SampleSource::new(
            request.connection.clone(),
            request.sql.clone(),
            request.label.clone(),
        );
        // 类型留空：编辑器手里没有真类型（行数据是字符串化的），取样后由洞察侧从临时表读出
        // 真类型——不编一个看起来像真的值（面板侧对空类型只显示来源）
        self.shared
            .open_insight_source_column(source, request.column.clone(), String::new(), cx);
    }

    fn request_reveal(&self, object: engine::ObjectRef) {
        self.shared.request_reveal(object);
    }

    /// 切到联邦前的确认（用户口径 2026-11-25：**默认挂上并设为主源**）。
    ///
    /// - 能 ATTACH（`AccelKind::from_db_type` 认这个驱动）：弹确认框，确定时先
    ///   `mount_network_source` 再回 `true`——主源由联邦计划按"本文档绑定的连接"认定，
    ///   所以"挂上"就等于"设为主源"，不再另传一个标记。
    /// - 不能 ATTACH：弹说明框（先到结果集标题栏右键「保存为临时表 / 项目表」，
    ///   切联邦后从 `analytics` 查），确定只回 `true`，不挂任何东西。
    /// - 弹不出窗口：照样回 `true`（菜单不能点不动）。
    fn confirm_federated_mount(
        &self,
        conn_id: &str,
        on_decision: Box<dyn FnOnce(bool, &mut gpui_kit::App)>,
        cx: &mut gpui_kit::App,
    ) {
        use gpui_kit::component::WindowExt as _;
        use std::cell::RefCell;
        use std::rc::Rc;

        // 两条路（确定 / 取消）共用同一个回调，只许调一次
        let decision: Rc<RefCell<Option<Box<dyn FnOnce(bool, &mut gpui_kit::App)>>>> =
            Rc::new(RefCell::new(Some(on_decision)));
        let take = {
            let decision = decision.clone();
            move |ok: bool, cx: &mut gpui_kit::App| {
                let cb = decision.borrow_mut().take();
                if let Some(cb) = cb {
                    cb(ok, cx);
                }
            }
        };

        let conn = self
            .shared
            .connections
            .borrow()
            .iter()
            .find(|item| item.id == conn_id)
            .cloned();
        let Some(conn) = conn else {
            // 连接已经不在清单里（刚被删 / 切了项目）：放行，切档本身还会各自报原因
            take(true, cx);
            return;
        };
        let capable =
            engine::duckdb::accel::AccelKind::from_db_type(&conn.driver).is_ok();
        let name = conn.name.clone();

        let Some(handle) = cx.active_window().or_else(|| cx.windows().first().copied()) else {
            take(true, cx);
            return;
        };
        let shared = self.shared.clone();
        let conn_id = conn_id.to_string();
        let _ = handle.update(cx, |_, window, cx| {
            window.open_alert_dialog(cx, move |alert, _window, _cx| {
                let take_ok = take.clone();
                let take_cancel = take.clone();
                if capable {
                    let shared = shared.clone();
                    let cid = conn_id.clone();
                    alert
                        .confirm()
                        .title(shared::i18n::tr("切到联邦"))
                        .description(format!(
                            "将把「{name}」挂进 DuckDB 作为联邦主源（SQL 不用改写）。是否继续？"
                        ))
                        .on_ok(move |_ev, _window, cx| {
                            crate::components::data_objects_load::mount_network_source(
                                &shared, &cid, cx,
                            );
                            take_ok(true, cx);
                            true
                        })
                        .on_cancel(move |_ev, _window, cx| {
                            take_cancel(false, cx);
                            true
                        })
                } else {
                    alert
                        .confirm()
                        .title(shared::i18n::tr("这条连接挂不上"))
                        .description(format!(
                            "「{name}」的驱动没有 DuckDB ATTACH 能力，切联邦后它不会以 catalog 出现。\n\
                             要用它的结果，先在结果集标题栏右键「保存为临时表」或「持久化为项目表」，\
                             切联邦后从 analytics 里查这两类表。仍要切到联邦吗？"
                        ))
                        .on_ok(move |_ev, _window, cx| {
                            take_ok(true, cx);
                            true
                        })
                        .on_cancel(move |_ev, _window, cx| {
                            take_cancel(false, cx);
                            true
                        })
                }
            });
        });
    }

    fn open_insight_result_set(
        &self,
        request: editor::shared::AnalyzeResultSetRequest,
        cx: &mut App,
    ) {
        // 表已由编辑器物化好（`tmp_q_`，同进程单例）：这里只把它当取样来源，
        // 「SELECT * FROM 表」的拼法与转义在 result_set_actions 里只有一份
        crate::components::result_set_actions::analyze_table(
            &self.shared,
            &request.table,
            &request.label,
            cx,
        );
        // 行数是编辑器侧的事实（物化后以表为准，这里只在状态栏留痕，供用户核对）
        self.shared.say(
            format!(
                "已在洞察中分析 {}（{} 行，来源 {}）",
                request.table, request.rows, request.label
            ),
            cx,
        );
    }

    fn note_result_set_materialized(
        &self,
        notice: editor::shared::MaterializedResultSetNotice,
        _cx: &mut App,
    ) {
        // 只是回执：状态栏与面板刷新由工作台泵统一做（`view/workspace.rs::ensure_persist_pump`）
        crate::services::persist_jobs::note_materialized(
            notice.table,
            notice.rows as u64,
            notice.label,
        );
    }

    fn persist_result_set(&self, request: editor::shared::PersistResultSetRequest, cx: &mut App) {
        // 表已由编辑器物化好（`tmp_q_`，同进程单例）：这里把它交给后台落库通道。
        // 事件路径上只入队（写库在 worker 线程），结果由工作台泵取后经状态栏告知。
        crate::components::result_set_actions::persist_to_project(
            &self.shared,
            &request.table,
            &request.label,
            cx,
        );
    }
}

/// 联邦源清单端口实现：直接读全局 `DataSourceService`（不持 `Shared`，
/// 可在 `QueryRunner` 的工作线程上调用；与原 workbench 执行线程的
/// `block_on(service.list())` 同一条路）。
pub struct WorkbenchFederatedSource;

impl FederatedSourcePort for WorkbenchFederatedSource {
    fn records(&self, focus: Option<&str>) -> Vec<FederatedSourceRecord> {
        let Ok(service) = crate::services::data_source_service::DataSourceService::global() else {
            return Vec::new();
        };
        let Ok(rt) = tokio::runtime::Runtime::new() else {
            return Vec::new();
        };
        let Ok(items) = rt.block_on(service.list()) else {
            return Vec::new();
        };
        items
            .into_iter()
            // 两源合一（判据见 `FederatedSourcePort` 的 doc）：项目登记过 / 本文档绑定
            // （`focus`）。全局池里的连接大多两样都没有——不收它们，否则每跑一次联邦
            // 都要去 `ATTACH` 一堆与本次工作无关的库。
            .filter(|item| {
                engine::persistence::analysis_sources::is_registered(&item.id)
                    || focus == Some(item.id.as_str())
            })
            .filter_map(|item| {
                connection::url::build_connection_url(&item)
                    .ok()
                    .map(|url| FederatedSourceRecord {
                        conn_id: item.id.clone(),
                        name: item.name.clone(),
                        // 别名优先用项目登记（面板刷新时发布的快照）；没登记 = 会话级源，
                        // 计划里退回「连接名 sanitize」（`plan_from_records`）
                        alias: engine::persistence::analysis_sources::alias_for_connection(&item.id),
                        db_type: item.db_type.clone(),
                        url,
                    })
            })
            .collect()
    }

    /// 主源进项目意图（`analysis_primary`，036）。
    ///
    /// 执行线程拿不到 `Shared`（项目根在 UI 状态里），项目根取
    /// `analysis_sources::current_project_root()`——那是 `publish_project` 发布的那一份，
    /// 与别名快照同一时机更新（进项目 / 面板刷新）。
    fn remember_primary(&self, alias: &str) -> Result<PrimaryMemory, String> {
        let Some(root) = engine::persistence::analysis_sources::current_project_root() else {
            // 没有项目：主源只活在本文档会话里（如实降级，不是失败）
            return Ok(PrimaryMemory::SessionOnly);
        };
        let conn = engine::persistence::analysis_sources::open_in_project(&root)
            .map_err(|error| error.to_string())?;
        let rows = engine::persistence::analysis_sources::list(&conn)
            .map_err(|error| error.to_string())?;
        // 只有**登记过**的源才能当项目主源；没登记 = 会话级源，同样降级成 SessionOnly
        let registered = rows.iter().any(|row| {
            row.alias == alias
                && row.mount_kind
                    != engine::persistence::analysis_sources::AnalysisMountKind::Accel
        });
        if !registered {
            return Ok(PrimaryMemory::SessionOnly);
        }
        engine::persistence::analysis_sources::set_primary(&conn, alias)
            .map_err(|error| error.to_string())?;
        // 两张快照一起刷新：执行侧下一次组装（以及重启后的第一次）就按这条主源
        engine::persistence::analysis_sources::publish_project(&root)
            .map_err(|error| error.to_string())?;
        Ok(PrimaryMemory::Project)
    }
}

/// 便捷构造：`WorkbenchFederatedSource` 的 `Arc` 包装（view 装配点用）。
pub fn federated_source() -> Arc<dyn FederatedSourcePort> {
    Arc::new(WorkbenchFederatedSource)
}
