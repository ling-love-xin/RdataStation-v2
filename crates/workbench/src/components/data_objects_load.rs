//! 数据对象面板的「加源」：把外部文件 / 草稿文件加载进**进程单例** DuckDB
//!
//! 为什么落在单例而不是联邦会话：二次分析与洞察本来就在 `GLOBAL_DUCKDB` 里跑，
//! 文件建成视图放进来，两处立刻都能查；联邦会话是另一个实例，放进去只有联邦
//! 看得到（`docs/architecture/federation/federation-entry-design.md` §11 第 14 条）。
//!
//! 视图不物化，文件改了下次查询跟着变；要稳定快照才需要落成 `tmp_q_` 临时表。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, Window};

use super::data_objects;
use super::source_picker::{self, ConfirmStrategy, Picked, PickerConfig, PickerState, SourceKind};
use crate::panels::Shared;

/// 「加源」只接这两类：能直接读的文件（外部 / 草稿）。
/// 库表、分析库表、结果集本来就在实例里或由连接下推，不需要再“加载”。
const LOAD_KINDS: &[SourceKind] = &[SourceKind::External, SourceKind::Scratchpad];

/// 打开「加源」对话框（面板工具栏按钮的出口）
pub fn open_load_files_dialog(shared: &Shared, window: &mut Window, cx: &mut App) {
    let config = PickerConfig {
        title: "加载文件到分析",
        kinds: LOAD_KINDS,
        confirm_prefix: "加载",
        multi: true,
    };
    let strategy: ConfirmStrategy = Rc::new(
        |shared: &Shared, state: &Rc<RefCell<PickerState>>, window: &mut Window, cx: &mut App| {
            confirm(shared, state, window, cx);
        },
    );
    source_picker::open_source_picker(shared, config, strategy, window, cx);
}

/// 逐项建视图；任一失败就停下回显（成功的已经建好了，重建会覆盖同名视图，无害）
fn confirm(shared: &Shared, state: &Rc<RefCell<PickerState>>, window: &mut Window, cx: &mut App) {
    let picks = state.borrow().picked_all();
    if picks.is_empty() {
        state.borrow_mut().set_note(Some("先选一个文件".to_string()));
        window.refresh();
        return;
    }

    let mut notes: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for picked in picks {
        let (path, shown) = match picked {
            Picked::External(path) => {
                let shown = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "外部文件".to_string());
                (path, shown)
            }
            Picked::Draft(rel, path) => (path, rel),
            _ => continue,
        };
        let Some(text) = path.to_str() else {
            failed.push(format!("{shown}：路径含非 UTF-8 字符"));
            continue;
        };
        match engine::duckdb::file_view::load_file_global(text) {
            Ok(alias) => {
                // 同名视图重建是覆盖，清单里也只留一条（路径换成最新的那份）
                let mut list = shared.loaded_files.borrow_mut();
                list.retain(|(seen, _)| seen != &alias);
                list.push((alias.clone(), text.to_string()));
                notes.push(format!("{alias} ← {shown}"));
            }
            Err(error) => failed.push(format!("{shown}：{error}")),
        }
    }

    if !failed.is_empty() {
        state
            .borrow_mut()
            .set_note(Some(format!("加载失败：{}", failed.join("；"))));
        window.refresh();
        return;
    }

    // 与面板 refresh 同一步：写缓存，下一帧就带上「已加载」那几行
    let collected = data_objects::collect(shared);
    *shared.data_objects.borrow_mut() = data_objects::objects(&collected);
    shared.notify_host(cx);
    drop(notes);
    window.close_dialog(cx);
}
