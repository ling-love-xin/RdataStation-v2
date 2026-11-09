//! 元数据异步搜索：防抖发送、批次守卫、结果回填泵。
//!
//! 发送（[`QuickOpenPalette::schedule_search`]）只在「词达门槛且与已发批次不同」时
//! 防抖发出；回填（[`QuickOpenPalette::pump_results`]）由常驻泵按 60ms（打开）/
//! 400ms（关闭）节奏拉取，序号 + 词双守卫丢弃过期批次。

use std::time::Duration;

use gpui_kit::*;

use crate::quick_open::model;
use crate::ui;

use super::QuickOpenPalette;
use super::files::FilesSnapshot;

impl QuickOpenPalette {
    /// 给后台排一次跨连接索引搜索（防抖 150ms；**单字符不发**）。
    pub(super) fn schedule_search(&mut self, query: &model::Query, cx: &mut Context<Self>) {
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
        // 档位：`#` 走内容档（FTS，注释 / 数据类型），其余走名称档。
        // 名称档**只查前缀段**：引擎按 `mode` 单段查询（`StartsWith` 不再做「前缀优先 + 中缀兜底」），
        // 依据 `store::persistence::metadata_cache::search_index` 的分支。
        let kind = match query.mode {
            model::Mode::FullText => database::nav_jobs::SearchKind::FullText,
            _ => database::nav_jobs::SearchKind::Name,
        };
        self.sent_query = Some(needle.clone());
        self.searching = true;
        let executor = cx.background_executor().clone();
        let weak = cx.entity().downgrade();
        // 序号守卫：每次发送自增 `seq`，回填时序号与词都要对上（任一对不上即丢弃）。
        // 注意**同词不会重发**：上面 `sent_query` 只比词，所以换档（`#` ↔ 无前缀）或连接目标变化
        // 都不会触发重搜——这是已知语义缺口（见 `quick-open-implementation-audit.md` D1），
        // 不是「防抖窗口内的重排会重发」的旧行为。
        let seq = self.search_seq + 1;
        self.search_seq += 1;
        // 防抖：句柄被下一枚替换即取消（`Task` drop = 取消）
        let task = cx.spawn(async move |_this, cx| {
            executor
                .timer(Duration::from_millis(ui::QUICK_OPEN_SEARCH_DEBOUNCE_MS))
                .await;
            let _ = weak.update(cx, |_this, _cx| {
                database::nav_jobs::enqueue_search(
                    database::nav_jobs::SearchConsumer::QuickOpen,
                    seq,
                    kind,
                    &needle,
                    // 快速打开（Quick Open）：无 `.*` / `Aa` 开关，走默认关。
                    false,
                    false,
                    // D11：无匹配模式 / 注释 / 定义开关，走默认前缀、不搜语料。
                    database::nav_jobs::NameMatchMode::StartsWith,
                    false,
                    false,
                    project_root.as_deref(),
                    targets,
                );
            });
        });
        self.debounce = Some(task);
    }

    /// 启动结果回填泵（构造时一次；关闭期间降频空转，视图销毁即退出）。
    pub(super) fn ensure_pump(&mut self, cx: &mut Context<Self>) {
        if self.pump.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
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
            }
        });
        self.pump = Some(task);
    }

    /// 取走元数据搜索结果 + 文件清单（过期批次丢弃；不碰 UI，只置脏标记）。
    ///
    /// `pub(crate)`：定位的窗口级测试直接调它（键位与泵的定时器各自已有覆盖），
    /// 免得测一个业务逻辑还要跟 60ms 的轮询时钟赛跑。
    pub(crate) fn pump_results(&mut self, cx: &mut Context<Self>) {
        let results =
            database::nav_jobs::drain_search_results(database::nav_jobs::SearchConsumer::QuickOpen);
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
            // 过期批次：序号不是最新（同词重排后重发）或词已被改（收尾交给当前词）。
            if result.seq != self.search_seq || result.query != current {
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
}
