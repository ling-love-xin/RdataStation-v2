//! 管理标签覆盖层对话框（规格 `docs/architecture/tag/tag-system-spec.md` §12 #3 标签侧）。
//!
//! 两个操作：
//! - **批量重命名**：行「重命名」→ 单 Input 表单对话框 → `TagTable::rename_tag`
//!   （改定义名，关联按 `tag_id` 自然跟随——导航 / 分析资源 / 草稿箱的 chip 与 facet 自动一致）；
//! - **清理空标签**：底部按钮（引用数为 0 的活跃标签数）→ 确认 → 逐个 `delete_tag`
//!   （软删，D2 不清关联）。
//!
//! 引用数口径（跨库聚合，D1 应用层解析）：标签定义与关联**同库**（D7），
//! `TagTable::reference_counts` 统计本库全部已知关联表；
//! - global 定义：global 库（导航 `connection_tags`）+ **项目库计数**（`analytics_resource_tags`
//!   跨库引用 global 定义，规格 D3/D4）；
//! - project 定义：项目库计数（`connection_tags` + `scratchpad_tags`；分析资源不引用 project 定义）。
//!
//! 与缓存管理对话框同一约定：数据就地重算（`Rc<RefCell<_>>`）+ `shared.notify_host(app)`
//! （宿主重绘重建对话框层），操作失败在对话框内给一条可读错误（不静默吞噬）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::{DialogButtonProps, DialogFooter};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::sheet::Sheet;
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use shared::error::{CoreError, StorageError};
use tag::TagDefinitions;

use crate::panels::Shared;

/// 单行标签数据（覆盖层展示用）。
#[derive(Clone)]
struct TagRow {
    id: String,
    name: String,
    /// 展示用作用域（「全局」/「项目」）。
    scope: &'static str,
    /// 是否为项目域定义（重命名 / 清理时据此选库打开）。
    project: bool,
    /// 全模块引用数（含跨库补数）。
    count: i64,
}

/// 采集全部标签行：定义 = global + project 两域；引用数 = 各库 `reference_counts` 聚合。
///
/// 返回 `(行, 空标签数, 错误)`。库打不开给一条可读错误（调用方在对话框内呈现），
/// 不影响其他域继续采集。
fn collect_rows(project_root: Option<&Path>) -> (Vec<TagRow>, usize, Option<String>) {
    let mut rows = Vec::new();
    let mut err = None;

    // 项目库先开：它的计数同时含 `analytics_resource_tags`（跨库引用 global 定义），
    // 用于给 global 标签补数（D1 应用层解析）。
    let mut project_counts: HashMap<String, i64> = HashMap::new();
    if let Some(root) = project_root {
        match tag::TagTable::open_project(root) {
            Ok(store) => match store.reference_counts() {
                Ok(c) => project_counts = c,
                Err(e) => err = Some(format!("读取项目库标签引用数失败：{e}")),
            },
            Err(e) => err = Some(format!("打开项目标签库失败：{e}")),
        }
    }

    // global 域定义
    match tag::TagTable::open_global() {
        Ok(store) => {
            let global_counts = store.reference_counts().unwrap_or_default();
            match store.list_tags() {
                Ok(tags) => {
                    let mut list: Vec<TagRow> = tags
                        .into_iter()
                        .map(|t| {
                            let count = global_counts.get(&t.id).copied().unwrap_or(0)
                                + project_counts.get(&t.id).copied().unwrap_or(0);
                            TagRow {
                                id: t.id,
                                name: t.name,
                                scope: "全局",
                                project: false,
                                count,
                            }
                        })
                        .collect();
                    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                    rows.extend(list);
                }
                Err(e) => err = Some(format!("读取全局标签失败：{e}")),
            }
        }
        Err(e) => err = Some(format!("打开全局标签库失败：{e}")),
    }

    // project 域定义（未打开项目时无）
    if let Some(root) = project_root {
        match tag::TagTable::open_project(root) {
            Ok(store) => match store.list_tags() {
                Ok(tags) => {
                    let mut list: Vec<TagRow> = tags
                        .into_iter()
                        .map(|t| {
                            let count = project_counts.get(&t.id).copied().unwrap_or(0);
                            TagRow {
                                id: t.id,
                                name: t.name,
                                scope: "项目",
                                project: true,
                                count,
                            }
                        })
                        .collect();
                    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                    rows.extend(list);
                }
                Err(e) => err = Some(format!("读取项目标签失败：{e}")),
            },
            Err(_) => {}
        }
    }

    let empty = rows.iter().filter(|r| r.count == 0).count();
    (rows, empty, err)
}

/// 按行打开对应域的 `TagTable`（重命名 / 清理都按 `project` 标志选库）。
fn open_table(project_root: Option<&Path>, project: bool) -> Result<tag::TagTable, CoreError> {
    match project {
        true => {
            let root = project_root.ok_or_else(|| {
                CoreError::storage(StorageError::Persistence {
                    store: "tags".to_string(),
                    operation: "open_project_tags".to_string(),
                    reason: "项目库未打开".to_string(),
                })
            })?;
            tag::TagTable::open_project(root)
        }
        false => tag::TagTable::open_global(),
    }
}

/// 标签重命名表单对话框（单 Input，复用分组表单范式）：名称必填，空名不提交。
fn open_tag_rename_dialog(
    window: &mut Window,
    cx: &mut App,
    shared: Shared,
    rows: Rc<RefCell<Vec<TagRow>>>,
    error: Rc<RefCell<Option<String>>>,
    row: &TagRow,
) {
    let root = shared.project_root();
    let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("标签名称"));
    if !row.name.trim().is_empty() {
        let value = row.name.clone();
        name_input.update(cx, |s, cx| s.set_value(value, window, cx));
    }
    let target = row.id.clone();
    let project = row.project;
    let root = root.map(PathBuf::from);

    let submit = {
        let name_input = name_input.clone();
        let target = target.clone();
        let root = root.clone();
        let rows = rows.clone();
        let error = error.clone();
        let shared = shared.clone();
        move |cx: &mut App| -> bool {
            let name = name_input.read(cx).value().trim().to_string();
            if name.is_empty() {
                return false;
            }
            match open_table(root.as_deref(), project)
                .and_then(|store| store.rename_tag(&target, &name))
            {
                Ok(_) => {
                    *error.borrow_mut() = None;
                    *rows.borrow_mut() = collect_rows(root.as_deref()).0;
                }
                Err(e) => {
                    *error.borrow_mut() = Some(format!("重命名标签失败：{e}"));
                }
            }
            shared.notify_host(cx);
            true
        }
    };
    // 对话框 builder 是 `Fn`（每帧重建），提交闭包在 builder 内部逐帧克隆。
    let submit_for_frame = submit.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let name_empty = name_input.read(cx).value().trim().is_empty();
        let submit_ok = submit_for_frame.clone();
        let submit_key = submit_for_frame.clone();
        dialog
            .title("重命名标签")
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("改名后，所有引用该标签的连接 / 资源 / 草稿自动跟随（按标签定义 id 关联）。"),
                    )
                    .child(Input::new(&name_input))
                    .when(name_empty, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.danger)
                                .child("标签名称不能为空"),
                        )
                    }),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("tag-mgr-rename-cancel")
                            .ghost()
                            .label("取消")
                            .on_click(move |_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("tag-mgr-rename-ok")
                            .label("保存")
                            .on_click(move |_, window, cx| {
                                if submit_ok(cx) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
            // Enter 确认 / Esc 取消（对话框 key context 自带绑定）。
            .on_ok(move |_, _window, cx| submit_key(cx))
            .on_cancel(|_, _, _| true)
    });
}

/// 清理空标签确认：引用数为 0 的活跃定义逐个软删（D2：不清关联）。
///
/// 失败时确认框保持打开（`on_ok` 返回 `false`），错误行在覆盖层呈现。
fn confirm_cleanup(
    window: &mut Window,
    cx: &mut App,
    shared: Shared,
    rows: Rc<RefCell<Vec<TagRow>>>,
    error: Rc<RefCell<Option<String>>>,
) {
    let empty: Vec<(String, String, bool)> = rows
        .borrow()
        .iter()
        .filter(|r| r.count == 0)
        .map(|r| (r.id.clone(), r.name.clone(), r.project))
        .collect();
    if empty.is_empty() {
        return;
    }
    let root = shared.project_root();
    let preview: Vec<String> = empty.iter().take(5).map(|(_, n, _)| format!("「{n}」")).collect();
    let mut preview_text = preview.join("、");
    if empty.len() > 5 {
        preview_text.push_str(&format!(" 等 {} 个", empty.len()));
    }

    window.open_alert_dialog(cx, move |alert, _window, _cx| {
        let empty = empty.clone();
        let root = root.clone();
        let rows = rows.clone();
        let error = error.clone();
        let shared = shared.clone();
        alert
            .confirm()
            .title("清理空标签")
            .description(format!(
                "确定清理 {n} 个没有任何对象引用的标签？{preview}（软删除：已打标的对象不受影响，同名标签可重新创建。）",
                n = empty.len(),
                preview = preview_text
            ))
            .button_props(
                DialogButtonProps::default()
                    .ok_text("清理")
                    .ok_variant(ButtonVariant::Danger)
                    .show_cancel(true),
            )
            .on_ok(move |_, _, app| {
                let mut ok = true;
                for (id, _name, project) in &empty {
                    match open_table(root.as_deref(), *project).and_then(|s| s.delete_tag(id)) {
                        Ok(()) => {}
                        Err(e) => {
                            *error.borrow_mut() =
                                Some(format!("清理标签失败：{e}"));
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    *error.borrow_mut() = None;
                    *rows.borrow_mut() = collect_rows(root.as_deref()).0;
                }
                shared.notify_host(app);
                ok
            })
    });
}

/// 单个标签删除确认（软删）。
fn confirm_delete_tag(
    window: &mut Window,
    cx: &mut App,
    shared: Shared,
    rows: Rc<RefCell<Vec<TagRow>>>,
    error: Rc<RefCell<Option<String>>>,
    tag_id: &str,
    tag_name: &str,
    project: bool,
) {
    let root = shared.project_root();
    let tag_id = tag_id.to_string();
    let tag_name = tag_name.to_string();
    window.open_alert_dialog(cx, move |alert, _window, _cx| {
        let tag_id = tag_id.clone();
        let tag_name = tag_name.clone();
        let root = root.clone();
        let rows = rows.clone();
        let error = error.clone();
        let shared = shared.clone();
        alert
            .confirm()
            .title("删除标签")
            .description(format!(
                "确定删除标签「{tag_name}」？软删除：已打标的对象不受影响，同名标签可重新创建。"
            ))
            .button_props(
                DialogButtonProps::default()
                    .ok_text("删除")
                    .ok_variant(ButtonVariant::Danger)
                    .show_cancel(true),
            )
            .on_ok(move |_, _, app| {
                match open_table(root.as_deref(), project).and_then(|s| s.delete_tag(&tag_id)) {
                    Ok(()) => {
                        *error.borrow_mut() = None;
                        *rows.borrow_mut() = collect_rows(root.as_deref()).0;
                    }
                    Err(e) => {
                        *error.borrow_mut() = Some(format!("删除标签失败：{e}"));
                    }
                }
                shared.notify_host(app);
                true
            })
    });
}

/// 打开「管理标签」覆盖层对话框（Sheet 侧边抽屉，2026-10-28 从 Dialog 迁移）。
pub fn open_tag_manager_dialog(window: &mut Window, cx: &mut App, shared: &Shared) {
    let root = shared.project_root();
    let (collected, _empty, err0) = collect_rows(root.as_deref());
    let rows = Rc::new(RefCell::new(collected));
    let error = Rc::new(RefCell::new(err0));
    let rows_build = rows.clone();
    let error_build = error.clone();
    let shared_build = shared.clone();
    // 新建标签输入框
    let new_input = cx.new(|cx| InputState::new(window, cx).placeholder("新标签名称"));
    let new_input_build = new_input.clone();

    window.open_sheet(cx, move |sheet, _window, cx| {
        let theme = cx.theme();
        let list = rows_build.borrow();
        let err = error_build.borrow();
        let empty_count = list.iter().filter(|r| r.count == 0).count();

        let mut body = div().v_flex().w_full().gap_2();
        body = body.child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(format!(
                    "共 {} 个标签定义（全局与项目域）。引用数为 0 的标签可一键清理。",
                    list.len()
                )),
        );
        if let Some(msg) = err.as_ref() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.danger)
                    .child(msg.clone()),
            );
        }

        // 新建标签行
        let new_name = new_input_build.read(cx).value().trim().to_string();
        let new_empty = new_name.is_empty();
        body = body.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().child(Input::new(&new_input_build)))
                .child(
                    Button::new("tag-mgr-create")
                        .ghost()
                        .small()
                        .label("新建")
                        .disabled(new_empty)
                        .on_click({
                            let rows = rows_build.clone();
                            let error = error_build.clone();
                            let shared = shared_build.clone();
                            let new_input = new_input_build.clone();
                            move |_, window, app| {
                                let name = new_input.read(app).value().trim().to_string();
                                if name.is_empty() {
                                    return;
                                }
                                let root = shared.project_root();
                                match tag::TagTable::open_global()
                                    .and_then(|s| s.create_tag(&name, None, None))
                                {
                                    Ok(_) => {
                                        *error.borrow_mut() = None;
                                        *rows.borrow_mut() = collect_rows(root.as_deref()).0;
                                        new_input.update(app, |s, cx| s.set_value("", window, cx));
                                    }
                                    Err(e) => {
                                        *error.borrow_mut() = Some(format!("新建标签失败：{e}"));
                                    }
                                }
                                shared.notify_host(app);
                            }
                        }),
                ),
        );
        if list.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("暂无标签。可在连接行 / 资源 / 草稿上新建。"),
            );
        } else {
            for r in list.iter() {
                let rid = r.id.clone();
                let rname = r.name.clone();
                let rcount = r.count;
                let rproject = r.project;
                let rid_del = rid.clone();
                let rname_del = rname.clone();

                body = body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .w_full()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .child(rname.clone()),
                        )
                        .child(
                            div()
                                .text_color(theme.colors.muted_foreground)
                                .child(r.scope),
                        )
                        .child(
                            div()
                                .text_color(theme.colors.muted_foreground)
                                .child(format!("{rcount} 引用")),
                        )
                        .child(
                            Button::new(ElementId::Name(SharedString::from(format!(
                                "tag-mgr-rename-{rid}"
                            ))))
                            .ghost()
                            .small()
                            .label("重命名")
                            .on_click({
                                let rows = rows_build.clone();
                                let error = error_build.clone();
                                let shared = shared_build.clone();
                                let row = TagRow {
                                    id: rid,
                                    name: rname,
                                    scope: r.scope,
                                    project: rproject,
                                    count: rcount,
                                };
                                move |_, window, app| {
                                    open_tag_rename_dialog(
                                        window,
                                        app,
                                        shared.clone(),
                                        rows.clone(),
                                        error.clone(),
                                        &row,
                                    );
                                }
                            }),
                        )
                        .child(
                            Button::new(ElementId::Name(SharedString::from(format!(
                                "tag-mgr-delete-{rid_del}"
                            ))))
                            .ghost()
                            .small()
                            .label("删除")
                            .danger()
                            .on_click({
                                let rows = rows_build.clone();
                                let error = error_build.clone();
                                let shared = shared_build.clone();
                                let rid = rid_del.clone();
                                let rname = rname_del.clone();
                                let rproject = rproject;
                                move |_, window, app| {
                                    confirm_delete_tag(window, app, shared.clone(), rows.clone(), error.clone(), &rid, &rname, rproject);
                                }
                            }),
                        ),
                );
            }
        }

        // 清理空标签（底部，引用数为 0 时禁用）
        body = body.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("tag-mgr-cleanup")
                        .ghost()
                        .label(format!("清理 {empty_count} 个空标签"))
                        .disabled(empty_count == 0)
                        .on_click({
                            let rows = rows_build.clone();
                            let error = error_build.clone();
                            let shared = shared_build.clone();
                            move |_, window, app| {
                                confirm_cleanup(window, app, shared.clone(), rows.clone(), error.clone());
                            }
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("空标签 = 没有被任何连接 / 资源 / 草稿引用的标签定义"),
                ),
        );

        sheet
            .title("管理标签")
            .size(px(420.))
            .child(body)
    });
}
