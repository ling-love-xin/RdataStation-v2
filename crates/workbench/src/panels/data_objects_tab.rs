//! 「数据对象」右 dock tab：所有可参与分析的对象一处分览。
//!
//! 取数在**事件路径**（构造 / 刷新）跑一次并写进 `Shared::data_objects`，渲染只读缓存——
//! `collect` 内含读盘与查库，放进 render 就是每帧 I/O（gpui-kit-dev 禁止）。
//!
//! 源行展开的枚举**另有后台通道**（[`crate::services::source_kids_jobs`]）：远端库逐层
//! introspect 是网络往返，跑在 UI 线程上会让面板每秒顿一下；回执由宿主泵取回再写进缓存。
//!
//! 行上四类动作各管一段：勾选进工作集（`toggle_picked`）、会话行菜单（重挂 / 设主源 /
//! 关会话）、结果集与洞察产物的「升级为资产」（进项目分析库的持久表，写库在 worker 线程）、
//! 资产行的「升级」（表型存档 → 全局，搬表也在 worker 线程）。
//!
//! 前两个是场景 3 的两段，顺序不能颠倒：临时表在进程单例里（项目库打不开它），
//! 所以先「升级为资产」拿到存档身份，才谈得上「升级到全局」并留下版本线索。

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::component::empty::{
    Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle,
};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tag::Tag;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use shared::i18n::tr;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use workbench_shell::panel_toolbar::PanelToolbar;
use workbench_shell::{tree, ui};

use crate::components::data_objects::{self, ExpandVia, Group};
use crate::view::RightPanel;

use super::Shared;

/// 面板底部那一行提示：成功与失败不能一个颜色，否则失败的话会被当背景读过去
#[derive(Clone)]
struct PanelNote {
    text: String,
    is_error: bool,
}

/// 一次画几张叶子（源行展开后的表 / 视图行）。
///
/// 与导航展开文件夹同一档（`ui::NAV_FOLDER_PAGE_SIZE`）：一个连接底下几百上千张表是常态，
/// 全量铺在单层滚动容器里就是「每帧几百上千个元素」——gpui 照单全画，滚动就掉帧。
/// 先画一页，底部一条「显示更多（还有 N 张）」，点一次加一页。
const LEAF_PAGE_SIZE: usize = ui::NAV_FOLDER_PAGE_SIZE;

/// 这个源当前该画几张叶子：`pages` 为空 = 还没点过「显示更多」（只画第一页）。
///
/// 只跟「已翻页数」有关，与清单长度无关——清单一换（刷新 / 换连接）就按现在的长度取小，
/// 不留下「翻过的页数大于清单」这种越界态。
fn leaf_shown(len: usize, pages: Option<usize>) -> usize {
    let pages = pages.unwrap_or(1).max(1);
    len.min(pages.saturating_mul(LEAF_PAGE_SIZE))
}

impl PanelNote {
    fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

/// 展开后的叶子清单由 [`crate::services::source_kids_jobs`] 在 worker 线程取回
use crate::services::source_kids_jobs::{self, Kids};

pub struct DataObjectsTab {
    shared: Shared,
    focus_handle: FocusHandle,
    /// 上一次动作的结果；`None` = 没动作过或已恢复
    note: Option<PanelNote>,
    /// 哪一行的会话动作条展开着：一屏只开一个，跟原型一致
    expanded_session: Option<String>,
    /// 分组筛选（四段 + 全部，2026-11-18 合并后 5 个 chips）：`None` = 全部
    filter: Option<Group>,
    /// 上次刷新时的进程内存读数（字节, 临时表数）；`None` = 读不到
    memory: Option<(u64, usize)>,
    /// 上次刷新时的会话挂载统计（已挂, 总数）；总数 0 = 这个文档没开会话
    session: Option<(usize, usize)>,
    /// 收起的段：段头照画（带计数），行不画——空段也得让人知道它空着
    collapsed: Vec<Group>,
    /// 展开着的源行（键 = 源行 key）。展开态**跨刷新保留**：会话可能被重建，
    /// 清单在每次 `refresh` 时对仍展开的源重取一遍（见 [`Self::refresh`] 尾段）
    expanded_sources: HashSet<String>,
    /// 源行展开出的表 / 视图清单（键 = 源行 key；`Arc` 让 render 只引用不拷贝）。
    /// 列举走引擎（事件路径写入），渲染纯读
    source_children: HashMap<String, Arc<Vec<engine::duckdb::accel::MountedTable>>>,
    /// **源库连接**展开出的表（键 = `conn:{conn_id}`）。与上一条分开存：
    /// 那一条是「已挂进 DuckDB 的源」的目录行（`MountedTable` 带限定名规则），
    /// 这一条是连接层的 `TableCandidate` 翻成的 Object（可选用、可取样，形状不同）
    conn_children: HashMap<String, Arc<Vec<data_objects::Object>>>,
    /// 正在后台列举的源（键 = 源行 key）：同一源不重复提交，展开时画「正在列举」
    loading_sources: HashSet<String>,
    /// 每个源已经翻过几页叶子（键 = 源行 key）：`None` / 没记录 = 第一页。
    /// 见 [`LEAF_PAGE_SIZE`] 与 [`leaf_shown`]——叶子多了只画一页，点「显示更多」才加。
    leaf_pages: HashMap<String, usize>,
    /// 收起的 schema 分组（键 = `{源行 key}|{层名}`）：三级树的第二层，点击切换。
    /// 默认全展开——`rds_src` / `f_*` 这种库往往只有一个 schema，收起来反而多点一次。
    collapsed_schemas: HashSet<String>,
    /// 导出格式不在这里：它是「这一次导出」的属性，活在导出窗口里（见 `components/export_dialog.rs`）
    /// 源段里「文件库还没挂」的行（行 key → 本体路径）：右键「挂载」用这一份
    unmounted_sources: Vec<(String, String)>,
    /// 源段里「文件库已经挂上」的行（行 key → 本体路径）：右键「卸载 / 重新挂载」用这一份
    mounted_sources: Vec<(String, String)>,
    /// 网络连接里「连上了、但没挂进实例」的行（连接 id）：右键「挂载」用这一份。
    /// 与文件库那条线分开：网络连接判的是加速会话，不是 `loaded_files` 路径。
    unmounted_network: Vec<String>,
    /// 网络连接里「加速会话在」的行（连接 id）：右键「卸载」用这一份
    mounted_network: Vec<String>,
    /// 项目登记表这份快照：`conn:` 行据此给「登记 / 移除登记」，`reg:` 行据此认本体
    registrations: Vec<engine::persistence::analysis_sources::AnalysisSourceRow>,
    /// 上次同步到的活动文档 key（`None` = 还没对上 / 没有文档）。
    /// 与 `Shared::working_set.current` 比对：不同就说明用户切了文档，工作集要换那一份
    active_doc_key: Option<String>,
    /// 上次同步到的切文档版本号（与 `data_objects_epoch` 同一手法：计数变了才动）
    seen_document_epoch: u64,
    /// 上次落盘 / 读盘用的项目根：切项目要先把旧项目那份存掉再读新的（见 `follow_active_document`）
    working_set_root: Option<std::path::PathBuf>,
    /// 清单版本号（`Shared::data_objects_epoch`）上次同步到的值：变了就重取，
    /// 让面板跟上「连接变化 / 查询产出结果集 / 加源 / 切项目」
    seen_epoch: u64,
    /// 过滤后的可见行缓存：`(段筛, 搜索词, 行)`。
    ///
    /// render 每帧都要这份清单，而 `data_objects` 在展开源库连接后可能是上千条——
    /// 每帧「整表克隆 + 逐条筛 + 再逐条克隆命中项」是纯浪费（一次滚动就几千次分配）。
    /// 只在三件事发生时重建：清单变了（[`Self::refresh`] 写缓存后 / [`Self::render`] 发现
    /// 外部版本号变了）、段筛变了、搜索词变了。
    rows_cache: Option<(Option<Group>, String, Vec<data_objects::Object>)>,
}

impl DataObjectsTab {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        let shared_epoch = shared.data_objects_epoch.get();
        let (active_doc_key, seen_document_epoch) = {
            let working_set = shared.working_set.borrow();
            (working_set.current.clone(), working_set.epoch)
        };
        let mut tab = Self {
            shared,
            focus_handle: cx.focus_handle(),
            note: None,
            expanded_session: None,
            filter: None,
            memory: None,
            session: None,
            collapsed: Vec::new(),
            expanded_sources: HashSet::new(),
            source_children: HashMap::new(),
            conn_children: HashMap::new(),
            loading_sources: HashSet::new(),
            leaf_pages: HashMap::new(),
            collapsed_schemas: HashSet::new(),
            unmounted_sources: Vec::new(),
            mounted_sources: Vec::new(),
            unmounted_network: Vec::new(),
            mounted_network: Vec::new(),
            registrations: Vec::new(),
            active_doc_key,
            seen_document_epoch,
            // `None` = 还没读过盘：构造那次 `refresh` 会按当前项目根读一遍
            working_set_root: None,
            // 初值故意落后于 Shared（`refresh` 里会同步成当前值）：构造那次刷新已经取过，
            // 不能让「首次 render 又发现 epoch 变了」再白重取一遍
            seen_epoch: shared_epoch,
            rows_cache: None,
        };
        tab.refresh(cx);
        tab
    }

    /// 删掉清单里的临时对象（进程内存里的临时表；删了要重跑 / 重新载入才有）。
    ///
    /// 范围由 [`data_objects::can_clear`] 定：结果集（`tmp_q_`）与载入（`tmp_l_`）——
    /// 前者删了要重跑查询，后者删了要重新从源里载。洞察取样 / Mock 同段但归各自模块管，
    /// 不从这里删（删了它们模块的登记就指向空表）。
    /// 属于破坏性动作，调用前必须过确认框；这里只管执行与把失败原话留在面板上。
    fn clear_temp_objects(&mut self, cx: &mut Context<Self>) {
        let names: Vec<String> = self
            .shared
            .data_objects
            .borrow()
            .iter()
            .filter(|object| data_objects::can_clear(object))
            .map(|object| object.name.clone())
            .collect();
        if names.is_empty() {
            self.note = Some(PanelNote::info("清单里没有临时对象"));
            cx.notify();
            return;
        }
        let mut dropped = 0usize;
        let mut failed: Vec<String> = Vec::new();
        {
            let connection = engine::duckdb::DuckDBManager::global();
            match connection.lock() {
                Ok(guard) => {
                    for name in &names {
                        let Some(source) = data_objects::temp_source_for(name) else {
                            failed.push(name.clone());
                            continue;
                        };
                        match engine::duckdb::temp_table::drop_temp_table(&guard, source, name) {
                            Ok(()) => dropped += 1,
                            // 表已经不在了也算清掉：目标是不留残留
                            Err(error) => failed.push(format!("{name}：{error}")),
                        }
                    }
                }
                Err(error) => {
                    failed.push(format!("拿不到进程库锁：{error}"));
                }
            }
        }
        self.note = Some(if failed.is_empty() {
            PanelNote::info(format!("已清理 {dropped} 个临时对象"))
        } else {
            PanelNote::error(format!(
                "清理 {dropped} 个，{} 个没清掉：{}",
                failed.len(),
                failed.join("；")
            ))
        });
        self.refresh(cx);
    }

    /// 删掉**一张**临时表（单行右键「清理这张表…」）。
    ///
    /// 与 [`Self::clear_temp_objects`] 的分工：那个是工具栏的全清（删掉清单里所有
    /// 结果集 / 载入），这个是"只删这一张"——用户常常只想丢掉一份载入的副本，
    /// 不该为此把别人的结果集一起清掉。来源仍由 [`data_objects::temp_source_for`]
    /// 按前缀认（`drop_temp_table` 会再校验一次，传错来源直接拒）。
    pub(crate) fn clear_one(&mut self, table: &str, cx: &mut Context<Self>) {
        let Some(source) = data_objects::temp_source_for(table) else {
            self.note = Some(PanelNote::error(format!(
                "清理 {table}：认不出这是哪一档临时表（只认结果集 / 载入）"
            )));
            cx.notify();
            return;
        };
        let outcome = engine::duckdb::DuckDBManager::global()
            .lock()
            .map_err(|error| format!("拿不到进程库锁：{error}"))
            .and_then(|guard| {
                engine::duckdb::temp_table::drop_temp_table(&guard, source, table)
                    .map_err(|error| error.to_string())
            });
        match outcome {
            Ok(()) => {
                self.note = Some(PanelNote::info(format!("已清理 {table}")));
                self.shared.mark_data_objects_dirty(cx);
            }
            Err(reason) => {
                self.note = Some(PanelNote::error(format!("清理 {table} 失败：{reason}")));
            }
        }
        self.refresh(cx);
    }

    /// 收起 / 展开一段（段头 caret 回调）
    fn toggle_group(&mut self, group: Group, cx: &mut Context<Self>) {
        if let Some(index) = self.collapsed.iter().position(|item| item == &group) {
            self.collapsed.remove(index);
        } else {
            self.collapsed.push(group);
        }
        cx.notify();
    }

    /// 执行一个会话动作（重挂 / 设主源 / 关闭会话），结果写进面板底部一行。
    ///
    /// 不吞错误：失败时把引擎原话留在面板上——用户点了没反应是最难查的那种。
    fn run_session_action(&mut self, action: data_objects::SessionAction, cx: &mut Context<Self>) {
        use data_objects::SessionAction;
        // 三个动作都直接落在**分析实例的会话**上：`federation::session` 那套按 owner 查
        // `SESSIONS` 的门面已经没有写入方（生产路径只开实例会话），按它找必然找不到。
        let outcome = match action {
            SessionAction::RefreshSource { owner, alias } => {
                engine::duckdb::instance::current()
                    .ok_or_else(|| "分析实例尚未启动（先跑一条查询）".to_string())
                    .and_then(|instance| {
                        instance.session_for_owner(&owner).ok_or_else(|| {
                            "联邦会话已经不在了（重跑一条联邦查询再试）".to_string()
                        })
                    })
                    .and_then(|session| {
                        let source = session
                            .mounted_sources()
                            .into_iter()
                            .find(|entry| entry.alias() == alias)
                            .map(|entry| entry.source)
                            .ok_or_else(|| format!("会话里没有源 {alias}"))?;
                        session
                            .mount_sources(&[source])
                            .map(|_| ())
                            .map_err(|error| error.to_string())
                    })
            }
            SessionAction::SetPrimary { owner, alias } => {
                engine::duckdb::instance::current()
                    .ok_or_else(|| "分析实例尚未启动（先跑一条查询）".to_string())
                    .and_then(|instance| {
                        instance.session_for_owner(&owner).ok_or_else(|| {
                            "联邦会话已经不在了（重跑一条联邦查询再试）".to_string()
                        })
                    })
                    .and_then(|session| {
                        let ready = session
                            .mounted_sources()
                            .iter()
                            .any(|entry| entry.alias() == alias && entry.is_ready());
                        if !ready {
                            return Err(format!("源 {alias} 当前不可用，不能设为主源"));
                        }
                        session
                            .use_catalog(&alias)
                            .map_err(|error| error.to_string())
                    })
            }
            SessionAction::DropSession { owner } => {
                engine::duckdb::instance::current()
                    .ok_or_else(|| "分析实例尚未启动（先跑一条查询）".to_string())
                    .and_then(|instance| {
                        instance
                            .session_for_owner(&owner)
                            .ok_or_else(|| format!("没有这个会话：{owner}"))
                            .map(|session| (instance, session))
                    })
                    .and_then(|(instance, session)| {
                        // 后端返 bool：没这个会话就是没关成，别报成功
                        if instance.close_session(session.id()) {
                            Ok(())
                        } else {
                            Err(format!("没有这个会话：{owner}"))
                        }
                    })
            }
        };
        self.note = Some(match outcome {
            Ok(()) => PanelNote::info("已更新会话，清单已刷新"),
            Err(reason) => PanelNote::error(format!("操作失败：{reason}")),
        });
        // 动作改了会话表清单：重取一次，别让行上的表数停在旧值
        self.refresh(cx);
    }

    /// 重新取一次清单（事件路径）。
    ///
    /// 资产那段不走库查询：资产面板实体已经把存活行载在内存里（`view_rows`），
    /// 这里只做形状翻译。资产面板在侧栏构造期就建好并**预载一次**（`build_resources_panel`
    /// 尾部的 `refresh_resources`，注释点名就是为本面板预载的），项目切换与资产增删也都会
    /// 推新快照——所以这里**永远有行可读**，不需要自己开连接查库（那是两套真相）。
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        // 同步版本号：这次取的就是当前现实，下一次 render 不该再判「脏了」
        self.seen_epoch = self.shared.data_objects_epoch.get();
        let mut collected = data_objects::collect(&self.shared);
        // 内存读数与会话统计都在这里取：render 每帧跑，不能在里面查库。
        // `None` 不算失败——上限没设、pragma 不支持都是正常形态，界面显示「—」。
        self.memory = engine::duckdb::DuckDBManager::memory_stats()
            .ok()
            .and_then(|stats| stats.database_bytes.map(|bytes| (bytes, stats.table_count)));
        let mounted = collected
            .session_sources
            .iter()
            // `failed` 是失败原话：`None` = 挂上了
            .filter(|source| source.failed.is_none())
            .count();
        self.session = Some((mounted, collected.session_sources.len()));
        // 文件库源里「注册了但还没挂」的那几行：面板据此把 chevron 换空位、右键给「挂载」
        self.unmounted_sources = data_objects::unmounted_file_sources(&collected);
        // 已挂的那几行：右键给「卸载 / 重新挂载」（同一份判据，反过来取）
        self.mounted_sources = data_objects::mounted_file_sources(&collected);
        // 网络连接那条线：判据是加速会话（与文件库的 `loaded_files` 路径互不相干）
        self.mounted_network = collected
            .accel_sources
            .iter()
            .map(|source| source.owner.clone())
            .collect();
        self.unmounted_network = collected
            .connections
            .iter()
            .filter(|conn| {
                conn.connected
                    // 文件库走上面那条线（路径判），这里只管网络库
                    && data_objects::file_source_path(conn).is_none()
                    && !self.mounted_network.iter().any(|id| id == &conn.id)
            })
            .map(|conn| conn.id.clone())
            .collect();
        self.registrations = collected.registered_sources.clone();
        collected.assets = self.assets_from_panel(cx);
        // 资产行单独存一份：`Object` 只带名字与版本，构造来源还要本体路径
        *self.shared.data_object_assets.borrow_mut() = collected.assets.clone();
        *self.shared.data_objects.borrow_mut() = data_objects::objects(&collected);
        // 清单换了：可见行缓存跟着作废（否则面板会一直画上一次的那份行）
        self.rows_cache = None;
        // 项目 / 活动文档可能一起变了（切项目的动作也会 bump 清单版本号）：
        // 工作集账本在这里对齐，别等到渲染里发现
        self.follow_active_document(cx);
        // 展开着的源行：清单重取一遍（会话可能被重建，旧清单会骗人）。
        // 刷新是例行动作，失败不刷提示——真正的展开点击才把原因亮出来。
        // 取数在 worker 线程（`source_kids_jobs`），回执由宿主泵写回，这里只提交
        let expanded: Vec<String> = self.expanded_sources.iter().cloned().collect();
        for key in expanded {
            self.request_source_children(&key, false);
        }
        cx.notify();
    }

    /// 展开 / 收起一个源行（联邦源与本地加速源可展开，其余行没有下一层）。
    ///
    /// `expand_via` 有值 = 这一行**没有自家挂载的表可列**：它是登记过、但挂不进联邦的
    /// 连接（`use_duckdb_fed` 开着却拿不到 ATTACH 能力）。展开改成在这条活连接上
    /// 开「加源」选源器，用户在那里选表 → 载入临时表（`LoadScope::Session`），
    /// 走的是和导航栏同一份内省（`NavigatorService`），不是第二套取数。
    fn toggle_source_expand(
        &mut self,
        key: &str,
        expand_via: Option<ExpandVia>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ExpandVia::Picker { conn_id }) = expand_via {
            crate::components::data_objects_load::open_load_dialog_for_connection(
                &self.shared,
                &conn_id,
                crate::components::data_objects_load::LoadScope::Session,
                window,
                cx,
            );
            return;
        }
        if self.expanded_sources.contains(key) {
            self.expanded_sources.remove(key);
            // 收起不清缓存：再展开立即可见，refresh 会让它跟上现实
        } else {
            self.expanded_sources.insert(key.to_string());
            // 展开即提交后台列举：远端库逐层内省不占 UI 线程
            self.request_source_children(key, true);
        }
        cx.notify();
    }

    /// 点「显示更多」：这个源多画一页叶子（见 [`LEAF_PAGE_SIZE`]）。
    fn show_more_leaves(&mut self, key: &str, cx: &mut Context<Self>) {
        let pages = self.leaf_pages.get(key).copied().unwrap_or(1);
        self.leaf_pages
            .insert(key.to_string(), pages.saturating_add(1));
        cx.notify();
    }

    /// 提交一次源行列举到 worker（立即返回，**不阻塞 UI**）。
    ///
    /// `announce`：用户点击展开时失败要给提示（否则点了像没反应）；例行刷新的重取则静默，
    /// 避免每秒弹一遍旧错误。重绘由调用方负责（这里不 `notify`）。
    ///
    /// 同一源已在列举中就不重复提交：刷新是 1s 一次的，展开中的源会被扫到很多次，
    /// 每次都排队只会把 worker 压满、回执更晚。
    fn request_source_children(&mut self, key: &str, announce: bool) {
        if self.loading_sources.contains(key) {
            return;
        }
        let Some(source) = data_objects::source_ref(key) else {
            return;
        };
        // 连接行要的两样宿主信息（`Shared` 不能跨线程，先在 UI 线程取出来）：
        // - 已挂载的文件库本体路径：文件库的叶子靠它算 ATTACH 别名（挂没挂看 `loaded_files`）
        // - 行上的名字：列举失败时用人名报错，不把 `conn:C_xxx` 甩给用户
        let mounted_file = self
            .mounted_sources
            .iter()
            .find(|(row_key, _)| row_key == key)
            .map(|(_, path)| path.clone());
        let display = self
            .shared
            .data_objects
            .borrow()
            .iter()
            .find(|object| object.key == key)
            .map(|object| object.name.clone())
            .unwrap_or_else(|| key.to_string());
        match source_kids_jobs::submit(key.to_string(), source, mounted_file, display, announce) {
            Ok(()) => {
                self.loading_sources.insert(key.to_string());
            }
            Err(reason) if announce => {
                self.note = Some(PanelNote::error(reason));
            }
            Err(_) => {}
        }
    }

    /// 「载入内存」回执的**一次性**小泵（铁律 H 组第 35 条 / E 组第 25 条）。
    ///
    /// 载入的搬运在 worker 线程（[`crate::services::load_jobs`]）；这里只等**自己那一次**
    /// 的回执（按任务号认领，连点两次也不会拿错），说完 + 标脏就结束，不常驻。
    /// 不并进宿主泵：那是导出回执的队列，两条队列混住会让"谁取走"变成两处都要知道的事。
    pub(crate) fn watch_load(&mut self, job: u64, cx: &mut Context<Self>) {
        self.watch_job(job, crate::services::load_jobs::take_for, cx);
    }

    /// 「归档到分析库」回执的一次性小泵（第 35 条前半 / 第 25 条）。
    ///
    /// 归档成功 → 项目分析库多一张持久表、资产列表多一条，两处清单都过期：
    /// 除了标脏数据对象，还要让导航缓存失效（新表在导航的「项目分析库」下也要出现）。
    pub(crate) fn watch_archive(&mut self, job: u64, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(200)).await;
                let Some((note, ok)) = crate::services::archive_jobs::take_for(job) else {
                    continue;
                };
                let _ = this.update(cx, |tab, cx| {
                    if ok {
                        tab.shared.say(note, cx);
                        tab.shared.invalidate_nav_cache();
                        tab.shared.mark_data_objects_dirty(cx);
                    } else {
                        tab.shared.say_error(note, cx);
                    }
                });
                return;
            }
        })
        .detach();
    }

    /// 载入 / 归档共用的一次性回执泵：按任务号认领，说完 + 标脏就结束，不常驻。
    fn watch_job(
        &mut self,
        job: u64,
        take: fn(u64) -> Option<(String, bool)>,
        cx: &mut Context<Self>,
    ) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(200)).await;
                let Some((note, ok)) = take(job) else {
                    continue;
                };
                let _ = this.update(cx, |tab, cx| {
                    if ok {
                        tab.shared.say(note, cx);
                        tab.shared.mark_data_objects_dirty(cx);
                    } else {
                        tab.shared.say_error(note, cx);
                    }
                });
                return;
            }
        })
        .detach();
    }

    /// 回执写回缓存（宿主泵调用）。`key` 仍然展开着才写——用户可能已经收起了。
    pub(crate) fn apply_kids(
        &mut self,
        outcome: source_kids_jobs::Outcome,
        cx: &mut Context<Self>,
    ) {
        self.loading_sources.remove(&outcome.key);
        match outcome.result {
            Ok(Kids::Mounted(tables)) => {
                self.source_children.insert(outcome.key, Arc::new(tables));
            }
            Ok(Kids::Connection(objects)) => {
                self.conn_children.insert(outcome.key, Arc::new(objects));
            }
            Err(reason) => {
                if outcome.announce {
                    self.note = Some(PanelNote::error(reason));
                }
            }
        }
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
                resource_id: row.id,
                // 表型才有表名（`kind` 已按表名判过，见 `ArchiveKind::resolve`）。
                // 行上不显示它，但「升级到全局」需要它——面板只拿得到行，拿不到详情。
                analytics_table: row.analytics_table,
                name: row.name,
                detail: row.tail,
            })
            .collect()
    }

    /// 勾 / 取消一个对象进工作集（存 `key`，不存行号：列表会重排）。
    ///
    /// **多选**（2026-11-22 改回）：工作集是「本次分析要用的对象」的集合，一次只留一个
    /// 会让软上限（`PICKED_SOFT_LIMIT`）与底部提示变成死代码，也没法表达「这几张表一起看」。
    /// 增删语义在 `data_objects::toggle_in_place`（纯函数，有单测钉住计数真的会长）。
    fn toggle_picked(&mut self, key: &str, cx: &mut Context<Self>) {
        data_objects::toggle_in_place(&mut self.shared.data_object_picked.borrow_mut(), key);
        // 工作集跟文档走、并且落盘：勾一下就要留下（下次打开这份文档还认得）
        self.save_working_set();
        cx.notify();
    }

    /// 工作集落在哪个项目根下（`None` = 没项目 → 只在内存里，不落盘）
    fn working_set_root_now(&self) -> Option<std::path::PathBuf> {
        self.shared
            .project
            .borrow()
            .as_ref()
            .map(|p| p.root.clone())
    }

    /// 把「活动文档的当前选择」回存进账本。
    ///
    /// 为什么账本不是唯一真相：活动那一份就摆在 `data_object_picked` 里（渲染每帧都要读它），
    /// 账本只在「切文档 / 落盘」这两个时刻需要它是最新的——所以那两个时刻先回存，别的一律不动。
    fn flush_active_picks(&self) {
        let Some(key) = self.active_doc_key.clone() else {
            return;
        };
        let picks = self.shared.data_object_picked.borrow().clone();
        self.shared
            .working_set
            .borrow_mut()
            .by_document
            .insert(key, picks);
    }

    /// 落盘（一份小 JSON，事件路径同步写；失败只说一句日志，不打断操作）
    fn save_working_set(&self) {
        let root = self.working_set_root_now();
        let Some(root) = root else {
            return;
        };
        self.flush_active_picks();
        let sets = self.shared.working_set.borrow().by_document.clone();
        if let Err(error) = crate::services::working_set::save(Some(&root), &sets) {
            tracing::warn!(%error, "保存数据对象工作集失败");
        }
    }

    /// 跟上「项目」和「活动文档」（**事件路径**：会读 / 写盘，不能放渲染里）。
    ///
    /// 两件事合成一个入口，因为它们的顺序有依赖：换项目要先存旧项目那份、再读新项目那份，
    /// 之后才谈得上「新项目里当前文档勾了什么」。
    pub(crate) fn follow_active_document(&mut self, cx: &mut Context<Self>) {
        // ① 换项目 / 头一次：工作集是项目内的文件，换项目必须换账本
        let root = self.working_set_root_now();
        if root != self.working_set_root {
            self.flush_active_picks();
            if let Some(old) = self.working_set_root.clone() {
                let sets = self.shared.working_set.borrow().by_document.clone();
                if let Err(error) = crate::services::working_set::save(Some(&old), &sets) {
                    tracing::warn!(%error, "切项目时保存工作集失败");
                }
            }
            let loaded = crate::services::working_set::load(root.as_deref());
            {
                let mut working_set = self.shared.working_set.borrow_mut();
                working_set.by_document = loaded;
                working_set.epoch = working_set.epoch.wrapping_add(1);
            }
            self.working_set_root = root;
            // 旧项目里勾的表 key 在新项目里没有意义：清掉，下面 ② 按新账本重取
            self.active_doc_key = None;
            *self.shared.data_object_picked.borrow_mut() = Vec::new();
        }

        // ② 活动文档变了：把自己这份回存，再换成新文档那一份
        let (epoch, current) = {
            let working_set = self.shared.working_set.borrow();
            (working_set.epoch, working_set.current.clone())
        };
        if epoch != self.seen_document_epoch || current != self.active_doc_key {
            self.seen_document_epoch = epoch;
            if current != self.active_doc_key {
                self.flush_active_picks();
                let picks = current
                    .as_ref()
                    .and_then(|key| {
                        self.shared
                            .working_set
                            .borrow()
                            .by_document
                            .get(key)
                            .cloned()
                    })
                    .unwrap_or_default();
                *self.shared.data_object_picked.borrow_mut() = picks;
                self.active_doc_key = current;
                self.save_working_set();
            }
            cx.notify();
        }
    }

    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // 只借不拷：清单在展开源库连接后可能上千条，每帧整表克隆（每条 3~4 个 String）
        // 是滚动时最大的一笔无谓分配。条数、名字回查都按需借。
        let total = self.shared.data_objects.borrow().len();
        // 搜索框：`InputState` 要窗口才能建，面板 `new` 没有窗口，所以按 key 在渲染时惰性建
        // （同一 id 每次拿到的是同一个实体，输入内容不会因重渲染丢）
        let search = window.use_keyed_state("data-objects-search", cx, |window, cx| {
            InputState::new(window, cx).placeholder(tr("输入表名 / 文件名，直接定位"))
        });
        let query = search.read(cx).value().trim().to_lowercase();

        // 可见行缓存：段筛 / 搜索词 / 清单三者都没变就不重筛（见字段上的注释）。
        // 筛选只改“看得见哪些”，不动工作集：选了某个对象后换个段看，已选仍在
        let cache_stale = match &self.rows_cache {
            Some((filter, seen_query, _)) => *filter != self.filter || *seen_query != query,
            None => true,
        };
        if cache_stale {
            let all = self.shared.data_objects.borrow();
            let rows: Vec<data_objects::Object> = all
                .iter()
                .filter(|object| {
                    self.filter.is_none_or(|group| object.group == group)
                        && (query.is_empty()
                            || object.name.to_lowercase().contains(&query)
                            || object.note.to_lowercase().contains(&query))
                })
                .cloned()
                .collect();
            self.rows_cache = Some((self.filter, query.clone(), rows));
        }
        let objects: &[data_objects::Object] = self
            .rows_cache
            .as_ref()
            .map(|(_, _, rows)| rows.as_slice())
            .unwrap_or_default();
        let picked = self.shared.data_object_picked.borrow().clone();
        // 行内「这行选没选」原来每行扫一遍工作集（行数 × 已选数）：换成集合查
        let picked_set: HashSet<&str> = picked.iter().map(|key| key.as_str()).collect();
        // 头部只说清“这是哪一档引擎、现在占多少”：读数在刷新时取一次，取不到就显示「—」
        // 总和“现在看得见几个”不是一回事：筛过之后得把两个数都说出来
        let count_label = if objects.len() == total {
            format!("{} 个 · 已选 {}", total, picked.len())
        } else {
            format!(
                "{} 个（显示 {}）· 已选 {}",
                total,
                objects.len(),
                picked.len()
            )
        };
        let memory_label = match self.memory {
            Some((bytes, tables)) => format!(
                "进程 DuckDB · {} · {} 张临时表",
                data_objects::format_bytes(bytes),
                tables
            ),
            None => "进程 DuckDB · —".to_string(),
        };
        let colors = cx.theme().colors;
        let entity = cx.entity();

        // 面板可用宽：上一帧 prepaint 量到的（0 = 还没量到，见文件末尾 `RowWidth`）。
        // 行布局按它判档（`data_objects::layout::RowTier`）：窄了先丢执行通道、再丢小标，
        // 把宽度让给名字——判据是实测出来的「名字可用宽 = 面板宽 − 290」，不是估的。
        let row_width =
            window.use_keyed_state("data-objects-row-width", cx, |_, _| RowWidth::default());
        let panel_width = row_width.read(cx).get();
        let tier = data_objects::layout::RowTier::for_width(panel_width);
        // 工具栏按钮在最窄一档退成纯图标：两个中文字的按钮一出现，搜索框就被压成半截。
        // 判据直接复用行布局的 `tier`（同一份实测边界），不另起一个阈值。
        // 首帧 `panel_width == 0` 按 `Full` 画，按钮带字（宁可比真实的宽一帧，不闪）。
        let icon_only = matches!(tier, data_objects::layout::RowTier::DropChannelAndPill);

        // 走马灯的两个前提：名字格实际多宽（prepaint 写入，下一帧读）、鼠标停在哪一行。
        // 名字格宽度**逐行**量（行尾按钮数随行的可点性变化，同宽面板里各行名字格不等宽）。
        let name_widths =
            window.use_keyed_state("data-objects-name-widths", cx, |_, _| NameWidths::default());
        let hovered_row =
            window.use_keyed_state("data-objects-hovered-row", cx, |_, _| HoveredRow::default());

        // 统一容器（`PanelToolbar`）：36 高 / 左缘 12 / 1px 下边界 / 左槽可伸缩 + 右槽固定
        // 都由它决定。左缘 = 外层 `PANEL_TOOLBAR_PADDING`(8) + `Toolbar::p_1`(4) = 12，
        // 与下面的 chips / 状态栏 / 行 / 托盘同一条线。
        //
        // 这一行就是这个面板的头行（Dock 标题栏只做容器管理，没有面板内的第二行标题），
        // 所以取 `PANEL_HEADER_HEIGHT`（2.25rem = 36）。32 是「面板内第二行」的档
        // （资产库那条面板内工具条），夹在一排 36 的头行里横向一眼就差 4px。
        //
        // 这两个动作**不放 Dock 标题栏**：兄弟面板一律把动作留在面板自己的工具栏里
        // （`toolbar_buttons` 返回 None，标题栏只做容器管理），否则同一排标题栏里
        // 有的 tab 有按钮、有的没有，横向也对不齐。
        //
        // 全面板一条左缘（12）：工具栏 / chips / 状态栏 / 行 / 托盘都从这条线起。
        // 上一版是 4（工具栏、chips）、8（状态栏）、12（行）三条线并在一屏里。
        let toolbar = PanelToolbar::new("data-objects-toolbar")
            .left(
                // 搜索框占满除按钮之外的宽度（`min_w_0`：窄面板下允许被压缩）
                div()
                    .flex_1()
                    .min_w_0()
                    .debug_selector(|| "data-objects-search-slot".to_string())
                    // 单行 `Input` 的高度由 `Size` 档定（`sizing.rs::input_h`：Small = `h_6` = 24），
                    // 它的 `.h()` 只对多行输入生效（`input.rs` 里 `.when(is_multi_line)` 才用）。
                    // 上一版写 `CONTROL_HEIGHT_SM`（26）实际画出来是 32（Medium 档），
                    // 与 24 的按钮差 8px——这一行四个控件必须同高。
                    // 样式与左侧导航 / 草稿箱 / 资产库同一档：**无边框 + `input.background`
                    // 填充底**（`bordered(false)` 只去边框、保留底色；`appearance(false)` 才会连底一起去）。
                    // 聚焦时才出边框（`focus_bordered`）——实机反馈 2026-11-25 第 2 条：
                    // 左右两个搜索框一个是填充底、一个是 1px 亮边框，并排看着不是一套控件。
                    .child(
                        Input::new(&search)
                            .small()
                            .bordered(false)
                            .focus_bordered(true),
                    ),
            )
            .right(
                // 外层包一格只为测量（`layout_probe` 量行内控件是否真的同高）；
                // `flex_none` + 一个子，不改变原来的排布
                div()
                    .flex_none()
                    .debug_selector(|| "data-objects-add-source-slot".to_string())
                    .child({
                        let shared = self.shared.clone();
                        let button = Button::new("data-objects-add-source")
                            .ghost()
                            .small()
                            .icon(gpui_kit::assets::IconName::FilePlus)
                            .tooltip(tr("按需加源：挂成视图读原文件，切项目即清"));
                        // 最窄一档只留图标（tooltip 仍给全文案），把宽度让回搜索框
                        let button = if icon_only {
                            button
                        } else {
                            button.label(tr("加源"))
                        };
                        button.on_click(move |_, window, cx| {
                            crate::components::data_objects_load::open_load_files_dialog(
                                &shared,
                                crate::components::data_objects_load::LoadScope::Session,
                                window,
                                cx,
                            );
                        })
                    }),
            )
            .right(
                div()
                    .flex_none()
                    .debug_selector(|| "data-objects-add-source-snapshot-slot".to_string())
                    .child({
                        let shared = self.shared.clone();
                        let button = Button::new("data-objects-add-source-snapshot")
                            .ghost()
                            .small()
                            .icon(gpui_kit::assets::IconName::BookMarked)
                            .tooltip(tr(
                                "把选中的文件在资源库里存一份只读副本，跟着项目走、带版本",
                            ));
                        let button = if icon_only {
                            button
                        } else {
                            button.label(tr("存快照"))
                        };
                        button.on_click(move |_, window, cx| {
                            crate::components::data_objects_load::open_load_files_dialog(
                                &shared,
                                crate::components::data_objects_load::LoadScope::Snapshot,
                                window,
                                cx,
                            );
                        })
                    }),
            )
            .right(
                div()
                    .flex_none()
                    .debug_selector(|| "data-objects-scope-hint-slot".to_string())
                    .child(
                        // 三条口径从常驻说明行收进这里：看一次就够的信息，不该常年占掉两行列表高度
                        Button::new("data-objects-scope-hint")
                            .ghost()
                            .small()
                            .icon(gpui_kit::assets::IconName::Info)
                            .tooltip(tr("只有三段（源 / 表 / 文件）；资产只给最新版本；洞察产物 tmp_i_ 不进本清单；源要展开连接才列表，没挂载的源展不开")),
                    ),
            );

        // 分组 chips：三个段各一个（业务身份是小标，不单独当筛选项）
        // 上留 8（`pt_2`）把工具栏与筛选断开：上一版两者各只有 `pb_2`，边界糊成一块。
        let mut chips = div()
            .h_flex()
            .items_center()
            .flex_wrap()
            .gap_1()
            .px_3()
            .pt_2()
            .pb_2()
            .debug_selector(|| "data-objects-chips".to_string());
        let mut entries: Vec<(Option<Group>, String)> = vec![(None, "全部".to_string())];
        // 段顺序只认 `Group::ALL`（唯一权威）：这里再抄一份列表，加段时漏改就会
        // 「排序里有、筛选项里没有」
        for group in Group::ALL {
            entries.push((Some(group), group.title().to_string()));
        }
        for (option, label) in entries {
            let entity = entity.clone();
            let active = self.filter == option;
            chips = chips.child(
                Button::new(SharedString::from(format!("data-objects-filter-{label}")))
                    .ghost()
                    .toggled(active)
                    .label(label)
                    .on_click(move |_, _, cx| {
                        entity.update(cx, |tab, cx| {
                            tab.filter = option;
                            cx.notify();
                        });
                    }),
            );
        }

        // 当前对象（原型 07 的 tray）：只显示“已选 N 个”看不出是哪 N 个，
        // 所以把名字列出来，每项带个移除
        let mut tray = div()
            .v_flex()
            .gap_1()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(colors.border);
        tray = tray.child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(format!("当前对象 · {} 个", picked.len())),
        );
        let mut selected_row = div().h_flex().items_center().flex_wrap().gap_1();
        // 名字回查按需借（工作集一般只有几个，扫一遍清单是 O(已选 × 清单)，借不拷）
        let all_for_names = self.shared.data_objects.borrow();
        for key in picked.iter() {
            // 名字要能显示：主清单里有的直接取；源库表叶子只在展开时存在（`conn_children`），
            // 折叠后也得显示得出来——最后按身份反解表名兜底，绝不把 `srctable:C_1|…` 露给用户
            let name = all_for_names
                .iter()
                .find(|object| &object.key == key)
                .map(|object| object.name.clone())
                .or_else(|| {
                    self.conn_children
                        .values()
                        .flat_map(|leaves| leaves.iter())
                        .find(|leaf| &leaf.key == key)
                        .map(|leaf| leaf.name.clone())
                })
                .or_else(|| {
                    data_objects::source_table_parts(key).map(|(_, _, _, table)| table.to_string())
                })
                .unwrap_or_else(|| key.clone());
            let key_for_remove = key.clone();
            let entity_for_remove = entity.clone();
            selected_row = selected_row.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(Tag::secondary().rounded_full().child(name))
                    .child(
                        Button::new(SharedString::from(format!("data-object-remove-{key}")))
                            .ghost()
                            .label("×")
                            .small()
                            .on_click(move |_, _, cx| {
                                let key = key_for_remove.clone();
                                entity_for_remove.update(cx, |tab, cx| {
                                    tab.toggle_picked(&key, cx);
                                });
                            }),
                    ),
            );
        }
        drop(all_for_names);
        tray = tray.child(selected_row);
        // 原型 07 的 tray 尾巴：先说清这批对象会被怎么用，再把主按钮放这儿——
        // 头部那排按钮是“对这面板做什么”，这里才是“拿这堆对象去做什么”。
        // 多选（2026-11-22）后必须说清口径：洞察**每源各取一份样本**，不做跨源 join，
        // 所以「选 3 个」不等于「3 个合成一张表」，用户不该靠自己猜。
        tray = tray.child(div().text_xs().text_color(colors.muted_foreground).child(
            match picked.len() {
                0 | 1 => "洞察只取这一个对象；想看别的就再点「选用」，各取各的样本".to_string(),
                count => format!("已选 {count} 个：洞察会为每个对象各取一份样本，不跨源 join"),
            },
        ));
        let shared_for_tray = self.shared.clone();
        let entity_for_tray = entity.clone();
        // 按钮文案跟着选中个数走：选 1 个说「此对象」，选多个说清是「这 N 个」，
        // 免得用户以为多选会被合成一张表（洞察是多源各取一份样本）
        let analyze_label = match picked.len() {
            0 | 1 => "洞察此对象".to_string(),
            count => format!("洞察这 {count} 个对象"),
        };
        tray = tray.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("data-objects-analyze")
                        .label(analyze_label.clone())
                        .on_click(move |_, _, cx| {
                            // 副作用只在事件路径做：转换是纯函数，开洞察在宿主
                            let objects = shared_for_tray.data_objects.borrow().clone();
                            let picked = shared_for_tray.data_object_picked.borrow().clone();
                            let assets = shared_for_tray.data_object_assets.borrow().clone();
                            // 资产本体在 `{项目}/resources/` 下：目录由项目根推，不写死路径
                            let resources_dir = shared_for_tray
                                .project
                                .borrow()
                                .as_ref()
                                .map(|p| p.root.join("resources"));
                            let (sources, skipped) = {
                                // 源库表（`srctable:`）先在宿主里转：取样 SQL 要按驱动方言拼，
                                // 那份知识在 `Shared::insight_sample_sql`（纯函数拿不到连接驱动）
                                let mut sources: Vec<insight::SampleSource> = Vec::new();
                                let mut rest: Vec<String> = Vec::new();
                                for key in &picked {
                                    match data_objects::source_table_parts(key) {
                                        Some((conn, catalog, schema, table)) => {
                                            let sql = shared_for_tray.insight_sample_sql(
                                                conn,
                                                &[catalog, schema, table],
                                            );
                                            let label = [catalog, schema, table]
                                                .into_iter()
                                                .filter(|part| !part.is_empty())
                                                .collect::<Vec<_>>()
                                                .join(".");
                                            let source = insight::SampleSource::new(
                                                conn.to_string(),
                                                sql,
                                                label,
                                            );
                                            if !sources.contains(&source) {
                                                sources.push(source);
                                            }
                                        }
                                        None => rest.push(key.clone()),
                                    }
                                }
                                let (mut more, skipped) = data_objects::analysis_sources(
                                    &objects,
                                    &rest,
                                    &assets,
                                    resources_dir.as_deref(),
                                );
                                sources.append(&mut more);
                                (sources, skipped)
                            };
                            if sources.is_empty() {
                                // 选的全是转不了的类型：别开一个空洞察面板，
                                // 但也不能一声不响——把**是哪几个、为什么**说出来
                                entity_for_tray.update(cx, |tab, cx| {
                                    tab.note = Some(PanelNote::error(format!(
                                        "选中的对象都转不进洞察：{}",
                                        skipped.join("；")
                                    )));
                                    cx.notify();
                                });
                                return;
                            }
                            let label = match skipped.is_empty() {
                                true => format!("{} 个来源", sources.len()),
                                false => format!(
                                    "{} 个来源（未包含：{}）",
                                    sources.len(),
                                    skipped.join("、")
                                ),
                            };
                            shared_for_tray.open_insight_source_tables(sources, label, cx);
                            entity_for_tray.update(cx, |_, cx| cx.notify());
                        }),
                )
                .child(div().flex_1())
                .child({
                    let entity = entity.clone();
                    Button::new("data-objects-clear")
                        .ghost()
                        .label(tr("清空"))
                        .on_click(move |_, _, cx| {
                            // 清空也要落盘：否则下次打开这份文档，刚清掉的那批又回来了
                            let entity = entity.clone();
                            entity.update(cx, |tab, cx| {
                                tab.shared.data_object_picked.borrow_mut().clear();
                                tab.save_working_set();
                                cx.notify();
                            });
                        })
                }),
        );

        // 底部会话条（原型 07 的 sess）：这个文档挂了几源、有几源没挂上，
        // 以及进程里堆着的临时对象清不清
        let session_text = self
            .session
            .filter(|(_, total)| *total > 0)
            .map(|(mounted, total)| {
                let unmounted = total.saturating_sub(mounted);
                match unmounted {
                    0 => format!("已挂 {mounted}/{total} 源"),
                    _ => format!("已挂 {mounted}/{total} 源 · {unmounted} 个未挂载"),
                }
            });
        // 临时对象现在落在「表」段（标「结果集」）：判据仍走 key 前缀，
        // 别拿段判断——段只会说「这是表」，说不清该不该给「清理」这颗按钮
        let has_temp = self
            .shared
            .data_objects
            .borrow()
            .iter()
            .any(data_objects::can_clear);
        let sess_row = (session_text.is_some() || has_temp).then(|| {
            let entity_for_clear = entity.clone();
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(colors.border)
                .text_xs()
                .text_color(colors.muted_foreground)
                .when_some(session_text, |el, text| el.child(text))
                .child(div().flex_1())
                .when(has_temp, |el| {
                    el.child(
                        Button::new("data-objects-clear-temp")
                            .ghost()
                            .label(tr("清理临时对象"))
                            .on_click(move |_, window, cx| {
                                let entity = entity_for_clear.clone();
                                // 删的是进程内存里的临时表：先问一句，别让人一抬手就没了
                                window.open_alert_dialog(cx, move |alert, _window, _cx| {
                                    let entity = entity.clone();
                                    alert
                                        .confirm()
                                        .title(tr("清理临时对象"))
                                        .description("将删掉清单里的临时表（结果集与载入的副本）。它们在进程内存里，删了要重跑 / 重新载入才有；想留就先「升级为资产」或「归档到分析库」。确定继续？")
                                        .on_ok(move |_window, _cx, cx| {
                                            entity.update(cx, |tab, cx| {
                                                tab.clear_temp_objects(cx);
                                            });
                                            true
                                        })
                                });
                            }),
                    )
                })
        });

        let mut list = div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .id("data-objects-list");

        // 每段几行：段头要显示计数。先一遍数出来，别在段头里再扫一遍整表（5 段 = 5 遍）
        let mut group_counts: Vec<(Group, usize)> = Vec::new();
        for object in objects {
            match group_counts
                .iter_mut()
                .find(|(group, _)| *group == object.group)
            {
                Some((_, count)) => *count += 1,
                None => group_counts.push((object.group, 1)),
            }
        }

        let mut last_group: Option<Group> = None;
        for object in objects {
            if last_group != Some(object.group) {
                last_group = Some(object.group);
                // 段头带计数：这一段里几行，扫一眼就知道要不要往下翻
                let group_count = group_counts
                    .iter()
                    .find(|(group, _)| *group == object.group)
                    .map(|(_, count)| *count)
                    .unwrap_or(0);
                let group = object.group;
                let expanded = !self.collapsed.contains(&group);
                let entity_for_group = entity.clone();
                // 段头用共享 `tree::GroupHeader`：与草稿箱的分区头（草稿 / 引用 / 书签 / 回收站）、
                // 数据导航的分组头是同一个原语——整行可点、计数紧跟组名、凹槽底色与原语统一。
                // 手搓版（Button 箭头 + 计数顶到右端 + 无底色）已撤：三段时间距 / 高度各写一套，
                // 与兄弟面板对不齐。
                // 外层 `pl_1p5` 把原语自带的 6px 内距补到面板的 12px 一线（铁律第 41 条：
                // 工具栏 / 行 / 状态栏同一条左缘），底色同 `group_box` 让凹槽从面板左缘铺起。
                list = list.child(
                    div().pl_1p5().bg(colors.group_box).child(
                        tree::GroupHeader::new(
                            SharedString::from(format!(
                                "data-objects-group-{}",
                                group.title()
                            )),
                            group.title(),
                        )
                        .debug_selector("data-objects-group-head")
                        .expanded(expanded)
                        .count(group_count, colors.muted_foreground)
                        // 可点的行必须给 hover 反馈（导航分组头同一口径）；草稿箱那几处是
                        // 常驻分区头、不传，这里属于「点了会折叠」的一类。
                        .hover_background(colors.list_hover)
                        .on_click(move |_, _, cx| {
                            let group = group;
                            entity_for_group.update(cx, |tab, cx| {
                                tab.toggle_group(group, cx);
                            });
                        }),
                    ),
                );
            }
            // 收起只是不画行：段头和计数留着，折叠起来也知道里面有几行
            if self.collapsed.contains(&object.group) {
                continue;
            }
            // 不能分析的行还是得被看见（常常只是缺扩展），给个 warn 标而不是直接藏起来
            let pill = if object.can_analyze() {
                Tag::secondary().rounded_full().flex_none()
            } else {
                Tag::warning().rounded_full().flex_none()
            };

            // 「怎么读」「还在不在」是选源时仅有的两个判断依据，放同一行右侧
            let state_color = if object.can_analyze() {
                colors.muted_foreground
            } else {
                colors.danger
            };
            let is_picked = picked_set.contains(object.key.as_str());
            // 只有能用的对象能进工作集：失效行点进去只会在分析时炸；
            // 连接行（`conn:`）也不行——它是表的位置，不是数据对象（见 `Object::can_pick`）
            let pick_button = if object.can_pick() {
                let key = object.key.clone();
                let entity = entity.clone();
                Some(
                    Button::new(SharedString::from(format!(
                        "data-object-pick-{}",
                        object.key
                    )))
                    .ghost()
                    .toggled(is_picked)
                    .label(if is_picked { "已选" } else { "选用" })
                    .on_click(move |_, _, cx| {
                        let key = key.clone();
                        entity.update(cx, |tab, cx| tab.toggle_picked(&key, cx));
                    }),
                )
            } else {
                None
            };
            // 源行行尾的**唯一**动作位（铁律第 42 条）：有 ATTACH 能力且已登记的连接
            // 给「挂载 / 卸载」，等于启用 / 关闭它在联邦库里的 catalog。无能力的行
            // （`source_action == None`）行尾为空，叶子更是零按钮——属性 / 改名 / 登记
            // / 移除登记全部走右键，这里不另摆按钮
            let mount_button = match &object.source_action {
                Some(data_objects::SourceAction::ToggleMount {
                    conn_id,
                    path,
                    mounted,
                }) => {
                    let shared = self.shared.clone();
                    let conn_id = conn_id.clone();
                    let path = path.clone();
                    let mounted = *mounted;
                    Some(
                        Button::new(SharedString::from(format!(
                            "data-object-mount-{}",
                            object.key
                        )))
                        .ghost()
                        .label(if mounted { tr("卸载") } else { tr("挂载") })
                        .on_click(move |_, window, cx| match (&path, mounted) {
                            (Some(path), true) => {
                                crate::components::data_objects_load::unmount_file_database(
                                    &shared, path, cx,
                                );
                                window.refresh();
                            }
                            (Some(path), false) => {
                                crate::components::data_objects_load::mount_file_database(
                                    &shared, path, cx,
                                );
                                window.refresh();
                            }
                            (None, true) => {
                                crate::components::data_objects_load::unmount_network_source(
                                    &shared, &conn_id, cx,
                                );
                                window.refresh();
                            }
                            (None, false) => {
                                crate::components::data_objects_load::mount_network_source(
                                    &shared, &conn_id, cx,
                                );
                                window.refresh();
                            }
                        }),
                    )
                }
                None => None,
            };
            // 会话行的动作（重挂 / 设主源 / 关会话）后端已有，界面不发明。
            // 原型是点开该行下方直接出动作条：比下拉菜单少一次移动，也看得见都有什么。
            // 判据由 `session_actions` 自己按 key 前缀决定（非会话键返回空），
            // 不再靠段判断——会话源和源库连接同段了
            let session_actions = data_objects::session_actions(&object.key, &object.state);
            let session_open = self.expanded_session.as_deref() == Some(object.key.as_str());
            let session_button = if session_actions.is_empty() {
                None
            } else {
                let key = object.key.clone();
                let entity = entity.clone();
                Some(
                    Button::new(SharedString::from(format!(
                        "data-object-session-{}",
                        object.key
                    )))
                    .ghost()
                    .label(if session_open { "收起" } else { "会话" })
                    .on_click(move |_, _, cx| {
                        let key = key.clone();
                        entity.update(cx, |tab, cx| {
                            tab.expanded_session =
                                if tab.expanded_session.as_deref() == Some(key.as_str()) {
                                    None
                                } else {
                                    Some(key)
                                };
                            cx.notify();
                        });
                    }),
                )
            };
            // 铁律 C 组第 9/10 条：未登记的表（与资产同段）可就地补身份（只写登记行，
            // 不搬数据、不涨版本）；「升级为资产」是另一件事（写表 + 登记）。两条都进右键，
            // 行上不再摆按钮。判据就是 key 前缀：段只说明是表，不说明有没有身份
            let register_target = object
                .key
                .strip_prefix("persistent:")
                .or_else(|| object.key.strip_prefix("project:"))
                .map(str::to_string);
            let persist_target = data_objects::can_persist(object).then(|| {
                (
                    object.name.clone(),
                    data_objects::persist_label(&object.name),
                )
            });
            // 单行的「清理这张表」：结果集 / 载入 都是会话级占用（`can_clear`）。
            // 工具栏那颗是「全清」——为删一张副本把别的结果集一起清掉，是这把按钮的粗口
            let clear_target = data_objects::can_clear(object).then(|| object.name.clone());
            // 缺扩展的行：与其只标个警告，给一条能走的路（跳插件管理）。
            // 只给**会话源**（联邦挂载失败）——源库连接没连上不是缺扩展，给「装扩展」是误导
            let install_button = if object.can_analyze() || !object.key.starts_with("session:") {
                None
            } else {
                let shared = self.shared.clone();
                let entity = entity.clone();
                Some(
                    Button::new(SharedString::from(format!(
                        "data-object-install-{}",
                        object.key
                    )))
                    .ghost()
                    .label(tr("装扩展"))
                    .on_click(move |_, window, cx| {
                        if !shared.show_plugin_manager(window, cx) {
                            entity.update(cx, |tab, cx| {
                                tab.note =
                                    Some(PanelNote::error("插件管理面板没挂上，这里装不了扩展"));
                                cx.notify();
                            });
                        }
                    }),
                )
            };
            // 源行可展开（联邦源 / 加速源 / 文件库 / 源库连接）：行首给展开指示，
            // 与段头 chevron 同一习惯。**没挂载的源不给展开**（铁律 B 组第 4 条）：
            // 没有表可列，给了 chevron 只会点出一个空列表。失败的联邦源同理。
            // 例外：登记过、挂不上、连接还活着的行（`expand_via`）——展开不是"列自家
            // 挂载的表"，而是去那条活连接上选表，所以它没有 can_analyze() 也照样给指示。
            let in_source_group = object.group == Group::Source;
            let source_expandable = in_source_group
                && (object.expand_via.is_some()
                    || (object.can_analyze()
                        && data_objects::source_ref(&object.key).is_some()));
            let expanded_now = self.expanded_sources.contains(&object.key);
            let disclosure = if in_source_group {
                Some(if source_expandable {
                    let key = object.key.clone();
                    let entity = entity.clone();
                    let expand_via = object.expand_via.clone();
                    Button::new(SharedString::from(format!(
                        "data-object-expand-{}",
                        object.key
                    )))
                    .ghost()
                    .xsmall()
                    .icon(if expanded_now {
                        gpui_kit::assets::IconName::ChevronDown
                    } else {
                        gpui_kit::assets::IconName::ChevronRight
                    })
                    .on_click(move |_, window, cx| {
                        let key = key.clone();
                        let expand_via = expand_via.clone();
                        entity.update(cx, |tab, cx| {
                            tab.toggle_source_expand(&key, expand_via, window, cx)
                        });
                    })
                    .into_any_element()
                } else {
                    tree::disclosure_slot().into_any_element()
                })
            } else {
                None
            };
            // 走马灯的两个实测数：这一行名字格多宽（上一帧 prepaint 量到的）、
            // 名字按同字体同字号排出来多宽（只在鼠标停在这行时才量，别为整张清单做 shaping）
            let name_space = name_widths.read(cx).get(&object.key).unwrap_or(0.0);
            let row_hovered = hovered_row.read(cx).0.as_deref() == Some(object.key.as_str());
            let name_text_width = if row_hovered {
                text_width(window, &object.name, window.rem_size() * 0.875)
            } else {
                0.0
            };
            let marquee =
                row_hovered && data_objects::layout::should_marquee(name_text_width, name_space);

            let name_cell = {
                let cell = div()
                    // 坐标锚点：窗口用例靠它量「名字这一格实际多宽」
                    // （`.id()` 只登记可点，不登记几何）
                    .debug_selector({
                        let key = object.key.clone();
                        move || format!("data-object-name-{key}")
                    })
                    .text_sm()
                    .text_color(colors.foreground)
                    // 窄面板里名字一长就顶出去，行尾按钮会被挤掉
                    .overflow_hidden();
                if marquee {
                    // 放不下：hover 时改走马灯。文字按**自然宽**排（`flex_none` 不许压缩），
                    // 外层照旧裁切，动画只是把裁切窗口顺着字往后挪——不是把字拉细。
                    let travel = (name_text_width - name_space).max(0.0);
                    cell.h_flex().child(
                        div()
                            .flex_none()
                            .debug_selector({
                                let key = object.key.clone();
                                move || format!("data-object-marquee-{key}")
                            })
                            .child(object.name.clone())
                            .with_animation(
                                SharedString::from(format!("data-objects-marquee-{}", object.key)),
                                Animation::new(std::time::Duration::from_millis(
                                    data_objects::layout::MARQUEE_PERIOD_MS,
                                ))
                                .repeat(),
                                move |el, delta| {
                                    el.left(px(data_objects::layout::marquee_offset(travel, delta)))
                                },
                            ),
                    )
                } else {
                    cell.text_ellipsis().child(object.name.clone())
                }
            };

            list = list.child(
                div()
                    // 每一行都要能被 hover 与键盘看清：hover 需要命中的 id，不是样式
                    .id(SharedString::from(format!("data-object-row-{}", object.key)))
                    .h_flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .hover(|s| s.bg(colors.list_hover))
                    // 走马灯只在鼠标停住的那一行跑：常驻滚动会让整张清单每帧重绘
                    .on_hover({
                        let hovered_slot = hovered_row.clone();
                        let entity_for_hover = entity.clone();
                        let key = object.key.clone();
                        move |&hovering, _window, cx| {
                            let changed = hovered_slot.update(cx, |slot, _| {
                                let next = hovering.then(|| key.clone());
                                if slot.0 == next {
                                    false
                                } else {
                                    slot.0 = next;
                                    true
                                }
                            });
                            if changed {
                                entity_for_hover.update(cx, |_, cx| cx.notify());
                            }
                        }
                    })
                    .border_b_1()
                    .border_color(colors.border)
                    .relative()
                    // 坐标锚点：整行外框（用例据此判断内容有没有被顶出面板）
                    .debug_selector({
                        let key = object.key.clone();
                        move || format!("data-object-row-{key}")
                    })
                    // 已选的行给一道左侧色条：走共用原语，别再用 border 挤掉 2px 内距
                    .when(is_picked, |el| el.child(workbench_shell::tree::active_bar(colors.primary)))
                    // 失效行压暗：仍然看得见，但别让人以为能直接点
                    .opacity(if object.can_analyze() { 1.0 } else { 0.55 })
                    // 源段的展开指示（可展开 = chevron 钮；不可展开 = 同宽空位）
                    .children(disclosure)
                    // 五段图标各不同，靠它扫一眼分形状（原型 07 的图标列）
                    .child(
                        div().flex_none().child(
                            Icon::new(object.icon())
                                .xsmall()
                                .text_color(if object.can_analyze() {
                                    colors.muted_foreground
                                } else {
                                    colors.danger
                                }),
                        ),
                    )
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            // 名字格的真实宽：prepaint 量到就写回 keyed state，供下一帧判走马灯。
                            // 逐行量而不是按面板宽推算：行尾按钮数随行的可点性变化（源行没有勾选钮）
                            .on_children_prepainted({
                                let widths = name_widths.clone();
                                let key = object.key.clone();
                                move |bounds, _window, cx| {
                                    let Some(first) = bounds.first() else {
                                        return;
                                    };
                                    let width: f32 = first.size.width.into();
                                    widths.update(cx, |map, _| {
                                        map.remember(key.clone(), width);
                                    });
                                }
                            })
                            .child(name_cell)
                            .child(
                                div()
                                    .debug_selector({
                                        let key = object.key.clone();
                                        move || format!("data-object-note-{key}")
                                    })
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(object.note.clone()),
                            ),
                    )
                    // 业务身份走标准 Tag：手写胶囊的圆角、内距、字号各写一遍就会漂
                    // 极窄档才丢小标（它是身份判据，见 `data_objects::layout`）
                    .when(tier.shows_pill(), |el| {
                        el.child(
                            div()
                                .debug_selector({
                                    let key = object.key.clone();
                                    move || format!("data-object-pill-{key}")
                                })
                                .child(pill.child(object.pill_text())),
                        )
                    })
                    .when(tier.shows_channel(), |el| {
                        el.child(
                            div()
                                .debug_selector({
                                    let key = object.key.clone();
                                    move || format!("data-object-channel-{key}")
                                })
                                .text_xs()
                                .flex_none()
                                // 固定一档最小宽：`→ 联邦` 与 `→ 进程 DuckDB` 差太多，
                                // 不钉住的话每行的按钮位置都在跳。
                                // 文案直接用 `channel.label()`（自带箭头）——这里再拼一层
                                // 会画出「→ → 联邦」（修掉的旧缺陷）
                                .min_w_16()
                                .text_color(colors.muted_foreground)
                                .child(object.channel.label()),
                        )
                    })
                    .child(
                        div()
                            .debug_selector({
                                let key = object.key.clone();
                                move || format!("data-object-state-{key}")
                            })
                            .text_xs()
                            .flex_none()
                            .min_w_10()
                            // 行上只放短码：全文（`availability_text`）30 字实测 209px，
                            // 窄面板里会把这一格顶到行外、连带挤掉行尾按钮。上限钉住（6rem = 96px，
                            // 够 6 字短码），再短也有 ellipsis 兜底（属性窗口里看全文，见铁律第 38 条）
                            .max_w(rems(6.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(state_color)
                            // 能用 → 寿命（会不会自己消失）；不能用 → 短原因（要动手解决什么）
                            .child(object.availability_chip()),
                    )
                    .children(pick_button)
                    .children(mount_button)
                    .children(install_button)
                    .children(session_button)
                    // 铁律 E 组第 20 条：与其他模块的交互一律走右键。
                    // `context_menu` 会换掉元素类型（不再有样式方法），所以必须挂在链尾。
                    .context_menu({
                        use crate::components::file_actions::{
                            archive_file_as_snapshot, bare_table_name,
                        };
                        let shared = self.shared.clone();
                        let sql_ref = object.sql_ref.clone();
                        // 只有「项目分析库里的表」能当 Mock 的结构来源：临时结果集只活在内存，
                        // 全局库是另一个文件（都不在这条读列的路上，见 `open_mock_from_analytics`）
                        let in_project_analytics = matches!(
                            object.key.split(':').next(),
                            Some("persistent" | "project" | "asset")
                        );
                        let label = object.name.clone();
                        let alias = data_objects::fileview_alias(&object.key).map(str::to_string);
                        let usable = object.can_analyze();
                        let register_target = register_target.clone();
                        let persist_target = persist_target.clone();
                        let clear_target = clear_target.clone();
                        // 单行清理要在确认框的回调里更新面板：把实体句柄带进菜单闭包
                        let entity_menu = entity.clone();
                        // 属性窗口要整行快照：必须先克隆到闭包外。`object` 是借来的引用，
                        // 在菜单闭包里 `object.clone()` 只会克隆那个引用，闭包就不满足 'static
                        let props_object = (*object).clone();
                        // 文件库源「挂载」：路径也先取出来（顺手带上它，菜单才知道挂哪个文件）
                        let unmounted_path = self
                            .unmounted_sources
                            .iter()
                            .find(|(key, _)| key == &object.key)
                            .map(|(_, path)| path.clone());
                        // 已挂上的那几行：卸载 / 重挂用同一条路径（判据与「未挂」互斥）
                        let mounted_path = self
                            .mounted_sources
                            .iter()
                            .find(|(key, _)| key == &object.key)
                            .map(|(_, path)| path.clone());
                        // 网络连接那条线：挂载三态的两半（登记 = 意图，加速会话 = 事实）。
                        // 连接行与「只剩登记」的 reg 行都要能在右键里补齐另一半
                        let conn_id = object.key.strip_prefix("conn:").map(str::to_string);
                        let conn_mounted = conn_id
                            .as_ref()
                            .map(|id| self.mounted_network.contains(id))
                            .unwrap_or(false);
                        let registered = conn_id.as_ref().and_then(|id| {
                            self.registrations
                                .iter()
                                .find(|row| row.connection_id.as_deref() == Some(id.as_str()))
                                .map(|row| row.id.clone())
                        });
                        let reg_row = object
                            .key
                            .strip_prefix("reg:")
                            .and_then(|id| {
                                self.registrations
                                    .iter()
                                    .find(|row| row.id == id)
                                    .cloned()
                            });
                        let conn_name = object.name.clone();
                        move |menu, _window, _cx| {
                            let mut menu = menu;
                            if let Some(reference) = sql_ref.clone().filter(|_| usable) {
                                let table = bare_table_name(&reference);
                                let open = shared.clone();
                                let open_ref = reference.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(tr("在编辑器打开")).on_click(move |_, _, cx| {
                                        open.request_query(workbench_shell::model::QueryRequest {
                                            conn_id: None,
                                            sql: format!("SELECT * FROM {open_ref}"),
                                            run: false,
                                        });
                                        open.notify_host(cx);
                                    }),
                                );
                                // 洞察：看**数据**（取样 → 表探查），与结果集右键同一出口
                                let insight = shared.clone();
                                let insight_table = table.clone();
                                let insight_label = label.clone();
                                menu = menu.item(PopupMenuItem::new(tr("洞察（看数据）")).on_click(
                                    move |_, _, cx| {
                                        crate::components::result_set_actions::open_insight_table(
                                            &insight,
                                            &insight_table,
                                            &insight_label,
                                            cx,
                                        );
                                    },
                                ));
                                // Mock：只出**结构**（铁律 E 组），不预览、不抽行
                                if in_project_analytics {
                                    let mock = shared.clone();
                                    let mock_table = table;
                                    menu = menu.item(PopupMenuItem::new(tr("作为 Mock 结构来源")).on_click(
                                        move |_, _, cx| {
                                            crate::components::result_set_actions::open_mock_from_analytics(
                                                &mock, &mock_table, cx,
                                            );
                                        },
                                    ));
                                }
                                // 导出 / 存草稿：同一张窗口，两个入口各带一档目的地
                                let export = shared.clone();
                                let export_ref = reference.clone();
                                let export_label = label.clone();
                                menu = menu.item(PopupMenuItem::new("导出…").on_click(
                                    move |_, window, cx| {
                                        crate::components::export_dialog::open_export_dialog(
                                            export.clone(),
                                            export_label.clone(),
                                            export_ref.clone(),
                                            crate::components::export_dialog::ExportKind::Global,
                                            window,
                                            cx,
                                        );
                                    },
                                ));
                                let draft = shared.clone();
                                let draft_ref = reference;
                                let draft_label = label.clone();
                                menu = menu.item(PopupMenuItem::new("存到草稿…").on_click(
                                    move |_, window, cx| {
                                        crate::components::export_dialog::open_export_dialog(
                                            draft.clone(),
                                            draft_label.clone(),
                                            draft_ref.clone(),
                                            crate::components::export_dialog::ExportKind::Scratchpad,
                                            window,
                                            cx,
                                        );
                                    },
                                ));
                            }
                            // 未登记段的表：就地补身份（只写登记，不搬数据、不涨版本）
                            if let Some(table) = register_target.clone() {
                                let register = shared.clone();
                                let register_label = label.clone();
                                menu = menu.item(PopupMenuItem::new(tr("登记为资产")).on_click(
                                    move |_, _, cx| {
                                        crate::components::result_set_actions::register_table_asset(
                                            &register,
                                            &table,
                                            &register_label,
                                            cx,
                                        );
                                    },
                                ));
                            }
                            // 临时表 / 结果集：写表 + 登记（写库走 worker，回执由工作台泵）
                            if let Some((table, table_label)) = persist_target.clone() {
                                let persist = shared.clone();
                                menu = menu.item(PopupMenuItem::new(tr("升级为资产")).on_click(
                                    move |_, _, cx| {
                                        crate::components::result_set_actions::persist_to_project(
                                            &persist,
                                            &table,
                                            &table_label,
                                            cx,
                                        );
                                    },
                                ));
                            }
                            // 单行清理（结果集 / 载入）：只删这一张，不牵连同段别的临时表。
                            // 破坏性动作照旧过确认框，文案说清「删了要重跑 / 重新载入」。
                            if let Some(table) = clear_target.clone() {
                                let entity = entity_menu.clone();
                                menu = menu.item(PopupMenuItem::new(tr("清理这张表…")).on_click(
                                    move |_, window, cx| {
                                        let entity = entity.clone();
                                        let table = table.clone();
                                        window.open_alert_dialog(cx, move |alert, _window, _cx| {
                                            let entity = entity.clone();
                                            let table = table.clone();
                                            alert
                                                .confirm()
                                                .title(tr("清理这张表"))
                                                .description(format!(
                                                    "将从进程内存里删掉 {table}：删了要重跑 / 重新载入才有。想留就先「升级为资产」或「归档到分析库」。确定继续？"
                                                ))
                                                .on_ok(move |_window, _cx, cx| {
                                                    entity.update(cx, |tab, cx| {
                                                        tab.clear_one(&table, cx);
                                                    });
                                                    true
                                                })
                                        });
                                    },
                                ));
                            }
                            if let Some(alias) = alias.clone() {
                                let snapshot = shared.clone();
                                menu = menu.item(PopupMenuItem::new("存为快照").on_click(
                                    move |_, _, cx| {
                                        let path = snapshot
                                            .loaded_files
                                            .borrow()
                                            .iter()
                                            .find(|(seen, _)| seen == &alias)
                                            .map(|(_, path)| path.clone());
                                        match path {
                                            Some(path) => {
                                                archive_file_as_snapshot(&snapshot, &path, cx)
                                            }
                                            None => snapshot.say_error(
                                                format!("存快照：找不到 {alias} 的路径"),
                                                cx,
                                            ),
                                        }
                                    },
                                ));
                            }
                            // 文件库源还没挂：挂载是这一行唯一的出口（铁律 B 组第 4 条）
                            if let Some(path) = unmounted_path.clone() {
                                let mount = shared.clone();
                                menu = menu.item(PopupMenuItem::new("挂载").on_click(
                                    move |_, window, cx| {
                                        crate::components::data_objects_load::mount_file_database(
                                            &mount, &path, cx,
                                        );
                                        // 挂完清单会重排（conn 行的失效态消失、源段多出文件库行）：
                                        // 请宿主重绘，别等下一次事件
                                        window.refresh();
                                    },
                                ));
                            }
                            // 已挂上的文件库：卸载 / 重挂（重挂 = 同一份描述再挂一次，表清单可能变）
                            if let Some(path) = mounted_path.clone() {
                                let unmount = shared.clone();
                                let unmount_path = path.clone();
                                menu = menu.item(PopupMenuItem::new("卸载").on_click(
                                    move |_, window, cx| {
                                        crate::components::data_objects_load::unmount_file_database(
                                            &unmount,
                                            &unmount_path,
                                            cx,
                                        );
                                        window.refresh();
                                    },
                                ));
                                let remount = shared.clone();
                                menu = menu.item(PopupMenuItem::new("重新挂载").on_click(
                                    move |_, window, cx| {
                                        crate::components::data_objects_load::mount_file_database(
                                            &remount, &path, cx,
                                        );
                                        window.refresh();
                                    },
                                ));
                            }

                            // 网络连接线（规格 §8 补的缺口）：挂载 / 卸载问的是**事实**，
                            // 登记 / 移除登记问的是**意图**，两件事分两条菜单项，别合成一步
                            if let Some(id) = conn_id.clone() {
                                if conn_mounted {
                                    let unmount = shared.clone();
                                    let cid = id.clone();
                                    menu = menu.item(PopupMenuItem::new("卸载").on_click(
                                        move |_, window, cx| {
                                            crate::components::data_objects_load::unmount_network_source(
                                                &unmount, &cid, cx,
                                            );
                                            window.refresh();
                                        },
                                    ));
                                } else {
                                    let mount = shared.clone();
                                    let cid = id.clone();
                                    menu = menu.item(PopupMenuItem::new("挂载").on_click(
                                        move |_, window, cx| {
                                            crate::components::data_objects_load::mount_network_source(
                                                &mount, &cid, cx,
                                            );
                                            window.refresh();
                                        },
                                    ));
                                }
                                match registered.clone() {
                                    // 没登记：把「会话级」的那半补成项目级
                                    None => {
                                        let reg = shared.clone();
                                        let cid = id.clone();
                                        let name = conn_name.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(tr("登记为项目源")).on_click(
                                                move |_, window, cx| {
                                                    crate::components::data_objects_load::register_connection_source(
                                                        &reg, &cid, &name, cx,
                                                    );
                                                    window.refresh();
                                                },
                                            ),
                                        );
                                    }
                                    // 已登记：移除意图（挂着的会话不动）
                                    Some(rid) => {
                                        let del = shared.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(tr("移除登记")).on_click(
                                                move |_, window, cx| {
                                                    crate::components::data_objects_load::unregister_source(
                                                        &del, &rid, cx,
                                                    );
                                                    window.refresh();
                                                },
                                            ),
                                        );
                                    }
                                }
                            }

                            // 「只剩登记」的行（`reg:`）：连接/文件都不在清单里，
                            // 右键只给能救回来的两件——按登记的本体挂回去，或直接删登记
                            if let Some(row) = reg_row.clone() {
                                match (row.connection_id.clone(), row.path.clone()) {
                                    (Some(cid), _) => {
                                        let mount = shared.clone();
                                        menu = menu.item(PopupMenuItem::new("挂载").on_click(
                                            move |_, window, cx| {
                                                crate::components::data_objects_load::mount_network_source(
                                                    &mount, &cid, cx,
                                                );
                                                window.refresh();
                                            },
                                        ));
                                    }
                                    (None, Some(path)) => {
                                        let mount = shared.clone();
                                        menu = menu.item(PopupMenuItem::new("挂载").on_click(
                                            move |_, window, cx| {
                                                crate::components::data_objects_load::mount_file_database(
                                                    &mount, &path, cx,
                                                );
                                                window.refresh();
                                            },
                                        ));
                                    }
                                    (None, None) => {}
                                }
                                let del = shared.clone();
                                let rid = row.id.clone();
                                menu = menu.item(PopupMenuItem::new(tr("移除登记")).on_click(
                                    move |_, window, cx| {
                                        crate::components::data_objects_load::unregister_source(
                                            &del, &rid, cx,
                                        );
                                        window.refresh();
                                    },
                                ));
                            }
                            // 属性：这一行到底是哪个东西（身份键 / 通道 / 寿命 / 列结构）
                            // 与「复制引用名」一起放在菜单尾部：都是"看/抄"，不是"动数据"
                            let props_row = props_object.clone();
                            // 资产行：版本历史（还原 / 取回 / 「用 vN 分析」都在那个对话框里；
                            // 历史版本本身**不进清单平铺**——铁律 D17 的二级入口就是这个门）
                            if let Some(resource_id) = props_object
                                .key
                                .strip_prefix("asset:")
                                .map(str::to_string)
                            {
                                let versions = shared.clone();
                                menu = menu.item(PopupMenuItem::new("版本…").on_click(
                                    move |_, _, cx| {
                                        let panel = versions
                                            .sidebar_panel
                                            .borrow()
                                            .as_ref()
                                            .and_then(|weak| weak.upgrade());
                                        let Some(panel) = panel else {
                                            versions.say_error(
                                                "资产库面板还没建好，稍后再试".to_string(),
                                                cx,
                                            );
                                            return;
                                        };
                                        let id = resource_id.clone();
                                        panel.update(cx, |panel, cx| {
                                            panel.request_repair_action(
                                                analytics_resource::dialogs::index_repair::RepairAction::OpenVersions {
                                                    resource_id: id,
                                                },
                                                cx,
                                            );
                                        });
                                    },
                                ));
                            }
                            let props = shared.clone();
                            menu = menu.item(PopupMenuItem::new("属性…").on_click(
                                move |_, window, cx| {
                                    crate::components::object_props::open_object_props(
                                        props.clone(),
                                        props_row.clone(),
                                        window,
                                        cx,
                                    );
                                },
                            ));
                            // 引用名只认 `sql_ref`（源库表 / 文件库行本来就没有可抄的名字）
                            if let Some(reference) = sql_ref.clone().filter(|r| !r.trim().is_empty()) {
                                let copy = shared.clone();
                                menu = menu.item(PopupMenuItem::new("复制引用名").on_click(
                                    move |_, _window, cx| {
                                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                            reference.clone(),
                                        ));
                                        copy.say(format!("已复制引用名：{reference}"), cx);
                                    },
                                ));
                            }
                            // 空菜单右键会像「没反应」：把可用性原因说出来，别让人以为界面坏了
                            if menu.is_empty() {
                                menu = menu.item(PopupMenuItem::new(
                                    "没有可执行的交互（原因见这一行的状态列）",
                                ));
                            }
                            menu
                        }
                    }),
            );
            if session_open {
                let mut bar = div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .pb_2()
                    .border_b_1()
                    .border_color(colors.border);
                for action in &session_actions {
                    let action = action.clone();
                    let label = action.label();
                    let entity = entity.clone();
                    bar = bar.child(
                        Button::new(SharedString::from(format!(
                            "data-object-session-action-{}-{}",
                            object.key, label
                        )))
                        .ghost()
                        .label(label)
                        .on_click(move |_, _, cx| {
                            let action = action.clone();
                            entity.update(cx, |tab, cx| {
                                tab.expanded_session = None;
                                tab.run_session_action(action, cx);
                                cx.notify();
                            });
                        }),
                    );
                }
                list = list.child(bar);
            }
            // 展开着的源行：把叶子（表 / 视图）画在它下面。清单没到（刷新竞态 /
            // 列举失败）就不画——下一次事件会补上，宁可少一拍也不画猜的
            if expanded_now {
                if matches!(
                    data_objects::source_ref(&object.key),
                    Some(data_objects::SourceRef::Connection { .. })
                ) {
                    // 源库表：叶子是**可选用**的对象（取样走连接下推，不搬数据）
                    match self.conn_children.get(&object.key) {
                        Some(children) if !children.is_empty() => {
                            // 一次只画一页（见 `LEAF_PAGE_SIZE`）：上千张表的连接全量铺开，
                            // 每帧要建上千个元素，滚动掉帧
                            let shown = leaf_shown(
                                children.len(),
                                self.leaf_pages.get(&object.key).copied(),
                            );
                            // 三级树第二层 = catalog + schema：只画一级会把 `orders.main`
                            // 与 `bi.main` 的同名表挤成两行一样的东西（第 37 条）
                            for (layer, indices) in
                                data_objects::conn_schema_groups(&children[..shown])
                            {
                                let schema_key = format!("{}|{layer}", object.key);
                                let open = !self.collapsed_schemas.contains(&schema_key);
                                list = list.child(source_schema_row(
                                    &schema_key,
                                    &layer,
                                    indices.len(),
                                    open,
                                    &entity,
                                    cx,
                                ));
                                if !open {
                                    continue;
                                }
                                for index in indices {
                                    let leaf = &children[index];
                                    let leaf_picked = picked_set.contains(leaf.key.as_str());
                                    list = list.child(source_table_leaf_row(
                                        leaf,
                                        leaf_picked,
                                        &entity,
                                        &self.shared,
                                        cx,
                                    ));
                                }
                            }
                            if shown < children.len() {
                                list = list.child(leaf_more_row(
                                    &object.key,
                                    children.len() - shown,
                                    &entity,
                                    cx,
                                ));
                            }
                        }
                        Some(_) => {
                            list = list.child(
                                div()
                                    .id(SharedString::from(format!(
                                        "data-object-leaf-empty-{}",
                                        object.key
                                    )))
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_1()
                                    .child(tree::indent_spacer(ui::TREE_INDENT, 1))
                                    .child(tree::disclosure_slot())
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(tr(
                                            "这个连接里没有可列的表 / 视图（目录为空或没有权限）",
                                        )),
                                    ),
                            );
                        }
                        // 后台列举还没回执：说一声在查（远端库要等网络往返）
                        None => {
                            if self.loading_sources.contains(&object.key) {
                                list = list.child(leaf_notice_row(
                                    &format!("data-object-leaf-loading-{}", object.key),
                                    "正在列举这个连接里的表…",
                                    colors.muted_foreground,
                                ));
                            }
                        }
                    }
                } else {
                    match self.source_children.get(&object.key) {
                        Some(children) if !children.is_empty() => {
                            // 同上：一页一页画，别把整个源的表一次铺进容器
                            let shown = leaf_shown(
                                children.len(),
                                self.leaf_pages.get(&object.key).copied(),
                            );
                            // 「载入内存…」只对挂在分析实例上的源开（见 `source_leaf_row`）
                            let leaf_source = data_objects::source_ref(&object.key);
                            // 第二层 = schema：叶子按 schema 切段，点击组头收起
                            for (layer, indices) in data_objects::schema_groups(&children[..shown])
                            {
                                let schema_key = format!("{}|{layer}", object.key);
                                let open = !self.collapsed_schemas.contains(&schema_key);
                                list = list.child(source_schema_row(
                                    &schema_key,
                                    &layer,
                                    indices.len(),
                                    open,
                                    &entity,
                                    cx,
                                ));
                                if !open {
                                    continue;
                                }
                                for index in indices {
                                    let table = &children[index];
                                    list = list.child(source_leaf_row(
                                        &object.key,
                                        table,
                                        &entity,
                                        &self.shared,
                                        leaf_source.as_ref(),
                                        &object.name,
                                        cx,
                                    ));
                                }
                            }
                            if shown < children.len() {
                                list = list.child(leaf_more_row(
                                    &object.key,
                                    children.len() - shown,
                                    &entity,
                                    cx,
                                ));
                            }
                        }
                        Some(_) => {
                            list = list.child(
                                div()
                                    .id(SharedString::from(format!(
                                        "data-object-leaf-empty-{}",
                                        object.key
                                    )))
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_1()
                                    .child(tree::indent_spacer(ui::TREE_INDENT, 1))
                                    .child(tree::disclosure_slot())
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(tr("没有可列的表（挂载成功但目录为空）")),
                                    ),
                            );
                        }
                        // 后台列举还没回执：说一声在查
                        None => {
                            if self.loading_sources.contains(&object.key) {
                                list = list.child(leaf_notice_row(
                                    &format!("data-object-leaf-loading-{}", object.key),
                                    "正在列举这个源里的表…",
                                    colors.muted_foreground,
                                ));
                            }
                        }
                    }
                }
            }
        }

        if objects.is_empty() {
            // 空态分两种：清单本来就空（该去建源），还是筛过之后空（该去改条件）。
            // 两种空法给的下一步完全不同，混成一句会让人以为清单被清空了。
            list = list.child(if total == 0 {
                Empty::new().header(
                    EmptyHeader::new()
                        .media(EmptyMedia::new())
                        .title(EmptyTitle::new().child(tr("还没有可用的数据对象")))
                        .description(EmptyDescription::new().child(
                            "先连一个数据源，或跑一次查询；能被 DuckDB 加载的文件也可以直接放进来。",
                        )),
                )
                // 空态给一条能直接走的路：说了“可以放文件”，就得有个按钮去放
                .content(EmptyContent::new().child({
                    let shared = self.shared.clone();
                    Button::new("data-objects-empty-load")
                        .label(tr("加载文件到分析"))
                        .tooltip(tr("按需加载：挂成视图读原文件，切项目即清"))
                        .on_click(move |_, window, cx| {
                            crate::components::data_objects_load::open_load_files_dialog(
                                &shared,
                                crate::components::data_objects_load::LoadScope::Session,
                                window,
                                cx,
                            );
                        })
                }))
            } else {
                Empty::new()
                    .header(
                        EmptyHeader::new()
                            .media(EmptyMedia::new())
                            .title(EmptyTitle::new().child(tr("这个条件下没有对象")))
                            .description(EmptyDescription::new().child(
                                "换个名字，或点「全部」看完整清单。",
                            )),
                    )
                    // 说“点「全部」”，就给一个点了真能回全部的地方：条件有两处（段与搜索词）
                    .content(EmptyContent::new().child({
                        let entity = entity.clone();
                        let search = search.clone();
                        Button::new("data-objects-empty-reset")
                            .ghost()
                            .label(tr("清条件看全部"))
                            .on_click(move |_, window, cx| {
                                search.update(cx, |state, cx| {
                                    state.set_value("", window, cx);
                                });
                                entity.update(cx, |tab, cx| {
                                    tab.filter = None;
                                    cx.notify();
                                });
                            })
                    }))
            });
        }

        // 宽度探针：这层只有一个子（滚动容器），`bounds[0]` 就是面板可用宽。
        // 量到就写进 keyed state 并请求重绘一次——下一帧行布局才能按真实宽度判档；
        // 相等时不 notify，避免自激成帧循环。
        let width_probe = row_width.clone();
        let entity_for_width = entity.clone();
        let list_frame = div()
            .flex_1()
            .min_h_0()
            .on_children_prepainted(move |bounds, _window, cx| {
                let Some(first) = bounds.first() else {
                    return;
                };
                let width: f32 = first.size.width.into();
                if (width_probe.read(cx).get() - width).abs() > 0.5 {
                    width_probe.update(cx, |slot, _| slot.set(width));
                    entity_for_width.update(cx, |_, cx| cx.notify());
                }
            })
            .child(list);

        // 状态栏（24px，与资产库面板同一档）：左边是状态口径，右边是「刷新」。
        //
        // 「几个对象 / 已选几个 / 占多少内存」是**状态**，不是工具栏内容：它挤在面板头部
        // 会占掉动作按钮的位置，也让每块面板的头部长得不一样。面板会变（连上/断开连接、
        // 临时表失效、资产改了版本），所以刷新钮跟状态放一起——状态旁边才是最想重取的地方。
        let status_bar = div()
            .flex_none()
            .h(rems(ui::ROW_HEIGHT))
            .h_flex()
            .items_center()
            .justify_between()
            .gap_2()
            // 与工具栏、chips、行同一条左缘（`px_3` = 12）：上一版这里单独是 8，
            // 同一面板里 4/8/12 三条线，横向扫一眼就是没对齐
            .px_3()
            .border_t(ui::HAIRLINE)
            .border_color(colors.border)
            .text_xs()
            .text_color(colors.muted_foreground)
            .debug_selector(|| "data-objects-status-bar".to_string())
            .child(
                // 窄面板里这行字允许被截（`min_w_0` + ellipsis）：宁可少几个数，
                // 也不能把刷新钮顶出去
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .debug_selector(|| "data-objects-status-text".to_string())
                    .child(format!("{count_label} · {memory_label}")),
            )
            .child({
                let entity = entity.clone();
                Button::new("data-objects-refresh")
                    .ghost()
                    .xsmall()
                    .label(tr("刷新"))
                    .on_click(move |_, _, cx| {
                        entity.update(cx, |tab, cx| tab.refresh(cx));
                    })
            });

        // 面板根**零顶距**：工具栏紧贴 dock 标签条下缘（2026-11-25 反馈②）。
        // 此前这里 `.pt_2()`（8）——`inner_padding` 已覆写为 false，剩下的 8px 缝就是它。
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .child(toolbar)
            .child(chips)
            .child(list_frame)
            .when(!picked.is_empty(), |el| el.child(tray))
            .when_some(sess_row, |el, row| el.child(row))
            .when_some(self.note.clone(), |el, note| {
                el.child(
                    div()
                        .px_3()
                        .py_2()
                        .border_t_1()
                        .border_color(colors.border)
                        .text_xs()
                        .text_color(if note.is_error {
                            colors.danger
                        } else {
                            colors.muted_foreground
                        })
                        .child(note.text),
                )
            })
            // 工作集软上限：只提示代价，不拦（拦下来等于替用户决定要不要等）
            .when_some(
                data_objects::picked_limit_note(picked.len()),
                move |el, note| {
                    el.child(
                        div()
                            .px_3()
                            .py_2()
                            .border_t_1()
                            .border_color(colors.border)
                            .text_xs()
                            .text_color(colors.warning)
                            .child(note),
                    )
                },
            )
            .child(status_bar)
    }
}

/// 展开源的**状态行**（正在列举 / 空清单）：缩进与叶子对齐，只说明状态、不占交互。
///
/// 列举挪到后台线程之后，「点击展开到清单回来」之间有一段空档：什么都不画会让人
/// 以为点了没反应（以前同步列举时这段空档短到看不见，现在必须说话）。
/// 叶子的「显示更多」行：一页画满（[`LEAF_PAGE_SIZE`]）才出现，点一次加一页。
///
/// 与导航展开文件夹的「更多」行同一形态（同缩进、同页大小），不是新发明：
/// 上千张表的连接全量铺开时，每帧要建上千个元素，滚动会掉帧。
fn leaf_more_row(
    parent_key: &str,
    remaining: usize,
    entity: &Entity<DataObjectsTab>,
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let key = parent_key.to_string();
    let entity = entity.clone();
    div()
        .id(SharedString::from(format!(
            "data-object-leaf-more-{parent_key}"
        )))
        .h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(colors.border)
        .hover(|s| s.bg(colors.list_hover))
        // 「更多」是叶子那一层的东西：与叶子同缩进（第二层 = schema 之下，第三层）
        .child(tree::indent_spacer(ui::TREE_INDENT, 2))
        .child(tree::disclosure_slot())
        .child(
            Button::new(SharedString::from(format!(
                "data-object-leaf-more-btn-{parent_key}"
            )))
            .ghost()
            .xsmall()
            .label(format!("显示更多（还有 {remaining} 张）"))
            .on_click(move |_, _, cx| {
                let key = key.clone();
                entity.update(cx, |tab, cx| tab.show_more_leaves(&key, cx));
            }),
        )
        .into_any_element()
}

fn leaf_notice_row(row_id: &str, text: &str, color: Hsla) -> AnyElement {
    div()
        .id(SharedString::from(row_id.to_string()))
        .h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .child(tree::indent_spacer(ui::TREE_INDENT, 1))
        .child(tree::disclosure_slot())
        .child(div().text_xs().text_color(color).child(text.to_string()))
        .into_any_element()
}

/// 三级树的**第二层**：schema 分组行（缩进 1、可点收起、无行尾按钮）。
///
/// 为什么要有这一层：源段叶子现在是「别名（catalog）→ schema → 表 / 视图」，
/// 只画两层时 `orders.main` 与 `bi.main` 的同名表会挤成两行看起来一样的东西
/// （铁律第 37 条）。整行点击切换收起，与草稿箱分区头同一交互（`chrome.rs:2318`）、
/// 与导航分组头同一形态（`nav_view/rows.rs:711`）：行本身就是命中区。
///
/// 行尾**不摆任何按钮**（铁律第 42 条）：计数徽章是信息不是控件。
fn source_schema_row(
    schema_key: &str,
    layer: &str,
    count: usize,
    open: bool,
    entity: &Entity<DataObjectsTab>,
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let muted = colors.muted_foreground;
    let row_id = format!("data-object-schema-{schema_key}");
    let toggle_key = schema_key.to_string();
    let entity = entity.clone();
    div()
        .id(SharedString::from(row_id.clone()))
        .debug_selector(|| "data-object-schema-row".to_string())
        .h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(colors.border)
        .cursor_pointer()
        .hover(|s| s.bg(colors.list_hover))
        .on_click(move |_, _, cx| {
            let key = toggle_key.clone();
            entity.update(cx, |tab, cx| {
                if tab.collapsed_schemas.contains(&key) {
                    tab.collapsed_schemas.remove(&key);
                } else {
                    tab.collapsed_schemas.insert(key);
                }
                cx.notify();
            });
        })
        .child(tree::indent_spacer(ui::TREE_INDENT, 1))
        .child(tree::disclosure_slot().child(tree::disclosure_icon(open, muted)))
        .child(
            Icon::new(gpui_kit::assets::IconName::Folder)
                .xsmall()
                .text_color(muted),
        )
        .child(
            div()
                // 名称按内容宽、**不再吃满一行**：计数紧跟名称（2026-11-25 用户口径：
                // 计数是纯文本不是胶囊，位置在组名 / 层名之后），弹簧挪到行尾。
                .min_w_0()
                .text_xs()
                .text_color(muted)
                .overflow_hidden()
                .text_ellipsis()
                .child(layer.to_string()),
        )
        .child(tree::group_count(count, muted))
        .child(div().flex_1())
        .into_any_element()
}

/// 源行展开出的表 / 视图叶子行：缩进 + 图标 + 名字（**行尾没有按钮**，铁律第 42 条）。
///
/// 动作全在右键：复制限定名（引擎按源种类生成，三段 / 两段，判据在
/// `AccelKind::two_segment_names`）、归档到分析库、载入内存。**不摆可见的「选用」**：
/// 联邦 / 加速的表在各自的专用连接上，洞察取样（进程单例）够不着——与其摆一个
/// 点了会炸的入口，不如只给写 SQL 真正需要的那一截。
fn source_leaf_row(
    parent_key: &str,
    table: &engine::duckdb::accel::MountedTable,
    entity: &Entity<DataObjectsTab>,
    shared: &Shared,
    source: Option<&data_objects::SourceRef>,
    source_name: &str,
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let muted = colors.muted_foreground;
    let leaf_id = format!("data-object-leaf-{parent_key}-{}", table.qualified);
    let mut row = div()
        .id(SharedString::from(leaf_id.clone()))
        .h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(colors.border)
        .hover(|s| s.bg(colors.list_hover))
        // 缩进 = `TREE_INDENT × 2`：叶子在「别名 → schema」之下（与 picker 同一套原语）
        .child(tree::indent_spacer(ui::TREE_INDENT, 2))
        // 展开槽留空：叶子没有下一层，但缩进对齐靠这个固定宽槽
        .child(tree::disclosure_slot())
        .child(
            Icon::new(if table.is_view {
                gpui_kit::assets::IconName::Eye
            } else {
                gpui_kit::assets::IconName::Table
            })
            .xsmall()
            .text_color(muted),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(colors.foreground)
                .overflow_hidden()
                .text_ellipsis()
                .child(if table.is_view {
                    format!("{}（视图）", table.name)
                } else {
                    table.name.clone()
                }),
        );
    // schema 不是 main 时补在行尾（两段名源与 DuckDB 文件源的 main 不展示，噪音）
    if table.schema != "main" && !table.schema.is_empty() {
        row = row.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(table.schema.clone()),
        );
    }
    let qualified = table.qualified.clone();
    let table_name = table.name.clone();
    // 菜单闭包要 `'static`：借来的参数先克隆成自有值（`context_menu` 存进元素树，
    // 生命周期比这个函数长）
    let shared_owned = shared.clone();
    let source_owned = source.cloned();
    let source_name_owned = source_name.to_string();
    let entity_owned = entity.clone();

    // 铁律第 42 条：叶子行尾**不摆按钮**，动作全走右键。
    //
    // 「复制限定名」每一张挂载源的表都能做（什么都不用查）；归档 / 载入只对挂在
    // 分析实例上的源开口（`source` 有值 = 会话 / 加速 / 文件库），裸连接的表还没有
    // 取数通道（见 `services::load_jobs::load` 的报错分支）。
    //
    // 载入 = 把这张**已经挂进分析实例**的源表复制成 `tmp_l_`，会话级、不登记资产（第 35 条）。
    // 写库在 worker 线程，回执由面板自己的小泵取回（第 25 条）。
    row.context_menu(move |menu, _window, _cx| {
        let mut menu = menu;
        {
            let qualified = qualified.clone();
            let entity = entity_owned.clone();
            menu = menu.item(
                PopupMenuItem::new(tr("复制限定名")).on_click(move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(qualified.clone()));
                    entity.update(cx, |tab, cx| {
                        tab.note = Some(PanelNote::info(format!("已复制限定名：{qualified}")));
                        cx.notify();
                    });
                }),
            );
        }
        if let Some(source) = source_owned.clone() {
            // 归档（第 35 条前半）：先问表名（撞名要用户改），再进 worker。
            {
                let qualified = qualified.clone();
                let table_name = table_name.clone();
                let source = source.clone();
                let source_name = source_name_owned.clone();
                let entity = entity_owned.clone();
                let shared = shared_owned.clone();
                menu = menu.item(PopupMenuItem::new("归档到分析库…").on_click(
                    move |_, window, cx| {
                        crate::components::archive_dialog::open_archive_dialog(
                            shared.clone(),
                            entity.clone(),
                            source.clone(),
                            source_name.clone(),
                            table_name.clone(),
                            qualified.clone(),
                            window,
                            cx,
                        );
                    },
                ));
            }
            // 载入（第 35 条后半）：一键落到 `tmp_l_`，不征询名字（会话级、不登记）。
            {
                let qualified = qualified.clone();
                let table_name_for_job = table_name.clone();
                let table_name_for_note = table_name.clone();
                let source = source.clone();
                let source_name = source_name_owned.clone();
                let entity = entity_owned.clone();
                let shared = shared_owned.clone();
                menu = menu.item(PopupMenuItem::new("载入内存…").on_click(move |_, _, cx| {
                    // 名字用 `<源>_<表>`：临时表名的尺子只留 ASCII，中文会折成下划线，
                    // 回执里显示的是**实际落下的表名**（`landed`），不是用户看到的这张表名
                    let description = format!("{source_name}_{table_name_for_job}");
                    let job = crate::services::load_jobs::start(
                        source.clone(),
                        source_name.clone(),
                        table_name_for_note.clone(),
                        qualified.clone(),
                        description,
                    );
                    shared.say(format!("正在载入「{table_name_for_note}」…"), cx);
                    entity.update(cx, |tab, cx| tab.watch_load(job, cx));
                }));
            }
        }
        menu
    })
    .into_any_element()
}

/// 源库表的叶子行：缩进 + 图标 + 表名 + 所在 catalog / schema + 「选用」。
///
/// 与 [`source_leaf_row`]（已挂载源的叶子）的区别就是**能选用**：已挂载源的表在各自的
/// 专用连接上，取样要等公共入口；而源库表本来就有连接 id，取样直接连接下推
/// （SQL 由 `Shared::insight_sample_sql` 按驱动方言拼），所以这一档能用。
fn source_table_leaf_row(
    leaf: &data_objects::Object,
    picked: bool,
    entity: &Entity<DataObjectsTab>,
    shared: &Shared,
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let muted = colors.muted_foreground;
    let leaf_id = SharedString::from(format!("data-object-srctable-{}", leaf.key));
    let key_for_menu = leaf.key.clone();
    let entity_for_menu = entity.clone();
    let props_object = leaf.clone();
    let shared_for_menu = shared.clone();
    // 小标写「已选」：选中状态仍要看得见（铁律 G 第 30 条），但**不做成行尾按钮**
    let pill = if picked {
        format!("{} · 已选", leaf.pill_text())
    } else {
        leaf.pill_text()
    };
    div()
        .id(leaf_id.clone())
        .h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(colors.border)
        .hover(|s| s.bg(colors.list_hover))
        .child(tree::indent_spacer(ui::TREE_INDENT, 2))
        .child(tree::disclosure_slot())
        .child(
            Icon::new(gpui_kit::assets::IconName::Table)
                .xsmall()
                .text_color(muted),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(colors.foreground)
                .overflow_hidden()
                .text_ellipsis()
                .child(leaf.name.clone()),
        )
        // 三级树下 schema 已进第二层分组行，这里不再重复印 `catalog.schema`
        // 铁律 G 第 30 条 + 第 42 条：源段这条是**源库里的表**（数据还在源库），
        // 小标说清它属于哪一段；行尾不摆「选用」按钮，动作走右键。
        .child(Tag::secondary().rounded_full().flex_none().child(pill))
        .context_menu(move |menu, _window, _cx| {
            let mut menu = menu;
            let key = key_for_menu.clone();
            let entity = entity_for_menu.clone();
            let label = if picked {
                "移出工作集"
            } else {
                "选入工作集"
            };
            menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                entity.update(cx, |tab, cx| tab.toggle_picked(&key, cx));
            }));
            let props = shared_for_menu.clone();
            let object = props_object.clone();
            menu = menu.item(PopupMenuItem::new("属性…").on_click(move |_, window, cx| {
                crate::components::object_props::open_object_props(
                    props.clone(),
                    object.clone(),
                    window,
                    cx,
                );
            }));
            menu
        })
        .into_any_element()
}

impl EventEmitter<BasePanelEvent> for DataObjectsTab {}

impl Focusable for DataObjectsTab {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DataObjectsTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 清单过期信号：render 只做**比对**（纯读一个计数），重取用 `defer_in` 排到帧后
        // （事件路径）。不能在这里直接 `refresh`：`collect` 有读盘与查库，渲染期做 I/O
        // 是 gpui-kit-dev 明令禁止的（`panels/editor.rs` 的连接详情卡就是登记的反例）。
        //
        // 先记下版本号再排刷新：这样「改了但这一帧没重取完」不会每帧重排一次。
        let epoch = self.shared.data_objects_epoch.get();
        if epoch != self.seen_epoch {
            self.seen_epoch = epoch;
            // 别的路径写了清单（加源、切项目）：缓存作废，这一帧就按新清单重建，
            // 不靠下一帧的 refresh 才跟上
            self.rows_cache = None;
            cx.defer_in(window, |tab, _window, cx| tab.refresh(cx));
        }
        // 切文档信号：同样只比对计数，换工作集用 `defer_in` 排到帧后（那一步要读 / 写盘）
        let document_epoch = self.shared.working_set.borrow().epoch;
        if document_epoch != self.seen_document_epoch {
            self.seen_document_epoch = document_epoch;
            cx.defer_in(window, |tab, _window, cx| tab.follow_active_document(cx));
        }
        self.render_body(window, cx)
    }
}

impl BasePanel for DataObjectsTab {
    fn panel_name(&self) -> &'static str {
        "data-objects"
    }

    fn closable(&self, _cx: &App) -> bool {
        // 右侧栏 tab 与中央标签一致（2026-11-24 用户验收）：悬停出 ✕、点击关掉这个 tab。
        true
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_right.set(RightPanel::DataObjects);
            shared.notify_host(cx);
            // 切到本面板时重取一次：进入面板 = 用户要看「现在有什么」，
            // 停在构造时那份快照会给出过期结论（资产段尤其明显）。
            self.refresh(cx);
        }
    }
}

impl ComponentPanel for DataObjectsTab {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 与其它右 dock tab 一致：返回 None，用 title() 渲染图标 + 文字
        None
    }

    /// 【关闭前询问】右侧栏 tab 的 ✕：没有脏概念，撤登记后交还皮肤移除。
    fn request_close(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::component::dock::CloseRequest {
        self.shared.close_right_panel(RightPanel::DataObjects, cx);
        gpui_kit::component::dock::CloseRequest::Proceed
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
    ) -> Option<Vec<Button>> {
        // 模块内部已有工具栏（`render_body` 的第一行：搜索框 + 加源 / 存快照），
        // Dock 标题栏只做容器管理——与资产库 / 数据库导航面板同一口径。
        // 「加源」是面板唯一往外接数据的入口：两档 scope（铁律 B 组第 6 条）按需（视图）/
        // 快照（存副本）；导出格式不在这两处，问在导出窗口里（2026-11-24 拍板）。
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

/// 面板可用宽（行宽）：prepaint 时量到就写进来，供**下一帧**的行布局判档。
///
/// 首帧是 0（还没量到），`RowTier::for_width(0)` 按全列画——宁可第一帧挤一点，
/// 也不要先画成极简再跳回全列。
#[derive(Default)]
struct RowWidth(std::cell::Cell<f32>);

impl RowWidth {
    fn get(&self) -> f32 {
        self.0.get()
    }

    fn set(&self, width: f32) {
        self.0.set(width);
    }
}

/// 每一行名字格的**实际宽**（prepaint 量到）：走马灯判据的另一半。
///
/// 不按「面板宽 − 常量」推：行尾按钮数随行的可点性变化（源连接行没有勾选钮），
/// 同宽面板里各行的名字格并不等宽——只有量出来的数才可信。
#[derive(Default)]
struct NameWidths(HashMap<String, f32>);

impl NameWidths {
    fn get(&self, key: &str) -> Option<f32> {
        self.0.get(key).copied()
    }

    /// 只在与上次不同（>0.5px）时才写：prepaint 每帧都跑，写回不判等会成帧抖。
    fn remember(&mut self, key: String, width: f32) {
        if let Some(&old) = self.0.get(&key) {
            if (old - width).abs() <= 0.5 {
                return;
            }
        }
        self.0.insert(key, width);
    }
}

/// 鼠标此刻停在哪一行（`None` = 没停在任何行）：只有这一行才跑走马灯。
#[derive(Default)]
struct HoveredRow(Option<String>);

/// 单行文本的渲染宽：与 `Text` 同一套 shaping（同字体、同字号、同字距）。
///
/// 换行符会被 `shape_line` 当非法输入（名字里不会有，但外部来的副标题可能有），
/// 统一折成空格再量。
fn text_width(window: &Window, text: &str, font_size: Pixels) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let flat;
    let text = if text.contains(['\n', '\r', '\t']) {
        flat = text.replace(['\n', '\r', '\t'], " ");
        flat.as_str()
    } else {
        text
    };
    let style = window.text_style();
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(
            SharedString::from(text.to_string()),
            font_size,
            &[run],
            None,
        )
        .width()
        .into()
}

#[cfg(test)]
mod tests {
    use super::{LEAF_PAGE_SIZE, leaf_shown};

    #[test]
    fn leaves_start_with_one_page_and_grow_by_a_page() {
        // 没点过「显示更多」：恰好一页
        assert_eq!(leaf_shown(LEAF_PAGE_SIZE + 5, None), LEAF_PAGE_SIZE);
        // 点过一次：两页（清单够两页时才画两页）
        assert_eq!(
            leaf_shown(LEAF_PAGE_SIZE * 2 + 5, Some(2)),
            LEAF_PAGE_SIZE * 2
        );
        // 清单只有一页零几张时，第二页也只画到清单尾
        assert_eq!(leaf_shown(LEAF_PAGE_SIZE + 5, Some(2)), LEAF_PAGE_SIZE + 5);
        // 清单不够一页：全画（多翻的页数不会越界）
        assert_eq!(leaf_shown(3, Some(9)), 3);
        // 页数为 0 是异常值：按一页算，绝不能画 0 行（否则展开像没反应）
        assert_eq!(leaf_shown(10, Some(0)), LEAF_PAGE_SIZE.min(10));
    }

    // ── 真实布局测量见 `layout_probe.rs`（headless 窗口 + gpui 排版引擎）────────
}

#[cfg(test)]
mod layout_probe;
