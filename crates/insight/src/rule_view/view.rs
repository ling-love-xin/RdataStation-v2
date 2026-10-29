//! 规则管理对话框 · 对话框视图（自 rule_view.rs 拆分，纯位移）

use super::*;
// ==================== 对话框状态 ====================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesDialogState {
    /// 首次取数中（已有数据时不回退到这个态：开关后的刷新不该让列表闪白）
    Loading,
    Ready,
    Error {
        message: String,
    },
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
    /// 项目规则信任门（Q1 ③ / D53）：用户对「该项目自带规则要不要加载」做出的决定
    TrustDecided { trusted: bool },
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
    /// 打开本对话框时捕获的窗口句柄。
    ///
    /// 首次遇到未信任的项目规则时要在**取数回填那一刻**弹确认框（那一刻没有窗口），
    /// 所以得先把窗口存下来，回填时用 `update_window` 打开（对话框层是栈式的，
    /// 叠在规则对话框上是 kit 支持的行为）。
    window: Option<AnyWindowHandle>,
    /// 本次会话是否已就该项目的未信任规则问过（问过就不再打扰，横幅始终保留入口）
    pub(crate) trust_prompted: bool,
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
            window: None,
            trust_prompted: false,
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
        let first_sight_of_untrusted = data.pending.is_some() && !self.trust_prompted;
        self.data = data;
        self.state = RulesDialogState::Ready;
        self.notice = None;
        self.refresh_visible();
        cx.notify();

        // **首次弹确认**（Q1 ③）：问一次就够，之后再遇到只留横幅
        if first_sight_of_untrusted {
            self.trust_prompted = true;
            self.open_trust_prompt(cx);
        }
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
        // 存下窗口：首次确认框要在「取数回填」那一刻弹（那时没有窗口）
        self.window = Some(window.window_handle());
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

    /// 信任门的回答（横幅按钮 / 确认框共用）：发事件，写库与重装由接缝完成。
    ///
    /// 视图**不自己改 `pending`**：真值只能来自「库里的决定 + 重新装配」——
    /// 就地改会做出一个库与注册表都没跟上的假象。
    pub fn request_trust(&mut self, trusted: bool, cx: &mut Context<Self>) {
        tracing::info!(trusted, "【探针】规则信任门决定（request_trust）");
        self.trust_prompted = true;
        cx.emit(RulesEvent::TrustDecided { trusted });
        cx.notify();
    }

    /// 首次遇到未信任的项目规则：叠一个确认框（决定记住，不在本视图）。
    ///
    /// 取数回填那一刻没有窗口，所以用 `begin_load` 时存下的句柄 `update_window`。
    /// 拿不到窗口（脱窗口单测 / 非常规装配）就不弹——横幅仍在，不会变成静默忽略。
    fn open_trust_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(window_handle) = self.window else {
            return;
        };
        let Some(pending) = self.data.pending.clone() else {
            return;
        };

        let entity = cx.entity();
        let opened = cx.update_window(window_handle, move |_, window, cx| {
            let entity = entity.clone();
            window.open_dialog(cx, move |dialog, _window, cx| {
                let theme = cx.theme();
                let mut body = div()
                    .v_flex()
                    .w_full()
                    .gap_2()
                    .text_sm()
                    .child(
                        div()
                            .text_color(theme.colors.foreground)
                            .child(format!("该项目带了 {} 条规则，是否加载？", pending.count())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(
                                "规则 SQL 会在本机执行。只放行单条只读查询，\
                                 但仍能读到本机能读到的数据——确认来源可信后再加载。",
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("规则：{}", pending.summary())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("目录：{}", pending.dir)),
                    );
                if pending.invalid > 0 {
                    body = body.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.warning)
                            .child(format!("其中 {} 条本身解析失败", pending.invalid)),
                    );
                }
                dialog
                    .title("项目规则：是否加载")
                    .child(body)
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text("信任并加载")
                            .cancel_text("不加载")
                            .show_cancel(true),
                    )
                    .on_ok({
                        let entity = entity.clone();
                        move |_, _window, app| {
                            entity.update(app, |view, cx| view.request_trust(true, cx));
                            true
                        }
                    })
                    .on_cancel({
                        let entity = entity.clone();
                        move |_, _window, app| {
                            entity.update(app, |view, cx| view.request_trust(false, cx));
                            true
                        }
                    })
            });
        });

        if let Err(e) = opened {
            // 弹不出来不是致命错误：横幅仍在（用户仍能做出决定）
            tracing::warn!("[rules] 打开项目规则确认框失败: {e}");
        }
    }

    /// 开关：先**就地翻位**（避免等一次后台往返才动的视觉回弹），再发请求；
    /// 写库失败时接缝回填真值（[`Self::set_data`] 会整体替换）。
    pub fn request_toggle(
        &mut self,
        scope: RuleScope,
        rule_id: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(rule_id = %rule_id, scope = ?scope, enabled, "【探针】规则对话框启用/禁用切换（request_toggle）");
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
        tracing::info!(scope = ?scope, "【探针】规则对话框新建规则（request_create_rule）");
        cx.emit(RulesEvent::CreateRuleRequested { scope });
    }

    pub fn request_open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        tracing::info!(path = %path.display(), "【探针】规则对话框打开规则文件（request_open_file）");
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
            bar = bar.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&input).cleanable(true)),
            );
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
        let mut line = format!("共 {} 条 · {} 条禁用", data.total, data.disabled);
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
                        move |_, _, app| entity.update(app, |view, cx| view.request_reload(cx))
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
                self.render_error(&message, &entity, theme)
                    .into_any_element()
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

        // 信任门横幅在**分组之前**：未信任的项目规则不在下面的分组里
        let banner = self
            .data
            .pending
            .as_ref()
            .map(|pending| pending_banner(pending, &entity, theme));

        let mut root = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(self.render_toolbar(&entity, theme));
        if let Some(banner) = banner {
            root = root.child(banner);
        }
        root.child(body).child(self.render_status(theme))
    }
}

/// 信任门横幅（Q1 ③ / D53）：未信任的项目规则**未装配**，得让用户看得见并做得成决定。
///
/// 放在分组之前而不是塞进项目组里：它决定的是「下面那个项目组会不会有内容」。
fn pending_banner(pending: &PendingRulesView, entity: &Entity<RulesView>, theme: &Theme) -> Div {
    let colors = theme.colors;
    let mut body = div()
        .v_flex()
        .w_full()
        .gap_1()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(colors.warning)
        .child(
            div()
                .text_sm()
                .text_color(colors.foreground)
                .child(if pending.declined {
                    format!(
                        "该项目带了 {} 条规则；你此前选择了不加载。",
                        pending.count()
                    )
                } else {
                    format!(
                        "该项目带了 {} 条规则，尚未加载（未信任）。",
                        pending.count()
                    )
                }),
        )
        .child(div().text_xs().text_color(colors.muted_foreground).child(
            "规则 SQL 会在本机执行——只放行单条只读查询，但仍能读到本机能读到的数据。\
                     确认来源可信后再加载；项目规则随项目走，克隆别人的仓库时尤其要看一眼。",
        ))
        .child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(format!("规则：{}", pending.summary())),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .text_ellipsis()
                .child(format!("目录：{}", pending.dir)),
        );

    if pending.invalid > 0 {
        body = body.child(
            div()
                .text_xs()
                .text_color(colors.warning)
                .child(format!("其中 {} 条本身解析失败", pending.invalid)),
        );
    }

    let mut actions = div().h_flex().w_full().gap_2().child(
        Button::new("insight-rule-trust-accept")
            .primary()
            .xsmall()
            .label(if pending.declined {
                "改为信任并加载"
            } else {
                "信任并加载"
            })
            .on_click({
                let entity = entity.clone();
                move |_, _, app| entity.update(app, |view, cx| view.request_trust(true, cx))
            }),
    );
    if !pending.declined {
        actions = actions.child(
            Button::new("insight-rule-trust-decline")
                .ghost()
                .xsmall()
                .label("不加载")
                .on_click({
                    let entity = entity.clone();
                    move |_, _, app| entity.update(app, |view, cx| view.request_trust(false, cx))
                }),
        );
    }

    body.child(actions)
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
                move |_, _, app| entity.update(app, |view, cx| view.request_create_rule(scope, cx))
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
fn rule_row(scope: RuleScope, row: &RuleRowView, entity: &Entity<RulesView>, theme: &Theme) -> Div {
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
            body = body.child(broken_line(
                IconName::TriangleAlert,
                error,
                colors.danger,
                theme,
            ));
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
