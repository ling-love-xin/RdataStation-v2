//! 日志查看对话框（日志系统的读路径）。
//!
//! 两个出口的关系（见 `docs/architecture/runtime/logging.md`）：
//! - **文件** `<RDS_HOME>/logs/app.YYYY-MM-DD`：tracing 直写、逐行脱敏，进程崩了也在；
//! - **全局库** `app_logs` 表：异步批量落库（每 100 条或 1 秒），本对话框读这一份——
//!   好处是可筛选、可搜索、跨会话，代价是最后 1 秒可能还没落库。
//!
//! 形态：**只读快照 + 手动刷新**。不做自动轮询：看日志是排障动作，不是常驻面板；
//! 轮询会让「我看到的和上一条不一致」变成日常。
//!
//! 「级别」是**门槛**（≥ 该级别）而不是精确匹配：查询一次取最新 N 条，级别在本地筛，
//! 所以点级别是瞬时的、不重查库（`LogStore::query_logs` 的 `level` 是精确匹配，
//! 拿它做门槛得查 5 次）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::*;

use engine::logging::get_log_store;
use engine::{LogLevel, LogQuery, LogRecord};

use workbench_shell::ui;

/// 一次取多少条（`LogStore` 服务端上限 500）。
const PAGE_SIZE: u32 = 500;

/// 级别门槛的可选项（`None` = 不过滤）。
const LEVEL_CHOICES: [Option<LogLevel>; 6] = [
    None,
    Some(LogLevel::Trace),
    Some(LogLevel::Debug),
    Some(LogLevel::Info),
    Some(LogLevel::Warn),
    Some(LogLevel::Error),
];

fn level_label(level: Option<LogLevel>) -> &'static str {
    match level {
        None => "全部",
        Some(LogLevel::Trace) => "TRACE",
        Some(LogLevel::Debug) => "DEBUG",
        Some(LogLevel::Info) => "INFO",
        Some(LogLevel::Warn) => "WARN",
        Some(LogLevel::Error) => "ERROR",
    }
}

/// 记录是否达到门槛（含等于）。
fn passes_threshold(level: LogLevel, threshold: Option<LogLevel>) -> bool {
    threshold.is_none_or(|t| level >= t)
}

/// 级别配色（与导航面板的状态色同一口径：危险 / 警告 / 中性）。
fn level_color(level: LogLevel, theme: &gpui_kit::component::Theme) -> Hsla {
    match level {
        LogLevel::Error => theme.colors.danger,
        LogLevel::Warn => theme.colors.warning,
        LogLevel::Info => theme.colors.foreground,
        LogLevel::Trace | LogLevel::Debug => theme.colors.muted_foreground,
    }
}

/// 时间戳 `2026-09-16T13:17:02.191Z` → `13:17:02`（列宽定死，塞不下完整 ISO）。
fn short_time(timestamp: &str) -> String {
    timestamp
        .split_once('T')
        .and_then(|(_, rest)| rest.get(..8))
        .unwrap_or(timestamp)
        .to_string()
}

/// 对话框状态：异步取数与渲染闭包共享同一份（渲染纯读，取数回填后刷新窗口）。
#[derive(Default)]
struct LogView {
    records: Vec<LogRecord>,
    /// 服务端匹配总数（关键字过滤之后）
    total: usize,
    /// 级别门槛（本地筛，不重查库）
    threshold: Option<LogLevel>,
    /// 上一次查询用的关键字（输入框的值在点刷新时才读走）
    keyword: String,
    /// 是否查过（区分「还没查」与「查了是空」）
    loaded: bool,
    loading: bool,
    error: Option<String>,
}

impl LogView {
    /// 当前门槛下的可见条数。
    fn visible(&self) -> usize {
        self.records
            .iter()
            .filter(|r| passes_threshold(r.level, self.threshold))
            .count()
    }

    fn status_text(&self) -> String {
        if self.loading {
            return "读取中…".to_string();
        }
        if let Some(e) = &self.error {
            return format!("读取失败：{e}");
        }
        if !self.loaded {
            return "尚未读取".to_string();
        }
        if self.records.is_empty() {
            return "没有匹配的日志".to_string();
        }
        let mut text = format!(
            "库中匹配 {} 条，取最新 {} 条，其中 {} 条达当前级别",
            self.total,
            self.records.len(),
            self.visible()
        );
        if self.total > self.records.len() {
            text.push_str("（缩小关键字可看到全部）");
        }
        text
    }

    /// 关键字是否为空（空则不传给查询，避免 `LIKE '%%'` 白跑一次索引）。
    fn keyword_query(&self) -> Option<String> {
        if self.keyword.is_empty() {
            None
        } else {
            Some(self.keyword.clone())
        }
    }
}

/// 取一次数据（异步）并刷新窗口。
///
/// 刷新靠 `App::refresh_windows`：对话框内容由窗口渲染时的闭包重建，
/// 不主动刷窗口的话，异步回来的数据不会出现在屏幕上。
fn fetch(state: Rc<RefCell<LogView>>, keyword: String, cx: &mut App) {
    {
        let mut view = state.borrow_mut();
        view.keyword = keyword;
        view.loading = true;
        view.error = None;
    }

    let Some(store) = get_log_store() else {
        let mut view = state.borrow_mut();
        view.loading = false;
        view.loaded = true;
        view.error = Some("日志系统未接线（本次运行没有可用日志）".to_string());
        drop(view);
        cx.refresh_windows();
        return;
    };

    let query = {
        let view = state.borrow();
        LogQuery {
            page: Some(1),
            page_size: Some(PAGE_SIZE),
            keyword: view.keyword_query(),
            ..LogQuery::default()
        }
    };

    cx.spawn(async move |cx| {
        let result = store.query_logs(&query).await;
        {
            let mut view = state.borrow_mut();
            view.loading = false;
            view.loaded = true;
            match result {
                Ok(page) => {
                    view.total = page.total as usize;
                    view.records = page.records;
                    view.error = None;
                }
                Err(e) => {
                    view.error = Some(e.to_string());
                    view.records.clear();
                    view.total = 0;
                }
            }
        }
        cx.update(|app| app.refresh_windows());
    })
    .detach();
}

/// 打开日志查看对话框（入口：设置页「日志 → 查看日志…」）。
pub fn open_log_dialog(window: &mut Window, cx: &mut App) {
    let state: Rc<RefCell<LogView>> = Rc::new(RefCell::new(LogView::default()));
    let keyword_input = cx.new(|cx| InputState::new(window, cx).placeholder("按消息 / 模块过滤…"));
    // 打开即取一次（最新 500 条、不过滤）：用户点开就想看到东西。
    fetch(state.clone(), String::new(), cx);

    let state_build = state.clone();
    let input_build = keyword_input.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let view = state_build.borrow();

        // ── 头：级别门槛 + 关键字 + 刷新 ──────────────────────────────
        let mut head = div().h_flex().w_full().items_center().gap_2().text_xs();
        head = head.child(
            div()
                .flex_none()
                .text_color(theme.colors.muted_foreground)
                .child("级别 ≥"),
        );
        for choice in LEVEL_CHOICES {
            let selected = view.threshold == choice;
            let state_click = state_build.clone();
            let mut button = Button::new(ElementId::Name(SharedString::from(format!(
                "log-level-{}",
                level_label(choice)
            ))))
            .ghost()
            .xsmall()
            .label(level_label(choice));
            if selected {
                // 选中态用实心变体表示（`Selectable` 的 selected 只影响无障碍元数据）
                button = button.with_variant(ButtonVariant::Primary);
            }
            head = head.child(button.on_click(move |_, _, app| {
                // 本地筛：只改门槛，不重查库。
                state_click.borrow_mut().threshold = choice;
                app.refresh_windows();
            }));
        }
        let state_refresh = state_build.clone();
        let input_refresh = input_build.clone();
        head = head
            .child(div().flex_1().min_w_0().child(Input::new(&input_build)))
            .child(
                Button::new("log-refresh")
                    .small()
                    .label("刷新")
                    .on_click(move |_, _, app| {
                        // 关键字的读取点**只在这里**：输入框变化不重查库
                        // （订阅输入事件会变成每次击键重查，代价与收益不成比例）。
                        let keyword = input_refresh.read(app).value().trim().to_string();
                        fetch(state_refresh.clone(), keyword, app);
                    }),
            );

        // ── 体：记录列表（最新在前，超出滚动） ──────────────────────
        let mut body = div()
            .v_flex()
            .w_full()
            .max_h(rems(ui::DIALOG_LOG_LIST_MAX_HEIGHT))
            .overflow_y_scrollbar()
            .gap_0p5();
        let rows: Vec<&LogRecord> = view
            .records
            .iter()
            .filter(|r| passes_threshold(r.level, view.threshold))
            .collect();
        if rows.is_empty() {
            body = body.child(
                div()
                    .py_2()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(if view.loading {
                        "读取中…"
                    } else {
                        "没有可显示的记录。"
                    }),
            );
        } else {
            for record in rows {
                let color = level_color(record.level, theme);
                body = body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .w_full()
                        .gap_2()
                        .py_0p5()
                        .text_xs()
                        .child(
                            div()
                                .w(rems(ui::DIALOG_LOG_TIME_WIDTH))
                                .flex_none()
                                .text_color(theme.colors.muted_foreground)
                                .child(short_time(&record.timestamp)),
                        )
                        .child(
                            div()
                                .w(rems(ui::DIALOG_LOG_LEVEL_WIDTH))
                                .flex_none()
                                .text_color(color)
                                .child(record.level.as_str()),
                        )
                        .child(
                            div()
                                .w(rems(ui::DIALOG_LOG_TARGET_WIDTH))
                                .flex_none()
                                .text_ellipsis()
                                .text_color(theme.colors.muted_foreground)
                                .child(record.target.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .child(record.message.clone()),
                        ),
                );
            }
        }

        dialog
            .title("日志")
            .child(head)
            .child(body)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(view.status_text()),
            )
            .child(
                div().text_xs().text_color(theme.colors.muted_foreground).child(
                    "库里的记录由后台批量写入，最新 1 秒内可能尚未出现；完整原文见日志文件。",
                ),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("log-open-dir").label("打开日志目录").on_click(
                            |_, _, app| {
                                let dir = paths::log_dir();
                                if let Err(e) = std::fs::create_dir_all(&dir) {
                                    eprintln!("[logs] 创建日志目录失败 {}: {e}", dir.display());
                                    return;
                                }
                                if let Err(e) = opener::open(&dir) {
                                    eprintln!("[logs] 打开日志目录失败 {}: {e}", dir.display());
                                }
                                let _ = app;
                            },
                        ),
                    )
                    .child(
                        Button::new("log-close")
                            .label("关闭")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .on_ok(|_, window, cx| {
                window.close_dialog(cx);
                false
            })
    });

    // 对话框层挂在**宿主视图**的 render 里（`Root::render_dialog_layer`），而 `cx.notify()`
    // 只重渲染被标脏的子树。入口是设置页里的一个按钮，没东西会顺着标脏到工作台——
    // 不主动刷一次窗口，表现就是「点了没反应」（同 `dialog_host_layer.rs` 记录的机制）。
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    // 显式列举依赖（**不通配导入**）：`super::*` 会把 `use gpui_kit::*` 带进来，
    // 于是 `#[test]` 解析成 gpui 的 `test` 宏，展开到递归上限——见 gpui-kit-dev skill。
    use super::{LEVEL_CHOICES, LogView, level_label, passes_threshold, short_time};
    use engine::{LogLevel, LogRecord};

    fn record(id: i32, level: LogLevel, message: &str) -> LogRecord {
        LogRecord {
            id,
            timestamp: "2026-09-16T13:17:02.191Z".into(),
            level,
            target: "rds_app".into(),
            message: message.into(),
            fields: None,
            file: None,
            line: None,
            session_id: "s".into(),
        }
    }

    /// 门槛是「≥」而不是「==」：这是本对话框与 `LogQuery::level` 的关键差别。
    #[test]
    fn threshold_is_inclusive() {
        assert!(passes_threshold(LogLevel::Error, Some(LogLevel::Warn)));
        assert!(passes_threshold(LogLevel::Warn, Some(LogLevel::Warn)));
        assert!(!passes_threshold(LogLevel::Info, Some(LogLevel::Warn)));
        assert!(passes_threshold(LogLevel::Trace, None), "不过滤时全都要");
    }

    /// 时间只取时分秒（列表列宽固定，塞不下完整 ISO 时间戳）。
    #[test]
    fn short_time_takes_hhmmss() {
        assert_eq!(short_time("2026-09-16T13:17:02.191Z"), "13:17:02");
        assert_eq!(short_time(""), "");
        assert_eq!(short_time("无分隔符"), "无分隔符");
    }

    /// 级别标签全覆盖（`LEVEL_CHOICES` 的每一档都能显示出来）。
    #[test]
    fn every_level_choice_has_a_label() {
        let labels: Vec<&str> = LEVEL_CHOICES.iter().map(|c| level_label(*c)).collect();
        assert_eq!(labels, ["全部", "TRACE", "DEBUG", "INFO", "WARN", "ERROR"]);
    }

    /// 状态行如实反映「可见 / 总数」，不谎报。
    #[test]
    fn status_text_counts_visible_rows() {
        let mut view = LogView {
            loaded: true,
            total: 10,
            ..Default::default()
        };
        view.records = vec![
            record(1, LogLevel::Error, "boom"),
            record(2, LogLevel::Info, "ok"),
        ];
        assert_eq!(view.visible(), 2);
        assert!(view.status_text().contains("其中 2 条达当前级别"));

        view.threshold = Some(LogLevel::Warn);
        assert_eq!(view.visible(), 1);
        assert!(view.status_text().contains("其中 1 条达当前级别"));
    }

    /// 空关键字不传给查询（别用 `LIKE '%%'` 白跑一次）。
    #[test]
    fn empty_keyword_is_not_sent() {
        let mut view = LogView::default();
        assert!(view.keyword_query().is_none());
        view.keyword = "  ".to_string(); // 输入侧已 trim，这里只验证非空透传
        assert_eq!(view.keyword_query().as_deref(), Some("  "));
    }
}
