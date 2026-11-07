// ==================== 详情视图（中央 tab：字段 + 预览） ====================

// 继承父模块（mock_view）的全部 use 绑定
use super::*;

// 样例预览的固定种子：样例稳定可复现（与最终生成的随机种子无关，纯示意）
use fake::rand::SeedableRng;
use fake::rand::rngs::StdRng;
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::separator::Separator;

mod column_dialog;
mod preview_table;
pub(crate) use preview_table::PreviewTableDelegate;
mod render;
pub(crate) use render::{PreviewSnapshot, render_status_cell};
mod fields;
mod params;
mod column_edit;

const SAMPLE_PREVIEW_SEED: u64 = 20261030;

/// 对话框里的一行参数（标量：单行输入；集合 / 加权：多行文本 + 格式提示）。
enum ParamRowView {
    Scalar {
        label: String,
        state: Entity<InputState>,
    },
    Complex {
        label: String,
        hint: &'static str,
        state: Entity<TextareaState>,
    },
}

/// 参数输入控件（标量走单行 `Input`；集合 / 加权走多行 `Textarea`）。
pub(crate) enum ParamWidget {
    /// 标量参数：单行输入
    Scalar(Entity<InputState>),
    /// 集合 / 加权选项：多行文本（一行一项，格式见 [`parse_complex_param`]）
    Complex(Entity<TextareaState>),
}

/// 生成器参数的输入控件（含订阅句柄：丢掉订阅会立即失效）。
pub(crate) struct ParamInput {
    pub(crate) key: String,
    pub(crate) widget: ParamWidget,
    _sub: Subscription,
}

/// 列编辑对话框的工作副本（「应用」才写回表配置）。
pub(crate) struct ColumnDraft {
    /// 既有列的 id
    id: u64,
    /// 列名输入
    name_input: Entity<InputState>,
    pub(crate) def: ColumnDef,
    /// 打开时的列快照（未应用修改判定：取消 / ESC 丢失防护用）
    original: ColumnDef,
    /// 打开时的参数序列化值（`param_text` / `complex_param_text` 口径，与输入框初值一致）
    original_params: Vec<String>,
    /// 打开时的空值率输入框初值（「50」）
    original_null: String,
    /// 用户是否主动设置过生成器身份（左栏选中 / 恢复智能默认 → true）：
    /// true 后列名失焦不再弹「推荐生成器」提示（用户已有明确选择）
    generator_edited: bool,
    /// 列名失焦时 `ColumnMapper::infer` 给出的推荐生成器配置（None = 不显示提示行）
    recommendation: Option<GeneratorConfig>,
    /// 列名输入的订阅（失焦重推）
    _name_sub: Option<Subscription>,
    pub(crate) confidence: String,
    sample_value: String,
    /// 参数实时样例预览（按当前输入框值生成 2 条；默认第 0 批，固定种子 → 可复现）
    pub(crate) sample_preview: String,
    /// 样例重抽批次：点「重新生成」递增，换一批示意值；默认 0 = 首次可复现的那批
    sample_round: u64,
    pub(crate) params: Vec<ParamInput>,
    /// 上一轮参数所属的生成器（目录名）：换生成器后同名字段一律不沿用旧输入
    /// （`length` / `min` 在新生成器下含义可能完全不同，甚至越界）
    param_generator: Option<String>,
    /// 用户点选的是**哪个自定义生成器**（`UserGenerator::id`；内建为 `None`）。
    /// `def.generator` 只有解包后的基础配置，同一 base 的多个自定义项在它上面完全一样，
    /// 只有这个 id 能说清「左栏亮的是哪一项 / 右栏该显示哪个名字」。
    picked_user: Option<String>,
    null_input: Entity<InputState>,
    type_label: String,
    /// 复杂参数解析失败的原因（键 + 文案）：只在对话框里就地提示，不写回生成器
    pub(crate) param_error: Option<(String, String)>,
}

impl ColumnDraft {
    /// 点「重新生成」：样例批次 +1（下一次 `refresh_sample_preview` 换一批示意值）。
    pub(crate) fn bump_sample_round(&mut self) {
        self.sample_round = self.sample_round.wrapping_add(1);
    }

    /// 正在编辑的列 id（列卡片据此挂「当前」标识条）。
    ///
    /// 走访问器而不是公开字段：`id` 是打开时钉住的，**不可改**（改了等于换一列在编辑，
    /// 而对话框里所有输入都还指着原来那列）。
    ///
    /// 目前只有测试读它（生产侧直接闭包捕获 `id`），故随测试编译存在。
    #[cfg(test)]
    pub(crate) fn column_id(&self) -> u64 {
        self.id
    }

    /// 用户点中的自定义生成器 id（内建 / 未选 → `None`）：左栏精确高亮、右栏显示名字用。
    pub(crate) fn picked_user(&self) -> Option<&str> {
        self.picked_user.as_deref()
    }
}

/// 中央详情 tab 的**身份**：一张结果表一个 tab，另有表配置 tab（列定义可编辑那张）。
///
/// 为何用名字而不是下标做身份：tab 是长命的东西，而 `results` 每次生成都会重建
/// （下标会指向别的表）；名字才是用户看到的那张表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailTarget {
    /// 表配置 tab：某一份表配置（列定义可编辑）+ 该配置目标表的预览。
    /// 带配置 id：每点一次「＋」都是一份新配置、一个自己的 tab（不再共用一个键）
    Config(u64),
    /// 结果表 tab：该表的列（只读）与预览
    Table(String),
}

impl DetailTarget {
    /// 宿主登记 tab 用的键（同一个键只开一个 tab）。
    pub fn key(&self) -> String {
        match self {
            Self::Config(id) => format!("config:{id}"),
            Self::Table(name) => format!("table:{name}"),
        }
    }

    /// 日志 / 测试用的短名。
    pub fn label(&self) -> String {
        match self {
            Self::Config(_) => "表配置".to_string(),
            Self::Table(name) => name.clone(),
        }
    }

    /// 这张 tab 对应哪份表配置（结果表 tab 为 `None`）。
    pub(crate) fn config_id(&self) -> Option<u64> {
        match self {
            Self::Config(id) => Some(*id),
            Self::Table(_) => None,
        }
    }
}

/// 状态栏指示的文案（宿主用；`None` = 不画那一块）。
///
/// 放在 mock crate 而不是宿主：量纲（批次 / 张表）与阶段名跟 `render_job_row` 同源，
/// 宿主自己拼一份会在阶段改名时静默不一致。
///
/// 为何状态栏要有一块（与 D38「状态单点」不冲）：D38 管读数放哪（仍在它自己的中央表头 /
/// 右 Dock 清单下），这里解决的是——面板被切走 / 中央 tab 被关掉时，任务还在跑却看不见。
pub fn status_chip_text(progress: Option<MockJobProgress>) -> Option<String> {
    // 文案本身归 `MockJobProgress::chip_text`：与中央表头 / 清单行的量纲同源，
    // 这里只管「没任务就不画」
    progress.map(MockJobProgress::chip_text)
}

/// Mock 详情（中央编辑区 tab）：字段清单（编辑走对话框）+ 预览表格。
///
/// 状态单一权威：持有配置面板实体，字段与预览都从它读取；编辑动作写回面板。
/// **一个 tab 对应一个目标**（表配置或某张结果表，见 [`DetailTarget`]）：tab 被激活就等于
/// 把那张表设为「当前表」，出口（落库 / 追加 / 导出）跟着它走。
pub struct MockDetailView {
    panel: Entity<MockPanel>,
    /// 这个 tab 管的是哪张表 / 表配置
    target: DetailTarget,
    /// 列编辑对话框的工作副本
    pub(crate) draft: Option<ColumnDraft>,
    focus_handle: FocusHandle,
    /// 所在 Dock tab 组（`Panel::on_added_to` 注入；重复点「查看详情」时用于聚焦自身）
    pub(crate) group: Option<WeakEntity<TabGroup>>,
    /// 预览表状态（组件库 `DataTable` 的 `TableState`）：懒创建（`TableState::new` 要 window），
    /// 数据由面板通知那一拍同步（见 [`MockDetailView::sync_preview_table`]）。
    pub(crate) preview_table: Option<Entity<TableState<PreviewTableDelegate>>>,
    /// 列编辑对话框内的生成器库状态（「选择生成器窗口」左栏：搜索 + 业务/数据双 Tab +
    /// 分组列表；选中写回表配置，点「应用」才落地）：打开列编辑时创建，关闭后随实体丢弃。
    pub(crate) generator_browser:
        Option<Entity<crate::mock_view::generator_library::GeneratorLibraryState>>,
    /// 鼠标正指着哪一张列卡片（卡片操作是悬停才显形的，要重绘就得有状态）。
    ///
    /// 键盘那边**不记状态**：焦点可由 `FocusHandle::is_focused` 当场查到，
    /// 再存一份就要在 focus / blur 回调里维护，还容易与 gpui 的焦点系统对不上。
    hovered_column: Option<u64>,
}

/// 把详情 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// **必须在实体更新之外调用**：`TabGroup` 切 tab 时会回读面板实体（`panel_name` /
/// `focus_handle`），若在 `update_entity(MockDetailView)` 的闭包里调用，gpui 会以
/// `cannot read … while it is already being updated` panic（double lease）。
/// 签名故意只收 `&Entity<Self>` + `&mut App`，从类型上就排除了「在自身 update 里调用」。
pub fn focus_detail_tab(detail: &Entity<MockDetailView>, window: &mut Window, cx: &mut App) {
    let (focus_handle, group) = {
        let view = detail.read(cx);
        (view.focus_handle.clone(), view.group.clone())
    };
    window.focus(&focus_handle, cx);
    let Some(group) = group else {
        // 未加入 Dock（宿主未装配 / 单测直接构造）：静默返回
        return;
    };
    let me = detail.entity_id();
    group
        .update(cx, |group, cx| {
            // 用 `view().entity_id()` 而不是 `panel_name(cx)` 找自己：后者在遍历中会读取
            // 组内每个面板实体（含自身），既多余又容易再踩租借冲突。
            let ix = group
                .panels()
                .iter()
                .position(|p| p.view().entity_id() == me);
            if let Some(ix) = ix {
                group.select_tab(ix, window, cx);
            }
        })
        .ok();
}

impl MockDetailView {
    /// 创建详情视图（持有配置面板实体，状态单一权威）。
    pub fn new(panel: Entity<MockPanel>, target: DetailTarget, cx: &mut Context<Self>) -> Self {
        // 状态变化即重绘：字段/预览跟随配置面板
        cx.observe(&panel, |view, panel, cx| {
            // 取样可能在刚才那一拍变了：推给预览表（事件路径；预览创建前是空操作）
            view.sync_preview_table(&panel, cx);
            cx.notify();
        })
        .detach();
        Self {
            panel,
            target,
            draft: None,
            focus_handle: cx.focus_handle(),
            group: None,
            preview_table: None,
            generator_browser: None,
            hovered_column: None,
        }
    }

    /// 本 tab 看的那张表（表配置 tab = 表配置的目标表；表名为空时没有可看的东西）。
    fn target_table(&self, panel: &MockPanel) -> Option<String> {
        match &self.target {
            DetailTarget::Config(id) => {
                let name = panel.config_for(*id).table_name.clone();
                (!name.is_empty()).then_some(name)
            }
            DetailTarget::Table(name) => Some(name.clone()),
        }
    }

    /// 预览表的取样（列名 + 行）与生效中的排序：与渲染读的是同一处。
    fn preview_snapshot(&self, panel: &MockPanel) -> Option<PreviewSnapshot> {
        let table = self.target_table(panel)?;
        let (preview, sort) = panel.preview_for(&table)?;
        Some(PreviewSnapshot {
            table,
            columns: preview.columns.clone(),
            rows: preview
                .rows
                .iter()
                .take(panel.preview_limit())
                .cloned()
                .collect(),
            sort: sort.map(|sort| (sort.column.clone(), sort.descending)),
        })
    }

    /// 面板通知那一拍：把取样推给预览表（只在真的变了时 `refresh`）。
    ///
    /// 为什么不让 delegate 每帧去面板里读：表格自己会在渲染中多次问 `rows_count` / `render_td`，
    /// 而渲染期不该改别的实体；这里在事件路径上把数据推进去，渲染期就只读。
    fn sync_preview_table(&mut self, panel: &Entity<MockPanel>, cx: &mut Context<Self>) {
        let Some(table) = self.preview_table.clone() else {
            return;
        };
        // 没有结果（切项目 / 还没生成）就不动它：这一帧本来就不渲染这张表
        let Some(snapshot) = self.preview_snapshot(panel.read(cx)) else {
            return;
        };
        table.update(cx, |state, cx| {
            // 行 / 列 / **生效中的排序**变了就重建表头（排序要靠重建 `col_groups` 才能改箭头：
            // 被拒的情形也走这里纠回来）。生成进行中面板每 120ms 通知一次，取样没变就不重建。
            if state.delegate_mut().set_preview(snapshot) {
                state.refresh(cx);
            }
        });
    }

    /// 懒创建预览表状态（`TableState::new` 需要 window；与面板输入框同一处理）。
    fn ensure_preview_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview_table.is_some() {
            return;
        }
        let Some(snapshot) = self.preview_snapshot(self.panel.read(cx)) else {
            return;
        };
        let panel = self.panel.downgrade();
        let state = cx.new(|cx| {
            TableState::new(PreviewTableDelegate::new(panel, snapshot), window, cx)
                // 只读取样：行选 / 列选 / 拖列都不需要打开；列宽保留可拖（默认开）——
                // 这正是手搓版缺的：值被截断时能拖开看全。
                //
                // 排序开着：**表头右端的箭头**点一下 = 按该列重查临时表（不是就地重排取样窗口，
                // 重查在面板里，见 `PreviewTableDelegate::perform_sort`）——与结果集网格同一处入口。
                .col_movable(false)
                .row_selectable(false)
                .col_selectable(false)
        });
        // 列宽是**组件自己**改的（`col_groups` 是它的私有状态），重建表头时只会从 `column()` 重取：
        // 订阅它拖完报一次的那个事件，把宽度记进 delegate，否则排序 / 改行数会把拖好的宽度打回默认档。
        // `emit` 是延迟投递的（pending effect），所以回调里再 update 那个实体是安全的。
        cx.subscribe_in(
            &state,
            window,
            |_view, state, event: &TableEvent, _window, cx| {
                if let TableEvent::ColumnWidthsChanged(widths) = event {
                    let widths = widths.clone();
                    state.update(cx, |state, _cx| state.delegate_mut().set_widths(&widths));
                }
            },
        )
        .detach();
        self.preview_table = Some(state);
    }

    /// `Ctrl+Enter`（key_context = `mock-detail`，键位在 `crates/app` 注册）= 生成。
    ///
    /// 只在**表配置 tab** 上生效：结果表 tab 没有「生成」这个动作（D38——它是这次生成的产物，
    /// 要改就回表配置改完再生成）。进行中静默忽略，与按钮的禁用态同语义
    /// （不为一次按键弹一句「已有任务在进行中」）。
    pub(crate) fn on_generate_key(
        &mut self,
        _: &crate::commands::GenerateMock,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(config_id) = self.config_id() else {
            return;
        };
        self.panel.update(cx, |panel, cx| {
            panel.focus_config(config_id);
            if !panel.is_running() {
                panel.run_generate(cx);
            }
        });
    }

    /// 这个 tab 对应哪份表配置（结果表 tab 为 `None`）
    fn config_id(&self) -> Option<u64> {
        self.target.config_id()
    }

    /// 这个 tab 管的目标（宿主建 tab / 测试用）。
    pub fn target(&self) -> &DetailTarget {
        &self.target
    }

    /// tab 标题：结果表写 `表名（N 行）`，表配置写目标表名（与旧行为一致）。
    ///
    /// 行数实时取自面板结果，所以重新生成 / 「编辑表」改完行数后标题会自己更新。
    pub fn tab_label(&self, cx: &App) -> String {
        let panel = self.panel.read(cx);
        match &self.target {
            DetailTarget::Config(id) => format!("Mock · {}", panel.config_for(*id).table_name),
            DetailTarget::Table(name) => {
                match panel.results().iter().find(|info| &info.table_name == name) {
                    Some(info) => format!(
                        "Mock · {name}（{} 行）",
                        with_thousands(u64::from(info.row_count))
                    ),
                    None => format!("Mock · {name}"),
                }
            }
        }
    }

    /// 配置面板实体（宿主登记 / 测试用）。
    pub fn panel(&self) -> &Entity<MockPanel> {
        &self.panel
    }

    /// 为何不给编辑入口：这张表的列是「这次生成」的产物（模板或表配置的结果），
    /// 就地改列会让「结果」与「产出它的配置」分叉——要改列回表配置 tab 改，再重新生成。
    fn render_result_columns(
        &self,
        info: Option<&MockGenInfo>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // 卡片 / chip 的形态走 `style` 原语，这里只留卡片内文字的两色
        let theme = cx.theme().clone();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;

        let mut block = div().v_flex().gap_2().w_full();
        let Some(info) = info else {
            return block.child(hint_line(
                &theme,
                "这一轮没有这张表的列信息（重新生成后再看）。",
            ));
        };
        // 段标题与表配置 tab 的「列（N）」同款（`section`）：两边是同一类清单，
        // 标题的字号 / 色 / 下边线必须一致，否则切 tab 时视觉要跳一次。
        // 不再画「列（N）· 只读」段标题（与表配置 tab 同口径：段标题只换来一个计数），
        // 只留一句解释「为什么这里不能改」
        block = block.child(
            div()
                .text_xs()
                .text_color(text_hint(&theme))
                .child("它们是这次生成的产物；要改列请回表配置 tab 改完重新生成"),
        );
        for def in info.columns.iter() {
            let mut meta: Vec<String> = Vec::new();
            if def.unique {
                meta.push("唯一".to_string());
            }
            if def.nullable_ratio > 0.0 {
                meta.push(format!(
                    "空值 {}%",
                    (def.nullable_ratio * 100.0).round() as i64
                ));
            }
            let reference = def
                .dependency
                .as_ref()
                .map(|dep| format!("引用 {}.{}", dep.ref_table, dep.ref_column));
            let spec = generator_catalog::spec_of(&def.generator);
            let summary = summarize_params(&def.generator);
            let note = if meta.is_empty() {
                summary.clone()
            } else {
                format!("{} · {}", meta.join(" · "), summary)
            };
            // 与表配置卡片同款的三段卡片（头部：名字 + 类型 chip + 引用；内容：生成器 + 参数摘要），
            // 只读——不给操作图标：就地改会让「结果」与产出它的配置分叉。
            let mut head = div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(card_title(def.name.clone()))
                .child(data_type_tag(&theme, &def.data_type));
            if let Some(text) = &reference {
                head = head.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(text.clone()),
                );
            }
            let body = div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(div().flex_none().text_xs().text_color(fg).child(spec.label))
                .child(card_note(&theme, note));
            block = block.child(
                card(&theme)
                    .child(head)
                    .child(card_divider(&theme))
                    .child(body),
            );
        }
        block
    }
}
// ==================== 中央 tab 的面板协议 ====================

impl EventEmitter<PanelEvent> for MockDetailView {}

impl Focusable for MockDetailView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for MockDetailView {
    fn panel_name(&self) -> &'static str {
        "mock_detail"
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }

    /// tab 被激活 = 它的目标成为面板里的「当前」：出口（落库 / 追加 / 导出）都作用于它。
    ///
    /// 这就是「切 tab 就是切目标」的落地点：用户不必再去面板里选一次。
    /// 结果表 tab 切当前表；表配置 tab 切**活动配置**（右面板的清单行、生成、出口都读活动那份）。
    /// 只在真正的激活事件里做——Dock 会渲染未激活的 tab，在 render 里切会互相抢。
    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if !active {
            return;
        }
        match self.target.clone() {
            DetailTarget::Config(id) => {
                self.panel.update(cx, |panel, cx| {
                    panel.focus_config(id);
                    cx.notify();
                });
            }
            DetailTarget::Table(name) => {
                self.panel
                    .update(cx, |panel, cx| panel.focus_table(&name, cx));
            }
        }
    }
}

impl ComponentPanel for MockDetailView {
    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(self.tab_label(cx).into())
    }

    fn title(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self.tab_label(cx);
        div().text_sm().font_weight(FontWeight::MEDIUM).child(label)
    }
}
