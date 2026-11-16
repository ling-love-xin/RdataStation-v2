//! 「数据对象」右 dock tab：所有可参与分析的对象一处分览。
//!
//! 取数在**事件路径**（构造 / 刷新）跑一次并写进 `Shared::data_objects`，渲染只读缓存——
//! `collect` 内含读盘与查库，放进 render 就是每帧 I/O（gpui-kit-dev 禁止）。
//!
//! 行上四类动作各管一段：勾选进工作集（`toggle_picked`）、会话行菜单（重挂 / 设主源 /
//! 关会话）、结果集与洞察产物的「存为表」（进项目分析库的持久表，写库在 worker 线程）、
//! 资产行的「升级」（表型存档 → 全局，搬表也在 worker 线程）。
//!
//! 前两个是场景 3 的两段，顺序不能颠倒：临时表在进程单例里（项目库打不开它），
//! 所以先「存为表」拿到存档身份，才谈得上「升级到全局」并留下版本线索。

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::tag::Tag;
use gpui_kit::*;

use crate::components::data_objects::{self, Group};
use crate::view::RightPanel;

use super::Shared;

pub struct DataObjectsTab {
    shared: Shared,
    focus_handle: FocusHandle,
    /// 上一次会话动作的结果（失败原话）；`None` = 没动作过或已恢复
    note: Option<String>,
    /// 哪一行的会话动作条展开着：一屏只开一个，跟原型一致
    expanded_session: Option<String>,
    /// 分组筛选（原型 07 的 6 个 chips）：`None` = 全部
    filter: Option<Group>,
    /// 上次刷新时的进程内存读数（字节, 临时表数）；`None` = 读不到
    memory: Option<(u64, usize)>,
    /// 上次刷新时的会话挂载统计（已挂, 总数）；总数 0 = 这个文档没开会话
    session: Option<(usize, usize)>,
    /// 收起的段：段头照画（带计数），行不画——空段也得让人知道它空着
    collapsed: Vec<Group>,
}

impl DataObjectsTab {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        let mut tab = Self {
            shared,
            focus_handle: cx.focus_handle(),
            note: None,
            expanded_session: None,
            filter: None,
            memory: None,
            session: None,
            collapsed: Vec::new(),
        };
        tab.refresh(cx);
        tab
    }

    /// 删掉清单里的临时对象（进程内存里的临时表；删了要重跑才有）。
    ///
    /// 属于破坏性动作，调用前必须过确认框；这里只管执行与把失败原话留在面板上。
    fn clear_temp_objects(&mut self, cx: &mut Context<Self>) {
        let names: Vec<String> = self
            .shared
            .data_objects
            .borrow()
            .iter()
            .filter(|object| object.group == Group::TemporaryTables)
            .map(|object| object.name.clone())
            .collect();
        if names.is_empty() {
            self.note = Some("清单里没有临时对象".to_string());
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
            format!("已清理 {dropped} 个临时对象")
        } else {
            format!("清理 {dropped} 个，{} 个没清掉：{}", failed.len(), failed.join("；"))
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
            Ok(()) => "已更新会话，清单已刷新".to_string(),
            Err(reason) => format!("操作失败：{reason}"),
        });
        // 动作改了会话表清单：重取一次，别让行上的表数停在旧值
        self.refresh(cx);
    }

    /// 重新取一次清单（事件路径）。
    ///
    /// 资产那段不走库查询：资产面板实体已经把存活行载在内存里（`view_rows`），
    /// 这里只做形状翻译。面板还没载过（弱句柄取不到 / 行还空）时资产段就是空——
    /// 显示「资产面板载过之后可见」，而不是自己开一次连接去查（那是两套真相）。
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
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
    fn toggle_picked(&mut self, key: &str, cx: &mut Context<Self>) {
        let mut picked = self.shared.data_object_picked.borrow_mut();
        // 面板只承载**一个**对象：多源在洞察面板里按「+」加。这里若允许攒多选，
        // 同一个对象会从两处各进一次来源，反而要用户去收拾重复。
        if picked.len() == 1 && picked[0] == *key {
            picked.clear();
        } else {
            picked.clear();
            picked.push(key.to_string());
        }
        drop(picked);
        cx.notify();
    }

    fn render_body(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let all = self.shared.data_objects.borrow().clone();
        let total = all.len();
        // 搜索框：`InputState` 要窗口才能建，面板 `new` 没有窗口，所以按 key 在渲染时惰性建
        // （同一 id 每次拿到的是同一个实体，输入内容不会因重渲染丢）
        let search = window.use_keyed_state("data-objects-search", cx, |window, cx| {
            InputState::new(window, cx).placeholder("输入表名 / 文件名，直接定位")
        });
        let query = search.read(cx).value().trim().to_lowercase();

        // 筛选只改“看得见哪些”，不动工作集：选了某个对象后换个段看，已选仍在
        let filter = self.filter;
        let objects: Vec<data_objects::Object> = all
            .iter()
            .filter(|object| {
                filter.is_none_or(|group| object.group == group)
                    && (query.is_empty()
                        || object.name.to_lowercase().contains(&query)
                        || object.note.to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        let picked = self.shared.data_object_picked.borrow().clone();
        // 头部只说清“这是哪一档引擎、现在占多少”：读数在刷新时取一次，取不到就显示「—」
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
                    .child(format!("{} 个 · 已选 {}", total, picked.len())),
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

        // 两条口径先写清楚：省得每行都要解释“为什么只有一版”“洞察产物在哪”
        let hint = div()
            .text_xs()
            .text_color(colors.muted_foreground)
            .px_3()
            .pb_2()
            .child("每组只显示最新版本；洞察产物 tmp_i_ 不进本清单");

        // 分组 chips：五段全展开太长，先收窄到一段再看（原型 07 的 6 个 chip）
        let mut chips = div()
            .h_flex()
            .items_center()
            .flex_wrap()
            .gap_1()
            .px_3()
            .pb_2();
        let mut entries: Vec<(Option<Group>, String)> = vec![(None, "全部".to_string())];
        for group in [
            Group::MountedSources,
            Group::ProjectLibrary,
            Group::GlobalLibrary,
            Group::ExternalFiles,
            Group::TemporaryTables,
        ] {
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
        for key in picked.iter() {
            let name = all
                .iter()
                .find(|object| &object.key == key)
                .map(|object| object.name.clone())
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
                            .on_click(move |_, _, cx| {
                                let key = key_for_remove.clone();
                                entity_for_remove.update(cx, |tab, cx| {
                                    tab.toggle_picked(&key, cx);
                                });
                            }),
                    ),
            );
        }
        tray = tray.child(selected_row);
        // 原型 07 的 tray 尾巴：先说清多处来源怎么加，再把主按钮放这儿——
        // 头部那排按钮是“对这面板做什么”，这里才是“拿这堆对象去做什么”
        tray = tray.child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("要多个来源，进洞察后用「+」再加"),
        );
        let shared_for_tray = self.shared.clone();
        let entity_for_tray = entity.clone();
        tray = tray.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("data-objects-analyze")
                        .label("洞察此对象")
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
                            let (sources, skipped) = data_objects::analysis_sources(
                                &objects,
                                &picked,
                                &assets,
                                resources_dir.as_deref(),
                            );
                            if sources.is_empty() {
                                // 选的全是转不了的类型：别开一个空洞察面板
                                return;
                            }
                            let label = match skipped.is_empty() {
                                true => format!("{} 个来源", sources.len()),
                                false => format!("{} 个来源（{} 未包含）", sources.len(), skipped.len()),
                            };
                            shared_for_tray.open_insight_source_tables(sources, label, cx);
                            entity_for_tray.update(cx, |_, cx| cx.notify());
                        }),
                )
                .child(div().flex_1())
                .child({
                    let shared = self.shared.clone();
                    let entity = entity.clone();
                    Button::new("data-objects-clear")
                        .ghost()
                        .label("清空")
                        .on_click(move |_, _, cx| {
                            shared.data_object_picked.borrow_mut().clear();
                            // 回调拿到的是 `&mut App`：通知要落到本面板实体上，不能裸调
                            entity.update(cx, |_, cx| cx.notify());
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
        let has_temp = self
            .shared
            .data_objects
            .borrow()
            .iter()
            .any(|object| object.group == Group::TemporaryTables);
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
                                        .description("将删掉清单里的临时表。它们在进程内存里，删了要重跑才有；想留就先「存为表」。确定继续？")
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

        let mut last_group: Option<Group> = None;
        for object in &objects {
            if last_group != Some(object.group) {
                last_group = Some(object.group);
                // 段头带计数：这一段里几行，扫一眼就知道要不要往下翻
                let group_count = objects.iter().filter(|o| o.group == object.group).count();
                let group = object.group.clone();
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
                                let group = group.clone();
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
            let is_picked = picked.iter().any(|k| k == &object.key);
            // 只有能用的对象能进工作集：失效行点进去只会在分析时炸
            let pick_button = if object.can_analyze() {
                let key = object.key.clone();
                let entity = entity.clone();
                Some(
                    Button::new(SharedString::from(format!(
                        "data-object-pick-{}",
                        object.key
                    )))
                    .ghost()
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
            let session_actions = if object.group == Group::MountedSources {
                data_objects::session_actions(&object.key, &object.state)
            } else {
                Vec::new()
            };
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
            // 资产行才给「升级到全局」：那一行已经有存档身份，升上去能回指上游。
            // 临时表 / 项目自建表先「存为表」——没有登记就没有身份，硬升会丢线索。
            // 结果集 / 洞察产物已经在进程单例里：落库就是整表搬一份。
            // 写库走 worker 线程（`services::persist_jobs`），这里只入队，回执由工作台泵到状态栏。
            let persist_button = if data_objects::can_persist(object) {
                let shared = self.shared.clone();
                let entity = entity.clone();
                let table = object.name.clone();
                let label = data_objects::persist_label(&table);
                Some(
                    Button::new(SharedString::from(format!(
                        "data-object-persist-{}",
                        object.key
                    )))
                    .ghost()
                    .label("存为表")
                    .on_click(move |_, _, cx| {
                        crate::components::result_set_actions::persist_to_project(
                            &shared, &table, &label, cx,
                        );
                        entity.update(cx, |_, cx| cx.notify());
                    }),
                )
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
                                    .child(object.name.clone()),
                            )
                            // 业务身份走标准 Tag：手写胶囊的圆角、内距、字号各写一遍就会漂
                            .child(
                                pill.child(object.pill.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(object.note.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_none()
                            .text_color(colors.muted_foreground)
                            .child(object.channel.label()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_none()
                            .text_color(state_color)
                            .child(object.state.label()),
                    )
                    .children(pick_button)
                    .children(persist_button)
                    .children(session_button),
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
        }

        if objects.is_empty() {
            // 空态走标准组件：图标 + 标题 + 说明 + 下一步，不再是一行灰字
            list = list.child(
                Empty::new().header(
                    EmptyHeader::new()
                        .media(EmptyMedia::new())
                        .title(EmptyTitle::new().child("还没有可用的数据对象"))
                        .description(EmptyDescription::new().child(
                            "先连一个数据源，或跑一次查询；能被 DuckDB 加载的文件也可以直接放进来。",
                        )),
                ),
            );
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
                        .text_color(colors.muted_foreground)
                        .child(note),
                )
            })
            // 工作集软上限：只提示代价，不拦（拦下来等于替用户决定要不要等）
            .when_some(data_objects::picked_limit_note(picked.len()), |el, note| {
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

impl EventEmitter<BasePanelEvent> for DataObjectsTab {}

impl Focusable for DataObjectsTab {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DataObjectsTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        // 「加源」是面板唯一往外接数据的入口：只接能直接读的文件，库表由连接下推
        Some(vec![Button::new("data-objects-add-source")
            .ghost()
            .label("加源")
            .on_click(cx.listener(|this, _, window, cx| {
                crate::components::data_objects_load::open_load_files_dialog(
                    &this.shared,
                    window,
                    cx,
                );
            }))])
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
