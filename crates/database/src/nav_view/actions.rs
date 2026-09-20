//! 面板的状态变更：展开 / 刷新 / 定位泵 / 后台结果回填 / 筛选落库 / 拖拽落点。
//!
//! 与渲染模块的边界：本模块几乎不画东西，只改状态并 `cx.notify()`；
//! 渲染期只读的纪律（render 内不做 I/O）在 `chrome.rs` / `rows.rs` 的注释里。

use super::*;

impl NavView {
    /// 写回 facet 筛选到 `settings.json`（chips 状态为准；搜索 token 不持久化）。
    pub(super) fn write_nav_filters(&self, cx: &mut Context<Self>) {
        let filters = {
            let view = self.nav.borrow();
            NavFilters {
                source: view.source_filter.map(|s| s.key().to_string()),
                db_type: view.type_filter.clone(),
                driver: view.driver_filter.clone(),
                tag: view.tag_filter.clone(),
            }
        };
        self.host.set_nav_filters(filters, cx);
    }

    /// 应用某个 facet 值（`None` = 清除该项），并持久化 + 重渲染。
    pub(super) fn apply_facet(
        &mut self,
        facet: NavFacet,
        value: Option<String>,
        cx: &mut Context<Self>,
    ) {
        {
            let mut view = self.nav.borrow_mut();
            match facet {
                NavFacet::Type => view.type_filter = value,
                NavFacet::Driver => view.driver_filter = value,
                NavFacet::Tag => view.tag_filter = value,
            }
        }
        self.write_nav_filters(cx);
        cx.notify();
    }

    /// 清空搜索框（`Esc` / `NavClearSearch`）。
    ///
    /// 只清**自由文本**，与资产库的 `ClearSearch` 同一条口径：facet（归属域 / 类型 / 驱动 /
    /// 标签）在 chips 与「筛选 ▾」里看得见，误清会让人以为筛选坏了。
    ///
    /// 为何连索引搜索结果也一起清：标题行（「索引搜索：X（N 条）」）是搜索框的注脚，
    /// 词都清了它还挂着「3 条命中」就是自相矛盾；等下一帧排程去清会让它多闪一帧。
    pub(super) fn clear_nav_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.nav_search.clone() {
            // `set_value` 会发 `InputEvent::Change`（订阅里已 `cx.notify()`），
            // 于是下一帧的 `render_nav` 会把本地过滤词也重算为空。
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        {
            let mut view = self.nav.borrow_mut();
            view.search_query = None;
            view.search_hits.clear();
            view.search_searched = 0;
        }
        cx.notify();
    }

    /// 清除全部 facet 筛选（附加 facet + 归属域）。
    pub(super) fn clear_nav_filters(&mut self, cx: &mut Context<Self>) {
        {
            let mut view = self.nav.borrow_mut();
            view.type_filter = None;
            view.driver_filter = None;
            view.tag_filter = None;
            view.source_filter = None;
        }
        self.write_nav_filters(cx);
        cx.notify();
    }

    /// 已生效的附加 facet 数（类型 / 驱动 / 标签；chips 与搜索 token 取并）。
    pub(super) fn nav_active_facet_count(&self) -> usize {
        let view = self.nav.borrow();
        let mut n = 0;
        if view.type_filter.is_some() || view.search_facets.db_type.is_some() {
            n += 1;
        }
        if view.driver_filter.is_some() || view.search_facets.driver.is_some() {
            n += 1;
        }
        if view.tag_filter.is_some() || view.search_facets.tag.is_some() {
            n += 1;
        }
        n
    }

    /// 是否存在任何生效筛选（含归属域与搜索 token）；用于「清除筛选」可用性。
    pub(super) fn nav_filters_active(&self) -> bool {
        let view = self.nav.borrow();
        view.source_filter.is_some()
            || view.type_filter.is_some()
            || view.driver_filter.is_some()
            || view.tag_filter.is_some()
            || view.search_facets.active > 0
    }

    /// 类型 / 驱动以驱动目录为主、连接实际使用值为兜底（目录未就绪时不落空）；
    /// 标签来自已缓存的组织数据。
    pub(super) fn nav_facet_candidates(
        &self,
    ) -> (Vec<(String, String)>, Vec<(String, String)>, Vec<String>) {
        use std::collections::{BTreeMap, BTreeSet};
        let mut types: BTreeMap<String, String> = BTreeMap::new();
        let mut drivers: BTreeMap<String, String> = BTreeMap::new();
        {
            let catalog = self.driver_catalog.borrow();
            for (id, meta) in catalog.iter() {
                drivers
                    .entry(id.clone())
                    .or_insert_with(|| meta.name.clone());
                types.entry(meta.type_id.clone()).or_insert_with(|| {
                    nav_type_short_label(
                        &meta.type_id,
                        meta.type_name.as_deref().zip(meta.type_category.as_deref()),
                    )
                });
            }
        }
        let conns: Vec<ConnectionItem> = self.host.connections();
        {
            let catalog = self.driver_catalog.borrow();
            for c in &conns {
                // 一次取齐（不能在这里再借 `self.driver_catalog`：已持锁，会 double borrow）
                let (tid, tname, tcat) = catalog
                    .get(&c.driver)
                    .map(|m| {
                        (
                            m.type_id.clone(),
                            m.type_name.clone(),
                            m.type_category.clone(),
                        )
                    })
                    .unwrap_or_else(|| (c.driver.clone(), None, None));
                types.entry(tid.clone()).or_insert_with(|| {
                    nav_type_short_label(&tid, tname.as_deref().zip(tcat.as_deref()))
                });
                drivers
                    .entry(c.driver.clone())
                    .or_insert_with(|| c.driver.clone());
            }
        }
        let tags: BTreeSet<String> = self
            .nav
            .borrow()
            .tags
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect();
        let mut types: Vec<(String, String)> = types.into_iter().collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let mut drivers: Vec<(String, String)> = drivers.into_iter().collect();
        drivers.sort_by(|a, b| a.1.cmp(&b.1));
        (types, drivers, tags.into_iter().collect())
    }

    /// 索引搜索排程（查询词变了才排队；命中行由 [`Self::collect_search_rows`] 进列表，
    /// 标题行由 [`Self::render_search_header`] 呈现）。
    ///
    /// 方向（已拍板）：搜索 / 命令这类“先输入再选”的交互后续要**独立成一个 crate**
    /// （类 VS Code 的 Quick Open / 命令面板），导航面板只保留这个“够用”版本；
    /// 新特性不要再往这里加。
    pub(super) fn nav_schedule_index_search(
        &self,
        conn_filter: &NavConnFilter,
        conns: &[ConnectionItem],
        filter: &str,
        cx: &mut Context<Self>,
    ) {
        let query = filter.trim().to_string();
        let started = {
            let mut view = self.nav.borrow_mut();
            if !nav_search_query_ready(&query) {
                view.search_query = None;
                view.search_hits.clear();
                view.search_searched = 0;
                false
            } else if view.search_query.as_deref() == Some(query.as_str()) {
                // 查询词没变：不重搜（结果已经在状态里）。
                false
            } else {
                view.search_query = Some(query.clone());
                view.search_hits.clear();
                view.search_searched = 0;
                true
            }
        };
        if !started {
            return;
        }
        let tags = self.nav.borrow().tags.clone();
        let targets: Vec<nav_jobs::SearchTarget> = conns
            .iter()
            .filter(|c| self.nav_conn_passes_facets(conn_filter, c, &tags))
            .map(|c| nav_jobs::SearchTarget {
                conn_id: c.id.clone(),
                label: c.name.clone(),
                driver: c.driver.clone(),
            })
            .collect();
        let root = self
            .host
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        nav_jobs::enqueue_search(
            nav_jobs::SearchConsumer::Navigator,
            nav_jobs::SearchKind::Name,
            &query,
            root.as_deref(),
            targets,
        );
        self.ensure_nav_pump(cx);
    }

    /// 展开 / 折叠节点；首次展开时排队后台懒加载，并持久化展开态。
    pub(super) fn toggle_nav_node(
        &mut self,
        conn_id: &str,
        key: &str,
        path: NavPath,
        cx: &mut Context<Self>,
    ) {
        let now_expanded = {
            let mut view = self.nav.borrow_mut();
            if view.expanded.contains(key) {
                view.expanded.remove(key);
                false
            } else {
                view.expanded.insert(key.to_string());
                true
            }
        };
        if now_expanded {
            // 连接根展开：未建连则先建连。否则 `NavigatorService` → `MetadataService`
            // 取不到运行时句柄，冒泡为 `[CONN_NOT_FOUND]`（用户看到的“连不上”）。
            if matches!(path, NavPath::Connection) && !self.ensure_connected_for_browse(conn_id, cx)
            {
                self.save_nav_state_for(conn_id);
                return;
            }
            self.ensure_nav_loaded(conn_id, key, path, false, cx);
        }
        self.save_nav_state_for(conn_id);
    }

    /// 展开前的隐式建连：未连接时先建连；返回是否可用（已连接 或 建连成功）。
    ///
    /// 失败时写面板提示（与 `toggle_connection` 同文案），不阻后续可重试。
    pub(super) fn ensure_connected_for_browse(
        &mut self,
        conn_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let already =
            self.host.is_connected(conn_id) || self.nav.borrow().connected.contains(conn_id);
        if already {
            return true;
        }
        let root = self
            .host
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        match self.host.connect(conn_id) {
            Ok(()) => {
                {
                    let mut view = self.nav.borrow_mut();
                    view.connected.insert(conn_id.to_string());
                    view.prefetched.clear();
                    // 清掉上一次的负载错误（如 CONN_NOT_FOUND），以便重试加载。
                    view.errors.remove(conn_id);
                }
                nav_jobs::warm_after_connect(conn_id, root.as_deref());
                self.ensure_warm_poll(cx);
                true
            }
            Err(e) => {
                self.host.notice(format!("连接失败: {e}"), cx);
                false
            }
        }
    }

    /// 排队后台懒加载子节点（已请求过则跳过；render 路径不做 I/O）。
    ///
    /// `fresh`（刷新模式）跳过 L2 读缓存并重写。结果由 [`Self::apply_load_results`] 回填。
    pub(super) fn ensure_nav_loaded(
        &self,
        conn_id: &str,
        key: &str,
        path: NavPath,
        fresh: bool,
        cx: &mut Context<Self>,
    ) {
        {
            let view = self.nav.borrow();
            if view.attempted.contains(key) {
                return;
            }
        }
        {
            let mut view = self.nav.borrow_mut();
            view.attempted.insert(key.to_string());
            view.loading.insert(key.to_string());
        }
        let project_root = self
            .host
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        nav_jobs::enqueue_load(conn_id, project_root.as_deref(), key, path, fresh);
        self.ensure_nav_pump(cx);
    }

    /// 回填索引搜索结果（过期批次直接丢弃：用户在等待期间已经把词改了）。
    pub(super) fn apply_search_results(
        &mut self,
        results: Vec<nav_jobs::SearchResult>,
        cx: &mut Context<Self>,
    ) {
        {
            let mut view = self.nav.borrow_mut();
            for r in results {
                if view.search_query.as_deref() != Some(r.query.as_str()) {
                    continue;
                }
                view.search_hits = r.hits;
                view.search_searched = r.searched;
                // 新一批结果 = 新一轮搜索：上一轮的定位提示不再适用
                view.reveal_note = None;
            }
        }
        cx.notify();
    }

    /// 从一条搜索命中**在树中定位**：展开链路并选中目标。
    ///
    /// 不可定位的命中会当场回一句可读理由：结果行上那个入口本就只在可定位时摆出，
    /// 这里再兜一层是因为「摆了入口却点了没反应」是最伤的交互。
    pub fn reveal_hit(&mut self, hit: &nav_jobs::SearchHit, cx: &mut Context<Self>) {
        let target = RevealTarget::from_hit(hit);
        self.begin_reveal(target, &hit.object_name, cx);
    }

    /// 从一条**统一引用**在树中定位（Quick Open 的 `⌥↵` 等入口）。
    pub fn reveal_ref(&mut self, object: &ObjectRef, cx: &mut Context<Self>) {
        let target = RevealTarget::from_ref(object);
        self.begin_reveal(target, &object.name, cx);
    }

    /// 两个入口共用的一段：置意图并推一步；不可定位就**当场回一句可读理由**。
    pub(super) fn begin_reveal(
        &mut self,
        target: Option<RevealTarget>,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        match target {
            Some(target) => {
                {
                    let mut view = self.nav.borrow_mut();
                    view.reveal_note = None;
                    view.reveal = Some(target);
                }
                self.pump_reveal(cx);
            }
            None => {
                self.nav.borrow_mut().reveal_note = Some(format!(
                    "「{name}」没有可定位的位置（缺 schema 归属或类别未知）"
                ));
                cx.notify();
            }
        }
    }

    /// 定位状态机（跨帧推进）。
    ///
    /// 树是**逐层异步加载**的：连接 → catalog → schema → 文件夹 → （列再一层表）。
    /// 因此定位不能一口气做完，只能「能推一步就推一步」，推不动就停下等回执
    /// （加载回执、定位页回执都会再进这里）。
    pub(super) fn pump_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.nav.borrow().reveal.clone() else {
            return;
        };
        let conn_key = target.conn_id.clone();

        // ① catalog 未知（内容档命中没有这一列）：用连接下**已加载**的第一个 catalog。
        //    大多数驱动只有一个 catalog（退化容器也算一个），这条规则足够且不做猜测定。
        let catalog = if target.catalog.is_empty() {
            let first = self
                .nav
                .borrow()
                .children
                .get(&conn_key)
                .and_then(|children| children.first().map(|n| n.name.clone()));
            match first {
                Some(name) => {
                    if let Some(t) = self.nav.borrow_mut().reveal.as_mut() {
                        t.catalog = name.clone();
                    }
                    name
                }
                None => {
                    // catalog 列表还没到：先把连接展开并排队，下一拍再看
                    self.nav.borrow_mut().expanded.insert(conn_key.clone());
                    self.ensure_nav_loaded(
                        &target.conn_id,
                        &conn_key,
                        NavPath::Connection,
                        false,
                        cx,
                    );
                    return;
                }
            }
        } else {
            target.catalog.clone()
        };

        // ② 逐层展开：哪一层的子节点还没到位就停下等它
        //
        // 先确保连接可用：子节点加载要经驱动回源（目录列表不在 L2 里），
        // 未建连时那条跳会以 `[CONN_NOT_FOUND]` 失败——不先建就连，定位会卡在那里不动。
        if !self.ensure_connected_for_browse(&target.conn_id, cx) {
            let mut view = self.nav.borrow_mut();
            view.reveal = None;
            view.reveal_note = Some(format!("定位中断：连接 {} 不可用", target.conn_id));
            drop(view);
            cx.notify();
            return;
        }
        let levels = target.levels(&catalog);
        for (key, path) in &levels {
            self.nav.borrow_mut().expanded.insert(key.clone());
            // 某一层已经报错：不再等（等不到），当场把原因摆出来
            let failed = self.nav.borrow().errors.get(key).cloned();
            if let Some(err) = failed {
                let mut view = self.nav.borrow_mut();
                view.reveal = None;
                view.reveal_note = Some(format!("定位中断：{err}"));
                drop(view);
                cx.notify();
                return;
            }
            if !self.nav.borrow().children.contains_key(key) {
                self.ensure_nav_loaded(&target.conn_id, key, path.clone(), false, cx);
                return;
            }
        }

        // ③ 最后一层的子节点里找目标
        let (parent_key, _) = levels.last().cloned().expect("至少有连接这一层");
        let children = self
            .nav
            .borrow()
            .children
            .get(&parent_key)
            .cloned()
            .unwrap_or_default();
        let target_key = target.node_key(&catalog);
        if children.iter().any(|n| n.key == target_key) {
            let mut view = self.nav.borrow_mut();
            view.selected_key = Some(target_key.clone());
            view.reveal = None;
            view.reveal_note = None;
            drop(view);
            // 滚到眼前：行集合下一帧才包含目标（上面刚改了展开 / 窗口），
            // 所以这里只登记意图，由 `render_nav` 在收集之后兑现。
            self.nav_pending_scroll = Some(target_key);
            cx.notify();
            return;
        }

        // ④ 不在已加载的那一窗里：数据侧还有就跳页；已经全加载了就如实说没有
        let (total, already_jumped) = {
            let view = self.nav.borrow();
            (
                view.child_total.get(&parent_key).copied(),
                view.jumped.contains_key(&parent_key),
            )
        };
        let paged = total.map(|t| t > children.len()).unwrap_or(false);
        let jumpable = matches!(target.folder, Some(NavFolder::Tables | NavFolder::Views));
        if paged && jumpable && !target.jumping && !already_jumped {
            if let Some(t) = self.nav.borrow_mut().reveal.as_mut() {
                t.jumping = true;
            }
            let Some(path) = levels.last().map(|(_, p)| p.clone()) else {
                return;
            };
            let root = self
                .host
                .project_root()
                .map(|p| p.to_string_lossy().to_string());
            nav_jobs::enqueue_locate_page(
                &target.conn_id,
                root.as_deref(),
                &parent_key,
                path,
                &target.name,
                nav_jobs::PAGE_SIZE,
            );
            self.ensure_nav_pump(cx);
            return;
        }

        // 不留悬念：不支持的类别 / 索引里确实没有，都当场说清楚
        let note = if paged && !jumpable {
            format!(
                "「{}」所在类别不支持跳页定位（仅表 / 视图可分页）",
                target.name
            )
        } else {
            format!("未在索引里找到「{}」（可能索引尚未重建）", target.name)
        };
        {
            let mut view = self.nav.borrow_mut();
            view.reveal = None;
            view.reveal_note = Some(note);
        }
        cx.notify();
    }

    /// 启动加载结果轮询（已有存活任务时不重复启动）。
    pub(super) fn ensure_nav_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.nav_pump.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                let results = nav_jobs::drain_load_results();
                let had = !results.is_empty();
                if had
                    && weak
                        .update(cx, |this, cx| this.apply_load_results(results, cx))
                        .is_err()
                {
                    return;
                }
                // 生成 SQL / 测试连接：同一轮询泵回填（两者都可能在菜单触发）。
                let sql_results = nav_jobs::drain_sql_results();
                if !sql_results.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_sql_results(sql_results, cx))
                        .is_err()
                {
                    return;
                }
                let test_results = nav_jobs::drain_test_results();
                if !test_results.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_test_results(test_results, cx))
                        .is_err()
                {
                    return;
                }
                // 搜索：同一轮询泵回填（搜索框的跨连接索引搜索）。
                let search_results =
                    nav_jobs::drain_search_results(nav_jobs::SearchConsumer::Navigator);
                if !search_results.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_search_results(search_results, cx))
                        .is_err()
                {
                    return;
                }
                let idle = !nav_jobs::has_pending_loads()
                    && !nav_jobs::has_pending_sql()
                    && !nav_jobs::has_pending_test()
                    && !nav_jobs::has_pending_search(nav_jobs::SearchConsumer::Navigator);
                if idle {
                    // 多等一拍确认没有新任务（render 可能刚入队）。
                    executor.timer(std::time::Duration::from_millis(120)).await;
                    if !nav_jobs::has_pending_loads()
                        && !nav_jobs::has_pending_sql()
                        && !nav_jobs::has_pending_test()
                        && !nav_jobs::has_pending_search(nav_jobs::SearchConsumer::Navigator)
                    {
                        break;
                    }
                }
            }
        });
        *self.nav_pump.borrow_mut() = Some(task);
    }

    /// 回填后台加载结果（主线程）。
    pub(super) fn apply_load_results(
        &mut self,
        results: Vec<nav_jobs::LoadResult>,
        cx: &mut Context<Self>,
    ) {
        for r in results {
            let key = r.key.clone();
            let is_tables_folder = matches!(
                &r.path,
                NavPath::Folder {
                    folder: NavFolder::Tables,
                    ..
                }
            );
            let (catalog, schema) = match &r.path {
                NavPath::Folder {
                    catalog, schema, ..
                } => (catalog.clone(), schema.clone()),
                _ => (String::new(), String::new()),
            };
            let conn_id = r.conn_id.clone();
            let project_root = r.project_root.clone();
            {
                let mut view = self.nav.borrow_mut();
                view.loading.remove(&key);
                match r.result {
                    Ok(page) => {
                        view.errors.remove(&key);
                        // 面板底「元数据更新于 X 分钟前」的事实源（取数成功就算一次）。
                        view.last_loaded_at = Some(SystemTime::now());
                        if let Some(pos) = r.jumped_to {
                            // 定位：**换窗**（当前只展示含目标的那一页），不是追加——
                            // 追加会把一窗行拼在前缀后面，既不对也不好解释。
                            view.jumped.insert(key.clone(), pos);
                            view.children.insert(key.clone(), page.nodes);
                            view.child_total.insert(key.clone(), page.total);
                        } else if r.offset == 0 {
                            // 首屏：整体替换（刷新 / 重新展开都走这里）。
                            // 同时也是「回到开头」那条路：定位窗口到此结束。
                            view.jumped.remove(&key);
                            view.children.insert(key.clone(), page.nodes);
                            view.child_total.insert(key.clone(), page.total);
                        } else {
                            // 追加页：去重后并入（见 `nav_merge_page`）。
                            let entry = view.children.entry(key.clone()).or_default();
                            let appended = nav_merge_page(entry, page.nodes);
                            let loaded = entry.len();
                            // 索引计数与实际行数可能不一致（索引陈旧）：翻到底（空块）就收敛到
                            // 实际已加载数，否则「加载更多」会永远挂在树上、点了没反应。
                            let total = if appended == 0 { loaded } else { page.total };
                            view.child_total.insert(key.clone(), total);
                        }
                    }
                    Err(e) => {
                        // 失败必须留痕：树上那行红字是**屏幕级**的，关掉窗口就没了；
                        // 真机报「展开表就报错」时，日志里得有连接 / 路径 / 原因。
                        tracing::warn!(
                            conn_id = %r.conn_id,
                            key = %key,
                            path = ?r.path,
                            offset = r.offset,
                            error = %e,
                            "导航加载失败（树内提示：展开后那一行红字）"
                        );
                        view.errors.insert(key.clone(), e);
                    }
                }
            }
            // C2：「表」文件夹首次加载成功后，排队预取前 N 张表的列。
            if is_tables_folder {
                self.maybe_prefetch(&conn_id, &key, &catalog, &schema, project_root.as_deref());
            }
        }
        // 定位：这一批回执可能正好把目标送到了（或送来了它所在的那一页），推进一步。
        self.pump_reveal(cx);
        cx.notify();
    }

    /// 回填「生成 SQL」结果：成功注入编辑区（打开一份绑定该连接的草稿），失败落提示。
    ///
    /// **不自动执行**：模板是给人改的（写语句更不该替用户跑）。
    pub(super) fn apply_sql_results(
        &mut self,
        results: Vec<nav_jobs::SqlGenResult>,
        cx: &mut Context<Self>,
    ) {
        for r in results {
            match r.result {
                Ok(sql) => {
                    let conn_id = self
                        .host
                        .selected_index()
                        .and_then(|i| self.host.connections().get(i).map(|item| item.id.clone()));
                    self.host.open_query(
                        QueryRequest {
                            conn_id,
                            sql,
                            run: false,
                        },
                        cx,
                    );
                    self.host.notify_host(cx);
                }
                Err(e) => {
                    self.host.notice(format!("生成 SQL 失败：{e}"), cx);
                }
            }
        }
        cx.notify();
    }

    /// 回填「测试连接」结果（结果文案加连接名前缀，直接落面板提示）。
    pub(super) fn apply_test_results(
        &mut self,
        results: Vec<nav_jobs::TestConnResult>,
        cx: &mut Context<Self>,
    ) {
        for r in results {
            let msg = match r.result {
                Ok(m) => format!("{}：{m}", r.name),
                Err(e) => format!("{}：{e}", r.name),
            };
            self.host.notice(msg, cx);
        }
        cx.notify();
    }

    /// C2：对刚加载完的「表」文件夹排队列预取（一次性）。
    pub(super) fn maybe_prefetch(
        &self,
        conn_id: &str,
        key: &str,
        catalog: &str,
        schema: &str,
        project_root: Option<&str>,
    ) {
        let first_time = self.nav.borrow_mut().prefetched.insert(key.to_string());
        if !first_time {
            return;
        }
        let targets: Vec<nav_jobs::ColumnTarget> = {
            let view = self.nav.borrow();
            view.children
                .get(key)
                .map(|kids| {
                    kids.iter()
                        .filter(|n| matches!(n.kind, NavNodeKind::Table { .. }))
                        .take(nav_jobs::PREFETCH_BATCH)
                        .map(|n| nav_jobs::ColumnTarget {
                            catalog: catalog.to_string(),
                            schema: schema.to_string(),
                            table: n.name.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        nav_jobs::prefetch_columns(conn_id, project_root, targets);
    }

    /// 当前项目根（项目 / 共享连接的导航状态落项目库）。

    /// 首次渲染某连接时，从库中恢复其展开态。
    pub(super) fn ensure_nav_state_loaded(&self, conn_id: &str) {
        {
            let view = self.nav.borrow();
            if view.state_loaded.contains(conn_id) {
                return;
            }
        }
        let state = crate::nav_store::load_nav_state(conn_id, self.host.project_root().as_deref());
        let prefix = format!("{conn_id}/");
        let mut view = self.nav.borrow_mut();
        for key in state.expanded_keys {
            if key == conn_id || key.starts_with(&prefix) {
                view.expanded.insert(key);
            }
        }
        view.state_loaded.insert(conn_id.to_string());
    }

    /// 持久化某连接的展开态。
    pub(super) fn save_nav_state_for(&self, conn_id: &str) {
        let prefix = format!("{conn_id}/");
        let keys: Vec<String> = {
            let view = self.nav.borrow();
            view.expanded
                .iter()
                .filter(|k| *k == conn_id || k.starts_with(&prefix))
                .cloned()
                .collect()
        };
        let state = crate::model::NavState {
            expanded_keys: keys,
            ..Default::default()
        };
        let _ =
            crate::nav_store::save_nav_state(conn_id, self.host.project_root().as_deref(), &state);
    }

    /// 容器展示名（分组名；`GROUP_UNGROUPED` → 「未分组」）：拖拽通知文案用。
    pub(super) fn container_label(&self, scope_id: &str) -> String {
        if scope_id == GROUP_UNGROUPED {
            return "未分组".to_string();
        }
        self.nav
            .borrow()
            .groups
            .iter()
            .find(|g| g.id == scope_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| scope_id.to_string())
    }

    /// 容器当前的**全部成员**顺序（不做搜索 / facet 筛选）。
    ///
    /// 排序落库要覆盖容器的全部成员：用渲染过的（已筛选）列表写库，会把被过滤掉的
    /// 行在下次写库时丢掉位置。
    pub(super) fn container_order(&self, scope_id: &str) -> Vec<String> {
        if scope_id != GROUP_UNGROUPED {
            return self
                .nav
                .borrow()
                .group_order
                .get(scope_id)
                .cloned()
                .unwrap_or_default();
        }
        // 未分组：成员由“不属于任何分组”推导；顺序与分组内同一条规则。
        let view = self.nav.borrow();
        let conns = self.host.connections();
        let ranked: HashMap<&str, i64> = view
            .ungrouped_order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i as i64))
            .collect();
        let stored: Vec<(String, Option<i64>)> = conns
            .iter()
            .filter(|c| {
                view.membership
                    .get(&c.id)
                    .map(|gs| gs.is_empty())
                    .unwrap_or(true)
            })
            .map(|c| (c.id.clone(), ranked.get(c.id.as_str()).copied()))
            .collect();
        nav_order_members(&stored, |id| {
            conns
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.to_string())
        })
    }

    /// 拖拽落点动作：把连接移到 `scope_id` 容器的指定位置。
    ///
    /// 归属与顺序分两步：先改归属（未分组 = 移出全部分组；分组 = 加入并**保留**其它归属，
    /// 多对多），再以**变更后**的真实成员算新顺序——拿拖拽前的快照会漏掉刚加入的成员。
    /// 已在目标容器且落点未造成位移时不写库（`nav_reorder` 返回 `None`）。
    pub(super) fn apply_conn_drop(
        &mut self,
        payload: &NavConnDragPayload,
        scope_id: &str,
        target: ConnDropTarget,
        cx: &mut Context<Self>,
    ) {
        let root = self.host.project_root();
        let conn_id = payload.conn_id.clone();
        let name = payload.name.clone();
        let label = self.container_label(scope_id);
        let groups_of: Vec<String> = self
            .nav
            .borrow()
            .membership
            .get(&conn_id)
            .cloned()
            .unwrap_or_default();
        let was_member = if scope_id == GROUP_UNGROUPED {
            groups_of.is_empty()
        } else {
            groups_of.iter().any(|g| g == scope_id)
        };

        // 1) 归属。
        let membership = if scope_id == GROUP_UNGROUPED {
            crate::nav_store::remove_from_all_groups(root.as_deref(), &conn_id)
        } else {
            crate::nav_store::add_to_group(root.as_deref(), scope_id, &conn_id)
        };
        if let Err(e) = membership {
            self.host.notice(format!("移动失败: {e}"), cx);
            cx.notify();
            return;
        }

        // 2) 顺序。落到容器头且已是成员时不改位置，避免“只是归组”把行拽到末尾。
        self.reload_nav_org();
        let next = match &target {
            ConnDropTarget::Container if was_member => None,
            ConnDropTarget::Container => {
                let current = self.container_order(scope_id);
                nav_reorder(&current, &conn_id, None)
            }
            ConnDropTarget::BeforeRow(before) => {
                let current = self.container_order(scope_id);
                nav_reorder(&current, &conn_id, Some(before.as_str()))
            }
        };
        if let Some(next) = next {
            if let Err(e) = crate::nav_store::set_container_order(root.as_deref(), scope_id, &next)
            {
                self.host.notice(format!("保存排序失败: {e}"), cx);
                cx.notify();
                return;
            }
            self.reload_nav_org();
        }

        // 3) 通知：先说归属变化（更重的动作），再说位置。
        self.host.notice(
            if !was_member {
                if scope_id == GROUP_UNGROUPED {
                    format!("已把「{name}」移出分组")
                } else {
                    format!("已把「{name}」加入「{label}」")
                }
            } else {
                format!("已调整「{name}」在「{label}」中的位置")
            },
            cx,
        );
        cx.notify();
    }

    /// 当前分组顺序（ID 列表，按 `sort_order` → 名称）。
    pub(super) fn group_ids(&self) -> Vec<String> {
        self.nav
            .borrow()
            .groups
            .iter()
            .map(|g| g.id.clone())
            .collect()
    }

    /// 分组拖拽落点：把分组排到 `before` 之前（`None` = 排到最后）。
    pub(super) fn apply_group_drop(
        &mut self,
        payload: &NavGroupDragPayload,
        before: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let ids = self.group_ids();
        let Some(next) = nav_reorder(&ids, &payload.group_id, before) else {
            return;
        };
        let root = self.host.project_root();
        match crate::nav_store::set_group_order(root.as_deref(), &next) {
            Ok(()) => {
                self.reload_nav_org();
                self.host
                    .notice(format!("已移动分组「{}」", payload.name), cx);
            }
            Err(e) => self.host.notice(format!("保存分组顺序失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 分组上移 / 下移一位（`delta` = -1 / +1）；边界为 no-op。
    pub(super) fn step_group(&mut self, group_id: &str, delta: i32, cx: &mut Context<Self>) {
        let ids = self.group_ids();
        let Some(next) = nav_step(&ids, group_id, delta) else {
            return;
        };
        let root = self.host.project_root();
        match crate::nav_store::set_group_order(root.as_deref(), &next) {
            Ok(()) => self.reload_nav_org(),
            Err(e) => self.host.notice(format!("保存分组顺序失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 键盘重排：把选中连接在它的**主容器**内上移 / 下移一位（`delta` = -1 / +1）。
    ///
    /// 只作用于连接行——对象节点（库 / 表 / 列…）的顺序由后端内省给出，不能重排。
    /// 容器取「主组解析」（与渲染同一套规则），无任何分组时就是「未分组」。
    pub(super) fn nav_step_selected(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(conn_id) = self.nav.borrow().selected_key.clone() else {
            return;
        };
        let Some(name) = self
            .host
            .connections()
            .iter()
            .find(|c| c.id == conn_id)
            .map(|c| c.name.clone())
        else {
            return;
        };
        let scope = {
            let view = self.nav.borrow();
            nav_primary_scope(&view.membership, &view.primary_group, &conn_id)
                .unwrap_or_else(|| GROUP_UNGROUPED.to_string())
        };
        let current = self.container_order(&scope);
        let Some(next) = nav_step(&current, &conn_id, delta) else {
            return;
        };
        let root = self.host.project_root();
        match crate::nav_store::set_container_order(root.as_deref(), &scope, &next) {
            Ok(()) => {
                self.reload_nav_org();
                let how = if delta < 0 { "上移" } else { "下移" };
                self.host.notice(format!("已{how}「{name}」"), cx);
            }
            Err(e) => self.host.notice(format!("保存排序失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 重载分组 / 成员关系 / 标签映射（组织变更后调用）。
    pub(super) fn reload_nav_org(&self) {
        let root = self.host.project_root();
        let groups = crate::nav_store::list_groups(root.as_deref());
        let mut membership: HashMap<String, Vec<String>> = HashMap::new();
        let mut group_order: HashMap<String, Vec<String>> = HashMap::new();
        // 名称表：未手动排序的成员要按名称升序，而名称不在组织存储里。
        let names: HashMap<String, String> = self
            .host
            .connections()
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        for group in &groups {
            let stored = crate::nav_store::list_group_members_detailed(root.as_deref(), &group.id);
            let ids = nav_order_members(&stored, |id| {
                names.get(id).cloned().unwrap_or_else(|| id.to_string())
            });
            for cid in &ids {
                membership
                    .entry(cid.clone())
                    .or_default()
                    .push(group.id.clone());
            }
            group_order.insert(group.id.clone(), ids);
        }
        let tags = crate::nav_store::list_all_tags(root.as_deref());
        let primary_group = crate::nav_store::list_primary_groups(root.as_deref());
        let ungrouped_order = crate::nav_store::list_ungrouped_order(root.as_deref());
        let driver_catalog = engine::persistence::load_driver_catalog();
        *self.driver_catalog.borrow_mut() = driver_catalog;
        let mut view = self.nav.borrow_mut();
        view.groups = groups;
        view.membership = membership;
        view.group_order = group_order;
        view.ungrouped_order = ungrouped_order;
        view.primary_group = primary_group;
        view.tags = tags;
        view.groups_loaded = true;
    }

    /// 分组是否折叠（缺省展开）。
    pub(super) fn group_collapsed(&self, group_id: &str) -> bool {
        self.nav.borrow().collapsed_groups.contains(group_id)
    }

    /// 该节点的对象总数（数据侧）；未知时回落到已加载条数。
    pub(super) fn node_total(&self, key: &str, loaded: usize) -> usize {
        self.nav
            .borrow()
            .child_total
            .get(key)
            .copied()
            .unwrap_or(loaded)
    }

    /// 共享至当前项目（`G_` → 项目侧 `GP_` 快照）。
    pub(super) fn share_connection_to_project(
        &mut self,
        conn_id: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        match self.host.share_connection(conn_id) {
            Ok(()) => {
                self.reload_connections(cx);
                self.host.notice(format!("「{name}」已共享至当前项目"), cx);
            }
            Err(e) => self.host.notice(format!("共享失败：{e}"), cx),
        }
        cx.notify();
    }

    /// 取消共享：删除项目侧 `GP_` 快照（全局定义保留）。
    pub(super) fn unshare_connection_from_project(
        &mut self,
        conn_id: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        let result = self.host.delete_connection(conn_id).map(|_| ());
        match result {
            Ok(()) => {
                // `delete` 对项目侧 `GP_` 即「取消共享」；运行时连接一并断开（缓存保留）。
                self.host.disconnect(conn_id).ok();
                self.reload_connections(cx);
                self.host
                    .notice(format!("已取消共享：「{name}」（全局定义保留）"), cx);
            }
            Err(e) => self.host.notice(format!("取消共享失败：{e}"), cx),
        }
        cx.notify();
    }

    /// 删除连接（物理删除；元数据缓存保留）。
    pub(super) fn delete_connection(&mut self, conn_id: &str, name: &str, cx: &mut Context<Self>) {
        let result = self.host.delete_connection(conn_id);
        // 运行时连接一并断开（缓存保留，可在「缓存管理」清理）。
        self.host.disconnect(conn_id).ok();
        match result {
            Ok(msg) => {
                self.reload_connections(cx);
                self.host.notice(format!("「{name}」：{msg}"), cx);
            }
            Err(e) => self.host.notice(format!("删除连接失败：{e}"), cx),
        }
        cx.notify();
    }

    /// 重载当前作用域可见连接（增 / 删 / 共享后调用）。
    ///
    /// 连接清单与选中下标修正都在宿主侧（那是宿主自持状态），这里只广播一次重载。
    pub(super) fn reload_connections(&mut self, cx: &mut Context<Self>) {
        self.host.reload_connections(cx);
        cx.notify();
    }

    /// 连接 / 断开运行时连接（保留缓存）。连接状态取运行时真值。
    pub(super) fn toggle_connection(
        &mut self,
        conn_id: &str,
        project_root: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let already =
            self.host.is_connected(conn_id) || self.nav.borrow().connected.contains(conn_id);
        let outcome = if already {
            self.host.disconnect(conn_id).map(|_| false)
        } else {
            self.host.connect(conn_id).map(|_| true)
        };
        match outcome {
            Ok(is_connected) => {
                {
                    let mut view = self.nav.borrow_mut();
                    if is_connected {
                        view.connected.insert(conn_id.to_string());
                        // 刷新模式下预取过的标记清空（重新连接后重新预取）。
                        view.prefetched.clear();
                    } else {
                        view.connected.remove(conn_id);
                    }
                }
                if is_connected {
                    // C1：连接成功后提交后台预热（仅 catalogs/schemas），不阻塞 UI。
                    nav_jobs::warm_after_connect(conn_id, project_root);
                    self.ensure_warm_poll(cx);
                }
            }
            Err(e) => self.host.notice(format!("连接操作失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 启动预热进度轮询（已有存活任务时不重复启动）。
    ///
    /// 预热在工作线程上跑，进度是原子量；这里用主线程 async 任务每 300ms 轮询并重绘，
    /// 结束后再重绘一次（让「预热中」指示消失）。
    pub(super) fn ensure_warm_poll(&mut self, cx: &mut Context<Self>) {
        if let Some(task) = &self.warm_poll {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            // 预热任务在队列里异步启动，`warm_active` 置位有延迟；
            // 因此用「连续两次非活动才退出」的判据，同时覆盖启动与结束。
            let mut idle = 0;
            loop {
                executor.timer(std::time::Duration::from_millis(300)).await;
                if nav_jobs::warm_active() {
                    idle = 0;
                } else {
                    idle += 1;
                    if idle >= 2 {
                        break;
                    }
                }
                if weak.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
            let _ = weak.update(cx, |_, cx| cx.notify());
        });
        self.warm_poll = Some(task);
    }

    /// 刷新某节点（含连接根）的元数据：清掉已缓存子节点后重新加载当前层级。
    ///
    /// 保留展开态：子节点重新渲染时会根据最新 `expand_path` 再次懒加载。缓存文件不删。
    pub(super) fn refresh_node(
        &self,
        conn_id: &str,
        key: &str,
        path: Option<NavPath>,
        cx: &mut Context<Self>,
    ) {
        let prefix = format!("{key}/");
        {
            let mut view = self.nav.borrow_mut();
            view.children
                .retain(|k, _| k != key && !k.starts_with(&prefix));
            view.attempted
                .retain(|k| k != key && !k.starts_with(&prefix));
            view.errors
                .retain(|k, _| k != key && !k.starts_with(&prefix));
            // 分页簿记也要清：孩子清了而总数留着的话，「加载更多」会按旧总数
            // 报出已不存在的余量（甚至对着空列表显示“余 5000”）。
            view.child_total
                .retain(|k, _| k != key && !k.starts_with(&prefix));
        }
        if let Some(p) = path {
            self.ensure_nav_loaded(conn_id, key, p, true, cx);
        }
        self.host.notice("已刷新元数据".to_string(), cx);
        cx.notify();
    }

    /// 刷新全部连接的元数据（清掉已加载子节点后重载仍展开的连接根）。
    pub(super) fn refresh_all(&self, cx: &mut Context<Self>) {
        let roots: Vec<String> = {
            let view = self.nav.borrow();
            self.host
                .connections()
                .iter()
                .filter(|c| view.expanded.contains(&c.id))
                .map(|c| c.id.clone())
                .collect()
        };
        {
            let mut view = self.nav.borrow_mut();
            view.children.clear();
            view.attempted.clear();
            view.errors.clear();
            view.child_total.clear();
        }
        for cid in roots {
            self.ensure_nav_loaded(&cid, &cid, NavPath::Connection, true, cx);
        }
        self.host.notice("已刷新全部元数据".to_string(), cx);
        cx.notify();
    }
}
