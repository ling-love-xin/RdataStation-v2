//! 右 Dock「历史」面板的视图（B8）
//!
//! 数据服务在 [`crate::history`]（读引擎的 `history_store`）。视图只做三件事：把快照画出来、
//! 把搜索框接进去、把动作发出去。**重放要开一份编辑器文档**——那是工作台的事，本视图不认识
//! 工作台，所以做成**端口注入**（[`HistoryView::attach_replay`]），与另存为路径选择器同口径。
//!
//! ## 刷新时机
//!
//! 历史文件是全局的，执行发生在别的线程（编辑器的执行通道），所以本视图挂一个 1 s 节拍的轮询：
//! 比较 [`crate::history::version`]（`AtomicU64`，O(1)），**只在版本变了时才读盘**。这样
//! “执行完自动出现新记录”不需要任何跨模块订阅，面板也不会每帧问文件。
//!
//! 删除 / 清空 / 搜索是自己发起的，直接在动作里重载。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::history::{self, HistoryItem};

/// 历史面板的数据端口（默认读引擎历史；测试注入假数据，**不碰真实历史文件**）
pub trait HistoryPort {
    /// 按关键词载入（空关键词 = 全部）
    fn load(&self, keyword: &str, limit: usize) -> Result<Vec<HistoryItem>, String>;
    /// 删一条（按记录 id）
    fn remove(&self, id: &str) -> Result<(), String>;
    /// 清空
    fn clear(&self) -> Result<(), String>;
}

/// 默认实现：引擎的历史存储（`crate::history`）
struct EngineHistory;

impl HistoryPort for EngineHistory {
    fn load(&self, keyword: &str, limit: usize) -> Result<Vec<HistoryItem>, String> {
        if keyword.trim().is_empty() {
            history::load(limit)
        } else {
            history::search(keyword.trim(), limit)
        }
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        history::remove(id)
    }

    fn clear(&self) -> Result<(), String> {
        history::clear()
    }
}

/// 重放端口（宿主注入：把一条 SQL 开进编辑器；`&mut App` 是因为宿主动作需要它）
pub type HistoryReplay = Rc<dyn Fn(&HistoryItem, &mut App)>;

/// 一次加载多少条（列表是逐行渲染的，别一次塞几千个元素）
const PAGE: usize = 200;

/// 轮询节拍（只在版本变化时读盘）
const PUMP_INTERVAL: Duration = Duration::from_millis(1000);

/// 右 Dock 的历史面板
pub struct HistoryView {
    items: Vec<HistoryItem>,
    /// 加载失败的原因（显示在列表位置，不是一句“无数据”）
    error: Option<String>,
    /// 当前关键词（空 = 全部）
    keyword: String,
    query: Entity<InputState>,
    port: Rc<dyn HistoryPort>,
    replay: Rc<RefCell<Option<HistoryReplay>>>,
    /// 已经加载到哪个版本（面板据此决定要不要重新读盘）
    loaded_version: u64,
    /// 已加载过一次（首次加载失败也要说原因，别假装“暂无历史”）
    loaded_once: bool,
    _query_sub: Subscription,
    _pump: RefCell<Option<Task<()>>>,
}

impl HistoryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("搜索历史…", window, cx);
            state
        });
        let sub = cx.subscribe_in(
            &query,
            window,
            |this, state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.set_keyword(state.read(cx).value().to_string(), cx);
                }
            },
        );

        let mut view = Self {
            items: Vec::new(),
            error: None,
            keyword: String::new(),
            query,
            port: Rc::new(EngineHistory),
            replay: Rc::new(RefCell::new(None)),
            loaded_version: u64::MAX,
            loaded_once: false,
            _query_sub: sub,
            _pump: RefCell::new(None),
        };
        view.reload(cx);
        view.ensure_pump(cx);
        view
    }

    /// 注入重放端口（**宿主调用一次**；未注入时点条目会明确报“未接入”）
    pub fn attach_replay(&self, replay: HistoryReplay) {
        *self.replay.borrow_mut() = Some(replay);
    }

    /// 换数据端口（测试用：不碰真实历史文件）
    pub fn set_port(&mut self, port: Rc<dyn HistoryPort>, cx: &mut Context<Self>) {
        self.port = port;
        self.reload(cx);
    }

    /// 设置关键词（搜索框变化时走这里；也是测试入口）
    pub fn set_keyword(&mut self, keyword: String, cx: &mut Context<Self>) {
        if self.keyword == keyword {
            return;
        }
        self.keyword = keyword;
        self.reload(cx);
    }

    /// 重新加载（**事件路径**：会读盘）
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let keyword = self.keyword.clone();
        let result = self.port.load(&keyword, PAGE);
        self.loaded_version = history::version();
        self.loaded_once = true;
        match result {
            Ok(items) => {
                self.items = items;
                self.error = None;
            }
            Err(error) => {
                self.items.clear();
                self.error = Some(error);
            }
        }
        cx.notify();
    }

    /// 版本变了就重新加载（轮询泵调用；版本没变什么都不做）
    fn refresh_if_stale(&mut self, cx: &mut Context<Self>) {
        if self.loaded_version != history::version() {
            self.reload(cx);
        }
    }

    /// 起轮询泵（构造时一次；面板还活着就一直转）
    fn ensure_pump(&self, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(PUMP_INTERVAL).await;
                let alive = weak
                    .update(cx, |this, cx| this.refresh_if_stale(cx))
                    .is_ok();
                if !alive {
                    return;
                }
            }
        });
        *self._pump.borrow_mut() = Some(task);
    }

    /// 重放：把这一条送进编辑器（端口未注入时说明原因，不静默）
    pub fn replay_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(index).cloned() else {
            return;
        };
        let replay = self.replay.borrow().clone();
        match replay {
            Some(replay) => replay(&item, cx),
            None => {
                self.error = Some("历史未接入编辑器（宿主未注入重放端口）".to_string());
                cx.notify();
            }
        }
    }

    /// 删除一条（删完就地重载，列表与文件一致）
    pub fn remove_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        let id = item.id.clone();
        match self.port.remove(&id) {
            Ok(()) => self.reload(cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    /// 清空全部（用户已经点过一次确认，这里不再问第二遍）
    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        match self.port.clear() {
            Ok(()) => self.reload(cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    // ===== 测试访问器 =====

    pub fn items_for_test(&self) -> Vec<HistoryItem> {
        self.items.clone()
    }

    pub fn error_for_test(&self) -> Option<String> {
        self.error.clone()
    }

    pub fn item_count_for_test(&self) -> usize {
        self.items.len()
    }
}

impl Render for HistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let danger = theme.colors.danger;
        let foreground = theme.colors.foreground;

        // 搜索行：输入框 + 清空（清空是**破坏性**动作，用 danger 让它的分量看得见）
        let search = div()
            .h_flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .child(div().flex_1().min_w_0().child(Input::new(&self.query)))
            .child({
                let entity = cx.entity();
                Button::new("history-clear")
                    .ghost()
                    .small()
                    .debug_selector(|| "history-clear".to_string())
                    .label("清空")
                    .on_click(move |_, _window, app| {
                        entity.update(app, |view, cx| view.clear_all(cx));
                    })
            });

        // 列表：一行 = 预览 +（时间 · 耗时 · 行数 · 来源），失败的行把原因带出来
        let mut list = div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scrollbar()
            .debug_selector(|| "history-list".to_string());
        for (index, item) in self.items.iter().enumerate() {
            let entity = cx.entity();
            let remove_entity = cx.entity();
            let meta = history_meta(item);
            list = list.child(
                ListItem::new(("history-item", index))
                    .suffix(move |_window, _cx| {
                        // `suffix` 是 `Fn`（会被反复调用）：每次 clone 一份实体再拿下一次点击
                        let entity = remove_entity.clone();
                        Button::new(("history-remove", index))
                            .ghost()
                            .small()
                            .label("删除")
                            .on_click(move |_, _window, app| {
                                entity.update(app, |view, cx| view.remove_at(index, cx));
                            })
                            .into_any_element()
                    })
                    .on_click(move |_, _window, app| {
                        entity.update(app, |view, cx| view.replay_at(index, cx));
                    })
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if item.failed() { danger } else { foreground })
                                    .text_ellipsis()
                                    .child(SharedString::from(item.preview.clone())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .text_ellipsis()
                                    .child(SharedString::from(meta)),
                            ),
                    ),
            );
        }

        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .debug_selector(|| "history-panel".to_string())
            .child(search)
            .when(self.items.is_empty(), |panel| {
                panel.child(
                    div()
                        .px_2()
                        .py_2()
                        .text_xs()
                        .text_color(if self.error.is_some() { danger } else { muted })
                        .debug_selector(|| "history-empty".to_string())
                        .child(SharedString::from(self.empty_text())),
                )
            })
            .when(!self.items.is_empty(), |panel| panel.child(list))
    }
}

impl HistoryView {
    /// 空态文案：搜索无结果 / 加载失败 / 真没有历史，三者说三句不同的话
    fn empty_text(&self) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        if !self.loaded_once {
            return "正在读取历史…".to_string();
        }
        if !self.keyword.trim().is_empty() {
            return format!("没有匹配「{}」的历史", self.keyword.trim());
        }
        "暂无历史记录——执行过的 SQL 会出现在这里".to_string()
    }
}

/// 第二行的元信息（纯函数：哪几段出现、怎么拼，都在这儿定，可逐条断言）
pub fn history_meta(item: &HistoryItem) -> String {
    let mut parts = vec![item.time_text.clone(), item.duration_text.clone()];
    if let Some(rows) = &item.rows_text {
        parts.push(rows.clone());
    }
    if let Some(source) = &item.source_text {
        parts.push(source.clone());
    }
    if let Some(sources) = &item.sources_text {
        parts.push(sources.clone());
    }
    if let Some(error) = &item.error {
        parts.push(error.clone());
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::TestAppContext;

    use super::{HistoryPort, HistoryView, history_meta};
    use crate::history::HistoryItem;

    fn item(id: &str, sql: &str) -> HistoryItem {
        HistoryItem {
            id: id.to_string(),
            sql: sql.to_string(),
            preview: sql.to_string(),
            time_text: "3 分钟前".to_string(),
            duration_text: "12 ms".to_string(),
            rows_text: Some("返回 1 行".to_string()),
            source_text: Some("MYSQL".to_string()),
            sources_text: None,
            error: None,
            conn_id: None,
        }
    }

    /// 假数据端口：内存里的一堆记录 + 收到过什么的流水（不碰真实历史文件）
    struct FakeHistory {
        items: Rc<RefCell<Vec<HistoryItem>>>,
        loaded: Rc<RefCell<Vec<String>>>,
        removed: Rc<RefCell<Vec<String>>>,
        cleared: Rc<RefCell<usize>>,
    }

    impl FakeHistory {
        fn new(items: Vec<HistoryItem>) -> Self {
            Self {
                items: Rc::new(RefCell::new(items)),
                loaded: Rc::new(RefCell::new(Vec::new())),
                removed: Rc::new(RefCell::new(Vec::new())),
                cleared: Rc::new(RefCell::new(0)),
            }
        }
    }

    impl HistoryPort for FakeHistory {
        fn load(&self, keyword: &str, _limit: usize) -> Result<Vec<HistoryItem>, String> {
            self.loaded.borrow_mut().push(keyword.to_string());
            let keyword = keyword.trim();
            Ok(self
                .items
                .borrow()
                .iter()
                .filter(|item| keyword.is_empty() || item.sql.contains(keyword))
                .cloned()
                .collect())
        }

        fn remove(&self, id: &str) -> Result<(), String> {
            self.removed.borrow_mut().push(id.to_string());
            self.items.borrow_mut().retain(|item| item.id != id);
            Ok(())
        }

        fn clear(&self) -> Result<(), String> {
            *self.cleared.borrow_mut() += 1;
            self.items.borrow_mut().clear();
            Ok(())
        }
    }

    #[gpui_kit::test]
    fn meta_line_joins_the_real_fields(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let failed = HistoryItem {
            error: Some("no such column: nope".to_string()),
            ..item("rec-1", "SELECT nope")
        };
        assert_eq!(
            history_meta(&item("rec-1", "SELECT 1")),
            "3 分钟前 · 12 ms · 返回 1 行 · MYSQL"
        );
        assert_eq!(
            history_meta(&failed),
            "3 分钟前 · 12 ms · 返回 1 行 · MYSQL · no such column: nope",
            "失败的行把原因带在第二行（列表里就能看出为什么失败）"
        );
        let mut bare = item("rec-1", "SELECT 1");
        bare.rows_text = None;
        bare.source_text = None;
        assert_eq!(history_meta(&bare), "3 分钟前 · 12 ms");

        // 【联邦】参与源跟在来源之后（不是联邦档 / 没记源就不显示）
        let mut federated = item("rec-2", "SELECT 1");
        federated.source_text = Some("MYSQL·联邦".to_string());
        federated.sources_text = Some("源 mysql_src, pg_warehouse".to_string());
        assert_eq!(
            history_meta(&federated),
            "3 分钟前 · 12 ms · 返回 1 行 · MYSQL·联邦 · 源 mysql_src, pg_warehouse"
        );
    }

    /// 列表 → 搜索 → 重放 → 删除 → 清空：每一步都真的到了端口
    #[gpui_kit::test]
    fn the_history_panel_lists_searches_replays_and_removes(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let fake = FakeHistory::new(vec![
            item("rec-1", "SELECT * FROM orders"),
            item("rec-2", "SELECT * FROM users"),
        ]);
        let loaded = fake.loaded.clone();
        let removed = fake.removed.clone();
        let cleared = fake.cleared.clone();
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut view = HistoryView::new(window, cx);
            view.set_port(Rc::new(FakeHistory {
                items: fake.items.clone(),
                loaded: fake.loaded.clone(),
                removed: fake.removed.clone(),
                cleared: fake.cleared.clone(),
            }), cx);
            view
        });

        assert_eq!(view.read_with(cx, |view, _| view.item_count_for_test()), 2);
        assert_eq!(loaded.borrow().as_slice(), [""], "初次加载问的是“全部”");

        // 重放：点第 0 条 → 端口拿到那条 SQL（这里只验到端口，开文档是宿主的事）
        let replayed = Rc::new(RefCell::new(Vec::new()));
        {
            let replayed = replayed.clone();
            view.update(cx, |view, _| {
                view.attach_replay(Rc::new(move |item, _app| {
                    replayed.borrow_mut().push(item.sql.clone());
                }));
            });
        }
        view.update(cx, |view, cx| view.replay_at(0, cx));
        assert_eq!(
            replayed.borrow().as_slice(),
            ["SELECT * FROM orders".to_string()],
            "重放把整条原文送出去（不是预览文本）"
        );

        // 搜索：关键词到端口，列表跟着收敛
        view.update(cx, |view, cx| view.set_keyword("users".to_string(), cx));
        assert_eq!(loaded.borrow().last().map(String::as_str), Some("users"));
        assert_eq!(view.read_with(cx, |view, _| view.item_count_for_test()), 1);
        assert_eq!(
            view.read_with(cx, |view, _| view.items_for_test()[0].sql.clone()),
            "SELECT * FROM users"
        );

        // 删除：按记录 id 删，删完就地重载
        view.update(cx, |view, cx| view.remove_at(0, cx));
        assert_eq!(removed.borrow().as_slice(), ["rec-2"], "删的是那一条的 id");
        assert_eq!(view.read_with(cx, |view, _| view.item_count_for_test()), 0);

        // 清空：端口收到一次 clear，列表空
        view.update(cx, |view, cx| {
            view.set_keyword(String::new(), cx);
            view.clear_all(cx);
        });
        assert_eq!(*cleared.borrow(), 1);
        assert_eq!(view.read_with(cx, |view, _| view.item_count_for_test()), 0);
    }

    /// 没注入重放端口：点条目要留一句可读原因（不静默）
    #[gpui_kit::test]
    fn replaying_without_a_port_says_so(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let fake = FakeHistory::new(vec![item("rec-1", "SELECT 1")]);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut view = HistoryView::new(window, cx);
            view.set_port(Rc::new(FakeHistory {
                items: fake.items.clone(),
                loaded: fake.loaded.clone(),
                removed: fake.removed.clone(),
                cleared: fake.cleared.clone(),
            }), cx);
            view
        });

        view.update(cx, |view, cx| view.replay_at(0, cx));
        let error = view.read_with(cx, |view, _| view.error_for_test());
        assert!(
            error.as_deref().is_some_and(|text| text.contains("未接入")),
            "要说明未接入，而不是点一下没反应：{error:?}"
        );
    }
}
