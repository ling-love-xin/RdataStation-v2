//! 编辑器的系统文件对话框（打开 / 另存为）
//!
//! ## 两条不同的接法
//!
//! - **打开**：宿主动作（`Ctrl+O` → `WorkbenchView::open_file_via_dialog`）直接调
//!   [`pick_open_path`]——它要开**新文档**（建面板），本来就是工作台级的事。
//! - **另存为**：编辑器一侧要的是“**拿到一个路径**”（保存流程、关闭三态、失败重试都在它那边），
//!   所以做成**端口注入**（[`attach`] → `EditorShared::attach_save_path_picker`）：
//!   编辑器不依赖 `rfd`，而“怎么弹框”只有宿主知道。
//!
//! ## 为什么用同步 API
//!
//! 调用点全在事件路径（快捷键 / 对话框回调）且就在主线程上；系统模态框本来就要阻塞应用的
//! 事件循环——这是模态对话框的固有代价，不是这里引入的。1b 若要改成异步
//! （`rfd::AsyncFileDialog` + `cx.spawn`），两个函数的形状不变，调用点也不动。
//!
//! 返回值语义：`None` = 用户取消（**不是错误**，不弹提示、不写 notice）。

use std::path::{Path, PathBuf};

use editor::shared::EditorShared;

/// 「打开 / 另存为」的扩展名过滤
///
/// 与 `editor::mode::mode_for_extension` 的三档判定对齐：SQL 脚本 / 分析笔记 / 文本与数据。
/// 过滤只是**默认视图**，用户仍可切到“所有文件”（未知扩展名按文本模式打开，是安全的默认）。
const FILTERS: &[(&str, &[&str])] = &[
    ("SQL 脚本", &["sql", "mysql", "pgsql", "psql", "ddl", "tsql"]),
    ("分析笔记", &["rdsnote", "sqlnote"]),
    ("文本与数据", &["txt", "md", "json", "csv", "log"]),
];

/// 注入另存为的路径选择器（**宿主调用一次**；未注入时“另存为”会明确报“未接入”）
pub fn attach(shared: &EditorShared) {
    shared.attach_save_path_picker(std::rc::Rc::new(
        |current: Option<PathBuf>, default_name: String| {
            pick_save_path(current.as_deref(), &default_name)
        },
    ));
}

/// 弹「打开文件」；用户取消返回 `None`
pub fn pick_open_path() -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title("打开文件");
    for (name, extensions) in FILTERS {
        dialog = dialog.add_filter(*name, extensions);
    }
    dialog.pick_file()
}

/// 弹「另存为」；用户取消返回 `None`
///
/// `current` 是文档已有路径（给默认目录与文件名）；未命名文档传 `None` 时用 `default_name`。
/// 返回值保持用户选的原样：**不替用户补扩展名**（补错了反而改了文件类型判定）。
fn pick_save_path(current: Option<&Path>, default_name: &str) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title("另存为");
    match current {
        Some(path) => {
            if let Some(dir) = path.parent() {
                dialog = dialog.set_directory(dir);
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(default_name);
            dialog = dialog.set_file_name(name);
        }
        None => dialog = dialog.set_file_name(default_name),
    }
    for (name, extensions) in FILTERS {
        dialog = dialog.add_filter(*name, extensions);
    }
    dialog.save_file()
}
