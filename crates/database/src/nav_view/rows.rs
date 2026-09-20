//! 导航树的**可见行**：扁平化、行高、以及「这一行长什么样」。
//!
//! 分工：`nav_rows.rs`（`crate::nav_rows`）只管顺序与身份，本模块管渲染与高度；
//! 两者成对维护——虚拟列表按给定高度定义布局，估小会压行（`v_virtual_list` 不回写实测值）。
//!
//! 为什么独立成模块：这里装着**七种行**的渲染（分组头 / 连接 / 引用 / 树行 / 「加载更多」/
//! 「已定位」/ 搜索命中）与它们的高度估算，是全文件最大的一块；与面板外壳（`chrome.rs`）
//! 分开后，改行样式不必跨过整个面板的脚手架。

use super::*;

impl NavView {
    /// 把某一行滚进视口（虚拟列表换来的可编程滚动入口）。
    ///
    /// 返回是否找到了这一行：定位是跨帧推进的（行集合下一帧才包含目标），
    /// 找不到就把意图留着，别把一次有效请求丢掉。
    ///
    /// 为何值得单独接：以前树是自绘递归，没有这个入口——「在树中定位」只能保证
    /// 选中 + 渲染窗口罩住目标，不能保证它在**可视区**。
    pub(super) fn nav_scroll_to_key(&self, key: &str) -> bool {
        let Some(ix) = self.rows.iter().position(|row| row.key() == key) else {
            return false;
        };
        // Center：目标放中间，父节点与邻行一起可见（Top 会把上下文切在屏外）。
        self.list_scroll.scroll_to_item(ix, ScrollStrategy::Center);
        true
    }

    /// 画第 `ix` 行（虚拟列表的 item 渲染器）。
    ///
    /// 顺序 / 归属 / 深度来自 [`Self::collect_nav_rows`]；这里只管「这一行长什么样」。
    /// 实体会在**布局期**（不是 render 期）被借用进来，所以这里只读状态、不写状态。
    pub(super) fn render_nav_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(ix) else {
            // 行集合刚变的那一帧可能还拿旧索引来问：给空元素，不 panic。
            return div().into_any_element();
        };
        match row {
            NavRow::GroupHeader {
                id,
                name,
                count,
                connected,
                failed,
            } => self
                .render_group_header(id, name, *count, *connected, *failed, cx)
                .into_any_element(),
            NavRow::Connection { conn, scope_key } => self
                .render_connection_row(conn, scope_key, cx)
                .into_any_element(),
            NavRow::Reference {
                conn,
                scope_key,
                primary_name,
            } => self
                .render_reference_row(conn, scope_key, primary_name, cx)
                .into_any_element(),
            NavRow::Tree {
                node,
                depth,
                scope_key,
            } => self
                .render_nav_node(node, *depth, scope_key, cx)
                .into_any_element(),
            NavRow::More { node, depth, .. } => {
                let (loaded, total) = self.nav_more_numbers(node);
                self.render_more_row(node, loaded, total, *depth, cx)
                    .into_any_element()
            }
            NavRow::Jump {
                node,
                depth,
                position,
                ..
            } => {
                let loaded = self.nav_node_loaded(&node.key);
                self.render_jumped_row(node, *position, loaded, *depth, cx)
                    .into_any_element()
            }
            NavRow::SearchHit { hit, ix } => {
                self.render_search_hit(hit, *ix, cx).into_any_element()
            }
        }
    }

    /// 某节点当前已加载的子节点条数。
    pub(super) fn nav_node_loaded(&self, key: &str) -> usize {
        self.nav
            .borrow()
            .children
            .get(key)
            .map(|c| c.len())
            .unwrap_or(0)
    }

    /// 「加载更多」行的两个数：已加载 / 数据侧总数。
    pub(super) fn nav_more_numbers(&self, node: &NavNode) -> (usize, usize) {
        let loaded = self.nav_node_loaded(&node.key);
        if !matches!(&node.kind, NavNodeKind::Folder(_)) {
            return (loaded, loaded);
        }
        (loaded, self.node_total(&node.key, loaded))
    }

    /// 虚拟列表的每行高度（`v_virtual_list` 只吃「每行多高」：它按给定高度定义布局，
    /// 不会反过来测量行内容）。
    ///
    /// 与渲染**成对维护**：附加行（[`nav_subline`]）与行内编辑器（`render_*_editor`）的
    /// 实际高度都已钉成 `ui::NAV_*` 常量，这里按同一组常量相加。
    /// 估大只会多留一段空隙；估小会让下一行压上来。
    pub(super) fn nav_row_sizes(&self, rem: Pixels, cx: &App) -> Rc<Vec<Size<Pixels>>> {
        let view = self.nav.borrow();
        let show_tags = self.host.show_tags(cx);
        // 行高表走共用原语（`workbench_shell::tree`）：与草稿箱同一套「基础高 + 附加块」口径。
        tree::row_sizes(self.rows.len(), rem, |ix| {
            nav_row_height(&self.rows[ix], &view, show_tags)
        })
    }

    /// 把可见行投影成**键盘漫游序列**（`nav_order`）。
    ///
    /// 为何是投影而不是渲染副产物：渲染现在只画视口内那几行，没被画到的行也得能
    /// ↑↓ 走到（否则“滚到哪才能选到哪”，与「在树中定位」的需求直接冲突）。
    ///
    /// 分组头是容器标题（[`NavRow::selectable`] 为假）不进序列；引用行 / 「更多」/
    /// 「已定位」/ 搜索命中行**进**序列——它们可点可选中，旧口径漏列会让 ↓ 跳过一行看得见的行。
    pub(super) fn sync_nav_order(&self) {
        let view = self.nav.borrow();
        let mut order = self.nav_order.borrow_mut();
        order.clear();
        for row in &self.rows {
            let (conn_id, path, property, has_children) = match row {
                NavRow::GroupHeader { .. } => continue,
                NavRow::Connection { conn, .. } => (
                    conn.id.clone(),
                    Some(NavPath::Connection),
                    Some(PropertyRef {
                        conn_id: conn.id.clone(),
                        source: NavSource::from_conn_id(&conn.id),
                        catalog: None,
                        schema: None,
                        parent: None,
                        name: conn.name.clone(),
                        kind: PropertyKind::Connection,
                    }),
                    true,
                ),
                // 引用行只指路：不展开、不打开属性（Enter / ←→ 落在它上面是无操作）。
                NavRow::Reference { conn, .. } => (conn.id.clone(), None, None, false),
                NavRow::Tree { node, .. } => (
                    node.connection_id.clone(),
                    node.expand_path.clone(),
                    node.property.clone(),
                    node.has_children,
                ),
                // 「更多」/「已定位」行自己带点击行为（见 `render_more_row`）。
                NavRow::More { node, .. } | NavRow::Jump { node, .. } => {
                    (node.connection_id.clone(), None, None, false)
                }
                // 搜索命中行：Enter 开属性面板（与行单击同一件事）；它不在树上，
                // 没有展开路径（←→ 落在它上面是无操作，要定位得点行尾的「定位」）。
                NavRow::SearchHit { hit, .. } => (
                    hit.conn_id.clone(),
                    None,
                    nav_search_hit_property(hit),
                    false,
                ),
            };
            order.push(NavOrderItem {
                key: row.key(),
                expanded: view.expanded.contains(&row.key()),
                conn_id,
                path,
                property,
                has_children,
            });
        }
    }

    /// 驱动 id → 数据源类型 id（目录优先；目录还没到时退化为驱动 id 本身）。
    ///
    /// 类型筛选要按 `type_id` 比而不是驱动 id：一个类型可以对应多个驱动。
    pub(super) fn nav_type_of(&self, driver: &str) -> String {
        self.driver_catalog
            .borrow()
            .get(driver)
            .map(|m| m.type_id.clone())
            .unwrap_or_else(|| driver.to_string())
    }

    /// 连接是否通过**连接级约束**（不含搜索词）。索引搜索「该搜哪些连接」用它。
    pub(super) fn nav_conn_passes_facets(
        &self,
        conn_filter: &NavConnFilter,
        conn: &ConnectionItem,
        tags: &HashMap<String, Vec<String>>,
    ) -> bool {
        conn_filter.passes(conn, tags.get(&conn.id), |driver| self.nav_type_of(driver))
    }

    /// 树里的连接行：在连接级约束之上，搜索词也顺手命中连接名 / 标签。
    ///
    /// 搜索词不参与索引搜索的目标筛选（那是**对象级**查询），但树里若不认连接名，
    /// 搜连接名时整棵树会被清空，看起来像坏了。
    pub(super) fn nav_conn_passes_tree_row(
        &self,
        conn_filter: &NavConnFilter,
        conn: &ConnectionItem,
        tags: &HashMap<String, Vec<String>>,
        filter: &str,
    ) -> bool {
        if !self.nav_conn_passes_facets(conn_filter, conn, tags) {
            return false;
        }
        if filter.is_empty() {
            return true;
        }
        if conn.name.to_lowercase().contains(filter) {
            return true;
        }
        tags.get(&conn.id)
            .map(|ts| ts.iter().any(|t| t.to_lowercase().contains(filter)))
            .unwrap_or(false)
    }

    /// 收集本次**可见行**（顺序权威；虚拟列表的 item 源与漫游序列都读它）。
    ///
    /// 这是原递归渲染里「顺序与身份」的那一半：哪些行可见、按什么顺序、属于哪个容器 / 分组。
    /// 「这一行长什么样」仍归 `render_*`（选中 / 展开 / 错误 / 悬停都由它们自己读），
    /// 本函数不复制那些判据，否则两份口径会各自漂移。
    ///
    /// 两处副作用跟着顺序走（[`Self::ensure_nav_loaded`] / [`Self::nav_schedule_index_search`]）：
    /// 把它们留在旧渲染路径上，切到虚拟列表后就会静默失效——展开不再加载、搜索不再排队。
    ///
    /// 分组 / 成员尚未就绪时只返回搜索命中行（`cx` 仍需传入：搜索排程就在本函数里）。
    pub(crate) fn collect_nav_rows(
        &self,
        source_filter: Option<NavSource>,
        cx: &mut Context<Self>,
    ) -> Vec<NavRow> {
        // 分组 / 成员未就绪：树行一行也没有，但命中行照旧要显示。
        //
        // 为何不被这道门拦住：命中是跨连接的**索引**结果，不依赖分组 / 成员是否已从库回来；
        // 挡在门后会让「开面板就搜」的那一瞬看不到结果（旧的结果区也不受这道门管）。
        if !self.nav.borrow().groups_loaded {
            return self.collect_search_rows();
        }
        let (filter, groups, membership, group_order, ungrouped_order, tags) = {
            let view = self.nav.borrow();
            (
                view.filter.to_lowercase(),
                view.groups.clone(),
                view.membership.clone(),
                view.group_order.clone(),
                view.ungrouped_order.clone(),
                view.tags.clone(),
            )
        };
        let conns: Vec<ConnectionItem> = self.host.connections();
        let by_id: HashMap<&str, &ConnectionItem> =
            conns.iter().map(|c| (c.id.as_str(), c)).collect();
        let conn_filter = NavConnFilter::snapshot(&self.nav.borrow(), source_filter);
        let primary_explicit = self.nav.borrow().primary_group.clone();
        // 主组：用户**显式指定**优先；未指定时回退到分组排序最靠前的一个。
        // 多组只全亮呈现一次，其余组以引用行出现。
        let primary_gid =
            |conn_id: &str| nav_primary_scope(&membership, &primary_explicit, conn_id);

        self.nav_schedule_index_search(&conn_filter, &conns, &filter, cx);

        // 命中行排在最前（与旧的「结果区在树顶」同观感），且必须在**排程之后**取：
        // 排程可能把过期命中清掉（查询词改了 / 太短），先取会把“刚被清掉的结果”多画一帧。
        let mut rows: Vec<NavRow> = self.collect_search_rows();

        // 运行时连接状态与错误集（分组头聚合健康度用；一次算完避免逐条查询）。
        let (connected_set, error_set) = {
            let view = self.nav.borrow();
            let mut set: HashSet<String> = view.connected.iter().cloned().collect();
            for c in &conns {
                if c.connected {
                    set.insert(c.id.clone());
                }
            }
            let errors: HashSet<String> = view.errors.keys().cloned().collect();
            (set, errors)
        };

        let mut shown = 0usize;
        for group in &groups {
            // 组内顺序以存储的手动排序为准（缺省无成员）。
            let members: Vec<&ConnectionItem> = group_order
                .get(&group.id)
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| by_id.get(id.as_str()).copied())
                        .filter(|c| self.nav_conn_passes_tree_row(&conn_filter, c, &tags, &filter))
                        .collect()
                })
                .unwrap_or_default();
            if members.is_empty() {
                continue;
            }
            shown += members.len();
            rows.push(NavRow::GroupHeader {
                id: group.id.clone(),
                name: group.name.clone(),
                count: members.len(),
                connected: members
                    .iter()
                    .filter(|c| connected_set.contains(&c.id))
                    .count(),
                failed: members.iter().filter(|c| error_set.contains(&c.id)).count(),
            });
            if self.group_collapsed(&group.id) {
                continue;
            }
            for conn in members {
                if primary_gid(&conn.id).as_deref() == Some(group.id.as_str()) {
                    self.collect_connection_rows(&mut rows, conn, &group.id, cx);
                } else {
                    // 引用行：全亮行在主组，这里只指路（不展开、不递归子节点）。
                    let primary_name = primary_gid(&conn.id)
                        .and_then(|gid| groups.iter().find(|g| g.id == gid).map(|g| g.name.clone()))
                        .unwrap_or_else(|| "未分组".to_string());
                    rows.push(NavRow::Reference {
                        conn: Box::new(conn.clone()),
                        scope_key: group.id.clone(),
                        primary_name,
                    });
                }
            }
        }

        // 「未分组」固定容器：收纳不属于任何自定义分组的连接。
        // 顺序与分组内同一条规则（`nav_order_members`）：手动排序在前，未排过的按名称升序。
        let candidates: Vec<&ConnectionItem> = conns
            .iter()
            .filter(|c| {
                membership
                    .get(&c.id)
                    .map(|gs| gs.is_empty())
                    .unwrap_or(true)
                    && self.nav_conn_passes_tree_row(&conn_filter, c, &tags, &filter)
            })
            .collect();
        let ranked: HashMap<&str, i64> = ungrouped_order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i as i64))
            .collect();
        let stored: Vec<(String, Option<i64>)> = candidates
            .iter()
            .map(|c| (c.id.clone(), ranked.get(c.id.as_str()).copied()))
            .collect();
        let ungrouped: Vec<&ConnectionItem> = nav_order_members(&stored, |id| {
            by_id
                .get(id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.to_string())
        })
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).copied())
        .collect();
        // 已有自定义分组时也渲染（空）未分组头：它是「拖拽移出分组」的常驻落点。
        if !ungrouped.is_empty() || (!groups.is_empty() && shown > 0) {
            rows.push(NavRow::GroupHeader {
                id: GROUP_UNGROUPED.to_string(),
                name: "未分组".to_string(),
                count: ungrouped.len(),
                connected: ungrouped
                    .iter()
                    .filter(|c| connected_set.contains(&c.id))
                    .count(),
                failed: ungrouped
                    .iter()
                    .filter(|c| error_set.contains(&c.id))
                    .count(),
            });
            if !self.group_collapsed(GROUP_UNGROUPED) {
                for conn in ungrouped {
                    self.collect_connection_rows(&mut rows, conn, GROUP_UNGROUPED, cx);
                }
            }
        }
        rows
    }

    /// 搜索结果行（索引命中，按位次进列表）。
    ///
    /// 上限仍是 `ui::NAV_SEARCH_MAX_ROWS`，但语义变了：它是「最多**渲染**多少条命中」，
    /// 不再是「一个 128px 小窗里塞多少条」——命中行现在与其他行共用同一片滚动区，
    /// 超出上限的部分由面板里那行说明（「仅显示前 N 条」）交代。
    ///
    /// 为何与本地筛选无关：本地过滤只遍历**已加载**的节点，索引搜索覆盖全部已建索引的对象，
    /// 两者互补（旧的结果区同样不受本地过滤影响）。
    fn collect_search_rows(&self) -> Vec<NavRow> {
        let view = self.nav.borrow();
        if view.search_hits.is_empty() {
            return Vec::new();
        }
        view.search_hits
            .iter()
            .take(ui::NAV_SEARCH_MAX_ROWS)
            .enumerate()
            .map(|(ix, hit)| NavRow::SearchHit {
                hit: Box::new(hit.clone()),
                ix,
            })
            .collect()
    }

    /// 连接行 + （展开时）它的子树。
    ///
    /// 与旧渲染同一条门：**仅在运行时已连接时**才排加载。
    /// 为何加这道门：展开态会跨重启从 `navigator_state` 恢复，但运行时连接不跨重启；
    /// 若此时仍排队，`NavigatorService` → `MetadataService` 取不到句柄，
    /// 会冒泡为用户看到的 `[CONN_NOT_FOUND]`。
    pub(super) fn collect_connection_rows(
        &self,
        rows: &mut Vec<NavRow>,
        conn: &ConnectionItem,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) {
        let (expanded, connected, children) = {
            let view = self.nav.borrow();
            (
                view.expanded.contains(&conn.id),
                view.connected.contains(&conn.id) || conn.connected,
                view.children.get(&conn.id).cloned().unwrap_or_default(),
            )
        };
        if expanded && connected {
            self.ensure_nav_loaded(&conn.id, &conn.id, NavPath::Connection, false, cx);
        }
        rows.push(NavRow::Connection {
            conn: Box::new(conn.clone()),
            scope_key: scope_key.to_string(),
        });
        if !expanded {
            return;
        }
        for child in &children {
            self.collect_node_rows(rows, child, 1, scope_key, cx);
        }
    }

    /// 对象树行 + （展开时）它的子树（`depth` 决定缩进；与旧 `render_nav_node` 逐条对应）。
    pub(super) fn collect_node_rows(
        &self,
        rows: &mut Vec<NavRow>,
        node: &NavNode,
        depth: usize,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) {
        let filter = self.nav.borrow().filter.to_lowercase();
        let expanded = self.nav.borrow().expanded.contains(&node.key);
        if expanded && node.has_children {
            if let Some(p) = node.expand_path.clone() {
                self.ensure_nav_loaded(&node.connection_id, &node.key, p, false, cx);
            }
        }
        let (skip, expanded_eff, children) = {
            let view = self.nav.borrow();
            let children = view.children.get(&node.key).cloned().unwrap_or_default();
            // 分页未拉完时，本地过滤只能覆盖**已加载**部分：此时不能因“已加载的都不匹配”
            // 就把整支隐藏——那会让用户连「加载更多」都点不到，看上去像对象不存在。
            let pending_more = view
                .child_total
                .get(&node.key)
                .map(|total| *total > children.len())
                .unwrap_or(false);
            let skip = !filter.is_empty()
                && !pending_more
                && !nav_node_matches(&view.children, node, &filter);
            (skip, expanded || !filter.is_empty(), children)
        };
        if skip {
            return;
        }
        rows.push(NavRow::Tree {
            node: Box::new(node.clone()),
            depth,
            scope_key: scope_key.to_string(),
        });
        if !expanded_eff {
            return;
        }
        // 类别文件夹分页：大 schema 分批取，「加载更多」逐页拉。
        //
        // 这里**不再有渲染窗口**（旧的 `page_limit`）：已加载的行全部进列表，
        // 只画视口内那几行是虚拟列表的事。
        let is_folder = matches!(&node.kind, NavNodeKind::Folder(_));
        let loaded = children.len();
        let total = if is_folder {
            self.node_total(&node.key, loaded)
        } else {
            loaded
        };
        // 定位窗口：先摆说明与回头路，再摆这一窗的行。
        let jumped_to = self.nav.borrow().jumped.get(&node.key).copied();
        if let Some(position) = jumped_to {
            rows.push(NavRow::Jump {
                node: Box::new(node.clone()),
                depth,
                scope_key: scope_key.to_string(),
                position,
            });
        }
        for child in &children {
            self.collect_node_rows(rows, child, depth + 1, scope_key, cx);
        }
        // 「加载更多」只剩一个意思：数据侧还有没取的。
        // **定位窗口例外**：那时的行集是一窗不是前缀，按条数往后翻会跳错地方，
        // 回头路走顶上那行「点此回到开头」。
        if is_folder && jumped_to.is_none() && loaded < total {
            rows.push(NavRow::More {
                node: Box::new(node.clone()),
                depth,
                scope_key: scope_key.to_string(),
            });
        }
    }

    /// 分组头：左侧统一色条 + 略深底 + **健康度**（已连/总）+ 计数 + 全折叠；右键菜单（重命名 / 新建 / 删除）。
    pub(super) fn render_group_header(
        &self,
        group_id: &str,
        name: &str,
        count: usize,
        connected_count: usize,
        failed_count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let danger = cx.theme().colors.danger;
        let bar = cx.theme().colors.primary;
        let bg = cx.theme().colors.sidebar_accent;
        let hover = cx.theme().colors.list_hover;
        let collapsed = self.group_collapsed(group_id);
        let entity = cx.entity();
        let gid = group_id.to_string();
        let gname = name.to_string();
        let is_ungrouped = group_id == GROUP_UNGROUPED;
        let health_text = format!("{connected_count}/{count}");
        let health_label = if failed_count > 0 {
            format!("已连 {health_text} · {failed_count} 失败")
        } else if connected_count < count {
            format!("已连 {health_text}")
        } else {
            count.to_string()
        };
        // 分组表单初值：新建用自动去重默认名，编辑预填现有名称与描述。
        let default_group_name = self.next_group_name();
        let group_seed = self.group_form_seed(group_id);
        // 上移 / 下移分组：边界项禁用（拖拽是主路径，菜单项是键盘 / 无鼠标的替代）。
        let group_ids_now = self.group_ids();
        let group_ix = group_ids_now.iter().position(|id| id == group_id);
        let can_up = group_ix.is_some_and(|i| i > 0);
        let can_down = group_ix.is_some_and(|i| i + 1 < group_ids_now.len());

        let mut header = div()
            .id(format!("nav-group-{group_id}"))
            .debug_selector(|| "nav-row-group".to_string())
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(ui::NAV_ROW_GROUP))
            .px_1()
            .gap_1()
            .rounded_md()
            .bg(bg)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click({
                let entity = entity.clone();
                let gid = gid.clone();
                move |_, _, app| {
                    let gid = gid.clone();
                    entity.update(app, |this, cx| {
                        {
                            let mut view = this.nav.borrow_mut();
                            if view.collapsed_groups.contains(&gid) {
                                view.collapsed_groups.remove(&gid);
                            } else {
                                view.collapsed_groups.insert(gid.clone());
                            }
                        }
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .w(ui::NAV_GROUP_BAR_WIDTH)
                    .h(rems(0.875))
                    .flex_none()
                    .rounded_full()
                    .bg(bar),
            )
            .child(nav_disclosure(true, !collapsed, muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_ellipsis()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child(name.to_string()),
            )
            // 聚合健康度与计数**合成一个元素**：`1/2` 的分母就是总数，再跟一个 `2`
            // 会被读成重复（原型 v9 修订点）。三档：全连 → 只报数（安静态）；
            // 有未连 → `已连 c/n`；有失败 → 追加 `· N 失败`（danger）。
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(if failed_count > 0 { danger } else { muted })
                    .child(health_label),
            )
            // 全折叠：一键折叠 / 展开全部同层分组（v5）。
            // 命中区按行高上限取 24×24（`w_6 h_6`）：旧写法 10×10 不仅难点中，
            // 而且 `⇅` 字形漏写 `text_xs`，以 16px 字号渲染进 10px 盒必然溢出。
            .child(
                div()
                    .id(format!("nav-group-foldall-{group_id}"))
                    .w_6()
                    .h_6()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(nav_icon(
                        "icons/chevrons-up-down.svg",
                        ui::ICON_SIZE_XS,
                        muted,
                    ))
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app: &mut App| {
                            entity.update(app, |this, cx| {
                                let ids: Vec<String> = this
                                    .nav
                                    .borrow()
                                    .groups
                                    .iter()
                                    .map(|g| g.id.clone())
                                    .collect();
                                {
                                    let mut view = this.nav.borrow_mut();
                                    let all_collapsed = !ids.is_empty()
                                        && ids.iter().all(|id| view.collapsed_groups.contains(id));
                                    if all_collapsed {
                                        view.collapsed_groups.clear();
                                    } else {
                                        for id in ids {
                                            view.collapsed_groups.insert(id);
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }
                    }),
            );

        // 分组之间可拖排序（「未分组」是伪分组，不可拖）。
        if !is_ungrouped {
            header = header.on_drag(
                NavGroupDragPayload {
                    group_id: gid.clone(),
                    name: gname.clone(),
                },
                {
                    // 幽灵：分组是容器，用文件夹形状（不给颜色身份，见 `nav_kind_icon`）。
                    let icon = "icons/folder.svg";
                    move |payload, _, _, cx| {
                        let label = payload.name.clone();
                        cx.new(|_| NavDragGhost { label, icon })
                    }
                },
            );
        }

        // 后续链接返回的是 `ContextMenu<..>`（不再与 `Stateful<Div>` 同型），故遮罩重绑。
        let header = header
            // 落点①（分组拖拽）：排到该分组之前；「未分组」头 = 排到最后。
            .drag_over::<NavGroupDragPayload>(|style, _, _, cx| {
                style.bg(cx.theme().colors.list_active)
            })
            .on_drop({
                let entity = entity.clone();
                let before = if is_ungrouped {
                    None
                } else {
                    Some(gid.clone())
                };
                move |payload: &NavGroupDragPayload, _, app| {
                    let before = before.clone();
                    entity.update(app, |this, cx| {
                        this.apply_group_drop(payload, before.as_deref(), cx)
                    });
                }
            })
            // 落点②（组归拖拽）：拖到分组头 = 归组；拖到「未分组」头 = 移出全部分组。
            .drag_over::<NavConnDragPayload>(|style, _, _, cx| {
                style.bg(cx.theme().colors.list_active)
            })
            .on_drop({
                let entity = entity.clone();
                let gid = gid.clone();
                move |payload: &NavConnDragPayload, _, app| {
                    let gid = gid.clone();
                    entity.update(app, |this, cx| {
                        this.apply_conn_drop(payload, &gid, ConnDropTarget::Container, cx)
                    });
                }
            })
            .context_menu({
                let entity = entity.clone();
                let host = self.host.clone();
                let gid = gid.clone();
                let gname = gname.clone();
                move |menu, _window, _cx| {
                    if is_ungrouped {
                        return menu.item(PopupMenuItem::new("新建分组").on_click({
                            let entity = entity.clone();
                            let host = host.clone();
                            let seed = GroupFormSeed::for_new(default_group_name.clone());
                            move |_, window, app| {
                                let e = entity.clone();
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
                        }));
                    }
                    let e_rename = entity.clone();
                    let gid_rename = gid.clone();
                    let e_new = entity.clone();
                    let seed_new = GroupFormSeed::for_new(default_group_name.clone());
                    let e_desc = entity.clone();
                    let seed_desc = group_seed.clone();
                    let e_del = entity.clone();
                    let gid_del = gid.clone();
                    let gname_del = gname.clone();
                    menu.item(PopupMenuItem::new("重命名分组").on_click(move |_, _, app| {
                        let gid = gid_rename.clone();
                        e_rename.update(app, |this, cx| {
                            this.nav.borrow_mut().group_rename_for = Some(gid.clone());
                            cx.notify();
                        });
                    }))
                    // 名称 + 描述一次编辑（行内重命名只改名称，这里补上描述）。
                    .item(PopupMenuItem::new("编辑分组…").on_click({
                        let host = host.clone();
                        move |_, window, app| {
                            let e = e_desc.clone();
                            let seed = seed_desc.clone();
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
                    }))
                    .item(PopupMenuItem::new("新建分组").on_click({
                        let host = host.clone();
                        move |_, window, app| {
                            let e = e_new.clone();
                            let seed = seed_new.clone();
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
                    }))
                    .separator()
                    // 分组排序：拖拽是主路径，这两项是键盘 / 无鼠标时的替代（边界置灰）。
                    .item(PopupMenuItem::new("上移分组").disabled(!can_up).on_click({
                        let e = entity.clone();
                        let gid = gid.clone();
                        move |_, _, app| {
                            let gid = gid.clone();
                            e.update(app, |this, cx| this.step_group(&gid, -1, cx));
                        }
                    }))
                    .item(
                        PopupMenuItem::new("下移分组")
                            .disabled(!can_down)
                            .on_click({
                                let e = entity.clone();
                                let gid = gid.clone();
                                move |_, _, app| {
                                    let gid = gid.clone();
                                    e.update(app, |this, cx| this.step_group(&gid, 1, cx));
                                }
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("删除分组").on_click(move |_, window, app| {
                            let entity = e_del.clone();
                            let gid = gid_del.clone();
                            let gname = gname_del.clone();
                            window.open_alert_dialog(app, move |alert, _window, _cx| {
                                let entity = entity.clone();
                                let gid = gid.clone();
                                alert
                                    .confirm()
                                    .title("删除分组")
                                    .description(format!(
                                        "确定删除分组「{gname}」？成员连接与缓存不会被删除。"
                                    ))
                                    .button_props(
                                        DialogButtonProps::default()
                                            .ok_text("删除")
                                            .ok_variant(ButtonVariant::Danger)
                                            .show_cancel(true),
                                    )
                                    .on_ok(move |_, _window, app| {
                                        entity.update(app, |this, cx| this.delete_group(&gid, cx));
                                        true
                                    })
                            });
                        }),
                    )
                }
            });

        let mut wrap = div().v_flex().w_full().gap_0p5();
        wrap = wrap.child(header);
        // 重命名内联输入（打开时创建，见 `render_nav`）。
        if self.nav.borrow().group_rename_for.as_deref() == Some(group_id) {
            if let Some(input) = &self.nav_group_input {
                wrap = wrap.child(div().px_1().pb_0p5().child(Input::new(input)));
            }
        }
        wrap
    }

    /// 引用行（v6）：连接已在其**主组**全亮呈现，此分组下只做指路。
    ///
    /// 不重复状态 / 徽标 / 操作位（避免重复向）；点击展开主组并选中该连接。
    /// `group_id` 为**当前包含它的分组**（用于唯一元素 ID），`primary_name` 为主组名。
    pub(super) fn render_reference_row(
        &self,
        conn: &ConnectionItem,
        group_id: &str,
        primary_name: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let selected_bg = cx.theme().colors.list_active;
        let active_border = cx.theme().colors.list_active_border;
        let entity = cx.entity();
        let conn_id = conn.id.clone();
        let selected = self.nav_row_selected(&conn.id);
        let drag_payload = NavConnDragPayload {
            conn_id: conn.id.clone(),
            name: conn.name.clone(),
        };
        let drop_scope = group_id.to_string();
        let drop_before = conn.id.clone();
        let primary_gid = self
            .nav
            .borrow()
            .membership
            .get(&conn.id)
            .and_then(|gs| gs.first().cloned());
        let label = format!("\u{2208} {primary_name}");
        div()
            .id(format!("nav-ref-{}::{}", group_id, conn.id))
            .debug_selector(|| "nav-row-ref".to_string())
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(ui::NAV_ROW_CONNECTION))
            .px_1()
            .gap_1()
            .rounded_md()
            .relative()
            .cursor_pointer()
            .when(selected, |s| {
                s.bg(selected_bg).child(tree::active_bar(active_border))
            })
            .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
            .child(div().w_2p5().flex_none())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(conn.name.clone()),
            )
            // `∈ 主组名`：长组名可缩可截，否则 `flex_none` 会把名称挤没（面板最宽 240px）。
            .child(
                div()
                    .flex_shrink_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(label),
            )
            // 引用行同样可拖（否则“全组成员都是引用行”的分组无法重排）
            // 与可落：落到引用行 = 加入该分组并插到它之前。
            .on_drag(drag_payload, move |payload, _, _, cx| {
                let label = payload.name.clone();
                cx.new(|_| NavDragGhost {
                    label,
                    icon: "icons/database.svg",
                })
            })
            .drag_over::<NavConnDragPayload>(|style, _, _, cx| {
                style.bg(cx.theme().colors.list_active)
            })
            .on_drop({
                let entity = entity.clone();
                let scope = drop_scope.clone();
                let before = drop_before.clone();
                move |payload: &NavConnDragPayload, _, app| {
                    let scope = scope.clone();
                    let before = before.clone();
                    entity.update(app, |this, cx| {
                        this.apply_conn_drop(payload, &scope, ConnDropTarget::BeforeRow(before), cx)
                    });
                }
            })
            .on_click(move |_, _, app| {
                let cid = conn_id.clone();
                entity.update(app, |this, cx| {
                    {
                        let mut view = this.nav.borrow_mut();
                        // 展开主组（若已折叠）并选中该连接，使全亮行可见。
                        if let Some(gid) = &primary_gid {
                            view.collapsed_groups.remove(gid);
                        }
                        view.selected_key = Some(cid.clone());
                    }
                    cx.notify();
                });
            })
    }

    /// 连接节点行（徽标（色=状态·形=类型） + 名称 + 归属域列 + 行尾操作）。
    ///
    /// `scope_key` 为其所属分组标识：同一连接可出现在多个分组，用于生成唯一元素 ID。
    pub(super) fn render_connection_row(
        &self,
        conn: &ConnectionItem,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let danger = cx.theme().colors.danger;

        let expanded = {
            let view = self.nav.borrow();
            view.expanded.contains(&conn.id)
        };
        let (connected, error, children) = {
            let view = self.nav.borrow();
            (
                view.connected.contains(&conn.id) || conn.connected,
                view.errors.get(&conn.id).cloned(),
                view.children.get(&conn.id).cloned().unwrap_or_default(),
            )
        };
        // 展开未加载时的「排一次后台加载」不在这里：它已随顺序一起搬到 `collect_nav_rows`
        //（渲染只画一行，副作用不能藏在「这一行恰好被画到」里）。

        let source = NavSource::from_conn_id(&conn.id);
        // 来源标识：短码 `P/G/GP` 或文字（设置项，默认短码）。
        let source_text = if self.host.source_short_code(cx) {
            source.code().to_string()
        } else {
            source.label().to_string()
        };
        let filter = self.nav.borrow().filter.to_lowercase();
        let match_bg = product_tokens::get(cx).search_match_background(cx.theme());
        let project_root = self
            .host
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        let conn_id = conn.id.clone();
        let scope_key = scope_key.to_string();
        let selected = self.nav_row_selected(&conn.id);
        let selected_bg = cx.theme().colors.list_active;
        let active_border = cx.theme().colors.list_active_border;

        // ---- v7：行内只常驻「徽标 + 名称 + 归属域列」；`+` 与行操作仅 hover / 选中显 ----
        let nav_view = self.driver_catalog.borrow().get(&conn.driver).map(|m| {
            (
                m.type_id.clone(),
                m.name.clone(),
                m.type_name.clone(),
                m.type_category.clone(),
            )
        });
        let type_id = nav_view
            .as_ref()
            .map(|(t, ..)| t.clone())
            .unwrap_or_else(|| conn.driver.clone());
        // 类型显示名：目录优先（`data_source_types.name` + 分类）——与连接对话框同源
        let type_from_catalog: Option<(String, String)> = nav_view
            .as_ref()
            .and_then(|(_, _, name, cat)| Some((name.clone()?, cat.clone()?)));
        let driver_name = nav_view.map(|(_, n, ..)| n);
        let nav_view = self.nav.borrow();
        let badge_status = if nav_view.loading.contains(&conn.id) {
            NavBadgeStatus::Connecting
        } else if error.is_some() {
            NavBadgeStatus::Failed
        } else if connected {
            NavBadgeStatus::Connected
        } else {
            NavBadgeStatus::Idle
        };
        let tag_list: Vec<String> = nav_view.tags.get(&conn.id).cloned().unwrap_or_default();
        drop(nav_view);

        let (badge_path, badge_letters) = nav_type_badge(&type_id);
        let badge_color = badge_status.color(cx.theme());
        // 双通道徽标：颜色 = 状态（能不能用），形状 = 类型（是什么库，内叠 2 字母）。
        let badge = div()
            .relative()
            .w(rems(ui::NAV_BADGE_SIZE))
            .h(rems(ui::NAV_BADGE_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::empty()
                            .path(badge_path)
                            .size(rems(ui::NAV_BADGE_SIZE))
                            .text_color(badge_color),
                    ),
            )
            .child(
                div()
                    .relative()
                    .text_size(rems(0.4))
                    .font_weight(FontWeight::BOLD)
                    .text_color(badge_color)
                    .child(badge_letters),
            );

        // 「连接中」才呼吸（原型设计 §2.3：`info` + 脉冲）。
        //
        // 为何只在**这一种**状态上挂：循环动画会让窗口每帧重绘，而建连是暂态——
        // 其余状态（已连 / 未连 / 失败）是普通元素，零开销。
        // 为何不是装饰：「在连」这个事实另有静态表达（`info` 色 + hover 卡写「连接中」），
        // 动效只是把「还在动」说清楚（上游动效规范：不得让动画成为唯一的状态表达）。
        let badge: AnyElement = if nav_badge_pulses(badge_status) {
            badge
                .with_animation(
                    // id 必须**逐行唯一**：动画起始时刻存在按全局 id 索引的元素状态里，
                    // 同帧两个徽标共用一个 id 会互相抢状态（相位抖动），
                    // 与行内既有元素 id 同一口径（`{分组}::{连接}`）。
                    SharedString::from(format!("nav-badge-pulse-{scope_key}::{}", conn.id)),
                    // 缓动必须**首尾同值**：`bounce` 把 0→1→0 折回来，循环接缝处不会“啪”地跳一下，
                    // 且 delta = 0 时得 1.0——`reduce_motion` 下动画停下时徽标正好是全不透明
                    // （用 `pulsating_between` 会停在中值上，稳态看起来像被蒙了一层）。
                    Animation::new(std::time::Duration::from_millis(ui::NAV_BADGE_PULSE_MS))
                        .repeat()
                        .with_easing(bounce(ease_in_out)),
                    // 只能改透明度：gpui 的 Style 没有 transform，`Transformation` 仅服务 SVG。
                    |badge, delta| badge.opacity(1.0 - delta * 0.5),
                )
                .into_any_element()
        } else {
            badge.into_any_element()
        };

        // 徽标 hover 卡：类型 / 状态 / 驱动的完整事实（gpui-kit 0.6.1 无通用 `.tooltip` 扩展，
        // 故用 `HoverCard` 承载；行内仍只显颜色 + 形状）。
        let badge = {
            let type_label = nav_type_label(
                &type_id,
                type_from_catalog
                    .as_ref()
                    .map(|(name, cat)| (name.as_str(), cat.as_str())),
            );
            let status_label = badge_status.label();
            let driver_label = driver_name
                .clone()
                .map(|n| format!("{n} · {}", conn.driver))
                .unwrap_or_else(|| conn.driver.clone());
            let hover_id = SharedString::from(format!("nav-badge-{}::{}", scope_key, conn.id));
            nav_badge_hover_card(hover_id, badge, type_label, status_label, driver_label)
        };

        // 标签 chip（可选显示，`⋯ → 显示标签`）：默认关；开启后「≤2 chip + `+N`」。
        let tag_chips = if self.host.show_tags(cx) && !tag_list.is_empty() {
            let chip_bg = cx.theme().colors.list_hover;
            // 标签字号用最初版小字 `text_xs`（与行内文字同尺寸，不因换行而变大）。
            let mut chips = div()
                .h_flex()
                .items_center()
                .gap_0p5()
                .flex_none()
                .text_xs();
            for t in tag_list.iter().take(2) {
                chips = chips.child(
                    div()
                        .px_1()
                        .rounded_sm()
                        .bg(chip_bg)
                        .text_color(muted)
                        .child(t.clone()),
                );
            }
            if tag_list.len() > 2 {
                chips = chips.child(
                    div()
                        .text_color(muted)
                        .child(format!("+{}", tag_list.len() - 2)),
                );
            }
            Some(chips)
        } else {
            None
        };

        // 归属域短码：右对齐固定列（可在 `⋯ → 显示归属域` 关闭）。
        let scope_visible = self.host.show_scope(cx);
        let scope_col = if self.host.source_short_code(cx) {
            ui::NAV_SCOPE_COL_SHORT
        } else {
            ui::NAV_SCOPE_COL_TEXT
        };
        let scope_color = match source {
            NavSource::Project => cx.theme().colors.info,
            NavSource::Global => muted,
            NavSource::Shared => cx.theme().colors.primary,
        };

        // 行尾操作：`+` 加标签 · `✎` 编辑；仅 hover / 选中显（连接 / 断开走右键菜单）。
        //
        // 隐藏用 `invisible()` 而不是 `opacity(0)`：两者的区别在命中测试——
        // `Visibility::Hidden` 的元素既不绘制也不登记 hitbox，`opacity(0)` 的元素
        // **仍能点到**（鼠标滑到行尾空白就可能误触发加标签 / 打开编辑对话框）。
        // 槽位宽度两种写法一样常驻，所以 hover 时不会左右跳动。
        let ops = {
            let entity = cx.entity();
            let host = self.host.clone();
            let cid = conn_id.clone();
            let hover_bg = cx.theme().colors.list_hover;
            let mut ops = div()
                .h_flex()
                .items_center()
                .gap_0p5()
                .flex_none()
                .when(!selected, |s| s.invisible())
                .group_hover("nav-conn-row", |s| s.visible());
            // `+`：仅**标签**行内编辑（`+` 只处理标签；归组走右键「移动到分组…」）。
            ops = ops.child(
                div()
                    .id(format!("nav-conn-addtag-{}::{}", scope_key, conn.id))
                    .w(rems(ui::NAV_ROW_ACTION_SIZE))
                    .h(rems(ui::NAV_ROW_ACTION_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.bg(hover_bg))
                    .child(nav_icon("icons/plus.svg", ui::ICON_SIZE_XS, muted))
                    .on_click({
                        let entity = entity.clone();
                        let cid = cid.clone();
                        move |_, _, app: &mut App| {
                            let cid = cid.clone();
                            entity.update(app, |this, cx| {
                                let mut view = this.nav.borrow_mut();
                                view.tag_editor_for =
                                    if view.tag_editor_for.as_deref() == Some(cid.as_str()) {
                                        None
                                    } else {
                                        Some(cid.clone())
                                    };
                                cx.notify();
                            });
                        }
                    }),
            );
            // `✎`：在对话框中编辑连接。
            ops = ops.child(
                div()
                    .id(format!("nav-conn-edit-{}::{}", scope_key, conn.id))
                    .w(rems(ui::NAV_ROW_ACTION_SIZE))
                    .h(rems(ui::NAV_ROW_ACTION_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.bg(hover_bg))
                    .child(nav_icon("icons/pencil.svg", ui::ICON_SIZE_XS, muted))
                    .on_click({
                        let host = host.clone();
                        let cid = cid.clone();
                        move |_, window, app: &mut App| {
                            host.edit_connection(&cid, window, app);
                        }
                    }),
            );
            // 连接 / 断开不进 hover 行操作：仅右键菜单提供（避免误触与视觉噪声）。
            ops
        };

        // 徽标 tooltip 材料（接入 `Tooltip` 组件前，事实统一在属性面板展示）。
        let _ = (&driver_name, danger);

        // 「设为主组」子菜单数据（仅归组的连接出现）：所属分组 + 当前主组 + 是否显式。
        let (menu_groups, menu_primary, menu_primary_explicit) = {
            let view = self.nav.borrow();
            let gids = view.membership.get(&conn.id).cloned().unwrap_or_default();
            let explicit = view
                .primary_group
                .get(&conn.id)
                .filter(|p| gids.iter().any(|g| g == *p))
                .cloned();
            let primary = explicit.clone().or_else(|| gids.first().cloned());
            let names: Vec<(String, String)> = gids
                .iter()
                .map(|gid| {
                    let name = view
                        .groups
                        .iter()
                        .find(|g| &g.id == gid)
                        .map(|g| g.name.clone())
                        .unwrap_or_else(|| gid.clone());
                    (gid.clone(), name)
                })
                .collect();
            (names, primary, explicit.is_some())
        };

        let mut block = div().v_flex().w_full();
        block = block.child(
            div()
                .id(format!("nav-conn-{}::{}", scope_key, conn.id))
                .debug_selector(|| "nav-row-conn".to_string())
                .h_flex()
                .items_center()
                .w_full()
                .h(rems(ui::NAV_ROW_CONNECTION))
                .px_1()
                .gap_1()
                .rounded_md()
                .relative()
                .cursor_pointer()
                // 悬停**不覆盖**选中：否则鼠标一停在选中行上，选中态就消失。
                .when(selected, |s| {
                    s.bg(selected_bg).child(tree::active_bar(active_border))
                })
                .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
                .on_click({
                    let entity = cx.entity();
                    let conn_id = conn_id.clone();
                    let conn_name = conn.name.clone();
                    let conn_driver = conn.driver.clone();
                    let focus = self.focus_handle.clone();
                    move |ev, window, app| {
                        focus.focus(window, app);
                        // 单击选中（键盘导航基准）；双击打开属性；再次点击展开 / 折叠。
                        let sel_key = conn_id.clone();
                        entity.update(app, |this, cx| {
                            this.nav.borrow_mut().selected_key = Some(sel_key.clone());
                            cx.notify();
                        });
                        if ev.click_count() >= 2 {
                            let req = PropertyRequest {
                                property: PropertyRef {
                                    conn_id: conn_id.clone(),
                                    source,
                                    catalog: None,
                                    schema: None,
                                    parent: None,
                                    name: conn_name.clone(),
                                    kind: PropertyKind::Connection,
                                },
                                conn_label: conn_name.clone(),
                                driver: conn_driver.clone(),
                            };
                            entity.update(app, |this, cx| {
                                this.host.show_properties(req, cx);
                                cx.notify();
                            });
                            return;
                        }
                        let conn_id = conn_id.clone();
                        entity.update(app, |this, cx| {
                            this.toggle_nav_node(&conn_id, &conn_id, NavPath::Connection, cx);
                            cx.notify();
                        });
                    }
                })
                .child(nav_disclosure(true, expanded, muted))
                .group("nav-conn-row")
                .child(badge)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(fg)
                        .text_ellipsis()
                        .child(nav_name_highlight(&conn.name, &filter, match_bg, fg)),
                )
                .when(scope_visible, |s| {
                    // 列本身包在 hover 卡里（§2.3）：短码不是自解释的，hover 给一句人话。
                    let column = div()
                        .w(rems(scope_col))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .text_xs()
                        .text_color(scope_color)
                        .child(source_text.clone());
                    s.child(nav_scope_hover_card(
                        SharedString::from(format!("nav-scope-{}::{}", scope_key, conn.id)),
                        column,
                        nav_scope_tooltip(source),
                    ))
                })
                // 行尾操作组（`+` 加标签 / `✎` 编辑）：仅 hover / 选中显。
                .child(ops)
                // 拖拽（归组 + 组内排序）：拖起本行；落点 = 行（插到该行之前）/ 分组头（见 `render_group_header`）。
                .on_drag(
                    NavConnDragPayload {
                        conn_id: conn.id.clone(),
                        name: conn.name.clone(),
                    },
                    {
                        let icon = "icons/database.svg";
                        move |payload, _, _, cx| {
                            let label = payload.name.clone();
                            cx.new(|_| NavDragGhost { label, icon })
                        }
                    },
                )
                .drag_over::<NavConnDragPayload>(|style, _, _, cx| {
                    style.bg(cx.theme().colors.list_active)
                })
                .on_drop({
                    let entity = cx.entity();
                    let scope = scope_key.clone();
                    let before = conn.id.clone();
                    move |payload: &NavConnDragPayload, _, app| {
                        let scope = scope.clone();
                        let before = before.clone();
                        entity.update(app, |this, cx| {
                            this.apply_conn_drop(
                                payload,
                                &scope,
                                ConnDropTarget::BeforeRow(before),
                                cx,
                            )
                        });
                    }
                })
                // 右键菜单（连接节点）：连接/断开、编辑、查看属性、分组/标签、复制、刷新。
                .context_menu({
                    let entity = cx.entity();
                    let host = self.host.clone();
                    let conn_id = conn.id.clone();
                    let conn_name = conn.name.clone();
                    let conn_driver = conn.driver.clone();
                    let root = project_root.clone();
                    let prop = PropertyRef {
                        conn_id: conn_id.clone(),
                        source,
                        catalog: None,
                        schema: None,
                        parent: None,
                        name: conn_name.clone(),
                        kind: PropertyKind::Connection,
                    };
                    let is_connected = connected;
                    move |menu, window, cx| {
                        let e_connect = entity.clone();
                        let cid_connect = conn_id.clone();
                        let root_connect = root.clone();
                        let e_edit = entity.clone();
                        let cid_edit = conn_id.clone();
                        let e_prop = entity.clone();
                        let prop_own = prop.clone();
                        let label_prop = conn_name.clone();
                        let drv_prop = conn_driver.clone();
                        let e_org = entity.clone();
                        let cid_org = conn_id.clone();
                        let e_copy = entity.clone();
                        let host_copy = host.clone();
                        let name_copy = conn_name.clone();
                        let e_refresh = entity.clone();
                        let cid_refresh = conn_id.clone();
                        let name_refresh = conn_name.clone();
                        let mut menu = menu
                            .item(
                                PopupMenuItem::new(if is_connected { "断开" } else { "连接" })
                                    .on_click(move |_, _, app| {
                                        let cid = cid_connect.clone();
                                        let root = root_connect.clone();
                                        e_connect.update(app, |this, cx| {
                                            this.toggle_connection(&cid, root.as_deref(), cx)
                                        });
                                    }),
                            )
                            .item(PopupMenuItem::new("测试连接").on_click({
                                let e = entity.clone();
                                let cid = conn_id.clone();
                                let root = root.clone();
                                let name = conn_name.clone();
                                // 探测入口是函数指针；句柄在这里克隆一份，避免把 `self` 借进 'static 闭包。
                                let host = host.clone();
                                move |_, _, app| {
                                    // 独立会话探测（不注册连接池 / 不写库）；结果落面板提示。
                                    // 探测入口是函数指针（可跨到工作线程）；视图搬入 `database`
                                    // 后改由宿主端口供给（`NavHost::connection_probe`）。
                                    nav_jobs::enqueue_test_connection(
                                        &cid,
                                        root.as_deref(),
                                        &name,
                                        host.connection_probe(),
                                    );
                                    e.update(app, |this, cx| this.ensure_nav_pump(cx));
                                }
                            }))
                            .item(PopupMenuItem::new("编辑连接…").on_click(move |_, window, app| {
                                let cid = cid_edit.clone();
                                e_edit.update(app, |this, cx| {
                                    // 命令端口：直接开对话框（不再置位请求字段等渲染消费）。
                                    this.host.edit_connection(&cid, window, cx);
                                });
                            }))
                            .separator()
                            .item(PopupMenuItem::new("查看属性").on_click(move |_, _, app| {
                                let prop = prop_own.clone();
                                let label = label_prop.clone();
                                let drv = drv_prop.clone();
                                e_prop.update(app, |this, cx| {
                                    this.host.show_properties(
                                        PropertyRequest {
                                            property: prop.clone(),
                                            conn_label: label.clone(),
                                            driver: drv.clone(),
                                        },
                                        cx,
                                    );
                                    cx.notify();
                                });
                            }))
                            .item(
                                PopupMenuItem::new("移动到分组…").on_click(move |_, _, app| {
                                    let cid = cid_org.clone();
                                    e_org.update(app, |this, cx| {
                                        this.nav.borrow_mut().group_picker_for =
                                            Some(cid.clone());
                                        cx.notify();
                                    });
                                }),
                            );
                        // 「设为主组 ▸」：仅在该连接已归组时出现（单选 + 「自动」回退）。
                        if !menu_groups.is_empty() {
                            let e_p = entity.clone();
                            let root_p = root.clone();
                            let cid_p = conn_id.clone();
                            let groups_for = menu_groups.clone();
                            let cur = menu_primary.clone();
                            let explicit = menu_primary_explicit;
                            menu = menu.submenu("设为主组", window, cx, move |m, _w, _c| {
                                let mut m = m.item(
                                    PopupMenuItem::new("自动（按分组排序）")
                                        .checked(!explicit)
                                        .on_click({
                                            let e = e_p.clone();
                                            let root = root_p.clone();
                                            let cid = cid_p.clone();
                                            move |_, _, app| {
                                                let root = root.clone();
                                                let cid = cid.clone();
                                                let root = root.as_deref().map(std::path::Path::new);
                                                e.update(app, |this, cx| {
                                                    let _ = crate::nav_store::clear_primary_group(
                                                        root,
                                                        &cid,
                                                    );
                                                    this.reload_nav_org();
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                );
                                for (gid, gname) in groups_for.clone() {
                                    let checked = cur.as_deref() == Some(gid.as_str());
                                    let e = e_p.clone();
                                    let root = root_p.clone();
                                    let cid = cid_p.clone();
                                    m = m.item(
                                        PopupMenuItem::new(gname).checked(checked).on_click(
                                            move |_, _, app| {
                                                let gid = gid.clone();
                                                let root = root.clone();
                                                let cid = cid.clone();
                                                let root = root.as_deref().map(std::path::Path::new);
                                                e.update(app, |this, cx| {
                                                    let _ =
                                                        crate::nav_store::set_primary_group(
                                                            root,
                                                            &cid,
                                                            &gid,
                                                        );
                                                    this.reload_nav_org();
                                                    cx.notify();
                                                });
                                            },
                                        ),
                                    );
                                }
                                m
                            });
                        }
                        // 复制（模板）/ 共享 / 删除：连接自身的管理动作。
                        // 共享快照（GP_）本身即副本，不提供复制。
                        if source != NavSource::Shared {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            menu = menu.separator().item(
                                PopupMenuItem::new("复制连接（模板）…").on_click(
                                    move |_, _, app| {
                                        let cid = cid.clone();
                                        e.update(app, |this, cx| {
                                            this.nav.borrow_mut().copy_for =
                                                Some(cid.clone());
                                            cx.notify();
                                        });
                                    },
                                ),
                            );
                        }
                        if source == NavSource::Global {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            let name = conn_name.clone();
                            menu = menu.item(
                                PopupMenuItem::new("共享至项目").on_click(move |_, _, app| {
                                    let cid = cid.clone();
                                    let name = name.clone();
                                    e.update(app, |this, cx| {
                                        this.share_connection_to_project(&cid, &name, cx)
                                    });
                                }),
                            );
                        }
                        if source == NavSource::Shared {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            let name = conn_name.clone();
                            menu = menu.item(
                                PopupMenuItem::new("取消共享").on_click(move |_, _, app| {
                                    let cid = cid.clone();
                                    let name = name.clone();
                                    e.update(app, |this, cx| {
                                        this.unshare_connection_from_project(&cid, &name, cx)
                                    });
                                }),
                            );
                        }
                        {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            let name = conn_name.clone();
                            menu = menu.separator().item(
                                PopupMenuItem::new("删除连接").on_click(move |_, window, app| {
                                    let e = e.clone();
                                    let cid = cid.clone();
                                    let name = name.clone();
                                    window.open_alert_dialog(app, move |alert, _window, _cx| {
                                        let e = e.clone();
                                        let cid = cid.clone();
                                        // 内层 `on_ok` 是 move 闭包：先在外层建一份新绑定，
                                        // 否则会把外层闭包环境里的 `name` 移出（E0507）。
                                        let name_for_ok = name.clone();
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
                                            .on_ok(move |_, _window, app| {
                                                let cid = cid.clone();
                                                let name = name_for_ok.clone();
                                                e.update(app, |this, cx| {
                                                    this.delete_connection(&cid, &name, cx)
                                                });
                                                true
                                            })
                                    });
                                }),
                            );
                        }
                        menu.item(PopupMenuItem::new("复制名称").on_click(move |_, _, app| {
                            let name = name_copy.clone();
                            app.write_to_clipboard(ClipboardItem::new_string(name.clone()));
                            host_copy.notice(format!("已复制：{name}"), app);
                            e_copy.update(app, |_, cx| cx.notify());
                        }))
                        .item(PopupMenuItem::new("刷新元数据").on_click(move |_, _, app| {
                            let cid = cid_refresh.clone();
                            let name = name_refresh.clone();
                            e_refresh.update(app, |this, cx| {
                                this.refresh_node(&cid, &cid, Some(NavPath::Connection), cx);
                                this.host.notice(format!("已刷新：{name}"), cx);
                            });
                        }))
                        // 通用模块入口（与连接状态无关）：SQL 编辑器 / 洞察。
                        // Mock 只针对表 / 视图，见 `render_nav_node` 的对象菜单。
                        .separator()
                        .item(PopupMenuItem::new("在 SQL 编辑器中打开").on_click({
                            // `host` 是上层块里的局部（`self` 不能进 'static 闭包）
                            let host = host.clone();
                            let cid = conn_id.clone();
                            move |_, _, app| {
                                // B11：只入队「打开查询」请求，开文档在宿主 render 里做
                                // （菜单回调拿不到 `Window`；绑定该连接 + 聚焦都由宿主完成）
                                host.open_query(QueryRequest {
                                    conn_id: Some(cid.clone()),
                                    sql: String::new(),
                                    run: false,
                                }, app);
                                                            }
                        }))
                        .item(PopupMenuItem::new("查看洞察").on_click({
                            let e = entity.clone();
                            move |_, _, app| {
                                e.update(app, |this, cx| {
this.host.open_right_panel(RightPanel::Insight, cx);
            });
                            }
                        }))
                    }
                }),
        );

        // 后台加载占位（避免展开后空白，误导为已加载完）。
        if self.nav.borrow().loading.contains(&conn.id) {
            block = block.child(nav_subline("加载中…", muted));
        }

        // 标签（v7 修订）：显示在连接名**下一行**（不在名称行内），仅 `⋯ → 显示标签`
        // 开启时；以「≤2 chip + `+N`」呈现，避免撑爆名称行 / 挤掉归属域列。
        if let Some(chips) = tag_chips {
            block = block.child(
                div()
                    .h_flex()
                    .items_center()
                    .h(rems(ui::NAV_SUBLINE))
                    .w_full()
                    .min_w_0()
                    .pl_6()
                    .overflow_hidden()
                    .child(chips),
            );
        }

        // 行内编辑器：归组（右键「移动到分组…」）与标签（行尾 `+`）分开，各司其职。
        // 块高在各自的渲染里钉成常量（见 `ui::NAV_EDITOR_*`）：虚拟列表按估算高度
        // 定义行槽位，内容高过槽位会压到下一行，所以不能是「内容说多少就多少」。
        let picker_open = self.nav.borrow().group_picker_for.as_deref() == Some(conn.id.as_str());
        if picker_open {
            block = block.child(self.render_group_editor(conn, &scope_key, cx));
        }
        let tag_open = self.nav.borrow().tag_editor_for.as_deref() == Some(conn.id.as_str());
        if tag_open {
            block = block.child(self.render_tag_editor(cx));
        }
        let copy_open = self.nav.borrow().copy_for.as_deref() == Some(conn.id.as_str());
        if copy_open {
            block = block.child(self.render_copy_editor(cx));
        }

        // 展开但未连接（如上次会话遗留的展开态）：不报错，给明下一步指引。
        let loading_here = self.nav.borrow().loading.contains(&conn.id);
        if expanded && !connected && children.is_empty() && error.is_none() && !loading_here {
            block = block.child(nav_subline("未连接 · 右键「连接」或再次展开", muted));
        }
        if let Some(err) = error {
            block = block.child(nav_subline(&err, danger));
        }

        block
    }

    /// 对象树节点行（懒加载；叶子不可展开）。
    ///
    /// `scope_key` 为所属分组标识：同一连接可出现在多个分组，用于生成唯一元素 ID。
    pub(super) fn render_nav_node(
        &self,
        node: &NavNode,
        depth: usize,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let pri = cx.theme().colors.primary;
        let danger = cx.theme().colors.danger;
        let active_border = cx.theme().colors.list_active_border;

        let filter = self.nav.borrow().filter.to_lowercase();
        // 展开态按**过滤生效后**的那份看：带筛选时所有行都当展开（否则命中的行藏在关着的支里）。
        // 「要不要加载」「这一行可不可见」不在这里——它们归 `collect_nav_rows`。
        let (expanded, error) = {
            let view = self.nav.borrow();
            (
                view.expanded.contains(&node.key) || !filter.is_empty(),
                view.errors.get(&node.key).cloned(),
            )
        };
        let kind_icon = nav_kind_icon(&node.kind, expanded);
        // 图标颜色只承载状态：正常情况下中性色（类别靠形状，见 `nav_kind_icon`），
        // 加载失败才转 `danger`——与连接行「徽标色 = 状态」是同一套语言。
        let icon_color = if error.is_some() { danger } else { muted };

        let entity = cx.entity();
        let n_conn_id = node.connection_id.clone();
        let n_key = node.key.clone();
        let n_path = node.expand_path.clone();
        let n_property = node.property.clone();
        let n_conn_label = self
            .host
            .connections()
            .iter()
            .find(|c| c.id == node.connection_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| node.connection_id.clone());
        let n_driver = self
            .host
            .connections()
            .iter()
            .find(|c| c.id == node.connection_id)
            .map(|c| c.driver.clone())
            .unwrap_or_default();
        // 供右键菜单使用（双击处理器已消费 `n_conn_label` / `n_driver`）。
        let m_conn_label = n_conn_label.clone();
        let m_driver = n_driver.clone();

        let right_meta: Option<(String, bool)> = match &node.kind {
            NavNodeKind::Column {
                data_type,
                primary,
                foreign,
                ..
            } => {
                let mut t = data_type.clone();
                if *primary {
                    t.push_str("  PK");
                }
                if *foreign {
                    t.push_str("  FK");
                }
                Some((t, *primary))
            }
            _ => None,
        };

        // 拖拽载荷：仅表 / 视图（原型 §6.1「拖拽表到编辑器」）。
        let drag_payload = match &node.kind {
            NavNodeKind::Table { .. } | NavNodeKind::View => {
                node.property.as_ref().map(|p| NavDragPayload {
                    qualified: nav_qualified_name(p),
                    label: node.name.clone(),
                })
            }
            _ => None,
        };

        // 缩进 = 基础内边距 + 层级 × 步长（设计 §2.1）。两者都是 rem 倍率，
        // 直接交给 `rems()` 换算（其基准是主题字号而非 4px，写 `/ 4.` 会放大 4 倍）。
        let indent = ui::TREE_BASE_PADDING + depth as f32 * ui::TREE_INDENT;
        let selected = self.nav_row_selected(&node.key);
        let selected_bg = cx.theme().colors.list_active;
        let mut block = div().v_flex().w_full();
        let mut row = div()
            .id(format!("nav-node-{}::{}", scope_key, node.key))
            .debug_selector(|| "nav-row-tree".to_string())
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(ui::NAV_ROW_TREE))
            .pr_1()
            .pl(rems(indent))
            .gap_1()
            .rounded_md()
            .relative()
            .cursor_pointer()
            // 悬停**不覆盖**选中：否则鼠标一停在选中行上，选中态就消失（失去“我在哪”的锚点）。
            .when(selected, |s| {
                s.bg(selected_bg).child(tree::active_bar(active_border))
            })
            .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
            .child(nav_disclosure(node.has_children, expanded, muted))
            .child(nav_icon(kind_icon, ui::ICON_SIZE_SM, icon_color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(fg)
                    .text_ellipsis()
                    // 行高由虚拟列表钉死（`NAV_ROW_TREE`），长名必须截断而不是换行。
                    .child(nav_name_highlight(
                        &node.name,
                        &filter,
                        product_tokens::get(cx).search_match_background(cx.theme()),
                        fg,
                    )),
            );
        if let Some((meta, is_pk)) = right_meta {
            row = row.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(if is_pk { pri } else { muted })
                    .child(meta),
            );
        }
        if let Some(payload) = drag_payload {
            // 幽灵用同一个类别图标（`&'static str` 是 `Copy`，可直接进闭包）。
            let ghost_icon = kind_icon;
            row = row.on_drag(payload, move |payload, _offset, _window, cx| {
                // 幽灵显示短名（与用户抓住的东西一致），插入的是限定名。
                let label = payload.label.clone();
                cx.new(|_| NavDragGhost {
                    label,
                    icon: ghost_icon,
                })
            });
        }
        row = row.on_click({
            let focus = self.focus_handle.clone();
            move |ev, window, app| {
                focus.focus(window, app);
                let sel_key = n_key.clone();
                entity.update(app, |this, cx| {
                    this.nav.borrow_mut().selected_key = Some(sel_key.clone());
                    cx.notify();
                });
                if ev.click_count() >= 2 {
                    if let Some(p) = n_property.clone() {
                        let req = PropertyRequest {
                            property: p,
                            conn_label: n_conn_label.clone(),
                            driver: n_driver.clone(),
                        };
                        entity.update(app, |this, cx| {
                            this.host.show_properties(req, cx);
                            cx.notify();
                        });
                        return;
                    }
                }
                if let Some(p) = n_path.clone() {
                    let conn_id = n_conn_id.clone();
                    let key = n_key.clone();
                    entity.update(app, |this, cx| {
                        this.toggle_nav_node(&conn_id, &key, p, cx);
                        cx.notify();
                    });
                }
            }
        });
        // 右键菜单（对象节点）：查看数据 / 属性 / 复制 / 刷新。
        block = block.child(row.context_menu({
            let entity = cx.entity();
            let host = self.host.clone();
            let conn_id = node.connection_id.clone();
            let nkey = node.key.clone();
            let dname = node.name.clone();
            let menu_path = node.expand_path.clone();
            let menu_prop = node.property.clone();
            let qualified = node.property.as_ref().map(nav_qualified_name);
            let conn_label = m_conn_label.clone();
            let driver = m_driver.clone();
            let data_like = matches!(&node.kind, NavNodeKind::Table { .. } | NavNodeKind::View);
            // 表 / 视图的「生成 SQL」需要列：由后台任务取（命中 L2 不发查询）。
            let dml_target = match &menu_path {
                Some(NavPath::Table {
                    catalog,
                    schema,
                    table,
                }) if data_like => Some((catalog.clone(), schema.clone(), table.clone())),
                _ => None,
            };
            let dml_root = if dml_target.is_some() {
                self.host
                    .project_root()
                    .map(|p| p.to_string_lossy().to_string())
            } else {
                None
            };
            // 结构洞察的靶要在闭包**外**算好（`node` 的借用活不过 `'static` 闭包）
            let insight_schema = insight_schema_target(&node.kind, menu_path.as_ref(), &conn_id);
            // Mock / 洞察表入口的引用靶同理；**kind 按节点类型给**：视图不能标成表
            // ——引用是跨屏身份，标错会传染到属性面板 / 洞察 / 将来的“在树中定位”。
            let data_target = dml_target.clone().and_then(|(catalog, schema, name)| {
                nav_data_target(&node.kind, &conn_id, catalog, schema, name)
            });
            move |menu, window, cx| {
                let mut menu = menu;
                if let Some(prop0) = menu_prop.clone() {
                    let e = entity.clone();
                    let label0 = conn_label.clone();
                    let drv0 = driver.clone();
                    menu = menu.item(PopupMenuItem::new("查看属性").on_click(move |_, _, app| {
                        let prop = prop0.clone();
                        let label = label0.clone();
                        let drv = drv0.clone();
                        e.update(app, |this, cx| {
                            this.host.show_properties(
                                PropertyRequest {
                                    property: prop.clone(),
                                    conn_label: label.clone(),
                                    driver: drv.clone(),
                                },
                                cx,
                            );
                            cx.notify();
                        });
                    }));
                }
                if data_like {
                    if let Some(q) = qualified.clone() {
                        let sql = format!("SELECT * FROM {q} LIMIT 200;");
                        let host_sql = host.clone();
                        let cid_view = conn_id.clone();
                        menu = menu.item(PopupMenuItem::new("查看数据（LIMIT 200）").on_click(
                            move |_, _, app| {
                                // B11：打开一份绑定该连接的查询并**自动执行**
                                // （M4 遗留的“查看数据不自动执行”在此关闭）
                                host_sql.open_query(
                                    QueryRequest {
                                        conn_id: Some(cid_view.clone()),
                                        sql: sql.clone(),
                                        run: true,
                                    },
                                    app,
                                );
                            },
                        ));
                    }
                    // 「生成 SQL ▸」：由列信息生成 INSERT / UPDATE / DELETE 模板（只注入不执行）。
                    if let Some((catalog, schema, table)) = dml_target.clone() {
                        let q = crate::sql_gen::qualified_name(
                            Some(catalog.as_str()),
                            Some(schema.as_str()),
                            &table,
                        );
                        let e = entity.clone();
                        let key = nkey.clone();
                        let cid = conn_id.clone();
                        let root = dml_root.clone();
                        menu = menu.submenu("生成 SQL", window, cx, move |m, _w, _c| {
                            let mut m = m;
                            for kind in [DmlKind::Insert, DmlKind::Update, DmlKind::Delete] {
                                let e = e.clone();
                                let key = key.clone();
                                let cid = cid.clone();
                                let root = root.clone();
                                let catalog = catalog.clone();
                                let schema = schema.clone();
                                let table = table.clone();
                                let q = q.clone();
                                m = m.item(PopupMenuItem::new(kind.label()).on_click(
                                    move |_, _, app| {
                                        nav_jobs::enqueue_generate_dml(
                                            &key,
                                            &cid,
                                            root.as_deref(),
                                            &catalog,
                                            &schema,
                                            &table,
                                            &q,
                                            kind,
                                        );
                                        e.update(app, |this, cx| this.ensure_nav_pump(cx));
                                    },
                                ));
                            }
                            m
                        });
                    }
                }
                {
                    let e = entity.clone();
                    let host_copy = host.clone();
                    let name_copy = dname.clone();
                    menu = menu.item(PopupMenuItem::new("复制名称").on_click(move |_, _, app| {
                        let name = name_copy.clone();
                        app.write_to_clipboard(ClipboardItem::new_string(name.clone()));
                        host_copy.notice(format!("已复制：{name}"), app);
                        e.update(app, |_, cx| cx.notify());
                    }));
                }
                if let Some(q) = qualified.clone() {
                    let e = entity.clone();
                    let host_copy = host.clone();
                    menu =
                        menu.item(PopupMenuItem::new("复制限定名").on_click(move |_, _, app| {
                            let q = q.clone();
                            app.write_to_clipboard(ClipboardItem::new_string(q.clone()));
                            host_copy.notice(format!("已复制：{q}"), app);
                            e.update(app, |_, cx| cx.notify());
                        }));
                }
                if let Some(p) = menu_path.clone() {
                    let e = entity.clone();
                    let cid = conn_id.clone();
                    let key = nkey.clone();
                    menu =
                        menu.item(PopupMenuItem::new("刷新元数据").on_click(move |_, _, app| {
                            let cid = cid.clone();
                            let key = key.clone();
                            let p = p.clone();
                            e.update(app, |this, cx| {
                                this.refresh_node(&cid, &key, Some(p.clone()), cx);
                            });
                        }));
                }
                // 通用模块入口（所有对象节点都有，与节点类型 / 连接状态无关）：
                // SQL 编辑器 / 洞察；Mock 只针对表 / 视图（`data_like`）。
                menu = menu.separator().item({
                    // `host` 是上层块里的局部（`self` 不能进 'static 闭包）
                    let host = host.clone();
                    let cid = conn_id.clone();
                    PopupMenuItem::new("在 SQL 编辑器中打开").on_click(move |_, _, app| {
                        // B11：同上方连接菜单——只入队，宿主开文档并聚焦
                        host.open_query(
                            QueryRequest {
                                conn_id: Some(cid.clone()),
                                sql: String::new(),
                                run: false,
                            },
                            app,
                        );
                    })
                });
                if data_like {
                    let e = entity.clone();
                    // 定向请求：连接 + 源库对象（catalog / schema / 名字）——Mock 面板据此
                    // 读源库结构并预填目标表名（v1 主路径：源库结构 → 造新数据）。
                    let request = data_target.clone();
                    // 洞察那份先克隆出来：Mock 的闭包会把 `request` move 进去
                    let request_for_insight = request.clone();
                    menu = menu.item(PopupMenuItem::new("生成 Mock 数据").on_click(
                        move |_, _, app| {
                            let request = request.clone();
                            e.update(app, |this, cx| {
                                this.host.open_mock_panel(request, cx);
                            });
                        },
                    ));
                    // 【M8】看这张源表的统计：取样（LIMIT 500 → 分析临时表）由洞察侧完成，
                    // 这里只给「哪条连接 + 哪张表」（D58 的统一入口）。
                    if let Some(source) = request_for_insight {
                        let e = entity.clone();
                        menu =
                            menu.item(PopupMenuItem::new("查看统计").on_click(move |_, _, app| {
                                let source = source.clone();
                                e.update(app, |this, cx| this.host.open_insight_table(source, cx));
                            }));
                    }
                }
                // 【M8】结构洞察（Schema 级）：表 / 视图用它所在的 schema，schema 节点用自己。
                // 与「查看统计」分工：那个看**数据**（取样），这个看**结构**（源库内省）。
                if let Some(target) = insight_schema.clone() {
                    let e = entity.clone();
                    menu = menu.item(PopupMenuItem::new("结构洞察").on_click(move |_, _, app| {
                        let target = target.clone();
                        e.update(app, |this, cx| this.host.open_insight_schema(target, cx));
                    }));
                }
                menu = menu.item({
                    let e = entity.clone();
                    PopupMenuItem::new("查看洞察").on_click(move |_, _, app| {
                        e.update(app, |this, cx| {
                            this.host.open_right_panel(RightPanel::Insight, cx);
                        });
                    })
                });
                menu
            }
        }));

        let indent_px = indent + ui::TREE_INDENT;
        if self.nav.borrow().loading.contains(&node.key) {
            block = block.child(nav_subline_at("加载中…", muted, indent_px));
        }
        if let Some(err) = error {
            block = block.child(nav_subline_at(&err, danger, indent_px));
        }
        // 子树与「加载更多」/「已定位」行不在这一行里：它们是**同桌的其它行**
        //（`collect_nav_rows` 把它们摆在同一层），所以这里不递归、不再往下画。
        block
    }

    /// 「加载更多」/「显示更多」行（大 schema 分页）。
    ///
    /// 两种语义分开（用户点击的代价不同，必须区分）：
    /// - `total > loaded`：数据侧还有没取的（索引分页）→ 点击**排队取下一页**；
    /// - 否则：数据已到手，只是超出渲染窗口 → 点击**只放大窗口**，不重查也不排队。
    pub(super) fn render_more_row(
        &self,
        node: &NavNode,
        loaded: usize,
        total: usize,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pri = cx.theme().colors.primary;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let selected_bg = cx.theme().colors.list_active;
        let active_border = cx.theme().colors.list_active_border;
        let indent = ui::TREE_BASE_PADDING + (depth as f32 + 1.0) * ui::TREE_INDENT;
        let entity = cx.entity();
        let key = node.key.clone();
        let to_fetch = total.saturating_sub(loaded);
        // 带筛选时，本地过滤只能命中**已加载**的那些：这一点必须说出口，
        // 否则用户会以为“搜不到 = 库里没有”。
        let filtering = !self.nav.borrow().filter.is_empty();
        let scope = if filtering {
            "；筛选仅覆盖已加载"
        } else {
            ""
        };
        // 取数中的那一帧改文案：否则行外观不变、再点也静默无反应（单线程串行队列）。
        let fetching = self.nav.borrow().loading.contains(&key);
        let label = if fetching {
            "加载中…".to_string()
        } else {
            format!("加载更多（余 {to_fetch} / 共 {total}{scope}）")
        };
        let selected = self.nav_row_selected(&key);
        let conn_id = node.connection_id.clone();
        let path = node.expand_path.clone();
        div()
            .id(format!("nav-more-{key}"))
            .debug_selector(|| "nav-row-more".to_string())
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(ui::NAV_ROW_TREE))
            .pl(rems(indent))
            .pr_1()
            .rounded_md()
            .relative()
            .cursor_pointer()
            .when(selected, |s| {
                s.bg(selected_bg).child(tree::active_bar(active_border))
            })
            // 这行也进键盘漫游（`NavRow::selectable`）：悬停 / 选中都要有反馈。
            .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
            // 定高槽位（`NAV_ROW_TREE`）：长文案截断，不靠换行（虚拟列表不回写实测行高）。
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(if fetching { muted } else { pri })
                    .text_ellipsis()
                    .child(label),
            )
            .on_click(move |_, _, app| {
                let k = key.clone();
                let conn_id = conn_id.clone();
                let path = path.clone();
                entity.update(app, |this, cx| {
                    // 已在取数就别重复排队：后台任务是单线程串行的，重复点会叠成一串。
                    if this.nav.borrow().loading.contains(&k) {
                        return;
                    }
                    let Some(path) = path.clone() else { return };
                    let root = this
                        .host
                        .project_root()
                        .map(|p| p.to_string_lossy().to_string());
                    this.nav.borrow_mut().loading.insert(k.clone());
                    nav_jobs::enqueue_load_page(
                        &conn_id,
                        root.as_deref(),
                        &k,
                        path,
                        false,
                        loaded,
                        nav_jobs::PAGE_SIZE,
                    );
                    this.ensure_nav_pump(cx);
                    cx.notify();
                });
            })
    }

    /// 「已定位到第 N 条」行（定位窗口的说明与回头路）。
    ///
    /// 为什么必须有它：定位跳到的是**一窗**而不是前缀，不说清楚用户会以为上面的对象没了；
    /// 同时它是回开头的唯一入口（定位窗口里不摆「加载更多」，那条语义对不上）。
    pub(super) fn render_jumped_row(
        &self,
        node: &NavNode,
        position: usize,
        loaded: usize,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let selected_bg = cx.theme().colors.list_active;
        let active_border = cx.theme().colors.list_active_border;
        let indent = ui::TREE_BASE_PADDING + (depth as f32 + 1.0) * ui::TREE_INDENT;
        let entity = cx.entity();
        let key = node.key.clone();
        let selected = self.nav_row_selected(&key);
        let conn_id = node.connection_id.clone();
        let path = node.expand_path.clone();
        div()
            .id(format!("nav-jumped-{key}"))
            .debug_selector(|| "nav-row-jump".to_string())
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(ui::NAV_ROW_TREE))
            .pl(rems(indent))
            .pr_1()
            .rounded_md()
            .relative()
            .cursor_pointer()
            .when(selected, |s| {
                s.bg(selected_bg).child(tree::active_bar(active_border))
            })
            .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
            // 文案压到最短（旧文案 ≈288px，超过 240px 面板的可用宽度，必然换行撑破定高槽位）；
            // 仍然截断兜底：`NAV_ROW_TREE` 是虚拟列表钉死的行高。
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(format!(
                        "已定位第 {} 条（本窗 {}）· 回开头",
                        position + 1,
                        loaded
                    )),
            )
            .on_click(move |_, _, app| {
                let k = key.clone();
                let conn_id = conn_id.clone();
                let path = path.clone();
                entity.update(app, |this, cx| {
                    this.nav.borrow_mut().jumped.remove(&k);
                    let Some(path) = path.clone() else { return };
                    let root = this
                        .host
                        .project_root()
                        .map(|p| p.to_string_lossy().to_string());
                    this.nav.borrow_mut().loading.insert(k.clone());
                    nav_jobs::enqueue_load_page(
                        &conn_id,
                        root.as_deref(),
                        &k,
                        path,
                        false,
                        0,
                        nav_jobs::PAGE_SIZE,
                    );
                    this.ensure_nav_pump(cx);
                    cx.notify();
                });
            })
    }

    /// 一条搜索命中行（索引搜索）：类别图标 + 名称（高亮命中）+ 类别 / 归属（`schema · 连接`）。
    ///
    /// 为何是「行」而不是「结果区块里的一行」：它现在在虚拟列表里（[`NavRow::SearchHit`]），
    /// 于是 ↑↓ 走得到、悬停与选中跟树行同一套（`nav_row_selected` 是唯一判据），
    /// 也不再有一百条命中挤在 128px 小组里二段滚动的事。
    ///
    /// 单击 → 选中 + 打开属性面板：搜索结果的价值就在「还没展开到那层时也能立刻看结构」。
    /// 行尾「定位」（**真能定位**时才摆）把这一行落到树上。
    pub(super) fn render_search_hit(
        &self,
        hit: &nav_jobs::SearchHit,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let pri = cx.theme().colors.primary;
        let selected_bg = cx.theme().colors.list_active;
        let active_border = cx.theme().colors.list_active_border;
        let match_bg = product_tokens::get(cx).search_match_background(cx.theme());
        let kind_icon = nav_kind_icon(&nav_search_hit_kind(hit), false);
        let filter = self.nav.borrow().filter.to_lowercase();
        // 业务键与元素 id 都走 `NavRow` 的口径（位次 + 引用），不在这里另拼一份：
        // 行态判据要认回同一把键，元素 id 又要同桌唯一（见 `search_hit_row_key`）。
        let key = search_hit_row_key(ix, hit);
        let id = search_hit_row_id(ix, hit);
        let selected = self.nav_row_selected(&key);

        // 归属：`schema · 连接`（列再带上所属表）
        let mut scope = Vec::new();
        if let Some(parent) = &hit.parent_name {
            scope.push(parent.clone());
        }
        if let Some(schema) = &hit.schema {
            scope.push(schema.clone());
        }
        scope.push(hit.conn_label.clone());
        let scope = scope.join(" · ");
        let kind_label = nav_object_type_label(&hit.object_type);

        let entity = cx.entity();
        let entity_locate = cx.entity();
        let property = nav_search_hit_property(hit);
        let conn_label = hit.conn_label.clone();
        let driver = hit.driver.clone();

        let mut row = div()
            .id(id.clone())
            .debug_selector(|| "nav-row-search-hit".to_string())
            .h_flex()
            .items_center()
            .gap_1p5()
            .w_full()
            .h(rems(ui::NAV_ROW_TREE))
            .px_1()
            .rounded_md()
            .relative()
            .cursor_pointer()
            .when(selected, |s| {
                s.bg(selected_bg).child(tree::active_bar(active_border))
            })
            // 进漫游序列的行都是同一条规矩：悬停只在未选中时生效（不盖选中底色）。
            .when(!selected, move |s| s.hover(move |s| s.bg(hover)))
            // 类别用**形状**（与树内同形），大小与树行一致——同一对象在结果区与树上要认得出来。
            .child(nav_icon(kind_icon, ui::ICON_SIZE_SM, muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(fg)
                    .text_ellipsis()
                    .child(nav_name_highlight(&hit.object_name, &filter, match_bg, fg)),
            )
            // 类别 / 归属元信息：可缩可截（旧写法 `flex_shrink_0` 无上限，会把名称挤没）。
            .child(
                div()
                    .flex_shrink_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(format!("{kind_label} · {scope}")),
            );

        // 「定位」只在**真能定位**时摆出（判据与状态机同一处：`RevealTarget::from_hit`）——
        // 摆了却点了没反应，比不摆更伤。
        if RevealTarget::from_hit(hit).is_some() {
            let hit_for_locate = hit.clone();
            row = row.child(
                div()
                    .id(format!("nav-locate-{id}"))
                    .flex_none()
                    .h_5()
                    .px_1p5()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .text_xs()
                    .text_color(pri)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .on_click(move |_, _, app| {
                        // 行本身的单击是「看属性」：点「定位」不该同时把属性面板也推出来。
                        app.stop_propagation();
                        let hit = hit_for_locate.clone();
                        entity_locate.update(app, |this, cx| this.reveal_hit(&hit, cx));
                    })
                    .child("定位"),
            );
        }

        row.on_click(move |_, _, app| {
            entity.update(app, |this, cx| {
                // 先选中再开面板：漫游序列里走到这一行时，行态得看得出选的是哪一行。
                this.nav.borrow_mut().selected_key = Some(key.clone());
                if let Some(property) = property.clone() {
                    let req = PropertyRequest {
                        property,
                        conn_label: conn_label.clone(),
                        driver: driver.clone(),
                    };
                    this.host.show_properties(req, cx);
                }
                cx.notify();
            });
        })
    }
}
