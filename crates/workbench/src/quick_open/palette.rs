//! Quick Open 浮层（独立视图实体）：输入 + 结果列表 + 键盘通道 + 元数据异步搜索。
//!
//! 宿主只做三件事：
//! 1. 把浮层挂进 overlay（`render_quick_open` 返回本实体的元素）；
//! 2. 注入动作端口 [`QuickOpenHost`]（执行一条结果要什么副作用，只有宿主知道）；
//! 3. 开关时置 `Shared::quick_open`（标题栏入口 / `Ctrl+P`），并调 [`QuickOpenPalette::on_opened`]。
//!
//! 这样键盘 / 焦点 / 防抖 / 回填都能用**简化宿主**在窗口测试里驱动（不必构造 `WorkbenchView`），
//! 也是将来把本模块升级成独立 crate 时的现成边界。
//!
//! 状态归属：
//! - `Shared::quick_open` = 开关（全应用一份：标题栏、设置页互斥、命令都读它）；
//! - 本实体 = 输入 / 结果 / 选中锚点 / 防抖与回填任务 / 元数据命中缓存。

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::{ActiveTheme, IndexPath};
use gpui_kit::*;

use crate::panels::Shared;
use crate::quick_open::delegate::QuickOpenDelegate;
use crate::quick_open::model::{self, Action};
use crate::ui;

/// 宿主动作端口：执行一条结果（副作用语义在宿主一侧）。
pub(crate) trait QuickOpenHost: 'static {
    /// `keep_open` = Shift+↵ / Ctrl+点击（保留面板）。
    fn execute(&self, action: Action, keep_open: bool, window: &mut Window, cx: &mut App);
}

/// 文件源快照（键 = 项目根：切项目后旧快照不匹配，不会显示别的项目的文件）。
struct FilesSnapshot {
    root: PathBuf,
    files: Vec<model::FileObject>,
    /// 后台因 [上限](scratchpad::jobs::FLAT_FILE_LIMIT) 截断过（行上要说「没全列」）。
    truncated: bool,
}

/// Quick Open 浮层。
pub(crate) struct QuickOpenPalette {
    shared: Shared,
    host: Rc<dyn QuickOpenHost>,
    /// 输入框（首帧渲染时创建：需要 `&mut Window`）。
    input: Option<Entity<InputState>>,
    _input_sub: Option<Subscription>,
    /// 结果列表（首帧渲染时创建；行 / 选中 / 空态归组件）。
    list: Option<Entity<ListState<QuickOpenDelegate>>>,
    /// 打开后待办：清输入 / 重置选中 / 聚焦 / 刷新（实体可能是上一帧才创建的）。
    pending_open: bool,
    /// 打开时「聚焦输入框」的待办标记。
    focus_pending: bool,
    /// 选中的行业务键（跨重算跟随的权威；组件的选中索引只是渲染锚点）。
    selected_key: Option<String>,
    /// 元数据命中（宿主缓存；后台回填后在 render 重建成行）。
    meta: Vec<model::MetaObject>,
    /// 文件源清单（每次打开重取一次；搜索是本地过滤，不跟输入走）。
    files: Option<FilesSnapshot>,
    /// 已为哪个项目根发过清单请求（避免每敲一个字都重发）。
    files_requested_for: Option<PathBuf>,
    /// 有清单请求在途。
    files_pending: bool,
    /// 已发给后台的搜索词（**只在词变时发**；回填时据此丢弃过期批次）。
    sent_query: Option<String>,
    /// 元数据搜索中（组头显示「搜索中…」）。
    searching: bool,
    /// 回填后有新数据、待 render 重建行（泵线程拿不到 `Window`）。
    rows_dirty: bool,
    /// 防抖任务句柄（替换即取消上一枚）。
    debounce: Option<Task<()>>,
    /// 结果回填泵（打开时启动；关闭期间降频空转）。
    pump: Option<Task<()>>,
}

impl QuickOpenPalette {
    pub(crate) fn new(shared: Shared, host: Rc<dyn QuickOpenHost>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            shared,
            host,
            input: None,
            _input_sub: None,
            list: None,
            pending_open: false,
            focus_pending: false,
            selected_key: None,
            meta: Vec::new(),
            files: None,
            files_requested_for: None,
            files_pending: false,
            sent_query: None,
            searching: false,
            rows_dirty: false,
            debounce: None,
            pump: None,
        };
        this.ensure_pump(cx);
        this
    }

    /// 打开边沿：宿主置位 `Shared::quick_open` 后调它。
    ///
    /// 真正的清输入 / 落选中 / 聚焦在下一帧 render 里做（输入框可能还没创建）。
    pub(crate) fn on_opened(&mut self, cx: &mut Context<Self>) {
        self.pending_open = true;
        self.focus_pending = true;
        self.selected_key = None;
        // 文件清单每次打开重取一次（面板关着的时候草稿可能被外部改过）；
        // 在途时不置位——否则一次打开会发两份清单，后到的那份白算。
        if !self.files_pending {
            self.files_requested_for = None;
        }
        cx.notify();
    }

    /// 关闭（Esc / 点遮罩 / 执行完）：只改共享开关，宿主与状态栏读同一份。
    fn close(&mut self, cx: &mut Context<Self>) {
        self.shared.quick_open.set(false);
        cx.notify();
    }

    /// 委托回传的选中变化（鼠标点击 / 组件漫游）。
    pub(crate) fn set_selection(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if self.selected_key == key {
            return;
        }
        self.selected_key = key;
        cx.notify();
    }

    /// 委托回传的确认（鼠标点击 / 组件内回车）→ 交给宿主执行；不保留则自行关闭。
    pub(crate) fn confirm(&mut self, keep_open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.selected_action(cx) else {
            return;
        };
        self.host.execute(action, keep_open, window, cx);
        // 「执行后关面板」是浮层自己的行为（与 Esc / 点遮罩同一组收尾），
        // 不依赖宿主实现——宿主可能只想记录/转发动作。
        if !keep_open {
            self.close(cx);
        }
    }

    fn selected_action(&self, cx: &App) -> Option<Action> {
        let key = self.selected_key.as_deref()?;
        Some(self.list.as_ref()?.read(cx).delegate().action_of(key)?)
    }

    /// 懒创建输入框与结果列表（首帧渲染；两者都需要 `&mut Window`）。
    fn ensure_entities(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder("搜索对象、命令…（> 命令 · # 全文）")
            });
            self._input_sub = Some(cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => this.refresh(window, cx),
                    InputEvent::PressEnter { shift, .. } => {
                        // ↵ 打开并关闭；Shift+↵ 保留面板（Ctrl+↵ 的后台打开待编辑器支持）。
                        let keep_open = *shift;
                        this.confirm(keep_open, window, cx);
                    }
                    _ => {}
                },
            ));
            self.input = Some(input);
        }
        if self.list.is_none() {
            let palette = cx.entity().downgrade();
            let list = cx.new(|cx| {
                ListState::new(QuickOpenDelegate::new(palette), window, cx).selectable(true)
            });
            self.list = Some(list);
        }
    }

    /// 上一帧遗留的打开待办（清输入 → 刷新 → 聚焦）。
    ///
    /// 「打开」是**权威的刷新边沿**，不等输入框的 `Change`：
    /// 空输入也要出默认分组（连接 / 文件 / 命令）——否则打开后结果区是空的，
    /// 看起来像「搜不到」；而且文件清单的请求就挂在这一跳上。
    fn consume_pending_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut opened = false;
        if self.pending_open {
            self.pending_open = false;
            opened = true;
            self.meta.clear();
            self.sent_query = None;
            self.searching = false;
            if let Some(input) = self.input.clone() {
                input.update(cx, |state, cx| state.set_value("", window, cx));
            }
        }
        if opened || self.rows_dirty {
            self.rows_dirty = false;
            self.refresh(window, cx);
        }
        if self.focus_pending {
            self.focus_pending = false;
            if let Some(input) = self.input.clone() {
                let handle = input.read(cx).focus_handle(cx);
                handle.focus(window, cx);
            }
        }
    }

    /// 重算结果并推给列表（输入变化 / 打开 / 回填后调用）。
    ///
    /// 选中按**业务键**跟随：键还在就保留，否则落到第一行（异步回填不抢用户位置）。
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(input), Some(list)) = (self.input.clone(), self.list.clone()) else {
            return;
        };
        let query = model::parse(&input.read(cx).value());
        let connections: Vec<(String, String)> = self
            .shared
            .connections
            .borrow()
            .iter()
            .map(|c| (c.name.clone(), c.driver.clone()))
            .collect();
        let sources = model::Sources {
            connections: &connections,
            meta: &self.meta,
            meta_searching: self.searching,
            files: self.visible_files(),
            files_note: self.files_note(),
        };
        let groups = model::build_groups(&query, &sources);
        let selected = self
            .selected_key
            .clone()
            .filter(|key| {
                groups
                    .iter()
                    .any(|group| group.rows.iter().any(|row| &row.key == key))
            })
            .or_else(|| {
                groups
                    .iter()
                    .flat_map(|group| group.rows.iter())
                    .map(|row| row.key.clone())
                    .next()
            });
        self.selected_key = selected.clone();
        list.update(cx, |state, cx| {
            state.delegate_mut().set_groups(
                groups,
                query.needle.clone(),
                query.mode,
                selected.clone(),
                cx,
            );
            let target = selected
                .as_deref()
                .and_then(|key| state.delegate().index_of(key));
            mirror_selection(state, target, window, cx);
        });
        // 元数据：词变了才防抖发一次
        self.schedule_search(&query, cx);
        // 文件：只在打开后拿一次（不跟词走）
        self.schedule_file_list(cx);
        cx.notify();
    }

    /// 当前项目根对应的文件清单（切项目 / 未打开项目时为空）。
    fn visible_files(&self) -> &[model::FileObject] {
        let Some(root) = self.shared.project_root() else {
            return &[];
        };
        self.files
            .as_ref()
            .filter(|snapshot| snapshot.root == root)
            .map(|snapshot| snapshot.files.as_slice())
            .unwrap_or(&[])
    }

    /// 文件组的组头补充（清单被后台截断时要说清「没全列」）。
    fn files_note(&self) -> String {
        let Some(root) = self.shared.project_root() else {
            return String::new();
        };
        match self.files.as_ref().filter(|snapshot| snapshot.root == root) {
            Some(snapshot) if snapshot.truncated => format!(
                "文件较多，只列前 {} 个（继续输入缩小范围）",
                scratchpad::jobs::FLAT_FILE_LIMIT
            ),
            _ => String::new(),
        }
    }

    /// 给后台排一次**扁平文件清单**（每次打开一次；结果由泵回填）。
    ///
    /// 与元数据搜索的关键差别：文件源**不分档、不跟词走**（过滤是本地的事），
    /// 所以不需要防抖，也不需要随输入重发——只在“打开”这个边沿拿一次。
    fn schedule_file_list(&mut self, _cx: &mut Context<Self>) {
        let Some(root) = self.shared.project_root() else {
            // 未打开项目：草稿箱是项目级能力（不留上一个项目的行）。
            self.files = None;
            self.files_requested_for = None;
            return;
        };
        if self.files_pending || self.files_requested_for.as_deref() == Some(root.as_path()) {
            return;
        }
        self.files_requested_for = Some(root.clone());
        self.files_pending = true;
        scratchpad::jobs::enqueue_flatten_files(&root);
    }

    /// 给后台排一次跨连接索引搜索（防抖 150ms；**单字符不发**）。
    fn schedule_search(&mut self, query: &model::Query, cx: &mut Context<Self>) {
        if !query.async_ready() {
            return;
        }
        if self.sent_query.as_deref() == Some(query.needle.as_str()) {
            return;
        }
        let targets: Vec<database::nav_jobs::SearchTarget> = self
            .shared
            .connections
            .borrow()
            .iter()
            .map(|c| database::nav_jobs::SearchTarget {
                conn_id: c.id.clone(),
                label: c.name.clone(),
                driver: c.driver.clone(),
            })
            .collect();
        let project_root = self
            .shared
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        let needle = query.needle.clone();
        // 档位：`#` 走内容档（FTS，注释 / 数据类型），其余走名称档（中缀 LIKE）。
        let kind = match query.mode {
            model::Mode::FullText => database::nav_jobs::SearchKind::FullText,
            _ => database::nav_jobs::SearchKind::Name,
        };
        self.sent_query = Some(needle.clone());
        self.searching = true;
        let executor = cx.background_executor().clone();
        let weak = cx.entity().downgrade();
        // 防抖：句柄被下一枚替换即取消（`Task` drop = 取消）
        let task = cx.spawn(async move |_this, cx| {
            executor
                .timer(Duration::from_millis(ui::QUICK_OPEN_SEARCH_DEBOUNCE_MS))
                .await;
            let _ = weak.update(cx, |_this, _cx| {
                database::nav_jobs::enqueue_search(
                    database::nav_jobs::SearchConsumer::QuickOpen,
                    kind,
                    &needle,
                    project_root.as_deref(),
                    targets,
                );
            });
        });
        self.debounce = Some(task);
    }

    /// 启动结果回填泵（构造时一次；关闭期间降频空转，视图销毁即退出）。
    fn ensure_pump(&mut self, cx: &mut Context<Self>) {
        if self.pump.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| loop {
            match weak.update(cx, |this, _| this.shared.quick_open.get()) {
                Ok(open) => {
                    // 开着时勤取（元数据正在回填），关着时降频空转
                    let wait = if open { 60 } else { 400 };
                    executor.timer(Duration::from_millis(wait)).await;
                    if open {
                        let _ = weak.update(cx, |this, cx| this.pump_results(cx));
                    }
                }
                // 视图已销毁：退出任务（否则会永远空转）
                Err(_) => return,
            }
        });
        self.pump = Some(task);
    }

    /// 取走元数据搜索结果 + 文件清单（过期批次丢弃；不碰 UI，只置脏标记）。
    fn pump_results(&mut self, cx: &mut Context<Self>) {
        let results = database::nav_jobs::drain_search_results(
            database::nav_jobs::SearchConsumer::QuickOpen,
        );
        let lists = scratchpad::jobs::drain_file_lists();
        if results.is_empty() && lists.is_empty() {
            return;
        }
        let current = self
            .input
            .as_ref()
            .map(|input| model::parse(&input.read(cx).value()).needle)
            .unwrap_or_default();
        let mut changed = false;
        for result in results {
            // 过期批次：词已经被改了（收尾交给当前词的那一批）
            if result.query != current {
                continue;
            }
            self.searching = false;
            self.meta = result.hits.iter().filter_map(model::meta_object).collect();
            changed = true;
        }
        for payload in lists {
            self.files_pending = false;
            // 无论收不收（可能是上一个项目的在途结果），都要重算一次：
            // 收下的换掉旧清单，丢掉的会触发下一次 refresh 去取当前项目的。
            changed = true;
            if self.files_requested_for.as_deref() != Some(payload.project_root.as_path()) {
                continue;
            }
            match payload.files {
                Some(files) => {
                    self.files = Some(FilesSnapshot {
                        root: payload.project_root,
                        files: files.iter().map(model::file_object).collect(),
                        truncated: payload.truncated,
                    });
                }
                // 读盘失败（原因已记日志）：保留上一份，清掉请求标记以便下次重试。
                None => self.files_requested_for = None,
            }
        }
        if changed {
            self.rows_dirty = true;
            cx.notify();
        }
    }

    /// ↑↓：在结果上漫游（焦点在输入框，键盘由本视图接住）。
    fn move_selection(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(list) = self.list.clone() else {
            return;
        };
        let mut moved: Option<String> = None;
        list.update(cx, |state, cx| {
            let from = state
                .selected_index()
                .or_else(|| state.delegate().first_index());
            let Some(from) = from else {
                return;
            };
            let Some(target) = state.delegate().step_index(from, delta) else {
                return;
            };
            state.delegate_mut().begin_host_sync();
            state.set_selected_index(Some(target), window, cx);
            state.delegate_mut().end_host_sync();
            moved = state.delegate().selected_key();
            state.scroll_to_selected_item(window, cx);
        });
        if let Some(key) = moved {
            self.selected_key = Some(key);
            cx.notify();
        }
    }

    /// 测试只读访问器：输入框当前文本。
    #[cfg(test)]
    pub(crate) fn query_text(&self, cx: &App) -> String {
        self.input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// 测试只读访问器：结果行数。
    #[cfg(test)]
    pub(crate) fn row_count(&self, cx: &App) -> usize {
        self.list
            .as_ref()
            .map(|list| list.read(cx).delegate().row_count())
            .unwrap_or(0)
    }

    /// 测试只读访问器：结果行的业务键（按渲染顺序）。
    #[cfg(test)]
    pub(crate) fn row_keys(&self, cx: &App) -> Vec<String> {
        self.list
            .as_ref()
            .map(|list| list.read(cx).delegate().keys())
            .unwrap_or_default()
    }

    /// 测试只读访问器：选中的行业务键。
    #[cfg(test)]
    pub(crate) fn selected_key(&self) -> Option<String> {
        self.selected_key.clone()
    }

    /// 测试只读访问器：是否处于「元数据搜索中」。
    #[cfg(test)]
    pub(crate) fn searching(&self) -> bool {
        self.searching
    }

    /// 测试只读访问器：已发给后台的搜索词（`None` = 一次都没发）。
    #[cfg(test)]
    pub(crate) fn sent_query(&self) -> Option<String> {
        self.sent_query.clone()
    }
}

/// 把宿主的选中镜像进列表（只在真的不一致时才动）。
///
/// 镜像期间组件回调**不回写**本视图（`begin_host_sync` / `end_host_sync` 守卫）：
/// 镜像常发生在渲染期，回写就是「更新正在被更新的实体」。
fn mirror_selection(
    state: &mut ListState<QuickOpenDelegate>,
    target: Option<IndexPath>,
    window: &mut Window,
    cx: &mut Context<ListState<QuickOpenDelegate>>,
) {
    if state.selected_index() == target {
        return;
    }
    state.delegate_mut().begin_host_sync();
    state.set_selected_index(target, window, cx);
    state.delegate_mut().end_host_sync();
    state.scroll_to_selected_item(window, cx);
}

impl Render for QuickOpenPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.shared.quick_open.get() {
            return div().into_any_element();
        }
        self.ensure_entities(window, cx);
        self.consume_pending_open(window, cx);

        let theme = cx.theme().clone();
        let input = self.input.clone().expect("input lazy init");
        let list = self.list.clone().expect("list lazy init");
        let query = model::parse(&input.read(cx).value());
        let rows = list.read(cx).delegate().row_count();
        // 词长达不到本档门槛：本地源照常，这里给还差几个字的提示。
        let hint_right = if !query.needle.is_empty() && !query.async_ready() {
            let short = query.min_len().saturating_sub(query.needle.chars().count());
            format!("再输入 {short} 个字符开始搜索元数据")
        } else if self.searching {
            "元数据搜索中…".to_string()
        } else {
            format!("{rows} 条")
        };
        let mode_chip = match query.mode {
            model::Mode::Command => Some((">", "命令")),
            model::Mode::FullText => Some(("#", "元数据全文")),
            model::Mode::Default => None,
        };

        let mut input_row = div().h_flex().items_center().gap_2();
        input_row = input_row.child(div().flex_1().min_w_0().child(Input::new(&input)));
        if let Some((glyph, label)) = mode_chip {
            input_row = input_row.child(
                div()
                    .h_flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .rounded_sm()
                    .bg(theme.colors.secondary)
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(glyph)
                    .child(label),
            );
        }

        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .bg(theme.colors.overlay)
            .child(
                div()
                    // 示意 v5：面板水平居中、顶部距标题栏 42px。
                    .mt(rems(ui::QUICK_OPEN_PANEL_TOP))
                    .w(rems(ui::QUICK_OPEN_PANEL_WIDTH))
                    .max_h(rems(ui::QUICK_OPEN_PANEL_MAX_HEIGHT))
                    .v_flex()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.popover)
                    .shadow_lg()
                    .child(input_row)
                    .child(List::new(&list).max_h(rems(ui::QUICK_OPEN_LIST_MAX_HEIGHT)))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .h(rems(ui::QUICK_OPEN_HINT_HEIGHT))
                            .border_t_1()
                            .border_color(theme.colors.border)
                            .px_1()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("↑↓ 选择 · ↵ 打开 · Shift+↵ 保留面板 · Esc 关闭")
                            .child(div().ml_auto().child(hint_right)),
                    ),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close(cx)))
            // 键盘：焦点在输入框里，单行 Input 不处理 ↑↓ / Esc（会 propagate），
            // 因此这三个 Action 在浮层根上接住；Enter 由输入框事件给出（含 Shift 三态）。
            .on_action(cx.listener(|this, _: &MoveUp, window, cx| {
                this.move_selection(-1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveDown, window, cx| {
                this.move_selection(1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Escape, _window, cx| this.close(cx)))
            .into_any_element()
    }
}
