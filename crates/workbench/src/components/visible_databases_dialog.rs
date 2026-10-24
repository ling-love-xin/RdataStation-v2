//! 数据库可见性管理对话框（S3-①b）。
//!
//! 连接行右键「数据库可见性…」打开。对齐 fluxDB / cellar 的「可见库白名单 + 系统对象隐藏」，
//! 交互形态参考 DBeaver 连接过滤对话框：
//!
//! - 候选清单 = 全量库 + schema 两级（**打开时查库**，含从未展开过的库；有 L1/L2 缓存时
//!   瞬时返回，首次内省一次——模态事件里短暂阻塞可接受，见 [`database::nav_jobs::picker_catalog_schemas`]）；
//! - 「显示系统对象」开关默认关：候选列表只显示非系统库 / 非系统 schema（已勾选的系统对象
//!   仍可见，便于取消勾选）；开 = 全部显示、可勾选；
//! - 勾选集合 = 可见库白名单（`settings::navigator.visible_databases`，catalog 名与 schema 名
//!   同表存储、按各自层级匹配——与树加载快照同源，见 S3-①a）；
//! - 确定 = 写白名单 + 重载该连接整棵子树（清掉旧子树按新快照过滤重新加载）。
//!
//! 能力边界（B 档，另会话立项）：DBeaver 的 include/exclude 双向 + 正则 mask + 全局过滤器
//! 不在本弹框实现范围。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, WindowExt as _};
use gpui_kit::*;

use database::nav_jobs;
use database::nav_view::NavView;
use database::navigator_service::{nav_is_system_catalog, nav_is_system_schema};

use crate::panels::Shared;
use crate::ui;

/// 弹框内部状态（Rc 共享给渲染闭包与各按钮回调；变化后由宿主重绘重建弹框层）。
struct PickerState {
    /// 候选清单：`catalog → schemas` 两级全量（含系统对象；打开时同步查一次）。
    entries: Vec<(String, Vec<String>)>,
    /// 「显示系统对象」开关（默认关）。
    show_system: bool,
    /// 已勾选集合（= 可见库白名单；catalog 名与 schema 名按层各自匹配）。
    checked: std::collections::BTreeSet<String>,
}

/// 打开「数据库可见性」对话框。
///
/// `nav_view`：确定后用它重载该连接树（弱引用，面板可能已销毁 / 未装配则跳过刷新）。
/// `project_root`：元数据缓存定位（与导航树加载同源，`None` = 仅全局连接缓存）。
pub fn open_visible_databases_dialog(
    conn_id: &str,
    conn_name: &str,
    project_root: Option<&str>,
    nav_view: Option<gpui::WeakEntity<NavView>>,
    window: &mut Window,
    cx: &mut App,
    shared: &Shared,
) {
    // 打开时同步查一次全量清单（模态事件短暂阻塞；有缓存时瞬时）。
    let entries = nav_jobs::picker_catalog_schemas(conn_id, project_root);
    let loaded = settings::SettingsService::visible_databases(conn_id, cx)
        .map(|v| v.into_iter().collect::<std::collections::BTreeSet<_>>());
    // 预填勾选：既有白名单（白名单模式）直接继承；默认模式 = 勾选全部非系统对象
    //（默认隐藏系统对象的口径；直接确定后写出的白名单与默认行为等价）。
    let mut checked = std::collections::BTreeSet::new();
    match &loaded {
        Some(set) => checked.extend(set.iter().cloned()),
        None => {
            for (catalog, schemas) in &entries {
                if !nav_is_system_catalog(catalog) {
                    checked.insert(catalog.clone());
                }
                for schema in schemas {
                    if !nav_is_system_schema(schema) {
                        checked.insert(schema.clone());
                    }
                }
            }
        }
    }
    let state = Rc::new(RefCell::new(PickerState {
        entries,
        show_system: false,
        checked,
    }));

    let state_build = state.clone();
    let shared_build = shared.clone();
    let conn_id_build = conn_id.to_string();
    // 渲染闭包是 `'static`：引用参数一律先转属主。
    let conn_name_build = conn_name.to_string();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let s = state_build.borrow();
        let muted = theme.colors.muted_foreground;

        // 候选展示过滤：「显示系统对象」关 = 只列非系统对象 + 已勾选的对象（后者保证
        // 已勾选的系统库仍可见、可取消勾选）；开 = 全部。
        let visible_name = |name: &str, is_system: bool| {
            !is_system || s.show_system || s.checked.contains(name)
        };

        let mut list = div().v_flex().w_full().gap_1().max_h(rems(ui::VIS_DIALOG_LIST_MAX_H));
        if s.entries.is_empty() {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("候选清单查询失败：连接未连接或元数据不可用。"),
            );
        }
        for (catalog, schemas) in s.entries.iter() {
            let is_sys_cat = nav_is_system_catalog(catalog);
            if !visible_name(catalog, is_sys_cat) {
                continue;
            }
            let checked_cat = s.checked.contains(catalog);
            list = list.child(
                div()
                    .h_flex()
                    .items_center()
                    .w_full()
                    .gap_2()
                    .text_xs()
                    .child(
                        Checkbox::new(ElementId::Name(SharedString::from(format!(
                            "vis-db-{}",
                            catalog
                        ))))
                        .checked(checked_cat)
                        .label(SharedString::from(catalog.clone()))
                        .on_click({
                            let state = state_build.clone();
                            let shared = shared_build.clone();
                            let name = catalog.clone();
                            move |checked, _, app| {
                                toggle(&state, &shared, &name, *checked, app);
                            }
                        }),
                    )
                    .child(if is_sys_cat {
                        div().text_color(muted).child("系统库")
                    } else {
                        div()
                    }),
            );
            // schema 层（PG 等多级驱动）：缩进一行，与库同表勾选。
            if schemas.is_empty() {
                continue;
            }
            for schema in schemas {
                let is_sys_sch = nav_is_system_schema(schema);
                if !visible_name(schema, is_sys_sch) {
                    continue;
                }
                let checked_sch = s.checked.contains(schema);
                list = list.child(
                    div()
                        .h_flex()
                        .items_center()
                        .w_full()
                        .gap_2()
                        .text_xs()
                        .pl(rems(ui::VIS_DIALOG_SCHEMA_INDENT))
                        .child(
                            Checkbox::new(ElementId::Name(SharedString::from(format!(
                                "vis-sch-{}",
                                schema
                            ))))
                            .checked(checked_sch)
                            .label(SharedString::from(schema.clone()))
                            .on_click({
                                let state = state_build.clone();
                                let shared = shared_build.clone();
                                let name = schema.clone();
                                move |checked, _, app| {
                                    toggle(&state, &shared, &name, *checked, app);
                                }
                            }),
                        )
                        .child(if is_sys_sch {
                            div().text_color(muted).child("系统 schema")
                        } else {
                            div()
                        }),
                );
            }
        }

        dialog
            .title("数据库可见性")
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .w_full()
                            .gap_2()
                            .text_xs()
                            .child(
                                div().flex_1().min_w_0().text_ellipsis().child(format!(
                                    "{} — 勾选树里显示的库 / schema",
                                    conn_name_build
                                )),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .text_color(muted)
                                    .child("显示系统对象")
                                    .child(
                                        Switch::new("vis-show-system")
                                            .checked(s.show_system)
                                            .on_click({
                                                let state = state_build.clone();
                                                let shared = shared_build.clone();
                                                move |on, _, app| {
                                                    state.borrow_mut().show_system = *on;
                                                    shared.notify_host(app);
                                                }
                                            }),
                                    ),
                            ),
                    )
                    .child(list),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("取消勾选 = 树里隐藏（含系统对象）；确定后立即生效并重载该连接。"),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("vis-apply")
                            .label("确定")
                            .on_click({
                                let state = state_build.clone();
                                let nav_view = nav_view.clone();
                                let cid = conn_id_build.clone();
                                let shared = shared_build.clone();
                                move |_, window, app| {
                                    let checked: Vec<String> =
                                        state.borrow().checked.iter().cloned().collect();
                                    settings::SettingsService::set_visible_databases(
                                        &cid, Some(checked), app,
                                    );
                                    // 重载该连接整棵子树（清旧子树、按新白名单快照重新过滤加载）。
                                    if let Some(nav) = nav_view.clone().and_then(|w| w.upgrade()) {
                                        let cid2 = cid.clone();
                                        nav.update(app, |this, cx| {
                                            this.refresh_connection_after_filter(&cid2, cx)
                                        });
                                    }
                                    window.close_dialog(app);
                                    shared.notify_host(app);
                                }
                            }),
                    )
                    .child(
                        Button::new("vis-cancel")
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}

/// 勾选 / 取消勾选一个候选（更新状态 + 宿主重绘弹框层）。
fn toggle(
    state: &Rc<RefCell<PickerState>>,
    shared: &Shared,
    name: &str,
    checked: bool,
    app: &mut App,
) {
    let mut s = state.borrow_mut();
    if checked {
        s.checked.insert(name.to_string());
    } else {
        s.checked.remove(name);
    }
    drop(s);
    shared.notify_host(app);
}
