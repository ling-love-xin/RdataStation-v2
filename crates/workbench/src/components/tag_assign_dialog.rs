//! 打标签对话框（对象级勾选）。
//!
//! 右键连接行 →「打标签…」→ 弹出 sheet：
//! - 列出全部标签定义（checkbox 勾选当前连接已有标签）
//! - 确认后覆盖式 set_tags

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::*;
use tag::TagDefinitions;

use crate::panels::Shared;

/// 采集全部标签名（global + project 两域合并去重）。
fn collect_tag_names(project_root: Option<&std::path::Path>) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    if let Ok(store) = tag::TagTable::open_global() {
        if let Ok(tags) = store.list_tags() {
            for t in tags {
                names.insert(t.name);
            }
        }
    }
    if let Some(root) = project_root {
        if let Ok(store) = tag::TagTable::open_project(root) {
            if let Ok(tags) = store.list_tags() {
                for t in tags {
                    names.insert(t.name);
                }
            }
        }
    }
    names.into_iter().collect()
}

pub fn open_tag_assign_dialog(
    window: &mut Window,
    cx: &mut App,
    shared: &Shared,
    conn_id: &str,
) {
    let root = shared.project_root();
    let root_path = root.as_deref();
    let all_tags = collect_tag_names(root_path);
    let current: Vec<String> = database::nav_store::list_tags(conn_id, root_path);

    // 勾选状态：与 all_tags 一一对应
    let checked: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(
        all_tags.iter().map(|t| current.contains(t)).collect(),
    ));
    let conn_id = conn_id.to_string();

    window.open_sheet(cx, move |sheet, _window, cx| {
        let theme = cx.theme();
        let tags = all_tags.clone();
        let checked = checked.clone();

        let mut body = div().v_flex().w_full().gap_2();

        body = body.child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("为连接勾选标签："),
        );

        if tags.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("暂无标签定义。可在连接行 / 资源 / 草稿上新建。"),
            );
        } else {
            for (i, name) in tags.iter().enumerate() {
                let is_checked = checked.borrow()[i];
                let name_clone = name.clone();
                let checked_clone = checked.clone();
                body = body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .py_1()
                        .child(
                            Button::new(ElementId::Name(SharedString::from(format!(
                                "tag-assign-{i}"
                            ))))
                            .ghost()
                            .small()
                            .label(if is_checked { "☑" } else { "☐" })
                            .on_click(move |_, _, _app| {
                                let mut c = checked_clone.borrow_mut();
                                c[i] = !c[i];
                            }),
                        )
                        .child(div().text_xs().child(name_clone)),
                );
            }
        }

        // 确认按钮
        body = body.child(
            div().h_flex().justify_end().gap_2().child(
                Button::new(ElementId::Name("tag-assign-confirm".into()))
                    .label("确认")
                    .on_click({
                        let checked = checked.clone();
                        let tags = tags.clone();
                        let conn_id = conn_id.clone();
                        let root = root.clone();
                        move |_, window, app| {
                            let c = checked.borrow();
                            let final_tags: Vec<String> = tags
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| c[*i])
                                .map(|(_, t)| t.clone())
                                .collect();
                            let root_ref = root.as_deref();
                            if let Err(e) = database::nav_store::set_tags(&conn_id, root_ref, &final_tags) {
                                eprintln!("保存标签失败：{e}");
                            }
                            window.close_sheet(app);
                        }
                    }),
            ),
        );

        sheet.title("打标签").size(px(320.)).child(body)
    });
}
