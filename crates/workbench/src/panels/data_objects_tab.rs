//! 「数据对象」右 dock tab：所有可参与分析的对象一处分览。
//!
//! 取数在**事件路径**（构造 / 刷新）跑一次并写进 `Shared::data_objects`，渲染只读缓存——
//! `collect` 内含读盘与查库，放进 render 就是每帧 I/O（gpui-kit-dev 禁止）。
//!
//! 本轮只做「看得见 + 分组 + 通道 / 状态」（原型 07 的骨架），行内「分析」与工作集
//! 留到下一步；先把清单与分组口径钉住，避免一次改太多分不清是哪层出的问题。

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;

use crate::components::data_objects::{self, Group};
use crate::view::RightPanel;

use super::Shared;

pub struct DataObjectsTab {
    shared: Shared,
    focus_handle: FocusHandle,
}

impl DataObjectsTab {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        let tab = Self {
            shared,
            focus_handle: cx.focus_handle(),
        };
        tab.refresh(cx);
        tab
    }

    /// 重新取一次清单（事件路径）。
    ///
    /// 资产那段不走库查询：资产面板实体已经把存活行载在内存里（`view_rows`），
    /// 这里只做形状翻译。面板还没载过（弱句柄取不到 / 行还空）时资产段就是空——
    /// 显示「资产面板载过之后可见」，而不是自己开一次连接去查（那是两套真相）。
    pub fn refresh(&self, cx: &mut Context<Self>) {
        let mut collected = data_objects::collect(&self.shared);
        collected.assets = self.assets_from_panel(cx);
        // 资产行单独存一份：`Object` 只带名字与版本，构造来源还要本体路径
        *self.shared.data_object_assets.borrow_mut() = collected.assets.clone();
        *self.shared.data_objects.borrow_mut() = data_objects::objects(&collected);
        cx.notify();
    }

    /// 从资产面板的已载行取资产（只读；不触发任何加载）
    fn assets_from_panel(&self, cx: &mut Context<Self>) -> Vec<data_objects::AssetRow> {
        let Some(weak) = self.shared.resources_panel.borrow().clone() else {
            return Vec::new();
        };
        let Some(panel) = weak.upgrade() else {
            return Vec::new();
        };
        let rows = panel.read(cx).view_rows().to_vec();
        rows.into_iter()
            .map(|row| data_objects::AssetRow {
                version: u32::try_from(row.version).unwrap_or(0),
                payload_rel_path: row.payload_rel_path,
                name: row.name,
                detail: row.tail,
            })
            .collect()
    }

    /// 勾 / 取消一个对象进工作集（存 `key`，不存行号：列表会重排）。
    fn toggle_picked(&self, key: &str, cx: &mut Context<Self>) {
        let mut picked = self.shared.data_object_picked.borrow_mut();
        if let Some(pos) = picked.iter().position(|k| k == key) {
            picked.remove(pos);
        } else {
            picked.push(key.to_string());
        }
        drop(picked);
        cx.notify();
    }

    fn render_body(&self, cx: &Context<Self>) -> Div {
        let objects = self.shared.data_objects.borrow().clone();
        let picked = self.shared.data_object_picked.borrow().clone();
        let colors = cx.theme().colors;
        let entity = cx.entity();

        let header = div()
            .h_flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.foreground)
                    .child("数据对象"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("{} 个 · 已选 {}", objects.len(), picked.len())),
            );
        let header = if picked.is_empty() {
            header
        } else {
            let shared = self.shared.clone();
            let entity_for_clear = entity.clone();
            let shared_for_run = self.shared.clone();
            let entity_for_run = entity.clone();
            header
                .child(
                    Button::new("data-objects-analyze")
                        .label(format!("分析已选（{}）", picked.len()))
                        .on_click(move |_, _, cx| {
                            // 副作用只在事件路径做：转换是纯函数，开洞察在宿主
                            let objects = shared_for_run.data_objects.borrow().clone();
                            let picked = shared_for_run.data_object_picked.borrow().clone();
                            let assets = shared_for_run.data_object_assets.borrow().clone();
                            // 资产本体在 `{项目}/resources/` 下：目录由项目根推，不写死路径
                            let resources_dir = shared_for_run
                                .project
                                .borrow()
                                .as_ref()
                                .map(|p| p.root.join("resources"));
                            let (sources, skipped) = data_objects::analysis_sources(
                                &objects,
                                &picked,
                                &assets,
                                resources_dir.as_deref(),
                            );
                            if sources.is_empty() {
                                // 选的全是转不了的类型：别开一个空洞察面板
                                return;
                            }
                            let label = match skipped.is_empty() {
                                true => format!("{} 个来源", sources.len()),
                                false => format!("{} 个来源（{} 未包含）", sources.len(), skipped.len()),
                            };
                            shared_for_run.open_insight_source_tables(sources, label, cx);
                            entity_for_run.update(cx, |_, cx| cx.notify());
                        }),
                )
                .child(
                    Button::new("data-objects-clear")
                        .ghost()
                        .label("清空已选")
                        .on_click(move |_, _, cx| {
                            shared.data_object_picked.borrow_mut().clear();
                            // 回调拿到的是 `&mut App`：通知要落到本面板实体上，不能裸调
                            entity_for_clear.update(cx, |_, cx| cx.notify());
                        }),
                )
        };

        let mut list = div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .id("data-objects-list");

        let mut last_group: Option<Group> = None;
        for object in &objects {
            if last_group != Some(object.group) {
                last_group = Some(object.group);
                list = list.child(
                    div()
                        .px_3()
                        .pt_3()
                        .pb_1()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(object.group.title()),
                );
            }
            // 「怎么读」「还在不在」是选源时仅有的两个判断依据，放同一行右侧
            let state_color = if object.can_analyze() {
                colors.muted_foreground
            } else {
                colors.danger
            };
            let is_picked = picked.iter().any(|k| k == &object.key);
            // 只有能用的对象能进工作集：失效行点进去只会在分析时炸
            let pick_button = if object.can_analyze() {
                let key = object.key.clone();
                let entity = entity.clone();
                Some(
                    Button::new(SharedString::from(format!(
                        "data-object-pick-{}",
                        object.key
                    )))
                    .ghost()
                    .label(if is_picked { "已选" } else { "加入" })
                    .on_click(move |_, _, cx| {
                        let key = key.clone();
                        entity.update(cx, |tab, cx| tab.toggle_picked(&key, cx));
                    }),
                )
            } else {
                None
            };
            list = list.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(colors.border)
                    // 已选的行给一道左侧色条：一屏几十行时靠它扫得到
                    .when(is_picked, |el| {
                        el.border_l_2().border_color(colors.primary)
                    })
                    // 失效行压暗：仍然看得见，但别让人以为能直接点
                    .opacity(if object.can_analyze() { 1.0 } else { 0.55 })
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .child(object.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(object.note.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_none()
                            .text_color(colors.muted_foreground)
                            .child(object.channel.label()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_none()
                            .text_color(state_color)
                            .child(object.state.label()),
                    )
                    .children(pick_button),
            );
        }

        if objects.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_6()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child("还没有可用的数据对象：先连一个数据源，或跑一次查询。"),
            );
        }

        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .pt_2()
            .child(header)
            .child(list)
    }
}

impl EventEmitter<BasePanelEvent> for DataObjectsTab {}

impl Focusable for DataObjectsTab {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DataObjectsTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_body(cx)
    }
}

impl BasePanel for DataObjectsTab {
    fn panel_name(&self) -> &'static str {
        "data-objects"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_right.set(RightPanel::DataObjects);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for DataObjectsTab {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 与其它右 dock tab 一致：返回 None，用 title() 渲染图标 + 文字
        None
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(RightPanel::DataObjects.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(RightPanel::DataObjects.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        Some(PanelControl::Menu)
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<gpui_kit::component::button::Button>> {
        None
    }

    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> PopupMenu {
        menu
    }
}
