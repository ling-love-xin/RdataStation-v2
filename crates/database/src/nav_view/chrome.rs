//! 面板外壳：面板头 / 搜索行 / 归属域 chips / facet 弹层 / 结果区标题行 / 空态 / 底部状态行。
//!
//! 与 `rows.rs` 的边界：本模块负责「列表之外的一切」——列表本体、虚拟列表装配、
//! 行高计算、以及**每一种行的渲染**（含搜索结果行）都在 `rows.rs`；
//! 本模块只留结果区的**标题行**（“搜索中 / 无命中 / 共几条”）。

use super::*;

impl NavView {
    /// 数据源导航面板（M4）：面板头 + 来源 chips + 搜索 + 分组树。
    pub fn render_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if self.nav_search.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx));
            state.update(cx, |s, cx| {
                s.set_placeholder("筛选数据源 / 表 / 列 / 标签…", window, cx)
            });
            // 输入变化时通知面板重渲染，否则过滤词不会即时生效；
            // 同时把搜索词标脏（防抖落库，重启后搜索框回到原样）。
            let sub = cx.subscribe_in(&state, window, |this, _e, ev: &InputEvent, _w, cx| {
                if matches!(ev, InputEvent::Change) {
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
                    let current =
                        crate::nav_store::list_tags(conn_id, self.host.project_root().as_deref());
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

        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let accent = cx.theme().colors.primary;
        let source_filter = self.nav.borrow().source_filter;

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

        let header = div()
            .h_flex()
            .items_center()
            .w_full()
            // 面板头高取 App 统一的 `PANEL_HEADER_HEIGHT`（36px）：这里曾是本仓唯一的
            // 30px 裸值（`rems(1.875)`），与编辑器工具条 / 设置页 / 资产库面板头对不齐
            // （规范：`ui-design-spec.md` §2.1）。
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .pl_2p5()
            .pr_2()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg)
                    .child("数据源"),
            )
            .child(div().flex_1())
            // C1 预热进度（后台任务进行中时显示，可取消）。
            .when(nav_jobs::warm_active(), |h| {
                let (done, total) = nav_jobs::warm_progress();
                h.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("预热 {done}/{total}")),
                )
                .child(
                    div()
                        .id("nav-warm-cancel")
                        .px_1()
                        .rounded_md()
                        .text_xs()
                        .text_color(muted)
                        .cursor_pointer()
                        .hover({
                            let h = cx.theme().colors.list_hover;
                            move |s| s.bg(h)
                        })
                        .child("取消")
                        .on_click(|_, _, _| nav_jobs::cancel_warm()),
                )
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
                div()
                    .id("nav-new-group")
                    .w_6()
                    .h_6()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover({
                        let h = cx.theme().colors.list_hover;
                        move |s| s.bg(h)
                    })
                    .child(nav_icon("icons/folder-plus.svg", ui::ICON_SIZE_SM, muted))
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
            .child(
                // 断开当前连接（设计 §2.1 / 原型 title「断开当前连接」）：仅运行时已连接时
                // 可用；断开只关运行时连接，元数据缓存保留（可离线浏览 / 重连秒开）。
                Button::new("nav-disconnect")
                    .ghost()
                    .small()
                    .icon(
                        Icon::empty()
                            .path("icons/plug.svg")
                            .text_color(cx.theme().colors.danger),
                    )
                    .disabled(!current_connected)
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
            .child(
                // 更多（⋯）：刷新全部元数据 / 缓存管理。
                Button::new("nav-more")
                    .ghost()
                    .small()
                    .icon(IconName::Ellipsis)
                    .dropdown_menu({
                        let entity = cx.entity();
                        let host_tags = self.host.clone();
                        let host_scope = self.host.clone();
                        let host_cache = self.host.clone();
                        let show_tags = self.host.show_tags(cx);
                        let show_scope = self.host.show_scope(cx);
                        move |menu, _window, _cx| {
                            let e_refresh = entity.clone();
                            let host_cache = host_cache.clone();
                            menu.item(PopupMenuItem::new("刷新全部元数据").on_click(
                                move |_, _, app| {
                                    e_refresh.update(app, |this, cx| this.refresh_all(cx));
                                },
                            ))
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
                                PopupMenuItem::new("缓存管理…").on_click(move |_, window, app| {
                                    host_cache.open_cache_dialog(window, app);
                                }),
                            )
                        }
                    }),
            );

        let chips = div()
            .h_flex()
            .items_center()
            .w_full()
            .gap_1()
            .pl_2p5()
            .pr_2()
            .pb_1p5()
            .child(self.nav_source_chip("全部", None, source_filter, fg, muted, accent, cx))
            .child(self.nav_source_chip(
                "项目",
                Some(NavSource::Project),
                source_filter,
                fg,
                muted,
                accent,
                cx,
            ))
            .child(self.nav_source_chip(
                "全局",
                Some(NavSource::Global),
                source_filter,
                fg,
                muted,
                accent,
                cx,
            ))
            .child(self.nav_source_chip(
                "共享",
                Some(NavSource::Shared),
                source_filter,
                fg,
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

        // 搜索行：输入框 + 「筛选 ▾ N」弹层（类型 / 驱动 / 标签；归属域由上方 chips 承担）。
        let active_facets = self.nav_active_facet_count();
        let facet_label = if active_facets > 0 {
            format!("筛选 ▾ {active_facets}")
        } else {
            "筛选 ▾".to_string()
        };
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
            .label(facet_label)
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
                    menu.submenu(gl.clone(), window, cx, {
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
                    })
                }
            });

        let mut search_row = div().h_flex().items_center().w_full().gap_1().px_2().pb_2();
        if let Some(input) = &self.nav_search {
            // 搜索框前导放大镜（原型 §2 的 `[🔍 …]`）：面板头部三行控件里
            // 只有这一行是输入框，给个形状信号比只靠 placeholder 更快认出。
            let search_icon = nav_icon("icons/search.svg", ui::ICON_SIZE_SM, muted);
            search_row = search_row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(input).prefix(search_icon)),
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
                move |_: &NavOpenProperties, _window, app| {
                    entity.update(app, |this, cx| this.nav_open_properties(cx));
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
    }

    /// 来源筛选 chip（全部 / 项目 / 全局 / 共享）；点击切换筛选，不占一级结构。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn nav_source_chip(
        &self,
        label: &str,
        target: Option<NavSource>,
        current: Option<NavSource>,
        _fg: Hsla,
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
        // 未选中态在 hover 时给一层浅底，保持“可点”的反馈。
        let hover_bg = if active {
            accent
        } else {
            cx.theme().colors.list_hover
        };
        div()
            .id(format!(
                "nav-source-{}",
                target.map(|s| s.code()).unwrap_or("all")
            ))
            .h_flex()
            .items_center()
            .px_1p5()
            .py_0p5()
            .rounded_full()
            .bg(bg)
            .text_xs()
            .font_weight(weight)
            .text_color(text)
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .child(label.to_string())
            .on_click(move |_, _, app| {
                entity.update(app, |this, cx| {
                    this.nav.borrow_mut().source_filter = target;
                    this.write_nav_filters(cx);
                    cx.notify();
                });
            })
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
        if !self.nav.borrow().groups_loaded {
            return body.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("加载中…"),
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
    pub(super) fn render_nav_empty_state(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let fg = cx.theme().colors.foreground;
        if self.host.connections().is_empty() {
            let host = self.host.clone();
            return div()
                .w_full()
                .pt_5()
                .px_3()
                .v_flex()
                .items_start()
                .gap_1p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child("还没有数据源"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("当前项目与全局库均无连接。点击右上角「＋」新建。"),
                )
                .child(
                    Button::new("nav-empty-new-connection")
                        .primary()
                        .small()
                        .icon(IconName::Plus)
                        .label("新建连接")
                        .on_click(move |_, window, app: &mut App| {
                            host.new_connection(window, app);
                        }),
                );
        }
        // 连接都在，只是都被筛掉了：空列表必须给出**出路**（原型 v9 修订点）——
        // 只说「没有匹配」而不说「谁把它筛空的」+「怎么回去」，用户只能自己回上面逐项找。
        //
        // 为何不再判「有没有搜索命中」：命中行已并入列表（`NavRow::SearchHit`），
        // 有命中时 `self.rows` 非空，就走不到空态——旧写法要显式挡一下才不至于让
        // 「没有匹配的数据源」与一批命中同屏打架，那层判断现在由行集合自己承担。
        let entity = cx.entity();
        let input = self.nav_search.clone();
        let summary = self.nav_filter_summary(cx);
        let mut card = div()
            .w_full()
            .pt_5()
            .px_3()
            .v_flex()
            .items_start()
            .gap_1p5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("没有匹配的数据源"),
            );
        if !summary.is_empty() {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("当前筛选：{}", summary.join(" · "))),
            );
        }
        card.child(
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
