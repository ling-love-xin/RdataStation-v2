//! 数据对象「导出」窗口（右键 → 导出… / 存到草稿…）。
//!
//! 为什么要有窗口（2026-11-24 用户拍板）：导出有三个会变的东西——**目的地、
//! 格式、文件名**，原来把格式做成工具栏上的全局轮换按钮、落点写死目录，
//! 等于让用户"先记住当前格式再点导出"，而且没法指定路径。三件事一次问清，
//! 格式就只是"这一次导出"的属性，不再冒充全局状态。
//!
//! 目的地三档（铁律 E 组第 22/24 条）：
//! - `results/`（全局）：跨项目可见，给别人复用；
//! - `{项目}/scratchpad/`（草稿）：只在本项目，之后可升级为资产；
//! - 指定目录：写磁盘任意位置，不进资源管理。
//!
//! 写盘仍走同一条引擎通道（`COPY … TO`），回执进状态栏——窗口只负责收参数。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, AppContext as _, ParentElement as _, Styled as _, Window, div, px};

use engine::duckdb::export::ExportFormat;

use crate::components::dialog_helpers::{standard_dialog_body, standard_dialog_footer};
use crate::components::file_actions::{export_file_stem, export_table_to_path, bare_table_name};
use crate::panels::Shared;

/// 落点（窗口里的三选一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Global,
    Scratchpad,
    Custom,
}

impl ExportKind {
    fn title(self) -> &'static str {
        match self {
            Self::Global => "全局 results/",
            Self::Scratchpad => "项目草稿箱 scratchpad/",
            Self::Custom => "指定目录…",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Global => "跨项目可见；给别人复用的走这条",
            Self::Scratchpad => "只在本项目；之后可升级为资产",
            Self::Custom => "写到磁盘任意位置，不进资源管理",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Self::Scratchpad => "存到草稿",
            _ => "导出",
        }
    }
}

struct State {
    kind: ExportKind,
    format: ExportFormat,
    custom_dir: Option<PathBuf>,
}

/// 打开导出窗口。
///
/// `preset` 决定初始落点：「导出…」给 Global，「存到草稿…」给 Scratchpad——
/// 同一张窗口，两个入口各带着自己那档进来，用户不用先点一次再改。
pub fn open_export_dialog(
    shared: Shared,
    label: String,
    sql_ref: String,
    preset: ExportKind,
    window: &mut Window,
    cx: &mut App,
) {
    let table = bare_table_name(&sql_ref);
    let stem = export_file_stem(&table);
    let file_input = cx.new(|cx| InputState::new(window, cx).placeholder("文件名（不含扩展名）"));
    if !stem.is_empty() {
        let value = stem.clone();
        file_input.update(cx, |state, cx| state.set_value(value, window, cx));
    }
    let state = Rc::new(RefCell::new(State {
        kind: preset,
        format: ExportFormat::Csv,
        custom_dir: None,
    }));

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let snapshot = state.borrow();
        let kind = snapshot.kind;
        let format = snapshot.format;
        let custom = snapshot.custom_dir.clone();
        drop(snapshot);
        let file_name_empty = file_input.read(cx).value().trim().is_empty();

        // 目的地：三档一档一行（像原型 08 的 `.mode` 卡），不给下拉
        let mut destinations = div().v_flex().w_full().gap_1();
        for option in [ExportKind::Global, ExportKind::Scratchpad, ExportKind::Custom] {
            let active = option == kind;
            let state_for_click = state.clone();
            destinations = destinations.child(
                Button::new(format!("export-dest-{}", option.title()))
                    .ghost()
                    .toggled(active)
                    .label(format!("{} · {}", option.title(), option.hint()))
                    .on_click(move |_, window, _cx| {
                        state_for_click.borrow_mut().kind = option;
                        window.refresh();
                    }),
            );
        }

        // 自定义目录：选了才给确认（没选时确认按钮自己会说清）
        let choose_dir = (kind == ExportKind::Custom).then(|| {
            let state_for_click = state.clone();
            div().h_flex().items_center().gap_2().w_full().child(
                Button::new("export-choose-dir")
                    .label("选择目录…")
                    .on_click(move |_, _window, _cx| {
                        let picked = rfd::FileDialog::new()
                            .set_title("选择导出目录")
                            .pick_folder();
                        if let Some(dir) = picked {
                            state_for_click.borrow_mut().custom_dir = Some(dir);
                            _window.refresh();
                        }
                    }),
            )
        });
        let custom_text = custom
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "还没选目录".to_string());

        // 格式：同样是三选一，扩展名由格式决定（文件名里不再重复写扩展名）
        let mut formats = div().h_flex().w_full().gap_1();
        for option in [ExportFormat::Csv, ExportFormat::Parquet, ExportFormat::Json] {
            let active = option == format;
            let state_for_click = state.clone();
            formats = formats.child(
                Button::new(format!("export-fmt-{}", option.extension()))
                    .ghost()
                    .toggled(active)
                    .label(option.extension().to_uppercase())
                    .on_click(move |_, window, _cx| {
                        state_for_click.borrow_mut().format = option;
                        window.refresh();
                    }),
            );
        }

        let summary = div()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child(format!("{label} — 导出只把这张表写出去：不搬内存、不改数据对象"));

        dialog
            .title("导出这张表")
            .w(px(520.0))
            .child(standard_dialog_body(
                div()
                    .v_flex()
                    .w_full()
                    .gap_2()
                    .child(summary)
                    .child(field_label(theme, "目的地"))
                    .child(destinations)
                    .children(choose_dir)
                    .when(kind == ExportKind::Custom, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child(custom_text.clone()),
                        )
                    })
                    .child(field_label(theme, "格式"))
                    .child(formats)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("只收 csv / parquet / json；写盘走引擎通道，回执进状态栏。"),
                    )
                    .child(field_label(theme, format!("文件名（自动加 .{}）", format.extension())))
                    .child(Input::new(&file_input))
                    .when(file_name_empty, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.danger)
                                .child("文件名不能为空"),
                        )
                    }),
            ))
            .footer(standard_dialog_footer(
                {
                    let shared = shared.clone();
                    let sql_ref = sql_ref.clone();
                    let state = state.clone();
                    let file_input = file_input.clone();
                    Button::new("export-ok")
                        .with_variant(ButtonVariant::Primary)
                        .label("导出")
                        .on_click(move |_, window, cx| {
                            let stem = file_input.read(cx).value().trim().to_string();
                            if stem.is_empty() {
                                return;
                            }
                            let snapshot = state.borrow();
                            let kind = snapshot.kind;
                            let format = snapshot.format;
                            let custom_dir = snapshot.custom_dir.clone();
                            drop(snapshot);
                            let dir = match kind {
                                ExportKind::Global => {
                                    Some(crate::services::query_export::default_export_dir())
                                }
                                ExportKind::Scratchpad => {
                                    shared.project_root().map(|root| root.join("scratchpad"))
                                }
                                ExportKind::Custom => custom_dir,
                            };
                            let Some(dir) = dir else {
                                shared.say_error(
                                    if kind == ExportKind::Scratchpad {
                                        "存到草稿：当前没有打开的项目".to_string()
                                    } else {
                                        "导出：先选一个目录".to_string()
                                    },
                                    cx,
                                );
                                return;
                            };
                            if let Err(error) = std::fs::create_dir_all(&dir) {
                                shared.say_error(
                                    format!("{}：创建目录失败（{error}）", kind.verb()),
                                    cx,
                                );
                                return;
                            }
                            let path = dir.join(format!("{stem}.{}", format.extension()));
                            if export_table_to_path(&shared, &sql_ref, &path, kind.verb(), cx) {
                                window.close_dialog(cx);
                            }
                        })
                },
                Some(
                    Button::new("export-cancel")
                        .label("取消")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            ))
            .on_cancel(|_, _, _| true)
    });
}

fn field_label(theme: &gpui_kit::component::Theme, text: impl Into<String>) -> gpui_kit::Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.into())
}
