//! 管理分组覆盖层对话框（原型 §11 #3 分组部分）。
//!
//! 集中处理三个分组操作：**重命名**（复用分组表单，带描述）、**合并分组**（成员并入目标组、
//! 主组跟随、源组删除；目标可选「未分组」= 移出所有组）、**删除**（成员保留、转未分组）。
//!
//! 分组只存在于**项目库**（`nav_store` 项目域）；未打开项目时分组树为空，对话框提示。
//! 标签部分（清理空标签 / 批量打标）与「管理分组与标签」的标签侧归 tag crate 会话，
//! 不在本对话框实现——本对话框只做分组结构。
//!
//! 与缓存管理对话框同一约定：数据就地重算（`Rc<RefCell<_>>`）+ `shared.notify_host(app)`
//! （宿主重绘重建对话框层），操作失败在对话框内给一条可读错误（不静默吞噬）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::*;

use database::nav_store;
use database::nav_store::list_primary_groups;

use crate::panels::Shared;

use super::group_form_dialog::{GroupFormSeed, open_group_form_dialog};

/// 单行分组数据（覆盖层展示用）。
#[derive(Clone)]
struct GroupRow {
    id: String,
    name: String,
    member_count: usize,
}

/// 全量采集分组行（含成员数），项目域。
///
/// 未打开项目（`root = None`）返回空集——调用方用对话框内错误行呈现「分组只存在于项目库」。
fn collect_rows(root: Option<&std::path::Path>) -> Vec<GroupRow> {
    let groups = nav_store::list_groups(root);
    groups
        .into_iter()
        .map(|g| {
            let count = nav_store::list_group_members(root, &g.id).len();
            GroupRow {
                id: g.id,
                name: g.name,
                member_count: count,
            }
        })
        .collect()
}

/// 合并分组：源组全部成员并入目标组（`target = None` 表示「未分组」= 移出所有组），
/// 主组跟随目标组（未分组则清除主组），最后删除源组。
///
/// `add_to_group` 底层是 `INSERT OR IGNORE`（幂等），成员已在目标组的天然去重；
/// 顺序：先并入 + 移出，再修主组，最后删源组（避免删组后主组引用悬空）。
fn merge_group(
    root: Option<&std::path::Path>,
    source: &str,
    target: Option<&str>,
) -> Result<(), String> {
    let members = nav_store::list_group_members(root, source);
    let primary = list_primary_groups(root);
    for m in &members {
        if let Some(t) = target {
            nav_store::add_to_group(root, t, m)?;
        }
        nav_store::remove_from_group(root, source, m)?;
    }
    for (conn_id, gid) in primary {
        if gid == source {
            match target {
                Some(t) => nav_store::set_primary_group(root, &conn_id, t)?,
                None => nav_store::clear_primary_group(root, &conn_id)?,
            }
        }
    }
    nav_store::delete_group(root, source)?;
    Ok(())
}

/// 打开「管理分组」覆盖层对话框。
///
/// 行尾操作：
/// - ✎ 重命名：复用分组表单（名称 + 描述），保存后落库并重算；
/// - 合并到…：下拉选目标组（或未分组）→ 确认 → 合并；
/// - 删除：确认（成员连接与缓存不受影响）→ 删组。
pub fn open_group_manager_dialog(window: &mut Window, cx: &mut App, shared: &Shared) {
    let root = shared.project_root();
    let rows = Rc::new(RefCell::new(collect_rows(root.as_deref())));
    let error = Rc::new(RefCell::new(None::<String>));
    if root.is_none() {
        *error.borrow_mut() = Some("分组只存在于项目库：请先打开项目。".to_string());
    }
    let rows_build = rows.clone();
    let error_build = error.clone();
    let shared_build = shared.clone();

    window.open_sheet(cx, move |sheet, _window, cx| {
        let theme = cx.theme();
        let list = rows_build.borrow();
        let err = error_build.borrow();

        let mut body = div().v_flex().w_full().gap_2();
        body = body.child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .text_xs()
                .child(div().flex_1().child("分组"))
                .child(
                    div()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("{} 个", list.len())),
                ),
        );
        if let Some(msg) = err.as_ref() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.danger)
                    .child(msg.clone()),
            );
        }
        if list.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("暂无分组。右键连接可新建分组。"),
            );
        } else {
            for r in list.iter() {
                let rid = r.id.clone();
                let rname = r.name.clone();
                let rcount = r.member_count;

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
                                .child(format!("{rcount} 成员")),
                        )
                        // 重命名：复用分组表单（名称 + 描述一次编辑）。
                        .child(
                            Button::new(ElementId::Name(SharedString::from(format!(
                                "group-mgr-rename-{rid}"
                            ))))
                            .ghost()
                            .small()
                            .label("重命名")
                            .on_click({
                                let rid = rid.clone();
                                let rname = rname.clone();
                                let rows = rows_build.clone();
                                let error = error_build.clone();
                                let shared = shared_build.clone();
                                let root = root.clone();
                                move |_, window, app| {
                                    // Fn 闭包内克隆出局部变量，才能 move 进内层（FnOnce）表单回调。
                                    let rows_v = rows.clone();
                                    let error_v = error.clone();
                                    let shared_v = shared.clone();
                                    let root_v = root.clone();
                                    let seed = GroupFormSeed {
                                        id: Some(rid.clone()),
                                        name: rname.clone(),
                                        description: None,
                                    };
                                    open_group_form_dialog(window, app, seed, move |gid, name, desc, app| {
                                        let gid = gid.unwrap_or_default();
                                        match nav_store::update_group(
                                            root_v.as_deref(),
                                            &gid,
                                            &name,
                                            desc.as_deref(),
                                        ) {
                                            Ok(()) => {
                                                *error_v.borrow_mut() = None;
                                                *rows_v.borrow_mut() = collect_rows(root_v.as_deref());
                                            }
                                            Err(e) => {
                                                *error_v.borrow_mut() =
                                                    Some(format!("保存分组失败：{e}"));
                                            }
                                        }
                                        shared_v.notify_host(app);
                                    });
                                }
                            }),
                        )
                        // 合并到…：下拉选目标（其他组 / 未分组），选择后二次确认。
                        .child(
                            Button::new(ElementId::Name(SharedString::from(format!(
                                "group-mgr-merge-{rid}"
                            ))))
                            .ghost()
                            .small()
                            .label("合并到…")
                            .dropdown_menu({
                                let rows = rows_build.clone();
                                let error = error_build.clone();
                                let shared = shared_build.clone();
                                let root = root.clone();
                                let source_id = rid.clone();
                                let source_name = rname.clone();
                                move |menu, _window, _cx| {
                                    // 目标列表 = 除自身外的所有组 + 未分组。
                                    let groups: Vec<(String, String)> = rows
                                        .borrow()
                                        .iter()
                                        .filter(|g| g.id != source_id)
                                        .map(|g| (g.id.clone(), g.name.clone()))
                                        .collect();
                                    let mut built = menu;
                                    for (gid, gname) in groups {
                                        let source_id = source_id.clone();
                                        let source_name = source_name.clone();
                                        let target_id = gid.clone();
                                        let target_name = gname.clone();
                                        let rows = rows.clone();
                                        let error = error.clone();
                                        let shared = shared.clone();
                                        let root = root.clone();
                                        built = built.item(
                                            PopupMenuItem::new(gname).on_click(move |_, window, app| {
                                                confirm_merge(
                                                    window,
                                                    app,
                                                    &source_id,
                                                    &source_name,
                                                    Some(&target_id),
                                                    &target_name,
                                                    rows.clone(),
                                                    error.clone(),
                                                    shared.clone(),
                                                    root.clone(),
                                                );
                                            }),
                                        );
                                    }
                                    let source_id = source_id.clone();
                                    let source_name = source_name.clone();
                                    let rows = rows.clone();
                                    let error = error.clone();
                                    let shared = shared.clone();
                                    let root = root.clone();
                                    built.item(
                                        PopupMenuItem::new("未分组").on_click(move |_, window, app| {
                                            confirm_merge(
                                                window,
                                                app,
                                                &source_id,
                                                &source_name,
                                                None,
                                                "未分组",
                                                rows.clone(),
                                                error.clone(),
                                                shared.clone(),
                                                root.clone(),
                                            );
                                        }),
                                    )
                                }
                            }),
                        )
                        // 删除：确认后删组（成员连接与缓存保留，转未分组）。
                        .child(
                            Button::new(ElementId::Name(SharedString::from(format!(
                                "group-mgr-del-{rid}"
                            ))))
                            .ghost()
                            .small()
                            .label("删除")
                            .on_click({
                                let rid = rid.clone();
                                let rname = rname.clone();
                                let rows = rows_build.clone();
                                let error = error_build.clone();
                                let shared = shared_build.clone();
                                let root = root.clone();
                                move |_, window, app| {
                                    // Fn 闭包内克隆出局部变量，move 进内层（Fn）alert builder 才合法。
                                    let rid_v = rid.clone();
                                    let rname_v = rname.clone();
                                    let rows_v = rows.clone();
                                    let error_v = error.clone();
                                    let shared_v = shared.clone();
                                    let root_v = root.clone();
                                    window.open_alert_dialog(app, move |alert, _window, _cx| {
                                        // alert builder 也是 Fn：on_ok 需 move，先把捕获克隆成局部。
                                        let rid2 = rid_v.clone();
                                        let rname2 = rname_v.clone();
                                        let rows2 = rows_v.clone();
                                        let error2 = error_v.clone();
                                        let shared2 = shared_v.clone();
                                        let root2 = root_v.clone();
                                        alert
                                            .confirm()
                                            .title("删除分组")
                                            .description(format!(
                                                "确定删除分组「{rname2}」？成员连接与缓存不会被删除，将回到未分组。"
                                            ))
                                            .button_props(
                                                DialogButtonProps::default()
                                                    .ok_text("删除")
                                                    .ok_variant(ButtonVariant::Danger)
                                                    .show_cancel(true),
                                            )
                                            .on_ok(move |_, _, app| {
                                                let ok = match nav_store::delete_group(
                                                    root2.as_deref(),
                                                    &rid2,
                                                ) {
                                                    Ok(()) => {
                                                        *error2.borrow_mut() = None;
                                                        *rows2.borrow_mut() =
                                                            collect_rows(root2.as_deref());
                                                        true
                                                    }
                                                    Err(e) => {
                                                        *error2.borrow_mut() =
                                                            Some(format!("删除分组失败：{e}"));
                                                        false
                                                    }
                                                };
                                                shared2.notify_host(app);
                                                ok
                                            })
                                    });
                                }
                            }),
                        ),
                );
            }
        }

        sheet
            .title("管理分组")
            .size(px(420.))
            .child(body)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("合并会把源组全部成员并入目标组；删除只移除分组结构。"),
            )
    });
}

/// 合并确认对话框：确认后执行合并并重算。
///
/// `target_id = None` 表示合并到「未分组」（成员移出所有组）。
/// 合并 / 删除失败时确认框保持打开（`on_ok` 返回 `false`），错误行在覆盖层呈现。
#[allow(clippy::too_many_arguments)]
fn confirm_merge(
    window: &mut Window,
    app: &mut App,
    source_id: &str,
    source_name: &str,
    target_id: Option<&str>,
    target_name: &str,
    rows: Rc<RefCell<Vec<GroupRow>>>,
    error: Rc<RefCell<Option<String>>>,
    shared: Shared,
    root: Option<PathBuf>,
) {
    let source_id = source_id.to_string();
    let source_name = source_name.to_string();
    let target_id = target_id.map(|s| s.to_string());
    let target_name = target_name.to_string();
    window.open_alert_dialog(app, move |alert, _window, _cx| {
        let source_id = source_id.clone();
        let target_id = target_id.clone();
        let rows = rows.clone();
        let error = error.clone();
        let shared = shared.clone();
        let root = root.clone();
        alert
            .confirm()
            .title("合并分组")
            .description(format!(
                "把「{source_name}」的全部成员并入「{target_name}」？合并后「{source_name}」分组将删除，成员的主组归属跟随目标分组。"
            ))
            .button_props(
                DialogButtonProps::default()
                    .ok_text("合并")
                    .ok_variant(ButtonVariant::Danger)
                    .show_cancel(true),
            )
            .on_ok(move |_, _, app| {
                let ok = match merge_group(root.as_deref(), &source_id, target_id.as_deref()) {
                    Ok(()) => {
                        *error.borrow_mut() = None;
                        *rows.borrow_mut() = collect_rows(root.as_deref());
                        true
                    }
                    Err(e) => {
                        *error.borrow_mut() = Some(format!("合并分组失败：{e}"));
                        false
                    }
                };
                shared.notify_host(app);
                ok
            })
    });
}
