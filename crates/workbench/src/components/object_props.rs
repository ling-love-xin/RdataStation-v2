//! 数据对象的「属性」窗口（右键 → 属性…）。
//!
//! 为什么单独开窗而不是塞进行里（铁律 E 组第 20 条：跨模块交互走右键）：
//! 行只有两行文字，装不下"身份键 / 通道 / 寿命 / 状态 / 列结构"这些**判断依据**；
//! 而用户要回答的问题恰恰是"这一行到底是哪个东西"——尤其是同名不同来源时。
//!
//! 字段口径：
//! - 名称 / 身份键 / 段 / 小标 / 通道 / 寿命 / 可用性 / 关联版本 → 全来自 [`Object`]，
//!   渲染期不再查库（面板纪律：渲染零 I/O）；
//! - 列结构 → **打开窗口时读一次**（项目分析库里的表才有），读不到就说原因；
//! - 引用名 → `sql_ref` 有才给（源库表 / 文件库行本来就抄不出去，见铁律 G 第 30 条）。
//!
//! 「复制引用名」在这里也有一个：右键菜单点一下就走，但属性窗口开着时手已经在窗口里了。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, ClipboardItem, ParentElement as _, Styled as _, Window, div, px};

use crate::components::data_objects::{self, Object};
use crate::components::dialog_helpers::{standard_dialog_body, standard_dialog_footer};
use crate::components::file_actions::bare_table_name;
use crate::panels::Shared;

/// 打开属性窗口（内容在**打开这一次**算好，窗口内渲染纯静态）。
pub fn open_object_props(shared: Shared, object: Object, window: &mut Window, cx: &mut App) {
    let mut rows: Vec<(String, String)> = vec![
        ("名称".to_string(), object.name.clone()),
        ("身份键".to_string(), object.key.clone()),
        ("所在段".to_string(), object.group.title().to_string()),
        (
            "身份小标".to_string(),
            if object.pill.is_empty() {
                "—".to_string()
            } else {
                object.pill_text()
            },
        ),
        ("通道".to_string(), object.channel.label().to_string()),
        ("寿命".to_string(), object.life.label().to_string()),
        ("可用性".to_string(), object.state.label()),
    ];
    if let Some(version) = object.version {
        rows.push(("关联版本".to_string(), format!("v{version}（只取最新版）")));
    }
    if let Some(reference) = object.sql_ref.as_ref().filter(|r| !r.trim().is_empty()) {
        rows.push(("SQL 引用名".to_string(), reference.clone()));
    }
    if let Some(note) = Some(object.note.as_str()).filter(|n| !n.trim().is_empty()) {
        rows.push(("副标题".to_string(), note.to_string()));
    }
    // 文件型（`f_*` 视图）：把本体路径从文件索引里取出来一次性写进来
    if let Some(alias) = data_objects::fileview_alias(&object.key) {
        let path = engine::duckdb::file_view::file_entries()
            .into_iter()
            .find(|(seen, _)| seen == &alias)
            .map(|(_, path)| path);
        rows.push((
            "本体文件".to_string(),
            path.unwrap_or_else(|| format!("{alias}（会话清单里已没有它）")),
        ));
    }

    // 列结构：只有「项目分析库里的表」读得到（临时结果集在内存、全局库是另一个文件）
    let in_project_analytics = matches!(
        object.key.split(':').next(),
        Some("persistent" | "project" | "asset")
    );
    let columns: Result<Vec<(String, String, bool)>, String> = match object
        .sql_ref
        .as_ref()
        .filter(|_| in_project_analytics)
    {
        Some(reference) => crate::services::mock_generator::table_columns_in_analytics(
            shared.project_root().as_deref(),
            &bare_table_name(reference),
        ),
        None => Err(if object.group == data_objects::Group::Source {
            "源行：表要展开所选连接才读得到（这一行本身不是表）".to_string()
        } else {
            "这一行不在项目分析库里：列结构要按需另开查询读（见右侧「通道」一列）".to_string()
        }),
    };

    let title = format!("{} · 属性", object.name);
    let copy_text = object.sql_ref.clone();
    let label = object.name.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let mut list = DescriptionList::new().label_width(px(96.0)).columns(1);
        for (key, value) in rows.clone() {
            list = list.item(key, value, 1);
        }
        // 列结构区：「读到了就列表，读不到就说原因」——不假装这张表没有列
        let columns_block: gpui_kit::Div = match columns.clone() {
            Ok(columns) => {
                let mut block = div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("列结构 · {} 列", columns.len())),
                    );
                for (name, data_type, nullable) in columns {
                    block = block.child(
                        div()
                            .h_flex()
                            .items_baseline()
                            .gap_2()
                            .text_xs()
                            .child(div().child(name))
                            .child(
                                div()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(data_type),
                            )
                            .when(nullable, |this| {
                                this.child(
                                    div()
                                        .text_color(theme.colors.muted_foreground)
                                        .child("可空"),
                                )
                            }),
                    );
                }
                block
            }
            Err(why) => div()
                .v_flex()
                .w_full()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("列结构"),
                )
                .child(div().text_xs().child(why)),
        };

        dialog
            .title(title.clone())
            .w(px(560.0))
            .child(standard_dialog_body(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .child(div().w_full().child(list))
                    .child(div().h(px(1.0)).w_full().bg(theme.colors.border))
                    .child(columns_block),
            ))
            .footer(standard_dialog_footer(
                {
                    let shared = shared.clone();
                    let copy_text = copy_text.clone();
                    let label = label.clone();
                    Button::new("object-props-copy")
                        .label("复制引用名")
                        .on_click(move |_, _window, cx| match copy_text.clone() {
                            Some(reference) => {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    reference.clone(),
                                ));
                                shared.say(format!("已复制引用名：{reference}"), cx);
                            }
                            None => shared.say_error(
                                format!("{label}：这一行没有可抄的引用名（源库表要带连接上下文）"),
                                cx,
                            ),
                        })
                },
                Some(
                    Button::new("object-props-close")
                        .label("关闭")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            ))
            .on_cancel(|_, _, _| true)
    });
}
