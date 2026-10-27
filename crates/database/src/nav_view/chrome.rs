//! 面板外壳：面板头 / 搜索行 / 归属域 chips / facet 弹层 / 结果区标题行 / 空态 / 底部状态行。
//!
//! 与 `rows.rs` 的边界：本模块负责「列表之外的一切」——列表本体、虚拟列表装配、
//! 行高计算、以及**每一种行的渲染**（含搜索结果行）都在 `rows.rs`；
//! 本模块只留结果区的**标题行**（“搜索中 / 无命中 / 共几条”）。

use super::*;
use super::PreparedStaged;
use crate::nav::capability::NavOp;
use crate::nav::family_profile;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::toolbar::Toolbar;

/// 面板头连接汇总面板的单行快照（需求①）：`id` 供行尾按钮定位，`connected` 用于
/// 排序（在线排前）与按钮文案（已连「断开」/ 未连「连接」）。
struct NavConnSummaryRow {
    id: String,
    name: String,
    status: NavBadgeStatus,
    connected: bool,
}

impl NavView {
    /// 数据源导航面板（M4）：面板头 + 来源 chips + 搜索 + 分组树。
    pub fn render_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.nav_search.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx));
            state.update(cx, |s, cx| {
                s.set_placeholder("筛选数据源 / 表 / 列 / 标签…", window, cx)
            });
            // 输入变化时通知面板重渲染，否则过滤词不会即时生效；
            // 同时把搜索词标脏（防抖落库，重启后搜索框回到原样）。
            let sub = cx.subscribe_in(&state, window, |this, _e, ev: &InputEvent, _w, cx| {
                if matches!(ev, InputEvent::Change) {
                    // 索引搜索排程走**事件路径**（render 是纯读路径，不再入队）：
                    // 查询词只能由输入变化与恢复回填两条路改，这里覆盖「用户真敲键」；
                    // 程序化 `set_value`（恢复面板状态）不发 `Change`，恢复路径单独排一次
                    //（见 `ensure_panel_state_loaded`）。
                    let filter = this
                        .nav_search
                        .as_ref()
                        .map(|s| s.read(cx).value().to_string())
                        .unwrap_or_default();
                    this.nav_schedule_index_search(&filter, cx);
                    this.mark_panel_state_dirty(cx);
                    cx.notify();
                }
            });
            self.nav_search = Some(state);
            self._nav_search_sub = Some(sub);
        }
        // Ctrl+F 请求（搜索框可能上一帧才创建，故在此统一消费）。
        if self.nav_search_focus_pending {
            self.nav_search_focus_pending = false;
            if let Some(input) = self.nav_search.clone() {
                let handle = input.read(cx).focus_handle(cx);
                handle.focus(window, cx);
            }
        }

        // 本地 SQLite 一次性读取（分组/标签、各连接展开态、面板级选中 / 搜索词）不在 render 做，
        // 推到本帧效果周期之后执行，完成后重绘。
        let need_org = !self.nav.borrow().groups_loaded;
        let need_panel_state = !self.nav.borrow().panel_state_loaded;
        let need_state: Vec<String> = {
            let view = self.nav.borrow();
            self.host
                .connections()
                .iter()
                .filter(|c| !view.state_loaded.contains(&c.id))
                .map(|c| c.id.clone())
                .collect()
        };
        if need_org || need_panel_state || !need_state.is_empty() {
            let conn_ids = need_state.clone();
            cx.defer_in(window, move |this, window, cx| {
                let org_pending = need_org && !this.nav.borrow().groups_loaded;
                if org_pending {
                    this.reload_nav_org();
                }
                for cid in &conn_ids {
                    this.ensure_nav_state_loaded(cid);
                }
                // 启动预连：所有连接的展开态恢复完成后，对「上次展开过」的连接后台建连。
                // 连接成功后 `apply_connect_results` 自动展开加载（无需用户再点一次）。
                this.prewarm_expanded_connections();
                // 面板级状态需窗口（把搜索词写回输入框），所以放在这个 `defer_in` 里。
                if need_panel_state {
                    this.ensure_panel_state_loaded(window, cx);
                }
                cx.notify();
            });
        }
        let raw_search = self
            .nav_search
            .as_ref()
            .map(|s| s.read(cx).value().to_string())
            .unwrap_or_default();
        // 搜索框 facet 语法（`scope:` / `type:` / `driver:` / `tag:`）拆为额外约束，
        // 其余为自由文本；与面板 chips 叠加（AND）而非写回，避免输入框反馈环。
        {
            let parsed = parse_nav_search(&raw_search);
            let mut view = self.nav.borrow_mut();
            view.filter = parsed.free.clone();
            view.search_facets = parsed;
        }

        // 连接行内联**标签**编辑器（`+` 打开）打开时，按需创建标签输入框
        // 并用当前标签预填；关闭时销毁，保证下次打开重新回填。
        let tag_editor_for = self.nav.borrow().tag_editor_for.clone();
        match &tag_editor_for {
            Some(conn_id) => {
                // 已为**本**连接建过则复用；否则重建并重新预填（曾在 A 开过再切 B 时
                // 会沿用 A 的输入值 → 回车把 A 的标签写到 B）。
                let stale = self.nav_tag_input.is_none()
                    || self.nav_tag_input_for.as_deref() != Some(conn_id.as_str());
                if stale {
                    // 预填标签来自 `+` 按钮**事件回调**里的快照（`tag_editor_tags`）：
                    // render 零 I/O——读库已从 render 路径移到 `+` 点击时完成。
                    let current = {
                        let view = self.nav.borrow();
                        if view
                            .tag_editor_tags
                            .as_ref()
                            .is_some_and(|(c, _)| c == conn_id)
                        {
                            view.tag_editor_tags
                                .as_ref()
                                .map(|(_, t)| t.clone())
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    };
                    let text = current.join(", ");
                    let input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("标签，逗号分隔"));
                    input.update(cx, |s, cx| s.set_value(text, window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_nav_tags(cx),
                            _ => {}
                        },
                    );
                    self.nav_tag_input = Some(input);
                    self.nav_tag_input_for = Some(conn_id.clone());
                    self._nav_tag_sub = Some(sub);
                }
            }
            None => {
                self.nav_tag_input = None;
                self.nav_tag_input_for = None;
                self._nav_tag_sub = None;
            }
        }

        // 连接行内「复制为模板」输入框：打开时创建并预填「原名 副本」，关闭时销毁。
        let copy_for = self.nav.borrow().copy_for.clone();
        match &copy_for {
            Some(conn_id) => {
                let stale = self.nav_copy_input.is_none()
                    || self.nav_copy_input_for.as_deref() != Some(conn_id.as_str());
                if stale {
                    let base = self
                        .host
                        .connections()
                        .iter()
                        .find(|c| c.id == *conn_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| conn_id.clone());
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder("新连接名称"));
                    input.update(cx, |s, cx| s.set_value(format!("{base} 副本"), window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_copy_connection(cx),
                            _ => {}
                        },
                    );
                    self.nav_copy_input = Some(input);
                    self.nav_copy_input_for = Some(conn_id.clone());
                    self._nav_copy_sub = Some(sub);
                }
            }
            None => {
                self.nav_copy_input = None;
                self.nav_copy_input_for = None;
                self._nav_copy_sub = None;
            }
        }

        // 分组重命名输入框：打开时按名称预填，关闭时销毁。
        let rename_for = self.nav.borrow().group_rename_for.clone();
        match &rename_for {
            Some(group_id) => {
                if self.nav_group_input.is_none() {
                    let name = self
                        .nav
                        .borrow()
                        .groups
                        .iter()
                        .find(|g| &g.id == group_id)
                        .map(|g| g.name.clone())
                        .unwrap_or_default();
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder("分组名"));
                    input.update(cx, |s, cx| s.set_value(name, window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_group_rename(cx),
                            _ => {}
                        },
                    );
                    self.nav_group_input = Some(input);
                    self._nav_group_sub = Some(sub);
                }
            }
            None => {
                self.nav_group_input = None;
                self._nav_group_sub = None;
            }
        }

        // 树对象内联重命名输入框（S4-④）：F2 置位时按当前名预填，提交 / 取消时销毁。
        let object_rename_for = self.nav.borrow().object_rename_for.clone();
        match &object_rename_for {
            Some(ctx) => {
                if self.nav_rename_input.is_none() {
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder("新名称"));
                    input.update(cx, |s, cx| s.set_value(ctx.name.clone(), window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_object_rename(cx),
                            _ => {}
                        },
                    );
                    self.nav_rename_input = Some(input);
                    self._nav_rename_sub = Some(sub);
                }
            }
            None => {
                self.nav_rename_input = None;
                self._nav_rename_sub = None;
            }
        }

        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let accent = cx.theme().colors.primary;
        let source_filter = self.nav.borrow().source_filter;
        // B6 staged：待提交条目数（面板头「提交 N」按钮预算；空集不渲染、不占槽位）。
        let staged_count = self.nav.borrow().staged_count();

        // 面板头「⟳ 刷新元数据」「断开当前连接」的作用目标：当前选中的连接。
        let current_conn = self.nav_current_connection();
        let current_connected = current_conn
            .as_deref()
            .map(|id| self.host.is_connected(id) || self.nav.borrow().connected.contains(id))
            .unwrap_or(false);
        let project_root = self
            .host
            .project_root()
            .map(|p| p.to_string_lossy().to_string());

        // 计算所有连接的已连接数量，用于决定显示「全部连接」还是「全部断开」按钮
        let all_conns = self.host.connections();
        let connected_set = self.nav.borrow().connected.clone();
        let connected_count = all_conns
            .iter()
            .filter(|c| self.host.is_connected(&c.id) || connected_set.contains(&c.id))
            .count();
        let has_unconnected = connected_count < all_conns.len();
        let has_connected = connected_count > 0;

        let header = div()
            .w_full()
            // 面板头高取 App 统一的 `PANEL_HEADER_HEIGHT`（36px）：这里曾是本仓唯一的
            // 30px 裸值（`rems(1.875)`），与编辑器工具条 / 设置页 / 资产库面板头对不齐
            // （规范：`ui-design-spec.md` §2.1）。
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .border_b_1()
            .border_color(border)
            .child(
                // 面板头按钮行迁移到标准 `Toolbar`：自带 roving 方向键焦点 + toolbar a11y
                // 语义 + 统一 size 托管；标题/徽标/弹簧/分隔线走 `.content()` 槽，按钮走
                // `.child()`（sized 控件）。外层 div 只保留高度与底边框，背景由 Toolbar
                // 透明穿透到面板容器。
                Toolbar::new("nav-header")
                    .h_full()
                    .content(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child("数据源"),
                    )
                    // 面板头连接状态徽标（原型建议①）：`N 连接 · M 在线` 常驻一眼可见，
                    // hover 出逐连接明细（状态点 + 名称 + 状态/失败原因）。与底部状态行同口径。
                    .when_some(self.render_nav_conn_badge(cx), |t, badge| t.content(badge))
                    .content(div().flex_1())
                    // C1 预热进度（后台任务进行中时显示，可取消）。
                    .when(nav_jobs::warm_active(), |t| {
                        let (done, total) = nav_jobs::warm_progress();
                        t.content(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("预热 {done}/{total}")),
                        )
                        .child(
                            Button::new("nav-warm-cancel")
                                .ghost()
                                .small()
                                .label("取消")
                                .on_click(|_, _, _| nav_jobs::cancel_warm()),
                        )
                        .content(Separator::vertical())
                    })
                    .child(
                // 新建数据源（＋）：与编辑区「新建连接」同一条对话框入口。
                Button::new("nav-new-connection")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .on_click({
                        let host = self.host.clone();
                        move |_, window, app: &mut App| {
                            // 命令端口：直接开对话框（不再置位请求字段等渲染消费）。
                            host.new_connection(window, app);
                        }
                    }),
            )
            .child(
                // 新建分组：项目级自定义分组。
                // 图标走资产（`folder-plus`）而不是 emoji `🗂＋`：emoji 是全库唯一的字形图标，
                // 字宽随字体变，与同排的 `IconName` 按钮对不齐。
                Button::new("nav-new-group")
                    .ghost()
                    .small()
                    .icon(
                        Icon::empty()
                            .path("icons/folder-plus.svg")
                            .text_color(muted),
                    )
                    .on_click({
                        let entity = cx.entity();
                        let host = self.host.clone();
                        let seed = GroupFormSeed::for_new(self.next_group_name());
                        move |_, window, app: &mut App| {
                            let e = entity.clone();
                            let host = host.clone();
                            let seed = seed.clone();
                            host.open_group_form(
                                seed,
                                window,
                                app,
                                Rc::new(move |gid, name, desc, app| {
                                    e.update(app, |this, cx| {
                                        this.save_group_form(gid, name, desc, cx);
                                    });
                                }),
                            );
                        }
                    }),
            )
            .child(
                // 刷新元数据（⟳）：刷新当前选中连接（设计 §4.2「单连接 = 工具栏 ⟳」，
                // 「全部」在「⋯」菜单）；未选中连接时禁用。
                Button::new("nav-refresh")
                    .ghost()
                    .small()
                    .icon(IconName::RotateCw)
                    .disabled(current_conn.is_none())
                    .on_click({
                        let entity = cx.entity();
                        let conn = current_conn.clone();
                        move |_, _, app| {
                            let Some(cid) = conn.clone() else {
                                return;
                            };
                            entity.update(app, |this, cx| {
                                this.refresh_node(&cid, &cid, Some(NavPath::Connection), cx);
                            });
                        }
                    }),
            )
            .when(current_connected, |t| {
                t.child(
                    // 断开当前连接（设计 §2.1 / 原型 title「断开当前连接」）：仅运行时已连接时
                    // 可用；断开只关运行时连接，元数据缓存保留（可离线浏览 / 重连秒开）。
                    // 仅已连接时才渲染，避免无用图标占位。
                    Button::new("nav-disconnect")
                        .ghost()
                        .small()
                        .icon(
                            Icon::empty()
                                .path("icons/plug.svg")
                                .text_color(cx.theme().colors.danger),
                        )
                        .on_click({
                            let entity = cx.entity();
                            let conn = current_conn.clone();
                            let root = project_root.clone();
                            move |_, _, app| {
                                let Some(cid) = conn.clone() else {
                                    return;
                                };
                                entity.update(app, |this, cx| {
                                    this.toggle_connection(&cid, root.as_deref(), cx);
                                });
                            }
                        }),
                )
            })
            .child(
                // 全部连接/断开（合并为一个按钮，根据当前状态显示不同图标）
                // 有未连接的 → 显示「全部连接」
                // 有已连接的 → 显示「全部断开」
                Button::new(if has_unconnected { "nav-connect-all" } else { "nav-disconnect-all" })
                    .ghost()
                    .small()
                    .icon(
                        Icon::empty()
                            .path(if has_unconnected { "icons/plug-2.svg" } else { "icons/unplug.svg" })
                            .text_color(muted),
                    )
                    .tooltip(if has_unconnected { "全部连接" } else { "全部断开" })
                    .disabled(!has_unconnected && !has_connected)
                    .on_click({
                        let entity = cx.entity();
                        let root = project_root.clone();
                        let has_unconnected = has_unconnected;
                        move |_, _, app: &mut App| {
                            entity.update(app, |this, cx| {
                                let conns = this.host.connections();
                                let connected: std::collections::HashSet<String> =
                                    this.nav.borrow().connected.iter().cloned().collect();
                                let mut pending = 0usize;
                                if has_unconnected {
                                    // 全部连接
                                    for c in &conns {
                                        let already =
                                            this.host.is_connected(&c.id) || connected.contains(&c.id);
                                        if !already {
                                            pending += 1;
                                            this.toggle_connection(&c.id, root.as_deref(), cx);
                                        }
                                    }
                                    if pending == 0 {
                                        this.host
                                            .notice("连接均已就绪，无需重复连接".to_string(), cx);
                                    }
                                } else {
                                    // 全部断开
                                    for c in &conns {
                                        let already =
                                            this.host.is_connected(&c.id) || connected.contains(&c.id);
                                        if already {
                                            pending += 1;
                                            this.toggle_connection(&c.id, None, cx);
                                        }
                                    }
                                    if pending == 0 {
                                        this.host.notice("没有已连接的连接".to_string(), cx);
                                    }
                                }
                            });
                        }
                    }),
            )
            // B6 staged：有待提交条目时显示「提交 N」；空集不渲染、不占槽位（常态零干扰）。
            // 点击走两阶段：Context 内拍快照与文案，App 作用域开汇总确认对话框；OK 回调
            // 触发后台逐条提交执行（同 B6-4 DeleteToStaged 开窗口径）。
            .when(staged_count > 0, |t| {
                t.content(Separator::vertical()).child(
                    Button::new("nav-commit-staged")
                        .ghost()
                        .small()
                        .label(format!("提交 {staged_count}"))
                        .tooltip(format!("提交 {staged_count} 项待变更（提交后不可恢复）"))
                        .on_click({
                            let entity = cx.entity();
                            move |_, window, app| {
                                let Some(prepared) =
                                    entity.update(app, |this, cx| this.prepare_commit_staged(cx))
                                else {
                                    return;
                                };
                                let entity = entity.clone();
                                window.open_alert_dialog(app, move |alert, _window, _cx| {
                                    let entity = entity.clone();
                                    let prepared = prepared.clone();
                                    let rows = staged_commit_rows(&prepared.plan, muted);
                                    alert
                                        .confirm()
                                        .title(prepared.title)
                                        .description(prepared.description)
                                        .button_props(
                                            DialogButtonProps::default()
                                                .ok_text(prepared.ok_text)
                                                .ok_variant(ButtonVariant::Danger)
                                                .show_cancel(true),
                                        )
                                        .children(rows)
                                        .on_ok(move |_, _, app| {
                                            let entity = entity.clone();
                                            entity.update(app, |this, cx| this.commit_staged(cx));
                                            true
                                        })
                                });
                            }
                        }),
                )
            })
            .content(
                // 更多（⋯）：刷新全部元数据 / 显示开关 / 缓存管理 / 删除当前连接（原型对齐）。
                // 注意：`.dropdown_menu(...)` 装饰返回 `DropdownMenuPopover<Button>`，不实现
                // `Sizable`，不能挂 Toolbar.child()；用 `.content()` 槽（只要求 IntoElement）。
                Button::new("nav-more")
                    .ghost()
                    .small()
                    .icon(IconName::Ellipsis)
                    .dropdown_menu({
                        let entity = cx.entity();
                        let host_tags = self.host.clone();
                        let host_scope = self.host.clone();
                        let host_short = self.host.clone();
                        let host_cache = self.host.clone();
                        let host_groups = self.host.clone();
                        let host_views = self.host.clone();
                        let host_stats = self.host.clone();
                        let host_comment = self.host.clone();
                        let host_size = self.host.clone();
                        let host_rowest = self.host.clone();
                        // 保存视图的筛选快照：菜单构建瞬间的 chips 状态（与 write_nav_filters 同构）。
                        let cur_filters = {
                            let v = self.nav.borrow();
                            NavFilters {
                                source: v.source_filter.map(|s| s.key().to_string()),
                                db_type: v.type_filter.clone(),
                                driver: v.driver_filter.clone(),
                                tag: v.tag_filter.clone(),
                            }
                        };
                        // 全空筛选没有保存意义（应用 = 清空筛选），入口禁用。
                        let filters_all_empty = cur_filters.source.is_none()
                            && cur_filters.db_type.is_none()
                            && cur_filters.driver.is_none()
                            && cur_filters.tag.is_none();
                        let show_tags = self.host.show_tags(cx);
                        let show_scope = self.host.show_scope(cx);
                        let source_short = self.host.source_short_code(cx);
                        let show_inline_stats = self.host.show_inline_stats(cx);
                        // 需求② 表行统计各子项开关：总开关开时仍受各自子项门控。
                        let show_inline_comment = self.host.show_inline_comment(cx);
                        let show_inline_size = self.host.show_inline_size(cx);
                        let show_row_estimate = self.host.row_estimate(cx);
                        let del_conn = current_conn.clone();
                        let del_name = current_conn.as_deref().and_then(|id| {
                            self.host
                                .connections()
                                .iter()
                                .find(|c| c.id == *id)
                                .map(|c| c.name.clone())
                        });
                        move |menu, window, cx| {
                            let e_refresh = entity.clone();
                            let host_cache = host_cache.clone();
                            menu.item(PopupMenuItem::new("刷新全部元数据").on_click(
                                move |_, _, app| {
                                    e_refresh.update(app, |this, cx| this.refresh_all(cx));
                                },
                            ))
                            .separator()
                            // 来源标识组（#29）：短码 / 归属域列 并排可独立开关；
                            // 均即时生效并持久化（settings registry）。
                            .item(
                                PopupMenuItem::new("来源短码")
                                    .checked(source_short)
                                    .on_click({
                                        let host_short = host_short.clone();
                                        move |_, _, app| {
                                            host_short.set_source_short_code(!source_short, app);
                                        }
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("显示归属域")
                                    .checked(show_scope)
                                    .on_click({
                                        let host_scope = host_scope.clone();
                                        move |_, _, app| {
                                            host_scope.set_show_scope(!show_scope, app);
                                        }
                                    }),
                            )
                            .separator()
                            .item(
                                // 选中态用 `checked`（与同文件的「设为主组」菜单一致）：
                                // 文本拼 `✓ ` 会在有勾 / 无勾时改变文案宽度。
                                PopupMenuItem::new("显示标签").checked(show_tags).on_click({
                                    let host_tags = host_tags.clone();
                                    move |_, _, app| {
                                        host_tags.set_show_tags(!show_tags, app);
                                    }
                                }),
                            )
                            .item(
                                // 表行统计徽标总开关（需求④）：关闭后表 / 视图行右侧的
                                // meta / 注释副标 / 分区·继承 / ≈N / 大小徽标全部不显。
                                PopupMenuItem::new("表行统计徽标")
                                    .checked(show_inline_stats)
                                    .on_click({
                                        let host_stats = host_stats.clone();
                                        move |_, _, app| {
                                            host_stats.set_show_inline_stats(!show_inline_stats, app);
                                        }
                                    }),
                            )
                            // 需求② 表行统计子项独立开关：总开关开时仍受各自子项门控。
                            .item(
                                PopupMenuItem::new("注释副标")
                                    .checked(show_inline_comment)
                                    .on_click({
                                        let host_comment = host_comment.clone();
                                        move |_, _, app| {
                                            host_comment.set_show_inline_comment(!show_inline_comment, app);
                                        }
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("大小徽标")
                                    .checked(show_inline_size)
                                    .on_click({
                                        let host_size = host_size.clone();
                                        move |_, _, app| {
                                            host_size.set_show_inline_size(!show_inline_size, app);
                                        }
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("行数估计")
                                    .checked(show_row_estimate)
                                    .on_click({
                                        let host_rowest = host_rowest.clone();
                                        move |_, _, app| {
                                            host_rowest.set_row_estimate(!show_row_estimate, app);
                                        }
                                    }),
                            )
                            .separator()
                            .item(
                                // 已保存视图（#10）：把当前四项 facet 存为命名视图 / 一键应用 /
                                // 管理删除。只动 facet，不动搜索框。
                                PopupMenuItem::new("将当前筛选存为视图…")
                                    .disabled(filters_all_empty)
                                    .on_click({
                                        let host_save = host_views.clone();
                                        let filters = cur_filters.clone();
                                        move |_, window, app| {
                                            host_save
                                                .open_saved_view_dialog(filters.clone(), window, app);
                                        }
                                    }),
                            )
                            .submenu("已保存视图 ▸", window, cx, {
                                let e = entity.clone();
                                let host_views = host_views.clone();
                                move |m, _w, cx| {
                                    let views = host_views.saved_views(cx);
                                    if views.is_empty() {
                                        m.item(PopupMenuItem::new("暂无已保存视图").disabled(true))
                                    } else {
                                        let mut m = m;
                                        for v in views {
                                            let e = e.clone();
                                            let filters = v.filters.clone();
                                            // 名称 + 筛选摘要同行（如「生产库 · type:mysql tag:prod」）。
                                            let label = format!("{} · {}", v.name, v.summary);
                                            m = m.item(
                                                PopupMenuItem::new(label).on_click(
                                                    move |_, _, app| {
                                                        e.update(app, |this, cx| {
                                                            this.apply_saved_view(
                                                                filters.clone(), cx,
                                                            )
                                                        });
                                                    },
                                                ),
                                            );
                                        }
                                        m
                                    }
                                }
                            })
                            .item(
                                PopupMenuItem::new("管理视图…").on_click({
                                    let host_mgr = host_views.clone();
                                    move |_, window, app| {
                                        host_mgr.open_manage_views_dialog(window, app);
                                    }
                                }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("缓存管理…").on_click(move |_, window, app| {
                                    host_cache.open_cache_dialog(window, app);
                                }),
                            )
                            .item(
                                // 管理分组覆盖层（#9 分组部分）：集中重命名 / 合并分组 / 删除，
                                // 标签部分（清理空标签 / 批量打标）归 tag crate 会话。
                                PopupMenuItem::new("管理分组…").on_click({
                                    let host_groups = host_groups.clone();
                                    move |_, window, app| {
                                        host_groups.open_group_manager(window, app);
                                    }
                                }),
                            )
                            .item(
                                // 管理标签覆盖层（tag-system-spec §12 #3 标签侧）：
                                // 批量重命名 / 清理空标签；标签定义跨模块共享（导航 / 资源 / 草稿）。
                                PopupMenuItem::new("管理标签…").on_click({
                                    let host_tags = host_groups.clone();
                                    move |_, window, app| {
                                        host_tags.open_tag_manager(window, app);
                                    }
                                }),
                            )
                            .separator()
                            .item(
                                // 删除当前连接（原型对齐）：作用于当前选中的连接；
                                // 与连接行右键「删除连接」同一确认对话框 + 删除动作。
                                // 无选中连接时禁用（`disabled` 是菜单自身语义，不用 danger
                                // 文案区分可用性）。
                                PopupMenuItem::new("删除当前连接")
                                    .disabled(del_conn.is_none())
                                    .on_click({
                                        let e = entity.clone();
                                        let cid = del_conn.clone();
                                        let name = del_name.clone();
                                        move |_, window, app| {
                                            let Some(cid) = cid.clone() else { return };
                                            let Some(name) = name.clone() else { return };
                                            let e = e.clone();
                                            window.open_alert_dialog(app, move |alert, _, _cx| {
                                                let e = e.clone();
                                                let cid = cid.clone();
                                                alert
                                                    .confirm()
                                                    .title("删除连接")
                                                    .description(format!(
                                                        "确定删除连接「{name}」？元数据缓存会保留（可在「缓存管理」清理）。"
                                                    ))
                                                    .button_props(
                                                        DialogButtonProps::default()
                                                            .ok_text("删除")
                                                            .ok_variant(ButtonVariant::Danger)
                                                            .show_cancel(true),
                                                    )
                                                    .on_ok({
                                                        // `on_ok` 是 `Fn`（可多次调用）：捕获值须可重复消费，
                                                        // 这里先 clone 一份再移进闭包（E0507 修复）。
                                                        let cid = cid.clone();
                                                        let value = name.clone();
                                                        move |_, _window, app| {
                                                            let cid = cid.clone();
                                                            let value = value.clone();
                                                            e.update(app, |this, cx| {
                                                                this.delete_connection(&cid, &value, cx)
                                                            });
                                                            true
                                                        }
                                                    })
                                            });
                                        }
                                    }),
                            )
                        }
                    }),
            ),
            );

        let chips = div()
            .h_flex()
            .items_center()
            .w_full()
            .gap_1()
            .pl_2p5()
            .pr_2()
            .pb_1p5()
            .child(self.nav_source_chip("全部", None, source_filter, muted, accent, cx))
            .child(self.nav_source_chip(
                "项目",
                Some(NavSource::Project),
                source_filter,
                muted,
                accent,
                cx,
            ))
            .child(self.nav_source_chip(
                "全局",
                Some(NavSource::Global),
                source_filter,
                muted,
                accent,
                cx,
            ))
            .child(self.nav_source_chip(
                "共享",
                Some(NavSource::Shared),
                source_filter,
                muted,
                accent,
                cx,
            ));

        // 可见行一次算好（顺序权威）：虚拟列表、键盘漫游序列、滚动定位都读它。
        self.rows = self.collect_nav_rows(source_filter, cx);
        self.sync_nav_order();
        // 跨帧的滚动意图（定位）到这里才能兑现：行集合刚刚才包含目标。
        if let Some(key) = self.nav_pending_scroll.clone() {
            if self.nav_scroll_to_key(&key) {
                self.nav_pending_scroll = None;
            }
        }
        let body = self.render_nav_body(window, cx);

        // 搜索行：输入框 + 「筛选」图标弹层（类型 / 驱动 / 标签；归属域由上方 chips 承担）。
        // 2026-10-20：#27 筛选从文字按钮改为图标按钮，激活时着色；提示词沿用面板
        // 搜索框当前 placeholder（"筛选数据源 / 表 / 列 / 标签…"）。
        let active_facets = self.nav_active_facet_count();
        let filters_active = self.nav_filters_active();
        let free_text = self.nav.borrow().search_facets.free.clone();
        let (cur_type, cur_driver, cur_tag) = {
            let view = self.nav.borrow();
            (
                view.type_filter.clone(),
                view.driver_filter.clone(),
                view.tag_filter.clone(),
            )
        };
        let (type_cands, driver_cands, tag_cands) = self.nav_facet_candidates();
        let tag_pairs: Vec<(String, String)> =
            tag_cands.iter().map(|t| (t.clone(), t.clone())).collect();
        let type_menu_label = match &cur_type {
            Some(t) => {
                let from_catalog = self.nav_type_from_catalog(t);
                let from_catalog = from_catalog
                    .as_ref()
                    .map(|(name, cat)| (name.as_str(), cat.as_str()));
                format!("类型：{}", nav_type_short_label(t, from_catalog))
            }
            None => "类型".to_string(),
        };
        let driver_menu_label = match &cur_driver {
            Some(d) => {
                let name = self
                    .driver_catalog
                    .borrow()
                    .get(d)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| d.clone());
                format!("驱动：{name}")
            }
            None => "驱动".to_string(),
        };
        let tag_menu_label = match &cur_tag {
            Some(t) => format!("标签：{t}"),
            None => "标签".to_string(),
        };

        let facet_button = Button::new("nav-facet-filter")
            .ghost()
            .small()
            // #27 筛选入口改图标（lucide filter），激活态用主题色强调；
            // 具体筛选项与激活计数在 tooltip 里补。
            .icon(nav_icon(
                // #8 实测：资产集无 `icons/filter.svg`（lucide 此套资源命名不同），
                // `sliders-horizontal.svg` 已确认存在（与 search.svg 同源）。
                "icons/sliders-horizontal.svg",
                ui::ICON_SIZE_SM,
                if active_facets > 0 { accent } else { muted },
            ))
            .selected(active_facets > 0)
            // `Button::tooltip` 只收静态文本：计数在构建期拼好（进入菜单可见具体项）。
            .tooltip(if active_facets > 0 {
                format!("已筛 {active_facets} 项：类型 / 驱动 / 标签")
            } else {
                "筛选：类型 / 驱动 / 标签".to_string()
            })
            .dropdown_menu({
                let entity = cx.entity();
                let input = self.nav_search.clone();
                let free = free_text.clone();
                let types = type_cands.clone();
                let drivers = driver_cands.clone();
                let tags = tag_pairs.clone();
                let ct = cur_type.clone();
                let cd = cur_driver.clone();
                let ctg = cur_tag.clone();
                let tl = type_menu_label.clone();
                let dl = driver_menu_label.clone();
                let gl = tag_menu_label.clone();
                // B-1 全局对象名过滤器入口（`None` = 全局级）。
                let host_mask = self.host.clone();
                move |menu, window, cx| {
                    let e_clear = entity.clone();
                    let input_clear = input.clone();
                    let free_clear = free.clone();
                    let mut menu = menu.item(
                        PopupMenuItem::new("清除筛选")
                            .disabled(!filters_active)
                            .on_click(move |_, window, app| {
                                if let Some(input) = &input_clear {
                                    let free = free_clear.clone();
                                    input
                                        .update(app, |s, cx| s.set_value(free.clone(), window, cx));
                                }
                                e_clear.update(app, |this, cx| this.clear_nav_filters(cx));
                            }),
                    );
                    menu = menu.separator();
                    menu = menu.submenu(tl.clone(), window, cx, {
                        let e = entity.clone();
                        let cur = ct.clone();
                        let c = types.clone();
                        move |m, _w, _c| {
                            Self::build_facet_items(
                                m,
                                e.clone(),
                                NavFacet::Type,
                                cur.clone(),
                                c.clone(),
                            )
                        }
                    });
                    menu = menu.submenu(dl.clone(), window, cx, {
                        let e = entity.clone();
                        let cur = cd.clone();
                        let c = drivers.clone();
                        move |m, _w, _c| {
                            Self::build_facet_items(
                                m,
                                e.clone(),
                                NavFacet::Driver,
                                cur.clone(),
                                c.clone(),
                            )
                        }
                    });
                    menu = menu.submenu(gl.clone(), window, cx, {
                        let e = entity.clone();
                        let cur = ctg.clone();
                        let c = tags.clone();
                        move |m, _w, _c| {
                            Self::build_facet_items(
                                m,
                                e.clone(),
                                NavFacet::Tag,
                                cur.clone(),
                                c.clone(),
                            )
                        }
                    });
                    // B-1 全局对象名过滤器入口（面板偏好，跨项目；空规则 = 不启用）。
                    let host_global = host_mask.clone();
                    menu = menu.separator();
                    menu.item(
                        PopupMenuItem::new("对象名过滤器（全局）…")
                            .on_click(move |_, window, app| {
                                host_global.open_object_mask_dialog(None, window, app);
                            }),
                    )
                }
            });

        // #2 工具栏与内容分层：搜索行整体给一层底（明亮主题下与列表区边界清晰），
        // 无边框搜索框在这层底上显形；间距上下对称（原只有 pb_2，顶不贴面板头）。
        let mut search_row = div()
            .h_flex()
            .items_center()
            .w_full()
            .gap_1()
            .px_2()
            .py_1p5()
            .bg(cx.theme().colors.group_box);
        // D11 搜索选项条：匹配模式（前缀 / 包含）+ 搜注释开关 + 搜定义（置灰待内省采集）。
        // 仅在输入框存在时渲染；会话级开关不落库（与「仅已连接」同档）。
        let mut search_options: Option<Div> = None;
        if let Some(input) = &self.nav_search {
            // 搜索框前导放大镜（原型 §2 的 `[🔍 …]`）：面板头部三行控件里
            // 只有这一行是输入框，给个形状信号比只靠 placeholder 更快认出。
            let search_icon = nav_icon("icons/search.svg", ui::ICON_SIZE_SM, muted);
            // #26 匹配开关（`.*` 正则 / `Aa` 区分大小写）移入输入框内部（suffix），
            // 与草稿箱搜索输入框同一形态：`bordered(false)` + `focus_bordered(true)`
            // 无边框风格，明暗主题下都靠 hover/focus 边框显形。
            // 点击只翻状态 + 重排索引搜索（词没变但签名变了 → `nav_schedule_index_search` 会重搜）；
            // 开关随面板状态持久化（重启后保持）。
            let (regex, case_sensitive, connected_only, name_mode, include_comments, include_definitions) = {
                let v = self.nav.borrow();
                (
                    v.search_regex,
                    v.search_case_sensitive,
                    v.search_connected_only,
                    v.search_name_mode,
                    v.search_comments,
                    v.search_definitions,
                )
            };
            let (selected_bg, hover_bg, fg) = {
                let theme = cx.theme();
                (
                    theme.tokens.primary.alpha(0.14),
                    theme.tokens.secondary_hover,
                    theme.colors.foreground,
                )
            };
            let entity_regex = cx.entity();
            let entity_cs = cx.entity();
            let entity_conn = cx.entity();
            let reg_btn = div()
                .id("nav-search-regex")
                .h_flex()
                .items_center()
                .justify_center()
                .px_1()
                .h_5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(if regex { fg } else { muted })
                .when(regex, |this| this.bg(selected_bg))
                .when(!regex, move |s| s.hover(move |s| s.bg(hover_bg)))
                .child(".*")
                .on_click(move |_, _, app| {
                    entity_regex.update(app, |this, cx| {
                        {
                            let mut v = this.nav.borrow_mut();
                            v.search_regex = !v.search_regex;
                        }
                        this.mark_panel_state_dirty(cx);
                        this.rerun_nav_search(cx);
                        cx.notify();
                    });
                });
            let case_btn = div()
                .id("nav-search-case")
                .h_flex()
                .items_center()
                .justify_center()
                .px_1()
                .h_5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(if case_sensitive { fg } else { muted })
                .when(case_sensitive, |this| this.bg(selected_bg))
                .when(!case_sensitive, move |s| s.hover(move |s| s.bg(hover_bg)))
                .child("Aa")
                .on_click(move |_, _, app| {
                    entity_cs.update(app, |this, cx| {
                        {
                            let mut v = this.nav.borrow_mut();
                            v.search_case_sensitive = !v.search_case_sensitive;
                        }
                        this.mark_panel_state_dirty(cx);
                        this.rerun_nav_search(cx);
                        cx.notify();
                    });
                });
            let conn_btn = div()
                .id("nav-search-connected")
                .h_flex()
                .items_center()
                .justify_center()
                .px_1()
                .h_5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(if connected_only { fg } else { muted })
                .when(connected_only, |this| this.bg(selected_bg))
                .when(!connected_only, move |s| s.hover(move |s| s.bg(hover_bg)))
                .child("已连接")
                .on_click(move |_, _, app| {
                    entity_conn.update(app, |this, cx| {
                        {
                            let mut v = this.nav.borrow_mut();
                            v.search_connected_only = !v.search_connected_only;
                        }
                        // 会话级开关不落库（`navigator_state.rs` 有他人改动），
                        // 但翻转后必须重搜——签名含该开关，`rerun` 自动判变。
                        this.rerun_nav_search(cx);
                        cx.notify();
                    });
                });

            // D11 匹配模式两档（前缀 / 包含，互斥）：默认前缀，对齐 DBeaver starts-with。
            // 会话级不落库；切换后重搜——签名含模式，`rerun` 自动判变。
            let prefix_active = matches!(name_mode, nav_jobs::NameMatchMode::StartsWith);
            let contains_active = matches!(name_mode, nav_jobs::NameMatchMode::Contains);
            let entity_prefix = cx.entity();
            let prefix_btn = Button::new("nav-search-mode-prefix")
                .ghost()
                .xsmall()
                .rounded_full()
                .px_1p5()
                .selected(prefix_active)
                .toggled(prefix_active)
                .label("前缀")
                .bg(if prefix_active { selected_bg } else { transparent_black() })
                .text_color(if prefix_active { fg } else { muted })
                .on_click(move |_, _, app| {
                    entity_prefix.update(app, |this, cx| {
                        this.nav.borrow_mut().search_name_mode = nav_jobs::NameMatchMode::StartsWith;
                        this.rerun_nav_search(cx);
                        cx.notify();
                    });
                });
            let entity_contains = cx.entity();
            let contains_btn = Button::new("nav-search-mode-contains")
                .ghost()
                .xsmall()
                .rounded_full()
                .px_1p5()
                .selected(contains_active)
                .toggled(contains_active)
                .label("包含")
                .bg(if contains_active { selected_bg } else { transparent_black() })
                .text_color(if contains_active { fg } else { muted })
                .on_click(move |_, _, app| {
                    entity_contains.update(app, |this, cx| {
                        this.nav.borrow_mut().search_name_mode = nav_jobs::NameMatchMode::Contains;
                        this.rerun_nav_search(cx);
                        cx.notify();
                    });
                });

            // D11 搜注释开关：开启时名称档并入 FTS 注释命中（类型按族 comments 档案过滤）。
            let entity_comments = cx.entity();
            let comments_btn = Button::new("nav-search-comments")
                .ghost()
                .xsmall()
                .rounded_full()
                .px_1p5()
                .selected(include_comments)
                .toggled(include_comments)
                .label("注释")
                .bg(if include_comments { selected_bg } else { transparent_black() })
                .text_color(if include_comments { fg } else { muted })
                .on_click(move |_, _, app| {
                    entity_comments.update(app, |this, cx| {
                        let current = this.nav.borrow().search_comments;
                        this.nav.borrow_mut().search_comments = !current;
                        this.rerun_nav_search(cx);
                        cx.notify();
                    });
                });

            // D11 搜定义：本批内省链路尚未采集视图 / 例程定义，开关置灰不可点、标注
            // 「待内省采集」；采集链路接通后改为可点（与注释开关同构）。
            let definition_btn = Button::new("nav-search-definitions")
                .ghost()
                .xsmall()
                .rounded_full()
                .px_1p5()
                .disabled(true)
                .label("定义")
                .text_color(muted)
                .tooltip("待内省采集：视图 / 例程定义批量采集链路接通后启用");
            let _ = include_definitions;

            search_options = Some(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .w_full()
                    .px_2()
                    .pb_1p5()
                    .bg(cx.theme().colors.group_box)
                    .child(prefix_btn)
                    .child(contains_btn)
                    .child(div().h(rems(1.)).w(ui::HAIRLINE).bg(border))
                    .child(comments_btn)
                    .child(definition_btn),
            );

            search_row = search_row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        Input::new(input)
                            .bordered(false)
                            .focus_bordered(true)
                            .w_full()
                            .prefix(search_icon)
                            .suffix(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_0p5()
                                    .child(case_btn)
                                    .child(reg_btn)
                                    .child(conn_btn),
                            ),
                    ),
            );
        }
        search_row = search_row.child(facet_button);

        div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .pt_1()
            .child(header)
            .child(search_row)
            .when_some(search_options, |this, options| this.child(options))
            .child(chips)
            .child(div().h(ui::HAIRLINE).w_full().bg(border))
            .child(body)
            .when_some(self.render_nav_status_bar(cx), |this, status| {
                this.child(div().h(ui::HAIRLINE).w_full().bg(border))
                    .child(status)
            })
            .key_context("database-nav")
            .track_focus(&self.focus_handle)
            .on_action({
                let entity = cx.entity();
                move |_: &NavUp, _window, app| {
                    entity.update(app, |this, cx| this.nav_move(-1, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavDown, _window, app| {
                    entity.update(app, |this, cx| this.nav_move(1, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavExpand, _window, app| {
                    entity.update(app, |this, cx| this.nav_expand(cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavCollapse, _window, app| {
                    entity.update(app, |this, cx| this.nav_collapse(cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavOpenProperties, window, app| {
                    entity.update(app, |this, cx| this.nav_open_properties(window, cx));
                }
            })
            // S4-② 键盘补全：Home / End 跳首尾、PageUp / PageDown 翻页。
            .on_action({
                let entity = cx.entity();
                move |_: &NavHome, _window, app| {
                    entity.update(app, |this, cx| this.nav_move_edge(true, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavEnd, _window, app| {
                    entity.update(app, |this, cx| this.nav_move_edge(false, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavPageUp, _window, app| {
                    entity.update(app, |this, cx| this.nav_move_page(-1, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavPageDown, _window, app| {
                    entity.update(app, |this, cx| this.nav_move_page(1, cx));
                }
            })
            // S4-② 前缀跳转：树获得焦点时按字母键跳到下一个以此开头的行。
            // 搜索框有自己的 Input 焦点（字符进搜索框不落这里）；这里只收
            // 「树键盘漫游中」的单字符键（gpui 的 `KeyDownEvent` 在 bubble 阶段）。
            .on_key_down({
                let entity = cx.entity();
                move |ev, _window, app| {
                    let key = ev.keystroke.key.chars().next();
                    if let Some(ch) = key {
                        if ch.is_alphabetic() {
                            entity.update(app, |this, cx| this.nav_jump_prefix(ch, cx));
                        }
                    }
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavReorderUp, _window, app| {
                    entity.update(app, |this, cx| this.nav_step_selected(-1, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavReorderDown, _window, app| {
                    entity.update(app, |this, cx| this.nav_step_selected(1, cx));
                }
            })
            // `Esc`：清空搜索框（需窗口：`InputState::set_value` 是窗口相关操作）。
            .on_action(cx.listener(|this, _: &NavClearSearch, window, cx| {
                this.clear_nav_search(window, cx);
            }))
            // `F2`：重命名。连接行走编辑对话框（与行尾 `✎` 同一入口）；
            // 树对象（S4-④）走行内重命名——需已连接（DDL 要活连接），未连接不进入。
            .on_action(cx.listener(|this, _: &NavRename, window, cx| {
                if let Some(item) = this.nav_selected() {
                    match item.path.as_ref() {
                        Some(NavPath::Connection) => {
                            this.host.edit_connection(&item.conn_id, window, cx);
                        }
                        _ => {
                            let Some(prop) = &item.property else { return; };
                            // M6：属性类别 → 文件夹，查表自 L1 族档案；不可重命名类别直接关闭。
                            let Some(folder) = family_profile::property_kind_to_folder(prop.kind)
                            else {
                                return;
                            };
                            if !this.host.is_connected(&item.conn_id) {
                                this.host.notice("对象重命名需要先建立连接".into(), cx);
                                return;
                            }
                            let driver = this
                                .host
                                .connections()
                                .iter()
                                .find(|c| c.id == item.conn_id)
                                .map(|c| c.driver.clone())
                                .unwrap_or_default();
                            this.nav.borrow_mut().object_rename_for = Some(ObjectRenameCtx {
                                key: item.key.clone(),
                                conn_id: item.conn_id.clone(),
                                driver,
                                folder,
                                catalog: prop.catalog.clone().unwrap_or_default(),
                                schema: prop.schema.clone().unwrap_or_default(),
                                name: prop.name.clone(),
                                parent: prop.parent.clone(),
                            });
                            this.focus_handle.focus(window, cx);
                            cx.notify();
                        }
                    }
                }
            }))
            // B6 staged：`Ctrl+Z` 撤销全部待提交变更（纯状态操作，Context 内完成）。
            .on_action(cx.listener(|this, _: &NavRevertAllStaged, _window, cx| {
                this.revert_staged(cx);
            }))
            // B6 staged：`Delete` 把当前选中对象加入待提交删除清单。
            // 两阶段：Context 内 `prepare_staged_enqueue` 完成校验并返回候选；
            // 校验通过后在 App 作用域开单项确认对话框（Context 回调拿不到 `&mut App`）。
            .on_action({
                let entity = cx.entity();
                move |_: &NavDeleteToStaged, window, app| {
                    let Some(prepared) =
                        entity.update(app, |this, cx| this.prepare_staged_enqueue(NavOp::Drop, cx))
                    else {
                        return;
                    };
                    let entity = entity.clone();
                    window.open_alert_dialog(app, move |alert, _window, _cx| {
                        let entity = entity.clone();
                        let prepared: PreparedStaged = prepared.clone();
                        alert
                            .confirm()
                            .title(prepared.title)
                            .description(prepared.description)
                            .button_props(
                                DialogButtonProps::default()
                                    .ok_text(prepared.ok_text)
                                    .ok_variant(ButtonVariant::Danger)
                                    .show_cancel(true),
                            )
                            .on_ok(move |_, _, app| {
                                let staged = prepared.staged.clone();
                                entity.update(app, |this, cx| {
                                    this.apply_staged_enqueue(staged, cx)
                                });
                                true
                            })
                    });
                }
            })
            // B6 staged：`Ctrl+S` 打开汇总确认对话框；空集由 `prepare_commit_staged` 记提示返回
            // None，不开窗。两阶段口径同 `DeleteToStaged`：Context 内拍快照，App 作用域开窗。
            .on_action({
                let entity = cx.entity();
                move |_: &NavCommitStaged, window, app| {
                    let Some(prepared) =
                        entity.update(app, |this, cx| this.prepare_commit_staged(cx))
                    else {
                        return;
                    };
                    let entity = entity.clone();
                    window.open_alert_dialog(app, move |alert, _window, _cx| {
                        let entity = entity.clone();
                        let prepared = prepared.clone();
                        let rows = staged_commit_rows(&prepared.plan, muted);
                        alert
                            .confirm()
                            .title(prepared.title)
                            .description(prepared.description)
                            .button_props(
                                DialogButtonProps::default()
                                    .ok_text(prepared.ok_text)
                                    .ok_variant(ButtonVariant::Danger)
                                    .show_cancel(true),
                            )
                            .children(rows)
                            .on_ok(move |_, _, app| {
                                let entity = entity.clone();
                                entity.update(app, |this, cx| this.commit_staged(cx));
                                true
                            })
                    });
                }
            })
            .id("nav-panel")
            .role(gpui::Role::Group)
    }

    /// 来源筛选 chip（全部 / 项目 / 全局 / 共享）；点击切换筛选，不占一级结构。
    ///
    /// 用 `Button` 骨架（ghost + selected + toggled）：白拿 hover / 焦点 / 键盘 / aria；
    /// 选中态视觉靠调用方样式（实底 accent 胶囊 / 未选中透明 + muted 字）——`Button` 的
    /// `.styles()` 会在选中 / 悬停状态内**后置重放**调用方样式（button.rs:788 注释原文），
    /// 所以选中实底在 hover / 选中态都不被组件样式覆盖。
    pub(super) fn nav_source_chip(
        &self,
        label: &str,
        target: Option<NavSource>,
        current: Option<NavSource>,
        muted: Hsla,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = target == current;
        let entity = cx.entity();
        let (bg, text) = if active {
            (accent, cx.theme().colors.primary_foreground)
        } else {
            // 未选中：无底色（透明），靠 hover 灰层给出可点反馈。
            (transparent_black(), muted)
        };
        let weight = if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        Button::new(format!(
            "nav-source-{}",
            target.map(|s| s.code()).unwrap_or("all")
        ))
        .ghost()
        .xsmall()
        .selected(active)
        .toggled(active)
        .label(label.to_string())
        .rounded_full()
        .px_1p5()
        .bg(bg)
        .text_color(text)
        .font_weight(weight)
        .on_click(move |_, _, app| {
            entity.update(app, |this, cx| {
                this.nav.borrow_mut().source_filter = target;
                this.write_nav_filters(cx);
                cx.notify();
            });
        })
    }

    /// 面板头连接状态徽标（需求①）：`N 连接 · M 在线` 常驻一眼可见，**点击**弹出
    /// 逐连接下拉面板——每连接一行（状态点 + 名称 + 状态 + 行尾「连接/断开」按钮），
    /// **在线（已连）排前**；按钮调用 [`NavView::toggle_connection`]（与面板头
    /// 断开按钮同一入口，乐观语义 + 后台建连）。
    ///
    /// 口径与 [`Self::render_nav_status_bar`] 完全一致（本会话建过连或记录标可用）；
    /// 状态判定与连接行徽标同一函数族（`loading` → 连接中 / `errors` → 失败 / `connected` → 已连）。
    /// 无连接时不显示（底部状态行同规则，空态已在说明同一件事）。
    ///
    /// 为什么用 `Popover` 而不是 hover：需求①要求「可操作」，面板里要放行内按钮；
    /// `Popover::content` 的闭包每帧重建，能实时反映连接状态变化（菜单实体是缓存
    /// 的，切换后内容会陈旧）。
    pub(super) fn render_nav_conn_badge(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        use gpui_kit::component::popover::Popover;
        let conns = self.host.connections();
        if conns.is_empty() {
            return None;
        }
        // 逐连接快照一次取好：Popover 的 content 闭包是 'static，不能在闭包里 borrow `nav`。
        let rows: Vec<NavConnSummaryRow> = {
            let view = self.nav.borrow();
            let mut rows: Vec<NavConnSummaryRow> = conns
                .iter()
                .map(|c| {
                    let err = view.errors.get(&c.id).cloned();
                    let connected = view.connected.contains(&c.id) || c.connected;
                    let status = if view.loading.contains(&c.id) {
                        NavBadgeStatus::Connecting
                    } else if err.is_some() {
                        NavBadgeStatus::Failed
                    } else if connected {
                        NavBadgeStatus::Connected
                    } else {
                        NavBadgeStatus::Idle
                    };
                    NavConnSummaryRow {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        status,
                        connected,
                    }
                })
                .collect();
            // 在线（已连）排前，其余保持原顺序（`sort_by_key` 稳定，不扰动分组序）。
            rows.sort_by_key(|r| !r.connected);
            rows
        };
        let online = rows.iter().filter(|r| r.connected).count();
        let total = rows.len();
        let muted = cx.theme().colors.muted_foreground;
        let hover_bg = cx.theme().colors.list_hover;
        let danger = cx.theme().colors.danger;
        let entity = cx.entity();
        let project_root = self.host.project_root();
        // 触发按钮：保留「N 连接 · M 在线」胶囊观感；Button 骨架自带 hover / 焦点 /
        // 键盘可达（可点元素优先 `Button`，禁手搓 div）。
        let trigger = Button::new("nav-conn-summary")
            .ghost()
            .xsmall()
            .px_1p5()
            .rounded_full()
            .bg(hover_bg)
            .text_color(muted)
            .label(format!("{total} 连接 · {online} 在线"))
            .tooltip("点击管理各连接：在线排前，行尾可连接 / 断开");
        Some(
            Popover::new("nav-conn-summary-popover")
                .trigger(trigger)
                .content(move |_state, _window, cx| {
                    let fg = cx.theme().colors.foreground;
                    let muted = cx.theme().colors.muted_foreground;
                    let mut list = div()
                        .v_flex()
                        .gap(rems(0.125))
                        .min_w(rems(14.))
                        .py_1()
                        .text_xs();
                    for r in &rows {
                        let color = r.status.color(cx.theme());
                        let status_label = r.status.label();
                        let entity = entity.clone();
                        let cid = r.id.clone();
                        let root = project_root.clone();
                        let line = div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .px_1()
                            .child(
                                div()
                                    .w(rems(0.5))
                                    .h(rems(0.5))
                                    .flex_none()
                                    .rounded_full()
                                    .bg(color),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_color(fg)
                                    .child(r.name.clone()),
                            )
                            // 状态列：失败用 danger 色，其余 muted（错误详情由连接行 hover 承载）。
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(if matches!(r.status, NavBadgeStatus::Failed) {
                                        danger
                                    } else {
                                        muted
                                    })
                                    .child(status_label),
                            )
                            // 行尾按钮：已连显示「断开」，未连显示「连接」。
                            .child(
                                Button::new(format!("nav-conn-toggle-{}", r.id))
                                    .ghost()
                                    .xsmall()
                                    .label(if r.connected { "断开" } else { "连接" })
                                    .on_click(move |_, _, app| {
                                        entity.update(app, |this, cx| {
                                            this.toggle_connection(
                                                &cid,
                                                root.as_deref().and_then(|p| p.to_str()),
                                                cx,
                                            );
                                        });
                                    }),
                            );
                        list = list.child(line);
                    }
                    list
                }),
        )
    }

    /// 面板底状态行（原型设计 §2）：`N 已连接 · M 离线 · 元数据更新于 X 分钟前`。
    ///
    /// 三条口径：
    /// - 一个连接都没有时整行不显示（空态已经在说同一件事，两处重复只是噪声）；
    /// - 离线为 0 时不写「0 离线」；
    /// - 元数据时间取自最近一次**成功回填**（`NavViewState::last_loaded_at`），没取过就不写。
    ///
    /// 「已连接」与本面板行徽标同一判据：本会话建过连（`nav.connected`）或连接记录
    /// 本身标为可用（`ConnectionItem::connected`）。
    pub(super) fn render_nav_status_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let conns = self.host.connections();
        if conns.is_empty() {
            return None;
        }
        let (muted, total, online, last_loaded_at) = {
            let view = self.nav.borrow();
            let online = conns
                .iter()
                .filter(|c| view.connected.contains(&c.id) || c.connected)
                .count();
            (
                cx.theme().colors.muted_foreground,
                conns.len(),
                online,
                view.last_loaded_at,
            )
        };
        let mut text = format!("{online} 已连接");
        if online < total {
            text.push_str(&format!(" · {} 离线", total - online));
        }
        if let Some(at) = last_loaded_at {
            text.push_str(&format!(
                " · 元数据更新于 {}",
                nav_relative_time(SystemTime::now(), at)
            ));
        }
        Some(
            div()
                .h_flex()
                .items_center()
                .w_full()
                .h(rems(ui::ROW_HEIGHT))
                .pl_2p5()
                .pr_2()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .debug_selector(|| "nav-status-bar".to_string())
                .child(text),
        )
    }

    /// 当前生效筛选的人读清单（空态说明用）：顺序与面板自上而下一致
    /// （归属域 chips → 附加 facet → 搜索词）。
    ///
    /// 为什么要说出来：被筛空时用户看到的是「没有匹配的数据源」，但**哪条约束**把列表
    /// 清空了得自己回上面逐项找——尤其搜索框里还留着上次的词时（chips 与搜索是两个入口，
    /// 两边都能筛）。
    pub(super) fn nav_filter_summary(&self, cx: &Context<Self>) -> Vec<String> {
        let mut parts: Vec<String> = Vec::new();
        {
            let view = self.nav.borrow();
            match view.source_filter {
                Some(NavSource::Project) => parts.push("归属域：项目".to_string()),
                Some(NavSource::Global) => parts.push("归属域：全局".to_string()),
                Some(NavSource::Shared) => parts.push("归属域：共享".to_string()),
                None => {}
            }
            if let Some(t) = view.type_filter.as_deref() {
                let from_catalog = self.nav_type_from_catalog(t);
                let from_catalog = from_catalog
                    .as_ref()
                    .map(|(name, cat)| (name.as_str(), cat.as_str()));
                parts.push(format!("类型：{}", nav_type_short_label(t, from_catalog)));
            }
            if let Some(d) = view.driver_filter.as_deref() {
                let name = self
                    .driver_catalog
                    .borrow()
                    .get(d)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| d.to_string());
                parts.push(format!("驱动：{name}"));
            }
            if let Some(t) = view.tag_filter.as_deref() {
                parts.push(format!("标签：{t}"));
            }
        }
        // 搜索框原样给出（内含 `type:` / `tag:` 这类 token 时也一眼看得出自己写了什么）。
        let raw = self
            .nav_search
            .as_ref()
            .map(|s| s.read(cx).value().to_string())
            .unwrap_or_default();
        let raw = raw.trim();
        if !raw.is_empty() {
            parts.push(format!("搜索：\"{raw}\""));
        }
        parts
    }

    /// 构建单个 facet 子菜单（「全部」+ 候选项，单选）。
    pub(super) fn build_facet_items(
        menu: PopupMenu,
        entity: Entity<Self>,
        facet: NavFacet,
        current: Option<String>,
        candidates: Vec<(String, String)>,
    ) -> PopupMenu {
        let mut menu = menu.item(
            PopupMenuItem::new("全部")
                .checked(current.is_none())
                .on_click({
                    let entity = entity.clone();
                    move |_, _, app| {
                        entity.update(app, |this, cx| this.apply_facet(facet, None, cx));
                    }
                }),
        );
        for (value, label) in candidates {
            let checked = current.as_deref() == Some(value.as_str());
            let entity = entity.clone();
            menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                move |_, _, app| {
                    let value = value.clone();
                    entity.update(app, |this, cx| {
                        this.apply_facet(facet, Some(value.clone()), cx);
                    });
                },
            ));
        }
        menu
    }

    /// 计算 facet 候选清单（类型 / 驱动 / 标签），值 → 展示名，已排序去重。
    ///
    /// 从驱动目录取该类型的（显示名, 分类）——同一 `type_id` 下多个驱动共享一份类型元数据。
    ///
    /// 用于只有类型 id、手上没有 `DriverMeta` 的场合（如筛选药丸文案）；
    /// 目录未就绪 / 类型不在目录里 → `None`（调用方回退到内置表）。
    pub(super) fn nav_type_from_catalog(&self, type_id: &str) -> Option<(String, String)> {
        self.driver_catalog
            .borrow()
            .values()
            .find(|m| m.type_id == type_id)
            .and_then(|m| Some((m.type_name.clone()?, m.type_category.clone()?)))
    }

    /// 树主体：「结果区标题行」（若有）+「加载中…」/ 虚拟列表 / 空态。
    ///
    /// 列表是**虚拟滚动**：只画视口内那几行，行集合与顺序由 [`Self::collect_nav_rows`]
    /// 一次算好（见 `docs/architecture/database/database-nav-dev-plan.md` §2.5）。
    /// 面板头 / 来源 chips / 搜索框 / 筛选弹层都在列表之外。
    pub(super) fn render_nav_body(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let mut body = div()
            .v_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .gap_0p5()
            .px_1()
            .pt_0p5()
            .pb_1();
        // 结果区**只剩一行说明**（命中行已并入列表：同一片滚动区、同一条 ↑↓ 漫游）。
        // 这里没有滚动也没有高度上限：它只有标题一行与偶发的说明行。
        if let Some(header) = self.render_search_header(cx) {
            body = body.child(header);
        }
        // 分组 / 成员 / 标签尚未就绪（首帧由 `render_nav` 的 defer 加载）。
        // 原型 loading 态用骨架行（`skel`）：分组就绪前给 4 条呼吸占位，避免「加载中…」裸文本。
        if !self.nav.borrow().groups_loaded {
            let muted = cx.theme().colors.muted_foreground;
            return body.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .px_1()
                    .pt_1()
                    .child(
                        div()
                            .h(rems(0.5625))
                            .w(rems(11.5))
                            .rounded_md()
                            .bg(muted.alpha(0.16)),
                    )
                    .child(
                        div()
                            .h(rems(0.5625))
                            .w(rems(8.5))
                            .rounded_md()
                            .bg(muted.alpha(0.16)),
                    )
                    .child(
                        div()
                            .h(rems(0.5625))
                            .w(rems(10.))
                            .rounded_md()
                            .bg(muted.alpha(0.16)),
                    )
                    .child(
                        div()
                            .h(rems(0.5625))
                            .w(rems(7.))
                            .rounded_md()
                            .bg(muted.alpha(0.16)),
                    ),
            );
        }
        if self.rows.is_empty() {
            return body.child(self.render_nav_empty_state(cx));
        }
        let sizes = self.nav_row_sizes(window.rem_size(), cx);
        let entity = cx.entity();
        let scroll = self.list_scroll.clone();
        body.child(
            div().flex_1().min_h_0().w_full().child(
                v_virtual_list(
                    entity,
                    "nav-tree-rows",
                    sizes,
                    move |this, range: std::ops::Range<usize>, _window, cx| {
                        range
                            .map(|ix| this.render_nav_row(ix, cx))
                            .collect::<Vec<AnyElement>>()
                    },
                )
                .track_scroll(&scroll),
            ),
        )
    }

    /// 空态：没有任何连接（引导新建）/ 有连接但都过不了筛选。
    ///
    /// 原型空态三要素：居中大图标块 + 标题 + 说明 + 主按钮（居中布局，非左上角文本）。
    pub(super) fn render_nav_empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::empty::{Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle};
        let muted = cx.theme().colors.muted_foreground;
        // 图标走 EmptyMedia::Icon 变体（2rem 圆角 muted 底 + 居中图标），
        // 与 analytics_resource / insight / project 空态统一。
        if self.host.connections().is_empty() {
            let host = self.host.clone();
            return Empty::new()
                .header(
                    EmptyHeader::new()
                        .media(
                            EmptyMedia::new()
                                .with_variant(gpui_kit::component::empty::EmptyMediaVariant::Icon)
                                .child(
                                    Icon::empty()
                                        .path("icons/database.svg")
                                        .text_color(muted),
                                ),
                        )
                        .title(EmptyTitle::new().child("还没有数据源"))
                        .description(
                            EmptyDescription::new().child("当前项目与全局库均无连接。点击下方「新建连接」开始。"),
                        ),
                )
                .content(
                    EmptyContent::new().child(
                        Button::new("nav-empty-new-connection")
                            .primary()
                            .small()
                            .icon(IconName::Plus)
                            .label("新建连接")
                            .on_click(move |_, window, app: &mut App| {
                                host.new_connection(window, app);
                            }),
                    ),
                );
        }
        // 连接都在，只是都被筛掉了：空列表必须给出**出路**（原型 v9 修订点）——
        // 只说「没有匹配」而不说「谁把它筛空的」+「怎么回去」，用户只能自己回上面逐项找。
        let entity = cx.entity();
        let input = self.nav_search.clone();
        let summary = self.nav_filter_summary(cx);
        let description = if summary.is_empty() {
            "试试调整搜索词或筛选条件。".to_string()
        } else {
            format!("当前筛选：{}", summary.join(" · "))
        };
        Empty::new()
            .header(
                EmptyHeader::new()
                    .media(
                        EmptyMedia::new()
                            .with_variant(gpui_kit::component::empty::EmptyMediaVariant::Icon)
                            .child(
                                Icon::empty()
                                    .path("icons/search.svg")
                                    .text_color(muted),
                            ),
                    )
                    .title(EmptyTitle::new().child("没有匹配的数据源"))
                    .description(EmptyDescription::new().child(description)),
            )
            .content(
                EmptyContent::new().child(
                    Button::new("nav-empty-clear-filters")
                        .ghost()
                        .small()
                        .label("清除筛选")
                        .on_click(move |_, window, app: &mut App| {
                            // 搜索词与 facet 是两个独立入口，清就一起清：
                            // 只清一边，点完还是空列表。
                            if let Some(input) = &input {
                                input.update(app, |s, cx| s.set_value("", window, cx));
                            }
                            entity.update(app, |this, cx| this.clear_nav_filters(cx));
                        }),
                ),
            )
    }

    /// 搜索结果**标题行**（树顶；有查询词时出现）。
    ///
    /// 只有标题与说明，没有命中行：命中行已并入虚拟列表（[`NavRow::SearchHit`]），
    /// 标题留在列表之外，是为了「正在搜 / 无命中 / 共几条」随时可见（滚到树深处也不丢）。
    ///
    /// 与本地过滤的分工（两者同时生效，互不替代）：
    /// - 本地过滤：命中**已加载**的节点（零往返、即时，改一个字就变）；
    /// - 索引搜索：命中**索引里的全部对象**（含尚未展开到的 schema），跨连接。
    pub(super) fn render_search_header(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (query, hits, searched) = {
            let view = self.nav.borrow();
            (
                view.search_query.clone()?,
                view.search_hits.len(),
                view.search_searched,
            )
        };
        let pri = cx.theme().colors.primary;
        let muted = cx.theme().colors.muted_foreground;
        let max = ui::NAV_SEARCH_MAX_ROWS;

        let title =
            if nav_jobs::has_pending_search(nav_jobs::SearchConsumer::Navigator) && hits == 0 {
                format!("索引搜索：{query}（搜索中…）")
            } else if hits == 0 {
                format!("索引搜索：{query}（无命中；已搜 {searched} 个有缓存的连接）")
            } else {
                format!("索引搜索：{query}（{hits} 条，覆盖 {searched} 个连接）")
            };

        let mut block = div()
            .v_flex()
            .w_full()
            .flex_none()
            .gap_0p5()
            .pb_0p5()
            .child(
                div()
                    .w_full()
                    .px_1()
                    .pb_0p5()
                    .text_xs()
                    .text_color(pri)
                    .child(title),
            );
        if hits > max {
            block = block.child(
                div()
                    .w_full()
                    .px_1()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("仅显示前 {max} 条（缩小搜索词可看到更多）")),
            );
        }
        // 定位的结局如实写在这里（不可定位 / 索引里没找到）：成功或未开定位时没有这一行。
        if let Some(note) = self.nav.borrow().reveal_note.clone() {
            block = block.child(
                div()
                    .w_full()
                    .px_1()
                    .pb_0p5()
                    .text_xs()
                    .text_color(cx.theme().colors.danger)
                    .child(note),
            );
        }
        Some(block)
    }
}
