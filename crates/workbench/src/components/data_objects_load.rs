//! 数据对象面板的「加源」：把对象加进「分析引擎视角」的清单（01 原型形态）。
//!
//! 两条线（`federation-entry-design.md` §11.14 / §12.6）：
//!
//! - **文件线**：外部 / 草稿文件加载进**进程单例** DuckDB（建 `f_<ascii>` 视图，不物化）。
//!   为什么落在单例而不是联邦会话：二次分析与洞察本来就在 `GLOBAL_DUCKDB` 里跑，
//!   文件建成视图放进来，两处立刻都能查；视图不物化，文件改了下次查询跟着变，
//!   要稳定快照才需要落成 `tmp_q_` 临时表。
//! - **连接线**：把选中的库表所属连接**标记 `use_duckdb_fed`**（持久开关，D15/D16 的
//!   联邦源登记口径）。不「立即挂进当前会话」——会话 owner 归文档（D2），面板不越权；
//!   开关翻开后，下一次联邦执行按新源清单建会话（源清单变化会重建会话，会话内临时表
//!   随之失效，D12——回执里要把这句说白）。
//!
//! 两条线在同一个对话框里混选：文件建视图立即生效；连接标记的回执留在对话框底部
//! （不静默关窗——标记的那个源**此刻还不会出现在清单里**，不给回执等于点了没反应）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, Window};

use super::data_objects;
use super::source_picker::{self, ConfirmStrategy, Picked, PickerConfig, PickerState, SourceKind};
use crate::panels::Shared;

/// 「加源」接三类：库表（→ 标记所属连接）+ 能直接读的文件（外部 / 草稿）。
/// DuckDB 根（结果集 / 洞察产物）在这个语境没有叶子——它们已经在实例里，
/// 不存在「再加一次」；三分法分组按 kinds 自动收起那一根。
const LOAD_KINDS: &[SourceKind] = &[
    SourceKind::Connected,
    SourceKind::External,
    SourceKind::Scratchpad,
];

/// 打开「加源」对话框（面板工具栏按钮的出口）
pub fn open_load_files_dialog(shared: &Shared, window: &mut Window, cx: &mut App) {
    let config = PickerConfig {
        title: "增加分析源",
        kinds: LOAD_KINDS,
        confirm_prefix: "加源",
        multi: true,
    };
    let strategy: ConfirmStrategy = Rc::new(
        |shared: &Shared, state: &Rc<RefCell<PickerState>>, window: &mut Window, cx: &mut App| {
            confirm(shared, state, window, cx);
        },
    );
    source_picker::open_source_picker(shared, config, strategy, window, cx);
}

/// 确认：文件逐项建视图、库表汇总成连接标记；任一失败就停下回显
/// （成功的已经生效，重建会覆盖同名视图，无害）。
fn confirm(shared: &Shared, state: &Rc<RefCell<PickerState>>, window: &mut Window, cx: &mut App) {
    let picks = state.borrow().picked_all();
    if picks.is_empty() {
        state.borrow_mut().set_note(Some("先选一个对象".to_string()));
        window.refresh();
        return;
    }

    let mut notes: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    // ── 连接线：选中的库表 → 所属连接去重后逐个标记 ─────────────────────
    // picker 里只有表 / 文件可勾（连接行只展开），所以「标记连接」经由表到达；
    // 同一连接选多张表只标记一次
    let conn_ids: Vec<String> = {
        let mut seen: Vec<String> = Vec::new();
        for picked in &picks {
            if let Picked::Db(conn_id, _) = picked
                && !seen.iter().any(|id| id == conn_id)
            {
                seen.push(conn_id.clone());
            }
        }
        seen
    };
    let mut marked = 0usize;
    if !conn_ids.is_empty() {
        let mark_outcome = mark_connections(shared, &conn_ids);
        match mark_outcome {
            Ok(names) => {
                marked = names.len();
                notes.push(format!(
                    "已标记 {} 个连接为联邦源：{}。下次在联邦通道执行时挂载；\
                     源清单变化会重建会话，会话内临时表将失效",
                    names.len(),
                    names.join("、")
                ));
            }
            Err(reason) => failed.push(reason),
        }
    }

    // ── 文件线：逐项建视图（原有路径） ─────────────────────────────────
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
        // 库文件（.duckdb / .sqlite）挂成 catalog（ATTACH,只读,可展开列表）;
        // 数据文件建成 `f_` 视图——同一个登记结构,面板按路径扩展分派形态
        let loaded = if engine::duckdb::file_view::is_database_file(text) {
            engine::duckdb::file_view::load_database_global(text)
                .map(|alias| (alias, "挂载"))
        } else {
            engine::duckdb::file_view::load_file_global(text)
                .map(|alias| (alias, "视图"))
        };
        match loaded {
            Ok((alias, how)) => {
                // 同名重建是覆盖（视图覆盖 / 重挂），清单里也只留一条（路径换成最新的那份）
                let mut list = shared.loaded_files.borrow_mut();
                list.retain(|(seen, _)| seen != &alias);
                list.push((alias.clone(), text.to_string()));
                notes.push(format!("{alias}（{how}）← {shown}"));
            }
            Err(error) => failed.push(format!("{shown}：{error}")),
        }
    }

    if !failed.is_empty() {
        state
            .borrow_mut()
            .set_note(Some(format!("加源失败：{}", failed.join("；"))));
        window.refresh();
        return;
    }

    // 与面板 refresh 同一步：写缓存，下一帧就带上「已加载」那几行
    let collected = data_objects::collect(shared);
    *shared.data_objects.borrow_mut() = data_objects::objects(&collected);
    // 递增清单版本号（顺带请宿主重绘）：别的读这份清单的地方（面板 defer 路径、
    // 工作集提示）据此知道「该重取了」，不靠这一处自己逐个通知
    shared.mark_data_objects_dirty(cx);

    // 纯文件线：关窗（行已出现在面板里，那就是回执）。
    // 带连接标记：**留窗给回执**——标记的源要等下一次联邦执行才进清单，
    // 静默关窗会让「点了没反应」与「标记成功了」不可分辨
    if marked == 0 {
        drop(notes);
        window.close_dialog(cx);
    } else {
        state.borrow_mut().set_note(Some(notes.join("；")));
        window.refresh();
    }
}

/// 把连接标记为联邦源（写持久开关，走 `set_use_duckdb_fed` 的整记录回填）。
///
/// 返回给回执用的**连接显示名**（面板 / 对话框都不该把 conn_id 直接给人看）。
/// 驱动异步服务用进程级 `bridge_runtime`（临时 runtime 会把连接池的后台任务
/// 一起丢掉，见 K19）。
fn mark_connections(shared: &Shared, conn_ids: &[String]) -> Result<Vec<String>, String> {
    let service = crate::services::data_source_service::DataSourceService::global()
        .map_err(|error| format!("数据源服务不可用：{error}"))?;
    let rt = crate::services::nav_runtime::bridge_runtime()?;
    let project = shared
        .project_root()
        .map(|root| root.to_string_lossy().to_string());

    let mut names = Vec::new();
    let mut failures = Vec::new();
    for conn_id in conn_ids {
        let outcome = rt.block_on(service.set_use_duckdb_fed(conn_id, true, project.as_deref()));
        match outcome {
            Ok(()) => {
                let name = shared
                    .connections
                    .borrow()
                    .iter()
                    .find(|item| &item.id == conn_id)
                    .map(|item| item.name.clone())
                    .unwrap_or_else(|| conn_id.clone());
                names.push(name);
            }
            Err(error) => failures.push(format!("{conn_id}：{error}")),
        }
    }
    if !failures.is_empty() {
        // 部分成功也如实分开报：成功的那几个已生效，重试只会重复翻同一个开关（幂等）
        return Err(format!(
            "标记联邦源失败（成功的已生效）：{}",
            failures.join("；")
        ));
    }
    Ok(names)
}
