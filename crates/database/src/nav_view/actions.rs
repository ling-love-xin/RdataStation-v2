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
            // `set_value` 会发 `InputEvent::Change`（订阅里会标脏 + `cx.notify()`），
            // 于是下一帧的 `render_nav` 会把本地过滤词也重算为空。
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        {
            let mut view = self.nav.borrow_mut();
            view.search_query = None;
            view.search_hits.clear();
            view.search_searched = 0;
        }
        // 清空是用户的**收尾动作**（而不是过程），不等防抖窗口，当场落库。
        self.flush_nav_state(cx);
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

    /// 应用一个已保存视图：把其筛选快照整体写入 chips（含归属域 `source`），
    /// 持久化 + 重渲染。只动四项 facet，**不动搜索框**（搜索是临时查询，不是视图内容）。
    pub(super) fn apply_saved_view(&mut self, filters: NavFilters, cx: &mut Context<Self>) {
        {
            let mut view = self.nav.borrow_mut();
            view.source_filter = filters.source.as_deref().and_then(NavSource::from_key);
            view.type_filter = filters.db_type;
            view.driver_filter = filters.driver;
            view.tag_filter = filters.tag;
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
            .flat_map(|v| v.iter().map(|t| t.name.clone()))
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
    ///
    /// 输入**防抖**（竞品对齐，见 `ui::NAV_SEARCH_DEBOUNCE_MS` 注）：排程只做
    /// 签名判变与「搜索中」状态置位，真正发任务放到防抖到期后（[`Self::nav_run_index_search`]）
    /// ——快速连击在窗口内被合并，不会每键发一条索引搜索。
    pub(super) fn nav_schedule_index_search(&self, filter: &str, cx: &mut Context<Self>) {
        let query = filter.trim().to_string();
        // 搜索签名 =（词, 正则, 大小写, 仅已连接）：词没变但开关变了也要重搜
        // （`Aa` / `.*` / 「已连接」toggle 切换时 query 相同，旧 `search_query` 判断会误判“没变”）。
        let (regex, case_sensitive, connected_only) = {
            let view = self.nav.borrow();
            (
                view.search_regex,
                view.search_case_sensitive,
                view.search_connected_only,
            )
        };
        let started = {
            let mut view = self.nav.borrow_mut();
            let sig = (query.clone(), regex, case_sensitive, connected_only);
            if !nav_search_query_ready(&query) {
                view.search_query = None;
                view.search_sig = None;
                view.search_hits.clear();
                view.search_searched = 0;
                false
            } else if view.search_sig.as_ref() == Some(&sig) {
                // 词与开关都没变：不重搜（结果已经在状态里）。
                false
            } else {
                view.search_query = Some(query.clone());
                view.search_sig = Some(sig);
                view.search_hits.clear();
                view.search_searched = 0;
                true
            }
        };
        if !started {
            return;
        }
        // 防抖：窗口内还有新输入就把上一枚任务丢掉重新计时（`Task` drop = 取消）。
        // 到期后才真正发任务（[`Self::nav_run_index_search`] 从状态现取词 / 筛选 /
        // 连接——窗口内 chips 变了也吃最新）；序号在发任务时递增，连续输入只发一次。
        let executor = cx.background_executor().clone();
        let weak = cx.entity().downgrade();
        let task = cx.spawn(async move |_this, cx| {
            executor
                .timer(std::time::Duration::from_millis(ui::NAV_SEARCH_DEBOUNCE_MS))
                .await;
            let _ = weak.update(cx, |this, cx| this.nav_run_index_search(cx));
        });
        *self.search_debounce.borrow_mut() = Some(task);
    }

    /// 防抖到期真正发起的索引搜索：从状态现取词 / 筛选 / 连接，递增序号后入队。
    ///
    /// 为什么排程时不把 targets 组装好带进闭包：防抖窗口内用户可能又改了 chips /
    /// 连接列表（排程时拍下的快照会过期），到期时现取才是用户看到的当前筛选。
    fn nav_run_index_search(&self, cx: &mut Context<Self>) {
        let (query, case_sensitive, regex, connected_only) = {
            let view = self.nav.borrow();
            (
                view.search_query.clone().unwrap_or_default(),
                view.search_regex,
                view.search_case_sensitive,
                view.search_connected_only,
            )
        };
        if !nav_search_query_ready(&query) {
            return;
        }
        let conn_filter =
            NavConnFilter::snapshot(&self.nav.borrow(), self.nav.borrow().source_filter);
        let conns = self.host.connections();
        let tags = self.nav.borrow().tags.clone();
        let targets: Vec<nav_jobs::SearchTarget> = conns
            .iter()
            // 「仅已连接」开关：只搜当前已连接（索引缓存存在但断开的连接不搜——
            // 命中行点开还要建连，开关语义 = 用户只关心现在连着的库）。
            .filter(|c| {
                (!connected_only || self.host.is_connected(&c.id))
                    && self.nav_conn_passes_facets(&conn_filter, c, &tags)
            })
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
        let seq = {
            let mut view = self.nav.borrow_mut();
            view.search_seq = view.search_seq.wrapping_add(1);
            view.search_seq
        };
        nav_jobs::enqueue_search(
            nav_jobs::SearchConsumer::Navigator,
            seq,
            nav_jobs::SearchKind::Name,
            &query,
            case_sensitive,
            regex,
            root.as_deref(),
            targets,
        );
        self.ensure_nav_pump(cx);
    }

    /// 重排一次索引搜索（搜索开关 `.*` / `Aa` 切换用）：词与连接筛选照当前状态现取。
    ///
    /// 为什么不能只靠 `InputEvent::Change`：toggle 点按不发输入事件，而词本身没变时
    /// [`Self::nav_schedule_index_search`] 靠**签名**（词, 正则, 大小写）判变——所以
    /// 开关翻转后必须主动排一次，否则结果集永远是旧开关下的。
    pub(super) fn rerun_nav_search(&self, cx: &mut Context<Self>) {
        let filter = self
            .nav_search
            .as_ref()
            .map(|s| s.read(cx).value().to_string())
            .unwrap_or_default();
        self.nav_schedule_index_search(&filter, cx);
    }

    /// 展开 / 折叠节点；首次展开（或已展开未加载）时排队后台懒加载，并持久化展开态。
    ///
    /// 三态语义（对齐 DBeaver「点开才加载」）：
    /// - 未展开 → 展开 + 加载本层；
    /// - 已展开且已加载 / 加载中 → 收起（清 loading/attempted 防残留与幂等挡死）；
    /// - **已展开但未加载**（跨重启恢复的展开态：树显示展开、内容未拉取）→ 本次点击
    ///   = 加载本层，**不收起**——否则恢复展开态后点箭头只会收起，永远拉不出内容。
    pub(super) fn toggle_nav_node(
        &mut self,
        conn_id: &str,
        key: &str,
        path: NavPath,
        cx: &mut Context<Self>,
    ) {
        let do_load = {
            let mut view = self.nav.borrow_mut();
            if view.expanded.contains(key) {
                let loaded_or_loading = view.children.contains_key(key) || view.loading.contains(key);
                if loaded_or_loading {
                    // 收起 = 取消本次加载意图：清 loading/attempted 防残留与幂等挡死。
                    // 残留场景（真机复现）：加载中收起该节点，loading 不消失；再展开时
                    // attempted 还在 → ensure_nav_loaded 被幂等挡掉 → 树永久「加载中」。
                    // 保留已加载的 children 与 errors（重展开直接显示，失败节点走重试）。
                    view.expanded.remove(key);
                    view.loading.remove(key);
                    view.attempted.remove(key);
                    false
                } else {
                    // 已展开未加载 → 本次点击 = 加载（保持展开态）
                    true
                }
            } else {
                view.expanded.insert(key.to_string());
                true
            }
        };
        if do_load {
            // 上次失败（红字挂在树上、手里没有子节点）：这次展开就是**重试**。
            // 判据与「点红字」共用（`nav_load_is_retry`），只是入口不同——
            // 少了这一支，`attempted` 会把重试一并拦掉，红字只能靠右键刷新才动。
            let retry = {
                let view = self.nav.borrow();
                nav_load_is_retry(&view, key)
            };
            if retry {
                self.retry_nav_node(conn_id, key, path, cx);
                self.mark_nav_state_dirty(conn_id, cx);
                return;
            }
            // 连接根展开：未建连则先建连。否则 `NavigatorService` → `MetadataService`
            // 取不到运行时句柄，冒泡为 `[CONN_NOT_FOUND]`（用户看到的“连不上”）。
            if matches!(path, NavPath::Connection) && !self.ensure_connected_for_browse(conn_id, cx)
            {
                self.mark_nav_state_dirty(conn_id, cx);
                return;
            }
            // 「已展开未加载」的恢复态点击：attempted 可能残留上次会话/中断痕迹，
            // 不清会被 `ensure_nav_loaded` 幂等挡掉 → 点了没反应（真机：恢复展开态
            // 后点 schema 箭头毫无变化）。
            {
                let mut view = self.nav.borrow_mut();
                view.attempted.remove(key);
            }
            self.ensure_nav_loaded(conn_id, key, path, false, cx);
        }
        self.mark_nav_state_dirty(conn_id, cx);
    }

    /// 展开前的隐式建连：未连接时先**调度**建连（后台线程执行，见
    /// `workbench::services::nav_runtime::connect_entry`）；返回是否可用。
    ///
    /// 2026-10-03 起为**乐观语义**：`Ok` 表示「建连请求已派发」——立即标记已连接并返回
    /// `true`，真正的连接结果（成功 → 预热重载；失败 → 提示 + 回滚标记）由
    /// [`Self::apply_connect_results`] 在轮询泵里回填。此前在 UI 主线程同步建连，
    /// 不可达主机（mysql / PG）会冻结窗口 30s+（“无响应”）。
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
        match self.host.connect(conn_id) {
            Ok(()) => {
                {
                    let mut view = self.nav.borrow_mut();
                    view.connected.insert(conn_id.to_string());
                    view.prefetched.clear();
                    // 清掉上一次的失败痕迹（errors/attempted/loading），以便重试加载；
                    // 真机复现：失败过的连接根被 attempted 挡住后树永不加载。
                    nav_forget_attempt(&mut view, conn_id);
                }
                true
            }
            Err(e) => {
                self.host.notice(format!("连接失败: {e}"), cx);
                false
            }
        }
    }

    /// 回填后台建连结果（主线程，轮询泵调用）。
    ///
    /// 成功 → 标记已连接（幂等）+ 清负载错误 + 提交预热并启动进度轮询；
    /// 失败 → 回滚乐观标记 + 面板提示。
    pub(super) fn apply_connect_results(
        &mut self,
        results: Vec<nav_jobs::ConnectResult>,
        cx: &mut Context<Self>,
    ) {
        for r in results {
            tracing::info!(conn_id = %r.conn_id, ok = r.ok, msg = %r.message, "【探针】apply_connect_results 收到结果");
            if r.ok {
                // 先判展开态与预连标记再改状态：`ensure_nav_loaded` 需要 `&self`，不能与
                // `borrow_mut` 同时存活。
                let (conn_expanded, was_prewarm) = {
                    let view = self.nav.borrow();
                    (
                        view.expanded.contains(&r.conn_id),
                        view.prewarming.contains(&r.conn_id),
                    )
                };
                {
                    let mut view = self.nav.borrow_mut();
                    view.connected.insert(r.conn_id.clone());
                    view.prewarming.remove(&r.conn_id);
                    view.prefetched.clear();
                    // 连接成功 = 「最近使用」：刷新会话内排序缓存（跨会话由最近记录
                    // JSON 持久化恢复；`note_connection_used` 是事件路径 I/O，低频可接受）。
                    view.recent.retain(|id| id != &r.conn_id);
                    view.recent.insert(0, r.conn_id.clone());
                    // 连接成功 = 新的事实：清掉该连接**全部**节点的 attempted/errors/loading
                    // 痕迹（不止连接根）。此前连接未注册时，子层（schema 等）在渲染驱动的
                    // 渐进加载里吃到 CONN_NOT_FOUND / 排队超时，错误留在 attempted 里，
                    // 连接就绪后渲染驱动不会再重试 → 树永远「加载中 / 红字」（真机：public
                    // schema 30s 超时后不恢复）。清掉后渲染驱动对已展开节点重新排队，
                    // 此时连接已就绪、L1/L2 缓存命中，秒出。
                    let prefix = format!("{}/", r.conn_id);
                    let stale: Vec<String> = view
                        .attempted
                        .iter()
                        .filter(|k| k.as_str() == r.conn_id || k.starts_with(&prefix))
                        .cloned()
                        .collect();
                    for k in stale {
                        nav_forget_attempt(&mut view, &k);
                    }
                }
                self.host.note_connection_used(&r.conn_id);
                // 建连是后台调度式（修 UI 卡死）：展开动作发生在连接注册进
                // `ConnectionManager` 之前，会先吃一次 `CONN_NOT_FOUND` 失败。
                // 连接就绪后若不自动重载，树就一直空着——用户得收起再展开一次。
                // 这里把该连接**已展开的连接根**重新入队（cache-aside，非 fresh），
                // 子层节点由渲染时的 `ensure_nav_loaded` 渐进补齐。
                //
                // **预连例外**：启动预连成功后**不**自动展开加载——连接已就绪，
                // 用户展开时走 L1/L2 秒开；自动加载会让恢复的展开态全树并发回源，
                // 压垮连接池（真机见过 `pool timed out` / 30s 加载超时）。
                if conn_expanded && !was_prewarm {
                    self.ensure_nav_loaded(&r.conn_id, &r.conn_id, NavPath::Connection, false, cx);
                }
                // **连接成功不自动预热**（对齐 DBeaver「点哪个展开哪个」：下一层由点击驱动）：
                // 展开连接根时 cache-aside 会实时内省并写 L2（等于把预热推迟到用户真正查看时，
                // 且每次连接只预热被展开的 schema，不再整连接逐 schema 回源）。此前「手动连接
                // 成功即 warm_schemas 整连接」是「没点击就加载下一层」的来源——它长时间占住
                // 连接池（真机见过 `pool timed out` / 30s 加载超时），收益却被 L2 覆盖。
                self.ensure_warm_poll(cx);
            } else {
                let mut view = self.nav.borrow_mut();
                view.connected.remove(&r.conn_id);
                view.prewarming.remove(&r.conn_id);
                drop(view);
                self.host.notice(format!("连接失败: {}", r.message), cx);
            }
            cx.notify();
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

    /// 重试某个节点的加载（点那行红字，或把一个失败的节点收起再展开）。
    ///
    /// 与「右键 → 刷新元数据」不同：那个是**新鲜取数**（跳过 L2 并重写），
    /// 这个是**照常取数**（cache-aside）——上次多半什么都没查到，缓存里本就没有它的行；
    /// 而查询本身成功过的那部分（如表清单拿到了、视图那类失败）应当照旧命中缓存，不白跑。
    pub(super) fn retry_nav_node(
        &mut self,
        conn_id: &str,
        key: &str,
        path: NavPath,
        cx: &mut Context<Self>,
    ) {
        // 连接根：失败原因可能就是「还没连上」，先补建连——否则重试只是把同一个错再报一遍。
        if matches!(path, NavPath::Connection) && !self.ensure_connected_for_browse(conn_id, cx) {
            return;
        }
        {
            let mut view = self.nav.borrow_mut();
            nav_forget_attempt(&mut view, key);
        }
        self.ensure_nav_loaded(conn_id, key, path, false, cx);
        cx.notify();
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
                // 序号守卫：这不是最新一批搜索的结果（同词重跑 / toggle 重排时旧批
                // 迟到，`query` 判据区分不了先后）→ 整批丢弃。
                if r.seq != view.search_seq {
                    continue;
                }
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
                    tracing::info!(key = %conn_key, "【探针】expanded.insert(pump_reveal ① 连接)");
                    // 定位途中展开的链路也要落库（否则重启后收回去，跟“定位完停在那里”不一致）。
                    self.mark_nav_state_dirty(&target.conn_id, cx);
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
            tracing::info!(key = %key, "【探针】expanded.insert(pump_reveal ② 逐层)");
            // 定位推进的每一拍都标脏：函数中间会提前 return，标注跟着插入走才不会漏。
            self.mark_nav_state_dirty(&target.conn_id, cx);
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
            {
                let mut view = self.nav.borrow_mut();
                view.reveal = None;
                view.reveal_note = None;
            }
            // 选中走唯一入口：定位结果也进面板级状态的落库（重启后回到这条）。
            self.set_nav_selected(Some(target_key.clone()), cx);
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
                // 导航建连（后台线程执行，2026-10-03）：结果回填——成功 → 预热，失败 → 回滚 + 提示。
                let connect_results = nav_jobs::drain_connect_results();
                if !connect_results.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_connect_results(connect_results, cx))
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
                    && !nav_jobs::has_pending_connect()
                    && !nav_jobs::has_pending_search(nav_jobs::SearchConsumer::Navigator);
                if idle {
                    // 多等一拍确认没有新任务（render 可能刚入队）。
                    executor.timer(std::time::Duration::from_millis(120)).await;
                    if !nav_jobs::has_pending_loads()
                        && !nav_jobs::has_pending_sql()
                        && !nav_jobs::has_pending_test()
                        && !nav_jobs::has_pending_connect()
                        && !nav_jobs::has_pending_search(nav_jobs::SearchConsumer::Navigator)
                    {
                        // 退出前最后消费一次：job 可能在「多等一拍」期间入队并完成，
                        // 结果滞留会让 loading 永久残留（真机复现：连接预热重载的 Connection
                        // 层在 idle 判定后完成，pump 已 break，树永远「加载中」）。
                        let last = nav_jobs::drain_load_results();
                        if !last.is_empty() {
                            if weak
                                .update(cx, |this, cx| this.apply_load_results(last, cx))
                                .is_err()
                            {
                                return;
                            }
                            continue;
                        }
                        // 确无滞留：清掉残留 loading（idle 时 loading 不应存在）。
                        // 失败路径已由 apply 清；这里兜底结果滞留 / 竞态导致的残留，
                        // 避免「加载中」字样挂在树上等不到清除。
                        weak.update(cx, |this, _| {
                            this.nav.borrow_mut().loading.clear();
                        })
                        .ok();
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
            // 连接类错误 → 自动重试一次（重建连接握手 ~10-30ms，用户无感）。
            // 背景（app_run57/58 铁证）：池连接被网络层静默丢弃后，acquire 验证
            // 挂在 TCP 半开上超时——失败后清 attempted 重入队，重试时 sqlx 已把
            // 死连接回收并新建，第二次秒过。非连接类错误（权限/对象不存在）不重试。
            let conn_error = nav_error_is_connection(&r.result);
            let auto_retry = conn_error && {
                let view = self.nav.borrow();
                !view.retried.contains(&key)
            };
            if auto_retry {
                {
                    let mut view = self.nav.borrow_mut();
                    view.retried.insert(key.clone());
                    // 不落红字（这次失败归因于连接，用户无感）；清 attempted 允许重入队。
                    nav_forget_attempt(&mut view, &key);
                }
                tracing::warn!(
                    conn_id = %conn_id,
                    key = %key,
                    path = ?r.path,
                    error = %r.result.as_ref().err().map(|s| s.as_str()).unwrap_or(""),
                    "导航加载连接类失败（自动重试一次）"
                );
                self.ensure_nav_loaded(&conn_id, &key, r.path.clone(), false, cx);
                continue;
            }
            {
                let mut view = self.nav.borrow_mut();
                view.loading.remove(&key);
                match r.result {
                    Ok(page) => {
                        view.errors.remove(&key);
                        // 重试成功后清除重试标记：下次同类失败仍可再自动重试一次。
                        view.retried.remove(&key);
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
                tracing::info!(key = %key, "【探针】expanded.insert(ensure_nav_state_loaded 恢复)");
                view.expanded.insert(key);
            }
        }
        view.state_loaded.insert(conn_id.to_string());
    }

    /// 启动预连（对齐 DBeaver/DataGrip 的「连接常驻」）：对**上次展开过**的连接根，
    /// 若运行时未连接则后台发起建连，成功后由 `apply_connect_results` 标记已连接。
    ///
    /// 为什么值得做：展开态跨重启恢复，但运行时连接不跨重启——首次展开要现场建连
    /// （MySQL/PG 实测 6~7s），用户感知「树出来慢」。预连把建连挪到启动后台消化，
    /// 用户展开时连接已就绪，对象树直接走 L1/L2 缓存（毫秒级）。
    ///
    /// **只预连一个（上限 1）**：启动把「上次展开过的」全部连上是过度行为——用户
    /// 明确「我只点了 PG，其他的也启动了」。DBeaver/DataGrip 语义是**点哪个连哪个**：
    /// 预连只挑一个（按连接列表顺序的第一个未连接展开根，即用户可见顺序里的最近操作位），
    /// 其余连接在用户展开时现场建连（池内复用，不额外抢服务端连接数）。
    ///
    /// 触发点：`chrome.rs` 首次渲染的 `defer_in`（每个连接只跑一次，天然是「启动预连」）。
    /// 静默语义：失败只留日志（不弹提示、不阻塞 UI），用户手动展开仍会走原有建连路径。
    pub(super) fn prewarm_expanded_connections(&self) {
        // 展开根的集合（连接 id 去重），按连接列表顺序——取前几个做预连。
        let root_set: std::collections::HashSet<String> = {
            let view = self.nav.borrow();
            view.expanded
                .iter()
                .map(|key| key.split('/').next().unwrap_or(key).to_string())
                .collect()
        };
        let ordered = self.host.connections();
        // 最多试 3 个展开过的连接（失败换下一个，直到有一个成功或全失败）。
        // 为何失败续试：预连选的第一个可能是配置失效的共享连接（真机：G_conn 认证配置
        // 缺失，建连失败），不续试就等于没预连。续试保证至少一个常用连接在启动后台就绪。
        let candidates: Vec<String> = ordered
            .iter()
            .map(|c| c.id.clone())
            .filter(|id| root_set.contains(id))
            .take(3)
            .collect();
        let mut attempted_any = false;
        for conn_id in candidates {
            let already =
                self.host.is_connected(&conn_id) || self.nav.borrow().connected.contains(&conn_id);
            if already {
                continue;
            }
            tracing::info!(conn_id = %conn_id, "启动预连（最多 3 个）：后台建连上次展开的连接");
            // 登记为「预连中」：成功后只标记已连接，不自动展开加载
            // （见 `NavViewState::prewarming` 的注释：全树并发回源会压垮连接池）。
            self.nav.borrow_mut().prewarming.insert(conn_id.clone());
            if let Err(e) = self.host.connect(&conn_id) {
                // 静默：预连失败不打扰用户（展开时仍会自动建连）；换下一个候选。
                self.nav.borrow_mut().prewarming.remove(&conn_id);
                tracing::warn!(conn_id = %conn_id, error = %e, "启动预连失败（换下一个候选）");
                continue;
            }
            attempted_any = true;
            break;
        }
        let _ = attempted_any;
    }

    /// 持久化某连接的展开态（**防抖**：攒到 `ui::NAV_STATE_SAVE_DEBOUNCE_MS` 无新变更再写）。
    ///
    /// 为何不在这里直接写库：展开 / 折叠是高频动作，而落库是 UI 线程上的 SQLite 写；
    /// 一次性批量落库（含面板级状态）在 [`Self::flush_nav_state`]。
    pub(super) fn mark_nav_state_dirty(&self, conn_id: &str, cx: &mut Context<Self>) {
        self.state_dirty
            .borrow_mut()
            .conns
            .insert(conn_id.to_string());
        self.schedule_nav_state_save(cx);
    }

    /// 记一次「面板级状态脏了」（选中 / 搜索词）；同样防抖。
    pub(super) fn mark_panel_state_dirty(&self, cx: &mut Context<Self>) {
        self.state_dirty.borrow_mut().panel = true;
        self.schedule_nav_state_save(cx);
    }

    /// 写选中（**全树唯一入口**：点击 / 键盘漫游 / 定位都走它）。
    ///
    /// 抽出来的两个理由：① 落库（搜索行那种瞬时键不写，见 [`nav_selection_is_persistable`]）；
    /// ② 只有一个地方记「选中变了」——旧写法是五处各自 `borrow_mut().selected_key = ...`。
    pub(super) fn set_nav_selected(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        let changed = {
            let mut view = self.nav.borrow_mut();
            if view.selected_key == key {
                false
            } else {
                view.selected_key = key;
                true
            }
        };
        if changed {
            self.mark_panel_state_dirty(cx);
        }
    }

    /// 面板级状态（选中 / 搜索词）的一次性恢复（首帧的 `defer_in` 调，需窗口才能填搜索框）。
    ///
    /// 恢复选中后**请它滚进视口**——这就是「滚动位置」的恢复方式（行集合要等懒加载回来
    /// 才齐，存像素偏移对不上；「我选中的那条」既稳又是用户记忆里的位置）。
    /// 行还没到时由已有的跨帧重试（`nav_pending_scroll`）兑现。
    pub(super) fn ensure_panel_state_loaded(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.nav.borrow().panel_state_loaded {
            return;
        }
        self.nav.borrow_mut().panel_state_loaded = true;
        let Some(state) = crate::nav_store::load_panel_state(self.host.project_root().as_deref())
        else {
            return;
        };
        if let Some(key) = state.selected_key.clone().filter(|k| !k.is_empty()) {
            self.nav.borrow_mut().selected_key = Some(key.clone());
            self.nav_pending_scroll = Some(key);
        }
        if !state.filter_text.is_empty() {
            // 恢复搜索开关（`Aa` / `.*` 随面板状态持久化；词空时开关也恢复——
            // 否则「开过正则又清空词」下次打开 toggle 回到关）。
            {
                let mut view = self.nav.borrow_mut();
                view.search_regex = state.search_regex;
                view.search_case_sensitive = state.search_case_sensitive;
            }
            if let Some(input) = self.nav_search.clone() {
                // 恢复回填：`set_value` 是程序化写值，**不发** `InputEvent::Change`
                //（gpui-base `input/base/state.rs` 内部临时关事件），订阅里的搜索排程
                // 不会被触发——所以这里**显式排一次**，否则恢复的搜索词永远等不到命中行；
                // 下一帧 `render_nav` 自会把本地过滤词重算成它。
                input.update(cx, |s, cx| {
                    s.set_value(state.filter_text.clone(), window, cx)
                });
                let filter = state.filter_text.clone();
                self.nav_schedule_index_search(&filter, cx);
            }
        }
        cx.notify();
    }

    /// 防抖排程：窗口内还有新变更就把上一枚任务丢掉（`Task` drop = 取消）重新计时。
    fn schedule_nav_state_save(&self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        let weak = cx.entity().downgrade();
        let task = cx.spawn(async move |_this, cx| {
            executor
                .timer(std::time::Duration::from_millis(
                    ui::NAV_STATE_SAVE_DEBOUNCE_MS,
                ))
                .await;
            let _ = weak.update(cx, |this, cx| this.flush_nav_state(cx));
        });
        *self.state_save.borrow_mut() = Some(task);
    }

    /// 把攒下的脏状态**一次写库**（防抖到期；需要立刻落时也可以直接调）。
    ///
    /// 两类写入：① 展开态按连接一行；② 面板级（选中 / 搜索词）一行。
    /// 两者共用 `navigator_state` 表，但**不写对方的字段**（否则写展开态会把选中抹掉——
    /// 旧实现 `save_nav_state_for` 用 `..Default::default()` 正是这个问题）。
    pub(super) fn flush_nav_state(&self, cx: &mut Context<Self>) {
        let (conns, panel) = {
            let mut dirty = self.state_dirty.borrow_mut();
            (
                std::mem::take(&mut dirty.conns),
                std::mem::replace(&mut dirty.panel, false),
            )
        };
        if conns.is_empty() && !panel {
            return;
        }
        let root = self.host.project_root();
        let root = root.as_deref();
        for conn_id in &conns {
            let state = crate::model::NavState {
                expanded_keys: self.nav_expanded_keys_for(conn_id),
                ..Default::default()
            };
            let _ = crate::nav_store::save_nav_state(conn_id, root, &state);
        }
        if panel {
            let selected = self.nav.borrow().selected_key.clone();
            let selected = selected.filter(|k| nav_selection_is_persistable(k));
            let filter_text = self
                .nav_search
                .as_ref()
                .map(|s| s.read(cx).value().to_string())
                .unwrap_or_default();
            let state = crate::model::NavState {
                selected_key: selected,
                filter_text,
                search_regex: self.nav.borrow().search_regex,
                search_case_sensitive: self.nav.borrow().search_case_sensitive,
                ..Default::default()
            };
            let _ = crate::nav_store::save_panel_state(root, &state);
        }
    }

    /// 某连接当前的展开键（持久化用；只收它自己子树的键）。
    fn nav_expanded_keys_for(&self, conn_id: &str) -> Vec<String> {
        let prefix = format!("{conn_id}/");
        let view = self.nav.borrow();
        view.expanded
            .iter()
            .filter(|k| *k == conn_id || k.starts_with(&prefix))
            .cloned()
            .collect()
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
        let recent = view.recent.clone();
        nav_order_members(&stored, |id| {
            conns
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.to_string())
        }, &recent)
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
        // 最近使用顺序（新→旧）：组内与未分组容器共用（手动排序优先原则不受影响）。
        let recent = self.host.recent_connection_ids();
        for group in &groups {
            let stored = crate::nav_store::list_group_members_detailed(root.as_deref(), &group.id);
            let ids = nav_order_members(&stored, |id| {
                names.get(id).cloned().unwrap_or_else(|| id.to_string())
            }, &recent);
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
        view.recent = recent;
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
    ///
    /// 2026-10-03 起连接分支为**乐观语义**：`host.connect` 只派发后台建连（立即返回
    /// `Ok`），这里先标记已连接；真正的成败由 [`Self::apply_connect_results`] 回填
    /// （成功 → 预热；失败 → 回滚标记 + 提示）。断开仍是本地操作，同步处理。
    pub(super) fn toggle_connection(
        &mut self,
        conn_id: &str,
        project_root: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(conn_id = %conn_id, "【探针】toggle_connection 被调用");
        // 建连已改后台调度：项目根只用于 apply_connect_results 里的预热（从 host 现取），
        // 本入口不再直接使用；保留参数仅为调用方签名稳定。
        let _ = project_root;
        let already =
            self.host.is_connected(conn_id) || self.nav.borrow().connected.contains(conn_id);
        tracing::info!(conn_id = %conn_id, already, "【探针】already 判定");
        let outcome = if already {
            self.host.disconnect(conn_id).map(|_| false)
        } else {
            self.host.connect(conn_id).map(|_| true)
        };
        tracing::info!(conn_id = %conn_id, is_ok = outcome.is_ok(), "【探针】connect/disconnect 返回");
        match outcome {
            Ok(is_connected) => {
                let mut view = self.nav.borrow_mut();
                if is_connected {
                    view.connected.insert(conn_id.to_string());
                    // 刷新模式下预取过的标记清空（重新连接后重新预取）。
                    view.prefetched.clear();
                    // 连接成功 → 允许连接根重新加载（与 apply_connect_results 同一卡死修复）。
                    nav_forget_attempt(&mut view, conn_id);
                } else {
                    view.connected.remove(conn_id);
                    // 断开 = 树收起来（缓存保留，重连后按需重载）。清掉该连接的
                    // 展开态 / 子节点 / 错误痕 / 预取标记——树收起，不留下半截子树。
                    let prefix = format!("{conn_id}/");
                    view.expanded.retain(|k| k != conn_id && !k.starts_with(&prefix));
                    // HashMap 的 retain 闭包是 (key, value) 两参。
                    view.children.retain(|k, _| k != conn_id && !k.starts_with(&prefix));
                    view.errors.retain(|k, _| k != conn_id && !k.starts_with(&prefix));
                    view.child_total.retain(|k, _| k != conn_id && !k.starts_with(&prefix));
                    // loading / prefetched 是 HashSet，retain 闭包只收一个参数。
                    view.loading.retain(|k| k != conn_id && !k.starts_with(&prefix));
                    view.prefetched.retain(|k| k != conn_id && !k.starts_with(&prefix));
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
