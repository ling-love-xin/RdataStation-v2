//! 编辑器宿主面板 —— 书签与编辑模式（load_bookmarks/sync_bookmarks/on_toggle_bookmark/set_edit_mode/enforce_edit_mode）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。


use gpui_kit::base::input::Position;
use gpui_kit::component::input::{
    EditorState, RangeDecoration,
    RangeDecorationStyle,
};
// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法


use crate::bookmark;
use crate::commands::{
    AddBookmarkNote, NextBookmark, PrevBookmark, ToggleBookmark,
};
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）


use super::*;
impl EditorHostPanel {

    /// 【书签】本文档书签的 sidecar 路径（`None` = 未命名文档，无落盘）
    fn bookmark_path(&self) -> Option<std::path::PathBuf> {
        self.document_path()
    }

    /// 【书签】从 sidecar 读入本文档书签并刷新标记（事件路径；未命名文档 / 无 meta = 空）
    ///
    /// 挂在 `on_added_to`（进 Dock 即读一次）。`read_meta` 对缺失文件返回默认空 meta，
    /// 所以「没建过书签的文件」与「空书签」同口径，不报错。
    pub(crate) fn load_bookmarks(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.bookmark_path() else {
            return;
        };
        match file_meta::read_meta(&path) {
            Ok(meta) => {
                let mut bookmarks = meta.bookmarks;
                bookmark::normalize(&mut bookmarks);
                self.shared.set_bookmarks(&self.document, bookmarks);
            }
            Err(_) => {
                self.set_message(Some("书签读取失败".to_string()), cx);
            }
        }
        self.refresh_bookmark_decorations(cx);
    }

    /// 【书签】把本文档当前书签写回 sidecar（事件路径；未命名文档 = 不落盘）
    ///
    /// 快照式：只写当前书签列表（默认说明 = 行号），不记历史。
    fn sync_bookmarks(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.bookmark_path() else {
            return;
        };
        let mut meta = file_meta::FileMeta::default();
        meta.bookmarks = self.shared.bookmarks(&self.document);
        if let Err(_) = file_meta::write_meta(&path, &meta) {
            self.set_message(Some("书签写入失败".to_string()), cx);
        }
    }

    /// 【书签】把本文档书签刷成行级 Fill 标记（懒创建集合；样式同隐藏字符先例）
    ///
    /// 书签行号 1 起（sidecar 口径）→ 内核 0 起换算；超出文档行数的书签直接跳过
    /// （行号漂移暂不校正，见 rds-file-meta 文档）。
    fn refresh_bookmark_decorations(&mut self, cx: &mut Context<Self>) {
        use gpui_kit::component::input::RopeExt as _;
        // 懒创建集合（先 update 再取色：`cx.theme()` 借用了 `cx`，见 gpui-kit-dev 规范）
        if self.bookmark_decorations.is_none() {
            self.bookmark_decorations = Some(self.editor.update(cx, |state, cx| {
                state.create_range_decorations_collection(Vec::new(), cx)
            }));
        }
        let color = cx.theme().colors.primary.opacity(0.14);
        let lines = bookmark::lines(&self.shared.bookmarks(&self.document));
        let rope = self.editor.read(cx).text().clone();
        let decorations = lines
            .into_iter()
            .filter_map(|line_1| {
                let row = line_1.saturating_sub(1);
                if row >= rope.lines_len() {
                    return None;
                }
                let range = rope.line_start_offset(row)..rope.line_end_offset(row);
                Some(
                    RangeDecoration::new(range)
                        .with_style(RangeDecorationStyle::Fill)
                        .with_color(color),
                )
            })
            .collect::<Vec<_>>();
        if let Some(collection) = &self.bookmark_decorations {
            collection.set(decorations, cx);
        }
    }

    /// 【书签】当前光标所在行（1 起；编辑器内核行号 0 起）
    fn cursor_line_1(&self, cx: &App) -> usize {
        use gpui_kit::component::input::RopeExt as _;
        let state = self.editor.read(cx);
        let offset = state.selected_range().end;
        state.text().offset_to_point(offset).row + 1
    }

    /// 【方案 A】文本变更后取内核记录的编辑行增量，校正 shared 书签并回写。
    ///
    /// 在 `InputEvent::Change` 回调里调用：内核在 `on_text_changed` 时记录了本次编辑的
    /// `LineDelta`，这里 `take` 出来（消费即清空，避免重复应用），按「编辑区之前不动 /
    /// 区内的锚定到编辑区首行 / 区后的整体平移」校正书签。书签有实际位移才重绘 + 落 sidecar。
    pub(crate) fn adjust_bookmarks_for_edit(&mut self, state: &Entity<EditorState>, cx: &mut Context<Self>) {
        let Some(delta) = state.update(cx, |s, _| s.take_last_edit_delta()) else {
            return;
        };
        let mut bookmarks = self.shared.bookmarks(&self.document);
        if bookmark::adjust_for_edit(&mut bookmarks, delta) {
            self.shared.set_bookmarks(&self.document, bookmarks);
            self.refresh_bookmark_decorations(cx);
            self.sync_bookmarks(cx);
        }
    }

    /// 【书签】`Ctrl+Shift+B`：切换当前行书签（书签不改文本，只读文档也可用）
    pub(crate) fn on_toggle_bookmark(
        &mut self,
        _: &ToggleBookmark,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line_1 = self.cursor_line_1(cx);
        let mut bookmarks = self.shared.bookmarks(&self.document);
        bookmark::toggle(&mut bookmarks, line_1);
        bookmark::normalize(&mut bookmarks);
        self.shared.set_bookmarks(&self.document, bookmarks);
        self.refresh_bookmark_decorations(cx);
        self.sync_bookmarks(cx);
        cx.notify();
    }

    /// 【书签】`F8`：跳转到下一个书签（循环）
    pub(crate) fn on_next_bookmark(
        &mut self,
        _: &NextBookmark,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.jump_to_bookmark(bookmark::next_after, window, cx);
    }

    /// 【书签】`Shift+F8`：跳转到上一个书签（循环）
    pub(crate) fn on_prev_bookmark(
        &mut self,
        _: &PrevBookmark,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.jump_to_bookmark(bookmark::prev_before, window, cx);
    }

    /// 【书签】把光标移到某个书签行（内核 0 起）；无书签给一条提示，不静默
    fn jump_to_bookmark(
        &mut self,
        pick: fn(&[file_meta::Bookmark], usize) -> Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (from_line_1, bookmarks) = {
            let from = self.cursor_line_1(cx);
            (from, self.shared.bookmarks(&self.document))
        };
        let Some(target_1) = pick(&bookmarks, from_line_1) else {
            self.set_message(Some("没有书签".to_string()), cx);
            return;
        };
        self.jump_to_line(target_1, window, cx);
    }

    /// 【书签】把光标移到指定行（1 起 → 内核 0 起；书签管理器「跳转」用）
    pub(crate) fn jump_to_line(
        &mut self,
        line_1: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target_0 = line_1.saturating_sub(1);
        // `Position` 是 lsp-types 的 (line, character)，字段为 `u32`；行号在 u32 范围内
        let target_0 = u32::try_from(target_0).unwrap_or(0);
        self.editor.update(cx, |state, cx| {
            state.set_cursor_position(Position::new(target_0, 0), window, cx);
        });
        cx.notify();
    }

    /// 【书签】应用某行书签说明（事件路径：`set_label` + 重绘 + 落盘 sidecar）
    ///
    /// 右键「添加书签/说明…」与书签管理器「编辑」都汇到这里：有则改 label、
    /// 无则插入一枚说明书签；空说明如实保存（用户显式输入，与默认行号可区分）。
    pub(crate) fn apply_bookmark_label(
        &mut self,
        line: usize,
        label: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut bookmarks = self.shared.bookmarks(&self.document);
        if bookmark::set_label(&mut bookmarks, line, label) {
            self.shared.set_bookmarks(&self.document, bookmarks);
            self.refresh_bookmark_decorations(cx);
            self.sync_bookmarks(cx);
        }
        cx.notify();
    }

    /// 【书签】删除某行书签（事件路径：`remove_line` + 重绘 + 落盘）
    pub(crate) fn delete_bookmark(
        &mut self,
        line: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut bookmarks = self.shared.bookmarks(&self.document);
        if bookmark::remove_line(&mut bookmarks, line) {
            self.shared.set_bookmarks(&self.document, bookmarks);
            self.refresh_bookmark_decorations(cx);
            self.sync_bookmarks(cx);
        }
        cx.notify();
    }

    /// 【书签】右键「添加书签/说明…」：当前行弹说明输入框（预填现有说明或行号）
    ///
    /// 不依赖折叠能力（Analysis 模式也能标记）；折叠区域的说明经此落成书签，
    /// 「只有写了说明才被记录」的口径由 [`Self::apply_bookmark_label`] 保证。
    pub(crate) fn on_add_bookmark_note(
        &mut self,
        _: &AddBookmarkNote,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line_1 = self.cursor_line_1(cx);
        let initial = self
            .shared
            .bookmarks(&self.document)
            .iter()
            .find(|b| b.line == line_1)
            .map(|b| b.label.clone())
            .unwrap_or_else(|| line_1.to_string());
        let entity = cx.entity();
        crate::view::dialogs::open_bookmark_note_dialog(
            window,
            cx,
            line_1,
            initial,
            move |label, window, cx| {
                entity.update(cx, |panel, cx| {
                    panel.apply_bookmark_label(line_1, label, window, cx);
                });
            },
        );
    }

    /// 【书签】工具栏「书签」按钮：打开书签管理器浮层（列表 + 跳转 / 编辑 / 删除）
    pub(crate) fn open_bookmark_manager(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bookmarks = self.shared.bookmarks(&self.document);
        let entity = cx.entity();
        // 每个回调各持一份 entity（`move` 闭包在定义时即捕获，不能共享同一份）
        let jump_entity = entity.clone();
        let edit_entity = entity.clone();
        let delete_entity = entity.clone();
        crate::view::dialogs::open_bookmark_manager(
            window,
            cx,
            bookmarks,
            // 跳转：定位到书签行
            move |line, window, cx| {
                jump_entity.update(cx, |panel, cx| panel.jump_to_line(line, window, cx));
            },
            // 编辑：换输入框弹窗改说明（先关管理器浮层再开）
            move |line, current, window, cx| {
                let edit_entity = edit_entity.clone();
                crate::view::dialogs::open_bookmark_note_dialog(
                    window,
                    cx,
                    line,
                    current,
                    move |label, window, cx| {
                        edit_entity.update(cx, |panel, cx| {
                            panel.apply_bookmark_label(line, label, window, cx);
                        });
                    },
                );
            },
            // 删除：移除该行书签
            move |line, window, cx| {
                delete_entity.update(cx, |panel, cx| panel.delete_bookmark(line, window, cx));
            },
        );
    }

    /// 【T2c-3 收尾】切换编辑内容模式（行编辑 / 列编辑）。只改面板态并重绘；
    /// 切回行编辑时的多光标收拢由调用方（菜单 on_click 持有 window）经 [`Self::enforce_edit_mode`] 触发。
    pub(crate) fn set_edit_mode(&mut self, mode: EditMode, cx: &mut Context<Self>) {
        if self.edit_mode == mode {
            return;
        }
        self.edit_mode = mode;
        cx.notify();
    }

    /// 【T2c-3 收尾】按当前编辑模式对齐内核光标：行编辑 = 强制单光标（多光标收拢到活动光标）；
    /// 列编辑 = 保持多光标。需要 `window`（内核 `collapse_cursors` 的签名要求，实参未用）。
    pub(crate) fn enforce_edit_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit_mode != EditMode::Row {
            return;
        }
        self.editor.update(cx, |state, cx| {
            if state.cursor_count() > 1 {
                state.collapse_cursors(window, cx);
            }
        });
    }
}
