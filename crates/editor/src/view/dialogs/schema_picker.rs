//! 库 / Schema 选择器（工具栏「schema@库」点击弹窗）。

use super::*;

// ==== 活跃 Database / Schema 选择器（工具栏「schema@库」点击弹窗；2026-10-27）====
//
// 图：左栏 Database 列表 + 右栏该库的 Schema 列表。左栏选中切右栏；点击右栏 Schema
// 绑定并关闭。显示规则：有独立 Schema 层时 `schema@库`，库即 Schema（MySQL 等 catalog
// 为 None）时只显示库（Schema）名——与工具栏槽位同一口径。
//
// 需要左栏切换驱动右栏重绘，故状态用 `Entity<SchemaPickerState>`（对话框内局部状态
// 的正当形状：点击即改，`cx.notify` 触发重绘，见 dialogs 模块头取舍 #2 的例外）。

/// 库 / Schema 选择器内部状态（两栏：左 Database、右 Schema）
pub struct SchemaPickerState {
    /// 去重后的库（catalog）列表；`None` = 库即 Schema（MySQL 无独立 Catalog 层）
    pub(crate) catalogs: Vec<Option<String>>,
    /// 每个库下的 Schema 列表（与 `catalogs` 对齐）
    pub(crate) schemas_by: Vec<Vec<String>>,
    /// 左栏当前选中的库下标
    pub(crate) selected: usize,
    /// 当前文档绑定的 Schema（勾选标记）
    pub(crate) current: Option<String>,
    /// 宿主绑定回调（确认后写文档 Schema + 状态刷新）
    on_pick: Rc<dyn Fn(String, &mut Window, &mut App)>,
}

impl SchemaPickerState {
    /// 从 SchemaInfo 列表聚合出「库 → Schema」两栏数据（去重、保序）
    pub fn new(
        schemas: Vec<crate::connection::SchemaInfo>,
        current: Option<String>,
        on_pick: impl Fn(String, &mut Window, &mut App) + 'static,
    ) -> Self {
        let mut catalogs: Vec<Option<String>> = Vec::new();
        let mut schemas_by: Vec<Vec<String>> = Vec::new();
        for item in schemas {
            let idx = catalogs
                .iter()
                .position(|c| c == &item.catalog)
                .unwrap_or_else(|| {
                    catalogs.push(item.catalog.clone());
                    schemas_by.push(Vec::new());
                    catalogs.len() - 1
                });
            schemas_by[idx].push(item.schema);
        }
        let selected = schemas_by
            .iter()
            .position(|schs| current.as_ref().is_some_and(|c| schs.contains(c)))
            .unwrap_or(0);
        Self {
            catalogs,
            schemas_by,
            selected,
            current,
            on_pick: Rc::new(on_pick),
        }
    }

    /// 渲染对话框正文（左库 + 右 Schema 两栏 + 底部当前绑定；只读，交互经捕获实体回写）
    fn body(&self, entity: Entity<Self>, _window: &mut Window, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let foreground = theme.colors.foreground;
        let border = theme.colors.border;

        // 左栏：Database 列表（当前选中 toggled）
        let left =
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .children(self.catalogs.iter().enumerate().map(|(i, cat)| {
                    let entity = entity.clone();
                    let label = match cat {
                        Some(c) => c.clone(),
                        None => "默认库".to_string(),
                    };
                    Button::new(SharedString::from(format!("schema-picker-cat-{i}")))
                        .ghost()
                        .small()
                        .toggled(i == self.selected)
                        .debug_selector(move || format!("schema-picker-cat-{i}"))
                        .label(label)
                        .on_click(move |_, _window, cx| {
                            entity.update(cx, |state, cx| {
                                state.selected = i;
                                cx.notify();
                            });
                        })
                }));

        // 右栏：当前库下的 Schema 列表（点击绑定并关闭）
        let right_schemas = self
            .schemas_by
            .get(self.selected)
            .cloned()
            .unwrap_or_default();
        let right_on_pick = self.on_pick.clone();
        let right = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .children(right_schemas.iter().map(|schema| {
                let on_pick = right_on_pick.clone();
                let schema = schema.clone();
                let selector = format!("schema-picker-sch-{schema}");
                let checked = self.current.as_deref() == Some(schema.as_str());
                Button::new(SharedString::from(selector.clone()))
                    .ghost()
                    .small()
                    .toggled(checked)
                    .debug_selector(move || selector.clone())
                    .label(schema.clone())
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        on_pick(schema.clone(), window, cx);
                    })
            }));

        // 底部当前绑定（`schema@库` / 仅库名，与工具栏槽位同口径）
        let current_label = match &self.current {
            Some(s) => {
                let catalog = self.catalogs.get(self.selected).and_then(|c| c.clone());
                match catalog {
                    Some(c) => format!("{s}@{c}"),
                    None => s.clone(),
                }
            }
            None => "（未选 Schema）".to_string(),
        };

        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .items_stretch()
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .flex_1()
                            .child(div().text_xs().text_color(muted).child("Database"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .border_1()
                                    .border_color(border)
                                    .rounded_md()
                                    .p_1()
                                    .child(left),
                            ),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .flex_1()
                            .child(div().text_xs().text_color(muted).child("Schema"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .border_1()
                                    .border_color(border)
                                    .rounded_md()
                                    .p_1()
                                    .child(right),
                            ),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if self.current.is_some() {
                        foreground
                    } else {
                        muted
                    })
                    .child(current_label),
            )
    }
}

/// 打开 Database / Schema 选择器对话框
///
/// 数据 = 绑定连接的 `Vec<SchemaInfo>`（聚合出 库 → Schema 两栏）；`on_pick` 携带选中的
/// Schema 名回调宿主（`set_schema`）。返回 `Entity<SchemaPickerState>` 供测试驱动。
pub fn open_schema_picker(
    window: &mut Window,
    cx: &mut App,
    schemas: Vec<crate::connection::SchemaInfo>,
    current: Option<String>,
    on_pick: impl Fn(String, &mut Window, &mut App) + 'static,
) -> Entity<SchemaPickerState> {
    let state = cx.new(|_cx| SchemaPickerState::new(schemas, current, on_pick));
    let entity = state.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let body = entity.read(cx).body(entity.clone(), _window, cx);
        let width = cx.theme().font_size * ui::SCHEMA_PICKER_DIALOG_WIDTH;
        dialog.title("选择 Database/Schema").w(width).child(body)
    });
    state
}
