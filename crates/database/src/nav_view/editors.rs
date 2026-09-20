//! 行内编辑器：归组 / 标签 / 复制为模板 的渲染与提交路径。
//!
//! 这些块**参与行高计算**（`NAV_EDITOR_TAG` / `_COPY` / `_GROUP`，与 `rows.rs` 的行高
//! 常量成对）：改块高必须同步改 `nav_row_height`，所以两者相邻放着更好找。

use super::*;

impl NavView {
    /// 连接行内联组织编辑器：分组多选（多对多）+「新建分组」+ 标签输入。
    /// 行内**归组**编辑器（右键「移动到分组…」打开）：多选切换 + 新建分组。
    ///
    /// 只处理分组；标签由 [`Self::render_tag_editor`]（行尾 `+`）单独负责。
    pub(super) fn render_group_editor(
        &self,
        conn: &ConnectionItem,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let hover = cx.theme().colors.list_hover;
        let accent = cx.theme().colors.primary;
        let bg = cx.theme().colors.popover;

        let (groups, membership) = {
            let view = self.nav.borrow();
            (
                view.groups.clone(),
                view.membership.get(&conn.id).cloned().unwrap_or_default(),
            )
        };
        let root = self.host.project_root();

        let mut panel = div()
            .id(SharedString::from(format!("nav-group-editor-{}", conn.id)))
            .v_flex()
            .w_full()
            .h(rems(ui::NAV_EDITOR_GROUP))
            .overflow_y_scroll()
            .ml_6()
            .mr_1()
            .mb_1()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(div().flex_none().text_xs().text_color(muted).child("归组"));

        for group in &groups {
            let checked = membership.iter().any(|g| g == &group.id);
            let gid = group.id.clone();
            let cid = conn.id.clone();
            let root = root.clone();
            panel = panel.child(
                div()
                    .id(format!(
                        "nav-org-g-{}::{}::{}",
                        scope_key, group.id, conn.id
                    ))
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .py_0p5()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(
                        // 勾选符号走资产（`check`）：与全应用的图标语言一致，不靠字体字形。
                        div()
                            .w_3()
                            .flex_none()
                            .flex()
                            .items_center()
                            .when(checked, |slot| {
                                slot.child(nav_icon("icons/check.svg", ui::ICON_SIZE_XS, accent))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(fg)
                            .child(group.name.clone()),
                    )
                    .on_click({
                        let entity = cx.entity();
                        move |_, _, app: &mut App| {
                            let gid = gid.clone();
                            let cid = cid.clone();
                            let root = root.clone();
                            entity.update(app, |this, cx| {
                                let in_group = this
                                    .nav
                                    .borrow()
                                    .membership
                                    .get(&cid)
                                    .map(|gs| gs.iter().any(|g| g == &gid))
                                    .unwrap_or(false);
                                let result = if in_group {
                                    crate::nav_store::remove_from_group(root.as_deref(), &gid, &cid)
                                } else {
                                    crate::nav_store::add_to_group(root.as_deref(), &gid, &cid)
                                };
                                match result {
                                    Ok(()) => this.reload_nav_org(),
                                    Err(e) => {
                                        this.host.notice(format!("更新分组失败: {e}"), cx);
                                    }
                                }
                                cx.notify();
                            });
                        }
                    }),
            );
        }

        // 新建分组并直接归入当前连接。
        let cid_new = conn.id.clone();
        panel = panel.child(
            div()
                .id(format!("nav-org-new::{}::{}", scope_key, conn.id))
                .h_flex()
                .items_center()
                .gap_1()
                .px_1()
                .py_0p5()
                .rounded_md()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(
                    div()
                        .w_3()
                        .flex_none()
                        .flex()
                        .items_center()
                        .child(nav_icon("icons/plus.svg", ui::ICON_SIZE_XS, accent)),
                )
                .child(div().text_xs().text_color(accent).child("新建分组"))
                .on_click({
                    let entity = cx.entity();
                    let host = self.host.clone();
                    let seed = GroupFormSeed::for_new(self.next_group_name());
                    move |_, window, app: &mut App| {
                        let e = entity.clone();
                        let host = host.clone();
                        let seed = seed.clone();
                        let cid = cid_new.clone();
                        host.open_group_form(
                            seed,
                            window,
                            app,
                            Rc::new(move |gid, name, desc, app| {
                                // 组内联编辑器：新建后直接把当前连接归入该组。
                                let cid = cid.clone();
                                e.update(app, |this, cx| {
                                    let Some(new_gid) = this.save_group_form(gid, name, desc, cx)
                                    else {
                                        return;
                                    };
                                    let root = this.host.project_root();
                                    if let Err(err) = crate::nav_store::add_to_group(
                                        root.as_deref(),
                                        &new_gid,
                                        &cid,
                                    ) {
                                        this.host.notice(format!("归组失败: {err}"), cx);
                                    }
                                    this.reload_nav_org();
                                    cx.notify();
                                });
                            }),
                        );
                    }
                }),
        );

        panel
    }

    /// 行内**标签**编辑器（行尾 `+` 打开）：仅标签输入，回车保存。
    pub(super) fn render_tag_editor(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let bg = cx.theme().colors.popover;
        let mut panel = div()
            .id("nav-tag-editor")
            .v_flex()
            .w_full()
            .h(rems(ui::NAV_EDITOR_TAG))
            .overflow_y_scroll()
            .ml_6()
            .mr_1()
            .mb_1()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child("标签（逗号分隔，回车保存）"),
            );
        if let Some(input) = &self.nav_tag_input {
            panel = panel.child(Input::new(input));
        }
        panel
    }

    /// 行内「复制为模板」编辑器（右键「复制连接（模板）…」打开）：输入新名，回车提交。
    pub(super) fn render_copy_editor(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let bg = cx.theme().colors.popover;
        let entity = cx.entity();
        let mut panel = div()
            .id("nav-copy-editor")
            .v_flex()
            .w_full()
            .h(rems(ui::NAV_EDITOR_COPY))
            .overflow_y_scroll()
            .ml_6()
            .mr_1()
            .mb_1()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child("复制为模板（不带密码；回车提交）"),
            );
        if let Some(input) = &self.nav_copy_input {
            panel = panel.child(Input::new(input));
        }
        panel.child(
            div().h_flex().justify_end().w_full().child(
                Button::new("nav-copy-cancel")
                    .ghost()
                    .small()
                    .label("取消")
                    .on_click(move |_, _, app| {
                        entity.update(app, |this, cx| this.cancel_copy_connection(cx));
                    }),
            ),
        )
    }

    /// 生成不与现有分组重名的默认分组名。
    pub(super) fn next_group_name(&self) -> String {
        let groups = self.nav.borrow().groups.clone();
        if !groups.iter().any(|g| g.name == "新建分组") {
            return "新建分组".to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("新建分组 {n}");
            if !groups.iter().any(|g| g.name == candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    /// 提交连接行内联标签输入（逗号分隔 → 覆盖式保存）。
    pub(super) fn commit_nav_tags(&mut self, cx: &mut Context<Self>) {
        let Some(conn_id) = self.nav.borrow().tag_editor_for.clone() else {
            return;
        };
        let Some(input) = self.nav_tag_input.clone() else {
            return;
        };
        let text = input.read(cx).value().to_string();
        let tags: Vec<String> = text
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let root = self.host.project_root();
        match crate::nav_store::set_tags(&conn_id, root.as_deref(), &tags) {
            Ok(()) => self.reload_nav_org(),
            Err(e) => self.host.notice(format!("保存标签失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 分组表单初值（编辑时需带上描述；分组头渲染只拿到名称）。
    pub(super) fn group_form_seed(&self, group_id: &str) -> GroupFormSeed {
        let view = self.nav.borrow();
        view.groups
            .iter()
            .find(|g| g.id == group_id)
            .map(|g| {
                GroupFormSeed::for_existing(g.id.clone(), g.name.clone(), g.description.clone())
            })
            .unwrap_or_else(|| GroupFormSeed::for_new(self.next_group_name()))
    }

    /// 提交分组表单（新建 / 编辑），成功时返回分组 ID。
    ///
    /// 名称唯一性不在这里拦：同名分组允许存在（排序 / 描述已经能区分），
    /// 但重名会让「移动到分组…」难以辨认，所以只在面板提示里点出来。
    pub(super) fn save_group_form(
        &mut self,
        group_id: Option<String>,
        name: String,
        description: Option<String>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let root = self.host.project_root();
        let result = match group_id {
            Some(id) => {
                crate::nav_store::update_group(root.as_deref(), &id, &name, description.as_deref())
                    .map(|()| id)
            }
            None => {
                crate::nav_store::create_group_with(root.as_deref(), &name, description.as_deref())
            }
        };
        let saved = match result {
            Ok(id) => {
                self.reload_nav_org();
                let duplicated = self
                    .nav
                    .borrow()
                    .groups
                    .iter()
                    .filter(|g| g.name == name)
                    .count()
                    > 1;
                self.host.notice(
                    if duplicated {
                        format!("已保存分组「{name}」（存在同名分组）")
                    } else {
                        format!("已保存分组「{name}」")
                    },
                    cx,
                );
                Some(id)
            }
            Err(e) => {
                self.host.notice(format!("保存分组失败: {e}"), cx);
                None
            }
        };
        cx.notify();
        saved
    }

    /// 删除分组（仅解除关系，不删成员连接与缓存）。
    pub(super) fn delete_group(&mut self, group_id: &str, cx: &mut Context<Self>) {
        let root = self.host.project_root();
        match crate::nav_store::delete_group(root.as_deref(), group_id) {
            Ok(()) => {
                self.reload_nav_org();
                self.host
                    .notice("分组已删除（成员连接保留）".to_string(), cx);
            }
            Err(e) => self.host.notice(format!("删除分组失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 提交行内「复制为模板」：成功后重载连接列表，并提示新名（不含密码）。
    pub(super) fn commit_copy_connection(&mut self, cx: &mut Context<Self>) {
        let Some(from_id) = self.nav.borrow().copy_for.clone() else {
            return;
        };
        let Some(input) = self.nav_copy_input.clone() else {
            return;
        };
        let new_name = input.read(cx).value().trim().to_string();
        if new_name.is_empty() {
            return;
        }
        let result = self.host.copy_connection(&from_id, &new_name);
        self.nav.borrow_mut().copy_for = None;
        match result {
            Ok(()) => {
                self.reload_connections(cx);
                self.host
                    .notice(format!("已复制为模板：「{new_name}」（不含密码）"), cx);
            }
            Err(e) => self.host.notice(format!("复制失败：{e}"), cx),
        }
        cx.notify();
    }

    /// 取消行内「复制为模板」。
    pub(super) fn cancel_copy_connection(&mut self, cx: &mut Context<Self>) {
        self.nav.borrow_mut().copy_for = None;
        cx.notify();
    }

    /// 提交分组内联重命名。
    pub(super) fn commit_group_rename(&mut self, cx: &mut Context<Self>) {
        let Some(group_id) = self.nav.borrow().group_rename_for.clone() else {
            return;
        };
        let Some(input) = self.nav_group_input.clone() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if !name.is_empty() {
            let root = self.host.project_root();
            match crate::nav_store::rename_group(root.as_deref(), &group_id, &name) {
                Ok(()) => self.reload_nav_org(),
                Err(e) => self.host.notice(format!("重命名失败: {e}"), cx),
            }
        }
        self.nav.borrow_mut().group_rename_for = None;
        cx.notify();
    }
}
