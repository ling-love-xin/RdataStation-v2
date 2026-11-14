//! 「引擎扩展」区块：把三份各自独立的事实摊在一起
//!
//! 为什么在插件管理页但这个文件归 `engine`：入口同级（用户就在这里找「装了什么」），
//! 但扩展不是插件的一种——没有目录、没有清单、没有贡献点，装了就是进程内 dlopen。
//! 所以这里只**读** engine 给的清单，不复制一份数据模型。
//!
//! 只做「看得见」这一件事：装 / 撤 / 校验是后续动作，先把「对不上」摊出来。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{IconName, Sizable, Size, Theme};
use gpui_kit::*;

use engine::duckdb::extensions::{
    ExtensionDrift, ExtensionEntry, ExtensionInventory, inventory,
};

use crate::ui;

/// 区块状态（事件路径写，`render` 只读）
#[derive(Default)]
pub(crate) struct ExtensionBoard {
    pub inventory: ExtensionInventory,
    pub error: Option<String>,
    pub loading: bool,
    /// 是否已经读过一次（决定「首次展开时自动读」）
    pub loaded: bool,
}

/// 偏差的视觉等级（纯函数：具体颜色在 render 里取，这里只表达「多严重」）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DriftTone {
    /// 登记说能用、实际用不了
    Danger,
    /// 能用但可疑 / 需重装
    Warning,
}

pub(crate) fn drift_tone(drift: &ExtensionDrift) -> DriftTone {
    match drift {
        ExtensionDrift::RegisteredMissingOnDisk => DriftTone::Danger,
        _ => DriftTone::Warning,
    }
}

/// 标题行文案（纯函数）
pub(crate) fn summary_line(inventory: &ExtensionInventory) -> String {
    if inventory.entries.is_empty() {
        return "还没有任何 DuckDB 扩展".to_string();
    }
    let drift = inventory.drift_count();
    if drift == 0 {
        format!("{} 个扩展，三份来源都对得上", inventory.entries.len())
    } else {
        format!(
            "{} 个扩展，其中 {drift} 项对不上",
            inventory.entries.len()
        )
    }
}

/// 字节数的人类可读形式（纯函数；不四舍五入到「0 MB」那种骗人的精度）
pub(crate) fn format_size(bytes: Option<i64>) -> String {
    let Some(bytes) = bytes else {
        return "—".to_string();
    };
    let bytes = bytes.max(0) as f64;
    if bytes < 1024.0 {
        format!("{bytes:.0} B")
    } else if bytes < 1024.0 * 1024.0 {
        format!("{:.1} KB", bytes / 1024.0)
    } else {
        format!("{:.1} MB", bytes / (1024.0 * 1024.0))
    }
}

/// 来源文案（`official` / `community` / 磁盘捡到的 `unknown`）
pub(crate) fn source_label(source: &str) -> &'static str {
    match source {
        "official" => "官方",
        "community" => "社区",
        "local_file" => "本地文件",
        _ => "未登记",
    }
}

impl super::PluginManagerPanel {
    /// 展开 / 收起「引擎扩展」；首次展开顺带读一次
    pub(crate) fn toggle_extensions(&mut self, cx: &mut Context<Self>) {
        self.show_extensions = !self.show_extensions;
        if self.show_extensions && !self.extensions.borrow().loaded {
            self.load_extensions(cx);
        }
        cx.notify();
    }

    /// 重新读一遍清单（后台读，回主线程写状态）
    ///
    /// 读盘包含「开临时内存连接查 `duckdb_extensions()`」，必须在后台 executor 上做：
    /// 常驻 tab 的 render 不能碰 I/O。
    pub(crate) fn load_extensions(&self, cx: &mut Context<Self>) {
        let board: Rc<RefCell<ExtensionBoard>> = self.extensions.clone();
        if board.borrow().loading {
            return;
        }
        {
            let mut state = board.borrow_mut();
            state.loading = true;
            state.error = None;
        }
        cx.notify();

        let _ = cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { inventory() })
                .await;
            let _ = this.update(cx, |this, cx| {
                let mut state = this.extensions.borrow_mut();
                state.loading = false;
                state.loaded = true;
                match loaded {
                    Ok(inventory) => {
                        state.inventory = inventory;
                        state.error = None;
                    }
                    Err(error) => {
                        state.inventory = ExtensionInventory::default();
                        state.error = Some(error);
                    }
                }
                drop(state);
                cx.notify();
            });
        });
    }

    pub(crate) fn show_extensions(&self) -> bool {
        self.show_extensions
    }
}

/// 整块渲染（头部可折叠条 + 展开后的表）
///
/// 只吃 `&Theme` + `Entity`、不碰 `Context`：本页的 `detail` 元素已经把 `cx`
/// 可变借用捕获到返回类型里（edition 2024 的 `impl IntoElement` 捕获全部入参），
/// 再从这里借一次就会 E0502。
pub(super) fn render_extensions(
    this: &super::PluginManagerPanel,
    theme: &Theme,
    manager: Entity<super::PluginManagerPanel>,
) -> Div {
    let board = this.extensions.borrow();
    let count = board.inventory.entries.len();
    let shown = this.show_extensions();

    let mut header = div()
        .h_flex()
        .items_center()
        .gap_2()
        .px_4()
        .py_2()
        .border_b(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .text_xs()
        .child(
            Button::new("plugin-manager-engine-extensions")
                .ghost()
                .label(SharedString::from(if shown {
                    "收起引擎扩展".to_string()
                } else if count == 0 {
                    "引擎扩展".to_string()
                } else {
                    format!("引擎扩展（{count}）")
                }))
                .with_size(Size::XSmall)
                .tooltip("分析引擎用的 DuckDB 扩展（不是连接里的 DuckDB 数据库）")
                .on_click({
                    let manager = manager.clone();
                    move |_, _, app| {
                        let _ = manager.update(app, |this, cx| this.toggle_extensions(cx));
                    }
                }),
        )
        .child(
            div()
                .text_color(theme.colors.muted_foreground)
                .child(summary_line(&board.inventory))
        )
        .child(div().flex_1())
        .child(
            Button::new("plugin-manager-engine-extensions-refresh")
                .ghost()
                .icon(IconName::RefreshCw)
                .with_size(Size::XSmall)
                .tooltip("重新对照：登记表 / DuckDB 自报 / 磁盘")
                .on_click({
                    let manager = manager.clone();
                    move |_, _, app| {
                        let _ = manager.update(app, |this, cx| this.load_extensions(cx));
                    }
                }),
        );

    if board.loading {
        header = header.child(
            div()
                .text_color(theme.colors.muted_foreground)
                .child("读取中…"),
        );
    }
    drop(board);

    let body = shown.then(|| {
        let board = this.extensions.borrow();
        let mut body = div()
            .v_flex()
            .gap_1()
            .px_4()
            .py_2()
            .border_b(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .text_xs();

        if let Some(error) = board.error.clone() {
            body = body.child(
                div()
                    .text_color(theme.colors.danger)
                    .child(format!("引擎扩展清单读取失败：{error}")),
            );
        }
        for note in board.inventory.notes.clone() {
            body = body.child(
                div()
                    .text_color(theme.colors.warning)
                    .child(note),
            );
        }

        if board.inventory.entries.is_empty() {
            body = body.child(
                div()
                    .text_color(theme.colors.muted_foreground)
                    .child(
                        "用到 XLSX 导出或 MySQL / PostgreSQL / SQLite / Oracle 联邦查询时，\
                         会自动装所需扩展并登记。",
                    ),
            );
        } else {
            // 有偏差的排前面：用户先要看见「会坏的那几个」
            let mut rows: Vec<&ExtensionEntry> = board.inventory.entries.iter().collect();
            rows.sort_by_key(|entry| {
                entry
                    .drift
                    .as_ref()
                    .map(|drift| drift.severity())
                    .unwrap_or(u8::MAX)
            });
            for entry in rows {
                body = body.child(render_extension_row(entry, theme));
            }
        }
        body
    });

    div().v_flex().child(header).children(body)
}

fn render_extension_row(entry: &ExtensionEntry, theme: &Theme) -> impl IntoElement {
    let (status_text, status_color) = match &entry.drift {
        Some(drift) => {
            let color = match drift_tone(drift) {
                DriftTone::Danger => theme.colors.danger,
                DriftTone::Warning => theme.colors.warning,
            };
            (drift.label(), color)
        }
        None if !entry.registered => ("能用，但没登记".to_string(), theme.colors.muted_foreground),
        None => ("已登记，可用".to_string(), theme.colors.success),
    };

    let mut row = div()
        .h_flex()
        .items_center()
        .gap_2()
        .py_1()
        .child(
            div()
                .w_40()
                .flex_none()
                .text_ellipsis()
                .child(SharedString::from(entry.name.clone())),
        )
        .child(
            div()
                .w_16()
                .flex_none()
                .text_color(theme.colors.muted_foreground)
                .child(source_label(&entry.source)),
        )
        .child(
            div()
                .w_40()
                .flex_none()
                .text_color(theme.colors.muted_foreground)
                .child(SharedString::from(format!(
                    "{} / {}",
                    entry.kernel_version, entry.platform
                ))),
        )
        .child(
            div()
                .w_16()
                .flex_none()
                .text_color(theme.colors.muted_foreground)
                .child(SharedString::from(format_size(entry.size_bytes))),
        )
        .child(div().flex_1().min_w_0().text_color(status_color).text_ellipsis().child(
            SharedString::from(status_text),
        ));

    if let Some(error) = entry.last_error.clone() {
        row = row.child(
            div()
                .flex_none()
                .text_color(theme.colors.muted_foreground)
                .child(SharedString::from(error)),
        );
    }
    row
}

#[cfg(test)]
mod tests {
    use super::{DriftTone, drift_tone, format_size, source_label, summary_line};
    use engine::duckdb::extensions::{
        ExtensionDrift, ExtensionEntry, ExtensionInventory,
    };

    fn entry(name: &str, drift: Option<ExtensionDrift>) -> ExtensionEntry {
        ExtensionEntry {
            name: name.to_string(),
            kernel_version: "v1.1.5".to_string(),
            platform: "windows_amd64".to_string(),
            source: "official".to_string(),
            install_path: None,
            size_bytes: None,
            sha256: None,
            registered: drift != Some(ExtensionDrift::UnregisteredOnDisk),
            reported: true,
            on_disk: drift != Some(ExtensionDrift::RegisteredMissingOnDisk),
            drift,
            last_error: None,
        }
    }

    #[test]
    fn missing_file_is_danger_the_rest_is_warning() {
        assert_eq!(
            drift_tone(&ExtensionDrift::RegisteredMissingOnDisk),
            DriftTone::Danger,
            "登记说能用、实际没有，用户会在用到时才发现，必须最醒目"
        );
        assert_eq!(
            drift_tone(&ExtensionDrift::KernelMismatch {
                registered: "v1.1.5".into(),
                running: "v1.1.6".into(),
            }),
            DriftTone::Warning
        );
        assert_eq!(
            drift_tone(&ExtensionDrift::UnregisteredOnDisk),
            DriftTone::Warning
        );
    }

    #[test]
    fn summary_line_counts_drift() {
        assert_eq!(
            summary_line(&ExtensionInventory::default()),
            "还没有任何 DuckDB 扩展"
        );
        let inventory = ExtensionInventory {
            entries: vec![
                entry("excel", None),
                entry(
                    "mysql",
                    Some(ExtensionDrift::RegisteredMissingOnDisk),
                ),
            ],
            notes: Vec::new(),
        };
        assert_eq!(summary_line(&inventory), "2 个扩展，其中 1 项对不上");
    }

    #[test]
    fn size_is_formatted_without_lying_precision() {
        assert_eq!(format_size(None), "—");
        assert_eq!(format_size(Some(0)), "0 B");
        assert_eq!(format_size(Some(2048)), "2.0 KB");
        assert_eq!(format_size(Some(5 * 1024 * 1024)), "5.0 MB");
    }

    #[test]
    fn unknown_source_is_not_called_official() {
        assert_eq!(source_label("official"), "官方");
        assert_eq!(source_label("community"), "社区");
        assert_eq!(source_label("unknown"), "未登记");
    }
}
