//! 「归档到分析库…」的确认窗口（铁律 H 组第 35 条前半）。
//!
//! 为什么要一个窗口而不是直接写：归档的目标表名是**会变的**——默认 `<源>_<表>`，
//! 撞名要由用户改（项目分析库里的持久表是用户的，覆盖得由用户再说一次）。
//! 同时把落点讲明（`{项目}/.RSmeta/analytics.duckdb`、写持久表 + 登记资产），
//! 用户在点之前就知道"这一下会把数据搬到项目里"。
//!
//! 写库不在 UI 线程（第 25 条）：确认后只入队（[`crate::services::archive_jobs`]），
//! 窗口立刻关、状态栏先说一句；结果由面板自己的小泵取回。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, AppContext as _, ParentElement as _, Styled as _, Window, div, px};

use crate::components::data_objects::SourceRef;
use crate::components::dialog_helpers::{standard_dialog_body, standard_dialog_footer};
use crate::panels::{DataObjectsTab, Shared};

/// 打开归档窗口。`table` / `source_name` 只用于文案与默认名，真正写库的是 `qualified`。
pub fn open_archive_dialog(
    shared: Shared,
    entity: gpui_kit::Entity<DataObjectsTab>,
    source: SourceRef,
    source_name: String,
    table: String,
    qualified: String,
    window: &mut Window,
    cx: &mut App,
) {
    let default_name = engine::analysis::materialize::archive_table_name(&source_name, &table);
    let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("项目分析库里的表名"));
    if !default_name.is_empty() {
        let value = default_name.clone();
        name_input.update(cx, |state, cx| state.set_value(value, window, cx));
    }
    let summary = format!("{source_name} · {table} → 项目分析库（.RSmeta/analytics.duckdb）");

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let name_empty = name_input.read(cx).value().trim().is_empty();

        dialog
            .title("归档到分析库")
            .w(px(560.0))
            .child(standard_dialog_body(
                div()
                    .v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("{summary}：写一张持久表，并登记成表型资产（两步一起做）。")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("只搬这一张表：源库那边一行不动，分析实例里怎么读，这一下就怎么搬（类型原样保留）。"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("表名"),
                    )
                    .child(Input::new(&name_input))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("默认 <源>_<表>。撞名不会覆盖——改个名字再归档。写库在后台线程，结果进状态栏。"),
                    )
                    .when(name_empty, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.danger)
                                .child("表名不能为空"),
                        )
                    }),
            ))
            .footer(standard_dialog_footer(
                {
                    let shared = shared.clone();
                    let entity = entity.clone();
                    let source = source.clone();
                    let source_name = source_name.clone();
                    let table = table.clone();
                    let qualified = qualified.clone();
                    let name_input = name_input.clone();
                    Button::new("archive-ok")
                        .with_variant(ButtonVariant::Primary)
                        .label("归档")
                        .on_click(move |_, window, cx| {
                            let dest_table = name_input.read(cx).value().trim().to_string();
                            if dest_table.is_empty() {
                                return;
                            }
                            let Some(root) = shared.project_root() else {
                                shared.say_error(
                                    "归档需要打开项目：落点是 {项目}/.RSmeta/analytics.duckdb",
                                    cx,
                                );
                                return;
                            };
                            let job = crate::services::archive_jobs::start(
                                root,
                                source.clone(),
                                source_name.clone(),
                                table.clone(),
                                qualified.clone(),
                                dest_table.clone(),
                            );
                            shared.say(
                                format!("正在归档「{table}」→ 项目分析库「{dest_table}」…"),
                                cx,
                            );
                            entity.update(cx, |tab, cx| tab.watch_archive(job, cx));
                            window.close_dialog(cx);
                        })
                },
                Some(
                    Button::new("archive-cancel")
                        .label("取消")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            ))
            .on_cancel(|_, _, _| true)
    });
}
