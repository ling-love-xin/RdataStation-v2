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

/// 加源的两档 scope（铁律 B 组第 6 条）。
///
/// - `Session`：会话级——`f_*` 视图挂进程单例、不物化，切项目清掉（按需读原文件）。
/// - `Snapshot`：项目级——走归档 `copy` 存一份只读副本，生成资产、跟着项目走、带版本。
///
/// 这两档对**文件**而言是两种落点（视图 vs 副本）；对**库表**是同一件事（把所属连接
/// 标记成联邦源，那条本身就是按项目持久），所以库表在两档下行为一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadScope {
    Session,
    Snapshot,
}

impl LoadScope {
    fn title(self) -> &'static str {
        match self {
            // 标题里就写明落点：不写的话两档在一堆复选框里长得一样
            LoadScope::Session => "增加分析源（按需 · 本次会话）",
            LoadScope::Snapshot => "增加分析源（快照 · 存进项目）",
        }
    }

    /// 给回执用的落点说法（用户得知道"切项目之后还在不在"）。
    pub fn hint(self) -> &'static str {
        match self {
            LoadScope::Session => "按需读原文件，切项目即清",
            LoadScope::Snapshot => "已存成快照，跟着项目走、只读带版本",
        }
    }
}

/// 打开「加源」对话框（面板工具栏按钮的出口）
pub fn open_load_files_dialog(
    shared: &Shared,
    scope: LoadScope,
    window: &mut Window,
    cx: &mut App,
) {
    open_load_dialog(shared, scope, None, window, cx);
}

/// 只在某一条连接上开「加源」（数据对象面板里「已登记、挂不上」的源行点展开走这条）
///
/// 与 [`open_load_files_dialog`] 同一套树 / 内省 / 确认动作，区别只是连接集合收成一条；
/// 前提是这条连接**活着**（死连接没有表可列，模型侧也不给展开）。
pub fn open_load_dialog_for_connection(
    shared: &Shared,
    conn_id: &str,
    scope: LoadScope,
    window: &mut Window,
    cx: &mut App,
) {
    open_load_dialog(shared, scope, Some(conn_id), window, cx);
}

fn open_load_dialog(
    shared: &Shared,
    scope: LoadScope,
    only_conn: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) {
    let config = PickerConfig {
        title: scope.title(),
        kinds: LOAD_KINDS,
        confirm_prefix: "加源",
        multi: true,
    };
    let strategy: ConfirmStrategy = Rc::new(
        move |shared: &Shared, state: &Rc<RefCell<PickerState>>, window: &mut Window, cx: &mut App| {
            confirm(shared, scope, state, window, cx);
        },
    );
    match only_conn {
        Some(conn_id) => {
            source_picker::open_source_picker_for_connection(
                shared, conn_id, config, strategy, window, cx,
            );
        }
        None => source_picker::open_source_picker(shared, config, strategy, window, cx),
    }
}

/// 确认：文件逐项建视图、库表汇总成连接标记；任一失败就停下回显
/// （成功的已经生效，重建会覆盖同名视图，无害）。
fn confirm(
    shared: &Shared,
    scope: LoadScope,
    state: &Rc<RefCell<PickerState>>,
    window: &mut Window,
    cx: &mut App,
) {
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
        // 项目级：存副本 = 走归档，不进视图（铁律第 6 条 + 第 16 条）
        if scope == LoadScope::Snapshot {
            crate::components::file_actions::archive_file_as_snapshot(shared, text, cx);
            notes.push(format!("{shown} → 快照（{}）", LoadScope::Snapshot.hint()));
            continue;
        }
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
    // 下列两种**留窗**：
    // - 带连接标记：标记的源要等下一次联邦执行才进清单，静默关窗会让「点了没反应」与
    //   「标记成功了」不可分辨；
    // - 快照档：归档是后台任务，此刻面板里还没有新行，关窗同样等于没回执。
    if marked == 0 && scope == LoadScope::Session {
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
        let name = shared
            .connections
            .borrow()
            .iter()
            .find(|item| &item.id == conn_id)
            .map(|item| item.name.clone())
            .unwrap_or_else(|| conn_id.clone());
        // 三态：`Some(true)` = 开本地加速（同时也说明这条连接能被 DuckDB 直连）
        let outcome = rt.block_on(service.set_use_duckdb_fed(
            conn_id,
            Some(true),
            project.as_deref(),
        ));
        match outcome {
            Ok(()) => {
                // 「有值即登记」（用户口径 2026-11-26）：开关有值 = 这条连接能当 DuckDB 源，
                // 项目登记表里必须有一行。登记失败不回滚开关（开关已生效），但要如实点名。
                if let Err(error) = ensure_registration(shared, conn_id, &name) {
                    failures.push(format!("{name}：开关已开，登记没写进去（{error}）"));
                }
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

/// 把**一个**文件库（`.duckdb` / `.sqlite`）挂进分析实例（源段右键「挂载」）。
///
/// 与「加源」的文件线同一条路径（`load_database_global` → ATTACH 成 catalog → 写进
/// `loaded_files` → 重取清单），区别只在入口：加源要开对话框选文件，挂载是源行上
/// 已经知道路径（注册成分析源的那条连接登记的本体文件）。
///
/// 挂上之后源段会多出一行「文件库」，原 conn 行不再是失效态——两边都指向**同一个**
/// 分析实例里的 catalog，不另存副本。
pub fn mount_file_database(shared: &Shared, path_text: &str, cx: &mut App) {
    match engine::duckdb::file_view::load_database_global(path_text) {
        Ok(alias) => {
            let mut list = shared.loaded_files.borrow_mut();
            // 同名重挂是覆盖：清单里只留最新那份路径
            list.retain(|(seen, _)| seen != &alias);
            list.push((alias.clone(), path_text.to_string()));
            drop(list);
            let collected = data_objects::collect(shared);
            *shared.data_objects.borrow_mut() = data_objects::objects(&collected);
            // 与加源同一步：递增清单版本号，面板/工作集据此重取（不各自猜）
            shared.mark_data_objects_dirty(cx);
            shared.say(format!("已挂载 {alias} ← {path_text}"), cx);
        }
        Err(error) => shared.say_error(format!("挂载失败：{error}"), cx),
    }
}

/// 把一个**已挂载**的文件库卸下来（源段右键「卸载」）。
///
/// 与 [`mount_file_database`] 对称：只动挂载关系，不碰磁盘上的库文件；卸完把路径
/// 从已加载清单里去掉，重取清单后 conn 行回到「未挂载」（不可展开、只给挂载）。
pub fn unmount_file_database(shared: &Shared, path_text: &str, cx: &mut App) {
    let alias = engine::duckdb::file_view::alias_for(path_text);
    match engine::duckdb::file_view::detach_database_global(&alias) {
        Ok(()) => {
            shared
                .loaded_files
                .borrow_mut()
                .retain(|(seen, _)| seen != &alias);
            let collected = data_objects::collect(shared);
            *shared.data_objects.borrow_mut() = data_objects::objects(&collected);
            shared.mark_data_objects_dirty(cx);
            shared.say(format!("已卸载 {alias}（磁盘上的文件没动）"), cx);
        }
        Err(error) => shared.say_error(format!("卸载失败：{error}"), cx),
    }
}

/// 用资产某一历史版本的内容做一次分析（版本对话框「用 vN 分析…」）。
///
/// 铁律 D17：历史版本**不平铺进数据对象清单**，要看旧数就走这个二级入口。
/// 三件事必须守住：
/// 1. **只读**：读的是 `.RSmeta/resources/versions/<id>/<N>/` 下的副本，本体与版本行都不动；
/// 2. **不覆盖当前版本**：副本与原文件同名，`alias_for` 会算出同一个 `f_*`，所以这里
///    用 `f_<stem>_v<N>` 的显式别名（`load_file_as_global` / `load_database_as_global`），
///    当前版本那行原样留着；
/// 3. 挂进进程单例（会话级），面板的「已加载」行据此多一行，切项目随 `f_*` 一起清掉。
pub fn analyze_asset_version(
    shared: &Shared,
    resource_id: &str,
    name: &str,
    version: i32,
    cx: &mut App,
) {
    let Some(root) = shared.project_root() else {
        shared.say_error("还没有打开项目".to_string(), cx);
        return;
    };
    let rt = match crate::services::nav_runtime::bridge_runtime() {
        Ok(rt) => rt,
        Err(error) => {
            shared.say_error(format!("异步运行时不可用：{error}"), cx);
            return;
        }
    };
    let copy = match rt.block_on(analytics_resource::version_file(root, resource_id, version)) {
        Ok(copy) => copy,
        Err(error) => {
            shared.say_error(format!("读历史副本失败：{error}"), cx);
            return;
        }
    };
    let Some(path) = copy else {
        shared.say_error(
            format!("「{name}」v{version} 没有内容副本（可能已被保留策略裁剪）"),
            cx,
        );
        return;
    };
    let Some(path_text) = path.to_str() else {
        shared.say_error("历史副本路径里有非 UTF-8 字符，无法分析".to_string(), cx);
        return;
    };

    // 数据库文件（`.duckdb` / `.sqlite`）走 ATTACH（与挂载同一口径）；其余走文件视图。
    let alias = format!(
        "{}_v{version}",
        engine::duckdb::file_view::alias_for(path_text)
    );
    let outcome = if engine::duckdb::file_view::is_database_file(path_text) {
        engine::duckdb::file_view::load_database_as_global(path_text, &alias)
    } else {
        engine::duckdb::file_view::load_file_as_global(path_text, &alias)
    };
    match outcome {
        Ok(()) => {
            let mut list = shared.loaded_files.borrow_mut();
            list.retain(|(seen, _)| seen != &alias);
            list.push((alias.clone(), path_text.to_string()));
            drop(list);
            let collected = data_objects::collect(shared);
            *shared.data_objects.borrow_mut() = data_objects::objects(&collected);
            shared.mark_data_objects_dirty(cx);
            shared.say(
                format!("已用「{name}」v{version} 分析：{alias}（本体与版本都没动）"),
                cx,
            );
        }
        Err(error) => shared.say_error(format!("用 v{version} 分析失败：{error}"), cx),
    }
}

/// 把一条**网络连接**挂进分析实例的本地加速档（源段右键「挂载」）。
///
/// 这是规格 §8 补上的缺口（铁律第 37 条记着的「workbench 里零挂载入口」）。
/// 挂载走 `accel::ensure_on_instance`（2026-11-26 起加速档落分析实例，不再自建会话）。
/// 凭据取法与编辑器侧同一口径：
/// 连接管理器给 `db_type`，运行时连接串（`url_override`，带凭据）优先，
/// 拿不到才退回 `ConnectionInfo.url`——DuckDB 1.5.5 的 mysql/pg 扫描器不认
/// Secret，凭据只能随连接串进 ATTACH。
pub fn mount_network_source(shared: &Shared, conn_id: &str, cx: &mut App) {
    let manager = engine::connection_manager::get_connection_manager();
    let rt = match crate::services::nav_runtime::bridge_runtime() {
        Ok(rt) => rt,
        Err(error) => {
            shared.say_error(format!("异步运行时不可用：{error}"), cx);
            return;
        }
    };
    let info = match rt.block_on(manager.get_connection_info(&conn_id.to_string())) {
        Some(info) => info,
        None => {
            shared.say_error(
                format!("拿不到连接 {conn_id} 的信息，无法在本地挂载它"),
                cx,
            );
            return;
        }
    };
    let url = rt
        .block_on(manager.get_connection_config(&conn_id.to_string()))
        .and_then(|config| config.url_override)
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| info.url.clone());
    let source = match engine::duckdb::accel::AccelSource::new(conn_id, &info.db_type, &url) {
        Ok(source) => source,
        Err(reason) => {
            shared.say_error(format!("挂载失败：{reason}"), cx);
            return;
        }
    };
    // 会话 owner：面板挂载目前用固定 owner（`data_objects`）。等「面板跟随当前编辑器文档」
    // 那一步落地后改成文档 id（见 `docs/architecture/resource-session-model.md` §5.2）。
    match engine::duckdb::accel::ensure_on_instance(&source, "data_objects") {
        Ok((_session, alias)) => {
            refresh(shared, cx);
            shared.say(
                format!("已挂载 {conn_id}（本地加速档，catalog 名 {alias}）"),
                cx,
            );
        }
        Err(reason) => shared.say_error(format!("挂载失败：{reason}"), cx),
    }
}

/// 卸下一条网络连接在**分析实例**上的挂载（源段右键「卸载」）：只断挂载，不动连接本身。
pub fn unmount_network_source(shared: &Shared, conn_id: &str, cx: &mut App) {
    let outcome = engine::duckdb::instance::current()
        .map(|instance| instance.unmount_conn(conn_id))
        .unwrap_or(Ok(false));
    match outcome {
        Ok(true) => {
            refresh(shared, cx);
            shared.say(format!("已卸载 {conn_id}（连接本身没断）"), cx);
        }
        Ok(false) => shared.say_error(format!("{conn_id} 没有挂载可卸"), cx),
        Err(reason) => shared.say_error(format!("卸载失败：{reason}"), cx),
    }
}

/// 把一条连接登记进项目库 `analysis_sources`（**幂等**：已登记就不再插第二行）。
///
/// 「有值即登记」的落点（用户口径 2026-11-26）：连接开关有值 = 它能被 DuckDB 直连，
/// 项目登记表里就必须有它；调用方（面板「加源」/ 右键「登记为项目源」）共用这一处，
/// 免得两个入口各写一套、别名分配各挤一次。
///
/// 只登记**意图**，不顺手挂载：挂载是会话事实（见 [`mount_network_source`]）。
fn ensure_registration(shared: &Shared, conn_id: &str, name: &str) -> Result<Option<String>, String> {
    let root = shared
        .project_root()
        .ok_or_else(|| "还没有打开项目，登记不落在任何项目里".to_string())?;
    // 幂等 + 别名分配都在 engine 那一处（连接对话框保存也走它，两个入口不许各写一套）
    engine::analysis::registration::ensure_conn_registered(&root, conn_id, name)
}

/// 把一条连接**登记成项目源**（写 `analysis_sources`，别名按规格 §6 取第一个）。
///
/// 只登记意图，不顺手挂载：挂载是会话事实（见 [`mount_network_source`]），
/// 两件事分开，用户才能看懂「未挂载 · 项目级」这一格。
pub fn register_connection_source(shared: &Shared, conn_id: &str, name: &str, cx: &mut App) {
    match ensure_registration(shared, conn_id, name) {
        Ok(Some(alias)) => {
            refresh(shared, cx);
            shared.say(format!("已登记为项目源：{name} → 别名 {alias}"), cx);
        }
        Ok(None) => {
            refresh(shared, cx);
            shared.say(format!("这条连接已经登记过（幂等，未重复写）：{name}"), cx);
        }
        Err(error) => shared.say_error(error, cx),
    }
}

/// 删掉一条登记（源段右键「移除登记」）：只删意图，实例里挂着的会话不动。
pub fn unregister_source(shared: &Shared, id: &str, cx: &mut App) {
    let Some(root) = shared.project_root() else {
        shared.say_error("还没有打开项目，登记不在任何项目里".to_string(), cx);
        return;
    };
    let conn = match engine::persistence::analysis_sources::open_in_project(&root) {
        Ok(conn) => conn,
        Err(error) => {
            shared.say_error(format!("打开项目库失败：{error}"), cx);
            return;
        }
    };
    match engine::persistence::analysis_sources::delete(&conn, id) {
        Ok(true) => {
            refresh(shared, cx);
            shared.say("已移除登记（挂着的会话没动）".to_string(), cx);
        }
        Ok(false) => shared.say_error("这条登记已经不在了".to_string(), cx),
        Err(error) => shared.say_error(format!("移除登记失败：{error}"), cx),
    }
}

/// 挂载 / 卸载 / 登记之后统一重取清单：面板与本视图都跟着新事实重排。
fn refresh(shared: &Shared, cx: &mut App) {
    let collected = data_objects::collect(shared);
    *shared.data_objects.borrow_mut() = data_objects::objects(&collected);
    shared.mark_data_objects_dirty(cx);
}

#[cfg(test)]
mod tests {
    use super::LoadScope;

    /// 两档 scope 的说法必须点明「切项目之后还在不在」——这是用户做选择的唯一依据。
    #[test]
    fn scope_hints_name_the_lifetime() {
        assert!(LoadScope::Session.hint().contains("切项目即清"));
        assert!(LoadScope::Snapshot.hint().contains("跟着项目"));
        // 两档标题也不能一样（列出同一批文件却不说落点，等于让用户猜）
        assert_ne!(LoadScope::Session.title(), LoadScope::Snapshot.title());
        assert!(LoadScope::Snapshot.title().contains("快照"));
        assert!(LoadScope::Session.title().contains("按需"));
    }
}
