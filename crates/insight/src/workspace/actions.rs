//! 工作区状态动作：tab 打开/切换/关闭 / 形态切换 / 对比候选勾选 / 刷新 / 开始对比。

use super::*;
use std::time::Duration;

impl super::InsightWorkspacePanel {
    /// 指向新目标：先把当前目标的画像摘要存档进 `opened`（对比聚合不重取数的前提），
    /// 再登记 / 去重，最后交 [`InsightView::set_target`]。
    ///
    /// 入口语义（D65）：「查看统计」在中间开 tab（本面板由宿主建一次），
    /// **多 tab 形态（A 方案，2026-10-30 落地）**：每个进入过洞察的目标占一个
    /// 顶部 tab（`opened` 即 tab 列表，新目标在前）；打开已有 tab 只切目标、
    /// 不重登记（去重分支）。
    pub fn open_target(&mut self, target: InsightTarget, cx: &mut Context<Self>) {
        self.archive_summary(cx);
        let key = target_key(&target);
        let already = self.opened.iter().any(|e| e.key == key);
        if let Some(entry) = self.opened.iter_mut().find(|e| e.key == key) {
            entry.target = target.clone();
        } else {
            self.opened.insert(
                0,
                OpenedTarget {
                    key,
                    target: target.clone(),
                    summary: None,
                },
            );
        }
        // 回到画像形态：打开某个目标意味着要看它的画像（D66 对齐原型入口语义）
        self.mode = WorkspaceMode::Insight;
        // 已登记目标 = 本次会话确认过（切 tab / 关 tab / 候选重开）：直接取数；
        // 新目标走探查确认窗口（C3：进入洞察先定数据量与列）
        self.view.update(cx, |view, cx| {
            if already {
                view.set_target_loaded(target, cx);
            } else {
                view.set_target(target, cx);
            }
        });
        cx.notify();
    }

    /// 顶部 tab 切换（A 方案）：切到已登记的 `opened` 目标。
    ///
    /// 复用 `open_target` 的去重分支（已登记则只更新目标 + 切画像形态）；
    /// 与新建入口的区别是这里**必然已登记**，不会新增 tab。
    pub fn activate_opened(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.opened.get(index).cloned() else {
            return;
        };
        self.open_target(entry.target, cx);
    }

    /// 顶部 tab 关闭（A 方案 ✕）：从 `opened` 移除该目标。
    ///
    /// 关的是**当前目标**时：先存档摘要，再切相邻目标（优先右侧原位，
    /// 边界回退左侧）；关掉**最后一个** tab 时清空面板回无目标空态。
    /// 对比候选是 `opened` 的子集语义（候选 = 已进入洞察的目标），
    /// 关闭的 tab 同时离开候选池。
    pub fn close_opened(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.opened.len() {
            return;
        }
        let closing_key = self.opened[index].key.clone();
        self.opened.remove(index);
        let current_key = self.view.read(cx).target().map(target_key);
        if current_key.as_deref() != Some(closing_key.as_str()) {
            cx.notify();
            return;
        }
        let next = match next_index_after_close(self.opened.len(), index) {
            Some(ix) => ix,
            None => {
                self.view.update(cx, |view, cx| view.clear_target(cx));
                cx.notify();
                return;
            }
        };
        let target = self.opened[next].target.clone();
        self.open_target(target, cx);
    }

    /// 切换形态（画像 / 对比）。切到对比不打断取数：`view` 状态保留，切回即见。
    pub fn set_mode(&mut self, mode: WorkspaceMode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            cx.notify();
        }
    }

    /// 对比候选勾选（Checkbox 回调只给布尔，这里按 key 翻转）
    pub fn toggle_candidate(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.compare_selected.iter().position(|k| k == key) {
            self.compare_selected.remove(pos);
        } else {
            self.compare_selected.push(key.to_string());
        }
        cx.notify();
    }

    /// 对比列表点行：重新打开该目标画像（并切回画像形态）
    pub fn open_candidate(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(entry) = self.opened.iter().find(|e| e.key == key) else {
            return;
        };
        let target = entry.target.clone();
        // 复用 open_target：登记 / 摘要存档 / 切回画像形态都在同一条路径
        self.open_target(target, cx);
        cx.notify();
    }

    /// 刷新（⟳）：重发当前 Tab 的取数请求（画像 / 结构按目标 Tab 各走各的）
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| view.reload(cx));
    }

    /// 开始对比（原型「开始对比」按钮）：勾选集 → 运行中 → 结果。
    ///
    /// 阶段②聚合会话内已完成的画像摘要、**不重新取数**，结果瞬时可得；
    /// 「运行中」给 600ms 示意（对齐原型三态交互），随后落成结果表。
    pub fn start_compare(&mut self, cx: &mut Context<Self>) {
        if self.compare_selected.is_empty() || self.compare_running {
            return;
        }
        self.compare_running = true;
        self.compare_done = false;
        cx.notify();
        let weak = cx.entity().downgrade();
        let timer = cx.background_executor().timer(Duration::from_millis(600));
        cx.spawn(async move |_, cx| {
            timer.await;
            let _ = weak.update(cx, |panel, cx| {
                panel.compare_running = false;
                panel.compare_done = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// 重新对比：清掉结果态回到候选态（勾选保留）
    pub(crate) fn reset_compare(&mut self, cx: &mut Context<Self>) {
        self.compare_running = false;
        self.compare_done = false;
        cx.notify();
    }
}
