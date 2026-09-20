//! 状态变更与后台接线：加载 / 懒加载 / 监控轮询 / 冲突检测 / 编辑提交 / 删除与撤销 /
//! 剪贴板 / 外部引用 / 打开位置。
//!
//! 纪律：**渲染期零 I/O**——本模块的方法只入队后台任务（[`crate::jobs`]）并回填结果；
//! 少数单次系统调用（新建 / 重命名 / 引用增删改，微秒~毫秒级）保持同步回填（判据见
//! `scratchpad-architecture.md` §13.1 K1c）。

use super::*;

impl ScratchpadView {
    /// 请求重载草稿箱（渲染与事件路径共用）：**只入队 + 起轮询，不做 I/O**。
    ///
    /// 实际读盘在 `scratchpad_jobs` 的工作线程；结果由 [`Self::apply_scratchpad_loads`] 回填。
    /// 重载时保留已展开子目录（在后台重新拉取），避免「操作后展开态看起来空了」。
    pub(super) fn request_scratchpad_load(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.host.project_root() else {
            // 项目关闭：停掉监控（不再关心旧项目的目录事件）。
            self.scratchpad_watch = None;
            let mut view = self.scratchpad.borrow_mut();
            view.loaded = true;
            view.loading = false;
            // 失效在途结果：项目已关闭，旧项目的加载结果不得回填。
            view.load_seq = scratchpad_jobs::invalidate_loads();
            view.error = Some("未打开项目：草稿箱根即项目目录，请先打开项目。".to_string());
            view.entries.clear();
            view.children.clear();
            view.external_refs.clear();
            view.trash.clear();
            return;
        };
        self.ensure_scratchpad_watch(&root, cx);
        let parents: Vec<String> = self.scratchpad.borrow().children.keys().cloned().collect();
        let seq = scratchpad_jobs::enqueue_root_load(&root, parents);
        // 本次重拉已覆盖“此刻之前的全部改动”（含我们自己刚写的文件），
        // 清掉标记避免紧接着再来一次多余的重拉。
        if let Some(watcher) = &self.scratchpad_watch {
            let _ = watcher.take_changed();
        }
        {
            let mut view = self.scratchpad.borrow_mut();
            // `loaded` = 已受理本次请求（防渲染帧重复入队）；加载中状态另行标记。
            view.loaded = true;
            view.loading = true;
            view.load_seq = seq;
            view.error = None;
        }
        self.ensure_scratchpad_pump(cx);
    }

    /// 确保草稿箱目录监控在跑（项目根变化时换监控点）。
    ///
    /// 注册监控是一次轻量 OS 调用（不读盘），且只在「项目根首次出现或变化」时发生，
    /// 因此放在请求加载路径（不在渲染循环里反复执行）。
    pub(super) fn ensure_scratchpad_watch(
        &mut self,
        root: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let dir = root.join(crate::MODULE_DIR_NAME);
        if self
            .scratchpad_watch
            .as_ref()
            .map(|w| w.dir() == dir.as_path())
            .unwrap_or(false)
        {
            return;
        }
        match crate::ScratchpadWatcher::start(dir) {
            Ok(watcher) => self.scratchpad_watch = Some(watcher),
            Err(e) => {
                // 监控失败不影响使用（只是不能自动刷新）：降级为手动 `↻`。
                tracing::warn!("[Scratchpad] 目录监控启动失败，将退化为手动刷新: {e}");
                self.scratchpad_watch = None;
                return;
            }
        }
        self.ensure_scratchpad_watch_poll(cx);
    }

    /// 刷新「编辑器脏文档」缓存；有变化返回 `true`（调用方据此重绘）。
    ///
    /// 脏点的判据在编辑器手里（哪些文档有未保存修改），草稿箱只经宿主端口要一份
    /// 绝对路径集合——不依赖 `editor` crate。与目录监控同拍调用，不在 `render` 里碰宿主。
    pub(super) fn refresh_dirty_cache(&self) -> bool {
        let now: HashSet<String> = self
            .host
            .dirty_files()
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let mut cache = self.dirty_seen.borrow_mut();
        if **cache == now {
            return false;
        }
        *cache = Rc::new(now);
        true
    }

    /// C-4：外部改动之后，把「编辑器里有未保存修改」的草稿标成冲突，并在后台算内容差异。
    ///
    /// 判据是**内容**而不是 mtime：`diff_with_content` 全是 `Unchanged` 就说明不是真冲突
    /// （外部改动可能已被写回，或者就是我们自己写的）。返回是否发起了差异计算
    /// （调用方据此确保轮询在跑）。
    pub(super) fn detect_scratchpad_conflicts(&mut self) -> bool {
        let Some(root) = self.host.project_root() else {
            return false;
        };
        let dirty = self.host.dirty_files();
        if dirty.is_empty() {
            return false;
        }
        let store = ScratchpadStore::new(root.clone());
        // 先在只读借用里算出新冲突（+ 要拿给后台任务算差异的缓冲区内容）。
        let mut fresh: Vec<(ScratchpadConflict, String)> = Vec::new();
        {
            let view = self.scratchpad.borrow();
            for absolute in dirty {
                let Some(relative) = store.relative_path_of(&absolute) else {
                    continue;
                };
                // 已在冲突列表（差异算好了或还在算）就不重复入队。
                if view.conflicts.iter().any(|c| c.relative == relative) {
                    continue;
                }
                let Some(content) = self.host.draft_content(&absolute) else {
                    continue;
                };
                fresh.push((
                    ScratchpadConflict {
                        relative,
                        absolute,
                        diff: None,
                    },
                    content,
                ));
            }
        }
        if fresh.is_empty() {
            return false;
        }
        let mut view = self.scratchpad.borrow_mut();
        for (conflict, content) in fresh {
            scratchpad_jobs::enqueue_diff(&root, &conflict.relative, content);
            view.conflicts.push(conflict);
        }
        true
    }

    /// C-4「查看差异」：把某份冲突的行级差异投给中央编辑区（差异还没算好就先提示）。
    pub(super) fn show_scratchpad_conflict_diff(
        &mut self,
        relative: String,
        cx: &mut Context<Self>,
    ) {
        let ready = {
            let view = self.scratchpad.borrow();
            view.conflicts
                .iter()
                .find(|c| c.relative == relative)
                .and_then(|c| c.diff.clone())
        };
        let Some(diff) = ready else {
            self.host.notice("差异正在计算…".to_string(), cx);
            return;
        };
        self.scratchpad.borrow_mut().shown_diff = Some(relative.clone());
        self.host.show_diff(
            Some(ScratchpadDiffView {
                relative_path: relative,
                diff,
            }),
            cx,
        );
    }

    /// C-4「照磁盘重载」：让编辑器用磁盘内容替掉缓冲区，冲突随之消解。
    pub(super) fn reload_scratchpad_conflict(
        &mut self,
        absolute: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        match self.host.reload_draft(&absolute) {
            Ok(()) => {
                let mut close = false;
                {
                    let mut view = self.scratchpad.borrow_mut();
                    if let Some(pos) = view.conflicts.iter().position(|c| c.absolute == absolute) {
                        let conflict = view.conflicts.remove(pos);
                        if view.shown_diff.as_deref() == Some(conflict.relative.as_str()) {
                            view.shown_diff = None;
                            close = true;
                        }
                    }
                }
                if close {
                    self.host.show_diff(None, cx);
                }
                self.host.notice("已按磁盘内容重载".to_string(), cx);
                cx.notify();
            }
            Err(e) => self.host.notice(format!("重载失败: {e}"), cx),
        }
    }

    /// C-4「忽略」：本次保留我的缓冲；下次磁盘再变才会重新报冲突。
    pub(super) fn ignore_scratchpad_conflict(&mut self, relative: String, cx: &mut Context<Self>) {
        let mut close = false;
        {
            let mut view = self.scratchpad.borrow_mut();
            view.conflicts.retain(|c| c.relative != relative);
            if view.shown_diff.as_deref() == Some(relative.as_str()) {
                view.shown_diff = None;
                close = true;
            }
        }
        if close {
            self.host.show_diff(None, cx);
        }
        cx.notify();
    }

    /// 启动监控轮询（常驻任务：每 ~1.2 s 看一次变更标记，有变化就重拉一次）。
    ///
    /// 轮询而非“事件驱动立即重拉”，是为了**去抖**：编辑器保存一次常触发多条 OS 事件，
    /// 立即刷新会把 UI 打成刷新循环。
    ///
    /// 同一拍里若结果面板还开着，还会用同一套查询/开关重跑一次搜索（K4）。
    pub(super) fn ensure_scratchpad_watch_poll(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.scratchpad_watch_poll.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(1200)).await;
                let action = weak.update(cx, |this, cx| {
                    // 脏点与目录监控同一拍：编辑器里存/改都会让这个集合变。
                    let dirty_changed = this.refresh_dirty_cache();
                    let changed = this
                        .scratchpad_watch
                        .as_ref()
                        .map(|w| w.take_changed())
                        .unwrap_or(false);
                    if !changed {
                        return dirty_changed;
                    }
                    // 正在内联编辑（新建/重命名）或已有加载在途：本次不打断，留给下一拍。
                    {
                        let view = this.scratchpad.borrow();
                        if view.edit.is_some() || view.loading {
                            return dirty_changed;
                        }
                    }
                    this.scratchpad.borrow_mut().loaded = false;
                    // C-4：外部改动 + 编辑器里有未保存修改 → 冲突（差异在后台算）。
                    if this.detect_scratchpad_conflicts() {
                        this.ensure_scratchpad_pump(cx);
                    }
                    // 结果面板若还开着，顺带重跑一次搜索：外部改动后旧的命中列表已是快照。
                    let pending_search = this.active_search.clone();
                    if let Some((query, is_regex, case_sensitive)) = pending_search {
                        if let Some(root) = this.host.project_root() {
                            scratchpad_jobs::enqueue_search(
                                &root,
                                &query,
                                case_sensitive,
                                is_regex,
                            );
                            this.ensure_scratchpad_pump(cx);
                        }
                    }
                    true
                });
                match action {
                    Ok(true) => {
                        if weak.update(cx, |_, cx| cx.notify()).is_err() {
                            return;
                        }
                    }
                    Ok(false) => {}
                    Err(_) => return,
                }
            }
        });
        *self.scratchpad_watch_poll.borrow_mut() = Some(task);
    }

    /// 启动草稿箱加载结果轮询（已有存活任务时不重复启动）。
    pub fn ensure_scratchpad_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.scratchpad_pump.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                let loads = scratchpad_jobs::drain_loads();
                if !loads.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_scratchpad_loads(loads, cx))
                        .is_err()
                {
                    return;
                }
                let dirs = scratchpad_jobs::drain_dirs();
                if !dirs.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_scratchpad_dirs(dirs, cx))
                        .is_err()
                {
                    return;
                }
                let ops = scratchpad_jobs::drain_ops();
                if !ops.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_scratchpad_ops(ops, cx))
                        .is_err()
                {
                    return;
                }
                if !scratchpad_jobs::has_pending() {
                    // 多等一拍确认没有新任务（render 可能刚入队）。
                    executor.timer(std::time::Duration::from_millis(120)).await;
                    if !scratchpad_jobs::has_pending() {
                        break;
                    }
                }
            }
        });
        *self.scratchpad_pump.borrow_mut() = Some(task);
    }

    /// 回填模块根加载结果（主线程）：丢弃过期序号，写入条目/引用/回收站/子目录缓存。
    pub(super) fn apply_scratchpad_loads(
        &mut self,
        results: Vec<scratchpad_jobs::LoadResult>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        {
            let mut view = self.scratchpad.borrow_mut();
            // 同一帧可能收到多份（连续重载）：只应用最新序号。
            let newest = results.iter().map(|r| r.seq).max().unwrap_or(0);
            if newest < view.load_seq {
                return;
            }
            for r in results.into_iter().filter(|r| r.seq == newest) {
                match r.entries {
                    Ok(entries) => {
                        view.entries = entries;
                        view.external_refs = r.refs;
                        view.trash = r.trash;
                        view.children = r.children.into_iter().collect();
                        view.error = None;
                    }
                    Err(e) => {
                        view.error = Some(format!("加载草稿箱失败: {e}"));
                    }
                }
                changed = true;
            }
            if changed {
                // 已应用最新序号（更晚的请求会走上面的 early return），加载态结束。
                view.loading = false;
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// 回填子目录懒加载结果（主线程）。
    pub(super) fn apply_scratchpad_dirs(
        &mut self,
        results: Vec<scratchpad_jobs::DirResult>,
        cx: &mut Context<Self>,
    ) {
        {
            let mut view = self.scratchpad.borrow_mut();
            for r in results {
                match r.result {
                    Ok(kids) => {
                        view.children.insert(r.parent, kids);
                    }
                    Err(e) => view.error = Some(format!("展开失败: {e}")),
                }
            }
        }
        cx.notify();
    }

    /// 回填写操作 / 搜索替换结果（主线程）。
    ///
    /// 文案、刷新与通知都在这里统一处理：任务层只回传「做了什么 + 成功/失败」。
    pub(super) fn apply_scratchpad_ops(
        &mut self,
        results: Vec<scratchpad_jobs::OpResult>,
        cx: &mut Context<Self>,
    ) {
        let mut reload = false;
        let mut notice: Option<String> = None;
        let mut error: Option<String> = None;
        let mut search_view: Option<Option<ScratchpadSearchView>> = None;
        let mut close_diff = false;

        for result in results {
            match result {
                scratchpad_jobs::OpResult::Import { outcome } => match outcome {
                    Ok(()) => {
                        reload = true;
                        notice = Some("已导入所选文件".to_string());
                    }
                    Err(e) => {
                        // 部分成功是可能的（逐个导入遇错即停），仍要刷新一次。
                        reload = true;
                        error = Some(format!("导入失败: {e}"));
                    }
                },
                scratchpad_jobs::OpResult::Paste { cut, outcome } => match outcome {
                    Ok(()) => {
                        reload = true;
                        if cut {
                            // 剪切粘贴成功才收起剪贴板（与同步版语义一致）。
                            self.scratchpad.borrow_mut().clipboard = None;
                        }
                        notice = Some("已粘贴".to_string());
                    }
                    Err(e) => {
                        reload = true;
                        error = Some(format!("粘贴失败: {e}"));
                    }
                },
                scratchpad_jobs::OpResult::EmptyTrash { outcome } => match outcome {
                    Ok(()) => {
                        reload = true;
                        self.scratchpad.borrow_mut().trash_expanded = false;
                        notice = Some("回收站已清空".to_string());
                    }
                    Err(e) => error = Some(format!("清空回收站失败: {e}")),
                },
                scratchpad_jobs::OpResult::Search {
                    query,
                    is_regex,
                    case_sensitive,
                    replaced,
                    outcome,
                } => match outcome {
                    Ok(payload) => {
                        if let Some((total, files)) = replaced {
                            notice = Some(format!("已替换 {total} 处（{files} 个文件）"));
                        }
                        search_view = Some(Some(search_view_from_payload(
                            query,
                            is_regex,
                            case_sensitive,
                            payload,
                        )));
                        error = None;
                    }
                    Err(e) => match replaced {
                        // 替换任务失败：提示走通知栏（结果栏保持旧内容）。
                        Some(_) => notice = Some(format!("替换失败: {e}")),
                        None => {
                            search_view = Some(None);
                            error = Some(format!("搜索失败: {e}"));
                        }
                    },
                },
                scratchpad_jobs::OpResult::Diff {
                    relative_path,
                    outcome,
                } => match outcome {
                    Ok(diff) => {
                        // 内容一致 → 不是真冲突（外部改动已被写回 / 就是自己写的），直接撤掉提示。
                        let real = diff.lines.iter().any(|l| l.kind != DiffLineKind::Unchanged);
                        let mut view = self.scratchpad.borrow_mut();
                        if real {
                            if let Some(slot) = view
                                .conflicts
                                .iter_mut()
                                .find(|c| c.relative == relative_path)
                            {
                                slot.diff = Some(diff);
                            }
                        } else {
                            view.conflicts.retain(|c| c.relative != relative_path);
                            if view.shown_diff.as_deref() == Some(relative_path.as_str()) {
                                view.shown_diff = None;
                                close_diff = true;
                            }
                        }
                    }
                    Err(e) => {
                        self.scratchpad
                            .borrow_mut()
                            .conflicts
                            .retain(|c| c.relative != relative_path);
                        error = Some(format!("差异计算失败: {e}"));
                    }
                },
            }
        }

        if close_diff {
            self.host.show_diff(None, cx);
        }

        {
            let mut view = self.scratchpad.borrow_mut();
            if reload {
                view.loaded = false;
                if error.is_none() {
                    view.error = None;
                }
            }
            if let Some(e) = error {
                view.error = Some(e);
            }
        }
        if let Some(view) = search_view {
            self.active_search = view
                .as_ref()
                .map(|v| (v.query.clone(), v.is_regex, v.case_sensitive));
            self.host.show_search_results(view, cx);
            self.host.notify_host(cx);
        }
        if let Some(text) = notice {
            self.host.notice(text, cx);
        }
        cx.notify();
    }

    /// 构建草稿箱存储 + 运行时（未打开项目时报错）。
    pub(super) fn scratchpad_store(
        &self,
    ) -> Result<(ScratchpadStore, tokio::runtime::Runtime), String> {
        let root = self
            .host
            .project_root()
            .ok_or_else(|| "未打开项目".to_string())?;
        let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
        Ok((ScratchpadStore::new(root), rt))
    }

    /// 懒创建草稿箱输入框（名称内联编辑 + 文件名过滤）并订阅事件。
    pub(super) fn ensure_scratchpad_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scratchpad.borrow().name_input.is_some() {
            return;
        }
        let name_input = cx.new(|cx| InputState::new(window, cx));
        let name_sub = cx.subscribe_in(
            &name_input,
            window,
            |this, _e, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.commit_scratchpad_edit(window, cx);
                }
            },
        );
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索…"));
        let search_sub = cx.subscribe_in(
            &search_input,
            window,
            |this, _e, ev: &InputEvent, _w, cx| match ev {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    let content_mode =
                        this.scratchpad.borrow().search_mode == ScratchpadSearchMode::Content;
                    if content_mode {
                        this.run_scratchpad_content_search(cx);
                    } else {
                        cx.notify();
                    }
                }
                _ => {}
            },
        );
        let mut view = self.scratchpad.borrow_mut();
        view.name_input = Some(name_input);
        view._name_sub = Some(name_sub);
        view.search_input = Some(search_input);
        view._search_sub = Some(search_sub);
    }

    /// 开始内联编辑（新建 / 重命名）。
    pub(super) fn start_scratchpad_edit(
        &mut self,
        edit: ScratchpadEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // M1：只读打开时禁止新建/重命名草稿。
        if self.host.read_only() {
            self.host.notice("只读模式：不允许修改草稿".to_string(), cx);
            cx.notify();
            return;
        }
        self.ensure_scratchpad_inputs(window, cx);
        // 新建落点：唯一选中且为文件夹 → 该目录（并展开，使内联行可见）；否则模块根。
        if matches!(edit, ScratchpadEdit::NewFile | ScratchpadEdit::NewFolder) {
            let target = self.scratchpad_paste_target();
            let mut view = self.scratchpad.borrow_mut();
            view.new_target = target.clone();
            if !target.is_empty() {
                view.expanded.insert(target);
            }
        }
        let initial = match &edit {
            ScratchpadEdit::Rename { path } => std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            ScratchpadEdit::NewReference { path } => path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            ScratchpadEdit::RenameReference { alias } => alias.clone(),
            _ => String::new(),
        };
        self.scratchpad.borrow_mut().edit = Some(edit);
        if let Some(input) = self.scratchpad.borrow().name_input.clone() {
            input.update(cx, |s, cx| s.set_value(initial, window, cx));
            let handle = input.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        cx.notify();
    }

    /// 取消内联编辑。
    pub(super) fn cancel_scratchpad_edit(&mut self, cx: &mut Context<Self>) {
        self.scratchpad.borrow_mut().edit = None;
        cx.notify();
    }

    /// 重命名当前唯一选中的条目（F2）。
    pub(super) fn rename_scratchpad_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = {
            let view = self.scratchpad.borrow();
            if view.selected.len() == 1 {
                view.selected.iter().next().cloned()
            } else {
                None
            }
        };
        if let Some(path) = path {
            self.start_scratchpad_edit(ScratchpadEdit::Rename { path }, window, cx);
        }
    }

    /// 提交内联编辑（新建 / 重命名）。
    pub(super) fn commit_scratchpad_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.scratchpad.borrow_mut().edit.take() else {
            return;
        };
        // M1：只读打开时禁止提交草稿修改。
        if self.host.read_only() {
            self.host.notice("只读模式：不允许修改草稿".to_string(), cx);
            cx.notify();
            return;
        }
        let name = self
            .scratchpad
            .borrow()
            .name_input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            cx.notify();
            return;
        }

        let outcome = match self.scratchpad_store() {
            Ok((store, rt)) => {
                let result = match &edit {
                    ScratchpadEdit::NewFile => {
                        // 模板：自动补后缀 + 创建后写入占位内容（空白模板不写）。
                        let (template, target) = {
                            let view = self.scratchpad.borrow();
                            (view.new_template, view.new_target.clone())
                        };
                        let parent = if target.is_empty() {
                            None
                        } else {
                            Some(target.as_str())
                        };
                        let final_name = scratchpad_apply_template_ext(&name, template);
                        let body = template.content(&final_name);
                        rt.block_on(store.create_entry(&final_name, parent, false))
                            .and_then(|_| {
                                if body.is_empty() {
                                    Ok(())
                                } else {
                                    rt.block_on(store.save_file(
                                        &join_scratchpad_rel(&target, &final_name),
                                        &body,
                                    ))
                                }
                            })
                    }
                    ScratchpadEdit::NewFolder => {
                        let target = self.scratchpad.borrow().new_target.clone();
                        let parent = if target.is_empty() {
                            None
                        } else {
                            Some(target.as_str())
                        };
                        rt.block_on(store.create_entry(&name, parent, true))
                            .map(|_| ())
                    }
                    ScratchpadEdit::Rename { path } => {
                        rt.block_on(store.rename_entry(path, &name)).map(|_| ())
                    }
                    ScratchpadEdit::NewReference { path } => rt
                        .block_on(store.add_external_reference(name.clone(), path.clone()))
                        .map(|_| ()),
                    ScratchpadEdit::RenameReference { alias } => {
                        rt.block_on(store.rename_external_reference(alias, &name))
                    }
                };
                result.map(|_| ()).map_err(|e| e.to_string())
            }
            Err(e) => Err(e),
        };

        let mut view = self.scratchpad.borrow_mut();
        if let Some(input) = view.name_input.clone() {
            input.update(cx, |s, cx| s.set_value("", window, cx));
        }
        match outcome {
            Ok(()) => {
                view.loaded = false;
                view.error = None;
            }
            Err(e) => view.error = Some(format!("操作失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 删除所选条目 → 项目级回收站，并记录撤销（批量）。
    pub(super) fn delete_scratchpad_selection(&mut self, cx: &mut Context<Self>) {
        // M1：只读打开时禁止删除草稿。
        if self.host.read_only() {
            self.host.notice("只读模式：不允许删除草稿".to_string(), cx);
            cx.notify();
            return;
        }
        let mut paths: Vec<String> = self.scratchpad.borrow().selected.iter().cloned().collect();
        if paths.is_empty() {
            return;
        }
        paths.sort();
        let label = if paths.len() == 1 {
            scratchpad_basename(&paths[0])
        } else {
            format!("{} 项", paths.len())
        };

        let result = (|| -> Result<Vec<String>, String> {
            let (store, rt) = self.scratchpad_store()?;
            let before: HashSet<String> = rt
                .block_on(store.list_trash())
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|e| e.manifest.id)
                .collect();
            for path in &paths {
                rt.block_on(store.delete_entry(path))
                    .map_err(|e| e.to_string())?;
            }
            let after = rt.block_on(store.list_trash()).map_err(|e| e.to_string())?;
            Ok(after
                .into_iter()
                .map(|e| e.manifest.id)
                .filter(|id| !before.contains(id))
                .collect())
        })();

        let mut undo_ids: Option<Vec<String>> = None;
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(trash_ids) => {
                undo_ids = Some(trash_ids.clone());
                view.undo = Some(ScratchpadUndo { label, trash_ids });
                view.selected.clear();
                view.anchor = None;
                view.loaded = false;
                view.error = None;
            }
            Err(e) => view.error = Some(format!("删除失败: {e}")),
        }
        drop(view);
        if let Some(ids) = undo_ids {
            self.schedule_undo_expiry(ids, cx);
        }
        cx.notify();
    }

    /// 删除单条（行内 ✕）：先设为唯一选中，再走批量删除。
    pub(super) fn delete_scratchpad_entry(
        &mut self,
        relative_path: String,
        cx: &mut Context<Self>,
    ) {
        {
            let mut view = self.scratchpad.borrow_mut();
            view.selected.clear();
            view.selected.insert(relative_path.clone());
            view.anchor = Some(relative_path);
        }
        self.delete_scratchpad_selection(cx);
    }

    /// 撤销上一次删除（从回收站还原全部条目）。
    pub(super) fn undo_scratchpad_delete(&mut self, cx: &mut Context<Self>) {
        let Some(undo) = self.scratchpad.borrow().undo.clone() else {
            return;
        };
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => {
                let mut failure: Option<String> = None;
                for id in &undo.trash_ids {
                    if let Err(e) = rt.block_on(store.restore_from_trash(id)) {
                        failure = Some(e.to_string());
                    }
                }
                match failure {
                    None => Ok(()),
                    Some(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        view.undo = None;
        match result {
            Ok(()) => view.loaded = false,
            Err(e) => view.error = Some(format!("撤销失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 把当前选中项放入剪贴板（剪切 / 复制）。
    pub(super) fn set_scratchpad_clipboard(
        &mut self,
        mode: ScratchpadClipboardMode,
        cx: &mut Context<Self>,
    ) {
        let mut paths: Vec<String> = self.scratchpad.borrow().selected.iter().cloned().collect();
        if paths.is_empty() {
            return;
        }
        paths.sort();
        self.scratchpad.borrow_mut().clipboard = Some(ScratchpadClipboard { mode, paths });
        cx.notify();
    }

    /// 粘贴剪贴板到当前选中的文件夹（未选中文件夹则粘到模块根）。
    ///
    /// 复制可能搬运大量字节（递归复制），故入队到后台；结果由 `apply_scratchpad_ops` 回填。
    pub(super) fn paste_scratchpad_clipboard(&mut self, cx: &mut Context<Self>) {
        // M1：只读打开时禁止写入草稿。
        if self.host.read_only() {
            self.host.notice("只读模式：不允许粘贴草稿".to_string(), cx);
            cx.notify();
            return;
        }
        let Some(clipboard) = self.scratchpad.borrow().clipboard.clone() else {
            return;
        };
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error = Some("未打开项目".to_string());
            cx.notify();
            return;
        };
        let target = self.scratchpad_paste_target();
        let cut = clipboard.mode == ScratchpadClipboardMode::Cut;
        scratchpad_jobs::enqueue_paste(&root, cut, clipboard.paths.clone(), &target);
        {
            let mut view = self.scratchpad.borrow_mut();
            view.selected.clear();
            view.anchor = None;
        }
        self.ensure_scratchpad_pump(cx);
        cx.notify();
    }

    /// 粘贴目标：唯一选中且为文件夹 → 该目录；否则模块根（空串）。
    pub(super) fn scratchpad_paste_target(&self) -> String {
        let view = self.scratchpad.borrow();
        if view.selected.len() == 1 {
            if let Some(path) = view.selected.iter().next() {
                if view
                    .kind_of(path)
                    .map(|k| k == ScratchpadEntryKind::Folder)
                    .unwrap_or(false)
                {
                    return path.clone();
                }
            }
        }
        String::new()
    }

    /// 从回收站还原指定条目。
    pub(super) fn restore_scratchpad_trash(&mut self, trash_id: String, cx: &mut Context<Self>) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.restore_from_trash(&trash_id))
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(()) => view.loaded = false,
            Err(e) => view.error = Some(format!("还原失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 清空回收站（删的是可能很大的 payload，故入队后台）。
    pub(super) fn empty_scratchpad_trash(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error = Some("未打开项目".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_empty_trash(&root);
        self.ensure_scratchpad_pump(cx);
        cx.notify();
    }

    /// 移除外部引用。
    pub(super) fn remove_scratchpad_reference(&mut self, alias: String, cx: &mut Context<Self>) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.remove_external_reference(&alias))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(()) => view.loaded = false,
            Err(e) => view.error = Some(format!("移除引用失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 重新引用（失效引用专用）：选新路径 → 只改路径，别名不变。
    pub(super) fn relink_scratchpad_reference(
        &mut self,
        alias: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.host.read_only() {
            self.host.notice("只读模式：不允许修改引用".to_string(), cx);
            cx.notify();
            return;
        }
        let entity = cx.entity();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("选择引用目标（文件或目录）".into()),
        });
        window
            .spawn(cx, async move |cx| {
                if let Ok(Ok(Some(paths))) = receiver.await {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = cx.update(|_window, cx| {
                            entity.update(cx, |this, cx| {
                                this.apply_scratchpad_relink(alias, path, cx)
                            });
                        });
                    }
                }
            })
            .detach();
    }

    /// 写入新的引用路径（`relink_scratchpad_reference` 选定路径后的落盘步骤）。
    pub(super) fn apply_scratchpad_relink(
        &mut self,
        alias: String,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.update_external_reference_path(&alias, path))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(()) => view.loaded = false,
            Err(e) => view.error = Some(format!("重新引用失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 请求在中央编辑器中打开草稿文件（双击 / Enter / 右键「打开」共用）。
    ///
    /// 只把**绝对路径**交给宿主（`ScratchpadHost::open_in_editor`）；编辑器无根，
    /// 自己按路径判定模式与只读等级（Phase C 契约）。
    pub(super) fn request_open_scratchpad_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.host.open_in_editor(std::path::PathBuf::from(path));
        self.host.notify_host(cx);
        cx.notify();
    }

    /// 请求在洞察面板里看这份草稿的数据统计（右键「查看统计」）。
    ///
    /// 与 [`Self::request_open_scratchpad_file`] 同理只给路径与显示名：能不能分析、
    /// 怎么取数都在宿主侧（`ScratchpadHost::view_stats`），草稿箱不依赖 `engine`。
    pub(super) fn request_view_stats(
        &mut self,
        path: String,
        label: String,
        cx: &mut Context<Self>,
    ) {
        self.host
            .view_stats(std::path::PathBuf::from(path), label, cx);
        self.host.notify_host(cx);
        cx.notify();
    }

    /// 懒加载子目录（展开文件夹时调用）：只入队 + 起轮询，结果由 `apply_scratchpad_dirs` 回填。
    pub(super) fn request_scratchpad_dir(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error =
                Some("未打开项目：草稿箱根即项目目录，请先打开项目。".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_dir_load(&root, &path);
        self.ensure_scratchpad_pump(cx);
    }

    /// 导入外部文件到草稿箱（复制进来；可能拷 GB 级文件，故入队后台）。
    pub(super) fn import_scratchpad_files(
        &mut self,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error = Some("未打开项目".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_import(&root, paths);
        self.ensure_scratchpad_pump(cx);
        cx.notify();
    }

    /// 在系统文件管理器中打开条目（需绝对路径）。
    pub(super) fn open_scratchpad_location(&mut self, path: String, cx: &mut Context<Self>) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.open_in_system_explorer(std::path::Path::new(&path)))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            self.scratchpad.borrow_mut().error = Some(format!("打开位置失败: {e}"));
            cx.notify();
        }
    }

    /// 当前可见行（渲染顺序）的条目路径，供键盘导航与滚动定位。
    pub(super) fn scratchpad_visible_keys(&self, cx: &App) -> Vec<String> {
        let view = self.scratchpad.borrow();
        let filter = view
            .search_input
            .as_ref()
            .map(|i| i.read(cx).value().to_lowercase().trim().to_string())
            .unwrap_or_default();
        let mut flat = Vec::new();
        flatten_scratchpad(
            &view.entries,
            0,
            &view.expanded,
            &view.children,
            view.sort,
            view.sort_desc,
            &filter,
            &mut flat,
        );
        flat.into_iter()
            .map(|(_, e)| e.path.to_string_lossy().to_string())
            .collect()
    }

    /// 树内键盘导航（↑↓）：按可见行顺序移动单选，并把选中项滚到视口内。
    pub(super) fn scratchpad_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let keys = self.scratchpad_visible_keys(cx);
        if keys.is_empty() {
            return;
        }
        let current = {
            let view = self.scratchpad.borrow();
            view.anchor
                .clone()
                .or_else(|| view.selected.iter().next().cloned())
        };
        let position = current.and_then(|k| keys.iter().position(|key| key == &k));
        let next = match position {
            Some(i) => (i as isize + delta).clamp(0, keys.len() as isize - 1) as usize,
            // 无选中：↓ 取首项，↑ 取末项。
            None if delta >= 0 => 0,
            None => keys.len() - 1,
        };
        let key = keys[next].clone();
        {
            let mut view = self.scratchpad.borrow_mut();
            view.selected.clear();
            view.selected.insert(key.clone());
            view.anchor = Some(key);
        }
        self.scratchpad
            .borrow()
            .list_scroll
            .handle()
            .scroll_to_item(next, ScrollStrategy::Center);
        cx.notify();
    }

    /// Enter / →：文件夹展开折叠（展开时顺带懒加载）；文件在中央编辑器中打开。
    pub(super) fn scratchpad_open_selection(&mut self, cx: &mut Context<Self>) {
        let path = {
            let view = self.scratchpad.borrow();
            if view.selected.len() == 1 {
                view.selected.iter().next().cloned()
            } else {
                None
            }
        };
        let Some(path) = path else {
            return;
        };
        let is_folder = self
            .scratchpad
            .borrow()
            .kind_of(&path)
            .map(|k| k == ScratchpadEntryKind::Folder)
            .unwrap_or(false);
        if !is_folder {
            // 文件：交给中央编辑器（同路径已打开只激活，不重读）。
            self.request_open_scratchpad_file(path.clone(), cx);
            return;
        }
        let needs_load = {
            let mut view = self.scratchpad.borrow_mut();
            if view.expanded.contains(&path) {
                view.expanded.remove(&path);
                false
            } else {
                view.expanded.insert(path.clone());
                !view.children.contains_key(&path)
            }
        };
        if needs_load {
            self.request_scratchpad_dir(path, cx);
        } else {
            cx.notify();
        }
    }

    /// 全选已加载条目（Ctrl+A）。
    pub(super) fn select_all_scratchpad(&mut self, cx: &mut Context<Self>) {
        fn walk(entries: &[ScratchpadEntry], out: &mut Vec<String>) {
            for e in entries {
                out.push(e.path.to_string_lossy().to_string());
                if let Some(kids) = &e.children {
                    walk(kids, out);
                }
            }
        }
        let mut keys = Vec::new();
        {
            let view = self.scratchpad.borrow();
            walk(&view.entries, &mut keys);
            for kids in view.children.values() {
                walk(kids, &mut keys);
            }
        }
        let mut view = self.scratchpad.borrow_mut();
        view.selected = keys.into_iter().collect();
        view.anchor = None;
        drop(view);
        cx.notify();
    }

    /// 运行内容搜索（经端口投递到中央编辑区展示；参数留存供外部改动后重跑）。
    ///
    /// 搜索要遍历全树，故入队后台；结果由 `apply_scratchpad_ops` 回填。
    pub(super) fn run_scratchpad_content_search(&mut self, cx: &mut Context<Self>) {
        let query = self
            .scratchpad
            .borrow()
            .search_input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if query.is_empty() {
            self.active_search = None;
            self.host.show_search_results(None, cx);
            self.host.notify_host(cx);
            cx.notify();
            return;
        }
        let (is_regex, case_sensitive) = {
            let v = self.scratchpad.borrow();
            (v.search_regex, v.search_case)
        };
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error = Some("未打开项目".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_search(&root, &query, case_sensitive, is_regex);
        self.ensure_scratchpad_pump(cx);
        cx.notify();
    }

    /// 懒加载子目录（展开文件夹时调用）。
    /// 安排撤销栏 5 秒后自动消失（仅当仍指向同一次删除）。
    pub(super) fn schedule_undo_expiry(&self, trash_ids: Vec<String>, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            executor.timer(std::time::Duration::from_secs(5)).await;
            let _ = weak.update(cx, |this, cx| {
                let matches = this
                    .scratchpad
                    .borrow()
                    .undo
                    .as_ref()
                    .map(|u| u.trash_ids == trash_ids)
                    .unwrap_or(false);
                if matches {
                    this.scratchpad.borrow_mut().undo = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl ScratchpadView {
    /// 打开系统文件对话框并导入所选文件（工具栏「⬇」与空态「导入」共用）。
    pub(super) fn pick_scratchpad_imports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择要导入的文件".into()),
        });
        window
            .spawn(cx, async move |cx| {
                if let Ok(Ok(Some(paths))) = receiver.await {
                    let _ = cx.update(|_window, cx| {
                        entity.update(cx, |this, cx| this.import_scratchpad_files(paths, cx));
                    });
                }
            })
            .detach();
    }
}
