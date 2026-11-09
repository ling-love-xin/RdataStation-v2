//! 测试专用：只读访问器与程序化设值辅助（本文件由 `#[cfg(test)] mod` 声明整体门控，
//! 不进生产构建）。

use gpui_kit::*;

use super::QuickOpenPalette;

impl QuickOpenPalette {
    /// 测试只读访问器：输入框当前文本。
    pub(crate) fn query_text(&self, cx: &App) -> String {
        self.input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// 测试只读访问器：结果行数。
    pub(crate) fn row_count(&self, cx: &App) -> usize {
        self.list
            .as_ref()
            .map(|list| list.read(cx).delegate().row_count())
            .unwrap_or(0)
    }

    /// 测试只读访问器：结果行的业务键（按渲染顺序）。
    pub(crate) fn row_keys(&self, cx: &App) -> Vec<String> {
        self.list
            .as_ref()
            .map(|list| list.read(cx).delegate().keys())
            .unwrap_or_default()
    }

    /// 测试只读访问器：选中的行业务键。
    pub(crate) fn selected_key(&self) -> Option<String> {
        self.selected_key.clone()
    }

    /// 测试只读访问器：当前锁定模式（`None` = 默认档，文本前缀解析）。
    pub(crate) fn current_mode(&self) -> Option<crate::quick_open::model::Mode> {
        self.mode_override
    }

    /// 测试只读访问器：是否处于「元数据搜索中」。
    pub(crate) fn searching(&self) -> bool {
        self.searching
    }

    /// 测试只读访问器：已发批次的搜索词（`None` = 一次都没发）。
    pub(crate) fn sent_query(&self) -> Option<String> {
        self.sent_key.as_ref().map(|key| key.needle.clone())
    }

    /// 测试只读访问器：已发批次的档（`true` = 全文 FTS；Goto 与 Default 同为名称档）。
    pub(crate) fn sent_fulltext(&self) -> Option<bool> {
        self.sent_key.as_ref().map(|key| key.fulltext)
    }

    /// 测试只读访问器：最新已发批次的序号（伪造回填批次时对齐用）。
    pub(crate) fn search_seq(&self) -> u64 {
        self.search_seq
    }

    /// 测试辅助：程序化设输入值并刷新。
    ///
    /// `set_value` 不发 `InputEvent::Change`，须手动 `refresh`——这与「档位 chip 点击」
    /// 走的是同一条路径（headless 下 `#` 键无法经 keystroke 模拟）。
    pub(crate) fn test_set_query(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.input.clone() {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
        self.refresh(window, cx);
    }
}
