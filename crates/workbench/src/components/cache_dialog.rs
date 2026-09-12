//! 缓存管理对话框（元数据缓存查看 / 清理）。
//!
//! 缓存**默认不自动删除**（设计 §5.2）：只有本对话框的显式「清理」会删除
//! `conn_{id}.sqlite` 元数据缓存文件；断开 / 关闭项目 / 删除连接都不删。
//!
//! 入口两处：设置面板「数据源导航 → 缓存管理…」与数据源导航面板头 `⋯` 菜单。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::*;

use engine::persistence::{ConnectionType, MetadataCacheManager};

use crate::panels::Shared;
use crate::view::ConnectionItem;

/// 单条连接的元数据缓存条目。
#[derive(Clone)]
struct CacheEntry {
    conn_id: String,
    name: String,
    size: u64,
    exists: bool,
}

/// 连接 ID 前缀 → 元数据缓存归属（`G_` 全局目录；`P_`/`GP_` 项目目录）。
fn conn_type(conn_id: &str) -> ConnectionType {
    if conn_id.starts_with("G_") {
        ConnectionType::Global
    } else {
        ConnectionType::Project
    }
}

/// 采集当前可见连接的缓存条目（按名称排序）。
fn collect_entries(shared: &Shared) -> Vec<CacheEntry> {
    let root = shared
        .project
        .borrow()
        .as_ref()
        .map(|p| p.root.to_string_lossy().to_string());
    let conns: Vec<ConnectionItem> = shared.connections.borrow().iter().cloned().collect();
    let mut out = Vec::new();
    for c in conns {
        let Ok(mgr) = MetadataCacheManager::new(&c.id, conn_type(&c.id), root.as_deref()) else {
            continue;
        };
        let exists = mgr.exists();
        let size = if exists { mgr.size().unwrap_or(0) } else { 0 };
        out.push(CacheEntry {
            conn_id: c.id,
            name: c.name,
            size,
            exists,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 删除某连接的元数据缓存文件（唯一删除路径）。
///
/// 连接仍在使用时文件可能被 SQLite 句柄占用，失败原因向上返回。
fn delete_cache(shared: &Shared, conn_id: &str) -> Result<(), String> {
    let root = shared
        .project
        .borrow()
        .as_ref()
        .map(|p| p.root.to_string_lossy().to_string());
    let mgr = MetadataCacheManager::new(conn_id, conn_type(conn_id), root.as_deref())
        .map_err(|e| e.to_string())?;
    mgr.delete().map_err(|e| e.to_string())
}

/// 人类可读大小。
fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let b = bytes as f64;
    if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 打开缓存管理对话框。列表随清理就地重算并通过宿主重绘刷新。
pub fn open_cache_dialog(window: &mut Window, cx: &mut App, shared: &Shared) {
    let entries = Rc::new(RefCell::new(collect_entries(shared)));
    let entries_build = entries.clone();
    let shared_build = shared.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let list = entries_build.borrow();
        let total: u64 = list.iter().map(|e| e.size).sum();
        let count = list.iter().filter(|e| e.exists).count();

        let mut body = div().v_flex().w_full().gap_2();
        body = body.child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .text_xs()
                .child(div().flex_1().child("元数据缓存"))
                .child(
                    div()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("{count} 项 · {}", human_size(total))),
                ),
        );
        if list.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("暂无连接缓存。"),
            );
        } else {
            for e in list.iter() {
                body =
                    body.child(
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
                                    .child(e.name.clone()),
                            )
                            .child(div().text_color(theme.colors.muted_foreground).child(
                                if e.exists {
                                    human_size(e.size)
                                } else {
                                    "无缓存".to_string()
                                },
                            ))
                            .child(
                                Button::new(ElementId::Name(SharedString::from(format!(
                                    "cache-del-{}",
                                    e.conn_id
                                ))))
                                .ghost()
                                .small()
                                .label("清理")
                                .disabled(!e.exists)
                                .on_click({
                                    let shared = shared_build.clone();
                                    let entries = entries_build.clone();
                                    let cid = e.conn_id.clone();
                                    move |_, _, app| {
                                        if let Err(e) = delete_cache(&shared, &cid) {
                                            *shared.notice.borrow_mut() =
                                                Some(format!("清理失败：{e}"));
                                        }
                                        *entries.borrow_mut() = collect_entries(&shared);
                                        // 宿主重绘：对话框层由宿主 render 重建，才能看到新数值。
                                        shared.notify_host(app);
                                    }
                                }),
                            ),
                    );
            }
        }

        dialog
            .title("缓存管理")
            .child(body)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("缓存默认不随断开 / 删除连接清除；仅此处显式清理。"),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("cache-clear-all")
                            .with_variant(ButtonVariant::Danger)
                            .label("清理全部")
                            .on_click({
                                let shared = shared_build.clone();
                                let entries = entries_build.clone();
                                move |_, _, app| {
                                    let ids: Vec<String> = entries
                                        .borrow()
                                        .iter()
                                        .map(|e| e.conn_id.clone())
                                        .collect();
                                    let mut failed = 0usize;
                                    for cid in ids {
                                        if delete_cache(&shared, &cid).is_err() {
                                            failed += 1;
                                        }
                                    }
                                    if failed > 0 {
                                        *shared.notice.borrow_mut() =
                                            Some(format!("{failed} 项缓存被占用，未能清理"));
                                    }
                                    *entries.borrow_mut() = collect_entries(&shared);
                                    shared.notify_host(app);
                                }
                            }),
                    )
                    .child(
                        Button::new("cache-close")
                            .label("关闭")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .on_ok(|_, window, cx| {
                window.close_dialog(cx);
                false
            })
    });
}
