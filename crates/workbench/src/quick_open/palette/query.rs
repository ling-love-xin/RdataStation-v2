//! 输入 → 查询：档位解析、源汇合、分组推送、选中跟随。
//!
//! [`QuickOpenPalette::refresh`] 是权威重算点（输入变化 / 打开 / 回填后调用），
//! 末尾触发两个异步源（元数据搜索 / 文件清单）。

use gpui_kit::*;

use crate::quick_open::model;

use super::QuickOpenPalette;
use super::selection::mirror_selection;

impl QuickOpenPalette {
    /// 重算结果并推给列表（输入变化 / 打开 / 回填后调用）。
    ///
    /// 选中按**业务键**跟随：键还在就保留，否则落到第一行（异步回填不抢用户位置）。
    pub(super) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(input), Some(list)) = (self.input.clone(), self.list.clone()) else {
            return;
        };
        // 模式锁定：Goto 档只取文本作词，`>` / `#` 前缀不切档；默认档照常解析前缀。
        let raw = input.read(cx).value();
        let query = match self.mode_override {
            Some(mode) => model::Query {
                mode,
                needle: model::parse(&raw).needle,
            },
            None => model::parse(&raw),
        };
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
        // 占位文案随档位切换（Goto 只描述可跳转对象类别；默认档提示前缀命令）。
        let placeholder = match query.mode {
            model::Mode::Goto => "跳转到对象（表 · 视图 · 例程 · 序列 · 触发器）",
            _ => "搜索对象、命令…（> 命令 · # 全文）",
        };
        input.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx)
        });
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
}
