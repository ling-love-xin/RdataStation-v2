//! 中央内容区面板：SQL 草稿区 / 查询结果 / 属性面板宿主 / 连接对话框宿主。
//!
//! 自 `panels.rs` 纯位移迁出（无语义改动）。
//!
//! 迁移去向（`docs/architecture/editor/editor-architecture.md` §3.3）：SQL 区块与执行链
//! 最终迁入 `crates/editor`；**属性面板宿主与连接对话框宿主保留在 workbench**——
//! 后者是宿主层职责（对话框层挂在 `WorkbenchView::render`），因此本模块是长期归属，
//! 不是过渡文件。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::EventEmitter;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _};
use gpui_kit::*;
use workbench_shell::ui;

// 连接对话框状态与宿主 trait 直接经 extern crate 路径导入——`crate::components::connection_dialog`
// 的 use 会遮蔽 crate 名，导致 `connection_dialog::host` 解析失败（E0432）。
use connection_dialog::host::ConnectionDialogHost;
use connection_dialog::ConnectionDialogState;

use crate::services::db_navigator::NavTable;
// M5 草稿箱替换入口已收敛到侧栏搜索面板（chrome.rs），本面板不再直接入队草稿箱任务。


use super::shared::QueryRequest;
use super::Shared;
// 跨模块：导航拖拽载荷（落点转成「打开查询」请求，见 `Shared::request_query`）/ 属性面板状态。
use database::nav_view::NavDragPayload;
use scratchpad::{
    ScratchpadDiffView,
    scratchpad_view::render_scratchpad_diff_pane,
};

/// 中央内容区面板。
pub struct EditorPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    // Phase A：新建连接对话框状态（懒创建，open_dialog 由"新建连接"按钮触发）。
    // `Rc` 包装：对话框内部（暂存列表 / 管理器等）需把状态句柄 clone 进回调。
    dialog: Option<Rc<ConnectionDialogState<EditorPanel>>>,
    /// 连接对话框项目下拉确认订阅（由 `ensure_dialog_subscription` 持有；句柄释放即取消）。
    _dialog_sub: Option<Subscription>,
    /// 环境下拉确认订阅（切环境刷新策略覆盖；仅持有，回调不重入面板）。
    _env_sub: Option<Subscription>,
    /// Round 25：连接详情卡「分析库元数据树」缓存（哪个连接加载的 + 表列表）。
    ///
    /// 宿主 / 项目宿主 / Mock 宿主**不直接读写这里**：它们只递增
    /// `Shared::nav_cache_epoch` 递失效信号，本面板在 `sync_shared_epochs` 消费。
    nav_for: Rc<RefCell<Option<String>>>,
    nav_tables: Rc<RefCell<Vec<NavTable>>>,
    /// 已消费的失效戳（与 `Shared` 逐帧对比，不等则丢弃自持缓存）。
    nav_cache_epoch: u64,
    /// M5：草稿箱冲突 Diff（原型 §4.5「查看差异」→ 落中央编辑区；消解动作在左侧冲突条上）。
    scratchpad_diff: Rc<RefCell<Option<ScratchpadDiffView>>>,
    /// 分析库元数据树是否正在后台加载（避免 render 每帧重复入队）。
    nav_tree_loading: Cell<bool>,
}

impl EditorPanel {
    /// 消费导航拖拽（表 / 视图 → 中央编辑器）。
    ///
    /// B12 后不再往“编辑区自己的 SQL 框”里写：拖拽结果与右键「查看数据」同口径，
    /// 都变成一条「打开查询」请求（`Shared::request_query`，宿主 render 消费）；
    /// 追加语义由 `Shared::append_sql` 保证，尾随空格避免连续拖多个表时粘成一个标识符。
    fn send_nav_drag(&self, qualified: &str, cx: &mut Context<Self>) {
        self.shared.request_query(QueryRequest {
            conn_id: self
                .shared
                .selected_connection()
                .map(|item| item.id.clone()),
            sql: format!("{qualified} "),
            run: false,
        });
        self.shared.notify_host(cx);
        cx.notify();
    }

    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            dialog: None,
            _dialog_sub: None,
            _env_sub: None,
            nav_for: Rc::new(RefCell::new(None)),
            nav_tables: Rc::new(RefCell::new(Vec::new())),
            nav_cache_epoch: 0,
            scratchpad_diff: Rc::new(RefCell::new(None)),
            nav_tree_loading: Cell::new(false),
        }
    }

    /// 消费宿主侧的失效信号（宿主只递增 `Shared` 的 epoch，不碰本面板数据）。
    fn sync_shared_epochs(&mut self) {
        let nav = self.shared.nav_cache_epoch.get();
        if nav != self.nav_cache_epoch {
            self.nav_cache_epoch = nav;
            *self.nav_for.borrow_mut() = None;
            self.nav_tables.borrow_mut().clear();
        }
    }

    /// 当前连接的分析库表名快照（宿主 Quick Open 的「表」分组用）。
    pub fn nav_table_names(&self) -> Vec<String> {
        self.nav_tables
            .borrow()
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    /// 连接对话框状态（懒创建：首次 `request_*` 时建立；宿主 / 测试只读访问）。
    pub fn dialog_state(&self) -> Option<Rc<ConnectionDialogState<EditorPanel>>> {
        self.dialog.clone()
    }

    /// 打开「新建数据源连接」对话框（编辑区按钮入口）。
    ///
    /// 打开后必须通知宿主重绘：对话框层挂在 `WorkbenchView::render` 上，
    /// 而 `Root` 的 notify 不会让子视图重建元素树。
    pub fn request_new_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(ConnectionDialogState::new(
                window, cx,
            )));
            // 新状态 → 旧订阅（若有）失去意义，重建。
            self._dialog_sub = None;
        }
        let dialog = self.dialog.clone().expect("dialog initialized");
        // 重新打开：重置元数据标记，强制下一次渲染重新拉取引用 / 类型 / 驱动目录。
        dialog.meta_refreshed.set(false);
        self.ensure_dialog_subscription(&dialog, window, cx);
        let shared_host: Rc<dyn ConnectionDialogHost> =
            Rc::new(self.shared.clone());
        dialog.open(cx.entity(), shared_host, None, window, cx);
        self.shared.notify_host(cx);
    }

    /// 打开「编辑数据源连接」对话框（侧边栏「编辑」入口）。
    pub fn request_edit_connection(
        &mut self,
        conn_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(ConnectionDialogState::new(
                window, cx,
            )));
            self._dialog_sub = None;
        }
        let dialog = self.dialog.clone().expect("dialog initialized");
        // 重新打开：重置元数据标记，强制下一次渲染重新拉取（引用 / 类型 / 驱动目录）。
        dialog.meta_refreshed.set(false);
        self.ensure_dialog_subscription(&dialog, window, cx);
        let shared_host: Rc<dyn ConnectionDialogHost> =
            Rc::new(self.shared.clone());
        dialog.open(cx.entity(), shared_host, Some(conn_id), window, cx);
        self.shared.notify_host(cx);
    }

    /// 确保项目下拉 / 环境下拉确认订阅已建立（只建一次；订阅句柄由本面板持有）。
    ///
    /// 订阅建立放在面板入口而非 `ConnectionDialogState::open`：`open` 在面板 `update`
    /// 上下文内被调用，在那里再 `update` 面板会触发重入 panic。
    fn ensure_dialog_subscription(
        &mut self,
        dialog: &Rc<ConnectionDialogState<EditorPanel>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 宿主能力经 trait 对象传入（对话框不依赖 Shared 具体类型；字段级 Rc 共享，语义一致）。
        let shared_host: Rc<dyn ConnectionDialogHost> =
            Rc::new(self.shared.clone());
        if self._dialog_sub.is_none() {
            self._dialog_sub = Some(dialog.subscribe_project_confirm(&shared_host, window, cx));
        }
        // 环境下拉确认订阅（切环境 → 刷新策略覆盖；句柄同样由面板持有）。
        if self._env_sub.is_none() {
            self._env_sub = Some(dialog.subscribe_env_confirm(&cx.entity(), window, cx));
        }
    }

    /// 后台加载分析库元数据树（连接详情卡用）。
    ///
    /// `load_navigator_tree` 会打开 DuckDB 文件（同步读盘），原先在 render 内直接调用——
    /// 选中联邦连接的那一帧会阻塞；现改为后台执行 + 回填，render 只读缓存。
    fn ensure_analysis_tree(&self, conn_id: String, cx: &mut Context<Self>) {
        if self.nav_tree_loading.get() {
            return;
        }
        self.nav_tree_loading.set(true);
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let path = crate::services::workspace_loader::global_analysis_db_path();
        cx.spawn(async move |_this, cx| {
            let tree = executor
                .spawn(async move {
                    crate::services::db_navigator::load_navigator_tree(&path).unwrap_or_default()
                })
                .await;
            let _ = weak.update(cx, |panel, cx| {
                *panel.nav_tables.borrow_mut() = tree;
                *panel.nav_for.borrow_mut() = Some(conn_id);
                panel.nav_tree_loading.set(false);
                cx.notify();
            });
        })
        .detach();
    }

    /// 投递草稿箱冲突 Diff（`EditorBridge::show_diff`；`None` = 关闭）。
    pub(super) fn set_scratchpad_diff(
        &mut self,
        view: Option<ScratchpadDiffView>,
        cx: &mut Context<Self>,
    ) {
        *self.scratchpad_diff.borrow_mut() = view;
        cx.notify();
    }
}

impl Focusable for EditorPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 宿主失效信号（切换连接 / 项目 / Mock 落库）在渲染入口消费：只读 Shared，不写别家数据。
        self.sync_shared_epochs();
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(ConnectionDialogState::new(
                window, cx,
            )));
        }

        // M5 草稿箱搜索结果：替换输入在**侧栏搜索面板**（2026-10-10 起，VSCode 形态），
        // 结果展示在**中央「搜索: query」标签页**（`SearchResultsHostPanel`）——本面板不承载。

        // 「编辑连接」/「新建连接」改由 `EditorBridge` 在事件路径直接调用（见 `shared.rs`）。
        // 导航的 SQL（查看数据 / 生成 SQL / 拖拽）在 B12 后不再落到本面板：它们走
        // `Shared::request_query`，由宿主开一份编辑器文档（真正的 SQL 编辑器在那里）。

        // Round 25：分析库元数据树按需后台加载（同样必须在 `let theme = cx.theme()` 之前：
        // theme 借了 `cx`，之后再要 `&mut Context` 会 E0502）。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed && self.nav_for.borrow().as_deref() != Some(item.id.as_str()) {
                self.ensure_analysis_tree(item.id.clone(), cx);
            }
        }

        let theme = cx.theme();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();

        let mut content = div()
            .v_flex()
            .size_full()
            .pt_4()
            .pl_4()
            .pr_4()
            .gap_3()
            .bg(theme.colors.background)
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.colors.foreground)
                            .child("数据源连接"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("选中连接查看真实元数据；本地加速（DuckDB Secret）将在连接建立后自动注册"),
                    ),
            );

        // Round 22：选中连接 → 真实元数据详情卡片（键值行）。
        if let Some(item) = self.shared.selected_connection() {
            let shared = self.shared.clone();
            let entity = entity.clone();
            let selected_id = item.id.clone();
            let selected_name = item.name.clone();
            let status = if item.connected {
                "已连接"
            } else {
                "未连接"
            };
            let status_color = if item.connected {
                theme.colors.success
            } else {
                theme.colors.muted
            };
            let fields: Vec<(&'static str, String)> = vec![
                ("名称", item.name.clone()),
                ("驱动", item.driver.clone()),
                ("主机", item.host.clone().unwrap_or_else(|| "-".to_string())),
                (
                    "端口",
                    item.port
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "数据库",
                    item.database.clone().unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "Schema",
                    item.schema.clone().unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "DuckDB 联邦",
                    if item.use_duckdb_fed {
                        "开启（本地加速）".to_string()
                    } else {
                        "关闭".to_string()
                    },
                ),
                (
                    "描述",
                    item.description.clone().unwrap_or_else(|| "-".to_string()),
                ),
                ("创建时间", item.created_at.clone()),
                ("更新时间", item.updated_at.clone()),
            ];

            let mut rows = div().v_flex().gap_1();
            for (label, value) in fields {
                rows = rows.child(
                    div()
                        .h_flex()
                        .items_center()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .w_24()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme.colors.foreground)
                                .child(value),
                        ),
                );
            }
            content = content.child(
                div().v_flex().gap_2().w_full().rounded_md().pl_3().pr_3().pt_2().pb_2()
                    .border_1().border_color(theme.colors.border)
                    .child(
                        div().h_flex().items_center().gap_2()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(item.name))
                            .child(
                                div().h_flex().items_center().gap_1()
                                    .child(div().w_2().h_2().rounded_full().bg(status_color))
                                    .child(div().text_xs().text_color(theme.colors.muted_foreground).child(status)),
                            ),
                    )
                    .child(rows)
                    .child(
                        div().h_flex().items_center().gap_2().mt_2()
                            .child(
                                Button::new("delete-connection")
                                    .danger()
                                    .outline()
                                    .small()
                                    .label("删除连接")
                                    .on_click(move |_, _, app| {
                                        // 项目作用域删除需要当前项目根（未打开项目时传 None）。
                                        let project_root = shared
                                            .project
                                            .borrow()
                                            .as_ref()
                                            .map(|p| p.root.to_string_lossy().to_string());
                                        match crate::services::workspace_loader::delete_connection(
                                            &selected_id,
                                            project_root.as_deref(),
                                        ) {
                                            Ok(()) => {
                                                let (items, _) =
                                                    crate::services::workspace_loader::load_connections_for_scope(
                                                        project_root
                                                            .as_deref()
                                                            .map(std::path::Path::new),
                                                    );
                                                *shared.connections.borrow_mut() = items;
                                                shared.selected.set(None);
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("连接「{}」已删除", selected_name));
                                            }
                                            Err(e) => {
                                                *shared.notice.borrow_mut() = Some(format!("删除失败: {}", e));
                                            }
                                        }
                                        entity.update(app, |_, cx| cx.notify());
                                    }),
                            ),
                    )
                    .child(
                        div().v_flex().gap_1().mt_2().pt_2()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("数据库导航（DuckDB 分析库）")),
                    ),
            );
        }

        // Round 25：数据库导航区——分析库元数据树由上文按需后台加载，这里只读缓存。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed {
                let nav = self.nav_tables.borrow();
                let mut nav_content = div().v_flex().gap_1().mt_1p5();
                if nav.is_empty() {
                    nav_content = nav_content.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("分析库暂无表——运行 seed_demo 或导入数据"),
                    );
                } else {
                    for table in nav.iter() {
                        nav_content =
                            nav_content.child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(div().h_flex().items_center().gap_2().child(
                                        div().text_xs().font_weight(FontWeight::MEDIUM).child(
                                            format!("{}（{} 列）", table.name, table.columns.len()),
                                        ),
                                    ))
                                    .child(div().v_flex().gap_1().pl_3().children(
                                        table.columns.iter().map(|col| {
                                            div()
                                                .h_flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .w(rems(ui::DATA_NAV_COL_NAME_WIDTH))
                                                        .flex_none()
                                                        .text_xs()
                                                        .child(col.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_xs()
                                                        .text_color(theme.colors.muted_foreground)
                                                        .child(col.data_type.clone()),
                                                )
                                                .child(if col.is_primary_key {
                                                    div()
                                                        .text_xs()
                                                        .text_color(theme.colors.info)
                                                        .child("PK")
                                                } else {
                                                    div().text_xs().child("")
                                                })
                                        }),
                                    )),
                            );
                    }
                }
                content = content.child(nav_content);
            }
        }

        content = content.child(div().h_6());

        // M7：Mock 生成入口的标题（原先挂在 SQL 区里；B12 删掉 SQL 区后独立成块）。
        content = content.child(
            div().v_flex().gap_1().child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Mock 数据（生成测试数据）"),
            ),
        );

        // M5 草稿箱内容搜索结果：2026-10-10 起已升为中央 Dock 独立 tab
        // （「搜索: query」标签页，见 `SearchResultsHostPanel` / `Shared::scratchpad_search`），
        // 与打开的文档 tab 并列（VSCode 全局搜索形态）；本面板不再承载搜索结果。

        // M5 草稿箱冲突 Diff（原型 §4.5：冲突时「查看差异」→ 落中央编辑区）。
        {
            let diff = self.scratchpad_diff.borrow();
            if let Some(diff) = diff.as_ref() {
                let shared = self.shared.clone();
                let diff_state = self.scratchpad_diff.clone();
                let entity = entity.clone();
                let clear = move |_: &gpui_kit::ClickEvent,
                                  _: &mut gpui_kit::Window,
                                  app: &mut App| {
                    *diff_state.borrow_mut() = None;
                    shared.notify_host(app);
                    entity.update(app, |_, cx| cx.notify());
                };
                content = content.child(render_scratchpad_diff_pane(diff, theme, clear));
            }
        }

        content = content.child(
            Button::new("mock-generate-btn")
                .primary()
                .label("生成 Mock")
                .on_click({
                    let shared = self.shared.clone();
                    move |_, _, app| {
                        // 单一入口：目标表 / 行数 / 列配置都在右 Dock 的 Mock 面板里确定。
                        shared.open_mock_panel(None, app);
                    }
                }),
        );

        content = content.child(
            Button::new("new-connection")
                .primary()
                .icon(IconName::Plus)
                .label("新建连接")
                .on_click({
                    let entity = entity.clone();
                    move |_, window, app| {
                        entity.update(app, |editor, cx| {
                            editor.request_new_connection(window, cx);
                        });
                    }
                }),
        );

        if notice.is_some() {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.info)
                    .child(notice.unwrap()),
            );
        }

        // 兜底落点：拖到本面板任意位置都变成「把限定名送进编辑器」的请求。
        // 真正的 SQL 编辑器在中央 Dock（`crates/editor`），落点只是把意图转成请求。
        content = content.on_drop({
            let entity = entity.clone();
            move |payload: &NavDragPayload, _window, app| {
                let qualified = payload.qualified.clone();
                entity.update(app, |this, cx| this.send_nav_drag(&qualified, cx));
            }
        });

        // `h_flex()` 默认交叉轴居中：不写 items_stretch，内容列会按内容高度被竖直居中，
        // 高于面板的部分上下同时被裁。
        let mut root = div()
            .id("editor-panel")
            .role(gpui::Role::Group)
            .test_support()
            .h_flex()
            .items_stretch()
            .size_full();
        root = root.child(div().flex_1().min_w_0().child(content));
        root
    }
}

impl EventEmitter<BasePanelEvent> for EditorPanel {}

impl BasePanel for EditorPanel {
    fn panel_name(&self) -> &'static str {
        "editor"
    }
}

impl ComponentPanel for EditorPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 返回 None：tab 条用 title() 渲染（图标 + 文字），对齐 VSCode 风格
        None
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(Icon::default().path("icons/layout-dashboard.svg"))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("工作台"),
            )
    }
}
