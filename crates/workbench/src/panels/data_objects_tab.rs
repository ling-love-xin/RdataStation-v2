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
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::empty::{
    Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle,
};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::tag::Tag;
use gpui_kit::*;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use workbench_shell::{tree, ui};

use crate::components::data_objects::{self, Group};
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
    /// 导出格式不在这里：它是「这一次导出」的属性，活在导出窗口里（见 `components/export_dialog.rs`）
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

    /// 删掉清单里的临时对象（进程内存里的临时表；删了要重跑才有）。
    ///
    /// 范围与「升级为资产」对称（`can_clear` = `result:` 前缀）：Mock / 插件表同段但
    /// 归各自模块管，不从这里删（删了它们模块的登记就指向空表）。
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
        let outcome = match action {
            SessionAction::RefreshSource { owner, alias } => {
                engine::duckdb::federation::session::refresh_source(&owner, &alias).map(|_| ())
            }
            SessionAction::SetPrimary { owner, alias } => {
                engine::duckdb::federation::session::set_primary(&owner, &alias)
            }
            SessionAction::DropSession { owner } => {
                let dropped = engine::duckdb::federation::session::drop_session(&owner);
                match dropped {
                    true => Ok(()),
                    // 后端返 bool：没这个会话就是没关成，别报成功
                    false => Err(format!("没有这个会话：{owner}")),
                }
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
    fn toggle_source_expand(&mut self, key: &str, cx: &mut Context<Self>) {
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
        // 项目根是连接列举唯一需要的宿主信息（`Shared` 不能跨线程）
        let project_root = self.shared.project.borrow().as_ref().map(|p| p.root.clone());
        match source_kids_jobs::submit(key.to_string(), source, project_root, announce) {
            Ok(()) => {
                self.loading_sources.insert(key.to_string());
            }
            Err(reason) if announce => {
                self.note = Some(PanelNote::error(reason));
            }
            Err(_) => {}
        }
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
                self.source_children
                    .insert(outcome.key, Arc::new(tables));
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
        self.shared.project.borrow().as_ref().map(|p| p.root.clone())
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
            InputState::new(window, cx).placeholder("输入表名 / 文件名，直接定位")
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
            format!("{} 个（显示 {}）· 已选 {}", total, objects.len(), picked.len())
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
                    .child(count_label),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(memory_label),
            )
            // 清单会变（连上/断开连接、临时表失效、资产改了版本），面板得有条自己重取的路
            .child({
                let entity = entity.clone();
                Button::new("data-objects-refresh")
                    .ghost()
                    .label("刷新")
                    .on_click(move |_, _, cx| {
                        entity.update(cx, |tab, cx| tab.refresh(cx));
                    })
            });

        let search_row = div().px_3().pb_2().child(Input::new(&search).small());

        // 三条口径先写清楚：省得每行都要解释"为什么只有一版""洞察产物在哪""为什么展不开"
        let hint = div()
            .text_xs()
            .text_color(colors.muted_foreground)
            .px_3()
            .pb_2()
            .child("只有三段（源 / 表 / 文件）；资产只给最新版本；洞察产物 tmp_i_ 不进本清单；源要展开连接才列表，没挂载的源展不开");

        // 分组 chips：三个段各一个（业务身份是小标，不单独当筛选项）
        let mut chips = div()
            .h_flex()
            .items_center()
            .flex_wrap()
            .gap_1()
            .px_3()
            .pb_2();
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
                    data_objects::source_table_parts(key)
                        .map(|(_, _, _, table)| table.to_string())
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
        tray = tray.child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(match picked.len() {
                    0 | 1 => "洞察只取这一个对象；想看别的就再点「选用」，各取各的样本".to_string(),
                    count => format!("已选 {count} 个：洞察会为每个对象各取一份样本，不跨源 join"),
                }),
        );
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
                        .label("清空")
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
                            .label("清理临时对象")
                            .on_click(move |_, window, cx| {
                                let entity = entity_for_clear.clone();
                                // 删的是进程内存里的临时表：先问一句，别让人一抬手就没了
                                window.open_alert_dialog(cx, move |alert, _window, _cx| {
                                    let entity = entity.clone();
                                    alert
                                        .confirm()
                                        .title("清理临时对象")
                                        .description("将删掉清单里的临时表。它们在进程内存里，删了要重跑才有；想留就先「升级为资产」。确定继续？")
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
            match group_counts.iter_mut().find(|(group, _)| *group == object.group) {
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
                list = list.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .pt_3()
                        .pb_1()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(
                            Button::new(SharedString::from(format!(
                                "data-objects-group-{}",
                                group.title()
                            )))
                            .ghost()
                            .icon(if expanded {
                                gpui_kit::assets::IconName::ChevronDown
                            } else {
                                gpui_kit::assets::IconName::ChevronRight
                            })
                            .on_click(move |_, _, cx| {
                                let group = group;
                                entity_for_group.update(cx, |tab, cx| {
                                    tab.toggle_group(group, cx);
                                });
                            }),
                        )
                        .child(object.group.title())
                        .child(div().flex_1())
                        .child(div().text_xs().child(group_count.to_string())),
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
            let persist_target = data_objects::can_persist(object)
                .then(|| (object.name.clone(), data_objects::persist_label(&object.name)));
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
                    .label("装扩展")
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
            let in_source_group = object.group == Group::Source;
            let source_expandable = in_source_group
                && object.can_analyze()
                && data_objects::source_ref(&object.key).is_some();
            let expanded_now = self.expanded_sources.contains(&object.key);
            let disclosure = if in_source_group {
                Some(if source_expandable {
                    let key = object.key.clone();
                    let entity = entity.clone();
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
                    .on_click(move |_, _, cx| {
                        let key = key.clone();
                        entity.update(cx, |tab, cx| tab.toggle_source_expand(&key, cx));
                    })
                    .into_any_element()
                } else {
                    tree::disclosure_slot().into_any_element()
                })
            } else {
                None
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
                    .border_b_1()
                    .border_color(colors.border)
                    .relative()
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
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    // 窄面板里名字一长就顶出去，行尾按钮会被挤掉
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(object.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(object.note.clone()),
                            ),
                    )
                    // 业务身份走标准 Tag：手写胶囊的圆角、内距、字号各写一遍就会漂
                    .child(pill.child(object.pill_text()))
                    .child(
                        div()
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
                    .child(
                        div()
                            .text_xs()
                            .flex_none()
                            .min_w_10()
                            .text_color(state_color)
                            // 能用 → 寿命（会不会自己消失）；不能用 → 原因（要动手解决什么）
                            .child(object.availability_text()),
                    )
                    .children(pick_button)
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
                        // 属性窗口要整行快照：必须先克隆到闭包外。`object` 是借来的引用，
                        // 在菜单闭包里 `object.clone()` 只会克隆那个引用，闭包就不满足 'static
                        let props_object = (*object).clone();
                        move |menu, _window, _cx| {
                            let mut menu = menu;
                            if let Some(reference) = sql_ref.clone().filter(|_| usable) {
                                let table = bare_table_name(&reference);
                                let open = shared.clone();
                                let open_ref = reference.clone();
                                menu = menu.item(
                                    PopupMenuItem::new("在编辑器打开").on_click(move |_, _, cx| {
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
                                menu = menu.item(PopupMenuItem::new("洞察（看数据）").on_click(
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
                                    menu = menu.item(PopupMenuItem::new("作为 Mock 结构来源").on_click(
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
                                menu = menu.item(PopupMenuItem::new("登记为资产").on_click(
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
                                menu = menu.item(PopupMenuItem::new("升级为资产").on_click(
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
                            if let Some(alias) = alias.clone() {
                                let snapshot = shared.clone();
                                menu = menu.item(PopupMenuItem::new("归档为快照").on_click(
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
                                                format!("归档：找不到 {alias} 的路径"),
                                                cx,
                                            ),
                                        }
                                    },
                                ));
                            }
                            // 属性：这一行到底是哪个东西（身份键 / 通道 / 寿命 / 列结构）
                            // 与「复制引用名」一起放在菜单尾部：都是"看/抄"，不是"动数据"
                            let props_row = props_object.clone();
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
                            for leaf in children.iter().take(shown) {
                                let leaf_picked = picked_set.contains(leaf.key.as_str());
                                list = list.child(source_table_leaf_row(
                                    leaf,
                                    leaf_picked,
                                    &entity,
                                    cx,
                                ));
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
                                    .child("这个连接里没有可列的表 / 视图（目录为空或没有权限）"),
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
                            for table in children.iter().take(shown) {
                                list =
                                    list.child(source_leaf_row(&object.key, table, &entity, cx));
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
                                            .child("没有可列的表（挂载成功但目录为空）"),
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
                        .title(EmptyTitle::new().child("还没有可用的数据对象"))
                        .description(EmptyDescription::new().child(
                            "先连一个数据源，或跑一次查询；能被 DuckDB 加载的文件也可以直接放进来。",
                        )),
                )
                // 空态给一条能直接走的路：说了“可以放文件”，就得有个按钮去放
                .content(EmptyContent::new().child({
                    let shared = self.shared.clone();
                    Button::new("data-objects-empty-load")
                        .label("加载文件到分析")
                        .tooltip("按需加载：挂成视图读原文件，切项目即清")
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
                            .title(EmptyTitle::new().child("这个条件下没有对象"))
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
                            .label("清条件看全部")
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

        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .pt_2()
            .child(header)
            .child(search_row)
            .child(chips)
            .child(hint)
            .child(list)
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
            .when_some(data_objects::picked_limit_note(picked.len()), move |el, note| {
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
            })
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
        .id(SharedString::from(format!("data-object-leaf-more-{parent_key}")))
        .h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(colors.border)
        .hover(|s| s.bg(colors.list_hover))
        .child(tree::indent_spacer(ui::TREE_INDENT, 1))
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

/// 源行展开出的表 / 视图叶子行：缩进 + 图标 + 名字 + 「复制名」。
///
/// 叶子**只做复制限定名**——限定名由引擎按源种类生成（三段 / 两段，判据在
/// `AccelKind::two_segment_names`），粘进对应通道的 SQL 就能用。**不给「选用」入口**：
/// 联邦 / 加速的表在各自的专用连接上，洞察取样（进程单例）够不着；等公共入口
/// Phase 3「洞察搬结果」落地再开。与其摆一个点了会炸的入口，不如只给写 SQL
/// 真正需要的那一截。
fn source_leaf_row(
    parent_key: &str,
    table: &engine::duckdb::accel::MountedTable,
    entity: &Entity<DataObjectsTab>,
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
        // 缩进一层（与 picker 树同一套原语：`TREE_INDENT × depth`）
        .child(tree::indent_spacer(ui::TREE_INDENT, 1))
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
    row.child(
        Button::new(SharedString::from(format!("data-object-leaf-copy-{leaf_id}")))
            .ghost()
            .xsmall()
            .label("复制名")
            .on_click({
                let qualified = qualified.clone();
                let entity = entity.clone();
                move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(qualified.clone()));
                    entity.update(cx, |tab, cx| {
                        tab.note = Some(PanelNote::info(format!("已复制限定名：{qualified}")));
                        cx.notify();
                    });
                }
            }),
    )
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
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let muted = colors.muted_foreground;
    let leaf_id = SharedString::from(format!("data-object-srctable-{}", leaf.key));
    let key_for_click = leaf.key.clone();
    let entity_for_click = entity.clone();
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
        .child(tree::indent_spacer(ui::TREE_INDENT, 1))
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
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(leaf.note.clone()),
        )
        // 铁律 G 第 30 条：源段这条是**源库里的表**（数据还在源库），与表段里
        // 同名的那条（资产最新版本）不是一回事，小标说清它属于哪一段。
        .child(
            Tag::secondary()
                .rounded_full()
                .flex_none()
                .child(leaf.pill_text()),
        )
        .child(
            Button::new(SharedString::from(format!(
                "data-object-srctable-pick-{}",
                leaf.key
            )))
            .ghost()
            .xsmall()
            .toggled(picked)
            .label(if picked { "已选" } else { "选用" })
            .on_click(move |_, _, cx| {
                let key = key_for_click.clone();
                entity_for_click.update(cx, |tab, cx| tab.toggle_picked(&key, cx));
            }),
        )
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
        false
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
        cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        // 「加源」是面板唯一往外接数据的入口：只接能直接读的文件，库表由连接下推。
        // 两档 scope（铁律 B 组第 6 条）在工具栏上就是两个按钮：按需（视图）/ 快照（存副本）。
        // 导出格式**不在这里**：它是「这一次导出」的属性，问在导出窗口里（2026-11-24 拍板）。
        Some(vec![
            Button::new("data-objects-add-source")
                .ghost()
                .label("加源")
                .tooltip("按需加源：挂成视图读原文件，切项目即清")
                .on_click(cx.listener(|this, _, window, cx| {
                    crate::components::data_objects_load::open_load_files_dialog(
                        &this.shared,
                        crate::components::data_objects_load::LoadScope::Session,
                        window,
                        cx,
                    );
                })),
            Button::new("data-objects-add-source-snapshot")
                .ghost()
                .label("存快照")
                .tooltip("把选中的文件在资源库里存一份只读副本，跟着项目走、带版本")
                .on_click(cx.listener(|this, _, window, cx| {
                    crate::components::data_objects_load::open_load_files_dialog(
                        &this.shared,
                        crate::components::data_objects_load::LoadScope::Snapshot,
                        window,
                        cx,
                    );
                })),
        ])
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
}

