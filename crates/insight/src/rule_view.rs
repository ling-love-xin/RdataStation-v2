//! 规则管理对话框（M8 Phase 2.3）。
//!
//! # 为什么是对话框而不是面板 Tab
//!
//! 三层分组 + 每行状态 + 错误原文需要宽度（280px 的右 Dock 放不下），
//! 且「配置分析器」与「看分析结果」是两类活动——与「项目设置 / 连接对话框」同口径。
//!
//! # 三条职责边界（与面板 `insight_view.rs` 完全一致）
//!
//! 1. **不做 I/O**：本视图只渲染 [`RulesData`]，取数与写库经 `jobs::attach_rules` 走后台
//!    （D20）。因此打开对话框 = 「开窗 + 发一次取数请求」，不是「自己读盘」。
//! 2. **写库只有一条路**：`RuleIndexStore::set_enabled`（索引的唯一用户写入入口），
//!    改完由接缝重新同步 → 注册表缓存失效。视图不碰规则正文，也不删文件。
//! 3. **不持色值**：语义（[`RuleRowStatus`] / 分组）→ `theme.colors.*` 的映射在渲染层。
//!
//! # 与 v1 的关键差异
//!
//! v1 的解析错误只写 `tracing::warn!`——用户在界面上只会看到「规则莫名其妙不见了」。
//! 本视图把 `load_error` 原文逐条摊开，这是规则格式（`deny_unknown_fields` 严格 schema）
//! 唯一的可见出口。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::base::StyledExt;
use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Size, Sizable as _, Theme, WindowExt as _};
use gpui_kit::*;

use crate::rule::RuleScope;
use crate::rule_registry::{get_global_rules_dir, get_project_rules_dir};
use crate::rule_types::RuleMeta;
use crate::service::indexer::{RuleIndexEntry, RuleLoadStatus};
use crate::ui;

/// 分组展示顺序：**覆盖优先级从高到低**（与加载顺序相反）。
///
/// 与加载顺序相反是为了让用户先看到「谁赢」——项目规则压全局、全局压内置，
/// 按加载顺序排版会让人以为内置规则最优先。
pub const GROUP_ORDER: [RuleScope; 3] = [RuleScope::Project, RuleScope::Global, RuleScope::Builtin];

// ==================== 视图模型（纯数据，可脱窗口单测） ====================

/// 规则行的异常状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleRowStatus {
    Ok,
    /// 文件在、解析失败（`error` 是 TOML 报错原文，含行列信息）
    Invalid { error: String },
    /// 索引里有记录、磁盘上文件已不在
    Missing,
}

impl RuleRowStatus {
    pub fn is_broken(&self) -> bool {
        !matches!(self, RuleRowStatus::Ok)
    }
}

/// 一条规则的展示行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRowView {
    pub id: String,
    /// 展示名（索引里的 `name` 为空时退回 `id`，不让用户看空白行）
    pub name: String,
    pub category: String,
    pub version: String,
    /// 当前启停状态（用户决策，落索引表）
    pub enabled: bool,
    pub status: RuleRowStatus,
    /// 可在系统编辑器中打开的文件（内置规则内嵌、文件已丢失时为 `None`）
    pub file: Option<PathBuf>,
}

/// 一个作用域分组
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleGroupView {
    pub scope: RuleScope,
    /// 分组标题（「项目规则」/「全局规则」/「内置规则」）
    pub title: &'static str,
    /// 该层落点说明：目录路径，或内置层的「应用内嵌」
    pub location: String,
    /// 该层目录尚不存在（内置层恒为 false）——界面据此给「新建」入口
    pub dir_missing: bool,
    pub rows: Vec<RuleRowView>,
}

impl RuleGroupView {
    /// 空态提示（按层给不同的话：空的含义不一样）
    pub fn empty_hint(&self) -> &'static str {
        match self.scope {
            RuleScope::Project => "还没有项目规则。项目规则随项目走，可提交到版本库",
            RuleScope::Global => "跨项目复用的规则放在这里，所有项目可见",
            RuleScope::Builtin => "内置规则缺失：安装包不完整",
        }
    }
}

/// 对话框的完整数据（三组 + 底部统计）
///
/// 统计数字取自**未过滤**的全量数据：搜索词不该让「共 20 条」变成「共 2 条」，
/// 那会让人误以为规则被删了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesData {
    pub groups: Vec<RuleGroupView>,
    pub total: usize,
    pub disabled: usize,
    /// 异常条数（解析失败 + 文件丢失）
    pub broken: usize,
}

impl Default for RulesData {
    fn default() -> Self {
        Self {
            groups: GROUP_ORDER.iter().map(|&scope| RuleGroupView {
                scope,
                title: group_title(scope),
                location: String::new(),
                dir_missing: false,
                rows: Vec::new(),
            }).collect(),
            total: 0,
            disabled: 0,
            broken: 0,
        }
    }
}

impl RulesData {
    pub fn group(&self, scope: RuleScope) -> Option<&RuleGroupView> {
        self.groups.iter().find(|g| g.scope == scope)
    }

    /// 可见行数（过滤后调用；底部统计用）。
    pub fn visible_rows(&self) -> usize {
        self.groups.iter().map(|g| g.rows.len()).sum()
    }

    /// 按搜索词过滤（事件路径调用，render 只读结果）。
    ///
    /// 只过滤行、保留分组骨架：空分组仍显示该层的空态与落点路径，
    /// 「搜不到」与「这一层本来就没规则」因此不会混为一谈。
    pub fn filtered(&self, query: &str) -> Self {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return self.clone();
        }
        let mut out = self.clone();
        for group in &mut out.groups {
            group.rows.retain(|row| row_matches(row, &needle));
        }
        out
    }
}

/// 行是否命中搜索词：规则名 / id / 分类三者任一包含即可（大小写不敏感）
fn row_matches(row: &RuleRowView, needle_lower: &str) -> bool {
    row.name.to_lowercase().contains(needle_lower)
        || row.id.to_lowercase().contains(needle_lower)
        || row.category.to_lowercase().contains(needle_lower)
}

fn group_title(scope: RuleScope) -> &'static str {
    match scope {
        RuleScope::Project => "项目规则",
        RuleScope::Global => "全局规则",
        RuleScope::Builtin => "内置规则",
    }
}

// ==================== 取数结果的组装（纯函数） ====================

/// 组装 [`RulesData`] 所需的原始输入（都是取数阶段已拿到的所有权数据）。
#[derive(Debug, Clone, Default)]
pub struct RuleDataInput {
    /// 项目规则目录（无项目时 `None`）
    pub project_dir: Option<PathBuf>,
    /// 全局规则目录（系统目录不可用时 `None`）
    pub global_dir: Option<PathBuf>,
    pub project_rows: Vec<RuleIndexEntry>,
    pub global_rows: Vec<RuleIndexEntry>,
    /// 内置规则元信息（正文内嵌，界面只需要展示字段）
    pub builtin: Vec<RuleMeta>,
}

impl RuleDataInput {
    /// 目录路径直接现算（宿主与索引同步器共用同一份拼装，不各自拼字符串）。
    pub fn with_dirs(project_root: Option<&Path>, global_dir: Option<PathBuf>) -> Self {
        Self {
            project_dir: project_root.map(get_project_rules_dir),
            global_dir,
            ..Default::default()
        }
    }
}

/// 内置层 id 集合：索引里出现这些 id 且没有来源文件时，是**抑制记录**而非规则文件。
fn builtin_id_set(builtin: &[RuleMeta]) -> HashSet<String> {
    builtin.iter().map(|m| m.id.clone()).collect()
}

/// 抑制记录判定：id 属于内置层，且**没有来源文件**。
///
/// `source_path` 为空是唯一的判据——同步器写入的抑制记录只带 id 与状态，
/// 而任何真实规则文件都必然有相对路径。反过来，用户完全可以写一个与内置规则同 id 的
/// 项目规则文件（那就是「覆盖」，不是抑制），这种行的 `source_path` 非空，不会被误判。
fn is_suppression(row: &RuleIndexEntry, builtin_ids: &HashSet<String>) -> bool {
    builtin_ids.contains(&row.rule_id) && row.source_path.is_empty()
}

/// 索引行 → 展示行。`dir` 是该层的规则目录（`source_path` 是相对路径）。
fn row_from_index(row: &RuleIndexEntry, dir: Option<&Path>) -> RuleRowView {
    let status = match row.load_status {
        RuleLoadStatus::Ok => RuleRowStatus::Ok,
        RuleLoadStatus::Invalid => RuleRowStatus::Invalid {
            error: row
                .load_error
                .clone()
                .unwrap_or_else(|| "解析失败（未记录错误详情）".to_string()),
        },
        RuleLoadStatus::Missing => RuleRowStatus::Missing,
    };
    // 文件已不在的报告行不给「打开」入口：点了必然失败，不如把状态说清楚
    let file = match (row.load_status, dir, row.source_path.is_empty()) {
        (RuleLoadStatus::Missing, _, _) | (_, None, _) | (_, _, true) => None,
        (_, Some(dir), false) => Some(dir.join(&row.source_path)),
    };
    RuleRowView {
        id: row.rule_id.clone(),
        name: if row.name.trim().is_empty() {
            row.rule_id.clone()
        } else {
            row.name.clone()
        },
        category: row.category.clone(),
        version: row.version.clone(),
        enabled: row.enabled,
        status,
        file,
    }
}

/// 目录是否存在（取数阶段判一次，界面不做 I/O 判定）。
fn dir_exists(dir: &Option<PathBuf>) -> bool {
    dir.as_ref().is_some_and(|d| d.is_dir())
}

/// 组装三组视图数据。
///
/// 顺序与优先级见 [`GROUP_ORDER`]；每层的 `enabled` 都读自索引，
/// 内置层没有索引行，其启停由**抑制记录**反推（`!suppressed`）。
pub fn build_rules_data(input: RuleDataInput) -> RulesData {
    let builtin_ids = builtin_id_set(&input.builtin);
    let suppressed: HashSet<&str> = input
        .project_rows
        .iter()
        .chain(input.global_rows.iter())
        .filter(|row| is_suppression(row, &builtin_ids) && !row.enabled)
        .map(|row| row.rule_id.as_str())
        .collect();

    let groups: Vec<RuleGroupView> = GROUP_ORDER
        .iter()
        .map(|&scope| {
            let (dir, rows): (&Option<PathBuf>, Vec<RuleRowView>) = match scope {
                RuleScope::Project => (
                    &input.project_dir,
                    input
                        .project_rows
                        .iter()
                        .filter(|row| !is_suppression(row, &builtin_ids))
                        .map(|row| row_from_index(row, input.project_dir.as_deref()))
                        .collect(),
                ),
                RuleScope::Global => (
                    &input.global_dir,
                    input
                        .global_rows
                        .iter()
                        .filter(|row| !is_suppression(row, &builtin_ids))
                        .map(|row| row_from_index(row, input.global_dir.as_deref()))
                        .collect(),
                ),
                RuleScope::Builtin => (
                    &None,
                    input
                        .builtin
                        .iter()
                        .map(|meta| RuleRowView {
                            id: meta.id.clone(),
                            name: if meta.name.trim().is_empty() {
                                meta.id.clone()
                            } else {
                                meta.name.clone()
                            },
                            category: meta.category.clone(),
                            version: meta.version.clone(),
                            enabled: !suppressed.contains(meta.id.as_str()),
                            status: RuleRowStatus::Ok,
                            // 正文内嵌在二进制里，没有可打开的文件
                            file: None,
                        })
                        .collect(),
                ),
            };
            RuleGroupView {
                scope,
                title: group_title(scope),
                location: match scope {
                    RuleScope::Builtin => "应用内嵌".to_string(),
                    _ => dir
                        .as_ref()
                        .map(|d| d.display().to_string())
                        .unwrap_or_else(|| "（不可用）".to_string()),
                },
                dir_missing: !matches!(scope, RuleScope::Builtin) && !dir_exists(dir),
                rows,
            }
        })
        .collect();

    let mut total = 0;
    let mut disabled = 0;
    let mut broken = 0;
    for group in &groups {
        for row in &group.rows {
            total += 1;
            if !row.enabled {
                disabled += 1;
            }
            if row.status.is_broken() {
                broken += 1;
            }
        }
    }

    RulesData {
        groups,
        total,
        disabled,
        broken,
    }
}

/// 该层的规则目录（供接缝建目录用；内置层为 `None`）。
pub fn scope_dir(project_root: Option<&Path>, scope: RuleScope) -> Option<PathBuf> {
    match scope {
        RuleScope::Builtin => None,
        RuleScope::Project => project_root.map(get_project_rules_dir),
        RuleScope::Global => engine::migration::get_system_dir()
            .ok()
            .map(|dir| get_global_rules_dir(&dir)),
    }
}

// ==================== 对话框状态 ====================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesDialogState {
    /// 首次取数中（已有数据时不回退到这个态：开关后的刷新不该让列表闪白）
    Loading,
    Ready,
    Error { message: String },
}

/// 对话框向接缝发出的请求。
///
/// 与面板同口径：视图只发事件，取数 / 写库 / 开进程都在接缝的事件路径上完成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesEvent {
    /// 请取数（首次打开 / ⟳ 重新加载 / 重试）
    ReloadRequested,
    /// 切换某条规则的启停。`scope` 是**规则所在层**，写哪个库由接缝决定
    ToggleRequested {
        rule_id: String,
        scope: RuleScope,
        enabled: bool,
    },
    /// 新建规则：建目录 + 写模板文件（内置层不参与）
    CreateRuleRequested { scope: RuleScope },
    /// 在系统编辑器中打开规则文件
    OpenFileRequested { path: PathBuf },
}

/// 规则管理对话框（实体；状态与渲染，不做 I/O）。
pub struct RulesView {
    state: RulesDialogState,
    /// 最近一次取数结果（全量）
    data: RulesData,
    /// 过滤后的可见数据（事件路径重算，render 只读）
    visible: RulesData,
    query: String,
    /// 一次性提示（写入失败等），下次成功刷新时清掉
    notice: Option<String>,
    /// 三个分组的展开态（顺序同 [`GROUP_ORDER`]；默认全展开——用户来这就是要看规则）
    open_groups: [bool; GROUP_ORDER.len()],
    /// 搜索框（**打开时懒创建**：`InputState::new` 需要窗口，而面板构造期没有窗口）
    search_input: Option<Entity<InputState>>,
    _search_sub: Option<Subscription>,
}

impl EventEmitter<RulesEvent> for RulesView {}

impl RulesView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            state: RulesDialogState::Loading,
            data: RulesData::default(),
            visible: RulesData::default(),
            query: String::new(),
            notice: None,
            open_groups: [true; GROUP_ORDER.len()],
            search_input: None,
            _search_sub: None,
        }
    }

    // ==================== 状态（只由接缝回填 / 自身交互驱动） ====================

    pub fn state(&self) -> &RulesDialogState {
        &self.state
    }

    pub fn data(&self) -> &RulesData {
        &self.data
    }

    pub fn visible(&self) -> &RulesData {
        &self.visible
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn groups_open(&self) -> [bool; GROUP_ORDER.len()] {
        self.open_groups
    }

    /// 取数完成（接缝调用）
    pub fn set_data(&mut self, data: RulesData, cx: &mut Context<Self>) {
        self.data = data;
        self.state = RulesDialogState::Ready;
        self.notice = None;
        self.refresh_visible();
        cx.notify();
    }

    /// 取数失败（接缝调用）：整体错误态——没有数据可看时才是这个态
    pub fn set_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        let message = message.into();
        self.state = RulesDialogState::Error { message };
        cx.notify();
    }

    /// 一次性失败提示（接缝调用）：**列表照旧可见**，只把失败原因挂在状态行上。
    ///
    /// 开关写库失败若整页转错误态，用户会以为规则列表坏了——实际只是这一次没存上。
    pub fn set_notice(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.notice = Some(message.into());
        cx.notify();
    }

    /// 搜索词变更（输入框订阅 / 程序性清空共用）
    pub fn set_query(&mut self, query: impl Into<String>, cx: &mut Context<Self>) {
        let query = query.into();
        if self.query == query {
            return;
        }
        self.query = query;
        self.refresh_visible();
        cx.notify();
    }

    /// 折叠态变更（Accordion 回调给出的是**当前展开的下标列表**）
    pub fn set_open_groups(&mut self, open: &[usize], cx: &mut Context<Self>) {
        let mut next = [false; GROUP_ORDER.len()];
        for &ix in open {
            if let Some(slot) = next.get_mut(ix) {
                *slot = true;
            }
        }
        if self.open_groups != next {
            self.open_groups = next;
            cx.notify();
        }
    }

    fn refresh_visible(&mut self) {
        self.visible = self.data.filtered(&self.query);
    }

    // ==================== 交互（只改状态 + 发事件） ====================

    /// ⚙ 的入口：打开对话框并发一次取数请求。
    ///
    /// 已有数据时不回退到 Loading——重开对话框不该闪白，后台刷新完成后就地替换。
    pub fn begin_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_search_input(window, cx);
        if self.data.total == 0 && !matches!(self.state, RulesDialogState::Error { .. }) {
            self.state = RulesDialogState::Loading;
        }
        cx.emit(RulesEvent::ReloadRequested);
        let weak = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, window, cx| {
            let body: AnyElement = match weak.upgrade() {
                // 实体交给元素树渲染：builder 里不手工 `read`，也就不会有租借纠缠
                Some(view) => view.into_any_element(),
                None => div().into_any_element(),
            };
            let theme = cx.theme();
            // 宽度取主题字号换算（常量保持 rem 倍率，视图不出现裸 px）
            let width = rems(ui::INSIGHT_RULES_DIALOG_WIDTH).to_pixels(window.rem_size());
            dialog
                .title("洞察规则")
                .w(width)
                .child(body)
                .overlay(true)
                .border_color(theme.colors.border)
        });
        cx.notify();
    }

    /// 重试 / ⟳：只发请求，状态由接缝回填
    pub fn request_reload(&mut self, cx: &mut Context<Self>) {
        cx.emit(RulesEvent::ReloadRequested);
    }

    /// 开关：先**就地翻位**（避免等一次后台往返才动的视觉回弹），再发请求；
    /// 写库失败时接缝回填真值（[`Self::set_data`] 会整体替换）。
    pub fn request_toggle(&mut self, scope: RuleScope, rule_id: &str, enabled: bool, cx: &mut Context<Self>) {
        for group in self
            .data
            .groups
            .iter_mut()
            .chain(self.visible.groups.iter_mut())
        {
            if group.scope != scope {
                continue;
            }
            for row in &mut group.rows {
                if row.id == rule_id {
                    row.enabled = enabled;
                }
            }
        }
        cx.emit(RulesEvent::ToggleRequested {
            rule_id: rule_id.to_string(),
            scope,
            enabled,
        });
        cx.notify();
    }

    pub fn request_create_rule(&mut self, scope: RuleScope, cx: &mut Context<Self>) {
        cx.emit(RulesEvent::CreateRuleRequested { scope });
    }

    pub fn request_open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.emit(RulesEvent::OpenFileRequested { path });
    }

    /// 懒创建搜索框与订阅（首次打开对话框时执行一次）。
    ///
    /// 订阅里**不调 `set_query`**：它会把同一个值再写回输入框，正在输入时是无谓的往返
    /// （`InputState::set_value` 不发 `Change` 事件，所以程序性改词仍要走 `set_query`）。
    fn ensure_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_input.is_some() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索规则名 / id / 分类"));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                this.query = emitter.read(cx).value().to_string();
                this.refresh_visible();
                cx.notify();
            },
        );
        self.search_input = Some(input);
        self._search_sub = Some(sub);
    }

    // ==================== 渲染 ====================

    fn render_toolbar(&self, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        let mut bar = div().h_flex().w_full().gap_2();
        if let Some(input) = self.search_input.clone() {
            bar = bar.child(div().flex_1().min_w_0().child(Input::new(&input).cleanable(true)));
        } else {
            bar = bar.child(div().flex_1());
        }
        bar.child(
            Button::new("insight-rule-new")
                .small()
                .icon(IconName::Plus)
                .label("新建项目规则")
                .tooltip("在 .RSmeta/insight-rules/ 建目录与模板文件并打开")
                .on_click({
                    let entity = entity.clone();
                    move |_, _, app| {
                        entity.update(app, |view, cx| {
                            view.request_create_rule(RuleScope::Project, cx)
                        });
                    }
                }),
        )
        .child(
            Button::new("insight-rule-reload")
                .ghost()
                .small()
                .icon(IconName::RotateCw)
                .label("重新加载")
                .tooltip("丢弃缓存并按三层重建规则集（目录监听的兜底与排障入口）")
                .on_click({
                    let entity = entity.clone();
                    move |_, _, app| entity.update(app, |view, cx| view.request_reload(cx))
                }),
        )
        .text_color(colors.foreground)
    }

    /// 三组列表：固定高度 + 内部滚动（对话框高度不因规则条数跳动）
    fn render_groups(&self, entity: &Entity<Self>, theme: &Theme) -> impl IntoElement {
        let colors = theme.colors;
        let mut accordion = Accordion::new("insight-rule-groups")
            .multiple(true)
            .with_size(Size::Small)
            .on_toggle_click({
                let entity = entity.clone();
                move |open, _window, app| {
                    entity.update(app, |view, cx| view.set_open_groups(open, cx));
                }
            });

        for (ix, group) in self.visible.groups.iter().enumerate() {
            let open = self.open_groups.get(ix).copied().unwrap_or(false);
            accordion = accordion.item(|item| {
                item.title(group_title_row(group, theme))
                    .open(open)
                    .children(self.render_group_body(group, entity, theme))
            });
        }

        div()
            .v_flex()
            .w_full()
            .h(rems(ui::DIALOG_TAB_BODY_HEIGHT))
            .min_h_0()
            .overflow_y_scrollbar()
            .child(accordion)
            .text_color(colors.foreground)
    }

    fn render_group_body(
        &self,
        group: &RuleGroupView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Vec<AnyElement> {
        if group.rows.is_empty() {
            return vec![empty_group_hint(group, entity, theme).into_any_element()];
        }
        group
            .rows
            .iter()
            .map(|row| rule_row(group.scope, row, entity, theme).into_any_element())
            .collect()
    }

    fn render_status(&self, theme: &Theme) -> Div {
        let colors = theme.colors;
        let data = &self.data;
        let mut line = format!(
            "共 {} 条 · {} 条禁用",
            data.total, data.disabled
        );
        if data.broken > 0 {
            line.push_str(&format!(" · {} 条异常", data.broken));
        }
        if !self.query.trim().is_empty() {
            line.push_str(&format!(" · 匹配 {} 条", self.visible.visible_rows()));
        }

        let mut row = div()
            .h_flex()
            .w_full()
            .gap_2()
            .pt_1()
            .border_t_1()
            .border_color(colors.border)
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(div().flex_1().min_w_0().text_ellipsis().child(line));
        if let Some(notice) = &self.notice {
            row = row.child(
                div()
                    .flex_none()
                    .text_color(colors.danger)
                    .child(notice.clone()),
            );
        }
        row
    }

    fn render_error(&self, message: &str, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        div()
            .v_flex()
            .w_full()
            .h(rems(ui::DIALOG_TAB_BODY_HEIGHT))
            .min_h_0()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Icon::new(IconName::TriangleAlert)
                    .text_color(colors.warning)
                    .size(rems(ui::INSIGHT_INLINE_ICON_SIZE * 2.0)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.foreground)
                    .child("规则索引读取失败"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(message.to_string()),
            )
            .child(
                Button::new("insight-rule-retry")
                    .small()
                    .label("重试")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            entity.update(app, |view, cx| view.request_reload(cx))
                        }
                    }),
            )
    }
}

impl Render for RulesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let theme = cx.theme();
        let body = match &self.state {
            RulesDialogState::Error { message } => {
                let message = message.clone();
                self.render_error(&message, &entity, theme).into_any_element()
            }
            RulesDialogState::Loading if self.data.total == 0 => div()
                .v_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.colors.muted_foreground)
                        .child("正在读取规则…"),
                )
                .into_any_element(),
            _ => self.render_groups(&entity, theme).into_any_element(),
        };

        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(self.render_toolbar(&entity, theme))
            .child(body)
            .child(self.render_status(theme))
    }
}

// ==================== 片段助手（纯渲染） ====================

fn group_title_row(group: &RuleGroupView, theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .flex_none()
                .text_sm()
                .text_color(colors.foreground)
                .child(group.title),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(colors.muted_foreground)
                .text_ellipsis()
                .child(group.location.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(format!("({})", group.rows.len())),
        )
}

/// 空分组：说明「这层是干什么的」；目录缺失时给「新建」入口（K7：首次写入时创建）
fn empty_group_hint(group: &RuleGroupView, entity: &Entity<RulesView>, theme: &Theme) -> Div {
    let colors = theme.colors;
    let scope = group.scope;
    let mut row = div()
        .h_flex()
        .w_full()
        .gap_2()
        .py_1()
        .text_xs()
        .text_color(colors.muted_foreground)
        .child(div().flex_1().min_w_0().child(group.empty_hint()));
    if group.dir_missing {
        row = row.child(
            Button::new(ElementId::Name(
                format!("insight-rule-new-{}", scope.as_str()).into(),
            ))
            .ghost()
            .xsmall()
            .icon(IconName::Plus)
            .label(if scope == RuleScope::Project {
                "创建并新建规则"
            } else {
                "创建目录并新建规则"
            })
            .tooltip("目录尚不存在；创建后会写一份模板规则并打开")
            .on_click({
                let entity = entity.clone();
                move |_, _, app| {
                    entity.update(app, |view, cx| view.request_create_rule(scope, cx))
                }
            }),
        );
    }
    row
}

/// 一条规则：
/// ```text
/// 名称（类型 / v版本）                  [开] [⧉]
/// ⚠ 第 3 行未知字段 `outputs`（期望 output）
/// ```
fn rule_row(
    scope: RuleScope,
    row: &RuleRowView,
    entity: &Entity<RulesView>,
    theme: &Theme,
) -> Div {
    let colors = theme.colors;
    let status_color = match row.status {
        RuleRowStatus::Ok => colors.foreground,
        RuleRowStatus::Invalid { .. } => colors.danger,
        RuleRowStatus::Missing => colors.warning,
    };

    let meta = {
        let mut parts = Vec::new();
        if !row.category.is_empty() {
            parts.push(row.category.clone());
        }
        if !row.version.trim().is_empty() {
            parts.push(format!("v{}", row.version));
        }
        parts.join(" · ")
    };

    let mut line = div()
        .h_flex()
        .w_full()
        .gap_2()
        .h(rems(ui::DIALOG_ROW_HEIGHT))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(status_color)
                .text_ellipsis()
                .child(row.name.clone()),
        );
    if !meta.is_empty() {
        line = line.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(meta),
        );
    }
    line = line.child(
        Switch::new(ElementId::Name(
            format!("insight-rule-{}-{}", scope.as_str(), row.id).into(),
        ))
        .checked(row.enabled)
        .tooltip(if row.enabled {
            "已启用：参与分析"
        } else {
            "已禁用：写抑制记录，规则文件不动"
        })
        .on_click({
            let entity = entity.clone();
            let rule_id = row.id.clone();
            move |checked, _window, app| {
                entity.update(app, |view, cx| {
                    view.request_toggle(scope, &rule_id, *checked, cx)
                });
            }
        }),
    );
    if let Some(file) = row.file.clone() {
        let tooltip = file.display().to_string();
        line = line.child(
            Button::new(ElementId::Name(
                format!("insight-rule-open-{}-{}", scope.as_str(), row.id).into(),
            ))
            .ghost()
            .xsmall()
            .icon(IconName::ExternalLink)
            .tooltip(SharedString::from(tooltip))
            .on_click({
                let entity = entity.clone();
                move |_, _, app| {
                    entity.update(app, |view, cx| view.request_open_file(file.clone(), cx))
                }
            }),
        );
    }

    let mut body = div().v_flex().w_full().gap_1().child(line);
    match &row.status {
        RuleRowStatus::Ok => {}
        RuleRowStatus::Invalid { error } => {
            body = body.child(broken_line(IconName::TriangleAlert, error, colors.danger, theme));
        }
        RuleRowStatus::Missing => {
            body = body.child(broken_line(
                IconName::Info,
                "规则文件已不在：删掉对应记录或把文件放回来",
                colors.warning,
                theme,
            ));
        }
    }
    body
}

fn broken_line(icon: IconName, text: &str, color: Hsla, _theme: &Theme) -> Div {
    div()
        .h_flex()
        .w_full()
        .gap_1()
        .pl_4()
        .text_xs()
        .text_color(color)
        .child(Icon::new(icon).size_3())
        .child(div().flex_1().min_w_0().child(text.to_string()))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use gpui_kit::component::{Root, WindowExt as _};
    use gpui_kit::{
        AppContext as _, Entity, IntoElement, ParentElement, Render, Styled as _, Subscription,
        TestAppContext, VisualTestContext, Window, div,
    };

    use super::{
        GROUP_ORDER, RuleDataInput, RuleRowStatus, RulesData, RulesEvent, RulesView, build_rules_data,
        row_matches, scope_dir,
    };
    use crate::rule::RuleScope;
    use crate::rule_types::RuleMeta;
    use crate::service::indexer::{RuleIndexEntry, RuleLoadStatus};

    fn meta(id: &str, name: &str) -> RuleMeta {
        RuleMeta {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            version: "1.0".into(),
            category: "column".into(),
            applies_to: vec!["Any".into()],
            builtin: true,
        }
    }

    fn index_row(id: &str, scope: RuleScope, enabled: bool) -> RuleIndexEntry {
        RuleIndexEntry {
            rule_id: id.into(),
            scope,
            category: "column".into(),
            name: format!("{id}-名称"),
            version: "1.2".into(),
            source_path: format!("{id}.rule.toml"),
            checksum: "c".into(),
            enabled,
            load_status: RuleLoadStatus::Ok,
            load_error: None,
        }
    }

    /// 抑制记录：同步器写的只有 id 与状态，没有来源文件
    fn suppression(id: &str, enabled: bool) -> RuleIndexEntry {
        RuleIndexEntry {
            rule_id: id.into(),
            scope: RuleScope::Project,
            category: String::new(),
            name: String::new(),
            version: String::new(),
            source_path: String::new(),
            checksum: String::new(),
            enabled,
            load_status: RuleLoadStatus::Ok,
            load_error: None,
        }
    }

    fn input() -> RuleDataInput {
        RuleDataInput {
            project_dir: Some(PathBuf::from("/p/.RSmeta/insight-rules")),
            global_dir: Some(PathBuf::from("/g/insight-rules")),
            project_rows: vec![index_row("my-rule", RuleScope::Project, true)],
            global_rows: vec![index_row("shared-rule", RuleScope::Global, true)],
            builtin: vec![meta("null-check", "空值检查"), meta("numeric-stats", "数值统计")],
        }
    }

    #[test]
    fn groups_are_ordered_by_override_priority() {
        let data = build_rules_data(input());
        let scopes: Vec<RuleScope> = data.groups.iter().map(|g| g.scope).collect();
        assert_eq!(scopes, GROUP_ORDER.to_vec());
        assert_eq!(
            scopes,
            vec![RuleScope::Project, RuleScope::Global, RuleScope::Builtin],
            "项目在前：先让用户看到「谁赢」"
        );
        assert_eq!(data.groups[0].title, "项目规则");
        assert_eq!(data.groups[2].location, "应用内嵌");
    }

    #[test]
    fn suppression_records_disable_builtin_rules_without_showing_as_project_rules() {
        let mut input = input();
        // 用户在项目里关掉了内置规则 `null-check`
        input.project_rows.push(suppression("null-check", false));

        let data = build_rules_data(input);
        let project = data.group(RuleScope::Project).unwrap();
        assert_eq!(
            project.rows.len(),
            1,
            "抑制记录不是项目规则，不得混进项目分组"
        );
        assert_eq!(project.rows[0].id, "my-rule");

        let builtin = data.group(RuleScope::Builtin).unwrap();
        let null_check = builtin.rows.iter().find(|r| r.id == "null-check").unwrap();
        assert!(!null_check.enabled, "抑制记录应让内置规则显示为已禁用");
        let numeric = builtin.rows.iter().find(|r| r.id == "numeric-stats").unwrap();
        assert!(numeric.enabled);
        assert!(numeric.file.is_none(), "内置规则没有可打开的文件");
        assert_eq!(data.disabled, 1);
    }

    /// 用户完全可以写一个与内置规则**同 id** 的项目规则文件（那就是覆盖，不是抑制）
    #[test]
    fn a_project_file_with_a_builtin_id_is_a_real_rule() {
        let mut input = input();
        input.project_rows.push(index_row("null-check", RuleScope::Project, true));

        let data = build_rules_data(input);
        let project = data.group(RuleScope::Project).unwrap();
        assert_eq!(project.rows.len(), 2, "有来源文件的行就是真规则");
        assert!(
            data.group(RuleScope::Builtin)
                .unwrap()
                .rows
                .iter()
                .all(|r| r.enabled),
            "覆盖 != 禁用"
        );
    }

    #[test]
    fn invalid_and_missing_rows_are_reported_with_their_own_state() {
        let mut input = input();
        input.project_rows.push(RuleIndexEntry {
            load_status: RuleLoadStatus::Invalid,
            load_error: Some("第 3 行未知字段 `outputs`（期望 output）".into()),
            ..index_row("broken", RuleScope::Project, true)
        });
        input.global_rows.push(RuleIndexEntry {
            load_status: RuleLoadStatus::Missing,
            ..index_row("gone", RuleScope::Global, true)
        });

        let data = build_rules_data(input);
        assert_eq!(data.broken, 2);
        let broken = data
            .group(RuleScope::Project)
            .unwrap()
            .rows
            .iter()
            .find(|r| r.id == "broken")
            .unwrap();
        match &broken.status {
            RuleRowStatus::Invalid { error } => {
                assert!(error.contains("未知字段"), "错误原文必须能展示：{error}");
            }
            other => panic!("应为解析失败，实际 {other:?}"),
        }
        assert!(broken.file.is_some(), "解析失败的文件仍在，可以打开修");

        let gone = data
            .group(RuleScope::Global)
            .unwrap()
            .rows
            .iter()
            .find(|r| r.id == "gone")
            .unwrap();
        assert_eq!(gone.status, RuleRowStatus::Missing);
        assert!(gone.file.is_none(), "文件已不在，不给必然失败的开入口");
    }

    #[test]
    fn statistics_count_every_layer() {
        let mut input = input();
        input.project_rows.push(suppression("null-check", false));
        input.project_rows.push(index_row("off", RuleScope::Project, false));
        input.builtin.push(meta("table-rows", "表行数"));

        let data = build_rules_data(input);
        assert_eq!(data.total, 6, "2 项目（my-rule / off）+ 1 全局 + 3 内置");
        assert_eq!(data.disabled, 2, "关掉的内置 + 关掉的项目规则");
        assert_eq!(data.broken, 0);
    }

    #[test]
    fn filtering_matches_name_id_and_category_but_keeps_group_skeleton() {
        let mut input = input();
        input.builtin.push(meta("table-rows", "表行数"));
        let data = build_rules_data(input);

        let hit = data.filtered("数值");
        assert_eq!(hit.group(RuleScope::Builtin).unwrap().rows.len(), 1);
        assert!(hit.group(RuleScope::Project).unwrap().rows.is_empty());
        assert_eq!(
            hit.total, data.total,
            "统计数字取全量：搜索不该让「共 N 条」变小"
        );
        assert_eq!(hit.groups.len(), 3, "分组骨架保留（空层仍显示落点与提示）");

        assert_eq!(data.filtered("  ").groups, data.groups, "空搜索词 = 不过滤");
        assert_eq!(data.filtered("MY-RULE").total, data.total);
        let by_id = data.filtered("table-rows");
        assert_eq!(by_id.visible_rows(), 1, "id 也能搜到");
    }

    #[test]
    fn row_matching_is_case_insensitive() {
        let row = index_row("My-Rule", RuleScope::Project, true);
        let view = build_rules_data(RuleDataInput {
            project_rows: vec![row],
            ..Default::default()
        })
        .groups
        .into_iter()
        .next()
        .unwrap()
        .rows
        .into_iter()
        .next()
        .unwrap();
        assert!(row_matches(&view, "my-rule"));
        assert!(row_matches(&view, "名称"));
        assert!(!row_matches(&view, "不存在"));
    }

    #[test]
    fn layer_dirs_come_from_one_place() {
        // 项目层：{项目}/.RSmeta/insight-rules/；内置层没有目录
        let dir = scope_dir(Some(std::path::Path::new("/p")), RuleScope::Project).unwrap();
        assert!(dir.to_string_lossy().contains(".RSmeta"), "{dir:?}");
        assert!(dir.to_string_lossy().ends_with("insight-rules"), "{dir:?}");
        assert_eq!(scope_dir(Some(std::path::Path::new("/p")), RuleScope::Builtin), None);
        assert_eq!(scope_dir(None, RuleScope::Project), None);
    }

    #[test]
    fn missing_dirs_are_reported_so_the_ui_can_offer_creation() {
        let input = RuleDataInput {
            project_dir: Some(PathBuf::from("/definitely-not-here-rds/x")),
            global_dir: None,
            ..Default::default()
        };
        let data = build_rules_data(input);
        assert!(data.group(RuleScope::Project).unwrap().dir_missing);
        assert!(!data.group(RuleScope::Builtin).unwrap().dir_missing);
        assert_eq!(
            data.group(RuleScope::Global).unwrap().location,
            "（不可用）",
            "系统目录拿不到时要说明，而不是给一个空路径"
        );
    }

    #[test]
    fn empty_data_has_three_empty_groups() {
        let data = RulesData::default();
        assert_eq!(data.groups.len(), 3);
        assert_eq!(data.visible_rows(), 0);
        assert_eq!(data.total, 0);
    }

    // ==================== 实体与窗口 ====================

    /// 记录对话框发出的事件（宿主替身；事件路径的断言都靠它）
    struct Sink {
        events: Rc<RefCell<Vec<RulesEvent>>>,
        _sub: Subscription,
    }

    impl Sink {
        fn attach(rules: &Entity<RulesView>, cx: &mut gpui_kit::Context<Self>) -> Self {
            let events: Rc<RefCell<Vec<RulesEvent>>> = Rc::new(RefCell::new(Vec::new()));
            let sink = events.clone();
            let _sub = cx.subscribe(rules, move |_this, _rules, event: &RulesEvent, _cx| {
                sink.borrow_mut().push(event.clone());
            });
            Self { events, _sub }
        }
    }

    /// 窗口根必须是组件库的 `Root`（`open_dialog` / `render_dialog_layer` 依赖它）
    struct Harness {
        rules: Entity<RulesView>,
        _sink: Entity<Sink>,
    }

    impl Render for Harness {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut gpui_kit::Context<Self>,
        ) -> impl IntoElement {
            let mut root = div().size_full().child(self.rules.clone());
            if let Some(layer) = Root::render_dialog_layer(window, cx) {
                root = root.child(layer);
            }
            root
        }
    }

    fn open_dialog_harness(
        cx: &mut TestAppContext,
    ) -> (Entity<RulesView>, Entity<Sink>, &mut VisualTestContext) {
        // 两个实体都不需要窗口，先建好再包一层 Harness 实体当窗口根
        let rules = cx.new(RulesView::new);
        let sink = cx.new(|cx| Sink::attach(&rules, cx));
        let rules_in = rules.clone();
        let sink_in = sink.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let harness = cx.new(|_cx| Harness {
                rules: rules_in,
                _sink: sink_in,
            });
            Root::new(harness, window, cx)
        });
        (rules, sink, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn sample_data() -> RulesData {
        build_rules_data(RuleDataInput {
            project_dir: Some(PathBuf::from("/p/.RSmeta/insight-rules")),
            global_dir: Some(PathBuf::from("/g/insight-rules")),
            project_rows: vec![index_row("my-rule", RuleScope::Project, true)],
            global_rows: Vec::new(),
            builtin: vec![meta("null-check", "空值检查")],
        })
    }

    /// ⚙ → 弹窗 + 发一次取数请求；数据回填后逐帧可渲染（四态都不 panic）
    #[gpui_kit::test]
    fn dialog_opens_requests_data_and_renders(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (rules, sink, cx) = open_dialog_harness(cx);

        cx.update(|window, cx| {
            rules.update(cx, |view, cx| view.begin_load(window, cx));
        });
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "⚙ 应弹出规则管理对话框"
        );
        assert_eq!(
            sink.read_with(cx, |sink, _| sink.events.borrow().clone()),
            vec![RulesEvent::ReloadRequested],
            "开窗就要发取数请求（视图自己不读盘）"
        );

        // 取数失败 → 错误态可渲染，重试仍是一个事件
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| view.set_error("项目库被占用", cx))
        });
        draw(cx);
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.request_reload(cx)));

        // 取数成功 → 数据态：三组 + 开关 + 状态行
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| view.set_data(sample_data(), cx))
        });
        draw(cx);
        rules.update(cx, |view, _| {
            assert_eq!(view.state(), &super::RulesDialogState::Ready);
            assert_eq!(view.visible().visible_rows(), 2, "1 项目 + 1 内置");
        });

        // 搜索（含清空）→ 事件路径重算可见行
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_query("空值", cx)));
        draw(cx);
        rules.update(cx, |view, _| {
            assert_eq!(view.visible().visible_rows(), 1);
            assert_eq!(view.data().total, 2, "统计仍取全量");
        });
        cx.update(|_window, cx| rules.update(cx, |view, cx| view.set_query("", cx)));
        draw(cx);
        rules.update(cx, |view, _| assert_eq!(view.visible().visible_rows(), 2));

        // 折叠分组：Accordion 回调给的是「当前展开的下标集合」
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| view.set_open_groups(&[1], cx))
        });
        draw(cx);
        rules.update(cx, |view, _| assert_eq!(view.groups_open(), [false, true, false]));
    }

    /// 开关：就地翻位（不等后台往返）+ 发事件；写入失败时列表不消失
    #[gpui_kit::test]
    fn toggling_flips_locally_and_only_emits(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (rules, sink, cx) = open_dialog_harness(cx);
        cx.update(|window, cx| rules.update(cx, |view, cx| view.begin_load(window, cx)));
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| view.set_data(sample_data(), cx))
        });
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| {
                view.request_toggle(RuleScope::Project, "my-rule", false, cx)
            });
        });

        let seen = sink.read_with(cx, |sink, _| sink.events.borrow().clone());
        assert_eq!(
            seen.last(),
            Some(&RulesEvent::ToggleRequested {
                rule_id: "my-rule".into(),
                scope: RuleScope::Project,
                enabled: false,
            }),
            "开关只发事件：写库由接缝在后台做"
        );
        rules.update(cx, |view, _| {
            let row = &view.visible().group(RuleScope::Project).unwrap().rows[0];
            assert!(!row.enabled, "就地翻位：不等后台往返才有即时反馈");
        });

        // 写入失败 → 一次性提示，列表照旧可见
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| view.set_notice("启停未保存：库被占用", cx))
        });
        rules.update(cx, |view, _| {
            assert!(matches!(view.state(), super::RulesDialogState::Ready));
            assert!(view.notice().is_some_and(|n| n.contains("未保存")));
            assert_eq!(view.visible().visible_rows(), 2);
        });
        draw(cx);

        // 下一次成功刷新会清掉提示
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| view.set_data(sample_data(), cx))
        });
        rules.update(cx, |view, _| assert!(view.notice().is_none()));
    }

    /// 新建与打开文件：都只是事件（建目录、拉起进程由接缝做）
    #[gpui_kit::test]
    fn create_and_open_are_requests_not_side_effects(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (rules, sink, cx) = open_dialog_harness(cx);
        cx.update(|_window, cx| {
            rules.update(cx, |view, cx| {
                view.request_create_rule(RuleScope::Project, cx);
                view.request_open_file(
                    PathBuf::from("/p/.RSmeta/insight-rules/my-rule.rule.toml"),
                    cx,
                );
            });
        });
        assert_eq!(
            sink.read_with(cx, |sink, _| sink.events.borrow().clone()),
            vec![
                RulesEvent::CreateRuleRequested {
                    scope: RuleScope::Project
                },
                RulesEvent::OpenFileRequested {
                    path: PathBuf::from("/p/.RSmeta/insight-rules/my-rule.rule.toml")
                },
            ]
        );
    }
}
